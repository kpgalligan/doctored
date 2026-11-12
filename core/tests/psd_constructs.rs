//! Per-construct PSD exchange rules, black-box through the C API: which
//! Photoshop constructs Import Layers preserves, which it omits with a report
//! sentence, and which the layered export rasterizes or refuses.
//!
//! Every PSD in this file is an independent byte construction
//! (`common/psd_mask_fixture.rs` at 8 bits, `common/psd16_fixture.rs` at 16),
//! and every expected picture is hand-computed source-over or a projection of
//! a document the exporter did not build. `DR_PSD_CONSTRUCT_FIXTURES=<dir>`
//! also writes the files, for review and for driving the built app.

use std::ffi::{c_char, CString};
use std::path::Path;
use std::ptr;
use std::sync::Arc;

use doctored_core::doc::{BlendMode, DrDocument, LayerKind};
use doctored_core::ffi_psd::{
    dr_doc_open_psd, dr_doc_psd_export_report, dr_doc_save_psd, dr_precision_doc_open_psd,
    dr_psd_import_report,
};
use doctored_core::job_control::DrJobControl;
use doctored_core::precision_document::PrecisionDocument;
use doctored_core::precision_image::REFERENCE_WORKING_BYTES;
use doctored_core::style::{
    BlendIf, BlendIfChannel, ColorOverlay, DropShadow, Effect, LayerStyle, Stroke, StrokePosition,
};
use image::{GrayImage, Rgba, RgbaImage};
use serde_json::Value;
use tempfile::TempDir;

use crate::common;
use crate::psd16_fixture as fixture16;
use crate::psd_mask_fixture as fixture8;
use common::{cpath, take_err_string};

const LAYERS: i32 = 0;
const FLATTENED: i32 = 1;
const CANVAS: [u32; 2] = [2, 1];

// ------------------------------------------------------------- C plumbing --

fn import_report(path: &Path) -> Value {
    let mut error = ptr::null_mut();
    let raw = unsafe { dr_psd_import_report(cpath(path).as_ptr(), &mut error) };
    assert!(!raw.is_null(), "{}", take_err_string(error));
    assert!(error.is_null());
    serde_json::from_str(&take_err_string(raw)).expect("import report JSON")
}

fn open_psd(path: &Path, mode: i32, report: &Value) -> Box<DrDocument> {
    let token = CString::new(report["source_fingerprint"].as_str().unwrap()).unwrap();
    let mut error = ptr::null_mut();
    let raw = unsafe {
        dr_doc_open_psd(
            cpath(path).as_ptr(),
            mode,
            token.as_ptr(),
            std::ptr::null(),
            &mut error,
        )
    };
    assert!(
        !raw.is_null(),
        "open_psd({mode}): {}",
        take_err_string(error)
    );
    assert!(error.is_null());
    unsafe { Box::from_raw(raw) }
}

/// A typed document handle freed through the C API on drop.
struct Owned(*mut PrecisionDocument);
impl Drop for Owned {
    fn drop(&mut self) {
        unsafe { doctored_core::ffi_precision::dr_precision_doc_free(self.0) };
    }
}
impl std::ops::Deref for Owned {
    type Target = PrecisionDocument;
    fn deref(&self) -> &PrecisionDocument {
        unsafe { &*self.0 }
    }
}

fn open_typed(path: &Path, mode: i32, report: &Value) -> Owned {
    let token = CString::new(report["source_fingerprint"].as_str().unwrap()).unwrap();
    let mut error = ptr::null_mut();
    let raw = unsafe {
        dr_precision_doc_open_psd(
            cpath(path).as_ptr(),
            mode,
            token.as_ptr(),
            REFERENCE_WORKING_BYTES,
            0,
            ptr::null::<DrJobControl>(),
            &mut error,
        )
    };
    assert!(!raw.is_null(), "typed open: {}", take_err_string(error));
    Owned(raw)
}

fn typed_info(document: &PrecisionDocument) -> Value {
    let mut error = ptr::null_mut();
    let json = unsafe { doctored_core::ffi_precision::dr_precision_doc_info(document, &mut error) };
    assert!(!json.is_null(), "{}", take_err_string(error));
    serde_json::from_str(&take_err_string(json)).expect("info JSON")
}

