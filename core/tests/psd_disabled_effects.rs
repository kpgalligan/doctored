//! A layer style whose effects are ALL disabled writes no `lfx2`, on both
//! depths.
//!
//! `lfx2` is the effects descriptor, and a descriptor holding nothing but
//! disabled effects reopens as a bare marker — Photoshop 26.10.0 itself
//! answers such a block with no `layerEffects` key at all (Run 3 of the
//! Phase 10 closeout). Fill opacity (`iOpa`) and Blend If (the record's
//! 40-byte blending ranges) are their OWN constructs, so they still travel,
//! and image resources 1037/1049 (the file's global light) follow the `lfx2`
//! rather than the style. The four decision sites therefore ask
//! `LayerStyle::has_enabled_effects()`, not `!effects.is_empty()`.
//!
//! This is its own file because `psd_layer_effects.rs` is already past the
//! ~1500-line ceiling. Every byte read here comes from this suite's own walk
//! of the written file, never from the crate's reader.

use std::ffi::{c_char, CString};
use std::path::Path;
use std::ptr;
use std::sync::Arc;

use doctored_core::doc::DrDocument;
use doctored_core::ffi_psd::{
    dr_doc_open_psd, dr_doc_psd_export_report, dr_doc_save_psd, dr_precision_doc_open_psd,
    dr_precision_doc_psd_export_report, dr_precision_doc_save_psd, dr_psd_import_report,
};
use doctored_core::icc::IccProfile;
use doctored_core::job_control::DrJobControl;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer as PLayer};
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use doctored_core::style::{BlendIf, BlendIfChannel, LayerStyle};
use image::{Rgba, RgbaImage};
use serde_json::Value;
use tempfile::TempDir;

use crate::common;
use common::{cpath, take_err_string};

const LAYERS: i32 = 0;
const LIMIT: u64 = REFERENCE_WORKING_BYTES;

/// Fill opacity 0.5, a red Blend If, and two effects that are switched OFF.
/// Nothing here renders: the whole point is that the style is still not
/// identity, so it is preserved, while its effects descriptor is not written.
const DISABLED_JSON: &str = r##"{
  "fill_opacity": 0.5,
  "blend_if": {"channel":"red","this_layer":[0,64,192,255],"underlying":[32,96,160,224]},
  "effects": [
    {"type":"drop_shadow","enabled":false,"blend":"multiply","color":"#112233","opacity":0.5,
     "angle":45,"use_global_light":false,"distance":6,"spread":0.25,"size":9,
     "layer_knocks_out":false},
    {"type":"color_overlay","enabled":false,"blend":"soft_light","color":"#00ff88","opacity":0.7}
  ]
}"##;

/// The same style a typed layer carries: its Blend If lives on the LAYER, so
/// the style itself holds only the fill opacity and the disabled effects.
const DISABLED_TYPED_JSON: &str = r##"{
  "fill_opacity": 0.5,
  "effects": [
    {"type":"drop_shadow","enabled":false,"blend":"multiply","color":"#112233","opacity":0.5,
     "angle":45,"use_global_light":false,"distance":6,"spread":0.25,"size":9,
     "layer_knocks_out":false},
    {"type":"color_overlay","enabled":false,"blend":"soft_light","color":"#00ff88","opacity":0.7}
  ]
}"##;

const LIGHT: (f32, f32) = (137.0, 52.0);

// ------------------------------------------------------------- C plumbing --

fn import_report(path: &Path) -> Value {
    let mut error = ptr::null_mut();
    let raw = unsafe { dr_psd_import_report(cpath(path).as_ptr(), &mut error) };
    assert!(!raw.is_null(), "{}", take_err_string(error));
    serde_json::from_str(&take_err_string(raw)).expect("import report JSON")
}

fn open_psd(path: &Path, report: &Value) -> Box<DrDocument> {
    let token = CString::new(report["source_fingerprint"].as_str().unwrap()).unwrap();
    let mut error = ptr::null_mut();
    let raw = unsafe { dr_doc_open_psd(cpath(path).as_ptr(), LAYERS, token.as_ptr(), &mut error) };
    assert!(!raw.is_null(), "open_psd: {}", take_err_string(error));
    unsafe { Box::from_raw(raw) }
}

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

fn open_typed(path: &Path, report: &Value) -> Owned {
    let token = CString::new(report["source_fingerprint"].as_str().unwrap()).unwrap();
    let mut error = ptr::null_mut();
    let raw = unsafe {
        dr_precision_doc_open_psd(
            cpath(path).as_ptr(),
            LAYERS,
            token.as_ptr(),
            LIMIT,
            0,
            ptr::null::<DrJobControl>(),
            &mut error,
        )
    };
    assert!(!raw.is_null(), "typed open: {}", take_err_string(error));
    Owned(raw)
}

