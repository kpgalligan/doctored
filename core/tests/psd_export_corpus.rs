//! The frozen 10B phase-2 corpus: seven documents written through the
//! production PSD writer over the C API, each committed beside a manifest that
//! states what the SOURCE document held.
//!
//! The manifest is built from the Rust document's own fields and its own
//! projection — never from the PSD reader, the decoder or any application.
//! `scripts/verify_phase10b_external.py` compares it against an independent
//! decode of the committed bytes and against GIMP 3.2.6's reopening of them.
//!
//! `DR_PSD_10B_FIXTURES=<dir>` writes `<dir>/<case>/source.psd` and
//! `manifest.json`; with the variable unset the test asserts the bytes and the
//! manifest equal the committed `tests/fixtures/psd_10b/<case>/` copies.

use std::ffi::c_char;
use std::path::{Path, PathBuf};
use std::ptr;
use std::sync::Arc;

use doctored_core::doc::{BlendMode, DrDocument, LayerKind};
use doctored_core::ffi_psd::{
    dr_doc_psd_export_report, dr_doc_save_psd, dr_precision_doc_save_psd,
};
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer as PLayer};
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use doctored_core::precision_mask::{PrecisionMask, PrecisionPlane};
use doctored_core::style::{
    BlendIf, BlendIfChannel, ColorOverlay, DropShadow, Effect, LayerStyle, Stroke, StrokePosition,
};
use image::{GrayImage, Rgba, RgbaImage};
use serde_json::{json, Map, Value};
use tempfile::TempDir;

use crate::common;
use common::{cpath, take_err_string};

const LAYERS: i32 = 0;
const FLATTENED: i32 = 1;
const LIMIT: u64 = REFERENCE_WORKING_BYTES;

// ------------------------------------------------------------- C plumbing --

/// The 8-bit save helper, the same shape `psd_export.rs` uses.
fn save(doc: &DrDocument, path: &Path, mode: i32) -> Result<(), String> {
    let c = cpath(path);
    let mut err: *mut c_char = ptr::null_mut();
    if unsafe {
        dr_doc_save_psd(
            doc,
            ptr::null(),
            c.as_ptr(),
            0,
            mode,
            std::ptr::null(),
            &mut err,
        )
    } {
        assert!(err.is_null(), "err_out set on success");
        Ok(())
    } else {
        Err(take_err_string(err))
    }
}

/// The 16-bit save helper, the same shape `psd_depth16.rs` uses.
fn save16(
    doc: &PrecisionDocument,
    projection: &PrecisionImage,
    path: &Path,
    mode: i32,
) -> Result<(), String> {
    let mut err: *mut c_char = ptr::null_mut();
    if unsafe {
        dr_precision_doc_save_psd(
            doc,
            projection,
            cpath(path).as_ptr(),
            0,
            mode,
            LIMIT,
            0,
            std::ptr::null(),
            &mut err,
        )
    } {
        assert!(err.is_null(), "err_out set on success");
        Ok(())
    } else {
        Err(take_err_string(err))
    }
}

fn export_report(doc: &DrDocument, mode: i32) -> Value {
    let mut err: *mut c_char = ptr::null_mut();
    let json = unsafe { dr_doc_psd_export_report(doc, 0, mode, &mut err) };
    assert!(!json.is_null(), "report: {}", take_err_string(err));
    let text = unsafe { std::ffi::CStr::from_ptr(json) }
        .to_string_lossy()
        .into_owned();
    unsafe { doctored_core::ffi::dr_string_free(json) };
    serde_json::from_str(&text).expect("report JSON")
}

fn export_report16(doc: &PrecisionDocument, mode: i32) -> Value {
    let mut err: *mut c_char = ptr::null_mut();
    let json = unsafe {
        doctored_core::ffi_psd::dr_precision_doc_psd_export_report(doc, 0, mode, &mut err)
    };
    assert!(!json.is_null(), "report: {}", take_err_string(err));
    serde_json::from_str(&take_err_string(json)).expect("report JSON")
}

