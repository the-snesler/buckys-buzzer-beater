import type { GameWebSocket, PlayerInfo, OutboundMessage } from "../types";

export class Player implements PlayerInfo {
  pid: number;
  name: string;
  token: string;
  score: number;
  canBuzz: boolean;
  connected: boolean;
  ws: GameWebSocket | null;

  constructor(pid: number, name: string, token: string) {
    this.pid = pid;
    this.name = name;
    this.token = token;
    this.score = 0;
    this.canBuzz = true;
    this.connected = false;
    this.ws = null;
  }

  connect(ws: GameWebSocket): void {
    this.ws = ws;
    this.connected = true;
  }

  disconnect(): void {
    this.ws = null;
    this.connected = false;
  }

  send(message: OutboundMessage): void {
    if (this.ws && this.connected) {
      this.ws.send(JSON.stringify(message));
    }
  }

  addScore(points: number): void {
    this.score += points;
  }

  resetBuzz(): void {
    this.canBuzz = true;
  }

  disableBuzz(): void {
    this.canBuzz = false;
  }

  toJSON(): { pid: number; name: string; score: number } {
    return {
      pid: this.pid,
      name: this.name,
      score: this.score,
    };
  }
}
