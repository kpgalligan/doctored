//! The regional render plan, for the tests that pin its admission and its
//! numbers. The app has no plan export — it calls only the render — so the
//! oracle is the pub Rust method the render's own admission produces:
//! `DrDocument::render_region_plan` / `PrecisionDocument::render_region_plan`
//! and `RegionalPlan::json`. A standalone `#[path]` module, not part of
//! `common/mod.rs`.
#![allow(dead_code)]

use doctored_core::doc::DrDocument;
use doctored_core::precision_document::PrecisionDocument;
use doctored_core::RegionRequest;
use serde_json::Value;

fn request((x, y, width, height): (u32, u32, u32, u32), preview_divisor: u32) -> RegionRequest {
    RegionRequest {
        x,
        y,
        width,
        height,
        preview_divisor,
    }
}

/// The 8-bit plan's JSON, or the refusal the render would raise.
pub fn byte(
    doc: &DrDocument,
    rect: (u32, u32, u32, u32),
    d: u32,
    max_output_bytes: u64,
) -> Result<Value, String> {
    doc.render_region_plan(request(rect, d), max_output_bytes)
        .map(|plan| plan.json())
}

/// The typed plan's JSON, or the refusal the render would raise.
pub fn typed(doc: &PrecisionDocument, rect: (u32, u32, u32, u32), d: u32) -> Result<Value, String> {
    doc.render_region_plan(request(rect, d))
        .map(|plan| plan.json())
}
