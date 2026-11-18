#!/usr/bin/env python3
"""Read-only inventory of a Minecraft Java world folder.

The utility does not import, convert or modify a world. It identifies the
world storage layout, reads the two DataVersion values from ``level.dat`` and
enumerates dimension region folders so M8 work starts from observed facts.

Only the Python standard library is required. Symlinked dimension folders are
reported but never followed outside the selected world.

Usage:
    python scripts/inspect_world_layout.py <world-folder>
    python scripts/inspect_world_layout.py <world-folder> --require-target
"""

from __future__ import annotations

import argparse
import json
import os
import struct
import sys
import zlib
from datetime import datetime, timezone
from pathlib import Path

TOOL_NAME = "furybuilder-world-layout-inspector"
TOOL_VERSION = 1

# The only DataVersion this repository currently targets (ADR-001).
TARGET_DATA_VERSION = 4189
TARGET_VERSION_NAME = "1.21.4"

# 26.1 Snapshot 6 split dimensions and level.dat; the 26.1 release is 4786.
# Snapshot data versions between 4672 and 4773 are treated as transitional.
LAST_CLASSIC_DATA_VERSION = 4671  # 1.21.11
FIRST_MODERN_DATA_VERSION = 4774  # 26.1 Snapshot 6

MAX_LEVEL_DAT_BYTES = 8 * 1024 * 1024
MAX_LEVEL_NBT_BYTES = 32 * 1024 * 1024
MAX_DEPTH = 64
MAX_ELEMENTS = 1_000_000
MAX_UNRECOGNIZED_ENTRIES = 32

KNOWN_TOP_LEVEL = {
    "advancements",
    "data",
    "datapacks",
    "DIM-1",
    "DIM1",
    "dimensions",
    "entities",
    "generated",
    "icon.png",
    "level.dat",
    "level.dat_new",
    "level.dat_old",
    "playerdata",
    "players",
    "poi",
    "region",
    "resourcepacks",
    "resources.zip",
    "session.lock",
    "stats",
}


class InvalidWorld(ValueError):
    """The selected folder cannot be inventoried as a Java world."""


def _take(data: bytes, pos: int, size: int) -> tuple[bytes, int]:
    if size < 0 or size > len(data) - pos:
        raise InvalidWorld("truncated NBT")
    return data[pos : pos + size], pos + size


