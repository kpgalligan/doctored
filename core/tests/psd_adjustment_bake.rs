//! Adjustment layers in a layered export never refuse: one with a
//! Photoshop record is written as that record — Brightness/Contrast (`brit`
//! with its `CgEd`) and Selective Color (`selc`) are the two this batch adds —
//! and any other is BAKED into a pixel layer holding the document's picture up
//! to and including it.
//!
//! Oracles. A baked layer's pixels are compared with the projection of a
//! document BUILT WITHOUT the layers above it, so the stamp is checked against
//! the ordinary compositor rather than against itself. The written records
//! are read by `common/psb_fixture.rs`, and the `selc` and `brit` bodies by
//! parsers written here from Adobe's "Selective color file format" table and
//! from the Photoshop-written `CgEd` in `fixtures/psd_photoshop/depth16`.
use std::sync::Arc;

use doctored_core::doc::DrDocument;
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer};
use doctored_core::precision_image::PrecisionImage;
use image::RgbaImage;
use serde_json::{json, Value};
use tempfile::TempDir;

use crate::psb_fixture::{self as fixture, pattern, LAYERS, PSB, PSD};

const W: u32 = 6;
const H: u32 = 4;

const HOLDS: &str = "It holds this document's picture up to and including that layer. The layers beneath it stay in the file but no longer show through it, and where they are partly transparent the baked layer adds its own coverage.";

fn meta(op: &str, params: &Value) -> String {
    json!({"type": "adjust", "op": op, "params": params}).to_string()
}

fn gradient() -> Value {
    json!({"gradient": {"stops": [
        {"position": 0.0, "color": "#102030", "opacity": 1.0},
        {"position": 1.0, "color": "#f0c040", "opacity": 1.0}
    ]}})
}

fn base() -> RgbaImage {
    let pixels: Vec<u8> = pattern(W, H, 9, 255)
        .iter()
        .flatten()
        .map(|v| *v as u8)
        .collect();
    RgbaImage::from_raw(W, H, pixels).unwrap()
}

/// An opaque base with one adjustment layer above it.
fn adjusted(op: &str, params: &Value, name: &str) -> DrDocument {
    let mut document = DrDocument::from_pixels(base())
        .adding_image_layer(0, RgbaImage::new(W, H), name)
        .expect("add layer");
    document.layers[1].meta = Some(meta(op, params));
    document
}

fn rgba8(image: &RgbaImage) -> Vec<[u16; 4]> {
    image.pixels().map(|p| p.0.map(u16::from)).collect()
}

fn written(
    document: &DrDocument,
    container: i32,
) -> (fixture::Parsed, std::path::PathBuf, TempDir) {
    let directory = TempDir::new().unwrap();
    let path = directory.path().join("out.psd");
    fixture::save8(document, &path, container, LAYERS).expect("save");
    (
        fixture::parse(&std::fs::read(&path).unwrap()),
        path,
        directory,
    )
}

// ------------------------------------------------------------------ bake --

#[test]
fn a_gradient_map_is_baked_named_and_reopens_to_the_same_picture() {
    let document = adjusted("gradient_map", &gradient(), "Map");
    let picture = document.flattened().unwrap();
    assert_ne!(picture, base(), "the map changes the picture");
    for container in [PSD, PSB] {
        let report = fixture::export_report8(&document, container, LAYERS);
        assert_eq!(report["available"], Value::Bool(true), "{report}");
        assert_eq!(report["refusal"], Value::Null);
        assert_eq!(report["bake_scratch_bytes"], json!(3 * 4 * W * H));
        let losses = fixture::strings(&report, "losses");
        let loss = losses
            .iter()
            .find(|loss| loss.starts_with("Adjustment layer “Map”"))
            .expect("the baked layer's loss");
        assert_eq!(
            loss,
            &format!("Adjustment layer “Map” (gradient_map) is written as a pixel layer, not as Photoshop's Gradient Map record (grdm): the published format gives that record's field sizes without their units, so this build does not write it. {HOLDS}")
        );

        let (parsed, path, _keep) = written(&document, container);
        assert_eq!(parsed.layers.len(), 2);
        let record = &parsed.layers[1];
        assert_eq!(record.name, "Map");
        assert_eq!(record.bounds, [0, 0, H as i32, W as i32]);
        assert_eq!(
            (&record.blend, record.opacity, record.clipping),
            (b"norm", 255, 0)
        );
        assert!(record.block(b"grdm").is_none(), "no record is guessed at");
        assert_eq!(record.rgba(1).unwrap(), rgba8(&picture));

        let reopened = fixture::open8(&path, LAYERS).expect("reopen");
        assert_eq!(reopened.layers.len(), 2);
        assert!(reopened.layers[1].meta.is_none(), "a baked layer is pixels");
        assert_eq!(reopened.flattened().unwrap(), picture);
    }
}

