//! 13B-6: the process-wide resident-cache registry, through the FFI only: the
//! report's `cache_registry` section, renders that fill the caches, and the
//! process working limit that sets the bound (the working limit / 8).
//!
//! Its own binary: the tests set the process working limit and assert on the
//! process-wide registry, so they hold one lock and run one at a time. Each
//! test sizes its bound from an entry it measured first, so no figure here
//! depends on the renderer's plane sizes, and each frees what it made (the
//! LUT memo through the eviction export) so the next one starts empty.

#[path = "common/resource_report.rs"]
mod resource_report;

use std::ffi::{c_char, CStr, CString};
use std::ptr;
use std::sync::{Mutex, MutexGuard, PoisonError};

use doctored_core::doc::DrDocument;
use doctored_core::ffi::*;
use doctored_core::ffi_doc::*;
use doctored_core::ffi_mask::dr_doc_set_mask_properties;
use doctored_core::ffi_precision::dr_precision_doc_render;
use doctored_core::ffi_resources::dr_set_process_working_limit;
use doctored_core::ffi_style::dr_doc_set_layer_style;
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer};
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use doctored_core::style::LayerStyle;
use serde_json::{json, Value};

const W: u32 = 400;
const H: u32 = 300;
const SHADOW: &str = r##"{"effects":[{"type":"drop_shadow","size":12,"distance":6}]}"##;

static LOCK: Mutex<()> = Mutex::new(());

/// The binary's one lock, with the default limit restored and the registry
/// asserted empty, so each test starts from nothing.
fn serial() -> MutexGuard<'static, ()> {
    let guard = LOCK.lock().unwrap_or_else(PoisonError::into_inner);
    assert!(dr_set_process_working_limit(REFERENCE_WORKING_BYTES));
    evict(&[]);
    let section = section();
    assert_eq!(section["bytes"], 0, "a test left entries: {section}");
    guard
}

/// The registry's bound set to `bytes`.
fn set_bound(bytes: u64) {
    assert!(dr_set_process_working_limit(bytes * 8));
    assert_eq!(section()["bound_bytes"], bytes);
}

fn section() -> Value {
    report(&[])["cache_registry"].clone()
}

fn report(documents: &[&Doc]) -> Value {
    let documents: Vec<*const DrDocument> = documents.iter().map(|d| d.0 as *const _).collect();
    resource_report::report(&documents, &[], &[], &[], 0, 0).expect("a report")
}

fn evict(documents: &[&Doc]) {
    let documents: Vec<*const DrDocument> = documents.iter().map(|d| d.0 as *const _).collect();
    resource_report::evict(&documents).expect("an eviction");
}

/// `(bytes, entries)` of `class` in a section.
fn class(section: &Value, class: &str) -> (u64, u64) {
    let figures = &section["classes"][class];
    (
        figures["bytes"].as_u64().expect("class bytes"),
        figures["entries"].as_u64().expect("class entries"),
    )
}

fn number(section: &Value, key: &str) -> u64 {
    section[key]
        .as_u64()
        .unwrap_or_else(|| panic!("{key} in {section}"))
}

fn category(report: &Value, category: &str) -> u64 {
    report["categories"][category]["unique_bytes"]
        .as_u64()
        .unwrap_or(0)
}

struct Doc(*mut DrDocument);

impl Drop for Doc {
    fn drop(&mut self) {
        unsafe { dr_doc_free(self.0) };
    }
}

fn take_error(error: *mut c_char) -> String {
    if error.is_null() {
        return String::new();
    }
    let text = unsafe { CStr::from_ptr(error) }
        .to_string_lossy()
        .into_owned();
    unsafe { dr_string_free(error) };
    text
}

/// An opaque disc on a transparent `W × H` layer (`seed` shifts its colour,
/// so no two documents share pixels), carrying `style` when given.
fn disc(seed: u8, style: Option<&str>) -> Doc {
    let mut pixels = vec![0_u8; W as usize * H as usize * 4];
    for y in 0..H {
        for x in 0..W {
            let (dx, dy) = (x as f32 - W as f32 / 2.0, y as f32 - H as f32 / 2.0);
            if dx * dx + dy * dy < 120.0 * 120.0 {
                let at = (y * W + x) as usize * 4;
                pixels[at..at + 4].copy_from_slice(&[seed, 90, 200, 255]);
            }
        }
    }
    let image = unsafe { dr_image_from_rgba(pixels.as_ptr(), W, H) };
    assert!(!image.is_null());
    let doc = unsafe { dr_doc_from_image(image) };
    unsafe { dr_image_free(image) };
    assert!(!doc.is_null());
    let doc = Doc(doc);
    let Some(style) = style else { return doc };
    let style = CString::new(style).unwrap();
    let mut error = ptr::null_mut();
    let styled = unsafe { dr_doc_set_layer_style(doc.0, 0, style.as_ptr(), &mut error) };
    assert!(
        !styled.is_null(),
        "a styled document: {}",
        take_error(error)
    );
    Doc(styled)
}

