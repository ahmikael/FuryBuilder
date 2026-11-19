//! Tests for the block-edit overlay over an imported classic Java world (M8.5).

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use flate2::write::ZlibEncoder;
use flate2::Compression;
use serde_json::Value;
use world_domain::{BiomeState, BlockState};
use world_io::nbt::{self, NbtFile, NbtValue, TAG_COMPOUND, TAG_STRING};
use world_io::overlay::{ChunkKey, WorldOverlay};
use world_io::{JavaWorld, RegionFile};

const TARGET: i32 = 4189;
const OVERWORLD: &str = "minecraft:overworld";

struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new(label: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "furybuilder-overlay-{label}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|duration| duration.as_nanos())
                .unwrap_or(0)
        ));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).expect("create temp directory");
        Self { path }
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

fn level_dat_bytes() -> Vec<u8> {
    let file = NbtFile::new(
        "",
        NbtValue::compound([(
            "Data",
            NbtValue::compound([
                ("DataVersion", NbtValue::Int(TARGET)),
                (
                    "Version",
                    NbtValue::compound([
                        ("Id", NbtValue::Int(TARGET)),
                        ("Name", NbtValue::String("1.21.4".to_owned())),
                    ]),
                ),
                ("LevelName", NbtValue::String("Overlay test".to_owned())),
            ]),
        )]),
    );
    nbt::write_gzip(&file).expect("encode level.dat")
}

fn block_palette(states: &[(&str, &[(&str, &str)])]) -> NbtValue {
    let items = states
        .iter()
        .map(|(name, properties)| {
            let mut compound = BTreeMap::new();
            compound.insert("Name".to_owned(), NbtValue::String((*name).to_owned()));
            if !properties.is_empty() {
                compound.insert(
                    "Properties".to_owned(),
                    NbtValue::Compound(
                        properties
                            .iter()
                            .map(|(key, value)| {
                                ((*key).to_owned(), NbtValue::String((*value).to_owned()))
                            })
                            .collect(),
                    ),
                );
            }
            NbtValue::Compound(compound)
        })
        .collect();
    NbtValue::compound([(
        "palette",
        NbtValue::List {
            kind: TAG_COMPOUND,
            items,
        },
    )])
}

fn biome_palette(biome: &str) -> NbtValue {
    NbtValue::compound([(
        "palette",
        NbtValue::List {
            kind: TAG_STRING,
            items: vec![NbtValue::String(biome.to_owned())],
        },
    )])
}

fn section(
    y: i8,
    name: &str,
    biome: &str,
    properties: &[(&str, &str)],
    unknown: Option<(&str, NbtValue)>,
) -> NbtValue {
    let mut entries = vec![
        ("Y", NbtValue::Byte(y)),
        ("block_states", block_palette(&[(name, properties)])),
        ("biomes", biome_palette(biome)),
    ];
    if let Some((key, value)) = unknown {
        entries.push((key, value));
    }
    NbtValue::compound(entries)
}

fn chunk_nbt(x: i32, z: i32, sections: Vec<NbtValue>, with_unknown: bool) -> NbtFile {
    let mut entries = vec![
        ("DataVersion", NbtValue::Int(TARGET)),
        ("xPos", NbtValue::Int(x)),
        ("zPos", NbtValue::Int(z)),
        ("yPos", NbtValue::Int(-4)),
        ("Status", NbtValue::String("minecraft:full".to_owned())),
        (
            "sections",
            NbtValue::List {
                kind: TAG_COMPOUND,
                items: sections,
            },
        ),
        (
            "block_entities",
            NbtValue::List {
                kind: TAG_COMPOUND,
                items: vec![NbtValue::compound([(
                    "id",
                    NbtValue::String("minecraft:chest".to_owned()),
                )])],
            },
        ),
        (
            "entities",
            NbtValue::List {
                kind: TAG_COMPOUND,
                items: vec![NbtValue::compound([(
                    "id",
                    NbtValue::String("minecraft:item".to_owned()),
                )])],
            },
        ),
    ];
    if with_unknown {
        entries.push((
            "UnknownChunkTag",
            NbtValue::compound([
                ("text", NbtValue::String("preserve me".to_owned())),
                ("number", NbtValue::Long(42)),
            ]),
        ));
        entries.push((
            "CustomList",
            NbtValue::List {
                kind: TAG_COMPOUND,
                items: vec![NbtValue::compound([(
                    "k",
                    NbtValue::String("v".to_owned()),
                )])],
            },
        ));
    }
    NbtFile::new("", NbtValue::compound(entries))
}

