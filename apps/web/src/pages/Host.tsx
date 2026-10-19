import { useParams, useNavigate } from "react-router-dom";
import { useWebSocket } from "../hooks/useWebSocket";
import { useState, useRef, useEffect } from "react";
import { QRCodeSVG } from "qrcode.react";
import type { GameState, Player } from "../types/game";
import { roundName } from "../lib/rounds";
import { formatMoney } from "../lib/money";

export default function Host() {
  console.log("Host mounted", new Date().toISOString());
  const { code } = useParams<{ code: string }>();
  const navigate = useNavigate();
  const [playerList, setPlayerList] = useState<Player[]>([]);
  const [gameState, setGameState] = useState<GameState | null>(null);
  const [buzzedPlayer, setBuzzedPlayer] = useState<{ pid: number; name: string } | null>(null);
  const [copySuccess, setCopySuccess] = useState(false);
  const [choosingPicker, setChoosingPicker] = useState(false);
  const displayWindowRef = useRef<Window | null>(null);
  const hostToken = sessionStorage.getItem(`host_token_${code}`);

  // Forward game state to display popup
  useEffect(() => {
    const handleMessage = (event: MessageEvent) => {
      console.log("Host received message:", event.data);
      if (event.data && event.data.type === "displayReady") {
        // Send current game state when display is ready
        if (displayWindowRef.current && !displayWindowRef.current.closed) {
          displayWindowRef.current.postMessage(
            { gameState, buzzedPlayer, playerList },
            "*"
          );
        }
      }
    };

    if (displayWindowRef.current && !displayWindowRef.current.closed) {
      displayWindowRef.current.postMessage(
        { gameState, buzzedPlayer, playerList },
        "*"
      );
    }
    window.addEventListener("message", handleMessage);
    return () => window.removeEventListener("message", handleMessage);
  }, [gameState, playerList, buzzedPlayer]);

  const openDisplayWindow = () => {
    const popup = window.open(
      `/host/${code}/display`,
      "hostDisplay",
      "width=1280,height=720"
    );
    displayWindowRef.current = popup;
    popup?.postMessage({ gameState, buzzedPlayer }, "*");
  };

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

  const { isConnected, sendMessage } = useWebSocket({
    roomCode: code!,
    token: hostToken!,
    autoConnect: true,
    onMessage: (message) => {
      console.log("Received message:", message);
      switch (message.type) {
        case "PlayerList":
          console.log("Received PlayerList update:", message.players);
          setPlayerList(message.players);
          break;
        case "GameState":
          setGameState({
            state: message.state,
            categories: message.categories,
            round: message.round,
            roundCount: message.roundCount,
            players: message.players,
            currentQuestion: message.currentQuestion,
            currentBuzzer: message.currentBuzzer,
            picker: message.picker,
            ddWager: message.ddWager,
            winner: message.winner,
          });
          if (message.state !== "selection") {
            setChoosingPicker(false);
          }
          if (message.state !== "answer") {
            setBuzzedPlayer(null); // Clear buzzed player when state changes
          }
          break;
        case "PlayerBuzzed":
          setBuzzedPlayer({ pid: message.pid, name: message.name });
          break;
        default:
          break;
      }
    },
  });

  const endRound = () => {
    if (!gameState) return;
    const current = roundName(gameState.round);
    const message =
      gameState.round + 1 < gameState.roundCount
        ? `End ${current} now and start ${roundName(gameState.round + 1)}? The clues left on this board will be skipped.`
        : `End ${current} now? This ends the game, and the clues left on the board will be skipped.`;
    if (window.confirm(message)) {
      sendMessage({ type: "HostNextRound", round: gameState.round });
    }
  };

  const setPicker = (pid: number) => {
    sendMessage({ type: "HostSetPicker", pid });
    setChoosingPicker(false);
  };

  const playerName = (pid: number | null | undefined) =>
    pid != null ? gameState?.players.find((p) => p.pid === pid)?.name ?? null : null;
  const pickerName = playerName(gameState?.picker);
  const currentCategory = gameState?.currentQuestion
    ? gameState.categories[gameState.currentQuestion[0]]
    : undefined;
  const currentClue = gameState?.currentQuestion
    ? currentCategory?.questions[gameState.currentQuestion[1]]
    : undefined;
  const ddWager = gameState?.ddWager ?? null;

  if (!hostToken) {
    return (
      <div className="min-h-screen flex items-center justify-center bg-gray-900">
        <div className="text-white text-center">
          <h1 className="text-2xl font-bold mb-4">Invalid Host Session</h1>
          <p className="text-gray-400">
            Please create a new room from the lobby.
          </p>
        </div>
      </div>
    );
  }

  return (
    <div className="min-h-screen bg-gray-900 p-8">
      <div className="max-w-4xl mx-auto">
        <div className="flex justify-between items-center mb-8">
          <button
            onClick={() => navigate("/")}
            className="px-3 py-1 bg-gray-700 text-white rounded hover:bg-gray-600"
          >
            ← Back
          </button>
          <h1 className="text-3xl font-bold text-white">Room: {code}</h1>
          <div className="flex gap-2 items-center">
            <button
              onClick={openDisplayWindow}
              className="px-3 py-1 bg-red-600 text-white rounded hover:bg-red-500 text-sm"
            >
              Open Display
            </button>
            <div
              className={`px-3 py-1 rounded text-sm ${
                isConnected ? "bg-green-600" : "bg-red-600"
              } text-white`}
            >
              {isConnected ? "Connected" : "Disconnected"}
            </div>
          </div>
        </div>

        {/* Show game board or lobby based on game state */}
        {gameState ? (
          <div className="space-y-6">
            {/* Current State Indicator */}
            <div className="bg-gray-800 rounded-lg p-4 flex justify-between items-center">
              <p className="text-gray-400 text-sm">
                State: <span className="text-white font-semibold">{gameState.state}</span>
              </p>
              {gameState.roundCount > 1 && (
                <p className="text-yellow-400 font-bold">
                  {roundName(gameState.round)}
                  <span className="text-gray-400 font-normal text-sm ml-2">
                    Round {gameState.round + 1} of {gameState.roundCount}
                  </span>
                </p>
              )}
            </div>

            {/* Selection State - Show game board */}
            {gameState.state === "selection" && (
              <div className="bg-gray-800 rounded-lg p-6">
                <div className="flex justify-between items-center mb-4">
                  {gameState.picker === null ? (
                    <h2 className="text-2xl font-semibold text-white">Who picks first?</h2>
                  ) : choosingPicker ? (
                    <h2 className="text-2xl font-semibold text-white">
                      Who picks next?
                      <button
                        onClick={() => setChoosingPicker(false)}
                        className="ml-3 text-sm font-normal text-gray-400 underline hover:text-gray-200"
                      >
                        cancel
                      </button>
                    </h2>
                  ) : (
                    <h2 className="text-2xl font-semibold text-white">
                      <span className="text-yellow-400">{pickerName ?? "Unknown player"}</span>'s pick
                      <button
                        onClick={() => setChoosingPicker(true)}
                        className="ml-3 text-sm font-normal text-gray-400 underline hover:text-gray-200"
                      >
                        change
                      </button>
                    </h2>
                  )}
                  <button
                    onClick={endRound}
                    className="px-4 py-2 bg-gray-600 text-white rounded hover:bg-gray-500 text-sm font-semibold"
                  >
                    {gameState.round + 1 < gameState.roundCount ? "End Round" : "End Game"}
                  </button>
                </div>
                {gameState.picker === null || choosingPicker ? (
                  gameState.players.length === 0 ? (
                    <p className="text-gray-400">Waiting for players to join...</p>
                  ) : (
                    <div className="grid grid-cols-2 md:grid-cols-4 gap-4">
                      {gameState.players.map((player) => (
                        <button
                          key={player.pid}
                          onClick={() => setPicker(player.pid)}
                          className={`py-4 rounded font-bold text-lg text-white ${
                            player.pid === gameState.picker
                              ? "bg-yellow-600 hover:bg-yellow-500"
                              : "bg-blue-600 hover:bg-blue-500"
                          }`}
                        >
                          {player.name}
                        </button>
                      ))}
                    </div>
                  )
                ) : (
                  <div className="overflow-x-auto">
                    <div className="grid gap-4" style={{ gridTemplateColumns: `repeat(${gameState.categories.length}, 1fr)` }}>
                      {gameState.categories.map((category, catIdx) => (
                        <div key={catIdx} className="space-y-2">
                          <h3 className="text-center text-yellow-400 font-bold text-sm uppercase truncate">
                            {category.title}
                          </h3>
                          {category.questions.map((question, qIdx) => (
                            <button
                              key={qIdx}
                              disabled={question.answered}
                              onClick={() => sendMessage({ type: "HostChoice", categoryIndex: catIdx, questionIndex: qIdx })}
                              className={`w-full py-4 rounded font-bold text-lg ${
                                question.answered
                                  ? "bg-gray-700 text-gray-500 cursor-not-allowed"
                                  : "bg-red-600 text-white hover:bg-red-500"
                              }`}
                            >
                              ${question.value}
                            </button>
                          ))}
                        </div>
                      ))}
                    </div>
                  </div>
                )}
              </div>
            )}

            {/* Question Reading State */}
            {gameState.state === "questionReading" && gameState.currentQuestion && (
              <div className="bg-gray-800 rounded-lg p-6">
                <h2 className="text-xl font-semibold text-yellow-400 mb-2">
                  {gameState.categories[gameState.currentQuestion[0]]?.title} - $
                  {gameState.categories[gameState.currentQuestion[0]]?.questions[gameState.currentQuestion[1]]?.value}
                </h2>
                <p className="text-3xl text-white mb-6">
                  {gameState.categories[gameState.currentQuestion[0]]?.questions[gameState.currentQuestion[1]]?.question}
                </p>
                <button
                  onClick={() => sendMessage({ type: "HostReady" })}
                  className="px-6 py-3 bg-green-600 text-white rounded-lg hover:bg-green-500 text-lg font-semibold"
                >
                  Open Buzzing
                </button>
                <button
                  onClick={() => sendMessage({ type: "HostSkip" })}
                  className="px-6 py-3 bg-gray-600 text-white rounded-lg hover:bg-gray-500 text-lg font-semibold"
                >
                  Skip Question
                </button>
              </div>
            )}

            {/* Daily Double - the clue stays hidden until the picker has wagered */}
            {gameState.state === "dailyDoubleWager" && (
              <div className="bg-gray-800 rounded-lg p-6 text-center">
                <h2 className="text-4xl font-black text-yellow-400 mb-2">Daily Double!</h2>
                <p className="text-xl text-white mb-6">
                  {currentCategory?.title} - ${currentClue?.value}
                </p>
                <p className="text-2xl text-gray-300 animate-pulse mb-2">
                  Waiting for {pickerName ?? "the picker"} to wager...
                </p>
                <p className="text-sm text-gray-400 mb-6">
                  The clue appears here once the wager is in.
                </p>
                <button
                  onClick={() => sendMessage({ type: "HostSkip" })}
                  className="px-6 py-3 bg-gray-600 text-white rounded-lg hover:bg-gray-500 text-lg font-semibold"
                >
                  Skip Daily Double
                </button>
              </div>
            )}

            {/* Waiting for Buzz State */}
            {gameState.state === "waitingForBuzz" && gameState.currentQuestion && (
              <div className="bg-gray-800 rounded-lg p-6">
                <h2 className="text-xl font-semibold text-yellow-400 mb-2">
                  {gameState.categories[gameState.currentQuestion[0]]?.title} - $
                  {gameState.categories[gameState.currentQuestion[0]]?.questions[gameState.currentQuestion[1]]?.value}
                </h2>
                <p className="text-3xl text-white mb-6">
                  {gameState.categories[gameState.currentQuestion[0]]?.questions[gameState.currentQuestion[1]]?.question}
                </p>
                <div className="text-center">
                  <p className="text-2xl text-green-400 animate-pulse mb-4">Waiting for buzz...</p>
                  <button
                    onClick={() => sendMessage({ type: "HostSkip" })}
                    className="px-6 py-3 bg-gray-600 text-white rounded-lg hover:bg-gray-500 text-lg font-semibold"
                  >
                    Skip Question
                  </button>
                </div>
              </div>
            )}

            {/* Answer State */}
            {gameState.state === "answer" && gameState.currentQuestion && (
              <div className="bg-gray-800 rounded-lg p-6">
                <h2 className="text-xl font-semibold text-yellow-400 mb-2">
                  {currentCategory?.title} -{" "}
                  {ddWager !== null ? `Daily Double for ${formatMoney(ddWager)}` : `$${currentClue?.value}`}
                </h2>
                <p className="text-3xl text-white mb-4">
                  {currentClue?.question}
                </p>
                <p className="text-lg text-gray-400 mb-6">
                  Answer: <span className="text-yellow-300">
                    {currentClue?.answer}
                  </span>
                </p>
                {ddWager !== null ? (
                  <p className="text-2xl text-red-400 mb-6">
                    {playerName(gameState.currentBuzzer)} wagered {formatMoney(ddWager)}
                  </p>
                ) : (
                  buzzedPlayer && (
                    <p className="text-2xl text-red-400 mb-6">
                      {buzzedPlayer.name} buzzed in!
                    </p>
                  )
                )}
                <div className="flex gap-4 justify-center">
                  <button
                    onClick={() => sendMessage({ type: "HostChecked", correct: true })}
                    className="px-8 py-3 bg-green-600 text-white rounded-lg hover:bg-green-500 text-lg font-semibold"
                  >
                    Correct
                  </button>
                  <button
                    onClick={() => sendMessage({ type: "HostChecked", correct: false })}
                    className="px-8 py-3 bg-red-600 text-white rounded-lg hover:bg-red-500 text-lg font-semibold"
                  >
                    Incorrect
                  </button>
                  {ddWager !== null && (
                    <button
                      onClick={() => sendMessage({ type: "HostSkip" })}
                      className="px-8 py-3 bg-gray-600 text-white rounded-lg hover:bg-gray-500 text-lg font-semibold"
                    >
                      Skip
                    </button>
                  )}
                </div>
              </div>
            )}

            {/* Answer Reveal State */}
            {gameState.state === "answerReveal" && gameState.currentQuestion && (
              <div className="bg-gray-800 rounded-lg p-6">
                <h2 className="text-xl font-semibold text-yellow-400 mb-2">
                  {gameState.categories[gameState.currentQuestion[0]]?.title} - $
                  {gameState.categories[gameState.currentQuestion[0]]?.questions[gameState.currentQuestion[1]]?.value}
                </h2>
                <p className="text-3xl text-white mb-4">
                  {gameState.categories[gameState.currentQuestion[0]]?.questions[gameState.currentQuestion[1]]?.question}
                </p>
                {ddWager !== null && (
                  <p className="text-lg text-gray-300 mb-2">
                    Daily Double: {pickerName} wagered {formatMoney(ddWager)}
                  </p>
                )}
                <p className="text-2xl text-green-400 mb-6">
                  Answer: <span className="text-yellow-300">
                    {gameState.categories[gameState.currentQuestion[0]]?.questions[gameState.currentQuestion[1]]?.answer}
                  </span>
                </p>
                {buzzedPlayer && (
                  <p className="text-lg text-gray-300 mb-6">
                    Answered by: {buzzedPlayer.name}
                  </p>
                )}
                <button
                  onClick={() => sendMessage({ type: "HostContinue" })}
                  className="px-8 py-3 bg-blue-600 text-white rounded-lg hover:bg-blue-500 text-lg font-semibold"
                >
                  Continue to Board
                </button>
              </div>
            )}

            {/* Game End State */}
            {gameState.state === "gameEnd" && (
              <div className="bg-gray-800 rounded-lg p-6 text-center">
                <h2 className="text-3xl font-bold text-yellow-400 mb-6">Game Over!</h2>
                <div className="space-y-2">
                  {[...gameState.players].sort((a, b) => b.score - a.score).map((player, idx) => (
                    <div key={player.pid} className={`p-3 rounded ${idx === 0 ? "bg-yellow-600" : "bg-gray-700"}`}>
                      <span className="text-white font-semibold">{idx + 1}. {player.name}</span>
                      <span className={`ml-4 ${
                          player.score < 0 ? "text-red-500" : player.score === 0 ? "text-gray-500" : "text-white"
                        }`}>{formatMoney(player.score)}</span>
                    </div>
                  ))}
                </div>
              </div>
            )}

            {/* Scoreboard */}
            <div className="bg-gray-800 rounded-lg p-6">
              <h2 className="text-xl font-semibold text-white mb-4">Scores</h2>
              <div className="grid grid-cols-2 md:grid-cols-4 gap-4">
                {gameState.players.map((player) => (
                  <div
                    key={player.pid}
                    className={`bg-gray-700 rounded p-3 text-center ${
                      player.pid === gameState.picker ? "ring-2 ring-yellow-400" : ""
                    }`}
                  >
                    <p className="text-white font-semibold">{player.name}</p>
                    <p className={`text-2xl ${
                        player.score < 0 ? "text-red-500" : player.score === 0 ? "text-gray-500" : "text-green-400"
                      }`}>{formatMoney(player.score)}</p>
                    {player.pid === gameState.picker && (
                      <p className="text-yellow-400 text-xs font-bold uppercase mt-1">Picking</p>
                    )}
                  </div>
                ))}
              </div>
            </div>
          </div>
        ) : (
          /* Lobby - Show players and start button */
          <div className="space-y-6">
            {/* QR Code for joining */}
            <div className="bg-gray-800 rounded-lg p-6">
              <h2 className="text-2xl font-semibold text-white mb-4">Join Game</h2>
              <div className="flex flex-col md:flex-row items-center gap-6">
                <div className="bg-white p-4 rounded-lg">
                  <QRCodeSVG
                    value={`${window.location.origin}/?code=${code}`}
                    size={200}
                    level="M"
                  />
                </div>
                <div className="flex-1 text-center md:text-left">
                  <p className="text-gray-400 mb-2">Scan QR code or visit:</p>
                  <p className="text-white font-mono text-lg mb-3">{window.location.origin}</p>
                  <p className="text-gray-400 mb-1">Room Code:</p>
                  <p className="text-yellow-400 font-bold text-4xl tracking-widest">
                    {code}
                  </p>
                  <button
                    onClick={copyJoinLink}
                    className="mt-4 px-4 py-2 bg-blue-600 text-white rounded hover:bg-blue-500 transition-colors"
                  >
                    {copySuccess ? "✓ Copied!" : "Copy Link"}
                  </button>
                </div>
              </div>
            </div>

            {/* Player List */}
            <div className="bg-gray-800 rounded-lg p-6">
              <h2 className="text-2xl font-semibold text-white mb-4">Players</h2>
              {playerList.length === 0 ? (
                <p className="text-gray-400">No players have joined yet.</p>
              ) : (
                <div className="flex flex-wrap justify-center gap-4">
                  {playerList.map((player) => (
                    <div
                      key={player.pid}
                      className="bg-gray-700 rounded py-2 px-4 text-white grow"
                    >
                      {player.name}
                    </div>
                  ))}
                </div>
              )}
              {playerList.length >= 1 && (
                <button
                  onClick={() => sendMessage({ type: "StartGame" })}
                  className="mt-6 px-4 py-2 bg-red-600 text-white rounded hover:bg-red-700"
                >
                  Start Game
                </button>
              )}
            </div>
          </div>
        )}
      </div>
    </div>
  );
}
