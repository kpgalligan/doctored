//! Type, shape and fill layers on a Grayscale document (#547 phase 6a-i,
//! `designs/grayscale-mode.md`): the greying of an artwork
//! description's colours and the fold of its rendered pixels.
//!
//! # Why the RGB renderer, unchanged
//!
//! A Gray document's storage is R = G = B in the twin (`gray_document`'s
//! module doc). An artwork description whose every colour is neutral renders
//! through the existing RGB renderer (`artwork_render`, the host's text
//! layout) in the twin and lands on equal channels, so there is no second
//! renderer. What makes a colour neutral is decided once, when the host
//! places the description ([`gray_artwork_colors`], asked by
//! `RasterDocument.grayArtwork`), as a layer style's colours are
//! (`gray_style`), so the stored description, `get_document`, the `.rz` body
//! and the render agree.
//!
//! # The rules
//!
//! * **An sRGB colour** (a run's, a shape's fill or stroke, a fill layer's
//!   colour or gradient stop) with r = g = b stays verbatim; any other
//!   becomes `[v, v, v]`, `v` the sRGB value of its luminance
//!   (`gray_style::srgb_gray`, Mode ▸ Grayscale's rule through the built-in
//!   sRGB profile), unrounded.
//! * **A linear colour** (a 32-bit layer's) with r = g = b stays; any other
//!   becomes `[Y, Y, Y]`, `Y` its luminance through sRGB's matrix
//!   (`gray_style::linear_gray`, extended range kept).
//! * **A pattern** fill's preset stays; a custom tile's non-neutral texels
//!   are greyed through the twin, alpha kept (`gray_style::grey_pattern`, a
//!   Pattern Overlay's edit rule).
//!
//! Neutral input is a fixed point, so re-greying a gray description changes
//! nothing.
//!
//! # The fold at publication
//!
//! `artwork_unlocked` passes every image a Gray document publishes through
//! [`fold_artwork`]: an image whose every pixel is neutral (and, at 8 bits,
//! whose gray and alpha samples are already on the ×257 grid) is published
//! as it is, with no allocation; any other is re-encoded by
//! `gray_convert::gray_image` with `keep_neutral` — each coloured pixel by
//! the Mode rule through the twin (`GrayPolicy::luminance_code` at 8 and 16
//! bits, `linear_luminance` at 32), each neutral pixel kept (snapped to the
//! grid at 8 bits), alpha on the grid at 8 bits. A colour glyph (an
//! intrinsic RGBA8 bitmap) and a coloured tile in an opened file are the
//! pixels it catches, and an 8-bit Gray artwork layer obeys the document's
//! grid (`gray_document`'s "every conversion into it rounds to that grid").

use serde_json::{Map, Value};

use crate::gray_document::{GrayDepth, GrayPolicy};
use crate::job_control::JobContext;
use crate::precision_document::PrecisionDocument;
use crate::precision_image::{PrecisionImage, PrecisionRun};

/// The most triples one list of the request holds.
const MAX_COLOURS: usize = 1024;

const NOT_GRAY: &str = "Grayscale artwork colours need a Grayscale document";
const BAD_TRIPLES: &str =
    "Grayscale artwork colours must be at most 1024 [r, g, b] triples of finite numbers (srgb 0–1)";

/// One request list as triples; `unit` bounds each value to 0…1 (srgb).
fn triples(value: &Value, unit: bool) -> Result<Vec<[f64; 3]>, String> {
    let list = value.as_array().ok_or(BAD_TRIPLES)?;
    if list.len() > MAX_COLOURS {
        return Err(BAD_TRIPLES.into());
    }
    list.iter()
        .map(|entry| {
            let entry = entry
                .as_array()
                .filter(|e| e.len() == 3)
                .ok_or(BAD_TRIPLES)?;
            let mut out = [0.0; 3];
            for (slot, v) in out.iter_mut().zip(entry) {
                let v = v
                    .as_f64()
                    .filter(|v| v.is_finite() && (!unit || (0.0..=1.0).contains(v)))
                    .ok_or(BAD_TRIPLES)?;
                *slot = v;
            }
            Ok(out)
        })
        .collect()
}

fn neutral(rgb: [f64; 3]) -> bool {
    rgb[0] == rgb[1] && rgb[1] == rgb[2]
}

