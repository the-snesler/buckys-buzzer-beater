use std::sync::Arc;

use anyhow::anyhow;
use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};
use serde::{Deserialize, Serialize};

use crate::{
    AppState, Room,
    api::handlers::RoomParams,
    game::Category,
    net::connection::{HostToken, RoomCode},
};

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

#[tracing::instrument(skip(state), fields(room_code = %rp.code))]
pub async fn cpr_handler(
    State(state): State<Arc<AppState>>,
    Path(rp @ RoomParams { .. }): Path<RoomParams>,
) -> String {
    let code = rp.code;
    let res = {
        let mut room_map = state.room_map.lock().await;
        let room_res = room_map
            .get_mut(&code)
            .ok_or_else(|| anyhow!("Room {} does not exist", code));
        let mut failures = 0_u32;
        match room_res {
            Err(e) => Err(e),
            Ok(room) => {
                for entry in &mut room.players {
                    match entry.heartbeat().await {
                        Ok(()) => {}
                        Err(e) => {
                            tracing::warn!(
                                player_id = entry.player.pid,
                                error = %e,
                                "Heartbeat failed"
                            );
                            failures += 1;
                        }
                    }
                }
                Ok(format!(
                    "Ok, requested {} heartbeats, {} failed immediately",
                    room.players.len(),
                    failures
                ))
            }
        }
    };
    match res {
        Ok(s) => s,
        Err(e) => {
            tracing::error!(error = %e, "CPR handler failed");
            format!("Err, {e}")
        }
    }
}
