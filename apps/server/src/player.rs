use std::fmt;

use serde::{Deserialize, Serialize};
use tokio_mpmc::{ChannelError, Sender};

use crate::{
    ConnectionStatus,
    ws_msg::{WsMsg, WsMsgChannel},
};

pub type PlayerId = u32;

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Player {
    pub pid: PlayerId,
    pub name: String,
    pub score: i32,
    pub buzzed: bool,
    pub token: String,
}

pub struct PlayerEntry {
    pub player: Player,
    pub sender: Sender<WsMsg>,
    pub status: ConnectionStatus,
    pub latencies: [u32; 5],
}

impl fmt::Debug for PlayerEntry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PlayerEntry")
            .field("player", &self.player)
            .field("status", &self.status)
            .field("latencies", &self.latencies)
            .finish()
    }
}

impl PlayerEntry {
    pub fn new(player: Player, sender: Sender<WsMsg>) -> Self {
        Self {
            player,
            sender,
            latencies: [0; 5],
            status: ConnectionStatus::Connected,
        }
    }

    pub fn did_buzz(&self) -> bool {
        self.player.buzzed
    }

    pub async fn update(&self, msg: &WsMsg) -> Result<(), ChannelError> {
        self.sender.send(msg.clone()).await?;
        Ok(())
    }
}

impl Player {
    pub fn new(pid: PlayerId, name: String, score: i32, buzzed: bool, token: String) -> Self {
        Self {
            pid,
            name,
            score,
            buzzed,
            token,
        }
    }
}
