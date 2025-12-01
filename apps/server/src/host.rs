use std::fmt;

use tokio_mpmc::Sender;

use crate::ws_msg::WsMsg;

pub struct HostEntry {
    pub pid: u32,
    pub sender: Sender<WsMsg>,
}

impl fmt::Debug for HostEntry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HostEntry")
            .field("pid", &self.pid)
            .field("sender len", &self.sender.len())
            .finish()
    }
}

impl HostEntry {
    pub fn new(pid: u32, sender: Sender<WsMsg>) -> Self {
        Self { pid, sender }
    }
}

