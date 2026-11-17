import { Euler, Vector3, type PerspectiveCamera } from "three";

function isTypingTarget(target: EventTarget | null): boolean {
  if (!(target instanceof HTMLElement)) return false;
  const tag = target.tagName;
  return (
    tag === "INPUT" ||
    tag === "TEXTAREA" ||
    tag === "SELECT" ||
    target.isContentEditable ||
    target.getAttribute("contenteditable") === "true" ||
    target.getAttribute("contenteditable") === ""
  );
}

/** Free-fly camera: WASD + Space/Ctrl, right mouse to look, wheel to change speed. */
export class FlyCameraController {
  readonly position = new Vector3(26, 84, 34);
  yaw = -2.45;
  pitch = -0.42;
  speed = 18;
  enabled = true;

  private keys = new Set<string>();
  private dragging = false;
  private lastX = 0;
  private lastY = 0;
  private readonly forward = new Vector3();
  private readonly right = new Vector3();
  private readonly move = new Vector3();
  private readonly up = new Vector3(0, 1, 0);

  constructor(
    private readonly camera: PerspectiveCamera,
    private readonly canvas: HTMLCanvasElement,
  ) {
    this.canvas.addEventListener("pointerdown", this.onPointerDown);
    this.canvas.addEventListener("pointermove", this.onPointerMove);
    this.canvas.addEventListener("pointerup", this.onPointerUp);
    this.canvas.addEventListener("pointercancel", this.onPointerUp);
    this.canvas.addEventListener("wheel", this.onWheel, { passive: false });
    this.canvas.addEventListener("contextmenu", this.onContextMenu);
    window.addEventListener("keydown", this.onKeyDown);
    window.addEventListener("keyup", this.onKeyUp);
    window.addEventListener("blur", this.onBlur);
    this.applyTransform();
  }

  update(deltaSeconds: number): void {
    if (!this.enabled) {
      this.applyTransform();
      return;
    }
    const dt = Math.min(deltaSeconds, 0.1);
    this.forward.set(0, 0, -1).applyEuler(new Euler(this.pitch, this.yaw, 0, "YXZ"));
    this.right.crossVectors(this.forward, this.up).normalize();
    this.move.set(0, 0, 0);

    if (this.keys.has("KeyW")) this.move.add(this.forward);
    if (this.keys.has("KeyS")) this.move.sub(this.forward);
    if (this.keys.has("KeyD")) this.move.add(this.right);
    if (this.keys.has("KeyA")) this.move.sub(this.right);
    if (this.keys.has("Space")) this.move.add(this.up);
    if (this.keys.has("ControlLeft") || this.keys.has("KeyC")) this.move.sub(this.up);

    if (this.move.lengthSq() > 0) {
      const boost = this.keys.has("ShiftLeft") || this.keys.has("ShiftRight") ? 2.6 : 1;
      this.move.normalize().multiplyScalar(this.speed * boost * dt);
      this.position.add(this.move);
    }
    this.applyTransform();
  }

  applyTransform(): void {
    this.camera.position.copy(this.position);
    this.camera.rotation.set(this.pitch, this.yaw, 0, "YXZ");
  }

  setPosition(x: number, y: number, z: number): void {
    this.position.set(x, y, z);
    this.applyTransform();
  }

  lookAt(target: Vector3): void {
    this.camera.position.copy(this.position);
    this.camera.lookAt(target);
    this.camera.rotation.reorder("YXZ");
    this.yaw = this.camera.rotation.y;
    this.pitch = this.camera.rotation.x;
    this.applyTransform();
  }

  syncFromCamera(): void {
    this.position.copy(this.camera.position);
    this.camera.rotation.reorder("YXZ");
    this.yaw = this.camera.rotation.y;
    this.pitch = this.camera.rotation.x;
    this.applyTransform();
  }

  forwardVector(target = new Vector3()): Vector3 {
    return target
      .set(0, 0, -1)
      .applyEuler(new Euler(this.pitch, this.yaw, 0, "YXZ"))
      .normalize();
  }

  dispose(): void {
    this.canvas.removeEventListener("pointerdown", this.onPointerDown);
    this.canvas.removeEventListener("pointermove", this.onPointerMove);
    this.canvas.removeEventListener("pointerup", this.onPointerUp);
    this.canvas.removeEventListener("pointercancel", this.onPointerUp);
    this.canvas.removeEventListener("wheel", this.onWheel);
    this.canvas.removeEventListener("contextmenu", this.onContextMenu);
    window.removeEventListener("keydown", this.onKeyDown);
    window.removeEventListener("keyup", this.onKeyUp);
    window.removeEventListener("blur", this.onBlur);
  }

  private onPointerDown = (event: PointerEvent): void => {
    if (!this.enabled || event.button !== 2) return;
    this.dragging = true;
    this.lastX = event.clientX;
    this.lastY = event.clientY;
    this.canvas.setPointerCapture(event.pointerId);
  };

  private onPointerMove = (event: PointerEvent): void => {
    if (!this.enabled || !this.dragging) return;
    const dx = event.clientX - this.lastX;
    const dy = event.clientY - this.lastY;
    this.lastX = event.clientX;
    this.lastY = event.clientY;
    this.yaw -= dx * 0.0032;
    this.pitch = Math.max(-1.54, Math.min(1.54, this.pitch - dy * 0.0032));
  };

  private onPointerUp = (event: PointerEvent): void => {
    if (!this.dragging) return;
    this.dragging = false;
    if (this.canvas.hasPointerCapture(event.pointerId)) {
      this.canvas.releasePointerCapture(event.pointerId);
    }
  };

  private onWheel = (event: WheelEvent): void => {
    if (!this.enabled) return;
    event.preventDefault();
    const factor = Math.exp(-event.deltaY * 0.0015);
    this.speed = Math.max(2, Math.min(180, this.speed * factor));
  };

  private onContextMenu = (event: MouseEvent): void => {
    event.preventDefault();
  };

  private onKeyDown = (event: KeyboardEvent): void => {
    if (isTypingTarget(event.target)) return;
    this.keys.add(event.code);
    if (
      ["KeyW", "KeyA", "KeyS", "KeyD", "Space", "ShiftLeft", "ControlLeft"].includes(event.code)
    ) {
      event.preventDefault();
    }
  };

  private onKeyUp = (event: KeyboardEvent): void => {
    this.keys.delete(event.code);
  };

  private onBlur = (): void => {
    this.keys.clear();
    this.dragging = false;
  };
}
