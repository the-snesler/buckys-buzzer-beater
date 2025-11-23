use axum::{
    extract::{ws::{WebSocket, WebSocketUpgrade}, Path, Query}, response::IntoResponse, routing::{get, post}, Json, Router
};

use http::StatusCode;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::player::*;

mod game;
mod player;
mod host;
mod ws_msg;

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

async fn ws_handler(
    ws_upgrade: WebSocketUpgrade,
    Path(RoomParams { code }): Path<RoomParams>,
    Query(WsQuery {
        token,
        player_name,
        player_id,
    }): Query<WsQuery>,
) {
    ws_upgrade.on_upgrade(|ws| {
        ws_socket_handler(
            ws,
            rp,
            WsQuery {
                token,
                player_name,
                player_id,
            },
        )
    });
}

async fn ws_socket_handler(
    mut ws: WebSocket,
    RoomParams { code }: RoomParams,
    WsQuery {
        token,
        player_name,
        player_id,
    }: WsQuery,
) -> anyhow::Result<impl IntoResponse> {
    while let Some(msg) = socket.recv().await {
        let msg = if let Ok(msg) = msg {
                msg
            } else {
                // client disconnected
                return Err(std::io::Error::new(
                    std::io::ErrorKind::HostUnreachable,
                    "websocket client disconnected in read"
                ));
            };
        // deser
        let msg: WsMsg = serde_json::from_str(&msg)?;
        // witness case
        match 
    }
    // witness case
    println!("{} {} {} {}", code, token, player_name, player_id);
}

#[tokio::main]
async fn main() {
    let room_routes = Router::new()
        .route("/create", post(|| async { StatusCode::CREATED }))
        .route("/{code}/ws", get(ws_upgrade_handler));

    let api_routes = Router::new()
        .nest("/rooms", room_routes);

    let app = Router::new()
        .route("/", get(|| async { "Hello, World!" }))
        .route("/health", get(|| async { "Server is up" }))
        .nest("/api/v1", api_routes);

    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await.unwrap();
    axum::serve(listener, app).await.unwrap();
}

type HeartbeatId = u32;
type UnixMs = u64; // # of milliseconds since unix epoch, or delta thereof

async fn json() -> Json<Value> {
    Json(json!({ "data": 42 }))
}
