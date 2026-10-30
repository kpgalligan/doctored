//! MODEL MASKS (Select ▸ Sky, Select ▸ Person Parts…, the Object
//! Selection tool's Object Finder) — a segmentation model's low-resolution
//! output plane turned into canvas-sized u16 coverage, the KERNEL under
//! `ffi_model_mask`. It names no document type: the host runs the model (Core
//! ML lives in Swift) and sends its output samples.
//!
//! **Plane.** `channels` planes of `width` x `height` f32 samples,
//! CHANNEL-MAJOR (`c·w·h + y·w + x`), as a Core ML multi-array `[1,]C,H,W`
//! reads. A request picks channels; their per-cell SUM is the value sampled
//! (a parts model's hair and clothes channels add to "hair or clothes",
//! because a softmax's class probabilities are disjoint shares of one cell).
//!
//! **Frame.** The plane covers the canvas rectangle `frame` `[x, y, w, h]` —
//! the crop the host fed the model, squashed to the model's input. Canvas
//! pixel `(x, y)` inside the frame reads the summed plane at the continuous
//! position `u = (x + ½ − fx)·pw/fw`, `v = (y + ½ − fy)·ph/fh` (centres at
//! half-integers on both sides, so the identity frame reads each cell at its
//! centre, exactly), bilinearly through `sampling_radial::sample_exact`, the
//! crate's master bilinear for continuous positions, with clamped edges.
//! Pixels outside the frame are 0.
//!
//! **Values.** `logit` is 1 where the sample is > 0, else 0 (SAM's mask
//! decoder answers logits, and SAM's own post-processing thresholds them at
//! zero; see [`Values::Logit`]); `probability` clamps it to `[0, 1]` (a
//! softmax or sigmoid output, whose channel sum can pass 1 only by rounding).
//! Interpolating BEFORE the threshold keeps a logit's zero crossing — the
//! model's own 50 % contour — where the bilinear surface crosses zero, rather
//! than stair-stepping the low-resolution cells; the host's edge stage then
//! supplies the soft edge. A value below [`COVERAGE_FLOOR`] (1/256) becomes 0.
//! The value is then
//! multiplied by the optional canvas-sized `clip` (`clip / 255`, the person
//! a part belongs to) and rounded to u16.
//!
//! **Refusals**, each a sentence: a non-finite selected sample, a sample
//! count that is not `w·h·c`, a plane over [`MAX_PLANE_SIDE`] or
//! [`MAX_CHANNELS`], a frame outside the canvas, a channel the plane lacks, a
//! malformed request.

use serde_json::Value;

use crate::job_control::JobContext;
use crate::precision_image::{pixel_count, reserved_vec};
use crate::sampling_radial::sample_exact;

/// The longest side a model output plane may have. Segmentation decoders
/// answer 256–1024 px (SAM's low-res masks are 256², MediaPipe's 256², the
/// sky U²-Net's 384²); 4096 is far above any of them and bounds the plane at
/// 64 MiB a channel.
pub(crate) const MAX_PLANE_SIDE: u32 = 4096;
/// The most channels a plane may have: MediaPipe's multiclass selfie model
/// has six classes and SAM's decoder three masks; 16 leaves room for a
/// larger label map without letting a malformed shape claim gigabytes.
pub(crate) const MAX_CHANNELS: u32 = 16;
/// Coverage below one step of the 8-bit display (1/256) is 0. A probability
/// never reaches exactly 0 on its own — a softmax share is positive in every
/// cell — so without a floor a pixel far
/// from the object keeps a few u16 codes of coverage, and the selection's
/// bounds become the whole frame: Crop to Selection, Copy and reported bounds
/// then act on the whole canvas, and 16/32-bit edits leak faintly outside
/// the object. A value under one display step is invisible in the marquee
/// and in any 8-bit result, so dropping it changes nothing a user sees.
pub(crate) const COVERAGE_FLOOR: f64 = 1.0 / 256.0;

