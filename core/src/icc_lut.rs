//! ICC **LUT** profiles: the v2 `mft1`/`mft2` and v4 `mAB `/`mBA ` tag types
//! parsed into one uniform [`LutPipeline`], and the [`ProofProfile`] a soft
//! proof, a gamut check or a LUT-destination conversion reads them through.
//! The matrix/TRC model stays in `icc_transform`; the intents, the black
//! point and the general src→dst pipeline are `icc_intent`.
//!
//! # What this parser models, and what it refuses
//!
//! A profile of class `scnr`, `mntr`, `prtr` or `spac` whose data colour
//! space is `RGB `, `GRAY` or `CMYK`, carrying at least one of `A2B0..2` /
//! `B2A0..2` as `mft1`, `mft2`, `mAB ` or `mBA ` — which is every CMYK press
//! profile Adobe ships, the ColorSync generic CMYK profile, and the v4 RGB
//! profiles under `/Library/Application Support/Adobe/Color/Profiles`. A
//! `GRAY` profile is modelled from its `kTRC` alone (`Y = kTRC(g)`,
//! chromaticity D50), which is what a Gray profile IS; a Gray profile with
//! only A2B tables is refused.
//!
//! Refused, each with a `refusal` sentence naming what the profile really
//! is so a host can say WHY rather than reporting a bare failure — checked in
//! the order class, data colour space, PCS, tables: a named-colour profile
//! (`nmcl`, a list of colours), a device link (`link`) or abstract profile
//! (`abst`, each a transform), any other non-pixel class; an `nCLR`
//! separation space (named by its colourant count, `4CLR` included) or any
//! other space but RGB, Gray and CMYK; an `mpet` (multiProcessElements)
//! typed `A2Bx`/`B2Ax`; a profile whose only tables are the floating-point
//! `D2B0..3` / `B2D0..3` (the first one found is named); and a CLUT whose
//! decoded samples would exceed [`MAX_CLUT_BYTES`]. These are the Phase 11C
//! boundary: each needs its own fixture set and its own reference-CMS
//! comparison before it can be believed, and half-believing one is how a
//! proof lies.
//!
//! # The PCS encodings
//!
//! A table's numbers are integers; what they MEAN depends on the tag type,
//! and getting this wrong shifts every proofed colour:
//!
//! | tag | PCS Lab | PCS XYZ |
//! |---|---|---|
//! | `mft1` (8-bit) | `L = 100·x`, `a = 255·x − 128` | u1Fixed15 |
//! | `mft2` (16-bit) | the v2 LEGACY encoding: `L = 100·x·65535/65280` | u1Fixed15 |
//! | `mAB `/`mBA ` | the v4 encoding: `L = 100·x`, `a = 255·x − 128` | u1Fixed15 |
//!
//! (`x` is the raw integer divided by its own maximum, so everything below
//! works in [0, 1] and one decode step turns it into real Lab or XYZ.) The
//! v2 legacy 16-bit Lab encoding puts L\* = 100 at 0xFF00 rather than
//! 0xFFFF, a 0.4 % scale — a quarter of an 8-bit code at mid grey, and
//! visible as a systematic lightness drift in a round trip that ignores it.
//!
//! # Interpolation
//!
//! Tetrahedral for a 3-input CLUT (the PCS→device direction of every RGB and
//! CMYK profile), multilinear otherwise. Tetrahedral is what every shipping
//! CMM uses on three inputs: it reads four corners instead of eight, and on
//! the near-diagonal ramps a grey axis is made of it is visibly smoother
//! than trilinear, whose eight-corner average rounds the neutral axis off.
//! A 4-input CLUT (CMYK→PCS) has no equally standard simplex decomposition,
//! so it takes the 16-corner multilinear read.
//!
//! `icc_lut` parses v2 `mft1`/`mft2` and v4 `mAB `/`mBA ` into ONE staged `LutPipeline` (curves /
//! 3x3 matrix / CLUT), holds the `ProofProfile` a proof or a LUT conversion reads, and owns the
//! PCS-encoding table (`mft1` and `mAB `/`mBA ` use the modern Lab encoding, `mft2` the v2 LEGACY
//! 16-bit one where L* = 100 sits at 0xFF00) plus the ONE Lab/XYZ pair and dE76. A 3-input CLUT is
//! TETRAHEDRAL and everything else multilinear; the simplex weights are the fractions THEMSELVES in
//! descending order, never their differences (the difference form is not exact at a cube corner,
//! which a 33-point press table hides and a two-point one exposes — that is how the bug was found).
//! Refused with a sentence naming what the profile really is, checked class → data space → PCS →
//! tables: a named-colour (`nmcl`, a list of colours), device-link or abstract class (a
//! transform) or any other non-pixel class, an `nCLR` space (named by its colourant count) or any
//! other space but RGB/Gray/CMYK, an `mpet`-typed `A2Bx`/`B2Ax`, a floating-point-only profile
//! (`missing_table_refusal` names the first `D2B0..3`/`B2D0..3` it finds through
//! `floating_point_tag` — shared with `icc_lookup`), a CLUT past `MAX_CLUT_BYTES`. A parsed profile
//! holding one direction only cannot be proofed: `ProofProfile::proof_refusal` names the missing
//! A2B/B2A table and its floating-point twin.

use std::sync::Arc;

use crate::icc::{self, IccBytes};
use crate::icc_intent::RenderingIntent;
use crate::icc_transform::Curve;

/// The largest decoded CLUT a profile may carry, in bytes of `f32` samples.
/// 64 MiB is four times the largest table Adobe ships (FOGRA39's 33³×4
/// `mft1`, 2.2 MiB decoded) and still small enough that a crafted profile
/// cannot make a proof allocate a gigabyte.
pub const MAX_CLUT_BYTES: u64 = 64 << 20;

/// The most device channels a LUT may take. Four is CMYK, the widest of the
/// three device spaces this build models; every `nCLR` separation space is
/// refused by its data colour space before any table is read.
pub const MAX_DEVICE_CHANNELS: usize = 4;

/// The ICC PCS white, D50, as the `wtpt` of every profile whose PCS is D50
/// already states it. Used for absolute-colorimetric scaling and BPC.
pub const D50: [f64; 3] = [0.9642, 1.0, 0.8249];

/// A profile's device colour space — the three this build models.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeviceSpace {
    Rgb,
    Gray,
    Cmyk,
}

impl DeviceSpace {
    pub fn channels(self) -> usize {
        match self {
            DeviceSpace::Rgb => 3,
            DeviceSpace::Gray => 1,
            DeviceSpace::Cmyk => 4,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            DeviceSpace::Rgb => "rgb",
            DeviceSpace::Gray => "gray",
            DeviceSpace::Cmyk => "cmyk",
        }
    }

