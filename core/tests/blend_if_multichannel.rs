//! Blend If on several channels at once at 8 bits, black-box
//! through `dr_doc_set_layer_style` and the projection: the `blend_if.extra`
//! JSON (round trip, canonical form, errors and the mode refusal of a range
//! as written), the weight of several ranges against the oracle below, a
//! single range's render unchanged, the byte `.rz` word 101 (first inside 45,
//! ahead of 99; both refusals; a comp-only claim), the blending ranges a
//! layered PSD writes (slots 0 and 1) and reads back, and the layer
//! clipboard.
//!
//! THE ORACLE (written here from the spec, not the crate's code): each range
//! `[lo0, lo1, hi0, hi1]` weights a channel value `v` by 0 below `lo0`, a
//! linear rise to `lo1`, 1 through `hi0`, a linear fall to `hi1`, 0 above;
//! the value is the channel of the 8-bit pixel (gray `round(0.30 R + 0.59 G
//! + 0.11 B)`); a range's weight is its This Layer ramp on the layer's pixel
//! times its Underlying ramp on the composite beneath, and several ranges
//! MULTIPLY (the take's PRODUCT,
//! `scripts/photoshop-probe/blend_if_multichannel_take.py`).
//!
//! Photoshop has no alpha-channel Blend If: a range naming an alpha channel
//! is written to slot 0 and renders as the Gray range, so take1's n1 and n2
//! PSDs import as Gray ranges and render as Photoshop's own TIFFs
//! (`blend_if_alpha_take.py`).

use std::path::PathBuf;

use doctored_core::doc::DrDocument;
use doctored_core::ffi_style::dr_doc_layer_style;
use doctored_core::style::LayerStyle;
use image::{Rgba, RgbaImage};
use serde_json::{json, Value};
use tempfile::TempDir;

use crate::common::{
    assert_close, ref_composite, solid, styled, take_err_string, to_unit, try_styled, BLEND_NORMAL,
};

// ---------------------------------------------------------------- oracle --

fn ramp(v: f64, r: [i64; 4]) -> f64 {
    let [lo0, lo1, hi0, hi1] = r.map(|k| k as f64);
    if v < lo0 {
        0.0
    } else if v < lo1 {
        (v - lo0) / (lo1 - lo0)
    } else if v <= hi0 {
        1.0
    } else if v <= hi1 {
        (hi1 - v) / (hi1 - hi0)
    } else {
        0.0
    }
}

fn value(channel: &str, px: [u8; 4]) -> f64 {
    let [r, g, b] = [px[0], px[1], px[2]].map(f64::from);
    match channel {
        "gray" => (0.30 * r + 0.59 * g + 0.11 * b).round(),
        "red" => r,
        "green" => g,
        "blue" => b,
        other => panic!("channel {other}"),
    }
}

