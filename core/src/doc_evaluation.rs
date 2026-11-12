//! Versioned evaluation policy (Phase 11E): the per-document choice between
//! `encoded` — the unchanged default, in which the projection composites the
//! stored code values directly — and `linear_light`, in which every sample is
//! DECODED through the document profile's tone reproduction curve, blended and
//! mixed in linear light at f32, and re-encoded exactly once at the output.
//!
//! # The projection is the invariant
//!
//! A policy is chosen deliberately (Image ▸ Evaluation… / `set_evaluation`) and
//! only ever holds for a stack this phase actually evaluates in linear light.
//! [`DrDocument::linear_light_refusal`] is the ONE walk that decides that, and
//! it runs at three places: at the switch, at every later edit that would
//! introduce an unmigrated operator (the host's `applyEdit` guard and the MCP
//! pre-check), and — as the backstop nothing can route around —
//! [`DrDocument::transfer`], which every `composite_env` caller obtains its
//! transfer from. A document carrying an unmigrated operator therefore cannot
//! render at all under this policy; it reports the refusal by name.
//!
//! An ENCODED document holds no transfer (`linear_light: None`), takes the
//! `transfer == None` branch everywhere and is byte-for-byte what it was
//! before this module existed; `linear_light_refusal` returns `None` before it
//! walks anything.
//!
//! # The tables
//!
//! Both transfers are built ONCE per document — at the switch, at open, at a
//! depth conversion and at a profile assign/convert ([`DrDocument::rebind_evaluation`])
//! — and held behind an `Arc` on the document, never per projection. The byte
//! table is a 256-entry decode per channel (≈6 KB); the typed one a
//! 65536-entry decode per channel plus the shared continuous encoder
//! (≈2.41 MB, counted by resource accounting). Both are filled from the ONE
//! forward curve evaluator (`icc_transform::Curve::eval`) in f64, and neither
//! direction adds a curve inverter: re-encoding goes through the existing
//! [`ToneEncoder`] threshold search at byte depth and through
//! [`PrecisionToneEncoder`] at typed depth.
//!
//! A retained source is evaluated like any raster (phase 187a0): no compositor reads the binding, so the layer composites its
//! rendered fallback, and its re-render on transform or replace resamples in
//! the stored encoding, the domain raster Free Transform (`doc_transform`) and
//! warp (`doc_warp`) already use in a linear-light document.
//!
//! The typed twin ([`TypedTransfer`]) and the `PrecisionDocument` half of the
//! policy live in `precision_evaluation`; the `.rz` wrapper in
//! `evaluation_native`; the exports in `ffi_evaluation`.
//!
//! `ByteTransfer` (256-entry decode per channel + the existing `ToneEncoder`, ≈6 KB) and
//! `TypedTransfer` (65536-entry decode + `PrecisionToneEncoder`, ≈2.41 MB, recorded by
//! `PrecisionDocument::record_resources` and `PrecisionScene::plan`) are filled from the ONE
//! forward curve evaluator and built ONCE per document — at the switch, at the two native readers,
//! at depth conversion and at `rebind_evaluation` (every profile write) — never inside a render,
//! plan, export or composite. **The projection is the invariant**: `DrDocument::transfer` /
//! `PrecisionDocument::transfer` runs `linear_light_refusal`'s walk and is where every
//! `composite_env` caller and `scene_cut` obtains its transfer, so an unmigrated operator added
//! after the switch refuses at the render as well as at the edit. `linear_light_refusal` returns
//! `None` before walking anything on an encoded document, and `encode_projection(.., None)` IS
//! `doc::quantize_with_job` statement for statement — the encoded path is byte-for-byte what it was. The
//! adjustment TRANSCODE rule (`adjusted = decode(kernel(encode(backdrop)))`, then the `k` lerp in
//! linear) is stated on `ByteTransfer::encode_f32`, and its byte double quantization is disclosed
//! and measured at 2 codes.

use std::sync::Arc;

