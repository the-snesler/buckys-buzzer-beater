use std::{fmt::Display, str::FromStr};

use rand::Rng;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// A unique identifier for a game room (e.g., "AFKRTWZ")
///
/// Room codes are generated using a restricted charset to ensure they are easy to read and type.
/// Characters such as I and O are omitted to reduce mistaken characters.
#[derive(PartialEq, Eq, Hash, Clone, Debug, Serialize, Deserialize)]
pub struct RoomCode(String);

impl RoomCode {
    /// Generates a random 6-character code.
    ///
    /// The default charset is "ABCDEFGHJKLMNPQRSTUVWXYZ".
    pub fn generate() -> Self {
        const CHARSET: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZ";
        let mut rng = rand::rng();
        let code: String = (0..6)
            .map(|_| {
                let idx = rng.random_range(0..CHARSET.len());
                CHARSET[idx] as char
            })
            .collect();
        Self(code)
    }
}

impl From<String> for RoomCode {
    fn from(value: String) -> Self {
        Self(value)
    }
}

impl FromStr for RoomCode {
    type Err = std::convert::Infallible; // Just strings, so infallible
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self(s.to_string()))
    }

}

impl Display for RoomCode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::ops::Deref for RoomCode {
    type Target = str;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

/// A UUID used by the room creator to prove they are the Host.
///
/// This token should be sent in the WebSocket handshake to authorize
/// administrative actions like starting the game or revealing answers.
#[derive(PartialEq, Eq, Hash, Clone, Debug, Serialize, Deserialize)]
pub struct HostToken(Uuid);

impl HostToken {
    /// Generates a new random UUID v4.
    pub fn generate() -> Self {
        Self(Uuid::new_v4())
    }

    pub fn to_string(&self) -> String {
        self.0.to_string()
    }
}

impl Display for HostToken {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::str::FromStr for HostToken {
    type Err = uuid::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self(Uuid::parse_str(s)?))
    }
}

/// A secret UUID assigned to each player upon joining a room.
#[derive(PartialEq, Eq, Hash, Clone, Debug, Serialize, Deserialize)]
pub struct PlayerToken(Uuid);

impl PlayerToken {
    /// Generates a new random UUID v4.
    pub fn generate() -> Self {
        Self(Uuid::new_v4())
    }

    pub fn to_string(&self) -> String {
        self.0.to_string()
    }
}

impl Display for PlayerToken {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::str::FromStr for PlayerToken {
    type Err = uuid::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self(Uuid::parse_str(s)?))
    }
}