fn pixels16(image: &PrecisionImage) -> Vec<u16> {
    let (w, h) = image.dimensions();
    let base = unsafe { doctored_core::ffi_precision::dr_precision_image_pixels_rgba16(image) };
    assert!(!base.is_null());
    unsafe { std::slice::from_raw_parts(base, w as usize * h as usize * 4) }.to_vec()
}

// --------------------------------------------------------------- fixtures --

fn opaque(w: u32, h: u32, seed: u32) -> RgbaImage {
    let mut out = RgbaImage::new(w, h);
    for y in 0..h {
        for x in 0..w {
            let i = x * 7 + y * 13 + seed;
            out.put_pixel(
                x,
                y,
                Rgba([(i % 251) as u8, (i % 241) as u8, (i % 239) as u8, 255]),
            );
        }
    }
    out
}

/// Partly transparent pixels that KEEP their colour under alpha 0.
fn hidden_rgb(w: u32, h: u32, seed: u32) -> RgbaImage {
    let mut out = RgbaImage::new(w, h);
    for y in 0..h {
        for x in 0..w {
            let a = [0u8, 1, 64, 127, 128, 200, 255][((x + y * w + seed) % 7) as usize];
            out.put_pixel(
                x,
                y,
                Rgba([17 + x as u8, 200 - y as u8, 99 + seed as u8, a]),
            );
        }
    }
    out
}

fn flat_alpha(w: u32, h: u32, alpha: u8, seed: u32) -> RgbaImage {
    let mut out = RgbaImage::new(w, h);
    for y in 0..h {
        for x in 0..w {
            let i = x * 11 + y * 5 + seed;
            out.put_pixel(
                x,
                y,
                Rgba([(i % 253) as u8, (i % 251) as u8, (i % 249) as u8, alpha]),
            );
        }
    }
    out
}

fn coverage(w: u32, h: u32, step: u32) -> GrayImage {
    let mut out = GrayImage::new(w, h);
    for (index, pixel) in out.pixels_mut().enumerate() {
        pixel.0[0] = ((index as u32 * step) % 256) as u8;
    }
    out
}

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

