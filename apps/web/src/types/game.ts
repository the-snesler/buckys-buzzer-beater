export type PlayerId = number;

/** The phase the game is in */
export type GamePhase =
  | "start"
  | "selection"
  | "questionReading"
  /** The picker chose a Daily Double and is deciding how much to wager */
  | "dailyDoubleWager"
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
  /** The current round's board */
  categories: Category[];
  /** Zero-based index of the current round */
  round: number;
  roundCount: number;
  players: Player[];
  currentQuestion: [number, number] | null;
  currentBuzzer: PlayerId | null;
  /** The player who chooses the next clue. Null until the host chooses who picks first. */
  picker: PlayerId | null;
  /** The picker's wager on the Daily Double being played, once they've made it */
  ddWager: number | null;
  winner: PlayerId | null;
}
