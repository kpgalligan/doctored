//! Phase 12E-2: POSITIONAL insertion at HIGH DEPTH — `precision_insert`'s
//! `insert_slot` / `inserting_record` / `add_adjustment_at` and the
//! `dr_precision_doc_add_adjustment_at` export, at u16 AND at 32-bit float.
//!
//! The oracles are written here, never read off the implementation: the
//! expected structure table after each of the three positions, `add_adjustment`
//! itself as the oracle for `Top`, an f64 evaluation of the operator for the
//! renders, and the unchanged input document for purity. Every surviving
//! record's pixel and mask allocation is compared by ADDRESS.
//!
//! Tolerances, fixed BEFORE anything was measured:
//! * u16 renders: within 1 code (`U16_TOLERANCE`);
//! * an opacity-0 insertion: EXACT, bit for bit, at both storages;
//! * float renders: relative 1e-5 (`FLOAT_TOLERANCE`).

use std::ffi::{c_char, CStr, CString};
use std::ptr;

use doctored_core::doc_evaluation::Evaluation;
use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_insert::dr_precision_doc_add_adjustment_at;
use doctored_core::ffi_precision::dr_precision_image_storage_identity;
use doctored_core::ffi_precision_edit::dr_precision_doc_edit;
use doctored_core::icc::IccProfile;
use doctored_core::layer_insert::InsertPosition;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer as Layer};
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use doctored_core::precision_mask::{PrecisionMask, PrecisionPlane};
use doctored_core::style::{BlendIf, BlendIfChannel, LayerStyle};
use serde_json::{json, Value};

const LIMIT: u64 = REFERENCE_WORKING_BYTES;
const U16_TOLERANCE: i64 = 1;
const FLOAT_TOLERANCE: f64 = 1e-5;
const FLOAT_FIXTURE: &str = "tests/fixtures/float_11f/scene.tiff";

const ABOVE: InsertPosition = InsertPosition::Above;
const TOP: InsertPosition = InsertPosition::Top;
const INTO: InsertPosition = InsertPosition::IntoGroup;

// ------------------------------------------------------------- plumbing --

fn owned(p: *mut c_char) -> Option<String> {
    if p.is_null() {
        return None;
    }
    let text = unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned();
    unsafe { dr_string_free(p) };
    Some(text)
}

