//! Copy, Cut, Copy Merged and Paste on and between CMYK documents (#1
//! phase 4c), black-box through the FFI: RZCB v4 round trips bit for bit and
//! refuses each malformed stream by name; Copy reads the raw ink under the
//! coverage (alpha `round(a·c / 65535)`), Cut erases it (`a·(1 − c/65535)`),
//! Copy Merged reads the composite and a group's Copy its raw projection;
//! Paste, Paste in Place, Paste Into and New from Clipboard keep ink bit for
//! bit between documents of one profile at any declared depth, and so do the
//! layers paste and Duplicate to New Document; Layer Via copies ink exactly.
//!
//! Across modes and profiles every conversion is held to an INDEPENDENT
//! oracle, within 2/65535 per channel (fixed here before measuring): RGB16 →
//! CMYK `cmyk_fixture::rgb_to_ink_in`, CMYK → RGB16
//! `cmyk_fixture::ink_to_rgb_in` (both relative colorimetric with BPC through
//! the profiles' own tags) and CMYK A → CMYK B `cmyk_paste_fixture::a_to_b`
//! (A2B → Lab → XYZ → BPC → B2A1 in closed form). Don't Convert keeps ink
//! between CMYK profiles and is refused across modes; a layers clip of the
//! other mode is refused by name.

use std::ffi::{c_char, CStr, CString};
use std::ptr;

use doctored_core::cmyk_clipboard_codec::CmykClipboardPayload;
use doctored_core::ffi::{dr_image_free, dr_string_free};
use doctored_core::ffi_clipboard::{
    dr_clipboard_payload_bytes, dr_clipboard_payload_decode, dr_clipboard_payload_editable,
    dr_clipboard_payload_free, dr_clipboard_payload_image, dr_clipboard_payload_info,
    dr_clipboard_payload_rgba8, dr_clipboard_payload_trim, dr_clipboard_payload_with_editable,
    dr_precision_doc_clipboard_copy, dr_precision_doc_clipboard_cut,
    dr_precision_doc_clipboard_paste, dr_precision_doc_from_clipboard, DrClipboardPayload,
};
use doctored_core::ffi_cmyk_clipboard::{
    dr_clipboard_payload_from_cmyk_image, dr_clipboard_payload_icc,
};
use doctored_core::ffi_layer_clipboard::dr_precision_doc_paste_layers;
use doctored_core::ffi_precision::dr_precision_doc_descriptor;
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_clipboard::ClipboardPayload;
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer};
use doctored_core::precision_image::PrecisionImage;
use serde_json::Value;

use crate::cmyk_fixture::{ink_to_rgb_in, press_profile, rgb_to_ink_in};
use crate::cmyk_paste_fixture::{a_to_b, cmyk_layers_in, second_press_profile};
use crate::cmyk_structure_fixture::{
    assert_flat, composite, free, json, level, ramp, then, try_edit, varied, with_levels,
    with_mode, with_opacity, RgbLayer, LIMIT,
};
use crate::cmyk_tiff_builder::cmyk_tiff;
use crate::common::{img_dims, img_pixels, take_err_string};

const W: u32 = 8;
const H: u32 = 6;
/// The conversion bound, per channel, in 65535ths.
const BOUND: f64 = 2.0;

fn take(err: *mut c_char) -> Option<String> {
    if err.is_null() {
        return None;
    }
    let text = unsafe { CStr::from_ptr(err) }
        .to_string_lossy()
        .into_owned();
    unsafe { dr_string_free(err) };
    Some(text)
}

/// Bottom (opaque), A (6 × 5 at (−2, −1)), C (canvas) in `profile`.
fn doc_in(profile: &[u8], depth: u32, seed: u32) -> *mut PrecisionDocument {
    let mut bottom = varied(W, H, seed);
    for p in &mut bottom {
        p[3] = 65535;
    }
    cmyk_layers_in(
        profile,
        depth,
        W,
        H,
        vec![
            RgbLayer {
                name: "Bottom",
                w: W,
                h: H,
                offset: (0, 0),
                pixels: bottom,
            },
            RgbLayer {
                name: "A",
                w: 6,
                h: 5,
                offset: (-2, -1),
                pixels: varied(6, 5, seed + 1),
            },
            RgbLayer {
                name: "C",
                w: W,
                h: H,
                offset: (0, 0),
                pixels: varied(W, H, seed + 2),
            },
        ],
    )
}

fn press() -> Vec<u8> {
    press_profile("Press Test")
}

