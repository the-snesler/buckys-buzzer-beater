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
}

pub struct PlayerEntry {
    pub player: Player,
    pub sender: Sender<WsMsg>,
    pub status: ConnectionStatus,
    pub latencies: [u32; 5],
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
    pub fn new(pid: PlayerId, name: String, score: i32, buzzed: bool) -> Self {
        Self {
            pid,
            name,
            score,
            buzzed,
        }
    }
}
