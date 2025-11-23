use anyhow::anyhow;
use serde::{Deserialize, Serialize};
use tokio_mpmc::Sender;

use crate::{
    PlayerEntry,
    host::HostEntry,
    player::{Player, PlayerId},
    ws_msg::{WsMsg, WsMsgChannel},
};

pub struct Room {
    pub code: String,
    pub host_token: String,
    pub state: GameState,
    pub host: Option<HostEntry>,
    pub players: Vec<PlayerEntry>,
    pub questions: Vec<String>,
}

impl Room {
    pub fn new(code: String, host_token: String) -> Self {
        Self {
            code,
            host_token,
            state: GameState::default(),
            host: None,
            players: Vec::new(),
            questions: Vec::new(),
        }
    }

    pub fn code(&self) -> &str {
        &self.code
    }

    pub fn host_token(&self) -> &str {
        &self.host_token
    }

    pub fn set_host(&mut self, host: HostEntry) {
        self.host = Some(host);
    }

    pub fn verify_host_token(&self, token: &str) -> bool {
        self.host_token == token
    }
}

impl Room {
    pub fn add_player(&mut self, pid: u32, name: String, sender: Sender<WsMsg>) {
        let player = Player::new(pid, name, 0, false);
        self.players.push(PlayerEntry::new(player, sender));
    }

    pub async fn update(&mut self, msg: &WsMsg, pid: Option<PlayerId>) -> anyhow::Result<()> {
        let own_entry: Option<&mut PlayerEntry> = if let Some(pid) = pid {
            let idx = self.players.iter().position(|p| p.player.pid == pid);
            idx.map(|i| &mut self.players[i])
        } else {
            None
        };
        match msg {
            WsMsg::PlayerList { .. } => {
                if let Some(host) = &self.host {
                    host.sender.send(msg.clone()).await?;
                }
            }
            WsMsg::HostChecked { correct } => {
                match correct {
                    // if false, have all players buzzed
                    false => {
                        match self.players.iter().all(|player| !player.did_buzz()) {
                            // if true, do questions remain?
                            true => match self.questions.len() {
                                // if false, game end
                                0 => self.state = GameState::GameEnd,
                                // if true, selection
                                _ => self.state = GameState::Selection,
                            },
                            // if false, wait for buzz
                            false => self.state = GameState::AwaitingBuzz,
                        }
                    }
                    // if true, do questions remain?
                    true => {
                        match self.questions.len() {
                            // if false, end game
                            0 => self.state = GameState::GameEnd,
                            // if true, selection
                            _ => self.state = GameState::Selection,
                        }
                    }
                }
            }
            WsMsg::StartGame => {
                self.state = GameState::Selection;
            }
            WsMsg::EndGame => {
                self.state = GameState::GameEnd;
            }
            // After host is done reading
            WsMsg::BuzzEnable => {
                // prolly start timer
                self.state = GameState::AwaitingBuzz;
            }
            WsMsg::BuzzDisable => todo!(),
            WsMsg::Buzz => {
                self.state = GameState::Answer(pid);
            }
            WsMsg::Heartbeat { hbid, t_dohb_recv } => {
                if let Some(entry) = own_entry {
                    if !entry.on_know_dohb_recv(*hbid, *t_dohb_recv) {
                        println!("WARN: failed to update DoHeartbeat recv time")
                    }
                } else {
                    println!("WARN: own entry missing handling Heartbeat, continuing anyway")
                }
            }
            WsMsg::LatencyOfHeartbeat { hbid, t_lat } => {
                if let Some(entry) = own_entry {
                    if !entry.on_latencyhb(
                        *hbid,
                        (*t_lat).try_into().expect(
                            "LatencyOfHeartbeat latency of heartbeat exceeds 32-bit integer limit",
                        ),
                    ) {
                        println!(
                            "WARN: handling LatencyOfHeartbeat failed to update latencies, continuing anyway"
                        );
                    }
                } else {
                    println!(
                        "WARN: own entry missing while handling LatencyOfHeartbeat, continuing anyway"
                    );
                }
            }
            _ => {}
        }
        Ok(())
    }
}

async fn send_all(players: &[PlayerEntry], msg: &WsMsg) {
    players.iter().for_each(|player| {
        player.update(msg);
    });
}

#[derive(Clone, Deserialize, Serialize)]
enum GameState {
    Start,
    Selection,
    QuestionReading,
    Answer(Option<PlayerId>),
    AwaitingBuzz,
    GameEnd,
}

impl Default for GameState {
    fn default() -> Self {
        Self::Start
    }
}
