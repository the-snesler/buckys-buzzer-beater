use std::sync::Arc;

use anyhow::anyhow;
use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};
use rand::Rng;
use serde::{Deserialize, Serialize};

use crate::{
    AppState, Room,
    api::handlers::RoomParams,
    game::{Category, FinalQuestion, place_random_daily_doubles},
    net::connection::{HostToken, RoomCode},
};

#[tracing::instrument(skip(state, body))]
pub async fn create_room(
    State(state): State<Arc<AppState>>,
    Json(body): Json<CreateRoomRequest>,
) -> (StatusCode, Json<CreateRoomResponse>) {
    // Built before taking the lock, which every room shares
    let final_question = body.final_question();
    let rounds = body.into_rounds(&mut rand::rng());

    let mut room_map = state.room_map.lock().await;

    // Generate a unique room code
    let code = loop {
        let candidate = RoomCode::generate();
        if !room_map.contains_key(&candidate.to_string()) {
            break candidate;
        }
    };

    let host_token = HostToken::generate();
    let mut room = Room::new(code.clone(), host_token.clone());

    room.rounds = rounds;
    room.final_question = final_question;

    room_map.insert(code.to_string(), room);

    tracing::info!(room_code = %code, "Room created");

    (
        StatusCode::CREATED,
        Json(CreateRoomResponse {
            room_code: code,
            host_token,
        }),
    )
}

#[derive(Serialize)]
pub struct CreateRoomResponse {
    room_code: RoomCode,
    host_token: HostToken,
}

#[derive(Deserialize)]
pub struct CreateRoomRequest {
    /// The Jeopardy round's board
    categories: Option<Vec<Category>>,
    /// The Double Jeopardy round's board. Empty if the game has no Double Jeopardy.
    #[serde(default, rename = "doubleCategories")]
    double_categories: Option<Vec<Category>>,
    /// Whether to hide Daily Doubles at random in rounds whose clues have none
    #[serde(default, rename = "randomDailyDoubles")]
    random_daily_doubles: Option<bool>,
    /// The Final Jeopardy clue. Null, omitted or without a clue if the game has none.
    #[serde(default, rename = "final")]
    final_question: Option<FinalQuestion>,
}

impl CreateRoomRequest {
    /// The Final Jeopardy clue, if the game has one. A clue that is only whitespace counts
    /// as none.
    fn final_question(&self) -> Option<FinalQuestion> {
        self.final_question
            .clone()
            .filter(|q| !q.question.trim().is_empty())
    }

    /// The board of each round, in order. A game without Double Jeopardy has one round.
    fn into_rounds<R: Rng + ?Sized>(self, rng: &mut R) -> Vec<Vec<Category>> {
        let mut rounds = vec![self.categories.unwrap_or_default()];
        let double_categories = self.double_categories.unwrap_or_default();
        if !double_categories.is_empty() {
            rounds.push(double_categories);
        }
        if self.random_daily_doubles.unwrap_or_default() {
            place_random_daily_doubles(&mut rounds, rng);
        }
        rounds
    }
}

#[tracing::instrument(skip(state), fields(room_code = %rp.code))]
pub async fn cpr_handler(
    State(state): State<Arc<AppState>>,
    Path(rp @ RoomParams { .. }): Path<RoomParams>,
) -> String {
    tracing::info!("cpr");
    let code = rp.code;
    let res = {
        let mut room_map = state.room_map.lock().await;
        let room_res = room_map
            .get_mut(&code)
            .ok_or_else(|| anyhow!("Room {} does not exist", code));
        let mut failures = 0_u32;
        match room_res {
            Err(e) => Err(e),
            Ok(room) => {
                for entry in &mut room.players {
                    match entry.heartbeat() {
                        Ok(()) => {}
                        Err(e) => {
                            tracing::warn!(
                                player_id = entry.player.pid,
                                error = %e,
                                "Heartbeat failed"
                            );
                            failures += 1;
                        }
                    }
                }
                Ok(format!(
                    "Ok, requested {} heartbeats, {} failed immediately",
                    room.players.len(),
                    failures
                ))
            }
        }
    };
    match res {
        Ok(s) => s,
        Err(e) => {
            tracing::error!(error = %e, "CPR handler failed");
            format!("Err, {e}")
        }
    }
}

#[cfg(test)]
mod test {
    use rand::{SeedableRng, rngs::StdRng};

    use super::CreateRoomRequest;
    use crate::game::FinalQuestion;

