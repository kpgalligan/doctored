//! Phase 12E-2c: the typed STAMP VISIBLE and FLATTEN IMAGE ops, reached
//! through `dr_precision_doc_edit` / `dr_precision_doc_edit_plan`.
//!
//! **Tolerances, fixed in this header BEFORE anything was measured:**
//! * 16 bits: **1 code of 65535**, absolute, per channel (`U16_TOLERANCE`);
//! * 32-bit float: **relative 1e-5** (`FLOAT_TOLERANCE`), against the scale
//!   `max(|want|, 1.0)`;
//! * Stamp's leaf against Flatten's leaf: **bit-identical**, no tolerance —
//!   both are the same projection, so an inequality there is a bug rather
//!   than arithmetic.
//!
//! Every pixel claim is checked against an f64 oracle written out in this
//! file (`composite`, the W3C source-over/blend equation, plus the clip-unit
//! and isolated-group rules restated here) over a fixture holding a group, a
//! clipped run, a masked layer, a hidden layer, an adjustment layer and a
//! Multiply layer. The Multiply function itself comes from
//! `tests/common/native_blend_oracle.rs`, which is written from the W3C
//! pseudocode and reads nothing out of `blend.rs`. Structural claims are read
//! back from a postorder walk this file performs; "untouched" is an ADDRESS
//! comparison of the sample and mask allocations; refusals are checked by
//! their SENTENCE, because a host shows that sentence verbatim.

use std::ffi::{c_char, CStr, CString};
use std::ptr;

use doctored_core::doc_evaluation::Evaluation;
use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_comps::dr_precision_doc_layer_comps_json;
use doctored_core::ffi_precision::{
    dr_precision_doc_duplicate_layer, dr_precision_image_storage_identity,
};
use doctored_core::ffi_precision_edit::{dr_precision_doc_edit, dr_precision_doc_edit_plan};
use doctored_core::icc::IccProfile;
use doctored_core::layer_insert::InsertPosition;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_capability::capability;
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer as Layer};
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use doctored_core::precision_mask::{PrecisionMask, PrecisionPlane};
use serde_json::{json, Value};

use crate::native_blend_oracle as blend_oracle;

const LIMIT: u64 = REFERENCE_WORKING_BYTES;
const U16_TOLERANCE: f64 = 1.0;
const FLOAT_TOLERANCE: f64 = 1e-5;
const FLOAT_FIXTURE: &str = "tests/fixtures/float_11f/scene.tiff";
/// `DrBlendMode::Multiply` and `DrBlendMode::Hue`, restated rather than read
/// out of the core.
const MULTIPLY: i32 = 1;
const HUE: i32 = 23;
/// `DrLockFlags`, restated for the same reason.
const LOCK_PIXELS: u32 = 2;
const LOCK_ALL: u32 = 7;

// ------------------------------------------------------- the f64 oracle --

/// `blend::composite_source_into` written out in f64 from the W3C
/// compositing-1 equation: straight source `cs` at source alpha `sa` over the
/// straight backdrop `b`, through the separable blend function `f`.
fn composite(b: [f64; 4], cs: [f64; 3], sa: f64, f: &dyn Fn(f64, f64) -> f64) -> [f64; 4] {
    let ab = b[3];
    let ao = sa + ab * (1.0 - sa);
    if ao <= 0.0 {
        return [0.0; 4];
    }
    let mut out = [0.0; 4];
    for c in 0..3 {
        let blended = f(b[c], cs[c]);
        out[c] = (sa * (1.0 - ab) * cs[c] + sa * ab * blended + (1.0 - sa) * ab * b[c]) / ao;
    }
    out[3] = ao;
    out
}

/// Normal: `B(cb, cs) = cs`.
fn normal(_cb: f64, cs: f64) -> f64 {
    cs
}

/// Multiply, from the independent W3C table.
fn multiply(cb: f64, cs: f64) -> f64 {
    blend_oracle::separable(MULTIPLY, cb, cs)
}

/// One clipped MEMBER of a unit: its straight colour, its source alpha and
/// its blend function.
type Member<'a> = ([f64; 3], f64, &'a dyn Fn(f64, f64) -> f64);

