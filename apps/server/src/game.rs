use crate::{
    PlayerEntry,
    host::HostEntry,
    player::Player,
    ws_msg::{WsMsg, WsMsgChannel},
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
        let player = Player::new(pid, name);
        self.players.push(PlayerEntry::new(player, channel));
    }

    pub fn update(&mut self, msg: &WsMsg) {
        match msg {
            WsMsg::Witness { msg } => {
                send_all(&self.players, msg);
            }
            WsMsg::PlayerList { .. } => {
                self.update(msg);
            }
            WsMsg::StartGame => {
                send_all(&self.players, msg);
                self.state = GameState::Selection;
            }
            WsMsg::EndGame => {
                send_all(&self.players, msg);
                self.state = GameState::GameEnd;
            }
            // After host is done reading
            WsMsg::BuzzEnable => {
                send_all(&self.players, msg);
                // prolly start timer
                self.state = GameState::AwaitingBuzz;
            }
            WsMsg::BuzzDisable => todo!(),
            WsMsg::Buzz => todo!(),
            WsMsg::DoHeartbeat { hbid, t_sent } => todo!(),
            WsMsg::Heartbeat { hbid } => todo!(),
            WsMsg::GotHeartbeat { hbid } => todo!(),
            WsMsg::LatencyOfHeartbeat { hbid, t_lat } => todo!(),
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
    Answer,
    AwaitingBuzz,
    GameEnd,
}

impl Default for GameState {
    fn default() -> Self {
        Self::Start
    }
}
