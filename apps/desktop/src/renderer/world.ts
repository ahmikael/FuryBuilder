import type {
  BlockPosition,
  BlockState,
  ChunkPos,
  ChunkSnapshot,
  RenderSnapshot,
  SectionSnapshot,
} from "@/renderer/types";
import { isAirName } from "@/renderer/assets";

export const CHUNK_SIZE = 16;
export const SECTION_SIZE = 16;

export const AIR: BlockState = { name: "minecraft:air", properties: {} };

export function floorDiv(value: number, divisor: number): number {
  return Math.floor(value / divisor);
}

export function modFloor(value: number, divisor: number): number {
  const remainder = value % divisor;
  return remainder < 0 ? remainder + divisor : remainder;
}

export function chunkKey(pos: ChunkPos): string {
  return `${pos.x},${pos.z}`;
}

export function sameChunk(a: ChunkPos, b: ChunkPos): boolean {
  return a.x === b.x && a.z === b.z;
}

/**
 * Reads one packed palette index from a u64 word array represented as little-endian
 * u32 halves. Values are at most 12 bits, so 32-bit arithmetic is sufficient.
 */
export function readPackedIndex(data: Uint32Array, bitsPerIndex: number, index: number): number {
  if (bitsPerIndex === 0) return 0;
  const bit = index * bitsPerIndex;
  const word = Math.floor(bit / 32);
  const offset = bit % 32;
  const mask = (1 << bitsPerIndex) - 1;
  let value = (data[word] ?? 0) >>> offset;
  if (offset + bitsPerIndex > 32) {
    value |= (data[word + 1] ?? 0) << (32 - offset);
  }
  return value & mask;
}

export function localIndex(x: number, y: number, z: number): number {
  return ((y & 15) * 16 + (z & 15)) * 16 + (x & 15);
}

export interface DecodedSection {
  sectionY: number;
  palette: BlockState[];
  data: Uint32Array;
  bitsPerIndex: number;
  nonAir: number;
}

export interface DecodedChunk {
  pos: ChunkPos;
  loaded: boolean;
  sections: Map<number, DecodedSection>;
}

/** CPU-side mirror of the sections the renderer currently knows about. */
export class RenderWorld {
  private chunks = new Map<string, DecodedChunk>();
  private currentRevision = 0;
  private currentDimension = "minecraft:overworld";
  private currentMinY = 0;
  private currentHeight = 256;

  get revision(): number {
    return this.currentRevision;
  }

  get dimension(): string {
    return this.currentDimension;
  }

  /** Inclusive bottom of the current dimension. */
  get minY(): number {
    return this.currentMinY;
  }

  /** Number of blocks between the inclusive bottom and exclusive top. */
  get height(): number {
    return this.currentHeight;
  }

  /** Inclusive top of the current dimension. */
  get maxY(): number {
    return this.currentMinY + this.currentHeight - 1;
  }

  setSnapshot(snapshot: RenderSnapshot): void {
    this.chunks.clear();
    this.currentRevision = snapshot.revision;
    this.currentDimension = snapshot.dimension;
    this.currentMinY = snapshot.minY;
    this.currentHeight = snapshot.height;
    for (const chunk of snapshot.chunks) {
      this.upsertChunk(chunk);
    }
  }

  setBounds(minY: number, height: number): void {
    this.currentMinY = minY;
    this.currentHeight = height;
  }

  upsertChunk(snapshot: ChunkSnapshot): void {
    const key = chunkKey(snapshot.chunk);
    if (!snapshot.loaded && snapshot.sections.length === 0) {
      this.chunks.delete(key);
      return;
    }
    const sections = new Map<number, DecodedSection>();
    for (const section of snapshot.sections) {
      sections.set(section.sectionY, decodeSection(section));
    }
    this.chunks.set(key, { pos: snapshot.chunk, loaded: snapshot.loaded, sections });
  }

  removeChunk(pos: ChunkPos): void {
    this.chunks.delete(chunkKey(pos));
  }

