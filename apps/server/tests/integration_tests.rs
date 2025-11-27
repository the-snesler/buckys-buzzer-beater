mod common;

use std::time::Duration;

use common::*;
use madhacks2025::{ws_msg::WsMsg, GameState};
use tokio::time::sleep;

mod smoke_tests {
    use super::*;

    #[tokio::test]
    async fn test_server_starts_and_health_check() {
        let (_server, port, _state) = start_test_server().await;

        let url = format!("http://127.0.0.1:{}/health", port);
        let response = reqwest::get(&url).await.expect("Health check failed");

        assert_eq!(response.status(), 200);
        let body = response.text().await.expect("Failed to read body");
        assert_eq!(body, "Server is up");
    }

    #[tokio::test]
    async fn test_create_room_via_http() {
        let (_server, port, state) = start_test_server().await;

        let room_code = create_room_http(port).await;

        let room_map = state.room_map.lock().await;
        assert!(
            room_map.contains_key(&room_code),
            "Room should exist in state"
        );

        assert_eq!(room_code.len(), 6);
        assert!(room_code.chars().all(|c| c.is_ascii_uppercase()));
    }

    #[tokio::test]
    async fn test_host_connects_via_websocket() {
        let (_server, port, state) = start_test_server().await;

        let room_code = create_room_http(port).await;

        let host_token = {
            let room_map = state.room_map.lock().await;
            room_map.get(&room_code).unwrap().host_token.clone()
        };

        let mut host_ws =
            connect_ws_client(port, &room_code, &format!("?token={}", host_token)).await;

        let messages = recv_msgs(&mut host_ws).await;

        assert!(!messages.is_empty(), "Host should receive initial messages");

        println!("Host received {} messages", messages.len());
        for msg in &messages {
            println!("  {:?}", msg);
        }
    }
}

#[tokio::test]
async fn test_player_joins_room() {
    let (_server, port, state) = start_test_server().await;
    let room_code = create_room_http(port).await;

    let host_token = {
        let room_map = state.room_map.lock().await;
        room_map.get(&room_code).unwrap().host_token.clone()
    };
    let mut host_ws = connect_ws_client(port, &room_code, &format!("?token={}", host_token)).await;
    let _initial_msgs = recv_msgs(&mut host_ws).await;

    let mut player_ws = connect_ws_client(port, &room_code, "?playerName=AJ").await;

    let player_msgs = recv_msgs(&mut player_ws).await;

    let new_player_msg = player_msgs
        .iter()
        .find(|m| matches!(m, WsMsg::NewPlayer { .. }));
    assert!(
        new_player_msg.is_some(),
        "Player should receive NewPlayer message"
    );

    if let Some(WsMsg::NewPlayer { pid, token, .. }) = new_player_msg {
        assert!(pid > &0, "Player should have valid ID");
        assert!(!token.is_empty(), "Player should have token");
    }

    let host_msgs = recv_msgs(&mut host_ws).await;

    let player_list_msg = host_msgs
        .iter()
        .find(|m| matches!(m, WsMsg::PlayerList { .. }));

    if let Some(WsMsg::PlayerList(players)) = player_list_msg {
        assert_eq!(players.len(), 1, "Should have 1 player");
        assert_eq!(players[0].name, "AJ");
    }

    let room_map = state.room_map.lock().await;
    let room = room_map.get(&room_code).unwrap();
    assert_eq!(room.players.len(), 1, "Room should have 1 player in state");
}

#[tokio::test]
async fn test_multiple_players_join() {
    let (_server, port, state) = start_test_server().await;
    let room_code = create_room_http(port).await;

    let host_token = {
        let room_map = state.room_map.lock().await;
        room_map.get(&room_code).unwrap().host_token.clone()
    };
    let mut host_ws = connect_ws_client(port, &room_code, &format!("?token={}", host_token)).await;
    let _initial = recv_msgs(&mut host_ws).await;

    let mut alice_ws = connect_ws_client(port, &room_code, "?playerName=Alice").await;
    let _alice_msgs = recv_msgs(&mut alice_ws).await;
    let _host_update1 = recv_msgs(&mut host_ws).await;

    let mut bob_ws = connect_ws_client(port, &room_code, "?playerName=Bob").await;
    let _bob_msgs = recv_msgs(&mut bob_ws).await;
    let _host_update2 = recv_msgs(&mut host_ws).await;

    let mut charlie_ws = connect_ws_client(port, &room_code, "?playerName=Charlie").await;
    let _charlie_msgs = recv_msgs(&mut charlie_ws).await;
    let host_final = recv_msgs(&mut host_ws).await;

    let player_list = host_final
        .iter()
        .find(|m| matches!(m, WsMsg::PlayerList { .. }));
    if let Some(WsMsg::PlayerList(players)) = player_list {
        assert_eq!(players.len(), 3, "Should have 3 players");
        let names: Vec<&str> = players.iter().map(|p| p.name.as_str()).collect();
        assert!(names.contains(&"Alice"));
        assert!(names.contains(&"Bob"));
        assert!(names.contains(&"Charlie"));
    } else {
        panic!("Should receive PlayerList");
    }

    let room_map = state.room_map.lock().await;
    let room = room_map.get(&room_code).unwrap();
    assert_eq!(room.players.len(), 3);
}

