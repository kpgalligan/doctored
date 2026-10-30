//! Black-box suite for 13B-2c-2a: the rect-bounded byte writers (heal,
//! content-aware fill, `with_layer_plane`, `painting_layer_plane`,
//! dodge/burn, bucket fill, Clear, the retouch-output deposit, red-eye) read
//! only their rect and write it back through `with_rect`, so a small edit on
//! a large layer leaves no contiguous view behind and shares every tile it
//! did not change; and `rendered_group`'s synthetic no longer seeds the view
//! cache (`LayerRaster::unseeded`).
//!
//! Oracles are independent of the implementation: the rect every op may
//! write is canvas-box arithmetic done here by hand; the changed tiles are
//! counted by diffing the source against the same op run on a CONTIGUOUS
//! twin (every raster re-adopted through `LayerRaster::shared`, so the
//! single-run read and the copy-then-tile write); expected view bytes are
//! dimension products. The per-op semantics are the existing suites'
//! (`heal_tests`, `content_aware_*`, `redeye_tests`, `selection_tests`,
//! `channel_tests`, `doc_retouch`, `retouch_output`).
//!
//! The view cache is process-wide, so every test holds `serial()`: a count
//! of `contiguous_views` must not see another test's rasters.

use crate::phase4_corpus;
use crate::serial;

use crate::common;

use std::collections::HashSet;
use std::ffi::{c_char, CString};
use std::ptr;
use std::sync::Arc;

use common::{fill_request, styled, take_err_string};
use doctored_core::doc::{BlendMode, DrDocument, LayerKind, LayerRaster};
use doctored_core::doc_lock::LOCK_TRANSPARENCY;
use doctored_core::ffi::dr_image_free;
use doctored_core::ffi_channel::{dr_doc_painting_layer_plane, dr_doc_with_layer_plane};
use doctored_core::ffi_content_aware::dr_doc_content_aware_fill_edit;
use doctored_core::ffi_doc::*;
use doctored_core::ffi_group::dr_doc_with_layer_locks;
use doctored_core::ffi_heal::{dr_doc_heal_layer, dr_doc_heal_to_new_layer, dr_doc_red_eye_layer};
use doctored_core::ffi_retouch_brush::{
    dr_doc_history_brush_layer, dr_doc_retouch_brush_layer, dr_doc_smudge_layer,
};
use doctored_core::layer_raster::contiguous_view_bytes;
use image::{Rgba, RgbaImage};
use serde_json::Value;

use crate::resource_report;

const EDGE: u32 = 256;
const DAB: [u8; 4] = [200, 40, 90, 255];

// ------------------------------------------------------------- plumbing --

/// An owned document handle an FFI op answered (NULL allowed).
struct Out(*mut DrDocument);

impl Drop for Out {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe { dr_doc_free(self.0) };
        }
    }
}

impl Out {
    fn get(&self) -> &DrDocument {
        assert!(!self.0.is_null());
        unsafe { &*self.0 }
    }
}

fn report(document: &DrDocument) -> Value {
    let handles = [document as *const DrDocument];
    resource_report::report(&handles, &[], &[], &[], 0, 0).expect("a resource report")
}

fn evict() {
    resource_report::evict(&[]).expect("an eviction report");
}

fn category(report: &Value, category: &str, field: &str) -> u64 {
    report["categories"][category][field].as_u64().unwrap_or(0)
}

/// Every raster re-adopted as CONTIGUOUS (`LayerRaster::shared` of an owned
/// copy): the same pixels, read through the single-run arm and written
/// through the copy-then-tile arm.
fn contiguous_twin(doc: &DrDocument) -> DrDocument {
    let mut out = doc.clone();
    for layer in &mut out.layers {
        if layer.kind == LayerKind::Raster {
            let copy = layer.pixels.to_contiguous().expect("a copy");
            layer.pixels = LayerRaster::shared(Arc::new(copy));
        }
    }
    out
}

// ------------------------------------------------------------- fixtures --

/// A canvas box `(x0, y0, x1, y1)`, exclusive ends; may leave the canvas.
type CBox = (i64, i64, i64, i64);

/// A layer-space rect `(x, y, w, h)`.
type Rect = (u32, u32, u32, u32);

/// The pixel a prepared source holds at canvas `(x, y)` inside a clipped
/// box, given the box and its centre.
type Paint = fn(CBox, (i64, i64), i64, i64) -> [u8; 4];

