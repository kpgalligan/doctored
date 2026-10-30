//! Wide-gamut style colours in PSD: an effect's `RGBC` in a Display
//! P3 file reads back as the slot's `linear` when it is not an sRGB byte, and
//! a wide colour writes the document's numbers — integers in an 8-bit file,
//! fractions in a 16-bit one. An sRGB file's integer colours add no key.
//!
//! Every file read here is an independent byte construction
//! (`common/psd_mask_fixture.rs`, `common/psd16_fixture.rs`,
//! `common/psd_descriptor_fixture.rs`); the oracle for P3 red is the
//! published linear Display P3 → linear sRGB matrix.

use serde_json::Value;
use tempfile::TempDir;

use doctored_core::icc::IccProfile;

use crate::psd16_fixture as fixture16;
use crate::psd_descriptor_fixture::{boolean, objc, rgbc};
use crate::psd_layer_effects::{
    block_body, file8, import_report, lfx2, open_psd, open_typed, resource, save, save16,
    with_resources,
};
use crate::psd_mask_fixture as fixture8;

const LIMIT: u64 = doctored_core::precision_image::REFERENCE_WORKING_BYTES;
/// Linear Display P3 red in linear sRGB components (the published matrix's
/// first column).
const P3_RED: [f64; 3] = [1.224_940_1, -0.042_056_9, -0.019_637_6];

/// An `lfx2` body holding one Color Overlay of `rgb`.
fn overlay(rgb: [f64; 3]) -> Vec<u8> {
    lfx2(&[
        boolean(b"masterFXSwitch", 1),
        objc(
            b"SoFi",
            b"SoFi",
            &[boolean(b"enab", 1), rgbc(b"Clr ", rgb[0], rgb[1], rgb[2])],
        ),
    ])
}

fn effect0(json: &str) -> Value {
    let style: Value = serde_json::from_str(json).unwrap();
    style["effects"][0].clone()
}

fn linear_of(effect: &Value) -> [f64; 3] {
    let list = effect["linear"]
        .as_array()
        .unwrap_or_else(|| panic!("a linear key: {effect}"));
    [0, 1, 2].map(|c| list[c].as_f64().unwrap())
}

/// The doubles of the one `RGBC` an `lfx2` body holds (its `Rd  `, `Grn `
/// and `Bl  ` `doub` items).
fn written_rgbc(body: &[u8]) -> [f64; 3] {
    [&b"Rd  doub"[..], b"Grn doub", b"Bl  doub"].map(|key| {
        let at = body
            .windows(key.len())
            .position(|w| w == key)
            .expect("an RGBC component")
            + key.len();
        f64::from_be_bytes(body[at..at + 8].try_into().unwrap())
    })
}

#[test]
fn an_8_bit_p3_red_imports_wide_and_exports_red() {
    let dir = TempDir::new().unwrap();
    let p3 = IccProfile::display_p3();
    let bytes = with_resources(
        file8(
            fixture8::block(b"lfx2", &overlay([255.0, 0.0, 0.0])),
            Vec::new(),
        ),
        &resource(1039, p3.bytes()),
    );
    let path = dir.path().join("p3-red.psd");
    std::fs::write(&path, &bytes).unwrap();
    let document = open_psd(&path, &import_report(&path));
    let effect = effect0(&document.layers[0].style.as_deref().unwrap().to_json());
    let linear = linear_of(&effect);
    for c in 0..3 {
        assert!(
            (linear[c] - P3_RED[c]).abs() <= 2e-3,
            "channel {c}: {linear:?} against {P3_RED:?}"
        );
    }
    assert_eq!(effect["color"], "#ff0000", "its nearest sRGB byte");
    let out = dir.path().join("p3-red-out.psd");
    save(&document, &out).expect("save");
    let written = std::fs::read(&out).unwrap();
    let body = block_body(&written, b"lfx2").expect("an lfx2 block");
    assert_eq!(written_rgbc(&body), [255.0, 0.0, 0.0]);
}

