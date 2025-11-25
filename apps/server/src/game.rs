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

    pub async fn broadcast_player_states(&self) -> anyhow::Result<()> {
        let can_buzz_state = self.state == GameState::WaitingForBuzz;

        for player_entry in &self.players {
            let player_state_msg = WsMsg::PlayerState {
                pid: player_entry.player.pid,
                buzzed: player_entry.player.buzzed,
                score: player_entry.player.score,
                can_buzz: can_buzz_state && !player_entry.player.buzzed,
            };
            let _ = player_entry.sender.send(player_state_msg).await;
        }
        Ok(())
    }

    pub async fn update(&mut self, msg: &WsMsg, pid: Option<PlayerId>) -> anyhow::Result<()> {
        let own_entry: Option<&mut PlayerEntry> = if let Some(pid) = pid {
            let idx = self.players.iter().position(|p| p.player.pid == pid);
            idx.map(|i| &mut self.players[i])
        } else {
            None
        };
        match msg {
            WsMsg::StartGame {} => {
                self.state = GameState::Selection;
                self.broadcast_state().await?;
                self.broadcast_player_states().await?;
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
                self.broadcast_player_states().await?;
            }

            WsMsg::HostReady {} => {
                self.state = GameState::WaitingForBuzz;
                self.broadcast_state().await?;
                self.broadcast_player_states().await?;
            }

            WsMsg::Buzz {} => {
                if self.state == GameState::WaitingForBuzz
                    && let Some(player_id) = pid
                    && let Some(player_entry) =
                        self.players.iter_mut().find(|p| p.player.pid == player_id)
                    && !player_entry.player.buzzed
                {
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
                    self.broadcast_player_states().await?;
                }
            }

            WsMsg::HostChecked { correct } => {
                if let Some((cat_idx, q_idx)) = self.current_question {
                    if *correct {
                        if let Some(category) = self.categories.get_mut(cat_idx)
                            && let Some(question) = category.questions.get_mut(q_idx)
                        {
                            question.answered = true;

                            if let Some(buzzer_id) = self.current_buzzer
                                && let Some(player) =
                                    self.players.iter_mut().find(|p| p.player.pid == buzzer_id)
                            {
                                let incr: i32 = question.value.try_into()?;
                                player.player.score += incr;
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
                        if let Some(category) = self.categories.get(cat_idx)
                            && let Some(question) = category.questions.get(q_idx)
                            && let Some(buzzer_id) = self.current_buzzer
                            && let Some(player) =
                                self.players.iter_mut().find(|p| p.player.pid == buzzer_id)
                        {
                            let decr: i32 = question.value.try_into()?;
                            player.player.score -= decr;
                        }
                        let any_can_buzz = self.players.iter().any(|p| !p.player.buzzed);
                        if any_can_buzz {
                            self.current_buzzer = None;
                            self.state = GameState::WaitingForBuzz;
                        } else {
                            if let Some(category) = self.categories.get_mut(cat_idx)
                                && let Some(question) = category.questions.get_mut(q_idx)
                            {
                                question.answered = true;
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
                self.broadcast_player_states().await?;
            }

            WsMsg::EndGame {} => {
                self.state = GameState::GameEnd;
                self.broadcast_state().await?;
                self.broadcast_player_states().await?;
            }
            WsMsg::Heartbeat { hbid, t_dohb_recv } => {
                if let Some(entry) = own_entry {
                    if entry.on_know_dohb_recv(*hbid, *t_dohb_recv) {
                        println!("successfully updated dohb_recv");
                    } else {
                        println!("WARN: failed to update DoHeartbeat recv time");
                    }
                } else {
                    println!("WARN: own entry missing handling Heartbeat, continuing anyway");
                }
            }
            WsMsg::LatencyOfHeartbeat { hbid, t_lat } => {
                if let Some(entry) = own_entry {
                    if !entry.on_latencyhb(
                        *hbid,
                        (*t_lat).try_into().expect(
                            "LatencyOfHeartbeat latency of heartbeat exceeds 32-bit integer limit",
                        ),
                    ) {
                        println!(
                            "WARN: handling LatencyOfHeartbeat failed to update latencies, continuing anyway"
                        );
                    }
                } else {
                    println!(
                        "WARN: own entry missing while handling LatencyOfHeartbeat, continuing anyway"
                    );
                }
            }
            _ => {}
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