struct Fixture {
    doc: DrDocument,
    idx: usize,
}

impl Fixture {
    fn canvas(&self) -> (u32, u32) {
        (self.doc.width, self.doc.height)
    }

    fn layer_size(&self) -> (u32, u32) {
        self.doc.layers[self.idx].pixels.dimensions()
    }

    fn offset(&self) -> (i64, i64) {
        let (x, y) = self.doc.layers[self.idx].offset;
        (i64::from(x), i64::from(y))
    }

    /// `b ∩ canvas ∩ layer`, in layer space.
    fn rect(&self, b: CBox) -> Option<Rect> {
        let (w, h) = self.canvas();
        let (lw, lh) = self.layer_size();
        let (ox, oy) = self.offset();
        let x0 = b.0.max(0).max(ox);
        let y0 = b.1.max(0).max(oy);
        let x1 = b.2.min(i64::from(w)).min(ox + i64::from(lw));
        let y1 = b.3.min(i64::from(h)).min(oy + i64::from(lh));
        (x0 < x1 && y0 < y1).then(|| {
            (
                (x0 - ox) as u32,
                (y0 - oy) as u32,
                (x1 - x0) as u32,
                (y1 - y0) as u32,
            )
        })
    }
}

fn corpus(width: u32, height: u32) -> RgbaImage {
    phase4_corpus::image(width, height)
}

fn with_raster(doc: &DrDocument, pixels: RgbaImage, offset: (i32, i32)) -> DrDocument {
    let mut out = doc
        .adding_image_layer(doc.layers.len() - 1, pixels, "Raster")
        .expect("add layer");
    let top = out.layers.len() - 1;
    out.layers[top].offset = offset;
    out
}

/// The 24 MP corpus as the whole document (layer 0 = the canvas).
fn big() -> Fixture {
    Fixture {
        doc: DrDocument::from_pixels(corpus(
            phase4_corpus::DIMENSIONS[0].0,
            phase4_corpus::DIMENSIONS[0].1,
        )),
        idx: 0,
    }
}

/// A 700×500 layer at (−50, 30) over a 600×400 canvas: it hangs off the
/// left, right and bottom edges.
fn offset() -> Fixture {
    let base = DrDocument::from_pixels(corpus(600, 400));
    let layer = RgbaImage::from_fn(700, 500, |x, y| {
        Rgba([
            ((x * 7 + y * 13) % 256) as u8,
            ((x * 3 + y * 5) % 256) as u8,
            ((x + 2 * y) % 256) as u8,
            255,
        ])
    });
    Fixture {
        doc: with_raster(&base, layer, (-50, 30)),
        idx: 1,
    }
}

/// A 300×200 layer at (700, 0), entirely right of a 600×400 canvas.
fn off_canvas() -> Fixture {
    let base = DrDocument::from_pixels(corpus(600, 400));
    Fixture {
        doc: with_raster(&base, corpus(300, 200), (700, 0)),
        idx: 1,
    }
}

fn clipped(b: CBox, (w, h): (u32, u32)) -> CBox {
    (
        b.0.max(0),
        b.1.max(0),
        b.2.min(i64::from(w)),
        b.3.min(i64::from(h)),
    )
}

fn inside(boxes: &[CBox], x: i64, y: i64) -> bool {
    boxes
        .iter()
        .any(|b| x >= b.0 && x < b.2 && y >= b.1 && y < b.3)
}

/// A canvas-sized buffer of `value` (one or four bytes) over the boxes.
fn canvas_buffer((w, h): (u32, u32), boxes: &[CBox], value: &[u8]) -> Vec<u8> {
    let n = value.len();
    let mut out = vec![0_u8; w as usize * h as usize * n];
    for &b in boxes {
        let (x0, y0, x1, y1) = clipped(b, (w, h));
        for y in y0..y1 {
            for x in x0..x1 {
                let at = (y as usize * w as usize + x as usize) * n;
                out[at..at + n].copy_from_slice(value);
            }
        }
    }
    out
}

// ---------------------------------------------------------------- sites --

#[derive(Clone, Copy, Debug, PartialEq)]
enum Site {
    Plane,
    PaintPlane,
    DodgeBurn,
    RetouchBrush,
    HistoryBrush,
    Smudge,
    Bucket,
    Clear,
    Deposit,
    RedEye { pupil: bool },
    Heal,
    Fill,
}

