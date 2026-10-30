//! Edit ▸ Stroke… with nothing selected and Layer ▸ Layer Mask ▸ Apply on a
//! CMYK document (#547), through `dr_precision_doc_edit`: the stroke's strip
//! pixels (where the band is certainly full) against this file's own straight
//! CMYKA source-over at coverage 1 and opacity 0.5, inside, centre and
//! outside; Preserve Transparency leaving alpha exactly as it was; and Apply
//! Mask as `round(alpha · m / 65535)` with every ink bit kept, a feathered
//! mask through `read_plane`'s `effective_mask`.

use std::ffi::CString;
use std::ptr;

use doctored_core::ffi_precision::{dr_precision_doc_free, dr_precision_doc_open};
use doctored_core::ffi_precision_edit::{dr_precision_doc_edit, dr_precision_doc_read_plane};
use doctored_core::precision_document::PrecisionDocument;
use tempfile::TempDir;

use crate::cmyk_fixture::press_profile;
use crate::cmyk_tiff_builder::cmyk_tiff;
use crate::common::{cpath, take_err_string};

const LIMIT: u64 = 2 * 1024 * 1024 * 1024;
const W: u32 = 24;
const H: u32 = 20;
/// The opaque rectangle `[x0, x1) × [y0, y1)` the stroke outlines.
const RECT: (u32, u32, u32, u32) = (8, 6, 16, 14);

type Ink = [f64; 5];

fn free(document: *mut PrecisionDocument) {
    unsafe { dr_precision_doc_free(document) };
}

