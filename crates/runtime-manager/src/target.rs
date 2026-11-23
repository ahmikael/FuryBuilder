//! The version that Java, server and datapack provisioning currently target.
//!
//! Every value comes from the version catalog. Nothing here is a second source of truth
//! for `DataVersion`, Java major or pack formats.

use std::sync::OnceLock;

use minecraft_version::{default_profile, VersionProfile};

fn profile() -> &'static VersionProfile {
    static PROFILE: OnceLock<VersionProfile> = OnceLock::new();
    PROFILE.get_or_init(default_profile)
}

pub fn target_profile() -> &'static VersionProfile {
    profile()
}

pub fn target_minecraft_version() -> &'static str {
    profile().minecraft_version.as_str()
}

pub fn target_data_version() -> i32 {
    profile().data_version
}

pub fn target_java_major() -> u32 {
    profile().java.recommended_major
}

pub fn target_datapack_format() -> u32 {
    profile()
        .datapack_format
        .expect("the default profile must declare a datapack format")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn target_comes_from_the_version_profile() {
        assert_eq!(target_minecraft_version(), "1.21.4");
        assert_eq!(target_data_version(), 4189);
        assert_eq!(target_java_major(), 21);
        assert_eq!(target_datapack_format(), 61);
        assert_eq!(
            target_profile().minecraft_version.as_str(),
            target_minecraft_version()
        );
    }
}