fn typed_pixels(document: &PrecisionDocument, index: usize) -> Vec<[u16; 4]> {
    let image =
        unsafe { doctored_core::ffi_precision::dr_precision_doc_layer_source(document, index) };
    assert!(!image.is_null(), "layer {index} has no raster source");
    let (w, h) = unsafe { &*image }.dimensions();
    let base = unsafe { doctored_core::ffi_precision::dr_precision_image_pixels_rgba16(&*image) };
    assert!(!base.is_null());
    let samples = unsafe { std::slice::from_raw_parts(base, w as usize * h as usize * 4) }.to_vec();
    unsafe { doctored_core::ffi_precision::dr_precision_image_free(image) };
    samples
        .chunks_exact(4)
        .map(|p| [p[0], p[1], p[2], p[3]])
        .collect()
}

fn export_report(doc: &DrDocument, mode: i32) -> Value {
    let mut error: *mut c_char = ptr::null_mut();
    let json = unsafe { dr_doc_psd_export_report(doc, 0, mode, &mut error) };
    assert!(!json.is_null(), "export report: {}", take_err_string(error));
    serde_json::from_str(&take_err_string(json)).expect("export report JSON")
}

fn save(doc: &DrDocument, path: &Path, mode: i32) -> Result<(), String> {
    let c = cpath(path);
    let mut error: *mut c_char = ptr::null_mut();
    if unsafe {
        dr_doc_save_psd(
            doc,
            ptr::null(),
            c.as_ptr(),
            0,
            mode,
            std::ptr::null(),
            &mut error,
        )
    } {
        assert!(error.is_null(), "err_out set on success");
        Ok(())
    } else {
        Err(take_err_string(error))
    }
}

fn strings(value: &Value, key: &str) -> Vec<String> {
    value[key]
        .as_array()
        .unwrap_or_else(|| panic!("{key} array"))
        .iter()
        .map(|v| v.as_str().expect("string").to_string())
        .collect()
}

fn write(dir: &TempDir, name: &str, bytes: &[u8]) -> std::path::PathBuf {
    let path = dir.path().join(name);
    std::fs::write(&path, bytes).unwrap();
    if let Ok(directory) = std::env::var("DR_PSD_CONSTRUCT_FIXTURES") {
        let directory = Path::new(&directory);
        std::fs::create_dir_all(directory).unwrap();
        std::fs::write(directory.join(name), bytes).unwrap();
    }
    path
}

// ---------------------------------------------------------- 8-bit fixture --

const BASE: [u8; 4] = [10, 20, 30, 255];
const ART: [u8; 4] = [200, 100, 50, 255];

/// The construct fixture: an opaque base, a CLIPPED pass-through folder with a
/// full mask holding one opaque raster, a pixel-less `curv` adjustment record
/// with a mask, a pixel-less `SoCo` fill record, and a pixel-less record with
/// no adjustment key at all.
fn constructs8() -> Vec<u8> {
    let mut base = fixture8::Record::raster([0, 0, 1, 1], &BASE, false);
    base.name = "Base";
    let mut art = fixture8::Record::raster([0, 0, 1, 2], &ART.repeat(2), false);
    art.name = "Art";
    let mut closer = fixture8::Record::folder(3, *b"norm");
    closer.name = "</Layer group>";
    let mut outer = fixture8::Record::folder(1, *b"pass").mask(&folder_mask(), false);
    outer.name = "Outer";
    outer.clipping = 1;
    let mut curves = empty8("Curves 1");
    curves.blocks = fixture8::block(b"curv", &[0; 4]);
    curves = curves.mask(&record_mask(), false);
    let mut fill = empty8("Fill");
    fill.blocks = fixture8::block(b"SoCo", &[0; 4]);
    let empty = empty8("Empty");
    fixture8::fixture(
        &[base, closer, art, outer, curves, fill, empty],
        CANVAS,
        false,
    )
}

/// A pixel-less record: a zero rectangle and four bare compression words.
fn empty8(name: &'static str) -> fixture8::Record {
    let mut record = fixture8::Record::raster([0; 4], &[], false);
    record.name = name;
    record
}

fn folder_mask() -> fixture8::ExpectedMask {
    fixture8::ExpectedMask {
        bounds: [0, 0, 1, 2],
        coverage: vec![255, 255],
        outside: 255,
        enabled: true,
    }
}

