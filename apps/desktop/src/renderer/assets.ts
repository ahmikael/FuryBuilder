/**
 * Placeholder asset source. FuryBuilder ships original, procedurally generated textures;
 * no Minecraft assets are bundled or downloaded here. The block registry (Rust) and the
 * asset source are intentionally separate: a missing texture never changes block data.
 */

export type RenderCategory = "opaque" | "cutout" | "translucent";

export interface BlockStyle {
  tile: number;
  category: RenderCategory;
  tint: [number, number, number];
  opaque: boolean;
}

export const ATLAS_COLUMNS = 8;
export const ATLAS_TILE_PIXELS = 16;
export const ATLAS_SIZE = ATLAS_COLUMNS * ATLAS_TILE_PIXELS;

export const TILE = {
  stone: 0,
  dirt: 1,
  grass: 2,
  sand: 3,
  planks: 4,
  log: 5,
  leaves: 6,
  glass: 7,
  water: 8,
  bricks: 9,
  stoneBricks: 10,
  deepslate: 11,
  netherrack: 12,
  snow: 13,
  obsidian: 14,
  gold: 15,
  seaLantern: 16,
  redWool: 17,
  whiteWool: 18,
  torch: 19,
  defaultTile: 20,
} as const;

const EXACT_TILES: Record<string, number> = {
  "minecraft:stone": TILE.stone,
  "minecraft:smooth_stone": TILE.stone,
  "minecraft:cobblestone": TILE.stone,
  "minecraft:andesite": TILE.stone,
  "minecraft:polished_andesite": TILE.stone,
  "minecraft:diorite": TILE.stone,
  "minecraft:granite": TILE.stone,
  "minecraft:tuff": TILE.stone,
  "minecraft:calcite": TILE.stone,
  "minecraft:clay": TILE.stone,
  "minecraft:dirt": TILE.dirt,
  "minecraft:podzol": TILE.dirt,
  "minecraft:mycelium": TILE.dirt,
  "minecraft:grass_block": TILE.grass,
  "minecraft:grass": TILE.grass,
  "minecraft:sand": TILE.sand,
  "minecraft:red_sand": TILE.sand,
  "minecraft:sandstone": TILE.sand,
  "minecraft:oak_planks": TILE.planks,
  "minecraft:spruce_planks": TILE.planks,
  "minecraft:birch_planks": TILE.planks,
  "minecraft:dark_oak_planks": TILE.planks,
  "minecraft:oak_stairs": TILE.planks,
  "minecraft:stone_stairs": TILE.stoneBricks,
  "minecraft:oak_slab": TILE.planks,
  "minecraft:stone_slab": TILE.stoneBricks,
  "minecraft:oak_fence": TILE.planks,
  "minecraft:oak_door": TILE.planks,
  "minecraft:oak_trapdoor": TILE.planks,
  "minecraft:chest": TILE.planks,
  "minecraft:crafting_table": TILE.planks,
  "minecraft:bookshelf": TILE.planks,
  "minecraft:barrel": TILE.planks,
  "minecraft:campfire": TILE.log,
  "minecraft:furnace": TILE.stone,
  "minecraft:oak_log": TILE.log,
  "minecraft:spruce_log": TILE.log,
  "minecraft:birch_log": TILE.log,
  "minecraft:dark_oak_log": TILE.log,
  "minecraft:oak_leaves": TILE.leaves,
  "minecraft:spruce_leaves": TILE.leaves,
  "minecraft:azalea_leaves": TILE.leaves,
  "minecraft:flowering_azalea_leaves": TILE.leaves,
  "minecraft:glass": TILE.glass,
  "minecraft:glass_pane": TILE.glass,
  "minecraft:water": TILE.water,
  "minecraft:ice": TILE.water,
  "minecraft:packed_ice": TILE.water,
  "minecraft:blue_ice": TILE.water,
  "minecraft:bricks": TILE.bricks,
  "minecraft:stone_bricks": TILE.stoneBricks,
  "minecraft:mossy_stone_bricks": TILE.stoneBricks,
  "minecraft:deepslate": TILE.deepslate,
  "minecraft:cobbled_deepslate": TILE.deepslate,
  "minecraft:deepslate_bricks": TILE.deepslate,
  "minecraft:netherrack": TILE.netherrack,
  "minecraft:snow_block": TILE.snow,
  "minecraft:snow": TILE.snow,
  "minecraft:obsidian": TILE.obsidian,
  "minecraft:gold_block": TILE.gold,
  "minecraft:sea_lantern": TILE.seaLantern,
  "minecraft:red_wool": TILE.redWool,
  "minecraft:white_wool": TILE.whiteWool,
  "minecraft:torch": TILE.torch,
  "minecraft:wall_torch": TILE.torch,
};

