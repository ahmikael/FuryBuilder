use std::collections::HashSet;
use std::path::{Path, PathBuf};

use crate::managed::ManagedRuntimeCache;
use crate::probe::report_for;
use crate::target::{target_java_major, target_minecraft_version};
use crate::types::{JavaRuntimeReport, ManagedCacheReport, RuntimeDiscoveryReport, RuntimeSource};

const MAX_CANDIDATES: usize = 24;
const MAX_SCAN_ENTRIES: usize = 96;

/// Discovers Java candidates from FuryBuilder's managed cache, `JAVA_HOME`, `PATH`,
/// conventional vendor directories and the official launcher's runtime folder.
/// Every candidate is executed; nothing is accepted from its name alone.
pub fn discover(cache: &ManagedRuntimeCache, saved: Option<&Path>) -> RuntimeDiscoveryReport {
    let managed = cache.list().unwrap_or_else(|_| ManagedCacheReport {
        root: cache.root().display().to_string(),
        runtimes: Vec::new(),
        total_bytes: 0,
        problems: Vec::new(),
    });
    let mut seen = HashSet::new();
    let mut candidates: Vec<(PathBuf, RuntimeSource, bool)> = Vec::new();
    let mut problems = Vec::new();

    for runtime in &managed.runtimes {
        push_candidate(
            &mut candidates,
            &mut seen,
            PathBuf::from(&runtime.java_path),
            RuntimeSource::Managed,
            true,
        );
    }
    if let Some(saved) = saved {
        push_candidate(
            &mut candidates,
            &mut seen,
            saved.to_path_buf(),
            RuntimeSource::Manual,
            false,
        );
    }
    for path in java_home_candidates() {
        push_candidate(
            &mut candidates,
            &mut seen,
            path,
            RuntimeSource::JavaHome,
            false,
        );
    }
    for path in path_candidates() {
        push_candidate(&mut candidates, &mut seen, path, RuntimeSource::Path, false);
    }
    for (root, depth) in common_roots() {
        for path in scan_for_java(&root, depth, &mut problems) {
            push_candidate(
                &mut candidates,
                &mut seen,
                path,
                RuntimeSource::Common,
                false,
            );
            if candidates.len() >= MAX_CANDIDATES {
                break;
            }
        }
        if candidates.len() >= MAX_CANDIDATES {
            break;
        }
    }
    candidates.truncate(MAX_CANDIDATES);

    let mut reports: Vec<JavaRuntimeReport> = candidates
        .into_iter()
        .map(|(path, source, managed)| report_for(&path, source, managed))
        .collect();
    reports.sort_by(|left, right| {
        let left_rank = (
            !left.is_compatible(),
            !left.managed,
            left.path.to_ascii_lowercase(),
        );
        let right_rank = (
            !right.is_compatible(),
            !right.managed,
            right.path.to_ascii_lowercase(),
        );
        left_rank.cmp(&right_rank)
    });

    let saved_report = saved.map(|path| {
        let canonical = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
        if let Some(found) = reports.iter().find(|report| {
            std::fs::canonicalize(&report.path)
                .map(|candidate| candidate == canonical)
                .unwrap_or(false)
        }) {
            let mut found = found.clone();
            found.source = RuntimeSource::Manual;
            return found;
        }
        let managed = canonical.starts_with(cache.root());
        report_for(&canonical, RuntimeSource::Manual, managed)
    });

    let compatible_count = reports
        .iter()
        .filter(|report| report.is_compatible())
        .count();
    if compatible_count == 0 {
        problems.push(format!(
            "no Java {} runtime was found; choose java.exe manually or install managed Temurin",
            target_java_major()
        ));
    }

    RuntimeDiscoveryReport {
        target_java_major: target_java_major(),
        target_minecraft_version: target_minecraft_version().to_owned(),
        candidates: reports,
        saved: saved_report,
        managed_root: managed.root,
        managed_bytes: managed.total_bytes,
        managed: managed.runtimes,
        problems,
        manual_hint: manual_hint(),
    }
}

fn push_candidate(
    candidates: &mut Vec<(PathBuf, RuntimeSource, bool)>,
    seen: &mut HashSet<String>,
    path: PathBuf,
    source: RuntimeSource,
    managed: bool,
) {
    let key = path_key(&path);
    if seen.insert(key) {
        candidates.push((path, source, managed));
    }
}

fn path_key(path: &Path) -> String {
    let text = path.display().to_string();
    if cfg!(windows) {
        text.to_ascii_lowercase()
    } else {
        text
    }
}

fn java_binary(root: &Path) -> PathBuf {
    if cfg!(windows) {
        root.join("bin").join("java.exe")
    } else {
        root.join("bin").join("java")
    }
}

