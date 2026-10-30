//! Selections on a CMYK document (#1): the Magic Wand on INK through
//! `dr_precision_doc_select` against this file's own projection-and-BFS
//! oracle (PN7: the ink's naive RGB `(1 − ink)(1 − K)`, rounded through
//! 8-bit codes on an 8-bit document, then R, G, B, A, R−G, G−B, R−B each
//! within ± tolerance of the seed's, four-connected, written from
//! `selection_region`'s module doc), over the composite and one layer at 8
//! and 16 bits; the Photoshop 26.10.0 takes' recorded masks
//! (`fixtures/photoshop_cmyk_wand/manifest.json`, take1 of
//! `scripts/photoshop-probe/cmyk_select_take.py`, re-derived for PN7, and
//! `confirm.json`, the confirming take of `cmyk_k_take.py`) with the shipped
//! member's residual pinned region by region; Color Range, Quick
//! Selection and the edge stage, which must equal the same kinds on an RGB16
//! document built from the CMYK document's own view; Select and Mask's
//! CMYK New Layer (`dr_precision_doc_add_cmyk_cutout`), with Decontaminate
//! Colors equal to the RGB16 encoded estimate run on C/M/Y and on K; and
//! the view kinds' refused layer source by name.

use std::ffi::{CStr, CString};
use std::ptr;

use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_cmyk::{
    dr_precision_doc_add_cmyk_cutout, dr_precision_doc_cmyk_cutout_plan,
    dr_precision_doc_cmyk_sample,
};
use doctored_core::ffi_foreground::dr_precision_doc_estimate_foreground;
use doctored_core::ffi_precision::{dr_precision_doc_free, dr_precision_doc_open};
use doctored_core::ffi_precision_edit::{
    dr_precision_doc_edit, dr_precision_doc_edit_plan, dr_precision_doc_read_plane,
    dr_precision_doc_select,
};
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::PrecisionDocument;
use doctored_core::precision_image::PrecisionImage;
use serde_json::{json, Value};
use tempfile::TempDir;

use crate::cmyk_fixture::press_profile;
use crate::cmyk_tiff_builder::cmyk_tiff;
use crate::common::{cpath, take_err_string};

const LIMIT: u64 = 2 * 1024 * 1024 * 1024;

type Ink = [u16; 5];

fn free(document: *mut PrecisionDocument) {
    unsafe { dr_precision_doc_free(document) };
}

/// An opaque 16-bit CMYK document of `samples` (C, M, Y, K codes) in the
/// synthetic press profile, opened through the typed open.
fn cmyk_doc(
    dir: &TempDir,
    name: &str,
    w: u32,
    h: u32,
    samples: &[[u16; 4]],
) -> *mut PrecisionDocument {
    cmyk_doc_at(dir, name, w, h, samples, 16)
}

