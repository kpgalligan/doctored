//! The two colour legs of a CMYK document (#547 phase 1), written ONCE and
//! shared with the two older callers that ran the same arithmetic inline:
//!
//! * **RGB → CMYK** ([`ToDevice`], [`RgbToCmyk`]): an RGB colour's PCS XYZ
//!   (the document profile's own model, relative — `document_xyz`), the
//!   absolute media-white scale, the ISO 18619 BPC affine from the RGB
//!   profile's source black to the CMYK profile's DESTINATION black, then
//!   the CMYK profile's `B2A` at the intent. This is exactly the first half
//!   of `soft_proof::Proofer::evaluate` (document → proof device), which now
//!   calls [`ToDevice`] rather than repeating it, so a proof and an
//!   Image ▸ Mode ▸ CMYK Color conversion of the same colour land on the same
//!   ink.
//! * **CMYK → RGB** ([`CmykToRgb`]): ink through the CMYK profile's `A2B` at
//!   the intent, the absolute scale, BPC from the CMYK profile's source black
//!   (`proof_black_point`, lcms's `cmsDetectBlackPoint`) to the destination's
//!   destination black, then `DocumentPcs::from_xyz` into the RGB profile.
//!   At relative colorimetric with BPC into the built-in sRGB this is
//!   `open_profile`'s flat-open conversion, which now runs through
//!   [`CmykToRgb::evaluate`] (byte-identical: the same calls in the same
//!   order).
//!
//! * **CMYK → CMYK** ([`CmykToCmyk`], #547 phases 5 and 4c): Edit ▸ Convert
//!   to Profile between two CMYK profiles, the CMYK document's proof, and a
//!   paste between CMYK documents of different profiles. The source's `A2B`
//!   at the intent, the absolute media-white scale, BPC from the source's
//!   `proof_black_point` to the destination's `proof_destination_black_point`
//!   (lcms's `ComputeConversion` asks each side exactly that), then the
//!   destination's `B2A` at the intent — the first half of [`CmykToRgb`]
//!   glued to the second half of [`ToDevice`], with no step of its own.
//!
//! Every step is the existing engine's — `icc_lut::ProofProfile::{to_xyz,
//! from_xyz}`, `icc_intent::{bpc_terms, DocumentPcs, media_white}` and
//! `icc_black_point` — and nothing here is new colour math.
//!
//! A profile with no `B2A` is refused through [`require_encodable`],
//! [`RgbToCmyk::new`]'s own sentence.
//!
//! # The view link
//!
//! [`CmykViewLink`] is the CMYK document's display projection: the CMYK →
//! RGB leg at relative colorimetric with BPC into the built-in Display P3
//! (the window server applies the monitor profile to the tagged P3 result
//! exactly as it does for every RGB document), SAMPLED in LINEAR light on a
//! 33⁴ grid of ink values, interpolated quadrilinearly, then clamped and
//! encoded through a 4097-sample table of the destination's own encoder.
//! The bound was fixed first: `tests/cmyk_document.rs` holds the link to
//! ≤ 1/255 per channel of the exact leg at every cell centre of its grid
//! (deep shadows included) for a synthetic profile and macOS's Generic
//! CMYK. Encoded nodes missed it (2.9/255 at 17 nodes, 1.4/255 at 33: the
//! gamut clamp and the encoding's toe are kinks no grid interpolates), and
//! linear nodes at 17 still missed on Generic CMYK (1.6/255), so the grid
//! was raised to 33 per axis as the spec rules, never the bound. Its
//! 1,185,921 nodes × 3 f32 are 14.2 MB, built once per document policy and
//! charged by every view plan, so a 24 MP render evaluates sixteen table
//! reads a pixel instead of a profile walk.

use std::collections::HashMap;
use std::sync::Arc;

use crate::icc::IccProfile;
use crate::icc_black_point::{
    black_point, destination_black_point, proof_black_point, proof_destination_black_point,
};
use crate::icc_intent::{self, DocumentPcs, RenderingIntent};
use crate::icc_lut::{ProofProfile, MAX_DEVICE_CHANNELS};
use crate::soft_proof::MEMO_CAPACITY;

