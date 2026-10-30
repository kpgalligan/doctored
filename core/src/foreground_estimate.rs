//! Foreground-color estimation with a supplied, unchanged matte: the byte
//! adapter over the `foreground_solve` kernel (Germer et al.'s multi-level
//! estimate), which `precision_foreground` shares at 16 bits. Select and
//! Mask's Decontaminate Colors is its caller. Inputs must be opaque
//! captured RGB composites.
//!
//! Explicit encoded/profile-linear domains share the ICC decoder/encoder; the solver's multilevel
//! field sampling shares `sampling.rs`. Source alpha and matte0/255 RGB remain exact; byte no-ops
//! return NULL without an error. After those no-ops, a profile-linear estimate in a document whose own
//! matrix/TRC profile fails the precision check refuses with
//! `icc_encode_refusal::own_profile_sentence`; the encoded domain never refuses.

use crate::{
    doc::DrDocument,
    foreground_solve::solve,
    icc_encode_refusal::{fails_precision, own_profile_sentence},
    icc_transform::ToneEncoder,
    job_control::JobContext,
    selection_tools::{number, object},
    DrImage,
};

impl DrImage {
    /// Estimates foreground RGB without applying or modifying the supplied
    /// matte. Source alpha and RGB at matte0/255 are retained exactly. Returns
    /// None for strength0, a binary matte or byte-identical output. Refuses
    /// nonopaque inputs, a canvas mismatch or unsupported profile, malformed parameters,
    /// spatially constant fractional alpha (unidentifiable color separation) and,
    /// after those no-ops, a profile-linear estimate whose encode back into the
    /// document's own profile fails the precision check (`own_profile_sentence`).
    /// Scratch includes the returned image; borrowed inputs are excluded. A bare
    /// image has no profile: the caller must supply pixels in document space.
    pub(crate) fn estimate_foreground(
        &self,
        document: &DrDocument,
        alpha: &[u8],
        json: &str,
        job: JobContext<'_>,
    ) -> Result<Option<Self>, String> {
        let parameters = object(json, &["color_space", "strength"])?;
        let linear = match parameters.get("color_space").and_then(|v| v.as_str()) {
            Some("encoded") => false,
            Some("linear") => true,
            _ => return Err("foreground estimation requires color_space encoded or linear".into()),
        };
        let strength = number(&parameters, "strength", 1.0, 0.0, 1.0)?;
        if self.pixels.dimensions() != (document.width, document.height) {
            return Err("foreground source dimensions do not match the document".into());
        }
        let model = document.profile.model().ok_or(
            "foreground estimation requires a matrix/TRC RGB profile; convert the document profile first",
        )?;
        let n = self.pixels.width() as usize * self.pixels.height() as usize;
        if alpha.len() != n {
            return Err("foreground alpha dimensions do not match the source".into());
        }
        let mut fractional = false;
        let (mut minimum, mut maximum) = (255, 0);
        for (i, (&a, p)) in alpha.iter().zip(self.pixels.pixels()).enumerate() {
            job.every(i)?;
            if p[3] != 255 {
                return Err("foreground estimation requires an opaque source composite".into());
            }
            fractional |= a > 0 && a < 255;
            minimum = minimum.min(a);
            maximum = maximum.max(a);
        }
        if strength == 0.0 || !fractional {
            return Ok(None);
        }
        if minimum == maximum {
            return Err(
                "constant fractional alpha cannot identify foreground and background colors".into(),
            );
        }
        if linear && fails_precision(&document.profile) {
            return Err(own_profile_sentence(document.profile.name()));
        }
        // Input pyramid <=32N bytes even for 1xN; live old+new F/B and
        // separable resize intermediate <=60N; output4N. 128N also covers
        // rounding of odd levels, weight vectors and allocator-sized staging.
        job.reserve(128 * n as u64 + 1024 * 1024)?;
        let encoder = ToneEncoder::new(model);
        let mut input = Vec::with_capacity(n);
        for (i, (p, &a)) in self.pixels.pixels().zip(alpha).enumerate() {
            job.every(i)?;
            let encoded = [p[0], p[1], p[2]].map(|v| f32::from(v) / 255.0);
            let rgb = if linear {
                model.decode_rgb(encoded).map(|v| v as f32)
            } else {
                encoded
            };
            input.push([rgb[0], rgb[1], rgb[2], f32::from(a) / 255.0]);
        }
        let estimate = solve(input, self.pixels.dimensions(), job)?;
        let mut pixels = self.pixels.clone();
        let mut changed = false;
        for (i, ((p, &a), colors)) in pixels.pixels_mut().zip(alpha).zip(&estimate).enumerate() {
            job.every(i)?;
            if a == 0 || a == 255 {
                continue;
            }
            let original = [p[0], p[1], p[2]].map(|v| f32::from(v) / 255.0);
            let original = if linear {
                model.decode_rgb(original).map(|v| v as f32)
            } else {
                original
            };
            for c in 0..3 {
                let value = original[c] + strength * (colors[c] - original[c]);
                let byte = if linear {
                    encoder.channel(c, value)
                } else {
                    (value.clamp(0.0, 1.0) * 255.0).round() as u8
                };
                changed |= byte != p[c];
                p[c] = byte;
            }
        }
        job.report(1.0, 5)?;
        Ok(changed.then_some(Self { pixels }))
    }
}
