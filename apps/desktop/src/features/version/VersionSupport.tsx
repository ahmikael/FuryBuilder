import { Check, Minus } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import {
  SUPPORT_LEVEL_DESCRIPTIONS,
  VERSION_ACTIONS,
  VERSION_ACTION_LABELS,
  capabilityDisabledReason,
  supportBadgeLabel,
  supportBadgeVariant,
} from "@/project/version-support";
import type { SupportLevel, VersionCapabilities } from "@/project/version-types";
import { cn } from "@/lib/utils";

export function VersionSupportBadge({
  level,
  className,
}: {
  level: SupportLevel | null | undefined;
  className?: string;
}) {
  return (
    <Badge
      variant={supportBadgeVariant(level)}
      className={className}
      title={
        level
          ? SUPPORT_LEVEL_DESCRIPTIONS[level]
          : "This build has no verified profile for this project version."
      }
    >
      {supportBadgeLabel(level)}
    </Badge>
  );
}

/**
 * The exact capabilities of one version profile. Disabled entries are visible and carry
 * the reason; the UI never renders a silent grey control.
 */
export function VersionCapabilities({
  capabilities,
  className,
}: {
  capabilities: VersionCapabilities | null;
  className?: string;
}) {
  if (!capabilities) {
    return (
      <p className={cn("text-sm leading-relaxed text-muted-foreground", className)}>
        Capabilities are unavailable because this build has no verified profile for the project
        version. Open the project with a supported release instead of assuming editor access.
      </p>
    );
  }

  const disabled = VERSION_ACTIONS.flatMap((action) => {
    const reason = capabilityDisabledReason(capabilities, action);
    return reason ? [{ action, label: VERSION_ACTION_LABELS[action], reason }] : [];
  });

  return (
    <div className={cn("flex flex-col gap-2", className)}>
      <ul className="flex flex-wrap gap-2" aria-label="Version capabilities">
        {VERSION_ACTIONS.map((action) => {
          const reason = capabilityDisabledReason(capabilities, action);
          const label = VERSION_ACTION_LABELS[action];
          return (
            <li key={action}>
              <Badge
                variant={reason ? "outline" : "secondary"}
                aria-label={reason ? `${label}: ${reason}` : `${label}: available`}
                title={reason ?? `${label} is available for this version.`}
              >
                {reason ? <Minus data-icon="inline-start" /> : <Check data-icon="inline-start" />}
                {label}
              </Badge>
            </li>
          );
        })}
      </ul>
      {disabled.length > 0 ? (
        <ul className="flex flex-col gap-1 text-xs leading-relaxed text-muted-foreground">
          {disabled.map(({ action, label, reason }) => (
            <li key={action}>
              <span className="font-medium text-foreground">{label}</span>: {reason}
            </li>
          ))}
        </ul>
      ) : (
        <p className="text-xs leading-relaxed text-muted-foreground">
          All listed capabilities are verified for this release.
        </p>
      )}
    </div>
  );
}
