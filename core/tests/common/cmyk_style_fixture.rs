//! The CMYK layer-style files' shared fixtures (#1, the styles batch:
//! `cmyk_styles.rs`, `cmyk_blend_if.rs`, `cmyk_knockout.rs`,
//! `cmyk_styles_stack.rs`).
//!
//! Documents are built through the FFI with EXACT ink: `new_cmyk` in the
//! synthetic press profile (`common/cmyk_fixture.rs`), the backdrop painted
//! in columns, each further layer painted with `paint_blend` at a per-pixel
//! coverage (so its alpha is the coverage code exactly). The ORACLES never
//! read the implementation: the f64 W3C step on `L = 1 − ink` is
//! `cmyk_structure_fixture::over`, and an effect's per-pixel COVERAGE is read
//! from an RGB16 TWIN — an sRGB document whose layer has the same alpha, Fill
//! 0, over black, carrying the same effect in white at Normal and opacity 1,
//! so each rendered red code is the contribution's `sa` (the u8 planes are
//! the one shared renderer; what is under test is how the coverage meets the
//! ink).

#![allow(dead_code)]

use std::ffi::CString;
use std::ptr;

use doctored_core::doc::BlendMode;
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer};
use doctored_core::precision_image::PrecisionImage;
use serde_json::{json, Value};

use crate::cmyk_fixture::press_profile;
use crate::cmyk_structure_fixture::{composite, free, over, then, unit, Ink, LIMIT};

