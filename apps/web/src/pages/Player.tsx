import { useParams } from 'react-router-dom';
import { useState, useEffect } from 'react';
import { useWebSocket } from '../hooks/useWebSocket';

export default function Player() {
  const { code } = useParams<{ code: string }>();

  const [hasJoined, setHasJoined] = useState(false);
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
      console.log("Received message:", message);
    },
    autoConnect: true,
  });

  // Auto-reconnect if we have existing credentials
  useEffect(() => {
    if (existingPlayerId && existingToken) {
      setHasJoined(true);
    }
  }, [existingPlayerId, existingToken]);

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
          {false ? (
            <>
              <h2 className="text-lg font-semibold text-white mb-4">
                Phase: yeah
              </h2>
              {/* TODO: Render phase-specific player UI */}
              <p className="text-gray-400">Waiting for game updates...</p>
            </>
          ) : (
            <p className="text-gray-400 text-center">Waiting for host...</p>
          )}
        </div>
      </div>
    </div>
  );
}
