# Camera controls (M3)

## Fly mode (default)

| Input | Action |
|---|---|
| `W` / `A` / `S` / `D` | Move forward / left / back / right |
| `Space` / `Ctrl` (or `C`) | Move up / down |
| `Shift` | 2.6× speed boost |
| Right-drag | Look around (yaw/pitch) |
| Mouse wheel | Adjust base speed (2…180) |
| Left click | Pick the block under the cursor |
| Double-click | Pick and fly to the hit block |

Keyboard input is ignored while focus is in an input, textarea, select or contenteditable
element, so typing coordinates never moves the camera. Losing window focus clears held
keys and drag state.

## Orbit mode

Enabled from the viewport toolbar. Drag rotates around the target, wheel zooms, and the
polar angle is clamped just above the horizon. Switching back to fly keeps the current
camera position and derives yaw/pitch from the orbit pose, so the view does not jump.

## Go to coordinates

The toolbar dialog accepts `x`, `y`, `z` block coordinates. The fly camera moves to an
offset above/next to the target and both camera modes are aimed at the block center.
Coordinates are signed integers; there is no clamp to the build height because the camera
is free to inspect the dimension range.

## Camera pose

`cameraPose()` returns position, target (32 blocks forward), yaw, pitch and FOV. Captures
record the same pose in Observation metadata so an image can be reproduced by calling:

```ts
viewport.setCameraPose(metadata.camera.position, metadata.camera.target);
```

`setCameraPose` exists for scripts and reproducibility checks; normal users use fly/orbit.
