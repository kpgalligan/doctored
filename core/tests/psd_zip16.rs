//! ZIP and ZIP-with-prediction 16-bit PSD planes, and the `Lr16` tail block
//! Photoshop keeps its 16-bit layer records in, through the C API.
//!
//! The oracle is `common/psd16_fixture.rs`, an independent builder/reader with
//! its OWN stored-block zlib writer, its own Adler-32, its own delta encoder
//! and its own delta decoder; nothing here is compared against the crate's
//! inflater or its prediction pass. The committed Photoshop-authored file in
//! `core/tests/psd_photoshop_depth16.rs` is the other half of this evidence:
//! it exercises real compressed streams, which this file's stored blocks
//! deliberately do not.
use doctored_core::icc::IccProfile;
use doctored_core::job_control::DrJobControl;
use doctored_core::precision_document::PrecisionDocument;
use doctored_core::precision_image::REFERENCE_WORKING_BYTES;
use doctored_core::psd_zip::unpack_zip16;
use serde_json::Value;
use std::ffi::CString;
use std::path::Path;
use std::ptr;
use tempfile::TempDir;

use crate::common;
use crate::psd16_fixture as fixture;
use common::{cpath, take_err_string};
use fixture::Record;

const LAYERS: i32 = 0;
const FLATTENED: i32 = 1;
const CANVAS: (u32, u32) = (8, 6);

// ------------------------------------------------------------ C plumbing --

/// A typed document handle freed through the C API on drop.
struct Owned(*mut PrecisionDocument);
impl Drop for Owned {
    fn drop(&mut self) {
        unsafe { doctored_core::ffi_precision::dr_precision_doc_free(self.0) };
    }
}
impl std::ops::Deref for Owned {
    type Target = PrecisionDocument;
    fn deref(&self) -> &PrecisionDocument {
        unsafe { &*self.0 }
    }
}

fn report(path: &Path) -> Value {
    let mut error = ptr::null_mut();
    let json =
        unsafe { doctored_core::ffi_psd::dr_psd_import_report(cpath(path).as_ptr(), &mut error) };
    assert!(!json.is_null(), "{}", take_err_string(error));
    serde_json::from_str(&take_err_string(json)).expect("report JSON")
}

fn open_typed(path: &Path, mode: i32) -> Owned {
    let notice = report(path);
    let expected = CString::new(notice["source_fingerprint"].as_str().unwrap()).unwrap();
    let mut error = ptr::null_mut();
    let document = unsafe {
        doctored_core::ffi_psd::dr_precision_doc_open_psd(
            cpath(path).as_ptr(),
            mode,
            expected.as_ptr(),
            REFERENCE_WORKING_BYTES,
            0,
            ptr::null::<DrJobControl>(),
            &mut error,
        )
    };
    assert!(
        !document.is_null(),
        "typed open: {}",
        take_err_string(error)
    );
    Owned(document)
}

fn info(document: &PrecisionDocument) -> Value {
    let mut error = ptr::null_mut();
    let json = unsafe { doctored_core::ffi_precision::dr_precision_doc_info(document, &mut error) };
    assert!(!json.is_null(), "{}", take_err_string(error));
    serde_json::from_str(&take_err_string(json)).expect("info JSON")
}

fn layer_pixels(document: &PrecisionDocument, index: usize) -> Vec<[u16; 4]> {
    let image =
        unsafe { doctored_core::ffi_precision::dr_precision_doc_layer_source(document, index) };
    assert!(!image.is_null(), "layer {index} has no raster source");
    let (w, h) = unsafe { (*image).dimensions() };
    let base = unsafe { doctored_core::ffi_precision::dr_precision_image_pixels_rgba16(image) };
    let samples = unsafe { std::slice::from_raw_parts(base, w as usize * h as usize * 4) }.to_vec();
    unsafe { doctored_core::ffi_precision::dr_precision_image_free(image) };
    samples
        .chunks_exact(4)
        .map(|p| [p[0], p[1], p[2], p[3]])
        .collect()
}

fn strings(value: &Value, key: &str) -> Vec<String> {
    value[key]
        .as_array()
        .expect(key)
        .iter()
        .map(|v| v.as_str().expect("string").to_string())
        .collect()
}

fn write(directory: &TempDir, name: &str, bytes: &[u8]) -> std::path::PathBuf {
    let path = directory.path().join(name);
    std::fs::write(&path, bytes).expect("write fixture");
    path
}

