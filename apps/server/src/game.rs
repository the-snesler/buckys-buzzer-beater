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

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_room() -> Room {
        let mut room = Room::new("TEST".to_string(), "token".to_string());

        room.categories = vec![Category {
            title: "Test Category".to_string(),
            questions: vec![
                Question {
                    question: "What is 2+2?".to_string(),
                    answer: "4".to_string(),
                    value: 200,
                    answered: false,
                },
                Question {
                    question: "What is 6?".to_string(),
                    answer: "6".to_string(),
                    value: 400,
                    answered: false,
                },
            ],
        }];

        room
    }

    fn add_test_player(room: &mut Room, pid: u32, name: &str) {
        use tokio_mpmc::channel;
        let (tx, _rx) = channel(10);

        let player = PlayerEntry::new(
            Player::new(pid, name.to_string(), 0, false, "token".to_string()),
            tx,
        );
        room.players.push(player);
    }

    #[test]
    fn test_game_state_transitions() {
        struct TestCase {
            name: &'static str,
            initial_state: GameState,
            setup: fn(&mut Room),
            message: WsMsg,
            sender_id: Option<PlayerId>,
            expected_state: GameState,
            assertions: fn(&Room),
        }

        let test_cases = vec![
            TestCase {
                name: "StartGame transitions to Selection",
                initial_state: GameState::Start,
                setup: |_| {},
                message: WsMsg::StartGame {},
                sender_id: None,
                expected_state: GameState::Selection,
                assertions: |_| {},
            },
            TestCase {
                name: "HostChoice transitions to QuestionReading",
                initial_state: GameState::Selection,
                setup: |_| {},
                message: WsMsg::HostChoice {
                    category_index: 0,
                    question_index: 0,
                },
                sender_id: None,
                expected_state: GameState::QuestionReading,
                assertions: |room| {
                    assert_eq!(room.current_question, Some((0, 0)));
                    assert_eq!(room.current_buzzer, None);
                },
            },
            TestCase {
                name: "HostChoice resets player buzz states",
                initial_state: GameState::Selection,
                setup: |room| {
                    add_test_player(room, 1, "AJ");
                    add_test_player(room, 1, "Sam");
                    room.players[0].player.buzzed = true;
                    room.players[1].player.buzzed = true;
                },
                message: WsMsg::HostChoice {
                    category_index: 0,
                    question_index: 0,
                },
                sender_id: None,
                expected_state: GameState::QuestionReading,
                assertions: |room| {
                    assert!(!room.players[0].player.buzzed);
                    assert!(!room.players[1].player.buzzed);
                },
            },
            TestCase {
                name: "HostReady transitions to WaitingForBuzz",
                initial_state: GameState::QuestionReading,
                setup: |_| {},
                message: WsMsg::HostReady {},
                sender_id: None,
                expected_state: GameState::WaitingForBuzz,
                assertions: |_| {},
            },
            TestCase {
                name: "Player buzz transitions to Answer",
                initial_state: GameState::WaitingForBuzz,
                setup: |room| {
                    add_test_player(room, 1, "AJ");
                },
                message: WsMsg::Buzz {},
                sender_id: Some(1),
                expected_state: GameState::Answer,
                assertions: |room| {
                    assert_eq!(room.current_buzzer, Some(1));
                    assert!(room.players[0].player.buzzed);
                },
            },
            TestCase {
                name: "Player cannot buzz twice",
                initial_state: GameState::WaitingForBuzz,
                setup: |room| {
                    add_test_player(room, 1, "AJ");
                    room.players[0].player.buzzed = true;
                },
                message: WsMsg::Buzz {},
                sender_id: Some(1),
                expected_state: GameState::WaitingForBuzz,
                assertions: |room| {
                    assert_eq!(room.current_buzzer, None);
                },
            },
        ];

        for tc in test_cases {
            let mut room = create_test_room();
            room.state = tc.initial_state;
            (tc.setup)(&mut room);

            room.handle_message(&tc.message, tc.sender_id);

            assert_eq!(
                room.state, tc.expected_state,
                "Test case failed: {}",
                tc.name
            );
            (tc.assertions)(&room)
        }
    }

    #[test]
    fn test_scoring() {
        struct TestCase {
            name: &'static str,
            setup: fn(&mut Room),
            correct: bool,
            expected_score: i32,
            expected_state: GameState,
            question_answered: bool,
        }

        let test_cases = vec![
            TestCase {
                name: "Correct answer awards points",
                setup: |room| {
                    add_test_player(room, 1, "AJ");
                    room.state = GameState::Answer;
                    room.current_question = Some((0, 0));
                    room.current_buzzer = Some(1);
                },
                correct: true,
                expected_score: 200,
                expected_state: GameState::Selection,
                question_answered: true,
            },
            TestCase {
                name: "Incorrect answer deducts points",
                setup: |room| {
                    add_test_player(room, 1, "AJ");
                    add_test_player(room, 2, "Sam");
                    room.state = GameState::Answer;
                    room.current_question = Some((0, 0));
                    room.current_buzzer = Some(1);
                    room.players[0].player.buzzed = true;
                },
                correct: false,
                expected_score: -200,
                expected_state: GameState::WaitingForBuzz,
                question_answered: false,
            },
            TestCase {
                name: "All players wrong marks question answered",
                setup: |room| {
                    add_test_player(room, 1, "AJ");
                    add_test_player(room, 2, "Sam");
                    room.state = GameState::Answer;
                    room.current_question = Some((0, 0));
                    room.current_buzzer = Some(1);
                    room.players[0].player.buzzed = true;
                    room.players[1].player.buzzed = true;
                },
                correct: false,
                expected_score: -200,
                expected_state: GameState::Selection,
                question_answered: true,
            },
            TestCase {
                name: "Game ends when no questions remain",
                setup: |room| {
                    add_test_player(room, 1, "AJ");
                    room.state = GameState::Answer;
                    room.categories[0].questions[0].answered = true;
                    room.current_question = Some((0, 1));
                    room.current_buzzer = Some(1);
                },
                correct: true,
                expected_score: 400,
                expected_state: GameState::GameEnd,
                question_answered: true,
            },
        ];

        for tc in test_cases {
            let mut room = create_test_room();
            (tc.setup)(&mut room);

            let (cat_idx, q_idx) = room
                .current_question
                .expect("Failed to get current question");

            room.handle_message(
                &WsMsg::HostChecked {
                    correct: tc.correct,
                },
                None,
            );

            assert_eq!(
                room.players[0].player.score, tc.expected_score,
                "Test case failed (score): {}",
                tc.name
            );
            assert_eq!(
                room.state, tc.expected_state,
                "Test case failed (state): {}",
                tc.name
            );
            assert_eq!(
                room.categories[cat_idx].questions[q_idx].answered, tc.question_answered,
                "Test case failed (answered): {}",
                tc.name
            );
        }
    }
}
