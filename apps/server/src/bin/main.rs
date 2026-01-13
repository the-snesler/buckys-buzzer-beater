use std::{sync::Arc, time::Duration};

use anyhow::Result;
use madhacks2025::{AppState, build_app, cleanup_inactive_rooms};

const HOST: &str = "0.0.0.0";
const PORT: u16 = 3000;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "madhacks2025=debug,tower_http=debug".into()),
        )
        .init();

    tracing::info!("Starting server on {}:{}", HOST, PORT);

    let state = Arc::new(AppState::new());
    let cleanup_state = state.clone();
    let app = build_app(state);

    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(60));
        loop {
            interval.tick().await;
            cleanup_inactive_rooms(&cleanup_state).await;
        }
    });

    let listener = tokio::net::TcpListener::bind(format!("{}:{}", HOST, PORT)).await?;
    tracing::info!("Server running on http://{}:{}", HOST, PORT);
    axum::serve(listener, app)
        .await
        .expect("Failed to start server");
    Ok(())
}
