//! Unified component model shared by the project schema and the component manager.
//!
//! A project needs three kinds of external components: a Java runtime, Minecraft
//! client assets and an official server JAR. Each one can be managed by FuryBuilder,
//! linked manually, or declared as an explicit placeholder. This crate contains only
//! side-effect-free facts and DTOs; downloading, probing and filesystem work belong to
//! the component facade.
//!
//! The status vocabulary is deliberately small:
//!
//! - `ready` — the component exists, was verified and may be used;
//! - `missing` — it was never installed or selected;
//! - `unverified` — it exists but a checksum or version could not be confirmed;
//! - `incompatible` — it exists but does not fit the project target;
//! - `error` — inspection failed and the message explains why;
//! - `not_required` — the project does not need this kind of component.

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// The three component kinds a project can reference.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ComponentKind {
    Java,
    Assets,
    Server,
}

impl ComponentKind {
    pub fn as_str(self) -> &'static str {
        match self {
            ComponentKind::Java => "java",
            ComponentKind::Assets => "assets",
            ComponentKind::Server => "server",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            ComponentKind::Java => "Java runtime",
            ComponentKind::Assets => "Client assets",
            ComponentKind::Server => "Server JAR",
        }
    }
}

/// How a project obtains a component. Managed and manual components must carry a verified
/// identity; a placeholder is an explicit statement that the component is not available.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ComponentMode {
    Managed,
    Manual,
    Placeholder,
}

impl ComponentMode {
    pub fn as_str(self) -> &'static str {
        match self {
            ComponentMode::Managed => "managed",
            ComponentMode::Manual => "manual",
            ComponentMode::Placeholder => "placeholder",
        }
    }

    /// Only managed and manual components can ever be ready. A placeholder never claims
    /// a verified identity.
    pub fn can_be_ready(self) -> bool {
        matches!(self, ComponentMode::Managed | ComponentMode::Manual)
    }
}

/// Lifecycle state of one component.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ComponentState {
    Ready,
    Missing,
    Unverified,
    Incompatible,
    Error,
    NotRequired,
}

impl ComponentState {
    pub fn as_str(self) -> &'static str {
        match self {
            ComponentState::Ready => "ready",
            ComponentState::Missing => "missing",
            ComponentState::Unverified => "unverified",
            ComponentState::Incompatible => "incompatible",
            ComponentState::Error => "error",
            ComponentState::NotRequired => "not_required",
        }
    }

    pub fn is_ready(self) -> bool {
        self == ComponentState::Ready
    }

    /// States that prevent using the component without explicit repair or replacement.
    pub fn is_blocking(self) -> bool {
        matches!(
            self,
            ComponentState::Missing
                | ComponentState::Unverified
                | ComponentState::Incompatible
                | ComponentState::Error
        )
    }
}

/// Hash algorithm used to identify a component file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HashAlgorithm {
    Sha1,
    Sha256,
}

impl HashAlgorithm {
    pub fn as_str(self) -> &'static str {
        match self {
            HashAlgorithm::Sha1 => "sha1",
            HashAlgorithm::Sha256 => "sha256",
        }
    }

    pub fn hex_len(self) -> usize {
        match self {
            HashAlgorithm::Sha1 => 40,
            HashAlgorithm::Sha256 => 64,
        }
    }
}

/// A lowercase hexadecimal checksum with its algorithm.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ComponentHash {
    pub algorithm: HashAlgorithm,
    pub value: String,
}

impl ComponentHash {
    pub fn sha1(value: impl Into<String>) -> Self {
        Self {
            algorithm: HashAlgorithm::Sha1,
            value: value.into(),
        }
    }

    pub fn sha256(value: impl Into<String>) -> Self {
        Self {
            algorithm: HashAlgorithm::Sha256,
            value: value.into(),
        }
    }

    fn validate(&self) -> Result<(), ComponentError> {
        let expected = self.algorithm.hex_len();
        let valid = self.value.len() == expected
            && self
                .value
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte));
        if !valid {
            return Err(ComponentError::InvalidStatus(format!(
                "{} hash must be {expected} lowercase hexadecimal characters",
                self.algorithm.as_str()
            )));
        }
        Ok(())
    }
}