/// One range: `(channel, this, underlying)`.
type Range = (&'static str, [i64; 4], [i64; 4]);

/// The PRODUCT of every range's two-ramp weight.
fn weight(ranges: &[Range], src: [u8; 4], under: [u8; 4]) -> f64 {
    ranges
        .iter()
        .map(|(c, this, below)| ramp(value(c, src), *this) * ramp(value(c, under), *below))
        .product()
}

fn range_json((channel, this, under): Range) -> Value {
    json!({"channel": channel, "this_layer": this, "underlying": under})
}

/// `{"blend_if": first + extra}`.
fn style_of(ranges: &[Range]) -> String {
    let mut b = range_json(ranges[0]);
    if ranges.len() > 1 {
        b["extra"] = Value::Array(ranges[1..].iter().map(|r| range_json(*r)).collect());
    }
    json!({ "blend_if": b }).to_string()
}

const FULL: [i64; 4] = [0, 0, 255, 255];
const BLACK: [u8; 4] = [0, 0, 0, 255];

// ---------------------------------------------------------------- scenes --

const W: u32 = 64;
const H: u32 = 16;

/// R the x ramp (0..252 by 4), G = B the y ramp (0..240 by 16): gray and
/// red vary independently, so a product and a min differ.
fn source(x: u32, y: u32) -> [u8; 4] {
    [(x * 4) as u8, (y * 16) as u8, (y * 16) as u8, 255]
}

fn over(bg: RgbaImage, layer: RgbaImage) -> DrDocument {
    DrDocument::from_pixels(bg)
        .adding_image_layer(0, layer, "Top")
        .expect("add layer")
}

fn source_doc() -> DrDocument {
    over(
        solid(W, H, BLACK),
        RgbaImage::from_fn(W, H, |x, y| Rgba(source(x, y))),
    )
}

/// The flattened render of `doc` against `want(x, y)`, ±1 per colour.
fn assert_render(doc: &DrDocument, mut want: impl FnMut(u32, u32) -> [u8; 4], what: &str) {
    let flat = doc.flattened().unwrap();
    let mut expected = Vec::with_capacity((W * H * 4) as usize);
    for y in 0..H {
        for x in 0..W {
            expected.extend(want(x, y));
        }
    }
    assert_close(flat.as_raw(), &expected, what);
}

fn composite(bg: [u8; 4], src: [u8; 4], w: f64) -> [u8; 4] {
    ref_composite(to_unit(bg), to_unit(src), w as f32, BLEND_NORMAL)
        .map(|v| (v * 255.0).round() as u8)
}

fn layer_style(doc: &DrDocument, idx: usize) -> Value {
    let raw = unsafe { dr_doc_layer_style(doc, idx) };
    assert!(!raw.is_null(), "layer {idx} has a style");
    serde_json::from_str(&take_err_string(raw)).unwrap()
}

// ------------------------------------------------------------------ JSON --

#[test]
fn extra_ranges_parse_canonicalize_round_trip_and_refuse_by_name() {
    let doc = source_doc();
    // Written out of order with an identity range: the canonical form drops
    // the identity, sorts the rest (gray before red) and fills `extra`.
    let written = json!({"blend_if": {"channel": "red", "this_layer": [0, 0, 64, 224],
        "extra": [{"channel": "gray", "this_layer": [0, 0, 96, 192]}, {"channel": "blue"}]}});
    let set = styled(&doc, 1, &written.to_string());
    let canonical = json!({"channel": "gray", "this_layer": [0, 0, 96, 192],
        "underlying": [0, 0, 255, 255], "extra": [{"channel": "red",
        "this_layer": [0, 0, 64, 224], "underlying": [0, 0, 255, 255]}]});
    let echoed = layer_style(&set, 1);
    assert_eq!(echoed["blend_if"], canonical);
    // The echo is the stored value: setting it again changes nothing.
    assert!(try_styled(&set, 1, Some(&echoed.to_string()))
        .unwrap()
        .is_none());
    // A single range writes no `extra` key, and an identity extra beside it
    // is dropped to exactly that.
    let single = styled(
        &doc,
        1,
        &json!({"blend_if": {"channel": "green", "underlying": [10, 20, 30, 40],
            "extra": [{"channel": "red"}]}})
        .to_string(),
    );
    let single = layer_style(&single, 1);
    assert_eq!(
        single["blend_if"],
        json!({"channel": "green", "this_layer": [0, 0, 255, 255],
            "underlying": [10, 20, 30, 40]})
    );
    // Every range identity is no style at all.
    assert!(try_styled(
        &doc,
        1,
        Some(&json!({"blend_if": {"channel": "red", "extra": [{"channel": "gray"}]}}).to_string())
    )
    .unwrap()
    .is_none());
    for (blend_if, message) in [
        (
            json!({"channel": "red", "this_layer": [0, 0, 64, 224],
                "extra": [{"channel": "red", "this_layer": [0, 0, 9, 9]}]}),
            "style: blend_if names the Red channel twice",
        ),
        (
            json!({"extra": [{"channel": "gray", "this_layer": [0, 0, 9, 9]}]}),
            "style: blend_if names the Gray channel twice",
        ),
        (
            json!({"channel": "red", "extra": {"channel": "gray"}}),
            "style: blend_if.extra must be an array",
        ),
        (
            json!({"channel": "red", "extra": [7]}),
            "style: blend_if.extra[0] must be an object",
        ),
        (
            json!({"channel": "red", "extra": [{"channel": "gray",
                "this_layer": [9, 0, 255, 255]}]}),
            "style: blend_if.extra[0].this_layer must be in non-decreasing order",
        ),
        (
            json!({"channel": "red", "extra": [{"channel": "alpha"}]}),
            "style: blend_if.extra[0].channel must be one of gray, red, green, blue, cyan, magenta, yellow, black",
        ),
        // An 8-bit document has no plates: a CMYK range refuses by name even
        // as an identity ramp in `extra` (canonicalization would drop it).
        (
            json!({"channel": "gray", "this_layer": [0, 0, 96, 192],
                "extra": [{"channel": "cyan"}]}),
            "Blend If on the Cyan channel needs a CMYK document",
        ),
        (
            json!({"channel": "gray", "this_layer": [0, 0, 96, 192],
                "extra": [{"channel": "black", "this_layer": [0, 0, 9, 9]}]}),
            "Blend If on the Black channel needs a CMYK document",
        ),
    ] {
        let Err(got) = try_styled(&doc, 1, Some(&json!({ "blend_if": blend_if }).to_string()))
        else {
            panic!("{blend_if} must refuse");
        };
        assert_eq!(got, message, "{blend_if}");
    }
}

// --------------------------------------------------------------- weights --

#[test]
fn several_this_layer_ranges_multiply_against_the_oracle() {
    let doc = source_doc();
    for ranges in [
        vec![
            ("gray", [0, 0, 96, 192], FULL),
            ("red", [0, 0, 64, 224], FULL),
        ],
        vec![
            ("gray", [16, 48, 160, 230], FULL),
            ("red", [30, 90, 180, 250], FULL),
            ("green", [0, 64, 255, 255], FULL),
        ],
        vec![
            ("blue", [0, 0, 100, 200], FULL),
            ("red", [40, 120, 255, 255], FULL),
        ],
    ] {
        let set = styled(&doc, 1, &style_of(&ranges));
        let mut product_differs_from_min = false;
        assert_render(
            &set,
            |x, y| {
                let src = source(x, y);
                let ws: Vec<f64> = ranges.iter().map(|r| weight(&[*r], src, BLACK)).collect();
                let w = weight(&ranges, src, BLACK);
                let min = ws.iter().copied().fold(1.0, f64::min);
                product_differs_from_min |= (w - min).abs() * 255.0 > 4.0;
                composite(BLACK, src, w)
            },
            &style_of(&ranges),
        );
        assert!(
            product_differs_from_min,
            "the scene separates PRODUCT from MIN"
        );
    }
}

#[test]
fn underlying_ranges_multiply_on_the_composite_beneath() {
    // The source is one solid colour over the ramp backdrop.
    let src = [230, 40, 120, 255];
    let doc = over(
        RgbaImage::from_fn(W, H, |x, y| Rgba(source(x, y))),
        solid(W, H, src),
    );
    let ranges = [
        ("gray", FULL, [20, 60, 255, 255]),
        ("red", FULL, [0, 0, 120, 220]),
    ];
    let set = styled(&doc, 1, &style_of(&ranges));
    assert_render(
        &set,
        |x, y| composite(source(x, y), src, weight(&ranges, src, source(x, y))),
        "underlying gray and red",
    );
}

#[test]
fn a_single_range_renders_exactly_as_before() {
    let doc = source_doc();
    let gray = [("gray", [0, 0, 96, 192], FULL)];
    let alone = styled(&doc, 1, &style_of(&gray));
    let with_identity = styled(
        &doc,
        1,
        &style_of(&[gray[0], ("red", FULL, FULL), ("blue", FULL, FULL)]),
    );
    assert_eq!(
        alone.flattened().unwrap(),
        with_identity.flattened().unwrap()
    );
    assert_eq!(layer_style(&alone, 1), layer_style(&with_identity, 1));
    assert_eq!(
        layer_style(&alone, 1)["blend_if"].get("extra"),
        None,
        "no extra key"
    );
    assert_render(
        &alone,
        |x, y| composite(BLACK, source(x, y), weight(&gray, source(x, y), BLACK)),
        "gray alone",
    );
}

// ---------------------------------------------------------------- .rz 101 --

const GRAY_RED: [Range; 2] = [
    ("gray", [0, 0, 96, 192], FULL),
    ("red", [0, 0, 64, 224], FULL),
];

#[test]
fn the_byte_word_101_comes_first_round_trips_and_refuses_both_ways() {
    let dir = TempDir::new().unwrap();
    let save = |name: &str, doc: &DrDocument| -> (PathBuf, Vec<u8>) {
        let path = dir.path().join(name);
        doc.save_native(path.to_str().unwrap()).unwrap();
        let bytes = std::fs::read(&path).unwrap();
        (path, bytes)
    };
    let (_, plain) = save(
        "single.rz",
        &styled(&source_doc(), 1, &style_of(&GRAY_RED[..1])),
    );
    assert_eq!(&plain[4..8], &45u32.to_le_bytes());
    assert_ne!(&plain[8..12], &101u32.to_le_bytes(), "no 101 for one range");
    let doc = styled(&source_doc(), 1, &style_of(&GRAY_RED));
    let (path, bytes) = save("multi.rz", &doc);
    assert_eq!(&bytes[4..8], &45u32.to_le_bytes());
    assert_eq!(&bytes[8..12], &101u32.to_le_bytes(), "FIRST inside 45");
    let reopened = DrDocument::open(path.to_str().unwrap()).unwrap();
    assert_eq!(layer_style(&reopened, 1), layer_style(&doc, 1));
    assert_eq!(reopened.flattened().unwrap(), doc.flattened().unwrap());
    // Stripped: the extra ranges need the word.
    let mut stripped = bytes.clone();
    stripped.drain(8..12);
    let cut = dir.path().join("cut.rz");
    std::fs::write(&cut, &stripped).unwrap();
    let Err(message) = DrDocument::open(cut.to_str().unwrap()) else {
        panic!("a multichannel Blend If without the word must refuse");
    };
    assert!(
        message.ends_with("native multichannel blend if requires DRDC 101"),
        "{message}"
    );
    // Forged onto a single-range file: the word claims what is not there.
    let mut forged = plain.clone();
    forged.splice(8..8, 101u32.to_le_bytes());
    let forged_path = dir.path().join("forged.rz");
    std::fs::write(&forged_path, &forged).unwrap();
    let Err(message) = DrDocument::open(forged_path.to_str().unwrap()) else {
        panic!("the word without a multichannel Blend If must refuse");
    };
    assert!(
        message.ends_with("invalid native multichannel blend if word"),
        "{message}"
    );
    // With a gradient glow too, 101 comes before 99.
    let mut both: Value = serde_json::from_str(&style_of(&GRAY_RED)).unwrap();
    both["effects"] = json!([{"type":"outer_glow","fill_type":"gradient","size":4}]);
    let (_, bytes) = save("both.rz", &styled(&source_doc(), 1, &both.to_string()));
    assert_eq!(&bytes[8..12], &101u32.to_le_bytes());
    assert_eq!(&bytes[12..16], &99u32.to_le_bytes(), "101 ahead of 99");
}

#[test]
fn a_comp_only_multichannel_blend_if_claims_the_word() {
    use doctored_core::doc_comps::{CompCaptures, CompRequest};
    let dir = TempDir::new().unwrap();
    let doc = styled(&source_doc(), 1, &style_of(&GRAY_RED));
    let save = CompRequest::Save {
        name: "Ranges".into(),
        comment: String::new(),
        captures: CompCaptures::default(),
    };
    let saved = doc.edit_layer_comps(&save).unwrap().unwrap();
    let cleared = try_styled(&saved, 1, None).unwrap().unwrap();
    assert!(cleared.layers[1].style.is_none());
    let path = dir.path().join("comp.rz");
    cleared.save_native(path.to_str().unwrap()).unwrap();
    let bytes = std::fs::read(&path).unwrap();
    assert_eq!(&bytes[8..12], &101u32.to_le_bytes());
    assert!(DrDocument::open(path.to_str().unwrap()).is_ok());
}

// ------------------------------------------------------------------- PSD --

#[test]
fn a_layered_psd_writes_each_range_in_its_slot_and_reads_both_back() {
    use doctored_core::ffi_psd::{
        dr_doc_open_psd, dr_doc_psd_export_report, dr_doc_save_psd, dr_psd_import_report,
    };
    let dir = TempDir::new().unwrap();
    let ranges = [
        ("gray", [0, 0, 96, 192], [5, 15, 235, 250]),
        ("red", [0, 0, 64, 224], FULL),
    ];
    let doc = styled(&source_doc(), 1, &style_of(&ranges));
    let mut err = std::ptr::null_mut();
    let report = unsafe { dr_doc_psd_export_report(&doc, 0, 0, &mut err) };
    assert!(!report.is_null(), "{}", take_err_string(err));
    let report = take_err_string(report);
    assert!(report.contains("Blend If on gray and red"), "{report}");
    let path = dir.path().join("ranges.psd");
    let c = crate::common::cpath(&path);
    assert!(
        unsafe { dr_doc_save_psd(&doc, std::ptr::null(), c.as_ptr(), 0, 0, &mut err) },
        "{}",
        take_err_string(err)
    );
    let bytes = std::fs::read(&path).unwrap();
    let mut section = 40u32.to_be_bytes().to_vec();
    section.extend([0, 0, 96, 192, 5, 15, 235, 250]);
    section.extend([0, 0, 64, 224, 0, 0, 255, 255]);
    for _ in 0..3 {
        section.extend([0, 0, 255, 255, 0, 0, 255, 255]);
    }
    assert_eq!(
        bytes
            .windows(section.len())
            .filter(|w| *w == section.as_slice())
            .count(),
        1,
        "slot 0 gray, slot 1 red, the rest default"
    );
    let import = unsafe { dr_psd_import_report(c.as_ptr(), &mut err) };
    assert!(!import.is_null(), "{}", take_err_string(err));
    let import: Value = serde_json::from_str(&take_err_string(import)).unwrap();
    assert!(
        !import["verified_losses"]
            .as_array()
            .unwrap()
            .iter()
            .any(|l| l.as_str().unwrap().contains("Blend If")),
        "{import}"
    );
    assert!(
        import["observations"].as_array().unwrap().iter().any(|o| o
            .as_str()
            .unwrap()
            .contains("Blend If ranges on gray and red")),
        "{import}"
    );
    let token = std::ffi::CString::new(import["source_fingerprint"].as_str().unwrap()).unwrap();
    let opened = unsafe { dr_doc_open_psd(c.as_ptr(), 0, token.as_ptr(), &mut err) };
    assert!(!opened.is_null(), "{}", take_err_string(err));
    let opened = unsafe { Box::from_raw(opened) };
    let top = opened.layers.len() - 1;
    assert_eq!(
        layer_style(&opened, top)["blend_if"],
        layer_style(&doc, 1)["blend_if"]
    );
}

// ------------------------------------------------------------- clipboard --

#[test]
fn the_layer_clipboard_carries_every_range() {
    use doctored_core::ffi_layer_clipboard::dr_doc_paste_layers;
    let source = styled(&source_doc(), 1, &style_of(&GRAY_RED));
    let target = DrDocument::from_pixels(solid(W, H, BLACK));
    let mut err = std::ptr::null_mut();
    let out = unsafe { dr_doc_paste_layers(&target, &source, 0, 1, 0, 0, false, &mut err) };
    assert!(!out.is_null(), "{}", take_err_string(err));
    let pasted = unsafe { Box::from_raw(out) };
    let top = pasted.layers.len() - 1;
    assert_eq!(
        pasted.layers[top].style.as_deref().map(LayerStyle::to_json),
        source.layers[1].style.as_deref().map(LayerStyle::to_json)
    );
    assert!(layer_style(&pasted, top)["blend_if"]["extra"].is_array());
}

// ------------------------------------------------------------- Photoshop --

fn fixture_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/psd_photoshop/blend_if_multichannel")
}