    fn from_signature(sig: &[u8]) -> Option<DeviceSpace> {
        match sig {
            b"RGB " => Some(DeviceSpace::Rgb),
            b"GRAY" => Some(DeviceSpace::Gray),
            b"CMYK" => Some(DeviceSpace::Cmyk),
            _ => None,
        }
    }
}

/// Which profile connection space a table's PCS side speaks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PcsSpace {
    Lab,
    Xyz,
}

impl PcsSpace {
    pub fn name(self) -> &'static str {
        match self {
            PcsSpace::Lab => "lab",
            PcsSpace::Xyz => "xyz",
        }
    }
}

/// How one table encodes its PCS side (module doc's table).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PcsEncoding {
    /// `L = 100·x`, `a = b = 255·x − 128` — the v4 and the 8-bit v2 form.
    LabModern,
    /// The v2 legacy 16-bit form, where 100 sits at 0xFF00.
    LabLegacy16,
    /// u1Fixed15: `XYZ = 65535·x / 32768`.
    Xyz,
}

/// The scale a legacy 16-bit Lab table's full range sits at: L\* = 100 is
/// encoded as 0xFF00, not 0xFFFF.
const LEGACY_LAB_SCALE: f64 = 65535.0 / 65280.0;

impl PcsEncoding {
    /// One normalized PCS triple as real units — Lab (L\* in [0, 100], a/b
    /// in [−128, 127]) or XYZ.
    pub fn decode(self, x: [f64; 3]) -> [f64; 3] {
        match self {
            PcsEncoding::LabModern => [x[0] * 100.0, x[1] * 255.0 - 128.0, x[2] * 255.0 - 128.0],
            PcsEncoding::LabLegacy16 => [
                x[0] * 100.0 * LEGACY_LAB_SCALE,
                x[1] * 255.0 * LEGACY_LAB_SCALE - 128.0,
                x[2] * 255.0 * LEGACY_LAB_SCALE - 128.0,
            ],
            PcsEncoding::Xyz => x.map(|v| v * 65535.0 / 32768.0),
        }
    }

    /// The inverse of [`PcsEncoding::decode`], clamped into [0, 1] because a
    /// table index outside its own range describes nothing.
    pub fn encode(self, v: [f64; 3]) -> [f64; 3] {
        let raw = match self {
            PcsEncoding::LabModern => {
                [v[0] / 100.0, (v[1] + 128.0) / 255.0, (v[2] + 128.0) / 255.0]
            }
            PcsEncoding::LabLegacy16 => [
                v[0] / (100.0 * LEGACY_LAB_SCALE),
                (v[1] + 128.0) / (255.0 * LEGACY_LAB_SCALE),
                (v[2] + 128.0) / (255.0 * LEGACY_LAB_SCALE),
            ],
            PcsEncoding::Xyz => v.map(|c| c * 32768.0 / 65535.0),
        };
        raw.map(|c| {
            if c.is_finite() {
                c.clamp(0.0, 1.0)
            } else {
                0.0
            }
        })
    }

    fn space(self) -> PcsSpace {
        match self {
            PcsEncoding::Xyz => PcsSpace::Xyz,
            _ => PcsSpace::Lab,
        }
    }
}

// ------------------------------------------------------------ Lab and XYZ --

/// CIE Lab (D50) from PCS XYZ. The ONE Lab⇄XYZ pair in this module; `lab.rs`
/// is the 8-bit Lab READOUT and is a different question (its own
/// rounding, its own byte encoding), which is why it is not routed through
/// here.
pub fn xyz_to_lab(xyz: [f64; 3]) -> [f64; 3] {
    let f = |t: f64| {
        if t > 216.0 / 24389.0 {
            t.cbrt()
        } else {
            (24389.0 / 27.0 * t + 16.0) / 116.0
        }
    };
    let fx = f(xyz[0] / D50[0]);
    let fy = f(xyz[1] / D50[1]);
    let fz = f(xyz[2] / D50[2]);
    [116.0 * fy - 16.0, 500.0 * (fx - fy), 200.0 * (fy - fz)]
}

/// PCS XYZ (D50) from CIE Lab — the exact inverse of [`xyz_to_lab`].
pub fn lab_to_xyz(lab: [f64; 3]) -> [f64; 3] {
    let fy = (lab[0] + 16.0) / 116.0;
    let fx = fy + lab[1] / 500.0;
    let fz = fy - lab[2] / 200.0;
    let g = |t: f64| {
        let cube = t * t * t;
        if cube > 216.0 / 24389.0 {
            cube
        } else {
            (116.0 * t - 16.0) * 27.0 / 24389.0
        }
    };
    [g(fx) * D50[0], g(fy) * D50[1], g(fz) * D50[2]]
}

/// CIE76 ΔE\*ab between two Lab triples — the gamut rule's metric
/// (`soft_proof`), stated here because Lab lives here.
pub fn delta_e76(a: [f64; 3], b: [f64; 3]) -> f64 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
}

// --------------------------------------------------------------- the CLUT --

/// A multi-dimensional colour lookup table, decoded to normalized `f32`.
#[derive(Debug)]
pub struct Clut {
    /// Grid points per INPUT channel, most-significant first (channel 0 is
    /// the outermost index, as ICC specifies).
    grid: Vec<usize>,
    outputs: usize,
    data: Vec<f32>,
}

impl Clut {
    fn inputs(&self) -> usize {
        self.grid.len()
    }

    fn sample(&self, index: usize, out: usize) -> f64 {
        f64::from(self.data[index * self.outputs + out])
    }

    /// The table at `input` (each component already clamped to [0, 1]),
    /// writing `outputs` values into `dst`.
    fn eval(&self, input: &[f64], dst: &mut [f64]) {
        if self.inputs() == 3 {
            self.tetrahedral(input, dst);
        } else {
            self.multilinear(input, dst);
        }
    }

    /// Per-axis grid position: the lower index and the fraction above it.
    fn locate(&self, input: &[f64]) -> ([usize; MAX_DEVICE_CHANNELS], [f64; MAX_DEVICE_CHANNELS]) {
        let mut base = [0usize; MAX_DEVICE_CHANNELS];
        let mut frac = [0.0f64; MAX_DEVICE_CHANNELS];
        for (axis, &points) in self.grid.iter().enumerate() {
            let x = input[axis].clamp(0.0, 1.0) * (points - 1) as f64;
            let i = (x.floor() as usize).min(points.saturating_sub(2));
            base[axis] = i;
            frac[axis] = (x - i as f64).clamp(0.0, 1.0);
        }
        (base, frac)
    }

