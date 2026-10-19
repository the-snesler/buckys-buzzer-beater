use std::{
    fmt,
    time::{Duration, SystemTime},
};

use crate::{
    HostEntry, Player, PlayerEntry, PlayerId,
    api::messages::{GameCommand, GameEvent, Sender},
    game::{Category, GameState, RoomResponse},
    net::connection::{HostToken, RoomCode, send_event},
};

/// How long the server keeps accepting buzzes after the first one arrives. Buzzes are
/// ranked by client-measured reaction time, so this needs to cover the spread of
/// players' network latencies.
pub const BUZZ_WINDOW: Duration = Duration::from_millis(250);

pub type BuzzWindowId = u64;

#[derive(Debug, PartialEq)]
pub struct BuzzWindow {
    pub id: BuzzWindowId,
    /// (player, reaction time in ms), in order of arrival
    pub buzzes: Vec<(PlayerId, u32)>,
}

pub struct Room {
    pub code: RoomCode,
    pub host_token: HostToken,
    pub state: GameState,
    pub host: Option<HostEntry>,
    pub players: Vec<PlayerEntry>,
    pub categories: Vec<Category>,
    pub current_question: Option<(usize, usize)>, // (category_index, question_index)
    pub current_buzzer: Option<PlayerId>,
    pub buzz_window: Option<BuzzWindow>,
    next_buzz_window_id: BuzzWindowId,
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
            .field("buzz window", &self.buzz_window)
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
            buzz_window: None,
            next_buzz_window_id: 0,
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

    /// Broadcasts a witnessed event to all players with latency compensation
    pub fn broadcast_witness(&self, event: GameEvent) {
        let _max_latency = self
            .players
            .iter()
            .filter_map(|p| p.latency().ok())
            .max()
            .unwrap_or(0);

        let witness_event = GameEvent::Witness {
            msg: Box::new(event),
        };

        for player in &self.players {
            let player_latency = player.latency().unwrap_or(0) as u64;
            let delay = Duration::from_millis(500u64.saturating_sub(player_latency));

            let sender = player.sender.clone();
            let event_clone = witness_event.clone();

            tokio::spawn(async move {
                tokio::time::sleep(delay).await;
                send_event(&sender, event_clone);
            });
        }
    }

    pub fn build_game_state_msg(&self) -> GameEvent {
        let players: Vec<Player> = self.players.iter().map(|e| e.player.clone()).collect();

        GameEvent::GameState {
            state: self.state.clone(),
            categories: self.categories.clone(),
            players,
            current_question: self.current_question,
            current_buzzer: self.current_buzzer,
            winner: self.winner,
        }
    }

    /// The game state every player may see. Players never receive [`GameEvent::GameState`],
    /// because it contains every clue's answer.
    pub fn build_public_state_msg(&self) -> GameEvent {
        GameEvent::PublicState {
            state: self.state.clone(),
            // Games have a single round, and nobody tracks whose pick it is yet
            round: 0,
            picker: None,
        }
    }

    pub fn build_player_state_msg(&self, player_id: PlayerId) -> Option<GameEvent> {
        let player = self.players.iter().find(|p| p.player.pid == player_id)?;
        let can_buzz = self.state == GameState::WaitingForBuzz && !player.player.buzzed;

        Some(GameEvent::PlayerState {
            pid: player.player.pid,
            buzzed: player.player.buzzed,
            score: player.player.score,
            can_buzz,
        })
    }

