//! Small HTTPS helpers shared by the Temurin and Mojang download clients.
//!
//! Every URL is checked against an explicit host allowlist before a request and again after
//! each redirect. Redirects are followed manually so a trusted host cannot bounce the client
//! to an arbitrary server.

use std::time::Duration;

const MAX_REDIRECTS: usize = 5;

pub(crate) fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(15))
        .timeout_read(Duration::from_secs(120))
        .redirects(0)
        .user_agent("FuryBuilder/0.1 (managed Minecraft dependencies)")
        .build()
}

/// Returns the host part of an HTTPS URL, or `None` for a malformed/non-HTTPS URL.
pub(crate) fn host_of(url: &str) -> Option<&str> {
    let rest = url.strip_prefix("https://")?;
    let authority = rest.split('/').next()?;
    if authority.contains('@') {
        return None;
    }
    let host = authority.split(':').next()?;
    (!host.is_empty()).then_some(host)
}

pub(crate) fn check_url(url: &str, allowed: &[&str]) -> Result<(), String> {
    if !url.starts_with("https://") {
        return Err(format!("refusing non-HTTPS URL: {url}"));
    }
    let rest = url
        .strip_prefix("https://")
        .ok_or_else(|| format!("invalid HTTPS URL: {url}"))?;
    let authority = rest
        .split('/')
        .next()
        .ok_or_else(|| format!("invalid HTTPS URL: {url}"))?;
    if let Some((_, port)) = authority.rsplit_once(':') {
        if !port.is_empty() && port != "443" {
            return Err(format!("refusing non-default HTTPS port in {url}"));
        }
    }
    let host = host_of(url).ok_or_else(|| format!("invalid HTTPS URL: {url}"))?;
    if allowed
        .iter()
        .any(|candidate| host.eq_ignore_ascii_case(candidate))
    {
        Ok(())
    } else {
        Err(format!("refusing URL from untrusted host {host}"))
    }
}

/// Opens an allowlisted HTTPS URL, following at most five redirects and validating every hop.
pub(crate) fn open(
    agent: &ureq::Agent,
    url: &str,
    allowed: &[&str],
) -> Result<ureq::Response, String> {
    let mut current = url.to_owned();
    for _ in 0..=MAX_REDIRECTS {
        check_url(&current, allowed)?;
        let response = agent.get(&current).call().map_err(http_error)?;
        let status = response.status();
        if matches!(status, 301 | 302 | 303 | 307 | 308) {
            let location = response
                .header("location")
                .ok_or_else(|| "redirect without Location".to_owned())?;
            if !location.starts_with("https://") {
                return Err("redirect to a non-HTTPS URL was refused".to_owned());
            }
            current = location.to_owned();
            continue;
        }
        return Ok(response);
    }
    Err("too many redirects".to_owned())
}

pub(crate) fn http_error(error: ureq::Error) -> String {
    match error {
        ureq::Error::Status(status, _) => format!("HTTP {status} from the remote source"),
        ureq::Error::Transport(error) => error.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_https_hosts_without_accepting_lookalikes() {
        assert_eq!(host_of("https://github.com/x"), Some("github.com"));
        assert_eq!(host_of("http://github.com/x"), None);
        assert_eq!(host_of("https://user@github.com/x"), None);
        assert!(check_url("https://github.com/x", &["github.com"]).is_ok());
        assert!(check_url("https://github.com:443/x", &["github.com"]).is_ok());
        assert!(check_url("https://github.com:444/x", &["github.com"]).is_err());
        assert!(check_url("https://github.com.evil.example/x", &["github.com"]).is_err());
        assert!(check_url("http://github.com/x", &["github.com"]).is_err());
    }
}
