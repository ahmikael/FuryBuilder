#!/usr/bin/env python3
"""Compare two independently produced, complete chunk samples.

This is a test oracle, not a world reader or generator. It never starts Java,
accepts an EULA, or downloads game files. See docs/formats.md
for the snapshot contract and why passing this test does not establish
general vanilla parity.
"""

from __future__ import annotations

import argparse
import json
import re
import sys
from pathlib import Path

MAX_SAMPLE_BYTES = 128 * 1024 * 1024
MAX_CHUNKS = 16
DIMENSIONS = {
    "minecraft:overworld": (-64, 384),
    "minecraft:the_nether": (0, 256),
    "minecraft:the_end": (0, 256),
}
SERVER_SHA1 = "4707d00eb834b446575d89a61a11b5d548d8c001"
NAMESPACED = re.compile(r"^[a-z0-9_.-]+:[a-z0-9_./-]+$")
SHA1 = re.compile(r"^[0-9a-f]{40}$")
SHA256 = re.compile(r"^[0-9a-f]{64}$")


class InvalidSample(ValueError):
    """A sample cannot support a meaningful comparison."""


def integer(value: object, name: str) -> int:
    if not isinstance(value, int) or isinstance(value, bool):
        raise InvalidSample(f"{name} must be an integer")
    return value


def resource(value: object, name: str) -> str:
    if not isinstance(value, str) or not NAMESPACED.fullmatch(value):
        raise InvalidSample(f"{name} must be a namespaced identifier")
    return value


def require_keys(obj: object, keys: set[str], name: str) -> dict:
    if not isinstance(obj, dict) or set(obj) != keys:
        raise InvalidSample(f"{name} must have exactly: {', '.join(sorted(keys))}")
    return obj


def validate_config(config: object) -> dict:
    keys = {
        "minecraft_version", "data_version", "server_sha1", "seed", "preset",
        "generator_settings", "generate_structures", "datapacks", "generation_order",
        "dimension", "min_y", "height",
    }
    config = require_keys(config, keys, "config")
    if config["minecraft_version"] != "1.21.4" or integer(config["data_version"], "data_version") != 4189:
        raise InvalidSample("only the selected Java 1.21.4 / DataVersion 4189 is accepted")
    if (
        not isinstance(config["server_sha1"], str)
        or not SHA1.fullmatch(config["server_sha1"])
        or config["server_sha1"] != SERVER_SHA1
    ):
        raise InvalidSample("server_sha1 must match the official 1.21.4 server metadata")
    seed = integer(config["seed"], "seed")
    if not -(2**63) <= seed < 2**63:
        raise InvalidSample("seed is outside signed 64-bit range")
    resource(config["preset"], "preset")
    dimension = resource(config["dimension"], "dimension")
    if not isinstance(config["generator_settings"], dict):
        raise InvalidSample("generator_settings must be an object")
    if not isinstance(config["generate_structures"], bool):
        raise InvalidSample("generate_structures must be boolean")
    packs = config["datapacks"]
    if not isinstance(packs, list):
        raise InvalidSample("datapacks must be an ordered list")
    for pack in packs:
        pack = require_keys(pack, {"id", "sha256"}, "datapack")
        if not isinstance(pack["id"], str) or not pack["id"]:
            raise InvalidSample("datapack id must be non-empty")
        if not isinstance(pack["sha256"], str) or not SHA256.fullmatch(pack["sha256"]):
            raise InvalidSample("datapack sha256 must be lowercase")
    min_y = integer(config["min_y"], "min_y")
    height = integer(config["height"], "height")
    if DIMENSIONS.get(dimension) != (min_y, height):
        raise InvalidSample("sample must use the full vanilla height of a supported dimension")
    order = config["generation_order"]
    if not isinstance(order, list) or not 1 <= len(order) <= MAX_CHUNKS:
        raise InvalidSample("generation_order must contain 1..16 chunk positions")
    seen = set()
    for pos in order:
        if not isinstance(pos, list) or len(pos) != 2:
            raise InvalidSample("chunk position must contain x and z")
        x, z = (integer(axis, "chunk coordinate") for axis in pos)
        if not all(-(2**31) <= axis < 2**31 for axis in (x, z)) or (x, z) in seen:
            raise InvalidSample("duplicate or out-of-range chunk coordinate")
        seen.add((x, z))
    return config


def block_state(state: object) -> tuple:
    state = require_keys(state, {"name", "properties"}, "block state")
    name = resource(state["name"], "block name")
    props = state["properties"]
    if not isinstance(props, dict) or any(
        not isinstance(k, str) or not k or not isinstance(v, str) or not v
        for k, v in props.items()
    ):
        raise InvalidSample("block properties must contain non-empty strings")
    return name, tuple(sorted(props.items()))


