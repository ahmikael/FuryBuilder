import {
  BoxGeometry,
  BufferGeometry,
  CanvasTexture,
  Color,
  DoubleSide,
  EdgesGeometry,
  Float32BufferAttribute,
  Fog,
  FrontSide,
  GridHelper,
  Group,
  LineBasicMaterial,
  LineSegments,
  Mesh,
  MeshBasicMaterial,
  NearestFilter,
  PerspectiveCamera,
  Raycaster,
  Scene,
  SRGBColorSpace,
  Vector2,
  Vector3,
  Uint32BufferAttribute,
  WebGLRenderer,
} from "three";
import type { Texture } from "three";
import { OrbitControls } from "three/addons/controls/OrbitControls.js";
import { AssetModels } from "@/assets/model-assets";
import type { AssetBundle } from "@/assets/render-types";
import { createPlaceholderAtlasCanvas } from "@/renderer/assets";
import { FlyCameraController } from "@/renderer/camera-controller";
import {
  ChunkMeshQueue,
  DEFAULT_MESH_BUDGET,
  normalizeMeshBudget,
  selectChunksToEvict,
  type MeshBudget,
} from "@/renderer/mesh-queue";
import { buildChunkMesh } from "@/renderer/mesher";
import { raycastBlocks, type RayHit } from "@/renderer/raycast";
import type {
  BlockPosition,
  Bounds3,
  CameraPose,
  ChunkPos,
  ChunkSnapshotBatch,
  GhostStyle,
  RenderMetrics,
  RenderSnapshot,
} from "@/renderer/types";
import type { CaptureResult } from "@/renderer/observation";
import { buildCaptureMetadata } from "@/renderer/observation";
import { chunkKey, floorDiv, RenderWorld } from "@/renderer/world";

export type CameraMode = "fly" | "orbit";

export interface ViewportCallbacks {
  onMetrics?: (metrics: RenderMetrics) => void;
  onCameraPose?: (pose: CameraPose) => void;
  onPick?: (hit: RayHit | null) => void;
  onHover?: (hit: RayHit | null) => void;
  onContextLost?: () => void;
  /** Reports chunks dropped by the residency budget; the caller may show a warning. */
  onChunksEvicted?: (chunks: ChunkPos[]) => void;
}

const CHUNK_COLOR = 0x687385;
const SELECTION_COLOR = 0xffb454;
const EDIT_SELECTION_COLOR = 0x7dd3fc;

const GHOST_COLORS: Record<GhostStyle, number> = {
  place: 0x4ade80,
  remove: 0xf87171,
  brush: 0xfacc15,
  box: 0x38bdf8,
  line: 0x22d3ee,
  fill: 0xfbbf24,
  replace: 0xc084fc,
  paste: 0xf472b6,
};

export class ViewportRenderer {
  readonly world = new RenderWorld();

  private readonly scene = new Scene();
  private readonly camera: PerspectiveCamera;
  private readonly renderer: WebGLRenderer;
  private readonly fly: FlyCameraController;
  private readonly orbit: OrbitControls;
  private atlas: Texture;
  private assetModels: AssetModels | null = null;
  private readonly materials: Record<"opaque" | "cutout" | "translucent", MeshBasicMaterial>;
  private readonly chunkGroup = new Group();
  private readonly borderGroup = new Group();
  private readonly borderMaterial = new LineBasicMaterial({
    color: CHUNK_COLOR,
    transparent: true,
    opacity: 0.55,
  });
  private readonly grid: GridHelper;
  private readonly selection: LineSegments;
  private readonly selectionBox: LineSegments;
  private readonly ghostFill: Mesh;
  private readonly ghostEdges: LineSegments;
  private readonly ghostMaterial: MeshBasicMaterial;
  private readonly raycaster = new Raycaster();
  private readonly chunkMeshes = new Map<string, Mesh[]>();
  private readonly requestedChunks: ChunkPos[] = [];
  private readonly meshQueue = new ChunkMeshQueue();
  private meshBudget: MeshBudget = DEFAULT_MESH_BUDGET;
  private evictedChunks = 0;
  private readonly clock = { last: performance.now() };
  private animationId = 0;
  private disposed = false;
  private frameCount = 0;
  private metricElapsed = 0;
  private fps = 0;
  private frameMs = 0;
  private lastPoseAt = 0;
  private cameraMode: CameraMode = "fly";
  private boundsVisible = true;
  private metrics: RenderMetrics = {
    fps: 0,
    frameMs: 0,
    drawCalls: 0,
    triangles: 0,
    geometries: 0,
    textures: 0,
    loadedChunks: 0,
    renderedSections: 0,
    meshCount: 0,
    pendingMeshJobs: 0,
    evictedChunks: 0,
  };

