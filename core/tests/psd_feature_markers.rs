//! Unsupported PSD metadata is reported from bounded markers, not guessed
//! descriptor contents. Independent PSD bytes and FFI observations qualify
//! reporting only; no authoring-application appearance claim is made here.
use crate::common;
use crate::identity;
use crate::psd_mask_fixture as fixture;
use common::{cpath, flat_pixels, take_err_string};
use doctored_core::doc::DrDocument;
use doctored_core::ffi_doc::dr_doc_save_native;
use doctored_core::ffi_psd::{dr_doc_open_psd, dr_psd_import_report};
use fixture::{block, fixture, layer_json, Record};
use serde_json::{json, Value};
use std::{ffi::CString, path::Path, ptr};
use tempfile::TempDir;

const CANVAS: [u32; 2] = [2, 1];
const RGBA: [u8; 8] = [11, 29, 47, 255, 101, 151, 199, 0];
const RENDER: [u8; 8] = [11, 29, 47, 255, 0, 0, 0, 0];
const STORED: [u8; 8] = [13, 27, 41, 255, 13, 27, 41, 255];

fn raster() -> Record {
    Record::raster([0, 0, 1, 2], &RGBA, false)
}
fn report(path: &Path) -> Value {
    let mut error = ptr::null_mut();
    let raw = unsafe { dr_psd_import_report(cpath(path).as_ptr(), &mut error) };
    assert!(!raw.is_null(), "{}", take_err_string(error));
    assert!(error.is_null());
    serde_json::from_str(&take_err_string(raw)).unwrap()
}
fn open(path: &Path, report: &Value, mode: i32) -> Box<DrDocument> {
    let token = CString::new(report["source_fingerprint"].as_str().unwrap()).unwrap();
    let mut error = ptr::null_mut();
    let raw = unsafe { dr_doc_open_psd(cpath(path).as_ptr(), mode, token.as_ptr(), &mut error) };
    assert!(!raw.is_null(), "{}", take_err_string(error));
    assert!(error.is_null());
    unsafe { Box::from_raw(raw) }
}
fn save(doc: &DrDocument, path: &Path) -> Vec<u8> {
    let mut error = ptr::null_mut();
    assert!(
        unsafe { dr_doc_save_native(doc, cpath(path).as_ptr(), &mut error) },
        "{}",
        take_err_string(error)
    );
    // 12E-1's always-written identity wrapper is peeled off: this file's
    // subject is the PSD exchange and the prior `.rz` body, so what it reads
    // back is byte-for-byte what the writer these tests were written against
    // produced.
    identity::byte_prior_body(&std::fs::read(path).unwrap())
}
fn qualify(name: &str, bytes: &[u8], losses: &[&str], observations: &[&str]) -> Value {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("metadata.psd");
    std::fs::write(&path, bytes).unwrap();
    let notice = report(&path);
    assert_eq!(notice["layered_available"], true, "{name}: {notice}");
    assert_eq!(notice["stored_composite_available"], true);
    let actual = notice["verified_losses"].as_array().unwrap();
    assert_eq!(actual.len(), losses.len(), "{name}: {notice}");
    for fragment in losses {
        assert!(
            actual
                .iter()
                .any(|v| v.as_str().unwrap().contains(fragment)),
            "{name}: {notice}"
        );
    }
    for fragment in observations {
        assert!(
            notice["observations"]
                .as_array()
                .unwrap()
                .iter()
                .any(|v| v.as_str().unwrap().contains(fragment)),
            "{name}: {notice}"
        );
    }
    let doc = open(&path, &notice, 0);
    assert_eq!(doc.layers.len(), 1);
    assert_eq!(doc.layers[0].pixels.contiguous_image().as_raw(), &RGBA);
    assert_eq!(flat_pixels(&*doc), RENDER);
    assert_eq!(flat_pixels(&*open(&path, &notice, 1)), STORED);
    let native = save(&doc, &temp.path().join("marked.rz"));
    let plain = temp.path().join("plain.psd");
    std::fs::write(&plain, fixture(&[raster()], CANVAS, false)).unwrap();
    assert_eq!(
        save(
            &open(&plain, &report(&plain), 0),
            &temp.path().join("plain.rz")
        ),
        native
    );
    assert_eq!(std::fs::read(path).unwrap(), bytes);
    if let Ok(directory) = std::env::var("DR_PSD_MARKER_FIXTURES") {
        let directory = Path::new(&directory);
        std::fs::create_dir_all(directory).unwrap();
        std::fs::write(directory.join(format!("{name}.psd")), bytes).unwrap();
        let expected = json!({"canvas":CANVAS,"layers":[layer_json(&raster(),0,&RGBA,None)],
            "composite_rgba":RENDER,"stored_composite_rgba":STORED,
            "loss_fragments":losses,"observation_fragments":observations});
        std::fs::write(
            directory.join(format!("{name}.psd.json")),
            serde_json::to_vec_pretty(&expected).unwrap(),
        )
        .unwrap();
    }
    notice
}