class NbtReader:
    """Bounded NBT reader for the small subset needed by ``level.dat``."""

    def __init__(self, data: bytes):
        self.data = data
        self.pos = 0

    def _take(self, size: int) -> bytes:
        value, self.pos = _take(self.data, self.pos, size)
        return value

    def _byte(self) -> int:
        return self._take(1)[0]

    def _u16(self) -> int:
        return struct.unpack(">H", self._take(2))[0]

    def _i32(self) -> int:
        return struct.unpack(">i", self._take(4))[0]

    def _i64(self) -> int:
        return struct.unpack(">q", self._take(8))[0]

    def _name(self) -> bytes:
        return self._take(self._u16())

    def _string(self) -> str:
        return self._take(self._u16()).decode("utf-8", "replace")

    def _length(self) -> int:
        length = self._i32()
        if length < 0 or length > MAX_ELEMENTS:
            raise InvalidWorld("invalid NBT collection length")
        return length

    def skip_payload(self, kind: int, depth: int) -> None:
        if depth > MAX_DEPTH:
            raise InvalidWorld("NBT nesting limit exceeded")
        if kind in (1, 2, 3, 4, 5, 6):
            self._take({1: 1, 2: 2, 3: 4, 4: 8, 5: 4, 6: 8}[kind])
        elif kind == 7:
            self._take(self._length())
        elif kind == 8:
            self._take(self._u16())
        elif kind == 9:
            element_kind = self._byte()
            count = self._length()
            if element_kind > 12 or (element_kind == 0 and count):
                raise InvalidWorld("invalid NBT list element type")
            if count > len(self.data) - self.pos:
                raise InvalidWorld("truncated NBT list")
            for _ in range(count):
                self.skip_payload(element_kind, depth + 1)
        elif kind == 10:
            while True:
                child = self._byte()
                if child == 0:
                    break
                self._name()
                self.skip_payload(child, depth + 1)
        elif kind == 11:
            self._take(self._length() * 4)
        elif kind == 12:
            self._take(self._length() * 8)
        else:
            raise InvalidWorld(f"unsupported NBT tag {kind}")

    def _read_int_collection(self, kind: int) -> list[int]:
        if kind == 9:
            element_kind = self._byte()
            if element_kind != 3:
                raise InvalidWorld("version_history must contain TAG_Int values")
            count = self._length()
            return [self._i32() for _ in range(count)]
        if kind == 11:
            count = self._length()
            return [self._i32() for _ in range(count)]
        raise InvalidWorld("version_history must be an INT_ARRAY or list of INT")

    def _read_version(self) -> dict[str, object]:
        version: dict[str, object] = {}
        while True:
            kind = self._byte()
            if kind == 0:
                break
            name = self._name()
            if name == b"Id":
                if "id" in version or kind != 3:
                    raise InvalidWorld("duplicate or non-integer Version.Id")
                version["id"] = self._i32()
            elif name == b"Name":
                if "name" in version or kind != 8:
                    raise InvalidWorld("duplicate or non-string Version.Name")
                version["name"] = self._string()
            elif name == b"Snapshot":
                if "snapshot" in version or kind != 1:
                    raise InvalidWorld("duplicate or non-byte Version.Snapshot")
                version["snapshot"] = bool(self._byte())
            else:
                self.skip_payload(kind, 3)
        return version

    def _read_data(self) -> dict[str, object]:
        data: dict[str, object] = {}
        while True:
            kind = self._byte()
            if kind == 0:
                break
            name = self._name()
            if name == b"DataVersion":
                if "dataVersion" in data or kind != 3:
                    raise InvalidWorld("duplicate or non-integer DataVersion")
                data["dataVersion"] = self._i32()
            elif name == b"Version":
                if "version" in data or kind != 10:
                    raise InvalidWorld("duplicate or non-compound Version")
                data["version"] = self._read_version()
            elif name == b"LevelName":
                if "levelName" in data or kind != 8:
                    raise InvalidWorld("duplicate or non-string LevelName")
                data["levelName"] = self._string()
            elif name == b"version_history":
                if "versionHistory" in data:
                    raise InvalidWorld("duplicate version_history")
                data["versionHistory"] = self._read_int_collection(kind)
            else:
                self.skip_payload(kind, 3)
        return data

    def read_level(self) -> dict[str, object]:
        if self._byte() != 10:
            raise InvalidWorld("level.dat root is not a compound")
        self._name()
        data: dict[str, object] | None = None
        while True:
            kind = self._byte()
            if kind == 0:
                break
            name = self._name()
            if name == b"Data":
                if data is not None or kind != 10:
                    raise InvalidWorld("duplicate or non-compound Data tag")
                data = self._read_data()
            else:
                self.skip_payload(kind, 1)
        if data is None:
            raise InvalidWorld("level.dat has no Data compound")
        if "dataVersion" not in data:
            raise InvalidWorld("level.dat has no DataVersion")
        if self.pos != len(self.data):
            raise InvalidWorld("trailing bytes after level.dat NBT")
        return data


