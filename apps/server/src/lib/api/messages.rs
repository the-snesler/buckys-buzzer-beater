use serde::{Deserialize, Serialize};

use crate::{GameState, Player, PlayerId, game::Category, net::connection::PlayerToken};

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(tag = "type")]
pub enum GameCommand {
    StartGame,
    EndGame,
    Buzz,
    HostReady,
    HostChoice {
        #[serde(rename = "categoryIndex")]
        category_index: usize,
        #[serde(rename = "questionIndex")]
        question_index: usize,
    },
    HostChecked {
        correct: bool,
    },
    HostSkip,
    HostContinue,
    Heartbeat {
        hbid: u32,
        #[serde(rename = "tDohbRecv")]
        t_dohb_recv: u64,
    },
    LatencyOfHeartbeat {
        hbid: u32,
        #[serde(rename = "tLat")]
        t_lat: u64,
    },
}

impl GameCommand {
    /// Helper to identify if a command should be echoed to others via witness system.
    pub fn should_witness(&self) -> bool {
        matches!(
            self,
            Self::HostReady
        )
    }
}

#[derive(Serialize, Clone, Debug)]
pub enum GameEvent {
    Witness {
        msg: Box<GameEvent>,
    },
    DoHeartbeat {
        hbid: u32,
        t_sent: u64,
    },
    GotHeartbeat {
        hbid: u32,
    },
    PlayerList(Vec<Player>),
    NewPlayer {
        pid: PlayerId,
        token: PlayerToken,
    },
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
    PlayerBuzzed {
        pid: PlayerId,
        name: String,
    },
}
