//! Soft proofing and the gamut warning: what the document would look like
//! printed on a given medium, rendered for the screen without touching the
//! master, the renders or the exports. The profile models are `icc` and
//! `icc_lut`; the intents, the media white and BPC are `icc_intent`.
//!
//! # The chain
//!
//! ```text
//! document RGB
//!   → PCS                   (the document's own A2B or matrix, RELATIVE)
//!   → proof device          (the proof's B2A at `setup.intent`, BPC per flag)
//!   → PCS                   (the proof's A2B, relative — ABSOLUTE when
//!                            `simulate_paper_white`, which is what puts the
//!                            paper's own cast back in)
//!   → output RGB            (the document's profile when it can encode,
//!                            else the built-in sRGB; relative, with BPC
//!                            UNLESS `simulate_ink_black`)
//! ```
//!
//! The last BPC is inverted on purpose. Off by default the proof's raised
//! black is mapped back onto the display's black, which is what a proof
//! normally wants; `simulate_ink_black` turns the mapping OFF so the ink's
//! actual black shows as the washed-out grey it really is.
//!
//! The result is TAGGED with the space it is in ([`ProofSpace`]) and handed
//! to the host, which draws it through CoreGraphics — so the monitor profile
//! is applied by the window server, exactly as it is for every other image
//! this app draws, and there is deliberately no monitor compensation in the
//! core.
//!
//! # The gamut rule, stated once
//!
//! A pixel is OUT OF GAMUT when the CIE76 ΔE\*ab (D50) between its own Lab
//! and the Lab of its proof ROUND TRIP exceeds [`GAMUT_DELTA_E`] (5.0). The
//! round trip is the proof's `B2A1` followed by its `A2B1` — **relative
//! colorimetric always**, no BPC and no media-white scaling, independent of
//! `setup.intent`. That is Little-CMS's `_cmsCreateGamutCheckPipeline` rule
//! and it is the only rule that answers the question actually being asked:
//! "can this medium hold this colour at all". Running the round trip at the
//! SETUP's intent instead would flag perfectly printable colours under
//! perceptual, whose whole job is to move in-gamut colours.
//!
//! The overlay is `setup.gamut_color` written opaque over the proofed pixel;
//! its default is 50 % grey.
//! Alpha is copied from the source in every case: alpha is coverage, not
//! colour.
//!
//! `soft_proof` is document -> PCS -> proof device -> PCS -> display, with the gamut rule stated
//! once: CIE76 dE above 5.0 between a colour's Lab and the Lab of its proof round trip, at RELATIVE
//! COLORIMETRIC ALWAYS and independent of the setup's intent. Absolute scales by the two media
//! whites on the way IN; without that it would only pick relative's table.

use std::collections::HashMap;
use std::sync::Arc;

use image::RgbaImage;
use serde_json::{json, Value};

use crate::cmyk_color::{document_xyz, ToDevice};
use crate::icc::IccProfile;
use crate::icc_black_point::{destination_black_point, proof_black_point};
use crate::icc_intent::{self, RenderingIntent};
use crate::icc_lut::{self, ProofProfile, D50};
use crate::icc_transform::PrecisionToneEncoder;
use crate::job_control::JobContext;
use crate::precision_image::{reserved_vec, PrecisionImage};

/// How far a colour may sit from its own proof round trip and still count as
/// printable. 5.0 ΔE76 is roughly "a difference a trained eye sees side by
/// side"; below it the medium is holding the colour, above it the medium is
/// substituting a different one. lcms's own gamut check uses the same order
/// of magnitude.
pub const GAMUT_DELTA_E: f64 = 5.0;

/// The most distinct input colours the per-pixel memo keeps. A proof runs
/// four LUT evaluations per colour, and a photograph repeats colours heavily;
/// past this many distinct ones the memo stops GROWING (it keeps answering
/// from what it has) so a synthetic gradient cannot turn a proof into a
/// hundred-megabyte allocation.
pub(crate) const MEMO_CAPACITY: usize = 1 << 20;

/// Peak bytes `Proofer::memo` can hold over `pixels` pixels. The memo grows
/// one insert at a time to at most `MEMO_CAPACITY` entries, so its table ends
/// at hashbrown's bucket count for that many: 4 below 4, 8 below 8, else the
/// next power of two of n·8/7 (the 7/8 load factor). Each bucket is one
/// entry plus 1 control byte; ×3/2 because the half-size table is still live
/// while the last grow rehashes into the new one; +64 for the two tables'
/// control-group tails. At the cap this is 78,643,264 B (75 MiB).
pub(crate) fn memo_peak_bytes(pixels: u64) -> u64 {
    memo_peak_bytes_for::<([f32; 3], f32)>(pixels)
}