    /// Flat sample index for a corner given per-axis offsets of 0 or 1.
    fn corner(&self, base: &[usize; MAX_DEVICE_CHANNELS], offset: &[usize]) -> usize {
        let mut index = 0usize;
        for (axis, &points) in self.grid.iter().enumerate() {
            let position = (base[axis] + offset[axis]).min(points - 1);
            index = index * points + position;
        }
        index
    }

    /// The 2ⁿ-corner read. Used for every input count but three.
    fn multilinear(&self, input: &[f64], dst: &mut [f64]) {
        let (base, frac) = self.locate(input);
        let n = self.inputs();
        for slot in dst.iter_mut().take(self.outputs) {
            *slot = 0.0;
        }
        for mask in 0..(1usize << n) {
            let mut weight = 1.0f64;
            let mut offset = [0usize; MAX_DEVICE_CHANNELS];
            for (axis, slot) in offset.iter_mut().enumerate().take(n) {
                if mask >> axis & 1 == 1 {
                    *slot = 1;
                    weight *= frac[axis];
                } else {
                    weight *= 1.0 - frac[axis];
                }
            }
            if weight == 0.0 {
                continue;
            }
            let index = self.corner(&base, &offset[..n]);
            for (out, slot) in dst.iter_mut().enumerate().take(self.outputs) {
                *slot += weight * self.sample(index, out);
            }
        }
    }

    /// The four-corner simplex read for a 3-input table.
    ///
    /// The cube is split into six tetrahedra by the ORDERING of the three
    /// fractions. Each result walks the simplex's own corner path
    /// `v000 → ca → cb → v111` and adds the three differences weighted by the
    /// fractions in DESCENDING order:
    ///
    /// ```text
    /// f = v000 + w0·(ca − v000) + w1·(cb − ca) + w2·(v111 − cb)
    /// ```
    ///
    /// which expands to the barycentric form
    /// `v000·(1 − w0) + ca·(w0 − w1) + cb·(w1 − w2) + v111·w2`. The weights are
    /// the fractions THEMSELVES, not their differences: at (1, 1, 1) the
    /// difference form collapses to `v000 + v111 − cb`, which is not `v111`
    /// unless the table happens to be affine there — a systematic error small
    /// enough to hide inside a 33-point press table and obvious on a
    /// two-point one, which is why `icc_lut_tests` builds one.
    fn tetrahedral(&self, input: &[f64], dst: &mut [f64]) {
        let (base, frac) = self.locate(input);
        let (rx, ry, rz) = (frac[0], frac[1], frac[2]);
        let c000 = self.corner(&base, &[0, 0, 0]);
        let c111 = self.corner(&base, &[1, 1, 1]);
        // The two intermediate corners and the descending weights.
        let (ca, cb, w0, w1, w2) = if rx >= ry {
            if ry >= rz {
                (
                    self.corner(&base, &[1, 0, 0]),
                    self.corner(&base, &[1, 1, 0]),
                    rx,
                    ry,
                    rz,
                )
            } else if rx >= rz {
                (
                    self.corner(&base, &[1, 0, 0]),
                    self.corner(&base, &[1, 0, 1]),
                    rx,
                    rz,
                    ry,
                )
            } else {
                (
                    self.corner(&base, &[0, 0, 1]),
                    self.corner(&base, &[1, 0, 1]),
                    rz,
                    rx,
                    ry,
                )
            }
        } else if rz >= ry {
            (
                self.corner(&base, &[0, 0, 1]),
                self.corner(&base, &[0, 1, 1]),
                rz,
                ry,
                rx,
            )
        } else if rz >= rx {
            (
                self.corner(&base, &[0, 1, 0]),
                self.corner(&base, &[0, 1, 1]),
                ry,
                rz,
                rx,
            )
        } else {
            (
                self.corner(&base, &[0, 1, 0]),
                self.corner(&base, &[1, 1, 0]),
                ry,
                rx,
                rz,
            )
        };
        for (out, slot) in dst.iter_mut().enumerate().take(self.outputs) {
            let v000 = self.sample(c000, out);
            let va = self.sample(ca, out);
            let vb = self.sample(cb, out);
            *slot = v000 + w0 * (va - v000) + w1 * (vb - va) + w2 * (self.sample(c111, out) - vb);
        }
    }

    fn bytes(&self) -> u64 {
        self.data.len() as u64 * 4
    }
}

// ----------------------------------------------------------- the pipeline --

/// One stage of a LUT pipeline. Every ICC LUT tag type is a sequence of
/// these three, which is why one evaluator serves all four.
#[derive(Debug)]
enum Stage {
    /// One curve per channel, evaluated in place.
    Curves(Vec<Curve>),
    /// A 3×3 matrix with three offsets, row-major then the offsets.
    Matrix([f64; 12]),
    Clut(Clut),
}

/// A parsed `mft1`, `mft2`, `mAB ` or `mBA ` table as an ordered stage list,
/// evaluated in normalized [0, 1] on both sides. [`LutPipeline::pcs`] says
/// how the PCS side's numbers are encoded.
#[derive(Debug)]
pub struct LutPipeline {
    stages: Vec<Stage>,
    inputs: usize,
    outputs: usize,
    pcs: PcsEncoding,
}

impl LutPipeline {
    pub fn inputs(&self) -> usize {
        self.inputs
    }

    pub fn outputs(&self) -> usize {
        self.outputs
    }

    /// How this table's PCS side is encoded (module doc).
    pub fn pcs(&self) -> PcsEncoding {
        self.pcs
    }

    /// Conservative decoded size, for the scratch a pipeline declares.
    pub fn bytes(&self) -> u64 {
        self.stages
            .iter()
            .map(|stage| match stage {
                Stage::Clut(clut) => clut.bytes(),
                Stage::Curves(curves) => curves
                    .iter()
                    .map(|c| match c {
                        Curve::Sampled(v) => v.len() as u64 * 2,
                        _ => 32,
                    })
                    .sum(),
                Stage::Matrix(_) => 96,
            })
            .sum::<u64>()
            + 256
    }

