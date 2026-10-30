//! Continuous encoding through the shared ICC forward curve evaluator.
//! Legacy ToneEncoder retains its 255 midpoint thresholds. This encoder samples
//! that SAME forward evaluator at 65537 encoded positions, searches the enclosing
//! linear-value interval, and interpolates its encoded coordinate. There is no
//! second curve formula or analytic curve inverter. The encoded interpolation
//! uncertainty is bounded by one grid interval (1/65536), before f32 rounding;
//! it is distinct from the existing 2e-6 compositor arithmetic gate.
//!
//! The interval search starts from a bucket index: `starts[c][b]` is
//! where the binary search over the whole table would cross `b / 4096`, so a
//! value in bucket `b` searches only `starts[b]..starts[b + 1]` and finds the
//! SAME upper index — every output bit is unchanged (`rgb_reference`, the
//! former body, is the test oracle) for 49 KB beside the 1.5 MB tables. It is
//! an index into the forward samples, not a LUT of encoded values and not an
//! inverse curve.
//!
//! `icc_precision.rs` specializes the shared forward curve evaluator for continuous encoding and
//! exact model identity, preserving the old byte thresholds and approximate equality.

use super::{Curve, MatrixTrc};
use crate::resources::Inventory;
use std::sync::Arc;

const INTERVALS: usize = 65536;
/// Buckets of the search index over [0, 1]. A power of two, so `v * 4096`
/// and `b / 4096` are exact in f64 and a bucket's bounds are exactly the
/// values `starts` was built from.
const INDEX_BUCKETS: usize = 4096;
/// The three forward tables (f64) and their three bucket indexes (u32).
pub const ENCODER_PAYLOAD_BYTES: u64 =
    ((INTERVALS + 1) * 3 * 8 + (INDEX_BUCKETS + 1) * 3 * 4) as u64;

impl Curve {
    /// Whether the curve's own parameters are usable, before any boundary or
    /// monotonicity check: a positive finite gamma, a non-decreasing sampled
    /// table of at least two entries, a `para` of a known type with finite
    /// parameters, a positive g, a positive a (types 1-4) and a non-negative
    /// linear-toe slope c (types 3-4). The per-curve gate of
    /// [`MatrixTrc::validate_precision`] and of `MatrixTrc::exposure_domain`.
    pub(crate) fn parameters_valid(&self) -> bool {
        match self {
            Curve::Identity => true,
            Curve::Gamma(g) => g.is_finite() && *g > 0.0,
            Curve::Sampled(values) => values.len() >= 2 && values.windows(2).all(|v| v[0] <= v[1]),
            Curve::Para {
                kind,
                g,
                a,
                b,
                c,
                d,
                e,
                f,
            } => {
                *kind <= 4
                    && [g, a, b, c, d, e, f].iter().all(|v| v.is_finite())
                    && *g > 0.0
                    && (*kind == 0 || *a > 0.0)
                    && (*kind < 3 || *c >= 0.0)
            }
        }
    }
}

impl MatrixTrc {
    /// Exact numeric model identity, independent of names/metadata and the
    /// legacy approximate-equivalence tolerances. No epsilon hides a typed edit.
    pub fn exact_eq(&self, other: &Self) -> bool {
        self.to_pcs == other.to_pcs
            && self.trc == other.trc
            && self.precision_compatible == other.precision_compatible
    }
    pub(crate) fn mark_precision_incompatible(&mut self) {
        self.precision_compatible = false;
    }
    pub fn validate_precision(&self) -> Result<(), String> {
        if !self.precision_compatible {
            return Err("high-depth profile has decreasing sampled tone curves".into());
        }
        for curve in &self.trc {
            if !curve.parameters_valid() {
                return Err("high-depth profile requires finite monotone RGB tone curves".into());
            }
            // Include parametric branch boundaries explicitly; a narrow falling
            // branch must not hide between neighboring encoded grid positions.
            if let Curve::Para { kind, a, b, d, .. } = curve {
                let split = if *kind < 3 && *kind > 0 { -b / a } else { *d };
                if *kind >= 3 && *d <= 1.0 && a * d.max(0.0) + b < 0.0 {
                    return Err("high-depth profile has an undefined parametric power base".into());
                }
                if (0.0..=1.0).contains(&split)
                    && curve.eval((split - 1e-12).max(0.0)) > curve.eval(split)
                {
                    return Err("high-depth profile has a decreasing parametric boundary".into());
                }
            }
            let mut previous = f64::NEG_INFINITY;
            for i in 0..=INTERVALS {
                let raw = curve.raw_eval(i as f64 / INTERVALS as f64);
                let value = raw.clamp(0.0, 1.0);
                if !raw.is_finite() || value < previous {
                    return Err(
                        "high-depth profile requires finite monotone forward tone curves".into(),
                    );
                }
                previous = value;
            }
        }
        Ok(())
    }
    fn payload_bytes(&self) -> u64 {
        self.trc
            .iter()
            .map(|curve| match curve {
                Curve::Sampled(v) => v.capacity() as u64 * 2,
                _ => 0,
            })
            .sum()
    }
}

