//! Bridge between the version catalog and the world adapters.
//!
//! Adapters never compare a `DataVersion` with a literal. They ask this module which
//! built-in version profile owns that value and whether its reader is verified.

use minecraft_version::{builtin_profiles, default_profile, VersionProfile};
use world_domain::WorldVersion;

/// Returns a codec-backed profile for the exact `DataVersion`.
pub fn profile_for(data_version: i32) -> Option<VersionProfile> {
    read_profile(data_version).cloned()
}

pub(crate) fn read_profile(data_version: i32) -> Option<&'static VersionProfile> {
    builtin_profiles()
        .iter()
        .find(|profile| profile.data_version == data_version && profile.capabilities.read)
}

/// Returns whether a `DataVersion` belongs to a built-in, codec-backed profile.
pub fn supports_data_version(data_version: i32) -> bool {
    read_profile(data_version).is_some()
}

/// Returns every `DataVersion` that this build can decode.
pub fn supported_data_versions() -> Vec<i32> {
    let mut versions = builtin_profiles()
        .iter()
        .filter(|profile| profile.capabilities.read)
        .map(|profile| profile.data_version)
        .collect::<Vec<_>>();
    versions.sort_unstable();
    versions
}

/// Returns the profile to use when no world or project exists yet.
pub fn default_version_profile() -> VersionProfile {
    default_profile()
}

/// Converts a verified profile into the lightweight version carried by an editable world.
pub fn world_version(profile: &VersionProfile) -> WorldVersion {
    WorldVersion::new(profile.minecraft_version.as_str(), profile.data_version)
        .expect("built-in profiles must carry a valid version identity")
}

#[cfg(test)]
mod tests {
    use super::*;
    use minecraft_version::profile_for_data_version;

    #[test]
    fn builtin_profiles_are_addressable_by_data_version() {
        let profile = profile_for(4189).expect("1.21.4 profile");

        assert_eq!(profile.minecraft_version.as_str(), "1.21.4");
        assert_eq!(profile.id, "java-classic-1.21.4");
        assert!(supports_data_version(4189));
        assert!(supports_data_version(2860), "1.18 is importable");
        assert!(!supports_data_version(9999));
        let versions = supported_data_versions();
        assert!(versions.contains(&4189));
        assert!(versions.contains(&2860));
        assert!(versions.contains(&4671));
        assert_eq!(versions.len(), 27);
        assert!(!versions.contains(&4786), "split profiles are not classic");
        assert!(
            !supports_data_version(1631),
            "1.13.2 metadata is not a verified reader"
        );
        assert!(
            !supports_data_version(2730),
            "1.17.1 metadata is not a verified reader"
        );
    }

    #[test]
    fn default_profile_and_world_version_agree() {
        let profile = default_version_profile();
        let version = world_version(&profile);

        assert_eq!(version.minecraft_version(), "1.21.4");
        assert_eq!(version.data_version(), 4189);
    }

    #[test]
    fn known_metadata_does_not_enable_world_decoding() {
        let profile = profile_for_data_version(4786).expect("known split release metadata");
        assert_eq!(profile.minecraft_version.as_str(), "26.1");
        assert!(!supports_data_version(profile.data_version));
        assert!(!supported_data_versions().contains(&profile.data_version));
    }
}
