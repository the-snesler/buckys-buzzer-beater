import type { InboundMessage } from "../types";

export function parseMessage(data: string | Buffer): InboundMessage | null {
  try {
    const str = typeof data === "string" ? data : data.toString();
    return JSON.parse(str) as InboundMessage;
  } catch {
    return null;
  }
}

export function isStartGame(msg: InboundMessage): msg is { StartGame: Record<string, never> } {
  return "StartGame" in msg;
}

export function isEndGame(msg: InboundMessage): msg is { EndGame: Record<string, never> } {
  return "EndGame" in msg;
}

export function isBuzzEnable(msg: InboundMessage): msg is { BuzzEnable: Record<string, never> } {
  return "BuzzEnable" in msg;
}

export function isBuzzDisable(msg: InboundMessage): msg is { BuzzDisable: Record<string, never> } {
  return "BuzzDisable" in msg;
}

export function isBuzz(msg: InboundMessage): msg is { Buzz: Record<string, never> } {
  return "Buzz" in msg;
}

export function isHostChecked(msg: InboundMessage): msg is { HostChecked: { correct: boolean } } {
  return "HostChecked" in msg;
}

export function isHeartbeat(msg: InboundMessage): msg is { Heartbeat: { hbid: number } } {
  return "Heartbeat" in msg;
}

export function isLatencyOfHeartbeat(
  msg: InboundMessage
): msg is { LatencyOfHeartbeat: { hbid: number; t_lat: number } } {
  return "LatencyOfHeartbeat" in msg;
}
