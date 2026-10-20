import { useEffect, useRef, useState } from 'react';
import { useParams, useNavigate } from 'react-router-dom';
import { useWebSocket } from '../hooks/useWebSocket';
import type { GamePhase } from '../types/game';
import type { WagerRange } from '../types/messages';
import { formatMoney } from '../lib/money';

// How long a player is locked out after pressing before the buzzer glows
const LOCKOUT_MS = 250;

// The longest Final Jeopardy response the server keeps. Keep in step with
// FINAL_ANSWER_MAX_CHARS in apps/server/src/lib/game/room.rs.
const MAX_ANSWER_LENGTH = 200;

// How long after the player stops typing their Final Jeopardy response is saved
const ANSWER_AUTOSAVE_MS = 600;

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
  // Final Jeopardy: whether this player may respond now, the response the server has, and
  // how the host judged it
  const [canAnswer, setCanAnswer] = useState(false);
  const [answer, setAnswer] = useState<string | null>(null);
  const [correct, setCorrect] = useState<boolean | null>(null);
  // The response being typed, which is only sent when the player submits it
  const [answerDraft, setAnswerDraft] = useState("");
  // A response sent to the server that it hasn't answered yet
  const [pendingAnswer, setPendingAnswer] = useState<string | null>(null);
  // Whether the player has typed in the response box. After that the box is theirs: the
  // server's copy never replaces what they typed, so it can't move the cursor or undo an
  // edit that crossed an autosave.
  const answerTouchedRef = useRef(false);
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
          setCanAnswer(message.canAnswer);
          setAnswer(message.answer);
          setCorrect(message.correct);
          // The server's state is the truth about the wager and the response, so stop
          // waiting for an answer to one that was sent. Without this, one lost to a dropped
          // connection would leave the form disabled for good.
          setPendingWager(null);
          setPendingAnswer(null);
          if (message.wagerRange === null) {
            // The wager was accepted, or the host moved on
            setWagerError(null);
            setWagerInput("");
          } else if (message.wager !== null && wagerInput === "") {
            // A Final Jeopardy wager made before reconnecting, which can still be changed
            setWagerInput(String(message.wager));
          }
          if (message.answer !== null && !answerTouchedRef.current) {
            // A response sent before reconnecting
            setAnswerDraft(message.answer);
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
          setPendingAnswer(null);
          setRejection(message.reason);
          break;
        case "PublicState":
          if (message.state !== phase) {
            setRejection(null);
          }
          setPhase(message.state);
          setPicker(message.picker);
          if (
            message.state !== "finalWager" &&
            message.state !== "finalAnswer" &&
            message.state !== "finalReveal"
          ) {
            setAnswerDraft("");
            answerTouchedRef.current = false;
          }
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

  const sendAnswer = (trimmed: string) => {
    setRejection(null);
    setPendingAnswer(trimmed);
    sendMessage({ type: "SubmitAnswer", answer: trimmed });
  };

  const submitAnswer = (e: React.FormEvent) => {
    e.preventDefault();
    sendAnswer(answerDraft.trim());
  };

  // Save the response shortly after the player stops typing, so an edit they never
  // submitted isn't lost when the host locks the responses
  useEffect(() => {
    if (!isConnected || phase !== "finalAnswer" || !canAnswer || !answerTouchedRef.current) return;
    const trimmed = answerDraft.trim();
    // Only deduplicate against pending or acknowledged text. PlayerState clears pending
    // sends after reconnect, so a response lost with the connection can be sent again.
    if (trimmed === (pendingAnswer ?? answer ?? "")) {
      return;
    }
    const timer = window.setTimeout(() => {
      sendAnswer(trimmed);
    }, ANSWER_AUTOSAVE_MS);
    return () => window.clearTimeout(timer);
  }, [answerDraft, answer, pendingAnswer, phase, canAnswer, isConnected]);

  const finalWager = phase === "finalWager";
  // The wager the server has, and the one typed, are the same, so there's nothing to send
  const wagerSubmitted = wager !== null && wagerInput !== "" && Number(wagerInput) === wager;
  const answerSubmitted = answer !== null && answerDraft.trim() === answer;

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
              <p className="text-3xl font-black text-yellow-300">
                {finalWager ? "Final Jeopardy!" : "Daily Double!"}
              </p>
              {prompt && <p className="text-lg text-white mt-1">{prompt}</p>}
              <p className="text-gray-300 mt-2">
                Wager between {formatMoney(wagerRange.min)} and {formatMoney(wagerRange.max)}
              </p>
              {finalWager && wager !== null && (
                <p className="text-green-300 font-semibold mt-2">
                  Your wager: {formatMoney(wager)} ✓ (you can change it until the clue is revealed)
                </p>
              )}
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
              disabled={pendingWager !== null || wagerInput === "" || wagerSubmitted}
              className="w-full py-4 rounded-lg bg-yellow-500 text-gray-900 text-xl font-black hover:bg-yellow-400 disabled:opacity-60"
            >
              {pendingWager !== null
                ? `Submitting ${formatMoney(pendingWager)}...`
                : wagerInput === ""
                ? "Enter a wager"
                : wagerSubmitted
                ? `Wagered ${formatMoney(Number(wagerInput))} ✓`
                : `Wager ${formatMoney(Number(wagerInput))}`}
            </button>
          </form>
        ) : finalWager ? (
          <div className="bg-blue-900 rounded-lg p-6 text-center">
            <p className="text-3xl font-black text-yellow-300">Final Jeopardy!</p>
            {prompt && <p className="text-lg text-white mt-1">{prompt}</p>}
            <p className="text-white text-xl mt-4">
              {wager !== null
                ? `You're wagering ${formatMoney(wager)}. Good luck.`
                : "Waiting for the clue..."}
            </p>
          </div>
        ) : phase === "finalAnswer" && canAnswer ? (
          <form onSubmit={submitAnswer} className="bg-blue-900 rounded-lg p-6 space-y-4">
            <div className="text-center">
              <p className="text-3xl font-black text-yellow-300">Final Jeopardy!</p>
              {prompt && <p className="text-xl text-white mt-2">{prompt}</p>}
              {wager !== null && (
                <p className="text-gray-300 mt-2">Your wager: {formatMoney(wager)}</p>
              )}
            </div>
            <textarea
              value={answerDraft}
              onChange={(e) => {
                answerTouchedRef.current = true;
                setAnswerDraft(e.target.value);
                setRejection(null);
              }}
              maxLength={MAX_ANSWER_LENGTH}
              rows={3}
              placeholder="What is..."
              aria-label="Your response"
              className="w-full px-4 py-3 rounded-lg bg-white text-gray-900 text-xl"
            />
            <p className="text-sm text-center min-h-[1.25rem]" aria-live="polite">
              {pendingAnswer !== null ? (
                <span className="text-gray-300">Saving…</span>
              ) : answerSubmitted ? (
                <span className="text-green-300 font-semibold">
                  Saved ✓ (you can edit until the host locks)
                </span>
              ) : null}
            </p>
            <button
              type="submit"
              disabled={pendingAnswer !== null || answerSubmitted || answerDraft.trim() === ""}
              className="w-full py-4 rounded-lg bg-yellow-500 text-gray-900 text-xl font-black hover:bg-yellow-400 disabled:opacity-60"
            >
              {answerSubmitted
                ? "Response saved"
                : answer !== null
                ? "Update response"
                : "Submit response"}
            </button>
          </form>
        ) : phase === "finalAnswer" || phase === "finalReveal" ? (
          <div className="bg-blue-900 rounded-lg p-6 text-center">
            <p className="text-3xl font-black text-yellow-300">Final Jeopardy!</p>
            {wager === null ? (
              <p className="text-gray-200 text-lg mt-4">
                You joined too late to play this one. Watch the screen!
              </p>
            ) : (
              <>
                {prompt && <p className="text-lg text-gray-200 mt-2">{prompt}</p>}
                <p className="text-gray-300 mt-4">Responses are locked. Your response:</p>
                <p className="text-white text-2xl font-bold mt-1 break-words">
                  {answer ? answer : "(no response)"}
                </p>
                <p className="text-gray-300 mt-2">Your wager: {formatMoney(wager)}</p>
                {correct === null ? (
                  <p className="text-gray-200 text-lg mt-4 animate-pulse">
                    Waiting for the host to reveal it...
                  </p>
                ) : correct ? (
                  <p className="text-green-300 text-2xl font-black mt-4">
                    Correct! +{formatMoney(wager)}
                  </p>
                ) : (
                  <p className="text-red-300 text-2xl font-black mt-4">
                    Incorrect. -{formatMoney(wager)}
                  </p>
                )}
              </>
            )}
          </div>
        ) : phase === "gameEnd" ? (
          <div className="bg-gray-800 rounded-lg p-6 text-center">
            <p className="text-3xl font-black text-yellow-300">Game over!</p>
            <p className="text-white text-xl mt-4">Final score: {formatMoney(score)}</p>
            {/* Set once the Final Jeopardy responses were locked */}
            {wager !== null && (
              <div className="mt-6 border-t border-gray-700 pt-4">
                <p className="text-gray-300">Your Final Jeopardy response:</p>
                <p className="text-white text-xl font-bold mt-1 break-words">
                  {answer ? answer : "(no response)"}
                </p>
                <p className="text-gray-300 mt-2">
                  {correct === null
                    ? `Wager: ${formatMoney(wager)}`
                    : correct
                    ? `Correct! +${formatMoney(wager)}`
                    : `Incorrect. -${formatMoney(wager)}`}
                </p>
              </div>
            )}
          </div>
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