use image::RgbaImage;
use serde_json::json;

use crate::doc::DrDocument;
use crate::icc::IccProfile;
use crate::icc_transform::{MatrixTrc, ToneEncoder};
use crate::job_control::JobContext;

/// How a document's projection evaluates its stack.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Evaluation {
    /// Composite the stored code values directly — every build before 11E,
    /// and still the default for every document.
    #[default]
    Encoded,
    /// Decode through the document profile's TRC, composite in linear light
    /// at f32, re-encode once at the output.
    LinearLight,
}

impl Evaluation {
    /// The wire name (`set_evaluation`, the archive, the operator table).
    pub fn name(self) -> &'static str {
        match self {
            Evaluation::Encoded => "encoded",
            Evaluation::LinearLight => "linear_light",
        }
    }

    /// The wire name read back; `None` for anything else.
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "encoded" => Some(Evaluation::Encoded),
            "linear_light" => Some(Evaluation::LinearLight),
            _ => None,
        }
    }

    /// The `DR_EVALUATION_*` value for the FFI.
    pub fn code(self) -> i32 {
        match self {
            Evaluation::Encoded => 0,
            Evaluation::LinearLight => 1,
        }
    }

    /// The `DR_EVALUATION_*` value read back; `None` for anything else.
    pub fn from_code(c: i32) -> Option<Self> {
        match c {
            0 => Some(Evaluation::Encoded),
            1 => Some(Evaluation::LinearLight),
            _ => None,
        }
    }

    /// The typed descriptor's `evaluation` tag. The encoded spelling is the
    /// one shipped since 7B and is unchanged.
    pub fn typed_tag(self) -> &'static str {
        match self {
            Evaluation::Encoded => "encoded_rgb_f32_v1",
            Evaluation::LinearLight => "linear_light_f32_v1",
        }
    }
}

/// The refusal a profile this crate cannot evaluate in linear light carries.
/// One sentence at both depths: the policy needs a forward TRC per channel
/// that is finite and monotone, which is exactly `validate_precision`.
pub(crate) fn unmodellable_profile_sentence(name: &str) -> String {
    format!(
        "“{name}” has no matrix/TRC model this build can decode, so linear-light evaluation \
         cannot be used with it. Convert to a matrix/TRC profile first, or keep encoded evaluation."
    )
}

/// The profile's matrix/TRC model when linear-light evaluation can run in
/// it, else the refusal: no model at all names the profile
/// ([`unmodellable_profile_sentence`]); a model whose curves are not finite
/// and monotone carries `validate_precision`'s sentence. Both transfers are
/// built through it, and a Convert or Assign into a profile it refuses is
/// refused up front (`doc_color_destination`).
pub(crate) fn linear_light_model(profile: &IccProfile) -> Result<&MatrixTrc, String> {
    let model = profile
        .model()
        .ok_or_else(|| unmodellable_profile_sentence(profile.name()))?;
    model.validate_precision()?;
    Ok(model)
}

/// The refusal a document whose held transfer does not belong to its CURRENT
/// profile carries — the net for a profile write that missed its rebind. It
/// is a defect report, not a user-facing choice, so it says what to do.
pub(crate) fn stale_transfer_sentence() -> String {
    "This document's linear-light transfer does not belong to its colour profile. \
     Switch Image ▸ Evaluation back to Encoded and choose it again."
        .to_string()
}

/// What the projection evaluates in linear light today.
const MIGRATED: &[&str] = &[
    "normal",
    "multiply",
    "screen",
    "overlay",
    "soft_light",
    "hard_light",
    "darken",
    "lighten",
    "difference",
    "exclusion",
    "color_dodge",
    "color_burn",
    "addition",
    "subtract",
    "linear_burn",
    "vivid_light",
    "linear_light",
    "pin_light",
    "hard_mix",
    "divide",
    "layer_opacity",
    "layer_mask",
    "pass_through_group",
    "isolated_group",
    "clip_group",
    "pointwise_adjustment_layer",
    "text_raster",
    "shape_raster",
    "export",
    "preview",
    "history",
    "snapshot",
    "recovery",
    "assign_profile",
    "convert_profile",
    "depth_conversion",
    "retained_source",
    "darker_color",
    "lighter_color",
    "hue",
    "saturation",
    "color",
    "luminosity",
    "dissolve",
    "guided_shadows_highlights",
    "layer_style",
    "blend_if",
    "fill_opacity",
    "knockout",
];

