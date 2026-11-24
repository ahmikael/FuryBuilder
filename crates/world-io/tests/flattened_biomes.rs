//! NBT-level biome conversion for Java 1.13–1.17.
//!
//! The fixtures use the real `Level.Biomes` shapes: 256 block columns before
//! 1.15 and 1024 section cells from 1.15. These releases are still not
//! admitted by the region reader; the tests exercise only the conversion
//! boundary and its explicit policies.

use minecraft_version::profile_builder::{build_version_profile, ProfileBuildInput};
use minecraft_version::{default_profile, SupportLevel, VersionProfile, WorldCapabilities};
use world_domain::{BiomeSection, BiomeState};
use world_io::nbt::{NbtFile, NbtValue};
use world_io::{
    decode_flattened_chunk_biomes, patch_flattened_chunk_biomes, ColumnBiomePolicy,
    FlattenedBiomeError,
};

fn flattened(version: &str, data_version: i32) -> VersionProfile {
    build_version_profile(ProfileBuildInput {
        version: version.parse().expect("version"),
        data_version,
        java_major: 8,
        resource_pack_format: Some(4),
        datapack_format: Some(4),
        pack_format_minors: None,
        capabilities: WorldCapabilities {
            support_level: SupportLevel::Unverified,
            read: false,
            edit: false,
            write: false,
            export: false,
            generate: false,
            assets: false,
        },
        notes: Vec::new(),
    })
    .expect("flattened profile")
}

fn chunk(profile: &VersionProfile, biomes: Vec<i32>) -> NbtFile {
    NbtFile::new(
        "",
        NbtValue::compound([
            ("DataVersion", NbtValue::Int(profile.data_version)),
            (
                "Level",
                NbtValue::compound([
                    ("Biomes", NbtValue::IntArray(biomes)),
                    ("Unknown", NbtValue::String("keep".to_owned())),
                ]),
            ),
        ]),
    )
}

fn biomes(nbt: &NbtFile) -> Vec<i32> {
    let level = nbt.root.as_compound().expect("root")["Level"]
        .as_compound()
        .expect("Level");
    match &level["Biomes"] {
        NbtValue::IntArray(ids) => ids.clone(),
        other => panic!("unexpected Biomes: {other:?}"),
    }
}

fn uniform(biome: &str) -> [BiomeSection; 16] {
    let biome: BiomeState = biome.parse().expect("biome");
    std::array::from_fn(|_| BiomeSection::uniform(biome.clone()))
}

#[test]
fn column_biomes_decode_into_sections_and_patch_only_the_array() {
    let profile = flattened("1.13.2", 1631);
    let source = chunk(&profile, vec![1; 256]);
    let projection =
        decode_flattened_chunk_biomes(&source, &profile, ColumnBiomePolicy::RequireUniform)
            .expect("uniform columns");
    assert_eq!(projection.mixed_column_cells, 0);
    assert_eq!(projection.sections.len(), 16);
    for section in &projection.sections {
        assert_eq!(section.palette().len(), 1);
        assert_eq!(
            section.get(0, 0, 0).expect("cell").as_str(),
            "minecraft:plains"
        );
        assert_eq!(
            section.get(3, 3, 3).expect("cell").as_str(),
            "minecraft:plains"
        );
    }

    let edited = uniform("minecraft:desert");
    let patched = patch_flattened_chunk_biomes(
        &source,
        &profile,
        &edited,
        ColumnBiomePolicy::RequireUniform,
    )
    .expect("patch columns");
    assert_eq!(biomes(&patched), vec![2; 256]);
    assert_eq!(biomes(&source), vec![1; 256], "source NBT was mutated");
    let level = patched.root.as_compound().expect("root")["Level"]
        .as_compound()
        .expect("Level");
    assert_eq!(level["Unknown"], NbtValue::String("keep".to_owned()));

    let round =
        decode_flattened_chunk_biomes(&patched, &profile, ColumnBiomePolicy::RequireUniform)
            .expect("redecode");
    assert!(round
        .sections
        .iter()
        .all(|section| section.get(3, 3, 3).expect("cell").as_str() == "minecraft:desert"));
}

