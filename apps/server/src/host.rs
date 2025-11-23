use serde::{Deserialize, Serialize};
use tokio_mpmc::{ChannelError, Sender};

use crate::{player::PlayerId, ws_msg::WsMsg};

pub struct HostEntry {
    pub pid: u32,
    pub sender: Sender<WsMsg>,
}

impl HostEntry {
    pub fn new(pid: u32, sender: Sender<WsMsg>) -> Self {
        Self { pid, sender }
    }

    pub async fn update(&self, msg: WsMsg) -> Result<(), ChannelError> {
        self.sender.send(msg).await?;
        Ok(())
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Host {
    pub pid: PlayerId,
}
