"""Tests for the independent reference-region inspection utility."""

import contextlib
import io
import struct
import tempfile
import unittest
import zlib
from pathlib import Path

from inspect_reference_region import InvalidReference, NbtReader, inspect_region, main


def named_int(name: bytes, value: int) -> bytes:
    return b"\x03" + struct.pack(">H", len(name)) + name + struct.pack(">i", value)


def nbt(version: int = 4189) -> bytes:
    return b"\x0a\x00\x00" + named_int(b"DataVersion", version) + b"\x00"


def region_bytes(*chunks: bytes) -> bytes:
    locations = bytearray(4096)
    body = bytearray()
    for index, chunk in enumerate(chunks):
        payload = zlib.compress(chunk)
        record = struct.pack(">I", len(payload) + 1) + b"\x02" + payload
        sectors = (len(record) + 4095) // 4096
        location = ((2 + len(body) // 4096) << 8) | sectors
        locations[index * 4 : index * 4 + 4] = location.to_bytes(4, "big")
        body.extend(record.ljust(sectors * 4096, b"\x00"))
    return bytes(locations) + bytes(4096) + bytes(body)


class ReferenceRegionTests(unittest.TestCase):
    def test_reads_each_allocated_chunk(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "r.0.0.mca"
            path.write_bytes(region_bytes(nbt(), nbt()))
            chunks = inspect_region(path)
            self.assertEqual([c["dataVersion"] for c in chunks], [4189, 4189])
            self.assertEqual([(c["localX"], c["localZ"]) for c in chunks], [(0, 0), (1, 0)])
            with contextlib.redirect_stdout(io.StringIO()) as output:
                self.assertEqual(main([str(path), "--expect-version", "4189"]), 0)
            self.assertIn('"4189": 2', output.getvalue())
            with contextlib.redirect_stdout(io.StringIO()), contextlib.redirect_stderr(io.StringIO()):
                self.assertEqual(main([str(path), "--expect-version", "4188"]), 1)

    def test_rejects_missing_nested_duplicate_and_noninteger_versions(self):
        nested = b"\x0a\x00\x00\x0a\x00\x01x" + named_int(b"DataVersion", 4189) + b"\x00\x00"
        duplicate = b"\x0a\x00\x00" + named_int(b"DataVersion", 4189) * 2 + b"\x00"
        wrong_type = b"\x0a\x00\x00\x08\x00\x0bDataVersion\x00\x044189\x00"
        for payload in (b"\x0a\x00\x00\x00", nested, duplicate, wrong_type):
            with self.subTest(payload=payload), self.assertRaises(InvalidReference):
                NbtReader(payload).root_data_version()

    def test_rejects_truncated_and_overlapping_region_data(self):
        data = bytearray(region_bytes(nbt(), nbt()))
        data[4:8] = data[0:4]
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "r.0.0.mca"
            path.write_bytes(data)
            with self.assertRaisesRegex(InvalidReference, "overlapping"):
                inspect_region(path)
            path.write_bytes(region_bytes(nbt())[:-1])
            with self.assertRaises(InvalidReference):
                inspect_region(path)

    def test_rejects_corrupt_compression_and_trailing_nbt(self):
        with self.assertRaisesRegex(InvalidReference, "trailing"):
            NbtReader(nbt() + b"\x00").root_data_version()
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "r.0.0.mca"
            data = bytearray(region_bytes(nbt()))
            data[8197] ^= 0xFF
            path.write_bytes(data)
            with self.assertRaises(InvalidReference):
                inspect_region(path)


if __name__ == "__main__":
    unittest.main()
