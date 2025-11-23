import { useState } from 'react';
import { useParams, useNavigate } from 'react-router-dom';
import { useWebSocket } from '../hooks/useWebSocket';

export default function Player() {
  const { code } = useParams<{ code: string }>();
  const navigate = useNavigate();
  const [canBuzz, setCanBuzz] = useState(false);
  const [hasBuzzed, setHasBuzzed] = useState(false);

  // Check for existing session
  const existingPlayerName = sessionStorage.getItem(`player_name`);
  const existingPlayerId = sessionStorage.getItem(`player_id_${code}`);
  const existingToken = sessionStorage.getItem(`player_token_${code}`);

  const { isConnected, sendMessage } = useWebSocket({
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
        case "GameState":
          const gameState = payload as { state: String };
          if (gameState.state === "waitingForBuzz") {
            setCanBuzz(true);
            setHasBuzzed(false);
          } else if (gameState.state === "selection") {
            setCanBuzz(false);
            setHasBuzzed(false);
          } else {
            setCanBuzz(false);
          }
          break;
        case "BuzzEnabled":
          setCanBuzz(true);
          setHasBuzzed(false);
          break;
        case "BuzzDisabled":
          setCanBuzz(false);
          break;
        case "AnswerResult":
          setHasBuzzed(false);
          break;
      }
    },
    autoConnect: true,
  });

  const handleBuzz = () => {
    if (canBuzz && !hasBuzzed) {
      sendMessage({ Buzz: {} });
      setHasBuzzed(true);
    }
  };

  return (
    <div className="min-h-screen bg-gray-900 p-4">
      <div className="max-w-md mx-auto">
        <div className="flex justify-between items-center mb-4">
          <button
            onClick={() => navigate("/")}
            className="px-2 py-1 bg-gray-700 text-white rounded text-sm hover:bg-gray-600"
          >
            ← Back
          </button>
          <h1 className="text-xl font-bold text-white">Room: {code}</h1>
          <div
            className={`px-2 py-1 rounded text-xs ${
              isConnected ? "bg-green-600" : "bg-red-600"
            } text-white`}
          >
            {isConnected ? "Connected" : "Reconnecting..."}
          </div>
        </div>

        <div className="bg-gray-800 rounded-lg p-6 flex flex-col items-center">
          <button
            onClick={handleBuzz}
            disabled={!canBuzz || hasBuzzed}
            className={`w-48 h-48 rounded-full text-2xl font-bold transition-all ${
              canBuzz && !hasBuzzed
                ? "bg-red-600 hover:bg-red-500 active:scale-95 text-white shadow-lg shadow-red-600/50 animate-pulse"
                : hasBuzzed
                ? "bg-yellow-600 text-white cursor-not-allowed"
                : "bg-gray-600 text-gray-400 cursor-not-allowed"
            }`}
          >
            {hasBuzzed ? "BUZZED!" : "BUZZ"}
          </button>
          <p className="text-gray-400 mt-4 text-center">
            {canBuzz && !hasBuzzed
              ? "Tap to buzz in!"
              : hasBuzzed
              ? "Waiting for result..."
              : "Waiting for host..."}
          </p>
        </div>
      </div>
    </div>
  );
}
