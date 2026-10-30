//! Move and Free Transform of one colour channel of a 24 MP layer are
//! admitted under their measured plan at 8, 16 and 32 bits. Opaque 6000×4000
//! layers (RZCB ramps at f32, the same ramps clamped to 0…1 as RGBA16 and
//! RGBA8), a selection of rect (300, 200, 5400, 3600) and `"target":"red"`;
//! M moves it by (37, 23) and T turns it 7° about the canvas centre,
//! bilinear, both carrying that rect as `bounds`. One 24 MP document at a
//! time: one `#[test]`, each document dropped before the next is built.
//!
//! Each scratch is at least ⌈1.25 × the largest measured footprint E⌉ in
//! `selected_color_plane`'s module doc (`# Admission`): one case per
//! process, three runs, `MallocLargeCache=0`, a baseline twin.
use std::ffi::{c_char, CStr, CString};
use std::ptr;

use doctored_core::doc::DrDocument;
use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_precision_edit::{dr_precision_doc_edit, dr_precision_doc_edit_plan};
use doctored_core::ffi_retouch_brush::{
    dr_precision_doc_history_brush_plan, dr_precision_doc_smudge_plan,
};
use doctored_core::ffi_selected_pixels::dr_doc_selected_pixels_edit_plan;
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_clipboard::ClipboardPayload;
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer};
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use image::RgbaImage;
use serde_json::{json, Value};

const W: u32 = 6000;
const H: u32 = 4000;
/// ⌈1.25 × max E⌉ per case: M8, M16, M32, T8, T16, T32.
const FLOOR_M8: u64 = 216_187_030;
const FLOOR_M16: u64 = 366_244_080;
const FLOOR_M32: u64 = 920_658_580;
const FLOOR_T8: u64 = 973_865_650;
const FLOOR_T16: u64 = 1_028_219_630;
const FLOOR_T32: u64 = 1_136_845_610;
/// The measurement's limit: T32's plan plus the document's retained bytes
/// passes the 2 GiB test pin, so every plan is read where it was measured.
const MEASURED_LIMIT: u64 = 8_589_934_592;

fn unit(i: u64) -> [f32; 4] {
    let (x, y) = ((i % u64::from(W)) as f32, (i / u64::from(W)) as f32);
    [
        -0.25 + 4.0 * x / W as f32,
        0.02 + 2.0 * y / H as f32,
        ((i * 2654435761) % 1000) as f32 / 500.0,
        1.0,
    ]
}

fn float_doc() -> PrecisionDocument {
    let profile = IccProfile::srgb();
    let mut bytes = b"RZCB".to_vec();
    for word in [3u32, 2, W, H, 0, 0] {
        bytes.extend(word.to_le_bytes());
    }
    bytes.extend(72f32.to_le_bytes());
    bytes.extend(72f32.to_le_bytes());
    bytes.extend((profile.bytes().len() as u32).to_le_bytes());
    bytes.extend(profile.bytes().iter());
    bytes.push(0);
    bytes.extend(0u32.to_le_bytes());
    bytes.extend(4f32.to_le_bytes());
    bytes.extend(0u32.to_le_bytes());
    bytes.reserve(W as usize * H as usize * 16);
    for i in 0..u64::from(W) * u64::from(H) {
        unit(i).iter().for_each(|v| bytes.extend(v.to_le_bytes()));
    }
    let payload = ClipboardPayload::decode(&bytes, REFERENCE_WORKING_BYTES, 0).unwrap();
    drop(bytes);
    let doc =
        PrecisionDocument::from_clipboard(&payload, REFERENCE_WORKING_BYTES, 0, None).unwrap();
    assert!(doc.is_float());
    doc
}

fn doc16() -> PrecisionDocument {
    let px: Vec<[u16; 4]> = (0..u64::from(W) * u64::from(H))
        .map(|i| unit(i).map(|v| (v.clamp(0.0, 1.0) * 65535.0).round() as u16))
        .collect();
    let image = PrecisionImage::from_rgba16(W, H, px, IccProfile::srgb(), None).unwrap();
    let layer = PrecisionDocumentLayer::raster("L", image, (0, 0), 1.0, true).unwrap();
    PrecisionDocument::from_parts(
        W,
        H,
        IccProfile::srgb(),
        Metadata::default(),
        Resolution::default(),
        vec![layer],
    )
    .unwrap()
}

