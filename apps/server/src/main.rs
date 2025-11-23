use std::{collections::HashMap, sync::Arc};

use anyhow::anyhow;
use axum::{
    Router,
    extract::{
        Path, Query, State,
        ws::{Message, Utf8Bytes, WebSocket, WebSocketUpgrade},
    },
    response::Response,
    routing::{get, post},
};

use futures::{FutureExt, select};
use http::StatusCode;
use serde::{Deserialize, Serialize};

use tokio::sync::Mutex;
use tokio_mpmc::channel;

use crate::{game::Room, player::*, ws_msg::WsMsg};

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
    token: String,
    #[serde(rename = "playerName")]
    player_name: String,
    #[serde(rename = "playerID")]
    player_id: String,
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
    ws_upgrade.on_upgrade(async |ws| {
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
        token,
        player_name,
        player_id,
    }: WsQuery,
) -> anyhow::Result<()> {
    // for debugging
    println!("{} {} {} {}", code, token, player_name, player_id);
    let ch: tokio_mpmc::Receiver<WsMsg>;
    (_, ch) = channel(10);
    let all_chans: Vec<tokio_mpmc::Sender<WsMsg>> = vec![];
    loop {
        select! {
            res = ch.recv().fuse() => match res {
                Ok(recv) => {
                    let ser = serde_json::to_string(&recv)?;
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
                        for other_ch in &all_chans {
                            other_ch.send(witness.clone()).await?;
                        }
                    };
                    let mut room_map = state.room_map.lock().await;
                    let room = room_map
                        .get_mut(&code)
                        .ok_or_else(|| anyhow!("Room {} does not exist", code))?;
                    room.update(&msg);
                }
            }
        }
    }
    Ok(())
}

#[tokio::main]
async fn main() {
    let state = Arc::new(AppState::new());

    let room_routes = Router::new()
        .route("/create", post(|| async { StatusCode::CREATED }))
        .route("/{code}/ws", get(ws_upgrade_handler))
        .with_state(state);

    let api_routes = Router::new().nest("/rooms", room_routes);

    let app = Router::new()
        .route("/", get(|| async { "Hello, World!" }))
        .route("/health", get(|| async { "Server is up" }))
        .nest("/api/v1", api_routes);

    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await.unwrap();
    axum::serve(listener, app).await.unwrap();
}

type HeartbeatId = u32;
type UnixMs = u64; // # of milliseconds since unix epoch, or delta thereof