  constructor(
    private readonly container: HTMLElement,
    private callbacks: ViewportCallbacks = {},
    budget: Partial<MeshBudget> = {},
  ) {
    this.meshBudget = normalizeMeshBudget(budget);
    const width = Math.max(1, container.clientWidth);
    const height = Math.max(1, container.clientHeight);

    this.scene.background = new Color(0x15171b);
    this.scene.fog = new Fog(0x15171b, 90, 300);

    this.camera = new PerspectiveCamera(70, width / height, 0.1, 1200);

    this.renderer = new WebGLRenderer({
      antialias: true,
      alpha: false,
      powerPreference: "high-performance",
    });
    this.renderer.setPixelRatio(Math.min(window.devicePixelRatio || 1, 2));
    this.renderer.setSize(width, height, false);
    this.renderer.outputColorSpace = SRGBColorSpace;
    this.renderer.domElement.className = "absolute inset-0 block h-full w-full";
    container.appendChild(this.renderer.domElement);

    this.fly = new FlyCameraController(this.camera, this.renderer.domElement);
    this.fly.lookAt(new Vector3(2, 68, -2));

    this.orbit = new OrbitControls(this.camera, this.renderer.domElement);
    this.orbit.enabled = false;
    this.orbit.enableDamping = true;
    this.orbit.dampingFactor = 0.08;
    this.orbit.target.set(2, 66, -2);
    this.orbit.minDistance = 4;
    this.orbit.maxDistance = 300;
    this.orbit.maxPolarAngle = Math.PI * 0.495;
    this.orbit.update();

    this.atlas = new CanvasTexture(createPlaceholderAtlasCanvas());
    this.atlas.colorSpace = SRGBColorSpace;
    // Tile UVs are computed from the top-left origin; CanvasTexture defaults to a
    // bottom-left origin, which would mirror every tile into an unpainted atlas row.
    this.atlas.flipY = false;
    this.atlas.magFilter = NearestFilter;
    this.atlas.minFilter = NearestFilter;
    this.atlas.generateMipmaps = false;
    this.atlas.needsUpdate = true;

    this.materials = {
      opaque: new MeshBasicMaterial({
        map: this.atlas,
        vertexColors: true,
        side: FrontSide,
      }),
      cutout: new MeshBasicMaterial({
        map: this.atlas,
        vertexColors: true,
        alphaTest: 0.5,
        side: DoubleSide,
      }),
      translucent: new MeshBasicMaterial({
        map: this.atlas,
        vertexColors: true,
        transparent: true,
        alphaTest: 0.01,
        depthWrite: false,
        side: DoubleSide,
        opacity: 1,
      }),
    };

    this.scene.add(this.chunkGroup);
    this.scene.add(this.borderGroup);

    this.grid = new GridHelper(256, 256, CHUNK_COLOR, 0x333a45);
    this.grid.position.y = 64.04;
    const gridMaterial = this.grid.material as LineBasicMaterial;
    gridMaterial.transparent = true;
    gridMaterial.opacity = 0.28;
    gridMaterial.depthWrite = false;
    this.scene.add(this.grid);

    this.selection = new LineSegments(
      new EdgesGeometry(new BoxGeometry(1.006, 1.006, 1.006)),
      new LineBasicMaterial({
        color: SELECTION_COLOR,
        depthTest: false,
        transparent: true,
        opacity: 0.95,
      }),
    );
    this.selection.renderOrder = 20;
    this.selection.visible = false;
    this.scene.add(this.selection);

    this.selectionBox = new LineSegments(
      new EdgesGeometry(new BoxGeometry(1.002, 1.002, 1.002)),
      new LineBasicMaterial({
        color: EDIT_SELECTION_COLOR,
        depthTest: false,
        transparent: true,
        opacity: 0.95,
      }),
    );
    this.selectionBox.renderOrder = 21;
    this.selectionBox.visible = false;
    this.scene.add(this.selectionBox);

    this.ghostMaterial = new MeshBasicMaterial({
      color: GHOST_COLORS.place,
      transparent: true,
      opacity: 0.16,
      depthWrite: false,
      side: DoubleSide,
    });
    this.ghostFill = new Mesh(new BoxGeometry(1, 1, 1), this.ghostMaterial);
    this.ghostFill.renderOrder = 19;
    this.ghostFill.visible = false;
    this.scene.add(this.ghostFill);

    this.ghostEdges = new LineSegments(
      new EdgesGeometry(new BoxGeometry(1.002, 1.002, 1.002)),
      new LineBasicMaterial({
        color: GHOST_COLORS.place,
        depthTest: false,
        transparent: true,
        opacity: 0.95,
      }),
    );
    this.ghostEdges.renderOrder = 22;
    this.ghostEdges.visible = false;
    this.scene.add(this.ghostEdges);

    this.renderer.domElement.addEventListener("webglcontextlost", this.onContextLost);
    this.animationId = requestAnimationFrame(this.animate);
  }