/// What it refuses, with the phase that owns each. Phase 2 is resampling.
const REFUSED: &[(&str, u32)] = &[("Image Size", 2)];

/// The operator table as JSON, for the host's Evaluation sheet and the MCP
/// `get_evaluation` reply:
/// `{"migrated":[..],"refused":[{"operator":..,"phase":..}],"resize_refusal":".."}`.
/// The resize sentence travels HERE rather than being written a second time on
/// the host side, so the guard and the MCP pre-check cannot drift apart.
pub fn operators_json() -> String {
    json!({
        "migrated": MIGRATED,
        "refused": REFUSED
            .iter()
            .map(|(operator, phase)| json!({"operator": operator, "phase": phase}))
            .collect::<Vec<_>>(),
        "resize_refusal": resize_refusal_sentence(),
    })
    .to_string()
}

/// The refusal wording `Image Size` carries at either depth — the one place
/// its sentence is written, so the host's guard and the MCP pre-check say the
/// same thing.
pub fn resize_refusal_sentence() -> String {
    "Image Size resamples in the stored encoding, which linear-light evaluation does not \
     support yet. Switch Image ▸ Evaluation back to Encoded to resize."
        .to_string()
}

// --------------------------------------------------------------- transfer --

/// The byte document's transfer tables: one 256-entry decode per channel and
/// the shared forward-threshold encoder, both built from the profile's own
/// TRCs. ≈6 KB, built once per document.
pub struct ByteTransfer {
    profile: Arc<IccProfile>,
    /// `decode8[c][v]` is channel `c`'s LINEAR value for stored code `v`.
    decode8: [[f32; 256]; 3],
    /// `document_v2`'s encoder, the existing `icc_transform` threshold search.
    encoder: ToneEncoder,
}

impl ByteTransfer {
    /// Builds the tables for `profile`. `Err` — a REFUSAL the host shows — for
    /// a profile with no matrix/TRC model and for one whose curves are not
    /// finite and monotone (`validate_precision`, the same gate the typed
    /// depth already applies).
    pub fn for_profile(profile: &Arc<IccProfile>) -> Result<Self, String> {
        let model = linear_light_model(profile)
            .map_err(|e| crate::icc_encode_refusal::own_profile_error(profile, e))?;
        let mut decode8 = [[0.0f32; 256]; 3];
        for v in 0..256usize {
            let x = v as f32 / 255.0;
            let linear = model.decode_rgb([x, x, x]);
            for (channel, slot) in decode8.iter_mut().enumerate() {
                slot[v] = linear[channel] as f32;
            }
        }
        Ok(Self {
            profile: Arc::clone(profile),
            decode8,
            encoder: ToneEncoder::new(model),
        })
    }

    /// The profile these tables were built from — the staleness check's
    /// subject (`Arc::ptr_eq` against the document's own handle).
    pub fn profile(&self) -> &Arc<IccProfile> {
        &self.profile
    }

    /// One straight encoded RGB triple in [0, 1] as LINEAR components.
    ///
    /// On a LATTICE value (`v · 255` integral — every stored byte) this is the
    /// table entry exactly. Off the lattice — the accumulator a group boundary
    /// or a styled bake hands back — it interpolates linearly between the two
    /// neighbouring entries, which is the same rule
    /// [`crate::precision_evaluation::TypedTransfer::decode`] applies at 16
    /// bits, so the two depths answer one question one way.
    pub fn decode(&self, rgb: [f32; 3]) -> [f32; 3] {
        std::array::from_fn(|c| {
            let position = rgb[c].clamp(0.0, 1.0) * 255.0;
            let low = position.floor() as usize;
            let high = position.ceil() as usize;
            let table = &self.decode8[c];
            let (low, high) = (table[low.min(255)], table[high.min(255)]);
            low + (high - low) * (position - position.floor())
        })
    }

