use std::net::SocketAddr;
use std::sync::Arc;

use futures::{SinkExt, StreamExt};
use tokio::{net::TcpStream, task::JoinHandle};
use tokio_tungstenite::tungstenite::Utf8Bytes;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream, connect_async};

use madhacks2025::ws_msg::WsMsg;
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

/// Send a message and receive all response (with timeout)
///
/// # Arguments
/// * `ws` - WebSocket stream
/// * `msg` - Message to send
///
/// # Returns
/// Vec of all received messages within timeout period (100ms)
pub async fn send_msg_and_recv_all(ws: &mut WsStream, msg: &WsMsg) -> Vec<WsMsg> {
    use tokio_tungstenite::tungstenite::Message;

    let json = serde_json::to_string(msg).expect("Failed to serialize");
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
pub async fn recv_msgs(ws: &mut WsStream) -> Vec<WsMsg> {
    use tokio_tungstenite::tungstenite::Message;

    let mut received = Vec::new();
    let timeout = tokio::time::Duration::from_millis(100);

    loop {
        match tokio::time::timeout(timeout, ws.next()).await {
            Ok(Some(Ok(Message::Text(text)))) => match serde_json::from_str::<WsMsg>(&text) {
                Ok(parsed) => received.push(parsed),
                Err(e) => panic!("Failed to parse WsMsg: {}. Text: {}", e, text),
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
