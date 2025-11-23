import { useParams } from 'react-router-dom';
import { useState, useEffect } from 'react';
import { useWebSocket } from '../hooks/useWebSocket';

export default function Player() {
  const { code } = useParams<{ code: string }>();
  const [error, setError] = useState<string | null>(null);

  // Check for existing session
  const existingPlayerName = sessionStorage.getItem(`player_name`);
  const existingPlayerId = sessionStorage.getItem(`player_id_${code}`);
  const existingToken = sessionStorage.getItem(`player_token_${code}`);

  const { isConnected } = useWebSocket({
    roomCode: code!,
    playerName: existingPlayerName!,
    playerId: existingPlayerId || undefined,
    token: existingToken || undefined,
    onMessage: (message) => {
      const [type, payload] = Object.entries(message)[0];
      console.log("Received message:", type, payload);
      switch (type) {
        case "NewPlayer":
          sessionStorage.setItem(`player_id_${code}`, (payload as any).pid);
          sessionStorage.setItem(
            `player_token_${code}`,
            (payload as any).token
          );
          break;
      }
      // const payload = message.payload as Record<string, unknown>;
      // if (message.type === 'ROOM_JOINED') {
      //   sessionStorage.setItem(`player_id_${code}`, payload.playerId as string);
      //   sessionStorage.setItem(`player_token_${code}`, payload.reconnectToken as string);
      //   setHasJoined(true);
      // } else if (message.type === 'SYNC_STATE') {
      //   setGameState(payload as unknown as PlayerViewState);
      // } else if (message.type === 'ERROR') {
      //   setError(payload.message as string);
      // }
    },
    autoConnect: true,
  });

  return (
    <div className="min-h-screen bg-gray-900 p-4">
      <div className="max-w-md mx-auto">
        <div className="flex justify-between items-center mb-4">
          <h1 className="text-xl font-bold text-white">Room: {code}</h1>
          <div
            className={`px-2 py-1 rounded text-xs ${
              isConnected ? "bg-green-600" : "bg-red-600"
            } text-white`}
          >
            {isConnected ? "Connected" : "Reconnecting..."}
          </div>
        </div>

        <div className="bg-gray-800 rounded-lg p-6">
          <p className="text-gray-400 text-center">Waiting for host...</p>
        </div>
      </div>
    </div>
  );
}
