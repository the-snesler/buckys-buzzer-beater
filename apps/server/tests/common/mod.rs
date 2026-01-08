use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;

use futures::{SinkExt, StreamExt};
use madhacks2025::game::room::Room;
use madhacks2025::game::{Category, Question};
use tokio::sync::MutexGuard;
use tokio::{net::TcpStream, task::JoinHandle};
use tokio_tungstenite::tungstenite::Utf8Bytes;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream, connect_async};

use madhacks2025::api::messages::{GameCommand, GameEvent};
use madhacks2025::{AppState, build_app};

pub type WsStream = WebSocketStream<MaybeTlsStream<TcpStream>>;

/// Start test server on random port
/// Returns (server task, port number, shared app state)
pub async fn start_test_server() -> (JoinHandle<()>, u16, Arc<AppState>) {
    let state = Arc::new(AppState::new());
    let app = build_app(state.clone());

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("Failed to bind test server");

    let addr: SocketAddr = listener.local_addr().expect("Failed to get local addr");
    let port = addr.port();

    let server_handle =
        tokio::spawn(async move { axum::serve(listener, app).await.expect("Server failed") });

    tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

    (server_handle, port, state)
}

/// Connect a WebSocket client to a room
///
/// # Arguments
/// * `port` - Server port
/// * `room_code` Room code to join
/// * `query_params` - Query string
///
/// # Returns
/// WebSocket stream ready to send/receive messages
pub async fn connect_ws_client(port: u16, room_code: &str, query_params: &str) -> WsStream {
    let url = format!(
        "ws://127.0.0.1:{}/api/v1/rooms/{}/ws{}",
        port, room_code, query_params
    );

    let (ws_stream, _response) = connect_async(&url)
        .await
        .expect("Failed to connect WebSocket");

    ws_stream
}

/// Send a command and receive all response (with timeout)
///
/// # Arguments
/// * `ws` - WebSocket stream
/// * `cmd` - GameCommand to send
///
/// # Returns
/// Vec of all received events within timeout period (100ms)
pub async fn send_cmd_and_recv_all(ws: &mut WsStream, cmd: &GameCommand) -> Vec<GameEvent> {
    use tokio_tungstenite::tungstenite::Message;

    let json = serde_json::to_string(cmd).expect("Failed to serialize");
    ws.send(Message::Text(Utf8Bytes::from(json)))
        .await
        .expect("Failed to send message");

    recv_msgs(ws).await
}

/// Receive messages from WebSocket (with timeout)
///
/// Waits for messages until timeout (100ms) with no new messages.
/// Useful for receiving broadcast messages without sending.
///
/// # Arguments
/// * `ws` - WebSocket stream
///
/// # Returns
/// Vec of all received messages
pub async fn recv_msgs(ws: &mut WsStream) -> Vec<GameEvent> {
    use tokio_tungstenite::tungstenite::Message;

    let mut received = Vec::new();
    let timeout = tokio::time::Duration::from_millis(100);

    loop {
        match tokio::time::timeout(timeout, ws.next()).await {
            Ok(Some(Ok(Message::Text(text)))) => match serde_json::from_str::<GameEvent>(&text) {
                Ok(parsed) => received.push(parsed),
                Err(e) => panic!("Failed to parse GameEvent: {}. Text: {}", e, text),
            },
            Ok(Some(Ok(_))) => {
                // Ignore non-text messages
            }
            Ok(Some(Err(e))) => panic!("WebSocket error: {}", e),
            Ok(None) => break, // Connection closed
            Err(_) => break,   // Timeout - no more messages
        }
    }

    received
}

/// Create a room via HTTP API
///
/// # Arguments
/// * `port` - Server port
///
/// # Returns
/// Room code of created room
pub async fn create_room_http(port: u16) -> String {
    let url = format!("http://127.0.0.1:{}/api/v1/rooms/create", port);

    let client = reqwest::Client::new();
    let response = client
        .post(&url)
        .json(&serde_json::json!({}))
        .send()
        .await
        .expect("Failed to create room");

    let json: serde_json::Value = response.json().await.expect("Failed to parse response");

    json["room_code"]
        .as_str()
        .expect("No room_code in response")
        .to_string()
}

/// Add categories to an existing room
pub async fn add_room_categories(state: &AppState, room_code: &str) {
    let mut room_map = state.room_map.lock().await;
    let room = room_map
        .get_mut(room_code)
        .unwrap_or_else(|| panic!("Failed to get room with code: {}", room_code));

    let questions: Vec<Question> = (0..=2)
        .map(|i| Question {
            question: format!("Question {}", i + 1),
            answer: format!("Answer {}", i + 1),
            value: (i as u32 + 1) * 100,
            answered: false,
        })
        .collect();

    room.categories.insert(
        0,
        Category {
            questions,
            title: "Category 1".to_string(),
        },
    );
}

/// Add a player and return their websocket and ID
pub async fn add_player(port: u16, room_code: &str, name: &str) -> (WsStream, u32) {
    let mut player_ws = connect_ws_client(port, room_code, &format!("?playerName={}", name)).await;
    let msgs = recv_msgs(&mut player_ws).await;

    let player_id = msgs
        .iter()
        .find_map(|m| {
            if let GameEvent::NewPlayer { pid, .. } = m {
                Some(*pid)
            } else {
                None
            }
        })
        .expect("Should receive NewPlayer message");

    (player_ws, player_id)
}

pub async fn play_question(
    host_ws: &mut WsStream,
    player_ws: &mut WsStream,
    c_idx: usize,
    q_idx: usize,
    correct: bool,
) {
    send_cmd_and_recv_all(
        host_ws,
        &GameCommand::HostChoice {
            category_index: c_idx,
            question_index: q_idx,
        },
    )
    .await;
    let _ = recv_msgs(player_ws).await;

    // Host starts question
    send_cmd_and_recv_all(host_ws, &GameCommand::HostReady).await;
    let _ = recv_msgs(player_ws).await;

    // Player buzz
    send_cmd_and_recv_all(player_ws, &GameCommand::Buzz).await;
    let _ = recv_msgs(host_ws).await;

    // Host checks answer
    send_cmd_and_recv_all(host_ws, &GameCommand::HostChecked { correct }).await;
    let _ = recv_msgs(player_ws).await;
}

/// Get player score from room map
pub fn get_player_score(
    room_map: &MutexGuard<HashMap<String, Room>>,
    room_code: &str,
    player_id: u32,
) -> i32 {
    let room = room_map.get(room_code).expect("Could not find room");
    room.players
        .iter()
        .find(|p| p.player.pid == player_id)
        .expect("Could not find player")
        .player
        .score
}

/// Start game and consume initial messages
pub async fn start_game(host_ws: &mut WsStream, player_ws_list: &mut [&mut WsStream]) {
    send_cmd_and_recv_all(host_ws, &GameCommand::StartGame).await;

    for player_ws in player_ws_list {
        let _ = recv_msgs(player_ws).await;
    }
}
