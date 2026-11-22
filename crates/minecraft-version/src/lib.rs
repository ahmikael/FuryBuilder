//! Minecraft Java version metadata for FuryBuilder.
//!
//! This crate is the single source of truth for version identities, version
//! families, support levels and world capabilities. It is independent of
//! Tauri, the UI, the filesystem and the network so it can be used by every
//! backend crate and tested in isolation.

#![forbid(unsafe_code)]
