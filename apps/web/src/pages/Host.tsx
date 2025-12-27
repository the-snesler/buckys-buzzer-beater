import { useParams, useNavigate } from "react-router-dom";
import { useWebSocket } from "../hooks/useWebSocket";
import { useState, useRef, useEffect } from "react";
import { QRCodeSVG } from "qrcode.react";

interface Question {
  question: string;
  answer: string;
  value: number;
  answered: boolean;
}

interface Category {
  title: string;
  questions: Question[];
}

interface PlayerState {
  pid: number;
  name: string;
  score: number;
}

interface GameState {
  state: string;
  categories: Category[];
  players: PlayerState[];
  currentQuestion: [number, number] | null;
  currentBuzzer: number | null;
}

export default function Host() {
  const { code } = useParams<{ code: string }>();
  const navigate = useNavigate();
  const [playerList, setPlayerList] = useState<PlayerState[]>([]);
  const [gameState, setGameState] = useState<GameState | null>(null);
  const [buzzedPlayer, setBuzzedPlayer] = useState<{ pid: number; name: string } | null>(null);
  const [copySuccess, setCopySuccess] = useState(false);
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
      const [type, payload] = Object.entries(message)[0];

      switch (type) {
        case "PlayerList":
          setPlayerList(payload as PlayerState[]);
          break;
        case "GameState":
          setGameState(payload as GameState);
          if ((payload as GameState).state !== "answer") {
            setBuzzedPlayer(null); // Clear buzzed player when state changes
          }
          break;
        case "Buzzed":
          setBuzzedPlayer(payload as { pid: number; name: string });
          break;
        case "AnswerResult":
          // Could show a notification, but GameState will update scores
          break;
        default:
          break;
      }
    },
  });

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
            <div className="bg-gray-800 rounded-lg p-4">
              <p className="text-gray-400 text-sm">
                State: <span className="text-white font-semibold">{gameState.state}</span>
              </p>
            </div>

            {/* Selection State - Show game board */}
            {gameState.state === "selection" && (
              <div className="bg-gray-800 rounded-lg p-6">
                <h2 className="text-2xl font-semibold text-white mb-4">Select a Question</h2>
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
                            onClick={() => sendMessage({ HostChoice: { categoryIndex: catIdx, questionIndex: qIdx } })}
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
                  onClick={() => sendMessage({ HostReady: {} })}
                  className="px-6 py-3 bg-green-600 text-white rounded-lg hover:bg-green-500 text-lg font-semibold"
                >
                  Open Buzzing
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
                    onClick={() => sendMessage({ HostSkip: {} })}
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
                  {gameState.categories[gameState.currentQuestion[0]]?.title} - $
                  {gameState.categories[gameState.currentQuestion[0]]?.questions[gameState.currentQuestion[1]]?.value}
                </h2>
                <p className="text-3xl text-white mb-4">
                  {gameState.categories[gameState.currentQuestion[0]]?.questions[gameState.currentQuestion[1]]?.question}
                </p>
                <p className="text-lg text-gray-400 mb-6">
                  Answer: <span className="text-yellow-300">
                    {gameState.categories[gameState.currentQuestion[0]]?.questions[gameState.currentQuestion[1]]?.answer}
                  </span>
                </p>
                {buzzedPlayer && (
                  <p className="text-2xl text-red-400 mb-6">
                    {buzzedPlayer.name} buzzed in!
                  </p>
                )}
                <div className="flex gap-4 justify-center">
                  <button
                    onClick={() => sendMessage({ HostChecked: { correct: true } })}
                    className="px-8 py-3 bg-green-600 text-white rounded-lg hover:bg-green-500 text-lg font-semibold"
                  >
                    Correct
                  </button>
                  <button
                    onClick={() => sendMessage({ HostChecked: { correct: false } })}
                    className="px-8 py-3 bg-red-600 text-white rounded-lg hover:bg-red-500 text-lg font-semibold"
                  >
                    Incorrect
                  </button>
                </div>
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
                        }`}>${player.score}</span>
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
                  <div key={player.pid} className="bg-gray-700 rounded p-3 text-center">
                    <p className="text-white font-semibold">{player.name}</p>
                    <p className={`text-2xl ${
                        player.score < 0 ? "text-red-500" : player.score === 0 ? "text-gray-500" : "text-green-400"
                      }`}>${player.score}</p>
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
                  onClick={() => sendMessage({ StartGame: {} })}
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
