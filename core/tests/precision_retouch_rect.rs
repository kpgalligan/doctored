//! 13B-9a: a typed Clone or Heal is RECT-FIRST (`precision_retouch_apply`'s
//! module doc). The product path (`dr_precision_doc_edit`) transforms the
//! source over the coverage's bound only, from the taps of one source rect;
//! the full-frame seam (`RetouchEdit::full_frame`, set after `parse`) runs
//! the whole-canvas transform from the whole plate. Every case answers the
//! same document as that seam to the bit (`PrecisionImage::same_samples` on
//! every layer, offsets and layer count equal), at 16 bits and 32-bit float,
//! for Clone and Heal, over translations, a rotation with scale, the canvas
//! corner, a selection plane, a smaller offset source layer, the `all` and
//! `current_and_below` plates, new-layer output, a grown destination and a
//! transparency-locked target.
//!
//! One case is independent of both paths' rect machinery: an opaque 16-bit
//! Clone at opacity 1 writes, wherever its taps all lie on the canvas, the
//! samples `dr_precision_image_transform_retouch_source` (the whole-canvas
//! master transform) answers there, and nothing outside its coverage.

use std::ffi::{CStr, CString};
use std::ptr;

use doctored_core::doc_lock::LOCK_TRANSPARENCY;
use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_precision_edit::dr_precision_doc_edit;
use doctored_core::ffi_retouch::dr_precision_image_transform_retouch_source;
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_clipboard::{ClipboardPayload, PasteRequest};
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer as Layer};
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use doctored_core::precision_retouch::RetouchEdit;
use serde_json::{json, Value};

use crate::float_fixture::{drcb, float_doc};

const LIMIT: u64 = REFERENCE_WORKING_BYTES;
const W: u32 = 96;
const H: u32 = 72;
const N: usize = W as usize * H as usize;
/// The dab every interior case covers.
const DAB: (u32, u32, u32, u32) = (30, 22, 24, 18);
/// The smaller source / target layer's rect on the canvas.
const SMALL: (u32, u32, u32, u32) = (25, 18, 40, 30);

// ------------------------------------------------------------- fixtures --

/// Samples varying in every channel, with a partly transparent lattice.
fn pattern16(seed: u32) -> Vec<[u16; 4]> {
    (0..W * H)
        .map(|i| {
            let (x, y) = (i % W, i / W);
            [
                ((x * 2711 + y * 911 + seed * 1234) % 65536) as u16,
                ((x * 523 + y * 3301 + seed * 77) % 65536) as u16,
                (((x ^ y) * 1999 + seed) % 65536) as u16,
                if (x + 2 * y + seed).is_multiple_of(11) {
                    30000
                } else {
                    65535
                },
            ]
        })
        .collect()
}

/// `pattern16` in an extended linear range (above 1 and below 0).
fn pattern32(seed: u32) -> Vec<[f32; 4]> {
    pattern16(seed)
        .into_iter()
        .map(|p| {
            [
                f32::from(p[0]) / 65535.0 * 2.0 - 0.25,
                f32::from(p[1]) / 65535.0,
                f32::from(p[2]) / 65535.0 * 0.5,
                f32::from(p[3]) / 65535.0,
            ]
        })
        .collect()
}

fn inside(i: usize, (x, y, w, h): (u32, u32, u32, u32)) -> bool {
    let (px, py) = ((i % W as usize) as u32, (i / W as usize) as u32);
    px >= x && px < x + w && py >= y && py < y + h
}

/// `doc` with `pattern32(seed)`'s `SMALL` rect pasted in place above
/// record 0: an RZCB v3 float payload (`float_fixture::drcb`) of the rect's
/// size whose offset words (bytes 20–27, after the magic, the version, the
/// storage and the size) are the rect's origin.
fn with_small32(doc: PrecisionDocument, seed: u32) -> PrecisionDocument {
    let (x, y, w, h) = SMALL;
    let pixels: Vec<[f32; 4]> = pattern32(seed)
        .into_iter()
        .enumerate()
        .filter(|(i, _)| inside(*i, SMALL))
        .map(|(_, p)| p)
        .collect();
    let mut bytes = drcb(w, h, IccProfile::srgb().bytes(), &pixels);
    bytes[20..24].copy_from_slice(&(x as i32).to_le_bytes());
    bytes[24..28].copy_from_slice(&(y as i32).to_le_bytes());
    let payload = ClipboardPayload::decode(&bytes, LIMIT, 0).unwrap();
    let request: PasteRequest = serde_json::from_value(json!({
        "name": "S", "anchor": 0, "position": "above",
        "placement": "in_place", "profile": "preserve",
    }))
    .unwrap();
    doc.clipboard_paste(&payload, &request, LIMIT, 0, None)
        .unwrap()
}

fn image16(w: u32, h: u32, pixels: Vec<[u16; 4]>) -> PrecisionImage {
    PrecisionImage::from_rgba16(w, h, pixels, IccProfile::srgb(), None).unwrap()
}