  setCallbacks(callbacks: ViewportCallbacks): void {
    this.callbacks = callbacks;
  }

  /** True when a verified Minecraft asset bundle currently backs the chunk meshes. */
  get hasAssetBundle(): boolean {
    return this.assetModels !== null;
  }

  get assetStats() {
    return this.assetModels?.stats ?? null;
  }

  /**
   * Replaces the open placeholder atlas with a verified bundle and rebuilds loaded chunks.
   * The atlas is decoded by the WebView; malformed PNG data rejects and the caller keeps
   * the placeholder fallback.
   */
  async setAssetBundle(bundle: AssetBundle): Promise<void> {
    const image = new Image();
    image.src = `data:image/png;base64,${bundle.atlasPngBase64}`;
    await image.decode();
    const previous = this.atlas;
    this.atlas = new CanvasTexture(image);
    this.configureAtlas(this.atlas);
    for (const material of Object.values(this.materials)) {
      material.map = this.atlas;
      material.needsUpdate = true;
    }
    previous.dispose();
    this.assetModels = new AssetModels(bundle);
    this.rebuildAllChunks();
  }

  /** Returns to the original open placeholder atlas without touching world data. */
  clearAssetBundle(): void {
    if (!this.assetModels) return;
    const previous = this.atlas;
    this.atlas = new CanvasTexture(createPlaceholderAtlasCanvas());
    this.configureAtlas(this.atlas);
    for (const material of Object.values(this.materials)) {
      material.map = this.atlas;
      material.needsUpdate = true;
    }
    previous.dispose();
    this.assetModels = null;
    this.rebuildAllChunks();
  }

  private configureAtlas(atlas: Texture): void {
    atlas.colorSpace = SRGBColorSpace;
    // Model and placeholder UVs use a top-left origin; CanvasTexture defaults to a
    // bottom-left origin, which would mirror every tile into an unpainted atlas row.
    atlas.flipY = false;
    atlas.magFilter = NearestFilter;
    atlas.minFilter = NearestFilter;
    atlas.generateMipmaps = false;
    atlas.needsUpdate = true;
  }

  private cameraChunk(): ChunkPos {
    return {
      x: floorDiv(this.camera.position.x, 16),
      z: floorDiv(this.camera.position.z, 16),
    };
  }

