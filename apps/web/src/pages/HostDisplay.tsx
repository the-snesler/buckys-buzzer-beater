import { useParams } from "react-router-dom";
import { useState, useEffect } from "react";
import React from "react";

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

interface DisplayMessage {
  gameState: GameState | null;
  buzzedPlayer: { pid: number; name: string } | null;
}

export default function HostDisplay() {
  const { code } = useParams<{ code: string }>();
  const [gameState, setGameState] = useState<GameState | null>(null);
  const [buzzedPlayer, setBuzzedPlayer] = useState<{
    pid: number;
    name: string;
  } | null>(null);

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
      }
    };
    // post message to let host know we're ready to receive updates

    window.addEventListener("message", handleMessage);
    window.opener.postMessage({ type: "displayReady" }, "*");
    return () => window.removeEventListener("message", handleMessage);
  }, []);

  // Waiting for connection from host window
  if (!gameState) {
    return (
      <div className="min-h-screen flex items-center justify-center bg-blue-900">
        <div className="text-white text-center">
          <h1 className="text-4xl font-bold mb-4">Room: {code}</h1>
          <p className="text-2xl text-blue-300 animate-pulse">
            Waiting for host to connect...
          </p>
        </div>
      </div>
    );
  }

  return (
    <div className="min-h-screen flex items-stretch pb-24 justify-stretch bg-blue-900 p-8">
      <div className="m-8 w-full h-full">
        {/* Selection State - Show game board */}
        {gameState.state === "selection" && (
          <div className="bg-blue-800 rounded-lg p-6">
            <div
              className="grid gap-4 grid-flow-col "
              style={{
                gridTemplateColumns: `repeat(${gameState.categories.length}, 1fr)`,
                gridTemplateRows: `repeat(${
                  gameState.categories[0].questions.length + 1
                }, auto)`,
              }}
            >
              {gameState.categories.map((category, catIdx) => (
                <React.Fragment key={catIdx}>
                  <h3 className="text-center text-yellow-400 font-bold text-lg uppercase py-4 bg-blue-700 rounded">
                    {category.title}
                  </h3>
                  {category.questions.map((question, qIdx) => (
                    <div
                      key={qIdx}
                      className={`w-full py-6 rounded font-bold text-2xl text-center ${
                        question.answered
                          ? "bg-blue-900 text-blue-700"
                          : "bg-blue-600 text-yellow-400"
                      }`}
                    >
                      {question.answered ? "" : `$${question.value}`}
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
            <p className="text-xl text-yellow-400 mb-4">
              {gameState.categories[gameState.currentQuestion[0]]?.title} - $
              {
                gameState.categories[gameState.currentQuestion[0]]?.questions[
                  gameState.currentQuestion[1]
                ]?.value
              }
            </p>
            <p className="text-5xl text-white text-center leading-relaxed">
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
            <p className="text-xl text-yellow-400 mb-4">
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
            <p className="text-xl text-yellow-400 mb-4">
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
              <div className="bg-yellow-500 px-8 py-4 rounded-lg">
                <p className="text-4xl text-black font-bold">
                  {buzzedPlayer.name}
                </p>
              </div>
            )}
          </div>
        )}

        {/* Game End State */}
        {gameState.state === "gameEnd" && (
          <div className="flex flex-col items-center justify-center min-h-[80vh]">
            <h2 className="text-5xl font-bold text-yellow-400 mb-12">
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
                        ? "bg-yellow-500 text-black"
                        : "bg-blue-700 text-white"
                    }`}
                  >
                    <span className="text-3xl font-bold">
                      {idx + 1}. {player.name}
                    </span>
                    <span className="text-3xl">${player.score}</span>
                  </div>
                ))}
            </div>
          </div>
        )}

        {/* Scoreboard - Always visible at bottom */}
        {gameState.state !== "gameEnd" && (
          <div className="fixed bottom-0 left-0 right-0 bg-blue-950 p-4">
            <div className="max-w-6xl mx-auto flex justify-center gap-8">
              {gameState.players.map((player) => (
                <div
                  key={player.pid}
                  className={`px-6 py-3 rounded text-center ${
                    buzzedPlayer?.pid === player.pid
                      ? "bg-yellow-500 text-black"
                      : "bg-blue-800 text-white"
                  }`}
                >
                  <p className="font-semibold text-lg">{player.name}</p>
                  <p className="text-2xl font-bold">${player.score}</p>
                </div>
              ))}
            </div>
          </div>
        )}
      </div>
    </div>
  );
}