fn record_mask() -> fixture8::ExpectedMask {
    fixture8::ExpectedMask {
        bounds: [0, 0, 1, 2],
        coverage: vec![128, 64],
        outside: 0,
        enabled: true,
    }
}

// ------------------------------------------------------- import, 8 bits --

#[test]
fn pixel_less_records_are_omitted_created_empty_and_the_clipped_folder_survives() {
    let dir = TempDir::new().unwrap();
    let path = write(&dir, "constructs8.psd", &constructs8());
    let report = import_report(&path);
    assert_eq!(report["layered_available"], Value::Bool(true), "{report}");

    let losses = strings(&report, "verified_losses");
    for key in ["curv", "SoCo"] {
        assert!(
            losses
                .iter()
                .any(|l| l.contains(&format!("({key}) with no pixels"))
                    && l.contains("layered import omits it and its mask")),
            "the {key} omission: {losses:?}"
        );
    }
    assert!(
        !losses.iter().any(|l| l.contains("rectangle")),
        "a pixel-less record is not a malformed rectangle: {losses:?}"
    );
    let observations = strings(&report, "observations");
    assert!(
        observations
            .iter()
            .any(|o| o.contains("“Empty”") && o.contains("empty canvas-sized layer")),
        "{observations:?}"
    );
    assert!(
        !observations.iter().any(|o| o.contains("extends outside")),
        "an omitted or empty record has no outside-canvas rectangle: {observations:?}"
    );

    let doc = open_psd(&path, LAYERS, &report);
    let names: Vec<&str> = doc.layers.iter().map(|l| l.name.as_str()).collect();
    assert_eq!(names, ["Base", "Art", "Outer", "Empty"]);
    assert_eq!(
        doc.layers.iter().map(|l| l.depth).collect::<Vec<_>>(),
        [0, 1, 0, 0]
    );
    assert_eq!(doc.layers[2].kind, LayerKind::Group);
    assert!(doc.layers[2].clipped, "the folder's clipping byte");
    assert!(doc.layers[2].mask.is_some(), "the folder's mask");
    assert_eq!(doc.layers[3].pixels.dimensions(), (CANVAS[0], CANVAS[1]));
    assert_eq!(doc.layers[3].offset, (0, 0));
    assert!(
        doc.layers[3]
            .pixels
            .contiguous_image()
            .as_raw()
            .iter()
            .all(|s| *s == 0),
        "the empty layer is transparent"
    );

    // Hand-computed: the folder is clipped to “Base”, which covers only the
    // left pixel, so “Art” lands there and the right pixel stays empty.
    let flat = doc.flattened().expect("flatten");
    assert_eq!(flat.dimensions(), (CANVAS[0], CANVAS[1]));
    assert_eq!(flat.get_pixel(0, 0), &Rgba(ART));
    assert_eq!(flat.get_pixel(1, 0), &Rgba([0, 0, 0, 0]));
}

#[test]
fn a_pixel_less_record_with_stray_plane_bytes_still_refuses() {
    let dir = TempDir::new().unwrap();
    let mut record = empty8("Empty");
    record.channels[0].1 = vec![0, 0, 7]; // a raw plane one byte too long
    let mut base = fixture8::Record::raster([0, 0, 1, 1], &BASE, false);
    base.name = "Base";
    let path = write(
        &dir,
        "stray8.psd",
        &fixture8::fixture(&[base, record], CANVAS, false),
    );
    let report = import_report(&path);
    assert_eq!(report["layered_available"], Value::Bool(false));
    let refusal = report["layered_refusal"].as_str().expect("refusal");
    assert!(refusal.contains("length mismatch"), "{refusal}");
}

// ------------------------------------------------------ import, 16 bits --

fn empty16(name: &str) -> fixture16::Record {
    fixture16::Record::raster(
        name,
        [0; 4],
        [-1i16, 0, 1, 2]
            .into_iter()
            .map(|id| (id, fixture16::raw_plane(&[])))
            .collect(),
    )
}

fn planes16(rgba: [u16; 4], count: usize) -> Vec<(i16, Vec<u8>)> {
    [(-1i16, 3usize), (0, 0), (1, 1), (2, 2)]
        .into_iter()
        .map(|(id, component)| (id, fixture16::raw_plane(&vec![rgba[component]; count])))
        .collect()
}

