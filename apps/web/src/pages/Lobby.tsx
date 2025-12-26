import { useState, useEffect } from 'react';
import { useNavigate, useLocation, useSearchParams, Link } from 'react-router-dom';
import { createRoom, type Category } from '../lib/api';

interface LocationState {
  categories?: Category[];
  fromBuilder?: boolean;
}

export default function Lobby() {
  const navigate = useNavigate();
  const location = useLocation();
  const [searchParams] = useSearchParams();
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

  // Pre-fill room code from URL query parameter
  useEffect(() => {
    const codeParam = searchParams.get('code');
    if (codeParam && codeParam.length === 6) {
      setRoomCode(codeParam.toUpperCase());
    }
  }, [searchParams]);

  const loadDefaultGame = async () => {
    setError(null);
    try {
      const response = await fetch('/formatted_game.json');
      const json = await response.json();

      const transformed: Category[] = json.game.single.map((cat: { category: string; clues: { value: number; clue: string; solution: string }[] }) => ({
        title: cat.category,
        questions: cat.clues.map((clue) => ({
          question: clue.clue,
          answer: clue.solution,
          value: clue.value,
          answered: false,
        })),
      }));

      setCategories(transformed);
      setFileName('Default Game');
      setFromBuilder(false);
    } catch (err) {
      setError('Failed to load default game');
    }
  };

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
    <div className="min-h-screen py-8 md:py-24 flex flex-col items-center justify-center fancy-bg relative overflow-hidden">
      <div className="relative z-10 w-full max-w-2xl px-6">
        {/* Icon and Title Section */}
        <div className="text-center mb-12">
          <div className="flex items-center justify-center gap-4 md:gap-8 mb-6">
            <img
              src="/bucky.svg"
              alt="Bucky"
              className="hidden md:block w-20 h-20 opacity-80 hover:scale-125 transition-transform duration-300"
            />
            <img
              src="/apple-touch-icon.png"
              alt="Bucky's Buzzer"
              className="w-24 md:w-32 h-24 md:h-32 drop-shadow-2xl hover:scale-110 hover:rotate-3 transition-transform rounded-xl border-t border-1 border-red-600 duration-300"
            />
            <img
              src="/bucky.svg"
              alt="Bucky"
              className="hidden md:block w-20 h-20 opacity-80 hover:scale-125 transition-transform duration-300"
              style={{ transform: "scaleX(-1)" }}
            />
          </div>
          <h1
            className="text-4xl md:text-6xl font-black text-white mb-3 tracking-tight"
            style={{
              fontFamily: 'Impact, "Arial Black", sans-serif',
              textShadow: "4px 4px 0px rgba(0,0,0,0.3)",
            }}
          >
            BUCKY'S
          </h1>
          <h2
            className="text-3xl md:text-5xl font-black text-red-200 tracking-wide"
            style={{
              fontFamily: 'Impact, "Arial Black", sans-serif',
              textShadow: "3px 3px 0px rgba(0,0,0,0.3)",
            }}
          >
            BUZZER BEATER
          </h2>
          <div className="h-1 w-32 bg-white mx-auto mt-4 rounded-full"></div>
        </div>

        {/* Join Game Card */}
        <div className="bg-white/95 backdrop-blur-sm rounded-2xl shadow-2xl p-4 md:p-8 mb-6 border-4 border-red-700 transform transition-transform">
          <h3
            className="text-2xl font-black text-red-900 mb-6 tracking-wide"
            style={{ fontFamily: 'Impact, "Arial Black", sans-serif' }}
          >
            JOIN A GAME
          </h3>
          <form onSubmit={handleJoin}>
            <div className="grid grid-cols-1 md:grid-cols-2 gap-4 mb-6">
              <div>
                <label className="block text-red-900 font-bold mb-2 text-sm uppercase tracking-wider">
                  Room Code
                </label>
                <input
                  type="text"
                  value={roomCode}
                  onChange={(e) => setRoomCode(e.target.value.toUpperCase())}
                  maxLength={6}
                  placeholder="ABC123"
                  className="w-full px-4 py-4 rounded-xl bg-stone-100 text-red-900 text-center text-3xl font-black tracking-widest uppercase border-3 border-stone-300 focus:border-red-500 focus:outline-none focus:ring-4 focus:ring-red-200 transition-all"
                  style={{ fontFamily: 'Consolas, "Courier New", monospace' }}
                />
              </div>
              <div>
                <label className="block text-red-900 font-bold mb-2 text-sm uppercase tracking-wider">
                  Your Name
                </label>
                <input
                  type="text"
                  value={playerName}
                  onChange={(e) => setPlayerName(e.target.value)}
                  placeholder="Player Name"
                  className="w-full px-4 py-4 rounded-xl bg-stone-100 text-red-900 text-center text-2xl font-bold border-3 border-stone-300 focus:border-red-500 focus:outline-none focus:ring-4 focus:ring-red-200 transition-all"
                />
              </div>
            </div>
            <button
              type="submit"
              disabled={roomCode.length !== 6 || !playerName.trim()}
              className="w-full py-5 bg-gradient-to-r from-red-600 to-red-700 text-white rounded-xl font-black text-xl uppercase tracking-wider shadow-lg hover:shadow-2xl hover:from-red-700 hover:to-red-800 disabled:opacity-50 disabled:cursor-not-allowed transform hover:scale-105 transition-all duration-200 border-b-4 border-red-900 active:border-b-0 active:mt-1 flex items-center justify-center gap-3"
              style={{ fontFamily: 'Impact, "Arial Black", sans-serif' }}
            >
              <img src="/buzzer.svg" alt="" className="w-6 h-6 opacity-90" />
              Join Game
            </button>
          </form>
        </div>

        {/* Host Game Card */}
        <div className="bg-stone-900/90 backdrop-blur-sm rounded-2xl shadow-2xl p-4 md:p-8 border-4 border-stone-700 transform transition-transform">
          <h3
            className="text-2xl font-black text-white mb-6 tracking-wide"
            style={{ fontFamily: 'Impact, "Arial Black", sans-serif' }}
          >
            HOST A GAME
          </h3>

          <div className="mb-6">
            <div className="flex flex-col md:flex-row md:items-center md:justify-between mb-3 gap-2">
              <label className="text-white font-bold text-sm uppercase tracking-wider">
                Choose Game
              </label>
              <div className="flex gap-3 text-sm">
                <button
                  type="button"
                  onClick={loadDefaultGame}
                  className="text-red-300 hover:text-red-200 font-semibold underline decoration-2 underline-offset-2 transition-colors"
                >
                  Load Default
                </button>
                <span className="text-stone-500">•</span>
                <Link
                  to="/create"
                  className="text-red-300 hover:text-red-200 font-semibold underline decoration-2 underline-offset-2 transition-colors"
                >
                  Create Custom
                </Link>
              </div>
            </div>

            <input
              type="file"
              accept=".json"
              onChange={handleFileUpload}
              className="w-full px-4 py-4 rounded-xl bg-stone-800 text-white border-2 border-stone-600 focus:border-red-400 focus:outline-none transition-all file:mr-4 file:py-2 file:px-6 file:rounded-lg file:border-0 file:bg-red-600 file:text-white file:font-bold file:cursor-pointer file:hover:bg-red-700 file:transition-colors"
            />

            {categories && (
              <div className="mt-3 flex items-center gap-2 text-green-300 bg-green-900/30 border-2 border-green-700 rounded-lg px-4 py-3">
                <span className="text-xl flex-shrink-0">✓</span>
                <p className="font-semibold break-words">
                  Loaded {categories.length} categories from{" "}
                  <span className="font-black break-all">
                    {fromBuilder ? "Game Builder" : fileName}
                  </span>
                </p>
              </div>
            )}
          </div>

          <button
            onClick={handleCreate}
            disabled={isCreating || !categories}
            className="w-full py-5 bg-gradient-to-r from-green-600 to-green-700 text-white rounded-xl font-black text-xl uppercase tracking-wider shadow-lg hover:shadow-2xl hover:from-green-700 hover:to-green-800 disabled:opacity-50 disabled:cursor-not-allowed disabled:hover:scale-100 transform hover:scale-105 transition-all duration-200 border-b-4 border-green-900 active:border-b-0 active:mt-1 flex items-center justify-center gap-3"
            style={{ fontFamily: 'Impact, "Arial Black", sans-serif' }}
          >
            {isCreating ? (
              <>
                <span className="inline-block animate-spin">⏳</span>
                Creating Room...
              </>
            ) : (
              <>
                <img src="/buzzer.svg" alt="" className="w-7 h-7 opacity-90" />
                Create Room
              </>
            )}
          </button>
        </div>

        {error && (
          <div className="mt-6 bg-red-600/90 backdrop-blur-sm border-4 border-red-800 rounded-xl px-6 py-4 text-center">
            <p className="text-white font-bold text-lg">⚠️ {error}</p>
          </div>
        )}
      </div>
    </div>
  );
}
