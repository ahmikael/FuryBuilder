//! Allowlisted HTTPS client with manual redirects, retries, offline mode and verified
//! streaming downloads.

use std::io::Read;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use crate::allowlist::Allowlist;
use crate::config::NetworkConfig;
use crate::disk;
use crate::error::PipelineError;
use crate::retry::retry_delay;
use crate::verify::{write_verified, DownloadProgress};

pub use crate::verify::{Checksum, DownloadOutcome};

/// Maximum number of redirect hops a single request may follow.
pub const MAX_REDIRECTS: usize = 5;
const MAX_RETRY_AFTER_SECONDS: u64 = 60;

/// One verified artifact request.
#[derive(Debug, Clone, Copy)]
pub struct DownloadSpec<'a> {
    pub url: &'a str,
    pub allowlist: &'a Allowlist,
    pub destination: &'a Path,
    pub expected_size: u64,
    pub checksum: Checksum<'a>,
    pub max_size: u64,
}

/// HTTPS client shared by Java, client assets and server JAR provisioning.
///
/// Construction is infallible: a malformed proxy is reported by the first request instead of
/// panicking during application setup.
#[derive(Clone)]
pub struct DownloadClient {
    config: NetworkConfig,
    agent: Result<ureq::Agent, String>,
}

impl DownloadClient {
    pub fn new(config: NetworkConfig) -> Self {
        let agent = build_agent(&config);
        Self { config, agent }
    }

    pub fn config(&self) -> &NetworkConfig {
        &self.config
    }

    pub fn is_offline(&self) -> bool {
        self.config.offline
    }

    /// Fetches a bounded metadata document. Redirects are followed manually and every hop is
    /// checked against the same allowlist.
    pub fn get_bytes(
        &self,
        url: &str,
        allowlist: &Allowlist,
        max_bytes: u64,
    ) -> Result<Vec<u8>, PipelineError> {
        if self.config.offline {
            return Err(PipelineError::Offline);
        }
        allowlist.check(url)?;
        self.with_retries(|| self.get_bytes_once(url, allowlist, max_bytes))
    }

    /// Streams an artifact into `destination`, enforcing the announced size and checksum.
    /// Retries restart the whole transfer; a checksum, size or policy failure is definitive.
    pub fn download(
        &self,
        spec: &DownloadSpec<'_>,
        cancel: &AtomicBool,
        progress: &dyn Fn(DownloadProgress),
    ) -> Result<DownloadOutcome, PipelineError> {
        if self.config.offline {
            return Err(PipelineError::Offline);
        }
        spec.checksum.validate()?;
        spec.allowlist.check(spec.url)?;
        if spec.expected_size == 0 {
            return Err(PipelineError::Limit(
                "the announced download size is zero".to_owned(),
            ));
        }
        if spec.expected_size > spec.max_size {
            return Err(PipelineError::Limit(format!(
                "announced size {} exceeds {} bytes",
                spec.expected_size, spec.max_size
            )));
        }
        let parent = spec
            .destination
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        std::fs::create_dir_all(parent)?;
        disk::ensure_space(parent, spec.expected_size)?;
        self.with_retries_cancellable(|| self.download_once(spec, cancel, progress), cancel)
    }

    fn get_bytes_once(
        &self,
        url: &str,
        allowlist: &Allowlist,
        max_bytes: u64,
    ) -> Result<Vec<u8>, PipelineError> {
        let response = self.open(url, allowlist)?;
        if response.status() != 200 {
            return Err(http_error(response.status(), &response, url));
        }
        if let Some(length) = content_length(&response) {
            if length > max_bytes {
                return Err(PipelineError::Limit(format!(
                    "announced size {length} exceeds {max_bytes} bytes"
                )));
            }
        }
        let mut bytes = Vec::new();
        response
            .into_reader()
            .take(max_bytes.saturating_add(1))
            .read_to_end(&mut bytes)
            .map_err(|error| {
                PipelineError::Transport(format!("cannot read the response body: {error}"))
            })?;
        if bytes.len() as u64 > max_bytes {
            return Err(PipelineError::Limit(format!(
                "response is larger than {max_bytes} bytes"
            )));
        }
        Ok(bytes)
    }