fn edit(
    doc: *const PrecisionDocument,
    request: &str,
    samples: &[u16],
) -> Result<*mut PrecisionDocument, String> {
    let text = CString::new(request).unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_edit(
            doc,
            text.as_ptr(),
            if samples.is_empty() {
                ptr::null()
            } else {
                samples.as_ptr()
            },
            samples.len(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    if out.is_null() && !err.is_null() {
        return Err(take_err_string(err));
    }
    Ok(out)
}

/// An opaque CMYK base (a gentle ink ramp) with a bare New Layer above it.
fn base(dir: &TempDir) -> (*mut PrecisionDocument, *mut PrecisionDocument) {
    let data: Vec<u8> = (0..W * H)
        .flat_map(|i| {
            [
                12000 + (i % W) as u16 * 300,
                9000,
                7000 + (i / W) as u16 * 200,
                4000,
            ]
        })
        .flat_map(|v| v.to_le_bytes())
        .collect();
    let path = dir.path().join("base.tif");
    let icc = press_profile("Press Stroke");
    std::fs::write(&path, cmyk_tiff(W, H, 16, 4, 0, &data, Some(&icc))).unwrap();
    let c = cpath(&path);
    let mut err = ptr::null_mut();
    let doc = unsafe { dr_precision_doc_open(c.as_ptr(), LIMIT, 0, ptr::null(), &mut err) };
    assert!(!doc.is_null(), "open: {}", take_err_string(err));
    let added = edit(
        doc,
        r#"{"op":"add_layer","name":"Top","anchor":0,"position":"above"}"#,
        &[],
    )
    .unwrap();
    (doc, added)
}

/// Layer 1 holding an opaque rectangle of `ink` on transparency.
fn rectangle(dir: &TempDir, ink: [f64; 5]) -> Vec<*mut PrecisionDocument> {
    let (doc, added) = base(dir);
    let (x0, y0, x1, y1) = RECT;
    let stroke: Vec<u16> = (0..W * H)
        .map(|i| {
            let (x, y) = (i % W, i / W);
            if (x0..x1).contains(&x) && (y0..y1).contains(&y) {
                65535
            } else {
                0
            }
        })
        .collect();
    let painted = edit(
        added,
        &format!(r#"{{"op":"paint_blend","layer":1,"ink":{ink:?},"opacity":1}}"#),
        &stroke,
    )
    .unwrap();
    vec![painted, added, doc]
}

/// Record `index`'s ink on the canvas at its offset; bare paper elsewhere.
fn placed(doc: *const PrecisionDocument, index: usize) -> Vec<[u16; 5]> {
    let layer = unsafe { &*doc }.layer(index).unwrap();
    let (ox, oy) = layer.offset();
    let (lw, lh) = layer.cmyk_source().unwrap().dimensions();
    let samples = unsafe { &*doc }.cmyk_layer_samples(index).unwrap();
    (0..W * H)
        .map(|i| {
            let (x, y) = (
                i64::from(i % W) - i64::from(ox),
                i64::from(i / W) - i64::from(oy),
            );
            if x >= 0 && y >= 0 && x < i64::from(lw) && y < i64::from(lh) {
                samples[(y * i64::from(lw) + x) as usize]
            } else {
                [0; 5]
            }
        })
        .collect()
}

fn unit(s: [u16; 5]) -> Ink {
    s.map(|v| f64::from(v) / 65535.0)
}

/// Straight CMYKA Normal over at effective alpha `sa` (the W3C source-over on
/// the four inks).
fn over(dst: Ink, src: [f64; 4], sa: f64) -> [u16; 5] {
    let ab = dst[4];
    let ao = sa + ab * (1.0 - sa);
    let mut out = [0.0; 5];
    for c in 0..4 {
        out[c] = (sa * src[c] + (1.0 - sa) * ab * dst[c]) / ao;
    }
    out[4] = ao;
    out.map(|v| (v.clamp(0.0, 1.0) * 65535.0).round() as u16)
}

fn close(got: [u16; 5], want: [u16; 5], what: &str) {
    for c in 0..5 {
        assert!(
            got[c].abs_diff(want[c]) <= 1,
            "{what}: {got:?} vs oracle {want:?}"
        );
    }
}

const STROKE_INK: [f64; 5] = [0.1, 0.8, 0.3, 0.05, 1.0];

fn stroke(
    doc: *const PrecisionDocument,
    location: &str,
    preserve: bool,
) -> Result<*mut PrecisionDocument, String> {
    edit(
        doc,
        &format!(
            r#"{{"op":"stroke_layer_edge","layer":1,"width":2,"location":"{location}","ink":{STROKE_INK:?},"opacity":0.5,"preserve_alpha":{preserve}}}"#
        ),
        &[],
    )
}

#[test]
fn the_stroke_band_is_ink_over_at_full_coverage_inside_centre_and_outside() {
    let dir = TempDir::new().unwrap();
    let fill = [0.7, 0.2, 0.1, 0.4, 1.0];
    let docs = rectangle(&dir, fill);
    let doc = docs[0];
    let before = placed(doc, 1);
    let (x0, y0, x1, y1) = RECT;
    let source = [STROKE_INK[0], STROKE_INK[1], STROKE_INK[2], STROKE_INK[3]];
    // The strip pixels on each side's middle rows/columns, 0.5 px from the edge.
    let mid = |i: u32, j: u32| (i + j) / 2;
    let inside = [
        (x0, mid(y0, y1)),
        (x1 - 1, mid(y0, y1)),
        (mid(x0, x1), y0),
        (mid(x0, x1), y1 - 1),
    ];
    let outside = [
        (x0 - 1, mid(y0, y1)),
        (x1, mid(y0, y1)),
        (mid(x0, x1), y0 - 1),
        (mid(x0, x1), y1),
    ];
    for location in ["inside", "center", "outside"] {
        let out = stroke(doc, location, false).unwrap();
        assert!(!out.is_null(), "{location} changed the layer");
        let after = placed(out, 1);
        let strips: Vec<(u32, u32)> = match location {
            "inside" => inside.to_vec(),
            "outside" => outside.to_vec(),
            _ => inside.iter().chain(outside.iter()).copied().collect(),
        };
        for (x, y) in strips {
            let i = (y * W + x) as usize;
            close(
                after[i],
                over(unit(before[i]), source, 0.5),
                &format!("{location} ({x},{y})"),
            );
        }
        // Far from the edge nothing changes: the rectangle's centre and the
        // canvas corner.
        for (x, y) in [(mid(x0, x1), mid(y0, y1)), (0, 0)] {
            let i = (y * W + x) as usize;
            assert_eq!(after[i], before[i], "{location} ({x},{y}) is untouched");
        }
        free(out);
    }
    for d in docs {
        free(d);
    }
}

#[test]
fn preserve_transparency_keeps_alpha_exactly() {
    let dir = TempDir::new().unwrap();
    let docs = rectangle(&dir, [0.3, 0.3, 0.6, 0.2, 1.0]);
    let doc = docs[0];
    let before = placed(doc, 1);
    let out = stroke(doc, "center", true).unwrap();
    let after = placed(out, 1);
    for (i, (a, b)) in before.iter().zip(&after).enumerate() {
        assert_eq!(a[4], b[4], "pixel {i}: alpha is kept");
    }
    let (x0, y0, _, y1) = RECT;
    let i = (((y0 + y1) / 2) * W + x0) as usize;
    let source = [STROKE_INK[0], STROKE_INK[1], STROKE_INK[2], STROKE_INK[3]];
    close(
        after[i],
        over(unit(before[i]), source, 0.5),
        "the inside half still paints",
    );
    // An outside stroke under Preserve Transparency lands on alpha 0 only.
    assert!(stroke(doc, "outside", true).unwrap().is_null());
    // A blend mode composites on `L = 1 − ink` (#547): Multiply inside, on
    // the opaque rectangle's strip, against the W3C step with B the product
    // of `L`s; Pass Through, a group's mode, refuses by name.
    let multiplied = edit(doc, &format!(r#"{{"op":"stroke_layer_edge","layer":1,"width":2,"location":"inside","ink":{STROKE_INK:?},"opacity":0.5,"blend_mode":1}}"#), &[]).unwrap();
    let moded = placed(multiplied, 1);
    let (x1, mid) = (RECT.2, (RECT.1 + RECT.3) / 2);
    for x in [x0, x1 - 1] {
        let i = (mid * W + x) as usize;
        let cb = unit(before[i]);
        let mut want = [0.0f64; 5];
        for c in 0..4 {
            let blended = 1.0 - (1.0 - cb[c]) * (1.0 - source[c]);
            want[c] = 0.5 * cb[c] + 0.5 * blended;
        }
        want[4] = 1.0;
        close(
            moded[i],
            want.map(|v| (v * 65535.0).round() as u16),
            "Multiply on L",
        );
    }
    free(multiplied);
    assert_eq!(
        edit(doc, &format!(r#"{{"op":"stroke_layer_edge","layer":1,"width":2,"location":"outside","ink":{STROKE_INK:?},"opacity":1,"blend_mode":27}}"#), &[]).unwrap_err(),
        "The Pass Through blend mode is unavailable in a CMYK document"
    );
    free(out);
    for d in docs {
        free(d);
    }
}

fn effective_mask(doc: *const PrecisionDocument, index: usize) -> Vec<u16> {
    let request = CString::new(format!(
        r#"{{"op":"read_plane","target":"effective_mask","index":{index}}}"#
    ))
    .unwrap();
    let mut out = vec![0u16; (W * H) as usize];
    let mut err = ptr::null_mut();
    assert!(
        unsafe {
            dr_precision_doc_read_plane(
                doc,
                request.as_ptr(),
                out.as_mut_ptr(),
                out.len(),
                LIMIT,
                0,
                ptr::null(),
                &mut err,
            )
        },
        "{}",
        take_err_string(err)
    );
    out
}

#[test]
fn apply_mask_bakes_coverage_into_alpha_and_keeps_the_ink() {
    let dir = TempDir::new().unwrap();
    let (doc, added) = base(&dir);
    let n = (W * H) as usize;
    // Layer 1 with every class of alpha.
    let coverage: Vec<u16> = (0..n as u64)
        .map(|i| 1 + (i * 2_654_435_761 % 65535) as u16)
        .collect();
    let painted = edit(
        added,
        r#"{"op":"paint_blend","layer":1,"ink":[0.2,0.5,0.9,0.1,1],"opacity":0.8}"#,
        &coverage,
    )
    .unwrap();
    let plane: Vec<u16> = (0..n as u32).map(|i| (i * 40_503 % 65536) as u16).collect();
    let masked = edit(
        painted,
        &format!(r#"{{"op":"set_mask","layer":1,"width":{W},"height":{H},"x":0,"y":0}}"#),
        &plane,
    )
    .unwrap();
    let before = placed(masked, 1);
    let applied = edit(masked, r#"{"op":"apply_mask","layer":1}"#, &[]).unwrap();
    assert!(unsafe { &*applied }.layer(1).unwrap().mask().is_none());
    let after = placed(applied, 1);
    for i in 0..n {
        assert_eq!(
            after[i][..4],
            before[i][..4],
            "pixel {i}: the ink is untouched"
        );
        let want = (f64::from(before[i][4]) * f64::from(plane[i]) / 65535.0).round() as u16;
        assert_eq!(after[i][4], want, "pixel {i}");
    }
    // A feathered mask bakes its effective coverage.
    let feathered = edit(
        masked,
        r#"{"op":"mask_properties","layer":1,"x":0,"y":0,"linked":true,"density":1,"feather":3,"enabled":true}"#,
        &[],
    )
    .unwrap();
    let effective = effective_mask(feathered, 1);
    let soft = edit(feathered, r#"{"op":"apply_mask","layer":1}"#, &[]).unwrap();
    let after = placed(soft, 1);
    for i in 0..n {
        assert_eq!(
            after[i][..4],
            before[i][..4],
            "pixel {i}: the ink is untouched"
        );
        let want = (f64::from(before[i][4]) * f64::from(effective[i]) / 65535.0).round() as u16;
        assert!(
            after[i][4].abs_diff(want) <= 1,
            "pixel {i}: {} vs {want}",
            after[i][4]
        );
    }
    for d in [soft, feathered, applied, masked, painted, added, doc] {
        free(d);
    }
}