/// A `w × h` CMYK document at `depth` whose one opaque layer ("Backdrop")
/// holds `columns[i]` (percent) in equal vertical bands.
pub fn backdrop(w: u32, h: u32, depth: u32, columns: &[[f64; 4]]) -> *mut PrecisionDocument {
    let profile = press_profile("Press Test");
    let request = format!(r#"{{"depth":{depth},"fill":[0,0,0,0,1],"layer_name":"Backdrop"}}"#);
    let doc = PrecisionDocument::new_cmyk(w, h, Some(&profile), &request, LIMIT, 0)
        .unwrap_or_else(|e| panic!("new CMYK: {e}"));
    let mut doc = Box::into_raw(Box::new(doc));
    let band = (w as usize).div_ceil(columns.len());
    for (i, ink) in columns.iter().enumerate() {
        // The fill is paper already.
        if *ink == [0.0; 4] {
            continue;
        }
        let coverage: Vec<u16> = (0..w * h)
            .map(|p| {
                if (p % w) as usize / band == i {
                    65535
                } else {
                    0
                }
            })
            .collect();
        doc = paint(doc, 0, *ink, &coverage);
    }
    doc
}

/// `ink` (percent) painted opaque onto layer `layer` at `coverage` (one u16
/// per canvas pixel), the input freed.
pub fn paint(
    doc: *mut PrecisionDocument,
    layer: usize,
    ink: [f64; 4],
    coverage: &[u16],
) -> *mut PrecisionDocument {
    let request = format!(
        r#"{{"op":"paint_blend","layer":{layer},"ink":[{},{},{},{},1],"opacity":1}}"#,
        ink[0] / 100.0,
        ink[1] / 100.0,
        ink[2] / 100.0,
        ink[3] / 100.0
    );
    then(doc, &request, coverage)
}

/// A new layer above the top named `name`, painted `ink` at `coverage`.
pub fn layer(
    doc: *mut PrecisionDocument,
    name: &str,
    ink: [f64; 4],
    coverage: &[u16],
) -> *mut PrecisionDocument {
    let top = unsafe { &*doc }.layer_count() - 1;
    let doc = then(
        doc,
        &format!(r#"{{"op":"add_layer","name":"{name}","anchor":{top},"position":"above"}}"#),
        &[],
    );
    let index = unsafe { &*doc }.layer_count() - 1;
    paint(doc, index, ink, coverage)
}

/// Layer `index`'s whole style, the input freed.
pub fn styled(doc: *mut PrecisionDocument, index: usize, style: &Value) -> *mut PrecisionDocument {
    then(
        doc,
        &format!(r#"{{"op":"layer_style","version":1,"layer":{index},"style":{style}}}"#),
        &[],
    )
}

/// The canvas coverage of a soft-edged rectangle: 65535 inside
/// `(x0, y0)..(x1, y1)`, a 2-px ramp outside it, 0 beyond.
pub fn soft_rect(w: u32, h: u32, rect: (u32, u32, u32, u32)) -> Vec<u16> {
    let (x0, y0, x1, y1) = rect;
    (0..w * h)
        .map(|p| {
            let (x, y) = (p % w, p / w);
            let dx = if x < x0 {
                x0 - x
            } else if x >= x1 {
                x + 1 - x1
            } else {
                0
            };
            let dy = if y < y0 {
                y0 - y
            } else if y >= y1 {
                y + 1 - y1
            } else {
                0
            };
            match dx.max(dy) {
                0 => 65535,
                1 => 43690,
                2 => 21845,
                _ => 0,
            }
        })
        .collect()
}

/// A hard rectangle's coverage.
pub fn hard_rect(w: u32, h: u32, rect: (u32, u32, u32, u32)) -> Vec<u16> {
    let (x0, y0, x1, y1) = rect;
    (0..w * h)
        .map(|p| {
            let (x, y) = (p % w, p / w);
            if x >= x0 && x < x1 && y >= y0 && y < y1 {
                65535
            } else {
                0
            }
        })
        .collect()
}

/// `effect` (one style effect object) as its twin's: white, Normal, opacity
/// 1, no ink; `side` keeps one bevel side lit (the other at opacity 0).
pub fn twin_effect(effect: &Value, side: Option<&str>) -> Value {
    let mut out = effect.clone();
    let o = out.as_object_mut().unwrap();
    for key in ["ink", "highlight_ink", "shadow_ink"] {
        o.remove(key);
    }
    let bevel = o.get("type") == Some(&json!("bevel_emboss"));
    for key in ["color", "highlight_color", "shadow_color"] {
        if key == "color" || bevel {
            o.insert(key.into(), json!("#ffffff"));
        }
    }
    for key in ["blend", "highlight_blend", "shadow_blend"] {
        o.insert(key.into(), json!("normal"));
    }
    for key in ["opacity", "highlight_opacity", "shadow_opacity"] {
        o.insert(key.into(), json!(1.0));
    }
    if let Some(gradient) = o.get_mut("gradient") {
        // The twin's stops are black → white, so its red is the gradient's
        // parameter `t` (`gradient_t`), not a coverage.
        if let Some(stops) = gradient.get_mut("stops").and_then(Value::as_array_mut) {
            let n = stops.len();
            for (i, stop) in stops.iter_mut().enumerate() {
                let s = stop.as_object_mut().unwrap();
                s.remove("ink");
                let v = if i + 1 == n { "#ffffff" } else { "#000000" };
                s.insert("color".into(), json!(v));
            }
        }
    }
    match side {
        Some("highlight") => {
            o.insert("shadow_opacity".into(), json!(0.0));
        }
        Some("shadow") => {
            o.insert("highlight_opacity".into(), json!(0.0));
        }
        _ => {}
    }
    out
}

/// The RGB16 twin's red plane (module doc): `alpha` (u16 per canvas pixel)
/// at Fill 0 over black, carrying `effect`, with the document light at
/// `light` (angle, altitude).
pub fn twin(w: u32, h: u32, alpha: &[u16], effect: &Value, light: (f32, f32)) -> Vec<f64> {
    let srgb = IccProfile::srgb();
    let black = PrecisionImage::from_rgba16(
        w,
        h,
        vec![[0, 0, 0, 65535]; (w * h) as usize],
        srgb.clone(),
        None,
    )
    .unwrap();
    let pixels: Vec<[u16; 4]> = alpha.iter().map(|&a| [0, 0, 0, a]).collect();
    let shape = PrecisionImage::from_rgba16(w, h, pixels, srgb.clone(), None).unwrap();
    let style = doctored_core::style::LayerStyle::from_json(
        &json!({"fill_opacity": 0.0, "effects": [effect]}).to_string(),
    )
    .unwrap();
    let layers = vec![
        PrecisionDocumentLayer::raster("Black", black, (0, 0), 1.0, true).unwrap(),
        PrecisionDocumentLayer::raster("Shape", shape, (0, 0), 1.0, true)
            .unwrap()
            .with_layer_style(Some(&style), (w, h))
            .unwrap(),
    ];
    let doc = PrecisionDocument::from_parts(
        w,
        h,
        srgb,
        Metadata::default(),
        Resolution::default(),
        layers,
    )
    .unwrap();
    let doc = doc
        .with_global_light(doctored_core::style::GlobalLight {
            angle: light.0,
            altitude: light.1,
        })
        .unwrap()
        .unwrap_or(doc);
    let rendered = doc.render(LIMIT, 0, None).unwrap().to_rgba16().unwrap();
    rendered.iter().map(|p| f64::from(p[0]) / 65535.0).collect()
}

/// A layer's stored straight sample at `(x, y)` in f64 (`None` outside).
pub fn sample(doc: *const PrecisionDocument, index: usize, x: u32, y: u32) -> Option<Ink> {
    let layer = unsafe { &*doc }.layer(index)?;
    let image = layer.cmyk_source()?;
    let (ox, oy) = layer.offset();
    let (px, py) = (i64::from(x) - i64::from(ox), i64::from(y) - i64::from(oy));
    if px < 0 || py < 0 {
        return None;
    }
    image.sample_at(px as u32, py as u32).map(unit)
}

pub fn ink4(ink: Ink) -> [f64; 4] {
    [ink[0], ink[1], ink[2], ink[3]]
}

pub fn percent(ink: [f64; 4]) -> [f64; 4] {
    ink.map(|v| v / 100.0)
}

/// Every pixel's composite against `want(x, y)` (straight, opaque stacks
/// here) within `bound` 16-bit codes per plate and in alpha.
pub fn assert_each(
    doc: *const PrecisionDocument,
    bound: u16,
    what: &str,
    want: impl Fn(u32, u32) -> Ink,
) {
    let (w, h) = unsafe { &*doc }.dimensions();
    for y in 0..h {
        for x in 0..w {
            let got = composite(doc, x as i32, y as i32);
            crate::cmyk_structure_fixture::assert_flat(
                got,
                want(x, y),
                bound,
                &format!("{what} at ({x}, {y})"),
            );
        }
    }
}

/// `over` on a copy.
pub fn over_to(mut dst: Ink, src: [f64; 4], sa: f64, mode: BlendMode) -> Ink {
    over(&mut dst, src, sa, mode);
    dst
}

/// The premultiplied mix `a + (b − a)·t` (`mix_premultiplied`), straight out.
pub fn mix(a: Ink, b: Ink, t: f64) -> Ink {
    let alpha = a[4] + (b[4] - a[4]) * t;
    if alpha <= 0.0 {
        return [0.0; 5];
    }
    let mut out = [0.0; 5];
    for c in 0..4 {
        let v = (a[c] * a[4] + (b[c] * b[4] - a[c] * a[4]) * t) / alpha;
        out[c] = v.clamp(0.0, 1.0);
    }
    out[4] = alpha.min(1.0);
    out
}

/// A raw request through `dr_precision_doc_edit` (`try_edit`), the input kept.
pub fn edit(
    doc: *const PrecisionDocument,
    request: &Value,
) -> Result<*mut PrecisionDocument, String> {
    crate::cmyk_structure_fixture::try_edit(doc, &request.to_string(), &[])
}

/// Frees every handle.
pub fn free_all(docs: &[*mut PrecisionDocument]) {
    for &doc in docs {
        if !doc.is_null() {
            free(doc);
        }
    }
}

pub fn c(text: &str) -> CString {
    CString::new(text).unwrap()
}

pub fn no_err() -> *mut std::ffi::c_char {
    ptr::null_mut()
}

/// The document's composite flattened onto paper (`{"op":"flatten"}`, the
/// one stack), row-major; the input kept.
pub fn flat(doc: *const PrecisionDocument) -> Vec<[u16; 5]> {
    let flattened = crate::cmyk_structure_fixture::try_edit(doc, r#"{"op":"flatten"}"#, &[])
        .unwrap_or_else(|e| panic!("flatten: {e}"));
    assert!(!flattened.is_null(), "flatten changed nothing");
    let samples = unsafe { &*flattened }.cmyk_layer_samples(0).unwrap();
    free(flattened);
    samples
}

/// `got` (a flattened sample: ink on paper) against an opaque `want` within
/// `bound` codes per plate.
pub fn assert_paper(got: [u16; 5], want: Ink, bound: u16, what: &str) {
    for c in 0..4 {
        let w = crate::cmyk_structure_fixture::code(want[c] * want[4]);
        assert!(
            got[c].abs_diff(w) <= bound,
            "{what}: plate {c}: got {got:?}, want {want:?} ({w})"
        );
    }
}

/// Every flattened pixel against `want(x, y)` within `bound`.
pub fn assert_flat_all(
    doc: *const PrecisionDocument,
    bound: u16,
    what: &str,
    want: impl Fn(u32, u32) -> Ink,
) {
    let (w, _) = unsafe { &*doc }.dimensions();
    for (i, got) in flat(doc).into_iter().enumerate() {
        let (x, y) = (i as u32 % w, i as u32 / w);
        assert_paper(got, want(x, y), bound, &format!("{what} at ({x}, {y})"));
    }
}
