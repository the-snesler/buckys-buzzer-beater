#[derive(Default, Clone)]
struct Game {
    state: GameState,
    host: Host,
    players: Vec<Player>,
    questions: Vec<String>,
}

#[derive(Clone, Default)]
struct Host {
    id: i64,
}

#[derive(Clone, Default)]
struct Player {
    id: i64,
    name: String,
}

impl Game {
    pub fn add_player(&mut self, id: i64, name: String) {
        self.players.push(Player { id, name });
    }
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