/// The options both conversions take (Image ▸ Mode's sheet, `convert_mode`):
/// the defaults are relative colorimetric with BPC.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ConversionOptions {
    pub intent: RenderingIntent,
    pub black_point_compensation: bool,
}

impl Default for ConversionOptions {
    fn default() -> Self {
        Self {
            intent: RenderingIntent::RelativeColorimetric,
            black_point_compensation: true,
        }
    }
}

/// An RGB document profile's PCS XYZ (D50) for one encoded triple in
/// [0, 1], relative: the matrix/TRC model, else the LUT's relative table,
/// black when it has none. `soft_proof::Proofer`'s document decode.
pub(crate) fn document_xyz(document: &IccProfile, rgb: [f64; 3]) -> [f64; 3] {
    match document.model() {
        Some(model) => model.to_pcs_xyz64(rgb),
        None => document
            .lut()
            .and_then(|lut| lut.to_xyz(&rgb, RenderingIntent::RelativeColorimetric))
            .unwrap_or([0.0; 3]),
    }
}

/// The document → device half of a conversion into a LUT profile: the
/// absolute white scale and the BPC affine, then the device's `B2A`.
pub(crate) struct ToDevice {
    intent: RenderingIntent,
    /// `MW_src / MW_dst` per component under absolute, else ones.
    white: [f64; 3],
    /// Document black → device destination black, when BPC is on.
    bpc: Option<[(f64, f64); 3]>,
}

impl ToDevice {
    /// The terms `Proofer::new` computed inline: the document's
    /// `cmsDetectBlackPoint` at relative, the device's
    /// `cmsDetectDestinationBlackPoint` at the intent, and the media whites.
    pub(crate) fn new(
        document: &IccProfile,
        device: &ProofProfile,
        intent: RenderingIntent,
        black_point_compensation: bool,
    ) -> Self {
        let document_black =
            black_point(document, RenderingIntent::RelativeColorimetric).unwrap_or([0.0; 3]);
        let device_black = proof_destination_black_point(device, intent);
        let bpc =
            black_point_compensation.then(|| icc_intent::bpc_terms(document_black, device_black));
        let device_white = device.media_white();
        let document_white = icc_intent::media_white(document);
        let white = if intent.is_absolute() {
            std::array::from_fn(|i| {
                if device_white[i].abs() < 1e-9 {
                    1.0
                } else {
                    document_white[i] / device_white[i]
                }
            })
        } else {
            [1.0, 1.0, 1.0]
        };
        Self { intent, white, bpc }
    }

    /// PCS XYZ (the document's) as device values and their count.
    pub(crate) fn device(
        &self,
        device: &ProofProfile,
        source: [f64; 3],
    ) -> Option<([f64; MAX_DEVICE_CHANNELS], usize)> {
        let mut xyz = source;
        for (slot, scale) in xyz.iter_mut().zip(self.white) {
            *slot *= scale;
        }
        if let Some(terms) = &self.bpc {
            for (slot, (a, b)) in xyz.iter_mut().zip(terms.iter()) {
                *slot = a * *slot + b;
            }
        }
        device.from_xyz(xyz, self.intent)
    }
}

/// RGB (an RGB `IccProfile`'s encoded values) → CMYK ink in [0, 1], memoized
/// per distinct input (the key the caller passes). It holds the RGB profile
/// by `Arc` so a caller can build one over a profile it made on the spot
/// (`srgb_to_ink`).
pub(crate) struct RgbToCmyk<'a> {
    rgb: Arc<IccProfile>,
    cmyk: &'a ProofProfile,
    leg: ToDevice,
    memo: HashMap<u64, [f64; 4]>,
}

impl<'a> RgbToCmyk<'a> {
    pub(crate) fn new(
        rgb: Arc<IccProfile>,
        cmyk: &'a ProofProfile,
        options: ConversionOptions,
    ) -> Result<Self, String> {
        if !rgb.can_decode() {
            return Err(format!(
                "the document profile \"{}\" carries no model this build can read",
                rgb.name()
            ));
        }
        require_cmyk(cmyk)?;
        require_encodable(cmyk)?;
        let leg = ToDevice::new(&rgb, cmyk, options.intent, options.black_point_compensation);
        Ok(Self {
            rgb,
            cmyk,
            leg,
            memo: HashMap::new(),
        })
    }