#[test]
fn each_documented_descriptor_marker_is_reported_without_payload_interpretation() {
    for (keys, category) in [
        (&["lrFX", "lfx2", "lmfx"][..], "layer-style metadata"),
        (
            &[
                "SoCo", "GdFl", "PtFl", "brit", "levl", "curv", "expA", "vibA", "hue ", "hue2",
                "blnc", "blwh", "phfl", "mixr", "clrL", "nvrt", "post", "thrs", "grdm", "selc",
            ][..],
            "adjustment/fill metadata",
        ),
        (&["tySh", "TySh", "Txt2"][..], "text/type metadata"),
        (
            &[
                "plLd", "PlLd", "SoLd", "SoLE", "lnkD", "lnk2", "lnk3", "lnkE",
            ][..],
            "placed/smart-object or linked-layer metadata",
        ),
        (
            &["vstk", "vscg", "vogk"][..],
            "vector stroke/origination metadata",
        ),
    ] {
        for key in keys {
            let mut record = raster();
            // Deliberately not a valid descriptor: presence does not validate
            // nested settings, and no decoder may follow lengths in this body.
            record.blocks = block(key.as_bytes().try_into().unwrap(), &[255; 4]);
            qualify(
                &format!(
                    "marker-{}-{}",
                    key.replace(' ', "_"),
                    key.as_bytes()
                        .iter()
                        .map(|v| format!("{v:02x}"))
                        .collect::<String>()
                ),
                &fixture(&[record], CANVAS, false),
                &[category],
                &[],
            );
        }
    }
}

// Add an independently framed range section to the one-raster fixture. The
// fixed offsets follow its authored header, four channel entries and empty
// mask section; they do not come from an importer or its metadata output.
fn ranges(bytes: &[u8]) -> Vec<u8> {
    let mut out = fixture(&[raster()], CANVAS, false);
    assert_eq!(&out[102..110], &[0; 8]);
    for offset in [34, 38, 98] {
        let old = u32::from_be_bytes(out[offset..offset + 4].try_into().unwrap());
        out[offset..offset + 4].copy_from_slice(&(old + bytes.len() as u32).to_be_bytes());
    }
    out[106..110].copy_from_slice(&(bytes.len() as u32).to_be_bytes());
    out.splice(110..110, bytes.iter().copied());
    out
}

