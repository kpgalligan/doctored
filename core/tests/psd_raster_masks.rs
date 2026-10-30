//! Independent ordinary-mask PSD fixtures through report/open/edit/render/native FFI.
//! DR_PSD_MASK_FIXTURES exports authored PSDs and independently expected manifests.
use crate::common;
use crate::identity;
use crate::psd16_fixture;
use crate::psd_mask_fixture as fixture;
use common::{cpath, flat_pixels, take_err_string};
use doctored_core::doc::{DrDocument, LayerKind};
use doctored_core::ffi_doc::{
    dr_doc_open, dr_doc_save_native, dr_doc_with_layer_mask_enabled, dr_doc_with_layer_offset,
};
use doctored_core::ffi_mask::{dr_doc_layer_mask_properties, dr_doc_layer_masks_equal};
use doctored_core::ffi_psd::{dr_doc_open_psd, dr_psd_import_report};
use fixture::*;
use serde_json::{json, Value};
use std::{ffi::CString, path::Path, ptr};
use tempfile::TempDir;

fn report(path: &Path) -> Value {
    let mut error = ptr::null_mut();
    let value = unsafe { dr_psd_import_report(cpath(path).as_ptr(), &mut error) };
    assert!(!value.is_null(), "{}", take_err_string(error));
    serde_json::from_str(&take_err_string(value)).unwrap()
}
fn open(path: &Path, mode: i32, notice: &Value) -> Result<Box<DrDocument>, String> {
    let fingerprint = CString::new(notice["source_fingerprint"].as_str().unwrap()).unwrap();
    let mut error = ptr::null_mut();
    let doc =
        unsafe { dr_doc_open_psd(cpath(path).as_ptr(), mode, fingerprint.as_ptr(), &mut error) };
    if doc.is_null() {
        Err(take_err_string(error))
    } else {
        assert!(error.is_null());
        Ok(unsafe { Box::from_raw(doc) })
    }
}
fn save(doc: &DrDocument, path: &Path) -> Vec<u8> {
    let mut error = ptr::null_mut();
    assert!(
        unsafe { dr_doc_save_native(doc, cpath(path).as_ptr(), &mut error) },
        "{}",
        take_err_string(error)
    );
    std::fs::read(path).unwrap()
}
fn reopen(path: &Path) -> Box<DrDocument> {
    let mut error = ptr::null_mut();
    let doc = unsafe { dr_doc_open(cpath(path).as_ptr(), &mut error) };
    assert!(!doc.is_null(), "{}", take_err_string(error));
    unsafe { Box::from_raw(doc) }
}
fn numbers(v: &Value) -> Vec<u8> {
    v.as_array()
        .unwrap()
        .iter()
        .map(|n| n.as_u64().unwrap() as u8)
        .collect()
}
fn check_layers(doc: &DrDocument, expected: &Value) {
    let layers = expected.as_array().unwrap();
    assert_eq!(doc.layers.len(), layers.len());
    for (index, (layer, e)) in doc.layers.iter().zip(layers).enumerate() {
        assert_eq!(layer.name, e["name"].as_str().unwrap());
        assert_eq!(layer.kind == LayerKind::Group, e["kind"] == "group");
        assert_eq!(layer.depth, e["depth"].as_u64().unwrap() as u16);
        assert_eq!(
            layer.offset,
            (
                e["offset"][0].as_i64().unwrap() as i32,
                e["offset"][1].as_i64().unwrap() as i32
            )
        );
        assert_eq!(
            layer.pixels.dimensions(),
            (
                e["size"][0].as_u64().unwrap() as u32,
                e["size"][1].as_u64().unwrap() as u32
            )
        );
        assert_eq!(
            layer.pixels.contiguous_image().as_raw(),
            &numbers(&e["rgba"])
        );
        assert_eq!(layer.visible, e["visible"].as_bool().unwrap());
        assert_eq!(layer.opacity, e["opacity"].as_f64().unwrap() as f32);
        assert_eq!(layer.open, e["open"].as_bool().unwrap());
        assert_eq!(
            layer.blend.to_c(),
            match e["blend"].as_str().unwrap() {
                "Pass Through" => 27,
                "Multiply" => 1,
                _ => 0,
            }
        );
        if e["mask"].is_null() {
            assert!(layer.mask.is_none());
            continue;
        }
        let m = &e["mask"];
        let raw = layer.mask.as_ref().unwrap();
        assert_eq!(raw.as_raw(), &numbers(&m["coverage"]));
        assert_eq!(
            raw.dimensions(),
            (
                m["size"][0].as_u64().unwrap() as u32,
                m["size"][1].as_u64().unwrap() as u32
            )
        );
        assert_eq!(layer.mask_enabled, m["enabled"].as_bool().unwrap());
        assert!(!layer.mask_properties.linked);
        assert_eq!(
            layer.mask_properties.outside_coverage,
            m["outside_coverage"].as_u64().unwrap() as u8
        );
        let properties: Value = serde_json::from_str(&take_err_string(unsafe {
            dr_doc_layer_mask_properties(doc, index)
        }))
        .unwrap();
        assert_eq!(properties["offset_x"], m["offset"][0]);
        assert_eq!(properties["offset_y"], m["offset"][1]);
        assert_eq!(properties["outside_coverage"], m["outside_coverage"]);
        assert_eq!(properties["density"], 1.0);
        assert_eq!(properties["feather"], 0.0);
        assert_eq!(properties["linked"], false);
    }
}
fn qualify(name: &str, bytes: &[u8], manifest: &Value) -> Box<DrDocument> {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("input.psd");
    std::fs::write(&path, bytes).unwrap();
    let notice = report(&path);
    assert_eq!(notice["layered_available"], true, "{notice}");
    if manifest["loss_contains"].is_string() {
        assert!(
            notice["verified_losses"]
                .to_string()
                .contains(manifest["loss_contains"].as_str().unwrap()),
            "{notice}"
        );
    } else {
        assert!(
            !notice["verified_losses"].to_string().contains("mask"),
            "{notice}"
        );
    }
    if manifest["layers"]
        .as_array()
        .unwrap()
        .iter()
        .any(|l| !l["mask"].is_null())
    {
        assert!(
            notice["observations"]
                .to_string()
                .contains("independent (unlinked)"),
            "{notice}"
        );
    }
    let doc = open(&path, 0, &notice).unwrap();
    check_layers(&doc, &manifest["layers"]);
    assert_eq!(
        flat_pixels(&*doc),
        numbers(&manifest["composite_rgba"]),
        "{name}"
    );
    let native = dir.path().join("first.rz");
    let before = save(&doc, &native);
    let reopened = reopen(&native);
    check_layers(&reopened, &manifest["layers"]);
    assert_eq!(
        flat_pixels(&*reopened),
        numbers(&manifest["composite_rgba"])
    );
    assert_eq!(save(&reopened, &dir.path().join("second.rz")), before);
    assert_eq!(
        flat_pixels(&*open(&path, 1, &notice).unwrap()),
        numbers(&manifest["stored_composite_rgba"])
    );
    if manifest["moved"].is_object() {
        let m = &manifest["moved"];
        let index = m["layer"].as_u64().unwrap() as usize;
        let result = unsafe {
            dr_doc_with_layer_offset(
                &*reopened,
                index,
                m["offset"][0].as_i64().unwrap() as i32,
                m["offset"][1].as_i64().unwrap() as i32,
            )
        };
        assert!(!result.is_null());
        let moved = unsafe { Box::from_raw(result) };
        assert!(unsafe { dr_doc_layer_masks_equal(&*reopened, &*moved, index) });
        assert_eq!(flat_pixels(&*moved), numbers(&m["composite_rgba"]));
        let mut moved_layers = manifest["layers"].clone();
        if let Some(offsets) = m["layer_offsets"].as_object() {
            for (index, offset) in offsets {
                moved_layers[index.parse::<usize>().unwrap()]["offset"] = offset.clone();
            }
        } else {
            moved_layers[index]["offset"] = m["offset"].clone();
        }
        check_layers(&moved, &moved_layers);
        assert_eq!(save(&reopened, &native), before);
        check_layers(&doc, &manifest["layers"]);
        assert_eq!(
            moved.layers[index].pixels.contiguous_image().as_raw(),
            reopened.layers[index].pixels.contiguous_image().as_raw()
        );
    }
    assert_eq!(std::fs::read(path).unwrap(), bytes);
    export(name, bytes, manifest);
    doc
}
const CANVAS: [u32; 2] = [3, 2];
const RGBA: [u8; 32] = [
    11, 23, 37, 0, 41, 53, 67, 255, 71, 83, 97, 128, 101, 113, 127, 255, 131, 143, 157, 64, 161,
    173, 187, 255, 191, 203, 217, 1, 221, 233, 247, 255,
];
fn ordinary(outside: u8, enabled: bool) -> ExpectedMask {
    ExpectedMask {
        bounds: [-1, 0, 2, 2],
        coverage: vec![0, 255, 64, 128, 200, 255],
        outside,
        enabled,
    }
}
fn single_manifest(record: &Record, rgba: &[u8], mask: Option<&ExpectedMask>, gray: bool) -> Value {
    json!({"description":"ordinary independent raster mask", "canvas":CANVAS,"layers":[layer_json(record,0,rgba,mask)],
        "composite_rgba":normal_projection(CANVAS,record.bounds,rgba,record.opacity,mask),"stored_composite_rgba":stored(CANVAS,gray)})
}
#[test]
fn raw_and_rle_different_mask_rectangle_outside_defaults_and_independent_moves() {
    for outside in [0, 255] {
        for compressed in [false, true] {
            let mask = ordinary(outside, true);
            let record = Record::raster([0, -1, 2, 3], &RGBA, false).mask(&mask, compressed);
            let mut manifest = single_manifest(&record, &RGBA, Some(&mask), false);
            manifest["moved"] = json!({"layer":0,"offset":[0,0],"composite_rgba":normal_projection(CANVAS,[0,0,2,4],&RGBA,255,Some(&mask))});
            qualify(
                &format!(
                    "ordinary_{}_outside{outside}",
                    if compressed { "rle" } else { "raw" }
                ),
                &fixture(&[record], CANVAS, false),
                &manifest,
            );
        }
    }
}
#[test]
fn disabled_mask_retains_samples_and_can_be_enabled_without_reimport() {
    let mask = ordinary(255, false);
    let record = Record::raster([0, -1, 2, 3], &RGBA, false).mask(&mask, true);
    let doc = qualify(
        "disabled_rle",
        &fixture(std::slice::from_ref(&record), CANVAS, false),
        &single_manifest(&record, &RGBA, Some(&mask), false),
    );
    let ptr = unsafe { dr_doc_with_layer_mask_enabled(&*doc, 0, true) };
    assert!(!ptr.is_null());
    let enabled = unsafe { Box::from_raw(ptr) };
    let mut on = mask.clone();
    on.enabled = true;
    assert_eq!(
        flat_pixels(&*enabled),
        normal_projection(CANVAS, record.bounds, &RGBA, 255, Some(&on))
    );
    assert!(!doc.layers[0].mask_enabled);
    assert_eq!(
        doc.layers[0].mask.as_ref().unwrap().as_raw(),
        &mask.coverage
    );
}
#[test]
fn offcanvas_and_signed_extreme_masks_keep_independent_storage() {
    for (name, bounds, outside) in [
        ("mask_wholly_offcanvas", [9, 7, 10, 9], 255),
        ("mask_ending_zero", [-1, -2, 0, 0], 0),
        (
            "mask_signed_min",
            [i32::MIN, i32::MIN, i32::MIN + 1, i32::MIN + 2],
            255,
        ),
        (
            "mask_signed_max",
            [i32::MAX - 1, i32::MAX - 2, i32::MAX, i32::MAX],
            255,
        ),
    ] {
        let mask = ExpectedMask {
            bounds,
            coverage: vec![17, 239],
            outside,
            enabled: true,
        };
        let record = Record::raster([0, -1, 2, 3], &RGBA, false).mask(&mask, false);
        qualify(
            name,
            &fixture(std::slice::from_ref(&record), CANVAS, false),
            &single_manifest(&record, &RGBA, Some(&mask), false),
        );
    }
}
#[test]
fn grayscale_and_repeat_packbits_keep_scalar_coverage() {
    let rgba = [
        31, 31, 31, 255, 97, 97, 97, 128, 181, 181, 181, 0, 241, 241, 241, 255,
    ];
    let mask = ExpectedMask {
        bounds: [0, 0, 2, 3],
        coverage: vec![63, 63, 63, 1, 127, 255],
        outside: 255,
        enabled: true,
    };
    let mut record = Record::raster([0, 0, 2, 2], &rgba, true).mask(&mask, false);
    record.channels[0].1 = rows(&[&[128, 254, 63], &[2, 1, 127, 255]]);
    qualify(
        "grayscale_repeat_mask",
        &fixture(std::slice::from_ref(&record), CANVAS, true),
        &single_manifest(&record, &rgba, Some(&mask), true),
    );
}
#[test]
fn closed_isolated_and_pass_through_folder_masks_keep_verified_ids() {
    let canvas = [2, 1];
    let back = [120, 180, 240, 255, 90, 150, 210, 255];
    let art = [200, 100, 50, 255, 50, 100, 200, 255];
    for pass in [false, true] {
        let mask = ExpectedMask {
            bounds: [0, 0, 1, 1],
            coverage: vec![128],
            outside: 255,
            enabled: true,
        };
        let base = Record::raster([0, 0, 1, 2], &back, false);
        let mut child = Record::raster([0, 0, 1, 2], &art, false);
        child.blend = *b"mul ";
        let mut folder =
            Record::folder(2, if pass { *b"pass" } else { *b"norm" }).mask(&mask, true);
        folder.opacity = 128;
        let mut expected = vec![];
        for x in 0..2 {
            for c in 0..3 {
                let b = f64::from(back[x * 4 + c]);
                let s = f64::from(art[x * 4 + c]);
                let a = 128. / 255. * f64::from(mask.at(x as i32, 0)) / 255.;
                expected
                    .push((b * (1. - a) + (if pass { b * s / 255. } else { s }) * a).round() as u8);
            }
            expected.push(255);
        }
        let records = [
            base.clone(),
            Record::folder(3, *b"norm"),
            child.clone(),
            folder.clone(),
        ];
        let manifest = json!({"description":"closed folder mask; Multiply child distinguishes isolation from Pass Through","canvas":canvas,
            "layers":[layer_json(&base,0,&back,None),layer_json(&child,1,&art,None),layer_json(&folder,0,&[0;4],Some(&mask))],
            "composite_rgba":expected,"stored_composite_rgba":stored(canvas,false)});
        qualify(
            if pass {
                "folder_pass_mask"
            } else {
                "folder_isolated_mask"
            },
            &fixture(&records, canvas, false),
            &manifest,
        );
        if !pass {
            // The same folder with the position-relative flag (bit 0)
            // set is the same document, because the flag is not applied.
            let mut flagged = folder.clone();
            flagged.mask_body[17] |= 1;
            let records = [base, Record::folder(3, *b"norm"), child, flagged];
            let bytes = fixture(&records, canvas, false);
            let dir = TempDir::new().unwrap();
            let path = dir.path().join("flagged.psd");
            std::fs::write(&path, &bytes).unwrap();
            assert_eq!(flag_phrases(&report(&path)), 1);
            qualify("folder_relative_flag_mask", &bytes, &manifest);
        }
    }
}
/// The report sentence a mask with bit 0 set gets, counted over a report.
const FLAG_PHRASE: &str =
    "placement at its stored rectangle (the PSD position-relative flag, bit 0, is set and not applied, as Photoshop reads it)";