  private enqueueChunk(pos: ChunkPos): void {
    if (!this.world.isLoaded(pos)) return;
    if (this.meshQueue.size >= this.meshBudget.maxPendingJobs) return;
    this.meshQueue.enqueue(pos);
  }

  private enqueueChunks(chunks: readonly ChunkPos[], center?: ChunkPos): void {
    if (this.meshQueue.size >= this.meshBudget.maxPendingJobs) return;
    const remaining = this.meshBudget.maxPendingJobs - this.meshQueue.size;
    this.meshQueue.enqueueAll(chunks, center, remaining);
  }

  private processMeshQueue(): void {
    if (this.meshQueue.size === 0) return;
    const jobs = this.meshQueue.take({
      maxJobs: this.meshBudget.maxJobsPerFrame,
      maxMs: this.meshBudget.maxFrameMs,
    });
    for (const pos of jobs) {
      if (this.world.isLoaded(pos)) this.rebuildChunk(pos);
    }
  }

  private flushMeshQueue(): void {
    while (this.meshQueue.size > 0) {
      const jobs = this.meshQueue.take({
        maxJobs: Number.MAX_SAFE_INTEGER,
        maxMs: Number.POSITIVE_INFINITY,
      });
      if (jobs.length === 0) break;
      for (const pos of jobs) {
        if (this.world.isLoaded(pos)) this.rebuildChunk(pos);
      }
    }
  }

  private enforceResidentBudget(): boolean {
    const excess = this.world.loadedChunkCount() - this.meshBudget.maxResidentChunks;
    if (excess <= 0) return false;
    const camera = this.cameraChunk();
    const evicted = selectChunksToEvict(this.world.loadedChunks(), camera, excess);
    if (evicted.length === 0) return false;
    const neighbours = new Map<string, ChunkPos>();
    for (const pos of evicted) {
      this.meshQueue.remove(pos);
      this.world.removeChunk(pos);
      this.removeChunkMesh(chunkKey(pos));
      for (let dx = -1; dx <= 1; dx += 1) {
        for (let dz = -1; dz <= 1; dz += 1) {
          if (dx === 0 && dz === 0) continue;
          const neighbour = { x: pos.x + dx, z: pos.z + dz };
          if (this.world.isLoaded(neighbour)) {
            neighbours.set(chunkKey(neighbour), neighbour);
          }
        }
      }
    }
    this.evictedChunks += evicted.length;
    this.enqueueChunks([...neighbours.values()], camera);
    this.callbacks.onChunksEvicted?.(evicted);
    return evicted.length > 0;
  }

  private addDirtyWithNeighbours(pos: ChunkPos, dirty: Map<string, ChunkPos>): void {
    dirty.set(chunkKey(pos), pos);
    for (let dx = -1; dx <= 1; dx += 1) {
      for (let dz = -1; dz <= 1; dz += 1) {
        const neighbour = { x: pos.x + dx, z: pos.z + dz };
        if (this.world.isLoaded(neighbour)) {
          dirty.set(chunkKey(neighbour), neighbour);
        }
      }
    }
  }

  private rebuildAllChunks(): void {
    this.meshQueue.clear();
    this.clearChunkMeshes();
    this.enqueueChunks(this.world.loadedChunks(), this.cameraChunk());
    if (this.meshQueue.size <= this.meshBudget.maxSynchronousSnapshotChunks) {
      this.flushMeshQueue();
    } else {
      this.processMeshQueue();
    }
    // The chunk set did not change (asset swap or full remesh), so the border
    // geometry is still valid and must not be rebuilt here.
  }

  setSnapshot(snapshot: RenderSnapshot): void {
    this.world.setSnapshot(snapshot);
    this.requestedChunks.length = 0;
    for (const chunk of snapshot.chunks) this.requestedChunks.push(chunk.chunk);
    this.meshQueue.clear();
    this.clearChunkMeshes();
    this.enforceResidentBudget();
    this.enqueueChunks(this.world.loadedChunks(), snapshot.center);
    if (this.world.loadedChunkCount() <= this.meshBudget.maxSynchronousSnapshotChunks) {
      this.flushMeshQueue();
    } else {
      this.processMeshQueue();
    }
    this.rebuildBorders();
  }

