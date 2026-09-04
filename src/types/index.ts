export interface Track {
  id: string;
  title: string;
  channel: string;
  duration_secs?: number;
  thumbnail_url?: string;
}

export interface PlayerContextType {
  currentTrack: Track | null;
  isPlaying: boolean;
  playTrack: (track: Track) => void;
  togglePlay: () => void;
}

export interface SearchResult {
  id: string;
  title: string;
  channel: string;
  duration_secs: number;
  thumbnail_url: string;
}