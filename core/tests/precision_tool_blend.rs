//! Tool blend modes at 16 bits: the Clone Stamp's `retouch_clone`
//! `blend_mode` and the Gradient tool's `gradient` `blend_mode`, each through
//! `dr_precision_doc_edit`.
//!
//! The oracle is the W3C compositing-1 step written out here in f64, with the
//! separable blend functions of `tests/common/native_blend_oracle.rs` (which
//! reads nothing out of `blend.rs`) and that file's integer Dissolve
//! threshold. Refusals are checked by their sentence.

use std::ffi::{CStr, CString};
use std::ptr;

use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_precision_edit::dr_precision_doc_edit;
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer as Layer};
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use serde_json::{json, Value};

use crate::native_blend_oracle as blend_oracle;

const LIMIT: u64 = REFERENCE_WORKING_BYTES;
/// Codes of 65535. The core composites in f32 and rounds once to u16; the
/// oracle is f64. The f32 error of the step is under 0.1 code, so a value near
/// a rounding boundary lands at most one code away; 2 leaves that room.
const TOLERANCE: f64 = 2.0;
/// `DrBlendMode` raw values, restated rather than read out of the core.
const NORMAL: u32 = 0;
const MULTIPLY: u32 = 1;
const DISSOLVE: u32 = 14;
/// The separable modes the clone cases walk: Multiply, Screen, Overlay,
/// Difference, Linear Dodge (Add), Linear Burn.
const SEPARABLE: [u32; 6] = [1, 2, 3, 8, 12, 15];
const LOCK_TRANSPARENCY: u32 = 1;

const W: u32 = 7;
const H: u32 = 5;

fn document(canvas: (u32, u32), layers: Vec<Layer>) -> PrecisionDocument {
    PrecisionDocument::from_parts(
        canvas.0,
        canvas.1,
        IccProfile::srgb(),
        Metadata::default(),
        Resolution::default(),
        layers,
    )
    .unwrap()
}

fn raster(name: &str, size: (u32, u32), offset: (i32, i32), pixels: Vec<[u16; 4]>) -> Layer {
    let image =
        PrecisionImage::from_rgba16(size.0, size.1, pixels, IccProfile::srgb(), None).unwrap();
    Layer::raster(name, image, offset, 1.0, true).unwrap()
}

fn samples(document: &PrecisionDocument, index: usize) -> Vec<[u16; 4]> {
    document.layer_source(index).unwrap().to_rgba16().unwrap()
}

fn execute(
    document: &PrecisionDocument,
    request: &Value,
    input: &[u16],
) -> Result<Option<Box<PrecisionDocument>>, String> {
    let text = CString::new(request.to_string()).unwrap();
    unsafe {
        let mut error = ptr::null_mut();
        let out = dr_precision_doc_edit(
            document,
            text.as_ptr(),
            input.as_ptr(),
            input.len(),
            LIMIT,
            0,
            ptr::null(),
            &mut error,
        );
        if !error.is_null() {
            let message = CStr::from_ptr(error).to_string_lossy().into_owned();
            dr_string_free(error);
            assert!(out.is_null(), "a refusal leaves no document");
            return Err(message);
        }
        Ok((!out.is_null()).then(|| Box::from_raw(out)))
    }
}

/// The destination: every alpha class (0, a fraction, opaque) over varied RGB.
fn destination(i: u32) -> [u16; 4] {
    let alpha = [65535u16, 0, 30000, 65535, 52000][(i % 5) as usize];
    [
        (9000 + i * 1500) as u16,
        (60000 - i * 1300) as u16,
        (20000 + i * 700) as u16,
        alpha,
    ]
}

/// The clone source: opaque and translucent samples, and one transparent.
fn source(i: u32) -> [u16; 4] {
    let alpha = [65535u16, 41000, 65535, 0][(i % 4) as usize];
    [
        (50000 - i * 1100) as u16,
        (12000 + i * 1400) as u16,
        (33000 + i * 600) as u16,
        alpha,
    ]
}

