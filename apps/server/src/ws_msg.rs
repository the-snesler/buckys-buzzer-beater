use serde::{Deserialize, Serialize};

use crate::{
    HeartbeatId, UnixMs,
    game::{Category, GameState},
    player::{Player, PlayerId},
};

#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum WsMsg {
    Witness {
        msg: Box<WsMsg>,
    },
    PlayerList(Vec<Player>),
    NewPlayer {
        pid: PlayerId,
        token: String,
    },

    // Game State Broadcast
    GameState {
        state: GameState,
        categories: Vec<Category>,
        players: Vec<Player>,
        #[serde(rename = "currentQuestion")]
        current_question: Option<(usize, usize)>,
        #[serde(rename = "currentBuzzer")]
        current_buzzer: Option<PlayerId>,
        winner: Option<PlayerId>,
    },

    PlayerState {
        pid: PlayerId,
        buzzed: bool,
        score: i32,
        #[serde(rename = "canBuzz")]
        can_buzz: bool,
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
    HostChecked {
        correct: bool,
    },
    #[serde(alias = "HostSkip")]
    HostSkip {},
    #[serde(alias = "HostContinue")]
    HostContinue {},

    // Buzzer
    #[serde(alias = "BuzzEnable")]
    BuzzEnable {},
    #[serde(alias = "BuzzDisable")]
    BuzzDisable {},
    #[serde(alias = "Buzz")]
    Buzz {},
    Buzzed {
        pid: PlayerId,
        name: String,
    },

    // Heartbeats
    DoHeartbeat {
        hbid: HeartbeatId,
        t_sent: UnixMs,
    },
    Heartbeat {
        hbid: HeartbeatId,
        t_dohb_recv: UnixMs,
    },
    GotHeartbeat {
        hbid: HeartbeatId,
    },
    LatencyOfHeartbeat {
        hbid: HeartbeatId,
        t_lat: UnixMs,
    },
}