// ------------------------------------------------------------- the bytes --

fn u32_at(bytes: &[u8], at: usize) -> usize {
    u32::from_be_bytes(bytes[at..at + 4].try_into().unwrap()) as usize
}

/// The offset of the layer-and-mask section's LENGTH field, walked from the
/// header exactly as Adobe's table describes it.
fn section_length_at(bytes: &[u8]) -> usize {
    let resources = 30 + u32_at(bytes, 26);
    resources + 4 + u32_at(bytes, resources)
}

/// One record's four colour planes in the chosen compression.
fn planes(rgba: &[[u16; 4]], width: usize, word: u16) -> Vec<(i16, Vec<u8>)> {
    [(-1i16, 3usize), (0, 0), (1, 1), (2, 2)]
        .into_iter()
        .map(|(id, component)| {
            let samples: Vec<u16> = rgba.iter().map(|p| p[component]).collect();
            (
                id,
                match word {
                    0 => fixture::raw_plane(&samples),
                    2 => fixture::zip_plane(&samples, width, false),
                    _ => fixture::zip_plane(&samples, width, true),
                },
            )
        })
        .collect()
}

fn picture(w: usize, h: usize, seed: u16) -> Vec<[u16; 4]> {
    (0..w * h)
        .map(|i| {
            let v = i as u16;
            [
                v.wrapping_mul(1013).wrapping_add(seed),
                // A long run inside a row, so prediction has zero deltas to
                // carry as well as large ones.
                if i % 8 < 4 { 40000 } else { 40000 - v },
                v.wrapping_mul(7919).wrapping_add(seed.wrapping_mul(13)),
                65535,
            ]
        })
        .collect()
}

struct Zip16 {
    bytes: Vec<u8>,
    base: Vec<[u16; 4]>,
    zipped: Vec<[u16; 4]>,
    predicted: Vec<[u16; 4]>,
    mask: Vec<u16>,
    composite: Vec<[u16; 4]>,
}

/// Photoshop's own 16-bit shape, written by the independent builder: an empty
/// layer-information section, a leading zero-length `Patt` tail block, the
/// records in `Lr16` with a NEGATIVE count (merged alpha), Raw / ZIP /
/// ZIP-predicted rasters, a predicted `-2` mask, a folder, and a pixel-less
/// `SoCo` fill record (Photoshop's own body, carried) carrying the
/// zero-rectangle mask body and a canvas-sized compression-3 `-2` plane that
/// must never be decoded.
fn zip16() -> Zip16 {
    let (w, h) = (CANVAS.0 as usize, CANVAS.1 as usize);
    let bounds = [0, 0, h as i32, w as i32];
    let base = picture(w, h, 11);
    let zipped = picture(w, h, 3001);
    let predicted = picture(w, h, 40009);
    let mask: Vec<u16> = (0..w * h).map(|i| (i as u16).wrapping_mul(2311)).collect();
    let composite: Vec<[u16; 4]> = (0..w * h)
        .map(|i| [i as u16 * 517, i as u16 * 2029, i as u16 * 37, 65535])
        .collect();

    let mut fill = Record::raster("Fill", [0, 0, 0, 0], vec![]);
    fill.channels = [-1i16, 0, 1, 2]
        .into_iter()
        .map(|id| (id, vec![0, 0]))
        .collect();
    fill.flags = 0x18;
    fill.blocks = fixture::block(b"SoCo", &common::psd_photoshop::soco_body());
    // The adjustment record's own "no mask" form: a 20-byte body with a ZERO
    // rectangle and outside 255, beside a canvas-sized comp-3 plane.
    let fill = fill.mask([0, 0, 0, 0], 255, true, fixture::zip_plane(&mask, w, true));

    let records = vec![
        Record::raster("Base", bounds, planes(&base, w, 0)),
        Record::raster("Zip", bounds, planes(&zipped, w, 2)),
        Record::folder("</Layer group>", 3, *b"norm"),
        Record::raster("Predicted", bounds, planes(&predicted, w, 3)).mask(
            bounds,
            0,
            true,
            fixture::zip_plane(&mask, w, true),
        ),
        Record::folder("Folder", 1, *b"norm"),
        fill,
    ];
    let bytes = fixture::fixture_lr16(
        CANVAS,
        &records,
        IccProfile::srgb().bytes(),
        (72.0, 72.0),
        &composite,
        true,
        &[(b"Patt", &[])],
    );
    Zip16 {
        bytes,
        base,
        zipped,
        predicted,
        mask,
        composite,
    }
}

