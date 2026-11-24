import { useState } from "react";
import { ArrowDown, ArrowUp, Boxes, Layers, Plus, Trash2 } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  Card,
  CardAction,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import { Field, FieldDescription, FieldGroup, FieldLabel } from "@/components/ui/field";
import { Input } from "@/components/ui/input";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import {
  flatTopY,
  flatTotalThickness,
  formatResourceId,
  type FlatDefaults,
  type FlatLayer,
  type FlatPreset,
} from "@/project/types";
import type { VersionProfile } from "@/project/version-types";

function overworldBounds(profile: VersionProfile | null): { minY: number; maxY: number } | null {
  const overworld = profile?.dimensionProfiles.find((dimension) => dimension.kind === "overworld");
  if (!overworld) return null;
  return { minY: overworld.minY, maxY: overworld.minY + overworld.height - 1 };
}

/**
 * Integer input that keeps a local draft so a leading minus can be typed before the value
 * becomes valid. The parent only receives whole numbers; blur restores the last committed one.
 */
function SignedIntegerInput({
  id,
  value,
  min,
  max,
  disabled,
  onValueChange,
}: {
  id: string;
  value: number;
  min?: number | undefined;
  max?: number | undefined;
  disabled: boolean;
  onValueChange: (value: number) => void;
}) {
  const [draft, setDraft] = useState(String(value));
  return (
    <Input
      id={id}
      type="number"
      className="font-mono"
      min={min}
      max={max}
      step={1}
      disabled={disabled}
      value={draft}
      onChange={(event) => {
        const raw = event.target.value;
        setDraft(raw);
        const parsed = Number(raw);
        if (raw.trim().length > 0 && Number.isInteger(parsed)) onValueChange(parsed);
      }}
      onBlur={() => setDraft(String(value))}
    />
  );
}

/** One summary tile. The layout is shared with the empty-source step. */
function SourceFact({ label, value }: { label: string; value: string }) {
  return (
    <Card className="gap-0 py-0">
      <CardContent className="flex flex-col gap-1 p-3">
        <span className="text-xs text-muted-foreground">{label}</span>
        <span className="font-mono text-sm">{value}</span>
      </CardContent>
    </Card>
  );
}

/**
 * Empty worlds have no source parameters. The step still states the version and the height
 * that the editor will use, so the first edit never happens against a surprise dimension.
 */
export function EmptySourceStep({
  version,
  profile,
}: {
  version: string;
  profile: VersionProfile | null;
}) {
  const bounds = overworldBounds(profile);
  return (
    <div className="flex flex-col gap-4">
      <Card className="gap-0 py-0">
        <CardContent className="flex items-start gap-3 p-5">
          <Boxes className="mt-0.5 size-5 shrink-0 text-muted-foreground" />
          <div className="flex flex-col gap-1">
            <p className="text-sm font-medium">Empty world</p>
            <p className="text-sm leading-relaxed text-muted-foreground">
              Every block starts as air. Nothing is generated, downloaded or substituted; all
              geometry comes from you or a tool.
            </p>
          </div>
        </CardContent>
      </Card>
      <div className="grid gap-3 sm:grid-cols-2">
        <SourceFact label="Minecraft version" value={version || "Not selected"} />
        <SourceFact
          label="Overworld height"
          value={bounds ? `Y ${bounds.minY}..=${bounds.maxY}` : "No verified overworld profile"}
        />
      </div>
    </div>
  );
}

/**
 * Flat source parameters. Every edit stays local until the wizard submits; the backend
 * re-validates the same limits and the selected profile before it writes a single block.
 */
