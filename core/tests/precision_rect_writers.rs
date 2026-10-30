//! Black-box suite for 13B-3d-1: the typed PAINT writers — 7D `paint` /
//! `erase` / `clone` / `heal`, the five native tools (`paint_blend`,
//! `erase_native`, `dodge_burn`, `bucket_fill`, `gradient`) and the Cut's
//! erase (u16 through `paint`'s erase, f32 through the float arm) — read and
//! write only the rect their coverage or window can change, so a dab leaves
//! no view, shares every untouched tile and costs the same checkpoints on a
//! small and a large plane.
//!
//! `record_rect_writers` prints one line per site and case —
//! `rect <site>/<case> <WxH> <sha256>`, the hash of `null`, `err:<message>`
//! or the target layer's dimensions, offset and little-endian samples (u16
//! codes, f32 bits) — so the same file run against HEAD and the tree diffs
//! byte-identical answers. It uses only APIs present at `97e5825`.
//!
//! An answer is GROWN when its target layer's dimensions or offset differ
//! from the source's (an expanding native tool whose changed ink leaves the
//! layer): it still rebuilds whole, so it is exempt from the sharing oracle;
//! since 13B-3d-3a its rebuild seeds nothing, so it holds no view either
//! (HEAD `63421b3` held its own `w·h·8`). The tests key on that shape, never
//! on a case name.
//!
//! `no_view_rect_writers` and `dab_checkpoints_are_plane_size_independent`
//! fail at `97e5825` by design; the other two pass there.
//! `no_view_rect_writers`' GROWN arm fails at `63421b3` by design
//! (13B-3d-3a). Since 13B-3d-2 the
//! transparency-lock restore is a rect writer too (`unshared_rect`), so
//! `no_view_rect_writers` covers the `locked` case as well. The view cache is
//! process-wide, so every test holds `serial()`.

use std::ffi::{c_char, CStr, CString};
use std::ptr;

use doctored_core::contiguous_views::contiguous_view_bytes;
use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_clipboard::{dr_precision_doc_clipboard_cut, DrClipboardPayload};
use doctored_core::ffi_precision_edit::dr_precision_doc_edit;
use doctored_core::icc::IccProfile;
use doctored_core::job_control::DrJobControl;
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
/// The `edge` / `grow` / `inside` fixture: a 700×500 layer at (−50, 30) on a
/// 600×400 canvas (it leaves the canvas left, right and below; the canvas
/// rows above y = 30 are off the layer).
const EDGE_CANVAS: (u32, u32) = (600, 400);
const EDGE_LAYER: ((u32, u32), (i32, i32)) = ((700, 500), (-50, 30));
const TILE: u32 = 256;

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

// ------------------------------------------------------------- fixtures --

/// Which sites a case runs.
#[derive(Clone, Copy, PartialEq)]
enum Sites {
    All,
    /// `locked`: 7D paint and erase only.
    PaintErase,
    /// `grow` / `inside`: the three expanding native tools only.
    Expanding,
}

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
    locked: bool,
    sites: Sites,
}

impl Case {
    fn new(name: &'static str, dab: (i64, i64, u32, u32)) -> Self {
        Self {
            name,
            canvas: CANVAS,
            layer: (CANVAS, (0, 0)),
            dab,
            empty: false,
            locked: false,
            sites: Sites::All,
        }
    }