fn doc8() -> DrDocument {
    let raw: Vec<u8> = (0..u64::from(W) * u64::from(H))
        .flat_map(|i| unit(i).map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8))
        .collect();
    DrDocument::from_pixels(RgbaImage::from_raw(W, H, raw).unwrap())
}

fn bounds() -> Value {
    json!([300, 200, 5400, 3600])
}

fn moved() -> String {
    json!({"op":"move_selected","layer":0,"dx":37,"dy":23,"bounds":bounds(),
           "target":"red","swatch":[220, 30, 170]})
    .to_string()
}

fn turned() -> String {
    let t = 7f64.to_radians();
    let (c, s) = (t.cos(), t.sin());
    let (cx, cy) = (f64::from(W) / 2.0, f64::from(H) / 2.0);
    let m = [c, s, -s, c, cx - c * cx + s * cy, cy - s * cx - c * cy];
    json!({"op":"transform_selected","layer":0,"matrix":m,"filter":"bilinear",
           "bounds":bounds(),"target":"red","swatch":[220, 30, 170]})
    .to_string()
}

fn scratch_of(out: *mut c_char, err: *mut c_char) -> u64 {
    if !err.is_null() {
        let message = unsafe { CStr::from_ptr(err) }
            .to_string_lossy()
            .into_owned();
        unsafe { dr_string_free(err) };
        panic!("the plan refuses: {message}");
    }
    assert!(!out.is_null(), "the plan answers");
    let value: Value =
        serde_json::from_str(&unsafe { CStr::from_ptr(out) }.to_string_lossy()).unwrap();
    unsafe { dr_string_free(out) };
    value["scratch_bytes"].as_u64().unwrap()
}

fn scratch8(doc: &DrDocument, text: &str) -> u64 {
    let text = CString::new(text).unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe { dr_doc_selected_pixels_edit_plan(doc, text.as_ptr(), &mut err) };
    scratch_of(out, err)
}

fn scratch(doc: &PrecisionDocument, text: &str) -> u64 {
    let text = CString::new(text).unwrap();
    let mut err = ptr::null_mut();
    let out =
        unsafe { dr_precision_doc_edit_plan(doc, text.as_ptr(), MEASURED_LIMIT, 0, &mut err) };
    scratch_of(out, err)
}

fn check(case: &str, got: u64, floor: u64) {
    println!("{case} scratch {got}");
    assert!(got >= floor, "{case}: {got} < floor {floor}");
}

