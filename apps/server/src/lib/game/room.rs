use std::{
    collections::BTreeMap,
    fmt,
    time::{Duration, SystemTime},
};

use crate::{
    HostEntry, Player, PlayerEntry, PlayerId,
    api::messages::{FinalEntryView, FinalView, GameCommand, GameEvent, Sender, WagerRange},
    game::{Category, FinalQuestion, GameState, Question, RoomResponse},
    net::connection::{HostToken, RoomCode, send_event},
};

/// How long the server keeps accepting buzzes after the first one arrives. Buzzes are
/// ranked by client-measured reaction time, so this needs to cover the spread of
/// players' network latencies.
pub const BUZZ_WINDOW: Duration = Duration::from_millis(250);

pub type BuzzWindowId = u64;

/// The smallest Daily Double wager, as on TV
pub const DAILY_DOUBLE_MIN_WAGER: u32 = 5;

/// A Daily Double being played
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DailyDouble {
    /// Who chose the Daily Double, and so wagers on it and answers it
    pub player: PlayerId,
    /// Their wager, once it has been accepted
    pub wager: Option<u32>,
}

/// The longest Final Jeopardy response kept, in characters. Anything longer is cut off.
pub const FINAL_ANSWER_MAX_CHARS: usize = 200;

/// The wager of a player with a score of $0 or less, who has nothing to wager
pub const FINAL_FIXED_WAGER: u32 = 1;

/// One player's part in Final Jeopardy
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FinalEntry {
    /// Filled in when the clue is revealed for anyone who didn't wager
    pub wager: Option<u32>,
    /// Filled in with "" when the responses are locked for anyone who didn't respond
    pub answer: Option<String>,
    pub correct: Option<bool>,
    /// Whether the host has shown everyone this response
    pub revealed: bool,
}

impl FinalEntry {
    /// The entry of a player with `score` as Final Jeopardy starts. A player with nothing
    /// to wager must wager exactly $1, so their wager is made for them.
    fn new(score: i32) -> Self {
        Self {
            wager: (score <= 0).then_some(FINAL_FIXED_WAGER),
            ..Self::default()
        }
    }
}

/// Final Jeopardy being played, from when it starts until the game ends
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FinalRound {
    /// Everyone playing, by pid. Players who join during the wager or the clue play too;
    /// players who join after the responses are locked only watch.
    pub entries: BTreeMap<PlayerId, FinalEntry>,
    /// Who is revealed in which order, set when the responses are locked: lowest score
    /// first, as on TV. Ties go to the lowest pid, the player who joined first.
    pub reveal_order: Vec<PlayerId>,
    /// Index into `reveal_order` of the next response to reveal and judge
    pub reveal_index: usize,
}

impl FinalRound {
    /// The pid of the next response to reveal or judge, if any are left
    fn current(&self) -> Option<PlayerId> {
        self.reveal_order.get(self.reveal_index).copied()
    }
}

