#![forbid(unsafe_code)]
//! `doctored-mcp`: the stdio MCP server inside Doctored's bundle (MC-3).
//! Everything is in the library; see `lib.rs`.

fn main() {
    doctored_mcp::run()
}
