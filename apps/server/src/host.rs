use serde::{Deserialize, Serialize};
use tokio::sync::mpsc::error::SendError;

use crate::{
    player::PlayerId,
    ws_msg::{WsMsg, WsMsgChannel},
};

#[derive(Debug)]
pub struct HostEntry {
    pid: u32,
    channel: WsMsgChannel,
}

impl HostEntry {
    pub fn new(pid: u32, channel: WsMsgChannel) -> Self {
        Self { pid, channel }
    }

    pub async fn update(&self, msg: WsMsg) -> Result<(), SendError<WsMsg>> {
        self.channel.0.send(msg).await?;
        Ok(())
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Host {
    pid: PlayerId,
}
