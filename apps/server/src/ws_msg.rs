use serde::{Deserialize, Serialize};
use tokio::sync::mpsc::{Receiver, Sender};

use crate::{player::{Player, PlayerId}, HeartbeatId, UnixMs};

pub type WsMsgChannel = (Sender<WsMsg>, Receiver<WsMsg>);

#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum WsMsg {
    Witness { msg: Box<WsMsg> },
    PlayerList(Vec<Player>),
    NewPlayer { pid: PlayerId, token: String },
    StartGame,
    EndGame,
    BuzzEnable,
    BuzzDisable,
    Buzz,
    HostChecked { correct: bool },
    DoHeartbeat { hbid: HeartbeatId, t_sent: UnixMs },
    Heartbeat { hbid: HeartbeatId },
    GotHeartbeat { hbid: HeartbeatId },
    LatencyOfHeartbeat { hbid: HeartbeatId, t_lat: UnixMs },
}
