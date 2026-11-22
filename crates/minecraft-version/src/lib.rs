//! Minecraft Java version metadata for FuryBuilder.
//!
//! This crate is the single source of truth for version identities, version
//! families, support levels and world capabilities. It is independent of
//! Tauri, the UI, the filesystem and the network so it can be used by every
//! backend crate and tested in isolation.

#![forbid(unsafe_code)]

pub mod catalog;
pub mod profile_builder;
pub mod profiles;
pub mod version_json;

pub use catalog::{
    fetch_official_manifest, is_allowed_download_url, CatalogError, CatalogSource, LatestVersions,
    ManifestEntry, ManifestVersionType, VersionCatalog, VersionManifest, DEFAULT_MANIFEST_TTL,
    MANIFEST_CACHE_SCHEMA_VERSION, OFFICIAL_MANIFEST_URL,
};
pub use profile_builder::{
    build_profile_from_version_json, build_version_profile, read_client_jar_version_json,
    JarPackVersion, JarVersionJson, ProfileBuildError, ProfileBuildInput,
};
pub use profiles::{builtin_profiles, java_classic_1_21_4, JAVA_CLASSIC_1_21_4_ID};
pub use version_json::{
    fetch_version_json, AssetIndex, JavaVersion, VersionDownload, VersionDownloads, VersionJson,
    VersionJsonError,
};

use std::cmp::Ordering;
use std::fmt;
use std::str::FromStr;

use serde::de::{self, Visitor};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use thiserror::Error;

/// A Minecraft Java version identifier such as `1.21.4`, `26.1-snapshot-3`
/// or `24w14a`.
///
/// The raw spelling is preserved. Ordering is numeric for release versions
/// (`1.21.5 < 1.22 < 26.1`) and stage-aware for snapshots and pre-releases
/// (`26.1-snapshot-3 < 26.1-pre1 < 26.1-rc1 < 26.1`). Legacy snapshots are
/// ordered by their year and week because their eventual release version is
/// not encoded in the id.
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct VersionId {
    raw: String,
    kind: VersionKind,
}

impl VersionId {
    /// Returns the original version id exactly as it was parsed.
    pub fn as_str(&self) -> &str {
        &self.raw
    }

    /// Returns the release channel encoded by the id.
    pub fn release_type(&self) -> ReleaseType {
        match &self.kind {
            VersionKind::Release { .. } => ReleaseType::Release,
            VersionKind::Snapshot { .. } | VersionKind::LegacySnapshot { .. } => {
                ReleaseType::Snapshot
            }
            VersionKind::PreRelease { .. } => ReleaseType::PreRelease,
            VersionKind::ReleaseCandidate { .. } => ReleaseType::ReleaseCandidate,
            VersionKind::OldAlpha { .. } => ReleaseType::OldAlpha,
            VersionKind::OldBeta { .. } => ReleaseType::OldBeta,
        }
    }

    /// Returns the world/format family this version belongs to.
    ///
    /// Legacy snapshots (`24w14a`) report [`VersionFamily::Unknown`] because
    /// their base release is not part of the id; profile resolution has to
    /// determine it from the manifest and DataVersion.
    pub fn family(&self) -> VersionFamily {
        match &self.kind {
            VersionKind::Release { numbers }
            | VersionKind::Snapshot { numbers, .. }
            | VersionKind::PreRelease { numbers, .. }
            | VersionKind::ReleaseCandidate { numbers, .. } => family_for(numbers),
            VersionKind::OldAlpha { .. } | VersionKind::OldBeta { .. } => VersionFamily::Legacy,
            VersionKind::LegacySnapshot { .. } => VersionFamily::Unknown,
        }
    }

    /// Returns whether this is a full release.
    pub fn is_release(&self) -> bool {
        self.release_type() == ReleaseType::Release
    }

    /// Returns whether this is a snapshot or an old-style snapshot.
    pub fn is_snapshot(&self) -> bool {
        self.release_type() == ReleaseType::Snapshot
    }

