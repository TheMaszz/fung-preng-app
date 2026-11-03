import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { usePlayer } from "../context/PlayerContext";
import { TrackContextMenu, TrackMenuState } from "./TrackContextMenu";
import {
  Heart,
  ListMusic,
  Pause,
  Play,
  SkipBack,
  SkipForward,
  Volume1,
  Volume2,
  VolumeX,
} from "lucide-react";

interface BottomPlayerBarProps {
  queueDrawerHandler: () => void;
}

export const BottomPlayerBar: React.FC<BottomPlayerBarProps> = ({
  queueDrawerHandler,
}) => {
  const {
    currentTrack,
    isPlaying,
    togglePlay,
    playPrevious,
    playNext,
    volume,
    updateVolume,
    positionSecs,
    durationSecs,
  } = usePlayer();
  const formatTime = (seconds: number) =>
    `${Math.floor(seconds / 60)}:${String(Math.floor(seconds % 60)).padStart(2, "0")}`;
  const totalDurationSecs = durationSecs || currentTrack?.duration_secs || 0;
  const hasDuration = totalDurationSecs > 0;
  const progressPercent =
    hasDuration ? Math.min(100, (positionSecs / totalDurationSecs) * 100) : 0;

  const handleVolumeChange = (e: React.ChangeEvent<HTMLInputElement>) => {
    void updateVolume(Number(e.target.value));
  };

  const VolumeIcon = volume === 0 ? VolumeX : volume < 50 ? Volume1 : Volume2;
  const [trackMenu, setTrackMenu] = useState<TrackMenuState>(null);
  const [toast, setToast] = useState<string | null>(null);
  const [isInPlaylist, setIsInPlaylist] = useState(false);
  const toastTimer = useRef<number | undefined>(undefined);
  const closeTrackMenu = useCallback(() => setTrackMenu(null), []);
  const notify = useCallback((message: string) => {
    setToast(message);
    window.clearTimeout(toastTimer.current);
    toastTimer.current = window.setTimeout(() => setToast(null), 2500);
  }, []);

  useEffect(() => () => window.clearTimeout(toastTimer.current), []);

  useEffect(() => {
    let active = true;
    const refreshMembership = () => {
      if (!currentTrack) {
        setIsInPlaylist(false);
        return;
      }
      void invoke<boolean>("is_track_in_any_playlist", {
        videoId: currentTrack.id,
      })
        .then((saved) => {
          if (active) setIsInPlaylist(saved);
        })
        .catch((error) => {
          console.error("Failed to check playlist membership:", error);
        });
    };

    refreshMembership();
    window.addEventListener("playlists-changed", refreshMembership);
    return () => {
      active = false;
      window.removeEventListener("playlists-changed", refreshMembership);
    };
  }, [currentTrack?.id]);

  const openPlaylistMenu = (e: React.MouseEvent<HTMLButtonElement>) => {
    if (!currentTrack) return;
    const bounds = e.currentTarget.getBoundingClientRect();
    setTrackMenu({ x: bounds.left, y: bounds.top, track: currentTrack });
  };

  return (
    <footer className="fixed bottom-0 left-0 right-0 z-30 flex h-20 items-center justify-between border-t border-[#30363d] bg-[#161b22]/95 px-4 backdrop-blur-md">
      {/* Dynamic Track Info */}
      <div className="flex w-1/4 min-w-[180px] items-center gap-3">
        {currentTrack?.thumbnail_url ? (
          <img
            src={currentTrack.thumbnail_url}
            alt={currentTrack.title}
            className="h-12 w-12 rounded-md object-cover"
          />
        ) : (
          <div className="h-12 w-12 rounded-md bg-gradient-to-br from-[#1f6feb] to-[#8957e5]" />
        )}
        <div className="max-w-[140px] truncate">
          <p
            className="truncate text-sm font-medium text-[#e6edf3]"
            title={currentTrack ? currentTrack.title : "No track playing"}
          >
            {currentTrack ? currentTrack.title : "No track playing"}
          </p>
          <p
            className="truncate text-xs text-[#8b949e]"
            title={currentTrack ? currentTrack.channel : "Select a song"}
          >
            {currentTrack ? currentTrack.channel : "Select a song"}
          </p>
        </div>
        <button
          type="button"
          aria-label="Add current song to playlist"
          title="Add to playlist"
          aria-pressed={isInPlaylist}
          disabled={!currentTrack}
          onClick={openPlaylistMenu}
          className={`ml-2 transition hover:text-[#f778ba] disabled:cursor-not-allowed disabled:opacity-40 ${isInPlaylist ? "text-[#f778ba]" : "text-[#8b949e]"}`}
        >
          <Heart size={16} fill={isInPlaylist ? "currentColor" : "none"} />
        </button>
      </div>

      {/* Controls */}
      <div className="flex w-1/2 max-w-xl flex-col items-center gap-1">
        <div className="flex items-center gap-5">
          <button
            className="text-[#c9d1d9] hover:text-[#e6edf3] transition"
            onClick={() => void playPrevious()}
          >
            <SkipBack size={20} fill="currentColor" />
          </button>

          <button
            onClick={togglePlay}
            disabled={!currentTrack}
            className="grid h-9 w-9 place-items-center rounded-full bg-[#a371f7] text-white hover:bg-[#b78af2] hover:scale-105 transition shadow-md shadow-[#a371f7]/30 disabled:opacity-50"
          >
            {isPlaying ? (
              <Pause size={16} fill="white" />
            ) : (
              <Play size={16} fill="white" className="ml-0.5" />
            )}
          </button>

          <button
            className="text-[#c9d1d9] hover:text-[#e6edf3] transition"
            onClick={() => void playNext()}
          >
            <SkipForward size={20} fill="currentColor" />
          </button>
        </div>

        <div className="flex w-full items-center gap-2 text-xs text-[#8b949e]">
          <span>{formatTime(positionSecs)}</span>
          <div
            role="progressbar"
            aria-label="Song progress"
            aria-valuemin={0}
            aria-valuemax={hasDuration ? totalDurationSecs : undefined}
            aria-valuenow={hasDuration ? Math.min(positionSecs, totalDurationSecs) : undefined}
            className="group relative h-1.5 flex-1 cursor-pointer overflow-hidden rounded-full bg-[#30363d]"
          >
            <div
              className={`h-full rounded-full bg-[#58a6ff] transition-[width] group-hover:bg-[#79c0ff] ${!hasDuration && isPlaying ? "progress-indeterminate" : ""}`}
              style={{ width: hasDuration ? `${progressPercent}%` : isPlaying ? "35%" : "0%" }}
            />
          </div>
          <span>{hasDuration ? formatTime(totalDurationSecs) : "--:--"}</span>
        </div>
      </div>

      {/* Volume Bar */}
      <div className="flex w-1/4 min-w-[180px] items-center justify-end gap-3 text-[#8b949e]">
        <button
          className="hover:text-[#e6edf3] transition"
          onClick={queueDrawerHandler}
        >
          <ListMusic size={16} />
        </button>
        <button
          onClick={() => void updateVolume(volume === 0 ? 80 : 0)}
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
      {trackMenu && (
        <TrackContextMenu
          menu={trackMenu}
          onClose={closeTrackMenu}
          onNotify={notify}
        />
      )}
      {toast && (
        <div
          role="status"
          className="fixed bottom-24 left-1/2 z-50 -translate-x-1/2 rounded-lg border border-[#30363d] bg-[#161b22] px-4 py-2 text-sm text-[#e6edf3] shadow-xl"
        >
          {toast}
        </div>
      )}
    </footer>
  );
};
