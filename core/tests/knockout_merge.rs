//! EXACT MERGES — Phase 12E-3b's contract (g): Merge Down and Merge Layers
//! evaluate a knockout against the REAL backdrop at 8 and 16 bits wherever one
//! exists, and Merge Visible stays exact.
//!
//! # The tolerances, stated BEFORE anything was measured
//!
//! **T1 — 1 code of 255, alpha included** at 8 bits; **T3 — 1 code of 65535,
//! alpha included** at 16. Every merge scene here is hard-edged (canvas-sized
//! opaque fills at promoted codes), so a single code is the whole quantization
//! budget and nothing hides in it. The central claim is an EQUALITY of two
//! renders of the same picture — the projection BEFORE the merge against the
//! projection AFTER it, at every canvas pixel — so a miss is a real picture
//! change and is recorded with its numbers rather than widened.
//!
//! # What is NOT claimed
//!
//! `style_knockout_merge`'s module doc states the limit: a merged raster
//! cannot un-draw what lies between, so exactness holds wherever `B0` is
//! PRESENT and OPAQUE under the shape. Where `B0` is ABSENT BY STRUCTURE — the
//! merge sits inside an ISOLATED group, or at the root of a document with no
//! qualifying Background — the hole still bakes as transparency. The last test
//! in this file pins that residual with the numbers on both sides rather than
//! leaving it to prose.
//!
//! # The budget refusal has no test, deliberately
//!
//! `RenderBudget`'s sentence is not black-box reachable at these sizes: every
//! merge uses `RenderBudget::default()`, which is four times `MAX_PIXELS`, and
//! these canvases are 64 pixels.

use std::ffi::{c_char, CString};
use std::ptr;

use doctored_core::doc::{BlendMode, DrDocument};
use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_doc::*;
use doctored_core::ffi_group::*;
use doctored_core::ffi_precision_edit::dr_precision_doc_edit;
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer as TLayer};
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use doctored_core::style::LayerStyle;
use serde_json::{json, Value};
use tempfile::TempDir;

use crate::common;
use common::*;

const W: u32 = 8;
const H: u32 = 8;

const BG: [u8; 4] = [204, 34, 34, 255];
const MID: [u8; 4] = [34, 204, 34, 255];
const HIGH: [u8; 4] = [34, 34, 204, 255];
const EXTRA: [u8; 4] = [204, 204, 34, 255];
const FOURTH: [u8; 4] = [120, 60, 200, 255];

const NORMAL: i32 = 0;
const PASS_THROUGH: i32 = 27;

/// T1: one 8-bit code, alpha included.
const T1: u8 = 1;
/// T3: one 16-bit code, alpha included.
const T3: u16 = 1;

// ------------------------------------------------------- byte plumbing --

fn scene(dir: &TempDir, color: [u8; 4]) -> *mut DrDocument {
    doc_from(dir, "bg.png", &solid(W, H, color))
}

fn over(
    dir: &TempDir,
    file: &str,
    doc: *mut DrDocument,
    idx: usize,
    color: [u8; 4],
    name: &str,
) -> *mut DrDocument {
    add_layer(dir, file, doc, idx, &solid(W, H, color), name)
}

fn set_blend(doc: *mut DrDocument, idx: usize, mode: i32) -> *mut DrDocument {
    apply_or_keep(doc, |d| unsafe {
        dr_doc_with_layer_blend_mode(d, idx, mode)
    })
}

fn set_clipped(doc: *mut DrDocument, idx: usize) -> *mut DrDocument {
    apply(doc, |d| unsafe { dr_doc_with_layer_clipped(d, idx, true) })
}

fn set_opacity(doc: *mut DrDocument, idx: usize, opacity: f32) -> *mut DrDocument {
    apply_or_keep(doc, |d| unsafe {
        dr_doc_with_layer_opacity(d, idx, opacity)
    })
}

/// `dr_doc_group_layers` over a set, answering the container's index.
fn group_many(doc: *mut DrDocument, idx: &[usize], name: &str) -> (*mut DrDocument, usize) {
    let cname = CString::new(name).expect("no interior NUL");
    let cap = unsafe { dr_doc_layer_count(doc) } + 1;
    let mut group = usize::MAX;
    let mut cleared = vec![0usize; cap];
    let mut cleared_len = 0usize;
    let mut reordered = vec![0usize; cap];
    let mut reordered_len = 0usize;
    let out = unsafe {
        dr_doc_group_layers(
            doc,
            idx.as_ptr(),
            idx.len(),
            cname.as_ptr(),
            &mut group,
            cleared.as_mut_ptr(),
            &mut cleared_len,
            reordered.as_mut_ptr(),
            &mut reordered_len,
            cap,
        )
    };
    assert!(!out.is_null(), "group_layers({idx:?}) refused");
    unsafe { dr_doc_free(doc) };
    (out, group)
}

