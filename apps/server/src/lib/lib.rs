pub mod api;
pub mod game;
pub mod host;
pub mod net;
pub mod player;
pub mod ws_msg;

use std::{
    collections::HashMap,
    sync::Arc,
    time::{Duration, SystemTime},
};

use anyhow::anyhow;
use axum::{
    Router,
    extract::{
        Path, Query, State, WebSocketUpgrade,
        ws::{Message, Utf8Bytes, WebSocket},
    },
    response::{IntoResponse, Response},
    routing::{any, get, post},
};
pub use game::GameState;
pub use host::HostEntry;
use http::StatusCode;
pub use player::*;
use tokio::sync::Mutex;
use tokio_mpmc::channel;
use tower_http::services::{ServeDir, ServeFile};

use futures::{FutureExt, select};

use crate::{
    api::{
        handlers::{AuthenticatedUser, RoomParams, WsQuery, perform_handshake},
        messages::{GameCommand, GameEvent},
        routes::create_room,
    },
    game::room::Room,
    net::connection::{PlayerEntry, PlayerToken},
};

pub type HeartbeatId = u32;
pub type UnixMs = u64; // # of milliseconds since unix epoch, or delta thereof

pub struct AppState {
    pub room_map: Mutex<HashMap<String, Room>>,
    pub room_ttl: Duration,
}

impl Default for AppState {
    fn default() -> Self {
        Self::new()
    }
}

impl AppState {
    pub fn new() -> Self {
        Self {
            room_map: Mutex::new(HashMap::new()),
            room_ttl: Duration::from_secs(30 * 60),
        }
    }

    pub fn with_ttl(ttl: Duration) -> Self {
        Self {
            room_map: Mutex::new(HashMap::new()),
            room_ttl: ttl,
        }
    }
}

pub fn build_app(state: Arc<AppState>) -> Router {
    let room_routes = Router::new()
        .route("/create", post(create_room))
        .route("/{code}/ws", any(ws_upgrade_handler))
        .route("/{code}/cpr", get(cpr_handler))
        .with_state(state);

    let api_routes = Router::new().nest("/rooms", room_routes);

    Router::new()
        .route("/health", get(|| async { "Server is up" }))
        .nest("/api/v1", api_routes)
        .fallback_service(
            ServeDir::new("public").not_found_service(ServeFile::new("public/index.html")),
        )
}

async fn ws_upgrade_handler(
    State(state): State<Arc<AppState>>,
    ws_upgrade: WebSocketUpgrade,
    Path(rp @ RoomParams { .. }): Path<RoomParams>,
    Query(WsQuery {
        token,
        player_name,
        player_id,
        host_token,
    }): Query<WsQuery>,
) -> Response {
    {
        let room_map = state.room_map.lock().await;
        if !room_map.contains_key(&rp.code) {
            return (StatusCode::NOT_FOUND, "Room does not exist").into_response();
        }
    }
    ws_upgrade.on_upgrade(async move |ws| {
        match ws_socket_handler(
            ws,
            rp,
            state,
            WsQuery {
                token,
                player_name,
                player_id,
                host_token,
            },
        )
        .await
        {
            Ok(()) => {}
            Err(e) => {
                tracing::error!(error = %e, "WebSocket handler failed");
            }
        }
    })
}

async fn send_player_list_to_host(host: &HostEntry, players: &[PlayerEntry]) -> anyhow::Result<()> {
    let list: Vec<Player> = players.iter().map(|entry| entry.player.clone()).collect();
    let msg = GameEvent::PlayerList(list);
    println!("send_player_list_to_host msg: {:?}", &msg);
    host.sender.send(msg).await?;
    Ok(())
}