  /** Updates a set of chunks and rebuilds neighbours whose faces can change. */
  updateChunks(snapshot: ChunkSnapshotBatch): void {
    this.world.setRevision(snapshot.revision);
    const dirty = new Map<string, ChunkPos>();
    let chunkSetChanged = false;
    for (const chunk of snapshot.chunks) {
      const wasLoaded = this.world.isLoaded(chunk.chunk);
      this.world.upsertChunk(chunk);
      if (wasLoaded !== this.world.isLoaded(chunk.chunk)) chunkSetChanged = true;
      this.addDirtyWithNeighbours(chunk.chunk, dirty);
    }
    const evicted = this.enforceResidentBudget();
    if (dirty.size <= this.meshBudget.maxSynchronousUpdateChunks) {
      for (const pos of dirty.values()) {
        if (this.world.isLoaded(pos)) this.rebuildChunk(pos);
        else this.removeChunkMesh(chunkKey(pos));
      }
      this.processMeshQueue();
    } else {
      for (const pos of dirty.values()) {
        if (this.world.isLoaded(pos)) this.enqueueChunk(pos);
        else this.removeChunkMesh(chunkKey(pos));
      }
      this.processMeshQueue();
    }
    // Ordinary edits replace block data inside existing chunks; their borders are
    // unchanged. Rebuild only when this call actually added or evicted chunks.
    if (chunkSetChanged || evicted) this.rebuildBorders();
  }

  /** Unloads chunks (for example when the view distance shrinks) and heals neighbours. */
  removeChunks(chunks: ChunkPos[]): void {
    const dirty = new Map<string, ChunkPos>();
    let removed = false;
    for (const chunk of chunks) {
      if (!this.world.isLoaded(chunk)) continue;
      removed = true;
      this.world.removeChunk(chunk);
      this.meshQueue.remove(chunk);
      this.removeChunkMesh(chunkKey(chunk));
      for (let dx = -1; dx <= 1; dx += 1) {
        for (let dz = -1; dz <= 1; dz += 1) {
          if (dx === 0 && dz === 0) continue;
          const neighbour = { x: chunk.x + dx, z: chunk.z + dz };
          if (this.world.isLoaded(neighbour)) {
            dirty.set(chunkKey(neighbour), neighbour);
          }
        }
      }
    }
    if (dirty.size <= this.meshBudget.maxSynchronousUpdateChunks) {
      for (const pos of dirty.values()) this.rebuildChunk(pos);
      this.processMeshQueue();
    } else {
      for (const pos of dirty.values()) this.enqueueChunk(pos);
      this.processMeshQueue();
    }
    if (removed) this.rebuildBorders();
  }

  rebuildDirtyChunks(chunks: ChunkPos[]): void {
    this.enqueueChunks(chunks, this.cameraChunk());
    if (this.meshQueue.size <= this.meshBudget.maxSynchronousUpdateChunks) {
      this.flushMeshQueue();
    } else {
      this.processMeshQueue();
    }
    // Data-only refresh: the chunk set and therefore border geometry are unchanged.
  }

  setMeshBudget(budget: Partial<MeshBudget>): void {
    this.meshBudget = normalizeMeshBudget(budget);
    this.enforceResidentBudget();
    this.processMeshQueue();
  }

  getMeshBudget(): MeshBudget {
    return { ...this.meshBudget };
  }

  get pendingMeshJobs(): number {
    return this.meshQueue.size;
  }

  pickAt(clientX: number, clientY: number): RayHit | null {
    const hit = this.castAt(clientX, clientY);
    this.setSelection(hit?.pos ?? null);
    this.callbacks.onPick?.(hit);
    return hit;
  }

  /** Hover picking never changes the inspector selection or emits `onPick`. */
  hoverAt(clientX: number, clientY: number): RayHit | null {
    const hit = this.castAt(clientX, clientY);
    this.setSelection(hit?.pos ?? null);
    this.callbacks.onHover?.(hit);
    return hit;
  }

