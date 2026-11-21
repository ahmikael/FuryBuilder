import { useEffect, useMemo, useState } from "react";
import { Loader2, MapPin, Route, ScanLine, Trees, Waves } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { NativeSelect, NativeSelectOption } from "@/components/ui/native-select";
import { applyEditorOperation } from "@/editor/commands";
import {
  boundsVolume,
  MAX_EDIT_VOLUME,
  planRiver,
  planRoad,
  planStructureSiteQuery,
  planVegetation,
  type RouteAxis,
} from "@/editor/session";
import type { BlockDefinition, StructureSiteSearch } from "@/editor/types";
import { isTauriRuntime, normalizeError } from "@/lib/ipc";
import { fetchBlockCatalog, findStructureSites, previewOperation } from "@/lib/world-api";
import type { BlockState } from "@/renderer/types";
import { useToolStore } from "@/state/tool-store";
import { useWorldStore } from "@/state/world-store";

type GeneratorMode = "road" | "river" | "vegetation" | "sites";

const MODES: { value: GeneratorMode; label: string; icon: typeof Route }[] = [
  { value: "road", label: "Road", icon: Route },
  { value: "river", label: "River", icon: Waves },
  { value: "vegetation", label: "Vegetation", icon: Trees },
  { value: "sites", label: "Sites", icon: MapPin },
];

function material(catalog: BlockDefinition[] | null, name: string): BlockState {
  const definition = catalog?.find((entry) => entry.name === name) ?? null;
  return { name, properties: definition ? { ...definition.properties } : {} };
}

/**
 * M9.12 local construction generators: roads, river channels, vegetation scatter and a
 * bounded structure-site rule query. Every edit is one reversible operation produced by
 * the Rust core; none of these tools claims vanilla world generation parity.
 */
