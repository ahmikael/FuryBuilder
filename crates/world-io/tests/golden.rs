use sha2::{Digest, Sha256};
use world_domain::BlockState;
use world_io::{default_version_profile, read_java_structure, write_java_structure};

const GOLDEN: &[u8] = include_bytes!("goldens/java-structure-1.21.4.nbt");

fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

#[test]
fn golden_java_structure_1_21_4_is_stable_and_preserves_properties() {
    assert_eq!(GOLDEN.len(), 252, "golden length changed");
    assert_eq!(
        sha256_hex(GOLDEN),
        "1801cdab7c0b4200266385552a57ae77b81d06b29d57d2892141251e89ad07e5",
        "golden bytes changed"
    );
    let parsed = read_java_structure(GOLDEN).expect("golden parses");
    let target = default_version_profile();
    assert_eq!(parsed.inspection.data_version, target.data_version);
    assert_eq!(
        parsed.inspection.target_data_version,
        Some(target.data_version)
    );
    assert_eq!(parsed.inspection.size, [3, 2, 2]);
    assert_eq!(parsed.inspection.volume, 12);
    assert_eq!(parsed.inspection.non_air_blocks, 2);
    assert_eq!(parsed.inspection.palette_size, 3);
    assert_eq!(parsed.inspection.entities, 0);
    assert_eq!(parsed.inspection.block_entities, 0);
    assert!(parsed.inspection.compatible);
    assert_eq!(parsed.author.as_deref(), Some("FuryBuilder golden fixture"));

    let stairs: BlockState =
        "minecraft:oak_stairs[facing=west,half=top,shape=straight,waterlogged=false]"
            .parse()
            .unwrap();
    assert_eq!(parsed.structure.palette[0], BlockState::default());
    assert_eq!(parsed.structure.palette[1], stairs);
    assert_eq!(parsed.structure.palette[2].name(), "minecraft:stone_bricks");
    assert_eq!(parsed.structure.blocks.len(), 2);
    assert_eq!(parsed.structure.blocks[0].pos, [0, 0, 0]);
    assert_eq!(parsed.structure.blocks[0].palette_index, 1);
    assert_eq!(parsed.structure.blocks[1].pos, [2, 1, 1]);
    assert_eq!(parsed.structure.blocks[1].palette_index, 2);

    // Re-encoding with the same author is byte-identical: this is the property export
    // verification relies on.
    assert_eq!(
        write_java_structure(&parsed.structure, "FuryBuilder golden fixture").unwrap(),
        GOLDEN
    );
}

#[test]
fn golden_corruption_is_never_accepted_silently() {
    let mut corrupt = GOLDEN.to_vec();
    let last = corrupt.len() - 1;
    corrupt[last] ^= 0xff;
    assert!(read_java_structure(&corrupt).is_err());

    let mut truncated = GOLDEN.to_vec();
    truncated.truncate(GOLDEN.len() / 2);
    assert!(read_java_structure(&truncated).is_err());
}