fn constructs16() -> Vec<u8> {
    let base = fixture16::Record::raster("Base", [0, 0, 1, 1], planes16(BASE16, 1));
    let art = fixture16::Record::raster("Art", [0, 0, 1, 2], planes16(ART16, 2));
    let closer = fixture16::Record::folder("</Layer group>", 3, *b"norm");
    let mut outer = fixture16::Record::folder("Outer", 1, *b"pass");
    outer.clipping = 1;
    let mut curves = empty16("Curves 1");
    curves.blocks = fixture16::block(b"curv", &[0; 4]);
    curves.clipping = 1;
    let mut fill = empty16("Fill");
    fill.blocks = fixture16::block(b"SoCo", &[0; 4]);
    let empty = empty16("Empty");
    fixture16::fixture(
        (CANVAS[0], CANVAS[1]),
        &[base, closer, art, outer, curves, fill, empty],
        doctored_core::icc::IccProfile::srgb().bytes(),
        (72.0, 72.0),
        &[ART16, ART16],
        true,
        false,
    )
}

const BASE16: [u16; 4] = [1000, 2000, 3000, 65535];
const ART16: [u16; 4] = [40000, 20000, 10000, 65535];

#[test]
fn the_typed_import_applies_the_same_per_record_rules() {
    let dir = TempDir::new().unwrap();
    let path = write(&dir, "constructs16.psd", &constructs16());
    let report = import_report(&path);
    assert_eq!(report["layered_available"], Value::Bool(true));
    assert_eq!(report["import_precision"], Value::from("rgba16"));

    let losses = strings(&report, "verified_losses");
    for key in ["curv", "SoCo"] {
        assert!(
            losses
                .iter()
                .any(|l| l.contains(&format!("({key}) with no pixels"))),
            "the {key} omission: {losses:?}"
        );
    }
    assert!(
        !losses.iter().any(|l| l.contains("is clipped")),
        "a 16-bit document retains clipping, and an omitted record has none left to lose: {losses:?}"
    );
    assert!(
        strings(&report, "observations")
            .iter()
            .any(|o| o.contains("“Empty”") && o.contains("empty canvas-sized layer")),
        "{:?}",
        strings(&report, "observations")
    );

    let document = open_typed(&path, LAYERS, &report);
    let info = typed_info(&document);
    let layers = info["layers"].as_array().expect("layers");
    let names: Vec<&str> = layers.iter().map(|l| l["name"].as_str().unwrap()).collect();
    assert_eq!(names, ["Base", "Art", "Outer", "Empty"]);
    let roots = layers
        .iter()
        .filter(|l| l["depth"].as_u64() == Some(0))
        .count();
    assert_eq!(roots, 3, "Base, Outer and Empty");
    assert_eq!(
        layers[2]["clipped"],
        Value::Bool(true),
        "the folder's clipping byte"
    );
    assert_eq!(layers[1]["clipped"], Value::Bool(false));
    assert_eq!(
        layers[3]["bounds"],
        serde_json::json!([0, 0, CANVAS[0], CANVAS[1]])
    );
    assert_eq!(
        typed_pixels(&document, 3),
        vec![[0u16; 4]; (CANVAS[0] * CANVAS[1]) as usize],
        "the empty layer is a canvas-sized transparent raster"
    );

    // Hand-computed, exactly as the 8-bit picture: the folder is clipped to
    // “Base”, which covers only the left pixel, so “Art” lands there and the
    // right pixel stays empty.
    let projection = document
        .render(REFERENCE_WORKING_BYTES, 0, None)
        .expect("render");
    assert_eq!(projection.to_rgba16().ok().unwrap(), &[ART16, [0u16; 4]]);
}

// ----------------------------------------- a file that retains nothing --

