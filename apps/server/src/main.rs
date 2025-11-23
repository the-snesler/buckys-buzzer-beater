use std::sync::{Arc, mpsc};

use axum::{
    Json, Router,
    extract::{
        Path, Query,
        ws::{Message, Utf8Bytes, WebSocket, WebSocketUpgrade},
    },
    response::{IntoResponse, Response},
    routing::{get, post},
};
use futures::{FutureExt, select};
use http::StatusCode;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use tokio_mpmc;
use tokio_mpmc::channel;

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
                    match msg.clone() {
                        m @ (WsMsg::StartGame
                        | WsMsg::EndGame
                        | WsMsg::BuzzEnable
                        | WsMsg::BuzzDisable
                        | WsMsg::Buzz { .. }) => {
                            let witness = WsMsg::Witness { msg: Box::new(m) };
                            for other_ch in &all_chans {
                                other_ch.send(witness.clone()).await?;
                            }
                        }
                        _ => {}
                    }
                    match msg {
                        WsMsg::StartGame => {},
                        WsMsg::EndGame => {},
                        WsMsg::BuzzEnable => {},
                        WsMsg::BuzzDisable => {},
                        WsMsg::Buzz => {},
                        WsMsg::Heartbeat { hbid } => {},
                        WsMsg::LatencyOfHeartbeat { hbid, t_lat } => {},
                        _ => {}
                    }
                    ()
                },
            }
        }
    }
    Ok(())
}

#[tokio::main]
async fn main() {
    let room_routes = Router::new()
        .route("/create", post(|| async { StatusCode::CREATED }))
        .route("/{code}/ws", get(ws_upgrade_handler));

    let api_routes = Router::new().nest("/rooms", room_routes);

    let app = Router::new()
        .route("/", get(|| async { "Hello, World!" }))
        .route("/health", get(|| async { "Server is up" }))
        .nest("/api/v1", api_routes);

    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await.unwrap();
    axum::serve(listener, app).await.unwrap();
}

type PlayerId = u32;
type HeartbeatId = u32;
type UnixMs = u64; // # of milliseconds since unix epoch, or delta thereof

#[derive(Serialize, Deserialize, Clone, Debug)]
struct PlayerEntry {
    pid: PlayerId,
    name: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
enum WsMsg {
    Witness { msg: Box<WsMsg> },
    PlayerList { list: Vec<PlayerEntry> },
    StartGame,
    EndGame,
    BuzzEnable,
    BuzzDisable,
    Buzz,
    DoHeartbeat { hbid: HeartbeatId, t_sent: UnixMs },
    Heartbeat { hbid: HeartbeatId },
    GotHeartbeat { hbid: HeartbeatId },
    LatencyOfHeartbeat { hbid: HeartbeatId, t_lat: UnixMs },
}

async fn json() -> Json<Value> {
    Json(json!({ "data": 42 }))
}