fn zlib(bytes: &[u8]) -> Vec<u8> {
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(bytes).expect("zlib write");
    encoder.finish().expect("zlib finish")
}

fn region_bytes(chunks: &[(i32, i32, NbtFile)]) -> Vec<u8> {
    let mut records = Vec::new();
    for (_, _, file) in chunks {
        let raw = nbt::write_uncompressed(file).expect("encode chunk");
        let payload = zlib(&raw);
        let record_len = 5 + payload.len();
        records.push((payload, record_len.div_ceil(4096)));
    }
    let total_sectors = 2 + records.iter().map(|(_, sectors)| *sectors).sum::<usize>();
    let mut file = vec![0u8; total_sectors * 4096];
    let mut offset = 2usize;
    for ((x, z, _), (payload, sectors)) in chunks.iter().zip(records.iter()) {
        let index = x.rem_euclid(32) + z.rem_euclid(32) * 32;
        let base = index as usize * 4;
        file[base] = ((offset >> 16) & 0xff) as u8;
        file[base + 1] = ((offset >> 8) & 0xff) as u8;
        file[base + 2] = (offset & 0xff) as u8;
        file[base + 3] = *sectors as u8;
        let timestamp = 1_700_000_000i32.to_be_bytes();
        file[4096 + base..4096 + base + 4].copy_from_slice(&timestamp);
        let start = offset * 4096;
        file[start..start + 4].copy_from_slice(&((payload.len() as u32 + 1).to_be_bytes()));
        file[start + 4] = 2;
        file[start + 5..start + 5 + payload.len()].copy_from_slice(payload);
        offset += sectors;
    }
    file
}

fn write_world(root: &Path) {
    fs::create_dir_all(root.join("region")).expect("create region");
    fs::write(root.join("level.dat"), level_dat_bytes()).expect("write level.dat");
    fs::write(root.join("session.lock"), b"1234567890123456").expect("write session lock");

    let player_chunk = chunk_nbt(
        0,
        0,
        vec![
            section(
                -4,
                "minecraft:stone",
                "minecraft:plains",
                &[],
                Some(("UnknownSectionTag", NbtValue::Long(7))),
            ),
            section(-3, "minecraft:sand", "minecraft:forest", &[], None),
        ],
        true,
    );
    let neighbor_chunk = chunk_nbt(
        1,
        0,
        vec![section(
            -4,
            "minecraft:stone",
            "minecraft:plains",
            &[],
            None,
        )],
        false,
    );
    fs::write(
        root.join("region").join("r.0.0.mca"),
        region_bytes(&[(0, 0, player_chunk), (1, 0, neighbor_chunk)]),
    )
    .expect("write region");
    fs::write(root.join("custom-unknown.dat"), b"do not touch").expect("write unknown file");
}

fn snapshot(root: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    let mut files = BTreeMap::new();
    let mut stack = vec![PathBuf::new()];
    while let Some(relative) = stack.pop() {
        let directory = root.join(&relative);
        for entry in fs::read_dir(&directory).expect("read directory") {
            let entry = entry.expect("entry");
            let entry_relative = relative.join(entry.file_name());
            let metadata = fs::symlink_metadata(entry.path()).expect("metadata");
            if metadata.is_dir() {
                stack.push(entry_relative);
            } else {
                files.insert(entry_relative, fs::read(entry.path()).expect("read file"));
            }
        }
    }
    files
}

fn state(value: &str) -> BlockState {
    value.parse().expect("valid state")
}

