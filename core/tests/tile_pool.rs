//! 13C-1: the content-addressed tile pool (`sha256.rs`, `tile_pool.rs`,
//! `pool_skeleton.rs`, `doc_tile_pool.rs`), black-box through the FFI.
//! Oracles: the FIPS 180-4 / NIST SHA-256 known answers and the test-side
//! digest (`common::sha256`); the native file `dr_doc_save_native` /
//! `save_native` writes for the same document, which a rebuild must equal
//! byte for byte; the samples each test generated; and tile counts taken
//! from the rect geometry (`⌈w/256⌉·⌈h/256⌉`, the tiles a rect meets).

use std::collections::HashSet;
use std::ffi::{c_char, CStr, CString};
use std::path::{Path, PathBuf};
use std::ptr;

use doctored_core::doc::{DrDocument, MaskKind};
use doctored_core::doc_tile_pool::DrTilePool;
use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_doc::{dr_doc_open, dr_doc_save_native};
use doctored_core::ffi_raw_source::dr_doc_attach_raw_source;
use doctored_core::ffi_tile_pool::*;
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::PrecisionDocument;
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use doctored_core::precision_mask_edit::MaskEdit;
use doctored_core::retained_source::{SourceEdit, SourceInput};
use doctored_core::sha256::{digest, Sha256};
use doctored_core::tile_pool::{key, TAG_GRAY16, TAG_RGBA16, TAG_RGBA8};
use image::{Rgba, RgbaImage};
use serde_json::{json, Value};
use tempfile::TempDir;

use crate::cmyk_fixture::press_profile;
use crate::common::sha256::sha256_hex;
use crate::float_fixture::float_doc;

const LIMIT: u64 = REFERENCE_WORKING_BYTES;

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn text(pointer: *mut c_char) -> String {
    if pointer.is_null() {
        return String::new();
    }
    let value = unsafe { CStr::from_ptr(pointer) }
        .to_string_lossy()
        .into_owned();
    unsafe { dr_string_free(pointer) };
    value
}

fn c(path: &Path) -> CString {
    CString::new(path.to_str().unwrap()).unwrap()
}

