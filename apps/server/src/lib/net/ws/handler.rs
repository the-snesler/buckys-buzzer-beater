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
use tokio::{select, sync::mpsc};

use crate::{
    AppState,
    api::{
        handlers::{RoomParams, WsQuery},
        messages::{GameCommand, GameEvent, Sender},
    },
    game::{RoomResponse, room::BUZZ_WINDOW},
    net::{
        connection::{EVENT_QUEUE_CAPACITY, send_event},
        ws::session::setup_session,
    },
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
    let (tx, mut rx) = mpsc::channel(EVENT_QUEUE_CAPACITY);

    tracing::info!("setting up session");
    let sender = setup_session(&state, &code, &query, tx.clone()).await?;

    let self_tx = tx.clone();

    loop {
        select! {
            res = rx.recv().fuse() => {
                match res {
                    Some(msg) => {
                        let text = serde_json::to_string(&msg)?;
                        ws.send(Message::Text(Utf8Bytes::from(text))).await?;
                    }
                    None => break, // Channel closed, exit loop
                }
            },
            msg = ws.recv().fuse() => {
                tracing::info!("WebSocket handler message {:?}", msg);
                let msg = match msg {
                    Some(Ok(m)) => m,
                    _ => break,
                };

                let cmd = match parse_message(msg, &code, sender, &mut ws).await? {
                    Some(c) => c,
                    None => continue,
                };

                if let GameCommand::Heartbeat { hbid, .. } = &cmd {
                    send_event(&self_tx, GameEvent::GotHeartbeat { hbid: *hbid });
                }

                let response = {
                    let mut room_map = state.room_map.lock().await;
                    let room = room_map
                        .get_mut(&code)
                        .ok_or(anyhow!("Room lost"))?;
                    let resp = room.handle_command(&cmd, sender);
                    room.touch();
                    resp
                };

                if let Some(window_id) = response.buzz_window_opened {
                    let state = state.clone();
                    let code = code.clone();
                    tokio::spawn(async move {
                        tokio::time::sleep(BUZZ_WINDOW).await;
                        let response = {
                            let mut room_map = state.room_map.lock().await;
                            let Some(room) = room_map.get_mut(&code) else {
                                return;
                            };
                            room.resolve_buzz_window(window_id)
                        };
                        dispatch_responses(&state, &code, response).await;
                    });
                }

                dispatch_responses(&state, &code, response).await;
            }
        }
    }
    tracing::info!(?sender, "WebSocket handler ending normally");
    Ok(())
}

/// Parse a WebSocket message into a GameCommand
async fn parse_message(
    msg: Message,
    code: &str,
    sender: Sender,
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
                        ?sender,
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

/// Dispatches response messages to appropriate recipients. Sending never waits, so a
/// client that stops reading can't hold up the room lock.
async fn dispatch_responses(state: &Arc<AppState>, code: &str, response: RoomResponse) {
    tracing::debug!(
        "Dispatching responses: {} to host, {} broadcast, {} specific",
        response.messages_to_host.len(),
        response.messages_to_players.len(),
        response.messages_to_specific.len()
    );

    let room_map = state.room_map.lock().await;
    if let Some(room) = room_map.get(code) {
        if let Some(event) = response.witness {
            room.broadcast_witness(event);
        }

        if let Some(host) = &room.host {
            tracing::debug!(
                "Sending {} messages to host",
                response.messages_to_host.len()
            );
            for msg in response.messages_to_host {
                send_event(&host.sender, msg);
            }
        }

        for msg in response.messages_to_players {
            for player in &room.players {
                send_event(&player.sender, msg.clone());
            }
        }

        for (pid, msg) in response.messages_to_specific {
            if let Some(player) = room.players.iter().find(|p| p.player.pid == pid) {
                send_event(&player.sender, msg);
            }
        }
    }
}