    #[test]
    fn test_create_room_request_rounds() {
        struct TestCase {
            name: &'static str,
            json: &'static str,
            expected_titles: Vec<Vec<&'static str>>,
        }

        let test_cases = vec![
            TestCase {
                name: "No categories",
                json: r#"{}"#,
                expected_titles: vec![vec![]],
            },
            TestCase {
                name: "Single round only",
                json: r#"{"categories": [
                    {"title": "Single", "questions": [{"question": "Q", "answer": "A", "value": 200}]}
                ]}"#,
                expected_titles: vec![vec!["Single"]],
            },
            TestCase {
                name: "Empty Double Jeopardy",
                json: r#"{"categories": [
                    {"title": "Single", "questions": [{"question": "Q", "answer": "A", "value": 200}]}
                ], "doubleCategories": []}"#,
                expected_titles: vec![vec!["Single"]],
            },
            TestCase {
                name: "Null Double Jeopardy",
                json: r#"{"categories": [
                    {"title": "Single", "questions": [{"question": "Q", "answer": "A", "value": 200}]}
                ], "doubleCategories": null}"#,
                expected_titles: vec![vec!["Single"]],
            },
            TestCase {
                name: "Single and Double Jeopardy",
                json: r#"{"categories": [
                    {"title": "Single", "questions": [{"question": "Q", "answer": "A", "value": 200}]}
                ], "doubleCategories": [
                    {"title": "Double 1", "questions": [{"question": "Q", "answer": "A", "value": 400}]},
                    {"title": "Double 2", "questions": [{"question": "Q", "answer": "A", "value": 800}]}
                ]}"#,
                expected_titles: vec![vec!["Single"], vec!["Double 1", "Double 2"]],
            },
        ];

        for tc in test_cases {
            let request: CreateRoomRequest = serde_json::from_str(tc.json)
                .unwrap_or_else(|e| panic!("{}: failed to deserialize: {}", tc.name, e));
            let titles: Vec<Vec<String>> = request
                .into_rounds(&mut StdRng::seed_from_u64(0))
                .iter()
                .map(|round| round.iter().map(|c| c.title.clone()).collect())
                .collect();
            assert_eq!(titles, tc.expected_titles, "{}", tc.name);
        }
    }

    #[test]
    fn test_create_room_request_final() {
        struct TestCase {
            name: &'static str,
            json: &'static str,
            expected: Option<FinalQuestion>,
        }

        let final_question = || FinalQuestion {
            category: "Rivers".to_string(),
            question: "It flows through Madison".to_string(),
            answer: "What is the Yahara?".to_string(),
        };
        let test_cases = vec![
            TestCase {
                name: "Omitted",
                json: r#"{"categories": []}"#,
                expected: None,
            },
            TestCase {
                name: "Null",
                json: r#"{"categories": [], "final": null}"#,
                expected: None,
            },
            TestCase {
                name: "Empty clue",
                json: r#"{"final": {"category": "Rivers", "question": "", "answer": "A"}}"#,
                expected: None,
            },
            TestCase {
                name: "Blank clue",
                json: r#"{"final": {"category": "Rivers", "question": "  ", "answer": "A"}}"#,
                expected: None,
            },
            TestCase {
                name: "No clue",
                json: r#"{"final": {"category": "Rivers"}}"#,
                expected: None,
            },
            TestCase {
                name: "A Final Jeopardy clue",
                json: r#"{"final": {"category": "Rivers", "question": "It flows through Madison", "answer": "What is the Yahara?"}}"#,
                expected: Some(final_question()),
            },
        ];

        for tc in test_cases {
            let request: CreateRoomRequest = serde_json::from_str(tc.json)
                .unwrap_or_else(|e| panic!("{}: failed to deserialize: {}", tc.name, e));
            assert_eq!(request.final_question(), tc.expected, "{}", tc.name);
        }
    }

    #[test]
    fn test_create_room_request_daily_doubles() {
        struct TestCase {
            name: &'static str,
            json: &'static str,
            /// The number of Daily Doubles in each round
            expected_counts: Vec<usize>,
        }

        // Three clues per category, so there is room below the top row
        const BOARDS: &str = r#""categories": [
            {"title": "S1", "questions": [
                {"question": "Q", "answer": "A", "value": 200},
                {"question": "Q", "answer": "A", "value": 400},
                {"question": "Q", "answer": "A", "value": 600}]},
            {"title": "S2", "questions": [
                {"question": "Q", "answer": "A", "value": 200},
                {"question": "Q", "answer": "A", "value": 400},
                {"question": "Q", "answer": "A", "value": 600}]}
        ], "doubleCategories": [
            {"title": "D1", "questions": [
                {"question": "Q", "answer": "A", "value": 400},
                {"question": "Q", "answer": "A", "value": 800},
                {"question": "Q", "answer": "A", "value": 1200, "dailyDouble": true}]},
            {"title": "D2", "questions": [
                {"question": "Q", "answer": "A", "value": 400},
                {"question": "Q", "answer": "A", "value": 800},
                {"question": "Q", "answer": "A", "value": 1200}]}
        ]"#;

        let test_cases = vec![
            TestCase {
                name: "Only the game file's Daily Doubles by default",
                json: "{BOARDS}",
                expected_counts: vec![0, 1],
            },
            TestCase {
                name: "Null randomDailyDoubles",
                json: r#"{BOARDS, "randomDailyDoubles": null}"#,
                expected_counts: vec![0, 1],
            },
            TestCase {
                name: "randomDailyDoubles off",
                json: r#"{BOARDS, "randomDailyDoubles": false}"#,
                expected_counts: vec![0, 1],
            },
            TestCase {
                name: "randomDailyDoubles fills the round without any",
                json: r#"{BOARDS, "randomDailyDoubles": true}"#,
                expected_counts: vec![1, 1],
            },
        ];

        for tc in test_cases {
            let json = tc.json.replace("BOARDS", BOARDS);
            let request: CreateRoomRequest = serde_json::from_str(&json)
                .unwrap_or_else(|e| panic!("{}: failed to deserialize: {}", tc.name, e));
            let counts: Vec<usize> = request
                .into_rounds(&mut StdRng::seed_from_u64(0))
                .iter()
                .map(|round| {
                    round
                        .iter()
                        .flat_map(|c| &c.questions)
                        .filter(|q| q.daily_double)
                        .count()
                })
                .collect();
            assert_eq!(counts, tc.expected_counts, "{}", tc.name);
        }
    }
}
