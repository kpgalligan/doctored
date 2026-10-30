//! Perspective Crop: the request, its validation and the one
//! destination → source homography both depths resample through. A kernel:
//! the byte adapter is `doc_perspective_crop`, the typed one
//! `precision_perspective_crop`; this module names neither document.
//!
//! The request is `{"op":"perspective_crop","quad":[x0,y0,…,x3,y3],
//! "width":W,"height":H,"filter":"nearest|bilinear|bicubic|lanczos3"}`: the
//! quad is four SOURCE canvas points in the order TL, TR, BR, BL, and the
//! result is a `W × H` canvas whose rectangle `(0, 0, W, H)` maps onto that
//! quad. The map is `doc_perspective::solve_rect_to_quad((0, 0, W, H), quad)`
//! — the same solve, the same fold check and the same `SourceMap::projective`
//! walk Edit ▸ Transform ▸ Perspective uses — read as the INVERSE map of the
//! resample (destination pixel centre → source canvas point). There is no
//! short-circuit to Crop: an axis-aligned integer quad at its own size runs
//! the resampler like any other, and lands on the same samples because every
//! destination centre maps onto a source centre.
//!
//! # Admission
//!
//! Each adapter charges every destination plane it allocates (each raster
//! and mask on the new canvas, each saved channel) [`DESTINATION_COPIES`]
//! times, the largest contiguous source copy one resample binds while its
//! output is still contiguous, the document's cloned structure, and
//! [`PLAN_SLACK_BYTES`] (`plan_scratch`).
//!
//! Measured with `MallocLargeCache=0 DR_PC_CASE=<case> make t
//! F=perspective_crop_tests T=admission_measure A='--ignored --nocapture'`
//! (Release, one case per process, 3 runs each plus a baseline twin that
//! builds the fixture and runs nothing; E = `ri_interval_max_phys_footprint`
//! − `ri_phys_footprint` after `proc_reset_footprint_interval`). The fixture
//! is a 4000×3000 canvas, two full-canvas layers one of them masked, two
//! saved channels; the request a keystone quad to 3600×2600, bilinear.
//!
//! | case     | E, 3 runs (bytes)                       | max E       | ⌈1.25 × max E⌉ | plan, 1 copy | plan, 2 copies |
//! |----------|-----------------------------------------|-------------|----------------|--------------|----------------|
//! | baseline | 0, 0, 0                                 | 0           | 0              | n/a          | n/a            |
//! | byte     | 123,256,928 / 123,273,312 / 123,256,928 | 123,273,312 | 154,091,640    | 192,597,343  | 295,557,343    |
//! | u16      | 321,437,936 / 321,437,936 / 321,405,144 | 321,437,936 | 401,797,420    | 381,029,927  | 586,971,047    |
//! | float    | 642,449,888 / 642,466,272 / 642,449,888 | 642,466,272 | 803,082,840    | 701,669,932  | 1,057,371,052  |
//!
//! One copy of each destination missed the 16-bit and float margins, so
//! [`DESTINATION_COPIES`] was raised from 1 to 2: the existing factor, no new
//! constant. The default test
//! `perspective_crop_tests::plans_cover_the_measured_peaks` pins every plan
//! at or above ⌈1.25 × max E⌉ of its case.

use serde::Deserialize;

use crate::doc::MAX_PIXELS;
use crate::doc_perspective::solve_rect_to_quad;
use crate::precision_sampling::GeometryFilter;

/// Largest corner coordinate accepted, in canvas pixels: far past any canvas
/// the pixel budget allows, and small enough that the solve stays well inside
/// f64's exact-integer range.
pub const MAX_COORDINATE: f64 = 1e7;

/// Copies of each DESTINATION plane a plan charges. One copy is what the
/// resample stores; the measured 16-bit and float peaks (module doc) need
/// the second for their 1.25× margin, so it was raised from 1 to 2.
pub(crate) const DESTINATION_COPIES: u64 = 2;

/// Fixed slack a plan adds for allocator rounding and bookkeeping — the
/// 4 MiB every whole-document geometry plan carries.
pub(crate) const PLAN_SLACK_BYTES: u64 = 4 * 1024 * 1024;

/// The sentence for a corner that is not a finite canvas coordinate.
pub const CORNERS_INVALID: &str =
    "perspective crop corners must be finite and within 10,000,000 pixels of the canvas origin";
/// The sentence for a quad the fold check refuses.
pub const QUAD_INVALID: &str =
    "perspective crop corners must form a convex quadrilateral (not concave, crossed or collapsed)";
/// The sentence for an empty or oversized result.
pub const SIZE_INVALID: &str =
    "perspective crop width and height must be positive and the result at most 100,000,000 pixels";
/// The sentence for a document holding a layer with a retained source (a
/// camera RAW layer, the only kind that keeps one since 2026-10-10).
pub const SOURCE_LAYERS: &str =
    "Perspective Crop needs every layer as pixels: rasterize RAW layers first.";