/// One request through the public typed edit surface — the only way this
/// file reaches an edit it has no direct constructor for.
fn changed(doc: &PrecisionDocument, request: Value) -> PrecisionDocument {
    let text = CString::new(request.to_string()).unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_edit(
            doc,
            text.as_ptr(),
            ptr::null(),
            0,
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(!out.is_null(), "{request}: {:?}", owned(err));
    *unsafe { Box::from_raw(out) }
}

/// `add_adjustment_at`'s refusal message; panics when the call succeeds.
fn refusal(
    doc: &PrecisionDocument,
    description: &str,
    anchor: usize,
    position: InsertPosition,
) -> String {
    match doc.add_adjustment_at("X", description, 1.0, anchor, position, LIMIT, 0, None) {
        Err(message) => message,
        Ok(_) => panic!("{position:?} at {anchor} must be refused"),
    }
}

fn invert() -> String {
    json!({"type":"adjust","op":"invert","params":{},"evaluation":"precision_v1"}).to_string()
}

fn exposure(stops: f64) -> String {
    json!({"type":"adjust","evaluation":"precision_v1","op":"exposure",
           "params":{"exposure": stops}})
    .to_string()
}

fn shadows_highlights() -> String {
    json!({"type":"adjust","evaluation":"precision_v1","op":"shadows_highlights",
           "params":{"shadows":{"amount":0.5,"tone":0.5},"radius":8.0}})
    .to_string()
}

fn add_at(
    doc: &PrecisionDocument,
    name: &str,
    description: &str,
    opacity: f32,
    anchor: usize,
    position: InsertPosition,
) -> PrecisionDocument {
    doc.add_adjustment_at(name, description, opacity, anchor, position, LIMIT, 0, None)
        .unwrap_or_else(|e| panic!("{name} at {position:?}: {e}"))
        .expect("an insertion is always a change")
}

// --------------------------------------------- the independent structure --

#[derive(Debug, PartialEq, Eq, Clone)]
struct Row {
    name: String,
    level: usize,
    clipped: bool,
}

/// The document's records in POSTORDER — children first, then their group,
/// the numbering `PrecisionDocument::layer` indexes — walked here rather than
/// read off any core report.
fn table(doc: &PrecisionDocument) -> Vec<Row> {
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
    walk(doc.root_layers(), 0, &mut out);
    out
}

fn expect(doc: &PrecisionDocument, want: &[(&str, usize, bool)]) {
    let want: Vec<Row> = want
        .iter()
        .map(|(name, level, clipped)| Row {
            name: (*name).to_string(),
            level: *level,
            clipped: *clipped,
        })
        .collect();
    assert_eq!(table(doc), want);
}

fn ids(doc: &PrecisionDocument) -> Vec<u64> {
    (0..doc.layer_count())
        .map(|i| doc.layer_id(i).expect("an id"))
        .collect()
}

fn index_of(doc: &PrecisionDocument, name: &str) -> usize {
    table(doc)
        .iter()
        .position(|r| r.name == name)
        .unwrap_or_else(|| panic!("{name} is present"))
}

/// The raster's STORAGE IDENTITY, so "the pixels were not touched" is an
/// identity claim rather than a comparison of equal values.
fn pixel_address(doc: &PrecisionDocument, name: &str) -> usize {
    let index = index_of(doc, name);
    let source = doc.layer_source(index).expect("a raster record");
    unsafe { dr_precision_image_storage_identity(source) as usize }
}

fn mask_address(doc: &PrecisionDocument, name: &str) -> usize {
    doc.layer(index_of(doc, name))
        .and_then(|l| l.mask())
        .expect("a masked record")
        .plane()
        .samples()
        .as_ptr() as usize
}

fn render(doc: &PrecisionDocument) -> Vec<[u16; 4]> {
    doc.render(LIMIT, 0, None).unwrap().to_rgba16().unwrap()
}

fn render_float(doc: &PrecisionDocument) -> Vec<[f32; 4]> {
    doc.render(LIMIT, 0, None).unwrap().to_rgba32().unwrap()
}

// -------------------------------------------------------------- fixtures --

fn image(pixels: Vec<[u16; 4]>) -> PrecisionImage {
    PrecisionImage::from_rgba16(pixels.len() as u32, 1, pixels, IccProfile::srgb(), None).unwrap()
}

fn raster(name: &str, value: u16) -> Layer {
    let px: Vec<[u16; 4]> = (0..4u16)
        .map(|i| [value + i * 11, value / 2 + i * 7, value / 3 + i * 3, 65535])
        .collect();
    Layer::raster(name, image(px), (0, 0), 1.0, true).unwrap()
}

fn mask(values: Vec<u16>) -> PrecisionMask {
    PrecisionMask::new(
        PrecisionPlane::from_u16(values.len() as u32, 1, values, None).unwrap(),
        Default::default(),
        true,
    )
    .unwrap()
}

fn blend_if_style() -> LayerStyle {
    let mut style = LayerStyle::default();
    style.blend_if = Some(BlendIf {
        channel: BlendIfChannel::Gray,
        this_layer: [0, 0, 255, 255],
        underlying: [10, 20, 240, 250],
        ..BlendIf::default()
    });
    style
}

fn drop_shadow_style() -> LayerStyle {
    LayerStyle::from_json(
        r#"{"effects":[{"type":"drop_shadow","use_global_light":false,"angle":90.0,
             "distance":1.0,"size":1.0,"opacity":0.5}]}"#,
    )
    .unwrap()
}

/// `[Base, ClipBase, ClipTop(clipped), Group{Inner}, Styled, Ranged]` — a clip
/// run, an ISOLATED group, a styled record and a Blend If record, which is
/// every neighbour the insertion must leave evaluating exactly as it did.
/// `ClipBase` carries a mask so the allocation check has one.
fn fixture() -> PrecisionDocument {
    let canvas = (4u32, 1u32);
    let layers = vec![
        raster("Base", 9000),
        raster("ClipBase", 21000).with_mask(Some(mask(vec![0, 30000, 50000, 65535]))),
        raster("ClipTop", 33000).with_clipped(true),
        Layer::isolated_normal("Group", vec![raster("Inner", 41000)], 1.0, true).unwrap(),
        raster("Styled", 15000)
            .with_layer_style(Some(&drop_shadow_style()), canvas)
            .unwrap(),
        raster("Ranged", 27000)
            .with_layer_style(Some(&blend_if_style()), canvas)
            .unwrap(),
    ];
    let doc = PrecisionDocument::from_parts(
        canvas.0,
        canvas.1,
        IccProfile::srgb(),
        Metadata::default(),
        Resolution::default(),
        layers,
    )
    .unwrap();
    expect(
        &doc,
        &[
            ("Base", 0, false),
            ("ClipBase", 0, false),
            ("ClipTop", 0, true),
            ("Inner", 1, false),
            ("Group", 0, false),
            ("Styled", 0, false),
            ("Ranged", 0, false),
        ],
    );
    doc
}