/// The `psd_constructs.rs::style()` shape: a drop shadow, an outside stroke, a
/// colour overlay, half fill opacity and a non-identity Blend If — all three
/// constructs this corpus case writes as `lfx2`, `iOpa` and blending ranges.
fn style() -> LayerStyle {
    let mut style = LayerStyle::default();
    style.fill_opacity = 0.5;
    style.blend_if = Some(BlendIf {
        channel: BlendIfChannel::Gray,
        this_layer: [0, 10, 240, 255],
        underlying: [0, 0, 255, 255],
        ..BlendIf::default()
    });
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

// ------------------------------------------------------ document builders --

/// Adds a raster above the whole stack. The last entry of a `doc_group`
/// forest is always a top-level entry, so this never lands inside a folder.
fn add_top(doc: DrDocument, pixels: RgbaImage, name: &str) -> DrDocument {
    let top = doc.layers.len() - 1;
    doc.adding_image_layer(top, pixels, name)
        .unwrap_or_else(|| panic!("insert {name}"))
}

fn grouped(doc: DrDocument, indices: &[usize], name: &str) -> (DrDocument, usize) {
    let (doc, index, _, _) = doc
        .group_layers(indices, name)
        .unwrap_or_else(|| panic!("group {name}"));
    (doc, index)
}

fn set_mask(
    doc: &mut DrDocument,
    index: usize,
    plane: GrayImage,
    at: (i32, i32),
    outside: u8,
    enabled: bool,
) {
    let layer = &mut doc.layers[index];
    layer.mask = Some(Arc::new(plane));
    layer.mask_enabled = enabled;
    layer.mask_properties.offset = at;
    layer.mask_properties.outside_coverage = outside;
}

fn layers_basic() -> DrDocument {
    let mut doc = DrDocument::from_pixels(opaque(8, 6, 3));
    doc.layers[0].name = "Base".into();
    let doc = add_top(doc, flat_alpha(5, 4, 255, 17), "Signed");
    let doc = add_top(doc, opaque(2, 2, 41), "Offcanvas");
    let name: String = "レイヤー名前テスト・".repeat(20);
    let mut doc = add_top(doc, hidden_rgb(4, 3, 0), &name);
    doc.layers[1].offset = (-2, -1);
    doc.layers[1].opacity = 128.0 / 255.0;
    doc.layers[2].offset = (9, -3);
    doc.layers[2].visible = false;
    doc.layers[3].offset = (3, 2);
    doc
}

fn groups_masks() -> DrDocument {
    let mut doc = DrDocument::from_pixels(opaque(8, 6, 7));
    doc.layers[0].name = "Base".into();
    let doc = add_top(doc, flat_alpha(8, 6, 193, 23), "Multiply child");
    let doc = add_top(doc, flat_alpha(8, 6, 137, 61), "Screen child");
    let (doc, inner) = grouped(doc, &[2], "Inner");
    let (doc, isolated) = grouped(doc, &[1, inner], "Isolated");
    let doc = add_top(doc, flat_alpha(6, 4, 211, 97), "Masked");
    let doc = add_top(doc, flat_alpha(4, 3, 255, 131), "Disabled");
    let (doc, pass) = grouped(doc, &[isolated + 1, isolated + 2], "Pass");
    let doc = add_top(doc, RgbaImage::new(1, 1), "Placeholder");
    let (doc, empty) = grouped(doc, &[pass + 1], "Empty");
    let mut doc = doc.removing_layer(empty - 1).expect("drop the placeholder");

    doc.resolution = Resolution { x: 300.0, y: 240.0 };
    doc.layers[1].blend = BlendMode::Multiply;
    doc.layers[2].blend = BlendMode::Screen;
    doc.layers[3].blend = BlendMode::Normal; // "Inner", opacity 255.
    doc.layers[3].open = false;
    doc.layers[4].blend = BlendMode::Normal;
    doc.layers[4].opacity = 128.0 / 255.0;
    doc.layers[4].open = false;
    set_mask(&mut doc, 4, coverage(8, 6, 11), (0, 0), 0, true);
    doc.layers[5].offset = (1, 1);
    set_mask(&mut doc, 5, coverage(4, 3, 29), (2, 2), 255, true);
    doc.layers[6].offset = (3, 2);
    set_mask(&mut doc, 6, coverage(4, 3, 53), (3, 2), 0, false);
    doc.layers[7].blend = BlendMode::PassThrough;
    doc.layers[7].opacity = 128.0 / 255.0;
    doc.layers[7].open = true;
    set_mask(&mut doc, 7, coverage(8, 6, 17), (0, 0), 0, true);
    assert_eq!(
        doc.layers
            .iter()
            .map(|l| (l.name.as_str(), l.depth, l.kind == LayerKind::Group))
            .collect::<Vec<_>>(),
        vec![
            ("Base", 0, false),
            ("Multiply child", 1, false),
            ("Screen child", 2, false),
            ("Inner", 1, true),
            ("Isolated", 0, true),
            ("Masked", 1, false),
            ("Disabled", 1, false),
            ("Pass", 0, true),
            ("Empty", 0, true),
        ]
    );
    doc
}

fn p3_ppi() -> DrDocument {
    let mut pixels = RgbaImage::new(4, 3);
    for (index, pixel) in pixels.pixels_mut().enumerate() {
        *pixel = if index % 2 == 0 {
            Rgba([211, 93, 17, 255])
        } else {
            Rgba([39, 187, 233, 255])
        };
    }
    let mut doc = DrDocument::from_pixels(pixels);
    doc.layers[0].name = "Display P3 source".into();
    doc.profile = IccProfile::display_p3();
    doc.resolution = Resolution { x: 300.0, y: 240.0 };
    doc
}

fn clipping() -> DrDocument {
    let mut doc = DrDocument::from_pixels(opaque(8, 6, 5));
    doc.layers[0].name = "Backdrop".into();
    let doc = add_top(doc, opaque(4, 4, 67), "Base");
    let doc = add_top(doc, hidden_rgb(6, 4, 2), "Clip A");
    let doc = add_top(doc, flat_alpha(3, 3, 199, 71), "Clip B");
    let doc = add_top(doc, opaque(4, 4, 151), "Base2");
    let doc = add_top(doc, hidden_rgb(8, 6, 4), "In folder");
    let (mut doc, folder) = grouped(doc, &[5], "Clipped folder");
    assert_eq!(folder, 6);
    doc.layers[1].offset = (1, 1);
    doc.layers[2].offset = (0, 0);
    doc.layers[2].clipped = true;
    doc.layers[3].offset = (3, 3);
    doc.layers[3].clipped = true;
    doc.layers[4].offset = (3, 1);
    doc.layers[6].blend = BlendMode::Normal;
    doc.layers[6].clipped = true;
    doc
}

fn styled_effects() -> DrDocument {
    let doc = DrDocument::from_pixels(opaque(32, 32, 9));
    let mut doc = add_top(doc, checker(12, 12), "Styled");
    doc.layers[0].name = "Base".into();
    let styled = &mut doc.layers[1];
    styled.offset = (10, 10);
    styled.opacity = 153.0 / 255.0;
    styled.blend = BlendMode::Multiply;
    styled.style = Some(Arc::new(style()));
    let mut mask = GrayImage::new(12, 12);
    for (index, pixel) in mask.pixels_mut().enumerate() {
        pixel.0[0] = (index * 3 % 256) as u8;
    }
    styled.mask = Some(Arc::new(mask));
    styled.mask_enabled = true;
    styled.mask_properties.offset = (10, 10);
    doc
}

struct Typed {
    document: PrecisionDocument,
    base: Vec<u16>,
    multiply: Vec<u16>,
    multiply_mask: Vec<u16>,
    hidden: Vec<u16>,
    hidden_mask: Vec<u16>,
    hidden_name: String,
}

fn ramp16(w: usize, h: usize, seed: u16) -> Vec<[u16; 4]> {
    (0..w * h)
        .map(|i| {
            let v = i as u16;
            [
                v.wrapping_mul(1013).wrapping_add(seed),
                v.wrapping_mul(3571).wrapping_add(seed.wrapping_mul(7)),
                v.wrapping_mul(7919).wrapping_add(seed.wrapping_mul(13)),
                65535,
            ]
        })
        .collect()
}

fn hidden16(w: usize, h: usize) -> Vec<[u16; 4]> {
    (0..w * h)
        .map(|i| {
            let alpha = [0u16, 1, 16384, 32767, 32768, 60000, 65535][i % 7];
            [4097 + i as u16, 51234 - i as u16, 60000, alpha]
        })
        .collect()
}

fn coverage16(count: usize, step: u16) -> Vec<u16> {
    (0..count).map(|i| (i as u16).wrapping_mul(step)).collect()
}

fn flatten16(pixels: &[[u16; 4]]) -> Vec<u16> {
    pixels.iter().flat_map(|p| p.iter().copied()).collect()
}

fn depth16() -> Typed {
    let profile = IccProfile::display_p3();
    let base = ramp16(4, 3, 77);
    let multiply = hidden16(3, 2);
    let hidden = hidden16(2, 2);
    let multiply_mask = coverage16(6, 9001);
    let hidden_mask = coverage16(4, 21001);
    let hidden_name = "n".repeat(360);
    let image = |w: u32, h: u32, pixels: Vec<[u16; 4]>| {
        PrecisionImage::from_rgba16(w, h, pixels, Arc::clone(&profile), None).unwrap()
    };
    let mask = |w: u32, h: u32, samples: Vec<u16>, at: (i32, i32), enabled: bool| {
        let mut properties = doctored_core::doc_mask::MaskProperties::at(at);
        properties.linked = false;
        properties.outside_coverage = 255;
        PrecisionMask::new(
            PrecisionPlane::from_u16(w, h, samples, None).unwrap(),
            properties,
            enabled,
        )
        .unwrap()
    };
    let multiply_layer = PLayer::raster(
        "Multiply",
        image(3, 2, multiply.clone()),
        (1, 0),
        128.0 / 255.0,
        true,
    )
    .unwrap()
    .with_blend_mode(BlendMode::Multiply)
    .unwrap()
    .with_mask(Some(mask(3, 2, multiply_mask.clone(), (1, 0), true)));
    let hidden_layer = PLayer::raster(
        &hidden_name,
        image(2, 2, hidden.clone()),
        (-1, 1),
        1.0,
        false,
    )
    .unwrap()
    .with_mask(Some(mask(2, 2, hidden_mask.clone(), (-1, 1), false)));
    let group =
        PLayer::pass_through("Group", vec![multiply_layer, hidden_layer], 1.0, true).unwrap();
    let document = PrecisionDocument::from_parts(
        4,
        3,
        Arc::clone(&profile),
        Metadata::default(),
        Resolution { x: 300.0, y: 240.0 },
        vec![
            PLayer::raster("Base", image(4, 3, base.clone()), (0, 0), 1.0, true).unwrap(),
            group,
        ],
    )
    .unwrap();
    Typed {
        document,
        base: flatten16(&base),
        multiply: flatten16(&multiply),
        multiply_mask,
        hidden: flatten16(&hidden),
        hidden_mask,
        hidden_name,
    }
}

// ---------------------------------------------------------------- manifest --

fn blend_key(blend: BlendMode) -> &'static str {
    match blend {
        BlendMode::Normal => "norm",
        BlendMode::Multiply => "mul ",
        BlendMode::Screen => "scrn",
        BlendMode::PassThrough => "pass",
        other => panic!("this corpus declares only qualified keys, not {other:?}"),
    }
}