/// The document's flattened pixels (which fills its caches).
fn flatten(doc: &Doc) -> Vec<u8> {
    let mut error = ptr::null_mut();
    let image = unsafe { dr_doc_flattened(doc.0, 0, ptr::null(), &mut error) };
    assert!(!image.is_null(), "a projection: {}", take_error(error));
    let pixels = unsafe {
        std::slice::from_raw_parts(dr_image_pixels_rgba(image), W as usize * H as usize * 4)
    }
    .to_vec();
    unsafe { dr_image_free(image) };
    pixels
}

/// One styled document rendered alone: the registry bytes of its one entry.
fn one_style_entry() -> u64 {
    let probe = disc(1, Some(SHADOW));
    flatten(&probe);
    let figures = section();
    assert_eq!(class(&figures, "style_planes").1, 1, "{figures}");
    let bytes = number(&figures, "bytes");
    assert!(bytes > 0);
    bytes
}

#[test]
fn inserts_past_the_bound_evict_and_the_total_stays_within_it() {
    let _serial = serial();
    let entry = one_style_entry();
    let bound = entry * 5 / 2;
    set_bound(bound);
    let before = number(&section(), "evictions");
    let docs: Vec<Doc> = (10..16).map(|seed| disc(seed, Some(SHADOW))).collect();
    for doc in &docs {
        flatten(doc);
        let figures = section();
        assert!(
            number(&figures, "bytes") <= bound,
            "the total stays within the bound: {figures}"
        );
    }
    let figures = section();
    assert_eq!(
        class(&figures, "style_planes").1,
        2,
        "two entries fit: {figures}"
    );
    assert_eq!(number(&figures, "evictions") - before, 4, "{figures}");
}

#[test]
fn the_least_recently_used_entry_goes_first() {
    let _serial = serial();
    let entry = one_style_entry();
    set_bound(entry * 5 / 2);
    let (a, b, c) = (
        disc(20, Some(SHADOW)),
        disc(21, Some(SHADOW)),
        disc(22, Some(SHADOW)),
    );
    flatten(&a);
    let b_pixels = flatten(&b);
    // A is used again, so B is now the least recently used.
    flatten(&a);
    flatten(&c);
    for (name, doc, held) in [("A", &a, true), ("B", &b, false), ("C", &c, true)] {
        let planes = category(&report(&[doc]), "style_cache_planes");
        assert_eq!(planes > 0, held, "{name}'s planes: {planes} bytes");
    }
    assert_eq!(flatten(&b), b_pixels, "an evicted entry renders the same");
}

/// A styled document whose layer carries a feathered mask, so one render
/// registers a derived and an aligned mask plane beside the style entry.
fn feathered(seed: u8) -> Doc {
    let doc = disc(seed, Some(SHADOW));
    let masked = unsafe { dr_doc_adding_layer_mask(doc.0, 0, 0, ptr::null(), 0, 0) };
    assert!(!masked.is_null());
    let masked = Doc(masked);
    let soft = unsafe { dr_doc_set_mask_properties(masked.0, 0, true, 0.8, 12.0) };
    assert!(!soft.is_null());
    Doc(soft)
}

/// A document whose one layer is a Color Lookup adjustment with a meta past
/// the memo's 64 KiB floor: reading it registers one LUT entry.
fn lut_document() -> Doc {
    let base = disc(30, None);
    let meta = json!({
        "type":"adjust", "op":"color_lookup", "params": {
            "kind":"1d", "size":2, "table":"AAAAAAAAAAAAAAAAAACAPwAAgD8AAIA/",
            "padding":"x".repeat(70_000)
        }
    })
    .to_string();
    let meta = CString::new(meta).unwrap();
    let adjusted = unsafe { dr_doc_with_layer_meta(base.0, 0, meta.as_ptr()) };
    assert!(!adjusted.is_null());
    let adjusted = Doc(adjusted);
    assert!(unsafe { dr_doc_layer_is_adjustment(adjusted.0, 0) });
    adjusted
}

#[test]
fn mask_and_lut_entries_are_members_and_leave_least_recent_first() {
    let _serial = serial();
    let masked = feathered(40);
    flatten(&masked);
    let lut = lut_document();
    let figures = section();
    for name in ["mask_derived", "mask_aligned", "lut"] {
        let (bytes, entries) = class(&figures, name);
        assert!(entries == 1 && bytes > 0, "{name}: {figures}");
    }
    // Bound to exactly what is held now, then more fresh style entries than
    // that bound can hold: some fresh entry must leave, and every entry
    // older than the fresh ones leaves before any of them.
    let held = number(&figures, "bytes");
    let (style_bytes, _) = class(&figures, "style_planes");
    set_bound(held);
    let fresh: Vec<Doc> = (0..held / style_bytes + 2)
        .map(|i| disc(41 + i as u8, Some(SHADOW)))
        .collect();
    for doc in &fresh {
        flatten(doc);
    }
    let figures = section();
    assert!(number(&figures, "bytes") <= held, "{figures}");
    for name in ["mask_derived", "mask_aligned", "lut"] {
        assert_eq!(class(&figures, name).1, 0, "{name} left first: {figures}");
    }
    assert!(class(&figures, "style_planes").1 >= 1, "{figures}");
    drop(lut);
}