/// `[Base, ClipBase, ClipTop(clipped), Group{Inner}]` — the fixture's clip run
/// and group WITHOUT the styled and Blend If records, which linear-light
/// evaluation refuses outright (`doc_evaluation`'s operator table).
fn linear_fixture() -> PrecisionDocument {
    let doc = PrecisionDocument::from_parts(
        4,
        1,
        IccProfile::srgb(),
        Metadata::default(),
        Resolution::default(),
        vec![
            raster("Base", 9000),
            raster("ClipBase", 21000).with_mask(Some(mask(vec![0, 30000, 50000, 65535]))),
            raster("ClipTop", 33000).with_clipped(true),
            Layer::isolated_normal("Group", vec![raster("Inner", 41000)], 1.0, true).unwrap(),
        ],
    )
    .unwrap();
    doc.with_evaluation(Evaluation::LinearLight)
        .expect("a switch")
        .expect("a change")
}

fn float_fixture() -> Option<PrecisionDocument> {
    if !std::path::Path::new(FLOAT_FIXTURE).exists() {
        return None;
    }
    let base = PrecisionDocument::open_preserving(FLOAT_FIXTURE, LIMIT, 0, None).ok()?;
    assert!(base.is_float());
    Some(base)
}

// ------------------------------------------------ (a) the three positions --

#[test]
fn a_the_three_positions_land_where_the_rule_says() {
    let doc = fixture();

    let slot = doc.insert_slot(1, ABOVE).expect("a slot");
    assert_eq!((slot.at, slot.level, slot.clipped), (2, 0, true));
    let above = add_at(&doc, "New", &invert(), 1.0, 1, ABOVE);
    expect(
        &above,
        &[
            ("Base", 0, false),
            ("ClipBase", 0, false),
            ("New", 0, true),
            ("ClipTop", 0, true),
            ("Inner", 1, false),
            ("Group", 0, false),
            ("Styled", 0, false),
            ("Ranged", 0, false),
        ],
    );

    // ABOVE a group is above the group, never inside it.
    assert_eq!(doc.insert_slot(4, ABOVE).unwrap().at, 5);

    let slot = doc.insert_slot(4, INTO).expect("a slot");
    assert_eq!((slot.at, slot.level, slot.clipped), (4, 1, false));
    let into = add_at(&doc, "New", &invert(), 1.0, 4, INTO);
    expect(
        &into,
        &[
            ("Base", 0, false),
            ("ClipBase", 0, false),
            ("ClipTop", 0, true),
            ("Inner", 1, false),
            ("New", 1, false),
            ("Group", 0, false),
            ("Styled", 0, false),
            ("Ranged", 0, false),
        ],
    );

    for anchor in 0..doc.layer_count() {
        let slot = doc.insert_slot(anchor, TOP).expect("a slot");
        assert_eq!((slot.at, slot.level, slot.clipped), (7, 0, false));
    }
    let top = add_at(&doc, "New", &invert(), 1.0, 3, TOP);
    expect(
        &top,
        &[
            ("Base", 0, false),
            ("ClipBase", 0, false),
            ("ClipTop", 0, true),
            ("Inner", 1, false),
            ("Group", 0, false),
            ("Styled", 0, false),
            ("Ranged", 0, false),
            ("New", 0, false),
        ],
    );

    // `Top` IS `add_adjustment`: same table, same ids carried, same render.
    let appended = doc
        .add_adjustment("New", &invert(), 1.0, LIMIT, 0, None)
        .expect("appended")
        .expect("a change");
    assert_eq!(table(&top), table(&appended));
    assert_eq!(ids(&top)[..7], ids(&appended)[..7]);
    assert_eq!(render(&top), render(&appended));
}

