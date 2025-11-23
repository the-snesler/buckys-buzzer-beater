import { useParams } from 'react-router-dom';
import { useEffect } from "react";
import { useWebSocket } from '../hooks/useWebSocket';

export default function Host() {
  const { code } = useParams<{ code: string }>();
  const hostToken = sessionStorage.getItem(`host_token_${code}`);

  const { isConnected, sendMessage } = useWebSocket({
    roomCode: code!,
    token: hostToken!,
    onMessage: (message) => {},
  });

  // Broadcast state changes to players
  useEffect(() => {
    if (isConnected) {
      // TODO: Send SYNC_STATE to all players when state changes
    }
  }, [isConnected, sendMessage]);

  if (!hostToken) {
    return (
      <div className="min-h-screen flex items-center justify-center bg-gray-900">
        <div className="text-white text-center">
          <h1 className="text-2xl font-bold mb-4">Invalid Host Session</h1>
          <p className="text-gray-400">Please create a new room from the lobby.</p>
        </div>
      </div>
    );
  }

  return (
    <div className="min-h-screen bg-gray-900 p-8">
      <div className="max-w-4xl mx-auto">
        <div className="flex justify-between items-center mb-8">
          <h1 className="text-3xl font-bold text-white">Room: {code}</h1>
          <div
            className={`px-3 py-1 rounded text-sm ${
              isConnected ? "bg-green-600" : "bg-red-600"
            } text-white`}
          >
            {isConnected ? "Connected" : "Disconnected"}
          </div>
        </div>

        <div className="bg-gray-800 rounded-lg p-6">
          <h2 className="text-xl font-semibold text-white mb-4">Phase: yeah</h2>

          {/* TODO: Render phase-specific UI */}
          <p className="text-gray-400">
            Game state machine is running. Players can join with code:{" "}
            <strong>{code}</strong>
          </p>
        </div>
      </div>
    </div>
  );
}
