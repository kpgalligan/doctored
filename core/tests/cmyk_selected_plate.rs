//! One ink plate's SELECTED PIXELS on a CMYK document (#547, the channels
//! batch: `cmyk_selected_plate`), through `dr_precision_doc_edit` with
//! `"target":"cyan"|"magenta"|"yellow"|"black"`: a move and a 90° transform
//! of each plate at 8 and 16 bits against this file's own shift and
//! index-remap oracle — the target plate moved, the vacated area the
//! swatch's gray read as ink (HG, on the 8-bit grid), the other three
//! plates and the alpha bit-identical; ⌥ duplicate leaves no hole; an
//! all-transparent lift refuses by its sentence; Lock Pixels
//! refuses; the take's recorded verdict (`fixtures/photoshop_cmyk_loads`,
//! `scripts/photoshop-probe/cmyk_channels_take.py` take1) is pinned; and the
//! refusals by name.

use std::ptr;

use doctored_core::ffi_precision::dr_precision_doc_open;
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer};
use doctored_core::precision_image::PrecisionImage;
use serde_json::{json, Value};
use tempfile::TempDir;

use crate::cmyk_fixture::press_profile;
use crate::cmyk_structure_fixture::{
    cmyk_layers, free, plan, refusal, then, try_edit, RgbLayer, LIMIT,
};
use crate::cmyk_tiff_builder::cmyk_tiff;
use crate::common::{cpath, take_err_string};

const W: u32 = 12;
const H: u32 = 9;
const PLATES: [&str; 4] = ["cyan", "magenta", "yellow", "black"];
/// take1's background swatch.
const SWATCH: [u8; 3] = [220, 30, 170];

/// An opaque CMYK document of distinct plates at `bits`.
fn doc_at(dir: &TempDir, bits: u16) -> *mut PrecisionDocument {
    let samples: Vec<[u16; 4]> = (0..W * H)
        .map(|i| {
            let j = i.wrapping_mul(2_654_435_761);
            [
                (j % 65536) as u16,
                (j.rotate_left(7) % 65536) as u16,
                (j.rotate_left(15) % 65536) as u16,
                (j.rotate_left(23) % 65536) as u16,
            ]
        })
        .collect();
    let data: Vec<u8> = match bits {
        8 => samples
            .iter()
            .flatten()
            .map(|v| ((u32::from(*v) + 128) / 257) as u8)
            .collect(),
        _ => samples
            .iter()
            .flatten()
            .flat_map(|v| v.to_le_bytes())
            .collect(),
    };
    let path = dir.path().join(format!("plates{bits}.tif"));
    let icc = press_profile("Press Plates");
    std::fs::write(&path, cmyk_tiff(W, H, bits, 4, 0, &data, Some(&icc))).unwrap();
    let c = cpath(&path);
    let mut err = ptr::null_mut();
    let doc = unsafe { dr_precision_doc_open(c.as_ptr(), LIMIT, 0, ptr::null(), &mut err) };
    assert!(!doc.is_null(), "open: {}", take_err_string(err));
    doc
}

fn samples(doc: *const PrecisionDocument) -> Vec<[u16; 5]> {
    unsafe { &*doc }.cmyk_layer_samples(0).unwrap()
}

/// The selection: 65535 over `(x, y, w, h)`, 0 elsewhere.
fn rect(x: u32, y: u32, w: u32, h: u32) -> Vec<u16> {
    (0..W * H)
        .map(|i| {
            let (px, py) = (i % W, i / W);
            if px >= x && px < x + w && py >= y && py < y + h {
                65535
            } else {
                0
            }
        })
        .collect()
}

/// The hole every plate's vacated area takes at either depth (module doc of
/// the tested module, HG): 255 − the swatch's 30/59/11 luma rounded half up,
/// as an 8-bit code stored × 257. (220, 30, 170): 255 − 102 = 153.
fn hole() -> u16 {
    let [r, g, b] = SWATCH.map(u32::from);
    let gray = (30 * r + 59 * g + 11 * b + 50) / 100;
    assert_eq!(gray, 102);
    ((255 - gray) * 257) as u16
}

