use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    PlayerId,
    game::room::Room,
};

#[derive(Deserialize, Debug)]
pub struct WsQuery {
    pub token: Option<Uuid>, // only rejoining players include both token & player_id
    #[serde(rename = "playerName")]
    pub player_name: Option<String>, // only players include player_name
    #[serde(rename = "playerID")]
    pub player_id: Option<u32>,
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

/// Validates the connection request.
///
/// This function authenticates users based on the credentials provided in the
/// query parameters. It checks credentials in the following priority order:
/// 1. **[HostToken]** - If provided and matches, authenticates as host
/// 2. **[PlayerId] + [PlayerToken]** - If both provided and valid, authenticates as existing player
/// 3. **Player name** - If provided, authenticates as new player
/// 4. Otherwise returns an error
///
/// # Arguments
/// - `room` - The room to authenticate against
/// - `query` - The WebSocket query parameters containing credentials
///
/// # Returns
/// - Ok([AuthenticatedUser]) - Successfully authenticated user
/// - `Err` - Invalid or missing credentials
///
/// # Examples
/// ```
/// use madhacks2025::api::handlers::{perform_handshake, WsQuery, AuthenticatedUser};
/// use madhacks2025::game::room::Room;
/// use madhacks2025::net::connection::{RoomCode, HostToken};
/// use uuid::Uuid;
///
/// let host_uuid = Uuid::new_v4();
/// let room = Room::new(
///     RoomCode::from("TEST".to_string()),
///     HostToken::from(host_uuid)
/// );
///
/// let query = WsQuery {
///     player_name: None,
///     token: Some(host_uuid),
///     player_id: None,
/// };
///
/// let result = perform_handshake(&room, &query);
/// assert!(result.is_ok());
/// assert!(matches!(result.unwrap(), AuthenticatedUser::Host));
/// ```
pub fn perform_handshake(room: &Room, query: &WsQuery) -> anyhow::Result<AuthenticatedUser> {
    if let Some(provided_token) = query.token {
        if room.host_token.matches(provided_token) {
            return Ok(AuthenticatedUser::Host);
        }

        if let Some(pid) = query.player_id {
            let found = room.players.iter().any(|p| {
                p.player.pid == pid && p.player.token.matches(provided_token)
            });

            if found {
                return Ok(AuthenticatedUser::ExistingPlayer { pid })
            }
        }

        return Err(anyhow::anyhow!("Invalid token"));
    }

    if let Some(name) = &query.player_name {
        return Ok(AuthenticatedUser::NewPlayer { name: name.clone() });
    }

    Err(anyhow::anyhow!("Missing connection credentials"))
}

#[cfg(test)]
mod test {
    use tokio_mpmc::channel;

    use crate::{
        Player,
        api::handlers::{AuthenticatedUser, WsQuery, perform_handshake},
        game::room::Room,
        net::connection::{HostToken, PlayerEntry, PlayerToken, RoomCode},
    };

    /// Helper to create a test room with a known host token
    fn create_test_room() -> (Room, HostToken) {
        let host_token = HostToken::generate();
        let room = Room::new(RoomCode::from("TEST".to_string()), host_token.clone());
        (room, host_token)
    }

    /// Helper to add a player to a room and return their token
    fn add_player_to_room(room: &mut Room, pid: u32, name: &str) -> PlayerToken {
        let (tx, _rx) = channel(10);
        let token = PlayerToken::generate();
        let player = PlayerEntry::new(
            Player::new(pid, name.to_string(), 0, false, token.clone()),
            tx,
        );
        room.players.push(player);
        token
    }
    #[test]
    fn test_perform_handshake_host() {
        use uuid::Uuid;

        let host_uuid = Uuid::new_v4();
        let room = Room::new(
            RoomCode::from("TEST".to_string()),
            HostToken::from(host_uuid)
        );

        let query = WsQuery {
            player_name: None,
            token: Some(host_uuid),
            player_id: None,
        };

        let result = perform_handshake(&room, &query);
        assert!(result.is_ok());
        assert!(matches!(result.unwrap(), AuthenticatedUser::Host));
    }

    #[test]
    fn test_perform_handshake_existing_player() {
        use uuid::Uuid;

        let player_uuid = Uuid::new_v4();
        let (mut room, _host_token) = create_test_room();
        let _player_token = add_player_to_room(&mut room, 1, "Alice");

        // Update the player's token to our known UUID
        room.players[0].player.token = PlayerToken::from(player_uuid);

        let query = WsQuery {
            player_name: None,
            token: Some(player_uuid),
            player_id: Some(1),
        };

        let result = perform_handshake(&room, &query);
        assert!(result.is_ok());
        match result.unwrap() {
            AuthenticatedUser::ExistingPlayer { pid } => {
                assert_eq!(pid, 1);
            }
            _ => panic!("Expected ExistingPlayer"),
        }
    }

    #[test]
    fn test_perform_handshake_new_player() {
        let (room, _host_token) = create_test_room();

        let query = WsQuery {
            player_name: Some("Bob".to_string()),
            token: None,
            player_id: None,
        };

        let result = perform_handshake(&room, &query);
        assert!(result.is_ok());
        match result.unwrap() {
            AuthenticatedUser::NewPlayer { name } => {
                assert_eq!(name, "Bob");
            }
            _ => panic!("Expected NewPlayer"),
        }
    }
}
