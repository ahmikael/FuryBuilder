import type {
  BlockState,
  ChunkSnapshot,
  ChunkPos,
  RenderSnapshot,
  SectionSnapshot,
} from "@/renderer/types";
import { floorDiv, localIndex, modFloor } from "@/renderer/world";

const SECTION_VOLUME = 16 * 16 * 16;
const AIR_NAME = "minecraft:air";

function stateOf(name: string): BlockState {
  return { name, properties: {} };
}

function bitsFor(paletteLength: number): number {
  if (paletteLength <= 1) return 0;
  return Math.ceil(Math.log2(paletteLength));
}

function writePackedIndex(data: Uint32Array, bits: number, index: number, value: number): void {
  if (bits === 0) return;
  const bit = index * bits;
  const word = Math.floor(bit / 32);
  const offset = bit % 32;
  const lowerBits = Math.min(bits, 32 - offset);
  const lowerMask = (1 << lowerBits) - 1;
  data[word] = ((data[word] ?? 0) & ~(lowerMask << offset)) | ((value & lowerMask) << offset);
  if (lowerBits < bits) {
    const upperBits = bits - lowerBits;
    const upperMask = (1 << upperBits) - 1;
    data[word + 1] = ((data[word + 1] ?? 0) & ~upperMask) | ((value >>> lowerBits) & upperMask);
  }
}

/**
 * Test/demo-only snapshot builder. Production data always comes from the Rust core; this
 * helper encodes the exact same palette format for unit tests and the offline preview.
 */
export class SnapshotBuilder {
  private chunks = new Map<string, Map<number, Map<number, string>>>();

  setBlock(x: number, y: number, z: number, name: string): this {
    const cx = floorDiv(x, 16);
    const cz = floorDiv(z, 16);
    const sectionY = floorDiv(y, 16);
    const key = `${cx},${cz}`;
    let chunk = this.chunks.get(key);
    if (!chunk) {
      chunk = new Map();
      this.chunks.set(key, chunk);
    }
    let section = chunk.get(sectionY);
    if (!section) {
      section = new Map();
      chunk.set(sectionY, section);
    }
    section.set(localIndex(x, y, z), name);
    return this;
  }

  setRegion(min: [number, number, number], max: [number, number, number], name: string): this {
    for (let x = min[0]; x <= max[0]; x += 1) {
      for (let y = min[1]; y <= max[1]; y += 1) {
        for (let z = min[2]; z <= max[2]; z += 1) {
          this.setBlock(x, y, z, name);
        }
      }
    }
    return this;
  }

  hasChunk(pos: ChunkPos): boolean {
    return this.chunks.has(`${pos.x},${pos.z}`);
  }

  build(dimension = "minecraft:overworld", revision = 1, radius = 1): RenderSnapshot {
    const chunks: ChunkSnapshot[] = [];
    for (const [key, sections] of this.chunks) {
      const [cx, cz] = key.split(",").map(Number) as [number, number];
      const sectionSnapshots: SectionSnapshot[] = [];
      for (const [sectionY, blocks] of [...sections].sort((a, b) => a[0] - b[0])) {
        if (blocks.size === 0) continue;
        const entries = new Array<string>(SECTION_VOLUME).fill(AIR_NAME);
        for (const [index, name] of blocks) entries[index] = name;
        const distinct = new Set(entries);
        if (distinct.size === 1 && distinct.has(AIR_NAME)) continue;
        const paletteNames = [AIR_NAME, ...[...distinct].filter((name) => name !== AIR_NAME)];
        const palette = paletteNames.map(stateOf);
        const paletteIndex = new Map(paletteNames.map((name, index) => [name, index]));
        const bits = bitsFor(palette.length);
        const data = new Uint32Array(Math.ceil((SECTION_VOLUME * bits) / 32));
        if (bits > 0) {
          for (let index = 0; index < SECTION_VOLUME; index += 1) {
            const value = paletteIndex.get(entries[index] ?? AIR_NAME) ?? 0;
            writePackedIndex(data, bits, index, value);
          }
        }
        sectionSnapshots.push({
          sectionY,
          palette,
          data: [...data],
          bitsPerIndex: bits,
          nonAir: entries.filter((name) => name !== AIR_NAME).length,
        });
      }
      chunks.push({ chunk: { x: cx, z: cz }, loaded: true, sections: sectionSnapshots });
    }
    for (let dx = -radius; dx <= radius; dx += 1) {
      for (let dz = -radius; dz <= radius; dz += 1) {
        const pos = { x: dx, z: dz };
        if (!chunks.some((chunk) => chunk.chunk.x === dx && chunk.chunk.z === dz)) {
          chunks.push({ chunk: pos, loaded: false, sections: [] });
        }
      }
    }
    chunks.sort((a, b) => a.chunk.x - b.chunk.x || a.chunk.z - b.chunk.z);
    return { dimension, revision, center: { x: 0, z: 0 }, radius, chunks };
  }
}

/** Snapshot from an explicit list of blocks, for tests. */
export function snapshotFromBlocks(
  blocks: Array<{ x: number; y: number; z: number; name: string }>,
  radius = 0,
): RenderSnapshot {
  const builder = new SnapshotBuilder();
  for (const block of blocks) builder.setBlock(block.x, block.y, block.z, block.name);
  return builder.build("minecraft:overworld", 1, radius);
}

export { modFloor };
