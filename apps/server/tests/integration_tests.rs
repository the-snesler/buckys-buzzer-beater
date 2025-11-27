mod common;

use std::time::Duration;

use tokio::time::sleep;

use common::*;
use madhacks2025::{
    GameState, PlayerEntry,
    game::{Category, Question},
    ws_msg::WsMsg,
};

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
    assert!(!got_new_player, "Should not get NewPlayer on reconnect");

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

#[tokio::test]
async fn test_correct_answer_gives_points() {
    let (_server, port, state) = start_test_server().await;
    let room_code = create_room_http(port).await;

    {
        let mut room_map = state.room_map.lock().await;
        let room = room_map.get_mut(&room_code).unwrap();
        room.categories.insert(
            0,
            Category {
                title: "Category".to_string(),
                questions: vec![Question {
                    question: "q".to_string(),
                    answer: "q".to_string(),
                    value: 200,
                    answered: false,
                }],
            },
        );
        room.categories.insert(
            1,
            Category {
                title: "Category 2".to_string(),
                questions: vec![Question {
                    question: "qq".to_string(),
                    answer: "qq".to_string(),
                    value: 200,
                    answered: false,
                }],
            },
        );
    }

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
        panic!("Should get NewPlayer");
    };

    send_msg_and_recv_all(&mut host_ws, &WsMsg::StartGame {}).await;
    let _player_start = recv_msgs(&mut player_ws).await;

    send_msg_and_recv_all(
        &mut host_ws,
        &WsMsg::HostChoice {
            category_index: 0,
            question_index: 0,
        },
    )
    .await;
    let _player_choice = recv_msgs(&mut player_ws).await;

    send_msg_and_recv_all(&mut host_ws, &WsMsg::HostReady {}).await;
    let _player_ready = recv_msgs(&mut player_ws).await;

    send_msg_and_recv_all(&mut player_ws, &WsMsg::Buzz {}).await;
    let _host_buzz = recv_msgs(&mut host_ws).await;

    let score_before = {
        let room_map = state.room_map.lock().await;
        let room = room_map.get(&room_code).unwrap();
        room.players
            .iter()
            .find(|p| p.player.pid == player_id)
            .unwrap()
            .player
            .score
    };
    assert_eq!(score_before, 0);

    let answer_msgs =
        send_msg_and_recv_all(&mut host_ws, &WsMsg::HostChecked { correct: true }).await;
    println!("Host got after correct answer: {:?}", answer_msgs);

    let player_answer = recv_msgs(&mut player_ws).await;
    println!("Player got after correct answer: {:?}", player_answer);

    let score_after = {
        let room_map = state.room_map.lock().await;
        let room = room_map.get(&room_code).unwrap();
        room.players
            .iter()
            .find(|p| p.player.pid == player_id)
            .unwrap()
            .player
            .score
    };
    assert!(
        score_after > score_before,
        "Score should increase after correct answer"
    );

    let room_map = state.room_map.lock().await;
    let room = room_map.get(&room_code).unwrap();
    assert!(matches!(room.state, GameState::Selection));
}

