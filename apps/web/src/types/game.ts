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
  /** Final Jeopardy: everyone sees the category and wagers */
  | "finalWager"
  /** Final Jeopardy: everyone sees the clue and writes a response */
  | "finalAnswer"
  /** Final Jeopardy: the host reveals and judges each response in turn */
  | "finalReveal"
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

/** One player's Final Jeopardy wager and response */
export interface FinalEntry {
  pid: PlayerId;
  wager: number | null;
  answer: string | null;
  /** How the host judged the response, once they have */
  correct: boolean | null;
  /** Whether the response has been shown to everyone */
  revealed: boolean;
}

/**
 * Final Jeopardy as the host sees it. The display gets a copy with everything that
 * hasn't been revealed yet removed, see displayGameState in Host.tsx.
 */
export interface FinalState {
  category: string;
  question: string;
  answer: string;
  /** Every player in Final Jeopardy: by pid until the responses are locked, then in reveal order */
  entries: FinalEntry[];
  /** Index into entries of the next response to reveal or judge; entries.length once all are judged */
  revealIndex: number;
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
  /** Whether the game ends with Final Jeopardy */
  hasFinal: boolean;
  /** Final Jeopardy, from when it starts */
  final: FinalState | null;
  winner: PlayerId | null;
}