    fn sort_key(&self) -> (u8, Vec<u32>, u8, u32) {
        match &self.kind {
            VersionKind::OldAlpha { numbers } | VersionKind::OldBeta { numbers } => {
                (0, numbers.clone(), 0, 0)
            }
            VersionKind::LegacySnapshot { year, week, letter } => {
                (2, vec![*year, *week, *letter as u32], 0, 0)
            }
            VersionKind::Snapshot { numbers, ordinal } => (2, numbers.clone(), 0, *ordinal),
            VersionKind::PreRelease { numbers, ordinal } => (2, numbers.clone(), 1, *ordinal),
            VersionKind::ReleaseCandidate { numbers, ordinal } => (2, numbers.clone(), 2, *ordinal),
            VersionKind::Release { numbers } => (2, numbers.clone(), 3, 0),
        }
    }
}

impl fmt::Display for VersionId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.raw)
    }
}

impl FromStr for VersionId {
    type Err = VersionIdError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        if value.is_empty() {
            return Err(VersionIdError::Empty);
        }
        if value.trim() != value || value.chars().any(char::is_whitespace) {
            return Err(VersionIdError::Whitespace);
        }

        let lower = value.to_ascii_lowercase();

        if let Some(numbers) = lower.strip_prefix('a').and_then(parse_dotted_numbers) {
            return Ok(Self {
                raw: value.to_owned(),
                kind: VersionKind::OldAlpha { numbers },
            });
        }
        if let Some(numbers) = lower.strip_prefix('b').and_then(parse_dotted_numbers) {
            return Ok(Self {
                raw: value.to_owned(),
                kind: VersionKind::OldBeta { numbers },
            });
        }
        if let Some(kind) = parse_legacy_snapshot(&lower) {
            return Ok(Self {
                raw: value.to_owned(),
                kind,
            });
        }

        let (base, suffix) = match lower.split_once('-') {
            Some((base, suffix)) => (base, Some(suffix)),
            None => (lower.as_str(), None),
        };
        let numbers = parse_dotted_numbers(base)
            .ok_or_else(|| VersionIdError::InvalidFormat(value.to_owned()))?;

        let kind = match suffix {
            None => VersionKind::Release { numbers },
            Some(suffix) => {
                if let Some(ordinal) = suffix.strip_prefix("snapshot-") {
                    VersionKind::Snapshot {
                        numbers,
                        ordinal: parse_ordinal(ordinal)
                            .ok_or_else(|| VersionIdError::InvalidFormat(value.to_owned()))?,
                    }
                } else if let Some(ordinal) = suffix.strip_prefix("pre") {
                    VersionKind::PreRelease {
                        numbers,
                        ordinal: parse_ordinal(ordinal)
                            .ok_or_else(|| VersionIdError::InvalidFormat(value.to_owned()))?,
                    }
                } else if let Some(ordinal) = suffix.strip_prefix("rc") {
                    VersionKind::ReleaseCandidate {
                        numbers,
                        ordinal: parse_ordinal(ordinal)
                            .ok_or_else(|| VersionIdError::InvalidFormat(value.to_owned()))?,
                    }
                } else {
                    return Err(VersionIdError::InvalidFormat(value.to_owned()));
                }
            }
        };

        Ok(Self {
            raw: value.to_owned(),
            kind,
        })
    }
}

impl Ord for VersionId {
    fn cmp(&self, other: &Self) -> Ordering {
        self.sort_key()
            .cmp(&other.sort_key())
            .then_with(|| self.raw.cmp(&other.raw))
    }
}

impl PartialOrd for VersionId {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Serialize for VersionId {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.raw)
    }
}

impl<'de> Deserialize<'de> for VersionId {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_str(VersionIdVisitor)
    }
}

struct VersionIdVisitor;

impl Visitor<'_> for VersionIdVisitor {
    type Value = VersionId;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a Minecraft Java version id")
    }

    fn visit_str<E: de::Error>(self, value: &str) -> Result<Self::Value, E> {
        value.parse().map_err(E::custom)
    }

    fn visit_string<E: de::Error>(self, value: String) -> Result<Self::Value, E> {
        self.visit_str(&value)
    }
}