fn knockout_style(knockout: &str, fill: f32) -> String {
    format!("{{\"knockout\":\"{knockout}\",\"fill_opacity\":{fill}}}")
}

/// The SAME style with no knockout key — the twin every "the knockout is
/// observable here" half is measured against.
fn plain_style(fill: f32) -> String {
    format!("{{\"fill_opacity\":{fill}}}")
}

/// The projection BEFORE and AFTER an op, compared at EVERY canvas pixel
/// within T1. The fixture is built twice so the "before" render can never be
/// the same handle the op consumed.
fn byte_exact(
    label: &str,
    build: impl Fn() -> *mut DrDocument,
    op: impl FnOnce(*const DrDocument) -> *mut DrDocument,
) {
    let before_doc = build();
    let before = flat_pixels(before_doc);
    unsafe { dr_doc_free(before_doc) };
    let merged = apply(build(), op);
    let after = flat_pixels(merged);
    unsafe { dr_doc_free(merged) };
    assert_eq!(before.len(), after.len(), "{label}: dimensions");
    let mut worst = (0usize, 0u8);
    for (i, (&b, &a)) in before.iter().zip(after.iter()).enumerate() {
        let d = b.abs_diff(a);
        if d > worst.1 {
            worst = (i, d);
        }
    }
    assert!(
        worst.1 <= T1,
        "{label}: before != after, worst {} codes at byte {} ({} vs {})",
        worst.1,
        worst.0,
        before[worst.0],
        after[worst.0]
    );
}

// ------------------------------------------------------ typed plumbing --

fn promoted(p: [u8; 4]) -> [u16; 4] {
    p.map(|v| u16::from(v) * 257)
}

fn tlayer(name: &str, color: [u8; 4]) -> TLayer {
    let image = PrecisionImage::from_rgba16(
        W,
        H,
        vec![promoted(color); (W * H) as usize],
        IccProfile::srgb(),
        None,
    )
    .unwrap();
    TLayer::raster(name, image, (0, 0), 1.0, true).unwrap()
}

fn tstyled(layer: TLayer, json: &str) -> TLayer {
    layer
        .with_layer_style(Some(&LayerStyle::from_json(json).unwrap()), (W, H))
        .unwrap_or_else(|e| panic!("{json}: {e}"))
}

fn tdoc(layers: Vec<TLayer>) -> PrecisionDocument {
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

fn trender(doc: &PrecisionDocument) -> Vec<[u16; 4]> {
    doc.render(REFERENCE_WORKING_BYTES, 0, None)
        .unwrap()
        .to_rgba16()
        .unwrap()
}

fn tedit(doc: &PrecisionDocument, request: Value) -> Result<PrecisionDocument, String> {
    let request = CString::new(request.to_string()).unwrap();
    let mut err: *mut c_char = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_edit(
            doc,
            request.as_ptr(),
            ptr::null(),
            0,
            REFERENCE_WORKING_BYTES,
            0,
            ptr::null(),
            &mut err,
        )
    };
    if !err.is_null() {
        let text = unsafe { std::ffi::CStr::from_ptr(err) }
            .to_string_lossy()
            .into_owned();
        unsafe { dr_string_free(err) };
        return Err(text);
    }
    assert!(!out.is_null(), "a typed merge is never an exact no-op");
    Ok(*unsafe { Box::from_raw(out) })
}

/// The typed projection BEFORE and AFTER an edit, at every canvas pixel
/// within T3.
fn typed_exact(label: &str, doc: &PrecisionDocument, request: Value) {
    let before = trender(doc);
    let merged = tedit(doc, request).unwrap_or_else(|e| panic!("{label}: {e}"));
    let after = trender(&merged);
    assert_eq!(before.len(), after.len(), "{label}: dimensions");
    let mut worst = (0usize, 0u16);
    for (i, (b, a)) in before.iter().zip(after.iter()).enumerate() {
        for c in 0..4 {
            let d = b[c].abs_diff(a[c]);
            if d > worst.1 {
                worst = (i, d);
            }
        }
    }
    assert!(
        worst.1 <= T3,
        "{label}: before != after, worst {} codes at pixel {} ({:?} vs {:?})",
        worst.1,
        worst.0,
        before[worst.0],
        after[worst.0]
    );
}