#[test]
fn class_figures_sum_and_match_the_report_categories() {
    let _serial = serial();
    let doc = disc(50, Some(SHADOW));
    flatten(&doc);
    let figures = section();
    let sum: u64 = [
        "style_planes",
        "typed_style_planes",
        "mask_derived",
        "mask_aligned",
        "lut",
    ]
    .iter()
    .map(|name| class(&figures, name).0)
    .sum();
    assert_eq!(sum, number(&figures, "bytes"));
    let entries: u64 = [
        "style_planes",
        "typed_style_planes",
        "mask_derived",
        "mask_aligned",
        "lut",
    ]
    .iter()
    .map(|name| class(&figures, name).1)
    .sum();
    assert_eq!(entries, number(&figures, "entries"));
    let categories = report(&[&doc]);
    assert_eq!(
        class(&figures, "style_planes").0,
        category(&categories, "style_cache_planes")
            + category(&categories, "style_cache_structure"),
        "an entry's registry bytes are what the report counts for it"
    );
    assert_eq!(figures["content_boxes"]["capacity"], 512);
    assert!(figures["contiguous_views"]["bound_bytes"].as_u64().unwrap() > 0);
}

#[test]
fn an_entry_larger_than_the_bound_is_not_kept() {
    let _serial = serial();
    let entry = one_style_entry();
    set_bound(entry / 2);
    let before = number(&section(), "evictions");
    let doc = disc(60, Some(SHADOW));
    let first = flatten(&doc);
    let figures = section();
    assert_eq!(number(&figures, "entries"), 0, "{figures}");
    assert_eq!(
        number(&figures, "evictions"),
        before,
        "nothing was evicted for it"
    );
    assert_eq!(flatten(&doc), first, "it renders the same without a cache");
}

#[test]
fn freeing_the_documents_returns_their_bytes() {
    let _serial = serial();
    let docs = [feathered(70), disc(71, Some(SHADOW))];
    for doc in &docs {
        flatten(doc);
    }
    assert!(number(&section(), "entries") >= 4);
    drop(docs);
    let figures = section();
    assert_eq!(number(&figures, "bytes"), 0, "{figures}");
    assert_eq!(number(&figures, "entries"), 0, "{figures}");
}

/// A 16-bit document: a soft opaque disc carrying the drop shadow.
fn typed(seed: u16) -> PrecisionDocument {
    let pixels: Vec<[u16; 4]> = (0..W * H)
        .map(|i| {
            let (x, y) = (
                (i % W) as f32 - W as f32 / 2.0,
                (i / W) as f32 - H as f32 / 2.0,
            );
            let inside = x * x + y * y < 120.0 * 120.0;
            [seed, 30000, 50000, if inside { 65535 } else { 0 }]
        })
        .collect();
    let image = PrecisionImage::from_rgba16(W, H, pixels, IccProfile::srgb(), None).unwrap();
    let layer = PrecisionDocumentLayer::raster("Disc", image, (0, 0), 1.0, true)
        .unwrap()
        .with_layer_style(Some(&LayerStyle::from_json(SHADOW).unwrap()), (W, H))
        .unwrap();
    PrecisionDocument::from_parts(
        W,
        H,
        IccProfile::srgb(),
        Metadata::default(),
        Resolution::default(),
        vec![layer],
    )
    .unwrap()
}

/// A typed render's samples as bytes (which fills the typed style cache).
fn render(doc: &PrecisionDocument) -> Vec<u16> {
    let mut error = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_render(doc, REFERENCE_WORKING_BYTES, 0, ptr::null(), &mut error)
    };
    assert!(!out.is_null(), "a typed render: {}", take_error(error));
    let image = unsafe { Box::from_raw(out) };
    image.to_rgba16().unwrap().into_iter().flatten().collect()
}

#[test]
fn typed_style_entries_are_members_and_leave_least_recent_first() {
    let _serial = serial();
    // Both built at the default limit: a typed document checks that it can
    // be saved and reopened inside the process limit, which the bound below
    // takes far under that.
    let (first, second) = (typed(1000), typed(2000));
    let samples = render(&first);
    let figures = section();
    let (entry, entries) = class(&figures, "typed_style_planes");
    assert!(entries == 1 && entry > 0, "{figures}");
    set_bound(entry * 3 / 2);
    let before = number(&figures, "evictions");
    render(&second);
    let figures = section();
    assert_eq!(class(&figures, "typed_style_planes").1, 1, "{figures}");
    assert_eq!(number(&figures, "evictions") - before, 1, "{figures}");
    let typed_planes = |doc: &PrecisionDocument| {
        let value = resource_report::report(&[], &[], &[doc as *const _], &[], 0, 0).unwrap();
        category(&value, "style_cache_planes")
    };
    assert_eq!(typed_planes(&first), 0, "the older entry left");
    assert!(typed_planes(&second) > 0, "the newer entry stays");
    assert_eq!(render(&first), samples, "an evicted entry renders the same");
}
