import { useEffect, useMemo, useState } from "react";
import {
  AlertTriangle,
  Archive,
  CheckCircle2,
  Clock,
  FolderOpen,
  HardDrive,
  History,
  Loader2,
  Pencil,
  Plus,
  RotateCcw,
  Save,
  ShieldAlert,
  Sparkles,
  Trash2,
  X,
} from "lucide-react";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Separator } from "@/components/ui/separator";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { isTauriRuntime } from "@/lib/ipc";
import { projectIsDirty, useProjectStore } from "@/state/project-store";
import { useWorldStore } from "@/state/world-store";
import {
  CREATION_MODES,
  creationModeLabel,
  formatBytes,
  formatRelativeTime,
  type CreationMode,
} from "@/project/types";
import { cn } from "@/lib/utils";

type PendingAction =
  { kind: "open"; folder: string; name: string } | { kind: "close" } | { kind: "create" } | null;

function ProjectRow({
  name,
  meta,
  active,
  recoverable,
  warnings,
  onOpen,
  onRename,
  onCheckpoints,
}: {
  name: string;
  meta: string;
  active: boolean;
  recoverable: boolean;
  warnings: string[];
  onOpen: () => void;
  onRename: () => void;
  onCheckpoints: () => void;
}) {
  return (
    <div
      data-testid={`project-row-${name}`}
      className={cn(
        "rounded-md border px-2.5 py-2",
        active ? "border-primary/50 bg-primary/10" : "border-border bg-background/40",
      )}
    >
      <div className="flex items-start gap-2">
        <div className="mt-0.5 grid size-7 shrink-0 place-items-center rounded border border-border bg-card">
          <Archive className="size-3.5 text-muted-foreground" />
        </div>
        <div className="min-w-0 flex-1">
          <div className="flex items-center gap-1.5">
            <span className="truncate text-[12px] font-medium">{name}</span>
            {active ? (
              <span className="rounded bg-primary/20 px-1 py-0.5 text-[8px] font-semibold tracking-wide text-primary uppercase">
                open
              </span>
            ) : null}
            {recoverable ? (
              <span className="rounded bg-status-warn/15 px-1 py-0.5 text-[8px] font-semibold tracking-wide text-status-warn uppercase">
                recoverable
              </span>
            ) : null}
          </div>
          <div className="mt-0.5 truncate font-mono text-[9px] text-muted-foreground">{meta}</div>
          {warnings.length > 0 ? (
            <div className="mt-1 text-[9px] leading-relaxed text-status-warn">
              {warnings.join(" · ")}
            </div>
          ) : null}
        </div>
        <div className="flex shrink-0 items-center gap-1">
          <Button size="sm" className="h-6 px-2 text-[10px]" onClick={onOpen}>
            <FolderOpen className="mr-1 size-3" /> Open
          </Button>
          <Button
            size="icon"
            variant="ghost"
            className="size-6"
            aria-label={`Rename ${name}`}
            onClick={onRename}
          >
            <Pencil className="size-3" />
          </Button>
          <Button
            size="icon"
            variant="ghost"
            className="size-6"
            aria-label={`Checkpoints for ${name}`}
            onClick={onCheckpoints}
          >
            <History className="size-3" />
          </Button>
        </div>
      </div>
    </div>
  );
}

