use crate::{
    PlayerEntry,
    host::HostEntry,
    player::Player,
    ws_msg::{WsMsg, WsMsgChannel},
};

pub struct Room {
    code: String,
    host_token: String,
    state: GameState,
    host: Option<HostEntry>,
    players: Vec<PlayerEntry>,
    questions: Vec<String>,
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