#[test]
fn b_identities_allocations_and_the_input_are_untouched() {
    let doc = fixture();
    let before = table(&doc);
    let before_ids = ids(&doc);
    let pixels: Vec<usize> = ["Base", "ClipBase", "ClipTop", "Inner", "Styled", "Ranged"]
        .iter()
        .map(|n| pixel_address(&doc, n))
        .collect();
    let mask_before = mask_address(&doc, "ClipBase");

    for (anchor, position) in [(1, ABOVE), (4, INTO), (0, TOP)] {
        let out = add_at(&doc, "New", &invert(), 1.0, anchor, position);
        for (i, name) in ["Base", "ClipBase", "ClipTop", "Inner", "Styled", "Ranged"]
            .iter()
            .enumerate()
        {
            assert_eq!(
                pixel_address(&out, name),
                pixels[i],
                "{name}'s samples are the same allocation after {position:?}"
            );
        }
        assert_eq!(mask_address(&out, "ClipBase"), mask_before);
        // Every prior id carried, in order, and exactly ONE fresh mint.
        let after = ids(&out);
        let fresh = out.layer_id(index_of(&out, "New")).expect("an id");
        let carried: Vec<u64> = after.iter().copied().filter(|&id| id != fresh).collect();
        assert_eq!(carried, before_ids);
        assert!(fresh != 0 && !before_ids.contains(&fresh), "a fresh id");
        // PURITY: the source document is the document it was.
        assert_eq!(table(&doc), before);
        assert_eq!(ids(&doc), before_ids);
    }
}

// ------------------------------------------------------- (c) the renders --

#[test]
fn c_an_inserted_adjustment_matches_an_f64_oracle() {
    // One opaque record; the oracle is the invert kernel in f64 —
    // `65535 - c` per RGB channel with alpha carried.
    let source = raster("Base", 12345);
    let base = PrecisionDocument::from_parts(
        4,
        1,
        IccProfile::srgb(),
        Metadata::default(),
        Resolution::default(),
        vec![source],
    )
    .unwrap();
    let want = render(&base);
    let out = render(&add_at(&base, "Invert", &invert(), 1.0, 0, ABOVE));
    for (got, src) in out.iter().zip(want.iter()) {
        for c in 0..3 {
            let oracle = 65535.0f64 - f64::from(src[c]);
            assert!(
                (f64::from(got[c]) - oracle).abs() <= U16_TOLERANCE as f64,
                "invert: {} vs {oracle}",
                got[c]
            );
        }
        assert_eq!(got[3], src[3], "alpha is carried");
    }

    // The same adjustment INSIDE an isolated group over an opaque backdrop:
    // the group's render IS the invert, so the oracle is unchanged.
    let grouped = PrecisionDocument::from_parts(
        4,
        1,
        IccProfile::srgb(),
        Metadata::default(),
        Resolution::default(),
        vec![
            raster("Base", 5000),
            Layer::isolated_normal("Group", vec![raster("Inner", 12345)], 1.0, true).unwrap(),
        ],
    )
    .unwrap();
    let group = index_of(&grouped, "Group");
    let into = render(&add_at(&grouped, "Invert", &invert(), 1.0, group, INTO));
    for (got, src) in into.iter().zip(want.iter()) {
        for c in 0..3 {
            let oracle = 65535.0f64 - f64::from(src[c]);
            assert!(
                (f64::from(got[c]) - oracle).abs() <= U16_TOLERANCE as f64,
                "invert inside the group: {} vs {oracle}",
                got[c]
            );
        }
    }
}

#[test]
fn d_an_opacity_zero_insertion_is_bit_identical() {
    // The strongest "the neighbours evaluate exactly as they did" claim: an
    // adjustment that WOULD change every sample, inserted at each of the
    // three positions at opacity 0, over a fixture holding an isolated group,
    // a clip run, a Blend If record and a styled record.
    let doc = fixture();
    let want = render(&doc);
    for (anchor, position) in [(1, ABOVE), (4, INTO), (0, TOP)] {
        let out = add_at(&doc, "Invert", &invert(), 0.0, anchor, position);
        assert_eq!(render(&out), want, "{position:?} at opacity 0 is exact");
    }
}

