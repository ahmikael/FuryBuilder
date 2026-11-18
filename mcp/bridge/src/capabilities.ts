/** Protocol and request-context facts shared by the MCP bridge and its tests. */

export const BRIDGE_NAME = "furybuilder";
export const BRIDGE_VERSION = "0.1.0";
export const MODERN_PROTOCOL_VERSION = "2026-07-28";

export type ProtocolEra = "modern" | "legacy";

/**
 * Optional protocol extensions are not advertised until they are implemented and tested.
 * Clients must keep working through ordinary tools when they are absent.
 */
export const OPTIONAL_EXTENSIONS = {
  tasks: false,
  apps: false,
} as const;

export const BRIDGE_INSTRUCTIONS = [
  "FuryBuilder is a local Minecraft Java world editor.",
  "Every call must identify its project explicitly with project_id.",
  "The bridge never keeps a project, selection or job in the transport session.",
  "Pass expected_revision for revision-sensitive reads and writes when the tool supports it.",
  "This bridge does not expose arbitrary filesystem or shell access.",
].join(" ");

/** Rejects empty or unbounded project identifiers before a tool reaches the Core. */
export function requireProjectId(value: string): string {
  const projectId = value.trim();
  if (projectId.length === 0 || projectId.length > 128) {
    throw new Error("project_id must be between 1 and 128 characters");
  }
  return projectId;
}
