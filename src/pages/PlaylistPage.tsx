// src/pages/PlaylistPage.tsx
import { useCallback, useEffect, useRef, useState } from "react";
import { useParams } from "react-router-dom";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { invoke } from "@tauri-apps/api/core";
import {
  closestCenter,
  DndContext,
  DragEndEvent,
  DragOverlay,
  DragStartEvent,
  KeyboardSensor,
  PointerSensor,
  useSensor,
  useSensors,
} from "@dnd-kit/core";
import {
  arrayMove,
  SortableContext,
  sortableKeyboardCoordinates,
  useSortable,
  verticalListSortingStrategy,
} from "@dnd-kit/sortable";
import { CSS } from "@dnd-kit/utilities";
import { GripVertical, ListMusic, Play, Shuffle, X } from "lucide-react";
import { SearchResult } from "../types";
import { usePlayer } from "../context/PlayerContext";

type Playlist = { id: number; name: string };

function shuffled<T>(items: T[]): T[] {
  const a = [...items];
  for (let i = a.length - 1; i > 0; i--) {
    const j = Math.floor(Math.random() * (i + 1));
    [a[i], a[j]] = [a[j], a[i]];
  }
  return a;
}

// ---------------- row ----------------
type RowContentProps = {
  track: SearchResult;
  index: number;
  handle: React.ReactNode;
  onRemove?: (track: SearchResult) => void;
};

const RowContent = ({ track, index, handle, onRemove }: RowContentProps) => (
  <>
    {handle}
    <span className="w-6 shrink-0 text-center text-sm tabular-nums text-[#5b6270]">
      {index + 1}
    </span>
    <img
      alt=""
      loading="lazy"
      src={track.thumbnail_url}
      className="h-9 w-16 shrink-0 rounded object-cover"
    />
    <div className="min-w-0 flex-1">
      <p className="truncate text-sm font-medium text-[#e6edf3]">{track.title}</p>
      <p className="truncate text-xs text-[#8b949e]">{track.channel}</p>
    </div>
    {onRemove && (
      <button
        aria-label={`Remove ${track.title} from playlist`}
        title="Remove from playlist"
        onClick={(e) => {
          e.stopPropagation();
          onRemove(track);
        }}
        className="rounded p-1.5 text-[#5b6270] opacity-0 transition hover:bg-[#f85149]/10 hover:text-[#f85149] focus-visible:opacity-100 group-hover:opacity-100"
      >
        <X size={16} />
      </button>
    )}
  </>
);

const rowClass =
  "group flex items-center gap-3 rounded-lg border border-transparent px-2 py-1.5 transition hover:border-[#30363d] hover:bg-[#161b22]";

type SortableRowProps = {
  track: SearchResult;
  index: number;
  onPlay: (index: number) => void;
  onRemove: (track: SearchResult) => void;
};

const SortableRow = ({ track, index, onPlay, onRemove }: SortableRowProps) => {
  const {
    attributes,
    listeners,
    setNodeRef,
    setActivatorNodeRef,
    transform,
    transition,
    isDragging,
  } = useSortable({ id: track.id });

  const style: React.CSSProperties = {
    // lock to the vertical axis
    transform: CSS.Transform.toString(transform ? { ...transform, x: 0 } : null),
    transition,
    opacity: isDragging ? 0.35 : 1,
  };

  return (
    <div
      ref={setNodeRef}
      style={style}
      className={`${rowClass} cursor-pointer`}
      onClick={() => onPlay(index)}
      onKeyDown={(e) => {
        if (e.target !== e.currentTarget) return;
        if (e.key === "Enter") onPlay(index);
        if (e.key === "Delete") onRemove(track);
      }}
      tabIndex={0}
    >
      <RowContent
        track={track}
        index={index}
        onRemove={onRemove}
        handle={
          <button
            ref={setActivatorNodeRef}
            {...attributes}
            {...listeners}
            onClick={(e) => e.stopPropagation()}
            aria-label={`Reorder ${track.title}. Press space to pick up, arrow keys to move.`}
            className="cursor-grab touch-none rounded p-1 text-[#5b6270] hover:text-[#c9d1d9] active:cursor-grabbing"
          >
            <GripVertical size={16} />
          </button>
        }
      />
    </div>
  );
};

