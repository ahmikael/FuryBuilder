"""Tests for the read-only Java world-layout inventory."""

import contextlib
import gzip
import io
import json
import os
import struct
import tempfile
import unittest
from pathlib import Path

from inspect_world_layout import (
    InvalidWorld,
    NbtReader,
    decompress_level_dat,
    inspect_world,
    main,
)


def tag_int(name: bytes, value: int) -> bytes:
    return b"\x03" + struct.pack(">H", len(name)) + name + struct.pack(">i", value)


def tag_byte(name: bytes, value: int) -> bytes:
    return b"\x01" + struct.pack(">H", len(name)) + name + struct.pack(">b", value)


def tag_string(name: bytes, value: str) -> bytes:
    raw = value.encode("utf-8")
    return b"\x08" + struct.pack(">H", len(name)) + name + struct.pack(">H", len(raw)) + raw


def tag_compound(name: bytes, children: bytes) -> bytes:
    return b"\x0a" + struct.pack(">H", len(name)) + name + children + b"\x00"


def tag_list_of_int(name: bytes, values: list[int]) -> bytes:
    body = b"".join(struct.pack(">i", value) for value in values)
    return (
        b"\x09"
        + struct.pack(">H", len(name))
        + name
        + b"\x03"
        + struct.pack(">i", len(values))
        + body
    )


def level_dat(
    data_version: int = 4189,
    version_id: int | None = None,
    version_name: str = "1.21.4",
    history: list[int] | None = None,
) -> bytes:
    version_id = data_version if version_id is None else version_id
    version = tag_compound(
        b"Version",
        tag_int(b"Id", version_id)
        + tag_string(b"Name", version_name)
        + tag_byte(b"Snapshot", 0),
    )
    data = (
        tag_int(b"DataVersion", data_version)
        + version
        + tag_string(b"LevelName", "Inventory test")
    )
    if history is not None:
        data += tag_list_of_int(b"version_history", history)
    return gzip.compress(tag_compound(b"", tag_compound(b"Data", data)))


def write_level(world: Path, payload: bytes | None = None) -> None:
    world.mkdir(parents=True, exist_ok=True)
    (world / "level.dat").write_bytes(level_dat() if payload is None else payload)


def touch(path: Path) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(b"")