fn splitmix(state: &mut u64) -> u64 {
    *state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut z = *state;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

fn noise8(w: u32, h: u32, seed: u64) -> RgbaImage {
    let mut state = seed;
    let mut out = RgbaImage::new(w, h);
    for p in out.pixels_mut() {
        p.0 = (splitmix(&mut state) as u32).to_le_bytes();
    }
    out
}

/// Every pixel's four u16 components from `sample(i, component)`.
fn plane16(w: u32, h: u32, mut sample: impl FnMut(usize) -> u16) -> Vec<[u16; 4]> {
    (0..(w * h) as usize)
        .map(|_| [sample(0), sample(1), sample(2), sample(3)])
        .collect()
}

fn typed(w: u32, h: u32, pixels: Vec<[u16; 4]>) -> PrecisionDocument {
    let image = PrecisionImage::from_rgba16(w, h, pixels, IccProfile::srgb(), None).unwrap();
    PrecisionDocument::from_image(image, Metadata::default(), Resolution { x: 72., y: 72. })
        .unwrap()
}

fn noise16(w: u32, h: u32, seed: u64) -> Vec<[u16; 4]> {
    let mut state = seed;
    plane16(w, h, |_| splitmix(&mut state) as u16)
}

struct Pool(*mut DrTilePool);

impl Drop for Pool {
    fn drop(&mut self) {
        unsafe { dr_tile_pool_free(self.0) };
    }
}

impl Pool {
    fn open(dir: &Path) -> Self {
        let mut err = ptr::null_mut();
        let pool = unsafe { dr_tile_pool_open(c(dir).as_ptr(), &mut err) };
        assert!(!pool.is_null(), "{}", text(err));
        Self(pool)
    }

    fn write_byte(&self, doc: &DrDocument, skeleton: &Path) -> Result<(), String> {
        let mut err = ptr::null_mut();
        let path = c(skeleton);
        let ok =
            unsafe { dr_doc_tile_pool_write(doc, self.0, path.as_ptr(), ptr::null(), &mut err) };
        if ok {
            Ok(())
        } else {
            Err(text(err))
        }
    }

    fn write_typed(&self, doc: &PrecisionDocument, skeleton: &Path) -> Result<(), String> {
        let mut err = ptr::null_mut();
        let path = c(skeleton);
        let ok = unsafe {
            dr_precision_doc_tile_pool_write(
                doc,
                self.0,
                path.as_ptr(),
                LIMIT,
                0,
                ptr::null(),
                &mut err,
            )
        };
        if ok {
            Ok(())
        } else {
            Err(text(err))
        }
    }

    fn rebuild(&self, skeleton: &Path, native: &Path) -> Result<Vec<u8>, String> {
        let mut err = ptr::null_mut();
        let (s, n) = (c(skeleton), c(native));
        let ok =
            unsafe { dr_tile_pool_rebuild(self.0, s.as_ptr(), n.as_ptr(), ptr::null(), &mut err) };
        if ok {
            Ok(std::fs::read(native).unwrap())
        } else {
            Err(text(err))
        }
    }

    fn collect(&self, skeletons: &[&Path]) -> Result<(), String> {
        let owned: Vec<CString> = skeletons.iter().map(|p| c(p)).collect();
        let pointers: Vec<*const c_char> = owned.iter().map(|s| s.as_ptr()).collect();
        let mut err = ptr::null_mut();
        let ok =
            unsafe { dr_tile_pool_collect(self.0, pointers.as_ptr(), pointers.len(), &mut err) };
        if ok {
            Ok(())
        } else {
            Err(text(err))
        }
    }

    fn info(&self) -> Value {
        let mut err = ptr::null_mut();
        let json = unsafe { dr_tile_pool_info(self.0, &mut err) };
        assert!(!json.is_null(), "{}", text(err));
        serde_json::from_str(&text(json)).unwrap()
    }
}

fn count(info: &Value, key: &str) -> u64 {
    info[key]
        .as_u64()
        .unwrap_or_else(|| panic!("{key} in {info}"))
}

fn saved_byte(doc: &DrDocument, path: &Path) -> Vec<u8> {
    let mut err = ptr::null_mut();
    let ok = unsafe { dr_doc_save_native(doc, c(path).as_ptr(), ptr::null(), &mut err) };
    assert!(ok, "{}", text(err));
    std::fs::read(path).unwrap()
}

fn saved_typed(doc: &PrecisionDocument, path: &Path) -> Vec<u8> {
    doc.save_native(path.to_str().unwrap(), LIMIT, 0, None)
        .unwrap();
    std::fs::read(path).unwrap()
}

fn open_byte(path: &Path) -> Box<DrDocument> {
    let mut err = ptr::null_mut();
    let doc = unsafe { dr_doc_open(c(path).as_ptr(), ptr::null(), &mut err) };
    assert!(!doc.is_null(), "{}", text(err));
    unsafe { Box::from_raw(doc) }
}

/// A file's path in the test folder.
fn at(dir: &TempDir, name: &str) -> PathBuf {
    dir.path().join(name)
}

/// The tiles of a `w × h` plane.
fn tiles(w: u32, h: u32) -> u64 {
    u64::from(w.div_ceil(256)) * u64::from(h.div_ceil(256))
}

fn noise_doc(w: u32, h: u32, seed: u64) -> DrDocument {
    DrDocument::from_pixels(noise8(w, h, seed))
}

// ---------------------------------------------------------------- SHA-256 --

#[test]
fn sha256_matches_the_known_answers() {
    // FIPS 180-4 / NIST CSRC example values: the empty message, "abc", the
    // 448-bit and 896-bit messages, and one million "a".
    let cases: [(&[u8], &str); 4] = [
        (b"", "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"),
        (b"abc", "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"),
        (
            b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq",
            "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1",
        ),
        (
            b"abcdefghbcdefghicdefghijdefghijkefghijklfghijklmghijklmnhijklmnoijklmnopjklmnopqklmnopqrlmnopqrsmnopqrstnopqrstu",
            "cf5b16a778af8380036ce59e7b0492370b249b11e8f07a51afac45037afee9d1",
        ),
    ];
    for (message, expected) in cases {
        assert_eq!(
            hex(&digest(message)),
            expected,
            "{:?}",
            String::from_utf8_lossy(message)
        );
    }
    let million = vec![b'a'; 1_000_000];
    assert_eq!(
        hex(&digest(&million)),
        "cdc76e5c9914fb9281a1c7e284d73e67f1809a48a497200e046d39ccc7112cd0"
    );
    // Split updates, at every boundary a block and its padding can fall on,
    // equal the one-shot digest and the test-side oracle.
    let mut state = 7;
    let data: Vec<u8> = (0..300).map(|_| splitmix(&mut state) as u8).collect();
    for len in 0..data.len() {
        let message = &data[..len];
        let whole = hex(&digest(message));
        assert_eq!(whole, sha256_hex(message), "length {len}");
        for split in [0, 1, len / 3, len / 2, len.saturating_sub(1)] {
            let split = split.min(len);
            let mut hasher = Sha256::new();
            hasher.update(&message[..split]);
            hasher.update(&message[split..]);
            assert_eq!(hex(&hasher.finish()), whole, "length {len} split {split}");
        }
    }
}

#[test]
fn the_key_separates_sample_type_and_size() {
    let bytes = [1_u8, 2, 3, 4, 5, 6, 7, 8];
    // The derivation, against the oracle: "DRTP", tag, w LE, h LE, bytes.
    let mut message = b"DRTP".to_vec();
    message.push(TAG_RGBA16);
    message.extend_from_slice(&1_u32.to_le_bytes());
    message.extend_from_slice(&1_u32.to_le_bytes());
    message.extend_from_slice(&bytes);
    assert_eq!(hex(&key(TAG_RGBA16, 1, 1, &bytes)), sha256_hex(&message));
    let keys: HashSet<[u8; 32]> = [
        key(TAG_RGBA16, 1, 1, &bytes),
        key(TAG_RGBA8, 2, 1, &bytes),
        key(TAG_RGBA8, 1, 2, &bytes),
        key(TAG_GRAY16, 4, 1, &bytes),
        key(TAG_GRAY16, 2, 2, &bytes),
    ]
    .into_iter()
    .collect();
    assert_eq!(
        keys.len(),
        5,
        "equal bytes of another type or shape share a key"
    );
}

// ------------------------------------------------------------ round trips --

/// 600×300 (no edge of a multiple of 256): a white Background, a placed
/// noise layer holding its 8-bit source, a reveal-all mask on it, an alpha
/// channel and a group around the placed layer.
fn byte_fixture() -> (DrDocument, RgbaImage) {
    let (w, h) = (600, 300);
    let background = DrDocument::from_pixels(RgbaImage::from_pixel(w, h, Rgba([255; 4])));
    let picture = noise8(w, h, 0x5EED);
    let source = DrDocument::from_pixels(picture.clone());
    let edit = SourceEdit::parse(
        &json!({"op":"place","source_id":"n","revision_id":"a","name":"Noise","x":0,"y":0})
            .to_string(),
    )
    .unwrap();
    let placed = background
        .source_edit(Some(&source), &edit, LIMIT, 0, None)
        .unwrap()
        .unwrap();
    let channel: Vec<u8> = noise8(w, h, 3).into_raw().into_iter().step_by(4).collect();
    let doc = placed
        .add_mask(1, MaskKind::RevealAll)
        .unwrap()
        .add_channel("Alpha 1", &channel, w, h, [255, 0, 0], 0.5)
        .unwrap();
    let (grouped, ..) = doc.group_layers(&[1], "Group").unwrap();
    (grouped, picture)
}

#[test]
fn a_byte_document_rebuilds_to_its_saved_file() {
    let dir = TempDir::new().unwrap();
    let (doc, picture) = byte_fixture();
    let saved = saved_byte(&doc, &at(&dir, "saved.doctored"));
    let pool = Pool::open(&at(&dir, "pool"));
    pool.write_byte(&doc, &at(&dir, "state.drskel")).unwrap();
    let info = pool.info();
    assert!(count(&info, "appended_tiles") > 0, "{info}");
    // The mask, the channel and the 8-bit source are tile segments too
    // (skeleton version 2): nothing is chunked.
    assert_eq!(count(&info, "appended_chunks"), 0, "{info}");
    let rebuilt = pool
        .rebuild(&at(&dir, "state.drskel"), &at(&dir, "rebuilt.doctored"))
        .unwrap();
    assert!(
        rebuilt == saved,
        "the rebuilt file differs from the saved one"
    );
    let reopened = open_byte(&at(&dir, "rebuilt.doctored"));
    let noise = reopened
        .layers
        .iter()
        .find(|layer| layer.name == "Noise")
        .unwrap();
    assert_eq!(
        noise.pixels.to_contiguous().unwrap().as_raw(),
        picture.as_raw()
    );
}

/// Two u16 records (noise), a u16 mask and one channel (each 300·250·2 =
/// 150,000 bytes, a blob), and the lower record's retained 16-bit master.
fn u16_fixture() -> (PrecisionDocument, Vec<[u16; 4]>) {
    let (w, h) = (300_u32, 250_u32);
    let pixels = noise16(w, h, 0x2545_F491_4F6C_DD1D);
    let retained = typed(w, h, pixels.clone())
        .source_edit(
            None,
            &SourceEdit::parse(r#"{"op":"retain","source_id":"pool13c1"}"#).unwrap(),
            LIMIT,
            0,
            None,
        )
        .unwrap()
        .unwrap();
    let two = retained.duplicate_layer(0).unwrap().unwrap();
    let mask_edit = |doc: &PrecisionDocument, request: Value, samples: &[u16]| {
        doc.mask_edit(
            &MaskEdit::parse(&request.to_string()).unwrap(),
            samples,
            LIMIT,
            0,
            None,
        )
        .unwrap()
        .unwrap()
    };
    let ramp: Vec<u16> = (0..w * h).map(|i| (i * 21) as u16).collect();
    let masked = mask_edit(
        &two,
        json!({"op":"set_mask","layer":1,"width":w,"height":h,"x":0,"linked":false,
            "density":1.0,"feather":0.0,"enabled":true}),
        &ramp,
    );
    let coverage: Vec<u16> = (0..w * h).map(|i| 65_535 - (i * 13) as u16).collect();
    let doc = mask_edit(
        &masked,
        json!({"op":"set_channel","index":null,"name":"Coverage"}),
        &coverage,
    );
    (doc, pixels)
}

#[test]
fn typed_documents_rebuild_to_their_saved_files() {
    let dir = TempDir::new().unwrap();
    let pool = Pool::open(&at(&dir, "pool"));
    // u16 with a mask, a channel and a retained master.
    let (u16_doc, u16_pixels) = u16_fixture();
    // f32 (two generated records, values in [0, 1] and alpha 1).
    let (fw, fh) = (290_u32, 270_u32);
    let mut state = 11;
    let mut float_pixels = || -> Vec<[f32; 4]> {
        (0..fw * fh)
            .map(|_| {
                let v = (splitmix(&mut state) >> 40) as f32 / (1 << 24) as f32;
                [v, 1.0 - v, v * 0.5, 1.0]
            })
            .collect()
    };
    let float_records = [float_pixels(), float_pixels()];
    let float = float_doc(fw, fh, &IccProfile::srgb(), &float_records);
    // CMYK (a filled 16-bit document).
    let press = press_profile("Pool Press");
    let cmyk = PrecisionDocument::new_cmyk(
        300,
        200,
        Some(&press),
        r#"{"depth":16,"fill":[0.2,0.4,0.6,0.1,1.0],"layer_name":"Paper","ppi":72}"#,
        LIMIT,
        0,
    )
    .unwrap();
    for (name, doc) in [("u16", &u16_doc), ("float", &float), ("cmyk", &cmyk)] {
        let saved = saved_typed(doc, &at(&dir, &format!("{name}.doctored")));
        let skeleton = at(&dir, &format!("{name}.drskel"));
        pool.write_typed(doc, &skeleton).unwrap();
        assert!(count(&pool.info(), "appended_tiles") > 0, "{name}");
        let rebuilt = pool
            .rebuild(&skeleton, &at(&dir, &format!("{name}-rebuilt.doctored")))
            .unwrap();
        assert!(
            rebuilt == saved,
            "{name}: the rebuilt file differs from the saved one"
        );
    }
    let reopen = |name: &str| {
        let path = at(&dir, &format!("{name}-rebuilt.doctored"));
        PrecisionDocument::open_native(path.to_str().unwrap(), LIMIT, 0, None).unwrap()
    };
    let u16_back = reopen("u16");
    assert_eq!(
        u16_back.layer_source(0).unwrap().to_rgba16().unwrap(),
        u16_pixels
    );
    let float_back = reopen("float");
    for (index, record) in float_records.iter().enumerate() {
        let back = float_back.layer_source(index).unwrap().to_rgba32().unwrap();
        let same = back
            .iter()
            .flatten()
            .zip(record.iter().flatten())
            .all(|(a, b)| a.to_bits() == b.to_bits());
        assert!(same && back.len() == record.len(), "float record {index}");
    }
    assert_eq!(reopen("cmyk").dimensions(), (300, 200));
}

// ------------------------------------------------------------------ dedup --

#[test]
fn a_second_write_appends_nothing() {
    let dir = TempDir::new().unwrap();
    let doc = noise_doc(600, 300, 1);
    let pool = Pool::open(&at(&dir, "pool"));
    pool.write_byte(&doc, &at(&dir, "a.drskel")).unwrap();
    let first = pool.info();
    assert_eq!(count(&first, "appended_tiles"), tiles(600, 300));
    assert_eq!(count(&first, "hashed_tiles"), tiles(600, 300));
    pool.write_byte(&doc, &at(&dir, "b.drskel")).unwrap();
    let again = pool.info();
    assert_eq!(count(&again, "appended_tiles"), 0, "{again}");
    assert_eq!(
        count(&again, "hashed_tiles"),
        0,
        "the memo answers every tile: {again}"
    );
    assert_eq!(count(&again, "stored_bytes"), count(&first, "stored_bytes"));
    drop(pool);
    // A fresh handle keys every tile again and finds each in the pool.
    let fresh = Pool::open(&at(&dir, "pool"));
    fresh.write_byte(&doc, &at(&dir, "c.drskel")).unwrap();
    let info = fresh.info();
    assert_eq!(count(&info, "appended_tiles"), 0, "{info}");
    assert_eq!(count(&info, "hashed_tiles"), tiles(600, 300), "{info}");
    let a = std::fs::read(at(&dir, "a.drskel")).unwrap();
    assert!(
        a == std::fs::read(at(&dir, "c.drskel")).unwrap(),
        "same document, same skeleton"
    );
}

#[test]
fn an_edit_appends_only_the_tiles_it_meets() {
    let dir = TempDir::new().unwrap();
    let doc = noise_doc(1024, 1024, 2)
        .adding_image_layer(0, noise8(1024, 1024, 3), "Top")
        .unwrap();
    let pool = Pool::open(&at(&dir, "pool"));
    pool.write_byte(&doc, &at(&dir, "a.drskel")).unwrap();
    assert_eq!(count(&pool.info(), "appended_tiles"), 2 * tiles(1024, 1024));
    // A 300×300 rect at (300, 300) meets columns 1–2 and rows 1–2.
    let (x, y, w, h) = (300_u32, 300_u32, 300_u32, 300_u32);
    let met =
        u64::from((x + w - 1) / 256 - x / 256 + 1) * u64::from((y + h - 1) / 256 - y / 256 + 1);
    let mut state = 4;
    let samples: Vec<[u8; 4]> = (0..w * h)
        .map(|_| (splitmix(&mut state) as u32).to_le_bytes())
        .collect();
    let mut edited = doc.clone();
    edited.layers[1].pixels = doc.layers[1]
        .pixels
        .with_rect(x, y, w, h, &samples)
        .unwrap()
        .unwrap();
    pool.write_byte(&edited, &at(&dir, "b.drskel")).unwrap();
    let info = pool.info();
    assert_eq!(count(&info, "appended_tiles"), met, "{info}");
    assert_eq!(
        count(&info, "hashed_tiles"),
        met,
        "unchanged tiles are memo hits: {info}"
    );
    let saved = saved_byte(&edited, &at(&dir, "b.doctored"));
    assert!(
        pool.rebuild(&at(&dir, "b.drskel"), &at(&dir, "b-rebuilt.doctored"))
            .unwrap()
            == saved
    );
    let saved = saved_byte(&doc, &at(&dir, "a.doctored"));
    assert!(
        pool.rebuild(&at(&dir, "a.drskel"), &at(&dir, "a-rebuilt.doctored"))
            .unwrap()
            == saved
    );
}

/// A 300×300 rect at (300, 300) meets columns 1–2 and rows 1–2 of a plane's
/// 256×256 tiles: four.
const DAB: (u32, u32, u32, u32) = (300, 300, 300, 300);
const DAB_TILES: u64 = 4;

/// `plane` (a `w`-wide row-major plane) with the dab rect inverted.
fn dabbed<T: Copy>(plane: &[T], w: u32, invert: impl Fn(T) -> T) -> Vec<T> {
    let (x, y, dw, dh) = DAB;
    let mut out = plane.to_vec();
    for row in y..y + dh {
        for column in x..x + dw {
            let at = (row * w + column) as usize;
            out[at] = invert(out[at]);
        }
    }
    out
}

/// The dab rect's samples of `plane`, inverted (`with_rect`'s input).
fn dab_rect(plane: &[u8], w: u32) -> Vec<u8> {
    let (x, y, dw, dh) = DAB;
    (y..y + dh)
        .flat_map(|row| (x..x + dw).map(move |column| !plane[(row * w + column) as usize]))
        .collect()
}

#[test]
fn a_byte_mask_or_channel_dab_appends_only_the_tiles_it_meets() {
    let dir = TempDir::new().unwrap();
    let (w, h) = (1024_u32, 1024_u32);
    let mask: Vec<u8> = noise8(w, h, 30).into_raw().into_iter().step_by(4).collect();
    let channel: Vec<u8> = noise8(w, h, 31).into_raw().into_iter().step_by(4).collect();
    let doc = noise_doc(w, h, 32)
        .adding_image_layer(0, noise8(w, h, 33), "Masked")
        .unwrap()
        .set_mask(
            1,
            image::GrayImage::from_raw(w, h, mask.clone()).unwrap(),
            (0, 0),
            true,
            255,
        )
        .unwrap()
        .add_channel("Alpha 1", &channel, w, h, [255, 0, 0], 0.5)
        .unwrap();
    let pool = Pool::open(&at(&dir, "pool"));
    pool.write_byte(&doc, &at(&dir, "a.drskel")).unwrap();
    let first = pool.info();
    assert_eq!(count(&first, "appended_chunks"), 0, "{first}");
    let (x, y, dw, dh) = DAB;
    let mut masked = doc.clone();
    masked.layers[1].mask = Some(
        doc.layers[1]
            .mask
            .as_ref()
            .unwrap()
            .with_rect(x, y, dw, dh, &dab_rect(&mask, w))
            .unwrap()
            .unwrap(),
    );
    let mut channeled = doc.clone();
    channeled.channels[0].data = doc.channels[0]
        .data
        .with_rect(x, y, dw, dh, &dab_rect(&channel, w))
        .unwrap()
        .unwrap();
    for (name, edited) in [("mask", &masked), ("channel", &channeled)] {
        let skeleton = at(&dir, &format!("{name}.drskel"));
        pool.write_byte(edited, &skeleton).unwrap();
        let info = pool.info();
        assert_eq!(count(&info, "appended_chunks"), 0, "{name}: {info}");
        assert_eq!(count(&info, "appended_tiles"), DAB_TILES, "{name}: {info}");
        let saved = saved_byte(edited, &at(&dir, &format!("{name}.doctored")));
        let rebuilt = pool
            .rebuild(&skeleton, &at(&dir, &format!("{name}-rebuilt.doctored")))
            .unwrap();
        assert!(rebuilt == saved, "{name}: the rebuilt file differs");
    }
}

#[test]
fn a_typed_mask_or_channel_dab_appends_only_the_tiles_it_meets() {
    let dir = TempDir::new().unwrap();
    let (w, h) = (1024_u32, 1024_u32);
    let mut state = 40;
    let mask: Vec<u16> = (0..w * h).map(|_| splitmix(&mut state) as u16).collect();
    let channel: Vec<u16> = (0..w * h).map(|_| splitmix(&mut state) as u16).collect();
    let edit = |doc: &PrecisionDocument, request: Value, samples: &[u16]| {
        doc.mask_edit(
            &MaskEdit::parse(&request.to_string()).unwrap(),
            samples,
            LIMIT,
            0,
            None,
        )
        .unwrap()
        .unwrap()
    };
    let set_mask = json!({"op":"set_mask","layer":0,"width":w,"height":h,"linked":false});
    let set_channel = |index: Value| json!({"op":"set_channel","index":index,"name":"Coverage"});
    let base = typed(w, h, noise16(w, h, 41));
    let doc = edit(
        &edit(&base, set_mask.clone(), &mask),
        set_channel(Value::Null),
        &channel,
    );
    let pool = Pool::open(&at(&dir, "pool"));
    pool.write_typed(&doc, &at(&dir, "a.drskel")).unwrap();
    let first = pool.info();
    assert_eq!(count(&first, "appended_chunks"), 0, "{first}");
    let masked = edit(&doc, set_mask, &dabbed(&mask, w, |v| !v));
    let channeled = edit(&doc, set_channel(json!(0)), &dabbed(&channel, w, |v| !v));
    for (name, edited) in [("mask", &masked), ("channel", &channeled)] {
        let skeleton = at(&dir, &format!("{name}.drskel"));
        pool.write_typed(edited, &skeleton).unwrap();
        let info = pool.info();
        assert_eq!(count(&info, "appended_chunks"), 0, "{name}: {info}");
        assert_eq!(count(&info, "appended_tiles"), DAB_TILES, "{name}: {info}");
        let saved = saved_typed(edited, &at(&dir, &format!("{name}.doctored")));
        let rebuilt = pool
            .rebuild(&skeleton, &at(&dir, &format!("{name}-rebuilt.doctored")))
            .unwrap();
        assert!(rebuilt == saved, "{name}: the rebuilt file differs");
    }
}

/// `skeleton` rewritten as version `version` under a fresh closing digest
/// (this file's own SHA-256).
fn as_version(skeleton: &Path, version: u32) {
    let mut bytes = std::fs::read(skeleton).unwrap();
    bytes[4..8].copy_from_slice(&version.to_le_bytes());
    let body = bytes.len() - 32;
    let digest = sha256_hex(&bytes[..body]);
    for (i, byte) in bytes[body..].iter_mut().enumerate() {
        *byte = u8::from_str_radix(&digest[2 * i..2 * i + 2], 16).unwrap();
    }
    std::fs::write(skeleton, bytes).unwrap();
}

/// A byte document whose skeleton holds only version 1's segments: two
/// layers (tiles), a placed 16-bit master (tiles) and its camera RAW
/// payload of 102,400 bytes (a blob), and literals.
fn version1_fixture() -> DrDocument {
    let (w, h) = (300_u32, 200_u32);
    let master = typed(w, h, noise16(w, h, 21));
    let edit = SourceEdit::parse(
        &json!({"op":"place","source_id":"camera","revision_id":"a","name":"Camera",
            "x":0,"y":0,"depth_conversion":"convert_fallback"})
        .to_string(),
    )
    .unwrap();
    let doc = noise_doc(w, h, 22)
        .source_edit_input(Some(SourceInput::Typed(&master)), &edit, LIMIT, 0, None)
        .unwrap()
        .unwrap();
    let resource = noise8(160, 160, 23).into_raw();
    let descriptor = CString::new(
        json!({"schema_version":1,"original_filename":"probe.dng","content_sha256":"0".repeat(64),
            "decoder":{"identifier":"independent","version":"1","implementation":"fixture"},
            "recipe":{"schema_version":3,"tint":11.999950408935547}})
        .to_string(),
    )
    .unwrap();
    let mut err = ptr::null_mut();
    let attached = unsafe {
        dr_doc_attach_raw_source(
            &doc,
            doc.layers.len() - 1,
            resource.as_ptr(),
            resource.len(),
            descriptor.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(!attached.is_null(), "{}", text(err));
    unsafe { *Box::from_raw(attached) }
}

#[test]
fn a_version_1_skeleton_still_rebuilds_and_cannot_name_a_gray_site() {
    let dir = TempDir::new().unwrap();
    let pool = Pool::open(&at(&dir, "pool"));
    let doc = version1_fixture();
    let skeleton = at(&dir, "old.drskel");
    pool.write_byte(&doc, &skeleton).unwrap();
    let info = pool.info();
    assert!(count(&info, "appended_chunks") > 0, "the RAW blob: {info}");
    assert!(count(&info, "appended_tiles") > 0, "{info}");
    as_version(&skeleton, 1);
    let saved = saved_byte(&doc, &at(&dir, "old.doctored"));
    let rebuilt = pool
        .rebuild(&skeleton, &at(&dir, "old-rebuilt.doctored"))
        .unwrap();
    assert!(rebuilt == saved, "a version 1 skeleton rebuilds the file");
    // A gray site (a byte mask's rows, 5) exists only from version 2.
    let masked = noise_doc(300, 200, 24)
        .add_mask(0, MaskKind::HideAll)
        .unwrap();
    let gray = at(&dir, "gray.drskel");
    pool.write_byte(&masked, &gray).unwrap();
    as_version(&gray, 1);
    let refused = pool.rebuild(&gray, &at(&dir, "gray-rebuilt.doctored"));
    assert_eq!(
        refused.unwrap_err(),
        format!("damaged tile pool skeleton {}", gray.display())
    );
}

#[test]
fn each_plane_keeps_its_smallest_codec() {
    let dir = TempDir::new().unwrap();
    let (w, h) = (512_u32, 512_u32);
    let mut state = 9;
    // Noise in every bit: no codec shrinks it, so it is stored raw.
    let noise = typed(w, h, noise16(w, h, 5));
    // Six noisy low bits under a constant high byte: the byte planes
    // separate them, so shuffle-deflate wins.
    let low = typed(
        w,
        h,
        plane16(w, h, |_| 0x8000 | (splitmix(&mut state) & 63) as u16),
    );
    // 8-bit gray promoted (v·257, one random v a pixel): each pixel is one
    // byte eight times, which plain deflate matches and the planes split.
    let mut gray = 0_u16;
    let promoted = typed(
        w,
        h,
        plane16(w, h, |component| {
            if component == 0 {
                gray = (splitmix(&mut state) & 255) as u16;
            }
            gray * 257
        }),
    );
    let pool = Pool::open(&at(&dir, "pool"));
    for (name, doc, codec) in [
        ("noise", &noise, "raw_payloads"),
        ("low", &low, "shuffle_payloads"),
        ("promoted", &promoted, "deflate_payloads"),
    ] {
        let skeleton = at(&dir, &format!("{name}.drskel"));
        pool.write_typed(doc, &skeleton).unwrap();
        let info = pool.info();
        assert_eq!(
            count(&info, "appended_tiles"),
            tiles(w, h),
            "{name}: {info}"
        );
        assert_eq!(count(&info, codec), tiles(w, h), "{name}: {info}");
        let saved = saved_typed(doc, &at(&dir, &format!("{name}.doctored")));
        let rebuilt = pool
            .rebuild(&skeleton, &at(&dir, &format!("{name}-rebuilt.doctored")))
            .unwrap();
        assert!(rebuilt == saved, "{name}");
    }
    // An 8-bit noise layer is stored raw as well.
    pool.write_byte(&noise_doc(w, h, 6), &at(&dir, "byte.drskel"))
        .unwrap();
    assert_eq!(count(&pool.info(), "raw_payloads"), tiles(w, h));
}

// ----------------------------------------------------------------- damage --

fn pack(dir: &TempDir, number: u32) -> PathBuf {
    at(dir, &format!("pool/pack-{number:08}"))
}

fn flip(path: &Path, offset: usize) {
    let mut bytes = std::fs::read(path).unwrap();
    bytes[offset] ^= 0x5A;
    std::fs::write(path, bytes).unwrap();
}

fn refused(result: Result<Vec<u8>, String>, sentence: &str) {
    let Err(error) = result else {
        panic!("expected a refusal naming {sentence:?}");
    };
    assert!(
        error.contains(sentence),
        "{error:?} should name {sentence:?}"
    );
    assert!(!error.contains("internal error"), "{error}");
}

#[test]
fn damage_refuses_by_name() {
    // Noise is stored raw, so a flipped byte still decodes: only the key
    // check can refuse it.
    let dir = TempDir::new().unwrap();
    let doc = noise_doc(512, 512, 7);
    let (skeleton, out) = (at(&dir, "a.drskel"), at(&dir, "out.doctored"));
    {
        let pool = Pool::open(&at(&dir, "pool"));
        pool.write_byte(&doc, &skeleton).unwrap();
        flip(&pack(&dir, 1), 8 + 1000);
        refused(pool.rebuild(&skeleton, &out), "failed its hash check");
        assert!(!out.exists(), "a refused rebuild publishes nothing");
    }
    // A pack cut short: this handle reads past its end; a fresh one drops
    // the entries it no longer holds.
    let dir = TempDir::new().unwrap();
    {
        let pool = Pool::open(&at(&dir, "pool"));
        pool.write_byte(&doc, &skeleton_in(&dir)).unwrap();
        let file = std::fs::OpenOptions::new()
            .write(true)
            .open(pack(&dir, 1))
            .unwrap();
        file.set_len(8 + 100).unwrap();
        refused(
            pool.rebuild(&skeleton_in(&dir), &at(&dir, "out")),
            "is truncated",
        );
    }
    refused(
        Pool::open(&at(&dir, "pool")).rebuild(&skeleton_in(&dir), &at(&dir, "out")),
        "is missing",
    );
    // No index: nothing is known.
    let dir = TempDir::new().unwrap();
    Pool::open(&at(&dir, "pool"))
        .write_byte(&doc, &skeleton_in(&dir))
        .unwrap();
    std::fs::remove_file(at(&dir, "pool/pool.index")).unwrap();
    refused(
        Pool::open(&at(&dir, "pool")).rebuild(&skeleton_in(&dir), &at(&dir, "out")),
        "is missing",
    );
    // A flipped skeleton byte fails its digest before any payload is read.
    let dir = TempDir::new().unwrap();
    let pool = Pool::open(&at(&dir, "pool"));
    pool.write_byte(&doc, &skeleton_in(&dir)).unwrap();
    flip(&skeleton_in(&dir), 20);
    refused(
        pool.rebuild(&skeleton_in(&dir), &at(&dir, "out")),
        "damaged tile pool skeleton",
    );
}

fn skeleton_in(dir: &TempDir) -> PathBuf {
    at(dir, "state.drskel")
}

#[test]
fn a_damaged_payload_is_written_again() {
    let dir = TempDir::new().unwrap();
    let doc = noise_doc(512, 512, 8);
    let saved = saved_byte(&doc, &at(&dir, "saved.doctored"));
    let pool = Pool::open(&at(&dir, "pool"));
    pool.write_byte(&doc, &at(&dir, "a.drskel")).unwrap();
    flip(&pack(&dir, 1), 8 + 1000);
    refused(
        pool.rebuild(&at(&dir, "a.drskel"), &at(&dir, "out.doctored")),
        "failed its hash check",
    );
    // The handle forgot the damaged payload: the same document writes it
    // again (a memo hit, so nothing is hashed), and its new entry wins.
    pool.write_byte(&doc, &at(&dir, "b.drskel")).unwrap();
    let info = pool.info();
    assert_eq!(count(&info, "appended_tiles"), 1, "{info}");
    assert_eq!(count(&info, "hashed_tiles"), 0, "{info}");
    assert!(
        pool.rebuild(&at(&dir, "b.drskel"), &at(&dir, "b.doctored"))
            .unwrap()
            == saved
    );
    drop(pool);
    let fresh = Pool::open(&at(&dir, "pool"));
    assert!(
        fresh
            .rebuild(&at(&dir, "b.drskel"), &at(&dir, "c.doctored"))
            .unwrap()
            == saved
    );
}

#[test]
fn a_crash_torn_index_is_repaired_at_open() {
    let dir = TempDir::new().unwrap();
    let (a, b) = (noise_doc(512, 256, 10), noise_doc(256, 512, 11));
    Pool::open(&at(&dir, "pool"))
        .write_byte(&a, &at(&dir, "a.drskel"))
        .unwrap();
    let index = at(&dir, "pool/pool.index");
    // A torn tail: a crash in the middle of the next entry.
    let mut bytes = std::fs::read(&index).unwrap();
    bytes.extend_from_slice(&[0xCD; 20]);
    std::fs::write(&index, bytes).unwrap();
    {
        let pool = Pool::open(&at(&dir, "pool"));
        let length = std::fs::metadata(&index).unwrap().len();
        assert_eq!(
            (length - 8) % 49,
            0,
            "the torn tail is cut back to an entry boundary"
        );
        pool.write_byte(&b, &at(&dir, "b.drskel")).unwrap();
    }
    let fresh = Pool::open(&at(&dir, "pool"));
    assert_eq!(count(&fresh.info(), "tiles"), 2 * tiles(512, 256));
    for (name, doc) in [("a", &a), ("b", &b)] {
        let saved = saved_byte(doc, &at(&dir, &format!("{name}.doctored")));
        let rebuilt = fresh
            .rebuild(
                &at(&dir, &format!("{name}.drskel")),
                &at(&dir, &format!("{name}-r.doctored")),
            )
            .unwrap();
        assert!(rebuilt == saved, "{name}");
    }
}

#[test]
fn an_entry_past_its_pack_stays_dropped() {
    let dir = TempDir::new().unwrap();
    let (a, b) = (noise_doc(512, 256, 14), noise_doc(256, 512, 15));
    Pool::open(&at(&dir, "pool"))
        .write_byte(&a, &at(&dir, "a.drskel"))
        .unwrap();
    // An entry whose payload ends past its pack: its bytes never landed.
    let index = at(&dir, "pool/pool.index");
    let pack_length = std::fs::metadata(pack(&dir, 1)).unwrap().len();
    let mut bytes = std::fs::read(&index).unwrap();
    bytes.extend_from_slice(&[0xAB; 32]);
    bytes.extend_from_slice(&1_u32.to_le_bytes());
    bytes.extend_from_slice(&(pack_length + 100).to_le_bytes());
    bytes.extend_from_slice(&64_u32.to_le_bytes());
    bytes.push(0);
    std::fs::write(&index, bytes).unwrap();
    {
        let pool = Pool::open(&at(&dir, "pool"));
        assert_eq!(count(&pool.info(), "tiles"), tiles(512, 256));
        // B's payloads grow pack 1 past the dropped entry's end.
        pool.write_byte(&b, &at(&dir, "b.drskel")).unwrap();
    }
    assert!(std::fs::metadata(pack(&dir, 1)).unwrap().len() > pack_length + 164);
    let fresh = Pool::open(&at(&dir, "pool"));
    assert_eq!(
        count(&fresh.info(), "tiles"),
        2 * tiles(512, 256),
        "the dropped entry does not come back once its pack grew"
    );
    let saved = saved_byte(&b, &at(&dir, "b.doctored"));
    assert!(
        fresh
            .rebuild(&at(&dir, "b.drskel"), &at(&dir, "b-r.doctored"))
            .unwrap()
            == saved
    );
}

// ------------------------------------------------------------- collection --

#[test]
fn collection_keeps_the_live_payloads_and_compacts() {
    let dir = TempDir::new().unwrap();
    // A is 6 tiles and B 4, both raw noise, A first in pack 1: once A is
    // dead the pack is 40 % live, under half, so it is compacted.
    let (a, b) = (noise_doc(768, 512, 12), noise_doc(512, 512, 13));
    let (a_saved, b_saved) = (
        saved_byte(&a, &at(&dir, "a.doctored")),
        saved_byte(&b, &at(&dir, "b.doctored")),
    );
    let pool = Pool::open(&at(&dir, "pool"));
    pool.write_byte(&a, &at(&dir, "a.drskel")).unwrap();
    pool.write_byte(&b, &at(&dir, "b.drskel")).unwrap();
    let before = pool.info();
    assert_eq!(count(&before, "tiles"), 10);
    // An unreadable skeleton refuses the whole collection.
    let refusal = pool
        .collect(&[&at(&dir, "b.drskel"), &at(&dir, "absent.drskel")])
        .unwrap_err();
    assert!(refusal.contains("absent.drskel"), "{refusal}");
    assert_eq!(count(&pool.info(), "tiles"), 10, "nothing removed");
    assert!(
        pool.rebuild(&at(&dir, "a.drskel"), &at(&dir, "a1.doctored"))
            .unwrap()
            == a_saved
    );
    pool.collect(&[&at(&dir, "b.drskel")]).unwrap();
    let after = pool.info();
    assert_eq!(count(&after, "tiles"), 4, "{after}");
    assert_eq!(count(&after, "packs"), 1, "{after}");
    assert!(
        count(&after, "stored_bytes") < count(&before, "stored_bytes"),
        "{after}"
    );
    assert!(!pack(&dir, 1).exists(), "the sparse pack is removed");
    assert!(pack(&dir, 2).exists(), "its live payloads moved");
    assert!(
        pool.rebuild(&at(&dir, "b.drskel"), &at(&dir, "b1.doctored"))
            .unwrap()
            == b_saved
    );
    refused(
        pool.rebuild(&at(&dir, "a.drskel"), &at(&dir, "a2.doctored")),
        "is missing",
    );
    // A is still alive on this handle: its memo entries answer its keys,
    // and the pool, which no longer holds them, takes every tile again.
    pool.write_byte(&a, &at(&dir, "a3.drskel")).unwrap();
    let info = pool.info();
    assert_eq!(count(&info, "appended_tiles"), 6, "{info}");
    assert_eq!(count(&info, "hashed_tiles"), 0, "{info}");
    assert!(
        pool.rebuild(&at(&dir, "a3.drskel"), &at(&dir, "a3.doctored"))
            .unwrap()
            == a_saved
    );
    drop(pool);
    let fresh = Pool::open(&at(&dir, "pool"));
    assert!(
        fresh
            .rebuild(&at(&dir, "b.drskel"), &at(&dir, "b2.doctored"))
            .unwrap()
            == b_saved
    );
}

// ------------------------------------------------------------------- NULL --

#[test]
fn null_inputs_refuse_without_crashing() {
    let dir = TempDir::new().unwrap();
    let pool = Pool::open(&at(&dir, "pool"));
    let doc = noise_doc(8, 8, 1);
    let typed_doc = typed(8, 8, noise16(8, 8, 1));
    let path = c(&at(&dir, "x"));
    let null_path: *const c_char = ptr::null();
    let refuses = |ok: bool, err: *mut c_char, what: &str| {
        assert!(!ok, "{what}");
        let message = text(err);
        assert!(
            !message.is_empty() && !message.contains("internal error"),
            "{what}: {message}"
        );
    };
    unsafe {
        let mut err = ptr::null_mut();
        assert!(dr_tile_pool_open(ptr::null(), &mut err).is_null());
        refuses(false, err, "open");
        dr_tile_pool_free(ptr::null_mut());
        let mut err = ptr::null_mut();
        let ok = dr_doc_tile_pool_write(ptr::null(), pool.0, path.as_ptr(), ptr::null(), &mut err);
        refuses(ok, err, "byte doc");
        let mut err = ptr::null_mut();
        let ok =
            dr_doc_tile_pool_write(&doc, ptr::null_mut(), path.as_ptr(), ptr::null(), &mut err);
        refuses(ok, err, "byte pool");
        let mut err = ptr::null_mut();
        let ok = dr_doc_tile_pool_write(&doc, pool.0, null_path, ptr::null(), &mut err);
        refuses(ok, err, "byte path");
        let mut err = ptr::null_mut();
        let ok = dr_precision_doc_tile_pool_write(
            ptr::null(),
            pool.0,
            path.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        );
        refuses(ok, err, "typed doc");
        let mut err = ptr::null_mut();
        let ok = dr_precision_doc_tile_pool_write(
            &typed_doc,
            ptr::null_mut(),
            path.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        );
        refuses(ok, err, "typed pool");
        let mut err = ptr::null_mut();
        let ok = dr_tile_pool_rebuild(
            ptr::null_mut(),
            path.as_ptr(),
            path.as_ptr(),
            ptr::null(),
            &mut err,
        );
        refuses(ok, err, "rebuild pool");
        let mut err = ptr::null_mut();
        let ok = dr_tile_pool_rebuild(pool.0, null_path, path.as_ptr(), ptr::null(), &mut err);
        refuses(ok, err, "rebuild skeleton");
        let mut err = ptr::null_mut();
        let ok = dr_tile_pool_collect(ptr::null_mut(), ptr::null(), 0, &mut err);
        refuses(ok, err, "collect pool");
        let mut err = ptr::null_mut();
        let ok = dr_tile_pool_collect(pool.0, ptr::null(), 2, &mut err);
        refuses(ok, err, "collect paths");
        let mut err = ptr::null_mut();
        let nulls = [null_path];
        let ok = dr_tile_pool_collect(pool.0, nulls.as_ptr(), 1, &mut err);
        refuses(ok, err, "collect a NULL path");
        let mut err = ptr::null_mut();
        assert!(dr_tile_pool_info(ptr::null(), &mut err).is_null());
        refuses(false, err, "info");
        // NULL err_out is tolerated.
        assert!(!dr_tile_pool_rebuild(
            ptr::null_mut(),
            null_path,
            null_path,
            ptr::null(),
            ptr::null_mut()
        ));
    }
    assert_eq!(dr_tile_pool_window_bytes(), 8 * 1024 * 1024);
    // An empty collection keeps nothing and still succeeds.
    pool.write_byte(&doc, &at(&dir, "s.drskel")).unwrap();
    pool.collect(&[]).unwrap();
    assert_eq!(count(&pool.info(), "tiles"), 0);
}
