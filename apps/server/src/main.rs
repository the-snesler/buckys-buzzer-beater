use std::{collections::HashMap, sync::Arc};

use anyhow::anyhow;
use axum::{
    Json, Router,
    extract::{
        Path, Query, State,
        ws::{Message, Utf8Bytes, WebSocket, WebSocketUpgrade},
    },
    response::Response,
    routing::{any, get, post},
};

use futures::{FutureExt, select};
use http::StatusCode;
use rand::Rng;
use serde::{Deserialize, Serialize};

use tokio::sync::Mutex;
use tokio_mpmc::channel;

use crate::{game::Room, host::HostEntry, player::*, ws_msg::WsMsg};

mod game;
mod host;
mod player;
mod ws_msg;

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

#[derive(Serialize)]
struct CreateRoomResponse {
    room_code: String,
    host_token: String,
}

async fn create_room(State(state): State<Arc<AppState>>) -> (StatusCode, Json<CreateRoomResponse>) {
    let mut room_map = state.room_map.lock().await;

    // Generate a unique room code
    let code = loop {
        let candidate = generate_room_code();
        if !room_map.contains_key(&candidate) {
            break candidate;
        }
    };

    let host_token = generate_host_token();
    let room = Room::new(code.clone(), host_token.clone());
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
    player_name: Option<String>,    // only players include player_name
    token: Option<String>,          // only rejoining players include both token & player_id
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
    {
        let mut room_map = state.room_map.lock().await;
        let room = room_map
            .get_mut(&code)
            .ok_or_else(|| anyhow!("Room {} does not exist", code))?;
        println!("room: {:?}", room);

        match (player_id, token, player_name) {
            (Some(id), Some(t), Some(name)) => {
                if t == room.host_token {
                    let host = HostEntry::new(id, tx);
                    let players: &Vec<Player> = &room.players.iter().clone().map(|entry| entry.player.clone()).collect();
                    let msg = WsMsg::PlayerList { list: players.clone() };
                    println!("{:?}", &msg);
                    host.sender.send(msg).await?;
                    room.host = Some(host);
                } else {
                    let player = PlayerEntry::new(Player::new(id, name, 0, false), tx);
                    room.players.push(player);
                }
            },
            (_, _, Some(name)) => {
                // Shouldnt fail conversion I hope
                let player = PlayerEntry::new(Player::new((room.players.len() + 1).try_into().unwrap(), name, 0, false), tx);
                room.players.push(player);
            },
            (_, Some(t), _) => {
                println!("fjdsklajfslk");
                if let Some(host) = &room.host {
                    if t == room.host_token {
                        let players: &Vec<Player> = &room.players.iter().clone().map(|entry| entry.player.clone()).collect();
                        let msg = WsMsg::PlayerList { list: players.clone() };
                        println!("{:?}", &msg);
                        host.sender.send(msg).await?;
                    }
                }
            }
            _ => {}
        }

        for player in &room.players {
            println!("player: {}", player.player.pid);
        }
    }
    loop {
        select! {
            res = ch.recv().fuse() => match res {
                Ok(recv) => {
                    let ser = serde_json::to_string(&recv)?;
                    println!("ser {}", ser);
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
                    if let m @ (WsMsg::StartGame
                        | WsMsg::EndGame
                        | WsMsg::BuzzEnable
                        | WsMsg::BuzzDisable
                        | WsMsg::Buzz) = msg.clone() {
                        let witness = WsMsg::Witness { msg: Box::new(m) };
                        let mut room_map = state.room_map.lock().await;
                        let room = room_map
                            .get_mut(&code)
                            .ok_or_else(|| anyhow!("Room {} does not exist", code))?;
                        for player in &room.players {
                            if let Some(id) = player_id {
                                if player.player.pid == id {
                                    continue;
                                }
                            }
                            let s = &player.sender;
                            s.send(witness.clone()).await?;
                        }
                    };
                    let mut room_map = state.room_map.lock().await;
                    let room = room_map
                        .get_mut(&code)
                        .ok_or_else(|| anyhow!("Room {} does not exist", code))?;
                    room.update(&msg, player_id).await?;
                }
            }
        }
    }
    Ok(())
}

const HOST: &str = "0.0.0.0";
const PORT: u16 = 3000;

#[tokio::main]
async fn main() {
    let state = Arc::new(AppState::new());

    let room_routes = Router::new()
        .route("/create", post(create_room))
        .route("/{code}/ws", any(ws_upgrade_handler))
        .with_state(state);

    let api_routes = Router::new().nest("/rooms", room_routes);

    let app = Router::new()
        .route("/", get(|| async { "Hello, World!" }))
        .route("/health", get(|| async { "Server is up" }))
        .nest("/api/v1", api_routes);

    let listener = tokio::net::TcpListener::bind(format!("{}:{}", HOST, PORT))
        .await
        .unwrap();
    println!("Server running on http://{}:{}", HOST, PORT);
    axum::serve(listener, app).await.unwrap();
}

type HeartbeatId = u32;
type UnixMs = u64; // # of milliseconds since unix epoch, or delta thereof
