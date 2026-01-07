use std::sync::Arc;

use anyhow::anyhow;
use tokio_mpmc::Sender;

use crate::{
    api::{
        handlers::{perform_handshake, AuthenticatedUser, WsQuery},
        messages::GameEvent,
    }, game::{room::Room, GameState}, net::connection::{PlayerEntry, PlayerToken}, AppState, HostEntry, Player, PlayerId
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
    room.host = Some(HostEntry::new(0, tx.clone()));

    let player_list =
        GameEvent::PlayerList(room.players.iter().map(|e| e.player.clone()).collect());
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

    if room.state != GameState::Start {
        let can_buzz = room.state == GameState::WaitingForBuzz && !p.player.buzzed;
        let player_state = GameEvent::PlayerState {
            pid: p.player.pid,
            buzzed: p.player.buzzed,
            score: p.player.score,
            can_buzz,
        };
        let _ = tx.send(player_state).await;
    }
    Ok(pid)
}

/// Registers a new player to the room
async fn register_new_player(
    room: &mut Room,
    name: String,
    tx: Sender<GameEvent>,
) -> anyhow::Result<PlayerId> {
    let new_id = (room.players.len() + 1) as u32;
    let token = PlayerToken::generate();
    let player = PlayerEntry::new(Player::new(new_id, name, 0, false, token.clone()), tx.clone());
    room.players.push(player);

    tx.send(GameEvent::NewPlayer { pid: new_id, token }).await?;

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
