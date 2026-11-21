import { useState } from "react";
import {
  AlertTriangle,
  Boxes,
  Crosshair,
  Hammer,
  Loader2,
  Plus,
  RotateCw,
  Undo2,
} from "lucide-react";

import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import { Input } from "@/components/ui/input";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { isTauriRuntime } from "@/lib/ipc";
import {
  HOUSE_GENERATOR_VERSION,
  PARAMETRIC_ROTATIONS,
  defaultParametricSpec,
  parametricRotationLabel,
  type ParametricConflictPolicy,
  type ParametricInspection,
  type ParametricObjectSummary,
  type ParametricRotation,
  type ParametricSpec,
} from "@/project/parametric-types";
import { useParametricStore } from "@/state/parametric-store";
import { useToolStore } from "@/state/tool-store";
import { useWorldStore } from "@/state/world-store";

const MIN_SIZE = 3;
const MAX_SIZE = 32;
const MIN_HEIGHT = 2;

const CONFLICT_POLICIES: { value: ParametricConflictPolicy; label: string }[] = [
  { value: "fail", label: "Fail on manual edits" },
  { value: "override", label: "Override manual edits" },
];

function clamp(value: number, min: number, max: number): number {
  return Math.min(max, Math.max(min, value));
}

function specFromSummary(summary: ParametricObjectSummary): ParametricSpec {
  return {
    name: summary.name,
    generator: summary.generator,
    generatorVersion: summary.generatorVersion,
    dimension: summary.dimension,
    origin: { ...summary.origin },
    rotation: summary.rotation,
    params: { ...summary.params },
    layerId: summary.layerId ?? null,
  };
}

function boundsLabel(object: ParametricObjectSummary): string {
  const { min, max } = object.bounds;
  return `${object.dimension.replace("minecraft:", "")} · (${min.x},${min.y},${min.z})..(${max.x},${max.y},${max.z})`;
}

/**
 * Parametric house recipes are compiled by the Rust Core into one reversible voxel
 * operation. The recipe, generator version and captured baseline stay in project.json;
 * transform, bake and rollback are explicit actions, and manual edits are reported instead
 * of being silently overwritten.
 */
