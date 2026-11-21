import { useState } from "react";
import { AlertTriangle, Loader2, ShieldCheck, Wrench } from "lucide-react";

import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import {
  Empty,
  EmptyDescription,
  EmptyHeader,
  EmptyMedia,
  EmptyTitle,
} from "@/components/ui/empty";
import { Input } from "@/components/ui/input";
import { NativeSelect, NativeSelectOption } from "@/components/ui/native-select";
import { applyEditorOperation } from "@/editor/commands";
import { isTauriRuntime, normalizeError } from "@/lib/ipc";
import { validateRegion } from "@/lib/validation-api";
import { previewOperation } from "@/lib/world-api";
import {
  DEFAULT_VALIDATION_RULES,
  VALIDATION_RULES,
  severityClass,
  validationBoundsLabel,
  validationRuleLabel,
  type FloatingFix,
  type ValidationQuery,
  type ValidationReport,
  type ValidationRuleId,
} from "@/project/validation-types";
import { useToolStore } from "@/state/tool-store";
import { useWorldStore } from "@/state/world-store";

/**
 * Read-only Validation/critic panel. Findings are bound to an exact world revision and
 * suggested fixes are ordinary operations: they are previewed and applied through the
 * shared confirmed transaction path, never written by the validator.
 */
export function ValidationPanel() {
  const selection = useToolStore((state) => state.selection);
  const dimension = useWorldStore((state) => state.dimension);
  const revision = useWorldStore((state) => state.revision);
  const [enabled, setEnabled] = useState<Record<ValidationRuleId, boolean>>(() => {
    const initial = {} as Record<ValidationRuleId, boolean>;
    for (const rule of VALIDATION_RULES) {
      initial[rule.value] = DEFAULT_VALIDATION_RULES.includes(rule.value);
    }
    return initial;
  });
  const [pathFromX, setPathFromX] = useState(0);
  const [pathFromZ, setPathFromZ] = useState(0);
  const [pathToX, setPathToX] = useState(0);
  const [pathToZ, setPathToZ] = useState(0);
  const [pathBaseY, setPathBaseY] = useState(64);
  const [pathWidth, setPathWidth] = useState(1);
  const [pathMaterial, setPathMaterial] = useState("minecraft:dirt");
  const [maxMaterials, setMaxMaterials] = useState(32);
  const [floatingFix, setFloatingFix] = useState<FloatingFix>("support");
  const [report, setReport] = useState<ValidationReport | null>(null);
  const [previews, setPreviews] = useState<Record<number, string>>({});
  const [busy, setBusy] = useState(false);
  const [applying, setApplying] = useState<number | null>(null);
  const [error, setError] = useState<string | null>(null);
  const editing = isTauriRuntime();

  if (!editing) {
    return (
      <div className="flex h-full flex-col items-center justify-center gap-2 text-center text-sm text-muted-foreground">
        <ShieldCheck className="size-5 text-muted-foreground/60" />
        <p className="max-w-md leading-relaxed">
          Validation runs bounded rules inside the Rust Core. Open the desktop app to inspect
          structured findings and preview suggested fixes.
        </p>
      </div>
    );
  }

  const runValidation = async () => {
    if (!selection) {
      setError("Select a box in the viewport first.");
      return;
    }
    const rules = VALIDATION_RULES.filter((rule) => enabled[rule.value]).map((rule) => rule.value);
    const query: ValidationQuery = {
      dimension,
      bounds: { min: selection.min, max: selection.max },
      rules,
      maxSurfaceMaterials: maxMaterials,
      floatingFix,
    };
    if (enabled.path_continuity) {
      query.path = {
        from: { x: pathFromX, z: pathFromZ },
        to: { x: pathToX, z: pathToZ },
        baseY: pathBaseY,
        width: pathWidth,
        material: { name: pathMaterial, properties: {} },
      };
    }
    setBusy(true);
    setError(null);
    try {
      const next = await validateRegion(query);
      setReport(next);
      setPreviews({});
    } catch (cause) {
      setError(normalizeError(cause).message);
    } finally {
      setBusy(false);
    }
  };

  const previewFix = async (index: number) => {
    const fix = report?.findings[index]?.suggestedFix;
    if (!fix) return;
    setError(null);
    try {
      const result = await previewOperation(fix.operation);
      setPreviews((current) => ({
        ...current,
        [index]: `Preview: ${result.changedBlocks} changed block(s), ${result.dirtyChunks.length} dirty chunk(s).`,
      }));
    } catch (cause) {
      setError(normalizeError(cause).message);
    }
  };

  const applyFix = async (index: number) => {
    const fix = report?.findings[index]?.suggestedFix;
    if (!fix) return;
    setApplying(index);
    setError(null);
    try {
      const result = await applyEditorOperation(fix.operation, {
        label: "Apply validation fix",
      });
      if (result) {
        await runValidation();
      }
    } finally {
      setApplying(null);
    }
  };

  return (
    <div className="flex min-h-0 flex-1 gap-4 text-sm" data-testid="validation-panel">
      <div className="scrollbar-thin flex min-h-0 w-80 shrink-0 flex-col gap-3 overflow-y-auto rounded-xl border bg-card p-4">
        <div className="flex items-center gap-2">
          <ShieldCheck className="size-4 text-muted-foreground" />
          <span className="text-base font-semibold">Validation</span>
          <span className="ml-auto font-mono text-xs text-muted-foreground">
            schema 1 · revision {revision}
          </span>
        </div>

        <div className="rounded-xl border bg-card p-4">
          <div className="text-xs text-muted-foreground">Selection bounds</div>
          <p className="mt-0.5 break-all font-mono text-xs">
            {selection ? validationBoundsLabel(selection) : "Select a box in the viewport."}
          </p>
        </div>

        <div className="space-y-2">
          <span className="text-xs font-medium text-muted-foreground">Rules</span>
          {VALIDATION_RULES.map((rule) => (
            <label key={rule.value} className="flex items-start gap-2 text-sm">
              <Checkbox
                className="mt-0.5"
                checked={enabled[rule.value]}
                onCheckedChange={(checked) =>
                  setEnabled((current) => ({ ...current, [rule.value]: checked === true }))
                }
              />
              <span className="min-w-0">
                <span className="block text-sm">{rule.label}</span>
                <span className="block text-xs text-muted-foreground">{rule.description}</span>
              </span>
            </label>
          ))}
        </div>

        <div className="grid grid-cols-2 gap-3">
          <label className="flex flex-col gap-1.5 text-sm text-muted-foreground">
            <span>Floating fix</span>
            <NativeSelect
              className="w-fit"
              value={floatingFix}
              aria-label="Floating fix"
              onChange={(event) => setFloatingFix(event.target.value as FloatingFix)}
            >
              <NativeSelectOption value="support">Place support</NativeSelectOption>
              <NativeSelectOption value="remove">Remove blocks</NativeSelectOption>
              <NativeSelectOption value="none">No suggestion</NativeSelectOption>
            </NativeSelect>
          </label>
          <label className="flex flex-col gap-1.5 text-sm text-muted-foreground">
            <span>Surface materials</span>
            <Input
              type="number"
              min={1}
              max={64}
              value={maxMaterials}
              aria-label="Surface material guideline"
              onChange={(event) => setMaxMaterials(Number(event.target.value))}
            />
          </label>
        </div>

        {enabled.path_continuity ? (
          <div className="space-y-1 rounded-lg border p-3">
            <div className="flex items-center gap-1">
              <span className="text-xs text-muted-foreground">Path corridor</span>
              <Button
                size="sm"
                variant="outline"
                className="ml-auto h-5 px-1 text-xs"
                onClick={() => {
                  if (!selection) return;
                  setPathFromX(selection.min.x);
                  setPathFromZ(Math.floor((selection.min.z + selection.max.z) / 2));
                  setPathToX(selection.max.x);
                  setPathToZ(Math.floor((selection.min.z + selection.max.z) / 2));
                  setPathBaseY(selection.max.y);
                }}
              >
                Use selection
              </Button>
            </div>
            <div className="grid grid-cols-2 gap-3">
              <label className="flex flex-col gap-1.5 text-sm text-muted-foreground">
                <span>From X</span>
                <Input
                  type="number"
                  value={pathFromX}
                  aria-label="Path from X"
                  onChange={(event) => setPathFromX(Number(event.target.value))}
                />
              </label>
              <label className="flex flex-col gap-1.5 text-sm text-muted-foreground">
                <span>From Z</span>
                <Input
                  type="number"
                  value={pathFromZ}
                  aria-label="Path from Z"
                  onChange={(event) => setPathFromZ(Number(event.target.value))}
                />
              </label>
              <label className="flex flex-col gap-1.5 text-sm text-muted-foreground">
                <span>To X</span>
                <Input
                  type="number"
                  value={pathToX}
                  aria-label="Path to X"
                  onChange={(event) => setPathToX(Number(event.target.value))}
                />
              </label>
              <label className="flex flex-col gap-1.5 text-sm text-muted-foreground">
                <span>To Z</span>
                <Input
                  type="number"
                  value={pathToZ}
                  aria-label="Path to Z"
                  onChange={(event) => setPathToZ(Number(event.target.value))}
                />
              </label>
              <label className="flex flex-col gap-1.5 text-sm text-muted-foreground">
                <span>Base Y</span>
                <Input
                  type="number"
                  value={pathBaseY}
                  aria-label="Path base Y"
                  onChange={(event) => setPathBaseY(Number(event.target.value))}
                />
              </label>
              <label className="flex flex-col gap-1.5 text-sm text-muted-foreground">
                <span>Width</span>
                <Input
                  type="number"
                  min={1}
                  max={16}
                  value={pathWidth}
                  aria-label="Path width"
                  onChange={(event) => setPathWidth(Number(event.target.value))}
                />
              </label>
            </div>
            <label className="flex flex-col gap-1.5 text-sm text-muted-foreground">
              <span>Material</span>
              <Input
                className="font-mono"
                value={pathMaterial}
                aria-label="Path material"
                onChange={(event) => setPathMaterial(event.target.value)}
              />
            </label>
          </div>
        ) : null}

        <Button size="sm" variant="outline" disabled={busy} onClick={() => void runValidation()}>
          {busy ? (
            <Loader2 className="mr-1 size-3 animate-spin" />
          ) : (
            <ShieldCheck className="mr-1 size-3" />
          )}
          Run validation
        </Button>
        <p className="text-sm text-muted-foreground">
          Read-only. Up to 2,000,000 scanned blocks and 128 findings. Floating-block support is a
          heuristic, not a Minecraft stability simulation.
        </p>
        {error ? <p className="text-xs text-status-error">{error}</p> : null}
      </div>

      <div className="scrollbar-thin flex min-h-0 flex-1 flex-col gap-4 overflow-y-auto rounded-xl border bg-card p-4">
        {report ? (
          <>
            <div className="flex flex-wrap items-center gap-2">
              <ShieldCheck className="size-4 text-muted-foreground" />
              <span className="text-base font-semibold">Findings</span>
              <Badge variant="outline" className="font-mono">
                {report.dimension.replace("minecraft:", "")}
              </Badge>
              <span className="ml-auto font-mono text-xs text-muted-foreground">
                world revision {report.worldRevision}
              </span>
            </div>

            <div className="flex flex-wrap gap-2 font-mono text-xs">
              <span className="text-status-error">{report.counts.error} error(s)</span>
              <span className="text-status-warn">{report.counts.warning} warning(s)</span>
              <span className="text-muted-foreground">{report.counts.info} info</span>
              <span className="text-muted-foreground">
                {report.scannedBlocks.toLocaleString()} blocks read
              </span>
              <span className="text-muted-foreground">{report.findings.length} finding(s)</span>
              {report.truncated ? <span className="text-status-warn">truncated to 128</span> : null}
            </div>
            <p className="break-all font-mono text-xs text-muted-foreground">
              bounds {validationBoundsLabel(report.bounds)} · rules {report.rules.join(", ")}
            </p>

            {report.findings.length === 0 ? (
              <Empty className="flex-1 border-0">
                <EmptyHeader>
                  <EmptyMedia variant="icon">
                    <ShieldCheck />
                  </EmptyMedia>
                  <EmptyTitle>No findings</EmptyTitle>
                  <EmptyDescription>
                    No findings for the selected rules and bounds.
                  </EmptyDescription>
                </EmptyHeader>
              </Empty>
            ) : (
              <div className="space-y-1.5">
                {report.findings.map((finding, index) => (
                  <div key={`${finding.ruleId}-${index}`} className="rounded-xl border bg-card p-4">
                    <div className="flex flex-wrap items-center gap-2">
                      <span
                        className={`text-xs font-semibold uppercase ${severityClass(finding.severity)}`}
                      >
                        {finding.severity}
                      </span>
                      <span className="text-xs font-semibold">
                        {validationRuleLabel(finding.ruleId)}
                      </span>
                      <Badge variant="outline" className="font-mono">
                        {finding.confidence}
                      </Badge>
                      <span className="ml-auto font-mono text-xs text-muted-foreground">
                        {finding.positionCount} position(s)
                      </span>
                    </div>
                    <p className="mt-1 text-sm leading-relaxed">{finding.message}</p>
                    <p className="mt-0.5 break-all font-mono text-xs text-muted-foreground">
                      {validationBoundsLabel(finding.bounds)}
                    </p>
                    {finding.suggestedFix ? (
                      <div className="mt-2 rounded-lg border bg-muted/30 p-3">
                        <div className="flex items-center gap-2">
                          <Wrench className="size-3.5 text-muted-foreground" />
                          <span className="text-sm">{finding.suggestedFix.description}</span>
                        </div>
                        <div className="mt-2 flex flex-wrap items-center gap-2">
                          <Button
                            size="sm"
                            variant="outline"
                            onClick={() => void previewFix(index)}
                          >
                            Preview fix
                          </Button>
                          <Button
                            size="sm"
                            variant="outline"
                            disabled={applying === index}
                            onClick={() => void applyFix(index)}
                          >
                            {applying === index ? (
                              <Loader2 className="mr-1 size-3 animate-spin" />
                            ) : null}
                            Apply fix
                          </Button>
                          {previews[index] ? (
                            <span className="font-mono text-xs text-muted-foreground">
                              {previews[index]}
                            </span>
                          ) : null}
                        </div>
                      </div>
                    ) : null}
                  </div>
                ))}
              </div>
            )}

            {report.warnings.length > 0 ? (
              <div className="space-y-1 rounded-lg border border-status-warn/30 bg-status-warn/5 p-3">
                {report.warnings.map((warning) => (
                  <p key={warning} className="flex items-start gap-2 text-sm text-status-warn">
                    <AlertTriangle className="mt-0.5 size-4 shrink-0" /> {warning}
                  </p>
                ))}
              </div>
            ) : null}

            <div className="space-y-0.5">
              {report.notes.map((note) => (
                <p key={note} className="text-xs text-muted-foreground">
                  {note}
                </p>
              ))}
            </div>
          </>
        ) : (
          <Empty className="h-full border-0">
            <EmptyHeader>
              <EmptyMedia variant="icon">
                <ShieldCheck />
              </EmptyMedia>
              <EmptyTitle>Nothing validated yet</EmptyTitle>
              <EmptyDescription>
                Select a region, choose rules and run validation to get structured findings.
              </EmptyDescription>
            </EmptyHeader>
          </Empty>
        )}
      </div>
    </div>
  );
}