/// [`cmyk_doc`] at `bits` (8 stores `round(v / 257)`).
fn cmyk_doc_at(
    dir: &TempDir,
    name: &str,
    w: u32,
    h: u32,
    samples: &[[u16; 4]],
    bits: u16,
) -> *mut PrecisionDocument {
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
    let path = dir.path().join(name);
    let icc = press_profile("Press Select");
    std::fs::write(&path, cmyk_tiff(w, h, bits, 4, 0, &data, Some(&icc))).unwrap();
    let c = cpath(&path);
    let mut err = ptr::null_mut();
    let doc = unsafe { dr_precision_doc_open(c.as_ptr(), LIMIT, 0, ptr::null(), &mut err) };
    assert!(!doc.is_null(), "open: {}", take_err_string(err));
    doc
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

fn plan(doc: *const PrecisionDocument, request: &str) -> Result<Value, String> {
    let text = CString::new(request).unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe { dr_precision_doc_edit_plan(doc, text.as_ptr(), LIMIT, 0, &mut err) };
    if out.is_null() {
        return Err(take_err_string(err));
    }
    let value = serde_json::from_str(unsafe { CStr::from_ptr(out) }.to_str().unwrap()).unwrap();
    unsafe { dr_string_free(out) };
    Ok(value)
}

/// `dr_precision_doc_select` of `request` (a whole selection request).
fn select(
    doc: *const PrecisionDocument,
    request: &Value,
    input: &[u16],
) -> Result<Vec<u16>, String> {
    let text = CString::new(request.to_string()).unwrap();
    let planned = plan(doc, &request.to_string())?;
    let mut out = vec![0u16; planned["output_samples"].as_u64().unwrap() as usize];
    let mut err = ptr::null_mut();
    let ok = unsafe {
        dr_precision_doc_select(
            doc,
            text.as_ptr(),
            if input.is_empty() {
                ptr::null()
            } else {
                input.as_ptr()
            },
            input.len(),
            out.as_mut_ptr(),
            out.len(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    if ok {
        Ok(out)
    } else {
        Err(take_err_string(err))
    }
}

fn wand(x: u32, y: u32, tolerance: u16, contiguous: bool) -> Value {
    json!({"kind":"magic_wand","x":x,"y":y,"tolerance":tolerance,"contiguous":contiguous})
}

/// The composite's straight ink at every pixel, read point by point through
/// `dr_precision_doc_cmyk_sample` (reach 0).
fn composite_ink(doc: *const PrecisionDocument, w: u32, h: u32) -> Vec<Ink> {
    let mut out = Vec::new();
    for y in 0..h {
        for x in 0..w {
            let mut err = ptr::null_mut();
            let text = unsafe {
                dr_precision_doc_cmyk_sample(doc, x as i32, y as i32, 0, LIMIT, 0, &mut err)
            };
            assert!(!text.is_null(), "{}", take_err_string(err));
            let value: Value =
                serde_json::from_str(unsafe { CStr::from_ptr(text) }.to_str().unwrap()).unwrap();
            unsafe { dr_string_free(text) };
            let ink: Vec<u16> = value["ink16"]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_u64().unwrap() as u16)
                .collect();
            out.push([
                ink[0],
                ink[1],
                ink[2],
                ink[3],
                value["alpha16"].as_u64().unwrap() as u16,
            ]);
        }
    }
    out
}

/// Record `index`'s own ink placed on the canvas at its offset; bare paper
/// (`[0; 5]`) where it does not reach.
fn placed_layer(doc: *const PrecisionDocument, index: usize, w: u32, h: u32) -> Vec<Ink> {
    let layer = unsafe { &*doc }.layer(index).unwrap();
    let (ox, oy) = layer.offset();
    let (lw, lh) = layer.cmyk_source().unwrap().dimensions();
    let samples = unsafe { &*doc }.cmyk_layer_samples(index).unwrap();
    (0..w * h)
        .map(|i| {
            let (x, y) = (
                i64::from(i % w) - i64::from(ox),
                i64::from(i / w) - i64::from(oy),
            );
            if x >= 0 && y >= 0 && x < i64::from(lw) && y < i64::from(lh) {
                samples[(y * i64::from(lw) + x) as usize]
            } else {
                [0; 5]
            }
        })
        .collect()
}

/// The oracle's projections, written out (module doc): the naive RGB of
/// the ink in 16-bit codes — at 8 bits `257 · round((255 − c8)(255 − k8) /
/// 255)` with `c8 = round(C / 257)`, at 16 `round((65535 − C)(65535 − K) /
/// 65535)` — then R, G, B, A, R−G, G−B, R−B.
fn projections(p: Ink, bits: u16) -> [i64; 7] {
    let [c, m, y, k, a] = p.map(f64::from);
    let plate = |v: f64| -> i64 {
        if bits == 8 {
            let (v8, k8) = ((v / 257.0).round(), (k / 257.0).round());
            257 * ((255.0 - v8) * (255.0 - k8) / 255.0).round() as i64
        } else {
            ((65535.0 - v) * (65535.0 - k) / 65535.0).round() as i64
        }
    };
    let (r, g, b) = (plate(c), plate(m), plate(y));
    [r, g, b, a as i64, r - g, g - b, r - b]
}

/// The oracle's region: global, or a four-connected BFS from the seed.
fn oracle(
    w: u32,
    h: u32,
    pixels: &[Ink],
    seed: (u32, u32),
    tolerance: u16,
    contiguous: bool,
    bits: u16,
) -> Vec<u16> {
    let start = (seed.1 * w + seed.0) as usize;
    let reference = projections(pixels[start], bits);
    let near = |i: usize| {
        projections(pixels[i], bits)
            .iter()
            .zip(reference)
            .all(|(a, b)| (a - b).abs() <= i64::from(tolerance))
    };
    if !contiguous {
        return (0..pixels.len())
            .map(|i| if near(i) { 65535 } else { 0 })
            .collect();
    }
    let mut out = vec![0u16; pixels.len()];
    out[start] = 65535;
    let mut queue = std::collections::VecDeque::from([start]);
    while let Some(i) = queue.pop_front() {
        let (x, y) = ((i as u32 % w) as i64, (i as u32 / w) as i64);
        for (nx, ny) in [(x - 1, y), (x + 1, y), (x, y - 1), (x, y + 1)] {
            if nx < 0 || ny < 0 || nx >= i64::from(w) || ny >= i64::from(h) {
                continue;
            }
            let j = (ny * i64::from(w) + nx) as usize;
            if out[j] == 0 && near(j) {
                out[j] = 65535;
                queue.push_back(j);
            }
        }
    }
    out
}

/// A varied opaque ink field: a base ink with deterministic per-pixel steps
/// up to ±40·257 per plate, so every tolerance meets a mix of near and far
/// pixels.
fn field(w: u32, h: u32) -> Vec<[u16; 4]> {
    let base = [30000i64, 24000, 20000, 15000];
    (0..w * h)
        .map(|i| {
            let i = u64::from(i);
            std::array::from_fn(|c| {
                let step = (((i * (2654435761 + c as u64 * 97)) >> 7) % 81) as i64 - 40;
                (base[c]
                    + step * 257
                        / if (i + c as u64).is_multiple_of(3) {
                            1
                        } else {
                            4
                        }) as u16
            })
        })
        .collect()
}

/// A two-layer document at `bits`: the field, and over it a layer painted
/// with partial coverage on its left half (so the composite and the top
/// layer differ).
fn layered(
    dir: &TempDir,
    w: u32,
    h: u32,
    bits: u16,
) -> (*mut PrecisionDocument, Vec<*mut PrecisionDocument>) {
    let base = cmyk_doc_at(dir, &format!("layered{bits}.tif"), w, h, &field(w, h), bits);
    let added = edit(
        base,
        r#"{"op":"add_layer","name":"Top","anchor":0,"position":"above"}"#,
        &[],
    )
    .unwrap();
    let stroke: Vec<u16> = (0..w * h)
        .map(|i| {
            if i % w < w / 2 {
                ((i * 977) % 65536) as u16
            } else {
                0
            }
        })
        .collect();
    let painted = edit(
        added,
        r#"{"op":"paint_blend","layer":1,"ink":[0.47,0.36,0.3,0.22,1],"opacity":0.9}"#,
        &stroke,
    )
    .unwrap();
    (painted, vec![base, added])
}

#[test]
fn the_wand_compares_the_inks_naive_rgb_over_the_composite_and_a_layer() {
    let dir = TempDir::new().unwrap();
    let (w, h) = (17u32, 13u32);
    for bits in [8u16, 16] {
        let (doc, owned) = layered(&dir, w, h, bits);
        let composite = composite_ink(doc, w, h);
        let layer = placed_layer(doc, 1, w, h);
        let mut seen_partial = false;
        for (source, pixels) in [("composite", &composite), ("layer", &layer)] {
            for tolerance in [0u16, 10 * 257, 32 * 257] {
                for contiguous in [true, false] {
                    for seed in [(0u32, 0u32), (12, 9), (4, 6)] {
                        let mut request = json!({"op":"selection","source":source,
                            "operation":wand(seed.0, seed.1, tolerance, contiguous)});
                        if source == "layer" {
                            request["layer"] = json!(1);
                        }
                        let got = select(doc, &request, &[]).unwrap();
                        let want = oracle(w, h, pixels, seed, tolerance, contiguous, bits);
                        assert_eq!(
                            got, want,
                            "{bits} bits {source} t {tolerance} contiguous {contiguous} seed {seed:?}"
                        );
                        let count = got.iter().filter(|v| **v != 0).count();
                        seen_partial |= count > 1 && count < got.len();
                    }
                }
            }
        }
        assert!(
            seen_partial,
            "the field exercises partial regions at {bits} bits"
        );
        free(doc);
        for d in owned {
            free(d);
        }
    }
}

#[test]
fn a_k_pair_and_a_gcr_twin_are_admitted_where_their_rgb_is() {
    // Every K pair and GCR twin the 11-projection ink box refused is admitted
    // (measured against Photoshop, take1): a pair that trades C for K at the
    // same naive RGB.
    let dir = TempDir::new().unwrap();
    let base = [110u16, 90, 80, 60].map(|v| v * 257);
    let twin = [102u16, 82, 72, 68].map(|v| v * 257);
    let far = [0u16, 255, 255, 0].map(|v| v * 257);
    let pixels: Vec<[u16; 4]> = (0..12).map(|i| [base, twin, far][i % 3]).collect();
    let doc = cmyk_doc_at(&dir, "twin.tif", 3, 4, &pixels, 8);
    let ink = composite_ink(doc, 3, 4);
    let t = 10 * 257;
    let got = select(
        doc,
        &json!({"op":"selection","operation":wand(0, 0, t, false)}),
        &[],
    )
    .unwrap();
    assert_eq!(got, oracle(3, 4, &ink, (0, 0), t, false, 8));
    assert!(
        got[1] != 0 && got[2] == 0,
        "the twin is admitted, the far ink not"
    );
    free(doc);
}

/// One recorded take (`manifest.json` or `confirm.json`): every case's
/// selected regions against Doctored's at 8 bits, the residual equal to the
/// recorded one; answers the case count.
fn check_take(manifest: &Value) -> usize {
    assert_eq!(manifest["result"], "VALID");
    assert_eq!(manifest["wand_member"], "PN7");
    let (w, h) = (
        manifest["width"].as_u64().unwrap() as u32,
        manifest["height"].as_u64().unwrap() as u32,
    );
    let seed = (
        manifest["seed"][0].as_u64().unwrap() as u32,
        manifest["seed"][1].as_u64().unwrap() as u32,
    );
    let dir = TempDir::new().unwrap();
    let mut docs = std::collections::BTreeMap::new();
    for (name, sheet) in manifest["sheets"].as_object().unwrap() {
        let mut pixels = vec![[0u16; 4]; (w * h) as usize];
        for region in sheet["regions"].as_array().unwrap() {
            let rect: Vec<u32> = region["rect"]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_u64().unwrap() as u32)
                .collect();
            let ink: Vec<u16> = region["ink"]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_u64().unwrap() as u16 * 257)
                .collect();
            for y in rect[1]..rect[1] + rect[3] {
                for x in rect[0]..rect[0] + rect[2] {
                    pixels[(y * w + x) as usize] = [ink[0], ink[1], ink[2], ink[3]];
                }
            }
        }
        // The takes' sheets are 8-bit files, as Photoshop read them.
        docs.insert(
            name.clone(),
            (
                cmyk_doc_at(&dir, &format!("{name}.tif"), w, h, &pixels, 8),
                sheet.clone(),
            ),
        );
    }
    let mut cases = 0;
    for case in manifest["cases"].as_array().unwrap() {
        let (doc, sheet) = &docs[case["sheet"].as_str().unwrap()];
        let t = case["tolerance"].as_u64().unwrap() as u16 * 257;
        let contiguous = case["contiguous"].as_bool().unwrap();
        let got = select(
            *doc,
            &json!({"op":"selection","operation":wand(seed.0, seed.1, t, contiguous)}),
            &[],
        )
        .unwrap();
        let regions = sheet["regions"].as_array().unwrap();
        let mut rasterize = Vec::new();
        for (k, region) in regions.iter().enumerate() {
            let rect: Vec<u32> = region["rect"]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_u64().unwrap() as u32)
                .collect();
            // The take's probe point (`probe_point`): the seed, a barrier ring pixel, a centre.
            let (x, y) = match k {
                0 => seed,
                1 => (rect[0] + 1, rect[1] + 1),
                _ => (rect[0] + rect[2] / 2, rect[1] + rect[3] / 2),
            };
            if got[(y * w + x) as usize] != 0 {
                rasterize.push(k as u64);
            }
        }
        let photoshop: Vec<u64> = case["selected_regions"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_u64().unwrap())
            .collect();
        let mut residual: Vec<u64> = rasterize
            .iter()
            .filter(|k| !photoshop.contains(k))
            .chain(photoshop.iter().filter(|k| !rasterize.contains(k)))
            .copied()
            .collect();
        residual.sort_unstable();
        let recorded: Vec<u64> = case["shipped_residual_regions"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_u64().unwrap())
            .collect();
        assert_eq!(
            residual, recorded,
            "{}: the recorded residual",
            case["name"]
        );
        cases += 1;
    }
    for (_, (doc, _)) in docs {
        free(doc);
    }
    cases
}

