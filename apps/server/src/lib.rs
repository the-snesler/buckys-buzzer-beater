pub mod game;
pub mod host;
pub mod player;
pub mod ws_msg;

use std::{
    collections::HashMap,
    sync::Arc,
    time::{Duration, SystemTime},
};

use anyhow::anyhow;
use axum::{
    Json, Router,
    extract::{
        Path, Query, State, WebSocketUpgrade,
        ws::{Message, Utf8Bytes, WebSocket},
    },
    response::{IntoResponse, Response},
    routing::{any, get, post},
};
pub use game::{GameState, Room};
pub use host::HostEntry;
use http::StatusCode;
pub use player::*;
use rand::Rng;
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;
use tokio_mpmc::channel;
use tower_http::services::{ServeDir, ServeFile};

use futures::{FutureExt, select};

use crate::ws_msg::WsMsg;

pub type HeartbeatId = u32;
pub type UnixMs = u64; // # of milliseconds since unix epoch, or delta thereof

#[derive(Deserialize)]
struct WsQuery {
    #[serde(rename = "playerName")]
    player_name: Option<String>, // only players include player_name
    token: Option<String>, // only rejoining players include both token & player_id
    #[serde(rename = "playerID")]
    player_id: Option<u32>,
}

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

fn generate_room_code() -> String {
    const CHARSET: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZ";
    let mut rng = rand::rng();
    (0..6)
        .map(|_| {
            let idx = rng.random_range(0..CHARSET.len());
            CHARSET[idx] as char
        })
        .collect()
}

fn generate_host_token() -> String {
    const CHARSET: &[u8] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
    let mut rng = rand::rng();
    (0..32)
        .map(|_| {
            let idx = rng.random_range(0..CHARSET.len());
            CHARSET[idx] as char
        })
        .collect()
}

fn generate_player_token() -> String {
    const CHARSET: &[u8] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
    let mut rng = rand::rng();
    (0..32)
        .map(|_| {
            let idx = rng.random_range(0..CHARSET.len());
            CHARSET[idx] as char
        })
        .collect()
}

async fn create_room(
    State(state): State<Arc<AppState>>,
    Json(body): Json<CreateRoomRequest>,
) -> (StatusCode, Json<CreateRoomResponse>) {
    let mut room_map = state.room_map.lock().await;

    // Generate a unique room code
    let code = loop {
        let candidate = generate_room_code();
        if !room_map.contains_key(&candidate) {
            break candidate;
        }
    };

    let host_token = generate_host_token();
    let mut room = Room::new(code.clone(), host_token.clone());

    if let Some(categories) = body.categories {
        room.categories = categories;
    }

    room_map.insert(code.clone(), room);

    tracing::info!(room_code = %code, "Room created");

    (
        StatusCode::CREATED,
        Json(CreateRoomResponse {
            room_code: code,
            host_token,
        }),
    )
}

#[derive(Serialize)]
struct CreateRoomResponse {
    room_code: String,
    host_token: String,
}

#[derive(Deserialize)]
struct CreateRoomRequest {
    categories: Option<Vec<game::Category>>,
}

#[derive(Debug)]
pub enum ConnectionStatus {
    Connected,
    Disconnected,
}
#[derive(Serialize, Deserialize)]
struct RoomParams {
    code: String,
}

