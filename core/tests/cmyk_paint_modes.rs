//! Paint, Fill and Gradient blend modes on a CMYK document (#547), through
//! `dr_precision_doc_edit`, against the f64 W3C step on `L = 1 − ink`
//! (`cmyk_structure_fixture::over`, written from the spec): Multiply and
//! Color on a layer of every alpha class, the same under Preserve
//! Transparency (alpha frozen: a transparent pixel untouched, a partial one
//! the step against its ink seen at alpha 1), the Paint Bucket's fill and a
//! gradient's ink; Pass Through refuses by name.

use doctored_core::doc::BlendMode;
use serde_json::json;

use crate::cmyk_structure_fixture::{cmyk_layers, free, over, try_edit, varied, Ink, RgbLayer};

const W: u32 = 16;
const H: u32 = 12;

fn document() -> *mut doctored_core::precision_document::PrecisionDocument {
    cmyk_layers(
        W,
        H,
        vec![RgbLayer {
            name: "Layer",
            w: W,
            h: H,
            offset: (0, 0),
            pixels: varied(W, H, 4),
        }],
    )
}

/// The frozen-alpha step (`blend::composite_source_frozen`'s rule on ink).
fn frozen(dst: &mut Ink, src: [f64; 4], sa: f64, mode: BlendMode) {
    let ab = dst[4];
    if ab <= 0.0 || sa <= 0.0 {
        return;
    }
    if ab >= 1.0 {
        over(dst, src, sa, mode);
        return;
    }
    let mut seen = [dst[0], dst[1], dst[2], dst[3], 1.0];
    over(&mut seen, src, sa, mode);
    dst[..4].copy_from_slice(&seen[..4]);
}

fn assert_close(got: [u16; 5], want: Ink, what: &str) {
    for c in 0..5 {
        let w = (want[c].clamp(0.0, 1.0) * 65535.0).round();
        assert!(
            (f64::from(got[c]) - w).abs() <= 1.0,
            "{what} plate {c}: {got:?} vs {want:?}"
        );
    }
}

fn samples(doc: *const doctored_core::precision_document::PrecisionDocument) -> Vec<[u16; 5]> {
    unsafe { &*doc }.cmyk_layer_samples(0).unwrap()
}

#[test]
fn paint_blend_composites_through_the_mode_on_ink() {
    let doc = document();
    let before = samples(doc);
    let n = (W * H) as usize;
    let coverage: Vec<u16> = (0..n).map(|i| (i as u32 * 997 % 65536) as u16).collect();
    let ink = [0.2f64, 0.7, 0.1, 0.3];
    for (mode, raw) in [
        (BlendMode::Multiply, 1),
        (BlendMode::Color, 25),
        (BlendMode::Darken, 6),
    ] {
        for preserve in [false, true] {
            let request = json!({"op":"paint_blend","layer":0,"ink":[ink[0],ink[1],ink[2],ink[3],0.9],
                "opacity":0.8,"blend_mode":raw,"preserve_alpha":preserve});
            let out = try_edit(doc, &request.to_string(), &coverage).unwrap();
            assert!(!out.is_null(), "{request}");
            let after = unsafe { &*out }.cmyk_layer_samples(0).unwrap();
            for i in 0..n {
                let sa = 0.9 * 0.8 * f64::from(coverage[i]) / 65535.0;
                let mut want = before[i].map(|v| f64::from(v) / 65535.0);
                if preserve {
                    frozen(&mut want, ink, sa, mode);
                } else {
                    over(&mut want, ink, sa, mode);
                }
                if before[i] == [0; 5] && (want[4] * 65535.0).round() == 0.0 {
                    want = [0.0; 5];
                }
                assert_close(
                    after[i],
                    want,
                    &format!("{mode:?} preserve {preserve} pixel {i}"),
                );
            }
            free(out);
        }
    }
    free(doc);
}

#[test]
fn the_bucket_and_the_gradient_take_the_mode() {
    let doc = document();
    let before = samples(doc);
    let n = (W * H) as usize;
    let full = vec![65535u16; n];
    let ink = [0.5f64, 0.1, 0.6, 0.2];
    // A global (non-contiguous) bucket at tolerance 65535 reaches every pixel.
    let bucket = json!({"op":"bucket_fill","layer":0,"ink":[ink[0],ink[1],ink[2],ink[3],1.0],
        "opacity":0.5,"blend_mode":1,"x":0,"y":0,"tolerance":65535,"contiguous":false});
    let out = try_edit(doc, &bucket.to_string(), &full).unwrap();
    let after = unsafe { &*out }.cmyk_layer_samples(0).unwrap();
    for i in 0..n {
        let mut want = before[i].map(|v| f64::from(v) / 65535.0);
        over(&mut want, ink, 0.5, BlendMode::Multiply);
        if before[i] == [0; 5] && (want[4] * 65535.0).round() == 0.0 {
            want = [0.0; 5];
        }
        assert_close(after[i], want, &format!("bucket pixel {i}"));
    }
    free(out);
    // A gradient of one ink end to end (both stops equal) in Color.
    let stop = [ink[0], ink[1], ink[2], ink[3], 1.0];
    let gradient = json!({"op":"gradient","layer":0,"opacity":0.7,"blend_mode":25,
        "from":[0.0,0.0],"to":[16.0,0.0],
        "stops":[{"position":0.0,"ink":stop},{"position":1.0,"ink":stop}]});
    let out = try_edit(doc, &gradient.to_string(), &full).unwrap();
    let after = unsafe { &*out }.cmyk_layer_samples(0).unwrap();
    for i in 0..n {
        let mut want = before[i].map(|v| f64::from(v) / 65535.0);
        over(&mut want, ink, 0.7, BlendMode::Color);
        if before[i] == [0; 5] && (want[4] * 65535.0).round() == 0.0 {
            want = [0.0; 5];
        }
        assert_close(after[i], want, &format!("gradient pixel {i}"));
    }
    free(out);
    assert_eq!(
        try_edit(
            doc,
            r#"{"op":"paint_blend","layer":0,"ink":[0,0,0,1,1],"opacity":1,"blend_mode":27}"#,
            &full
        )
        .unwrap_err(),
        "The Pass Through blend mode is unavailable in a CMYK document"
    );
    free(doc);
}
