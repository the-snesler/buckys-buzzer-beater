use serde::{Deserialize, Serialize};

use crate::{
    Player, PlayerId,
    game::{Category, GameState},
    net::connection::PlayerToken,
};

/// Commands sent from clients to the server.
///
/// # Examples
///
/// Deserialize a start game command:
/// ```
/// use madhacks2025::api::messages::GameCommand;
///
/// let json = r#"{"type": "StartGame"}"#;
/// let cmd: GameCommand = serde_json::from_str(json).unwrap();
/// assert!(matches!(cmd, GameCommand::StartGame));
/// ```
///
/// Deserialize a host choice with parameters:
/// ```
/// use madhacks2025::api::messages::GameCommand;
/// let json = r#"{"type": "HostChoice", "categoryIndex": 2, "questionIndex": 3}"#;
/// let cmd: GameCommand = serde_json::from_str(json).unwrap();
/// match cmd {
///     GameCommand::HostChoice { category_index, question_index } => {
///         assert_eq!(category_index, 2);
///         assert_eq!(question_index, 3);
///     }
///     _ => panic!("Wrong variant"),
/// }
/// ```
#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(tag = "type")]
pub enum GameCommand {
    StartGame,
    EndGame,
    Buzz,
    HostReady,
    HostChoice {
        #[serde(rename = "categoryIndex")]
        category_index: usize,
        #[serde(rename = "questionIndex")]
        question_index: usize,
    },
    HostChecked {
        correct: bool,
    },
    HostSkip,
    HostContinue,
    Heartbeat {
        hbid: u32,
        #[serde(rename = "tDohbRecv")]
        t_dohb_recv: u64,
    },
    LatencyOfHeartbeat {
        hbid: u32,
        #[serde(rename = "tLat")]
        t_lat: u64,
    },
}

impl GameCommand {
    /// Helper to identify if a command should be echoed to others via witness system.
    ///
    /// # Examples
    ///
    /// ```
    /// use madhacks2025::api::messages::GameCommand;
    ///
    /// let cmd = GameCommand::HostReady;
    /// assert!(cmd.should_witness(), "HostReady needs synchronization");
    ///
    /// let cmd = GameCommand::Buzz;
    /// assert!(!cmd.should_witness(), "Buzz is handled directly");
    /// ```
    pub fn should_witness(&self) -> bool {
        matches!(self, Self::HostReady)
    }
}

/// Events sent from server to clients.
///
/// # Examples
///
/// Serialize a player state event:
/// ```
/// use madhacks2025::api::messages::GameEvent;
///
/// let event = GameEvent::PlayerState {
///     pid: 1,
///     buzzed: false,
///     score: 500,
///     can_buzz: true,
/// };
/// let json = serde_json::to_string(&event).unwrap();
/// assert!(json.contains("PlayerState"));
/// ```
#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum GameEvent {
    Witness {
        msg: Box<GameEvent>,
    },
    DoHeartbeat {
        hbid: u32,
        t_sent: u64,
    },
    GotHeartbeat {
        hbid: u32,
    },
    PlayerList(Vec<Player>),
    NewPlayer {
        pid: PlayerId,
        token: PlayerToken,
    },
    GameState {
        state: GameState,
        categories: Vec<Category>,
        players: Vec<Player>,
        #[serde(rename = "currentQuestion")]
        current_question: Option<(usize, usize)>,
        #[serde(rename = "currentBuzzer")]
        current_buzzer: Option<PlayerId>,
        winner: Option<PlayerId>,
    },
    PlayerState {
        pid: PlayerId,
        buzzed: bool,
        score: i32,
        #[serde(rename = "canBuzz")]
        can_buzz: bool,
    },
    PlayerBuzzed {
        pid: PlayerId,
        name: String,
    },
}

#[cfg(test)]
mod test {
    use crate::{
        api::messages::{GameCommand, GameEvent},
        game::GameState,
        net::connection::PlayerToken,
    };

