//! Alpha channels, vector masks and their DRDC 85 on a CMYK document (#1,
//! the channels batch), through the FFI: the channel edits (set, duplicate,
//! remove, rename, properties, import, a scalar fill, `read_plane`) keep the
//! ink bit-identical and the planes exactly what they were given; a vector
//! mask on a CMYK raster bakes over the layer's own rect when linked, and on
//! a group; the writer's word is 85 exactly when the document holds a
//! channel or a vector mask (82/83/84 otherwise) and the reopen is bit-exact
//! (ink, channels, vector masks, paths); a word patched to 84 refuses with
//! the pre-85 sentence and an 85 holding neither as an invalid word; and every
//! op that rebuilds a CMYK document — 16 → 8, Convert to Profile, both Mode
//! conversions, Flatten, Image Size, Crop, Rotate 90 and a layer's selected
//! pixels — keeps every channel (16 → 8 on the 8-bit grid; crop and rotation
//! against an index-remap oracle) and every raster-less vector mask, its
//! shape mapped as the document's own saved path of the same shape is.

use std::ffi::CString;
use std::ptr;

use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_cmyk::{
    dr_precision_doc_convert_cmyk_to_rgb, dr_precision_doc_convert_to_cmyk,
};
use doctored_core::ffi_cmyk_colour::dr_precision_doc_cmyk_profile_edit;
use doctored_core::ffi_path::dr_precision_doc_with_paths_json;
use doctored_core::ffi_precision_edit::dr_precision_doc_read_plane;
use doctored_core::ffi_psd::dr_precision_doc_psd_export_report;
use doctored_core::ffi_vector_mask::dr_precision_doc_set_vector_mask;
use doctored_core::path_geometry::PathShape;
use doctored_core::precision_document::PrecisionDocument;
use serde_json::{json, Value};
use tempfile::TempDir;

use crate::cmyk_fixture::{press_profile, press_profile_b};
use crate::cmyk_structure_fixture::{
    cmyk_layers, cmyk_word, free, open, save, then, try_edit, varied, RgbLayer, LIMIT,
};
use crate::common::take_err_string;

const W: u32 = 16;
const H: u32 = 12;

fn plane(seed: u32) -> Vec<u16> {
    (0..W * H)
        .map(|i| (i.wrapping_mul(2_654_435_761).wrapping_add(seed * 7919) % 65536) as u16)
        .collect()
}

/// Two CMYK layers: Bottom (full canvas) and Top (9 × 6 at (5, 3)).
fn base() -> *mut PrecisionDocument {
    cmyk_layers(
        W,
        H,
        vec![
            RgbLayer {
                name: "Bottom",
                w: W,
                h: H,
                offset: (0, 0),
                pixels: varied(W, H, 5),
            },
            RgbLayer {
                name: "Top",
                w: 9,
                h: 6,
                offset: (5, 3),
                pixels: varied(9, 6, 9),
            },
        ],
    )
}

fn channels(doc: *const PrecisionDocument) -> Vec<(String, Vec<u16>)> {
    unsafe { &*doc }
        .channels()
        .iter()
        .map(|c| (c.name().to_string(), c.plane().samples().to_vec()))
        .collect()
}

fn inks(doc: *const PrecisionDocument) -> Vec<Vec<[u16; 5]>> {
    let d = unsafe { &*doc };
    (0..d.layer_count())
        .filter_map(|i| d.cmyk_layer_samples(i))
        .collect()
}

/// The shape both the vector mask and the saved path take: a quad that
/// leaves Top's rect on the left and the bottom.
fn quad() -> Value {
    json!({"fill_rule":"nonzero","subpaths":[{"closed":true,
        "anchors":[[2.5,4.0],[12.0,2.5],[13.5,11.5],[3.0,10.0]]}]})
}

fn with_vector_mask(
    doc: *mut PrecisionDocument,
    layer: usize,
    shape: &Value,
) -> *mut PrecisionDocument {
    let text = CString::new(shape.to_string()).unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_set_vector_mask(doc, layer, text.as_ptr(), LIMIT, 0, ptr::null(), &mut err)
    };
    assert!(!out.is_null(), "vector mask: {}", take_err_string(err));
    free(doc);
    out
}

fn with_path(doc: *mut PrecisionDocument, shape: &Value) -> *mut PrecisionDocument {
    let mut path = shape.clone();
    path["id"] = json!(0);
    path["name"] = json!("Outline");
    path["kind"] = json!("saved");
    let text = CString::new(json!({"paths":[path]}).to_string()).unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe { dr_precision_doc_with_paths_json(doc, text.as_ptr(), &mut err) };
    assert!(!out.is_null(), "paths: {}", take_err_string(err));
    free(doc);
    out
}

