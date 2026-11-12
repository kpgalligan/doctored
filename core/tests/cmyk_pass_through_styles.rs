//! Pass Through styles on a CMYK document, at 8 and 16 bits: a layer style or
//! Blend If on a Pass Through group composites the group ISOLATED (take1's
//! P1 and P3), through `cmyk_stack::entry` for a fill-only style and the
//! styled path (`cmyk_style::needs_styled`) for effects and Blend If.
//!
//! The scene is the backdrop in four ink columns under a Pass Through group
//! holding an opaque square and the tests' Levels record
//! (`cmyk_structure_fixture::LEVELS`). The oracles:
//! - TWIN: the same scene with the group Normal and the same style flattens
//!   sample for sample the same;
//! - PLAIN: a backdrop pixel outside the children is Levels of the backdrop
//!   (`cmyk_structure_fixture::levels`) under the unstyled Pass Through group
//!   and the backdrop itself under the styled one.
//!
//! Also: a styled Normal group switches to Pass Through and keeps its style;
//! `.rz` keeps it; the layered PSD writes the group's effects as `lfxs` under
//! its `pass` key and reopens it as a styled Pass Through group.

use std::path::Path;
use std::ptr;

use doctored_core::doc::BlendMode;
use doctored_core::ffi_psd::dr_precision_doc_save_psd;
use doctored_core::precision_document::PrecisionDocument;
use serde_json::Value;
use tempfile::TempDir;

use crate::cmyk_psd_fixture::read;
use crate::cmyk_psd_import::{open_psd, report};
use crate::cmyk_structure_fixture::{
    composite, free, levels, open, save, then, unit, with_levels, with_mode,
};
use crate::cmyk_style_fixture::{backdrop, flat, layer, styled};
use crate::common::{cpath, take_err_string};

const W: u32 = 16;
const H: u32 = 8;
const COLUMNS: [[f64; 4]; 4] = [
    [10.0, 20.0, 30.0, 5.0],
    [40.0, 10.0, 0.0, 20.0],
    [0.0, 60.0, 20.0, 0.0],
    [25.0, 25.0, 25.0, 25.0],
];
/// The square inside the group: x0, y0, x1, y1 (exclusive).
const SQUARE: (u32, u32, u32, u32) = (10, 2, 14, 6);
/// A backdrop pixel clear of the square and of a 2 px stroke around it.
const PLAIN: (i32, i32) = (1, 1);
/// Backdrop 0, square 1, Levels 2, the group 3.
const GROUP: usize = 3;
const PASS: [u8; 4] = *b"pass";

const STROKE_AND_BLEND_IF: &str = r##"{"blend_if":{"channel":"gray","this_layer":[0,0,128,128],"underlying":[0,0,255,255]},"effects":[{"type":"stroke","size":2,"color":"#ff0000"}]}"##;
const BLEND_IF_ONLY: &str =
    r#"{"blend_if":{"channel":"gray","this_layer":[0,0,128,128],"underlying":[0,0,255,255]}}"#;
const FILL_ONLY: &str = r#"{"fill_opacity":0.5}"#;
const STYLES: [&str; 3] = [STROKE_AND_BLEND_IF, BLEND_IF_ONLY, FILL_ONLY];

fn style(text: &str) -> Value {
    serde_json::from_str(text).unwrap()
}