/// A minimal 8-bit RGB PSD with one record whose first channel uses ZIP.
fn eight_bit_zip() -> Vec<u8> {
    let (w, h) = (2usize, 2usize);
    let mut record = Vec::new();
    for value in [0i32, 0, h as i32, w as i32] {
        record.extend(value.to_be_bytes());
    }
    record.extend(3u16.to_be_bytes());
    let planes: Vec<Vec<u8>> = vec![
        [vec![0, 2], fixture::zlib_stored(&[7u8; 4])].concat(),
        [vec![0, 0], vec![9u8; 4]].concat(),
        [vec![0, 0], vec![5u8; 4]].concat(),
    ];
    for (id, plane) in [0i16, 1, 2].into_iter().zip(&planes) {
        record.extend(id.to_be_bytes());
        record.extend((plane.len() as u32).to_be_bytes());
    }
    record.extend(b"8BIM");
    record.extend(b"norm");
    record.extend([255u8, 0, 0, 0]);
    let mut extra = 0u32.to_be_bytes().to_vec(); // no mask
    extra.extend(0u32.to_be_bytes()); // no blending ranges
    extra.extend([3u8, b'Z', b'i', b'p']);
    record.extend((extra.len() as u32).to_be_bytes());
    record.extend(extra);

    let mut info = 1i16.to_be_bytes().to_vec();
    info.extend(record);
    for plane in &planes {
        info.extend(plane);
    }
    let mut section = (info.len() as u32).to_be_bytes().to_vec();
    section.extend(info);
    section.extend(0u32.to_be_bytes());

    let mut bytes = b"8BPS\0\x01\0\0\0\0\0\0".to_vec();
    bytes.extend(3u16.to_be_bytes());
    bytes.extend((h as u32).to_be_bytes());
    bytes.extend((w as u32).to_be_bytes());
    bytes.extend(8u16.to_be_bytes());
    bytes.extend(3u16.to_be_bytes());
    bytes.extend(0u32.to_be_bytes()); // colour mode data
    bytes.extend(0u32.to_be_bytes()); // image resources
    bytes.extend((section.len() as u32).to_be_bytes());
    bytes.extend(section);
    bytes.extend(0u16.to_be_bytes());
    bytes.extend(vec![0u8; w * h * 3]);
    bytes
}

// ------------------------------------------------------------ unit cases --

#[test]
fn the_zip_decoder_inverts_the_independent_encoder_at_every_width() {
    for width in [1usize, 64, 300] {
        for rows in [1usize, 3] {
            let samples: Vec<u16> = (0..width * rows)
                .map(|i| match i % 11 {
                    // Long equal runs (zero deltas), a wrap across 0, and a
                    // sample that is only one code from its neighbour.
                    0..=4 => 65000,
                    5 => 3,
                    6 => 65535,
                    7 => 0,
                    8 => 1,
                    _ => (i * 7919 % 65521) as u16,
                })
                .collect();
            for predicted in [false, true] {
                let plane = fixture::zip_plane(&samples, width, predicted);
                let decoded = unpack_zip16(&plane[2..], width * 2, rows, predicted)
                    .expect("the plane decodes");
                let got: Vec<u16> = decoded
                    .chunks_exact(2)
                    .map(|s| u16::from_be_bytes([s[0], s[1]]))
                    .collect();
                assert_eq!(got, samples, "{width} x {rows}, predicted {predicted}");
                // A row never predicts across into the next one.
                if predicted && rows > 1 {
                    assert_eq!(&got[..width], &samples[..width]);
                }
            }
        }
    }
}

