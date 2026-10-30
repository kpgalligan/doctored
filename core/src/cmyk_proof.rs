//! A CMYK document's soft proof and the one-colour separation (#547 phase
//! 5): View ▸ Proof Setup on a CMYK document (Custom with Preserve CMYK
//! Numbers, Working CMYK and its four plates), and [`separate`], the Info
//! panel's CMYK readout of an RGB colour with its out-of-gamut "!" and the
//! Color Picker's gamut alarm.
//!
//! # The proof
//!
//! Each pixel of the composite (`cmyk_stack`, the one compositor) goes to
//! the proof device: its own ink when `preserve_numbers` (Preserve CMYK
//! Numbers sends the numbers to the press unconverted), else
//! through `cmyk_color::CmykToCmyk` from the document's profile to the proof
//! profile at the setup's intent and BPC. The device colour then takes the
//! RGB proof's own second half, `soft_proof::ProofTail` (Simulate Paper
//! Color and Simulate Black Ink as there), into the built-in Display P3 —
//! the space a CMYK document's view is in — reported as `"display-p3"`. A
//! plate shows the device's one ink as the Channels panel shows a channel,
//! `255 − round(255·ink)` (`soft_proof::plate_grey`), of the composite on
//! paper (`cmyk_composite::on_paper`), opaque; a colour proof keeps the
//! composite's alpha, as the view does.
//!
//! The proof profile must be CMYK ("a CMYK document proofs through a CMYK
//! profile"). Results are memoized per distinct 16-bit ink, capped at
//! `MEMO_CAPACITY`, the peak charged by the plan.
//!
//! # Gamut Warning
//!
//! View ▸ Gamut Warning on a CMYK document flags the inks its PROOF profile
//! cannot hold, the owner's 2026-10-14 request on #547 for a Gamut Warning
//! on a CMYK document against its proof profile. A pixel is
//! outside when the proof converts its numbers (`!preserve_numbers`) and
//! `soft_proof::gamut_delta` of the ink's PCS colour (the document profile's
//! relative-colorimetric `A2B`) against the proof profile exceeds
//! `GAMUT_DELTA_E` — `soft_proof`'s ONE gamut rule, relative colorimetric
//! always, whatever the setup's intent. An outside pixel shows
//! `setup.gamut_color` opaque over the proofed colour (or plate), alpha
//! kept, and the info's `out_of_gamut` counts the outside pixels whether or
//! not the warning is shown, as the RGB proof's does. A proof that preserves
//! the document's numbers sends them to the press unconverted and flags
//! nothing; so does the working CMYK, whose setup the host reconciles to
//! preserve them.
//!
//! # Separate
//!
//! [`separate`] takes one encoded RGB colour in an RGB profile: its PCS XYZ
//! (`cmyk_color::document_xyz`), the `ToDevice` leg into the CMYK profile
//! (Image ▸ Mode's leg, so the readout is the ink a conversion would write),
//! the gamut verdict `soft_proof::gamut_delta > GAMUT_DELTA_E` (the ONE
//! rule, relative colorimetric always), and the in-gamut colour: that ink
//! back through `CmykToRgb` into the same RGB profile, clamped — the
//! Picker's "click to bring into gamut" swatch.

use std::collections::HashMap;

use image::RgbaImage;
use serde_json::{json, Value};

use crate::cmyk_color::{
    document_xyz, ink_key, require_encodable, CmykToCmyk, CmykToRgb, ConversionOptions, ToDevice,
};
use crate::cmyk_composite::{code, on_paper, unit};
use crate::icc::IccProfile;
use crate::icc_intent::RenderingIntent;
use crate::icc_lut::{DeviceSpace, ProofProfile};
use crate::job_control::{DrJobControl, JobContext};
use crate::precision_document::PrecisionDocument;
use crate::precision_image::{admit, pixel_count, reserved_vec, BOOKKEEPING_BYTES};
use crate::soft_proof::{
    gamut_delta, memo_peak_bytes_for, plate_grey, ProofSetup, ProofSpace, ProofTail, GAMUT_DELTA_E,
    MEMO_CAPACITY,
};

