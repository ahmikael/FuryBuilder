//! CI guardrail: the baseline version literals belong to the version catalog, fixtures and
//! tests, never to a second production source of truth.
//!
//! A source file is scanned up to its first `#[cfg(test)]` module; test-only code after that
//! point is not part of the shipped working path. `tests/`, `fixtures/`, `examples/` and
//! versioned test files are skipped by path.

use std::fs;
use std::path::{Path, PathBuf};

const BASELINE_LITERALS: &[&str] = &["1.21.4", "4189"];

/// The empty-project default is the one production fallback sanctioned by the plan. It is
/// replaced by the selected project version as soon as a real project is created or opened.
const EMPTY_PROJECT_DEFAULT: &str = "crates/world-domain/src/world.rs";

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("crate lives in crates/minecraft-version")
        .to_path_buf()
}

fn is_skipped(path: &Path) -> bool {
    path.components().any(|component| {
        matches!(
            component.as_os_str().to_str(),
            Some(
                "target" | "node_modules" | ".git" | "tests" | "fixtures" | "examples" | "goldens"
            )
        )
    })
}

fn is_test_file(path: &Path) -> bool {
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default();
    name.ends_with(".test.ts")
        || name.ends_with(".test.tsx")
        || name.ends_with(".spec.ts")
        || name.ends_with(".spec.tsx")
        || name.ends_with("_test.rs")
}

fn collect_files(root: &Path, files: &mut Vec<PathBuf>) {
    let entries = match fs::read_dir(root) {
        Ok(entries) => entries,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if is_skipped(&path) {
            continue;
        }
        if path.is_dir() {
            collect_files(&path, files);
            continue;
        }
        let extension = path.extension().and_then(|value| value.to_str());
        if matches!(extension, Some("rs" | "ts" | "tsx")) && !is_test_file(&path) {
            files.push(path);
        }
    }
}

fn production_part<'a>(path: &Path, text: &'a str) -> &'a str {
    if path.extension().and_then(|value| value.to_str()) != Some("rs") {
        return text;
    }
    match text.find("#[cfg(test)]") {
        Some(index) => &text[..index],
        None => text,
    }
}

fn is_allowed_empty_default(relative: &str, line: &str) -> bool {
    relative == EMPTY_PROJECT_DEFAULT
        && (line.contains("minecraft_version: \"1.21.4\"")
            || line.trim_end().ends_with("data_version: 4189,"))
}

#[test]
fn baseline_version_literals_stay_in_the_catalog_and_tests() {
    let root = workspace_root();
    let mut files = Vec::new();
    for directory in ["crates", "apps/desktop/src", "apps/desktop/src-tauri/src"] {
        collect_files(&root.join(directory), &mut files);
    }
    files.sort();

    let mut violations = Vec::new();
    for path in files {
        let Ok(text) = fs::read_to_string(&path) else {
            continue;
        };
        let relative = path
            .strip_prefix(&root)
            .unwrap_or(&path)
            .to_string_lossy()
            .replace('\\', "/");
        if relative.starts_with("crates/minecraft-version/") {
            continue;
        }
        for (index, line) in production_part(&path, &text).lines().enumerate() {
            if BASELINE_LITERALS
                .iter()
                .any(|literal| line.contains(literal))
                && !is_allowed_empty_default(&relative, line)
            {
                violations.push(format!("{relative}:{}: {}", index + 1, line.trim()));
            }
        }
    }

    assert!(
        violations.is_empty(),
        "baseline version literals must come from the version catalog:\n{}",
        violations.join("\n")
    );
}
