import { create } from "zustand";
import { persist } from "zustand/middleware";

/** Recent and favorite block ids are UI preferences, not project or world data. */
export const PALETTE_STORAGE_KEY = "furybuilder.palette-history";
const PALETTE_STORE_VERSION = 1;
export const MAX_RECENT_BLOCKS = 24;
export const MAX_FAVORITE_BLOCKS = 256;
export const MAX_PALETTE_CATALOGS = 8;

interface PaletteHistory {
  /** Most recently selected first. */
  recent: string[];
  /** Most recently favorited first. */
  favorites: string[];
}

interface PaletteState {
  /** Keyed by the pinned `catalogId`; curated fallback uses its own key. */
  catalogs: Record<string, PaletteHistory>;
  recordRecent: (catalogKey: string, blockId: string) => void;
  toggleFavorite: (catalogKey: string, blockId: string) => void;
}

const EMPTY_HISTORY: PaletteHistory = { recent: [], favorites: [] };

function normalizeIds(value: unknown, limit: number): string[] {
  if (!Array.isArray(value)) return [];
  const ids: string[] = [];
  const seen = new Set<string>();
  for (const item of value) {
    if (typeof item !== "string" || item.length === 0 || seen.has(item)) continue;
    seen.add(item);
    ids.push(item);
    if (ids.length >= limit) break;
  }
  return ids;
}

function normalizeCatalogs(value: unknown): Record<string, PaletteHistory> {
  if (typeof value !== "object" || value === null || Array.isArray(value)) return {};
  const catalogs: [string, PaletteHistory][] = [];
  for (const [key, history] of Object.entries(value)) {
    if (key.length === 0 || typeof history !== "object" || history === null) continue;
    const record = history as Record<string, unknown>;
    const recent = normalizeIds(record.recent, MAX_RECENT_BLOCKS);
    const favorites = normalizeIds(record.favorites, MAX_FAVORITE_BLOCKS);
    if (recent.length === 0 && favorites.length === 0) continue;
    catalogs.push([key, { recent, favorites }]);
  }
  return Object.fromEntries(catalogs.slice(-MAX_PALETTE_CATALOGS));
}

function updateCatalog(
  catalogs: Record<string, PaletteHistory>,
  catalogKey: string,
  update: (history: PaletteHistory) => PaletteHistory,
): Record<string, PaletteHistory> {
  const next = update(Object.hasOwn(catalogs, catalogKey) ? catalogs[catalogKey]! : EMPTY_HISTORY);
  const rest = Object.entries(catalogs).filter(([key]) => key !== catalogKey);
  if (next.recent.length > 0 || next.favorites.length > 0) {
    rest.push([catalogKey, next]);
  }
  return Object.fromEntries(rest.slice(-MAX_PALETTE_CATALOGS));
}

export const usePaletteStore = create<PaletteState>()(
  persist(
    (set) => ({
      catalogs: {},
      recordRecent: (catalogKey, blockId) => {
        if (!catalogKey || !blockId) return;
        set((state) => ({
          catalogs: updateCatalog(state.catalogs, catalogKey, (history) => ({
            ...history,
            recent: [blockId, ...history.recent.filter((id) => id !== blockId)].slice(
              0,
              MAX_RECENT_BLOCKS,
            ),
          })),
        }));
      },
      toggleFavorite: (catalogKey, blockId) => {
        if (!catalogKey || !blockId) return;
        set((state) => ({
          catalogs: updateCatalog(state.catalogs, catalogKey, (history) => ({
            ...history,
            favorites: history.favorites.includes(blockId)
              ? history.favorites.filter((id) => id !== blockId)
              : [blockId, ...history.favorites].slice(0, MAX_FAVORITE_BLOCKS),
          })),
        }));
      },
    }),
    {
      name: PALETTE_STORAGE_KEY,
      version: PALETTE_STORE_VERSION,
      partialize: (state) => ({ catalogs: state.catalogs }),
      // This is a disposable UI cache. A stored shape from another version or a damaged
      // array is ignored and normalized, never migrated into project or world data.
      merge: (persisted, current) => {
        const stored = (persisted ?? {}) as { catalogs?: unknown };
        return { ...current, catalogs: normalizeCatalogs(stored.catalogs) };
      },
    },
  ),
);
