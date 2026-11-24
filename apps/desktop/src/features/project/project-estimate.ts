import {
  formatBytes,
  type CreationMode,
  type FlatPreset,
  type JavaWorldAnalysis,
} from "@/project/types";

/** Conservative storage hint per chunk column, matching the generation preflight budget. */
export const CONSERVATIVE_CHUNK_BYTES = 24 * 1024;
const CHUNK_EDGE = 16;

export interface ProjectEstimate {
  blocks: number | null;
  chunks: number | null;
  bytes: number | null;
  /** Human-readable size, explicitly marked as an estimate where one is used. */
  sizeLabel: string;
  timeLabel: string;
  detail: string;
}

function blockCount(value: number): string {
  return new Intl.NumberFormat("en-US").format(value);
}

/**
 * Honest pre-creation estimate for the Review step. Empty and flat sources are fully known,
 * imports are bounded by the analyzed chunk count, and no mode claims a generation time it
 * cannot prove.
 */
export function estimateProjectCreation(input: {
  mode: CreationMode;
  flat: FlatPreset | null;
  flatFootprint: number;
  analysis: JavaWorldAnalysis | null;
}): ProjectEstimate {
  const { mode, flat, flatFootprint, analysis } = input;

  if (mode === "import" && analysis) {
    const overlayBytes = analysis.chunks * CONSERVATIVE_CHUNK_BYTES;
    const bytes = analysis.sourceBytes + overlayBytes;
    return {
      blocks: null,
      chunks: analysis.chunks,
      bytes,
      sizeLabel: `${formatBytes(analysis.sourceBytes)} source + about ${formatBytes(overlayBytes)} overlay`,
      timeLabel: "Copy and verification only; no generator runs",
      detail: `${analysis.chunks} chunk(s) from ${analysis.regions} region file(s). The source copy stays immutable.`,
    };
  }

  if (mode === "flat" && flat) {
    const thickness = flat.layers.reduce((total, layer) => total + layer.thickness, 0);
    const blocks = flatFootprint * flatFootprint * thickness;
    const chunks = Math.ceil(flatFootprint / CHUNK_EDGE) ** 2;
    const bytes = chunks * CONSERVATIVE_CHUNK_BYTES;
    return {
      blocks,
      chunks,
      bytes,
      sizeLabel: `${blockCount(blocks)} blocks · about ${formatBytes(bytes)}`,
      timeLabel: "Under a second",
      detail: `${flatFootprint}×${flatFootprint} platform across ${chunks} chunk column(s). This is FuryBuilder world data, not vanilla generation.`,
    };
  }

  return {
    blocks: 0,
    chunks: 0,
    bytes: 0,
    sizeLabel: "No world data yet",
    timeLabel: "Ready immediately",
    detail: "The world starts empty; blocks are added by you or a tool.",
  };
}