#[test]
fn the_photoshop_takes_are_matched_by_pn7_with_their_residuals() {
    let take1: Value =
        serde_json::from_str(include_str!("fixtures/photoshop_cmyk_wand/manifest.json")).unwrap();
    assert_eq!(check_take(&take1), 12);
    let confirm: Value =
        serde_json::from_str(include_str!("fixtures/photoshop_cmyk_wand/confirm.json")).unwrap();
    assert_eq!(
        check_take(&confirm),
        confirm["cases"].as_array().unwrap().len()
    );
    assert!(!confirm["cases"].as_array().unwrap().is_empty());
}

/// The CMYK document's view as an RGB16 document (Display P3, its own profile).
fn view_document(doc: *const PrecisionDocument) -> *mut PrecisionDocument {
    let image: PrecisionImage = unsafe { &*doc }.render(LIMIT, 0, None).unwrap();
    Box::into_raw(Box::new(
        PrecisionDocument::from_image(image, Metadata::default(), Resolution::default()).unwrap(),
    ))
}

#[test]
fn the_colour_kinds_read_the_view_exactly_as_an_rgb16_document_of_it() {
    let dir = TempDir::new().unwrap();
    let (w, h) = (24u32, 18u32);
    let (doc, owned) = layered(&dir, w, h, 16);
    let rgb = view_document(doc);
    let n = (w * h) as usize;
    let coverage: Vec<u16> = (0..n)
        .map(|i| {
            if (i as u32 % w) > 4 && (i as u32 % w) < 16 && (i as u32 / w) > 3 {
                65535
            } else {
                0
            }
        })
        .collect();
    let mut area = coverage.clone();
    area.extend((0..n).map(|i| [0u16, 128, 255][i % 3]));
    let cases: [(Value, &[u16]); 6] = [
        (
            json!({"kind":"color_range","params":{"mode":"sampled","samples":[[30000,26000,24000]],"tolerance":25,"softness":10}}),
            &[],
        ),
        (
            json!({"kind":"color_range","params":{"mode":"tonal","lower":20,"upper":60,"softness":15}}),
            &[],
        ),
        (
            json!({"kind":"quick","params":{"seeds":[[3,3],[20,12]],"tolerance":20,"radius":2}}),
            &[],
        ),
        (
            json!({"kind":"refine_edge","edge_radius":4,"smart_radius":true}),
            &coverage,
        ),
        (
            json!({"kind":"refine_edge","edge_radius":3,"area":true}),
            &area,
        ),
        (json!({"kind":"magic_wand","x":0,"y":0,"tolerance":0}), &[]),
    ];
    for (operation, input) in cases.iter().take(5) {
        let request = json!({"op":"selection","operation":operation});
        let got = select(doc, &request, input).unwrap();
        let want = select(rgb, &request, input).unwrap();
        assert_eq!(got, want, "{operation}");
    }
    // The wand alone reads ink, so it may part from the view's RGB wand; it
    // must still answer.
    assert!(select(doc, &json!({"op":"selection","operation":cases[5].0}), &[]).is_ok());
    free(rgb);
    free(doc);
    for d in owned {
        free(d);
    }
}

