//! Black-box suite for 13B-3d-2: the typed RETOUCH / scalar / lock-restore /
//! whole-image writers bind no whole view. The in-place retouch arm (the
//! app's 16-bit Clone Stamp, Healing Brush, Spot Heal, Content-Aware Fill and
//! red-eye) and the transparency-lock restore read and write ONE rect; the
//! whole-image writers (`scalar_edit`'s RGBA arm, mask apply, the destructive
//! adjustment, the selected adjustment, the profile conversion, a new-layer or
//! grown retouch) read rows / runs and answer through `rebuilt_*`.
//!
//! `record_retouch_writers` prints one line per site and case —
//! `retouch <site>/<case> <WxH> <sha256>`, the hash of `null`, `err:<message>`
//! or the answer's target layer (a new-layer or conversion answer: every
//! layer) as dimensions, offset and little-endian samples (u16 codes, f32
//! bits) — so the same file run against HEAD and the tree diffs
//! byte-identical answers. It uses only APIs present at `a33e203`.
//!
//! Classes: RECT sites (every in-place retouch, the three sites on a
//! `layer_lock` 1 layer, the float Cut unlocked and locked) leave no view;
//! since 13B-3d-3a WHOLE sites leave no view either (`rebuilt_*` and the
//! retouch new layer seed nothing), a no-change answer included; the selected
//! FILTER reads its input through `selected_filter`'s residual whole reader
//! (13B-3e) and is recorded only. DAB-shaped RECT sites on `dab` / `seam`
//! share every tile but at most 4 with their source.
//!
//! `no_view_retouch_writers` fails at `63421b3` by design (13B-3d-3a: its
//! WHOLE and no-change arms assert 0); the other three pass there. The view
//! cache is process-wide, so every test holds `serial()`.

use std::ffi::{c_char, CStr, CString};
use std::ptr;

use doctored_core::contiguous_views::contiguous_view_bytes;
use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_clipboard::{dr_precision_doc_clipboard_cut, DrClipboardPayload};
use doctored_core::ffi_color::{dr_builtin_profile, dr_builtin_profile_len};
use doctored_core::ffi_output_color::dr_precision_doc_convert_to_profile_with;
use doctored_core::ffi_precision_color::dr_precision_doc_apply_adjustment;
use doctored_core::ffi_precision_edit::dr_precision_doc_edit;
use doctored_core::ffi_selected_adjustment::dr_precision_doc_selected_edit;
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer as Layer};
use doctored_core::precision_image::{PrecisionImage, PrecisionStorage, REFERENCE_WORKING_BYTES};
use doctored_core::tiff_float::FloatTiff;
use serde_json::{json, Value};

use crate::common::sha256;
use crate::resource_report;
use crate::serial;

const LIMIT: u64 = REFERENCE_WORKING_BYTES;
const CANVAS: (u32, u32) = (1024, 768);
/// The `edge` fixture: a 700×500 layer at (−50, 30) on a 600×400 canvas.
const EDGE_CANVAS: (u32, u32) = (600, 400);
const EDGE_LAYER: ((u32, u32), (i32, i32)) = ((700, 500), (-50, 30));
/// `DR_PROFILE_DISPLAY_P3`, the builtin non-sRGB profile.
const PROFILE_DISPLAY_P3: std::ffi::c_int = 1;

// ------------------------------------------------------------- plumbing --

fn pattern16(width: u32, height: u32, seed: u32) -> Vec<[u16; 4]> {
    (0..width * height)
        .map(|i| {
            let (x, y) = (i % width, i / width);
            [
                ((x * 2711 + y * 911 + seed * 1234) % 65536) as u16,
                ((x * 523 + y * 3301 + seed * 77) % 65536) as u16,
                (((x ^ y) * 1999 + seed) % 65536) as u16,
                (9000 + (x * 3001 + y * 1777 + seed * 97) % 56536) as u16,
            ]
        })
        .collect()
}

