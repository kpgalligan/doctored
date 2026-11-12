//! The banded 16-bit native writer keeps every saved byte. Each shape's
//! file is pinned by a SHA-256 captured with the former writer (`encode_native_png`
//! through image/png, before the change), after zeroing the `LIDS` ids (minted
//! process-wide, so not reproducible), and each PNG record (u64 length through
//! IEND) is pinned on its own. A pinned digest is never edited. The plan's write
//! term is then checked against this file's own formula.

use doctored_core::doc::DrDocument;
use doctored_core::doc_evaluation::Evaluation;
use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_doc::dr_doc_save_native;
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::PrecisionDocument;
use doctored_core::precision_geometry::GeometryEdit;
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use doctored_core::precision_mask_edit::MaskEdit;
use doctored_core::retained_source::{SourceEdit, SourceInput};
use image::RgbaImage;
use serde_json::{json, Value};
use std::ffi::{CStr, CString};
use std::ptr;

use crate::common;
use crate::warp_fixture;

const LIMIT: u64 = REFERENCE_WORKING_BYTES;
const SIGNATURE: &[u8] = b"\x89PNG\r\n\x1a\n";
const IEND: [u8; 12] = [0, 0, 0, 0, b'I', b'E', b'N', b'D', 0xAE, 0x42, 0x60, 0x82];
const FLOAT_FIXTURE: &str = "tests/fixtures/float_11f/scene.tiff";

/// A shape's pinned digests: the whole file with its `LIDS` ids zeroed, then
/// each PNG record in file order.
struct Pin {
    file: &'static str,
    pngs: &'static [&'static str],
}

/// xorshift64 noise in all four channels.
fn noise(w: u32, h: u32, seed: u64) -> PrecisionImage {
    let mut state = seed;
    let pixels = (0..w as usize * h as usize)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            [
                state as u16,
                (state >> 16) as u16,
                (state >> 32) as u16,
                (state >> 48) as u16,
            ]
        })
        .collect();
    PrecisionImage::from_rgba16(w, h, pixels, IccProfile::srgb(), None).unwrap()
}

fn document(image: PrecisionImage) -> PrecisionDocument {
    PrecisionDocument::from_image(image, Metadata::default(), Resolution { x: 72., y: 72. })
        .unwrap()
}

fn geometry(doc: &PrecisionDocument, request: Value) -> PrecisionDocument {
    doc.geometry(
        &GeometryEdit::parse(&request.to_string()).unwrap(),
        LIMIT,
        0,
        None,
    )
    .unwrap()
    .unwrap()
}

fn u16_doc() -> PrecisionDocument {
    document(noise(517, 389, 0x2545_F491_4F6C_DD1D))
}

/// R = 255 · (8 · 32 + 1) = 65,535: exactly one full stored block.
fn blk_doc() -> PrecisionDocument {
    document(
        PrecisionImage::from_rgba16(
            32,
            255,
            vec![[12_000, 34_000, 56_000, 65_535]; 32 * 255],
            IccProfile::srgb(),
            None,
        )
        .unwrap(),
    )
}

fn placed_base() -> PrecisionDocument {
    placed_base_at((0, 0))
}

