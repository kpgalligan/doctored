//! Typed model/native integration. Numerical expectations below are independent
//! source-over and affine RGB formulas; retained float intermediates are tested
//! with changes deliberately smaller than one u16 storage code.
use crate::common;
use crate::group_roots;
use crate::precision_native_fixture as native_fixture;
use common::probe::{cancelled_parked_at, entry_then_cancel, parked_line};
use doctored_core::icc::IccProfile;
use doctored_core::job_control::DrJobControl;
use doctored_core::layer_insert::InsertPosition;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_adjustment::PrecisionAdjustmentState;
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer as Layer};
use doctored_core::precision_geometry::GeometryEdit;
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use doctored_core::precision_render::{require_supported, PrototypeOperation};
use group_roots::GroupRoots;
use serde_json::json;
use std::sync::Arc;
use std::time::Duration;

use crate::identity;

/// Every native encode below goes through here. Phase 12E-1 made the
/// always-written identity wrapper the OUTERMOST `.rz` word and its `LIDS`
/// table the file's last block; this file's subject is an EARLIER wrapper,
/// so the outer one is asserted and then peeled, leaving byte-for-byte what
/// the writer these tests were written against produced.
fn encoded(doc: &PrecisionDocument) -> Vec<u8> {
    identity::typed_prior_body(&doc.encode_native(LIMIT, 0, None).unwrap())
}

const LIMIT: u64 = REFERENCE_WORKING_BYTES;
fn op(name: &str, params: serde_json::Value) -> String {
    json!({"type":"adjust","op":name,"params":params,"evaluation":"precision_v1"}).to_string()
}
fn doc(pixels: Vec<[u16; 4]>) -> PrecisionDocument {
    let image =
        PrecisionImage::from_rgba16(pixels.len() as u32, 1, pixels, IccProfile::srgb(), None)
            .unwrap();
    PrecisionDocument::from_image(image, Metadata::default(), Resolution::default()).unwrap()
}
fn add(d: &PrecisionDocument, description: &str, opacity: f32) -> PrecisionDocument {
    d.add_adjustment("Tone", description, opacity, LIMIT, 0, None)
        .unwrap()
        .unwrap()
}
fn render(d: &PrecisionDocument) -> Vec<[u16; 4]> {
    d.render(LIMIT, 0, None).unwrap().to_rgba16().unwrap()
}

#[test]
fn successive_live_adjustments_keep_sub_u16_changes_until_final_storage() {
    require_supported(PrototypeOperation::Adjustment).unwrap();
    let original = doc(vec![
        [20000, 30000, 40000, 65535],
        [20001, 30001, 40001, 10001],
    ]);
    let description = op("bcs", json!({"brightness":0.30/65535.0}));
    let once = add(&original, &description, 1.0);
    let twice = add(&once, &description, 1.0);
    assert_eq!(
        render(&once),
        original.layer_source(0).unwrap().to_rgba16().ok().unwrap()
    );
    let output = render(&twice);
    for (before, after) in original
        .layer_source(0)
        .unwrap()
        .to_rgba16()
        .ok()
        .unwrap()
        .iter()
        .zip(output)
    {
        for c in 0..3 {
            assert_eq!(after[c], before[c] + 1);
        }
        assert_eq!(after[3], before[3]);
    }
    // A destructive write has an explicit u16 boundary and detects that a
    // single sub-half-code operation does not actually alter storage.
    assert!(original
        .apply_adjustment(0, &description, LIMIT, 0, None)
        .unwrap()
        .is_none());
    assert_eq!(original.layer_count(), 1);
    assert!(PrecisionImage::ptr_eq(
        twice.layer_source(0).unwrap(),
        original.layer_source(0).unwrap()
    ));
}