/// `dr_precision_doc_gray_artwork_colors`' answer (module doc, "The
/// rules"): the request's `srgb`, `linear` and `pattern` keys greyed, each
/// answered under the same key. Refuses a document that is not Gray, a
/// request that is not a JSON object, an unknown key, a list over
/// [`MAX_COLOURS`] or a malformed triple, and a malformed pattern with its
/// own parser's sentence.
pub(crate) fn gray_artwork_colors(
    document: &PrecisionDocument,
    request: &str,
) -> Result<String, String> {
    let policy = document.gray_policy().ok_or(NOT_GRAY)?;
    let value: Value = serde_json::from_str(request)
        .map_err(|e| format!("invalid Grayscale artwork colour request: {e}"))?;
    let object = value
        .as_object()
        .ok_or("invalid Grayscale artwork colour request: the request must be a JSON object")?;
    for key in object.keys() {
        if !matches!(key.as_str(), "srgb" | "linear" | "pattern") {
            return Err(format!(
                "Grayscale artwork colour request has an unknown key \"{key}\""
            ));
        }
    }
    let mut out = Map::new();
    if let Some(list) = object.get("srgb") {
        let greyed: Vec<Value> = triples(list, true)?
            .into_iter()
            .map(|rgb| {
                let rgb = if neutral(rgb) {
                    rgb
                } else {
                    [crate::gray_style::srgb_gray(rgb); 3]
                };
                Value::from(rgb.to_vec())
            })
            .collect();
        out.insert("srgb".into(), Value::Array(greyed));
    }
    if let Some(list) = object.get("linear") {
        let mut greyed = Vec::new();
        for rgb in triples(list, false)? {
            let rgb = if neutral(rgb) {
                rgb
            } else {
                [crate::gray_style::linear_gray(rgb)
                    .ok_or("the built-in sRGB profile has no matrix model")?; 3]
            };
            greyed.push(Value::from(rgb.to_vec()));
        }
        out.insert("linear".into(), Value::Array(greyed));
    }
    if object.contains_key("pattern") {
        let pattern = crate::style_pattern_fill::parse_pattern(
            object,
            "request",
            crate::style::Strictness::Strict,
        )?;
        let greyed = crate::gray_style::grey_pattern(&pattern, policy.twin(), policy, false)?;
        out.insert(
            "pattern".into(),
            crate::style_pattern_fill::pattern_value(&greyed),
        );
    }
    Ok(Value::Object(out).to_string())
}

/// `image`, published on a Gray document under `policy`, folded to its
/// gray (module doc, "The fold at publication"): `None` when every pixel is
/// R = G = B and, at 8 bits, its gray and alpha samples are multiples of
/// 257 — the image is then published as it is; else a new image.
pub(crate) fn fold_artwork(
    image: &PrecisionImage,
    policy: &GrayPolicy,
    job: JobContext<'_>,
) -> Result<Option<PrecisionImage>, String> {
    let (w, h) = image.dimensions();
    let eight = policy.depth() == GrayDepth::Eight;
    let mut folded = true;
    'rows: for y in 0..h {
        for (_, run) in image.row_segments(y, 0, w) {
            let gray = match run {
                PrecisionRun::Rgba16(run) => run.iter().all(|p| {
                    p[0] == p[1] && p[1] == p[2] && (!eight || (p[0] % 257 == 0 && p[3] % 257 == 0))
                }),
                PrecisionRun::Rgba32(run) => run.iter().all(|p| p[0] == p[1] && p[1] == p[2]),
            };
            if !gray {
                folded = false;
                break 'rows;
            }
        }
        // Cancellation only: `gray_image` below reports this job's progress from 0.
        job.check()?;
    }
    if folded {
        return Ok(None);
    }
    crate::gray_convert::gray_image(image, policy.twin(), policy, true, job).map(Some)
}

/// `image`, a result `doc` is about to publish, folded to its gray when `doc`
/// is a Gray document (#547 phase 6b: the neural, generative and Merge to
/// HDR results, and since the close a placed image's rendered pixels, whose
/// colour comes from outside the document): `image`
/// unchanged on any other document or when [`fold_artwork`] finds it
/// already gray, else the folded image.
pub(crate) fn folded_result(
    doc: &PrecisionDocument,
    image: PrecisionImage,
    job: JobContext<'_>,
) -> Result<PrecisionImage, String> {
    let Some(policy) = doc.gray_policy() else {
        return Ok(image);
    };
    Ok(fold_artwork(&image, policy, job)?.unwrap_or(image))
}

/// The bytes [`folded_result`] may allocate for an image of `pixels` pixels
/// on `doc`: 0 off Gray, else the one image `gray_convert::gray_image`
/// builds — 8 B/px of u16 RGBA, 16 B/px of float RGBA at 32 bits.
pub(crate) fn fold_bytes(doc: &PrecisionDocument, pixels: u64) -> u64 {
    match doc.gray_policy() {
        None => 0,
        Some(policy) if policy.depth() == GrayDepth::ThirtyTwo => pixels.saturating_mul(16),
        Some(_) => pixels.saturating_mul(8),
    }
}
