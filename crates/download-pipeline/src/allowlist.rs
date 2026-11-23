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
    /// Integration tests use a loopback HTTP server on an ephemeral port. Production
    /// allowlists never set this, so HTTPS on port 443 remains mandatory there.
    local_test: bool,
}

impl Allowlist {
    /// Mojang's official version metadata and object storage.
    pub const MOJANG: Self = Self::new(MOJANG_HOSTS, "Mojang");
    /// The Adoptium API used for Java release metadata.
    pub const ADOPTIUM: Self = Self::new(ADOPTIUM_HOSTS, "Adoptium");
    /// GitHub release assets used for the Temurin archive.
    pub const GITHUB_RELEASES: Self = Self::new(GITHUB_RELEASE_HOSTS, "GitHub release assets");

    pub const fn new(hosts: &'static [&'static str], label: &'static str) -> Self {
        Self {
            hosts,
            label,
            local_test: false,
        }
    }

    /// Allows a plain-HTTP loopback server on an ephemeral port. The exact host list still
    /// applies; this constructor exists only for integration tests.
    #[doc(hidden)]
    pub const fn for_local_test(hosts: &'static [&'static str], label: &'static str) -> Self {
        Self {
            hosts,
            label,
            local_test: true,
        }
    }

    pub const fn allows_plain_http(self) -> bool {
        self.local_test
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
        let host = host_of_with(url, self.local_test)
            .ok_or_else(|| PipelineError::DisallowedUrl(url.to_owned()))?;
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
    host_of_with(url, false)
}

fn host_of_with(url: &str, local_test: bool) -> Option<&str> {
    let rest = if let Some(rest) = url.strip_prefix("https://") {
        rest
    } else if local_test {
        url.strip_prefix("http://")?
    } else {
        return None;
    };
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
        if local_test {
            if port.is_empty() || port.parse::<u16>().map_or(true, |port| port == 0) {
                return None;
            }
        } else if port != "443" {
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

    #[test]
    fn local_test_allowlist_accepts_loopback_http_on_an_ephemeral_port() {
        let local = Allowlist::for_local_test(&["127.0.0.1"], "local test");
        assert!(local.allows_plain_http());
        assert!(local.check("http://127.0.0.1:51234/file").is_ok());
        assert!(local.check("http://127.0.0.1:0/file").is_err());
        assert!(local.check("http://127.0.0.1:not-a-port/file").is_err());
        assert!(local.check("http://evil.example:51234/file").is_err());
        assert!(!Allowlist::MOJANG.allows_plain_http());
        assert!(Allowlist::MOJANG
            .check("http://127.0.0.1:51234/file")
            .is_err());
        assert_eq!(host_of("http://127.0.0.1:51234/file"), None);
    }
}