fn opacity_byte(opacity: f32) -> u8 {
    (opacity.clamp(0.0, 1.0) * 255.0).round() as u8
}

fn mask_row(plane: &GrayImage, at: (i32, i32), outside: u8, enabled: bool) -> Value {
    let (w, h) = plane.dimensions();
    json!({"size": [w, h], "offset": [at.0, at.1], "coverage": plane.as_raw(),
           "outside_coverage": outside, "enabled": enabled})
}

/// One manifest row per 8-bit entry, bottom-first with children before their
/// folder — the order `doc.layers` already holds and the decoder reproduces.
fn rows8(doc: &DrDocument, baked: &[&str]) -> Vec<Value> {
    doc.layers
        .iter()
        .map(|layer| {
            let group = layer.kind == LayerKind::Group;
            let bake = baked.contains(&layer.name.as_str());
            let (w, h) = layer.pixels.dimensions();
            let mask = match (&layer.mask, bake) {
                (Some(plane), false) => mask_row(
                    plane,
                    layer.mask_properties.offset,
                    layer.mask_properties.outside_coverage,
                    layer.mask_enabled,
                ),
                _ => Value::Null,
            };
            let mut row = json!({
                "name": layer.name,
                "kind": if group { "group" } else { "raster" },
                "depth": layer.depth,
                "visible": layer.visible,
                "opacity_byte": opacity_byte(layer.opacity),
                "blend": blend_key(layer.blend),
                "clipped": layer.clipped,
                "open": layer.open,
                "bake": bake,
                // A PSD folder record carries empty bounds and no pixels.
                "offset": if group { [0, 0] } else { [layer.offset.0, layer.offset.1] },
                "size": if group { [0, 0] } else { [w as i32, h as i32] },
                // A baked row's pixels are not the written ones; gate 1 reads the
                // decoded record instead.
                "rgba": if group || bake { Value::Null } else { json!(layer.pixels.contiguous_image().as_raw()) },
                "mask": mask,
            });
            // The style the record carries as Photoshop layer effects and
            // blending options; absent from a row that has none.
            if let Some(style) = layer.style.as_deref() {
                row["style"] = json!(style.to_json());
            }
            row
        })
        .collect()
}

