mod common;

use std::time::Duration;

use tokio::time::sleep;

use common::*;
use madhacks2025::{GameState, PlayerEntry, ws_msg::WsMsg};

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
            .find(|m| matches!(m, WsMsg::PlayerList { .. }));

        if let Some(WsMsg::PlayerList(players)) = player_list_msg {
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
        let room = room_map.get(&room_code).expect("Could not find room");
        assert_eq!(room.players.len(), 3);
    }

    #[tokio::test]
    async fn test_game_flow_start_to_buzz() {
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

        let (mut player_ws, player_id) = add_player(port, &room_code, "AJ").await;
        let _ = recv_msgs(&mut host_ws).await; // Consume host update

        start_game(&mut host_ws, &mut [&mut player_ws]).await;

        let start_msgs = send_msg_and_recv_all(&mut host_ws, &WsMsg::StartGame {}).await;
        println!("After StartGame, host got: {:?}", start_msgs);

        send_msg_and_recv_all(&mut host_ws, &WsMsg::HostReady {}).await;
        let player_update = recv_msgs(&mut player_ws).await;

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

        send_msg_and_recv_all(&mut player_ws, &WsMsg::Buzz {}).await;
        let host_buzz = recv_msgs(&mut host_ws).await;

        let buzz_notification = host_buzz.iter().find(|m| matches!(m, WsMsg::Buzzed { .. }));
        assert!(
            buzz_notification.is_some(),
            "Host should receive PlayerBuzzed"
        );

        if let Some(WsMsg::Buzzed { pid, .. }) = buzz_notification {
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

        let (player_ws, player_id) = add_player(port, &room_code, "AJ").await;
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

        start_game(&mut host_ws, &mut [&mut player_ws]).await;

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

        start_game(&mut host_ws, &mut [&mut player_ws]).await;

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

        let (mut player_ws, _player_id) = add_player(port, &room_code, "AJ").await;
        let _ = recv_msgs(&mut host_ws).await;

        start_game(&mut host_ws, &mut [&mut player_ws]).await;

        send_msg_and_recv_all(
            &mut host_ws,
            &WsMsg::HostChoice {
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

        start_game(&mut host_ws, &mut [&mut aj_ws, &mut sam_ws]).await;

        // Question 1: AJ buzzes and gets it correct (+100)
        play_question(&mut host_ws, &mut aj_ws, 0, 0, true).await;
        let _ = recv_msgs(&mut sam_ws).await;

        {
            let room_map = state.room_map.lock().await;
            assert_eq!(get_player_score(&room_map, &room_code, aj_id), 100);
        }

        // Question 2: Sam buzzes and gets it incorrect (-200)
        play_question(&mut host_ws, &mut sam_ws, 0, 1, false).await;
        let _ = recv_msgs(&mut aj_ws).await;

        {
            let room_map = state.room_map.lock().await;
            assert_eq!(get_player_score(&room_map, &room_code, aj_id), 100);
            assert_eq!(get_player_score(&room_map, &room_code, sam_id), -200);
        }

        // Question 2 again: AJ buzzes and gets it correct (+200 = 300 total)
        play_question(&mut host_ws, &mut aj_ws, 0, 1, true).await;
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

        start_game(&mut host_ws, &mut [&mut aj_ws, &mut sam_ws]).await;

        send_msg_and_recv_all(
            &mut host_ws,
            &WsMsg::HostChoice {
                category_index: 0,
                question_index: 0,
            },
        )
        .await;

        let _ = recv_msgs(&mut aj_ws).await;
        let _ = recv_msgs(&mut sam_ws).await;

        send_msg_and_recv_all(&mut host_ws, &WsMsg::HostReady {}).await;
        let _ = recv_msgs(&mut aj_ws).await;
        let _ = recv_msgs(&mut sam_ws).await;

        let aj_buzz = tokio::spawn({
            let mut ws = aj_ws;
            async move {
                send_msg_and_recv_all(&mut ws, &WsMsg::Buzz {}).await;
                ws
            }
        });

        let sam_buzz = tokio::spawn({
            let mut ws = sam_ws;
            async move {
                send_msg_and_recv_all(&mut ws, &WsMsg::Buzz {}).await;
                ws
            }
        });

        let _aj_ws = aj_buzz.await.expect("Could not find AJ websocket");
        let _sam_ws = sam_buzz.await.expect("Could not find Sam websocket");

        let host_msgs = recv_msgs(&mut host_ws).await;
        let buzz_count = host_msgs
            .iter()
            .filter(|m| matches!(m, WsMsg::Buzzed { .. }))
            .count();
        assert_eq!(buzz_count, 1, "Host should receive exactly one buzz");

        let buzzed_player = host_msgs
            .iter()
            .find_map(|m| {
                if let WsMsg::Buzzed { pid, .. } = m {
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
            if let WsMsg::PlayerList(players) = m {
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
            if let WsMsg::DoHeartbeat { hbid, t_sent } = m {
                Some((*hbid, *t_sent))
            } else {
                None
            }
        });

        assert!(do_heartbeat.is_some(), "Player should receive DoHeartbeat");

        let (hbid, t_sent) = do_heartbeat.expect("Could not do heartbeat");

        let t_dohb_recv = PlayerEntry::time_ms();
        let got_msgs =
            send_msg_and_recv_all(&mut player_ws, &WsMsg::Heartbeat { hbid, t_dohb_recv }).await;

        let got_heartbeat = got_msgs
            .iter()
            .any(|m| matches!(m, WsMsg::GotHeartbeat { hbid: id } if *id == hbid));

        assert!(got_heartbeat, "Player should receive GotHeartbeat");

        let t_lat = PlayerEntry::time_ms() - t_sent;
        send_msg_and_recv_all(&mut player_ws, &WsMsg::LatencyOfHeartbeat { hbid, t_lat }).await;

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
        send_msg_and_recv_all(
            &mut player_ws,
            &WsMsg::Heartbeat {
                hbid: invalid_hbid,
                t_dohb_recv,
            },
        )
        .await;

        let got_msgs = recv_msgs(&mut player_ws).await;
        let got_heartbeat = got_msgs
            .iter()
            .any(|m| matches!(m, WsMsg::GotHeartbeat { .. }));

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

    use madhacks2025::{AppState, Room, cleanup_inactive_rooms};

    use super::*;

    #[tokio::test]
    async fn test_active_room_not_cleaned_up() {
        let state = Arc::new(AppState::with_ttl(Duration::from_secs(60)));
        let mut room_map = state.room_map.lock().await;

        let room = Room::new("TEST01".to_string(), "token".to_string());
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

        let room = Room::new("TEST01".to_string(), "token".to_string());
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

        let room = Room::new("TEST01".to_string(), "token".to_string());
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
            Room::new("ACTIVE".to_string(), "t1".to_string()),
        );
        room_map.insert(
            "STALE1".to_string(),
            Room::new("STALE1".to_string(), "t2".to_string()),
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
