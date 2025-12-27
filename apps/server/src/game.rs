use std::{fmt, time::SystemTime};

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

            WsMsg::HostSkip {} => self.handle_host_skip(),

            WsMsg::HostContinue {} => self.handle_host_continue(),

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
            self.state = GameState::AnswerReveal;
        } else if any_can_buzz {
            self.current_buzzer = None;
            self.state = GameState::WaitingForBuzz;
        } else {
            question.answered = true;
            self.state = GameState::AnswerReveal;
        }

        RoomResponse::broadcast_state(self.build_game_state_msg())
            .merge(self.build_all_player_states())
    }

    fn handle_host_skip(&mut self) -> RoomResponse {
        let Some((cat_idx, q_idx)) = self.current_question else {
            return RoomResponse::new();
        };

        tracing::info!(
            category_index = cat_idx,
            question_index = q_idx,
            "Host skipped question"
        );

        // Mark question as answered
        if let Some(question) = self
            .categories
            .get_mut(cat_idx)
            .and_then(|cat| cat.questions.get_mut(q_idx))
        {
            question.answered = true;
        }

        self.state = GameState::AnswerReveal;

        RoomResponse::broadcast_state(self.build_game_state_msg())
            .merge(self.build_all_player_states())
    }

    fn handle_host_continue(&mut self) -> RoomResponse {
        tracing::info!("Host continuing from answer reveal");

        // Clear current question and buzzer
        self.current_question = None;
        self.current_buzzer = None;

        for player in &mut self.players {
            player.player.buzzed = false;
        }

        // Transition to Selection or GameEnd
        self.state = if self.has_remaining_questions() {
            GameState::Selection
        } else {
            self.determine_winner();
            GameState::GameEnd
        };

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

#[derive(Clone, Deserialize, Serialize, Debug, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub enum GameState {
    #[default]
    Start,
    Selection,
    QuestionReading,
    Answer,
    WaitingForBuzz,
    AnswerReveal,
    GameEnd,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_winner_determined_on_game_end() {
        let mut room = create_test_room();
        add_test_player(&mut room, 1, "Winner");
        add_test_player(&mut room, 2, "Loser");

        room.players[0].player.score = 1000;
        room.players[1].player.score = 500;

        room.state = GameState::Answer;
        room.current_question = Some((0, 1));
        room.current_buzzer = Some(1);
        room.categories[0].questions[0].answered = true;

        room.handle_message(&WsMsg::HostChecked { correct: true }, None);

        assert_eq!(room.state, GameState::AnswerReveal);

        room.handle_message(&WsMsg::HostContinue {}, None);

        assert_eq!(room.state, GameState::GameEnd);
        assert_eq!(room.winner, Some(1), "Player 1 should be winner");
    }

    #[test]
    fn test_tie_results_in_no_winner() {
        let mut room = create_test_room();
        add_test_player(&mut room, 1, "Player1");
        add_test_player(&mut room, 2, "Player2");

        room.players[0].player.score = 1000;
        room.players[1].player.score = 1000;

        room.determine_winner();

        assert_eq!(room.winner, None, "Tie should result in no winner");
    }

    #[test]
    fn test_manual_end_game_determines_winner() {
        let mut room = create_test_room();
        add_test_player(&mut room, 1, "Winner");
        add_test_player(&mut room, 2, "Loser");

        room.players[0].player.score = 800;
        room.players[1].player.score = 200;

        room.handle_message(&WsMsg::EndGame {}, None);

        assert_eq!(room.state, GameState::GameEnd);
        assert_eq!(room.winner, Some(1));
    }

    #[test]
    fn test_negative_scores_winner() {
        let mut room = create_test_room();
        add_test_player(&mut room, 1, "LeastBad");
        add_test_player(&mut room, 2, "ReallyBad");

        room.players[0].player.score = -200;
        room.players[1].player.score = -1000;

        room.determine_winner();

        assert_eq!(
            room.winner,
            Some(1),
            "Player with higher negative score wins"
        );
    }

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
                expected_state: GameState::AnswerReveal,
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
                expected_state: GameState::AnswerReveal,
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
                expected_state: GameState::AnswerReveal,
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

    #[test]
    fn test_host_skip_marks_question_answered() {
        let mut room = create_test_room();
        add_test_player(&mut room, 1, "Player1");

        room.state = GameState::WaitingForBuzz;
        room.current_question = Some((0, 0));

        room.handle_message(&WsMsg::HostSkip {}, None);

        assert!(
            room.categories[0].questions[0].answered,
            "Skipped question should be marked as answered"
        );
        assert_eq!(
            room.state,
            GameState::AnswerReveal,
            "Should transition to AnswerReveal"
        );
    }

    #[test]
    fn test_host_skip_transitions_to_selection() {
        let mut room = create_test_room();
        add_test_player(&mut room, 1, "Player1");

        room.state = GameState::WaitingForBuzz;
        room.current_question = Some((0, 0));

        room.handle_message(&WsMsg::HostSkip {}, None);

        assert_eq!(
            room.state,
            GameState::AnswerReveal,
            "Should first go to AnswerReveal"
        );

        room.handle_message(&WsMsg::HostContinue {}, None);

        assert_eq!(
            room.state,
            GameState::Selection,
            "Should return to Selection when questions remain"
        );
    }

    #[test]
    fn test_host_skip_transitions_to_game_end() {
        let mut room = create_test_room();
        add_test_player(&mut room, 1, "Winner");
        add_test_player(&mut room, 2, "Loser");

        room.players[0].player.score = 500;
        room.players[1].player.score = 200;

        room.state = GameState::WaitingForBuzz;
        room.categories[0].questions[0].answered = true;
        room.current_question = Some((0, 1)); // Last question

        room.handle_message(&WsMsg::HostSkip {}, None);

        assert_eq!(
            room.state,
            GameState::AnswerReveal,
            "Should first go to AnswerReveal"
        );

        room.handle_message(&WsMsg::HostContinue {}, None);

        assert_eq!(
            room.state,
            GameState::GameEnd,
            "Should transition to GameEnd when no questions remain"
        );
        assert_eq!(
            room.winner,
            Some(1),
            "Should determine winner when game ends"
        );
    }

    #[test]
    fn test_host_skip_resets_buzz_states() {
        let mut room = create_test_room();
        add_test_player(&mut room, 1, "Player1");
        add_test_player(&mut room, 2, "Player2");

        room.state = GameState::WaitingForBuzz;
        room.current_question = Some((0, 0));
        room.players[0].player.buzzed = true;
        room.players[1].player.buzzed = true;
        room.current_buzzer = Some(1);

        room.handle_message(&WsMsg::HostSkip {}, None);
        room.handle_message(&WsMsg::HostContinue {}, None);

        assert!(
            !room.players[0].player.buzzed,
            "Player 1 buzz state should be reset"
        );
        assert!(
            !room.players[1].player.buzzed,
            "Player 2 buzz state should be reset"
        );
    }

    #[test]
    fn test_host_skip_does_not_affect_scores() {
        let mut room = create_test_room();
        add_test_player(&mut room, 1, "Player1");

        room.state = GameState::WaitingForBuzz;
        room.current_question = Some((0, 0));
        room.players[0].player.score = 100;

        room.handle_message(&WsMsg::HostSkip {}, None);

        assert_eq!(
            room.players[0].player.score, 100,
            "Skipping should not affect player scores"
        );
    }

    #[test]
    fn test_host_skip_without_current_question() {
        let mut room = create_test_room();

        room.state = GameState::Selection;
        room.current_question = None;

        let response = room.handle_message(&WsMsg::HostSkip {}, None);

        assert_eq!(
            room.state,
            GameState::Selection,
            "State should not change when there's no current question"
        );
        assert_eq!(
            response.messages_to_host.len(),
            0,
            "Should return empty response when there's no current question"
        );
    }

    #[test]
    fn test_answer_reveal_after_correct() {
        let mut room = create_test_room();
        add_test_player(&mut room, 1, "Player1");

        room.state = GameState::Answer;
        room.current_question = Some((0, 0));
        room.current_buzzer = Some(1);

        // Host marks answer correct
        room.handle_message(&WsMsg::HostChecked { correct: true }, None);

        assert_eq!(
            room.state,
            GameState::AnswerReveal,
            "Should transition to AnswerReveal after correct answer"
        );
        assert_eq!(room.players[0].player.score, 200, "Score should be updated");

        // Host continues
        room.handle_message(&WsMsg::HostContinue {}, None);

        assert_eq!(
            room.state,
            GameState::Selection,
            "Should transition to Selection after continue"
        );
        assert_eq!(
            room.current_question, None,
            "Current question should be cleared"
        );
        assert_eq!(
            room.current_buzzer, None,
            "Current buzzer should be cleared"
        );
    }

    #[test]
    fn test_answer_reveal_after_all_incorrect() {
        let mut room = create_test_room();
        add_test_player(&mut room, 1, "Player1");
        add_test_player(&mut room, 2, "Player2");

        room.state = GameState::Answer;
        room.current_question = Some((0, 0));
        room.current_buzzer = Some(1);
        room.players[0].player.buzzed = true;
        room.players[1].player.buzzed = true; // All players have buzzed

        // Host marks answer incorrect
        room.handle_message(&WsMsg::HostChecked { correct: false }, None);

        assert_eq!(
            room.state,
            GameState::AnswerReveal,
            "Should transition to AnswerReveal when all players buzzed incorrectly"
        );
        assert_eq!(
            room.players[0].player.score, -200,
            "Score should be deducted"
        );

        // Host continues
        room.handle_message(&WsMsg::HostContinue {}, None);

        assert_eq!(
            room.state,
            GameState::Selection,
            "Should transition to Selection after continue"
        );
    }

    #[test]
    fn test_answer_reveal_after_skip() {
        let mut room = create_test_room();
        add_test_player(&mut room, 1, "Player1");

        room.state = GameState::WaitingForBuzz;
        room.current_question = Some((0, 0));
        room.players[0].player.score = 100;

        // Host skips question
        room.handle_message(&WsMsg::HostSkip {}, None);

        assert_eq!(
            room.state,
            GameState::AnswerReveal,
            "Should transition to AnswerReveal after skip"
        );
        assert_eq!(
            room.players[0].player.score, 100,
            "Score should not change after skip"
        );
        assert!(
            room.categories[0].questions[0].answered,
            "Question should be marked as answered"
        );

        // Host continues
        room.handle_message(&WsMsg::HostContinue {}, None);

        assert_eq!(
            room.state,
            GameState::Selection,
            "Should transition to Selection after continue"
        );
    }

    #[test]
    fn test_answer_reveal_to_game_end() {
        let mut room = create_test_room();
        add_test_player(&mut room, 1, "Winner");
        add_test_player(&mut room, 2, "Loser");

        room.players[0].player.score = 500;
        room.players[1].player.score = 200;

        room.state = GameState::Answer;
        room.categories[0].questions[0].answered = true; // First question already answered
        room.current_question = Some((0, 1)); // Last question
        room.current_buzzer = Some(1);

        // Host marks answer correct
        room.handle_message(&WsMsg::HostChecked { correct: true }, None);

        assert_eq!(
            room.state,
            GameState::AnswerReveal,
            "Should transition to AnswerReveal"
        );

        // Host continues from last question
        room.handle_message(&WsMsg::HostContinue {}, None);

        assert_eq!(
            room.state,
            GameState::GameEnd,
            "Should transition to GameEnd when no questions remain"
        );
        assert_eq!(room.winner, Some(1), "Winner should be determined");
    }

    #[test]
    fn test_incorrect_stays_in_waiting_for_buzz() {
        let mut room = create_test_room();
        add_test_player(&mut room, 1, "Player1");
        add_test_player(&mut room, 2, "Player2");

        room.state = GameState::Answer;
        room.current_question = Some((0, 0));
        room.current_buzzer = Some(1);
        room.players[0].player.buzzed = true;
        room.players[1].player.buzzed = false; // Player 2 hasn't buzzed yet

        // Host marks answer incorrect
        room.handle_message(&WsMsg::HostChecked { correct: false }, None);

        assert_eq!(
            room.state,
            GameState::WaitingForBuzz,
            "Should stay in WaitingForBuzz when more players can buzz"
        );
        assert_eq!(
            room.current_buzzer, None,
            "Current buzzer should be cleared"
        );
        assert_eq!(
            room.current_question,
            Some((0, 0)),
            "Current question should remain"
        );
    }
}
