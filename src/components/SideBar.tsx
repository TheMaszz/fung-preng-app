import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import {
  Edit,
  Home,
  ListMusic,
  Loader2,
  Music,
  Plus,
  Settings,
  Trash2,
} from "lucide-react";
import { useLocation, useNavigate } from "react-router-dom";

type Playlist = { id: number; name: string };
type MenuState = { x: number; y: number; playlist: Playlist } | null;

const MAX_NAME = 60;
const MENU_W = 176;
const MENU_H = 88; // Increased height to fit both Rename and Delete options in the menu

export const Sidebar: React.FC = () => {
  const navigate = useNavigate();
  const location = useLocation();
  const [playlists, setPlaylists] = useState<Playlist[]>([]);

  // create
  const [creating, setCreating] = useState(false);
  const [newName, setNewName] = useState("");
  const [saving, setSaving] = useState(false);
  const [createError, setCreateError] = useState<string | null>(null);

  // rename state
  const [renamingPlaylist, setRenamingPlaylist] = useState<Playlist | null>(null);
  const [renameValue, setRenameValue] = useState("");
  const [renaming, setRenaming] = useState(false);
  const [renameError, setRenameError] = useState<string | null>(null);

  // context menu + delete
  const [menu, setMenu] = useState<MenuState>(null);
  const [pendingDelete, setPendingDelete] = useState<Playlist | null>(null);

  // transient error (e.g. delete failed)
  const [error, setError] = useState<string | null>(null);
  const errorTimer = useRef<number | undefined>(undefined);

  const mainNav = [
    { label: "Home", icon: Home, path: "/" },
    { label: "Setting", icon: Settings, path: "/setting" }, 
  ];

  const showError = (msg: string) => {
    setError(msg);
    window.clearTimeout(errorTimer.current);
    errorTimer.current = window.setTimeout(() => setError(null), 4000);
  };
  useEffect(() => () => window.clearTimeout(errorTimer.current), []);

  const loadPlaylists = useCallback(async () => {
    try {
      setPlaylists(await invoke<Playlist[]>("get_playlists"));
    } catch (e) {
      console.error("Failed to load playlists:", e);
    }
  }, []);

  useEffect(() => {
    void loadPlaylists();
  }, [loadPlaylists]);

  // ---------------- create ----------------
  const startCreate = () => {
    setCreating(true);
    setNewName("");
    setCreateError(null);
  };

  const cancelCreate = () => {
    setCreating(false);
    setNewName("");
    setCreateError(null);
  };

  const submitCreate = async () => {
    const name = newName.trim();
    if (!name) return cancelCreate();
    if (playlists.some((p) => p.name.toLowerCase() === name.toLowerCase())) {
      setCreateError("You already have a playlist with that name");
      return;
    }
    setSaving(true);
    try {
      await invoke("create_playlist", { name });
      await loadPlaylists();
      cancelCreate();
    } catch (e) {
      console.error("Failed to create playlist:", e);
      setCreateError("Couldn't create the playlist. Try again.");
    } finally {
      setSaving(false);
    }
  };

  // ---------------- rename ----------------
  const startRename = (playlist: Playlist) => {
    setRenamingPlaylist(playlist);
    setRenameValue(playlist.name);
    setRenameError(null);
  };

  const cancelRename = () => {
    setRenamingPlaylist(null);
    setRenameValue("");
    setRenameError(null);
  };

  const submitRename = async () => {
    if (!renamingPlaylist) return;
    const name = renameValue.trim();
    if (!name || name === renamingPlaylist.name) {
      return cancelRename();
    }
    if (playlists.some((p) => p.id !== renamingPlaylist.id && p.name.toLowerCase() === name.toLowerCase())) {
      setRenameError("A playlist with that name already exists");
      return;
    }
    setRenaming(true);
    try {
      // Assuming your backend command looks like `update_playlist` or `rename_playlist`. 
      // Adjust the arguments ({ playlistId, name }) to match your Rust backend signature.
      await invoke("rename_playlist", { playlistId: renamingPlaylist.id, name });
      await loadPlaylists();
      cancelRename();
    } catch (e) {
      console.error("Failed to rename playlist:", e);
      setRenameError("Couldn't rename the playlist. Try again.");
    } finally {
      setRenaming(false);
    }
  };

  // ---------------- right-click menu ----------------
  const openMenu = (e: React.MouseEvent<HTMLElement>, playlist: Playlist) => {
    e.preventDefault();
    let x = e.clientX;
    let y = e.clientY;
    if (x === 0 && y === 0) {
      const r = e.currentTarget.getBoundingClientRect();
      x = r.left + 16;
      y = r.bottom;
    }
    x = Math.min(x, window.innerWidth - MENU_W - 8);
    y = Math.min(y, window.innerHeight - MENU_H - 8);
    setMenu({ x, y, playlist });
  };

  useEffect(() => {
    if (!menu) return;
    const close = () => setMenu(null);
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && close();
    window.addEventListener("keydown", onKey);
    window.addEventListener("resize", close);
    window.addEventListener("blur", close);
    return () => {
      window.removeEventListener("keydown", onKey);
      window.removeEventListener("resize", close);
      window.removeEventListener("blur", close);
    };
  }, [menu]);

  useEffect(() => {
    if (!pendingDelete) return;
    const onKey = (e: KeyboardEvent) =>
      e.key === "Escape" && setPendingDelete(null);
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [pendingDelete]);

  // ---------------- delete ----------------
  const confirmDelete = async () => {
    const target = pendingDelete;
    if (!target) return;
    setPendingDelete(null);

    const previous = playlists;
    setPlaylists((list) => list.filter((p) => p.id !== target.id)); // optimistic
    try {
      await invoke("delete_playlist", { playlistId: target.id });
    } catch (e) {
      console.error("Failed to delete playlist:", e);
      setPlaylists(previous); // roll back
      showError(`Couldn't delete "${target.name}"`);
    }
  };

  useEffect(() => {
    const refresh = () => void loadPlaylists();
    window.addEventListener("playlists-changed", refresh);
    return () => window.removeEventListener("playlists-changed", refresh);
  }, [loadPlaylists]);

  return (
    <>
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
          <button
            onClick={creating ? cancelCreate : startCreate}
            aria-label="Create playlist"
            title="Create playlist"
            className="rounded p-1 hover:bg-[#21262d] hover:text-[#e6edf3] transition"
          >
            <Plus
              size={16}
              className={`transition-transform ${creating ? "rotate-45" : ""}`}
            />
          </button>
        </div>

        {error && (
          <p role="alert" className="mt-2 px-3 text-xs text-[#f85149]">
            {error}
          </p>
        )}

        {/* Playlists List */}
        <div className="mt-2 flex-1 overflow-y-auto space-y-1 pr-1">
          {creating && (
            <div className="px-1 pb-1">
              <div className="flex items-center gap-2 rounded-md border border-[#a371f7] bg-[#0d1117] px-2 py-1.5">
                <ListMusic size={15} className="shrink-0 text-[#a371f7]" />
                <input
                  autoFocus
                  value={newName}
                  maxLength={MAX_NAME}
                  disabled={saving}
                  placeholder="Playlist name"
                  aria-label="New playlist name"
                  onChange={(e) => {
                    setNewName(e.target.value);
                    if (createError) setCreateError(null);
                  }}
                  onKeyDown={(e) => {
                    if (e.key === "Enter") void submitCreate();
                    if (e.key === "Escape") cancelCreate();
                  }}
                  onBlur={() => {
                    if (!saving && !newName.trim()) cancelCreate();
                  }}
                  className="min-w-0 flex-1 bg-transparent text-sm text-[#e6edf3] outline-none placeholder:text-[#5b6270]"
                />
                {saving && (
                  <Loader2 size={14} className="animate-spin text-[#8b949e]" />
                )}
              </div>
              {createError ? (
                <p className="mt-1 px-1 text-xs text-[#f85149]">
                  {createError}
                </p>
              ) : (
                <p className="mt-1 px-1 text-xs text-[#5b6270]">
                  Enter to create · Esc to cancel
                </p>
              )}
            </div>
          )}

          {playlists.map((playlist) => {
            const isRenaming = renamingPlaylist?.id === playlist.id;

            if (isRenaming) {
              return (
                <div key={playlist.id} className="px-1 pb-1">
                  <div className="flex items-center gap-2 rounded-md border border-[#a371f7] bg-[#0d1117] px-2 py-1.5">
                    <ListMusic size={15} className="shrink-0 text-[#a371f7]" />
                    <input
                      autoFocus
                      value={renameValue}
                      maxLength={MAX_NAME}
                      disabled={renaming}
                      placeholder="Playlist name"
                      aria-label="Rename playlist"
                      onChange={(e) => {
                        setRenameValue(e.target.value);
                        if (renameError) setRenameError(null);
                      }}
                      onKeyDown={(e) => {
                        if (e.key === "Enter") void submitRename();
                        if (e.key === "Escape") cancelRename();
                      }}
                      onBlur={() => {
                        if (!renaming) void submitRename();
                      }}
                      className="min-w-0 flex-1 bg-transparent text-sm text-[#e6edf3] outline-none placeholder:text-[#5b6270]"
                    />
                    {renaming && (
                      <Loader2 size={14} className="animate-spin text-[#8b949e]" />
                    )}
                  </div>
                  {renameError && (
                    <p className="mt-1 px-1 text-xs text-[#f85149]">{renameError}</p>
                  )}
                </div>
              );
            }

            return (
              <button
                key={playlist.id}
                onClick={() => navigate(`/playlist/${playlist.id}`)}
                onContextMenu={(e) => openMenu(e, playlist)}
                onKeyDown={(e) => {
                  if (e.key === "Delete") setPendingDelete(playlist);
                }}
                className="flex w-full items-center gap-2 rounded-md px-3 py-1.5 text-left text-sm text-[#8b949e] transition hover:bg-[#161b22] hover:text-[#e6edf3] focus-visible:outline focus-visible:outline-2 focus-visible:outline-[#a371f7]"
              >
                <ListMusic size={15} className="shrink-0" />
                <span className="truncate">{playlist.name}</span>
              </button>
            );
          })}

          {playlists.length === 0 && !creating && (
            <p className="px-3 py-2 text-xs text-[#5b6270]">
              No playlists yet. Press + to create one.
            </p>
          )}
        </div>
      </aside>

      {/* Right-click menu */}
      {menu && (
        <>
          <div
            className="fixed inset-0 z-40"
            onClick={() => setMenu(null)}
            onContextMenu={(e) => {
              e.preventDefault();
              setMenu(null);
            }}
          />
          <div
            role="menu"
            className="fixed z-50 overflow-hidden rounded-lg border border-[#30363d] bg-[#161b22] p-1 shadow-xl"
            style={{ left: menu.x, top: menu.y, width: MENU_W }}
          >
            <button
              role="menuitem"
              autoFocus
              onClick={() => {
                startRename(menu.playlist);
                setMenu(null);
              }}
              className="flex w-full items-center gap-2 rounded-md px-3 py-2 text-left text-sm text-[#8b949e] hover:bg-[#161b22] hover:text-[#e6edf3] focus-visible:outline-none"
            >
              <Edit size={14} />
              Rename
            </button>
            <button
              role="menuitem"
              onClick={() => {
                setPendingDelete(menu.playlist);
                setMenu(null);
              }}
              className="flex w-full items-center gap-2 rounded-md px-3 py-2 text-left text-sm text-[#f85149] hover:bg-[#f85149]/10 focus-visible:bg-[#f85149]/10 focus-visible:outline-none"
            >
              <Trash2 size={14} />
              Delete playlist
            </button>
          </div>
        </>
      )}

      {/* Delete confirmation */}
      {pendingDelete && (
        <div
          className="fixed inset-0 z-50 grid place-items-center bg-black/60 p-4"
          onClick={() => setPendingDelete(null)}
        >
          <div
            role="alertdialog"
            aria-modal="true"
            aria-labelledby="delete-title"
            className="w-full max-w-sm rounded-xl border border-[#30363d] bg-[#161b22] p-5"
            onClick={(e) => e.stopPropagation()}
          >
            <h3
              id="delete-title"
              className="text-base font-semibold text-[#e6edf3]"
            >
              Delete “{pendingDelete.name}”?
            </h3>
            <p className="mt-2 text-sm text-[#8b949e]">
              This removes the playlist. The songs themselves aren't affected.
            </p>
            <div className="mt-5 flex justify-end gap-2">
              <button
                autoFocus
                onClick={() => setPendingDelete(null)}
                className="rounded-lg border border-[#30363d] px-3 py-[7px] text-[13px] text-[#8b949e] hover:text-[#c9d1d9]"
              >
                Cancel
              </button>
              <button
                onClick={() => void confirmDelete()}
                className="rounded-lg bg-[#f85149] px-3 py-[7px] text-[13px] text-white hover:bg-[#ff7b72]"
              >
                Delete
              </button>
            </div>
          </div>
        </div>
      )}
    </>
  );
};