export function GeneratorsPanel() {
  const selection = useToolStore((state) => state.selection);
  const setSelection = useToolStore((state) => state.setSelection);
  const job = useToolStore((state) => state.job);
  const dimension = useWorldStore((state) => state.dimension);
  const [mode, setMode] = useState<GeneratorMode>("road");
  const [axis, setAxis] = useState<RouteAxis>("x");
  const [seed, setSeed] = useState(0);
  const [width, setWidth] = useState(3);
  const [meander, setMeander] = useState(4);
  const [clearance, setClearance] = useState(2);
  const [depth, setDepth] = useState(3);
  const [groundCover, setGroundCover] = useState(35);
  const [treeChance, setTreeChance] = useState(2);
  const [treeSpacing, setTreeSpacing] = useState(5);
  const [surfaceName, setSurfaceName] = useState("minecraft:stone_bricks");
  const [waterName, setWaterName] = useState("minecraft:water");
  const [bedName, setBedName] = useState("minecraft:sand");
  const [plantName, setPlantName] = useState("minecraft:short_grass");
  const [trunkName, setTrunkName] = useState("minecraft:oak_log");
  const [leavesName, setLeavesName] = useState("minecraft:oak_leaves");
  const [siteWidth, setSiteWidth] = useState(5);
  const [siteDepth, setSiteDepth] = useState(5);
  const [siteStep, setSiteStep] = useState(5);
  const [siteSlope, setSiteSlope] = useState(2);
  const [siteClearance, setSiteClearance] = useState(2);
  const [siteLimit, setSiteLimit] = useState(8);
  const [catalog, setCatalog] = useState<BlockDefinition[] | null>(null);
  const [catalogError, setCatalogError] = useState<string | null>(null);
  const [sites, setSites] = useState<StructureSiteSearch | null>(null);
  const [sitesError, setSitesError] = useState<string | null>(null);
  const [searching, setSearching] = useState(false);
  const [preview, setPreview] = useState<{
    changedBlocks: number;
    affectedBlocks: number;
    warnings: string[];
  } | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [previewing, setPreviewing] = useState(false);
  const editing = isTauriRuntime();
  const busy = Boolean(job) || previewing || searching;

  useEffect(() => {
    if (!editing) return;
    let cancelled = false;
    fetchBlockCatalog()
      .then((entries) => {
        if (!cancelled) {
          setCatalog(entries);
          setCatalogError(null);
        }
      })
      .catch((cause: unknown) => {
        if (!cancelled) setCatalogError(normalizeError(cause).message);
      });
    return () => {
      cancelled = true;
    };
  }, [editing]);

  const routeOperation = useMemo(() => {
    if (mode === "road") {
      return planRoad({
        dimension,
        selection,
        axis,
        seed,
        width,
        meander,
        clearance,
        surface: material(catalog, surfaceName),
      });
    }
    if (mode === "river") {
      return planRiver({
        dimension,
        selection,
        axis,
        seed,
        width,
        meander,
        depth,
        water: material(catalog, waterName),
        bed: material(catalog, bedName),
      });
    }
    if (mode === "vegetation") {
      return planVegetation({
        dimension,
        selection,
        seed,
        groundCover,
        treeChance,
        treeSpacing,
        plant: material(catalog, plantName),
        trunk: material(catalog, trunkName),
        leaves: material(catalog, leavesName),
      });
    }
    return null;
  }, [
    axis,
    bedName,
    catalog,
    clearance,
    depth,
    dimension,
    groundCover,
    leavesName,
    meander,
    mode,
    plantName,
    seed,
    selection,
    surfaceName,
    treeChance,
    treeSpacing,
    trunkName,
    waterName,
    width,
  ]);

  const siteQuery = useMemo(
    () =>
      planStructureSiteQuery({
        dimension,
        selection,
        width: siteWidth,
        depth: siteDepth,
        step: siteStep,
        maxSlope: siteSlope,
        clearance: siteClearance,
        limit: siteLimit,
      }),
    [dimension, selection, siteClearance, siteDepth, siteLimit, siteSlope, siteStep, siteWidth],
  );

  const volume = routeOperation ? boundsVolume(routeOperation.bounds) : 0;
  const tooLarge = volume > MAX_EDIT_VOLUME;

  function resetPreview(): void {
    setPreview(null);
    setError(null);
  }

  async function previewGenerator(): Promise<void> {
    if (!routeOperation) return;
    setPreviewing(true);
    setError(null);
    try {
      const result = await previewOperation(routeOperation);
      setPreview({
        changedBlocks: result.changedBlocks,
        affectedBlocks: result.affectedBlocks,
        warnings: result.warnings,
      });
    } catch (cause) {
      setPreview(null);
      setError(normalizeError(cause).message);
    } finally {
      setPreviewing(false);
    }
  }

  async function applyGenerator(): Promise<void> {
    if (!routeOperation) return;
    setPreview(null);
    setError(null);
    await applyEditorOperation(routeOperation);
  }

  async function searchSites(): Promise<void> {
    if (!siteQuery) return;
    setSearching(true);
    setSitesError(null);
    try {
      setSites(await findStructureSites(siteQuery));
    } catch (cause) {
      setSites(null);
      setSitesError(normalizeError(cause).message);
    } finally {
      setSearching(false);
    }
  }

  function selectFirstSite(): void {
    const site = sites?.sites[0];
    if (!site || !siteQuery) return;
    setSelection({
      min: { x: site.origin.x, y: site.minSurfaceY + 1, z: site.origin.z },
      max: {
        x: site.origin.x + siteQuery.width - 1,
        y: site.origin.y + siteQuery.clearance,
        z: site.origin.z + siteQuery.depth - 1,
      },
    });
  }

  return (
    <div className="flex flex-wrap items-start gap-4 text-sm">
      <div className="flex w-80 flex-col gap-1.5">
        <div className="flex items-center gap-2">
          <Route className="size-3.5 text-primary" />
          <span className="font-medium">Local generators</span>
          <Badge variant="secondary" className="uppercase">
            deterministic
          </Badge>
        </div>
        <div className="flex flex-wrap gap-2">
          {MODES.map((entry) => {
            const Icon = entry.icon;
            return (
              <Button
                key={entry.value}
                size="sm"
                variant={mode === entry.value ? "default" : "outline"}
                aria-pressed={mode === entry.value}
                onClick={() => {
                  setMode(entry.value);
                  resetPreview();
                }}
              >
                <Icon className="mr-1 size-3" />
                {entry.label}
              </Button>
            );
          })}
        </div>

        {mode === "sites" ? (
          <div className="grid grid-cols-3 gap-1.5">
            <NumberField
              label="Site width"
              value={siteWidth}
              min={1}
              max={32}
              disabled={busy}
              onChange={(value) => {
                setSiteWidth(value);
                setSites(null);
              }}
            />
            <NumberField
              label="Site depth"
              value={siteDepth}
              min={1}
              max={32}
              disabled={busy}
              onChange={(value) => {
                setSiteDepth(value);
                setSites(null);
              }}
            />
            <NumberField
              label="Site step"
              value={siteStep}
              min={1}
              max={64}
              disabled={busy}
              onChange={(value) => {
                setSiteStep(value);
                setSites(null);
              }}
            />
            <NumberField
              label="Max slope"
              value={siteSlope}
              min={0}
              max={32}
              disabled={busy}
              onChange={(value) => {
                setSiteSlope(value);
                setSites(null);
              }}
            />
            <NumberField
              label="Site clearance"
              value={siteClearance}
              min={0}
              max={32}
              disabled={busy}
              onChange={(value) => {
                setSiteClearance(value);
                setSites(null);
              }}
            />
            <NumberField
              label="Site limit"
              value={siteLimit}
              min={1}
              max={64}
              disabled={busy}
              onChange={(value) => {
                setSiteLimit(value);
                setSites(null);
              }}
            />
          </div>
        ) : (
          <>
            <div className="grid grid-cols-3 gap-1.5">
              <label className="flex flex-col gap-0.5">
                <span className="text-muted-foreground">Axis</span>
                <NativeSelect
                  aria-label="Route axis"
                  className="w-fit"
                  value={axis}
                  disabled={busy}
                  onChange={(event) => {
                    setAxis(event.target.value as RouteAxis);
                    resetPreview();
                  }}
                >
                  <NativeSelectOption value="x">X</NativeSelectOption>
                  <NativeSelectOption value="z">Z</NativeSelectOption>
                </NativeSelect>
              </label>
              <NumberField
                label="Seed"
                value={seed}
                min={0}
                disabled={busy}
                onChange={(value) => {
                  setSeed(value);
                  resetPreview();
                }}
              />
              <NumberField
                label="Width"
                value={width}
                min={1}
                max={16}
                disabled={busy}
                onChange={(value) => {
                  setWidth(value);
                  resetPreview();
                }}
              />
              <NumberField
                label="Meander"
                value={meander}
                min={0}
                max={64}
                disabled={busy}
                onChange={(value) => {
                  setMeander(value);
                  resetPreview();
                }}
              />
              {mode === "road" ? (
                <NumberField
                  label="Clearance"
                  value={clearance}
                  min={0}
                  max={8}
                  disabled={busy}
                  onChange={(value) => {
                    setClearance(value);
                    resetPreview();
                  }}
                />
              ) : null}
              {mode === "river" ? (
                <NumberField
                  label="Depth"
                  value={depth}
                  min={1}
                  max={16}
                  disabled={busy}
                  onChange={(value) => {
                    setDepth(value);
                    resetPreview();
                  }}
                />
              ) : null}
              {mode === "vegetation" ? (
                <>
                  <NumberField
                    label="Ground cover"
                    value={groundCover}
                    min={0}
                    max={100}
                    disabled={busy}
                    onChange={(value) => {
                      setGroundCover(value);
                      resetPreview();
                    }}
                  />
                  <NumberField
                    label="Tree chance"
                    value={treeChance}
                    min={0}
                    max={100}
                    disabled={busy}
                    onChange={(value) => {
                      setTreeChance(value);
                      resetPreview();
                    }}
                  />
                  <NumberField
                    label="Tree spacing"
                    value={treeSpacing}
                    min={1}
                    max={32}
                    disabled={busy}
                    onChange={(value) => {
                      setTreeSpacing(value);
                      resetPreview();
                    }}
                  />
                </>
              ) : null}
            </div>
            <div className="text-sm leading-relaxed text-muted-foreground">
              Routes run between the selection edges on the chosen axis. They follow the topmost
              non-air surface, skip fluid columns, and never write outside the selection. Repeating
              the same seed and parameters is idempotent.
            </div>
          </>
        )}
      </div>

      {mode === "sites" ? (
        <div className="w-64 space-y-3 rounded-xl border bg-card p-4">
          <div className="flex items-center justify-between gap-2">
            <span className="text-muted-foreground">Placement rules</span>
            <span className="font-mono">
              {selection ? `${boundsVolume(selection).toLocaleString()} blocks` : "no selection"}
            </span>
          </div>
          <div className="text-sm leading-relaxed text-muted-foreground">
            Greedy, non-overlapping search for flat, dry, clear footprints. This is a deterministic
            heuristic, not a claim about vanilla structure placement.
          </div>
          <div className="flex flex-wrap gap-2 pt-0.5">
            <Button
              size="sm"
              disabled={!editing || !siteQuery || busy}
              aria-label="Find structure sites"
              onClick={() => void searchSites()}
            >
              {searching ? <Loader2 className="mr-1 size-3 animate-spin" /> : null}
              Find sites
            </Button>
            <Button
              size="sm"
              variant="outline"
              disabled={!sites?.sites.length}
              aria-label="Use first structure site"
              onClick={selectFirstSite}
            >
              Use first site
            </Button>
          </div>
          {!selection ? (
            <div className="text-xs text-status-warn">
              Use the Box select tool over the area to inspect.
            </div>
          ) : null}
          {sitesError ? (
            <div className="rounded-lg border border-status-error/30 bg-status-error/5 p-3 text-sm text-status-error">
              {sitesError}
            </div>
          ) : null}
          {sites ? (
            <div
              className="rounded-lg border border-status-ok/30 bg-status-ok/5 p-3 text-sm text-status-ok"
              data-testid="structure-sites-result"
            >
              {sites.sites.length} site(s) · scanned {sites.scannedCandidates.toLocaleString()} ·
              rejected {sites.rejectedCandidates.toLocaleString()}
              {sites.sites.slice(0, 4).map((site) => (
                <div
                  key={`${site.origin.x},${site.origin.y},${site.origin.z}`}
                  className="font-mono"
                >
                  origin {site.origin.x},{site.origin.y},{site.origin.z} · surface{" "}
                  {site.minSurfaceY}..{site.maxSurfaceY}
                </div>
              ))}
              {sites.sites.length === 0 ? (
                <div className="text-status-warn">
                  No flat, dry, clear footprint matched these rules.
                </div>
              ) : null}
            </div>
          ) : null}
        </div>
      ) : (
        <div className="w-44 space-y-3 rounded-xl border bg-card p-4">
          {mode === "road" ? (
            <MaterialSelect
              label="Road surface"
              value={surfaceName}
              catalog={catalog}
              disabled={busy || !editing}
              onChange={(value) => {
                setSurfaceName(value);
                resetPreview();
              }}
            />
          ) : null}
          {mode === "river" ? (
            <>
              <MaterialSelect
                label="River water"
                value={waterName}
                catalog={catalog}
                disabled={busy || !editing}
                onChange={(value) => {
                  setWaterName(value);
                  resetPreview();
                }}
              />
              <MaterialSelect
                label="River bed"
                value={bedName}
                catalog={catalog}
                disabled={busy || !editing}
                onChange={(value) => {
                  setBedName(value);
                  resetPreview();
                }}
              />
            </>
          ) : null}
          {mode === "vegetation" ? (
            <>
              <MaterialSelect
                label="Plant"
                value={plantName}
                catalog={catalog}
                disabled={busy || !editing}
                onChange={(value) => {
                  setPlantName(value);
                  resetPreview();
                }}
              />
              <MaterialSelect
                label="Tree trunk"
                value={trunkName}
                catalog={catalog}
                disabled={busy || !editing}
                onChange={(value) => {
                  setTrunkName(value);
                  resetPreview();
                }}
              />
              <MaterialSelect
                label="Tree leaves"
                value={leavesName}
                catalog={catalog}
                disabled={busy || !editing}
                onChange={(value) => {
                  setLeavesName(value);
                  resetPreview();
                }}
              />
            </>
          ) : null}
          {catalogError ? (
            <div className="text-xs text-status-warn">Catalog unavailable: {catalogError}</div>
          ) : null}
          <div className="text-sm leading-relaxed text-muted-foreground">
            Trees need a free trunk and canopy volume; plants only fill existing air.
          </div>
        </div>
      )}

      {mode === "sites" ? null : (
        <div className="w-64 space-y-3 rounded-xl border bg-card p-4">
          <div className="flex items-center justify-between gap-2">
            <span className="text-muted-foreground">Selection</span>
            <span className="font-mono">
              {selection ? `${boundsVolume(selection).toLocaleString()} blocks` : "not set"}
            </span>
          </div>
          {routeOperation ? (
            <div className="font-mono text-xs text-muted-foreground">
              bounds Y {routeOperation.bounds.min.y}..{routeOperation.bounds.max.y} ·{" "}
              {volume.toLocaleString()} blocks
            </div>
          ) : (
            <div className="text-xs text-status-warn">
              Use the Box select tool, then choose a generator.
            </div>
          )}
          {tooLarge ? (
            <div className="text-xs text-status-error">
              Footprint exceeds the {MAX_EDIT_VOLUME.toLocaleString()} block limit.
            </div>
          ) : null}
          <div className="flex flex-wrap gap-2 pt-0.5">
            <Button
              size="sm"
              variant="outline"
              disabled={!editing || !routeOperation || busy || tooLarge}
              aria-label="Preview generator"
              onClick={() => void previewGenerator()}
            >
              {previewing ? (
                <Loader2 className="mr-1 size-3 animate-spin" />
              ) : (
                <ScanLine className="mr-1 size-3" />
              )}
              Preview
            </Button>
            <Button
              size="sm"
              disabled={!editing || !routeOperation || busy || tooLarge}
              aria-label="Apply generator"
              onClick={() => void applyGenerator()}
            >
              {job ? <Loader2 className="mr-1 size-3 animate-spin" /> : null}
              Generate
            </Button>
          </div>
          {preview ? (
            <div
              className="rounded-lg border border-status-ok/30 bg-status-ok/5 p-3 text-sm text-status-ok"
              data-testid="generator-preview"
            >
              {preview.changedBlocks.toLocaleString()} block(s) would change out of{" "}
              {preview.affectedBlocks.toLocaleString()} selected.
              {preview.warnings.map((warning) => (
                <div key={warning} className="text-status-warn">
                  {warning}
                </div>
              ))}
            </div>
          ) : null}
          {error ? (
            <div className="rounded-lg border border-status-error/30 bg-status-error/5 p-3 text-sm text-status-error">
              {error}
            </div>
          ) : null}
          <div className="text-xs text-muted-foreground">
            Local construction tools; no vanilla worldgen parity claim.
          </div>
        </div>
      )}
    </div>
  );
}