/// The release channel of a version id.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ReleaseType {
    Release,
    Snapshot,
    PreRelease,
    ReleaseCandidate,
    OldAlpha,
    OldBeta,
}

/// Coarse world/format family used to select codecs, profiles and adapters.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum VersionFamily {
    /// 1.18 and newer classic Anvil worlds.
    Modern,
    /// 1.13 through 1.17 flattened classic Anvil worlds.
    Flattened,
    /// 1.8 through 1.12 numbered block ids in classic Anvil worlds.
    Numeric,
    /// Pre-1.8 and McRegion worlds.
    Legacy,
    /// 26.1 and newer split world layout.
    Split,
    /// Not enough information to classify the id.
    Unknown,
}

/// A complete, verified description of one Minecraft Java version.
///
/// Profiles are facts, not guesses: every field is filled from a version
/// manifest entry, the client/server JAR metadata or a fixture-backed family
/// adapter. Missing facts stay `None` instead of being approximated.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VersionProfile {
    /// Stable profile id, for example `java-classic-1.21.4`.
    pub id: String,
    pub minecraft_version: VersionId,
    pub data_version: i32,
    pub release_type: ReleaseType,
    pub family: VersionFamily,
    pub world_layout: WorldLayout,
    pub chunk_family: ChunkFamily,
    pub biome_format: BiomeFormat,
    pub dimension_profiles: Vec<DimensionProfile>,
    pub java: JavaRequirement,
    pub resource_pack_format: Option<u32>,
    pub datapack_format: Option<u32>,
    pub content_catalog: ContentCatalogProfile,
    pub structure_formats: StructureProfile,
    pub server: ServerProfile,
    pub capabilities: WorldCapabilities,
    pub notes: Vec<String>,
}

/// How a world folder is laid out on disk.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum WorldLayout {
    /// `region/`, `DIM-1/`, `DIM1/` and a single `level.dat`.
    ClassicAnvil,
    /// 26.1+ `dimensions/<namespace>/<id>/region/` and `players/`.
    SplitAnvil,
    /// Pre-Anvil `.mcr` region files.
    McRegion,
    Unknown,
}

/// The chunk NBT family a profile must decode.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ChunkFamily {
    /// 1.18+ paletted sections with block states, biomes and light.
    ModernPaletted,
    /// 1.15–1.17 section biomes and 1.13–1.14 column biomes.
    SectionBiomes,
    /// 1.8–1.12 numeric block ids plus metadata.
    NumericIds,
    /// Pre-1.8 chunk data.
    Legacy,
    Unknown,
}

/// How biome cells are stored inside a chunk.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum BiomeFormat {
    /// 4×4×4 biome cells with a shared palette.
    #[serde(rename = "palette-4x4x4")]
    Palette4x4x4,
    /// One biome per X/Z column, repeated across Y.
    Column,
    /// Per-section biome palettes.
    Section,
    Unknown,
}

/// One dimension with its real vertical bounds and on-disk folder.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DimensionProfile {
    /// Namespaced dimension id, for example `minecraft:overworld`.
    pub id: String,
    pub kind: DimensionKind,
    pub min_y: i32,
    pub height: i32,
    /// Region folder relative to the world root.
    pub folder: String,
}

/// Dimension kind without depending on the world-domain crate.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DimensionKind {
    Overworld,
    Nether,
    End,
    Custom,
}

/// Java runtime requirement for a version.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JavaRequirement {
    pub minimum_major: u32,
    pub recommended_major: u32,
}

/// What the client JAR contains for the block/item catalog.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContentCatalogProfile {
    pub registry_format: CatalogRegistryFormat,
    pub blockstate_format: BlockstateFormat,
    pub item_model_format: ItemModelFormat,
    pub creative_categories: bool,
    pub data_components: bool,
}

