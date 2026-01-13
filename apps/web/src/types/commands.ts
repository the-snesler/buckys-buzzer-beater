// Game commands (client -> server) using tagged format
export type GameCommand =
  | { type: "StartGame" }
  | { type: "EndGame" }
  | { type: "Buzz" }
  | { type: "HostReady" }
  | {
      type: "HostChoice";
      categoryIndex: number;
      questionIndex: number;
    }
  | {
      type: "HostChecked";
      correct: boolean;
    }
  | { type: "HostSkip" }
  | { type: "HostContinue" }
  | {
      type: "Heartbeat";
      hbid: number;
      tDohbRecv: number;
    }
  | {
      type: "LatencyOfHeartbeat";
      hbid: number;
      tLat: number;
    };