/// [`memo_peak_bytes`] for a `MEMO_CAPACITY`-capped `HashMap<u64, V>` of
/// another value type `V` (`cmyk_color::RgbToCmyk`'s `[f64; 4]`): the same
/// bucket arithmetic over that entry's own size.
pub(crate) fn memo_peak_bytes_for<V>(pixels: u64) -> u64 {
    let n = pixels.min(MEMO_CAPACITY as u64);
    let buckets = match n {
        0..=3 => 4,
        4..=7 => 8,
        _ => (n * 8 / 7).next_power_of_two(),
    };
    let entry = std::mem::size_of::<(u64, V)>() as u64 + 1;
    buckets * entry * 3 / 2 + 64
}

/// Which space a [`ProofOutput`]'s pixels are in, so the host can tag the
/// image it draws.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProofSpace {
    /// The document's own profile.
    Document,
    /// The built-in sRGB, used when the document's profile has no way back
    /// from PCS (a LUT profile with no B2A table).
    Srgb,
    /// The built-in Display P3: a CMYK document's proof (`cmyk_proof`), the
    /// space its view is in. Only that proof reports it.
    DisplayP3,
}

impl ProofSpace {
    pub fn name(self) -> &'static str {
        match self {
            ProofSpace::Document => "document",
            ProofSpace::Srgb => "srgb",
            ProofSpace::DisplayP3 => "display-p3",
        }
    }
}

/// Everything a proof is parameterized by. Pure VIEW state: nothing here is
/// persisted in a document, undone, or recorded as an Action step.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ProofSetup {
    pub intent: RenderingIntent,
    pub black_point_compensation: bool,
    pub simulate_paper_white: bool,
    pub simulate_ink_black: bool,
    pub gamut_warning: bool,
    pub gamut_color: [u8; 3],
    /// Preserve CMYK Numbers (#1 phase 5): a CMYK document's ink is sent
    /// to the proof profile as it is, unconverted (`cmyk_proof`). Refused on
    /// an RGB document.
    pub preserve_numbers: bool,
    /// One plate of the proof device (0–3 = cyan, magenta, yellow, black),
    /// shown as the Channels panel shows a channel: grey, ink dark. Refused
    /// with a proof profile that is not CMYK.
    pub plate: Option<u8>,
}

/// The plate names a setup's `plate` key takes, in device order.
const PLATES: [&str; 4] = ["cyan", "magenta", "yellow", "black"];

impl Default for ProofSetup {
    fn default() -> Self {
        ProofSetup {
            intent: RenderingIntent::RelativeColorimetric,
            black_point_compensation: true,
            simulate_paper_white: false,
            simulate_ink_black: false,
            gamut_warning: false,
            gamut_color: [128, 128, 128],
            preserve_numbers: false,
            plate: None,
        }
    }
}

impl ProofSetup {
    /// Parses the setup object. Every key is optional; an unknown one is
    /// refused rather than ignored.
    pub fn parse(json: &str) -> Result<ProofSetup, String> {
        if json.trim().is_empty() {
            return Ok(ProofSetup::default());
        }
        let value: Value = serde_json::from_str(json)
            .map_err(|e| format!("proof setup is not valid JSON: {e}"))?;
        ProofSetup::from_value(&value)
    }