#[test]
fn a_file_of_nothing_but_adjustment_records_opens_its_stored_composite() {
    let dir = TempDir::new().unwrap();
    let mut fill = empty8("Fill");
    fill.blocks = fixture8::block(b"SoCo", &[0; 4]);
    let path = write(
        &dir,
        "only-fill8.psd",
        &fixture8::fixture(&[fill], CANVAS, false),
    );
    let report = import_report(&path);
    assert_eq!(report["layered_available"], Value::Bool(true));
    assert!(strings(&report, "verified_losses")
        .iter()
        .any(|l| l.contains("(SoCo) with no pixels")));
    assert!(
        strings(&report, "observations")
            .iter()
            .any(|o| o.contains("Import Layers opens the stored composite as a single layer")),
        "{:?}",
        strings(&report, "observations")
    );
    let layered = open_psd(&path, LAYERS, &report);
    let composite = open_psd(&path, FLATTENED, &report);
    assert_eq!(layered.layers.len(), 1);
    assert_eq!(
        layered.layers[0].pixels.contiguous_image().as_raw(),
        composite.layers[0].pixels.contiguous_image().as_raw()
    );

    let mut fill16 = empty16("Fill");
    fill16.blocks = fixture16::block(b"SoCo", &[0; 4]);
    let path = write(
        &dir,
        "only-fill16.psd",
        &fixture16::fixture(
            (CANVAS[0], CANVAS[1]),
            &[fill16],
            doctored_core::icc::IccProfile::srgb().bytes(),
            (72.0, 72.0),
            &[ART16, ART16],
            true,
            false,
        ),
    );
    let report = import_report(&path);
    assert_eq!(report["layered_available"], Value::Bool(true));
    assert!(strings(&report, "verified_losses")
        .iter()
        .any(|l| l.contains("(SoCo) with no pixels")));
    assert!(strings(&report, "observations")
        .iter()
        .any(|o| o.contains("Import Layers opens the stored composite as a single layer")));
    let layered = open_typed(&path, LAYERS, &report);
    let composite = open_typed(&path, FLATTENED, &report);
    assert_eq!(typed_info(&layered)["layers"].as_array().unwrap().len(), 1);
    assert_eq!(typed_pixels(&layered, 0), typed_pixels(&composite, 0));
}

// ----------------------------------------------------- export fixtures --

fn checker(w: u32, h: u32) -> RgbaImage {
    let mut out = RgbaImage::new(w, h);
    for y in 0..h {
        for x in 0..w {
            let a = if (x + y) % 5 == 0 { 130 } else { 255 };
            out.put_pixel(
                x,
                y,
                Rgba([(x * 13 % 251) as u8, (y * 7 % 241) as u8, 90, a]),
            );
        }
    }
    out
}

fn style() -> LayerStyle {
    let mut style = LayerStyle::default();
    style.fill_opacity = 0.5;
    style.blend_if = Some(BlendIf {
        channel: BlendIfChannel::Gray,
        this_layer: [0, 10, 240, 255],
        underlying: [0, 0, 255, 255],
        ..BlendIf::default()
    });
    // Declaration order IS render order, so the stack stays sorted by kind.
    style.effects = vec![
        Effect::DropShadow(DropShadow {
            distance: 5.0,
            angle: 120.0,
            use_global_light: false,
            size: 4.0,
            ..DropShadow::default()
        }),
        Effect::Stroke(Stroke {
            size: 2.0,
            position: StrokePosition::Outside,
            ..Stroke::default()
        }),
        Effect::ColorOverlay(ColorOverlay::default()),
    ];
    style
}

/// A 64 x 64 document holding a styled 16 x 16 raster at (20, 20) — opacity
/// 0.6, Multiply, a mask — with a plain layer clipped to it directly above.
fn styled_document(mask_enabled: bool, density: f32, feather: f32) -> DrDocument {
    let doc = DrDocument::from_pixels(RgbaImage::new(64, 64));
    let doc = doc
        .adding_image_layer(0, checker(16, 16), "Styled")
        .expect("styled layer");
    let doc = doc
        .adding_image_layer(1, checker(8, 8), "Clipped")
        .expect("clipped layer");
    let mut doc = doc;
    let styled = &mut doc.layers[1];
    styled.offset = (20, 20);
    styled.opacity = 0.6;
    styled.blend = BlendMode::Multiply;
    styled.style = Some(Arc::new(style()));
    let mut mask = GrayImage::new(16, 16);
    for (index, pixel) in mask.pixels_mut().enumerate() {
        pixel.0[0] = (index * 3 % 256) as u8;
    }
    styled.mask = Some(Arc::new(mask));
    styled.mask_enabled = mask_enabled;
    styled.mask_properties.offset = (20, 20);
    styled.mask_properties.density = density;
    styled.mask_properties.feather = feather;
    doc.layers[2].offset = (24, 24);
    doc.layers[2].clipped = true;
    doc
}

