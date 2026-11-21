import { useState } from "react";
import { Building2, Crosshair, Landmark, Loader2, Plus, Route, Shapes, Trash2 } from "lucide-react";

import { Badge } from "@/components/ui/badge";
import {
  Empty,
  EmptyDescription,
  EmptyHeader,
  EmptyMedia,
  EmptyTitle,
} from "@/components/ui/empty";
import { Button } from "@/components/ui/button";
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
  SCENE_OBJECT_KINDS,
  sceneObjectKindLabel,
  type SceneObjectInspection,
  type SceneObjectKind,
} from "@/project/scene-types";
import { useSceneStore } from "@/state/scene-store";
import { useToolStore } from "@/state/tool-store";
import { useWorldStore } from "@/state/world-store";
import { cn } from "@/lib/utils";

function kindIcon(kind: SceneObjectKind) {
  if (kind === "building") return Building2;
  if (kind === "road") return Route;
  return Landmark;
}

/**
 * Semantic districts, buildings and roads. Objects are project metadata only: creating or
 * deleting one never changes blocks, and inspection reports the current voxel statistics
 * for its bounds at the authoritative revision.
 */
export function ScenePanel() {
  const scene = useSceneStore((state) => state.scene);
  const create = useSceneStore((state) => state.create);
  const remove = useSceneStore((state) => state.remove);
  const inspect = useSceneStore((state) => state.inspect);
  const saving = useSceneStore((state) => state.saving);
  const loading = useSceneStore((state) => state.loading);
  const storeError = useSceneStore((state) => state.error);
  const selection = useToolStore((state) => state.selection);
  const dimension = useWorldStore((state) => state.dimension);
  const [name, setName] = useState("New object");
  const [kind, setKind] = useState<SceneObjectKind>("district");
  const [parentId, setParentId] = useState("");
  const [note, setNote] = useState("");
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [inspection, setInspection] = useState<SceneObjectInspection | null>(null);
  const [inspecting, setInspecting] = useState(false);
  const [actionError, setActionError] = useState<string | null>(null);
  const editing = isTauriRuntime();
  const selected = scene.objects.find((object) => object.id === selectedId) ?? null;
  const parentOptions = scene.objects.filter(
    (object) => object.dimension === dimension && object.id !== selectedId,
  );

  if (!editing) {
    return (
      <div className="flex h-full flex-col items-center justify-center gap-2 text-center text-sm text-muted-foreground">
        <Shapes className="size-5 text-muted-foreground/60" />
        <p className="max-w-md leading-relaxed">
          Semantic districts, buildings and roads are stored in the project and inspected by the
          Rust Core. Open the desktop app to manage them.
        </p>
      </div>
    );
  }

  const selectObject = async (id: string) => {
    setSelectedId(id);
    setInspection(null);
    setInspecting(true);
    setActionError(null);
    const result = await inspect(id);
    setInspecting(false);
    setInspection(result);
  };

  const createFromSelection = async () => {
    setActionError(null);
    if (!selection) {
      setActionError("Select a box in the viewport first.");
      return;
    }
    const object = await create({
      name,
      kind,
      dimension,
      bounds: selection,
      parentId: parentId || null,
      note: note.trim() || null,
    });
    if (!object) return;
    setName("New object");
    setNote("");
    setParentId("");
    await selectObject(object.id);
  };

  const deleteSelected = async () => {
    if (!selectedId) return;
    setActionError(null);
    if (await remove(selectedId)) {
      setSelectedId(null);
      setInspection(null);
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

  return (
    <div className="flex min-h-0 flex-1 gap-4 text-sm" data-testid="scene-panel">
      <div className="flex min-h-0 w-80 shrink-0 flex-col gap-1.5 rounded-xl border bg-card p-4">
        <div className="flex items-center gap-2">
          <Shapes className="size-3 text-muted-foreground" />
          <span className="text-base font-semibold">Objects</span>
          <span className="text-xs text-muted-foreground">{scene.objects.length} / 1024</span>
          {loading || inspecting ? (
            <Loader2 className="ml-auto size-3 animate-spin text-muted-foreground" />
          ) : null}
        </div>
        <div className="scrollbar-thin min-h-0 flex-1 space-y-1 overflow-y-auto">
          {scene.objects.length === 0 ? (
            <p className="p-1 text-xs text-muted-foreground">
              No semantic objects yet. Select an area and describe what it is.
            </p>
          ) : (
            scene.objects.map((object) => {
              const Icon = kindIcon(object.kind);
              return (
                <button
                  key={object.id}
                  type="button"
                  onClick={() => void selectObject(object.id)}
                  className={cn(
                    "w-full rounded-lg border px-3 py-2.5 text-left",
                    object.id === selectedId
                      ? "border-primary/60 bg-primary/10"
                      : "bg-card hover:bg-accent",
                  )}
                >
                  <div className="flex items-center gap-1">
                    <Icon className="size-3 shrink-0 text-muted-foreground" />
                    <span className="truncate text-sm">{object.name}</span>
                    <span className="ml-auto font-mono text-xs text-muted-foreground">
                      {sceneObjectKindLabel(object.kind)}
                    </span>
                  </div>
                  <div className="truncate font-mono text-xs text-muted-foreground">
                    {object.dimension.replace("minecraft:", "")} · ({object.bounds.min.x},
                    {object.bounds.min.y},{object.bounds.min.z})..({object.bounds.max.x},
                    {object.bounds.max.y},{object.bounds.max.z})
                  </div>
                </button>
              );
            })
          )}
        </div>
      </div>

      <div className="scrollbar-thin flex min-h-0 w-80 shrink-0 flex-col gap-1.5 overflow-y-auto rounded-xl border bg-card p-4">
        <div className="text-base font-semibold">Create from selection</div>
        <Input
          value={name}
          onChange={(event) => setName(event.target.value)}
          aria-label="Object name"
        />
        <Select
          items={SCENE_OBJECT_KINDS}
          value={kind}
          onValueChange={(value) => setKind(value as SceneObjectKind)}
        >
          <SelectTrigger className="w-fit" aria-label="Object kind">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            {SCENE_OBJECT_KINDS.map((entry) => (
              <SelectItem key={entry.value} value={entry.value}>
                {entry.label}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
        <Select
          items={[
            { value: "", label: "No parent" },
            ...parentOptions.map((object) => ({ value: object.id, label: object.name })),
          ]}
          value={parentId}
          onValueChange={(value) => setParentId(value ?? "")}
        >
          <SelectTrigger className="w-fit" aria-label="Parent object">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value="">No parent</SelectItem>
            {parentOptions.map((object) => (
              <SelectItem key={object.id} value={object.id}>
                {object.name}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
        <Input
          value={note}
          onChange={(event) => setNote(event.target.value)}
          placeholder="Optional note"
          aria-label="Object note"
        />
        <Button
          size="sm"
          variant="outline"
          disabled={saving || !selection}
          title={selection ? "Create from the current selection" : "Select a box first"}
          onClick={() => void createFromSelection()}
        >
          <Plus className="size-3" /> Create object
        </Button>
        <p className="text-sm text-muted-foreground">
          Semantic objects are metadata. Creating or deleting one does not place or remove blocks.
        </p>
      </div>

      <div className="scrollbar-thin flex min-h-0 flex-1 flex-col gap-2 overflow-y-auto rounded-xl border bg-card p-4">
        {inspection && selected ? (
          <>
            <div className="flex items-center gap-2">
              <Landmark className="size-3 text-muted-foreground" />
              <span className="text-base font-semibold">{inspection.object.name}</span>
              <Badge variant="outline" className="font-mono">
                {inspection.object.kind}
              </Badge>
              <span className="ml-auto font-mono text-xs text-muted-foreground">
                {inspection.object.id}
              </span>
            </div>
            <div className="grid grid-cols-2 gap-x-3 gap-y-1 font-mono text-xs text-muted-foreground">
              <span>dimension: {inspection.object.dimension.replace("minecraft:", "")}</span>
              <span>volume: {inspection.volume.toLocaleString()}</span>
              <span>non-air: {inspection.nonAirBlocks.toLocaleString()}</span>
              <span>children: {inspection.children.length}</span>
              <span className="col-span-2">
                bounds ({inspection.object.bounds.min.x},{inspection.object.bounds.min.y},
                {inspection.object.bounds.min.z})..({inspection.object.bounds.max.x},
                {inspection.object.bounds.max.y},{inspection.object.bounds.max.z})
              </span>
              {inspection.object.parentId ? (
                <span className="col-span-2">parent: {inspection.object.parentId}</span>
              ) : null}
              {inspection.object.note ? (
                <span className="col-span-2 text-foreground/80">{inspection.object.note}</span>
              ) : null}
              {Object.entries(inspection.object.tags ?? {}).map(([key, value]) => (
                <span key={key} className="col-span-2">
                  {key}: {value}
                </span>
              ))}
            </div>
            <div className="flex flex-wrap gap-2">
              <Button size="sm" variant="outline" onClick={useObjectBounds}>
                <Crosshair className="size-3" /> Use bounds as selection
              </Button>
              <Button
                size="sm"
                variant="outline"
                disabled={saving}
                onClick={() => void deleteSelected()}
              >
                <Trash2 className="size-3" /> Delete object
              </Button>
            </div>
            <div>
              <div className="mb-1 text-sm font-semibold">Current palette</div>
              <div className="space-y-0.5 font-mono text-xs text-muted-foreground">
                {inspection.palette.length === 0 ? (
                  <p>No blocks in bounds.</p>
                ) : (
                  inspection.palette.slice(0, 8).map((entry) => (
                    <div key={entry.state} className="flex justify-between gap-2">
                      <span className="truncate">{entry.state}</span>
                      <span>{entry.count.toLocaleString()}</span>
                    </div>
                  ))
                )}
                {inspection.palette.length > 8 ? (
                  <p>+{inspection.palette.length - 8} more state(s)</p>
                ) : null}
              </div>
            </div>
            {inspection.children.length > 0 ? (
              <div>
                <div className="mb-1 text-sm font-semibold">Children</div>
                <div className="space-y-0.5 text-xs">
                  {inspection.children.map((child) => (
                    <button
                      key={child.id}
                      type="button"
                      className="block truncate text-left hover:underline"
                      onClick={() => void selectObject(child.id)}
                    >
                      {child.name} · {child.kind}
                    </button>
                  ))}
                </div>
              </div>
            ) : null}
          </>
        ) : (
          <Empty className="h-full border-0">
            <EmptyHeader>
              <EmptyMedia variant="icon">
                <Shapes />
              </EmptyMedia>
              <EmptyTitle>No object selected</EmptyTitle>
              <EmptyDescription>
                Select an object to inspect its bounds and current block palette.
              </EmptyDescription>
            </EmptyHeader>
          </Empty>
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