    pub fn from_value(value: &Value) -> Result<ProofSetup, String> {
        if value.is_null() {
            return Ok(ProofSetup::default());
        }
        let object = value
            .as_object()
            .ok_or("proof setup must be a JSON object")?;
        const KEYS: [&str; 8] = [
            "intent",
            "black_point_compensation",
            "simulate_paper_white",
            "simulate_ink_black",
            "gamut_warning",
            "gamut_color",
            "preserve_numbers",
            "plate",
        ];
        for key in object.keys() {
            if !KEYS.contains(&key.as_str()) {
                return Err(format!("proof setup has an unknown key \"{key}\""));
            }
        }
        let mut setup = ProofSetup::default();
        if let Some(value) = object.get("intent").filter(|v| !v.is_null()) {
            let text = value
                .as_str()
                .ok_or("proof setup intent must be a string")?;
            setup.intent = RenderingIntent::parse(text).ok_or_else(|| {
                format!(
                    "intent \"{text}\" is not one of perceptual, relative, saturation or absolute"
                )
            })?;
        }
        let flag = |key: &str, default: bool| -> Result<bool, String> {
            match object.get(key) {
                None | Some(Value::Null) => Ok(default),
                Some(Value::Bool(value)) => Ok(*value),
                Some(_) => Err(format!("proof setup {key} must be a boolean")),
            }
        };
        setup.black_point_compensation =
            flag("black_point_compensation", setup.black_point_compensation)?;
        setup.simulate_paper_white = flag("simulate_paper_white", setup.simulate_paper_white)?;
        setup.simulate_ink_black = flag("simulate_ink_black", setup.simulate_ink_black)?;
        setup.gamut_warning = flag("gamut_warning", setup.gamut_warning)?;
        if let Some(value) = object.get("gamut_color").filter(|v| !v.is_null()) {
            let text = value
                .as_str()
                .ok_or("proof setup gamut_color must be a \"#RRGGBB\" string")?;
            setup.gamut_color = parse_hex(text)?;
        }
        setup.preserve_numbers = flag("preserve_numbers", setup.preserve_numbers)?;
        if let Some(value) = object.get("plate").filter(|v| !v.is_null()) {
            let text = value.as_str().unwrap_or("");
            setup.plate = Some(
                PLATES
                    .iter()
                    .position(|name| *name == text)
                    .ok_or("proof setup plate must be cyan, magenta, yellow or black")?
                    as u8,
            );
        }
        Ok(setup)
    }

    pub fn to_json(&self) -> Value {
        json!({
            "intent": self.intent.name(),
            "black_point_compensation": self.black_point_compensation,
            "simulate_paper_white": self.simulate_paper_white,
            "simulate_ink_black": self.simulate_ink_black,
            "gamut_warning": self.gamut_warning,
            "gamut_color": format!(
                "#{:02X}{:02X}{:02X}",
                self.gamut_color[0], self.gamut_color[1], self.gamut_color[2]
            ),
            "preserve_numbers": self.preserve_numbers,
            "plate": self.plate.map(|p| PLATES[usize::from(p)]),
        })
    }
}

/// The grey a plate shows for one device ink in [0, 1]: the Channels
/// panel's `255 − round(255·ink)` (`cmyk_render`'s plane view, its master),
/// as a [0, 1] value the proofs' own `round(v·255)` encodes back exactly.
pub(crate) fn plate_grey(ink: f64) -> f32 {
    let grey = 255 - (ink.clamp(0.0, 1.0) * 255.0).round() as u8;
    f32::from(grey) / 255.0
}

/// THE gamut rule (module doc), asked of one PCS XYZ colour: the CIE76 ΔE
/// between it and its round trip through `proof`'s `B2A1` then `A2B1`,
/// relative colorimetric always. 0 when either table answers nothing. The
/// RGB proof, the CMYK document's `separate` and the Info panel's "!" all
/// ask it here.
pub(crate) fn gamut_delta(proof: &ProofProfile, source_xyz: [f64; 3]) -> f64 {
    let relative = RenderingIntent::RelativeColorimetric;
    let Some((device, count)) = proof.from_xyz(source_xyz, relative) else {
        return 0.0;
    };
    let Some(round_trip) = proof.to_xyz(&device[..count], relative) else {
        return 0.0;
    };
    icc_lut::delta_e76(
        icc_lut::xyz_to_lab(source_xyz),
        icc_lut::xyz_to_lab(round_trip),
    )
}

/// The proof chain's second half (module doc): proof device → PCS (the
/// proof's relative `A2B`, the paper white put back under
/// `simulate_paper_white`), BPC onto the output's black unless
/// `simulate_ink_black`, then the output space's encoder. The RGB proof
/// (`Proofer`) and the CMYK document's proof (`cmyk_proof`) share it.
pub(crate) struct ProofTail {
    output: Arc<IccProfile>,
    /// Proof black → output black, unless `simulate_ink_black`.
    to_output_bpc: Option<[(f64, f64); 3]>,
    /// The proof's media white over D50, for `simulate_paper_white`.
    paper: [f64; 3],
    /// The output profile's continuous encoder, built once.
    encoder: Option<PrecisionToneEncoder>,
}