/// A validated-on-use Perspective Crop request.
#[derive(Clone, Copy, Debug)]
pub struct PerspectiveCropRequest {
    /// TL, TR, BR, BL source canvas points, `x, y` each.
    pub quad: [f64; 8],
    pub width: u32,
    pub height: u32,
    pub filter: GeometryFilter,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Wire {
    op: String,
    quad: [f64; 8],
    width: u32,
    height: u32,
    filter: GeometryFilter,
}

impl PerspectiveCropRequest {
    /// Parses the JSON request; `Err` for malformed JSON, an unknown key or
    /// an `op` other than `perspective_crop`. Values are checked by
    /// [`Self::source_map`].
    pub fn parse(text: &str) -> Result<Self, String> {
        let wire: Wire = serde_json::from_str(text)
            .map_err(|e| format!("invalid perspective crop request: {e}"))?;
        if wire.op != "perspective_crop" {
            return Err("invalid perspective crop request: op must be perspective_crop".into());
        }
        Ok(Self {
            quad: wire.quad,
            width: wire.width,
            height: wire.height,
            filter: wire.filter,
        })
    }

    /// Whether a request text names this op, read before the shared geometry
    /// parser so its exhaustive enum never sees it.
    pub(crate) fn names(text: &str) -> bool {
        #[derive(Deserialize)]
        struct Tag<'a> {
            #[serde(borrow)]
            op: &'a str,
        }
        serde_json::from_str::<Tag<'_>>(text).is_ok_and(|tag| tag.op == "perspective_crop")
    }

    /// The row-major destination → source homography, after every value
    /// check: corners finite and within [`MAX_COORDINATE`], a positive size
    /// within `MAX_PIXELS`, and a quad that passes the fold check.
    pub(crate) fn source_map(&self) -> Result<[f64; 9], String> {
        if !self
            .quad
            .iter()
            .all(|v| v.is_finite() && v.abs() <= MAX_COORDINATE)
        {
            return Err(CORNERS_INVALID.into());
        }
        if self.width == 0
            || self.height == 0
            || u64::from(self.width) * u64::from(self.height) > MAX_PIXELS
        {
            return Err(SIZE_INVALID.into());
        }
        let rect = (0.0, 0.0, f64::from(self.width), f64::from(self.height));
        solve_rect_to_quad(rect, &self.quad)
            .map(|h| h.0)
            .ok_or_else(|| QUAD_INVALID.into())
    }

    /// The source → destination map (the visible-side inverse of
    /// [`Self::source_map`]'s homography), for the points that follow the
    /// picture: colour samplers. `None` when the inverse is not finite.
    pub(crate) fn destination_map(&self) -> Result<impl Fn(f64, f64) -> (f64, f64), String> {
        let forward = crate::doc_perspective::Homography(self.source_map()?);
        let g = forward.visible_inverse().ok_or(QUAD_INVALID)?;
        Ok(move |x: f64, y: f64| {
            let w = g[6] * x + g[7] * y + g[8];
            if w <= 0.0 {
                return (f64::NAN, f64::NAN);
            }
            (
                (g[0] * x + g[1] * y + g[2]) / w,
                (g[3] * x + g[4] * y + g[5]) / w,
            )
        })
    }

    /// The whole `canvas` at its own size: nothing would move.
    pub(crate) fn is_identity(&self, canvas: (u32, u32)) -> bool {
        let (w, h) = (f64::from(canvas.0), f64::from(canvas.1));
        (self.width, self.height) == canvas && self.quad == [0.0, 0.0, w, 0.0, w, h, 0.0, h]
    }

    /// Scale Effects: `sqrt(W·H / area(quad))`, the reciprocal of the factor
    /// `doc_perspective::perspective_layer` uses for content landing on a
    /// quad, since here the quad lands on the rectangle.
    pub(crate) fn style_factor(&self) -> f64 {
        let onto_quad = crate::style::quad_mean_scale(self.quad, self.width, self.height);
        if onto_quad > 0.0 && onto_quad.is_finite() {
            1.0 / onto_quad
        } else {
            1.0
        }
    }

    /// Destination pixel count.
    pub(crate) fn pixels(&self) -> u64 {
        u64::from(self.width) * u64::from(self.height)
    }
}

/// The scratch a plan charges: every destination plane times
/// [`DESTINATION_COPIES`], the largest transient source copy, the cloned
/// structure and [`PLAN_SLACK_BYTES`]. Saturating: an overflow is a plan no
/// limit admits.
pub(crate) fn plan_scratch(destinations: u64, largest_transient: u64, structure: u64) -> u64 {
    destinations
        .saturating_mul(DESTINATION_COPIES)
        .saturating_add(largest_transient)
        .saturating_add(structure)
        .saturating_add(PLAN_SLACK_BYTES)
}
