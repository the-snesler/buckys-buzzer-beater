import { useParams } from "react-router-dom";
import { useWebSocket } from "../hooks/useWebSocket";
import { useState } from "react";

export default function Host() {
  const { code } = useParams<{ code: string }>();
  const [playerList, setPlayerList] = useState(
    [] as Array<{ pid: number; name: string }>
  );
  const hostToken = sessionStorage.getItem(`host_token_${code}`);

  const { isConnected, sendMessage } = useWebSocket({
    roomCode: code!,
    token: hostToken!,
    autoConnect: true,
    onMessage: (message) => {
      console.log("Received message:", message);
      const [type, payload] = Object.entries(message)[0];

      switch (type) {
        case "PlayerList":
          setPlayerList(payload as any);
          break;
        // Handle other message types as needed
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
          <h1 className="text-3xl font-bold text-white">Room: {code}</h1>
          <div
            className={`px-3 py-1 rounded text-sm ${
              isConnected ? "bg-green-600" : "bg-red-600"
            } text-white`}
          >
            {isConnected ? "Connected" : "Disconnected"}
          </div>
        </div>

        <div className="bg-gray-800 rounded-lg p-6">
          <h2 className="text-2xl font-semibold text-white mb-4">Players</h2>
          {playerList.length === 0 ? (
            <p className="text-gray-400">No players have joined yet.</p>
          ) : (
            <ul className="space-y-2">
              {playerList.map((player) => (
                <li
                  key={player.pid}
                  className="bg-gray-700 rounded p-3 text-white"
                >
                  {player.name} (ID: {player.pid})
                </li>
              ))}
            </ul>
          )}
          {playerList.length >= 2 && (
            <button
              onClick={() => sendMessage({ BuzzEnable: {} })}
              className="mt-6 px-4 py-2 bg-blue-600 text-white rounded hover:bg-blue-700"
            >
              Start Game
            </button>
          )}
        </div>
      </div>
    </div>
  );
}