fn rows() -> Value {
    let text = std::fs::read_to_string(fixture_dir().join("rows.json"))
        .expect("tests/fixtures/psd_photoshop/blend_if_multichannel/rows.json is committed");
    serde_json::from_str(&text).unwrap()
}

fn cells(group: &Value, row: &str) -> Vec<Vec<f64>> {
    group[row]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| {
            c.as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_f64().unwrap())
                .collect()
        })
        .collect()
}

/// The take's COMBINE judge restated (`blend_if_multichannel_take.py`, module
/// doc): every gate read off Photoshop's own single-range rows, the winner
/// predicting row 3 within the declared bound on every judged cell.
#[test]
fn photoshops_rows_meet_the_takes_winner_within_its_bound() {
    let fixture = rows();
    let winner = fixture["winner"].as_str().expect("a winner");
    let min_value = fixture["judged_min"].as_f64().unwrap();
    let combine = |a: f64, b: f64| if winner == "MIN" { a.min(b) } else { a * b };
    let mut judged = 0;
    for (name, group) in fixture["groups"].as_object().unwrap() {
        let bound = fixture["bound"][if name.ends_with("16") { "16" } else { "8" }]
            .as_f64()
            .unwrap();
        let kind = &name[..1];
        let [zero, one, two, three] = [0, 1, 2, 3].map(|i| cells(group, &format!("{kind}{i}")));
        for i in 0..zero.len() {
            let base = &zero[i];
            let want: Vec<f64> = if kind == "u" {
                if base.iter().cloned().fold(0.0, f64::max) < min_value {
                    continue;
                }
                let k = (0..3)
                    .max_by(|&a, &b| (128.0 - base[a]).abs().total_cmp(&(128.0 - base[b]).abs()))
                    .unwrap();
                let span = 128.0 - base[k];
                if span == 0.0 {
                    continue;
                }
                let w = combine((one[i][k] - base[k]) / span, (two[i][k] - base[k]) / span);
                (0..3).map(|c| base[c] + w * (128.0 - base[c])).collect()
            } else {
                let channels: &[usize] = if kind == "p" { &[0, 3] } else { &[0, 1, 2] };
                let k = *channels
                    .iter()
                    .max_by(|&&a, &&b| base[a].total_cmp(&base[b]))
                    .unwrap();
                if base[k] < min_value {
                    continue;
                }
                let w = combine(one[i][k] / base[k], two[i][k] / base[k]);
                base.iter().map(|v| v * w).collect()
            };
            judged += 1;
            for (c, w) in want.iter().enumerate() {
                assert!(
                    (w - three[i][c]).abs() <= bound,
                    "{name} cell {i} channel {c}: {winner} predicts {w}, Photoshop read {}",
                    three[i][c]
                );
            }
        }
    }
    assert!(judged > 0, "judged cells");
}