    fn download_once(
        &self,
        spec: &DownloadSpec<'_>,
        cancel: &AtomicBool,
        progress: &dyn Fn(DownloadProgress),
    ) -> Result<DownloadOutcome, PipelineError> {
        if cancel.load(Ordering::Relaxed) {
            return Err(PipelineError::Cancelled);
        }
        let response = self.open(spec.url, spec.allowlist)?;
        if response.status() != 200 {
            return Err(http_error(response.status(), &response, spec.url));
        }
        if let Some(length) = content_length(&response) {
            if length != spec.expected_size {
                return Err(PipelineError::Size {
                    expected: spec.expected_size,
                    actual: length,
                });
            }
        }
        write_verified(
            response.into_reader(),
            spec.destination,
            spec.expected_size,
            spec.checksum,
            spec.max_size,
            cancel,
            progress,
        )
    }

    fn open(&self, url: &str, allowlist: &Allowlist) -> Result<ureq::Response, PipelineError> {
        let agent = self.agent()?;
        let mut current = url.to_owned();
        for _ in 0..=MAX_REDIRECTS {
            allowlist.check(&current)?;
            match agent.get(&current).call() {
                Ok(response) => {
                    let status = response.status();
                    if matches!(status, 301 | 302 | 303 | 307 | 308) {
                        let location = response.header("location").ok_or_else(|| {
                            PipelineError::Transport(
                                "the remote source returned a redirect without Location".to_owned(),
                            )
                        })?;
                        current = resolve_redirect_with(
                            &current,
                            location,
                            allowlist.allows_plain_http(),
                        )?;
                        continue;
                    }
                    if !(200..=299).contains(&status) {
                        return Err(http_error(status, &response, &current));
                    }
                    return Ok(response);
                }
                Err(ureq::Error::Status(status, response)) => {
                    return Err(http_error(status, &response, &current));
                }
                Err(ureq::Error::Transport(error)) => {
                    return Err(PipelineError::Transport(error.to_string()));
                }
            }
        }
        Err(PipelineError::Transport(
            "the remote source redirected too many times".to_owned(),
        ))
    }

    fn agent(&self) -> Result<&ureq::Agent, PipelineError> {
        self.agent
            .as_ref()
            .map_err(|error| PipelineError::Proxy(error.clone()))
    }

    fn with_retries<T>(
        &self,
        mut operation: impl FnMut() -> Result<T, PipelineError>,
    ) -> Result<T, PipelineError> {
        let attempts = self.config.effective_attempts();
        let mut attempt = 0u32;
        loop {
            match operation() {
                Ok(value) => return Ok(value),
                Err(error) => {
                    attempt += 1;
                    if !error.is_retryable() || attempt >= attempts {
                        return Err(error);
                    }
                    std::thread::sleep(retry_delay(&self.config, attempt - 1, &error));
                }
            }
        }
    }

    fn with_retries_cancellable<T>(
        &self,
        mut operation: impl FnMut() -> Result<T, PipelineError>,
        cancel: &AtomicBool,
    ) -> Result<T, PipelineError> {
        let attempts = self.config.effective_attempts();
        let mut attempt = 0u32;
        loop {
            if cancel.load(Ordering::Relaxed) {
                return Err(PipelineError::Cancelled);
            }
            match operation() {
                Ok(value) => return Ok(value),
                Err(error) => {
                    attempt += 1;
                    if !error.is_retryable()
                        || attempt >= attempts
                        || cancel.load(Ordering::Relaxed)
                    {
                        return Err(error);
                    }
                    std::thread::sleep(retry_delay(&self.config, attempt - 1, &error));
                }
            }
        }
    }
}

fn build_agent(config: &NetworkConfig) -> Result<ureq::Agent, String> {
    let mut builder = ureq::AgentBuilder::new()
        .timeout_connect(config.connect_timeout)
        .timeout_read(config.read_timeout)
        .redirects(0)
        .user_agent(&config.user_agent);
    match config.proxy.as_deref() {
        Some(proxy) => {
            if proxy
                .chars()
                .any(|character| character.is_whitespace() || character.is_control())
            {
                return Err(format!("{proxy}: a proxy URL cannot contain whitespace"));
            }
            let proxy = ureq::Proxy::new(proxy).map_err(|error| format!("{proxy}: {error}"))?;
            builder = builder.proxy(proxy);
        }
        None => {
            builder = builder.try_proxy_from_env(true);
        }
    }
    Ok(builder.build())
}

