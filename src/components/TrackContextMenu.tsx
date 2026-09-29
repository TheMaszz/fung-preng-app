import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { ListMusic, Loader2, Plus } from "lucide-react";
import { SearchResult } from "../types";

type Playlist = { id: number; name: string };
export type TrackMenuState = { x: number; y: number; track: SearchResult } | null;

const MENU_W = 248;
const MENU_MAX_H = 340;

type Props = {
  menu: NonNullable<TrackMenuState>;
  onClose: () => void; // must be referentially stable (useCallback)
  onNotify: (message: string) => void;
};

export const TrackContextMenu = ({ menu, onClose, onNotify }: Props) => {
  const { x, y, track } = menu;
  const [playlists, setPlaylists] = useState<Playlist[] | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [creating, setCreating] = useState(false);
  const [name, setName] = useState("");

  // Mounted fresh on every open, so the list is always up to date.
  useEffect(() => {
    let active = true;
    invoke<Playlist[]>("get_playlists")
      .then((list) => active && setPlaylists(list))
      .catch((e) => {
        console.error("Failed to load playlists:", e);
        if (active) {
          setPlaylists([]);
          setError("Couldn't load your playlists");
        }
      });
    return () => {
      active = false;
    };
  }, []);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && onClose();
    window.addEventListener("keydown", onKey);
    window.addEventListener("resize", onClose);
    window.addEventListener("blur", onClose);
    return () => {
      window.removeEventListener("keydown", onKey);
      window.removeEventListener("resize", onClose);
      window.removeEventListener("blur", onClose);
    };
  }, [onClose]);

  const addTo = async (playlist: Playlist) => {
    setBusy(true);
    setError(null);
    try {
      const added = await invoke<boolean>("add_track_to_playlist", {
        playlistId: playlist.id,
        track,
      });
      onNotify(added ? `Added to “${playlist.name}”` : `Already in “${playlist.name}”`);
      onClose();
    } catch (e) {
      console.error("Failed to add track:", e);
      setError("Couldn't add the song. Try again.");
      setBusy(false);
    }
  };

  const createAndAdd = async () => {
    const n = name.trim();
    if (!n) {
      setCreating(false);
      return;
    }
    if (playlists?.some((p) => p.name.toLowerCase() === n.toLowerCase())) {
      setError("You already have a playlist with that name");
      return;
    }
    setBusy(true);
    setError(null);
    try {
      await invoke("create_playlist", { name: n });
      window.dispatchEvent(new Event("playlists-changed")); // lets the Sidebar refresh
      const list = await invoke<Playlist[]>("get_playlists");
      const created = [...list].reverse().find((p) => p.name === n);
      if (!created) throw new Error("New playlist not found");
      await addTo(created);
    } catch (e) {
      console.error("Failed to create playlist:", e);
      setError("Couldn't create the playlist. Try again.");
      setBusy(false);
    }
  };

  const left = Math.max(8, Math.min(x, window.innerWidth - MENU_W - 8));
  const top = Math.max(8, Math.min(y, window.innerHeight - MENU_MAX_H - 8));

  return (
    <>
      {/* backdrop: any click (left or right) outside closes the menu */}
      <div
        className="fixed inset-0 z-40"
        onClick={onClose}
        onContextMenu={(e) => {
          e.preventDefault();
          onClose();
        }}
      />
      <div
        role="menu"
        aria-label="Add to playlist"
        className="fixed z-50 flex flex-col overflow-hidden rounded-lg border border-[#30363d] bg-[#161b22] shadow-xl"
        style={{ left, top, width: MENU_W, maxHeight: MENU_MAX_H }}
      >
        <div className="border-b border-[#30363d] px-3 py-2.5">
          <p className="text-[11px] font-semibold uppercase tracking-wider text-[#8b949e]">
            Add to playlist
          </p>
          <p className="mt-0.5 truncate text-sm text-[#e6edf3]">{track.title}</p>
        </div>

        <div className="min-h-0 flex-1 overflow-y-auto p-1">
          {playlists === null ? (
            <div className="flex items-center gap-2 px-3 py-2 text-sm text-[#8b949e]">
              <Loader2 size={14} className="animate-spin" />
              Loading…
            </div>
          ) : playlists.length === 0 && !error ? (
            <p className="px-3 py-2 text-sm text-[#5b6270]">No playlists yet.</p>
          ) : (
            playlists.map((p, i) => (
              <button
                key={p.id}
                role="menuitem"
                autoFocus={i === 0}
                disabled={busy}
                onClick={() => void addTo(p)}
                className="flex w-full items-center gap-2 rounded-md px-3 py-2 text-left text-sm text-[#c9d1d9] hover:bg-[#21262d] focus-visible:bg-[#21262d] focus-visible:outline-none disabled:opacity-50"
              >
                <ListMusic size={15} className="shrink-0 text-[#8b949e]" />
                <span className="truncate">{p.name}</span>
              </button>
            ))
          )}
        </div>

        {error && <p className="px-3 pb-2 text-xs text-[#f85149]">{error}</p>}

        <div className="border-t border-[#30363d] p-1">
          {creating ? (
            <div className="flex items-center gap-2 rounded-md border border-[#a371f7] bg-[#0d1117] px-2 py-1.5">
              <input
                autoFocus
                value={name}
                maxLength={60}
                disabled={busy}
                placeholder="Playlist name"
                aria-label="New playlist name"
                onChange={(e) => {
                  setName(e.target.value);
                  if (error) setError(null);
                }}
                onKeyDown={(e) => {
                  if (e.key === "Enter") void createAndAdd();
                  if (e.key === "Escape") {
                    e.stopPropagation();
                    setCreating(false);
                    setName("");
                  }
                }}
                className="min-w-0 flex-1 bg-transparent text-sm text-[#e6edf3] outline-none placeholder:text-[#5b6270]"
              />
              {busy && <Loader2 size={14} className="animate-spin text-[#8b949e]" />}
            </div>
          ) : (
            <button
              role="menuitem"
              disabled={busy}
              onClick={() => setCreating(true)}
              className="flex w-full items-center gap-2 rounded-md px-3 py-2 text-left text-sm text-[#a371f7] hover:bg-[#a371f7]/10 focus-visible:bg-[#a371f7]/10 focus-visible:outline-none"
            >
              <Plus size={15} />
              New playlist
            </button>
          )}
        </div>
      </div>
    </>
  );
};