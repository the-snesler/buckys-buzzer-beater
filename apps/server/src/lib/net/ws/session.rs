use std::sync::Arc;

use anyhow::anyhow;

use crate::{
    AppState, HostEntry, Player, PlayerId,
    api::{
        handlers::{AuthenticatedUser, WsQuery, perform_handshake},
        messages::{GameEvent, Sender},
    },
    game::{GameState, room::Room},
    net::connection::{PlayerEntry, PlayerToken},
};

/// Performs authentication and sets up the session for a WebsocketConnection.
/// Returns who the connection's commands come from after successful authentication.
pub async fn setup_session(
    state: &Arc<AppState>,
    code: &str,
    query: &WsQuery,
    tx: tokio_mpmc::Sender<GameEvent>,
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
                register_host(room, tx).await?;
                Sender::Host
            }
            AuthenticatedUser::ExistingPlayer { pid } => {
                Sender::Player(reconnect_player(room, pid, tx).await?)
            }
            AuthenticatedUser::NewPlayer { name } => {
                Sender::Player(register_new_player(room, name, tx).await?)
            }
        }
    };

    Ok(sender)
}

/// Registers a host connection
async fn register_host(room: &mut Room, tx: tokio_mpmc::Sender<GameEvent>) -> anyhow::Result<()> {
    tracing::info!("Registering host for room {}", room.code);
    room.host = Some(HostEntry::new(0, tx.clone()));

    let player_list = GameEvent::PlayerList {
        players: room.players.iter().map(|e| e.player.clone()).collect(),
    };
    tracing::info!(
        "Sending player list to host: {} players",
        room.players.len()
    );
    let _ = tx.send(player_list).await;

    if room.state != GameState::Start {
        let _ = tx.send(room.build_game_state_msg()).await;
    }

    Ok(())
}

/// Reconnects an existing player
async fn reconnect_player(
    room: &mut Room,
    pid: PlayerId,
    tx: tokio_mpmc::Sender<GameEvent>,
) -> anyhow::Result<PlayerId> {
    let p = room
        .players
        .iter_mut()
        .find(|p| p.player.pid == pid)
        .ok_or(anyhow!("Player {} not found", pid))?;

    p.sender = tx.clone();

    send_player_view(room, pid, &tx).await;
    Ok(pid)
}

/// Registers a new player to the room
async fn register_new_player(
    room: &mut Room,
    name: String,
    tx: tokio_mpmc::Sender<GameEvent>,
) -> anyhow::Result<PlayerId> {
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
    room.players.push(player);

    tx.send(GameEvent::NewPlayer { pid: new_id, token }).await?;
    send_player_view(room, new_id, &tx).await;

    if let Some(host) = &room.host {
        if room.state != GameState::Start {
            let _ = host.sender.send(room.build_game_state_msg()).await;
        }
        let _ = send_player_list_to_host(host, &room.players).await;
    }

    Ok(new_id)
}

/// Sends a player their own state and, once the game has started, the public state
async fn send_player_view(room: &Room, pid: PlayerId, tx: &tokio_mpmc::Sender<GameEvent>) {
    if let Some(player_state) = room.build_player_state_msg(pid) {
        let _ = tx.send(player_state).await;
    }
    if room.state != GameState::Start {
        let _ = tx.send(room.build_public_state_msg()).await;
    }
}

/// Sends the current player list to the host
pub async fn send_player_list_to_host(
    host: &HostEntry,
    players: &[PlayerEntry],
) -> anyhow::Result<()> {
    let list: Vec<Player> = players.iter().map(|entry| entry.player.clone()).collect();
    let msg = GameEvent::PlayerList { players: list };
    host.sender.send(msg).await?;
    Ok(())
}