  private castAt(clientX: number, clientY: number): RayHit | null {
    const rect = this.renderer.domElement.getBoundingClientRect();
    if (rect.width === 0 || rect.height === 0) return null;
    const ndc = new Vector2(
      ((clientX - rect.left) / rect.width) * 2 - 1,
      -((clientY - rect.top) / rect.height) * 2 + 1,
    );
    this.raycaster.setFromCamera(ndc, this.camera);
    const origin = this.raycaster.ray.origin;
    const direction = this.raycaster.ray.direction;
    return raycastBlocks(
      this.world,
      [origin.x, origin.y, origin.z],
      [direction.x, direction.y, direction.z],
      480,
    );
  }

  /** Blue outline for the editable box selection. */
  setSelectionBox(bounds: Bounds3 | null): void {
    if (!bounds) {
      this.selectionBox.visible = false;
      return;
    }
    this.applyBoundsToBox(this.selectionBox, bounds);
    this.selectionBox.visible = true;
  }

  /** Coloured ghost of the operation that a click would apply. */
  setGhostBox(bounds: Bounds3 | null, kind: GhostStyle = "place"): void {
    if (!bounds) {
      this.ghostFill.visible = false;
      this.ghostEdges.visible = false;
      return;
    }
    const color = GHOST_COLORS[kind] ?? GHOST_COLORS.place;
    this.ghostMaterial.color.setHex(color);
    (this.ghostEdges.material as LineBasicMaterial).color.setHex(color);
    this.applyBoundsToBox(this.ghostFill, bounds);
    this.applyBoundsToBox(this.ghostEdges, bounds);
    this.ghostFill.visible = true;
    this.ghostEdges.visible = true;
  }

  clearEditOverlays(): void {
    this.selectionBox.visible = false;
    this.ghostFill.visible = false;
    this.ghostEdges.visible = false;
    this.selection.visible = false;
  }

  private applyBoundsToBox(object: Mesh | LineSegments, bounds: Bounds3): void {
    const width = bounds.max.x - bounds.min.x + 1;
    const height = bounds.max.y - bounds.min.y + 1;
    const depth = bounds.max.z - bounds.min.z + 1;
    object.scale.set(width, height, depth);
    object.position.set(
      bounds.min.x + width / 2,
      bounds.min.y + height / 2,
      bounds.min.z + depth / 2,
    );
    object.updateMatrixWorld();
  }

  setSelection(pos: BlockPosition | null): void {
    if (!pos) {
      this.selection.visible = false;
      return;
    }
    this.selection.position.set(pos.x + 0.5, pos.y + 0.5, pos.z + 0.5);
    this.selection.visible = true;
  }

  focusOn(pos: BlockPosition): void {
    this.fly.setPosition(pos.x + 22, pos.y + 16, pos.z + 24);
    this.fly.lookAt(new Vector3(pos.x, pos.y, pos.z));
    this.orbit.target.set(pos.x + 0.5, pos.y + 0.5, pos.z + 0.5);
  }

  /** Places both camera modes at an explicit pose (used by scripts and reproducibility tests). */
  setCameraPose(position: [number, number, number], target: [number, number, number]): void {
    this.fly.setPosition(position[0], position[1], position[2]);
    this.fly.lookAt(new Vector3(target[0], target[1], target[2]));
    this.orbit.target.set(target[0], target[1], target[2]);
    this.orbit.update();
  }

  setCameraMode(mode: CameraMode): void {
    this.cameraMode = mode;
    if (mode === "orbit") {
      const forward = this.fly.forwardVector();
      this.orbit.target.copy(this.camera.position).addScaledVector(forward, 24);
      this.orbit.enabled = true;
      this.fly.enabled = false;
    } else {
      this.orbit.enabled = false;
      this.fly.syncFromCamera();
      this.fly.enabled = true;
    }
  }

  getCameraMode(): CameraMode {
    return this.cameraMode;
  }