#[test]
fn the_stamp_stops_at_its_layer_and_keeps_a_hidden_layer_hidden() {
    // [base, grayscale, an opaque block above]: the stamp is the picture of
    // the first two alone.
    let below = adjusted("grayscale", &json!({}), "Gray");
    let block = RgbaImage::from_pixel(2, 2, image::Rgba([250, 10, 10, 255]));
    let mut document = below.adding_image_layer(1, block, "Block").expect("add");
    document.layers[2].offset = (1, 1);
    let (parsed, path, _keep) = written(&document, PSD);
    assert_eq!(parsed.layers.len(), 3);
    assert_eq!(
        parsed.layers[1].rgba(1).unwrap(),
        rgba8(&below.flattened().unwrap()),
        "the block above contributes nothing to the stamp"
    );
    let reopened = fixture::open8(&path, LAYERS).expect("reopen");
    assert_eq!(reopened.flattened().unwrap(), document.flattened().unwrap());

    // Hidden: the record keeps its hidden flag, its pixels are what showing
    // it would show, and the reopened picture is still the base alone.
    let shown = adjusted("sepia", &json!({}), "Tone");
    let hidden = shown.with_layer_visible(1, false).expect("hide");
    assert_eq!(hidden.flattened().unwrap(), base());
    let (parsed, path, _keep) = written(&hidden, PSD);
    assert_eq!(parsed.layers[1].flags & 2, 2, "the record stays hidden");
    assert_eq!(
        parsed.layers[1].rgba(1).unwrap(),
        rgba8(&shown.flattened().unwrap())
    );
    let reopened = fixture::open8(&path, LAYERS).expect("reopen");
    assert!(!reopened.layers[1].visible);
    assert_eq!(reopened.flattened().unwrap(), base());
}

#[test]
fn the_stamp_inside_a_hidden_group_is_what_showing_the_group_would_show() {
    // [base, Set { Member, Gray }] with Set hidden. A hidden group renders
    // nothing, so a stamp that forced only the adjustment visible would be the
    // base alone and would cover Member once Set is shown in the file.
    let member = RgbaImage::from_pixel(2, 2, image::Rgba([250, 10, 10, 255]));
    let mut document = DrDocument::from_pixels(base())
        .adding_image_layer(0, member, "Member")
        .expect("add member");
    document.layers[1].offset = (1, 1);
    let mut document = document
        .adding_image_layer(1, RgbaImage::new(W, H), "Gray")
        .expect("add adjustment");
    document.layers[2].meta = Some(meta("grayscale", &json!({})));
    let (shown, group, _, _) = document.group_layers(&[1, 2], "Set").expect("group");
    let picture = shown.flattened().unwrap();
    assert_ne!(picture, base(), "the group changes the picture");
    let hidden = shown.with_layer_visible(group, false).expect("hide");
    assert_eq!(hidden.flattened().unwrap(), base());

    let (parsed, path, _keep) = written(&hidden, PSD);
    let record = parsed
        .layers
        .iter()
        .find(|layer| layer.name == "Gray")
        .expect("the baked record");
    assert_eq!(record.flags & 2, 0, "the adjustment itself is not hidden");
    assert_eq!(record.rgba(1).unwrap(), rgba8(&picture));

    // The file still opens with the group hidden, and showing it gives the
    // picture the source gives when its group is shown.
    let reopened = fixture::open8(&path, LAYERS).expect("reopen");
    assert_eq!(reopened.flattened().unwrap(), base());
    let set = reopened
        .layers
        .iter()
        .position(|layer| layer.name == "Set")
        .expect("the group");
    assert!(!reopened.layers[set].visible, "the group stays hidden");
    let revealed = reopened.with_layer_visible(set, true).expect("show");
    assert_eq!(revealed.flattened().unwrap(), picture);
}

