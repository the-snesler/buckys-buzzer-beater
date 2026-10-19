mod common;

use std::time::Duration;

use tokio::time::sleep;

use common::*;
use madhacks2025::api::messages::{GameCommand, GameEvent};

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
            room_map
                .get(&room_code)
                .expect("Could not find room")
                .host_token
                .clone()
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

mod gameplay_tests {
    use madhacks2025::{api::messages::WagerRange, game::GameState, net::connection::PlayerEntry};

    use super::*;

    #[tokio::test]
    async fn test_player_joins_room() {
        let (_server, port, state) = start_test_server().await;
        let room_code = create_room_http(port).await;

        let host_token = {
            let room_map = state.room_map.lock().await;
            room_map
                .get(&room_code)
                .expect("Could now find room")
                .host_token
                .clone()
        };
        let mut host_ws =
            connect_ws_client(port, &room_code, &format!("?token={}", host_token)).await;
        let _initial_msgs = recv_msgs(&mut host_ws).await;

        let (_player_ws, player_id) = add_player(port, &room_code, "AJ").await;

        let host_msgs = recv_msgs(&mut host_ws).await;
        let player_list_msg = host_msgs
            .iter()
            .find(|m| matches!(m, GameEvent::PlayerList { .. }));

        if let Some(GameEvent::PlayerList { players }) = player_list_msg {
            assert_eq!(players.len(), 1, "Should have 1 player");
            assert_eq!(players[0].name, "AJ");
            assert_eq!(players[0].pid, player_id);
        } else {
            panic!("Host should receive PlayerList");
        }

        let room_map = state.room_map.lock().await;
        let room = room_map.get(&room_code).expect("Could not find room");
        assert_eq!(room.players.len(), 1, "Room should have 1 player in state");
    }

    #[tokio::test]
    async fn test_multiple_players_join() {
        let (_server, port, state) = start_test_server().await;
        let room_code = create_room_http(port).await;

        let host_token = {
            let room_map = state.room_map.lock().await;
            room_map
                .get(&room_code)
                .expect("Could not find room")
                .host_token
                .clone()
        };
        let mut host_ws =
            connect_ws_client(port, &room_code, &format!("?token={}", host_token)).await;
        let _initial = recv_msgs(&mut host_ws).await;

        let (_alice_ws, _alice_id) = add_player(port, &room_code, "Alice").await;
        let _host_update1 = recv_msgs(&mut host_ws).await;

        let (_bob_ws, _bob_id) = add_player(port, &room_code, "Bob").await;
        let _host_update2 = recv_msgs(&mut host_ws).await;

        let (_charlie_ws, _charlie_id) = add_player(port, &room_code, "Charlie").await;
        let host_final = recv_msgs(&mut host_ws).await;

        let player_list = host_final
            .iter()
            .find(|m| matches!(m, GameEvent::PlayerList { .. }));
        if let Some(GameEvent::PlayerList { players }) = player_list {
            assert_eq!(players.len(), 3, "Should have 3 players");
            let names: Vec<&str> = players.iter().map(|p| p.name.as_str()).collect();
            assert!(names.contains(&"Alice"));
            assert!(names.contains(&"Bob"));
            assert!(names.contains(&"Charlie"));
        } else {
            panic!("Should receive PlayerList");
        }

        let room_map = state.room_map.lock().await;
        let room = room_map.get(&room_code).expect("Could not find room");
        assert_eq!(room.players.len(), 3);
    }

    #[tokio::test]
    async fn test_game_flow_start_to_buzz() {
        let (_server, port, state) = start_test_server().await;
        let room_code = create_room_http(port).await;
        add_room_categories(&state, &room_code).await;

        let host_token = {
            let room_map = state.room_map.lock().await;
            room_map
                .get(&room_code)
                .expect("Could not find room")
                .host_token
                .clone()
        };
        let mut host_ws =
            connect_ws_client(port, &room_code, &format!("?token={}", host_token)).await;
        let _initial = recv_msgs(&mut host_ws).await;

        let (mut player_ws, player_id) = add_player(port, &room_code, "AJ").await;
        let _ = recv_msgs(&mut host_ws).await; // Consume host update

        start_game(&mut host_ws, &mut [&mut player_ws], player_id).await;

        send_cmd_and_recv_all(
            &mut host_ws,
            &GameCommand::HostChoice {
                category_index: 0,
                question_index: 0,
            },
        )
        .await;
        let _ = recv_msgs(&mut player_ws).await;

        send_cmd_and_recv_all(&mut host_ws, &GameCommand::HostReady {}).await;
        let player_update = recv_msgs(&mut player_ws).await;

        let buzz_state = player_update.iter().find(|m| {
            if let GameEvent::PublicState { state, .. } = m {
                matches!(state, GameState::WaitingForBuzz)
            } else {
                false
            }
        });
        assert!(
            buzz_state.is_some(),
            "Player should get WaitingForBuzz state"
        );
        assert!(
            !player_update
                .iter()
                .any(|m| matches!(m, GameEvent::GameState { .. })),
            "Player should not get the full GameState"
        );

        send_cmd_and_recv_all(&mut player_ws, &GameCommand::Buzz { reaction_ms: 200 }).await;
        wait_for_buzz_window().await;
        let host_buzz = recv_msgs(&mut host_ws).await;

        let buzz_notification = host_buzz
            .iter()
            .find(|m| matches!(m, GameEvent::PlayerBuzzed { .. }));
        assert!(
            buzz_notification.is_some(),
            "Host should receive PlayerBuzzed"
        );

        if let Some(GameEvent::PlayerBuzzed { pid, .. }) = buzz_notification {
            assert_eq!(*pid, player_id, "Correct player buzzed");
        }

        let room_map = state.room_map.lock().await;
        let room = room_map.get(&room_code).expect("Could not find room");
        assert!(matches!(room.state, GameState::Answer));
    }