// ------------------------------------------------- scene 1-3: the root --

/// `[BG, MidA, MidB, Top(Deep, fill 0)]`: the Background is qualifying, a
/// layer sits BETWEEN the merged pair and it, and `Top` punches all the way
/// down. This is the scene D7's old disclosure was about.
fn root_fixture(dir: &TempDir) -> *mut DrDocument {
    let mut doc = scene(dir, BG);
    doc = over(dir, "a.png", doc, 0, MID, "MidA");
    doc = over(dir, "b.png", doc, 1, EXTRA, "MidB");
    doc = over(dir, "top.png", doc, 2, HIGH, "Top");
    set_style(doc, 3, &knockout_style("deep", 0.0))
}

fn typed_root() -> PrecisionDocument {
    tdoc(vec![
        tlayer("BG", BG),
        tlayer("MidA", MID),
        tlayer("MidB", EXTRA),
        tstyled(tlayer("Top", HIGH), &knockout_style("deep", 0.0)),
    ])
}

#[test]
fn the_five_scenes_merge_exactly_at_8_bits() {
    let dir = TempDir::new().unwrap();

    // 1. Merge Down of the knockout into the layer beneath it.
    byte_exact(
        "scene 1 merge_down",
        || root_fixture(&dir),
        |d| unsafe { dr_doc_merging_down(d, 3) },
    );

    // 2. The same set through Merge Layers.
    byte_exact(
        "scene 2 merge_layers",
        || root_fixture(&dir),
        |d| {
            let idx = [2usize, 3];
            unsafe { dr_doc_merge_layers(d, idx.as_ptr(), idx.len()) }
        },
    );

    // 3. Merge Visible over the whole scene — already exact at 8 bits
    // (`doc_structure::merge_visible` bakes the document's own projection),
    // and this is the gate that KEEPS it so.
    byte_exact(
        "scene 3 merge_visible",
        || root_fixture(&dir),
        |d| unsafe { dr_doc_merge_visible(d) },
    );

    // 4. A Shallow child of a PASS-THROUGH group merged into its lower
    // sibling, with a third sibling beneath both.
    byte_exact(
        "scene 4 shallow inside a pass-through group",
        || pass_through_fixture(&dir),
        |d| unsafe { dr_doc_merging_down(d, 3) },
    );

    // 5. A knockout CLIPPED MEMBER merged WITH its base: `member_env`'s
    // base-only snapshot, which was already internal, and must stay exact.
    // NON-VACUITY FIRST: `C0` is the lower clipped member that makes this
    // scene able to SEE the knockout at all — the base-only snapshot is Base,
    // while a no-knockout render of the same stack shows C0. Without it a Deep
    // member at fill 0 reads a snapshot equal to the picture that was already
    // there, and an exact merge would prove nothing.
    let knocked_doc = clip_fixture(&dir, true);
    let knocked = pixel(&flat_pixels(knocked_doc), W, 4, 4);
    unsafe { dr_doc_free(knocked_doc) };
    let plain_doc = clip_fixture(&dir, false);
    let plain = pixel(&flat_pixels(plain_doc), W, 4, 4);
    unsafe { dr_doc_free(plain_doc) };
    assert_eq!(knocked, MID, "the hole shows the base-only snapshot");
    assert_eq!(plain, EXTRA, "without the knockout the lower member shows");
    byte_exact(
        "scene 5 clipped member merged with its base",
        || clip_fixture(&dir, true),
        |d| {
            let idx = [1usize, 2, 3];
            unsafe { dr_doc_merge_layers(d, idx.as_ptr(), idx.len()) }
        },
    );
}

/// `[BG, Sib, P(Pass Through){Lower, K(Shallow, fill 0)}]` — indices
/// `[BG 0, Sib 1, Lower 2, K 3, P 4]`. Shallow's `B0` is the accumulator as
/// the group's children found it: BG under Sib.
fn pass_through_fixture(dir: &TempDir) -> *mut DrDocument {
    let mut doc = scene(dir, BG);
    doc = over(dir, "sib.png", doc, 0, MID, "Sib");
    doc = over(dir, "lower.png", doc, 1, EXTRA, "Lower");
    doc = over(dir, "k.png", doc, 2, HIGH, "K");
    doc = set_style(doc, 3, &knockout_style("shallow", 0.0));
    let (doc, group) = group_many(doc, &[2, 3], "P");
    set_blend(doc, group, PASS_THROUGH)
}