#[derive(Clone)]
pub struct PrecisionToneEncoder {
    tables: Arc<[Vec<f64>; 3]>,
    /// `starts[c][b] = tables[c].partition_point(|&x| x < b / INDEX_BUCKETS)`
    /// for `b` in `0..=INDEX_BUCKETS`.
    starts: Arc<[Vec<u32>; 3]>,
}
impl PrecisionToneEncoder {
    pub fn new(destination: &MatrixTrc) -> Result<Self, String> {
        destination.validate_precision()?;
        let mut tables: [Vec<f64>; 3] = std::array::from_fn(|_| Vec::new());
        let mut starts: [Vec<u32>; 3] = std::array::from_fn(|_| Vec::new());
        for ((table, start), curve) in tables.iter_mut().zip(&mut starts).zip(&destination.trc) {
            table
                .try_reserve_exact(INTERVALS + 1)
                .map_err(|_| "could not allocate high-depth ICC encoding table")?;
            let mut previous = f64::NEG_INFINITY;
            for i in 0..=INTERVALS {
                let value = curve.eval(i as f64 / INTERVALS as f64);
                if !value.is_finite() || value < previous {
                    return Err("high-depth profile has a nonmonotone encoding curve".into());
                }
                table.push(value);
                previous = value;
            }
            start
                .try_reserve_exact(INDEX_BUCKETS + 1)
                .map_err(|_| "could not allocate high-depth ICC encoding table")?;
            for b in 0..=INDEX_BUCKETS {
                let bound = b as f64 / INDEX_BUCKETS as f64;
                start.push(table.partition_point(|&x| x < bound) as u32);
            }
        }
        Ok(Self {
            tables: Arc::new(tables),
            starts: Arc::new(starts),
        })
    }
    pub fn scratch_bytes() -> u64 {
        ENCODER_PAYLOAD_BYTES + 4096
    }
    /// `linear` encoded through the forward tables. The search for the first
    /// sample not below `v` is narrowed to `v`'s bucket: every sample before
    /// `starts[b]` is below `b / 4096 <= v`, every sample from
    /// `starts[b + 1]` on is at least `(b + 1) / 4096 > v` (or, in the last
    /// bucket, at least 1 >= v), so the narrowed search answers the index the
    /// whole-table search answers and the output bits are
    /// [`Self::rgb_reference`]'s. A NaN lands in bucket 0, whose start is 0
    /// (every sample is in [0, 1]), and answers 0 as before.
    pub fn rgb(&self, linear: [f64; 3]) -> [f32; 3] {
        std::array::from_fn(|c| {
            let values = &self.tables[c];
            let v = linear[c].clamp(0.0, 1.0);
            if v <= values[0] {
                return 0.0;
            }
            if v > values[INTERVALS] {
                return 1.0;
            }
            let starts = &self.starts[c];
            let b = ((v * INDEX_BUCKETS as f64) as usize).min(INDEX_BUCKETS - 1);
            let (first, last) = (starts[b] as usize, starts[b + 1] as usize);
            let upper = first + values[first..last].partition_point(|&x| x < v);
            if upper == 0 {
                return 0.0;
            }
            let low = upper - 1;
            let fraction = (v - values[low]) / (values[upper] - values[low]);
            ((low as f64 + fraction) / INTERVALS as f64) as f32
        })
    }
    /// The former [`Self::rgb`] verbatim — the binary search over the whole
    /// table — kept as the bit-exactness oracle for the bucket index
    /// (`tests/precision_icc_math.rs`). Not for production callers.
    #[doc(hidden)]
    pub fn rgb_reference(&self, linear: [f64; 3]) -> [f32; 3] {
        std::array::from_fn(|c| {
            let values = &self.tables[c];
            let v = linear[c].clamp(0.0, 1.0);
            if v <= values[0] {
                return 0.0;
            }
            if v > values[INTERVALS] {
                return 1.0;
            }
            let upper = values.partition_point(|&x| x < v);
            if upper == 0 {
                return 0.0;
            }
            let low = upper - 1;
            let fraction = (v - values[low]) / (values[upper] - values[low]);
            ((low as f64 + fraction) / INTERVALS as f64) as f32
        })
    }
    /// Each channel's slope of encoded value against linear light at diffuse
    /// white, read off the forward table's last interval: one grid step over
    /// the linear step it spans, `(1/INTERVALS) / (t[c][INTERVALS] −
    /// t[c][INTERVALS − 1])`. `display_encoding` continues the curve above 1
    /// at this slope, so the join is C1 for any profile. Infinite where the
    /// table is flat at its end (`display_encoding` refuses that).
    pub(crate) fn white_slope(&self) -> [f64; 3] {
        std::array::from_fn(|c| {
            let t = &self.tables[c];
            (1.0 / INTERVALS as f64) / (t[INTERVALS] - t[INTERVALS - 1])
        })
    }
    pub(crate) fn record_resources(&self, inventory: &mut Inventory) {
        inventory.object(&*self.tables, "precision_icc_tables", false);
        for table in &*self.tables {
            inventory.vec(table, "precision_icc_tables", false);
        }
        inventory.object(&*self.starts, "precision_icc_tables", false);
        for start in &*self.starts {
            inventory.vec(start, "precision_icc_tables", false);
        }
    }
}

