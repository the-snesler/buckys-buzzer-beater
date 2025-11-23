import { useState, useEffect } from 'react';
import { useNavigate, useLocation, Link } from 'react-router-dom';
import { createRoom, type Category } from '../lib/api';

interface LocationState {
  categories?: Category[];
  fromBuilder?: boolean;
}

export default function Lobby() {
  const navigate = useNavigate();
  const location = useLocation();
  const [roomCode, setRoomCode] = useState('');
  const [playerName, setPlayerName] = useState("");
  const [isCreating, setIsCreating] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [categories, setCategories] = useState<Category[] | null>(null);
  const [fileName, setFileName] = useState<string | null>(null);
  const [fromBuilder, setFromBuilder] = useState(false);

  // Load categories from navigation state (from GameBuilder)
  useEffect(() => {
    const state = location.state as LocationState | null;
    if (state?.categories && state?.fromBuilder) {
      setCategories(state.categories);
      setFromBuilder(true);
      setFileName(null);
      // Clear navigation state to prevent reloading on refresh
      window.history.replaceState({}, document.title);
    }
  }, [location.state]);

  const handleFileUpload = (e: React.ChangeEvent<HTMLInputElement>) => {
    const file = e.target.files?.[0];
    if (!file) return;

    setFileName(file.name);
    setFromBuilder(false);
    setError(null);

    const reader = new FileReader();
    reader.onload = (event) => {
      try {
        const json = JSON.parse(event.target?.result as string);

        // Validate structure
        if (!json.game?.single || !Array.isArray(json.game.single)) {
          throw new Error("Invalid format: expected game.single array");
        }

        // Transform to Category[] format
        const transformed: Category[] = json.game.single.map((cat: { category: string; clues: { value: number; clue: string; solution: string }[] }) => {
          if (!cat.category || !Array.isArray(cat.clues)) {
            throw new Error("Invalid category format");
          }

          return {
            title: cat.category,
            questions: cat.clues.map((clue) => {
              if (typeof clue.value !== 'number' || !clue.clue || !clue.solution) {
                throw new Error("Invalid clue format");
              }
              return {
                question: clue.clue,
                answer: clue.solution,
                value: clue.value,
                answered: false,
              };
            }),
          };
        });

        setCategories(transformed);
      } catch (err) {
        setError(err instanceof Error ? err.message : "Invalid JSON file");
        setCategories(null);
      }
    };
    reader.readAsText(file);
  };

  const handleJoin = (e: React.FormEvent) => {
    e.preventDefault();
    if (roomCode.length === 6 && playerName.trim()) {
      sessionStorage.setItem(`player_name`, playerName);
      navigate(`/play/${roomCode.toUpperCase()}`);
    }
  };

  const handleCreate = async () => {
    if (!categories) return;
    setIsCreating(true);
    setError(null);
    try {
      const { roomCode, hostToken } = await createRoom({
        categories,
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
            className="w-full mt-4 px-4 py-3 bg-red-600 text-white rounded font-semibold hover:bg-red-700 disabled:opacity-50 disabled:cursor-not-allowed"
          >
            Join Game
          </button>
        </form>

        <div className="border-t border-gray-700 pt-6">
          <div className="flex items-center justify-between mb-2">
            <label className="block text-gray-300">Game File</label>
            <Link
              to="/create"
              className="text-blue-400 hover:text-blue-300 text-sm"
            >
              or create your own
            </Link>
          </div>
          <input
            type="file"
            accept=".json"
            onChange={handleFileUpload}
            className="w-full px-4 py-3 rounded bg-gray-700 text-white mb-2 file:mr-4 file:py-2 file:px-4 file:rounded file:border-0 file:bg-gray-600 file:text-white file:cursor-pointer"
          />
          {categories && (
            <p className="text-green-400 text-sm mb-2">
              Loaded {categories.length} categories from {fromBuilder ? 'Game Builder' : fileName}
            </p>
          )}
          <button
            onClick={handleCreate}
            disabled={isCreating || !categories}
            className="w-full px-4 py-3 bg-green-600 text-white rounded font-semibold hover:bg-green-700 disabled:opacity-50 disabled:cursor-not-allowed"
          >
            {isCreating ? "Creating..." : "Create Room"}
          </button>
        </div>

        {error && <p className="mt-4 text-red-400 text-center">{error}</p>}
      </div>
    </div>
  );
}