#[test]
fn group_adjustments_observe_isolation_and_premultiplied_group_opacity() {
    let source = doc(vec![[12001, 23002, 34003, 12345]]);
    let profile = source.profile().clone();
    let adjustment =
        Layer::adjustment("Invert", &op("invert", json!({})), 0.625, true, &profile).unwrap();
    for (pass, expected_changes) in [(false, false), (true, true)] {
        let group = if pass {
            Layer::pass_through("Group", vec![adjustment.clone()], 0.375, true)
        } else {
            Layer::isolated_normal("Group", vec![adjustment.clone()], 0.375, true)
        }
        .unwrap();
        let d = PrecisionDocument::from_parts(
            1,
            1,
            profile.clone(),
            Metadata::default(),
            Resolution::default(),
            vec![source.root_layers()[0].clone(), group],
        )
        .unwrap();
        let output = render(&d)[0];
        for (c, &input) in [12001_u16, 23002, 34003].iter().enumerate() {
            let normalized = f64::from(input) / 65535.0;
            let expected = if expected_changes {
                // Each opacity at its byte (#2).
                normalized
                    + (1.0 - 2.0 * normalized)
                        * crate::common::byte_unit(0.625)
                        * crate::common::byte_unit(0.375)
            } else {
                normalized
            };
            assert!(output[c].abs_diff((expected * 65535.0).round() as u16) <= 1);
        }
        assert_eq!(output[3], 12345);
    }
}

#[test]
fn canonical_noops_identity_and_destructive_alpha_are_explicit() {
    let pixels = vec![
        [10001, 20002, 30003, 0],
        [10002, 20003, 30004, 1],
        [10003, 20004, 30005, 12345],
    ];
    let original = doc(pixels.clone());
    let identity = op("bcs", json!({}));
    let live = add(&original, &identity, 1.0);
    assert_eq!(live.layer_count(), 2); // An intentional editable live record.
    assert!(live
        .set_adjustment(1, &identity, LIMIT, 0, None)
        .unwrap()
        .is_none());
    assert!(original
        .apply_adjustment(0, &identity, LIMIT, 0, None)
        .unwrap()
        .is_none());
    let invert = op("invert", json!({}));
    let changed = original
        .apply_adjustment(0, &invert, LIMIT, 0, None)
        .unwrap()
        .unwrap();
    let actual = changed.layer_source(0).unwrap().to_rgba16().ok().unwrap();
    assert_eq!(actual[0], pixels[0]); // Established destructive hidden-RGB policy.
    for (input, output) in pixels.iter().zip(actual.iter()).skip(1) {
        for c in 0..3 {
            assert_eq!(output[c], 65535 - input[c]);
        }
        assert_eq!(output[3], input[3]);
    }
    assert_eq!(
        original.layer_source(0).unwrap().to_rgba16().ok().unwrap(),
        pixels
    );
    assert!(original.set_adjustment(0, &invert, LIMIT, 0, None).is_err());
    assert!(live.apply_adjustment(1, &invert, LIMIT, 0, None).is_err());
    assert!(live.adjustment_scratch_bytes(None, &invert, true).is_err());
}