def decompress_level_dat(payload: bytes) -> bytes:
    if len(payload) > MAX_LEVEL_DAT_BYTES:
        raise InvalidWorld("level.dat is larger than the safety limit")
    decompressor = zlib.decompressobj(16 + zlib.MAX_WBITS)
    try:
        raw = decompressor.decompress(payload, MAX_LEVEL_NBT_BYTES + 1)
        if len(raw) > MAX_LEVEL_NBT_BYTES:
            raise InvalidWorld("level.dat expands beyond the safety limit")
        raw += decompressor.flush()
    except zlib.error as error:
        raise InvalidWorld(f"level.dat is not valid gzip-compressed NBT: {error}") from error
    if len(raw) > MAX_LEVEL_NBT_BYTES:
        raise InvalidWorld("level.dat expands beyond the safety limit")
    if not decompressor.eof:
        raise InvalidWorld("level.dat gzip stream is truncated")
    if decompressor.unused_data:
        raise InvalidWorld("level.dat has data after the gzip stream")
    return raw


def read_level_dat(path: Path) -> dict[str, object]:
    if path.is_symlink():
        raise InvalidWorld("level.dat is a symlink; refusing to follow it")
    if not path.is_file():
        raise InvalidWorld("level.dat is missing or not a regular file")
    try:
        payload = path.read_bytes()
    except OSError as error:
        raise InvalidWorld(f"level.dat could not be read: {error}") from error
    raw = decompress_level_dat(payload)
    data = NbtReader(raw).read_level()
    version = data.get("version")
    data["version"] = version if isinstance(version, dict) else None
    data["dataVersionAgrees"] = (
        isinstance(version, dict)
        and isinstance(version.get("id"), int)
        and version["id"] == data["dataVersion"]
    )
    return data


def _is_real_dir(path: Path) -> bool:
    return path.is_dir() and not path.is_symlink()


def _is_real_file(path: Path) -> bool:
    return path.is_file() and not path.is_symlink()


def _region_stats(folder: Path) -> dict[str, object]:
    if not _is_real_dir(folder):
        return {
            "present": False,
            "files": 0,
            "bytes": 0,
            "legacyFiles": 0,
            "legacyBytes": 0,
        }
    files = 0
    total = 0
    legacy_files = 0
    legacy_total = 0
    with os.scandir(folder) as entries:
        for entry in entries:
            if not entry.is_file(follow_symlinks=False):
                continue
            size = entry.stat(follow_symlinks=False).st_size
            if entry.name.endswith(".mca"):
                files += 1
                total += size
            elif entry.name.endswith(".mcr"):
                legacy_files += 1
                legacy_total += size
    return {
        "present": True,
        "files": files,
        "bytes": total,
        "legacyFiles": legacy_files,
        "legacyBytes": legacy_total,
    }


def _dimension(world: Path, dimension_id: str, folder: Path) -> dict[str, object]:
    relative = folder.relative_to(world).as_posix() or "."
    record = {
        "id": dimension_id,
        "folder": relative,
        "region": _region_stats(folder / "region"),
        "entities": _is_real_dir(folder / "entities"),
        "poi": _is_real_dir(folder / "poi"),
        "data": _is_real_dir(folder / "data"),
    }
    if folder == world:
        record["folder"] = "."
    return record


def _custom_dimensions(world: Path) -> list[dict[str, object]]:
    root = world / "dimensions"
    if not _is_real_dir(root):
        return []
    dimensions = []
    with os.scandir(root) as namespaces:
        for namespace in namespaces:
            if not namespace.is_dir(follow_symlinks=False) or namespace.is_symlink():
                continue
            namespace_path = Path(namespace.path)
            with os.scandir(namespace_path) as paths:
                for path in paths:
                    if not path.is_dir(follow_symlinks=False) or path.is_symlink():
                        continue
                    folder = Path(path.path)
                    dimensions.append(
                        _dimension(world, f"{namespace.name}:{path.name}", folder)
                    )
    return sorted(dimensions, key=lambda item: str(item["id"]))


