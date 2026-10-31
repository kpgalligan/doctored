//! Load Selection from a colour plane, the luminosity plane or a
//! layer's transparency on a 32-bit float document. Each f32 sample becomes
//! u16 coverage by clamping the scene-linear value to 0…1 (Photoshop 2025's
//! measured mapping, `designs/phase-11f-hdr-float-status.md`), whatever the
//! document's tone map; the luminosity plane is the document profile's Y row
//! over the UNclamped samples, clamped only as a sum (measured on sRGB
//! and ROMM). Magic Wand, Color Range, Select Similar, Quick Selection and
//! Select and Mask's edge stage plan as well (their coverage is
//! `float_smart_selection.rs`'s).
//!
//! Oracles are independent of the implementation: every expected code is
//! computed HERE in f64 from the authored samples (`round(65535 · clamp(v,
//! 0, 1))`; luminosity `round(65535 · clamp(Σ yᵢ·vᵢ, 0, 1))` with the Y row
//! read from the profile's own rXYZ/gXYZ/bXYZ tags by this file's
//! s15Fixed16 reader). Float documents are authored as float TIFFs and
//! opened with `open_preserving`, so a raw leaf holds exactly the samples the
//! oracle reads.

use std::ffi::{CStr, CString};
use std::ptr;

use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_float::dr_float_operators;
use doctored_core::ffi_precision_edit::{dr_precision_doc_edit_plan, dr_precision_doc_select};
use doctored_core::float_document::ToneMap;
use doctored_core::icc::IccProfile;
use doctored_core::metadata::Metadata;
use doctored_core::precision_document::PrecisionDocument;
use doctored_core::precision_geometry::GeometryEdit;
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use doctored_core::tiff_float::FloatTiff;
use serde_json::{json, Value};

const LIMIT: u64 = REFERENCE_WORKING_BYTES;
const W: u32 = 11;
const H: u32 = 5;

/// Component values: negatives, 0, (0, 1), 1, above white and far above.
const VALUES: [f32; 11] = [
    -1.0, -0.25, 0.0, 0.01, 0.18, 0.5, 0.75, 1.0, 1.5, 8.0, 100.0,
];
/// Alpha 0, fractional alpha and alpha 1.
const ALPHAS: [f32; 5] = [0.0, 0.5, 0.25, 1.0, 0.7];
/// Values whose code must be exact, with that code.
const ANCHORS: [(f32, u16); 6] = [
    (-0.25, 0),
    (0.0, 0),
    (1.0, 65535),
    (8.0, 65535),
    (100.0, 65535),
    (0.5, 32768),
];

// ------------------------------------------------------------- fixtures --

fn samples(width: u32, height: u32) -> Vec<[f32; 4]> {
    (0..width * height)
        .map(|i| {
            let (x, y) = ((i % width) as usize, (i / width) as usize);
            [
                VALUES[x % 11],
                VALUES[(x + y) % 11],
                VALUES[(x + 2 * y + 3) % 11],
                ALPHAS[(x + y) % 5],
            ]
        })
        .collect()
}

/// A float document holding one raster of `samples`, through a float TIFF
/// opened with `open_preserving`; its raster holds exactly the samples.
fn float_document(width: u32, height: u32, samples: &[[f32; 4]]) -> PrecisionDocument {
    let tiff = FloatTiff {
        width,
        height,
        channels: 4,
        alpha: Some(false),
        samples: samples.iter().flatten().copied().collect(),
        icc: None,
        resolution: None,
        metadata: Metadata::default(),
        non_finite: 0,
    };
    let bytes = doctored_core::tiff_float::encode_to_vec(&tiff, false).expect("float TIFF");
    let directory = tempfile::TempDir::new().unwrap();
    let path = directory.path().join("selection.tiff");
    std::fs::write(&path, &bytes).unwrap();
    let doc = PrecisionDocument::open_preserving(path.to_str().unwrap(), LIMIT, 0, None)
        .expect("the float document opens");
    assert!(doc.is_float());
    let held: Vec<[u32; 4]> = doc
        .layer(0)
        .and_then(|layer| layer.source())
        .expect("one float raster")
        .to_rgba32()
        .unwrap()
        .into_iter()
        .map(|p| p.map(f32::to_bits))
        .collect();
    let wanted: Vec<[u32; 4]> = samples.iter().map(|p| p.map(f32::to_bits)).collect();
    assert_eq!(held, wanted, "the raster holds the TIFF samples");
    doc
}

