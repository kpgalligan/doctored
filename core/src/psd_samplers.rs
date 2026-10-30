//! PSD image resource **1073**, Photoshop's colour samplers, both
//! directions and both depths.
//!
//! Layout, from Adobe's "Color samplers resource format" CORRECTED by
//! Photoshop-authored files (`tests/fixtures/psd_photoshop/color_samplers/`,
//! Photoshop 2025 26.10.0), all big-endian: a header of u32 version (1, 2 or
//! 3) and u32 count, then one record per sampler:
//!
//! | version | record | bytes |
//! | --- | --- | --- |
//! | 1 | i32 vertical, i32 horizontal (1/32 pixel), i16 colour space | 10 |
//! | 2 | f32 vertical, f32 horizontal (pixels), i16 colour space, i16 depth | 12 |
//! | 3 | u16 14, then the version-2 record | 14 |
//! | 3 | u32 per-sampler version, then the version-2 record (Adobe's text) | 16 |
//!
//! Three facts the published text gets wrong or leaves open, each MEASURED
//! against Photoshop rather than assumed:
//!
//! - The position is VERTICAL first, then horizontal ("horizontal and
//!   vertical" in the text). A sampler Photoshop placed at pixel (5, 7)
//!   stores 7.0 then 5.0.
//! - The stored position is the PIXEL INDEX: that sampler stores exactly
//!   5.0, not the pixel centre 5.5, and a written 5.0 reopens in Photoshop
//!   at x = 5. The reader floors, so a fractional value names the pixel it
//!   falls in.
//! - Version 1's position is NOT Adobe's 16.16 `Fixed`: Photoshop reads it
//!   in 1/32 pixel (a 16.16 value of 5 opened at x = 10240 = 5·65536/32),
//!   the same unit the obsolete resource 1038 stores.
//!
//! Photoshop 26.10.0 WRITES version 3 with 14-byte records — a u16 holding
//! 14 (the record's own size) where the text promises a u32 "version" —
//! colour space −1 ("Actual Color", the Info panel's default) and depth 8
//! at both file depths, and also writes resource 1038. The version-3 record
//! size is therefore taken as whichever of 14 and 16 makes `8 + count·size`
//! the body length.
//!
//! **Writer.** Version 2 (Photoshop reads it back at the same pixels, which
//! is the probe this module's tests were built from), with Photoshop's own
//! default colour space −1 and depth 8, so a Doctored sampler opens in
//! Photoshop exactly as one placed there. Written only when the list is
//! non-empty, so a sampler-free export is byte-for-byte the prior writer's.
//!
//! **Reader. Never a refusal.** Every odd case is one sentence in the
//! import report and the rest of the import is untouched: an unknown
//! version or a body length that matches no record size skips the resource;
//! a non-finite or off-canvas position drops that sampler; entries past
//! `doc_sampler::MAX_COLOR_SAMPLERS` are dropped; a colour space other than
//! −1 or 0 keeps the sampler and notes that Photoshop's per-sampler readout
//! mode is not kept (Doctored has one app-wide readout mode). Photoshop
//! stores no per-sampler sample size, so an imported sampler takes 1 and the
//! depth word is ignored; the export preflight discloses the reverse
//! (`psd_export_preflight`).
//!
//! `psd_samplers` is image resource 1073 both ways at 8 and 16 bits, its layout MEASURED against
//! Photoshop 2025 (`tests/fixtures/psd_photoshop/color_samplers/`): vertical then horizontal, the
//! pixel index, v1 in 1/32 px, Photoshop writing v3 with 14-byte records (u16 14 + the v2 record);
//! the reader never refuses (`Container::color_samplers`, `tag`, `psd_import16`, sentences in
//! `report`), and the export preflight's `sampler_sizes_lost` is the one loss.

use crate::doc_sampler::{ColorSampler, MAX_COLOR_SAMPLERS};

/// What one 1073 body imports as: the samplers kept, and the sentences the
/// import report carries.
pub(crate) struct SamplerImport {
    pub kept: Vec<ColorSampler>,
    pub losses: Vec<String>,
    pub observations: Vec<String>,
}

impl SamplerImport {
    fn skipped(reason: String) -> Self {
        Self {
            kept: Vec::new(),
            losses: vec![reason],
            observations: Vec::new(),
        }
    }
}

fn u16_at(b: &[u8], at: usize) -> u16 {
    u16::from_be_bytes([b[at], b[at + 1]])
}
fn u32_at(b: &[u8], at: usize) -> u32 {
    u32::from_be_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]])
}

