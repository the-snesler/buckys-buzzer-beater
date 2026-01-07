refactor notes

# cleanup monolith

1. new files and modules

api/
    mod.rs
    routes.rs
    handlers.rs
game/
    mod.rs
    room.rs
    models.rs
    state.rs
net/
    mod.rs
    connection.rs
lib.rs
main.rs

2. cleanup lib.rs
 - create fn perform_handshake inside handlers.rs by copying the big if/else block that checks tokens and room codes in ws_websocket_handler
 - move create_room, cpr_handler, etc to routes.rs

3. ws_socket_handler
 - call perform_handshake at top
 - remove logic for generating tokens and adds players (now belongs to handshake function)
 - now look like Handshake -> Loop { Select { Recv -> Handle } }

# refactor messages

1. split messages ws_msg.rs
 - create api/messages.rs
 - enum GameCommand { inputs: buzz, startgame }
 - enum GameEvent { outputs: playerbuzzed, gamestate }
 - update room::handle_message to accept gamecommand instead of WsMsg

2. refactor playerentry
 - in connection.rs, remove times_doheartbeat map
 - add latency_ms: u32 and last_ping_sent: Option<u64>
 - implement PING/PONG
    - server sends Ping -> Stores timestamp
    - Client replies Pong -> Server does (Now - Timestamp / 2)

3. implement fairness buffer
 - in room.rs, add buzz_candidates: Vec and buzz_window_end: Option<SystemTime>
 - update handle_buzz(Buzz)
    - do not declare winner immediately
    - push player + current timestamp + their latency into buzz_candidates
    - start the buzz_window_end timer
 - add function Room::tick()
    - check if buzz_window_end has passed
    - if yes, sort candidates by Arrival - Latency
    - declare winner and broadcast GameEvent::Buzzed

4. Update game loop
 - update ws_socket_handler main loop by adding a tick branch to the select!


