export interface BlockState {
  name: string;
  properties?: Record<string, string>;
}

export interface ChunkPos {
  x: number;
  z: number;
}

export interface SectionSnapshot {
  sectionY: number;
  palette: BlockState[];
  /** Little-endian u32 halves of packed u64 words: [lo0, hi0, lo1, hi1, ...]. */
  data: number[];
  bitsPerIndex: number;
  nonAir: number;
}

export interface ChunkSnapshot {
  chunk: ChunkPos;
  loaded: boolean;
  sections: SectionSnapshot[];
}

export interface RenderSnapshot {
  dimension: string;
  revision: number;
  center: ChunkPos;
  radius: number;
  chunks: ChunkSnapshot[];
}

export interface BlockPosition {
  x: number;
  y: number;
  z: number;
}

export interface Bounds3 {
  min: BlockPosition;
  max: BlockPosition;
}

export type GhostStyle =
  | "place"
  | "remove"
  | "brush"
  | "box"
  | "line"
  | "fill"
  | "replace"
  | "paste";

export interface BlockSelection {
  dimension: string;
  pos: BlockPosition;
  state: BlockState;
  face: [number, number, number];
}

export interface CameraPose {
  position: [number, number, number];
  target: [number, number, number];
  yaw: number;
  pitch: number;
  fov: number;
}

export interface RenderMetrics {
  fps: number;
  frameMs: number;
  drawCalls: number;
  triangles: number;
  geometries: number;
  textures: number;
  loadedChunks: number;
  renderedSections: number;
  meshCount: number;
  /** Chunks waiting for a bounded mesh build. */
  pendingMeshJobs?: number;
  /** Chunks dropped by the residency budget since the renderer started. */
  evictedChunks?: number;
}

export interface ChunkSnapshotBatch {
  dimension: string;
  revision: number;
  chunks: ChunkSnapshot[];
}
