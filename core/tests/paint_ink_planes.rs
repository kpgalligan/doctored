//! A `paint_blend` with `ink_planes` (the coloured Brush stroke's typed
//! commit, `precision_paint_tools`' module doc), black-box through the C ABI:
//! planes holding one code everywhere are the plain `paint_blend` of that
//! document colour bit for bit (16-bit RGB, Gray and Lab, 32-bit float; with
//! and without a selection); two-colour planes against a W3C Normal source
//! over written here; and each refusal.
use crate::float_fixture::float_doc;
use crate::native_scalar::{error, text, LIMIT};
use doctored_core::ffi_bitmap::dr_precision_doc_convert_to_bitmap;
use doctored_core::ffi_gray::dr_precision_doc_new_gray;
use doctored_core::ffi_indexed::dr_precision_doc_convert_to_indexed;
use doctored_core::ffi_lab::dr_precision_doc_new_lab;
use doctored_core::ffi_precision::dr_precision_doc_free;
use doctored_core::ffi_precision_edit::{dr_precision_doc_edit, dr_precision_doc_edit_plan};
use doctored_core::icc::IccProfile;
use doctored_core::precision_document::PrecisionDocument;
use doctored_core::precision_image::PrecisionImage;
use serde_json::{json, Value};
use std::ffi::{CStr, CString};
use std::ptr;

const W: u32 = 9;
const H: u32 = 7;
const N: usize = (W * H) as usize;

const BLEND: &str =
    "native paint ink planes require Normal blending with no pattern and no plane target";
const MODE: &str = "colour dynamics paints RGB, Grayscale, Duotone and Lab layers; this document's mode takes one ink a stroke";

/// A document the test owns, freed on drop when it came from the C ABI.
enum Doc {
    Owned(Box<PrecisionDocument>),
    Raw(*mut PrecisionDocument),
}

impl Doc {
    fn get(&self) -> &PrecisionDocument {
        match self {
            Doc::Owned(d) => d,
            Doc::Raw(d) => unsafe { &**d },
        }
    }
}

impl Drop for Doc {
    fn drop(&mut self) {
        if let Doc::Raw(d) = self {
            unsafe { dr_precision_doc_free(*d) };
        }
    }
}

fn raw(doc: *mut PrecisionDocument, err: *mut std::ffi::c_char) -> Doc {
    assert!(!doc.is_null(), "{}", unsafe { error(err) });
    Doc::Raw(doc)
}

/// A 16-bit RGB document over a varied, partly transparent backdrop.
fn rgb16() -> Doc {
    let pixels = (0..N as u32)
        .map(|i| {
            let v = (i * 977 % 65536) as u16;
            [
                v,
                65535 - v,
                (i * 4111 % 65536) as u16,
                [65535, 30000, 0][i as usize % 3],
            ]
        })
        .collect();
    let image = PrecisionImage::from_rgba16(W, H, pixels, IccProfile::srgb(), None).unwrap();
    Doc::Owned(Box::new(
        PrecisionDocument::from_image(image, Default::default(), Default::default()).unwrap(),
    ))
}