#[test]
fn the_typed_stamp_inside_a_hidden_group_is_what_showing_the_group_would_show() {
    let directory = TempDir::new().unwrap();
    let profile = IccProfile::srgb();
    let mut description: Value = serde_json::from_str(&meta("gradient_map", &gradient())).unwrap();
    description["evaluation"] = json!("precision_v1");
    let image = |seed: u16, w: u32, h: u32| {
        PrecisionImage::from_rgba16(w, h, pattern(w, h, seed, 65535), Arc::clone(&profile), None)
            .unwrap()
    };
    let build = |group_visible: bool| {
        let members = vec![
            PrecisionDocumentLayer::raster("Member", image(5, 2, 2), (1, 1), 1.0, true).unwrap(),
            PrecisionDocumentLayer::adjustment(
                "Map",
                &description.to_string(),
                1.0,
                true,
                &profile,
            )
            .unwrap(),
        ];
        PrecisionDocument::from_parts(
            W,
            H,
            Arc::clone(&profile),
            Metadata::default(),
            Resolution::default(),
            vec![
                PrecisionDocumentLayer::raster("Base", image(3, W, H), (0, 0), 1.0, true).unwrap(),
                PrecisionDocumentLayer::isolated_normal("Set", members, 1.0, group_visible)
                    .unwrap(),
            ],
        )
        .unwrap()
    };
    let picture = fixture::rendered16(&build(true));
    let hidden = build(false);
    let beneath = fixture::rendered16(&hidden);
    assert_ne!(picture, beneath, "the group changes the picture");

    let path = directory.path().join("hidden16.psd");
    fixture::save16(&hidden, &path, PSD, LAYERS).expect("save");
    let parsed = fixture::parse(&std::fs::read(&path).unwrap());
    let record = parsed
        .layers
        .iter()
        .find(|layer| layer.name == "Map")
        .expect("the baked record");
    assert_eq!(record.rgba(2).unwrap(), picture);
    let reopened = fixture::open16(&path, LAYERS).expect("reopen");
    assert_eq!(
        fixture::rendered16(&reopened),
        beneath,
        "the group stays hidden"
    );
}

#[test]
fn a_baked_layer_that_cannot_be_followed_exactly_says_where() {
    let exact = "which a baked layer cannot follow exactly, so this file can look different there; the flattened mode is exact.";
    let loss_of = |document: &DrDocument| {
        fixture::strings(&fixture::export_report8(document, PSD, LAYERS), "losses")
            .into_iter()
            .find(|loss| loss.starts_with("Adjustment layer “Gray”"))
            .expect("loss")
    };
    let plain = adjusted("grayscale", &json!({}), "Gray");
    assert_eq!(
        loss_of(&plain),
        format!("Adjustment layer “Gray” (grayscale) has no Photoshop record and is written as a pixel layer. {HOLDS}")
    );

    let mut clipped = plain.clone();
    clipped.layers[1].clipped = true;
    assert!(
        loss_of(&clipped).ends_with(&format!(" It is clipped to the layer beneath it, {exact}")),
        "{}",
        loss_of(&clipped)
    );
    let (parsed, _, _keep) = written(&clipped, PSD);
    assert_eq!(parsed.layers[1].clipping, 1, "clipping is kept");

    let (mut grouped, group, _, _) = plain.group_layers(&[1], "Set").expect("group");
    assert!(
        !loss_of(&grouped).contains("It is"),
        "a plain group is exact"
    );
    grouped.layers[group].opacity = 0.5;
    assert!(
        loss_of(&grouped).ends_with(&format!(
            " It is inside group “Set”, which has its own blend mode, opacity, mask, style or clipping, {exact}"
        )),
        "{}",
        loss_of(&grouped)
    );
}

