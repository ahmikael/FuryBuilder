//! The asset target is taken from the version catalog, never from a local constant.

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

pub fn target_resource_pack_format() -> i32 {
    profile()
        .resource_pack_format
        .expect("the default profile must declare a resource pack format") as i32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn target_comes_from_the_version_profile() {
        assert_eq!(target_minecraft_version(), "1.21.4");
        assert_eq!(target_data_version(), 4189);
        assert_eq!(target_resource_pack_format(), 46);
        assert_eq!(
            target_profile().minecraft_version.as_str(),
            target_minecraft_version()
        );
    }
}
