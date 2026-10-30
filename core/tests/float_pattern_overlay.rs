//! Pattern Overlay in a 32-bit float document: the tile's codes are
//! the document's own LINEAR values (take1's VERBATIM), so at scale 1 every
//! pixel of the rect is exactly code / 255 of the texel it reads; the float
//! document's one refused part is still an effect blend mode outside its set
//! (`overlay`), by its sentence; the typed `.rz` round trip keeps it; and,
//! when take2's Photoshop files are on this machine, P0 and S37 render within
//! 2 codes of Photoshop's own 32-bit TIFFs inside the rect.

use std::ffi::{c_char, CString};
use std::path::{Path, PathBuf};
use std::ptr;

use doctored_core::ffi_precision_edit::dr_precision_doc_edit;
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer as Layer};
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use serde_json::{json, Value};
use tempfile::TempDir;

use crate::common::take_err_string;

const LIMIT: u64 = REFERENCE_WORKING_BYTES;
const CANVAS: (u32, u32) = (96, 64);
const RECT: (i32, i32, u32, u32) = (13, 9, 50, 40);
const OVERLAY_REFUSAL: &str =
    "the overlay blend mode on the pattern_overlay layer effect is unavailable in a 32-bit float document";

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
            out.push(if i <= chunk.len() {
                T[((n >> (18 - 6 * i)) & 63) as usize] as char
            } else {
                '='
            });
        }
    }
    out
}

/// take1's tile T: 12 x 8, opaque, (20x + 10, 30y + 15, 200 | 60).
fn tile_t() -> (u32, u32, Vec<u8>) {
    let mut rgba = Vec::new();
    for y in 0..8u32 {
        for x in 0..12u32 {
            rgba.extend_from_slice(&[
                (20 * x + 10) as u8,
                (30 * y + 15) as u8,
                if (x + y).is_multiple_of(2) { 200 } else { 60 },
                255,
            ]);
        }
    }
    (12, 8, rgba)
}

fn style(tile: &(u32, u32, Vec<u8>), extra: Value) -> Value {
    let mut effect = json!({"type":"pattern_overlay",
        "pattern":{"name":"T","width":tile.0,"height":tile.1,"rgba":base64(&tile.2)}});
    for (k, v) in extra.as_object().unwrap() {
        effect[k] = v.clone();
    }
    json!({"effects":[effect]})
}

fn image16(w: u32, h: u32, px: [u8; 4]) -> PrecisionImage {
    PrecisionImage::from_rgba16(
        w,
        h,
        vec![px.map(|v| u16::from(v) * 257); (w * h) as usize],
        IccProfile::srgb(),
        None,
    )
    .unwrap()
}

/// A 32-bit float document: a white canvas under a green rect made at RECT.
fn float_doc() -> PrecisionDocument {
    let base = Layer::raster(
        "Base",
        image16(CANVAS.0, CANVAS.1, [255; 4]),
        (0, 0),
        1.0,
        true,
    )
    .unwrap();
    let rect = Layer::raster(
        "Rect",
        image16(RECT.2, RECT.3, [0x30, 0xa0, 0x50, 255]),
        (RECT.0, RECT.1),
        1.0,
        true,
    )
    .unwrap();
    let doc = PrecisionDocument::from_parts(
        CANVAS.0,
        CANVAS.1,
        IccProfile::srgb(),
        Metadata::default(),
        Resolution::default(),
        vec![base, rect],
    )
    .unwrap();
    let float = doc.promote_to_float(LIMIT, 0, None).unwrap();
    assert!(float.is_float());
    float
}