/// The placed photo, its one record made with `reference` (`from_image`'s
/// own `from_parts`, the record carried with that point).
fn placed_base_at(reference: (i32, i32)) -> PrecisionDocument {
    let doc = document(noise(320, 240, 0x9E37_79B9_7F4A_7C15));
    let (w, h) = doc.dimensions();
    let record = doc.layer(0).unwrap().clone().with_reference(reference);
    let profile = doc.profile().clone();
    PrecisionDocument::from_parts(
        w,
        h,
        profile,
        Metadata::default(),
        doc.resolution(),
        vec![record],
    )
    .unwrap()
    .source_edit(
        None,
        &SourceEdit::parse(r#"{"op":"retain","source_id":"pin390"}"#).unwrap(),
        LIMIT,
        0,
        None,
    )
    .unwrap()
    .unwrap()
}

/// The placed photo turned 17° into the 378 × 324 canvas the pin was captured
/// on: Rotate Canvas's plan of that time (the symmetric box, whose origin was
/// (29, 42)), restated as a `straighten` with the rotation about the centre
/// and that window, so its map is the same matrix bit for bit (today's
/// filtered Rotate Canvas is 377 × 324 at (28, 42)). The reference
/// point follows that map (POINT), so the record is made at the point it maps
/// to (0, 0) — round(M((−68, 20))) = round((0.20, −0.29)) — and the pinned
/// file, captured when the turn kept (0, 0), still holds no `LREF` table.
fn placed_doc() -> PrecisionDocument {
    let (s, c) = 17f64.to_radians().sin_cos();
    let (cx, cy) = (160.0, 120.0);
    let centred = [c, s, -s, c, cx - cx * c + cy * s, cy - cx * s - cy * c];
    let out = geometry(
        &placed_base_at((-68, 20)),
        json!({"op":"straighten","matrix":centred,"filter":"bicubic",
               "x":-29,"y":-42,"width":378,"height":324}),
    );
    assert_eq!(
        out.layer(0).unwrap().reference(),
        (0, 0),
        "the turned point"
    );
    out
}

fn proj_doc() -> PrecisionDocument {
    let (w, h) = (320., 240.);
    let quad = [0., 0., w, 0., w * 1.15, h * 1.15, -w * 0.15, h * 1.15];
    geometry(
        &placed_base(),
        json!({"op":"perspective","layer":0,"target":"pixels","quad":quad,"filter":"bicubic"}),
    )
}

fn mask_edit(doc: &PrecisionDocument, request: Value, samples: &[u16]) -> PrecisionDocument {
    doc.mask_edit(
        &MaskEdit::parse(&request.to_string()).unwrap(),
        samples,
        LIMIT,
        0,
        None,
    )
    .unwrap()
    .unwrap()
}

/// Two rasters, a u16 layer mask on the upper one and one saved channel.
fn mc_doc() -> PrecisionDocument {
    let (w, h) = (64_u32, 48_u32);
    let two = document(noise(w, h, 0x1234_5678_9ABC_DEF1))
        .duplicate_layer(0)
        .unwrap()
        .unwrap();
    let ramp: Vec<u16> = (0..w * h).map(|i| (i * 21) as u16).collect();
    let masked = mask_edit(
        &two,
        json!({"op":"set_mask","layer":1,"width":w,"height":h,"x":0,"linked":false,
            "density":1.0,"feather":0.0,"enabled":true}),
        &ramp,
    );
    let coverage: Vec<u16> = (0..w * h).map(|i| 65_535 - (i * 13) as u16).collect();
    mask_edit(
        &masked,
        json!({"op":"set_channel","index":null,"name":"Coverage"}),
        &coverage,
    )
}

fn lin_doc() -> PrecisionDocument {
    u16_doc()
        .with_evaluation(Evaluation::LinearLight)
        .unwrap()
        .unwrap()
}

fn flt_doc() -> PrecisionDocument {
    PrecisionDocument::open_preserving(FLOAT_FIXTURE, LIMIT, 0, None).unwrap()
}

fn warp_set_doc() -> PrecisionDocument {
    warp_fixture::typed_document()
        .warp_edit(
            &warp_fixture::edit(warp_fixture::field(3, 3, "bicubic")),
            LIMIT,
            0,
            None,
        )
        .unwrap()
        .unwrap()
}

/// A byte document holding a genuine u16 placed source.
fn b16_doc() -> DrDocument {
    let source = document(noise(96, 64, 0x0F1E_2D3C_4B5A_6978));
    DrDocument::from_pixels(RgbaImage::from_pixel(
        96,
        64,
        image::Rgba([17, 31, 47, 255]),
    ))
    .source_edit_input(
        Some(SourceInput::Typed(&source)),
        &SourceEdit::parse(
            &json!({"op":"place","source_id":"b16","revision_id":"revision_a","name":"b16",
                "x":0,"y":0,"depth_conversion":"convert_fallback"})
            .to_string(),
        )
        .unwrap(),
        LIMIT,
        0,
        None,
    )
    .unwrap()
    .unwrap()
}

/// A byte document with a placed 8-bit photo (an RGBA8 source asset).
fn b8_doc() -> DrDocument {
    let source = common::doc_of(96, 64, common::plasma);
    let place =
        SourceEdit::parse(r#"{"op":"place","source_id":"b8","name":"Original","x":0,"y":0}"#)
            .unwrap();
    source
        .source_edit(Some(&source), &place, LIMIT, 0, None)
        .unwrap()
        .unwrap()
        .remove_layers(&[0])
        .unwrap()
}

/// A typed document's file, checked against its plan's bound.
fn typed(doc: &PrecisionDocument) -> Vec<u8> {
    let bytes = doc.encode_native(LIMIT, 0, None).unwrap();
    assert!(bytes.len() as u64 <= doc.native_plan().unwrap().encoded_bound);
    bytes
}

/// A byte document's file, as `dr_doc_save_native` writes it.
fn byte_file(doc: &DrDocument) -> Vec<u8> {
    let directory = tempfile::TempDir::new().unwrap();
    let path = directory.path().join("pin.rz");
    let name = CString::new(path.to_str().unwrap()).unwrap();
    let mut error = ptr::null_mut();
    let accepted = unsafe { dr_doc_save_native(doc, name.as_ptr(), std::ptr::null(), &mut error) };
    if !error.is_null() {
        let message = unsafe { CStr::from_ptr(error) }
            .to_string_lossy()
            .into_owned();
        unsafe { dr_string_free(error) };
        panic!("{message}");
    }
    assert!(accepted);
    std::fs::read(path).unwrap()
}

/// `bytes` with its trailing `LIDS` ids zeroed, after asserting the framing
/// (tag, schema 1, count n, n unique non-zero ids closing the file).
fn without_ids(bytes: &[u8]) -> Vec<u8> {
    let len = bytes.len();
    let n = (0..=(len.saturating_sub(12) / 8))
        .find(|&n| {
            let at = len - 8 * n - 12;
            &bytes[at..at + 4] == b"LIDS"
                && bytes[at + 4..at + 8] == 1_u32.to_le_bytes()
                && bytes[at + 8..at + 12] == (n as u32).to_le_bytes()
        })
        .expect("LIDS closes the file");
    let ids: Vec<u64> = bytes[len - 8 * n..]
        .chunks_exact(8)
        .map(|c| u64::from_le_bytes(c.try_into().unwrap()))
        .collect();
    for (i, id) in ids.iter().enumerate() {
        assert!(*id != 0 && !ids[..i].contains(id), "unique non-zero ids");
    }
    let mut out = bytes.to_vec();
    out[len - 8 * n..].fill(0);
    out
}

/// Every PNG record: the PNG, walked by its chunks through IEND, with the
/// length prefix the writer framed it with (u64 LE for typed rasters and
/// source assets, u32 LE for byte layer rasters).
fn png_records(file: &[u8]) -> Vec<&[u8]> {
    let mut out = Vec::new();
    let mut at = 8;
    while let Some(offset) = file[at..]
        .windows(SIGNATURE.len())
        .position(|w| w == SIGNATURE)
    {
        let start = at + offset;
        let mut end = start + 8;
        loop {
            let size = u32::from_be_bytes(file[end..end + 4].try_into().unwrap()) as usize;
            let kind = &file[end + 4..end + 8];
            end += 12 + size;
            if kind == b"IEND" {
                break;
            }
        }
        assert_eq!(&file[end - 12..end], &IEND, "IEND closes the PNG");
        let len = (end - start) as u64;
        let prefix = if u64::from_le_bytes(file[start - 8..start].try_into().unwrap()) == len {
            8
        } else {
            assert_eq!(
                u64::from(u32::from_le_bytes(
                    file[start - 4..start].try_into().unwrap()
                )),
                len,
                "a u32 or u64 length prefix"
            );
            4
        };
        out.push(&file[start - prefix..end]);
        at = end;
    }
    out
}

fn digests(bytes: &[u8]) -> (String, Vec<String>) {
    // The pins predate the product rename, whose writer starts every file
    // `DRDC` where the former one wrote `RZDC`; the digest is taken over the
    // former writer's magic so every other byte is still held to its pin.
    assert_eq!(&bytes[..4], b"DRDC", "this build's magic");
    let mut former = without_ids(bytes);
    former[..4].copy_from_slice(b"RZDC");
    let file = common::sha256::sha256_hex(&former);
    let pngs = png_records(bytes)
        .into_iter()
        .map(common::sha256::sha256_hex)
        .collect();
    (file, pngs)
}

/// Prints every shape's digests, then checks each against its pin.
fn pin(shapes: &[(&str, &[u8], &Pin)]) {
    let found: Vec<_> = shapes
        .iter()
        .map(|(name, bytes, _)| {
            let (file, pngs) = digests(bytes);
            println!("PIN {name} len {} file {file} pngs {pngs:?}", bytes.len());
            (file, pngs)
        })
        .collect();
    for ((name, _, expected), (file, pngs)) in shapes.iter().zip(found) {
        assert_eq!(file, expected.file, "{name}: file");
        assert_eq!(pngs, expected.pngs, "{name}: PNG records");
    }
}

const U16: Pin = Pin {
    file: "c7689319e6889c7d2fb977cbca9c0924d6f6c0dc32f7be5241e71a521032cc70",
    pngs: &["014307a0f47e3fa7c5854560445d7ea89cf7cf662cc29809837804b9001a47eb"],
};
const BLK: Pin = Pin {
    file: "e0229a5c53e60d0194696acf646c7f7396704f16fc2400acd850f29133101363",
    pngs: &["e1b0572a4d3f04715d40080fa4eb9327e872ae316da3b478915d944aa69cfbce"],
};
const PLACED: Pin = Pin {
    file: "2aecf58f0df01cf0335010b1bf2073c7535ff765447aa05129c30319485d73e6",
    pngs: &[
        "fac74f1a87d454f7047ce78475d8e20ede352c54394211314086cb8014f5b0bf",
        "500ca29ca793053128866b2e9364a11c4db5d058d38f5f6ae6c2bac3025acf4b",
    ],
};
const PROJ: Pin = Pin {
    file: "2c0abeabfaee579d1c4e4d0c0e8b5487e7aef668e9418f50ec44fc1203f552f9",
    pngs: &[
        "f4e14aed50bf7b267f04b3672eac24f7a63b3a62b309aa8853791112bb15f8a4",
        "500ca29ca793053128866b2e9364a11c4db5d058d38f5f6ae6c2bac3025acf4b",
    ],
};
const MC: Pin = Pin {
    file: "bbf72ca504f17f95d04636cc327962addebab2f7e6d837866d3c5308c7edcbd5",
    pngs: &[
        "0d2a4ebcc969cc76d3806144dd201dfc5e6de7dd2b081531a5b5a20d14485635",
        "0d2a4ebcc969cc76d3806144dd201dfc5e6de7dd2b081531a5b5a20d14485635",
    ],
};
const LIN: Pin = Pin {
    file: "32fa0032d6e1badb1e610c9ceabbf9510bee9ae3c0ad516f3ea6648bab796ddd",
    pngs: &["014307a0f47e3fa7c5854560445d7ea89cf7cf662cc29809837804b9001a47eb"],
};
const FLT: Pin = Pin {
    file: "d94ab06897b5a465986b4192a928ac91f2f525d833cf5afe63a8a538c0b06fa1",
    pngs: &[],
};
const WARP: Pin = Pin {
    file: "bcb0c14be56022262cdc57ec85f9e9a1194c3e74effb794e1ac4d19002975b2c",
    pngs: &["e339ccce703075d1d1fd6807415bb08669c873552a29c961856960e162be6da1"],
};
const WARP_SET: Pin = Pin {
    file: "2b4bf0300972a51eca3ee24176bdf75b6109d0ffaef52f7c6e53d788686dec87",
    pngs: &["01876b927463e54d245b2ef45c67a144b0ce5c7e584de76ad47597da6f2960b2"],
};
const B16: Pin = Pin {
    file: "f394ae1e0b98f7c956641b990b147b2c1f070909e7408b4f681dcb82fd566146",
    pngs: &[
        "95d059df17ebdbe23a77206a2a27a1023d2fd0a9fef80dd3365ec8f93021b440",
        "c63d0ef1499e9223763d1740fa4ef80a74e5eba0ee6ac476d37bebc5fef25a5d",
        "edb70eef7ffc537e6f39513f45317d0f0f064da21a8a6ab2353b4738b208e18d",
    ],
};
const B8: Pin = Pin {
    file: "ca597aa54c48708a6fac2184aaa3df82038619de822328720ffc8581645b2c21",
    pngs: &[
        "c2998a43e4d2e9b0e85321da96d295b55d1e12b89294e28fb14add09f91c8d62",
        "764c6b61ea0d8f202bf7edee819595124901402a5e1efac7a1223a6cec06b85b",
    ],
};

#[test]
fn a_noise_raster_saves_its_pinned_bytes() {
    pin(&[("U16", &typed(&u16_doc()), &U16)]);
}

#[test]
fn a_one_block_raster_saves_its_pinned_bytes() {
    pin(&[("BLK", &typed(&blk_doc()), &BLK)]);
}

#[test]
fn a_turned_placed_photo_saves_its_pinned_bytes() {
    pin(&[("PLACED", &typed(&placed_doc()), &PLACED)]);
}

#[test]
fn a_projected_placed_photo_saves_its_pinned_bytes() {
    let bytes = typed(&proj_doc());
    assert!(
        bytes.windows(4).any(|w| w == b"RSPJ"),
        "the projective table"
    );
    pin(&[("PROJ", &bytes, &PROJ)]);
}

#[test]
fn masks_and_channels_save_their_pinned_bytes() {
    pin(&[("MC", &typed(&mc_doc()), &MC)]);
}

#[test]
fn a_linear_light_raster_saves_its_pinned_bytes() {
    pin(&[("LIN", &typed(&lin_doc()), &LIN)]);
}

#[test]
fn a_float_document_saves_its_pinned_bytes() {
    pin(&[("FLT", &typed(&flt_doc()), &FLT)]);
}

#[test]
fn the_warp_fixture_saves_its_pinned_bytes() {
    let warp_set = typed(&warp_set_doc());
    assert!(warp_set.windows(4).any(|w| w == b"WARP"), "the WARP table");
    pin(&[
        ("WARP", &typed(&warp_fixture::typed_document()), &WARP),
        ("WARP_SET", &warp_set, &WARP_SET),
    ]);
}

#[test]
fn a_byte_document_s_u16_source_saves_its_pinned_bytes() {
    pin(&[("B16", &byte_file(&b16_doc()), &B16)]);
}

#[test]
fn a_byte_document_s_8_bit_source_saves_its_pinned_bytes() {
    pin(&[("B8", &byte_file(&b8_doc()), &B8)]);
}

/// This file's own length formula for a stored RGBA16 PNG (copied from
/// `native_png_bound.rs`): R row bytes (a filter byte per row), Z the zlib
/// stream (2-byte header, 5 bytes per stored block of ≤ 65,535, Adler-32),
/// then signature, IHDR, one IDAT per 2^31 − 1 bytes, IEND.
fn expected_len(w: u32, h: u32) -> u64 {
    let r = u64::from(h) * (8 * u64::from(w) + 1);
    let z = 2 + r + 5 * r.div_ceil(65_535) + 4;
    8 + (12 + 13) + z + 12 * z.div_ceil((1 << 31) - 1) + 12
}

const BOOKKEEPING: u64 = 1 << 20;
const NATIVE_FIXED: u64 = 4 << 20;
/// The streamed writer's window (`native_stream`): its 1 MiB
/// `BufWriter` and IDAT window, with room to spare.
const STREAM_WINDOW: u64 = 4 << 20;

#[test]
fn the_save_plan_charges_the_header_twice_one_row_the_window_and_a_quarter() {
    // One raster and nothing else: the plan's header is `encoded_bound` less
    // the PNG. The streamed save holds no file: the write term is the
    // pre-built tables and one scratch table (each at most that header), one
    // 8·w row plus the fixed allowance, the sink's window and bookkeeping,
    // and a quarter of their sum.
    let doc = u16_doc();
    let plan = doc.native_plan().unwrap();
    let header = plan.encoded_bound - expected_len(517, 389);
    let base = 2 * header + 8 * 517 + NATIVE_FIXED + STREAM_WINDOW + BOOKKEEPING;
    assert_eq!(plan.write_scratch_bytes, base + base.div_ceil(4));
    // And the save admits at exactly that figure beside the retained pixels.
    let limit = doc.retained_bytes() + plan.write_scratch_bytes;
    assert!(doc.encode_native(limit, 0, None).is_ok());
    assert!(doc.encode_native(limit, 1, None).is_err());
}

#[test]
fn a_float_save_plan_is_the_streamed_write_term() {
    // One raw float raster: the header is `encoded_bound` less its u32
    // width, u32 height, u64 length and `w · h · 16` body. No transient (no
    // PNG is encoded): twice the header, the window and bookkeeping, plus a
    // quarter.
    let doc = flt_doc();
    let plan = doc.native_plan().unwrap();
    let (w, h) = doc.layer_source(0).unwrap().dimensions();
    let header = plan.encoded_bound - 16 - 16 * u64::from(w) * u64::from(h);
    let base = 2 * header + STREAM_WINDOW + BOOKKEEPING;
    assert_eq!(plan.write_scratch_bytes, base + base.div_ceil(4));
}
