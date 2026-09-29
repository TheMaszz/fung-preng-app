import { invoke } from "@tauri-apps/api/core";
import { storageUtil } from "./storageUtil";
import { STORAGE_KEYS } from "./storageKeys";

export type ListenHistoryEntry = {
  id: string;
  title: string;
  channel: string;
  count: number;
  lastPlayedAt: number;
};

let migrationPromise: Promise<void> | undefined;

export async function migrateLegacyListenHistory() {
  if (typeof window === "undefined") return;
  if (!migrationPromise) {
    migrationPromise = (async () => {
      const legacy = storageUtil.get<Record<string, ListenHistoryEntry>>(
        STORAGE_KEYS.LISTEN_HISTORY,
      );
      if (!legacy) return;

      await invoke("import_legacy_listen_history", {
        entries: Object.values(legacy),
      });
      storageUtil.remove(STORAGE_KEYS.LISTEN_HISTORY);
    })();
  }

  try {
    await migrationPromise;
  } catch (error) {
    migrationPromise = undefined;
    throw error;
  }
}

export async function getListenHistory() {
  await migrateLegacyListenHistory();
  return invoke<ListenHistoryEntry[]>("get_listen_history");
}

export async function recordListen(track: {
  id: string;
  title: string;
  channel: string;
}) {
  await migrateLegacyListenHistory();
  await invoke("record_listen_history", track);
}

export async function clearListenHistory() {
  await migrateLegacyListenHistory();
  await invoke("clear_listen_history");
}