#[tracing::instrument(
    name = "ws_handler",
    skip(ws, state),
    fields(
        room_code = %code,
        player_id = tracing::field::Empty,
        is_host = tracing::field::Empty
    )
)]
async fn ws_socket_handler(
    mut ws: WebSocket,
    RoomParams { code }: RoomParams,
    state: Arc<AppState>,
    query: WsQuery,
) -> anyhow::Result<()> {
    // for debugging
    let (tx, mut rx): (
        tokio_mpmc::Sender<GameEvent>,
        tokio_mpmc::Receiver<GameEvent>,
    ) = channel(20);
    let auth = {
        let room_map = state.room_map.lock().await;
        let room = room_map
            .get(&code)
            .ok_or(anyhow::anyhow!("Room {} not found", &code))?;
        perform_handshake(room, &query)?
    };
    let player_id = {
        let mut room_map = state.room_map.lock().await;
        let room = room_map
            .get_mut(&code)
            .ok_or(anyhow::anyhow!("Room {} not found", &code))?;

        match auth {
            AuthenticatedUser::Host => {
                room.host = Some(HostEntry::new(0, tx.clone()));
                let player_list =
                    GameEvent::PlayerList(room.players.iter().map(|e| e.player.clone()).collect());
                let _ = tx.send(player_list).await;
                if room.state != GameState::Start {
                    let _ = tx.send(room.build_game_state_msg()).await;
                }
                0 // host pid
            }
            AuthenticatedUser::ExistingPlayer { pid } => {
                let p = room
                    .players
                    .iter_mut()
                    .find(|p| p.player.pid == pid)
                    .ok_or(anyhow::anyhow!("Player {} not found", pid))?;
                p.sender = tx.clone();
                if room.state != GameState::Start {
                    let can_buzz = room.state == GameState::WaitingForBuzz && !p.player.buzzed;
                    let player_state = GameEvent::PlayerState {
                        pid: p.player.pid,
                        buzzed: p.player.buzzed,
                        score: p.player.score,
                        can_buzz,
                    };
                    let _ = tx.send(player_state).await;
                }
                pid
            }
            AuthenticatedUser::NewPlayer { name } => {
                let new_id = (room.players.len() + 1) as u32;
                let token = PlayerToken::generate();
                let player = PlayerEntry::new(
                    Player::new(new_id, name, 0, false, token.clone()),
                    tx.clone(),
                );
                room.players.push(player);

                tx.send(GameEvent::NewPlayer { pid: new_id, token }).await?;
                if let Some(host) = &room.host {
                    let _ = send_player_list_to_host(host, &room.players).await;
                }
                new_id
            }
        }
    };

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
                let msg = match msg {
                    Some(Ok(m)) => m,
                    _ => break,
                };

                let cmd = match msg {
                    Message::Text(text) => {
                        let text_str = text.to_string();
                        match serde_json::from_str::<GameCommand>(&text_str) {
                            Ok(c) => c,
                            Err(e) => {
                                tracing::warn!(
                                    room_code = %code,
                                    player_id = player_id,
                                    error = %e,
                                    "Failed to parse GameCommand"
                                );
                                continue;
                            }
                        }
                    }
                    Message::Ping(data) => {
                        let _ = ws.send(Message::Pong(data)).await;
                        continue;
                    }
                    Message::Pong(_) => continue,
                    Message::Close(_) => break,
                    Message::Binary(_) => {
                        tracing::warn!(room_code = %code, "Unexpected binary message");
                        continue;
                    }
                };


                if let GameCommand::Heartbeat { hbid, .. } = &cmd {
                    let _ = self_tx.send(GameEvent::GotHeartbeat { hbid: *hbid }).await;
                }

                if cmd.should_witness() {
                    let room_map = state.room_map.lock().await;
                    if let Some(room) = room_map.get(&code) {
                        let witness_event = match &cmd {
                            GameCommand::HostReady => {
                                Some(room.build_game_state_msg())
                            }
                            _ => None,
                        };

                        if let Some(event) = witness_event {
                            room.broadcast_witness(event).await;
                        }
                    }
                }

                let response = {
                    let mut room_map = state.room_map.lock().await;
                    if let Some(room) = room_map.get_mut(&code) {
                        let resp = room.handle_command(&cmd, Some(player_id));
                        room.touch();
                        resp
                    } else {
                        return Err(anyhow!("Room lost"));
                    }
                };

                {
                    {
                        let room_map = state.room_map.lock().await;
                        if let Some(room) = room_map.get(&code) {
                            // Send to host
                            if let Some(host) = &room.host {
                                for msg in response.messages_to_host {
                                    let _ = host.sender.send(msg).await;
                                }
                            }

                            // Broadcast to all players
                            for msg in response.messages_to_players {
                                for player in &room.players {
                                    let _ = player.sender.send(msg.clone()).await;
                                }
                            }

                            // Send to specific players (THIS WAS MISSING!)
                            for (pid, msg) in response.messages_to_specific {
                                if let Some(player) = room.players.iter().find(|p| p.player.pid == pid) {
                                    let _ = player.sender.send(msg).await;
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    Ok(())
}

#[tracing::instrument(skip(state), fields(room_code = %rp.code))]
async fn cpr_handler(
    State(state): State<Arc<AppState>>,
    Path(rp @ RoomParams { .. }): Path<RoomParams>,
) -> String {
    let code = rp.code;
    let res = {
        let mut room_map = state.room_map.lock().await;
        let room_res = room_map
            .get_mut(&code)
            .ok_or_else(|| anyhow!("Room {} does not exist", code));
        let mut failures = 0_u32;
        match room_res {
            Err(e) => Err(e),
            Ok(room) => {
                for entry in &mut room.players {
                    match entry.heartbeat().await {
                        Ok(()) => {}
                        Err(e) => {
                            tracing::warn!(
                                player_id = entry.player.pid,
                                error = %e,
                                "Heartbeat failed"
                            );
                            failures += 1;
                        }
                    }
                }
                Ok(format!(
                    "Ok, requested {} heartbeats, {} failed immediately",
                    room.players.len(),
                    failures
                ))
            }
        }
    };
    match res {
        Ok(s) => s,
        Err(e) => {
            tracing::error!(error = %e, "CPR handler failed");
            format!("Err, {e}")
        }
    }
}

#[tracing::instrument(skip(state))]
pub async fn cleanup_inactive_rooms(state: &Arc<AppState>) {
    let mut room_map = state.room_map.lock().await;
    let threshold = SystemTime::now()
        .checked_sub(state.room_ttl)
        .unwrap_or(SystemTime::UNIX_EPOCH);

    let rooms_to_remove: Vec<String> = room_map
        .iter()
        .filter(|(_, room)| room.last_activity < threshold)
        .map(|(code, _)| code.clone())
        .collect();

    if rooms_to_remove.is_empty() {
        tracing::trace!("No inactive rooms to clean up");
    } else {
        for code in &rooms_to_remove {
            room_map.remove(code);
        }
        tracing::info!(count = rooms_to_remove.len(), "Cleaned up inactive rooms");
    }
}
