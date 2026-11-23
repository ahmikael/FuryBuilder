use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use crate::error::{unsupported, RuntimeError};
use crate::target::target_java_major;
use crate::types::{JavaRuntimeReport, RuntimeSource, RuntimeStatus};

const PROBE_TIMEOUT: Duration = Duration::from_secs(8);
const PROBE_OUTPUT_LIMIT: usize = 64 * 1024;

#[derive(Debug, Clone, Default)]
pub struct JavaProbe {
    pub version: Option<String>,
    pub major: Option<u32>,
    pub vendor: Option<String>,
    pub arch: Option<String>,
    pub vm: Option<String>,
    pub home: Option<String>,
}

impl JavaProbe {
    pub fn classify(&self) -> (RuntimeStatus, Vec<String>) {
        self.classify_for(target_java_major())
    }

    /// Classifies a probe against an explicit Java major. The project target is authoritative,
    /// so Java 17 can be ready for a project that requires 17 while this build's default
    /// profile still requires 21.
    pub fn classify_for(&self, expected: u32) -> (RuntimeStatus, Vec<String>) {
        let mut warnings = Vec::new();
        let Some(major) = self.major else {
            return (
                RuntimeStatus::NotJava,
                vec!["no Java version was reported".to_owned()],
            );
        };
        let status = match major.cmp(&expected) {
            std::cmp::Ordering::Equal => RuntimeStatus::Compatible,
            std::cmp::Ordering::Less => {
                warnings.push(format!(
                    "Java {major} is older than the required Java {expected}"
                ));
                RuntimeStatus::Outdated
            }
            std::cmp::Ordering::Greater => {
                warnings.push(format!(
                    "Java {major} is newer than the verified Java {expected}"
                ));
                RuntimeStatus::Newer
            }
        };
        if let Some(arch) = &self.arch {
            let lower = arch.to_ascii_lowercase();
            if lower == "x86" || lower == "i386" {
                warnings.push("32-bit Java cannot run the target Java worldgen".to_owned());
                return (RuntimeStatus::UnsupportedArch, warnings);
            }
        } else {
            warnings.push("Java did not report os.arch".to_owned());
        }
        (status, warnings)
    }
}

/// Executes a candidate exactly as a JVM would see it. A file name alone is never trusted.
pub fn probe_java(path: &Path) -> Result<JavaProbe, RuntimeError> {
    let metadata = std::fs::symlink_metadata(path)
        .map_err(|error| RuntimeError::Invalid(format!("{}: {error}", path.display())))?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(RuntimeError::Invalid(format!(
            "{} is not a regular file",
            path.display()
        )));
    }
    let mut command = Command::new(path);
    command
        .arg("-XshowSettings:properties")
        .arg("-version")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // Do not flash a console window for a read-only compatibility probe.
        command.creation_flags(0x0800_0000);
    }
    let mut child = command
        .spawn()
        .map_err(|error| RuntimeError::Invalid(format!("{}: {error}", path.display())))?;
    let deadline = Instant::now() + PROBE_TIMEOUT;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let mut output = Vec::new();
                if let Some(mut stdout) = child.stdout.take() {
                    let _ = stdout
                        .by_ref()
                        .take(PROBE_OUTPUT_LIMIT as u64)
                        .read_to_end(&mut output);
                }
                if let Some(mut stderr) = child.stderr.take() {
                    let _ = stderr
                        .by_ref()
                        .take(PROBE_OUTPUT_LIMIT as u64)
                        .read_to_end(&mut output);
                }
                if !status.success() {
                    return Err(RuntimeError::Invalid(format!(
                        "{} exited with {status}",
                        path.display()
                    )));
                }
                let text = String::from_utf8_lossy(&output);
                let probe = parse_java_probe(&text);
                if probe.major.is_none() {
                    return Err(unsupported(format!(
                        "{} did not report a Java version",
                        path.display()
                    )));
                }
                return Ok(probe);
            }
            Ok(None) if Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(RuntimeError::Invalid(format!(
                    "{} did not answer within {} seconds",
                    path.display(),
                    PROBE_TIMEOUT.as_secs()
                )));
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(25)),
            Err(error) => {
                let _ = child.kill();
                return Err(RuntimeError::Io(format!(
                    "cannot wait for {}: {error}",
                    path.display()
                )));
            }
        }
    }
}

