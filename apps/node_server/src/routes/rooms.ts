import { roomManager } from "../game/RoomManager";
import type { CreateRoomResponse } from "../types";
import type { Category } from "../game/gameMachine";

interface CreateRoomBody {
  categories?: Category[];
}

export async function handleCreateRoom(req: Request): Promise<Response> {
  let categories: Category[] = [];

  try {
    const body = await req.json() as CreateRoomBody;
    if (body.categories && Array.isArray(body.categories)) {
      categories = body.categories;
    }
  } catch {
    // No body or invalid JSON - use empty categories
  }

  const { code, hostToken } = roomManager.createRoom(categories);

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

export async function handleRoomRoutes(
  req: Request,
  pathname: string
): Promise<Response | null> {
  const method = req.method;

  // POST /api/v1/rooms/create
  if (method === "POST" && pathname === "/api/v1/rooms/create") {
    return handleCreateRoom(req);
  }

  return null;
}
