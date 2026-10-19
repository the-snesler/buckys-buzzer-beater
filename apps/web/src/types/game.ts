export type PlayerId = number;

/** The phase the game is in */
export type GamePhase =
  | "start"
  | "selection"
  | "questionReading"
  | "answer"
  | "answerReveal"
  | "waitingForBuzz"
  | "gameEnd";

export interface Question {
  question: string;
  answer: string;
  value: number;
  answered: boolean;
}

export interface Category {
  title: string;
  questions: Question[];
}

export interface Player {
  pid: PlayerId;
  name: string;
  score: number;
  buzzed: boolean;
}

/**
 * The full game state. The server only sends it to the host, which forwards it to the
 * display. Players get PublicState instead, since this contains every answer.
 */
export interface GameState {
  state: GamePhase;
  categories: Category[];
  players: Player[];
  currentQuestion: [number, number] | null;
  currentBuzzer: PlayerId | null;
  winner: PlayerId | null;
}
