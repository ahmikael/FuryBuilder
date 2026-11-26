import { useEffect, useMemo, useRef, useState } from "react";
import { AlertTriangle, ChevronDown, Loader2, RotateCcw, Search, Star, X } from "lucide-react";
import type { CatalogFlags, CatalogIconAtlas, CatalogSnapshot } from "@/assets/types";
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent } from "@/components/ui/card";
import { Collapsible, CollapsibleContent, CollapsibleTrigger } from "@/components/ui/collapsible";
import {
  Empty,
  EmptyDescription,
  EmptyHeader,
  EmptyMedia,
  EmptyTitle,
} from "@/components/ui/empty";
import { InputGroup, InputGroupAddon, InputGroupInput } from "@/components/ui/input-group";
import {
  Select,
  SelectContent,
  SelectGroup,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Separator } from "@/components/ui/separator";
import type { BlockDefinition } from "@/editor/types";
import {
  CatalogBlockIcon,
  catalogIconIndex,
  type CatalogIconIndex,
} from "@/features/workspace/CatalogBlockIcon";
import { CatalogStatePicker } from "@/features/workspace/CatalogStatePicker";
import {
  PALETTE_CATEGORIES,
  PALETTE_FLAG_FILTERS,
  categorizeBlock,
  matchesPaletteFlagFilter,
  matchesPaletteSearch,
  paletteCategoryLabel,
  paletteFlagFilterLabel,
  type PaletteFilterId,
  type PaletteFlagFilterId,
  type PaletteGroupId,
} from "@/features/workspace/palette-categories";
import {
  buildPaletteSearchIndex,
  normalizePaletteSearch,
} from "@/features/workspace/palette-search-index";
import { loadProjectCatalog, loadProjectCatalogAtlas } from "@/lib/assets-api";
import { isTauriRuntime } from "@/lib/ipc";
import { fetchCuratedBlockCatalog } from "@/lib/world-api";
import { usePaletteStore } from "@/state/palette-store";
import { useToolStore } from "@/state/tool-store";

interface PaletteEntry {
  name: string;
  displayName: string | null;
  iconTexture: string | null;
  properties: Record<string, string>;
  propertyValues: Record<string, string[]>;
  propertyValuesComplete: boolean;
  multipart: boolean;
  groups: PaletteGroupId[];
  tags: string[];
  flags: CatalogFlags | null;
}

interface UnavailableQuickPick {
  name: string;
  displayName: string | null;
}

interface PaletteLoad {
  catalog: CatalogSnapshot | null;
  curated: BlockDefinition[] | null;
  fallbackReason: string | null;
  error: string | null;
}

const PLACEHOLDER_REASON =
  "This project uses placeholder textures, so it has no client JAR catalog. Showing curated quick picks instead.";
const EMPTY_IDS: readonly string[] = [];
const FILTER_OPTIONS = PALETTE_CATEGORIES.map(({ id, label }) => ({ value: id, label }));
const FLAG_FILTER_OPTIONS = PALETTE_FLAG_FILTERS.map(({ id, label }) => ({ value: id, label }));
const ROW_HEIGHT = 32;
const VIRTUAL_OVERSCAN = 6;
const FALLBACK_VIEWPORT_HEIGHT = 320;
const ICON_REQUEST_DEBOUNCE_MS = 80;
const MAX_ICON_ATLAS_BATCHES = 8;
const MAX_ICON_ATLAS_BYTES = 16 * 1024 * 1024;

function atlasBatchBytes(atlas: CatalogIconAtlas): number {
  return atlas.pngBase64.length + atlas.textures.length * 64;
}

/** Keeps the most recent lazy icon batches inside the WebView memory budget. */
function mergeAtlasBatches(
  current: readonly CatalogIconAtlas[],
  next: CatalogIconAtlas,
): CatalogIconAtlas[] {
  const merged = [
    ...current.filter(
      (batch) => batch.catalogId === next.catalogId && batch.cacheKey !== next.cacheKey,
    ),
    next,
  ];
  const kept: CatalogIconAtlas[] = [];
  let bytes = 0;
  for (let index = merged.length - 1; index >= 0; index -= 1) {
    const batch = merged[index];
    const batchBytes = atlasBatchBytes(batch);
    if (kept.length >= MAX_ICON_ATLAS_BATCHES || bytes + batchBytes > MAX_ICON_ATLAS_BYTES) {
      continue;
    }
    kept.unshift(batch);
    bytes += batchBytes;
  }
  return kept;
}

