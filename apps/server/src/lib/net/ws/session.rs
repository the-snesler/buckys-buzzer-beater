use std::sync::Arc;

use anyhow::anyhow;
use tokio_mpmc::Sender;

use crate::{
    AppState, HostEntry, Player, PlayerId,
    api::{
        handlers::{AuthenticatedUser, WsQuery, perform_handshake},
        messages::GameEvent,
    },
    game::{GameState, room::Room},
    net::connection::{PlayerEntry, PlayerToken},
};

/// Performs authentication and sets up the session for a WebsocketConnection.
/// Returns the player ID after successful authentication.
pub async fn setup_session(
    state: &Arc<AppState>,
    code: &str,
    query: &WsQuery,
    tx: Sender<GameEvent>,
) -> anyhow::Result<PlayerId> {
    let auth = {
        let room_map = state.room_map.lock().await;
        let room = room_map
            .get(code)
            .ok_or(anyhow!("Room {} not found", code))?;
        perform_handshake(room, query)?
    };

    let player_id = {
        let mut room_map = state.room_map.lock().await;
        let room = room_map
            .get_mut(code)
            .ok_or(anyhow!("Room {} not found", code))?;

        match auth {
            AuthenticatedUser::Host => register_host(room, tx).await?,
            AuthenticatedUser::ExistingPlayer { pid } => reconnect_player(room, pid, tx).await?,
            AuthenticatedUser::NewPlayer { name } => register_new_player(room, name, tx).await?,
        }
    };

    Ok(player_id)
}

/// Registers a host connection
async fn register_host(room: &mut Room, tx: Sender<GameEvent>) -> anyhow::Result<PlayerId> {
    tracing::info!("Registering host for room {}", room.code);
    room.host = Some(HostEntry::new(0, tx.clone()));

    let player_list =
        GameEvent::PlayerList(room.players.iter().map(|e| e.player.clone()).collect());
    tracing::info!("Sending player list to host: {} players", room.players.len());
    let _ = tx.send(player_list).await;

    if room.state != GameState::Start {
        let _ = tx.send(room.build_game_state_msg()).await;
    }

    Ok(0)
}

/// Reconnects an existing player
async fn reconnect_player(
    room: &mut Room,
    pid: PlayerId,
    tx: Sender<GameEvent>,
) -> anyhow::Result<PlayerId> {
    let p = room
        .players
        .iter_mut()
        .find(|p| p.player.pid == pid)
        .ok_or(anyhow!("Player {} not found", pid))?;

    p.sender = tx.clone();

    // Always send player state on reconnect
    let can_buzz = room.state == GameState::WaitingForBuzz && !p.player.buzzed;
    let player_state = GameEvent::PlayerState {
        pid: p.player.pid,
        buzzed: p.player.buzzed,
        score: p.player.score,
        can_buzz,
    };
    let _ = tx.send(player_state).await;

    // Also send game state if game has started
    if room.state != GameState::Start {
        let _ = tx.send(room.build_game_state_msg()).await;
    }
    Ok(pid)
}

/// Registers a new player to the room
async fn register_new_player(
    room: &mut Room,
    name: String,
    tx: Sender<GameEvent>,
) -> anyhow::Result<PlayerId> {
    tracing::info!("Registering new player '{}' in room {}", name, room.code);
    let new_id = (room.players.len() + 1) as u32;
    let token = PlayerToken::generate();
    let player = PlayerEntry::new(
        Player::new(new_id, name, 0, false, token.clone()),
        tx.clone(),
    );
    tracing::info!("Broadcasting new player {} to {} existing players and host", &player.player.pid, room.players.len());
    room.players.push(player);


    tx.send(GameEvent::NewPlayer { pid: new_id, token }).await?;
    let can_buzz = room.state == GameState::WaitingForBuzz;
    let player_state = GameEvent::PlayerState {
        pid: new_id,
        buzzed: false,
        score: 0,
        can_buzz,
    };
    let _ = tx.send(player_state).await;

    if room.state != GameState::Start {
        let _ = tx.send(room.build_game_state_msg()).await;

        let game_state = room.build_game_state_msg();
        if let Some(host) = &room.host {
            let _ = host.sender.send(game_state.clone()).await;
        }
        for player in &room.players {
            if player.player.pid != new_id {
                let _ = player.sender.send(game_state.clone()).await;
            }
        }
    } else {
        if let Some(host) = &room.host {
            let _ = send_player_list_to_host(host, &room.players).await;
        }
    }

    if let Some(host) = &room.host {
        let _ = send_player_list_to_host(host, &room.players).await;
    }

    Ok(new_id)
}

/// Sends the current player list to the host
pub async fn send_player_list_to_host(
    host: &HostEntry,
    players: &[PlayerEntry],
) -> anyhow::Result<()> {
    let list: Vec<Player> = players.iter().map(|entry| entry.player.clone()).collect();
    let msg = GameEvent::PlayerList(list);
    host.sender.send(msg).await?;
    Ok(())
}