    /// `Some(n)` when the pipeline is curves → ONE 3-input `n³` CLUT →
    /// curves, with no matrix and every curve linear ([`is_linear`]): its
    /// CLUT nodes are then the whole transform at `k / (n − 1)`, so a look
    /// can store them at their own grid (`icc_lookup`).
    pub(crate) fn plain_grid(&self) -> Option<usize> {
        let mut grid = None;
        for stage in &self.stages {
            match stage {
                Stage::Curves(curves) => {
                    if !curves.iter().all(is_linear) {
                        return None;
                    }
                }
                Stage::Matrix(_) => return None,
                Stage::Clut(clut) => {
                    let n = *clut.grid.first()?;
                    if grid.is_some() || clut.inputs() != 3 || clut.grid.iter().any(|&g| g != n) {
                        return None;
                    }
                    grid = Some(n);
                }
            }
        }
        grid
    }

    /// The largest grid count of the pipeline's 3-input CLUT, if it has one.
    pub(crate) fn clut_grid(&self) -> Option<usize> {
        self.stages.iter().find_map(|stage| match stage {
            Stage::Clut(clut) if clut.inputs() == 3 => clut.grid.iter().copied().max(),
            _ => None,
        })
    }

    /// Runs the pipeline. `input` must hold [`LutPipeline::inputs`] values in
    /// [0, 1]; the result holds [`LutPipeline::outputs`] of them.
    pub fn eval(&self, input: &[f64]) -> [f64; MAX_DEVICE_CHANNELS] {
        let mut buffer = [0.0f64; MAX_DEVICE_CHANNELS];
        let mut count = self.inputs.min(MAX_DEVICE_CHANNELS);
        for (slot, value) in buffer.iter_mut().zip(input.iter()).take(count) {
            *slot = if value.is_finite() {
                value.clamp(0.0, 1.0)
            } else {
                0.0
            };
        }
        for stage in &self.stages {
            match stage {
                Stage::Curves(curves) => {
                    for (slot, curve) in buffer.iter_mut().zip(curves.iter()).take(count) {
                        *slot = curve.eval(*slot);
                    }
                }
                Stage::Matrix(m) => {
                    if count >= 3 {
                        let v = [buffer[0], buffer[1], buffer[2]];
                        for i in 0..3 {
                            buffer[i] = (m[3 * i] * v[0]
                                + m[3 * i + 1] * v[1]
                                + m[3 * i + 2] * v[2]
                                + m[9 + i])
                                .clamp(0.0, 1.0);
                        }
                    }
                }
                Stage::Clut(clut) => {
                    let input: [f64; MAX_DEVICE_CHANNELS] = buffer;
                    let mut out = [0.0f64; MAX_DEVICE_CHANNELS];
                    clut.eval(&input[..clut.inputs()], &mut out);
                    buffer = out;
                    count = clut.outputs;
                }
            }
        }
        for slot in buffer.iter_mut() {
            *slot = if slot.is_finite() {
                slot.clamp(0.0, 1.0)
            } else {
                0.0
            };
        }
        buffer
    }
}

/// True for a curve that is the identity on [0, 1]: `Identity`, `Gamma(1)`,
/// or a sampled table whose entry `k` is `round(65535·k/(len − 1))` (what a
/// linear `mft1`/`mft2` input or output table decodes to). A parametric
/// curve is never taken as linear.
fn is_linear(curve: &Curve) -> bool {
    match curve {
        Curve::Identity => true,
        Curve::Gamma(g) => *g == 1.0,
        Curve::Sampled(table) => {
            let last = table.len().saturating_sub(1);
            last > 0
                && table.iter().enumerate().all(|(k, &v)| {
                    u64::from(v) == (65535 * k as u64 * 2 + last as u64) / (2 * last as u64)
                })
        }
        Curve::Para { .. } => false,
    }
}

// ------------------------------------------------------------ the profile --

/// A profile a proof, a gamut check or a LUT conversion can run through:
/// its device space, its media white, and its six possible tables.
///
/// Built by [`ProofProfile::parse`] from ANY profile bytes — unlike
/// `IccProfile`, which stores only what a DOCUMENT's pixels may be
/// interpreted in and therefore refuses Gray and CMYK outright.
#[derive(Debug)]
pub struct ProofProfile {
    name: String,
    device: DeviceSpace,
    pcs: PcsSpace,
    media_white: [f64; 3],
    /// A2B0, A2B1, A2B2 — device to PCS.
    a2b: [Option<Arc<LutPipeline>>; 3],
    /// B2A0, B2A1, B2A2 — PCS to device.
    b2a: [Option<Arc<LutPipeline>>; 3],
    /// A Gray profile's `kTRC`; `None` for every other device space.
    gray: Option<Curve>,
    /// The header's major version. Only the black-point rule reads it: a v4
    /// perceptual table lands on the spec's fixed perceptual black, a v2 one
    /// has no such reference (`icc_intent`).
    major_version: u8,
    /// True when the header class is `prtr` (an output device). Only the
    /// black-point rule reads it: Little CMS's `cmsDetectBlackPoint` asks an
    /// OUTPUT CMYK profile for its perceptual black at relative colorimetric,
    /// to discount ink limiting (`icc_intent`).
    output_class: bool,
}

impl ProofProfile {
    /// Parses any ICC profile into a proofing model. `Err` names the element
    /// that made it impossible — the sentence a host shows beside the file it
    /// chose.
    pub fn parse(bytes: &[u8]) -> Result<ProofProfile, String> {
        let icc = IccBytes::new(bytes).ok_or("the file is not an ICC profile")?;
        // Class before data space: a named-colour, device-link or abstract
        // profile is refused for what it IS, whatever colour space its header
        // happens to name (an abstract profile's is Lab, which would
        // otherwise be reported as an unsupported device space).
        if !icc.is_pixel_class() {
            return Err(match icc.class_name().as_str() {
                "nmcl" => "a named-colour profile (nmcl) is a list of colours, not a colour space"
                    .to_string(),
                "link" => "a device-link profile (link) describes a transform, not a colour space"
                    .to_string(),
                "abst" => "an abstract profile (abst) describes a transform, not a colour space"
                    .to_string(),
                class => format!("the profile class \"{class}\" is not supported"),
            });
        }
        let device = DeviceSpace::from_signature(icc.data_space())
            .ok_or_else(|| unmodelled_space_refusal(&icc))?;
        let pcs = match icc.pcs_space() {
            b"Lab " => PcsSpace::Lab,
            b"XYZ " => PcsSpace::Xyz,
            other => {
                return Err(format!(
                    "a profile connection space of \"{}\" is not supported",
                    String::from_utf8_lossy(other).trim_end()
                ))
            }
        };
        let name = icc.name();
        let media_white = icc.media_white().unwrap_or(D50);
        let major_version = icc.major_version();
        let output_class = icc.class_name() == "prtr";

        if device == DeviceSpace::Gray {
            let gray = icc
                .tag(b"kTRC")
                .and_then(icc::curve)
                .ok_or("a grey profile without a kTRC curve is not supported")?;
            return Ok(ProofProfile {
                name,
                device,
                pcs,
                media_white,
                a2b: [None, None, None],
                b2a: [None, None, None],
                gray: Some(gray),
                major_version,
                output_class,
            });
        }

        let mut a2b: [Option<Arc<LutPipeline>>; 3] = [None, None, None];
        let mut b2a: [Option<Arc<LutPipeline>>; 3] = [None, None, None];
        let mut budget = MAX_CLUT_BYTES;
        for table in 0..3 {
            let forward = [b"A2B0", b"A2B1", b"A2B2"][table];
            let inverse = [b"B2A0", b"B2A1", b"B2A2"][table];
            if let Some(data) = icc.tag(forward) {
                a2b[table] = Some(Arc::new(parse_table(
                    data,
                    device.channels(),
                    3,
                    pcs,
                    Direction::ToPcs,
                    &mut budget,
                )?));
            }
            if let Some(data) = icc.tag(inverse) {
                b2a[table] = Some(Arc::new(parse_table(
                    data,
                    3,
                    device.channels(),
                    pcs,
                    Direction::FromPcs,
                    &mut budget,
                )?));
            }
        }
        if a2b.iter().chain(b2a.iter()).all(Option::is_none) {
            return Err(missing_table_refusal(&icc));
        }
        Ok(ProofProfile {
            name,
            device,
            pcs,
            media_white,
            a2b,
            b2a,
            gray: None,
            major_version,
            output_class,
        })
    }