fn raster16(seed: u32) -> Layer {
    Layer::raster("L", image16(W, H, pattern16(seed)), (0, 0), 1.0, true).unwrap()
}

fn small16(seed: u32) -> Layer {
    let (x, y, w, h) = SMALL;
    let pixels: Vec<[u16; 4]> = pattern16(seed)
        .into_iter()
        .enumerate()
        .filter(|(i, _)| inside(*i, SMALL))
        .map(|(_, p)| p)
        .collect();
    Layer::raster("S", image16(w, h, pixels), (x as i32, y as i32), 1.0, true).unwrap()
}

fn document16(layers: Vec<Layer>) -> PrecisionDocument {
    PrecisionDocument::from_parts(
        W,
        H,
        IccProfile::srgb(),
        Metadata::default(),
        Resolution::default(),
        layers,
    )
    .unwrap()
}

/// The documents each depth's cases run on: one layer; a canvas layer under
/// a smaller one at an offset; three canvas layers.
struct Docs {
    one: PrecisionDocument,
    two: PrecisionDocument,
    three: PrecisionDocument,
}

fn docs16() -> Docs {
    Docs {
        one: document16(vec![raster16(1)]),
        two: document16(vec![raster16(1), small16(2)]),
        three: document16(vec![raster16(1), raster16(2), raster16(3)]),
    }
}

fn docs32() -> Docs {
    let srgb = IccProfile::srgb();
    let docs = Docs {
        one: float_doc(W, H, &srgb, &[pattern32(1)]),
        two: with_small32(float_doc(W, H, &srgb, &[pattern32(1)]), 2),
        three: float_doc(W, H, &srgb, &[pattern32(1), pattern32(2), pattern32(3)]),
    };
    let small = docs.two.layer(1).unwrap();
    assert_eq!(
        (small.offset(), small.source().unwrap().dimensions()),
        ((SMALL.0 as i32, SMALL.1 as i32), (SMALL.2, SMALL.3)),
        "the float source layer is the smaller one at its offset"
    );
    docs
}

/// Full coverage over `rect` with a two-pixel feathered ring around it.
fn coverage(rect: (u32, u32, u32, u32)) -> Vec<u16> {
    let (x, y, w, h) = rect;
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

/// `coverage(DAB)` followed by a selection plane ramping across the canvas.
fn selected() -> Vec<u16> {
    let mut out = coverage(DAB);
    out.extend((0..N).map(|i| ((i % W as usize) * 65535 / (W as usize - 1)) as u16));
    out
}

fn rotated() -> [f64; 6] {
    let (s, c) = (0.3f64.sin() * 1.07, 0.3f64.cos() * 1.07);
    [c, -s, s, c, -20.0, 5.0]
}

const SHIFT: [f64; 6] = [1.0, 0.0, 0.0, 1.0, 17.25, -9.5];

// ----------------------------------------------------------------- paths --

fn execute(
    doc: &PrecisionDocument,
    request: &Value,
    coverage: &[u16],
) -> Result<Option<Box<PrecisionDocument>>, String> {
    let text = CString::new(request.to_string()).unwrap();
    unsafe {
        let mut err = ptr::null_mut();
        let out = dr_precision_doc_edit(
            doc,
            text.as_ptr(),
            coverage.as_ptr(),
            coverage.len(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        );
        if !err.is_null() {
            let message = CStr::from_ptr(err).to_string_lossy().into_owned();
            dr_string_free(err);
            return Err(message);
        }
        Ok((!out.is_null()).then(|| Box::from_raw(out)))
    }
}

/// The full-frame seam: the whole-canvas transform from the whole plate.
fn full_frame(
    doc: &PrecisionDocument,
    request: &Value,
    coverage: &[u16],
) -> Result<Option<PrecisionDocument>, String> {
    let mut edit = RetouchEdit::parse(&request.to_string())?;
    edit.full_frame = true;
    doc.retouch(&edit, coverage, LIMIT, 0, None)
}

/// Where `a` and `b` differ, or `None` when they are the same document's
/// layers to the bit.
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
            return Some(format!(
                "layer {i} offset {:?} against {:?}",
                la.offset(),
                lb.offset()
            ));
        }
        match (la.source(), lb.source()) {
            (Some(x), Some(y)) if x.same_samples(y) => {}
            (None, None) => {}
            _ => return Some(format!("layer {i}'s samples differ")),
        }
    }
    None
}

/// One case: the product and the seam both change the document, alike.
fn check(
    failures: &mut Vec<String>,
    label: &str,
    doc: &PrecisionDocument,
    request: Value,
    coverage: &[u16],
) {
    let product = execute(doc, &request, coverage);
    let oracle = full_frame(doc, &request, coverage);
    match (product, oracle) {
        (Ok(Some(product)), Ok(Some(oracle))) => {
            if let Some(why) = difference(&product, &oracle) {
                failures.push(format!("{label}: {why}"));
            }
        }
        (product, oracle) => failures.push(format!(
            "{label}: product {:?}, full frame {:?}",
            product.map(|d| d.is_some()),
            oracle.map(|d| d.is_some())
        )),
    }
}

