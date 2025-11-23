import type { WSData } from "./types";
import { handleRoomRoutes } from "./routes/rooms";
import {
  handleUpgrade,
  handleOpen,
  handleMessage,
  handleClose,
} from "./ws/handler";

const PORT = process.env.PORT ? parseInt(process.env.PORT, 10) : 3000;

const server = Bun.serve<WSData>({
  port: PORT,

  fetch(req, server) {
    const url = new URL(req.url);
    const pathname = url.pathname;

    // Handle WebSocket upgrade for /api/v1/rooms/:code/ws
    if (pathname.match(/^\/api\/v1\/rooms\/[A-Z]{6}\/ws$/i)) {
      return handleUpgrade(req, server);
    }

    // Handle room routes
    const roomResponse = handleRoomRoutes(req, pathname);
    if (roomResponse) {
      return roomResponse;
    }

    // Health check endpoint
    if (pathname === "/health" && req.method === "GET") {
      return new Response(JSON.stringify({ status: "ok" }), {
        headers: { "Content-Type": "application/json" },
      });
    }

    // 404 for unknown routes
    return new Response("Not Found", { status: 404 });
  },

  websocket: {
    open(ws) {
      handleOpen(ws);
    },
    message(ws, message) {
      handleMessage(ws, message);
    },
    close(ws) {
      handleClose(ws);
    },
  },
});

console.log(`Jeopardy server running on http://localhost:${server.port}`);
