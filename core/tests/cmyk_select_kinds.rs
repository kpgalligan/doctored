//! The channels batch's selection kinds on a CMYK document (#547) through
//! `dr_precision_doc_select`, each against an oracle written here from the
//! module docs, never from the implementation: an ink plate loads its stored
//! domain `65535 − ink` (Load Selection from a plate) over the composite and
//! one layer at 8 and 16 bits; `alpha` is the ink's alpha; `luminance` is the
//! shipped member LP — the Rec. 709 luma of the composite's sRGB (the press
//! profile's own tags through `cmyk_fixture::ink_to_srgb`, relative with BPC);
//! `similar` (Select ▸ Grow / Similar) is PN7 over the selection's references,
//! by an independent projection-and-BFS oracle, contiguous on and off; a
//! backdrop wand equals the wand on a document holding only the records below
//! (groups kept); the take's recorded plate codes
//! (`fixtures/photoshop_cmyk_loads/manifest.json`,
//! `scripts/photoshop-probe/cmyk_channels_take.py` take1) are reproduced
//! exactly, its verdicts pinned; and every refusal by name.

use std::ffi::{CStr, CString};
use std::ptr;

use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_cmyk::dr_precision_doc_cmyk_sample;
use doctored_core::ffi_precision::{dr_precision_doc_descriptor, dr_precision_doc_open};
use doctored_core::ffi_precision_edit::{dr_precision_doc_edit_plan, dr_precision_doc_select};
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer};
use doctored_core::precision_image::PrecisionImage;
use serde_json::{json, Value};
use tempfile::TempDir;

use crate::cmyk_fixture::{ink_to_srgb, press_profile};
use crate::cmyk_structure_fixture::{cmyk_layers, free, then, varied, RgbLayer};
use crate::cmyk_tiff_builder::cmyk_tiff;
use crate::common::{cpath, take_err_string};

const LIMIT: u64 = 2 * 1024 * 1024 * 1024;

type Ink = [u16; 5];

/// An opaque CMYK document of `samples` at `bits` in the press profile.
fn cmyk_doc(
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
    let icc = press_profile("Press Kinds");
    std::fs::write(&path, cmyk_tiff(w, h, bits, 4, 0, &data, Some(&icc))).unwrap();
    let c = cpath(&path);
    let mut err = ptr::null_mut();
    let doc = unsafe { dr_precision_doc_open(c.as_ptr(), LIMIT, 0, ptr::null(), &mut err) };
    assert!(!doc.is_null(), "open: {}", take_err_string(err));
    doc
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

fn kind(operation: Value) -> Value {
    json!({"op":"selection","operation":operation})
}

/// The composite's straight ink at every pixel (`dr_precision_doc_cmyk_sample`).
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

/// Record `index`'s own ink on the canvas at its offset, paper elsewhere
/// and wherever it is fully transparent (`cmyk_select`'s module doc).
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
                let p = samples[(y * i64::from(lw) + x) as usize];
                if p[4] == 0 {
                    [0; 5]
                } else {
                    p
                }
            } else {
                [0; 5]
            }
        })
        .collect()
}

/// A varied opaque ink field.
fn field(w: u32, h: u32) -> Vec<[u16; 4]> {
    (0..w * h)
        .map(|i| {
            let j = i.wrapping_mul(2_654_435_761);
            [
                (j % 65536) as u16,
                (j.rotate_left(9) % 65536) as u16,
                (j.rotate_left(17) % 65536) as u16,
                (j.rotate_left(25) % 65536) as u16,
            ]
        })
        .collect()
}

/// Three layers with every alpha class at offsets on a 13 × 9 canvas.
fn layered() -> *mut PrecisionDocument {
    cmyk_layers(
        13,
        9,
        vec![
            RgbLayer {
                name: "Bottom",
                w: 13,
                h: 9,
                offset: (0, 0),
                pixels: varied(13, 9, 3),
            },
            RgbLayer {
                name: "Middle",
                w: 7,
                h: 5,
                offset: (3, 2),
                pixels: varied(7, 5, 8),
            },
            RgbLayer {
                name: "Top",
                w: 6,
                h: 6,
                offset: (6, 1),
                pixels: varied(6, 6, 12),
            },
        ],
    )
}

