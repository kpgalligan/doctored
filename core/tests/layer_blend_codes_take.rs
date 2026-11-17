//! #2: the code-lattice take (`scripts/photoshop-probe/blend_code_forms_2_take.py`
//! take1, Photoshop 26.10.0; fixtures `psd_photoshop/blend_codes_2/`, README)
//! read every code pair of an 8-bit RGB document under Vivid Light and
//! Linear Light (one opaque layer, source grey `y` over the backdrop grey `x`)
//! and under a Divide clip base (an opaque base grey `y` in Divide, a Normal
//! member grey 57 at alpha 96 clipped to it). Doctored's flatten of the same
//! documents, channel 0 against the fixture, holds the exact-pair count the
//! take's judge printed as a floor and its worst code as a ceiling (`TAKE`).
//! On these lattices the float forms differ from the code forms on 15,401,
//! 32,640 and 7,562 pairs, by up to 2, 1 and 5 codes.

use std::ffi::c_char;
use std::path::Path;
use std::ptr;

use doctored_core::doc::DrDocument;
use doctored_core::ffi::{dr_image_free, dr_image_height, dr_image_pixels_rgba, dr_image_width};
use doctored_core::ffi_doc::*;
use image::{Rgba, RgbaImage};
use tempfile::TempDir;

use crate::common::{
    add_layer, apply, apply_or_keep, doc_from, take_err_string, BLEND_DIVIDE, BLEND_LINEAR_LIGHT,
    BLEND_VIVID_LIGHT,
};

/// The lattice side: backdrop grey `x`, source grey `y`.
const N: u32 = 256;
/// The clip member (the measured topology).
const MEMBER_GREY: u8 = 57;
const MEMBER_ALPHA: u8 = 96;

#[derive(Clone, Copy, Debug)]
enum Topology {
    /// One layer in the mode, alpha 255.
    Layer,
    /// The base in the mode, a Normal member clipped to it.
    ClipBase,
}

/// `(mode, fixture, topology, exact pairs floor, max ceiling)` as
/// `blend_code_forms_2_take.py --judge` printed them for take1.
const TAKE: [(i32, &str, Topology, usize, u8); 3] = [
    (
        BLEND_VIVID_LIGHT,
        "vivid_light.png",
        Topology::Layer,
        65536,
        0,
    ),
    (
        BLEND_LINEAR_LIGHT,
        "linear_light.png",
        Topology::Layer,
        65536,
        0,
    ),
    (BLEND_DIVIDE, "divide.png", Topology::ClipBase, 65536, 0),
];

fn grey(v: u8, a: u8) -> Rgba<u8> {
    Rgba([v, v, v, a])
}

/// The take's lattice of `topology` in `mode`, as a document.
fn document(dir: &TempDir, mode: i32, topology: Topology) -> *mut DrDocument {
    let backdrop = RgbaImage::from_fn(N, N, |x, _| grey(x as u8, 255));
    let source = RgbaImage::from_fn(N, N, |_, y| grey(y as u8, 255));
    let doc = doc_from(dir, "back.png", &backdrop);
    let doc = add_layer(dir, "source.png", doc, 0, &source, "Source");
    let doc = apply_or_keep(doc, |d| unsafe { dr_doc_with_layer_blend_mode(d, 1, mode) });
    match topology {
        Topology::Layer => doc,
        Topology::ClipBase => {
            let member = RgbaImage::from_pixel(N, N, grey(MEMBER_GREY, MEMBER_ALPHA));
            let doc = add_layer(dir, "member.png", doc, 1, &member, "Member");
            apply(doc, |d| unsafe { dr_doc_with_layer_clipped(d, 2, true) })
        }
    }
}

/// Channel 0 of the full-frame flatten; frees `doc`.
fn flattened(doc: *mut DrDocument, what: &str) -> Vec<u8> {
    let mut err: *mut c_char = ptr::null_mut();
    let flat = unsafe { dr_doc_flattened(doc, u64::MAX, ptr::null(), &mut err) };
    unsafe { dr_doc_free(doc) };
    assert!(!flat.is_null(), "{what}: {}", take_err_string(err));
    let (w, h) = unsafe { (dr_image_width(flat), dr_image_height(flat)) };
    assert_eq!((w, h), (N, N), "{what}: the lattice's size");
    let pixels = unsafe { dr_image_pixels_rgba(flat) };
    let bytes = unsafe { std::slice::from_raw_parts(pixels, (w * h * 4) as usize) };
    let red = bytes.chunks_exact(4).map(|px| px[0]).collect();
    unsafe { dr_image_free(flat) };
    red
}

fn fixture(name: &str) -> Vec<u8> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/psd_photoshop/blend_codes_2")
        .join(name);
    let img = image::open(&path)
        .unwrap_or_else(|e| panic!("{}: {e}", path.display()))
        .to_luma8();
    assert_eq!(img.dimensions(), (N, N), "{name}: the lattice's size");
    img.into_raw()
}

#[test]
fn the_code_lattice_take_holds_each_confirmed_mode() {
    for (mode, file, topology, floor, ceiling) in TAKE {
        let dir = TempDir::new().unwrap();
        let what = format!("{file} ({topology:?})");
        let ours = flattened(document(&dir, mode, topology), &what);
        let theirs = fixture(file);
        let exact = ours.iter().zip(&theirs).filter(|(a, b)| a == b).count();
        let worst = ours.iter().zip(&theirs).map(|(a, b)| a.abs_diff(*b)).max();
        let first: Vec<_> = (0..ours.len())
            .filter(|&i| ours[i] != theirs[i])
            .take(6)
            .map(|i| (i % N as usize, i / N as usize, ours[i], theirs[i]))
            .collect();
        assert!(
            exact >= floor && worst <= Some(ceiling),
            "{what}: {exact} exact pairs (floor {floor}), worst {worst:?} (ceiling {ceiling}); \
             (b, s, ours, take): {first:?}"
        );
    }
}
