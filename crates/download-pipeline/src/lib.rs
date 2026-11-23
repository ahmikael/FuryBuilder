//! One shared HTTPS pipeline for every FuryBuilder-managed download.
//!
//! Java, client assets and server JARs all go through this crate:
//!
//! - every URL, including each redirect hop, must be HTTPS and belong to an explicit
//!   [`Allowlist`];
//! - an optional explicit proxy or the standard proxy environment is honored, and offline
//!   mode refuses every request instead of silently falling back to the network;
//! - transient transport errors and retryable HTTP statuses are retried with bounded
//!   backoff, while checksum and size failures are never retried;
//! - the destination volume is checked before a byte is written;
//! - bytes stream into a same-directory temporary file, are verified by size and SHA-1 or
//!   SHA-256, and only then replace the destination atomically.

mod allowlist;
mod atomic;
mod client;
mod config;
mod disk;
mod error;
mod retry;
mod verify;

pub use allowlist::{host_of, Allowlist};
pub use atomic::{publish_directory, replace_file, write_file};
pub use client::{Checksum, DownloadClient, DownloadOutcome, DownloadSpec, MAX_REDIRECTS};
pub use config::{NetworkConfig, DEFAULT_ATTEMPTS, DEFAULT_CONNECT_TIMEOUT, DEFAULT_READ_TIMEOUT};
pub use disk::{available_space, ensure_space, DISK_MARGIN_BYTES};
pub use error::PipelineError;
pub use retry::retry_delay;
pub use verify::{write_verified, DownloadPhase, DownloadProgress};