const PLATES: [&str; 4] = ["cyan", "magenta", "yellow", "black"];

#[test]
fn a_plate_loads_its_stored_domain_over_the_composite_and_a_layer() {
    let dir = TempDir::new().unwrap();
    for bits in [8u16, 16] {
        let doc = cmyk_doc(
            &dir,
            &format!("plates{bits}.tif"),
            11,
            7,
            &field(11, 7),
            bits,
        );
        let ink = composite_ink(doc, 11, 7);
        for (k, plate) in PLATES.iter().enumerate() {
            let got = select(doc, &kind(json!({"kind":"color","channel":plate})), &[]).unwrap();
            let want: Vec<u16> = ink.iter().map(|p| 65535 - p[k]).collect();
            assert_eq!(got, want, "{bits}-bit {plate}");
            if bits == 8 {
                assert!(
                    got.iter().all(|v| v % 257 == 0),
                    "8-bit {plate} stays on the grid"
                );
            }
        }
        let alpha = select(doc, &kind(json!({"kind":"alpha"})), &[]).unwrap();
        assert!(alpha.iter().all(|v| *v == 65535), "{bits}-bit opaque alpha");
        free(doc);
    }
    let doc = layered();
    let ink = composite_ink(doc, 13, 9);
    for index in 0..3 {
        let own = placed_layer(doc, index, 13, 9);
        for (k, plate) in PLATES.iter().enumerate() {
            let request = json!({"op":"selection","source":"layer","layer":index,
                "operation":{"kind":"color","channel":plate}});
            let want: Vec<u16> = own.iter().map(|p| 65535 - p[k]).collect();
            assert_eq!(
                select(doc, &request, &[]).unwrap(),
                want,
                "layer {index} {plate}"
            );
        }
        let request =
            json!({"op":"selection","source":"layer","layer":index,"operation":{"kind":"alpha"}});
        let want: Vec<u16> = own.iter().map(|p| p[4]).collect();
        assert_eq!(
            select(doc, &request, &[]).unwrap(),
            want,
            "layer {index} alpha"
        );
    }
    let alpha = select(doc, &kind(json!({"kind":"alpha"})), &[]).unwrap();
    assert_eq!(
        alpha,
        ink.iter().map(|p| p[4]).collect::<Vec<_>>(),
        "composite alpha"
    );
    free(doc);
}

/// The luma oracle: Rec. 709 of the encoded sRGB codes (module doc).
fn luma(ink: Ink) -> u16 {
    let rgb = ink_to_srgb(
        [ink[0], ink[1], ink[2], ink[3]].map(|v| f64::from(v) / 65535.0),
        true,
    );
    let c = rgb.map(|v| (v.clamp(0.0, 1.0) * 65535.0).round());
    (0.2126f32 as f64 * c[0] + 0.7152f32 as f64 * c[1] + 0.0722f32 as f64 * c[2]).round() as u16
}

#[test]
fn luminance_is_the_luma_of_the_composites_srgb() {
    let dir = TempDir::new().unwrap();
    for bits in [8u16, 16] {
        let doc = cmyk_doc(&dir, &format!("luma{bits}.tif"), 9, 6, &field(9, 6), bits);
        let ink = composite_ink(doc, 9, 6);
        let got = select(doc, &kind(json!({"kind":"luminance"})), &[]).unwrap();
        for (i, (g, p)) in got.iter().zip(&ink).enumerate() {
            let want = luma(*p);
            assert!(
                g.abs_diff(want) <= 2,
                "{bits}-bit pixel {i}: {g} vs oracle {want}"
            );
        }
        free(doc);
    }
    // Paper is white.
    let doc = cmyk_doc(&dir, "paper.tif", 2, 2, &[[0; 4]; 4], 16);
    let got = select(doc, &kind(json!({"kind":"luminance"})), &[]).unwrap();
    assert!(got.iter().all(|v| *v >= 65533), "paper {got:?}");
    free(doc);
}

/// PN7's projections (`selection_region`'s module doc): naive RGB in 16-bit
/// codes, then R, G, B, A, R−G, G−B, R−B.
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