fn source_chunk(world: &JavaWorld, x: i32, z: i32) -> NbtFile {
    world
        .read_region(OVERWORLD, 0, 0)
        .expect("region")
        .read_chunk(x, z)
        .expect("chunk")
        .nbt
}

fn root_map(file: &NbtFile) -> &BTreeMap<String, NbtValue> {
    file.root.as_compound().expect("compound root")
}

fn section_map(file: &NbtFile) -> BTreeMap<i8, &BTreeMap<String, NbtValue>> {
    let (_, items) = root_map(file)
        .get("sections")
        .and_then(NbtValue::as_list)
        .expect("sections list");
    items
        .iter()
        .map(|item| {
            let compound = item.as_compound().expect("section compound");
            let y = match compound.get("Y") {
                Some(NbtValue::Byte(value)) => *value,
                _ => panic!("section Y"),
            };
            (y, compound)
        })
        .collect()
}

#[test]
fn edits_blocks_without_touching_source_bytes() {
    let dir = TempDir::new("blocks");
    let source = dir.path().join("world");
    write_world(&source);
    let before = snapshot(&source);

    let mut overlay = WorldOverlay::open(&source).expect("open overlay");
    assert_eq!(overlay.change_count(), 0);
    assert_eq!(
        overlay
            .get_block(OVERWORLD, 1, -64, 2)
            .expect("read source block"),
        state("minecraft:stone")
    );

    let change = overlay
        .set_block(OVERWORLD, 1, -64, 2, &state("minecraft:dirt"))
        .expect("set block");
    assert_eq!(change.previous, state("minecraft:stone"));
    assert_eq!(change.current, state("minecraft:dirt"));
    assert!(change.changed);
    assert_eq!(
        overlay
            .get_block(OVERWORLD, 1, -64, 2)
            .expect("read overlay block"),
        state("minecraft:dirt")
    );
    assert_eq!(overlay.change_count(), 1);
    assert_eq!(overlay.dirty_section_count(), 1);
    assert_eq!(
        overlay.dirty_chunks().cloned().collect::<Vec<_>>(),
        vec![ChunkKey::new(OVERWORLD, 0, 0)]
    );

    let merged = overlay.read_chunk(OVERWORLD, 0, 0).expect("merged chunk");
    assert!(merged.overridden);
    assert_eq!(merged.changes, 1);
    assert_eq!(merged.dirty_sections, BTreeSet::from([-4]));
    assert_eq!(
        merged.chunk.block_at(1, -64, 2),
        Some(&state("minecraft:dirt"))
    );
    assert_eq!(merged.chunk.biome_at(1, -64, 2), Some("minecraft:plains"));
    assert_eq!(merged.chunk.block_entities, 1);
    assert_eq!(merged.chunk.entities, 1);

    assert_eq!(
        snapshot(&source),
        before,
        "source world must stay unchanged"
    );

    let revert = overlay
        .set_block(OVERWORLD, 1, -64, 2, &state("minecraft:stone"))
        .expect("revert block");
    assert!(revert.changed);
    assert_eq!(overlay.change_count(), 0);
    assert!(!overlay.is_dirty());
    assert_eq!(overlay.dirty_chunks().count(), 0);
    assert!(
        !overlay
            .read_chunk(OVERWORLD, 0, 0)
            .expect("chunk")
            .overridden
    );
    assert_eq!(snapshot(&source), before);
}

