use std::path::PathBuf;

pub fn temp_root(label: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "furybuilder-it-{label}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or(0)
    ));
    let _ = std::fs::remove_dir_all(&root);
    root
}
