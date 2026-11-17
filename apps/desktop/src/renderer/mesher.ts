import { isAirName, styleForBlock, tileUv, type BlockStyle } from "@/renderer/assets";
import { cachedBoxShapeFor } from "@/renderer/shape";
import type { ChunkPos } from "@/renderer/types";
import {
  chunkKey,
  localIndex,
  readPackedIndex,
  type DecodedSection,
  type RenderWorld,
} from "@/renderer/world";

export interface MeshBuffers {
  positions: Float32Array;
  normals: Float32Array;
  uvs: Float32Array;
  colors: Float32Array;
  indices: Uint32Array;
}

export interface ChunkMeshData {
  chunk: ChunkPos;
  opaque: MeshBuffers;
  cutout: MeshBuffers;
  translucent: MeshBuffers;
  faceCount: number;
}

export type MutableMeshBuffers = {
  positions: number[];
  normals: number[];
  uvs: number[];
  colors: number[];
  indices: number[];
};

export function emptyBuffers(): MutableMeshBuffers {
  return { positions: [], normals: [], uvs: [], colors: [], indices: [] };
}

function emptyMesh(): MeshBuffers {
  return {
    positions: new Float32Array(0),
    normals: new Float32Array(0),
    uvs: new Float32Array(0),
    colors: new Float32Array(0),
    indices: new Uint32Array(0),
  };
}

export function emptyChunkMesh(chunk: ChunkPos): ChunkMeshData {
  return {
    chunk,
    opaque: emptyMesh(),
    cutout: emptyMesh(),
    translucent: emptyMesh(),
    faceCount: 0,
  };
}

type Vec3 = readonly [number, number, number];

interface FaceDefinition {
  normal: Vec3;
  shade: number;
  corners: readonly [Vec3, Vec3, Vec3, Vec3];
}

// Counter-clockwise winding when viewed from outside the cube.
const FACES: readonly FaceDefinition[] = [
  {
    normal: [1, 0, 0],
    shade: 0.66,
    corners: [
      [1, 0, 0],
      [1, 1, 0],
      [1, 1, 1],
      [1, 0, 1],
    ],
  },
  {
    normal: [-1, 0, 0],
    shade: 0.66,
    corners: [
      [0, 0, 1],
      [0, 1, 1],
      [0, 1, 0],
      [0, 0, 0],
    ],
  },
  {
    normal: [0, 1, 0],
    shade: 1.0,
    corners: [
      [0, 1, 0],
      [0, 1, 1],
      [1, 1, 1],
      [1, 1, 0],
    ],
  },
  {
    normal: [0, -1, 0],
    shade: 0.52,
    corners: [
      [0, 0, 0],
      [1, 0, 0],
      [1, 0, 1],
      [0, 0, 1],
    ],
  },
  {
    normal: [0, 0, 1],
    shade: 0.84,
    corners: [
      [0, 0, 1],
      [1, 0, 1],
      [1, 1, 1],
      [0, 1, 1],
    ],
  },
  {
    normal: [0, 0, -1],
    shade: 0.84,
    corners: [
      [1, 0, 0],
      [0, 0, 0],
      [0, 1, 0],
      [1, 1, 0],
    ],
  },
];

const FACE_OFFSETS: readonly Vec3[] = FACES.map((face) => face.normal);

function shouldRenderFace(
  currentName: string,
  currentStyle: BlockStyle,
  neighborName: string | null,
): boolean {
  if (neighborName === null) return true; // unloaded space: keep the face visible
  if (isAirName(neighborName)) return true;
  const neighborStyle = styleForBlock(neighborName);
  if (neighborStyle.opaque) return false;
  if (currentStyle.category !== "opaque" && currentName === neighborName) return false;
  return true;
}

function neighborName(
  world: RenderWorld,
  section: DecodedSection,
  paletteIndex: number,
  localX: number,
  localY: number,
  localZ: number,
  face: Vec3,
  globalX: number,
  globalY: number,
  globalZ: number,
): string | null {
  const nx = localX + face[0];
  const ny = localY + face[1];
  const nz = localZ + face[2];
  if (nx >= 0 && nx < 16 && ny >= 0 && ny < 16 && nz >= 0 && nz < 16) {
    if (section.palette.length === 1) return section.palette[0]?.name ?? "minecraft:air";
    const index = readPackedIndex(section.data, section.bitsPerIndex, localIndex(nx, ny, nz));
    return section.palette[index]?.name ?? null;
  }
  void paletteIndex;
  return (
    world.blockAt({ x: globalX + face[0], y: globalY + face[1], z: globalZ + face[2] })?.name ??
    null
  );
}