/// Parses `-XshowSettings:properties -version` output. Kept pure for fixture tests.
pub fn parse_java_probe(text: &str) -> JavaProbe {
    let mut probe = JavaProbe::default();
    for line in text.lines() {
        let line = line.trim();
        if let Some((key, value)) = line.split_once('=') {
            let key = key.trim();
            let value = value.trim();
            if value.is_empty() {
                continue;
            }
            match key {
                "java.version" => probe.version = Some(value.to_owned()),
                "java.vendor" => probe.vendor = Some(value.to_owned()),
                "os.arch" => probe.arch = Some(value.to_owned()),
                "java.vm.name" => probe.vm = Some(value.to_owned()),
                "java.home" => probe.home = Some(value.to_owned()),
                _ => {}
            }
        }
    }
    if probe.version.is_none() {
        for line in text.lines() {
            let trimmed = line.trim();
            if let Some(rest) = trimmed.strip_prefix("openjdk version \"") {
                if let Some(version) = rest.split('"').next() {
                    probe.version = Some(version.to_owned());
                    break;
                }
            }
            if let Some(rest) = trimmed.strip_prefix("java version \"") {
                if let Some(version) = rest.split('"').next() {
                    probe.version = Some(version.to_owned());
                    break;
                }
            }
        }
    }
    probe.major = probe.version.as_deref().and_then(parse_java_major);
    probe
}

pub fn parse_java_major(version: &str) -> Option<u32> {
    let mut parts = version.trim().split(['.', '+', '-', '_']);
    let first = parts.next()?.parse::<u32>().ok()?;
    if first == 1 {
        // Legacy versioning: 1.8.0_401 is Java 8.
        parts.next()?.parse::<u32>().ok()
    } else {
        Some(first)
    }
}

pub fn report_for(path: &Path, source: RuntimeSource, managed: bool) -> JavaRuntimeReport {
    report_for_major(path, source, managed, target_java_major())
}

/// Probes a runtime and classifies it against an explicit project Java major.
pub fn report_for_major(
    path: &Path,
    source: RuntimeSource,
    managed: bool,
    expected_major: u32,
) -> JavaRuntimeReport {
    let text = path.display().to_string();
    match probe_java(path) {
        Ok(probe) => {
            let (status, warnings) = probe.classify_for(expected_major);
            JavaRuntimeReport {
                path: text,
                source,
                status,
                version: probe.version,
                major: probe.major,
                vendor: probe.vendor,
                arch: probe.arch,
                vm: probe.vm,
                managed,
                warnings,
                error: None,
            }
        }
        Err(error) => JavaRuntimeReport {
            path: text,
            source,
            status: RuntimeStatus::Unreadable,
            version: None,
            major: None,
            vendor: None,
            arch: None,
            vm: None,
            managed,
            warnings: Vec::new(),
            error: Some(error.to_string()),
        },
    }
}

/// Validates a manually selected executable against the project's Java major.
pub fn probe_compatible_for_major(
    path: &Path,
    source: RuntimeSource,
    managed: bool,
    expected_major: u32,
) -> Result<JavaRuntimeReport, RuntimeError> {
    let report = report_for_major(path, source, managed, expected_major);
    if report.status == RuntimeStatus::Compatible {
        Ok(report)
    } else {
        Err(unsupported(
            report
                .error
                .clone()
                .or_else(|| report.warnings.first().cloned())
                .unwrap_or_else(|| {
                    format!("Java at {} is {}", report.path, report.status.as_str())
                }),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_temurin_properties_and_versions() {
        let text = r#"
Property settings:
    java.home = C:\Program Files\Eclipse Adoptium\jdk-21.0.13.9-jre
    java.vendor = Eclipse Adoptium
    java.version = 21.0.13
    java.vm.name = OpenJDK 64-Bit Server VM
    os.arch = amd64

openjdk version "21.0.13" 2025-10-21 LTS
OpenJDK Runtime Environment Temurin-21.0.13+9 (build 21.0.13+9-LTS)
"#;
        let probe = parse_java_probe(text);
        assert_eq!(probe.version.as_deref(), Some("21.0.13"));
        assert_eq!(probe.major, Some(21));
        assert_eq!(probe.vendor.as_deref(), Some("Eclipse Adoptium"));
        assert_eq!(probe.arch.as_deref(), Some("amd64"));
        let (status, warnings) = probe.classify();
        assert_eq!(status, RuntimeStatus::Compatible);
        assert!(warnings.is_empty(), "{warnings:?}");
    }

    #[test]
    fn classifies_old_new_legacy_and_32_bit_java() {
        assert_eq!(parse_java_major("1.8.0_401"), Some(8));
        assert_eq!(parse_java_major("17.0.12"), Some(17));
        assert_eq!(parse_java_major("21"), Some(21));
        assert_eq!(parse_java_major("not-a-version"), None);

        let old = parse_java_probe("java.version = 17.0.12\nos.arch = amd64");
        assert_eq!(old.classify().0, RuntimeStatus::Outdated);

        let newer = parse_java_probe("java.version = 25\nos.arch = aarch64");
        assert_eq!(newer.classify().0, RuntimeStatus::Newer);

        let x86 = parse_java_probe("java.version = 21.0.1\nos.arch = x86");
        assert_eq!(x86.classify().0, RuntimeStatus::UnsupportedArch);

        let missing = parse_java_probe("openjdk version \"21.0.1\" 2024-10-15");
        assert_eq!(missing.major, Some(21));
    }
}