fn u16_document() -> PrecisionDocument {
    let pixels = (0..W * H)
        .map(|i| [(i * 1000) as u16, (i * 700) as u16, (i * 300) as u16, 65535])
        .collect();
    PrecisionDocument::from_image(
        PrecisionImage::from_rgba16(W, H, pixels, IccProfile::srgb(), None).unwrap(),
        Default::default(),
        Default::default(),
    )
    .unwrap()
}

/// `samples` (`size` at `offset`) placed on a `canvas`-sized transparent plane.
fn placed(
    samples: &[[f32; 4]],
    size: (u32, u32),
    canvas: (u32, u32),
    offset: (i32, i32),
) -> Vec<[f32; 4]> {
    (0..canvas.0 * canvas.1)
        .map(|i| {
            let x = i64::from(i % canvas.0) - i64::from(offset.0);
            let y = i64::from(i / canvas.0) - i64::from(offset.1);
            if x < 0 || y < 0 || x >= i64::from(size.0) || y >= i64::from(size.1) {
                [0.0; 4]
            } else {
                samples[y as usize * size.0 as usize + x as usize]
            }
        })
        .collect()
}

// -------------------------------------------------------------- oracles --

/// `round(65535 · clamp(v, 0, 1))` in f64.
fn code(v: f32) -> u16 {
    (f64::from(v).clamp(0.0, 1.0) * 65535.0).round() as u16
}

fn be32(b: &[u8], at: usize) -> u32 {
    u32::from_be_bytes(b[at..at + 4].try_into().unwrap())
}

/// The Y entries of the profile's rXYZ, gXYZ and bXYZ tags, read here as
/// s15Fixed16 from the ICC bytes.
fn y_row(profile: &IccProfile) -> [f64; 3] {
    let icc: &[u8] = profile.bytes();
    let found = |sig: &[u8; 4]| {
        (0..be32(icc, 128) as usize)
            .map(|n| 132 + n * 12)
            .find(|&entry| &icc[entry..entry + 4] == sig)
            .map(|entry| be32(icc, entry + 4) as usize)
            .unwrap_or_else(|| panic!("no {} tag", String::from_utf8_lossy(sig)))
    };
    [b"rXYZ", b"gXYZ", b"bXYZ"].map(|sig| {
        let at = found(sig);
        assert_eq!(&icc[at..at + 4], b"XYZ ");
        f64::from(be32(icc, at + 12) as i32) / 65536.0
    })
}

/// `round(65535 · clamp(Σ yᵢ·vᵢ, 0, 1))` over the UNclamped components.
fn luma(p: &[f32; 4], y: [f64; 3]) -> u16 {
    let sum = y[0] * f64::from(p[0]) + y[1] * f64::from(p[1]) + y[2] * f64::from(p[2]);
    (sum.clamp(0.0, 1.0) * 65535.0).round() as u16
}

/// The expected coverage of one plane over straight samples, `y` the
/// document profile's Y row.
fn oracle(plane: &str, canvas: &[[f32; 4]], y: [f64; 3]) -> Vec<u16> {
    canvas
        .iter()
        .map(|p| match plane {
            "red" => code(p[0]),
            "green" => code(p[1]),
            "blue" => code(p[2]),
            "alpha" => code(p[3]),
            _ => luma(p, y),
        })
        .collect()
}

