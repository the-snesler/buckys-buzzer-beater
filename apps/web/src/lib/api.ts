import type { GameCategory } from "./gameFile";

const API_BASE = "/api/v1";

export interface CreateRoomResponse {
  roomCode: string;
  hostToken: string;
}

export interface CreateRoomOptions {
  /** The Jeopardy round's board */
  categories?: GameCategory[];
  /** The Double Jeopardy round's board. Empty or omitted if the game has none. */
  doubleCategories?: GameCategory[];
  /**
   * Whether the server hides Daily Doubles at random: 1 in Jeopardy and 2 in Double
   * Jeopardy, in rounds where the game file has none
   */
  randomDailyDoubles?: boolean;
}

export async function createRoom(options: CreateRoomOptions = {}): Promise<CreateRoomResponse> {
  const response = await fetch(`${API_BASE}/rooms/create`, {
    method: "POST",
    headers: {
      "Content-Type": "application/json",
    },
    body: JSON.stringify(options),
  });

  if (!response.ok) {
    throw new Error("Failed to create room");
  }

  const { room_code: roomCode, host_token: hostToken } = await response.json();
  return { roomCode, hostToken };
}

export function getWebSocketUrl(
  roomCode: string,
  params: Record<string, string>
): string {
  const protocol = window.location.protocol === "https:" ? "wss:" : "ws:";
  const host = window.location.host;
  const queryString = new URLSearchParams(params).toString();
  return `${protocol}//${host}${API_BASE}/rooms/${roomCode}/ws?${queryString}`;
}
