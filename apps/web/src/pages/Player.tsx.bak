import { useState } from 'react';
import { useParams, useNavigate } from 'react-router-dom';
import { useWebSocket } from '../hooks/useWebSocket';

export default function Player() {
  const { code } = useParams<{ code: string }>();
  const navigate = useNavigate();
  const [canBuzz, setCanBuzz] = useState(false);
  const [score, setScore] = useState<number>(0);
  const [hasBuzzed, setHasBuzzed] = useState(false);
  const [gameStarted, setGameStarted] = useState(false);
  const [copySuccess, setCopySuccess] = useState(false);

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
        case "PlayerState":
          const playerState = payload as {
            pid: number;
            buzzed: boolean;
            score: number;
            canBuzz: boolean;
          };
          setHasBuzzed(playerState.buzzed);
          setCanBuzz(playerState.canBuzz);
          setScore(playerState.score);
          break;
        case "GameState":
          const gameState = payload as {
            state: String;
          };
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
        case "Witness":
          const witnessMsg = payload as any;
          if (witnessMsg.msg && witnessMsg.msg.StartGame) {
            setGameStarted(true);
          }
          break;
      }
    },
    autoConnect: true,
  });

  const copyJoinLink = async () => {
    const joinUrl = `${window.location.origin}/?code=${code}`;
    try {
      await navigator.clipboard.writeText(joinUrl);
      setCopySuccess(true);
      setTimeout(() => setCopySuccess(false), 2000);
    } catch (err) {
      console.error("Failed to copy:", err);
    }
  };

  const handleBuzz = () => {
    if (canBuzz && !hasBuzzed) {
      sendMessage({ Buzz: {} });
      setHasBuzzed(true);
    }
  };

  return (
    <div className="min-h-screen bg-gray-900 p-4">
      <div className="max-w-md mx-auto">
        <div className="mb-4 space-y-2">
          <div className="flex justify-between items-center">
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
              {isConnected ? "●" : "○"}
            </div>
          </div>
          <div
            className={`text-2xl font-bold ${
              score < 0
                ? "text-red-500"
                : score === 0
                ? "text-gray-500"
                : "text-green-400"
            }`}
          >
            ${score}
          </div>
        </div>

        {!gameStarted && (
          <div className="bg-gray-800 rounded-lg p-6 mb-4">
            <h2 className="text-xl font-semibold text-white mb-3">
              Waiting for the host to start the game...
            </h2>
            <button
              onClick={copyJoinLink}
              className="w-full px-4 py-2 bg-blue-600 text-white rounded hover:bg-blue-500 transition-colors"
            >
              {copySuccess ? "✓ Copied!" : "Copy Join Link"}
            </button>
          </div>
        )}

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
