// src/pages/HomePage.tsx
import { useInfiniteQuery, useQueries } from "@tanstack/react-query";
import { invoke } from "@tauri-apps/api/core";
import { useSearchParams } from "react-router-dom";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { SearchResult } from "../types";
import { usePlayer } from "../context/PlayerContext";
import { getListenHistory, ListenHistoryEntry } from "../utils/listenHistory";
import { TrackContextMenu, TrackMenuState } from "../components/TrackContextMenu";

const SEED_COUNT = 3; // how many favourite songs we ask YouTube for "related" of
const SUGGESTION_COUNT = 6;
const RECENT_COUNT = 3;
const RELATED_STALE_MS = 30 * 60 * 1000; // coming back to Home = instant, no refetch

/** Play count, but older listens fade out (~30 day half-feel), so taste can change. */
function tasteScore(e: ListenHistoryEntry, now: number) {
  const ageDays = Math.max(0, now - e.lastPlayedAt) / 86_400_000;
  return e.count * Math.exp(-ageDays / 30);
}

/** History only stores id/title/channel; YouTube thumbnails are predictable from the id. */
function historyToTrack(e: ListenHistoryEntry): SearchResult {
  return {
    id: e.id,
    title: e.title,
    channel: e.channel,
    thumbnail_url: `https://i.ytimg.com/vi/${e.id}/mqdefault.jpg`,
  } as SearchResult;
}

/**
 * Round-robin across seeds so the first favourite doesn't fill the whole list.
 * Skips duplicates and anything in `exclude`.
 */
function interleave(
  lists: SearchResult[][],
  exclude: Set<string>,
  limit: number,
): SearchResult[] {
  const seen = new Set(exclude);
  const out: SearchResult[] = [];
  const longest = Math.max(0, ...lists.map((l) => l.length));
  for (let i = 0; i < longest && out.length < limit; i++) {
    for (const list of lists) {
      const t = list[i];
      if (t && !seen.has(t.id)) {
        seen.add(t.id);
        out.push(t);
        if (out.length >= limit) break;
      }
    }
  }
  return out;
}

type TrackCardProps = {
  track: SearchResult;
  as?: "h2" | "h3";
  onPlay: (track: SearchResult) => void;
  onPrefetch: (id: string) => void;
  onMenu: (e: React.MouseEvent<HTMLElement>, track: SearchResult) => void;
};

const TrackCard = ({
  track,
  as: Heading = "h3",
  onPlay,
  onPrefetch,
  onMenu,
}: TrackCardProps) => (
  <div
    role="button"
    tabIndex={0}
    aria-haspopup="menu"
    className="cursor-pointer overflow-hidden rounded-lg border border-[#30363d] bg-[#161b22] transition hover:bg-[#21262d] focus-visible:outline focus-visible:outline-2 focus-visible:outline-[#a371f7]"
    onClick={() => onPlay(track)}
    onContextMenu={(e) => onMenu(e, track)}
    onKeyDown={(e) => {
      if (e.key === "Enter" || e.key === " ") {
        e.preventDefault();
        onPlay(track);
      }
    }}
    onMouseEnter={() => onPrefetch(track.id)}
    onFocus={() => onPrefetch(track.id)}
  >
    <img
      alt=""
      loading="lazy"
      className="aspect-video w-full object-cover"
      src={track.thumbnail_url}
    />
    <div className="p-4">
      <Heading className="line-clamp-2 font-medium text-[#e6edf3]">
        {track.title}
      </Heading>
      <p className="mt-2 text-sm text-[#8b949e]">{track.channel}</p>
    </div>
  </div>
);

const SkeletonCard = () => (
  <div className="animate-pulse overflow-hidden rounded-lg border border-[#30363d] bg-[#161b22]">
    <div className="aspect-video w-full bg-[#21262d]" />
    <div className="space-y-2 p-4">
      <div className="h-4 w-4/5 rounded bg-[#21262d]" />
      <div className="h-3 w-2/5 rounded bg-[#21262d]" />
    </div>
  </div>
);

