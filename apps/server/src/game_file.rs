use serde::Deserialize;

use crate::game::{Category, Question};


#[derive(Deserialize)]
struct GameFileClue {
    value: u32,
    clue: String,
    solution: String,
}

#[derive(Deserialize)]
struct GameFileCategory {
    category: String,
    clues: Vec<GameFileClue>,
}

#[derive(Deserialize)]
struct GameFile {
    game: GameFileGame,
}

#[derive(Deserialize)]
struct GameFileGame {
    single: Vec<GameFileCategory>,
}

impl From<GameFileCategory> for Category {
    fn from(gfc: GameFileCategory) -> Self {
        Category {
            title: gfc.category,
            questions: gfc.clues.into_iter().map(|c| Question {
                question: c.clue,
                answer: c.solution,
                value: c.value,
                answered: false,
            }).collect(),
        }
    }
}
