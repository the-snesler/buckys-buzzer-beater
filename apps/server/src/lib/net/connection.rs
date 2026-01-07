use std::{collections::HashMap, fmt::{self, Display}, str::FromStr, time::{SystemTime, UNIX_EPOCH}};

use rand::Rng;
use serde::{Deserialize, Serialize};
use tokio_mpmc::Sender;
use uuid::Uuid;

use crate::{ws_msg::WsMsg, HeartbeatId, Player, TrackedMessageTime, UnixMs};

/// A unique identifier for a game room (e.g., "AFKRTWZ")
///
/// Room codes are generated using a restricted charset to ensure they are easy to read and type.
/// Characters such as I and O are omitted to reduce mistaken characters.
#[derive(PartialEq, Eq, Hash, Clone, Debug, Serialize, Deserialize)]
pub struct RoomCode(String);

impl RoomCode {
    /// Generates a random 6-character code.
    ///
    /// The default charset is "ABCDEFGHJKLMNPQRSTUVWXYZ".
    pub fn generate() -> Self {
        const CHARSET: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZ";
        let mut rng = rand::rng();
        let code: String = (0..6)
            .map(|_| {
                let idx = rng.random_range(0..CHARSET.len());
                CHARSET[idx] as char
            })
            .collect();
        Self(code)
    }
}

impl From<String> for RoomCode {
    fn from(value: String) -> Self {
        Self(value)
    }
}

impl FromStr for RoomCode {
    type Err = std::convert::Infallible; // Just strings, so infallible
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self(s.to_string()))
    }

}

impl Display for RoomCode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::ops::Deref for RoomCode {
    type Target = str;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

/// A UUID used by the room creator to prove they are the Host.
///
/// This token should be sent in the WebSocket handshake to authorize
/// administrative actions like starting the game or revealing answers.
#[derive(PartialEq, Eq, Hash, Clone, Debug, Serialize, Deserialize)]
pub struct HostToken(Uuid);

impl HostToken {
    /// Generates a new random UUID v4.
    pub fn generate() -> Self {
        Self(Uuid::new_v4())
    }

    pub fn to_string(&self) -> String {
        self.0.to_string()
    }
}

impl Display for HostToken {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::str::FromStr for HostToken {
    type Err = uuid::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self(Uuid::parse_str(s)?))
    }
}

/// A secret UUID assigned to each player upon joining a room.
#[derive(PartialEq, Eq, Hash, Clone, Debug, Serialize, Deserialize)]
pub struct PlayerToken(Uuid);

impl PlayerToken {
    /// Generates a new random UUID v4.
    pub fn generate() -> Self {
        Self(Uuid::new_v4())
    }

    pub fn to_string(&self) -> String {
        self.0.to_string()
    }
}

impl Display for PlayerToken {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::str::FromStr for PlayerToken {
    type Err = uuid::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self(Uuid::parse_str(s)?))
    }
}

pub struct PlayerEntry {
    pub player: Player,
    pub sender: Sender<WsMsg>,
    pub status: ConnectionStatus,
    latencies: [u32; 5],
    times_doheartbeat: HashMap<HeartbeatId, TrackedMessageTime>,
    hbid_counter: u32,
}

impl fmt::Debug for PlayerEntry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PlayerEntry")
            .field("player", &self.player)
            .field("status", &self.status)
            .field("latencies", &self.latencies)
            .field("sender len", &self.sender.len())
            .field("times_doheartbeat", &self.times_doheartbeat)
            .field("hbid_counter", &self.hbid_counter)
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
}

impl PlayerEntry {
    pub fn latency(&self) -> anyhow::Result<u32> {
        let sum: u32 = self.latencies.iter().sum();
        let latencies_len: u32 = self.latencies.len().try_into()?;
        Ok(sum / latencies_len)
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
                let lat = t_lathb.saturating_sub(lat_fwd);
                tracing::trace!(
                    player_id = self.player.pid,
                    hbid,
                    latency = lat,
                    "Updated player latency"
                );
                for i in 1..(self.latencies.len() - 1) {
                    self.latencies[i - 1] = self.latencies[i];
                }
                self.latencies[self.latencies.len() - 1] = lat;
                self.times_doheartbeat.clear();
                true
            } else {
                tracing::warn!(
                    player_id = self.player.pid,
                    hbid,
                    "DoHeartbeat time sent but not received"
                );
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

#[derive(Debug)]
pub enum ConnectionStatus {
    Connected,
    Disconnected,
}