#[test]
fn a_short_long_corrupt_or_headerless_zip_plane_refuses() {
    let samples: Vec<u16> = (0..64).map(|i| i as u16 * 601).collect();
    let plane = fixture::zip_plane(&samples, 8, true);
    // The declared plane is one row longer than the stream decodes to.
    assert!(unpack_zip16(&plane[2..], 16, 9, true).is_err(), "short");
    // …and one row shorter: the inflater's LIMIT is the admitted size, so an
    // over-long stream never allocates past it.
    let error = unpack_zip16(&plane[2..], 16, 7, true).expect_err("over limit");
    assert!(!error.is_empty(), "{error}");
    let mut corrupt = plane.clone();
    let last = corrupt.len() - 1;
    corrupt[last] ^= 0xff; // the Adler-32 check value
    assert!(unpack_zip16(&corrupt[2..], 16, 8, true).is_err(), "corrupt");
    // A byte the zlib stream does not own is refused, exactly as the RLE
    // path refuses trailing data — the plane decodes, but the channel is
    // longer than what produced it.
    let mut trailing = plane[2..].to_vec();
    trailing.push(0);
    assert_eq!(
        unpack_zip16(&trailing, 16, 8, true).expect_err("trailing"),
        "PSD ZIP channel has trailing data"
    );
    for header in [vec![0u8, 0], vec![0x78, 0x02], vec![0x18, 0x01], vec![0x78]] {
        assert_eq!(
            unpack_zip16(&header, 16, 8, true).expect_err("bad header"),
            "PSD ZIP channel has an invalid zlib header"
        );
    }
}

// -------------------------------------------------------- the whole file --

#[test]
fn a_photoshop_shaped_lr16_file_imports_layered_and_opens_sample_exact() {
    let directory = TempDir::new().expect("tempdir");
    let source = zip16();
    let path = write(&directory, "lr16.psd", &source.bytes);

    let notice = report(&path);
    assert_eq!(notice["depth"], Value::from(16));
    assert_eq!(notice["import_precision"], Value::from("rgba16"));
    assert_eq!(notice["layered_available"], Value::Bool(true), "{notice}");
    assert_eq!(notice["layered_refusal"], Value::Null, "{notice}");
    assert_eq!(notice["layer_count"], Value::from(4), "{notice}");
    assert_eq!(notice["group_count"], Value::from(1));
    let losses = strings(&notice, "verified_losses");
    assert!(
        losses
            .iter()
            .all(|loss| !loss.contains("raster mask channel")),
        "the zero-rectangle mask body of the fill record is not a loss: {losses:?}"
    );
    assert!(
        !losses.iter().any(|loss| loss.contains("(“Fill”)")),
        "the carried SoCo costs no loss: {losses:?}"
    );
    assert!(
        strings(&notice, "observations").iter().any(|o| o.contains(
            "(“Fill”) is a Photoshop Solid Color Fill layer (SoCo) with no pixels; layered import carries it"
        )),
        "{notice}"
    );

    let document = open_typed(&path, LAYERS);
    let info = info(&document);
    let layers = info["layers"].as_array().expect("layers");
    let names: Vec<&str> = layers.iter().map(|l| l["name"].as_str().unwrap()).collect();
    // `dr_precision_doc_info` lists the stack postorder, so a folder's
    // members precede it.
    assert_eq!(names, ["Base", "Zip", "Predicted", "Folder", "Fill"]);
    let depths: Vec<u64> = layers
        .iter()
        .map(|l| l["depth"].as_u64().unwrap())
        .collect();
    assert_eq!(depths, [0, 0, 1, 0, 0]);
    // The carried fill: Photoshop's colour over the canvas, and the zero-
    // rectangle mask body is still no mask — its comp-3 `-2` plane is never
    // decoded.
    let numbers = common::psd_photoshop::rgbc_numbers(&common::psd_photoshop::soco_body());
    common::psd_photoshop::assert_solid_description(
        document
            .artwork_description(4)
            .expect("the fill's description"),
        numbers,
        CANVAS,
        "lr16",
    );
    assert!(document.layer(4).unwrap().mask().is_none(), "no mask");
    for pixel in layer_pixels(&document, 4) {
        for c in 0..3 {
            assert!(
                (f64::from(pixel[c]) - numbers[c] * 257.0).abs() <= 1.0,
                "{pixel:?} vs {numbers:?}"
            );
        }
        assert_eq!(pixel[3], 65535);
    }
    assert_eq!(layer_pixels(&document, 0), source.base, "the Raw plane");
    assert_eq!(layer_pixels(&document, 1), source.zipped, "the ZIP plane");
    assert_eq!(
        layer_pixels(&document, 2),
        source.predicted,
        "the ZIP-with-prediction plane"
    );
    let mask = document
        .layer(2)
        .and_then(|layer| layer.mask())
        .expect("the predicted mask");
    assert_eq!(mask.plane().samples(), &source.mask[..]);
    assert_eq!(mask.plane().dimensions(), CANVAS);
    assert_eq!(mask.properties().outside_coverage, 0);
    assert!(mask.enabled());
    assert!(!mask.properties().linked);

    // The merged-alpha sign lives in the `Lr16` count, so the stored
    // composite is still read as four white-matted channels.
    let stored = open_typed(&path, FLATTENED);
    assert_eq!(layer_pixels(&stored, 0), source.composite);
}

