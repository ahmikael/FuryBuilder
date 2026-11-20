"""Validate the committed M11.4 demo project, screenshots, MCP probe and documentation.

These checks are intentionally independent from the desktop app. They prove that the
committed demo folder is a real, checksum-valid FuryBuilder project, that every recorded
image exists with the published digest and is not blank, that the MCP probe is truthful,
and that the public documentation explains the boundaries.
"""

import hashlib
import json
import re
import struct
import unittest
import zlib
from pathlib import Path


ROOT = Path(__file__).resolve().parent.parent
DEMO = ROOT / "docs" / "demo"
SCREENSHOTS = DEMO / "screenshots"
EXAMPLES = ROOT / "examples" / "furybuilder-demo.furybuilder"
PROJECT = EXAMPLES / "project.json"
SHA256 = re.compile(r"^[0-9a-f]{64}$")
PNG_SIGNATURE = b"\x89PNG\r\n\x1a\n"


def read_json(path: Path) -> dict:
    return json.loads(path.read_text(encoding="utf-8"))


def sha256(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def decode_png(raw: bytes) -> tuple[int, int, int, bytes]:
    """Decode the 8-bit non-interlaced RGB/RGBA PNGs used by the app."""
    if raw[:8] != PNG_SIGNATURE:
        raise ValueError("not a PNG")
    offset = 8
    width = height = bit_depth = color_type = interlace = 0
    idat: list[bytes] = []
    while offset < len(raw):
        length = struct.unpack(">I", raw[offset : offset + 4])[0]
        kind = raw[offset + 4 : offset + 8]
        payload = raw[offset + 8 : offset + 8 + length]
        offset += 12 + length
        if kind == b"IHDR":
            width, height, bit_depth, color_type, _, _, interlace = struct.unpack(
                ">IIBBBBB", payload
            )
        elif kind == b"IDAT":
            idat.append(payload)
        elif kind == b"IEND":
            break
    if bit_depth != 8 or interlace != 0 or color_type not in (2, 6):
        raise ValueError(
            f"unsupported PNG (depth {bit_depth}, color {color_type}, interlace {interlace})"
        )
    channels = 4 if color_type == 6 else 3
    stride = width * channels
    filtered = zlib.decompress(b"".join(idat))
    pixels = bytearray(stride * height)
    position = 0
    for y in range(height):
        filter_type = filtered[position]
        position += 1
        row = y * stride
        previous = row - stride
        for x in range(stride):
            value = filtered[position]
            position += 1
            left = pixels[row + x - channels] if x >= channels else 0
            up = pixels[previous + x] if y > 0 else 0
            up_left = (
                pixels[previous + x - channels]
                if y > 0 and x >= channels
                else 0
            )
            if filter_type == 0:
                predict = 0
            elif filter_type == 1:
                predict = left
            elif filter_type == 2:
                predict = up
            elif filter_type == 3:
                predict = (left + up) >> 1
            elif filter_type == 4:
                p = left + up - up_left
                pa, pb, pc = abs(p - left), abs(p - up), abs(p - up_left)
                predict = left if pa <= pb and pa <= pc else up if pb <= pc else up_left
            else:
                raise ValueError(f"unsupported PNG filter {filter_type}")
            pixels[row + x] = (value + predict) & 0xFF
    return width, height, channels, bytes(pixels)


def image_statistics(raw: bytes) -> dict:
    width, height, channels, pixels = decode_png(raw)
    colors = set()
    luminance_sum = 0.0
    luminance_squares = 0.0
    opaque = 0
    samples = 0
    step = max(1, (width * height) // 40_000)
    for index in range(0, width * height, step):
        base = index * channels
        r, g, b = pixels[base], pixels[base + 1], pixels[base + 2]
        alpha = pixels[base + 3] if channels == 4 else 255
        if alpha < 16:
            continue
        opaque += 1
        samples += 1
        colors.add((r >> 4, g >> 4, b >> 4))
        luminance = 0.2126 * r + 0.7152 * g + 0.0722 * b
        luminance_sum += luminance
        luminance_squares += luminance * luminance
    mean = luminance_sum / samples if samples else 0.0
    variance = max(0.0, luminance_squares / samples - mean * mean) if samples else 0.0
    return {
        "width": width,
        "height": height,
        "opaqueRatio": opaque / max(1, (width * height) // step),
        "uniqueColors": len(colors),
        "meanLuminance": mean,
        "standardDeviation": variance**0.5,
    }


class DemoAssetsTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.evidence = read_json(DEMO / "evidence.json")
        cls.probe = read_json(DEMO / "mcp-probe.json")
        cls.manifest = read_json(PROJECT)

    def test_evidence_records_a_real_isolated_run(self):
        evidence = self.evidence
        self.assertEqual(evidence["schemaVersion"], 1)
        self.assertEqual(evidence["milestone"], "M11")
        self.assertEqual(evidence["task"], "demo-project-screenshots-mcp-setup")
        self.assertIs(evidence["pass"], True)
        self.assertEqual(evidence["environment"]["platform"], "Windows")
        self.assertEqual(evidence["environment"]["appVersion"], "0.1.0")
        self.assertTrue(evidence["environment"]["isolatedAppData"])
        self.assertEqual(evidence["consoleErrors"], [])
        self.assertTrue(evidence["assets"]["mojangAssetsBundled"] is False)
        self.assertEqual(evidence["assets"]["savedSourceKind"], "minecraft_root")
        self.assertIs(evidence["assets"]["savedSourceValid"], True)
        self.assertIn("12 models", evidence["assets"]["label"])

    def test_demo_project_is_a_checksum_valid_furybuilder_project(self):
        self.assertEqual(self.manifest["projectSchemaVersion"], 1)
        self.assertEqual(self.manifest["name"], "FuryBuilder Demo")
        self.assertEqual(self.manifest["creationMode"], "empty")
        self.assertEqual(self.manifest["targetMinecraftVersion"], "1.21.4")
        self.assertEqual(self.manifest["targetDataVersion"], 4189)
        self.assertEqual(self.manifest["worldRevision"], 1)
        self.assertEqual(self.manifest["lastSaveKind"], "manual")
        self.assertEqual(self.manifest["editorState"]["loadRadius"], 1)
        self.assertEqual(self.manifest["source"]["kind"], "generated")
        self.assertIs(self.manifest["source"]["immutable"], True)
        self.assertEqual(self.manifest["createdBy"], "FuryBuilder")

        world_file = EXAMPLES / self.manifest["worldFile"]
        self.assertTrue(world_file.is_file(), world_file)
        self.assertTrue(self.manifest["worldFile"].startswith("data/"))
        self.assertEqual(world_file.stat().st_size, self.manifest["worldBytes"])
        self.assertEqual(sha256(world_file), self.manifest["worldChecksum"])
        raw = world_file.read_bytes()
        self.assertEqual(raw[:8], b"FURYSNAP")
        snapshot_source = (ROOT / "crates" / "project-store" / "src" / "snapshot.rs").read_text(
            encoding="utf-8"
        )
        format_version = int(
            re.search(
                r"pub const SNAPSHOT_FORMAT_VERSION: u16 = (\d+);", snapshot_source
            ).group(1)
        )
        self.assertEqual(struct.unpack("<H", raw[8:10])[0], format_version)
        self.assertEqual(struct.unpack("<Q", raw[10:18])[0], 1)
        self.assertEqual(
            sha256(PROJECT),
            self.evidence["project"]["manifestSha256"],
        )
        self.assertEqual(
            self.manifest["worldChecksum"],
            self.evidence["project"]["worldChecksum"],
        )

        summary = read_json(EXAMPLES / "source" / "source.json")
        self.assertEqual(summary["files"], [])

    def test_demo_project_contains_no_minecraft_game_files(self):
        forbidden = {".jar", ".mca", ".mcr", ".dat", ".png", ".jpg", ".jpeg", ".ogg", ".zip"}
        files = [path for path in EXAMPLES.rglob("*") if path.is_file()]
        self.assertTrue(files)
        for path in files:
            with self.subTest(file=path.relative_to(EXAMPLES)):
                self.assertNotIn(path.suffix.lower(), forbidden)
                self.assertNotIn(
                    path.name,
                    {"level.dat", "client.jar", "server.jar", "icon.png"},
                )

    def test_every_published_screenshot_exists_and_matches_its_digest(self):
        screenshots = self.evidence["screenshots"]
        self.assertEqual(
            set(screenshots),
            {"editor", "viewport", "mcpAccess", "observationTopDown"},
        )
        for key, record in screenshots.items():
            with self.subTest(screenshot=key):
                path = DEMO / record["file"]
                self.assertTrue(path.is_file(), path)
                raw = path.read_bytes()
                self.assertEqual(raw[:8], PNG_SIGNATURE)
                self.assertEqual(raw[12:16], b"IHDR")
                width, height = struct.unpack(">II", raw[16:24])
                self.assertEqual((width, height), (record["width"], record["height"]))
                self.assertEqual(len(raw), record["bytes"])
                self.assertEqual(hashlib.sha256(raw).hexdigest(), record["sha256"])
                self.assertRegex(record["sha256"], SHA256)
                stats = image_statistics(raw)
                self.assertEqual(stats["width"], width)
                self.assertEqual(stats["height"], height)
                self.assertGreater(stats["opaqueRatio"], 0.95)
                self.assertGreaterEqual(stats["uniqueColors"], 4)
                self.assertGreater(stats["standardDeviation"], 8)

    def test_screenshot_dimensions_match_their_purpose(self):
        screenshots = self.evidence["screenshots"]
        self.assertGreaterEqual(screenshots["editor"]["width"], 1600)
        self.assertGreaterEqual(screenshots["editor"]["height"], 900)
        self.assertGreaterEqual(screenshots["mcpAccess"]["width"], 1600)
        self.assertGreaterEqual(screenshots["viewport"]["width"], 1000)
        self.assertGreaterEqual(screenshots["viewport"]["height"], 700)
        self.assertEqual(screenshots["observationTopDown"]["width"], 48)
        self.assertEqual(screenshots["observationTopDown"]["height"], 48)

    def test_viewport_metadata_is_complete_and_honest(self):
        viewport = self.evidence["screenshots"]["viewport"]
        metadata = viewport["metadata"]
        self.assertEqual(viewport["kind"], "webgl_viewport")
        self.assertEqual(metadata["worldRevision"], 1)
        self.assertEqual(metadata["dimension"], "minecraft:overworld")
        self.assertEqual(metadata["renderer"], "three-webgl2")
        self.assertEqual(metadata["textureSource"], "minecraft_assets")
        self.assertIs(metadata["partial"], False)
        self.assertEqual(metadata["missingChunks"], [])
        self.assertEqual(len(metadata["requestedChunks"]), 9)
        self.assertEqual(len(metadata["loadedChunks"]), 9)
        states = {entry["state"] for entry in metadata["legend"]}
        for required in (
            "minecraft:grass_block",
            "minecraft:oak_planks",
            "minecraft:water",
            "minecraft:glass",
            "minecraft:oak_stairs",
        ):
            self.assertIn(required, states)
        notes = " ".join(metadata["notes"])
        self.assertIn("locally configured Minecraft asset bundle", notes)
        self.assertIn("not embedded in FuryBuilder", notes)
        self.assertNotIn("original FuryBuilder placeholders", notes)

    def test_observation_map_is_deterministic_and_bounded(self):
        observation = self.evidence["screenshots"]["observationTopDown"]
        self.assertEqual(observation["kind"], "deterministic_observation")
        self.assertEqual(observation["mode"], "top_down")
        self.assertEqual(observation["worldRevision"], 1)
        self.assertIs(observation["partial"], False)
        self.assertEqual(observation["warnings"], [])
        self.assertGreaterEqual(observation["legendEntries"], 4)
        self.assertEqual(
            observation["bounds"]["min"],
            {"x": -16, "y": 60, "z": -16},
        )
        self.assertEqual(
            observation["bounds"]["max"],
            {"x": 31, "y": 72, "z": 31},
        )

    def test_mcp_probe_used_the_real_bridge_read_only(self):
        probe = self.probe
        self.assertEqual(probe["result"], "passed")
        self.assertEqual(probe["protocolEra"], "modern")
        self.assertEqual(probe["worldRevision"], 1)
        self.assertEqual(probe["observations"]["block"]["state"], "minecraft:grass_block")
        self.assertEqual(probe["observations"]["block"]["worldRevision"], 1)
        region = probe["observations"]["region"]
        self.assertEqual(region["blocks"], 27)
        self.assertEqual(region["paletteTotal"], 27)
        self.assertEqual(region["nonAirBlocks"], 18)
        stale = probe["observations"]["staleRejection"]
        self.assertIs(stale["isError"], True)
        self.assertEqual(stale["code"], "revision_mismatch")
        capture = probe["observations"]["capture"]
        self.assertTrue(capture["imageMatchesMetadata"])
        self.assertRegex(capture["sha256"], SHA256)
        self.assertEqual(capture["metadata"]["worldRevision"], 1)
        self.assertIs(capture["metadata"]["partial"], False)
        self.assertIs(capture["verification"]["stale"], False)
        self.assertEqual(capture["verification"]["authoritativeRevision"], 1)

    def test_demo_documentation_explains_how_to_open_and_limits(self):
        readme = (DEMO / "README.md").read_text(encoding="utf-8")
        for expected in (
            "../mcp-setup.md",
            "../../examples/furybuilder-demo.furybuilder",
            "screenshots/editor.png",
            "screenshots/viewport.png",
            "screenshots/mcp-access.png",
            "screenshots/observation-top-down.png",
            "scripts/demo-webview2.mjs",
            "%APPDATA%\\com.furybuilder.desktop\\projects",
            "not a Minecraft world",
            "Mojang",
            "placeholder",
        ):
            self.assertIn(expected, readme)
        self.assertIn(
            "does not include Minecraft",
            " ".join(readme.split()),
        )

        mcp = (ROOT / "docs" / "mcp-setup.md").read_text(encoding="utf-8")
        self.assertIn("demo/README.md", mcp)
        self.assertIn("furybuilder-demo.furybuilder", mcp)
        self.assertIn("cargo test -p furybuilder-mcp-bridge", mcp)

        root_readme = (ROOT / "README.md").read_text(encoding="utf-8")
        self.assertIn("docs/demo/README.md", root_readme)
        self.assertIn("docs/demo/screenshots/editor.png", root_readme)

        matrix = (ROOT / "docs" / "testing" / "test-matrix.md").read_text(
            encoding="utf-8"
        )
        self.assertIn("M11.4", matrix)
        self.assertIn("docs/demo/evidence.json", matrix)

    def test_demo_documentation_relative_links_and_images_resolve(self):
        readme = (DEMO / "README.md").read_text(encoding="utf-8")
        targets = re.findall(r"\]\(([^)]+)\)", readme)
        targets += re.findall(r'<img[^>]+src="([^"]+)"', readme)
        self.assertTrue(targets, "the demo README must link its evidence")
        for target in targets:
            if target.startswith(("http://", "https://", "#")):
                continue
            path = (DEMO / target.split("#", 1)[0]).resolve()
            with self.subTest(link=target):
                self.assertTrue(path.exists(), target)
        self.assertTrue((DEMO / "screenshots" / "editor.png").is_file())
        self.assertTrue(EXAMPLES.is_dir())
        root_readme = (ROOT / "README.md").read_text(encoding="utf-8")
        self.assertTrue(
            (ROOT / "docs" / "demo" / "screenshots" / "editor.png").is_file()
        )
        self.assertIn("docs/demo/screenshots/editor.png", root_readme)

    def test_evidence_contains_no_token_or_absolute_user_path(self):
        combined = "\n".join(
            path.read_text(encoding="utf-8")
            for path in (
                DEMO / "evidence.json",
                DEMO / "mcp-probe.json",
                DEMO / "README.md",
            )
        )
        self.assertNotIn("token", combined.lower())
        self.assertNotIn("\\\\.\\pipe", combined)
        self.assertNotIn("C:\\Users\\", combined)
        self.assertNotIn("D:\\mitrium", combined)
        self.assertNotIn("BEGIN PRIVATE KEY", combined)


if __name__ == "__main__":
    unittest.main()