    pub fn handle_command(&mut self, cmd: &GameCommand, sender: Sender) -> RoomResponse {
        if !cmd.allowed_from(sender) {
            tracing::warn!(
                room_code = %self.code,
                ?sender,
                command = cmd.name(),
                "Ignoring command not allowed from this sender"
            );
            return RoomResponse::new();
        }
        if !self.allowed_in_state(cmd) {
            tracing::info!(
                room_code = %self.code,
                ?sender,
                command = cmd.name(),
                state = ?self.state,
                "Ignoring command not allowed in the current state"
            );
            return RoomResponse::new();
        }

        let response = match cmd {
            GameCommand::StartGame => {
                self.state = GameState::Selection;
                self.broadcast_state()
            }

            GameCommand::HostChoice {
                category_index,
                question_index,
            } => {
                let unanswered = self
                    .categories
                    .get(*category_index)
                    .and_then(|cat| cat.questions.get(*question_index))
                    .is_some_and(|q| !q.answered);
                if !unanswered {
                    tracing::info!(
                        room_code = %self.code,
                        category_index,
                        question_index,
                        "Ignoring choice of a missing or answered question"
                    );
                    return RoomResponse::new();
                }

                self.current_question = Some((*category_index, *question_index));
                self.current_buzzer = None;
                self.buzz_window = None;
                for player in &mut self.players {
                    player.player.buzzed = false;
                }
                self.state = GameState::QuestionReading;
                self.broadcast_state()
            }

            GameCommand::Buzz { reaction_ms } => {
                if let Sender::Player(player_id) = sender
                    && let Some(player_entry) =
                        self.players.iter().find(|p| p.player.pid == player_id)
                    && !player_entry.player.buzzed
                {
                    tracing::debug!(player_id, reaction_ms, "Player buzz received");
                    match &mut self.buzz_window {
                        Some(window) => {
                            if !window.buzzes.iter().any(|(pid, _)| *pid == player_id) {
                                window.buzzes.push((player_id, *reaction_ms));
                            }
                        }
                        None => {
                            self.next_buzz_window_id += 1;
                            let id = self.next_buzz_window_id;
                            self.buzz_window = Some(BuzzWindow {
                                id,
                                buzzes: vec![(player_id, *reaction_ms)],
                            });
                            return RoomResponse {
                                buzz_window_opened: Some(id),
                                ..RoomResponse::new()
                            };
                        }
                    }
                }
                RoomResponse::new()
            }
            GameCommand::HostReady => {
                self.buzz_window = None;
                self.state = GameState::WaitingForBuzz;
                self.broadcast_state()
            }

            GameCommand::HostChecked { correct } => self.handle_host_checked(*correct),

            GameCommand::HostSkip => self.handle_host_skip(),

            GameCommand::HostContinue => self.handle_host_continue(),

            GameCommand::Heartbeat { hbid, t_dohb_recv } => {
                if let Sender::Player(player_id) = sender
                    && let Some(entry) = self.players.iter_mut().find(|p| p.player.pid == player_id)
                {
                    entry.on_know_dohb_recv(*hbid, *t_dohb_recv);
                }
                RoomResponse::new()
            }

            GameCommand::LatencyOfHeartbeat { hbid, t_lat } => {
                if let Sender::Player(player_id) = sender
                    && let Some(entry) = self.players.iter_mut().find(|p| p.player.pid == player_id)
                {
                    let t_lat_u32 = (*t_lat).try_into().unwrap_or(u32::MAX);
                    entry.on_latencyhb(*hbid, t_lat_u32);
                }
                RoomResponse::new()
            }

            GameCommand::EndGame => {
                self.determine_winner();
                tracing::info!(?self.winner, "Game ended");
                self.state = GameState::GameEnd;
                self.broadcast_state()
            }
        };

        if cmd.should_witness() {
            RoomResponse {
                witness: Some(self.build_public_state_msg()),
                ..response
            }
        } else {
            response
        }
    }

    /// Whether `cmd` can be handled in the current state. Anything else is ignored, so a
    /// double-click or a stale command can't skip part of the game.
    fn allowed_in_state(&self, cmd: &GameCommand) -> bool {
        match cmd {
            GameCommand::StartGame => self.state == GameState::Start,
            GameCommand::EndGame => self.state != GameState::GameEnd,
            GameCommand::HostChoice { .. } => self.state == GameState::Selection,
            GameCommand::HostReady => self.state == GameState::QuestionReading,
            GameCommand::Buzz { .. } => self.state == GameState::WaitingForBuzz,
            GameCommand::HostChecked { .. } => self.state == GameState::Answer,
            GameCommand::HostSkip => matches!(
                self.state,
                GameState::QuestionReading | GameState::WaitingForBuzz
            ),
            GameCommand::HostContinue => self.state == GameState::AnswerReveal,
            GameCommand::Heartbeat { .. } | GameCommand::LatencyOfHeartbeat { .. } => true,
        }
    }

