import { Compass, Home, Library, Music, Plus, Settings } from "lucide-react";
import { useLocation, useNavigate } from "react-router-dom";

export const Sidebar: React.FC = () => {
  const navigate = useNavigate();
  const location = useLocation();

  const mainNav = [
    { label: "Home", icon: Home, path: "/" },
    { label: "Explore", icon: Compass, path: "/explore" },
    { label: "Library", icon: Library, path: "/library" },
    { label: "Setting", icon: Settings, path: "/setting" },
  ];

  const playlists = [
    "Liked Songs",
    "Coding & Chill",
    "Top Hits 2026",
    "Lo-Fi Beats",
    "Acoustic Evening",
  ];

  return (
    <aside className="flex w-60 flex-col border-r border-[#30363d] bg-[#010409] p-4 pb-24 shrink-0 select-none">
      {/* App Logo */}
      <div className="flex items-center gap-3 px-2 py-3 mb-2">
        <div className="grid h-8 w-8 place-items-center rounded-lg bg-gradient-to-tr from-[#a371f7] to-[#58a6ff] text-white font-bold">
          <Music size={18} />
        </div>
        <span className="text-lg font-bold tracking-tight text-[#e6edf3]">
          Melody
        </span>
      </div>

      {/* Main Navigation */}
      <nav className="flex flex-col gap-1">
        {mainNav.map((item) => {
          const Icon = item.icon;
          const isActive = location.pathname === item.path;
          return (
            <button
              key={item.label}
              onClick={() => navigate(item.path)}
              className={`flex items-center gap-3 rounded-md px-3 py-2 text-sm font-medium transition ${
                isActive
                  ? "bg-[#21262d] text-[#58a6ff]"
                  : "text-[#8b949e] hover:bg-[#161b22] hover:text-[#e6edf3]"
              }`}
            >
              <Icon size={18} />
              <span>{item.label}</span>
            </button>
          );
        })}
      </nav>

      <hr className="my-4 border-[#30363d]" />

      {/* Playlists Header */}
      <div className="flex items-center justify-between px-3 text-xs font-semibold uppercase tracking-wider text-[#8b949e]">
        <span>Playlists</span>
        <button className="rounded p-1 hover:bg-[#21262d] hover:text-[#e6edf3] transition">
          <Plus size={16} />
        </button>
      </div>

      {/* Playlists List */}
      <div className="mt-2 flex-1 overflow-y-auto space-y-1 pr-1">
        {playlists.map((playlist) => (
          <button
            key={playlist}
            className="block w-full truncate rounded-md px-3 py-1.5 text-left text-sm text-[#8b949e] hover:bg-[#161b22] hover:text-[#e6edf3] transition"
          >
            {playlist}
          </button>
        ))}
      </div>
    </aside>
  );
};