/// The fixture of the carry cases: [`base`] with channels "Alpha 1" and
/// "Alpha 2", a vector mask on Top and the saved path of the same shape.
fn carried() -> *mut PrecisionDocument {
    let doc = then(
        base(),
        r#"{"op":"set_channel","index":null,"name":"Alpha 1"}"#,
        &plane(1),
    );
    let doc = then(
        doc,
        r#"{"op":"set_channel","index":null,"name":"Alpha 2"}"#,
        &plane(2),
    );
    with_path(with_vector_mask(doc, 1, &quad()), &quad())
}

fn vector_shape(doc: *const PrecisionDocument, layer: usize) -> Option<PathShape> {
    unsafe { &*doc }.vector_mask(layer).map(|view| view.shape)
}

fn path_shape(doc: *const PrecisionDocument) -> PathShape {
    unsafe { &*doc }.paths()[0].shape.clone()
}

#[test]
fn channel_edits_keep_the_ink_and_store_the_planes() {
    let doc = base();
    let ink = inks(doc);
    let one = then(
        base(),
        r#"{"op":"set_channel","index":null,"name":"Alpha 1"}"#,
        &plane(1),
    );
    assert_eq!(channels(one), vec![("Alpha 1".to_string(), plane(1))]);
    assert_eq!(inks(one), ink);
    let duplicated = then(one, r#"{"op":"duplicate_channel","index":0}"#, &[]);
    assert_eq!(
        channels(duplicated)[1],
        ("Alpha 1 copy".to_string(), plane(1))
    );
    let renamed = then(
        duplicated,
        r#"{"op":"rename_channel","index":1,"name":"Matte"}"#,
        &[],
    );
    let styled = then(
        renamed,
        r#"{"op":"channel_properties","index":1,"overlay_color":[0,200,40],"overlay_opacity":0.5}"#,
        &[],
    );
    assert_eq!(
        unsafe { &*styled }.channels()[1].metadata().overlay_color,
        [0, 200, 40]
    );
    let replaced = then(
        styled,
        r#"{"op":"set_channel","index":0,"name":"Alpha 1"}"#,
        &plane(3),
    );
    assert_eq!(channels(replaced)[0].1, plane(3));
    let imported = then(
        replaced,
        &json!({"op":"import_channel","name":"Imported","width":W,"height":H}).to_string(),
        &plane(4),
    );
    assert_eq!(channels(imported)[2], ("Imported".to_string(), plane(4)));
    // A scalar fill of one channel (painting and Fill on a channel target).
    let filled = then(
        imported,
        r#"{"op":"scalar_edit","target":"channel","index":1,"operation":{"kind":"fill","value":20000}}"#,
        &[],
    );
    assert!(channels(filled)[1].1.iter().all(|v| *v == 20000));
    assert_eq!(channels(filled)[0].1, plane(3));
    // `read_plane` reads a channel back.
    let request = CString::new(r#"{"op":"read_plane","target":"channel","index":2}"#).unwrap();
    let mut out = vec![0u16; (W * H) as usize];
    let mut err = ptr::null_mut();
    assert!(
        unsafe {
            dr_precision_doc_read_plane(
                filled,
                request.as_ptr(),
                out.as_mut_ptr(),
                out.len(),
                LIMIT,
                0,
                ptr::null(),
                &mut err,
            )
        },
        "{}",
        take_err_string(err)
    );
    assert_eq!(out, plane(4));
    let removed = then(filled, r#"{"op":"remove_channel","index":0}"#, &[]);
    assert_eq!(channels(removed).len(), 2);
    assert_eq!(channels(removed)[1].1, plane(4));
    assert_eq!(inks(removed), ink);
    free(removed);
    free(doc);
}

#[test]
fn a_vector_mask_bakes_over_the_cmyk_rasters_own_rect_and_on_a_group() {
    let doc = with_vector_mask(base(), 1, &quad());
    let mask = unsafe { &*doc }.layer(1).unwrap().mask().unwrap();
    // The quad's pixel box (2..14, 2..12) inside Top's rect (5..14, 3..9).
    assert_eq!(mask.properties().offset, (5, 3));
    assert_eq!(mask.plane().dimensions(), (9, 6));
    let pathed = with_path(base(), &quad());
    assert_eq!(vector_shape(doc, 1).unwrap(), path_shape(pathed));
    free(pathed);
    // Unlinked, the plane spans the canvas part of the box.
    let grouped = then(
        base(),
        r#"{"op":"group_layers","layers":[0,1],"name":"G","pass_through":false}"#,
        &[],
    );
    let grouped = with_vector_mask(grouped, 2, &quad());
    let group_mask = unsafe { &*grouped }.layer(2).unwrap().mask().unwrap();
    assert_eq!(group_mask.properties().offset, (2, 2));
    assert_eq!(group_mask.plane().dimensions(), (12, 10));
    assert!(vector_shape(grouped, 2).is_some());
    free(grouped);
    free(doc);
}

#[test]
fn the_word_is_85_exactly_for_channels_and_vector_masks_and_reopens_bit_exact() {
    let dir = TempDir::new().unwrap();
    let plain = base();
    assert_eq!(cmyk_word(&save(plain, &dir.path().join("plain.rz"))), 82);
    let masked = then(
        base(),
        r#"{"op":"set_mask","layer":1,"x":5,"y":3,"width":9,"height":6}"#,
        &plane(5)[..54],
    );
    assert_eq!(cmyk_word(&save(masked, &dir.path().join("masked.rz"))), 83);
    let structured = then(
        base(),
        r#"{"op":"group_layers","layers":[0,1],"name":"G","pass_through":false}"#,
        &[],
    );
    assert_eq!(
        cmyk_word(&save(structured, &dir.path().join("structured.rz"))),
        84
    );
    // Paths alone stay pre-85.
    let pathed = with_path(base(), &quad());
    assert_eq!(cmyk_word(&save(pathed, &dir.path().join("pathed.rz"))), 82);
    let channelled = then(
        base(),
        r#"{"op":"set_channel","index":null,"name":"Alpha 1"}"#,
        &plane(1),
    );
    assert_eq!(
        cmyk_word(&save(channelled, &dir.path().join("channel.rz"))),
        85
    );
    let vector = with_vector_mask(base(), 1, &quad());
    assert_eq!(cmyk_word(&save(vector, &dir.path().join("vector.rz"))), 85);
    let grouped_vector = with_vector_mask(
        then(
            base(),
            r#"{"op":"group_layers","layers":[0,1],"name":"G","pass_through":true}"#,
            &[],
        ),
        2,
        &quad(),
    );
    let bytes = save(grouped_vector, &dir.path().join("group_vector.rz"));
    assert_eq!(cmyk_word(&bytes), 85);
    // The full fixture round trips bit-exact.
    let doc = carried();
    let path = dir.path().join("carried.rz");
    let bytes = save(doc, &path);
    assert_eq!(cmyk_word(&bytes), 85);
    let back = open(&path).unwrap();
    assert!(unsafe { &*back }.is_cmyk());
    assert_eq!(inks(back), inks(doc));
    assert_eq!(channels(back), channels(doc));
    assert_eq!(vector_shape(back, 1), vector_shape(doc, 1));
    // Path ids are minted per session, never persisted.
    let named = |d: *const PrecisionDocument| {
        unsafe { &*d }
            .paths()
            .iter()
            .map(|p| (p.name.clone(), p.kind, p.shape.clone()))
            .collect::<Vec<_>>()
    };
    assert_eq!(named(back), named(doc));
    assert_eq!(
        unsafe { &*back }
            .layer(1)
            .unwrap()
            .mask()
            .unwrap()
            .plane()
            .samples(),
        unsafe { &*doc }
            .layer(1)
            .unwrap()
            .mask()
            .unwrap()
            .plane()
            .samples()
    );
    let group_back = open(&dir.path().join("group_vector.rz")).unwrap();
    assert!(vector_shape(group_back, 2).is_some());
    free(group_back);
    // Patched to 84 the channels refuse with the pre-85 sentence…
    let mut patched = bytes.clone();
    patched[8..12].copy_from_slice(&84u32.to_le_bytes());
    let at = dir.path().join("patched84.rz");
    std::fs::write(&at, &patched).unwrap();
    assert_eq!(
        open(&at).unwrap_err(),
        "a CMYK alpha channel or vector mask requires DRDC 85"
    );
    // …a vector mask too (on a group, so the file's 84 structure claim holds)…
    let mut patched = std::fs::read(dir.path().join("group_vector.rz")).unwrap();
    patched[8..12].copy_from_slice(&84u32.to_le_bytes());
    std::fs::write(&at, &patched).unwrap();
    assert_eq!(
        open(&at).unwrap_err(),
        "a CMYK alpha channel or vector mask requires DRDC 85"
    );
    // …and an 85 holding neither is an invalid word.
    let mut patched = std::fs::read(dir.path().join("plain.rz")).unwrap();
    patched[8..12].copy_from_slice(&85u32.to_le_bytes());
    std::fs::write(&at, &patched).unwrap();
    assert_eq!(open(&at).unwrap_err(), "invalid native CMYK channel word");
    for d in [
        plain,
        masked,
        structured,
        pathed,
        channelled,
        vector,
        grouped_vector,
        doc,
        back,
    ] {
        free(d);
    }
}

fn profile_edit(
    doc: *const PrecisionDocument,
    icc: Option<&[u8]>,
    request: &str,
) -> *mut PrecisionDocument {
    let text = CString::new(request).unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_cmyk_profile_edit(
            doc,
            icc.map_or(ptr::null(), |b| b.as_ptr()),
            icc.map_or(0, |b| b.len()),
            text.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(!out.is_null(), "{request}: {}", take_err_string(err));
    out
}

/// Every channel kept, by name and samples, and Top's vector mask live with
/// the saved path's shape.
fn assert_kept(before: *const PrecisionDocument, after: *const PrecisionDocument, what: &str) {
    assert_eq!(channels(after), channels(before), "{what}: channels");
    assert_eq!(
        vector_shape(after, 1),
        Some(path_shape(after)),
        "{what}: vector mask"
    );
}

#[test]
fn every_rebuild_keeps_the_channels_and_vector_masks() {
    let doc = carried();
    // 16 → 8 puts every channel on the 8-bit grid.
    let eight = profile_edit(doc, None, r#"{"op":"depth","depth":8}"#);
    let rounded: Vec<(String, Vec<u16>)> = channels(doc)
        .into_iter()
        .map(|(name, samples)| {
            (
                name,
                samples
                    .iter()
                    .map(|v| ((u32::from(*v) + 128) / 257 * 257) as u16)
                    .collect(),
            )
        })
        .collect();
    assert_eq!(channels(eight), rounded, "16 -> 8");
    assert_eq!(
        vector_shape(eight, 1),
        Some(path_shape(eight)),
        "16 -> 8 vector mask"
    );
    let converted = profile_edit(
        doc,
        Some(&press_profile_b("Press B")),
        r#"{"op":"convert"}"#,
    );
    assert_kept(doc, converted, "Convert to Profile");
    // Both Mode conversions.
    let mut err = ptr::null_mut();
    let rgb = unsafe {
        dr_precision_doc_convert_cmyk_to_rgb(
            doc,
            ptr::null(),
            0,
            c"{}".as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(!rgb.is_null(), "to RGB: {}", take_err_string(err));
    assert!(!unsafe { &*rgb }.is_cmyk());
    assert_kept(doc, rgb, "RGB Color");
    let profile = press_profile("Press Test");
    let back = unsafe {
        dr_precision_doc_convert_to_cmyk(
            rgb,
            profile.as_ptr(),
            profile.len(),
            c"{}".as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(!back.is_null(), "to CMYK: {}", take_err_string(err));
    assert!(unsafe { &*back }.is_cmyk());
    assert_kept(doc, back, "CMYK Color");
    // Flatten keeps the channels (the layers merge).
    let flat = then(carried(), r#"{"op":"flatten"}"#, &[]);
    assert_eq!(channels(flat), channels(doc), "Flatten");
    assert_eq!(unsafe { &*flat }.layer_count(), 1);
    // Image Size: every channel at the new size, the mask's shape mapped as the path's.
    let resized = try_edit(
        doc,
        r#"{"op":"resize","width":24,"height":18,"filter":"bilinear"}"#,
        &[],
    )
    .unwrap();
    for (_, samples) in channels(resized) {
        assert_eq!(samples.len(), 24 * 18);
    }
    assert_eq!(
        vector_shape(resized, 1),
        Some(path_shape(resized)),
        "Image Size"
    );
    assert_ne!(path_shape(resized), path_shape(doc));
    // Crop and Rotate 90 against an index remap.
    let cropped = try_edit(
        doc,
        r#"{"op":"crop","x":2,"y":1,"width":10,"height":8}"#,
        &[],
    )
    .unwrap();
    for ((_, got), (_, source)) in channels(cropped).iter().zip(channels(doc)) {
        let want: Vec<u16> = (0..10 * 8)
            .map(|i| source[((i / 10 + 1) * W + i % 10 + 2) as usize])
            .collect();
        assert_eq!(*got, want, "Crop");
    }
    assert_eq!(vector_shape(cropped, 1), Some(path_shape(cropped)), "Crop");
    let rotated = try_edit(doc, r#"{"op":"rotate90"}"#, &[]).unwrap();
    for ((_, got), (_, source)) in channels(rotated).iter().zip(channels(doc)) {
        // Clockwise: the new (x, y) is the old (y, H − 1 − x); the new width is H.
        let want: Vec<u16> = (0..W * H)
            .map(|i| {
                let (x, y) = (i % H, i / H);
                source[((H - 1 - x) * W + y) as usize]
            })
            .collect();
        assert_eq!(*got, want, "Rotate 90");
    }
    assert_eq!(
        vector_shape(rotated, 1),
        Some(path_shape(rotated)),
        "Rotate 90"
    );
    // A layer's selected pixels.
    let coverage: Vec<u16> = (0..W * H)
        .map(|i| if i % W < 6 && i / W < 5 { 65535 } else { 0 })
        .collect();
    let moved = try_edit(
        doc,
        r#"{"op":"move_selected","layer":0,"dx":2,"dy":1}"#,
        &coverage,
    )
    .unwrap();
    assert!(!moved.is_null());
    assert_kept(doc, moved, "move_selected");
    for d in [
        eight, converted, rgb, back, flat, resized, cropped, rotated, moved, doc,
    ] {
        free(d);
    }
}

/// `dr_precision_doc_psd_export_report`'s losses for a PSD in `mode` (0
/// Layers, 1 Flattened).
fn psd_losses(doc: *const PrecisionDocument, mode: i32) -> Vec<String> {
    let mut err = ptr::null_mut();
    let out = unsafe { dr_precision_doc_psd_export_report(doc, 0, mode, &mut err) };
    assert!(!out.is_null(), "{}", take_err_string(err));
    let text = unsafe { std::ffi::CStr::from_ptr(out) }
        .to_string_lossy()
        .into_owned();
    unsafe { dr_string_free(out) };
    let report: Value = serde_json::from_str(&text).expect("report JSON");
    report["losses"]
        .as_array()
        .expect("losses")
        .iter()
        .map(|line| line.as_str().expect("a sentence").to_string())
        .collect()
}

/// A CMYK PSD writes Top's raster-less vector mask as its path (`vmsk`), so
/// it names no vector mask written as pixels, Top's or Bottom's ordinary
/// mask, and says its two alpha channels are not written; the Flattened
/// mode's sentence already bakes every mask.
#[test]
fn cmyk_psd_export_writes_a_vector_mask_as_its_path_and_names_channels() {
    let doc = then(
        carried(),
        r#"{"op":"set_mask","layer":0,"x":0,"y":0,"width":16,"height":12}"#,
        &plane(6),
    );
    let losses = psd_losses(doc, 0);
    assert!(
        !losses
            .iter()
            .any(|line| line.starts_with("The vector mask on")),
        "{losses:?}"
    );
    assert!(
        losses.contains(
            &"2 saved channels are not written; this export stores no PSD alpha channels."
                .to_string()
        ),
        "{losses:?}"
    );
    let flattened = psd_losses(doc, 1);
    assert!(
        !flattened
            .iter()
            .any(|line| line.starts_with("The vector mask on")),
        "{flattened:?}"
    );
    free(doc);
}

/// The CMYK words are a ladder (`cmyk_native`): a document holding an
/// alpha channel AND a layer effect writes 86, which admits the channels
/// 85 introduced, and reopens with both.
#[test]
fn effects_over_channels_write_86_and_reopen_with_both() {
    let dir = TempDir::new().unwrap();
    let channelled = then(
        base(),
        r#"{"op":"set_channel","index":null,"name":"Alpha 1"}"#,
        &plane(1),
    );
    let styled = then(
        channelled,
        r#"{"op":"layer_style","version":1,"layer":1,"style":{"effects":[{"type":"drop_shadow"}]}}"#,
        &[],
    );
    let path = dir.path().join("styled_channels.rz");
    assert_eq!(cmyk_word(&save(styled, &path)), 86);
    let back = open(&path).unwrap();
    assert_eq!(channels(back), channels(styled));
    assert!(unsafe { &*back }.layer(1).unwrap().style().is_some());
    free(back);
    free(styled);
}