/// Bytes a canvas pixel the proof charges beside the stack, the memo and
/// the profile: the RGBA8 result (4 B/px), the host's copy of it and its
/// display preparation. Fitted to `scripts/cmyk-admission/measure.py`
/// (`proof-cmyk-16`); a miss raises this term.
pub(crate) const PROOF_BYTES_PER_PIXEL: u64 = 16;

/// The proof's refusal of a proof profile that is not CMYK.
fn not_cmyk(proof: &ProofProfile) -> String {
    format!(
        "the proof profile \"{}\" is not CMYK; a CMYK document proofs through a CMYK profile",
        proof.name()
    )
}

/// The parsed proof inputs, or the refusal (module doc).
fn prepare(proof_icc: &[u8], setup: &str) -> Result<(ProofProfile, ProofSetup), String> {
    if proof_icc.len() > crate::drdc::MAX_DRDC_BLOB_LEN as usize {
        return Err("the proof profile exceeds the 16 MiB limit".into());
    }
    let proof = ProofProfile::parse(proof_icc)?;
    let setup = ProofSetup::parse(setup)?;
    if proof.device() != DeviceSpace::Cmyk {
        return Err(not_cmyk(&proof));
    }
    if !proof.can_decode() {
        return Err(format!(
            "the proof profile \"{}\" has no A2B (device-to-PCS) table",
            proof.name()
        ));
    }
    Ok((proof, setup))
}

impl PrecisionDocument {
    /// The proof's working memory (module doc) and its refusal.
    pub(crate) fn cmyk_proof_scratch(
        &self,
        proof_icc: &[u8],
        setup: &str,
    ) -> (u64, Option<String>) {
        let (w, h) = self.dimensions();
        let pixels = u64::from(w) * u64::from(h);
        let stack = self.cmyk_stack_scratch_bytes().unwrap_or(u64::MAX / 4);
        let base = pixels.saturating_mul(PROOF_BYTES_PER_PIXEL)
            + u64::from(w) * 20
            + stack
            + memo_peak_bytes_for::<([u8; 3], bool)>(pixels)
            + BOOKKEEPING_BYTES;
        let policy = match self.require_cmyk_policy("A CMYK proof") {
            Ok(policy) => policy,
            Err(e) => return (base, Some(e)),
        };
        match prepare(proof_icc, setup) {
            Ok((proof, setup)) => {
                let refusal = (!setup.preserve_numbers)
                    .then(|| {
                        CmykToCmyk::new(
                            &policy.profile,
                            &proof,
                            ConversionOptions {
                                intent: setup.intent,
                                black_point_compensation: setup.black_point_compensation,
                            },
                        )
                        .err()
                    })
                    .flatten();
                (base + proof.bytes(), refusal)
            }
            Err(e) => (base, Some(e)),
        }
    }

    /// `dr_precision_doc_cmyk_proof_plan`'s answer:
    /// `{"working_memory_plan_bytes","refusal"}`.
    pub fn cmyk_proof_plan(&self, proof_icc: &[u8], setup: &str) -> Result<Value, String> {
        let (bytes, refusal) = self.cmyk_proof_scratch(proof_icc, setup);
        Ok(json!({"working_memory_plan_bytes":bytes,"refusal":refusal}))
    }

