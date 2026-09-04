import { useQuery } from "@tanstack/react-query";
import { invoke } from "@tauri-apps/api/core";
import { ChevronLeft, ChevronRight, Search } from "lucide-react";
import React, { useEffect, useRef, useState } from "react";
import { useNavigate, useSearchParams } from "react-router-dom";

const HeaderBar: React.FC = () => {
  const navigate = useNavigate();
  const [searchParams] = useSearchParams();
  const [search, setSearch] = useState(searchParams.get("q") ?? "");
  const [debouncedSearch, setDebouncedSearch] = useState("");
  const [showSuggestions, setShowSuggestions] = useState(false);
  const searchContainerRef = useRef<HTMLFormElement>(null);

  const suggestionsQuery = useQuery({
    queryKey: ["youtube-suggestions", debouncedSearch],
    queryFn: () =>
      invoke<string[]>("youtube_suggestions", { query: debouncedSearch }),
    enabled: debouncedSearch.length >= 2 && showSuggestions,
    staleTime: 60_000,
    refetchOnWindowFocus: false,
  });
  const suggestions = suggestionsQuery.data ?? [];

  useEffect(() => {
    const timeout = window.setTimeout(() => {
      setDebouncedSearch(search.trim());
    }, 250);
    return () => window.clearTimeout(timeout);
  }, [search]);

  useEffect(() => {
    const handleOutsideClick = (event: MouseEvent) => {
      if (
        searchContainerRef.current &&
        !searchContainerRef.current.contains(event.target as Node)
      ) {
        setShowSuggestions(false);
      }
    };
    const handleEscape = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        setShowSuggestions(false);
      }
    };

    document.addEventListener("mousedown", handleOutsideClick);
    document.addEventListener("keydown", handleEscape);
    return () => {
      document.removeEventListener("mousedown", handleOutsideClick);
      document.removeEventListener("keydown", handleEscape);
    };
  }, []);

  const submitSearch = (event: React.FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    const query = search.trim();
    setShowSuggestions(false);
    navigate(query ? `/?q=${encodeURIComponent(query)}` : "/");
  };

  return (
    <header className="sticky top-0 z-20 flex items-center justify-between border-b border-[#30363d] bg-[#010409]/80 px-6 py-3 backdrop-blur-md">
      <div className="flex items-center gap-2">
        <button
          onClick={() => navigate(-1)}
          className="grid h-8 w-8 place-items-center rounded-full bg-[#21262d] hover:bg-[#30363d] transition text-[#c9d1d9]"
        >
          <ChevronLeft size={18} />
        </button>
        <button
          onClick={() => navigate(1)}
          className="grid h-8 w-8 place-items-center rounded-full bg-[#21262d] hover:bg-[#30363d] transition text-[#c9d1d9]"
        >
          <ChevronRight size={18} />
        </button>
      </div>

      <form
        className="relative w-full max-w-sm"
        onSubmit={submitSearch}
        ref={searchContainerRef}
      >
        <Search
          size={16}
          className="absolute left-3 top-1/2 -translate-y-1/2 text-[#8b949e]"
        />
        <input
          type="text"
          placeholder="What do you want to play?"
          className="w-full rounded-full border border-[#30363d] bg-[#161b22] py-2 pl-9 pr-4 text-sm text-[#c9d1d9] placeholder-[#8b949e] outline-none focus:ring-2 focus:ring-[#58a6ff]"
          onChange={(event) => {
            setSearch(event.currentTarget.value);
            setShowSuggestions(true);
          }}
          onFocus={() => setShowSuggestions(true)}
          value={search}
        />
        {showSuggestions && suggestions.length > 0 && (
          <div className="absolute left-0 right-0 top-full mt-2 overflow-hidden rounded-lg border border-[#30363d] bg-[#161b22] shadow-xl">
            {suggestions.map((suggestion) => (
              <button
                className="block w-full px-4 py-2 text-left text-sm text-[#c9d1d9] hover:bg-[#21262d]"
                key={suggestion}
                onClick={() => {
                  setSearch(suggestion);
                  setShowSuggestions(false);
                  navigate(`/?q=${encodeURIComponent(suggestion)}`);
                }}
                type="button"
              >
                {suggestion}
              </button>
            ))}
          </div>
        )}
      </form>

      <div className="flex items-center gap-3">
        <div className="h-8 w-8 rounded-full bg-gradient-to-br from-[#58a6ff] to-[#1f6feb]" />
      </div>
    </header>
  );
};

export default HeaderBar;
