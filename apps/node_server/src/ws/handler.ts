import type { WSData, GameWebSocket } from "../types";
import { roomManager } from "../game/RoomManager";
import {
  parseMessage,
  isStartGame,
  isEndGame,
  isBuzzEnable,
  isBuzzDisable,
  isBuzz,
  isHostChecked,
  isHeartbeat,
  isHostChoice,
  isHostReady,
} from "./messages";
import { getEligiblePlayers } from "../game/gameMachine";

// Handle WebSocket upgrade request
export function handleUpgrade(
  req: Request,
  server: { upgrade: (req: Request, options: { data: WSData }) => boolean }
): Response | undefined {
  const url = new URL(req.url);
  const pathname = url.pathname;

  // Match /api/v1/rooms/:code/ws
  const match = pathname.match(/^\/api\/v1\/rooms\/([A-Z]{6})\/ws$/i);
  if (!match || !match[1]) {
    return new Response("Invalid WebSocket path", { status: 400 });
  }

  const roomCode = match[1].toUpperCase();
  const room = roomManager.getRoom(roomCode);

  if (!room) {
    return new Response("Room not found", { status: 404 });
  }

  // Parse query parameters
  const token = url.searchParams.get("token");
  const playerName = url.searchParams.get("playerName");
  const playerID = url.searchParams.get("playerID");

  // Determine connection type
  let wsData: WSData;

  if (token && token === room.hostToken) {
    // Host connection
    wsData = {
      roomCode,
      isHost: true,
    };
  } else if (playerID && token) {
    // Player reconnection
    const pid = parseInt(playerID, 10);
    const player = room.getPlayer(pid);

    if (!player || player.token !== token) {
      return new Response("Invalid player credentials", { status: 401 });
    }

    wsData = {
      roomCode,
      isHost: false,
      playerId: pid,
      playerToken: token,
    };
  } else if (playerName) {
    // New player joining
    const playerToken = roomManager.generatePlayerToken();
    const player = room.addPlayer(playerName, playerToken);

    wsData = {
      roomCode,
      isHost: false,
      playerId: player.pid,
      playerToken: playerToken,
    };
  } else {
    return new Response("Missing required parameters", { status: 400 });
  }

  // Upgrade to WebSocket
  const success = server.upgrade(req, { data: wsData });
  if (success) {
    return undefined; // Bun handles the response
  }

  return new Response("WebSocket upgrade failed", { status: 500 });
}

// Handle WebSocket open
export function handleOpen(ws: GameWebSocket): void {
  const { roomCode, isHost, playerId, playerToken } = ws.data;
  const room = roomManager.getRoom(roomCode);

  if (!room) {
    ws.close(1008, "Room not found");
    return;
  }

  if (!isHost) {
    // if they could buzz in right now, send BuzzEnabled
    const snapshot = room.gameActor?.getSnapshot();
    if (
      snapshot &&
      room.buzzingEnabled &&
      getEligiblePlayers(
        snapshot.context.players,
        snapshot.context.excludedPlayers
      ).some((p) => p.pid === playerId)
    ) {
      ws.send(JSON.stringify({ BuzzEnabled: {} }));
    }
  }

  if (isHost) {
    room.connectHost(ws);
    // Send current player list to host
    room.sendPlayerList();
    room.sendGameStateToHost();
    console.log(`Host connected to room ${roomCode}`);
  } else if (playerId !== undefined) {
    const player = room.getPlayer(playerId);
    if (player) {
      player.connect(ws);
      // Send NewPlayer message with pid and token
      player.send({
        NewPlayer: {
          pid: player.pid,
          token: playerToken!,
        },
      });
      // Update host with new player list
      room.sendPlayerList();
      console.log(
        `Player ${player.name} (${playerId}) connected to room ${roomCode}`
      );
    }
  }
}

// Handle WebSocket message
export function handleMessage(
  ws: GameWebSocket,
  message: string | Buffer
): void {
  const { roomCode, isHost, playerId } = ws.data;
  const room = roomManager.getRoom(roomCode);

  if (!room) {
    ws.close(1008, "Room not found");
    return;
  }

  const msg = parseMessage(message);
  if (!msg) {
    console.error("Failed to parse message:", message);
    return;
  }

  if (isHost) {
    handleHostMessage(room, msg);
  } else if (playerId !== undefined) {
    handlePlayerMessage(room, playerId, msg);
  }
}

// Handle messages from host
function handleHostMessage(
  room: ReturnType<typeof roomManager.getRoom>,
  msg: ReturnType<typeof parseMessage>
): void {
  if (!room || !msg) return;

  if (isStartGame(msg)) {
    room.startGame();
    console.log(`Game started in room ${room.code}`);
  } else if (isEndGame(msg)) {
    room.endGame();
    console.log(`Game ended in room ${room.code}`);
  } else if (isBuzzEnable(msg)) {
    room.enableBuzzing();
    console.log(`Buzzing enabled in room ${room.code}`);
  } else if (isBuzzDisable(msg)) {
    room.disableBuzzing();
    console.log(`Buzzing disabled in room ${room.code}`);
  } else if (isHostChecked(msg)) {
    // Use the new state machine methods
    if (msg.HostChecked.correct) {
      room.handleHostCorrect();
    } else {
      room.handleHostIncorrect();
    }
    console.log(`Host checked answer: ${msg.HostChecked.correct}`);
  } else if (isHostChoice(msg)) {
    room.handleHostChoice(msg.HostChoice.categoryIndex, msg.HostChoice.questionIndex);
    console.log(`Host selected question: category ${msg.HostChoice.categoryIndex}, question ${msg.HostChoice.questionIndex}`);
  } else if (isHostReady(msg)) {
    room.handleHostReady();
    console.log(`Host ready for buzzing in room ${room.code}`);
  }
}

// Handle messages from players
function handlePlayerMessage(
  room: ReturnType<typeof roomManager.getRoom>,
  playerId: number,
  msg: ReturnType<typeof parseMessage>
): void {
  if (!room || !msg) return;

  const player = room.getPlayer(playerId);
  if (!player) return;

  if (isBuzz(msg)) {
    // Try state machine buzz first (if game is running)
    let success = room.handlePlayerBuzz(playerId);
    // Fall back to old buzz handler if state machine not active
    if (!success) {
      success = room.handleBuzz(playerId);
    }
    if (success) {
      console.log(`Player ${player.name} buzzed in room ${room.code}`);
    }
  } else if (isHeartbeat(msg)) {
    // Respond to heartbeat
    player.send({
      GotHeartbeat: {
        hbid: msg.Heartbeat.hbid,
      },
    });
  }
}

// Handle WebSocket close
export function handleClose(ws: GameWebSocket): void {
  const { roomCode, isHost, playerId } = ws.data;
  const room = roomManager.getRoom(roomCode);

  if (!room) return;

  if (isHost) {
    room.disconnectHost();
    console.log(`Host disconnected from room ${roomCode}`);
  } else if (playerId !== undefined) {
    const player = room.getPlayer(playerId);
    if (player) {
      player.disconnect();
      room.sendPlayerList();
      console.log(`Player ${player.name} disconnected from room ${roomCode}`);
    }
  }
}