/// How the sampled value becomes coverage.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Values {
    /// 1 where the sample is > 0, else 0: SAM's own post-processing
    /// (`SAM2ImagePredictor` upsamples `low_res_masks` bilinearly to the image
    /// and thresholds at `mask_threshold = 0.0`). The logistic of the logit is
    /// not coverage: SAM's low-confidence masks carry a background haze of
    /// negative logits whose logistic was measured at up to 35/255 more than
    /// 160 px from the object and 47/255 within 16 px, above any display
    /// floor, so a click selection's bounds became the whole canvas and an
    /// edit through it leaked into the background. The hard mask's edge is
    /// made soft by the host's edge stage (`SelectionCapture.refinedEdge`).
    Logit,
    /// The sample clamped to `[0, 1]`.
    Probability,
}

/// One parsed request: where the plane lies on the canvas, how its values
/// read, and which channels are summed.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ModelMaskRequest {
    pub frame: [u32; 4],
    pub values: Values,
    pub channels: Vec<usize>,
}

/// The request JSON `{"frame":[x,y,w,h],"values":"logit"|"probability",
/// "channels":[c…]}` checked against a plane of `plane_channels` channels and
/// the `canvas` size. Keys are strict (an unknown key is refused); `frame`
/// is integers with `w, h ≥ 1` inside the canvas; `channels` is unique, each
/// below `plane_channels`, `[0]` when omitted, and exactly one for `logit`
/// (a sum of logits is no logit of anything).
pub(crate) fn request_from_json(
    text: &str,
    plane_channels: u32,
    canvas: (u32, u32),
) -> Result<ModelMaskRequest, String> {
    let value: Value = serde_json::from_str(text)
        .map_err(|e| format!("model mask request json is malformed: {e}"))?;
    let object = value
        .as_object()
        .ok_or("model mask request json must be an object")?;
    if let Some(key) = object
        .keys()
        .find(|k| !matches!(k.as_str(), "frame" | "values" | "channels"))
    {
        return Err(format!("model mask request has an unknown key \"{key}\""));
    }
    let frame = object
        .get("frame")
        .and_then(Value::as_array)
        .filter(|list| list.len() == 4)
        .ok_or("a model mask request needs a frame [x, y, width, height]")?;
    let mut parsed = [0u32; 4];
    for (slot, item) in parsed.iter_mut().zip(frame) {
        *slot = item
            .as_u64()
            .and_then(|v| u32::try_from(v).ok())
            .ok_or("a model mask frame must be four non-negative integers")?;
    }
    let [x, y, w, h] = parsed;
    if w == 0 || h == 0 {
        return Err("a model mask frame must have a positive width and height".into());
    }
    if u64::from(x) + u64::from(w) > u64::from(canvas.0)
        || u64::from(y) + u64::from(h) > u64::from(canvas.1)
    {
        return Err("the model mask frame lies outside the canvas".into());
    }
    let values = match object.get("values").and_then(Value::as_str) {
        Some("logit") => Values::Logit,
        Some("probability") => Values::Probability,
        _ => {
            return Err("a model mask request's values must be \"logit\" or \"probability\"".into())
        }
    };
    let channels = match object.get("channels") {
        None => vec![0],
        Some(list) => {
            let list = list
                .as_array()
                .ok_or("a model mask request's channels must be an array")?;
            let mut channels = Vec::with_capacity(list.len());
            for item in list {
                let c = item
                    .as_u64()
                    .ok_or("a model mask channel must be a non-negative integer")?;
                if c >= u64::from(plane_channels) {
                    return Err(format!(
                        "model mask channel {c} is not in the model output ({plane_channels} channels)"
                    ));
                }
                let c = c as usize;
                if channels.contains(&c) {
                    return Err(format!("model mask channel {c} is listed twice"));
                }
                channels.push(c);
            }
            channels
        }
    };
    if channels.is_empty() {
        return Err("a model mask request needs at least one channel".into());
    }
    if values == Values::Logit && channels.len() != 1 {
        return Err("a logit model mask reads exactly one channel".into());
    }
    Ok(ModelMaskRequest {
        frame: parsed,
        values,
        channels,
    })
}

/// The working memory [`model_mask_coverage`] may hold over a `width` x
/// `height` canvas from a plane of `plane_len` samples beyond its inputs:
/// the summed plane (at most the whole plane, 4 bytes a sample) and the u16
/// canvas output.
pub(crate) fn coverage_scratch(width: u32, height: u32, plane_len: usize) -> u64 {
    plane_len as u64 * 4 + u64::from(width) * u64::from(height) * 2
}

