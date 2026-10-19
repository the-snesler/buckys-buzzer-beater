import type { ParsedGame } from "../lib/gameFile";

/** Navigation state the lobby reads, set by the Game Builder when it hands over a game */
export interface LobbyLocationState {
  game?: ParsedGame;
  fromBuilder?: boolean;
}