#[test]
fn captured_profile_identity_and_admission_precede_preparation() {
    let d = doc(vec![[10001, 20002, 30003, 65535]]);
    let description = op("invert", json!({}));
    let different = Arc::new(IccProfile::parse(d.profile().bytes()).unwrap());
    let adjustment =
        Layer::adjustment("Different capture", &description, 1.0, false, &different).unwrap();
    assert!(PrecisionDocument::from_parts(
        1,
        1,
        d.profile().clone(),
        Metadata::default(),
        Resolution::default(),
        vec![d.root_layers()[0].clone(), adjustment]
    )
    .is_err());
    let scratch = d
        .adjustment_scratch_bytes(None, &description, false)
        .unwrap();
    let token = DrJobControl::default();
    assert!(d
        .add_adjustment(
            "Invert",
            &description,
            1.0,
            d.retained_bytes() + scratch - 1,
            0,
            Some(&token)
        )
        .is_err());
    assert_eq!(token.scratch_bytes(), 0);
    for bad in [
        r#"{"type":"adjust","op":"invert"}"#,
        r#"{"type":"adjust","op":"invented","evaluation":"precision_v1"}"#,
        r#"{"type":"adjust","op":"invert","evaluation":"precision_v99"}"#,
    ] {
        assert!(d.add_adjustment("Bad", bad, 1.0, LIMIT, 0, None).is_err());
    }
    let complete = DrJobControl::default();
    let changed = d
        .add_adjustment("Invert", &description, 1.0, LIMIT, 0, Some(&complete))
        .unwrap()
        .unwrap();
    assert_eq!(complete.progress(), 1.0);
    assert_eq!(complete.scratch_bytes(), 0);
    assert!(d.render(LIMIT, 0, Some(&complete)).is_err());
    let twice = changed.duplicate_layer(1).unwrap().unwrap();
    let prepare =
        PrecisionAdjustmentState::prepare_scratch_bytes(&description, d.profile()).unwrap();
    // Each record is charged its measured retention, not a full preparation
    // allowance: a second copy costs more than nothing and less than that.
    let grown = twice
        .native_plan()
        .unwrap()
        .reopen_retained_bytes
        .checked_sub(changed.native_plan().unwrap().reopen_retained_bytes)
        .unwrap();
    assert!(0 < grown && grown < prepare, "{grown} vs {prepare}");
    // Shared live operators cost one allocation until persisted records reopen.
    // Rebuilding the tiny layer Vec can reduce spare capacity, so compare the
    // upper bound directly rather than subtracting unsigned retained totals.
    assert!(twice.retained_bytes() < changed.retained_bytes() + prepare);
}

#[test]
fn current_native_preserves_nested_operator_state_and_reads_v11() {
    let d = add(
        &doc(vec![[10001, 22222, 33333, 43210]]),
        &op("bcs", json!({"brightness":0.01234567})),
        0.25,
    )
    .group_all_pass_through(0.75)
    .unwrap()
    .unwrap()
    .group_all(0.5)
    .unwrap()
    .unwrap();
    let bytes = encoded(&d);
    assert_eq!(u32::from_le_bytes(bytes[4..8].try_into().unwrap()), 13);
    let reopened = PrecisionDocument::parse_native(&bytes, LIMIT, 0, None).unwrap();
    assert_eq!(
        reopened.adjustment_description(1),
        d.adjustment_description(1)
    );
    assert_eq!(render(&reopened), render(&d));
    assert!(reopened.layer(2).unwrap().is_pass_through());
    assert!(!reopened.layer(3).unwrap().is_pass_through());
    let old = native_fixture::image_file(11, &[[30001, 30002, 30003, 1]], None);
    assert_eq!(
        PrecisionDocument::parse_native(&old, LIMIT, 0, None)
            .unwrap()
            .layer_source(0)
            .unwrap()
            .to_rgba16()
            .ok()
            .unwrap(),
        &[[30001, 30002, 30003, 1]]
    );
    let wrong_version = native_fixture::image_file(
        11,
        &[[30001, 30002, 30003, 1]],
        Some(&op("invert", json!({}))),
    );
    assert!(
        PrecisionDocument::parse_native(&wrong_version, LIMIT, 0, None)
            .err()
            .unwrap()
            .contains("record kind")
    );
}

#[test]
fn native_rejects_unknown_later_operator_before_decoding_any_pixels() {
    let d = add(
        &doc(vec![[30001, 30002, 30003, 65535]]),
        &op("invert", json!({})),
        1.0,
    );
    let mut bytes = encoded(&d);
    let png = bytes
        .windows(8)
        .position(|s| s == b"\x89PNG\r\n\x1a\n")
        .unwrap();
    let length = u64::from_le_bytes(bytes[png - 8..png].try_into().unwrap()) as usize;
    // Valid IHDR, missing IDAT: decoding the first raster would fail. The
    // following unknown operator must be reported first, even when hidden.
    bytes.drain(png + 33..png + length);
    bytes[png - 8..png].copy_from_slice(&33_u64.to_le_bytes());
    let name = bytes.windows(6).position(|s| s == b"invert").unwrap();
    bytes[name..name + 6].copy_from_slice(b"badbad");
    let error = PrecisionDocument::parse_native(&bytes, LIMIT, 0, None)
        .err()
        .unwrap();
    assert!(
        error.contains("adjustment") || error.contains("operator"),
        "{error}"
    );
    assert!(
        !error.contains("PNG"),
        "operator validation must precede pixel decode: {error}"
    );
}

