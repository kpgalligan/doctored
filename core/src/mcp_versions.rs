//! The MCP protocol revisions Doctored speaks, and how a requested one is
//! answered. One file for two readers: `agent.rs`'s
//! `initialize` and the stdio proxy (`mcp-proxy/`), which includes this
//! file by `#[path]`, so the app and the proxy cannot disagree. It therefore
//! uses `std` alone and names nothing of this crate.

/// Protocol revisions this server implements identically (tools-only
/// servers are unchanged across them).
pub const SUPPORTED_VERSIONS: [&str; 3] = ["2024-11-05", "2025-03-26", "2025-06-18"];

/// The newest revision we know: the answer to a request for any other.
pub const LATEST_VERSION: &str = "2025-06-18";

/// The revision to answer a client that asked for `requested`: the same one
/// when supported, else the newest we know, per the spec's negotiation rule.
pub fn negotiate(requested: &str) -> &'static str {
    SUPPORTED_VERSIONS
        .iter()
        .find(|version| **version == requested)
        .copied()
        .unwrap_or(LATEST_VERSION)
}
