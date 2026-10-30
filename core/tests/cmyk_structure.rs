//! Groups, clipping masks and the DRDC 84 word on a CMYK document (#547
//! phase 4a): Normal and Pass Through groups with masks and opacity, nested;
//! clip units (a Multiply member, a Pass Through base, a base at Fill 0.5)
//! against `common/cmyk_structure_fixture.rs`'s recursive f64 group oracle;
//! the `.rz` round trip of a structured document at word 84 (and plain and
//! masked ones still at 82 and 83); the pre-84 and empty-84 refusals of
//! hand-patched files; and both mode conversions carrying the tree.

use std::ptr;

use doctored_core::doc::BlendMode;
use doctored_core::ffi_cmyk::{
    dr_precision_doc_convert_cmyk_to_rgb, dr_precision_doc_convert_to_cmyk,
};
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer};
use tempfile::TempDir;

use crate::cmyk_fixture::press_profile;
use crate::cmyk_structure_fixture::{
    assert_composite, cmyk_layers, cmyk_word, composite, free, open, ramp, save, then, varied,
    with_fill, with_levels, with_mask, with_mode, with_opacity, RgbLayer, LIMIT,
};
use crate::common::take_err_string;

const W: u32 = 9;
const H: u32 = 7;

fn opaque(mut pixels: Vec<[u16; 4]>) -> Vec<[u16; 4]> {
    for p in &mut pixels {
        p[3] = 65535;
    }
    pixels
}

/// Six raster records: an opaque Bottom, then A–E with every alpha class,
/// B a 6 × 5 rect at (2, 1).
fn six() -> *mut PrecisionDocument {
    let full = |name, seed| RgbLayer {
        name,
        w: W,
        h: H,
        offset: (0, 0),
        pixels: varied(W, H, seed),
    };
    cmyk_layers(
        W,
        H,
        vec![
            RgbLayer {
                pixels: opaque(varied(W, H, 1)),
                ..full("Bottom", 1)
            },
            full("A", 2),
            RgbLayer {
                name: "B",
                w: 6,
                h: 5,
                offset: (2, 1),
                pixels: varied(6, 5, 3),
            },
            full("C", 4),
            full("D", 5),
            full("E", 6),
        ],
    )
}

/// The postorder index of the record named `name`.
fn index(doc: *const PrecisionDocument, name: &str) -> usize {
    let doc = unsafe { &*doc };
    (0..doc.layer_count())
        .find(|&i| doc.layer(i).unwrap().name() == name)
        .unwrap_or_else(|| panic!("no record {name}"))
}

fn group(
    doc: *mut PrecisionDocument,
    names: &[&str],
    name: &str,
    pass_through: bool,
) -> *mut PrecisionDocument {
    let layers: Vec<String> = names.iter().map(|n| index(doc, n).to_string()).collect();
    then(
        doc,
        &format!(
            r#"{{"op":"group_layers","layers":[{}],"name":"{name}","pass_through":{pass_through}}}"#,
            layers.join(",")
        ),
        &[],
    )
}