/// `[Target, Source]`, both canvas-sized.
fn fixture() -> PrecisionDocument {
    let n = W * H;
    document(
        (W, H),
        vec![
            raster("Target", (W, H), (0, 0), (0..n).map(destination).collect()),
            raster("Source", (W, H), (0, 0), (0..n).map(source).collect()),
        ],
    )
}

/// Stroke coverage: zeros, full codes and fractions.
fn coverage() -> Vec<u16> {
    (0..W * H)
        .map(|i| [65535u16, 20000, 0, 48000, 65535, 9000][(i % 6) as usize])
        .collect()
}

fn clone_request(mode: Option<u32>, opacity: f64) -> Value {
    let mut request = json!({"op": "retouch_clone", "layer": 0, "source_layer": 1,
                             "sample": "current", "opacity": opacity});
    if let Some(mode) = mode {
        request["blend_mode"] = json!(mode);
    }
    request
}

/// The W3C step in f64, codes in and continuous codes out: destination `d`,
/// source `s`, effective source alpha `sa`, separable mode `mode`. With
/// `frozen` the destination's alpha is kept and its colour is the step run
/// against the same colour at alpha 1 (Lock Transparency).
fn composite(d: [u16; 4], s: [u16; 4], sa: f64, mode: u32, frozen: bool) -> [f64; 4] {
    let unit = |p: [u16; 4]| p.map(|v| f64::from(v) / 65535.0);
    let (d, s) = (unit(d), unit(s));
    if frozen && d[3] == 0.0 {
        return d.map(|v| v * 65535.0);
    }
    let ab = if frozen { 1.0 } else { d[3] };
    let ao = sa + ab * (1.0 - sa);
    let mut out = [0.0; 4];
    for c in 0..3 {
        let blended = blend_oracle::separable(mode as i32, d[c], s[c]);
        out[c] = (sa * (1.0 - ab) * s[c] + sa * ab * blended + (1.0 - sa) * ab * d[c]) / ao;
    }
    out[3] = if frozen { d[3] } else { ao };
    out.map(|v| v * 65535.0)
}

fn close(got: [u16; 4], want: [f64; 4], what: &str) {
    for c in 0..4 {
        assert!(
            (f64::from(got[c]) - want[c]).abs() <= TOLERANCE,
            "{what} channel {c}: {} against {} (tolerance {TOLERANCE})",
            got[c],
            want[c]
        );
    }
}

#[test]
fn clone_composites_the_source_through_each_separable_mode() {
    let doc = fixture();
    let plane = coverage();
    let (before, from) = (samples(&doc, 0), samples(&doc, 1));
    for mode in SEPARABLE {
        let out = execute(&doc, &clone_request(Some(mode), 0.8), &plane)
            .unwrap()
            .expect("a blended clone changes the layer");
        let after = samples(&out, 0);
        assert_eq!(samples(&out, 1), from, "the source layer is untouched");
        for i in 0..before.len() {
            if plane[i] == 0 || from[i][3] == 0 {
                assert_eq!(after[i], before[i], "mode {mode} pixel {i}: not covered");
                continue;
            }
            let sa = f64::from(from[i][3]) / 65535.0 * 0.8 * f64::from(plane[i]) / 65535.0;
            close(
                after[i],
                composite(before[i], from[i], sa, mode, false),
                &format!("mode {mode} pixel {i}"),
            );
        }
    }
}

#[test]
fn normal_is_the_request_with_no_blend_mode_bit_for_bit() {
    let doc = fixture();
    let plane = coverage();
    let plain = execute(&doc, &clone_request(None, 0.8), &plane)
        .unwrap()
        .unwrap();
    let normal = execute(&doc, &clone_request(Some(NORMAL), 0.8), &plane)
        .unwrap()
        .unwrap();
    assert_eq!(samples(&normal, 0), samples(&plain, 0));
    // And that output is plain source-over.
    let (before, from, after) = (samples(&doc, 0), samples(&doc, 1), samples(&plain, 0));
    for i in 0..before.len() {
        if plane[i] == 0 || from[i][3] == 0 {
            continue;
        }
        let sa = f64::from(from[i][3]) / 65535.0 * 0.8 * f64::from(plane[i]) / 65535.0;
        close(
            after[i],
            composite(before[i], from[i], sa, NORMAL, false),
            &format!("pixel {i}"),
        );
    }
}

