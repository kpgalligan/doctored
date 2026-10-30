//! #1 spot channels 1c-2b (`tiff_spot`'s LAYERED paragraph,
//! `tiff_layers_write::write_spots`): the layered TIFF's spot channels in
//! and out, through the C API.
//!
//! Oracles, all written here or in `spot_tiff.rs` (its TIFF directory and
//! strip reader and its 8BIM resource reader): the layout Photoshop 26.10.0
//! writes in its own layered TIFFs (`scripts/photoshop-probe/spot_tiff_take.py`
//! take2's `rgb8_l.tif` and `cmyk16_l.tif`, read in
//! `spot_tiff_photoshop.rs`) is the flat file's: the spot channels are the
//! composite's trailing extra samples (ExtraSamples 0) after the
//! transparency, stored `max − coverage`, named and inked by resources
//! 1006/1045/1077/1053 in tag 34377 led by "Transparency", with tag 37724
//! (the layer block) beside them. Never a golden file.

use std::path::Path;
use std::ptr;

use doctored_core::doc::DrDocument;
use doctored_core::ffi_psd::dr_precision_doc_save_psd;
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::PrecisionDocument;
use doctored_core::precision_image::PrecisionImage;
use doctored_core::spot_channel::{SpotInk, TIFF_REFUSAL};
use image::{Rgba, RgbaImage};
use tempfile::TempDir;

use crate::cmyk_fixture::press_profile;
use crate::common::{cpath, take_err_string};
use crate::psb_fixture::{
    export_report16, export_report8, import_report, open16, open8, save16, save8, strings,
    FLATTENED, LAYERS, TIFF,
};
use crate::spot_tiff::{
    byte_spots, open_byte, open_typed, read_tiff, resources, typed_spots, Tiff,
};

const LIMIT: u64 = 2 * 1024 * 1024 * 1024;
const W: u32 = 32;
const H: u32 = 8;
const RAMP: &str = "Spot Ramp";
const BLOC: &str = "Spot Bloc \u{e9}";

fn ramp_ink() -> SpotInk {
    SpotInk::new([55.25, 70.5, -30.75], 40).unwrap()
}

fn bloc_ink() -> SpotInk {
    SpotInk::new([80.0, -20.0, 60.0], 100).unwrap()
}

fn ramp8() -> Vec<u8> {
    (0..H)
        .flat_map(|y| (0..W).map(move |x| ((x * 37 + y * 11) % 256) as u8))
        .collect()
}

fn bloc8() -> Vec<u8> {
    (0..H)
        .flat_map(|y| {
            (0..W).map(move |x| u8::from((8..24).contains(&x) && (2..6).contains(&y)) * 255)
        })
        .collect()
}

fn ramp16() -> Vec<u16> {
    (0..H * W)
        .map(|i| ((i * 2053 + 17) % 65536) as u16)
        .collect()
}

fn wide(plane: &[u8]) -> Vec<u16> {
    plane.iter().map(|&v| u16::from(v) * 257).collect()
}

fn byte_base() -> DrDocument {
    DrDocument::from_pixels(RgbaImage::from_fn(W, H, |x, y| {
        Rgba([(x * 8) as u8, (y * 30) as u8, 90, 255])
    }))
}

fn byte_doc() -> DrDocument {
    byte_base()
        .add_spot_channel(RAMP, ramp_ink(), &ramp8())
        .unwrap()
        .add_spot_channel(BLOC, bloc_ink(), &bloc8())
        .unwrap()
}

fn rgb16() -> PrecisionDocument {
    let pixels = (0..W * H)
        .map(|i| [(i * 1500) as u16, 40000, (60000 - i * 200) as u16, 65535])
        .collect();
    let image = PrecisionImage::from_rgba16(W, H, pixels, IccProfile::srgb(), None).unwrap();
    PrecisionDocument::from_image(image, Metadata::default(), Resolution::default()).unwrap()
}

