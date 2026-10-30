//! #1 phase 6a-i, type, shape and fill layers on a Grayscale document
//! (`gray_artwork`), through the FFI. The colour oracle is sRGB's published
//! luminance row (`SRGB_Y_ROW`) over the IEC decode, re-encoded by the IEC
//! curve for an sRGB colour and kept linear for a linear one; a custom
//! tile's coloured texel is Gray Gamma 2.2's code of its luminance
//! (`Y^(1/2.2)`). The render oracle is the RGB TWIN: a shape of the greyed
//! colour rendered by the one RGB renderer and published on the twin equals
//! the Gray layer exactly at 16 and 32 bits, and at 8 bits each sample is
//! the twin's on the ×257 grid. A colour glyph (intrinsic RGBA8) published
//! on a 16-bit Gray document stores the Gray Gamma 2.2 code of each coloured
//! pixel's luminance. The `.rz` round trip keeps the layer, its description,
//! a path and a vector mask. Phase 6a-ii: RGB → Gray of a document with a
//! shape layer has no blocker and keeps the layer (name, id) as its pixels,
//! its description dropped; the twin's render of the greyed colour published
//! over it equals the same render published on a fresh Gray document (the
//! oracle above) sample for sample at 16 bits, and on the ×257 grid at 8.
//! Bounds declared before measuring.

use std::ffi::{c_char, CStr, CString};
use std::ptr;

use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_artwork::{
    dr_artwork_path_coverage, dr_artwork_render, dr_artwork_render_float, dr_precision_doc_artwork,
    dr_precision_doc_artwork_description,
};
use doctored_core::ffi_gray::{
    dr_precision_doc_convert_to_gray, dr_precision_doc_convert_to_gray_plan,
    dr_precision_doc_gray_artwork_colors,
};
use doctored_core::ffi_path::{dr_precision_doc_paths_json, dr_precision_doc_with_paths_json};
use doctored_core::ffi_precision::{dr_precision_doc_open, dr_precision_doc_save_native};
use doctored_core::ffi_vector_mask::{
    dr_precision_doc_set_vector_mask, dr_precision_doc_vector_mask_json,
};
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::PrecisionDocument;
use doctored_core::precision_image::PrecisionImage;
use serde_json::{json, Value};
use tempfile::TempDir;

use crate::common::{cpath, take_err_string};
use crate::gray_fixture::{srgb_decode, srgb_encode, SRGB_Y_ROW};
use crate::gray_ops::{assert_neutral, free, gray_doc, layer, twin, LIMIT};

const W: u32 = 16;
const H: u32 = 12;
/// Gray Gamma 2.2's exponent.
const GAMMA: f64 = 2.2;
const SHAPE: &str = r#"{"type":"shape","version":4,"artwork":{"ink":"srgb"}}"#;

fn owned(p: *mut c_char) -> Option<String> {
    if p.is_null() {
        return None;
    }
    let text = unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned();
    unsafe { dr_string_free(p) };
    Some(text)
}

/// `dr_precision_doc_gray_artwork_colors`' answer, or its refusal.
fn colors(doc: *const PrecisionDocument, request: &str) -> Result<Value, String> {
    let text = CString::new(request).unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe { dr_precision_doc_gray_artwork_colors(doc, text.as_ptr(), &mut err) };
    match owned(out) {
        Some(answer) => {
            assert!(err.is_null());
            Ok(serde_json::from_str(&answer).unwrap())
        }
        None => Err(take_err_string(err)),
    }
}

