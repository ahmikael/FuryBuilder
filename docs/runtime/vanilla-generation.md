# Isolated Vanilla World generation (M9.5–M9.8)

Scope: explicitly accept the Minecraft EULA and run the **official** Minecraft Java 1.21.4
server process in an isolated temporary directory. M9.5 adds the safe process boundary,
EULA flow, progress and termination. M9.6 adds the Vanilla World wizard: Java/runtime setup
check, seed, preset, supported worldgen settings, validated local data packs and a separate
explicit consent for the local run. M9.7 imports a completed run into the Rust World Core as
a Vanilla World project with a byte-preserving immutable source copy and full generator
provenance. M9.8 compares the saved imported base with the original generator's reference
chunks and refuses mismatched settings or unsupported references.

Implementation: `crates/runtime-manager/src/server/generation.rs`,
`process_tree.rs` and `datapack.rs`, `crates/world-io/src/parity.rs`, the Tauri
`server_*generation*`, `vanilla_setup_check`, `project_import_generated_world` and
`project_verify_vanilla_parity` commands, `Settings → Minecraft server JAR → Isolated
vanilla generation`, and `Project → New project → Vanilla World` plus the project's
`Generated base verification` panel.

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
- Starting a run requires a **second, separate confirmation** that the selected official
  server JAR may run locally with the selected Java 21 runtime. The backend rejects
  `consent=false` with `consent_required`; it never infers consent from the EULA record or
  from an earlier run. Each report records `consentAt` and `eulaAcceptedAt`.

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
   chunks), time, memory, view distance, CPU threads, structures flag, consent and the
   selected data packs.
2. Create the run directory, write `.furybuilder-run.json`, `server.properties` and the
   run-local `eula.txt`. Validated `.zip` data packs are copied into
   `world/datapacks/` before the server starts and their names, sizes, formats and SHA-256
   are recorded.
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

Presets exposed by M9.5/M9.6 are `normal`, `flat`, `large_biomes` and `amplified` via
`level-type`. The supported worldgen setting exposed by the wizard is
`generate-structures` (`generate-structures=true|false`). Other `server.properties` values
remain fixed by the isolation policy.

## Vanilla World wizard (M9.6)

`Project → New project → Vanilla World` performs a single setup check before exposing the
generation action:

| Check      | What is verified                                                                                                                    |
| ---------- | ----------------------------------------------------------------------------------------------------------------------------------- |
| Target     | Minecraft Java 1.21.4, `DataVersion` 4189, Java major 21, data pack format 61                                                       |
| Java       | A saved runtime that was actually probed as compatible; the wizard can select a discovered Java 21 candidate or a manual `java.exe` |
| Loader JAR | A ready, verified 1.21.4 `server.jar` from the managed cache or a manual file                                                       |
| EULA       | Acceptance is tied to the selected JAR's SHA-1; the EULA is never accepted implicitly                                               |

The action stays disabled while a blocker remains. It also stays disabled until the user
checks the separate run-consent box. The backend re-checks the JAR, Java, EULA record,
consent, seed, limits and every data pack; the UI is not the security boundary.

### Data pack validation

Only regular local `.zip` files are accepted. The backend checks:

- a bounded number of packs (16), per-file size (64 MiB), total size (256 MiB), archive
  entry count (20 000) and declared uncompressed size (512 MiB);
- valid ZIP structure and safe entry paths (no absolute paths, drive prefixes, `..`, `.`
  components or backslashes; no symlink entries);
- exactly a root `pack.mcmeta` with a numeric `pack_format` and valid JSON;
- format 61 is the target; older formats are accepted with a warning; newer formats are
  refused unless `supported_formats` explicitly includes 61;
- duplicate file names are refused; the file is re-inspected and hash-checked immediately
  before copying, so a change between selection and start fails closed.

The copied pack is recorded in the run manifest and report. This validates archive safety
and declared compatibility, **not** the semantic behavior of the pack. Third-party data
packs are still untrusted game content; selected packs are never downloaded automatically.

## Importing a generated world (M9.7)

`Open as project` in the wizard calls `project_import_generated_world`. The run id alone is
never trusted as a path: `ServerManager` reopens the run, rejects symlinks and traversal,
requires the run manifest and terminal report to belong to that exact id and to agree on
seed, preset, center, radius, structures and data packs, and requires
`stop_reason = completed`, `worldComplete = true` and enough generated chunks. The world
directory must still be `<run>/world` inside the runs root.

The import then reuses the M8 pipeline:

