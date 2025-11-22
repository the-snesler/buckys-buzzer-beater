use axum::{Json, Router, routing::get};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

#[tokio::main]
async fn main() {
    let app = Router::new()
        .route("/", get(|| async { "Hello, World!" }))
        .route("/health", get(|| async { "Server is up" }))
        .route("/api", get(json));

    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await.unwrap();
    axum::serve(listener, app).await.unwrap();
}

type PlayerId = u32;
type HeartbeatId = u32;
type UnixMs = u64; // # of milliseconds since unix epoch, or delta thereof

#[derive(Serialize, Deserialize, Clone, Debug)]
enum WsMsg {
    Witness {
        msg: Box<WsMsg>,
    },
    Join {
        pid: PlayerId,
        name: String,
    },
    StartGame,
    EndGame,
    BuzzEnable,
    BuzzDisable,
    Buzz {
        pid: PlayerId,
    },
    DoHeartbeat {
        hbid: HeartbeatId,
        t_sent: UnixMs,
    },
    Heartbeat {
        hbid: HeartbeatId,
    },
    GotHeartbeat {
        hbid: HeartbeatId,
    },
    LatencyOfHeartbeat {
        pid: PlayerId,
        hbid: HeartbeatId,
        t_lat: UnixMs,
    },
}

async fn json() -> Json<Value> {
    Json(json!({ "data": 42 }))
}
