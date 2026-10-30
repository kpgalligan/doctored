//! Independent f64 premultiplied group evaluation for the 7B extension.
use doctored_core::icc::IccProfile;
use doctored_core::precision_image::{PrecisionImage, PrecisionStorage, REFERENCE_WORKING_BYTES};
use doctored_core::precision_render::{PrecisionLayer, PrecisionScene};

fn raster(samples: Vec<[u16; 4]>, opacity: f32) -> PrecisionLayer {
    let image =
        PrecisionImage::from_rgba16(samples.len() as u32, 1, samples, IccProfile::srgb(), None)
            .unwrap();
    PrecisionLayer::raster(image, (0, 0), opacity).unwrap()
}
fn premul(pixel: [u16; 4]) -> [f64; 4] {
    let value = pixel.map(|v| f64::from(v) / 65535.0);
    [
        value[0] * value[3],
        value[1] * value[3],
        value[2] * value[3],
        value[3],
    ]
}
fn over(source: [f64; 4], destination: [f64; 4], opacity: f32) -> [f64; 4] {
    let opacity = f64::from(opacity);
    std::array::from_fn(|c| source[c] * opacity + destination[c] * (1.0 - source[3] * opacity))
}
fn gate(before: [f64; 4], after: [f64; 4], weight: f32) -> [f64; 4] {
    let weight = f64::from(weight);
    std::array::from_fn(|c| (1.0 - weight) * before[c] + weight * after[c])
}

#[test]
fn nested_pass_through_and_isolation_keep_fractional_precision_against_f64() {
    let background = [[30000, 11001, 49002, 32123], [12345, 45678, 23456, 0]];
    let first = [[45678, 34567, 23456, 54321], [30001, 11002, 49003, 23456]];
    let second = [[23456, 45678, 56789, 12345], [65535, 11111, 22222, 34567]];
    let a = 0.625;
    let b = 0.375;
    let isolated_opacity = 0.75;
    let outer_opacity = 0.3125;
    let layers = vec![
        raster(background.to_vec(), 1.0),
        PrecisionLayer::pass_through(
            vec![
                raster(first.to_vec(), a),
                PrecisionLayer::isolated_normal(
                    vec![
                        PrecisionLayer::pass_through(vec![raster(second.to_vec(), b)], 0.5)
                            .unwrap(),
                    ],
                    isolated_opacity,
                )
                .unwrap(),
            ],
            outer_opacity,
        )
        .unwrap(),
    ];
    let scene = PrecisionScene::new(2, 1, IccProfile::srgb(), layers, None, false).unwrap();
    let plan = scene.plan(PrecisionStorage::Rgba16Unorm).unwrap();
    assert_eq!(plan.group_depth, 3);
    for storage in [PrecisionStorage::Rgba16Unorm, PrecisionStorage::Rgba32Float] {
        let image = scene
            .render(storage, REFERENCE_WORKING_BYTES, 0, None)
            .unwrap();
        for i in 0..2 {
            let before = premul(background[i]);
            let first_result = over(premul(first[i]), before, a);
            let internal = gate([0.0; 4], over(premul(second[i]), [0.0; 4], b), 0.5);
            let after = over(internal, first_result, isolated_opacity);
            let expected = gate(before, after, outer_opacity);
            let straight = [
                expected[0] / expected[3],
                expected[1] / expected[3],
                expected[2] / expected[3],
                expected[3],
            ];
            let width = image.dimensions().0;
            let (x, y) = (i as u32 % width, i as u32 / width);
            let actual = match storage {
                PrecisionStorage::Rgba16Unorm => image
                    .rgba16_at(x, y)
                    .unwrap()
                    .map(|v| f64::from(v) / 65535.0),
                PrecisionStorage::Rgba32Float => image.sample_at(x, y).unwrap().map(f64::from),
            };
            let tolerance = 2e-6
                + if storage == PrecisionStorage::Rgba16Unorm {
                    0.5 / 65535.0
                } else {
                    0.0
                };
            for c in 0..4 {
                assert!(
                    (actual[c] - straight[c]).abs() <= tolerance,
                    "{storage:?} pixel{i} channel{c}"
                );
            }
        }
    }
}

#[test]
fn pass_through_endpoints_are_exact_and_tiny_positive_alpha_refuses() {
    let bottom = [[12345, 23456, 34567, 45678], [54321, 43210, 32109, 0]];
    let top = [[12346, 23457, 34568, 34567], [54322, 43211, 32110, 65535]];
    let render = |layers| {
        PrecisionScene::new(2, 1, IccProfile::srgb(), layers, None, false)
            .unwrap()
            .render(
                PrecisionStorage::Rgba32Float,
                REFERENCE_WORKING_BYTES,
                0,
                None,
            )
            .unwrap()
    };
    let before = render(vec![raster(bottom.to_vec(), 1.0)]);
    let after = render(vec![
        raster(bottom.to_vec(), 1.0),
        raster(top.to_vec(), 1.0),
    ]);
    for (opacity, expected) in [(0.0, before), (1.0, after)] {
        let actual = render(vec![
            raster(bottom.to_vec(), 1.0),
            PrecisionLayer::pass_through(vec![raster(top.to_vec(), 1.0)], opacity).unwrap(),
        ]);
        assert_eq!(actual.to_rgba32().ok(), expected.to_rgba32().ok());
    }
    let layer =
        PrecisionLayer::pass_through(vec![raster(top.to_vec(), 1.0)], f32::from_bits(1)).unwrap();
    let scene = PrecisionScene::new(2, 1, IccProfile::srgb(), vec![layer], None, false).unwrap();
    assert!(scene
        .render(
            PrecisionStorage::Rgba16Unorm,
            REFERENCE_WORKING_BYTES,
            0,
            None
        )
        .is_err());
}