function NumberField({
  label,
  value,
  min,
  max,
  disabled,
  onChange,
}: {
  label: string;
  value: number;
  min: number;
  max?: number;
  disabled: boolean;
  onChange: (value: number) => void;
}) {
  return (
    <label className="flex flex-col gap-0.5">
      <span className="text-muted-foreground">{label}</span>
      <Input
        aria-label={label}
        type="number"
        min={min}
        max={max}
        value={value}
        disabled={disabled}
        onChange={(event) => onChange(Number(event.target.value))}
      />
    </label>
  );
}

function MaterialSelect({
  label,
  value,
  catalog,
  disabled,
  onChange,
}: {
  label: string;
  value: string;
  catalog: BlockDefinition[] | null;
  disabled: boolean;
  onChange: (value: string) => void;
}) {
  return (
    <label className="flex flex-col gap-1">
      <span className="text-muted-foreground">{label}</span>
      <NativeSelect
        aria-label={label}
        className="w-fit"
        value={value}
        disabled={disabled}
        onChange={(event) => onChange(event.target.value)}
      >
        {catalog?.map((entry) => (
          <NativeSelectOption key={entry.name} value={entry.name}>
            {entry.name}
          </NativeSelectOption>
        )) ?? <NativeSelectOption value={value}>{value}</NativeSelectOption>}
      </NativeSelect>
    </label>
  );
}
