import { useParams } from "react-router-dom";
import { useState, useEffect, useRef } from "react";
import React from "react";
import { QRCodeSVG } from "qrcode.react";
import type { GameState, Player } from "../types/game";
import { roundName } from "../lib/rounds";

/** How long the banner announcing a new round covers the board */
const ROUND_BANNER_MS = 4000;

interface DisplayMessage {
  gameState: GameState | null;
  buzzedPlayer: { pid: number; name: string } | null;
  playerList: Player[];
}

export default function HostDisplay() {
  const { code } = useParams<{ code: string }>();
  const [gameState, setGameState] = useState<GameState | null>(null);
  const [buzzedPlayer, setBuzzedPlayer] = useState<{
    pid: number;
    name: string;
  } | null>(null);
  const [playerList, setPlayerList] = useState<Player[]>([]);
  const [roundBanner, setRoundBanner] = useState<number | null>(null);
  const lastRoundRef = useRef<number | null>(null);

  useEffect(() => {
    const handleMessage = (event: MessageEvent<DisplayMessage>) => {
      // Only accept messages with the expected structure
      console.log("HostDisplay received message:", event.data);
      if (
        event.data &&
        typeof event.data === "object" &&
        "gameState" in event.data
      ) {
        setGameState(event.data.gameState);
        setBuzzedPlayer(event.data.buzzedPlayer);
        setPlayerList(event.data.playerList || []);
      }
    };
    // post message to let host know we're ready to receive updates

    window.addEventListener("message", handleMessage);
    window.opener.postMessage({ type: "displayReady" }, "*");
    return () => window.removeEventListener("message", handleMessage);
  }, []);

  // Announce each new round, but not the round in progress when the display opens
  const round = gameState?.round;
  useEffect(() => {
    if (round === undefined) return;
    const previous = lastRoundRef.current;
    lastRoundRef.current = round;
    if (previous === null || round <= previous) return;

    setRoundBanner(round);
    const timer = setTimeout(() => setRoundBanner(null), ROUND_BANNER_MS);
    return () => clearTimeout(timer);
  }, [round]);

  // Waiting for connection from host window
  if (!gameState) {
    return (
      <div className="min-h-screen flex items-center justify-center fancy-bg">
        <div className="text-white text-center">
          <h1 className="text-4xl font-bold mb-4">Room: {code}</h1>
          <p className="text-2xl text-red-300 animate-pulse">
            Waiting for the game to start...
          </p>
          <div className="flex flex-col md:flex-row items-center gap-6 mt-12">
            <div className="bg-white p-4 rounded-lg">
              <QRCodeSVG
                value={`${window.location.origin}/?code=${code}`}
                size={200}
                level="M"
              />
            </div>
            <div className="flex-1 text-center md:text-left">
              <p className="text-gray-400 mb-2">Scan QR code or visit:</p>
              <p className="text-white font-mono text-lg mb-3">
                {window.location.origin}
              </p>
              <p className="text-gray-400 mb-1">Room Code:</p>
              <p className="text-yellow-400 font-bold text-4xl tracking-widest">
                {code}
              </p>
            </div>
          </div>
          <div className="mt-12 p-6">
            <h2 className="text-2xl font-semibold text-white mb-4">Players</h2>
            {playerList.length === 0 ? (
              <p className="text-gray-400">No players have joined yet.</p>
            ) : (
              <ul className="flex flex-wrap max-w-screen-md gap-4 justify-center">
                {playerList.map((player) => (
                  <li
                    key={player.pid}
                    className="bg-white/10 rounded p-3 text-white"
                  >
                    {player.name}
                  </li>
                ))}
              </ul>
            )}
          </div>
        </div>
      </div>
    );
  }

  return (
    <div className="min-h-screen flex items-stretch pb-24 justify-stretch fancy-bg p-8">
      <div className="m-8 w-full h-full">
        {/* New round banner */}
        {gameState.state === "selection" && roundBanner !== null && (
          <div className="flex flex-col items-center justify-center min-h-[80vh]">
            <h1
              className="text-8xl font-black text-white tracking-wide text-center animate-pulse"
              style={{
                fontFamily: 'Impact, "Arial Black", sans-serif',
                textShadow: "6px 6px 0px rgba(0,0,0,0.3)",
              }}
            >
              {roundName(roundBanner).toUpperCase()}
            </h1>
          </div>
        )}

        {/* Selection State - Show game board */}
        {gameState.state === "selection" && roundBanner === null && (
          <div className="bg-stone-900/90 backdrop-blur-sm rounded-lg p-6 border-4 border-stone-700">
            {gameState.roundCount > 1 && (
              <h2
                className="text-center text-3xl font-black text-white tracking-wide mb-4"
                style={{ fontFamily: 'Impact, "Arial Black", sans-serif' }}
              >
                {roundName(gameState.round).toUpperCase()}
              </h2>
            )}
            <div
              className="grid gap-4"
              style={{
                gridTemplateColumns: `repeat(${gameState.categories.length}, 1fr)`,
                // Categories can have different numbers of clues, so size the board for the
                // longest one and place every cell explicitly
                gridTemplateRows: `repeat(${
                  Math.max(0, ...gameState.categories.map((c) => c.questions.length)) + 1
                }, auto)`,
              }}
            >
              {gameState.categories.map((category, catIdx) => (
                <React.Fragment key={catIdx}>
                  <h3
                    className="text-center text-white font-bold text-lg uppercase py-4 bg-red-900/80 rounded"
                    style={{ gridColumn: catIdx + 1, gridRow: 1 }}
                  >
                    {category.title}
                  </h3>
                  {category.questions.map((question, qIdx) => (
                    <div
                      key={qIdx}
                      style={{ gridColumn: catIdx + 1, gridRow: qIdx + 2 }}
                      className={`w-full py-6 rounded font-bold text-2xl text-center ${
                        question.answered
                          ? "bg-stone-800 text-stone-600"
                          : "bg-red-600 text-white border-2 border-red-600"
                      }`}
                    >
                      {`$${question.value}`}
                    </div>
                  ))}
                </React.Fragment>
              ))}
            </div>
          </div>
        )}

        {/* Question Reading State */}
        {gameState.state === "questionReading" && gameState.currentQuestion && (
          <div className="flex flex-col items-center justify-center min-h-[80vh]">
            <p className="text-xl text-red-200 mb-4">
              {gameState.categories[gameState.currentQuestion[0]]?.title} - $
              {
                gameState.categories[gameState.currentQuestion[0]]?.questions[
                  gameState.currentQuestion[1]
                ]?.value
              }
            </p>
            <p className="text-5xl text-white text-center leading-relaxed mb-[4.25rem]">
              {
                gameState.categories[gameState.currentQuestion[0]]?.questions[
                  gameState.currentQuestion[1]
                ]?.question
              }
            </p>
          </div>
        )}

        {/* Waiting for Buzz State */}
        {gameState.state === "waitingForBuzz" && gameState.currentQuestion && (
          <div className="flex flex-col items-center justify-center min-h-[80vh]">
            <p className="text-xl text-red-200 mb-4">
              {gameState.categories[gameState.currentQuestion[0]]?.title} - $
              {
                gameState.categories[gameState.currentQuestion[0]]?.questions[
                  gameState.currentQuestion[1]
                ]?.value
              }
            </p>
            <p className="text-5xl text-white text-center leading-relaxed mb-8">
              {
                gameState.categories[gameState.currentQuestion[0]]?.questions[
                  gameState.currentQuestion[1]
                ]?.question
              }
            </p>
            <p className="text-3xl text-green-400 animate-pulse">
              Buzzers open!
            </p>
          </div>
        )}

        {/* Answer State - Show question and who buzzed, but NOT the answer */}
        {gameState.state === "answer" && gameState.currentQuestion && (
          <div className="flex flex-col items-center justify-center min-h-[80vh]">
            <p className="text-xl text-red-200 mb-4">
              {gameState.categories[gameState.currentQuestion[0]]?.title} - $
              {
                gameState.categories[gameState.currentQuestion[0]]?.questions[
                  gameState.currentQuestion[1]
                ]?.value
              }
            </p>
            <p className="text-5xl text-white text-center leading-relaxed mb-8">
              {
                gameState.categories[gameState.currentQuestion[0]]?.questions[
                  gameState.currentQuestion[1]
                ]?.question
              }
            </p>
            {buzzedPlayer && (
              <div className="bg-white/95 px-12 py-6 border-4 border-red-700 rounded-2xl shadow-2xl">
                <p className="text-4xl text-red-900 font-bold">
                  {buzzedPlayer.name}
                </p>
              </div>
            )}
          </div>
        )}

        {/* Answer Reveal State - Show the answer to the audience */}
        {gameState.state === "answerReveal" && gameState.currentQuestion && (
          <div className="flex flex-col items-center justify-center min-h-[80vh]">
            <p className="text-xl text-red-200 mb-4">
              {gameState.categories[gameState.currentQuestion[0]]?.title} - $
              {gameState.categories[gameState.currentQuestion[0]]?.questions[gameState.currentQuestion[1]]?.value}
            </p>

            {/* Show the answer prominently */}
            <div className="bg-white/20 outline outline-4 outline-white/40 rounded-2xl px-16 py-8 mb-8">
              <p className="text-xl text-green-100 mb-2">Answer:</p>
              <p className="text-5xl text-white font-bold text-center">
                {gameState.categories[gameState.currentQuestion[0]]?.questions[gameState.currentQuestion[1]]?.answer}
              </p>
            </div>

            <p className="text-2xl italic text-white text-center leading-relaxed mb-8">
              {gameState.categories[gameState.currentQuestion[0]]?.questions[gameState.currentQuestion[1]]?.question}
            </p>
          </div>
        )}

        {/* Game End State */}
        {gameState.state === "gameEnd" && (
          <div className="flex flex-col items-center justify-center min-h-[80vh]">
            <h2 className="text-5xl font-bold text-white mb-12">
              Game Over!
            </h2>
            <div className="space-y-4 w-full max-w-2xl">
              {[...gameState.players]
                .sort((a, b) => b.score - a.score)
                .map((player, idx) => (
                  <div
                    key={player.pid}
                    className={`p-6 rounded-lg flex justify-between items-center ${
                      idx === 0
                        ? "bg-red-700 text-white border-4 border-stone-700"
                        : "bg-stone-800/90 text-white border-2 border-stone-600"
                    }`}
                  >
                    <span className="text-3xl font-bold">
                      {idx + 1}. {player.name}
                    </span>
                    <span
                      className={`text-3xl ${
                        player.score < 0
                          ? "text-red-500"
                          : player.score === 0
                          ? "text-gray-500"
                          : ""
                      }`}
                    >
                      ${player.score}
                    </span>
                  </div>
                ))}
            </div>
          </div>
        )}

        {/* Scoreboard - Always visible at bottom */}
        {gameState.state !== "gameEnd" && (
          <div className="fixed bottom-0 left-0 right-0 p-4">
            <div className="max-w-6xl mx-auto flex justify-center gap-8">
              {gameState.players.map((player) => {
                const picking =
                  gameState.state === "selection" && gameState.picker === player.pid;
                return (
                  <div
                    key={player.pid}
                    className={`relative px-6 py-3 text-center rounded-lg transition-all duration-300 ${
                      buzzedPlayer?.pid === player.pid
                        ? "bg-white/95 text-red-900 border-4 border-red-700 rounded-lg scale-110"
                        : picking
                        ? "bg-stone-800 text-white border-4 border-yellow-400 rounded-lg scale-110"
                        : "bg-stone-800 text-white border-4 border-stone-600 rounded-lg"
                    }`}
                  >
                    {picking && (
                      <span className="absolute -top-4 left-1/2 -translate-x-1/2 px-2 py-0.5 rounded bg-yellow-400 text-stone-900 text-xs font-black tracking-widest">
                        PICKING
                      </span>
                    )}
                    <p className="font-semibold text-lg">{player.name}</p>
                    <p
                      className={`text-2xl font-bold ${
                        player.score < 0
                          ? "text-red-500"
                          : player.score === 0
                          ? "text-gray-500"
                          : ""
                      }`}
                    >
                      ${player.score}
                    </p>
                  </div>
                );
              })}
            </div>
          </div>
        )}
      </div>
    </div>
  );
}
