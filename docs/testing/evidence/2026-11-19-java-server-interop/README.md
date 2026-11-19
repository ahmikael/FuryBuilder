# M11.5 Java server import/export evidence

One recorded release-level round trip against the only supported world format:
**Minecraft Java 1.21.4**, classic Anvil layout, `DataVersion` 4189.

- `evidence.json` — machine-readable result of the run.
- Harness: `scripts/java-server-interop-webview2.mjs`.
- Explanation and reproduction: `docs/testing/java-server-interop.md`.

Evidence SHA-256: `144837f6a454328cfc2b5da63d316ff476b507dd2ccacbe3914341863ee95544`.

The run used a user-provided official 1.21.4 server JAR
(SHA-1 `4707d00eb834b446575d89a61a11b5d548d8c001`) and Java 21. It started the
official dedicated server twice, imported and edited the world through the real Tauri
application, exported a new complete world folder through the backend `world_export`
path, verified the edited and untouched blocks with `/execute if block`, then
re-imported the server-saved export into the application.

No server JAR, Java runtime, Minecraft asset, generated world or server log is committed
to this repository. The temporary harness output only contained those files.

Command used:

```powershell
node scripts/java-server-interop-webview2.mjs <new-output-directory> `
  <java.exe> <server.jar> 4707d00eb834b446575d89a61a11b5d548d8c001 `
  --accept-minecraft-eula
```

`--accept-minecraft-eula` is always explicit; the harness never accepts the Minecraft
EULA on the user's behalf.