/// Extended-range samples with alpha 0, 1 and fractions, row-major.
fn extended(width: u32, height: u32, seed: u32) -> Vec<[f32; 4]> {
    (0..width * height)
        .map(|i| {
            let (x, y) = (i % width, i / width);
            let v = ((x * 37 + y * 101 + seed * 13) % 1000) as f32 / 1000.0;
            let a = match (x + 2 * y) % 11 {
                0 => 0.0,
                1 | 2 => 1.0,
                k => k as f32 / 11.0,
            };
            [v * 6.0 - 1.0, 1.0 - v, ((x ^ y) % 97) as f32 / 24.0, a]
        })
        .collect()
}

fn take(p: *mut c_char) -> String {
    if p.is_null() {
        return "<no error message>".into();
    }
    let text = unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned();
    unsafe { dr_string_free(p) };
    text
}

fn evict() {
    resource_report::evict(&[]).expect("an eviction report");
}

fn builtin_bytes(which: std::ffi::c_int) -> Vec<u8> {
    let len = dr_builtin_profile_len(which);
    let mut out = vec![0u8; len];
    assert!(unsafe { dr_builtin_profile(which, out.as_mut_ptr(), len) });
    out
}

// ------------------------------------------------------------- fixtures --

#[derive(Clone, Copy)]
struct Case {
    name: &'static str,
    canvas: (u32, u32),
    /// The target layer's dimensions and canvas offset.
    layer: ((u32, u32), (i32, i32)),
    /// The dab's canvas rect `(x, y, w, h)`; the coverage is its part on
    /// the canvas.
    dab: (i64, i64, u32, u32),
    empty: bool,
}

impl Case {
    fn new(name: &'static str, dab: (i64, i64, u32, u32)) -> Self {
        Self {
            name,
            canvas: CANVAS,
            layer: (CANVAS, (0, 0)),
            dab,
            empty: false,
        }
    }

    /// The dab's canvas part `(x, y, w, h)`.
    fn clipped(&self) -> (u32, u32, u32, u32) {
        let (x, y, w, h) = self.dab;
        let x0 = x.max(0);
        let y0 = y.max(0);
        let x1 = (x + i64::from(w)).min(i64::from(self.canvas.0));
        let y1 = (y + i64::from(h)).min(i64::from(self.canvas.1));
        (x0 as u32, y0 as u32, (x1 - x0) as u32, (y1 - y0) as u32)
    }

    /// The synthesis hole: at most 64×64, 24 px (or a quarter of the dab)
    /// inside the dab's canvas part — across the tile seam on `seam`.
    fn hole(&self) -> (u32, u32, u32, u32) {
        let (x, y, w, h) = self.clipped();
        let (dx, dy) = ((w / 4).min(24), (h / 4).min(24));
        (x + dx, y + dy, (w - dx).min(64), (h - dy).min(64))
    }
}

fn cases() -> Vec<Case> {
    vec![
        Case::new("dab", (100, 100, 300, 300)),
        Case::new("seam", (200, 200, 300, 300)),
        Case {
            canvas: EDGE_CANVAS,
            layer: EDGE_LAYER,
            ..Case::new("edge", (-20, 0, 120, 90))
        },
        Case {
            layer: ((300, 200), (1100, 50)),
            ..Case::new("offcanvas", (100, 100, 300, 300))
        },
        Case {
            layer: ((300, 200), (600, 500)),
            ..Case::new("missing", (100, 100, 300, 300))
        },
        Case::new("one", (517, 263, 1, 1)),
        Case {
            empty: true,
            ..Case::new("empty", (100, 100, 300, 300))
        },
    ]
}