function shortBlockId(id: string): string {
  return id.startsWith("minecraft:") ? id.slice("minecraft:".length) : id;
}

function reasonMessage(reason: unknown): string {
  return reason instanceof Error ? reason.message : String(reason);
}

function formatProperties(properties: Record<string, string>): string {
  return Object.keys(properties)
    .sort()
    .map((key) => `${key}=${properties[key]}`)
    .join(", ");
}

async function loadPalette(): Promise<PaletteLoad> {
  const [catalogResult, curatedResult] = await Promise.allSettled([
    loadProjectCatalog(),
    fetchCuratedBlockCatalog(),
  ]);
  const curated = curatedResult.status === "fulfilled" ? curatedResult.value : null;

  if (catalogResult.status === "fulfilled" && catalogResult.value) {
    return { catalog: catalogResult.value, curated, fallbackReason: null, error: null };
  }

  const fallbackReason =
    catalogResult.status === "fulfilled"
      ? PLACEHOLDER_REASON
      : `The project catalog could not be rebuilt: ${reasonMessage(catalogResult.reason)}. Showing curated quick picks instead.`;

  if (curated) {
    return { catalog: null, curated, fallbackReason, error: null };
  }

  return {
    catalog: null,
    curated: null,
    fallbackReason,
    error:
      curatedResult.status === "rejected"
        ? reasonMessage(curatedResult.reason)
        : "The curated quick-pick catalog is unavailable.",
  };
}