impl ProofTail {
    pub(crate) fn new(
        proof: &ProofProfile,
        output: Arc<IccProfile>,
        setup: &ProofSetup,
    ) -> Result<Self, String> {
        let proof_black_relative = proof_black_point(proof, RenderingIntent::RelativeColorimetric);
        let output_black = destination_black_point(&output, RenderingIntent::RelativeColorimetric)
            .unwrap_or([0.0; 3]);
        let to_output_bpc = (!setup.simulate_ink_black)
            .then(|| icc_intent::bpc_terms(proof_black_relative, output_black));
        let white = proof.media_white();
        let paper = if setup.simulate_paper_white {
            std::array::from_fn(|i| white[i] / D50[i])
        } else {
            [1.0, 1.0, 1.0]
        };
        let encoder = output
            .model()
            .map(PrecisionToneEncoder::new)
            .transpose()
            .map_err(|e| crate::icc_encode_refusal::own_profile_error(&output, e))?;
        Ok(Self {
            output,
            to_output_bpc,
            paper,
            encoder,
        })
    }

    /// The tail for one device colour; `None` when the proof's `A2B`
    /// answers nothing.
    fn simulate(&self, proof: &ProofProfile, device: &[f64]) -> Option<[f32; 3]> {
        // Proof device → PCS. Absolute puts the paper's own white back.
        let mut simulated = proof.to_xyz(device, RenderingIntent::RelativeColorimetric)?;
        for (slot, scale) in simulated.iter_mut().zip(self.paper) {
            *slot *= scale;
        }
        if let Some(terms) = &self.to_output_bpc {
            for (slot, (a, b)) in simulated.iter_mut().zip(terms.iter()) {
                *slot = a * *slot + b;
            }
        }

        // PCS → the display space, through the encoder built once in
        // [`ProofTail::new`] — a 65537-entry table per pixel is not a proof.
        Some(match (self.output.model(), &self.encoder) {
            (Some(model), Some(encoder)) => encoder.rgb(model.from_pcs_linear(simulated)),
            _ => match self
                .output
                .lut()
                .and_then(|lut| lut.from_xyz(simulated, RenderingIntent::RelativeColorimetric))
            {
                Some((device, _)) => std::array::from_fn(|c| device[c] as f32),
                None => [0.0; 3],
            },
        })
    }

    /// The encoded output colour (unclamped) of one proof-device colour;
    /// black when the proof's `A2B` answers nothing.
    pub(crate) fn display(&self, proof: &ProofProfile, device: &[f64]) -> [f32; 3] {
        self.simulate(proof, device).unwrap_or([0.0; 3])
    }
}

fn parse_hex(text: &str) -> Result<[u8; 3], String> {
    let body = text.strip_prefix('#').unwrap_or(text);
    if body.len() != 6 || !body.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(format!("\"{text}\" is not a \"#RRGGBB\" colour"));
    }
    let mut out = [0u8; 3];
    for (i, slot) in out.iter_mut().enumerate() {
        *slot = u8::from_str_radix(&body[i * 2..i * 2 + 2], 16)
            .map_err(|_| format!("\"{text}\" is not a \"#RRGGBB\" colour"))?;
    }
    Ok(out)
}

/// What one proof render produced.
pub struct ProofOutput {
    /// The proofed pixels, in [`ProofOutput::space`].
    pub image: RgbaImage,
    pub space: ProofSpace,
    /// The per-pixel gamut mask (255 out of gamut, 0 inside), present only
    /// when the caller asked for it.
    pub mask: Option<Vec<u8>>,
    /// How many pixels the gamut rule flagged. Counted whenever the mask is
    /// built or the warning is on, 0 otherwise.
    pub out_of_gamut: u64,
}

impl ProofOutput {
    pub fn info_json(&self) -> Value {
        json!({
            "space": self.space.name(),
            "out_of_gamut": self.out_of_gamut,
        })
    }
}

/// The prepared chain: three profile models and the two affine XYZ steps
/// between them.
struct Proofer<'a> {
    document: &'a IccProfile,
    proof: &'a ProofProfile,
    space: ProofSpace,
    /// Document → proof device: the absolute media-white scale and the BPC
    /// affine (document black → proof black, when
    /// `setup.black_point_compensation`), then the proof's `B2A` at the
    /// setup's intent — the RGB → CMYK leg Image ▸ Mode runs too
    /// (`cmyk_color::ToDevice`).
    to_proof: ToDevice,
    /// Proof device → output ([`ProofTail`]).
    tail: ProofTail,
    /// The plate shown instead of the colour (`ProofSetup::plate`).
    plate: Option<usize>,
    memo: HashMap<u64, ([f32; 3], f32)>,
}