    fn on_edge(mut self) -> Self {
        self.canvas = EDGE_CANVAS;
        self.layer = EDGE_LAYER;
        self
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
}

fn cases() -> Vec<Case> {
    let (w, h) = CANVAS;
    vec![
        Case::new("dab", (100, 100, 300, 300)),
        Case::new("seam", (200, 200, 300, 300)),
        Case::new("corner_top_left", (0, 0, 40, 40)),
        Case::new(
            "corner_bottom_right",
            (i64::from(w) - 40, i64::from(h) - 40, 40, 40),
        ),
        Case::new("one", (517, 263, 1, 1)),
        Case::new("edge", (-20, 0, 120, 90)).on_edge(),
        Case {
            layer: ((300, 200), (1100, 50)),
            ..Case::new("offcanvas", (100, 100, 300, 300))
        },
        Case {
            empty: true,
            ..Case::new("empty", (100, 100, 300, 300))
        },
        Case {
            locked: true,
            sites: Sites::PaintErase,
            ..Case::new("locked", (100, 100, 300, 300))
        },
        Case {
            sites: Sites::Expanding,
            ..Case::new("grow", (300, 0, 100, 100)).on_edge()
        },
        Case {
            sites: Sites::Expanding,
            ..Case::new("inside", (200, 150, 100, 100)).on_edge()
        },
    ]
}

/// Canvas-sized u16 dab coverage: 0, 65535 and fractions inside the dab (a
/// 1×1 dab is 65535), 0 elsewhere and everywhere for `empty`.
fn coverage(case: &Case) -> Vec<u16> {
    let (cw, ch) = case.canvas;
    let mut out = vec![0_u16; cw as usize * ch as usize];
    if case.empty {
        return out;
    }
    let (x0, y0, w, h) = case.clipped();
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

/// A canvas-sized selection plane with zero, fractional and full bands, so
/// a selected dab has zero-coverage samples inside its rect.
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

/// Layer 0 (the target, the case's geometry) and layer 1 (a canvas-sized
/// clone/heal source), both u16; locked through `layer_lock` when the case
/// says so.
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
    let doc = Box::new(
        PrecisionDocument::from_parts(
            cw,
            ch,
            profile.clone(),
            Metadata::default(),
            Resolution::default(),
            layers,
        )
        .unwrap(),
    );
    if !case.locked {
        return doc;
    }
    edit(
        &doc,
        &json!({"op":"layer_lock","layer":0,"locks":1}),
        &[],
        None,
    )
    .expect("a lock")
    .expect("a locked document")
}

/// A float document holding one canvas-sized raster, through a float TIFF
/// opened with `open_preserving`.
fn float_document(canvas: (u32, u32)) -> PrecisionDocument {
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
    let path = directory.path().join("rect.tiff");
    std::fs::write(&path, &bytes).unwrap();
    let doc = PrecisionDocument::open_preserving(path.to_str().unwrap(), LIMIT, 0, None)
        .expect("the float document opens");
    assert!(doc.is_float());
    doc
}

// ---------------------------------------------------------------- sites --

type Answer = Result<Option<Box<PrecisionDocument>>, String>;

fn edit(
    doc: &PrecisionDocument,
    request: &Value,
    samples: &[u16],
    control: Option<&DrJobControl>,
) -> Answer {
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
            control.map_or(ptr::null(), |c| c),
            &mut error,
        )
    };
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
    control: Option<&DrJobControl>,
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
            control.map_or(ptr::null(), |c| c),
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

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    /// A 7D paint request (`paint`, `erase`, `clone`).
    Paint,
    Heal,
    /// A native tool; `expands` for the three that grow the layer.
    Native {
        expands: bool,
        bucket: bool,
    },
    Cut16,
    Cut32,
}

/// A site's request for a case (and whether its selection plane is on).
type Request = fn(&Case, bool) -> Value;

struct Site {
    name: String,
    kind: Kind,
    selection: bool,
    request: Request,
}

/// The heal window: 64×64 at the dab's canvas origin.
fn heal_window(case: &Case) -> (i64, i64, u32, u32) {
    let (x, y, _, _) = case.clipped();
    (i64::from(x), i64::from(y), 64, 64)
}

/// The bucket seed: the first dab sample the stroke (and the selection)
/// cover, else the dab's canvas origin.
fn bucket_seed(case: &Case, selected: bool) -> (u32, u32) {
    let cover = coverage(case);
    let plane = selection(case.canvas);
    let (x0, y0, w, h) = case.clipped();
    for y in y0..y0 + h {
        for x in x0..x0 + w {
            let i = (y * case.canvas.0 + x) as usize;
            if cover[i] > 0 && (!selected || plane[i] > 0) {
                return (x, y);
            }
        }
    }
    (x0, y0)
}