    /// Ink for one encoded RGB triple, unmemoized: the RGB profile's PCS
    /// XYZ, the [`ToDevice`] leg, each ink clamped to [0, 1] (zero ink when
    /// the `B2A` answers nothing).
    pub(crate) fn evaluate(&self, rgb: [f64; 3]) -> [f64; 4] {
        match self.leg.device(self.cmyk, document_xyz(&self.rgb, rgb)) {
            Some((device, _)) => std::array::from_fn(|i| device[i].clamp(0.0, 1.0)),
            None => [0.0; 4],
        }
    }

    /// Ink for one encoded RGB triple; `key` identifies it in the memo.
    pub(crate) fn ink(&mut self, key: u64, rgb: [f64; 3]) -> [f64; 4] {
        if let Some(hit) = self.memo.get(&key) {
            return *hit;
        }
        let value = self.evaluate(rgb);
        if self.memo.len() < MEMO_CAPACITY {
            self.memo.insert(key, value);
        }
        value
    }
}

/// CMYK ink in [0, 1] → encoded RGB of a destination `IccProfile`, memoized
/// per distinct input key.
pub(crate) struct CmykToRgb<'a> {
    cmyk: &'a ProofProfile,
    pcs: DocumentPcs,
    intent: RenderingIntent,
    white: [f64; 3],
    bpc: Option<[(f64, f64); 3]>,
    memo: HashMap<u64, [f32; 3]>,
}

impl<'a> CmykToRgb<'a> {
    pub(crate) fn new(
        cmyk: &'a ProofProfile,
        rgb: &IccProfile,
        options: ConversionOptions,
    ) -> Result<Self, String> {
        require_cmyk(cmyk)?;
        if cmyk.to_pcs(options.intent).is_none() {
            return Err(format!(
                "the CMYK profile \"{}\" has no A2B (device-to-PCS) table",
                cmyk.name()
            ));
        }
        let pcs = DocumentPcs::new(rgb)?;
        let source_black = proof_black_point(cmyk, options.intent);
        let destination_black = destination_black_point(rgb, options.intent).unwrap_or([0.0; 3]);
        let bpc = options
            .black_point_compensation
            .then(|| icc_intent::bpc_terms(source_black, destination_black));
        let cmyk_white = cmyk.media_white();
        let rgb_white = icc_intent::media_white(rgb);
        let white = if options.intent.is_absolute() {
            std::array::from_fn(|i| {
                if rgb_white[i].abs() < 1e-9 {
                    1.0
                } else {
                    cmyk_white[i] / rgb_white[i]
                }
            })
        } else {
            [1.0, 1.0, 1.0]
        };
        Ok(Self {
            cmyk,
            pcs,
            intent: options.intent,
            white,
            bpc,
            memo: HashMap::new(),
        })
    }

    /// The exact leg, unmemoized: `ProofProfile::to_xyz`, the scale and the
    /// BPC affine, `DocumentPcs::from_xyz`. Values are encoded RGB, not yet
    /// clamped.
    pub(crate) fn evaluate(&self, ink: [f64; 4]) -> [f32; 3] {
        self.pcs.from_xyz(self.xyz(ink))
    }

    /// The leg up to the destination's encode: the PCS XYZ, scaled and
    /// compensated, that [`Self::evaluate`] hands `DocumentPcs::from_xyz`.
    pub(crate) fn xyz(&self, ink: [f64; 4]) -> [f64; 3] {
        let mut xyz = self.cmyk.to_xyz(&ink, self.intent).unwrap_or([0.0; 3]);
        for (slot, scale) in xyz.iter_mut().zip(self.white) {
            *slot *= scale;
        }
        if let Some(terms) = &self.bpc {
            for (slot, (a, b)) in xyz.iter_mut().zip(terms.iter()) {
                *slot = a * *slot + b;
            }
        }
        xyz
    }