/// Select ▸ Grow / Similar's oracle: every ≥ 50 % covered pixel is a
/// reference; admitted inside the references' range ± tolerance; a BFS from
/// every reference (Grow) or every admitted pixel (Similar).
fn similar_oracle(
    w: u32,
    h: u32,
    pixels: &[Ink],
    coverage: &[u16],
    tolerance: u16,
    contiguous: bool,
    bits: u16,
) -> Vec<u16> {
    let refs: Vec<usize> = (0..pixels.len())
        .filter(|&i| coverage[i] >= 32768)
        .collect();
    let mut lo = [i64::MAX; 7];
    let mut hi = [i64::MIN; 7];
    for &i in &refs {
        let p = projections(pixels[i], bits);
        for k in 0..7 {
            lo[k] = lo[k].min(p[k]);
            hi[k] = hi[k].max(p[k]);
        }
    }
    let t = i64::from(tolerance);
    let admit = |i: usize| {
        let p = projections(pixels[i], bits);
        (0..7).all(|k| p[k] >= lo[k] - t && p[k] <= hi[k] + t)
    };
    let mut out = vec![0u16; pixels.len()];
    if !contiguous {
        for (i, v) in out.iter_mut().enumerate() {
            if admit(i) {
                *v = 65535;
            }
        }
        return out;
    }
    let mut queue = std::collections::VecDeque::new();
    for &i in &refs {
        out[i] = 65535;
        queue.push_back(i);
    }
    while let Some(i) = queue.pop_front() {
        let (x, y) = ((i as u32 % w) as i64, (i as u32 / w) as i64);
        for (nx, ny) in [(x - 1, y), (x + 1, y), (x, y - 1), (x, y + 1)] {
            if nx < 0 || ny < 0 || nx >= i64::from(w) || ny >= i64::from(h) {
                continue;
            }
            let j = (ny * i64::from(w) + nx) as usize;
            if out[j] == 0 && admit(j) {
                out[j] = 65535;
                queue.push_back(j);
            }
        }
    }
    out
}

/// Blocks of four inks with a GCR twin of the top-left block's.
fn blocks(w: u32, h: u32) -> Vec<[u16; 4]> {
    let inks = [
        [20000, 15000, 12000, 5000],
        [12000, 7000, 4000, 13000],
        [20500, 15400, 12000, 5000],
        [50000, 3000, 30000, 0],
    ];
    (0..w * h)
        .map(|i| {
            let (x, y) = (i % w, i / w);
            let block = ((x / 4) + 2 * (y / 4)) as usize % 4;
            inks[if x == w - 1 && y == h - 1 { 0 } else { block }]
        })
        .collect()
}

#[test]
fn similar_is_pn7_over_the_selections_references() {
    let dir = TempDir::new().unwrap();
    let (w, h) = (12u32, 8u32);
    for bits in [8u16, 16] {
        let doc = cmyk_doc(
            &dir,
            &format!("similar{bits}.tif"),
            w,
            h,
            &blocks(w, h),
            bits,
        );
        let ink = composite_ink(doc, w, h);
        let coverage: Vec<u16> = (0..w * h)
            .map(|i| {
                if i % w < 2 && i / w < 2 {
                    65535
                } else if i == 2 {
                    40000
                } else {
                    0
                }
            })
            .collect();
        for tolerance in [0u16, 257 * 4, 257 * 40] {
            for contiguous in [true, false] {
                let request =
                    kind(json!({"kind":"similar","tolerance":tolerance,"contiguous":contiguous}));
                let got = select(doc, &request, &coverage).unwrap();
                let want = similar_oracle(w, h, &ink, &coverage, tolerance, contiguous, bits);
                assert_eq!(
                    got, want,
                    "{bits}-bit t {tolerance} contiguous {contiguous}"
                );
            }
        }
        // A selection with no pixel half covered refuses by name.
        let none = vec![10000u16; (w * h) as usize];
        assert_eq!(
            select(
                doc,
                &kind(json!({"kind":"similar","tolerance":0,"contiguous":true})),
                &none
            )
            .unwrap_err(),
            "No pixel is more than half selected, so there is no colour to match."
        );
        free(doc);
    }
}

