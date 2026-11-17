"""Verify the independently captured Minecraft 1.21.4 structure fixtures."""

import gzip
import hashlib
import unittest
from pathlib import Path

from inspect_reference_region import InvalidReference, NbtReader


FIXTURES = Path(__file__).resolve().parent.parent / "fixtures" / "java-1.21.4" / "structures"
EXPECTED_HASHES = {
    "plain.nbt": "35a3f89de502cd9a6f1cc1b7496d4aebbfc336625389ef50dc5d22932849616b",
    "chest_boundary.nbt": "ad4066be63af09d269c8b5c6297548b8c948daaaed2e74df238ea35267f8b7ad",
}
STAIRS = {
    "Name": "minecraft:oak_stairs",
    "Properties": {
        "facing": "east",
        "half": "bottom",
        "shape": "straight",
        "waterlogged": "false",
    },
}


class FixtureReader(NbtReader):
    """Read the limited NBT tag types used by these fixed, hashed fixtures."""

    def payload(self, kind, depth=0):
        if depth > 64:
            raise InvalidReference("NBT nesting limit exceeded")
        if kind == 3:
            return self.i32()
        if kind == 8:
            return self.name().decode("utf-8")
        if kind == 9:
            element_kind = self.byte()
            count = self.length()
            if element_kind == 0 and count:
                raise InvalidReference("non-empty TAG_End list")
            return [self.payload(element_kind, depth + 1) for _ in range(count)]
        if kind == 10:
            value = {}
            while True:
                child_kind = self.byte()
                if child_kind == 0:
                    return value
                name = self.name().decode("utf-8")
                if name in value:
                    raise InvalidReference("duplicate NBT tag")
                value[name] = self.payload(child_kind, depth + 1)
        raise InvalidReference(f"unexpected fixture tag type: {kind}")

    def root(self):
        if self.byte() != 10 or self.name() != b"":
            raise InvalidReference("expected an unnamed root compound")
        value = self.payload(10)
        if self.pos != len(self.data):
            raise InvalidReference("trailing bytes after root compound")
        return value


def states_by_position(root):
    palette = root["palette"]
    result = {}
    for block in root["blocks"]:
        position = tuple(block["pos"])
        if position in result:
            raise ValueError("duplicate position in fixture")
        index = block["state"]
        if index < 0 or index >= len(palette):
            raise ValueError("invalid palette index in fixture")
        result[position] = (palette[index], block.get("nbt"))
    return result


class GameReferenceTests(unittest.TestCase):
    def load_fixture(self, name):
        raw = (FIXTURES / name).read_bytes()
        self.assertEqual(hashlib.sha256(raw).hexdigest(), EXPECTED_HASHES[name])
        uncompressed = gzip.decompress(raw)
        self.assertEqual(NbtReader(uncompressed).root_data_version(), 4189)
        root = FixtureReader(uncompressed).root()
        self.assertEqual(set(root), {"size", "entities", "blocks", "palette", "DataVersion"})
        self.assertEqual(root["DataVersion"], 4189)
        self.assertEqual(root["entities"], [])
        return root

    def test_plain_structure_preserves_directional_block_state(self):
        root = self.load_fixture("plain.nbt")
        self.assertEqual(root["size"], [1, 2, 1])
        self.assertEqual(
            states_by_position(root),
            {
                (0, 0, 0): ({"Name": "minecraft:stone_bricks"}, None),
                (0, 1, 0): (STAIRS, None),
            },
        )

    def test_boundary_structure_contains_air_and_chest_block_entity(self):
        root = self.load_fixture("chest_boundary.nbt")
        self.assertEqual(root["size"], [2, 2, 2])
        stone = ({"Name": "minecraft:stone_bricks"}, None)
        air = ({"Name": "minecraft:air"}, None)
        chest = (
            {
                "Name": "minecraft:chest",
                "Properties": {
                    "facing": "north",
                    "type": "single",
                    "waterlogged": "false",
                },
            },
            {"id": "minecraft:chest", "Items": []},
        )
        self.assertEqual(
            states_by_position(root),
            {
                (0, 0, 0): stone,
                (0, 0, 1): stone,
                (1, 0, 0): stone,
                (1, 0, 1): stone,
                (0, 1, 0): (STAIRS, None),
                (0, 1, 1): air,
                (1, 1, 0): chest,
                (1, 1, 1): air,
            },
        )


if __name__ == "__main__":
    unittest.main()
