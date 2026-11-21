import { useRef, useState } from "react";
import type { ChangeEvent, ReactNode } from "react";
import {
  AlertTriangle,
  Boxes,
  CheckCircle2,
  Download,
  HardDrive,
  Loader2,
  Package,
  ShieldAlert,
  Upload,
} from "lucide-react";
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import {
  Empty,
  EmptyDescription,
  EmptyHeader,
  EmptyMedia,
  EmptyTitle,
} from "@/components/ui/empty";
import { Input } from "@/components/ui/input";
import { Separator } from "@/components/ui/separator";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { isTauriRuntime, normalizeError } from "@/lib/ipc";
import {
  exportStructure,
  previewStructureExport,
  previewStructureImport,
  readStructureFile,
} from "@/lib/structure-api";
import { currentEditorState, projectIsDirty, useProjectStore } from "@/state/project-store";
import { pushToast } from "@/state/toast-store";
import { useToolStore } from "@/state/tool-store";
import { useWorldStore } from "@/state/world-store";
import {
  STRUCTURE_DIMENSIONS,
  shortBlockState,
  type StructureExportPreview,
  type StructureExportResult,
  type StructureImportPreview,
} from "@/project/structure-types";
import { formatBytes } from "@/project/types";
import { cn } from "@/lib/utils";

type ExchangeMode = "import" | "export";
type ImportDestination = "new" | "insert";

function defaultOriginY(height: number, dimension: string): number {
  const minY = dimension === "minecraft:overworld" ? -64 : 0;
  const maxY =
    dimension === "minecraft:the_nether" ? 127 : dimension === "minecraft:the_end" ? 255 : 319;
  return Math.max(minY, Math.min(64, maxY - height + 1));
}

function Meta({ label, value }: { label: string; value: ReactNode }) {
  return (
    <div className="flex min-w-0 items-baseline gap-2">
      <span className="shrink-0 text-muted-foreground">{label}</span>
      <span className="truncate font-mono text-xs text-foreground">{value}</span>
    </div>
  );
}

function Palette({ preview }: { preview: StructureImportPreview | StructureExportPreview }) {
  return (
    <div className="space-y-1">
      <div className="flex items-center gap-2 text-sm text-muted-foreground">
        <Boxes className="size-4" /> Palette ({preview.paletteSize})
      </div>
      <div className="flex max-h-16 flex-wrap gap-1 overflow-y-auto">
        {preview.palette.map((entry) => (
          <span
            key={entry.state}
            title={entry.state}
            className="rounded-md border bg-muted/40 px-1.5 py-0.5 font-mono text-xs text-muted-foreground"
          >
            {shortBlockState(entry.state)}{" "}
            <span className="text-foreground">×{entry.count.toLocaleString()}</span>
          </span>
        ))}
        {preview.paletteSize > preview.palette.length ? (
          <span className="px-1 py-0.5 text-xs text-muted-foreground">
            +{preview.paletteSize - preview.palette.length} more
          </span>
        ) : null}
      </div>
    </div>
  );
}

function Warnings({ items }: { items: string[] }) {
  if (items.length === 0) return null;
  return (
    <div className="space-y-1.5 rounded-xl border border-status-warn/30 bg-status-warn/5 p-4 text-sm leading-relaxed text-status-warn">
      {items.map((warning) => (
        <div key={warning} className="flex gap-2">
          <AlertTriangle className="mt-0.5 size-3.5 shrink-0" />
          <span>{warning}</span>
        </div>
      ))}
    </div>
  );
}