    /// [`Self::evaluate`] through the memo (`key` identifies the input).
    pub(crate) fn rgb(&mut self, key: u64, ink: [f64; 4]) -> [f32; 3] {
        if let Some(hit) = self.memo.get(&key) {
            return *hit;
        }
        let value = self.evaluate(ink);
        if self.memo.len() < MEMO_CAPACITY {
            self.memo.insert(key, value);
        }
        value
    }
}

/// CMYK ink in one profile → CMYK ink in another (module doc), memoized per
/// distinct input key.
pub(crate) struct CmykToCmyk<'a> {
    source: &'a ProofProfile,
    destination: &'a ProofProfile,
    intent: RenderingIntent,
    /// `MW_src / MW_dst` per component under absolute, else ones.
    white: [f64; 3],
    /// Source black → destination black, when BPC is on.
    bpc: Option<[(f64, f64); 3]>,
    memo: HashMap<u64, [f64; 4]>,
}

impl<'a> CmykToCmyk<'a> {
    /// Refuses a source that is not CMYK or has no `A2B` at the intent
    /// ([`CmykToRgb::new`]'s sentences) and a destination that is not CMYK
    /// or has no `B2A` ([`RgbToCmyk::new`]'s).
    pub(crate) fn new(
        source: &'a ProofProfile,
        destination: &'a ProofProfile,
        options: ConversionOptions,
    ) -> Result<Self, String> {
        require_cmyk(source)?;
        if source.to_pcs(options.intent).is_none() {
            return Err(format!(
                "the CMYK profile \"{}\" has no A2B (device-to-PCS) table",
                source.name()
            ));
        }
        require_cmyk(destination)?;
        require_encodable(destination)?;
        let source_black = proof_black_point(source, options.intent);
        let destination_black = proof_destination_black_point(destination, options.intent);
        let bpc = options
            .black_point_compensation
            .then(|| icc_intent::bpc_terms(source_black, destination_black));
        let source_white = source.media_white();
        let destination_white = destination.media_white();
        let white = if options.intent.is_absolute() {
            std::array::from_fn(|i| {
                if destination_white[i].abs() < 1e-9 {
                    1.0
                } else {
                    source_white[i] / destination_white[i]
                }
            })
        } else {
            [1.0, 1.0, 1.0]
        };
        Ok(Self {
            source,
            destination,
            intent: options.intent,
            white,
            bpc,
            memo: HashMap::new(),
        })
    }

    /// The exact leg, unmemoized: each output ink clamped to [0, 1], zero
    /// ink when a table answers nothing.
    pub(crate) fn evaluate(&self, ink: [f64; 4]) -> [f64; 4] {
        let Some(mut xyz) = self.source.to_xyz(&ink, self.intent) else {
            return [0.0; 4];
        };
        for (slot, scale) in xyz.iter_mut().zip(self.white) {
            *slot *= scale;
        }
        if let Some(terms) = &self.bpc {
            for (slot, (a, b)) in xyz.iter_mut().zip(terms.iter()) {
                *slot = a * *slot + b;
            }
        }
        match self.destination.from_xyz(xyz, self.intent) {
            Some((device, _)) => std::array::from_fn(|i| device[i].clamp(0.0, 1.0)),
            None => [0.0; 4],
        }
    }

    /// [`Self::evaluate`] through the memo (`key` identifies the input).
    pub(crate) fn ink(&mut self, key: u64, ink: [f64; 4]) -> [f64; 4] {
        if let Some(hit) = self.memo.get(&key) {
            return *hit;
        }
        let value = self.evaluate(ink);
        if self.memo.len() < MEMO_CAPACITY {
            self.memo.insert(key, value);
        }
        value
    }
}

/// One encoded sRGB colour (the built-in sRGB, 0…1) as ink in `profile`
/// (0…1) — the Color Picker's, the eyedropper's and MCP's single colour
/// across the CMYK boundary (#547 phase 2), through [`RgbToCmyk`] and no
/// other arithmetic.
pub(crate) fn srgb_to_ink(
    profile: &ProofProfile,
    rgb: [f64; 3],
    options: ConversionOptions,
) -> Result<[f64; 4], String> {
    let mut leg = RgbToCmyk::new(IccProfile::srgb(), profile, options)?;
    Ok(leg.ink(0, rgb.map(|v| v.clamp(0.0, 1.0))))
}