    /// True for an ICC v4 profile (major version 4 or above).
    pub fn is_v4(&self) -> bool {
        self.major_version >= 4
    }

    /// True for an output-device (`prtr`) profile — the class the black-point
    /// rule's ink-limit discount applies to (`icc_intent`).
    pub fn is_output_class(&self) -> bool {
        self.output_class
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn device(&self) -> DeviceSpace {
        self.device
    }

    pub fn pcs(&self) -> PcsSpace {
        self.pcs
    }

    /// The profile's `wtpt`, D50 when it has none. Absolute colorimetric
    /// scales by the ratio of two of these.
    pub fn media_white(&self) -> [f64; 3] {
        self.media_white
    }

    /// The device→PCS table for `intent`, with the documented fallback:
    /// perceptual takes table 0, relative and absolute take table 1 then
    /// table 0, saturation takes table 2 then table 0.
    pub fn to_pcs(&self, intent: RenderingIntent) -> Option<&LutPipeline> {
        pick(&self.a2b, intent)
    }

    /// The PCS→device table for `intent`, same fallback.
    pub fn from_pcs(&self, intent: RenderingIntent) -> Option<&LutPipeline> {
        pick(&self.b2a, intent)
    }

    /// A Gray profile's tone curve, `None` for RGB and CMYK.
    pub fn gray_curve(&self) -> Option<&Curve> {
        self.gray.as_ref()
    }

    /// True when this profile can turn device values into PCS — the
    /// direction a document, a proof source or a gamut check needs.
    pub fn can_decode(&self) -> bool {
        self.gray.is_some() || self.a2b.iter().any(Option::is_some)
    }

    /// True when this profile can turn PCS into device values — the
    /// direction a conversion DESTINATION and a proof simulation need.
    pub fn can_encode(&self) -> bool {
        self.gray.is_some() || self.b2a.iter().any(Option::is_some)
    }

    /// Which intents have their OWN table in each direction, for the host's
    /// popup (a missing one still works, through the fallback above).
    pub fn has_table(&self, intent: RenderingIntent) -> bool {
        let slot = intent.table();
        self.a2b[slot].is_some() || self.b2a[slot].is_some() || self.gray.is_some()
    }

    /// One device colour as PCS XYZ (D50). `device` holds
    /// `self.device().channels()` normalized components.
    pub fn to_xyz(&self, device: &[f64], intent: RenderingIntent) -> Option<[f64; 3]> {
        if let Some(curve) = &self.gray {
            let y = curve.eval(device.first().copied().unwrap_or(0.0));
            return Some([D50[0] * y, D50[1] * y, D50[2] * y]);
        }
        let table = self.to_pcs(intent)?;
        let raw = table.eval(device);
        let decoded = table.pcs().decode([raw[0], raw[1], raw[2]]);
        Some(match table.pcs().space() {
            PcsSpace::Lab => lab_to_xyz(decoded),
            PcsSpace::Xyz => decoded,
        })
    }

    /// One PCS XYZ (D50) as device values: the components and how many of
    /// them are meaningful.
    pub fn from_xyz(
        &self,
        xyz: [f64; 3],
        intent: RenderingIntent,
    ) -> Option<([f64; MAX_DEVICE_CHANNELS], usize)> {
        if let Some(curve) = &self.gray {
            // A Gray profile has no inverse curve and this crate has no
            // curve inverter (the `icc_transform` module doc), so the grey
            // is found by the same forward-threshold search every encode in
            // this crate uses, at 16-bit resolution.
            let y = (xyz[1] / D50[1]).clamp(0.0, 1.0);
            let mut low = 0.0f64;
            let mut high = 1.0f64;
            for _ in 0..24 {
                let mid = 0.5 * (low + high);
                if curve.eval(mid) < y {
                    low = mid;
                } else {
                    high = mid;
                }
            }
            let mut out = [0.0f64; MAX_DEVICE_CHANNELS];
            out[0] = 0.5 * (low + high);
            return Some((out, 1));
        }
        let table = self.from_pcs(intent)?;
        let encoded = match table.pcs().space() {
            PcsSpace::Lab => table.pcs().encode(xyz_to_lab(xyz)),
            PcsSpace::Xyz => table.pcs().encode(xyz),
        };
        Some((table.eval(&encoded), table.outputs()))
    }

    /// Why this profile cannot be proofed through: `None` exactly when
    /// [`can_decode`](Self::can_decode) && [`can_encode`](Self::can_encode).
    /// The missing direction is named (decode first), plus its floating-point
    /// twin when `bytes` — what this profile was parsed from — carries one.
    pub(crate) fn proof_refusal(&self, bytes: &[u8]) -> Option<String> {
        let (missing, to_pcs) = if !self.can_decode() {
            ("it has no A2B (device-to-PCS) table", true)
        } else if !self.can_encode() {
            ("it has no B2A (PCS-to-device) table", false)
        } else {
            return None;
        };
        let twin = IccBytes::new(bytes).and_then(|icc| floating_point_tag(&icc, to_pcs));
        Some(match twin {
            Some(tag) => {
                format!("{missing}; its {tag} is floating-point, which this build does not read")
            }
            None => missing.to_string(),
        })
    }

    /// Conservative decoded byte size of every table held, for scratch.
    pub fn bytes(&self) -> u64 {
        self.a2b
            .iter()
            .chain(self.b2a.iter())
            .filter_map(|t| t.as_ref().map(|t| t.bytes()))
            .sum::<u64>()
            + 1024
    }
}

/// Why a pixel-class profile's data colour space is not one this build
/// models. An `nCLR` separation space (`2CLR`..`FCLR`, `n` a hex digit) is
/// named by its colourant count; anything else by its signature.
fn unmodelled_space_refusal(icc: &IccBytes) -> String {
    let sig = icc.space_name();
    if let [d @ (b'2'..=b'9' | b'A'..=b'F'), b'C', b'L', b'R'] = icc.data_space() {
        let n = char::from(*d).to_digit(16).unwrap_or(0);
        return format!(
            "a {n}-colour ({sig}) device space is not supported; this build reads RGB, Gray and CMYK"
        );
    }
    format!("a {sig} device space is not supported; this build reads RGB, Gray and CMYK")
}

/// Why a profile with no usable A2B/B2A table cannot be read. It names the
/// element rather than saying "unsupported": the families below are the
/// documented Phase 11C boundary. Shared by [`ProofProfile::parse`] and the
/// Color Lookup profile reader (`icc_lookup`). The probe walks every
/// floating-point `D2B0..D2B3` then `B2D0..B2D3` tag and names the first it
/// finds; a named-colour profile never reaches here (its class is refused
/// first), and an `mpet`-TYPED `A2Bx`/`B2Ax` is refused by [`parse_table`].
pub(crate) fn missing_table_refusal(icc: &IccBytes) -> String {
    match floating_point_tag(icc, true).or_else(|| floating_point_tag(icc, false)) {
        Some(name) => format!("a floating-point {name} element is not supported"),
        None => "the profile carries no A2B or B2A table".into(),
    }
}

/// The floating-point device→PCS tags, in the order they are probed.
const D2B_TAGS: [&[u8; 4]; 4] = [b"D2B0", b"D2B1", b"D2B2", b"D2B3"];
/// The floating-point PCS→device tags, in the order they are probed.
const B2D_TAGS: [&[u8; 4]; 4] = [b"B2D0", b"B2D1", b"B2D2", b"B2D3"];

/// First of D2B0..D2B3 (to_pcs) or B2D0..B2D3 (!to_pcs) present, by tag name.
pub(crate) fn floating_point_tag(icc: &IccBytes, to_pcs: bool) -> Option<&'static str> {
    let tags = if to_pcs { D2B_TAGS } else { B2D_TAGS };
    tags.into_iter()
        .find(|sig| icc.tag(sig).is_some())
        .and_then(|sig| std::str::from_utf8(sig).ok())
}

