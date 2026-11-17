//! Versioned world formats.
//!
//! Implemented in milestone M6 (NBT structure / Sponge) and M8 (Anvil).
//! No format adapter silently drops unknown data.

/// Placeholder proving the workspace link; replaced by real adapters in M6.
pub fn crate_ready() -> bool {
    true
}

#[cfg(test)]
mod tests {
    #[test]
    fn io_crate_is_linked() {
        assert!(super::crate_ready());
    }
}
