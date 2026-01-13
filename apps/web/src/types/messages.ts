import { Category } from "../lib/api";

export type PlayerId = number;
export type PlayerToken = string;

export interface Player {
  pid: PlayerId;
  name: string;
  score: number;
  buzzed: boolean;
  token: PlayerToken;
}

export type GameState =
  | "start"
  | "selection"
  | "questionReading"
  | "answer"
  | "answerReveal"
  | "waitingForBuzz"
  | "gameEnd";
  
/**
 * Events sent from the server to clients.
 */
export type GameEvent =
  | { type: "Witness"; msg: GameEvent }
  | { type: "DoHeartbeat"; hbid: number; t_sent: number }
  | { type: "GotHeartbeat"; hbid: number }
  | { type: "PlayerList"; players: Player[] }
  | { type: "NewPlayer"; pid: PlayerId; token: PlayerToken }
  | {
      type: "GameState";
      state: GameState;
      categories: Category[];
      players: Player[];
      currentQuestion: [number, number] | null;
      currentBuzzer: PlayerId | null;
      winner: PlayerId | null;
    }
  | {
      type: "PlayerState";
      pid: PlayerId;
      buzzed: boolean;
      score: number;
      canBuzz: boolean;
    }
  | { type: "PlayerBuzzed"; pid: PlayerId; name: string };

/**
 * Events sent from clients to the server.
 */
export type GameCommand =
  | { type: "StartGame" }
  | { type: "EndGame" }
  | { type: "Buzz" }
  | { type: "HostReady" }
  | { type: "HostChoice"; categoryIndex: number; questionIndex: number }
  | { type: "HostChecked"; correct: boolean }
  | { type: "HostSkip" }
  | { type: "HostContinue" }
  | { type: "Heartbeat"; hbid: number; tDohbRecv: number }
  | { type: "LatencyOfHeartbeat"; hbid: number; tLat: number };