/// One 3-in, 3-out `A2B` table read on its own, outside any
/// [`ProofProfile`]: a device link's or an abstract profile's `A2B0`
/// (`icc_lookup`). Parsed in the device→PCS direction under a fresh
/// [`MAX_CLUT_BYTES`] budget; `pcs` sets only how the output side is
/// encoded ([`LutPipeline::pcs`]), which a device link never reads.
pub(crate) fn transform_table(data: &[u8], pcs: PcsSpace) -> Result<LutPipeline, String> {
    let mut budget = MAX_CLUT_BYTES;
    parse_table(data, 3, 3, pcs, Direction::ToPcs, &mut budget)
}

/// An ABSTRACT profile's `A2B0` (`icc_lookup`), whose two sides are
/// both PCS: `input` is the header's data colour space and `output` its PCS,
/// each Lab or XYZ. Returns the pipeline, whose [`LutPipeline::pcs`] is the
/// OUTPUT side's encoding, and the INPUT side's encoding, which the caller
/// encodes each node with before [`LutPipeline::eval`]. The encodings follow
/// the module doc's table on each side. A `lut8`/`lut16` table's 3×3 matrix
/// applies iff `input` is XYZ — the same rule [`transform_table`]'s
/// PCS→device parse applies, since the matrix sits on the table's input
/// side whichever direction the profile runs.
pub(crate) fn abstract_table(
    data: &[u8],
    input: PcsSpace,
    output: PcsSpace,
) -> Result<(LutPipeline, PcsEncoding), String> {
    let mut pipeline = transform_table(data, output)?;
    let input_encoding = match data.get(0..4) {
        Some(b"mft1") | Some(b"mft2") => {
            if input == PcsSpace::Xyz {
                if let Some(matrix) = mft_matrix(data)? {
                    pipeline.stages.insert(0, Stage::Matrix(matrix));
                }
            }
            mft_encoding(
                input,
                if data.get(0..4) == Some(b"mft1") {
                    1
                } else {
                    2
                },
            )
        }
        // `mAB `: the v4 encodings, as `parse_mab` sets the output side's.
        _ => match input {
            PcsSpace::Lab => PcsEncoding::LabModern,
            PcsSpace::Xyz => PcsEncoding::Xyz,
        },
    };
    Ok((pipeline, input_encoding))
}

fn pick(tables: &[Option<Arc<LutPipeline>>; 3], intent: RenderingIntent) -> Option<&LutPipeline> {
    tables[intent.table()]
        .as_deref()
        .or_else(|| tables[0].as_deref())
}

// ----------------------------------------------------------- tag decoding --

#[derive(Clone, Copy, PartialEq, Eq)]
enum Direction {
    ToPcs,
    FromPcs,
}

/// Dispatches on the tag's own type signature. `inputs`/`outputs` are what
/// the DIRECTION requires; a table that declares different counts is refused
/// rather than reinterpreted.
fn parse_table(
    data: &[u8],
    inputs: usize,
    outputs: usize,
    pcs: PcsSpace,
    direction: Direction,
    budget: &mut u64,
) -> Result<LutPipeline, String> {
    match data.get(0..4) {
        Some(b"mft1") => parse_mft(data, inputs, outputs, pcs, direction, 1, budget),
        Some(b"mft2") => parse_mft(data, inputs, outputs, pcs, direction, 2, budget),
        Some(b"mAB ") => parse_mab(data, inputs, outputs, pcs, direction, false, budget),
        Some(b"mBA ") => parse_mab(data, inputs, outputs, pcs, direction, true, budget),
        // ICC v4 allows the `mpet` (multiProcessElements) type only in the
        // floating-point D2Bx/B2Dx tags; an `mpet`-typed A2Bx/B2Ax is a
        // non-conformant or iccMAX profile, which lcms refuses too.
        Some(b"mpet") => Err("a multiProcessElements tag is not supported".into()),
        Some(other) => Err(format!(
            "the \"{}\" table type is not supported",
            String::from_utf8_lossy(other).trim_end()
        )),
        None => Err("a colour table is truncated".into()),
    }
}

