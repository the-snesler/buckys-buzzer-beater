use crate::{
    host::HostEntry, player::{Player, PlayerId}, ws_msg::{WsMsg, WsMsgChannel}, PlayerEntry
};

pub struct Room {
    code: String,
    state: GameState,
    host: HostEntry,
    players: Vec<PlayerEntry>,
    questions: Vec<String>,
}

impl Room {
    pub fn add_player(&mut self, pid: u32, name: String, channel: WsMsgChannel) {
        let player = Player::new(pid, name, 0, false);
        self.players.push(PlayerEntry::new(player, channel));
    }

    pub fn update(&mut self, msg: &WsMsg, pid: PlayerId) {
        match msg {
            WsMsg::PlayerList { .. } => {
                self.update(msg, pid);
            }
            WsMsg::HostChecked { correct } => {
                match correct {
                    // if false, have all players buzzed
                        // if true, do questions remain?
                            // if true, selection
                            // if false, game end
                        // if false, wait for buzz
                    // if true, do questions remain?
                        // if true, selection
                        // if false, end game
                    false => {
                        match self.players.iter().all(|player| !player.did_buzz()) {
                            true => match self.questions.len() {
                                0 => self.state = GameState::GameEnd,
                                _ => self.state = GameState::Selection,
                            }
                            false => self.state = GameState::AwaitingBuzz,
                        }
                    }
                    true => {
                        match self.questions.len() {
                            0 => self.state = GameState::GameEnd,
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
            },
            WsMsg::DoHeartbeat { hbid, t_sent } => todo!(),
            WsMsg::Heartbeat { hbid } => todo!(),
            WsMsg::GotHeartbeat { hbid } => todo!(),
            WsMsg::LatencyOfHeartbeat { hbid, t_lat } => todo!(),
            _ => {}
        }
    }
}

async fn send_all(players: &[PlayerEntry], msg: &WsMsg) {
    players.iter().for_each(|player| {
        player.update(msg);
    });
}

#[derive(Clone)]
enum GameState {
    Start,
    Selection,
    QuestionReading,
    Answer(PlayerId),
    AwaitingBuzz,
    GameEnd,
}

impl Default for GameState {
    fn default() -> Self {
        Self::Start
    }
}
