import { useEffect, useState } from "react";
import {
  ArrowDown,
  ArrowUp,
  CheckCircle2,
  FolderOpen,
  HardDrive,
  Loader2,
  RefreshCw,
  ShieldAlert,
  Trash2,
  X,
} from "lucide-react";
import { Button } from "@/components/ui/button";
import type {
  AssetDiscoveryReport,
  AssetPathReport,
  AssetSourceKind,
  InstallationReport,
  SavedAssetSource,
  VersionReport,
} from "@/assets/types";
import {
  clearAssetBundleCache,
  clearMinecraftAssets,
  discoverMinecraftAssets,
  inspectAssetPath,
  saveMinecraftAssets,
  saveMinecraftPackStack,
  savedMinecraftAssets,
} from "@/lib/assets-api";
import { cn } from "@/lib/utils";

type BusyAction =
  | "load"
  | "detect"
  | "root"
  | "jar"
  | "pack-zip"
  | "pack-folder"
  | "pack"
  | "cache"
  | "clear"
  | null;

export function MinecraftAssetsSection() {
  const [discovery, setDiscovery] = useState<AssetDiscoveryReport | null>(null);
  const [saved, setSaved] = useState<SavedAssetSource | null>(null);
  const [inspection, setInspection] = useState<AssetPathReport | null>(null);
  const [busy, setBusy] = useState<BusyAction>("load");
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    void (async () => {
      try {
        const [savedSource, found] = await Promise.all([
          savedMinecraftAssets(),
          discoverMinecraftAssets(),
        ]);
        if (cancelled) return;
        setSaved(savedSource);
        setDiscovery(found);
        setInspection(savedSource.report);
      } catch (cause) {
        if (!cancelled) {
          setError(cause instanceof Error ? cause.message : String(cause));
        }
      } finally {
        if (!cancelled) setBusy(null);
      }
    })();
    return () => {
      cancelled = true;
    };
  }, []);

  async function run<T>(action: BusyAction, operation: () => Promise<T>): Promise<T | null> {
    setBusy(action);
    setError(null);
    setNotice(null);
    try {
      return await operation();
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : String(cause));
      return null;
    } finally {
      setBusy(null);
    }
  }

  async function detect() {
    const found = await run("detect", discoverMinecraftAssets);
    if (found) {
      setDiscovery(found);
      setNotice(
        found.installations.length > 0
          ? `Detected ${found.installations.length} launcher root(s).`
          : "No Minecraft Java installation was detected automatically.",
      );
    }
  }

  async function chooseFolder(kind: AssetSourceKind, action: BusyAction) {
    const selected = await selectPath(
      true,
      kind === "minecraft_root" ? "Select .minecraft folder" : "Select resource pack folder",
    );
    if (!selected) return;
    await acceptSelection(kind, selected, action);
  }

  async function chooseFile(kind: AssetSourceKind, action: BusyAction) {
    const selected = await selectPath(
      false,
      kind === "client_jar" ? "Select Minecraft client.jar" : "Select resource pack .zip",
      kind === "client_jar"
        ? [{ name: "Minecraft client JAR", extensions: ["jar"] }]
        : [{ name: "Resource pack", extensions: ["zip"] }],
    );
    if (!selected) return;
    await acceptSelection(kind, selected, action);
  }

  async function acceptSelection(kind: AssetSourceKind, path: string, action: BusyAction) {
    setBusy(action);
    setError(null);
    setNotice(null);
    try {
      const report = await inspectAssetPath(path);
      setInspection(report);
      if (!report.valid) {
        setError(
          report.errors[0] ??
            "The selected path is not a compatible Minecraft 1.21.4 asset source.",
        );
        return;
      }
      const next = await saveMinecraftAssets(kind, path);
      setSaved(next);
      if (next.report) setInspection(next.report);
      setNotice("Minecraft asset source saved.");
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : String(cause));
    } finally {
      setBusy(null);
    }
  }

  async function forget() {
    const next = await run("clear", clearMinecraftAssets);
    if (next) {
      setSaved(next);
      setInspection(null);
      setNotice("Minecraft asset source cleared. Placeholder textures stay available.");
    }
  }

  async function clearCache() {
    setBusy("cache");
    setError(null);
    setNotice(null);
    try {
      await clearAssetBundleCache();
      setNotice(
        "Derived asset cache cleared. The viewport rebuilds models and textures from the selected source on its next load.",
      );
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : String(cause));
    } finally {
      setBusy(null);
    }
  }

  async function writePacks(paths: string[], message: string) {
    const next = await run("pack", () => saveMinecraftPackStack(paths));
    if (next) {
      setSaved(next);
      setNotice(message);
    }
  }

  async function addPack(kind: "zip" | "folder") {
    const selected = await selectPath(
      kind === "folder",
      kind === "zip" ? "Select resource pack .zip" : "Select resource pack folder",
      kind === "zip" ? [{ name: "Resource pack", extensions: ["zip"] }] : undefined,
    );
    if (!selected) return;
    setBusy("pack");
    setError(null);
    setNotice(null);
    try {
      const report = await inspectAssetPath(selected);
      if (!report.valid || report.kind !== "resource_pack") {
        setError(
          report.errors[0] ?? "The selection is not a compatible Minecraft 1.21.4 resource pack.",
        );
        return;
      }
      const current = saved?.packs.map((pack) => pack.path) ?? [];
      if (current.some((path) => path.toLowerCase() === selected.toLowerCase())) {
        setError("That resource pack is already in the stack.");
        return;
      }
      const next = await saveMinecraftPackStack([selected, ...current]);
      setSaved(next);
      setNotice("Resource pack added at the highest priority.");
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : String(cause));
    } finally {
      setBusy(null);
    }
  }

  async function removePack(path: string) {
    const paths = (saved?.packs ?? [])
      .filter((pack) => pack.path !== path)
      .map((pack) => pack.path);
    await writePacks(paths, "Resource pack removed.");
  }

  async function movePack(index: number, offset: -1 | 1) {
    const paths = (saved?.packs ?? []).map((pack) => pack.path);
    const target = index + offset;
    if (target < 0 || target >= paths.length) return;
    const swapped = [...paths];
    [swapped[index], swapped[target]] = [swapped[target]!, swapped[index]!];
    await writePacks(swapped, "Resource pack priority updated.");
  }

  const packs = saved?.packs ?? [];

  const targetVersion = discovery
    ? `Minecraft ${discovery.targetMinecraftVersion} · DV ${discovery.targetDataVersion} · pack format ${discovery.targetResourcePackFormat}`
    : "Minecraft 1.21.4 · DV 4189 · pack format 46";

  return (
    <section className="min-w-0 space-y-2">
      <div className="flex items-center justify-between gap-2">
        <h3 className="text-[11px] font-semibold tracking-wide uppercase">Minecraft assets</h3>
        <span className="rounded bg-muted px-1.5 py-0.5 font-mono text-[9px] text-muted-foreground">
          {targetVersion}
        </span>
      </div>

      <p className="text-[10px] text-muted-foreground">
        Read-only discovery of an installed Java Edition. FuryBuilder never downloads, extracts or
        redistributes Mojang files; Empty Project keeps using placeholder textures when nothing is
        found.
      </p>

      {busy === "load" ? (
        <div className="flex items-center gap-1.5 text-[10px] text-muted-foreground">
          <Loader2 className="size-3 animate-spin" /> Scanning launcher folders…
        </div>
      ) : null}

      {saved?.source ? (
        <div className="space-y-1 rounded border border-border bg-card/60 p-2 text-[10px]">
          <div className="flex flex-wrap items-center gap-1.5">
            <span className="rounded bg-status-ok/15 px-1 py-0.5 text-[8px] font-semibold uppercase text-status-ok">
              selected
            </span>
            <span className="font-mono">{saved.source.kind.replaceAll("_", " ")}</span>
          </div>
          <div className="truncate font-mono text-muted-foreground" title={saved.source.path}>
            {compactPath(saved.source.path)}
          </div>
          {saved.report && !saved.report.valid ? (
            <div className="text-status-error">
              The saved path is no longer valid: {saved.report.errors[0]}
            </div>
          ) : null}
          {saved.loadWarning ? (
            <div className="text-status-warn">Settings warning: {saved.loadWarning}</div>
          ) : null}
        </div>
      ) : (
        <div className="rounded border border-border bg-background/40 p-2 text-[10px] text-muted-foreground">
          No Minecraft asset source is selected. Placeholder textures remain active.
        </div>
      )}

      <div className="space-y-1.5 rounded border border-border bg-card/40 p-2 text-[10px]">
        <div className="flex flex-wrap items-center justify-between gap-2">
          <span className="text-[10px] font-semibold tracking-wide uppercase">
            Resource pack stack
          </span>
          <span className="font-mono text-[9px] text-muted-foreground">
            {packs.length}/16 · highest first
          </span>
        </div>
        {packs.length === 0 ? (
          <p className="text-[10px] text-muted-foreground">
            No user packs. Verified assets from the selected source are used directly.
          </p>
        ) : (
          <div className="space-y-1">
            {packs.map((pack, index) => (
              <div
                key={pack.path}
                className="rounded border border-border/70 bg-background/40 p-1.5"
              >
                <div className="flex flex-wrap items-center gap-1.5">
                  <span className="font-mono text-[9px] text-muted-foreground">#{index + 1}</span>
                  <span className="truncate font-medium" title={pack.name}>
                    {pack.name}
                  </span>
                  <span
                    className={cn(
                      "ml-auto rounded px-1 py-0.5 font-mono text-[8px]",
                      pack.status === "compatible"
                        ? "bg-status-ok/15 text-status-ok"
                        : "bg-status-warn/15 text-status-warn",
                    )}
                  >
                    format {pack.packFormat ?? "?"} · {pack.status.replaceAll("_", " ")}
                  </span>
                  <Button
                    variant="ghost"
                    size="icon"
                    className="size-6"
                    aria-label={`Move ${pack.name} up`}
                    disabled={index === 0 || busy !== null}
                    onClick={() => void movePack(index, -1)}
                  >
                    <ArrowUp />
                  </Button>
                  <Button
                    variant="ghost"
                    size="icon"
                    className="size-6"
                    aria-label={`Move ${pack.name} down`}
                    disabled={index === packs.length - 1 || busy !== null}
                    onClick={() => void movePack(index, 1)}
                  >
                    <ArrowDown />
                  </Button>
                  <Button
                    variant="ghost"
                    size="icon"
                    className="size-6 text-status-warn"
                    aria-label={`Remove ${pack.name}`}
                    disabled={busy !== null}
                    onClick={() => void removePack(pack.path)}
                  >
                    <Trash2 />
                  </Button>
                </div>
                <div
                  className="truncate font-mono text-[9px] text-muted-foreground"
                  title={pack.path}
                >
                  {compactPath(pack.path)}
                </div>
                {pack.errors.slice(0, 2).map((message) => (
                  <div key={message} className="text-[9px] text-status-warn">
                    {message}
                  </div>
                ))}
              </div>
            ))}
          </div>
        )}
        <div className="flex flex-wrap gap-1.5">
          <Button
            variant="outline"
            size="sm"
            onClick={() => void addPack("zip")}
            disabled={busy !== null}
          >
            {busy === "pack" ? <Loader2 className="animate-spin" /> : <FolderOpen />} Add pack .zip
          </Button>
          <Button
            variant="outline"
            size="sm"
            onClick={() => void addPack("folder")}
            disabled={busy !== null}
          >
            {busy === "pack" ? <Loader2 className="animate-spin" /> : <FolderOpen />} Add pack
            folder
          </Button>
          {packs.length > 0 ? (
            <Button
              variant="ghost"
              size="sm"
              onClick={() => void writePacks([], "Resource pack stack cleared.")}
              disabled={busy !== null}
            >
              <X /> Clear stack
            </Button>
          ) : null}
        </div>
        <p className="text-[9px] text-muted-foreground">
          Packs override blockstates, models and textures by path. Files are read in memory;
          traversal, symlinks and wrong pack formats are refused.
        </p>
      </div>

      {discovery ? (
        <div className="space-y-1.5">
          {discovery.installations.length === 0 ? (
            <div className="space-y-1 rounded border border-status-warn/30 bg-status-warn/5 p-2 text-[10px] text-status-warn">
              <div className="font-medium">Automatic detection found no Java Edition.</div>
              {discovery.problems.map((problem) => (
                <div key={problem}>{problem}</div>
              ))}
              <div className="text-muted-foreground">{discovery.manualHint}</div>
            </div>
          ) : (
            discovery.installations.map((installation) => (
              <InstallationCard
                key={`${installation.root}-${installation.source}`}
                installation={installation}
                target={discovery.targetMinecraftVersion}
              />
            ))
          )}
        </div>
      ) : null}

      {inspection ? <InspectionCard report={inspection} /> : null}

      {error ? (
        <div className="rounded border border-status-error/30 bg-status-error/5 p-2 text-[10px] text-status-error">
          {error}
        </div>
      ) : null}
      {notice ? (
        <div className="rounded border border-status-ok/30 bg-status-ok/5 p-2 text-[10px] text-status-ok">
          {notice}
        </div>
      ) : null}

      <div className="flex flex-wrap gap-1.5">
        <Button variant="outline" size="sm" onClick={() => void detect()} disabled={busy !== null}>
          {busy === "detect" ? <Loader2 className="animate-spin" /> : <RefreshCw />} Detect again
        </Button>
        <Button
          variant="outline"
          size="sm"
          onClick={() => void chooseFolder("minecraft_root", "root")}
          disabled={busy !== null}
        >
          {busy === "root" ? <Loader2 className="animate-spin" /> : <FolderOpen />} .minecraft
        </Button>
        <Button
          variant="outline"
          size="sm"
          onClick={() => void chooseFile("client_jar", "jar")}
          disabled={busy !== null}
        >
          {busy === "jar" ? <Loader2 className="animate-spin" /> : <HardDrive />} client.jar
        </Button>
        <Button
          variant="outline"
          size="sm"
          onClick={() => void chooseFile("resource_pack", "pack-zip")}
          disabled={busy !== null}
        >
          {busy === "pack-zip" ? <Loader2 className="animate-spin" /> : <FolderOpen />} Pack .zip
        </Button>
        <Button
          variant="outline"
          size="sm"
          onClick={() => void chooseFolder("resource_pack", "pack-folder")}
          disabled={busy !== null}
        >
          {busy === "pack-folder" ? <Loader2 className="animate-spin" /> : <FolderOpen />} Pack
          folder
        </Button>
        {saved?.source ? (
          <Button variant="ghost" size="sm" onClick={() => void forget()} disabled={busy !== null}>
            {busy === "clear" ? <Loader2 className="animate-spin" /> : <X />} Forget
          </Button>
        ) : null}
        <Button
          variant="ghost"
          size="sm"
          onClick={() => void clearCache()}
          disabled={busy !== null}
          aria-label="Clear derived asset cache"
        >
          {busy === "cache" ? <Loader2 className="animate-spin" /> : <Trash2 />} Clear asset cache
        </Button>
      </div>

      <p className="text-[9px] text-muted-foreground">
        Derived model/texture bundles are cached in memory so repeated viewport loads stay fast.
        Clear the asset cache after replacing a JAR or pack outside FuryBuilder; world data is never
        touched. JARs are pinned to the client SHA-1 declared by the launcher metadata. Archive
        entries are validated in memory; traversal, symlinks and oversized packs are refused.
        Verified packs feed the blockstate/model/texture renderer; malformed assets fall back to the
        missing checker without changing block data.
      </p>
    </section>
  );
}

