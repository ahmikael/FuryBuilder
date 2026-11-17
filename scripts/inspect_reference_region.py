#!/usr/bin/env python3
"""Read root DataVersion tags from a user-supplied Java Anvil region.

This independent, read-only verification utility is not an import adapter. It
never downloads, changes, or redistributes a world. Only the standard library
is required. Unknown compression types, malformed NBT and inconsistent region
locations are errors rather than evidence of a particular version.
"""

from __future__ import annotations

import argparse
import collections
import json
import os
import struct
import sys
import zlib
from pathlib import Path

SECTOR_SIZE = 4096
HEADER_SIZE = 2 * SECTOR_SIZE
MAX_CHUNK_BYTES = 64 * 1024 * 1024
MAX_DEPTH = 64
MAX_ELEMENTS = 8_000_000


class InvalidReference(ValueError):
    """The input cannot establish the DataVersion of an Anvil chunk."""


class NbtReader:
    def __init__(self, data: bytes):
        self.data = data
        self.pos = 0

    def take(self, size: int) -> bytes:
        if size < 0 or size > len(self.data) - self.pos:
            raise InvalidReference("truncated NBT")
        start = self.pos
        self.pos += size
        return self.data[start : self.pos]

    def byte(self) -> int:
        return self.take(1)[0]

    def u16(self) -> int:
        return struct.unpack(">H", self.take(2))[0]

    def i32(self) -> int:
        return struct.unpack(">i", self.take(4))[0]

    def name(self) -> bytes:
        return self.take(self.u16())

    def length(self) -> int:
        length = self.i32()
        if length < 0 or length > MAX_ELEMENTS:
            raise InvalidReference("invalid NBT collection length")
        return length

    def skip_payload(self, kind: int, depth: int) -> None:
        if depth > MAX_DEPTH:
            raise InvalidReference("NBT nesting limit exceeded")
        if kind in (1, 2, 3, 4, 5, 6):
            self.take({1: 1, 2: 2, 3: 4, 4: 8, 5: 4, 6: 8}[kind])
        elif kind == 7:
            self.take(self.length())
        elif kind == 8:
            self.take(self.u16())
        elif kind == 9:
            element_kind = self.byte()
            count = self.length()
            if element_kind > 12 or (element_kind == 0 and count):
                raise InvalidReference("invalid NBT list element type")
            # Every non-End tag occupies at least one payload byte.
            if count > len(self.data) - self.pos:
                raise InvalidReference("truncated NBT list")
            for _ in range(count):
                self.skip_payload(element_kind, depth + 1)
        elif kind == 10:
            while True:
                child = self.byte()
                if child == 0:
                    break
                self.name()
                self.skip_payload(child, depth + 1)
        elif kind in (11, 12):
            self.take(self.length() * (4 if kind == 11 else 8))
        else:
            raise InvalidReference(f"unsupported NBT tag {kind}")

    def root_data_version(self) -> int:
        if self.byte() != 10:
            raise InvalidReference("NBT root is not a compound")
        self.name()
        version = None
        while True:
            kind = self.byte()
            if kind == 0:
                break
            name = self.name()
            if name == b"DataVersion":
                if version is not None or kind != 3:
                    raise InvalidReference("duplicate or non-integer root DataVersion")
                version = self.i32()
            else:
                self.skip_payload(kind, 1)
        if self.pos != len(self.data):
            raise InvalidReference("trailing bytes after NBT root")
        if version is None or version <= 0:
            raise InvalidReference("missing or invalid root DataVersion")
        return version


def decode_chunk(payload: bytes, compression: int) -> bytes:
    if compression == 3:
        if len(payload) > MAX_CHUNK_BYTES:
            raise InvalidReference("uncompressed chunk exceeds limit")
        return payload
    if compression not in (1, 2):
        raise InvalidReference(f"unsupported compression type {compression}")
    decoder = zlib.decompressobj(31 if compression == 1 else 15)
    try:
        result = decoder.decompress(payload, MAX_CHUNK_BYTES + 1)
    except zlib.error as exc:
        raise InvalidReference("invalid compressed chunk") from exc
    if len(result) > MAX_CHUNK_BYTES or decoder.unconsumed_tail:
        raise InvalidReference("decompressed chunk exceeds limit")
    if not decoder.eof or decoder.unused_data:
        raise InvalidReference("truncated or trailing compressed data")
    return result


def inspect_region(path: Path) -> list[dict[str, int]]:
    """Return one entry per allocated chunk, failing on any invalid chunk."""
    with path.open("rb") as region:
        size = os.fstat(region.fileno()).st_size
        if size < HEADER_SIZE or size % SECTOR_SIZE:
            raise InvalidReference("invalid region file size")
        header = region.read(HEADER_SIZE)
        occupied = {0, 1}
        chunks = []
        for index in range(1024):
            location = int.from_bytes(header[index * 4 : index * 4 + 4], "big")
            if location == 0:
                continue
            sector, count = location >> 8, location & 0xFF
            if sector < 2 or count == 0 or (sector + count) * SECTOR_SIZE > size:
                raise InvalidReference(f"invalid location for chunk {index}")
            sectors = set(range(sector, sector + count))
            if occupied.intersection(sectors):
                raise InvalidReference(f"overlapping sectors for chunk {index}")
            occupied.update(sectors)
            region.seek(sector * SECTOR_SIZE)
            length = struct.unpack(">I", region.read(4))[0]
            if length < 1 or length > count * SECTOR_SIZE - 4:
                raise InvalidReference(f"invalid payload length for chunk {index}")
            compression = region.read(1)[0]
            raw = decode_chunk(region.read(length - 1), compression)
            version = NbtReader(raw).root_data_version()
            chunks.append({"localX": index % 32, "localZ": index // 32, "dataVersion": version})
    if not chunks:
        raise InvalidReference("region contains no chunks")
    return chunks


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("region", type=Path, help="path to a local r.x.z.mca file")
    parser.add_argument("--expect-version", type=int, help="fail if any chunk differs")
    args = parser.parse_args(argv)
    try:
        chunks = inspect_region(args.region)
        counts = dict(sorted(collections.Counter(c["dataVersion"] for c in chunks).items()))
        print(json.dumps({"chunks": len(chunks), "dataVersionCounts": counts}, sort_keys=True))
        if args.expect_version is not None and set(counts) != {args.expect_version}:
            print("reference contains unexpected DataVersion values", file=sys.stderr)
            return 1
        return 0
    except (OSError, InvalidReference) as exc:
        print(f"cannot inspect reference: {exc}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
