import { useMemo, useState } from "react";
import { CheckCircle2, Info, Loader2, RefreshCw, Search, TriangleAlert } from "lucide-react";
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent } from "@/components/ui/card";
import { Checkbox } from "@/components/ui/checkbox";
import { Field, FieldDescription, FieldLabel } from "@/components/ui/field";
import { Input } from "@/components/ui/input";
import { ScrollArea } from "@/components/ui/scroll-area";
import type { ErrorPresentation } from "@/lib/error-messages";
import {
  SUPPORT_LEVEL_LABELS,
  supportLevelLabel,
  versionSeries,
  type SupportLevel,
  type VersionCatalogEntry,
  type VersionCatalogReport,
  type VersionProfile,
} from "@/project/version-types";

const SUPPORT_BADGE_VARIANT: Record<
  SupportLevel,
  "default" | "secondary" | "destructive" | "outline"
> = {
  full: "default",
  "read-write-subset": "secondary",
  "read-only": "secondary",
  convert: "secondary",
  "out-of-scope": "destructive",
};

const LAYOUT_LABELS: Record<string, string> = {
  "classic-anvil": "Classic Anvil",
  "split-anvil": "Split Anvil",
  "mc-region": "McRegion",
  unknown: "Unknown layout",
};

function selectedEntry(
  catalog: VersionCatalogReport | null,
  version: string,
): VersionCatalogEntry | null {
  return catalog?.versions.find((entry) => entry.id === version) ?? null;
}

function supportNotice(profile: VersionProfile) {
  switch (profile.capabilities.supportLevel) {
    case "full":
      return null;
    case "read-write-subset":
      return {
        variant: "default" as const,
        title: "Read + write subset",
        body: profile.capabilities.generate
          ? "Import, block/biome editing, snapshots, same-version export and generation with the exact official server JAR are enabled; everything else is preserved byte for byte."
          : "Import, block/biome editing, snapshots and same-version export are enabled; everything else is preserved byte for byte. Generation is not enabled for this release.",
      };
    case "read-only":
      return {
        variant: "default" as const,
        title: "Read-only version",
        body: "Worlds of this version can be analyzed and copied, but not edited or saved yet.",
      };
    case "convert":
      return {
        variant: "default" as const,
        title: "Convert only",
        body: "The world is imported and converted to a supported target; the original stays unchanged.",
      };
    case "out-of-scope":
      return {
        variant: "destructive" as const,
        title: "Outside the supported scope",
        body: "This version cannot be opened even for analysis in this build.",
      };
  }
}

function VersionFacts({ profile }: { profile: VersionProfile }) {
  const overworld = profile.dimensionProfiles.find((dimension) => dimension.kind === "overworld");
  const height = overworld
    ? `${overworld.minY} to ${overworld.minY + overworld.height - 1} (${overworld.height})`
    : "unknown";
  const facts: Array<[string, string]> = [
    ["DataVersion", String(profile.dataVersion)],
    ["Java", `${profile.java.minimumMajor} (recommended ${profile.java.recommendedMajor})`],
    ["Overworld height", height],
    ["World layout", LAYOUT_LABELS[profile.worldLayout] ?? profile.worldLayout],
    [
      "Resource pack",
      profile.resourcePackFormat != null ? String(profile.resourcePackFormat) : "unknown",
    ],
    ["Data pack", profile.datapackFormat != null ? String(profile.datapackFormat) : "unknown"],
    ["Block states", profile.contentCatalog.blockstateFormat],
    ["Item models", profile.contentCatalog.itemModelFormat],
    ["Registry", profile.contentCatalog.registryFormat],
    [
      "Creative categories",
      profile.contentCatalog.creativeCategories ? "available" : "not exposed",
    ],
    ["Data components", profile.contentCatalog.dataComponents ? "supported" : "not used"],
  ];

  return (
    <dl className="grid gap-x-4 gap-y-2 text-sm sm:grid-cols-2">
      {facts.map(([label, value]) => (
        <div key={label} className="min-w-0">
          <dt className="text-muted-foreground">{label}</dt>
          <dd className="truncate font-mono text-xs text-foreground">{value}</dd>
        </div>
      ))}
    </dl>
  );
}

