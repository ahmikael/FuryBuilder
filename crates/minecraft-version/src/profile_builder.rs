//! Assembly of version profiles from family defaults and verified metadata.
//!
//! A profile is never guessed from the version string alone. The caller
//! supplies the DataVersion and pack formats read from the client JAR, the
//! Java major from the Mojang version JSON, and the capability claim that has
//! actually been verified. Family defaults fill the layout, dimensions,
//! catalog and server facts that are shared by a whole world family.

use std::fs;
use std::io::Read;
use std::path::Path;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::version_json::VersionJson;
use crate::{
    BiomeFormat, BlockstateFormat, CapabilityError, CatalogRegistryFormat, ChunkFamily,
    ContentCatalogProfile, DimensionKind, DimensionProfile, GenerationCapabilities,
    ItemModelFormat, JavaRequirement, LevelTypeSyntax, ServerProfile, StructureProfile,
    StructureSupport, VersionFamily, VersionId, VersionProfile, WorldCapabilities, WorldLayout,
};

const MAX_JAR_VERSION_JSON_BYTES: u64 = 1024 * 1024;

/// Facts collected from the version JSON and client JAR before a profile exists.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProfileBuildInput {
    pub version: VersionId,
    pub data_version: i32,
    pub java_major: u32,
    pub resource_pack_format: Option<u32>,
    pub datapack_format: Option<u32>,
    pub capabilities: WorldCapabilities,
    pub notes: Vec<String>,
}

/// Builds a profile from the family defaults and the supplied verified facts.
pub fn build_version_profile(
    input: ProfileBuildInput,
) -> Result<VersionProfile, ProfileBuildError> {
    if input.data_version <= 0 {
        return Err(ProfileBuildError::InvalidDataVersion(input.data_version));
    }
    if input.java_major == 0 {
        return Err(ProfileBuildError::InvalidJavaMajor);
    }
    input.capabilities.validate()?;

    let family = input.version.family();
    let release_type = input.version.release_type();
    let defaults = family_defaults(family, &input.version)?;
    let profile_id = profile_id(&input.version, defaults.world_layout);

    Ok(VersionProfile {
        id: profile_id,
        minecraft_version: input.version,
        data_version: input.data_version,
        release_type,
        family,
        world_layout: defaults.world_layout,
        chunk_family: defaults.chunk_family,
        biome_format: defaults.biome_format,
        dimension_profiles: defaults.dimension_profiles,
        java: JavaRequirement {
            minimum_major: input.java_major,
            recommended_major: input.java_major,
        },
        resource_pack_format: input.resource_pack_format,
        datapack_format: input.datapack_format,
        content_catalog: defaults.content_catalog,
        structure_formats: defaults.structure_formats,
        server: defaults.server,
        capabilities: input.capabilities,
        notes: input.notes,
    })
}

/// Builds a profile from the Mojang version JSON plus the client JAR metadata.
pub fn build_profile_from_version_json(
    version_json: &VersionJson,
    jar: &JarVersionJson,
    capabilities: WorldCapabilities,
    notes: Vec<String>,
) -> Result<VersionProfile, ProfileBuildError> {
    if jar.id != version_json.id.as_str() {
        return Err(ProfileBuildError::IdMismatch {
            expected: version_json.id.to_string(),
            actual: jar.id.clone(),
        });
    }

    let java_major = version_json
        .java_major()
        .ok_or(ProfileBuildError::MissingJavaVersion)?;
    let (resource_pack_format, datapack_format) = jar.pack_formats()?;

    build_version_profile(ProfileBuildInput {
        version: version_json.id.clone(),
        data_version: jar.world_version,
        java_major,
        resource_pack_format,
        datapack_format,
        capabilities,
        notes,
    })
}

/// The subset of the client JAR's `version.json` that FuryBuilder uses.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct JarVersionJson {
    pub id: String,
    pub world_version: i32,
    #[serde(default)]
    pub pack_version: Option<JarPackVersion>,
    #[serde(default)]
    pub stable: Option<bool>,
}