#[test]
fn live_and_destructive_adjustment_cancel_without_publishing() {
    let d = doc(vec![[12345, 23456, 34567, 65535]; 2_000_000]);
    let description = op("curves", json!({"rgb":[[0,0],[64,50],[128,145],[255,255]]}));
    let live = add(&d, &description, 1.0);
    for destructive in [false, true] {
        let original = if destructive { &d } else { &live };
        let description = &description;
        let render = move |token: usize| {
            let control = unsafe { &*(token as *const DrJobControl) };
            let result = if destructive {
                original
                    .apply_adjustment(0, description, LIMIT, 0, Some(control))
                    .map(|_| ())
            } else {
                original.render(LIMIT, 0, Some(control)).map(|_| ())
            };
            match result {
                Ok(()) => (true, String::new()),
                Err(error) => (false, error),
            }
        };
        let at = if destructive { 0.4 } else { 0.6 };
        let (entry, next) = entry_then_cancel(&|progress, _, _| progress >= at, render);
        let n0 = entry.checkpoints;
        let parked = if destructive {
            assert!(
                next.expect("the entry's kernel never reported again")
                    .checkpoints
                    > n0 + 3,
                "the entry's kernel polls fewer than three checkpoints before its next report"
            );
            cancelled_parked_at(
                n0 + 3,
                &|progress, stage| stage == entry.stage && progress == entry.progress,
                render,
            )
        } else {
            // k = 0: the live render polls only at its reports.
            cancelled_parked_at(n0 + 1, &|_, stage| stage == entry.stage, render)
        };
        eprintln!(
            "destructive={destructive} adjustment cancellation: {}",
            parked_line(&parked)
        );
    }
    assert_eq!(d.layer_count(), 1);
}

#[test]
fn spatial_guide_is_shared_by_live_and_destructive_paths_and_budgeted() {
    let pixels: Vec<_> = (0..257)
        .map(|x| {
            let value = if (100..157).contains(&x) { 6001 } else { 50003 };
            [value, value, value, 65535]
        })
        .collect();
    let original = doc(pixels);
    let description = op(
        "shadows_highlights",
        json!({"radius":12.0,"shadows":{"amount":0.65},"highlights":{"amount":0.2}}),
    );
    let guided = add(&original, &description, 1.0);
    let plain = add(&original, &op("invert", json!({})), 1.0);
    let guided_plan = guided.render_plan().unwrap();
    assert!(guided_plan.work_units > plain.render_plan().unwrap().work_units);
    // This small guide fits under the final u16 conversion overlap, but the
    // destructive plan must explicitly include it alongside the output plane.
    let inactive = op(
        "shadows_highlights",
        json!({"radius":12.0,"shadows":{"amount":0.0},"highlights":{"amount":0.0}}),
    );
    assert!(
        original
            .adjustment_scratch_bytes(Some(0), &description, true)
            .unwrap()
            > original
                .adjustment_scratch_bytes(Some(0), &inactive, true)
                .unwrap()
    );
    let applied = original
        .apply_adjustment(0, &description, LIMIT, 0, None)
        .unwrap()
        .unwrap();
    assert_eq!(
        render(&guided),
        applied.layer_source(0).unwrap().to_rgba16().ok().unwrap()
    );
    assert_ne!(render(&guided), render(&original));
    let token = DrJobControl::default();
    assert!(guided
        .render(
            guided_plan.unique_retained_bytes + guided_plan.scratch_bytes - 1,
            0,
            Some(&token)
        )
        .is_err());
    assert_eq!(token.scratch_bytes(), 0);
}

