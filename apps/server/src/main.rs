use axum::{
    extract::{Path, Query}, routing::{get, post}, Router
};
use http::StatusCode;
use serde::{Deserialize, Serialize};

mod game;

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
    Path(RoomParams { code }): Path<RoomParams>,
    Query(WsQuery {
        token,
        player_name,
        player_id
    }): Query<WsQuery>,
) {
    println!("{} {} {} {}", code, token, player_name, player_id);
}

#[tokio::main]
async fn main() {
    let room_routes = Router::new()
        .route("/create", post(|| async { StatusCode::CREATED }))
        .route("/{code}/ws", get(ws_handler));

    let api_routes = Router::new()
        .nest("/rooms", room_routes);

    let app = Router::new()
        .route("/", get(|| async { "Hello, World!" }))
        .route("/health", get(|| async { "Server is up" }))
        .nest("/api/v1", api_routes);

    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await.unwrap();
    axum::serve(listener, app).await.unwrap();
}