/// Canvas-sized u16 coverage over `rect`: 0, 65535 and fractions (a 1×1
/// rect is 65535), 0 elsewhere and everywhere for `empty`.
fn coverage_in(case: &Case, (x0, y0, w, h): (u32, u32, u32, u32)) -> Vec<u16> {
    let (cw, ch) = case.canvas;
    let mut out = vec![0_u16; cw as usize * ch as usize];
    if case.empty {
        return out;
    }
    for y in y0..y0 + h {
        for x in x0..x0 + w {
            out[(y * cw + x) as usize] = if w * h == 1 {
                u16::MAX
            } else {
                match (x * 7 + y * 13) % 11 {
                    0 => 0,
                    1..=6 => u16::MAX,
                    k => (k * 6007) as u16,
                }
            };
        }
    }
    out
}

fn coverage(case: &Case) -> Vec<u16> {
    coverage_in(case, case.clipped())
}

/// A canvas-sized selection plane with zero, fractional and full bands.
fn selection(canvas: (u32, u32)) -> Vec<u16> {
    (0..canvas.0 * canvas.1)
        .map(|i| {
            let (x, y) = (i % canvas.0, i / canvas.0);
            match (x / 16 + y / 24) % 4 {
                0 => 0,
                1 => 20000,
                _ => u16::MAX,
            }
        })
        .collect()
}

type Answer = Result<Option<Box<PrecisionDocument>>, String>;

fn edit(doc: &PrecisionDocument, request: &Value, samples: &[u16]) -> Answer {
    let text = CString::new(request.to_string()).unwrap();
    let mut error = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_edit(
            doc,
            text.as_ptr(),
            if samples.is_empty() {
                ptr::null()
            } else {
                samples.as_ptr()
            },
            samples.len(),
            LIMIT,
            0,
            ptr::null(),
            &mut error,
        )
    };
    if !error.is_null() {
        assert!(out.is_null());
        return Err(take(error));
    }
    Ok((!out.is_null()).then(|| unsafe { Box::from_raw(out) }))
}

fn selected(doc: &PrecisionDocument, request: &Value, samples: &[u16]) -> Answer {
    let text = CString::new(request.to_string()).unwrap();
    let mut error = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_selected_edit(
            doc,
            text.as_ptr(),
            samples.as_ptr(),
            samples.len(),
            LIMIT,
            0,
            ptr::null(),
            &mut error,
        )
    };
    if !error.is_null() {
        assert!(out.is_null());
        return Err(take(error));
    }
    Ok((!out.is_null()).then(|| unsafe { Box::from_raw(out) }))
}

fn adjust(doc: &PrecisionDocument, description: &Value) -> Answer {
    let text = CString::new(description.to_string()).unwrap();
    let mut error = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_apply_adjustment(doc, 0, text.as_ptr(), LIMIT, 0, ptr::null(), &mut error)
    };
    if !error.is_null() {
        assert!(out.is_null());
        return Err(take(error));
    }
    Ok((!out.is_null()).then(|| unsafe { Box::from_raw(out) }))
}

fn convert(doc: &PrecisionDocument, profile: &[u8]) -> Answer {
    let options = CString::new("{}").unwrap();
    let mut error = ptr::null_mut();
    let mut report = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_convert_to_profile_with(
            doc,
            profile.as_ptr(),
            profile.len(),
            options.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut report,
            &mut error,
        )
    };
    if !report.is_null() {
        unsafe { dr_string_free(report) };
    }
    if !error.is_null() {
        assert!(out.is_null());
        return Err(take(error));
    }
    Ok((!out.is_null()).then(|| unsafe { Box::from_raw(out) }))
}

