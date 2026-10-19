use rand::{Rng, seq::IndexedRandom};
use serde::{Deserialize, Serialize};

use crate::{api::messages::GameEvent, player::PlayerId};

pub mod room;

/// How many Daily Doubles are hidden in each round when they're placed at random: one in
/// Jeopardy and two in Double Jeopardy, as on TV
pub const RANDOM_DAILY_DOUBLES: [usize; 2] = [1, 2];

/// Hides [`RANDOM_DAILY_DOUBLES`] at random in each round whose board has none, leaving
/// rounds the game file already placed Daily Doubles in alone. They never go in the top
/// row, and each goes in a different category. A board too small for them all gets as
/// many as fit.
pub fn place_random_daily_doubles<R: Rng + ?Sized>(rounds: &mut [Vec<Category>], rng: &mut R) {
    for (round, count) in rounds.iter_mut().zip(RANDOM_DAILY_DOUBLES) {
        if round
            .iter()
            .flat_map(|cat| &cat.questions)
            .any(|q| q.daily_double)
        {
            continue;
        }

        // Every unanswered clue below the top row, by category
        let candidates: Vec<Vec<usize>> = round
            .iter()
            .map(|cat| {
                cat.questions
                    .iter()
                    .enumerate()
                    .skip(1)
                    .filter(|(_, q)| !q.answered)
                    .map(|(q_idx, _)| q_idx)
                    .collect()
            })
            .collect();
        let categories: Vec<usize> = (0..round.len())
            .filter(|&cat_idx| !candidates[cat_idx].is_empty())
            .collect();

        for &cat_idx in categories.choose_multiple(rng, count) {
            if let Some(&q_idx) = candidates[cat_idx].choose(rng)
                && let Some(question) = round
                    .get_mut(cat_idx)
                    .and_then(|cat| cat.questions.get_mut(q_idx))
            {
                question.daily_double = true;
            }
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Question {
    pub question: String,
    pub answer: String,
    pub value: u32,
    #[serde(default)]
    pub answered: bool,
    /// Read from the game file when the room is created, but never sent to anyone, host
    /// included: Daily Doubles are a surprise until a player chooses one
    #[serde(default, rename = "dailyDouble", skip_serializing)]
    pub daily_double: bool,
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
    /// The picker chose a Daily Double and is deciding how much to wager
    DailyDoubleWager,
    Answer,
    AnswerReveal,
    WaitingForBuzz,
    GameEnd,
}

#[cfg(test)]
mod test {
    use rand::{SeedableRng, rngs::StdRng};

    use super::{Category, Question, RANDOM_DAILY_DOUBLES, place_random_daily_doubles};

    /// A board of `categories` categories with `clues` clues each, worth 200, 400, ...
    fn board(categories: usize, clues: usize) -> Vec<Category> {
        (0..categories)
            .map(|cat_idx| Category {
                title: format!("Category {cat_idx}"),
                questions: (0..clues)
                    .map(|q_idx| Question {
                        question: format!("Question {q_idx}"),
                        answer: format!("Answer {q_idx}"),
                        value: (q_idx as u32 + 1) * 200,
                        answered: false,
                        daily_double: false,
                    })
                    .collect(),
            })
            .collect()
    }

    /// The `(category, clue)` of every Daily Double in a round
    fn daily_doubles(round: &[Category]) -> Vec<(usize, usize)> {
        round
            .iter()
            .enumerate()
            .flat_map(|(cat_idx, cat)| {
                cat.questions
                    .iter()
                    .enumerate()
                    .filter(|(_, q)| q.daily_double)
                    .map(move |(q_idx, _)| (cat_idx, q_idx))
            })
            .collect()
    }

    #[test]
    fn test_random_daily_doubles() {
        for seed in 0..200 {
            let mut rng = StdRng::seed_from_u64(seed);
            let mut rounds = vec![board(6, 5), board(6, 5)];

            place_random_daily_doubles(&mut rounds, &mut rng);

            for (round, expected) in rounds.iter().zip(RANDOM_DAILY_DOUBLES) {
                let placed = daily_doubles(round);
                assert_eq!(placed.len(), expected, "seed {seed}: {placed:?}");
                assert!(
                    placed.iter().all(|&(_, q_idx)| q_idx != 0),
                    "seed {seed}: Daily Double in the top row: {placed:?}"
                );
                let mut categories: Vec<usize> = placed.iter().map(|&(c, _)| c).collect();
                categories.sort_unstable();
                categories.dedup();
                assert_eq!(
                    categories.len(),
                    placed.len(),
                    "seed {seed}: Daily Doubles share a category: {placed:?}"
                );
            }
        }
    }

    #[test]
    fn test_random_daily_doubles_reach_every_allowed_clue() {
        let mut rng = StdRng::seed_from_u64(7);
        let mut seen = std::collections::HashSet::new();
        for _ in 0..500 {
            let mut rounds = vec![board(2, 3)];
            place_random_daily_doubles(&mut rounds, &mut rng);
            seen.extend(daily_doubles(&rounds[0]));
        }
        let mut seen: Vec<_> = seen.into_iter().collect();
        seen.sort();
        assert_eq!(seen, vec![(0, 1), (0, 2), (1, 1), (1, 2)]);
    }

    #[test]
    fn test_random_daily_doubles_keep_the_game_files() {
        let mut rng = StdRng::seed_from_u64(1);
        let mut rounds = vec![board(6, 5), board(6, 5)];
        rounds[0][3].questions[0].daily_double = true;
        rounds[1][2].questions[4].daily_double = true;

        place_random_daily_doubles(&mut rounds, &mut rng);

        assert_eq!(daily_doubles(&rounds[0]), vec![(3, 0)]);
        assert_eq!(daily_doubles(&rounds[1]), vec![(2, 4)]);
    }

    #[test]
    fn test_random_daily_doubles_only_fill_unflagged_rounds() {
        let mut rng = StdRng::seed_from_u64(2);
        let mut rounds = vec![board(6, 5), board(6, 5)];
        rounds[0][1].questions[2].daily_double = true;

        place_random_daily_doubles(&mut rounds, &mut rng);

        assert_eq!(daily_doubles(&rounds[0]), vec![(1, 2)]);
        assert_eq!(daily_doubles(&rounds[1]).len(), 2);
    }

    #[test]
    fn test_random_daily_doubles_on_small_boards() {
        struct TestCase {
            name: &'static str,
            rounds: Vec<Vec<Category>>,
            expected_counts: Vec<usize>,
        }

        let test_cases = vec![
            TestCase {
                name: "Single round",
                rounds: vec![board(6, 5)],
                expected_counts: vec![1],
            },
            TestCase {
                name: "Only top-row clues",
                rounds: vec![board(6, 1), board(6, 1)],
                expected_counts: vec![0, 0],
            },
            TestCase {
                name: "One category in Double Jeopardy",
                rounds: vec![board(1, 5), board(1, 5)],
                expected_counts: vec![1, 1],
            },
            TestCase {
                name: "Empty boards",
                rounds: vec![vec![], vec![]],
                expected_counts: vec![0, 0],
            },
        ];

        for mut tc in test_cases {
            let mut rng = StdRng::seed_from_u64(3);
            place_random_daily_doubles(&mut tc.rounds, &mut rng);
            let counts: Vec<usize> = tc.rounds.iter().map(|r| daily_doubles(r).len()).collect();
            assert_eq!(counts, tc.expected_counts, "{}", tc.name);
        }
    }

    #[test]
    fn test_daily_double_flag_is_read_but_never_sent() {
        let flagged: Question = serde_json::from_str(
            r#"{"question": "Q", "answer": "A", "value": 400, "dailyDouble": true}"#,
        )
        .expect("Failed to deserialize a flagged clue");
        assert!(flagged.daily_double);

        let unflagged: Question =
            serde_json::from_str(r#"{"question": "Q", "answer": "A", "value": 400}"#)
                .expect("Failed to deserialize a clue without a flag");
        assert!(!unflagged.daily_double);

        let json = serde_json::to_string(&flagged).expect("Failed to serialize");
        assert!(!json.contains("dailyDouble"), "Flag sent: {json}");
        assert!(!json.contains("daily_double"), "Flag sent: {json}");
    }
}