/// `[BG, Base, C0(clipped), Member(clipped, Deep, fill 0)]`, merged WHOLE
/// (`Merge Layers {Base, C0, Member}`) so the lowest member is the BASE and
/// the set is not the excluded one. `C0` sits BETWEEN the base and the
/// knockout member, which is what makes the base-only snapshot differ from the
/// picture a no-knockout render shows.
fn clip_fixture(dir: &TempDir, knocked: bool) -> *mut DrDocument {
    let mut doc = scene(dir, BG);
    doc = over(dir, "base.png", doc, 0, MID, "Base");
    doc = over(dir, "c0.png", doc, 1, EXTRA, "C0");
    doc = set_clipped(doc, 2);
    doc = over(dir, "member.png", doc, 2, HIGH, "Member");
    doc = set_clipped(doc, 3);
    let style = if knocked {
        knockout_style("deep", 0.0)
    } else {
        plain_style(0.0)
    };
    set_style(doc, 3, &style)
}

/// The typed twin of [`clip_fixture`].
fn typed_clip(knocked: bool) -> PrecisionDocument {
    let style = if knocked {
        knockout_style("deep", 0.0)
    } else {
        plain_style(0.0)
    };
    tdoc(vec![
        tlayer("BG", BG),
        tlayer("Base", MID),
        tlayer("C0", EXTRA).with_clipped(true),
        tstyled(tlayer("Member", HIGH), &style).with_clipped(true),
    ])
}

#[test]
fn the_five_scenes_merge_exactly_at_16_bits() {
    // 1-3: the root scene.
    let doc = typed_root();
    typed_exact(
        "scene 1 merge_down",
        &doc,
        json!({"op":"merge_down","layer":3}),
    );
    typed_exact(
        "scene 2 merge_layers",
        &doc,
        json!({"op":"merge_layers","layers":[2,3]}),
    );
    typed_exact("scene 3 merge_visible", &doc, json!({"op":"merge_visible"}));

    // 4: a Shallow child of a pass-through group, with a third sibling below.
    let doc = tdoc(vec![
        tlayer("BG", BG),
        tlayer("Sib", MID),
        TLayer::pass_through(
            "P",
            vec![
                tlayer("Lower", EXTRA),
                tstyled(tlayer("K", HIGH), &knockout_style("shallow", 0.0)),
            ],
            1.0,
            true,
        )
        .unwrap(),
    ]);
    typed_exact(
        "scene 4 shallow inside a pass-through group",
        &doc,
        json!({"op":"merge_down","layer":3}),
    );

    // 5: a knockout clipped member merged WITH its base, with the lower
    // clipped member C0 between them. Non-vacuity first, as at 8 bits.
    let doc = typed_clip(true);
    let probe = (4 * W + 4) as usize;
    assert_eq!(
        trender(&doc)[probe],
        promoted(MID),
        "the hole shows the base-only snapshot"
    );
    assert_eq!(
        trender(&typed_clip(false))[probe],
        promoted(EXTRA),
        "without the knockout the lower member shows"
    );
    typed_exact(
        "scene 5 clipped member merged with its base",
        &doc,
        json!({"op":"merge_layers","layers":[1,2,3]}),
    );
}

// -------------------------------------------------- merging INTO the BG --

#[test]
fn merging_into_the_background_captures_the_background_snapshot_at_both_depths() {
    // `at == 0`: the capture must be D5's Background snapshot, which only
    // `composite_projection_into`'s own offer of the FULL root level can give
    // it — the remaining level would have offered `NONE`.
    let dir = TempDir::new().unwrap();
    let build = || {
        let doc = scene(&dir, BG);
        let doc = over(&dir, "k.png", doc, 0, HIGH, "K");
        set_style(doc, 1, &knockout_style("deep", 0.0))
    };
    // Non-vacuity FIRST: with a transparent `B0` the merged layer would be a
    // hole, so the projection is NOT trivially unchanged.
    let probe_doc = build();
    let before = flat_pixels(probe_doc);
    unsafe { dr_doc_free(probe_doc) };
    assert_eq!(pixel(&before, W, 4, 4), BG, "the projection punches to BG");
    byte_exact("merge into the Background", build, |d| unsafe {
        dr_doc_merging_down(d, 1)
    });

    let doc = tdoc(vec![
        tlayer("BG", BG),
        tstyled(tlayer("K", HIGH), &knockout_style("deep", 0.0)),
    ]);
    let before = trender(&doc);
    assert_eq!(
        before[(4 * W + 4) as usize],
        promoted(BG),
        "the typed projection punches to BG"
    );
    typed_exact(
        "merge into the Background",
        &doc,
        json!({"op":"merge_down","layer":1}),
    );
}