fn operation(plane: &str) -> Value {
    match plane {
        "alpha" => json!({"kind":"alpha"}),
        "luminance" => json!({"kind":"luminance"}),
        channel => json!({"kind":"color","channel":channel}),
    }
}

const PLANES: [&str; 5] = ["red", "green", "blue", "luminance", "alpha"];

// ------------------------------------------------------------------ FFI --

fn ffi_error(error: *mut std::ffi::c_char) -> String {
    assert!(!error.is_null());
    let text = unsafe { CStr::from_ptr(error) }
        .to_string_lossy()
        .into_owned();
    unsafe { dr_string_free(error) };
    text
}

fn plan(doc: &PrecisionDocument, request: &Value) -> Result<Value, String> {
    let text = CString::new(request.to_string()).unwrap();
    let mut error = ptr::null_mut();
    let out = unsafe { dr_precision_doc_edit_plan(doc, text.as_ptr(), LIMIT, 0, &mut error) };
    if out.is_null() {
        return Err(ffi_error(error));
    }
    let value = unsafe { CStr::from_ptr(out) }
        .to_string_lossy()
        .into_owned();
    unsafe { dr_string_free(out) };
    Ok(serde_json::from_str(&value).unwrap())
}

fn select(doc: &PrecisionDocument, request: Value) -> Vec<u16> {
    let planned = plan(doc, &request).expect("the selection plans");
    assert_eq!(planned["input_samples"], 0);
    let mut out = vec![999u16; planned["output_samples"].as_u64().unwrap() as usize];
    let text = CString::new(request.to_string()).unwrap();
    let mut error = ptr::null_mut();
    let ok = unsafe {
        dr_precision_doc_select(
            doc,
            text.as_ptr(),
            ptr::null(),
            0,
            out.as_mut_ptr(),
            out.len(),
            LIMIT,
            0,
            ptr::null(),
            &mut error,
        )
    };
    assert!(ok, "{}", ffi_error(error));
    out
}

fn request(source: &str, plane: &str) -> Value {
    let mut value = json!({"op":"selection","source":source,"operation":operation(plane)});
    match source {
        "layer" => value["layer"] = json!(0),
        "backdrop" => value["below"] = json!(1),
        _ => {}
    }
    value
}

/// Every code within 1 of the oracle where `checked`; exact where the
/// authored value (or alpha) is an anchor.
fn assert_plane(
    got: &[u16],
    want: &[u16],
    canvas: &[[f32; 4]],
    plane: &str,
    checked: impl Fn(&[f32; 4]) -> bool,
    context: &str,
) {
    assert_eq!(got.len(), want.len(), "{context}");
    for (i, p) in canvas.iter().enumerate() {
        if !checked(p) {
            continue;
        }
        let (g, w) = (got[i], want[i]);
        assert!(
            g.abs_diff(w) <= 1,
            "{context} {plane} pixel {i} {p:?}: {g} vs {w}"
        );
        let component = match plane {
            "red" => Some(p[0]),
            "green" => Some(p[1]),
            "blue" => Some(p[2]),
            "alpha" => Some(p[3]),
            _ => None,
        };
        if let Some(v) = component {
            for (anchor, exact) in ANCHORS {
                if v == anchor {
                    assert_eq!(g, exact, "{context} {plane} anchor {v} at pixel {i}");
                }
            }
        }
    }
}

// ---------------------------------------------------------------- tests --