    /// Closes the buzz window and gives the question to the fastest reaction time.
    /// Does nothing if the window was already closed or the game has moved on.
    pub fn resolve_buzz_window(&mut self, id: BuzzWindowId) -> RoomResponse {
        let Some(window) = self.buzz_window.take_if(|w| w.id == id) else {
            return RoomResponse::new();
        };
        if self.state != GameState::WaitingForBuzz {
            return RoomResponse::new();
        }

        // Ties go to whoever reached the server first
        let Some(&(player_id, reaction_ms)) = window.buzzes.iter().min_by_key(|(_, r)| *r) else {
            return RoomResponse::new();
        };
        let Some(player_entry) = self.players.iter_mut().find(|p| p.player.pid == player_id) else {
            return RoomResponse::new();
        };

        tracing::info!(
            player_id,
            player_name = %player_entry.player.name,
            reaction_ms,
            contenders = window.buzzes.len(),
            "Player buzzed in"
        );
        player_entry.player.buzzed = true;
        self.current_buzzer = Some(player_id);
        self.state = GameState::Answer;

        let buzzed_msg = GameEvent::PlayerBuzzed {
            pid: player_id,
            name: player_entry.player.name.clone(),
        };

        RoomResponse::to_host(buzzed_msg).merge(self.broadcast_state())
    }

    pub fn handle_host_skip(&mut self) -> RoomResponse {
        let Some((cat_idx, q_idx)) = self.current_question else {
            return RoomResponse::new();
        };

        tracing::info!(
            category_index = cat_idx,
            question_index = q_idx,
            "Host skipped question"
        );

        if let Some(question) = self
            .categories
            .get_mut(cat_idx)
            .and_then(|cat| cat.questions.get_mut(q_idx))
        {
            question.answered = true;
        }

        self.state = GameState::AnswerReveal;

        self.broadcast_state()
    }

    fn handle_host_continue(&mut self) -> RoomResponse {
        tracing::info!("Host continuing from answer reveal");

        self.current_question = None;
        self.current_buzzer = None;

        for player in &mut self.players {
            player.player.buzzed = false;
        }

        self.state = if self.has_remaining_questions() {
            GameState::Selection
        } else {
            // No more questions, determine the winner and end
            self.determine_winner();
            GameState::GameEnd
        };

        tracing::debug!(
            next_state = ?self.state,
            winner = ?self.winner,
            "Transitioning after answer reveal"
        );

        self.broadcast_state()
    }

    /// Sends the full game state to the host, the public state to every player, and each
    /// player their own state
    fn broadcast_state(&self) -> RoomResponse {
        RoomResponse::broadcast_state(self.build_game_state_msg(), self.build_public_state_msg())
            .merge(self.build_all_player_states())
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

        if !correct && any_can_buzz {
            self.current_buzzer = None;
            self.state = GameState::WaitingForBuzz;
        } else {
            question.answered = true;
            self.current_buzzer = None;
            self.state = GameState::AnswerReveal;
        }

        self.broadcast_state()
    }

    fn has_remaining_questions(&self) -> bool {
        self.categories
            .iter()
            .any(|cat| cat.questions.iter().any(|q| !q.answered))
    }
}

#[cfg(test)]
mod tests {
    use tokio::sync::mpsc::channel;

    use crate::{game::Question, net::connection::PlayerToken};

    use super::*;

