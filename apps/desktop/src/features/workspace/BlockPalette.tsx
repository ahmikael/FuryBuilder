import { useEffect, useMemo, useState } from "react";
import { AlertTriangle, Loader2, RotateCcw, Search, Star, X } from "lucide-react";
import type { CatalogFlags, CatalogSnapshot } from "@/assets/types";
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent } from "@/components/ui/card";
import {
  Empty,
  EmptyDescription,
  EmptyHeader,
  EmptyMedia,
  EmptyTitle,
} from "@/components/ui/empty";
import { Input } from "@/components/ui/input";
import { InputGroup, InputGroupAddon, InputGroupInput } from "@/components/ui/input-group";
import {
  Select,
  SelectContent,
  SelectGroup,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import type { BlockDefinition } from "@/editor/types";
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
import { loadProjectCatalog } from "@/lib/assets-api";
import { isTauriRuntime } from "@/lib/ipc";
import { fetchBlockCatalog } from "@/lib/world-api";
import { blockColor } from "@/renderer/assets";
import { usePaletteStore } from "@/state/palette-store";
import { useToolStore } from "@/state/tool-store";

interface PaletteEntry {
  name: string;
  displayName: string | null;
  properties: Record<string, string>;
  propertyValues: Record<string, string[]>;
  propertyValuesComplete: boolean;
  multipart: boolean;
  groups: PaletteGroupId[];
  tags: string[];
  flags: CatalogFlags | null;
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

function shortBlockId(id: string): string {
  return id.startsWith("minecraft:") ? id.slice("minecraft:".length) : id;
}

async function loadPalette(): Promise<PaletteLoad> {
  let fallbackReason: string;
  try {
    const snapshot = await loadProjectCatalog();
    if (snapshot) {
      return { catalog: snapshot, curated: null, fallbackReason: null, error: null };
    }
    fallbackReason = PLACEHOLDER_REASON;
  } catch (reason: unknown) {
    fallbackReason = `The project catalog could not be rebuilt: ${
      reason instanceof Error ? reason.message : String(reason)
    }. Showing curated quick picks instead.`;
  }
  try {
    const curated = await fetchBlockCatalog();
    return { catalog: null, curated, fallbackReason, error: null };
  } catch (reason: unknown) {
    return {
      catalog: null,
      curated: null,
      fallbackReason,
      error: reason instanceof Error ? reason.message : String(reason),
    };
  }
}

export function BlockPalette() {
  const [result, setResult] = useState<PaletteLoad | null>(null);
  const [attempt, setAttempt] = useState(0);
  const { catalog, curated, fallbackReason, error } = result ?? {};
  const [search, setSearch] = useState("");
  const [category, setCategory] = useState<PaletteFilterId>("all");
  const [flagFilter, setFlagFilter] = useState<PaletteFlagFilterId>("any");
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

  const palette = useMemo<PaletteEntry[]>(() => {
    if (catalog) {
      return catalog.entries.flatMap((entry) => {
        const states = entry.states;
        if (!states) return [];
        return [
          {
            name: entry.id,
            displayName: entry.displayName,
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
    if (!search.trim()) {
      return entries.filter((entry) => matchesPaletteFlagFilter(entry.flags, flagFilter));
    }
    return entries
      .filter((entry) => matchesPaletteSearch(entry, search))
      .filter((entry) => matchesPaletteFlagFilter(entry.flags, flagFilter));
  }, [category, palette, recentEntries, favoriteEntries, search, flagFilter]);

  const definition = entryById.get(selectedBlock.name) ?? null;
  const overrides = selectedBlock.properties ?? {};
  const propertyKeys = definition
    ? Array.from(new Set([...Object.keys(definition.propertyValues), ...Object.keys(overrides)]))
    : Object.keys(overrides);
  const source = catalog ? "clientJar" : curated ? "curated" : null;
  const loading = result === null;
  const selectedFavorite = definition ? favoriteSet.has(definition.name) : false;

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
              onChange={(event) => setSearch(event.target.value)}
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
            onValueChange={(value) => setCategory(value ?? "all")}
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
            onValueChange={(value) => setFlagFilter((value as PaletteFlagFilterId | null) ?? "any")}
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
          role="list"
          aria-label="Block palette"
          className="scrollbar-thin flex min-h-0 flex-1 flex-col overflow-y-auto rounded-lg border"
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
            visible.map((entry) => {
              const active = entry.name === selectedBlock.name;
              const favorite = favoriteSet.has(entry.name);
              return (
                <div key={entry.name} role="listitem" className="flex items-center gap-1 px-1">
                  <Button
                    type="button"
                    variant={active ? "secondary" : "ghost"}
                    size="sm"
                    title={entry.displayName ? `${entry.displayName} · ${entry.name}` : entry.name}
                    aria-pressed={active}
                    onClick={() => selectEntry(entry)}
                    className="min-w-0 flex-1 justify-start text-left"
                  >
                    <span
                      className="size-4 shrink-0 rounded-sm border"
                      style={{ backgroundColor: blockColor(entry.name) }}
                      aria-hidden="true"
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
            })
          )}
        </div>
      </div>

      <Card className="scrollbar-thin flex min-h-0 w-80 shrink-0 flex-col gap-0 overflow-y-auto py-0">
        <CardContent className="space-y-4 p-4">
          <div className="flex items-center gap-3">
            <span
              className="size-9 rounded-lg border border-black/30"
              style={{ backgroundColor: blockColor(selectedBlock.name) }}
            />
            <div className="min-w-0">
              <div className="truncate font-mono text-sm">{selectedBlock.name}</div>
              <div className="text-xs text-muted-foreground">
                {definition
                  ? source === "clientJar"
                    ? definition.multipart
                      ? "Client JAR multipart state"
                      : "Client JAR state"
                    : "Curated quick pick"
                  : "Custom state"}{" "}
                · brush/paste target
              </div>
            </div>
          </div>

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

          <div className="space-y-2">
            {propertyKeys.length === 0 ? (
              <div className="text-xs text-muted-foreground">
                {definition?.multipart
                  ? "No single JAR default: multipart states are assembled by the game."
                  : "The blockstate file declares no properties; the game default is used."}
              </div>
            ) : (
              propertyKeys.map((key) => (
                <label key={key} className="grid grid-cols-[1fr_1.4fr] items-center gap-1.5">
                  <span className="truncate font-mono text-xs text-muted-foreground">{key}</span>
                  <Input
                    className="h-8 px-2 font-mono text-xs"
                    value={overrides[key] ?? ""}
                    placeholder={definition?.propertyValues[key]?.join(" | ") ?? "game default"}
                    aria-label={`Property ${key}`}
                    onChange={(event) => {
                      const next = { ...overrides };
                      if (event.target.value === "") {
                        delete next[key];
                      } else {
                        next[key] = event.target.value;
                      }
                      setSelectedBlock({ ...selectedBlock, properties: next });
                    }}
                  />
                </label>
              ))
            )}
            {source === "clientJar" &&
            definition &&
            !definition.propertyValuesComplete &&
            propertyKeys.length > 0 ? (
              <p className="text-[11px] leading-relaxed text-muted-foreground">
                This blockstate file declares only part of the state space. Declared keys are
                checked; undeclared values are accepted.
              </p>
            ) : null}
            {definition?.multipart ? (
              <p className="text-[11px] leading-relaxed text-muted-foreground">
                Multipart states are assembled by the game; an empty selection uses its default
                state.
              </p>
            ) : null}
          </div>

          <div className="flex flex-wrap gap-2">
            <Button
              size="sm"
              variant="outline"
              disabled={!definition}
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
              Defaults
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
                ? "Block names and property keys come from the project's pinned client JAR catalog. Basic groups use its direct tags and ids; recent and favorites stay local to this catalog."
                : "Curated quick picks are shown because this project has no client JAR catalog."}
            </div>
          </div>
        </CardContent>
      </Card>
    </div>
  );
}