#[test]
fn e_an_insert_above_a_clip_base_joins_the_run() {
    // [Base, ClipBase (opaque on the left half, absent on the right),
    //  ClipTop (clipped, fully transparent)] — so the only visible effect of
    // the inserted invert is where the clip run confines it.
    let base: Vec<[u16; 4]> = (0..4u16)
        .map(|i| [1000 + i * 100, 20000, 40000, 65535])
        .collect();
    let clip_base: Vec<[u16; 4]> = (0..4u16)
        .map(|i| {
            if i < 2 {
                [30000, 44000, 9000, 65535]
            } else {
                [0, 0, 0, 0]
            }
        })
        .collect();
    let doc = PrecisionDocument::from_parts(
        4,
        1,
        IccProfile::srgb(),
        Metadata::default(),
        Resolution::default(),
        vec![
            Layer::raster("Base", image(base.clone()), (0, 0), 1.0, true).unwrap(),
            Layer::raster("ClipBase", image(clip_base.clone()), (0, 0), 1.0, true).unwrap(),
            Layer::raster("ClipTop", image(vec![[0; 4]; 4]), (0, 0), 1.0, true)
                .unwrap()
                .with_clipped(true),
        ],
    )
    .unwrap();

    let out = add_at(&doc, "Invert", &invert(), 1.0, 1, ABOVE);
    let rows = table(&out);
    assert!(rows[2].clipped, "the new record joined the run");
    assert!(rows[3].clipped, "ClipTop's own flag is untouched");
    assert!(!rows[1].clipped, "the base is still the base");

    // The oracle: inside the base's footprint the backdrop is ClipBase's own
    // colour, inverted; outside it the clip unit contributes nothing and the
    // projection is `Base`. An UNCLIPPED insert would have inverted the right
    // half too, which is exactly the difference this asserts.
    let got = render(&out);
    for i in 0..4usize {
        for c in 0..3 {
            let oracle = if i < 2 {
                65535.0f64 - f64::from(clip_base[i][c])
            } else {
                f64::from(base[i][c])
            };
            assert!(
                (f64::from(got[i][c]) - oracle).abs() <= U16_TOLERANCE as f64,
                "clip-run join at x={i}: {} vs {oracle}",
                got[i][c]
            );
        }
    }
}

// ------------------------------------------------------ (f) linear light --

#[test]
fn f_linear_light_evaluates_and_refuses_the_same_way() {
    let doc = linear_fixture();
    assert_eq!(doc.linear_light_refusal(), None, "the fixture is admitted");

    // Inserting INTO the group is the same document `add_adjustment` +
    // `move_layer_to` produces — the same structure and the same projection,
    // so the positional path is not a second evaluation.
    let group = index_of(&doc, "Group");
    let into = add_at(&doc, "Invert", &invert(), 1.0, group, INTO);
    let appended = doc
        .add_adjustment("Invert", &invert(), 1.0, LIMIT, 0, None)
        .expect("appended")
        .expect("a change");
    let moved = appended
        .move_layer_to(appended.layer_count() - 1, 4, 1)
        .expect("a move")
        .expect("a change");
    assert_eq!(table(&into), table(&moved));
    assert_eq!(render(&into), render(&moved));

    // Shadows/Highlights is unmigrated: the refusal walk names it at every
    // position, with the same operator and the same tail, differing only in
    // the INDEX the record landed at (which is the sentence's whole point).
    for (anchor, position, landing) in [(1, ABOVE, 2usize), (group, INTO, 4), (0, TOP, 5)] {
        let out = add_at(&doc, "SH", &shadows_highlights(), 1.0, anchor, position);
        let sentence = out.linear_light_refusal().expect("a refusal");
        assert_eq!(
            sentence,
            format!(
                "Layer {landing} “SH” uses Shadows/Highlights with a guide, which linear-light \
                 evaluation does not support yet. Switch Image ▸ Evaluation back to Encoded, or \
                 remove it from the layer."
            ),
            "{position:?}"
        );
    }
}

// ------------------------------------------------------------- (g) float --