/// What an op answers.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Answer {
    /// A new document whose target changed inside the boxes.
    Changed,
    /// NULL (and no error message).
    Null,
    /// A new handle holding the same pixels under a fresh identity.
    Unchanged,
}

impl Site {
    /// The layer the op writes (the deposit inserts one above the target).
    fn target(self, idx: usize) -> usize {
        if self == Site::Deposit {
            idx + 1
        } else {
            idx
        }
    }

    /// The source document: the fixture itself, or with the pixels the op
    /// needs painted into the boxes (a uniform block for the bucket, a gray
    /// square with a red pupil for red-eye).
    fn prepare(self, fixture: &Fixture, boxes: &[CBox]) -> DrDocument {
        let mut doc = fixture.doc.clone();
        let paint: Paint = match self {
            Site::Bucket => |_, _, _, _| [10, 200, 30, 254],
            Site::RedEye { pupil } => {
                if pupil {
                    |b, (cx, cy), x, y| {
                        let r = ((b.2 - b.0).min(b.3 - b.1)) / 5;
                        let (dx, dy) = (x - cx, y - cy);
                        if dx * dx + dy * dy <= r * r {
                            [200, 30, 30, 255]
                        } else {
                            [90, 90, 90, 255]
                        }
                    }
                } else {
                    |_, _, _, _| [90, 90, 90, 255]
                }
            }
            _ => return doc,
        };
        let mut image = doc.layers[fixture.idx]
            .pixels
            .to_contiguous()
            .expect("a copy");
        let (ox, oy) = fixture.offset();
        for &b in boxes {
            let Some((rx, ry, rw, rh)) = fixture.rect(b) else {
                continue;
            };
            let c = clipped(b, fixture.canvas());
            let centre = ((c.0 + c.2) / 2, (c.1 + c.3) / 2);
            for ly in ry..ry + rh {
                for lx in rx..rx + rw {
                    let (x, y) = (i64::from(lx) + ox, i64::from(ly) + oy);
                    image.put_pixel(lx, ly, Rgba(paint(c, centre, x, y)));
                }
            }
        }
        doc.layers[fixture.idx].pixels = LayerRaster::new(image);
        doc
    }

