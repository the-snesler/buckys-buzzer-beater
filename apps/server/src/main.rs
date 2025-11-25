use std::{
    collections::HashMap,
    sync::Arc,
    thread,
    time::{Duration, SystemTime},
};

use anyhow::anyhow;
use axum::{
    Json, Router,
    extract::{
        Path, Query, State,
        ws::{Message, Utf8Bytes, WebSocket, WebSocketUpgrade},
    },
    response::{IntoResponse, Response},
    routing::{any, get, post},
};
use axum_macros::debug_handler;
use tower_http::services::{ServeDir, ServeFile};

use futures::{FutureExt, select};
use http::StatusCode;
use rand::Rng;
use serde::{Deserialize, Serialize};

use tokio::sync::Mutex;
use tokio_mpmc::channel;

use crate::{
    game::{GameState, Room},
    host::HostEntry,
    player::*,
    ws_msg::WsMsg,
};

mod game;
mod game_file;
mod host;
mod player;
mod ws_msg;

const ROOM_TTL_MINUTES: u64 = 30;

struct AppState {
    room_map: Mutex<HashMap<String, Room>>,
}

impl AppState {
    pub fn new() -> Self {
        Self {
            room_map: Mutex::new(HashMap::new()),
        }
    }
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

#[derive(Serialize)]
struct CreateRoomResponse {
    room_code: String,
    host_token: String,
}

#[derive(Deserialize)]
struct CreateRoomRequest {
    categories: Option<Vec<game::Category>>,
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

    (
        StatusCode::CREATED,
        Json(CreateRoomResponse {
            room_code: code,
            host_token,
        }),
    )
}

#[derive(Debug)]
enum ConnectionStatus {
    Connected,
    Disconnected,
}
#[derive(Serialize, Deserialize)]
struct RoomParams {
    code: String,
}

#[derive(Deserialize)]
struct WsQuery {
    #[serde(rename = "playerName")]
    player_name: Option<String>, // only players include player_name
    token: Option<String>, // only rejoining players include both token & player_id
    #[serde(rename = "playerID")]
    player_id: Option<u32>,
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
                println!("WebSocket handler failed (died but didn't panic): {e}");
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
    println!("{:?} {:?} {:?} {:?}", code, token, player_name, player_id);
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
        println!("room: {:?}", room);

        let is_host = token.as_ref() == Some(&room.host_token);

        if is_host {
            let host = HostEntry::new(player_id.unwrap_or(0), tx.clone());
            send_player_list_to_host(&host, &room.players).await?;

            if room.state != GameState::Start {
                let players: Vec<Player> = room.players.iter().map(|e| e.player.clone()).collect();
                let game_state_msg = WsMsg::GameState {
                    state: room.state.clone(),
                    categories: room.categories.clone(),
                    players,
                    current_question: room.current_question,
                    current_buzzer: room.current_buzzer,
                    winner: room.winner,
                };
                tx.send(game_state_msg).await?;
            }

            room.host = Some(host);
        } else if let (Some(id), Some(_tok)) = (player_id, &token) {
            if let Some(existing) = room.players.iter_mut().find(|p| p.player.pid == id) {
                // Update existing player's send channel
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
            let new_id: u32 = (room.players.len() + 1).try_into().unwrap();
            connection_player_id = Some(new_id);
            let player_token = generate_player_token();
            let player = PlayerEntry::new(
                Player::new(new_id, name, 0, false, player_token.clone()),
                tx.clone(),
            );
            room.players.push(player);

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

        room.touch();

        for player in &room.players {
            println!("player: {}", player.player.pid);
        }
    }
    loop {
        select! {
            res = ch.recv().fuse() => match res {
                Ok(recv) => {
                    let ser = serde_json::to_string(&recv)?;
                    if let Some(r) = &recv {
                        match &r {
                            WsMsg::GameState { state, .. } => println!("sending GameState: {:?}", state),
                            other => println!("sending {:?}", other),
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
                                .map(|p| (p.player.pid, p.sender.clone(), p.latency().into()))
                                .collect()
                        };
                        let sender_player_id = connection_player_id;
                        for (cpid, csender, lat) in player_info {
                            let witnessc = witness.clone();
                            let latc = lat.clone();
                            tokio::spawn(async move {
                                if let Some(id) = sender_player_id {
                                    if cpid == id {
                                        return Ok(());
                                    }
                                }
                                let s = csender;
                                tokio::time::sleep(Duration::from_millis(500_u64 .saturating_sub(latc))).await;
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
                }
            }
        }
    }
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
                            println!("cpr_handler heartbeat failure, did not panic: {e}");
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

async fn cleanup_inactive_rooms(state: &Arc<AppState>) {
    let mut room_map = state.room_map.lock().await;
    let threshold = SystemTime::now()
        .checked_sub(Duration::from_secs(ROOM_TTL_MINUTES * 60))
        .unwrap_or(SystemTime::UNIX_EPOCH);

    let rooms_to_remove: Vec<String> = room_map
        .iter()
        .filter(|(_, room)| room.last_activity < threshold)
        .map(|(code, _)| code.clone())
        .collect();

    for code in &rooms_to_remove {
        room_map.remove(code);
        println!("Cleaned up inactive room: {}", code);
    }

    if !rooms_to_remove.is_empty() {
        println!("Cleaned up {} inactive rooms", rooms_to_remove.len());
    }
}

const HOST: &str = "0.0.0.0";
const PORT: u16 = 3000;

#[tokio::main]
async fn main() {
    let state = Arc::new(AppState::new());

    let cleanup_state = state.clone();

    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(60));
        loop {
            interval.tick().await;
            cleanup_inactive_rooms(&cleanup_state).await;
        }
    });

    let room_routes = Router::new()
        .route("/create", post(create_room))
        .route("/{code}/ws", any(ws_upgrade_handler))
        .route("/{code}/cpr", get(cpr_handler))
        .with_state(state);

    let api_routes = Router::new().nest("/rooms", room_routes);

    let app = Router::new()
        .route("/health", get(|| async { "Server is up" }))
        .nest("/api/v1", api_routes)
        .fallback_service(
            ServeDir::new("public").not_found_service(ServeFile::new("public/index.html")),
        );

    let listener = tokio::net::TcpListener::bind(format!("{}:{}", HOST, PORT))
        .await
        .unwrap();
    println!("Server running on http://{}:{}", HOST, PORT);
    axum::serve(listener, app).await.unwrap();
}

type HeartbeatId = u32;
type UnixMs = u64; // # of milliseconds since unix epoch, or delta thereof
