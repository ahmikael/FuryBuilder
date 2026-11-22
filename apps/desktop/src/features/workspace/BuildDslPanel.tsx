import { useState } from "react";
import { Braces, Loader2, Play, RotateCcw, ScanLine } from "lucide-react";

import { Badge } from "@/components/ui/badge";
import {
  Empty,
  EmptyDescription,
  EmptyHeader,
  EmptyMedia,
  EmptyTitle,
} from "@/components/ui/empty";
import { Textarea } from "@/components/ui/textarea";
import { Button } from "@/components/ui/button";
import { applyEditorOperation } from "@/editor/commands";
import { previewBuildDsl } from "@/lib/build-dsl-api";
import { isTauriRuntime, normalizeError } from "@/lib/ipc";
import {
  defaultBuildRecipeText,
  type BuildDslPreview,
  type BuildRecipe,
} from "@/project/build-dsl-types";

/**
 * Editor for the versioned Build DSL. The component only parses JSON and sends data to
 * the Core; it never evaluates a recipe. Compilation, budgets, workspace policy and
 * preview all happen in Rust, and the compiled Batch goes through the normal apply path.
 */
export function BuildDslPanel() {
  const [text, setText] = useState(defaultBuildRecipeText);
  const [preview, setPreview] = useState<BuildDslPreview | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const editing = isTauriRuntime();

  if (!editing) {
    return (
      <div className="flex h-full flex-col items-center justify-center gap-2 text-center text-sm text-muted-foreground">
        <Braces className="size-5 text-muted-foreground/60" />
        <p className="max-w-md leading-relaxed">
          Compile, preview and apply recipes in the desktop app.
        </p>
      </div>
    );
  }

  const parse = (): unknown | null => {
    setError(null);
    try {
      return JSON.parse(text) as unknown;
    } catch (cause) {
      setPreview(null);
      setError(`Invalid JSON: ${cause instanceof Error ? cause.message : String(cause)}`);
      return null;
    }
  };

  const format = () => {
    const parsed = parse();
    if (parsed === null) return;
    setText(JSON.stringify(parsed, null, 2));
  };

  const compileAndPreview = async () => {
    const parsed = parse();
    if (parsed === null) return;
    setBusy(true);
    setPreview(null);
    try {
      setPreview(await previewBuildDsl(parsed as BuildRecipe));
    } catch (cause) {
      setError(normalizeError(cause).message);
    } finally {
      setBusy(false);
    }
  };

  const applyCompiled = async () => {
    if (!preview) return;
    setBusy(true);
    setError(null);
    try {
      const result = await applyEditorOperation(preview.operation, { label: "Build DSL" });
      if (result) setPreview(null);
    } catch (cause) {
      setError(normalizeError(cause).message);
    } finally {
      setBusy(false);
    }
  };

  const reset = () => {
    setText(defaultBuildRecipeText());
    setPreview(null);
    setError(null);
  };

  return (
    <div className="flex min-h-0 flex-1 gap-4 text-sm" data-testid="build-dsl-panel">
      <div className="flex min-h-0 flex-1 flex-col gap-3 rounded-xl border bg-card p-4">
        <div className="flex items-center gap-2">
          <Braces className="size-3 text-muted-foreground" />
          <span className="text-base font-semibold">Build DSL</span>
          <Badge variant="secondary" className="font-mono">
            schema 1 · house v1
          </Badge>
          <Button size="sm" variant="ghost" className="ml-auto" onClick={reset} disabled={busy}>
            <RotateCcw className="mr-1 size-3" /> Reset sample
          </Button>
        </div>
        <Textarea
          className="scrollbar-thin min-h-0 flex-1 resize-none font-mono text-sm leading-relaxed"
          value={text}
          spellCheck={false}
          aria-label="Build DSL recipe"
          onChange={(event) => {
            setText(event.target.value);
            setPreview(null);
            setError(null);
          }}
        />
        <div className="flex flex-wrap items-center gap-1">
          <Button size="sm" variant="outline" onClick={format} disabled={busy}>
            Format JSON
          </Button>
          <Button
            size="sm"
            variant="outline"
            onClick={() => void compileAndPreview()}
            disabled={busy}
          >
            {busy ? (
              <Loader2 className="mr-1 size-3 animate-spin" />
            ) : (
              <ScanLine className="mr-1 size-3" />
            )}
            Compile &amp; preview
          </Button>
          <Button
            size="sm"
            onClick={() => void applyCompiled()}
            disabled={busy || !preview || preview.preview.changedBlocks === 0}
          >
            <Play className="mr-1 size-3" /> Apply compiled
          </Button>
        </div>
        <p className="text-sm text-muted-foreground">
          Data only: no JavaScript, shell or file access. The Core compiles the recipe into one
          reversible Batch (max 64 houses, 16,384 blocks), then the ordinary preview and workspace
          policy run before any write.
        </p>
      </div>

      <div className="scrollbar-thin flex min-h-0 w-80 shrink-0 flex-col gap-3 overflow-y-auto rounded-xl border bg-card p-4">
        <div className="text-sm font-semibold">Preview</div>
        {preview ? (
          <>
            <div className="grid grid-cols-2 gap-x-3 gap-y-1 font-mono text-xs text-muted-foreground">
              <span>houses: {preview.summary.houses}</span>
              <span>blocks: {preview.summary.blocks.toLocaleString()}</span>
              <span>affected: {preview.preview.affectedBlocks.toLocaleString()}</span>
              <span>changed: {preview.preview.changedBlocks.toLocaleString()}</span>
              <span className="col-span-2">dimension: {preview.summary.dimension}</span>
              <span className="col-span-2">
                bounds: ({preview.summary.bounds.min.x},{preview.summary.bounds.min.y},
                {preview.summary.bounds.min.z})..({preview.summary.bounds.max.x},
                {preview.summary.bounds.max.y},{preview.summary.bounds.max.z})
              </span>
              <span className="col-span-2">dirty chunks: {preview.preview.dirtyChunks.length}</span>
            </div>
            {[...preview.summary.warnings, ...preview.preview.warnings].map((warning) => (
              <p key={warning} className="text-xs text-status-warn">
                {warning}
              </p>
            ))}
            <p className="text-sm text-muted-foreground">
              Apply uses the same revision-checked operation path as manual edits. The compiled
              operation is not written until you press Apply.
            </p>
          </>
        ) : (
          <Empty className="h-full border-0">
            <EmptyHeader>
              <EmptyMedia variant="icon">
                <Braces />
              </EmptyMedia>
              <EmptyTitle>Nothing compiled yet</EmptyTitle>
              <EmptyDescription>
                Compile a recipe to see its budget and exact preview.
              </EmptyDescription>
            </EmptyHeader>
          </Empty>
        )}
        {error ? (
          <div
            role="alert"
            className="rounded-lg border border-status-error/30 bg-status-error/5 p-3 text-sm text-status-error"
          >
            {error}
          </div>
        ) : null}
      </div>
    </div>
  );
}
