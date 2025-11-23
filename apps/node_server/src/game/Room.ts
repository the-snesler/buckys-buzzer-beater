import type { GameWebSocket, GameState, OutboundMessage } from "../types";
import { Player } from "./Player";

export class Room {
  code: string;
  hostToken: string;
  hostWs: GameWebSocket | null;
  players: Map<number, Player>;
  gameState: GameState;
  buzzingEnabled: boolean;
  currentBuzzer: number | null;
  private nextPid: number;

  constructor(code: string, hostToken: string) {
    this.code = code;
    this.hostToken = hostToken;
    this.hostWs = null;
    this.players = new Map();
    this.gameState = "lobby";
    this.buzzingEnabled = false;
    this.currentBuzzer = null;
    this.nextPid = 1;
  }

  // Host management
  connectHost(ws: GameWebSocket): void {
    this.hostWs = ws;
  }

  disconnectHost(): void {
    this.hostWs = null;
  }

  isHostConnected(): boolean {
    return this.hostWs !== null;
  }

  sendToHost(message: OutboundMessage): void {
    if (this.hostWs) {
      this.hostWs.send(JSON.stringify(message));
    }
  }

  // Player management
  addPlayer(name: string, token: string): Player {
    const pid = this.nextPid++;
    const player = new Player(pid, name, token);
    this.players.set(pid, player);
    return player;
  }

  getPlayer(pid: number): Player | undefined {
    return this.players.get(pid);
  }

  getPlayerByToken(token: string): Player | undefined {
    for (const player of this.players.values()) {
      if (player.token === token) {
        return player;
      }
    }
    return undefined;
  }

  removePlayer(pid: number): void {
    this.players.delete(pid);
  }

  // Broadcasting
  broadcast(message: OutboundMessage): void {
    const msgStr = JSON.stringify(message);
    for (const player of this.players.values()) {
      if (player.ws && player.connected) {
        player.ws.send(msgStr);
      }
    }
  }

  broadcastToAll(message: OutboundMessage): void {
    this.broadcast(message);
    this.sendToHost(message);
  }

  // Send player list to host
  sendPlayerList(): void {
    const playerList = Array.from(this.players.values()).map((p) => p.toJSON());
    this.sendToHost({ PlayerList: playerList });
  }

  // Game state management
  startGame(): void {
    this.gameState = "playing";
    this.broadcastToAll({ GameStarted: {} });
  }

  endGame(): void {
    this.gameState = "ended";
    this.broadcastToAll({ GameEnded: {} });
  }

  // Buzzing management
  enableBuzzing(): void {
    this.buzzingEnabled = true;
    this.currentBuzzer = null;
    // Reset buzz ability for all players
    for (const player of this.players.values()) {
      player.resetBuzz();
    }
    this.broadcastToAll({ BuzzEnabled: {} });
  }

  disableBuzzing(): void {
    this.buzzingEnabled = false;
    this.broadcastToAll({ BuzzDisabled: {} });
  }

  handleBuzz(pid: number): boolean {
    const player = this.players.get(pid);
    if (!player) return false;

    // Check if buzzing is enabled and player can buzz
    if (!this.buzzingEnabled || !player.canBuzz) {
      return false;
    }

    // First valid buzz
    this.currentBuzzer = pid;
    this.buzzingEnabled = false;

    // Notify host about the buzz
    this.sendToHost({
      Buzzed: {
        pid: player.pid,
        name: player.name,
      },
    });

    // Notify all clients buzzing is disabled
    this.broadcastToAll({ BuzzDisabled: {} });

    return true;
  }

  handleHostChecked(correct: boolean, pointValue: number = 100): void {
    if (this.currentBuzzer === null) return;

    const player = this.players.get(this.currentBuzzer);
    if (!player) return;

    if (correct) {
      player.addScore(pointValue);
      this.broadcastToAll({
        AnswerResult: {
          pid: player.pid,
          correct: true,
          newScore: player.score,
        },
      });
      // Send updated player list to host
      this.sendPlayerList();
    } else {
      // Mark this player as unable to buzz for this question
      player.disableBuzz();
      this.broadcastToAll({
        AnswerResult: {
          pid: player.pid,
          correct: false,
          newScore: player.score,
        },
      });
      // Re-enable buzzing for other players
      this.currentBuzzer = null;
    }
  }

  // Cleanup
  cleanup(): void {
    // Close all player connections
    for (const player of this.players.values()) {
      if (player.ws) {
        player.ws.close();
      }
    }
    // Close host connection
    if (this.hostWs) {
      this.hostWs.close();
    }
  }
}
