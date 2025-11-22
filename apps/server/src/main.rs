use axum::{routing::get, Json, Router};
use serde_json::{json, Value};

#[tokio::main]
async fn main() {
    let app = Router::new()
        .route("/", get(|| async { "Hello, World!" }))
        .route("/health", get(|| async { "Server is up" }))
        .route("/api", get(json));

    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await.unwrap();
    axum::serve(listener, app).await.unwrap();
}

async fn json() -> Json<Value> {
    Json(json!({ "data": 42 }))
}
