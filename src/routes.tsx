import { Route, Routes } from "react-router-dom";
import HomePage from "./pages/HomePage";
import NotFoundPage from "./pages/NotFoundPage";
import SettingPage from "./pages/SettingPage";
import PlaylistPage from "./pages/PlaylistPage";

export function AppRoutes() {
  return (
    <Routes>
      <Route path="/" element={<HomePage />} />
      <Route path="/setting" element={<SettingPage />} />
      <Route path="/playlist/:id" element={<PlaylistPage />} />
      <Route path="*" element={<NotFoundPage />} />
    </Routes>
  );
}