    /// The proof (module doc): RGBA8 in the built-in Display P3 and its info
    /// `{"space":"display-p3","out_of_gamut":<outside pixels>}`.
    pub fn cmyk_proof(
        &self,
        proof_icc: &[u8],
        setup: &str,
        limit: u64,
        other: u64,
        control: Option<&DrJobControl>,
    ) -> Result<(RgbaImage, Value), String> {
        let _run = control.map(DrJobControl::begin).transpose()?;
        let job = control.map_or(JobContext::NONE, JobContext::new);
        let (scratch, refusal) = self.cmyk_proof_scratch(proof_icc, setup);
        if let Some(reason) = refusal {
            return Err(reason);
        }
        admit(self.retained_bytes(), scratch, other, limit)?;
        job.reserve(scratch)?;
        let policy = self.require_cmyk_policy("A CMYK proof")?;
        let (proof, setup) = prepare(proof_icc, setup)?;
        let leg = if setup.preserve_numbers {
            None
        } else {
            Some(CmykToCmyk::new(
                &policy.profile,
                &proof,
                ConversionOptions {
                    intent: setup.intent,
                    black_point_compensation: setup.black_point_compensation,
                },
            )?)
        };
        let tail = ProofTail::new(&proof, IccProfile::display_p3(), &setup)?;
        let plate = setup.plate.map(usize::from);
        let (w, h) = self.dimensions();
        let mut stack = self.cmyk_stack(None, job)?;
        let mut acc = vec![[0f32; 5]; w as usize];
        let mut memo: HashMap<u64, ([u8; 3], bool)> = HashMap::new();
        let mut bytes = reserved_vec(pixel_count(w, h)? * 4)?;
        let mut out_of_gamut = 0u64;
        for y in 0..h {
            job.check()?;
            stack.row(i64::from(y), 0, &mut acc);
            for pixel in &acc {
                // A plate reads the composite on paper (module doc).
                let (ink, alpha) = match plate {
                    Some(_) => {
                        let paper = on_paper(*pixel);
                        ([paper[0], paper[1], paper[2], paper[3]], 255u8)
                    }
                    None => (
                        [pixel[0], pixel[1], pixel[2], pixel[3]],
                        (pixel[4].clamp(0.0, 1.0) * 255.0).round() as u8,
                    ),
                };
                let codes = ink.map(code);
                let key = ink_key(codes);
                let (rgb, outside) = match memo.get(&key) {
                    Some(hit) => *hit,
                    None => {
                        let ink = codes.map(|v| f64::from(unit(v)));
                        let device = match &leg {
                            Some(leg) => leg.evaluate(ink),
                            None => ink,
                        };
                        let out = match plate {
                            Some(p) => [plate_grey(device[p]); 3],
                            None => tail.display(&proof, &device),
                        };
                        let encoded: [u8; 3] =
                            out.map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8);
                        // The gamut verdict (module doc): only a converting
                        // proof can lose a colour.
                        let outside = leg.is_some()
                            && policy
                                .profile
                                .to_xyz(&ink, RenderingIntent::RelativeColorimetric)
                                .is_some_and(|xyz| gamut_delta(&proof, xyz) > GAMUT_DELTA_E);
                        if memo.len() < MEMO_CAPACITY {
                            memo.insert(key, (encoded, outside));
                        }
                        (encoded, outside)
                    }
                };
                let rgb = if outside {
                    out_of_gamut += 1;
                    if setup.gamut_warning {
                        setup.gamut_color
                    } else {
                        rgb
                    }
                } else {
                    rgb
                };
                bytes.extend_from_slice(&[rgb[0], rgb[1], rgb[2], alpha]);
            }
            job.report(f64::from(y + 1) / f64::from(h), 5)?;
        }
        let image =
            RgbaImage::from_raw(w, h, bytes).ok_or("the proof produced invalid dimensions")?;
        Ok((
            image,
            json!({"space":ProofSpace::DisplayP3.name(),"out_of_gamut":out_of_gamut}),
        ))
    }
}

/// One RGB colour's separation (module doc): `{"ink":[c,m,y,k] 0…1,
/// "out_of_gamut","delta_e","in_gamut":[r,g,b] 0…1,"profile"}`.
pub(crate) fn separate(
    rgb: &IccProfile,
    cmyk: &ProofProfile,
    values: [f64; 3],
    options: ConversionOptions,
) -> Result<Value, String> {
    if !rgb.can_decode() {
        return Err(format!(
            "the document profile \"{}\" carries no model this build can read",
            rgb.name()
        ));
    }
    let back = CmykToRgb::new(cmyk, rgb, options)?;
    require_encodable(cmyk)?;
    let xyz = document_xyz(rgb, values.map(|v| v.clamp(0.0, 1.0)));
    let leg = ToDevice::new(rgb, cmyk, options.intent, options.black_point_compensation);
    let ink: [f64; 4] = match leg.device(cmyk, xyz) {
        Some((device, _)) => std::array::from_fn(|i| device[i].clamp(0.0, 1.0)),
        None => [0.0; 4],
    };
    let delta = gamut_delta(cmyk, xyz);
    let in_gamut = back.evaluate(ink).map(|v| f64::from(v.clamp(0.0, 1.0)));
    Ok(
        json!({"ink":ink,"out_of_gamut":delta > GAMUT_DELTA_E,"delta_e":delta,
        "in_gamut":in_gamut,"profile":cmyk.name()}),
    )
}
