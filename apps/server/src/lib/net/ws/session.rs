use std::sync::Arc;

use anyhow::anyhow;

use crate::{
    AppState, HostEntry, Player, PlayerId,
    api::{
        handlers::{AuthenticatedUser, WsQuery, perform_handshake},
        messages::{GameEvent, Sender},
    },
    game::{GameState, room::Room},
    net::connection::{EventSender, PlayerEntry, PlayerToken, send_event},
};

/// Performs authentication and sets up the session for a WebsocketConnection.
/// Returns who the connection's commands come from after successful authentication.
pub async fn setup_session(
    state: &Arc<AppState>,
    code: &str,
    query: &WsQuery,
    tx: EventSender,
) -> anyhow::Result<Sender> {
    let auth = {
        let room_map = state.room_map.lock().await;
        let room = room_map
            .get(code)
            .ok_or(anyhow!("Room {} not found", code))?;
        perform_handshake(room, query)?
    };

    let sender = {
        let mut room_map = state.room_map.lock().await;
        let room = room_map
            .get_mut(code)
            .ok_or(anyhow!("Room {} not found", code))?;

        match auth {
            AuthenticatedUser::Host => {
                register_host(room, tx);
                Sender::Host
            }
            AuthenticatedUser::ExistingPlayer { pid } => {
                Sender::Player(reconnect_player(room, pid, tx)?)
            }
            AuthenticatedUser::NewPlayer { name } => {
                Sender::Player(register_new_player(room, name, tx))
            }
        }
    };

    Ok(sender)
}

/// Registers a host connection
fn register_host(room: &mut Room, tx: EventSender) {
    tracing::info!("Registering host for room {}", room.code);
    room.host = Some(HostEntry::new(0, tx.clone()));

    let player_list = GameEvent::PlayerList {
        players: room.players.iter().map(|e| e.player.clone()).collect(),
    };
    tracing::info!(
        "Sending player list to host: {} players",
        room.players.len()
    );
    send_event(&tx, player_list);

    if room.state != GameState::Start {
        send_event(&tx, room.build_game_state_msg());
    }
}

/// Reconnects an existing player
fn reconnect_player(room: &mut Room, pid: PlayerId, tx: EventSender) -> anyhow::Result<PlayerId> {
    let p = room
        .players
        .iter_mut()
        .find(|p| p.player.pid == pid)
        .ok_or(anyhow!("Player {} not found", pid))?;

    p.sender = tx.clone();

    send_player_view(room, pid, &tx);
    Ok(pid)
}

/// Registers a new player to the room
fn register_new_player(room: &mut Room, name: String, tx: EventSender) -> PlayerId {
    tracing::info!("Registering new player '{}' in room {}", name, room.code);
    let new_id = (room.players.len() + 1) as u32;
    let token = PlayerToken::generate();
    let player = PlayerEntry::new(
        Player::new(new_id, name, 0, false, token.clone()),
        tx.clone(),
    );
    tracing::info!(
        "Notifying host of new player {} ({} existing players)",
        &player.player.pid,
        room.players.len()
    );
    room.add_player(player);

    send_event(&tx, GameEvent::NewPlayer { pid: new_id, token });
    send_player_view(room, new_id, &tx);

    if let Some(host) = &room.host {
        if room.state != GameState::Start {
            send_event(&host.sender, room.build_game_state_msg());
        }
        send_player_list_to_host(host, &room.players);
    }

    new_id
}

/// Sends a player the public state once the game has started, then their own state. This
/// is the same order as a broadcast, so the player's own state wins.
fn send_player_view(room: &Room, pid: PlayerId, tx: &EventSender) {
    if room.state != GameState::Start {
        send_event(tx, room.build_public_state_msg());
    }
    if let Some(player_state) = room.build_player_state_msg(pid) {
        send_event(tx, player_state);
    }
}

/// Sends the current player list to the host
pub fn send_player_list_to_host(host: &HostEntry, players: &[PlayerEntry]) {
    let list: Vec<Player> = players.iter().map(|entry| entry.player.clone()).collect();
    send_event(&host.sender, GameEvent::PlayerList { players: list });
}