#[test]
fn materializes_patched_chunk_and_preserves_unknown_fields() {
    let dir = TempDir::new("materialize");
    let source = dir.path().join("world");
    write_world(&source);
    let before = snapshot(&source);

    let mut overlay = WorldOverlay::open(&source).expect("open overlay");
    overlay
        .set_block(OVERWORLD, 1, -64, 2, &state("minecraft:dirt"))
        .expect("first edit");
    overlay
        .set_block(
            OVERWORLD,
            2,
            -48,
            3,
            &state("minecraft:oak_stairs[facing=east,half=top]"),
        )
        .expect("second edit");

    let patched = overlay
        .materialize_chunk_nbt(OVERWORLD, 0, 0)
        .expect("materialize")
        .expect("dirty chunk");
    assert_eq!(
        overlay
            .materialize_chunk_nbt(OVERWORLD, 1, 0)
            .expect("clean chunk"),
        None
    );

    let decoded = overlay
        .world()
        .decode_chunk_nbt(OVERWORLD, 0, 0, &patched)
        .expect("re-decode patched NBT");
    assert_eq!(decoded.block_at(1, -64, 2), Some(&state("minecraft:dirt")));
    assert_eq!(
        decoded.block_at(2, -48, 3),
        Some(&state("minecraft:oak_stairs[facing=east,half=top]"))
    );
    assert_eq!(decoded.block_entities, 1);
    assert_eq!(decoded.entities, 1);

    let original = source_chunk(overlay.world(), 0, 0);
    let original_root = root_map(&original);
    let patched_root = root_map(&patched);
    assert_eq!(
        original_root.keys().collect::<Vec<_>>(),
        patched_root.keys().collect::<Vec<_>>()
    );
    for (key, value) in original_root {
        if key != "sections" {
            assert_eq!(patched_root.get(key), Some(value), "root tag `{key}`");
        }
    }
    let original_sections = section_map(&original);
    let patched_sections = section_map(&patched);
    assert_eq!(
        original_sections.keys().collect::<Vec<_>>(),
        patched_sections.keys().collect::<Vec<_>>()
    );
    for (y, original_section) in original_sections {
        let patched_section = patched_sections.get(&y).expect("section");
        for (key, value) in original_section {
            if key != "block_states" {
                assert_eq!(
                    patched_section.get(key),
                    Some(value),
                    "section {y} tag `{key}`"
                );
            }
        }
    }

    let raw = nbt::write_uncompressed(&patched).expect("encode patched NBT");
    let reparsed = nbt::read_uncompressed(&raw).expect("decode patched NBT");
    assert_eq!(reparsed, patched);
    assert_eq!(snapshot(&source), before);
}

#[test]
fn rejects_invalid_positions_missing_chunks_and_missing_sections_atomically() {
    let dir = TempDir::new("rejects");
    let source = dir.path().join("world");
    write_world(&source);
    let before = snapshot(&source);
    let mut overlay = WorldOverlay::open(&source).expect("open overlay");

    let unknown_dimension = overlay
        .set_block("minecraft:custom", 0, 0, 0, &state("minecraft:dirt"))
        .expect_err("unknown dimension");
    assert_eq!(unknown_dimension.code(), "invalid_argument");

    let above = overlay
        .set_block(OVERWORLD, 0, 320, 0, &state("minecraft:dirt"))
        .expect_err("above world");
    assert_eq!(above.code(), "invalid_argument");
    let below = overlay
        .set_block(OVERWORLD, 0, -65, 0, &state("minecraft:dirt"))
        .expect_err("below world");
    assert_eq!(below.code(), "invalid_argument");

    let missing_chunk = overlay
        .set_block(OVERWORLD, 100, 0, 100, &state("minecraft:dirt"))
        .expect_err("missing chunk");
    assert_eq!(missing_chunk.code(), "not_found");

    let missing_section = overlay
        .set_block(OVERWORLD, 0, 0, 0, &state("minecraft:dirt"))
        .expect_err("missing section");
    assert_eq!(missing_section.code(), "unsupported_format");
    assert_eq!(overlay.change_count(), 0);

    let edits = vec![
        world_io::overlay::BlockEdit::new(OVERWORLD.to_owned(), 0, -64, 0, state("minecraft:dirt")),
        world_io::overlay::BlockEdit::new(
            OVERWORLD.to_owned(),
            100,
            -64,
            100,
            state("minecraft:dirt"),
        ),
    ];
    let batch = overlay.apply(&edits).expect_err("batch must fail");
    assert_eq!(batch.code(), "not_found");
    assert_eq!(overlay.change_count(), 0, "failed batch must be atomic");

    let duplicate = overlay
        .apply(&[
            world_io::overlay::BlockEdit::new(
                OVERWORLD.to_owned(),
                0,
                -64,
                0,
                state("minecraft:dirt"),
            ),
            world_io::overlay::BlockEdit::new(
                OVERWORLD.to_owned(),
                0,
                -64,
                0,
                state("minecraft:sand"),
            ),
        ])
        .expect_err("duplicate positions");
    assert_eq!(duplicate.code(), "invalid_argument");
    assert_eq!(overlay.change_count(), 0);
    assert_eq!(snapshot(&source), before);
}

