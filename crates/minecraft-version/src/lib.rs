//! Minecraft Java version metadata for FuryBuilder.
//!
//! This crate is the single source of truth for version identities, version
//! families, support levels and world capabilities. It is independent of
//! Tauri, the UI, the filesystem and the network so it can be used by every
//! backend crate and tested in isolation.

#![forbid(unsafe_code)]

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