    /// Runs the op through the FFI; a fallible op must not report an error.
    fn run(self, doc: &DrDocument, fixture: &Fixture, boxes: &[CBox]) -> Out {
        let d = doc as *const DrDocument;
        let idx = fixture.idx;
        let (w, h) = fixture.canvas();
        let mut error: *mut c_char = ptr::null_mut();
        let out = unsafe {
            match self {
                Site::Plane => {
                    let raster = &doc.layers[idx].pixels;
                    let (ox, oy) = fixture.offset();
                    let mut src = vec![0_u8; w as usize * h as usize];
                    for y in 0..i64::from(h) {
                        for x in 0..i64::from(w) {
                            let Some(p) = u32::try_from(x - ox)
                                .ok()
                                .zip(u32::try_from(y - oy).ok())
                                .and_then(|(lx, ly)| raster.sample_at(lx, ly))
                            else {
                                continue;
                            };
                            let v = if inside(boxes, x, y) {
                                255 - p[0]
                            } else {
                                p[0]
                            };
                            src[(y * i64::from(w) + x) as usize] = v;
                        }
                    }
                    dr_doc_with_layer_plane(d, idx, 0, src.as_ptr(), w, h)
                }
                Site::PaintPlane => {
                    let overlay = canvas_buffer((w, h), boxes, &[255, 255, 255, 255]);
                    dr_doc_painting_layer_plane(d, idx, 0, overlay.as_ptr(), w, h)
                }
                Site::DodgeBurn => {
                    let overlay = canvas_buffer((w, h), boxes, &DAB);
                    dr_doc_dodge_burn_layer(d, idx, overlay.as_ptr(), w, h, 0.5, 1, true, false)
                }
                Site::RetouchBrush => {
                    // Desaturate at full strength: every coloured pixel moves.
                    let overlay = canvas_buffer((w, h), boxes, &DAB);
                    dr_doc_retouch_brush_layer(
                        d,
                        idx,
                        overlay.as_ptr(),
                        w,
                        h,
                        3,
                        1.0,
                        false,
                        false,
                        ptr::null_mut(),
                    )
                }
                Site::HistoryBrush => {
                    // The history state: the same document with the target's
                    // pixels replaced by one colour no fixture holds.
                    let overlay = canvas_buffer((w, h), boxes, &DAB);
                    let (lw, lh) = fixture.layer_size();
                    let mut history = doc.clone();
                    history.layers[idx].pixels = LayerRaster::unseeded(RgbaImage::from_pixel(
                        lw,
                        lh,
                        Rgba([9, 99, 199, 255]),
                    ));
                    dr_doc_history_brush_layer(
                        d,
                        idx,
                        overlay.as_ptr(),
                        w,
                        h,
                        &history as *const DrDocument,
                        idx,
                        1.0,
                    )
                }
                Site::Smudge => {
                    // One diagonal move per box, its dabs sized so the
                    // stroke's footprint stays inside the box.
                    let brush = CString::new(
                        r#"{"version":1,"hardness":1.0,"roundness":1.0,"angle_degrees":0.0,"pixel":false}"#,
                    )
                    .unwrap();
                    let mut dabs = Vec::new();
                    for b in boxes {
                        let r = (b.2 - b.0).min(b.3 - b.1) as f64 / 4.0;
                        dabs.extend([b.0 as f64 + r, b.1 as f64 + r, 2.0 * r, 1.0]);
                        dabs.extend([b.2 as f64 - r, b.3 as f64 - r, 2.0 * r, 1.0]);
                    }
                    dr_doc_smudge_layer(
                        d,
                        idx,
                        brush.as_ptr(),
                        if dabs.is_empty() {
                            ptr::null()
                        } else {
                            dabs.as_ptr()
                        },
                        dabs.len() / 4,
                        0.5,
                        ptr::null(),
                        0,
                        false,
                        &mut error,
                    )
                }
                Site::Bucket => {
                    let (ox, oy) = fixture.offset();
                    let (x, y) = match fixture.rect(boxes[0]) {
                        Some((rx, ry, ..)) => (i64::from(rx) + ox, i64::from(ry) + oy),
                        None => (boxes[0].0, boxes[0].1),
                    };
                    let color = [250_u8, 250, 0, 255];
                    dr_doc_bucket_fill(
                        d,
                        idx,
                        x as i32,
                        y as i32,
                        0,
                        color.as_ptr(),
                        true,
                        ptr::null(),
                    )
                }
                Site::Clear => {
                    let mask = canvas_buffer((w, h), boxes, &[255]);
                    dr_doc_clear_selection(d, idx, mask.as_ptr(), w, h)
                }
                Site::Deposit => {
                    let overlay = canvas_buffer((w, h), boxes, &DAB);
                    let plate = corpus(w, h);
                    let name = CString::new("Retouch").unwrap();
                    dr_doc_heal_to_new_layer(
                        d,
                        idx,
                        overlay.as_ptr(),
                        w,
                        h,
                        1.0,
                        plate.as_raw().as_ptr(),
                        name.as_ptr(),
                        ptr::null(),
                        &mut error,
                    )
                }
                Site::RedEye { .. } => {
                    let b = boxes[0];
                    dr_doc_red_eye_layer(
                        d,
                        idx,
                        b.0 as i32,
                        b.1 as i32,
                        (b.2 - b.0) as u32,
                        (b.3 - b.1) as u32,
                        1.0,
                        0.5,
                    )
                }
                Site::Heal => {
                    let overlay = canvas_buffer((w, h), boxes, &DAB);
                    dr_doc_heal_layer(d, idx, overlay.as_ptr(), w, h, 1.0, ptr::null(), &mut error)
                }
                Site::Fill => {
                    let mask = canvas_buffer((w, h), boxes, &[255]);
                    let request = fill_request(idx, &mask, (w, h), (16, 1), false);
                    dr_doc_content_aware_fill_edit(
                        d,
                        request.as_ptr(),
                        mask.as_ptr(),
                        ptr::null(),
                        &mut error,
                    )
                }
            }
        };
        assert!(
            error.is_null(),
            "{self:?}: unexpected error {}",
            take_err_string(error)
        );
        Out(out)
    }

    /// The layer-space rects the op may write: the boxes on the target.
    fn rects(self, fixture: &Fixture, boxes: &[CBox]) -> Vec<Rect> {
        boxes
            .iter()
            .filter_map(|&b| {
                if self == Site::Deposit {
                    // A canvas-sized layer at (0, 0).
                    let c = clipped(b, fixture.canvas());
                    (c.0 < c.2 && c.1 < c.3).then(|| {
                        (
                            c.0 as u32,
                            c.1 as u32,
                            (c.2 - c.0) as u32,
                            (c.3 - c.1) as u32,
                        )
                    })
                } else {
                    fixture.rect(b)
                }
            })
            .collect()
    }
}

