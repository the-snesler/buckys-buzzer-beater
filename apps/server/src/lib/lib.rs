//! Madhacks 2025 Bucky's Buzzer Beater
//!
//! A real-time multiplayer game built with Axum and WebSockets.
//!
//! # Game Flow
//!
//! 1. **Room Creation**: Host creates a room via HTTP POST
//! 2. **Player Joining**: Players connect via WebSocket with room code
//! 3. **Game Play**: Host controls game flow, players buzz in to answer
//! 4. **Scoring**: Points awarded/deducted based on answer correctness
//!
//! # Modules
//! 
//! - [`api`] - HTTP routes and WebSocket handlers
//! - [`game`] - Game logic and state management
//! - [`net`] - Networking connections and tokens
//! - [`player`] - Player data structures

pub mod api;
pub mod game;
pub mod host;
pub mod net;
pub mod player;
pub mod ws_msg;

use std::{
    collections::HashMap,
    sync::Arc,
    time::{Duration, SystemTime},
};

use axum::{
    Router,
    routing::{any, get, post},
};
use host::HostEntry;
use player::*;
use tokio::sync::Mutex;
use tower_http::services::{ServeDir, ServeFile};

use crate::{
    api::routes::{cpr_handler, create_room},
    game::room::Room,
    net::{connection::PlayerEntry, ws::handler::ws_upgrade_handler},
};

pub type HeartbeatId = u32;
pub type UnixMs = u64; // # of milliseconds since unix epoch, or delta thereof

pub struct AppState {
    pub room_map: Mutex<HashMap<String, Room>>,
    pub room_ttl: Duration,
}

impl Default for AppState {
    fn default() -> Self {
        Self::new()
    }
}

impl AppState {
    pub fn new() -> Self {
        Self {
            room_map: Mutex::new(HashMap::new()),
            room_ttl: Duration::from_secs(30 * 60),
        }
    }

    pub fn with_ttl(ttl: Duration) -> Self {
        Self {
            room_map: Mutex::new(HashMap::new()),
            room_ttl: ttl,
        }
    }
}

pub fn build_app(state: Arc<AppState>) -> Router {
    let room_routes = Router::new()
        .route("/create", post(create_room))
        .route("/{code}/ws", any(ws_upgrade_handler))
        .route("/{code}/cpr", get(cpr_handler))
        .with_state(state);

    let api_routes = Router::new().nest("/rooms", room_routes);

    Router::new()
        .route("/health", get(|| async { "Server is up" }))
        .nest("/api/v1", api_routes)
        .fallback_service(
            ServeDir::new("public").not_found_service(ServeFile::new("public/index.html")),
        )
}

#[tracing::instrument(skip(state))]
pub async fn cleanup_inactive_rooms(state: &Arc<AppState>) {
    let mut room_map = state.room_map.lock().await;
    let threshold = SystemTime::now()
        .checked_sub(state.room_ttl)
        .unwrap_or(SystemTime::UNIX_EPOCH);

    let rooms_to_remove: Vec<String> = room_map
        .iter()
        .filter(|(_, room)| room.last_activity < threshold)
        .map(|(code, _)| code.clone())
        .collect();

    if rooms_to_remove.is_empty() {
        tracing::trace!("No inactive rooms to clean up");
    } else {
        for code in &rooms_to_remove {
            room_map.remove(code);
        }
        tracing::info!(count = rooms_to_remove.len(), "Cleaned up inactive rooms");
    }
}
