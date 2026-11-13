//! `dr_precision_doc_flat_state_losses`: the editable state a flattened
//! file of a typed document has no place for. A plain one-layer Grayscale or
//! CMYK document answers `[]`; each kind of state added through its own
//! export adds its own line and no other. The oracle is the file itself: a
//! Grayscale document carrying the state is written through the kept-gray
//! export and a CMYK one through its TIFF save, each reopened, and the
//! reopened document holds none of what the list named (read through the
//! guide, sampler, channel and layer accessors), while a plain document
//! reopens with nothing to name. The byte list's words and order are
//! `preservation_tests.rs`'s. The pixel lines' scan runs once per plane
//! (`alpha_scans_pending`), and an erased plane answers for its own samples
//! while the handle it came from keeps its answer.

use std::ffi::{CStr, CString};
use std::ptr;

use doctored_core::doc_evaluation::Evaluation;
use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_cmyk::dr_precision_doc_save_cmyk;
use doctored_core::ffi_evaluation::{dr_precision_doc_evaluation, dr_precision_doc_set_evaluation};
use doctored_core::ffi_export::{
    dr_export_result_bytes, dr_export_result_free, dr_precision_doc_export_encode,
};
use doctored_core::ffi_gray::{dr_precision_doc_convert_to_gray, dr_precision_doc_open_gray};
use doctored_core::ffi_guide::{
    dr_precision_doc_add_guide, dr_precision_doc_guide_count, dr_precision_doc_set_ruler_origin,
};
use doctored_core::ffi_precision::dr_precision_doc_open;
use doctored_core::ffi_precision_conversion::dr_precision_doc_promote_to_float;
use doctored_core::ffi_preservation::dr_precision_doc_flat_state_losses;
use doctored_core::ffi_sampler::{
    dr_precision_doc_color_sampler_count, dr_precision_doc_with_color_samplers,
};
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer};
use doctored_core::precision_image::PrecisionImage;
use tempfile::TempDir;

use crate::cmyk_structure_fixture::{
    cmyk_layers, free, then, try_edit, with_levels, with_mask, with_opacity, RgbLayer, LIMIT,
};
use crate::common::{cpath, take_err_string};
use crate::gray_fixture::gray_icc;

const W: u32 = 6;
const H: u32 = 4;

fn opaque() -> Vec<[u16; 4]> {
    (0..W * H)
        .map(|i| {
            let v = (i * 2500 + 1000) as u16;
            [v, v, v, 65535]
        })
        .collect()
}

/// A one-layer 16-bit Grayscale document ("Background") of `pixels`.
fn gray(pixels: Vec<[u16; 4]>) -> *mut PrecisionDocument {
    let image = PrecisionImage::from_rgba16(W, H, pixels, IccProfile::srgb(), None).unwrap();
    to_gray(Box::into_raw(Box::new(
        PrecisionDocument::from_image(image, Metadata::default(), Resolution::default()).unwrap(),
    )))
}

