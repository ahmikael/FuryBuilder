//! FuryBuilder's local MCP stdio bridge.
//!
//! The bridge is intentionally thin: it validates tool arguments, translates them into
//! requests against the authenticated local endpoint owned by the desktop process, and
//! formats the answers as MCP content. No project, world or job state lives here.

pub mod apps;
pub mod args;
pub mod capabilities;
pub mod context;
pub mod desktop;
pub mod prompts;
pub mod schema;
pub mod server;
pub mod tools;
