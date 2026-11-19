import {
  isAirName,
  styleForBlock,
  tileUv,
  type BlockStyle,
  type RenderCategory,
} from "@/renderer/assets";
import type {
  BundleBlock,
  BundleElement,
  BundleElementRotation,
  BundleFace,
  BundlePart,
  BundleTexture,
  FaceDirection,
} from "@/assets/render-types";
import { cachedBoxShapeFor } from "@/renderer/shape";
import type { BlockState, ChunkPos } from "@/renderer/types";
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

/** The subset of `AssetModels` needed by the mesher, kept structural to avoid a cycle. */
export interface MeshAssets {
  block(state: BlockState): BundleBlock | undefined;
  texture(key: string): BundleTexture;
  textureQuad(
    key: string,
    uv: readonly [number, number, number, number],
    rotation: number,
  ): Array<[number, number]>;
  textureHasAlpha(key: string): boolean;
  tint(state: BlockState, tintIndex: number | null): [number, number, number];
}

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

const DIRECTION_INDEX: Record<FaceDirection, number> = {
  east: 0,
  west: 1,
  up: 2,
  down: 3,
  south: 4,
  north: 5,
};

const FACE_OFFSETS: readonly Vec3[] = FACES.map((face) => face.normal);
const MODEL_SCALE = 1 / 16;
const BLOCK_CENTER: Vec3 = [8, 8, 8];

const FLUID_TEXTURES: Record<string, string> = {
  "minecraft:water": "minecraft:block/water_still",
  "minecraft:flowing_water": "minecraft:block/water_flow",
  "minecraft:lava": "minecraft:block/lava_still",
  "minecraft:flowing_lava": "minecraft:block/lava_flow",
};

function fluidTextureFor(name: string): string | null {
  return FLUID_TEXTURES[name] ?? null;
}

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
  return (
    world.blockAt({ x: globalX + face[0], y: globalY + face[1], z: globalZ + face[2] })?.name ??
    null
  );
}

function neighborOpaque(neighbor: string | null): boolean {
  return neighbor !== null && !isAirName(neighbor) && styleForBlock(neighbor).opaque;
}

function tileQuad(tile: number): Array<[number, number]> {
  const [u0, v0, u1, v1] = tileUv(tile);
  return [
    [u0, v1],
    [u0, v0],
    [u1, v0],
    [u1, v1],
  ];
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
  uvQuad: Array<[number, number]>,
  color: readonly [number, number, number],
): void {
  const baseVertex = buffers.positions.length / 3;
  for (let corner = 0; corner < 4; corner += 1) {
    const offset = definition.corners[corner]!;
    buffers.positions.push(
      originX + offset[0] * sizeX,
      originY + offset[1] * sizeY,
      originZ + offset[2] * sizeZ,
    );
    buffers.normals.push(definition.normal[0], definition.normal[1], definition.normal[2]);
    const uv = uvQuad[corner]!;
    buffers.uvs.push(uv[0], uv[1]);
    buffers.colors.push(
      color[0] * definition.shade,
      color[1] * definition.shade,
      color[2] * definition.shade,
    );
  }
  // Two counter-clockwise triangles per quad. Without an index buffer, 4 vertices per
  // face would be interpreted as unrelated triangles and produce stretched spikes.
  buffers.indices.push(baseVertex, baseVertex + 1, baseVertex + 2);
  buffers.indices.push(baseVertex, baseVertex + 2, baseVertex + 3);
}

function rotateVector(vector: Vec3, axis: string, angleDegrees: number): Vec3 {
  const radians = (angleDegrees * Math.PI) / 180;
  const cos = Math.cos(radians);
  const sin = Math.sin(radians);
  const [x, y, z] = vector;
  if (axis === "x") {
    return [x, y * cos - z * sin, y * sin + z * cos];
  }
  if (axis === "z") {
    return [x * cos - y * sin, x * sin + y * cos, z];
  }
  return [x * cos + z * sin, y, -x * sin + z * cos];
}

function rotatePoint(point: Vec3, axis: string, angleDegrees: number, origin: Vec3): Vec3 {
  if (angleDegrees === 0) return point;
  const local: Vec3 = [point[0] - origin[0], point[1] - origin[1], point[2] - origin[2]];
  const rotated = rotateVector(local, axis, angleDegrees);
  return [rotated[0] + origin[0], rotated[1] + origin[1], rotated[2] + origin[2]];
}