export function ProjectManager() {
  const open = useProjectStore((state) => state.managerOpen);
  const setOpen = useProjectStore((state) => state.setManagerOpen);
  const projects = useProjectStore((state) => state.projects);
  const warnings = useProjectStore((state) => state.warnings);
  const current = useProjectStore((state) => state.current);
  const recovery = useProjectStore((state) => state.recovery);
  const checkpoints = useProjectStore((state) => state.checkpoints);
  const loading = useProjectStore((state) => state.loading);
  const error = useProjectStore((state) => state.error);
  const lastSave = useProjectStore((state) => state.lastSave);
  const worldRevision = useWorldStore((state) => state.revision);
  const [tab, setTab] = useState("projects");
  const [name, setName] = useState("");
  const [mode, setMode] = useState<CreationMode>("empty");
  const [renameFolder, setRenameFolder] = useState<string | null>(null);
  const [renameValue, setRenameValue] = useState("");
  const [pending, setPending] = useState<PendingAction>(null);
  const [checkpointReason, setCheckpointReason] = useState("manual checkpoint");
  const dirty = projectIsDirty(current, worldRevision);
  const activeFolder = current?.folder ?? null;
  const desktop = isTauriRuntime();

  useEffect(() => {
    if (!open || !desktop) return;
    void useProjectStore.getState().refreshList();
    void useProjectStore.getState().refreshSession();
    if (current?.folder) void useProjectStore.getState().loadCheckpoints(current.folder);
  }, [open, current?.folder, desktop]);

  const currentProject = useMemo(
    () => projects.find((project) => project.folder === activeFolder) ?? null,
    [projects, activeFolder],
  );

  const requestOpen = (folder: string, projectName: string): void => {
    if (folder === activeFolder) {
      setOpen(false);
      return;
    }
    if (dirty) {
      setPending({ kind: "open", folder, name: projectName });
      return;
    }
    void useProjectStore.getState().open(folder);
  };

  const confirmPending = async (strategy: "save" | "discard"): Promise<void> => {
    const action = pending;
    if (!action) return;
    if (strategy === "save") {
      const saved = await useProjectStore.getState().save();
      if (!saved) return;
    }
    if (action.kind === "open") {
      await useProjectStore.getState().open(action.folder, { discardCurrent: true });
    } else if (action.kind === "close") {
      await useProjectStore.getState().close(false);
      setTab("projects");
    } else if (action.kind === "create") {
      await useProjectStore.getState().close(false);
      setTab("create");
    }
    setPending(null);
  };

  const submitCreate = async (): Promise<void> => {
    if (dirty) {
      setPending({ kind: "create" });
      return;
    }
    const created = await useProjectStore.getState().create(name, mode);
    if (created) {
      setName("");
      setTab("projects");
    }
  };

  const submitRename = async (): Promise<void> => {
    if (!renameFolder) return;
    const renamed = await useProjectStore.getState().rename(renameFolder, renameValue);
    if (renamed) {
      setRenameFolder(null);
      setRenameValue("");
    }
  };

  return (
    <Dialog open={open} onOpenChange={setOpen}>
      <DialogContent className="max-w-4xl">
        <DialogHeader>
          <DialogTitle>Projects</DialogTitle>
          <DialogDescription>
            Project folders live outside the repository in the app data directory. Snapshots are
            verified against a SHA-256 checksum before the manifest references them.
          </DialogDescription>
        </DialogHeader>

        {!desktop ? (
          <div className="rounded border border-status-warn/40 bg-status-warn/10 p-3 text-[11px] leading-relaxed text-muted-foreground">
            <div className="mb-1 flex items-center gap-1.5 font-medium text-status-warn">
              <ShieldAlert className="size-3.5" /> Desktop core required
            </div>
            Project storage is implemented in the Tauri backend (atomic snapshots, checksums,
            autosave and checkpoints). Browser preview mode can inspect the demo scene, but list,
            create, open and save need the desktop app.
          </div>
        ) : (
          <>
            {error ? (
              <div className="flex items-center gap-2 rounded border border-status-error/40 bg-status-error/5 px-2 py-1.5 text-[10px] text-status-error">
                <AlertTriangle className="size-3.5" /> {error}
              </div>
            ) : null}

            {recovery ? (
              <div className="rounded border border-status-warn/40 bg-status-warn/10 p-2.5 text-[10px]">
                <div className="flex items-center gap-1.5 font-medium text-status-warn">
                  <ShieldAlert className="size-3.5" /> Recovered unsaved work
                </div>
                <p className="mt-1 leading-relaxed text-muted-foreground">
                  {recovery.reason} Revision {recovery.recoveredRevision} was loaded from a verified
                  autosave. Save the project to keep it.
                </p>
                <div className="mt-1.5 flex gap-1">
                  <Button
                    size="sm"
                    variant="outline"
                    className="h-6 px-2 text-[10px]"
                    onClick={() => useProjectStore.getState().dismissRecovery()}
                  >
                    <CheckCircle2 className="mr-1 size-3" /> Keep recovered data
                  </Button>
                  <Button
                    size="sm"
                    variant="ghost"
                    className="h-6 px-2 text-[10px]"
                    onClick={() => void useProjectStore.getState().discardRecovery()}
                  >
                    <Trash2 className="mr-1 size-3" /> Discard and reload saved
                  </Button>
                </div>
              </div>
            ) : null}

            {pending ? (
              <div className="rounded border border-primary/40 bg-primary/10 p-2.5 text-[10px]">
                <div className="font-medium">Unsaved changes in “{current?.name ?? "project"}”</div>
                <p className="mt-1 text-muted-foreground">
                  {pending.kind === "open"
                    ? `Save before opening “${pending.name}”, or discard the current edits.`
                    : "Save before leaving this project, or discard the current edits."}
                </p>
                <div className="mt-1.5 flex gap-1">
                  <Button
                    size="sm"
                    className="h-6 px-2 text-[10px]"
                    disabled={loading}
                    onClick={() => void confirmPending("save")}
                  >
                    <Save className="mr-1 size-3" /> Save and continue
                  </Button>
                  <Button
                    size="sm"
                    variant="outline"
                    className="h-6 px-2 text-[10px]"
                    disabled={loading}
                    onClick={() => void confirmPending("discard")}
                  >
                    Discard changes
                  </Button>
                  <Button
                    size="sm"
                    variant="ghost"
                    className="h-6 px-2 text-[10px]"
                    onClick={() => setPending(null)}
                  >
                    Cancel
                  </Button>
                </div>
              </div>
            ) : null}

            <Tabs value={tab} onValueChange={setTab} className="min-h-0">
              <TabsList>
                <TabsTrigger value="projects">
                  <FolderOpen className="size-3" /> Projects ({projects.length})
                </TabsTrigger>
                <TabsTrigger value="create">
                  <Plus className="size-3" /> New project
                </TabsTrigger>
                <TabsTrigger value="checkpoints">
                  <History className="size-3" /> Checkpoints
                </TabsTrigger>
              </TabsList>

              <TabsContent value="projects" className="m-0 space-y-2 pt-2">
                {current ? (
                  <div className="flex items-center gap-2 rounded border border-border bg-card/60 px-2.5 py-1.5 text-[10px]">
                    <HardDrive className="size-3.5 text-primary" />
                    <span className="font-medium">{current.name}</span>
                    <span className="font-mono text-muted-foreground">
                      rev {current.savedRevision}
                      {current.autosavedRevision !== null
                        ? ` · autosave r${current.autosavedRevision}`
                        : ""}
                    </span>
                    <span
                      className={cn(
                        "rounded px-1 py-0.5 text-[8px] font-semibold uppercase",
                        dirty
                          ? "bg-status-warn/15 text-status-warn"
                          : "bg-status-ok/15 text-status-ok",
                      )}
                    >
                      {dirty ? "unsaved" : "saved"}
                    </span>
                    <span className="ml-auto flex items-center gap-1">
                      <Button
                        size="sm"
                        variant="outline"
                        className="h-6 px-2 text-[10px]"
                        disabled={loading || !dirty}
                        onClick={() => void useProjectStore.getState().save()}
                      >
                        {loading ? (
                          <Loader2 className="mr-1 size-3 animate-spin" />
                        ) : (
                          <Save className="mr-1 size-3" />
                        )}
                        Save
                      </Button>
                      <Button
                        size="sm"
                        variant="ghost"
                        className="h-6 px-2 text-[10px]"
                        onClick={() =>
                          dirty
                            ? setPending({ kind: "close" })
                            : void useProjectStore.getState().close(false)
                        }
                      >
                        <X className="mr-1 size-3" /> Close
                      </Button>
                    </span>
                  </div>
                ) : (
                  <div className="rounded border border-dashed border-border px-3 py-4 text-center text-[11px] text-muted-foreground">
                    No project is open. Pick one below or create a new project.
                  </div>
                )}

                {lastSave ? (
                  <div className="rounded border border-status-ok/30 bg-status-ok/5 px-2 py-1 font-mono text-[9px] text-muted-foreground">
                    saved {lastSave.snapshotFile} · {formatBytes(lastSave.bytesWritten)} · checksum{" "}
                    {lastSave.checksum.slice(0, 16)}… · {lastSave.project.summary.lastSaveKind}
                    {lastSave.backupCreated ? " · manifest backup" : ""}
                    {lastSave.prunedSnapshots > 0
                      ? ` · pruned ${lastSave.prunedSnapshots} snapshot(s)`
                      : ""}
                  </div>
                ) : null}

                <div className="scrollbar-thin max-h-[46vh] space-y-1.5 overflow-y-auto pr-1">
                  {projects.length === 0 ? (
                    <div className="flex flex-col items-center gap-2 py-8 text-[11px] text-muted-foreground">
                      <Archive className="size-6 text-muted-foreground/50" />
                      <p>No projects yet.</p>
                      <Button
                        size="sm"
                        className="h-7 px-2 text-[10px]"
                        onClick={() => setTab("create")}
                      >
                        <Plus className="mr-1 size-3" /> Create an Empty Project
                      </Button>
                    </div>
                  ) : (
                    projects.map((project) => (
                      <ProjectRow
                        key={project.folder}
                        name={project.name}
                        active={project.folder === activeFolder}
                        recoverable={project.recoverable}
                        warnings={
                          project.schemaVersion > 1
                            ? ["Newer schema; update FuryBuilder"]
                            : project.bytes === 0
                              ? ["Manifest has no snapshot size"]
                              : []
                        }
                        meta={`${creationModeLabel(project.creationMode)} · ${project.targetMinecraftVersion} · DV ${project.targetDataVersion} · rev ${project.worldRevision} · ${formatBytes(project.bytes)} · saved ${formatRelativeTime(project.lastSavedAtMs)}`}
                        onOpen={() => requestOpen(project.folder, project.name)}
                        onRename={() => {
                          setRenameFolder(project.folder);
                          setRenameValue(project.name);
                        }}
                        onCheckpoints={() => {
                          void useProjectStore.getState().loadCheckpoints(project.folder);
                          setTab("checkpoints");
                        }}
                      />
                    ))
                  )}
                </div>

                {warnings.length > 0 ? (
                  <div className="space-y-1 rounded border border-status-warn/30 bg-status-warn/5 p-2 text-[9px] text-status-warn">
                    {warnings.map((warning) => (
                      <div key={warning}>{warning}</div>
                    ))}
                  </div>
                ) : null}

                {renameFolder ? (
                  <div className="flex items-center gap-1.5 rounded border border-border bg-background/40 p-2">
                    <Input
                      className="h-6 text-[11px]"
                      value={renameValue}
                      aria-label="New project name"
                      onChange={(event) => setRenameValue(event.target.value)}
                    />
                    <Button
                      size="sm"
                      className="h-6 px-2 text-[10px]"
                      disabled={loading || renameValue.trim().length === 0}
                      onClick={() => void submitRename()}
                    >
                      Rename
                    </Button>
                    <Button
                      size="sm"
                      variant="ghost"
                      className="h-6 px-2 text-[10px]"
                      onClick={() => setRenameFolder(null)}
                    >
                      Cancel
                    </Button>
                  </div>
                ) : null}
              </TabsContent>

              <TabsContent value="create" className="m-0 space-y-2 pt-2">
                <div className="space-y-1">
                  <label
                    className="text-[10px] font-medium text-muted-foreground"
                    htmlFor="project-name"
                  >
                    Project name
                  </label>
                  <Input
                    id="project-name"
                    value={name}
                    placeholder="My Castle"
                    maxLength={64}
                    onChange={(event) => setName(event.target.value)}
                  />
                </div>

                <div className="grid grid-cols-2 gap-1.5">
                  {CREATION_MODES.map((entry) => (
                    <button
                      key={entry.mode}
                      type="button"
                      disabled={!entry.available || loading}
                      aria-pressed={mode === entry.mode}
                      onClick={() => setMode(entry.mode)}
                      className={cn(
                        "rounded border p-2 text-left",
                        mode === entry.mode && entry.available
                          ? "border-primary/60 bg-primary/10"
                          : "border-border bg-background/40",
                        entry.available
                          ? "hover:border-primary/40"
                          : "cursor-not-allowed opacity-55",
                      )}
                    >
                      <div className="flex items-center gap-1.5 text-[11px] font-medium">
                        {entry.label}
                        {entry.available ? null : (
                          <span className="rounded bg-muted px-1 py-0.5 text-[8px] font-semibold tracking-wide uppercase">
                            future
                          </span>
                        )}
                      </div>
                      <p className="mt-1 text-[9px] leading-relaxed text-muted-foreground">
                        {entry.description}
                      </p>
                    </button>
                  ))}
                </div>

                <div className="flex items-center gap-2 rounded border border-border bg-background/40 px-2 py-1.5 text-[10px]">
                  <Sparkles className="size-3.5 text-primary" />
                  <span className="text-muted-foreground">Target Java</span>
                  <select
                    aria-label="Target Java version"
                    className="rounded border border-border bg-card px-1.5 py-0.5 font-mono text-[10px]"
                    value="1.21.4"
                    disabled
                  >
                    <option value="1.21.4">Minecraft Java 1.21.4 · DataVersion 4189</option>
                  </select>
                  <span className="ml-auto text-[9px] text-muted-foreground">
                    Saved as <span className="font-mono">project_schema_version 1</span>
                  </span>
                </div>

                <div className="flex items-center gap-2">
                  <Button
                    size="sm"
                    disabled={loading || name.trim().length === 0}
                    onClick={() => void submitCreate()}
                  >
                    {loading ? (
                      <Loader2 className="mr-1 size-3 animate-spin" />
                    ) : (
                      <Plus className="mr-1 size-3" />
                    )}
                    Create project
                  </Button>
                  <span className="text-[9px] text-muted-foreground">
                    Folders get a unique suffix when the name already exists. Renaming changes the
                    display name only.
                  </span>
                </div>
              </TabsContent>

              <TabsContent value="checkpoints" className="m-0 space-y-2 pt-2">
                <div className="flex items-center gap-2">
                  <Input
                    className="h-6 text-[11px]"
                    value={checkpointReason}
                    aria-label="Checkpoint reason"
                    onChange={(event) => setCheckpointReason(event.target.value)}
                  />
                  <Button
                    size="sm"
                    className="h-6 px-2 text-[10px]"
                    disabled={loading || !activeFolder}
                    onClick={() =>
                      void useProjectStore.getState().createCheckpoint(checkpointReason || "manual")
                    }
                  >
                    <Plus className="mr-1 size-3" /> Create checkpoint
                  </Button>
                  <Button
                    size="sm"
                    variant="outline"
                    className="h-6 px-2 text-[10px]"
                    onClick={() =>
                      void useProjectStore.getState().loadCheckpoints(activeFolder ?? undefined)
                    }
                  >
                    <RotateCcw className="mr-1 size-3" /> Refresh
                  </Button>
                </div>
                <p className="text-[9px] text-muted-foreground">
                  Checkpoints are full verified snapshots kept for the current project (up to 5).
                  Restoring one replaces the open world and leaves it unsaved until you save.
                </p>
                {checkpoints.length === 0 ? (
                  <div className="flex items-center gap-2 rounded border border-dashed border-border px-2 py-3 text-[10px] text-muted-foreground">
                    <History className="size-3.5" /> No checkpoints for{" "}
                    {currentProject?.name ?? activeFolder ?? "this project"}.
                  </div>
                ) : (
                  <div className="space-y-1">
                    {checkpoints.map((checkpoint) => (
                      <div
                        key={checkpoint.revision}
                        className="flex items-center gap-2 rounded border border-border bg-background/40 px-2 py-1 font-mono text-[9px]"
                      >
                        <Clock className="size-3 text-muted-foreground" />
                        <span>rev {checkpoint.revision}</span>
                        <span className="text-muted-foreground">
                          {formatBytes(checkpoint.bytes)}
                        </span>
                        <span className="truncate text-muted-foreground">{checkpoint.reason}</span>
                        <span className="ml-auto text-muted-foreground">
                          {formatRelativeTime(checkpoint.savedAtMs)}
                        </span>
                        <Button
                          size="sm"
                          variant="outline"
                          className="h-5 px-1.5 text-[9px]"
                          disabled={loading}
                          onClick={() => {
                            if (
                              window.confirm(
                                `Restore checkpoint revision ${checkpoint.revision}? The open world will be replaced.`,
                              )
                            ) {
                              void useProjectStore
                                .getState()
                                .restoreCheckpoint(checkpoint.revision);
                            }
                          }}
                        >
                          Restore
                        </Button>
                      </div>
                    ))}
                  </div>
                )}
              </TabsContent>
            </Tabs>

            <Separator />
            <div className="flex items-center gap-2 text-[9px] text-muted-foreground">
              <HardDrive className="size-3" />
              <span>
                Atomic save: temp file → decode + re-encode verification → checksum → publish →
                manifest. Autosave runs every 30 seconds while edits are unsaved and on window
                close.
              </span>
              {loading ? <Loader2 className="ml-auto size-3 animate-spin text-primary" /> : null}
            </div>
          </>
        )}
      </DialogContent>
    </Dialog>
  );
}

export function useProjectAutosave(intervalMs = 30_000): void {
  useEffect(() => {
    const tick = (): void => {
      const state = useProjectStore.getState();
      if (state.loading || !state.current?.folder) return;
      const revision = useWorldStore.getState().revision;
      if (revision === state.current.savedRevision) return;
      if (state.current.autosavedRevision === revision) return;
      void state.autosave();
    };
    const timer = window.setInterval(tick, intervalMs);
    const onVisibility = (): void => {
      if (document.visibilityState === "hidden") tick();
    };
    document.addEventListener("visibilitychange", onVisibility);
    return () => {
      window.clearInterval(timer);
      document.removeEventListener("visibilitychange", onVisibility);
    };
  }, [intervalMs]);
}