#[test]
fn a_typed_adjustment_without_a_record_is_baked_at_sixteen_bits() {
    let directory = TempDir::new().unwrap();
    let profile = IccProfile::srgb();
    let image =
        PrecisionImage::from_rgba16(W, H, pattern(W, H, 3, 65535), Arc::clone(&profile), None)
            .unwrap();
    let mut description: Value = serde_json::from_str(&meta("gradient_map", &gradient())).unwrap();
    description["evaluation"] = json!("precision_v1");
    let document = PrecisionDocument::from_parts(
        W,
        H,
        Arc::clone(&profile),
        Metadata::default(),
        Resolution::default(),
        vec![
            PrecisionDocumentLayer::raster("Base", image, (0, 0), 1.0, true).unwrap(),
            PrecisionDocumentLayer::adjustment(
                "Map",
                &description.to_string(),
                1.0,
                true,
                &profile,
            )
            .unwrap(),
        ],
    )
    .unwrap();
    let picture = fixture::rendered16(&document);
    let report = fixture::export_report16(&document, PSD, LAYERS);
    assert_eq!(report["available"], Value::Bool(true), "{report}");
    assert_eq!(report["bake_scratch_bytes"], json!(3 * 4 * 2 * W * H));
    assert!(fixture::strings(&report, "losses").iter().any(|loss| loss
        .contains("(gradient_map) is written as a pixel layer")
        && loss.contains("(grdm)")));

    let path = directory.path().join("out16.psd");
    fixture::save16(&document, &path, PSD, LAYERS).expect("save");
    let parsed = fixture::parse(&std::fs::read(&path).unwrap());
    assert_eq!(parsed.layers[1].name, "Map");
    assert_eq!(parsed.layers[1].rgba(2).unwrap(), picture);
    let reopened = fixture::open16(&path, LAYERS).expect("reopen");
    assert_eq!(fixture::rendered16(&reopened), picture);
}

// --------------------------------------------------------------- records --

/// The i32 after `key` + `long` in a descriptor body, read without the crate.
fn long_after(body: &[u8], key: &[u8]) -> i32 {
    let mut needle = key.to_vec();
    needle.extend_from_slice(b"long");
    let at = body
        .windows(needle.len())
        .position(|window| window == needle)
        .expect("key")
        + needle.len();
    i32::from_be_bytes(body[at..at + 4].try_into().unwrap())
}

fn bool_after(body: &[u8], key: &[u8]) -> bool {
    let mut needle = key.to_vec();
    needle.extend_from_slice(b"bool");
    let at = body
        .windows(needle.len())
        .position(|window| window == needle)
        .expect("key")
        + needle.len();
    body[at] != 0
}