/// An authored colour's LINEAR components in a destination's primaries: the
/// source's own curves, then the combined source → destination matrix. The
/// ONE statement of that rule — [`PrecisionTransform::linear`] routes through
/// it, a float document's sRGB Fill ink (`float_paint`) reads it there, and a
/// float document's layer-style colours (`style_float`) hold one directly.
#[derive(Clone)]
pub(crate) struct LinearInk {
    source: MatrixTrc,
    matrix: [[f64; 3]; 3],
}

impl LinearInk {
    pub(crate) fn new(source: &MatrixTrc, destination: &MatrixTrc) -> Self {
        Self {
            source: source.clone(),
            matrix: source.combined_with(destination),
        }
    }

    /// `profile`'s own decode and nothing else: an EXACT identity matrix, so
    /// each component is its curve's value bit for bit — the ink of a colour
    /// already in `profile`'s numbers (`float_paint`'s document space, and a
    /// style colour on a document `MatrixTrc::approx_eq` calls sRGB).
    pub(crate) fn decoding(profile: &MatrixTrc) -> Self {
        Self {
            source: profile.clone(),
            matrix: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
        }
    }

    /// `rgb` decoded through the source's curves, then carried into the
    /// destination's LINEAR components by the combined matrix.
    pub(crate) fn linear(&self, rgb: [f32; 3]) -> [f64; 3] {
        let linear = self.source.decode_rgb(rgb);
        self.matrix
            .map(|row| (0..3).map(|c| row[c] * linear[c]).sum())
    }

    /// `rgb` taken as LINEAR components already (no source decode), carried
    /// into the destination's linear components by the same combined matrix
    /// — a 32-bit float document's type and shape colour, whose stored
    /// numbers Photoshop 26.10.0 renders as linear light (H-raw;
    /// `artwork_render`).
    pub(crate) fn undecoded(&self, rgb: [f32; 3]) -> [f64; 3] {
        self.matrix
            .map(|row| (0..3).map(|c| row[c] * f64::from(rgb[c])).sum())
    }
}

/// Prepared conversion reusing MatrixTrc's continuous decoder/combined matrix
/// and the forward encoding table above. Normalized source alpha is external.
pub struct PrecisionTransform {
    ink: LinearInk,
    encoder: Option<PrecisionToneEncoder>,
}
impl PrecisionTransform {
    pub fn prepare_scratch_bytes(source: &MatrixTrc, destination: &MatrixTrc) -> u64 {
        source.payload_bytes()
            + if source.exact_eq(destination) {
                0
            } else {
                PrecisionToneEncoder::scratch_bytes()
            }
            + 4096
    }
    /// Only the DESTINATION is validated, and only when it is encoded into:
    /// the source is decoded through `Curve::eval`, which is finite
    /// and in [0, 1] for every model the parser builds, so a source keeps the
    /// parser's repairs (gamma 0 reads linear, a falling table its running
    /// maximum) exactly as the byte `Transform` decodes it. An exact identity
    /// encodes nothing and so validates nothing.
    pub fn new(source: &MatrixTrc, destination: &MatrixTrc) -> Result<Self, String> {
        Ok(Self {
            ink: LinearInk::new(source, destination),
            encoder: if source.exact_eq(destination) {
                None
            } else {
                Some(PrecisionToneEncoder::new(destination)?)
            },
        })
    }
    pub fn color(&self, rgb: [f32; 3]) -> [f32; 3] {
        let Some(encoder) = &self.encoder else {
            return rgb;
        };
        encoder.rgb(self.linear(rgb))
    }
    /// `rgb` decoded through the source's own curves, then carried into the
    /// destination's LINEAR components by the combined matrix — the half of
    /// [`Self::color`] before its encode. A float document's sRGB ink
    /// (`float_paint`) stops here: its samples are linear light.
    pub fn linear(&self, rgb: [f32; 3]) -> [f64; 3] {
        self.ink.linear(rgb)
    }
    /// `rgb` as linear source components carried into the destination's
    /// linear components by the combined matrix, with no decode — the H-raw
    /// colour of a float document's type and shape (`LinearInk::undecoded`).
    pub fn undecoded(&self, rgb: [f32; 3]) -> [f64; 3] {
        self.ink.undecoded(rgb)
    }
    pub(crate) fn record_resources(&self, inventory: &mut Inventory) {
        self.ink.source.record_resources(inventory);
        if let Some(encoder) = &self.encoder {
            encoder.record_resources(inventory);
        }
    }
    pub fn retained_bytes(&self) -> u64 {
        let mut inventory = Inventory::default();
        self.record_resources(&mut inventory);
        inventory.unique_bytes()
    }
}
