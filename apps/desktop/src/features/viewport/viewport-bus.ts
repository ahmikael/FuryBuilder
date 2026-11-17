import type { ChunkPos } from "@/renderer/types";

/**
 * Small typed event bridge between editing commands and the imperative viewport.
 * React never re-renders the Three.js scene; commands request chunk refreshes here.
 */
export const viewportBus = new EventTarget();

export function refreshViewportChunks(chunks: ChunkPos[]): void {
  if (chunks.length === 0) return;
  viewportBus.dispatchEvent(new CustomEvent("refresh-chunks", { detail: chunks }));
}
