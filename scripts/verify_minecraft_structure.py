#!/usr/bin/env python3
"""Load a Java structure in an isolated official Minecraft server and check blocks.

This is an opt-in interoperability check, not part of normal editing. It:
  * requires explicit paths to a server JAR, a Java executable and a user's accepted EULA;
  * copies the JAR and EULA into a new temporary directory (the originals are untouched);
  * binds the disposable server to loopback only and uses a fresh flat world;
  * places the structure with `/place template` and verifies exact block states.

The script never accepts the EULA, downloads anything, or opens an external port. The
server JAR is only executed if its SHA-1 matches `--expected-server-sha1` when supplied.
It uses Python's standard library only.

Example:
  python scripts/verify_minecraft_structure.py \
    --server-jar D:/path/server.jar \
    --java D:/path/java.exe \
    --eula D:/path/eula.txt \
    --structure target/furybuilder-demo.nbt \
    --origin 0 64 0 \
    --check "0,64,0=minecraft:stone_bricks" \
    --check "2,65,0=minecraft:oak_stairs[facing=south,half=top,shape=straight,waterlogged=false]"
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import queue
import re
import shutil
import socket
import subprocess
import sys
import tempfile
import threading
import time
from pathlib import Path

TAG = "furybuilder"
TEMPLATE = "probe"


def check_spec(value: str) -> tuple[int, int, int, str]:
    if "=" not in value:
        raise argparse.ArgumentTypeError("check must be x,y,z=block-state")
    coordinates, state = value.split("=", 1)
    parts = coordinates.split(",")
    if len(parts) != 3:
        raise argparse.ArgumentTypeError("check coordinates must be x,y,z")
    try:
        x, y, z = (int(part) for part in parts)
    except ValueError as error:
        raise argparse.ArgumentTypeError("check coordinates must be integers") from error
    if not state or not re.fullmatch(r"[A-Za-z0-9_:.,=\[\]-]+", state):
        raise argparse.ArgumentTypeError("block state contains unsupported characters")
    return x, y, z, state


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--server-jar", required=True, type=Path)
    parser.add_argument("--java", required=True, type=Path)
    parser.add_argument("--eula", required=True, type=Path)
    parser.add_argument("--structure", required=True, type=Path)
    parser.add_argument("--origin", nargs=3, required=True, type=int, metavar=("X", "Y", "Z"))
    parser.add_argument("--check", action="append", default=[], type=check_spec)
    parser.add_argument("--expected-server-sha1", default=None)
    parser.add_argument("--timeout", type=float, default=240.0)
    return parser.parse_args()


def wait_for(events: queue.Queue[str], pattern: re.Pattern[str], timeout: float) -> str:
    until = time.monotonic() + timeout
    while time.monotonic() < until:
        try:
            line = events.get(timeout=max(0.01, until - time.monotonic()))
        except queue.Empty:
            break
        if pattern.search(line):
            print(f"SERVER: {line}", flush=True)
            return line
    raise TimeoutError(f"did not observe {pattern.pattern}")


def main() -> int:
    args = parse_args()
    server_jar = args.server_jar.resolve()
    java = args.java.resolve()
    eula = args.eula.resolve()
    structure = args.structure.resolve()
    if not server_jar.is_file() or not java.is_file() or not structure.is_file():
        print("server JAR, Java and structure must be existing files", file=sys.stderr)
        return 2
    eula_bytes = eula.read_bytes()
    if not re.search(rb"(?m)^\s*eula\s*=\s*true\s*$", eula_bytes):
        print("the supplied EULA file does not record acceptance", file=sys.stderr)
        return 2
    server_sha1 = hashlib.sha1(server_jar.read_bytes()).hexdigest()
    if args.expected_server_sha1 and server_sha1.lower() != args.expected_server_sha1.lower():
        print(
            f"server JAR SHA-1 {server_sha1} does not match the expected checksum",
            file=sys.stderr,
        )
        return 2

    temp_root = Path(
        tempfile.mkdtemp(prefix="fury-structure-check-", dir=tempfile.gettempdir())
    )
    shutil.copyfile(server_jar, temp_root / "server.jar")
    (temp_root / "eula.txt").write_bytes(eula_bytes)
    target = temp_root / "world" / "generated" / TAG / "structures"
    target.mkdir(parents=True)
    shutil.copyfile(structure, target / f"{TEMPLATE}.nbt")

    with socket.socket() as sock:
        sock.bind(("127.0.0.1", 0))
        port = sock.getsockname()[1]
    properties = [
        "server-ip=127.0.0.1",
        f"server-port={port}",
        "online-mode=false",
        "enable-rcon=false",
        "enable-query=false",
        "enable-command-block=false",
        "max-players=1",
        "spawn-protection=0",
        "view-distance=2",
        "simulation-distance=2",
        "allow-flight=true",
        "allow-nether=false",
        "generate-structures=false",
        "level-seed=47042024",
        "level-type=minecraft:flat",
        'generator-settings={"biome":"minecraft:plains","features":false,"lakes":false,'
        '"layers":[{"block":"minecraft:bedrock","height":1},'
        '{"block":"minecraft:dirt","height":2},{"block":"minecraft:grass_block","height":1}],'
        '"structure_overrides":[]}',
        "difficulty=peaceful",
        "gamemode=creative",
        "motd=FuryBuilder structure verification",
    ]
    (temp_root / "server.properties").write_text("\n".join(properties) + "\n", encoding="utf-8")

    events: queue.Queue[str] = queue.Queue()
    log: list[str] = []

    def collect(stream) -> None:
        for raw in stream:
            line = raw.rstrip()
            log.append(line)
            events.put(line)

    process = subprocess.Popen(
        [
            str(java),
            "-XX:ActiveProcessorCount=2",
            "-Xms256M",
            "-Xmx1024M",
            "-jar",
            str(temp_root / "server.jar"),
            "nogui",
        ],
        cwd=temp_root,
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        text=True,
        encoding="utf-8",
        errors="replace",
        bufsize=1,
    )
    threading.Thread(target=collect, args=(process.stdout,), daemon=True).start()
    try:
        wait_for(events, re.compile(r"Done \([0-9.]+s\)!"), args.timeout)
        x, y, z = args.origin
        process.stdin.write(f"forceload add {x - 16} {z - 16} {x + 32} {z + 32}\n")
        process.stdin.flush()
        wait_for(events, re.compile(r"Marked .*chunks?"), 60)
        process.stdin.write(f"place template {TAG}:{TEMPLATE} {x} {y} {z}\n")
        process.stdin.flush()
        wait_for(events, re.compile(r"Loaded template|Successfully placed"), 60)
        for index, (cx, cy, cz, state) in enumerate(args.check):
            marker = f"FURY_CHECK_OK_{index}"
            process.stdin.write(
                f"execute if block {cx} {cy} {cz} {state} run say {marker}\n"
            )
            process.stdin.flush()
            wait_for(events, re.compile(rf"\b{marker}\b"), 30)
        print(
            f"SUCCESS: official server loaded {structure.name} "
            f"({len(args.check)} exact block check(s))",
            flush=True,
        )
    except TimeoutError as error:
        print(f"FAILED: {error}", file=sys.stderr)
        print("\n".join(log[-40:]), file=sys.stderr)
        return 1
    finally:
        if process.poll() is None:
            try:
                process.stdin.write("save-all flush\nstop\n")
                process.stdin.flush()
                process.wait(timeout=60)
            except Exception:
                process.kill()
                process.wait(timeout=10)
        (temp_root / "probe.log").write_text("\n".join(log) + "\n", encoding="utf-8")
        print(
            json.dumps(
                {
                    "workspace": str(temp_root),
                    "server_sha1": server_sha1,
                    "structure_sha256": hashlib.sha256(structure.read_bytes()).hexdigest(),
                    "checks": len(args.check),
                },
                indent=2,
            ),
            flush=True,
        )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