  setGridVisible(visible: boolean): void {
    this.grid.visible = visible;
  }

  setChunkBordersVisible(visible: boolean): void {
    this.boundsVisible = visible;
    this.borderGroup.visible = visible;
  }

  isChunkBordersVisible(): boolean {
    return this.boundsVisible;
  }

  resize(): void {
    if (this.disposed) return;
    const width = Math.max(1, this.container.clientWidth);
    const height = Math.max(1, this.container.clientHeight);
    this.camera.aspect = width / height;
    this.camera.updateProjectionMatrix();
    this.renderer.setPixelRatio(Math.min(window.devicePixelRatio || 1, 2));
    this.renderer.setSize(width, height, false);
  }

  capture(): CaptureResult {
    // A screenshot is an explicit synchronous request: finish pending meshes first so
    // completeness metadata and pixels describe the same revision.
    this.flushMeshQueue();
    this.renderer.render(this.scene, this.camera);
    const canvas = this.renderer.domElement;
    const dataUrl = canvas.toDataURL("image/png");
    const metadata = buildCaptureMetadata(
      this.world,
      this.cameraPose(),
      [canvas.width, canvas.height],
      this.requestedChunks,
    );
    return { dataUrl, metadata };
  }

  cameraPose(): CameraPose {
    const forward = new Vector3(0, 0, -1).applyQuaternion(this.camera.quaternion);
    const target = this.camera.position.clone().addScaledVector(forward, 32);
    this.camera.rotation.reorder("YXZ");
    return {
      position: [this.camera.position.x, this.camera.position.y, this.camera.position.z],
      target: [target.x, target.y, target.z],
      yaw: this.camera.rotation.y,
      pitch: this.camera.rotation.x,
      fov: this.camera.fov,
    };
  }

  dispose(): void {
    if (this.disposed) return;
    this.disposed = true;
    cancelAnimationFrame(this.animationId);
    this.meshQueue.clear();
    this.renderer.domElement.removeEventListener("webglcontextlost", this.onContextLost);
    this.fly.dispose();
    this.orbit.dispose();
    this.clearChunkMeshes();
    this.clearBorders();
    this.borderMaterial.dispose();
    this.grid.geometry.dispose();
    (this.grid.material as LineBasicMaterial).dispose();
    this.selection.geometry.dispose();
    (this.selection.material as LineBasicMaterial).dispose();
    this.selectionBox.geometry.dispose();
    (this.selectionBox.material as LineBasicMaterial).dispose();
    this.ghostFill.geometry.dispose();
    this.ghostMaterial.dispose();
    this.ghostEdges.geometry.dispose();
    (this.ghostEdges.material as LineBasicMaterial).dispose();
    for (const material of Object.values(this.materials)) material.dispose();
    this.atlas.dispose();
    this.assetModels = null;
    this.renderer.dispose();
    this.renderer.forceContextLoss();
    this.renderer.domElement.remove();
  }

  private removeChunkMesh(key: string): void {
    const existing = this.chunkMeshes.get(key);
    if (!existing) return;
    for (const mesh of existing) {
      this.chunkGroup.remove(mesh);
      mesh.geometry.dispose();
    }
    this.chunkMeshes.delete(key);
  }

  private rebuildChunk(pos: ChunkPos): void {
    const key = chunkKey(pos);
    this.removeChunkMesh(key);

    const data = buildChunkMesh(this.world, pos, this.assetModels);
    const meshes: Mesh[] = [];
    for (const category of ["opaque", "cutout", "translucent"] as const) {
      const buffers = data[category];
      if (buffers.positions.length === 0) continue;
      const geometry = new BufferGeometry();
      geometry.setAttribute("position", new Float32BufferAttribute(buffers.positions, 3));
      geometry.setAttribute("normal", new Float32BufferAttribute(buffers.normals, 3));
      geometry.setAttribute("uv", new Float32BufferAttribute(buffers.uvs, 2));
      geometry.setAttribute("color", new Float32BufferAttribute(buffers.colors, 3));
      geometry.setIndex(new Uint32BufferAttribute(buffers.indices, 1));
      geometry.computeBoundingSphere();
      const mesh = new Mesh(geometry, this.materials[category]);
      mesh.name = `chunk:${key}:${category}`;
      mesh.matrixAutoUpdate = false;
      mesh.updateMatrix();
      this.chunkGroup.add(mesh);
      meshes.push(mesh);
    }
    if (meshes.length > 0) this.chunkMeshes.set(key, meshes);
  }

