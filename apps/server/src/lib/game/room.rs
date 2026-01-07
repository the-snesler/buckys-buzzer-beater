use std::{fmt, time::SystemTime};

use crate::{game::{Category, RoomResponse}, net::connection::{HostToken, RoomCode}, ws_msg::WsMsg, GameState, HostEntry, Player, PlayerEntry, PlayerId};


pub struct Room {
    pub code: RoomCode,
    pub host_token: HostToken,
    pub state: GameState,
    pub host: Option<HostEntry>,
    pub players: Vec<PlayerEntry>,
    pub categories: Vec<Category>,
    pub current_question: Option<(usize, usize)>, // (category_index, question_index)
    pub current_buzzer: Option<PlayerId>,
    pub last_activity: SystemTime,
    pub winner: Option<PlayerId>,
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

impl Room {
    pub fn new(code: RoomCode, host_token: HostToken) -> Self {
        Self {
            code,
            host_token,
            state: GameState::default(),
            host: None,
            players: Vec::new(),
            categories: Vec::new(),
            current_question: None,
            current_buzzer: None,
            last_activity: SystemTime::now(),
            winner: None,
        }
    }

    pub fn touch(&mut self) {
        self.last_activity = SystemTime::now();
    }
}

impl Room {
    fn determine_winner(&mut self) {
        if self.players.is_empty() {
            self.winner = None;
            tracing::debug!(room_code = %self.code, "No players, no winner");
            return;
        }

        let max_score = self
            .players
            .iter()
            .map(|p| p.player.score)
            .max()
            .unwrap_or(0);

        let winners: Vec<_> = self
            .players
            .iter()
            .filter(|p| p.player.score == max_score)
            .collect();

        self.winner = if winners.len() == 1 {
            let winner_id = Some(winners[0].player.pid);
            tracing::info!(
                room_code = %self.code,
                player_id = ?winner_id,
                player_name = %winners[0].player.name,
                score = max_score,
                "Winner determined"
            );
            winner_id
        } else {
            tracing::info!(
                room_code = %self.code,
                tie_count = winners.len(),
                score = max_score,
                "Game ended in a tie"
            );
            None
        };
    }

    fn build_game_state_msg(&self) -> WsMsg {
        let players: Vec<Player> = self.players.iter().map(|e| e.player.clone()).collect();

        WsMsg::GameState {
            state: self.state.clone(),
            categories: self.categories.clone(),
            players,
            current_question: self.current_question,
            current_buzzer: self.current_buzzer,
            winner: self.winner,
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

    #[tracing::instrument(skip(self, msg), fields(room_code = %self.code))]
    pub fn handle_message(&mut self, msg: &WsMsg, sender_id: Option<PlayerId>) -> RoomResponse {
        match msg {
            WsMsg::StartGame {} => {
                tracing::info!("Game started");
                self.state = GameState::Selection;
                RoomResponse::broadcast_state(self.build_game_state_msg())
                    .merge(self.build_all_player_states())
            }

            WsMsg::HostChoice {
                category_index,
                question_index,
            } => {
                tracing::debug!(category_index, question_index, "Host selected question");
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
                    tracing::info!(
                        player_id,
                        player_name = %player_entry.player.name,
                        "Player buzzed in"
                    );
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
                if let Some(sender_id) = sender_id
                    && let Some(entry) = self.players.iter_mut().find(|p| p.player.pid == sender_id)
                {
                    entry.on_know_dohb_recv(*hbid, *t_dohb_recv);
                }
                RoomResponse::new()
            }

            WsMsg::LatencyOfHeartbeat { hbid, t_lat } => {
                if let Some(sender_id) = sender_id
                    && let Some(entry) = self.players.iter_mut().find(|p| p.player.pid == sender_id)
                {
                    let t_lat_u32 = (*t_lat).try_into().unwrap_or(u32::MAX);
                    entry.on_latencyhb(*hbid, t_lat_u32);
                }
                RoomResponse::new()
            }

            WsMsg::EndGame {} => {
                self.determine_winner();
                tracing::info!(?self.winner, "Game ended");
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

        if let Some(buzzer_id) = self.current_buzzer
            && let Some(player) = self.players.iter_mut().find(|p| p.player.pid == buzzer_id)
        {
            if correct {
                player.player.score += question_value;
            } else {
                player.player.score -= question_value;
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
                self.determine_winner();
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
                self.determine_winner();
                GameState::GameEnd
            };
        }

        RoomResponse::broadcast_state(self.build_game_state_msg())
            .merge(self.build_all_player_states())
    }

    #[tracing::instrument(skip(self, msg), fields(room_code = %self.code))]
    pub async fn update(&mut self, msg: &WsMsg, pid: Option<PlayerId>) -> anyhow::Result<()> {
        tracing::trace!(?msg, ?pid, "Processing message");

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