def _top_level(world: Path) -> dict[str, object]:
    root_region = _region_stats(world / "region")
    markers = {
        "rootRegion": _is_real_dir(world / "region"),
        "rootMcaRegion": int(root_region["files"]) > 0,
        "rootMcrRegion": int(root_region["legacyFiles"]) > 0,
        "rootEntities": _is_real_dir(world / "entities"),
        "rootPoi": _is_real_dir(world / "poi"),
        "netherDimFolder": _is_real_dir(world / "DIM-1"),
        "endDimFolder": _is_real_dir(world / "DIM1"),
        "modernOverworldRegion": _is_real_dir(
            world / "dimensions" / "minecraft" / "overworld" / "region"
        ),
        "worldGenSettingsFile": _is_real_file(
            world / "data" / "minecraft" / "world_gen_settings.dat"
        ),
        "playersDirectory": _is_real_dir(world / "players"),
        "rootPlayerdata": _is_real_dir(world / "playerdata"),
        "rootStats": _is_real_dir(world / "stats"),
        "rootAdvancements": _is_real_dir(world / "advancements"),
        "rootResourcesZip": _is_real_file(world / "resources.zip"),
        "resourcepacksZip": _is_real_file(world / "resourcepacks" / "resources.zip"),
    }
    unrecognized: list[str] = []
    with os.scandir(world) as entries:
        for entry in entries:
            if entry.name in KNOWN_TOP_LEVEL:
                continue
            if len(unrecognized) < MAX_UNRECOGNIZED_ENTRIES:
                unrecognized.append(entry.name)
    markers["unrecognizedTopLevel"] = sorted(unrecognized)
    return markers


def _layout(markers: dict[str, object]) -> tuple[str, list[str]]:
    issues: list[str] = []
    modern = bool(markers["modernOverworldRegion"]) or (
        bool(markers["playersDirectory"]) and bool(markers["worldGenSettingsFile"])
    )
    if modern:
        layout = "modern_26_1"
        if markers["rootRegion"]:
            issues.append("modern layout still has a root region folder")
        if markers["netherDimFolder"] or markers["endDimFolder"]:
            issues.append("modern layout still has DIM-1 or DIM1 folders")
    elif markers["rootRegion"]:
        if markers["rootMcrRegion"] and not markers["rootMcaRegion"]:
            layout = "legacy_mcr"
            issues.append("MCRegion (.mcr) worlds are out of scope")
        else:
            layout = "classic_anvil"
            if markers["rootMcrRegion"]:
                issues.append("classic Anvil layout also contains legacy .mcr files")
        if markers["playersDirectory"]:
            issues.append("classic layout has a players folder")
        if markers["worldGenSettingsFile"]:
            issues.append("classic layout has a split world_gen_settings.dat")
    else:
        layout = "unknown"
        issues.append("neither region/ nor dimensions/minecraft/overworld/region exists")
    return layout, issues


def _layout_consistency(layout: str, data_version: int | None) -> str:
    if not isinstance(data_version, int) or layout in ("unknown", "legacy_mcr"):
        return "unknown"
    if LAST_CLASSIC_DATA_VERSION < data_version < FIRST_MODERN_DATA_VERSION:
        # 26.1 snapshots straddle the folder change; do not guess from a number.
        return "unknown"
    if layout == "classic_anvil":
        return "consistent" if data_version <= LAST_CLASSIC_DATA_VERSION else "inconsistent"
    if layout == "modern_26_1":
        return "consistent" if data_version >= FIRST_MODERN_DATA_VERSION else "inconsistent"
    return "unknown"