function SelectedVersionCard({
  entry,
  locked,
}: {
  entry: VersionCatalogEntry | null;
  locked: boolean;
}) {
  if (!entry) {
    return (
      <Card className="gap-0 py-0">
        <CardContent className="flex flex-col items-center gap-2 p-8 text-center">
          <TriangleAlert className="size-5 text-muted-foreground" />
          <p className="text-sm font-medium">This version is not in the official catalog</p>
          <p className="max-w-xs text-sm text-muted-foreground">
            Choose another version from the list. FuryBuilder never invents version metadata.
          </p>
        </CardContent>
      </Card>
    );
  }

  const profile = entry.profile;
  const notice = profile ? supportNotice(profile) : null;
  return (
    <Card className="gap-0 py-0">
      <CardContent className="space-y-4 p-5">
        <div className="flex flex-wrap items-center gap-2">
          <h3 className="font-mono text-lg font-semibold">{entry.id}</h3>
          {entry.latestRelease ? <Badge variant="outline">Latest release</Badge> : null}
          {entry.latestSnapshot ? <Badge variant="outline">Latest snapshot</Badge> : null}
          {entry.recommended ? <Badge>Recommended</Badge> : null}
          {locked ? <Badge variant="secondary">Detected from the world</Badge> : null}
        </div>

        {profile ? (
          <>
            <div className="flex flex-wrap items-center gap-2">
              <Badge variant={SUPPORT_BADGE_VARIANT[profile.capabilities.supportLevel]}>
                {supportLevelLabel(profile.capabilities.supportLevel)}
              </Badge>
              <span className="text-xs text-muted-foreground">
                Released {entry.releaseTime.slice(0, 10)}
              </span>
            </div>

            <VersionFacts profile={profile} />

            {notice ? (
              <Alert variant={notice.variant}>
                <Info />
                <AlertTitle>{notice.title}</AlertTitle>
                <AlertDescription>{notice.body}</AlertDescription>
              </Alert>
            ) : (
              <Alert>
                <CheckCircle2 />
                <AlertTitle>Verified writes</AlertTitle>
                <AlertDescription>
                  Create, edit, save, export and generation are backed by this build&apos;s codecs
                  and fixtures.
                </AlertDescription>
              </Alert>
            )}

            <div className="rounded-lg border bg-muted/40 p-3 text-xs leading-relaxed text-muted-foreground">
              Block and item counts are filled from the exact client JAR after assets are prepared.
              Technical and creative-only entries stay visible and labelled there.
            </div>

            {profile.notes.length > 0 ? (
              <ul className="list-disc space-y-1 pl-4 text-xs leading-relaxed text-muted-foreground">
                {profile.notes.map((note) => (
                  <li key={note}>{note}</li>
                ))}
              </ul>
            ) : null}
          </>
        ) : (
          <>
            <div className="flex flex-wrap items-center gap-2">
              <Badge variant="destructive">No verified profile</Badge>
              <span className="text-xs text-muted-foreground">
                Released {entry.releaseTime.slice(0, 10)}
              </span>
            </div>
            <Alert variant="destructive">
              <TriangleAlert />
              <AlertTitle>Not selectable in this build</AlertTitle>
              <AlertDescription>
                {entry.unavailableReason ??
                  "FuryBuilder has no verified profile for this version yet."}
              </AlertDescription>
            </Alert>
            <p className="text-xs leading-relaxed text-muted-foreground">
              A version is added only after its world codec, component requirements and write path
              are verified. Choose a version with a support badge to continue.
            </p>
          </>
        )}
      </CardContent>
    </Card>
  );
}

