"""Synthetic tests of the parity oracle; these do not claim Minecraft parity."""

import copy
import contextlib
import io
import json
import tempfile
import unittest
from pathlib import Path

from compare_worldgen_samples import SERVER_SHA1, InvalidSample, compare, load, main


def sample():
    section = {
        "y": -1,
        "block_palette": [{"name": "minecraft:air", "properties": {}},
                          {"name": "minecraft:oak_stairs", "properties": {"facing": "north"}}],
        "block_indices": [0] * 4096,
        "biome_palette": ["minecraft:plains", "minecraft:forest"],
        "biome_indices": [0] * 64,
    }
    section["block_indices"][15] = 1
    section["biome_indices"][3] = 1
    return {
        "schema_version": 1,
        "producer": "synthetic test input",
        "config": {
            "minecraft_version": "1.21.4", "data_version": 4189,
            "server_sha1": SERVER_SHA1, "seed": 12345, "preset": "minecraft:normal",
            "generator_settings": {}, "generate_structures": True, "datapacks": [],
            "generation_order": [[-1, 0]], "dimension": "minecraft:overworld",
            "min_y": -64, "height": 384,
        },
        "chunks": [{"x": -1, "z": 0, "sections": [
            {**copy.deepcopy(section), "y": y} for y in range(-4, 20)
        ]}],
    }


class ParityOracleTests(unittest.TestCase):
    def test_compares_semantics_not_palette_order(self):
        reference = sample()
        candidate = copy.deepcopy(reference)
        section = candidate["chunks"][0]["sections"][0]
        section["block_palette"].reverse()
        section["block_indices"] = [1 - i for i in section["block_indices"]]
        section["biome_palette"].reverse()
        section["biome_indices"] = [1 - i for i in section["biome_indices"]]
        self.assertEqual(compare(reference, candidate), [])

    def test_reports_block_properties_and_biome_differences(self):
        reference = sample()
        candidate = copy.deepcopy(reference)
        section = candidate["chunks"][0]["sections"][0]
        section["block_palette"][1]["properties"]["facing"] = "south"
        section["biome_indices"][3] = 0
        differences = compare(reference, candidate)
        self.assertTrue(any("blocks[15]" in entry for entry in differences))
        self.assertTrue(any("biomes[3]" in entry for entry in differences))

    def test_rejects_mismatched_generation_inputs(self):
        for key, value in (("seed", 4), ("preset", "minecraft:flat"),
                           ("generate_structures", False)):
            candidate = sample()
            candidate["config"][key] = value
            with self.subTest(key=key), self.assertRaisesRegex(InvalidSample, "configurations"):
                compare(sample(), candidate)

    def test_rejects_missing_sections_and_invalid_indices(self):
        missing = sample()
        missing["chunks"][0]["sections"] = []
        with self.assertRaises(InvalidSample):
            compare(sample(), missing)
        invalid = sample()
        invalid["chunks"][0]["sections"][0]["block_indices"][0] = 42
        with self.assertRaises(InvalidSample):
            compare(sample(), invalid)

    def test_rejects_unknown_server_and_partial_height(self):
        candidate = sample()
        candidate["config"]["server_sha1"] = "1" * 40
        with self.assertRaises(InvalidSample):
            compare(sample(), candidate)
        candidate = sample()
        candidate["config"]["min_y"] = -16
        candidate["config"]["height"] = 16
        with self.assertRaises(InvalidSample):
            compare(sample(), candidate)

    def test_rejects_ambiguous_json_and_nonfinite_numbers(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "sample.json"
            for text in ('{"a":1,"a":2}', '{"value":NaN}'):
                path.write_text(text, encoding="utf-8")
                with self.subTest(text=text), self.assertRaises(InvalidSample):
                    load(path)

    def test_cli_distinguishes_match_difference_and_invalid_sample(self):
        with tempfile.TemporaryDirectory() as directory:
            reference = Path(directory) / "reference.json"
            candidate = Path(directory) / "candidate.json"
            reference.write_text(json.dumps(sample()), encoding="utf-8")
            changed = sample()
            candidate.write_text(json.dumps(changed), encoding="utf-8")
            with contextlib.redirect_stdout(io.StringIO()) as output:
                self.assertEqual(main([str(reference), str(candidate)]), 0)
            self.assertIn("not a claim of general vanilla parity", output.getvalue())
            changed["chunks"][0]["sections"][0]["block_indices"][0] = 1
            candidate.write_text(json.dumps(changed), encoding="utf-8")
            with contextlib.redirect_stderr(io.StringIO()):
                self.assertEqual(main([str(reference), str(candidate)]), 1)
            candidate.write_text("{}", encoding="utf-8")
            with contextlib.redirect_stderr(io.StringIO()):
                self.assertEqual(main([str(reference), str(candidate)]), 2)


if __name__ == "__main__":
    unittest.main()