#[test]
fn document_round_trips_and_rejects_tampering() {
    let dir = TempDir::new("document");
    let source = dir.path().join("world");
    write_world(&source);
    let mut overlay = WorldOverlay::open(&source).expect("open overlay");
    overlay
        .set_block(OVERWORLD, 1, -64, 2, &state("minecraft:dirt"))
        .expect("first edit");
    overlay
        .set_block(
            OVERWORLD,
            2,
            -48,
            3,
            &state("minecraft:oak_stairs[facing=east,half=top]"),
        )
        .expect("second edit");

    let json = overlay.to_json_pretty().expect("serialize overlay");
    assert!(json.contains("\"schemaVersion\": 2"));
    assert!(json.contains("\"dataVersion\": 4189"));
    assert!(json.contains("\"bitsPerIndex\""));
    let loaded = WorldOverlay::from_json(&source, &json).expect("load overlay");
    assert_eq!(loaded.change_count(), overlay.change_count());
    assert_eq!(loaded.dirty_section_count(), overlay.dirty_section_count());
    assert_eq!(
        loaded.get_block(OVERWORLD, 1, -64, 2).expect("block"),
        state("minecraft:dirt")
    );
    assert_eq!(
        loaded.get_block(OVERWORLD, 2, -48, 3).expect("stairs"),
        state("minecraft:oak_stairs[facing=east,half=top]")
    );

    let mut legacy: Value = serde_json::from_str(&json).expect("JSON value");
    legacy["schemaVersion"] = Value::from(1u64);
    for chunk in legacy["chunks"].as_array_mut().expect("chunks") {
        chunk
            .as_object_mut()
            .expect("chunk object")
            .remove("biomes");
    }
    let legacy =
        WorldOverlay::from_json(&source, &legacy.to_string()).expect("schema 1 remains readable");
    assert_eq!(legacy.change_count(), overlay.change_count());
    assert_eq!(
        legacy
            .get_block(OVERWORLD, 1, -64, 2)
            .expect("legacy block"),
        state("minecraft:dirt")
    );
    assert_eq!(
        legacy
            .get_biome(OVERWORLD, 1, -64, 2)
            .expect("legacy biome")
            .as_str(),
        "minecraft:plains"
    );

    let mut value: Value = serde_json::from_str(&json).expect("JSON value");
    value["schemaVersion"] = Value::from(1u64);
    value["chunks"][0]["biomes"] = serde_json::json!([{
        "y": -4,
        "palette": ["minecraft:desert"],
        "cells": vec![0u16; 64],
    }]);
    assert!(
        WorldOverlay::from_json(&source, &value.to_string()).is_err(),
        "a schema 1 document cannot carry biome edits"
    );

    let mut value: Value = serde_json::from_str(&json).expect("JSON value");
    value["schemaVersion"] = Value::from(3u64);
    assert_eq!(
        WorldOverlay::from_json(&source, &value.to_string())
            .expect_err("schema")
            .code(),
        "unsupported_version"
    );

    let mut value: Value = serde_json::from_str(&json).expect("JSON value");
    value["dataVersion"] = Value::from(4188u64);
    assert_eq!(
        WorldOverlay::from_json(&source, &value.to_string())
            .expect_err("version")
            .code(),
        "unsupported_version"
    );

    let mut value: Value = serde_json::from_str(&json).expect("JSON value");
    value["chunks"][0]["sections"][0]["y"] = Value::from(99);
    assert_eq!(
        WorldOverlay::from_json(&source, &value.to_string())
            .expect_err("section")
            .code(),
        "unsupported_format"
    );

    let mut value: Value = serde_json::from_str(&json).expect("JSON value");
    value["chunks"][0]["sections"][0]["palette"][0] = Value::from("not a block");
    assert_eq!(
        WorldOverlay::from_json(&source, &value.to_string())
            .expect_err("palette")
            .code(),
        "unsupported_format"
    );

    let mut value: Value = serde_json::from_str(&json).expect("JSON value");
    value["chunks"][0]["sections"][0]["bitsPerIndex"] = Value::from(0u64);
    assert_eq!(
        WorldOverlay::from_json(&source, &value.to_string())
            .expect_err("bits")
            .code(),
        "unsupported_format"
    );

    let mut value: Value = serde_json::from_str(&json).expect("JSON value");
    if let Some(word) = value["chunks"][0]["sections"][0]["data"]
        .as_array_mut()
        .and_then(|words| words.first_mut())
    {
        *word = Value::from("zzzz");
        assert_eq!(
            WorldOverlay::from_json(&source, &value.to_string())
                .expect_err("hex")
                .code(),
            "unsupported_format"
        );
    }

    let mut value: Value = serde_json::from_str(&json).expect("JSON value");
    let chunk = value["chunks"][0].clone();
    value["chunks"].as_array_mut().expect("chunks").push(chunk);
    assert_eq!(
        WorldOverlay::from_json(&source, &value.to_string())
            .expect_err("duplicate chunk")
            .code(),
        "unsupported_format"
    );

    let mut value: Value = serde_json::from_str(&json).expect("JSON value");
    value["chunks"][0]["x"] = Value::from(100);
    value["chunks"][0]["z"] = Value::from(100);
    assert_eq!(
        WorldOverlay::from_json(&source, &value.to_string())
            .expect_err("missing chunk")
            .code(),
        "not_found"
    );
}

