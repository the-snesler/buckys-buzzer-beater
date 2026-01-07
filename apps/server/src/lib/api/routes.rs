use std::sync::Arc;

use axum::{extract::State, Json, http::StatusCode};
use serde::{Deserialize, Serialize};

use crate::{game::Category, net::connection::{HostToken, RoomCode}, AppState, Room};

#[tracing::instrument(skip(state, body))]
pub async fn create_room(
    State(state): State<Arc<AppState>>,
    Json(body): Json<CreateRoomRequest>,
) -> (StatusCode, Json<CreateRoomResponse>) {
    let mut room_map = state.room_map.lock().await;

    // Generate a unique room code
    let code = loop {
        let candidate = RoomCode::generate();
        if !room_map.contains_key(&candidate.to_string()) {
            break candidate;
        }
    };

    let host_token = HostToken::generate();
    let mut room = Room::new(code.clone(), host_token.clone());

    if let Some(categories) = body.categories {
        room.categories = categories;
    }

    room_map.insert(code.to_string(), room);

    tracing::info!(room_code = %code, "Room created");

    (
        StatusCode::CREATED,
        Json(CreateRoomResponse {
            room_code: code,
            host_token,
        }),
    )
}

#[derive(Serialize)]
pub struct CreateRoomResponse {
    room_code: RoomCode,
    host_token: HostToken,
}

#[derive(Deserialize)]
pub struct CreateRoomRequest {
    categories: Option<Vec<Category>>,
}

