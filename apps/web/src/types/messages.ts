import type { GamePhase, GameState, Player, PlayerId } from "./game";

export type PlayerToken = string;

/** The wagers a player may make, inclusive */
export interface WagerRange {
  min: number;
  max: number;
}

/**
 * Events sent from the server to clients.
 */
export type GameEvent =
  | { type: "Witness"; msg: GameEvent }
  | { type: "DoHeartbeat"; hbid: number; t_sent: number }
  | { type: "GotHeartbeat"; hbid: number }
  | { type: "PlayerList"; players: Player[] }
  | { type: "NewPlayer"; pid: PlayerId; token: PlayerToken }
  // Sent to the host only, since it contains every clue's answer
  | ({ type: "GameState" } & GameState)
  // Sent to players instead of GameState
  | {
      type: "PublicState";
      state: GamePhase;
      round: number;
      picker: PlayerId | null;
    }
  // Sent to one player only
  | {
      type: "PlayerState";
      pid: PlayerId;
      buzzed: boolean;
      score: number;
      canBuzz: boolean;
      /** Set while this player must make a wager, which is accepted in this range */
      wagerRange: WagerRange | null;
      /** This player's wager on the clue being played, or on Final Jeopardy */
      wager: number | null;
      /**
       * What this player is wagering or answering on: the Daily Double's category, the
       * Final Jeopardy category while wagering, or the Final Jeopardy clue after that
       */
      prompt: string | null;
      /** Whether this player may write a response to the Final Jeopardy clue now */
      canAnswer: boolean;
      /** This player's own Final Jeopardy response, once they've sent one */
      answer: string | null;
      /** How the host judged this player's own Final Jeopardy response, once they have */
      correct: boolean | null;
    }
  // Sent to one player when a command of theirs was refused, such as a wager out of range
  | { type: "Rejected"; reason: string }
  | { type: "PlayerBuzzed"; pid: PlayerId; name: string };

/**
 * Events sent from clients to the server.
 */
export type GameCommand =
  | { type: "StartGame" }
  | { type: "EndGame" }
  | { type: "Buzz"; reactionMs: number }
  | { type: "HostReady" }
  | { type: "HostChoice"; categoryIndex: number; questionIndex: number }
  | { type: "HostChecked"; correct: boolean }
  | { type: "HostSkip" }
  | { type: "HostContinue" }
  // `round` is the round being ended, so a repeated click can't also end the next one
  | { type: "HostNextRound"; round: number }
  | { type: "HostSetPicker"; pid: PlayerId }
  | { type: "SubmitWager"; amount: number }
  // Can be resent to change the response until the host locks them
  | { type: "SubmitAnswer"; answer: string }
  | { type: "HostRevealFinalClue" }
  | { type: "HostLockFinalAnswers" }
  // `pid` is the player being revealed, so a repeated click can't reveal the next one
  | { type: "HostRevealFinalResponse"; pid: PlayerId }
  | { type: "Heartbeat"; hbid: number; tDohbRecv: number }
  | { type: "LatencyOfHeartbeat"; hbid: number; tLat: number };