#[tokio::test]
async fn test_incorrect_answer_deducts_points() {
    let (_server, port, state) = start_test_server().await;
    let room_code = create_room_http(port).await;

    {
        let mut room_map = state.room_map.lock().await;
        let room = room_map.get_mut(&room_code).unwrap();
        room.categories.insert(
            0,
            Category {
                title: "Category".to_string(),
                questions: vec![Question {
                    question: "q".to_string(),
                    answer: "q".to_string(),
                    value: 200,
                    answered: false,
                }],
            },
        );
        room.categories.insert(
            1,
            Category {
                title: "Category 2".to_string(),
                questions: vec![Question {
                    question: "qq".to_string(),
                    answer: "qq".to_string(),
                    value: 200,
                    answered: false,
                }],
            },
        );
    }

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
        panic!("Should get NewPlayer");
    };

    send_msg_and_recv_all(&mut host_ws, &WsMsg::StartGame {}).await;
    let _player_start = recv_msgs(&mut player_ws).await;

    send_msg_and_recv_all(
        &mut host_ws,
        &WsMsg::HostChoice {
            category_index: 0,
            question_index: 0,
        },
    )
    .await;
    let _player_choice = recv_msgs(&mut player_ws).await;

    send_msg_and_recv_all(&mut host_ws, &WsMsg::HostReady {}).await;
    let _player_ready = recv_msgs(&mut player_ws).await;

    send_msg_and_recv_all(&mut player_ws, &WsMsg::Buzz {}).await;
    let _host_buzz = recv_msgs(&mut host_ws).await;

    let score_before = {
        let room_map = state.room_map.lock().await;
        let room = room_map.get(&room_code).unwrap();
        room.players
            .iter()
            .find(|p| p.player.pid == player_id)
            .unwrap()
            .player
            .score
    };
    assert_eq!(score_before, 0);

    let answer_msgs =
        send_msg_and_recv_all(&mut host_ws, &WsMsg::HostChecked { correct: false }).await;
    println!("Host got after correct answer: {:?}", answer_msgs);

    let player_answer = recv_msgs(&mut player_ws).await;
    println!("Player got after correct answer: {:?}", player_answer);

    let score_after = {
        let room_map = state.room_map.lock().await;
        let room = room_map.get(&room_code).unwrap();
        room.players
            .iter()
            .find(|p| p.player.pid == player_id)
            .unwrap()
            .player
            .score
    };
    assert!(
        score_after < score_before,
        "Score should decrease after incorrect answer"
    );

    let room_map = state.room_map.lock().await;
    let room = room_map.get(&room_code).unwrap();
    assert!(matches!(
        room.state,
        madhacks2025::game::GameState::Selection
    ));
}

#[tokio::test]
async fn test_host_reconnect() {
    let (_server, port, state) = start_test_server().await;
    let room_code = create_room_http(port).await;

    {
        let mut room_map = state.room_map.lock().await;
        let room = room_map.get_mut(&room_code).unwrap();
        room.categories.insert(
            0,
            Category {
                title: "Category".to_string(),
                questions: vec![Question {
                    question: "q".to_string(),
                    answer: "q".to_string(),
                    value: 200,
                    answered: false,
                }],
            },
        );
        room.categories.insert(
            1,
            Category {
                title: "Category 2".to_string(),
                questions: vec![Question {
                    question: "qq".to_string(),
                    answer: "qq".to_string(),
                    value: 200,
                    answered: false,
                }],
            },
        );
    }

    let host_token = {
        let room_map = state.room_map.lock().await;
        room_map.get(&room_code).unwrap().host_token.clone()
    };

    let mut host_ws = connect_ws_client(port, &room_code, &format!("?token={}", host_token)).await;
    let _initial = recv_msgs(&mut host_ws).await;

    let mut player_ws = connect_ws_client(port, &room_code, "?playerName=AJ").await;
    let _player_init = recv_msgs(&mut player_ws).await;
    let _host_update = recv_msgs(&mut host_ws).await;

    send_msg_and_recv_all(&mut host_ws, &WsMsg::StartGame {}).await;
    let _player_start = recv_msgs(&mut player_ws).await;

    send_msg_and_recv_all(
        &mut host_ws,
        &WsMsg::HostChoice {
            category_index: 0,
            question_index: 0,
        },
    )
    .await;
    let _player_choice = recv_msgs(&mut player_ws).await;

    let state_before = {
        let room_map = state.room_map.lock().await;
        let room = room_map.get(&room_code).unwrap();
        room.state.clone()
    };
    assert!(matches!(state_before, GameState::QuestionReading));

    // Host Disconnect
    drop(host_ws);
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    let mut host_reconnect =
        connect_ws_client(port, &room_code, &format!("?token={}", host_token)).await;

    let reconnect_msgs = recv_msgs(&mut host_reconnect).await;
    println!("Host reconnect messages: {:?}", reconnect_msgs);

    let game_state_msg = reconnect_msgs
        .iter()
        .find(|m| matches!(m, WsMsg::GameState { .. }));
    assert!(
        game_state_msg.is_some(),
        "Host should receive GameState on reconnect"
    );

    if let Some(WsMsg::GameState {
        state,
        players,
        current_question,
        ..
    }) = game_state_msg
    {
        assert!(matches!(state, GameState::QuestionReading));
        assert_eq!(players.len(), 1, "Should still have 1 player");
        assert_eq!(
            current_question,
            &Some((0, 0)),
            "Should have current question set"
        );
    }

    send_msg_and_recv_all(&mut host_reconnect, &WsMsg::HostReady {}).await;
    let player_ready = recv_msgs(&mut player_ws).await;

    let waiting_state = player_ready.iter().any(|m| {
        matches!(
            m,
            WsMsg::GameState {
                state: GameState::WaitingForBuzz,
                ..
            }
        )
    });
    assert!(waiting_state, "Game should continue after host reconnects");
}

