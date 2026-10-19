use std::{
    fmt,
    time::{Duration, SystemTime},
};

use crate::{
    HostEntry, Player, PlayerEntry, PlayerId,
    api::messages::{GameCommand, GameEvent, Sender, WagerRange},
    game::{Category, GameState, Question, RoomResponse},
    net::connection::{HostToken, RoomCode, send_event},
};

/// How long the server keeps accepting buzzes after the first one arrives. Buzzes are
/// ranked by client-measured reaction time, so this needs to cover the spread of
/// players' network latencies.
pub const BUZZ_WINDOW: Duration = Duration::from_millis(250);

pub type BuzzWindowId = u64;

/// The smallest Daily Double wager, as on TV
pub const DAILY_DOUBLE_MIN_WAGER: u32 = 5;

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
    /// The board of each round, in order: Jeopardy, then Double Jeopardy if the game has
    /// one. There is always at least one round.
    pub rounds: Vec<Vec<Category>>,
    /// Index into `rounds` of the round being played
    pub round_index: usize,
    pub current_question: Option<(usize, usize)>, // (category_index, question_index)
    pub current_buzzer: Option<PlayerId>,
    /// The player whose turn it is to choose the next clue. The host chooses who picks
    /// first, so this is `None` until then, and clues can't be chosen without it.
    pub picker: Option<PlayerId>,
    /// The picker's wager on the Daily Double being played. Set from their wager until the
    /// host moves on from the clue.
    pub dd_wager: Option<u32>,
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
            .field("round", &self.round_index)
            .field("round count", &self.rounds.len())
            .field("category count", &self.current_categories().len())
            .field("current question", &self.current_question)
            .field("current buzzer", &self.current_buzzer)
            .field("picker", &self.picker)
            .field("daily double wager", &self.dd_wager)
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
            rounds: vec![Vec::new()],
            round_index: 0,
            current_question: None,
            current_buzzer: None,
            picker: None,
            dd_wager: None,
            buzz_window: None,
            next_buzz_window_id: 0,
            last_activity: SystemTime::now(),
            winner: None,
        }
    }

    pub fn touch(&mut self) {
        self.last_activity = SystemTime::now();
    }

    /// The board of the round being played
    pub fn current_categories(&self) -> &[Category] {
        self.rounds.get(self.round_index).map_or(&[], Vec::as_slice)
    }

    pub fn current_categories_mut(&mut self) -> &mut [Category] {
        self.rounds
            .get_mut(self.round_index)
            .map_or(&mut [], Vec::as_mut_slice)
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
            categories: self.current_categories().to_vec(),
            round: self.round_index,
            round_count: self.rounds.len(),
            players,
            current_question: self.current_question,
            current_buzzer: self.current_buzzer,
            picker: self.picker,
            dd_wager: self.dd_wager,
            winner: self.winner,
        }
    }

    /// The game state every player may see. Players never receive [`GameEvent::GameState`],
    /// because it contains every clue's answer.
    pub fn build_public_state_msg(&self) -> GameEvent {
        GameEvent::PublicState {
            state: self.state.clone(),
            round: self.round_index,
            picker: self.picker,
        }
    }

    pub fn build_player_state_msg(&self, player_id: PlayerId) -> Option<GameEvent> {
        let player = self.players.iter().find(|p| p.player.pid == player_id)?;
        let can_buzz = self.state == GameState::WaitingForBuzz && !player.player.buzzed;
        // Only the picker plays a Daily Double, so only they are told about their wager
        let playing_daily_double = self.picker == Some(player_id)
            && (self.state == GameState::DailyDoubleWager || self.dd_wager.is_some());

        Some(GameEvent::PlayerState {
            pid: player.player.pid,
            buzzed: player.player.buzzed,
            score: player.player.score,
            can_buzz,
            wager_range: self.dd_wager_range(player_id),
            wager: self.dd_wager.filter(|_| playing_daily_double),
            prompt: self
                .current_category()
                .filter(|_| playing_daily_double)
                .map(|cat| cat.title.clone()),
        })
    }

    /// The category of the clue being played
    fn current_category(&self) -> Option<&Category> {
        let (cat_idx, _) = self.current_question?;
        self.current_categories().get(cat_idx)
    }

    /// The clue being played
    fn current_clue(&self) -> Option<&Question> {
        let (_, q_idx) = self.current_question?;
        self.current_category()?.questions.get(q_idx)
    }

    /// What the clue being played is worth: the Daily Double wager if there is one,
    /// otherwise the clue's value
    pub fn current_stake(&self) -> Option<u32> {
        self.dd_wager
            .or_else(|| self.current_clue().map(|q| q.value))
    }

    /// The wagers `player_id` may make on the Daily Double, if they are the one wagering:
    /// from $5 up to their score, or up to the most valuable clue on the board if that's
    /// more
    fn dd_wager_range(&self, player_id: PlayerId) -> Option<WagerRange> {
        if self.state != GameState::DailyDoubleWager || self.picker != Some(player_id) {
            return None;
        }
        let score = self
            .players
            .iter()
            .find(|p| p.player.pid == player_id)?
            .player
            .score;
        let board_max = self
            .current_categories()
            .iter()
            .flat_map(|cat| &cat.questions)
            .map(|q| q.value)
            .max()
            .unwrap_or(0);
        let max = u32::try_from(score)
            .unwrap_or(0)
            .max(board_max)
            // A board of tiny clues must still allow the minimum wager
            .max(DAILY_DOUBLE_MIN_WAGER);
        Some(WagerRange {
            min: DAILY_DOUBLE_MIN_WAGER,
            max,
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
                self.enter_round(0);
                self.broadcast_state()
            }

            GameCommand::HostChoice {
                category_index,
                question_index,
            } => {
                if self.picker.is_none() {
                    tracing::info!(
                        room_code = %self.code,
                        "Ignoring HostChoice before the host chose who picks first"
                    );
                    return RoomResponse::new();
                }
                let Some(question) = self
                    .current_categories()
                    .get(*category_index)
                    .and_then(|cat| cat.questions.get(*question_index))
                    .filter(|q| !q.answered)
                else {
                    tracing::info!(
                        room_code = %self.code,
                        category_index,
                        question_index,
                        "Ignoring choice of a missing or answered question"
                    );
                    return RoomResponse::new();
                };
                let daily_double = question.daily_double;

                self.clear_clue();
                self.current_question = Some((*category_index, *question_index));
                if daily_double {
                    tracing::info!(
                        room_code = %self.code,
                        picker = ?self.picker,
                        "Picker found a Daily Double"
                    );
                    self.state = GameState::DailyDoubleWager;
                } else {
                    self.state = GameState::QuestionReading;
                }
                self.broadcast_state()
            }

            GameCommand::SubmitWager { amount } => self.handle_submit_wager(sender, *amount),

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

            GameCommand::HostNextRound { round } => {
                // A stale second click must not end the round that the first one started
                if *round != self.round_index {
                    tracing::info!(
                        room_code = %self.code,
                        round,
                        current_round = self.round_index,
                        "Ignoring HostNextRound for a round that already ended"
                    );
                    return RoomResponse::new();
                }
                tracing::info!(
                    room_code = %self.code,
                    round = self.round_index,
                    "Host ended the round early"
                );
                self.finish_round();
                self.broadcast_state()
            }

            GameCommand::HostSetPicker { pid } => {
                if !self.players.iter().any(|p| p.player.pid == *pid) {
                    tracing::info!(
                        room_code = %self.code,
                        pid,
                        "Ignoring HostSetPicker for a player who isn't in the room"
                    );
                    return RoomResponse::new();
                }
                tracing::info!(room_code = %self.code, pid, "Host set the picker");
                self.picker = Some(*pid);
                self.broadcast_state()
            }

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
            GameCommand::HostSkip => match self.state {
                GameState::QuestionReading
                | GameState::WaitingForBuzz
                | GameState::DailyDoubleWager => true,
                // Nobody else can answer a Daily Double, so the host may skip it while the
                // picker answers. A normal clue goes back to buzzing after a wrong answer.
                GameState::Answer => self.dd_wager.is_some(),
                _ => false,
            },
            GameCommand::SubmitWager { .. } => self.state == GameState::DailyDoubleWager,
            GameCommand::HostContinue => self.state == GameState::AnswerReveal,
            GameCommand::HostNextRound { .. } | GameCommand::HostSetPicker { .. } => {
                self.state == GameState::Selection
            }
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
            .current_categories_mut()
            .get_mut(cat_idx)
            .and_then(|cat| cat.questions.get_mut(q_idx))
        {
            question.answered = true;
        }

        self.state = GameState::AnswerReveal;

        self.broadcast_state()
    }

    /// Accepts the picker's wager on a Daily Double and has them answer it. Nobody buzzes
    /// on a Daily Double.
    fn handle_submit_wager(&mut self, sender: Sender, amount: u32) -> RoomResponse {
        let Sender::Player(player_id) = sender else {
            return RoomResponse::new();
        };
        let Some(range) = self.dd_wager_range(player_id) else {
            tracing::info!(
                room_code = %self.code,
                player_id,
                picker = ?self.picker,
                "Rejecting a Daily Double wager from a player who isn't the picker"
            );
            return RoomResponse::to_player(
                player_id,
                GameEvent::Rejected {
                    reason: "Only the player who chose the Daily Double can wager on it"
                        .to_string(),
                },
            );
        };
        if !range.contains(amount) {
            tracing::info!(
                room_code = %self.code,
                player_id,
                amount,
                ?range,
                "Rejecting a Daily Double wager out of range"
            );
            return RoomResponse::to_player(
                player_id,
                GameEvent::Rejected {
                    reason: format!(
                        "Your wager must be between ${} and ${}",
                        range.min, range.max
                    ),
                },
            );
        }

        tracing::info!(room_code = %self.code, player_id, amount, "Daily Double wager");
        self.dd_wager = Some(amount);
        self.current_buzzer = Some(player_id);
        self.state = GameState::Answer;
        self.broadcast_state()
    }

    /// Forgets the clue that was being played, including any Daily Double wager on it
    fn clear_clue(&mut self) {
        self.current_question = None;
        self.current_buzzer = None;
        self.buzz_window = None;
        self.dd_wager = None;
        for player in &mut self.players {
            player.player.buzzed = false;
        }
    }

    fn handle_host_continue(&mut self) -> RoomResponse {
        tracing::info!("Host continuing from answer reveal");

        self.clear_clue();

        if self.has_remaining_questions() {
            self.state = GameState::Selection;
        } else {
            self.finish_round();
        }

        tracing::debug!(
            next_state = ?self.state,
            round = self.round_index,
            winner = ?self.winner,
            "Transitioning after answer reveal"
        );

        self.broadcast_state()
    }

    /// Moves on from the current round, whether or not its board is cleared: to the next
    /// round with clues to play if there is one, otherwise to the end of the game
    fn finish_round(&mut self) {
        self.enter_round(self.round_index + 1);
    }

    /// Starts the first round from `first` on that has a clue left to play, skipping
    /// empty rounds. If there is none, the game ends.
    fn enter_round(&mut self, first: usize) {
        self.clear_clue();

        let next = (first..self.rounds.len()).find(|&round| round_has_clues(&self.rounds[round]));

        if let Some(round) = next {
            self.round_index = round;
            // When the game starts, the host chooses who picks first. In later rounds, as
            // on TV, the player in last place picks first.
            if self.state != GameState::Start {
                self.picker = self.lowest_scorer();
            }
            tracing::info!(
                room_code = %self.code,
                round = self.round_index,
                picker = ?self.picker,
                "Starting round"
            );
            self.state = GameState::Selection;
        } else {
            self.determine_winner();
            self.state = GameState::GameEnd;
        }
    }

    /// The player with the lowest score. Ties go to the player who joined first (the lowest
    /// pid), so the choice doesn't depend on the order of `players`.
    fn lowest_scorer(&self) -> Option<PlayerId> {
        self.players
            .iter()
            .map(|p| (p.player.score, p.player.pid))
            .min()
            .map(|(_, pid)| pid)
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
        let Some(stake) = self.current_stake() else {
            return RoomResponse::new();
        };
        let stake = i32::try_from(stake).unwrap_or(i32::MAX);
        let daily_double = self.dd_wager.is_some();

        if let Some(buzzer_id) = self.current_buzzer
            && let Some(player) = self.players.iter_mut().find(|p| p.player.pid == buzzer_id)
        {
            if correct {
                player.player.score = player.player.score.saturating_add(stake);
                // Whoever answers correctly picks next. After a wrong answer, or if nobody
                // gets it, the picker stays the same.
                self.picker = Some(buzzer_id);
            } else {
                player.player.score = player.player.score.saturating_sub(stake);
            }
        }

        // Only the picker plays a Daily Double, so a wrong answer doesn't reopen buzzing
        let any_can_buzz = !daily_double && self.players.iter().any(|p| !p.player.buzzed);

        if !correct && any_can_buzz {
            self.current_buzzer = None;
            self.state = GameState::WaitingForBuzz;
        } else {
            if let Some(question) = self
                .current_categories_mut()
                .get_mut(cat_idx)
                .and_then(|cat| cat.questions.get_mut(q_idx))
            {
                question.answered = true;
            }
            self.current_buzzer = None;
            self.state = GameState::AnswerReveal;
        }

        self.broadcast_state()
    }

    /// Whether the current round's board has any clues left
    fn has_remaining_questions(&self) -> bool {
        round_has_clues(self.current_categories())
    }
}