fn file16_p3(blocks: Vec<u8>) -> Vec<u8> {
    let px: [[u16; 4]; 2] = [[4097, 51234, 60000, 65535], [4098, 51233, 60000, 32768]];
    let planes = [(-1i16, 3usize), (0, 0), (1, 1), (2, 2)]
        .into_iter()
        .map(|(id, component)| {
            (
                id,
                fixture16::raw_plane(&px.iter().map(|p| p[component]).collect::<Vec<_>>()),
            )
        })
        .collect();
    let mut record = fixture16::Record::raster("Styled", [0, 0, 1, 2], planes);
    record.blocks.extend(blocks);
    fixture16::fixture(
        (2, 1),
        &[record],
        IccProfile::display_p3().bytes(),
        (72.0, 72.0),
        &px,
        true,
        false,
    )
}

#[test]
fn a_16_bit_p3_fraction_round_trips() {
    let dir = TempDir::new().unwrap();
    let authored = [200.5, 10.25, 40.75];
    let bytes = file16_p3(fixture16::block(b"lfx2", &overlay(authored)));
    let path = dir.path().join("p3-16.psd");
    std::fs::write(&path, &bytes).unwrap();
    let report = import_report(&path);
    assert_eq!(report["import_precision"], "rgba16", "{report}");
    let document = open_typed(&path, &report);
    let effect = effect0(&document.layer(0).unwrap().reported_style().unwrap());
    assert!(effect["linear"].is_array(), "{effect}");
    let projection = document.render(LIMIT, 0, None).unwrap();
    let out = dir.path().join("p3-16-out.psd");
    save16(&document, &projection, &out).expect("save16");
    let written = std::fs::read(&out).unwrap();
    let body = block_body(&written, b"lfx2").expect("an lfx2 block");
    let got = written_rgbc(&body);
    for c in 0..3 {
        assert!(
            (got[c] - authored[c]).abs() <= 1e-3,
            "channel {c}: {got:?} against {authored:?}"
        );
    }
}

#[test]
fn srgb_integer_colours_add_no_key_at_either_depth() {
    let dir = TempDir::new().unwrap();
    let srgb = IccProfile::srgb();
    let bytes = with_resources(
        file8(
            fixture8::block(b"lfx2", &overlay([10.0, 200.0, 37.0])),
            Vec::new(),
        ),
        &resource(1039, srgb.bytes()),
    );
    let path = dir.path().join("srgb8.psd");
    std::fs::write(&path, &bytes).unwrap();
    let document = open_psd(&path, &import_report(&path));
    let effect = effect0(&document.layers[0].style.as_deref().unwrap().to_json());
    assert!(effect.get("linear").is_none(), "{effect}");
    assert_eq!(effect["color"], "#0ac825");
    // Photoshop's own byte colours sit on its 15-bit scale
    // (`blend_if/source.psd` writes 129 as 129.0019): still bytes, no key.
    let bytes = with_resources(
        file8(
            fixture8::block(
                b"lfx2",
                &overlay([129.001_922_607_421_88, 215.000_610_351_562_5, 0.0]),
            ),
            Vec::new(),
        ),
        &resource(1039, srgb.bytes()),
    );
    let path = dir.path().join("srgb8-15bit.psd");
    std::fs::write(&path, &bytes).unwrap();
    let document = open_psd(&path, &import_report(&path));
    let effect = effect0(&document.layers[0].style.as_deref().unwrap().to_json());
    assert!(effect.get("linear").is_none(), "{effect}");
    assert_eq!(effect["color"], "#81d700");

    let px: [[u16; 4]; 2] = [[1, 2, 3, 65535], [4, 5, 6, 65535]];
    let planes = [(-1i16, 3usize), (0, 0), (1, 1), (2, 2)]
        .into_iter()
        .map(|(id, component)| {
            (
                id,
                fixture16::raw_plane(&px.iter().map(|p| p[component]).collect::<Vec<_>>()),
            )
        })
        .collect();
    let mut record = fixture16::Record::raster("Styled", [0, 0, 1, 2], planes);
    record
        .blocks
        .extend(fixture16::block(b"lfx2", &overlay([10.0, 200.0, 37.0])));
    let bytes = fixture16::fixture(
        (2, 1),
        &[record],
        srgb.bytes(),
        (72.0, 72.0),
        &px,
        true,
        false,
    );
    let path = dir.path().join("srgb16.psd");
    std::fs::write(&path, &bytes).unwrap();
    let report = import_report(&path);
    let document = open_typed(&path, &report);
    let effect = effect0(&document.layer(0).unwrap().reported_style().unwrap());
    assert!(effect.get("linear").is_none(), "{effect}");
    assert_eq!(effect["color"], "#0ac825");
}