#[test]
fn brightness_contrast_is_written_as_brit_with_its_cged_and_reopens() {
    for (params, brit, values, legacy) in [
        (
            json!({"brightness": 0.25, "contrast": -0.5}),
            vec![0, 25, 0xff, 0xce, 0, 127, 0, 0],
            (25, -50),
            true,
        ),
        (
            json!({"brightness": 0.2, "contrast": -0.4, "legacy": false}),
            vec![0; 8],
            (30, -20),
            false,
        ),
        (
            json!({"brightness": -1.0, "contrast": 0.6, "legacy": false}),
            vec![0; 8],
            (-150, 60),
            false,
        ),
    ] {
        let document = adjusted("bcs", &params, "Tone");
        let report = fixture::export_report8(&document, PSD, LAYERS);
        assert_eq!(
            report["bake_scratch_bytes"],
            json!(0),
            "a record is not baked"
        );
        let (parsed, path, _keep) = written(&document, PSD);
        let record = &parsed.layers[1];
        assert_eq!(record.bounds, [0; 4], "an adjustment record has no pixels");
        assert_eq!(record.block(b"brit").expect("brit"), brit.as_slice());
        let at = record
            .blocks
            .iter()
            .position(|(key, _)| key == b"brit")
            .unwrap();
        assert_eq!(
            &record.blocks[at + 1].0,
            b"CgEd",
            "CgEd follows brit directly"
        );
        let cged = record.block(b"CgEd").unwrap();
        assert_eq!(&cged[..4], &16u32.to_be_bytes());
        assert_eq!(long_after(cged, b"Vrsn"), 1);
        assert_eq!(long_after(cged, b"Brgh"), values.0);
        assert_eq!(long_after(cged, b"Cntr"), values.1);
        assert_eq!(long_after(cged, b"means"), 127);
        assert_eq!(bool_after(cged, b"useLegacy"), legacy);

        let reopened = fixture::open8(&path, LAYERS).expect("reopen");
        let read: Value =
            serde_json::from_str(reopened.layers[1].meta.as_deref().expect("meta")).unwrap();
        assert_eq!(read["op"], json!("bcs"));
        assert_eq!(read["params"]["legacy"], json!(legacy));
        for key in ["brightness", "contrast"] {
            let (got, want) = (
                read["params"][key].as_f64().unwrap(),
                params[key].as_f64().unwrap(),
            );
            assert!((got - want).abs() < 1e-12, "{key}: {got} vs {want}");
        }
        assert_eq!(reopened.flattened().unwrap(), document.flattened().unwrap());
    }
}

#[test]
fn selective_color_is_written_as_adobes_table_and_reopens() {
    let params = json!({
        "method": "absolute",
        "reds": {"c": 0.1, "m": -0.2, "y": 0.3, "k": -1.0},
        "blacks": {"c": 0.0, "m": 0.0, "y": 0.0, "k": 0.05}
    });
    let document = adjusted("selective_color", &params, "Selective");
    let (parsed, path, _keep) = written(&document, PSD);
    let body = parsed.layers[1].block(b"selc").expect("selc");
    // Adobe's "Selective color file format": version, method, then ten
    // 4 x i16 CMYK records — a reserved zero one, then reds, yellows, greens,
    // cyans, blues, magentas, whites, neutrals, blacks.
    assert_eq!(body.len(), 84);
    let word = |at: usize| i16::from_be_bytes([body[at], body[at + 1]]);
    assert_eq!((word(0), word(2)), (1, 1), "version 1, absolute");
    let record =
        |index: usize| -> [i16; 4] { std::array::from_fn(|ink| word(4 + index * 8 + ink * 2)) };
    assert_eq!(record(0), [0; 4], "the reserved record");
    assert_eq!(record(1), [10, -20, 30, -100], "reds");
    for index in 2..9 {
        assert_eq!(record(index), [0; 4], "band {index}");
    }
    assert_eq!(record(9), [0, 0, 0, 5], "blacks");

    let report = fixture::import_report(&path);
    assert!(fixture::strings(&report, "observations")
        .iter()
        .any(|line| line.contains(
            "is a Photoshop Selective Color adjustment layer (selc); layered import carries it"
        )));
    let reopened = fixture::open8(&path, LAYERS).expect("reopen");
    let read: Value =
        serde_json::from_str(reopened.layers[1].meta.as_deref().expect("meta")).unwrap();
    assert_eq!(read["op"], json!("selective_color"));
    assert_eq!(read["params"], params);
    assert_eq!(reopened.flattened().unwrap(), document.flattened().unwrap());

    // The relative method is word 0, and it is the default.
    let relative = adjusted(
        "selective_color",
        &json!({"cyans": {"y": -0.5}}),
        "Selective",
    );
    let (parsed, _, _keep) = written(&relative, PSD);
    let body = parsed.layers[1].block(b"selc").unwrap();
    assert_eq!(&body[..4], &[0, 1, 0, 0]);
    assert_eq!(
        &body[4 + 4 * 8..4 + 5 * 8],
        &[0, 0, 0, 0, 0xff, 0xce, 0, 0],
        "cyans"
    );
}