/// `lut8Type` and `lut16Type`: matrix (XYZ input only), input tables, CLUT,
/// output tables, in that order.
fn parse_mft(
    data: &[u8],
    inputs: usize,
    outputs: usize,
    pcs: PcsSpace,
    direction: Direction,
    width: usize,
    budget: &mut u64,
) -> Result<LutPipeline, String> {
    let head = |at: usize| data.get(at).copied().ok_or("a colour table is truncated");
    let declared_in = head(8)? as usize;
    let declared_out = head(9)? as usize;
    let grid = head(10)? as usize;
    if declared_in != inputs || declared_out != outputs {
        return Err(format!(
            "a colour table declares {declared_in} inputs and {declared_out} outputs where {inputs} and {outputs} were expected"
        ));
    }
    if grid < 2 {
        return Err("a colour table has fewer than two grid points".into());
    }
    let mut stages = Vec::new();
    // The 3x3 matrix applies only when the table's INPUT is PCS XYZ; every
    // other profile writes the identity there and the ICC spec requires it.
    if direction == Direction::FromPcs && pcs == PcsSpace::Xyz {
        if let Some(m) = mft_matrix(data)? {
            stages.push(Stage::Matrix(m));
        }
    }
    let (entries_in, entries_out, mut at) = if width == 1 {
        (256usize, 256usize, 48usize)
    } else {
        let n = icc::be_u16(data, 48).ok_or("a colour table is truncated")? as usize;
        let m = icc::be_u16(data, 50).ok_or("a colour table is truncated")? as usize;
        if !(2..=4096).contains(&n) || !(2..=4096).contains(&m) {
            return Err("a colour table has an unusable tone-curve length".into());
        }
        (n, m, 52usize)
    };
    stages.push(Stage::Curves(read_tables(
        data, &mut at, inputs, entries_in, width,
    )?));
    let samples = grid
        .checked_pow(inputs as u32)
        .and_then(|n| n.checked_mul(outputs))
        .ok_or("a colour table is too large")?;
    charge(budget, samples as u64 * 4)?;
    let mut values = Vec::new();
    values
        .try_reserve_exact(samples)
        .map_err(|_| "could not allocate a colour table")?;
    for i in 0..samples {
        values.push(read_sample(data, at + i * width, width)? as f32);
    }
    at += samples * width;
    stages.push(Stage::Clut(Clut {
        grid: vec![grid; inputs],
        outputs,
        data: values,
    }));
    stages.push(Stage::Curves(read_tables(
        data,
        &mut at,
        outputs,
        entries_out,
        width,
    )?));
    Ok(LutPipeline {
        stages,
        inputs,
        outputs,
        pcs: mft_encoding(pcs, width),
    })
}

/// A `lut8`/`lut16` table's 3×3 matrix (no offsets), or `None` when it is
/// the identity — which every table whose input is not PCS XYZ must carry.
fn mft_matrix(data: &[u8]) -> Result<Option<[f64; 12]>, String> {
    let mut m = [0.0f64; 12];
    for (i, slot) in m.iter_mut().take(9).enumerate() {
        *slot = icc::s15(data, 12 + 4 * i).ok_or("a colour table's matrix is truncated")?;
    }
    Ok((m[..9] != [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0]).then_some(m))
}

/// The module doc's encoding table, as a function.
fn mft_encoding(pcs: PcsSpace, width: usize) -> PcsEncoding {
    match (pcs, width) {
        (PcsSpace::Xyz, _) => PcsEncoding::Xyz,
        (PcsSpace::Lab, 1) => PcsEncoding::LabModern,
        (PcsSpace::Lab, _) => PcsEncoding::LabLegacy16,
    }
}

fn read_sample(data: &[u8], at: usize, width: usize) -> Result<f64, String> {
    if width == 1 {
        Ok(f64::from(*data.get(at).ok_or("a colour table is truncated")?) / 255.0)
    } else {
        Ok(f64::from(icc::be_u16(data, at).ok_or("a colour table is truncated")?) / 65535.0)
    }
}

/// `count` uniformly sampled tone curves of `entries` samples each, as
/// [`Curve::Sampled`] so the shared evaluator interpolates them.
fn read_tables(
    data: &[u8],
    at: &mut usize,
    count: usize,
    entries: usize,
    width: usize,
) -> Result<Vec<Curve>, String> {
    let mut curves = Vec::new();
    curves
        .try_reserve_exact(count)
        .map_err(|_| "could not allocate a colour table")?;
    for _ in 0..count {
        let mut table = Vec::new();
        table
            .try_reserve_exact(entries)
            .map_err(|_| "could not allocate a colour table")?;
        for i in 0..entries {
            let value = read_sample(data, *at + i * width, width)?;
            table.push((value * 65535.0).round() as u16);
        }
        *at += entries * width;
        curves.push(Curve::Sampled(table));
    }
    Ok(curves)
}