// ------------------------------------------------ a gated pass-through --

#[test]
fn a_gated_pass_through_parent_is_exact_at_8_bits() {
    // A pass-through group carrying a sub-1 opacity takes
    // `composite_gated_pass`, which borrows the untouched accumulator as
    // Shallow's `B0` and allocates nothing extra.
    let dir = TempDir::new().unwrap();
    byte_exact(
        "gated pass-through parent",
        || {
            let doc = pass_through_fixture(&dir);
            set_opacity(doc, 4, 0.5)
        },
        |d| unsafe { dr_doc_merging_down(d, 3) },
    );
}

#[test]
fn a_gated_pass_through_parent_is_exact_at_16_bits() {
    // The typed twin of the scene above: the `Kind::PassThrough` arm hands its
    // children `shallow: Some(&*acc)` — the untouched accumulator — and gates
    // the evaluated buffer by the group's opacity afterwards, so a sub-1
    // opacity changes nothing about WHICH backdrop the child reads.
    // Postorder: BG 0, Sib 1, Lower 2, K 3, P 4.
    let doc = tdoc(vec![
        tlayer("BG", BG),
        tlayer("Sib", MID),
        TLayer::pass_through(
            "P",
            vec![
                tlayer("Lower", EXTRA),
                tstyled(tlayer("K", HIGH), &knockout_style("shallow", 0.0)),
            ],
            0.5,
            true,
        )
        .unwrap(),
    ]);
    typed_exact(
        "gated pass-through parent",
        &doc,
        json!({"op":"merge_down","layer":3}),
    );
}

// ------------------------------------------- the nested early returns --

#[test]
fn a_knockout_merge_inside_an_isolated_group_is_exact_when_nothing_lies_beneath_it() {
    // `rendered_group`'s capture early return: the group renders only far
    // enough to record the backdrop (absent — D4's hard stop), and the merge
    // then bakes exactly what the projection showed, because no visible
    // sibling sits beneath the merged pair inside the group.
    let dir = TempDir::new().unwrap();
    let build = || {
        let mut doc = scene(&dir, BG);
        doc = over(&dir, "l.png", doc, 0, EXTRA, "L");
        doc = over(&dir, "k.png", doc, 1, HIGH, "K");
        doc = set_style(doc, 2, &knockout_style("deep", 0.0));
        let (doc, group) = group_many(doc, &[1, 2], "I");
        set_blend(doc, group, NORMAL)
    };
    byte_exact("isolated group, nothing beneath", build, |d| unsafe {
        dr_doc_merging_down(d, 2)
    });

    let doc = tdoc(vec![
        tlayer("BG", BG),
        TLayer::isolated_normal(
            "I",
            vec![
                tlayer("L", EXTRA),
                tstyled(tlayer("K", HIGH), &knockout_style("deep", 0.0)),
            ],
            1.0,
            true,
        )
        .unwrap(),
    ]);
    typed_exact(
        "isolated group, nothing beneath",
        &doc,
        json!({"op":"merge_down","layer":2}),
    );
}