/// `rgb` converted to 16-bit Grayscale, `rgb` freed.
fn to_gray(rgb: *mut PrecisionDocument) -> *mut PrecisionDocument {
    let icc = gray_icc("Preservation Gray", 2.2);
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_convert_to_gray(
            rgb,
            icc.as_ptr(),
            icc.len(),
            c"{\"depth\":16}".as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(!out.is_null(), "to Gray: {}", take_err_string(err));
    free(rgb);
    out
}

/// A one-layer CMYK document ("Background") of `pixels` in the press profile.
fn cmyk(pixels: Vec<[u16; 4]>) -> *mut PrecisionDocument {
    cmyk_layers(
        W,
        H,
        vec![RgbLayer {
            name: "Background",
            w: W,
            h: H,
            offset: (0, 0),
            pixels,
        }],
    )
}

fn losses(doc: *const PrecisionDocument, alpha_kept: bool) -> Vec<String> {
    let mut err = ptr::null_mut();
    let json = unsafe { dr_precision_doc_flat_state_losses(doc, alpha_kept, false, &mut err) };
    assert!(!json.is_null(), "{}", take_err_string(err));
    let list = serde_json::from_str(unsafe { CStr::from_ptr(json) }.to_str().unwrap()).unwrap();
    unsafe { dr_string_free(json) };
    list
}

/// A typed result, the input freed.
fn swap(
    doc: *mut PrecisionDocument,
    next: *mut PrecisionDocument,
    what: &str,
) -> *mut PrecisionDocument {
    assert!(!next.is_null(), "{what} refused");
    free(doc);
    next
}

fn plane() -> Vec<u16> {
    (0..W * H).map(|i| (i * 3000) as u16).collect()
}

type Edit = fn(*mut PrecisionDocument) -> *mut PrecisionDocument;

/// Each kind of state, through its own export, with the one line it adds.
fn states() -> Vec<(&'static str, &'static str, Edit)> {
    vec![
        ("guide", "guides and ruler origin", |d| {
            swap(d, unsafe { dr_precision_doc_add_guide(d, 1, 2.0) }, "guide")
        }),
        ("ruler origin", "guides and ruler origin", |d| {
            swap(
                d,
                unsafe { dr_precision_doc_set_ruler_origin(d, 2.0, 1.0) },
                "ruler",
            )
        }),
        ("alpha channel", "saved selection channels", |d| {
            then(
                d,
                r#"{"op":"set_channel","index":null,"name":"Alpha 1"}"#,
                &plane(),
            )
        }),
        ("sampler", "color samplers", |d| {
            let xys = [2u32, 1, 1];
            swap(
                d,
                unsafe { dr_precision_doc_with_color_samplers(d, xys.as_ptr(), 1) },
                "sampler",
            )
        }),
        ("mask", "layer masks", |d| with_mask(d, 0, &plane())),
        ("opacity", "layer properties", |d| with_opacity(d, 0, 0.5)),
        ("hidden", "layer properties", |d| {
            then(
                d,
                r#"{"op":"layer_visible","layer":0,"visible":false}"#,
                &[],
            )
        }),
        ("blend mode", "layer properties", |d| {
            then(
                d,
                r#"{"op":"layer_blend","version":1,"layer":0,"blend_mode":1}"#,
                &[],
            )
        }),
        ("lock", "layer properties", |d| {
            then(d, r#"{"op":"layer_lock","layer":0,"locks":1}"#, &[])
        }),
        ("label", "layer properties", |d| {
            then(d, r#"{"op":"layer_label","layer":0,"label":3}"#, &[])
        }),
        ("name", "layer properties", |d| {
            then(
                d,
                r#"{"op":"rename_layer","layer":0,"name":"Retouch"}"#,
                &[],
            )
        }),
        // Named "Background" too, so its name is no layer property.
        ("adjustment record", "layers and groups", |d| {
            then(
                with_levels(d, 1.0),
                r#"{"op":"rename_layer","layer":1,"name":"Background"}"#,
                &[],
            )
        }),
    ]
}

#[test]
fn a_plain_gray_or_cmyk_document_loses_nothing() {
    let doc = gray(opaque());
    assert!(losses(doc, true).is_empty(), "{:?}", losses(doc, true));
    assert!(losses(doc, false).is_empty(), "{:?}", losses(doc, false));
    free(doc);
    let doc = cmyk(opaque());
    assert!(losses(doc, false).is_empty(), "{:?}", losses(doc, false));
    free(doc);
}

#[test]
fn each_kind_of_state_names_its_own_line_and_no_other() {
    for (mode, make) in [("gray", gray as fn(_) -> _), ("cmyk", cmyk)] {
        for (what, line, edit) in states() {
            let doc = edit(make(opaque()));
            assert_eq!(
                losses(doc, mode == "gray"),
                vec![line.to_string()],
                "{mode} {what}"
            );
            free(doc);
        }
    }
}

#[test]
fn transparency_counts_only_where_the_file_has_no_alpha() {
    let mut pixels = opaque();
    pixels[3][3] = 30000;
    let doc = gray(pixels.clone());
    assert!(losses(doc, true).is_empty(), "{:?}", losses(doc, true));
    assert_eq!(losses(doc, false), vec!["pixel transparency".to_string()]);
    free(doc);
    let doc = cmyk(pixels);
    assert_eq!(losses(doc, false), vec!["pixel transparency".to_string()]);
    free(doc);
}

/// A kept-gray export of `doc` in `format` (`png` or `jpeg`), reopened.
fn gray_round_trip(
    doc: *const PrecisionDocument,
    format: &str,
    dir: &TempDir,
) -> *mut PrecisionDocument {
    let depth = if format == "jpeg" { 8 } else { 16 };
    let request = CString::new(format!(
        r#"{{"format":"{format}","bit_depth":{depth},"jpeg_quality":100,"profile":"document","dither":"none"}}"#
    ))
    .unwrap();
    let mut err = ptr::null_mut();
    let result = unsafe {
        dr_precision_doc_export_encode(
            doc,
            ptr::null(),
            request.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(!result.is_null(), "{format}: {}", take_err_string(err));
    let mut len = 0usize;
    let bytes = unsafe {
        let p = dr_export_result_bytes(result, &mut len);
        std::slice::from_raw_parts(p, len).to_vec()
    };
    unsafe { dr_export_result_free(result) };
    let path = dir.path().join(format!("flat.{format}"));
    std::fs::write(&path, bytes).unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_open_gray(
            cpath(&path).as_ptr(),
            c"gray_gamma_22".as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(!out.is_null(), "reopen {format}: {}", take_err_string(err));
    out
}

/// The CMYK TIFF save of `doc`, reopened.
fn cmyk_round_trip(doc: *const PrecisionDocument, dir: &TempDir) -> *mut PrecisionDocument {
    let path = dir.path().join("flat.tif");
    let mut err = ptr::null_mut();
    assert!(
        unsafe {
            dr_precision_doc_save_cmyk(
                doc,
                cpath(&path).as_ptr(),
                c"{\"format\":\"tiff\"}".as_ptr(),
                LIMIT,
                0,
                ptr::null(),
                &mut err,
            )
        },
        "{}",
        take_err_string(err)
    );
    let mut err = ptr::null_mut();
    let out =
        unsafe { dr_precision_doc_open(cpath(&path).as_ptr(), LIMIT, 0, ptr::null(), &mut err) };
    assert!(!out.is_null(), "reopen: {}", take_err_string(err));
    out
}

/// What the reopened file kept of the state the list names: guides, the ruler
/// origin, samplers, alpha channels, layers, and the one layer's opacity,
/// visibility, lock, label, name and mask.
fn kept(doc: *const PrecisionDocument) -> Vec<&'static str> {
    let d = unsafe { &*doc };
    let layer = &d.root_layers()[0];
    let mut out = Vec::new();
    if unsafe { dr_precision_doc_guide_count(doc) } > 0 {
        out.push("guides");
    }
    if unsafe { dr_precision_doc_color_sampler_count(doc) } > 0 {
        out.push("samplers");
    }
    if !d.channels().is_empty() {
        out.push("channels");
    }
    if d.root_layers().len() != 1 {
        out.push("layers");
    }
    if layer.opacity() != 1.0 || !layer.visible() || layer.locks() != 0 || layer.label() != 0 {
        out.push("layer properties");
    }
    if layer.name() != "Background" {
        out.push("name");
    }
    if layer.mask().is_some() {
        out.push("mask");
    }
    out
}

#[test]
fn the_flat_file_drops_what_the_list_names_and_a_plain_file_reopens_plain() {
    let dir = TempDir::new().unwrap();
    let rich: Edit = |d| {
        let d = swap(d, unsafe { dr_precision_doc_add_guide(d, 0, 1.0) }, "guide");
        let xys = [2u32, 1, 1];
        let d = swap(
            d,
            unsafe { dr_precision_doc_with_color_samplers(d, xys.as_ptr(), 1) },
            "s",
        );
        let d = then(
            d,
            r#"{"op":"set_channel","index":null,"name":"Alpha 1"}"#,
            &plane(),
        );
        let d = then(d, r#"{"op":"layer_label","layer":0,"label":3}"#, &[]);
        let d = then(d, r#"{"op":"layer_lock","layer":0,"locks":1}"#, &[]);
        let d = then(
            d,
            r#"{"op":"rename_layer","layer":0,"name":"Retouch"}"#,
            &[],
        );
        with_mask(with_opacity(d, 0, 0.5), 0, &plane())
    };
    let named = [
        "layer masks",
        "layer properties",
        "saved selection channels",
        "guides and ruler origin",
        "color samplers",
    ];
    for mode in ["gray", "cmyk"] {
        let make = if mode == "gray" { gray } else { cmyk };
        let doc = rich(make(opaque()));
        assert_eq!(losses(doc, mode == "gray"), named, "{mode}");
        assert_eq!(
            kept(doc),
            [
                "guides",
                "samplers",
                "channels",
                "layer properties",
                "name",
                "mask"
            ],
            "{mode}: the state took"
        );
        let again = if mode == "gray" {
            gray_round_trip(doc, "png", &dir)
        } else {
            cmyk_round_trip(doc, &dir)
        };
        assert!(
            kept(again).is_empty(),
            "{mode}: the file kept {:?}",
            kept(again)
        );
        assert!(
            losses(again, mode == "gray").is_empty(),
            "{mode}: {:?}",
            losses(again, true)
        );
        free(again);
        free(doc);
        let plain = make(opaque());
        let again = if mode == "gray" {
            gray_round_trip(plain, "png", &dir)
        } else {
            cmyk_round_trip(plain, &dir)
        };
        assert!(
            losses(again, mode == "gray").is_empty(),
            "{mode} plain: {:?}",
            losses(again, true)
        );
        free(again);
        free(plain);
    }
}

/// A Grayscale pixel at alpha 0 with a gray value: the kept-gray file writes
/// the composite, whose clear pixels hold 0, so the value is "hidden pixel
/// colors" even in a PNG, which keeps alpha. A JPEG, which keeps none,
/// reopens opaque, which "pixel transparency" names.
#[test]
fn clear_pixels_lose_their_colour_and_a_jpeg_its_transparency() {
    let dir = TempDir::new().unwrap();
    let mut pixels = opaque();
    pixels[0] = [30000, 30000, 30000, 0];
    let doc = gray(pixels);
    let stored = unsafe { &*doc }.root_layers()[0]
        .source()
        .unwrap()
        .rgba16_at(0, 0)
        .unwrap();
    assert!(
        stored[3] == 0 && stored[0] != 0,
        "fixture keeps the clear pixel's gray: {stored:?}"
    );
    assert_eq!(losses(doc, true), vec!["hidden pixel colors".to_string()]);
    assert_eq!(
        losses(doc, false),
        vec![
            "pixel transparency".to_string(),
            "hidden pixel colors".to_string()
        ]
    );
    let png = gray_round_trip(doc, "png", &dir);
    let reopened = unsafe { &*png }.root_layers()[0]
        .source()
        .unwrap()
        .rgba16_at(0, 0)
        .unwrap();
    assert_eq!(reopened[3], 0, "PNG keeps the transparency");
    assert_eq!(reopened[0], 0, "PNG drops the clear pixel's gray");
    free(png);
    let jpeg = gray_round_trip(doc, "jpeg", &dir);
    let reopened = unsafe { &*jpeg }.root_layers()[0]
        .source()
        .unwrap()
        .rgba16_at(0, 0)
        .unwrap();
    assert_eq!(reopened[3], 65535, "JPEG drops the transparency");
    free(jpeg);
    free(doc);
}

/// The linear-light evaluation policy of a 16-bit RGB document is persisted
/// state a flat file has no place for (it reopens `encoded`); an `encoded`
/// document and a 32-bit float one, linear by construction, name nothing.
#[test]
fn a_linear_light_policy_is_named_and_a_float_document_is_not() {
    let image = PrecisionImage::from_rgba16(W, H, opaque(), IccProfile::srgb(), None).unwrap();
    let rgb = Box::into_raw(Box::new(
        PrecisionDocument::from_image(image, Metadata::default(), Resolution::default()).unwrap(),
    ));
    assert!(losses(rgb, true).is_empty(), "{:?}", losses(rgb, true));
    let mut err = ptr::null_mut();
    let linear =
        unsafe { dr_precision_doc_set_evaluation(rgb, Evaluation::LinearLight.code(), &mut err) };
    assert!(!linear.is_null(), "{}", take_err_string(err));
    for alpha_kept in [true, false] {
        assert_eq!(
            losses(linear, alpha_kept),
            vec!["linear-light evaluation".to_string()]
        );
    }
    let mut err = ptr::null_mut();
    let float = unsafe { dr_precision_doc_promote_to_float(rgb, LIMIT, 0, ptr::null(), &mut err) };
    assert!(!float.is_null(), "{}", take_err_string(err));
    assert_eq!(
        unsafe { dr_precision_doc_evaluation(float) },
        Evaluation::LinearLight.code(),
        "a float document evaluates linear"
    );
    assert!(losses(float, true).is_empty(), "{:?}", losses(float, true));
    free(float);
    free(linear);
    free(rgb);
}

#[test]
fn a_null_document_is_refused() {
    unsafe {
        assert!(
            dr_precision_doc_flat_state_losses(ptr::null(), true, true, ptr::null_mut()).is_null()
        );
        let mut err = ptr::null_mut();
        assert!(dr_precision_doc_flat_state_losses(ptr::null(), false, false, &mut err).is_null());
        assert!(take_err_string(err).contains("NULL"));
    }
}

/// A 16-bit RGB document of three `opaque()` layers, each named
/// "Background", so its plain list is "layers and groups" alone.
fn rgb_three() -> *mut PrecisionDocument {
    let srgb = IccProfile::srgb();
    let layers = (0..3)
        .map(|_| {
            let image = PrecisionImage::from_rgba16(W, H, opaque(), srgb.clone(), None).unwrap();
            PrecisionDocumentLayer::raster("Background", image, (0, 0), 1.0, true).unwrap()
        })
        .collect();
    Box::into_raw(Box::new(
        PrecisionDocument::from_parts(
            W,
            H,
            srgb,
            Metadata::default(),
            Resolution::default(),
            layers,
        )
        .unwrap(),
    ))
}

fn float_three() -> *mut PrecisionDocument {
    let rgb = rgb_three();
    let mut err = ptr::null_mut();
    let float = unsafe { dr_precision_doc_promote_to_float(rgb, LIMIT, 0, ptr::null(), &mut err) };
    swap(rgb, float, &format!("to float: {}", take_err_string(err)))
}

fn gray_three() -> *mut PrecisionDocument {
    to_gray(rgb_three())
}

fn cmyk_three() -> *mut PrecisionDocument {
    let layer = || RgbLayer {
        name: "Background",
        w: W,
        h: H,
        offset: (0, 0),
        pixels: opaque(),
    };
    cmyk_layers(W, H, vec![layer(), layer(), layer()])
}

type Make = fn() -> *mut PrecisionDocument;

/// Every typed storage the scan reads: u16 and f32 RGB, Grayscale, CMYK.
const THREE_LAYERS: [(&str, Make); 4] = [
    ("16-bit RGB", rgb_three),
    ("32-bit RGB", float_three),
    ("Gray", gray_three),
    ("CMYK", cmyk_three),
];

/// The Eraser at full strength on pixel 5 of the middle layer: alpha 0 with
/// the colour (or ink) kept, which a flat file writes as zero.
fn erase_one(doc: *const PrecisionDocument) -> *mut PrecisionDocument {
    let mut stroke = vec![0u16; (W * H) as usize];
    stroke[5] = u16::MAX;
    let out = try_edit(
        doc,
        r#"{"op":"erase_native","layer":1,"opacity":1}"#,
        &stroke,
    )
    .unwrap();
    assert!(!out.is_null(), "the erase changed nothing");
    out
}

fn pending(doc: *const PrecisionDocument) -> usize {
    unsafe { &*doc }.alpha_scans_pending()
}

/// The pixel scan behind the list runs once per plane, not once per call: a
/// list leaves no plane pending, a pixel edit leaves only the plane it
/// rewrote, and a property edit or the first handle leaves none.
#[test]
fn each_plane_is_scanned_once_and_a_pixel_edit_rescans_only_its_own() {
    for (mode, make) in THREE_LAYERS {
        let doc = make();
        assert_eq!(pending(doc), 3, "{mode}: a fresh document");
        assert_eq!(losses(doc, true), ["layers and groups"], "{mode}");
        assert_eq!(pending(doc), 0, "{mode}: one list scans every plane");
        let erased = erase_one(doc);
        assert_eq!(pending(erased), 1, "{mode}: the erase rewrote one plane");
        assert_eq!(
            pending(doc),
            0,
            "{mode}: the first handle keeps its answers"
        );
        let renamed = try_edit(
            doc,
            r#"{"op":"rename_layer","layer":2,"name":"Retouch"}"#,
            &[],
        )
        .unwrap();
        assert!(!renamed.is_null(), "{mode}: the rename changed nothing");
        assert_eq!(
            pending(renamed),
            0,
            "{mode}: a property edit shares every plane"
        );
        for d in [renamed, erased, doc] {
            free(d);
        }
    }
}

/// A plane's answer is its own samples': the erased handle names the hidden
/// colour (and, with no alpha in the file, the transparency) while the
/// handle it came from, asked again, still names neither, and both facts are
/// kept, so `alpha_kept` false then true on one handle gives the two lists.
#[test]
fn an_edited_plane_answers_for_its_own_samples() {
    for (mode, make) in THREE_LAYERS {
        let doc = make();
        assert_eq!(losses(doc, false), ["layers and groups"], "{mode}");
        let erased = erase_one(doc);
        assert_eq!(
            losses(erased, false),
            [
                "layers and groups",
                "pixel transparency",
                "hidden pixel colors"
            ],
            "{mode}: erased, no alpha"
        );
        assert_eq!(
            losses(erased, true),
            ["layers and groups", "hidden pixel colors"],
            "{mode}: erased, alpha kept"
        );
        assert_eq!(
            losses(doc, true),
            ["layers and groups"],
            "{mode}: the first handle"
        );
        free(erased);
        free(doc);
    }
}

/// The list's wall-clock record on a 24 MP layered opaque document: the
/// first call, the rest of one `get_document`'s calls (23 at 16 bits, 25 at
/// 32, 11 in Grayscale: a document opened from a flat file, whose
/// `writableTypes` also asks about its own type), and File ▸ Save's at most
/// 9 calls on a freshly built document. Every layer's samples vary per pixel,
/// so no tile is shared or filled. The 32-bit document has two layers: the
/// promotion's reopen admission refuses a third at the 2 GiB process limit
/// this binary keeps.
mod bench {
    use std::ptr;
    use std::time::{Duration, Instant};

    use doctored_core::ffi_gray::dr_precision_doc_convert_to_gray;
    use doctored_core::ffi_precision_conversion::dr_precision_doc_promote_to_float;
    use doctored_core::icc::IccProfile;
    use doctored_core::metadata::{Metadata, Resolution};
    use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer};
    use doctored_core::precision_image::PrecisionImage;

    use super::losses;
    use crate::cmyk_structure_fixture::free;
    use crate::common::take_err_string;
    use crate::gray_fixture::gray_icc;

    const BW: u32 = 6000;
    const BH: u32 = 4000;
    const LAYERS: u32 = 4;
    const FLOAT_LAYERS: u32 = 2;
    /// The working limit the conversions are admitted under: the bench
    /// measures the list, not admission.
    const BIG: u64 = 16 << 30;
    /// Calls 2 to N of one `get_document` on an unchanged document read no
    /// pixel, declared before measuring.
    const REST_BOUND: Duration = Duration::from_millis(5);

    /// Opaque samples varying per pixel, seeded by `seed`.
    fn opaque_varied(seed: u32) -> Vec<[u16; 4]> {
        (0..BW * BH)
            .map(|i| {
                let j = i
                    .wrapping_mul(2_654_435_761)
                    .wrapping_add(seed.wrapping_mul(40_503));
                [j as u16, (j >> 8) as u16, (j >> 16) as u16, u16::MAX]
            })
            .collect()
    }

    fn rgb16() -> *mut PrecisionDocument {
        rgb16_of(LAYERS)
    }

    /// A 16-bit RGB document of `count` such layers, built fresh.
    fn rgb16_of(count: u32) -> *mut PrecisionDocument {
        let srgb = IccProfile::srgb();
        let layers = (0..count)
            .map(|k| {
                let image = PrecisionImage::from_rgba16_unseeded(
                    BW,
                    BH,
                    opaque_varied(k),
                    srgb.clone(),
                    None,
                )
                .unwrap();
                PrecisionDocumentLayer::raster("Background", image, (0, 0), 1.0, true).unwrap()
            })
            .collect();
        Box::into_raw(Box::new(
            PrecisionDocument::from_parts(
                BW,
                BH,
                srgb,
                Metadata::default(),
                Resolution::default(),
                layers,
            )
            .unwrap(),
        ))
    }

    fn float32() -> *mut PrecisionDocument {
        let rgb = rgb16_of(FLOAT_LAYERS);
        let mut err = ptr::null_mut();
        let out = unsafe { dr_precision_doc_promote_to_float(rgb, BIG, 0, ptr::null(), &mut err) };
        assert!(!out.is_null(), "to float: {}", take_err_string(err));
        free(rgb);
        out
    }

    fn gray16() -> *mut PrecisionDocument {
        let rgb = rgb16();
        let icc = gray_icc("Bench Gray", 2.2);
        let mut err = ptr::null_mut();
        let out = unsafe {
            dr_precision_doc_convert_to_gray(
                rgb,
                icc.as_ptr(),
                icc.len(),
                c"{\"depth\":16}".as_ptr(),
                BIG,
                0,
                ptr::null(),
                &mut err,
            )
        };
        assert!(!out.is_null(), "to Gray: {}", take_err_string(err));
        free(rgb);
        out
    }

    /// Each of `calls` lists of `doc` timed, alternating `alpha_kept`.
    fn timed(doc: *const PrecisionDocument, calls: usize) -> (Vec<Duration>, Vec<String>) {
        let mut times = Vec::new();
        let mut first = Vec::new();
        for i in 0..calls {
            let began = Instant::now();
            let list = losses(doc, i % 2 == 0);
            times.push(began.elapsed());
            if i == 0 {
                first = list;
            }
        }
        (times, first)
    }

    fn record(what: &str, layers: u32, make: fn() -> *mut PrecisionDocument, calls: usize) {
        let doc = make();
        let (times, list) = timed(doc, calls);
        free(doc);
        let rest: Duration = times[1..].iter().sum();
        let doc = make();
        let (save, _) = timed(doc, 9);
        free(doc);
        let save: Duration = save.iter().sum();
        eprintln!(
            "flat-save list, {what}, {BW}x{BH}, {layers} opaque layers: first call {:?}; \
             calls 2..{calls} {rest:?}; get_document's {calls} {:?}; Save's 9 on a fresh \
             document {save:?}; list {list:?}",
            times[0],
            times[0] + rest,
        );
        assert!(
            rest <= REST_BOUND,
            "{what}: calls 2..{calls} took {rest:?}, over {REST_BOUND:?}"
        );
    }

    #[test]
    #[ignore = "make cancel-bench: a wall-clock verdict"]
    fn rgb16_lists_after_the_first_read_no_pixel() {
        record("16-bit RGB", LAYERS, rgb16, 23);
    }

    #[test]
    #[ignore = "make cancel-bench: a wall-clock verdict"]
    fn float32_lists_after_the_first_read_no_pixel() {
        record("32-bit RGB", FLOAT_LAYERS, float32, 25);
    }

    #[test]
    #[ignore = "make cancel-bench: a wall-clock verdict"]
    fn gray16_lists_after_the_first_read_no_pixel() {
        record("16-bit Gray", LAYERS, gray16, 11);
    }
}