/// Standard base64 (RFC 4648, padded), written here.
fn base64(bytes: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(T[((n >> (18 - 6 * i)) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// Its inverse.
fn unbase64(text: &str) -> Vec<u8> {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let digits: Vec<u32> = text
        .bytes()
        .filter(|b| *b != b'=')
        .map(|b| T.iter().position(|t| *t == b).unwrap() as u32)
        .collect();
    let mut out = Vec::new();
    for chunk in digits.chunks(4) {
        let mut n = 0u32;
        for (i, d) in chunk.iter().enumerate() {
            n |= d << (18 - 6 * i);
        }
        for i in 0..chunk.len() - 1 {
            out.push((n >> (16 - 8 * i)) as u8);
        }
    }
    out
}

/// The luminance of an sRGB colour (0…1) by the published row.
fn luminance(rgb: [f64; 3]) -> f64 {
    (0..3).map(|k| SRGB_Y_ROW[k] * srgb_decode(rgb[k])).sum()
}

fn triple(value: &Value) -> [f64; 3] {
    let list = value.as_array().unwrap();
    std::array::from_fn(|k| list[k].as_f64().unwrap())
}

#[test]
fn the_colours_grey_by_the_mode_rule_and_neutral_ones_stay() {
    let gray = gray_doc(W, H, 16);
    let tile = [255u8, 0, 0, 255, 128, 128, 128, 200];
    let request = json!({
        "srgb": [[1.0, 0.0, 0.0], [0.2, 0.6, 0.9], [0.5, 0.5, 0.5]],
        "linear": [[1.0, 0.0, 0.0], [0.3, 0.3, 0.3]],
        "pattern": {"name": "Tile", "width": 2, "height": 1, "rgba": base64(&tile)},
    });
    let answer = colors(gray, &request.to_string()).unwrap();
    let srgb = answer["srgb"].as_array().unwrap();
    for (got, rgb) in srgb.iter().zip([[1.0, 0.0, 0.0], [0.2, 0.6, 0.9]]) {
        let want = srgb_encode(luminance(rgb));
        let got = triple(got);
        assert!(got[0] == got[1] && got[1] == got[2], "{got:?}");
        assert!((got[0] - want).abs() < 1e-4, "{rgb:?}: {got:?} vs {want}");
    }
    assert_eq!(triple(&srgb[2]), [0.5; 3]);
    let linear = answer["linear"].as_array().unwrap();
    let red = triple(&linear[0]);
    assert!(red[0] == red[1] && red[1] == red[2], "{red:?}");
    assert!((red[0] - SRGB_Y_ROW[0]).abs() < 1e-4, "{red:?}");
    assert_eq!(triple(&linear[1]), [0.3; 3]);
    // The tile: the red texel is Gray Gamma 2.2's code of its luminance,
    // the gray one verbatim, alpha kept.
    let texels = unbase64(answer["pattern"]["rgba"].as_str().unwrap());
    let want = (255.0 * SRGB_Y_ROW[0].powf(1.0 / GAMMA)).round();
    assert!(
        texels[0] == texels[1] && texels[1] == texels[2],
        "{texels:?}"
    );
    assert!((f64::from(texels[0]) - want).abs() <= 1.0, "{texels:?}");
    assert_eq!(texels[3], 255);
    assert_eq!(&texels[4..], &[128, 128, 128, 200]);
    assert_eq!(answer["pattern"]["width"], 2);
    // A preset is verbatim; a greyed answer is a fixed point.
    assert_eq!(
        colors(gray, r#"{"pattern":{"preset":"stripes"}}"#).unwrap(),
        json!({"pattern":{"preset":"stripes"}})
    );
    let again =
        json!({"srgb": answer["srgb"], "linear": answer["linear"], "pattern": answer["pattern"]});
    assert_eq!(colors(gray, &again.to_string()).unwrap(), again);
    free(gray);
}

#[test]
fn the_colour_request_refuses_by_name() {
    let gray = gray_doc(W, H, 16);
    let rgb = twin(gray);
    assert_eq!(
        colors(rgb, r#"{"srgb":[[1,0,0]]}"#).unwrap_err(),
        "Grayscale artwork colours need a Grayscale document"
    );
    assert!(colors(gray, "{")
        .unwrap_err()
        .starts_with("invalid Grayscale artwork colour request: "));
    assert_eq!(
        colors(gray, r#"{"cmyk":[]}"#).unwrap_err(),
        "Grayscale artwork colour request has an unknown key \"cmyk\""
    );
    let triples = "Grayscale artwork colours must be at most 1024 [r, g, b] triples of finite numbers (srgb 0–1)";
    let too_many = json!({"srgb": vec![[0.1, 0.2, 0.3]; 1025]}).to_string();
    for bad in [
        r#"{"srgb":[[1.5,0,0]]}"#,
        r#"{"srgb":[[1,0]]}"#,
        r#"{"srgb":"red"}"#,
        r#"{"linear":[[1,"x",0]]}"#,
        too_many.as_str(),
    ] {
        assert_eq!(colors(gray, bad).unwrap_err(), triples, "{bad}");
    }
    assert_eq!(
        colors(
            gray,
            &json!({"linear": vec![[2.0, 0.0, 0.0]; 1024]}).to_string()
        )
        .unwrap()["linear"]
            .as_array()
            .unwrap()
            .len(),
        1024
    );
    assert_eq!(
        colors(gray, r#"{"pattern":{"preset":"nope"}}"#).unwrap_err(),
        "request.pattern.preset must be one of checkerboard, stripes, dots, grid"
    );
    unsafe {
        let text = CString::new("{}").unwrap();
        let mut err = ptr::null_mut();
        assert!(
            dr_precision_doc_gray_artwork_colors(ptr::null(), text.as_ptr(), &mut err).is_null()
        );
        dr_string_free(err);
        let mut err = ptr::null_mut();
        assert!(dr_precision_doc_gray_artwork_colors(gray, ptr::null(), &mut err).is_null());
        dr_string_free(err);
        assert!(
            dr_precision_doc_gray_artwork_colors(ptr::null(), ptr::null(), ptr::null_mut())
                .is_null()
        );
    }
    free(rgb);
    free(gray);
}

/// An anti-aliased quadrilateral's coverage over `W × H`.
fn polygon() -> Vec<u16> {
    let commands = [0u8, 1, 1, 1, 4];
    let points: [[f64; 2]; 4] = [[2.3, 1.7], [13.6, 3.2], [10.4, 9.8], [3.1, 8.4]];
    let mut out = vec![0u16; (W * H) as usize];
    let mut err = ptr::null_mut();
    let ok = unsafe {
        dr_artwork_path_coverage(
            commands.as_ptr(),
            commands.len(),
            points.as_ptr().cast(),
            points.len(),
            W,
            H,
            false,
            out.as_mut_ptr(),
            out.len(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(ok, "{}", take_err_string(err));
    out
}

/// The RGB renderer's image of `components` in `doc`'s own profile.
fn render(
    doc: *const PrecisionDocument,
    float: bool,
    w: u32,
    h: u32,
    components: &Value,
    samples: &[u16],
    intrinsic: &[u8],
) -> *mut PrecisionImage {
    let description = CString::new(components.to_string()).unwrap();
    let profile = unsafe { &*doc }.profile().clone();
    let call = if float {
        dr_artwork_render_float
    } else {
        dr_artwork_render
    };
    let mut err = ptr::null_mut();
    let out = unsafe {
        call(
            w,
            h,
            description.as_ptr(),
            if samples.is_empty() {
                ptr::null()
            } else {
                samples.as_ptr()
            },
            samples.len(),
            if intrinsic.is_empty() {
                ptr::null()
            } else {
                intrinsic.as_ptr()
            },
            intrinsic.len(),
            profile.bytes().as_ptr(),
            profile.bytes().len(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(!out.is_null(), "render: {}", take_err_string(err));
    out
}

/// `image` published as a new artwork layer of `doc` at (0, 0).
fn publish(
    doc: *const PrecisionDocument,
    image: *const PrecisionImage,
    description: &str,
) -> *mut PrecisionDocument {
    let name = CString::new("Shape").unwrap();
    let description = CString::new(description).unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_artwork(
            doc,
            0,
            false,
            image,
            0,
            0,
            name.as_ptr(),
            description.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(!out.is_null(), "publish: {}", take_err_string(err));
    out
}

fn top(doc: *const PrecisionDocument) -> usize {
    unsafe { &*doc }.layer_count() - 1
}

#[test]
fn a_shape_equals_the_twins_render_of_its_greyed_colour() {
    let coverage = polygon();
    for depth in [8, 16, 32] {
        let gray = gray_doc(W, H, depth);
        let rgb = twin(gray);
        let float = depth == 32;
        let colour = if float {
            let answer = colors(gray, r#"{"linear":[[0.9,0.3,0.1]]}"#).unwrap();
            triple(&answer["linear"][0])
        } else {
            let answer = colors(gray, r#"{"srgb":[[0.9,0.3,0.1]]}"#).unwrap();
            triple(&answer["srgb"][0])
        };
        let components = json!([{"kind":"coverage","color":[colour[0],colour[1],colour[2],1.0]}]);
        let image = render(gray, float, W, H, &components, &coverage, &[]);
        let g = publish(gray, image, SHAPE);
        let r = publish(rgb, image, SHAPE);
        let got = layer(g, top(g), depth);
        let want = layer(r, top(r), depth);
        assert_neutral(&got, &format!("{depth}-bit shape"));
        assert_eq!(got.len(), want.len());
        if depth == 8 {
            let mut off_grid = false;
            for (i, (g8, s)) in got.iter().zip(&want).enumerate() {
                off_grid |= s[3] % 257 != 0;
                let snapped = s.map(|v| (v + 128) / 257 * 257);
                assert_eq!(*g8, snapped, "pixel {i}");
                assert!(g8.iter().all(|v| v % 257 == 0), "pixel {i}: {g8:?}");
            }
            assert!(off_grid, "the twin's anti-aliased alpha is off the grid");
        } else {
            assert_eq!(got, want, "{depth}-bit shape");
        }
        unsafe { doctored_core::ffi_precision::dr_precision_image_free(image) };
        for d in [g, r, rgb, gray] {
            free(d);
        }
    }
}

#[test]
fn a_colour_glyph_stores_the_gray_of_each_coloured_pixel() {
    let gray = gray_doc(3, 1, 16);
    let glyph = [255u8, 0, 0, 255, 0, 255, 0, 255, 128, 128, 128, 255];
    let image = render(
        gray,
        false,
        3,
        1,
        &json!([{"kind":"intrinsic_rgba8","opacity":1}]),
        &[],
        &glyph,
    );
    let rendered = unsafe { &*image }.to_rgba16().unwrap();
    let g = publish(gray, image, r#"{"type":"text","version":3,"payload":"A"}"#);
    let got = layer(g, top(g), 16);
    assert_neutral(&got, "glyph");
    for (i, rgb) in [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0]].into_iter().enumerate() {
        let want = (65535.0 * luminance(rgb).powf(1.0 / GAMMA)).round();
        assert!(
            (f64::from(got[i][0]) - want).abs() <= 4.0,
            "pixel {i}: {:?} vs {want}",
            got[i]
        );
        assert_eq!(got[i][3], 65535);
    }
    assert_eq!(got[2], rendered[2].map(u32::from), "the gray pixel is kept");
    unsafe { doctored_core::ffi_precision::dr_precision_image_free(image) };
    free(g);
    free(gray);
}

/// The paths JSON with each path's session id dropped (an open assigns
/// fresh ones).
fn paths(doc: *const PrecisionDocument) -> Option<Value> {
    let text = owned(unsafe { dr_precision_doc_paths_json(doc) })?;
    let mut value: Value = serde_json::from_str(&text).unwrap();
    for path in value["paths"].as_array_mut()? {
        path.as_object_mut()?.remove("id");
    }
    Some(value)
}

fn vector_mask(doc: *const PrecisionDocument, index: usize) -> Option<String> {
    owned(unsafe { dr_precision_doc_vector_mask_json(doc, index) })
}

fn description(doc: *const PrecisionDocument, index: usize) -> Option<String> {
    owned(unsafe { dr_precision_doc_artwork_description(doc, index) })
}

#[test]
fn an_rz_round_trip_keeps_the_layer_its_description_a_path_and_a_vector_mask() {
    let dir = TempDir::new().unwrap();
    let quad = json!({"fill_rule":"nonzero","subpaths":[{"closed":true,
        "anchors":[[2.5,2.0],[12.0,2.5],[13.5,10.5],[3.0,9.0]]}]});
    let gray = gray_doc(W, H, 16);
    let answer = colors(gray, r#"{"srgb":[[0.2,0.6,0.9]]}"#).unwrap();
    let colour = triple(&answer["srgb"][0]);
    let image = render(
        gray,
        false,
        W,
        H,
        &json!([{"kind":"coverage","color":[colour[0],colour[1],colour[2],1.0]}]),
        &polygon(),
        &[],
    );
    let shaped = publish(gray, image, SHAPE);
    let index = top(shaped);
    let mut path = quad.clone();
    path["id"] = json!(0);
    path["name"] = json!("Outline");
    path["kind"] = json!("saved");
    let text = CString::new(json!({"paths":[path]}).to_string()).unwrap();
    let mut err = ptr::null_mut();
    let pathed = unsafe { dr_precision_doc_with_paths_json(shaped, text.as_ptr(), &mut err) };
    assert!(!pathed.is_null(), "paths: {}", take_err_string(err));
    let shape = CString::new(quad.to_string()).unwrap();
    let masked = unsafe {
        dr_precision_doc_set_vector_mask(
            pathed,
            index,
            shape.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(!masked.is_null(), "vector mask: {}", take_err_string(err));
    assert!(unsafe { &*masked }.is_gray());
    let file = dir.path().join("artwork.rz");
    let c = cpath(&file);
    assert!(
        unsafe {
            dr_precision_doc_save_native(masked, c.as_ptr(), LIMIT, 0, ptr::null(), &mut err)
        },
        "{}",
        take_err_string(err)
    );
    let back = unsafe { dr_precision_doc_open(c.as_ptr(), LIMIT, 0, ptr::null(), &mut err) };
    assert!(!back.is_null(), "{}", take_err_string(err));
    assert!(unsafe { &*back }.is_gray());
    assert_eq!(description(back, index).as_deref(), Some(SHAPE));
    assert!(paths(back).is_some());
    assert_eq!(paths(back), paths(masked));
    assert!(vector_mask(back, index).is_some());
    assert_eq!(vector_mask(back, index), vector_mask(masked, index));
    assert_eq!(layer(back, index, 16), layer(masked, index, 16));
    let composite = unsafe { &*back }.render(LIMIT, 0, None).unwrap();
    let composite: Vec<[u32; 4]> = composite
        .to_rgba16()
        .unwrap()
        .iter()
        .map(|p| p.map(u32::from))
        .collect();
    assert_neutral(&composite, "composite");
    unsafe { doctored_core::ffi_precision::dr_precision_image_free(image) };
    for d in [back, masked, pathed, shaped, gray] {
        free(d);
    }
}

/// `image` published over layer `index` of `doc` (`replacing: true`).
fn republish(
    doc: *const PrecisionDocument,
    index: usize,
    image: *const PrecisionImage,
) -> *mut PrecisionDocument {
    let name = CString::new("Shape").unwrap();
    let description = CString::new(SHAPE).unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_artwork(
            doc,
            index,
            true,
            image,
            0,
            0,
            name.as_ptr(),
            description.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(!out.is_null(), "republish: {}", take_err_string(err));
    out
}

#[test]
fn rgb_to_gray_keeps_a_shape_layer_and_its_rerender_equals_a_fresh_gray_one() {
    let coverage = polygon();
    for depth in [8u32, 16] {
        let pixels: Vec<[u16; 4]> = (0..W * H)
            .map(|i| [(i * 300) as u16, (i * 170) as u16, (i * 90) as u16, 65535])
            .collect();
        let image = PrecisionImage::from_rgba16(W, H, pixels, IccProfile::srgb(), None).unwrap();
        let rgb = Box::into_raw(Box::new(
            PrecisionDocument::from_image(image, Metadata::default(), Resolution::default())
                .unwrap(),
        ));
        let red = json!([{"kind":"coverage","color":[0.9,0.3,0.1,1.0]}]);
        let source = render(rgb, false, W, H, &red, &coverage, &[]);
        let shaped = publish(rgb, source, SHAPE);
        let index = top(shaped);
        let id = unsafe { &*shaped }.layer_id(index);
        assert_eq!(description(shaped, index).as_deref(), Some(SHAPE));
        let options =
            CString::new(format!(r#"{{"space":"gray_gamma_22","depth":{depth}}}"#)).unwrap();
        let mut err = ptr::null_mut();
        let plan = owned(unsafe {
            dr_precision_doc_convert_to_gray_plan(
                shaped,
                ptr::null(),
                0,
                options.as_ptr(),
                &mut err,
            )
        })
        .unwrap_or_else(|| panic!("plan: {}", take_err_string(err)));
        let plan: Value = serde_json::from_str(&plan).unwrap();
        assert_eq!(plan["flatten_required"], false, "{plan}");
        assert!(plan["refusal"].is_null(), "{plan}");
        let gray = unsafe {
            dr_precision_doc_convert_to_gray(
                shaped,
                ptr::null(),
                0,
                options.as_ptr(),
                LIMIT,
                0,
                ptr::null(),
                &mut err,
            )
        };
        assert!(!gray.is_null(), "to gray: {}", take_err_string(err));
        let g = unsafe { &*gray };
        assert!(g.is_gray());
        assert_eq!(g.layer(index).unwrap().name(), "Shape");
        assert_eq!(g.layer_id(index), id);
        assert_eq!(description(gray, index), None);
        assert_neutral(&layer(gray, index, depth), "converted shape");
        // The host's re-render: the greyed colour through the twin.
        let answer = colors(gray, r#"{"srgb":[[0.9,0.3,0.1]]}"#).unwrap();
        let colour = triple(&answer["srgb"][0]);
        let components = json!([{"kind":"coverage","color":[colour[0],colour[1],colour[2],1.0]}]);
        let greyed = render(gray, false, W, H, &components, &coverage, &[]);
        let carried = republish(gray, index, greyed);
        let fresh_doc = gray_doc(W, H, depth);
        let fresh = publish(fresh_doc, greyed, SHAPE);
        let got = layer(carried, index, depth);
        assert_eq!(got, layer(fresh, top(fresh), depth), "{depth}-bit carry");
        assert_eq!(description(carried, index).as_deref(), Some(SHAPE));
        if depth == 8 {
            assert!(
                got.iter().all(|p| p.iter().all(|v| v % 257 == 0)),
                "8-bit carry is on the grid"
            );
        }
        for image in [source, greyed] {
            unsafe { doctored_core::ffi_precision::dr_precision_image_free(image) };
        }
        for d in [fresh, fresh_doc, carried, gray, shaped, rgb] {
            free(d);
        }
    }
}