#[test]
fn a_pass_through_group_inside_an_isolated_group_captures_its_entry_state() {
    // `[BG, I(Normal){O opaque, P(Pass Through){S3, L, K Shallow fill 0}}]`,
    // merging K into L. `B0` is P's entry state — I's buffer after O — so the
    // capture has to come out of a NESTED accumulator, through both early
    // returns, and neither may raise an error a full render would not.
    let dir = TempDir::new().unwrap();
    let build = || {
        // Inside-out: O, S3, L, K at the root, then P over {S3, L, K}, then I
        // over {O, P}.
        let mut doc = scene(&dir, BG);
        doc = over(&dir, "o.png", doc, 0, MID, "O");
        doc = over(&dir, "s3.png", doc, 1, FOURTH, "S3");
        doc = over(&dir, "l.png", doc, 2, EXTRA, "L");
        doc = over(&dir, "k.png", doc, 3, HIGH, "K");
        doc = set_style(doc, 4, &knockout_style("shallow", 0.0));
        let (doc, p) = group_many(doc, &[2, 3, 4], "P");
        let doc = set_blend(doc, p, PASS_THROUGH);
        let (doc, i) = group_many(doc, &[1, p], "I");
        set_blend(doc, i, NORMAL)
    };
    // The merged record is K, whose index after the two groupings is 4.
    byte_exact("pass-through inside an isolated group", build, |d| unsafe {
        dr_doc_merging_down(d, 4)
    });

    let doc = tdoc(vec![
        tlayer("BG", BG),
        TLayer::isolated_normal(
            "I",
            vec![
                tlayer("O", MID),
                TLayer::pass_through(
                    "P",
                    vec![
                        tlayer("S3", FOURTH),
                        tlayer("L", EXTRA),
                        tstyled(tlayer("K", HIGH), &knockout_style("shallow", 0.0)),
                    ],
                    1.0,
                    true,
                )
                .unwrap(),
            ],
            1.0,
            true,
        )
        .unwrap(),
    ]);
    // Postorder: O 0, S3 1, L 2, K 3, P 4, I 5.
    typed_exact(
        "pass-through inside an isolated group",
        &doc,
        json!({"op":"merge_down","layer":3}),
    );
}

// ---------------------------------------- D5 must not fire in a members doc --

#[test]
fn a_typed_merge_never_lets_d5_fire_inside_the_derived_document() {
    // `[BG, L, X2, G(Pass Through){K Deep fill 0}]`, Merge Layers {L, X2, G}
    // with L canvas-sized, opaque, Normal, opacity 1 at (0, 0): exactly what
    // `background_record` accepts. If the members document were rendered as
    // an ordinary PROJECTION, D5 would take L for a Background and K would
    // punch to it — showing L where the screen showed BG. Rendering every
    // members document `Handed` is what forbids that.
    let doc = tdoc(vec![
        tlayer("BG", BG),
        tlayer("L", MID),
        tlayer("X2", EXTRA),
        TLayer::pass_through(
            "G",
            vec![tstyled(tlayer("K", HIGH), &knockout_style("deep", 0.0))],
            1.0,
            true,
        )
        .unwrap(),
    ]);
    // Postorder: BG 0, L 1, X2 2, K 3, G 4.
    typed_exact(
        "a pass-through group member holding a Deep child",
        &doc,
        json!({"op":"merge_layers","layers":[1,2,4]}),
    );
}

// ---------------------------------------------- the two excluded shapes --

#[test]
fn a_lowest_member_clipped_to_an_outside_base_merges_with_a_transparent_hole() {
    // `[BG, Base, C1(clipped), C2(clipped, Deep fill 0)]`, Merge Layers
    // {C1, C2}. C1's base is OUTSIDE the set, so the merged pair is baseless
    // in its private level and would read the LEVEL window where the
    // projection gave it the base-only snapshot. `merge_wants_backdrop`
    // excludes the set and the merge keeps `NONE` — which is what today's
    // code does, and what this asserts: the merged record's own pixels carry
    // a TRANSPARENT hole.
    let dir = TempDir::new().unwrap();
    let mut doc = scene(&dir, BG);
    doc = over(&dir, "base.png", doc, 0, MID, "Base");
    doc = over(&dir, "c1.png", doc, 1, EXTRA, "C1");
    doc = set_clipped(doc, 2);
    doc = over(&dir, "c2.png", doc, 2, HIGH, "C2");
    doc = set_clipped(doc, 3);
    doc = set_style(doc, 3, &knockout_style("deep", 0.0));
    let idx = [2usize, 3];
    let merged = apply(doc, |d| unsafe {
        dr_doc_merge_layers(d, idx.as_ptr(), idx.len())
    });
    let plate = layer_pixels(merged, 2);
    assert_eq!(
        pixel(&plate, W, 4, 4)[3],
        0,
        "the excluded set bakes the hole as transparency, exactly as today"
    );
    unsafe { dr_doc_free(merged) };

    // The TYPED twin, which is `merge_wants_backdrop16`'s exclusion clause's
    // one DIRECT test: were the clause dropped, `level_backdrop` would hand
    // this set the root level's Background snapshot and the merged record's
    // hole would carry BG's opaque pixels instead of transparency.
    let doc = tdoc(vec![
        tlayer("BG", BG),
        tlayer("Base", MID),
        tlayer("C1", EXTRA).with_clipped(true),
        tstyled(tlayer("C2", HIGH), &knockout_style("deep", 0.0)).with_clipped(true),
    ]);
    let merged = tedit(&doc, json!({"op":"merge_layers","layers":[2,3]})).unwrap();
    let plate = merged
        .layer_source(2)
        .expect("the merged record is a raster")
        .to_rgba16()
        .expect("a u16 merge");
    assert_eq!(
        plate[(4 * W + 4) as usize][3],
        0,
        "the excluded set bakes the hole as transparency at 16 bits too"
    );
}

