import type { Bounds3 } from "@/renderer/types";
import type { WorkspacePolicy, WorkspaceRegion, WorkspaceLayer } from "@/project/types";

export const MAX_WORKSPACE_LAYERS = 64;
export const MAX_WORKSPACE_MASKS = 64;
export const MAX_WORKSPACE_PROTECTED_REGIONS = 64;
export const MAX_WORKSPACE_NAME_CHARS = 64;
const MAX_WORKSPACE_ID_CHARS = 64;

export type WorkspaceRegionKind = "mask" | "protected";

/** Mirrors the Rust id rules so the UI never sends an id the Core must reject. */
export function workspaceSlug(name: string, existing: Iterable<string>): string {
  const used = new Set(existing);
  let base = name
    .trim()
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/-+/g, "-")
    .replace(/^-|-$/g, "");
  if (!base) base = "region";
  base = base.slice(0, MAX_WORKSPACE_ID_CHARS).replace(/-+$/g, "");
  if (!/^[a-z0-9]/.test(base)) base = `r${base}`.slice(0, MAX_WORKSPACE_ID_CHARS);
  if (!used.has(base)) return base;
  for (let suffix = 2; suffix < Number.MAX_SAFE_INTEGER; suffix += 1) {
    const candidate = `${base}-${suffix}`;
    if (!used.has(candidate)) return candidate;
  }
  return `${base}-2`;
}

function validName(name: string): string | null {
  const trimmed = name.trim();
  if (!trimmed || trimmed.length > MAX_WORKSPACE_NAME_CHARS) return null;
  return trimmed;
}

function ordered(bounds: Bounds3): boolean {
  return (
    bounds.min.x <= bounds.max.x && bounds.min.y <= bounds.max.y && bounds.min.z <= bounds.max.z
  );
}

export function withNewLayer(policy: WorkspacePolicy, name: string): WorkspacePolicy | null {
  const trimmed = validName(name);
  if (!trimmed || policy.layers.length >= MAX_WORKSPACE_LAYERS) return null;
  const id = workspaceSlug(
    trimmed,
    policy.layers.map((layer) => layer.id),
  );
  return {
    ...policy,
    layers: [...policy.layers, { id, name: trimmed, visible: true, locked: false }],
    activeLayerId: id,
  };
}

export function withLayerUpdate(
  policy: WorkspacePolicy,
  id: string,
  patch: Partial<Pick<WorkspaceLayer, "visible" | "locked" | "name">>,
): WorkspacePolicy {
  return {
    ...policy,
    layers: policy.layers.map((layer) => (layer.id === id ? { ...layer, ...patch } : layer)),
  };
}

export function withActiveLayer(policy: WorkspacePolicy, id: string): WorkspacePolicy {
  if (!policy.layers.some((layer) => layer.id === id)) return policy;
  return { ...policy, activeLayerId: id };
}

export function withoutLayer(policy: WorkspacePolicy, id: string): WorkspacePolicy {
  if (policy.layers.length <= 1 || policy.activeLayerId === id) return policy;
  return { ...policy, layers: policy.layers.filter((layer) => layer.id !== id) };
}

export function withNewRegion(
  policy: WorkspacePolicy,
  kind: WorkspaceRegionKind,
  name: string,
  dimension: string,
  bounds: Bounds3,
): WorkspacePolicy | null {
  const trimmed = validName(name);
  if (!trimmed || !dimension.trim() || !ordered(bounds)) return null;
  const isMask = kind === "mask";
  const regions = isMask ? policy.masks : policy.protectedRegions;
  if (regions.length >= (isMask ? MAX_WORKSPACE_MASKS : MAX_WORKSPACE_PROTECTED_REGIONS)) {
    return null;
  }
  const id = workspaceSlug(
    trimmed,
    regions.map((region) => region.id),
  );
  const region: WorkspaceRegion = {
    id,
    name: trimmed,
    dimension,
    bounds: {
      min: { ...bounds.min },
      max: { ...bounds.max },
    },
  };
  return isMask
    ? { ...policy, masks: [...policy.masks, region], activeMaskId: id }
    : { ...policy, protectedRegions: [...policy.protectedRegions, region] };
}

export function withoutRegion(
  policy: WorkspacePolicy,
  kind: WorkspaceRegionKind,
  id: string,
): WorkspacePolicy {
  if (kind === "mask") {
    return {
      ...policy,
      masks: policy.masks.filter((region) => region.id !== id),
      activeMaskId: policy.activeMaskId === id ? null : policy.activeMaskId,
    };
  }
  return {
    ...policy,
    protectedRegions: policy.protectedRegions.filter((region) => region.id !== id),
  };
}

export function withActiveMask(policy: WorkspacePolicy, id: string | null): WorkspacePolicy {
  if (id !== null && !policy.masks.some((mask) => mask.id === id)) return policy;
  return { ...policy, activeMaskId: id };
}