/// Registry identity format used by the version.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CatalogRegistryFormat {
    /// Named ids such as `minecraft:stone`.
    Named,
    /// Numeric ids plus metadata, 1.8–1.12.
    NumericIds,
    /// Pre-1.8 registry.
    Legacy,
    Unknown,
}

/// How block states are described in the client JAR.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum BlockstateFormat {
    /// `assets/<namespace>/blockstates/<block>.json`.
    Json,
    /// Pre-1.8 metadata-based states.
    LegacyMetadata,
    None,
    Unknown,
}

/// How item models are described in the client JAR.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ItemModelFormat {
    /// `assets/<namespace>/items/<item>.json` item definitions.
    ItemDefinitions,
    /// Classic `assets/<namespace>/models/item/<item>.json`.
    ItemModels,
    /// Pre-1.8 metadata-based items.
    LegacyMetadata,
    None,
    Unknown,
}

/// Structure formats available to a version.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StructureProfile {
    pub java_nbt: StructureSupport,
    pub sponge_schem: StructureSupport,
    pub legacy_schematic: StructureSupport,
    pub litematic: StructureSupport,
}

/// Import/export availability of one structure format.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StructureSupport {
    pub import: bool,
    pub export: bool,
}

/// Official server availability and generation syntax for a version.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerProfile {
    pub official_jar: bool,
    pub level_type_syntax: LevelTypeSyntax,
    pub generation: GenerationCapabilities,
}

/// The `level-type` spelling used by a version's server.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum LevelTypeSyntax {
    /// 1.16+ namespaced values such as `minecraft:flat`.
    ModernNamespaced,
    /// 1.13–1.15 camel-case values such as `largeBiomes`.
    LegacyCamelCase,
    /// 1.8–1.12 upper-case values such as `LARGEBIOMES`.
    LegacyUpper,
    Unknown,
}

/// Generation presets the official server can produce.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GenerationCapabilities {
    pub normal: bool,
    pub flat: bool,
    pub large_biomes: bool,
    pub amplified: bool,
}

/// What FuryBuilder can actually do with this version.
///
/// The support level is the contract shown in the UI and MCP; the booleans
/// describe individual operations so callers never infer them from the
/// version string. [`WorldCapabilities::validate`] rejects contradictory
/// combinations such as `full` without write access.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorldCapabilities {
    pub support_level: SupportLevel,
    pub read: bool,
    pub edit: bool,
    pub write: bool,
    pub export: bool,
    pub generate: bool,
    pub assets: bool,
}

/// The honest support contract for a version family.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SupportLevel {
    /// Create, import, edit, save, export and use assets.
    Full,
    /// Read and edit a verified subset; untouched data is preserved.
    ReadWriteSubset,
    /// Analyze, view and copy without editing.
    ReadOnly,
    /// Read and convert to a supported version; the original is preserved.
    Convert,
    /// Explicit refusal, for example Bedrock Edition.
    OutOfScope,
}

impl WorldCapabilities {
    /// Rejects capability claims that contradict their support level.
    pub fn validate(self) -> Result<(), CapabilityError> {
        match self.support_level {
            SupportLevel::Full => {
                if !(self.read && self.edit && self.write && self.export && self.assets) {
                    return Err(CapabilityError::FullRequiresFullAccess);
                }
            }
            SupportLevel::ReadWriteSubset => {
                if !(self.read && self.edit && self.write) {
                    return Err(CapabilityError::ReadWriteSubsetRequiresWrite);
                }
            }
            SupportLevel::ReadOnly => {
                if !self.read || self.edit || self.write {
                    return Err(CapabilityError::ReadOnlyMustNotWrite);
                }
            }
            SupportLevel::Convert => {
                if !self.read || self.write {
                    return Err(CapabilityError::ConvertMustReadWithoutWriting);
                }
            }
            SupportLevel::OutOfScope => {
                if self.read
                    || self.edit
                    || self.write
                    || self.export
                    || self.generate
                    || self.assets
                {
                    return Err(CapabilityError::OutOfScopeMustNotClaimCapabilities);
                }
            }
        }
        Ok(())
    }
}