fn rows16(layers: &[PLayer], depth: u16, out: &mut Vec<Value>) {
    for layer in layers {
        if let Some(children) = layer.children() {
            rows16(children, depth + 1, out);
        }
        let group = layer.children().is_some();
        let (w, h) = layer.source().map_or((0, 0), |image| image.dimensions());
        let mask = match layer.mask() {
            Some(mask) => {
                let (mw, mh) = mask.plane().dimensions();
                let (mx, my) = mask.properties().offset;
                json!({"size": [mw, mh], "offset": [mx, my],
                       "coverage": mask.plane().samples(),
                       "outside_coverage": mask.properties().outside_coverage,
                       "enabled": mask.enabled()})
            }
            None => Value::Null,
        };
        out.push(json!({
            "name": layer.name(),
            "kind": if group { "group" } else { "raster" },
            "depth": depth,
            "visible": layer.visible(),
            "opacity_byte": opacity_byte(layer.opacity()),
            "blend": if group && layer.is_pass_through() { "pass" } else { blend_key(layer.blend_mode()) },
            "clipped": false,
            // The 16-bit writer has no open/closed state and always writes an
            // OPEN folder (`psd_export16.rs`); rasters ignore the field.
            "open": true,
            "bake": false,
            "offset": if group { [0, 0] } else { [layer.offset().0, layer.offset().1] },
            "size": if group { [0, 0] } else { [w as i32, h as i32] },
            "rgba": if group { Value::Null } else { json!(layer.source().unwrap().to_rgba16().ok().unwrap().iter().flat_map(|p| p.iter().copied()).collect::<Vec<u16>>()) },
            "mask": mask,
        }));
    }
}