fn sites() -> Vec<Site> {
    let mut out = vec![
        Site {
            name: "paint".into(),
            kind: Kind::Paint,
            selection: false,
            request: |_, _| {
                json!({"op":"paint","layer":0,"color":[0.312501,0.500009,0.812503,0.9],
                    "color_space":"document","opacity":0.85})
            },
        },
        Site {
            name: "erase".into(),
            kind: Kind::Paint,
            selection: false,
            request: |_, _| json!({"op":"erase","layer":0,"opacity":0.6}),
        },
        Site {
            name: "clone".into(),
            kind: Kind::Paint,
            selection: false,
            request: |_, _| json!({"op":"clone","layer":0,"source_layer":1,"dx":37,"dy":-21,"opacity":1.0}),
        },
        Site {
            name: "heal".into(),
            kind: Kind::Heal,
            selection: false,
            request: |case, _| {
                let (x, y, width, height) = heal_window(case);
                json!({"op":"heal","layer":0,"source_layer":1,"dx":11,"dy":9,"x":x,"y":y,
                    "width":width,"height":height,"opacity":0.9})
            },
        },
    ];
    let native: [(&str, bool, bool, Request); 7] = [
        ("paint_blend_normal", true, false, |_, s| blend(0, s)),
        ("paint_blend_multiply", true, false, |_, s| blend(1, s)),
        ("paint_blend_dissolve", true, false, |_, s| blend(14, s)),
        (
            "erase_native",
            false,
            false,
            |_, s| json!({"op":"erase_native","layer":0,"opacity":0.7,"selection":s}),
        ),
        ("dodge_burn", false, false, |_, s| {
            json!({"op":"dodge_burn","layer":0,"exposure":0.5,"range":1,"burn":false,
                "selection":s})
        }),
        ("bucket_fill", true, true, |case, s| {
            let (x, y) = bucket_seed(case, s);
            json!({"op":"bucket_fill","layer":0,"color":[0.9,0.1,0.3,1.0],
                "color_space":"document","opacity":0.8,"blend_mode":0,"x":x,"y":y,
                "tolerance":12000,"contiguous":true,"selection":s})
        }),
        ("gradient", true, false, |case, s| {
            let (x, y, w, h) = case.clipped();
            let (x, y) = (f64::from(x), f64::from(y));
            json!({"op":"gradient","layer":0,"start":[0.0,0.2,0.9,0.6],"end":[1.0,0.8,0.3,1.0],
                "color_space":"document","opacity":0.9,"blend_mode":0,"from":[x,y],
                "to":[x + f64::from(w), y + f64::from(h) / 2.0],"radial":false,"selection":s})
        }),
    ];
    for (name, expands, bucket, request) in native {
        for selected in [false, true] {
            out.push(Site {
                name: if selected {
                    format!("{name}+selection")
                } else {
                    name.into()
                },
                kind: Kind::Native { expands, bucket },
                selection: selected,
                request,
            });
        }
    }
    for (name, kind) in [("cut_u16", Kind::Cut16), ("cut_f32", Kind::Cut32)] {
        out.push(Site {
            name: name.into(),
            kind,
            selection: false,
            request: |_, _| Value::Null,
        });
    }
    out
}

fn blend(mode: u32, selected: bool) -> Value {
    json!({"op":"paint_blend","layer":0,"color":[0.2,0.6,0.9,0.8],"color_space":"document",
        "opacity":0.9,"blend_mode":mode,"selection":selected})
}

fn applies(site: &Site, case: &Case) -> bool {
    match case.sites {
        Sites::All => true,
        Sites::PaintErase => site.name == "paint" || site.name == "erase",
        Sites::Expanding => matches!(site.kind, Kind::Native { expands: true, .. }),
    }
}

/// The documents one case's sites run on.
struct Documents {
    u16: Box<PrecisionDocument>,
    f32: PrecisionDocument,
}

fn documents(case: &Case) -> Documents {
    Documents {
        u16: u16_document(case),
        f32: float_document(case.canvas),
    }
}

fn run(site: &Site, case: &Case, docs: &Documents, control: Option<&DrJobControl>) -> Answer {
    let mut input = coverage(case);
    match site.kind {
        Kind::Cut16 => cut(&docs.u16, case.clipped(), &input, control),
        Kind::Cut32 => cut(&docs.f32, case.clipped(), &input, control),
        _ => {
            if site.selection {
                input.extend(selection(case.canvas));
            }
            edit(
                &docs.u16,
                &(site.request)(case, site.selection),
                &input,
                control,
            )
        }
    }
}

fn source<'a>(site: &Site, docs: &'a Documents) -> &'a PrecisionImage {
    let doc = if site.kind == Kind::Cut32 {
        &docs.f32
    } else {
        &*docs.u16
    };
    doc.layer(0).unwrap().source().unwrap()
}

fn target(doc: &PrecisionDocument) -> (&PrecisionImage, (i32, i32)) {
    let layer = doc.layer(0).unwrap();
    (layer.source().unwrap(), layer.offset())
}

/// The answer's target layer's dimensions or offset differ from the source's.
fn grown(site: &Site, docs: &Documents, answer: &PrecisionDocument) -> bool {
    let doc = if site.kind == Kind::Cut32 {
        &docs.f32
    } else {
        &*docs.u16
    };
    let (before, before_offset) = target(doc);
    let (after, after_offset) = target(answer);
    before.dimensions() != after.dimensions() || before_offset != after_offset
}

fn answer_bytes(answer: &Answer) -> Vec<u8> {
    match answer {
        Err(e) => format!("err:{e}").into_bytes(),
        Ok(None) => b"null".to_vec(),
        Ok(Some(doc)) => {
            let (image, (x, y)) = target(doc);
            let (w, h) = image.dimensions();
            let mut out: Vec<u8> = [w, h].iter().flat_map(|v| v.to_le_bytes()).collect();
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
            out
        }
    }
}