    #[test]
    fn test_game_command_deserialize() {
        struct TestCase {
            name: &'static str,
            json: &'static str,
            expected: GameCommand,
        }

        let test_cases = vec![
            TestCase {
                name: "StartGame",
                json: r#"{"type": "StartGame"}"#,
                expected: GameCommand::StartGame,
            },
            TestCase {
                name: "EndGame",
                json: r#"{"type": "EndGame"}"#,
                expected: GameCommand::EndGame,
            },
            TestCase {
                name: "Buzz",
                json: r#"{"type": "Buzz"}"#,
                expected: GameCommand::Buzz,
            },
            TestCase {
                name: "HostReady",
                json: r#"{"type": "HostReady"}"#,
                expected: GameCommand::HostReady,
            },
            TestCase {
                name: "HostSkip",
                json: r#"{"type": "HostSkip"}"#,
                expected: GameCommand::HostSkip,
            },
            TestCase {
                name: "HostContinue",
                json: r#"{"type": "HostContinue"}"#,
                expected: GameCommand::HostContinue,
            },
            TestCase {
                name: "HostChoice",
                json: r#"{"type": "HostChoice", "categoryIndex": 2, "questionIndex": 3}"#,
                expected: GameCommand::HostChoice {
                    category_index: 2,
                    question_index: 3,
                },
            },
            TestCase {
                name: "HostChecked correct",
                json: r#"{"type": "HostChecked", "correct": true}"#,
                expected: GameCommand::HostChecked { correct: true },
            },
            TestCase {
                name: "HostChecked incorrect",
                json: r#"{"type": "HostChecked", "correct": false}"#,
                expected: GameCommand::HostChecked { correct: false },
            },
            TestCase {
                name: "Heartbeat",
                json: r#"{"type": "Heartbeat", "hbid": 12345, "tDohbRecv": 1609459200000}"#,
                expected: GameCommand::Heartbeat {
                    hbid: 12345,
                    t_dohb_recv: 1609459200000,
                },
            },
            TestCase {
                name: "LatencyOfHeartbeat",
                json: r#"{"type": "LatencyOfHeartbeat", "hbid": 67890, "tLat": 50}"#,
                expected: GameCommand::LatencyOfHeartbeat {
                    hbid: 67890,
                    t_lat: 50,
                },
            },
        ];

        for tc in test_cases {
            let result: Result<GameCommand, _> = serde_json::from_str(tc.json);
            assert!(
                result.is_ok(),
                "Failed to deserialize {}: {:?}",
                tc.name,
                result.err()
            );
            let cmd = result.unwrap();
            assert_eq!(
                format!("{:?}", cmd),
                format!("{:?}", tc.expected),
                "Mismatch for {}",
                tc.name
            );
        }
    }

    #[test]
    fn test_game_command_deserialize_errors() {
        struct TestCase {
            name: &'static str,
            json: &'static str,
        }

        let test_cases = vec![
            TestCase {
                name: "Invalid command type",
                json: r#"{"type": InvalidCommand}"#,
            },
            TestCase {
                name: "Missing required field",
                json: r#"{"type": "HostChoice", "categoryIndex: 2"}"#,
            },
            TestCase {
                name: "Empty JSON",
                json: r#"{}"#,
            },
            TestCase {
                name: "Invalid JSON syntax",
                json: r#"{"type": "StartGame""#,
            },
        ];

        for tc in test_cases {
            let result: Result<GameCommand, _> = serde_json::from_str(tc.json);
            assert!(result.is_err(), "{} should fail to deserialize", tc.name);
        }
    }