#[test]
fn capture_descriptor_never_copies_large_operator_payloads_or_names() {
    let original = doc(vec![[10001, 20002, 30003, 65535]]);
    // Every four A characters encode three zero bytes. 33^3 RGB float entries
    // are a valid constant-black LUT with a large serialized payload.
    let table = "A".repeat(33_usize.pow(3) * 3 * 4 / 3 * 4);
    let description = op("color_lookup", json!({"kind":"3d","size":33,"table":table}));
    let d = original
        .add_adjustment(&"L".repeat(65536), &description, 1.0, LIMIT, 0, None)
        .unwrap()
        .unwrap();
    let descriptor = d.descriptor().unwrap();
    assert!(descriptor.to_string().len() < 4096);
    for field in ["layers", "profile", "import_warnings"] {
        assert!(descriptor.get(field).is_none());
    }
    assert!(descriptor["native_scratch_bytes"].as_u64().unwrap() > 0);
    let info = d.info().unwrap();
    assert_eq!(info["layers"][1]["adjustment"]["operation"], "color_lookup");
    assert!(
        info["layers"][1]["adjustment"]["description_bytes"]
            .as_u64()
            .unwrap()
            > 500_000
    );
    assert!(info.to_string().len() < 70_000);
}

// ---- measured adjustment retention (writer plan and native reopen) ----

/// The calibrated White Balance encoder table: 3 channels x 65537 binary64
/// entries. (A calibrated Exposure holds no encoder since #2: it needs only
/// one exponent per channel.)
const ENC: u64 = 3 * 65_537 * 8;

fn white_balance() -> String {
    op("white_balance", json!({"temperature":5200}))
}
fn lut33() -> String {
    // Every four A characters encode three zero bytes (the fixture above).
    let table = "A".repeat(33_usize.pow(3) * 3 * 4 / 3 * 4);
    op("color_lookup", json!({"kind":"3d","size":33,"table":table}))
}
fn stack(base: &PrecisionDocument, description: &str, count: usize) -> PrecisionDocument {
    (0..count).fold(base.clone(), |d, _| add(&d, description, 1.0))
}
fn reopen_plan(d: &PrecisionDocument) -> u64 {
    d.native_plan().unwrap().reopen_retained_bytes
}
fn native(d: &PrecisionDocument) -> Vec<u8> {
    d.encode_native(LIMIT, 0, None).unwrap()
}
/// Valid IHDR, IDAT drained: decoding the (only) raster would fail
/// (`native_rejects_unknown_later_operator_before_decoding_any_pixels`).
fn corrupt_raster(mut bytes: Vec<u8>) -> Vec<u8> {
    let png = bytes
        .windows(8)
        .position(|s| s == b"\x89PNG\r\n\x1a\n")
        .unwrap();
    let length = u64::from_le_bytes(bytes[png - 8..png].try_into().unwrap()) as usize;
    bytes.drain(png + 33..png + length);
    bytes[png - 8..png].copy_from_slice(&33_u64.to_le_bytes());
    bytes
}

