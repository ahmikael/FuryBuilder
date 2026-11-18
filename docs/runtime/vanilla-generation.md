# Isolated Vanilla World generation (M9.5)

Scope: explicitly accept the Minecraft EULA and run the **official** Minecraft Java 1.21.4
server process in an isolated temporary directory. M9.5 proves the safe process boundary,
EULA flow, progress and termination. The Vanilla World wizard, datapacks, presets UI inside
project creation, and importing the generated world into the Rust World Core belong to
M9.6 and later tasks.

Implementation: `crates/runtime-manager/src/server/generation.rs` and `process_tree.rs`,
the Tauri `server_*generation*` commands and `Settings → Minecraft server JAR → Isolated
vanilla generation`.

## Explicit EULA

FuryBuilder never accepts the Minecraft EULA on the user's behalf.

- The panel shows a checkbox and a separate **Accept EULA** button only after a verified
  `server.jar` is selected.
- The backend command `server_accept_eula(accepted)` rejects `false`; the UI passes `true`
  only after the checkbox is checked. The official text is linked at
  <https://aka.ms/MinecraftEULA>.
- Acceptance is stored in `<app-data>/server.json` and tied to the JAR's SHA-1. Changing the
  selected JAR invalidates the match and requires confirmation again. Clearing the selection
  does **not** silently accept a new file.
- `eula.txt` with `eula=true` is written only inside a new run directory, and only after the
  acceptance record for that exact SHA-1 has been checked.
- The acceptance can be withdrawn with **Withdraw**; no server can start afterwards.

## Isolation and limits

The official server is started with `nogui` and:

| Control           | Value                                                                                                  |
| ----------------- | ------------------------------------------------------------------------------------------------------ |
| Listener          | `server-ip=127.0.0.1`, ephemeral loopback port                                                         |
| Remote access     | `online-mode=false`, `enable-rcon=false`, `enable-query=false`, `enable-status=false`, `max-players=1` |
| JVM memory        | bounded `-Xmx` (512–4096 MiB, default 2048) and a Windows Job Object process-memory cap                |
| CPU               | `-XX:ActiveProcessorCount` (1–8)                                                                       |
| Time              | explicit wall-clock limit (10–3600 s), checked every 200 ms                                            |
| Tick watchdog     | disabled (`max-tick-time=-1`) so the explicit limit is authoritative                                   |
| Process tree      | Windows Job Object with `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`; Unix process group                       |
| Working directory | a new `<app-data>/server-runs/<run-id>/`                                                               |
| Console           | bounded in-memory tail and `logs/furybuilder-console.log`                                              |

The Job Object also guarantees that an abnormal FuryBuilder exit closes the job handle and
Windows terminates the Java/bundler tree. On normal completion, cancel and crash the handle is
closed only after the tree has been stopped. `taskkill /T /F` is a fallback if the Job Object
cannot be created; the fallback is reported as `processTreeGuarded=false`.

Only loopback is reachable. The supervisor parses the server's actual
`Starting Minecraft server on …` line and refuses a non-loopback bind address. It never opens
a firewall port for external players.

## Generation flow

1. Validate the seed (`1–64` ASCII letters/digits/`-_.`), preset, center, radius (1–16
   chunks), time, memory, view distance and CPU threads.
2. Create the run directory, write `.furybuilder-run.json`, `server.properties` and the
   run-local `eula.txt`.
3. Start the official server with the selected, re-probed Java 21 executable.
4. Wait for the `Done (…)!` line, then send `/forceload add` commands covering the requested
   area. Areas larger than 256 chunks are split into bounded tiles.
5. Send `save-all flush` periodically. Progress is counted from the Anvil region headers of
   chunks inside the requested area, not from a filename or a log message. An area is complete
   when every requested chunk is present.
6. Save, send `forceload remove all`, flush, then `stop`. A grace period is followed by a
   process-tree kill if the server does not exit.
7. Write `report.json` and present the result. Cancelled/crashed/incomplete runs are reported
   honestly; they are never labelled completed.

Presets exposed by M9.5 are `normal`, `flat`, `large_biomes` and `amplified` via
`level-type`. Datapacks and the full project wizard are not part of M9.5.

## Run directory

```text
<app-data>/
  server.json                 # selected JAR + EULA acceptance record
  server-runs/
    run-<millis>-<pid>/
      .furybuilder-run.json   # ownership marker
      server.properties
      eula.txt                # only after explicit acceptance
      logs/furybuilder-console.log
      world/                  # official server output
      report.json             # terminal report
      libraries/ versions/    # extracted by the official bundler
```

`Clear generation runs` removes only directories that carry
`.furybuilder-run.json`; unrelated files and user-selected external JARs are untouched. An
active run must be cancelled first. The run directory, including the bundler's extracted
libraries, is temporary and can be large; this is deliberate so the source JAR is never
modified.

## Commands

| Command                        | Purpose                                                |
| ------------------------------ | ------------------------------------------------------ |
| `server_accept_eula`           | Record an explicit acceptance for the selected JAR     |
| `server_clear_eula`            | Withdraw the acceptance                                |
| `server_start_generation`      | Validate and start one isolated run                    |
| `server_generation_status`     | Active snapshot and last terminal report               |
| `server_cancel_generation`     | Request cooperative stop, then forced tree termination |
| `server_generation_runs`       | Inventory of FuryBuilder-owned run directories         |
| `server_clear_generation_runs` | Delete only manifest-owned run directories             |

The UI polls `server_generation_status` once per second while a run is active; no unbounded
event stream is required.

## Evidence

`docs/testing/evidence/2026-11-18-vanilla-generation-ui.json` and `.png` record a real
WebView2 run with isolated app data:

- the selected Java 21.0.12.1 runtime was probed;
- the manual official 1.21.4 JAR was verified (`4707d00e…8c001`, 56,880,250 bytes,
  `DataVersion` 4189);
- the **Accept EULA** button was disabled until the checkbox was checked, then acceptance was
  recorded for that SHA-1;
- the UI started a real generation run: 9/9 requested chunks, 8 region files, bind
  `127.0.0.1:51778`, Job Object active, exit code 0;
- a second larger run was started and cancelled from the UI; it reported `cancelled` and its
  reported PID no longer existed;
- **Clear generation runs** removed both run directories (2 → 0);
- the external `java.exe` SHA-256 was unchanged and no console errors were recorded.

Reproduce:

```powershell
cargo test -p furybuilder-runtime-manager
cargo run -p furybuilder-runtime-manager --example generation_probe -- `
  --root <dir> --java <java.exe> --jar <server.jar> --accept-eula

# start the Vite dev server first
node scripts/generation-webview2-smoke.mjs <java.exe> <server.jar> <out.json> <out.png>
```

The CLI example prints progress to stderr and the final report to stdout. The `--accept-eula`
flag is that CLI's explicit confirmation; the desktop UI uses the checkbox and button above.

## Limits

- No world is imported into the editor in M9.5; the generated `world/` is only evidence and
  temporary output until M9.6 adds the wizard/import.
- Spawn-area chunks are generated in addition to the requested area; only the in-area chunks
  are counted for completion.
- `incomplete` and `time limit reached` are valid terminal states; they are not silently
  retried and the user sees the shortfall.
- No resume of an interrupted generation; start a new run.
- The server can still write its normal logs and `world/` inside the isolated run directory.
  It is not given access to the user's original world.
- Minecraft's EULA text is not reproduced or accepted by FuryBuilder; only the official link
  is shown.
