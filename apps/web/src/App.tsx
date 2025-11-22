import { Routes, Route } from 'react-router-dom';
import Lobby from './pages/Lobby';

export default function App() {
  return (
    <Routes>
      <Route path="/" element={<Lobby />} />
      {/* <Route path="/host/:code" element={<Host />} />
      <Route path="/play/:code" element={<Player />} /> */}
    </Routes>
  );
}
