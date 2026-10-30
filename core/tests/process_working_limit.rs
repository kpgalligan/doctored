//! The process-wide working limit (`dr_set_process_working_limit`): the
//! limit read wherever no caller limit is passed. Exactly one test, because
//! the limit is process-global and tests in one binary run in parallel.
use doctored_core::ffi_resources::{dr_process_cache_trim_limit, dr_set_process_working_limit};
use doctored_core::icc::IccProfile;
use doctored_core::precision_image::{
    process_working_limit, PrecisionImage, PrecisionStorage, REFERENCE_WORKING_BYTES,
};
use doctored_core::precision_render::{PrecisionLayer, PrecisionScene};

fn small() -> Result<PrecisionImage, String> {
    PrecisionImage::from_rgba16(2, 2, vec![[0, 0, 0, 65535]; 4], IccProfile::srgb(), None)
}

#[test]
fn the_process_limit_governs_internal_admission_only() {
    assert_eq!(process_working_limit(), REFERENCE_WORKING_BYTES);
    assert_eq!(dr_process_cache_trim_limit(), REFERENCE_WORKING_BYTES / 8);

    assert!(!dr_set_process_working_limit(0));
    assert_eq!(process_working_limit(), REFERENCE_WORKING_BYTES);

    assert!(small().is_ok());

    assert!(dr_set_process_working_limit(1024 * 1024));
    assert_eq!(dr_process_cache_trim_limit(), 1024 * 1024 / 8);
    let error = small().err().expect("refused at 1 MiB");
    assert!(error.contains("working limit"), "{error}");

    assert!(dr_set_process_working_limit(REFERENCE_WORKING_BYTES));
    assert_eq!(process_working_limit(), REFERENCE_WORKING_BYTES);
    let image = small().expect("admitted at the default");

    let layer = PrecisionLayer::raster(image, (0, 0), 1.0).expect("layer");
    let scene =
        PrecisionScene::new(2, 2, IccProfile::srgb(), vec![layer], None, false).expect("scene");
    assert!(scene
        .render(
            PrecisionStorage::Rgba16Unorm,
            4 * REFERENCE_WORKING_BYTES,
            0,
            None
        )
        .is_ok());
}