class WorldLayoutTests(unittest.TestCase):
    def test_inventories_a_classic_world(self):
        with tempfile.TemporaryDirectory() as directory:
            world = Path(directory) / "world"
            touch(world / "level.dat_old")
            touch(world / "session.lock")
            touch(world / "region" / "r.0.0.mca")
            touch(world / "entities" / "r.0.0.mca")
            touch(world / "poi" / "r.0.0.mca")
            touch(world / "DIM-1" / "region" / "r.0.0.mca")
            touch(world / "playerdata" / "player.dat")
            touch(world / "datapacks" / "pack.mcmeta")
            write_level(world, level_dat(history=[4082, 4189]))

            inventory = inspect_world(world)
            self.assertEqual(inventory["layout"], "classic_anvil")
            self.assertEqual(inventory["layoutConsistency"], "consistent")
            self.assertEqual(inventory["dataVersion"], 4189)
            self.assertTrue(inventory["dataVersionAgrees"])
            self.assertEqual(inventory["versionHistory"], [4082, 4189])
            self.assertTrue(inventory["compatibility"]["targetMatch"])
            self.assertEqual(
                [item["id"] for item in inventory["dimensions"]],
                ["minecraft:overworld", "minecraft:the_nether"],
            )
            overworld = inventory["dimensions"][0]
            self.assertEqual(overworld["folder"], ".")
            self.assertEqual(overworld["region"]["files"], 1)
            self.assertTrue(overworld["entities"])
            self.assertTrue(inventory["markers"]["rootPlayerdata"])
            self.assertFalse(inventory["markers"]["worldGenSettingsFile"])

    def test_inventories_the_26_1_split_layout(self):
        with tempfile.TemporaryDirectory() as directory:
            world = Path(directory) / "world"
            for dimension in ("overworld", "the_nether", "the_end"):
                touch(world / "dimensions" / "minecraft" / dimension / "region" / "r.0.0.mca")
                touch(world / "dimensions" / "minecraft" / dimension / "poi" / "r.0.0.mca")
            touch(world / "data" / "minecraft" / "world_gen_settings.dat")
            touch(world / "players" / "data" / "player.dat")
            touch(world / "players" / "stats" / "player.json")
            touch(world / "resourcepacks" / "resources.zip")
            write_level(
                world,
                level_dat(4786, version_name="26.1", history=[4189, 4786]),
            )

            inventory = inspect_world(world)
            self.assertEqual(inventory["layout"], "modern_26_1")
            self.assertEqual(inventory["layoutConsistency"], "consistent")
            self.assertFalse(inventory["compatibility"]["targetMatch"])
            self.assertEqual(
                [item["id"] for item in inventory["dimensions"]],
                [
                    "minecraft:overworld",
                    "minecraft:the_end",
                    "minecraft:the_nether",
                ],
            )
            self.assertTrue(inventory["markers"]["playersDirectory"])
            self.assertTrue(inventory["markers"]["worldGenSettingsFile"])
            self.assertTrue(inventory["markers"]["resourcepacksZip"])
            self.assertEqual(inventory["issues"], [])

            with contextlib.redirect_stdout(io.StringIO()), contextlib.redirect_stderr(
                io.StringIO()
            ):
                self.assertEqual(main([str(world), "--require-target"]), 1)

    def test_flags_a_version_disagreement_and_a_layout_mismatch(self):
        with tempfile.TemporaryDirectory() as directory:
            world = Path(directory) / "world"
            touch(world / "region" / "r.0.0.mca")
            write_level(world, level_dat(4189, version_id=4325, version_name="1.21.5"))

            inventory = inspect_world(world)
            self.assertFalse(inventory["dataVersionAgrees"])
            self.assertFalse(inventory["compatibility"]["targetMatch"])
            self.assertTrue(
                any("Version.Id 4325" in issue for issue in inventory["issues"])
            )

            modern = Path(directory) / "modern"
            touch(modern / "dimensions" / "minecraft" / "overworld" / "region" / "r.0.0.mca")
            write_level(modern, level_dat(4189))
            inventory = inspect_world(modern)
            self.assertEqual(inventory["layout"], "modern_26_1")
            self.assertEqual(inventory["layoutConsistency"], "inconsistent")
            self.assertTrue(
                any("not consistent" in issue for issue in inventory["issues"])
            )

    def test_marks_mcregion_worlds_as_out_of_scope(self):
        with tempfile.TemporaryDirectory() as directory:
            world = Path(directory) / "world"
            touch(world / "region" / "r.0.0.mcr")
            write_level(world, level_dat(1343, version_name="1.12.2"))
            inventory = inspect_world(world)
            self.assertEqual(inventory["layout"], "legacy_mcr")
            self.assertEqual(inventory["layoutConsistency"], "unknown")
            self.assertFalse(inventory["compatibility"]["targetMatch"])
            self.assertTrue(
                any("out of scope" in issue for issue in inventory["issues"])
            )
            self.assertEqual(inventory["dimensions"][0]["region"]["legacyFiles"], 1)

    def test_transition_snapshot_is_not_forced_into_one_layout(self):
        with tempfile.TemporaryDirectory() as directory:
            world = Path(directory) / "world"
            touch(world / "region" / "r.0.0.mca")
            write_level(world, level_dat(4770, version_name="26.1 Snapshot 5"))
            inventory = inspect_world(world)
            self.assertEqual(inventory["layout"], "classic_anvil")
            self.assertEqual(inventory["layoutConsistency"], "unknown")
            self.assertFalse(inventory["compatibility"]["targetMatch"])

    def test_cli_reports_the_expected_version_and_target(self):
        with tempfile.TemporaryDirectory() as directory:
            world = Path(directory) / "world"
            touch(world / "region" / "r.0.0.mca")
            write_level(world)
            with contextlib.redirect_stdout(io.StringIO()) as output, contextlib.redirect_stderr(
                io.StringIO()
            ):
                self.assertEqual(main([str(world), "--require-target"]), 0)
            result = json.loads(output.getvalue())
            self.assertEqual(result["dataVersion"], 4189)
            self.assertEqual(result["tool"]["name"], "furybuilder-world-layout-inspector")
            with contextlib.redirect_stdout(io.StringIO()), contextlib.redirect_stderr(
                io.StringIO()
            ):
                self.assertEqual(main([str(world), "--expect-data-version", "4325"]), 1)

    def test_refuses_a_symlinked_level_dat(self):
        with tempfile.TemporaryDirectory() as directory:
            world = Path(directory) / "world"
            world.mkdir()
            real = Path(directory) / "real-level.dat"
            real.write_bytes(level_dat())
            try:
                os.symlink(real, world / "level.dat")
            except (OSError, NotImplementedError) as error:
                self.skipTest(f"symlinks are unavailable: {error}")
            with self.assertRaisesRegex(InvalidWorld, "symlink"):
                inspect_world(world)

    def test_rejects_missing_corrupt_or_ambiguous_level_dat(self):
        with tempfile.TemporaryDirectory() as directory:
            world = Path(directory) / "world"
            world.mkdir()
            with self.assertRaisesRegex(InvalidWorld, "level.dat is missing"):
                inspect_world(world)

            (world / "level.dat").write_bytes(b"not gzip")
            with self.assertRaisesRegex(InvalidWorld, "gzip"):
                inspect_world(world)

            payload = level_dat() + b"\x00"
            (world / "level.dat").write_bytes(payload)
            with self.assertRaisesRegex(InvalidWorld, "data after the gzip stream"):
                inspect_world(world)

            duplicate = tag_compound(
                b"",
                tag_compound(b"Data", tag_int(b"DataVersion", 4189))
                + tag_compound(b"Data", tag_int(b"DataVersion", 4189)),
            )
            (world / "level.dat").write_bytes(gzip.compress(duplicate))
            with self.assertRaisesRegex(InvalidWorld, "duplicate"):
                inspect_world(world)

    def test_decompression_is_bounded_and_requires_a_compound(self):
        with self.assertRaisesRegex(InvalidWorld, "larger than the safety limit"):
            decompress_level_dat(b"\x00" * (8 * 1024 * 1024 + 1))
        with self.assertRaisesRegex(InvalidWorld, "truncated"):
            decompress_level_dat(b"\x00")
        raw = tag_compound(b"", tag_int(b"Wrong", 4189))
        with self.assertRaisesRegex(InvalidWorld, "no Data compound"):
            NbtReader(raw).read_level()
        with self.assertRaisesRegex(InvalidWorld, "not a compound"):
            NbtReader(b"\x03\x00\x00\x00\x00\x00\x01").read_level()


if __name__ == "__main__":
    unittest.main()
