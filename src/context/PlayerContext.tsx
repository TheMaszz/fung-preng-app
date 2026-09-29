// src/context/PlayerContext.tsx
import React, {
  createContext,
  useContext,
  useEffect,
  useRef,
  useState,
} from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { recordListen } from "../utils/listenHistory";

export interface Track {
  id: string;
  title: string;
  channel: string;
  duration_secs?: number;
  thumbnail_url?: string;
  directUrl?: string;
}

interface PlayerContextType {
  currentTrack: Track | null;
  queue: Track[];
  currentIndex: number;
  isPlaying: boolean;
  volume: number;
  positionSecs: number;
  durationSecs: number;
  playTrack: (
    track: Track,
    newQueue?: Track[],
    directUrl?: string,
  ) => Promise<void>;
  playNext: () => Promise<void>;
  playPrevious: () => Promise<void>;
  togglePlay: () => Promise<void>;
  updateVolume: (newVolume: number) => Promise<void>;
  prefetchTracks: (videoIds: string[]) => Promise<void>;
  playTrackWithRadio: (track: Track) => Promise<void>;
  playQueue: (tracks: Track[], startIndex?: number) => Promise<void>;
}

const PlayerContext = createContext<PlayerContextType | undefined>(undefined);

// Last-resort fallback: if the backend hasn't told us the mix is ready by
// the time only this many seconds are left, we force a transition anyway
const CROSSFADE_LAST_RESORT_LEAD_SECONDS = 1.5;