def decode_palette(palette: object, indices: object, count: int, is_block: bool) -> tuple:
    if not isinstance(palette, list) or not 1 <= len(palette) <= count:
        raise InvalidSample("invalid palette length")
    values = [
        block_state(value) if is_block else resource(value, "biome")
        for value in palette
    ]
    if not isinstance(indices, list) or len(indices) != count:
        raise InvalidSample(f"expected exactly {count} palette indices")
    result = []
    for index in indices:
        index = integer(index, "palette index")
        if not 0 <= index < len(values):
            raise InvalidSample("palette index is out of bounds")
        result.append(values[index])
    return tuple(result)


def parse_sample(document: object) -> tuple[dict, dict]:
    document = require_keys(document, {"schema_version", "producer", "config", "chunks"}, "sample")
    if integer(document["schema_version"], "schema_version") != 1 or not isinstance(document["producer"], str):
        raise InvalidSample("invalid schema_version or producer")
    if not document["producer"].strip():
        raise InvalidSample("producer must identify the independent source")
    config = validate_config(document["config"])
    chunks = document["chunks"]
    if not isinstance(chunks, list) or len(chunks) != len(config["generation_order"]):
        raise InvalidSample("chunk count does not match the requested sample")
    expected_sections = set(range(config["min_y"] // 16, (config["min_y"] + config["height"]) // 16))
    result = {}
    for chunk in chunks:
        chunk = require_keys(chunk, {"x", "z", "sections"}, "chunk")
        pos = (integer(chunk["x"], "x"), integer(chunk["z"], "z"))
        if pos in result or list(pos) not in config["generation_order"]:
            raise InvalidSample("duplicate or unrequested chunk")
        sections = chunk["sections"]
        if not isinstance(sections, list) or len(sections) != len(expected_sections):
            raise InvalidSample("sample must contain every section, including empty sections")
        parsed = {}
        for section in sections:
            section = require_keys(
                section, {"y", "block_palette", "block_indices", "biome_palette", "biome_indices"},
                "section",
            )
            y = integer(section["y"], "section y")
            if y in parsed or y not in expected_sections:
                raise InvalidSample("duplicate or out-of-range section")
            parsed[y] = (
                decode_palette(section["block_palette"], section["block_indices"], 4096, True),
                decode_palette(section["biome_palette"], section["biome_indices"], 64, False),
            )
        result[pos] = parsed
    return config, result


def compare(reference: object, candidate: object) -> list[str]:
    reference_config, reference_chunks = parse_sample(reference)
    candidate_config, candidate_chunks = parse_sample(candidate)
    if json.dumps(reference_config, sort_keys=True, allow_nan=False) != json.dumps(
        candidate_config, sort_keys=True, allow_nan=False
    ):
        raise InvalidSample("worldgen configurations differ; parity cannot be evaluated")
    mismatches = []
    for pos in sorted(reference_chunks):
        for y in sorted(reference_chunks[pos]):
            for label, left, right in zip(
                ("blocks", "biomes"), reference_chunks[pos][y], candidate_chunks[pos][y]
            ):
                for index, (a, b) in enumerate(zip(left, right)):
                    if a != b:
                        mismatches.append(f"chunk {pos}, section {y}, {label}[{index}]")
                        if len(mismatches) == 20:
                            return mismatches
    return mismatches


def load(path: Path) -> object:
    if path.stat().st_size > MAX_SAMPLE_BYTES:
        raise InvalidSample("sample exceeds the file size limit")

    def unique_keys(pairs):
        obj = {}
        for key, value in pairs:
            if key in obj:
                raise InvalidSample(f"duplicate JSON key: {key}")
            obj[key] = value
        return obj

    def invalid_constant(value):
        raise InvalidSample(f"invalid JSON constant: {value}")

    with path.open("r", encoding="utf-8") as stream:
        return json.load(stream, object_pairs_hook=unique_keys, parse_constant=invalid_constant)


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("reference", type=Path)
    parser.add_argument("candidate", type=Path)
    args = parser.parse_args(argv)
    try:
        differences = compare(load(args.reference), load(args.candidate))
        if differences:
            print("sample differs:\n" + "\n".join(differences), file=sys.stderr)
            return 1
        print("sampled chunks match; this is not a claim of general vanilla parity")
        return 0
    except (OSError, UnicodeError, ValueError, RecursionError) as exc:
        print(f"cannot compare samples: {exc}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