fn cut(
    doc: &PrecisionDocument,
    (x, y, width, height): (u32, u32, u32, u32),
    coverage: &[u16],
) -> Answer {
    let text = CString::new(
        json!({"op":"cut","layer":0,"x":x,"y":y,"width":width,"height":height}).to_string(),
    )
    .unwrap();
    let mut payload: *mut DrClipboardPayload = ptr::null_mut();
    let mut error = ptr::null_mut();
    let next = unsafe {
        dr_precision_doc_clipboard_cut(
            doc,
            text.as_ptr(),
            coverage.as_ptr(),
            coverage.len(),
            LIMIT,
            0,
            ptr::null(),
            &mut payload,
            &mut error,
        )
    };
    if !error.is_null() {
        assert!(next.is_null() && payload.is_null());
        return Err(take(error));
    }
    drop(unsafe { Box::from_raw(payload) });
    Ok((!next.is_null()).then(|| unsafe { Box::from_raw(next) }))
}

fn locked(doc: &PrecisionDocument) -> Box<PrecisionDocument> {
    edit(doc, &json!({"op":"layer_lock","layer":0,"locks":1}), &[])
        .expect("a lock")
        .expect("a locked document")
}

/// Layer 0 (the target, the case's geometry) and layer 1 (a canvas-sized
/// clone/heal source), both u16.
fn u16_document(case: &Case) -> Box<PrecisionDocument> {
    let profile = IccProfile::srgb();
    let ((lw, lh), offset) = case.layer;
    let (cw, ch) = case.canvas;
    let image = |w: u32, h: u32, seed: u32| {
        PrecisionImage::from_rgba16(w, h, pattern16(w, h, seed), profile.clone(), None).unwrap()
    };
    let layers = vec![
        Layer::raster("Target", image(lw, lh, 1), offset, 1.0, true).unwrap(),
        Layer::raster("Source", image(cw, ch, 2), (0, 0), 1.0, true).unwrap(),
    ];
    Box::new(
        PrecisionDocument::from_parts(
            cw,
            ch,
            profile.clone(),
            Metadata::default(),
            Resolution::default(),
            layers,
        )
        .unwrap(),
    )
}

/// `u16_document` with a layer-sized mask on the target (`set_mask`).
fn masked_document(case: &Case) -> Box<PrecisionDocument> {
    let doc = u16_document(case);
    let ((lw, lh), (x, y)) = case.layer;
    let mask: Vec<u16> = (0..lw * lh)
        .map(|i| ((i % lw * 131 + i / lw * 71) % 65536) as u16)
        .collect();
    edit(
        &doc,
        &json!({"op":"set_mask","layer":0,"width":lw,"height":lh,"x":x,"y":y}),
        &mask,
    )
    .expect("a mask")
    .expect("a masked document")
}

/// A float document holding one canvas-sized raster, through a float TIFF
/// opened with `open_preserving`.
fn float_document(canvas: (u32, u32)) -> Box<PrecisionDocument> {
    let (width, height) = canvas;
    let tiff = FloatTiff {
        width,
        height,
        channels: 4,
        alpha: Some(false),
        samples: extended(width, height, 5).into_iter().flatten().collect(),
        icc: None,
        resolution: None,
        metadata: Metadata::default(),
        non_finite: 0,
    };
    let bytes = doctored_core::tiff_float::encode_to_vec(&tiff, false).expect("float TIFF");
    let directory = tempfile::TempDir::new().unwrap();
    let path = directory.path().join("retouch.tiff");
    std::fs::write(&path, &bytes).unwrap();
    let doc = PrecisionDocument::open_preserving(path.to_str().unwrap(), LIMIT, 0, None)
        .expect("the float document opens");
    assert!(doc.is_float());
    Box::new(doc)
}

/// The documents one case's sites run on.
struct Documents {
    u16: Box<PrecisionDocument>,
    locked: Box<PrecisionDocument>,
    masked: Box<PrecisionDocument>,
    f32: Box<PrecisionDocument>,
    f32_locked: Box<PrecisionDocument>,
    p3: Vec<u8>,
}

fn documents(case: &Case) -> Documents {
    let u16 = u16_document(case);
    let f32 = float_document(case.canvas);
    Documents {
        locked: locked(&u16),
        masked: masked_document(case),
        f32_locked: locked(&f32),
        u16,
        f32,
        p3: builtin_bytes(PROFILE_DISPLAY_P3),
    }
}

