import { usePlayer } from "../context/PlayerContext";
import { Heart, ListMusic, Mic2, Pause, Play, Repeat, Shuffle, SkipBack, SkipForward, Volume1, Volume2, VolumeX } from "lucide-react";

export const BottomPlayerBar: React.FC = () => {
  const {
    currentTrack,
    isPlaying,
    togglePlay,
    volume,
    updateVolume,
    positionSecs,
    durationSecs,
  } = usePlayer();
  const formatTime = (seconds: number) =>
    `${Math.floor(seconds / 60)}:${String(Math.floor(seconds % 60)).padStart(2, "0")}`;
  const progressPercent =
    durationSecs > 0 ? Math.min(100, (positionSecs / durationSecs) * 100) : 0;

  const handleVolumeChange = (e: React.ChangeEvent<HTMLInputElement>) => {
    void updateVolume(Number(e.target.value));
  };

  const VolumeIcon = volume === 0 ? VolumeX : volume < 50 ? Volume1 : Volume2;

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
          <p className="truncate text-sm font-medium text-[#e6edf3]">
            {currentTrack ? currentTrack.title : "No track playing"}
          </p>
          <p className="truncate text-xs text-[#8b949e]">
            {currentTrack ? currentTrack.channel : "Select a song"}
          </p>
        </div>
        <button className="ml-2 text-[#8b949e] hover:text-[#f778ba] transition">
          <Heart size={16} />
        </button>
      </div>

      {/* Controls */}
      <div className="flex w-1/2 max-w-xl flex-col items-center gap-1">
        <div className="flex items-center gap-5">
          <button className="text-[#8b949e] hover:text-[#e6edf3] transition">
            <Shuffle size={18} />
          </button>
          <button className="text-[#c9d1d9] hover:text-[#e6edf3] transition">
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

          <button className="text-[#c9d1d9] hover:text-[#e6edf3] transition">
            <SkipForward size={20} fill="currentColor" />
          </button>
          <button className="text-[#8b949e] hover:text-[#e6edf3] transition">
            <Repeat size={18} />
          </button>
        </div>

        <div className="flex w-full items-center gap-2 text-xs text-[#8b949e]">
          <span>{formatTime(positionSecs)}</span>
          <div className="h-1 flex-1 rounded-full bg-[#30363d] group cursor-pointer">
            <div
              className="h-1 rounded-full bg-[#58a6ff] group-hover:bg-[#79c0ff] transition"
              style={{ width: `${progressPercent}%` }}
            />
          </div>
          <span>{formatTime(durationSecs || currentTrack?.duration_secs || 0)}</span>
        </div>
      </div>

      {/* Volume Bar */}
      <div className="flex w-1/4 min-w-[180px] items-center justify-end gap-3 text-[#8b949e]">
        <button className="hover:text-[#e6edf3] transition">
          <Mic2 size={16} />
        </button>
        <button className="hover:text-[#e6edf3] transition">
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
    </footer>
  );
};