fn report_summary(report: &Value) -> Value {
    json!({"losses": report["losses"], "observations": report["observations"]})
}

#[allow(clippy::too_many_arguments)]
fn manifest(
    name: &str,
    depth: u32,
    mode: &str,
    canvas: [u32; 2],
    resolution: Resolution,
    profile: &IccProfile,
    appearance_qualified: bool,
    bake: bool,
    report: &Value,
    layers: Vec<Value>,
    render: Value,
) -> Value {
    let mut map = Map::new();
    map.insert("schema".into(), json!(1));
    map.insert("name".into(), json!(name));
    map.insert("depth".into(), json!(depth));
    map.insert("mode".into(), json!(mode));
    map.insert("canvas".into(), json!(canvas));
    map.insert(
        "resolution".into(),
        json!([resolution.x as f64, resolution.y as f64]),
    );
    map.insert(
        "profile_sha256".into(),
        json!(common::sha256::sha256_hex(profile.bytes())),
    );
    map.insert("appearance_qualified".into(), json!(appearance_qualified));
    map.insert("bake".into(), json!(bake));
    map.insert("export_report".into(), report_summary(report));
    map.insert("layers".into(), Value::Array(layers));
    map.insert("render".into(), render);
    Value::Object(map)
}

// ----------------------------------------------------------------- emit --

fn fixture_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/psd_10b")
}

fn emit(name: &str, bytes: &[u8], manifest: &Value) {
    let text = serde_json::to_string(manifest).expect("manifest JSON") + "\n";
    if let Ok(directory) = std::env::var("DR_PSD_10B_FIXTURES") {
        let folder = Path::new(&directory).join(name);
        std::fs::create_dir_all(&folder).expect("fixture folder");
        std::fs::write(folder.join("source.psd"), bytes).expect("write source.psd");
        std::fs::write(folder.join("manifest.json"), &text).expect("write manifest.json");
        return;
    }
    let folder = fixture_root().join(name);
    let committed = std::fs::read(folder.join("source.psd"))
        .unwrap_or_else(|e| panic!("{name}/source.psd: {e}"));
    assert!(
        bytes == committed.as_slice(),
        "{name}/source.psd changed: wrote {} bytes, committed {}",
        bytes.len(),
        committed.len()
    );
    let expected = std::fs::read_to_string(folder.join("manifest.json"))
        .unwrap_or_else(|e| panic!("{name}/manifest.json: {e}"));
    assert!(text == expected, "{name}/manifest.json changed");
}