fn tiles((w, h): (u32, u32)) -> usize {
    (w.div_ceil(EDGE) * h.div_ceil(EDGE)) as usize
}

/// The tiles a layer-space rect touches.
fn touched((x, y, w, h): Rect) -> Vec<(u32, u32)> {
    let mut out = Vec::new();
    for ty in y / EDGE..=(y + h - 1) / EDGE {
        for tx in x / EDGE..=(x + w - 1) / EDGE {
            out.push((tx, ty));
        }
    }
    out
}

/// The tiles holding at least one byte that differs between `a` and `b`.
fn changed_tiles(a: &RgbaImage, b: &RgbaImage) -> HashSet<(u32, u32)> {
    assert_eq!(a.dimensions(), b.dimensions());
    let (w, _) = a.dimensions();
    let (ra, rb) = (a.as_raw(), b.as_raw());
    let mut out = HashSet::new();
    for (i, (pa, pb)) in ra.chunks_exact(4).zip(rb.chunks_exact(4)).enumerate() {
        if pa != pb {
            let (x, y) = (i as u32 % w, i as u32 / w);
            out.insert((x / EDGE, y / EDGE));
        }
    }
    out
}

/// Runs `site` over `boxes` on `fixture` and checks the answer: (a) after an
/// eviction the op leaves only the views it must, the changed tiles (the
/// source diffed against the contiguous twin's answer) lie in the tiles the
/// hand-computed rects touch and every other tile is shared with the
/// source, and every byte outside the rects is the source's; (b) every byte
/// equals the twin's answer.
fn check(site: Site, fixture: &Fixture, boxes: &[CBox], answer: Answer, what: &str) {
    let what = format!("{site:?} {what}");
    let source = site.prepare(fixture, boxes);
    let idx = fixture.idx;
    evict();
    let out = site.run(&source, fixture, boxes);
    let views = contiguous_view_bytes();
    match answer {
        Answer::Null => {
            assert!(out.0.is_null(), "{what}: NULL");
            return;
        }
        Answer::Unchanged => {
            assert!(!out.0.is_null(), "{what}: a new handle");
            assert_eq!(views, 0, "{what}: no view");
            let before = &source.layers[idx].pixels;
            let after = &out.get().layers[idx].pixels;
            assert!(
                !LayerRaster::ptr_eq(before, after),
                "{what}: fresh identity"
            );
            assert_eq!(
                after.shared_tile_count(before),
                tiles(before.dimensions()),
                "{what}: every tile shared"
            );
            return;
        }
        Answer::Changed => assert!(!out.0.is_null(), "{what}: an edited document"),
    }
    // A changed answer leaves no view either: since 13B-2c-3a the fill's
    // sampling plate, `inpaint_source.rs`'s `copy_layer`, copies the target
    // layer's rows through `row_segments` instead of binding its view.
    assert_eq!(views, 0, "{what}: views left behind");
    let target = site.target(idx);
    let twin = contiguous_twin(&source);
    let twin_out = site.run(&twin, fixture, boxes);
    assert!(!twin_out.0.is_null(), "{what}: the twin edits too");
    let after = &out.get().layers[target].pixels;
    let after_bytes = after.to_contiguous().expect("a copy");
    let twin_bytes = twin_out.get().layers[target]
        .pixels
        .to_contiguous()
        .expect("a copy");
    // (b) the same op on the contiguous twin.
    assert!(
        after_bytes.as_raw() == twin_bytes.as_raw(),
        "{what}: equals the contiguous twin"
    );
    let before_bytes = if site == Site::Deposit {
        RgbaImage::new(after_bytes.width(), after_bytes.height())
    } else {
        source.layers[idx].pixels.to_contiguous().expect("a copy")
    };
    let rects = site.rects(fixture, boxes);
    // Bytes outside the rects are the source's.
    let (w, _) = after_bytes.dimensions();
    let in_rects = |x: u32, y: u32| {
        rects
            .iter()
            .any(|&(rx, ry, rw, rh)| x >= rx && x < rx + rw && y >= ry && y < ry + rh)
    };
    for (i, (pa, pb)) in after_bytes
        .as_raw()
        .chunks_exact(4)
        .zip(before_bytes.as_raw().chunks_exact(4))
        .enumerate()
    {
        if pa != pb {
            let (x, y) = (i as u32 % w, i as u32 / w);
            assert!(
                in_rects(x, y),
                "{what}: ({x}, {y}) changed outside the rects"
            );
        }
    }
    // The tile oracle.
    let changed = changed_tiles(&before_bytes, &twin_bytes);
    assert!(!changed.is_empty(), "{what}: something changed");
    let bound: HashSet<(u32, u32)> = rects.iter().flat_map(|&r| touched(r)).collect();
    assert!(changed.is_subset(&bound), "{what}: changed ⊆ touched(rect)");
    if site != Site::Deposit {
        let before = &source.layers[idx].pixels;
        assert_eq!(
            after.shared_tile_count(before),
            tiles(before.dimensions()) - changed.len(),
            "{what}: shared = tiles − changed"
        );
    }
}