    /// One LINEAR triple as the stored 8-bit codes, through the existing
    /// forward-threshold search (`icc_transform`'s module doc: no inverter).
    pub fn encode(&self, linear: [f32; 3]) -> [u8; 3] {
        std::array::from_fn(|c| self.encoder.channel(c, linear[c]))
    }

    /// [`Self::encode`] as a LATTICE [0, 1] value — what the adjustment
    /// transcode hands an op's kernel, so the kernel sees exactly the number
    /// the encoded path shows it.
    pub fn encode_f32(&self, linear: [f32; 3]) -> [f32; 3] {
        self.encode(linear).map(|code| f32::from(code) / 255.0)
    }
}

/// Quantizes a straight-alpha f32 accumulator to RGBA8 under `transfer`:
/// `None` is [`crate::doc::quantize_with_job`] statement for statement (an encoded
/// document's projection is untouched), `Some` re-encodes RGB through the
/// profile's curves once and rounds alpha, which is coverage and never
/// colour. The `NONE` wrapper of [`encode_projection_with_job`].
pub(crate) fn encode_projection(
    acc: &[[f32; 4]],
    w: u32,
    h: u32,
    transfer: Option<&ByteTransfer>,
) -> RgbaImage {
    encode_projection_with_job(acc, w, h, transfer, JobContext::NONE)
        .expect("uncancellable context")
}

/// [`encode_projection`] polling `job` at the head of every row (Phase
/// 13A-5b-ii-b): `None` is [`crate::doc::quantize_with_job`], `Some` the same
/// row-major samples through the same [`encode_sample`].
pub(crate) fn encode_projection_with_job(
    acc: &[[f32; 4]],
    w: u32,
    h: u32,
    transfer: Option<&ByteTransfer>,
    job: JobContext<'_>,
) -> Result<RgbaImage, String> {
    let Some(transfer) = transfer else {
        return crate::doc::quantize_with_job(acc, w, h, job);
    };
    let mut out = RgbaImage::new(w, h);
    if w == 0 || h == 0 {
        return Ok(out);
    }
    let row = w as usize;
    for (dst_row, src_row) in out.chunks_exact_mut(row * 4).zip(acc.chunks(row)) {
        job.check()?;
        for (dst, src) in dst_row.chunks_exact_mut(4).zip(src_row) {
            dst.copy_from_slice(&encode_sample(*src, Some(transfer)));
        }
    }
    Ok(out)
}

/// One pixel of [`encode_projection`] — the MASTER per-pixel body, moved here
/// unchanged so the regional compositor (`doc_regional_tree`) encodes an
/// isolated group's sample at its boundary exactly as `rendered_group` does.
/// `None` is `doc::quantize_sample`, the encoded path.
#[inline]
pub(crate) fn encode_sample(px: [f32; 4], transfer: Option<&ByteTransfer>) -> [u8; 4] {
    let Some(transfer) = transfer else {
        return crate::doc::quantize_sample(px);
    };
    let mut out = [0u8; 4];
    let rgb = transfer.encode([px[0], px[1], px[2]]);
    out[..3].copy_from_slice(&rgb);
    out[3] = (px[3].clamp(0.0, 1.0) * 255.0).round() as u8;
    out
}