  setRevision(revision: number): void {
    this.currentRevision = revision;
  }

  isLoaded(pos: ChunkPos): boolean {
    return this.chunks.has(chunkKey(pos));
  }

  chunk(pos: ChunkPos): DecodedChunk | undefined {
    return this.chunks.get(chunkKey(pos));
  }

  loadedChunks(): ChunkPos[] {
    return [...this.chunks.values()].map((chunk) => chunk.pos);
  }

  missingChunks(requested: ChunkPos[]): ChunkPos[] {
    return requested.filter((pos) => !this.isLoaded(pos));
  }

  loadedChunkCount(): number {
    return this.chunks.size;
  }

  sectionCount(): number {
    let count = 0;
    for (const chunk of this.chunks.values()) count += chunk.sections.size;
    return count;
  }

  sectionAt(cx: number, sectionY: number, cz: number): DecodedSection | undefined {
    return this.chunks.get(`${cx},${cz}`)?.sections.get(sectionY);
  }

  /**
   * Reads a block by global coordinates. Returns null when the chunk is not loaded
   * (distinct from air), so meshing and picking can decide how to treat unknown space.
   */
  blockAt(pos: BlockPosition): BlockState | null {
    const cx = floorDiv(pos.x, CHUNK_SIZE);
    const cz = floorDiv(pos.z, CHUNK_SIZE);
    const chunk = this.chunks.get(`${cx},${cz}`);
    if (!chunk || !chunk.loaded) return null;
    const section = chunk.sections.get(floorDiv(pos.y, SECTION_SIZE));
    if (!section) return AIR;
    const index = localIndex(pos.x, pos.y, pos.z);
    if (section.palette.length === 1) return section.palette[0] ?? AIR;
    const paletteIndex = readPackedIndex(section.data, section.bitsPerIndex, index);
    return section.palette[paletteIndex] ?? AIR;
  }

  blockNameAt(pos: BlockPosition): string | null {
    return this.blockAt(pos)?.name ?? null;
  }

  /** Unique non-air block states in loaded chunks, bounded for an asset bundle request. */
  uniqueBlockStates(limit = 512): BlockState[] {
    const seen = new Set<string>();
    const states: BlockState[] = [];
    for (const chunk of this.chunks.values()) {
      for (const section of chunk.sections.values()) {
        for (const state of section.palette) {
          if (isAirName(state.name)) continue;
          const properties = state.properties ?? {};
          const key = `${state.name}[${Object.keys(properties)
            .sort()
            .map((name) => `${name}=${properties[name]}`)
            .join(",")}]`;
          if (seen.has(key)) continue;
          seen.add(key);
          states.push(state);
          if (states.length >= limit) return states;
        }
      }
    }
    return states;
  }

  /** Counts non-air blocks by state name across loaded chunks (for observation legends). */
  blockCountsByState(): Map<string, number> {
    const counts = new Map<string, number>();
    const add = (name: string, amount: number): void => {
      if (amount > 0) counts.set(name, (counts.get(name) ?? 0) + amount);
    };
    for (const chunk of this.chunks.values()) {
      for (const section of chunk.sections.values()) {
        if (section.palette.length === 1) {
          const state = section.palette[0];
          if (state && !isAirName(state.name)) add(state.name, section.nonAir);
          continue;
        }
        for (let index = 0; index < 4096; index += 1) {
          const paletteIndex = readPackedIndex(section.data, section.bitsPerIndex, index);
          const state = section.palette[paletteIndex];
          if (state && !isAirName(state.name)) add(state.name, 1);
        }
      }
    }
    return counts;
  }
}

export function decodeSection(snapshot: SectionSnapshot): DecodedSection {
  const palette = snapshot.palette.length > 0 ? snapshot.palette : [AIR];
  return {
    sectionY: snapshot.sectionY,
    palette,
    data: Uint32Array.from(snapshot.data),
    bitsPerIndex: snapshot.bitsPerIndex,
    nonAir: snapshot.nonAir,
  };
}