async fn ws_upgrade_handler(
    State(state): State<Arc<AppState>>,
    ws_upgrade: WebSocketUpgrade,
    Path(rp @ RoomParams { .. }): Path<RoomParams>,
    Query(WsQuery {
        token,
        player_name,
        player_id,
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
    let msg = WsMsg::PlayerList(list);
    println!("send_player_list_to_host msg: {:?}", &msg);
    host.sender.send(msg).await?;
    Ok(())
}

async fn ws_socket_handler(
    mut ws: WebSocket,
    RoomParams { code }: RoomParams,
    state: Arc<AppState>,
    WsQuery {
        player_name,
        token,
        player_id,
    }: WsQuery,
) -> anyhow::Result<()> {
    // for debugging
    tracing::debug!(
    room_code = %code,
        ?token,
        ?player_name,
        ?player_id,
        "WebSocket connection attempt"
    );
    let ch: tokio_mpmc::Receiver<WsMsg>;
    let tx: tokio_mpmc::Sender<WsMsg>;
    (tx, ch) = channel(20);
    let mut connection_player_id: Option<u32> = player_id;
    let tx_internal = tx.clone();
    {
        let mut room_map = state.room_map.lock().await;
        let room = room_map
            .get_mut(&code)
            .ok_or_else(|| anyhow!("Room {} does not exist", code))?;
        // println!("room: {:?}", room);

        let is_host = token.as_ref() == Some(&room.host_token);

        if is_host {
            let host = HostEntry::new(player_id.unwrap_or(0), tx.clone());
            send_player_list_to_host(&host, &room.players).await?;

            tracing::info!(room_code = %code, "Host connected");

            if room.state != GameState::Start {
                let players: Vec<Player> = room.players.iter().map(|e| e.player.clone()).collect();
                let game_state_msg = WsMsg::GameState {
                    state: room.state.clone(),
                    categories: room.categories.clone(),
                    players,
                    current_question: room.current_question,
                    current_buzzer: room.current_buzzer,
                    winner: None,
                };
                tx.send(game_state_msg).await?;
                tracing::debug!(room_code = %code, state = ?room.state, "Sending game state to reconnecting host");
            }

            room.host = Some(host);
        } else if let (Some(id), Some(_tok)) = (player_id, &token) {
            if let Some(existing) = room.players.iter_mut().find(|p| p.player.pid == id) {
                // Update existing player's send channel
                existing.sender = tx.clone();

                tracing::info!(room_code = %code, player_id = id, "Player reconnected");

                let can_buzz = room.state == GameState::WaitingForBuzz;
                let player_state_msg = WsMsg::PlayerState {
                    pid: existing.player.pid,
                    buzzed: existing.player.buzzed,
                    score: existing.player.score,
                    can_buzz,
                };
                tx.send(player_state_msg).await?;
            } else {
                return Err(anyhow!(
                    "Player with ID {} could not be found in room {}",
                    id,
                    code
                ));
            }
            if let Some(host) = &room.host {
                send_player_list_to_host(host, &room.players).await?;
            }
        } else if let Some(name) = player_name {
            let new_id: u32 = (room.players.len() + 1).try_into()?;
            connection_player_id = Some(new_id);
            let player_token = generate_player_token();
            let player = PlayerEntry::new(
                Player::new(new_id, name.clone(), 0, false, player_token.clone()),
                tx.clone(),
            );
            room.players.push(player);

            tracing::info!(room_code = %code, player_id = new_id, player_name = %name, "Player joined");

            let new_player_msg = WsMsg::NewPlayer {
                pid: new_id,
                token: player_token,
            };
            tx.send(new_player_msg).await?;

            if let Some(host) = &room.host {
                send_player_list_to_host(host, &room.players).await?;
            }
        } else if let Some(tok) = &token {
            if let Some(existing) = room.players.iter_mut().find(|p| p.player.token == *tok) {
                connection_player_id = Some(existing.player.pid);
                existing.sender = tx.clone();

                let can_buzz = room.state == GameState::WaitingForBuzz;
                let player_state_msg = WsMsg::PlayerState {
                    pid: existing.player.pid,
                    buzzed: existing.player.buzzed,
                    score: existing.player.score,
                    can_buzz,
                };

                tx.send(player_state_msg).await?;
            } else {
                return Err(anyhow!("Invalid player token"));
            }
        } else {
            // Invalid connection
            return Err(anyhow!(
                "Invalid connection: must provide player_name (new player) or token (reconnect)"
            ));
        }
        //
        // for player in &room.players {
        //     println!("player: {}", player.player.pid);
        // }
    }
    loop {
        select! {
            res = ch.recv().fuse() => match res {
                Ok(recv) => {
                    let ser = serde_json::to_string(&recv)?;
                    if let Some(r) = &recv {
                        match &r {
                            WsMsg::GameState { state, .. } => tracing::debug!(room_code = %code, ?state, "Sending GameState"),
                            other => tracing::trace!(room_code = %code, "Sending message: {:?}", other),
                        }
                    }
                    ws.send(Message::Text(Utf8Bytes::from(ser))).await?;
                },
                Err(e) => Err(e)?
            },
            msg_opt = ws.recv().fuse() => match msg_opt {
                None => break,
                Some(msg) => {
                    let msg = if let Ok(msg) = msg {
                        msg
                    } else {
                        // client disconnected
                        Err(std::io::Error::new(
                            std::io::ErrorKind::HostUnreachable,
                            "websocket client disconnected in read",
                        ))?
                    };
                    let msg: String = msg.into_text()?.to_string();
                    // deser
                    let msg: WsMsg = serde_json::from_str(&msg)?;
                    // witness case, just for now
                    if let m @ (WsMsg::StartGame {}
                        | WsMsg::EndGame {}
                        | WsMsg::BuzzEnable {}
                        | WsMsg::BuzzDisable {}
                        | WsMsg::Buzz {}) = msg.clone() {
                        let witness = WsMsg::Witness { msg: Box::new(m) };
                        let player_info: Vec<(u32, tokio_mpmc::Sender<WsMsg>, u64)> = {
                            let room_map = state.room_map.lock().await;
                            let room = room_map
                                .get(&code)
                                .ok_or_else(|| anyhow!("Room {} does not exist", code))?;
                            room.players
                                .iter()
                                .map(|p| (p.player.pid, p.sender.clone(), p.latency().unwrap_or(0).into()))
                                .collect()
                        };
                        let sender_player_id = connection_player_id;
                        for (cpid, csender, lat) in player_info {
                            let witnessc = witness.clone();
                            let latc = lat;
                            tokio::spawn(async move {
                                if let Some(id) = sender_player_id
                                    && cpid == id {
                                        return Ok(());
                                    }
                                let s = csender;
                                tokio::time::sleep(Duration::from_millis(500_u64.saturating_sub(latc))).await;
                                s.send(witnessc).await
                            });
                        }
                    };
                    // heartbeat case
                    if let WsMsg::Heartbeat { hbid, .. } = msg.clone() {
                        tx_internal.send(WsMsg::GotHeartbeat { hbid }).await?;
                        //continue;
                    }
                    // everything else
                    let mut room_map = state.room_map.lock().await;
                    let room = room_map
                        .get_mut(&code)
                        .ok_or_else(|| anyhow!("Room {} does not exist", code))?;
                    room.update(&msg, connection_player_id).await?;
                    room.touch();
                }
            }
        }
    }
    tracing::info!(room_code = %code, ?connection_player_id, "WebSocket connection closed");
    Ok(())
}

//#[debug_handler]
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
            println!("cpr_handler failure, did not panic: {e}");
            format!("Err, {e}")
        }
    }
}

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
