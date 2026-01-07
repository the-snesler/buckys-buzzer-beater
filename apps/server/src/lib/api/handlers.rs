use serde::{Deserialize, Serialize};

use crate::{game::room::Room, net::connection::{HostToken, PlayerToken}, PlayerId};

#[derive(Deserialize, Debug)]
pub struct WsQuery {
    #[serde(rename = "playerName")]
    pub player_name: Option<String>, // only players include player_name
    pub token: Option<PlayerToken>, // only rejoining players include both token & player_id

    #[serde(rename = "playerID")]
    pub player_id: Option<u32>,
    pub host_token: Option<HostToken>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct RoomParams {
    pub code: String,
}

/// An AuthenticatedUser represents the result of the WebSocket handshake which validates users.
pub enum AuthenticatedUser {
    /// The host of a game
    Host,

    /// A brand new player who needs to be registered.
    NewPlayer { name: String },

    /// A returning player that has been verified by ID and Token.
    ExistingPlayer { pid: PlayerId },
}

/// Validates the connection request against the room's current state.
///
/// This function only checks if the credentials are valid but does not register the user or update
/// channels.
pub fn perform_handshake(
    room: &Room,
    query: &WsQuery,
) -> anyhow::Result<AuthenticatedUser> {
    if let Some(provided_token) = &query.token {
        if provided_token.to_string() == room.host_token.to_string() {
            return Ok(AuthenticatedUser::Host);
        }
    }

    if let (Some(pid), Some(token)) = (query.player_id, &query.token) {
        let found = room.players.iter().any(|p| {
            p.player.pid == pid && p.player.token == *token
        });
         
        if found {
            return Ok(AuthenticatedUser::ExistingPlayer { pid });
        } else {
            return Err(anyhow::anyhow!("Invalid player credentials"));
        }
    }

    if let Some(name) = &query.player_name {
        return Ok(AuthenticatedUser::NewPlayer { name: name.clone() });
    }

    Err(anyhow::anyhow!("Missing connection credentials"))
}
