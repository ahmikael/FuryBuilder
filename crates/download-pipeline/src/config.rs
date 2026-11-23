//! Network policy shared by every managed download.

use std::time::Duration;

/// Total attempts for a retryable request, including the first one.
pub const DEFAULT_ATTEMPTS: u32 = 3;
pub const DEFAULT_CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
pub const DEFAULT_READ_TIMEOUT: Duration = Duration::from_secs(120);

/// Offline/proxy/retry policy for one [`crate::DownloadClient`].
#[derive(Debug, Clone)]
pub struct NetworkConfig {
    /// Refuse every request. Cached metadata remains usable through its own cache layer.
    pub offline: bool,
    /// Explicit proxy URL. When `None`, the standard proxy environment is used.
    pub proxy: Option<String>,
    /// Total attempts including the first one; values outside `1..=5` are clamped.
    pub attempts: u32,
    pub retry_base_delay: Duration,
    pub connect_timeout: Duration,
    pub read_timeout: Duration,
    pub user_agent: String,
}

impl Default for NetworkConfig {
    fn default() -> Self {
        Self::from_lookup(|name| std::env::var(name).ok())
    }
}

impl NetworkConfig {
    /// Builds a policy from an environment-like lookup. `FURYBUILDER_OFFLINE` disables the
    /// network; `FURYBUILDER_PROXY`/`FURYBUILDER_HTTPS_PROXY` set an explicit proxy. When no
    /// explicit proxy is present, the HTTP client still honors the standard proxy variables.
    pub fn from_lookup(lookup: impl Fn(&str) -> Option<String>) -> Self {
        let offline = lookup("FURYBUILDER_OFFLINE")
            .as_deref()
            .map(str::trim)
            .is_some_and(|value| {
                value.eq_ignore_ascii_case("1")
                    || value.eq_ignore_ascii_case("true")
                    || value.eq_ignore_ascii_case("yes")
            });
        let proxy = lookup("FURYBUILDER_PROXY")
            .or_else(|| lookup("FURYBUILDER_HTTPS_PROXY"))
            .and_then(|value| normalize_proxy(Some(value)));
        Self {
            offline,
            proxy,
            attempts: DEFAULT_ATTEMPTS,
            retry_base_delay: Duration::from_millis(300),
            connect_timeout: DEFAULT_CONNECT_TIMEOUT,
            read_timeout: DEFAULT_READ_TIMEOUT,
            user_agent: "FuryBuilder/0.2 (managed Minecraft dependencies)".to_owned(),
        }
    }

    /// Explicitly offline, independent of the environment.
    pub fn offline() -> Self {
        Self {
            offline: true,
            ..Self::default()
        }
    }

    /// Explicitly online, independent of the environment's offline flag.
    pub fn online() -> Self {
        Self {
            offline: false,
            ..Self::default()
        }
    }

    pub fn with_proxy(mut self, proxy: impl Into<String>) -> Self {
        self.proxy = normalize_proxy(Some(proxy.into()));
        self
    }

    pub fn without_proxy(mut self) -> Self {
        self.proxy = None;
        self
    }

    pub fn with_attempts(mut self, attempts: u32) -> Self {
        self.attempts = attempts.clamp(1, 5);
        self
    }

    pub fn effective_attempts(&self) -> u32 {
        self.attempts.clamp(1, 5)
    }
}

fn normalize_proxy(value: Option<String>) -> Option<String> {
    value.and_then(|value| {
        let trimmed = value.trim();
        (!trimmed.is_empty()).then(|| trimmed.to_owned())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn lookup(values: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
        let values: HashMap<String, String> = values
            .iter()
            .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
            .collect();
        move |name| values.get(name).cloned()
    }

    #[test]
    fn offline_and_proxy_come_from_explicit_environment_names() {
        let config = NetworkConfig::from_lookup(lookup(&[
            ("FURYBUILDER_OFFLINE", " TRUE "),
            ("FURYBUILDER_PROXY", " http://127.0.0.1:8080 "),
            ("HTTPS_PROXY", "http://ignored.example:1"),
        ]));
        assert!(config.offline);
        assert_eq!(config.proxy.as_deref(), Some("http://127.0.0.1:8080"));

        let config = NetworkConfig::from_lookup(lookup(&[
            ("FURYBUILDER_OFFLINE", "0"),
            ("FURYBUILDER_HTTPS_PROXY", "https://proxy.example:8443"),
        ]));
        assert!(!config.offline);
        assert_eq!(config.proxy.as_deref(), Some("https://proxy.example:8443"));

        let config = NetworkConfig::from_lookup(lookup(&[("FURYBUILDER_PROXY", "   ")]));
        assert!(config.proxy.is_none());
    }

    #[test]
    fn attempts_are_bounded_and_overrides_are_explicit() {
        let config = NetworkConfig::from_lookup(|_| None);
        assert_eq!(config.effective_attempts(), DEFAULT_ATTEMPTS);
        assert_eq!(config.clone().with_attempts(0).effective_attempts(), 1);
        assert_eq!(config.clone().with_attempts(99).effective_attempts(), 5);
        assert_eq!(config.with_attempts(2).effective_attempts(), 2);

        let config = NetworkConfig::offline();
        assert!(config.offline);
        assert!(NetworkConfig::online().proxy == config.proxy);
    }
}