#[test]
fn wide_palette_uses_non_spanning_words() {
    let dir = TempDir::new("wide-palette");
    let source = dir.path().join("world");
    write_world(&source);
    let mut overlay = WorldOverlay::open(&source).expect("open overlay");

    let mut expected = Vec::new();
    for index in 0..40i32 {
        let state = state(&format!("minecraft:test_block_{index}"));
        let local_x = index % 16;
        let local_z = index / 16;
        overlay
            .set_block(OVERWORLD, local_x, -64, local_z, &state)
            .expect("set palette block");
        expected.push((local_x as u8, local_z as u8, state));
    }
    let patched = overlay
        .materialize_chunk_nbt(OVERWORLD, 0, 0)
        .expect("materialize")
        .expect("dirty chunk");
    let decoded = overlay
        .world()
        .decode_chunk_nbt(OVERWORLD, 0, 0, &patched)
        .expect("decode");
    for (local_x, local_z, state) in &expected {
        assert_eq!(
            decoded.block_at(*local_x, -64, *local_z),
            Some(state),
            "block {local_x}, {local_z}"
        );
    }

    let sections = section_map(&patched);
    let section = sections.get(&-4).expect("section -4");
    let block_states = section["block_states"].as_compound().expect("block_states");
    let palette = block_states["palette"].as_list().expect("palette").1;
    assert!(palette.len() >= 40);
    let bits = (usize::BITS - (palette.len() - 1).leading_zeros()) as usize;
    let values_per_long = 64 / bits.max(4);
    let expected_words = 4096usize.div_ceil(values_per_long);
    match &block_states["data"] {
        NbtValue::LongArray(words) => assert_eq!(words.len(), expected_words),
        other => panic!("expected LongArray, got {other:?}"),
    }
}

