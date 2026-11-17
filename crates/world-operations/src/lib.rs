//! World operations: selections, fills, transactions and history.
//!
//! Implemented in milestone M2. This crate depends only on `world-domain`.

/// Placeholder proving the workspace link; replaced by real operations in M2.
pub fn crate_ready() -> bool {
    true
}

#[cfg(test)]
mod tests {
    #[test]
    fn operations_crate_is_linked() {
        assert!(super::crate_ready());
    }
}