#[test]
fn the_selection_plane_scales_a_blended_clone() {
    let doc = fixture();
    let stroke = coverage();
    let selected: Vec<u16> = (0..W * H)
        .map(|i| [65535u16, 0, 30000][(i % 3) as usize])
        .collect();
    let mut input = stroke.clone();
    input.extend_from_slice(&selected);
    let mut request = clone_request(Some(MULTIPLY), 1.0);
    request["selection"] = json!(true);
    let out = execute(&doc, &request, &input).unwrap().unwrap();
    let (before, from, after) = (samples(&doc, 0), samples(&doc, 1), samples(&out, 0));
    for i in 0..before.len() {
        if stroke[i] == 0 || selected[i] == 0 || from[i][3] == 0 {
            assert_eq!(
                after[i], before[i],
                "pixel {i}: outside stroke or selection"
            );
            continue;
        }
        let sa = f64::from(from[i][3]) / 65535.0 * f64::from(stroke[i]) / 65535.0
            * f64::from(selected[i])
            / 65535.0;
        close(
            after[i],
            composite(before[i], from[i], sa, MULTIPLY, false),
            &format!("pixel {i}"),
        );
    }
}

#[test]
fn a_transparency_lock_keeps_alpha_under_a_blended_clone() {
    let doc = fixture()
        .with_layer_locks(0, LOCK_TRANSPARENCY)
        .unwrap()
        .unwrap();
    let plane = coverage();
    let out = execute(&doc, &clone_request(Some(MULTIPLY), 0.8), &plane)
        .unwrap()
        .unwrap();
    let (before, from, after) = (samples(&doc, 0), samples(&doc, 1), samples(&out, 0));
    for i in 0..before.len() {
        assert_eq!(after[i][3], before[i][3], "pixel {i}: alpha is frozen");
        if plane[i] == 0 || from[i][3] == 0 {
            assert_eq!(after[i], before[i], "pixel {i}: not covered");
            continue;
        }
        let sa = f64::from(from[i][3]) / 65535.0 * 0.8 * f64::from(plane[i]) / 65535.0;
        close(
            after[i],
            composite(before[i], from[i], sa, MULTIPLY, true),
            &format!("pixel {i}"),
        );
    }
}

#[test]
fn dissolve_dithers_at_each_pixels_canvas_position() {
    // A 6×4 target at canvas (3, 2) on a 12×8 canvas, so a dither keyed on
    // layer coordinates instead of canvas ones would pick other pixels.
    let (cw, ch, lw, lh, ox, oy) = (12u32, 8u32, 6u32, 4u32, 3i32, 2i32);
    let target: Vec<[u16; 4]> = (0..lw * lh).map(|_| [1000, 2000, 3000, 65535]).collect();
    let from: Vec<[u16; 4]> = (0..cw * ch)
        .map(|i| [(40000 + i * 100) as u16, 50000, 60000, 65535])
        .collect();
    let doc = document(
        (cw, ch),
        vec![
            raster("Target", (lw, lh), (ox, oy), target.clone()),
            raster("Source", (cw, ch), (0, 0), from.clone()),
        ],
    );
    // Half coverage everywhere: about half the pixels take the source.
    let plane = vec![32768u16; (cw * ch) as usize];
    let out = execute(&doc, &clone_request(Some(DISSOLVE), 1.0), &plane)
        .unwrap()
        .unwrap();
    let after = samples(&out, 0);
    let sa = 32768.0 / 65535.0;
    let (mut taken, mut kept) = (0, 0);
    for ly in 0..lh {
        for lx in 0..lw {
            let (cx, cy) = (i64::from(lx) + i64::from(ox), i64::from(ly) + i64::from(oy));
            let threshold = blend_oracle::dissolve_threshold(cx, cy);
            if (threshold - sa).abs() <= blend_oracle::DISSOLVE_BAND {
                continue;
            }
            let got = after[(ly * lw + lx) as usize];
            if threshold < sa {
                let source = from[(cy as u32 * cw + cx as u32) as usize];
                assert_eq!(got, source, "({cx},{cy}) shows the source, opaque");
                taken += 1;
            } else {
                assert_eq!(got, target[(ly * lw + lx) as usize], "({cx},{cy}) is kept");
                kept += 1;
            }
        }
    }
    assert!(taken > 0 && kept > 0, "the dither both takes and keeps");
}