/// The per-case boxes, in canvas space: a 300×300 dab, a 1×1 rect, a rect
/// across a 256-px seam (canvas and layer alike), a rect touching the canvas
/// corner, a stroke straddling the layer's edge.
fn cases(fixture: &Fixture, big: bool) -> Vec<(&'static str, CBox)> {
    let (w, h) = fixture.canvas();
    let (w, h) = (i64::from(w), i64::from(h));
    vec![
        ("dab", (100, 100, 400, 400)),
        ("1x1", (300, 300, 301, 301)),
        ("seam", (180, 250, 300, 330)),
        ("edge", (w - 60, h - 60, w, h)),
        (
            "straddle",
            if big {
                (-20, -20, 80, 80)
            } else {
                (100, 0, 200, 80)
            },
        ),
    ]
}

/// Every case on the big and the offset fixture, the off-canvas layer and
/// empty coverage. `one` answers the 1×1 case on the big and the offset
/// fixture; `off` / `empty` the last two.
fn run_site(site: Site, one: [Answer; 2], off: Answer, empty: Option<Answer>) {
    for (name, fixture, one) in [("big", big(), one[0]), ("offset", offset(), one[1])] {
        let is_big = name == "big";
        for (case, b) in cases(&fixture, is_big) {
            let answer = if case == "1x1" { one } else { Answer::Changed };
            check(site, &fixture, &[b], answer, &format!("{name} {case}"));
        }
    }
    check(
        site,
        &off_canvas(),
        &[(100, 100, 400, 400)],
        off,
        "off-canvas layer",
    );
    if let Some(answer) = empty {
        let boxes: &[CBox] = if matches!(site, Site::RedEye { .. }) {
            &[(100, 100, 200, 200)]
        } else {
            &[]
        };
        check(site, &offset(), boxes, answer, "empty coverage");
    }
}

// ---------------------------------------------------------------- tests --

#[test]
fn with_layer_plane_writes_only_its_rect() {
    let _serial = serial();
    run_site(
        Site::Plane,
        [Answer::Changed; 2],
        Answer::Null,
        Some(Answer::Null),
    );
}

#[test]
fn painting_layer_plane_writes_only_its_rect() {
    let _serial = serial();
    run_site(
        Site::PaintPlane,
        [Answer::Changed; 2],
        Answer::Null,
        Some(Answer::Null),
    );
}

#[test]
fn dodge_burn_writes_only_its_rect() {
    let _serial = serial();
    run_site(
        Site::DodgeBurn,
        [Answer::Changed; 2],
        Answer::Null,
        Some(Answer::Null),
    );
}

#[test]
fn retouch_brush_writes_only_its_rect() {
    let _serial = serial();
    run_site(
        Site::RetouchBrush,
        [Answer::Changed; 2],
        Answer::Null,
        Some(Answer::Null),
    );
}

#[test]
fn history_brush_writes_only_its_rect() {
    let _serial = serial();
    run_site(
        Site::HistoryBrush,
        [Answer::Changed; 2],
        Answer::Null,
        Some(Answer::Null),
    );
}

#[test]
fn smudge_writes_only_its_footprint() {
    let _serial = serial();
    // A stroke inside one pixel has nothing to drag: its field is that pixel.
    run_site(
        Site::Smudge,
        [Answer::Null; 2],
        Answer::Null,
        Some(Answer::Null),
    );
}

#[test]
fn bucket_fill_writes_only_its_region() {
    let _serial = serial();
    // The seed check refuses an off-layer seed before any rect; a seed is
    // always in its own region, so there is no empty case.
    run_site(Site::Bucket, [Answer::Changed; 2], Answer::Null, None);
}