1. `world_io::analyze_java_world` and `load_java_world` decode the verified 1.21.4 world into
   the Rust World Core; unsupported versions or layouts fail closed.
2. `passthrough_java_world` copies every generated file byte-for-byte into the project's
   `source/world/` (the only skipped file is `session.lock`). Every copied file's SHA-256 and
   size is recorded in `source/source.json`; the run directory itself is never modified.
3. The project is created with `creation_mode = vanilla` and `source.kind = generated`,
   `immutable = true`, `reference = "world"`.
4. The exact generator identity is stored in `project.json` under `vanilla`: seed, preset,
   center/radius, structures flag, each data pack's name/SHA-256/size/format, Minecraft
   1.21.4 + `DataVersion` 4189, Java version, official server JAR SHA-1, run id, generation
   time, requested chunk count, region count and generated world bytes.

Edits are stored in the project overlay/snapshot. The generated base stays read-only, so a
later export can use passthrough for untouched files. Clearing the generation runs after a
successful import does not delete the project's own source copy.

## Reference-chunk parity (M9.8)

`project_verify_vanilla_parity` compares the saved editable snapshot with the reference
chunks that the original generator wrote. It is not a second world generator and it never
rewrites either world.

Preconditions, all enforced in Rust:

- the project is a Vanilla World project with an immutable `source.kind = generated` tree;
- the saved snapshot revision is exactly 1; an edited base is refused, because comparing
  later revisions would report expected edits as generator differences;
- when the original run is still available, seed, preset, center/radius, structures flag,
  ordered data-pack name/SHA-256/size/format, Java version, official server SHA-1, generated
  chunk/region counts and world bytes must match the project provenance exactly;
- when the run was cleared, the project's immutable byte-for-byte source copy is used and the
  report records that fallback;
- `analyze_java_world` must accept the reference (1.21.4 classic layout, complete save);
  missing requested chunks or chunks without `minecraft:full` status refuse the check;
- the sample is capped at 64 requested chunks (all chunks when the area fits), and
  `world_io::parity` compares every block position in sections that are non-empty on either
  side, with a 50,000,000-position budget and up to 20 detailed differences.

The comparison is **block-state only**. Biomes, heightmaps, lighting, block entities,
entities and POI remain in the immutable source copy but are not modelled by the editable
world, so the report never claims parity for them. A missing project chunk is a real
difference; a missing reference chunk is an incomplete generator result and is refused.

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

| Command                          | Purpose                                                   |
| -------------------------------- | --------------------------------------------------------- |
| `vanilla_setup_check`            | Read-only Java/JAR/EULA readiness and blockers            |
| `server_inspect_datapacks`       | Validate selected local `.zip` data packs per path        |
| `server_accept_eula`             | Record an explicit acceptance for the selected JAR        |
| `server_clear_eula`              | Withdraw the acceptance                                   |
| `server_start_generation`        | Validate (including consent/data packs) and start one run |
| `server_generation_status`       | Active snapshot and last terminal report                  |
| `server_cancel_generation`       | Request cooperative stop, then forced tree termination    |
| `server_generation_runs`         | Inventory of FuryBuilder-owned run directories            |
| `server_clear_generation_runs`   | Delete only manifest-owned run directories                |
| `project_import_generated_world` | Verify a completed run and import it as a Vanilla project |
| `project_verify_vanilla_parity`  | Compare a saved Vanilla base with reference chunks        |

The UI polls `server_generation_status` once per second while a run is active; no unbounded
event stream is required.

## Evidence

### M9.5 process boundary

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

### M9.6 Vanilla World wizard

`docs/testing/evidence/2026-11-19-vanilla-wizard-ui.json` and `.png` record a real WebView2
wizard run with fresh isolated app data:

- the wizard started **not ready**: only the EULA blocker remained before acceptance, and
  the Accept button was disabled until its checkbox was checked;
- after EULA acceptance for `4707d00e…8c001`, `vanilla_setup_check` reported Java 21,
  verified JAR and EULA ready;
- the Start button was disabled before the separate run consent was checked;
- the wizard generated 9/9 chunks in 8 regions for seed `FuryBuilderM96UI`, bind
  `127.0.0.1:60645`, Job Object active, `generateStructures=true`, consent and EULA times
  recorded;
- a second real run used the validated local `furybuilder-m96-pack.zip`
  (format 61, SHA-256 `08de01a4…6623`), `generateStructures=false`; the pack was copied into
  `world/datapacks/` with the same SHA-256, the report recorded it, and the server properties
  contained `generate-structures=false`;
