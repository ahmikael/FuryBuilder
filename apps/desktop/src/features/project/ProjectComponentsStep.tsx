import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import type { ReactNode } from "react";
import {
  Ban,
  BookOpen,
  CheckCircle2,
  Coffee,
  Download,
  FolderOpen,
  HardDrive,
  Loader2,
  PackageCheck,
  RefreshCw,
  Server,
  ShieldCheck,
  TriangleAlert,
} from "lucide-react";
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent } from "@/components/ui/card";
import { Progress } from "@/components/ui/progress";
import { ToggleGroup, ToggleGroupItem } from "@/components/ui/toggle-group";
import type { CatalogSnapshot } from "@/assets/types";
import { loadAssetCatalog } from "@/lib/assets-api";
import {
  cancelComponentInstall,
  componentAssetsRelease,
  componentInventory,
  componentJavaRelease,
  componentServerRelease,
  componentStatus,
  installComponent,
  listenComponentChanged,
  listenComponentError,
  listenComponentProgress,
  relinkComponent,
  selectPlaceholderComponent,
} from "@/lib/components-api";
import type {
  ClientJarReleaseInfo,
  ComponentInstallRequest,
  ComponentInventory,
  ComponentKind,
  ComponentMode,
  ComponentProgress,
  ComponentReport,
  ComponentRequirements,
  ComponentState,
  ComponentStatus,
  ComponentTarget,
} from "@/lib/component-types";
import { presentError, type ErrorPresentation } from "@/lib/error-messages";
import { formatBytes, type CreationMode } from "@/project/types";
import type { VersionProfile } from "@/project/version-types";
import type { ServerReleaseInfo } from "@/runtime/server-types";
import type { RuntimeReleaseInfo } from "@/runtime/types";
import { cn } from "@/lib/utils";

/** One resolved component as the Review step should present it. */
export interface ComponentStepItem {
  kind: ComponentKind;
  mode: ComponentMode | null;
  state: ComponentState;
  version: string | null;
  bytes: number | null;
  hash: string | null;
  path: string | null;
}

/** The verified component facts captured by the Components step for the Review summary. */
export interface ComponentStepSummary {
  java: ComponentStepItem | null;
  assets: ComponentStepItem | null;
  server: ComponentStepItem | null;
  catalog: {
    id: string;
    hash: string;
    blocks: number;
    items: number;
  } | null;
  totalBytes: number;
}

/** The state the wizard needs to validate the step without reaching into its children. */
export interface ComponentStepState {
  targetKey: string;
  checking: boolean;
  ready: boolean;
  errors: string[];
  warnings: string[];
  summary: ComponentStepSummary | null;
}

type ProvisionMode = "auto" | "existing" | "manual";
type AssetProvisionMode = "auto" | "manual" | "placeholder";

interface ProjectComponentsStepProps {
  /** Stable key for the selected version and creation mode; the step echoes it back. */
  targetKey: string;
  mode: CreationMode;
  profile: VersionProfile | null;
  desktop: boolean;
  disabled?: boolean;
  onStateChange: (state: ComponentStepState) => void;
}

const STATE_LABELS: Record<ComponentState, string> = {
  ready: "Ready",
  missing: "Not set up",
  unverified: "Unverified",
  incompatible: "Incompatible",
  error: "Error",
  not_required: "Not required",
};

function stateVariant(state: ComponentState): "default" | "secondary" | "destructive" | "outline" {
  switch (state) {
    case "ready":
      return "secondary";
    case "incompatible":
    case "error":
      return "destructive";
    case "not_required":
    case "missing":
      return "outline";
    default:
      return "secondary";
  }
}

function modeLabel(mode: ComponentMode): string {
  switch (mode) {
    case "managed":
      return "Managed";
    case "manual":
      return "Manual";
    case "placeholder":
      return "Placeholder";
  }
}

function targetFor(profile: VersionProfile): ComponentTarget {
  return {
    profileId: profile.id,
    minecraftVersion: profile.minecraftVersion,
    dataVersion: profile.dataVersion,
    javaMajor: profile.java.recommendedMajor,
  };
}

function releaseHost(url: string): string {
  try {
    return new URL(url).host;
  } catch {
    return url;
  }
}

function shortPath(path: string): string {
  if (path.length <= 52) return path;
  return `…${path.slice(-49)}`;
}

function hashLabel(status: ComponentStatus | null): string | null {
  if (!status?.hash) return null;
  return `${status.hash.algorithm.toUpperCase()} ${status.hash.value.slice(0, 12)}…`;
}

function componentLabel(kind: ComponentKind): string {
  switch (kind) {
    case "java":
      return "Java runtime";
    case "assets":
      return "Minecraft client assets";
    case "server":
      return "Official server JAR";
  }
}

function findStatus(report: ComponentReport | null, kind: ComponentKind): ComponentStatus | null {
  return report?.components.find((component) => component.kind === kind) ?? null;
}

