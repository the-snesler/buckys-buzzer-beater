use std::fmt;

use serde::{Deserialize, Serialize};

use crate::{
    PlayerEntry,
    host::HostEntry,
    player::{Player, PlayerId},
    ws_msg::WsMsg,
};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Question {
    pub text: String,
    pub answer: String,
    pub value: u32,
    pub answered: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Category {
    pub title: String,
    pub questions: Vec<Question>,
}

pub struct Room {
    pub code: String,
    pub host_token: String,
    pub state: GameState,
    pub host: Option<HostEntry>,
    pub players: Vec<PlayerEntry>,
    pub categories: Vec<Category>,
    pub current_question: Option<(usize, usize)>, // (category_index, question_index)
    pub current_buzzer: Option<PlayerId>,
}

impl fmt::Debug for Room {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Room")
            .field("code", &self.code)
            .field("host_token", &self.host_token)
            .field("host", &self.host)
            .field("state", &self.state)
            .field("players", &self.players)
            .field("category count", &self.categories.len())
            .field("current question", &self.current_question)
            .field("current buzzer", &self.current_buzzer)
            .finish()
    }
}

pub struct RoomResponse {
    pub messages_to_host: Vec<WsMsg>,
    pub messages_to_players: Vec<WsMsg>,
    pub messages_to_specific: Vec<(PlayerId, WsMsg)>,
}

impl RoomResponse {
    pub fn new() -> Self {
        Self {
            messages_to_host: vec![],
            messages_to_players: vec![],
            messages_to_specific: vec![],
        }
    }

    pub fn broadcast_state(state_msg: WsMsg) -> Self {
        Self {
            messages_to_host: vec![state_msg.clone()],
            messages_to_players: vec![state_msg],
            messages_to_specific: vec![],
        }
    }

    pub fn to_host(msg: WsMsg) -> Self {
        Self {
            messages_to_host: vec![msg],
            messages_to_players: vec![],
            messages_to_specific: vec![],
        }
    }

    pub fn to_player(player_id: PlayerId, msg: WsMsg) -> Self {
        Self {
            messages_to_host: vec![],
            messages_to_players: vec![],
            messages_to_specific: vec![(player_id, msg)],
        }
    }

    pub fn merge(mut self, other: RoomResponse) -> Self {
        self.messages_to_host.extend(other.messages_to_host);
        self.messages_to_players.extend(other.messages_to_players);
        self.messages_to_specific.extend(other.messages_to_specific);
        self
    }
}

impl Room {
    pub fn new(code: String, host_token: String) -> Self {
        Self {
            code,
            host_token,
            state: GameState::default(),
            host: None,
            players: Vec::new(),
            categories: Vec::new(),
            current_question: None,
            current_buzzer: None,
        }
    }
}

impl Room {
    fn build_game_state_msg(&self) -> WsMsg {
        let players: Vec<Player> = self.players.iter().map(|e| e.player.clone()).collect();

        WsMsg::GameState {
            state: self.state.clone(),
            categories: self.categories.clone(),
            players,
            current_question: self.current_question,
            current_buzzer: self.current_buzzer,
        }
    }

    fn build_player_state_msg(&self, player_id: PlayerId) -> Option<WsMsg> {
        let player = self.players.iter().find(|p| p.player.pid == player_id)?;
        let can_buzz = self.state == GameState::WaitingForBuzz && !player.player.buzzed;

        Some(WsMsg::PlayerState {
            pid: player.player.pid,
            buzzed: player.player.buzzed,
            score: player.player.score,
            can_buzz,
        })
    }