function transformModelPoint(
  point: Vec3,
  elementRotation: BundleElementRotation | null,
  rotationX: number,
  rotationY: number,
): Vec3 {
  let result = point;
  if (elementRotation) {
    result = rotatePoint(
      result,
      elementRotation.axis,
      elementRotation.angle,
      elementRotation.origin,
    );
  }
  result = rotatePoint(result, "x", rotationX, BLOCK_CENTER);
  result = rotatePoint(result, "y", rotationY, BLOCK_CENTER);
  return result;
}

function transformNormal(
  normal: Vec3,
  elementRotation: BundleElementRotation | null,
  rotationX: number,
  rotationY: number,
): Vec3 {
  let result = normal;
  if (elementRotation) {
    result = rotateVector(result, elementRotation.axis, elementRotation.angle);
  }
  result = rotateVector(result, "x", rotationX);
  result = rotateVector(result, "y", rotationY);
  const length = Math.hypot(result[0], result[1], result[2]) || 1;
  return [result[0] / length, result[1] / length, result[2] / length];
}

function categoryForFace(style: BlockStyle, textureHasAlpha: boolean): RenderCategory {
  if (style.category === "translucent") return "translucent";
  if (style.category === "cutout" || textureHasAlpha) return "cutout";
  return "opaque";
}

function buffersFor(
  category: RenderCategory,
  opaque: MutableMeshBuffers,
  cutout: MutableMeshBuffers,
  translucent: MutableMeshBuffers,
): MutableMeshBuffers {
  return category === "translucent" ? translucent : category === "cutout" ? cutout : opaque;
}

function pushModelFace(
  buffers: MutableMeshBuffers,
  face: BundleFace,
  element: BundleElement,
  part: BundlePart,
  state: BlockState,
  globalX: number,
  globalY: number,
  globalZ: number,
  assets: MeshAssets,
): void {
  const definition = FACES[DIRECTION_INDEX[face.direction]];
  if (!definition) return;
  const uvQuad = assets.textureQuad(face.texture, face.uv, face.rotation);
  const tint = assets.tint(state, face.tintIndex);
  const shade = element.shade ? definition.shade : 1;
  const baseVertex = buffers.positions.length / 3;
  for (let corner = 0; corner < 4; corner += 1) {
    const unit = definition.corners[corner]!;
    const local: Vec3 = [
      element.from[0] + unit[0] * (element.to[0] - element.from[0]),
      element.from[1] + unit[1] * (element.to[1] - element.from[1]),
      element.from[2] + unit[2] * (element.to[2] - element.from[2]),
    ];
    const transformed = transformModelPoint(
      local,
      element.rotation,
      part.rotationX,
      part.rotationY,
    );
    buffers.positions.push(
      globalX + transformed[0] * MODEL_SCALE,
      globalY + transformed[1] * MODEL_SCALE,
      globalZ + transformed[2] * MODEL_SCALE,
    );
    const normal = transformNormal(
      definition.normal,
      element.rotation,
      part.rotationX,
      part.rotationY,
    );
    buffers.normals.push(normal[0], normal[1], normal[2]);
    const uv = uvQuad[corner]!;
    buffers.uvs.push(uv[0], uv[1]);
    buffers.colors.push(tint[0] * shade, tint[1] * shade, tint[2] * shade);
  }
  buffers.indices.push(baseVertex, baseVertex + 1, baseVertex + 2);
  buffers.indices.push(baseVertex, baseVertex + 2, baseVertex + 3);
}

function pushMissingCube(
  opaque: MutableMeshBuffers,
  cutout: MutableMeshBuffers,
  translucent: MutableMeshBuffers,
  state: BlockState,
  style: BlockStyle,
  globalX: number,
  globalY: number,
  globalZ: number,
  neighbor: (face: Vec3) => string | null,
  assets: MeshAssets,
): number {
  const uvQuad = assets.textureQuad("__missing__", [0, 0, 16, 16], 0);
  const buffers = buffersFor(style.category, opaque, cutout, translucent);
  let faces = 0;
  for (const face of FACES) {
    if (!shouldRenderFace(state.name, style, neighbor(face.normal))) continue;
    pushQuad(buffers, face, globalX, globalY, globalZ, 1, 1, 1, uvQuad, style.tint);
    faces += 1;
  }
  return faces;
}

/**
 * Vanilla fluids are not block models. Render a full cube with the real still/flow
 * texture as a documented approximation; internal faces between equal fluids and faces
 * against opaque neighbours are skipped.
 */
