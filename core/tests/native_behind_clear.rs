//! Behind and Clear on typed documents (`paint_mode`'s rule), black-box
//! through the request API: `paint_blend` (the Brush and Fill…) and
//! `stroke_layer_edge` (Stroke…) on a 16-bit RGB layer against the rule's
//! formulas written here in f64; the frozen alpha; Grayscale and Lab, where
//! Behind over an alpha-0 pixel is the Normal paint there and the opaque and
//! colour-exact cases hold; and the refusals by name — the Paint Bucket and
//! the Gradient, CMYK, Indexed and Bitmap.

use std::ffi::CString;
use std::ptr;

use doctored_core::doc_lock::LOCK_TRANSPARENCY;
use doctored_core::ffi_lab::dr_precision_doc_convert_to_lab;
use doctored_core::icc::IccProfile;
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer};
use doctored_core::precision_image::PrecisionImage;
use serde_json::{json, Value};

use crate::bitmap_mode::{gray_doc, to_bitmap};
use crate::cmyk_structure_fixture::{cmyk_layers, RgbLayer};
use crate::common::take_err_string;
use crate::indexed_fixture::{free, layer16, paint, rgb_doc, to_indexed, LIMIT};

const BEHIND: u32 = 28;
const CLEAR: u32 = 29;
const INK: [f64; 3] = [0.8, 0.3, 0.1];
const OPACITY: f64 = 0.6;
const W: u32 = 16;
const H: u32 = 10;

/// Alphas 0, 65535 and partial, with colour under alpha 0 too.
fn samples() -> Vec<[u16; 4]> {
    (0..W * H)
        .map(|i| {
            let a = match i % 5 {
                0 => 0,
                1 => 65535,
                k => (k * 13001 + i * 97) as u16,
            };
            [(i * 4099) as u16, (i * 911) as u16, 30000, a]
        })
        .collect()
}

fn coverage() -> Vec<u16> {
    (0..W * H)
        .map(|i| match i % 7 {
            0 => 0,
            1 => 65535,
            k => (k * 8191 + i * 13) as u16,
        })
        .collect()
}

/// A typed sRGB document of one `size` layer of `pixels` at `offset`.
fn typed(
    canvas: (u32, u32),
    pixels: Vec<[u16; 4]>,
    size: (u32, u32),
    offset: (i32, i32),
) -> *mut PrecisionDocument {
    let image =
        PrecisionImage::from_rgba16(size.0, size.1, pixels, IccProfile::srgb(), None).unwrap();
    let layer = PrecisionDocumentLayer::raster("Layer", image, offset, 1., true).unwrap();
    let doc = PrecisionDocument::from_parts(
        canvas.0,
        canvas.1,
        IccProfile::srgb(),
        Default::default(),
        Default::default(),
        vec![layer],
    )
    .unwrap();
    Box::into_raw(Box::new(doc))
}

fn blend(mode: u32, extra: Value) -> Value {
    let mut request = json!({"op":"paint_blend","layer":0,"color":[INK[0],INK[1],INK[2],1.0],
        "color_space":"document","opacity":OPACITY,"blend_mode":mode});
    for (key, value) in extra.as_object().unwrap() {
        request[key] = value.clone();
    }
    request
}

fn painted(
    doc: *const PrecisionDocument,
    request: &Value,
    input: &[u16],
) -> Option<*mut PrecisionDocument> {
    paint(doc, request, input).unwrap_or_else(|e| panic!("{request}: {e}"))
}

fn refusal(doc: *const PrecisionDocument, request: &Value, input: &[u16]) -> String {
    match paint(doc, request, input) {
        Err(e) => e,
        Ok(_) => panic!("{request} was expected to refuse"),
    }
}

fn geometry(doc: *const PrecisionDocument) -> ((u32, u32), (i32, i32)) {
    let layer = unsafe { &*doc }.layer(0).unwrap();
    (layer.source().unwrap().dimensions(), layer.offset())
}

/// The rule in f64 on unit samples: Behind or Clear of `b` at `sa`.
fn rule(mode: u32, b: [u16; 4], cs: [f64; 3], sa: f64) -> [f64; 4] {
    let unit = b.map(|v| f64::from(v) / 65535.0);
    let ab = unit[3];
    if mode == CLEAR {
        return [unit[0], unit[1], unit[2], ab * (1.0 - sa)];
    }
    let ao = ab + sa * (1.0 - ab);
    let mut out = [0.0; 4];
    for c in 0..3 {
        out[c] = (unit[c] * ab + cs[c] * sa * (1.0 - ab)) / ao;
    }
    out[3] = ao;
    out
}

