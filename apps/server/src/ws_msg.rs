use serde::{Deserialize, Serialize};
use tokio::sync::mpsc::{Receiver, Sender};

use crate::{HeartbeatId, UnixMs, player::Player};

pub type WsMsgChannel = (Sender<WsMsg>, Receiver<WsMsg>);

#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum WsMsg {
    Witness { msg: Box<WsMsg> },
    PlayerList { list: Vec<Player> },
    StartGame,
    EndGame,
    BuzzEnable,
    BuzzDisable,
    Buzz,
    DoHeartbeat { hbid: HeartbeatId, t_sent: UnixMs },
    Heartbeat { hbid: HeartbeatId },
    GotHeartbeat { hbid: HeartbeatId },
    LatencyOfHeartbeat { hbid: HeartbeatId, t_lat: UnixMs },
}