function InstallationCard({
  installation,
  target,
}: {
  installation: InstallationReport;
  target: string;
}) {
  const compatible = installation.versions.filter(
    (version) => version.status === "compatible" || version.status === "compatible_unverified",
  );
  return (
    <div className="space-y-1 rounded border border-border bg-card/60 p-2 text-[10px]">
      <div className="flex flex-wrap items-center gap-1.5">
        <span
          className={cn(
            "rounded px-1 py-0.5 text-[8px] font-semibold uppercase",
            compatible.length > 0
              ? "bg-status-ok/15 text-status-ok"
              : "bg-status-warn/15 text-status-warn",
          )}
        >
          {compatible.length > 0 ? "target found" : "no target"}
        </span>
        <span className="font-mono text-muted-foreground">
          {installation.source.replaceAll("_", " ")}
        </span>
        <span className="truncate font-mono" title={installation.root}>
          {compactPath(installation.root, 48)}
        </span>
      </div>
      <div className="flex flex-wrap gap-1 text-[9px] text-muted-foreground">
        <span>{installation.versions.length} version(s)</span>
        <span>·</span>
        <span>{installation.assetIndexes} asset index(es)</span>
        <span>·</span>
        <span>{installation.resourcePacks.length} resource pack(s)</span>
        {installation.compatibleVersions.length > 0 ? (
          <>
            <span>·</span>
            <span className="text-status-ok">{installation.compatibleVersions.join(", ")}</span>
          </>
        ) : null}
      </div>
      {compatible.length > 0 ? (
        compatible
          .slice(0, 3)
          .map((version) => <VersionLine key={version.id} version={version} target={target} />)
      ) : (
        <div className="text-status-warn">
          No verified {target} client. {installation.warnings[0] ?? "Choose a client.jar manually."}
        </div>
      )}
    </div>
  );
}