#[test]
fn planes_clamp_scene_linear_samples_from_every_source() {
    let authored = samples(W, H);
    // The fixture really holds what the anchors are for.
    for (anchor, _) in ANCHORS {
        assert!(
            authored.iter().any(|p| p[..3].contains(&anchor)),
            "{anchor}"
        );
    }
    assert!(authored.iter().any(|p| p[3] == 0.5));
    assert!(authored.iter().any(|p| p[3] == 0.0));
    let doc = float_document(W, H, &authored);
    let doubled = doc.duplicate_layer(0).unwrap().expect("duplicated");
    let y = y_row(doc.profile());
    for plane in PLANES {
        let want = oracle(plane, &authored, y);
        // The raw leaf: every pixel, alpha 0 included.
        let got = select(&doc, request("layer", plane));
        assert_plane(&got, &want, &authored, plane, |_| true, "layer");
        // The composite and the backdrop below the duplicate render the one
        // raster over transparency: alpha everywhere, colour where visible.
        for (source, from) in [("composite", &doc), ("backdrop", &doubled)] {
            let got = select(from, request(source, plane));
            let visible = |p: &[f32; 4]| plane == "alpha" || p[3] > 0.0;
            assert_plane(&got, &want, &authored, plane, visible, source);
        }
    }
}

#[test]
fn an_offset_raster_places_zeros_off_image() {
    let authored = samples(W, H);
    let doc = float_document(W, H, &authored);
    let edit = GeometryEdit::parse(
        &json!({"op":"canvas_size","width":W + 2,"height":H + 3,"x":-1,"y":1}).to_string(),
    )
    .unwrap();
    let resized = doc
        .geometry(&edit, LIMIT, 0, None)
        .unwrap()
        .expect("resized");
    assert!(resized.is_float());
    let canvas = (W + 2, H + 3);
    let expected = placed(&authored, (W, H), canvas, (-1, 1));
    let y = y_row(resized.profile());
    for plane in PLANES {
        let got = select(&resized, request("layer", plane));
        assert_plane(
            &got,
            &oracle(plane, &expected, y),
            &expected,
            plane,
            |_| true,
            "offset",
        );
        // Off-image is exactly zero: the top row and the three right columns.
        for x in 0..canvas.0 {
            assert_eq!(got[x as usize], 0, "{plane} top row {x}");
        }
        for y in 0..canvas.1 {
            for x in W - 1..canvas.0 {
                assert_eq!(got[(y * canvas.0 + x) as usize], 0, "{plane} ({x}, {y})");
            }
        }
    }
}

#[test]
fn luminance_weights_the_document_profile_y_row_over_unclamped_samples() {
    // Coloured, out-of-range and negative components, where the profile's
    // Y row over unclamped samples, Rec.709 over them and Rec.709 over
    // clamped components all disagree.
    let colours: [[f32; 3]; 8] = [
        [2.0, 0.0, 0.0],
        [0.0, 0.0, 2.0],
        [-1.0, 0.5, 2.0],
        [0.9, 0.1, 0.0],
        [-0.5, 1.0, 3.0],
        [3.0, 0.2, -0.4],
        [0.05, 0.05, 1.5],
        [1.2, 0.1, -0.6],
    ];
    let authored: Vec<[f32; 4]> = (0..W * H)
        .map(|i| {
            let [r, g, b] = colours[i as usize % colours.len()];
            [r, g, b, 1.0]
        })
        .collect();
    // `tiff_float::encode_to_vec` writes no ICC, so the P3 document comes
    // from the shared RZCB fixture, which carries the profile bytes.
    let p3 = IccProfile::display_p3();
    let doc = crate::float_fixture::float_doc(W, H, &p3, std::slice::from_ref(&authored));
    assert_eq!(
        doc.profile().bytes(),
        p3.bytes(),
        "the document keeps Display P3"
    );
    let y = y_row(&p3);
    let srgb = y_row(&IccProfile::srgb());
    assert!(
        (y[0] - srgb[0]).abs() > 0.01,
        "P3's Y row is not sRGB's: {y:?}"
    );
    let want = oracle("luminance", &authored, y);
    for source in ["layer", "composite"] {
        let got = select(&doc, request(source, "luminance"));
        assert_plane(&got, &want, &authored, "luminance", |_| true, source);
    }
    // The oracle really tells the rules apart on these samples.
    let rec709 = |p: &[f32; 4], clamped: bool| {
        let c = |v: f32| {
            let v = f64::from(v);
            if clamped {
                v.clamp(0.0, 1.0)
            } else {
                v
            }
        };
        ((0.2126 * c(p[0]) + 0.7152 * c(p[1]) + 0.0722 * c(p[2])).clamp(0.0, 1.0) * 65535.0).round()
            as u16
    };
    for clamped in [false, true] {
        let apart = authored
            .iter()
            .zip(&want)
            .filter(|(p, w)| rec709(p, clamped).abs_diff(**w) > 256)
            .count();
        assert!(
            apart > W as usize,
            "clamped {clamped}: only {apart} pixels apart"
        );
    }
}

