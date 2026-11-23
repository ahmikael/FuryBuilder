//! Bounded exponential backoff shared by metadata and artifact requests.

use std::time::Duration;

use crate::config::NetworkConfig;
use crate::error::PipelineError;

const MAX_DELAY: Duration = Duration::from_secs(8);

/// Delay before the next attempt. A server-provided `Retry-After` wins, otherwise the base
/// delay doubles per attempt. Both are capped so a background download never sleeps for an
/// unbounded amount of time.
pub fn retry_delay(config: &NetworkConfig, attempt: u32, error: &PipelineError) -> Duration {
    if let Some(retry_after) = error.retry_after() {
        return retry_after.min(MAX_DELAY);
    }
    let factor = 1u32 << attempt.min(3);
    config
        .retry_base_delay
        .saturating_mul(factor)
        .min(MAX_DELAY)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config() -> NetworkConfig {
        NetworkConfig {
            retry_base_delay: Duration::from_millis(100),
            ..NetworkConfig::online()
        }
    }

    #[test]
    fn backoff_doubles_then_stops_at_the_cap() {
        let error = PipelineError::Transport("reset".to_owned());
        assert_eq!(
            retry_delay(&config(), 0, &error),
            Duration::from_millis(100)
        );
        assert_eq!(
            retry_delay(&config(), 1, &error),
            Duration::from_millis(200)
        );
        assert_eq!(
            retry_delay(&config(), 2, &error),
            Duration::from_millis(400)
        );
        assert_eq!(
            retry_delay(&config(), 3, &error),
            Duration::from_millis(800)
        );
        assert_eq!(
            retry_delay(&config(), 9, &error),
            Duration::from_millis(800)
        );
    }

    #[test]
    fn retry_after_is_honored_but_capped() {
        let error = PipelineError::Http {
            status: 429,
            url: "https://piston-data.mojang.com/x".to_owned(),
            retry_after: Some(Duration::from_secs(2)),
        };
        assert_eq!(retry_delay(&config(), 0, &error), Duration::from_secs(2));
        let error = PipelineError::Http {
            status: 503,
            url: "https://piston-data.mojang.com/x".to_owned(),
            retry_after: Some(Duration::from_secs(600)),
        };
        assert_eq!(retry_delay(&config(), 0, &error), MAX_DELAY);
    }
}