/// Ink in `profile` (0…1) as encoded sRGB (the built-in sRGB, clamped to
/// 0…1), through [`CmykToRgb::evaluate`].
pub(crate) fn ink_to_srgb(
    profile: &ProofProfile,
    ink: [f64; 4],
    options: ConversionOptions,
) -> Result<[f64; 3], String> {
    let srgb = IccProfile::srgb();
    let leg = CmykToRgb::new(profile, &srgb, options)?;
    Ok(leg
        .evaluate(ink.map(|v| v.clamp(0.0, 1.0)))
        .map(|v| f64::from(v.clamp(0.0, 1.0))))
}

/// The memo key of four u16 inks.
pub(crate) fn ink_key(ink: [u16; 4]) -> u64 {
    ink.iter()
        .fold(0u64, |acc, &sample| (acc << 16) | u64::from(sample))
}

fn require_cmyk(profile: &ProofProfile) -> Result<(), String> {
    if profile.device() != crate::icc_lut::DeviceSpace::Cmyk {
        return Err(format!("“{}” is not a CMYK profile", profile.name()));
    }
    Ok(())
}

/// Refuses a CMYK profile with no `B2A` (PCS → device) table: nothing can be
/// converted into it. [`RgbToCmyk::new`]'s sentence, shared with the plans
/// of an edit that re-separates ink through the profile (Hue/Saturation).
pub(crate) fn require_encodable(cmyk: &ProofProfile) -> Result<(), String> {
    if !cmyk.can_encode() {
        return Err(format!(
            "the CMYK profile \"{}\" has no B2A (PCS-to-device) table, so nothing can be converted into it",
            cmyk.name()
        ));
    }
    Ok(())
}

/// The largest per-channel difference between the view link and the exact
/// leg it samples, over every CELL CENTRE of the link's own grid —
/// `(2j + 1) / (2(n − 1))` per axis, `(n − 1)⁴` points, every one as far
/// from a node as a point can be, deep shadows included (at 17 nodes these
/// were the `16⁴` points `(2j + 1) / 32`; at 33 those are nodes, so the
/// centres move with the grid). A black-box seam for the link's accuracy
/// bound (`tests/cmyk_document.rs`); nothing else calls it.
#[doc(hidden)]
pub fn view_link_deviation(icc: &[u8]) -> Result<f32, String> {
    let profile = ProofProfile::parse(icc)?;
    let link = CmykViewLink::build(&profile)?;
    let p3 = IccProfile::display_p3();
    let leg = CmykToRgb::new(&profile, &p3, ConversionOptions::default())?;
    let cells = VIEW_NODES - 1;
    let centre = |j: usize| (2 * j + 1) as f64 / (2 * cells) as f64;
    let mut worst = 0f32;
    for c in 0..cells {
        for m in 0..cells {
            for y in 0..cells {
                for k in 0..cells {
                    let ink = [centre(c), centre(m), centre(y), centre(k)];
                    let exact = leg.evaluate(ink).map(|v| v.clamp(0.0, 1.0));
                    let linked = link.eval(ink.map(|v| v as f32)).map(|v| v.clamp(0.0, 1.0));
                    for (a, b) in exact.iter().zip(linked) {
                        worst = worst.max((a - b).abs());
                    }
                }
            }
        }
    }
    Ok(worst)
}

/// Nodes per axis of the view link (module doc).
pub(crate) const VIEW_NODES: usize = 33;

/// The sampled CMYK → Display P3 view (module doc).
pub struct CmykViewLink {
    /// LINEAR Display P3 at every node, unclamped.
    nodes: Vec<[f32; 3]>,
    /// The Display P3 encode per channel, `ENCODE_STEPS + 1` samples of
    /// `PrecisionToneEncoder::rgb` over linear [0, 1].
    encode: [Vec<f32>; 3],
}

/// Steps of the view's encode table: the TRC's linear toe spans a dozen of
/// them, and between samples of its power part linear interpolation is
/// within 2·10⁻⁵ of the curve.
const ENCODE_STEPS: usize = 4096;

