use std::sync::Arc;

use anyhow::anyhow;
use axum::{
    extract::{
        Path, Query, State, WebSocketUpgrade,
        ws::{Message, Utf8Bytes, WebSocket},
    },
    http::StatusCode,
    response::{IntoResponse, Response},
};
use futures::FutureExt;
use tokio::select;

use crate::{
    AppState,
    api::{
        handlers::{RoomParams, WsQuery},
        messages::{GameCommand, GameEvent},
    },
    game::RoomResponse,
    net::ws::session::setup_session,
};

pub async fn ws_upgrade_handler(
    State(state): State<Arc<AppState>>,
    ws_upgrade: WebSocketUpgrade,
    Path(rp @ RoomParams { .. }): Path<RoomParams>,
    Query(query): Query<WsQuery>,
) -> Response {
    tracing::info!("upgrading ws");
    {
        let room_map = state.room_map.lock().await;
        if !room_map.contains_key(&rp.code) {
            return (StatusCode::NOT_FOUND, "Room does not exist").into_response();
        }
    }
    ws_upgrade.on_upgrade(
        async move |ws| match ws_socket_handler(ws, rp, state, query).await {
            Ok(()) => {}
            Err(e) => {
                tracing::error!(error = %e, "WebSocket handler failed");
            }
        },
    )
}

/// Main WebSocket connection handler
#[tracing::instrument(
    name = "ws_handler",
    skip(ws, state),
    fields(
        room_code = %code,
        player_id = tracing::field::Empty,
        is_host = tracing::field::Empty
    )
)]
pub async fn ws_socket_handler(
    mut ws: WebSocket,
    RoomParams { code }: RoomParams,
    state: Arc<AppState>,
    query: WsQuery,
) -> anyhow::Result<()> {
    let (tx, rx) = tokio_mpmc::channel(20);

    tracing::info!("setting up session");
    let player_id = setup_session(&state, &code, &query, tx.clone()).await?;

    let self_tx = tx.clone();

    loop {
        select! {
            res = rx.recv().fuse() => {
                match res {
                    Ok(Some(msg)) => {
                        let text = serde_json::to_string(&msg)?;
                        ws.send(Message::Text(Utf8Bytes::from(text))).await?;
                    }
                    _ => break, // Channel closed, exit loop
                }
            },
            msg = ws.recv().fuse() => {
                tracing::info!("WebSocket handler message {:?}", msg);
                let msg = match msg {
                    Some(Ok(m)) => m,
                    _ => break,
                };

                let cmd = match parse_message(msg, &code, player_id, &mut ws).await? {
                    Some(c) => c,
                    None => continue,
                };

                if let GameCommand::Heartbeat { hbid, .. } = &cmd {
                    let _ = self_tx.send(GameEvent::GotHeartbeat { hbid: *hbid }).await;
                }

                if cmd.should_witness() {
                    handle_witness(&state, &code, &cmd, player_id).await;
                }

                let response = {
                    let mut room_map = state.room_map.lock().await;
                    let room = room_map
                        .get_mut(&code)
                        .ok_or(anyhow!("Room lost"))?;
                    let resp = room.handle_command(&cmd, Some(player_id));
                    room.touch();
                    resp
                };

                dispatch_responses(&state, &code, response).await;
            }
        }
    }
    tracing::info!("WebSocket handler ending normally for player {}", player_id);
    Ok(())
}

/// Parse a WebSocket message into a GameCommand
async fn parse_message(
    msg: Message,
    code: &str,
    player_id: u32,
    ws: &mut WebSocket,
) -> anyhow::Result<Option<GameCommand>> {
    match msg {
        Message::Text(text) => {
            let text_str = text.to_string();
            match serde_json::from_str::<GameCommand>(&text_str) {
                Ok(c) => Ok(Some(c)),
                Err(e) => {
                    tracing::warn!(
                        room_code = %code,
                        player_id = player_id,
                        error = %e,
                        "Failed to parse GameCommand"
                    );
                    Ok(None)
                }
            }
        }
        Message::Ping(data) => {
            let _ = ws.send(Message::Pong(data)).await;
            Ok(None)
        }
        Message::Pong(_) => Ok(None),
        Message::Close(_) => Err(anyhow!("Connection closed")),
        Message::Binary(_) => {
            tracing::warn!(room_code = %code, "Unexpected binary message");
            Ok(None)
        }
    }
}

/// Hanldes witness events for time-critical synchronization.
async fn handle_witness(state: &Arc<AppState>, code: &str, cmd: &GameCommand, _player_id: u32) {
    let room_map = state.room_map.lock().await;
    if let Some(room) = room_map.get(code) {
        let witness_event = match cmd {
            GameCommand::HostReady => Some(room.build_game_state_msg()),
            _ => None,
        };

        if let Some(event) = witness_event {
            room.broadcast_witness(event).await;
        }
    }
}

/// Dispatches response messages to appropriate recipients.
async fn dispatch_responses(state: &Arc<AppState>, code: &str, response: RoomResponse) {
    tracing::debug!(
        "Dispatching responses: {} to host, {} broadcast, {} specific",
        response.messages_to_host.len(),
        response.messages_to_players.len(),
        response.messages_to_specific.len()
    );

    let room_map = state.room_map.lock().await;
    if let Some(room) = room_map.get(code) {
        if let Some(host) = &room.host {
            tracing::debug!(
                "Sending {} messages to host",
                response.messages_to_host.len()
            );
            for msg in response.messages_to_host {
                let _ = host.sender.send(msg).await;
            }
        }

        for msg in response.messages_to_players {
            for player in &room.players {
                let _ = player.sender.send(msg.clone()).await;
            }
        }

        for (pid, msg) in response.messages_to_specific {
            if let Some(player) = room.players.iter().find(|p| p.player.pid == pid) {
                let _ = player.sender.send(msg).await;
            }
        }
    }
}
