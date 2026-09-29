// src/components/QueueDrawer.tsx
import React from "react";
import { X, Music } from "lucide-react";
import { usePlayer } from "../context/PlayerContext";

interface QueueDrawerProps {
  onClose: () => void;
}

const QueueDrawer: React.FC<QueueDrawerProps> = ({ onClose }) => {
  const { currentTrack, queue, currentIndex, playTrack } = usePlayer();

  const nextTrack = currentIndex >= 0 ? queue[currentIndex + 1] : undefined;
  const suggestions = currentIndex >= 0 ? queue.slice(currentIndex + 2) : queue;

  return (
    <aside className="fixed inset-y-0 right-0 z-40 flex w-80 flex-col border-l border-[#30363d] bg-[#161b22]/95 backdrop-blur-xl shadow-2xl transition-transform duration-300 ease-in-out">
      {/* Drawer Header */}
      <div className="flex items-center justify-between border-b border-[#30363d] px-4 py-4">
        <div className="flex items-center gap-2">
          <Music size={18} className="text-[#a371f7]" />
          <h2 className="font-semibold text-[#e6edf3]">Play Queue</h2>
        </div>
        <button
          onClick={onClose}
          className="rounded-lg p-1.5 text-[#8b949e] hover:bg-[#30363d] hover:text-[#e6edf3] transition"
        >
          <X size={18} />
        </button>
      </div>

      {/* Drawer Body / Scrollable Content */}
      <div className="flex-1 overflow-y-auto p-4 space-y-6">
        {/* Now Playing Section */}
        <div>
          <h3 className="mb-2 text-xs font-semibold uppercase tracking-wider text-[#8b949e]">
            Now Playing
          </h3>
          {currentTrack ? (
            <div className="flex items-center gap-3 rounded-lg border border-[#a371f7]/40 bg-[#a371f7]/10 p-2.5">
              <img
                src={currentTrack.thumbnail_url}
                alt={currentTrack.title}
                className="h-10 w-10 rounded object-cover shadow-sm"
              />
              <div className="flex-1 min-w-0">
                <p className="truncate text-sm font-medium text-[#e6edf3]">
                  {currentTrack.title}
                </p>
                <p className="truncate text-xs text-[#8b949e]">
                  {currentTrack.channel}
                </p>
              </div>
              <span className="flex h-2 w-2 relative">
                <span className="animate-ping absolute inline-flex h-full w-full rounded-full bg-[#a371f7] opacity-75"></span>
                <span className="relative inline-flex rounded-full h-2 w-2 bg-[#a371f7]"></span>
              </span>
            </div>
          ) : (
            <p className="text-xs text-[#8b949e] italic">No track currently playing</p>
          )}
        </div>

        {/* Next Up Section - exactly one track: queue[currentIndex + 1] */}
        <div>
          <h3 className="mb-2 text-xs font-semibold uppercase tracking-wider text-[#8b949e]">
            Next up
          </h3>
          {nextTrack ? (
            <div
              className="group flex items-center gap-3 rounded-lg border border-[#30363d] bg-[#21262d]/60 p-2.5 cursor-pointer hover:border-[#58a6ff]/50 hover:bg-[#21262d] transition"
              onClick={() => playTrack(nextTrack)}
            >
              <img
                src={nextTrack.thumbnail_url}
                alt={nextTrack.title}
                className="h-10 w-10 rounded object-cover shadow-sm"
              />
              <div className="flex-1 min-w-0">
                <p className="truncate text-sm font-medium text-[#e6edf3] group-hover:text-[#58a6ff] transition">
                  {nextTrack.title}
                </p>
                <p className="truncate text-xs text-[#8b949e]">
                  {nextTrack.channel}
                </p>
              </div>
            </div>
          ) : (
            <p className="text-xs text-[#8b949e] italic">Nothing queued yet</p>
          )}
        </div>

        {/* Suggestion Section - everything after "Next up" */}
        <div>
          <div className="flex items-center justify-between mb-2">
            <h3 className="text-xs font-semibold uppercase tracking-wider text-[#8b949e]">
              Suggestion ({suggestions.length})
            </h3>
          </div>

          {suggestions.length === 0 ? (
            <div className="rounded-lg border border-dashed border-[#30363d] p-6 text-center">
              <p className="text-xs text-[#8b949e]">Your queue is empty</p>
              <p className="text-[10px] text-[#8b949e]/60 mt-1">Add tracks to keep the music going</p>
            </div>
          ) : (
            <div className="space-y-1.5">
              {suggestions.map((track, index) => (
                <div
                  key={`${track.id}-${index}`}
                  className="group flex items-center gap-2 rounded-lg p-2 hover:bg-[#21262d] transition"
                >
                  <img
                    src={track.thumbnail_url}
                    alt={track.title}
                    className="h-8 w-8 rounded object-cover"
                  />
                  <div
                    className="flex-1 min-w-0 cursor-pointer"
                    onClick={() => playTrack(track)}
                  >
                    <p className="truncate text-xs font-medium text-[#e6edf3] group-hover:text-[#58a6ff] transition">
                      {track.title}
                    </p>
                    <p className="truncate text-[10px] text-[#8b949e]">
                      {track.channel}
                    </p>
                  </div>
                </div>
              ))}
            </div>
          )}
        </div>
      </div>
    </aside>
  );
};

export default QueueDrawer;