export const PlayerProvider: React.FC<{ children: React.ReactNode }> = ({
  children,
}) => {
  const [queue, setQueue] = useState<Track[]>([]);
  const [currentIndex, setCurrentIndex] = useState<number>(-1);
  const [currentTrack, setCurrentTrack] = useState<Track | null>(null);
  const [isPlaying, setIsPlaying] = useState<boolean>(false);
  const [isStarting] = useState<boolean>(false);
  const [volume, setVolume] = useState<number>(80);
  const [positionSecs, setPositionSecs] = useState(0);
  const [durationSecs, setDurationSecs] = useState(0);

  const queueRef = useRef<Track[]>([]);
  const indexRef = useRef<number>(-1);
  queueRef.current = queue;
  indexRef.current = currentIndex;

  const mixPrepKeyRef = useRef<string | null>(null);
  const transitioningRef = useRef(false);
  const similarFetchKeyRef = useRef<string | null>(null);
  const transitionPointsRef = useRef<Map<string, number>>(new Map());
  const trackStartedAtRef = useRef<number>(0);
  const radioQueueRef = useRef(true);

  const prefetchTrackUrl = async (
    index: number,
    currentQueue = queueRef.current,
  ) => {
    const targetTrack = currentQueue[index];
    if (!targetTrack || targetTrack.directUrl) return;

    try {
      const directUrl = await invoke<string>("resolve_audio_url", {
        videoId: targetTrack.id,
      });

      setQueue((prevQueue) => {
        const updated = [...prevQueue];
        if (updated[index] && updated[index].id === targetTrack.id) {
          updated[index] = { ...updated[index], directUrl };
        }
        return updated;
      });

      console.log(
        `[PREFETCH] Successfully pre-cached stream URL for: ${targetTrack.title}`,
      );
    } catch (err) {
      console.warn(
        `[PREFETCH] Failed to prefetch URL for ${targetTrack.title}:`,
        err,
      );
    }
  };

  const prefetchTracks = async (videoIds: string[]) => {
    const validIds = videoIds.filter((id) => id && id.trim().length > 0);
    if (validIds.length === 0) return;
    try {
      await invoke("prefetch_track_urls", { videoIds: validIds });
    } catch (err) {
      console.error("Failed to prefetch tracks:", err);
    }
  };

  // Single Track Direct Playback
  // Single Track Direct Playback
  const playTrackAtIndex = async (
    index: number,
    currentQueue = queueRef.current,
    overrideDirectUrl?: string,
  ) => {
    const targetTrack = currentQueue[index];
    if (!targetTrack) return;

    let effectiveDirectUrl = overrideDirectUrl ?? targetTrack.directUrl;

    // If we don't have a direct URL yet, resolve it right now before playing!
    if (!effectiveDirectUrl) {
      try {
        effectiveDirectUrl = await invoke<string>("resolve_audio_url", {
          videoId: targetTrack.id,
        });
        // Update it in the queue so we have it cached
        setQueue((prevQueue) => {
          const updated = [...prevQueue];
          if (updated[index] && updated[index].id === targetTrack.id) {
            updated[index] = {
              ...updated[index],
              directUrl: effectiveDirectUrl,
            };
          }
          return updated;
        });
      } catch (err) {
        console.error(
          `[PLAY] Failed to resolve audio URL for ${targetTrack.title}:`,
          err,
        );
        return;
      }
    }

    transitioningRef.current = false;
    trackStartedAtRef.current = performance.now();

    setCurrentIndex(index);
    setCurrentTrack(targetTrack);
    setIsPlaying(true);
    setPositionSecs(0);
    setDurationSecs(targetTrack.duration_secs ?? 0);
    void recordListen(targetTrack).catch((error) => {
      console.warn("Failed to record listening history:", error);
    });

    await invoke("play_audio", {
      videoId: targetTrack.id,
      durationSecs: targetTrack.duration_secs ?? null,
      directUrl: effectiveDirectUrl,
    });

    if (index + 1 < currentQueue.length) {
      void prefetchTrackUrl(index + 1, currentQueue);
    }
  };

  // Smart Crossfade Transition between currentTrack (A) and nextTrack (B)
  const playMixedTransition = async (
    fromIndex: number,
    toIndex: number,
    currentQueue = queueRef.current,
  ) => {
    const trackA = currentQueue[fromIndex];
    const trackB = currentQueue[toIndex];

    if (!trackB) return;

    if (!trackA) {
      await playTrackAtIndex(toIndex, currentQueue);
      return;
    }

    setCurrentIndex(toIndex);
    setCurrentTrack(trackB);
    setIsPlaying(true);
    setPositionSecs(0);
    setDurationSecs(trackB.duration_secs ?? 0);

    try {
      await invoke("play_mixed_session", {
        videoIdA: trackA.id,
        videoIdB: trackB.id,
      });
      transitionPointsRef.current.delete(`${trackA.id}->${trackB.id}`);
    } catch (err) {
      console.error(
        "Mixed transition failed, falling back to direct play:",
        err,
      );
      await playTrackAtIndex(toIndex, currentQueue);
    }

    if (toIndex + 1 < currentQueue.length) {
      void prefetchTrackUrl(toIndex + 1, currentQueue);
    }
  };

  const playTrack = async (
    track: Track,
    newQueue?: Track[],
    directUrl?: string,
  ) => {
    const activeQueue = newQueue ?? queueRef.current;
    if (newQueue) setQueue(newQueue);

    let idx = activeQueue.findIndex((t) => t.id === track.id);
    if (idx === -1) {
      const updatedQueue = [...activeQueue, track];
      setQueue(updatedQueue);
      idx = updatedQueue.length - 1;
      await playTrackAtIndex(idx, updatedQueue, directUrl);
    } else {
      await playTrackAtIndex(idx, activeQueue, directUrl);
    }
  };

  const playQueue = async (tracks: Track[], startIndex = 0) => {
    if (tracks.length === 0) return;
    const start = Math.max(0, Math.min(startIndex, tracks.length - 1));
    radioQueueRef.current = false;
    setQueue(tracks);
    await playTrackAtIndex(start, tracks);
  };

  const fetchAndAppendSimilar = async (
    baseTrackId: string,
  ): Promise<Track[]> => {
    if (!radioQueueRef.current) return queueRef.current;

    try {
      const related = await invoke<Track[]>("get_related_tracks", {
        videoId: baseTrackId,
      });

      if (related.length === 0) return queueRef.current;

      const existingIds = new Set(queueRef.current.map((t) => t.id));
      const newTracks = related.filter((t) => !existingIds.has(t.id));

      const updatedQueue = [...queueRef.current, ...newTracks];
      setQueue(updatedQueue);

      const nextIdx = indexRef.current + 1;
      if (nextIdx < updatedQueue.length) {
        void prefetchTrackUrl(nextIdx, updatedQueue);
      }

      return updatedQueue;
    } catch (err) {
      console.error("Failed to fetch similar tracks:", err);
      return queueRef.current;
    }
  };

  useEffect(() => {
    const idx = indexRef.current;
    const hasNext = idx + 1 < queueRef.current.length;
    if (!currentTrack || hasNext) return;
    if (similarFetchKeyRef.current === currentTrack.id) return;
    similarFetchKeyRef.current = currentTrack.id;
    console.log(
      `[MIX] +${(
        (performance.now() - trackStartedAtRef.current) /
        1000
      ).toFixed(
        1,
      )}s: fetching related tracks for ${currentTrack.id} (no next track queued yet)`,
    );
    void fetchAndAppendSimilar(currentTrack.id);
  }, [currentTrack, queue]);

  const playNext = async () => {
    if (transitioningRef.current) return;
    transitioningRef.current = true;

    try {
      let activeQueue = queueRef.current;
      let currentIdx = indexRef.current;

      if (activeQueue.length === 0) {
        if (currentTrack && radioQueueRef.current) {
          activeQueue = await fetchAndAppendSimilar(currentTrack.id);
          currentIdx = 0;
        } else {
          return;
        }
      }

      const nextIdx = currentIdx + 1;

      if (
        nextIdx >= activeQueue.length &&
        currentTrack &&
        radioQueueRef.current
      ) {
        activeQueue = await fetchAndAppendSimilar(currentTrack.id);
      }

      if (nextIdx < activeQueue.length) {
        await playMixedTransition(currentIdx, nextIdx, activeQueue);
      } else if (!radioQueueRef.current) {
        setIsPlaying(false);
        await invoke("stop_audio");
      }
    } finally {
      setTimeout(() => {
        transitioningRef.current = false;
      }, 1000);
    }
  };

  const playPrevious = async () => {
    const prevIdx = indexRef.current - 1;
    if (prevIdx >= 0) {
      await playTrackAtIndex(prevIdx);
    }
  };

  // Fallback safety net for track ending signals
  useEffect(() => {
    const unlisten = listen("track-ended", () => {
      void playNext();
    });

    return () => {
      void unlisten.then((fn) => fn());
    };
  }, []);

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

  const playTrackWithRadio = async (track: Track) => {
    transitioningRef.current = false;
    radioQueueRef.current = true;

    let effectiveDirectUrl = track.directUrl;
    if (!effectiveDirectUrl) {
      try {
        effectiveDirectUrl = await invoke<string>("resolve_audio_url", {
          videoId: track.id,
        });
      } catch (err) {
        console.error("Failed to resolve initial radio track URL:", err);
        return;
      }
    }

    const initialTrack = { ...track, directUrl: effectiveDirectUrl };

    setCurrentTrack(initialTrack);
    setIsPlaying(true);
    setPositionSecs(0);
    setDurationSecs(initialTrack.duration_secs ?? 0);
    void recordListen(initialTrack).catch((error) => {
      console.warn("Failed to record listening history:", error);
    });

    void invoke("play_audio", {
      videoId: initialTrack.id,
      durationSecs: initialTrack.duration_secs ?? null,
      directUrl: effectiveDirectUrl,
    });

    try {
      const related = await invoke<Track[]>("get_related_tracks", {
        videoId: initialTrack.id,
      });

      const radioQueue = [initialTrack, ...related];
      setQueue(radioQueue);
      setCurrentIndex(0);

      if (related.length > 0) {
        void prefetchTrackUrl(1, radioQueue);
      }
    } catch (err) {
      console.error("Failed to fetch radio queue:", err);
      setQueue([initialTrack]);  
      setCurrentIndex(0);
    }
  };

  // Progress Polling + Proactive Pre-warming + Proactive Crossfade
  useEffect(() => {
    if (!isPlaying || isStarting) return;
    const timer = window.setInterval(() => {
      void invoke<{ position_secs: number; duration_secs: number }>(
        "get_playback_progress",
      ).then((progress) => {
        setPositionSecs(progress.position_secs);
        if (progress.duration_secs > 0) {
          setDurationSecs(progress.duration_secs);

          const remaining = progress.duration_secs - progress.position_secs;
          const currIdx = indexRef.current;
          const currTrack = queueRef.current[currIdx];
          const nextIdx = currIdx + 1;
          const nextTrack = queueRef.current[nextIdx];

          if (remaining <= 15 && nextIdx < queueRef.current.length) {
            void prefetchTrackUrl(nextIdx);
          }

          if (nextTrack && currTrack && !transitioningRef.current) {
            const transitionKey = `${currTrack.id}->${nextTrack.id}`;
            const exactTransitionAt =
              transitionPointsRef.current.get(transitionKey);

            let shouldTransition = false;
            if (exactTransitionAt !== undefined) {
              shouldTransition = progress.position_secs >= exactTransitionAt;
            } else if (remaining <= CROSSFADE_LAST_RESORT_LEAD_SECONDS) {
              console.warn(
                `[MIX] ${transitionKey} not ready with only ${remaining.toFixed(
                  1,
                )}s left - forcing on-the-spot build (will be rough).`,
              );
              shouldTransition = true;
            }

            if (shouldTransition) {
              void playNext();
            }
          }
        }
      });
    }, 250);
    return () => window.clearInterval(timer);
  }, [isPlaying, isStarting]);

  // Kick off crossfade mix tied securely to index changes
  useEffect(() => {
    const currIdx = indexRef.current;
    const currTrack = queueRef.current[currIdx];
    const nextTrack = queueRef.current[currIdx + 1];
    if (!currTrack || !nextTrack) return;

    const prepKey = `${currTrack.id}->${nextTrack.id}`;
    if (mixPrepKeyRef.current === prepKey) return;
    mixPrepKeyRef.current = prepKey;

    console.log(
      `[MIX] +${((performance.now() - trackStartedAtRef.current) / 1000).toFixed(1)}s: requesting crossfade prep: ${prepKey}`,
    );
    void invoke("prepare_mixed_session", {
      videoIdA: currTrack.id,
      videoIdB: nextTrack.id,
    }).catch((err) => {
      console.warn("[MIX] Failed to prepare crossfade:", err);
      if (mixPrepKeyRef.current === prepKey) {
        mixPrepKeyRef.current = null;
      }
    });
  }, [currentIndex, queue]);

  useEffect(() => {
    const unlisten = listen<{ video_id: string }>(
      "active-track-changed",
      (event) => {
        const actualId = event.payload.video_id;
        const targetIdx = queueRef.current.findIndex((t) => t.id === actualId);

        if (targetIdx !== -1 && targetIdx !== indexRef.current) {
          console.warn(
            `[SYNC] Backend forced track change. Aligning UI to index ${targetIdx}`,
          );
          setCurrentIndex(targetIdx);
          setCurrentTrack(queueRef.current[targetIdx]);
          setPositionSecs(0);
          setDurationSecs(queueRef.current[targetIdx].duration_secs ?? 0);
        }
      },
    );

    return () => {
      void unlisten.then((fn) => fn());
    };
  }, []);

  useEffect(() => {
    const unlisten = listen<{
      video_id_a: string;
      video_id_b: string;
      transition_at_secs: number;
    }>("mix-ready", (event) => {
      const { video_id_a, video_id_b, transition_at_secs } = event.payload;
      const key = `${video_id_a}->${video_id_b}`;
      transitionPointsRef.current.set(key, transition_at_secs);
      console.log(
        `[MIX] +${(
          (performance.now() - trackStartedAtRef.current) /
          1000
        ).toFixed(
          1,
        )}s: mix-ready for ${key}, transition at ${transition_at_secs.toFixed(2)}s`,
      );
    });

    return () => {
      void unlisten.then((fn) => fn());
    };
  }, []);

  return (
    <PlayerContext.Provider
      value={{
        currentTrack,
        queue,
        currentIndex,
        isPlaying,
        volume,
        positionSecs,
        durationSecs,
        playTrack,
        playNext,
        playPrevious,
        prefetchTracks,
        togglePlay,
        updateVolume,
        playTrackWithRadio,
        playQueue,
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
