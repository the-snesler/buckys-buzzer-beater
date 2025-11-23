use serde::{Deserialize, Serialize};
use tokio::sync::mpsc::error::SendError;

use crate::{
    ConnectionStatus,
    ws_msg::{WsMsg, WsMsgChannel},
};

pub type PlayerId = u32;

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Player {
    pid: PlayerId,
    name: String,
    score: i32,
    buzzed: bool,
}

#[derive(Debug)]
pub struct PlayerEntry {
    player: Player,
    channel: WsMsgChannel,
    status: ConnectionStatus,
    latencies: [u32; 5],
}

impl PlayerEntry {
    pub fn new(player: Player, channel: WsMsgChannel) -> Self {
        Self {
            player,
            channel,
            latencies: [0; 5],
            status: ConnectionStatus::Connected,
        }
    }

    pub fn did_buzz(&self) -> bool {
        self.player.buzzed
    }

    pub async fn update(&self, msg: &WsMsg) -> Result<(), SendError<WsMsg>> {
        self.channel.0.send(msg.clone()).await?;
        Ok(())
    }
}

impl Player {
    pub fn new(pid: PlayerId, name: String, score: i32, buzzed: bool) -> Self {
        Self { pid, name, score, buzzed }
    }
}
