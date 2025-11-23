import { Routes, Route } from 'react-router-dom';
import Lobby from './pages/Lobby';
import Host from "./pages/Host";
import HostDisplay from "./pages/HostDisplay";
import Player from "./pages/Player";
import GameBuilder from "./pages/GameBuilder";

export default function App() {
  return (
    <Routes>
      <Route path="/" element={<Lobby />} />
      <Route path="/create" element={<GameBuilder />} />
      <Route path="/host/:code" element={<Host />} />
      <Route path="/host/:code/display" element={<HostDisplay />} />
      <Route path="/play/:code" element={<Player />} />
    </Routes>
  );
}
