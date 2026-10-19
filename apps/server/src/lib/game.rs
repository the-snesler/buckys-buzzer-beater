use serde::{Deserialize, Serialize};

use crate::{api::messages::GameEvent, player::PlayerId};

pub mod room;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Question {
    pub question: String,
    pub answer: String,
    pub value: u32,
    #[serde(default)]
    pub answered: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Category {
    pub title: String,
    pub questions: Vec<Question>,
}

#[derive(Clone, Debug)]
pub struct RoomResponse {
    pub messages_to_host: Vec<GameEvent>,
    pub messages_to_players: Vec<GameEvent>,
    pub messages_to_specific: Vec<(PlayerId, GameEvent)>,
    /// Set when a buzz opened a new buzz window, which must be resolved once
    /// [`room::BUZZ_WINDOW`] has elapsed
    pub buzz_window_opened: Option<room::BuzzWindowId>,
    /// Set when every player should receive this event at the same moment, see
    /// [`room::Room::broadcast_witness`]
    pub witness: Option<GameEvent>,
}

impl Default for RoomResponse {
    fn default() -> Self {
        Self::new()
    }
}

impl RoomResponse {
    pub fn new() -> Self {
        Self {
            messages_to_host: vec![],
            messages_to_players: vec![],
            messages_to_specific: vec![],
            buzz_window_opened: None,
            witness: None,
        }
    }

    /// Sends `host_state` to the host and `public_state` to every player
    pub fn broadcast_state(host_state: GameEvent, public_state: GameEvent) -> Self {
        Self {
            messages_to_host: vec![host_state],
            messages_to_players: vec![public_state],
            messages_to_specific: vec![],
            buzz_window_opened: None,
            witness: None,
        }
    }

    pub fn to_host(msg: GameEvent) -> Self {
        Self {
            messages_to_host: vec![msg],
            messages_to_players: vec![],
            messages_to_specific: vec![],
            buzz_window_opened: None,
            witness: None,
        }
    }

    pub fn to_player(player_id: PlayerId, msg: GameEvent) -> Self {
        Self {
            messages_to_host: vec![],
            messages_to_players: vec![],
            messages_to_specific: vec![(player_id, msg)],
            buzz_window_opened: None,
            witness: None,
        }
    }

    pub fn merge(mut self, other: RoomResponse) -> Self {
        self.messages_to_host.extend(other.messages_to_host);
        self.messages_to_players.extend(other.messages_to_players);
        self.messages_to_specific.extend(other.messages_to_specific);
        self.buzz_window_opened = self.buzz_window_opened.or(other.buzz_window_opened);
        self.witness = self.witness.or(other.witness);
        self
    }
}

#[derive(Clone, Deserialize, Serialize, Debug, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub enum GameState {
    #[default]
    Start,
    Selection,
    QuestionReading,
    Answer,
    AnswerReveal,
    WaitingForBuzz,
    GameEnd,
}