    #[tokio::test]
    async fn test_player_reconnect() {
        let (_server, port, state) = start_test_server().await;
        let room_code = create_room_http(port).await;
        add_room_categories(&state, &room_code).await;

        let host_token = {
            let room_map = state.room_map.lock().await;
            room_map
                .get(&room_code)
                .expect("Could not find room")
                .host_token
                .clone()
        };
        let mut host_ws =
            connect_ws_client(port, &room_code, &format!("?token={}", host_token)).await;
        let _ = recv_msgs(&mut host_ws).await;

        let (mut player_ws, player_id) = add_player(port, &room_code, "AJ").await;
        let _ = recv_msgs(&mut host_ws).await;
        start_game(&mut host_ws, &mut [&mut player_ws], player_id).await;
        let player_token = {
            let room_map = state.room_map.lock().await;
            let room = room_map.get(&room_code).expect("Could not find room");
            room.players
                .iter()
                .find(|p| p.player.pid == player_id)
                .expect("Could not find player")
                .player
                .token
                .clone()
        };

        {
            let room_map = state.room_map.lock().await;
            let room = room_map.get(&room_code).expect("Could not find room");
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
            let room = room_map.get(&room_code).expect("Could not find room");
            assert_eq!(
                room.players.len(),
                1,
                "Should have 1 player after disconnect"
            );
        }

        // Reconnect
        let mut player_reconnect = connect_ws_client(
            port,
            &room_code,
            &format!("?token={}&playerID={}", player_token, player_id),
        )
        .await;

        let reconnect_msgs = recv_msgs(&mut player_reconnect).await;

        let got_new_player = reconnect_msgs
            .iter()
            .any(|m| matches!(m, GameEvent::NewPlayer { .. }));
        assert!(!got_new_player, "Should not get NewPlayer on reconnect");

        let has_public_state = reconnect_msgs.iter().any(|m| {
            matches!(
                m,
                GameEvent::PublicState {
                    state: GameState::Selection,
                    ..
                }
            )
        });
        assert!(has_public_state, "Should receive PublicState on reconnect");
        let has_player_state = reconnect_msgs
            .iter()
            .any(|m| matches!(m, GameEvent::PlayerState { .. }));
        assert!(has_player_state, "Should receive PlayerState on reconnect");

        if let Some(GameEvent::PlayerState { pid, .. }) = reconnect_msgs
            .iter()
            .find(|m| matches!(m, GameEvent::PlayerState { .. }))
        {
            let room_map = state.room_map.lock().await;
            let room = room_map.get(&room_code).expect("Could not find room ");
            let player = room
                .players
                .iter()
                .find(|p| &p.player.pid == pid)
                .map(|p| &p.player)
                .expect("Could not find player");
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
            let room = room_map.get(&room_code).expect("Could not find room");
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
        add_room_categories(state.as_ref(), &room_code).await;

        let host_token = {
            let room_map = state.room_map.lock().await;
            room_map
                .get(&room_code)
                .expect("Could not find room")
                .host_token
                .clone()
        };
        let mut host_ws =
            connect_ws_client(port, &room_code, &format!("?token={}", host_token)).await;
        let _initial = recv_msgs(&mut host_ws).await;

        let (mut player_ws, player_id) = add_player(port, &room_code, "AJ").await;
        let _ = recv_msgs(&mut host_ws).await;

        start_game(&mut host_ws, &mut [&mut player_ws], player_id).await;

        play_question(&mut host_ws, &mut player_ws, 0, 0, true).await;

        let room_map = state.room_map.lock().await;
        let score = get_player_score(&room_map, &room_code, player_id);
        assert_eq!(score, 100, "Score should be 100 after correct answer");

        let room = room_map.get(&room_code).expect("Could not find room");
        assert!(matches!(room.state, GameState::Selection));
    }

    #[tokio::test]
    async fn test_incorrect_answer_deducts_points() {
        let (_server, port, state) = start_test_server().await;
        let room_code = create_room_http(port).await;
        add_room_categories(state.as_ref(), &room_code).await;

        let host_token = {
            let room_map = state.room_map.lock().await;
            room_map
                .get(&room_code)
                .expect("Could not find room code")
                .host_token
                .clone()
        };
        let mut host_ws =
            connect_ws_client(port, &room_code, &format!("?token={}", host_token)).await;
        let _initial = recv_msgs(&mut host_ws).await;

        let (mut player_ws, player_id) = add_player(port, &room_code, "AJ").await;
        let _ = recv_msgs(&mut host_ws).await;

        start_game(&mut host_ws, &mut [&mut player_ws], player_id).await;

        play_question(&mut host_ws, &mut player_ws, 0, 0, false).await;

        let room_map = state.room_map.lock().await;
        let score = get_player_score(&room_map, &room_code, player_id);
        assert_eq!(score, -100, "Score should be -100 after correct answer");

        let room = room_map.get(&room_code).expect("Could not find room");
        assert!(matches!(room.state, GameState::Selection));
    }

    #[tokio::test]
    async fn test_host_reconnect() {
        let (_server, port, state) = start_test_server().await;
        let room_code = create_room_http(port).await;
        add_room_categories(&state, &room_code).await;

        let host_token = {
            let room_map = state.room_map.lock().await;
            room_map
                .get(&room_code)
                .expect("Could not find room")
                .host_token
                .clone()
        };

        let mut host_ws =
            connect_ws_client(port, &room_code, &format!("?token={}", host_token)).await;
        let _initial = recv_msgs(&mut host_ws).await;

        let (mut player_ws, player_id) = add_player(port, &room_code, "AJ").await;
        let _ = recv_msgs(&mut host_ws).await;

        start_game(&mut host_ws, &mut [&mut player_ws], player_id).await;

        send_cmd_and_recv_all(
            &mut host_ws,
            &GameCommand::HostChoice {
                category_index: 0,
                question_index: 0,
            },
        )
        .await;
        let _ = recv_msgs(&mut player_ws).await;

        let state_before = {
            let room_map = state.room_map.lock().await;
            let room = room_map.get(&room_code).expect("Could not find room");
            room.state.clone()
        };
        assert!(matches!(state_before, GameState::QuestionReading));

        // Host Disconnect
        drop(host_ws);
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

        let mut host_reconnect =
            connect_ws_client(port, &room_code, &format!("?token={}", host_token)).await;
        let reconnect_msgs = recv_msgs(&mut host_reconnect).await;

        let game_state_msg = reconnect_msgs
            .iter()
            .find(|m| matches!(m, GameEvent::GameState { .. }));
        assert!(
            game_state_msg.is_some(),
            "Host should receive GameState on reconnect"
        );

        if let Some(GameEvent::GameState {
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

        send_cmd_and_recv_all(&mut host_reconnect, &GameCommand::HostReady {}).await;
        let player_ready = recv_msgs(&mut player_ws).await;

        let waiting_state = player_ready.iter().any(|m| {
            matches!(
                m,
                GameEvent::PublicState {
                    state: GameState::WaitingForBuzz,
                    ..
                }
            )
        });
        assert!(waiting_state, "Game should continue after host reconnects");
    }

    #[tokio::test]
    async fn test_players_never_receive_game_state() {
        let (_server, port, state) = start_test_server().await;
        let room_code = create_room_http(port).await;
        add_room_categories(&state, &room_code).await;

        let host_token = {
            let room_map = state.room_map.lock().await;
            room_map
                .get(&room_code)
                .expect("Could not find room")
                .host_token
                .clone()
        };
        let mut host_ws =
            connect_ws_client(port, &room_code, &format!("?token={}", host_token)).await;
        let _ = recv_msgs(&mut host_ws).await;

        let mut aj_ws = connect_ws_client(port, &room_code, "?playerName=AJ").await;
        let mut aj_texts = recv_texts(&mut aj_ws).await;
        let mut sam_ws = connect_ws_client(port, &room_code, "?playerName=Sam").await;
        let mut sam_texts = recv_texts(&mut sam_ws).await;

        send_cmd_and_recv_all(&mut host_ws, &GameCommand::StartGame).await;
        send_cmd_and_recv_all(&mut host_ws, &GameCommand::HostSetPicker { pid: 1 }).await;
        send_cmd_and_recv_all(
            &mut host_ws,
            &GameCommand::HostChoice {
                category_index: 0,
                question_index: 0,
            },
        )
        .await;
        send_cmd_and_recv_all(&mut host_ws, &GameCommand::HostReady).await;
        // Let the delayed HostReady witness arrive
        sleep(Duration::from_millis(600)).await;
        aj_texts.extend(recv_texts(&mut aj_ws).await);
        sam_texts.extend(recv_texts(&mut sam_ws).await);

        let witness = sam_texts
            .iter()
            .filter_map(|t| serde_json::from_str::<GameEvent>(t).ok())
            .find_map(|m| match m {
                GameEvent::Witness { msg } => Some(*msg),
                _ => None,
            })
            .expect("Players should receive the HostReady witness");
        assert!(matches!(
            witness,
            GameEvent::PublicState {
                state: GameState::WaitingForBuzz,
                ..
            }
        ));

        // A late joiner and a reconnecting player both get the public state
        let mut late_ws = connect_ws_client(port, &room_code, "?playerName=Late").await;
        let mut late_texts = recv_texts(&mut late_ws).await;

        let (aj_id, aj_token) = {
            let room_map = state.room_map.lock().await;
            let room = room_map.get(&room_code).expect("Could not find room");
            let aj = &room.players[0].player;
            (aj.pid, aj.token.clone())
        };
        drop(aj_ws);
        sleep(Duration::from_millis(100)).await;
        let mut aj_ws = connect_ws_client(
            port,
            &room_code,
            &format!("?token={}&playerID={}", aj_token, aj_id),
        )
        .await;
        let reconnect_texts = recv_texts(&mut aj_ws).await;
        for texts in [&late_texts, &reconnect_texts] {
            assert!(
                texts
                    .iter()
                    .filter_map(|t| serde_json::from_str::<GameEvent>(t).ok())
                    .any(|m| matches!(
                        m,
                        GameEvent::PublicState {
                            state: GameState::WaitingForBuzz,
                            ..
                        }
                    )),
                "Should receive PublicState on join: {:?}",
                texts
            );
        }
        aj_texts.extend(reconnect_texts);

        send_cmd_and_recv_all(&mut sam_ws, &GameCommand::Buzz { reaction_ms: 200 }).await;
        wait_for_buzz_window().await;
        send_cmd_and_recv_all(&mut host_ws, &GameCommand::HostChecked { correct: true }).await;
        send_cmd_and_recv_all(&mut host_ws, &GameCommand::HostContinue).await;
        aj_texts.extend(recv_texts(&mut aj_ws).await);
        sam_texts.extend(recv_texts(&mut sam_ws).await);
        late_texts.extend(recv_texts(&mut late_ws).await);

        let (tokens, answers): (Vec<String>, Vec<String>) = {
            let room_map = state.room_map.lock().await;
            let room = room_map.get(&room_code).expect("Could not find room");
            (
                room.players
                    .iter()
                    .map(|p| p.player.token.to_string())
                    .collect(),
                room.rounds[0][0]
                    .questions
                    .iter()
                    .map(|q| q.answer.clone())
                    .collect(),
            )
        };

        for (i, texts) in [&aj_texts, &sam_texts, &late_texts].iter().enumerate() {
            assert!(
                texts.iter().any(|t| t.contains(r#""type":"PublicState""#)),
                "Player {} should receive PublicState",
                i
            );
            for text in texts.iter() {
                assert!(
                    !text.contains(r#""type":"GameState""#),
                    "Player {} received GameState: {}",
                    i,
                    text
                );
                for answer in &answers {
                    assert!(!text.contains(answer), "Answer leaked: {}", text);
                }
                for (j, token) in tokens.iter().enumerate() {
                    // Only NewPlayer may carry a token, and only the owner's
                    let allowed = i == j && text.contains(r#""type":"NewPlayer""#);
                    assert!(
                        allowed || !text.contains(token),
                        "Player {}'s token leaked to player {}: {}",
                        j,
                        i,
                        text
                    );
                }
            }
        }
    }

    #[tokio::test]
    async fn test_player_cannot_send_host_commands() {
        let (_server, port, state) = start_test_server().await;
        let room_code = create_room_http(port).await;
        add_room_categories(&state, &room_code).await;

        let host_token = {
            let room_map = state.room_map.lock().await;
            room_map
                .get(&room_code)
                .expect("Could not find room")
                .host_token
                .clone()
        };
        let mut host_ws =
            connect_ws_client(port, &room_code, &format!("?token={}", host_token)).await;
        let _ = recv_msgs(&mut host_ws).await;

        let (mut player_ws, _player_id) = add_player(port, &room_code, "AJ").await;
        let _ = recv_msgs(&mut host_ws).await;

        let player_msgs = send_cmd_and_recv_all(&mut player_ws, &GameCommand::StartGame).await;
        let host_msgs = recv_msgs(&mut host_ws).await;
        assert!(player_msgs.is_empty(), "Got {:?}", player_msgs);
        assert!(host_msgs.is_empty(), "Got {:?}", host_msgs);

        let room_map = state.room_map.lock().await;
        let room = room_map.get(&room_code).expect("Could not find room");
        assert!(matches!(room.state, GameState::Start));
    }

    #[tokio::test]
    async fn test_disconnected_player_does_not_freeze_server() {
        const QUESTIONS: usize = 40;

        let (_server, port, state) = start_test_server().await;
        let room_code = create_room_http(port).await;
        add_room_category(&state, &room_code, QUESTIONS).await;

        let host_token = {
            let room_map = state.room_map.lock().await;
            room_map
                .get(&room_code)
                .expect("Could not find room")
                .host_token
                .clone()
        };
        let mut host_ws =
            connect_ws_client(port, &room_code, &format!("?token={}", host_token)).await;
        let _ = recv_msgs(&mut host_ws).await;

        let (player_ws, player_id) = add_player(port, &room_code, "AJ").await;
        let _ = recv_msgs(&mut host_ws).await;
        let player_token = {
            let room_map = state.room_map.lock().await;
            let room = room_map.get(&room_code).expect("Could not find room");
            room.players[0].player.token.clone()
        };

        // The player closes their tab, but stays in the room
        drop(player_ws);
        sleep(Duration::from_millis(100)).await;

        // Every one of these sends the player several messages that nobody reads
        send_cmd(&mut host_ws, &GameCommand::StartGame).await;
        send_cmd(&mut host_ws, &GameCommand::HostSetPicker { pid: player_id }).await;
        for question_index in 0..QUESTIONS {
            send_cmd(
                &mut host_ws,
                &GameCommand::HostChoice {
                    category_index: 0,
                    question_index,
                },
            )
            .await;
            send_cmd(&mut host_ws, &GameCommand::HostSkip).await;
            send_cmd(&mut host_ws, &GameCommand::HostContinue).await;
        }

        let finished = tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                let _ = recv_msgs(&mut host_ws).await;
                let room_map = state.room_map.lock().await;
                let room = room_map.get(&room_code).expect("Could not find room");
                if room.state == GameState::GameEnd {
                    break;
                }
            }
        })
        .await;
        assert!(
            finished.is_ok(),
            "Server froze sending to a disconnected player"
        );

        // The player can still come back
        let mut player_reconnect = connect_ws_client(
            port,
            &room_code,
            &format!("?token={}&playerID={}", player_token, player_id),
        )
        .await;
        let reconnect_msgs =
            tokio::time::timeout(Duration::from_secs(5), recv_msgs(&mut player_reconnect))
                .await
                .expect("Reconnect timed out");
        assert!(
            reconnect_msgs.iter().any(|m| matches!(
                m,
                GameEvent::PublicState {
                    state: GameState::GameEnd,
                    ..
                }
            )),
            "Reconnected player should get the current state: {:?}",
            reconnect_msgs
        );
    }

    #[tokio::test]
    async fn test_full_game() {
        let (_server, port, state) = start_test_server().await;
        let room_code = create_room_http(port).await;
        add_room_categories(state.as_ref(), &room_code).await;

        let host_token = {
            let room_map = state.room_map.lock().await;
            room_map
                .get(&room_code)
                .expect("Could not find room")
                .host_token
                .clone()
        };

        let mut host_ws =
            connect_ws_client(port, &room_code, &format!("?token={}", host_token)).await;
        let _initial = recv_msgs(&mut host_ws).await;

        let (mut aj_ws, aj_id) = add_player(port, &room_code, "AJ").await;
        let _ = recv_msgs(&mut host_ws).await;

        let (mut sam_ws, sam_id) = add_player(port, &room_code, "Sam").await;
        let _ = recv_msgs(&mut host_ws).await;

        start_game(&mut host_ws, &mut [&mut aj_ws, &mut sam_ws], aj_id).await;

        // Question 1: AJ buzzes and gets it correct (+100)
        play_question(&mut host_ws, &mut aj_ws, 0, 0, true).await;
        let _ = recv_msgs(&mut sam_ws).await;

        {
            let room_map = state.room_map.lock().await;
            assert_eq!(get_player_score(&room_map, &room_code, aj_id), 100);
        }

        // Question 2: Sam buzzes and gets it incorrect (-200)
        send_cmd_and_recv_all(
            &mut host_ws,
            &GameCommand::HostChoice {
                category_index: 0,
                question_index: 1,
            },
        )
        .await;
        send_cmd_and_recv_all(&mut host_ws, &GameCommand::HostReady).await;
        let _ = recv_msgs(&mut sam_ws).await;
        send_cmd_and_recv_all(&mut sam_ws, &GameCommand::Buzz { reaction_ms: 200 }).await;
        wait_for_buzz_window().await;
        send_cmd_and_recv_all(&mut host_ws, &GameCommand::HostChecked { correct: false }).await;
        let _ = recv_msgs(&mut sam_ws).await;
        let _ = recv_msgs(&mut aj_ws).await;

        {
            let room_map = state.room_map.lock().await;
            assert_eq!(get_player_score(&room_map, &room_code, aj_id), 100);
            assert_eq!(get_player_score(&room_map, &room_code, sam_id), -200);
            let room = room_map.get(&room_code).expect("Could not find room");
            assert!(
                matches!(room.state, GameState::WaitingForBuzz),
                "Buzzing should reopen for AJ"
            );
        }

        // Question 2 rebound: AJ buzzes and gets it correct (+200 = 300 total)
        send_cmd_and_recv_all(&mut aj_ws, &GameCommand::Buzz { reaction_ms: 200 }).await;
        wait_for_buzz_window().await;
        send_cmd_and_recv_all(&mut host_ws, &GameCommand::HostChecked { correct: true }).await;
        send_cmd_and_recv_all(&mut host_ws, &GameCommand::HostContinue).await;
        let _ = recv_msgs(&mut aj_ws).await;
        let _ = recv_msgs(&mut sam_ws).await;

        // Question 3: AJ buzzes and gets it correct (+400 = 600 total)
        play_question(&mut host_ws, &mut aj_ws, 0, 2, true).await;
        let _ = recv_msgs(&mut sam_ws).await;

        {
            let room_map = state.room_map.lock().await;
            let room = room_map.get(&room_code).expect("Could not find room");
            assert_eq!(
                get_player_score(&room_map, &room_code, aj_id),
                600,
                "AJ should have 600 points"
            );
            assert_eq!(
                get_player_score(&room_map, &room_code, sam_id),
                -200,
                "Sam should have -200 points"
            );
            assert!(matches!(room.state, GameState::GameEnd));
        }
    }

    #[tokio::test]
    async fn test_two_round_game() {
        let (_server, port, state) = start_test_server().await;
        let category = |title: &str, value: u32| {
            serde_json::json!({
                "title": title,
                "questions": [
                    {"question": "Question 1", "answer": "Answer 1", "value": value},
                    {"question": "Question 2", "answer": "Answer 2", "value": value * 2},
                ],
            })
        };
        let room_code = create_room_http_with(
            port,
            serde_json::json!({
                "categories": [category("Single", 200)],
                "doubleCategories": [category("Double", 400)],
            }),
        )
        .await;

        let host_token = {
            let room_map = state.room_map.lock().await;
            let room = room_map.get(&room_code).expect("Could not find room");
            assert_eq!(room.rounds.len(), 2, "Room should have two rounds");
            room.host_token.clone()
        };
        let mut host_ws =
            connect_ws_client(port, &room_code, &format!("?token={}", host_token)).await;
        let _ = recv_msgs(&mut host_ws).await;

        let (mut aj_ws, aj_id) = add_player(port, &room_code, "AJ").await;
        let (mut sam_ws, sam_id) = add_player(port, &room_code, "Sam").await;
        let _ = recv_msgs(&mut host_ws).await;

        start_game(&mut host_ws, &mut [&mut aj_ws, &mut sam_ws], aj_id).await;

        // Clearing the Jeopardy board (+200, +400) starts Double Jeopardy
        play_question(&mut host_ws, &mut aj_ws, 0, 0, true).await;
        play_question(&mut host_ws, &mut aj_ws, 0, 1, true).await;
        {
            let room_map = state.room_map.lock().await;
            let room = room_map.get(&room_code).expect("Could not find room");
            assert!(matches!(room.state, GameState::Selection));
            assert_eq!(room.round_index, 1, "Double Jeopardy should start");
            assert_eq!(get_player_score(&room_map, &room_code, aj_id), 600);
        }
        let sam_msgs = recv_msgs(&mut sam_ws).await;
        let last_public = sam_msgs
            .iter()
            .rev()
            .find_map(|m| match m {
                GameEvent::PublicState { state, round, .. } => Some((state.clone(), *round)),
                _ => None,
            })
            .expect("Sam should receive PublicState");
        assert_eq!(
            last_public,
            (GameState::Selection, 1),
            "Players should be told Double Jeopardy started"
        );

        // A reconnecting host gets the Double Jeopardy board
        drop(host_ws);
        sleep(Duration::from_millis(100)).await;
        let mut host_ws =
            connect_ws_client(port, &room_code, &format!("?token={}", host_token)).await;
        let reconnect_state = recv_msgs(&mut host_ws)
            .await
            .into_iter()
            .find_map(|m| match m {
                GameEvent::GameState {
                    round,
                    round_count,
                    categories,
                    ..
                } => Some((round, round_count, categories[0].title.clone())),
                _ => None,
            })
            .expect("Reconnecting host should receive GameState");
        assert_eq!(reconnect_state, (1, 2, "Double".to_string()));

        // A late joiner is told it's Double Jeopardy
        let mut late_ws = connect_ws_client(port, &room_code, "?playerName=Late").await;
        let late_round = recv_msgs(&mut late_ws)
            .await
            .into_iter()
            .find_map(|m| match m {
                GameEvent::PublicState { state, round, .. } => Some((state, round)),
                _ => None,
            });
        assert_eq!(late_round, Some((GameState::Selection, 1)));
        let _ = recv_msgs(&mut host_ws).await;

        // Clue indices now refer to the Double Jeopardy board (+800)
        play_question(&mut host_ws, &mut aj_ws, 0, 1, true).await;
        {
            let room_map = state.room_map.lock().await;
            assert_eq!(get_player_score(&room_map, &room_code, aj_id), 1400);
            assert_eq!(get_player_score(&room_map, &room_code, sam_id), 0);
        }

        // The host ends Double Jeopardy with a clue left, which ends the game
        let host_msgs =
            send_cmd_and_recv_all(&mut host_ws, &GameCommand::HostNextRound { round: 1 }).await;
        let game_state = host_msgs
            .iter()
            .find_map(|m| match m {
                GameEvent::GameState {
                    state,
                    round,
                    round_count,
                    categories,
                    winner,
                    ..
                } => Some((
                    state.clone(),
                    *round,
                    *round_count,
                    categories[0].title.clone(),
                    *winner,
                )),
                _ => None,
            })
            .expect("Host should receive GameState");
        assert_eq!(
            game_state,
            (GameState::GameEnd, 1, 2, "Double".to_string(), Some(aj_id))
        );
    }

    /// The last PublicState in `msgs`, as `(state, round, picker)`
    fn last_public_state(msgs: &[GameEvent]) -> Option<(GameState, usize, Option<u32>)> {
        msgs.iter().rev().find_map(|m| match m {
            GameEvent::PublicState {
                state,
                round,
                picker,
            } => Some((state.clone(), *round, *picker)),
            _ => None,
        })
    }

    /// What the host is told: `(state, current buzzer, picker, Daily Double wager)`
    type HostView = (GameState, Option<u32>, Option<u32>, Option<u32>);

    /// The last GameState in `msgs`
    fn last_game_state(msgs: &[GameEvent]) -> Option<HostView> {
        msgs.iter().rev().find_map(|m| match m {
            GameEvent::GameState {
                state,
                current_buzzer,
                picker,
                dd_wager,
                ..
            } => Some((state.clone(), *current_buzzer, *picker, *dd_wager)),
            _ => None,
        })
    }

    fn parse_all(texts: &[String]) -> Vec<GameEvent> {
        texts
            .iter()
            .map(|t| serde_json::from_str(t).expect("Failed to parse GameEvent"))
            .collect()
    }

    #[tokio::test]
    async fn test_picker_and_daily_double() {
        let (_server, port, state) = start_test_server().await;
        let room_code = create_room_http_with(
            port,
            serde_json::json!({
                "categories": [{"title": "Single", "questions": [
                    {"question": "Question 1", "answer": "Answer 1", "value": 200},
                    {"question": "Question 2", "answer": "Answer 2", "value": 400, "dailyDouble": true},
                    {"question": "Question 3", "answer": "Answer 3", "value": 600},
                ]}],
                "doubleCategories": [{"title": "Double", "questions": [
                    {"question": "Question 4", "answer": "Answer 4", "value": 400},
                    {"question": "Question 5", "answer": "Answer 5", "value": 800},
                ]}],
            }),
        )
        .await;

        let host_token = {
            let room_map = state.room_map.lock().await;
            let room = room_map.get(&room_code).expect("Could not find room");
            assert!(room.rounds[0][0].questions[1].daily_double);
            room.host_token.clone()
        };
        let mut host_ws =
            connect_ws_client(port, &room_code, &format!("?token={}", host_token)).await;
        let mut host_texts = recv_texts(&mut host_ws).await;
        let (mut aj_ws, aj_id) = add_player(port, &room_code, "AJ").await;
        let (mut sam_ws, sam_id) = add_player(port, &room_code, "Sam").await;
        host_texts.extend(recv_texts(&mut host_ws).await);
        let mut aj_texts = Vec::new();

        // Nobody can choose a clue until the host says who picks first
        send_cmd(&mut host_ws, &GameCommand::StartGame).await;
        host_texts.extend(recv_texts(&mut host_ws).await);
        let sam_msgs = recv_msgs(&mut sam_ws).await;
        assert_eq!(
            last_public_state(&sam_msgs),
            Some((GameState::Selection, 0, None))
        );
        send_cmd(
            &mut host_ws,
            &GameCommand::HostChoice {
                category_index: 0,
                question_index: 0,
            },
        )
        .await;
        let ignored = recv_texts(&mut host_ws).await;
        assert!(ignored.is_empty(), "HostChoice needs a picker: {ignored:?}");

        let host_msgs =
            send_cmd_and_recv_all(&mut host_ws, &GameCommand::HostSetPicker { pid: aj_id }).await;
        assert_eq!(
            last_game_state(&host_msgs),
            Some((GameState::Selection, None, Some(aj_id), None))
        );
        let sam_msgs = recv_msgs(&mut sam_ws).await;
        assert_eq!(
            last_public_state(&sam_msgs),
            Some((GameState::Selection, 0, Some(aj_id)))
        );

        // AJ chooses a clue, and Sam answers it correctly (+200), so Sam picks next
        play_question(&mut host_ws, &mut sam_ws, 0, 0, true).await;
        aj_texts.extend(recv_texts(&mut aj_ws).await);
        assert_eq!(
            last_public_state(&parse_all(&aj_texts)),
            Some((GameState::Selection, 0, Some(sam_id)))
        );

        // Sam finds the Daily Double
        send_cmd(
            &mut host_ws,
            &GameCommand::HostChoice {
                category_index: 0,
                question_index: 1,
            },
        )
        .await;
        let texts = recv_texts(&mut host_ws).await;
        assert_eq!(
            last_game_state(&parse_all(&texts)),
            Some((GameState::DailyDoubleWager, None, Some(sam_id), None))
        );
        host_texts.extend(texts);
        let sam_msgs = recv_msgs(&mut sam_ws).await;
        let sam_view = sam_msgs.iter().find_map(|m| match m {
            GameEvent::PlayerState {
                wager_range,
                prompt,
                ..
            } => Some((*wager_range, prompt.clone())),
            _ => None,
        });
        assert_eq!(
            sam_view,
            Some((
                Some(WagerRange { min: 5, max: 600 }),
                Some("Single".to_string())
            )),
            "Sam can wager up to the top clue, since that's more than Sam's score"
        );

        // Only Sam may wager, and only within the range
        send_cmd(&mut aj_ws, &GameCommand::SubmitWager { amount: 100 }).await;
        let texts = recv_texts(&mut aj_ws).await;
        assert!(
            parse_all(&texts)
                .iter()
                .any(|m| matches!(m, GameEvent::Rejected { .. })),
            "AJ's wager should be rejected: {texts:?}"
        );
        aj_texts.extend(texts);
        let sam_msgs =
            send_cmd_and_recv_all(&mut sam_ws, &GameCommand::SubmitWager { amount: 601 }).await;
        assert!(
            sam_msgs
                .iter()
                .any(|m| matches!(m, GameEvent::Rejected { .. })),
            "Sam's wager above the range should be rejected: {sam_msgs:?}"
        );

        // A valid wager goes straight to Sam's answer, with nobody buzzing
        send_cmd(&mut sam_ws, &GameCommand::SubmitWager { amount: 500 }).await;
        let texts = recv_texts(&mut host_ws).await;
        assert_eq!(
            last_game_state(&parse_all(&texts)),
            Some((GameState::Answer, Some(sam_id), Some(sam_id), Some(500)))
        );
        host_texts.extend(texts);
        let sam_msgs = recv_msgs(&mut sam_ws).await;
        assert!(sam_msgs.iter().any(|m| matches!(
            m,
            GameEvent::PlayerState {
                wager: Some(500),
                ..
            }
        )));

        // Sam wins the wager rather than the clue's value (+500 = 700)
        send_cmd(&mut host_ws, &GameCommand::HostChecked { correct: true }).await;
        send_cmd(&mut host_ws, &GameCommand::HostContinue).await;
        host_texts.extend(recv_texts(&mut host_ws).await);
        aj_texts.extend(recv_texts(&mut aj_ws).await);
        let _ = recv_msgs(&mut sam_ws).await;
        {
            let room_map = state.room_map.lock().await;
            assert_eq!(get_player_score(&room_map, &room_code, sam_id), 700);
            assert_eq!(get_player_score(&room_map, &room_code, aj_id), 0);
            let room = room_map.get(&room_code).expect("Could not find room");
            assert_eq!(room.dd_wager, None);
        }

        // Clearing the board (+600 = 1300) starts Double Jeopardy, where AJ, in last place,
        // picks first
        play_question(&mut host_ws, &mut sam_ws, 0, 2, true).await;
        aj_texts.extend(recv_texts(&mut aj_ws).await);
        assert_eq!(
            last_public_state(&parse_all(&aj_texts)),
            Some((GameState::Selection, 1, Some(aj_id)))
        );
        {
            let room_map = state.room_map.lock().await;
            assert_eq!(get_player_score(&room_map, &room_code, sam_id), 1300);
            let room = room_map.get(&room_code).expect("Could not find room");
            assert_eq!(room.picker, Some(aj_id));
        }

        // Nobody, host included, is told where the Daily Doubles are, and AJ never sees
        // Sam's wager
        for text in host_texts.iter().chain(aj_texts.iter()) {
            assert!(!text.contains(r#""dailyDouble""#), "Flag sent: {text}");
        }
        for msg in parse_all(&aj_texts) {
            if let GameEvent::PlayerState {
                wager_range,
                wager,
                prompt,
                ..
            } = msg
            {
                assert_eq!((wager_range, wager, prompt), (None, None, None));
            }
        }
    }

    #[tokio::test]
    async fn test_random_daily_doubles() {
        let (_server, port, state) = start_test_server().await;
        let board = |value: u32| {
            (0..3)
                .map(|c| {
                    serde_json::json!({
                        "title": format!("Category {c}"),
                        "questions": (1..=3)
                            .map(|i| serde_json::json!({
                                "question": "Q", "answer": "A", "value": value * i,
                            }))
                            .collect::<Vec<_>>(),
                    })
                })
                .collect::<Vec<_>>()
        };
        let room_code = create_room_http_with(
            port,
            serde_json::json!({
                "categories": board(200),
                "doubleCategories": board(400),
                "randomDailyDoubles": true,
            }),
        )
        .await;

        let room_map = state.room_map.lock().await;
        let room = room_map.get(&room_code).expect("Could not find room");
        let counts: Vec<usize> = room
            .rounds
            .iter()
            .map(|round| {
                round
                    .iter()
                    .flat_map(|c| &c.questions)
                    .filter(|q| q.daily_double)
                    .count()
            })
            .collect();
        assert_eq!(counts, vec![1, 2]);
    }

    #[tokio::test]
    async fn test_concurrent_buzzes() {
        let (_server, port, state) = start_test_server().await;
        let room_code = create_room_http(port).await;
        add_room_categories(&state, &room_code).await;

        let host_token = {
            let room_map = state.room_map.lock().await;
            room_map
                .get(&room_code)
                .expect("Could not find room")
                .host_token
                .clone()
        };
        let mut host_ws =
            connect_ws_client(port, &room_code, &format!("?token={}", host_token)).await;
        let _initial = recv_msgs(&mut host_ws).await;

        let (mut aj_ws, aj_id) = add_player(port, &room_code, "AJ").await;
        let _ = recv_msgs(&mut host_ws).await;

        let (mut sam_ws, sam_id) = add_player(port, &room_code, "Sam").await;
        let _ = recv_msgs(&mut host_ws).await;

        start_game(&mut host_ws, &mut [&mut aj_ws, &mut sam_ws], aj_id).await;

        send_cmd_and_recv_all(
            &mut host_ws,
            &GameCommand::HostChoice {
                category_index: 0,
                question_index: 0,
            },
        )
        .await;

        let _ = recv_msgs(&mut aj_ws).await;
        let _ = recv_msgs(&mut sam_ws).await;

        send_cmd_and_recv_all(&mut host_ws, &GameCommand::HostReady {}).await;
        let _ = recv_msgs(&mut aj_ws).await;
        let _ = recv_msgs(&mut sam_ws).await;

        let aj_buzz = tokio::spawn({
            let mut ws = aj_ws;
            async move {
                send_cmd_and_recv_all(&mut ws, &GameCommand::Buzz { reaction_ms: 200 }).await;
                ws
            }
        });

        let sam_buzz = tokio::spawn({
            let mut ws = sam_ws;
            async move {
                send_cmd_and_recv_all(&mut ws, &GameCommand::Buzz { reaction_ms: 200 }).await;
                ws
            }
        });

        let _aj_ws = aj_buzz.await.expect("Could not find AJ websocket");
        let _sam_ws = sam_buzz.await.expect("Could not find Sam websocket");
        wait_for_buzz_window().await;

        let host_msgs = recv_msgs(&mut host_ws).await;
        let buzz_count = host_msgs
            .iter()
            .filter(|m| matches!(m, GameEvent::PlayerBuzzed { .. }))
            .count();
        assert_eq!(buzz_count, 1, "Host should receive exactly one buzz");

        let buzzed_player = host_msgs
            .iter()
            .find_map(|m| {
                if let GameEvent::PlayerBuzzed { pid, .. } = m {
                    Some(*pid)
                } else {
                    None
                }
            })
            .expect("Should have a buzzed player");

        assert!(
            buzzed_player == aj_id || buzzed_player == sam_id,
            "Buzzed player should be either Alice or Bob"
        );

        {
            let room_map = state.room_map.lock().await;
            let room = room_map.get(&room_code).expect("Could not find room");
            assert_eq!(
                room.current_buzzer,
                Some(buzzed_player),
                "Only one player should be the buzzer"
            );

            let buzzer = room
                .players
                .iter()
                .find(|p| p.player.pid == buzzed_player)
                .expect("Could not find buzzed player");
            assert!(buzzer.player.buzzed, "Buzzer should be marked as buzzed");
        }
    }

    #[tokio::test]
    async fn test_concurrent_player_joins() {
        let (_server, port, state) = start_test_server().await;
        let room_code = create_room_http(port).await;

        let host_token = {
            let room_map = state.room_map.lock().await;
            room_map
                .get(&room_code)
                .expect("Could not find room")
                .host_token
                .clone()
        };
        let mut host_ws =
            connect_ws_client(port, &room_code, &format!("?token={}", host_token)).await;
        let _initial = recv_msgs(&mut host_ws).await;

        let mut join_handles = vec![];
        for i in 0..5 {
            let room_code = room_code.clone();
            let handle = tokio::spawn(async move {
                let name = format!("Player{}", i);
                add_player(port, &room_code, &name).await
            });
            join_handles.push(handle);
        }

        let mut player_ids = vec![];
        for handle in join_handles {
            let (_ws, id) = handle.await.expect("Could not find ws handle");
            player_ids.push(id);
        }

        sleep(Duration::from_millis(200)).await;

        {
            let room_map = state.room_map.lock().await;
            let room = room_map.get(&room_code).expect("Could not find room");
            assert_eq!(room.players.len(), 5, "Should have 5 players");
        }

        let mut unique_ids = player_ids.clone();
        unique_ids.sort();
        unique_ids.dedup();
        assert_eq!(
            unique_ids.len(),
            player_ids.len(),
            "All player IDs should be unique"
        );

        let final_msgs = recv_msgs(&mut host_ws).await;
        let final_list = final_msgs.iter().rev().find_map(|m| {
            if let GameEvent::PlayerList { players } = m {
                Some(players)
            } else {
                None
            }
        });

        if let Some(players) = final_list {
            assert_eq!(players.len(), 5, "Final player list should have 5 players");
        }
    }

    #[tokio::test]
    async fn test_heartbeat_roundtrip() {
        let (_server, port, state) = start_test_server().await;
        let room_code = create_room_http(port).await;

        let host_token = {
            let room_map = state.room_map.lock().await;
            room_map
                .get(&room_code)
                .expect("Could not find room")
                .host_token
                .clone()
        };
        let mut _host_ws =
            connect_ws_client(port, &room_code, &format!("?token={}", host_token)).await;
        let (mut player_ws, player_id) = add_player(port, &room_code, "AJ").await;

        let url = format!("http://127.0.0.1:{}/api/v1/rooms/{}/cpr", port, room_code);
        let _response = reqwest::get(&url).await.expect("CPR request failed");

        let player_msgs = recv_msgs(&mut player_ws).await;

        let do_heartbeat = player_msgs.iter().find_map(|m| {
            if let GameEvent::DoHeartbeat { hbid, t_sent } = m {
                Some((*hbid, *t_sent))
            } else {
                None
            }
        });

        assert!(do_heartbeat.is_some(), "Player should receive DoHeartbeat");

        let (hbid, t_sent) = do_heartbeat.expect("Could not do heartbeat");

        let t_dohb_recv = PlayerEntry::time_ms();
        let got_msgs = send_cmd_and_recv_all(
            &mut player_ws,
            &GameCommand::Heartbeat { hbid, t_dohb_recv },
        )
        .await;

        let got_heartbeat = got_msgs
            .iter()
            .any(|m| matches!(m, GameEvent::GotHeartbeat { hbid: id } if *id == hbid));

        assert!(got_heartbeat, "Player should receive GotHeartbeat");

        let t_lat = PlayerEntry::time_ms() - t_sent;
        send_cmd_and_recv_all(
            &mut player_ws,
            &GameCommand::LatencyOfHeartbeat { hbid, t_lat },
        )
        .await;

        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
        {
            let room_map = state.room_map.lock().await;
            let room = room_map.get(&room_code).expect("Could not get room");
            let player = room
                .players
                .iter()
                .find(|p| p.player.pid == player_id)
                .expect("Could not find player");

            let latency = player.latency().expect("Could not get latency");
            assert!(latency > 0, "Latency should be recorded");
        }
    }

    #[tokio::test]
    async fn test_heartbeat_with_invalid_hbid() {
        let (_server, port, state) = start_test_server().await;
        let room_code = create_room_http(port).await;

        let host_token = {
            let room_map = state.room_map.lock().await;
            room_map
                .get(&room_code)
                .expect("Could not get room")
                .host_token
                .clone()
        };
        let mut _host_ws =
            connect_ws_client(port, &room_code, &format!("?token={}", host_token)).await;

        let (mut player_ws, player_id) = add_player(port, &room_code, "AJ").await;

        let invalid_hbid = 99999;
        let t_dohb_recv = PlayerEntry::time_ms();
        send_cmd_and_recv_all(
            &mut player_ws,
            &GameCommand::Heartbeat {
                hbid: invalid_hbid,
                t_dohb_recv,
            },
        )
        .await;

        let got_msgs = recv_msgs(&mut player_ws).await;
        let got_heartbeat = got_msgs
            .iter()
            .any(|m| matches!(m, GameEvent::GotHeartbeat { .. }));

        assert!(
            !got_heartbeat,
            "Should not receive GotHeartbeat for invalid hbid"
        );

        // Latency should remain 0
        {
            let room_map = state.room_map.lock().await;
            let room = room_map.get(&room_code).expect("Could not get room");
            let player = room
                .players
                .iter()
                .find(|p| p.player.pid == player_id)
                .expect("Could not find player");
            assert_eq!(
                player.latency().expect("Could not get latency"),
                0,
                "Latency should remain 0 with invalid hbid"
            );
        }
    }
}

mod room_cleanup {
    use std::sync::Arc;

    use madhacks2025::{
        AppState, cleanup_inactive_rooms,
        game::room::Room,
        net::connection::{HostToken, RoomCode},
    };

    use super::*;

    #[tokio::test]
    async fn test_active_room_not_cleaned_up() {
        let state = Arc::new(AppState::with_ttl(Duration::from_secs(60)));
        let mut room_map = state.room_map.lock().await;

        let room = Room::new(RoomCode::from("TEST01".to_string()), HostToken::generate());
        room_map.insert("TEST01".to_string(), room);
        drop(room_map);

        cleanup_inactive_rooms(&state).await;

        let room_map = state.room_map.lock().await;
        assert!(
            room_map.contains_key("TEST01"),
            "Active room should not be removed"
        );
    }

    #[tokio::test]
    async fn test_inactive_room_cleaned_up() {
        let state = Arc::new(AppState::with_ttl(Duration::from_millis(100)));
        let mut room_map = state.room_map.lock().await;

        let room = Room::new(RoomCode::from("TEST01".to_string()), HostToken::generate());
        room_map.insert("TEST01".to_string(), room);
        drop(room_map);

        // Total time waited: 1s
        tokio::time::sleep(Duration::from_secs(1)).await;
        cleanup_inactive_rooms(&state).await;

        let room_map = state.room_map.lock().await;
        assert!(
            !room_map.contains_key("TEST01"),
            "Inactive room should be removed"
        );
    }

    #[tokio::test]
    async fn test_touch_extends_room_lifetime() {
        let state = Arc::new(AppState::with_ttl(Duration::from_millis(100)));
        let mut room_map = state.room_map.lock().await;

        let room = Room::new(RoomCode::from("TEST01".to_string()), HostToken::generate());
        room_map.insert("TEST01".to_string(), room);
        drop(room_map);

        tokio::time::sleep(Duration::from_millis(80)).await;

        let mut room_map = state.room_map.lock().await;
        room_map
            .get_mut("TEST01")
            .expect("TEST01 should be in room map")
            .touch();
        drop(room_map);

        // Total time waited: 160ms
        tokio::time::sleep(Duration::from_millis(80)).await;

        cleanup_inactive_rooms(&state).await;

        let room_map = state.room_map.lock().await;
        assert!(
            room_map.contains_key("TEST01"),
            "Touched room should not be removed"
        );
    }

    #[tokio::test]
    async fn test_cleanup_only_inactive_rooms() {
        let state = Arc::new(AppState::with_ttl(Duration::from_millis(150)));
        let mut room_map = state.room_map.lock().await;

        room_map.insert(
            "ACTIVE".to_string(),
            Room::new(RoomCode::from("ACTIVE".to_string()), HostToken::generate()),
        );
        room_map.insert(
            "STALE1".to_string(),
            Room::new(RoomCode::from("STALE1".to_string()), HostToken::generate()),
        );

        // Wait a bit to allow STALE1 to expire before ACTIVE
        tokio::time::sleep(Duration::from_millis(100)).await;
        room_map
            .get_mut("ACTIVE")
            .expect("ACTIVE should be in room map")
            .touch();
        drop(room_map);

        // Wait for STALE1 to expire
        tokio::time::sleep(Duration::from_millis(100)).await;

        cleanup_inactive_rooms(&state).await;

        let room_map = state.room_map.lock().await;
        assert!(room_map.contains_key("ACTIVE"));
        assert!(!room_map.contains_key("STALE1"));
    }
}