const CUTOUT_NAMES = new Set([
  "minecraft:oak_leaves",
  "minecraft:spruce_leaves",
  "minecraft:azalea_leaves",
  "minecraft:flowering_azalea_leaves",
  "minecraft:torch",
  "minecraft:wall_torch",
  "minecraft:moss_carpet",
  "minecraft:grass",
]);

const TRANSLUCENT_NAMES = new Set([
  "minecraft:water",
  "minecraft:glass",
  "minecraft:glass_pane",
  "minecraft:ice",
  "minecraft:packed_ice",
  "minecraft:blue_ice",
]);

const styleCache = new Map<string, BlockStyle>();

export function tileForBlock(name: string): number {
  const exact = EXACT_TILES[name];
  if (exact !== undefined) return exact;
  if (name.includes("leaves")) return TILE.leaves;
  if (name.includes("glass")) return TILE.glass;
  if (name.includes("water")) return TILE.water;
  if (name.includes("brick")) return TILE.bricks;
  if (
    name.includes("planks") ||
    name.includes("stairs") ||
    name.includes("slab") ||
    name.includes("fence") ||
    name.includes("door") ||
    name.includes("trapdoor") ||
    name.includes("chest") ||
    name.includes("barrel") ||
    name.includes("bookshelf") ||
    name.includes("crafting_table")
  ) {
    return TILE.planks;
  }
  if (name.includes("log")) return TILE.log;
  if (name.includes("sand")) return TILE.sand;
  if (name.includes("snow")) return TILE.snow;
  if (name.includes("wool") || name.includes("concrete")) return TILE.whiteWool;
  if (name.includes("slate")) return TILE.deepslate;
  if (name.includes("nether")) return TILE.netherrack;
  return TILE.defaultTile;
}

export function styleForBlock(name: string): BlockStyle {
  const cached = styleCache.get(name);
  if (cached) return cached;
  const category: RenderCategory = TRANSLUCENT_NAMES.has(name)
    ? "translucent"
    : CUTOUT_NAMES.has(name)
      ? "cutout"
      : "opaque";
  const style: BlockStyle = {
    tile: tileForBlock(name),
    category,
    tint: [1, 1, 1],
    opaque: category === "opaque" && name !== "minecraft:air",
  };
  styleCache.set(name, style);
  return style;
}

export function isAirName(name: string | null | undefined): boolean {
  return (
    !name ||
    name === "minecraft:air" ||
    name === "minecraft:cave_air" ||
    name === "minecraft:void_air"
  );
}

/** Approximate average colour per placeholder tile, used only for capture legends. */
const TILE_COLORS: Record<number, string> = {
  [TILE.stone]: "#8b8b8b",
  [TILE.dirt]: "#6d4c33",
  [TILE.grass]: "#5f8f3e",
  [TILE.sand]: "#d8c68f",
  [TILE.planks]: "#a97c4b",
  [TILE.log]: "#6b4f2f",
  [TILE.leaves]: "#3f6b2e",
  [TILE.glass]: "#c8e8f0",
  [TILE.water]: "#3a6ed6",
  [TILE.bricks]: "#8d453d",
  [TILE.stoneBricks]: "#8b8b8b",
  [TILE.deepslate]: "#4d4d52",
  [TILE.netherrack]: "#6d3434",
  [TILE.snow]: "#eef2f5",
  [TILE.obsidian]: "#211a33",
  [TILE.gold]: "#e8c447",
  [TILE.seaLantern]: "#bfe6d8",
  [TILE.redWool]: "#a43131",
  [TILE.whiteWool]: "#e8e8e8",
  [TILE.torch]: "#ffcf4d",
  [TILE.defaultTile]: "#b05fa0",
};

export function tileColor(tile: number): string {
  return TILE_COLORS[tile] ?? TILE_COLORS[TILE.defaultTile]!;
}