fn assert_rule(
    mode: u32,
    before: &[[u16; 4]],
    after: &[[u16; 4]],
    sa: impl Fn(usize) -> f64,
    what: &str,
) {
    let mut moved = 0;
    for (i, (b, a)) in before.iter().zip(after).enumerate() {
        let s = sa(i);
        if s <= 0.0 || (mode == BEHIND && b[3] == 65535) {
            assert_eq!(a, b, "{what}: sample {i} is untouched");
            continue;
        }
        if mode == CLEAR {
            assert_eq!(a[..3], b[..3], "{what}: Clear writes no colour at {i}");
        }
        let want = rule(mode, *b, INK, s);
        if mode == BEHIND && b[3] == 0 && want[3] * 65535.0 < 0.5 {
            // Sub-quantum: no storage is created for an invisible sample.
            continue;
        }
        for c in 0..4 {
            assert!(
                (f64::from(a[c]) - want[c] * 65535.0).abs() <= 1.0,
                "{what}: sample {i} channel {c}: {a:?} vs {want:?}"
            );
        }
        moved += usize::from(a != b);
    }
    assert!(moved > 20, "{what}: {moved} samples moved");
}

#[test]
fn rgb16_paint_blend_follows_the_rule() {
    let pixels = samples();
    let doc = typed((W, H), pixels.clone(), (W, H), (0, 0));
    let input = coverage();
    let sa = |i: usize| f64::from(input[i]) / 65535.0 * OPACITY;
    for (mode, name) in [(BEHIND, "Behind"), (CLEAR, "Clear")] {
        let out =
            painted(doc, &blend(mode, json!({})), &input).expect("the paint changes the layer");
        assert_rule(mode, &pixels, &layer16(out), sa, name);
        free(out);
        // With a selection the second plane multiplies the first.
        let mut both = vec![65535u16; (W * H) as usize];
        both.extend(&input);
        let selected = painted(doc, &blend(mode, json!({"selection": true})), &both).unwrap();
        assert_rule(
            mode,
            &pixels,
            &layer16(selected),
            sa,
            &format!("{name}, selected"),
        );
        free(selected);
    }
    free(doc);
}

#[test]
fn behind_grows_a_trimmed_layer_and_clear_never_does() {
    let pixels = samples();
    let doc = typed((W + 8, H + 6), pixels, (W, H), (4, 3));
    let all = vec![65535u16; ((W + 8) * (H + 6)) as usize];
    let behind = painted(doc, &blend(BEHIND, json!({})), &all).unwrap();
    assert_eq!(
        geometry(behind),
        ((W + 8, H + 6), (0, 0)),
        "Behind paints the empty canvas"
    );
    let clear = painted(doc, &blend(CLEAR, json!({})), &all).unwrap();
    assert_eq!(geometry(clear), ((W, H), (4, 3)), "Clear keeps the storage");
    let edge = |mode: u32| {
        json!({"op":"stroke_layer_edge","layer":0,"width":3.0,"location":"center",
            "color":[INK[0],INK[1],INK[2],1.0],"color_space":"document","opacity":1.0,
            "blend_mode":mode})
    };
    let stroked = painted(doc, &edge(CLEAR), &[]).expect("Clear takes the band's alpha off");
    assert_eq!(
        geometry(stroked),
        ((W, H), (4, 3)),
        "a Clear stroke never grows"
    );
    let grown = painted(doc, &edge(BEHIND), &[]).expect("Behind paints the band");
    assert_ne!(
        geometry(grown),
        ((W, H), (4, 3)),
        "a Behind stroke grows like Normal"
    );
    for out in [behind, clear, stroked, grown, doc] {
        free(out);
    }
}

#[test]
fn a_frozen_alpha_changes_nothing() {
    let doc = typed((W, H), samples(), (W, H), (0, 0));
    let locked = Box::into_raw(Box::new(
        unsafe { &*doc }
            .with_layer_locks(0, LOCK_TRANSPARENCY)
            .unwrap()
            .unwrap(),
    ));
    let input = coverage();
    for mode in [BEHIND, CLEAR] {
        let free_out = painted(doc, &blend(mode, json!({})), &input);
        assert!(free_out.is_some(), "{mode}: the free layer changes");
        free_out.into_iter().for_each(free);
        assert!(painted(doc, &blend(mode, json!({"preserve_alpha": true})), &input).is_none());
        assert!(painted(locked, &blend(mode, json!({})), &input).is_none());
    }
    free(locked);
    free(doc);
}

#[test]
fn the_bucket_and_the_gradient_refuse_by_name() {
    let doc = rgb_doc(4, 4, &[[10, 20, 30, 128]; 16]);
    let all = vec![65535u16; 16];
    for (mode, name) in [(BEHIND, "Behind"), (CLEAR, "Clear")] {
        let bucket = json!({"op":"bucket_fill","layer":0,"color":[1,0,0,1],"color_space":"document",
            "opacity":1.0,"blend_mode":mode,"x":1,"y":1,"tolerance":0,"contiguous":true});
        let want = format!("The {name} blend mode applies to the Brush, Fill and Stroke only");
        assert_eq!(refusal(doc, &bucket, &all), want);
        let gradient = json!({"op":"gradient","layer":0,"start":[1,0,0,1],"end":[0,0,1,1],
            "color_space":"document","opacity":1.0,"blend_mode":mode,"from":[0,0],"to":[4,4],
            "radial":false});
        assert_eq!(refusal(doc, &gradient, &all), want);
    }
    free(doc);
}