/// The same styled layer alone on the same canvas, as a PACKAGE: opacity 1,
/// Normal, no Blend If — which is exactly what the record's own bytes then
/// apply. Its projection is the independent oracle for the bake.
fn package(doc: &DrDocument) -> DrDocument {
    let mut single = DrDocument::from_pixels(RgbaImage::new(doc.width, doc.height));
    let mut layer = doc.layers[1].clone();
    layer.opacity = 1.0;
    layer.blend = BlendMode::Normal;
    layer.clipped = false;
    let mut package_style = style();
    package_style.blend_if = None;
    layer.style = Some(Arc::new(package_style));
    single.layers = vec![layer];
    single
}

fn rasterize_loss(report: &Value) -> Option<String> {
    strings(report, "losses")
        .into_iter()
        .find(|l| l.contains("its effects are rasterized into the layer's pixels"))
}

/// The reopened twin's pixels against the crop of the package projection, and
/// the package's emptiness everywhere outside that rectangle.
fn assert_bake_matches(doc: &DrDocument, twin: &doctored_core::doc::Layer) {
    let projection = package(doc).flattened().expect("package projection");
    let (ox, oy) = twin.offset;
    let (tw, th) = twin.pixels.dimensions();
    assert!(
        ox >= 0 && oy >= 0 && ox as u32 + tw <= doc.width && oy as u32 + th <= doc.height,
        "the rasterized rectangle {:?} {:?} must lie inside the canvas",
        twin.offset,
        twin.pixels.dimensions()
    );
    let twin_image = twin.pixels.contiguous_image();
    for y in 0..doc.height {
        for x in 0..doc.width {
            let inside =
                x >= ox as u32 && x < ox as u32 + tw && y >= oy as u32 && y < oy as u32 + th;
            let expected = projection.get_pixel(x, y);
            if inside {
                assert_eq!(
                    twin_image.get_pixel(x - ox as u32, y - oy as u32),
                    expected,
                    "baked pixel at {x},{y}"
                );
            } else {
                assert_eq!(expected.0[3], 0, "the package is empty at {x},{y}");
            }
        }
    }
}

// -------------------------------------------------------------- export --

#[test]
fn a_styled_raster_is_rasterized_reported_and_reopens_as_plain_pixels() {
    let dir = TempDir::new().unwrap();
    let doc = styled_document(true, 1.0, 1.5);
    let report = export_report(&doc, LAYERS);
    assert_eq!(report["available"], Value::Bool(true));
    assert_eq!(report["refusal"], Value::Null);
    let loss = rasterize_loss(&report).expect("the rasterize loss");
    assert!(
        loss.contains("drop_shadow, stroke, color_overlay"),
        "{loss}"
    );
    assert!(
        loss.contains("rasterized into the layer's pixels"),
        "{loss}"
    );
    assert!(
        loss.contains("Its Blend If ranges are not written"),
        "{loss}"
    );
    assert!(loss.contains("baked into those pixels"), "{loss}");
    assert!(
        loss.contains("Layers clipped to it are written above"),
        "{loss}"
    );
    assert_eq!(
        strings(&report, "losses")
            .iter()
            .filter(|l| l.contains("layer style"))
            .count(),
        1,
        "exactly one sentence per styled layer"
    );

    let path = dir.path().join("styled.psd");
    save(&doc, &path, LAYERS).expect("save layered");
    let bytes = std::fs::read(&path).expect("read back");
    write(&dir, "styled.psd", &bytes);

    let back = import_report(&path);
    assert_eq!(back["layered_available"], Value::Bool(true));
    let reopened = open_psd(&path, LAYERS, &back);
    assert_eq!(reopened.layers.len(), 3);
    let twin = &reopened.layers[1];
    assert_eq!(twin.name, "Styled");
    assert!(twin.style.is_none(), "no style travels with the file");
    assert!(twin.mask.is_none(), "the enabled mask is inside the bake");
    assert_eq!((twin.opacity * 255.0).round() as u8, 153);
    assert_eq!(twin.blend, BlendMode::Multiply);
    assert!(reopened.layers[2].clipped, "the clipped neighbour");
    assert_bake_matches(&doc, twin);
}