impl CmykViewLink {
    /// Samples `profile`'s relative + BPC leg into Display P3 at every node.
    ///
    /// The nodes hold the leg in LINEAR light, before the destination's
    /// clamp and encode, and [`Self::eval`] encodes after interpolating: the
    /// clamp at the gamut boundary and the encoding's toe are kinks no grid
    /// interpolates well (measured: 2.9/255 at 17 nodes, 1.4/255 at 33 when
    /// the nodes were encoded), while the linear leg is smooth across both.
    pub(crate) fn build(profile: &ProofProfile) -> Result<Self, String> {
        let p3 = IccProfile::display_p3();
        let model = p3
            .model()
            .ok_or("the view profile has no matrix/TRC model")?;
        let leg = CmykToRgb::new(profile, &p3, ConversionOptions::default())?;
        let n = VIEW_NODES;
        let step = 1.0 / (n - 1) as f64;
        let mut nodes = crate::precision_image::reserved_vec(n * n * n * n)?;
        for c in 0..n {
            for m in 0..n {
                for y in 0..n {
                    for k in 0..n {
                        let ink = [
                            c as f64 * step,
                            m as f64 * step,
                            y as f64 * step,
                            k as f64 * step,
                        ];
                        let linear = model.from_pcs_linear(leg.xyz(ink));
                        nodes.push(linear.map(|v| v as f32));
                    }
                }
            }
        }
        let encoder = crate::icc_transform::PrecisionToneEncoder::new(model)?;
        let mut encode: [Vec<f32>; 3] =
            std::array::from_fn(|_| Vec::with_capacity(ENCODE_STEPS + 1));
        for i in 0..=ENCODE_STEPS {
            let v = i as f64 / ENCODE_STEPS as f64;
            let encoded = encoder.rgb([v, v, v]);
            for (table, value) in encode.iter_mut().zip(encoded) {
                table.push(value);
            }
        }
        Ok(Self { nodes, encode })
    }

    /// Bytes the link holds: the node table and the encode tables.
    pub(crate) fn bytes(&self) -> u64 {
        (self.nodes.capacity() * std::mem::size_of::<[f32; 3]>()
            + 3 * (ENCODE_STEPS + 1) * std::mem::size_of::<f32>()) as u64
    }

    pub(crate) fn nodes_ptr(&self) -> usize {
        self.nodes.as_ptr() as usize
    }

    /// The bytes [`Self::bytes`] answers for a built link, before it exists.
    pub(crate) fn planned_bytes() -> u64 {
        let n = VIEW_NODES as u64;
        n * n * n * n * 12 + 3 * (ENCODE_STEPS as u64 + 1) * 4
    }

    /// Encoded Display P3 for ink in [0, 1] (clamped): quadrilinear over the
    /// linear node grid, clamped to [0, 1], then the encode table.
    pub fn eval(&self, ink: [f32; 4]) -> [f32; 3] {
        let n = VIEW_NODES;
        let top = (n - 1) as f32;
        let mut base = [0usize; 4];
        let mut frac = [0f32; 4];
        for i in 0..4 {
            let v = ink[i].clamp(0.0, 1.0) * top;
            let b = (v.floor() as usize).min(n - 2);
            base[i] = b;
            frac[i] = v - b as f32;
        }
        let stride = [n * n * n, n * n, n, 1];
        let origin: usize = (0..4).map(|i| base[i] * stride[i]).sum();
        let mut linear = [0f32; 3];
        for corner in 0..16usize {
            let mut weight = 1.0f32;
            let mut at = origin;
            for (i, s) in stride.iter().enumerate() {
                if corner >> (3 - i) & 1 == 1 {
                    weight *= frac[i];
                    at += s;
                } else {
                    weight *= 1.0 - frac[i];
                }
            }
            if weight == 0.0 {
                continue;
            }
            let node = self.nodes[at];
            for c in 0..3 {
                linear[c] += weight * node[c];
            }
        }
        std::array::from_fn(|c| {
            let t = linear[c].clamp(0.0, 1.0) * ENCODE_STEPS as f32;
            let i = (t.floor() as usize).min(ENCODE_STEPS - 1);
            let f = t - i as f32;
            let table = &self.encode[c];
            table[i] + (table[i + 1] - table[i]) * f
        })
    }
}
