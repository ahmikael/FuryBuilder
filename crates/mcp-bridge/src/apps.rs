//! MCP Apps (SEP-1865, extension `io.modelcontextprotocol/ui`).
//!
//! The bridge only uses this extension as an optional presentation layer. Every `observe`
//! call still returns an ordinary PNG image and JSON metadata, so a host that does not
//! declare the extension keeps the same behavior.

use std::sync::LazyLock;

use rmcp::model::{Annotations, ClientCapabilities, MetaObject, Resource, ResourceContents, Role};
use serde::Serialize;
use serde_json::{json, Map, Value};

pub const APPS_EXTENSION_ID: &str = "io.modelcontextprotocol/ui";
pub const APPS_RESOURCE_MIME_TYPE: &str = "text/html;profile=mcp-app";
pub const OBSERVATION_APP_URI: &str = "ui://furybuilder/observation";
pub const OBSERVATION_APP_NAME: &str = "furybuilder-observation";
pub const OBSERVATION_APP_VERSION: &str = env!("CARGO_PKG_VERSION");
pub const OBSERVATION_TOOL_NAME: &str = "observe";

/// The version placeholder is replaced at startup so the discovery document and the
/// rendered view always report the version of the running binary.
pub static OBSERVATION_APP_HTML: LazyLock<String> = LazyLock::new(|| {
    include_str!("observation.html").replace("${OBSERVATION_APP_VERSION}", OBSERVATION_APP_VERSION)
});

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppsMode {
    Apps,
    Fallback,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppsSupportDecision {
    pub mode: &'static str,
    pub reason: &'static str,
    pub extension_advertised: bool,
    pub mime_type: &'static str,
}

impl AppsSupportDecision {
    pub fn to_json(&self) -> Value {
        serde_json::to_value(self).expect("apps decision is serializable")
    }
}

fn apps_settings(client_capabilities: Option<&ClientCapabilities>) -> Option<&Map<String, Value>> {
    let capabilities = client_capabilities?;
    let extensions = capabilities.extensions.as_ref()?;
    extensions.get(APPS_EXTENSION_ID)
}

/// Negotiates MCP Apps per connection. A missing extension or MIME type is a normal
/// fallback, never an error: the ordinary observation result is complete.
pub fn resolve_apps_support(
    client_capabilities: Option<&ClientCapabilities>,
    bridge_implements_apps: bool,
) -> AppsSupportDecision {
    let Some(settings) = apps_settings(client_capabilities) else {
        return AppsSupportDecision {
            mode: "fallback",
            reason: "client_did_not_declare_apps",
            extension_advertised: false,
            mime_type: APPS_RESOURCE_MIME_TYPE,
        };
    };
    let supports_mime_type = settings
        .get("mimeTypes")
        .and_then(Value::as_array)
        .is_some_and(|mime_types| {
            mime_types.iter().any(|value| {
                value
                    .as_str()
                    .is_some_and(|mime| mime.to_ascii_lowercase() == APPS_RESOURCE_MIME_TYPE)
            })
        });
    if !supports_mime_type {
        return AppsSupportDecision {
            mode: "fallback",
            reason: "client_mime_type_missing",
            extension_advertised: false,
            mime_type: APPS_RESOURCE_MIME_TYPE,
        };
    }
    if !bridge_implements_apps {
        return AppsSupportDecision {
            mode: "fallback",
            reason: "apps_not_implemented",
            extension_advertised: false,
            mime_type: APPS_RESOURCE_MIME_TYPE,
        };
    }
    AppsSupportDecision {
        mode: "apps",
        reason: "apps_negotiated",
        extension_advertised: true,
        mime_type: APPS_RESOURCE_MIME_TYPE,
    }
}

/// Restrictive metadata for the static observation view. The view needs no network, no
/// nested frames and no external assets; inline styles and scripts are enough to render
/// the PNG passed in the tool result.
pub fn observation_resource_meta() -> MetaObject {
    MetaObject::from(
        json!({
            "ui": {
                "csp": {
                    "connectDomains": [],
                    "resourceDomains": [],
                    "frameDomains": [],
                    "baseUriDomains": []
                },
                "prefersBorder": true
            }
        })
        .as_object()
        .cloned()
        .expect("resource metadata is an object"),
    )
}

pub fn observation_tool_meta() -> MetaObject {
    MetaObject::from(
        json!({
            "ui": {
                "resourceUri": OBSERVATION_APP_URI,
                "visibility": ["model", "app"]
            }
        })
        .as_object()
        .cloned()
        .expect("tool metadata is an object"),
    )
}

pub fn observation_resource() -> Resource {
    let annotations = Annotations::default()
        .with_audience(vec![Role::User])
        .with_priority(0.5);
    Resource::new(OBSERVATION_APP_URI, OBSERVATION_APP_NAME)
        .with_title("FuryBuilder observation preview")
        .with_description(
            "Interactive MCP Apps view for observe results. Hosts without MCP Apps keep the PNG image and JSON metadata returned by the observe tool.",
        )
        .with_mime_type(APPS_RESOURCE_MIME_TYPE)
        .with_meta(observation_resource_meta())
        .with_annotations(annotations)
}

pub fn observation_resource_contents(uri: &str) -> ResourceContents {
    ResourceContents::text(OBSERVATION_APP_HTML.as_str(), uri)
        .with_mime_type(APPS_RESOURCE_MIME_TYPE)
        .with_meta(observation_resource_meta())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn apps_capabilities() -> ClientCapabilities {
        let mut extensions = rmcp::model::ExtensionCapabilities::new();
        extensions.insert(
            APPS_EXTENSION_ID.to_owned(),
            json!({ "mimeTypes": [APPS_RESOURCE_MIME_TYPE] })
                .as_object()
                .cloned()
                .expect("settings"),
        );
        ClientCapabilities::builder()
            .enable_extensions_with(extensions)
            .build()
    }

    #[test]
    fn negotiates_only_when_the_client_declares_the_mime_type() {
        let decision = resolve_apps_support(Some(&apps_capabilities()), true);
        assert_eq!(decision.mode, "apps");
        assert!(decision.extension_advertised);

        let no_apps = ClientCapabilities::builder().build();
        assert_eq!(
            resolve_apps_support(Some(&no_apps), true).reason,
            "client_did_not_declare_apps"
        );
        assert_eq!(
            resolve_apps_support(Some(&apps_capabilities()), false).reason,
            "apps_not_implemented"
        );
    }

    #[test]
    fn the_view_is_self_contained() {
        assert!(OBSERVATION_APP_HTML.starts_with("<!DOCTYPE html>"));
        assert!(OBSERVATION_APP_HTML.contains("ui/initialize"));
        assert!(OBSERVATION_APP_HTML.contains("ui/notifications/tool-result"));
        assert!(OBSERVATION_APP_HTML.contains("ui/resource-teardown"));
        assert!(!OBSERVATION_APP_HTML.contains("https://"));
        assert!(!OBSERVATION_APP_HTML.contains("http://"));
        assert!(!OBSERVATION_APP_HTML.contains("<script src"));
        assert!(!OBSERVATION_APP_HTML.contains("<link"));
        assert!(!OBSERVATION_APP_HTML.contains("innerHTML"));
        assert!(!OBSERVATION_APP_HTML.contains("eval("));
        assert!(!OBSERVATION_APP_HTML.contains("new Function"));
        assert!(!OBSERVATION_APP_HTML.contains("document.write"));
    }

    #[test]
    fn the_resource_links_the_observe_tool() {
        let resource = observation_resource();
        assert_eq!(resource.uri, OBSERVATION_APP_URI);
        assert_eq!(resource.mime_type.as_deref(), Some(APPS_RESOURCE_MIME_TYPE));
        let contents = observation_resource_contents(OBSERVATION_APP_URI);
        match contents {
            ResourceContents::TextResourceContents {
                uri,
                mime_type,
                text,
                meta,
            } => {
                assert_eq!(uri, OBSERVATION_APP_URI);
                assert_eq!(mime_type.as_deref(), Some(APPS_RESOURCE_MIME_TYPE));
                assert_eq!(text, *OBSERVATION_APP_HTML);
                assert!(meta.is_some());
            }
            _ => panic!("observation resource is text"),
        }
    }
}