/// Decodes a 1073 body for a `width × height` canvas. Never fails; see the
/// module doc for every sentence it can add.
pub(crate) fn decode(body: &[u8], width: u32, height: u32) -> SamplerImport {
    if body.len() < 8 {
        return SamplerImport::skipped(
            "Photoshop's color sampler resource is truncated; its color samplers are not imported."
                .into(),
        );
    }
    let version = u32_at(body, 0);
    let count = u32_at(body, 4) as u64;
    let rest = (body.len() - 8) as u64;
    // (record size, offset of the vertical position inside the record).
    let layout = match version {
        1 => Some((10u64, 0usize)),
        2 => Some((12, 0)),
        3 if count.checked_mul(14) == Some(rest) => Some((14, 2)),
        3 if count.checked_mul(16) == Some(rest) => Some((16, 4)),
        3 => None,
        _ => {
            return SamplerImport::skipped(format!(
                "Photoshop's color sampler resource has version {version}, which Doctored does not read; its color samplers are not imported."
            ))
        }
    };
    let Some((size, at)) = layout.filter(|(size, _)| count.checked_mul(*size) == Some(rest)) else {
        return SamplerImport::skipped(
            "Photoshop's color sampler resource has a length that matches none of its record layouts; its color samplers are not imported."
                .into(),
        );
    };
    let mut kept = Vec::new();
    let mut outside = 0usize;
    let mut beyond = 0usize;
    let mut modes = Vec::new();
    for index in 0..count as usize {
        let record = &body[8 + index * size as usize..8 + (index + 1) * size as usize];
        let (vertical, horizontal, space) = if version == 1 {
            // 1/32 pixel, measured (module doc).
            (
                f64::from(u32_at(record, 0) as i32) / 32.0,
                f64::from(u32_at(record, 4) as i32) / 32.0,
                u16_at(record, 8) as i16,
            )
        } else {
            (
                f64::from(f32::from_bits(u32_at(record, at))),
                f64::from(f32::from_bits(u32_at(record, at + 4))),
                u16_at(record, at + 8) as i16,
            )
        };
        let (x, y) = (horizontal.floor(), vertical.floor());
        let inside = x.is_finite()
            && y.is_finite()
            && (0.0..f64::from(width)).contains(&x)
            && (0.0..f64::from(height)).contains(&y);
        if !inside {
            outside += 1;
            continue;
        }
        if kept.len() == MAX_COLOR_SAMPLERS {
            beyond += 1;
            continue;
        }
        if space != -1 && space != 0 {
            modes.push((kept.len() + 1, space));
        }
        kept.push(ColorSampler {
            x: x as u32,
            y: y as u32,
            sample_size: 1,
        });
    }
    let mut losses = Vec::new();
    let mut observations = Vec::new();
    match outside {
        0 => {}
        1 => losses.push(
            "1 Photoshop color sampler lies outside the canvas (or has no finite position) and is not imported."
                .into(),
        ),
        n => losses.push(format!(
            "{n} Photoshop color samplers lie outside the canvas (or have no finite position) and are not imported."
        )),
    }
    if beyond > 0 {
        losses.push(format!(
            "This file has {} color samplers on the canvas; Doctored keeps the first {MAX_COLOR_SAMPLERS} and drops the other {beyond}.",
            MAX_COLOR_SAMPLERS + beyond
        ));
    }
    for (label, space) in modes {
        observations.push(format!(
            "Color sampler #{label} reads in Photoshop color mode {space}; Doctored keeps the sampler but not that readout mode — every sampler reads in the Info panel's one readout mode."
        ));
    }
    if !kept.is_empty() {
        observations.push(format!(
            "{} color sampler{} imported at a point (1 × 1) sample size; Photoshop stores no per-sampler sample size.",
            kept.len(),
            if kept.len() == 1 { " is" } else { "s are" }
        ));
    }
    SamplerImport {
        kept,
        losses,
        observations,
    }
}

/// The 1073 body for `s`, or an empty Vec when there is nothing to write:
/// version 2, each record f32 vertical, f32 horizontal (the pixel index),
/// colour space −1 and depth 8 — Photoshop's own values for a default
/// sampler (module doc).
pub(crate) fn encode(s: &[ColorSampler]) -> Vec<u8> {
    if s.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::with_capacity(8 + 12 * s.len());
    out.extend_from_slice(&2u32.to_be_bytes());
    out.extend_from_slice(&(s.len() as u32).to_be_bytes());
    for sampler in s {
        out.extend_from_slice(&(sampler.y as f32).to_be_bytes());
        out.extend_from_slice(&(sampler.x as f32).to_be_bytes());
        out.extend_from_slice(&(-1i16).to_be_bytes());
        out.extend_from_slice(&8i16.to_be_bytes());
    }
    out
}