/// Status of one component as shown to the UI and MCP. Optional fields are omitted when
/// unknown; an empty string is never used to mean "not available".
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ComponentStatus {
    pub kind: ComponentKind,
    /// Whether the project needs this component for its current creation mode.
    pub required: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<ComponentMode>,
    pub state: ComponentState,
    /// Logical cache or selection id. Never an absolute path.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// Component version, for example a Java release or a client JAR id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    /// User-visible local path, resolved from the cache when the component is used.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hash: Option<ComponentHash>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bytes: Option<u64>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl ComponentStatus {
    pub fn new(kind: ComponentKind, state: ComponentState) -> Self {
        Self {
            kind,
            required: true,
            mode: None,
            state,
            id: None,
            version: None,
            path: None,
            hash: None,
            bytes: None,
            notes: Vec::new(),
            error: None,
        }
    }

    pub fn not_required(kind: ComponentKind) -> Self {
        Self {
            kind,
            required: false,
            mode: None,
            state: ComponentState::NotRequired,
            id: None,
            version: None,
            path: None,
            hash: None,
            bytes: None,
            notes: Vec::new(),
            error: None,
        }
    }

    /// Checks the invariants that every status DTO must uphold. Reports are built by the
    /// facade, so an invalid status is always a programming or storage error.
    pub fn validate(&self) -> Result<(), ComponentError> {
        for (field, value) in [
            ("id", self.id.as_deref()),
            ("version", self.version.as_deref()),
            ("path", self.path.as_deref()),
            ("error", self.error.as_deref()),
        ] {
            if let Some(value) = value {
                if value.trim().is_empty() {
                    return Err(ComponentError::InvalidStatus(format!(
                        "{} must not be empty",
                        field
                    )));
                }
                if value.chars().any(char::is_control) {
                    return Err(ComponentError::InvalidStatus(format!(
                        "{} must not contain control characters",
                        field
                    )));
                }
            }
        }
        if self.notes.len() > 64
            || self
                .notes
                .iter()
                .any(|note| note.trim().is_empty() || note.chars().any(char::is_control))
        {
            return Err(ComponentError::InvalidStatus(
                "notes must be non-empty single-line text, limited to 64 entries".to_owned(),
            ));
        }
        if self.bytes == Some(0) {
            return Err(ComponentError::InvalidStatus(
                "bytes must be positive when present".to_owned(),
            ));
        }
        if let Some(hash) = &self.hash {
            hash.validate()?;
        }
        if self.mode == Some(ComponentMode::Placeholder) {
            if self.state == ComponentState::Ready {
                return Err(ComponentError::InvalidStatus(
                    "a placeholder component cannot be ready".to_owned(),
                ));
            }
            if self.hash.is_some() || self.bytes.is_some() {
                return Err(ComponentError::InvalidStatus(
                    "a placeholder component cannot claim a checksum or size".to_owned(),
                ));
            }
        }
        if self.state == ComponentState::Ready {
            let verified = self.mode.is_some_and(ComponentMode::can_be_ready);
            if !verified {
                return Err(ComponentError::InvalidStatus(
                    "a ready component needs a managed or manual mode".to_owned(),
                ));
            }
            if self.hash.is_none() || self.path.is_none() {
                return Err(ComponentError::InvalidStatus(
                    "a ready component needs a checksum and a path".to_owned(),
                ));
            }
        }
        if self.state == ComponentState::Error && self.error.is_none() {
            return Err(ComponentError::InvalidStatus(
                "an error component needs an error message".to_owned(),
            ));
        }
        if self.state == ComponentState::NotRequired {
            if self.required {
                return Err(ComponentError::InvalidStatus(
                    "a not_required component must not be required".to_owned(),
                ));
            }
            if self.mode.is_some()
                || self.id.is_some()
                || self.version.is_some()
                || self.path.is_some()
                || self.hash.is_some()
                || self.bytes.is_some()
            {
                return Err(ComponentError::InvalidStatus(
                    "a not_required component must not carry an identity".to_owned(),
                ));
            }
        }
        Ok(())
    }
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ComponentError {
    #[error("invalid component status: {0}")]
    InvalidStatus(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assets_ready() -> ComponentStatus {
        ComponentStatus {
            kind: ComponentKind::Assets,
            required: true,
            mode: Some(ComponentMode::Managed),
            state: ComponentState::Ready,
            id: Some("java-client".to_owned()),
            version: Some("test-client".to_owned()),
            path: Some("components/assets/test-client/client.jar".to_owned()),
            hash: Some(ComponentHash::sha1("a".repeat(40))),
            bytes: Some(1024),
            notes: vec!["catalog rebuilt".to_owned()],
            error: None,
        }
    }

    #[test]
    fn serde_uses_stable_wire_names() {
        assert_eq!(
            serde_json::to_string(&ComponentKind::Assets).expect("kind"),
            "\"assets\""
        );
        assert_eq!(
            serde_json::to_string(&ComponentMode::Placeholder).expect("mode"),
            "\"placeholder\""
        );
        assert_eq!(
            serde_json::to_string(&ComponentState::NotRequired).expect("state"),
            "\"not_required\""
        );

        let value = serde_json::to_value(assets_ready()).expect("status");
        assert_eq!(value["kind"], "assets");
        assert_eq!(value["mode"], "managed");
        assert_eq!(value["state"], "ready");
        assert_eq!(value["hash"]["algorithm"], "sha1");
        assert!(value.get("error").is_none(), "empty optionals are omitted");
        let parsed: ComponentStatus = serde_json::from_value(value).expect("round trip");
        assert_eq!(parsed, assets_ready());
    }

    #[test]
    fn ready_status_needs_a_verified_source_checksum_and_path() {
        assets_ready().validate().expect("valid ready status");

        let mut placeholder = assets_ready();
        placeholder.mode = Some(ComponentMode::Placeholder);
        assert!(placeholder.validate().is_err());

        let mut unmanaged = assets_ready();
        unmanaged.mode = None;
        assert!(unmanaged.validate().is_err());

        let mut no_hash = assets_ready();
        no_hash.hash = None;
        assert!(no_hash.validate().is_err());

        let mut no_path = assets_ready();
        no_path.path = None;
        assert!(no_path.validate().is_err());
    }

    #[test]
    fn placeholder_status_is_explicitly_unverified() {
        let placeholder = ComponentStatus {
            kind: ComponentKind::Assets,
            required: true,
            mode: Some(ComponentMode::Placeholder),
            state: ComponentState::Unverified,
            id: None,
            version: None,
            path: None,
            hash: None,
            bytes: None,
            notes: vec!["textures unavailable".to_owned()],
            error: None,
        };
        placeholder.validate().expect("explicit placeholder");

        let mut claiming = placeholder;
        claiming.hash = Some(ComponentHash::sha1("b".repeat(40)));
        assert!(claiming.validate().is_err());
    }

    #[test]
    fn not_required_carries_no_identity() {
        ComponentStatus::not_required(ComponentKind::Server)
            .validate()
            .expect("server is not required");

        let mut invalid = ComponentStatus::not_required(ComponentKind::Server);
        invalid.mode = Some(ComponentMode::Placeholder);
        assert!(invalid.validate().is_err());

        let mut invalid = ComponentStatus::not_required(ComponentKind::Server);
        invalid.required = true;
        assert!(invalid.validate().is_err());
    }

    #[test]
    fn checksums_are_algorithm_specific() {
        ComponentHash::sha1("a".repeat(40))
            .validate()
            .expect("sha1");
        ComponentHash::sha256("a".repeat(64))
            .validate()
            .expect("sha256");
        assert!(ComponentHash::sha1("a".repeat(64)).validate().is_err());
        assert!(ComponentHash::sha256("A".repeat(64)).validate().is_err());
    }

    #[test]
    fn error_state_needs_a_message() {
        let mut status = ComponentStatus::new(ComponentKind::Java, ComponentState::Error);
        assert!(status.validate().is_err());
        status.error = Some("java.exe could not be executed".to_owned());
        status.mode = Some(ComponentMode::Manual);
        status.validate().expect("error with a message");
    }
}
