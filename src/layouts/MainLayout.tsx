// src/components/Layout.tsx
import React, { useEffect, useRef, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { invoke } from "@tauri-apps/api/core";
import { useNavigate, useLocation, useSearchParams } from "react-router-dom";
import {
  ChevronLeft,
  ChevronRight,
  Search,
  Heart,
  Shuffle,
  SkipBack,
  Play,
  Pause,
  SkipForward,
  Repeat,
  Mic2,
  ListMusic,
  Volume2,
  Volume1,
  VolumeX,
  Home,
  Compass,
  Library,
  Plus,
  Music,
  Settings,
} from "lucide-react";

interface LayoutProps {
  children: React.ReactNode;
}

const Layout: React.FC<LayoutProps> = ({ children }) => {
  return (
    <div className="flex h-screen overflow-hidden bg-[#0d1117] text-[#c9d1d9]">
      {/* Sidebar Navigation */}
      <Sidebar />

      {/* Main Content Area */}
      <div className="flex flex-1 flex-col overflow-hidden">
        <Header />

        <main className="flex-1 overflow-y-auto bg-gradient-to-b from-[#161b22] to-[#0d1117] px-6 pb-28 pt-4">
          {children}
        </main>
      </div>

      {/* Persistent Player */}
      <BottomPlayerBar />
    </div>
  );
};

export default Layout;

/* ---------------- Sidebar ---------------- */
const Sidebar: React.FC = () => {
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

/* ---------------- Header ---------------- */
const Header: React.FC = () => {
  const navigate = useNavigate();
  const [searchParams] = useSearchParams();
  const [search, setSearch] = useState(searchParams.get("q") ?? "");
  const [debouncedSearch, setDebouncedSearch] = useState("");
  const [showSuggestions, setShowSuggestions] = useState(false);
  const searchContainerRef = useRef<HTMLFormElement>(null);

  const suggestionsQuery = useQuery({
    queryKey: ["youtube-suggestions", debouncedSearch],
    queryFn: () => invoke<string[]>("youtube_suggestions", { query: debouncedSearch }),
    enabled: debouncedSearch.length >= 2 && showSuggestions,
    staleTime: 60_000,
    refetchOnWindowFocus: false,
  });
  const suggestions = suggestionsQuery.data ?? [];

  useEffect(() => {
    const timeout = window.setTimeout(() => {
      setDebouncedSearch(search.trim());
    }, 250);
    return () => window.clearTimeout(timeout);
  }, [search]);

  useEffect(() => {
    const handleOutsideClick = (event: MouseEvent) => {
      if (
        searchContainerRef.current &&
        !searchContainerRef.current.contains(event.target as Node)
      ) {
        setShowSuggestions(false);
      }
    };
    const handleEscape = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        setShowSuggestions(false);
      }
    };

    document.addEventListener("mousedown", handleOutsideClick);
    document.addEventListener("keydown", handleEscape);
    return () => {
      document.removeEventListener("mousedown", handleOutsideClick);
      document.removeEventListener("keydown", handleEscape);
    };
  }, []);

  const submitSearch = (event: React.FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    const query = search.trim();
    setShowSuggestions(false);
    navigate(query ? `/?q=${encodeURIComponent(query)}` : "/");
  };

  return (
    <header className="sticky top-0 z-20 flex items-center justify-between border-b border-[#30363d] bg-[#010409]/80 px-6 py-3 backdrop-blur-md">
      <div className="flex items-center gap-2">
        <button 
          onClick={() => navigate(-1)}
          className="grid h-8 w-8 place-items-center rounded-full bg-[#21262d] hover:bg-[#30363d] transition text-[#c9d1d9]"
        >
          <ChevronLeft size={18} />
        </button>
        <button 
          onClick={() => navigate(1)}
          className="grid h-8 w-8 place-items-center rounded-full bg-[#21262d] hover:bg-[#30363d] transition text-[#c9d1d9]"
        >
          <ChevronRight size={18} />
        </button>
      </div>

      <form
        className="relative w-full max-w-sm"
        onSubmit={submitSearch}
        ref={searchContainerRef}
      >
        <Search
          size={16}
          className="absolute left-3 top-1/2 -translate-y-1/2 text-[#8b949e]"
        />
        <input
          type="text"
          placeholder="What do you want to play?"
          className="w-full rounded-full border border-[#30363d] bg-[#161b22] py-2 pl-9 pr-4 text-sm text-[#c9d1d9] placeholder-[#8b949e] outline-none focus:ring-2 focus:ring-[#58a6ff]"
          onChange={(event) => {
            setSearch(event.currentTarget.value);
            setShowSuggestions(true);
          }}
          onFocus={() => setShowSuggestions(true)}
          value={search}
        />
        {showSuggestions && suggestions.length > 0 && (
          <div className="absolute left-0 right-0 top-full mt-2 overflow-hidden rounded-lg border border-[#30363d] bg-[#161b22] shadow-xl">
            {suggestions.map((suggestion) => (
              <button
                className="block w-full px-4 py-2 text-left text-sm text-[#c9d1d9] hover:bg-[#21262d]"
                key={suggestion}
                onClick={() => {
                  setSearch(suggestion);
                  setShowSuggestions(false);
                  navigate(`/?q=${encodeURIComponent(suggestion)}`);
                }}
                type="button"
              >
                {suggestion}
              </button>
            ))}
          </div>
        )}
      </form>

      <div className="flex items-center gap-3">
        <div className="h-8 w-8 rounded-full bg-gradient-to-br from-[#58a6ff] to-[#1f6feb]" />
      </div>
    </header>
  );
};