fn flag_phrases(notice: &Value) -> usize {
    notice["observations"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|line| line.as_str().unwrap().contains(FLAG_PHRASE))
        .count()
}
#[test]
fn a_position_relative_raster_mask_sits_at_its_stored_rectangle() {
    // Photoshop reads the rectangle as canvas coordinates whether or not
    // bit 0 is set. The layer starts at (3, 2), so the reading that added the
    // layer's top/left would have put this mask at (5, 3) instead of (2, 1).
    let canvas = [6, 5];
    for outside in [0, 255] {
        for compressed in [false, true] {
            let mask = ExpectedMask {
                bounds: [1, 2, 4, 4],
                coverage: vec![0, 255, 64, 128, 200, 255],
                outside,
                enabled: true,
            };
            let clear = Record::raster([2, 3, 4, 7], &RGBA, false).mask(&mask, compressed);
            let mut flagged = clear.clone();
            flagged.mask_body[17] |= 1;
            let layers = json!([layer_json(&clear, 0, &RGBA, Some(&mask))]);
            let projection = normal_projection(canvas, clear.bounds, &RGBA, 255, Some(&mask));
            let dir = TempDir::new().unwrap();
            let mut saved = Vec::new();
            for (name, record, phrases) in [("clear", clear, 0), ("flagged", flagged, 1)] {
                let path = dir.path().join(format!("{name}.psd"));
                std::fs::write(&path, fixture(&[record], canvas, false)).unwrap();
                let notice = report(&path);
                assert_eq!(notice["layered_available"], true, "{notice}");
                assert!(
                    notice["verified_losses"].as_array().unwrap().is_empty(),
                    "{notice}"
                );
                assert_eq!(flag_phrases(&notice), phrases, "{notice}");
                assert_eq!(
                    notice["observations"]
                        .to_string()
                        .contains("absolute placement"),
                    phrases == 0,
                    "{notice}"
                );
                let doc = open(&path, 0, &notice).unwrap();
                // `check_layers` asserts the mask offset is the stored
                // rectangle's left/top, for both files.
                check_layers(&doc, &layers);
                assert_eq!(flat_pixels(&*doc), projection, "{name}");
                saved.push(identity::byte_prior_body(&save(
                    &doc,
                    &dir.path().join(format!("{name}.rz")),
                )));
            }
            assert_eq!(saved[0], saved[1], "the flag changes nothing that is saved");
        }
    }
}
#[test]
fn a_sixteen_bit_position_relative_mask_sits_at_its_stored_rectangle() {
    use doctored_core::precision_image::REFERENCE_WORKING_BYTES as LIMIT;
    let (w, h) = (4usize, 2usize);
    let rgba: Vec<[u16; 4]> = (0..w * h)
        .map(|i| {
            let v = i as u16;
            [
                v * 4099 + 17,
                v * 8209 + 31,
                60000 - v * 257,
                [65535u16, 32768, 1, 65535][i % 4],
            ]
        })
        .collect();
    let planes: Vec<(i16, Vec<u8>)> = [(-1i16, 3usize), (0, 0), (1, 1), (2, 2)]
        .into_iter()
        .map(|(id, component)| {
            let samples: Vec<u16> = rgba.iter().map(|p| p[component]).collect();
            (id, psd16_fixture::raw_plane(&samples))
        })
        .collect();
    let coverage: Vec<u16> = vec![0, 65535, 16384, 32768, 51400, 65535];
    let stored_rectangle = [1, 2, 4, 4];
    let clear = psd16_fixture::Record::raster("Masked", [2, 3, 4, 7], planes).mask(
        stored_rectangle,
        0,
        true,
        psd16_fixture::raw_plane(&coverage),
    );
    let mut flagged = clear.clone();
    flagged.mask_body[17] |= 1;
    let composite = vec![[13u16, 27, 41, 65535]; 6 * 5];
    let dir = TempDir::new().unwrap();
    let mut seen = Vec::new();
    for (name, record, phrases) in [("clear", clear, 0), ("flagged", flagged, 1)] {
        let path = dir.path().join(format!("{name}.psd"));
        let bytes = psd16_fixture::fixture(
            (6, 5),
            &[record],
            doctored_core::icc::IccProfile::srgb().bytes(),
            (72.0, 72.0),
            &composite,
            false,
            false,
        );
        std::fs::write(&path, bytes).unwrap();
        let notice = report(&path);
        assert_eq!(notice["layered_available"], true, "{notice}");
        assert!(
            notice["verified_losses"].as_array().unwrap().is_empty(),
            "{notice}"
        );
        assert_eq!(flag_phrases(&notice), phrases, "{notice}");
        let fingerprint = CString::new(notice["source_fingerprint"].as_str().unwrap()).unwrap();
        let mut error = ptr::null_mut();
        let raw = unsafe {
            doctored_core::ffi_psd::dr_precision_doc_open_psd(
                cpath(&path).as_ptr(),
                0,
                fingerprint.as_ptr(),
                LIMIT,
                0,
                ptr::null(),
                &mut error,
            )
        };
        assert!(!raw.is_null(), "{}", take_err_string(error));
        let document = unsafe { &*raw };
        let mask = document.layer(0).and_then(|layer| layer.mask()).unwrap();
        assert_eq!(
            mask.properties().offset,
            (2, 1),
            "{name}: the stored left/top"
        );
        assert_eq!(mask.plane().dimensions(), (2, 3), "{name}");
        assert_eq!(mask.plane().samples(), &coverage[..], "{name}");
        let source = unsafe { doctored_core::ffi_precision::dr_precision_doc_layer_source(raw, 0) };
        assert!(!source.is_null());
        let rendered = unsafe {
            doctored_core::ffi_precision::dr_precision_doc_render(
                raw,
                LIMIT,
                0,
                ptr::null(),
                &mut error,
            )
        };
        assert!(!rendered.is_null(), "{}", take_err_string(error));
        let samples = |image: *mut doctored_core::precision_image::PrecisionImage| {
            let (iw, ih) = unsafe { &*image }.dimensions();
            let base =
                unsafe { doctored_core::ffi_precision::dr_precision_image_pixels_rgba16(image) };
            assert!(!base.is_null());
            let out =
                unsafe { std::slice::from_raw_parts(base, iw as usize * ih as usize * 4) }.to_vec();
            unsafe { doctored_core::ffi_precision::dr_precision_image_free(image) };
            out
        };
        let native = dir.path().join(format!("{name}.rz"));
        assert!(
            unsafe {
                doctored_core::ffi_precision::dr_precision_doc_save_native(
                    raw,
                    cpath(&native).as_ptr(),
                    LIMIT,
                    0,
                    ptr::null(),
                    &mut error,
                )
            },
            "{}",
            take_err_string(error)
        );
        seen.push((
            samples(source),
            samples(rendered),
            identity::typed_prior_body(&std::fs::read(&native).unwrap()),
        ));
        unsafe { doctored_core::ffi_precision::dr_precision_doc_free(raw) };
    }
    let layer: Vec<u16> = rgba.iter().flatten().copied().collect();
    assert_eq!(seen[0].0, layer, "the layer's own samples");
    assert!(seen[0].1.chunks_exact(4).any(|p| p[3] != 0));
    assert!(seen[0] == seen[1], "the flag changes nothing that is read");
}
#[test]
fn empty_sibling_groups_with_duplicate_names_keep_their_own_masks() {
    let black = ExpectedMask {
        bounds: [-1, 2, 0, 3],
        coverage: vec![37],
        outside: 0,
        enabled: false,
    };
    let white = ExpectedMask {
        bounds: [1, -1, 2, 0],
        coverage: vec![219],
        outside: 255,
        enabled: true,
    };
    let bottom = Record::folder(2, *b"pass").mask(&black, false);
    let top = Record::folder(1, *b"norm").mask(&white, true);
    let manifest = json!({"canvas":[1,1],"description":"empty duplicate-name sibling folder masks",
        "layers":[layer_json(&bottom,0,&[0;4],Some(&black)),layer_json(&top,0,&[0;4],Some(&white))],
        "composite_rgba":vec![0;4],"stored_composite_rgba":stored([1,1],false)});
    qualify(
        "empty_folder_masks",
        &fixture(
            &[
                Record::folder(3, *b"norm"),
                bottom,
                Record::folder(3, *b"norm"),
                top,
            ],
            [1, 1],
            false,
        ),
        &manifest,
    );
}
#[test]
fn group_move_uses_descendant_origin_and_leaves_unlinked_group_mask_in_place() {
    let canvas = [3, 1];
    let rgba = [31, 97, 181, 255, 241, 139, 53, 128];
    let mask = ExpectedMask {
        bounds: [0, 0, 1, 1],
        coverage: vec![64],
        outside: 255,
        enabled: true,
    };
    let child = Record::raster([0, 1, 1, 3], &rgba, false);
    let folder = Record::folder(2, *b"norm").mask(&mask, false);
    let manifest = json!({"description":"group move computes delta from descendant origin; unlinked mask stays absolute","canvas":canvas,
        "layers":[layer_json(&child,1,&rgba,None),layer_json(&folder,0,&[0;4],Some(&mask))],
        "composite_rgba":normal_projection(canvas,child.bounds,&rgba,255,Some(&mask)),"stored_composite_rgba":stored(canvas,false),
        "moved":{"layer":1,"offset":[0,0],"layer_offsets":{"0":[0,0],"1":[0,0]},"composite_rgba":normal_projection(canvas,[0,0,1,2],&rgba,255,Some(&mask))}});
    qualify(
        "group_independent_move",
        &fixture(&[Record::folder(3, *b"norm"), child, folder], canvas, false),
        &manifest,
    );
}
#[test]
fn unsupported_mask_forms_remain_precise_losses_without_decoding() {
    let mask = ordinary(255, true);
    let base = Record::raster([0, -1, 2, 3], &RGBA, false).mask(&mask, false);
    let mut cases = Vec::new();
    let mut add = |name: &str, record: Record, expected: &str| {
        cases.push((name.to_string(), record, expected.to_string()))
    };
    for (flag, name, reason) in [
        (4, "obsolete_invert", "inverted"),
        (8, "rendered", "rendered"),
        (16, "parameters", "density/feather"),
        (32, "unknown_flags", "unknown"),
    ] {
        let mut r = base.clone();
        r.mask_body[17] = flag;
        r.channels[0].1 = vec![0, 0, 99];
        add(name, r, reason);
    }
    for (length, name) in [
        (18, "short_body"),
        (36, "extended_body"),
        (40, "extended_parameters"),
    ] {
        let mut r = base.clone();
        r.mask_body.resize(length, 0);
        r.channels[0].1 = vec![0, 0, 99];
        add(name, r, "metadata length");
    }
    for key in [*b"vmsk", *b"vsms", *b"lmgm", *b"vmgm"] {
        let mut r = base.clone();
        r.blocks = block(&key, &[0; 8]);
        r.channels[0].1 = vec![0, 0, 99];
        add(std::str::from_utf8(&key).unwrap(), r, "advanced mask");
    }
    let mut r = base.clone();
    r.channels.push((-3, vec![0, 2, 99]));
    add("real_plus_user", r, "real-user");
    let mut r = base.clone();
    r.channels[0].0 = -3;
    add("real_only", r, "real-user");
    let mut r = base.clone();
    r.mask_body.clear();
    add("missing_metadata", r, "not both present");
    let mut r = base.clone();
    r.channels.remove(0);
    add("missing_channel", r, "not both present");
    let mut r = base.clone();
    r.mask_body[16] = 127;
    add("unsupported_default", r, "default is not");
    // Words 2 and 3 (ZIP) are read at both depths (`psd_zip8.rs`),
    // so only a word outside Adobe's table is an unsupported codec.
    for compression in [4, 91] {
        let mut r = base.clone();
        r.channels[0].1 = vec![0, compression, 99];
        add(
            &format!("codec_{compression}"),
            r,
            "compression is unsupported",
        );
    }
    for (name, record, expected) in cases {
        let mut manifest = single_manifest(&record, &RGBA, None, false);
        manifest["loss_contains"] = json!(expected);
        qualify(
            &format!("loss_{name}"),
            &fixture(&[record], CANVAS, false),
            &manifest,
        );
    }
    // Presence-only vector masks also produce a file-specific loss, without a -2 channel.
    let mut r = Record::raster([0, -1, 2, 3], &RGBA, false);
    r.blocks = block(b"vmsk", &[0; 8]);
    let mut manifest = single_manifest(&r, &RGBA, None, false);
    manifest["loss_contains"] = json!("advanced mask");
    qualify("loss_vector_only", &fixture(&[r], CANVAS, false), &manifest);
}
fn refused(name: &str, records: &[Record], reason: &str) {
    let bytes = fixture(records, CANVAS, false);
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("bad.psd");
    std::fs::write(&path, &bytes).unwrap();
    let notice = report(&path);
    assert_eq!(notice["layered_available"], false, "{name}: {notice}");
    assert!(
        notice["layered_refusal"].as_str().unwrap().contains(reason),
        "{name}: {notice}"
    );
    assert_eq!(
        notice["stored_composite_available"], true,
        "{name}: {notice}"
    );
    let error = open(&path, 0, &notice).err().unwrap();
    assert!(error.contains(reason), "{error}");
    assert!(!error.contains("internal error"), "{error}");
    assert_eq!(
        flat_pixels(&*open(&path, 1, &notice).unwrap()),
        stored(CANVAS, false)
    );
    assert_eq!(std::fs::read(&path).unwrap(), bytes);
    export(
        name,
        &bytes,
        &json!({"description":name,"canvas":CANVAS,"layered_available":false,"layered_refusal_contains":reason,
        "stored_composite_rgba":stored(CANVAS,false)}),
    );
}
#[test]
fn admitted_ordinary_mask_malformed_raw_and_rle_refuse_before_decoding() {
    let mask = ExpectedMask {
        bounds: [0, 0, 2, 2],
        coverage: vec![0, 1, 127, 255],
        outside: 255,
        enabled: true,
    };
    let base = Record::raster([0, -1, 2, 3], &RGBA, false).mask(&mask, false);
    for (name, data, reason) in [
        ("raw_short", vec![0, 0, 1, 2, 3], "length mismatch"),
        ("raw_long", vec![0, 0, 1, 2, 3, 4, 5], "length mismatch"),
        ("rle_table", vec![0, 1, 0, 3], "RLE row table"),
        ("rle_spill", rows(&[&[2, 1, 2, 3], &[0, 1]]), "RLE row"),
        (
            "rle_underflow",
            rows(&[&[0, 1], &[2, 1, 2, 3]]),
            "decoded width",
        ),
        ("rle_literal", rows(&[&[1, 9], &[1, 1, 2]]), "RLE row"),
        ("rle_repeat", rows(&[&[255], &[1, 1, 2]]), "RLE row"),
        (
            "rle_trailing",
            [rows(&[&[1, 1, 2], &[1, 3, 4]]), vec![0]].concat(),
            "trailing RLE",
        ),
    ] {
        let mut r = base.clone();
        r.channels[0].1 = data;
        refused(&format!("refusal_{name}"), &[r], reason);
    }
    for (name, bounds) in [
        ("empty", [0, 0, 0, 2]),
        ("reverse", [0, 0, 2, -1]),
        ("wide", [0, 0, 1, 30001]),
        ("signed_overflow", [i32::MIN, i32::MIN, i32::MAX, i32::MAX]),
        ("pixel_cap", [0, 0, 10001, 10000]),
    ] {
        let mut r = base.clone();
        r.mask_body = mask_body(bounds, 255, 0);
        refused(&format!("refusal_mask_{name}"), &[r], "mask rectangle");
    }
    let mut duplicate = base.clone();
    duplicate.channels.push((-2, raw(&[1])));
    refused("refusal_duplicate_mask", &[duplicate], "duplicate channel");
    let mut divider = Record::folder(3, *b"norm").mask(&mask, false);
    divider.mask_body.clear();
    refused(
        "refusal_divider_mask",
        &[divider, base, Record::folder(1, *b"norm")],
        "bounding divider",
    );
}
#[test]
fn mask_aggregate_and_decode_envelope_precede_channel_validation_and_allocation() {
    // Deliberately tiny invalid channel payloads: allocation refusal must win.
    let mask = ExpectedMask {
        bounds: [0, 0, 10000, 10000],
        coverage: vec![1],
        outside: 255,
        enabled: true,
    };
    let large = Record::raster([0, 0, 1, 1], &[7, 19, 43, 255], false).mask(&mask, false);
    refused(
        "refusal_mask_aggregate",
        &vec![large.clone(); 69],
        "aggregate mask pixels",
    );
    refused(
        "refusal_mask_envelope",
        &vec![large; 18],
        "2 GiB decode memory envelope",
    );
    // Group descriptors must participate too, even with no raster descendants.
    let group = Record::folder(1, *b"pass").mask(&mask, false);
    let records: Vec<_> = (0..18)
        .flat_map(|_| [Record::folder(3, *b"norm"), group.clone()])
        .collect();
    refused(
        "refusal_group_mask_envelope",
        &records,
        "2 GiB decode memory envelope",
    );
}
#[test]
fn overdeep_group_mask_is_disclosed_and_never_decoded() {
    let mask = ExpectedMask {
        bounds: [0, 0, 1, 1],
        coverage: vec![0],
        outside: 0,
        enabled: true,
    };
    let mut inner = Record::folder(1, *b"pass").mask(&mask, false);
    inner.channels[0].1 = vec![0, 0, 99, 98];
    let mut records = vec![Record::folder(3, *b"norm"); 11];
    records.push(Record::raster([0, 0, 1, 1], &[7, 19, 43, 255], false));
    records.push(inner);
    records.extend(vec![Record::folder(1, *b"pass"); 10]);
    let bytes = fixture(&records, [1, 1], false);
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("deep.psd");
    std::fs::write(&path, &bytes).unwrap();
    let notice = report(&path);
    assert_eq!(notice["layered_available"], true, "{notice}");
    assert!(
        notice["verified_losses"]
            .to_string()
            .contains("masked folder is dissolved"),
        "{notice}"
    );
    let doc = open(&path, 0, &notice).unwrap();
    assert_eq!(doc.layers.len(), 11);
    assert!(doc.layers.iter().all(|l| l.mask.is_none()));
    assert_eq!(flat_pixels(&*doc), [7, 19, 43, 255]);
}
#[test]
fn mask_refusal_and_stale_source_leave_captured_native_state_exact() {
    let mask = ordinary(255, true);
    let record = Record::raster([0, -1, 2, 3], &RGBA, false).mask(&mask, false);
    let bytes = fixture(std::slice::from_ref(&record), CANVAS, false);
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("input.psd");
    std::fs::write(&path, &bytes).unwrap();
    let notice = report(&path);
    let captured = open(&path, 0, &notice).unwrap();
    let native = dir.path().join("captured.rz");
    let before = save(&captured, &native);
    let mut malformed = record;
    malformed.channels[0].1 = vec![0, 0, 9];
    std::fs::write(&path, fixture(&[malformed], CANVAS, false)).unwrap();
    assert!(open(&path, 0, &notice)
        .err()
        .unwrap()
        .contains("changed since"));
    let fresh = report(&path);
    assert_eq!(fresh["layered_available"], false);
    assert!(open(&path, 0, &fresh).is_err());
    assert_eq!(save(&captured, &native), before);
    assert_eq!(
        captured.layers[0].mask.as_ref().unwrap().as_raw(),
        &mask.coverage
    );
}
