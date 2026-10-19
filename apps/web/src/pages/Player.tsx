import { useEffect, useRef, useState } from 'react';
import { useParams, useNavigate } from 'react-router-dom';
import { useWebSocket } from '../hooks/useWebSocket';
import type { GamePhase } from '../types/game';
import type { WagerRange } from '../types/messages';
import { formatMoney } from '../lib/money';

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
  const [myPid, setMyPid] = useState<number | null>(null);
  const [phase, setPhase] = useState<GamePhase>("start");
  const [picker, setPicker] = useState<number | null>(null);
  const [wagerRange, setWagerRange] = useState<WagerRange | null>(null);
  const [wager, setWager] = useState<number | null>(null);
  const [prompt, setPrompt] = useState<string | null>(null);
  const [wagerInput, setWagerInput] = useState("");
  // A wager sent to the server that it hasn't answered yet
  const [pendingWager, setPendingWager] = useState<number | null>(null);
  // Why this phone won't send the wager as typed
  const [wagerError, setWagerError] = useState<string | null>(null);
  // Why the server refused this player's last command, until the game moves on
  const [rejection, setRejection] = useState<string | null>(null);
  // Whether the clue being played is a Daily Double. Players are only told while the
  // picker wagers, so this carries it on through the answer.
  const [dailyDoubleInPlay, setDailyDoubleInPlay] = useState(false);
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
          setMyPid(message.pid);
          setWagerRange(message.wagerRange);
          setWager(message.wager);
          setPrompt(message.prompt);
          // The server's state is the truth about the wager, so stop waiting for an answer
          // to one that was sent. Without this, a wager lost to a dropped connection would
          // leave the form disabled for good.
          setPendingWager(null);
          if (message.wagerRange === null) {
            // The wager was accepted, or the host moved on
            setWagerError(null);
            setWagerInput("");
          }
          setHasBuzzed(message.buzzed);
          updateCanBuzz(message.canBuzz);
          setScore(message.score);
          if (message.canBuzz || message.score !== 0) {
            setGameStarted(true);
          }
          break;
        case "Rejected":
          setPendingWager(null);
          setRejection(message.reason);
          break;
        case "PublicState":
          if (message.state !== phase) {
            setRejection(null);
          }
          setPhase(message.state);
          setPicker(message.picker);
          if (message.state === "dailyDoubleWager") {
            setDailyDoubleInPlay(true);
          } else if (message.state !== "answer" && message.state !== "answerReveal") {
            setDailyDoubleInPlay(false);
          }
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

  const changeWagerInput = (value: string) => {
    setWagerInput(value);
    setWagerError(null);
    setRejection(null);
  };

  const submitWager = (e: React.FormEvent) => {
    e.preventDefault();
    if (!wagerRange) return;
    const amount = Number(wagerInput);
    if (wagerInput.trim() === "" || !Number.isInteger(amount)) {
      setWagerError("Enter a whole number of dollars");
      return;
    }
    if (amount < wagerRange.min || amount > wagerRange.max) {
      setWagerError(
        `Your wager must be between ${formatMoney(wagerRange.min)} and ${formatMoney(wagerRange.max)}`
      );
      return;
    }
    setWagerError(null);
    setRejection(null);
    setPendingWager(amount);
    sendMessage({ type: "SubmitWager", amount });
  };

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
            {formatMoney(score)}
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

        {phase === "selection" && myPid !== null && picker === myPid && (
          <div className="bg-yellow-500 rounded-lg p-4 mb-4 text-center">
            <p className="text-2xl font-black text-gray-900">Your pick!</p>
            <p className="text-gray-900">Tell the host which clue you want.</p>
          </div>
        )}

        {rejection && wagerRange === null && (
          <div className="bg-red-900 rounded-lg p-4 mb-4 text-center" role="alert">
            <p className="text-red-100 font-semibold">{rejection}</p>
          </div>
        )}

        {wagerRange !== null ? (
          <form onSubmit={submitWager} className="bg-blue-900 rounded-lg p-6 space-y-4">
            <div className="text-center">
              <p className="text-3xl font-black text-yellow-300">Daily Double!</p>
              {prompt && <p className="text-lg text-white mt-1">{prompt}</p>}
              <p className="text-gray-300 mt-2">
                Wager between {formatMoney(wagerRange.min)} and {formatMoney(wagerRange.max)}
              </p>
            </div>
            <div className="flex items-center gap-2">
              <span className="text-3xl font-bold text-white">$</span>
              <input
                type="text"
                inputMode="numeric"
                pattern="[0-9]*"
                autoComplete="off"
                value={wagerInput}
                onChange={(e) => changeWagerInput(e.target.value.replace(/[^0-9]/g, ""))}
                disabled={pendingWager !== null}
                placeholder={String(wagerRange.min)}
                aria-label="Wager"
                className="w-full px-4 py-3 rounded-lg bg-white text-gray-900 text-3xl font-bold text-center disabled:opacity-60"
              />
            </div>
            <div className="flex gap-2">
              <button
                type="button"
                onClick={() => changeWagerInput(String(wagerRange.min))}
                disabled={pendingWager !== null}
                className="flex-1 py-2 rounded-lg bg-gray-700 text-white font-semibold hover:bg-gray-600 disabled:opacity-60"
              >
                {formatMoney(wagerRange.min)} min
              </button>
              <button
                type="button"
                onClick={() => changeWagerInput(String(wagerRange.max))}
                disabled={pendingWager !== null}
                className="flex-1 py-2 rounded-lg bg-gray-700 text-white font-semibold hover:bg-gray-600 disabled:opacity-60"
              >
                {/* The cap can be the board's top clue rather than this player's score */}
                {wagerRange.max > score ? "Max" : "All in"} ({formatMoney(wagerRange.max)})
              </button>
            </div>
            {(wagerError ?? rejection) && (
              <p className="text-red-300 font-semibold text-center" role="alert">
                {wagerError ?? rejection}
              </p>
            )}
            <button
              type="submit"
              disabled={pendingWager !== null || wagerInput === ""}
              className="w-full py-4 rounded-lg bg-yellow-500 text-gray-900 text-xl font-black hover:bg-yellow-400 disabled:opacity-60"
            >
              {pendingWager !== null
                ? `Submitting ${formatMoney(pendingWager)}...`
                : wagerInput === ""
                ? "Enter a wager"
                : `Wager ${formatMoney(Number(wagerInput))}`}
            </button>
          </form>
        ) : wager !== null && phase === "answer" ? (
          <div className="bg-blue-900 rounded-lg p-6 text-center">
            <p className="text-3xl font-black text-yellow-300">Daily Double!</p>
            {prompt && <p className="text-lg text-gray-200 mt-1">{prompt}</p>}
            <p className="text-white text-xl mt-4">
              You wagered {formatMoney(wager)}. Give the host your answer!
            </p>
          </div>
        ) : dailyDoubleInPlay &&
          (phase === "dailyDoubleWager" || phase === "answer") ? (
          <div className="bg-blue-900 rounded-lg p-6 text-center">
            <p className="text-3xl font-black text-yellow-300">Daily Double!</p>
            <p className="text-gray-200 text-lg mt-2">
              {phase === "dailyDoubleWager"
                ? "The picker is making a wager..."
                : "The picker is answering..."}
            </p>
          </div>
        ) : (
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
        )}
      </div>
    </div>
  );
}