#[test]
fn photoshops_own_c3_psd_imports_with_both_ranges() {
    use doctored_core::ffi_psd::{dr_doc_open_psd, dr_psd_import_report};
    let path = fixture_dir().join("c3_rgb8.psd");
    let c = crate::common::cpath(&path);
    let mut err = std::ptr::null_mut();
    let report = unsafe { dr_psd_import_report(c.as_ptr(), &mut err) };
    assert!(!report.is_null(), "{}", take_err_string(err));
    let report: Value = serde_json::from_str(&take_err_string(report)).unwrap();
    assert!(
        !report["verified_losses"]
            .as_array()
            .unwrap()
            .iter()
            .any(|l| l.as_str().unwrap().contains("Blend If")),
        "{report}"
    );
    let token = std::ffi::CString::new(report["source_fingerprint"].as_str().unwrap()).unwrap();
    let opened = unsafe { dr_doc_open_psd(c.as_ptr(), 0, token.as_ptr(), &mut err) };
    assert!(!opened.is_null(), "{}", take_err_string(err));
    let opened = unsafe { Box::from_raw(opened) };
    let styled = (0..opened.layers.len())
        .find(|&i| opened.layers[i].style.is_some())
        .expect("the source layer carries the ranges");
    assert_eq!(
        layer_style(&opened, styled)["blend_if"],
        json!({"channel": "gray", "this_layer": [0, 0, 96, 192], "underlying": FULL,
            "extra": [{"channel": "red", "this_layer": [0, 0, 64, 224], "underlying": FULL}]})
    );
}