fn content_length(response: &ureq::Response) -> Option<u64> {
    response
        .header("content-length")
        .and_then(|value| value.trim().parse::<u64>().ok())
}

fn http_error(status: u16, response: &ureq::Response, url: &str) -> PipelineError {
    PipelineError::Http {
        status,
        url: url.to_owned(),
        retry_after: parse_retry_after(response.header("retry-after")),
    }
}

fn parse_retry_after(value: Option<&str>) -> Option<Duration> {
    let value = value?.trim();
    value
        .parse::<u64>()
        .ok()
        .map(|seconds| Duration::from_secs(seconds.min(MAX_RETRY_AFTER_SECONDS)))
}

/// Resolves an HTTP `Location` against the request URL without ever accepting a non-HTTPS
/// scheme. The resulting host is checked again by the redirect loop.
#[cfg(test)]
fn resolve_redirect(current: &str, location: &str) -> Result<String, PipelineError> {
    resolve_redirect_with(current, location, false)
}

fn resolve_redirect_with(
    current: &str,
    location: &str,
    allow_plain_http: bool,
) -> Result<String, PipelineError> {
    if location.is_empty() {
        return Err(PipelineError::Transport(
            "the remote source returned an empty redirect target".to_owned(),
        ));
    }
    if location
        .chars()
        .any(|character| character.is_control() || character.is_whitespace())
    {
        return Err(PipelineError::DisallowedUrl(location.to_owned()));
    }
    if location.starts_with("https://") {
        return Ok(location.to_owned());
    }
    if allow_plain_http && location.starts_with("http://") {
        return Ok(location.to_owned());
    }
    if location.starts_with("http://") || location.contains("://") {
        return Err(PipelineError::DisallowedUrl(location.to_owned()));
    }
    let scheme_end = current
        .find("://")
        .map(|index| index + 3)
        .ok_or_else(|| PipelineError::DisallowedUrl(current.to_owned()))?;
    let after_scheme = &current[scheme_end..];
    let authority_end = after_scheme
        .find(['/', '?', '#'])
        .map(|index| scheme_end + index)
        .unwrap_or(current.len());
    let origin = &current[..authority_end];
    if location.starts_with("//") {
        let scheme = if allow_plain_http && current.starts_with("http://") {
            "http"
        } else {
            "https"
        };
        return Ok(format!("{scheme}:{location}"));
    }
    if location.starts_with('/') {
        return Ok(format!("{origin}{location}"));
    }
    let path_end = current.find(['?', '#']).unwrap_or(current.len());
    let base = &current[..path_end];
    let base_end = base
        .rfind('/')
        .filter(|index| *index >= scheme_end)
        .unwrap_or(origin.len());
    Ok(format!("{}/{}", &base[..base_end], location))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn offline_mode_refuses_before_any_request() {
        let client = DownloadClient::new(NetworkConfig::offline());
        assert!(client.is_offline());
        assert!(matches!(
            client.get_bytes(
                "https://piston-meta.mojang.com/mc/game/version_manifest_v2.json",
                &Allowlist::MOJANG,
                1024
            ),
            Err(PipelineError::Offline)
        ));
        let destination = std::env::temp_dir().join("furybuilder-offline-download");
        let _ = std::fs::remove_file(&destination);
        assert!(matches!(
            client.download(
                &DownloadSpec {
                    url: "https://piston-data.mojang.com/v1/objects/x/server.jar",
                    allowlist: &Allowlist::MOJANG,
                    destination: &destination,
                    expected_size: 1,
                    checksum: Checksum::Sha1(&"a".repeat(40)),
                    max_size: 1024,
                },
                &AtomicBool::new(false),
                &|_| {}
            ),
            Err(PipelineError::Offline)
        ));
        assert!(!destination.exists());
    }

    #[test]
    fn malformed_proxy_and_disallowed_hosts_fail_without_network_access() {
        let client =
            DownloadClient::new(NetworkConfig::online().with_proxy("ftp://proxy.example:21"));
        assert!(matches!(
            client.get_bytes("https://piston-meta.mojang.com/x", &Allowlist::MOJANG, 1024),
            Err(PipelineError::Proxy(_))
        ));

        let client = DownloadClient::new(NetworkConfig::online().with_proxy("http://bad proxy"));
        assert!(matches!(
            client.get_bytes("https://piston-meta.mojang.com/x", &Allowlist::MOJANG, 1024),
            Err(PipelineError::Proxy(_))
        ));

        let client = DownloadClient::new(NetworkConfig::online());
        assert!(matches!(
            client.get_bytes("https://evil.example/x", &Allowlist::MOJANG, 1024),
            Err(PipelineError::DisallowedUrl(_))
        ));
        assert!(matches!(
            client.get_bytes("http://piston-meta.mojang.com/x", &Allowlist::MOJANG, 1024),
            Err(PipelineError::DisallowedUrl(_))
        ));
    }

    #[test]
    fn invalid_checksum_is_rejected_before_any_request() {
        let client = DownloadClient::new(NetworkConfig::online());
        let destination = std::env::temp_dir().join("furybuilder-invalid-checksum-download");
        let _ = std::fs::remove_file(&destination);
        let error = client
            .download(
                &DownloadSpec {
                    url: "https://piston-data.mojang.com/v1/objects/x/server.jar",
                    allowlist: &Allowlist::MOJANG,
                    destination: &destination,
                    expected_size: 10,
                    checksum: Checksum::Sha1("not-a-sha1"),
                    max_size: 1024,
                },
                &AtomicBool::new(false),
                &|_| {},
            )
            .expect_err("invalid checksum");
        assert!(matches!(error, PipelineError::Checksum { .. }));
        assert!(!destination.exists());
    }

    #[test]
    fn redirects_resolve_relative_and_protocol_relative_locations() {
        let current = "https://github.com/adoptium/temurin21/releases/download/x";
        assert_eq!(
            resolve_redirect(current, "https://objects.githubusercontent.com/a").unwrap(),
            "https://objects.githubusercontent.com/a"
        );
        assert_eq!(
            resolve_redirect(current, "//objects.githubusercontent.com/a").unwrap(),
            "https://objects.githubusercontent.com/a"
        );
        assert_eq!(
            resolve_redirect(current, "/adoptium/other").unwrap(),
            "https://github.com/adoptium/other"
        );
        assert_eq!(
            resolve_redirect(current, "next.zip").unwrap(),
            "https://github.com/adoptium/temurin21/releases/download/next.zip"
        );
        assert_eq!(
            resolve_redirect("https://github.com", "next.zip").unwrap(),
            "https://github.com/next.zip"
        );
        assert_eq!(
            resolve_redirect("https://github.com/a/b?token=1", "next").unwrap(),
            "https://github.com/a/next"
        );
    }

    #[test]
    fn redirects_never_downgrade_to_plain_http() {
        for location in [
            "http://piston-data.mojang.com/x",
            "ftp://piston-data.mojang.com/x",
            " https://piston-data.mojang.com/x",
            "",
        ] {
            assert!(
                resolve_redirect("https://piston-meta.mojang.com/x", location).is_err(),
                "{location}"
            );
        }
    }

    #[test]
    fn local_test_redirects_keep_the_loopback_scheme() {
        let current = "http://127.0.0.1:51234/start";
        assert_eq!(
            resolve_redirect_with(current, "//127.0.0.1:51234/final", true).expect("relative"),
            "http://127.0.0.1:51234/final"
        );
        assert_eq!(
            resolve_redirect_with(current, "http://127.0.0.1:51234/final", true).expect("absolute"),
            "http://127.0.0.1:51234/final"
        );
        assert!(resolve_redirect_with(current, "http://127.0.0.1:51234/final", false).is_err());
    }

    #[test]
    fn retry_after_seconds_are_parsed_and_capped() {
        assert_eq!(parse_retry_after(Some("3")), Some(Duration::from_secs(3)));
        assert_eq!(
            parse_retry_after(Some("600")),
            Some(Duration::from_secs(MAX_RETRY_AFTER_SECONDS))
        );
        assert_eq!(parse_retry_after(Some("soon")), None);
        assert_eq!(parse_retry_after(None), None);
    }
}