fn cmyk(bits: u16) -> PrecisionDocument {
    rgb16()
        .convert_to_cmyk(
            Some(&press_profile("Press Test")),
            &format!(r#"{{"depth":{bits}}}"#),
            LIMIT,
            0,
            None,
        )
        .unwrap()
}

fn with_spots(doc: &PrecisionDocument, ramp: &[u16], bloc: &[u16]) -> PrecisionDocument {
    doc.add_spot_channel(RAMP, ramp_ink(), ramp, LIMIT, 0, None)
        .unwrap()
        .add_spot_channel(BLOC, bloc_ink(), bloc, LIMIT, 0, None)
        .unwrap()
}

/// A typed document saved with no projection (a CMYK document's form).
fn save_cmyk(doc: &PrecisionDocument, path: &Path, mode: i32) {
    let mut err = ptr::null_mut();
    let ok = unsafe {
        dr_precision_doc_save_psd(
            doc,
            ptr::null(),
            cpath(path).as_ptr(),
            TIFF,
            mode,
            LIMIT,
            0,
            &mut err,
        )
    };
    assert!(ok, "{}: {}", path.display(), take_err_string(err));
}

fn observation() -> String {
    format!("2 spot channels open as spot channels with their names, inks (D50 Lab) and solidity: \u{201c}{RAMP}\u{201d}, \u{201c}{BLOC}\u{201d}.")
}

/// A 1077 record, (colour space, four words, opacity, kind), as bytes.
fn display_record(space: u16, words: [u16; 4], opacity: u16, kind: u8) -> Vec<u8> {
    let mut out = space.to_be_bytes().to_vec();
    words
        .iter()
        .for_each(|w| out.extend_from_slice(&w.to_be_bytes()));
    out.extend_from_slice(&opacity.to_be_bytes());
    out.push(kind);
    out
}

/// The file's layout as the layered spot rule writes it: the colour samples,
/// the straight transparency, then the two spot samples (ExtraSamples 0)
/// at `bits`; the spot samples `max − coverage` (`coverage` at the file's
/// depth); resources 1045, 1077 and 1053 led by the transparency's entry and
/// 1006 present; tag 37724 beside them.
fn assert_layered_spots(tiff: &Tiff, colour: usize, bits: u32, coverage: &[Vec<u16>], what: &str) {
    let samples = colour + 1 + coverage.len();
    assert_eq!(tiff.one(277) as usize, samples, "{what}: SamplesPerPixel");
    assert_eq!(tiff.tag(258), vec![bits; samples], "{what}: BitsPerSample");
    assert_eq!(tiff.tag(338), vec![2, 0, 0], "{what}: ExtraSamples");
    let max = if bits == 16 { 65535 } else { 255 };
    for (k, plane) in coverage.iter().enumerate() {
        let want: Vec<u16> = plane.iter().map(|&c| max - c).collect();
        assert_eq!(tiff.planes[colour + 1 + k], want, "{what}: spot {k}, H1");
    }
    assert!(tiff.bytes.contains_key(&37724), "{what}: the layer block");
    let found = resources(tiff.bytes.get(&34377).expect("tag 34377"));
    assert!(
        found[&1006].starts_with(b"\x0cTransparency"),
        "{what}: 1006 led by the transparency"
    );
    let mut unicode = Vec::new();
    for name in ["Transparency", RAMP, BLOC] {
        let units: Vec<u16> = name.encode_utf16().chain([0]).collect();
        unicode.extend_from_slice(&(units.len() as u32).to_be_bytes());
        units
            .iter()
            .for_each(|u| unicode.extend_from_slice(&u.to_be_bytes()));
    }
    assert_eq!(found[&1045], unicode, "{what}: 1045");
    let word = |v: f64| (v * 100.0).round() as i16 as u16;
    let mut display = 1u32.to_be_bytes().to_vec();
    display.extend(display_record(0, [65535, 0, 0, 0], 100, 1));
    for ink in [ramp_ink(), bloc_ink()] {
        let lab = ink.lab();
        display.extend(display_record(
            7,
            [word(lab[0]), word(lab[1]), word(lab[2]), 0],
            u16::from(ink.solidity()),
            2,
        ));
    }
    assert_eq!(found[&1077], display, "{what}: 1077");
    let ids: Vec<u8> = [0u32, 3, 4].iter().flat_map(|i| i.to_be_bytes()).collect();
    assert_eq!(found[&1053], ids, "{what}: 1053");
}

fn narrow(plane: &[u8]) -> Vec<u16> {
    plane.iter().map(|&v| u16::from(v)).collect()
}

#[test]
fn a_byte_layered_tiff_carries_its_spot_channels_in_both_modes() {
    let dir = TempDir::new().unwrap();
    let doc = byte_doc();
    let plain = dir.path().join("plain.tif");
    save8(&byte_base(), &plain, TIFF, LAYERS).unwrap();
    let plain = read_tiff(&std::fs::read(&plain).unwrap());
    for mode in [LAYERS, FLATTENED] {
        let what = format!("byte, mode {mode}");
        let report = export_report8(&doc, TIFF, mode);
        assert!(
            strings(&report, "observations")
                .iter()
                .any(|o| o.starts_with("2 spot channels are written as Photoshop spot channels")),
            "{what}: {report}"
        );
        let path = dir.path().join(format!("byte{mode}.tif"));
        save8(&doc, &path, TIFF, mode).unwrap();
        let tiff = read_tiff(&std::fs::read(&path).unwrap());
        assert_eq!(tiff.one(262), 2, "{what}: RGB");
        assert_layered_spots(&tiff, 3, 8, &[narrow(&ramp8()), narrow(&bloc8())], &what);
        assert_eq!(
            tiff.planes[..4],
            plain.planes[..4],
            "{what}: the composite's own samples are the spotless file's"
        );
        assert!(
            strings(&import_report(&path), "observations").contains(&observation()),
            "{what}"
        );
        let back = open8(&path, LAYERS).unwrap();
        assert_eq!(byte_spots(&back), byte_spots(&doc), "{what}: layered open");
        assert_eq!(
            byte_spots(&open_byte(&path)),
            byte_spots(&doc),
            "{what}: flat open"
        );
    }
}

#[test]
fn a_sixteen_bit_rgb_layered_tiff_carries_its_spot_channels() {
    let dir = TempDir::new().unwrap();
    let bloc = wide(&bloc8());
    let doc = with_spots(&rgb16(), &ramp16(), &bloc);
    for mode in [LAYERS, FLATTENED] {
        let what = format!("rgb16, mode {mode}");
        let path = dir.path().join(format!("rgb16{mode}.tif"));
        save16(&doc, &path, TIFF, mode).unwrap();
        let tiff = read_tiff(&std::fs::read(&path).unwrap());
        assert_eq!(tiff.one(262), 2, "{what}: RGB");
        assert_layered_spots(&tiff, 3, 16, &[ramp16(), bloc.clone()], &what);
        assert!(
            strings(&import_report(&path), "observations").contains(&observation()),
            "{what}"
        );
        let back = open16(&path, LAYERS).unwrap();
        assert_eq!(
            typed_spots(&back),
            typed_spots(&doc),
            "{what}: layered open"
        );
        let flat = open_typed(&path).unwrap();
        assert_eq!(typed_spots(&flat), typed_spots(&doc), "{what}: flat open");
    }
}

#[test]
fn a_cmyk_layered_tiff_carries_its_spot_channels_at_eight_and_sixteen_bits() {
    let dir = TempDir::new().unwrap();
    for bits in [8u16, 16] {
        let what = format!("cmyk {bits}");
        // At 8 bits every coverage is a whole code ×257, so it survives.
        let ramp = if bits == 8 { wide(&ramp8()) } else { ramp16() };
        let bloc = wide(&bloc8());
        let doc = with_spots(&cmyk(bits), &ramp, &bloc);
        let path = dir.path().join(format!("cmyk{bits}.tif"));
        save_cmyk(&doc, &path, LAYERS);
        let tiff = read_tiff(&std::fs::read(&path).unwrap());
        assert_eq!(tiff.one(262), 5, "{what}: separated");
        let file = |c: &[u16]| -> Vec<u16> {
            if bits == 8 {
                c.iter().map(|v| v / 257).collect()
            } else {
                c.to_vec()
            }
        };
        assert_layered_spots(
            &tiff,
            4,
            u32::from(bits),
            &[file(&ramp), file(&bloc)],
            &what,
        );
        assert!(
            strings(&import_report(&path), "observations").contains(&observation()),
            "{what}"
        );
        let back = open16(&path, LAYERS).unwrap();
        assert!(back.is_cmyk(), "{what}");
        assert_eq!(
            typed_spots(&back),
            typed_spots(&doc),
            "{what}: layered open"
        );
        let flat = open_typed(&path).unwrap();
        assert!(flat.is_cmyk(), "{what}");
        assert_eq!(typed_spots(&flat), typed_spots(&doc), "{what}: flat open");
    }
}

#[test]
fn a_cmyk_flattened_tiff_export_is_admitted_at_the_flat_tiff_saves_plan() {
    for bits in [8u16, 16] {
        let base = cmyk(bits);
        let doc = with_spots(&base, &wide(&ramp8()), &wide(&bloc8()));
        let plan = |d: &PrecisionDocument| {
            export_report16(d, TIFF, FLATTENED)["working_memory_plan_bytes"]
                .as_u64()
                .unwrap()
        };
        let save = doc.save_cmyk_plan(r#"{"format":"tiff"}"#).unwrap()["working_memory_plan_bytes"]
            .as_u64()
            .unwrap();
        assert_eq!(plan(&doc), save, "{bits}: the flat CMYK TIFF save's plan");
        assert!(
            plan(&doc) > plan(&base),
            "{bits}: the spot planes are charged"
        );
    }
}

#[test]
fn a_layered_tiff_without_spot_channels_declares_none() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("plain.tif");
    save8(&byte_base(), &path, TIFF, LAYERS).unwrap();
    let tiff = read_tiff(&std::fs::read(&path).unwrap());
    assert_eq!((tiff.one(277), tiff.tag(338)), (4, vec![2]));
    let found = tiff.bytes.get(&34377).map(|run| resources(run));
    assert!(
        !found.is_some_and(|r| r.contains_key(&1077)),
        "no DisplayInfo"
    );
}

#[test]
fn gray_lab_and_float_documents_refuse_a_layered_tiff_with_spot_channels() {
    let dir = TempDir::new().unwrap();
    let gray = rgb16()
        .convert_to_gray(
            None,
            r#"{"space":"gray_gamma_22","depth":16}"#,
            LIMIT,
            0,
            None,
        )
        .unwrap();
    let lab = rgb16()
        .convert_to_lab(r#"{"depth":16}"#, LIMIT, 0, None)
        .unwrap();
    // A 32-bit float document: the float fixture, its spot planes at its size.
    let float =
        PrecisionDocument::open_preserving("tests/fixtures/float_11f/scene.tiff", LIMIT, 0, None)
            .unwrap();
    assert!(float.is_float());
    for (name, doc) in [("gray", gray), ("lab", lab), ("float", float)] {
        let (w, h) = doc.dimensions();
        let plane: Vec<u16> = (0..w * h).map(|i| (i * 977 % 65536) as u16).collect();
        let doc = with_spots(&doc, &plane, &plane);
        for mode in [LAYERS, FLATTENED] {
            let report = export_report16(&doc, TIFF, mode);
            assert_eq!(report["available"], false, "{name} {mode}");
            assert_eq!(report["refusal"], TIFF_REFUSAL, "{name} {mode}");
            let path = dir.path().join(format!("{name}{mode}.tif"));
            assert_eq!(
                save16(&doc, &path, TIFF, mode).err().unwrap_or_default(),
                TIFF_REFUSAL,
                "{name} {mode}"
            );
            assert!(!path.exists(), "{name} {mode}: no file");
        }
    }
}