/// The plane's shape, checked: `(width, height, channels)` within the caps
/// and `w·h·c == len`. Answers the cell count `w·h`.
pub(crate) fn check_plane(len: usize, (pw, ph, pc): (u32, u32, u32)) -> Result<usize, String> {
    if pw == 0 || ph == 0 || pc == 0 {
        return Err("model output plane must have a positive size and channel count".into());
    }
    if pw > MAX_PLANE_SIDE || ph > MAX_PLANE_SIDE || pc > MAX_CHANNELS {
        return Err(format!(
            "model output plane is larger than {MAX_PLANE_SIDE} px a side or {MAX_CHANNELS} channels"
        ));
    }
    let cell = pw as usize * ph as usize;
    let expected = cell * pc as usize;
    if len != expected {
        return Err(format!(
            "model output has {len} samples; {pw} x {ph} x {pc} needs {expected}"
        ));
    }
    Ok(cell)
}

/// Canvas-sized u16 coverage of `plane` (`size` = `(width, height,
/// channels)`, channel-major) under `request`, optionally multiplied by the
/// canvas-sized `clip`, and the count of samples at or above half coverage
/// (32768). Refuses, by sentence, a non-finite selected sample, a plane
/// whose length or size breaks the caps, a request channel outside the
/// plane, a frame outside the canvas, a clip of the wrong length, or a
/// cancel.
pub(crate) fn model_mask_coverage(
    plane: &[f32],
    size: (u32, u32, u32),
    request: &ModelMaskRequest,
    clip: Option<&[u8]>,
    canvas: (u32, u32),
    job: JobContext<'_>,
) -> Result<(Vec<u16>, u64), String> {
    let cell = check_plane(plane.len(), size)?;
    let (pw, ph, pc) = size;
    let n = pixel_count(canvas.0, canvas.1)?;
    if clip.is_some_and(|c| c.len() != n) {
        return Err("model mask clip must be canvas-sized".into());
    }
    if request.channels.iter().any(|&c| c >= pc as usize) {
        return Err(format!(
            "a model mask channel is not in the model output ({pc} channels)"
        ));
    }
    let [fx, fy, fw, fh] = request.frame;
    if fw == 0
        || fh == 0
        || u64::from(fx) + u64::from(fw) > u64::from(canvas.0)
        || u64::from(fy) + u64::from(fh) > u64::from(canvas.1)
    {
        return Err("the model mask frame lies outside the canvas".into());
    }
    let mut summed: Vec<f32> = reserved_vec(cell)?;
    summed.resize(cell, 0.0);
    for &c in &request.channels {
        job.check()?;
        for (sum, &s) in summed.iter_mut().zip(&plane[c * cell..(c + 1) * cell]) {
            *sum += s;
        }
    }
    if summed.iter().any(|s| !s.is_finite()) {
        return Err("model output is not finite".into());
    }
    let mut output: Vec<u16> = reserved_vec(n)?;
    output.resize(n, 0);
    // The summed value rides in the red channel of an opaque sample, so
    // `sample_exact`'s premultiplication (by alpha 1) leaves it unchanged.
    let read = |i: usize| [summed[i], 0.0, 0.0, 1.0];
    let (sx, sy) = (f64::from(pw) / f64::from(fw), f64::from(ph) / f64::from(fh));
    let width = canvas.0 as usize;
    let mut covered = 0u64;
    for y in fy..fy + fh {
        job.report(f64::from(y - fy) / f64::from(fh), 0)?;
        let v = (f64::from(y - fy) + 0.5) * sy;
        let row = y as usize * width;
        for x in fx..fx + fw {
            let u = (f64::from(x - fx) + 0.5) * sx;
            let s = sample_exact(&read, (pw as usize, ph as usize), (u, v), true)[0];
            let value = match request.values {
                Values::Logit => {
                    if s > 0.0 {
                        1.0
                    } else {
                        0.0
                    }
                }
                Values::Probability => s.clamp(0.0, 1.0),
            };
            let value = if value < COVERAGE_FLOOR { 0.0 } else { value };
            let index = row + x as usize;
            let keep = clip.map_or(1.0, |c| f64::from(c[index]) / 255.0);
            let sample = (value * keep * 65535.0).round() as u16;
            if sample >= 32768 {
                covered += 1;
            }
            output[index] = sample;
        }
    }
    job.check()?;
    Ok((output, covered))
}