    fn create_test_room() -> Room {
        let mut room = Room::new(RoomCode::from("TEST".to_string()), HostToken::generate());

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
                    question: "What is 6 * 2?".to_string(),
                    answer: "12".to_string(),
                    value: 400,
                    answered: false,
                },
            ],
        }];

        room
    }

    fn add_test_player(room: &mut Room, pid: u32, name: &str) {
        let (tx, _rx) = channel(10);
        let player = PlayerEntry::new(
            Player::new(pid, name.to_string(), 0, false, PlayerToken::generate()),
            tx,
        );
        room.players.push(player);
    }

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

        room.handle_command(&GameCommand::HostChecked { correct: true }, Sender::Host);
        room.handle_command(&GameCommand::HostContinue, Sender::Host);

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

        room.handle_command(&GameCommand::EndGame {}, Sender::Host);

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

    #[test]
    fn test_game_state_transitions() {
        struct TestCase {
            name: &'static str,
            initial_state: GameState,
            setup: fn(&mut Room),
            command: GameCommand,
            sender: Sender,
            expected_state: GameState,
            assertions: fn(&Room),
        }

        let test_cases = vec![
            TestCase {
                name: "StartGame transitions to Selection",
                initial_state: GameState::Start,
                setup: |_| {},
                command: GameCommand::StartGame {},
                sender: Sender::Host,
                expected_state: GameState::Selection,
                assertions: |_| {},
            },
            TestCase {
                name: "HostChoice transitions to QuestionReading",
                initial_state: GameState::Selection,
                setup: |_| {},
                command: GameCommand::HostChoice {
                    category_index: 0,
                    question_index: 0,
                },
                sender: Sender::Host,
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
                command: GameCommand::HostChoice {
                    category_index: 0,
                    question_index: 0,
                },
                sender: Sender::Host,
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
                command: GameCommand::HostReady {},
                sender: Sender::Host,
                expected_state: GameState::WaitingForBuzz,
                assertions: |_| {},
            },
            TestCase {
                name: "Player buzz opens a buzz window",
                initial_state: GameState::WaitingForBuzz,
                setup: |room| {
                    add_test_player(room, 1, "AJ");
                },
                command: GameCommand::Buzz { reaction_ms: 200 },
                sender: Sender::Player(1),
                expected_state: GameState::WaitingForBuzz,
                assertions: |room| {
                    assert_eq!(room.current_buzzer, None);
                    let window = room.buzz_window.as_ref().expect("window should be open");
                    assert_eq!(window.buzzes, vec![(1, 200)]);
                },
            },
            TestCase {
                name: "Player cannot buzz twice",
                initial_state: GameState::WaitingForBuzz,
                setup: |room| {
                    add_test_player(room, 1, "AJ");
                    room.players[0].player.buzzed = true;
                },
                command: GameCommand::Buzz { reaction_ms: 200 },
                sender: Sender::Player(1),
                expected_state: GameState::WaitingForBuzz,
                assertions: |room| {
                    assert_eq!(room.current_buzzer, None);
                    assert!(room.buzz_window.is_none());
                },
            },
        ];

        for tc in test_cases {
            let mut room = create_test_room();
            room.state = tc.initial_state;
            (tc.setup)(&mut room);

            room.handle_command(&tc.command, tc.sender);

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

            room.handle_command(
                &GameCommand::HostChecked {
                    correct: tc.correct,
                },
                Sender::Host,
            );

            if room.state == GameState::AnswerReveal {
                room.handle_command(&GameCommand::HostContinue, Sender::Host);
            }

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

        room.handle_command(&GameCommand::HostSkip {}, Sender::Host);

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

        room.handle_command(&GameCommand::HostSkip {}, Sender::Host);

        assert_eq!(
            room.state,
            GameState::AnswerReveal,
            "Should first go to AnswerReveal"
        );

        room.handle_command(&GameCommand::HostContinue {}, Sender::Host);

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

        room.handle_command(&GameCommand::HostSkip {}, Sender::Host);

        assert_eq!(
            room.state,
            GameState::AnswerReveal,
            "Should first go to AnswerReveal"
        );

        room.handle_command(&GameCommand::HostContinue {}, Sender::Host);

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

        room.handle_command(&GameCommand::HostSkip {}, Sender::Host);
        room.handle_command(&GameCommand::HostContinue {}, Sender::Host);

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

        room.handle_command(&GameCommand::HostSkip {}, Sender::Host);

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

        let response = room.handle_command(&GameCommand::HostSkip {}, Sender::Host);

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
        room.handle_command(&GameCommand::HostChecked { correct: true }, Sender::Host);
        assert_eq!(
            room.state,
            GameState::AnswerReveal,
            "Should transition to AnswerReveal after correct answer"
        );
        assert_eq!(room.players[0].player.score, 200, "Score should be updated");

        // Host continues
        room.handle_command(&GameCommand::HostContinue {}, Sender::Host);

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
        room.handle_command(&GameCommand::HostChecked { correct: false }, Sender::Host);

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
        room.handle_command(&GameCommand::HostContinue {}, Sender::Host);

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
        room.handle_command(&GameCommand::HostSkip {}, Sender::Host);

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
        room.handle_command(&GameCommand::HostContinue {}, Sender::Host);

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
        room.handle_command(&GameCommand::HostChecked { correct: true }, Sender::Host);

        assert_eq!(
            room.state,
            GameState::AnswerReveal,
            "Should transition to AnswerReveal"
        );

        // Host continues from last question
        room.handle_command(&GameCommand::HostContinue {}, Sender::Host);

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
        room.handle_command(&GameCommand::HostChecked { correct: false }, Sender::Host);

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

    fn is_empty(response: &RoomResponse) -> bool {
        response.messages_to_host.is_empty()
            && response.messages_to_players.is_empty()
            && response.messages_to_specific.is_empty()
            && response.buzz_window_opened.is_none()
            && response.witness.is_none()
    }

    #[test]
    fn test_commands_from_wrong_role_are_ignored() {
        struct TestCase {
            name: &'static str,
            initial_state: GameState,
            command: GameCommand,
            sender: Sender,
        }

        let test_cases = vec![
            TestCase {
                name: "Player cannot start the game",
                initial_state: GameState::Start,
                command: GameCommand::StartGame,
                sender: Sender::Player(1),
            },
            TestCase {
                name: "Player cannot end the game",
                initial_state: GameState::Selection,
                command: GameCommand::EndGame,
                sender: Sender::Player(1),
            },
            TestCase {
                name: "Player cannot choose a question",
                initial_state: GameState::Selection,
                command: GameCommand::HostChoice {
                    category_index: 0,
                    question_index: 0,
                },
                sender: Sender::Player(1),
            },
            TestCase {
                name: "Player cannot open buzzing",
                initial_state: GameState::QuestionReading,
                command: GameCommand::HostReady,
                sender: Sender::Player(1),
            },
            TestCase {
                name: "Player cannot judge their own answer",
                initial_state: GameState::Answer,
                command: GameCommand::HostChecked { correct: true },
                sender: Sender::Player(1),
            },
            TestCase {
                name: "Player cannot skip a question",
                initial_state: GameState::WaitingForBuzz,
                command: GameCommand::HostSkip,
                sender: Sender::Player(1),
            },
            TestCase {
                name: "Player cannot continue from the answer reveal",
                initial_state: GameState::AnswerReveal,
                command: GameCommand::HostContinue,
                sender: Sender::Player(1),
            },
            TestCase {
                name: "Host cannot buzz",
                initial_state: GameState::WaitingForBuzz,
                command: GameCommand::Buzz { reaction_ms: 100 },
                sender: Sender::Host,
            },
        ];

        for tc in test_cases {
            let mut room = create_test_room();
            add_test_player(&mut room, 1, "AJ");
            room.state = tc.initial_state.clone();
            room.current_question = Some((0, 0));
            room.current_buzzer = Some(1);

            let response = room.handle_command(&tc.command, tc.sender);

            assert_eq!(room.state, tc.initial_state, "{}: state changed", tc.name);
            assert!(is_empty(&response), "{}: response not empty", tc.name);
            assert_eq!(
                room.players[0].player.score, 0,
                "{}: score changed",
                tc.name
            );
            assert!(
                !room.categories[0].questions[0].answered,
                "{}: question marked answered",
                tc.name
            );
            assert!(
                room.buzz_window.is_none(),
                "{}: buzz window opened",
                tc.name
            );
        }
    }

    #[test]
    fn test_commands_in_wrong_state_are_ignored() {
        struct TestCase {
            name: &'static str,
            initial_state: GameState,
            setup: fn(&mut Room),
            command: GameCommand,
            sender: Sender,
        }

        let test_cases = vec![
            TestCase {
                name: "StartGame after the game started",
                initial_state: GameState::Selection,
                setup: |_| {},
                command: GameCommand::StartGame,
                sender: Sender::Host,
            },
            TestCase {
                name: "HostChoice while a question is being read",
                initial_state: GameState::QuestionReading,
                setup: |_| {},
                command: GameCommand::HostChoice {
                    category_index: 0,
                    question_index: 1,
                },
                sender: Sender::Host,
            },
            TestCase {
                name: "HostChoice of an answered question",
                initial_state: GameState::Selection,
                setup: |room| room.categories[0].questions[1].answered = true,
                command: GameCommand::HostChoice {
                    category_index: 0,
                    question_index: 1,
                },
                sender: Sender::Host,
            },
            TestCase {
                name: "HostChoice of a missing category",
                initial_state: GameState::Selection,
                setup: |_| {},
                command: GameCommand::HostChoice {
                    category_index: 5,
                    question_index: 0,
                },
                sender: Sender::Host,
            },
            TestCase {
                name: "HostChoice of a missing question",
                initial_state: GameState::Selection,
                setup: |_| {},
                command: GameCommand::HostChoice {
                    category_index: 0,
                    question_index: 5,
                },
                sender: Sender::Host,
            },
            TestCase {
                name: "HostReady from Selection",
                initial_state: GameState::Selection,
                setup: |_| {},
                command: GameCommand::HostReady,
                sender: Sender::Host,
            },
            TestCase {
                name: "HostReady while a player is answering",
                initial_state: GameState::Answer,
                setup: |_| {},
                command: GameCommand::HostReady,
                sender: Sender::Host,
            },
            TestCase {
                name: "Buzz while the question is being read",
                initial_state: GameState::QuestionReading,
                setup: |_| {},
                command: GameCommand::Buzz { reaction_ms: 100 },
                sender: Sender::Player(1),
            },
            TestCase {
                name: "HostChecked while waiting for a buzz",
                initial_state: GameState::WaitingForBuzz,
                setup: |_| {},
                command: GameCommand::HostChecked { correct: true },
                sender: Sender::Host,
            },
            TestCase {
                name: "HostChecked twice",
                initial_state: GameState::AnswerReveal,
                setup: |_| {},
                command: GameCommand::HostChecked { correct: true },
                sender: Sender::Host,
            },
            TestCase {
                name: "HostSkip while a player is answering",
                initial_state: GameState::Answer,
                setup: |_| {},
                command: GameCommand::HostSkip,
                sender: Sender::Host,
            },
            TestCase {
                name: "HostSkip during the answer reveal",
                initial_state: GameState::AnswerReveal,
                setup: |_| {},
                command: GameCommand::HostSkip,
                sender: Sender::Host,
            },
            TestCase {
                name: "HostContinue from Selection",
                initial_state: GameState::Selection,
                setup: |_| {},
                command: GameCommand::HostContinue,
                sender: Sender::Host,
            },
            TestCase {
                name: "HostContinue while waiting for a buzz",
                initial_state: GameState::WaitingForBuzz,
                setup: |_| {},
                command: GameCommand::HostContinue,
                sender: Sender::Host,
            },
            TestCase {
                name: "EndGame after the game ended",
                initial_state: GameState::GameEnd,
                setup: |_| {},
                command: GameCommand::EndGame,
                sender: Sender::Host,
            },
        ];

        for tc in test_cases {
            let mut room = create_test_room();
            add_test_player(&mut room, 1, "AJ");
            room.state = tc.initial_state.clone();
            room.current_question = Some((0, 0));
            room.current_buzzer = Some(1);
            (tc.setup)(&mut room);

            let response = room.handle_command(&tc.command, tc.sender);

            assert_eq!(room.state, tc.initial_state, "{}: state changed", tc.name);
            assert!(is_empty(&response), "{}: response not empty", tc.name);
            assert_eq!(
                room.current_question,
                Some((0, 0)),
                "{}: current question changed",
                tc.name
            );
            assert_eq!(room.current_buzzer, Some(1), "{}: buzzer changed", tc.name);
            assert_eq!(
                room.players[0].player.score, 0,
                "{}: score changed",
                tc.name
            );
            assert!(
                room.buzz_window.is_none(),
                "{}: buzz window opened",
                tc.name
            );
        }
    }

    #[test]
    fn test_only_accepted_host_ready_is_witnessed() {
        let mut room = create_test_room();
        add_test_player(&mut room, 1, "AJ");
        room.state = GameState::QuestionReading;
        room.current_question = Some((0, 0));

        let response = room.handle_command(&GameCommand::HostReady, Sender::Player(1));
        assert!(response.witness.is_none(), "Player's HostReady is ignored");

        let response = room.handle_command(&GameCommand::HostReady, Sender::Host);
        assert!(response.witness.is_some(), "Host's HostReady is witnessed");

        let response = room.handle_command(&GameCommand::HostReady, Sender::Host);
        assert!(
            response.witness.is_none(),
            "A repeated HostReady is ignored"
        );
    }

    #[test]
    fn test_players_only_receive_public_state() {
        let mut room = create_test_room();
        add_test_player(&mut room, 1, "AJ");
        add_test_player(&mut room, 2, "Sam");
        for (i, question) in room.categories[0].questions.iter_mut().enumerate() {
            question.answer = format!("Secret answer {i}");
        }

        let mut responses = vec![room.handle_command(&GameCommand::StartGame, Sender::Host)];
        for question_index in 0..2 {
            responses.push(room.handle_command(
                &GameCommand::HostChoice {
                    category_index: 0,
                    question_index,
                },
                Sender::Host,
            ));
            responses.push(room.handle_command(&GameCommand::HostReady, Sender::Host));
            let opened = buzz(&mut room, 1, 200);
            let window = opened
                .buzz_window_opened
                .expect("Buzz should open a window");
            responses.push(opened);
            responses.push(room.resolve_buzz_window(window));
            responses.push(
                room.handle_command(&GameCommand::HostChecked { correct: false }, Sender::Host),
            );
            let opened = buzz(&mut room, 2, 200);
            let window = opened
                .buzz_window_opened
                .expect("Buzz should open a window");
            responses.push(opened);
            responses.push(room.resolve_buzz_window(window));
            responses.push(
                room.handle_command(&GameCommand::HostChecked { correct: true }, Sender::Host),
            );
            responses.push(room.handle_command(&GameCommand::HostContinue, Sender::Host));
        }
        assert_eq!(room.state, GameState::GameEnd);

        let secrets: Vec<String> = room
            .players
            .iter()
            .map(|p| p.player.token.to_string())
            .chain(
                room.categories[0]
                    .questions
                    .iter()
                    .map(|q| q.answer.clone()),
            )
            .collect();

        let mut public_states = 0;
        for response in &responses {
            let to_players = response
                .messages_to_players
                .iter()
                .chain(response.messages_to_specific.iter().map(|(_, msg)| msg))
                .chain(response.witness.iter());
            for msg in to_players {
                if matches!(msg, GameEvent::PublicState { .. }) {
                    public_states += 1;
                }
                assert!(
                    !matches!(msg, GameEvent::GameState { .. }),
                    "Player received GameState"
                );
                let json = serde_json::to_string(msg).expect("Failed to serialize event");
                for secret in &secrets {
                    assert!(
                        !json.contains(secret),
                        "{secret} leaked to a player: {json}"
                    );
                }
            }

            for msg in &response.messages_to_host {
                let json = serde_json::to_string(msg).expect("Failed to serialize event");
                assert!(!json.contains("token"), "Token sent to the host: {json}");
            }
        }
        assert!(public_states > 0, "Players should receive PublicState");
        assert!(
            responses.iter().any(|r| r
                .messages_to_host
                .iter()
                .any(|m| matches!(m, GameEvent::GameState { .. }))),
            "Host should still receive GameState"
        );
    }

    fn buzz(room: &mut Room, pid: PlayerId, reaction_ms: u32) -> RoomResponse {
        room.handle_command(&GameCommand::Buzz { reaction_ms }, Sender::Player(pid))
    }

    #[test]
    fn test_fastest_reaction_wins_buzz_window() {
        let mut room = create_test_room();
        add_test_player(&mut room, 1, "Player1");
        add_test_player(&mut room, 2, "Player2");
        add_test_player(&mut room, 3, "Player3");
        room.state = GameState::WaitingForBuzz;
        room.current_question = Some((0, 0));

        let id = buzz(&mut room, 1, 300)
            .buzz_window_opened
            .expect("First buzz should open a window");
        assert!(buzz(&mut room, 2, 150).buzz_window_opened.is_none());
        buzz(&mut room, 3, 220);

        room.resolve_buzz_window(id);

        assert_eq!(room.state, GameState::Answer);
        assert_eq!(
            room.current_buzzer,
            Some(2),
            "Fastest reaction should win even if it arrived later"
        );
        assert!(room.players[1].player.buzzed);
        assert!(
            !room.players[0].player.buzzed && !room.players[2].player.buzzed,
            "Losers should be able to buzz again if the winner is wrong"
        );
        assert!(room.buzz_window.is_none());
    }

    #[test]
    fn test_buzz_window_tie_goes_to_first_arrival() {
        let mut room = create_test_room();
        add_test_player(&mut room, 1, "Player1");
        add_test_player(&mut room, 2, "Player2");
        room.state = GameState::WaitingForBuzz;

        let id = buzz(&mut room, 2, 180)
            .buzz_window_opened
            .expect("Buzz should open a window");
        buzz(&mut room, 1, 180);
        room.resolve_buzz_window(id);

        assert_eq!(room.current_buzzer, Some(2));
    }

    #[test]
    fn test_repeat_buzz_in_window_keeps_first() {
        let mut room = create_test_room();
        add_test_player(&mut room, 1, "Player1");
        room.state = GameState::WaitingForBuzz;

        buzz(&mut room, 1, 400);
        buzz(&mut room, 1, 10);

        assert_eq!(
            room.buzz_window
                .as_ref()
                .expect("window should be open")
                .buzzes,
            vec![(1, 400)]
        );
    }

    #[test]
    fn test_stale_buzz_window_is_ignored() {
        let mut room = create_test_room();
        add_test_player(&mut room, 1, "Player1");
        room.state = GameState::WaitingForBuzz;
        room.current_question = Some((0, 0));

        let id = buzz(&mut room, 1, 200)
            .buzz_window_opened
            .expect("Buzz should open a window");
        room.handle_command(&GameCommand::HostSkip, Sender::Host);
        room.resolve_buzz_window(id);

        assert_eq!(room.state, GameState::AnswerReveal);
        assert_eq!(room.current_buzzer, None);
        assert!(room.buzz_window.is_none());

        // Resolving a window that was replaced does nothing
        room.state = GameState::WaitingForBuzz;
        let new_id = buzz(&mut room, 1, 200)
            .buzz_window_opened
            .expect("Buzz should open a window");
        room.resolve_buzz_window(id);
        assert_eq!(room.state, GameState::WaitingForBuzz);
        room.resolve_buzz_window(new_id);
        assert_eq!(room.current_buzzer, Some(1));
    }
}