#[test]
fn blend_mode_refusals_name_the_rule() {
    let doc = fixture();
    let plane = coverage();
    // Only the clone has a source colour to composite.
    for op in ["retouch_heal", "spot_heal", "content_aware_fill", "red_eye"] {
        let request = json!({"op": op, "layer": 0, "blend_mode": MULTIPLY});
        assert_eq!(
            execute(&doc, &request, &plane).err().unwrap(),
            "blend_mode is not an argument of this retouch operation",
            "{op}"
        );
    }
    for mode in [27u32, 1000, 4294967295] {
        assert_eq!(
            execute(&doc, &clone_request(Some(mode), 1.0), &plane)
                .err()
                .unwrap(),
            "retouch blend mode is invalid",
            "{mode}"
        );
    }
    // A negative value is not a `u32`: the request's own parse refuses it.
    let mut negative = clone_request(None, 1.0);
    negative["blend_mode"] = json!(-1);
    let refusal = execute(&doc, &negative, &plane).err().unwrap();
    assert!(
        refusal.starts_with("invalid native retouch request"),
        "{refusal}"
    );
    let mut new_layer = clone_request(Some(MULTIPLY), 1.0);
    new_layer["output"] = json!("new_layer");
    assert_eq!(
        execute(&doc, &new_layer, &plane).err().unwrap(),
        "a clone onto a new layer has nothing to blend with; blend_mode requires output layer"
    );
    // Normal onto a new layer is still the clone it always was.
    let mut normal = clone_request(Some(NORMAL), 1.0);
    normal["output"] = json!("new_layer");
    let out = execute(&doc, &normal, &plane).unwrap().unwrap();
    assert_eq!(out.layer_count(), 3);
}

#[test]
fn gradient_multiplies_a_flat_ramp_into_a_flat_colour() {
    let n = (W * H) as usize;
    let base = [40000u16, 20000, 60000, 65535];
    let doc = document(
        (W, H),
        vec![raster("Target", (W, H), (0, 0), vec![base; n])],
    );
    let ink = [0.5f64, 0.25, 0.75, 1.0];
    let request = |mode: Option<u32>| {
        let mut request = json!({"op": "gradient", "layer": 0, "start": ink, "end": ink,
            "color_space": "document", "opacity": 1.0, "from": [0.0, 0.0],
            "to": [f64::from(W), 0.0], "radial": false});
        if let Some(mode) = mode {
            request["blend_mode"] = json!(mode);
        }
        request
    };
    let plane = vec![65535u16; n];
    let out = execute(&doc, &request(Some(MULTIPLY)), &plane)
        .unwrap()
        .expect("a multiplied gradient");
    for (i, got) in samples(&out, 0).iter().enumerate() {
        let want = [
            f64::from(base[0]) * ink[0],
            f64::from(base[1]) * ink[1],
            f64::from(base[2]) * ink[2],
            65535.0,
        ];
        close(*got, want, &format!("pixel {i}"));
    }
    // Normal through the key is the request without it.
    let plain = execute(&doc, &request(None), &plane).unwrap().unwrap();
    let normal = execute(&doc, &request(Some(NORMAL)), &plane)
        .unwrap()
        .unwrap();
    assert_eq!(samples(&normal, 0), samples(&plain, 0));
}
