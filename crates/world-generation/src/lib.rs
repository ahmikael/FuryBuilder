//! Deterministic local generation: terrain brushes, objects and the Build DSL.
//!
//! Implemented in milestone M9. These generators complement manual edits; they never
//! pretend to reproduce Minecraft's vanilla world generator.

/// Placeholder proving the workspace link; replaced by real generators in M9.
pub fn crate_ready() -> bool {
    true
}

#[cfg(test)]
mod tests {
    #[test]
    fn generation_crate_is_linked() {
        assert!(super::crate_ready());
    }
}
