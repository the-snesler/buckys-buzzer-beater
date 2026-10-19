import { useEffect, useRef, useState } from 'react';
import { useParams, useNavigate } from 'react-router-dom';
import { useWebSocket } from '../hooks/useWebSocket';

// How long a player is locked out after pressing before the buzzer glows
const LOCKOUT_MS = 250;

export default function Player() {
  const { code } = useParams<{ code: string }>();
  const navigate = useNavigate();
  const [canBuzz, setCanBuzz] = useState(false);
  const [score, setScore] = useState<number>(0);
  const [hasBuzzed, setHasBuzzed] = useState(false);
  const [gameStarted, setGameStarted] = useState(false);
  const [copySuccess, setCopySuccess] = useState(false);
  const [lockedOut, setLockedOut] = useState(false);
  const enabledAtRef = useRef<number | null>(null);
  const lockedUntilRef = useRef(0);
  const lockoutTimerRef = useRef<number | undefined>(undefined);

  useEffect(() => () => window.clearTimeout(lockoutTimerRef.current), []);

  // Reaction time is measured from the moment buzzing is enabled on this device
  const updateCanBuzz = (next: boolean) => {
    if (!next) {
      enabledAtRef.current = null;
    } else if (enabledAtRef.current === null) {
      enabledAtRef.current = performance.now();
    }
    setCanBuzz(next);
  };

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
      console.log("Received message:", message);
      switch (message.type) {
        case "NewPlayer":
          sessionStorage.setItem(`player_id_${code}`, message.pid.toString());
          sessionStorage.setItem(
            `player_token_${code}`,
            message.token
          );
          break;
        case "PlayerState":
          setHasBuzzed(message.buzzed);
          updateCanBuzz(message.canBuzz);
          setScore(message.score);
          if (message.canBuzz || message.score !== 0) {
            setGameStarted(true);
          }
          break;
        case "PublicState":
          if (message.state !== "start") {
            setGameStarted(true);
          }
          // Buzzing is enabled by the PlayerState that accompanies waitingForBuzz,
          // since it knows whether this player already buzzed on this question
          if (message.state === "questionReading" || message.state === "answer") {
            updateCanBuzz(false);
            setHasBuzzed(false);
          } else if (message.state !== "waitingForBuzz") {
            updateCanBuzz(false);
          }
          break;
      }
    },
    autoConnect: true,
  });

  const showIndicators = !import.meta.env.PROD;

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

  const handleBuzz = (pressedAt: number) => {
    if (hasBuzzed) return;
    const enabledAt = enabledAtRef.current;
    if (!canBuzz || enabledAt === null) {
      // Pressing before the buzzer glows locks you out, like on Jeopardy
      lockedUntilRef.current = pressedAt + LOCKOUT_MS;
      setLockedOut(true);
      window.clearTimeout(lockoutTimerRef.current);
      lockoutTimerRef.current = window.setTimeout(() => setLockedOut(false), LOCKOUT_MS);
      return;
    }
    if (pressedAt < lockedUntilRef.current) return;
    sendMessage({
      type: "Buzz",
      reactionMs: Math.max(0, Math.round(pressedAt - enabledAt)),
    });
    setHasBuzzed(true);
  };

  const glowing = canBuzz && !hasBuzzed && !lockedOut;

  return (
    <div className="min-h-[100svh] bg-gray-900 p-4">
      <div className="max-w-md mx-auto">
        <div className="mb-4 space-y-2">
          <div className="flex justify-between items-center">
            {showIndicators && (
              <button
                onClick={() => navigate("/")}
                className="px-2 py-1 bg-gray-700 text-white rounded text-sm hover:bg-gray-600"
              >
                ← Back
              </button>
            )}
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
            <p className="text-gray-300 mb-4">
              The buzzer starts glowing when the host finishes reading a question. To buzz
              in, press it as soon as it starts glowing. Don't mash: pressing before it
              glows locks you out for a moment. The fastest reaction to the glow wins, no
              matter whose connection is faster.
            </p>
            <button
              onClick={copyJoinLink}
              className="w-full px-4 py-2 bg-blue-600 text-white rounded hover:bg-blue-500 transition-colors"
            >
              {copySuccess ? "✓ Copied!" : "Copy Join Link"}
            </button>
          </div>
        )}

        <div className="bg-gray-800 rounded-lg p-6 flex flex-col items-center">
          {/* Buzz on press rather than release, and keep the button pressable before
              it glows so early presses trigger the lockout */}
          <button
            onPointerDown={(e) => handleBuzz(e.timeStamp)}
            onKeyDown={(e) => {
              if ((e.key === " " || e.key === "Enter") && !e.repeat) {
                e.preventDefault();
                handleBuzz(e.timeStamp);
              }
            }}
            disabled={hasBuzzed}
            className={`w-48 h-48 rounded-full text-2xl font-bold transition-all select-none touch-manipulation ${
              lockedOut
                ? "bg-gray-700 text-gray-500 scale-95"
                : glowing
                ? "bg-red-600 hover:bg-red-500 active:scale-95 text-white shadow-lg shadow-red-600/50 animate-pulse"
                : hasBuzzed
                ? "bg-yellow-600 text-white cursor-not-allowed"
                : "bg-gray-600 text-gray-400"
            }`}
          >
            {hasBuzzed ? "BUZZED!" : lockedOut ? "TOO EARLY" : "BUZZ"}
          </button>
          <p className="text-gray-400 mt-4 text-center">
            {lockedOut
              ? "Too early! Wait for the glow."
              : glowing
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
