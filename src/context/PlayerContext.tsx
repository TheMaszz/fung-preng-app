import React, { createContext, useContext, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { PlayerContextType, Track } from "../types";

const PlayerContext = createContext<PlayerContextType | undefined>(undefined);

export const PlayerProvider: React.FC<{ children: React.ReactNode }> = ({ children }) => {
  const [currentTrack, setCurrentTrack] = useState<Track | null>(null);
  const [isPlaying, setIsPlaying] = useState<boolean>(false);

  const playTrack = (track: Track) => {
    setCurrentTrack(track);
    setIsPlaying(true);
    void invoke("play_track", { videoId: track.id }).catch((error: unknown) => {
      console.error("Failed to play track:", error);
      setIsPlaying(false);
    });
  };

  const togglePlay = () => {
    if (currentTrack) {
      setIsPlaying((prev) => !prev);
    }
  };

  return (
    <PlayerContext.Provider value={{ currentTrack, isPlaying, playTrack, togglePlay }}>
      {children}
    </PlayerContext.Provider>
  );
};

export const usePlayer = () => {
  const context = useContext(PlayerContext);
  if (!context) throw new Error("usePlayer must be used within a PlayerProvider");
  return context;
};