#[test]
fn default_ranges_and_simple_disabled_settings_are_not_substantive_losses() {
    qualify("plain", &fixture(&[raster()], CANVAS, false), &[], &[]);
    for count in [1, 4] {
        qualify(
            &format!("default-ranges-{count}"),
            &ranges(&[0, 0, 255, 255, 0, 0, 255, 255].repeat(count)),
            &[],
            &["unrestricted default Blend If ranges"],
        );
    }
    for (key, body, observation) in [
        (b"knko", [0, 99, 88, 77], "disabled knockout flag"),
        (b"lspf", [0; 4], "zero layer-protection flags"),
        (
            b"clbl",
            [1, 0, 0, 0],
            "additional clipping/interior blending metadata",
        ),
        (
            b"infx",
            [0; 4],
            "additional clipping/interior blending metadata",
        ),
        (b"iOpa", [255, 0, 0, 0], "fill-opacity metadata"),
    ] {
        let mut record = raster();
        record.blocks = block(key, &body);
        qualify(
            &format!("default-{}", std::str::from_utf8(key).unwrap()),
            &fixture(&[record], CANVAS, false),
            &[],
            &[observation],
        );
    }
}

#[test]
fn nondefault_or_unrecognized_simple_fields_get_precise_disclosures() {
    // A restricted ALPHA slot: gray, red, green and blue are carried as a
    // Doctored Blend If (`psd_layer_effects`, one range each), so only a
    // channel it has no range for is still discarded.
    qualify(
        "changed-ranges",
        &ranges(
            &[
                [0, 0, 255, 255, 0, 0, 255, 255].repeat(4),
                vec![0, 0, 255, 255, 0, 3, 250, 255],
            ]
            .concat(),
        ),
        &["nondefault or unrecognized Blend If"],
        &[],
    );
    qualify(
        "unrecognized-ranges",
        &ranges(&[0, 0, 255, 255]),
        &["nondefault or unrecognized Blend If"],
        &[],
    );
    for (key, body, fragment) in [
        // An UNRECOGNIZED knockout value: `[1, 0, 0, 0]` is Shallow and is
        // CARRIED since 12E-3c, which would change this record's `.rz`
        // equality, so the unrecognized case names a value no build maps.
        (
            b"knko",
            &[3, 0, 0, 0][..],
            "nonzero or unrecognized knockout",
        ),
        (b"knko", &[0, 0][..], "nonzero or unrecognized knockout"),
        (
            b"lspf",
            &[0, 0, 0, 0x10][..],
            "nonzero or unrecognized layer-protection",
        ),
        (
            b"lspf",
            &[0, 0][..],
            "nonzero or unrecognized layer-protection",
        ),
    ] {
        let mut record = raster();
        record.blocks = block(key, body);
        qualify(
            &format!(
                "changed-{}-{}",
                std::str::from_utf8(key).unwrap(),
                body.len()
            ),
            &fixture(&[record], CANVAS, false),
            &[fragment],
            &[],
        );
    }
    // Unqualified option payloads remain an explicit observation even when
    // nondefault bytes occur; the report never invents active-effect semantics.
    for key in [b"clbl", b"infx", b"iOpa"] {
        let mut record = raster();
        record.blocks = block(key, &[127; 4]);
        let notice = qualify(
            &format!("opaque-{}", std::str::from_utf8(key).unwrap()),
            &fixture(&[record], CANVAS, false),
            &[],
            &["payload is not interpreted or retained"],
        );
        assert!(notice["general_limitations"]
            .to_string()
            .contains("unknown keys and global additional-info blocks"));
    }
}

#[test]
fn marker_categories_are_bounded_deduplicated_and_do_not_guess_unknown_keys() {
    let mut record = raster();
    for _ in 0..64 {
        record.blocks.extend(block(b"lrFX", &[255; 4]));
        record.blocks.extend(block(b"lfx2", &[255; 4]));
    }
    qualify(
        "duplicate-effects",
        &fixture(&[record], CANVAS, false),
        &["layer-style metadata"],
        &[],
    );
    let mut record = raster();
    record.blocks = block(b"lspf", &[0; 4]);
    record.blocks.extend(block(b"lspf", &[0, 0, 0, 1]));
    let notice = qualify(
        "conflicting-protection",
        &fixture(&[record], CANVAS, false),
        &["nonzero or unrecognized layer-protection"],
        &[],
    );
    assert!(!notice["observations"]
        .to_string()
        .contains("zero layer-protection flags"));
    let mut record = raster();
    for key in [b"ZZZZ", b"lyid", b"lclr", b"fxrp"] {
        record.blocks.extend(block(key, &[255; 4]));
    }
    let notice = qualify(
        "uninspected-keys",
        &fixture(&[record], CANVAS, false),
        &[],
        &[],
    );
    assert!(!notice["observations"].to_string().contains("metadata"));
}