#[test]
fn every_kind_and_source_a_cmyk_document_cannot_read_refuses_by_name() {
    // The ink kinds, luminance and the backdrop source read a CMYK document
    // since the channels batch (`cmyk_select_kinds.rs`); a view kind still
    // refuses a layer source by name.
    let dir = TempDir::new().unwrap();
    let doc = cmyk_doc(&dir, "refuse.tif", 4, 3, &field(4, 3));
    assert_eq!(
        select(
            doc,
            &json!({"op":"selection","source":"layer","layer":0,
            "operation":{"kind":"quick","params":{"seeds":[[0,0]]}}}),
            &[]
        )
        .unwrap_err(),
        "A layer source for the \"quick\" selection is unavailable in a CMYK document"
    );
    free(doc);
}

fn cutout(
    doc: *const PrecisionDocument,
    matte: &[u16],
    decontaminate: f32,
) -> Result<*mut PrecisionDocument, String> {
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_add_cmyk_cutout(
            doc,
            matte.as_ptr(),
            decontaminate,
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

fn read_mask(doc: *const PrecisionDocument, index: usize, n: usize) -> Vec<u16> {
    let request = CString::new(format!(
        r#"{{"op":"read_plane","target":"mask","index":{index}}}"#
    ))
    .unwrap();
    let mut out = vec![0u16; n];
    let mut err = ptr::null_mut();
    assert!(
        unsafe {
            dr_precision_doc_read_plane(
                doc,
                request.as_ptr(),
                out.as_mut_ptr(),
                n,
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

/// A soft matte: a ramp across the canvas with full and empty columns.
fn matte(w: u32, h: u32) -> Vec<u16> {
    (0..w * h)
        .map(|i| {
            let x = i % w;
            if x < 3 {
                0
            } else if x >= w - 3 {
                65535
            } else {
                ((x * 65535 / w + (i / w) * 911) % 65536) as u16
            }
        })
        .collect()
}

#[test]
fn new_layer_puts_the_composites_ink_under_the_matte_and_hides_the_rest() {
    let dir = TempDir::new().unwrap();
    let (w, h) = (20u32, 14u32);
    let n = (w * h) as usize;
    let (doc, owned) = layered(&dir, w, h, 16);
    let composite = composite_ink(doc, w, h);
    let soft = matte(w, h);
    let mut err = ptr::null_mut();
    let text = unsafe { dr_precision_doc_cmyk_cutout_plan(doc, 0.0, &mut err) };
    assert!(!text.is_null(), "{}", take_err_string(err));
    let planned: Value =
        serde_json::from_str(unsafe { CStr::from_ptr(text) }.to_str().unwrap()).unwrap();
    unsafe { dr_string_free(text) };
    assert_eq!(planned["foreground_storage"], "cmyka16_unorm");
    assert_eq!(planned["new_layer_index"], 2);
    let out = cutout(doc, &soft, 0.0).unwrap();
    let result = unsafe { &*out };
    assert_eq!(result.layer_count(), 3);
    assert_eq!(result.layer(2).unwrap().name(), "Refined Cutout");
    assert!(result.layer(2).unwrap().visible());
    assert!(!result.layer(0).unwrap().visible() && !result.layer(1).unwrap().visible());
    assert_eq!(
        result.cmyk_layer_samples(2).unwrap(),
        composite,
        "the composite's ink and alpha"
    );
    assert_eq!(read_mask(out, 2, n), soft, "the mask is the matte");
    // An empty matte is no cutout.
    assert!(cutout(doc, &vec![0u16; n], 0.0).unwrap().is_null());
    assert_eq!(
        cutout(doc, &soft, 1.5).unwrap_err(),
        "decontaminate must be 0 (off) or a strength in (0, 1]"
    );
    free(out);
    free(doc);
    for d in owned {
        free(d);
    }
}

/// The RGB16 encoded estimate of `pixels` (opaque RGB16 codes) under `matte`.
fn rgb_estimate(
    w: u32,
    h: u32,
    pixels: Vec<[u16; 4]>,
    matte: &[u16],
    strength: f64,
) -> Vec<[u16; 4]> {
    let image =
        PrecisionImage::from_rgba16(w, h, pixels.clone(), IccProfile::srgb(), None).unwrap();
    let doc =
        PrecisionDocument::from_image(image.clone(), Metadata::default(), Resolution::default())
            .unwrap();
    let params = CString::new(format!(
        r#"{{"color_space":"encoded","strength":{strength}}}"#
    ))
    .unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_estimate_foreground(
            &doc,
            &image,
            matte.as_ptr(),
            matte.len(),
            params.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    if out.is_null() {
        assert!(err.is_null(), "{}", take_err_string(err));
        return pixels;
    }
    let estimated = unsafe { Box::from_raw(out) };
    estimated.to_rgba16().unwrap()
}

#[test]
fn decontaminate_colors_is_the_rgb_estimate_run_on_cmy_and_on_k() {
    let dir = TempDir::new().unwrap();
    let (w, h) = (22u32, 16u32);
    let pixels: Vec<[u16; 4]> = (0..w * h)
        .map(|i| {
            if i % w < w / 2 {
                [52000, 9000, 14000, 30000]
            } else {
                [6000, 41000, 47000, 2000 + (i % 7) as u16 * 900]
            }
        })
        .collect();
    let doc = cmyk_doc(&dir, "decon.tif", w, h, &pixels);
    let ink = composite_ink(doc, w, h);
    let soft = matte(w, h);
    for strength in [1.0f32, 0.6] {
        let out = cutout(doc, &soft, strength).unwrap();
        let got = unsafe { &*out }.cmyk_layer_samples(1).unwrap();
        let cmy = rgb_estimate(
            w,
            h,
            ink.iter().map(|p| [p[0], p[1], p[2], 65535]).collect(),
            &soft,
            f64::from(strength),
        );
        let k = rgb_estimate(
            w,
            h,
            ink.iter().map(|p| [p[3], p[3], p[3], 65535]).collect(),
            &soft,
            f64::from(strength),
        );
        for i in 0..got.len() {
            let want = [cmy[i][0], cmy[i][1], cmy[i][2], k[i][0], ink[i][4]];
            assert_eq!(got[i], want, "pixel {i} at strength {strength}");
            if soft[i] == 0 || soft[i] == 65535 {
                assert_eq!(got[i], ink[i], "pixel {i}: ink at a binary matte is exact");
            }
        }
        assert_ne!(got, ink, "the estimate moved the fringe");
        free(out);
    }
    // A constant fractional matte cannot separate two inks.
    assert_eq!(
        cutout(doc, &vec![30000u16; (w * h) as usize], 1.0).unwrap_err(),
        "constant fractional alpha cannot identify foreground and background colors"
    );
    free(doc);
}