export function blockColor(name: string): string {
  return tileColor(styleForBlock(name).tile);
}

function mulberry32(seed: number): () => number {
  let state = seed >>> 0;
  return () => {
    state = (state + 0x6d2b79f5) >>> 0;
    let t = state;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

function shade(hex: string, amount: number): string {
  const value = Number.parseInt(hex.slice(1), 16);
  const r = Math.max(0, Math.min(255, Math.round(((value >> 16) & 255) * amount)));
  const g = Math.max(0, Math.min(255, Math.round(((value >> 8) & 255) * amount)));
  const b = Math.max(0, Math.min(255, Math.round((value & 255) * amount)));
  return `rgb(${r},${g},${b})`;
}

function noiseTile(
  ctx: CanvasRenderingContext2D,
  x0: number,
  y0: number,
  base: string,
  seed: number,
  strength = 0.18,
): void {
  const random = mulberry32(seed);
  ctx.fillStyle = base;
  ctx.fillRect(x0, y0, ATLAS_TILE_PIXELS, ATLAS_TILE_PIXELS);
  for (let y = 0; y < ATLAS_TILE_PIXELS; y += 1) {
    for (let x = 0; x < ATLAS_TILE_PIXELS; x += 1) {
      if (random() < 0.55) {
        const amount = 1 - random() * strength;
        ctx.fillStyle = shade(base, amount);
        ctx.fillRect(x0 + x, y0 + y, 1, 1);
      }
    }
  }
}

function paintTile(ctx: CanvasRenderingContext2D, tile: number, x0: number, y0: number): void {
  const random = mulberry32(tile * 7919 + 13);
  switch (tile) {
    case TILE.stone:
      noiseTile(ctx, x0, y0, "#8b8b8b", 1, 0.12);
      break;
    case TILE.dirt:
      noiseTile(ctx, x0, y0, "#6d4c33", 2, 0.25);
      break;
    case TILE.grass:
      noiseTile(ctx, x0, y0, "#5f8f3e", 3, 0.3);
      for (let i = 0; i < 18; i += 1) {
        ctx.fillStyle = i % 2 === 0 ? "#6fa348" : "#4c7531";
        ctx.fillRect(x0 + Math.floor(random() * 16), y0 + Math.floor(random() * 16), 1, 2);
      }
      break;
    case TILE.sand:
      noiseTile(ctx, x0, y0, "#d8c68f", 4, 0.12);
      break;
    case TILE.planks:
      noiseTile(ctx, x0, y0, "#a97c4b", 5, 0.16);
      ctx.fillStyle = "#7d5a35";
      for (let y = 3; y < 16; y += 4) ctx.fillRect(x0, y0 + y, 16, 1);
      break;
    case TILE.log:
      noiseTile(ctx, x0, y0, "#6b4f2f", 6, 0.3);
      ctx.fillStyle = "#513a21";
      for (let x = 1; x < 16; x += 4) ctx.fillRect(x0 + x, y0, 1, 16);
      break;
    case TILE.leaves:
      ctx.clearRect(x0, y0, 16, 16);
      for (let y = 0; y < 16; y += 1) {
        for (let x = 0; x < 16; x += 1) {
          if (random() < 0.82) {
            ctx.fillStyle = shade("#3f6b2e", 0.75 + random() * 0.5);
            ctx.fillRect(x0 + x, y0 + y, 1, 1);
          }
        }
      }
      break;
    case TILE.glass:
      ctx.clearRect(x0, y0, 16, 16);
      ctx.fillStyle = "rgba(200,232,240,0.28)";
      ctx.fillRect(x0, y0, 16, 16);
      ctx.strokeStyle = "rgba(230,250,255,0.75)";
      ctx.strokeRect(x0 + 0.5, y0 + 0.5, 15, 15);
      ctx.fillStyle = "rgba(255,255,255,0.45)";
      ctx.fillRect(x0 + 2, y0 + 2, 5, 1);
      break;
    case TILE.water:
      ctx.clearRect(x0, y0, 16, 16);
      ctx.fillStyle = "rgba(58,110,214,0.72)";
      ctx.fillRect(x0, y0, 16, 16);
      ctx.fillStyle = "rgba(110,160,255,0.35)";
      for (let y = 2; y < 16; y += 5) ctx.fillRect(x0, y0 + y, 16, 1);
      break;
    case TILE.bricks:
      ctx.fillStyle = "#8d453d";
      ctx.fillRect(x0, y0, 16, 16);
      ctx.fillStyle = "#cbb9a8";
      for (let y = 0; y < 16; y += 4) ctx.fillRect(x0, y0 + y, 16, 1);
      for (let y = 0; y < 16; y += 8) {
        ctx.fillRect(x0 + 8, y0 + y, 1, 4);
        ctx.fillRect(x0, y0 + y + 4, 1, 4);
      }
      break;
    case TILE.stoneBricks:
      ctx.fillStyle = "#8b8b8b";
      ctx.fillRect(x0, y0, 16, 16);
      ctx.fillStyle = "#6f6f6f";
      for (let y = 0; y < 16; y += 8) ctx.fillRect(x0, y0 + y, 16, 1);
      for (let y = 0; y < 16; y += 8) {
        ctx.fillRect(x0 + 8, y0 + y, 1, 8);
        ctx.fillRect(x0, y0 + y + 4, 1, 4);
      }
      break;
    case TILE.deepslate:
      noiseTile(ctx, x0, y0, "#4d4d52", 7, 0.15);
      break;
    case TILE.netherrack:
      noiseTile(ctx, x0, y0, "#6d3434", 8, 0.25);
      break;
    case TILE.snow:
      noiseTile(ctx, x0, y0, "#eef2f5", 9, 0.06);
      break;
    case TILE.obsidian:
      noiseTile(ctx, x0, y0, "#211a33", 10, 0.2);
      break;
    case TILE.gold:
      noiseTile(ctx, x0, y0, "#e8c447", 11, 0.12);
      break;
    case TILE.seaLantern:
      ctx.fillStyle = "#bfe6d8";
      ctx.fillRect(x0, y0, 16, 16);
      ctx.fillStyle = "#eafff7";
      ctx.fillRect(x0 + 3, y0 + 3, 4, 4);
      ctx.fillRect(x0 + 9, y0 + 9, 4, 4);
      break;
    case TILE.redWool:
      noiseTile(ctx, x0, y0, "#a43131", 12, 0.1);
      break;
    case TILE.whiteWool:
      noiseTile(ctx, x0, y0, "#e8e8e8", 13, 0.08);
      break;
    case TILE.torch:
      ctx.clearRect(x0, y0, 16, 16);
      ctx.fillStyle = "#6b4f2f";
      ctx.fillRect(x0 + 7, y0 + 6, 2, 10);
      ctx.fillStyle = "#ffcf4d";
      ctx.fillRect(x0 + 7, y0 + 3, 2, 3);
      ctx.fillStyle = "#ff9d2e";
      ctx.fillRect(x0 + 6, y0 + 4, 4, 2);
      break;
    default:
      noiseTile(ctx, x0, y0, "#b05fa0", 14, 0.2);
      ctx.fillStyle = "#7d3f70";
      ctx.fillRect(x0, y0, 8, 8);
      ctx.fillRect(x0 + 8, y0 + 8, 8, 8);
      break;
  }
}

/** Builds the open placeholder atlas (no Mojang assets). */
export function createPlaceholderAtlasCanvas(): HTMLCanvasElement {
  const canvas = document.createElement("canvas");
  canvas.width = ATLAS_SIZE;
  canvas.height = ATLAS_SIZE;
  const ctx = canvas.getContext("2d");
  if (!ctx) throw new Error("2D canvas context is unavailable");
  ctx.imageSmoothingEnabled = false;
  for (let tile = 0; tile < ATLAS_COLUMNS * ATLAS_COLUMNS; tile += 1) {
    const column = tile % ATLAS_COLUMNS;
    const row = Math.floor(tile / ATLAS_COLUMNS);
    paintTile(ctx, tile, column * ATLAS_TILE_PIXELS, row * ATLAS_TILE_PIXELS);
  }
  return canvas;
}

export function tileUv(tile: number): [number, number, number, number] {
  const column = tile % ATLAS_COLUMNS;
  const row = Math.floor(tile / ATLAS_COLUMNS);
  const size = 1 / ATLAS_COLUMNS;
  const inset = 0.5 / ATLAS_SIZE;
  return [
    column * size + inset,
    row * size + inset,
    (column + 1) * size - inset,
    (row + 1) * size - inset,
  ];
}