#[test]
fn a_disabled_mask_is_written_beside_the_preserved_style_and_its_density_still_refuses() {
    let dir = TempDir::new().unwrap();
    let doc = styled_document(false, 1.0, 0.0);
    let report = export_report(&doc, LAYERS);
    assert_eq!(report["available"], Value::Bool(true));
    assert!(
        rasterize_loss(&report).is_none(),
        "a disabled mask shapes nothing, so nothing is baked"
    );

    let path = dir.path().join("styled-disabled.psd");
    save(&doc, &path, LAYERS).expect("save layered");
    let back = import_report(&path);
    let reopened = open_psd(&path, LAYERS, &back);
    let twin = &reopened.layers[1];
    assert!(twin.mask.is_some(), "the disabled mask is still written");
    assert!(!twin.mask_enabled);
    assert_eq!(
        twin.mask.as_ref().map(|m| m.as_raw().clone()),
        doc.layers[1].mask.as_ref().map(|m| m.as_raw().clone())
    );
    assert_eq!(
        twin.mask_properties.offset,
        doc.layers[1].mask_properties.offset
    );
    assert_eq!(
        twin.pixels.contiguous_image().as_raw(),
        doc.layers[1].pixels.contiguous_image().as_raw()
    );
    assert!(twin.style.is_some(), "the style travels with the file");

    // A disabled mask is written as a PSD mask, so its density still refuses.
    let dense = styled_document(false, 0.5, 0.0);
    let report = export_report(&dense, LAYERS);
    assert_eq!(report["available"], Value::Bool(false));
    assert!(
        report["refusal"]
            .as_str()
            .expect("refusal")
            .contains("density or feather"),
        "{}",
        report["refusal"]
    );
}

/// The preserve case: a plain enabled mask leaves nothing Photoshop cannot
/// hold, so the style is written as `lfx2`/`iOpa`/blending ranges and comes
/// back as a Doctored style.
#[test]
fn a_styled_raster_with_a_plain_mask_is_written_as_effects_and_reopens_styled() {
    let dir = TempDir::new().unwrap();
    let doc = styled_document(true, 1.0, 0.0);
    let report = export_report(&doc, LAYERS);
    assert_eq!(report["available"], Value::Bool(true), "{report}");
    assert_eq!(report["refusal"], Value::Null);
    assert!(rasterize_loss(&report).is_none(), "nothing is baked");
    let observation = strings(&report, "observations")
        .into_iter()
        .find(|o| o.contains("has a layer style"))
        .expect("the preserve observation");
    assert!(
        observation
            .contains("(drop_shadow, stroke, color_overlay, fill opacity 50 %, Blend If on gray)"),
        "{observation}"
    );
    assert!(
        observation.contains(
            "written as Photoshop layer effects and blending options (lfx2, iOpa, blending ranges)"
        ),
        "{observation}"
    );

    let path = dir.path().join("styled-preserved.psd");
    save(&doc, &path, LAYERS).expect("save layered");
    let bytes = std::fs::read(&path).expect("read back");
    write(&dir, "styled-preserved.psd", &bytes);
    for key in [b"8BIMlfx2".as_slice(), b"8BIMiOpa"] {
        assert!(
            bytes.windows(8).any(|window| window == key),
            "the {} block is written",
            String::from_utf8_lossy(key)
        );
    }

    let back = import_report(&path);
    assert_eq!(back["layered_available"], Value::Bool(true));
    let carried = strings(&back, "observations")
        .into_iter()
        .find(|o| o.contains("has Photoshop layer effects"))
        .expect("the carry observation");
    assert!(
        carried.contains(
            "(drop_shadow, stroke, color_overlay, fill opacity 50 % and Blend If ranges on gray)"
        ),
        "{carried}"
    );
    let losses = strings(&back, "verified_losses");
    assert!(
        !losses
            .iter()
            .any(|l| l.contains("layer-style metadata") || l.contains("Blend If")),
        "nothing about the carried constructs is discarded: {losses:?}"
    );

    let reopened = open_psd(&path, LAYERS, &back);
    assert_eq!(reopened.layers.len(), 3);
    let twin = &reopened.layers[1];
    assert_eq!(twin.name, "Styled");
    assert_eq!(twin.offset, doc.layers[1].offset, "its own rectangle");
    assert_eq!(
        twin.pixels.contiguous_image().as_raw(),
        doc.layers[1].pixels.contiguous_image().as_raw()
    );
    assert!(twin.mask.is_some(), "its own -2 mask channel");
    assert_eq!(
        twin.mask.as_ref().map(|m| m.as_raw().clone()),
        doc.layers[1].mask.as_ref().map(|m| m.as_raw().clone())
    );
    let expected = doc.layers[1].style.as_deref().expect("the source style");
    let style = twin.style.as_deref().expect("the carried style");
    assert_eq!(style.effects, expected.effects);
    assert_eq!(style.blend_if, expected.blend_if);
    // Fill opacity crosses in 1/255 steps (Photoshop's own `iOpa` byte).
    assert!(
        (style.fill_opacity - expected.fill_opacity).abs() <= 1.0 / 255.0,
        "{} vs {}",
        style.fill_opacity,
        expected.fill_opacity
    );
    assert_eq!(reopened.global_light, doc.global_light);
}

