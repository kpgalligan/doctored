//! The Modify Selection radius limit both depths share (a leaf: it imports
//! nothing from the crate). The typed plane operations refuse a Feather,
//! Smooth, Grow, Shrink or Border radius, or a Stroke width, above it
//! (`precision_selection_plane::PlaneOperation::plan`); the byte exports
//! `dr_selection_feather` and `dr_selection_smooth` answer `false` above it
//! (`ffi_doc.rs`). Every app entry already offers at most 250 px, so the
//! limit refuses only a caller outside the UI and MCP.

/// The largest selection radius (or stroke width) in pixels. With it, a
/// feather's Gaussian is at most 501 taps, and the support-bounded window
/// (`precision_selection_support`) grows the selection by at most 250 px a
/// side.
pub(crate) const MAX_SELECTION_RADIUS: f32 = 250.0;