const HomePage = () => {
  const [searchParams] = useSearchParams();
  const query = searchParams.get("q")?.trim() ?? "";
  const { prefetchTracks, playTrackWithRadio } = usePlayer();
  const loadMoreRef = useRef<HTMLDivElement>(null);
  const [history, setHistory] = useState<ListenHistoryEntry[]>([]);

  useEffect(() => {
    let active = true;
    void getListenHistory()
      .then((entries) => {
        if (active) setHistory(entries);
      })
      .catch((error) => console.error("Failed to load listening history:", error));
    return () => {
      active = false;
    };
  }, [query]);

  // ---------------- search ----------------
  const searchQuery = useInfiniteQuery({
    queryKey: ["youtube-search", query],
    queryFn: ({ pageParam }) =>
      invoke<SearchResult[]>("search_youtube", { query, offset: pageParam }),
    initialPageParam: 0,
    getNextPageParam: (lastPage, allPages) =>
      lastPage.length === 10 ? allPages.length * 10 : undefined,
    enabled: query.length > 0,
  });

  // Memoised: a fresh array every render used to re-trigger the prefetch effect below.
  const results = useMemo(
    () => searchQuery.data?.pages.flat() ?? [],
    [searchQuery.data],
  );

  // ---------------- listening history ----------------
  const seeds = useMemo(() => {
    const now = Date.now();
    return [...history]
      .sort((a, b) => tasteScore(b, now) - tasteScore(a, now))
      .slice(0, SEED_COUNT);
  }, [history]);

  const recent = useMemo(
    () =>
      [...history]
        .sort((a, b) => b.lastPlayedAt - a.lastPlayedAt)
        .slice(0, RECENT_COUNT)
        .map(historyToTrack),
    [history],
  );

  // One cached request per seed, fetched in parallel.
  const relatedQueries = useQueries({
    queries: seeds.map((seed) => ({
      queryKey: ["related-tracks", seed.id],
      queryFn: () =>
        invoke<SearchResult[]>("get_related_tracks", { videoId: seed.id }),
      staleTime: RELATED_STALE_MS,
      retry: 1,
      enabled: query.length === 0,
    })),
  });

  const forYouLoading = relatedQueries.some((q) => q.isLoading);
  const forYou = interleave(
    relatedQueries.map((q) => q.data ?? []),
    new Set(seeds.map((s) => s.id)),
    SUGGESTION_COUNT,
  );

  // ---------------- prefetch ----------------
  useEffect(() => {
    if (results.length > 0) {
      prefetchTracks(results.slice(0, 5).map((r) => r.id));
    }
  }, [results, prefetchTracks]);

  const prefetched = useRef(new Set<string>());
  const prefetchOne = useCallback(
    (id: string) => {
      if (prefetched.current.has(id)) return;
      prefetched.current.add(id);
      prefetchTracks([id]);
    },
    [prefetchTracks],
  );

  const play = useCallback(
    (track: SearchResult) => void playTrackWithRadio(track),
    [playTrackWithRadio],
  );

  // ---------------- right-click: add to playlist ----------------
  const [trackMenu, setTrackMenu] = useState<TrackMenuState>(null);
  const closeTrackMenu = useCallback(() => setTrackMenu(null), []);

  const openTrackMenu = useCallback(
    (e: React.MouseEvent<HTMLElement>, track: SearchResult) => {
      e.preventDefault();
      let x = e.clientX;
      let y = e.clientY;
      // Keyboard-triggered context menu (Menu key / Shift+F10) reports 0,0.
      if (x === 0 && y === 0) {
        const r = e.currentTarget.getBoundingClientRect();
        x = r.left + 16;
        y = r.top + 16;
      }
      setTrackMenu({ x, y, track });
    },
    [],
  );

  const [toast, setToast] = useState<string | null>(null);
  const toastTimer = useRef<number | undefined>(undefined);
  const notify = useCallback((message: string) => {
    setToast(message);
    window.clearTimeout(toastTimer.current);
    toastTimer.current = window.setTimeout(() => setToast(null), 2500);
  }, []);
  useEffect(() => () => window.clearTimeout(toastTimer.current), []);

  // ---------------- infinite scroll ----------------
  useEffect(() => {
    const target = loadMoreRef.current;
    if (!target) return;
    const observer = new IntersectionObserver(
      ([entry]) => {
        if (
          entry.isIntersecting &&
          searchQuery.hasNextPage &&
          !searchQuery.isFetchingNextPage
        ) {
          void searchQuery.fetchNextPage();
        }
      },
      { rootMargin: "400px" },
    );
    observer.observe(target);
    return () => observer.disconnect();
  }, [
    searchQuery.fetchNextPage,
    searchQuery.hasNextPage,
    searchQuery.isFetchingNextPage,
  ]);

  const grid = "grid gap-4 sm:grid-cols-2 lg:grid-cols-3";
  const cardProps = { onPlay: play, onPrefetch: prefetchOne, onMenu: openTrackMenu };

  return (
    <section className="mx-auto max-w-6xl pb-12">
      <h1 className="text-3xl font-bold text-[#e6edf3]">
        {query ? `Search results for "${query}"` : "Welcome to Fung Pleng"}
      </h1>

      {/* ---------------- Home (no search) ---------------- */}
      {!query && (
        <>
          {history.length === 0 ? (
            <p className="mt-3 max-w-xl text-[#8b949e]">
              Search for a track above. Songs you play will show up here, along
              with picks based on what you listen to.
            </p>
          ) : (
            <>
              <section className="mt-8">
                <h2 className="mb-4 text-xl font-semibold text-[#e6edf3]">
                  Jump back in
                </h2>
                <div className={grid}>
                  {recent.map((t) => (
                    <TrackCard key={t.id} track={t} {...cardProps} />
                  ))}
                </div>
              </section>

              <section className="mt-10">
                <h2 className="mb-4 text-xl font-semibold text-[#e6edf3]">
                  For you
                </h2>
                {forYouLoading && forYou.length === 0 ? (
                  <div className={grid}>
                    {Array.from({ length: SUGGESTION_COUNT }, (_, i) => (
                      <SkeletonCard key={i} />
                    ))}
                  </div>
                ) : forYou.length > 0 ? (
                  <div className={grid}>
                    {forYou.map((t) => (
                      <TrackCard key={t.id} track={t} {...cardProps} />
                    ))}
                  </div>
                ) : (
                  <p className="text-sm text-[#8b949e]">
                    Couldn't load suggestions right now. Check your connection
                    and reopen Home to try again.
                  </p>
                )}
              </section>
            </>
          )}
        </>
      )}

      {/* ---------------- Search states ---------------- */}
      {searchQuery.isLoading && (
        <p className="mt-8 text-[#8b949e]">Searching YouTube...</p>
      )}
      {searchQuery.isError && (
        <p className="mt-8 text-red-400">
          Search failed:{" "}
          {searchQuery.error instanceof Error
            ? searchQuery.error.message
            : String(searchQuery.error)}
        </p>
      )}
      {searchQuery.data && results.length === 0 && (
        <p className="mt-8 text-[#8b949e]">No results found.</p>
      )}

      {results.length > 0 && (
        <div className={`mt-8 ${grid}`}>
          {results.map((r) => (
            <TrackCard key={r.id} track={r} as="h2" {...cardProps} />
          ))}
        </div>
      )}

      {searchQuery.hasNextPage && (
        <div ref={loadMoreRef} className="py-8 text-center text-sm text-[#8b949e]">
          {searchQuery.isFetchingNextPage ? "Loading more..." : "Scroll for more"}
        </div>
      )}

      {/* Right-click menu + toast */}
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
    </section>
  );
};

export default HomePage;