/// The working profile of a receipt row.
#[derive(Clone, Copy, PartialEq)]
enum Profile442 {
    Srgb,
    Romm,
}

/// Photoshop 2025's Load Selection from the RGB composite on 32-bit float
/// documents, read back as 8-bit selection codes: `(profile, rgb, code)`.
/// Take 1's `a` (sRGB) and `b` (ROMM) planes and take 2's rows (the issue
/// body), from `scripts/photoshop-probe/float_selection.py`'s receipt.
const RECEIPT_442: [(Profile442, [f32; 3], u8); 18] = [
    (Profile442::Srgb, [3.0, 0.2, -0.4], 201),
    (Profile442::Srgb, [0.05, 0.05, 1.5], 35),
    (Profile442::Srgb, [-1.0, 0.5, 2.0], 66),
    (Profile442::Srgb, [0.4, 0.4, 4.0], 158),
    (Profile442::Srgb, [0.2, 0.1, 3.0], 76),
    (Profile442::Srgb, [1.2, 0.1, -0.6], 77),
    (Profile442::Romm, [2.0, 0.0, 0.0], 147),
    (Profile442::Romm, [0.0, 0.0, 2.0], 0),
    (Profile442::Romm, [-1.0, 0.5, 2.0], 17),
    (Profile442::Romm, [0.9, 0.1, 0.0], 84),
    (Profile442::Romm, [-0.5, 1.0, 3.0], 145),
    (Profile442::Romm, [3.0, 0.2, -0.4], 255),
    (Profile442::Srgb, [2.0, 0.0, 0.0], 113),
    (Profile442::Srgb, [0.0, 2.0, 0.0], 255),
    (Profile442::Srgb, [0.0, 0.0, 2.0], 31),
    (Profile442::Srgb, [-0.5, 1.0, 1.0], 170),
    (Profile442::Srgb, [0.5, 0.2, 0.1], 66),
    (Profile442::Srgb, [1.5, 0.5, 0.25], 180),
];

/// The receipt's `y_rows`: the Y row Photoshop's own profiles carry.
const Y_SRGB_442: [f64; 3] = [0.2224884033203125, 0.7168731689453125, 0.06060791015625];
const Y_ROMM_442: [f64; 3] = [0.2880401611328125, 0.711883544921875, 9.1552734375e-05];

#[test]
fn luminance_reproduces_photoshops_442_receipts() {
    for (profile, receipt_y) in [
        (Profile442::Srgb, Y_SRGB_442),
        (Profile442::Romm, Y_ROMM_442),
    ] {
        let icc = match profile {
            Profile442::Srgb => IccProfile::srgb(),
            Profile442::Romm => IccProfile::prophoto_rgb(),
        };
        // Our built-in profiles carry the receipt's Y row to within two
        // s15Fixed16 steps. sRGB green sits ON the bound (`icc_builtin.rs`'s
        // 0xB787 = 46983/65536 against the receipt's 46981/65536); the other
        // five entries are exact.
        let y = y_row(&icc);
        for i in 0..3 {
            assert!(
                (y[i] - receipt_y[i]).abs() <= 2.0 / 65536.0,
                "Y row entry {i}: ours {} receipt {}",
                y[i],
                receipt_y[i]
            );
        }
        let rows: Vec<([f32; 3], u8)> = RECEIPT_442
            .iter()
            .filter(|row| row.0 == profile)
            .map(|row| (row.1, row.2))
            .collect();
        let authored: Vec<[f32; 4]> = rows.iter().map(|(v, _)| [v[0], v[1], v[2], 1.0]).collect();
        let doc = crate::float_fixture::float_doc(
            rows.len() as u32,
            1,
            &icc,
            std::slice::from_ref(&authored),
        );
        let got = select(&doc, request("composite", "luminance"));
        assert_eq!(got.len(), rows.len());
        for ((v, reading), c) in rows.iter().zip(&got) {
            let ours = (255.0 * f64::from(*c) / 65535.0 + 0.5).floor() as i64;
            assert!(
                (ours - i64::from(*reading)).abs() <= 1,
                "{v:?}: ours {ours} (u16 {c}), Photoshop {reading}"
            );
            // The receipt's own prediction from its Y row is exact.
            let sum: f64 = (0..3).map(|i| receipt_y[i] * f64::from(v[i])).sum();
            let predicted = (255.0 * sum.clamp(0.0, 1.0) + 0.5).floor() as i64;
            assert_eq!(predicted, i64::from(*reading), "{v:?}: the receipt's rule");
        }
    }
}

