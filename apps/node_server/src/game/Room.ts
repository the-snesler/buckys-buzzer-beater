import type { GameWebSocket, GameState, OutboundMessage } from "../types";
import { Player } from "./Player";
import { createActor, type Actor } from "xstate";
import {
  gameMachine,
  createGameStateSnapshot,
  type Category,
  type GameContext,
  type GameEvent,
  getEligiblePlayers,
} from "./gameMachine";

export class Room {
  code: string;
  hostToken: string;
  hostWs: GameWebSocket | null;
  players: Map<number, Player>;
  gameState: GameState;
  buzzingEnabled: boolean;
  currentBuzzer: number | null;
  private nextPid: number;
  gameActor: Actor<typeof gameMachine> | null = null;
  private categories: Category[] = [];

  constructor(code: string, hostToken: string, categories: Category[] = []) {
    this.code = code;
    this.hostToken = hostToken;
    this.hostWs = null;
    this.players = new Map();
    this.gameState = "lobby";
    this.buzzingEnabled = false;
    this.currentBuzzer = null;
    this.nextPid = 1;
    this.categories = categories;
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

    // Create initial context with current players
    const initialPlayers = Array.from(this.players.values()).map((p) => ({
      pid: p.pid,
      name: p.name,
      score: p.score,
    }));

    // Create the state machine actor
    this.gameActor = createActor(gameMachine, {
      input: {
        categories: this.categories,
        players: initialPlayers,
      },
    });

    // Subscribe to state changes to send GameState to host
    this.gameActor.subscribe((snapshot) => {
      this.sendGameStateToHost();
    });

    // Start the actor
    this.gameActor.start();

    this.broadcastToAll({ GameStarted: {} });
    this.sendGameStateToHost();
  }

  endGame(): void {
    this.gameState = "ended";
    if (this.gameActor) {
      this.gameActor.stop();
      this.gameActor = null;
    }
    this.broadcastToAll({ GameEnded: {} });
  }

  // Send current game state to host
  sendGameStateToHost(): void {
    if (!this.gameActor || !this.hostWs) return;

    const snapshot = this.gameActor.getSnapshot();
    const stateName =
      typeof snapshot.value === "string"
        ? snapshot.value
        : Object.keys(snapshot.value)[0] ?? "unknown";

    this.sendToHost({
      GameState: createGameStateSnapshot(stateName, snapshot.context),
    });
  }

  // State machine event handlers
  handleHostChoice(categoryIndex: number, questionIndex: number): void {
    if (!this.gameActor) return;
    this.gameActor.send({ type: "HOST_CHOICE", categoryIndex, questionIndex });
  }

  handleHostReady(): void {
    if (!this.gameActor) return;
    this.gameActor.send({ type: "HOST_READY" });
    // Enable buzzing when host is ready
    this.enableBuzzing();
  }

  handlePlayerBuzz(pid: number): boolean {
    if (!this.gameActor) return false;

    const snapshot = this.gameActor.getSnapshot();
    // Only allow buzz in waitingForBuzz state
    if (snapshot.value !== "waitingForBuzz") return false;

    // Check if player is excluded
    if (snapshot.context.excludedPlayers.includes(pid)) return false;

    this.gameActor.send({ type: "PLAYER_BUZZ", pid });
    this.currentBuzzer = pid;
    this.disableBuzzing();

    // Notify host about the buzz
    const player = this.players.get(pid);
    if (player) {
      this.sendToHost({
        Buzzed: {
          pid: player.pid,
          name: player.name,
        },
      });
    }

    return true;
  }

  handleHostCorrect(): void {
    if (!this.gameActor) return;

    // Update player score in our players map
    if (this.currentBuzzer !== null) {
      const player = this.players.get(this.currentBuzzer);
      const snapshot = this.gameActor.getSnapshot();
      if (player && snapshot.context.currentQuestion) {
        const [catIdx, qIdx] = snapshot.context.currentQuestion;
        const pointValue =
          this.categories[catIdx]?.questions[qIdx]?.value ?? 100;
        player.addScore(pointValue);

        this.broadcastToAll({
          AnswerResult: {
            pid: player.pid,
            correct: true,
            newScore: player.score,
          },
        });
      }
    }

    this.gameActor.send({ type: "HOST_CORRECT" });
    this.currentBuzzer = null;

    // Reset buzz ability for all players
    for (const player of this.players.values()) {
      player.resetBuzz();
    }

    // Check if game ended
    const snapshot = this.gameActor.getSnapshot();
    if (snapshot.value === "gameEnd") {
      this.endGame();
    }
  }

  handleHostIncorrect(): void {
    if (!this.gameActor) return;

    // Notify about incorrect answer
    if (this.currentBuzzer !== null) {
      const player = this.players.get(this.currentBuzzer);
      if (player) {
        player.disableBuzz();
        this.broadcastToAll({
          AnswerResult: {
            pid: player.pid,
            correct: false,
            newScore: player.score,
          },
        });
      }
    }

    this.gameActor.send({ type: "HOST_INCORRECT" });
    this.currentBuzzer = null;

    // Check new state
    const snapshot = this.gameActor.getSnapshot();
    if (snapshot.value === "waitingForBuzz") {
      // Re-enable buzzing for remaining players
      this.enableBuzzing();
    } else if (snapshot.value === "gameEnd") {
      this.endGame();
    }
  }

  // Sync player to state machine
  syncPlayerToMachine(pid: number, name: string): void {
    if (this.gameActor) {
      this.gameActor.send({ type: "ADD_PLAYER", pid, name });
    }
  }

  removePlayerFromMachine(pid: number): void {
    if (this.gameActor) {
      this.gameActor.send({ type: "REMOVE_PLAYER", pid });
    }
  }

  // Set categories (for when host uploads game config)
  setCategories(categories: Category[]): void {
    this.categories = categories;
  }

  // Buzzing management
  enableBuzzing(): void {
    if (!this.gameActor) return;

    this.buzzingEnabled = true;
    this.currentBuzzer = null;

    const snapshot = this.gameActor.getSnapshot();
    for (const player of getEligiblePlayers(
      snapshot.context.players,
      snapshot.context.excludedPlayers
    )) {
      const p = this.players.get(player.pid);
      if (p) {
        p.send({ BuzzEnabled: {} });
      }
    }
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
