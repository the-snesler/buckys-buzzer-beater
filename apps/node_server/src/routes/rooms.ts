import { roomManager } from "../game/RoomManager";
import type { CreateRoomResponse } from "../types";

export function handleCreateRoom(): Response {
  const { code, hostToken } = roomManager.createRoom();

  const response: CreateRoomResponse = {
    room_code: code,
    host_token: hostToken,
  };

  return new Response(JSON.stringify(response), {
    status: 201,
    headers: {
      "Content-Type": "application/json",
    },
  });
}

export function handleRoomRoutes(
  req: Request,
  pathname: string
): Response | null {
  const method = req.method;

  // POST /api/v1/rooms/create
  if (method === "POST" && pathname === "/api/v1/rooms/create") {
    return handleCreateRoom();
  }

  return null;
}