#[tokio::test]
async fn test_full_game() {
    let (_server, port, state) = start_test_server().await;
    let room_code = create_room_http(port).await;

    {
        let mut room_map = state.room_map.lock().await;
        let room = room_map.get_mut(&room_code).unwrap();
        room.categories.insert(
            0,
            Category {
                title: "Test Category".to_string(),
                questions: vec![
                    Question {
                        question: "Question 1".to_string(),
                        answer: "Answer 1".to_string(),
                        value: 100,
                        answered: false,
                    },
                    Question {
                        question: "Question 2".to_string(),
                        answer: "Answer 2".to_string(),
                        value: 200,
                        answered: false,
                    },
                ],
            },
        );
    }

    let host_token = {
        let room_map = state.room_map.lock().await;
        room_map.get(&room_code).unwrap().host_token.clone()
    };

    let mut host_ws = connect_ws_client(port, &room_code, &format!("?token={}", host_token)).await;
    let _initial = recv_msgs(&mut host_ws).await;

    let mut aj_ws = connect_ws_client(port, &room_code, "?playerName=AJ").await;
    let aj_init = recv_msgs(&mut aj_ws).await;
    let _host_update1 = recv_msgs(&mut host_ws).await;

    let aj_id = aj_init
        .iter()
        .find_map(|m| {
            if let WsMsg::NewPlayer { pid, .. } = m {
                Some(*pid)
            } else {
                None
            }
        })
        .unwrap();

    let mut sam_ws = connect_ws_client(port, &room_code, "?playerName=Sam").await;
    let sam_init = recv_msgs(&mut sam_ws).await;
    let _host_update2 = recv_msgs(&mut host_ws).await;

    let sam_id = sam_init
        .iter()
        .find_map(|m| {
            if let WsMsg::NewPlayer { pid, .. } = m {
                Some(*pid)
            } else {
                None
            }
        })
        .unwrap();

    send_msg_and_recv_all(&mut host_ws, &WsMsg::StartGame {}).await;
    let _aj_start = recv_msgs(&mut aj_ws).await;
    let _sam_start = recv_msgs(&mut sam_ws).await;

    send_msg_and_recv_all(
        &mut host_ws,
        &WsMsg::HostChoice {
            category_index: 0,
            question_index: 0,
        },
    )
    .await;
    let _aj_choice1 = recv_msgs(&mut aj_ws).await;
    let _sam_choice1 = recv_msgs(&mut sam_ws).await;

    send_msg_and_recv_all(&mut host_ws, &WsMsg::HostReady {}).await;
    let _aj_ready1 = recv_msgs(&mut aj_ws).await;
    let _sam_ready1 = recv_msgs(&mut sam_ws).await;

    send_msg_and_recv_all(&mut aj_ws, &WsMsg::Buzz {}).await;
    let _host_buzz1 = recv_msgs(&mut host_ws).await;
    let _sam_buzz1 = recv_msgs(&mut sam_ws).await;

    send_msg_and_recv_all(&mut host_ws, &WsMsg::HostChecked { correct: true }).await;
    let _aj_answer1 = recv_msgs(&mut aj_ws).await;
    let _sam_answer1 = recv_msgs(&mut sam_ws).await;

    {
        let room_map = state.room_map.lock().await;
        let room = room_map.get(&room_code).unwrap();
        let aj = room.players.iter().find(|p| p.player.pid == aj_id).unwrap();
        assert_eq!(aj.player.score, 100);
    }

    send_msg_and_recv_all(
        &mut host_ws,
        &WsMsg::HostChoice {
            category_index: 0,
            question_index: 1,
        },
    )
    .await;
    let _aj_choice2 = recv_msgs(&mut aj_ws).await;
    let _sam_choice2 = recv_msgs(&mut sam_ws).await;

    send_msg_and_recv_all(&mut host_ws, &WsMsg::HostReady {}).await;
    let _aj_ready2 = recv_msgs(&mut aj_ws).await;
    let _sam_ready2 = recv_msgs(&mut sam_ws).await;

    send_msg_and_recv_all(&mut sam_ws, &WsMsg::Buzz {}).await;
    let _host_buzz2 = recv_msgs(&mut host_ws).await;
    let _aj_buzz2 = recv_msgs(&mut aj_ws).await;

    send_msg_and_recv_all(&mut host_ws, &WsMsg::HostChecked { correct: false }).await;
    let _aj_answer2 = recv_msgs(&mut aj_ws).await;
    let _sam_answer2 = recv_msgs(&mut sam_ws).await;

    {
        let room_map = state.room_map.lock().await;
        let room = room_map.get(&room_code).unwrap();
        let aj = room.players.iter().find(|p| p.player.pid == aj_id).unwrap();
        let sam = room
            .players
            .iter()
            .find(|p| p.player.pid == sam_id)
            .unwrap();
        assert_eq!(aj.player.score, 100);
        assert_eq!(sam.player.score, -200);
    }

    send_msg_and_recv_all(
        &mut host_ws,
        &WsMsg::HostChoice {
            category_index: 0,
            question_index: 1,
        },
    )
    .await;
    let _aj_choice3 = recv_msgs(&mut aj_ws).await;
    let _sam_choice3 = recv_msgs(&mut sam_ws).await;

    send_msg_and_recv_all(&mut host_ws, &WsMsg::HostReady {}).await;
    let _aj_ready3 = recv_msgs(&mut aj_ws).await;
    let _sam_ready3 = recv_msgs(&mut sam_ws).await;

    send_msg_and_recv_all(&mut aj_ws, &WsMsg::Buzz {}).await;
    let _host_buzz3 = recv_msgs(&mut host_ws).await;
    let _sam_answer3 = recv_msgs(&mut sam_ws).await;

    let host_answer3 =
        send_msg_and_recv_all(&mut host_ws, &WsMsg::HostChecked { correct: true }).await;
    let _aj_answer3 = recv_msgs(&mut aj_ws).await;
    let _sam_answer3 = recv_msgs(&mut sam_ws).await;

    let game_end = host_answer3.iter().any(|m| {
        matches!(
            m,
            WsMsg::GameState {
                state: GameState::GameEnd,
                ..
            }
        )
    });
    assert!(game_end, "Game should end after last question");
    {
        let room_map = state.room_map.lock().await;
        let room = room_map.get(&room_code).unwrap();
        let aj = room.players.iter().find(|p| p.player.pid == aj_id).unwrap();
        let sam = room
            .players
            .iter()
            .find(|p| p.player.pid == sam_id)
            .unwrap();
        assert_eq!(aj.player.score, 300, "AJ should have 300 points");
        assert_eq!(sam.player.score, -200, "Sam should have -200 points");
        assert!(matches!(room.state, GameState::GameEnd));
    }
}