/// take1's N source (`blend_if_multichannel_take.py:254-264`): R the x ramp
/// and G = B the y ramp, each stepped every 4 px by an integer divide first,
/// then a float divide and round (no tie exists, so `round` equals Python's).
fn n_source(x: u32, y: u32) -> [f64; 3] {
    let r = (f64::from(x / 4) * 255.0 / 23.0).round();
    let g = (f64::from(y / 4) * 255.0 / 15.0).round();
    [r, g, g]
}

/// THE BOUND (declared before measuring): take1's `jobs/record_n8/n{1,2}_rgb8
/// .psd`, whose one range names `Alpha 1` (n1 This [0, 0, 128, 128], n2
/// Underlying [128, 128, 255, 255]), import with no Blend If loss as the
/// Gray range and the layered import's 8-bit render is within 1 code per
/// colour channel of Photoshop's own `n{1,2}_rgb8.tif`: on every pixel for
/// n2, and for n1 on every pixel whose source gray `0.30 R + 0.59 G + 0.11 B`
/// is more than 1 from the 128 knee. The take folder is outside the repo;
/// the test skips when it is absent. A miss is recorded, never widened.
#[test]
fn photoshops_alpha_named_ranges_import_and_render_as_gray() {
    use crate::common::{free, open_ok, pixels, take1_558};
    use doctored_core::ffi_psd::{dr_doc_open_psd, dr_psd_import_report};
    const N_W: u32 = 96;
    const N_H: u32 = 64;
    if take1_558("record_n8", "n1_rgb8.psd").is_none() {
        eprintln!("skipping: blend-if-multichannel-558/take1 is absent");
        return;
    }
    let source = n_source(41, 33);
    assert_eq!(source, [111.0, 136.0, 136.0], "the knee check point");
    for (n, want_range) in [
        (
            1,
            json!({"channel": "gray", "this_layer": [0, 0, 128, 128], "underlying": FULL}),
        ),
        (
            2,
            json!({"channel": "gray", "this_layer": FULL, "underlying": [128, 128, 255, 255]}),
        ),
    ] {
        let psd = take1_558("record_n8", &format!("n{n}_rgb8.psd")).expect("the psd");
        let tif = take1_558("record_n8", &format!("n{n}_rgb8.tif")).expect("the tif");
        let c = crate::common::cpath(&psd);
        let mut err = std::ptr::null_mut();
        let report = unsafe { dr_psd_import_report(c.as_ptr(), &mut err) };
        assert!(!report.is_null(), "n{n}: {}", take_err_string(err));
        let report: Value = serde_json::from_str(&take_err_string(report)).unwrap();
        assert!(
            !report["verified_losses"]
                .as_array()
                .unwrap()
                .iter()
                .any(|l| l.as_str().unwrap().contains("Blend If")),
            "n{n}: {report}"
        );
        let token = std::ffi::CString::new(report["source_fingerprint"].as_str().unwrap()).unwrap();
        let opened = unsafe { dr_doc_open_psd(c.as_ptr(), 0, token.as_ptr(), &mut err) };
        assert!(!opened.is_null(), "n{n}: {}", take_err_string(err));
        let opened = unsafe { Box::from_raw(opened) };
        let styled = (0..opened.layers.len())
            .find(|&i| opened.layers[i].style.is_some())
            .expect("the source layer carries the range");
        assert_eq!(layer_style(&opened, styled)["blend_if"], want_range, "n{n}");
        let image = open_ok(&tif);
        let want = pixels(image);
        free(image);
        let got = opened.flattened().unwrap();
        assert_eq!((got.width(), got.height()), (N_W, N_H), "n{n}");
        assert_eq!(want.len(), (N_W * N_H * 4) as usize, "n{n}");
        let got = got.as_raw();
        let (mut worst, mut judged, mut skipped) = (0, 0, 0);
        for y in 0..N_H {
            for x in 0..N_W {
                let [r, g, b] = n_source(x, y);
                if n == 1 && (0.30 * r + 0.59 * g + 0.11 * b - 128.0).abs() <= 1.0 {
                    skipped += 1;
                    continue;
                }
                judged += 1;
                let i = ((y * N_W + x) * 4) as usize;
                for c in 0..3 {
                    let e = (i32::from(got[i + c]) - i32::from(want[i + c])).abs();
                    worst = worst.max(e);
                }
            }
        }
        eprintln!("n{n}: worst {worst} codes over {judged} pixels ({skipped} on the knee)");
        assert!(judged > 0, "n{n}: judged pixels");
        assert!(worst <= 1, "n{n}: {worst} codes");
    }
}
