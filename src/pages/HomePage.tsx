// src/pages/HomePage.tsx
import { useInfiniteQuery } from "@tanstack/react-query";
import { invoke } from "@tauri-apps/api/core";
import { useSearchParams } from "react-router-dom";
import { useEffect, useRef } from "react";
import { SearchResult } from "../types";
import { usePlayer } from "../context/PlayerContext";

const HomePage = () => {
  const [searchParams] = useSearchParams();
  const query = searchParams.get("q")?.trim() ?? "";
  const searchQuery = useInfiniteQuery({
    queryKey: ["youtube-search", query],
    queryFn: ({ pageParam }) =>
      invoke<SearchResult[]>("search_youtube", {
        query,
        offset: pageParam,
      }),
    initialPageParam: 0,
    getNextPageParam: (lastPage, allPages) =>
      lastPage.length === 10 ? allPages.length * 10 : undefined,
    enabled: query.length > 0,
  });

  const loadMoreRef = useRef<HTMLDivElement>(null);
  const { playTrack, prefetchTracks } = usePlayer();
  const directUrls = useRef(new Map<string, string>());
  const resolvingIds = useRef(new Set<string>());

  const results = searchQuery.data?.pages.flat() ?? [];

  // 1. Auto-prefetch the top 5 search results as soon as query completes
  useEffect(() => {
    if (results.length > 0) {
      const topIds = results.slice(0, 5).map((r) => r.id);
      prefetchTracks(topIds);
    }
  }, [results, prefetchTracks]);

  // 2. Prefetch individual track on hover/focus if not already resolving
  const prefetchAudioUrl = (videoId: string) => {
    if (directUrls.current.has(videoId) || resolvingIds.current.has(videoId)) {
      return;
    }
    resolvingIds.current.add(videoId);
    void invoke<string>("resolve_audio_url", { videoId })
      .then((url) => directUrls.current.set(videoId, url))
      .catch((error: unknown) =>
        console.warn("Could not prefetch audio URL:", error),
      )
      .finally(() => resolvingIds.current.delete(videoId));
  };

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

  return (
    <section className="mx-auto max-w-6xl">
      <h1 className="text-3xl font-bold text-[#e6edf3]">
        {query ? `Search results for "${query}"` : "Welcome to Fung Preng"}
      </h1>
      {!query && (
        <p className="mt-3 text-[#8b949e]">
          Search for a YouTube track using the search bar above.
        </p>
      )}
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

      <div className="mt-8 grid gap-4 sm:grid-cols-2 lg:grid-cols-3">
        {results.map((result) => (
          <article
            key={result.id}
            className="overflow-hidden rounded-lg border border-[#30363d] bg-[#161b22] hover:bg-[#21262d] cursor-pointer transition"
            onClick={() => void playTrack(result, directUrls.current.get(result.id))}
            onMouseEnter={() => prefetchAudioUrl(result.id)}
            onFocus={() => prefetchAudioUrl(result.id)}
          >
            <img
              alt=""
              className="aspect-video w-full object-cover"
              src={result.thumbnail_url}
            />
            <div className="p-4">
              <h2 className="line-clamp-2 font-medium text-[#e6edf3]">
                {result.title}
              </h2>
              <p className="mt-2 text-sm text-[#8b949e]">{result.channel}</p>
            </div>
          </article>
        ))}
      </div>

      {searchQuery.hasNextPage && (
        <div
          ref={loadMoreRef}
          className="py-8 text-center text-sm text-[#8b949e]"
        >
          {searchQuery.isFetchingNextPage
            ? "Loading more..."
            : "Scroll for more"}
        </div>
      )}
    </section>
  );
};

export default HomePage;