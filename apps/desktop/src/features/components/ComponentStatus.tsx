import { Badge } from "@/components/ui/badge";
import type {
  ComponentMode,
  ComponentState,
  ComponentStatus as ComponentStatusData,
} from "@/lib/component-types";
import { cn } from "@/lib/utils";
import { formatBytes } from "@/lib/runtime-api";

export const COMPONENT_STATE_LABELS: Record<ComponentState, string> = {
  ready: "Ready",
  missing: "Not set up",
  unverified: "Unverified",
  incompatible: "Incompatible",
  error: "Error",
  not_required: "Not required",
};

export function componentStateVariant(
  state: ComponentState,
): "default" | "secondary" | "destructive" | "outline" {
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

export function componentModeLabel(mode: ComponentMode): string {
  switch (mode) {
    case "managed":
      return "Managed";
    case "manual":
      return "Manual";
    case "placeholder":
      return "Placeholder";
  }
}

export function componentHashLabel(hash: ComponentStatusData["hash"]): string | null {
  if (!hash) return null;
  return `${hash.algorithm.toUpperCase()} ${hash.value.slice(0, 12)}…`;
}

export function ComponentStateBadge({
  state,
  label,
}: {
  state: ComponentState;
  label?: string | undefined;
}) {
  return (
    <Badge variant={componentStateVariant(state)}>{label ?? COMPONENT_STATE_LABELS[state]}</Badge>
  );
}

export function ComponentModeBadge({ mode }: { mode: ComponentMode }) {
  return <Badge variant="outline">{componentModeLabel(mode)}</Badge>;
}

export function ComponentStatusBadges({
  status,
  label,
}: {
  status: ComponentStatusData;
  label?: string | undefined;
}) {
  return (
    <>
      <ComponentStateBadge state={status.state} label={label} />
      {status.mode ? <ComponentModeBadge mode={status.mode} /> : null}
    </>
  );
}

/** The facts and explanation behind a component status. */
export function ComponentStatusDetails({ status }: { status: ComponentStatusData | null }) {
  if (!status) return null;
  const hash = componentHashLabel(status.hash);
  const notes = status.notes ?? [];
  const readyFacts =
    status.state === "ready"
      ? [
          status.version,
          status.bytes !== undefined ? formatBytes(status.bytes) : null,
          hash,
          status.path,
        ].filter((fact): fact is string => Boolean(fact))
      : [];

  if (status.state === "ready") {
    if (readyFacts.length === 0 && notes.length === 0) return null;
    return (
      <div className="space-y-1 text-xs">
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
        {notes.length ? (
          <ul className="space-y-0.5 text-muted-foreground">
            {notes.map((note) => (
              <li key={note}>{note}</li>
            ))}
          </ul>
        ) : null}
      </div>
    );
  }

  return (
    <div className="space-y-1 text-xs">
      {status.mode === "placeholder" ? (
        <p className="text-status-warn">
          No client JAR, catalog or checksum is associated with this explicit choice.
        </p>
      ) : status.state === "missing" ? (
        <p className="text-muted-foreground">Nothing is selected for this project yet.</p>
      ) : (
        <p className="text-status-warn">{status.error ?? "This component is not verified."}</p>
      )}
      {notes.length ? (
        <ul className="space-y-0.5 text-muted-foreground">
          {notes.map((note) => (
            <li key={note}>{note}</li>
          ))}
        </ul>
      ) : null}
    </div>
  );
}

/**
 * Shared component state display. The wizard passes a project status; Settings passes the
 * result of an inventory verification with its own short label.
 */
export function ComponentStatus({
  status,
  label,
  className,
}: {
  status: ComponentStatusData | null;
  label?: string | undefined;
  className?: string;
}) {
  if (!status) return null;
  return (
    <div className={cn("space-y-1", className)}>
      <div className="flex flex-wrap items-center gap-2">
        <ComponentStatusBadges status={status} label={label} />
      </div>
      <ComponentStatusDetails status={status} />
    </div>
  );
}
