import type { ServerWebSocket } from "bun";

// Game state
export type GameState = "lobby" | "playing" | "ended";

// Player data stored on WebSocket
export interface WSData {
  roomCode: string;
  isHost: boolean;
  playerId?: number;
  playerToken?: string;
}

// WebSocket with our custom data
export type GameWebSocket = ServerWebSocket<WSData>;

// Player representation
export interface PlayerInfo {
  pid: number;
  name: string;
  token: string;
  score: number;
  canBuzz: boolean;
  connected: boolean;
  ws: GameWebSocket | null;
}

// Room creation response
export interface CreateRoomResponse {
  room_code: string;
  host_token: string;
}

// ============================================
// WebSocket Message Types (JSON format)
// ============================================

// --- Inbound Messages (Client -> Server) ---

export interface StartGameMessage {
  StartGame: Record<string, never>;
}

export interface EndGameMessage {
  EndGame: Record<string, never>;
}

export interface BuzzEnableMessage {
  BuzzEnable: Record<string, never>;
}

export interface BuzzDisableMessage {
  BuzzDisable: Record<string, never>;
}

export interface BuzzMessage {
  Buzz: Record<string, never>;
}

export interface HostCheckedMessage {
  HostChecked: {
    correct: boolean;
  };
}

export interface HeartbeatMessage {
  Heartbeat: {
    hbid: number;
  };
}

export interface LatencyOfHeartbeatMessage {
  LatencyOfHeartbeat: {
    hbid: number;
    t_lat: number;
  };
}

export type InboundMessage =
  | StartGameMessage
  | EndGameMessage
  | BuzzEnableMessage
  | BuzzDisableMessage
  | BuzzMessage
  | HostCheckedMessage
  | HeartbeatMessage
  | LatencyOfHeartbeatMessage;

// --- Outbound Messages (Server -> Client) ---

export interface WitnessMessage {
  Witness: {
    pid: number;
    msg: string;
  };
}

export interface NewPlayerMessage {
  NewPlayer: {
    pid: number;
    token: string;
  };
}

export interface PlayerListMessage {
  PlayerList: Array<{
    pid: number;
    name: string;
    score: number;
  }>;
}

export interface DoHeartbeatMessage {
  DoHeartbeat: {
    hbid: number;
    t_sent: number;
  };
}

export interface GotHeartbeatMessage {
  GotHeartbeat: {
    hbid: number;
  };
}

export interface BuzzEnabledMessage {
  BuzzEnabled: Record<string, never>;
}

export interface BuzzDisabledMessage {
  BuzzDisabled: Record<string, never>;
}

export interface GameStartedMessage {
  GameStarted: Record<string, never>;
}

export interface GameEndedMessage {
  GameEnded: Record<string, never>;
}

export interface BuzzedMessage {
  Buzzed: {
    pid: number;
    name: string;
  };
}

export interface AnswerResultMessage {
  AnswerResult: {
    pid: number;
    correct: boolean;
    newScore: number;
  };
}

export type OutboundMessage =
  | WitnessMessage
  | NewPlayerMessage
  | PlayerListMessage
  | DoHeartbeatMessage
  | GotHeartbeatMessage
  | BuzzEnabledMessage
  | BuzzDisabledMessage
  | GameStartedMessage
  | GameEndedMessage
  | BuzzedMessage
  | AnswerResultMessage;
