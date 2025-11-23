import type { WSData } from "./types";
import { handleRoomRoutes } from "./routes/rooms";
import {
  handleUpgrade,
  handleOpen,
  handleMessage,
  handleClose,
} from "./ws/handler";
import { join } from "path";

const PORT = process.env.PORT ? parseInt(process.env.PORT, 10) : 3000;
const PUBLIC_DIR = process.env.PUBLIC_DIR || join(import.meta.dir, "../public");

// MIME types for static file serving
const MIME_TYPES: Record<string, string> = {
  ".html": "text/html",
  ".css": "text/css",
  ".js": "application/javascript",
  ".json": "application/json",
  ".png": "image/png",
  ".jpg": "image/jpeg",
  ".jpeg": "image/jpeg",
  ".gif": "image/gif",
  ".svg": "image/svg+xml",
  ".ico": "image/x-icon",
  ".woff": "font/woff",
  ".woff2": "font/woff2",
  ".ttf": "font/ttf",
  ".eot": "application/vnd.ms-fontobject",
};

async function serveStaticFile(pathname: string): Promise<Response | null> {
  const filePath = join(PUBLIC_DIR, pathname);
  const file = Bun.file(filePath);

  if (await file.exists()) {
    const ext = pathname.substring(pathname.lastIndexOf("."));
    const contentType = MIME_TYPES[ext] || "application/octet-stream";
    return new Response(file, {
      headers: { "Content-Type": contentType },
    });
  }
  return null;
}

async function serveIndexHtml(): Promise<Response> {
  const indexPath = join(PUBLIC_DIR, "index.html");
  const file = Bun.file(indexPath);

  if (await file.exists()) {
    return new Response(file, {
      headers: { "Content-Type": "text/html" },
    });
  }
  return new Response("Not Found", { status: 404 });
}

const server = Bun.serve<WSData>({
  port: PORT,

  async fetch(req, server) {
    const url = new URL(req.url);
    const pathname = url.pathname;

    // Handle WebSocket upgrade for /api/v1/rooms/:code/ws
    if (pathname.match(/^\/api\/v1\/rooms\/[A-Z]{6}\/ws$/i)) {
      return handleUpgrade(req, server);
    }

    // Handle room routes
    const roomResponse = await handleRoomRoutes(req, pathname);
    if (roomResponse) {
      return roomResponse;
    }

    // Health check endpoint
    if (pathname === "/health" && req.method === "GET") {
      return new Response(JSON.stringify({ status: "ok" }), {
        headers: { "Content-Type": "application/json" },
      });
    }

    // Serve static files
    const staticResponse = await serveStaticFile(pathname);
    if (staticResponse) {
      return staticResponse;
    }

    // SPA fallback: serve index.html for non-API routes
    if (!pathname.startsWith("/api/")) {
      return serveIndexHtml();
    }

    // 404 for unknown API routes
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