// ---------------------------------------------------------------- sites --

#[derive(Clone, Copy, PartialEq)]
enum Class {
    /// A rect writer: no view.
    Rect,
    /// A whole-image writer: at most the answered rasters' own seeds.
    Whole,
    /// Reads through a residual whole reader (13B-3e): recorded only.
    Residual,
}

struct Site {
    name: &'static str,
    class: Class,
    /// Its answer changes only the dab's tiles (the sharing oracle).
    dab_shaped: bool,
    /// Hash every layer of the answer (new layer, conversion).
    all_layers: bool,
    /// Runs on the float document.
    float: bool,
    run: fn(&Case, &Documents) -> Answer,
}

fn clone_request(selected: bool) -> Value {
    json!({"op":"retouch_clone","layer":0,"source_layer":1,"sample":"current",
        "matrix":[1,0,0,1,37.5,-21.25],"opacity":0.9,"selection":selected})
}

fn with_selection(case: &Case, mut input: Vec<u16>) -> Vec<u16> {
    input.extend(selection(case.canvas));
    input
}

fn synthesis(op: &str, case: &Case, seed: u64) -> Value {
    let (x, y, w, h) = case.hole();
    json!({"op":op,"layer":0,"x":x,"y":y,"width":w,"height":h,"seed":seed})
}

fn invert() -> Value {
    json!({"type":"adjust","op":"invert","params":{},"evaluation":"precision_v1"})
}