/// Every sample as u64 words (u16 codes, f32 bits), for storage-blind
/// comparison.
fn words(image: &PrecisionImage) -> Vec<[u64; 4]> {
    match image.storage() {
        PrecisionStorage::Rgba16Unorm => image
            .to_rgba16()
            .unwrap()
            .iter()
            .map(|p| p.map(u64::from))
            .collect(),
        PrecisionStorage::Rgba32Float => image
            .to_rgba32()
            .unwrap()
            .iter()
            .map(|p| p.map(|c| u64::from(c.to_bits())))
            .collect(),
    }
}

/// The site's canvas rect: the heal window, else the dab.
fn site_rect(site: &Site, case: &Case) -> (i64, i64, u32, u32) {
    if site.kind == Kind::Heal {
        heal_window(case)
    } else {
        case.dab
    }
}

// ---------------------------------------------------------------- tests --

#[test]
fn record_rect_writers() {
    let _serial = serial();
    let sites = sites();
    for case in cases() {
        let docs = documents(&case);
        for site in sites.iter().filter(|s| applies(s, &case)) {
            let answer = run(site, &case, &docs, None);
            let (w, h) = case.canvas;
            println!(
                "rect {}/{} {w}x{h} {}",
                site.name,
                case.name,
                sha256::sha256_hex(&answer_bytes(&answer))
            );
        }
    }
}

#[test]
fn no_view_rect_writers() {
    let _serial = serial();
    let sites = sites();
    for case in &cases() {
        let docs = documents(case);
        for site in sites.iter().filter(|s| applies(s, case)) {
            evict();
            assert_eq!(contiguous_view_bytes(), 0, "evicted");
            let answer = run(site, case, &docs, None);
            assert_eq!(
                contiguous_view_bytes(),
                0,
                "{}/{}: a rect writer leaves no view (a grown answer too, 13B-3d-3a)",
                site.name,
                case.name
            );
            drop(answer);
        }
    }
}

#[test]
fn rect_writers_share_untouched_tiles() {
    let _serial = serial();
    let sites = sites();
    for case in cases() {
        let docs = documents(&case);
        for site in sites.iter().filter(|s| applies(s, &case)) {
            let label = format!("{}/{}", site.name, case.name);
            let Ok(Some(doc)) = run(site, &case, &docs, None) else {
                continue;
            };
            if grown(site, &docs, &doc) {
                continue;
            }
            let before = source(site, &docs);
            let (after, (ox, oy)) = target(&doc);
            let (w, h) = before.dimensions();
            let (a, b) = (words(before), words(after));
            let (tx, ty) = (w.div_ceil(TILE), h.div_ceil(TILE));
            let mut changed_tiles = vec![false; (tx * ty) as usize];
            let (rx, ry, rw, rh) = site_rect(site, &case);
            for y in 0..h {
                for x in 0..w {
                    let i = (y * w + x) as usize;
                    if a[i] == b[i] {
                        continue;
                    }
                    changed_tiles[((y / TILE) * tx + x / TILE) as usize] = true;
                    let (cx, cy) = (i64::from(x) + i64::from(ox), i64::from(y) + i64::from(oy));
                    assert!(
                        (rx..rx + i64::from(rw)).contains(&cx)
                            && (ry..ry + i64::from(rh)).contains(&cy),
                        "{label}: sample ({x}, {y}) outside the site's rect changed"
                    );
                }
            }
            let changed = changed_tiles.iter().filter(|c| **c).count();
            assert_eq!(
                after.shared_tile_count(before),
                (tx * ty) as usize - changed,
                "{label}: every tile without a changed sample is shared"
            );
        }
    }
}

#[test]
fn dab_checkpoints_are_plane_size_independent() {
    let _serial = serial();
    let sites = sites();
    let small = Case::new("dab", (100, 100, 300, 300));
    let large = Case {
        canvas: (2048, 1536),
        layer: ((2048, 1536), (0, 0)),
        ..small
    };
    let (small_docs, large_docs) = (documents(&small), documents(&large));
    for site in &sites {
        if matches!(
            site.kind,
            Kind::Native { bucket: true, .. } | Kind::Cut16 | Kind::Cut32
        ) {
            continue;
        }
        let count = |case: &Case, docs: &Documents| {
            let control = DrJobControl::default();
            let answer = run(site, case, docs, Some(&control));
            assert!(
                matches!(answer, Ok(Some(_))),
                "{}: a changed document",
                site.name
            );
            control.checkpoints()
        };
        assert_eq!(
            count(&small, &small_docs),
            count(&large, &large_docs),
            "{}: the same dab costs the same checkpoints on a 4× plane",
            site.name
        );
    }
}