impl<'a> Proofer<'a> {
    fn new(
        document: &'a IccProfile,
        proof: &'a ProofProfile,
        setup: &ProofSetup,
    ) -> Result<Proofer<'a>, String> {
        // A Lab Color projection (#1 Lab phase 5b) proofs through its
        // Display P3 view, which the host hands in instead; the twin itself
        // is no document profile.
        if crate::lab_document::is_lab_twin(document) {
            return Err(crate::lab_document::PROOF_VIEW_REFUSAL.into());
        }
        if !document.can_decode() {
            return Err(format!(
                "the document profile \"{}\" carries no model this build can read",
                document.name()
            ));
        }
        if !proof.can_decode() || !proof.can_encode() {
            return Err(format!(
                "the proof profile \"{}\" needs both an A2B and a B2A table",
                proof.name()
            ));
        }
        if setup.preserve_numbers {
            return Err("Preserve Numbers needs a CMYK document".into());
        }
        if setup.plate.is_some() && proof.device() != icc_lut::DeviceSpace::Cmyk {
            return Err(format!(
                "the proof profile \"{}\" is not CMYK, so it has no plates",
                proof.name()
            ));
        }
        let (output, space) = if document.can_encode() {
            // `IccProfile` is not `Clone`; the document's own bytes are what
            // the output space is, so re-parse them into an owned handle
            // rather than borrowing two lifetimes into one struct.
            (
                Arc::new(
                    IccProfile::parse(document.bytes())
                        .ok_or("the document profile could not be reread")?,
                ),
                ProofSpace::Document,
            )
        } else {
            (IccProfile::srgb(), ProofSpace::Srgb)
        };
        // Each leg's SOURCE black is `cmsDetectBlackPoint`'s and its
        // DESTINATION black `cmsDetectDestinationBlackPoint`'s, as lcms's
        // `ComputeConversion` asks them (`icc_black_point`).
        let to_proof = ToDevice::new(
            document,
            proof,
            setup.intent,
            setup.black_point_compensation,
        );
        let tail = ProofTail::new(proof, output, setup)?;
        Ok(Proofer {
            document,
            proof,
            space,
            to_proof,
            tail,
            plate: setup.plate.map(usize::from),
            memo: HashMap::new(),
        })
    }

    /// The document's own PCS XYZ for one encoded RGB triple.
    fn document_xyz(&self, rgb: [f64; 3]) -> [f64; 3] {
        document_xyz(self.document, rgb)
    }

    /// The whole chain plus the gamut ΔE, for one encoded RGB triple. The
    /// ONE evaluator; [`Proofer::pixel`] is the memoized wrapper around it.
    fn evaluate(&self, rgb: [f64; 3]) -> ([f32; 3], f32) {
        let source = self.document_xyz(rgb);

        // Document → proof device. Absolute puts the SOURCE's white back
        // before the proof's own B2A reads it.
        let Some((device, count)) = self.to_proof.device(self.proof, source) else {
            return ([0.0; 3], 0.0);
        };

        // A plate is the device ink itself, ink dark (`ProofSetup::plate`).
        let out = match self.plate {
            Some(plate) => [plate_grey(device[plate]); 3],
            None => match self.tail.simulate(self.proof, &device[..count]) {
                Some(out) => out,
                None => return ([0.0; 3], 0.0),
            },
        };

        // The gamut question is asked of the ORIGINAL colour, always at the
        // colorimetric pair (module doc).
        (out, gamut_delta(self.proof, source) as f32)
    }

    /// The memoized evaluation. A proof runs four LUT lookups per distinct
    /// colour and a photograph repeats colours heavily, so this is the
    /// difference between an interactive proof and a stall.
    fn pixel(&mut self, key: u64, rgb: [f64; 3]) -> ([f32; 3], f32) {
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

/// Proofs one already flattened byte projection. `document` is the profile
/// its numbers are in.
pub(crate) fn proof_render8(
    flat: &RgbaImage,
    document: &IccProfile,
    proof: &ProofProfile,
    setup: &ProofSetup,
    job: JobContext<'_>,
) -> Result<ProofOutput, String> {
    proof_render8_with(flat, document, proof, setup, setup.gamut_warning, job)
}

/// The same render with the mask requested independently of the warning
/// flag — what `dr_proof_gamut_mask` needs.
pub(crate) fn proof_render8_with(
    flat: &RgbaImage,
    document: &IccProfile,
    proof: &ProofProfile,
    setup: &ProofSetup,
    want_mask: bool,
    job: JobContext<'_>,
) -> Result<ProofOutput, String> {
    let mut proofer = Proofer::new(document, proof, setup)?;
    let (w, h) = flat.dimensions();
    let mut bytes = reserved_vec(flat.as_raw().len())?;
    let mut mask = if want_mask {
        let mut mask = reserved_vec((w as usize) * (h as usize))?;
        mask.resize((w as usize) * (h as usize), 0u8);
        Some(mask)
    } else {
        None
    };
    let mut out_of_gamut = 0u64;
    for (i, pixel) in flat.pixels().enumerate() {
        job.every(i)?;
        let key = u64::from(pixel[0]) << 16 | u64::from(pixel[1]) << 8 | u64::from(pixel[2]);
        let rgb = [
            f64::from(pixel[0]) / 255.0,
            f64::from(pixel[1]) / 255.0,
            f64::from(pixel[2]) / 255.0,
        ];
        let (out, delta) = proofer.pixel(key, rgb);
        let outside = f64::from(delta) > GAMUT_DELTA_E;
        if outside {
            out_of_gamut += 1;
            if let Some(mask) = &mut mask {
                mask[i] = 255;
            }
        }
        let encoded = if outside && setup.gamut_warning {
            setup.gamut_color
        } else {
            std::array::from_fn(|c| (out[c].clamp(0.0, 1.0) * 255.0).round() as u8)
        };
        bytes.extend_from_slice(&[encoded[0], encoded[1], encoded[2], pixel[3]]);
    }
    let image = RgbaImage::from_raw(w, h, bytes).ok_or("the proof produced invalid dimensions")?;
    Ok(ProofOutput {
        image,
        space: proofer.space,
        mask,
        out_of_gamut,
    })
}

/// Proofs an already captured typed projection into an RGBA8 DISPLAY image —
/// the extra depth protects the chain's arithmetic, not the screen, which
/// has eight bits either way.
pub(crate) fn proof_render16(
    projection: &PrecisionImage,
    proof: &ProofProfile,
    setup: &ProofSetup,
    job: JobContext<'_>,
) -> Result<ProofOutput, String> {
    proof_render16_with(projection, proof, setup, setup.gamut_warning, job)
}

pub(crate) fn proof_render16_with(
    projection: &PrecisionImage,
    proof: &ProofProfile,
    setup: &ProofSetup,
    want_mask: bool,
    job: JobContext<'_>,
) -> Result<ProofOutput, String> {
    let document = projection.profile();
    let mut proofer = Proofer::new(document, proof, setup)?;
    let (w, h) = projection.dimensions();
    let runs = projection
        .runs_rgba16()
        .ok_or("a typed proof requires RGBA16 UNORM storage")?;
    let n = w as usize * h as usize;
    let mut bytes = reserved_vec(n * 4)?;
    let mut mask = if want_mask {
        let mut mask = reserved_vec(n)?;
        mask.resize(n, 0u8);
        Some(mask)
    } else {
        None
    };
    let mut out_of_gamut = 0u64;
    for (i, pixel) in runs.flatten().enumerate() {
        job.every(i)?;
        let key = u64::from(pixel[0]) << 32 | u64::from(pixel[1]) << 16 | u64::from(pixel[2]);
        let rgb = [
            f64::from(pixel[0]) / 65535.0,
            f64::from(pixel[1]) / 65535.0,
            f64::from(pixel[2]) / 65535.0,
        ];
        let (out, delta) = proofer.pixel(key, rgb);
        let outside = f64::from(delta) > GAMUT_DELTA_E;
        if outside {
            out_of_gamut += 1;
            if let Some(mask) = &mut mask {
                mask[i] = 255;
            }
        }
        let encoded: [u8; 3] = if outside && setup.gamut_warning {
            setup.gamut_color
        } else {
            std::array::from_fn(|c| (out[c].clamp(0.0, 1.0) * 255.0).round() as u8)
        };
        // The typed projection's alpha reduces exactly as every other byte
        // preview does: one round, no dither.
        let alpha = ((f64::from(pixel[3]) / 65535.0) * 255.0).round() as u8;
        bytes.extend_from_slice(&[encoded[0], encoded[1], encoded[2], alpha]);
    }
    let image = RgbaImage::from_raw(w, h, bytes).ok_or("the proof produced invalid dimensions")?;
    Ok(ProofOutput {
        image,
        space: proofer.space,
        mask,
        out_of_gamut,
    })
}