function installRequest(
  kind: ComponentKind,
  target: ComponentTarget,
  releases: {
    java: RuntimeReleaseInfo | null;
    assets: ClientJarReleaseInfo | null;
    server: ServerReleaseInfo | null;
  },
): ComponentInstallRequest | null {
  if (kind === "java") {
    const release = releases.java;
    if (!release) return null;
    return {
      kind,
      target,
      expectedVersion: release.version,
      expectedHash: { algorithm: "sha256", value: release.sha256 },
    };
  }
  if (kind === "assets") {
    const release = releases.assets;
    if (!release) return null;
    return {
      kind,
      target,
      expectedVersion: release.minecraftVersion,
      expectedHash: { algorithm: "sha1", value: release.sha1 },
    };
  }
  const release = releases.server;
  if (!release) return null;
  return {
    kind,
    target,
    expectedVersion: release.minecraftVersion,
    expectedHash: { algorithm: "sha1", value: release.sha1 },
  };
}

function StatusDetails({ status }: { status: ComponentStatus | null }) {
  if (!status) return null;
  const hash = hashLabel(status);
  return (
    <div className="space-y-1 text-xs">
      {status.state === "ready" ? (
        <div className="flex flex-wrap items-center gap-x-3 gap-y-1 text-muted-foreground">
          {status.version ? <span className="font-mono">{status.version}</span> : null}
          {status.bytes !== undefined ? <span>{formatBytes(status.bytes)}</span> : null}
          {hash ? (
            <span className="font-mono" title={status.hash?.value}>
              {hash}
            </span>
          ) : null}
          {status.path ? (
            <span className="max-w-full min-w-0 truncate font-mono" title={status.path}>
              {status.path}
            </span>
          ) : null}
        </div>
      ) : status.mode === "placeholder" ? (
        <p className="text-status-warn">
          No client JAR, catalog or checksum is associated with this explicit choice.
        </p>
      ) : status.state === "missing" ? (
        <p className="text-muted-foreground">Nothing is selected for this project yet.</p>
      ) : (
        <p className="text-status-warn">{status.error ?? "This component is not verified."}</p>
      )}
      {status.notes?.length ? (
        <ul className="space-y-0.5 text-muted-foreground">
          {status.notes.map((note) => (
            <li key={note}>{note}</li>
          ))}
        </ul>
      ) : null}
    </div>
  );
}

function ComponentCard({
  icon,
  title,
  description,
  status,
  children,
}: {
  icon: ReactNode;
  title: string;
  description: string;
  status: ComponentStatus | null;
  children: ReactNode;
}) {
  return (
    <Card className="gap-0 py-0">
      <CardContent className="space-y-4 p-5">
        <div className="flex flex-wrap items-start gap-3">
          <div className="flex size-9 shrink-0 items-center justify-center rounded-lg border bg-muted/40">
            {icon}
          </div>
          <div className="min-w-0 flex-1 space-y-1">
            <div className="flex flex-wrap items-center gap-2">
              <h3 className="text-sm font-semibold">{title}</h3>
              {status ? (
                <Badge variant={stateVariant(status.state)}>{STATE_LABELS[status.state]}</Badge>
              ) : null}
              {status?.mode ? <Badge variant="outline">{modeLabel(status.mode)}</Badge> : null}
            </div>
            <p className="text-sm leading-relaxed text-muted-foreground">{description}</p>
          </div>
        </div>
        <StatusDetails status={status} />
        {children}
      </CardContent>
    </Card>
  );
}

interface ReleaseSummary {
  title: string;
  meta: string;
  size: number;
  hash: string;
  host: string;
  downloadLabel: string;
}

function ReleasePanel({
  summary,
  error,
  busy,
  progress,
  readyManaged,
  disabled,
  onDownload,
  onCancel,
  onRetry,
}: {
  summary: ReleaseSummary | null;
  error: ErrorPresentation | null;
  busy: boolean;
  progress: ComponentProgress | null;
  readyManaged: boolean;
  disabled: boolean;
  onDownload: () => void;
  onCancel: () => void;
  onRetry: () => void;
}) {
  if (busy) {
    const value =
      progress && progress.total > 0
        ? Math.min(100, Math.round((progress.downloaded / progress.total) * 100))
        : null;
    return (
      <div className="space-y-2 rounded-lg border bg-muted/20 p-3">
        <div className="flex items-center gap-2 text-sm">
          <Loader2 className="size-4 animate-spin" />
          <span className="font-medium">{progress?.phase ?? "Downloading"}</span>
          {progress && progress.total > 0 ? (
            <span className="ml-auto font-mono text-xs text-muted-foreground">
              {formatBytes(progress.downloaded)} / {formatBytes(progress.total)}
            </span>
          ) : null}
        </div>
        {value !== null ? (
          <Progress value={value} aria-label="Component download progress" />
        ) : null}
        <Button type="button" size="sm" variant="outline" onClick={onCancel}>
          <Ban data-icon="inline-start" />
          Cancel download
        </Button>
      </div>
    );
  }

  if (readyManaged) {
    return (
      <p className="flex items-center gap-2 text-sm text-status-ok">
        <CheckCircle2 className="size-4" />
        A verified managed component is ready. No download is needed.
      </p>
    );
  }

  if (error) {
    return (
      <Alert variant="destructive">
        <TriangleAlert />
        <AlertTitle>Download preview unavailable</AlertTitle>
        <AlertDescription>
          <p>{error.message}</p>
          {error.hint ? <p>{error.hint}</p> : null}
          <Button type="button" size="sm" variant="outline" className="mt-2" onClick={onRetry}>
            <RefreshCw data-icon="inline-start" />
            Retry preview
          </Button>
        </AlertDescription>
      </Alert>
    );
  }

  if (!summary) {
    return <p className="text-sm text-muted-foreground">Loading the official release preview…</p>;
  }

  return (
    <div className="space-y-2 rounded-lg border bg-muted/20 p-3">
      <div className="flex flex-wrap items-center gap-x-2 gap-y-1 text-sm">
        <span className="font-medium">{summary.title}</span>
        <span className="text-muted-foreground">{summary.meta}</span>
      </div>
      <div className="flex flex-wrap items-center gap-x-3 gap-y-1 text-xs text-muted-foreground">
        <span>{formatBytes(summary.size)}</span>
        <span className="font-mono">{summary.hash}</span>
        <span className="truncate">{summary.host}</span>
      </div>
      <Button
        type="button"
        size="sm"
        variant="outline"
        disabled={disabled}
        onClick={onDownload}
        aria-label={summary.downloadLabel}
      >
        <Download data-icon="inline-start" />
        {summary.downloadLabel}
      </Button>
    </div>
  );
}