fn gray16() -> Doc {
    let request =
        CString::new(r#"{"depth":16,"space":"gray_gamma_22","fill_srgb":[0.3,0.3,0.3,1]}"#)
            .unwrap();
    let mut err = ptr::null_mut();
    let doc = unsafe {
        dr_precision_doc_new_gray(W, H, ptr::null(), 0, request.as_ptr(), LIMIT, 0, &mut err)
    };
    raw(doc, err)
}

fn gray8() -> Doc {
    let request =
        CString::new(r#"{"depth":8,"space":"gray_gamma_22","fill_srgb":[0.7,0.7,0.7,1]}"#).unwrap();
    let mut err = ptr::null_mut();
    let doc = unsafe {
        dr_precision_doc_new_gray(W, H, ptr::null(), 0, request.as_ptr(), LIMIT, 0, &mut err)
    };
    raw(doc, err)
}

fn lab16() -> Doc {
    let request = CString::new(r#"{"depth":16,"fill_srgb":[0.2,0.6,0.9,1]}"#).unwrap();
    let mut err = ptr::null_mut();
    let doc = unsafe { dr_precision_doc_new_lab(W, H, request.as_ptr(), LIMIT, 0, &mut err) };
    raw(doc, err)
}

fn float32() -> Doc {
    let pixels = (0..N)
        .map(|i| {
            let v = i as f32 / N as f32;
            [v * 2.5, 1. - v, 0.25, [1., 0.5, 0.][i % 3]]
        })
        .collect();
    Doc::Owned(Box::new(float_doc(W, H, &IccProfile::srgb(), &[pixels])))
}

fn plan(doc: &PrecisionDocument, request: &Value) -> Result<Value, String> {
    let t = text(request.clone());
    let mut e = ptr::null_mut();
    let p = unsafe { dr_precision_doc_edit_plan(doc, t.as_ptr(), LIMIT, 0, &mut e) };
    if p.is_null() {
        return Err(unsafe { error(e) });
    }
    let value = serde_json::from_slice(unsafe { CStr::from_ptr(p) }.to_bytes()).unwrap();
    unsafe { doctored_core::ffi::dr_string_free(p) };
    Ok(value)
}

/// The edit's result (None: nothing changed), or its refusal.
fn edit(doc: &PrecisionDocument, request: &Value, input: &[u16]) -> Result<Option<Doc>, String> {
    let t = text(request.clone());
    let mut e = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_edit(
            doc,
            t.as_ptr(),
            input.as_ptr(),
            input.len(),
            LIMIT,
            0,
            ptr::null(),
            &mut e,
        )
    };
    if !e.is_null() {
        return Err(unsafe { error(e) });
    }
    Ok((!out.is_null()).then_some(Doc::Raw(out)))
}

fn blend(color: [f32; 4], selection: bool, ink_planes: bool) -> Value {
    json!({"op":"paint_blend","layer":0,"color":color,"color_space":"document",
           "opacity":0.8,"blend_mode":0,"selection":selection,"ink_planes":ink_planes})
}

/// Coverage holding 0, 65535 and intermediate weights.
fn coverage() -> Vec<u16> {
    (0..N as u32)
        .map(|i| match i % 4 {
            0 => 0,
            1 => 65535,
            k => (k * 13001 + i * 31) as u16,
        })
        .collect()
}

fn selection() -> Vec<u16> {
    (0..N as u32)
        .map(|i| [65535, 20000, 0, 50000][(i / 2) as usize % 4])
        .collect()
}

/// The layer's samples as raw bits, at either storage.
fn bits(doc: &PrecisionDocument) -> Vec<u32> {
    let image = doc.layer_source(0).unwrap();
    if doc.is_float() {
        image
            .to_rgba32()
            .unwrap()
            .into_iter()
            .flatten()
            .map(f32::to_bits)
            .collect()
    } else {
        image
            .to_rgba16()
            .unwrap()
            .into_iter()
            .flatten()
            .map(u32::from)
            .collect()
    }
}

#[test]
fn planes_of_one_code_are_the_plain_paint_blend_of_that_colour() {
    let codes = [
        [51234_u16, 1200, 40000],
        [0, 65535, 32768],
        [7, 65000, 12345],
    ];
    for (name, make) in [
        ("rgb16", rgb16 as fn() -> Doc),
        ("gray16", gray16),
        ("lab16", lab16),
        ("float32", float32),
    ] {
        let doc = make();
        for code in codes {
            let color = [
                f32::from(code[0]) / 65535.,
                f32::from(code[1]) / 65535.,
                f32::from(code[2]) / 65535.,
                0.9,
            ];
            for selected in [false, true] {
                let mut input = coverage();
                if selected {
                    input.extend(selection());
                }
                let plain = edit(doc.get(), &blend(color, selected, false), &input)
                    .unwrap()
                    .expect("the plain stroke paints");
                for c in code {
                    input.extend(std::iter::repeat_n(c, N));
                }
                // The request's RGB is inert; its alpha is kept.
                let request = blend([0.3, 0.3, 0.3, 0.9], selected, true);
                let p = plan(doc.get(), &request).unwrap();
                let planes = if selected { 5 } else { 4 };
                assert_eq!(p["input_planes"], planes, "{name}");
                assert_eq!(p["input_samples"], planes * N as u64, "{name}");
                let inked = edit(doc.get(), &request, &input)
                    .unwrap()
                    .expect("the inked stroke paints");
                assert_eq!(
                    bits(inked.get()),
                    bits(plain.get()),
                    "{name} {code:?} selected {selected}"
                );
            }
        }
    }
}

#[test]
fn two_colour_planes_composite_each_pixel_in_its_own_ink() {
    let doc = rgb16();
    let backdrop = doc.get().layer_source(0).unwrap().to_rgba16().unwrap();
    let inks = [[60000_u16, 1000, 20000], [500, 30000, 65535]];
    let which = |i: usize| (i * 7 / 3) % 2;
    let cov = coverage();
    let sel = selection();
    let mut input = cov.clone();
    input.extend(&sel);
    for ch in [0, 1, 2] {
        input.extend((0..N).map(|i| inks[which(i)][ch]));
    }
    let (alpha, opacity) = (0.9_f64, 0.8_f64);
    let request = blend([1., 1., 1., alpha as f32], true, true);
    let got = edit(doc.get(), &request, &input).unwrap().expect("paints");
    let got = got.get().layer_source(0).unwrap().to_rgba16().unwrap();
    for i in 0..N {
        let sa = alpha * opacity * f64::from(cov[i]) / 65535. * f64::from(sel[i]) / 65535.;
        let b = backdrop[i].map(|v| f64::from(v) / 65535.);
        let oa = sa + b[3] * (1. - sa);
        let mut want = [0_f64; 4];
        for ch in 0..3 {
            let s = f64::from(inks[which(i)][ch]) / 65535.;
            want[ch] = if oa > 0. {
                (s * sa + b[ch] * b[3] * (1. - sa)) / oa
            } else {
                b[ch]
            };
        }
        want[3] = oa;
        for ch in 0..4 {
            let w = (want[ch] * 65535.).round();
            if ch < 3 && oa * 65535. < 1. {
                continue;
            }
            assert!(
                (f64::from(got[i][ch]) - w).abs() <= 1.,
                "pixel {i} channel {ch}: {} against {w}",
                got[i][ch]
            );
        }
    }
}

#[test]
fn ink_planes_refuse_a_mode_a_pattern_a_plane_and_a_one_ink_document() {
    let input = vec![65535_u16; 4 * N];
    let doc = rgb16();
    let mut multiply = blend([1.; 4], false, true);
    multiply["blend_mode"] = json!(1);
    assert_eq!(plan(doc.get(), &multiply).unwrap_err(), BLEND);
    assert_eq!(edit(doc.get(), &multiply, &input).err().unwrap(), BLEND);
    let mut patterned = blend([1.; 4], false, true);
    patterned["pattern"] = json!({"name":"T","width":1,"height":1,"rgba":"/wAA/w=="});
    assert_eq!(plan(doc.get(), &patterned).unwrap_err(), BLEND);
    let float = float32();
    let mut plane = blend([1.; 4], false, true);
    plane["plane"] = json!("red");
    assert_eq!(plan(float.get(), &plane).unwrap_err(), BLEND);
    // Without ink planes those requests plan.
    multiply["ink_planes"] = json!(false);
    assert!(plan(doc.get(), &multiply).is_ok());
    plane["ink_planes"] = json!(false);
    assert!(plan(float.get(), &plane).is_ok());

    // Indexed and Bitmap hold one ink a stroke.
    let options = CString::new(r#"{"palette":"exact","dither":"none"}"#).unwrap();
    let mut err = ptr::null_mut();
    let indexed = raw(
        unsafe {
            dr_precision_doc_convert_to_indexed(
                doc.get(),
                options.as_ptr(),
                LIMIT,
                0,
                ptr::null(),
                &mut err,
            )
        },
        err,
    );
    let request = blend([1.; 4], false, true);
    assert_eq!(edit(indexed.get(), &request, &input).err().unwrap(), MODE);
    let gray = gray8();
    let options = CString::new(r#"{"method":"threshold"}"#).unwrap();
    let mut err = ptr::null_mut();
    let bitmap = raw(
        unsafe {
            dr_precision_doc_convert_to_bitmap(
                gray.get(),
                options.as_ptr(),
                LIMIT,
                0,
                ptr::null(),
                &mut err,
            )
        },
        err,
    );
    assert_eq!(edit(bitmap.get(), &request, &input).err().unwrap(), MODE);
}

#[test]
fn a_short_input_is_refused() {
    for make in [rgb16 as fn() -> Doc, float32] {
        let doc = make();
        let request = blend([1.; 4], true, true);
        let short = vec![65535_u16; 5 * N - 1];
        assert_eq!(
            edit(doc.get(), &request, &short).err().unwrap(),
            "high-depth input sample length does not match the core plan"
        );
        // The coverage and selection alone are short too.
        let two = vec![65535_u16; 2 * N];
        assert!(edit(doc.get(), &request, &two).is_err());
    }
}