/// A capability set that contradicts its support level.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Error)]
pub enum CapabilityError {
    #[error("support level `full` requires read, edit, write, export and assets")]
    FullRequiresFullAccess,
    #[error("support level `read-write-subset` requires read, edit and write")]
    ReadWriteSubsetRequiresWrite,
    #[error("support level `read-only` requires read and must not claim edit or write")]
    ReadOnlyMustNotWrite,
    #[error("support level `convert` requires read and must not claim write")]
    ConvertMustReadWithoutWriting,
    #[error("support level `out-of-scope` must not claim any capability")]
    OutOfScopeMustNotClaimCapabilities,
}

/// An invalid Minecraft version id.
#[derive(Clone, Debug, Eq, PartialEq, Error)]
pub enum VersionIdError {
    #[error("Minecraft version id is empty")]
    Empty,
    #[error("Minecraft version id contains whitespace")]
    Whitespace,
    #[error("unsupported Minecraft version id: {0}")]
    InvalidFormat(String),
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
enum VersionKind {
    Release { numbers: Vec<u32> },
    Snapshot { numbers: Vec<u32>, ordinal: u32 },
    PreRelease { numbers: Vec<u32>, ordinal: u32 },
    ReleaseCandidate { numbers: Vec<u32>, ordinal: u32 },
    LegacySnapshot { year: u32, week: u32, letter: char },
    OldAlpha { numbers: Vec<u32> },
    OldBeta { numbers: Vec<u32> },
}

fn parse_dotted_numbers(value: &str) -> Option<Vec<u32>> {
    if value.is_empty() {
        return None;
    }
    value
        .split('.')
        .map(|part| {
            if part.is_empty() || !part.bytes().all(|byte| byte.is_ascii_digit()) {
                None
            } else {
                part.parse::<u32>().ok()
            }
        })
        .collect()
}

fn parse_ordinal(value: &str) -> Option<u32> {
    if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    value.parse().ok()
}

fn parse_legacy_snapshot(value: &str) -> Option<VersionKind> {
    let bytes = value.as_bytes();
    if bytes.len() != 6
        || bytes[2] != b'w'
        || !bytes[..2].iter().all(|byte| byte.is_ascii_digit())
        || !bytes[3..5].iter().all(|byte| byte.is_ascii_digit())
        || !bytes[5].is_ascii_alphabetic()
    {
        return None;
    }

    Some(VersionKind::LegacySnapshot {
        year: value[0..2].parse().ok()?,
        week: value[3..5].parse().ok()?,
        letter: char::from(bytes[5]),
    })
}

fn family_for(numbers: &[u32]) -> VersionFamily {
    match numbers {
        [1, minor, ..] if *minor >= 18 => VersionFamily::Modern,
        [1, minor, ..] if (13..=17).contains(minor) => VersionFamily::Flattened,
        [1, minor, ..] if (8..=12).contains(minor) => VersionFamily::Numeric,
        [1, ..] => VersionFamily::Legacy,
        [major, ..] if *major >= 26 => VersionFamily::Split,
        _ => VersionFamily::Unknown,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn version(value: &str) -> VersionId {
        value.parse().expect("valid version id")
    }

    #[test]
    fn parses_release_versions() {
        for value in ["1.21.4", "1.21", "1.20.6", "1.7.10", "1.0", "26.1"] {
            let id = version(value);
            assert_eq!(id.as_str(), value);
            assert_eq!(id.release_type(), ReleaseType::Release);
            assert!(id.is_release());
            assert!(!id.is_snapshot());
        }
    }

    #[test]
    fn parses_snapshots_and_legacy_snapshots() {
        let snapshot = version("26.1-snapshot-3");
        assert_eq!(snapshot.release_type(), ReleaseType::Snapshot);
        assert_eq!(snapshot.family(), VersionFamily::Split);
        assert!(snapshot.is_snapshot());

        let legacy = version("24w14a");
        assert_eq!(legacy.release_type(), ReleaseType::Snapshot);
        assert_eq!(legacy.family(), VersionFamily::Unknown);
    }

    #[test]
    fn parses_pre_releases_rc_and_old_channels() {
        assert_eq!(
            version("1.21.4-pre2").release_type(),
            ReleaseType::PreRelease
        );
        assert_eq!(
            version("1.21.4-rc1").release_type(),
            ReleaseType::ReleaseCandidate
        );
        assert_eq!(version("b1.7.3").release_type(), ReleaseType::OldBeta);
        assert_eq!(version("a1.2.6").release_type(), ReleaseType::OldAlpha);
        assert_eq!(version("b1.7.3").family(), VersionFamily::Legacy);
    }

    #[test]
    fn classifies_version_families() {
        let cases = [
            ("1.21.4", VersionFamily::Modern),
            ("1.18.2", VersionFamily::Modern),
            ("1.16.5", VersionFamily::Flattened),
            ("1.13", VersionFamily::Flattened),
            ("1.12.2", VersionFamily::Numeric),
            ("1.8.9", VersionFamily::Numeric),
            ("1.7.10", VersionFamily::Legacy),
            ("1.0", VersionFamily::Legacy),
            ("26.1", VersionFamily::Split),
        ];

        for (value, expected) in cases {
            assert_eq!(version(value).family(), expected, "family of {value}");
        }
    }

    #[test]
    fn orders_release_versions_numerically() {
        let mut versions = ["26.1", "1.22", "1.21.5", "1.21.4", "1.21", "1.20.6"]
            .map(version)
            .to_vec();
        versions.sort();

        let ordered: Vec<&str> = versions.iter().map(VersionId::as_str).collect();
        assert_eq!(
            ordered,
            ["1.20.6", "1.21", "1.21.4", "1.21.5", "1.22", "26.1"]
        );
    }

    #[test]
    fn orders_snapshots_and_pre_releases_before_their_release() {
        let mut versions = [
            "26.1",
            "26.1-rc1",
            "26.1-pre2",
            "26.1-snapshot-10",
            "26.1-snapshot-3",
        ]
        .map(version)
        .to_vec();
        versions.sort();

        let ordered: Vec<&str> = versions.iter().map(VersionId::as_str).collect();
        assert_eq!(
            ordered,
            [
                "26.1-snapshot-3",
                "26.1-snapshot-10",
                "26.1-pre2",
                "26.1-rc1",
                "26.1"
            ]
        );
    }

    #[test]
    fn serializes_as_a_plain_string() {
        let id = version("1.21.4");
        let json = serde_json::to_string(&id).expect("version serializes");
        assert_eq!(json, "\"1.21.4\"");

        let restored: VersionId = serde_json::from_str(&json).expect("version deserializes");
        assert_eq!(restored, id);
    }

    fn sample_profile() -> VersionProfile {
        VersionProfile {
            id: "java-classic-1.21.4".to_owned(),
            minecraft_version: version("1.21.4"),
            data_version: 4189,
            release_type: ReleaseType::Release,
            family: VersionFamily::Modern,
            world_layout: WorldLayout::ClassicAnvil,
            chunk_family: ChunkFamily::ModernPaletted,
            biome_format: BiomeFormat::Palette4x4x4,
            dimension_profiles: vec![
                DimensionProfile {
                    id: "minecraft:overworld".to_owned(),
                    kind: DimensionKind::Overworld,
                    min_y: -64,
                    height: 384,
                    folder: "region".to_owned(),
                },
                DimensionProfile {
                    id: "minecraft:the_nether".to_owned(),
                    kind: DimensionKind::Nether,
                    min_y: 0,
                    height: 256,
                    folder: "DIM-1/region".to_owned(),
                },
                DimensionProfile {
                    id: "minecraft:the_end".to_owned(),
                    kind: DimensionKind::End,
                    min_y: 0,
                    height: 256,
                    folder: "DIM1/region".to_owned(),
                },
            ],
            java: JavaRequirement {
                minimum_major: 21,
                recommended_major: 21,
            },
            resource_pack_format: Some(46),
            datapack_format: Some(61),
            content_catalog: ContentCatalogProfile {
                registry_format: CatalogRegistryFormat::Named,
                blockstate_format: BlockstateFormat::Json,
                item_model_format: ItemModelFormat::ItemDefinitions,
                creative_categories: true,
                data_components: true,
            },
            structure_formats: StructureProfile {
                java_nbt: StructureSupport {
                    import: true,
                    export: true,
                },
                sponge_schem: StructureSupport {
                    import: true,
                    export: true,
                },
                legacy_schematic: StructureSupport {
                    import: true,
                    export: false,
                },
                litematic: StructureSupport {
                    import: true,
                    export: false,
                },
            },
            server: ServerProfile {
                official_jar: true,
                level_type_syntax: LevelTypeSyntax::ModernNamespaced,
                generation: GenerationCapabilities {
                    normal: true,
                    flat: true,
                    large_biomes: true,
                    amplified: true,
                },
            },
            capabilities: WorldCapabilities {
                support_level: SupportLevel::Full,
                read: true,
                edit: true,
                write: true,
                export: true,
                generate: true,
                assets: true,
            },
            notes: vec!["Only 1.21.4 classic Anvil writes are verified.".to_owned()],
        }
    }

    #[test]
    fn profile_serializes_with_camel_case_fields() {
        let value = serde_json::to_value(sample_profile()).expect("profile serializes");

        assert_eq!(value["minecraftVersion"], "1.21.4");
        assert_eq!(value["dataVersion"], 4189);
        assert_eq!(value["releaseType"], "release");
        assert_eq!(value["family"], "modern");
        assert_eq!(value["worldLayout"], "classic-anvil");
        assert_eq!(value["chunkFamily"], "modern-paletted");
        assert_eq!(value["biomeFormat"], "palette-4x4x4");
        assert_eq!(value["dimensionProfiles"][0]["minY"], -64);
        assert_eq!(value["dimensionProfiles"][0]["kind"], "overworld");
        assert_eq!(value["java"]["minimumMajor"], 21);
        assert_eq!(value["java"]["recommendedMajor"], 21);
        assert_eq!(value["resourcePackFormat"], 46);
        assert_eq!(value["datapackFormat"], 61);
        assert_eq!(value["contentCatalog"]["registryFormat"], "named");
        assert_eq!(
            value["contentCatalog"]["itemModelFormat"],
            "item-definitions"
        );
        assert_eq!(value["contentCatalog"]["dataComponents"], true);
        assert_eq!(value["structureFormats"]["javaNbt"]["export"], true);
        assert_eq!(value["structureFormats"]["litematic"]["export"], false);
        assert_eq!(value["server"]["levelTypeSyntax"], "modern-namespaced");
        assert_eq!(value["server"]["generation"]["largeBiomes"], true);
        assert_eq!(value["capabilities"]["assets"], true);
        assert_eq!(value["capabilities"]["supportLevel"], "full");
        assert!(value.get("minecraft_version").is_none());
    }

    #[test]
    fn support_level_serializes_as_kebab_case() {
        let cases = [
            (SupportLevel::Full, "full"),
            (SupportLevel::ReadWriteSubset, "read-write-subset"),
            (SupportLevel::ReadOnly, "read-only"),
            (SupportLevel::Convert, "convert"),
            (SupportLevel::OutOfScope, "out-of-scope"),
        ];

        for (level, expected) in cases {
            assert_eq!(
                serde_json::to_value(level).expect("level serializes"),
                expected
            );
        }
    }

    #[test]
    fn capabilities_accept_their_support_level() {
        assert!(sample_profile().capabilities.validate().is_ok());

        let subset = WorldCapabilities {
            support_level: SupportLevel::ReadWriteSubset,
            read: true,
            edit: true,
            write: true,
            ..empty_capabilities(SupportLevel::ReadWriteSubset)
        };
        assert!(subset.validate().is_ok());

        let read_only = WorldCapabilities {
            support_level: SupportLevel::ReadOnly,
            read: true,
            export: true,
            ..empty_capabilities(SupportLevel::ReadOnly)
        };
        assert!(read_only.validate().is_ok());

        let convert = WorldCapabilities {
            support_level: SupportLevel::Convert,
            read: true,
            export: true,
            ..empty_capabilities(SupportLevel::Convert)
        };
        assert!(convert.validate().is_ok());

        let out_of_scope = empty_capabilities(SupportLevel::OutOfScope);
        assert!(out_of_scope.validate().is_ok());
    }

    #[test]
    fn capabilities_reject_contradictions() {
        let full_without_write = WorldCapabilities {
            support_level: SupportLevel::Full,
            read: true,
            edit: true,
            export: true,
            assets: true,
            ..empty_capabilities(SupportLevel::Full)
        };
        assert_eq!(
            full_without_write.validate(),
            Err(CapabilityError::FullRequiresFullAccess)
        );

        let read_only_with_write = WorldCapabilities {
            support_level: SupportLevel::ReadOnly,
            read: true,
            write: true,
            ..empty_capabilities(SupportLevel::ReadOnly)
        };
        assert_eq!(
            read_only_with_write.validate(),
            Err(CapabilityError::ReadOnlyMustNotWrite)
        );

        let convertible_but_writable = WorldCapabilities {
            support_level: SupportLevel::Convert,
            read: true,
            write: true,
            ..empty_capabilities(SupportLevel::Convert)
        };
        assert_eq!(
            convertible_but_writable.validate(),
            Err(CapabilityError::ConvertMustReadWithoutWriting)
        );

        let out_of_scope_with_read = WorldCapabilities {
            support_level: SupportLevel::OutOfScope,
            read: true,
            ..empty_capabilities(SupportLevel::OutOfScope)
        };
        assert_eq!(
            out_of_scope_with_read.validate(),
            Err(CapabilityError::OutOfScopeMustNotClaimCapabilities)
        );
    }

    fn empty_capabilities(level: SupportLevel) -> WorldCapabilities {
        WorldCapabilities {
            support_level: level,
            read: false,
            edit: false,
            write: false,
            export: false,
            generate: false,
            assets: false,
        }
    }

    #[test]
    fn profile_round_trips_through_json() {
        let profile = sample_profile();
        let json = serde_json::to_string(&profile).expect("profile serializes");
        let restored: VersionProfile = serde_json::from_str(&json).expect("profile deserializes");
        assert_eq!(restored, profile);
    }

    #[test]
    fn profile_keeps_dimensions_and_unknown_formats_honest() {
        let mut profile = sample_profile();
        profile.resource_pack_format = None;
        profile.datapack_format = None;

        assert_eq!(profile.dimension_profiles.len(), 3);
        assert_eq!(profile.dimension_profiles[0].id, "minecraft:overworld");
        assert_eq!(profile.dimension_profiles[0].min_y, -64);
        assert_eq!(profile.dimension_profiles[0].height, 384);

        let value = serde_json::to_value(profile).expect("profile serializes");
        assert!(value["resourcePackFormat"].is_null());
        assert!(value["datapackFormat"].is_null());
    }

    #[test]
    fn rejects_invalid_ids() {
        assert_eq!("".parse::<VersionId>(), Err(VersionIdError::Empty));
        assert_eq!(
            " 1.21.4".parse::<VersionId>(),
            Err(VersionIdError::Whitespace)
        );
        for value in [
            "banana",
            "1..21",
            "1.21.",
            "1.21.4-beta",
            "1.21.4-pre",
            "26.1-snapshot",
            "1.21.4-preX",
        ] {
            assert!(
                matches!(
                    value.parse::<VersionId>(),
                    Err(VersionIdError::InvalidFormat(bad)) if bad == value
                ),
                "{value} must be rejected"
            );
        }
    }
}