#[tokio::test]
async fn test_game_flow_start_to_buzz() {
    let (_server, port, state) = start_test_server().await;
    let room_code = create_room_http(port).await;

    let host_token = {
        let room_map = state.room_map.lock().await;
        room_map.get(&room_code).unwrap().host_token.clone()
    };
    let mut host_ws = connect_ws_client(port, &room_code, &format!("?token={}", host_token)).await;
    let _initial = recv_msgs(&mut host_ws).await;

    let mut player_ws = connect_ws_client(port, &room_code, "?playerName=AJ").await;
    let player_init = recv_msgs(&mut player_ws).await;
    let _host_update = recv_msgs(&mut host_ws).await;

    let player_id = if let Some(WsMsg::NewPlayer { pid, .. }) = player_init
        .iter()
        .find(|m| matches!(m, WsMsg::NewPlayer { .. }))
    {
        *pid
    } else {
        panic!("Player should receive NewPlayer message");
    };

    let start_msgs = send_msg_and_recv_all(&mut host_ws, &WsMsg::StartGame {}).await;
    println!("After StartGame, host got: {:?}", start_msgs);

    let game_state = start_msgs
        .iter()
        .find(|m| matches!(m, WsMsg::GameState { .. }));
    assert!(
        game_state.is_some(),
        "Host should receive GameState after start"
    );

    let player_msgs = recv_msgs(&mut player_ws).await;
    println!("Player got: {:?}", player_msgs);

    let ready_msgs = send_msg_and_recv_all(&mut host_ws, &WsMsg::HostReady {}).await;
    println!("After HostReady, host got: {:?}", ready_msgs);

    let player_update = recv_msgs(&mut player_ws).await;
    println!("Player got after HostReady: {:?}", player_update);

    let buzz_state = player_update.iter().find(|m| {
        if let WsMsg::GameState { state, .. } = m {
            matches!(state, GameState::WaitingForBuzz)
        } else {
            false
        }
    });
    assert!(
        buzz_state.is_some(),
        "Player should get WaitingForBuzz state"
    );

    let buzz_msgs = send_msg_and_recv_all(&mut player_ws, &WsMsg::Buzz {}).await;
    println!("After Buzz, player got: {:?}", buzz_msgs);

    let host_buzz = recv_msgs(&mut host_ws).await;
    println!("Host got after buzz: {:?}", host_buzz);

    let buzz_notification = host_buzz.iter().find(|m| matches!(m, WsMsg::Buzzed { .. }));
    assert!(
        buzz_notification.is_some(),
        "Host should receive PlayerBuzzed"
    );

    if let Some(WsMsg::Buzzed { pid, .. }) = buzz_notification {
        assert_eq!(*pid, player_id, "Correct player buzzed");
    }

    let room_map = state.room_map.lock().await;
    let room = room_map.get(&room_code).unwrap();
    assert!(matches!(room.state, GameState::Answer));
}

#[tokio::test]
async fn test_player_reconnect() {
    let (_server, port, state) = start_test_server().await;
    let room_code = create_room_http(port).await;

    let host_token = {
        let room_map = state.room_map.lock().await;
        room_map.get(&room_code).unwrap().host_token.clone()
    };
    let mut _host_ws = connect_ws_client(port, &room_code, &format!("?token={}", host_token)).await;

    let mut player_ws = connect_ws_client(port, &room_code, "?playerName=AJ").await;
    let player_msgs = recv_msgs(&mut player_ws).await;

    let (player_id, player_token) = if let Some(WsMsg::NewPlayer { pid, token, .. }) = player_msgs
        .iter()
        .find(|m| matches!(m, WsMsg::NewPlayer { .. }))
    {
        (*pid, token.clone())
    } else {
        panic!("Should get NewPlayer message");
    };

    {
        let room_map = state.room_map.lock().await;
        let room = room_map.get(&room_code).unwrap();
        assert_eq!(
            room.players.len(),
            1,
            "Should have 1 player before disconnect"
        );
    }

    drop(player_ws);
    sleep(Duration::from_millis(100)).await;

    {
        let room_map = state.room_map.lock().await;
        let room = room_map.get(&room_code).unwrap();
        assert_eq!(
            room.players.len(),
            1,
            "Should have 1 player after disconnect"
        );
    }

    let mut player_reconnect = connect_ws_client(
        port,
        &room_code,
        &format!("?token={}&playerID={}", player_token, player_id),
    )
    .await;

    let reconnect_msgs = recv_msgs(&mut player_reconnect).await;
    println!("Reconnect messages: {:?}", reconnect_msgs);

    let got_new_player = reconnect_msgs
        .iter()
        .any(|m| matches!(m, WsMsg::NewPlayer { .. }));
    assert!(
        !got_new_player,
        "Should not get NewPlayer on reconnect"
    );

    let has_state = reconnect_msgs
        .iter()
        .any(|m| matches!(m, WsMsg::PlayerState { .. } | WsMsg::GameState { .. }));
    assert!(has_state, "Should receive state on reconnect");

    if let Some(WsMsg::PlayerState { pid, .. }) = reconnect_msgs
        .iter()
        .find(|m| matches!(m, WsMsg::PlayerState { .. }))
    {
        let room_map = state.room_map.lock().await;
        let room = room_map.get(&room_code).unwrap();
        let player = room
            .players
            .iter()
            .find(|p| &p.player.pid == pid)
            .map(|p| &p.player)
            .unwrap();
        assert_eq!(
            player.pid, player_id,
            "Reconnected player should have same ID"
        );
        assert_eq!(
            player.name, "AJ",
            "Reconnected player should have same name"
        );
    }

    {
        let room_map = state.room_map.lock().await;
        let room = room_map.get(&room_code).unwrap();
        assert_eq!(
            room.players.len(),
            1,
            "Should still have 1 player after reconnect"
        );
    }
}