/// Clear then Behind on a mode's document: Clear keeps the colour exactly
/// and lowers alpha; Behind over a now-transparent sample is the Normal
/// paint there, and leaves an opaque sample alone.
fn assert_mode_twin(doc: *const PrecisionDocument, what: &str) {
    let (w, h) = unsafe { &*doc }.dimensions();
    let n = (w * h) as usize;
    let half: Vec<u16> = (0..n).map(|i| if i % 2 == 0 { 65535 } else { 0 }).collect();
    let before = layer16(doc);
    let cleared = painted(doc, &blend(CLEAR, json!({"opacity": 1.0})), &half).expect(what);
    let after = layer16(cleared);
    for i in 0..n {
        assert_eq!(
            after[i][..3],
            before[i][..3],
            "{what}: Clear keeps colour at {i}"
        );
        let want = if i % 2 == 0 { 0 } else { before[i][3] };
        assert_eq!(after[i][3], want, "{what}: Clear's alpha at {i}");
    }
    let all = vec![65535u16; n];
    let behind = painted(cleared, &blend(BEHIND, json!({})), &all).expect(what);
    let normal = painted(cleared, &blend(0, json!({})), &all).expect(what);
    let (b, m) = (layer16(behind), layer16(normal));
    for i in 0..n {
        if i % 2 == 0 {
            assert_eq!(b[i], m[i], "{what}: Behind over alpha 0 is Normal at {i}");
        } else {
            assert_eq!(b[i], after[i], "{what}: Behind leaves opaque sample {i}");
        }
    }
    for out in [cleared, behind, normal] {
        free(out);
    }
}

#[test]
fn grayscale_and_lab_take_both_modes() {
    let gray = gray_doc(6, 4, &[30, 90, 150, 210, 250, 5].repeat(4));
    assert_mode_twin(gray, "Grayscale");
    free(gray);
    let rgb = rgb_doc(6, 4, &[[120, 60, 30, 255], [10, 200, 90, 255]].repeat(12));
    let options = CString::new(r#"{"depth":16}"#).unwrap();
    let mut err = ptr::null_mut();
    let lab = unsafe {
        dr_precision_doc_convert_to_lab(rgb, options.as_ptr(), LIMIT, 0, ptr::null(), &mut err)
    };
    assert!(!lab.is_null(), "to Lab: {}", take_err_string(err));
    assert_mode_twin(lab, "Lab");
    free(lab);
    free(rgb);
}

#[test]
fn cmyk_indexed_and_bitmap_refuse_by_name() {
    let all = vec![65535u16; 40];
    let cmyk = cmyk_layers(
        8,
        5,
        vec![RgbLayer {
            name: "Layer",
            w: 8,
            h: 5,
            offset: (0, 0),
            pixels: vec![[30000, 20000, 10000, 40000]; 40],
        }],
    );
    for (mode, name) in [(BEHIND, "Behind"), (CLEAR, "Clear")] {
        let ink = json!({"op":"paint_blend","layer":0,"ink":[0,0,0,1,1],"opacity":1.0,
            "blend_mode":mode});
        let want = format!("The {name} blend mode is unavailable in a CMYK document");
        assert_eq!(refusal(cmyk, &ink, &all), want);
        let edge = json!({"op":"stroke_layer_edge","layer":0,"width":2.0,"location":"center",
            "ink":[0,0,0,1,1],"opacity":1.0,"blend_mode":mode});
        assert_eq!(refusal(cmyk, &edge, &[]), want);
    }
    free(cmyk);
    let rgb = rgb_doc(8, 5, &[[255, 0, 0, 255]; 40]);
    let indexed = to_indexed(rgb, &json!({"palette": "exact", "dither": "none"})).unwrap();
    free(rgb);
    let request = blend(BEHIND, json!({}));
    let said = refusal(indexed, &request, &all);
    assert!(
        said.starts_with("The Behind blend mode is unavailable in an Indexed Color document"),
        "{said}"
    );
    free(indexed);
    let gray = gray_doc(8, 5, &[100; 40]);
    let bitmap = to_bitmap(gray, &json!({"method": "threshold"})).unwrap();
    free(gray);
    // Bitmap's gate admits Normal, Darken and Lighten only, and says so.
    let said = refusal(bitmap, &blend(CLEAR, json!({})), &all);
    assert!(
        said.contains("paint in Normal, Darken or Lighten mode in a Bitmap document"),
        "{said}"
    );
    free(bitmap);
}