#[test]
fn batch_updates_multiple_chunks_and_supports_clear() {
    let dir = TempDir::new("batch");
    let source = dir.path().join("world");
    write_world(&source);
    let before = snapshot(&source);
    let mut overlay = WorldOverlay::open(&source).expect("open overlay");

    let report = overlay
        .apply(&[
            world_io::overlay::BlockEdit::new(
                OVERWORLD.to_owned(),
                0,
                -64,
                0,
                state("minecraft:dirt"),
            ),
            world_io::overlay::BlockEdit::new(
                OVERWORLD.to_owned(),
                1,
                -64,
                1,
                state("minecraft:sand"),
            ),
            world_io::overlay::BlockEdit::new(
                OVERWORLD.to_owned(),
                16,
                -64,
                0,
                state("minecraft:dirt"),
            ),
        ])
        .expect("batch");
    assert_eq!(report.requested, 3);
    assert_eq!(report.changed, 3);
    assert_eq!(report.chunks, 2);
    assert_eq!(report.sections, 2);
    assert_eq!(overlay.change_count(), 3);
    assert_eq!(overlay.dirty_section_count(), 2);

    assert!(overlay.clear_chunk(OVERWORLD, 1, 0));
    assert_eq!(overlay.change_count(), 2);
    assert_eq!(overlay.dirty_chunks().count(), 1);
    assert!(!overlay.clear_chunk(OVERWORLD, 1, 0));

    overlay.clear();
    assert_eq!(overlay.change_count(), 0);
    assert_eq!(overlay.dirty_chunks().count(), 0);
    assert_eq!(
        overlay
            .get_block(OVERWORLD, 1, -64, 2)
            .expect("source block"),
        state("minecraft:stone")
    );
    assert_eq!(snapshot(&source), before);
}

#[test]
fn empty_batch_and_clearing_missing_chunks_are_no_ops() {
    let dir = TempDir::new("empty");
    let source = dir.path().join("world");
    write_world(&source);
    let mut overlay = WorldOverlay::open(&source).expect("open overlay");

    let report = overlay.apply(&[]).expect("empty batch");
    assert_eq!(report.requested, 0);
    assert_eq!(report.changed, 0);
    assert_eq!(report.chunks, 0);
    assert!(!overlay.clear_chunk(OVERWORLD, 5, 5));
    assert_eq!(overlay.change_count(), 0);
}

#[test]
fn region_helper_still_opens_the_fixture() {
    let dir = TempDir::new("region-helper");
    let source = dir.path().join("world");
    write_world(&source);
    let region = RegionFile::open(source.join("region").join("r.0.0.mca")).expect("region");
    assert_eq!(region.entries().len(), 2);
}

fn biome(name: &str) -> BiomeState {
    name.parse().expect("valid biome")
}

