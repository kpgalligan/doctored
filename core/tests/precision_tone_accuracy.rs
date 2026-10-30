//! Nonlinear precision qualification with an independently derived cubic
//! Bezier reference. No production spline builder/evaluator enters the oracle.
use doctored_core::adjust_precision::PrecisionAdjustment;
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer as Layer};
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use serde_json::json;

const FLOAT_BOUND: f64 = 2e-6;
const STORED_BOUND: f64 = FLOAT_BOUND + 0.5 / 65535.0;

fn description() -> String {
    // Source parameters keep the established0..255 authoring domain. The red
    // curve is severe shadow compression; the master is its brightening mirror.
    json!({"type":"adjust","evaluation":"precision_v1","op":"curves","params":{
        "r":[[0,0],[127.5,15.9375],[255,255]],
        "g":[[0,0],[127.5,239.0625],[255,255]],
        "rgb":[[0,0],[127.5,239.0625],[255,255]]
    }})
    .to_string()
}

/// De Casteljau evaluation in normalized coordinates, deliberately a different
/// polynomial representation from the engine's second-derivative evaluation.
fn bezier(control: [f64; 4], t: f64) -> f64 {
    let mix = |a: f64, b: f64| (1.0 - t) * a + t * b;
    let a = mix(control[0], control[1]);
    let b = mix(control[1], control[2]);
    let c = mix(control[2], control[3]);
    mix(mix(a, b), mix(b, c))
}
fn shadow(x: f64) -> f64 {
    // The natural cubic spline (S'' = 0 at both ends) through the
    // knots (0,0), (1/2,1/16), (1,1), worked in normalized coordinates (a
    // natural spline is unchanged by scaling both axes alike). Widths
    // h = 1/2, secants d0 = 1/8 and d1 = 15/8. With M0 = M2 = 0 the one
    // interior equation h*M0 + 2(h+h)*M1 + h*M2 = 6(d1 - d0) reads
    // 2*M1 = 21/2, so M1 = 21/4. The slopes S' = d -/+ h(2M_near + M_far)/6:
    // at 0, 1/8 - (1/2)(21/4)/6 = -5/16; at 1/2, 1/8 + (1/2)(21/2)/6 = 1;
    // at 1, 15/8 + (1/2)(21/4)/6 = 37/16. Cubic Bezier handles are
    // endpoint +/- interval_width*slope/3 = slope/6. The spline dips below
    // 0 near the origin, and the 16-bit curve clamps it to [0, 1].
    let y = if x <= 0.5 {
        bezier(
            [0.0, -5.0 / 96.0, 1.0 / 16.0 - 1.0 / 6.0, 1.0 / 16.0],
            2.0 * x,
        )
    } else {
        bezier(
            [1.0 / 16.0, 1.0 / 16.0 + 1.0 / 6.0, 1.0 - 37.0 / 96.0, 1.0],
            2.0 * x - 1.0,
        )
    };
    y.clamp(0.0, 1.0)
}
fn highlight(x: f64) -> f64 {
    1.0 - shadow(1.0 - x)
}
fn expected(rgb: [f64; 3]) -> [f64; 3] {
    [
        highlight(shadow(rgb[0])),
        highlight(highlight(rgb[1])),
        highlight(rgb[2]),
    ]
}

#[test]
fn nonlinear_channel_then_master_curve_matches_bezier_on_every_u16_code() {
    let operation = PrecisionAdjustment::parse(&description(), &IccProfile::srgb()).unwrap();
    assert!(!operation.is_identity());
    let mut maximum = 0.0_f64;
    let mut order_difference = 0.0_f64;
    for code in 0..=u16::MAX {
        let x = f64::from(code) / 65535.0;
        let input = [x, 1.0 - x, x];
        let output = operation.apply_at(input.map(|v| v as f32), 0.0, (i64::from(code), 0));
        for (actual, reference) in output.into_iter().zip(expected(input)) {
            maximum = maximum.max((f64::from(actual) - reference).abs());
        }
        order_difference =
            order_difference.max((highlight(shadow(x)) - shadow(highlight(x))).abs());
    }
    assert!(
        order_difference > 0.5,
        "fixture must distinguish channel/master order"
    );
    assert!(maximum <= FLOAT_BOUND, "nonlinear curve error {maximum}");
    let left = operation.apply_at([32768.0 / 65535.0; 3], 0.0, (0, 0))[0];
    let right = operation.apply_at([32769.0 / 65535.0; 3], 0.0, (0, 0))[0];
    assert!(
        right > left,
        "genuine adjacent u16 values must survive this part of the curve"
    );
    eprintln!("nonlinear channel/master Bezier maximum float error {maximum:.10e}");
}

#[test]
fn nonlinear_curve_survives_tiny_alpha_nested_groups_and_native_reopen() {
    let width = 257;
    let profile = IccProfile::srgb();
    let pixels: Vec<_> = (0..width)
        .map(|i| {
            let r = 10001 + (i * 173 % 40000) as u16;
            [
                r,
                65535 - r,
                20003 + (i * 89 % 25000) as u16,
                [0, 1, 17, 10001, 65535][i as usize % 5],
            ]
        })
        .collect();
    let image =
        PrecisionImage::from_rgba16(width, 1, pixels.clone(), profile.clone(), None).unwrap();
    let source = Layer::raster("Source", image, (0, 0), 1.0, true).unwrap();
    let curve = Layer::adjustment("Nonlinear", &description(), 1.0, true, &profile).unwrap();
    let pass = Layer::pass_through("Backdrop access", vec![curve], 1.0, true).unwrap();
    let group = Layer::isolated_normal("Float isolation", vec![source, pass], 0.375, true).unwrap();
    let document = PrecisionDocument::from_parts(
        width,
        1,
        profile,
        Metadata::default(),
        Resolution::default(),
        vec![group],
    )
    .unwrap();
    let output = document.render(REFERENCE_WORKING_BYTES, 0, None).unwrap();
    let output = output.to_rgba16().ok().unwrap();
    let mut maximum = 0.0_f64;
    for (source, actual) in pixels.iter().zip(output.iter()) {
        // The group's 37.5 % at its byte (#2).
        let q = crate::common::byte_unit(0.375);
        assert_eq!(actual[3], (f64::from(source[3]) * q).round() as u16);
        if source[3] == 0 {
            assert_eq!(*actual, [0; 4]);
            continue;
        }
        let reference = expected(std::array::from_fn(|c| f64::from(source[c]) / 65535.0));
        for c in 0..3 {
            maximum = maximum.max((f64::from(actual[c]) / 65535.0 - reference[c]).abs());
        }
    }
    assert!(
        maximum <= STORED_BOUND,
        "stored nonlinear curve error {maximum}"
    );
    assert_eq!(
        document.layer_source(0).unwrap().to_rgba16().ok().unwrap(),
        pixels
    );
    let bytes = document
        .encode_native(REFERENCE_WORKING_BYTES, 0, None)
        .unwrap();
    let restored =
        PrecisionDocument::parse_native(&bytes, REFERENCE_WORKING_BYTES, 0, None).unwrap();
    assert_eq!(
        restored
            .render(REFERENCE_WORKING_BYTES, 0, None)
            .unwrap()
            .to_rgba16()
            .ok()
            .unwrap(),
        output
    );
    assert_eq!(
        restored.adjustment_description(1),
        document.adjustment_description(1)
    );
    eprintln!("nonlinear curve nested-group maximum stored error {maximum:.10e}");
}