#[test]
fn a_merge_with_no_knockout_member_composites_the_plain_source_over() {
    // `merge_wants_backdrop` is false, so `MergeBackdrop::none()` is returned
    // and the merge executes today's statements. The observable is the plain
    // W3C source-over of the two opaque fills, computed here independently.
    let dir = TempDir::new().unwrap();
    let mut doc = scene(&dir, BG);
    doc = over(&dir, "a.png", doc, 0, MID, "MidA");
    doc = over(&dir, "b.png", doc, 1, EXTRA, "MidB");
    let merged = apply(doc, |d| unsafe { dr_doc_merging_down(d, 2) });
    let plate = layer_pixels(merged, 1);
    // Both fills are opaque, so the source-over is the upper one verbatim.
    assert_eq!(pixel(&plate, W, 4, 4), EXTRA, "a plain merge is unchanged");
    unsafe { dr_doc_free(merged) };

    // The TYPED twin: `merge_wants_backdrop16`'s FIRST clause. No member
    // carries a knockout, so the seed is `Handed { None, None }` and the
    // derived document renders exactly what it rendered before 12E-3b.
    let doc = tdoc(vec![
        tlayer("BG", BG),
        tlayer("MidA", MID),
        tlayer("MidB", EXTRA),
    ]);
    let merged = tedit(&doc, json!({"op":"merge_down","layer":2})).unwrap();
    let plate = merged
        .layer_source(1)
        .expect("the merged record is a raster")
        .to_rgba16()
        .expect("a u16 merge");
    assert_eq!(
        plate[(4 * W + 4) as usize],
        promoted(EXTRA),
        "a plain merge is unchanged at 16 bits"
    );
}

// ------------------------------------ a clipped group that renders nothing --
//
// The Merge Layers half of 12E-3b decision (ix) was pinned here: D6's "visible
// clipped members" read the DOCUMENT's `visible` flags, so a visible clipped
// group that renders nothing still turned the flag OFF. P7-39 lifted that
// narrowing — the flag no longer asks about members at all — so what is
// pinned now is that such a group changes nothing: the merge with G visible
// is the merge with G hidden, at both depths. (A grouped-interior merge
// bakes the unit against the merge's own transparent accumulator, so it is
// not the projection; that divergence is the merge's, outside P7-39.) Fill
// is 0.5 because at fill 1 a Normal overlay makes D6 unobservable over the
// merge's transparent accumulator, and the test would prove nothing.

const WHITE: [u8; 4] = [255, 255, 255, 255];
const MULTIPLY: i32 = 1;

fn set_visible(doc: *mut DrDocument, idx: usize, visible: bool) -> *mut DrDocument {
    apply_or_keep(doc, |d| unsafe {
        dr_doc_with_layer_visible(d, idx, visible)
    })
}

fn d6_style() -> String {
    "{\"blend_interior_as_group\":true,\"fill_opacity\":0.5,\"effects\":[\
     {\"type\":\"color_overlay\",\"color\":\"#00ff00\",\"opacity\":1.0}]}"
        .to_string()
}

/// `[BG, Base(Multiply, fill 0.5, colour overlay, grouped interior), G(clipped,
/// visible)]`. With `hidden_child` G keeps one HIDDEN child `X` (indices
/// `[BG 0, Base 1, X 2, G 3]`); otherwise G is EMPTY (`[BG 0, Base 1, G 2]`).
/// Either way G renders nothing. Answers the document and G's index.
fn d6_fixture(dir: &TempDir, hidden_child: bool) -> (*mut DrDocument, usize) {
    let mut doc = scene(dir, BG);
    doc = over(dir, "base.png", doc, 0, WHITE, "Base");
    doc = set_blend(doc, 1, MULTIPLY);
    doc = set_style(doc, 1, &d6_style());
    doc = over(dir, "x.png", doc, 1, HIGH, "X");
    let (mut doc, mut group) = group_many(doc, &[2], "G");
    if hidden_child {
        doc = set_visible(doc, 2, false);
    } else {
        doc = apply(doc, |d| unsafe {
            dr_doc_remove_layers(d, [2usize].as_ptr(), 1)
        });
        group = 2;
    }
    (set_clipped(doc, group), group)
}

