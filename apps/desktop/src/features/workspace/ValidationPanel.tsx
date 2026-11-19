import { useState } from "react";
import { AlertTriangle, Loader2, ShieldCheck, Wrench } from "lucide-react";

import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
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
      <div className="flex h-full flex-col items-center justify-center gap-1.5 text-center text-[11px] text-muted-foreground">
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
    <div className="flex min-h-0 flex-1 gap-2 text-[11px]" data-testid="validation-panel">
      <div className="scrollbar-thin flex min-h-0 w-80 shrink-0 flex-col gap-1.5 overflow-y-auto rounded border border-border bg-background/40 p-2">
        <div className="flex items-center gap-1.5">
          <ShieldCheck className="size-3 text-muted-foreground" />
          <span className="text-[10px] font-semibold tracking-wide uppercase">Validation</span>
          <span className="ml-auto font-mono text-[8px] text-muted-foreground">
            schema 1 · revision {revision}
          </span>
        </div>

        <div className="rounded border border-border/70 p-1.5">
          <div className="text-[9px] text-muted-foreground">Selection bounds</div>
          <p className="mt-0.5 break-all font-mono text-[9px]">
            {selection ? validationBoundsLabel(selection) : "Select a box in the viewport."}
          </p>
        </div>

        <div className="space-y-0.5">
          <span className="text-[9px] text-muted-foreground">Rules</span>
          {VALIDATION_RULES.map((rule) => (
            <label key={rule.value} className="flex items-start gap-1.5 text-[9px]">
              <input
                type="checkbox"
                className="mt-0.5 size-3 accent-current"
                checked={enabled[rule.value]}
                onChange={(event) =>
                  setEnabled((current) => ({ ...current, [rule.value]: event.target.checked }))
                }
              />
              <span className="min-w-0">
                <span className="block text-[10px]">{rule.label}</span>
                <span className="block text-[8px] text-muted-foreground">{rule.description}</span>
              </span>
            </label>
          ))}
        </div>

        <div className="grid grid-cols-2 gap-1">
          <label className="flex flex-col gap-0.5 text-[9px] text-muted-foreground">
            <span>Floating fix</span>
            <select
              className="h-6 rounded border border-border bg-background px-1 text-[10px]"
              value={floatingFix}
              aria-label="Floating fix"
              onChange={(event) => setFloatingFix(event.target.value as FloatingFix)}
            >
              <option value="support">Place support</option>
              <option value="remove">Remove blocks</option>
              <option value="none">No suggestion</option>
            </select>
          </label>
          <label className="flex flex-col gap-0.5 text-[9px] text-muted-foreground">
            <span>Surface materials</span>
            <Input
              className="h-6"
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
          <div className="space-y-1 rounded border border-border/70 p-1.5">
            <div className="flex items-center gap-1">
              <span className="text-[9px] text-muted-foreground">Path corridor</span>
              <Button
                size="sm"
                variant="outline"
                className="ml-auto h-5 px-1 text-[8px]"
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
            <div className="grid grid-cols-2 gap-1">
              <label className="flex flex-col gap-0.5 text-[8px] text-muted-foreground">
                <span>From X</span>
                <Input
                  className="h-6"
                  type="number"
                  value={pathFromX}
                  aria-label="Path from X"
                  onChange={(event) => setPathFromX(Number(event.target.value))}
                />
              </label>
              <label className="flex flex-col gap-0.5 text-[8px] text-muted-foreground">
                <span>From Z</span>
                <Input
                  className="h-6"
                  type="number"
                  value={pathFromZ}
                  aria-label="Path from Z"
                  onChange={(event) => setPathFromZ(Number(event.target.value))}
                />
              </label>
              <label className="flex flex-col gap-0.5 text-[8px] text-muted-foreground">
                <span>To X</span>
                <Input
                  className="h-6"
                  type="number"
                  value={pathToX}
                  aria-label="Path to X"
                  onChange={(event) => setPathToX(Number(event.target.value))}
                />
              </label>
              <label className="flex flex-col gap-0.5 text-[8px] text-muted-foreground">
                <span>To Z</span>
                <Input
                  className="h-6"
                  type="number"
                  value={pathToZ}
                  aria-label="Path to Z"
                  onChange={(event) => setPathToZ(Number(event.target.value))}
                />
              </label>
              <label className="flex flex-col gap-0.5 text-[8px] text-muted-foreground">
                <span>Base Y</span>
                <Input
                  className="h-6"
                  type="number"
                  value={pathBaseY}
                  aria-label="Path base Y"
                  onChange={(event) => setPathBaseY(Number(event.target.value))}
                />
              </label>
              <label className="flex flex-col gap-0.5 text-[8px] text-muted-foreground">
                <span>Width</span>
                <Input
                  className="h-6"
                  type="number"
                  min={1}
                  max={16}
                  value={pathWidth}
                  aria-label="Path width"
                  onChange={(event) => setPathWidth(Number(event.target.value))}
                />
              </label>
            </div>
            <label className="flex flex-col gap-0.5 text-[8px] text-muted-foreground">
              <span>Material</span>
              <Input
                className="h-6 font-mono"
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
        <p className="text-[9px] text-muted-foreground">
          Read-only. Up to 2,000,000 scanned blocks and 128 findings. Floating-block support is a
          heuristic, not a Minecraft stability simulation.
        </p>
        {error ? <p className="text-[10px] text-status-error">{error}</p> : null}
      </div>

      <div className="scrollbar-thin flex min-h-0 flex-1 flex-col gap-2 overflow-y-auto rounded border border-border bg-background/40 p-2">
        {report ? (
          <>
            <div className="flex flex-wrap items-center gap-1.5">
              <ShieldCheck className="size-3 text-muted-foreground" />
              <span className="text-[11px] font-semibold">Findings</span>
              <span className="rounded border border-border px-1 font-mono text-[8px] text-muted-foreground">
                {report.dimension.replace("minecraft:", "")}
              </span>
              <span className="ml-auto font-mono text-[8px] text-muted-foreground">
                world revision {report.worldRevision}
              </span>
            </div>

            <div className="flex flex-wrap gap-2 font-mono text-[9px]">
              <span className="text-status-error">{report.counts.error} error(s)</span>
              <span className="text-status-warning">{report.counts.warning} warning(s)</span>
              <span className="text-muted-foreground">{report.counts.info} info</span>
              <span className="text-muted-foreground">
                {report.scannedBlocks.toLocaleString()} blocks read
              </span>
              <span className="text-muted-foreground">{report.findings.length} finding(s)</span>
              {report.truncated ? (
                <span className="text-status-warning">truncated to 128</span>
              ) : null}
            </div>
            <p className="break-all font-mono text-[8px] text-muted-foreground">
              bounds {validationBoundsLabel(report.bounds)} · rules {report.rules.join(", ")}
            </p>

            {report.findings.length === 0 ? (
              <div className="flex flex-1 flex-col items-center justify-center gap-1 text-center text-[10px] text-muted-foreground">
                <ShieldCheck className="size-5 text-muted-foreground/50" />
                <p>No findings for the selected rules and bounds.</p>
              </div>
            ) : (
              <div className="space-y-1.5">
                {report.findings.map((finding, index) => (
                  <div
                    key={`${finding.ruleId}-${index}`}
                    className="rounded border border-border/70 p-1.5"
                  >
                    <div className="flex flex-wrap items-center gap-1.5">
                      <span
                        className={`text-[9px] font-semibold uppercase ${severityClass(finding.severity)}`}
                      >
                        {finding.severity}
                      </span>
                      <span className="text-[10px] font-semibold">
                        {validationRuleLabel(finding.ruleId)}
                      </span>
                      <span className="rounded border border-border px-1 font-mono text-[8px] text-muted-foreground">
                        {finding.confidence}
                      </span>
                      <span className="ml-auto font-mono text-[8px] text-muted-foreground">
                        {finding.positionCount} position(s)
                      </span>
                    </div>
                    <p className="mt-0.5 text-[10px] leading-relaxed">{finding.message}</p>
                    <p className="mt-0.5 break-all font-mono text-[8px] text-muted-foreground">
                      {validationBoundsLabel(finding.bounds)}
                    </p>
                    {finding.suggestedFix ? (
                      <div className="mt-1 rounded border border-border/60 bg-background/50 p-1">
                        <div className="flex items-center gap-1">
                          <Wrench className="size-3 text-muted-foreground" />
                          <span className="text-[9px]">{finding.suggestedFix.description}</span>
                        </div>
                        <div className="mt-1 flex flex-wrap items-center gap-1">
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
                            <span className="font-mono text-[8px] text-muted-foreground">
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
              <div className="rounded border border-status-warning/40 bg-status-warning/5 p-1.5">
                {report.warnings.map((warning) => (
                  <p
                    key={warning}
                    className="flex items-start gap-1 text-[9px] text-status-warning"
                  >
                    <AlertTriangle className="mt-0.5 size-3 shrink-0" /> {warning}
                  </p>
                ))}
              </div>
            ) : null}

            <div className="space-y-0.5">
              {report.notes.map((note) => (
                <p key={note} className="text-[9px] text-muted-foreground">
                  {note}
                </p>
              ))}
            </div>
          </>
        ) : (
          <div className="flex h-full flex-col items-center justify-center gap-1 text-center text-[10px] text-muted-foreground">
            <ShieldCheck className="size-5 text-muted-foreground/50" />
            <p>Select a region, choose rules and run validation to get structured findings.</p>
          </div>
        )}
      </div>
    </div>
  );
}
