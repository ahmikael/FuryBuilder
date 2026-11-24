import type { ReactNode } from "react";

import { Card, CardContent } from "@/components/ui/card";
import type { ComponentStatus as ComponentStatusData } from "@/lib/component-types";
import { cn } from "@/lib/utils";

import { ComponentModeBadge, ComponentStateBadge, ComponentStatusDetails } from "./ComponentStatus";

export interface ComponentCardProps {
  icon?: ReactNode;
  title: string;
  /** Heading level for the card title; parent sections already own their heading. */
  titleAs?: "h2" | "h3" | "h4";
  description?: ReactNode;
  status?: ComponentStatusData | null;
  /** Summary shown at the right of the title, such as inventory size. */
  meta?: ReactNode;
  /** Primary action shown at the right of the title. */
  action?: ReactNode;
  /** Accessible group name when the card represents a collection. */
  label?: string;
  children?: ReactNode;
  className?: string;
  contentClassName?: string;
}

/**
 * Shared component shell for the project wizard and Settings. The wizard supplies a project
 * status and provisioning controls; Settings supplies inventory metadata and maintenance
 * actions instead. Neither surface owns a second card layout.
 */
export function ComponentCard({
  icon,
  title,
  titleAs: Title = "h3",
  description,
  status,
  meta,
  action,
  label,
  children,
  className,
  contentClassName,
}: ComponentCardProps) {
  return (
    <Card
      role={label ? "group" : undefined}
      aria-label={label}
      className={cn("min-w-0 gap-0 py-0", className)}
    >
      <CardContent className={cn("space-y-4 p-5", contentClassName)}>
        <div className="flex flex-wrap items-start gap-3">
          {icon ? (
            <div className="flex size-9 shrink-0 items-center justify-center rounded-lg border bg-muted/40">
              {icon}
            </div>
          ) : null}
          <div className="min-w-0 flex-1 space-y-1">
            <div className="flex flex-wrap items-center gap-2">
              <Title className="text-sm font-semibold">{title}</Title>
              {status ? <ComponentStateBadge state={status.state} /> : null}
              {status?.mode ? <ComponentModeBadge mode={status.mode} /> : null}
              {meta || action ? (
                <div className="ml-auto flex flex-wrap items-center gap-2">
                  {meta}
                  {action}
                </div>
              ) : null}
            </div>
            {description ? (
              <p className="text-sm leading-relaxed text-muted-foreground">{description}</p>
            ) : null}
          </div>
        </div>
        {status ? <ComponentStatusDetails status={status} /> : null}
        {children}
      </CardContent>
    </Card>
  );
}