fn export_report(doc: &DrDocument) -> Value {
    let mut error: *mut c_char = ptr::null_mut();
    let json = unsafe { dr_doc_psd_export_report(doc, 0, LAYERS, &mut error) };
    assert!(!json.is_null(), "export report: {}", take_err_string(error));
    serde_json::from_str(&take_err_string(json)).expect("export report JSON")
}

fn export_report16(doc: &PrecisionDocument) -> Value {
    let mut error: *mut c_char = ptr::null_mut();
    let json = unsafe { dr_precision_doc_psd_export_report(doc, 0, LAYERS, &mut error) };
    assert!(!json.is_null(), "export report: {}", take_err_string(error));
    serde_json::from_str(&take_err_string(json)).expect("export report JSON")
}

fn save(doc: &DrDocument, path: &Path) -> Result<(), String> {
    let c = cpath(path);
    let mut error: *mut c_char = ptr::null_mut();
    if unsafe { dr_doc_save_psd(doc, ptr::null(), c.as_ptr(), 0, LAYERS, &mut error) } {
        Ok(())
    } else {
        Err(take_err_string(error))
    }
}

fn save16(doc: &PrecisionDocument, projection: &PrecisionImage, path: &Path) -> Result<(), String> {
    let mut error: *mut c_char = ptr::null_mut();
    if unsafe {
        dr_precision_doc_save_psd(
            doc,
            projection,
            cpath(path).as_ptr(),
            0,
            LAYERS,
            LIMIT,
            0,
            &mut error,
        )
    } {
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

// ----------------------------------------------------------- byte reading --

/// Additional-layer-information block `key`'s body, read out of the written
/// file by this suite's own walk.
fn block_body(bytes: &[u8], key: &[u8; 4]) -> Option<Vec<u8>> {
    let mut marker = b"8BIM".to_vec();
    marker.extend(key);
    let at = bytes
        .windows(8)
        .position(|window| window == marker.as_slice())?;
    let len = u32::from_be_bytes(bytes[at + 8..at + 12].try_into().unwrap()) as usize;
    Some(bytes[at + 12..at + 12 + len].to_vec())
}

/// Image resource `id`'s body, read out of the written file's resource block.
fn resource_body(bytes: &[u8], id: u16) -> Option<Vec<u8>> {
    let mut marker = b"8BIM".to_vec();
    marker.extend(id.to_be_bytes());
    marker.extend([0, 0]);
    let at = bytes
        .windows(8)
        .position(|window| window == marker.as_slice())?;
    let len = u32::from_be_bytes(bytes[at + 8..at + 12].try_into().unwrap()) as usize;
    Some(bytes[at + 12..at + 12 + len].to_vec())
}

/// The 40-byte blending-ranges section a red Blend If must produce: the red
/// slot (index 1) holds the two authored ramps, every other slot the
/// unrestricted default.
fn expected_ranges() -> Vec<u8> {
    let mut ranges = [0u8; 40];
    for slot in ranges.chunks_exact_mut(4) {
        slot.copy_from_slice(&[0, 0, 255, 255]);
    }
    ranges[8..12].copy_from_slice(&[0, 64, 192, 255]);
    ranges[12..16].copy_from_slice(&[32, 96, 160, 224]);
    let mut section = 40u32.to_be_bytes().to_vec();
    section.extend(ranges);
    section
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

/// What the WRITTEN file must not contain, whatever wrote it: no effects
/// descriptor and no global-light resources, because image resources
/// 1037/1049 are written iff at least one record writes an `lfx2` (the
/// preflight's own `writes_effects` / `preserves_a_style` rule).
fn assert_no_effects_written(bytes: &[u8]) {
    assert!(
        block_body(bytes, b"lfx2").is_none(),
        "an all-disabled style writes no lfx2"
    );
    for id in [1037u16, 1049] {
        assert!(
            resource_body(bytes, id).is_none(),
            "image resource {id} follows the lfx2, not the style"
        );
    }
    assert_eq!(
        block_body(bytes, b"iOpa").expect("an iOpa block"),
        [128, 0, 0, 0],
        "fill opacity is its own construct; 0.5 is one byte, 128"
    );
    let section = expected_ranges();
    assert_eq!(
        bytes
            .windows(section.len())
            .filter(|window| *window == section.as_slice())
            .count(),
        1,
        "exactly one record carries the red blending ranges"
    );
}

/// The style that must come BACK from such a file: the fill opacity as its
/// `iOpa` byte (128/255, which is what a one-byte construct can hold — 0.5 is
/// not representable in it), the Blend If exactly, and NO effect, because the
/// disabled ones were never written.
fn expect_reopened(style: &str) {
    let value: Value = serde_json::from_str(style).expect("style JSON");
    assert_eq!(value["fill_opacity"], serde_json::json!(0.502), "{style}");
    assert_eq!(
        value["blend_if"],
        serde_json::json!({"channel":"red","this_layer":[0,64,192,255],
                           "underlying":[32,96,160,224]}),
        "{style}"
    );
    assert_eq!(
        value["effects"],
        serde_json::json!([]),
        "no effect comes back: {style}"
    );
}

// ------------------------------------------------------------- 8 bits --

#[test]
fn an_all_disabled_style_writes_no_lfx2_at_eight_bits() {
    let dir = TempDir::new().unwrap();
    let doc = DrDocument::from_pixels(checker(24, 20))
        .adding_image_layer(0, checker(8, 6), "Styled")
        .expect("styled layer")
        .set_global_light(doctored_core::style::GlobalLight {
            angle: LIGHT.0,
            altitude: LIGHT.1,
        })
        .expect("the nondefault light");
    let mut doc = doc;
    doc.layers[0].name = "Base".into();
    doc.layers[1].offset = (4, 3);
    doc.layers[1].style = Some(Arc::new(
        LayerStyle::from_json(DISABLED_JSON).expect("the disabled style"),
    ));

    let report = export_report(&doc);
    assert_eq!(report["available"], Value::Bool(true), "{report}");
    assert!(
        !strings(&report, "losses")
            .iter()
            .any(|l| l.contains("rasterized")),
        "nothing is baked: {:?}",
        strings(&report, "losses")
    );
    let observation = strings(&report, "observations")
        .into_iter()
        .find(|o| o.contains("has a layer style"))
        .expect("the preserve observation");
    assert!(
        observation.contains("(fill opacity 50 %, Blend If on red)"),
        "the observation names no effect: {observation}"
    );

    let path = dir.path().join("disabled8.psd");
    save(&doc, &path).expect("save layered");
    let bytes = std::fs::read(&path).expect("read back");
    assert!(
        report["worst_case_bytes"].as_u64().unwrap() >= bytes.len() as u64,
        "the forecast covers the blocks it wrote"
    );
    assert_no_effects_written(&bytes);

    let back = import_report(&path);
    let reopened = open_psd(&path, &back);
    expect_reopened(
        &reopened.layers[1]
            .style
            .as_deref()
            .expect("the reopened style")
            .to_json(),
    );
}

// ------------------------------------------------------------- 16 bits --

#[test]
fn an_all_disabled_style_writes_no_lfx2_at_sixteen_bits() {
    let dir = TempDir::new().unwrap();
    let profile = IccProfile::srgb();
    let pixels: Vec<[u16; 4]> = (0..48).map(|i| [i * 700, 20000, 30000, 65535]).collect();
    let image = PrecisionImage::from_rgba16(8, 6, pixels, Arc::clone(&profile), None).unwrap();
    let style = LayerStyle::from_json(DISABLED_TYPED_JSON).expect("the disabled style");
    let styled = PLayer::raster("Styled", image, (4, 3), 1.0, true)
        .unwrap()
        .with_layer_style(Some(&style), (24, 20))
        .expect("the typed style")
        .with_blend_if(Some(BlendIf {
            channel: BlendIfChannel::Red,
            this_layer: [0, 64, 192, 255],
            underlying: [32, 96, 160, 224],
            ..BlendIf::default()
        }))
        .expect("the typed Blend If");
    let base = PLayer::raster(
        "Base",
        PrecisionImage::from_rgba16(
            24,
            20,
            vec![[10000u16, 20000, 30000, 65535]; 480],
            Arc::clone(&profile),
            None,
        )
        .unwrap(),
        (0, 0),
        1.0,
        true,
    )
    .unwrap();
    let document = PrecisionDocument::from_parts(
        24,
        20,
        Arc::clone(&profile),
        Metadata::default(),
        Resolution::default(),
        vec![base, styled],
    )
    .unwrap();

    let report = export_report16(&document);
    assert_eq!(report["available"], Value::Bool(true), "{report}");
    let observation = strings(&report, "observations")
        .into_iter()
        .find(|o| o.contains("has a layer style"))
        .expect("the preserve observation");
    assert!(
        observation.contains("(fill opacity 50 %, Blend If on red)"),
        "the observation names no effect: {observation}"
    );

    let projection = document.render(LIMIT, 0, None).expect("projection");
    let path = dir.path().join("disabled16.psd");
    save16(&document, &projection, &path).expect("save layered");
    let bytes = std::fs::read(&path).expect("read back");
    assert_no_effects_written(&bytes);

    let back = import_report(&path);
    let reopened = open_typed(&path, &back);
    expect_reopened(
        &reopened
            .layer(1)
            .unwrap()
            .reported_style()
            .expect("the reopened style"),
    );
}
