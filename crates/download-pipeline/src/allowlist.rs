//! Exact-host allowlists for official download sources.

use crate::error::PipelineError;

/// Official Mojang metadata and object hosts.
pub const MOJANG_HOSTS: &[&str] = &[
    "piston-meta.mojang.com",
    "launchermeta.mojang.com",
    "piston-data.mojang.com",
    "launcher.mojang.com",
];

/// Adoptium API host used only to resolve a Temurin release preview.
pub const ADOPTIUM_HOSTS: &[&str] = &["api.adoptium.net"];

/// GitHub release hosts that redirect to the actual Adoptium archive.
pub const GITHUB_RELEASE_HOSTS: &[&str] = &[
    "github.com",
    "objects.githubusercontent.com",
    "release-assets.githubusercontent.com",
    "github-releases.githubusercontent.com",
];

/// A set of exact hosts a request and every redirect hop must belong to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Allowlist {
    hosts: &'static [&'static str],
    label: &'static str,
}

impl Allowlist {
    /// Mojang's official version metadata and object storage.
    pub const MOJANG: Self = Self {
        hosts: MOJANG_HOSTS,
        label: "Mojang",
    };
    /// The Adoptium API used for Java release metadata.
    pub const ADOPTIUM: Self = Self {
        hosts: ADOPTIUM_HOSTS,
        label: "Adoptium",
    };
    /// GitHub release assets used for the Temurin archive.
    pub const GITHUB_RELEASES: Self = Self {
        hosts: GITHUB_RELEASE_HOSTS,
        label: "GitHub release assets",
    };

    pub const fn new(hosts: &'static [&'static str], label: &'static str) -> Self {
        Self { hosts, label }
    }

    pub fn label(self) -> &'static str {
        self.label
    }

    pub fn is_allowed(self, url: &str) -> bool {
        self.check(url).is_ok()
    }

    /// Rejects a URL that is not HTTPS, carries credentials, uses a non-default port or
    /// points at any host outside this exact allowlist.
    pub fn check(self, url: &str) -> Result<(), PipelineError> {
        let host = host_of(url).ok_or_else(|| PipelineError::DisallowedUrl(url.to_owned()))?;
        if self
            .hosts
            .iter()
            .any(|candidate| host.eq_ignore_ascii_case(candidate))
        {
            Ok(())
        } else {
            Err(PipelineError::DisallowedUrl(url.to_owned()))
        }
    }
}

/// Returns the exact host of an HTTPS URL, or `None` when the URL is malformed, uses a
/// non-default port, embeds credentials or is not HTTPS.
pub fn host_of(url: &str) -> Option<&str> {
    let rest = url.strip_prefix("https://")?;
    let authority_end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let authority = &rest[..authority_end];
    if authority.is_empty() || authority.contains('@') || authority.starts_with('[') {
        return None;
    }
    let (host, port) = match authority.rsplit_once(':') {
        Some((host, port)) => (host, Some(port)),
        None => (authority, None),
    };
    if let Some(port) = port {
        if port != "443" {
            return None;
        }
    }
    if host.is_empty()
        || host.contains('\\')
        || host
            .chars()
            .any(|character| character.is_whitespace() || character.is_control())
    {
        return None;
    }
    Some(host)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_only_exact_https_hosts_on_the_default_port() {
        assert_eq!(
            host_of("https://piston-data.mojang.com/v1/objects/x"),
            Some("piston-data.mojang.com")
        );
        assert_eq!(
            host_of("https://piston-data.mojang.com:443/v1/objects/x"),
            Some("piston-data.mojang.com")
        );
        assert!(Allowlist::MOJANG
            .check("https://piston-data.mojang.com/v1/objects/x")
            .is_ok());
        assert!(Allowlist::MOJANG
            .check("https://PISTON-DATA.MOJANG.COM/v1/objects/x")
            .is_ok());

        for rejected in [
            "http://piston-data.mojang.com/x",
            "https://piston-data.mojang.com:444/x",
            "https://user@piston-data.mojang.com/x",
            "https://piston-data.mojang.com.evil.example/x",
            "https://evil.example/piston-data.mojang.com",
            "https://piston-data.mojang.com\\@evil.example/x",
            "https://piston-data.mojang.com:/x",
            "https:///x",
        ] {
            assert!(
                Allowlist::MOJANG.check(rejected).is_err(),
                "{rejected} must be rejected"
            );
        }
    }

    #[test]
    fn allowlists_do_not_share_hosts_implicitly() {
        assert!(Allowlist::ADOPTIUM
            .check("https://api.adoptium.net/v3/assets/latest/21")
            .is_ok());
        assert!(Allowlist::ADOPTIUM
            .check("https://github.com/adoptium/x")
            .is_err());
        assert!(Allowlist::GITHUB_RELEASES
            .check("https://github.com/adoptium/x")
            .is_ok());
        assert!(Allowlist::MOJANG
            .check("https://api.adoptium.net/v3/assets/latest/21")
            .is_err());
    }
}
