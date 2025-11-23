import { useParams } from 'react-router-dom';
import { useState, useEffect } from 'react';
import { useWebSocket } from '../hooks/useWebSocket';

export default function Player() {
  const { code } = useParams<{ code: string }>();

  const [playerName, setPlayerName] = useState('');
  const [isJoining, setIsJoining] = useState(false);
  const [hasJoined, setHasJoined] = useState(false);
  const [error, setError] = useState<string | null>(null);

  // Check for existing session
  const existingPlayerId = sessionStorage.getItem(`player_id_${code}`);
  const existingToken = sessionStorage.getItem(`player_token_${code}`);

  const { isConnected, connect } = useWebSocket({
    roomCode: code!,
    playerName: hasJoined ? playerName : undefined,
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
    },
    autoConnect: !!existingPlayerId,
  });

  const handleJoin = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!playerName.trim()) return;

    setIsJoining(true);
    setError(null);
    setHasJoined(true);
    connect();
  };

  // Auto-reconnect if we have existing credentials
  useEffect(() => {
    if (existingPlayerId && existingToken) {
      setHasJoined(true);
    }
  }, [existingPlayerId, existingToken]);

  if (!hasJoined) {
    return (
      <div className="min-h-screen flex items-center justify-center bg-gray-900">
        <div className="bg-gray-800 p-8 rounded-lg shadow-xl max-w-md w-full">
          <h1 className="text-2xl font-bold text-white text-center mb-2">
            Join Room
          </h1>
          <p className="text-gray-400 text-center mb-6">Code: {code}</p>

          <form onSubmit={handleJoin}>
            <label className="block text-gray-300 mb-2">Your Name</label>
            <input
              type="text"
              value={playerName}
              onChange={(e) => setPlayerName(e.target.value)}
              maxLength={20}
              placeholder="Enter your name"
              className="w-full px-4 py-3 rounded bg-gray-700 text-white"
              autoFocus
            />
            <button
              type="submit"
              disabled={!playerName.trim() || isJoining}
              className="w-full mt-4 px-4 py-3 bg-blue-600 text-white rounded font-semibold hover:bg-blue-700 disabled:opacity-50"
            >
              {isJoining ? "Joining..." : "Join"}
            </button>
          </form>

          {error && <p className="mt-4 text-red-400 text-center">{error}</p>}
        </div>
      </div>
    );
  }

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