fn set_style(doc: &PrecisionDocument, style: &Value) -> Result<PrecisionDocument, String> {
    let request = json!({"op":"layer_style","version":1,"layer":1,"style":style});
    let c = CString::new(request.to_string()).unwrap();
    let mut err: *mut c_char = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_edit(
            doc,
            c.as_ptr(),
            ptr::null(),
            0,
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    if out.is_null() {
        return Err(take_err_string(err));
    }
    Ok(*unsafe { Box::from_raw(out) })
}

fn render(doc: &PrecisionDocument) -> Vec<[f32; 4]> {
    doc.render(LIMIT, 0, None).unwrap().to_rgba32().unwrap()
}

fn inside(x: u32, y: u32) -> bool {
    let (x, y) = (x as i32, y as i32);
    x >= RECT.0 && y >= RECT.1 && x < RECT.0 + RECT.2 as i32 && y < RECT.1 + RECT.3 as i32
}

#[test]
fn the_tile_is_read_verbatim_as_linear_values() {
    let t = tile_t();
    let doc = set_style(&float_doc(), &style(&t, json!({"phase":[5,2]}))).unwrap();
    let pixels = render(&doc);
    for y in 0..CANVAS.1 {
        for x in 0..CANVAS.0 {
            if !inside(x, y) {
                continue;
            }
            let (tx, ty) = ((x as i32 - 5).rem_euclid(12), (y as i32 - 2).rem_euclid(8));
            let at = ((ty * 12 + tx) * 4) as usize;
            let got = pixels[(y * CANVAS.0 + x) as usize];
            for (c, value) in got.iter().enumerate().take(3) {
                let want = f32::from(t.2[at + c]) / 255.0;
                assert!(
                    (value - want).abs() <= 1e-5,
                    "({x}, {y}) channel {c}: {value} against code {}",
                    t.2[at + c]
                );
            }
        }
    }
}

#[test]
fn an_overlay_blend_refuses_by_its_sentence() {
    let Err(message) = set_style(&float_doc(), &style(&tile_t(), json!({"blend":"overlay"})))
    else {
        panic!("the overlay blend must refuse in a float document");
    };
    assert_eq!(message, OVERLAY_REFUSAL);
}

#[test]
fn the_native_file_round_trips_the_overlay() {
    let dir = TempDir::new().unwrap();
    let doc = set_style(&float_doc(), &style(&tile_t(), json!({"scale":0.37}))).unwrap();
    let path = dir.path().join("float.rz");
    doc.save_native(path.to_str().unwrap(), LIMIT, 0, None)
        .unwrap();
    let back = PrecisionDocument::open_native(path.to_str().unwrap(), LIMIT, 0, None).unwrap();
    assert!(back.is_float());
    assert_eq!(
        back.layer(1).unwrap().reported_style(),
        doc.layer(1).unwrap().reported_style()
    );
    assert_eq!(render(&back), render(&doc));
}

/// take2's 32-bit jobs, when this machine holds them.
fn take2() -> Option<PathBuf> {
    let home = std::env::var_os("HOME")?;
    let dir = Path::new(&home).join("devel/doctored-fixtures/photoshop/pattern-overlay-557/take2");
    dir.join("jobs/rgb32").is_dir().then_some(dir)
}

#[test]
fn photoshops_p0_and_s37_match_inside_the_rect() {
    let Some(take) = take2() else {
        eprintln!("SKIP: take2's rgb32 jobs are not on this machine");
        return;
    };
    let tile = image::open(take.join("inputs/tile.png"))
        .unwrap()
        .to_rgba8();
    let t = (tile.width(), tile.height(), tile.into_raw());
    for (name, scale) in [("p0_rgb32.tif", 1.0), ("s37_rgb32.tif", 0.37)] {
        let doc = set_style(&float_doc(), &style(&t, json!({"scale":scale}))).unwrap();
        let ours = render(&doc);
        let path = take.join("jobs/rgb32").join(name);
        let theirs =
            PrecisionDocument::open_preserving(path.to_str().unwrap(), LIMIT, 0, None).unwrap();
        let theirs = render(&theirs);
        let mut worst = 0f32;
        for y in 0..CANVAS.1 {
            for x in 0..CANVAS.0 {
                if !inside(x, y) {
                    continue;
                }
                let i = (y * CANVAS.0 + x) as usize;
                for c in 0..3 {
                    worst = worst.max((ours[i][c] - theirs[i][c]).abs() * 255.0);
                }
            }
        }
        assert!(worst <= 2.0, "{name}: {worst} codes from Photoshop's TIFF");
    }
}
