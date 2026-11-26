/**
 * Search index for the catalog palette.
 *
 * The exact catalog can grow past 10k entries, so the UI precomputes one lowercase haystack
 * per entry and a trigram posting list. Queries of three or more characters start from the
 * rarest trigram instead of scanning every entry; shorter queries use the cached haystack.
 * The index is disposable: if its estimated memory exceeds the budget, callers fall back to
 * the same linear matcher without retaining the index.
 */

export const PALETTE_SEARCH_INDEX_BUDGET_BYTES = 24 * 1024 * 1024;

const TRIGRAM_SIZE = 3;
const RECORD_OVERHEAD_BYTES = 96;
const TRIGRAM_OVERHEAD_BYTES = 32;
const POSTING_BYTES = 8;

export interface PaletteSearchEntry {
  name: string;
  displayName: string | null;
  tags?: readonly string[];
}

export interface PaletteSearchIndex {
  readonly entryCount: number;
  readonly estimatedBytes: number;
  /** `normalizedQuery` must come from `normalizePaletteSearch`. */
  search(normalizedQuery: string): ReadonlySet<string>;
}

/** Lowercases and trims a query once per render instead of once per entry. */
export function normalizePaletteSearch(query: string): string {
  return query.trim().toLowerCase();
}

export function buildPaletteSearchIndex(
  entries: readonly PaletteSearchEntry[],
  budget = PALETTE_SEARCH_INDEX_BUDGET_BYTES,
): PaletteSearchIndex | null {
  const names: string[] = [];
  const searchTexts: string[] = [];
  const postings = new Map<string, number[]>();
  let estimatedBytes = 0;

  for (const entry of entries) {
    const searchText = [entry.name, entry.displayName ?? "", ...(entry.tags ?? [])]
      .join("\n")
      .toLowerCase();
    estimatedBytes += RECORD_OVERHEAD_BYTES + (entry.name.length + searchText.length) * 2;
    const position = searchTexts.length;
    names.push(entry.name);
    searchTexts.push(searchText);

    const seen = new Set<string>();
    for (let offset = 0; offset <= searchText.length - TRIGRAM_SIZE; offset += 1) {
      const trigram = searchText.slice(offset, offset + TRIGRAM_SIZE);
      if (seen.has(trigram)) continue;
      seen.add(trigram);
      let list = postings.get(trigram);
      if (!list) {
        list = [];
        postings.set(trigram, list);
        estimatedBytes += TRIGRAM_OVERHEAD_BYTES;
      }
      list.push(position);
      estimatedBytes += POSTING_BYTES;
    }
    if (estimatedBytes > budget) return null;
  }

  return {
    entryCount: entries.length,
    estimatedBytes,
    search(normalizedQuery) {
      const matches = new Set<string>();
      if (!normalizedQuery) {
        for (const name of names) matches.add(name);
        return matches;
      }

      if (normalizedQuery.length < TRIGRAM_SIZE) {
        for (let index = 0; index < searchTexts.length; index += 1) {
          if (searchTexts[index].includes(normalizedQuery)) matches.add(names[index]);
        }
        return matches;
      }

      let rarest: number[] | null = null;
      const seen = new Set<string>();
      for (let offset = 0; offset <= normalizedQuery.length - TRIGRAM_SIZE; offset += 1) {
        const trigram = normalizedQuery.slice(offset, offset + TRIGRAM_SIZE);
        if (seen.has(trigram)) continue;
        seen.add(trigram);
        const list = postings.get(trigram);
        if (!list) return matches;
        if (!rarest || list.length < rarest.length) rarest = list;
      }
      if (!rarest) return matches;
      for (const candidate of rarest) {
        if (searchTexts[candidate].includes(normalizedQuery)) matches.add(names[candidate]);
      }
      return matches;
    },
  };
}