/// What a player is told about their wager and response on the clue being played, see
/// [`GameEvent::PlayerState`]
#[derive(Debug, Default, PartialEq)]
struct WagerView {
    range: Option<WagerRange>,
    wager: Option<u32>,
    prompt: Option<String>,
    can_answer: bool,
    answer: Option<String>,
    correct: Option<bool>,
}

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
    /// first, so this is `None` until then, and clues can't be chosen without it. A player
    /// who disconnects stays in the room, so they stay the picker until the host chooses
    /// someone else.
    pub picker: Option<PlayerId>,
    /// The Daily Double being played, from when it's chosen until the host moves on from
    /// the clue
    pub daily_double: Option<DailyDouble>,
    /// The Final Jeopardy clue, if the game ends with one
    pub final_question: Option<FinalQuestion>,
    /// Final Jeopardy, once it has started
    pub final_round: Option<FinalRound>,
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
            .field("daily double", &self.daily_double)
            .field("has final", &self.final_question.is_some())
            .field("final round", &self.final_round)
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
            daily_double: None,
            final_question: None,
            final_round: None,
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
            dd_wager: self.dd_wager(),
            has_final: self.final_question.is_some(),
            final_jeopardy: self.build_final_view(),
            winner: self.winner,
        }
    }

    /// Final Jeopardy as the host sees it, once it has started: the clue and the correct
    /// response, so the host can read and judge them, and every wager and response, so
    /// the host can see who is done. The host's display must only show what's revealed.
    fn build_final_view(&self) -> Option<FinalView> {
        let question = self.final_question.as_ref()?;
        let final_round = self.final_round.as_ref()?;
        let entry_view = |pid: PlayerId| {
            let entry = final_round.entries.get(&pid)?;
            Some(FinalEntryView {
                pid,
                wager: entry.wager,
                answer: entry.answer.clone(),
                correct: entry.correct,
                revealed: entry.revealed,
            })
        };
        let entries = if final_round.reveal_order.is_empty() {
            final_round
                .entries
                .keys()
                .filter_map(|&pid| entry_view(pid))
                .collect()
        } else {
            final_round
                .reveal_order
                .iter()
                .filter_map(|&pid| entry_view(pid))
                .collect()
        };
        Some(FinalView {
            category: question.category.clone(),
            question: question.question.clone(),
            answer: question.answer.clone(),
            entries,
            reveal_index: final_round.reveal_index,
        })
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
        let WagerView {
            range,
            wager,
            prompt,
            can_answer,
            answer,
            correct,
        } = self.wager_view(player_id);

        Some(GameEvent::PlayerState {
            pid: player.player.pid,
            buzzed: player.player.buzzed,
            score: player.player.score,
            can_buzz,
            wager_range: range,
            wager,
            prompt,
            can_answer,
            answer,
            correct,
        })
    }

    /// What `player_id` is told about their wager in the current state. Each phase with a
    /// wager has its own arm.
    fn wager_view(&self, player_id: PlayerId) -> WagerView {
        match self.state {
            GameState::DailyDoubleWager | GameState::Answer | GameState::AnswerReveal => {
                self.daily_double_view(player_id)
            }
            GameState::FinalWager | GameState::FinalAnswer | GameState::FinalReveal => {
                self.final_view(player_id)
            }
            GameState::Start
            | GameState::Selection
            | GameState::QuestionReading
            | GameState::WaitingForBuzz
            | GameState::GameEnd => WagerView::default(),
        }
    }

    /// Final Jeopardy as `player_id` sees it: only their own wager and response. The
    /// prompt is the category while wagering and the clue after that. Players are never
    /// sent the correct response. A player who joined after the responses were locked has
    /// no entry and only watches.
    fn final_view(&self, player_id: PlayerId) -> WagerView {
        let (Some(question), Some(entry)) = (
            self.final_question.as_ref(),
            self.final_round
                .as_ref()
                .and_then(|f| f.entries.get(&player_id)),
        ) else {
            return WagerView::default();
        };
        let prompt = match self.state {
            GameState::FinalWager => question.category.clone(),
            _ => question.question.clone(),
        };
        WagerView {
            range: self.final_wager_range(player_id),
            wager: entry.wager,
            prompt: Some(prompt),
            can_answer: self.state == GameState::FinalAnswer,
            answer: entry.answer.clone(),
            correct: entry.correct,
        }
    }

    /// The score of `player_id`, if they're in the room
    fn score_of(&self, player_id: PlayerId) -> Option<i32> {
        self.players
            .iter()
            .find(|p| p.player.pid == player_id)
            .map(|p| p.player.score)
    }

    /// The wagers `player_id` may make on Final Jeopardy while wagers are open: anything
    /// from $0 to their score. A player with a score of $0 or less has no choice, since
    /// their wager is $1, so they get no range.
    fn final_wager_range(&self, player_id: PlayerId) -> Option<WagerRange> {
        if self.state != GameState::FinalWager
            || !self
                .final_round
                .as_ref()
                .is_some_and(|f| f.entries.contains_key(&player_id))
        {
            return None;
        }
        let max = u32::try_from(self.score_of(player_id)?)
            .ok()
            .filter(|&score| score > 0)?;
        Some(WagerRange { min: 0, max })
    }

    /// The Daily Double as `player_id` sees it. Only the player who chose it wagers and
    /// answers, so nobody else is told anything.
    fn daily_double_view(&self, player_id: PlayerId) -> WagerView {
        let Some(daily_double) = self.daily_double.filter(|dd| dd.player == player_id) else {
            return WagerView::default();
        };
        WagerView {
            range: self.dd_wager_range(player_id),
            wager: daily_double.wager,
            prompt: self.current_category().map(|cat| cat.title.clone()),
            ..WagerView::default()
        }
    }

    /// The wager on the Daily Double being played, once it has been made
    pub fn dd_wager(&self) -> Option<u32> {
        self.daily_double.and_then(|dd| dd.wager)
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
        self.dd_wager()
            .or_else(|| self.current_clue().map(|q| q.value))
    }

    /// The wagers `player_id` may make on the Daily Double, if they are the one wagering
    /// and haven't yet: from $5 up to their score, or up to the most valuable clue on the
    /// board if that's more
    fn dd_wager_range(&self, player_id: PlayerId) -> Option<WagerRange> {
        let daily_double = self.daily_double?;
        if self.state != GameState::DailyDoubleWager
            || daily_double.player != player_id
            || daily_double.wager.is_some()
        {
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
                let Some(picker) = self.picker else {
                    tracing::info!(
                        room_code = %self.code,
                        "Ignoring HostChoice before the host chose who picks first"
                    );
                    return RoomResponse::new();
                };
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
                        picker,
                        "Picker found a Daily Double"
                    );
                    self.daily_double = Some(DailyDouble {
                        player: picker,
                        wager: None,
                    });
                    self.state = GameState::DailyDoubleWager;
                } else {
                    self.state = GameState::QuestionReading;
                }
                self.broadcast_state()
            }

            GameCommand::SubmitWager { amount } => self.handle_submit_wager(sender, *amount),

            GameCommand::SubmitAnswer { answer } => self.handle_submit_answer(sender, answer),

            GameCommand::HostRevealFinalClue => self.handle_reveal_final_clue(),

            GameCommand::HostLockFinalAnswers => self.handle_lock_final_answers(),

            GameCommand::HostRevealFinalResponse { pid } => self.handle_reveal_final_response(*pid),

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

            GameCommand::HostChecked { correct } => match self.state {
                GameState::FinalReveal => self.handle_final_judged(*correct),
                _ => self.handle_host_checked(*correct),
            },

            GameCommand::HostSkip => self.handle_host_skip(),

            GameCommand::HostContinue => match self.state {
                GameState::FinalReveal => self.handle_finish_final(),
                _ => self.handle_host_continue(),
            },

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
            // In Final Jeopardy, this judges the response that was just revealed
            GameCommand::HostChecked { .. } => {
                matches!(self.state, GameState::Answer | GameState::FinalReveal)
            }
            GameCommand::HostSkip => match self.state {
                GameState::QuestionReading
                | GameState::WaitingForBuzz
                | GameState::DailyDoubleWager => true,
                // Nobody else can answer a Daily Double, so the host may skip it while the
                // picker answers. A normal clue goes back to buzzing after a wrong answer.
                GameState::Answer => self.daily_double.is_some(),
                _ => false,
            },
            GameCommand::SubmitWager { .. } => {
                matches!(
                    self.state,
                    GameState::DailyDoubleWager | GameState::FinalWager
                )
            }
            GameCommand::SubmitAnswer { .. } => self.state == GameState::FinalAnswer,
            GameCommand::HostRevealFinalClue => self.state == GameState::FinalWager,
            GameCommand::HostLockFinalAnswers => self.state == GameState::FinalAnswer,
            GameCommand::HostRevealFinalResponse { .. } => self.state == GameState::FinalReveal,
            // From Final Jeopardy, this ends the game once every response is judged
            GameCommand::HostContinue => {
                matches!(self.state, GameState::AnswerReveal | GameState::FinalReveal)
            }
            // The picker can only be changed between clues. If the picker disconnects while
            // wagering on a Daily Double, the host's only way on is to skip it.
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

        // Nobody is answering a skipped clue, including the picker on a Daily Double
        self.current_buzzer = None;
        self.state = GameState::AnswerReveal;

        self.broadcast_state()
    }

    /// Handles a player's wager in whichever phase is collecting one
    fn handle_submit_wager(&mut self, sender: Sender, amount: u32) -> RoomResponse {
        let Sender::Player(player_id) = sender else {
            return RoomResponse::new();
        };
        match self.state {
            GameState::DailyDoubleWager => self.handle_daily_double_wager(player_id, amount),
            GameState::FinalWager => self.handle_final_wager(player_id, amount),
            _ => RoomResponse::new(),
        }
    }

    /// Accepts a player's Final Jeopardy wager. It can be changed until the clue is
    /// revealed.
    fn handle_final_wager(&mut self, player_id: PlayerId, amount: u32) -> RoomResponse {
        let Some(score) = self.score_of(player_id) else {
            return RoomResponse::new();
        };
        let range = match self.final_wager_range(player_id) {
            Some(range) => range,
            None if score <= 0 => {
                if amount == FINAL_FIXED_WAGER {
                    return self.update_player(player_id);
                }
                tracing::info!(
                    room_code = %self.code,
                    player_id,
                    amount,
                    score,
                    "Rejecting a Final Jeopardy wager from a player with nothing to wager"
                );
                return reject(
                    player_id,
                    "With a score of $0 or less, your wager is $1".to_string(),
                );
            }
            None => {
                tracing::info!(
                    room_code = %self.code,
                    player_id,
                    "Rejecting a Final Jeopardy wager from a player who isn't playing it"
                );
                return reject(
                    player_id,
                    "You joined too late to play Final Jeopardy".to_string(),
                );
            }
        };
        if !range.contains(amount) {
            tracing::info!(
                room_code = %self.code,
                player_id,
                amount,
                ?range,
                "Rejecting a Final Jeopardy wager out of range"
            );
            return reject(player_id, out_of_range_reason(range));
        }

        tracing::info!(room_code = %self.code, player_id, "Final Jeopardy wager");
        if let Some(entry) = self.final_entry_mut(player_id) {
            entry.wager = Some(amount);
        }
        self.update_player(player_id)
    }

    /// Accepts a player's Final Jeopardy response, trimmed and cut off at
    /// [`FINAL_ANSWER_MAX_CHARS`]. It can be changed until the host locks the responses.
    fn handle_submit_answer(&mut self, sender: Sender, answer: &str) -> RoomResponse {
        let Sender::Player(player_id) = sender else {
            return RoomResponse::new();
        };
        let answer: String = answer.trim().chars().take(FINAL_ANSWER_MAX_CHARS).collect();
        // Cutting it off can leave whitespace at the end
        let answer = answer.trim_end().to_string();
        let Some(entry) = self.final_entry_mut(player_id) else {
            tracing::info!(
                room_code = %self.code,
                player_id,
                "Rejecting a Final Jeopardy response from a player who isn't playing it"
            );
            return reject(
                player_id,
                "You joined too late to play Final Jeopardy".to_string(),
            );
        };
        entry.answer = Some(answer);
        tracing::info!(room_code = %self.code, player_id, "Final Jeopardy response");
        self.update_player(player_id)
    }

    fn final_entry_mut(&mut self, player_id: PlayerId) -> Option<&mut FinalEntry> {
        self.final_round.as_mut()?.entries.get_mut(&player_id)
    }

    /// Tells the host and `player_id` about a change only `player_id` can see, such as
    /// their Final Jeopardy wager. Nobody else's view changes, so nobody else is sent
    /// anything, which also leaves other players' half-typed responses alone.
    fn update_player(&self, player_id: PlayerId) -> RoomResponse {
        let response = RoomResponse::to_host(self.build_game_state_msg());
        match self.build_player_state_msg(player_id) {
            Some(msg) => response.merge(RoomResponse::to_player(player_id, msg)),
            None => response,
        }
    }

    /// Starts Final Jeopardy: everyone in the room plays, and wagers on the category
    fn enter_final(&mut self) {
        let entries = self
            .players
            .iter()
            .map(|p| (p.player.pid, FinalEntry::new(p.player.score)))
            .collect();
        self.final_round = Some(FinalRound {
            entries,
            ..FinalRound::default()
        });
        tracing::info!(
            room_code = %self.code,
            players = self.players.len(),
            "Starting Final Jeopardy"
        );
        self.state = GameState::FinalWager;
    }

    /// Adds a player who joined the room. Anyone joining before the Final Jeopardy
    /// responses are locked plays it, with nothing to wager, so for $1.
    pub fn add_player(&mut self, entry: PlayerEntry) {
        let pid = entry.player.pid;
        let score = entry.player.score;
        self.players.push(entry);
        if matches!(self.state, GameState::FinalWager | GameState::FinalAnswer)
            && let Some(final_round) = &mut self.final_round
        {
            final_round
                .entries
                .entry(pid)
                .or_insert_with(|| FinalEntry::new(score));
        }
    }

    /// Closes the wagers and shows everyone the clue. Anyone who didn't wager wagers
    /// nothing, or $1 if that's all they could wager.
    fn handle_reveal_final_clue(&mut self) -> RoomResponse {
        let scores: Vec<(PlayerId, i32)> = self
            .players
            .iter()
            .map(|p| (p.player.pid, p.player.score))
            .collect();
        if let Some(final_round) = &mut self.final_round {
            for (pid, score) in scores {
                if let Some(entry) = final_round.entries.get_mut(&pid) {
                    entry
                        .wager
                        .get_or_insert(if score > 0 { 0 } else { FINAL_FIXED_WAGER });
                }
            }
        }
        tracing::info!(room_code = %self.code, "Final Jeopardy clue revealed");
        self.state = GameState::FinalAnswer;
        self.broadcast_state()
    }

    /// Closes the responses, with an empty one for anyone who didn't respond, and decides
    /// the order they're revealed in: lowest score first, ties to the lowest pid
    fn handle_lock_final_answers(&mut self) -> RoomResponse {
        let mut order: Vec<(i32, PlayerId)> = self
            .players
            .iter()
            .map(|p| (p.player.score, p.player.pid))
            .collect();
        order.sort_unstable();
        if let Some(final_round) = &mut self.final_round {
            for entry in final_round.entries.values_mut() {
                entry.answer.get_or_insert_with(String::new);
            }
            final_round.reveal_order = order
                .into_iter()
                .map(|(_, pid)| pid)
                .filter(|pid| final_round.entries.contains_key(pid))
                .collect();
            final_round.reveal_index = 0;
        }
        tracing::info!(room_code = %self.code, "Final Jeopardy responses locked");
        self.state = GameState::FinalReveal;
        self.broadcast_state()
    }

    /// Shows everyone `pid`'s response, if they're next to be revealed and haven't been
    fn handle_reveal_final_response(&mut self, pid: PlayerId) -> RoomResponse {
        let Some(final_round) = &mut self.final_round else {
            return RoomResponse::new();
        };
        let Some(entry) = final_round
            .current()
            .filter(|&current| current == pid)
            .and_then(|current| final_round.entries.get_mut(&current))
            .filter(|entry| !entry.revealed)
        else {
            tracing::info!(
                room_code = %self.code,
                pid,
                next = ?final_round.current(),
                "Ignoring a reveal of a response that isn't next"
            );
            return RoomResponse::new();
        };
        entry.revealed = true;
        tracing::info!(room_code = %self.code, pid, "Final Jeopardy response revealed");
        self.broadcast_state()
    }

    /// Judges the response that was just revealed, moves its player's score by their
    /// wager, and moves on to the next one. A response must be revealed before it's
    /// judged, so a repeated click can't judge the next player too.
    fn handle_final_judged(&mut self, correct: bool) -> RoomResponse {
        let Some(final_round) = &mut self.final_round else {
            return RoomResponse::new();
        };
        let Some((pid, entry)) = final_round
            .current()
            .and_then(|pid| Some((pid, final_round.entries.get_mut(&pid)?)))
            .filter(|(_, entry)| entry.revealed && entry.correct.is_none())
        else {
            tracing::info!(
                room_code = %self.code,
                next = ?final_round.current(),
                "Ignoring a judgement before the next response was revealed"
            );
            return RoomResponse::new();
        };
        entry.correct = Some(correct);
        let stake = i32::try_from(entry.wager.unwrap_or(0)).unwrap_or(i32::MAX);
        final_round.reveal_index += 1;

        if let Some(player) = self.players.iter_mut().find(|p| p.player.pid == pid) {
            player.player.score = if correct {
                player.player.score.saturating_add(stake)
            } else {
                player.player.score.saturating_sub(stake)
            };
        }
        tracing::info!(room_code = %self.code, pid, correct, "Final Jeopardy response judged");
        self.broadcast_state()
    }

    /// Ends the game once every Final Jeopardy response has been judged
    fn handle_finish_final(&mut self) -> RoomResponse {
        if let Some(next) = self.final_round.as_ref().and_then(FinalRound::current) {
            tracing::info!(
                room_code = %self.code,
                next,
                "Ignoring HostContinue before every Final Jeopardy response was judged"
            );
            return RoomResponse::new();
        }
        self.determine_winner();
        tracing::info!(room_code = %self.code, winner = ?self.winner, "Game ended");
        self.state = GameState::GameEnd;
        self.broadcast_state()
    }

    /// Accepts the wager on a Daily Double from the player who chose it, and has them
    /// answer it. Nobody buzzes on a Daily Double.
    fn handle_daily_double_wager(&mut self, player_id: PlayerId, amount: u32) -> RoomResponse {
        let Some(range) = self.dd_wager_range(player_id) else {
            tracing::info!(
                room_code = %self.code,
                player_id,
                daily_double = ?self.daily_double,
                "Rejecting a Daily Double wager from a player who didn't choose it"
            );
            return reject(
                player_id,
                "Only the player who chose the Daily Double can wager on it".to_string(),
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
            return reject(player_id, out_of_range_reason(range));
        }

        tracing::info!(room_code = %self.code, player_id, amount, "Daily Double wager");
        self.daily_double = Some(DailyDouble {
            player: player_id,
            wager: Some(amount),
        });
        self.current_buzzer = Some(player_id);
        self.state = GameState::Answer;
        self.broadcast_state()
    }

    /// Forgets the clue that was being played, including any Daily Double wager on it
    fn clear_clue(&mut self) {
        self.current_question = None;
        self.current_buzzer = None;
        self.buzz_window = None;
        self.daily_double = None;
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
    /// round with clues to play if there is one, otherwise to Final Jeopardy or the end of
    /// the game
    fn finish_round(&mut self) {
        self.enter_round(self.round_index + 1);
    }

    /// Starts the first round from `first` on that has a clue left to play, skipping
    /// empty rounds. If there is none, Final Jeopardy starts, or the game ends if it has
    /// none.
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
        } else if self.final_question.is_some() {
            self.enter_final();
        } else {
            self.determine_winner();
            self.state = GameState::GameEnd;
        }
    }

    /// The connected player with the lowest score, or the lowest scorer of all if nobody
    /// is connected. Ties go to the player who joined first (the lowest pid), so the choice
    /// doesn't depend on the order of `players`.
    fn lowest_scorer(&self) -> Option<PlayerId> {
        let lowest = |connected_only: bool| {
            self.players
                .iter()
                .filter(|p| !connected_only || p.is_connected())
                .map(|p| (p.player.score, p.player.pid))
                .min()
                .map(|(_, pid)| pid)
        };
        lowest(true).or_else(|| lowest(false))
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
        let daily_double = self.daily_double.is_some();

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

/// Tells one player why their command was refused
fn reject(player_id: PlayerId, reason: String) -> RoomResponse {
    RoomResponse::to_player(player_id, GameEvent::Rejected { reason })
}

/// Why a wager outside `range` was refused
fn out_of_range_reason(range: WagerRange) -> String {
    format!(
        "Your wager must be between ${} and ${}",
        range.min, range.max
    )
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

    /// Adds a player whose connection stays open while the returned receiver lives. Players
    /// added by [`add_test_player`] count as disconnected.
    fn add_connected_player(
        room: &mut Room,
        pid: u32,
        name: &str,
    ) -> tokio::sync::mpsc::Receiver<GameEvent> {
        let (tx, rx) = channel(10);
        room.players.push(PlayerEntry::new(
            Player::new(pid, name.to_string(), 0, false, PlayerToken::generate()),
            tx,
        ));
        rx
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
            TestCase {
                name: "Host cannot wager on Final Jeopardy",
                initial_state: GameState::FinalWager,
                command: GameCommand::SubmitWager { amount: 0 },
                sender: Sender::Host,
            },
            TestCase {
                name: "Host cannot respond to Final Jeopardy",
                initial_state: GameState::FinalAnswer,
                command: GameCommand::SubmitAnswer {
                    answer: "Yahara".to_string(),
                },
                sender: Sender::Host,
            },
            TestCase {
                name: "Player cannot reveal the Final Jeopardy clue",
                initial_state: GameState::FinalWager,
                command: GameCommand::HostRevealFinalClue,
                sender: Sender::Player(1),
            },
            TestCase {
                name: "Player cannot lock the Final Jeopardy responses",
                initial_state: GameState::FinalAnswer,
                command: GameCommand::HostLockFinalAnswers,
                sender: Sender::Player(1),
            },
            TestCase {
                name: "Player cannot reveal a Final Jeopardy response",
                initial_state: GameState::FinalReveal,
                command: GameCommand::HostRevealFinalResponse { pid: 1 },
                sender: Sender::Player(1),
            },
            TestCase {
                name: "Player cannot judge a Final Jeopardy response",
                initial_state: GameState::FinalReveal,
                command: GameCommand::HostChecked { correct: true },
                sender: Sender::Player(1),
            },
            TestCase {
                name: "Player cannot finish Final Jeopardy",
                initial_state: GameState::FinalReveal,
                command: GameCommand::HostContinue,
                sender: Sender::Player(1),
            },
        ];

        for tc in test_cases {
            let mut room = create_two_round_room();
            add_test_player(&mut room, 1, "AJ");
            room.state = tc.initial_state.clone();
            room.current_question = Some((0, 0));
            room.current_buzzer = Some(1);
            room.final_question = Some(final_question());
            let final_round = revealed_final_round(1);
            room.final_round = Some(final_round.clone());

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
            assert_eq!(
                room.final_round,
                Some(final_round),
                "{}: Final Jeopardy changed",
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
                setup: |room| {
                    room.daily_double = Some(DailyDouble {
                        player: 1,
                        wager: Some(200),
                    })
                },
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
            TestCase {
                name: "SubmitAnswer during Selection",
                initial_state: GameState::Selection,
                setup: |_| {},
                command: GameCommand::SubmitAnswer {
                    answer: "Yahara".to_string(),
                },
                sender: Sender::Player(1),
            },
            TestCase {
                name: "SubmitAnswer during a Daily Double",
                initial_state: GameState::Answer,
                setup: |_| {},
                command: GameCommand::SubmitAnswer {
                    answer: "Yahara".to_string(),
                },
                sender: Sender::Player(1),
            },
            TestCase {
                name: "SubmitAnswer during the Final Jeopardy wager",
                initial_state: GameState::FinalWager,
                setup: |_| {},
                command: GameCommand::SubmitAnswer {
                    answer: "Yahara".to_string(),
                },
                sender: Sender::Player(1),
            },
            TestCase {
                name: "SubmitAnswer after the responses were locked",
                initial_state: GameState::FinalReveal,
                setup: |_| {},
                command: GameCommand::SubmitAnswer {
                    answer: "Yahara".to_string(),
                },
                sender: Sender::Player(1),
            },
            TestCase {
                name: "SubmitAnswer after the game ended",
                initial_state: GameState::GameEnd,
                setup: |_| {},
                command: GameCommand::SubmitAnswer {
                    answer: "Yahara".to_string(),
                },
                sender: Sender::Player(1),
            },
            TestCase {
                name: "SubmitWager after the Final Jeopardy clue was revealed",
                initial_state: GameState::FinalAnswer,
                setup: |_| {},
                command: GameCommand::SubmitWager { amount: 0 },
                sender: Sender::Player(1),
            },
            TestCase {
                name: "SubmitWager while Final Jeopardy is revealed",
                initial_state: GameState::FinalReveal,
                setup: |_| {},
                command: GameCommand::SubmitWager { amount: 0 },
                sender: Sender::Player(1),
            },
            TestCase {
                name: "HostRevealFinalClue during Selection",
                initial_state: GameState::Selection,
                setup: |_| {},
                command: GameCommand::HostRevealFinalClue,
                sender: Sender::Host,
            },
            TestCase {
                name: "HostRevealFinalClue twice",
                initial_state: GameState::FinalAnswer,
                setup: |_| {},
                command: GameCommand::HostRevealFinalClue,
                sender: Sender::Host,
            },
            TestCase {
                name: "HostRevealFinalClue during the reveal",
                initial_state: GameState::FinalReveal,
                setup: |_| {},
                command: GameCommand::HostRevealFinalClue,
                sender: Sender::Host,
            },
            TestCase {
                name: "HostLockFinalAnswers before the clue was revealed",
                initial_state: GameState::FinalWager,
                setup: |_| {},
                command: GameCommand::HostLockFinalAnswers,
                sender: Sender::Host,
            },
            TestCase {
                name: "HostLockFinalAnswers twice",
                initial_state: GameState::FinalReveal,
                setup: |_| {},
                command: GameCommand::HostLockFinalAnswers,
                sender: Sender::Host,
            },
            TestCase {
                name: "HostLockFinalAnswers during a normal clue",
                initial_state: GameState::Answer,
                setup: |_| {},
                command: GameCommand::HostLockFinalAnswers,
                sender: Sender::Host,
            },
            TestCase {
                name: "HostRevealFinalResponse during the wager",
                initial_state: GameState::FinalWager,
                setup: |_| {},
                command: GameCommand::HostRevealFinalResponse { pid: 1 },
                sender: Sender::Host,
            },
            TestCase {
                name: "HostRevealFinalResponse before the responses were locked",
                initial_state: GameState::FinalAnswer,
                setup: |_| {},
                command: GameCommand::HostRevealFinalResponse { pid: 1 },
                sender: Sender::Host,
            },
            TestCase {
                name: "HostRevealFinalResponse after the game ended",
                initial_state: GameState::GameEnd,
                setup: |_| {},
                command: GameCommand::HostRevealFinalResponse { pid: 1 },
                sender: Sender::Host,
            },
            TestCase {
                name: "HostChecked during the Final Jeopardy wager",
                initial_state: GameState::FinalWager,
                setup: |_| {},
                command: GameCommand::HostChecked { correct: true },
                sender: Sender::Host,
            },
            TestCase {
                name: "HostChecked before the responses were locked",
                initial_state: GameState::FinalAnswer,
                setup: |_| {},
                command: GameCommand::HostChecked { correct: true },
                sender: Sender::Host,
            },
            TestCase {
                name: "HostContinue during the Final Jeopardy wager",
                initial_state: GameState::FinalWager,
                setup: |_| {},
                command: GameCommand::HostContinue,
                sender: Sender::Host,
            },
            TestCase {
                name: "HostContinue before the responses were locked",
                initial_state: GameState::FinalAnswer,
                setup: |_| {},
                command: GameCommand::HostContinue,
                sender: Sender::Host,
            },
            TestCase {
                name: "HostNextRound during Final Jeopardy",
                initial_state: GameState::FinalWager,
                setup: |_| {},
                command: GameCommand::HostNextRound { round: 0 },
                sender: Sender::Host,
            },
            TestCase {
                name: "HostSkip during Final Jeopardy",
                initial_state: GameState::FinalAnswer,
                setup: |_| {},
                command: GameCommand::HostSkip,
                sender: Sender::Host,
            },
            TestCase {
                name: "HostChoice during Final Jeopardy",
                initial_state: GameState::FinalReveal,
                setup: |_| {},
                command: GameCommand::HostChoice {
                    category_index: 0,
                    question_index: 1,
                },
                sender: Sender::Host,
            },
            TestCase {
                name: "Buzz during Final Jeopardy",
                initial_state: GameState::FinalAnswer,
                setup: |_| {},
                command: GameCommand::Buzz { reaction_ms: 100 },
                sender: Sender::Player(1),
            },
            TestCase {
                name: "HostSetPicker during Final Jeopardy",
                initial_state: GameState::FinalWager,
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
            room.final_question = Some(final_question());
            room.final_round = Some(FinalRound {
                entries: BTreeMap::from([(1, FinalEntry::new(0))]),
                ..FinalRound::default()
            });
            (tc.setup)(&mut room);
            let picker = room.picker;
            let daily_double = room.daily_double;
            let final_round = room.final_round.clone();

            let response = room.handle_command(&tc.command, tc.sender);

            assert_eq!(room.state, tc.initial_state, "{}: state changed", tc.name);
            assert!(is_empty(&response), "{}: response not empty", tc.name);
            assert_eq!(room.picker, picker, "{}: picker changed", tc.name);
            assert_eq!(
                room.daily_double, daily_double,
                "{}: Daily Double changed",
                tc.name
            );
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
            assert_eq!(
                room.final_round, final_round,
                "{}: Final Jeopardy changed",
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
            TestCase {
                name: "Clearing the Double Jeopardy board starts Final Jeopardy",
                create_room: create_final_room,
                initial_state: GameState::AnswerReveal,
                setup: |room| {
                    room.round_index = 1;
                    answer_round(room, 1);
                    room.current_question = Some((0, 1));
                },
                command: GameCommand::HostContinue,
                expected_state: GameState::FinalWager,
                expected_round: 1,
            },
            TestCase {
                name: "Clearing the Jeopardy board still starts Double Jeopardy before Final",
                create_room: create_final_room,
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
                name: "Clearing the board of a single-round game starts Final Jeopardy",
                create_room: create_single_round_final_room,
                initial_state: GameState::AnswerReveal,
                setup: |room| {
                    answer_round(room, 0);
                    room.current_question = Some((0, 1));
                },
                command: GameCommand::HostContinue,
                expected_state: GameState::FinalWager,
                expected_round: 0,
            },
            TestCase {
                name: "HostNextRound in the last round starts Final Jeopardy",
                create_room: create_final_room,
                initial_state: GameState::Selection,
                setup: |room| room.round_index = 1,
                command: GameCommand::HostNextRound { round: 1 },
                expected_state: GameState::FinalWager,
                expected_round: 1,
            },
            TestCase {
                name: "HostNextRound in a single-round game starts Final Jeopardy",
                create_room: create_single_round_final_room,
                initial_state: GameState::Selection,
                setup: |_| {},
                command: GameCommand::HostNextRound { round: 0 },
                expected_state: GameState::FinalWager,
                expected_round: 0,
            },
            TestCase {
                name: "HostNextRound skips an answered Double Jeopardy to Final Jeopardy",
                create_room: create_final_room,
                initial_state: GameState::Selection,
                setup: |room| answer_round(room, 1),
                command: GameCommand::HostNextRound { round: 0 },
                expected_state: GameState::FinalWager,
                expected_round: 0,
            },
            TestCase {
                name: "StartGame with no clues goes straight to Final Jeopardy",
                create_room: create_final_room,
                initial_state: GameState::Start,
                setup: |room| {
                    room.rounds[0].clear();
                    room.rounds[1].clear();
                },
                command: GameCommand::StartGame,
                expected_state: GameState::FinalWager,
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
            } else {
                assert_eq!(room.winner, None, "{}: winner before the end", tc.name);
            }
            assert_eq!(
                room.final_round.is_some(),
                tc.expected_state == GameState::FinalWager,
                "{}: Final Jeopardy started",
                tc.name
            );
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
        assert_eq!(
            room.daily_double,
            Some(DailyDouble {
                player: 1,
                wager: None
            })
        );
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
                assert_eq!(room.dd_wager(), Some(tc.amount), "{}: wager", tc.name);
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
                assert_eq!(room.dd_wager(), None, "{}: wager", tc.name);
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
        assert_eq!(room.dd_wager(), None);
        let (to, _) = rejection(&response).expect("Sam's wager should be rejected");
        assert_eq!(to, 2, "Only Sam is told");
        assert_eq!(response.messages_to_specific.len(), 1);

        // The picker can try again after a rejected wager
        wager(&mut room, 1, 1000);
        assert_eq!(room.state, GameState::DailyDoubleWager);
        wager(&mut room, 1, 300);
        assert_eq!(room.state, GameState::Answer);
        assert_eq!(room.dd_wager(), Some(300));
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
                room.dd_wager(),
                Some(300),
                "{}: the wager is shown until the host moves on",
                tc.name
            );

            let response = room.handle_command(&GameCommand::HostContinue, Sender::Host);
            assert_eq!(room.state, GameState::Selection, "{}: state", tc.name);
            assert_eq!(room.daily_double, None, "{}: not cleared", tc.name);
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
            assert_eq!(room.current_buzzer, None, "wagered: {wagered}");

            room.handle_command(&GameCommand::HostContinue, Sender::Host);
            assert_eq!(room.state, GameState::Selection);
            assert_eq!(room.daily_double, None);
        }
    }

    #[test]
    fn test_current_stake() {
        let mut room = create_daily_double_room();
        assert_eq!(room.current_stake(), None);

        room.current_question = Some((0, 0));
        assert_eq!(room.current_stake(), Some(200));

        room.current_question = Some((0, 1));
        room.daily_double = Some(DailyDouble {
            player: 1,
            wager: Some(1234),
        });
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

    #[test]
    fn test_new_round_picker_prefers_connected_players() {
        struct TestCase {
            name: &'static str,
            /// (pid, score, connected) for each player
            players: Vec<(PlayerId, i32, bool)>,
            expected_picker: PlayerId,
        }

        let test_cases = vec![
            TestCase {
                name: "A player who left in last place is passed over",
                players: vec![(1, -500, false), (2, 0, true), (3, 100, true)],
                expected_picker: 2,
            },
            TestCase {
                name: "A connected player in last place picks",
                players: vec![(1, 500, false), (2, -100, true), (3, 100, true)],
                expected_picker: 2,
            },
            TestCase {
                name: "Ties among connected players go to the lowest pid",
                players: vec![(1, 0, false), (2, 300, true), (3, 300, true)],
                expected_picker: 2,
            },
            TestCase {
                name: "With nobody connected, the lowest scorer picks anyway",
                players: vec![(1, 200, false), (2, -100, false), (3, 0, false)],
                expected_picker: 2,
            },
        ];

        for tc in test_cases {
            let mut room = create_two_round_room();
            let mut receivers = Vec::new();
            for (pid, score, connected) in &tc.players {
                if *connected {
                    receivers.push(add_connected_player(&mut room, *pid, "Connected"));
                } else {
                    add_test_player(&mut room, *pid, "Gone");
                }
                if let Some(entry) = room.players.last_mut() {
                    entry.player.score = *score;
                }
            }
            room.state = GameState::Selection;
            room.picker = Some(3);

            room.handle_command(&GameCommand::HostNextRound { round: 0 }, Sender::Host);

            assert_eq!(room.round_index, 1, "{}: round", tc.name);
            assert_eq!(room.picker, Some(tc.expected_picker), "{}", tc.name);
            drop(receivers);
        }
    }

    fn final_question() -> FinalQuestion {
        FinalQuestion {
            category: "Wisconsin Rivers".to_string(),
            question: "This river flows through Madison's lakes".to_string(),
            answer: "What is the Yahara?".to_string(),
        }
    }

    /// A two-round room that ends with Final Jeopardy
    fn create_final_room() -> Room {
        let mut room = create_two_round_room();
        room.final_question = Some(final_question());
        room
    }

    /// A single-round room that ends with Final Jeopardy
    fn create_single_round_final_room() -> Room {
        let mut room = create_test_room();
        room.final_question = Some(final_question());
        room
    }

    /// Final Jeopardy with only `pid` playing, whose response has been revealed but not
    /// judged
    fn revealed_final_round(pid: PlayerId) -> FinalRound {
        FinalRound {
            entries: BTreeMap::from([(
                pid,
                FinalEntry {
                    wager: Some(100),
                    answer: Some("Yahara".to_string()),
                    correct: None,
                    revealed: true,
                },
            )]),
            reveal_order: vec![pid],
            reveal_index: 0,
        }
    }

    /// A room at the start of Final Jeopardy, reached by ending Double Jeopardy, with a
    /// player for each score: pid 1, 2, ... in that order
    fn final_room(scores: &[i32]) -> Room {
        let mut room = create_final_room();
        for (i, score) in scores.iter().enumerate() {
            let pid = i as PlayerId + 1;
            add_test_player(&mut room, pid, &format!("Player {pid}"));
            room.players[i].player.score = *score;
        }
        room.round_index = 1;
        room.state = GameState::Selection;
        room.picker = Some(1);
        room.handle_command(&GameCommand::HostNextRound { round: 1 }, Sender::Host);
        assert_eq!(room.state, GameState::FinalWager);
        room
    }

    fn final_answer(room: &mut Room, pid: PlayerId, answer: &str) -> RoomResponse {
        room.handle_command(
            &GameCommand::SubmitAnswer {
                answer: answer.to_string(),
            },
            Sender::Player(pid),
        )
    }

    fn reveal_and_judge(room: &mut Room, pid: PlayerId, correct: bool) {
        room.handle_command(&GameCommand::HostRevealFinalResponse { pid }, Sender::Host);
        room.handle_command(&GameCommand::HostChecked { correct }, Sender::Host);
    }

    fn final_entry(room: &Room, pid: PlayerId) -> FinalEntry {
        room.final_round
            .as_ref()
            .and_then(|f| f.entries.get(&pid))
            .cloned()
            .unwrap_or_else(|| panic!("Player {pid} has no Final Jeopardy entry"))
    }

    fn score(room: &Room, pid: PlayerId) -> i32 {
        room.score_of(pid)
            .unwrap_or_else(|| panic!("Player {pid} isn't in the room"))
    }

    /// The Final Jeopardy fields of the PlayerState sent to `pid`, as
    /// `(wager range, wager, prompt, can answer, answer, correct)`
    type FinalPlayerView = (
        Option<WagerRange>,
        Option<u32>,
        Option<String>,
        bool,
        Option<String>,
        Option<bool>,
    );

    fn player_final_view(response: &RoomResponse, pid: PlayerId) -> Option<FinalPlayerView> {
        response
            .messages_to_specific
            .iter()
            .find_map(|(to, msg)| match msg {
                GameEvent::PlayerState {
                    wager_range,
                    wager,
                    prompt,
                    can_answer,
                    answer,
                    correct,
                    ..
                } if *to == pid => Some((
                    *wager_range,
                    *wager,
                    prompt.clone(),
                    *can_answer,
                    answer.clone(),
                    *correct,
                )),
                _ => None,
            })
    }

    /// The Final Jeopardy the host was last told about in `response`
    fn host_final_view(response: &RoomResponse) -> Option<FinalView> {
        response
            .messages_to_host
            .iter()
            .rev()
            .find_map(|msg| match msg {
                GameEvent::GameState { final_jeopardy, .. } => Some(final_jeopardy.clone()),
                _ => None,
            })
            .flatten()
    }

    #[test]
    fn test_final_jeopardy_after_double_jeopardy() {
        let mut room = create_final_room();
        add_test_player(&mut room, 1, "AJ");
        add_test_player(&mut room, 2, "Sam");

        let response = room.handle_command(&GameCommand::StartGame, Sender::Host);
        assert!(
            response.messages_to_host.iter().any(|msg| matches!(
                msg,
                GameEvent::GameState {
                    has_final: true,
                    final_jeopardy: None,
                    ..
                }
            )),
            "The host knows a Final Jeopardy is coming, but not what it is"
        );
        room.handle_command(&GameCommand::HostSetPicker { pid: 1 }, Sender::Host);

        // Jeopardy: AJ +200, Sam +400
        play_clue(&mut room, 0, 0, 1, true);
        play_clue(&mut room, 0, 1, 2, true);
        assert_eq!(room.round_index, 1);
        assert_eq!(room.state, GameState::Selection);

        // Double Jeopardy: AJ +400, Sam -800, and AJ doesn't try it
        play_clue(&mut room, 0, 0, 1, true);
        room.handle_command(
            &GameCommand::HostChoice {
                category_index: 0,
                question_index: 1,
            },
            Sender::Host,
        );
        room.handle_command(&GameCommand::HostReady, Sender::Host);
        buzz_and_judge(&mut room, 2, false);
        room.handle_command(&GameCommand::HostSkip, Sender::Host);
        let response = room.handle_command(&GameCommand::HostContinue, Sender::Host);
        assert_eq!(room.state, GameState::FinalWager);
        assert_eq!((score(&room, 1), score(&room, 2)), (600, -400));
        assert_eq!(room.winner, None, "Nobody wins before Final Jeopardy");

        let final_view = host_final_view(&response).expect("The host sees Final Jeopardy");
        assert_eq!(final_view.category, "Wisconsin Rivers");
        assert_eq!(final_view.answer, "What is the Yahara?");
        assert_eq!(
            final_view
                .entries
                .iter()
                .map(|e| (e.pid, e.wager))
                .collect::<Vec<_>>(),
            vec![(1, None), (2, Some(1))],
            "Sam has nothing to wager, so wagers $1"
        );
        assert_eq!(
            player_final_view(&response, 1),
            Some((
                Some(WagerRange { min: 0, max: 600 }),
                None,
                Some("Wisconsin Rivers".to_string()),
                false,
                None,
                None
            )),
            "AJ is asked to wager on the category"
        );
        assert_eq!(
            player_final_view(&response, 2),
            Some((
                None,
                Some(1),
                Some("Wisconsin Rivers".to_string()),
                false,
                None,
                None
            )),
            "Sam is told the $1 wager"
        );

        wager(&mut room, 1, 600);
        let response = room.handle_command(&GameCommand::HostRevealFinalClue, Sender::Host);
        assert_eq!(room.state, GameState::FinalAnswer);
        assert_eq!(
            player_final_view(&response, 1),
            Some((
                None,
                Some(600),
                Some("This river flows through Madison's lakes".to_string()),
                true,
                None,
                None
            )),
            "AJ is shown the clue"
        );

        final_answer(&mut room, 1, "What is the Yahara?");
        final_answer(&mut room, 2, "What is the Wisconsin?");
        room.handle_command(&GameCommand::HostLockFinalAnswers, Sender::Host);
        assert_eq!(room.state, GameState::FinalReveal);

        // Sam, in last place, is revealed first
        reveal_and_judge(&mut room, 2, false);
        reveal_and_judge(&mut room, 1, true);
        assert_eq!((score(&room, 1), score(&room, 2)), (1200, -401));
        assert_eq!(room.state, GameState::FinalReveal);
        assert_eq!(room.winner, None, "The host finishes the game");

        let response = room.handle_command(&GameCommand::HostContinue, Sender::Host);
        assert_eq!(room.state, GameState::GameEnd);
        assert_eq!(room.winner, Some(1));
        let final_view = host_final_view(&response).expect("The host still sees the results");
        assert_eq!(final_view.reveal_index, 2);
        assert_eq!(
            final_view
                .entries
                .iter()
                .map(|e| (e.pid, e.correct))
                .collect::<Vec<_>>(),
            vec![(2, Some(false)), (1, Some(true))]
        );
    }

    #[test]
    fn test_final_jeopardy_after_a_single_round() {
        let mut room = create_single_round_final_room();
        add_test_player(&mut room, 1, "AJ");
        room.handle_command(&GameCommand::StartGame, Sender::Host);
        room.handle_command(&GameCommand::HostSetPicker { pid: 1 }, Sender::Host);

        play_clue(&mut room, 0, 0, 1, true);
        play_clue(&mut room, 0, 1, 1, true);
        assert_eq!(room.state, GameState::FinalWager);
        assert_eq!(room.round_index, 0);

        wager(&mut room, 1, 0);
        room.handle_command(&GameCommand::HostRevealFinalClue, Sender::Host);
        room.handle_command(&GameCommand::HostLockFinalAnswers, Sender::Host);
        reveal_and_judge(&mut room, 1, false);
        room.handle_command(&GameCommand::HostContinue, Sender::Host);

        assert_eq!(room.state, GameState::GameEnd);
        assert_eq!(score(&room, 1), 600, "A $0 wager changes nothing");
        assert_eq!(room.winner, Some(1));
    }

    #[test]
    fn test_final_wager_bounds() {
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
                accepted: true,
            },
            TestCase {
                name: "Some",
                score: 1000,
                amount: 333,
                accepted: true,
            },
            TestCase {
                name: "All in",
                score: 1000,
                amount: 1000,
                accepted: true,
            },
            TestCase {
                name: "More than the score",
                score: 1000,
                amount: 1001,
                accepted: false,
            },
            TestCase {
                name: "All in with $1",
                score: 1,
                amount: 1,
                accepted: true,
            },
            TestCase {
                name: "$0 with a score of $0",
                score: 0,
                amount: 0,
                accepted: false,
            },
            TestCase {
                name: "$2 with a score of $0",
                score: 0,
                amount: 2,
                accepted: false,
            },
            TestCase {
                name: "$1 with a score of $0",
                score: 0,
                amount: 1,
                accepted: true,
            },
            TestCase {
                name: "$0 with a negative score",
                score: -400,
                amount: 0,
                accepted: false,
            },
            TestCase {
                name: "$2 with a negative score",
                score: -400,
                amount: 2,
                accepted: false,
            },
        ];

        for tc in test_cases {
            let mut room = final_room(&[tc.score]);
            let before = final_entry(&room, 1).wager;
            assert_eq!(
                before,
                (tc.score <= 0).then_some(1),
                "{}: starting wager",
                tc.name
            );

            let response = wager(&mut room, 1, tc.amount);

            assert_eq!(room.state, GameState::FinalWager, "{}: state", tc.name);
            if tc.accepted {
                assert_eq!(rejection(&response), None, "{}: rejected", tc.name);
                assert_eq!(
                    final_entry(&room, 1).wager,
                    Some(tc.amount),
                    "{}: wager",
                    tc.name
                );
                let (_, wager, ..) = player_final_view(&response, 1)
                    .unwrap_or_else(|| panic!("{}: player not told", tc.name));
                assert_eq!(wager, Some(tc.amount), "{}: player's view", tc.name);
                assert!(
                    response
                        .messages_to_host
                        .iter()
                        .any(|m| matches!(m, GameEvent::GameState { .. })),
                    "{}: host not told",
                    tc.name
                );
                assert!(
                    response.messages_to_players.is_empty(),
                    "{}: other players told",
                    tc.name
                );
            } else {
                let (to, reason) = rejection(&response)
                    .unwrap_or_else(|| panic!("{}: wager should be rejected", tc.name));
                assert_eq!(to, 1, "{}", tc.name);
                let expected = if tc.score > 0 { "$0" } else { "$1" };
                assert!(reason.contains(expected), "{}: reason {reason}", tc.name);
                assert_eq!(final_entry(&room, 1).wager, before, "{}: wager", tc.name);
                assert!(response.messages_to_host.is_empty(), "{}", tc.name);
            }
        }
    }

    #[test]
    fn test_final_wager_can_be_changed_until_the_clue() {
        let mut room = final_room(&[1000]);

        let response = wager(&mut room, 1, 1000);
        assert_eq!(
            player_final_view(&response, 1).map(|(range, wager, ..)| (range, wager)),
            Some((Some(WagerRange { min: 0, max: 1000 }), Some(1000))),
            "The form stays open after a wager"
        );
        wager(&mut room, 1, 250);
        assert_eq!(final_entry(&room, 1).wager, Some(250));

        room.handle_command(&GameCommand::HostRevealFinalClue, Sender::Host);
        let response = wager(&mut room, 1, 900);
        assert!(is_empty(&response), "Wagers close with the clue");
        assert_eq!(final_entry(&room, 1).wager, Some(250));
    }

    #[test]
    fn test_final_wrong_answer_with_nothing_loses_a_dollar() {
        for start in [0, -300] {
            let mut room = final_room(&[start]);
            room.handle_command(&GameCommand::HostRevealFinalClue, Sender::Host);
            room.handle_command(&GameCommand::HostLockFinalAnswers, Sender::Host);
            reveal_and_judge(&mut room, 1, false);
            assert_eq!(score(&room, 1), start - 1, "starting score {start}");
        }
    }

    #[test]
    fn test_final_missing_wagers_and_answers() {
        // A player with money who doesn't wager, and players with nothing who can't
        let mut room = final_room(&[800, 0, -200]);
        room.handle_command(&GameCommand::HostRevealFinalClue, Sender::Host);
        let wagers: Vec<_> = (1..=3).map(|pid| final_entry(&room, pid).wager).collect();
        assert_eq!(wagers, vec![Some(0), Some(1), Some(1)]);

        final_answer(&mut room, 2, "What is the Rock?");
        room.handle_command(&GameCommand::HostLockFinalAnswers, Sender::Host);
        let answers: Vec<_> = (1..=3).map(|pid| final_entry(&room, pid).answer).collect();
        assert_eq!(
            answers,
            vec![
                Some(String::new()),
                Some("What is the Rock?".to_string()),
                Some(String::new())
            ]
        );
    }

    #[test]
    fn test_final_answers() {
        struct TestCase {
            name: &'static str,
            answers: Vec<String>,
            expected: String,
        }

        let test_cases = vec![
            TestCase {
                name: "Trimmed",
                answers: vec!["  What is the Yahara?\n".to_string()],
                expected: "What is the Yahara?".to_string(),
            },
            TestCase {
                name: "Changed",
                answers: vec!["Wisconsin".to_string(), "Yahara".to_string()],
                expected: "Yahara".to_string(),
            },
            TestCase {
                name: "Cleared",
                answers: vec!["Wisconsin".to_string(), "   ".to_string()],
                expected: String::new(),
            },
            TestCase {
                name: "Cut off",
                answers: vec!["y".repeat(FINAL_ANSWER_MAX_CHARS + 50)],
                expected: "y".repeat(FINAL_ANSWER_MAX_CHARS),
            },
            TestCase {
                name: "Cut off by characters, not bytes",
                answers: vec!["é".repeat(FINAL_ANSWER_MAX_CHARS + 1)],
                expected: "é".repeat(FINAL_ANSWER_MAX_CHARS),
            },
            TestCase {
                name: "Cut off before a space",
                answers: vec![format!("{} more", "y".repeat(FINAL_ANSWER_MAX_CHARS - 1))],
                expected: "y".repeat(FINAL_ANSWER_MAX_CHARS - 1),
            },
        ];

        for tc in test_cases {
            let mut room = final_room(&[500, 500]);
            room.handle_command(&GameCommand::HostRevealFinalClue, Sender::Host);
            let mut response = RoomResponse::new();
            for answer in &tc.answers {
                response = final_answer(&mut room, 1, answer);
            }

            assert_eq!(room.state, GameState::FinalAnswer, "{}", tc.name);
            assert_eq!(
                final_entry(&room, 1).answer,
                Some(tc.expected.clone()),
                "{}",
                tc.name
            );
            assert_eq!(final_entry(&room, 2).answer, None, "{}", tc.name);
            let (.., can_answer, answer, _) = player_final_view(&response, 1)
                .unwrap_or_else(|| panic!("{}: player not told", tc.name));
            assert!(can_answer, "{}: can still change it", tc.name);
            assert_eq!(answer, Some(tc.expected), "{}: player's view", tc.name);
            assert!(
                player_final_view(&response, 2).is_none(),
                "{}: other player told",
                tc.name
            );
        }
    }

    #[test]
    fn test_final_answers_close_when_locked() {
        let mut room = final_room(&[500]);
        room.handle_command(&GameCommand::HostRevealFinalClue, Sender::Host);
        final_answer(&mut room, 1, "Yahara");
        let response = room.handle_command(&GameCommand::HostLockFinalAnswers, Sender::Host);
        assert_eq!(
            player_final_view(&response, 1).map(|(.., can_answer, answer, _)| (can_answer, answer)),
            Some((false, Some("Yahara".to_string())))
        );

        let response = final_answer(&mut room, 1, "Wisconsin");
        assert!(is_empty(&response));
        assert_eq!(final_entry(&room, 1).answer, Some("Yahara".to_string()));
    }

    #[test]
    fn test_final_reveal_order() {
        struct TestCase {
            name: &'static str,
            scores: Vec<i32>,
            expected_order: Vec<PlayerId>,
        }

        let test_cases = vec![
            TestCase {
                name: "Lowest score first",
                scores: vec![1200, -100, 800, 0],
                expected_order: vec![2, 4, 3, 1],
            },
            TestCase {
                name: "Ties go to the lowest pid",
                scores: vec![500, 300, 500, 300],
                expected_order: vec![2, 4, 1, 3],
            },
        ];

        for tc in test_cases {
            let mut room = final_room(&tc.scores);
            room.handle_command(&GameCommand::HostRevealFinalClue, Sender::Host);
            let response = room.handle_command(&GameCommand::HostLockFinalAnswers, Sender::Host);

            let final_round = room.final_round.as_ref().expect("Final Jeopardy started");
            assert_eq!(final_round.reveal_order, tc.expected_order, "{}", tc.name);
            assert_eq!(final_round.reveal_index, 0, "{}", tc.name);
            let host_order: Vec<PlayerId> = host_final_view(&response)
                .expect("The host sees Final Jeopardy")
                .entries
                .iter()
                .map(|e| e.pid)
                .collect();
            assert_eq!(host_order, tc.expected_order, "{}: host's order", tc.name);
        }
    }

    #[test]
    fn test_final_winner_is_decided_after_the_reveal() {
        // The leader bets everything and misses, and the trailer bets everything and hits
        let mut room = final_room(&[1000, 600]);
        wager(&mut room, 1, 1000);
        wager(&mut room, 2, 600);
        room.handle_command(&GameCommand::HostRevealFinalClue, Sender::Host);
        room.handle_command(&GameCommand::HostLockFinalAnswers, Sender::Host);
        reveal_and_judge(&mut room, 2, true);
        reveal_and_judge(&mut room, 1, false);
        assert_eq!(room.winner, None);

        room.handle_command(&GameCommand::HostContinue, Sender::Host);
        assert_eq!((score(&room, 1), score(&room, 2)), (0, 1200));
        assert_eq!(room.winner, Some(2));
    }

    #[test]
    fn test_final_repeated_clicks_skip_nobody() {
        let mut room = final_room(&[300, 200, 100]);
        for pid in 1..=3 {
            wager(&mut room, pid, 100);
        }
        room.handle_command(&GameCommand::HostRevealFinalClue, Sender::Host);
        room.handle_command(&GameCommand::HostLockFinalAnswers, Sender::Host);
        // Revealed in the order 3, 2, 1

        // Judging before anything is revealed does nothing
        let response =
            room.handle_command(&GameCommand::HostChecked { correct: true }, Sender::Host);
        assert!(is_empty(&response), "Judged an unrevealed response");

        // Revealing anyone but the next player does nothing
        for pid in [1, 2, 9] {
            let response =
                room.handle_command(&GameCommand::HostRevealFinalResponse { pid }, Sender::Host);
            assert!(is_empty(&response), "Revealed {pid} out of turn");
        }

        // A double click on Reveal reveals once
        room.handle_command(
            &GameCommand::HostRevealFinalResponse { pid: 3 },
            Sender::Host,
        );
        let response = room.handle_command(
            &GameCommand::HostRevealFinalResponse { pid: 3 },
            Sender::Host,
        );
        assert!(is_empty(&response), "Revealed twice");
        assert!(final_entry(&room, 3).revealed);
        assert!(!final_entry(&room, 2).revealed);

        // A double click on Correct judges once
        room.handle_command(&GameCommand::HostChecked { correct: true }, Sender::Host);
        let response =
            room.handle_command(&GameCommand::HostChecked { correct: true }, Sender::Host);
        assert!(is_empty(&response), "Judged the next player too");
        assert_eq!(final_entry(&room, 3).correct, Some(true));
        assert_eq!(final_entry(&room, 2).correct, None);
        assert_eq!((score(&room, 3), score(&room, 2)), (200, 200));

        // A stale reveal of someone already judged does nothing
        let response = room.handle_command(
            &GameCommand::HostRevealFinalResponse { pid: 3 },
            Sender::Host,
        );
        assert!(is_empty(&response), "Revealed a judged player again");

        reveal_and_judge(&mut room, 2, false);
        reveal_and_judge(&mut room, 1, true);
        let final_round = room.final_round.as_ref().expect("Final Jeopardy started");
        assert_eq!(final_round.reveal_index, 3);
        assert!(final_round.entries.values().all(|e| e.correct.is_some()));
        assert_eq!(
            (score(&room, 1), score(&room, 2), score(&room, 3)),
            (400, 100, 200)
        );

        // Nothing is left to reveal or judge
        let response =
            room.handle_command(&GameCommand::HostChecked { correct: false }, Sender::Host);
        assert!(is_empty(&response));
        assert_eq!(score(&room, 1), 400);
    }

    #[test]
    fn test_final_host_continue_waits_for_every_judgement() {
        let mut room = final_room(&[300, 200]);
        room.handle_command(&GameCommand::HostRevealFinalClue, Sender::Host);
        room.handle_command(&GameCommand::HostLockFinalAnswers, Sender::Host);

        let response = room.handle_command(&GameCommand::HostContinue, Sender::Host);
        assert!(is_empty(&response), "Nobody judged yet");

        reveal_and_judge(&mut room, 2, true);
        room.handle_command(
            &GameCommand::HostRevealFinalResponse { pid: 1 },
            Sender::Host,
        );
        let response = room.handle_command(&GameCommand::HostContinue, Sender::Host);
        assert!(is_empty(&response), "Revealed but not judged");
        assert_eq!(room.state, GameState::FinalReveal);
        assert_eq!(room.winner, None);

        room.handle_command(&GameCommand::HostChecked { correct: true }, Sender::Host);
        room.handle_command(&GameCommand::HostContinue, Sender::Host);
        assert_eq!(room.state, GameState::GameEnd);
        assert_eq!(room.winner, Some(1));
    }

    #[test]
    fn test_final_with_nobody_playing() {
        let mut room = final_room(&[]);
        room.handle_command(&GameCommand::HostRevealFinalClue, Sender::Host);
        room.handle_command(&GameCommand::HostLockFinalAnswers, Sender::Host);
        room.handle_command(&GameCommand::HostContinue, Sender::Host);
        assert_eq!(room.state, GameState::GameEnd);
        assert_eq!(room.winner, None);
    }

    /// Adds a player as joining the room does
    fn join(room: &mut Room, pid: PlayerId, name: &str) {
        let (tx, _rx) = channel(10);
        room.add_player(PlayerEntry::new(
            Player::new(pid, name.to_string(), 0, false, PlayerToken::generate()),
            tx,
        ));
    }

    #[test]
    fn test_final_late_joiners() {
        // Joining during the wager: plays for $1
        let mut room = final_room(&[500]);
        join(&mut room, 2, "Late");
        assert_eq!(final_entry(&room, 2).wager, Some(1));
        let response = wager(&mut room, 2, 0);
        assert!(
            rejection(&response).is_some(),
            "A late joiner can't wager $0"
        );
        let player_state = room.build_player_state_msg(2).expect("Late is in the room");
        assert!(matches!(
            player_state,
            GameEvent::PlayerState {
                wager_range: None,
                wager: Some(1),
                ..
            }
        ));

        // Joining during the clue: plays for $1 too, and can respond
        room.handle_command(&GameCommand::HostRevealFinalClue, Sender::Host);
        join(&mut room, 3, "Later");
        assert_eq!(final_entry(&room, 3).wager, Some(1));
        let response = final_answer(&mut room, 3, "Yahara");
        assert_eq!(rejection(&response), None);
        assert_eq!(final_entry(&room, 3).answer, Some("Yahara".to_string()));

        // Joining during the reveal: only watches
        room.handle_command(&GameCommand::HostLockFinalAnswers, Sender::Host);
        join(&mut room, 4, "Latest");
        let final_round = room.final_round.as_ref().expect("Final Jeopardy started");
        assert!(!final_round.entries.contains_key(&4));
        assert_eq!(final_round.reveal_order, vec![2, 3, 1]);
        let player_state = room
            .build_player_state_msg(4)
            .expect("Latest is in the room");
        assert!(matches!(
            player_state,
            GameEvent::PlayerState {
                wager: None,
                prompt: None,
                can_answer: false,
                answer: None,
                ..
            }
        ));

        reveal_and_judge(&mut room, 2, false);
        reveal_and_judge(&mut room, 3, true);
        reveal_and_judge(&mut room, 1, true);
        room.handle_command(&GameCommand::HostContinue, Sender::Host);
        assert_eq!(room.state, GameState::GameEnd);
        assert_eq!(
            (1..=4).map(|pid| score(&room, pid)).collect::<Vec<_>>(),
            vec![500, -1, 1, 0]
        );
    }

    #[test]
    fn test_final_late_joiner_during_the_reveal_cannot_play() {
        let mut room = final_room(&[500]);
        room.handle_command(&GameCommand::HostRevealFinalClue, Sender::Host);
        room.handle_command(&GameCommand::HostLockFinalAnswers, Sender::Host);
        join(&mut room, 2, "Latest");
        // The state check comes first, so this is ignored rather than rejected
        let response = final_answer(&mut room, 2, "Yahara");
        assert!(is_empty(&response));
    }

    #[test]
    fn test_final_jeopardy_privacy() {
        let mut room = final_room(&[3000, 2000, -500]);
        let wagers = [(1, 2345), (2, 1789)];
        let answers = [
            (1, "Secret response from AJ"),
            (2, "Secret response from Sam"),
            (3, "Secret response from Lee"),
        ];
        let correct_response = final_question().answer;

        // Each response, with whether the clue's correct response may be in it
        let mut responses = vec![];
        for (pid, amount) in wagers {
            responses.push(wager(&mut room, pid, amount));
        }
        responses.push(room.handle_command(&GameCommand::HostRevealFinalClue, Sender::Host));
        for (pid, answer) in answers {
            responses.push(final_answer(&mut room, pid, answer));
        }
        responses.push(room.handle_command(&GameCommand::HostLockFinalAnswers, Sender::Host));
        for (pid, correct) in [(3, false), (2, true), (1, false)] {
            responses.push(
                room.handle_command(&GameCommand::HostRevealFinalResponse { pid }, Sender::Host),
            );
            responses
                .push(room.handle_command(&GameCommand::HostChecked { correct }, Sender::Host));
        }
        responses.push(room.handle_command(&GameCommand::HostContinue, Sender::Host));
        assert_eq!(room.state, GameState::GameEnd);
        assert_eq!(room.winner, Some(2));

        let secrets_of = |pid: PlayerId| {
            let mut secrets: Vec<String> = answers
                .iter()
                .filter(|(p, _)| *p == pid)
                .map(|(_, a)| a.to_string())
                .collect();
            secrets.extend(
                wagers
                    .iter()
                    .filter(|(p, _)| *p == pid)
                    .map(|(_, w)| w.to_string()),
            );
            secrets
        };

        let mut checked = 0;
        for response in &responses {
            for msg in response
                .messages_to_players
                .iter()
                .chain(response.witness.iter())
            {
                let json = serde_json::to_string(msg).expect("Failed to serialize event");
                assert!(
                    !json.contains(&correct_response),
                    "Sent to everyone: {json}"
                );
                for pid in 1..=3 {
                    for secret in secrets_of(pid) {
                        assert!(!json.contains(&secret), "Sent to everyone: {json}");
                    }
                }
                checked += 1;
            }
            for (to, msg) in &response.messages_to_specific {
                let json = serde_json::to_string(msg).expect("Failed to serialize event");
                assert!(!json.contains(&correct_response), "Sent to {to}: {json}");
                for pid in (1..=3).filter(|pid| pid != to) {
                    for secret in secrets_of(pid) {
                        assert!(
                            !json.contains(&secret),
                            "{pid}'s {secret} sent to {to}: {json}"
                        );
                    }
                }
                checked += 1;
            }
        }
        assert!(checked > 0);

        // The host is told everything
        let final_view = responses
            .iter()
            .rev()
            .find_map(host_final_view)
            .expect("The host sees Final Jeopardy");
        assert_eq!(final_view.answer, correct_response);
        assert_eq!(
            final_view
                .entries
                .iter()
                .map(|e| (e.pid, e.wager, e.revealed))
                .collect::<Vec<_>>(),
            vec![
                (3, Some(1), true),
                (2, Some(1789), true),
                (1, Some(2345), true)
            ]
        );
    }
}