#[test]
fn edits_biome_cells_without_touching_blocks_and_round_trips() {
    let dir = TempDir::new("biomes");
    let source = dir.path().join("world");
    write_world(&source);
    let before = snapshot(&source);
    let mut overlay = WorldOverlay::open(&source).expect("open overlay");

    assert_eq!(
        overlay
            .get_biome(OVERWORLD, 1, -64, 2)
            .expect("source biome")
            .as_str(),
        "minecraft:plains"
    );
    let change = overlay
        .set_biome(OVERWORLD, 1, -64, 2, &biome("minecraft:desert"))
        .expect("biome edit");
    assert_eq!(change.previous, biome("minecraft:plains"));
    assert!(change.changed);
    assert_eq!(
        overlay
            .get_biome(OVERWORLD, 3, -61, 3)
            .expect("same cell")
            .as_str(),
        "minecraft:desert",
        "the whole 4x4x4 cell must change"
    );
    assert_eq!(
        overlay
            .get_biome(OVERWORLD, 4, -64, 2)
            .expect("next cell")
            .as_str(),
        "minecraft:plains",
        "a neighbouring cell must stay untouched"
    );
    assert_eq!(
        overlay.get_block(OVERWORLD, 1, -64, 2).expect("block"),
        state("minecraft:stone"),
        "biome edits must not change block states"
    );

    // A second biome in the same section exercises a multi-entry biome palette.
    overlay
        .set_biome(OVERWORLD, 8, -64, 8, &biome("minecraft:forest"))
        .expect("second biome");
    assert_eq!(overlay.dirty_section_count(), 1);

    let merged = overlay.read_chunk(OVERWORLD, 0, 0).expect("merged chunk");
    assert_eq!(merged.chunk.biome_at(1, -64, 2), Some("minecraft:desert"));
    assert_eq!(merged.chunk.biome_at(8, -64, 8), Some("minecraft:forest"));
    assert_eq!(
        merged.chunk.block_at(1, -64, 2),
        Some(&state("minecraft:stone"))
    );

    let patched = overlay
        .materialize_chunk_nbt(OVERWORLD, 0, 0)
        .expect("materialize")
        .expect("dirty chunk");
    let decoded = overlay
        .world()
        .decode_chunk_nbt(OVERWORLD, 0, 0, &patched)
        .expect("decode patched chunk");
    assert_eq!(decoded.biome_at(1, -64, 2), Some("minecraft:desert"));
    assert_eq!(decoded.biome_at(8, -64, 8), Some("minecraft:forest"));
    assert_eq!(decoded.block_at(1, -64, 2), Some(&state("minecraft:stone")));

    let json = overlay.to_json_pretty().expect("serialize overlay");
    assert!(json.contains("\"biomes\""));
    let loaded = WorldOverlay::from_json(&source, &json).expect("load overlay");
    assert_eq!(
        loaded
            .get_biome(OVERWORLD, 1, -64, 2)
            .expect("loaded biome")
            .as_str(),
        "minecraft:desert"
    );
    assert_eq!(
        loaded.get_block(OVERWORLD, 1, -64, 2).expect("block"),
        state("minecraft:stone")
    );

    overlay.clear();
    assert_eq!(
        overlay
            .get_biome(OVERWORLD, 1, -64, 2)
            .expect("source biome"),
        biome("minecraft:plains")
    );
    assert_eq!(snapshot(&source), before);
}

#[test]
fn reverting_a_block_edit_keeps_biome_edits() {
    let dir = TempDir::new("biome-block-revert");
    let source = dir.path().join("world");
    write_world(&source);
    let mut overlay = WorldOverlay::open(&source).expect("open overlay");

    overlay
        .set_biome(OVERWORLD, 1, -64, 2, &biome("minecraft:desert"))
        .expect("biome edit");
    overlay
        .set_block(OVERWORLD, 1, -64, 2, &state("minecraft:gold_block"))
        .expect("block edit");
    overlay
        .set_block(OVERWORLD, 1, -64, 2, &state("minecraft:stone"))
        .expect("revert block edit");

    assert_eq!(
        overlay.get_block(OVERWORLD, 1, -64, 2).expect("block"),
        state("minecraft:stone"),
        "the block edit must be gone"
    );
    assert_eq!(
        overlay
            .get_biome(OVERWORLD, 1, -64, 2)
            .expect("biome")
            .as_str(),
        "minecraft:desert",
        "the independent biome edit must survive a block revert"
    );
    assert_eq!(overlay.dirty_section_count(), 1);
    assert!(overlay.change_count() > 0);
}

#[test]
fn rejects_duplicate_biome_cells_atomically() {
    let dir = TempDir::new("biome-limits");
    let source = dir.path().join("world");
    write_world(&source);
    let before = snapshot(&source);
    let mut overlay = WorldOverlay::open(&source).expect("open overlay");

    let error = overlay
        .apply_biomes(&[
            world_io::overlay::BiomeEdit::new(
                OVERWORLD.to_owned(),
                0,
                -64,
                0,
                biome("minecraft:desert"),
            ),
            world_io::overlay::BiomeEdit::new(
                OVERWORLD.to_owned(),
                3,
                -61,
                3,
                biome("minecraft:forest"),
            ),
        ])
        .expect_err("duplicate cell");
    assert!(matches!(
        error,
        world_io::overlay::OverlayError::DuplicateBiomeEdit { .. }
    ));
    assert_eq!(overlay.change_count(), 0);
    assert_eq!(snapshot(&source), before);
}