fn cases(depth: &str, docs: &Docs) -> Vec<String> {
    let locked = docs
        .one
        .with_layer_locks(0, LOCK_TRANSPARENCY)
        .unwrap()
        .unwrap();
    let mut failures = Vec::new();
    for op in ["retouch_clone", "retouch_heal"] {
        let label = |case: &str| format!("{depth} {op} {case}");
        let request = |extra: Value| {
            let mut r = json!({"op": op, "layer": 0, "sample": "current", "matrix": SHIFT,
                "opacity": 0.85});
            for (k, v) in extra.as_object().unwrap() {
                r[k] = v.clone();
            }
            r
        };
        let dab = coverage(DAB);
        let f = &mut failures;
        check(
            f,
            &label("translation"),
            &docs.one,
            request(json!({})),
            &dab,
        );
        // No feather: covered pixels reach the coverage's bbox, so a Heal's
        // membrane ring lies only in the bound's grown pixel.
        let hard: Vec<u16> = (0..N)
            .map(|i| if inside(i, DAB) { 65535 } else { 0 })
            .collect();
        check(f, &label("hard edge"), &docs.one, request(json!({})), &hard);
        let turned = json!({"matrix": rotated()});
        check(
            f,
            &label("rotated and scaled"),
            &docs.one,
            request(turned),
            &dab,
        );
        let corner = coverage((0, H - 14, 20, 14));
        let up = json!({"matrix": [1.0, 0.0, 0.0, 1.0, -6.5, -20.25]});
        check(f, &label("canvas corner"), &docs.one, request(up), &corner);
        let selection = json!({"selection": true, "matrix": rotated()});
        check(
            f,
            &label("selection"),
            &docs.one,
            request(selection),
            &selected(),
        );
        let source = json!({"source_layer": 1, "matrix": [1.0, 0.0, 0.0, 1.0, 4.75, 3.5]});
        check(
            f,
            &label("smaller offset source"),
            &docs.two,
            request(source),
            &dab,
        );
        for sample in ["all", "current_and_below"] {
            let plate = json!({"layer": 1, "sample": sample, "matrix": rotated()});
            check(f, &label(sample), &docs.three, request(plate), &dab);
        }
        let output = json!({"output": "new_layer"});
        check(f, &label("new layer"), &docs.one, request(output), &dab);
        // Across the smaller layer's left edge, sampling inside it.
        let grown = json!({"layer": 1, "expand_destination": true,
            "matrix": [1.0, 0.0, 0.0, 1.0, 20.5, 1.25]});
        let edge = coverage((15, 30, 20, 12));
        check(
            f,
            &label("grown destination"),
            &docs.two,
            request(grown),
            &edge,
        );
        check(
            f,
            &label("locked transparency"),
            &locked,
            request(json!({})),
            &dab,
        );
    }
    failures
}

// ---------------------------------------------------------------- tests --

#[test]
fn the_16_bit_rect_path_equals_the_full_frame_path() {
    let failures = cases("16-bit", &docs16());
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn the_float_rect_path_equals_the_full_frame_path() {
    let failures = cases("float", &docs32());
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn an_opaque_clone_writes_the_master_transform_s_samples() {
    let opaque: Vec<[u16; 4]> = pattern16(5)
        .into_iter()
        .map(|p| [p[0], p[1], p[2], 65535])
        .collect();
    let doc = document16(vec![Layer::raster(
        "L",
        image16(W, H, opaque.clone()),
        (0, 0),
        1.0,
        true,
    )
    .unwrap()]);
    let dab: Vec<u16> = (0..N)
        .map(|i| if inside(i, DAB) { 65535 } else { 0 })
        .collect();
    let matrix = rotated();
    let request = json!({"op":"retouch_clone","layer":0,"sample":"current","matrix":matrix,
        "opacity":1.0});
    let out = execute(&doc, &request, &dab)
        .expect("the clone runs")
        .expect("the clone changes the layer");
    let written = out.layer_source(0).unwrap().to_rgba16().unwrap();
    let mut err = ptr::null_mut();
    let source = doc.layer_source(0).unwrap();
    let moved = unsafe {
        dr_precision_image_transform_retouch_source(
            source,
            matrix.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(
        !moved.is_null() && err.is_null(),
        "the master transform runs"
    );
    let moved = unsafe { Box::from_raw(moved) }.to_rgba16().unwrap();
    let mut judged = 0;
    for i in 0..N {
        if !inside(i, DAB) {
            assert_eq!(
                written[i], opaque[i],
                "pixel {i} outside the coverage keeps its samples"
            );
        } else if moved[i][3] == 65535 {
            assert_eq!(
                written[i], moved[i],
                "pixel {i}: the master transform's sample"
            );
            judged += 1;
        }
    }
    assert!(
        judged > 300,
        "{judged} pixels whose taps all lie on the canvas"
    );
}