/// Whether a round's board has any clues left to play
fn round_has_clues(categories: &[Category]) -> bool {
    categories
        .iter()
        .any(|cat| cat.questions.iter().any(|q| !q.answered))
}

#[cfg(test)]
mod tests {
    use tokio::sync::mpsc::channel;

    use crate::{game::Question, net::connection::PlayerToken};

    use super::*;

    fn create_test_room() -> Room {
        let mut room = Room::new(RoomCode::from("TEST".to_string()), HostToken::generate());

        room.rounds = vec![vec![Category {
            title: "Test Category".to_string(),
            questions: vec![
                Question {
                    question: "What is 2+2?".to_string(),
                    answer: "4".to_string(),
                    value: 200,
                    answered: false,
                    daily_double: false,
                },
                Question {
                    question: "What is 6 * 2?".to_string(),
                    answer: "12".to_string(),
                    value: 400,
                    answered: false,
                    daily_double: false,
                },
            ],
        }]];

        room
    }

    /// A test room whose second round is a Double Jeopardy board worth 400 and 800
    fn create_two_round_room() -> Room {
        let mut room = create_test_room();
        room.rounds.push(vec![Category {
            title: "Double Category".to_string(),
            questions: vec![
                Question {
                    question: "What is 10 * 10?".to_string(),
                    answer: "100".to_string(),
                    value: 400,
                    answered: false,
                    daily_double: false,
                },
                Question {
                    question: "What is 2 ^ 10?".to_string(),
                    answer: "1024".to_string(),
                    value: 800,
                    answered: false,
                    daily_double: false,
                },
            ],
        }]);
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
        room.rounds[0][0].questions[0].answered = true;

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
                setup: |room| {
                    add_test_player(room, 1, "AJ");
                    room.picker = Some(1);
                },
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
                    add_test_player(room, 2, "Sam");
                    room.players[0].player.buzzed = true;
                    room.players[1].player.buzzed = true;
                    room.picker = Some(1);
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
                    room.rounds[0][0].questions[0].answered = true;
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
                room.rounds[0][cat_idx].questions[q_idx].answered, tc.question_answered,
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
            room.rounds[0][0].questions[0].answered,
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
        room.rounds[0][0].questions[0].answered = true;
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
            room.rounds[0][0].questions[0].answered,
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
        room.rounds[0][0].questions[0].answered = true; // First question already answered
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
                name: "Player cannot end the round",
                initial_state: GameState::Selection,
                command: GameCommand::HostNextRound { round: 0 },
                sender: Sender::Player(1),
            },
            TestCase {
                name: "Player cannot choose who picks",
                initial_state: GameState::Selection,
                command: GameCommand::HostSetPicker { pid: 1 },
                sender: Sender::Player(1),
            },
            TestCase {
                name: "Host cannot wager",
                initial_state: GameState::DailyDoubleWager,
                command: GameCommand::SubmitWager { amount: 100 },
                sender: Sender::Host,
            },
            TestCase {
                name: "Host cannot buzz",
                initial_state: GameState::WaitingForBuzz,
                command: GameCommand::Buzz { reaction_ms: 100 },
                sender: Sender::Host,
            },
        ];

        for tc in test_cases {
            let mut room = create_two_round_room();
            add_test_player(&mut room, 1, "AJ");
            room.state = tc.initial_state.clone();
            room.current_question = Some((0, 0));
            room.current_buzzer = Some(1);

            let response = room.handle_command(&tc.command, tc.sender);

            assert_eq!(room.state, tc.initial_state, "{}: state changed", tc.name);
            assert!(is_empty(&response), "{}: response not empty", tc.name);
            assert_eq!(room.picker, None, "{}: picker changed", tc.name);
            assert_eq!(
                room.players[0].player.score, 0,
                "{}: score changed",
                tc.name
            );
            assert!(
                !room.rounds[0][0].questions[0].answered,
                "{}: question marked answered",
                tc.name
            );
            assert!(
                room.buzz_window.is_none(),
                "{}: buzz window opened",
                tc.name
            );
            assert_eq!(room.round_index, 0, "{}: round changed", tc.name);
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
                setup: |room| room.rounds[0][0].questions[1].answered = true,
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
            TestCase {
                name: "HostNextRound before the game started",
                initial_state: GameState::Start,
                setup: |_| {},
                command: GameCommand::HostNextRound { round: 0 },
                sender: Sender::Host,
            },
            TestCase {
                name: "HostNextRound while a question is being read",
                initial_state: GameState::QuestionReading,
                setup: |_| {},
                command: GameCommand::HostNextRound { round: 0 },
                sender: Sender::Host,
            },
            TestCase {
                name: "HostNextRound while waiting for a buzz",
                initial_state: GameState::WaitingForBuzz,
                setup: |_| {},
                command: GameCommand::HostNextRound { round: 0 },
                sender: Sender::Host,
            },
            TestCase {
                name: "HostNextRound while a player is answering",
                initial_state: GameState::Answer,
                setup: |_| {},
                command: GameCommand::HostNextRound { round: 0 },
                sender: Sender::Host,
            },
            TestCase {
                name: "HostNextRound during the answer reveal",
                initial_state: GameState::AnswerReveal,
                setup: |_| {},
                command: GameCommand::HostNextRound { round: 0 },
                sender: Sender::Host,
            },
            TestCase {
                name: "HostNextRound after the game ended",
                initial_state: GameState::GameEnd,
                setup: |_| {},
                command: GameCommand::HostNextRound { round: 0 },
                sender: Sender::Host,
            },
            TestCase {
                name: "HostChoice before the host chose who picks",
                initial_state: GameState::Selection,
                setup: |room| room.picker = None,
                command: GameCommand::HostChoice {
                    category_index: 0,
                    question_index: 1,
                },
                sender: Sender::Host,
            },
            TestCase {
                name: "HostSetPicker for a player who isn't in the room",
                initial_state: GameState::Selection,
                setup: |_| {},
                command: GameCommand::HostSetPicker { pid: 9 },
                sender: Sender::Host,
            },
            TestCase {
                name: "HostSetPicker before the game started",
                initial_state: GameState::Start,
                setup: |room| add_test_player(room, 2, "Sam"),
                command: GameCommand::HostSetPicker { pid: 2 },
                sender: Sender::Host,
            },
            TestCase {
                name: "HostSetPicker while a question is being read",
                initial_state: GameState::QuestionReading,
                setup: |room| add_test_player(room, 2, "Sam"),
                command: GameCommand::HostSetPicker { pid: 2 },
                sender: Sender::Host,
            },
            TestCase {
                name: "HostSetPicker while a player is answering",
                initial_state: GameState::Answer,
                setup: |room| add_test_player(room, 2, "Sam"),
                command: GameCommand::HostSetPicker { pid: 2 },
                sender: Sender::Host,
            },
            TestCase {
                name: "HostSetPicker during the answer reveal",
                initial_state: GameState::AnswerReveal,
                setup: |room| add_test_player(room, 2, "Sam"),
                command: GameCommand::HostSetPicker { pid: 2 },
                sender: Sender::Host,
            },
            TestCase {
                name: "HostSetPicker during a Daily Double",
                initial_state: GameState::DailyDoubleWager,
                setup: |room| add_test_player(room, 2, "Sam"),
                command: GameCommand::HostSetPicker { pid: 2 },
                sender: Sender::Host,
            },
            TestCase {
                name: "HostReady during a Daily Double wager",
                initial_state: GameState::DailyDoubleWager,
                setup: |_| {},
                command: GameCommand::HostReady,
                sender: Sender::Host,
            },
            TestCase {
                name: "Buzz during a Daily Double wager",
                initial_state: GameState::DailyDoubleWager,
                setup: |_| {},
                command: GameCommand::Buzz { reaction_ms: 100 },
                sender: Sender::Player(1),
            },
            TestCase {
                name: "HostChecked before the Daily Double wager",
                initial_state: GameState::DailyDoubleWager,
                setup: |_| {},
                command: GameCommand::HostChecked { correct: true },
                sender: Sender::Host,
            },
            TestCase {
                name: "HostContinue during a Daily Double wager",
                initial_state: GameState::DailyDoubleWager,
                setup: |_| {},
                command: GameCommand::HostContinue,
                sender: Sender::Host,
            },
            TestCase {
                name: "HostNextRound during a Daily Double wager",
                initial_state: GameState::DailyDoubleWager,
                setup: |_| {},
                command: GameCommand::HostNextRound { round: 0 },
                sender: Sender::Host,
            },
            TestCase {
                name: "SubmitWager before the game started",
                initial_state: GameState::Start,
                setup: |_| {},
                command: GameCommand::SubmitWager { amount: 100 },
                sender: Sender::Player(1),
            },
            TestCase {
                name: "SubmitWager during Selection",
                initial_state: GameState::Selection,
                setup: |_| {},
                command: GameCommand::SubmitWager { amount: 100 },
                sender: Sender::Player(1),
            },
            TestCase {
                name: "SubmitWager while a normal clue is being read",
                initial_state: GameState::QuestionReading,
                setup: |_| {},
                command: GameCommand::SubmitWager { amount: 100 },
                sender: Sender::Player(1),
            },
            TestCase {
                name: "SubmitWager while waiting for a buzz",
                initial_state: GameState::WaitingForBuzz,
                setup: |_| {},
                command: GameCommand::SubmitWager { amount: 100 },
                sender: Sender::Player(1),
            },
            TestCase {
                name: "SubmitWager again while answering the Daily Double",
                initial_state: GameState::Answer,
                setup: |room| room.dd_wager = Some(200),
                command: GameCommand::SubmitWager { amount: 100 },
                sender: Sender::Player(1),
            },
            TestCase {
                name: "SubmitWager during the answer reveal",
                initial_state: GameState::AnswerReveal,
                setup: |_| {},
                command: GameCommand::SubmitWager { amount: 100 },
                sender: Sender::Player(1),
            },
            TestCase {
                name: "SubmitWager after the game ended",
                initial_state: GameState::GameEnd,
                setup: |_| {},
                command: GameCommand::SubmitWager { amount: 100 },
                sender: Sender::Player(1),
            },
            TestCase {
                name: "HostSetPicker after the game ended",
                initial_state: GameState::GameEnd,
                setup: |room| add_test_player(room, 2, "Sam"),
                command: GameCommand::HostSetPicker { pid: 2 },
                sender: Sender::Host,
            },
        ];

        for tc in test_cases {
            let mut room = create_two_round_room();
            add_test_player(&mut room, 1, "AJ");
            room.state = tc.initial_state.clone();
            room.current_question = Some((0, 0));
            room.current_buzzer = Some(1);
            room.picker = Some(1);
            (tc.setup)(&mut room);
            let picker = room.picker;
            let dd_wager = room.dd_wager;

            let response = room.handle_command(&tc.command, tc.sender);

            assert_eq!(room.state, tc.initial_state, "{}: state changed", tc.name);
            assert!(is_empty(&response), "{}: response not empty", tc.name);
            assert_eq!(room.picker, picker, "{}: picker changed", tc.name);
            assert_eq!(room.dd_wager, dd_wager, "{}: wager changed", tc.name);
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
            assert_eq!(room.round_index, 0, "{}: round changed", tc.name);
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
        for (i, question) in room.rounds[0][0].questions.iter_mut().enumerate() {
            question.answer = format!("Secret answer {i}");
        }

        let mut responses = vec![
            room.handle_command(&GameCommand::StartGame, Sender::Host),
            room.handle_command(&GameCommand::HostSetPicker { pid: 1 }, Sender::Host),
        ];
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
            .chain(room.rounds[0][0].questions.iter().map(|q| q.answer.clone()))
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

    fn answer_round(room: &mut Room, round: usize) {
        for category in &mut room.rounds[round] {
            for question in &mut category.questions {
                question.answered = true;
            }
        }
    }

    #[test]
    fn test_round_transitions() {
        struct TestCase {
            name: &'static str,
            create_room: fn() -> Room,
            initial_state: GameState,
            setup: fn(&mut Room),
            command: GameCommand,
            expected_state: GameState,
            expected_round: usize,
        }

        let test_cases = vec![
            TestCase {
                name: "Clues left in the round returns to the board",
                create_room: create_two_round_room,
                initial_state: GameState::AnswerReveal,
                setup: |room| {
                    room.rounds[0][0].questions[0].answered = true;
                    room.current_question = Some((0, 0));
                },
                command: GameCommand::HostContinue,
                expected_state: GameState::Selection,
                expected_round: 0,
            },
            TestCase {
                name: "Clearing the Jeopardy board starts Double Jeopardy",
                create_room: create_two_round_room,
                initial_state: GameState::AnswerReveal,
                setup: |room| {
                    answer_round(room, 0);
                    room.current_question = Some((0, 1));
                },
                command: GameCommand::HostContinue,
                expected_state: GameState::Selection,
                expected_round: 1,
            },
            TestCase {
                name: "Clearing the Double Jeopardy board ends the game",
                create_room: create_two_round_room,
                initial_state: GameState::AnswerReveal,
                setup: |room| {
                    room.round_index = 1;
                    answer_round(room, 1);
                    room.current_question = Some((0, 1));
                },
                command: GameCommand::HostContinue,
                expected_state: GameState::GameEnd,
                expected_round: 1,
            },
            TestCase {
                name: "Clearing the board of a single-round game ends the game",
                create_room: create_test_room,
                initial_state: GameState::AnswerReveal,
                setup: |room| {
                    answer_round(room, 0);
                    room.current_question = Some((0, 1));
                },
                command: GameCommand::HostContinue,
                expected_state: GameState::GameEnd,
                expected_round: 0,
            },
            TestCase {
                name: "HostNextRound with clues left starts Double Jeopardy",
                create_room: create_two_round_room,
                initial_state: GameState::Selection,
                setup: |_| {},
                command: GameCommand::HostNextRound { round: 0 },
                expected_state: GameState::Selection,
                expected_round: 1,
            },
            TestCase {
                name: "HostNextRound in the last round ends the game",
                create_room: create_two_round_room,
                initial_state: GameState::Selection,
                setup: |room| room.round_index = 1,
                command: GameCommand::HostNextRound { round: 1 },
                expected_state: GameState::GameEnd,
                expected_round: 1,
            },
            TestCase {
                name: "HostNextRound in a single-round game ends the game",
                create_room: create_test_room,
                initial_state: GameState::Selection,
                setup: |_| {},
                command: GameCommand::HostNextRound { round: 0 },
                expected_state: GameState::GameEnd,
                expected_round: 0,
            },
            TestCase {
                name: "StartGame starts Jeopardy",
                create_room: create_two_round_room,
                initial_state: GameState::Start,
                setup: |_| {},
                command: GameCommand::StartGame,
                expected_state: GameState::Selection,
                expected_round: 0,
            },
            TestCase {
                name: "StartGame skips an empty Jeopardy board",
                create_room: create_two_round_room,
                initial_state: GameState::Start,
                setup: |room| room.rounds[0].clear(),
                command: GameCommand::StartGame,
                expected_state: GameState::Selection,
                expected_round: 1,
            },
            TestCase {
                name: "StartGame skips a Jeopardy board with every clue answered",
                create_room: create_two_round_room,
                initial_state: GameState::Start,
                setup: |room| answer_round(room, 0),
                command: GameCommand::StartGame,
                expected_state: GameState::Selection,
                expected_round: 1,
            },
            TestCase {
                name: "StartGame with no clues in any round ends the game",
                create_room: create_two_round_room,
                initial_state: GameState::Start,
                setup: |room| {
                    room.rounds[0].clear();
                    room.rounds[1].clear();
                },
                command: GameCommand::StartGame,
                expected_state: GameState::GameEnd,
                expected_round: 0,
            },
            TestCase {
                name: "Clearing the Jeopardy board skips a Double Jeopardy without clues",
                create_room: create_two_round_room,
                initial_state: GameState::AnswerReveal,
                setup: |room| {
                    room.rounds[1][0].questions.clear();
                    answer_round(room, 0);
                    room.current_question = Some((0, 1));
                },
                command: GameCommand::HostContinue,
                expected_state: GameState::GameEnd,
                expected_round: 0,
            },
            TestCase {
                name: "HostNextRound skips a Double Jeopardy with every clue answered",
                create_room: create_two_round_room,
                initial_state: GameState::Selection,
                setup: |room| answer_round(room, 1),
                command: GameCommand::HostNextRound { round: 0 },
                expected_state: GameState::GameEnd,
                expected_round: 0,
            },
        ];

        for tc in test_cases {
            let mut room = (tc.create_room)();
            add_test_player(&mut room, 1, "AJ");
            add_test_player(&mut room, 2, "Sam");
            room.players[0].player.score = 600;
            room.players[0].player.buzzed = true;
            room.current_buzzer = Some(1);
            room.buzz_window = Some(BuzzWindow {
                id: 1,
                buzzes: vec![(2, 300)],
            });
            room.state = tc.initial_state;
            (tc.setup)(&mut room);

            let response = room.handle_command(&tc.command, Sender::Host);

            assert_eq!(room.state, tc.expected_state, "{}: state", tc.name);
            assert_eq!(room.round_index, tc.expected_round, "{}: round", tc.name);
            assert_eq!(room.current_question, None, "{}: question", tc.name);
            assert_eq!(room.current_buzzer, None, "{}: buzzer", tc.name);
            assert!(room.buzz_window.is_none(), "{}: buzz window", tc.name);
            assert!(
                room.players.iter().all(|p| !p.player.buzzed),
                "{}: buzz states not reset",
                tc.name
            );
            assert_eq!(
                room.players[0].player.score, 600,
                "{}: score changed",
                tc.name
            );
            if tc.expected_state == GameState::GameEnd {
                assert_eq!(room.winner, Some(1), "{}: winner", tc.name);
            }
            assert!(
                response.messages_to_players.iter().any(|msg| matches!(
                    msg,
                    GameEvent::PublicState { round, .. } if *round == tc.expected_round
                )),
                "{}: players not told the round",
                tc.name
            );
        }
    }

    #[test]
    fn test_stale_host_next_round_is_ignored() {
        let mut room = create_two_round_room();
        add_test_player(&mut room, 1, "AJ");
        room.state = GameState::Selection;

        room.handle_command(&GameCommand::HostNextRound { round: 0 }, Sender::Host);
        assert_eq!(room.round_index, 1);

        // A second click on End Round for Jeopardy arrives after Double Jeopardy started
        let response = room.handle_command(&GameCommand::HostNextRound { round: 0 }, Sender::Host);
        assert!(is_empty(&response), "Stale HostNextRound should be ignored");
        assert_eq!(room.state, GameState::Selection);
        assert_eq!(room.round_index, 1);

        room.handle_command(&GameCommand::HostNextRound { round: 1 }, Sender::Host);
        assert_eq!(room.state, GameState::GameEnd);
    }

    /// Plays one clue to the end: `pid` buzzes in and is judged, then the host continues.
    /// Returns the response to the host continuing.
    fn play_clue(
        room: &mut Room,
        category_index: usize,
        question_index: usize,
        pid: PlayerId,
        correct: bool,
    ) -> RoomResponse {
        room.handle_command(
            &GameCommand::HostChoice {
                category_index,
                question_index,
            },
            Sender::Host,
        );
        room.handle_command(&GameCommand::HostReady, Sender::Host);
        let window = buzz(room, pid, 200)
            .buzz_window_opened
            .expect("Buzz should open a window");
        room.resolve_buzz_window(window);
        room.handle_command(&GameCommand::HostChecked { correct }, Sender::Host);
        assert_eq!(room.state, GameState::AnswerReveal);
        room.handle_command(&GameCommand::HostContinue, Sender::Host)
    }

    /// The rounds reported to the players and to the host in `response`, as
    /// `(public rounds, (round, round count, first category title) per host state)`
    fn reported_rounds(response: &RoomResponse) -> (Vec<usize>, Vec<(usize, usize, String)>) {
        let public = response
            .messages_to_players
            .iter()
            .filter_map(|msg| match msg {
                GameEvent::PublicState { round, .. } => Some(*round),
                _ => None,
            })
            .collect();
        let host = response
            .messages_to_host
            .iter()
            .filter_map(|msg| match msg {
                GameEvent::GameState {
                    round,
                    round_count,
                    categories,
                    ..
                } => Some((
                    *round,
                    *round_count,
                    categories
                        .first()
                        .map(|c| c.title.clone())
                        .unwrap_or_default(),
                )),
                _ => None,
            })
            .collect();
        (public, host)
    }

    #[test]
    fn test_two_round_game() {
        let mut room = create_two_round_room();
        add_test_player(&mut room, 1, "AJ");
        add_test_player(&mut room, 2, "Sam");

        let response = room.handle_command(&GameCommand::StartGame, Sender::Host);
        assert_eq!(
            reported_rounds(&response),
            (vec![0], vec![(0, 2, "Test Category".to_string())])
        );
        room.handle_command(&GameCommand::HostSetPicker { pid: 1 }, Sender::Host);

        play_clue(&mut room, 0, 0, 1, true);
        let response = play_clue(&mut room, 0, 1, 2, true);

        assert_eq!(room.state, GameState::Selection);
        assert_eq!(room.round_index, 1);
        assert_eq!(
            reported_rounds(&response),
            (vec![1], vec![(1, 2, "Double Category".to_string())]),
            "Everyone should be told Double Jeopardy started"
        );
        assert_eq!(room.players[0].player.score, 200, "Scores carry over");
        assert_eq!(room.players[1].player.score, 400, "Scores carry over");
        assert!(
            room.rounds[1][0].questions.iter().all(|q| !q.answered),
            "The Double Jeopardy board starts full"
        );

        // Clue indices now refer to the Double Jeopardy board
        play_clue(&mut room, 0, 1, 1, true);
        assert_eq!(room.players[0].player.score, 1000);
        assert!(room.rounds[1][0].questions[1].answered);
        assert!(!room.rounds[1][0].questions[0].answered);

        let response = room.handle_command(&GameCommand::HostNextRound { round: 1 }, Sender::Host);
        assert_eq!(room.state, GameState::GameEnd);
        assert_eq!(room.winner, Some(1));
        assert_eq!(
            reported_rounds(&response),
            (vec![1], vec![(1, 2, "Double Category".to_string())])
        );
        assert_eq!(room.players[0].player.score, 1000);
        assert_eq!(room.players[1].player.score, 400);
    }

    /// The picker reported to the players in `response`'s public states
    fn reported_pickers(response: &RoomResponse) -> Vec<Option<PlayerId>> {
        response
            .messages_to_players
            .iter()
            .filter_map(|msg| match msg {
                GameEvent::PublicState { picker, .. } => Some(*picker),
                _ => None,
            })
            .collect()
    }

    fn reported_host_pickers(response: &RoomResponse) -> Vec<Option<PlayerId>> {
        response
            .messages_to_host
            .iter()
            .filter_map(|msg| match msg {
                GameEvent::GameState { picker, .. } => Some(*picker),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn test_host_chooses_who_picks_first() {
        let mut room = create_test_room();
        add_test_player(&mut room, 1, "AJ");
        add_test_player(&mut room, 2, "Sam");

        let response = room.handle_command(&GameCommand::StartGame, Sender::Host);
        assert_eq!(room.picker, None, "Nobody picks until the host says so");
        assert_eq!(reported_pickers(&response), vec![None]);

        let choice = GameCommand::HostChoice {
            category_index: 0,
            question_index: 0,
        };
        let response = room.handle_command(&choice, Sender::Host);
        assert!(is_empty(&response), "HostChoice needs a picker");
        assert_eq!(room.state, GameState::Selection);
        assert_eq!(room.current_question, None);

        let response = room.handle_command(&GameCommand::HostSetPicker { pid: 2 }, Sender::Host);
        assert_eq!(room.picker, Some(2));
        assert_eq!(reported_pickers(&response), vec![Some(2)]);
        assert_eq!(reported_host_pickers(&response), vec![Some(2)]);

        // The host can change their mind
        room.handle_command(&GameCommand::HostSetPicker { pid: 1 }, Sender::Host);
        assert_eq!(room.picker, Some(1));

        room.handle_command(&choice, Sender::Host);
        assert_eq!(room.state, GameState::QuestionReading);
    }

    /// Chooses clue (0, 0) and opens buzzing
    fn open_clue(room: &mut Room) {
        room.handle_command(
            &GameCommand::HostChoice {
                category_index: 0,
                question_index: 0,
            },
            Sender::Host,
        );
        room.handle_command(&GameCommand::HostReady, Sender::Host);
    }

    /// `pid` buzzes in on the open clue and is judged
    fn buzz_and_judge(room: &mut Room, pid: PlayerId, correct: bool) {
        let window = buzz(room, pid, 200)
            .buzz_window_opened
            .expect("Buzz should open a window");
        room.resolve_buzz_window(window);
        room.handle_command(&GameCommand::HostChecked { correct }, Sender::Host);
    }

    #[test]
    fn test_picker_after_a_clue() {
        struct TestCase {
            name: &'static str,
            play: fn(&mut Room),
            expected_picker: PlayerId,
        }

        let test_cases = vec![
            TestCase {
                name: "A correct answer makes the answerer the picker",
                play: |room| {
                    open_clue(room);
                    buzz_and_judge(room, 2, true);
                },
                expected_picker: 2,
            },
            TestCase {
                name: "The picker answering correctly keeps picking",
                play: |room| {
                    open_clue(room);
                    buzz_and_judge(room, 1, true);
                },
                expected_picker: 1,
            },
            TestCase {
                name: "A correct answer on the rebound makes that player the picker",
                play: |room| {
                    open_clue(room);
                    buzz_and_judge(room, 2, false);
                    buzz_and_judge(room, 3, true);
                },
                expected_picker: 3,
            },
            TestCase {
                name: "A wrong answer, then a skip, keeps the picker",
                play: |room| {
                    open_clue(room);
                    buzz_and_judge(room, 2, false);
                    room.handle_command(&GameCommand::HostSkip, Sender::Host);
                },
                expected_picker: 1,
            },
            TestCase {
                name: "Everyone wrong keeps the picker",
                play: |room| {
                    open_clue(room);
                    buzz_and_judge(room, 2, false);
                    buzz_and_judge(room, 3, false);
                    buzz_and_judge(room, 1, false);
                },
                expected_picker: 1,
            },
            TestCase {
                name: "A skip keeps the picker",
                play: |room| {
                    room.handle_command(
                        &GameCommand::HostChoice {
                            category_index: 0,
                            question_index: 0,
                        },
                        Sender::Host,
                    );
                    room.handle_command(&GameCommand::HostSkip, Sender::Host);
                },
                expected_picker: 1,
            },
        ];

        for tc in test_cases {
            let mut room = create_test_room();
            add_test_player(&mut room, 1, "AJ");
            add_test_player(&mut room, 2, "Sam");
            add_test_player(&mut room, 3, "Lee");
            room.state = GameState::Selection;
            room.picker = Some(1);

            (tc.play)(&mut room);
            assert_eq!(room.state, GameState::AnswerReveal, "{}: state", tc.name);

            let response = room.handle_command(&GameCommand::HostContinue, Sender::Host);
            assert_eq!(room.state, GameState::Selection, "{}: state", tc.name);
            assert_eq!(room.picker, Some(tc.expected_picker), "{}", tc.name);
            assert_eq!(
                reported_pickers(&response),
                vec![Some(tc.expected_picker)],
                "{}: players not told the picker",
                tc.name
            );
        }
    }

    #[test]
    fn test_lowest_scorer_picks_first_in_a_new_round() {
        struct TestCase {
            name: &'static str,
            scores: [i32; 3],
            expected_picker: PlayerId,
        }

        let test_cases = vec![
            TestCase {
                name: "Last place picks",
                scores: [600, -200, 400],
                expected_picker: 2,
            },
            TestCase {
                name: "A tie for last place goes to the lowest pid",
                scores: [600, 0, 0],
                expected_picker: 2,
            },
            TestCase {
                name: "The picker can be last place",
                scores: [-400, 0, 200],
                expected_picker: 1,
            },
        ];

        for tc in test_cases {
            for end_round in [
                GameCommand::HostNextRound { round: 0 },
                GameCommand::HostContinue,
            ] {
                let mut room = create_two_round_room();
                // Players in an order other than by pid, so ties can't depend on it
                for (pid, name) in [(3, "Lee"), (1, "AJ"), (2, "Sam")] {
                    add_test_player(&mut room, pid, name);
                }
                for entry in &mut room.players {
                    entry.player.score = tc.scores[entry.player.pid as usize - 1];
                }
                room.picker = Some(3);
                if matches!(end_round, GameCommand::HostContinue) {
                    answer_round(&mut room, 0);
                    room.state = GameState::AnswerReveal;
                    room.current_question = Some((0, 1));
                } else {
                    room.state = GameState::Selection;
                }

                let response = room.handle_command(&end_round, Sender::Host);

                let name = format!("{} ({})", tc.name, end_round.name());
                assert_eq!(room.round_index, 1, "{name}: round");
                assert_eq!(room.picker, Some(tc.expected_picker), "{name}");
                assert_eq!(
                    reported_pickers(&response),
                    vec![Some(tc.expected_picker)],
                    "{name}: players not told the picker"
                );
                assert_eq!(
                    reported_host_pickers(&response),
                    vec![Some(tc.expected_picker)],
                    "{name}: host not told the picker"
                );
            }
        }
    }

    #[test]
    fn test_host_overrides_the_new_round_picker() {
        let mut room = create_two_round_room();
        add_test_player(&mut room, 1, "AJ");
        add_test_player(&mut room, 2, "Sam");
        room.players[0].player.score = 1000;
        room.state = GameState::Selection;
        room.picker = Some(1);

        room.handle_command(&GameCommand::HostNextRound { round: 0 }, Sender::Host);
        assert_eq!(room.picker, Some(2));

        room.handle_command(&GameCommand::HostSetPicker { pid: 1 }, Sender::Host);
        assert_eq!(room.picker, Some(1));
    }

    #[test]
    fn test_start_game_skipping_a_round_leaves_the_picker_to_the_host() {
        let mut room = create_two_round_room();
        add_test_player(&mut room, 1, "AJ");
        add_test_player(&mut room, 2, "Sam");
        room.players[1].player.score = -100;
        answer_round(&mut room, 0);

        room.handle_command(&GameCommand::StartGame, Sender::Host);

        assert_eq!(room.round_index, 1);
        assert_eq!(room.picker, None);
    }

    /// A two-round room whose 400 clue in the first round is a Daily Double. AJ (pid 1) is
    /// the picker, and Sam (pid 2) is also playing. The highest clue on the board is 400.
    fn create_daily_double_room() -> Room {
        let mut room = create_two_round_room();
        room.rounds[0][0].questions[1].daily_double = true;
        add_test_player(&mut room, 1, "AJ");
        add_test_player(&mut room, 2, "Sam");
        room.state = GameState::Selection;
        room.picker = Some(1);
        room
    }

    fn choose_daily_double(room: &mut Room) -> RoomResponse {
        room.handle_command(
            &GameCommand::HostChoice {
                category_index: 0,
                question_index: 1,
            },
            Sender::Host,
        )
    }

    fn wager(room: &mut Room, pid: PlayerId, amount: u32) -> RoomResponse {
        room.handle_command(&GameCommand::SubmitWager { amount }, Sender::Player(pid))
    }

    /// The `(wager range, wager, prompt)` in the PlayerState sent to `pid`
    fn player_wager_view(
        response: &RoomResponse,
        pid: PlayerId,
    ) -> Option<(Option<WagerRange>, Option<u32>, Option<String>)> {
        response
            .messages_to_specific
            .iter()
            .find_map(|(to, msg)| match msg {
                GameEvent::PlayerState {
                    wager_range,
                    wager,
                    prompt,
                    ..
                } if *to == pid => Some((*wager_range, *wager, prompt.clone())),
                _ => None,
            })
    }

    fn rejection(response: &RoomResponse) -> Option<(PlayerId, String)> {
        response
            .messages_to_specific
            .iter()
            .find_map(|(to, msg)| match msg {
                GameEvent::Rejected { reason } => Some((*to, reason.clone())),
                _ => None,
            })
    }

    #[test]
    fn test_choosing_a_daily_double() {
        let mut room = create_daily_double_room();

        let response = choose_daily_double(&mut room);

        assert_eq!(room.state, GameState::DailyDoubleWager);
        assert_eq!(room.current_question, Some((0, 1)));
        assert_eq!(room.current_buzzer, None);
        assert_eq!(room.dd_wager, None);
        assert_eq!(
            player_wager_view(&response, 1),
            Some((
                Some(WagerRange { min: 5, max: 400 }),
                None,
                Some("Test Category".to_string())
            )),
            "The picker is asked to wager"
        );
        assert_eq!(
            player_wager_view(&response, 2),
            Some((None, None, None)),
            "Other players don't wager"
        );
        assert!(
            response.messages_to_players.iter().any(|msg| matches!(
                msg,
                GameEvent::PublicState {
                    state: GameState::DailyDoubleWager,
                    picker: Some(1),
                    ..
                }
            )),
            "Everyone is told it's a Daily Double"
        );
    }

    #[test]
    fn test_daily_double_wager_bounds() {
        struct TestCase {
            name: &'static str,
            score: i32,
            amount: u32,
            accepted: bool,
        }

        let test_cases = vec![
            TestCase {
                name: "Nothing",
                score: 1000,
                amount: 0,
                accepted: false,
            },
            TestCase {
                name: "Below the minimum",
                score: 1000,
                amount: 4,
                accepted: false,
            },
            TestCase {
                name: "The minimum",
                score: 0,
                amount: 5,
                accepted: true,
            },
            TestCase {
                name: "Up to the top clue with a low score",
                score: 100,
                amount: 400,
                accepted: true,
            },
            TestCase {
                name: "Above the top clue with a low score",
                score: 100,
                amount: 401,
                accepted: false,
            },
            TestCase {
                name: "Up to the top clue with a negative score",
                score: -600,
                amount: 400,
                accepted: true,
            },
            TestCase {
                name: "All in with a high score",
                score: 1000,
                amount: 1000,
                accepted: true,
            },
            TestCase {
                name: "Above the score with a high score",
                score: 1000,
                amount: 1001,
                accepted: false,
            },
        ];

        for tc in test_cases {
            let mut room = create_daily_double_room();
            room.players[0].player.score = tc.score;
            choose_daily_double(&mut room);

            let response = wager(&mut room, 1, tc.amount);

            if tc.accepted {
                assert_eq!(room.state, GameState::Answer, "{}: state", tc.name);
                assert_eq!(room.dd_wager, Some(tc.amount), "{}: wager", tc.name);
                assert_eq!(room.current_buzzer, Some(1), "{}: answerer", tc.name);
                assert!(room.buzz_window.is_none(), "{}: buzz window", tc.name);
                assert_eq!(rejection(&response), None, "{}: rejected", tc.name);
                assert_eq!(
                    player_wager_view(&response, 1),
                    Some((None, Some(tc.amount), Some("Test Category".to_string()))),
                    "{}: picker's view",
                    tc.name
                );
            } else {
                assert_eq!(
                    room.state,
                    GameState::DailyDoubleWager,
                    "{}: state",
                    tc.name
                );
                assert_eq!(room.dd_wager, None, "{}: wager", tc.name);
                let (to, reason) = rejection(&response)
                    .unwrap_or_else(|| panic!("{}: wager should be rejected", tc.name));
                assert_eq!(to, 1, "{}: rejection sent to the wrong player", tc.name);
                assert!(reason.contains("$5"), "{}: reason {reason}", tc.name);
                assert!(response.messages_to_host.is_empty(), "{}", tc.name);
                assert!(response.messages_to_players.is_empty(), "{}", tc.name);
                assert_eq!(response.messages_to_specific.len(), 1, "{}", tc.name);
            }
        }
    }

    #[test]
    fn test_daily_double_wager_from_another_player_is_rejected() {
        let mut room = create_daily_double_room();
        choose_daily_double(&mut room);

        let response = wager(&mut room, 2, 100);
        assert_eq!(room.state, GameState::DailyDoubleWager);
        assert_eq!(room.dd_wager, None);
        let (to, _) = rejection(&response).expect("Sam's wager should be rejected");
        assert_eq!(to, 2, "Only Sam is told");
        assert_eq!(response.messages_to_specific.len(), 1);

        // The picker can try again after a rejected wager
        wager(&mut room, 1, 1000);
        assert_eq!(room.state, GameState::DailyDoubleWager);
        wager(&mut room, 1, 300);
        assert_eq!(room.state, GameState::Answer);
        assert_eq!(room.dd_wager, Some(300));
        assert_eq!(room.current_buzzer, Some(1));
    }

    #[test]
    fn test_daily_double_scoring() {
        struct TestCase {
            name: &'static str,
            correct: bool,
            expected_score: i32,
        }

        let test_cases = vec![
            TestCase {
                name: "A correct answer wins the wager, not the clue's value",
                correct: true,
                expected_score: 500 + 300,
            },
            TestCase {
                name: "A wrong answer loses the wager, with no rebound",
                correct: false,
                expected_score: 500 - 300,
            },
        ];

        for tc in test_cases {
            let mut room = create_daily_double_room();
            room.players[0].player.score = 500;
            choose_daily_double(&mut room);
            wager(&mut room, 1, 300);

            room.handle_command(
                &GameCommand::HostChecked {
                    correct: tc.correct,
                },
                Sender::Host,
            );

            assert_eq!(room.state, GameState::AnswerReveal, "{}: state", tc.name);
            assert_eq!(
                room.players[0].player.score, tc.expected_score,
                "{}: score",
                tc.name
            );
            assert_eq!(room.players[1].player.score, 0, "{}: Sam's score", tc.name);
            assert!(
                room.rounds[0][0].questions[1].answered,
                "{}: answered",
                tc.name
            );
            assert_eq!(room.picker, Some(1), "{}: picker", tc.name);
            assert_eq!(
                room.dd_wager,
                Some(300),
                "{}: the wager is shown until the host moves on",
                tc.name
            );

            let response = room.handle_command(&GameCommand::HostContinue, Sender::Host);
            assert_eq!(room.state, GameState::Selection, "{}: state", tc.name);
            assert_eq!(room.dd_wager, None, "{}: wager not cleared", tc.name);
            assert_eq!(
                player_wager_view(&response, 1),
                Some((None, None, None)),
                "{}: picker's view after the clue",
                tc.name
            );

            // The next clue is scored by its value again
            room.handle_command(&GameCommand::HostSetPicker { pid: 2 }, Sender::Host);
            open_clue(&mut room);
            buzz_and_judge(&mut room, 2, true);
            assert_eq!(room.players[1].player.score, 200, "{}: next clue", tc.name);
        }
    }

    #[test]
    fn test_skipping_a_daily_double() {
        for wagered in [false, true] {
            let mut room = create_daily_double_room();
            room.players[0].player.score = 500;
            choose_daily_double(&mut room);
            if wagered {
                wager(&mut room, 1, 300);
                assert_eq!(room.state, GameState::Answer);
            }

            room.handle_command(&GameCommand::HostSkip, Sender::Host);

            assert_eq!(room.state, GameState::AnswerReveal, "wagered: {wagered}");
            assert_eq!(room.players[0].player.score, 500, "wagered: {wagered}");
            assert!(room.rounds[0][0].questions[1].answered);
            assert_eq!(room.picker, Some(1));

            room.handle_command(&GameCommand::HostContinue, Sender::Host);
            assert_eq!(room.state, GameState::Selection);
            assert_eq!(room.dd_wager, None);
        }
    }

    #[test]
    fn test_current_stake() {
        let mut room = create_daily_double_room();
        assert_eq!(room.current_stake(), None);

        room.current_question = Some((0, 0));
        assert_eq!(room.current_stake(), Some(200));

        room.current_question = Some((0, 1));
        room.dd_wager = Some(1234);
        assert_eq!(room.current_stake(), Some(1234));
    }

    #[test]
    fn test_daily_doubles_are_never_sent() {
        let mut room = create_daily_double_room();
        for category in &mut room.rounds[1] {
            for question in &mut category.questions {
                question.daily_double = true;
            }
        }
        room.players[0].player.score = 1000;
        room.state = GameState::Start;
        room.picker = None;

        // Each response, with the player who may be told about the Daily Double
        let mut responses = vec![
            (
                room.handle_command(&GameCommand::StartGame, Sender::Host),
                1,
            ),
            (
                room.handle_command(&GameCommand::HostSetPicker { pid: 1 }, Sender::Host),
                1,
            ),
            (choose_daily_double(&mut room), 1),
            (wager(&mut room, 2, 777), 1),
            (wager(&mut room, 1, 777), 1),
            (
                room.handle_command(&GameCommand::HostChecked { correct: true }, Sender::Host),
                1,
            ),
            (
                room.handle_command(&GameCommand::HostContinue, Sender::Host),
                1,
            ),
            (
                room.handle_command(&GameCommand::HostNextRound { round: 0 }, Sender::Host),
                2,
            ),
        ];
        // Sam is in last place, so Sam picks first in Double Jeopardy, and finds a Daily Double
        assert_eq!(room.picker, Some(2));
        responses.push((
            room.handle_command(
                &GameCommand::HostChoice {
                    category_index: 0,
                    question_index: 0,
                },
                Sender::Host,
            ),
            2,
        ));
        assert_eq!(room.state, GameState::DailyDoubleWager);
        assert_eq!(room.round_index, 1);

        let mut checked = 0;
        for (response, wagerer) in &responses {
            let to_everyone = response
                .messages_to_host
                .iter()
                .chain(response.messages_to_players.iter())
                .chain(response.messages_to_specific.iter().map(|(_, msg)| msg));
            for msg in to_everyone {
                let json = serde_json::to_string(msg).expect("Failed to serialize event");
                // The key, not the `dailyDoubleWager` state
                assert!(!json.contains(r#""dailyDouble""#), "Flag sent: {json}");
                assert!(!json.contains("daily_double"), "Flag sent: {json}");
                checked += 1;
            }

            // Only the player making the wager is told about it
            for msg in &response.messages_to_players {
                let json = serde_json::to_string(msg).expect("Failed to serialize event");
                assert!(!json.contains("777"), "Wager sent to everyone: {json}");
            }
            for (to, msg) in &response.messages_to_specific {
                if to == wagerer {
                    continue;
                }
                match msg {
                    GameEvent::PlayerState {
                        wager_range,
                        wager,
                        prompt,
                        ..
                    } => assert_eq!(
                        (wager_range, wager, prompt),
                        (&None, &None, &None),
                        "Daily Double sent to {to}"
                    ),
                    GameEvent::Rejected { .. } => {}
                    other => panic!("Unexpected event for {to}: {other:?}"),
                }
            }
        }
        assert!(checked > 0);
    }
}