function pushQuad(
  buffers: MutableMeshBuffers,
  definition: FaceDefinition,
  originX: number,
  originY: number,
  originZ: number,
  sizeX: number,
  sizeY: number,
  sizeZ: number,
  style: BlockStyle,
): void {
  const [u0, v0, u1, v1] = tileUv(style.tile);
  const uvOrder: Array<[number, number]> = [
    [u0, v1],
    [u0, v0],
    [u1, v0],
    [u1, v1],
  ];
  const baseVertex = buffers.positions.length / 3;
  for (let corner = 0; corner < 4; corner += 1) {
    const offset = definition.corners[corner]!;
    buffers.positions.push(
      originX + offset[0] * sizeX,
      originY + offset[1] * sizeY,
      originZ + offset[2] * sizeZ,
    );
    buffers.normals.push(definition.normal[0], definition.normal[1], definition.normal[2]);
    const uv = uvOrder[corner]!;
    buffers.uvs.push(uv[0], uv[1]);
    buffers.colors.push(
      style.tint[0] * definition.shade,
      style.tint[1] * definition.shade,
      style.tint[2] * definition.shade,
    );
  }
  // Two counter-clockwise triangles per quad. Without an index buffer, 4 vertices per
  // face would be interpreted as unrelated triangles and produce stretched spikes.
  buffers.indices.push(baseVertex, baseVertex + 1, baseVertex + 2);
  buffers.indices.push(baseVertex, baseVertex + 2, baseVertex + 3);
}

function finalize(buffers: MutableMeshBuffers): MeshBuffers {
  return {
    positions: Float32Array.from(buffers.positions),
    normals: Float32Array.from(buffers.normals),
    uvs: Float32Array.from(buffers.uvs),
    colors: Float32Array.from(buffers.colors),
    indices: Uint32Array.from(buffers.indices),
  };
}

/**
 * Builds merged chunk geometry with hidden-face culling. One geometry per render
 * category per chunk; never one mesh per block. Greedy rectangle merging can be added
 * later behind the same buffers contract (see ADR-003 and M3 profiling notes).
 */
export function buildChunkMesh(world: RenderWorld, chunkPos: ChunkPos): ChunkMeshData {
  const chunk = world.chunk(chunkPos);
  if (!chunk) return emptyChunkMesh(chunkPos);
  const opaque = emptyBuffers();
  const cutout = emptyBuffers();
  const translucent = emptyBuffers();
  let faceCount = 0;

  for (const section of chunk.sections.values()) {
    if (section.nonAir === 0) continue;
    const styles = section.palette.map((state) => styleForBlock(state.name));
    const baseY = section.sectionY * 16;
    for (let localY = 0; localY < 16; localY += 1) {
      const globalY = baseY + localY;
      for (let localZ = 0; localZ < 16; localZ += 1) {
        const globalZ = chunkPos.z * 16 + localZ;
        for (let localX = 0; localX < 16; localX += 1) {
          const paletteIndex =
            section.palette.length === 1
              ? 0
              : readPackedIndex(
                  section.data,
                  section.bitsPerIndex,
                  localIndex(localX, localY, localZ),
                );
          const state = section.palette[paletteIndex];
          if (!state || isAirName(state.name)) continue;
          const style = styles[paletteIndex]!;
          const buffers =
            style.category === "translucent"
              ? translucent
              : style.category === "cutout"
                ? cutout
                : opaque;
          const globalX = chunkPos.x * 16 + localX;
          const shape = cachedBoxShapeFor(state);

          if (shape) {
            // Approximate non-cube models: every box face is emitted. Culling these
            // against neighbours would be wrong because the shapes only partially
            // occupy their cell (ADR-009).
            for (const box of shape) {
              const sizeX = box.max[0] - box.min[0];
              const sizeY = box.max[1] - box.min[1];
              const sizeZ = box.max[2] - box.min[2];
              for (const definition of FACES) {
                pushQuad(
                  buffers,
                  definition,
                  globalX + box.min[0],
                  globalY + box.min[1],
                  globalZ + box.min[2],
                  sizeX,
                  sizeY,
                  sizeZ,
                  style,
                );
                faceCount += 1;
              }
            }
            continue;
          }

          for (let faceIndex = 0; faceIndex < FACES.length; faceIndex += 1) {
            const definition = FACES[faceIndex]!;
            const neighbor = neighborName(
              world,
              section,
              paletteIndex,
              localX,
              localY,
              localZ,
              FACE_OFFSETS[faceIndex]!,
              globalX,
              globalY,
              globalZ,
            );
            if (!shouldRenderFace(state.name, style, neighbor)) continue;
            pushQuad(buffers, definition, globalX, globalY, globalZ, 1, 1, 1, style);
            faceCount += 1;
          }
        }
      }
    }
  }

  return {
    chunk: { ...chunkPos },
    opaque: finalize(opaque),
    cutout: finalize(cutout),
    translucent: finalize(translucent),
    faceCount,
  };
}

export function buildAllChunkMeshes(world: RenderWorld): Map<string, ChunkMeshData> {
  const meshes = new Map<string, ChunkMeshData>();
  for (const chunk of world.loadedChunks()) {
    meshes.set(chunkKey(chunk), buildChunkMesh(world, chunk));
  }
  return meshes;
}
