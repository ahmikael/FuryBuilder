//! Tests for the read-only Anvil adapter. All fixtures are synthetic and in-memory
//! except for the temporary files used to exercise the real file format.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use flate2::write::{GzEncoder, ZlibEncoder};
use flate2::Compression;
use world_domain::DimensionKind;
use world_io::anvil::{
    region_file_name, AnvilError, ChunkCompression, JavaWorld, LevelDat, RegionChunkSummary,
    RegionFile,
};
use world_io::nbt::{self, NbtFile, NbtValue, TAG_COMPOUND, TAG_STRING};

const TARGET: i32 = 4189;

struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new(label: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "furybuilder-anvil-{label}-{}-{}",
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

fn level_dat_bytes(data_version: i32, version_id: i32, name: &str) -> Vec<u8> {
    let file = NbtFile::new(
        "",
        NbtValue::compound([(
            "Data",
            NbtValue::compound([
                ("DataVersion", NbtValue::Int(data_version)),
                (
                    "Version",
                    NbtValue::compound([
                        ("Id", NbtValue::Int(version_id)),
                        ("Name", NbtValue::String(name.to_owned())),
                        ("Snapshot", NbtValue::Byte(0)),
                    ]),
                ),
                ("LevelName", NbtValue::String("Anvil test".to_owned())),
                (
                    "WorldGenSettings",
                    NbtValue::compound([(
                        "dimensions",
                        NbtValue::compound([
                            (
                                "minecraft:overworld",
                                NbtValue::compound([("seed", NbtValue::Long(1))]),
                            ),
                            (
                                "minecraft:the_nether",
                                NbtValue::compound([("seed", NbtValue::Long(1))]),
                            ),
                            (
                                "minecraft:the_end",
                                NbtValue::compound([("seed", NbtValue::Long(1))]),
                            ),
                        ]),
                    )]),
                ),
            ]),
        )]),
    );
    nbt::write_gzip(&file).expect("encode level.dat")
}

fn section(y: i8) -> NbtValue {
    NbtValue::compound([
        ("Y", NbtValue::Byte(y)),
        (
            "block_states",
            NbtValue::compound([(
                "palette",
                NbtValue::List {
                    kind: TAG_COMPOUND,
                    items: vec![NbtValue::compound([(
                        "Name",
                        NbtValue::String("minecraft:air".to_owned()),
                    )])],
                },
            )]),
        ),
        (
            "biomes",
            NbtValue::compound([(
                "palette",
                NbtValue::List {
                    kind: TAG_STRING,
                    items: vec![NbtValue::String("minecraft:plains".to_owned())],
                },
            )]),
        ),
    ])
}

fn chunk_nbt(x: i32, z: i32, data_version: i32, sections: Vec<NbtValue>) -> NbtFile {
    NbtFile::new(
        "",
        NbtValue::compound([
            ("DataVersion", NbtValue::Int(data_version)),
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
            ("block_entities", NbtValue::empty_list(nbt::TAG_END)),
            ("entities", NbtValue::empty_list(nbt::TAG_END)),
        ]),
    )
}

fn gzip(bytes: &[u8]) -> Vec<u8> {
    let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(bytes).expect("gzip write");
    encoder.finish().expect("gzip finish")
}

fn zlib(bytes: &[u8]) -> Vec<u8> {
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(bytes).expect("zlib write");
    encoder.finish().expect("zlib finish")
}

fn chunk_index(x: i32, z: i32) -> usize {
    let local_x = x.rem_euclid(32) as usize;
    let local_z = z.rem_euclid(32) as usize;
    local_x + local_z * 32
}

struct RegionBuilder {
    locations: Vec<u8>,
    timestamps: Vec<u8>,
    body: Vec<u8>,
}

impl RegionBuilder {
    fn new() -> Self {
        Self {
            locations: vec![0; 4096],
            timestamps: vec![0; 4096],
            body: Vec::new(),
        }
    }

    fn set_location(&mut self, index: usize, offset: u32, sectors: u8) {
        let base = index * 4;
        self.locations[base] = (offset >> 16) as u8;
        self.locations[base + 1] = (offset >> 8) as u8;
        self.locations[base + 2] = offset as u8;
        self.locations[base + 3] = sectors;
    }

    fn add(&mut self, index: usize, compression: u8, payload: &[u8]) {
        assert!(index < 1024);
        let length = (payload.len() + 1) as u32;
        let record_len = 5 + payload.len();
        let sectors = (record_len.div_ceil(4096)) as u8;
        assert!(sectors > 0 && sectors < 128, "fixture must stay internal");
        let offset = 2 + (self.body.len() / 4096) as u32;
        self.set_location(index, offset, sectors);
        self.body.extend_from_slice(&length.to_be_bytes());
        self.body.push(compression);
        self.body.extend_from_slice(payload);
        let target = (offset as usize - 2 + sectors as usize) * 4096;
        self.body.resize(target, 0);
    }

    fn build(self) -> Vec<u8> {
        let mut bytes = self.locations;
        bytes.extend_from_slice(&self.timestamps);
        bytes.extend_from_slice(&self.body);
        bytes
    }
}

fn write_region(dir: &Path, region_x: i32, region_z: i32, builder: RegionBuilder) -> PathBuf {
    fs::create_dir_all(dir).expect("create region fixture directory");
    let path = dir.join(region_file_name(region_x, region_z));
    fs::write(&path, builder.build()).expect("write region fixture");
    path
}

fn write_level_dat(world: &Path, data_version: i32, version_id: i32, name: &str) {
    fs::create_dir_all(world).expect("create world folder");
    fs::write(
        world.join("level.dat"),
        level_dat_bytes(data_version, version_id, name),
    )
    .expect("write level.dat");
}

#[test]
fn level_dat_reads_target_version_and_requires_agreeing_versions() {
    let dir = TempDir::new("level");
    let path = dir.path().join("level.dat");
    fs::write(&path, level_dat_bytes(TARGET, TARGET, "1.21.4")).expect("write level.dat");

    let level = LevelDat::read(&path).expect("read level.dat");
    assert_eq!(level.data_version, TARGET);
    assert_eq!(level.version_id, TARGET);
    assert_eq!(level.version_name.as_deref(), Some("1.21.4"));
    assert_eq!(level.level_name.as_deref(), Some("Anvil test"));
    level.ensure_supported().expect("target version");
    assert_eq!(
        level.raw,
        nbt::read_gzip(&fs::read(&path).expect("read bytes")).expect("decode NBT")
    );

    fs::write(&path, level_dat_bytes(TARGET, 4786, "26.1")).expect("write mismatch");
    assert!(matches!(
        LevelDat::read(&path),
        Err(AnvilError::InvalidLevel(message)) if message.contains("does not match")
    ));

    fs::write(&path, level_dat_bytes(4786, 4786, "26.1")).expect("write other version");
    let other = LevelDat::read(&path).expect("inspect other version");
    assert!(matches!(
        other.ensure_supported(),
        Err(AnvilError::UnsupportedDataVersion {
            found: 4786,
            supported
        }) if !supported.contains(&4786)
    ));

    fs::write(&path, b"not a gzip stream").expect("write garbage");
    assert!(matches!(LevelDat::read(&path), Err(AnvilError::Nbt(_))));
}

#[test]
fn region_reads_zlib_chunk_at_negative_coordinates() {
    let dir = TempDir::new("negative");
    let raw = nbt::write_uncompressed(&chunk_nbt(-1, -1, TARGET, vec![section(0)]))
        .expect("encode chunk");
    let mut builder = RegionBuilder::new();
    builder.add(chunk_index(-1, -1), 2, &zlib(&raw));
    let path = write_region(dir.path(), -1, -1, builder);

    let region = RegionFile::open(&path).expect("open region");
    assert_eq!(region.region_coordinates(), (-1, -1));
    assert_eq!(region.entries().len(), 1);
    assert_eq!(region.entries()[0].x, -1);
    assert_eq!(region.entries()[0].z, -1);

    let chunk = region.read_chunk(-1, -1).expect("read chunk");
    assert_eq!(chunk.compression, ChunkCompression::Zlib);
    assert_eq!(chunk.summary.x, -1);
    assert_eq!(chunk.summary.z, -1);
    assert_eq!(chunk.summary.data_version, TARGET);
    assert_eq!(chunk.summary.status.as_deref(), Some("minecraft:full"));
    assert_eq!(chunk.summary.y_pos, Some(-4));
    assert_eq!(chunk.summary.sections, 1);
    assert_eq!(chunk.summary.block_entities, 0);
    assert_eq!(chunk.summary.entities, 0);
    assert_eq!(
        chunk.nbt,
        nbt::read_uncompressed(&raw).expect("decode chunk")
    );
    assert!(chunk.compressed_bytes > 0);
    assert!(chunk.nbt_bytes > 0);

    assert!(matches!(
        region.read_chunk(0, 0),
        Err(AnvilError::ChunkOutsideRegion {
            x: 0,
            z: 0,
            region_x: -1,
            region_z: -1
        })
    ));
}

#[test]
fn region_reads_gzip_and_uncompressed_chunks_then_summarises_all() {
    let dir = TempDir::new("schemes");
    let raw_zero = nbt::write_uncompressed(&chunk_nbt(0, 0, TARGET, vec![])).expect("chunk 0");
    let raw_one =
        nbt::write_uncompressed(&chunk_nbt(1, 0, TARGET, vec![section(-4)])).expect("chunk 1");
    let mut builder = RegionBuilder::new();
    builder.add(chunk_index(0, 0), 1, &gzip(&raw_zero));
    builder.add(chunk_index(1, 0), 3, &raw_one);
    let path = write_region(dir.path(), 0, 0, builder);

    let region = RegionFile::open(&path).expect("open region");
    assert_eq!(region.entries().len(), 2);
    assert_eq!(
        region.read_chunk(0, 0).expect("gzip chunk").compression,
        ChunkCompression::Gzip
    );
    assert_eq!(
        region.read_chunk(1, 0).expect("raw chunk").compression,
        ChunkCompression::Uncompressed
    );
    let summaries = region.summarize_all().expect("summarise region");
    assert_eq!(summaries.len(), 2);
    assert_eq!(summaries[0].sections, 0);
    assert_eq!(summaries[1].sections, 1);
    assert!(matches!(
        region.read_chunk(2, 0),
        Err(AnvilError::ChunkNotPresent { x: 2, z: 0 })
    ));
}

#[test]
fn region_refuses_external_lz4_and_custom_compression() {
    let dir = TempDir::new("compression");
    let mut lz4 = RegionBuilder::new();
    lz4.add(chunk_index(0, 0), 4, &[1, 2, 3, 4]);
    let lz4 = write_region(dir.path(), 0, 0, lz4);
    assert!(matches!(
        RegionFile::open(&lz4).expect("open").read_chunk(0, 0),
        Err(AnvilError::UnsupportedCompression(4))
    ));

    let mut external = RegionBuilder::new();
    external.add(chunk_index(0, 0), 130, &[]);
    let external = write_region(&dir.path().join("external"), 0, 0, external);
    assert!(matches!(
        RegionFile::open(&external).expect("open").read_chunk(0, 0),
        Err(AnvilError::ExternalChunk { x: 0, z: 0 })
    ));

    let mut custom = RegionBuilder::new();
    custom.add(chunk_index(0, 0), 127, b"custom");
    let custom = write_region(&dir.path().join("custom"), 0, 0, custom);
    assert!(matches!(
        RegionFile::open(&custom).expect("open").read_chunk(0, 0),
        Err(AnvilError::UnsupportedCompression(127))
    ));
}

#[test]
fn region_header_validation_rejects_overlap_short_files_and_bad_names() {
    let dir = TempDir::new("header");

    let mut file = vec![0u8; 16384];
    let set = |file: &mut [u8], index: usize, offset: u32, sectors: u8| {
        let base = index * 4;
        file[base] = (offset >> 16) as u8;
        file[base + 1] = (offset >> 8) as u8;
        file[base + 2] = offset as u8;
        file[base + 3] = sectors;
    };
    set(&mut file, 0, 2, 2);
    set(&mut file, 1, 3, 1);
    fs::write(dir.path().join("r.0.0.mca"), &file).expect("write overlap");
    assert!(matches!(
        RegionFile::open(dir.path().join("r.0.0.mca")),
        Err(AnvilError::InvalidRegion(message)) if message.contains("overlap")
    ));

    let mut file = vec![0u8; 8192];
    set(&mut file, 0, 1, 1);
    fs::write(dir.path().join("r.1.0.mca"), &file).expect("write header offset");
    assert!(matches!(
        RegionFile::open(dir.path().join("r.1.0.mca")),
        Err(AnvilError::InvalidRegion(message)) if message.contains("header")
    ));

    let mut file = vec![0u8; 8192];
    set(&mut file, 0, 2, 0);
    fs::write(dir.path().join("r.2.0.mca"), &file).expect("write zero sectors");
    assert!(matches!(
        RegionFile::open(dir.path().join("r.2.0.mca")),
        Err(AnvilError::InvalidRegion(message)) if message.contains("partially allocated")
    ));

    fs::write(dir.path().join("r.3.0.mca"), vec![0u8; 8193]).expect("write unaligned");
    assert!(matches!(
        RegionFile::open(dir.path().join("r.3.0.mca")),
        Err(AnvilError::InvalidRegion(message)) if message.contains("aligned")
    ));

    fs::write(dir.path().join("r.4.0.mca"), vec![0u8; 4096]).expect("write short");
    assert!(matches!(
        RegionFile::open(dir.path().join("r.4.0.mca")),
        Err(AnvilError::InvalidRegion(message)) if message.contains("at least 8 KiB")
    ));

    assert!(matches!(
        RegionFile::open(dir.path().join("r.0.mca")),
        Err(AnvilError::InvalidRegionName(_))
    ));
    assert!(matches!(
        RegionFile::open(dir.path().join("region.mca")),
        Err(AnvilError::InvalidRegionName(_))
    ));
    assert!(matches!(
        RegionFile::open(dir.path().join("missing").join("r.0.0.mca")),
        Err(AnvilError::Missing(_))
    ));
}

#[test]
fn region_chunk_validation_rejects_bad_lengths_positions_and_versions() {
    let dir = TempDir::new("chunk-validation");

    let raw = nbt::write_uncompressed(&chunk_nbt(0, 0, TARGET, vec![])).expect("chunk");
    let mut builder = RegionBuilder::new();
    builder.add(chunk_index(0, 0), 2, &zlib(&raw));
    let mut bytes = builder.build();
    bytes[8192..8196].copy_from_slice(&u32::MAX.to_be_bytes());
    fs::write(dir.path().join("r.0.0.mca"), bytes).expect("write long chunk");
    assert!(matches!(
        RegionFile::open(dir.path().join("r.0.0.mca"))
            .expect("open")
            .read_chunk(0, 0),
        Err(AnvilError::InvalidRegion(message)) if message.contains("exceeds")
    ));

    let wrong_position =
        nbt::write_uncompressed(&chunk_nbt(1, 0, TARGET, vec![])).expect("wrong position");
    let mut builder = RegionBuilder::new();
    builder.add(chunk_index(0, 0), 2, &zlib(&wrong_position));
    let path = write_region(&dir.path().join("position"), 0, 0, builder);
    assert!(matches!(
        RegionFile::open(&path).expect("open").read_chunk(0, 0),
        Err(AnvilError::InvalidRegion(message)) if message.contains("xPos/zPos")
    ));

    let wrong_version =
        nbt::write_uncompressed(&chunk_nbt(0, 0, 9999, vec![])).expect("wrong version");
    let mut builder = RegionBuilder::new();
    builder.add(chunk_index(0, 0), 2, &zlib(&wrong_version));
    let path = write_region(&dir.path().join("version"), 0, 0, builder);
    assert!(matches!(
        RegionFile::open(&path).expect("open").read_chunk(0, 0),
        Err(AnvilError::UnsupportedDataVersion { found: 9999, .. })
    ));

    let trailing = {
        let mut raw = nbt::write_uncompressed(&chunk_nbt(0, 0, TARGET, vec![])).expect("chunk");
        raw.push(0);
        raw
    };
    let mut builder = RegionBuilder::new();
    builder.add(chunk_index(0, 0), 2, &zlib(&trailing));
    let path = write_region(&dir.path().join("trailing"), 0, 0, builder);
    assert!(matches!(
        RegionFile::open(&path).expect("open").read_chunk(0, 0),
        Err(AnvilError::Nbt(nbt::NbtError::TrailingBytes))
    ));
}

#[test]
fn chunk_summary_requires_real_sections() {
    let dir = TempDir::new("sections");
    let missing_y = NbtValue::compound([("block_states", NbtValue::Compound(Default::default()))]);
    let raw = nbt::write_uncompressed(&chunk_nbt(0, 0, TARGET, vec![missing_y])).expect("chunk");
    let mut builder = RegionBuilder::new();
    builder.add(chunk_index(0, 0), 2, &zlib(&raw));
    let path = write_region(dir.path(), 0, 0, builder);
    assert!(matches!(
        RegionFile::open(&path).expect("open").read_chunk(0, 0),
        Err(AnvilError::InvalidRegion(message)) if message.contains("no Y")
    ));
}

#[test]
fn java_world_opens_classic_layout_and_reads_its_region() {
    let dir = TempDir::new("world");
    let root = dir.path().join("world");
    write_level_dat(&root, TARGET, TARGET, "1.21.4");
    fs::create_dir_all(root.join("region")).expect("create region");
    fs::create_dir_all(root.join("DIM-1").join("region")).expect("create nether region");
    fs::create_dir_all(root.join("DIM1")).expect("create end folder");
    let raw =
        nbt::write_uncompressed(&chunk_nbt(0, 0, TARGET, vec![section(0)])).expect("encode chunk");
    let mut builder = RegionBuilder::new();
    builder.add(chunk_index(0, 0), 2, &zlib(&raw));
    fs::write(root.join("region").join("r.0.0.mca"), builder.build()).expect("write region");

    let world = JavaWorld::open(&root).expect("open classic world");
    assert_eq!(world.level.data_version, TARGET);
    assert_eq!(
        world
            .dimensions
            .iter()
            .map(|dimension| dimension.id.as_str())
            .collect::<Vec<_>>(),
        [
            "minecraft:overworld",
            "minecraft:the_nether",
            "minecraft:the_end"
        ]
    );
    let overworld = world.dimension("minecraft:overworld").expect("overworld");
    assert_eq!(overworld.region_files, 1);
    assert_eq!(overworld.kind, DimensionKind::Overworld);
    assert_eq!((overworld.min_y, overworld.height), (-64, 384));
    assert_eq!(
        world.declared_dimensions,
        [
            "minecraft:overworld",
            "minecraft:the_end",
            "minecraft:the_nether"
        ]
    );
    let nether = world.dimension("minecraft:the_nether").expect("nether");
    assert_eq!(
        (nether.kind, nether.min_y, nether.height),
        (DimensionKind::Nether, 0, 256)
    );
    let end = world.dimension("minecraft:the_end").expect("end");
    assert_eq!(
        (end.kind, end.min_y, end.height),
        (DimensionKind::End, 0, 256)
    );
    assert_eq!(
        world
            .region_coordinates("minecraft:overworld")
            .expect("list regions"),
        vec![(0, 0)]
    );
    assert_eq!(
        world
            .region_coordinates("minecraft:the_nether")
            .expect("list nether regions"),
        Vec::new()
    );
    let summaries = world
        .read_region("minecraft:overworld", 0, 0)
        .expect("read region")
        .summarize_all()
        .expect("summarise");
    assert_eq!(
        summaries,
        vec![RegionChunkSummary {
            x: 0,
            z: 0,
            data_version: TARGET,
            status: Some("minecraft:full".to_owned()),
            y_pos: Some(-4),
            sections: 1,
            block_entities: 0,
            entities: 0,
        }]
    );
    assert!(matches!(
        world.read_region("minecraft:the_end", 0, 0),
        Err(AnvilError::Missing(_))
    ));
    assert!(matches!(
        world.read_region("custom:dimension", 0, 0),
        Err(AnvilError::UnknownDimension(_))
    ));
}

#[test]
fn java_world_refuses_unsupported_versions_layouts_and_mixed_markers() {
    let dir = TempDir::new("layouts");

    let other = dir.path().join("other");
    write_level_dat(&other, 4786, 4786, "26.1");
    fs::create_dir_all(other.join("region")).expect("create region");
    assert!(matches!(
        JavaWorld::open(&other),
        Err(AnvilError::UnsupportedDataVersion { found: 4786, .. })
    ));

    let modern = dir.path().join("modern");
    write_level_dat(&modern, TARGET, TARGET, "1.21.4");
    fs::create_dir_all(
        modern
            .join("dimensions")
            .join("minecraft")
            .join("overworld")
            .join("region"),
    )
    .expect("create modern overworld");
    assert!(matches!(
        JavaWorld::open(&modern),
        Err(AnvilError::UnsupportedLayout(message)) if message.contains("26.1+")
    ));

    let mixed = dir.path().join("mixed");
    write_level_dat(&mixed, TARGET, TARGET, "1.21.4");
    fs::create_dir_all(mixed.join("region")).expect("create classic region");
    fs::create_dir_all(mixed.join("dimensions").join("minecraft").join("overworld"))
        .expect("create modern marker");
    assert!(matches!(
        JavaWorld::open(&mixed),
        Err(AnvilError::UnsupportedLayout(message)) if message.contains("mixed")
    ));

    let no_region = dir.path().join("no-region");
    write_level_dat(&no_region, TARGET, TARGET, "1.21.4");
    assert!(matches!(
        JavaWorld::open(&no_region),
        Err(AnvilError::UnsupportedLayout(message)) if message.contains("region/")
    ));

    let custom = dir.path().join("custom-dimension");
    write_level_dat(&custom, TARGET, TARGET, "1.21.4");
    fs::create_dir_all(custom.join("region")).expect("create classic region");
    fs::create_dir_all(
        custom
            .join("dimensions")
            .join("custom")
            .join("void")
            .join("region"),
    )
    .expect("create custom dimension");
    assert!(matches!(
        JavaWorld::open(&custom),
        Err(AnvilError::UnsupportedLayout(message)) if message.contains("custom dimension")
    ));

    let empty_custom = dir.path().join("empty-dimensions");
    write_level_dat(&empty_custom, TARGET, TARGET, "1.21.4");
    fs::create_dir_all(empty_custom.join("region")).expect("create classic region");
    fs::create_dir_all(empty_custom.join("dimensions")).expect("create empty dimensions");
    JavaWorld::open(&empty_custom).expect("an empty dimensions/ folder carries no data");
}

#[test]
fn java_world_refuses_symlinked_dimension_folders() {
    let dir = TempDir::new("dimension-symlink");
    let root = dir.path().join("world");
    write_level_dat(&root, TARGET, TARGET, "1.21.4");
    fs::create_dir_all(root.join("region")).expect("create classic region");
    let real = dir.path().join("real-nether");
    fs::create_dir_all(real.join("region")).expect("create real nether");
    if !make_symlink_dir(&real, &root.join("DIM-1")) {
        return;
    }
    assert!(matches!(
        JavaWorld::open(&root),
        Err(AnvilError::Symlink(path)) if path.ends_with("DIM-1")
    ));
}

#[cfg(windows)]
fn make_symlink_dir(target: &Path, link: &Path) -> bool {
    std::os::windows::fs::symlink_dir(target, link).is_ok()
}

#[cfg(unix)]
fn make_symlink_dir(target: &Path, link: &Path) -> bool {
    std::os::unix::fs::symlink(target, link).is_ok()
}

#[test]
fn region_loader_does_not_follow_a_symlink() {
    let dir = TempDir::new("symlink");
    let real = dir.path().join("real.mca");
    fs::write(&real, vec![0u8; 8192]).expect("write real region");
    let link = dir.path().join("r.0.0.mca");
    let created = make_symlink(&real, &link);
    if !created {
        return;
    }
    assert!(matches!(
        RegionFile::open(&link),
        Err(AnvilError::Symlink(_))
    ));
}

#[cfg(windows)]
fn make_symlink(target: &Path, link: &Path) -> bool {
    std::os::windows::fs::symlink_file(target, link).is_ok()
}

#[cfg(unix)]
fn make_symlink(target: &Path, link: &Path) -> bool {
    std::os::unix::fs::symlink(target, link).is_ok()
}
