// src/pages/SettingPage.tsx
import React, { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import {
  AudioWaveform,
  Check,
  FileClock,
  History,
  Loader2,
  Music2,
  RotateCcw,
  Trash2,
} from "lucide-react";
import { usePlayer } from "../context/PlayerContext";
import { STORAGE_KEYS } from "../utils/storageKeys";
import { storageUtil } from "../utils/storageUtil";
import { clearListenHistory, getListenHistory } from "../utils/listenHistory";

const STATS_REFRESH_MS = 5000;
const LEGACY_EQ_KEY = "fung-pleng-eq-settings";

const BANDS = [
  { hz: 60, label: "60" },
  { hz: 150, label: "150" },
  { hz: 400, label: "400" },
  { hz: 1000, label: "1k" },
  { hz: 2400, label: "2.4k" },
  { hz: 6000, label: "6k" },
  { hz: 15000, label: "15k" },
] as const;

const PRESETS: Record<string, number[]> = {
  Flat: [0, 0, 0, 0, 0, 0, 0],
  "Bass boost": [6, 5, 3, 0, -1, -1, 0],
  Vocal: [-2, -1, 1, 4, 3, 1, 0],
  Treble: [-2, -1, 0, 1, 3, 5, 6],
  Loudness: [4, 2, 0, -1, 0, 2, 4],
};

function presetForValues(values: number[]) {
  const entry = Object.entries(PRESETS).find(([, v]) =>
    v.every((n, i) => n === values[i]),
  );
  return entry ? entry[0] : "Custom";
}

function formatBytes(bytes: number) {
  if (bytes >= 1024 ** 3) return `${(bytes / 1024 ** 3).toFixed(1)} GB`;
  if (bytes >= 1024 ** 2) return `${(bytes / 1024 ** 2).toFixed(1)} MB`;
  if (bytes >= 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${Math.round(bytes)} B`;
}

function readEqSettings() {
  const current = storageUtil.get<{ enabled?: boolean; values?: number[] }>(
    STORAGE_KEYS.EQ_SETTINGS,
  );
  const legacy = current
    ? null
    : storageUtil.get<{ enabled?: boolean; values?: number[] }>(LEGACY_EQ_KEY);
  const parsed = current ?? legacy;
  if (!parsed) {
    return { enabled: true, values: PRESETS.Flat };
  }
  if (legacy) {
    storageUtil.set(STORAGE_KEYS.EQ_SETTINGS, legacy);
    storageUtil.remove(LEGACY_EQ_KEY);
  }
  const values = Array.isArray(parsed.values) && parsed.values.length === 7
    ? parsed.values.map((value) => Math.max(-12, Math.min(12, Number(value) || 0)))
    : PRESETS.Flat;
  return { enabled: parsed.enabled ?? true, values };
}

type CacheStats = {
  songs: number;
  analysis: number;
  temp: number;
};

type ClearState = "idle" | "confirm" | "clearing" | "done";

function BandSlider({
  hz,
  label,
  value,
  disabled,
  onChange,
}: {
  hz: number;
  label: string;
  value: number;
  disabled: boolean;
  onChange: (hz: number, value: number) => void;
}) {
  return (
    <div className="flex w-9 flex-col items-center">
      <span
        className={`mb-2 min-h-[16px] text-xs tabular-nums ${
          value === 0 ? "text-[#5b6270]" : "text-[#a371f7]"
        }`}
      >
        {value > 0 ? `+${value}` : value}
      </span>
      <div className="relative flex h-[140px] items-center justify-center">
        <div className="absolute h-full w-px bg-[#30363d]" aria-hidden />
        <input
          type="range"
          min={-12}
          max={12}
          step={1}
          value={value}
          disabled={disabled}
          onChange={(e) => onChange(hz, parseInt(e.target.value, 10))}
          aria-label={`${label} Hz gain`}
          className="eq-band-slider h-[140px] w-1 cursor-pointer disabled:cursor-default"
          style={{ writingMode: "vertical-lr", direction: "rtl" }}
        />
      </div>
      <span className="mt-2 text-xs text-[#8b949e]">{label}</span>
    </div>
  );
}

const SettingPage: React.FC = () => {
  // NOTE: adjust these names if your PlayerContext exposes them differently.
  const { queue, currentIndex } = usePlayer();

  // ---------------- Equalizer (UI state only for now) ----------------
  const initialEq = useMemo(() => readEqSettings(), []);
  const [eqEnabled, setEqEnabled] = useState(initialEq.enabled);
  const [values, setValues] = useState<number[]>(initialEq.values);
  const activePreset = useMemo(() => presetForValues(values), [values]);

  useEffect(() => {
    storageUtil.set(STORAGE_KEYS.EQ_SETTINGS, { enabled: eqEnabled, values });

    void invoke("set_eq", { enabled: eqEnabled, gains: values }).catch((error) => {
      console.warn("Failed to apply equalizer settings:", error);
    });
  }, [eqEnabled, values]);

  const setBand = (hz: number, val: number) => {
    const idx = BANDS.findIndex((b) => b.hz === hz);
    setValues((prev) => prev.map((v, i) => (i === idx ? val : v)));
  };
  const applyPreset = (name: string) => setValues(PRESETS[name]);
  const resetEq = () => setValues(PRESETS.Flat);

  // ---------------- Cache ----------------
  const [cache, setCache] = useState<CacheStats>({ songs: 0, analysis: 0, temp: 0 });
  const [clearState, setClearState] = useState<ClearState>("idle");
  const doneTimer = useRef<number | undefined>(undefined);
  const total = cache.songs + cache.analysis + cache.temp;

  const loadCacheStats = useCallback(async () => {
    try {
      setCache(await invoke<CacheStats>("get_cache_stats"));
    } catch (error) {
      console.error("Failed to load cache stats:", error);
    }
  }, []);

  // Live numbers: songs keep caching in the background while this page is open.
  useEffect(() => {
    void loadCacheStats();
    const t = window.setInterval(() => {
      if (!document.hidden) void loadCacheStats();
    }, STATS_REFRESH_MS);
    return () => window.clearInterval(t);
  }, [loadCacheStats]);

  useEffect(() => () => window.clearTimeout(doneTimer.current), []);

  const handleClear = async () => {
    setClearState("clearing");
    // Never delete what is playing now or being prepared for the crossfade.
    const keepIds = [queue?.[currentIndex]?.id, queue?.[currentIndex + 1]?.id].filter(
      (id): id is string => Boolean(id),
    );
    try {
      setCache(await invoke<CacheStats>("clear_cache", { keepIds }));
      setClearState("done");
      doneTimer.current = window.setTimeout(() => setClearState("idle"), 2200);
    } catch (error) {
      console.error("Failed to clear cache:", error);
      setClearState("idle");
    }
  };

  // ---------------- Listening history ----------------
  const [historyCount, setHistoryCount] = useState(0);
  const [historyConfirm, setHistoryConfirm] = useState(false);

  useEffect(() => {
    let active = true;
    void getListenHistory()
      .then((history) => {
        if (active) setHistoryCount(history.length);
      })
      .catch((error) => console.error("Failed to load listening history:", error));
    return () => {
      active = false;
    };
  }, []);

  const resetHistory = async () => {
    try {
      await clearListenHistory();
      setHistoryCount(0);
    } catch (error) {
      console.error("Failed to reset listening history:", error);
    }
    setHistoryConfirm(false);
  };

  const rows = [
    { key: "songs", label: "Downloaded songs", icon: Music2, bytes: cache.songs, dot: "bg-[#a371f7]" },
    { key: "analysis", label: "Metadata & analysis", icon: AudioWaveform, bytes: cache.analysis, dot: "bg-[#58a6ff]" },
    { key: "temp", label: "Temporary files", icon: FileClock, bytes: cache.temp, dot: "bg-[#5b6270]" },
  ] as const;

  return (
    // Scoping wrapper: keeps eq-band-slider rules below from ever touching
    // range inputs outside this page (e.g. the volume slider in BottomPlayerBar).
    <div className="setting-page mx-auto max-w-2xl pb-12">
      <style>{`
        .setting-page .eq-band-slider {
          -webkit-appearance: none;
          appearance: none;
          background: transparent;
        }
        .setting-page .eq-band-slider::-webkit-slider-runnable-track {
          background: #30363d;
          border-radius: 4px;
        }
        .setting-page .eq-band-slider::-webkit-slider-thumb {
          -webkit-appearance: none;
          width: 14px;
          height: 14px;
          border-radius: 50%;
          background: #a371f7;
          border: 2px solid #0d1117;
          cursor: pointer;
        }
        .setting-page .eq-band-slider:disabled::-webkit-slider-thumb {
          background: #5b6270;
          cursor: default;
        }
        .setting-page .eq-band-slider::-moz-range-track {
          background: #30363d;
          border-radius: 4px;
        }
        .setting-page .eq-band-slider::-moz-range-thumb {
          width: 14px;
          height: 14px;
          border-radius: 50%;
          background: #a371f7;
          border: 2px solid #0d1117;
          cursor: pointer;
        }
        .setting-page .eq-band-slider:disabled::-moz-range-thumb {
          background: #5b6270;
          cursor: default;
        }
      `}</style>

      <h1 className="text-2xl font-semibold text-[#e6edf3]">Settings</h1>
      <p className="mt-1 text-sm text-[#8b949e]">
        Tune your sound and manage storage on this device.
      </p>

      {/* ---------------- Equalizer ---------------- */}
      <section className="mt-10">
        <div className="flex items-center justify-between">
          <h2 className="text-[17px] font-semibold text-[#e6edf3]">Equalizer</h2>
          <button
            role="switch"
            aria-checked={eqEnabled}
            aria-label="Enable equalizer"
            onClick={() => setEqEnabled((v) => !v)}
            className={`relative h-[22px] w-10 flex-shrink-0 rounded-full border-none transition-colors ${
              eqEnabled ? "bg-[#a371f7]" : "bg-[#30363d]"
            }`}
          >
            <span
              className="absolute top-[3px] h-4 w-4 rounded-full bg-[#0d1117] transition-transform"
              style={{ left: eqEnabled ? 21 : 3 }}
            />
          </button>
        </div>
        <div className="mb-5 mt-3 h-px bg-[#30363d]" />

        <div
          className={`transition-opacity ${
            eqEnabled ? "opacity-100" : "pointer-events-none opacity-40"
          }`}
          aria-disabled={!eqEnabled}
        >
          <div className="flex flex-wrap gap-2">
            {Object.keys(PRESETS).map((name) => {
              const isActive = activePreset === name;
              return (
                <button
                  key={name}
                  disabled={!eqEnabled}
                  onClick={() => applyPreset(name)}
                  className={`rounded-full border px-3 py-1.5 text-[13px] transition ${
                    isActive
                      ? "border-[#a371f7] bg-[#a371f7]/15 text-[#a371f7]"
                      : "border-[#30363d] text-[#8b949e] hover:text-[#c9d1d9]"
                  }`}
                >
                  {name}
                </button>
              );
            })}
            <button
              disabled={!eqEnabled}
              onClick={resetEq}
              title="Reset to flat"
              className="flex items-center gap-1.5 rounded-full border border-[#30363d] px-2.5 py-1.5 text-[13px] text-[#8b949e] hover:text-[#c9d1d9]"
            >
              <RotateCcw size={13} />
              Reset
            </button>
          </div>

          {activePreset === "Custom" && (
            <p className="mt-3 text-xs text-[#5b6270]">
              Custom curve — adjust any band below.
            </p>
          )}

          <div className="mt-6 flex items-end justify-between rounded-[10px] border border-[#30363d] bg-[#161b22] px-[18px] pb-4 pt-5">
            {BANDS.map((b, i) => (
              <BandSlider
                key={b.hz}
                hz={b.hz}
                label={b.label}
                value={values[i]}
                disabled={!eqEnabled}
                onChange={setBand}
              />
            ))}
          </div>
        </div>
      </section>

      {/* ---------------- Storage ---------------- */}
      <section className="mt-12">
        <h2 className="text-[17px] font-semibold text-[#e6edf3]">Storage</h2>
        <div className="mb-5 mt-3 h-px bg-[#30363d]" />

        <div className="rounded-[10px] border border-[#30363d] bg-[#161b22] p-5">
          <div className="flex items-baseline justify-between gap-3">
            <span className="text-sm text-[#8b949e]">Cache on this device</span>
            <span className="text-[22px] font-semibold text-[#e6edf3]">
              {formatBytes(total)}
            </span>
          </div>

          <div className="mt-4 flex h-1.5 overflow-hidden rounded-full bg-[#0d1117]">
            {total > 0 &&
              rows.map((r) => (
                <div
                  key={r.key}
                  className={r.dot}
                  title={`${r.label}: ${formatBytes(r.bytes)}`}
                  style={{ width: `${(r.bytes / total) * 100}%` }}
                />
              ))}
          </div>

          <div className="mt-5 flex flex-col gap-3">
            {rows.map((r) => {
              const Icon = r.icon;
              return (
                <div key={r.key} className="flex items-center justify-between text-sm">
                  <div className="flex items-center gap-2.5 text-[#8b949e]">
                    <span className={`inline-block h-2 w-2 rounded-sm ${r.dot}`} />
                    <Icon size={15} />
                    {r.label}
                  </div>
                  <span className="text-[#c9d1d9]">{formatBytes(r.bytes)}</span>
                </div>
              );
            })}
          </div>

          <div className="my-[18px] h-px bg-[#30363d]" />

          {clearState === "done" ? (
            <div className="flex items-center gap-2 text-sm text-[#a371f7]">
              <Check size={16} />
              Cache cleared
            </div>
          ) : clearState === "confirm" ? (
            <div className="flex items-center justify-between gap-3">
              <p className="text-sm text-[#8b949e]">
                Songs will need to download again. The current and next song are kept. Continue?
              </p>
              <div className="flex shrink-0 gap-2">
                <button
                  onClick={() => setClearState("idle")}
                  className="rounded-lg border border-[#30363d] px-3 py-[7px] text-[13px] text-[#8b949e] hover:text-[#c9d1d9]"
                >
                  Cancel
                </button>
                <button
                  onClick={() => void handleClear()}
                  className="rounded-lg border-none bg-[#f85149] px-3 py-[7px] text-[13px] text-white hover:bg-[#ff7b72]"
                >
                  Clear cache
                </button>
              </div>
            </div>
          ) : (
            <button
              onClick={() => setClearState("confirm")}
              disabled={total === 0 || clearState === "clearing"}
              className={`flex items-center gap-[7px] rounded-lg border px-[14px] py-[9px] text-[13px] ${
                total === 0
                  ? "cursor-default border-[#30363d] text-[#5b6270]"
                  : "cursor-pointer border-[#4a2620] text-[#f85149] hover:bg-[#f85149]/10"
              }`}
            >
              {clearState === "clearing" ? (
                <>
                  <Loader2 size={14} className="animate-spin" />
                  Clearing…
                </>
              ) : (
                <>
                  <Trash2 size={14} />
                  Clear cache
                </>
              )}
            </button>
          )}
        </div>

        {/* Listening history (drives "For you" on the home page) */}
        <div className="mt-4 rounded-[10px] border border-[#30363d] bg-[#161b22] p-5">
          <div className="flex items-center justify-between gap-3">
            <div className="flex items-center gap-2.5 text-sm text-[#8b949e]">
              <History size={15} />
              {historyCount === 0
                ? "No listening history yet"
                : `${historyCount} ${historyCount === 1 ? "song" : "songs"} shape your recommendations`}
            </div>

            {historyConfirm ? (
              <div className="flex shrink-0 gap-2">
                <button
                  onClick={() => setHistoryConfirm(false)}
                  className="rounded-lg border border-[#30363d] px-3 py-[7px] text-[13px] text-[#8b949e] hover:text-[#c9d1d9]"
                >
                  Cancel
                </button>
                <button
                  onClick={resetHistory}
                  className="rounded-lg border-none bg-[#f85149] px-3 py-[7px] text-[13px] text-white hover:bg-[#ff7b72]"
                >
                  Reset
                </button>
              </div>
            ) : (
              <button
                onClick={() => setHistoryConfirm(true)}
                disabled={historyCount === 0}
                className={`shrink-0 rounded-lg border px-3 py-[7px] text-[13px] ${
                  historyCount === 0
                    ? "cursor-default border-[#30363d] text-[#5b6270]"
                    : "border-[#30363d] text-[#8b949e] hover:text-[#c9d1d9]"
                }`}
              >
                Reset recommendations
              </button>
            )}
          </div>
        </div>
      </section>
    </div>
  );
};

export default SettingPage;