/// The plate after a remap: `map(x, y)` is where source pixel (x, y) of the
/// selection lands (inside the canvas), the selection's pixels not landed on
/// take `hole`, everything else is kept.
fn oracle(
    before: &[[u16; 5]],
    k: usize,
    selection: &[u16],
    map: &dyn Fn(u32, u32) -> (u32, u32),
    hole: Option<u16>,
) -> Vec<u16> {
    let mut out: Vec<u16> = before.iter().map(|p| p[k]).collect();
    if let Some(hole) = hole {
        for (i, c) in selection.iter().enumerate() {
            if *c > 0 {
                out[i] = hole;
            }
        }
    }
    for (i, c) in selection.iter().enumerate() {
        if *c > 0 {
            let (x, y) = map(i as u32 % W, i as u32 / W);
            if x < W && y < H {
                out[(y * W + x) as usize] = before[i][k];
            }
        }
    }
    out
}

fn assert_plate(before: &[[u16; 5]], after: &[[u16; 5]], k: usize, want: &[u16], what: &str) {
    for (i, (b, a)) in before.iter().zip(after).enumerate() {
        assert_eq!(a[k], want[i], "{what}: pixel {i} plate {k}");
        for c in (0..5).filter(|c| *c != k) {
            assert_eq!(
                a[c], b[c],
                "{what}: pixel {i} component {c} must be untouched"
            );
        }
    }
}

#[test]
fn a_moved_plate_shifts_and_leaves_the_swatch_ink() {
    let dir = TempDir::new().unwrap();
    for bits in [8u16, 16] {
        let doc = doc_at(&dir, bits);
        let before = samples(doc);
        let selection = rect(2, 1, 5, 4);
        for (k, plate) in PLATES.iter().enumerate() {
            let request = json!({"op":"move_selected","layer":0,"dx":3,"dy":2,"target":plate,"swatch":SWATCH});
            let moved = try_edit(doc, &request.to_string(), &selection).unwrap();
            let want = oracle(&before, k, &selection, &|x, y| (x + 3, y + 2), Some(hole()));
            assert_plate(
                &before,
                &samples(moved),
                k,
                &want,
                &format!("{bits}-bit move {plate}"),
            );
            free(moved);
            // ⌥-drag duplicates: no hole.
            let request = json!({"op":"move_selected","layer":0,"dx":3,"dy":2,"target":plate,"duplicate":true});
            let copied = try_edit(doc, &request.to_string(), &selection).unwrap();
            let want = oracle(&before, k, &selection, &|x, y| (x + 3, y + 2), None);
            assert_plate(
                &before,
                &samples(copied),
                k,
                &want,
                &format!("{bits}-bit duplicate {plate}"),
            );
            free(copied);
        }
        // Moved past the canvas edge, the plate is clipped to the layer.
        let request =
            json!({"op":"move_selected","layer":0,"dx":9,"dy":0,"target":"black","swatch":SWATCH});
        let clipped = try_edit(doc, &request.to_string(), &selection).unwrap();
        let want = oracle(&before, 3, &selection, &|x, y| (x + 9, y), Some(hole()));
        assert_plate(&before, &samples(clipped), 3, &want, "clipped");
        free(clipped);
        free(doc);
    }
}

#[test]
fn a_turned_plate_is_the_index_remap() {
    let dir = TempDir::new().unwrap();
    for bits in [8u16, 16] {
        let doc = doc_at(&dir, bits);
        let before = samples(doc);
        // 90° clockwise about (5, 4): x' = 9 − y, y' = x − 1 in canvas
        // coordinates, so pixel (x, y) lands on (8 − y, x − 1).
        let selection = rect(2, 2, 6, 4);
        for (k, plate) in PLATES.iter().enumerate() {
            let request = json!({"op":"transform_selected","layer":0,"matrix":[0.0,1.0,-1.0,0.0,9.0,-1.0],
                "filter":"nearest","target":plate,"swatch":SWATCH});
            let turned = try_edit(doc, &request.to_string(), &selection).unwrap();
            let want = oracle(&before, k, &selection, &|x, y| (8 - y, x - 1), Some(hole()));
            assert_plate(
                &before,
                &samples(turned),
                k,
                &want,
                &format!("{bits}-bit turn {plate}"),
            );
            free(turned);
        }
        free(doc);
    }
}