/// One clip UNIT under the clip rule (#578): the base renders into a
/// transparent buffer at full opacity in Normal, its alpha footprint is
/// recorded, each member composites onto the base with that alpha FROZEN
/// (the W3C step against the base colour seen at alpha 1, `lerp(Cb, B(Cb,
/// Cs), sa)`), and the finished buffer composites once at the footprint with
/// the base's mode and opacity.
fn clip_unit(acc: [f64; 4], base: ([f64; 3], f64), members: &[Member<'_>]) -> [f64; 4] {
    let mut buf = composite([0.0; 4], base.0, base.1, &normal);
    let footprint = buf[3];
    for (cs, sa, f) in members {
        buf[3] = 1.0;
        buf = composite(buf, *cs, *sa, *f);
    }
    buf[3] = footprint;
    composite(acc, [buf[0], buf[1], buf[2]], buf[3], &normal)
}

fn rgb(p: [f64; 4]) -> [f64; 3] {
    [p[0], p[1], p[2]]
}

fn unit16(p: [u16; 4]) -> [f64; 4] {
    p.map(|v| f64::from(v) / 65535.0)
}

fn unit32(p: [f32; 4]) -> [f64; 4] {
    p.map(f64::from)
}

fn close16(got: &[[u16; 4]], want: &[[f64; 4]], what: &str) {
    assert_eq!(got.len(), want.len(), "{what}: sample count");
    for (i, (g, w)) in got.iter().zip(want).enumerate() {
        for c in 0..4 {
            let expected = w[c].clamp(0.0, 1.0) * 65535.0;
            let difference = (f64::from(g[c]) - expected).abs();
            assert!(
                difference <= U16_TOLERANCE,
                "{what}: pixel {i} channel {c} is {} against the oracle's {expected} \
                 (difference {difference} codes, tolerance {U16_TOLERANCE})",
                g[c]
            );
        }
    }
}

fn close32(got: &[[f32; 4]], want: &[[f64; 4]], what: &str) {
    assert_eq!(got.len(), want.len(), "{what}: sample count");
    for (i, (g, w)) in got.iter().zip(want).enumerate() {
        for c in 0..4 {
            let difference = (f64::from(g[c]) - w[c]).abs();
            let scale = w[c].abs().max(1.0);
            assert!(
                difference <= FLOAT_TOLERANCE * scale,
                "{what}: pixel {i} channel {c} is {} against the oracle's {} \
                 (relative tolerance {FLOAT_TOLERANCE})",
                g[c],
                w[c]
            );
        }
    }
}

// ------------------------------------------------------------- plumbing --

fn take_error(error: *mut c_char) -> Option<String> {
    if error.is_null() {
        return None;
    }
    let text = unsafe { CStr::from_ptr(error) }
        .to_string_lossy()
        .into_owned();
    unsafe { dr_string_free(error) };
    Some(text)
}

fn edit_with(
    document: &PrecisionDocument,
    request: Value,
    samples: &[u16],
) -> Result<Option<Box<PrecisionDocument>>, String> {
    let text = CString::new(request.to_string()).unwrap();
    let mut error = ptr::null_mut();
    let result = unsafe {
        dr_precision_doc_edit(
            document,
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
            &mut error,
        )
    };
    if let Some(error) = take_error(error) {
        assert!(result.is_null(), "a refusal leaves no document");
        return Err(error);
    }
    Ok((!result.is_null()).then(|| unsafe { Box::from_raw(result) }))
}

fn edit(
    document: &PrecisionDocument,
    request: Value,
) -> Result<Option<Box<PrecisionDocument>>, String> {
    edit_with(document, request, &[])
}

fn must(document: &PrecisionDocument, request: Value) -> Box<PrecisionDocument> {
    match edit(document, request.clone()) {
        Ok(Some(out)) => out,
        Ok(None) => panic!("{request} reported no change"),
        Err(message) => panic!("{request}: {message}"),
    }
}

fn must_with(
    document: &PrecisionDocument,
    request: Value,
    samples: &[u16],
) -> Box<PrecisionDocument> {
    match edit_with(document, request.clone(), samples) {
        Ok(Some(out)) => out,
        Ok(None) => panic!("{request} reported no change"),
        Err(message) => panic!("{request}: {message}"),
    }
}

fn refusal(document: &PrecisionDocument, request: Value) -> String {
    match edit(document, request.clone()) {
        Err(message) => message,
        Ok(_) => panic!("{request} must be refused"),
    }
}

fn plan_result(document: &PrecisionDocument, request: Value) -> Result<Value, String> {
    let text = CString::new(request.to_string()).unwrap();
    let mut error = ptr::null_mut();
    let out = unsafe { dr_precision_doc_edit_plan(document, text.as_ptr(), LIMIT, 0, &mut error) };
    if let Some(error) = take_error(error) {
        return Err(error);
    }
    assert!(!out.is_null(), "a plan with no error");
    let value = unsafe { CStr::from_ptr(out) }
        .to_string_lossy()
        .into_owned();
    unsafe { dr_string_free(out) };
    Ok(serde_json::from_str(&value).expect("plan JSON"))
}

fn plan(document: &PrecisionDocument, request: Value) -> Value {
    match plan_result(document, request.clone()) {
        Ok(value) => value,
        Err(message) => panic!("{request}: {message}"),
    }
}

fn info(document: &PrecisionDocument) -> Vec<Value> {
    let value: Value =
        serde_json::from_str(&document.info().expect("info").to_string()).expect("info JSON");
    value["layers"].as_array().expect("layer rows").clone()
}

fn comps_of(document: &PrecisionDocument) -> Value {
    let mut error = ptr::null_mut();
    let text = unsafe { dr_precision_doc_layer_comps_json(document, &mut error) };
    assert!(!text.is_null(), "{:?}", take_error(error));
    let value = unsafe { CStr::from_ptr(text) }
        .to_string_lossy()
        .into_owned();
    unsafe { dr_string_free(text) };
    serde_json::from_str(&value).expect("comps JSON")
}

// -------------------------------------------- the independent structure --

#[derive(Debug, PartialEq, Eq, Clone)]
struct Row {
    name: String,
    level: usize,
    clipped: bool,
}

/// The document's records in POSTORDER — children first, then their group,
/// the numbering `PrecisionDocument::layer` indexes — walked here rather than
/// read off any core report.
fn table(document: &PrecisionDocument) -> Vec<Row> {
    fn walk(layers: &[Layer], level: usize, out: &mut Vec<Row>) {
        for layer in layers {
            if let Some(children) = layer.children() {
                walk(children, level + 1, out);
            }
            out.push(Row {
                name: layer.name().to_string(),
                level,
                clipped: layer.clipped(),
            });
        }
    }
    let mut out = Vec::new();
    walk(document.root_layers(), 0, &mut out);
    out
}

fn expect(document: &PrecisionDocument, want: &[(&str, usize, bool)]) {
    let want: Vec<Row> = want
        .iter()
        .map(|(name, level, clipped)| Row {
            name: (*name).to_string(),
            level: *level,
            clipped: *clipped,
        })
        .collect();
    assert_eq!(table(document), want);
}

fn index_of(document: &PrecisionDocument, name: &str) -> usize {
    table(document)
        .iter()
        .position(|r| r.name == name)
        .unwrap_or_else(|| panic!("{name} is present"))
}

fn ids(document: &PrecisionDocument) -> Vec<u64> {
    (0..document.layer_count())
        .map(|i| document.layer_id(i).expect("an id"))
        .collect()
}

/// The raster's STORAGE IDENTITY, so "the pixels were not touched" is an
/// identity claim rather than a comparison of equal values.
fn pixel_address(document: &PrecisionDocument, name: &str) -> usize {
    let source = document
        .layer_source(index_of(document, name))
        .expect("a raster record");
    unsafe { dr_precision_image_storage_identity(source) as usize }
}

fn mask_address(document: &PrecisionDocument, name: &str) -> usize {
    document
        .layer(index_of(document, name))
        .and_then(|l| l.mask())
        .expect("a masked record")
        .plane()
        .samples()
        .as_ptr() as usize
}

fn samples16(document: &PrecisionDocument, index: usize) -> Vec<[u16; 4]> {
    document
        .layer_source(index)
        .expect("a raster record")
        .to_rgba16()
        .expect("u16 storage")
}

fn samples32(document: &PrecisionDocument, index: usize) -> Vec<[f32; 4]> {
    document
        .layer_source(index)
        .expect("a raster record")
        .to_rgba32()
        .expect("f32 storage")
}

fn invert_description() -> String {
    json!({"type":"adjust","op":"invert","params":{},"evaluation":"precision_v1"}).to_string()
}

fn exposure_description(stops: f64) -> String {
    json!({"type":"adjust","evaluation":"precision_v1","op":"exposure",
           "params":{"exposure": stops}})
    .to_string()
}

// ------------------------------------------------------- the u16 fixture --

const W: u32 = 4;
const H: u32 = 1;

/// Per-pixel straight RGBA codes for each raster of the fixture. Written out
/// here so the oracle reads the same numbers the document does.
const BASE: [[u16; 4]; 4] = [
    [8000, 20000, 40000, 65535],
    [11000, 21000, 38000, 65535],
    [14000, 22000, 36000, 65535],
    [17000, 23000, 34000, 65535],
];
const MASKED: [[u16; 4]; 4] = [
    [60000, 30000, 10000, 45000],
    [58000, 31000, 12000, 45000],
    [56000, 32000, 14000, 45000],
    [54000, 33000, 16000, 45000],
];
const MASK: [u16; 4] = [0, 20000, 45000, 65535];
const CLIP_BASE: [[u16; 4]; 4] = [
    [12000, 50000, 22000, 65535],
    [13000, 49000, 23000, 65535],
    [14000, 48000, 24000, 65535],
    [15000, 47000, 25000, 65535],
];
const CLIP_TOP: [[u16; 4]; 4] = [
    [65535, 5000, 33000, 30000],
    [64000, 6000, 32000, 30000],
    [63000, 7000, 31000, 30000],
    [62000, 8000, 30000, 30000],
];
const HIDDEN: [[u16; 4]; 4] = [[65535, 0, 0, 65535]; 4];
const INNER: [[u16; 4]; 4] = [
    [3000, 61000, 17000, 22000],
    [4000, 60000, 18000, 22000],
    [5000, 59000, 19000, 22000],
    [6000, 58000, 20000, 22000],
];
const MULT: [[u16; 4]; 4] = [
    [50000, 26000, 61000, 40000],
    [49000, 27000, 60000, 40000],
    [48000, 28000, 59000, 40000],
    [47000, 29000, 58000, 40000],
];

fn image(pixels: &[[u16; 4]; 4]) -> PrecisionImage {
    PrecisionImage::from_rgba16(W, H, pixels.to_vec(), IccProfile::srgb(), None).unwrap()
}

fn raster(name: &str, pixels: &[[u16; 4]; 4], visible: bool) -> Layer {
    Layer::raster(name, image(pixels), (0, 0), 1.0, visible).unwrap()
}

fn mask_of(values: &[u16]) -> PrecisionMask {
    PrecisionMask::new(
        PrecisionPlane::from_u16(values.len() as u32, 1, values.to_vec(), None).unwrap(),
        Default::default(),
        true,
    )
    .unwrap()
}

/// `[Base, Masked, ClipBase, ClipTop(clipped), Hidden, Group{Inner}, Invert,
/// Mult(Multiply)]` — a group, a clipped run, a masked layer, a hidden layer,
/// an adjustment and a non-Normal blend, which is every construct the
/// projection has to bake.
fn fixture() -> PrecisionDocument {
    let base = PrecisionDocument::from_parts(
        W,
        H,
        IccProfile::srgb(),
        Metadata::default(),
        Resolution::default(),
        vec![
            raster("Base", &BASE, true),
            raster("Masked", &MASKED, true).with_mask(Some(mask_of(&MASK))),
            raster("ClipBase", &CLIP_BASE, true),
            raster("ClipTop", &CLIP_TOP, true).with_clipped(true),
            raster("Hidden", &HIDDEN, false),
            Layer::isolated_normal("Group", vec![raster("Inner", &INNER, true)], 1.0, true)
                .unwrap(),
            raster("Mult", &MULT, true),
        ],
    )
    .unwrap();
    // The adjustment goes in through the shipped positional insert, above the
    // group and below `Mult`.
    let with_adjustment = base
        .add_adjustment_at(
            "Invert",
            &invert_description(),
            1.0,
            index_of(&base, "Group"),
            InsertPosition::Above,
            LIMIT,
            0,
            None,
        )
        .expect("an adjustment insert")
        .expect("an insertion is always a change");
    let out = must(
        &with_adjustment,
        json!({"op": "layer_blend", "version": 1,
               "layer": index_of(&with_adjustment, "Mult"), "blend_mode": MULTIPLY}),
    );
    expect(
        &out,
        &[
            ("Base", 0, false),
            ("Masked", 0, false),
            ("ClipBase", 0, false),
            ("ClipTop", 0, true),
            ("Hidden", 0, false),
            ("Inner", 1, false),
            ("Group", 0, false),
            ("Invert", 0, false),
            ("Mult", 0, false),
        ],
    );
    *out
}

/// The fixture's projection, computed here from the constants above through
/// the f64 equations at the top of the file.
fn u16_oracle() -> Vec<[f64; 4]> {
    (0..4)
        .map(|x| {
            let base = unit16(BASE[x]);
            let masked = unit16(MASKED[x]);
            let clip_base = unit16(CLIP_BASE[x]);
            let clip_top = unit16(CLIP_TOP[x]);
            let inner = unit16(INNER[x]);
            let mult = unit16(MULT[x]);
            let coverage = f64::from(MASK[x]) / 65535.0;

            let mut p = composite([0.0; 4], rgb(base), base[3], &normal);
            p = composite(p, rgb(masked), masked[3] * coverage, &normal);
            // The clipped run: ClipTop is clipped to ClipBase.
            p = clip_unit(
                p,
                (rgb(clip_base), clip_base[3]),
                &[(rgb(clip_top), clip_top[3], &normal)],
            );
            // `Hidden` contributes nothing at all.
            // The isolated Normal group holding one member is that member
            // composited over transparency, then over the backdrop at the
            // group's own opacity and mode.
            let group = composite([0.0; 4], rgb(inner), inner[3], &normal);
            p = composite(p, rgb(group), group[3], &normal);
            // The adjustment: `adjusted = 1 - c` on RGB, alpha untouched,
            // lerped back at `k = opacity * coverage = 1`.
            p = [1.0 - p[0], 1.0 - p[1], 1.0 - p[2], p[3]];
            composite(p, rgb(mult), mult[3], &multiply)
        })
        .collect()
}

// ----------------------------------------------------- the float fixture --

/// The same nine-record shape at 32-bit float, built out of the 11F scene by
/// duplication (there is no float leaf constructor outside the core) with
/// `+1 EV` exposure standing in for the invert the float mode does not admit.
/// `None` when the fixture is absent.
fn float_fixture() -> Option<(PrecisionDocument, Vec<u16>)> {
    if !std::path::Path::new(FLOAT_FIXTURE).exists() {
        return None;
    }
    let scene = PrecisionDocument::open_preserving(FLOAT_FIXTURE, LIMIT, 0, None).ok()?;
    assert!(scene.is_float(), "the fixture is a float document");
    let (width, height) = scene.dimensions();
    let count = (width * height) as usize;

    let mut document = scene;
    for _ in 0..6 {
        let mut error = ptr::null_mut();
        let out = unsafe { dr_precision_doc_duplicate_layer(&document, 0, &mut error) };
        assert!(!out.is_null(), "{:?}", take_error(error));
        document = *unsafe { Box::from_raw(out) };
    }
    assert_eq!(document.layer_count(), 7, "seven float rasters");
    for (index, name) in [
        "Base", "Masked", "ClipBase", "ClipTop", "Hidden", "Inner", "Mult",
    ]
    .iter()
    .enumerate()
    {
        document = *must(
            &document,
            json!({"op": "rename_layer", "layer": index, "name": name}),
        );
    }
    let coverage: Vec<u16> = (0..count)
        .map(|i| ((i as u64 * 65535) / (count as u64 - 1)) as u16)
        .collect();
    document = *must_with(
        &document,
        json!({"op": "set_mask", "layer": 1, "width": width, "height": height,
               "x": 0, "y": 0, "linked": true, "density": 1.0, "feather": 0.0}),
        &coverage,
    );
    document = *must(
        &document,
        json!({"op": "layer_clipped", "layer": 3, "clipped": true}),
    );
    document = *must(
        &document,
        json!({"op": "layer_visible", "layer": 4, "visible": false}),
    );
    // The container is asked for as an isolated Normal group (a float
    // document mints Pass Through only when asked).
    document = *must(
        &document,
        json!({"op": "group_layers", "layers": [5], "name": "Group", "pass_through": false}),
    );
    document = document
        .add_adjustment_at(
            "Exposure",
            &exposure_description(1.0),
            1.0,
            index_of(&document, "Group"),
            InsertPosition::Above,
            LIMIT,
            0,
            None,
        )
        .expect("a float adjustment insert")
        .expect("an insertion is always a change");
    document = *must(
        &document,
        json!({"op": "layer_blend", "version": 1,
               "layer": index_of(&document, "Mult"), "blend_mode": MULTIPLY}),
    );
    expect(
        &document,
        &[
            ("Base", 0, false),
            ("Masked", 0, false),
            ("ClipBase", 0, false),
            ("ClipTop", 0, true),
            ("Hidden", 0, false),
            ("Inner", 1, false),
            ("Group", 0, false),
            ("Exposure", 0, false),
            ("Mult", 0, false),
        ],
    );
    Some((document, coverage))
}

/// The float fixture's projection: every raster carries the SAME scene
/// samples, so the oracle reads them once.
fn float_oracle(document: &PrecisionDocument, coverage: &[u16]) -> Vec<[f64; 4]> {
    let scene = samples32(document, index_of(document, "Base"));
    scene
        .iter()
        .enumerate()
        .map(|(i, sample)| {
            let s = unit32(*sample);
            let m = f64::from(coverage[i]) / 65535.0;
            let mut p = composite([0.0; 4], rgb(s), s[3], &normal);
            p = composite(p, rgb(s), s[3] * m, &normal);
            p = clip_unit(p, (rgb(s), s[3]), &[(rgb(s), s[3], &normal)]);
            let group = composite([0.0; 4], rgb(s), s[3], &normal);
            p = composite(p, rgb(group), group[3], &normal);
            // +1 EV in LINEAR light is exactly a gain of 2 on each component
            // (`adjust_tone::Exposure::apply_linear` with offset 0, gamma 1);
            // the reference-basis round trip is a matrix times its inverse.
            // A float scene's adjustment is skipped at alpha 0, and this
            // fixture's alpha is positive everywhere (asserted below).
            p = [p[0] * 2.0, p[1] * 2.0, p[2] * 2.0, p[3]];
            composite(p, rgb(s), s[3], &multiply)
        })
        .collect()
}

// ------------------------------------------------- (a) the pixel contract --

#[test]
fn a_stamp_and_flatten_hold_the_projection_and_are_the_same_picture() {
    let document = fixture();
    let want = u16_oracle();

    let stamped = must(
        &document,
        json!({"op": "stamp_visible", "name": "Stamp", "anchor": index_of(&document, "Mult")}),
    );
    let leaf = index_of(&stamped, "Stamp");
    close16(&samples16(&stamped, leaf), &want, "the stamped leaf");

    let flattened = must(&document, json!({"op": "flatten"}));
    assert_eq!(flattened.layer_count(), 1, "one record survives a flatten");
    close16(&samples16(&flattened, 0), &want, "the flattened leaf");

    assert_eq!(
        samples16(&stamped, leaf),
        samples16(&flattened, 0),
        "both leaves are the SAME projection, bit for bit"
    );
    // Purity: the input is untouched.
    assert_eq!(document.layer_count(), 9);
}

#[test]
fn a_float_stamp_and_flatten_hold_the_float_projection() {
    let Some((document, coverage)) = float_fixture() else {
        eprintln!("the 11F float fixture is absent; skipping");
        return;
    };
    let base = samples32(&document, index_of(&document, "Base"));
    assert!(
        base.iter().all(|p| p[3] > 0.0),
        "the oracle's adjustment step assumes a positive alpha everywhere"
    );
    let want = float_oracle(&document, &coverage);

    let stamped = must(
        &document,
        json!({"op": "stamp_visible", "name": "Stamp", "anchor": index_of(&document, "Mult")}),
    );
    let leaf = index_of(&stamped, "Stamp");
    assert!(stamped.is_float(), "the result is still a float document");
    close32(&samples32(&stamped, leaf), &want, "the float stamped leaf");

    let flattened = must(&document, json!({"op": "flatten"}));
    assert_eq!(flattened.layer_count(), 1);
    assert!(flattened.is_float());
    close32(&samples32(&flattened, 0), &want, "the float flattened leaf");
    assert_eq!(
        samples32(&stamped, leaf),
        samples32(&flattened, 0),
        "both float leaves are the SAME projection, bit for bit"
    );
}

// --------------------------------------------- (b) the stamp's structure --

#[test]
fn b_a_stamp_leaves_every_existing_record_the_allocation_it_was() {
    let document = fixture();
    let before: Vec<(String, usize, usize)> = [
        "Base", "Masked", "ClipBase", "ClipTop", "Hidden", "Inner", "Mult",
    ]
    .iter()
    .map(|name| ((*name).to_string(), pixel_address(&document, name), 0usize))
    .collect();
    let mask_before = mask_address(&document, "Masked");
    let rows_before = info(&document);

    let stamped = must(
        &document,
        json!({"op": "stamp_visible", "name": "Stamp", "anchor": index_of(&document, "Mult")}),
    );
    for (name, address, _) in &before {
        assert_eq!(
            pixel_address(&stamped, name),
            *address,
            "{name}'s samples are the allocation they were"
        );
    }
    assert_eq!(
        mask_address(&stamped, "Masked"),
        mask_before,
        "and so is the mask plane"
    );

    // Every prior id, label, lock and link travels, and exactly ONE id is
    // minted.
    let old: Vec<u64> = ids(&document);
    let new: Vec<u64> = ids(&stamped);
    assert_eq!(new.len(), old.len() + 1);
    for id in &old {
        assert!(new.contains(id), "id {id} survived");
    }
    let minted: Vec<u64> = new.iter().filter(|id| !old.contains(id)).copied().collect();
    assert_eq!(minted.len(), 1, "exactly one mint");
    let rows_after = info(&stamped);
    let leaf = index_of(&stamped, "Stamp");
    for (i, row) in rows_before.iter().enumerate() {
        let j = if i >= leaf { i + 1 } else { i };
        for key in ["id", "label", "locks", "link", "name"] {
            assert_eq!(row[key], rows_after[j][key], "{key} on record {i}");
        }
    }
    assert_eq!(
        rows_after[leaf]["label"],
        json!(0),
        "the leaf is unlabelled"
    );
    assert_eq!(rows_after[leaf]["locks"], json!(0));
    assert_eq!(rows_after[leaf]["link"], json!(0));
    assert_eq!(rows_after[leaf]["opacity"], json!(1.0));
    assert_eq!(rows_after[leaf]["blend_mode"], json!(0), "Normal");
    assert_eq!(rows_after[leaf]["offset"], json!([0, 0]));
    assert_eq!(rows_after[leaf]["dimensions"], json!([W, H]));
    assert_eq!(rows_after[leaf]["mask"], Value::Null);
}

#[test]
fn b_a_stamp_lands_above_the_anchors_whole_subtree_and_is_never_clipped() {
    let document = fixture();

    // A LEAF anchor at the root level.
    let above_base = must(
        &document,
        json!({"op": "stamp_visible", "name": "S", "anchor": index_of(&document, "Base")}),
    );
    expect(
        &above_base,
        &[
            ("Base", 0, false),
            ("S", 0, false),
            ("Masked", 0, false),
            ("ClipBase", 0, false),
            ("ClipTop", 0, true),
            ("Hidden", 0, false),
            ("Inner", 1, false),
            ("Group", 0, false),
            ("Invert", 0, false),
            ("Mult", 0, false),
        ],
    );

    // A GROUP anchor: above the WHOLE subtree, at the group's own level.
    let above_group = must(
        &document,
        json!({"op": "stamp_visible", "name": "S", "anchor": index_of(&document, "Group")}),
    );
    assert_eq!(
        table(&above_group)[7],
        Row {
            name: "S".into(),
            level: 0,
            clipped: false
        }
    );

    // A NESTED anchor: above the member, INSIDE the group.
    let above_inner = must(
        &document,
        json!({"op": "stamp_visible", "name": "S", "anchor": index_of(&document, "Inner")}),
    );
    assert_eq!(
        table(&above_inner)[6],
        Row {
            name: "S".into(),
            level: 1,
            clipped: false
        }
    );
    assert_eq!(table(&above_inner)[7].name, "Group", "still inside it");

    // The clip run: the slot rule would make an ordinary insertion JOIN the
    // run, and the stamp must not — the byte insert sets only the level.
    let anchor = index_of(&document, "ClipBase");
    let slot = document
        .insert_slot(anchor, InsertPosition::Above)
        .expect("a slot");
    assert!(slot.clipped, "the slot rule itself says JOIN the run");
    let stamped = must(
        &document,
        json!({"op": "stamp_visible", "name": "S", "anchor": anchor}),
    );
    assert_eq!(
        table(&stamped)[3],
        Row {
            name: "S".into(),
            level: 0,
            clipped: false
        },
        "the stamped leaf is UNCLIPPED between the base and the clipped member"
    );
    // …and the ordinary ADD at the same anchor still joins, so this is the
    // stamp's own rule rather than a change to the slot.
    let added = must(
        &document,
        json!({"op": "add_layer", "name": "A", "anchor": anchor, "position": "above"}),
    );
    assert!(table(&added)[3].clipped, "add_layer still joins the run");
}

#[test]
fn b_a_stamp_is_lock_exempt() {
    let document = fixture();
    let anchor = index_of(&document, "Base");
    let locked = must(
        &document,
        json!({"op": "layer_lock", "layer": anchor, "locks": LOCK_ALL}),
    );
    let stamped = must(
        &locked,
        json!({"op": "stamp_visible", "name": "S", "anchor": anchor}),
    );
    assert_eq!(stamped.layer_count(), 10, "a locked anchor does not refuse");

    // …and a locked GROUP anchor is no different: insertion is STRUCTURE.
    let group = index_of(&document, "Group");
    let locked_group = must(
        &document,
        json!({"op": "layer_lock", "layer": group, "locks": LOCK_PIXELS}),
    );
    let stamped = must(
        &locked_group,
        json!({"op": "stamp_visible", "name": "S", "anchor": group}),
    );
    assert_eq!(stamped.layer_count(), 10);
}

// ------------------------------------------- (c) the flatten's structure --

#[test]
fn c_a_flatten_leaves_one_new_background_and_keeps_the_documents_chrome() {
    let document = fixture();
    let channelled = must_with(
        &document,
        json!({"op": "set_channel", "index": null, "name": "Coverage"}),
        &[100, 200, 300, 400],
    );
    let guided = channelled
        .add_guide(doctored_core::doc_guide::GuideOrientation::Vertical, 2.0)
        .expect("a guide")
        .set_ruler_origin(1.0, 0.0)
        .expect("an origin");
    let with_comp = must(
        &guided,
        json!({"op": "layer_comp", "action": "save", "name": "A"}),
    );
    assert_eq!(
        comps_of(&with_comp)["comps"].as_array().map(Vec::len),
        Some(1),
        "the fixture carries a comp"
    );
    let old = ids(&with_comp);

    let flattened = must(&with_comp, json!({"op": "flatten"}));
    assert_eq!(flattened.layer_count(), 1);
    let rows = info(&flattened);
    assert_eq!(rows[0]["name"], "Background", "`from_pixels`' own name");
    assert_eq!(rows[0]["label"], json!(0), "a new picture is unlabelled");
    assert_eq!(rows[0]["locks"], json!(0));
    assert_eq!(rows[0]["link"], json!(0));
    assert_eq!(rows[0]["opacity"], json!(1.0));
    assert_eq!(rows[0]["blend_mode"], json!(0));
    assert_eq!(rows[0]["offset"], json!([0, 0]));
    assert_eq!(rows[0]["mask"], Value::Null);
    let minted = flattened.layer_id(0).expect("an id");
    assert!(
        !old.contains(&minted),
        "the flattened record mints a NEW id"
    );

    // The comps go with the identities they bound.
    assert_eq!(
        comps_of(&flattened)["comps"].as_array().map(Vec::len),
        Some(0),
        "a flatten drops the whole comp list"
    );
    // …and the chrome stays.
    assert_eq!(flattened.channels().len(), 1);
    assert_eq!(flattened.channels()[0].name(), "Coverage");
    assert_eq!(flattened.guides().len(), 1);
    assert_eq!(
        flattened.guides()[0].position,
        with_comp.guides()[0].position
    );
    assert_eq!(flattened.ruler_origin(), (1.0, 0.0));
    assert_eq!(flattened.evaluation(), with_comp.evaluation());
    assert_eq!(flattened.dimensions(), (W, H));
}

#[test]
fn c_a_flatten_answers_a_one_record_document_and_an_all_locked_stack() {
    // A single record still flattens: the picture is a NEW one, so there is
    // no no-op here. Only the host MENU asks for more than one layer.
    let one = PrecisionDocument::from_parts(
        W,
        H,
        IccProfile::srgb(),
        Metadata::default(),
        Resolution::default(),
        vec![raster("Only", &BASE, true)],
    )
    .unwrap();
    let flattened = must(&one, json!({"op": "flatten"}));
    assert_eq!(flattened.layer_count(), 1);
    assert_eq!(info(&flattened)[0]["name"], "Background");
    assert_ne!(
        flattened.layer_id(0).expect("an id"),
        one.layer_id(0).expect("an id"),
        "a new picture, so a new identity"
    );

    // Every record locked, including Lock All: a flatten is lock-EXEMPT.
    let mut document = fixture();
    for index in 0..document.layer_count() {
        document = *must(
            &document,
            json!({"op": "layer_lock", "layer": index, "locks": LOCK_ALL}),
        );
    }
    let flattened = must(&document, json!({"op": "flatten"}));
    assert_eq!(flattened.layer_count(), 1);
    assert_eq!(info(&flattened)[0]["locks"], json!(0));
}

// --------------------------------------------------------- (d) refusals --

#[test]
fn d_the_refusals_are_named() {
    let document = fixture();

    assert_eq!(
        refusal(
            &document,
            json!({"op": "stamp_visible", "name": "S", "anchor": 99})
        ),
        "high-depth layer index is out of range",
    );

    // Nothing contributes at all.
    let mut hidden = fixture();
    for index in 0..hidden.layer_count() {
        if hidden.layer(index).expect("a record").visible() {
            hidden = *must(
                &hidden,
                json!({"op": "layer_visible", "layer": index, "visible": false}),
            );
        }
    }
    assert_eq!(
        refusal(
            &hidden,
            json!({"op": "stamp_visible", "name": "S", "anchor": 0})
        ),
        "high-depth stamp visible requires a contributing raster layer",
    );

    // A VISIBLE ADJUSTMENT alone is not a contributing RASTER — the 12E-1b
    // narrowing, where the byte predicate counts an adjustment layer.
    let pair = PrecisionDocument::from_parts(
        W,
        H,
        IccProfile::srgb(),
        Metadata::default(),
        Resolution::default(),
        vec![raster("Base", &BASE, true)],
    )
    .unwrap()
    .add_adjustment_at(
        "Invert",
        &invert_description(),
        1.0,
        0,
        InsertPosition::Above,
        LIMIT,
        0,
        None,
    )
    .expect("an adjustment insert")
    .expect("a change");
    let adjustment_only = must(
        &pair,
        json!({"op": "layer_visible", "layer": 0, "visible": false}),
    );
    assert!(
        adjustment_only.layer(1).expect("the adjustment").visible(),
        "the adjustment itself still contributes"
    );
    assert_eq!(
        refusal(
            &adjustment_only,
            json!({"op": "stamp_visible", "name": "S", "anchor": 1})
        ),
        "high-depth stamp visible requires a contributing raster layer",
    );

    // The SAME floor at 32-bit FLOAT: hide every record of the float fixture
    // and the stamp refuses by the same name. The drive's C3 gate is this
    // one's MCP twin; skipped, never faked, when the 11F fixture is absent.
    if let Some((float, _)) = float_fixture() {
        let mut hidden = float;
        for index in 0..hidden.layer_count() {
            if hidden.layer(index).expect("a record").visible() {
                hidden = *must(
                    &hidden,
                    json!({"op": "layer_visible", "layer": index, "visible": false}),
                );
            }
        }
        assert_eq!(
            refusal(
                &hidden,
                json!({"op": "stamp_visible", "name": "S", "anchor": 0})
            ),
            "high-depth stamp visible requires a contributing raster layer",
        );
    } else {
        eprintln!("the 11F float fixture is absent; skipping the float refusal");
    }

    // Missing fields, and an UNKNOWN field on each op by name.
    for request in [
        json!({"op": "stamp_visible", "anchor": 0}),
        json!({"op": "stamp_visible", "name": "S"}),
        json!({"op": "stamp_visible", "name": "S", "anchor": 0, "x": 1}),
        json!({"op": "flatten", "x": 1}),
        // The pre-existing Merge Visible variant is an empty STRUCT too since
        // 12E-2c, so it refuses an extra key the same way.
        json!({"op": "merge_visible", "x": 1}),
    ] {
        let message = refusal(&document, request.clone());
        assert!(
            message.starts_with("invalid high-depth layer property: "),
            "{request}: {message}"
        );
    }
    // A plain Merge Visible still works, so the variant change is a
    // tightening and not a break.
    assert_eq!(
        must(&document, json!({"op": "merge_visible"})).layer_count(),
        2,
        "the merged leaf plus the hidden record, which contributed nothing"
    );
}

#[test]
fn d_the_plan_charges_at_least_what_a_merge_visible_does() {
    let document = fixture();
    let merge = plan(&document, json!({"op": "merge_visible"}));
    for request in [
        json!({"op": "stamp_visible", "name": "Stamp", "anchor": 0}),
        json!({"op": "flatten"}),
    ] {
        let value = plan(&document, request.clone());
        let charged = value["scratch_bytes"].as_u64().expect("scratch_bytes");
        let floor = merge["scratch_bytes"].as_u64().expect("scratch_bytes");
        assert!(
            charged >= floor,
            "{request} charges {charged}, below Merge Visible's {floor}"
        );
        assert_eq!(value["input_samples"], json!(0), "neither takes samples");
    }
}

// ------------------------------------------------------ (e) 11E and 11F --

#[test]
fn e_an_unmigrated_linear_light_operator_refuses_both_ops_with_the_walks_sentence() {
    let encoded = PrecisionDocument::from_parts(
        W,
        H,
        IccProfile::srgb(),
        Metadata::default(),
        Resolution::default(),
        vec![raster("Base", &BASE, true), raster("Top", &MULT, true)],
    )
    .unwrap();
    let linear = encoded
        .with_evaluation(Evaluation::LinearLight)
        .expect("a switch")
        .expect("a change");
    assert!(
        linear.linear_light_refusal().is_none(),
        "a migrated linear stack refuses nothing"
    );
    // …and both ops answer on it.
    assert_eq!(
        must(&linear, json!({"op": "flatten"})).evaluation(),
        Evaluation::LinearLight,
        "a flatten keeps the policy"
    );
    assert_eq!(
        must(
            &linear,
            json!({"op": "stamp_visible", "name": "S", "anchor": 0})
        )
        .layer_count(),
        3,
    );

    let broken = must(
        &linear,
        json!({"op": "layer_blend", "version": 1, "layer": 1, "blend_mode": HUE}),
    );
    let sentence = broken
        .linear_light_refusal()
        .expect("the walk names the operator");
    for request in [
        json!({"op": "stamp_visible", "name": "S", "anchor": 0}),
        json!({"op": "flatten"}),
    ] {
        assert_eq!(
            refusal(&broken, request.clone()),
            sentence,
            "{request} answers the walk's own sentence"
        );
        assert_eq!(
            plan_result(&broken, request.clone()).expect_err("the plan refuses"),
            sentence,
        );
    }
}

#[test]
fn e_the_capability_table_admits_both_families_at_both_typed_depths() {
    for operation in ["layer_stamp", "layer_flatten"] {
        for depth in [16u32, 32] {
            let row = capability(operation, "document", depth);
            assert_eq!(
                row["supported"],
                json!(true),
                "{operation} at depth {depth}: {row}"
            );
            let row = capability(operation, "raster", depth);
            assert_eq!(
                row["supported"],
                json!(false),
                "{operation} does not target a raster at depth {depth}"
            );
        }
    }
}

// -------------------------------------------------- (f) the round trips --

fn round_trip(document: &PrecisionDocument, what: &str) {
    let bytes = document.encode_native(LIMIT, 0, None).expect("bytes");
    let reopened = PrecisionDocument::parse_native(&bytes, LIMIT, 0, None).expect("reopen");
    assert_eq!(reopened.layer_count(), document.layer_count(), "{what}");
    let again = reopened.encode_native(LIMIT, 0, None).expect("bytes again");
    assert_eq!(bytes, again, "{what}: reopen → resave is byte-identical");
}

#[test]
fn f_a_stamped_and_a_flattened_document_survive_the_native_round_trip() {
    let document = fixture();
    let stamped = must(
        &document,
        json!({"op": "stamp_visible", "name": "Stamp", "anchor": index_of(&document, "Mult")}),
    );
    round_trip(&stamped, "a stamped u16 document");
    let flattened = must(&document, json!({"op": "flatten"}));
    round_trip(&flattened, "a flattened u16 document");

    let bytes = stamped.encode_native(LIMIT, 0, None).expect("bytes");
    let reopened = PrecisionDocument::parse_native(&bytes, LIMIT, 0, None).expect("reopen");
    assert_eq!(
        samples16(&reopened, index_of(&reopened, "Stamp")),
        samples16(&stamped, index_of(&stamped, "Stamp")),
        "the stamped pixels survive"
    );

    let Some((float, _)) = float_fixture() else {
        eprintln!("the 11F float fixture is absent; skipping the float round trip");
        return;
    };
    let stamped = must(
        &float,
        json!({"op": "stamp_visible", "name": "Stamp", "anchor": index_of(&float, "Mult")}),
    );
    round_trip(&stamped, "a stamped float document");
    let flattened = must(&float, json!({"op": "flatten"}));
    round_trip(&flattened, "a flattened float document");
}
