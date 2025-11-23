import { useState } from 'react';
import { useNavigate } from 'react-router-dom';
import { createRoom } from '../lib/api';

export default function Lobby() {
  const navigate = useNavigate();
  const [roomCode, setRoomCode] = useState('');
  const [isCreating, setIsCreating] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const handleJoin = (e: React.FormEvent) => {
    e.preventDefault();
    if (roomCode.length === 4) {
      navigate(`/play/${roomCode.toUpperCase()}`);
    }
  };

  const handleCreate = async () => {
    setIsCreating(true);
    setError(null);
    try {
      const { roomCode, hostToken } = await createRoom();
      // Store host token for WebSocket auth
      sessionStorage.setItem(`host_token_${roomCode}`, hostToken);
      navigate(`/host/${roomCode}`);
    } catch (err) {
      setError('Failed to create room');
    } finally {
      setIsCreating(false);
    }
  };

  return (
    <div className="min-h-screen flex items-center justify-center bg-gray-900">
      <div className="bg-gray-800 p-8 rounded-lg shadow-xl max-w-md w-full">
        <h1 className="text-3xl font-bold text-white text-center mb-8">
          Bucky's Buzzer Banger
        </h1>

        <form onSubmit={handleJoin} className="mb-6">
          <label className="block text-gray-300 mb-2">Room Code</label>
          <input
            type="text"
            value={roomCode}
            onChange={(e) => setRoomCode(e.target.value.toUpperCase())}
            maxLength={4}
            placeholder="ABCD"
            className="w-full px-4 py-3 rounded bg-gray-700 text-white text-center text-2xl tracking-widest uppercase"
          />
          <button
            type="submit"
            disabled={roomCode.length !== 4}
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
