//! The coordinate bound an artwork outline is held to — a shared limit in a
//! leaf module that imports nothing from the crate (core/CLAUDE.md,
//! "Layers"). `artwork_coverage::outline` refuses an input point past it,
//! `outline_map` refuses a source point and a MAPPED point past it, and
//! `outline_envelope` refuses a box past it.

/// The largest coordinate magnitude an outline point may have, in pixels:
/// far past any raster an outline is drawn into, so a point past it is a
/// broken input or a broken map rather than geometry.
pub const MAX_OUTLINE_COORDINATE: f64 = 1e9;