// ---------------- cover ----------------
/**
 * 0 songs -> gradient + icon
 * 1 song  -> that thumbnail
 * 2 songs -> two side-by-side
 * 3 songs -> first one tall on the left, next two stacked on the right
 * 4+      -> 2x2 grid of the first four (in playlist order)
 */
const PlaylistCover = ({ tracks }: { tracks: SearchResult[] }) => {
  const shown = tracks.slice(0, 4);
  const box = "h-28 w-28 shrink-0 overflow-hidden rounded-xl";

  if (shown.length === 0) {
    return (
      <div
        className={`${box} grid place-items-center bg-gradient-to-tr from-[#a371f7] to-[#58a6ff] text-white`}
      >
        <ListMusic size={44} />
      </div>
    );
  }

  const layout =
    shown.length === 1
      ? "grid-cols-1"
      : shown.length === 2
        ? "grid-cols-2"
        : "grid-cols-2 grid-rows-2";

  return (
    <div className={`${box} grid gap-px bg-[#0d1117] ${layout}`}>
      {shown.map((t, i) => (
        <img
          key={t.id}
          alt=""
          src={t.thumbnail_url}
          className={`h-full w-full bg-[#21262d] object-cover ${
            shown.length === 3 && i === 0 ? "row-span-2" : ""
          }`}
        />
      ))}
    </div>
  );
};