export function FlatSourceStep({
  version,
  profile,
  preset,
  defaults,
  disabled,
  onChange,
}: {
  version: string;
  profile: VersionProfile | null;
  preset: FlatPreset | null;
  defaults: FlatDefaults | null;
  disabled: boolean;
  onChange: (preset: FlatPreset) => void;
}) {
  const bounds = overworldBounds(profile);

  if (!preset || !defaults) {
    return (
      <Card className="gap-0 py-0">
        <CardContent className="p-5 text-sm text-muted-foreground">
          Flat source defaults are not available. Retry loading the project settings before
          continuing.
        </CardContent>
      </Card>
    );
  }

  const total = flatTotalThickness(preset);
  const top = flatTopY(preset);
  const blockChoices = Array.from(
    new Set([...defaults.blockChoices, ...preset.layers.map((layer) => layer.block)]),
  );
  const biomeChoices = Array.from(new Set([...defaults.biomeChoices, preset.biome]));
  const blockItems = blockChoices.map((id) => ({ value: id, label: formatResourceId(id) }));
  const biomeItems = biomeChoices.map((id) => ({ value: id, label: formatResourceId(id) }));

  const changeLayer = (index: number, patch: Partial<FlatLayer>): void => {
    onChange({
      ...preset,
      layers: preset.layers.map((layer, layerIndex) =>
        layerIndex === index ? { ...layer, ...patch } : layer,
      ),
    });
  };

  const moveLayer = (index: number, direction: -1 | 1): void => {
    const target = index + direction;
    if (target < 0 || target >= preset.layers.length) return;
    const layers = [...preset.layers];
    const [layer] = layers.splice(index, 1);
    if (!layer) return;
    layers.splice(target, 0, layer);
    onChange({ ...preset, layers });
  };

  const removeLayer = (index: number): void => {
    if (preset.layers.length <= 1) return;
    onChange({ ...preset, layers: preset.layers.filter((_, layerIndex) => layerIndex !== index) });
  };

  const addLayer = (): void => {
    if (preset.layers.length >= defaults.limits.maxLayers) return;
    const block = defaults.blockChoices[0] ?? "minecraft:stone";
    onChange({ ...preset, layers: [...preset.layers, { block, thickness: 1 }] });
  };

  return (
    <div className="flex flex-col gap-5">
      <Card className="gap-0 py-0">
        <CardHeader>
          <div className="flex flex-col gap-1">
            <CardTitle>Flat source</CardTitle>
            <CardDescription>
              A {defaults.limits.footprint}×{defaults.limits.footprint} platform around the origin,
              built bottom-up from your layers. This is FuryBuilder geometry, not vanilla world
              generation.
            </CardDescription>
          </div>
          <CardAction>
            <Badge variant="outline" className="font-mono">
              {version || "No version"}
            </Badge>
          </CardAction>
        </CardHeader>
        <CardContent>
          <FieldGroup className="grid gap-4 sm:grid-cols-2">
            <Field>
              <FieldLabel htmlFor="wizard-flat-base-y">Bottom Y</FieldLabel>
              <SignedIntegerInput
                id="wizard-flat-base-y"
                min={bounds?.minY}
                max={bounds?.maxY}
                disabled={disabled}
                value={preset.baseY}
                onValueChange={(baseY) => onChange({ ...preset, baseY })}
              />
              <FieldDescription>
                {bounds
                  ? `The selected version's overworld spans Y ${bounds.minY}..=${bounds.maxY}.`
                  : "Choose a version with a verified overworld profile."}
              </FieldDescription>
            </Field>

            <Field>
              <FieldLabel htmlFor="wizard-flat-biome">Biome</FieldLabel>
              <Select
                items={biomeItems}
                value={preset.biome}
                onValueChange={(value) => {
                  if (value) onChange({ ...preset, biome: value });
                }}
                disabled={disabled}
              >
                <SelectTrigger
                  id="wizard-flat-biome"
                  className="w-full"
                  aria-label="Flat source biome"
                >
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  {biomeItems.map((item) => (
                    <SelectItem key={item.value} value={item.value}>
                      {item.label}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
              <FieldDescription>
                Applied uniformly to every loaded section of the starting area.
              </FieldDescription>
            </Field>
          </FieldGroup>
        </CardContent>
      </Card>

      <section className="flex flex-col gap-3" aria-labelledby="wizard-flat-layers-title">
        <div className="flex flex-wrap items-center justify-between gap-3">
          <div className="flex items-center gap-2">
            <Layers className="size-4 text-muted-foreground" />
            <h3 id="wizard-flat-layers-title" className="text-sm font-medium">
              Layers
            </h3>
            <Badge variant="secondary" className="tabular-nums">
              {preset.layers.length}/{defaults.limits.maxLayers}
            </Badge>
          </div>
          <Button
            type="button"
            size="sm"
            variant="outline"
            disabled={disabled || preset.layers.length >= defaults.limits.maxLayers}
            onClick={addLayer}
          >
            <Plus data-icon="inline-start" />
            Add layer
          </Button>
        </div>

        <div className="flex flex-col gap-2">
          {preset.layers.map((layer, index) => (
            <Card key={index} className="gap-0 py-0">
              <CardContent className="flex flex-col gap-3 p-3 sm:flex-row sm:items-end">
                <Field className="min-w-0 flex-1">
                  <FieldLabel htmlFor={`wizard-flat-layer-${index}-block`}>
                    Layer {index + 1} block
                  </FieldLabel>
                  <Select
                    items={blockItems}
                    value={layer.block}
                    onValueChange={(value) => {
                      if (value) changeLayer(index, { block: value });
                    }}
                    disabled={disabled}
                  >
                    <SelectTrigger
                      id={`wizard-flat-layer-${index}-block`}
                      className="w-full"
                      aria-label={`Layer ${index + 1} block`}
                    >
                      <SelectValue />
                    </SelectTrigger>
                    <SelectContent>
                      {blockItems.map((item) => (
                        <SelectItem key={item.value} value={item.value}>
                          {item.label}
                        </SelectItem>
                      ))}
                    </SelectContent>
                  </Select>
                </Field>

                <Field className="w-full sm:w-32">
                  <FieldLabel htmlFor={`wizard-flat-layer-${index}-thickness`}>
                    Layer {index + 1} thickness
                  </FieldLabel>
                  <Input
                    id={`wizard-flat-layer-${index}-thickness`}
                    type="number"
                    className="font-mono"
                    min={1}
                    max={defaults.limits.maxLayerThickness}
                    step={1}
                    disabled={disabled}
                    value={layer.thickness}
                    onChange={(event) =>
                      changeLayer(index, { thickness: Number(event.target.value) || 0 })
                    }
                  />
                </Field>

                <div className="flex items-center gap-1">
                  <Button
                    type="button"
                    size="icon"
                    variant="ghost"
                    aria-label={`Move layer ${index + 1} up`}
                    disabled={disabled || index === 0}
                    onClick={() => moveLayer(index, -1)}
                  >
                    <ArrowUp />
                  </Button>
                  <Button
                    type="button"
                    size="icon"
                    variant="ghost"
                    aria-label={`Move layer ${index + 1} down`}
                    disabled={disabled || index === preset.layers.length - 1}
                    onClick={() => moveLayer(index, 1)}
                  >
                    <ArrowDown />
                  </Button>
                  <Button
                    type="button"
                    size="icon"
                    variant="ghost"
                    aria-label={`Remove layer ${index + 1}`}
                    disabled={disabled || preset.layers.length <= 1}
                    onClick={() => removeLayer(index)}
                  >
                    <Trash2 />
                  </Button>
                </div>
              </CardContent>
            </Card>
          ))}
        </div>

        <p className="text-xs leading-relaxed text-muted-foreground">
          Layers are written from the bottom up. Each layer may be 1–
          {defaults.limits.maxLayerThickness} blocks thick; the whole source may be 1–
          {defaults.limits.maxTotalThickness} blocks thick and must stay inside the overworld
          height. Current span:{" "}
          <span className="font-mono">
            Y {preset.baseY}..={top} ({total} blocks)
          </span>
          .
        </p>
      </section>
    </div>
  );
}