#[test]
fn an_empty_lift_a_lock_and_the_wrong_targets_refuse_by_name() {
    // A layer transparent in its top-left 4 × 4.
    let pixels: Vec<[u16; 4]> = (0..W * H)
        .map(|i| {
            let alpha = if i % W < 4 && i / W < 4 { 0 } else { 65535 };
            [(i * 997 % 65536) as u16, 20000, 40000, alpha]
        })
        .collect();
    let doc = cmyk_layers(
        W,
        H,
        vec![RgbLayer {
            name: "Holes",
            w: W,
            h: H,
            offset: (0, 0),
            pixels,
        }],
    );
    let lift = rect(0, 0, 3, 3);
    let request = json!({"op":"move_selected","layer":0,"dx":5,"dy":1,"target":"cyan"}).to_string();
    assert_eq!(
        refusal(doc, &request, &lift),
        "The selection holds no pixels in this colour channel."
    );
    let locked = then(
        cmyk_layers(
            W,
            H,
            vec![RgbLayer {
                name: "L",
                w: W,
                h: H,
                offset: (0, 0),
                pixels: vec![[30000, 20000, 10000, 65535]; (W * H) as usize],
            }],
        ),
        r#"{"op":"layer_lock","layer":0,"locks":2}"#,
        &[],
    );
    let refused = refusal(locked, &request, &rect(2, 2, 3, 3));
    assert_eq!(refused, "high-depth layer 0 is locked (pixels)");
    assert_eq!(
        refusal(
            doc,
            &json!({"op":"move_selected","layer":0,"dx":1,"dy":1,"target":"layer","swatch":SWATCH})
                .to_string(),
            &lift
        ),
        "swatch applies to a red, green, blue, cyan, magenta, yellow or black target only"
    );
    assert_eq!(
        refusal(doc, &json!({"op":"move_selected","layer":0,"dx":1,"dy":1,"target":"green"}).to_string(), &lift),
        "Moving or transforming the \"green\" channel's selected pixels is unavailable in a CMYK document"
    );
    // The plan answers for a plate target.
    let planned = plan(
        doc,
        &json!({"op":"move_selected","layer":0,"dx":1,"dy":1,"target":"black"}).to_string(),
    )
    .unwrap();
    assert!(planned["scratch_bytes"].as_u64().unwrap() > 0);
    free(locked);
    free(doc);
    // An ink plate on an RGB document.
    let srgb = IccProfile::srgb();
    let image = PrecisionImage::from_rgba16(
        W,
        H,
        vec![[1000, 2000, 3000, 65535]; (W * H) as usize],
        srgb.clone(),
        None,
    )
    .unwrap();
    let rgb = Box::into_raw(Box::new(
        PrecisionDocument::from_parts(
            W,
            H,
            srgb,
            Metadata::default(),
            Resolution::default(),
            vec![PrecisionDocumentLayer::raster("L", image, (0, 0), 1.0, true).unwrap()],
        )
        .unwrap(),
    ));
    assert_eq!(
        refusal(
            rgb,
            &json!({"op":"move_selected","layer":0,"dx":1,"dy":1,"target":"cyan"}).to_string(),
            &lift
        ),
        "The cyan channel requires a CMYK document"
    );
    free(rgb);
}

#[test]
fn the_take_found_photoshop_moving_one_plate() {
    let manifest: Value = serde_json::from_str(
        &std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/photoshop_cmyk_loads/manifest.json"
        ))
        .unwrap(),
    )
    .unwrap();
    let take = &manifest["plate_move"];
    assert_eq!(take["verdict"], "ALLOWED");
    assert_eq!(take["checks"]["translate_others_untouched"], true);
    assert_eq!(take["checks"]["translate_moved_exact"], true);
    assert_eq!(take["hole_member"], "HG");
    assert_eq!(take["holes"]["HG"]["value"], 153);
    assert_eq!(take["rotate"]["clockwise_vacated"], json!([156]));
    assert!(take["lift"]["error"]
        .as_str()
        .unwrap()
        .contains("Could not complete the command because the selected area is empty."));
}