/// A CMYK document holding only `layers` of [`layered`]'s source pictures.
fn only(layers: &[usize]) -> *mut PrecisionDocument {
    let all = [
        RgbLayer {
            name: "Bottom",
            w: 13,
            h: 9,
            offset: (0, 0),
            pixels: varied(13, 9, 3),
        },
        RgbLayer {
            name: "Middle",
            w: 7,
            h: 5,
            offset: (3, 2),
            pixels: varied(7, 5, 8),
        },
        RgbLayer {
            name: "Top",
            w: 6,
            h: 6,
            offset: (6, 1),
            pixels: varied(6, 6, 12),
        },
    ];
    let picked = all
        .into_iter()
        .enumerate()
        .filter(|(i, _)| layers.contains(i))
        .map(|(_, l)| l)
        .collect();
    cmyk_layers(13, 9, picked)
}

#[test]
fn a_backdrop_reads_only_the_records_below_with_their_groups() {
    let doc = layered();
    let wand = json!({"kind":"magic_wand","x":4,"y":3,"tolerance":2000,"contiguous":false});
    for below in [1usize, 2, 3] {
        let request = json!({"op":"selection","source":"backdrop","below":below,"operation":wand});
        let reference = only(&(0..below).collect::<Vec<_>>());
        assert_eq!(
            select(doc, &request, &[]).unwrap(),
            select(reference, &kind(wand.clone()), &[]).unwrap(),
            "below {below}"
        );
        for k in [0usize, 3] {
            let plate = json!({"op":"selection","source":"backdrop","below":below,
                "operation":{"kind":"color","channel":PLATES[k]}});
            assert_eq!(
                select(doc, &plate, &[]).unwrap(),
                select(
                    reference,
                    &kind(json!({"kind":"color","channel":PLATES[k]})),
                    &[]
                )
                .unwrap(),
                "below {below} plate {k}"
            );
        }
        free(reference);
    }
    // Below 0 is bare paper: every pixel matches the seed's paper.
    let request = json!({"op":"selection","source":"backdrop","below":0,"operation":wand});
    assert!(select(doc, &request, &[])
        .unwrap()
        .iter()
        .all(|v| *v == 65535));
    // A group of Middle and Top (postorder 0, 1, 2, group 3): below 2 keeps
    // the group around Middle alone.
    let grouped = then(
        layered(),
        r#"{"op":"group_layers","layers":[1,2],"name":"G","pass_through":false}"#,
        &[],
    );
    assert_eq!(unsafe { &*grouped }.layer_count(), 4);
    let request = json!({"op":"selection","source":"backdrop","below":2,"operation":wand});
    let reference = only(&[0, 1]);
    assert_eq!(
        select(grouped, &request, &[]).unwrap(),
        select(reference, &kind(wand.clone()), &[]).unwrap(),
        "the group around Middle"
    );
    free(reference);
    free(grouped);
    assert_eq!(
        select(
            doc,
            &json!({"op":"selection","source":"backdrop","below":4,"operation":wand}),
            &[]
        )
        .unwrap_err(),
        "record 4 is out of range"
    );
    free(doc);
}

#[test]
fn the_take_plate_codes_are_reproduced_and_its_verdicts_pinned() {
    let manifest: Value = serde_json::from_str(
        &std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/photoshop_cmyk_loads/manifest.json"
        ))
        .unwrap(),
    )
    .unwrap();
    assert_eq!(manifest["result"], "VALID");
    assert_eq!(manifest["plate_verdict"], "PASS");
    assert_eq!(manifest["composite"]["member"], "LP");
    assert_eq!(manifest["grow"]["verdict"], "INVALID");
    assert_eq!(manifest["plate_move"]["verdict"], "ALLOWED");
    assert_eq!(manifest["plate_move"]["hole_member"], "HG");
    let (w, h) = (
        manifest["width"].as_u64().unwrap() as u32,
        manifest["height"].as_u64().unwrap() as u32,
    );
    let stripe: Vec<u16> = manifest["stripe"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_u64().unwrap() as u16)
        .collect();
    let mut pixels = vec![[0u16; 4]; (w * h) as usize];
    for (i, p) in pixels.iter_mut().enumerate() {
        if i as u32 % w >= u32::from(stripe[0]) {
            *p = [stripe[1], stripe[2], stripe[3], stripe[4]].map(|v| v * 257);
        }
    }
    let regions = manifest["regions"].as_array().unwrap();
    let rect =
        |r: &Value| -> [u32; 4] { std::array::from_fn(|i| r["rect"][i].as_u64().unwrap() as u32) };
    for region in regions {
        let [x0, y0, rw, rh] = rect(region);
        let ink: [u16; 4] =
            std::array::from_fn(|i| region["ink"][i].as_u64().unwrap() as u16 * 257);
        if region["name"] == "paper" {
            continue;
        }
        for y in y0..y0 + rh {
            for x in x0..x0 + rw {
                pixels[(y * w + x) as usize] = ink;
            }
        }
    }
    let dir = TempDir::new().unwrap();
    let doc = cmyk_doc(&dir, "loads.tif", w, h, &pixels, 8);
    for (k, plate) in ["C", "M", "Y", "K"].iter().enumerate() {
        let got = select(doc, &kind(json!({"kind":"color","channel":PLATES[k]})), &[]).unwrap();
        let codes = manifest["plate_codes"][plate].as_array().unwrap();
        for (region, code) in regions.iter().zip(codes) {
            let [x0, y0, rw, rh] = rect(region);
            let at = ((y0 + rh / 2) * w + x0 + rw / 2) as usize;
            assert_eq!(
                u64::from(got[at] / 257),
                code.as_u64().unwrap(),
                "{plate} {}",
                region["name"]
            );
            assert_eq!(got[at] % 257, 0);
        }
    }
    free(doc);
}