fn write_case(
    directory: &TempDir,
    name: &str,
    doc: &DrDocument,
    mode: i32,
    appearance_qualified: bool,
    baked: &[&str],
) {
    let path = directory.path().join(format!("{name}.psd"));
    save(doc, &path, mode).unwrap_or_else(|e| panic!("save {name}: {e}"));
    let bytes = std::fs::read(&path).expect("read back");
    let report = export_report(doc, mode);
    assert_eq!(report["available"], Value::Bool(true), "{name}: {report}");
    let projection = doc.flattened().expect("projection");
    let layers = if mode == FLATTENED {
        vec![json!({
            "name": "Composite", "kind": "raster", "depth": 0, "visible": true,
            "opacity_byte": 255, "blend": "norm", "clipped": false, "open": true,
            "bake": false, "offset": [0, 0], "size": [doc.width as i32, doc.height as i32],
            "rgba": projection.as_raw(), "mask": Value::Null,
        })]
    } else {
        rows8(doc, baked)
    };
    let manifest = manifest(
        name,
        8,
        if mode == FLATTENED {
            "flattened"
        } else {
            "layers"
        },
        [doc.width, doc.height],
        doc.resolution,
        &doc.profile,
        appearance_qualified,
        !baked.is_empty() && mode == LAYERS,
        &report,
        layers,
        json!(projection.as_raw()),
    );
    emit(name, &bytes, &manifest);
}

#[test]
fn the_ten_b_corpus_matches_its_committed_sources_and_manifests() {
    let directory = TempDir::new().expect("tempdir");

    write_case(
        &directory,
        "layers_basic",
        &layers_basic(),
        LAYERS,
        true,
        &[],
    );
    let groups = groups_masks();
    write_case(&directory, "groups_masks", &groups, LAYERS, true, &[]);
    write_case(&directory, "flattened", &groups, FLATTENED, true, &[]);
    write_case(&directory, "p3_ppi", &p3_ppi(), LAYERS, true, &[]);
    write_case(&directory, "clipping", &clipping(), LAYERS, false, &[]);
    write_case(
        &directory,
        "styled_effects",
        &styled_effects(),
        LAYERS,
        false,
        &[],
    );

    let typed = depth16();
    let projection = typed.document.render(LIMIT, 0, None).expect("projection");
    let path = directory.path().join("depth16.psd");
    save16(&typed.document, &projection, &path, LAYERS).expect("save depth16");
    let bytes = std::fs::read(&path).expect("read back");
    let report = export_report16(&typed.document, LAYERS);
    assert_eq!(report["depth"], Value::from(16));
    assert_eq!(report["available"], Value::Bool(true));
    let mut layers = Vec::new();
    rows16(typed.document.root_layers(), 0, &mut layers);
    assert_eq!(layers.len(), 4);
    assert_eq!(layers[0]["name"], json!("Base"));
    assert_eq!(layers[0]["rgba"], json!(typed.base));
    assert_eq!(layers[1]["rgba"], json!(typed.multiply));
    assert_eq!(layers[1]["mask"]["coverage"], json!(typed.multiply_mask));
    assert_eq!(layers[2]["name"], json!(typed.hidden_name));
    assert_eq!(layers[2]["rgba"], json!(typed.hidden));
    assert_eq!(layers[2]["mask"]["coverage"], json!(typed.hidden_mask));
    assert_eq!(layers[3]["rgba"], Value::Null);
    let manifest = manifest(
        "depth16",
        16,
        "layers",
        [4, 3],
        Resolution { x: 300.0, y: 240.0 },
        &IccProfile::display_p3(),
        true,
        false,
        &report,
        layers,
        json!(pixels16(&projection)),
    );
    emit("depth16", &bytes, &manifest);
}