#[test]
fn a_styled_group_is_written_and_an_oversized_bake_refuses_without_touching_the_file() {
    let dir = TempDir::new().unwrap();
    let sentinel = b"not a psd".to_vec();

    // A folder's effects are written as its own `lfx2`, so nothing refuses.
    let (mut group, index, _, _) = DrDocument::from_pixels(RgbaImage::new(8, 8))
        .group_layers(&[0], "Set")
        .expect("group");
    group.layers[index].style = Some(Arc::new(style()));
    let report = export_report(&group, LAYERS);
    assert_eq!(report["available"], Value::Bool(true), "{report}");
    assert!(strings(&report, "observations")
        .iter()
        .any(|o| o.contains("Layer “Set” has a layer style")));
    let path = dir.path().join("styled-group.psd");
    save(&group, &path, LAYERS).expect("save layered");
    let back = import_report(&path);
    let reopened = open_psd(&path, LAYERS, &back);
    let folder = reopened
        .layers
        .iter()
        .find(|layer| layer.kind == LayerKind::Group)
        .expect("the folder");
    assert_eq!(folder.name, "Set");
    assert_eq!(
        folder.style.as_deref().map(|s| s.effects.clone()),
        Some(style().effects),
        "the folder's effects travel"
    );

    // A style Photoshop cannot hold is baked, and an oversized bake refuses.
    let mut huge = DrDocument::from_pixels(RgbaImage::new(10_000, 9_999));
    let mut overlay = LayerStyle::default();
    overlay.effects = vec![
        Effect::Stroke(Stroke {
            size: 0.5,
            ..Stroke::default()
        }),
        Effect::ColorOverlay(ColorOverlay::default()),
    ];
    huge.layers[0].style = Some(Arc::new(overlay));
    assert!(
        huge.layers[0].style.as_ref().expect("style").pad() >= 1,
        "the rasterized rectangle must grow past the limit"
    );
    let report = export_report(&huge, LAYERS);
    assert_eq!(report["available"], Value::Bool(false));
    let refusal = report["refusal"].as_str().expect("refusal");
    assert!(refusal.contains("past the 100 MP layer limit"), "{refusal}");
    let path = dir.path().join("huge-style.psd");
    std::fs::write(&path, &sentinel).expect("sentinel");
    let error = save(&huge, &path, LAYERS).expect_err("refused");
    assert_eq!(error, refusal, "the save carries its own refusal");
    assert_eq!(std::fs::read(&path).expect("still there"), sentinel);
}

#[test]
fn the_rasterize_loss_is_per_layer_and_group_clipping_is_no_longer_a_limitation() {
    // A feathered enabled mask is what still forces the bake.
    let styled = styled_document(true, 1.0, 1.5);
    assert!(rasterize_loss(&export_report(&styled, LAYERS)).is_some());

    let plain = DrDocument::from_pixels(RgbaImage::new(8, 8));
    let report = export_report(&plain, LAYERS);
    assert!(rasterize_loss(&report).is_none());
    assert!(strings(&report, "losses").is_empty(), "a plain document");
    let limitations = report["general_limitations"].to_string();
    assert!(
        !limitations.contains("group clipping"),
        "group clipping round-trips now: {limitations}"
    );
    assert!(
        limitations.contains("Layer styles are rasterized into their layer's pixels"),
        "{limitations}"
    );
}