#[test]
fn a_colour_channel_edit_is_admitted_at_24mp() {
    let d8 = doc8();
    check("M8", scratch8(&d8, &moved()), FLOOR_M8);
    check("T8", scratch8(&d8, &turned()), FLOOR_T8);
    drop(d8);
    let d32 = float_doc();
    check("M32", scratch(&d32, &moved()), FLOOR_M32);
    check("T32", scratch(&d32, &turned()), FLOOR_T32);
    // A Fill of the Red channel of the same float layer, whose plan is
    // the layer fill's figure; ⌈1.25 × the largest measured footprint⌉ in
    // `float_paint`'s module doc (`# Admission`).
    const FLOOR_P32: u64 = 829_440_600;
    let fill = json!({"op":"paint_blend","layer":0,"color":[0.8627451, 0.11764706, 0.6666667, 1.0],
        "color_space":"document","opacity":1.0,"blend_mode":0,"plane":"red"})
    .to_string();
    check("P32", scratch(&d32, &fill), FLOOR_P32);
    // A Brush stroke, an Eraser stroke and a Gradient over the same
    // float layer; ⌈1.25 × the largest measured footprint⌉ of each in
    // `float_paint`'s module doc (`# Brush, Eraser, Gradient`).
    const FLOOR_B32: u64 = 962_478_740;
    const FLOOR_E32: u64 = 962_458_290;
    const FLOOR_G32: u64 = 962_458_260;
    let brush =
        json!({"op":"paint_blend","layer":0,"color":[0.8627451, 0.11764706, 0.6666667, 1.0],
        "color_space":"document","opacity":1.0,"blend_mode":0})
        .to_string();
    check("B32", scratch(&d32, &brush), FLOOR_B32);
    let eraser = json!({"op":"erase_native","layer":0,"opacity":0.5}).to_string();
    check("E32", scratch(&d32, &eraser), FLOOR_E32);
    let ramp = json!({"op":"gradient","layer":0,"start":[0.8627451, 0.11764706, 0.6666667, 1.0],
        "end":[0.1, 0.9, 0.3, 1.0],"color_space":"document","opacity":1.0,"blend_mode":0,
        "from":[0.0, 0.0],"to":[6000.0, 4000.0],"radial":false})
    .to_string();
    check("G32", scratch(&d32, &ramp), FLOOR_G32);
    // A Clone Stamp stroke, a heal, a spot heal and a Content-Aware
    // Fill (in place and onto a new layer) over the same float layer;
    // ⌈1.25 × the largest measured footprint⌉ of each in `float_retouch`'s
    // module doc (`# Admission`), the requests of
    // `examples/float_retouch_admission.rs`.
    const FLOOR_RC32: u64 = 1_924_916_610;
    const FLOOR_RH32: u64 = 1_442_714_620;
    const FLOOR_RS32: u64 = 962_458_290;
    const FLOOR_RF32: u64 = 962_437_780;
    const FLOOR_RN32: u64 = 2_670_655_420;
    let translate = [1.0, 0.0, 0.0, 1.0, 64.0, 0.0];
    let clone = json!({"op":"retouch_clone","layer":0,"matrix":translate}).to_string();
    check("RC32", scratch(&d32, &clone), FLOOR_RC32);
    let heal = json!({"op":"retouch_heal","layer":0,"matrix":translate,
        "x":2500,"y":1500,"width":1000,"height":1000})
    .to_string();
    check("RH32", scratch(&d32, &heal), FLOOR_RH32);
    let spot = json!({"op":"spot_heal","layer":0,
        "x":2500,"y":1500,"width":1000,"height":1000,"ring":0,"seed":7})
    .to_string();
    check("RS32", scratch(&d32, &spot), FLOOR_RS32);
    let fill = json!({"op":"content_aware_fill","layer":0,
        "x":2500,"y":1500,"width":1000,"height":1000,"ring":0,"seed":7});
    check("RF32", scratch(&d32, &fill.to_string()), FLOOR_RF32);
    let mut layered = fill;
    layered["output"] = json!("new_layer");
    layered["sample"] = json!("all");
    check("RN32", scratch(&d32, &layered.to_string()), FLOOR_RN32);
    // Red Eye over the whole canvas, its largest encoded copy;
    // ⌈1.25 × the largest measured footprint⌉ in `float_retouch`'s module doc.
    const FLOOR_RE32: u64 = 1_230_828_420;
    let red_eye = json!({"op":"red_eye","layer":0,"x":0,"y":0,"width":6000,"height":4000,
        "pupil_size":1.0,"darken":0.5})
    .to_string();
    check("RE32", scratch(&d32, &red_eye), FLOOR_RE32);
    // A Blur stroke, a Sharpen stroke, three Smudge strokes (the
    // record's, a size-1000 tip and a thin stroke across a tile row) and a
    // History Brush stroke over the same float layer; ⌈1.25 × the largest
    // measured footprint⌉ of each in `float_retouch_brush`'s module doc
    // (`# Admission`), the calls of
    // `examples/float_retouch_brush_admission.rs`.
    const FLOOR_BB32: u64 = 1_806_296_330;
    const FLOOR_SB32: u64 = 1_803_429_130;
    const FLOOR_SM32: u64 = 56_381_470;
    const FLOOR_SM32L: u64 = 77_721_660;
    const FLOOR_SM32T: u64 = 66_437_150;
    const FLOOR_HB32: u64 = 1_442_755_640;
    let blur = json!({"op":"retouch_brush","layer":0,"kind":"blur","strength":1.0}).to_string();
    check("BB32", scratch(&d32, &blur), FLOOR_BB32);
    let sharpen =
        json!({"op":"retouch_brush","layer":0,"kind":"sharpen","strength":1.0}).to_string();
    check("SB32", scratch(&d32, &sharpen), FLOOR_SB32);
    let tip = CString::new(
        r#"{"version":1,"hardness":1.0,"roundness":1.0,"angle_degrees":0.0,"pixel":false}"#,
    )
    .unwrap();
    let smudge = |dabs: [f64; 8]| {
        let mut err = ptr::null_mut();
        let out = unsafe {
            dr_precision_doc_smudge_plan(
                &d32,
                0,
                tip.as_ptr(),
                dabs.as_ptr(),
                2,
                false,
                false,
                MEASURED_LIMIT,
                0,
                &mut err,
            )
        };
        scratch_of(out, err)
    };
    let record = [1000.0, 1000.0, 200.0, 1.0, 1800.0, 1800.0, 200.0, 1.0];
    check("SM32", smudge(record), FLOOR_SM32);
    let large = [2000.0, 2000.0, 1000.0, 1.0, 2150.0, 2000.0, 1000.0, 1.0];
    check("SM32L", smudge(large), FLOOR_SM32L);
    let thin = [100.0, 250.0, 20.0, 1.0, 5900.0, 250.0, 20.0, 1.0];
    check("SM32T", smudge(thin), FLOOR_SM32T);
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_history_brush_plan(&d32, 0, &d32, 0, false, MEASURED_LIMIT, 0, &mut err)
    };
    check("HB32", scratch_of(out, err), FLOOR_HB32);
    // The Paint Bucket, Dodge and the Sponge over the same float layer,
    // Dodge on a canvas-sized mask (`scalar_edit`), and the Magic Wand, Color
    // Range and Quick Selection over its composite; ⌈1.25 × the largest
    // measured footprint⌉ of each in `float_paint_tone`'s,
    // `float_retouch_brush`'s and `precision_selection`'s module docs, the
    // calls of `examples/float_paint_admission.rs` (`bucket`, `dodge`,
    // `mask_dodge`), `float_retouch_brush_admission.rs` (`sponge`) and
    // `float_selection_admission.rs` (`wand`, `range`, `quick`, and `refine`:
    // Select and Mask's edge stage on R1, rule E).
    const FLOOR_BK32: u64 = 1_022_567_150;
    const FLOOR_DB32: u64 = 962_458_260;
    const FLOOR_MD32: u64 = 120_115_290;
    const FLOOR_SP32: u64 = 1_802_999_080;
    const FLOOR_WD32: u64 = 962_499_250;
    const FLOOR_CR32: u64 = 962_478_770;
    const FLOOR_QS32: u64 = 962_478_770;
    const FLOOR_RF32E: u64 = 962_478_800;
    let bucket = json!({"op":"bucket_fill","layer":0,
        "color":[0.8627451, 0.11764706, 0.6666667, 1.0],"color_space":"document",
        "opacity":1.0,"x":0,"y":0,"tolerance":65535,"contiguous":true})
    .to_string();
    check("BK32", scratch(&d32, &bucket), FLOOR_BK32);
    let dodge =
        json!({"op":"dodge_burn","layer":0,"exposure":0.5,"range":1,"burn":false}).to_string();
    check("DB32", scratch(&d32, &dodge), FLOOR_DB32);
    let sponge =
        json!({"op":"retouch_brush","layer":0,"kind":"saturate","strength":1.0}).to_string();
    check("SP32", scratch(&d32, &sponge), FLOOR_SP32);
    let selection = |operation: Value| {
        json!({"op":"selection","source":"composite","operation":operation}).to_string()
    };
    check(
        "WD32",
        scratch(
            &d32,
            &selection(
                json!({"kind":"magic_wand","x":0,"y":0,"tolerance":65535,"contiguous":true}),
            ),
        ),
        FLOOR_WD32,
    );
    check(
        "CR32",
        scratch(
            &d32,
            &selection(json!({"kind":"color_range","params":{"mode":"sampled",
                "samples":[[32768,32768,32768]],"tolerance":50,"softness":10}})),
        ),
        FLOOR_CR32,
    );
    check(
        "QS32",
        scratch(
            &d32,
            &selection(
                json!({"kind":"quick","params":{"seeds":[[W / 2, H / 2]],"radius":100,
                "smooth":1}}),
            ),
        ),
        FLOOR_QS32,
    );
    check(
        "RF32E",
        scratch(
            &d32,
            &selection(json!({"kind":"refine_edge","edge_radius":8,"smart_radius":true})),
        ),
        FLOOR_RF32E,
    );
    let ramp: Vec<u16> = (0..u64::from(W) * u64::from(H))
        .map(|i| (i % 65536) as u16)
        .collect();
    let set_mask = CString::new(
        json!({"op":"set_mask","layer":0,"width":W,"height":H,"x":0,"y":0,"linked":true})
            .to_string(),
    )
    .unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_edit(
            &d32,
            set_mask.as_ptr(),
            ramp.as_ptr(),
            ramp.len(),
            MEASURED_LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(err.is_null() && !out.is_null(), "the mask lands");
    let masked = *unsafe { Box::from_raw(out) };
    drop(ramp);
    let mask_dodge = json!({"op":"scalar_edit","target":"mask","index":0,"selection":false,
        "operation":{"kind":"dodge_burn","exposure":0.5,"range":1,"burn":false}})
    .to_string();
    check("MD32", scratch(&masked, &mask_dodge), FLOOR_MD32);
    // A filter on one plane of the same float document, `blur`
    // σ 2 and `surface_blur` r 5 t 15 on Red and `blur` σ 2 on the mask;
    // ⌈1.25 × the largest measured footprint⌉ of each in
    // `float_plane_filter`'s module doc (`# Admission`), the cases of
    // `examples/float_plane_filter_admission.rs`. The Red blur plans no more
    // than the whole-layer `blur` σ 2 (`float_filter_apply`'s table).
    const FLOOR_PFR32: u64 = 720_466_460;
    const FLOOR_PFS32: u64 = 1_200_722_820;
    const FLOOR_PFM32: u64 = 780_493_370;
    const LAYER_BLUR32: u64 = 1_026_291_456;
    let plane_filter = |target: &str, op: &str, params: Value| {
        json!({"op":"scalar_edit","target":target,"index":0,"selection":false,
            "operation":{"kind":"filter","edit":{"layer":0,"op":op,"params":params}}})
        .to_string()
    };
    let red_blur = scratch(&masked, &plane_filter("red", "blur", json!({"sigma": 2})));
    check("PFR32", red_blur, FLOOR_PFR32);
    assert!(
        red_blur <= LAYER_BLUR32,
        "PFR32: {red_blur} > {LAYER_BLUR32}"
    );
    check(
        "PFS32",
        scratch(
            &masked,
            &plane_filter("red", "surface_blur", json!({"radius": 5, "threshold": 15})),
        ),
        FLOOR_PFS32,
    );
    check(
        "PFM32",
        scratch(&masked, &plane_filter("mask", "blur", json!({"sigma": 2}))),
        FLOOR_PFM32,
    );
    drop(masked);
    drop(d32);
    let d16 = doc16();
    check("M16", scratch(&d16, &moved()), FLOOR_M16);
    check("T16", scratch(&d16, &turned()), FLOOR_T16);
    // The 16-bit move runs under the 2 GiB test pin.
    let coverage: Vec<u16> = (0..W * H)
        .map(|i| {
            let (x, y) = (i % W, i / W);
            if (300..5700).contains(&x) && (200..3800).contains(&y) {
                u16::MAX
            } else {
                0
            }
        })
        .collect();
    let text = CString::new(moved()).unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_edit(
            &d16,
            text.as_ptr(),
            coverage.as_ptr(),
            coverage.len(),
            REFERENCE_WORKING_BYTES,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(err.is_null(), "the move is admitted");
    assert!(!out.is_null(), "the move changes the red channel");
    drop(unsafe { Box::from_raw(out) });
}
