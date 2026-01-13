use serde::{Deserialize, Serialize};

use crate::{UnixMs, net::connection::PlayerToken};

pub type PlayerId = u32;

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Player {
    pub pid: PlayerId,
    pub name: String,
    pub score: i32,
    pub buzzed: bool,
    pub token: PlayerToken,
}

#[derive(Copy, Clone, Debug)]
pub struct TrackedMessageTime {
    pub t_sent: UnixMs,
    pub t_recv: Option<UnixMs>,
}

impl TrackedMessageTime {
    pub fn delta(&self) -> Option<u64> {
        self.t_recv.map(|x| x.saturating_sub(self.t_sent))
    }

    pub fn delta_32bit(&self) -> Option<u32> {
        self.delta().map(|x| {
            x.try_into()
                .expect("delta_32bit used when delta exceeds 32-bit integer limit")
        })
    }
}

impl Player {
    pub fn new(pid: PlayerId, name: String, score: i32, buzzed: bool, token: PlayerToken) -> Self {
        Self {
            pid,
            name,
            score,
            buzzed,
            token,
        }
    }
}