fn sites() -> Vec<Site> {
    let site = |name, class, dab_shaped, run| Site {
        name,
        class,
        dab_shaped,
        all_layers: false,
        float: false,
        run,
    };
    vec![
        site("retouch_clone", Class::Rect, true, |c, d| {
            edit(&d.u16, &clone_request(false), &coverage(c))
        }),
        site("retouch_clone+selection", Class::Rect, true, |c, d| {
            edit(
                &d.u16,
                &clone_request(true),
                &with_selection(c, coverage(c)),
            )
        }),
        site("retouch_heal", Class::Rect, true, |c, d| {
            let request = json!({"op":"retouch_heal","layer":0,"source_layer":1,
                "sample":"current","matrix":[1,0,0,1,18,-9],"opacity":0.8});
            edit(&d.u16, &request, &coverage(c))
        }),
        site("spot_heal", Class::Rect, true, |c, d| {
            edit(
                &d.u16,
                &synthesis("spot_heal", c, 12345),
                &coverage_in(c, c.hole()),
            )
        }),
        site("content_aware_fill", Class::Rect, true, |c, d| {
            let input = coverage_in(c, c.hole());
            edit(&d.u16, &synthesis("content_aware_fill", c, 7), &input)
        }),
        site("red_eye", Class::Rect, true, |c, d| {
            let (x, y, w, h) = c.clipped();
            let request = json!({"op":"red_eye","layer":0,"x":x,"y":y,"width":w,"height":h,
                "pupil_size":1.0,"darken":0.5});
            edit(&d.u16, &request, &coverage(c))
        }),
        Site {
            all_layers: true,
            ..site("retouch_clone/new_layer", Class::Whole, false, |c, d| {
                let request = json!({"op":"retouch_clone","layer":0,
                    "sample":"current_and_below","output":"new_layer",
                    "matrix":[1,0,0,1,-3.25,2.5]});
                edit(&d.u16, &request, &coverage(c))
            })
        },
        site("retouch_clone/expand", Class::Whole, false, |c, d| {
            let mut request = clone_request(false);
            request["expand_destination"] = json!(true);
            edit(&d.u16, &request, &coverage(c))
        }),
        site("scalar_edit/red_paint", Class::Whole, false, |c, d| {
            let request = json!({"op":"scalar_edit","target":"red","index":0,
                "operation":{"kind":"paint","value":65535,"opacity":0.8},"selection":false});
            edit(&d.u16, &request, &coverage(c))
        }),
        site("scalar_edit/alpha_fill", Class::Whole, false, |c, d| {
            let request = json!({"op":"scalar_edit","target":"alpha","index":0,
                "operation":{"kind":"fill","value":30000,"opacity":0.7},"selection":true});
            edit(&d.u16, &request, &coverage(c))
        }),
        site("apply_mask", Class::Whole, false, |_, d| {
            edit(&d.masked, &json!({"op":"apply_mask","layer":0}), &[])
        }),
        site("apply_adjustment/invert", Class::Whole, false, |_, d| {
            adjust(&d.u16, &invert())
        }),
        site("apply_adjustment/no_op", Class::Whole, false, |_, d| {
            let description = json!({"type":"adjust","op":"bcs",
                "params":{"brightness":0.0000001},"evaluation":"precision_v1"});
            adjust(&d.u16, &description)
        }),
        site("selected_edit/adjustment", Class::Whole, false, |c, d| {
            let request = json!({"layer":0,"op":"adjustment","description":invert().to_string()});
            selected(&d.u16, &request, &coverage(c))
        }),
        site("selected_edit/filter", Class::Residual, false, |c, d| {
            let request = json!({"layer":0,"op":"high_pass","params":{"radius":2}});
            selected(&d.u16, &request, &coverage(c))
        }),
        Site {
            all_layers: true,
            ..site("convert_to_profile", Class::Whole, false, |_, d| {
                convert(&d.u16, &d.p3)
            })
        },
        site("locked/paint", Class::Rect, true, |c, d| {
            let request = json!({"op":"paint","layer":0,"color":[0.312501,0.500009,0.812503,0.9],
                "color_space":"document","opacity":0.85});
            edit(&d.locked, &request, &coverage(c))
        }),
        site("locked/retouch_clone", Class::Rect, true, |c, d| {
            edit(&d.locked, &clone_request(false), &coverage(c))
        }),
        site("locked/apply_adjustment", Class::Rect, false, |_, d| {
            adjust(&d.locked, &invert())
        }),
        Site {
            float: true,
            ..site("cut_f32", Class::Rect, true, |c, d| {
                cut(&d.f32, c.clipped(), &coverage(c))
            })
        },
        Site {
            float: true,
            ..site("locked/cut_f32", Class::Rect, false, |c, d| {
                cut(&d.f32_locked, c.clipped(), &coverage(c))
            })
        },
    ]
}

/// The document a site's answer derives from.
fn input<'a>(site: &Site, docs: &'a Documents) -> &'a PrecisionDocument {
    match (site.float, site.name.starts_with("locked/")) {
        (true, false) => &docs.f32,
        (true, true) => &docs.f32_locked,
        (false, true) => &docs.locked,
        (false, false) if site.name == "apply_mask" => &docs.masked,
        (false, false) => &docs.u16,
    }
}

fn layer_bytes(doc: &PrecisionDocument, index: usize, out: &mut Vec<u8>) {
    let layer = doc.layer(index).unwrap();
    let Some(image) = layer.source() else {
        out.extend(b"none");
        return;
    };
    let (w, h) = image.dimensions();
    let (x, y) = layer.offset();
    out.extend([w, h].iter().flat_map(|v| v.to_le_bytes()));
    out.extend(x.to_le_bytes());
    out.extend(y.to_le_bytes());
    match image.storage() {
        PrecisionStorage::Rgba16Unorm => out.extend(
            image
                .to_rgba16()
                .unwrap()
                .iter()
                .flat_map(|p| p.iter().flat_map(|c| c.to_le_bytes())),
        ),
        PrecisionStorage::Rgba32Float => out.extend(
            image
                .to_rgba32()
                .unwrap()
                .iter()
                .flat_map(|p| p.iter().flat_map(|c| c.to_bits().to_le_bytes())),
        ),
    }
}