fn copy(
    doc: *const PrecisionDocument,
    request: &str,
    coverage: Option<&[u16]>,
) -> Result<*mut DrClipboardPayload, String> {
    let text = CString::new(request).unwrap();
    let mut err = ptr::null_mut();
    let (pointer, length) = coverage.map_or((ptr::null(), 0), |c| (c.as_ptr(), c.len()));
    let out = unsafe {
        dr_precision_doc_clipboard_copy(
            doc,
            text.as_ptr(),
            pointer,
            length,
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    if out.is_null() {
        return Err(take(err).expect("a refusal names itself"));
    }
    Ok(out)
}

fn must_copy(
    doc: *const PrecisionDocument,
    request: &str,
    coverage: Option<&[u16]>,
) -> *mut DrClipboardPayload {
    copy(doc, request, coverage).unwrap_or_else(|e| panic!("{request}: {e}"))
}

fn bytes(payload: *const DrClipboardPayload) -> Vec<u8> {
    let mut length = 0usize;
    let pointer = unsafe { dr_clipboard_payload_bytes(payload, &mut length) };
    unsafe { std::slice::from_raw_parts(pointer, length) }.to_vec()
}

fn decoded(payload: *const DrClipboardPayload) -> CmykClipboardPayload {
    CmykClipboardPayload::decode(&bytes(payload), LIMIT, 0).expect("a CMYK payload")
}

fn info(payload: *const DrClipboardPayload) -> Value {
    let mut err = ptr::null_mut();
    json(unsafe { dr_clipboard_payload_info(payload, &mut err) }, err)
}

fn paste(
    doc: *const PrecisionDocument,
    payload: *const DrClipboardPayload,
    request: &str,
) -> Result<*mut PrecisionDocument, String> {
    let text = CString::new(request).unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_clipboard_paste(
            doc,
            payload,
            text.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    if out.is_null() {
        return Err(take(err).expect("a refusal names itself"));
    }
    Ok(out)
}

fn must_paste(
    doc: *const PrecisionDocument,
    payload: *const DrClipboardPayload,
    request: &str,
) -> *mut PrecisionDocument {
    paste(doc, payload, request).unwrap_or_else(|e| panic!("{request}: {e}"))
}

fn top_request(doc: *const PrecisionDocument, placement: &str, extra: &str) -> String {
    let anchor = unsafe { &*doc }.layer_count() - 1;
    format!(
        r#"{{"name":"Pasted","anchor":{anchor},"position":"top","placement":"{placement}"{extra}}}"#
    )
}

fn top(doc: *const PrecisionDocument) -> PrecisionDocumentLayer {
    let doc = unsafe { &*doc };
    doc.layer(doc.layer_count() - 1).unwrap().clone()
}

fn samples(layer: &PrecisionDocumentLayer) -> Vec<[u16; 5]> {
    layer
        .cmyk_source()
        .expect("a CMYK leaf")
        .to_contiguous()
        .unwrap()
}

fn cover(alpha: u16, coverage: u16) -> u16 {
    ((u32::from(alpha) * u32::from(coverage) + 32767) / 65535) as u16
}

fn full_window() -> String {
    format!(r#""x":0,"y":0,"width":{W},"height":{H}"#)
}

#[test]
fn v4_round_trips_bit_exactly_and_describes_itself() {
    let doc = doc_in(&press(), 16, 1);
    let coverage = ramp(W, H, 7);
    let payload = must_copy(
        doc,
        r#"{"op":"copy","layer":1,"x":1,"y":1,"width":5,"height":4}"#,
        Some(&coverage),
    );
    let encoded = bytes(payload);
    let mut err = ptr::null_mut();
    let again =
        unsafe { dr_clipboard_payload_decode(encoded.as_ptr(), encoded.len(), LIMIT, 0, &mut err) };
    assert!(!again.is_null(), "{}", take_err_string(err));
    assert_eq!(bytes(again), encoded, "decode then encode is the identity");
    let described = info(again);
    assert_eq!(described["version"], 4);
    assert_eq!(described["mode"], "cmyk");
    assert_eq!(described["storage"], "cmyka16_unorm");
    assert_eq!(described["cmyk_depth"], 16);
    assert_eq!(described["width"], 5);
    assert_eq!(described["offset"], serde_json::json!([1, 1]));
    assert_eq!(described["has_coverage"], true);
    assert_eq!(described["editable"], Value::Null);
    // The ink is not an RGB image. It carries no editable section until the
    // host attaches one (#1's artwork batch), which then rides the v4
    // stream through an encode and a decode.
    assert!(unsafe { dr_clipboard_payload_image(again) }.is_null());
    let mut err = ptr::null_mut();
    assert!(unsafe { dr_clipboard_payload_editable(again, &mut err) }.is_null());
    assert!(err.is_null(), "no section is not an error");
    let section = c"{\"type\":\"text\",\"record\":\"x\"}";
    let mut err = ptr::null_mut();
    let edited =
        unsafe { dr_clipboard_payload_with_editable(again, section.as_ptr(), LIMIT, 0, &mut err) };
    assert!(!edited.is_null(), "{}", take_err_string(err));
    let carried = info(edited);
    assert_eq!(carried["editable"]["type"], "text");
    assert_eq!(carried["editable"]["record_bytes"], 1);
    let encoded_section = bytes(edited);
    let mut err = ptr::null_mut();
    let reread = unsafe {
        dr_clipboard_payload_decode(
            encoded_section.as_ptr(),
            encoded_section.len(),
            LIMIT,
            0,
            &mut err,
        )
    };
    assert!(!reread.is_null(), "{}", take_err_string(err));
    let mut err = ptr::null_mut();
    let text = unsafe { dr_clipboard_payload_editable(reread, &mut err) };
    assert_eq!(
        take(text).as_deref(),
        Some("{\"type\":\"text\",\"record\":\"x\"}")
    );
    assert_eq!(bytes(reread), encoded_section);
    let bad = c"{\"type\":\"text\"}";
    let mut err = ptr::null_mut();
    assert!(
        unsafe { dr_clipboard_payload_with_editable(again, bad.as_ptr(), LIMIT, 0, &mut err) }
            .is_null()
    );
    assert_eq!(
        take(err).unwrap(),
        "clipboard editable section has no `record`"
    );
    unsafe {
        dr_clipboard_payload_free(edited);
        dr_clipboard_payload_free(reread);
    }
    // The profile bytes are the policy's, verbatim.
    let mut length = 0usize;
    let icc = unsafe { dr_clipboard_payload_icc(again, &mut length) };
    assert_eq!(
        unsafe { std::slice::from_raw_parts(icc, length) },
        &press()[..]
    );
    // An RGB payload says so.
    let rgb = rgb_doc(&[[1000, 2000, 3000, 65535]; 4], 2, 2);
    let rgb_payload = must_copy(
        rgb,
        r#"{"op":"copy_merged","x":0,"y":0,"width":2,"height":2}"#,
        None,
    );
    assert_eq!(info(rgb_payload)["mode"], "rgb");
    unsafe {
        dr_clipboard_payload_free(rgb_payload);
        dr_clipboard_payload_free(again);
        dr_clipboard_payload_free(payload);
    }
    free(rgb);
    free(doc);
}

fn word(bytes: &mut [u8], at: usize, value: u32) {
    bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
}

#[test]
fn v4_refuses_every_malformed_stream_by_name() {
    let doc = doc_in(&press(), 16, 2);
    let payload = must_copy(
        doc,
        r#"{"op":"copy","layer":2,"x":0,"y":0,"width":3,"height":2}"#,
        None,
    );
    let good = bytes(payload);
    let p = press().len();
    let refuse = |mutate: &dyn Fn(&mut Vec<u8>)| -> String {
        let mut stream = good.clone();
        mutate(&mut stream);
        match CmykClipboardPayload::decode(&stream, LIMIT, 0) {
            Err(e) => e,
            Ok(_) => panic!("the stream decoded"),
        }
    };
    assert_eq!(
        refuse(&|s| word(s, 4, 3)),
        "clipboard payload version 3 is not a CMYK payload (CMYK payloads are version 4)"
    );
    assert_eq!(
        refuse(&|s| word(s, 8, 1)),
        "clipboard payload version 4 holds only CMYK storage; storage code 1 is not CMYK"
    );
    assert_eq!(
        refuse(&|s| word(s, 12, 0)),
        "clipboard payload has an empty window"
    );
    assert_eq!(
        refuse(&|s| s[28..32].copy_from_slice(&f32::NAN.to_le_bytes())),
        "clipboard payload resolution must be finite and positive"
    );
    assert_eq!(
        refuse(&|s| s[40 + p] = 2),
        "clipboard payload flags 2 are invalid"
    );
    assert_eq!(
        refuse(&|s| s[40 + p] = 4),
        "clipboard payload editable flag disagrees with its section length 0"
    );
    assert_eq!(
        refuse(&|s| s[40 + p] = 8),
        "clipboard payload flags 8 are invalid"
    );
    assert_eq!(
        refuse(&|s| word(s, 41 + p, 1)),
        "clipboard payload tone-map operator 1 is invalid on CMYK storage"
    );
    assert_eq!(
        refuse(&|s| word(s, 49 + p, 5)),
        "clipboard payload editable flag disagrees with its section length 5"
    );
    assert_eq!(
        refuse(&|s| word(s, 53 + p, 12)),
        "clipboard payload CMYK depth 12 is not 8 or 16"
    );
    assert!(refuse(&|s| {
        s.pop();
    })
    .contains("its header declares"));
    assert_eq!(
        refuse(&|s| s[0] = b'X'),
        "clipboard payload is not an RZCB stream"
    );
    // An RGB profile in a CMYK stream is CmykPolicy's refusal.
    let srgb = IccProfile::srgb().bytes().to_vec();
    let mut swapped = good[..36].to_vec();
    swapped.extend_from_slice(&(srgb.len() as u32).to_le_bytes());
    swapped.extend_from_slice(&srgb);
    swapped.extend_from_slice(&good[40 + p..]);
    let sentence = CmykClipboardPayload::decode(&swapped, LIMIT, 0)
        .err()
        .unwrap();
    assert!(
        sentence.starts_with("cannot use the CMYK profile"),
        "{sentence}"
    );
    // The RGB reader still refuses version 4 at its version check.
    let rgb = ClipboardPayload::decode(&good, LIMIT, 0).err().unwrap();
    assert!(rgb.contains("version 4 is not supported"), "{rgb}");
    unsafe { dr_clipboard_payload_free(payload) };
    free(doc);
}

#[test]
fn copy_reads_raw_ink_and_copy_merged_the_composite_under_coverage() {
    let doc = doc_in(&press(), 16, 3);
    let coverage = ramp(W, H, 11);
    let a = unsafe { &*doc }.layer(1).unwrap().clone();
    let image = a.cmyk_source().unwrap();
    let payload = must_copy(
        doc,
        &format!(r#"{{"op":"copy","layer":1,{}}}"#, full_window()),
        Some(&coverage),
    );
    let got = decoded(payload);
    assert_eq!((got.offset(), got.dimensions()), ((0, 0), (W, H)));
    let ink = got.image().to_contiguous().unwrap();
    for y in 0..H {
        for x in 0..W {
            let c = coverage[(y * W + x) as usize];
            // A sits at (−2, −1).
            let want = image
                .sample_at(x + 2, y + 1)
                .map_or([0; 5], |s| [s[0], s[1], s[2], s[3], cover(s[4], c)]);
            assert_eq!(ink[(y * W + x) as usize], want, "copy at ({x}, {y})");
        }
    }
    assert_eq!(got.coverage().unwrap(), &coverage[..]);
    unsafe { dr_clipboard_payload_free(payload) };
    let merged = must_copy(
        doc,
        &format!(r#"{{"op":"copy_merged",{}}}"#, full_window()),
        Some(&coverage),
    );
    let ink = decoded(merged).image().to_contiguous().unwrap();
    for y in 0..H {
        for x in 0..W {
            let s = composite(doc, x as i32, y as i32);
            let want = [
                s[0],
                s[1],
                s[2],
                s[3],
                cover(s[4], coverage[(y * W + x) as usize]),
            ];
            assert_eq!(ink[(y * W + x) as usize], want, "copy merged at ({x}, {y})");
        }
    }
    unsafe { dr_clipboard_payload_free(merged) };
    free(doc);
}

#[test]
fn a_groups_copy_is_its_raw_projection_and_an_adjustment_refuses() {
    let mut doc = doc_in(&press(), 16, 4);
    doc = then(
        doc,
        r#"{"op":"group_layers","layers":[1,2],"name":"G","pass_through":false}"#,
        &[],
    );
    doc = with_opacity(doc, 3, 0.5);
    doc = with_mode(doc, 3, doctored_core::doc::BlendMode::Multiply.to_c());
    let children: Vec<PrecisionDocumentLayer> = unsafe { &*doc }
        .layer(3)
        .unwrap()
        .children()
        .unwrap()
        .to_vec();
    let payload = must_copy(
        doc,
        &format!(r#"{{"op":"copy","layer":3,{}}}"#, full_window()),
        None,
    );
    let got = decoded(payload);
    for y in 0..H {
        for x in 0..W {
            let mut want = [0.0; 5];
            level(&children, i64::from(x), i64::from(y), &mut want);
            assert_flat(
                got.image().sample_at(x, y).unwrap(),
                want,
                2,
                &format!("group copy ({x}, {y})"),
            );
        }
    }
    unsafe { dr_clipboard_payload_free(payload) };
    let adjusted = with_levels(doc, 1.0);
    let top = unsafe { &*adjusted }.layer_count() - 1;
    assert_eq!(
        copy(
            adjusted,
            &format!(r#"{{"op":"copy","layer":{top},{}}}"#, full_window()),
            None
        )
        .err()
        .unwrap(),
        format!(
            "Copy requires a raster layer; layer {top} is an adjustment layer (use Copy Merged)"
        )
    );
    free(adjusted);
}

fn cut(
    doc: *const PrecisionDocument,
    layer: usize,
    coverage: Option<&[u16]>,
) -> Result<(*mut DrClipboardPayload, *mut PrecisionDocument), String> {
    let text = CString::new(format!(
        r#"{{"op":"cut","layer":{layer},{}}}"#,
        full_window()
    ))
    .unwrap();
    let (pointer, length) = coverage.map_or((ptr::null(), 0), |c| (c.as_ptr(), c.len()));
    let mut payload = ptr::null_mut();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_clipboard_cut(
            doc,
            text.as_ptr(),
            pointer,
            length,
            LIMIT,
            0,
            ptr::null(),
            &mut payload,
            &mut err,
        )
    };
    if let Some(e) = take(err) {
        return Err(e);
    }
    Ok((payload, out))
}

#[test]
fn cut_is_copy_then_the_ink_erase() {
    let doc = doc_in(&press(), 16, 5);
    let coverage = ramp(W, H, 13);
    let before = samples(&unsafe { &*doc }.layer(2).unwrap().clone());
    let (payload, after) = cut(doc, 2, Some(&coverage)).unwrap();
    assert!(!payload.is_null() && !after.is_null());
    let copied = decoded(payload).image().to_contiguous().unwrap();
    let erased = samples(&unsafe { &*after }.layer(2).unwrap().clone());
    for (i, ((b, c), e)) in before.iter().zip(&coverage).zip(&erased).enumerate() {
        assert_eq!(
            copied[i],
            [b[0], b[1], b[2], b[3], cover(b[4], *c)],
            "copied {i}"
        );
        assert_eq!(&e[..4], &b[..4], "the ink is untouched at {i}");
        let want = f64::from(b[4]) * (1.0 - f64::from(*c) / 65535.0);
        assert!(
            (f64::from(e[4]) - want).abs() <= 1.0,
            "erased alpha {i}: {} vs {want}",
            e[4]
        );
    }
    unsafe { dr_clipboard_payload_free(payload) };
    free(after);
    // A group refuses; a transparency lock answers no document.
    let grouped = then(
        doc,
        r#"{"op":"group_layers","layers":[1],"name":"G","pass_through":false}"#,
        &[],
    );
    assert_eq!(
        cut(grouped, 2, None).err().unwrap(),
        "Cut requires a raster layer; layer 2 is a group"
    );
    let locked = then(grouped, r#"{"op":"layer_lock","layer":3,"locks":1}"#, &[]);
    let (payload, after) = cut(locked, 3, None).unwrap();
    assert!(!payload.is_null(), "the payload is still made");
    assert!(after.is_null(), "a transparency lock changes nothing");
    unsafe { dr_clipboard_payload_free(payload) };
    free(locked);
}

#[test]
fn paste_keeps_ink_bit_for_bit_between_documents_of_one_profile() {
    let source = doc_in(&press(), 16, 6);
    let payload = must_copy(
        source,
        r#"{"op":"copy","layer":2,"x":2,"y":1,"width":4,"height":3}"#,
        None,
    );
    let copied = decoded(payload).image().to_contiguous().unwrap();
    for depth in [16, 8] {
        let target = doc_in(&press(), depth, 30);
        for (placement, extra, offset) in [
            ("in_place", "", (2, 1)),
            ("centered", "", (2, 1)),
            (
                "into",
                r#","frame":[1,1,3,2]"#,
                (1 + (3 - 4i32).div_euclid(2), 1 + (2 - 3i32).div_euclid(2)),
            ),
        ] {
            let after = must_paste(target, payload, &top_request(target, placement, extra));
            let pasted = top(after);
            assert_eq!(pasted.name(), "Pasted");
            assert_eq!(pasted.offset(), offset, "{placement} at depth {depth}");
            assert_eq!(
                samples(&pasted),
                copied,
                "{placement}: ink kept bit for bit at depth {depth}"
            );
            assert!(unsafe { &*after }.is_cmyk());
            free(after);
        }
        free(target);
    }
    // New from Clipboard: one Background of the payload's own policy.
    let mut err = ptr::null_mut();
    let made = unsafe { dr_precision_doc_from_clipboard(payload, LIMIT, 0, ptr::null(), &mut err) };
    assert!(!made.is_null(), "{}", take_err_string(err));
    let made_doc = unsafe { &*made };
    assert_eq!(made_doc.dimensions(), (4, 3));
    assert_eq!(
        made_doc.cmyk_policy().unwrap().profile_bytes(),
        &press()[..]
    );
    assert_eq!(made_doc.cmyk_policy().unwrap().depth().bits(), 16);
    assert_eq!(made_doc.layer(0).unwrap().name(), "Background");
    assert_eq!(samples(made_doc.layer(0).unwrap()), copied);
    free(made);
    unsafe { dr_clipboard_payload_free(payload) };
    free(source);
}

#[test]
fn trim_cuts_a_cmyk_payload_to_its_alpha_box() {
    let doc = doc_in(&press(), 16, 7);
    let mut coverage = vec![0u16; (W * H) as usize];
    for y in 2..4 {
        for x in 3..6 {
            coverage[(y * W + x) as usize] = 65535;
        }
    }
    let payload = must_copy(
        doc,
        &format!(r#"{{"op":"copy","layer":0,{}}}"#, full_window()),
        Some(&coverage),
    );
    let mut err = ptr::null_mut();
    let trimmed = unsafe { dr_clipboard_payload_trim(payload, LIMIT, 0, &mut err) };
    assert!(!trimmed.is_null(), "{}", take_err_string(err));
    let got = decoded(trimmed);
    assert_eq!((got.offset(), got.dimensions()), ((3, 2), (3, 2)));
    let full = decoded(payload).image().to_contiguous().unwrap();
    let ink = got.image().to_contiguous().unwrap();
    for y in 0..2u32 {
        for x in 0..3u32 {
            assert_eq!(
                ink[(y * 3 + x) as usize],
                full[((y + 2) * W + x + 3) as usize]
            );
        }
    }
    // Already tight: nothing to trim.
    let mut err = ptr::null_mut();
    assert!(unsafe { dr_clipboard_payload_trim(trimmed, LIMIT, 0, &mut err) }.is_null());
    assert!(err.is_null());
    unsafe {
        dr_clipboard_payload_free(trimmed);
        dr_clipboard_payload_free(payload);
    }
    free(doc);
}

#[test]
fn layer_via_copies_ink_exactly_and_cuts_through_the_erase() {
    let doc = doc_in(&press(), 16, 8);
    let coverage = ramp(W, H, 17);
    let source = samples(&unsafe { &*doc }.layer(2).unwrap().clone());
    for cut in [false, true] {
        let after = try_edit(
            doc,
            &format!(r#"{{"op":"layer_via","layer":2,"cut":{cut},"name":"Via","selection":true}}"#),
            &coverage,
        )
        .unwrap();
        assert!(!after.is_null());
        let doc_after = unsafe { &*after };
        assert_eq!(doc_after.layer(3).unwrap().name(), "Via");
        let via = samples(doc_after.layer(3).unwrap());
        for (i, (s, c)) in source.iter().zip(&coverage).enumerate() {
            assert_eq!(via[i], [s[0], s[1], s[2], s[3], cover(s[4], *c)], "via {i}");
        }
        let kept = samples(doc_after.layer(2).unwrap());
        if cut {
            for (i, (s, c)) in source.iter().zip(&coverage).enumerate() {
                let want = f64::from(s[4]) * (1.0 - f64::from(*c) / 65535.0);
                assert!((f64::from(kept[i][4]) - want).abs() <= 1.0, "cut {i}");
            }
        } else {
            assert_eq!(kept, source);
        }
        free(after);
    }
    free(doc);
}

fn duplicate(doc: *const PrecisionDocument, layers: &str) -> *mut PrecisionDocument {
    let out = try_edit(
        doc,
        &format!(r#"{{"op":"duplicate_to_new_document","layers":[{layers}]}}"#),
        &[],
    )
    .unwrap();
    assert!(!out.is_null());
    out
}

fn paste_layers(
    doc: *const PrecisionDocument,
    clip: *const PrecisionDocument,
    offset: (i32, i32),
    preserve: bool,
) -> Result<*mut PrecisionDocument, String> {
    let anchor = unsafe { &*doc }.layer_count() - 1;
    let mut report = ptr::null_mut();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_paste_layers(
            doc,
            clip,
            anchor,
            1,
            offset.0,
            offset.1,
            preserve,
            LIMIT,
            0,
            &mut report,
            &mut err,
        )
    };
    take(report);
    if out.is_null() {
        return Err(take(err).expect("a refusal names itself"));
    }
    Ok(out)
}

#[test]
fn same_profile_layers_paste_and_duplicate_are_bit_exact() {
    let source = doc_in(&press(), 16, 9);
    let clip = duplicate(source, "1,2");
    let clip_doc = unsafe { &*clip };
    assert!(clip_doc.is_cmyk());
    assert_eq!(
        clip_doc.cmyk_policy().unwrap().profile_bytes(),
        &press()[..]
    );
    assert_eq!(clip_doc.layer_count(), 2);
    for (i, src) in [1usize, 2].iter().enumerate() {
        let want = unsafe { &*source }.layer(*src).unwrap();
        let got = clip_doc.layer(i).unwrap();
        assert_eq!(samples(got), samples(want));
        assert_eq!(got.offset(), want.offset());
    }
    let target = doc_in(&press(), 16, 40);
    let after = paste_layers(target, clip, (0, 0), false).unwrap();
    let n = unsafe { &*after }.layer_count();
    assert_eq!(n, 5);
    for (i, src) in [1usize, 2].iter().enumerate() {
        let got = unsafe { &*after }.layer(3 + i).unwrap();
        let want = unsafe { &*source }.layer(*src).unwrap();
        assert_eq!(samples(got), samples(want), "pasted layer {i}");
        assert_eq!(got.offset(), want.offset());
    }
    free(after);
    // A placement offset moves every CMYK raster by arithmetic alone.
    let moved = paste_layers(target, clip, (2, -1), false).unwrap();
    let a = unsafe { &*moved }.layer(3).unwrap();
    assert_eq!(a.offset(), (0, -2));
    assert_eq!(samples(a), samples(unsafe { &*source }.layer(1).unwrap()));
    free(moved);
    free(target);
    free(clip);
    free(source);
}

/// A `w × h` sRGB u16 document of one layer.
fn rgb_doc(pixels: &[[u16; 4]], w: u32, h: u32) -> *mut PrecisionDocument {
    let image =
        PrecisionImage::from_rgba16(w, h, pixels.to_vec(), IccProfile::srgb(), None).unwrap();
    let layer = PrecisionDocumentLayer::raster("Layer", image, (0, 0), 1.0, true).unwrap();
    Box::into_raw(Box::new(
        PrecisionDocument::from_parts(
            w,
            h,
            IccProfile::srgb(),
            Metadata::default(),
            Resolution::default(),
            vec![layer],
        )
        .unwrap(),
    ))
}

fn rgb_ramp() -> Vec<[u16; 4]> {
    (0..W * H)
        .map(|i| {
            let j = i.wrapping_mul(2_654_435_761);
            [
                (j % 65536) as u16,
                (j.rotate_left(9) % 65536) as u16,
                (j.rotate_left(19) % 65536) as u16,
                [65535, 40000, 9000][(i % 3) as usize],
            ]
        })
        .collect()
}

#[test]
fn rgb16_pastes_into_cmyk_through_the_profiles() {
    let pixels = rgb_ramp();
    let rgb = rgb_doc(&pixels, W, H);
    let payload = must_copy(
        rgb,
        &format!(r#"{{"op":"copy","layer":0,{}}}"#, full_window()),
        None,
    );
    let target = doc_in(&press(), 16, 50);
    let after = must_paste(target, payload, &top_request(target, "in_place", ""));
    let got = samples(&top(after));
    let srgb = IccProfile::srgb();
    for (i, (p, g)) in pixels.iter().zip(&got).enumerate() {
        let want = rgb_to_ink_in(
            [p[0], p[1], p[2]].map(|v| f64::from(v) / 65535.0),
            &srgb,
            true,
        );
        for c in 0..4 {
            assert!(
                (f64::from(g[c]) - want[c] * 65535.0).abs() <= BOUND,
                "pixel {i} plate {c}: {g:?} vs {want:?}"
            );
        }
        assert_eq!(g[4], p[3], "alpha {i}");
    }
    // Don't Convert across modes is refused by name.
    assert_eq!(
        paste(target, payload, &top_request(target, "in_place", r#","profile":"preserve""#)).err().unwrap(),
        "Don't Convert keeps numbers within one colour mode; pixels pasted between RGB and CMYK are always converted"
    );
    free(after);
    free(target);
    unsafe { dr_clipboard_payload_free(payload) };
    free(rgb);
}

#[test]
fn cmyk_pastes_into_rgb16_through_the_profiles() {
    let source = doc_in(&press(), 16, 10);
    let payload = must_copy(
        source,
        &format!(r#"{{"op":"copy","layer":2,{}}}"#, full_window()),
        None,
    );
    let ink = decoded(payload).image().to_contiguous().unwrap();
    let rgb = rgb_doc(&rgb_ramp(), W, H);
    let after = must_paste(
        rgb,
        payload,
        r#"{"name":"Pasted","anchor":0,"position":"top","placement":"in_place"}"#,
    );
    let doc_after = unsafe { &*after };
    let got = doc_after
        .layer(1)
        .unwrap()
        .source()
        .unwrap()
        .to_rgba16()
        .unwrap();
    let srgb = IccProfile::srgb();
    for (i, (s, g)) in ink.iter().zip(&got).enumerate() {
        let want = ink_to_rgb_in(
            [s[0], s[1], s[2], s[3]].map(|v| f64::from(v) / 65535.0),
            &srgb,
            true,
        );
        for c in 0..3 {
            let w = want[c].clamp(0.0, 1.0) * 65535.0;
            assert!(
                (f64::from(g[c]) - w).abs() <= BOUND,
                "pixel {i} channel {c}: {g:?} vs {want:?}"
            );
        }
        assert_eq!(g[3], s[4], "alpha {i}");
    }
    assert_eq!(
        paste(rgb, payload, r#"{"name":"P","anchor":0,"position":"top","placement":"in_place","profile":"preserve"}"#).err().unwrap(),
        "Don't Convert keeps numbers within one colour mode; pixels pasted between RGB and CMYK are always converted"
    );
    // The 8-bit read converts through the same leg.
    let srgb_bytes = srgb.bytes().to_vec();
    let mut err = ptr::null_mut();
    let image = unsafe {
        dr_clipboard_payload_rgba8(
            payload,
            srgb_bytes.as_ptr(),
            srgb_bytes.len(),
            false,
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(!image.is_null(), "{}", take_err_string(err));
    assert_eq!(img_dims(image), (W, H));
    let eight = img_pixels(image);
    for (i, g) in got.iter().enumerate() {
        for c in 0..3 {
            let want = (u32::from(g[c]) * 255 + 32767) / 65535;
            assert!(
                u32::from(eight[i * 4 + c]).abs_diff(want) <= 1,
                "8-bit read {i}"
            );
        }
    }
    unsafe { dr_image_free(image) };
    free(after);
    free(rgb);
    unsafe { dr_clipboard_payload_free(payload) };
    free(source);
}

#[test]
fn cmyk_a_pastes_into_cmyk_b_through_both_profiles() {
    let second = second_press_profile("Press B");
    let source = doc_in(&press(), 16, 11);
    let payload = must_copy(
        source,
        &format!(r#"{{"op":"copy","layer":2,{}}}"#, full_window()),
        None,
    );
    let ink = decoded(payload).image().to_contiguous().unwrap();
    let target = doc_in(&second, 16, 60);
    let after = must_paste(target, payload, &top_request(target, "in_place", ""));
    let got = samples(&top(after));
    let check = |got: &[[u16; 5]], what: &str| {
        for (i, (s, g)) in ink.iter().zip(got).enumerate() {
            let want = a_to_b([s[0], s[1], s[2], s[3]].map(|v| f64::from(v) / 65535.0));
            for c in 0..4 {
                assert!(
                    (f64::from(g[c]) - want[c] * 65535.0).abs() <= BOUND,
                    "{what} pixel {i} plate {c}: {g:?} vs {want:?}"
                );
            }
            assert_eq!(g[4], s[4], "{what} alpha {i}");
        }
    };
    check(&got, "paste");
    free(after);
    // Don't Convert keeps the ink.
    let kept = must_paste(
        target,
        payload,
        &top_request(target, "in_place", r#","profile":"preserve""#),
    );
    assert_eq!(samples(&top(kept)), ink);
    free(kept);
    // The layers paste converts every leaf the same way, and keeps under
    // Don't Convert.
    let clip = duplicate(source, "2");
    let layered = paste_layers(target, clip, (0, 0), false).unwrap();
    check(&samples(&top(layered)), "layers paste");
    free(layered);
    let relabelled = paste_layers(target, clip, (0, 0), true).unwrap();
    assert_eq!(samples(&top(relabelled)), ink);
    free(relabelled);
    free(clip);
    free(target);
    unsafe { dr_clipboard_payload_free(payload) };
    free(source);
}

#[test]
fn a_layers_clip_of_the_other_mode_refuses_by_name() {
    let cmyk = doc_in(&press(), 16, 12);
    let rgb = rgb_doc(&rgb_ramp(), W, H);
    let cmyk_clip = duplicate(cmyk, "1");
    let rgb_clip = duplicate(rgb, "0");
    assert_eq!(
        paste_layers(cmyk, rgb_clip, (0, 0), false).err().unwrap(),
        "pasted layers must be CMYK to land in a CMYK document; convert them to CMYK first"
    );
    assert_eq!(
        paste_layers(rgb, cmyk_clip, (0, 0), false).err().unwrap(),
        "pasted CMYK layers must be converted to RGB before they land in an RGB document"
    );
    for d in [rgb_clip, cmyk_clip, rgb, cmyk] {
        free(d);
    }
}

fn from_image(
    bytes: &[u8],
    fallback: Option<&[u8]>,
) -> Result<(*mut DrClipboardPayload, Option<String>), String> {
    let (fp, fl) = fallback.map_or((ptr::null(), 0), |f| (f.as_ptr(), f.len()));
    let mut assigned = ptr::null_mut();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_clipboard_payload_from_cmyk_image(
            bytes.as_ptr(),
            bytes.len(),
            fp,
            fl,
            LIMIT,
            0,
            &mut assigned,
            &mut err,
        )
    };
    if let Some(e) = take(err) {
        return Err(e);
    }
    Ok((out, take(assigned)))
}

#[test]
fn a_cmyk_tiff_on_the_pasteboard_reads_as_a_cmyk_payload() {
    let ink: Vec<[u16; 4]> = (0..6u16)
        .map(|i| [i * 9000, 60000 - i * 7000, i * 3000, 1000 * i])
        .collect();
    let data: Vec<u8> = ink.iter().flatten().flat_map(|v| v.to_le_bytes()).collect();
    let tagged = cmyk_tiff(3, 2, 16, 4, 0, &data, Some(&press()));
    let (payload, assigned) = from_image(&tagged, None).unwrap();
    assert!(!payload.is_null());
    assert!(assigned.is_none(), "its own profile is kept");
    let got = decoded(payload);
    assert_eq!(got.depth().bits(), 16);
    assert_eq!(got.policy().profile_bytes(), &press()[..]);
    let samples = got.image().to_contiguous().unwrap();
    for (s, i) in samples.iter().zip(&ink) {
        assert_eq!(*s, [i[0], i[1], i[2], i[3], 65535]);
    }
    unsafe { dr_clipboard_payload_free(payload) };
    // Untagged with no working CMYK profile in this process, and bytes that
    // are not a CMYK image: NULL with no error.
    let untagged = cmyk_tiff(3, 2, 16, 4, 0, &data, None);
    let (none, assigned) = from_image(&untagged, Some(&press())).unwrap();
    assert!(none.is_null() && assigned.is_none());
    let (none, _) = from_image(b"not an image at all", None).unwrap();
    assert!(none.is_null());
}

#[test]
fn every_clipboard_plan_is_present_and_admits() {
    let mut doc = doc_in(&press(), 16, 13);
    doc = then(
        doc,
        r#"{"op":"group_layers","layers":[1,2],"name":"G","pass_through":false}"#,
        &[],
    );
    let mut err = ptr::null_mut();
    let descriptor = json(unsafe { dr_precision_doc_descriptor(doc, &mut err) }, err);
    for key in [
        "cmyk_copy_scratch_bytes",
        "cmyk_copy_merged_scratch_bytes",
        "cmyk_cut_scratch_bytes",
        "cmyk_paste_scratch_bytes",
        "cmyk_paste_convert_scratch_bytes",
        "cmyk_paste_to_rgb_scratch_bytes",
        "cmyk_layer_via_copy_scratch_bytes",
        "cmyk_layer_via_cut_scratch_bytes",
        "cmyk_paste_layers_convert_scratch_bytes",
        "cmyk_stamp_selected_scratch_bytes",
        "cmyk_merge_group_scratch_bytes",
    ] {
        let plan = descriptor[key]
            .as_u64()
            .unwrap_or_else(|| panic!("{key}: {descriptor}"));
        assert!(plan > 0 && plan < LIMIT, "{key} = {plan}");
    }
    free(doc);
}