- both runs stayed loopback-only, exited 0 with the requested chunks, and no console errors
  were recorded.

### M9.7 World Core import

`docs/testing/evidence/2026-11-19-vanilla-world-import.json` and `.png` record the same real
WebView2 flow with the import step:

- after a completed `9/9`-chunk run, **Open as project** created
  `Vanilla FuryBuilderM96UI.furybuilder` with `creationMode = vanilla` and
  `source.kind = generated`, `immutable = true`, `reference = "world"`;
- `project.json` stored the run id, seed `FuryBuilderM96UI`, `normal` preset, center
  `(4500, 4500)`, radius 1, structures enabled, zero data packs, Java 21.0.12.1, official
  server SHA-1 `4707d00e…8c001`, `DataVersion` 4189, 9 requested chunks and 8 regions;
- the project's immutable `source/world` had 29 files; a SHA-256 tree digest
  (`1dfc64f9…076f`) matched the generated run world exactly, and re-hashing the run world
  after import produced the same digest (the generated source was untouched);
- the imported project opened non-empty and editable: 242 overworld chunks and 7,627,482
  blocks in the Rust World Core, revision 1, not dirty;
- a second run with the validated data pack and `generateStructures=false` still completed
  after the import, and all prior loopback/process-tree guarantees held.

### M9.8 reference-chunk parity

`docs/testing/evidence/2026-11-19-vanilla-parity-ui.json`, `.png` and `-parity.png` record
the same real flow with the parity step:

- the saved revision-1 base matched the original run exactly: 9/9 sampled chunks, 77
  non-empty sections, 315,392 block positions, 0 differences; the UI panel showed
  `Block parity matched` and the same counts;
- the comparison reported the explicit limitations (block states only, requested overworld
  area only, not a general vanilla parity proof);
- pointing the check at the second completed run was refused with concrete reasons: seed,
  center X/Z, structures flag, data-pack count and generated bytes differed, and no
  comparison was attempted;
- native tests cover semantic palette equality, absolute mismatch coordinates,
  missing/extra sections, bounded detail output and deterministic sampling.

Reproduce:

```powershell
cargo test -p furybuilder-runtime-manager
cargo run -p furybuilder-runtime-manager --example generation_probe -- `
  --root <dir> --java <java.exe> --jar <server.jar> --accept-eula --consent `
  [--no-structures] [--datapack <pack.zip>]

# start the Vite dev server first
node scripts/generation-webview2-smoke.mjs <java.exe> <server.jar> <out.json> <out.png>
node scripts/vanilla-wizard-webview2-smoke.mjs <java.exe> <server.jar> <pack.zip> <out.json> <out.png>
```

The CLI example prints progress to stderr and the final report to stdout. `--accept-eula`
and `--consent` are that CLI's two explicit confirmations; the desktop UI uses the wizard
checkboxes and buttons above.

## Limits

- Only a `completed` run can be opened as a project. `incomplete`, `time_limit`, `cancelled`
  and failed runs remain visible in the run inventory but are not importable.
- The requested area is bounded (radius 1–16 chunks) and generated in forceload tiles. This
  is limited/batched generation, not an infinite lazy world stream.
- Spawn-area chunks are generated and imported in addition to the requested area; only the
  in-area chunks are counted for completion. The M9.7 smoke imported 242 overworld chunks for
  a 9-chunk requested area.
- `incomplete` and `time limit reached` are valid terminal states; they are not silently
  retried and the user sees the shortfall.
- No resume of an interrupted generation; start a new run.
- Reference-chunk parity is only defined for the untouched imported base (revision 1) and
  compares block states in a bounded sample. It is not a general vanilla parity claim and
  does not verify biomes or derived data in the editable model.
- An independent second generation with identical settings is not part of the parity check;
  the check compares the imported project representation with the recorded generator
  output.
- Data packs are validated as archives and for declared format compatibility, not for their
  game behavior, dependencies or worldgen changes. Only `.zip` files are accepted; a folder
  pack can be zipped by the user.
- Backslash ZIP entry paths and symlink entries are refused; such archives are not portable
  to the official Java server either.
- The wizard exposes only `generate-structures` as an extra worldgen setting. Other
  `server.properties` values are fixed by the isolation policy.
- The server can still write its normal logs and `world/` inside the isolated run directory.
  It is not given access to the user's original world.
- Minecraft's EULA text is not reproduced or accepted by FuryBuilder; only the official link
  is shown.
