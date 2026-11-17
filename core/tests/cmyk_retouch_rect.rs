//! 13B-9c on a CMYK document's inks: an in-place Clone or Heal runs its two
//! `L` passes over the stroke's rect, not the canvas (`cmyk_retouch`'s rect
//! path: rect-sized pass documents, a plate document holding the taps only,
//! the join and the clone composite over the rect). The product path
//! (`dr_precision_doc_edit`, in canvas form and in rect form, the
//! `coverage_rect` naming the stroke's rect) is held to the full-frame seam
//! (`RetouchEdit::full_frame`, set after `parse`), which keeps the canvas
//! pass documents and the canvas plate: every case answers the same
//! document to the bit (`CmykImage::same_samples` on every layer, offsets
//! and layer count equal), or both answer nothing.
//!
//! The cases: blend modes and opacities, a selection plane, a
//! transparency-locked target, a layer smaller than the canvas at an offset
//! (partly off it), a scaled and rotated source matrix, and a matrix that
//! puts every tap off the canvas (nothing to sample, so nothing changes),
//! for the Clone and the Heal.

use doctored_core::doc_lock::LOCK_TRANSPARENCY;
use doctored_core::precision_document::PrecisionDocument;
use doctored_core::precision_retouch::RetouchEdit;
use serde_json::{json, Value};

use crate::cmyk_structure_fixture::{cmyk_layers, free, try_edit, RgbLayer, LIMIT};

const W: u32 = 120;
const H: u32 = 90;
const N: usize = W as usize * H as usize;
/// The offset layer: partly off the canvas's right edge.
const OFFSET: (i32, i32, u32, u32) = (70, 20, 64, 50);
/// The dab: full coverage with a two-pixel feathered ring.
const DAB: (u32, u32, u32, u32) = (80, 34, 20, 16);
/// The rect the rect-form requests name: the dab's ring grown by 1.
const RECT: (u32, u32, u32, u32) = (77, 31, 26, 22);

/// Samples varying in every channel, a few at partial alpha.
fn pattern(w: u32, h: u32, seed: u32) -> Vec<[u16; 4]> {
    (0..w * h)
        .map(|i| {
            let (x, y) = (i % w, i / w);
            [
                ((x * 2711 + y * 911 + seed * 1234) % 65536) as u16,
                ((x * 523 + y * 3301 + seed * 77) % 65536) as u16,
                (((x ^ y) * 1999 + seed) % 65536) as u16,
                if (x + y * 3 + seed).is_multiple_of(17) {
                    20000
                } else {
                    65535
                },
            ]
        })
        .collect()
}

/// A canvas layer under the offset layer.
fn document() -> *mut PrecisionDocument {
    let (x, y, w, h) = OFFSET;
    cmyk_layers(
        W,
        H,
        vec![
            RgbLayer {
                name: "Canvas",
                w: W,
                h: H,
                offset: (0, 0),
                pixels: pattern(W, H, 1),
            },
            RgbLayer {
                name: "Offset",
                w,
                h,
                offset: (x, y),
                pixels: pattern(w, h, 4),
            },
        ],
    )
}

fn coverage() -> Vec<u16> {
    let (x, y, w, h) = DAB;
    (0..N)
        .map(|i| {
            let (px, py) = ((i % W as usize) as i64, (i / W as usize) as i64);
            let dx = (i64::from(x) - px).max(px - i64::from(x + w) + 1).max(0);
            let dy = (i64::from(y) - py).max(py - i64::from(y + h) + 1).max(0);
            match dx.max(dy) {
                0 => 65535,
                1 => 40000,
                2 => 16000,
                _ => 0,
            }
        })
        .collect()
}

/// A selection ramping across the canvas, zero left of x = 84.
fn selection() -> Vec<u16> {
    (0..N)
        .map(|i| {
            let x = i % W as usize;
            if x < 84 {
                0
            } else {
                ((x - 83) * 65535 / (W as usize - 83)) as u16
            }
        })
        .collect()
}

fn crop(plane: &[u16], (x, y, w, h): (u32, u32, u32, u32)) -> Vec<u16> {
    let mut out = Vec::new();
    for row in y..y + h {
        let at = (row * W + x) as usize;
        out.extend_from_slice(&plane[at..at + w as usize]);
    }
    out
}

/// Where `a` and `b` differ, or `None` when their layers match to the bit.
fn difference(a: &PrecisionDocument, b: &PrecisionDocument) -> Option<String> {
    if a.layer_count() != b.layer_count() {
        return Some(format!(
            "{} layers against {}",
            a.layer_count(),
            b.layer_count()
        ));
    }
    for i in 0..a.layer_count() {
        let (la, lb) = (a.layer(i).unwrap(), b.layer(i).unwrap());
        if la.offset() != lb.offset() {
            return Some(format!("layer {i} offset"));
        }
        match (la.cmyk_source(), lb.cmyk_source()) {
            (Some(x), Some(y)) if x.same_samples(y) => {}
            (None, None) => {}
            _ => return Some(format!("layer {i}'s inks differ")),
        }
    }
    None
}