// ---------------- page ----------------
const PlaylistPage = () => {
  const { id } = useParams();
  const playlistId = Number(id);
  const valid = Number.isFinite(playlistId);
  const queryClient = useQueryClient();
  const { playQueue } = usePlayer(); // see note: needs adding to PlayerContext

  const tracksKey = ["playlist-tracks", playlistId] as const;

  const playlistsQuery = useQuery({
    queryKey: ["playlists"],
    queryFn: () => invoke<Playlist[]>("get_playlists"),
  });
  const tracksQuery = useQuery({
    queryKey: tracksKey,
    queryFn: () => invoke<SearchResult[]>("get_playlist_tracks", { playlistId }),
    enabled: valid,
    staleTime: 0,
  });

  const name = playlistsQuery.data?.find((p) => p.id === playlistId)?.name ?? "Playlist";
  const tracks = tracksQuery.data ?? [];

  const [activeId, setActiveId] = useState<string | null>(null);
  const activeIndex = tracks.findIndex((t) => t.id === activeId);

  const [error, setError] = useState<string | null>(null);
  const errorTimer = useRef<number | undefined>(undefined);
  const showError = (msg: string) => {
    setError(msg);
    window.clearTimeout(errorTimer.current);
    errorTimer.current = window.setTimeout(() => setError(null), 4000);
  };
  useEffect(() => () => window.clearTimeout(errorTimer.current), []);

  const sensors = useSensors(
    // small distance so a plain click still plays the song
    useSensor(PointerSensor, { activationConstraint: { distance: 6 } }),
    useSensor(KeyboardSensor, { coordinateGetter: sortableKeyboardCoordinates }),
  );

  const handleDragStart = (e: DragStartEvent) => setActiveId(String(e.active.id));

  const handleDragEnd = async (e: DragEndEvent) => {
    setActiveId(null);
    const { active, over } = e;
    if (!over || active.id === over.id) return;

    const from = tracks.findIndex((t) => t.id === active.id);
    const to = tracks.findIndex((t) => t.id === over.id);
    if (from < 0 || to < 0) return;

    const previous = tracks;
    const next = arrayMove(tracks, from, to);
    queryClient.setQueryData(tracksKey, next); // optimistic
    try {
      await invoke("reorder_playlist_tracks", {
        playlistId,
        trackIds: next.map((t) => t.id),
      });
    } catch (err) {
      console.error("Failed to save order:", err);
      queryClient.setQueryData(tracksKey, previous);
      showError("Couldn't save the new order");
    }
  };

  const removeTrack = useCallback(
    async (track: SearchResult) => {
      const previous = queryClient.getQueryData<SearchResult[]>(tracksKey) ?? [];
      queryClient.setQueryData(
        tracksKey,
        previous.filter((t) => t.id !== track.id),
      );
      try {
        await invoke("remove_track_from_playlist", {
          playlistId,
          trackId: track.id,
        });
      } catch (err) {
        console.error("Failed to remove track:", err);
        queryClient.setQueryData(tracksKey, previous);
        showError(`Couldn't remove "${track.title}"`);
      }
    },
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [playlistId, queryClient],
  );

  // Plays in exactly the order shown on screen.
  const playFrom = (index: number) => void playQueue(tracks, index);
  const playAll = () => tracks.length > 0 && playFrom(0);
  const playShuffled = () => tracks.length > 0 && void playQueue(shuffled(tracks), 0);

  if (!valid) {
    return <p className="text-[#8b949e]">That playlist doesn't exist.</p>;
  }

  return (
    <section className="mx-auto max-w-3xl pb-12">
      {/* header */}
      <div className="flex items-end gap-5">
        <PlaylistCover tracks={tracks} />
        <div className="min-w-0">
          <p className="text-xs font-semibold uppercase tracking-wider text-[#8b949e]">
            Playlist
          </p>
          <h1 className="truncate text-3xl font-bold text-[#e6edf3]">{name}</h1>
          <p className="mt-1 text-sm text-[#8b949e]">
            {tracks.length} {tracks.length === 1 ? "song" : "songs"}
          </p>
        </div>
      </div>

      <div className="mt-6 flex items-center gap-3">
        <button
          onClick={playAll}
          disabled={tracks.length === 0}
          className="flex items-center gap-2 rounded-full bg-[#a371f7] px-5 py-2 text-sm font-medium text-white transition hover:bg-[#b388ff] disabled:cursor-default disabled:opacity-40"
        >
          <Play size={16} fill="currentColor" />
          Play
        </button>
        <button
          onClick={playShuffled}
          disabled={tracks.length === 0}
          className="flex items-center gap-2 rounded-full border border-[#30363d] px-4 py-2 text-sm text-[#c9d1d9] transition hover:bg-[#161b22] disabled:cursor-default disabled:opacity-40"
        >
          <Shuffle size={16} />
          Shuffle
        </button>
        {error && (
          <span role="alert" className="text-sm text-[#f85149]">
            {error}
          </span>
        )}
      </div>

      <div className="my-5 h-px bg-[#30363d]" />

      {/* body */}
      {tracksQuery.isLoading ? (
        <div className="space-y-2">
          {Array.from({ length: 5 }, (_, i) => (
            <div key={i} className="h-12 animate-pulse rounded-lg bg-[#161b22]" />
          ))}
        </div>
      ) : tracksQuery.isError ? (
        <p className="text-sm text-[#f85149]">Couldn't load this playlist.</p>
      ) : tracks.length === 0 ? (
        <div className="rounded-xl border border-dashed border-[#30363d] px-6 py-12 text-center">
          <p className="text-[#c9d1d9]">This playlist is empty</p>
          <p className="mt-1 text-sm text-[#8b949e]">
            Right-click any song on the home page and choose “Add to playlist”.
          </p>
        </div>
      ) : (
        <DndContext
          sensors={sensors}
          collisionDetection={closestCenter}
          onDragStart={handleDragStart}
          onDragEnd={(e) => void handleDragEnd(e)}
          onDragCancel={() => setActiveId(null)}
        >
          <SortableContext
            items={tracks.map((t) => t.id)}
            strategy={verticalListSortingStrategy}
          >
            <div className="flex flex-col gap-0.5">
              {tracks.map((track, index) => (
                <SortableRow
                  key={track.id}
                  track={track}
                  index={index}
                  onPlay={playFrom}
                  onRemove={(t) => void removeTrack(t)}
                />
              ))}
            </div>
          </SortableContext>

          <DragOverlay>
            {activeIndex >= 0 && (
              <div
                className={`${rowClass} border-[#a371f7] bg-[#161b22] shadow-2xl`}
              >
                <RowContent
                  track={tracks[activeIndex]}
                  index={activeIndex}
                  handle={
                    <span className="rounded p-1 text-[#a371f7]">
                      <GripVertical size={16} />
                    </span>
                  }
                />
              </div>
            )}
          </DragOverlay>
        </DndContext>
      )}
    </section>
  );
};

export default PlaylistPage;