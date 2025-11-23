use std::sync::mpsc::SendError;

use serde::{Deserialize, Serialize};

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
    pub fn update(&self, msg: WsMsg) -> Result<(), SendError<WsMsg>> {
        self.channel.0.send(msg)?;
        Ok(())
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Host {
    pid: PlayerId,
}