interface ExistingItem {
  id: string;
  title: string;
  meta: string;
  path: string;
  selected: boolean;
}

function ExistingPanel({
  items,
  empty,
  disabled,
  onUse,
}: {
  items: ExistingItem[];
  empty: string;
  disabled: boolean;
  onUse: (path: string) => void;
}) {
  if (items.length === 0) {
    return <p className="text-sm text-muted-foreground">{empty}</p>;
  }
  return (
    <div className="space-y-2">
      {items.map((item) => (
        <div
          key={item.id}
          className="flex flex-wrap items-center gap-2 rounded-lg border bg-muted/20 px-3 py-2"
        >
          <div className="min-w-0 flex-1">
            <p className="text-sm font-medium">{item.title}</p>
            <p className="truncate font-mono text-xs text-muted-foreground" title={item.path}>
              {item.meta}
            </p>
          </div>
          {item.selected ? <Badge variant="secondary">Selected</Badge> : null}
          <Button
            type="button"
            size="sm"
            variant="outline"
            disabled={disabled || item.selected}
            onClick={() => onUse(item.path)}
            aria-label={`Use ${item.title}`}
          >
            Use
          </Button>
        </div>
      ))}
    </div>
  );
}

function ManualPanel({
  description,
  disabled,
  actions,
}: {
  description: string;
  disabled: boolean;
  actions: { label: string; ariaLabel: string; onClick: () => void }[];
}) {
  return (
    <div className="space-y-2 rounded-lg border bg-muted/20 p-3">
      <p className="text-sm leading-relaxed text-muted-foreground">{description}</p>
      <div className="flex flex-wrap gap-2">
        {actions.map((action) => (
          <Button
            key={action.ariaLabel}
            type="button"
            size="sm"
            variant="outline"
            disabled={disabled}
            onClick={action.onClick}
            aria-label={action.ariaLabel}
          >
            <FolderOpen data-icon="inline-start" />
            {action.label}
          </Button>
        ))}
      </div>
    </div>
  );
}

/**
 * Components step for the creation wizard. It reports the verified state of Java, client
 * assets and the official server JAR for the exact target, and only downloads after an
 * explicit button press. Manual paths are validated by the same backend facade; placeholder
 * textures stay an explicit, clearly marked choice.
 */