#[test]
fn a_full_record_stack_of_cheap_adjustments_plans_saves_and_reopens() {
    let cheap = [
        op("bcs", json!({"brightness":0.01})),
        op("invert", json!({})),
        op("levels", json!({"gamma":1.1})),
    ];
    let base = doc(vec![
        [10001, 20002, 30003, 65535],
        [40004, 50005, 60006, 30000],
    ]);
    let d = (0..1023).fold(base, |d, i| add(&d, &cheap[i % 3], 1.0));
    assert_eq!(d.layer_count(), 1024);
    d.native_plan().unwrap();
    let reopened = PrecisionDocument::parse_native(&native(&d), LIMIT, 0, None).unwrap();
    assert_eq!(reopened.layer_count(), 1024);
    assert_eq!(render(&reopened), render(&d));
    let error = d
        .add_adjustment("Tone", &cheap[0], 1.0, LIMIT, 0, None)
        .err()
        .unwrap();
    assert!(
        error.contains("too many high-depth document records"),
        "{error}"
    );
    let crop = GeometryEdit::parse(r#"{"op":"crop","x":0,"y":0,"width":1,"height":1}"#).unwrap();
    assert!(d.geometry(&crop, LIMIT, 0, None).unwrap().is_some());
}

fn encoder_delta_and_heavy_stack(wrap: fn(PrecisionDocument) -> PrecisionDocument) {
    let base = doc(vec![[10001, 20002, 30003, 65535]]);
    for k in 1..4 {
        let delta = reopen_plan(&wrap(stack(&base, &white_balance(), k + 1)))
            .checked_sub(reopen_plan(&wrap(stack(&base, &white_balance(), k))))
            .unwrap();
        assert!(delta >= ENC, "white balance {k}: {delta} < {ENC}");
    }
    let delta = reopen_plan(&wrap(stack(&base, &lut33(), 2)))
        .checked_sub(reopen_plan(&wrap(stack(&base, &lut33(), 1))))
        .unwrap();
    assert!(delta >= 33 * 33 * 33 * 3 * 4, "color lookup: {delta}");
    let heavy = [
        op("exposure", json!({"exposure":1})),
        white_balance(),
        op("curves", json!({"rgb":[[0,0],[64,50],[128,145],[255,255]]})),
        op(
            "gradient_map",
            json!({"dither":false,"gradient":{"stops":[{"position":0,"color":"#000000"},{"position":1,"color":"#ffffff"}]}}),
        ),
        op("selective_color", json!({})),
        op(
            "shadows_highlights",
            json!({"radius":12.0,"shadows":{"amount":0.65},"highlights":{"amount":0.2}}),
        ),
        lut33(),
    ];
    let d = wrap(heavy.iter().fold(base, |d, h| add(&d, h, 1.0)));
    let plan = d.native_plan().unwrap();
    let bytes = native(&d);
    let reopened = PrecisionDocument::parse_native(&bytes, LIMIT, 0, None).unwrap();
    assert!(
        reopened.retained_bytes() <= plan.reopen_retained_bytes,
        "{} > {}",
        reopened.retained_bytes(),
        plan.reopen_retained_bytes
    );
    let exact = plan.reopen_retained_bytes + plan.decode_scratch_bytes;
    PrecisionDocument::parse_native(&bytes, exact, 0, None).unwrap();
}

#[test]
fn plan_charges_each_record_its_measured_operator_tables() {
    encoder_delta_and_heavy_stack(|d| d);
}

#[test]
fn reopened_documents_plan_exactly_what_their_writer_planned() {
    let lut = |values: &[f32]| {
        let bytes: Vec<u8> = values.iter().flat_map(|v| v.to_le_bytes()).collect();
        let digits = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut table = String::new();
        for chunk in bytes.chunks(3) {
            let v = (u32::from(chunk[0]) << 16)
                | (u32::from(*chunk.get(1).unwrap_or(&0)) << 8)
                | u32::from(*chunk.get(2).unwrap_or(&0));
            for digit in 0..4 {
                table.push(if digit > chunk.len() {
                    '='
                } else {
                    digits[((v >> (18 - 6 * digit)) & 63) as usize] as char
                });
            }
        }
        json!({"kind":"1d", "size":2, "table":table})
    };
    // `tests/precision_adjustment_math.rs`'s 21 cases; exposure keeps its
    // default calibrated evaluation.
    let cases = [
        ("bcs", json!({"brightness":0.125})),
        ("levels", json!({"gamma":0.1})),
        ("hue_rotate", json!({"degrees":120})),
        ("threshold", json!({"level":0.4})),
        ("posterize", json!({"levels":5})),
        ("curves", json!({"rgb":[[0,0],[255,127.5]]})),
        ("invert", json!({})),
        ("grayscale", json!({})),
        ("sepia", json!({})),
        ("exposure", json!({"exposure":1})),
        ("vibrance", json!({"vibrance":0.5})),
        ("hue_saturation", json!({"lightness":0.25})),
        ("color_balance", json!({})),
        ("black_and_white", json!({"reds":0.9})),
        (
            "photo_filter",
            json!({"color":"#808080","density":0.5,"preserve_luminosity":false}),
        ),
        (
            "channel_mixer",
            json!({"monochrome":true,"gray":{"r":0.25,"g":0.25,"b":0.25,"constant":0.125}}),
        ),
        ("selective_color", json!({})),
        (
            "shadows_highlights",
            json!({"shadows":{"amount":1,"tone":0.5},"highlights":{"amount":0},"color":0}),
        ),
        ("white_balance", json!({})),
        (
            "gradient_map",
            json!({"dither":false,"gradient":{"stops":[{"position":0,"color":"#000000"},{"position":1,"color":"#ffffff"}]}}),
        ),
        ("color_lookup", lut(&[0.0, 0.125, 0.25, 1.0, 0.875, 0.75])),
    ];
    assert_eq!(cases.len(), 21);
    for (name, params) in cases {
        let d = add(
            &doc(vec![[10001, 20002, 30003, 65535]]),
            &op(name, params),
            1.0,
        );
        let reopened = PrecisionDocument::parse_native(&native(&d), LIMIT, 0, None).unwrap();
        assert_eq!(reopen_plan(&reopened), reopen_plan(&d), "{name}");
    }
}

#[test]
fn reader_admission_refuses_mid_walk_before_any_pixel_decode() {
    let d = stack(
        &doc(vec![[10001, 20002, 30003, 65535]]),
        &white_balance(),
        20,
    );
    let plan = d.native_plan().unwrap();
    let whole = plan.reopen_retained_bytes + plan.decode_scratch_bytes;
    let bytes = corrupt_raster(encoded(&d));
    let error = PrecisionDocument::parse_native(&bytes, LIMIT, 0, None)
        .err()
        .unwrap();
    // The drained raster fails in its decoder's chunk walk. The structural
    // preflight accepted the file: the run below reaches the adjustment walk.
    assert!(error.contains("end of file"), "{error}");
    assert!(!error.contains("precision operation needs"), "{error}");
    // The up-front `base + peak + scratch` fits; a later
    // `base + prepared records + next allowance` does not.
    let other = LIMIT - whole + 20 * ENC / 2;
    let token = DrJobControl::default();
    let error = PrecisionDocument::parse_native(&bytes, LIMIT, other, Some(&token))
        .err()
        .unwrap();
    assert!(error.contains("precision operation needs"), "{error}");
    assert!(!error.contains("end of file"), "{error}");
    assert_eq!(token.scratch_bytes(), 0);
}

/// A canvas-sized empty raster (a memory-cheap filled plane) under a
/// `width x height` canvas, then one `description` duplicated `copies` times
/// or until the native plan refuses.
fn duplicated(
    width: u32,
    height: u32,
    description: &str,
    copies: usize,
) -> Result<PrecisionDocument, (usize, String)> {
    let profile = IccProfile::srgb();
    let one =
        PrecisionImage::from_rgba16(1, 1, vec![[1, 2, 3, 65535]], profile.clone(), None).unwrap();
    let background = Layer::raster("Background", one, (0, 0), 1.0, true).unwrap();
    let d = PrecisionDocument::from_parts(
        width,
        height,
        profile,
        Metadata::default(),
        Resolution::default(),
        vec![background],
    )
    .unwrap()
    .add_layer_at("Plane", 0, InsertPosition::Top)
    .unwrap()
    .unwrap();
    let mut d = add(&d, description, 1.0);
    for _ in 0..copies {
        d = match d.duplicate_layer(2) {
            Ok(next) => next.unwrap(),
            Err(error) => return Err((d.layer_count(), error)),
        };
    }
    Ok(d)
}

#[test]
fn writer_plan_refuses_a_heavy_stack_before_the_record_cap() {
    let (width, height) = (6000, 5000);
    let (records, error) = duplicated(width, height, &white_balance(), 1021)
        .err()
        .unwrap();
    assert!(
        error.contains("At 16 bits the result needs more than the 2 GB working limit"),
        "{error}"
    );
    assert!(records < 1024, "{records}");
    let cheap = duplicated(
        width,
        height,
        &op("bcs", json!({"brightness":0.01})),
        records - 3,
    )
    .unwrap();
    assert_eq!(cheap.layer_count(), records);
}

/// Parse `d` once to find the first stage-6 report, then park a fresh parse
/// at that checkpoint and read the live reservation.
fn reservation_at_first_decode(d: &PrecisionDocument, records: u64) {
    let bytes = Arc::new(native(d));
    let probe = DrJobControl::default();
    probe.record_reports();
    PrecisionDocument::parse_native(&bytes, LIMIT, 0, Some(&probe)).unwrap();
    let at = probe
        .reports()
        .iter()
        .find(|r| r.stage == 6)
        .unwrap()
        .checkpoints;
    let token = Arc::new(DrJobControl::default());
    token.park_at(at);
    let (worker_token, worker_bytes) = (token.clone(), bytes.clone());
    let worker = std::thread::spawn(move || {
        PrecisionDocument::parse_native(&worker_bytes, LIMIT, 0, Some(&worker_token)).map(|_| ())
    });
    let parked = token.wait_parked(Duration::from_secs(10));
    let reserved = token.scratch_bytes();
    token.release();
    worker.join().unwrap().unwrap();
    assert!(parked);
    assert!(
        reserved >= bytes.len() as u64 + records * ENC,
        "{reserved} < {} + {records} x {ENC}",
        bytes.len()
    );
}

#[test]
fn reopen_reservation_covers_every_prepared_record_through_decode() {
    let d = stack(
        &doc(vec![[10001, 20002, 30003, 65535]]),
        &white_balance(),
        20,
    );
    reservation_at_first_decode(&d, 20);
}

#[test]
fn nested_groups_charge_and_reserve_the_same_measured_retention() {
    encoder_delta_and_heavy_stack(|d| d.group_all(0.5).unwrap().unwrap());
    let d = stack(
        &doc(vec![[10001, 20002, 30003, 65535]]),
        &white_balance(),
        20,
    )
    .group_all(0.5)
    .unwrap()
    .unwrap();
    reservation_at_first_decode(&d, 20);
}

mod bench {
    use super::*;
    use std::time::Instant;

    #[test]
    #[ignore = "make cancel-bench: a wall-clock verdict"]
    fn live_and_destructive_adjustment_cancel_without_publishing() {
        let d = doc(vec![[12345, 23456, 34567, 65535]; 2_000_000]);
        let description = op("curves", json!({"rgb":[[0,0],[64,50],[128,145],[255,255]]}));
        let live = add(&d, &description, 1.0);
        for destructive in [false, true] {
            let token = Arc::new(DrJobControl::default());
            let worker_token = token.clone();
            let original = if destructive { d.clone() } else { live.clone() };
            let description = description.clone();
            let worker = std::thread::spawn(move || {
                if destructive {
                    original
                        .apply_adjustment(0, &description, LIMIT, 0, Some(&worker_token))
                        .map(|_| ())
                } else {
                    original.render(LIMIT, 0, Some(&worker_token)).map(|_| ())
                }
            });
            let deadline = Instant::now() + Duration::from_secs(10);
            while token.progress() < if destructive { 0.4 } else { 0.6 }
                && !worker.is_finished()
                && Instant::now() < deadline
            {
                std::thread::yield_now();
            }
            assert!(
                !worker.is_finished(),
                "fixture must reach active operation before completion"
            );
            let start = Instant::now();
            token.cancel();
            assert!(worker.join().unwrap().unwrap_err().contains("cancelled"));
            assert!(start.elapsed() < Duration::from_millis(500));
            assert_eq!(token.scratch_bytes(), 0);
        }
        assert_eq!(d.layer_count(), 1);
    }
}