#[test]
fn the_new_kinds_refuse_by_name_and_plan() {
    let dir = TempDir::new().unwrap();
    let doc = cmyk_doc(&dir, "refuse.tif", 4, 3, &field(4, 3), 16);
    for channel in ["red", "green", "blue"] {
        assert_eq!(
            select(doc, &kind(json!({"kind":"color","channel":channel})), &[]).unwrap_err(),
            format!("The \"{channel}\" channel selection is unavailable in a CMYK document")
        );
    }
    // A layer source for `luminance` is admitted (Apply Image and
    // Calculations): on a one-layer opaque document it is the composite's.
    assert_eq!(
        select(
            doc,
            &json!({"op":"selection","source":"layer","layer":0,"operation":{"kind":"luminance"}}),
            &[]
        )
        .unwrap(),
        select(doc, &kind(json!({"kind":"luminance"})), &[]).unwrap()
    );
    let mut err = ptr::null_mut();
    let text = unsafe { dr_precision_doc_descriptor(doc, &mut err) };
    assert!(!text.is_null(), "{}", take_err_string(err));
    let descriptor: Value =
        serde_json::from_str(unsafe { CStr::from_ptr(text) }.to_str().unwrap()).unwrap();
    unsafe { dr_string_free(text) };
    for key in [
        "cmyk_plate_load_scratch_bytes",
        "cmyk_luminance_scratch_bytes",
        "cmyk_similar_scratch_bytes",
        "cmyk_backdrop_wand_scratch_bytes",
        "cmyk_move_plate_scratch_bytes",
        "cmyk_transform_plate_scratch_bytes",
    ] {
        assert!(
            descriptor[key].as_u64().is_some_and(|v| v > 0),
            "{key}: {}",
            descriptor[key]
        );
    }
    free(doc);
    // A plate on an RGB document refuses by name.
    let srgb = IccProfile::srgb();
    let image =
        PrecisionImage::from_rgba16(2, 2, vec![[1000, 2000, 3000, 65535]; 4], srgb.clone(), None)
            .unwrap();
    let rgb = Box::into_raw(Box::new(
        PrecisionDocument::from_parts(
            2,
            2,
            srgb,
            Metadata::default(),
            Resolution::default(),
            vec![PrecisionDocumentLayer::raster("L", image, (0, 0), 1.0, true).unwrap()],
        )
        .unwrap(),
    ));
    assert_eq!(
        select(rgb, &kind(json!({"kind":"color","channel":"cyan"})), &[]).unwrap_err(),
        "The cyan channel requires a CMYK document"
    );
    // The RGB luminance stays the Rec. 709 codes of the samples.
    let luma = (f64::from(0.2126f32) * 1000.0
        + f64::from(0.7152f32) * 2000.0
        + f64::from(0.0722f32) * 3000.0)
        .round() as u16;
    assert_eq!(
        select(rgb, &kind(json!({"kind":"luminance"})), &[]).unwrap(),
        vec![luma; 4]
    );
    free(rgb);
}