export function BlockPalette() {
  const [result, setResult] = useState<PaletteLoad | null>(null);
  const [attempt, setAttempt] = useState(0);
  const { catalog, curated, fallbackReason, error } = result ?? {};
  const [search, setSearch] = useState("");
  const [category, setCategory] = useState<PaletteFilterId>("all");
  const [flagFilter, setFlagFilter] = useState<PaletteFlagFilterId>("any");
  const [atlasBatches, setAtlasBatches] = useState<CatalogIconAtlas[]>([]);
  const [atlasError, setAtlasError] = useState<{ catalogId: string; message: string } | null>(null);
  const [scrollTop, setScrollTop] = useState(0);
  const [viewportHeight, setViewportHeight] = useState(0);
  const listRef = useRef<HTMLDivElement>(null);
  const selectedBlock = useToolStore((state) => state.selectedBlock);
  const setSelectedBlock = useToolStore((state) => state.setSelectedBlock);
  const setReplaceFrom = useToolStore((state) => state.setReplaceFrom);
  const replaceFrom = useToolStore((state) => state.replaceFrom);
  const paletteKey = catalog?.catalogId ?? "curated";
  const recentIds = usePaletteStore((state) => state.catalogs[paletteKey]?.recent ?? EMPTY_IDS);
  const favoriteIds = usePaletteStore(
    (state) => state.catalogs[paletteKey]?.favorites ?? EMPTY_IDS,
  );
  const recordRecent = usePaletteStore((state) => state.recordRecent);
  const toggleFavorite = usePaletteStore((state) => state.toggleFavorite);
  const catalogId = catalog?.catalogId ?? null;

  useEffect(() => {
    if (!isTauriRuntime()) return;
    let cancelled = false;
    void loadPalette().then((result) => {
      if (!cancelled) setResult(result);
    });
    return () => {
      cancelled = true;
    };
  }, [attempt]);

  useEffect(() => {
    const element = listRef.current;
    if (!element) return;
    const measure = () => setViewportHeight(element.clientHeight);
    measure();
    window.addEventListener("resize", measure);
    let observer: ResizeObserver | null = null;
    if (typeof ResizeObserver !== "undefined") {
      observer = new ResizeObserver(measure);
      observer.observe(element);
    }
    return () => {
      window.removeEventListener("resize", measure);
      observer?.disconnect();
    };
  }, []);

  const resetScroll = () => {
    setScrollTop(0);
    if (listRef.current) listRef.current.scrollTop = 0;
  };

  const palette = useMemo<PaletteEntry[]>(() => {
    if (catalog) {
      return catalog.entries.flatMap((entry) => {
        const states = entry.states;
        if (!states) return [];
        return [
          {
            name: entry.id,
            displayName: entry.displayName,
            iconTexture: entry.iconTexture,
            properties: states.defaultProperties ?? {},
            propertyValues: states.properties,
            propertyValuesComplete: states.propertyValuesComplete,
            multipart: states.multipart,
            groups: categorizeBlock(entry.id, entry.tags),
            tags: entry.tags,
            flags: entry.flags,
          },
        ];
      });
    }
    if (curated) {
      return curated.map((entry) => ({
        name: entry.name,
        displayName: null,
        iconTexture: null,
        properties: entry.properties,
        propertyValues: Object.fromEntries(
          Object.entries(entry.properties).map(([key, value]) => [key, [value]]),
        ),
        propertyValuesComplete: false,
        multipart: false,
        groups: categorizeBlock(entry.name, []),
        tags: [],
        flags: null,
      }));
    }
    return [];
  }, [catalog, curated]);

  const searchIndex = useMemo(() => buildPaletteSearchIndex(palette), [palette]);
  const entryById = useMemo(() => new Map(palette.map((entry) => [entry.name, entry])), [palette]);
  const favoriteSet = useMemo(() => new Set(favoriteIds), [favoriteIds]);
  const recentEntries = useMemo(
    () =>
      recentIds
        .map((id) => entryById.get(id))
        .filter((entry): entry is PaletteEntry => Boolean(entry)),
    [recentIds, entryById],
  );
  const favoriteEntries = useMemo(
    () =>
      favoriteIds
        .map((id) => entryById.get(id))
        .filter((entry): entry is PaletteEntry => Boolean(entry)),
    [favoriteIds, entryById],
  );

  const categoryCounts = useMemo<Record<PaletteFilterId, number>>(() => {
    const counts: Record<PaletteFilterId, number> = {
      all: palette.length,
      recent: recentEntries.length,
      favorites: favoriteEntries.length,
      building: 0,
      colored: 0,
      natural: 0,
      functional: 0,
      redstone: 0,
      technical: 0,
      other: 0,
    };
    for (const entry of palette) {
      for (const group of entry.groups) counts[group] += 1;
    }
    return counts;
  }, [palette, recentEntries.length, favoriteEntries.length]);

  const visible = useMemo(() => {
    let entries: PaletteEntry[];
    if (category === "recent") {
      entries = recentEntries;
    } else if (category === "favorites") {
      entries = favoriteEntries;
    } else if (category === "all") {
      entries = palette;
    } else {
      entries = palette.filter((entry) => entry.groups.includes(category));
    }
    const needle = normalizePaletteSearch(search);
    if (!needle) {
      return entries.filter((entry) => matchesPaletteFlagFilter(entry.flags, flagFilter));
    }
    const matchedNames = searchIndex?.search(needle) ?? null;
    return entries
      .filter((entry) =>
        matchedNames ? matchedNames.has(entry.name) : matchesPaletteSearch(entry, search),
      )
      .filter((entry) => matchesPaletteFlagFilter(entry.flags, flagFilter));
  }, [category, palette, recentEntries, favoriteEntries, search, flagFilter, searchIndex]);

  const unavailableQuickPicks = useMemo<UnavailableQuickPick[]>(() => {
    if (!catalog || !curated) return [];
    const available = new Set(palette.map((entry) => entry.name));
    const seen = new Set<string>();
    return curated
      .filter((entry) => !available.has(entry.name))
      .filter((entry) => matchesPaletteSearch({ name: entry.name, displayName: null }, search))
      .filter((entry) => {
        if (seen.has(entry.name)) return false;
        seen.add(entry.name);
        return true;
      })
      .map((entry) => ({ name: entry.name, displayName: null }));
  }, [catalog, curated, palette, search]);

  const effectiveViewportHeight = viewportHeight || FALLBACK_VIEWPORT_HEIGHT;
  const maxScrollTop = Math.max(0, visible.length * ROW_HEIGHT - effectiveViewportHeight);
  const clampedScrollTop = Math.min(scrollTop, maxScrollTop);
  const startIndex = Math.max(0, Math.floor(clampedScrollTop / ROW_HEIGHT) - VIRTUAL_OVERSCAN);
  const endIndex = Math.min(
    visible.length,
    Math.ceil((clampedScrollTop + effectiveViewportHeight) / ROW_HEIGHT) + VIRTUAL_OVERSCAN,
  );
  const windowEntries = visible.slice(startIndex, endIndex);

  const definition = entryById.get(selectedBlock.name) ?? null;
  const loadedAtlasBatches = useMemo(
    () => atlasBatches.filter((batch) => batch.catalogId === catalogId),
    [atlasBatches, catalogId],
  );
  const iconIndex = useMemo<CatalogIconIndex | null>(
    () => (loadedAtlasBatches.length > 0 ? catalogIconIndex(loadedAtlasBatches) : null),
    [loadedAtlasBatches],
  );
  const desiredTextureKeys = useMemo(() => {
    const keys = new Set<string>();
    for (const entry of windowEntries) {
      if (entry.iconTexture) keys.add(entry.iconTexture);
    }
    if (definition?.iconTexture) keys.add(definition.iconTexture);
    return [...keys].sort();
  }, [windowEntries, definition]);
  const missingIconKey = desiredTextureKeys
    .filter((textureKey) => !iconIndex?.has(textureKey))
    .join("\n");

  useEffect(() => {
    if (!isTauriRuntime() || !catalogId || !missingIconKey) return;
    let cancelled = false;
    const textureKeys = missingIconKey.split("\n");
    const timer = window.setTimeout(() => {
      void loadProjectCatalogAtlas(textureKeys)
        .then((next) => {
          if (cancelled || !next || next.catalogId !== catalogId) return;
          setAtlasBatches((current) => mergeAtlasBatches(current, next));
          setAtlasError(null);
        })
        .catch((reason: unknown) => {
          if (!cancelled) setAtlasError({ catalogId, message: reasonMessage(reason) });
        });
    }, ICON_REQUEST_DEBOUNCE_MS);
    return () => {
      cancelled = true;
      window.clearTimeout(timer);
    };
  }, [catalogId, missingIconKey]);

  const overrides = selectedBlock.properties ?? {};
  const selectedStateSummary = formatProperties(overrides);
  const declaredDefaultSummary = definition ? formatProperties(definition.properties) : "";
  const source = catalog ? "clientJar" : curated ? "curated" : null;
  const loading = result === null;
  const selectedFavorite = definition ? favoriteSet.has(definition.name) : false;
  const visibleAtlasError = atlasError?.catalogId === catalogId ? atlasError.message : null;
  const showUnavailable =
    category === "all" && flagFilter === "any" && unavailableQuickPicks.length > 0;

  const selectEntry = (entry: PaletteEntry) => {
    setSelectedBlock({
      name: entry.name,
      properties: { ...entry.properties },
    });
    recordRecent(paletteKey, entry.name);
  };

  const retry = () => {
    setResult(null);
    setAttempt((current) => current + 1);
  };

  const emptyListMessage = search.trim()
    ? `No block matches “${search.trim()}”${
        category === "all" ? "" : ` in ${paletteCategoryLabel(category)}`
      }${flagFilter === "any" ? "" : ` with ${paletteFlagFilterLabel(flagFilter).toLowerCase()}`}.`
    : flagFilter !== "any" && category === "all"
      ? `No blocks match ${paletteFlagFilterLabel(flagFilter).toLowerCase()}.`
      : category === "recent"
        ? "No recent blocks yet. Select a block to add it here."
        : category === "favorites"
          ? "No favorites yet. Use the star on a block to pin it."
          : category === "all"
            ? "This catalog has no blocks."
            : `No blocks in ${paletteCategoryLabel(category)}.`;

  if (!isTauriRuntime()) {
    return (
      <Empty className="h-full border-0">
        <EmptyHeader>
          <EmptyMedia variant="icon">
            <Search />
          </EmptyMedia>
          <EmptyTitle>Desktop core required</EmptyTitle>
          <EmptyDescription>The block catalog requires the desktop app.</EmptyDescription>
        </EmptyHeader>
      </Empty>
    );
  }

  if (error) {
    return (
      <Empty className="h-full border-0">
        <EmptyHeader>
          <EmptyMedia variant="icon">
            <AlertTriangle className="text-status-warn" />
          </EmptyMedia>
          <EmptyTitle>Block catalog unavailable</EmptyTitle>
          <EmptyDescription>{error}</EmptyDescription>
        </EmptyHeader>
        <Button size="sm" variant="outline" onClick={retry}>
          Retry
        </Button>
      </Empty>
    );
  }

  return (
    <div className="flex min-h-0 flex-1 gap-4">
      <div className="flex min-h-0 min-w-0 w-full max-w-xl flex-col gap-2">
        <div className="flex flex-wrap items-center gap-1.5">
          <Badge variant={catalog ? "secondary" : "outline"}>
            {catalog
              ? `Client JAR · ${catalog.minecraftVersion}`
              : loading
                ? "Loading catalog"
                : "Curated quick picks"}
          </Badge>
          {catalog ? (
            <span className="font-mono text-[10px] text-muted-foreground">
              {catalog.counts.blocks} blocks · {catalog.catalogId}
            </span>
          ) : null}
        </div>
        {fallbackReason ? (
          <Alert className="shrink-0">
            <AlertTriangle />
            <AlertTitle>Curated fallback</AlertTitle>
            <AlertDescription>{fallbackReason}</AlertDescription>
          </Alert>
        ) : null}
        <div className="flex shrink-0 items-center gap-2">
          <InputGroup>
            <InputGroupInput
              placeholder={`Search ${palette.length} blocks…`}
              value={search}
              disabled={loading}
              onChange={(event) => {
                setSearch(event.target.value);
                resetScroll();
              }}
              aria-label="Search blocks"
            />
            <InputGroupAddon>
              <Search aria-hidden="true" />
            </InputGroupAddon>
          </InputGroup>
          <Select
            items={FILTER_OPTIONS}
            value={category}
            disabled={loading}
            onValueChange={(value) => {
              resetScroll();
              setCategory(value ?? "all");
            }}
          >
            <SelectTrigger className="w-36 shrink-0" aria-label="Block category">
              <SelectValue />
            </SelectTrigger>
            <SelectContent alignItemWithTrigger={false}>
              <SelectGroup>
                {PALETTE_CATEGORIES.map(({ id, label }) => (
                  <SelectItem key={id} value={id}>
                    {label} ({categoryCounts[id]})
                  </SelectItem>
                ))}
              </SelectGroup>
            </SelectContent>
          </Select>
          <Select
            items={FLAG_FILTER_OPTIONS}
            value={flagFilter}
            disabled={loading}
            onValueChange={(value) => {
              resetScroll();
              setFlagFilter((value as PaletteFlagFilterId | null) ?? "any");
            }}
          >
            <SelectTrigger className="w-36 shrink-0" aria-label="Block filter">
              <SelectValue />
            </SelectTrigger>
            <SelectContent alignItemWithTrigger={false}>
              <SelectGroup>
                {PALETTE_FLAG_FILTERS.map(({ id, label }) => (
                  <SelectItem key={id} value={id}>
                    {label}
                  </SelectItem>
                ))}
              </SelectGroup>
            </SelectContent>
          </Select>
        </div>
        <div
          ref={listRef}
          role="list"
          aria-label="Block palette"
          className="scrollbar-thin flex min-h-0 flex-1 flex-col overflow-y-auto rounded-lg border"
          onScroll={(event) => setScrollTop(event.currentTarget.scrollTop)}
        >
          {loading ? (
            <div
              role="status"
              className="flex items-center gap-1.5 p-2 text-xs text-muted-foreground"
            >
              <Loader2 className="size-3 animate-spin" /> Reading project catalog…
            </div>
          ) : visible.length === 0 ? (
            <Empty className="p-2" role="status">
              <EmptyHeader>
                <EmptyTitle>{emptyListMessage}</EmptyTitle>
              </EmptyHeader>
            </Empty>
          ) : (
            <div
              className="relative w-full"
              style={{ height: visible.length * ROW_HEIGHT }}
              aria-hidden="false"
            >
              {windowEntries.map((entry, offset) => {
                const index = startIndex + offset;
                const active = entry.name === selectedBlock.name;
                const favorite = favoriteSet.has(entry.name);
                return (
                  <div
                    key={entry.name}
                    role="listitem"
                    aria-setsize={visible.length}
                    aria-posinset={index + 1}
                    className="absolute inset-x-0 flex items-center gap-1 px-1"
                    style={{ top: index * ROW_HEIGHT, height: ROW_HEIGHT }}
                  >
                    <Button
                      type="button"
                      variant={active ? "secondary" : "ghost"}
                      size="sm"
                      title={
                        entry.displayName ? `${entry.displayName} · ${entry.name}` : entry.name
                      }
                      aria-pressed={active}
                      onClick={() => selectEntry(entry)}
                      className="min-w-0 flex-1 justify-start text-left"
                    >
                      <CatalogBlockIcon
                        name={entry.name}
                        textureKey={entry.iconTexture}
                        icons={iconIndex}
                        size={20}
                      />
                      <span className="min-w-0 flex-1 truncate text-xs">
                        <span className="font-mono">{shortBlockId(entry.name)}</span>
                        {entry.displayName ? (
                          <span className="ml-1.5 text-muted-foreground">{entry.displayName}</span>
                        ) : null}
                      </span>
                    </Button>
                    <Button
                      variant="ghost"
                      size="icon-xs"
                      className="shrink-0"
                      aria-label={`Favorite ${shortBlockId(entry.name)}`}
                      title={favorite ? "Remove from favorites" : "Add to favorites"}
                      aria-pressed={favorite}
                      onClick={() => toggleFavorite(paletteKey, entry.name)}
                    >
                      <Star className={favorite ? "fill-current" : undefined} />
                    </Button>
                  </div>
                );
              })}
            </div>
          )}
        </div>
        {showUnavailable ? (
          <Collapsible className="shrink-0 rounded-lg border">
            <CollapsibleTrigger className="flex w-full cursor-pointer items-center justify-between gap-2 px-2 py-1.5 text-left [&[data-panel-open]>svg]:rotate-180">
              <span className="text-xs font-medium">Quick picks not in this version</span>
              <span className="flex items-center gap-1">
                <Badge variant="outline">{unavailableQuickPicks.length}</Badge>
                <ChevronDown className="size-3.5 transition-transform" aria-hidden="true" />
              </span>
            </CollapsibleTrigger>
            <CollapsibleContent>
              <div className="px-2 pb-1.5">
                <Separator className="mb-1.5" />
                <p className="pb-1.5 text-[11px] leading-relaxed text-muted-foreground">
                  These curated quick-pick ids are proven absent from the exact{" "}
                  {catalog?.minecraftVersion} catalog, so selection and placement are disabled. This
                  is not a complete cross-version catalog diff, and IDs are not remapped; a renamed
                  block can appear here under its old quick-pick id.
                </p>
                <div
                  role="list"
                  aria-label="Quick picks not in this version"
                  className="scrollbar-thin flex max-h-48 flex-col overflow-y-auto"
                >
                  {unavailableQuickPicks.map((entry) => (
                    <div
                      key={entry.name}
                      role="listitem"
                      aria-disabled="true"
                      title={`Not in ${catalog?.minecraftVersion}: this quick pick is absent from the exact catalog.`}
                      className="flex items-center gap-2 px-1 py-1 opacity-70"
                    >
                      <CatalogBlockIcon
                        name={entry.name}
                        textureKey={null}
                        icons={iconIndex}
                        size={20}
                      />
                      <div className="min-w-0 flex-1">
                        <div className="truncate font-mono text-xs text-muted-foreground">
                          {shortBlockId(entry.name)}
                        </div>
                        <div className="text-[10px] text-muted-foreground">
                          Absent from {catalog?.minecraftVersion}; selection disabled.
                        </div>
                      </div>
                      <Badge variant="outline">Not in this version</Badge>
                    </div>
                  ))}
                </div>
              </div>
            </CollapsibleContent>
          </Collapsible>
        ) : null}
      </div>

      <Card className="scrollbar-thin flex min-h-0 w-80 shrink-0 flex-col gap-0 overflow-y-auto py-0">
        <CardContent className="flex flex-col gap-4 p-4">
          <div className="flex items-center gap-3">
            <span title="Isometric preview of the catalog icon texture, not a full block-model render.">
              <CatalogBlockIcon
                name={selectedBlock.name}
                textureKey={definition?.iconTexture ?? null}
                icons={iconIndex}
                size={40}
                mode="cube"
              />
            </span>
            <div className="min-w-0">
              <div className="truncate font-mono text-sm">{selectedBlock.name}</div>
              <div className="text-xs text-muted-foreground">
                {definition
                  ? source === "clientJar"
                    ? definition.multipart
                      ? "Client JAR multipart state"
                      : "Client JAR state"
                    : "Curated quick pick"
                  : catalog
                    ? "Not in this version"
                    : "Unknown state"}{" "}
                · brush/paste target
              </div>
            </div>
          </div>

          {!definition ? (
            <Alert>
              <AlertTriangle />
              <AlertTitle>{catalog ? "Not in this version" : "Unknown state"}</AlertTitle>
              <AlertDescription>
                {catalog
                  ? `This state is absent from the exact ${catalog.minecraftVersion} catalog. Existing blocks keep it and it can be used as a replace source, but the write registry refuses new placement.`
                  : "No exact client JAR catalog is loaded, so this state cannot be matched to a version. Its properties are preserved as stored and the write registry still refuses new placement."}
              </AlertDescription>
            </Alert>
          ) : null}

          {definition ? (
            <div className="flex flex-wrap items-center gap-1">
              {definition.groups.map((group) => (
                <Badge key={group} variant="outline">
                  {paletteCategoryLabel(group)}
                </Badge>
              ))}
              {definition.flags?.technical && !definition.groups.includes("technical") ? (
                <Badge variant="outline">Technical</Badge>
              ) : null}
              {definition.flags?.creativeOnly ? (
                <Badge variant="secondary">Creative only</Badge>
              ) : null}
              {definition.flags?.fluid ? <Badge variant="outline">Fluid</Badge> : null}
              {definition.flags?.airlike ? <Badge variant="outline">Air-like</Badge> : null}
              {definition.flags?.blockEntity ? <Badge variant="outline">Block entity</Badge> : null}
              {selectedFavorite ? <Badge variant="secondary">Favorite</Badge> : null}
            </div>
          ) : null}

          <p className="text-xs leading-relaxed">
            <span className="text-muted-foreground">Selected properties: </span>
            <span className="font-mono">{selectedStateSummary || "game default"}</span>
          </p>
          {definition && declaredDefaultSummary ? (
            <p className="text-[11px] leading-relaxed text-muted-foreground">
              Declared default: <span className="font-mono">{declaredDefaultSummary}</span>
            </p>
          ) : null}

          {definition ? (
            <CatalogStatePicker
              schema={{
                properties: definition.propertyValues,
                propertyValuesComplete: definition.propertyValuesComplete,
                multipart: definition.multipart,
              }}
              values={overrides}
              onChange={(properties) => setSelectedBlock({ ...selectedBlock, properties })}
              exact={source === "clientJar"}
            />
          ) : (
            <p className="text-[11px] leading-relaxed text-muted-foreground">
              {catalog
                ? "The state cannot be edited against a blockstate schema because its block is absent from the exact catalog. Its properties are preserved as stored."
                : "No exact catalog is loaded, so the state cannot be edited against a blockstate schema. Its properties are preserved as stored."}
            </p>
          )}

          <div className="flex flex-wrap gap-2">
            <Button
              size="sm"
              variant="outline"
              disabled={!definition}
              title={
                definition
                  ? "Use the game default state; no property overrides are sent."
                  : catalog
                    ? "Unavailable: this state is not declared by the exact catalog."
                    : "Unavailable: no exact catalog is loaded."
              }
              onClick={() => {
                if (definition) {
                  setSelectedBlock({
                    name: definition.name,
                    properties: { ...definition.properties },
                  });
                }
              }}
            >
              <RotateCcw data-icon="inline-start" />
              Game default
            </Button>
            <Button size="sm" variant="outline" onClick={() => setReplaceFrom(selectedBlock)}>
              Use as replace source
            </Button>
          </div>

          <div className="rounded-xl border bg-background p-3 text-xs leading-relaxed text-muted-foreground">
            Replace source:{" "}
            <span className="font-mono text-foreground/80">
              {replaceFrom?.name.replace("minecraft:", "") ?? "not set"}
            </span>
            {replaceFrom ? (
              <Button
                variant="ghost"
                size="icon-xs"
                className="ml-1"
                aria-label="Clear replace source"
                onClick={() => setReplaceFrom(null)}
              >
                <X />
              </Button>
            ) : null}
            <div className="mt-1.5">
              {source === "clientJar"
                ? "Block names and property keys come from the project's pinned client JAR catalog. Icons load only for the visible rows, and the preview is an isometric view of the catalog icon texture, not a full block-model render."
                : "Curated quick picks are shown because this project has no client JAR catalog. Their properties are not an exact blockstate schema."}
            </div>
            {visibleAtlasError ? (
              <div className="mt-1.5">Visible icons could not be loaded: {visibleAtlasError}</div>
            ) : null}
          </div>
        </CardContent>
      </Card>
    </div>
  );
}