#[test]
fn mixed_columns_require_an_explicit_center_sampling_choice() {
    let profile = flattened("1.14.4", 1976);
    let mut ids = vec![1; 256];
    ids[0] = 2; // Footprint (0, 0); the sampled center (2, 2) stays plains.
    let source = chunk(&profile, ids.clone());

    assert_eq!(
        decode_flattened_chunk_biomes(&source, &profile, ColumnBiomePolicy::RequireUniform)
            .expect_err("mixed footprint"),
        FlattenedBiomeError::MixedColumns { x: 0, z: 0 }
    );
    let projection =
        decode_flattened_chunk_biomes(&source, &profile, ColumnBiomePolicy::SampleCenter)
            .expect("center sampling");
    assert_eq!(projection.mixed_column_cells, 1);
    assert_eq!(
        projection.sections[0].get(0, 0, 0).expect("cell").as_str(),
        "minecraft:plains"
    );

    // The projected biome did not change, so the original mixed columns stay.
    let unchanged = patch_flattened_chunk_biomes(
        &source,
        &profile,
        &uniform("minecraft:plains"),
        ColumnBiomePolicy::RequireUniform,
    )
    .expect("unchanged projection");
    assert_eq!(biomes(&unchanged), ids);

    let desert = uniform("minecraft:desert");
    assert_eq!(
        patch_flattened_chunk_biomes(
            &source,
            &profile,
            &desert,
            ColumnBiomePolicy::RequireUniform
        )
        .expect_err("changing a mixed footprint"),
        FlattenedBiomeError::MixedColumns { x: 0, z: 0 }
    );

    let mut one_footprint = uniform("minecraft:plains");
    let desert: BiomeState = "minecraft:desert".parse().expect("biome");
    for section in &mut one_footprint {
        for y in 0..4 {
            assert_eq!(section.set(0, y, 0, &desert), Some(true));
        }
    }
    let patched = patch_flattened_chunk_biomes(
        &source,
        &profile,
        &one_footprint,
        ColumnBiomePolicy::SampleCenter,
    )
    .expect("explicit footprint replacement");
    let out = biomes(&patched);
    for z in 0..4 {
        for x in 0..4 {
            assert_eq!(out[z * 16 + x], 2, "column ({x}, {z})");
        }
    }
    assert_eq!(out[4], 1, "the next footprint must not change");
}

#[test]
fn volumetric_biomes_keep_yzx_order_and_round_trip() {
    let profile = flattened("1.16.5", 2586);
    let mut ids = vec![1; 1024];
    ids[..64].fill(8); // section 0: nether_wastes
    ids[64..128].fill(171); // section 1: crimson_forest
    let source = chunk(&profile, ids);

    let projection =
        decode_flattened_chunk_biomes(&source, &profile, ColumnBiomePolicy::RequireUniform)
            .expect("volumetric");
    assert_eq!(projection.mixed_column_cells, 0);
    assert_eq!(
        projection.sections[0].get(0, 0, 0).expect("cell").as_str(),
        "minecraft:nether_wastes"
    );
    assert_eq!(
        projection.sections[1].get(3, 3, 3).expect("cell").as_str(),
        "minecraft:crimson_forest"
    );
    assert_eq!(
        projection.sections[2].get(0, 0, 0).expect("cell").as_str(),
        "minecraft:plains"
    );

    let mut edited = projection.sections;
    let warped: BiomeState = "minecraft:warped_forest".parse().expect("biome");
    assert_eq!(edited[0].set(1, 2, 3, &warped), Some(true));
    let patched = patch_flattened_chunk_biomes(
        &source,
        &profile,
        &edited,
        ColumnBiomePolicy::RequireUniform,
    )
    .expect("patch volumetric");
    let out = biomes(&patched);
    assert_eq!(out.len(), 1024);
    assert_eq!(out[(2 * 4 + 3) * 4 + 1], 172, "cell (1, 2, 3)");
    assert_eq!(out[0], 8, "other cells in the section must not change");
    assert_eq!(out[64], 171, "other sections must not change");

    let round =
        decode_flattened_chunk_biomes(&patched, &profile, ColumnBiomePolicy::RequireUniform)
            .expect("redecode");
    assert_eq!(
        round.sections[0].get(1, 2, 3).expect("cell").as_str(),
        "minecraft:warped_forest"
    );
    assert_eq!(
        round.sections[0].get(0, 0, 0).expect("cell").as_str(),
        "minecraft:nether_wastes"
    );
}