#[test]
fn the_tone_map_never_changes_coverage() {
    let authored = samples(W, H);
    let doc = float_document(W, H, &authored);
    assert_eq!(doc.tone_map(), Some(ToneMap::Clip));
    let toned = doc
        .with_tone_map(ToneMap::Reinhard { white: 4.0 })
        .unwrap()
        .expect("a new tone map");
    for plane in PLANES {
        for source in ["layer", "composite"] {
            assert_eq!(
                select(&toned, request(source, plane)),
                select(&doc, request(source, plane)),
                "{source} {plane}"
            );
        }
    }
}

#[test]
fn tolerance_distance_and_edge_kinds_plan() {
    let authored = samples(W, H);
    let float = float_document(W, H, &authored);
    let wide = u16_document();
    // The five compare on the R1 display copy (`float_smart_selection.rs`
    // tests their coverage).
    let kinds = [
        json!({"kind":"magic_wand","x":1,"y":1,"tolerance":10,"contiguous":true}),
        json!({"kind":"color_range","params":{"mode":"sampled","samples":[[30001,30001,30001]],"tolerance":0,"softness":0}}),
        json!({"kind":"similar","tolerance":10,"contiguous":false}),
        json!({"kind":"quick","params":{"seeds":[[1,0]],"tolerance":0,"edge":0}}),
        json!({"kind":"refine_edge","edge_radius":2.0}),
    ];
    for kind in kinds {
        for source in ["composite", "layer"] {
            let mut value = json!({"op":"selection","source":source,"operation":kind});
            if source == "layer" {
                value["layer"] = json!(0);
            }
            assert!(plan(&float, &value).is_ok(), "float {source} {kind}");
            assert!(plan(&wide, &value).is_ok(), "u16 {source} {kind}");
        }
    }
}

#[test]
fn the_operator_table_admits_plane_selection() {
    let raw = unsafe { dr_float_operators() };
    assert!(!raw.is_null());
    let text = unsafe { CStr::from_ptr(raw) }
        .to_string_lossy()
        .into_owned();
    unsafe { dr_string_free(raw) };
    let table: Value = serde_json::from_str(&text).unwrap();
    assert!(table["admitted"]
        .as_array()
        .unwrap()
        .contains(&json!("source_plane_selection")));
    let refused = table["refused"].as_array().unwrap();
    assert!(!refused.iter().any(|row| row["operator"]
        .as_str()
        .unwrap()
        .starts_with("Select and Mask's edge stage")));
    for admitted in [
        "magic_wand",
        "color_range",
        "select_similar",
        "quick_selection",
    ] {
        assert!(table["admitted"]
            .as_array()
            .unwrap()
            .contains(&json!(admitted)));
    }
    assert!(!refused.iter().any(|row| row["operator"]
        .as_str()
        .unwrap()
        .contains("scoped statistics")));
}
