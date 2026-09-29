import { useQuery } from "@tanstack/react-query";
import { invoke } from "@tauri-apps/api/core";
import { ChevronLeft, ChevronRight, Search, X } from "lucide-react";
import React, { useEffect, useId, useRef, useState } from "react";
import { useLocation, useNavigate, useSearchParams } from "react-router-dom";

const focusRing =
  "focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-[#58a6ff] focus-visible:ring-offset-2 focus-visible:ring-offset-[#010409]";

const navButton = `grid h-8 w-8 shrink-0 place-items-center rounded-full bg-[#21262d] text-[#c9d1d9] transition-colors hover:bg-[#30363d] active:bg-[#484f58] disabled:cursor-not-allowed disabled:opacity-40 disabled:hover:bg-[#21262d] ${focusRing}`;

const HeaderBar: React.FC = () => {
  const navigate = useNavigate();
  const location = useLocation(); // re-renders on navigation so canGoBack stays fresh
  const [searchParams] = useSearchParams();
  const urlQuery = searchParams.get("q") ?? "";

  const [search, setSearch] = useState(urlQuery);
  const [debouncedSearch, setDebouncedSearch] = useState("");
  const [showSuggestions, setShowSuggestions] = useState(false);
  const [activeIndex, setActiveIndex] = useState(-1);

  const searchContainerRef = useRef<HTMLFormElement>(null);
  const inputRef = useRef<HTMLInputElement>(null);
  const listboxId = useId();

  const suggestionsQuery = useQuery({
    queryKey: ["youtube-suggestions", debouncedSearch],
    queryFn: () =>
      invoke<string[]>("youtube_suggestions", { query: debouncedSearch }),
    enabled: debouncedSearch.length >= 2 && showSuggestions,
    staleTime: 60_000,
    refetchOnWindowFocus: false,
    placeholderData: (previous: string[] | undefined) => previous,
  });

  const suggestions =
    search.trim().length >= 2 ? (suggestionsQuery.data ?? []) : [];
  const isOpen = showSuggestions && suggestions.length > 0;

  const historyIdx = window.history.state?.idx as number | undefined;
  const canGoBack = historyIdx === undefined ? true : historyIdx > 0;

  useEffect(() => {
    const timeout = window.setTimeout(() => {
      setDebouncedSearch(search.trim());
    }, 250);
    return () => window.clearTimeout(timeout);
  }, [search]);

  useEffect(() => {
    setSearch(urlQuery);
  }, [urlQuery]);

  useEffect(() => {
    setActiveIndex(-1);
  }, [debouncedSearch, showSuggestions]);

  useEffect(() => {
    const handleOutsidePointer = (event: PointerEvent) => {
      if (!searchContainerRef.current?.contains(event.target as Node)) {
        setShowSuggestions(false);
      }
    };
    document.addEventListener("pointerdown", handleOutsidePointer);
    return () =>
      document.removeEventListener("pointerdown", handleOutsidePointer);
  }, []);

  useEffect(() => {
    const handleSlash = (event: KeyboardEvent) => {
      const el = event.target as HTMLElement;
      const isTyping =
        el.tagName === "INPUT" ||
        el.tagName === "TEXTAREA" ||
        el.isContentEditable;
      if (
        event.key === "/" &&
        !isTyping &&
        !event.metaKey &&
        !event.ctrlKey &&
        !event.altKey
      ) {
        event.preventDefault();
        inputRef.current?.focus();
      }
    };
    document.addEventListener("keydown", handleSlash);
    return () => document.removeEventListener("keydown", handleSlash);
  }, []);

  const goToQuery = (query: string) => {
    const trimmed = query.trim();
    setShowSuggestions(false);
    navigate(trimmed ? `/?q=${encodeURIComponent(trimmed)}` : "/");
  };

  const selectSuggestion = (suggestion: string) => {
    setSearch(suggestion);
    goToQuery(suggestion);
  };

  const submitSearch = (event: React.FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    goToQuery(search);
  };

  const handleInputKeyDown = (event: React.KeyboardEvent<HTMLInputElement>) => {
    if (!suggestions.length) return;

    if (event.key === "ArrowDown") {
      event.preventDefault();
      if (!isOpen) return setShowSuggestions(true);
      setActiveIndex((i) => (i + 1) % suggestions.length);
    } else if (event.key === "ArrowUp") {
      event.preventDefault();
      if (!isOpen) return setShowSuggestions(true);
      setActiveIndex((i) => (i <= 0 ? suggestions.length - 1 : i - 1));
    } else if (event.key === "Enter" && isOpen && activeIndex >= 0) {
      event.preventDefault();
      selectSuggestion(suggestions[activeIndex]);
    }
  };

  return (
    <header
      className="sticky top-0 z-20 flex items-center gap-4 border-b border-[#30363d] bg-[#010409]/80 px-4 py-3 backdrop-blur-md sm:px-6"
      data-path={location.pathname}
    >
      <nav aria-label="History" className="flex shrink-0 items-center gap-2">
        <button
          type="button"
          onClick={() => navigate(-1)}
          disabled={!canGoBack}
          aria-label="Go back"
          className={navButton}
        >
          <ChevronLeft size={18} aria-hidden="true" />
        </button>
        <button
          type="button"
          onClick={() => navigate(1)}
          aria-label="Go forward"
          className={navButton}
        >
          <ChevronRight size={18} aria-hidden="true" />
        </button>
      </nav>

      <form
        role="search"
        ref={searchContainerRef}
        onSubmit={submitSearch}
        onKeyDown={(event) => {
          if (event.key === "Escape") setShowSuggestions(false);
        }}
        className="relative mx-auto w-full max-w-md"
      >
        <Search
          size={16}
          aria-hidden="true"
          className="pointer-events-none absolute left-3.5 top-1/2 -translate-y-1/2 text-[#8b949e]"
        />

        <input
          ref={inputRef}
          type="text"
          role="combobox"
          aria-label="Search"
          aria-expanded={isOpen}
          aria-controls={listboxId}
          aria-autocomplete="list"
          aria-activedescendant={
            activeIndex >= 0 ? `${listboxId}-${activeIndex}` : undefined
          }
          autoComplete="off"
          spellCheck={false}
          placeholder="What do you want to play?"
          value={search}
          onChange={(event) => {
            setSearch(event.currentTarget.value);
            setShowSuggestions(true);
          }}
          onFocus={() => setShowSuggestions(true)}
          onKeyDown={handleInputKeyDown}
          className="w-full rounded-full border border-[#30363d] bg-[#161b22] py-2 pl-10 pr-10 text-sm text-[#c9d1d9] placeholder-[#8b949e] outline-none transition-colors hover:border-[#484f58] focus:border-[#58a6ff] focus:ring-2 focus:ring-[#58a6ff]/30"
        />

        {search ? (
          <button
            type="button"
            aria-label="Clear search"
            onClick={() => {
              setSearch("");
              setShowSuggestions(false);
              inputRef.current?.focus();
            }}
            className={`absolute right-2.5 top-1/2 grid h-6 w-6 -translate-y-1/2 place-items-center rounded-full text-[#8b949e] transition-colors hover:bg-[#30363d] hover:text-[#c9d1d9] ${focusRing}`}
          >
            <X size={14} aria-hidden="true" />
          </button>
        ) : (
          <kbd
            aria-hidden="true"
            className="pointer-events-none absolute right-3 top-1/2 hidden -translate-y-1/2 rounded border border-[#30363d] px-1.5 text-xs leading-5 text-[#8b949e] sm:block"
          >
            /
          </kbd>
        )}

        {isOpen && (
          <ul
            id={listboxId}
            role="listbox"
            aria-label="Search suggestions"
            className="absolute left-0 right-0 top-full mt-2 max-h-72 overflow-y-auto rounded-xl border border-[#30363d] bg-[#161b22] p-1 shadow-[0_8px_24px_rgba(1,4,9,0.6)]"
          >
            {suggestions.map((suggestion, index) => (
              <li
                key={suggestion}
                id={`${listboxId}-${index}`}
                role="option"
                aria-selected={index === activeIndex}
                // mousedown so the input doesn't blur before the click lands
                onMouseDown={(event) => {
                  event.preventDefault();
                  selectSuggestion(suggestion);
                }}
                onMouseEnter={() => setActiveIndex(index)}
                className={`flex cursor-pointer items-center gap-3 rounded-lg px-3 py-2 text-sm text-[#c9d1d9] ${
                  index === activeIndex ? "bg-[#21262d]" : ""
                }`}
              >
                <Search
                  size={14}
                  aria-hidden="true"
                  className="shrink-0 text-[#8b949e]"
                />
                <span className="truncate">{suggestion}</span>
              </li>
            ))}
          </ul>
        )}
      </form>
    </header>
  );
};

export default HeaderBar;