#[test]
fn registry_aliases_and_unknown_ids_are_explicit() {
    let v113 = flattened("1.13.2", 1631);
    let mut ids = vec![1; 256];
    ids[200] = 255;
    assert_eq!(
        decode_flattened_chunk_biomes(&chunk(&v113, ids), &v113, ColumnBiomePolicy::SampleCenter)
            .expect_err("unknown id outside a sampled footprint"),
        FlattenedBiomeError::UnknownId {
            id: 255,
            index: 200
        }
    );

    let v115 = flattened("1.15.2", 2230);
    let mut ids = vec![1; 1024];
    ids[0] = 8;
    let projection =
        decode_flattened_chunk_biomes(&chunk(&v115, ids), &v115, ColumnBiomePolicy::RequireUniform)
            .expect("1.15 nether alias");
    assert_eq!(
        projection.sections[0].get(0, 0, 0).expect("cell").as_str(),
        "minecraft:nether"
    );

    let v116 = flattened("1.16.5", 2586);
    let mut ids = vec![1; 1024];
    ids[0] = 8;
    let projection =
        decode_flattened_chunk_biomes(&chunk(&v116, ids), &v116, ColumnBiomePolicy::RequireUniform)
            .expect("1.16 nether name");
    assert_eq!(
        projection.sections[0].get(0, 0, 0).expect("cell").as_str(),
        "minecraft:nether_wastes"
    );

    assert_eq!(
        patch_flattened_chunk_biomes(
            &chunk(&v113, vec![1; 256]),
            &v113,
            &uniform("minecraft:soul_sand_valley"),
            ColumnBiomePolicy::RequireUniform
        )
        .expect_err("1.13 cannot represent a 1.16 biome"),
        FlattenedBiomeError::UnrepresentableBiome("minecraft:soul_sand_valley".to_owned())
    );

    let patched = patch_flattened_chunk_biomes(
        &chunk(&v116, vec![1; 1024]),
        &v116,
        &uniform("minecraft:nether_wastes"),
        ColumnBiomePolicy::RequireUniform,
    )
    .expect("1.16 can represent nether_wastes");
    assert!(biomes(&patched).iter().all(|&id| id == 8));
}

#[test]
fn invalid_shapes_and_versions_fail_without_writing() {
    let profile = flattened("1.14.4", 1976);
    assert_eq!(
        decode_flattened_chunk_biomes(
            &chunk(&profile, vec![1; 255]),
            &profile,
            ColumnBiomePolicy::RequireUniform
        )
        .expect_err("short array"),
        FlattenedBiomeError::InvalidLength {
            found: 255,
            expected: 256
        }
    );

    let mut wrong_version = chunk(&profile, vec![1; 256]);
    let NbtValue::Compound(root) = &mut wrong_version.root else {
        unreachable!("fixture root")
    };
    root.insert(
        "DataVersion".to_owned(),
        NbtValue::Int(profile.data_version + 1),
    );
    assert_eq!(
        decode_flattened_chunk_biomes(&wrong_version, &profile, ColumnBiomePolicy::RequireUniform)
            .expect_err("wrong DataVersion"),
        FlattenedBiomeError::VersionMismatch {
            found: profile.data_version + 1,
            expected: profile.data_version
        }
    );

    let missing_level = NbtFile::new(
        "",
        NbtValue::compound([("DataVersion", NbtValue::Int(profile.data_version))]),
    );
    assert_eq!(
        decode_flattened_chunk_biomes(&missing_level, &profile, ColumnBiomePolicy::RequireUniform)
            .expect_err("missing Level"),
        FlattenedBiomeError::InvalidChunk
    );

    let mut sections = uniform("minecraft:plains");
    let desert: BiomeState = "minecraft:desert".parse().expect("biome");
    assert_eq!(sections[1].set(0, 0, 0, &desert), Some(true));
    assert_eq!(
        patch_flattened_chunk_biomes(
            &chunk(&profile, vec![1; 256]),
            &profile,
            &sections,
            ColumnBiomePolicy::SampleCenter
        )
        .expect_err("column format cannot store vertical variation"),
        FlattenedBiomeError::VerticalVariation { x: 0, z: 0 }
    );

    let modern = default_profile();
    assert!(matches!(
        decode_flattened_chunk_biomes(
            &chunk(&modern, vec![1; 1024]),
            &modern,
            ColumnBiomePolicy::RequireUniform
        ),
        Err(FlattenedBiomeError::UnsupportedProfile(_))
    ));
    assert!(
        !world_io::version_support::supports_data_version(profile.data_version),
        "biome conversion must not admit 1.14 worlds to the region reader"
    );
}
