import type { ParsedGame } from "../lib/gameFile";

/** Navigation state the lobby reads, set by the Game Builder when it hands over a game */
export interface LobbyLocationState {
  game?: ParsedGame;
  fromBuilder?: boolean;
}

/** Navigation state the Game Builder reads, set by the lobby to edit the game it loaded */
export interface BuilderLocationState {
  game?: ParsedGame;
}