/// One COMPOSITED sample as the encoded numbers a rule authored on encoded
/// values reads: `None` is the sample as it stands; under `linear_light` its
/// RGB is re-encoded to the LATTICE value the projection would store
/// ([`ByteTransfer::encode_f32`]) and its alpha, which is coverage, kept.
/// Two readers, one rule (the adjustment transcode applied to a kernel's
/// other inputs):
///
/// * the Shadows/Highlights GUIDE is built from the backdrop's encoded view,
///   so over an opaque lattice backdrop a guided adjustment at full opacity
///   equals the encoded result and only its `k` lerp is linear;
/// * BLEND IF reads the Underlying composite (and a composited This Layer)
///   through it: its ranges are authored in display levels, so a range
///   selects the pixels it selects in the encoded document over the same
///   picture. A stored sample is already encoded and is read as it is.
pub(crate) fn encoded_view(px: [f32; 4], transfer: Option<&ByteTransfer>) -> [f32; 4] {
    match transfer {
        None => px,
        Some(transfer) => {
            let rgb = transfer.encode_f32([px[0], px[1], px[2]]);
            [rgb[0], rgb[1], rgb[2], px[3]]
        }
    }
}

// ------------------------------------------------------------ the policy --

impl DrDocument {
    /// This document's evaluation policy — held state, not a guess: the
    /// transfer's presence IS the policy.
    pub fn evaluation(&self) -> Evaluation {
        if self.linear_light.is_some() {
            Evaluation::LinearLight
        } else {
            Evaluation::Encoded
        }
    }

    /// The document under `policy`. `Ok(None)` when it is already in that
    /// policy — the core's no-op rule, so the host registers no phantom undo
    /// step; `Err` is the REFUSAL (a profile with no finite monotone
    /// matrix/TRC model) named in full.
    pub fn with_evaluation(&self, policy: Evaluation) -> Result<Option<Self>, String> {
        if self.evaluation() == policy {
            return Ok(None);
        }
        let mut out = self.clone();
        match policy {
            Evaluation::Encoded => out.linear_light = None,
            Evaluation::LinearLight => {
                out.linear_light = Some(Arc::new(ByteTransfer::for_profile(&self.profile)?));
                if let Some(reason) = out.linear_light_refusal() {
                    return Err(reason);
                }
            }
        }
        Ok(Some(out))
    }

    /// Why this document's projection cannot evaluate in linear light, as the
    /// sentence the host shows: a held transfer that does not belong to the
    /// current profile (a profile write that missed its rebind). `None` —
    /// before anything is compared — for an ENCODED document, and for every
    /// linear-light stack whose tables are current: every operator is
    /// evaluated (P7-24).
    pub fn linear_light_refusal(&self) -> Option<String> {
        let transfer = self.linear_light.as_ref()?;
        (!Arc::ptr_eq(transfer.profile(), &self.profile)).then(stale_transfer_sentence)
    }

    /// The transfer this document's projection composites through: `Ok(None)`
    /// for an encoded document, `Err` for a linear-light one whose transfer
    /// is stale ([`Self::linear_light_refusal`]). EVERY projection obtains it
    /// here, which is what makes the staleness net the invariant rather than
    /// a check a later edit could route around.
    pub(crate) fn transfer(&self) -> Result<Option<&ByteTransfer>, String> {
        let Some(transfer) = self.linear_light.as_ref() else {
            return Ok(None);
        };
        match self.linear_light_refusal() {
            Some(reason) => Err(reason),
            None => Ok(Some(transfer)),
        }
    }

    /// Rebuilds the transfer when the document's profile has been replaced.
    /// Called by every profile write (assign, convert) right after the new
    /// profile lands, so the staleness net in
    /// [`Self::linear_light_refusal`] never has to fire. Every one of them
    /// reaches it, so it first re-bakes each abstract Color Lookup look
    /// through the new profile (`doc_look`); a failed re-bake fails
    /// the write.
    pub(crate) fn rebind_evaluation(&mut self) -> Result<(), String> {
        self.rebake_looks()?;
        let Some(transfer) = self.linear_light.as_ref() else {
            return Ok(());
        };
        if Arc::ptr_eq(transfer.profile(), &self.profile) {
            return Ok(());
        }
        self.linear_light = Some(Arc::new(ByteTransfer::for_profile(&self.profile)?));
        Ok(())
    }
}