/// `lutAtoBType` / `lutBtoAType`. The five offsets are identical in both; only
/// the ORDER the elements run in differs — A → CLUT → M → matrix → B for
/// A-to-B, and B → matrix → M → CLUT → A for B-to-A.
#[allow(clippy::too_many_arguments)]
fn parse_mab(
    data: &[u8],
    inputs: usize,
    outputs: usize,
    pcs: PcsSpace,
    direction: Direction,
    reversed: bool,
    budget: &mut u64,
) -> Result<LutPipeline, String> {
    let expected_reversed = direction == Direction::FromPcs;
    if reversed != expected_reversed {
        return Err("a colour table runs in the wrong direction".into());
    }
    let declared_in = *data.get(8).ok_or("a colour table is truncated")? as usize;
    let declared_out = *data.get(9).ok_or("a colour table is truncated")? as usize;
    if declared_in != inputs || declared_out != outputs {
        return Err(format!(
            "a colour table declares {declared_in} inputs and {declared_out} outputs where {inputs} and {outputs} were expected"
        ));
    }
    let offset = |at: usize| icc::be_u32(data, at).map(|v| v as usize);
    let b_at = offset(12).ok_or("a colour table is truncated")?;
    let matrix_at = offset(16).ok_or("a colour table is truncated")?;
    let m_at = offset(20).ok_or("a colour table is truncated")?;
    let clut_at = offset(24).ok_or("a colour table is truncated")?;
    let a_at = offset(28).ok_or("a colour table is truncated")?;

    // In `mAB ` the A curves take the INPUT channels and the B curves the
    // output ones; in `mBA ` it is the other way round.
    let (a_count, b_count) = if reversed {
        (outputs, inputs)
    } else {
        (inputs, outputs)
    };
    // M curves sit next to the matrix, which is 3x3: three of them.
    let a = read_curve_run(data, a_at, a_count)?;
    let b = read_curve_run(data, b_at, b_count)?;
    let m = read_curve_run(data, m_at, 3)?;
    let matrix = if matrix_at == 0 {
        None
    } else {
        let mut values = [0.0f64; 12];
        for (i, slot) in values.iter_mut().enumerate() {
            *slot =
                icc::s15(data, matrix_at + 4 * i).ok_or("a colour table's matrix is truncated")?;
        }
        Some(values)
    };
    // In both directions the CLUT takes the tag's inputs and produces its
    // outputs: `mAB ` runs A → CLUT → M, `mBA ` runs M → CLUT → A, and the
    // element between the curves is the same table shape either way.
    let clut = if clut_at == 0 {
        None
    } else {
        Some(read_clut(data, clut_at, inputs, outputs, budget)?)
    };
    if matrix.is_some() && m.is_none() {
        return Err("a colour table has a matrix without its M curves".into());
    }
    if clut.is_none() && b.is_none() {
        return Err("a colour table carries no usable element".into());
    }

    let mut stages = Vec::new();
    if reversed {
        if let Some(curves) = b {
            stages.push(Stage::Curves(curves));
        }
        if let Some(matrix) = matrix {
            stages.push(Stage::Matrix(matrix));
        }
        if let Some(curves) = m {
            stages.push(Stage::Curves(curves));
        }
        if let Some(clut) = clut {
            stages.push(Stage::Clut(clut));
        }
        if let Some(curves) = a {
            stages.push(Stage::Curves(curves));
        }
    } else {
        if let Some(curves) = a {
            stages.push(Stage::Curves(curves));
        }
        if let Some(clut) = clut {
            stages.push(Stage::Clut(clut));
        }
        if let Some(curves) = m {
            stages.push(Stage::Curves(curves));
        }
        if let Some(matrix) = matrix {
            stages.push(Stage::Matrix(matrix));
        }
        if let Some(curves) = b {
            stages.push(Stage::Curves(curves));
        }
    }
    Ok(LutPipeline {
        stages,
        inputs,
        outputs,
        // v4 `mAB `/`mBA ` use the MODERN Lab encoding (L* = 100 at 0xFFFF);
        // an XYZ PCS is the same u1Fixed15 as everywhere else. Which of the
        // two applies is the profile HEADER's question, not the tag's.
        pcs: match pcs {
            PcsSpace::Lab => PcsEncoding::LabModern,
            PcsSpace::Xyz => PcsEncoding::Xyz,
        },
    })
}

/// `count` consecutive `curv`/`para` tags, each padded to a 4-byte boundary.
/// `None` when the offset is 0, which means the element is absent.
fn read_curve_run(data: &[u8], at: usize, count: usize) -> Result<Option<Vec<Curve>>, String> {
    if at == 0 {
        return Ok(None);
    }
    let mut curves = Vec::new();
    curves
        .try_reserve_exact(count)
        .map_err(|_| "could not allocate a colour table")?;
    let mut cursor = at;
    for _ in 0..count {
        let length = curve_length(data, cursor)?;
        let end = cursor
            .checked_add(length)
            .filter(|end| *end <= data.len())
            .ok_or("a colour table's curve is truncated")?;
        let curve = icc::curve(&data[cursor..end]).ok_or("a colour table has an unusable curve")?;
        curves.push(curve);
        cursor = end.next_multiple_of(4);
    }
    Ok(Some(curves))
}

/// The byte length of one `curv`/`para` element, before its 4-byte padding.
fn curve_length(data: &[u8], at: usize) -> Result<usize, String> {
    match data.get(at..at + 4) {
        Some(b"curv") => {
            let count =
                icc::be_u32(data, at + 8).ok_or("a colour table's curve is truncated")? as usize;
            count
                .checked_mul(2)
                .and_then(|n| n.checked_add(12))
                .ok_or_else(|| "a colour table's curve is too large".to_string())
        }
        Some(b"para") => {
            let kind = icc::be_u16(data, at + 8).ok_or("a colour table's curve is truncated")?;
            let n = match kind {
                0 => 1usize,
                1 => 3,
                2 => 4,
                3 => 5,
                4 => 7,
                _ => return Err("a colour table has an unknown parametric curve".into()),
            };
            Ok(12 + 4 * n)
        }
        _ => Err("a colour table has an unusable curve".into()),
    }
}

/// The `mAB `/`mBA ` CLUT element: up to 16 per-axis grid counts, a precision
/// byte, then the samples.
fn read_clut(
    data: &[u8],
    at: usize,
    inputs: usize,
    outputs: usize,
    budget: &mut u64,
) -> Result<Clut, String> {
    let head = data
        .get(at..at + 20)
        .ok_or("a colour table's CLUT is truncated")?;
    let mut grid = Vec::new();
    grid.try_reserve_exact(inputs)
        .map_err(|_| "could not allocate a colour table")?;
    let mut samples = outputs;
    for &point in head.iter().take(inputs) {
        let points = point as usize;
        if points < 2 {
            return Err("a colour table has fewer than two grid points".into());
        }
        samples = samples
            .checked_mul(points)
            .ok_or("a colour table is too large")?;
        grid.push(points);
    }
    let width = match head[16] {
        1 => 1usize,
        2 => 2,
        other => {
            return Err(format!(
                "a CLUT precision of {other} bytes is not supported"
            ))
        }
    };
    charge(budget, samples as u64 * 4)?;
    let mut values = Vec::new();
    values
        .try_reserve_exact(samples)
        .map_err(|_| "could not allocate a colour table")?;
    for i in 0..samples {
        values.push(read_sample(data, at + 20 + i * width, width)? as f32);
    }
    Ok(Clut {
        grid,
        outputs,
        data: values,
    })
}

fn charge(budget: &mut u64, bytes: u64) -> Result<(), String> {
    if bytes > *budget {
        return Err(format!(
            "the profile's colour tables exceed the {} MiB limit",
            MAX_CLUT_BYTES >> 20
        ));
    }
    *budget -= bytes;
    Ok(())
}