/* ---------------- Bottom Player Bar ---------------- */
const BottomPlayerBar: React.FC = () => {
  const [isPlaying, setIsPlaying] = useState(false);
  const [volume, setVolume] = useState(70);

  const handleVolumeChange = (e: React.ChangeEvent<HTMLInputElement>) => {
    setVolume(Number(e.target.value));
  };

  const VolumeIcon = volume === 0 ? VolumeX : volume < 50 ? Volume1 : Volume2;

  return (
    <footer className="fixed bottom-0 left-0 right-0 z-30 flex h-20 items-center justify-between border-t border-[#30363d] bg-[#161b22]/95 px-4 backdrop-blur-md">
      {/* Track Info */}
      <div className="flex w-1/4 min-w-[180px] items-center gap-3">
        <div className="h-12 w-12 rounded-md bg-gradient-to-br from-[#1f6feb] to-[#8957e5]" />
        <div>
          <p className="text-sm font-medium text-[#e6edf3]">Song Title</p>
          <p className="text-xs text-[#8b949e]">Artist Name</p>
        </div>
        <button className="ml-2 text-[#8b949e] hover:text-[#f778ba] transition">
          <Heart size={16} />
        </button>
      </div>

      {/* Player Controls */}
      <div className="flex w-1/2 max-w-xl flex-col items-center gap-1">
        <div className="flex items-center gap-5">
          <button className="text-[#8b949e] hover:text-[#e6edf3] transition">
            <Shuffle size={18} />
          </button>
          <button className="text-[#c9d1d9] hover:text-[#e6edf3] transition">
            <SkipBack size={20} fill="currentColor" />
          </button>

          <button
            onClick={() => setIsPlaying(!isPlaying)}
            className="grid h-9 w-9 place-items-center rounded-full bg-[#a371f7] text-white hover:bg-[#b78af2] hover:scale-105 transition shadow-md shadow-[#a371f7]/30"
          >
            {isPlaying ? (
              <Pause size={16} fill="white" />
            ) : (
              <Play size={16} fill="white" className="ml-0.5" />
            )}
          </button>

          <button className="text-[#c9d1d9] hover:text-[#e6edf3] transition">
            <SkipForward size={20} fill="currentColor" />
          </button>
          <button className="text-[#8b949e] hover:text-[#e6edf3] transition">
            <Repeat size={18} />
          </button>
        </div>
        <div className="flex w-full items-center gap-2 text-xs text-[#8b949e]">
          <span>1:24</span>
          <div className="h-1 flex-1 rounded-full bg-[#30363d] group cursor-pointer">
            <div className="h-1 w-1/3 rounded-full bg-[#58a6ff] group-hover:bg-[#79c0ff] transition" />
          </div>
          <span>3:45</span>
        </div>
      </div>

      {/* Volume / Extra */}
      <div className="flex w-1/4 min-w-[180px] items-center justify-end gap-3 text-[#8b949e]">
        <button className="hover:text-[#e6edf3] transition">
          <Mic2 size={16} />
        </button>
        <button className="hover:text-[#e6edf3] transition">
          <ListMusic size={16} />
        </button>

        <button
          onClick={() => setVolume(volume === 0 ? 70 : 0)}
          className="hover:text-[#e6edf3] transition"
        >
          <VolumeIcon size={16} />
        </button>

        <input
          type="range"
          min={0}
          max={100}
          value={volume}
          onChange={handleVolumeChange}
          className="h-1 w-20 cursor-pointer appearance-none rounded-full bg-[#30363d] accent-[#a371f7]"
          style={{
            background: `linear-gradient(to right, #a371f7 ${volume}%, #30363d ${volume}%)`,
          }}
        />
      </div>
    </footer>
  );
};