export function StructureExchange() {
  const desktop = isTauriRuntime();
  const current = useProjectStore((state) => state.current);
  const loading = useProjectStore((state) => state.loading);
  const revision = useWorldStore((state) => state.revision);
  const worldDimension = useWorldStore((state) => state.dimension);
  const selection = useToolStore((state) => state.selection);
  const dirty = projectIsDirty(current, revision);
  const projectOpen = Boolean(current?.folder);

  const [mode, setMode] = useState<ExchangeMode>("import");
  const [busy, setBusy] = useState(false);
  const fileInput = useRef<HTMLInputElement>(null);

  // Import state.
  const [file, setFile] = useState<{ name: string; size: number; payload: string } | null>(null);
  const [preview, setPreview] = useState<StructureImportPreview | null>(null);
  const [importError, setImportError] = useState<string | null>(null);
  const [destination, setDestination] = useState<ImportDestination>("new");
  const [name, setName] = useState("");
  const [importDimension, setImportDimension] = useState(worldDimension);
  const [origin, setOrigin] = useState({ x: 0, y: 64, z: 0 });
  const [includeAir, setIncludeAir] = useState(false);

  // Export state.
  const [exportDimension, setExportDimension] = useState(worldDimension);
  const [useSelection, setUseSelection] = useState(true);
  const [exportPreview, setExportPreview] = useState<StructureExportPreview | null>(null);
  const [exportResult, setExportResult] = useState<StructureExportResult | null>(null);
  const [exportError, setExportError] = useState<string | null>(null);

  const disabled = !desktop || busy || loading;
  // A box selection has no dimension of its own. Reusing it after switching the export
  // dimension would silently export a different area, so only offer it in its own dimension.
  const selectionMatchesDimension = exportDimension === worldDimension;
  const exportBounds = useSelection && selection && selectionMatchesDimension ? selection : null;

  const handleFile = async (event: ChangeEvent<HTMLInputElement>): Promise<void> => {
    const selected = event.target.files?.[0];
    event.target.value = "";
    if (!selected) return;
    setBusy(true);
    setImportError(null);
    setPreview(null);
    setExportPreview(null);
    setExportResult(null);
    try {
      const payload = await readStructureFile(selected);
      const result = await previewStructureImport(payload);
      setFile({ name: selected.name, size: selected.size, payload });
      setPreview(result);
      setDestination(projectOpen ? "insert" : "new");
      setName(selected.name.replace(/\.nbt$/i, "").slice(0, 64) || "Imported structure");
      setOrigin({ x: 0, y: defaultOriginY(result.size[1], importDimension), z: 0 });
    } catch (error) {
      const normalized = normalizeError(error);
      setImportError(normalized.message);
      pushToast({
        kind: "error",
        title: "Structure preview failed",
        description: normalized.message,
      });
    } finally {
      setBusy(false);
    }
  };

  const changeImportDimension = (dimension: string): void => {
    setImportDimension(dimension);
    if (preview) {
      setOrigin((value) => ({ ...value, y: defaultOriginY(preview.size[1], dimension) }));
    }
  };

  const submitImport = async (): Promise<void> => {
    if (!file || !preview || !preview.compatible) return;
    setBusy(true);
    setImportError(null);
    try {
      const base = {
        payload: file.payload,
        dimension: importDimension,
        origin: [origin.x, origin.y, origin.z] as [number, number, number],
        includeAir,
      };
      const ok =
        destination === "new"
          ? await useProjectStore.getState().importStructure({
              ...base,
              name: name.trim(),
              sourceFileName: file.name,
              editorState: currentEditorState(),
            })
          : await useProjectStore.getState().insertStructure(base);
      if (ok) {
        setFile(null);
        setPreview(null);
      }
    } finally {
      setBusy(false);
    }
  };

  const requestExportPreview = async (): Promise<void> => {
    setBusy(true);
    setExportError(null);
    setExportResult(null);
    try {
      const result = await previewStructureExport(exportDimension, exportBounds);
      setExportPreview(result);
    } catch (error) {
      const normalized = normalizeError(error);
      setExportError(normalized.message);
      pushToast({ kind: "error", title: "Export preview failed", description: normalized.message });
    } finally {
      setBusy(false);
    }
  };

  const executeExport = async (): Promise<void> => {
    if (!exportPreview) return;
    setBusy(true);
    setExportError(null);
    try {
      const result = await exportStructure({
        dimension: exportDimension,
        bounds: exportBounds,
        fileName: exportPreview.fileName,
        expectedChecksum: exportPreview.checksum,
      });
      setExportResult(result);
      pushToast({
        kind: "success",
        title: "Structure exported",
        description: `${result.outcome.relativePath} · ${formatBytes(result.outcome.bytes)} · verified`,
      });
    } catch (error) {
      const normalized = normalizeError(error);
      setExportError(normalized.message);
      pushToast({ kind: "error", title: "Export failed", description: normalized.message });
    } finally {
      setBusy(false);
    }
  };

  const newProjectBlocked = destination === "new" && dirty;

  return (
    <div className="space-y-4">
      <div className="flex flex-wrap items-center gap-2">
        <Button
          size="sm"
          variant={mode === "import" ? "default" : "outline"}
          onClick={() => setMode("import")}
        >
          <Upload className="mr-1 size-3" /> Import structure
        </Button>
        <Button
          size="sm"
          variant={mode === "export" ? "default" : "outline"}
          onClick={() => setMode("export")}
        >
          <Download className="mr-1 size-3" /> Export structure
        </Button>
        <span className="ml-auto font-mono text-xs text-muted-foreground">
          Java structure · Minecraft 1.21.4 · DataVersion 4189
        </span>
      </div>

      <Separator />

      {!desktop ? (
        <Alert className="p-4">
          <ShieldAlert />
          <AlertTitle>Desktop core required</AlertTitle>
          <AlertDescription>
            Structure import/export runs in the Tauri backend where files can be verified and
            published atomically. Browser preview mode cannot read or write project folders.
          </AlertDescription>
        </Alert>
      ) : mode === "import" ? (
        <div className="scrollbar-thin max-h-[52vh] space-y-3 overflow-y-auto pr-1">
          <div className="flex flex-wrap items-center gap-3 rounded-xl border bg-muted/30 p-4">
            <Button
              size="sm"
              variant="outline"
              disabled={disabled}
              onClick={() => fileInput.current?.click()}
            >
              <Upload className="mr-1 size-3" /> Choose .nbt file
            </Button>
            <input
              ref={fileInput}
              type="file"
              accept=".nbt,application/octet-stream"
              className="hidden"
              aria-label="Structure file"
              onChange={(event) => void handleFile(event)}
            />
            <div className="min-w-0 flex-1 text-sm text-muted-foreground">
              {file ? (
                <span className="truncate font-mono text-foreground">
                  {file.name} · {formatBytes(file.size)}
                </span>
              ) : (
                "The file is read locally and previewed before any world changes."
              )}
            </div>
            {busy ? <Loader2 className="size-3.5 animate-spin text-primary" /> : null}
          </div>

          {importError ? (
            <div className="flex items-start gap-2 rounded-lg border border-destructive/40 bg-destructive/5 p-3 text-sm text-destructive">
              <AlertTriangle className="mt-0.5 size-3 shrink-0" />
              <span>{importError}</span>
            </div>
          ) : null}

          {preview ? (
            <>
              <div
                className={cn(
                  "rounded border p-2",
                  preview.compatible
                    ? "border-status-ok/30 bg-status-ok/5"
                    : "border-status-error/40 bg-status-error/5",
                )}
              >
                <div className="flex items-center gap-1.5 text-xs font-medium">
                  {preview.compatible ? (
                    <>
                      <CheckCircle2 className="size-3.5 text-status-ok" /> Compatible structure
                    </>
                  ) : (
                    <>
                      <AlertTriangle className="size-3.5 text-status-error" /> Import blocked:
                      content cannot be preserved
                    </>
                  )}
                  <span className="ml-auto font-mono text-xs text-muted-foreground">
                    {preview.format} · DV {preview.dataVersion}
                  </span>
                </div>
                <div className="mt-1.5 grid grid-cols-2 gap-x-3 gap-y-0.5 text-xs md:grid-cols-3">
                  <Meta label="Size" value={`${preview.size.join(" × ")}`} />
                  <Meta label="Volume" value={preview.volume.toLocaleString()} />
                  <Meta label="Non-air" value={preview.nonAirBlocks.toLocaleString()} />
                  <Meta label="Stored" value={preview.storedBlocks.toLocaleString()} />
                  <Meta label="Entities" value={preview.entities} />
                  <Meta label="Block entities" value={preview.blockEntities} />
                  <Meta label="Target" value={preview.targetMinecraftVersion} />
                  <Meta label="Target DV" value={preview.targetDataVersion} />
                  <Meta label="Unknown tags" value={preview.unknownTags.length} />
                </div>
              </div>

              <Palette preview={preview} />
              <Warnings items={preview.warnings} />

              <div className="grid gap-4 rounded-xl border bg-muted/30 p-4 md:grid-cols-2">
                <div className="space-y-1">
                  <div className="text-xs font-medium text-muted-foreground">Destination</div>
                  <div className="flex gap-1">
                    <Button
                      size="sm"
                      variant={destination === "new" ? "default" : "outline"}
                      disabled={disabled}
                      onClick={() => setDestination("new")}
                    >
                      New project
                    </Button>
                    <Button
                      size="sm"
                      variant={destination === "insert" ? "default" : "outline"}
                      disabled={disabled || !projectOpen}
                      onClick={() => setDestination("insert")}
                    >
                      Insert into open project
                    </Button>
                  </div>
                  {destination === "new" ? (
                    <Input
                      value={name}
                      maxLength={64}
                      placeholder="Imported structure"
                      aria-label="Imported project name"
                      className="h-6 text-xs"
                      onChange={(event) => setName(event.target.value)}
                    />
                  ) : (
                    <p className="text-xs leading-relaxed text-muted-foreground">
                      Inserts into “{current?.name}”. The project stays unsaved until you save.
                    </p>
                  )}
                </div>

                <div className="space-y-1">
                  <div className="text-xs font-medium text-muted-foreground">Placement</div>
                  <div className="flex items-center gap-1">
                    <Select
                      items={STRUCTURE_DIMENSIONS.map((dimension) => ({
                        value: dimension.id,
                        label: dimension.label,
                      }))}
                      value={importDimension}
                      onValueChange={(value) => {
                        if (value !== null) changeImportDimension(value);
                      }}
                    >
                      <SelectTrigger className="w-fit" aria-label="Import dimension">
                        <SelectValue />
                      </SelectTrigger>
                      <SelectContent>
                        {STRUCTURE_DIMENSIONS.map((dimension) => (
                          <SelectItem key={dimension.id} value={dimension.id}>
                            {dimension.label}
                          </SelectItem>
                        ))}
                      </SelectContent>
                    </Select>
                    {(["x", "y", "z"] as const).map((axis) => (
                      <label key={axis} className="flex items-center gap-0.5 font-mono text-xs">
                        <span className="text-muted-foreground">{axis}</span>
                        <Input
                          type="number"
                          aria-label={`Origin ${axis}`}
                          value={origin[axis]}
                          className="w-14"
                          onChange={(event) =>
                            setOrigin((value) => ({
                              ...value,
                              [axis]: Math.trunc(Number(event.target.value) || 0),
                            }))
                          }
                        />
                      </label>
                    ))}
                  </div>
                  <label className="flex items-center gap-1.5 text-xs text-muted-foreground">
                    <Checkbox
                      checked={includeAir}
                      onCheckedChange={(checked) => setIncludeAir(checked === true)}
                    />
                    Replace the whole volume (clear air positions). Off: overlay only non-air
                    blocks.
                  </label>
                </div>
              </div>

              {newProjectBlocked ? (
                <div className="rounded border border-status-warn/40 bg-status-warn/10 p-2 text-xs text-status-warn">
                  “{current?.name}” has unsaved changes. Save or close it before creating a new
                  imported project.
                </div>
              ) : null}

              <div className="flex items-center gap-2">
                <Button
                  size="sm"

                  disabled={
                    disabled ||
                    !preview.compatible ||
                    (destination === "new" && (name.trim().length === 0 || newProjectBlocked))
                  }
                  onClick={() => void submitImport()}
                >
                  {busy ? (
                    <Loader2 className="mr-1 size-3 animate-spin" />
                  ) : destination === "new" ? (
                    <Package className="mr-1 size-3" />
                  ) : (
                    <Boxes className="mr-1 size-3" />
                  )}
                  {destination === "new" ? "Create project from structure" : "Insert structure"}
                </Button>
                <span className="text-xs text-muted-foreground">
                  The original file is copied into the project&apos;s immutable source layer.
                </span>
              </div>
            </>
          ) : (
            <Empty className="border-0 py-8">
              <EmptyHeader>
                <EmptyMedia variant="icon">
                  <Package />
                </EmptyMedia>
                <EmptyTitle>No structure selected</EmptyTitle>
                <EmptyDescription>
                  Choose a Java structure file to inspect its size, palette and compatibility before
                  importing.
                </EmptyDescription>
              </EmptyHeader>
            </Empty>
          )}
        </div>
      ) : (
        <div className="scrollbar-thin max-h-[52vh] space-y-3 overflow-y-auto pr-1">
          {!projectOpen ? (
            <Empty className="border-0 py-8">
              <EmptyHeader>
                <EmptyMedia variant="icon">
                  <HardDrive />
                </EmptyMedia>
                <EmptyTitle>No project open</EmptyTitle>
                <EmptyDescription>
                  Open a project to export its world as a Java structure.
                </EmptyDescription>
              </EmptyHeader>
            </Empty>
          ) : (
            <>
              <div className="flex flex-wrap items-center gap-3 rounded-xl border bg-muted/30 p-4">
                <label className="flex items-center gap-1 text-xs text-muted-foreground">
                  Dimension
                  <Select
                    items={STRUCTURE_DIMENSIONS.map((dimension) => ({
                      value: dimension.id,
                      label: dimension.label,
                    }))}
                    value={exportDimension}
                    onValueChange={(value) => {
                      if (value === null) return;
                      setExportDimension(value);
                      setExportPreview(null);
                      setExportResult(null);
                    }}
                  >
                    <SelectTrigger className="w-fit" aria-label="Export dimension">
                      <SelectValue />
                    </SelectTrigger>
                    <SelectContent>
                      {STRUCTURE_DIMENSIONS.map((dimension) => (
                        <SelectItem key={dimension.id} value={dimension.id}>
                          {dimension.label}
                        </SelectItem>
                      ))}
                    </SelectContent>
                  </Select>
                </label>
                <label className="flex items-center gap-1 text-xs text-muted-foreground">
                  <Checkbox
                    checked={useSelection && selectionMatchesDimension}
                    disabled={!selection || !selectionMatchesDimension}
                    onCheckedChange={(checked) => {
                      setUseSelection(checked === true);
                      setExportPreview(null);
                      setExportResult(null);
                    }}
                  />
                  Use box selection
                  {!selection
                    ? " (none)"
                    : selectionMatchesDimension
                      ? ""
                      : ` (made in ${worldDimension.split(":").pop()})`}
                </label>
                <span className="max-w-64 truncate font-mono text-xs text-muted-foreground">
                  {exportBounds
                    ? `${exportBounds.min.x},${exportBounds.min.y},${exportBounds.min.z} → ${exportBounds.max.x},${exportBounds.max.y},${exportBounds.max.z}`
                    : "content bounds (all non-air blocks)"}
                </span>
                <Button
                  size="sm"
                  variant="outline"
                  className="ml-auto"
                  disabled={disabled}
                  onClick={() => void requestExportPreview()}
                >
                  {busy ? (
                    <Loader2 className="mr-1 size-3 animate-spin" />
                  ) : (
                    <Download className="mr-1 size-3" />
                  )}
                  Preview export
                </Button>
              </div>

              {exportError ? (
                <div className="flex items-start gap-2 rounded-lg border border-destructive/40 bg-destructive/5 p-3 text-sm text-destructive">
                  <AlertTriangle className="mt-0.5 size-3 shrink-0" />
                  <span>{exportError}</span>
                </div>
              ) : null}

              {exportPreview ? (
                <>
                  <div className="rounded-xl border border-status-ok/30 bg-status-ok/5 p-4 text-sm">
                    <div className="flex items-center gap-1.5 text-xs font-medium">
                      <CheckCircle2 className="size-3.5 text-status-ok" /> Export ready
                      <span className="ml-auto font-mono text-xs text-muted-foreground">
                        {exportPreview.format} · DV {exportPreview.dataVersion} →{" "}
                        {exportPreview.targetDataVersion}
                      </span>
                    </div>
                    <div className="mt-1.5 grid grid-cols-2 gap-x-3 gap-y-0.5 text-xs md:grid-cols-3">
                      <Meta label="Size" value={`${exportPreview.size.join(" × ")}`} />
                      <Meta label="Volume" value={exportPreview.volume.toLocaleString()} />
                      <Meta label="Non-air" value={exportPreview.nonAirBlocks.toLocaleString()} />
                      <Meta label="Stored" value={exportPreview.storedBlocks.toLocaleString()} />
                      <Meta label="Bytes" value={formatBytes(exportPreview.bytes)} />
                      <Meta label="Target" value={exportPreview.targetMinecraftVersion} />
                    </div>
                    <div className="mt-1 grid gap-0.5 font-mono text-xs text-muted-foreground">
                      <span className="truncate">path: {exportPreview.relativePath}</span>
                      <span className="truncate">checksum: {exportPreview.checksum}</span>
                    </div>
                  </div>
                  <Palette preview={exportPreview} />
                  <Warnings items={exportPreview.warnings} />

                  <div className="flex items-center gap-2">
                    <Button
                      size="sm"

                      disabled={disabled}
                      onClick={() => void executeExport()}
                    >
                      {busy ? (
                        <Loader2 className="mr-1 size-3 animate-spin" />
                      ) : (
                        <Download className="mr-1 size-3" />
                      )}
                      Export .nbt
                    </Button>
                    <span className="text-xs text-muted-foreground">
                      Written under the project&apos;s exports folder, then re-read and compared
                      before success is reported.
                    </span>
                  </div>
                </>
              ) : (
                <Empty className="border-0 py-8">
                  <EmptyHeader>
                    <EmptyMedia variant="icon">
                      <Download />
                    </EmptyMedia>
                    <EmptyTitle>Nothing previewed yet</EmptyTitle>
                    <EmptyDescription>
                      Preview the export to see size, block counts, target DataVersion, warnings and
                      the destination path before writing a file.
                    </EmptyDescription>
                  </EmptyHeader>
                </Empty>
              )}

              {exportResult ? (
                <div className="space-y-2 rounded-xl border border-status-ok/40 bg-status-ok/5 p-4 text-sm">
                  <div className="flex items-center gap-1.5 font-medium text-status-ok">
                    <CheckCircle2 className="size-3.5" /> Verified export
                  </div>
                  <div className="grid gap-0.5 font-mono text-xs text-muted-foreground">
                    <span className="truncate">file: {exportResult.outcome.relativePath}</span>
                    <span>
                      bytes: {formatBytes(exportResult.outcome.bytes)} · non-air:{" "}
                      {exportResult.nonAirBlocks.toLocaleString()} · DV {exportResult.dataVersion}
                    </span>
                    <span className="truncate">checksum: {exportResult.outcome.checksum}</span>
                  </div>
                </div>
              ) : null}
            </>
          )}
        </div>
      )}

      <Separator />
      <div className="flex items-center gap-2 text-sm text-muted-foreground">
        <HardDrive className="size-3" />
        <span>
          Block states and properties are preserved verbatim. Entities, block entities and
          FuryBuilder-only metadata (objects, DSL, history) are not part of this format and are
          refused on import rather than silently dropped.
        </span>
        {busy ? <Loader2 className="ml-auto size-3 animate-spin text-primary" /> : null}
      </div>
    </div>
  );
}