fn clip(doc: *mut PrecisionDocument, name: &str) -> *mut PrecisionDocument {
    let i = index(doc, name);
    then(
        doc,
        &format!(r#"{{"op":"layer_clipped","layer":{i},"clipped":true}}"#),
        &[],
    )
}

fn mode(doc: *mut PrecisionDocument, name: &str, mode: BlendMode) -> *mut PrecisionDocument {
    let i = index(doc, name);
    with_mode(doc, i, mode.to_c())
}

fn opacity(doc: *mut PrecisionDocument, name: &str, opacity: f32) -> *mut PrecisionDocument {
    let i = index(doc, name);
    with_opacity(doc, i, opacity)
}

fn mask(doc: *mut PrecisionDocument, name: &str, seed: u32) -> *mut PrecisionDocument {
    let i = index(doc, name);
    with_mask(doc, i, &ramp(W, H, seed))
}

fn fill(doc: *mut PrecisionDocument, name: &str, fill: f64) -> *mut PrecisionDocument {
    let i = index(doc, name);
    with_fill(doc, i, fill)
}

/// Normal and Pass Through groups with masks, opacity and modes, nested.
fn grouped() -> *mut PrecisionDocument {
    let mut doc = six();
    doc = group(doc, &["A", "B"], "Normal", false);
    doc = mask(doc, "Normal", 3);
    doc = opacity(doc, "Normal", 0.7);
    doc = mode(doc, "Normal", BlendMode::Screen);
    doc = mode(doc, "B", BlendMode::Overlay);
    doc = group(doc, &["C", "D"], "Pass", true);
    doc = mask(doc, "Pass", 7);
    doc = opacity(doc, "Pass", 0.6);
    doc = mode(doc, "D", BlendMode::Multiply);
    doc = group(doc, &["Normal", "Pass"], "Outer", false);
    opacity(doc, "Outer", 0.9)
}

#[test]
fn groups_with_masks_and_opacity_composite_as_the_oracle() {
    let doc = grouped();
    let outer = unsafe { &*doc }.layer(index(doc, "Outer")).unwrap();
    assert_eq!(outer.children().map(<[_]>::len), Some(2));
    assert!(unsafe { &*doc }
        .layer(index(doc, "Pass"))
        .unwrap()
        .is_pass_through());
    assert_composite(doc, 2, "groups");
    free(doc);
}

#[test]
fn clip_units_composite_as_the_oracle() {
    // A Pass Through base (isolated by its members) with a Multiply member
    // and a half-opacity one.
    let mut doc = six();
    doc = group(doc, &["A", "B"], "Base", true);
    doc = mask(doc, "Base", 11);
    doc = clip(doc, "C");
    doc = mode(doc, "C", BlendMode::Multiply);
    doc = clip(doc, "D");
    doc = opacity(doc, "D", 0.5);
    assert_composite(doc, 2, "pass-through base");
    free(doc);
    // A raster base at Fill 0.5 under a mask: its members show inside its
    // whole alpha, its own pixels at half strength.
    let mut doc = six();
    doc = fill(doc, "A", 0.5);
    doc = mask(doc, "A", 5);
    doc = clip(doc, "B");
    doc = mode(doc, "B", BlendMode::Multiply);
    doc = clip(doc, "C");
    doc = mode(doc, "E", BlendMode::ColorBurn);
    assert_composite(doc, 2, "filled base");
    // A clipped Pass Through group member composites isolated.
    doc = group(doc, &["D", "E"], "Member", true);
    doc = clip(doc, "Member");
    assert_composite(doc, 2, "pass-through member");
    free(doc);
}

/// The structured document the round trip saves: groups, masks, clipping,
/// modes, Fill and a Multiply Levels record.
fn structured() -> *mut PrecisionDocument {
    let mut doc = grouped();
    doc = clip(doc, "E");
    doc = fill(doc, "C", 0.4);
    doc = with_levels(doc, 0.8);
    mode(doc, "Levels", BlendMode::Multiply)
}

fn same_tree(a: &[PrecisionDocumentLayer], b: &[PrecisionDocumentLayer]) {
    assert_eq!(a.len(), b.len());
    for (la, lb) in a.iter().zip(b) {
        assert_eq!(la.name(), lb.name());
        assert_eq!(la.id(), lb.id(), "{}", la.name());
        assert_eq!(la.opacity(), lb.opacity(), "{}", la.name());
        assert_eq!(la.visible(), lb.visible(), "{}", la.name());
        assert_eq!(la.blend_mode(), lb.blend_mode(), "{}", la.name());
        assert_eq!(la.clipped(), lb.clipped(), "{}", la.name());
        assert_eq!(la.is_pass_through(), lb.is_pass_through(), "{}", la.name());
        assert_eq!(
            la.style().map(|s| s.fill_opacity),
            lb.style().map(|s| s.fill_opacity),
            "{}",
            la.name()
        );
        assert_eq!(la.offset(), lb.offset(), "{}", la.name());
        match (la.mask(), lb.mask()) {
            (None, None) => {}
            (Some(ma), Some(mb)) => {
                assert_eq!(ma.plane().samples(), mb.plane().samples(), "{}", la.name());
                assert_eq!(ma.properties().offset, mb.properties().offset);
            }
            _ => panic!("{}: the mask did not round trip", la.name()),
        }
        match (la.cmyk_source(), lb.cmyk_source()) {
            (None, None) => {}
            (Some(ia), Some(ib)) => assert!(ia.same_samples(ib), "{}", la.name()),
            _ => panic!("{}: the pixels did not round trip", la.name()),
        }
        match (la.children(), lb.children()) {
            (None, None) => {}
            (Some(ca), Some(cb)) => same_tree(ca, cb),
            _ => panic!("{}: the group did not round trip", la.name()),
        }
    }
}

#[test]
fn a_structured_document_saves_word_84_and_reopens_bit_exact() {
    let dir = TempDir::new().unwrap();
    let doc = structured();
    let bytes = save(doc, &dir.path().join("structured.rz"));
    assert_eq!(cmyk_word(&bytes), 84);
    let again = open(&dir.path().join("structured.rz")).unwrap();
    let (a, b) = (unsafe { &*doc }, unsafe { &*again });
    same_tree(a.root_layers(), b.root_layers());
    let levels = index(doc, "Levels");
    assert_eq!(
        a.adjustment_description(levels),
        b.adjustment_description(levels)
    );
    for y in 0..H as i32 {
        for x in 0..W as i32 {
            assert_eq!(composite(doc, x, y), composite(again, x, y), "({x}, {y})");
        }
    }
    free(again);
    free(doc);
}

#[test]
fn plain_and_masked_documents_keep_words_82_and_83() {
    let dir = TempDir::new().unwrap();
    let doc = six();
    assert_eq!(cmyk_word(&save(doc, &dir.path().join("plain.rz"))), 82);
    let masked = mask(doc, "C", 2);
    assert_eq!(cmyk_word(&save(masked, &dir.path().join("masked.rz"))), 83);
    free(masked);
}

fn patched(bytes: &[u8], word: u32, dir: &TempDir, name: &str) -> String {
    let mut bytes = bytes.to_vec();
    bytes[8..12].copy_from_slice(&word.to_le_bytes());
    let path = dir.path().join(name);
    std::fs::write(&path, bytes).unwrap();
    match open(&path) {
        Ok(doc) => {
            free(doc);
            panic!("{name} opened")
        }
        Err(e) => e,
    }
}

#[test]
fn pre_84_words_refuse_structure_and_84_must_claim_it() {
    const REFUSAL: &str =
        "a CMYK layer group, clipping mask, blend mode or fill opacity requires DRDC 84";
    let dir = TempDir::new().unwrap();
    // A group read under 83 and under 82.
    let doc = group(six(), &["A", "B"], "G", false);
    let bytes = save(doc, &dir.path().join("group.rz"));
    assert_eq!(cmyk_word(&bytes), 84);
    assert_eq!(patched(&bytes, 83, &dir, "group83.rz"), REFUSAL);
    assert_eq!(patched(&bytes, 82, &dir, "group82.rz"), REFUSAL);
    free(doc);
    // A clipped record, a mode and a Fill, each read under 83.
    for (name, doc) in [
        ("clip", clip(six(), "B")),
        ("mode", mode(six(), "B", BlendMode::Screen)),
        ("fill", fill(six(), "B", 0.5)),
    ] {
        let bytes = save(doc, &dir.path().join(format!("{name}.rz")));
        assert_eq!(cmyk_word(&bytes), 84, "{name}");
        assert_eq!(patched(&bytes, 83, &dir, &format!("{name}83.rz")), REFUSAL);
        free(doc);
    }
    // A plain document claiming 84.
    let doc = six();
    let bytes = save(doc, &dir.path().join("plain.rz"));
    assert_eq!(
        patched(&bytes, 84, &dir, "plain84.rz"),
        "invalid native CMYK structure word"
    );
    free(doc);
}

#[test]
fn both_conversions_carry_a_grouped_document() {
    let doc = grouped();
    let doc = clip(doc, "E");
    let doc = fill(doc, "C", 0.4);
    let mut err = ptr::null_mut();
    let rgb = unsafe {
        dr_precision_doc_convert_cmyk_to_rgb(
            doc,
            ptr::null(),
            0,
            c"{}".as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(!rgb.is_null(), "{}", take_err_string(err));
    let profile = press_profile("Press Test");
    let back = unsafe {
        dr_precision_doc_convert_to_cmyk(
            rgb,
            profile.as_ptr(),
            profile.len(),
            c"{}".as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(!back.is_null(), "{}", take_err_string(err));
    // The tree, ids, modes, clipping, Fill and masks ride both ways.
    fn shape(a: &[PrecisionDocumentLayer], b: &[PrecisionDocumentLayer]) {
        assert_eq!(a.len(), b.len());
        for (la, lb) in a.iter().zip(b) {
            assert_eq!(
                (
                    la.name(),
                    la.id(),
                    la.blend_mode(),
                    la.clipped(),
                    la.is_pass_through()
                ),
                (
                    lb.name(),
                    lb.id(),
                    lb.blend_mode(),
                    lb.clipped(),
                    lb.is_pass_through()
                )
            );
            assert_eq!(la.opacity(), lb.opacity());
            assert_eq!(
                la.style().map(|s| s.fill_opacity),
                lb.style().map(|s| s.fill_opacity)
            );
            assert_eq!(la.mask().is_some(), lb.mask().is_some());
            match (la.children(), lb.children()) {
                (None, None) => {}
                (Some(ca), Some(cb)) => shape(ca, cb),
                _ => panic!("{}: the group did not carry", la.name()),
            }
        }
    }
    shape(
        unsafe { &*doc }.root_layers(),
        unsafe { &*rgb }.root_layers(),
    );
    shape(
        unsafe { &*doc }.root_layers(),
        unsafe { &*back }.root_layers(),
    );
    assert!(unsafe { &*back }.is_cmyk());
    assert_composite(back, 2, "carried back");
    for d in [doc, rgb, back] {
        free(d);
    }
}