export function ParametricPanel() {
  const objects = useParametricStore((state) => state.objects);
  const create = useParametricStore((state) => state.create);
  const update = useParametricStore((state) => state.update);
  const bake = useParametricStore((state) => state.bake);
  const rollback = useParametricStore((state) => state.rollback);
  const inspect = useParametricStore((state) => state.inspect);
  const saving = useParametricStore((state) => state.saving);
  const loading = useParametricStore((state) => state.loading);
  const storeError = useParametricStore((state) => state.error);
  const selection = useToolStore((state) => state.selection);
  const dimension = useWorldStore((state) => state.dimension);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [inspection, setInspection] = useState<ParametricInspection | null>(null);
  const [inspecting, setInspecting] = useState(false);
  const [actionError, setActionError] = useState<string | null>(null);
  const [conflictPolicy, setConflictPolicy] = useState<ParametricConflictPolicy>("fail");
  const [forceRollback, setForceRollback] = useState(false);
  const [spec, setSpec] = useState<ParametricSpec>(defaultParametricSpec);
  const editing = isTauriRuntime();
  const selected = objects.find((object) => object.id === selectedId) ?? null;
  const sourceDimension = selected?.dimension ?? dimension;

  if (!editing) {
    return (
      <div className="flex h-full flex-col items-center justify-center gap-2 text-center text-sm text-muted-foreground">
        <Boxes className="size-5 text-muted-foreground/60" />
        <p className="max-w-md leading-relaxed">
          Parametric recipes are compiled and validated by the Rust Core. Open the desktop app to
          create, transform, bake or roll back generated structures.
        </p>
      </div>
    );
  }

  const selectObject = async (id: string) => {
    setSelectedId(id);
    setInspection(null);
    setInspecting(true);
    setActionError(null);
    setForceRollback(false);
    const summary = objects.find((object) => object.id === id);
    if (summary) setSpec(specFromSummary(summary));
    const result = await inspect(id);
    setInspecting(false);
    setInspection(result);
  };

  const updateParams = (patch: Partial<ParametricSpec["params"]>) =>
    setSpec((current) => ({ ...current, params: { ...current.params, ...patch } }));
  const updateOrigin = (axis: "x" | "y" | "z", value: number) =>
    setSpec((current) => ({ ...current, origin: { ...current.origin, [axis]: value } }));

  const useSelection = () => {
    setActionError(null);
    if (!selection) {
      setActionError("Select a box in the viewport first.");
      return;
    }
    const width = clamp(selection.max.x - selection.min.x + 1, MIN_SIZE, MAX_SIZE);
    const depth = clamp(selection.max.z - selection.min.z + 1, MIN_SIZE, MAX_SIZE);
    const height = clamp(selection.max.y - selection.min.y + 1 - 2, MIN_HEIGHT, MAX_SIZE);
    setSpec((current) => ({
      ...current,
      origin: { ...selection.min },
      params: { ...current.params, width, depth, height },
    }));
  };

  const createHouse = async () => {
    setActionError(null);
    const result = await create({ ...spec, dimension });
    if (!result) return;
    setSelectedId(result.object.id);
    setInspection(await inspect(result.object.id));
    setForceRollback(false);
  };

  const updateSelected = async () => {
    if (!selectedId || !selected) return;
    setActionError(null);
    const result = await update(
      selectedId,
      { ...spec, dimension: selected.dimension },
      conflictPolicy,
    );
    if (!result) return;
    setInspection(await inspect(selectedId));
    setForceRollback(false);
  };

  const bakeSelected = async () => {
    if (!selectedId) return;
    setActionError(null);
    if (await bake(selectedId)) {
      setSelectedId(null);
      setInspection(null);
    }
  };

  const rollbackSelected = async () => {
    if (!selectedId) return;
    setActionError(null);
    const hasConflicts = (inspection?.conflictCount ?? 0) > 0;
    if (hasConflicts && !forceRollback) {
      setActionError("Manual edits were detected. Enable forced rollback to discard them.");
      return;
    }
    if (await rollback(selectedId, forceRollback)) {
      setSelectedId(null);
      setInspection(null);
      setForceRollback(false);
    }
  };

  const useObjectBounds = () => {
    if (!inspection) return;
    if (inspection.object.dimension !== dimension) {
      setActionError(
        `This object is in ${inspection.object.dimension.replace("minecraft:", "")}; switch dimension first.`,
      );
      return;
    }
    setActionError(null);
    useToolStore.getState().setSelection(inspection.object.bounds);
  };

  const canCreate = spec.name.trim().length > 0 && !saving;
  const canUpdate = Boolean(selected) && spec.name.trim().length > 0 && !saving;

  return (
    <div className="flex min-h-0 flex-1 gap-4 text-sm" data-testid="parametric-panel">
      <div className="flex min-h-0 w-72 shrink-0 flex-col gap-3 rounded-xl border bg-card p-4">
        <div className="flex items-center gap-2">
          <Boxes className="size-4 text-muted-foreground" />
          <span className="text-base font-semibold">Parametric</span>
          <span className="text-xs text-muted-foreground">{objects.length} / 64</span>
          {loading || inspecting ? (
            <Loader2 className="ml-auto size-3 animate-spin text-muted-foreground" />
          ) : null}
        </div>
        <div className="scrollbar-thin min-h-0 flex-1 space-y-1 overflow-y-auto">
          {objects.length === 0 ? (
            <p className="p-1 text-sm text-muted-foreground">
              No parametric objects yet. A recipe stays editable until it is baked.
            </p>
          ) : (
            objects.map((object) => (
              <button
                key={object.id}
                type="button"
                onClick={() => void selectObject(object.id)}
                className={
                  object.id === selectedId
                    ? "w-full rounded-lg border border-primary/60 bg-primary/10 px-3 py-2.5 text-left"
                    : "w-full rounded-lg border bg-card px-3 py-2.5 text-left hover:bg-accent"
                }
              >
                <div className="flex items-center gap-1">
                  <Boxes className="size-3 shrink-0 text-muted-foreground" />
                  <span className="truncate text-sm">{object.name}</span>
                  <span className="ml-auto font-mono text-xs text-muted-foreground">
                    r{object.appliedRevision}
                  </span>
                </div>
                <div className="truncate font-mono text-xs text-muted-foreground">
                  {object.generator} v{object.generatorVersion} · {boundsLabel(object)}
                </div>
                <div className="flex justify-between font-mono text-xs text-muted-foreground">
                  <span>{parametricRotationLabel(object.rotation)}</span>
                  <span>{object.generatedBlocks.toLocaleString()} blocks</span>
                </div>
              </button>
            ))
          )}
        </div>
      </div>

      <div className="scrollbar-thin flex min-h-0 w-80 shrink-0 flex-col gap-3 overflow-y-auto rounded-xl border bg-card p-4">
        <div className="flex items-center gap-2">
          <span className="text-base font-semibold">
            {selected ? "Edit recipe" : "Create house"}
          </span>
          <span className="ml-auto font-mono text-xs text-muted-foreground">
            house v{HOUSE_GENERATOR_VERSION}
          </span>
        </div>
        <Input
          value={spec.name}
          onChange={(event) => setSpec((current) => ({ ...current, name: event.target.value }))}
          aria-label="Parametric object name"
        />
        <div className="grid grid-cols-3 gap-3">
          {(["x", "y", "z"] as const).map((axis) => (
            <label key={axis} className="flex flex-col gap-1.5 text-sm text-muted-foreground">
              <span>Origin {axis.toUpperCase()}</span>
              <Input
                type="number"
                value={spec.origin[axis]}
                aria-label={`Origin ${axis.toUpperCase()}`}
                onChange={(event) => updateOrigin(axis, Number(event.target.value))}
              />
            </label>
          ))}
        </div>
        <div className="grid grid-cols-3 gap-3">
          {(
            [
              ["width", "Width", MIN_SIZE],
              ["depth", "Depth", MIN_SIZE],
              ["height", "Height", MIN_HEIGHT],
            ] as const
          ).map(([key, label, min]) => (
            <label key={key} className="flex flex-col gap-1.5 text-sm text-muted-foreground">
              <span>{label}</span>
              <Input
                type="number"
                min={min}
                max={MAX_SIZE}
                value={spec.params[key]}
                aria-label={`House ${key}`}
                onChange={(event) => updateParams({ [key]: Number(event.target.value) })}
              />
            </label>
          ))}
        </div>
        <div className="grid grid-cols-2 gap-3">
          {(
            [
              ["wall", "Wall"],
              ["floor", "Floor"],
              ["roof", "Roof"],
              ["window", "Window"],
            ] as const
          ).map(([key, label]) => (
            <label key={key} className="flex flex-col gap-1.5 text-sm text-muted-foreground">
              <span>{label} block</span>
              <Input
                value={spec.params[key] ?? ""}
                aria-label={`${label} block`}
                onChange={(event) => updateParams({ [key]: event.target.value || null })}
              />
            </label>
          ))}
        </div>
        <div className="grid grid-cols-2 gap-3">
          <label className="flex flex-col gap-1.5 text-sm text-muted-foreground">
            <span>Rotation</span>
            <Select
              items={PARAMETRIC_ROTATIONS}
              value={spec.rotation}
              onValueChange={(value) =>
                setSpec((current) => ({
                  ...current,
                  rotation: value as ParametricRotation,
                }))
              }
            >
              <SelectTrigger className="w-fit" aria-label="Rotation">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                {PARAMETRIC_ROTATIONS.map((entry) => (
                  <SelectItem key={entry.value} value={entry.value}>
                    {entry.label}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </label>
          <label className="flex flex-col gap-1.5 text-sm text-muted-foreground">
            <span>Conflict policy</span>
            <Select
              items={CONFLICT_POLICIES}
              value={conflictPolicy}
              onValueChange={(value) => setConflictPolicy(value as ParametricConflictPolicy)}
            >
              <SelectTrigger className="w-fit" aria-label="Conflict policy">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                {CONFLICT_POLICIES.map((entry) => (
                  <SelectItem key={entry.value} value={entry.value}>
                    {entry.label}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </label>
        </div>
        <label className="flex flex-col gap-1.5 text-sm text-muted-foreground">
          <span>Layer id (optional)</span>
          <Input
            value={spec.layerId ?? ""}
            aria-label="Layer id"
            onChange={(event) =>
              setSpec((current) => ({ ...current, layerId: event.target.value || null }))
            }
          />
        </label>
        <div className="font-mono text-xs text-muted-foreground">
          dimension: {sourceDimension.replace("minecraft:", "")}
        </div>
        <div className="flex flex-wrap gap-2">
          <Button size="sm" variant="outline" onClick={useSelection} disabled={!selection}>
            <Crosshair className="size-3" /> Use selection
          </Button>
          {selected ? (
            <Button
              size="sm"
              variant="outline"
              disabled={!canUpdate}
              onClick={() => void updateSelected()}
            >
              <RotateCw className="size-3" /> Update recipe
            </Button>
          ) : (
            <Button
              size="sm"
              variant="outline"
              disabled={!canCreate}
              onClick={() => void createHouse()}
            >
              <Plus className="size-3" /> Create house
            </Button>
          )}
        </div>
        <p className="text-sm text-muted-foreground">
          Each apply is one reversible world operation. The capture of pre-existing blocks is stored
          with the recipe so rollback can restore terrain.
        </p>
      </div>

      <div className="scrollbar-thin flex min-h-0 flex-1 flex-col gap-4 overflow-y-auto rounded-xl border bg-card p-4">
        {inspection && selected ? (
          <>
            <div className="flex items-center gap-2">
              <Boxes className="size-3 text-muted-foreground" />
              <span className="text-xs font-semibold">{inspection.object.name}</span>
              <span className="rounded border border-border px-1 font-mono text-xs text-muted-foreground">
                {inspection.object.generator} v{inspection.object.generatorVersion}
              </span>
              <span className="ml-auto font-mono text-xs text-muted-foreground">
                {inspection.object.id}
              </span>
            </div>
            <div className="grid grid-cols-2 gap-x-3 gap-y-1 font-mono text-xs text-muted-foreground">
              <span>rotation: {parametricRotationLabel(inspection.object.rotation)}</span>
              <span>generated: {inspection.generatedBlocks.toLocaleString()}</span>
              <span>applied at revision: {inspection.object.appliedRevision}</span>
              <span>conflicts: {inspection.conflictCount.toLocaleString()}</span>
              <span className="col-span-2">bounds: {boundsLabel(inspection.object)}</span>
              <span className="col-span-2">
                origin: ({inspection.object.origin.x},{inspection.object.origin.y},
                {inspection.object.origin.z}) · size {inspection.object.params.width}×
                {inspection.object.params.depth}×{inspection.object.params.height}
              </span>
              {inspection.object.layerId ? (
                <span className="col-span-2">layer: {inspection.object.layerId}</span>
              ) : null}
              <span className="col-span-2">
                wall {inspection.object.params.wall} · floor {inspection.object.params.floor} · roof{" "}
                {inspection.object.params.roof}
              </span>
            </div>
            <div className="flex flex-wrap gap-2">
              <Button size="sm" variant="outline" onClick={useObjectBounds}>
                <Crosshair className="size-3" /> Use bounds as selection
              </Button>
              <Button
                size="sm"
                variant="outline"
                disabled={saving}
                onClick={() => void bakeSelected()}
              >
                <Hammer className="size-3" /> Bake / detach
              </Button>
              <Button
                size="sm"
                variant="outline"
                disabled={saving}
                onClick={() => void rollbackSelected()}
              >
                <Undo2 className="size-3" /> Roll back
              </Button>
            </div>
            {inspection.conflictCount > 0 ? (
              <div className="rounded-lg border border-status-warn/40 bg-status-warn/5 p-3 text-sm">
                <div className="mb-1 flex items-center gap-1 text-xs text-status-warn">
                  <AlertTriangle className="size-3" />
                  Manual edits no longer match the recipe
                </div>
                <label className="flex items-center gap-1 text-xs text-muted-foreground">
                  <Checkbox
                    checked={forceRollback}
                    aria-label="Force rollback"
                    onCheckedChange={(checked) => setForceRollback(checked === true)}
                  />
                  Forced rollback discards those manual blocks.
                </label>
                <div className="mt-1 space-y-0.5 font-mono text-xs text-muted-foreground">
                  {inspection.conflicts.slice(0, 8).map((conflict) => (
                    <div key={`${conflict.pos.x}:${conflict.pos.y}:${conflict.pos.z}`}>
                      ({conflict.pos.x},{conflict.pos.y},{conflict.pos.z}) expected{" "}
                      {conflict.expected.name} found {conflict.found.name}
                    </div>
                  ))}
                  {inspection.conflictCount > inspection.conflicts.length ? (
                    <p>
                      +{inspection.conflictCount - inspection.conflicts.length} more conflict(s)
                    </p>
                  ) : null}
                </div>
              </div>
            ) : (
              <p className="text-sm text-muted-foreground">
                No manual conflicts. The generated overlay still matches this recipe.
              </p>
            )}
          </>
        ) : (
          <div className="flex h-full flex-col items-center justify-center gap-2 text-center text-sm text-muted-foreground">
            <Boxes className="size-5 text-muted-foreground/50" />
            <p>Select a parametric object to inspect its recipe, conflicts and rollback actions.</p>
          </div>
        )}
        {inspecting ? (
          <div className="flex items-center gap-1 text-xs text-muted-foreground">
            <Loader2 className="size-3 animate-spin" /> Inspecting…
          </div>
        ) : null}
        {actionError || storeError ? (
          <p className="text-xs text-status-error">{actionError ?? storeError}</p>
        ) : null}
      </div>
    </div>
  );
}