    pub fn handle_message(&mut self, msg: &WsMsg, sender_id: Option<PlayerId>) -> RoomResponse {
        match msg {
            WsMsg::StartGame {} => {
                self.state = GameState::Selection;
                RoomResponse::broadcast_state(self.build_game_state_msg())
                    .merge(self.build_all_player_states())
            }

            WsMsg::HostChoice {
                category_index,
                question_index,
            } => {
                self.current_question = Some((*category_index, *question_index));
                self.current_buzzer = None;
                for player in &mut self.players {
                    player.player.buzzed = false;
                }
                self.state = GameState::QuestionReading;
                RoomResponse::broadcast_state(self.build_game_state_msg())
                    .merge(self.build_all_player_states())
            }

            WsMsg::Buzz {} => {
                if self.state == GameState::WaitingForBuzz
                    && let Some(player_id) = sender_id
                    && let Some(player_entry) =
                        self.players.iter_mut().find(|p| p.player.pid == player_id)
                    && !player_entry.player.buzzed
                {
                    player_entry.player.buzzed = true;
                    self.current_buzzer = Some(player_id);
                    self.state = GameState::Answer;

                    let buzzed_msg = WsMsg::Buzzed {
                        pid: player_id,
                        name: player_entry.player.name.clone(),
                    };

                    return RoomResponse::to_host(buzzed_msg)
                        .merge(RoomResponse::broadcast_state(self.build_game_state_msg()))
                        .merge(self.build_all_player_states());
                }
                RoomResponse::new()
            }

            WsMsg::HostReady {} => {
                self.state = GameState::WaitingForBuzz;
                RoomResponse::broadcast_state(self.build_game_state_msg())
                    .merge(self.build_all_player_states())
            }

            WsMsg::HostChecked { correct } => self.handle_host_checked(*correct),

            WsMsg::Heartbeat { hbid, t_dohb_recv } => {
                if let Some(sender_id) = sender_id {
                    if let Some(entry) = self.players.iter_mut().find(|p| p.player.pid == sender_id)
                    {
                        entry.on_know_dohb_recv(*hbid, *t_dohb_recv);
                    }
                }
                RoomResponse::new()
            }

            WsMsg::LatencyOfHeartbeat { hbid, t_lat } => {
                if let Some(sender_id) = sender_id {
                    if let Some(entry) = self.players.iter_mut().find(|p| p.player.pid == sender_id)
                    {
                        let t_lat_u32 = (*t_lat).try_into().unwrap_or(u32::MAX);
                        entry.on_latencyhb(*hbid, t_lat_u32);
                    }
                }
                RoomResponse::new()
            }

            WsMsg::EndGame {} => {
                self.state = GameState::GameEnd;
                RoomResponse::broadcast_state(self.build_game_state_msg())
                    .merge(self.build_all_player_states())
            }

            _ => RoomResponse::new(),
        }
    }

    fn build_all_player_states(&self) -> RoomResponse {
        let mut response = RoomResponse::new();
        for player in &self.players {
            if let Some(msg) = self.build_player_state_msg(player.player.pid) {
                response.messages_to_specific.push((player.player.pid, msg));
            }
        }
        response
    }

    fn handle_host_checked(&mut self, correct: bool) -> RoomResponse {
        let Some((cat_idx, q_idx)) = self.current_question else {
            return RoomResponse::new();
        };

        let question = self
            .categories
            .get_mut(cat_idx)
            .and_then(|cat| cat.questions.get_mut(q_idx));

        let question_value = question.as_ref().map(|q| q.value as i32);
        let Some(question) = question else {
            return RoomResponse::new();
        };

        let Some(question_value) = question_value else {
            return RoomResponse::new();
        };

        if let Some(buzzer_id) = self.current_buzzer {
            if let Some(player) = self.players.iter_mut().find(|p| p.player.pid == buzzer_id) {
                if correct {
                    player.player.score += question_value;
                } else {
                    player.player.score -= question_value;
                }
            }
        }

        let any_can_buzz = self.players.iter().any(|p| !p.player.buzzed);

        if correct {
            question.answered = true;
            self.current_question = None;
            self.current_buzzer = None;
            self.state = if self.has_remaining_questions() {
                GameState::Selection
            } else {
                GameState::GameEnd
            };
        } else if any_can_buzz {
            self.current_buzzer = None;
            self.state = GameState::WaitingForBuzz;
        } else {
            question.answered = true;
            self.current_question = None;
            self.current_buzzer = None;
            self.state = if self.has_remaining_questions() {
                GameState::Selection
            } else {
                GameState::GameEnd
            };
        }

        RoomResponse::broadcast_state(self.build_game_state_msg())
            .merge(self.build_all_player_states())
    }

    pub async fn update(&mut self, msg: &WsMsg, pid: Option<PlayerId>) -> anyhow::Result<()> {
        let response = self.handle_message(msg, pid);

        for msg in response.messages_to_host {
            if let Some(host) = &self.host {
                let _ = host.sender.send(msg).await;
            }
        }

        for msg in response.messages_to_players {
            for player in &self.players {
                let _ = player.sender.send(msg.clone()).await;
            }
        }

        for (player_id, msg) in response.messages_to_specific {
            if let Some(player) = self.players.iter().find(|p| p.player.pid == player_id) {
                let _ = player.sender.send(msg).await;
            }
        }

        Ok(())
    }

    fn has_remaining_questions(&self) -> bool {
        self.categories
            .iter()
            .any(|cat| cat.questions.iter().any(|q| !q.answered))
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
    WaitingForBuzz,
    GameEnd,
}
