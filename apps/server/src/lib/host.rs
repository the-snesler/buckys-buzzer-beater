use std::fmt;

use crate::net::connection::{EventSender, queued_events};

pub struct HostEntry {
    pub pid: u32,
    pub sender: EventSender,
}

impl fmt::Debug for HostEntry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HostEntry")
            .field("pid", &self.pid)
            .field("queued events", &queued_events(&self.sender))
            .finish()
    }
}

impl HostEntry {
    pub fn new(pid: u32, sender: EventSender) -> Self {
        Self { pid, sender }
    }
}