#[test]
fn documented_header_flags_are_disclosed_only_when_their_meaning_is_valid() {
    let mut protected = raster();
    protected.flags |= 1;
    protected.blocks = block(b"lspf", &[0; 4]);
    let notice = qualify(
        "protected-header",
        &fixture(&[protected], CANVAS, false),
        &["layer-protection metadata"],
        &[],
    );
    assert!(!notice["observations"]
        .to_string()
        .contains("zero layer-protection flags"));
    let mut irrelevant = raster();
    irrelevant.flags = 24;
    qualify(
        "irrelevant-pixels",
        &fixture(&[irrelevant], CANVAS, false),
        &["raster pixels irrelevant to document appearance"],
        &[],
    );
    let mut invalid_bit4 = raster();
    invalid_bit4.flags = 16;
    qualify(
        "invalid-pixel-meaning",
        &fixture(&[invalid_bit4], CANVAS, false),
        &[],
        &[],
    );

    let mut folder = Record::folder(2, *b"norm");
    folder.flags = 24;
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("folder.psd");
    std::fs::write(
        &path,
        fixture(
            &[Record::folder(3, *b"norm"), raster(), folder],
            CANVAS,
            false,
        ),
    )
    .unwrap();
    let notice = report(&path);
    assert_eq!(notice["layered_available"], true, "{notice}");
    assert!(
        notice["verified_losses"].as_array().unwrap().is_empty(),
        "{notice}"
    );
    assert_eq!(flat_pixels(&*open(&path, &notice, 0)), RENDER);
}

#[test]
fn duplicate_names_still_correlate_marker_losses_to_verified_rasters_and_folders() {
    let mut lower = raster();
    lower.blocks = block(b"lrFX", &[255; 4]);
    let mut inside = raster();
    inside.blocks = block(b"TySh", &[255; 4]);
    let mut folder = Record::folder(2, *b"norm");
    folder.blocks = block(b"SoLd", &[255; 4]);
    let mut close = Record::folder(3, *b"norm");
    close.blocks = block(b"knko", &[1, 0, 0, 0]);
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("duplicate.psd");
    let bytes = fixture(&[lower, close, inside, folder], CANVAS, false);
    std::fs::write(&path, &bytes).unwrap();
    let notice = report(&path);
    assert_eq!(notice["layered_available"], true, "{notice}");
    let losses = notice["verified_losses"].as_array().unwrap();
    assert_eq!(losses.len(), 4, "{notice}");
    for text in [
        "Layer 0 (“Duplicate”) has text/type",
        "Layer 1 (“Duplicate”) has layer-style",
        "Group 1 (“Duplicate”) has placed/smart-object",
        "PSD bounding-divider metadata has nonzero or unrecognized knockout",
    ] {
        assert!(
            losses.iter().any(|v| v.as_str().unwrap().starts_with(text)),
            "{notice}"
        );
    }
    let doc = open(&path, &notice, 0);
    assert_eq!(
        doc.layers.iter().map(|l| l.depth).collect::<Vec<_>>(),
        [0, 1, 0]
    );
    assert_eq!(doc.layers[0].pixels.contiguous_image().as_raw(), &RGBA);
    assert_eq!(doc.layers[1].pixels.contiguous_image().as_raw(), &RGBA);
    assert_eq!(flat_pixels(&*doc), RENDER);
    assert_eq!(flat_pixels(&*open(&path, &notice, 1)), STORED);
    assert_eq!(std::fs::read(path).unwrap(), bytes);
}
