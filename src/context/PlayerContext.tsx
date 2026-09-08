import React, { createContext, useContext, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";

export interface Track {
  id: string;
  title: string;
  channel: string;
  duration_secs?: number;
  thumbnail_url?: string;
}

interface PlayerContextType {
  currentTrack: Track | null;
  isPlaying: boolean;
  volume: number;
  playTrack: (track: Track, directUrl?: string) => Promise<void>;
  prefetchTracks: (videoIds: string[]) => Promise<void>; // Added prefetch method
  togglePlay: () => Promise<void>;
  updateVolume: (newVolume: number) => Promise<void>;
  positionSecs: number;
  durationSecs: number;
}

const PlayerContext = createContext<PlayerContextType | undefined>(undefined);

export const PlayerProvider: React.FC<{ children: React.ReactNode }> = ({
  children,
}) => {
  const [currentTrack, setCurrentTrack] = useState<Track | null>(null);
  const [isPlaying, setIsPlaying] = useState<boolean>(false);
  const [volume, setVolume] = useState<number>(80);
  const [positionSecs, setPositionSecs] = useState(0);
  const [durationSecs, setDurationSecs] = useState(0);
  const [isStarting, setIsStarting] = useState(false);

  useEffect(() => {
    if (!isPlaying || isStarting) return;
    const timer = window.setInterval(() => {
      void invoke<{ position_secs: number; duration_secs: number }>(
        "get_playback_progress",
      ).then((progress) => {
        setPositionSecs(progress.position_secs);
        if (progress.duration_secs > 0) {
          setDurationSecs(progress.duration_secs);
        }
      });
    }, 250);
    return () => window.clearInterval(timer);
  }, [isPlaying, isStarting]);

  // Non-blocking prefetch call to Rust background task pool
  const prefetchTracks = async (videoIds: string[]) => {
    const validIds = videoIds.filter((id) => id && id.trim().length > 0);
    if (validIds.length === 0) return;
    try {
      await invoke("prefetch_track_urls", { videoIds: validIds });
    } catch (err) {
      console.error("Failed to prefetch tracks:", err);
    }
  };

  const playTrack = async (track: Track, directUrl?: string) => {
    // If clicking the same track that is currently paused, resume it from current position
    if (currentTrack?.id === track.id && !isPlaying) {
      await invoke("resume_audio");
      setIsPlaying(true);
      return;
    }

    try {
      setCurrentTrack(track);
      setIsPlaying(true);
      setPositionSecs(0);
      setDurationSecs(track.duration_secs ?? 0);
      setIsStarting(true);

      await invoke("play_audio", {
        videoId: track.id,
        durationSecs: track.duration_secs ?? null,
        directUrl: directUrl ?? null,
      });
    } catch (err) {
      console.error("Failed to play track:", err);
      setIsPlaying(false);
    } finally {
      setIsStarting(false);
    }
  };

  const togglePlay = async () => {
    if (!currentTrack) return;

    if (isPlaying) {
      await invoke("pause_audio");
      setIsPlaying(false);
    } else {
      await invoke("resume_audio");
      setIsPlaying(true);
    }
  };

  const updateVolume = async (newVolume: number) => {
    setVolume(newVolume);
    await invoke("set_volume", { volume: newVolume / 100 });
  };

  return (
    <PlayerContext.Provider
      value={{
        currentTrack,
        isPlaying,
        volume,
        playTrack,
        prefetchTracks,
        togglePlay,
        updateVolume,
        positionSecs,
        durationSecs,
      }}
    >
      {children}
    </PlayerContext.Provider>
  );
};

export const usePlayer = () => {
  const context = useContext(PlayerContext);
  if (!context)
    throw new Error("usePlayer must be used within a PlayerProvider");
  return context;
};
