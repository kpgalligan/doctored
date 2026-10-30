//! The test files that evict or assert on the process-wide view cache or memos, as one binary.
//! Every test takes `serial()` as its first statement, so its guard drops last and each test
//! starts with no live plane of another; the tests run one at a time under both `make test` and
//! plain `cargo test`.
#![allow(clippy::duplicate_mod)]

mod common;
mod live_filter_support;
#[path = "common/phase4_corpus.rs"]
mod phase4_corpus;
#[path = "common/psd16_fixture.rs"]
mod psd16_fixture;
#[path = "common/resource_report.rs"]
mod resource_report;
#[path = "common/rgba_layer.rs"]
mod rgba_layer;
#[allow(dead_code)]
#[path = "common/warp_fixture.rs"]
mod warp_fixture;

static SERIAL: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// The one lock of this suite, taken first by every test; a lock poisoned by a failed test is
/// taken anyway.
fn serial() -> std::sync::MutexGuard<'static, ()> {
    SERIAL
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

#[path = "doc_tests.rs"]
mod doc_tests;
#[path = "independent_masks.rs"]
mod independent_masks;
#[path = "layer_raster.rs"]
mod layer_raster;
#[path = "layer_raster_assets.rs"]
mod layer_raster_assets;
#[path = "layer_raster_compare.rs"]
mod layer_raster_compare;
#[path = "layer_raster_persistence.rs"]
mod layer_raster_persistence;
#[path = "layer_raster_reads.rs"]
mod layer_raster_reads;
#[path = "layer_raster_tiles.rs"]
mod layer_raster_tiles;
#[path = "layer_raster_transforms.rs"]
mod layer_raster_transforms;
#[path = "layer_raster_writers.rs"]
mod layer_raster_writers;
#[path = "native_reopen_plan8.rs"]
mod native_reopen_plan8;
#[path = "native_save_plan8.rs"]
mod native_save_plan8;
#[path = "precision_constructor_seeds.rs"]
mod precision_constructor_seeds;
#[path = "precision_content_box.rs"]
mod precision_content_box;
#[path = "precision_export_reads.rs"]
mod precision_export_reads;
#[path = "precision_filled_outputs.rs"]
mod precision_filled_outputs;
#[path = "precision_image_reads.rs"]
mod precision_image_reads;
#[path = "precision_image_tiles.rs"]
mod precision_image_tiles;
#[path = "precision_image_writers.rs"]
mod precision_image_writers;
#[path = "precision_layer_producers.rs"]
mod precision_layer_producers;
#[path = "precision_plates.rs"]
mod precision_plates;
#[path = "precision_projection_consumers.rs"]
mod precision_projection_consumers;
#[path = "precision_projection_unseeded.rs"]
mod precision_projection_unseeded;
#[path = "precision_rect_writers.rs"]
mod precision_rect_writers;
#[path = "precision_retouch_writers.rs"]
mod precision_retouch_writers;
#[path = "precision_whole_readers.rs"]
mod precision_whole_readers;
#[path = "raw_develop_reads.rs"]
mod raw_develop_reads;
#[path = "resources.rs"]
mod resources;
#[path = "style_cache.rs"]
mod style_cache;