#[test]
fn clear_selection_writes_only_its_rect() {
    let _serial = serial();
    // A selection that reaches nothing on the layer still succeeds
    // unchanged (the doc comment's promise), off-canvas included.
    run_site(
        Site::Clear,
        [Answer::Changed; 2],
        Answer::Unchanged,
        Some(Answer::Unchanged),
    );
}

#[test]
fn retouch_output_deposits_only_its_rect() {
    let _serial = serial();
    // The deposit writes a new canvas-sized layer from the caller's plate, so
    // the target's own extent does not matter: an off-canvas target still
    // gets its retouch layer. A 1×1 footprint has no interior to heal.
    run_site(
        Site::Deposit,
        [Answer::Null; 2],
        Answer::Changed,
        Some(Answer::Null),
    );
}

#[test]
fn red_eye_writes_only_its_rect() {
    let _serial = serial();
    // A 1×1 rect's only red pixel reaches all four sides of the rect; with
    // no pupil in the rect nothing is red.
    run_site(
        Site::RedEye { pupil: true },
        [Answer::Null; 2],
        Answer::Null,
        None,
    );
    check(
        Site::RedEye { pupil: false },
        &offset(),
        &[(100, 100, 200, 200)],
        Answer::Null,
        "no pupil",
    );
}

#[test]
fn heal_writes_only_its_rect() {
    let _serial = serial();
    // A 1×1 footprint has no interior: every covered pixel is on the join.
    run_site(
        Site::Heal,
        [Answer::Null; 2],
        Answer::Null,
        Some(Answer::Null),
    );
}

#[test]
fn content_aware_fill_writes_only_its_rect() {
    let _serial = serial();
    // The offset layer's pattern is periodic, so a one-pixel hole there is
    // refilled with its own bytes and nothing changes.
    run_site(
        Site::Fill,
        [Answer::Changed, Answer::Null],
        Answer::Null,
        Some(Answer::Null),
    );
}

#[test]
fn heal_and_fill_read_one_union_rect_for_two_components() {
    let _serial = serial();
    for site in [Site::Heal, Site::Fill] {
        check(
            site,
            &big(),
            &[(100, 100, 160, 160), (900, 700, 960, 760)],
            Answer::Changed,
            "big two components",
        );
        check(
            site,
            &offset(),
            &[(60, 60, 100, 100), (400, 300, 450, 350)],
            Answer::Changed,
            "offset two components",
        );
    }
}

#[test]
fn clear_selection_on_a_transparency_locked_layer_is_null() {
    let _serial = serial();
    let fixture = offset();
    let locked = Out(unsafe {
        dr_doc_with_layer_locks(&fixture.doc as *const _, fixture.idx, LOCK_TRANSPARENCY)
    });
    assert!(!locked.0.is_null());
    let (w, h) = fixture.canvas();
    let mask = canvas_buffer((w, h), &[(100, 100, 400, 400)], &[255]);
    let out = unsafe { dr_doc_clear_selection(locked.0, fixture.idx, mask.as_ptr(), w, h) };
    assert!(out.is_null(), "clearing IS an alpha edit");
}

#[test]
fn a_materialized_group_seeds_no_view() {
    let _serial = serial();
    // An isolated styled group (materialized through `rendered_group`),
    // built before the eviction.
    let base = DrDocument::from_pixels(corpus(600, 300));
    let doc = with_raster(
        &base,
        RgbaImage::from_pixel(270, 120, Rgba([40, 90, 200, 180])),
        (300, 150),
    );
    let doc = with_raster(
        &doc,
        RgbaImage::from_pixel(200, 90, Rgba([200, 90, 40, 220])),
        (-20, 220),
    );
    let (mut doc, g, ..) = doc.group_layers(&[1, 2], "Group").expect("group");
    doc.layers[g].blend = BlendMode::Normal;
    let doc = styled(
        &doc,
        g,
        "{\"effects\":[{\"type\":\"drop_shadow\",\"distance\":5,\"size\":2},\
         {\"type\":\"stroke\",\"size\":3}]}",
    );
    evict();
    // 256 live 1×1 rasters fill the cache's entry bound exactly.
    let live: Vec<LayerRaster> = (0..256)
        .map(|i| LayerRaster::new(RgbaImage::from_pixel(1, 1, Rgba([i as u8, 0, 0, 255]))))
        .collect();
    assert_eq!(contiguous_view_bytes(), 1024);
    let mut err: *mut c_char = ptr::null_mut();
    let image = unsafe { dr_doc_flattened(&doc as *const _, 0, std::ptr::null(), &mut err) };
    assert!(!image.is_null(), "flattened: {}", take_err_string(err));
    unsafe { dr_image_free(image) };
    // A seeded synthetic would have evicted one 1×1 view.
    assert_eq!(contiguous_view_bytes(), 1024);
    assert_eq!(
        category(&report(&doc), "contiguous_views", "allocation_count"),
        0
    );
    drop(live);
}