#[test]
fn g_float_admits_the_three_positions() {
    let Some(base) = float_fixture() else {
        eprintln!("skipping: {FLOAT_FIXTURE} is absent");
        return;
    };
    // The exposure oracle: +1 EV in linear light is a factor of two, applied
    // to the reference-basis components with no TRC round trip.
    let before = base.layer_source(0).unwrap().to_rgba32().unwrap();
    let out = render_float(&add_at(&base, "Exposure", &exposure(1.0), 1.0, 0, ABOVE));
    for (got, src) in out.iter().zip(before.iter()) {
        if src[3] == 0.0 {
            continue;
        }
        for c in 0..3 {
            let oracle = f64::from(src[c]) * 2.0;
            assert!(
                (f64::from(got[c]) - oracle).abs() <= FLOAT_TOLERANCE * oracle.abs().max(1e-3),
                "+1 EV: {} vs {oracle}",
                got[c]
            );
        }
        assert_eq!(got[3], src[3], "alpha is carried");
    }

    // A group and a clip run at float, with an opacity-0 insertion at each
    // position: EXACT, bit for bit.
    let second = base.duplicate_layer(0).unwrap().unwrap();
    let grouped = second
        .group_layers(&[1], "Group", false)
        .expect("grouping")
        .expect("a change")
        .0;
    let third = grouped.duplicate_layer(0).unwrap().unwrap();
    let clipped = changed(
        &third,
        json!({"op":"layer_clipped","layer":1,"clipped":true}),
    );
    assert_eq!(clipped.float_refusal(), None, "the float fixture renders");
    let want = render_float(&clipped);
    let group = index_of(&clipped, "Group");
    for (anchor, position) in [(0, ABOVE), (group, INTO), (0, TOP)] {
        let out = add_at(&clipped, "Exposure", &exposure(1.0), 0.0, anchor, position);
        assert_eq!(
            render_float(&out),
            want,
            "{position:?} at opacity 0 is exact at float"
        );
    }
}

// ----------------------------------------------------------- (h) refusals --

#[test]
fn h_every_refusal_is_named() {
    let doc = fixture();
    assert_eq!(
        doc.insert_slot(99, ABOVE).unwrap_err(),
        "layer insert anchor is out of range"
    );
    assert_eq!(
        doc.insert_slot(0, INTO).unwrap_err(),
        "into_group requires a group layer"
    );
    assert_eq!(
        refusal(&doc, &invert(), 99, ABOVE),
        "layer insert anchor is out of range"
    );
    assert_eq!(
        refusal(&doc, &invert(), 0, INTO),
        "into_group requires a group layer"
    );
    // The slot is resolved BEFORE the operator table is prepared, so a bad
    // position on a malformed description still reports the POSITION.
    assert_eq!(
        refusal(&doc, "not json", 0, INTO),
        "into_group requires a group layer"
    );
    // The nesting ceiling has no typed twin here on purpose. `layer_insert`'s
    // branch is shared, but the typed limit is a subtree HEIGHT checked when
    // a group is BUILT (`PrecisionDocumentLayer::group` refuses children at
    // `MAX_GROUP_DEPTH`), so a typed group can only reach level
    // `MAX_GROUP_DEPTH - 1` and `into_group` on it lands exactly AT the cap:
    // the sentence is unreachable at this depth. The BYTE limit is a LEVEL,
    // an emptied group can sit AT it, and `tests/layer_insert.rs` builds that
    // stack and asserts the sentence.
}

// ---------------------------------------------------------------- (i) FFI --

#[test]
fn i_the_export_tolerates_null_and_refuses_a_bad_code() {
    let name = CString::new("New").unwrap();
    let description = CString::new(invert()).unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_add_adjustment_at(
            ptr::null(),
            name.as_ptr(),
            description.as_ptr(),
            1.0,
            0,
            0,
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(out.is_null());
    assert_eq!(owned(err).as_deref(), Some("required handle is NULL"));

    let doc = fixture();
    // An unknown position code, reported by name rather than defaulted.
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_add_adjustment_at(
            &doc,
            name.as_ptr(),
            description.as_ptr(),
            1.0,
            0,
            9,
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(out.is_null());
    assert_eq!(
        owned(err).as_deref(),
        Some("unsupported layer insert position")
    );

    // A NULL name.
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_add_adjustment_at(
            &doc,
            ptr::null(),
            description.as_ptr(),
            1.0,
            0,
            0,
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(out.is_null());
    assert_eq!(owned(err).as_deref(), Some("name is NULL"));

    // …and the three codes that ARE the enum.
    for (code, landing) in [(0u32, 1usize), (1, 7), (2, 4)] {
        let anchor = if code == 2 { 4 } else { 0 };
        let mut err = ptr::null_mut();
        let out = unsafe {
            dr_precision_doc_add_adjustment_at(
                &doc,
                name.as_ptr(),
                description.as_ptr(),
                1.0,
                anchor,
                code,
                LIMIT,
                0,
                ptr::null(),
                &mut err,
            )
        };
        assert!(!out.is_null(), "code {code}: {:?}", owned(err));
        let result = *unsafe { Box::from_raw(out) };
        assert_eq!(index_of(&result, "New"), landing, "code {code}");
    }
}
