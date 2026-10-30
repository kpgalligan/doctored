use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::PrecisionDocument;
use doctored_core::precision_image::PrecisionImage;
use doctored_core::raw_local_develop;

const LIMIT: u64 = 2 << 30;
fn ramp(profile: std::sync::Arc<IccProfile>) -> PrecisionDocument {
    let mut pixels = Vec::new();
    for y in 0..80_u32 {
        for x in 0..100_u32 {
            pixels.push([
                ((x * 367 + y * 227 + 1300) % 65536) as u16,
                ((x * 239 + y * 593 + 18000) % 65536) as u16,
                ((x * 479 + y * 151 + 28000) % 65536) as u16,
                65535,
            ]);
        }
    }
    let image = PrecisionImage::from_rgba16(100, 80, pixels, profile, None).unwrap();
    PrecisionDocument::from_image(image, Metadata::default(), Resolution::default()).unwrap()
}
fn develop(
    base: &PrecisionDocument,
    stops: f64,
    reference: Option<([u32; 2], [f64; 6])>,
) -> PrecisionDocument {
    let mut fields = serde_json::json!({"stops":stops,"center":[31.0,53.6],"falloff":1.7});
    if let Some((size, matrix)) = reference {
        fields["reference_size"] = serde_json::json!(size);
        fields["reference_from_source"] = serde_json::json!(matrix);
    }
    raw_local_develop::develop(
        base,
        &[],
        &[],
        "[]",
        Some(&fields.to_string()),
        LIMIT,
        0,
        None,
    )
    .unwrap()
    .unwrap()
}
fn pixels(document: &PrecisionDocument) -> Vec<[u16; 4]> {
    document.layer_source(0).unwrap().to_rgba16().unwrap()
}
#[test]
fn registered_reference_exact_permutations_crop_pad_and_profiles() {
    let mut compared = 0;
    for profile in [IccProfile::srgb(), IccProfile::display_p3()] {
        for stops in [-1.25, 0.75] {
            let source = ramp(profile.clone());
            let legacy = develop(&source, stops, None);
            let identity = develop(&source, stops, Some(([100, 80], [1., 0., 0., 1., 0., 0.])));
            assert_eq!(pixels(&legacy), pixels(&identity));
            let mut requests = vec![
                (
                    serde_json::json!({"version":1,"kind":"affine","width":80,"height":60,
                    "inverse":[1.,0.,0.,1.,20.,10.],"filter":"bilinear"}),
                    [1., 0., 0., 1., 20., 10.],
                ),
                (
                    serde_json::json!({"version":1,"kind":"affine","width":120,"height":96,
                    "inverse":[1.,0.,0.,1.,-10.,-8.],"filter":"bicubic"}),
                    [1., 0., 0., 1., -10., -8.],
                ),
            ];
            let exif = [
                [1., 0., 0., 1., 0., 0.],
                [-1., 0., 0., 1., 100., 0.],
                [-1., 0., 0., -1., 100., 80.],
                [1., 0., 0., -1., 0., 80.],
                [0., 1., 1., 0., 0., 0.],
                [0., -1., 1., 0., 0., 80.],
                [0., -1., -1., 0., 100., 80.],
                [0., 1., -1., 0., 100., 0.],
            ];
            for (i, matrix) in exif.into_iter().enumerate() {
                requests.push((
                    serde_json::json!({"version":1,"kind":"exif","orientation":i+1}),
                    matrix,
                ));
            }
            for (request, matrix) in requests {
                let text = request.to_string();
                let mapped_source = source
                    .raw_source_geometry(&text, LIMIT, 0, None)
                    .unwrap()
                    .unwrap_or_else(|| source.clone());
                let mapped_legacy = legacy
                    .raw_source_geometry(&text, LIMIT, 0, None)
                    .unwrap()
                    .unwrap_or_else(|| legacy.clone());
                let registered = develop(&mapped_source, stops, Some(([100, 80], matrix)));
                assert_eq!(
                    pixels(&registered),
                    pixels(&mapped_legacy),
                    "registered vignette disagrees with exact source permutation {text}"
                );
                compared += pixels(&registered).len();
            }
        }
    }
    println!("registered-vignette-exact samples_compared={compared} max_error=0 profiles=2 signs=2 maps=10");
}

