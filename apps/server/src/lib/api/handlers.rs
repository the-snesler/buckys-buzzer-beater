use serde::{Deserialize, Serialize};

use crate::net::connection::{HostToken, PlayerToken};


#[derive(Deserialize)]
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