export function ProjectComponentsStep({
  targetKey,
  mode,
  profile,
  desktop,
  disabled = false,
  onStateChange,
}: ProjectComponentsStepProps) {
  const target = useMemo(() => (profile ? targetFor(profile) : null), [profile]);
  const vanilla = mode === "vanilla";
  // Java is requested for its status even in editing-only modes. The card explains that it is
  // optional there, so the local errors (not report.ready) decide whether the step can pass.
  const requirements = useMemo<ComponentRequirements>(
    () => ({ java: true, assets: true, server: vanilla, assetsPlaceholder: false }),
    [vanilla],
  );

  const [report, setReport] = useState<ComponentReport | null>(null);
  const [inventory, setInventory] = useState<ComponentInventory | null>(null);
  const [loading, setLoading] = useState(true);
  const [loadError, setLoadError] = useState<ErrorPresentation | null>(null);
  const [releases, setReleases] = useState<{
    java: RuntimeReleaseInfo | null;
    assets: ClientJarReleaseInfo | null;
    server: ServerReleaseInfo | null;
  }>({ java: null, assets: null, server: null });
  const [releaseErrors, setReleaseErrors] = useState<
    Partial<Record<ComponentKind, ErrorPresentation>>
  >({});
  const [previewAttempt, setPreviewAttempt] = useState(0);
  const [action, setAction] = useState<{
    kind: ComponentKind;
    phase: "install" | "relink" | "placeholder";
  } | null>(null);
  const [progress, setProgress] = useState<ComponentProgress | null>(null);
  const [actionError, setActionError] = useState<ErrorPresentation | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [javaMode, setJavaMode] = useState<ProvisionMode>("auto");
  const [assetsMode, setAssetsMode] = useState<AssetProvisionMode>("auto");
  const [serverMode, setServerMode] = useState<ProvisionMode>("auto");
  const [catalog, setCatalog] = useState<CatalogSnapshot | null>(null);
  const [catalogError, setCatalogError] = useState<string | null>(null);
  const [catalogAttempt, setCatalogAttempt] = useState(0);
  const catalogKeyRef = useRef<string | null>(null);

  const refreshStatus = useCallback(async (): Promise<void> => {
    if (!target) return;
    const [nextReport, nextInventory] = await Promise.all([
      componentStatus(target, requirements),
      componentInventory(),
    ]);
    setReport(nextReport);
    setInventory(nextInventory);
    setLoadError(null);
  }, [target, requirements]);

  useEffect(() => {
    if (!desktop || !target) return;
    let cancelled = false;
    void (async () => {
      try {
        await refreshStatus();
      } catch (error) {
        if (!cancelled) setLoadError(presentError(error));
      } finally {
        if (!cancelled) setLoading(false);
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [desktop, target, refreshStatus]);

  useEffect(() => {
    if (!desktop || !target) return;
    let cancelled = false;
    void (async () => {
      const [java, assets, server] = await Promise.allSettled([
        componentJavaRelease(target),
        componentAssetsRelease(target),
        vanilla ? componentServerRelease(target) : Promise.resolve(null),
      ]);
      if (cancelled) return;
      const next = {
        java: java.status === "fulfilled" ? java.value : null,
        assets: assets.status === "fulfilled" ? assets.value : null,
        server: server.status === "fulfilled" ? server.value : null,
      };
      const errors: Partial<Record<ComponentKind, ErrorPresentation>> = {};
      if (java.status === "rejected") errors.java = presentError(java.reason);
      if (assets.status === "rejected") errors.assets = presentError(assets.reason);
      if (server.status === "rejected") errors.server = presentError(server.reason);
      setReleases(next);
      setReleaseErrors(errors);
    })();
    return () => {
      cancelled = true;
    };
  }, [desktop, target, vanilla, previewAttempt]);

  useEffect(() => {
    if (!desktop) return;
    let disposed = false;
    const stops: Array<() => void> = [];
    const keep = (stop: () => void): void => {
      if (disposed) stop();
      else stops.push(stop);
    };
    void listenComponentProgress((next) => {
      if (!disposed) setProgress(next);
    })
      .then(keep)
      .catch(() => undefined);
    void listenComponentChanged(() => {
      if (disposed) return;
      void refreshStatus().catch((error) => {
        if (!disposed) setLoadError(presentError(error));
      });
    })
      .then(keep)
      .catch(() => undefined);
    void listenComponentError((event) => {
      if (!disposed) setActionError(presentError(event));
    })
      .then(keep)
      .catch(() => undefined);
    return () => {
      disposed = true;
      for (const stop of stops) stop();
    };
  }, [desktop, refreshStatus]);

  const assetsStatus = findStatus(report, "assets");
  const javaStatus = findStatus(report, "java");
  const serverStatus = findStatus(report, "server");
  const catalogKey =
    assetsStatus?.state === "ready" && assetsStatus.mode !== "placeholder"
      ? (assetsStatus.hash?.value ?? assetsStatus.id ?? null)
      : null;

  useEffect(() => {
    if (!desktop || !catalogKey) return;
    if (catalogKeyRef.current === catalogKey) return;
    catalogKeyRef.current = catalogKey;
    let cancelled = false;
    void loadAssetCatalog()
      .then((value) => {
        if (cancelled) return;
        setCatalog(value);
        setCatalogError(null);
      })
      .catch((error: unknown) => {
        if (cancelled) return;
        catalogKeyRef.current = null;
        setCatalogError(presentError(error).message);
      });
    return () => {
      cancelled = true;
    };
  }, [desktop, catalogKey, catalogAttempt]);

  const errors = useMemo(() => {
    const next: string[] = [];
    if (!desktop) {
      next.push("Component setup needs the desktop app.");
      return next;
    }
    if (!profile) {
      next.push("Choose a verified version before setting up components.");
      return next;
    }
    if (loadError) {
      next.push(`Retry the component check: ${loadError.message}`);
      return next;
    }
    if (loading || !report) {
      next.push("Wait for the component check to finish.");
      return next;
    }
    if (!assetsStatus) {
      next.push("Check Minecraft client assets for this project.");
    } else if (assetsStatus.mode === "placeholder") {
      // Explicit placeholder is allowed; the warning explains the missing catalog.
    } else if (assetsStatus.state !== "ready") {
      next.push(
        assetsStatus.error ??
          "Download or choose Minecraft client assets, or explicitly choose placeholder textures.",
      );
    }
    if (vanilla) {
      if (!javaStatus) {
        next.push("Check the Java runtime for this project.");
      } else if (javaStatus.state !== "ready") {
        next.push(javaStatus.error ?? "Set up a compatible Java runtime for the official server.");
      }
      if (!serverStatus) {
        next.push("Check the official server JAR for this project.");
      } else if (serverStatus.state !== "ready") {
        next.push(
          serverStatus.error ?? "Download or choose the official server JAR for this version.",
        );
      }
    }
    return next;
  }, [
    assetsStatus,
    desktop,
    javaStatus,
    loadError,
    loading,
    profile,
    report,
    serverStatus,
    vanilla,
  ]);

  const warnings = useMemo(() => {
    const next: string[] = [];
    if (assetsStatus?.mode === "placeholder") {
      next.push(
        "Placeholder textures are active; there is no verified block/item catalog for this project.",
      );
    }
    if (!vanilla && javaStatus && javaStatus.state !== "ready") {
      next.push(
        "Java is optional for editing; generation and version upgrades will need it later.",
      );
    }
    return next;
  }, [assetsStatus?.mode, javaStatus, vanilla]);

  const checking = desktop && profile !== null && (loading || (!report && !loadError));
  const ready = errors.length === 0;

  const locked = disabled || action !== null;

  const retryLoad = (): void => {
    setLoading(true);
    setLoadError(null);
    void refreshStatus()
      .catch((error) => setLoadError(presentError(error)))
      .finally(() => setLoading(false));
  };

  const retryPreview = (): void => {
    setPreviewAttempt((value) => value + 1);
  };

  const install = async (kind: ComponentKind): Promise<void> => {
    if (!target) return;
    const request = installRequest(kind, target, releases);
    if (!request) {
      setActionError(
        presentError({
          code: "not_ready",
          message: "The download preview is not available. Retry the preview and try again.",
        }),
      );
      return;
    }
    setAction({ kind, phase: "install" });
    setActionError(null);
    setNotice(null);
    setProgress(null);
    try {
      const resolved = await installComponent(request);
      setNotice(
        `${componentLabel(kind)} is installed and verified${resolved.version ? ` (${resolved.version})` : ""}.`,
      );
      await refreshStatus();
    } catch (error) {
      setActionError(presentError(error));
    } finally {
      setAction(null);
      setProgress(null);
    }
  };

  const cancelInstall = async (kind: ComponentKind): Promise<void> => {
    try {
      await cancelComponentInstall(kind);
      setNotice("Cancellation requested. Partial download files are removed by the backend.");
    } catch (error) {
      setActionError(presentError(error));
    }
  };

  const relink = async (kind: ComponentKind, path: string): Promise<void> => {
    if (!target) return;
    setAction({ kind, phase: "relink" });
    setActionError(null);
    setNotice(null);
    try {
      const status = await relinkComponent(kind, path, target);
      setNotice(
        `${componentLabel(kind)} selected and verified${status.version ? ` (${status.version})` : ""}.`,
      );
      await refreshStatus();
    } catch (error) {
      setActionError(presentError(error));
    } finally {
      setAction(null);
    }
  };

  const choosePath = async (
    kind: "java" | "assets-root" | "assets-file" | "server",
  ): Promise<void> => {
    try {
      const { open } = await import("@tauri-apps/plugin-dialog");
      const selected = await open({
        title:
          kind === "java"
            ? "Choose a Java executable"
            : kind === "server"
              ? "Choose the official Minecraft server JAR"
              : kind === "assets-file"
                ? "Choose a Minecraft client JAR"
                : "Choose a .minecraft folder or version folder",
        multiple: false,
        directory: kind === "assets-root",
        ...(kind === "java" ? { filters: [{ name: "Java executable", extensions: ["exe"] }] } : {}),
        ...(kind === "server" || kind === "assets-file"
          ? { filters: [{ name: "Java archive", extensions: ["jar"] }] }
          : {}),
      });
      if (typeof selected !== "string" || selected.length === 0) return;
      const componentKind: ComponentKind =
        kind === "java" ? "java" : kind === "server" ? "server" : "assets";
      await relink(componentKind, selected);
    } catch (error) {
      setActionError(presentError(error));
    }
  };

  const selectPlaceholder = async (): Promise<void> => {
    if (!target) return;
    setAction({ kind: "assets", phase: "placeholder" });
    setActionError(null);
    setNotice(null);
    try {
      await selectPlaceholderComponent(target);
      setAssetsMode("placeholder");
      setNotice("Placeholder textures selected. No client JAR or block/item catalog is claimed.");
      await refreshStatus();
    } catch (error) {
      setActionError(presentError(error));
    } finally {
      setAction(null);
    }
  };

  const javaRelease = releases.java;
  const assetsRelease = releases.assets;
  const serverRelease = releases.server;
  const javaExisting = useMemo<ExistingItem[]>(() => {
    if (!target || !inventory) return [];
    return inventory.java.runtimes
      .filter((runtime) => runtime.major === target.javaMajor)
      .map((runtime) => ({
        id: runtime.id,
        title: `Java ${runtime.version}`,
        meta: `${formatBytes(runtime.bytes)} · ${shortPath(runtime.javaPath)}`,
        path: runtime.javaPath,
        selected: javaStatus?.path === runtime.javaPath,
      }));
  }, [inventory, javaStatus?.path, target]);
  const serverExisting = useMemo<ExistingItem[]>(() => {
    if (!target || !inventory) return [];
    return inventory.servers.servers
      .filter((server) => server.version === target.minecraftVersion)
      .map((server) => ({
        id: server.version,
        title: `Minecraft ${server.version} server JAR`,
        meta: `${formatBytes(server.size)} · SHA-1 ${server.sha1.slice(0, 12)}…`,
        path: server.jarPath,
        selected: serverStatus?.path === server.jarPath,
      }));
  }, [inventory, serverStatus?.path, target]);

  const catalogForTarget =
    catalog &&
    target &&
    catalog.minecraftVersion === target.minecraftVersion &&
    (!catalog.clientJarSha1 ||
      !assetsStatus?.hash ||
      catalog.clientJarSha1 === assetsStatus.hash.value)
      ? catalog
      : null;

  const summary = useMemo<ComponentStepSummary | null>(() => {
    if (!report) return null;
    const item = (kind: ComponentKind): ComponentStepItem | null => {
      const status = findStatus(report, kind);
      if (!status) return null;
      return {
        kind,
        mode: status.mode ?? null,
        state: status.state,
        version: status.version ?? null,
        bytes: status.bytes ?? null,
        hash: status.hash?.value ?? null,
        path: status.path ?? null,
      };
    };
    const components = [javaStatus, assetsStatus, serverStatus].filter(
      (status): status is ComponentStatus => status !== null,
    );
    return {
      java: item("java"),
      assets: item("assets"),
      server: item("server"),
      catalog: catalogForTarget
        ? {
            id: catalogForTarget.catalogId,
            hash: catalogForTarget.catalogHash,
            blocks: catalogForTarget.counts.blocks,
            items: catalogForTarget.counts.items,
          }
        : null,
      totalBytes: components.reduce((total, status) => total + (status.bytes ?? 0), 0),
    };
  }, [assetsStatus, catalogForTarget, javaStatus, report, serverStatus]);

  useEffect(() => {
    onStateChange({ targetKey, checking, ready, errors, warnings, summary });
  }, [targetKey, checking, ready, errors, warnings, summary, onStateChange]);

  const javaSummary: ReleaseSummary | null = javaRelease
    ? {
        title: `Temurin ${javaRelease.version}`,
        meta: `Java ${javaRelease.major} · ${javaRelease.vendor} · ${javaRelease.os}/${javaRelease.arch}`,
        size: javaRelease.size,
        hash: `SHA-256 ${javaRelease.sha256.slice(0, 12)}…`,
        host: releaseHost(javaRelease.downloadUrl),
        downloadLabel: `Download Java ${javaRelease.major}`,
      }
    : null;
  const assetsSummary: ReleaseSummary | null = assetsRelease
    ? {
        title: `Minecraft ${assetsRelease.minecraftVersion} client JAR`,
        meta: assetsRelease.versionType,
        size: assetsRelease.size,
        hash: `SHA-1 ${assetsRelease.sha1.slice(0, 12)}…`,
        host: releaseHost(assetsRelease.downloadUrl),
        downloadLabel: "Download Minecraft client JAR",
      }
    : null;
  const serverSummary: ReleaseSummary | null = serverRelease
    ? {
        title: `Minecraft ${serverRelease.minecraftVersion} server JAR`,
        meta: `Java ${serverRelease.javaMajor}`,
        size: serverRelease.size,
        hash: `SHA-1 ${serverRelease.sha1.slice(0, 12)}…`,
        host: releaseHost(serverRelease.downloadUrl),
        downloadLabel: "Download official server JAR",
      }
    : null;

  return (
    <div className="space-y-4">
      <div className="flex flex-wrap items-center gap-2">
        <PackageCheck className="size-4" />
        <h3 className="text-sm font-semibold">Project components</h3>
        {target ? (
          <Badge variant="outline" className="font-mono">
            Minecraft {target.minecraftVersion} · DV {target.dataVersion} · Java {target.javaMajor}
          </Badge>
        ) : null}
        <Button
          type="button"
          variant="ghost"
          size="icon-sm"
          className="ml-auto"
          onClick={retryLoad}
          disabled={!desktop || !target || loading}
          aria-label="Refresh component status"
        >
          {loading ? <Loader2 className="animate-spin" /> : <RefreshCw />}
        </Button>
      </div>
      <p className="text-sm leading-relaxed text-muted-foreground">
        Components are shared between projects and cached locally. Nothing is downloaded until you
        press a download button; manual files are validated in place and never copied or edited.
      </p>

      {!desktop ? (
        <Alert>
          <TriangleAlert />
          <AlertTitle>Desktop core required</AlertTitle>
          <AlertDescription>
            Component checks, downloads and manual verification need the desktop app.
          </AlertDescription>
        </Alert>
      ) : null}

      {desktop && !profile ? (
        <Alert variant="destructive">
          <TriangleAlert />
          <AlertTitle>Choose a verified version first</AlertTitle>
          <AlertDescription>
            Components are checked against the exact project version. Go back to the Version step
            and choose a version with a verified profile.
          </AlertDescription>
        </Alert>
      ) : null}

      {desktop && profile && loadError ? (
        <Alert variant="destructive">
          <TriangleAlert />
          <AlertTitle>Component check unavailable</AlertTitle>
          <AlertDescription>
            <p>{loadError.message}</p>
            {loadError.hint ? <p>{loadError.hint}</p> : null}
            <Button type="button" size="sm" variant="outline" className="mt-2" onClick={retryLoad}>
              <RefreshCw data-icon="inline-start" />
              Retry
            </Button>
          </AlertDescription>
        </Alert>
      ) : null}

      {desktop && profile && loading && !report ? (
        <Card className="gap-0 py-0">
          <CardContent className="flex items-center gap-2 p-5 text-sm text-muted-foreground">
            <Loader2 className="size-4 animate-spin" />
            Checking Java, client assets and the official server for this version…
          </CardContent>
        </Card>
      ) : null}

      {report ? (
        <>
          <ComponentCard
            icon={<Coffee className="size-4" />}
            title={`Java ${target?.javaMajor ?? ""}`}
            description={
              vanilla
                ? "Required to run the official server for Vanilla World generation."
                : "Optional for editing. Generation and version upgrades need a compatible runtime later."
            }
            status={javaStatus}
          >
            <ToggleGroup
              variant="outline"
              size="sm"
              value={[javaMode]}
              disabled={locked}
              className="grid w-full grid-cols-1 sm:grid-cols-3"
              onValueChange={(value) => {
                const next = value[0];
                if (next === "auto" || next === "existing" || next === "manual") setJavaMode(next);
              }}
            >
              <ToggleGroupItem value="auto">Auto (recommended)</ToggleGroupItem>
              <ToggleGroupItem value="existing">Use existing</ToggleGroupItem>
              <ToggleGroupItem value="manual">Manual path</ToggleGroupItem>
            </ToggleGroup>
            {javaMode === "auto" ? (
              <ReleasePanel
                summary={javaSummary}
                error={releaseErrors.java ?? null}
                busy={action?.kind === "java" && action.phase === "install"}
                progress={progress?.kind === "java" ? progress : null}
                readyManaged={javaStatus?.state === "ready" && javaStatus.mode === "managed"}
                disabled={locked}
                onDownload={() => void install("java")}
                onCancel={() => void cancelInstall("java")}
                onRetry={retryPreview}
              />
            ) : null}
            {javaMode === "existing" ? (
              <ExistingPanel
                items={javaExisting}
                empty={`No managed Java runtime matches Java ${target?.javaMajor ?? ""}. Use Auto to download one or choose a manual path.`}
                disabled={locked}
                onUse={(path) => void relink("java", path)}
              />
            ) : null}
            {javaMode === "manual" ? (
              <ManualPanel
                description="Choose a java.exe. FuryBuilder probes its major version, architecture and vendor; newer Java is allowed with a warning, older or 32-bit Java is blocked."
                disabled={locked}
                actions={[
                  {
                    label: "Choose java.exe",
                    ariaLabel: "Choose Java executable",
                    onClick: () => void choosePath("java"),
                  },
                ]}
              />
            ) : null}
          </ComponentCard>

          <ComponentCard
            icon={<HardDrive className="size-4" />}
            title="Minecraft client assets"
            description="Blocks, items, textures and the palette catalog for this exact version."
            status={assetsStatus}
          >
            <ToggleGroup
              variant="outline"
              size="sm"
              value={[assetsMode]}
              disabled={locked}
              className="grid w-full grid-cols-1 sm:grid-cols-3"
              onValueChange={(value) => {
                const next = value[0];
                if (next === "auto" || next === "manual" || next === "placeholder") {
                  setAssetsMode(next);
                }
              }}
            >
              <ToggleGroupItem value="auto">Auto download</ToggleGroupItem>
              <ToggleGroupItem value="manual">Manual</ToggleGroupItem>
              <ToggleGroupItem value="placeholder">Placeholder</ToggleGroupItem>
            </ToggleGroup>
            {assetsMode === "auto" ? (
              <ReleasePanel
                summary={assetsSummary}
                error={releaseErrors.assets ?? null}
                busy={action?.kind === "assets" && action.phase === "install"}
                progress={progress?.kind === "assets" ? progress : null}
                readyManaged={assetsStatus?.state === "ready" && assetsStatus.mode === "managed"}
                disabled={locked}
                onDownload={() => void install("assets")}
                onCancel={() => void cancelInstall("assets")}
                onRetry={retryPreview}
              />
            ) : null}
            {assetsMode === "manual" ? (
              <ManualPanel
                description="Choose a .minecraft folder, a versions/<id> folder or a client.jar. The selected file stays untouched; a declared version or checksum mismatch is refused."
                disabled={locked}
                actions={[
                  {
                    label: "Choose .minecraft",
                    ariaLabel: "Choose .minecraft folder",
                    onClick: () => void choosePath("assets-root"),
                  },
                  {
                    label: "Choose client.jar",
                    ariaLabel: "Choose client JAR",
                    onClick: () => void choosePath("assets-file"),
                  },
                ]}
              />
            ) : null}
            {assetsMode === "placeholder" ? (
              <div
                className={cn(
                  "space-y-2 rounded-lg border p-3",
                  "border-status-warn/40 bg-status-warn/5",
                )}
              >
                <p className="text-sm leading-relaxed">
                  Placeholder textures are an explicit mode, not a silent fallback. The palette will
                  not claim a Minecraft catalog and block data stays unchanged.
                </p>
                <Button
                  type="button"
                  size="sm"
                  variant="outline"
                  disabled={locked || assetsStatus?.mode === "placeholder"}
                  onClick={() => void selectPlaceholder()}
                  aria-label="Use placeholder textures"
                >
                  <ShieldCheck data-icon="inline-start" />
                  {assetsStatus?.mode === "placeholder"
                    ? "Placeholder active"
                    : "Use placeholder textures"}
                </Button>
              </div>
            ) : null}

            {assetsStatus?.mode === "placeholder" ? null : catalogForTarget ? (
              <div className="space-y-1 rounded-lg border bg-muted/20 p-3">
                <div className="flex flex-wrap items-center gap-2">
                  <BookOpen className="size-4" />
                  <span className="text-sm font-medium">Block and item catalog</span>
                  <Badge variant="secondary">
                    {catalogForTarget.counts.blocks} blocks · {catalogForTarget.counts.items} items
                  </Badge>
                  <span className="ml-auto truncate font-mono text-xs text-muted-foreground">
                    {catalogForTarget.catalogId}
                  </span>
                </div>
                <p className="text-xs leading-relaxed text-muted-foreground">
                  {catalogForTarget.counts.blocksWithItems} blocks with items ·{" "}
                  {catalogForTarget.counts.itemOnly} item-only · {catalogForTarget.counts.textures}{" "}
                  textures · hash{" "}
                  <span className="font-mono">{catalogForTarget.catalogHash.slice(0, 12)}…</span>
                </p>
              </div>
            ) : assetsStatus?.state === "ready" ? (
              <p className="flex items-center gap-2 text-sm text-muted-foreground">
                <Loader2 className="size-4 animate-spin" />
                Building the block/item catalog from the verified client JAR…
              </p>
            ) : null}

            {catalogError ? (
              <Alert>
                <TriangleAlert />
                <AlertTitle>Catalog unavailable</AlertTitle>
                <AlertDescription>
                  <p>{catalogError}</p>
                  <Button
                    type="button"
                    size="sm"
                    variant="outline"
                    className="mt-2"
                    onClick={() => {
                      catalogKeyRef.current = null;
                      setCatalogAttempt((value) => value + 1);
                    }}
                  >
                    <RefreshCw data-icon="inline-start" />
                    Retry catalog
                  </Button>
                </AlertDescription>
              </Alert>
            ) : null}
          </ComponentCard>

          {vanilla ? (
            <ComponentCard
              icon={<Server className="size-4" />}
              title="Official server JAR"
              description="Required for Vanilla World generation. The server runs only after explicit EULA and per-run consent in the Source step."
              status={serverStatus}
            >
              <ToggleGroup
                variant="outline"
                size="sm"
                value={[serverMode]}
                disabled={locked}
                className="grid w-full grid-cols-1 sm:grid-cols-3"
                onValueChange={(value) => {
                  const next = value[0];
                  if (next === "auto" || next === "existing" || next === "manual") {
                    setServerMode(next);
                  }
                }}
              >
                <ToggleGroupItem value="auto">Auto download</ToggleGroupItem>
                <ToggleGroupItem value="existing">Use existing</ToggleGroupItem>
                <ToggleGroupItem value="manual">Manual JAR</ToggleGroupItem>
              </ToggleGroup>
              {serverMode === "auto" ? (
                <ReleasePanel
                  summary={serverSummary}
                  error={releaseErrors.server ?? null}
                  busy={action?.kind === "server" && action.phase === "install"}
                  progress={progress?.kind === "server" ? progress : null}
                  readyManaged={serverStatus?.state === "ready" && serverStatus.mode === "managed"}
                  disabled={locked}
                  onDownload={() => void install("server")}
                  onCancel={() => void cancelInstall("server")}
                  onRetry={retryPreview}
                />
              ) : null}
              {serverMode === "existing" ? (
                <ExistingPanel
                  items={serverExisting}
                  empty="No managed server JAR matches this version. Use Auto to download one or choose a manual JAR."
                  disabled={locked}
                  onUse={(path) => void relink("server", path)}
                />
              ) : null}
              {serverMode === "manual" ? (
                <ManualPanel
                  description="Choose the official server.jar for this exact version. FuryBuilder checks the internal version, DataVersion, Java major and entry point before recording it."
                  disabled={locked}
                  actions={[
                    {
                      label: "Choose server.jar",
                      ariaLabel: "Choose server JAR",
                      onClick: () => void choosePath("server"),
                    },
                  ]}
                />
              ) : null}
            </ComponentCard>
          ) : null}

          <Alert variant={ready ? "default" : "destructive"}>
            {ready ? <CheckCircle2 /> : <TriangleAlert />}
            <AlertTitle>
              {ready ? "Required components are ready" : "Components still need attention"}
            </AlertTitle>
            <AlertDescription>
              {ready ? (
                <p>
                  Every required choice is set for this exact version. Nothing is downloaded or
                  started until you ask for it.
                </p>
              ) : (
                <ul className="list-disc space-y-1 pl-4">
                  {errors.map((error) => (
                    <li key={error}>{error}</li>
                  ))}
                </ul>
              )}
              {warnings.length > 0 ? (
                <ul className="mt-2 list-disc space-y-1 pl-4 text-muted-foreground">
                  {warnings.map((warning) => (
                    <li key={warning}>{warning}</li>
                  ))}
                </ul>
              ) : null}
            </AlertDescription>
          </Alert>
        </>
      ) : null}

      {actionError ? (
        <Alert variant="destructive">
          <TriangleAlert />
          <AlertTitle>Component action failed</AlertTitle>
          <AlertDescription>
            <p>{actionError.message}</p>
            {actionError.hint ? <p>{actionError.hint}</p> : null}
            <Button
              type="button"
              size="sm"
              variant="outline"
              className="mt-2"
              onClick={() => setActionError(null)}
            >
              Dismiss
            </Button>
          </AlertDescription>
        </Alert>
      ) : null}

      {notice ? (
        <Alert>
          <CheckCircle2 />
          <AlertTitle>Component update</AlertTitle>
          <AlertDescription>{notice}</AlertDescription>
        </Alert>
      ) : null}
    </div>
  );
}