function VersionLine({ version, target }: { version: VersionReport; target: string }) {
  return (
    <div className="flex flex-wrap items-center gap-1.5 text-[9px]">
      <CheckCircle2 className="size-3 text-status-ok" />
      <span className="font-mono">{version.id}</span>
      <span className="text-muted-foreground">DV {version.worldVersion ?? "?"}</span>
      <span className="text-muted-foreground">
        Java {version.javaVersionFromJar ?? version.javaMajor ?? "?"}
      </span>
      <span className="text-status-ok">
        {version.checksumVerified ? "SHA-1 verified" : "checksum not declared"}
      </span>
      {version.id !== target ? (
        <span className="text-status-warn">not the target version</span>
      ) : null}
    </div>
  );
}

function InspectionCard({ report }: { report: AssetPathReport }) {
  return (
    <div className="space-y-1 rounded border border-border bg-background/40 p-2 text-[10px]">
      <div className="flex flex-wrap items-center gap-1.5">
        <span
          className={cn(
            "rounded px-1 py-0.5 text-[8px] font-semibold uppercase",
            report.valid
              ? "bg-status-ok/15 text-status-ok"
              : "bg-status-error/15 text-status-error",
          )}
        >
          {report.valid ? "usable" : "refused"}
        </span>
        <span className="font-mono">{report.kind.replaceAll("_", " ")}</span>
        <span className="truncate text-muted-foreground" title={report.path}>
          {compactPath(report.path, 46)}
        </span>
      </div>
      {report.version ? (
        <div className="flex flex-wrap items-center gap-1.5 text-[9px]">
          {report.version.checksumVerified ? (
            <CheckCircle2 className="size-3 text-status-ok" />
          ) : (
            <ShieldAlert className="size-3 text-status-warn" />
          )}
          <span className="font-mono">{report.version.id}</span>
          <span className="text-muted-foreground">
            {report.version.status.replaceAll("_", " ")}
          </span>
          <span className="text-muted-foreground">DV {report.version.worldVersion ?? "?"}</span>
          <span className="text-muted-foreground">
            pack {report.version.resourcePackFormat ?? "?"}
          </span>
        </div>
      ) : null}
      {report.resourcePack ? (
        <div className="flex flex-wrap items-center gap-1.5 text-[9px]">
          <span className="font-mono">{report.resourcePack.name}</span>
          <span className="text-muted-foreground">
            {report.resourcePack.status.replaceAll("_", " ")}
          </span>
          <span className="text-muted-foreground">
            format {report.resourcePack.packFormat ?? report.resourcePack.supportedFormats ?? "?"}
          </span>
          <span className="text-muted-foreground">{report.resourcePack.entryCount} entries</span>
        </div>
      ) : null}
      {report.errors.slice(0, 3).map((error) => (
        <div key={error} className="text-status-error">
          {error}
        </div>
      ))}
      {report.warnings.slice(0, 3).map((warning) => (
        <div key={warning} className="text-status-warn">
          {warning}
        </div>
      ))}
    </div>
  );
}

async function selectPath(
  directory: boolean,
  title: string,
  filters?: Array<{ name: string; extensions: string[] }>,
): Promise<string | null> {
  try {
    const { open } = await import("@tauri-apps/plugin-dialog");
    const options = {
      directory,
      multiple: false as const,
      title,
      ...(filters ? { filters } : {}),
    };
    const selected = await open(options);
    return typeof selected === "string" ? selected : null;
  } catch (cause) {
    throw new Error(
      cause instanceof Error
        ? cause.message
        : "The native file picker is only available in the desktop app.",
    );
  }
}

function compactPath(path: string, max = 58): string {
  if (path.length <= max) return path;
  const head = Math.max(12, Math.floor(max * 0.3));
  const tail = Math.max(18, max - head - 1);
  return `${path.slice(0, head)}…${path.slice(-tail)}`;
}