    #[test]
    fn test_game_event_serialize() {
        struct TestCase {
            name: &'static str,
            event: GameEvent,
            expected_substrings: Vec<&'static str>,
        }

        let test_cases = vec![
            TestCase {
                name: "PlayerList",
                event: GameEvent::PlayerList(vec![]),
                expected_substrings: vec!["PlayerList"],
            },
            TestCase {
                name: "NewPlayer",
                event: GameEvent::NewPlayer {
                    pid: 1,
                    token: PlayerToken::generate(),
                },
                expected_substrings: vec!["NewPlayer", r#""pid":1"#, r#""token""#],
            },
            TestCase {
                name: "GameState",
                event: GameEvent::GameState {
                    state: GameState::Start,
                    categories: vec![],
                    players: vec![],
                    current_question: None,
                    current_buzzer: None,
                    winner: None,
                },
                expected_substrings: vec!["GameState", r#""state""#, r#""categories""#],
            },
            TestCase {
                name: "PlayerState with camelCase",
                event: GameEvent::PlayerState {
                    pid: 1,
                    buzzed: true,
                    score: 500,
                    can_buzz: false,
                },
                expected_substrings: vec![
                    "PlayerState",
                    r#""pid":1"#,
                    r#""buzzed":true"#,
                    r#""score":500"#,
                    r#""canBuzz":false"#,
                ],
            },
            TestCase {
                name: "PlayerBuzzed",
                event: GameEvent::PlayerBuzzed {
                    pid: 2,
                    name: "PlayerName".to_string(),
                },
                expected_substrings: vec!["PlayerBuzzed", r#""pid":2"#, "PlayerName"],
            },
            TestCase {
                name: "DoHeartbeat",
                event: GameEvent::DoHeartbeat {
                    hbid: 123,
                    t_sent: 1609459200000,
                },
                expected_substrings: vec!["DoHeartbeat", r#""hbid":123"#, "1609459200000"],
            },
            TestCase {
                name: "GotHeartbeat",
                event: GameEvent::GotHeartbeat { hbid: 456 },
                expected_substrings: vec!["GotHeartbeat", r#""hbid":456"#],
            },
            TestCase {
                name: "Witness nested",
                event: GameEvent::Witness {
                    msg: Box::new(GameEvent::PlayerBuzzed {
                        pid: 1,
                        name: "Bob".to_string(),
                    }),
                },
                expected_substrings: vec!["Witness", r#""msg""#, "PlayerBuzzed"],
            },
        ];

        for tc in test_cases {
            let json = serde_json::to_string(&tc.event)
                .unwrap_or_else(|e| panic!("Failed to serialize {}: {}", tc.name, e));

            for expected in &tc.expected_substrings {
                assert!(
                    json.contains(expected),
                    "{}: Expected substring '{}' not found in JSON: {}",
                    tc.name,
                    expected,
                    json
                );
            }
        }
    }

    #[test]
    fn test_should_witness() {
        struct TestCase {
            name: &'static str,
            command: GameCommand,
            should_witness: bool,
        }

        let test_cases = vec![
            TestCase {
                name: "HostReady",
                command: GameCommand::HostReady,
                should_witness: true,
            },
            TestCase {
                name: "StartGame",
                command: GameCommand::StartGame,
                should_witness: false,
            },
            TestCase {
                name: "EndGame",
                command: GameCommand::EndGame,
                should_witness: false,
            },
            TestCase {
                name: "Buzz",
                command: GameCommand::Buzz,
                should_witness: false,
            },
            TestCase {
                name: "HostChoice",
                command: GameCommand::HostChoice {
                    category_index: 0,
                    question_index: 0,
                },
                should_witness: false,
            },
            TestCase {
                name: "HostChecked",
                command: GameCommand::HostChecked { correct: true },
                should_witness: false,
            },
            TestCase {
                name: "HostSkip",
                command: GameCommand::HostSkip,
                should_witness: false,
            },
            TestCase {
                name: "HostContinue",
                command: GameCommand::HostContinue,
                should_witness: false,
            },
            TestCase {
                name: "Heartbeat",
                command: GameCommand::Heartbeat {
                    hbid: 1,
                    t_dohb_recv: 0,
                },
                should_witness: false,
            },
            TestCase {
                name: "LatencyOfHeartbeat",
                command: GameCommand::LatencyOfHeartbeat { hbid: 1, t_lat: 0 },
                should_witness: false,
            },
        ];

        for tc in test_cases {
            assert_eq!(
                tc.command.should_witness(),
                tc.should_witness,
                "{}: expected should_witness={}, got {}",
                tc.name,
                tc.should_witness,
                tc.command.should_witness()
            );
        }
    }
}