fn full_frame(
    doc: &PrecisionDocument,
    request: &Value,
    samples: &[u16],
) -> Result<Option<PrecisionDocument>, String> {
    let mut edit = RetouchEdit::parse(&request.to_string())?;
    edit.full_frame = true;
    doc.retouch(&edit, samples, LIMIT, 0, None)
}

/// The canvas form and the rect form each against the full-frame seam;
/// `changes` asks that the seam changes the document.
fn check(
    failures: &mut Vec<String>,
    label: &str,
    doc: *mut PrecisionDocument,
    request: &Value,
    (stroke, gate): (&[u16], Option<&[u16]>),
    changes: bool,
) {
    let mut canvas = stroke.to_vec();
    let mut rect = crop(stroke, RECT);
    if let Some(gate) = gate {
        canvas.extend_from_slice(gate);
        rect.extend(crop(gate, RECT));
    }
    let oracle = full_frame(unsafe { &*doc }, request, &canvas);
    let mut rect_request = request.clone();
    rect_request["coverage_rect"] = json!([RECT.0, RECT.1, RECT.2, RECT.3]);
    for (form, request, samples) in [
        ("canvas", request.clone(), &canvas),
        ("rect", rect_request, &rect),
    ] {
        let product = try_edit(doc, &request.to_string(), samples);
        match (&product, &oracle) {
            (Ok(p), Ok(Some(o))) if !p.is_null() => {
                if let Some(why) = difference(unsafe { &**p }, o) {
                    failures.push(format!("{label}, {form} form: {why}"));
                }
            }
            (Ok(p), Ok(None)) if p.is_null() && !changes => {}
            _ => failures.push(format!(
                "{label}, {form} form: product {:?}, full frame {:?}",
                product.as_ref().map(|p| !p.is_null()),
                oracle.as_ref().map(|o| o.is_some())
            )),
        }
        if let Ok(p) = product {
            if !p.is_null() {
                free(p);
            }
        }
    }
}

#[test]
fn the_rect_passes_equal_the_full_frame_passes() {
    let doc = document();
    let locked = Box::into_raw(Box::new(
        unsafe { &*doc }
            .with_layer_locks(1, LOCK_TRANSPARENCY)
            .unwrap()
            .unwrap(),
    ));
    let stroke = coverage();
    let gate = selection();
    // Inside the offset layer, so its own heal finds a source there.
    let shift = [1.0, 0.0, 0.0, 1.0, 10.5, 9.25];
    let (s, c) = (0.3f64.sin() * 1.07, 0.3f64.cos() * 1.07);
    let turned = [c, -s, s, c, -40.0, 12.0];
    let away = [1.0, 0.0, 0.0, 1.0, 10_000.0, 0.0];
    let mut failures = Vec::new();
    let f = &mut failures;
    for op in ["retouch_clone", "retouch_heal"] {
        let base = |layer: usize, matrix: [f64; 6]| json!({"op": op, "layer": layer, "sample": "current", "matrix": matrix});
        for layer in [0usize, 1] {
            let label = format!("{op} layer {layer}");
            check(f, &label, doc, &base(layer, shift), (&stroke, None), true);
            let mut faint = base(layer, shift);
            faint["opacity"] = json!(0.45);
            check(
                f,
                &format!("{label} opacity 0.45"),
                doc,
                &faint,
                (&stroke, None),
                true,
            );
            let mut selected = base(layer, shift);
            selected["selection"] = json!(true);
            check(
                f,
                &format!("{label} selection"),
                doc,
                &selected,
                (&stroke, Some(&gate)),
                true,
            );
            check(
                f,
                &format!("{label} scaled and rotated"),
                doc,
                &base(layer, turned),
                (&stroke, None),
                true,
            );
            check(
                f,
                &format!("{label} every tap off the canvas"),
                doc,
                &base(layer, away),
                (&stroke, None),
                false,
            );
        }
        check(
            f,
            &format!("{op} transparency-locked offset layer"),
            locked,
            &base(1, shift),
            (&stroke, None),
            true,
        );
        // A source layer below the target.
        let mut sourced = base(1, shift);
        sourced["source_layer"] = json!(0);
        check(
            f,
            &format!("{op} source layer 0"),
            doc,
            &sourced,
            (&stroke, None),
            true,
        );
    }
    // Multiply, Screen, Darken, Color Dodge, Dissolve (which reads the
    // canvas position) and Darker Color.
    for mode in [1, 2, 6, 10, 14, 16] {
        let request = json!({"op": "retouch_clone", "layer": 0, "sample": "current",
            "matrix": shift, "opacity": 0.8, "blend_mode": mode});
        check(
            f,
            &format!("retouch_clone blend mode {mode}"),
            doc,
            &request,
            (&stroke, None),
            true,
        );
    }
    free(locked);
    free(doc);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