fn answer_bytes(site: &Site, answer: &Answer) -> Vec<u8> {
    match answer {
        Err(e) => format!("err:{e}").into_bytes(),
        Ok(None) => b"null".to_vec(),
        Ok(Some(doc)) => {
            let mut out = Vec::new();
            if site.all_layers {
                out.extend((doc.layer_count() as u64).to_le_bytes());
                for index in 0..doc.layer_count() {
                    layer_bytes(doc, index, &mut out);
                }
            } else {
                layer_bytes(doc, 0, &mut out);
            }
            out
        }
    }
}

// ---------------------------------------------------------------- tests --

#[test]
fn record_retouch_writers() {
    let _serial = serial();
    let sites = sites();
    for case in cases() {
        let docs = documents(&case);
        for site in &sites {
            let answer = (site.run)(&case, &docs);
            if let Err(e) = &answer {
                eprintln!("refused {}/{}: {e}", site.name, case.name);
            }
            let (w, h) = case.canvas;
            println!(
                "retouch {}/{} {w}x{h} {}",
                site.name,
                case.name,
                sha256::sha256_hex(&answer_bytes(site, &answer))
            );
        }
    }
}

#[test]
fn no_view_retouch_writers() {
    let _serial = serial();
    let sites = sites();
    for case in cases() {
        let docs = documents(&case);
        for site in sites.iter().filter(|s| s.class != Class::Residual) {
            evict();
            assert_eq!(contiguous_view_bytes(), 0, "evicted");
            let answer = (site.run)(&case, &docs);
            let views = contiguous_view_bytes();
            let label = format!("{}/{}", site.name, case.name);
            match (site.class, &answer) {
                (Class::Rect, _) => {
                    assert_eq!(views, 0, "{label}: a rect writer leaves no view")
                }
                (_, Ok(Some(_))) => assert_eq!(
                    views, 0,
                    "{label}: a whole-image writer's answer holds no view (13B-3d-3a)"
                ),
                (_, Ok(None)) => {
                    // A no-change `rebuilt_*` (the no-op destructive
                    // adjustment) seeds nothing since 13B-3d-3a, so no view
                    // stays (HEAD `63421b3` left the target's `w·h·8`).
                    assert_eq!(views, 0, "{label}: a no-change answer leaves no view")
                }
                (_, Err(_)) => assert_eq!(views, 0, "{label}: a refusal leaves no view"),
            }
            drop(answer);
        }
    }
}

#[test]
fn retouch_dabs_share_untouched_tiles() {
    let _serial = serial();
    let sites = sites();
    for case in cases()
        .into_iter()
        .filter(|c| c.name == "dab" || c.name == "seam")
    {
        let docs = documents(&case);
        for site in sites.iter().filter(|s| s.dab_shaped) {
            let label = format!("{}/{}", site.name, case.name);
            let answer = (site.run)(&case, &docs);
            let Ok(Some(doc)) = answer else {
                panic!("{label}: a changed document");
            };
            let before = input(site, &docs).layer(0).unwrap().source().unwrap();
            let after = doc.layer(0).unwrap().source().unwrap();
            let (w, h) = before.dimensions();
            let tiles = (w.div_ceil(256) * h.div_ceil(256)) as usize;
            let shared = after.shared_tile_count(before);
            assert!(
                shared + 4 >= tiles,
                "{label}: {shared} of {tiles} tiles shared, at most 4 new"
            );
        }
    }
}

#[test]
fn float_destructive_adjustment_refuses_by_name() {
    let _serial = serial();
    let doc = float_document(CANVAS);
    let Err(refusal) = adjust(&doc, &invert()) else {
        panic!("a float destructive adjustment refuses");
    };
    // A float apply takes the layer's form, so Invert refuses
    // as its layer does, by name.
    assert_eq!(
        refusal,
        "the invert adjustment is unavailable in a 32-bit float document"
    );
}
