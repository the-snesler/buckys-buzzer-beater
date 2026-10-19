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
    Buzz {
        /// Milliseconds between the client's buzzer enabling and the player pressing it.
        /// Measured client-side so network latency doesn't decide who buzzed first.
        #[serde(rename = "reactionMs")]
        reaction_ms: u32,
    },
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
    /// Ends the current round early, moving on to the next round or ending the game
    HostNextRound {
        /// The round the host is ending. Ignored unless it is the current round, so a
        /// repeated click can't skip the next round too.
        round: usize,
    },
    /// Makes `pid` the player who chooses the next clue
    HostSetPicker {
        pid: PlayerId,
    },
    /// A player's wager on the Daily Double they chose
    SubmitWager {
        amount: u32,
    },
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
    /// let cmd = GameCommand::Buzz { reaction_ms: 120 };
    /// assert!(!cmd.should_witness(), "Buzz is handled directly");
    /// ```
    pub fn should_witness(&self) -> bool {
        matches!(self, Self::HostReady)
    }

    /// Whether only the host may send this command. Every other command may only be
    /// sent by a player.
    ///
    /// # Examples
    ///
    /// ```
    /// use madhacks2025::api::messages::{GameCommand, Sender};
    ///
    /// let cmd = GameCommand::HostChecked { correct: true };
    /// assert!(cmd.is_host_command());
    /// assert!(cmd.allowed_from(Sender::Host));
    /// assert!(!cmd.allowed_from(Sender::Player(1)));
    /// ```
    pub fn is_host_command(&self) -> bool {
        match self {
            Self::StartGame
            | Self::EndGame
            | Self::HostReady
            | Self::HostChoice { .. }
            | Self::HostChecked { .. }
            | Self::HostSkip
            | Self::HostContinue
            | Self::HostNextRound { .. }
            | Self::HostSetPicker { .. } => true,
            Self::Buzz { .. }
            | Self::SubmitWager { .. }
            | Self::Heartbeat { .. }
            | Self::LatencyOfHeartbeat { .. } => false,
        }
    }

    /// The command's `type` tag, for logging without the client-controlled payload
    pub fn name(&self) -> &'static str {
        match self {
            Self::StartGame => "StartGame",
            Self::EndGame => "EndGame",
            Self::Buzz { .. } => "Buzz",
            Self::HostReady => "HostReady",
            Self::HostChoice { .. } => "HostChoice",
            Self::HostChecked { .. } => "HostChecked",
            Self::HostSkip => "HostSkip",
            Self::HostContinue => "HostContinue",
            Self::HostNextRound { .. } => "HostNextRound",
            Self::HostSetPicker { .. } => "HostSetPicker",
            Self::SubmitWager { .. } => "SubmitWager",
            Self::Heartbeat { .. } => "Heartbeat",
            Self::LatencyOfHeartbeat { .. } => "LatencyOfHeartbeat",
        }
    }

    /// Whether `sender`'s role is allowed to send this command
    pub fn allowed_from(&self, sender: Sender) -> bool {
        match sender {
            Sender::Host => self.is_host_command(),
            Sender::Player(_) => !self.is_host_command(),
        }
    }
}

/// Who sent a [`GameCommand`], as established by the WebSocket handshake
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Sender {
    Host,
    Player(PlayerId),
}

/// The wagers a player may make, inclusive
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub struct WagerRange {
    pub min: u32,
    pub max: u32,
}

