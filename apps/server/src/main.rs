use axum::{routing::get, Json, Router};
use serde_json::{json, Value};

const HOST: &str = "0.0.0.0";
const PORT: u16 = 3000;

#[tokio::main]
async fn main() {
    let app = Router::new()
        .route("/", get(|| async { "Hello, World!" }))
        .route("/health", get(|| async { "Server is up" }))
        .route("/api", get(json));

    let listener = tokio::net::TcpListener::bind(format!("{}:{}", HOST, PORT)).await.unwrap();
    println!("Server running on http://{}:{}", HOST, PORT);
    axum::serve(listener, app).await.unwrap();
}

async fn json() -> Json<Value> {
    Json(json!({ "data": 42 }))
}