impl JarVersionJson {
    /// Returns `(resource, data)` pack formats.
    pub fn pack_formats(&self) -> Result<(Option<u32>, Option<u32>), ProfileBuildError> {
        match self.pack_version {
            None => Ok((None, None)),
            Some(JarPackVersion::Split { resource, data }) => {
                if resource == 0 || data == 0 {
                    return Err(ProfileBuildError::InvalidPackFormat);
                }
                Ok((Some(resource), Some(data)))
            }
            Some(JarPackVersion::Combined(format)) => {
                if format == 0 {
                    return Err(ProfileBuildError::InvalidPackFormat);
                }
                Ok((Some(format), Some(format)))
            }
        }
    }
}

/// `pack_version` was a single number in older versions and an object later.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum JarPackVersion {
    Split { resource: u32, data: u32 },
    Combined(u32),
}

/// Reads and validates `version.json` from a client JAR.
pub fn read_client_jar_version_json(
    jar_path: impl AsRef<Path>,
    expected: &VersionId,
) -> Result<JarVersionJson, ProfileBuildError> {
    let file = fs::File::open(jar_path)?;
    let mut archive = zip::ZipArchive::new(file)?;
    let entry = archive
        .by_name("version.json")
        .map_err(|error| match error {
            zip::result::ZipError::FileNotFound => ProfileBuildError::MissingVersionJson,
            other => ProfileBuildError::Zip(other),
        })?;

    if entry.size() > MAX_JAR_VERSION_JSON_BYTES {
        return Err(ProfileBuildError::VersionJsonTooLarge);
    }

    let mut bytes = Vec::new();
    entry
        .take(MAX_JAR_VERSION_JSON_BYTES + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_JAR_VERSION_JSON_BYTES {
        return Err(ProfileBuildError::VersionJsonTooLarge);
    }

    let json = std::str::from_utf8(&bytes)?;
    let version: JarVersionJson = serde_json::from_str(json)?;

    let actual = version
        .id
        .parse::<VersionId>()
        .map_err(|_| ProfileBuildError::InvalidVersionId(version.id.clone()))?;
    if actual != *expected {
        return Err(ProfileBuildError::IdMismatch {
            expected: expected.to_string(),
            actual: version.id.clone(),
        });
    }
    if version.world_version <= 0 {
        return Err(ProfileBuildError::InvalidDataVersion(version.world_version));
    }
    version.pack_formats()?;

    Ok(version)
}

struct FamilyDefaults {
    world_layout: WorldLayout,
    chunk_family: ChunkFamily,
    biome_format: BiomeFormat,
    dimension_profiles: Vec<DimensionProfile>,
    content_catalog: ContentCatalogProfile,
    structure_formats: StructureProfile,
    server: ServerProfile,
}

fn family_defaults(
    family: VersionFamily,
    version: &VersionId,
) -> Result<FamilyDefaults, ProfileBuildError> {
    match family {
        VersionFamily::Modern => Ok(FamilyDefaults {
            world_layout: WorldLayout::ClassicAnvil,
            chunk_family: ChunkFamily::ModernPaletted,
            biome_format: BiomeFormat::Palette4x4x4,
            dimension_profiles: classic_dimensions(-64, 384),
            content_catalog: named_catalog(ItemModelFormat::ItemDefinitions, true),
            structure_formats: java_nbt_only(),
            server: server_profile(true, LevelTypeSyntax::ModernNamespaced, true),
        }),
        VersionFamily::Split => Ok(FamilyDefaults {
            world_layout: WorldLayout::SplitAnvil,
            chunk_family: ChunkFamily::ModernPaletted,
            biome_format: BiomeFormat::Palette4x4x4,
            dimension_profiles: split_dimensions(),
            content_catalog: named_catalog(ItemModelFormat::ItemDefinitions, true),
            structure_formats: no_structures(),
            server: server_profile(true, LevelTypeSyntax::ModernNamespaced, true),
        }),
        VersionFamily::Flattened => {
            let section_biomes = minor_version(version).is_some_and(|minor| minor >= 15);
            let modern_level_type = minor_version(version).is_some_and(|minor| minor >= 16);
            Ok(FamilyDefaults {
                world_layout: WorldLayout::ClassicAnvil,
                chunk_family: ChunkFamily::SectionBiomes,
                biome_format: if section_biomes {
                    BiomeFormat::Section
                } else {
                    BiomeFormat::Column
                },
                dimension_profiles: classic_dimensions(0, 256),
                content_catalog: named_catalog(ItemModelFormat::ItemModels, false),
                structure_formats: no_structures(),
                server: server_profile(
                    true,
                    if modern_level_type {
                        LevelTypeSyntax::ModernNamespaced
                    } else {
                        LevelTypeSyntax::LegacyCamelCase
                    },
                    true,
                ),
            })
        }
        VersionFamily::Numeric => Ok(FamilyDefaults {
            world_layout: WorldLayout::ClassicAnvil,
            chunk_family: ChunkFamily::NumericIds,
            biome_format: BiomeFormat::Column,
            dimension_profiles: classic_dimensions(0, 256),
            content_catalog: ContentCatalogProfile {
                registry_format: CatalogRegistryFormat::NumericIds,
                blockstate_format: BlockstateFormat::LegacyMetadata,
                item_model_format: ItemModelFormat::LegacyMetadata,
                creative_categories: true,
                data_components: false,
            },
            structure_formats: no_structures(),
            server: server_profile(true, LevelTypeSyntax::LegacyUpper, true),
        }),
        VersionFamily::Legacy => Ok(FamilyDefaults {
            world_layout: WorldLayout::McRegion,
            chunk_family: ChunkFamily::Legacy,
            biome_format: BiomeFormat::Column,
            dimension_profiles: classic_dimensions(0, 128),
            content_catalog: ContentCatalogProfile {
                registry_format: CatalogRegistryFormat::Legacy,
                blockstate_format: BlockstateFormat::LegacyMetadata,
                item_model_format: ItemModelFormat::LegacyMetadata,
                creative_categories: false,
                data_components: false,
            },
            structure_formats: no_structures(),
            server: server_profile(false, LevelTypeSyntax::LegacyUpper, false),
        }),
        VersionFamily::Unknown => Err(ProfileBuildError::UnsupportedFamily(family)),
    }
}

fn profile_id(version: &VersionId, layout: WorldLayout) -> String {
    let prefix = match layout {
        WorldLayout::ClassicAnvil => "java-classic",
        WorldLayout::SplitAnvil => "java-split",
        WorldLayout::McRegion => "java-mcregion",
        WorldLayout::Unknown => "java-unknown",
    };
    format!("{prefix}-{version}")
}

fn minor_version(version: &VersionId) -> Option<u32> {
    version
        .as_str()
        .split('-')
        .next()?
        .split('.')
        .nth(1)?
        .parse()
        .ok()
}

fn classic_dimensions(min_y: i32, height: i32) -> Vec<DimensionProfile> {
    vec![
        DimensionProfile {
            id: "minecraft:overworld".to_owned(),
            kind: DimensionKind::Overworld,
            min_y,
            height,
            folder: "region".to_owned(),
        },
        DimensionProfile {
            id: "minecraft:the_nether".to_owned(),
            kind: DimensionKind::Nether,
            min_y,
            height,
            folder: "DIM-1/region".to_owned(),
        },
        DimensionProfile {
            id: "minecraft:the_end".to_owned(),
            kind: DimensionKind::End,
            min_y,
            height,
            folder: "DIM1/region".to_owned(),
        },
    ]
}

fn split_dimensions() -> Vec<DimensionProfile> {
    vec![
        DimensionProfile {
            id: "minecraft:overworld".to_owned(),
            kind: DimensionKind::Overworld,
            min_y: -64,
            height: 384,
            folder: "dimensions/minecraft/overworld/region".to_owned(),
        },
        DimensionProfile {
            id: "minecraft:the_nether".to_owned(),
            kind: DimensionKind::Nether,
            min_y: 0,
            height: 256,
            folder: "dimensions/minecraft/the_nether/region".to_owned(),
        },
        DimensionProfile {
            id: "minecraft:the_end".to_owned(),
            kind: DimensionKind::End,
            min_y: 0,
            height: 256,
            folder: "dimensions/minecraft/the_end/region".to_owned(),
        },
    ]
}

fn named_catalog(
    item_model_format: ItemModelFormat,
    data_components: bool,
) -> ContentCatalogProfile {
    ContentCatalogProfile {
        registry_format: CatalogRegistryFormat::Named,
        blockstate_format: BlockstateFormat::Json,
        item_model_format,
        creative_categories: true,
        data_components,
    }
}

fn java_nbt_only() -> StructureProfile {
    StructureProfile {
        java_nbt: StructureSupport {
            import: true,
            export: true,
        },
        sponge_schem: StructureSupport {
            import: false,
            export: false,
        },
        legacy_schematic: StructureSupport {
            import: false,
            export: false,
        },
        litematic: StructureSupport {
            import: false,
            export: false,
        },
    }
}

fn no_structures() -> StructureProfile {
    StructureProfile {
        java_nbt: StructureSupport {
            import: false,
            export: false,
        },
        sponge_schem: StructureSupport {
            import: false,
            export: false,
        },
        legacy_schematic: StructureSupport {
            import: false,
            export: false,
        },
        litematic: StructureSupport {
            import: false,
            export: false,
        },
    }
}

fn server_profile(
    official_jar: bool,
    level_type_syntax: LevelTypeSyntax,
    generation: bool,
) -> ServerProfile {
    ServerProfile {
        official_jar,
        level_type_syntax,
        generation: GenerationCapabilities {
            normal: generation,
            flat: generation,
            large_biomes: generation,
            amplified: generation,
        },
    }
}

/// Errors from profile assembly or client JAR inspection.
#[derive(Debug, Error)]
pub enum ProfileBuildError {
    #[error("version family `{0:?}` has no profile defaults")]
    UnsupportedFamily(VersionFamily),
    #[error(transparent)]
    Capabilities(#[from] CapabilityError),
    #[error("invalid DataVersion: {0}")]
    InvalidDataVersion(i32),
    #[error("invalid Java major")]
    InvalidJavaMajor,
    #[error("the version JSON does not declare a Java major")]
    MissingJavaVersion,
    #[error("invalid pack format")]
    InvalidPackFormat,
    #[error("client JAR I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("client JAR is not a valid zip: {0}")]
    Zip(#[from] zip::result::ZipError),
    #[error("client JAR does not contain version.json")]
    MissingVersionJson,
    #[error("client JAR version.json is too large")]
    VersionJsonTooLarge,
    #[error("invalid client JAR version.json: {0}")]
    Json(#[from] serde_json::Error),
    #[error("client JAR version.json is not valid UTF-8: {0}")]
    Utf8(#[from] std::str::Utf8Error),
    #[error("invalid version id in client JAR: {0}")]
    InvalidVersionId(String),
    #[error("client JAR version id mismatch: expected {expected}, got {actual}")]
    IdMismatch { expected: String, actual: String },
}

#[cfg(test)]
mod tests {
    use std::io::Write as _;
    use std::path::PathBuf;

    use super::*;
    use crate::SupportLevel;

    const JAR_VERSION_JSON: &str = r#"{
      "id": "1.21.4",
      "name": "1.21.4",
      "world_version": 4189,
      "series": "main",
      "protocol_version": 769,
      "pack_version": { "resource": 46, "data": 61 },
      "build_time": "2024-12-03T10:12:57+00:00",
      "stable": true
    }"#;

    const MOJANG_VERSION_JSON: &str = r#"{
      "id": "1.21.4",
      "type": "release",
      "javaVersion": { "component": "java-runtime-delta", "majorVersion": 21 },
      "downloads": {},
      "time": "2024-12-03T10:12:57+00:00",
      "releaseTime": "2024-12-03T10:12:57+00:00"
    }"#;

    fn version(value: &str) -> VersionId {
        value.parse().expect("valid version id")
    }

    fn full_capabilities() -> WorldCapabilities {
        WorldCapabilities {
            support_level: SupportLevel::Full,
            read: true,
            edit: true,
            write: true,
            export: true,
            generate: true,
            assets: true,
        }
    }

    fn temp_jar(name: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "furybuilder-profile-jar-{}-{name}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("temp dir");
        root.join("client.jar")
    }

    fn write_jar(path: &Path, entries: &[(&str, &[u8])]) {
        let file = fs::File::create(path).expect("create jar");
        let mut zip = zip::ZipWriter::new(file);
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored);
        for (name, bytes) in entries {
            zip.start_file(*name, options).expect("start jar entry");
            zip.write_all(bytes).expect("write jar entry");
        }
        zip.finish().expect("finish jar");
    }

    #[test]
    fn builds_a_modern_profile_from_verified_facts() {
        let profile = build_version_profile(ProfileBuildInput {
            version: version("1.21.4"),
            data_version: 4189,
            java_major: 21,
            resource_pack_format: Some(46),
            datapack_format: Some(61),
            capabilities: full_capabilities(),
            notes: vec!["fixture-backed".to_owned()],
        })
        .expect("modern profile");

        assert_eq!(profile.id, "java-classic-1.21.4");
        assert_eq!(profile.family, VersionFamily::Modern);
        assert_eq!(profile.world_layout, WorldLayout::ClassicAnvil);
        assert_eq!(profile.chunk_family, ChunkFamily::ModernPaletted);
        assert_eq!(profile.biome_format, BiomeFormat::Palette4x4x4);
        assert_eq!(profile.dimension_profiles[0].min_y, -64);
        assert_eq!(profile.dimension_profiles[0].height, 384);
        assert_eq!(profile.data_version, 4189);
        assert_eq!(profile.java.recommended_major, 21);
        assert_eq!(
            profile.content_catalog.registry_format,
            CatalogRegistryFormat::Named
        );
        assert!(profile.structure_formats.java_nbt.import);
        assert!(profile.server.generation.normal);
    }

    #[test]
    fn builds_a_split_profile_with_dimension_folders() {
        let profile = build_version_profile(ProfileBuildInput {
            version: version("26.1"),
            data_version: 4786,
            java_major: 25,
            resource_pack_format: Some(64),
            datapack_format: Some(80),
            capabilities: WorldCapabilities {
                support_level: SupportLevel::ReadOnly,
                read: true,
                edit: false,
                write: false,
                export: true,
                generate: false,
                assets: true,
            },
            notes: Vec::new(),
        })
        .expect("split profile");

        assert_eq!(profile.id, "java-split-26.1");
        assert_eq!(profile.world_layout, WorldLayout::SplitAnvil);
        assert_eq!(
            profile.dimension_profiles[0].folder,
            "dimensions/minecraft/overworld/region"
        );
        assert!(!profile.structure_formats.java_nbt.import);
        assert_eq!(profile.capabilities.support_level, SupportLevel::ReadOnly);
    }

    #[test]
    fn builds_a_flattened_profile_with_version_specific_biomes() {
        let profile = build_version_profile(ProfileBuildInput {
            version: version("1.16.5"),
            data_version: 2586,
            java_major: 8,
            resource_pack_format: Some(6),
            datapack_format: Some(6),
            capabilities: WorldCapabilities {
                support_level: SupportLevel::ReadOnly,
                read: true,
                edit: false,
                write: false,
                export: true,
                generate: false,
                assets: true,
            },
            notes: Vec::new(),
        })
        .expect("flattened profile");

        assert_eq!(profile.id, "java-classic-1.16.5");
        assert_eq!(profile.chunk_family, ChunkFamily::SectionBiomes);
        assert_eq!(profile.biome_format, BiomeFormat::Section);
        assert_eq!(
            profile.server.level_type_syntax,
            LevelTypeSyntax::ModernNamespaced
        );
        assert_eq!(
            profile.content_catalog.item_model_format,
            ItemModelFormat::ItemModels
        );

        let older = build_version_profile(ProfileBuildInput {
            version: version("1.13.2"),
            data_version: 1631,
            java_major: 8,
            resource_pack_format: Some(4),
            datapack_format: Some(4),
            capabilities: WorldCapabilities {
                support_level: SupportLevel::ReadOnly,
                read: true,
                edit: false,
                write: false,
                export: true,
                generate: false,
                assets: true,
            },
            notes: Vec::new(),
        })
        .expect("1.13 profile");
        assert_eq!(older.biome_format, BiomeFormat::Column);
        assert_eq!(
            older.server.level_type_syntax,
            LevelTypeSyntax::LegacyCamelCase
        );
    }

    #[test]
    fn reads_version_json_from_a_client_jar() {
        let jar = temp_jar("basic");
        write_jar(&jar, &[("version.json", JAR_VERSION_JSON.as_bytes())]);

        let parsed = read_client_jar_version_json(&jar, &version("1.21.4")).expect("jar metadata");
        assert_eq!(parsed.id, "1.21.4");
        assert_eq!(parsed.world_version, 4189);
        assert_eq!(
            parsed.pack_formats().expect("pack formats"),
            (Some(46), Some(61))
        );
        assert_eq!(parsed.stable, Some(true));

        let _ = fs::remove_dir_all(jar.parent().expect("jar parent"));
    }

    #[test]
    fn builds_a_profile_from_version_json_and_jar_metadata() {
        let jar = temp_jar("assembled");
        write_jar(&jar, &[("version.json", JAR_VERSION_JSON.as_bytes())]);
        let jar_json =
            read_client_jar_version_json(&jar, &version("1.21.4")).expect("jar metadata");
        let mojang_json = VersionJson::from_json(MOJANG_VERSION_JSON).expect("version json");

        let profile = build_profile_from_version_json(
            &mojang_json,
            &jar_json,
            full_capabilities(),
            Vec::new(),
        )
        .expect("assembled profile");
        assert_eq!(profile.id, "java-classic-1.21.4");
        assert_eq!(profile.data_version, 4189);
        assert_eq!(profile.java.recommended_major, 21);
        assert_eq!(profile.resource_pack_format, Some(46));
        assert_eq!(profile.datapack_format, Some(61));

        let _ = fs::remove_dir_all(jar.parent().expect("jar parent"));
    }

    #[test]
    fn accepts_a_combined_pack_format_from_older_jars() {
        let jar = temp_jar("combined");
        let json = JAR_VERSION_JSON.replace(
            "\"pack_version\": { \"resource\": 46, \"data\": 61 }",
            "\"pack_version\": 4",
        );
        write_jar(&jar, &[("version.json", json.as_bytes())]);

        let parsed = read_client_jar_version_json(&jar, &version("1.21.4")).expect("jar metadata");
        assert_eq!(
            parsed.pack_formats().expect("pack formats"),
            (Some(4), Some(4))
        );

        let _ = fs::remove_dir_all(jar.parent().expect("jar parent"));
    }

    #[test]
    fn rejects_missing_or_mismatched_jar_metadata() {
        let missing = temp_jar("missing");
        write_jar(&missing, &[("other.txt", b"nope")]);
        assert!(matches!(
            read_client_jar_version_json(&missing, &version("1.21.4")),
            Err(ProfileBuildError::MissingVersionJson)
        ));
        let _ = fs::remove_dir_all(missing.parent().expect("jar parent"));

        let mismatched = temp_jar("mismatched");
        write_jar(
            &mismatched,
            &[("version.json", JAR_VERSION_JSON.as_bytes())],
        );
        assert!(matches!(
            read_client_jar_version_json(&mismatched, &version("1.21.5")),
            Err(ProfileBuildError::IdMismatch { expected, actual })
                if expected == "1.21.5" && actual == "1.21.4"
        ));
        let _ = fs::remove_dir_all(mismatched.parent().expect("jar parent"));
    }

    #[test]
    fn rejects_unknown_families_and_inconsistent_capabilities() {
        let unknown = build_version_profile(ProfileBuildInput {
            version: version("24w14a"),
            data_version: 4189,
            java_major: 21,
            resource_pack_format: None,
            datapack_format: None,
            capabilities: full_capabilities(),
            notes: Vec::new(),
        });
        assert!(matches!(
            unknown,
            Err(ProfileBuildError::UnsupportedFamily(VersionFamily::Unknown))
        ));

        let inconsistent = build_version_profile(ProfileBuildInput {
            version: version("1.21.4"),
            data_version: 4189,
            java_major: 21,
            resource_pack_format: Some(46),
            datapack_format: Some(61),
            capabilities: WorldCapabilities {
                support_level: SupportLevel::Full,
                read: true,
                edit: true,
                write: false,
                export: true,
                generate: true,
                assets: true,
            },
            notes: Vec::new(),
        });
        assert!(matches!(
            inconsistent,
            Err(ProfileBuildError::Capabilities(
                CapabilityError::FullRequiresFullAccess
            ))
        ));
    }
}