export function ProjectVersionStep({
  catalog,
  loading,
  error,
  desktop,
  selected,
  locked,
  onSelect,
  onRetry,
}: {
  catalog: VersionCatalogReport | null;
  loading: boolean;
  error: ErrorPresentation | null;
  desktop: boolean;
  selected: string;
  locked: boolean;
  onSelect: (version: string) => void;
  onRetry: () => void;
}) {
  const [search, setSearch] = useState("");
  const [showSnapshots, setShowSnapshots] = useState(false);

  const entries = useMemo(() => {
    const query = search.trim().toLowerCase();
    return (catalog?.versions ?? []).filter((entry) => {
      if (!showSnapshots && entry.versionType !== "release") return false;
      return query.length === 0 || entry.id.toLowerCase().includes(query);
    });
  }, [catalog, search, showSnapshots]);

  const groups = useMemo(() => {
    const grouped = new Map<string, VersionCatalogEntry[]>();
    for (const entry of entries) {
      const series = versionSeries(entry.id);
      const list = grouped.get(series);
      if (list) {
        list.push(entry);
      } else {
        grouped.set(series, [entry]);
      }
    }
    return [...grouped.entries()];
  }, [entries]);

  const selectedVersion = selectedEntry(catalog, selected);

  return (
    <div className="space-y-4">
      <div className="grid gap-3 sm:grid-cols-[minmax(0,1fr)_auto] sm:items-end">
        <Field>
          <FieldLabel htmlFor="wizard-version-search">Find a version</FieldLabel>
          <div className="relative">
            <Search className="pointer-events-none absolute top-1/2 left-2.5 size-4 -translate-y-1/2 text-muted-foreground" />
            <Input
              id="wizard-version-search"
              className="pl-8"
              value={search}
              placeholder="Search by version id"
              disabled={!catalog}
              onChange={(event) => setSearch(event.target.value)}
            />
          </div>
          <FieldDescription>
            {catalog
              ? `${entries.length} version${entries.length === 1 ? "" : "s"} shown, grouped by release series.`
              : "The list comes from the official Mojang version manifest."}
          </FieldDescription>
        </Field>
        <label className="flex items-center gap-2 pb-6 text-sm">
          <Checkbox
            checked={showSnapshots}
            disabled={!catalog}
            onCheckedChange={(checked) => setShowSnapshots(checked === true)}
          />
          Show snapshots
        </label>
      </div>

      <div
        className={
          catalog ? "grid gap-4 lg:grid-cols-[minmax(0,0.9fr)_minmax(0,1.1fr)]" : undefined
        }
      >
        <Card className="gap-0 py-0">
          <CardContent className="p-0">
            {!desktop ? (
              <div className="p-4">
                <Alert>
                  <TriangleAlert />
                  <AlertTitle>Desktop core required</AlertTitle>
                  <AlertDescription>
                    Version metadata comes from the Rust backend. Open the project wizard in the
                    desktop build.
                  </AlertDescription>
                </Alert>
              </div>
            ) : loading ? (
              <div className="flex items-center gap-3 p-6 text-sm text-muted-foreground">
                <Loader2 className="size-4 animate-spin text-primary" />
                Loading the official version catalog…
              </div>
            ) : error ? (
              <div className="space-y-3 p-4">
                <Alert variant="destructive">
                  <TriangleAlert />
                  <AlertTitle>Version catalog unavailable</AlertTitle>
                  <AlertDescription>
                    <p>{error.message}</p>
                    {error.hint ? <p className="mt-1">{error.hint}</p> : null}
                  </AlertDescription>
                </Alert>
                <Button type="button" size="sm" variant="outline" onClick={onRetry}>
                  <RefreshCw data-icon="inline-start" />
                  Retry
                </Button>
              </div>
            ) : catalog ? (
              <ScrollArea className="h-96">
                <div className="space-y-1 p-2">
                  {groups.length === 0 ? (
                    <p className="px-2 py-6 text-sm text-muted-foreground">
                      No versions match this search.
                    </p>
                  ) : (
                    groups.map(([series, list]) => (
                      <div key={series} className="space-y-1">
                        <p className="px-2 pt-2 text-xs font-medium tracking-wide text-muted-foreground uppercase">
                          {series}
                        </p>
                        {list.map((entry) => (
                          <Button
                            key={entry.id}
                            type="button"
                            size="sm"
                            variant={entry.id === selected ? "secondary" : "ghost"}
                            className="h-auto w-full justify-between gap-2 px-2 py-2 text-left"
                            aria-pressed={entry.id === selected}
                            disabled={locked}
                            onClick={() => onSelect(entry.id)}
                          >
                            <span className="flex min-w-0 flex-wrap items-center gap-1.5">
                              <span className="font-mono text-sm">{entry.id}</span>
                              {entry.latestRelease ? <Badge variant="outline">Latest</Badge> : null}
                              {entry.recommended ? <Badge>Recommended</Badge> : null}
                            </span>
                            <Badge
                              variant={
                                entry.profile
                                  ? SUPPORT_BADGE_VARIANT[entry.profile.capabilities.supportLevel]
                                  : "secondary"
                              }
                            >
                              {entry.profile
                                ? SUPPORT_LEVEL_LABELS[entry.profile.capabilities.supportLevel]
                                : "No profile"}
                            </Badge>
                          </Button>
                        ))}
                      </div>
                    ))
                  )}
                </div>
              </ScrollArea>
            ) : (
              <div className="flex flex-col items-center gap-2 p-8 text-center">
                <Search className="size-5 text-muted-foreground" />
                <p className="text-sm font-medium">No catalog loaded</p>
                <p className="text-sm text-muted-foreground">
                  Retry loading the official version manifest.
                </p>
                <Button type="button" size="sm" variant="outline" onClick={onRetry}>
                  <RefreshCw data-icon="inline-start" />
                  Load versions
                </Button>
              </div>
            )}
          </CardContent>
        </Card>

        {catalog ? <SelectedVersionCard entry={selectedVersion} locked={locked} /> : null}
      </div>
    </div>
  );
}