#[test]
fn registered_vignette_rejects_bad_reference_atomically() {
    let source = ramp(IccProfile::srgb());
    let initial = pixels(&source).to_vec();
    for fields in [
        serde_json::json!({"reference_size":[100,80]}),
        serde_json::json!({"reference_size":[0,80],"reference_from_source":[1,0,0,1,0,0]}),
        serde_json::json!({"reference_size":[100,80],"reference_from_source":[1,0,0,0,0,0]}),
        serde_json::json!({"reference_size":[100,80],"reference_from_source":[1,0,0,1,1e100,0]}),
    ] {
        let mut request = serde_json::json!({"stops":-1.25,"center":[31.0,53.6],"falloff":1.7});
        request
            .as_object_mut()
            .unwrap()
            .extend(fields.as_object().unwrap().clone());
        assert!(raw_local_develop::develop(
            &source,
            &[],
            &[],
            "[]",
            Some(&request.to_string()),
            LIMIT,
            0,
            None
        )
        .is_err());
        assert_eq!(pixels(&source), initial);
    }
}

fn analytic_srgb(v: f64) -> f64 {
    if v <= 0.04045 {
        v / 12.92
    } else {
        ((v + 0.055) / 1.055).powf(2.4)
    }
}
fn sampled_profile_decode(v: f64) -> f64 {
    let p = v.clamp(0.0, 1.0) * 1023.0;
    let i = (p.floor() as usize).min(1022);
    let sample = |k: usize| (analytic_srgb(k as f64 / 1023.0) * 65535.0).round() / 65535.0;
    sample(i) * (1.0 - (p - i as f64)) + sample(i + 1) * (p - i as f64)
}
fn sampled_profile_encode(v: f64) -> f64 {
    let mut low = 0.0;
    let mut high = 1.0;
    for _ in 0..32 {
        let mid = (low + high) / 2.0;
        if sampled_profile_decode(mid) < v {
            low = mid;
        } else {
            high = mid;
        }
    }
    (low + high) / 2.0
}
#[test]
fn registered_affine_uses_actual_current_inputs_not_filtered_old_result() {
    let matrix = [1.0, 0.07, 0.15, 1.0, 11.3, 7.2];
    let request = serde_json::json!({"version":1,"kind":"affine","width":80,"height":60,
        "inverse":matrix,"filter":"bilinear"})
    .to_string();
    let center = [31.0, 53.6];
    let radius = [0.5_f64, 99.5]
        .into_iter()
        .flat_map(|x| [0.5, 79.5].map(move |y| (x - center[0]).hypot(y - center[1])))
        .fold(0.0, f64::max);
    let mut count = 0;
    let mut max_error = 0;
    let mut wrong_policy_difference = 0;
    for profile in [IccProfile::srgb(), IccProfile::display_p3()] {
        let source = ramp(profile);
        let current = source
            .raw_source_geometry(&request, LIMIT, 0, None)
            .unwrap()
            .unwrap();
        let corrected = develop(&current, -1.25, Some(([100, 80], matrix)));
        let old_policy = raw_local_develop::develop(
            &current,
            &[],
            &[],
            "[]",
            Some(
                &serde_json::json!({"stops":-1.25,"center":[24.8,40.2],"falloff":1.7}).to_string(),
            ),
            LIMIT,
            0,
            None,
        )
        .unwrap()
        .unwrap();
        for (i, (&actual, &input)) in pixels(&corrected)
            .iter()
            .zip(pixels(&current).iter())
            .enumerate()
        {
            let x = (i % 80) as f64 + 0.5;
            let y = (i / 80) as f64 + 0.5;
            let mapped = [
                matrix[0] * x + matrix[2] * y + matrix[4],
                matrix[1] * x + matrix[3] * y + matrix[5],
            ];
            let distance = (mapped[0] - center[0]).hypot(mapped[1] - center[1]);
            let stops = (-1.25 * (distance / radius).min(1.0).powf(1.7)) as f32;
            let gain = f64::from(stops.exp2());
            for c in 0..3 {
                let v = f64::from(input[c]) / 65535.0;
                let expected = (sampled_profile_encode(sampled_profile_decode(v) * gain)
                    .clamp(0.0, 1.0)
                    * 65535.0)
                    .round() as u16;
                max_error = max_error.max(actual[c].abs_diff(expected));
                wrong_policy_difference =
                    wrong_policy_difference.max(actual[c].abs_diff(pixels(&old_policy)[i][c]));
            }
            assert_eq!(actual[3], input[3]);
            count += 1;
        }
    }
    assert!(
        max_error <= 3,
        "registered affine ICC reference max error {max_error}"
    );
    assert!(
        wrong_policy_difference >= 128,
        "old policy difference was vacuous: {wrong_policy_difference}"
    );
    println!("registered-vignette-affine samples={count} max_icc_error={max_error} old_policy_difference={wrong_policy_difference}");
}