def inspect_world(folder: str | os.PathLike[str]) -> dict[str, object]:
    try:
        world = Path(folder).expanduser().resolve(strict=True)
    except OSError as error:
        raise InvalidWorld(f"world folder does not exist: {error}") from error
    if not world.is_dir():
        raise InvalidWorld("world path is not a directory")

    level = read_level_dat(world / "level.dat")
    markers = _top_level(world)
    layout, layout_issues = _layout(markers)
    data_version = level["dataVersion"]
    version = level["version"] if isinstance(level.get("version"), dict) else None
    version_id = version.get("id") if version else None
    version_name = version.get("name") if version else None
    version_snapshot = version.get("snapshot") if version else None

    issues = list(layout_issues)
    if version is None:
        issues.append("level.dat has no Version compound")
    elif not isinstance(version_id, int):
        issues.append("level.dat has no integer Version.Id")
    elif version_id != data_version:
        issues.append(
            f"DataVersion {data_version} does not match Version.Id {version_id}"
        )

    dimensions: list[dict[str, object]] = []
    if layout in ("classic_anvil", "legacy_mcr"):
        dimensions.append(_dimension(world, "minecraft:overworld", world))
        for folder_name, dimension_id in (
            ("DIM-1", "minecraft:the_nether"),
            ("DIM1", "minecraft:the_end"),
        ):
            if _is_real_dir(world / folder_name):
                dimensions.append(_dimension(world, dimension_id, world / folder_name))
    dimensions.extend(_custom_dimensions(world))
    unique: list[dict[str, object]] = []
    seen: set[str] = set()
    for dimension in dimensions:
        dimension_id = str(dimension["id"])
        if dimension_id in seen:
            issues.append(f"duplicate dimension id {dimension_id}")
            continue
        seen.add(dimension_id)
        unique.append(dimension)
    dimensions = sorted(unique, key=lambda item: str(item["id"]))

    layout_consistency = _layout_consistency(layout, data_version)
    if layout_consistency == "inconsistent":
        issues.append(
            f"layout {layout} is not consistent with DataVersion {data_version}"
        )

    target_match = (
        data_version == TARGET_DATA_VERSION
        and layout == "classic_anvil"
        and level["dataVersionAgrees"]
    )

    return {
        "tool": {"name": TOOL_NAME, "version": TOOL_VERSION},
        "inspectedAt": datetime.now(timezone.utc).isoformat(),
        "worldFolder": str(world),
        "layout": layout,
        "layoutConsistency": layout_consistency,
        "dataVersion": data_version,
        "version": {
            "id": version_id,
            "name": version_name,
            "snapshot": version_snapshot,
        },
        "dataVersionAgrees": level["dataVersionAgrees"],
        "levelName": level.get("levelName"),
        "versionHistory": level.get("versionHistory"),
        "dimensions": dimensions,
        "markers": markers,
        "compatibility": {
            "targetDataVersion": TARGET_DATA_VERSION,
            "targetVersion": TARGET_VERSION_NAME,
            "targetMatch": target_match,
            "readImplemented": False,
            "writeVerified": False,
        },
        "issues": issues,
        "readOnly": True,
    }


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(
        description="Read-only inventory of a Minecraft Java world folder."
    )
    parser.add_argument("world", help="world folder that contains level.dat")
    parser.add_argument(
        "--require-target",
        action="store_true",
        help="exit 1 unless the world matches DataVersion 4189 and classic layout",
    )
    parser.add_argument(
        "--expect-data-version",
        type=int,
        help="exit 1 unless level.dat reports this DataVersion",
    )
    parser.add_argument(
        "--compact",
        action="store_true",
        help="print one-line JSON instead of indented JSON",
    )
    args = parser.parse_args(argv)

    try:
        inventory = inspect_world(args.world)
    except InvalidWorld as error:
        print(f"invalid_world: {error}", file=sys.stderr)
        return 2

    print(
        json.dumps(
            inventory,
            ensure_ascii=False,
            indent=None if args.compact else 2,
            sort_keys=False,
        )
    )

    if args.expect_data_version is not None:
        if inventory["dataVersion"] != args.expect_data_version:
            print(
                f"data_version_mismatch: expected {args.expect_data_version}, "
                f"found {inventory['dataVersion']}",
                file=sys.stderr,
            )
            return 1
    if args.require_target and not inventory["compatibility"]["targetMatch"]:
        print(
            "target_mismatch: world is not a supported 1.21.4 classic world",
            file=sys.stderr,
        )
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