impl WagerRange {
    pub fn contains(&self, amount: u32) -> bool {
        (self.min..=self.max).contains(&amount)
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
///     wager_range: None,
///     wager: None,
///     prompt: None,
/// };
/// let json = serde_json::to_string(&event).unwrap();
/// assert!(json.contains("PlayerState"));
/// assert!(json.contains("\"type\":\"PlayerState\""));
/// ```
#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(tag = "type")]
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
    PlayerList {
        players: Vec<Player>,
    },
    NewPlayer {
        pid: PlayerId,
        token: PlayerToken,
    },
    /// Sent to the host only, since it contains every clue's answer
    GameState {
        state: GameState,
        /// The current round's board
        categories: Vec<Category>,
        /// Zero-based index of the current round
        round: usize,
        #[serde(rename = "roundCount")]
        round_count: usize,
        players: Vec<Player>,
        #[serde(rename = "currentQuestion")]
        current_question: Option<(usize, usize)>,
        #[serde(rename = "currentBuzzer")]
        current_buzzer: Option<PlayerId>,
        /// The player whose turn it is to choose the next clue, if the host has chosen one
        picker: Option<PlayerId>,
        /// The picker's wager on the Daily Double being played, once they've made it
        #[serde(rename = "ddWager")]
        dd_wager: Option<u32>,
        winner: Option<PlayerId>,
    },
    /// The part of the game state every player may see. Players receive this instead of
    /// [`GameEvent::GameState`], which is only sent to the host.
    PublicState {
        state: GameState,
        /// Zero-based index of the current round
        round: usize,
        /// The player whose turn it is to choose the next clue, if known
        picker: Option<PlayerId>,
    },
    /// Sent to one player only
    PlayerState {
        pid: PlayerId,
        buzzed: bool,
        score: i32,
        #[serde(rename = "canBuzz")]
        can_buzz: bool,
        /// Set while this player must make a wager, which is then accepted in this range
        #[serde(rename = "wagerRange")]
        wager_range: Option<WagerRange>,
        /// This player's wager on the clue being played
        wager: Option<u32>,
        /// What this player is wagering or answering on, such as the Daily Double's category
        prompt: Option<String>,
    },
    /// Sent to one player when a command of theirs was refused, such as a wager out of
    /// range, so they can be told why
    Rejected {
        reason: String,
    },
    PlayerBuzzed {
        pid: PlayerId,
        name: String,
    },
}

#[cfg(test)]
mod test {
    use crate::{
        Player,
        api::messages::{GameCommand, GameEvent, Sender, WagerRange},
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
                json: r#"{"type": "Buzz", "reactionMs": 120}"#,
                expected: GameCommand::Buzz { reaction_ms: 120 },
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
                name: "HostNextRound",
                json: r#"{"type": "HostNextRound", "round": 1}"#,
                expected: GameCommand::HostNextRound { round: 1 },
            },
            TestCase {
                name: "HostSetPicker",
                json: r#"{"type": "HostSetPicker", "pid": 2}"#,
                expected: GameCommand::HostSetPicker { pid: 2 },
            },
            TestCase {
                name: "SubmitWager",
                json: r#"{"type": "SubmitWager", "amount": 1500}"#,
                expected: GameCommand::SubmitWager { amount: 1500 },
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
            let cmd =
                result.unwrap_or_else(|e| panic!("Failed to deserialize {}: {:?}", tc.name, e));
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
                name: "HostNextRound without a round",
                json: r#"{"type": "HostNextRound"}"#,
            },
            TestCase {
                name: "HostSetPicker without a pid",
                json: r#"{"type": "HostSetPicker"}"#,
            },
            TestCase {
                name: "SubmitWager without an amount",
                json: r#"{"type": "SubmitWager"}"#,
            },
            TestCase {
                name: "SubmitWager with a negative amount",
                json: r#"{"type": "SubmitWager", "amount": -5}"#,
            },
            TestCase {
                name: "SubmitWager with a fractional amount",
                json: r#"{"type": "SubmitWager", "amount": 5.5}"#,
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
                event: GameEvent::PlayerList { players: vec![] },
                expected_substrings: vec![r#""type":"PlayerList""#, r#""players":[]"#],
            },
            TestCase {
                name: "NewPlayer",
                event: GameEvent::NewPlayer {
                    pid: 1,
                    token: PlayerToken::generate(),
                },
                expected_substrings: vec![r#""type":"NewPlayer""#, r#""pid":1"#, r#""token""#],
            },
            TestCase {
                name: "GameState",
                event: GameEvent::GameState {
                    state: GameState::Start,
                    categories: vec![],
                    round: 1,
                    round_count: 2,
                    players: vec![],
                    current_question: None,
                    current_buzzer: None,
                    picker: Some(3),
                    dd_wager: Some(1200),
                    winner: None,
                },
                expected_substrings: vec![
                    r#""type":"GameState""#,
                    r#""state""#,
                    r#""categories""#,
                    r#""round":1"#,
                    r#""roundCount":2"#,
                    r#""picker":3"#,
                    r#""ddWager":1200"#,
                ],
            },
            TestCase {
                name: "PlayerState with camelCase",
                event: GameEvent::PlayerState {
                    pid: 1,
                    buzzed: true,
                    score: 500,
                    can_buzz: false,
                    wager_range: None,
                    wager: None,
                    prompt: None,
                },
                expected_substrings: vec![
                    r#""type":"PlayerState""#,
                    r#""pid":1"#,
                    r#""buzzed":true"#,
                    r#""score":500"#,
                    r#""canBuzz":false"#,
                    r#""wagerRange":null"#,
                    r#""wager":null"#,
                    r#""prompt":null"#,
                ],
            },
            TestCase {
                name: "PlayerState making a wager",
                event: GameEvent::PlayerState {
                    pid: 1,
                    buzzed: false,
                    score: 500,
                    can_buzz: false,
                    wager_range: Some(WagerRange { min: 5, max: 1000 }),
                    wager: Some(600),
                    prompt: Some("Potent Potables".to_string()),
                },
                expected_substrings: vec![
                    r#""wagerRange":{"min":5,"max":1000}"#,
                    r#""wager":600"#,
                    r#""prompt":"Potent Potables""#,
                ],
            },
            TestCase {
                name: "Rejected",
                event: GameEvent::Rejected {
                    reason: "Too much".to_string(),
                },
                expected_substrings: vec![r#""type":"Rejected""#, r#""reason":"Too much""#],
            },
            TestCase {
                name: "PublicState",
                event: GameEvent::PublicState {
                    state: GameState::WaitingForBuzz,
                    round: 1,
                    picker: Some(2),
                },
                expected_substrings: vec![
                    r#""type":"PublicState""#,
                    r#""state":"waitingForBuzz""#,
                    r#""round":1"#,
                    r#""picker":2"#,
                ],
            },
            TestCase {
                name: "PlayerList omits tokens",
                event: GameEvent::PlayerList {
                    players: vec![Player::new(
                        1,
                        "AJ".to_string(),
                        0,
                        false,
                        PlayerToken::generate(),
                    )],
                },
                expected_substrings: vec![r#""type":"PlayerList""#, r#""name":"AJ""#],
            },
            TestCase {
                name: "PlayerBuzzed",
                event: GameEvent::PlayerBuzzed {
                    pid: 2,
                    name: "PlayerName".to_string(),
                },
                expected_substrings: vec![r#""type":"PlayerBuzzed""#, r#""pid":2"#, "PlayerName"],
            },
            TestCase {
                name: "DoHeartbeat",
                event: GameEvent::DoHeartbeat {
                    hbid: 123,
                    t_sent: 1609459200000,
                },
                expected_substrings: vec![
                    r#""type":"DoHeartbeat""#,
                    r#""hbid":123"#,
                    "1609459200000",
                ],
            },
            TestCase {
                name: "GotHeartbeat",
                event: GameEvent::GotHeartbeat { hbid: 456 },
                expected_substrings: vec![r#""type":"GotHeartbeat""#, r#""hbid":456"#],
            },
            TestCase {
                name: "Witness nested",
                event: GameEvent::Witness {
                    msg: Box::new(GameEvent::PlayerBuzzed {
                        pid: 1,
                        name: "Bob".to_string(),
                    }),
                },
                expected_substrings: vec![r#""type":"Witness""#, r#""msg""#, "PlayerBuzzed"],
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
    fn test_game_event_round_trip() {
        let events = vec![
            GameEvent::PublicState {
                state: GameState::Selection,
                round: 0,
                picker: None,
            },
            GameEvent::PublicState {
                state: GameState::AnswerReveal,
                round: 1,
                picker: Some(3),
            },
            GameEvent::Witness {
                msg: Box::new(GameEvent::PublicState {
                    state: GameState::WaitingForBuzz,
                    round: 0,
                    picker: None,
                }),
            },
            GameEvent::PlayerState {
                pid: 1,
                buzzed: false,
                score: -200,
                can_buzz: true,
                wager_range: None,
                wager: None,
                prompt: None,
            },
            GameEvent::PlayerState {
                pid: 2,
                buzzed: false,
                score: 300,
                can_buzz: false,
                wager_range: Some(WagerRange { min: 5, max: 1000 }),
                wager: Some(5),
                prompt: Some("Rivers".to_string()),
            },
            GameEvent::Rejected {
                reason: "Wager at least $5".to_string(),
            },
        ];

        for event in events {
            let json = serde_json::to_string(&event)
                .unwrap_or_else(|e| panic!("Failed to serialize {:?}: {}", event, e));
            let parsed: GameEvent = serde_json::from_str(&json)
                .unwrap_or_else(|e| panic!("Failed to deserialize {}: {}", json, e));
            assert_eq!(format!("{:?}", parsed), format!("{:?}", event));
        }
    }

    #[test]
    fn test_player_token_not_serialized() {
        let token = PlayerToken::generate();
        let player = Player::new(1, "AJ".to_string(), 0, false, token.clone());

        let json = serde_json::to_string(&player).expect("Failed to serialize player");
        assert!(!json.contains("token"), "Token key in {}", json);
        assert!(
            !json.contains(&token.to_string()),
            "Token value in {}",
            json
        );

        let parsed: Player = serde_json::from_str(&json).expect("Failed to deserialize player");
        assert_ne!(parsed.token, token);
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
                command: GameCommand::Buzz { reaction_ms: 120 },
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
                name: "HostNextRound",
                command: GameCommand::HostNextRound { round: 0 },
                should_witness: false,
            },
            TestCase {
                name: "HostSetPicker",
                command: GameCommand::HostSetPicker { pid: 1 },
                should_witness: false,
            },
            TestCase {
                name: "SubmitWager",
                command: GameCommand::SubmitWager { amount: 5 },
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

    #[test]
    fn test_command_roles() {
        struct TestCase {
            name: &'static str,
            command: GameCommand,
            host_command: bool,
        }

        let test_cases = vec![
            TestCase {
                name: "StartGame",
                command: GameCommand::StartGame,
                host_command: true,
            },
            TestCase {
                name: "EndGame",
                command: GameCommand::EndGame,
                host_command: true,
            },
            TestCase {
                name: "HostReady",
                command: GameCommand::HostReady,
                host_command: true,
            },
            TestCase {
                name: "HostChoice",
                command: GameCommand::HostChoice {
                    category_index: 0,
                    question_index: 0,
                },
                host_command: true,
            },
            TestCase {
                name: "HostChecked",
                command: GameCommand::HostChecked { correct: true },
                host_command: true,
            },
            TestCase {
                name: "HostSkip",
                command: GameCommand::HostSkip,
                host_command: true,
            },
            TestCase {
                name: "HostContinue",
                command: GameCommand::HostContinue,
                host_command: true,
            },
            TestCase {
                name: "HostNextRound",
                command: GameCommand::HostNextRound { round: 0 },
                host_command: true,
            },
            TestCase {
                name: "HostSetPicker",
                command: GameCommand::HostSetPicker { pid: 1 },
                host_command: true,
            },
            TestCase {
                name: "Buzz",
                command: GameCommand::Buzz { reaction_ms: 120 },
                host_command: false,
            },
            TestCase {
                name: "SubmitWager",
                command: GameCommand::SubmitWager { amount: 5 },
                host_command: false,
            },
            TestCase {
                name: "Heartbeat",
                command: GameCommand::Heartbeat {
                    hbid: 1,
                    t_dohb_recv: 0,
                },
                host_command: false,
            },
            TestCase {
                name: "LatencyOfHeartbeat",
                command: GameCommand::LatencyOfHeartbeat { hbid: 1, t_lat: 0 },
                host_command: false,
            },
        ];

        for tc in test_cases {
            let json = serde_json::to_value(&tc.command).expect("Failed to serialize");
            assert_eq!(json["type"], tc.command.name(), "{}: wrong name", tc.name);
            assert_eq!(
                tc.command.is_host_command(),
                tc.host_command,
                "{}: wrong role",
                tc.name
            );
            assert_eq!(
                tc.command.allowed_from(Sender::Host),
                tc.host_command,
                "{}: allowed_from(Host)",
                tc.name
            );
            assert_eq!(
                tc.command.allowed_from(Sender::Player(1)),
                !tc.host_command,
                "{}: allowed_from(Player)",
                tc.name
            );
        }
    }
}