#[test]
fn an_ordinary_layer_information_section_wins_over_a_tail_block() {
    let directory = TempDir::new().expect("tempdir");
    let (w, h) = (CANVAS.0 as usize, CANVAS.1 as usize);
    let base = picture(w, h, 77);
    let mut bytes = fixture::fixture(
        CANVAS,
        &[Record::raster(
            "Ordinary",
            [0, 0, h as i32, w as i32],
            planes(&base, w, 0),
        )],
        IccProfile::srgb().bytes(),
        (72.0, 72.0),
        &vec![[0u16, 0, 0, 65535]; w * h],
        false,
        false,
    );
    // A tail `Lr16` whose body is nonsense: a file with a real
    // layer-information section must never look at it.
    let at = section_length_at(&bytes);
    let length = u32_at(&bytes, at);
    let mut tail = fixture::block(b"Lr16", &[0xff; 16]);
    let composite = bytes.split_off(at + 4 + length);
    bytes[at..at + 4].copy_from_slice(&((length + tail.len()) as u32).to_be_bytes());
    bytes.append(&mut tail);
    bytes.extend(composite);

    let path = write(&directory, "both.psd", &bytes);
    let notice = report(&path);
    assert_eq!(notice["layered_refusal"], Value::Null, "{notice}");
    assert_eq!(notice["layer_count"], Value::from(1));
    let document = open_typed(&path, LAYERS);
    assert_eq!(
        info(&document)["layers"][0]["name"],
        Value::from("Ordinary")
    );
    assert_eq!(layer_pixels(&document, 0), base);
}

#[test]
fn a_bad_tail_signature_refuses_layers_and_keeps_the_stored_composite() {
    let directory = TempDir::new().expect("tempdir");
    let source = zip16();
    let mut bytes = source.bytes.clone();
    let at = section_length_at(&bytes) + 4 + 8; // past both empty sub-sections
    bytes[at..at + 4].copy_from_slice(b"XXXX");
    let path = write(&directory, "badtail.psd", &bytes);
    let notice = report(&path);
    assert_eq!(notice["layered_available"], Value::Bool(false));
    assert!(
        notice["layered_refusal"]
            .as_str()
            .unwrap_or_default()
            .contains("invalid PSD tagged layer block signature"),
        "{notice}"
    );
    assert_eq!(notice["stored_composite_available"], Value::Bool(true));
    let stored = open_typed(&path, FLATTENED);
    assert_eq!(layer_pixels(&stored, 0), source.composite);
}

/// 8-bit ZIP channels and ZIP composites are read (`psd_zip8.rs`
/// holds their pixel oracles). What still refuses is a plane that only CLAIMS
/// to be ZIP: a composite whose compression word says 2 over bytes that are
/// not a zlib stream.
#[test]
fn eight_bit_zip_channels_open_and_a_composite_that_is_not_zlib_refuses() {
    let directory = TempDir::new().expect("tempdir");
    let path = write(&directory, "zip8.psd", &eight_bit_zip());
    let notice = report(&path);
    assert_eq!(notice["depth"], Value::from(8));
    assert_eq!(notice["layered_refusal"], Value::Null, "{notice}");
    assert_eq!(notice["layered_available"], Value::Bool(true), "{notice}");

    let source = zip16();
    let mut bytes = source.bytes.clone();
    let at = section_length_at(&bytes);
    let composite = at + 4 + u32_at(&bytes, at);
    bytes[composite..composite + 2].copy_from_slice(&2u16.to_be_bytes());
    let path = write(&directory, "zipcomposite.psd", &bytes);
    let notice = report(&path);
    assert_eq!(notice["layered_available"], Value::Bool(true), "{notice}");
    assert_eq!(
        notice["stored_composite_available"],
        Value::Bool(false),
        "{notice}"
    );
    // The bytes after the word are the composite's own planes, which do not
    // begin with a zlib header; the header check is the one both ZIP paths
    // share, so its sentence says "channel".
    assert_eq!(
        notice["stored_composite_refusal"],
        Value::from("PSD ZIP channel has an invalid zlib header"),
        "{notice}"
    );
}
