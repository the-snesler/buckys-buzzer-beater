use std::fmt;

use serde::{Deserialize, Serialize};
use tokio_mpmc::Sender;

use crate::{
    PlayerEntry,
    host::HostEntry,
    player::{Player, PlayerId},
    ws_msg::{WsMsg, WsMsgChannel},
};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Question {
    pub question: String,
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
            .field("players", &self.players)
            .field("category count", &self.categories.len())
            .field("current question", &self.current_question)
            .field("current buzzer", &self.current_buzzer)
            .finish()
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

    pub fn code(&self) -> &str {
        &self.code
    }

    pub fn host_token(&self) -> &str {
        &self.host_token
    }

    pub fn set_host(&mut self, host: HostEntry) {
        self.host = Some(host);
    }

    pub fn verify_host_token(&self, token: &str) -> bool {
        self.host_token == token
    }
}

impl Room {
    pub async fn broadcast_state(&self) -> anyhow::Result<()> {
        let players: Vec<Player> = self.players.iter().map(|e| e.player.clone()).collect();

        let msg = WsMsg::GameState {
            state: self.state.clone(),
            categories: self.categories.clone(),
            players: players.clone(),
            current_buzzer: self.current_buzzer,
            current_question: self.current_question,
        };

        if let Some(host) = &self.host {
            host.sender.send(msg.clone()).await?;
        }

        for player_entry in &self.players {
            let _ = player_entry.sender.send(msg.clone()).await;
        }

        Ok(())
    }
    pub async fn update(&mut self, msg: &WsMsg, pid: Option<PlayerId>) -> anyhow::Result<()> {
        match msg {
            WsMsg::StartGame {} => {
                self.state = GameState::Selection;
                self.broadcast_state().await?;
            }

            WsMsg::HostChoice {
                category_index,
                question_index,
            } => {
                self.current_question = Some((*category_index, *question_index));
                self.current_buzzer = None;
                // Reset all player buzz states
                for player in &mut self.players {
                    player.player.buzzed = false;
                }
                self.state = GameState::QuestionReading;
                self.broadcast_state().await?;
            }

            WsMsg::HostReady {} => {
                self.state = GameState::WaitingForBuzz;
                self.broadcast_state().await?;
            }

            WsMsg::Buzz {} => {
                if self.state == GameState::WaitingForBuzz {
                    if let Some(player_id) = pid {
                        if let Some(player_entry) =
                            self.players.iter_mut().find(|p| p.player.pid == player_id)
                        {
                            if !player_entry.player.buzzed {
                                player_entry.player.buzzed = true;
                                self.current_buzzer = Some(player_id);
                                self.state = GameState::Answer;

                                if let Some(host) = &self.host {
                                    let buzzed_msg = WsMsg::Buzzed {
                                        pid: player_id,
                                        name: player_entry.player.name.clone(),
                                    };
                                    host.sender.send(buzzed_msg).await?;
                                }

                                self.broadcast_state().await?;
                            }
                        }
                    }
                }
            }

            WsMsg::HostChecked { correct } => {
                if let Some((cat_idx, q_idx)) = self.current_question {
                    if *correct {
                        if let Some(category) = self.categories.get_mut(cat_idx) {
                            if let Some(question) = category.questions.get_mut(q_idx) {
                                question.answered = true;

                                if let Some(buzzer_id) = self.current_buzzer {
                                    if let Some(player) =
                                        self.players.iter_mut().find(|p| p.player.pid == buzzer_id)
                                    {
                                        player.player.score += question.value as i32;
                                    }
                                }
                            }
                        }
                        self.current_question = None;
                        self.current_buzzer = None;

                        if self.has_remaining_questions() {
                            self.state = GameState::Selection;
                        } else {
                            self.state = GameState::GameEnd;
                        }
                    } else {
                        if let Some(category) = self.categories.get(cat_idx) {
                            if let Some(question) = category.questions.get(q_idx) {
                                if let Some(buzzer_id) = self.current_buzzer {
                                    if let Some(player) = self.players.iter_mut().find(|p| p.player.pid == buzzer_id) {
                                        player.player.score -= question.value as i32;
                                    }
                                }
                            }
                        }
                        let any_can_buzz = self.players.iter().any(|p| !p.player.buzzed);
                        if any_can_buzz {
                            self.current_buzzer = None;
                            self.state = GameState::WaitingForBuzz;
                        } else {
                            if let Some(category) = self.categories.get_mut(cat_idx) {
                                if let Some(question) = category.questions.get_mut(q_idx) {
                                    question.answered = true;
                                }
                            }
                            self.current_question = None;
                            self.current_buzzer = None;

                            if self.has_remaining_questions() {
                                self.state = GameState::Selection;
                            } else {
                                self.state = GameState::GameEnd;
                            }
                        }
                    }
                }
                self.broadcast_state().await?;
            }

            WsMsg::EndGame {} => {
                self.state = GameState::GameEnd;
                self.broadcast_state().await?;
            }

            _ => {}
        }

        Ok(())
    }

    fn has_remaining_questions(&self) -> bool {
        self.categories.iter().any(|cat| {
            cat.questions.iter().any(|q| !q.answered)
        })
    }
}

#[derive(Clone, Deserialize, Serialize, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum GameState {
    Start,
    Selection,
    QuestionReading,
    Answer,
    WaitingForBuzz,
    GameEnd,
}

impl Default for GameState {
    fn default() -> Self {
        Self::Start
    }
}
