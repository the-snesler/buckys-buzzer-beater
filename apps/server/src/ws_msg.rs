use serde::{Deserialize, Serialize};
use tokio::sync::mpsc::{Receiver, Sender};

use crate::{game::{Category, GameState}, player::{Player, PlayerId}, HeartbeatId, UnixMs};

pub type WsMsgChannel = (Sender<WsMsg>, Receiver<WsMsg>);

#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum WsMsg {
    Witness { msg: Box<WsMsg> },
    PlayerList(Vec<Player>),
    NewPlayer { pid: PlayerId, token: String },

    // Game State Broadcast
    GameState {
        state: GameState,
        categories: Vec<Category>,
        players: Vec<Player>,
        #[serde(rename = "currentQuestion")]
        current_question: Option<(usize, usize)>,
        #[serde(rename = "currentBuzzer")]
        current_buzzer: Option<PlayerId>,
    },

    // Host Actions
    #[serde(alias = "StartGame")]
    StartGame {},
    #[serde(alias = "EndGame")]
    EndGame {},
    HostChoice {
        #[serde(rename = "categoryIndex")]
        category_index: usize,
        #[serde(rename = "questionIndex")]
        question_index: usize,
    },
    #[serde(alias = "HostReady")]
    HostReady {},
    HostChecked { correct: bool },

    // Buzzer
    #[serde(alias = "BuzzEnable")]
    BuzzEnable {},
    #[serde(alias = "BuzzDisable")]
    BuzzDisable {},
    #[serde(alias = "Buzz")]
    Buzz {},
    Buzzed { pid: PlayerId, name: String },

    // Heartbeat
    DoHeartbeat { hbid: HeartbeatId, t_sent: UnixMs },
    Heartbeat { hbid: HeartbeatId },
    GotHeartbeat { hbid: HeartbeatId },
    LatencyOfHeartbeat { hbid: HeartbeatId, t_lat: UnixMs },
}
