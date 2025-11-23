const API_BASE = "/api/v1";

export interface Question {
  question: string;
  answer: string;
  value: number;
  answered: boolean;
}

export interface Category {
  title: string;
  questions: Question[];
}

export interface CreateRoomResponse {
  roomCode: string;
  hostToken: string;
}

export interface CreateRoomOptions {
  categories?: Category[];
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
