import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { VersionCapabilities, VersionSupportBadge } from "@/features/version/VersionSupport";
import { creationModeLabel, type ProjectSummary } from "@/project/types";

/**
 * Version facts and the verified capability contract of an existing project. It is the
 * Project Settings source for support badges and disabled reasons; it never infers
 * capabilities from the version string.
 */
export function ProjectVersionSupport({ project }: { project: ProjectSummary }) {
  const capabilities = project.capabilities;
  return (
    <Card>
      <CardHeader>
        <div className="flex flex-wrap items-center gap-2">
          <CardTitle>Version and support</CardTitle>
          <VersionSupportBadge level={capabilities?.supportLevel} />
        </div>
        <CardDescription>
          The exact target is fixed when the project is created. Every capability below comes from
          its verified built-in profile.
        </CardDescription>
      </CardHeader>
      <CardContent className="flex flex-col gap-4">
        <dl className="grid gap-x-4 gap-y-2 text-sm sm:grid-cols-2">
          <div className="min-w-0">
            <dt className="text-muted-foreground">Minecraft Java</dt>
            <dd className="truncate font-mono text-xs text-foreground">
              {project.targetMinecraftVersion} · DataVersion {project.targetDataVersion}
            </dd>
          </div>
          <div className="min-w-0">
            <dt className="text-muted-foreground">Profile</dt>
            <dd className="truncate font-mono text-xs text-foreground">
              {project.targetProfileId}
            </dd>
          </div>
          <div className="min-w-0">
            <dt className="text-muted-foreground">Creation mode</dt>
            <dd className="text-foreground">{creationModeLabel(project.creationMode)}</dd>
          </div>
          <div className="min-w-0">
            <dt className="text-muted-foreground">Saved revision</dt>
            <dd className="font-mono text-xs text-foreground tabular-nums">
              rev {project.worldRevision}
            </dd>
          </div>
        </dl>

        <VersionCapabilities capabilities={capabilities} />

        <p className="text-xs leading-relaxed text-muted-foreground">
          Version changes are upgrade-only and copy-only: downgrades are refused. A same-family
          upgrade takes a mandatory checkpoint and publishes a verified copy, leaving this
          project&apos;s manifest and snapshot unchanged.
        </p>
      </CardContent>
    </Card>
  );
}