#[test]
fn a_visible_clipped_group_that_renders_nothing_merges_as_if_hidden_at_8_bits() {
    let dir = TempDir::new().unwrap();
    for hidden_child in [false, true] {
        let merged = |visible: bool| {
            let (doc, group) = d6_fixture(&dir, hidden_child);
            let doc = set_visible(doc, group, visible);
            let set = [1usize, group];
            let out = apply(doc, |d| unsafe {
                dr_doc_merge_layers(d, set.as_ptr(), set.len())
            });
            let pixels = flat_pixels(out);
            unsafe { dr_doc_free(out) };
            pixels
        };
        assert_eq!(
            merged(true),
            merged(false),
            "hidden child {hidden_child}: G renders nothing and D6 no longer reads it"
        );
    }
}

#[test]
fn a_visible_clipped_group_that_renders_nothing_merges_as_if_hidden_at_16_bits() {
    let build = |visible: bool| {
        let base = tstyled(
            tlayer("Base", WHITE)
                .with_blend_mode(BlendMode::Multiply)
                .unwrap(),
            &d6_style(),
        );
        let group = TLayer::isolated_normal("G", vec![], 1.0, visible)
            .unwrap()
            .with_clipped(true);
        tdoc(vec![tlayer("BG", BG), base, group])
    };
    let merged = |visible: bool| {
        trender(&tedit(&build(visible), json!({"op":"merge_layers","layers":[1,2]})).unwrap())
    };
    assert_eq!(
        trender(&build(true)),
        trender(&build(false)),
        "the projection"
    );
    assert_eq!(merged(true), merged(false), "the merge");
}

#[test]
fn a_visible_empty_group_as_a_clip_base_merges_exactly_at_8_bits() {
    // The new path: a visible empty group can be a clip BASE in the
    // flat level, so it reaches the flat walk instead of being skipped as
    // invisible. `[BG, G(empty, visible), R(clipped)]`, merged `{G, R}`.
    let dir = TempDir::new().unwrap();
    let build = || {
        let mut doc = scene(&dir, BG);
        doc = over(&dir, "x.png", doc, 0, HIGH, "X");
        let (doc, _) = group_many(doc, &[1], "G");
        let mut doc = apply(doc, |d| unsafe {
            dr_doc_remove_layers(d, [1usize].as_ptr(), 1)
        });
        doc = over(&dir, "r.png", doc, 1, MID, "R");
        set_clipped(doc, 2)
    };
    let set = [1usize, 2];
    byte_exact("empty group base", build, |d| unsafe {
        dr_doc_merge_layers(d, set.as_ptr(), set.len())
    });
}

// ------------------------------------------------------- the residual --

#[test]
fn a_knockout_merge_inside_an_isolated_group_over_a_sibling_is_not_exact() {
    // Decision (vi), stated rather than smoothed over. `[BG, G(Normal){S0
    // opaque, L, K Deep fill 0}]`: inside an ISOLATED group `B0` is always
    // transparent (D4's hard stop), so the hole is transparency in the merged
    // record too — and S0, the visible sibling beneath the merged pair, then
    // shows where the GROUP's own transparency used to let BG through.
    let dir = TempDir::new().unwrap();
    let build = || {
        let mut doc = scene(&dir, BG);
        doc = over(&dir, "s0.png", doc, 0, MID, "S0");
        doc = over(&dir, "l.png", doc, 1, EXTRA, "L");
        doc = over(&dir, "k.png", doc, 2, HIGH, "K");
        doc = set_style(doc, 3, &knockout_style("deep", 0.0));
        let (doc, group) = group_many(doc, &[1, 2, 3], "G");
        set_blend(doc, group, NORMAL)
    };
    let before_doc = build();
    let before = pixel(&flat_pixels(before_doc), W, 4, 4);
    unsafe { dr_doc_free(before_doc) };
    let merged = apply(build(), |d| unsafe { dr_doc_merging_down(d, 3) });
    let after = pixel(&flat_pixels(merged), W, 4, 4);
    unsafe { dr_doc_free(merged) };
    assert_eq!(
        before, BG,
        "before: the hole shows what lies beneath the GROUP"
    );
    assert_eq!(
        after, MID,
        "after: it shows the sibling beneath the merged pair"
    );
    assert_ne!(before, after, "the residual is real and is disclosed");
}
