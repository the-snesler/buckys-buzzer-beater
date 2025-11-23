import { useState } from 'react';
import { useNavigate } from 'react-router-dom';
import { createRoom, type Category } from '../lib/api';

// Sample game configuration for testing
const SAMPLE_CATEGORIES: Category[] = [
  {
    title: "Science",
    questions: [
      { question: "What is the chemical symbol for water?", answer: "H2O", value: 100, answered: false },
      { question: "What planet is known as the Red Planet?", answer: "Mars", value: 200, answered: false },
      { question: "What is the powerhouse of the cell?", answer: "Mitochondria", value: 300, answered: false },
      { question: "What is the speed of light in vacuum (in m/s)?", answer: "299,792,458", value: 400, answered: false },
      { question: "What is the atomic number of Carbon?", answer: "6", value: 500, answered: false },
    ],
  },
  {
    title: "History",
    questions: [
      { question: "In what year did World War II end?", answer: "1945", value: 100, answered: false },
      { question: "Who was the first President of the United States?", answer: "George Washington", value: 200, answered: false },
      { question: "What ancient wonder was located in Alexandria?", answer: "The Lighthouse (Pharos)", value: 300, answered: false },
      { question: "What year did the Berlin Wall fall?", answer: "1989", value: 400, answered: false },
      { question: "Who was the first Emperor of Rome?", answer: "Augustus", value: 500, answered: false },
    ],
  },
  {
    title: "Geography",
    questions: [
      { question: "What is the capital of France?", answer: "Paris", value: 100, answered: false },
      { question: "What is the longest river in the world?", answer: "The Nile", value: 200, answered: false },
      { question: "What is the smallest country in the world?", answer: "Vatican City", value: 300, answered: false },
      { question: "What mountain range separates Europe from Asia?", answer: "The Ural Mountains", value: 400, answered: false },
      { question: "What is the deepest ocean trench?", answer: "Mariana Trench", value: 500, answered: false },
    ],
  },
  {
    title: "Pop Culture",
    questions: [
      { question: "What is the name of Harry Potter's owl?", answer: "Hedwig", value: 100, answered: false },
      { question: "Who directed the movie 'Inception'?", answer: "Christopher Nolan", value: 200, answered: false },
      { question: "What band performed 'Bohemian Rhapsody'?", answer: "Queen", value: 300, answered: false },
      { question: "What is the highest-grossing film of all time?", answer: "Avatar", value: 400, answered: false },
      { question: "Who wrote the 'A Song of Ice and Fire' series?", answer: "George R.R. Martin", value: 500, answered: false },
    ],
  },
];

export default function Lobby() {
  const navigate = useNavigate();
  const [roomCode, setRoomCode] = useState('');
  const [playerName, setPlayerName] = useState("");
  const [isCreating, setIsCreating] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const handleJoin = (e: React.FormEvent) => {
    e.preventDefault();
    if (roomCode.length === 6 && playerName.trim()) {
      sessionStorage.setItem(`player_name`, playerName);
      navigate(`/play/${roomCode.toUpperCase()}`);
    }
  };

  const handleCreate = async () => {
    setIsCreating(true);
    setError(null);
    try {
      const { roomCode, hostToken } = await createRoom({
        categories: SAMPLE_CATEGORIES,
      });
      // Store host token for WebSocket auth
      sessionStorage.setItem(`host_token_${roomCode}`, hostToken);
      navigate(`/host/${roomCode}`);
    } catch (err) {
      setError("Failed to create room");
    } finally {
      setIsCreating(false);
    }
  };

  return (
    <div className="min-h-screen flex flex-col items-center justify-center bg-gray-900">
      <div className="bg-gray-800 p-8 rounded-lg shadow-xl w-lg">
        <h1 className="text-3xl font-bold text-white text-center mb-8">
          Bucky's Buzzer Banger
        </h1>

        <form onSubmit={handleJoin} className="mb-6">
          <div className="grid grid-cols-2 gap-4">
            <div>
              <label className="block text-gray-300 mb-2">Code</label>
              <input
                type="text"
                value={roomCode}
                onChange={(e) => setRoomCode(e.target.value.toUpperCase())}
                maxLength={6}
                placeholder="67ABCD"
                className="w-full px-4 py-3 rounded bg-gray-700 text-white text-center text-2xl tracking-widest uppercase mb-4"
              />
            </div>
            <div>
              <label className="block text-gray-300 mb-2">Name</label>
              <input
                type="text"
                value={playerName}
                onChange={(e) => setPlayerName(e.target.value)}
                placeholder="Your Name"
                className="w-full px-4 py-3 rounded bg-gray-700 text-white text-center text-2xl"
              />
            </div>
          </div>
          <button
            type="submit"
            disabled={roomCode.length !== 6 || !playerName.trim()}
            className="w-full mt-4 px-4 py-3 bg-blue-600 text-white rounded font-semibold hover:bg-blue-700 disabled:opacity-50 disabled:cursor-not-allowed"
          >
            Join Game
          </button>
        </form>

        <div className="border-t border-gray-700 pt-6">
          <button
            onClick={handleCreate}
            disabled={isCreating}
            className="w-full px-4 py-3 bg-green-600 text-white rounded font-semibold hover:bg-green-700 disabled:opacity-50"
          >
            {isCreating ? "Creating..." : "Create Room"}
          </button>
        </div>

        {error && <p className="mt-4 text-red-400 text-center">{error}</p>}
      </div>
    </div>
  );
}