  private clearChunkMeshes(): void {
    for (const meshes of this.chunkMeshes.values()) {
      for (const mesh of meshes) {
        this.chunkGroup.remove(mesh);
        mesh.geometry.dispose();
      }
    }
    this.chunkMeshes.clear();
  }

  private rebuildBorders(): void {
    this.clearBorders();
    const points: number[] = [];
    const yBottom = -64;
    const yTop = 72;
    for (const chunk of this.world.loadedChunks()) {
      const x = chunk.x * 16;
      const z = chunk.z * 16;
      const corners = [
        [x, z],
        [x + 16, z],
        [x + 16, z + 16],
        [x, z + 16],
      ] as const;
      for (const [cx, cz] of corners) {
        points.push(cx, yBottom, cz, cx, yTop, cz);
      }
      points.push(
        x,
        yTop,
        z,
        x + 16,
        yTop,
        z,
        x + 16,
        yTop,
        z,
        x + 16,
        yTop,
        z + 16,
        x + 16,
        yTop,
        z + 16,
        x,
        yTop,
        z + 16,
        x,
        yTop,
        z + 16,
        x,
        yTop,
        z,
      );
    }
    if (points.length === 0) {
      this.borderGroup.visible = false;
      return;
    }
    const geometry = new BufferGeometry();
    geometry.setAttribute("position", new Float32BufferAttribute(points, 3));
    this.borderGroup.add(new LineSegments(geometry, this.borderMaterial));
    this.borderGroup.visible = this.boundsVisible;
  }

  private clearBorders(): void {
    for (const child of [...this.borderGroup.children]) {
      this.borderGroup.remove(child);
      if (child instanceof LineSegments) {
        child.geometry.dispose();
      }
    }
  }

  private animate = (now: number): void => {
    if (this.disposed) return;
    this.processMeshQueue();
    const delta = Math.max(0, Math.min((now - this.clock.last) / 1000, 0.25));
    this.clock.last = now;
    if (this.cameraMode === "orbit") {
      this.orbit.update();
    } else {
      this.fly.update(delta);
    }
    this.renderer.render(this.scene, this.camera);

    this.frameCount += 1;
    this.metricElapsed += delta;
    this.frameMs = this.frameMs === 0 ? delta * 1000 : this.frameMs * 0.9 + delta * 100;
    if (this.metricElapsed >= 0.4) {
      this.fps = this.frameCount / this.metricElapsed;
      const info = this.renderer.info;
      this.metrics = {
        fps: Math.round(this.fps),
        frameMs: Math.round(this.frameMs * 10) / 10,
        drawCalls: info.render.calls,
        triangles: info.render.triangles,
        geometries: info.memory.geometries,
        textures: info.memory.textures,
        loadedChunks: this.world.loadedChunkCount(),
        renderedSections: this.world.sectionCount(),
        meshCount: this.chunkGroup.children.length,
        pendingMeshJobs: this.meshQueue.size,
        evictedChunks: this.evictedChunks,
      };
      this.callbacks.onMetrics?.(this.metrics);
      this.frameCount = 0;
      this.metricElapsed = 0;
    }
    if (now - this.lastPoseAt > 220) {
      this.lastPoseAt = now;
      this.callbacks.onCameraPose?.(this.cameraPose());
    }
    this.animationId = requestAnimationFrame(this.animate);
  };

  private onContextLost = (event: Event): void => {
    event.preventDefault();
    cancelAnimationFrame(this.animationId);
    this.callbacks.onContextLost?.();
  };
}
