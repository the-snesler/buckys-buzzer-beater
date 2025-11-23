use std::{
    collections::HashMap,
    fmt,
    time::{SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};
use tokio_mpmc::{ChannelError, Sender};

use crate::{
    ConnectionStatus, HeartbeatId, UnixMs,
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
    latencies: [u32; 5],
    times_doheartbeat: HashMap<HeartbeatId, TrackedMessageTime>,
    hbid_counter: u32,
}

#[derive(Copy, Clone, Debug)]
pub struct TrackedMessageTime {
    pub t_sent: UnixMs,
    pub t_recv: Option<UnixMs>,
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
            times_doheartbeat: HashMap::new(),
            status: ConnectionStatus::Connected,
            hbid_counter: 0,
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

impl PlayerEntry {
    pub fn latency(&self) -> u32 {
        let sum: u32 = self.latencies.iter().sum();
        sum / (self.latencies.len() as u32)
    }

    pub fn time_ms() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time non-monotonic")
            .as_millis()
            .try_into()
            .expect("system time in ms exceeds 64-bit integer limit")
    }

    pub fn on_know_dohb_recv(&mut self, hbid: HeartbeatId, t_dohb_recv: UnixMs) -> bool {
        if let Some(tmt) = self.times_doheartbeat.get_mut(&hbid) {
            tmt.t_recv = Some(t_dohb_recv);
            true
        } else {
            false
        }
    }

    pub fn record_dohb(&mut self, hbid: HeartbeatId, t_sent: UnixMs) {
        self.times_doheartbeat.insert(
            hbid,
            TrackedMessageTime {
                t_sent,
                t_recv: None,
            },
        );
    }

    pub fn on_latencyhb(&mut self, hbid: HeartbeatId, t_lathb: u32) -> bool {
        if let Some(dohb) = self.times_doheartbeat.get(&hbid) {
            if let Some(lat_fwd) = dohb.delta_32bit() {
                println!("t_lathb={t_lathb},lat_fwd={lat_fwd}");
                let lat = if (t_lathb > lat_fwd) {
                    t_lathb - lat_fwd
                } else {
                    0
                };
                for i in 1..(self.latencies.len() - 1) {
                    self.latencies[i - 1] = self.latencies[i];
                }
                self.latencies[self.latencies.len() - 1] = lat;
                self.times_doheartbeat.clear();
                true
            } else {
                println!("WARN (PRE-WARN): DoHeartbeat had time sent but not received");
                false
            }
        } else {
            false
        }
    }

    fn generate_hbid(&mut self, t_sent: UnixMs) -> HeartbeatId {
        let t_part: u32 = (t_sent % 1_000)
            .try_into()
            .expect("ms part of time exceeds 32-bit integer limit (impossible)");
        t_part + (self.hbid_counter * 1_000)
    }

    pub async fn heartbeat(&mut self) -> anyhow::Result<()> {
        let t_sent = Self::time_ms();
        let hbid = self.generate_hbid(t_sent);
        self.sender
            .send(WsMsg::DoHeartbeat { hbid, t_sent })
            .await?;
        self.record_dohb(hbid, t_sent);
        Ok(())
    }
}

impl TrackedMessageTime {
    pub fn delta(&self) -> Option<u64> {
        self.t_recv.map(|x| {
            if (x > self.t_sent) {
                x - self.t_sent
            } else {
                0
            }
        })
    }

    pub fn delta_32bit(&self) -> Option<u32> {
        self.delta().map(|x| {
            x.try_into()
                .expect("delta_32bit used when delta exceeds 32-bit integer limit")
        })
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