#[test]
fn heal_refuses_an_oversized_window_that_misses_the_layer() {
    let _serial = serial();
    // A 40000×1000 canvas holding one small layer at its right end; a
    // 4-connected staircase from (0, 0) to (33000, 999) has a padded box of
    // 33002 × 1000 — above `MAX_SOLVE_WINDOW_PIXELS` — that misses the layer.
    let mut doc = DrDocument::from_pixels(corpus(100, 100));
    doc.width = 40000;
    doc.height = 1000;
    doc.layers[0].offset = (35000, 400);
    let (w, h) = (doc.width, doc.height);
    let mut overlay = vec![0_u8; w as usize * h as usize * 4];
    for y in 0..1000_usize {
        for x in 33 * y..=33 * y + 33 {
            let at = (y * w as usize + x) * 4;
            overlay[at..at + 4].copy_from_slice(&DAB);
        }
    }
    let mut error: *mut c_char = ptr::null_mut();
    let out = unsafe {
        dr_doc_heal_layer(
            &doc as *const _,
            0,
            overlay.as_ptr(),
            w,
            h,
            1.0,
            ptr::null(),
            &mut error,
        )
    };
    assert!(out.is_null());
    assert_eq!(
        take_err_string(error),
        "the healed region spans a 33002 x 1000 box (33.0 megapixels); healing needs working \
         buffers over that box and is limited to 32 megapixels — heal it in shorter strokes, or \
         select a smaller region."
    );
}

#[test]
fn a_fill_whose_windows_miss_the_layer_is_null_without_an_error() {
    let _serial = serial();
    // The target is a 50×50 layer at (500, 300); the hole at (50, 50) and
    // every window around it are far from it (the 13B-2c-2a decision).
    let base = DrDocument::from_pixels(corpus(600, 400));
    let doc = with_raster(&base, corpus(50, 50), (500, 300));
    let (w, h) = (doc.width, doc.height);
    let mask = canvas_buffer((w, h), &[(50, 50, 70, 70)], &[255]);
    let request = fill_request(1, &mask, (w, h), (16, 1), false);
    let mut error: *mut c_char = ptr::null_mut();
    let out = unsafe {
        dr_doc_content_aware_fill_edit(
            &doc as *const _,
            request.as_ptr(),
            mask.as_ptr(),
            ptr::null(),
            &mut error,
        )
    };
    assert!(out.is_null());
    assert!(error.is_null(), "no error: {}", take_err_string(error));
}

#[test]
fn a_bucket_fill_with_the_regions_own_colour_is_reidentified() {
    let _serial = serial();
    // A uniform 300×300 block filled with its own colour: `with_rect`
    // answers `Ok(None)`, which the bucket maps to `reidentified`.
    let block = [10_u8, 200, 30, 255];
    let mut image = corpus(600, 400);
    for y in 100..400 {
        for x in 100..400 {
            image.put_pixel(x, y, Rgba(block));
        }
    }
    let doc = DrDocument::from_pixels(image);
    evict();
    let out = Out(unsafe {
        dr_doc_bucket_fill(
            &doc as *const _,
            0,
            150,
            150,
            0,
            block.as_ptr(),
            true,
            ptr::null(),
        )
    });
    assert!(!out.0.is_null(), "a new handle");
    assert_eq!(contiguous_view_bytes(), 0, "no view");
    let before = &doc.layers[0].pixels;
    let after = &out.get().layers[0].pixels;
    assert!(!LayerRaster::ptr_eq(before, after), "fresh identity");
    assert!(
        after.to_contiguous().expect("a copy").as_raw()
            == before.to_contiguous().expect("a copy").as_raw(),
        "every byte equal"
    );
    assert_eq!(
        after.shared_tile_count(before),
        tiles(before.dimensions()),
        "every tile shared"
    );
}