/// The scene at `depth`, its group Pass Through (`pass`) or Normal.
fn scene(depth: u32, pass: bool) -> *mut PrecisionDocument {
    let doc = backdrop(W, H, depth, &COLUMNS);
    let (x0, y0, x1, y1) = SQUARE;
    let coverage: Vec<u16> = (0..W * H)
        .map(|p| {
            let (x, y) = (p % W, p / W);
            if x >= x0 && x < x1 && y >= y0 && y < y1 {
                65535
            } else {
                0
            }
        })
        .collect();
    let doc = layer(doc, "Square", [0.0, 0.0, 0.0, 30.0], &coverage);
    let doc = with_levels(doc, 1.0);
    then(
        doc,
        &format!(r#"{{"op":"group_layers","layers":[1,2],"name":"P","pass_through":{pass}}}"#),
        &[],
    )
}

/// One ink code of `depth`'s storage, in 16-bit units, plus one.
fn bound(depth: u32) -> f64 {
    if depth == 8 {
        2.0 * 257.0
    } else {
        2.0
    }
}

#[test]
fn a_styled_pass_through_group_composites_isolated_on_cmyk() {
    for depth in [8, 16] {
        let plain_doc = backdrop(W, H, depth, &COLUMNS);
        let b = composite(plain_doc, PLAIN.0, PLAIN.1);
        free(plain_doc);
        let unstyled = scene(depth, true);
        assert!(unsafe { &*unstyled }
            .layer(GROUP)
            .unwrap()
            .is_pass_through());
        // The unstyled group's Levels reaches the backdrop.
        let got = unit(composite(unstyled, PLAIN.0, PLAIN.1));
        let want = levels([0, 1, 2, 3].map(|c| unit(b)[c]));
        for c in 0..4 {
            assert!(
                ((got[c] - want[c]) * 65535.0).abs() <= bound(depth),
                "{depth}-bit unstyled plate {c}: {got:?} vs Levels {want:?}"
            );
        }
        free(unstyled);
        for text in STYLES {
            let pass = styled(scene(depth, true), GROUP, &style(text));
            let group = unsafe { &*pass }.layer(GROUP).unwrap();
            assert!(group.is_pass_through(), "{depth} {text}");
            assert!(group.reported_style().is_some(), "{depth} {text}");
            assert_eq!(
                composite(pass, PLAIN.0, PLAIN.1),
                b,
                "{depth}-bit {text}: the backdrop is not adjusted"
            );
            let twin = styled(scene(depth, false), GROUP, &style(text));
            assert_eq!(
                flat(pass),
                flat(twin),
                "{depth}-bit {text}: the isolated twin"
            );
            free(pass);
            free(twin);
        }
    }
}

#[test]
fn a_styled_normal_cmyk_group_switches_to_pass_through_and_keeps_its_style() {
    for text in STYLES {
        let twin = styled(scene(16, false), GROUP, &style(text));
        let pass = with_mode(
            styled(scene(16, false), GROUP, &style(text)),
            GROUP,
            BlendMode::PassThrough.to_c(),
        );
        let group = unsafe { &*pass }.layer(GROUP).unwrap();
        assert!(group.is_pass_through(), "{text}");
        assert_eq!(
            group.reported_style(),
            unsafe { &*twin }.layer(GROUP).unwrap().reported_style(),
            "{text}"
        );
        assert_eq!(flat(pass), flat(twin), "{text}");
        free(pass);
        free(twin);
    }
}

#[test]
fn native_save_keeps_the_styled_cmyk_pass_through_group() {
    let dir = TempDir::new().unwrap();
    for depth in [8, 16] {
        for (i, text) in STYLES.iter().enumerate() {
            let doc = styled(scene(depth, true), GROUP, &style(text));
            let path = dir.path().join(format!("p{depth}_{i}.rz"));
            save(doc, &path);
            let back = open(&path).unwrap();
            let group = unsafe { &*back }.layer(GROUP).unwrap();
            assert!(group.is_pass_through(), "{depth} {text}");
            assert_eq!(
                group.reported_style(),
                unsafe { &*doc }.layer(GROUP).unwrap().reported_style(),
                "{depth} {text}"
            );
            assert_eq!(flat(back), flat(doc), "{depth} {text}");
            free(back);
            free(doc);
        }
    }
}

fn save_psd(doc: *const PrecisionDocument, path: &Path) {
    let mut err = ptr::null_mut();
    let ok = unsafe {
        dr_precision_doc_save_psd(
            doc,
            ptr::null(),
            cpath(path).as_ptr(),
            0,
            0,
            crate::cmyk_structure_fixture::LIMIT,
            0,
            std::ptr::null(),
            &mut err,
        )
    };
    assert!(ok, "{}", take_err_string(err));
}

#[test]
fn the_layered_psd_writes_lfxs_under_pass_and_reopens_the_styled_group() {
    let dir = TempDir::new().unwrap();
    for depth in [8, 16] {
        for (i, text) in STYLES.iter().enumerate() {
            let doc = styled(scene(depth, true), GROUP, &style(text));
            let path = dir.path().join(format!("p{depth}_{i}.psd"));
            save_psd(doc, &path);
            let file = read(&std::fs::read(&path).unwrap());
            let record = file.by_name("P");
            assert_eq!(
                record.blend, PASS,
                "{depth} {text}: the record keeps `pass`"
            );
            assert_eq!(record.divider_blend.unwrap_or(PASS), PASS, "{depth} {text}");
            let effects = text.contains("stroke");
            assert_eq!(record.block(b"lfxs").is_some(), effects, "{depth} {text}");
            assert!(record.block(b"lfx2").is_none(), "{depth} {text}");
            let ranged = text.contains("blend_if");
            assert_eq!(
                record.ranges.chunks_exact(4).any(|v| v != [0, 0, 255, 255]),
                ranged,
                "{depth} {text}: {:?}",
                record.ranges
            );
            let losses = report(&path)["verified_losses"].clone();
            assert!(
                !losses
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|l| l.as_str().unwrap().contains("“P”")),
                "{depth} {text}: {losses}"
            );
            let back = open_psd(&path, 0).unwrap();
            let d = unsafe { &*back };
            let group = (0..d.layer_count())
                .map(|i| d.layer(i).unwrap())
                .find(|l| l.name() == "P")
                .expect("the group");
            assert!(group.is_pass_through(), "{depth} {text}");
            assert!(group.reported_style().is_some(), "{depth} {text}");
            assert_eq!(group.blend_if().is_some(), ranged, "{depth} {text}");
            // The stroke's colour crosses as a `CMYC` ink, so the 16-bit
            // file is held to the 8-bit bound too.
            for (j, (g, w)) in flat(back).iter().zip(flat(doc)).enumerate() {
                for c in 0..5 {
                    assert!(
                        (f64::from(g[c]) - f64::from(w[c])).abs() <= bound(8),
                        "{depth}-bit {text}: pixel {j} plane {c}: {g:?} vs {w:?}"
                    );
                }
            }
            free(back);
            free(doc);
        }
    }
}
