import type { RenderSnapshot } from "@/renderer/types";
import { SnapshotBuilder } from "@/renderer/builders";

/**
 * Offline preview scene for browser development and the pre-Tauri milestone.
 * It is clearly labelled in the UI and is never used as project data. The desktop build
 * always requests snapshots from the Rust core.
 */
export function createDemoSnapshot(radius = 1): RenderSnapshot {
  const builder = new SnapshotBuilder();

  for (let chunkX = -1; chunkX <= 1; chunkX += 1) {
    for (let chunkZ = -1; chunkZ <= 1; chunkZ += 1) {
      for (let localX = 0; localX < 16; localX += 1) {
        for (let localZ = 0; localZ < 16; localZ += 1) {
          const x = chunkX * 16 + localX;
          const z = chunkZ * 16 + localZ;
          builder.setBlock(x, 59, z, "minecraft:deepslate");
          builder.setBlock(x, 60, z, "minecraft:stone");
          builder.setBlock(x, 61, z, "minecraft:dirt");
          builder.setBlock(x, 62, z, "minecraft:dirt");
          builder.setBlock(x, 63, z, "minecraft:dirt");
          builder.setBlock(x, 64, z, "minecraft:grass_block");
        }
      }
    }
  }

  // Pond.
  for (let x = 10; x <= 15; x += 1) {
    for (let z = 4; z <= 10; z += 1) {
      builder.setBlock(x, 64, z, "minecraft:water");
      builder.setBlock(x, 63, z, "minecraft:water");
    }
  }

  // House floor.
  builder.setRegion([2, 65, -8], [10, 65, 0], "minecraft:stone_bricks");
  // Walls.
  builder.setRegion([2, 66, -8], [10, 69, -8], "minecraft:oak_planks");
  builder.setRegion([2, 66, 0], [10, 69, 0], "minecraft:oak_planks");
  builder.setRegion([2, 66, -8], [2, 69, 0], "minecraft:oak_planks");
  builder.setRegion([10, 66, -8], [10, 69, 0], "minecraft:oak_planks");
  // Door and windows.
  builder.setBlock(6, 66, -8, "minecraft:air");
  builder.setBlock(6, 67, -8, "minecraft:air");
  builder.setBlock(6, 66, 0, "minecraft:air");
  builder.setBlock(6, 67, 0, "minecraft:air");
  for (const x of [4, 8]) {
    builder.setBlock(x, 67, -8, "minecraft:glass");
    builder.setBlock(x, 67, 0, "minecraft:glass");
  }
  for (const z of [-4, -6]) {
    builder.setBlock(2, 67, z, "minecraft:glass");
    builder.setBlock(10, 67, z, "minecraft:glass");
  }
  // Roof.
  builder.setRegion([1, 70, -9], [11, 70, 1], "minecraft:dark_oak_planks");
  builder.setRegion([1, 71, -9], [11, 71, -9], "minecraft:oak_stairs");
  builder.setRegion([1, 71, 1], [11, 71, 1], "minecraft:oak_stairs");
  builder.setRegion([1, 71, -9], [1, 71, 1], "minecraft:oak_stairs");
  builder.setRegion([11, 71, -9], [11, 71, 1], "minecraft:oak_stairs");
  // Lights and interior.
  for (const [x, z] of [
    [4, -6],
    [8, -2],
  ] as const) {
    builder.setBlock(x, 66, z, "minecraft:torch");
    builder.setBlock(x, 70, z, "minecraft:sea_lantern");
  }
  builder.setBlock(4, 66, -3, "minecraft:crafting_table");
  builder.setBlock(8, 66, -5, "minecraft:chest");

  // Trees.
  for (const [treeX, treeZ] of [
    [-6, -6],
    [-6, 6],
    [12, -10],
  ] as const) {
    for (let y = 65; y <= 67; y += 1) {
      builder.setBlock(treeX, y, treeZ, "minecraft:oak_log");
    }
    for (let dx = -2; dx <= 2; dx += 1) {
      for (let dz = -2; dz <= 2; dz += 1) {
        for (let dy = 0; dy <= 1; dy += 1) {
          if (Math.abs(dx) === 2 && Math.abs(dz) === 2) continue;
          builder.setBlock(treeX + dx, 68 + dy, treeZ + dz, "minecraft:oak_leaves");
        }
      }
    }
  }

  // A small stone path connecting the house to the pond.
  builder.setRegion([6, 64, 1], [8, 64, 4], "minecraft:stone_bricks");
  // Flower accents.
  for (const [x, z] of [
    [13, -4],
    [14, -2],
    [12, -6],
    [3, 4],
    [-4, 3],
  ] as const) {
    builder.setBlock(x, 65, z, "minecraft:torch");
  }

  return builder.build("minecraft:overworld", 1, radius);
}