function pushFluidCube(
  opaque: MutableMeshBuffers,
  cutout: MutableMeshBuffers,
  translucent: MutableMeshBuffers,
  state: BlockState,
  style: BlockStyle,
  textureKey: string,
  globalX: number,
  globalY: number,
  globalZ: number,
  neighbor: (face: Vec3) => string | null,
  assets: MeshAssets,
): number {
  const uvQuad = assets.textureQuad(textureKey, [0, 0, 16, 16], 0);
  const buffers = buffersFor(style.category, opaque, cutout, translucent);
  const tint = assets.tint(state, 0);
  let faces = 0;
  for (const face of FACES) {
    const adjacent = neighbor(face.normal);
    if (adjacent !== null && (adjacent === state.name || neighborOpaque(adjacent))) {
      continue;
    }
    pushQuad(buffers, face, globalX, globalY, globalZ, 1, 1, 1, uvQuad, tint);
    faces += 1;
  }
  return faces;
}

function finalize(buffers: MutableMeshBuffers): MeshBuffers {
  // Direct typed-array construction copies once without the generic iterator path
  // used by `Float32Array.from`. This runs for every rebuilt chunk.
  return {
    positions: new Float32Array(buffers.positions),
    normals: new Float32Array(buffers.normals),
    uvs: new Float32Array(buffers.uvs),
    colors: new Float32Array(buffers.colors),
    indices: new Uint32Array(buffers.indices),
  };
}

/**
 * Builds merged chunk geometry with hidden-face culling. One geometry per render
 * category per chunk; never one mesh per block. When a verified asset bundle is active,
 * blockstate/model elements are emitted with atlas UVs and face culling; blocks without a
 * usable model render a clearly marked missing cube. Without a bundle, the original open
 * placeholder assets and approximate box shapes are used.
 */
export function buildChunkMesh(
  world: RenderWorld,
  chunkPos: ChunkPos,
  assets?: MeshAssets | null,
): ChunkMeshData {
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
          const globalX = chunkPos.x * 16 + localX;
          const neighbor = (face: Vec3): string | null =>
            neighborName(world, section, localX, localY, localZ, face, globalX, globalY, globalZ);

          if (assets) {
            const model = assets.block(state);
            if (!model || model.missingModel || model.parts.length === 0) {
              const fluidTexture = fluidTextureFor(state.name);
              if (fluidTexture && !assets.texture(fluidTexture).missing) {
                faceCount += pushFluidCube(
                  opaque,
                  cutout,
                  translucent,
                  state,
                  style,
                  fluidTexture,
                  globalX,
                  globalY,
                  globalZ,
                  neighbor,
                  assets,
                );
                continue;
              }
              faceCount += pushMissingCube(
                opaque,
                cutout,
                translucent,
                state,
                style,
                globalX,
                globalY,
                globalZ,
                neighbor,
                assets,
              );
              continue;
            }
            for (const part of model.parts) {
              for (const element of part.elements) {
                for (const face of element.faces) {
                  const definition = FACES[DIRECTION_INDEX[face.direction]];
                  if (!definition) continue;
                  if (face.cullface) {
                    const offset = FACE_OFFSETS[DIRECTION_INDEX[face.direction]]!;
                    if (neighborOpaque(neighbor(offset))) continue;
                  }
                  const category = categoryForFace(style, assets.textureHasAlpha(face.texture));
                  pushModelFace(
                    buffersFor(category, opaque, cutout, translucent),
                    face,
                    element,
                    part,
                    state,
                    globalX,
                    globalY,
                    globalZ,
                    assets,
                  );
                  faceCount += 1;
                }
              }
            }
            continue;
          }

          const shape = cachedBoxShapeFor(state);
          const buffers =
            style.category === "translucent"
              ? translucent
              : style.category === "cutout"
                ? cutout
                : opaque;

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
                  tileQuad(style.tile),
                  style.tint,
                );
                faceCount += 1;
              }
            }
            continue;
          }

          for (let faceIndex = 0; faceIndex < FACES.length; faceIndex += 1) {
            const definition = FACES[faceIndex]!;
            const neighborNameValue = neighbor(FACE_OFFSETS[faceIndex]!);
            if (!shouldRenderFace(state.name, style, neighborNameValue)) continue;
            pushQuad(
              buffers,
              definition,
              globalX,
              globalY,
              globalZ,
              1,
              1,
              1,
              tileQuad(style.tile),
              style.tint,
            );
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

export function buildAllChunkMeshes(
  world: RenderWorld,
  assets?: MeshAssets | null,
): Map<string, ChunkMeshData> {
  const meshes = new Map<string, ChunkMeshData>();
  for (const chunk of world.loadedChunks()) {
    meshes.set(chunkKey(chunk), buildChunkMesh(world, chunk, assets));
  }
  return meshes;
}