fn java_home_candidates() -> Vec<PathBuf> {
    let Some(home) = std::env::var_os("JAVA_HOME") else {
        return Vec::new();
    };
    let home = PathBuf::from(home);
    if home.ends_with("bin") {
        vec![if cfg!(windows) {
            home.join("java.exe")
        } else {
            home.join("java")
        }]
    } else {
        vec![java_binary(&home)]
    }
}

fn path_candidates() -> Vec<PathBuf> {
    let Some(path) = std::env::var_os("PATH") else {
        return Vec::new();
    };
    std::env::split_paths(&path)
        .map(|entry| {
            if cfg!(windows) {
                entry.join("java.exe")
            } else {
                entry.join("java")
            }
        })
        .filter(|candidate| candidate.is_file())
        .take(8)
        .collect()
}

fn common_roots() -> Vec<(PathBuf, usize)> {
    let mut roots = Vec::new();
    if cfg!(windows) {
        for variable in ["ProgramFiles", "ProgramFiles(x86)", "LOCALAPPDATA"] {
            if let Some(base) = std::env::var_os(variable) {
                let base = PathBuf::from(base);
                let programs = if variable == "LOCALAPPDATA" {
                    base.join("Programs")
                } else {
                    base
                };
                for vendor in [
                    "Eclipse Adoptium",
                    "Java",
                    "Microsoft",
                    "Amazon Corretto",
                    "Zulu",
                    "BellSoft",
                ] {
                    roots.push((programs.join(vendor), 2));
                }
            }
        }
        if let Some(appdata) = std::env::var_os("APPDATA") {
            roots.push((
                PathBuf::from(appdata).join(".minecraft").join("runtimes"),
                4,
            ));
        }
    } else if cfg!(target_os = "macos") {
        roots.push((PathBuf::from("/Library/Java/JavaVirtualMachines"), 4));
        if let Some(home) = std::env::var_os("HOME") {
            let home = PathBuf::from(home);
            roots.push((home.join("Library/Java/JavaVirtualMachines"), 4));
            roots.push((home.join(".minecraft").join("runtimes"), 4));
        }
    } else {
        roots.push((PathBuf::from("/usr/lib/jvm"), 2));
        roots.push((PathBuf::from("/usr/java"), 2));
        if let Some(home) = std::env::var_os("HOME") {
            roots.push((PathBuf::from(home).join(".minecraft").join("runtimes"), 4));
        }
    }
    roots
        .into_iter()
        .filter(|(root, _)| root.is_dir())
        .collect()
}

fn scan_for_java(root: &Path, max_depth: usize, problems: &mut Vec<String>) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut queue: Vec<(PathBuf, usize)> = vec![(root.to_path_buf(), 0)];
    let mut visited = 0usize;
    while let Some((folder, depth)) = queue.pop() {
        if visited >= MAX_SCAN_ENTRIES || found.len() >= 8 {
            break;
        }
        visited += 1;
        let executable = java_binary(&folder);
        if executable.is_file() {
            found.push(executable);
            continue;
        }
        if depth >= max_depth {
            continue;
        }
        let entries = match std::fs::read_dir(&folder) {
            Ok(entries) => entries,
            Err(_) => continue,
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let metadata = match std::fs::symlink_metadata(&path) {
                Ok(metadata) => metadata,
                Err(error) => {
                    problems.push(format!("cannot inspect {}: {error}", path.display()));
                    continue;
                }
            };
            if metadata.file_type().is_symlink() {
                continue;
            }
            if metadata.is_dir() {
                queue.push((path, depth + 1));
            }
        }
    }
    found
}

fn manual_hint() -> String {
    if cfg!(windows) {
        "Choose the java.exe inside a Java 21 installation (for example C:\\Program Files\\Eclipse Adoptium\\jdk-21...\\bin\\java.exe), or install managed Temurin."
            .to_owned()
    } else {
        "Choose the java executable from a Java 21 installation, or install managed Temurin."
            .to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::RuntimeStatus;

    #[test]
    fn path_keys_are_case_insensitive_only_on_windows() {
        let left = path_key(Path::new("C:\\Java\\bin\\java.exe"));
        let right = path_key(Path::new("c:\\java\\BIN\\JAVA.EXE"));
        if cfg!(windows) {
            assert_eq!(left, right);
        } else {
            assert_ne!(left, right);
        }
    }

    #[test]
    fn missing_saved_runtime_is_reported_not_hidden() {
        let dir = std::env::temp_dir().join(format!(
            "furybuilder-runtime-discovery-{}",
            std::process::id()
        ));
        let cache = ManagedRuntimeCache::new(dir.join("managed"));
        let saved = dir.join("does-not-exist").join("java.exe");
        let report = discover(&cache, Some(&saved));
        let saved_report = report.saved.expect("saved report");
        assert_eq!(saved_report.status, RuntimeStatus::Unreadable);
        assert!(saved_report.error.is_some());
    }
}
