//! Pattern fill layers in a CMYK PSD: our 8- and 16-bit
//! layered CMYK export writes a pattern fill layer in Photoshop's `PtFl`
//! form, as the RGB walk does — its ink over its mask's rectangle (the
//! canvas without a mask), channels `-1`, `0`–`3` and `-2`, flags `0x18`,
//! `PtFl`, `fxrp` = its reference, its tile in `Patt`/`Pat2` — and our
//! importer reads it back as the same pattern fill layer: its description,
//! samples, mask and opacity kept, the report observing it, the export
//! report naming no description loss. With its tile gone, the record stays a
//! raster of its own pixels with its marker losses. The pixel-less form is
//! read too, and is omitted with its reason when the tile is gone.
//!
//! The written channels are checked against the document's own samples
//! (alpha as stored, each ink `max − v` at the file's depth); the reopened
//! fill against the original's samples, which `pattern_fill_cmyk.rs` holds
//! to its closed-form oracle.

use std::ffi::CString;
use std::path::Path;
use std::ptr;

use doctored_core::ffi_cmyk_artwork::dr_precision_doc_cmyk_artwork;
use doctored_core::ffi_precision::{dr_precision_doc_free, dr_precision_doc_set_layer_opacity};
use doctored_core::ffi_precision_edit::dr_precision_doc_edit;
use doctored_core::ffi_psd::{dr_precision_doc_psd_export_report, dr_precision_doc_save_psd};
use doctored_core::precision_document::PrecisionDocument;
use doctored_core::precision_image::REFERENCE_WORKING_BYTES;
use serde_json::{json, Value};
use tempfile::TempDir;

use crate::cmyk_fixture::press_profile;
use crate::common::{cpath, take_err_string};
use crate::psd_pattern_fill_layer::{
    contains, framed_records, import_report, norm, pixel_less, plane, try_open16,
};

const LIMIT: u64 = REFERENCE_WORKING_BYTES;
const W: u32 = 24;
const H: u32 = 16;
/// The masked fill's mask: (3, 2), 14 × 9.
const MASK_AT: (i32, i32) = (3, 2);
const MASK_SIZE: (u32, u32) = (14, 9);
const MASKED_RECT: [i32; 4] = [
    MASK_AT.1,
    MASK_AT.0,
    MASK_AT.1 + MASK_SIZE.1 as i32,
    MASK_AT.0 + MASK_SIZE.0 as i32,
];
const CANVAS_RECT: [i32; 4] = [0, 0, H as i32, W as i32];

fn base64(bytes: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        for i in 0..4 {
            out.push(if i <= chunk.len() {
                T[((n >> (18 - 6 * i)) & 63) as usize] as char
            } else {
                '='
            });
        }
    }
    out
}

/// One fill under test: linked or not, its phase and its layer's reference.
#[derive(Clone, Copy)]
struct Case {
    linked: bool,
    phase: (i32, i32),
    reference: (i32, i32),
}

impl Case {
    /// The `pattern_fill` keys as a fill layer's description stores them:
    /// a 5 × 3 tile of distinct opaque colours.
    fn keys(&self) -> Value {
        let mut rgba = Vec::new();
        for y in 0..3u32 {
            for x in 0..5u32 {
                rgba.extend([
                    (30 + 40 * x) as u8,
                    (20 + 90 * y) as u8,
                    (220 - 30 * x) as u8,
                    255,
                ]);
            }
        }
        json!({
            "pattern": {"name": "T", "width": 5, "height": 3, "rgba": base64(&rgba)},
            "angle": 0, "scale": 1, "link_with_layer": self.linked,
            "phase": [self.phase.0, self.phase.1],
        })
    }

    fn description(&self) -> String {
        json!({"type": "fill", "version": 1, "fill": "pattern",
               "pattern_fill": self.keys(), "canvas": [W, H]})
        .to_string()
    }

    /// The host's drawing: the keys plus `anchor` = −reference when linked.
    fn components(&self) -> String {
        let mut fill = self.keys();
        if self.linked {
            fill["anchor"] = json!([-self.reference.0, -self.reference.1]);
        }
        json!([{"kind": "pattern", "fill": fill}]).to_string()
    }
}

/// Linked on a layer moved (+7, +3); unlinked with a phase, never moved.
const LINKED: Case = Case {
    linked: true,
    phase: (0, 0),
    reference: (7, 3),
};
const UNLINKED: Case = Case {
    linked: false,
    phase: (5, 2),
    reference: (0, 0),
};

pub(crate) fn free(doc: *mut PrecisionDocument) {
    unsafe { dr_precision_doc_free(doc) };
}

/// `dr_precision_doc_edit`, the input freed.
pub(crate) fn then(
    doc: *mut PrecisionDocument,
    request: &Value,
    samples: &[u16],
) -> *mut PrecisionDocument {
    let text = CString::new(request.to_string()).unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_edit(
            doc,
            text.as_ptr(),
            if samples.is_empty() {
                ptr::null()
            } else {
                samples.as_ptr()
            },
            samples.len(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(!out.is_null(), "{request}: {}", take_err_string(err));
    free(doc);
    out
}

/// `case`'s fill published over (or, `replacing`, onto) layer `layer`, the
/// input freed.
fn publish(
    doc: *mut PrecisionDocument,
    layer: usize,
    replacing: bool,
    case: &Case,
    name: &str,
) -> *mut PrecisionDocument {
    let components = CString::new(case.components()).unwrap();
    let name = CString::new(name).unwrap();
    let description = CString::new(case.description()).unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_cmyk_artwork(
            doc,
            layer,
            replacing,
            0,
            W,
            H,
            components.as_ptr(),
            ptr::null(),
            0,
            ptr::null(),
            0,
            0,
            0,
            name.as_ptr(),
            description.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(!out.is_null(), "publish: {}", take_err_string(err));
    free(doc);
    out
}

/// Paper, then LINKED (moved to its reference, re-rendered there, masked,
/// opacity 0.6), then UNLINKED, at `depth`.
fn cmyk_doc(depth: u32) -> *mut PrecisionDocument {
    let profile = press_profile("Press Test");
    let request = format!(r#"{{"depth":{depth},"fill":[0,0,0,0,1],"layer_name":"Paper"}}"#);
    let doc = PrecisionDocument::new_cmyk(W, H, Some(&profile), &request, LIMIT, 0).unwrap();
    let doc = Box::into_raw(Box::new(doc));
    let doc = publish(doc, 0, false, &LINKED, "Fill 0");
    // A pure translation moves the reference with the layer; the fill is
    // then re-rendered over the canvas at the moved reference (the host's
    // move).
    let (dx, dy) = LINKED.reference;
    let doc = then(
        doc,
        &json!({"op": "affine", "layer": 1, "target": "pixels",
                "matrix": [1, 0, 0, 1, dx, dy], "filter": "bilinear"}),
        &[],
    );
    assert_eq!(
        unsafe { &*doc }.layer(1).unwrap().reference(),
        LINKED.reference
    );
    let doc = publish(doc, 1, true, &LINKED, "Fill 0");
    let doc = then(
        doc,
        &json!({"op": "set_mask", "layer": 1, "width": MASK_SIZE.0, "height": MASK_SIZE.1,
                "x": MASK_AT.0, "y": MASK_AT.1}),
        &vec![65535u16; (MASK_SIZE.0 * MASK_SIZE.1) as usize],
    );
    let mut err = ptr::null_mut();
    let opaque = unsafe { dr_precision_doc_set_layer_opacity(doc, 1, 0.6, &mut err) };
    assert!(!opaque.is_null(), "opacity: {}", take_err_string(err));
    free(doc);
    publish(opaque, 1, false, &UNLINKED, "Fill 1")
}

pub(crate) fn save_layers(doc: *const PrecisionDocument, path: &Path) {
    let mut err = ptr::null_mut();
    let ok = unsafe {
        dr_precision_doc_save_psd(
            doc,
            ptr::null(),
            cpath(path).as_ptr(),
            0,
            0,
            LIMIT,
            0,
            std::ptr::null(),
            &mut err,
        )
    };
    assert!(ok, "save: {}", take_err_string(err));
}

pub(crate) fn open(path: &Path) -> *mut PrecisionDocument {
    let token = import_report(path)["source_fingerprint"]
        .as_str()
        .unwrap()
        .to_string();
    let doc = try_open16(path, &token, LIMIT).unwrap_or_else(|e| panic!("open: {e}"));
    Box::into_raw(Box::new(doc))
}

/// The written `PtFl` record named `name` is Photoshop's form: its
/// rectangle `rect`, bit 4, `PtFl`, `fxrp` = `case`'s reference, a `-2`
/// channel exactly when masked, and channels `-1`, `0`–`3` holding layer
/// `index`'s samples over `rect` at `sample` bytes.
#[allow(clippy::too_many_arguments)]
fn assert_fill_record(
    bytes: &[u8],
    doc: *const PrecisionDocument,
    index: usize,
    name: &str,
    case: &Case,
    rect: [i32; 4],
    sample: usize,
    what: &str,
) {
    let records = framed_records(bytes);
    let record = records
        .iter()
        .find(|r| r.name == name)
        .unwrap_or_else(|| panic!("{what}: the record {name}"));
    assert_eq!(record.bounds, rect, "{what}: {name}'s rectangle");
    assert_eq!(record.flags & 0x18, 0x18, "{what}: {name}'s bit 4");
    assert!(
        record.blocks.iter().any(|(key, _)| key == b"PtFl"),
        "{what}: PtFl"
    );
    let fxrp = &record
        .blocks
        .iter()
        .find(|(key, _)| key == b"fxrp")
        .unwrap_or_else(|| panic!("{what}: {name} carries fxrp"))
        .1;
    let x = f64::from_be_bytes(fxrp[0..8].try_into().unwrap());
    let y = f64::from_be_bytes(fxrp[8..16].try_into().unwrap());
    assert_eq!(
        (x, y),
        (f64::from(case.reference.0), f64::from(case.reference.1)),
        "{what}: {name}'s fxrp"
    );
    let masked = rect == MASKED_RECT;
    assert_eq!(
        record.channels.iter().any(|(id, _)| *id == -2),
        masked,
        "{what}: {name}'s mask channel"
    );
    if masked {
        assert_eq!(record.mask, Some((rect, 0)), "{what}: the mask's rectangle");
        // The `-2` channel's bytes are the mask's samples (all opaque).
        let stored = &record.channels.iter().find(|(id, _)| *id == -2).unwrap().1;
        let decoded = plane(stored, w_of(rect), h_of(rect), sample);
        let full = if sample == 1 {
            vec![255u8]
        } else {
            vec![255u8, 255]
        };
        assert!(
            decoded.chunks(sample).all(|v| v == full.as_slice()),
            "{what}: {name}'s -2 samples"
        );
    }
    let samples = unsafe { &*doc }.cmyk_layer_samples(index).unwrap();
    let (w, h) = ((rect[3] - rect[1]) as usize, (rect[2] - rect[0]) as usize);
    let max = if sample == 1 { 255u16 } else { 65535 };
    for (id, component) in [(-1i16, 4usize), (0, 0), (1, 1), (2, 2), (3, 3)] {
        let stored = &record
            .channels
            .iter()
            .find(|(c, _)| *c == id)
            .unwrap_or_else(|| panic!("{what}: channel {id}"))
            .1;
        let decoded = plane(stored, w, h, sample);
        for j in 0..h {
            for i in 0..w {
                let (cx, cy) = (rect[1] as u32 + i as u32, rect[0] as u32 + j as u32);
                let v = samples[(cy * W + cx) as usize][component];
                let at = (j * w + i) * sample;
                let got = if sample == 1 {
                    u16::from(decoded[at])
                } else {
                    u16::from_be_bytes([decoded[at], decoded[at + 1]])
                };
                let value = if sample == 1 {
                    ((u32::from(v) + 128) / 257) as u16
                } else {
                    v
                };
                let want = if id == -1 { value } else { max - value };
                assert_eq!(got, want, "{what}: {name} channel {id} at ({cx},{cy})");
            }
        }
    }
}

pub(crate) fn w_of(rect: [i32; 4]) -> usize {
    (rect[3] - rect[1]) as usize
}

fn h_of(rect: [i32; 4]) -> usize {
    (rect[2] - rect[0]) as usize
}

pub(crate) fn description(doc: *const PrecisionDocument, index: usize) -> Option<Value> {
    unsafe { &*doc }
        .artwork_description(index)
        .map(|d| serde_json::from_str(d).unwrap())
}

#[test]
fn a_cmyk_pattern_fill_layer_round_trips_in_photoshops_ptfl_form() {
    let dir = TempDir::new().unwrap();
    for (depth, table) in [(8u32, b"8BIMPatt"), (16, b"8BIMPat2")] {
        let what = format!("{depth}-bit");
        let doc = cmyk_doc(depth);
        let path = dir.path().join(format!("fill{depth}.psd"));
        save_layers(doc, &path);
        let bytes = std::fs::read(&path).unwrap();
        assert!(contains(&bytes, b"8BIMPtFl"), "{what}: PtFl");
        assert!(contains(&bytes, table), "{what}: the pattern table");
        let sample = (depth / 8) as usize;
        assert_fill_record(
            &bytes,
            doc,
            1,
            "Fill 0",
            &LINKED,
            MASKED_RECT,
            sample,
            &what,
        );
        assert_fill_record(
            &bytes,
            doc,
            2,
            "Fill 1",
            &UNLINKED,
            CANVAS_RECT,
            sample,
            &what,
        );
        // The export report names no description loss: the fill is its
        // record.
        let mut err = ptr::null_mut();
        let report = unsafe { dr_precision_doc_psd_export_report(doc, 0, 0, &mut err) };
        assert!(!report.is_null(), "{}", take_err_string(err));
        let report: Value = serde_json::from_str(&take_err_string(report)).unwrap();
        assert!(
            !report["losses"]
                .to_string()
                .contains("re-editable description"),
            "{what}: {}",
            report["losses"]
        );
        // Reopened: the same two fill layers.
        let observations = import_report(&path)["observations"].to_string();
        for name in ["Fill 0", "Fill 1"] {
            assert!(
                observations.contains(&format!(
                    "(“{name}”) is a Photoshop Pattern Fill layer (PtFl)"
                )),
                "{what}: {observations}"
            );
        }
        let losses = import_report(&path)["verified_losses"].to_string();
        assert!(!losses.contains("PtFl"), "{what}: {losses}");
        let back = open(&path);
        let reopened = unsafe { &*back };
        assert_eq!(reopened.layer_count(), 3, "{what}");
        for (index, case) in [(1, LINKED), (2, UNLINKED)] {
            let d = description(back, index).expect("a description");
            assert_eq!(d["fill"], "pattern", "{what}");
            assert!(d.get("ink").is_none(), "{what}: no ink form");
            assert_eq!(
                norm(&d["pattern_fill"]),
                norm(&case.keys()),
                "{what} layer {index}"
            );
            let layer = reopened.layer(index).unwrap();
            assert_eq!(layer.reference(), case.reference, "{what} layer {index}");
            assert_eq!(layer.offset(), (0, 0), "{what} layer {index}");
            assert_eq!(
                reopened.cmyk_layer_samples(index),
                unsafe { &*doc }.cmyk_layer_samples(index),
                "{what} layer {index}: samples"
            );
        }
        let layer = reopened.layer(1).unwrap();
        assert!((layer.opacity() - 0.6).abs() < 1e-2, "{what}: opacity");
        let mask = layer.mask().expect("the mask");
        assert_eq!(
            mask.properties().offset,
            MASK_AT,
            "{what}: the mask's place"
        );
        assert_eq!(mask.plane().dimensions(), MASK_SIZE, "{what}");
        assert!(mask.plane().samples().iter().all(|v| *v == 65535), "{what}");
        free(back);
        free(doc);
    }
}

#[test]
fn a_cmyk_ptfl_whose_tile_is_missing_stays_a_raster_with_its_marker_losses() {
    let dir = TempDir::new().unwrap();
    for (depth, table) in [(8u32, b"8BIMPatt"), (16, b"8BIMPat2")] {
        let doc = cmyk_doc(depth);
        let path = dir.path().join(format!("fill{depth}.psd"));
        save_layers(doc, &path);
        let mut bytes = std::fs::read(&path).unwrap();
        let at = bytes
            .windows(8)
            .position(|w| w == table)
            .expect("the table");
        bytes[at + 4] = b'X';
        let crafted = dir.path().join(format!("missing{depth}.psd"));
        std::fs::write(&crafted, &bytes).unwrap();
        let losses = import_report(&crafted)["verified_losses"].to_string();
        for fragment in [
            "(“Fill 0”) has adjustment/fill metadata",
            "(“Fill 0”) declares its raster pixels irrelevant",
        ] {
            assert!(losses.contains(fragment), "{depth}: {fragment}: {losses}");
        }
        let back = open(&crafted);
        let reopened = unsafe { &*back };
        assert_eq!(
            reopened.layer_count(),
            3,
            "{depth}: both fills stay rasters"
        );
        assert!(description(back, 1).is_none(), "{depth}: no description");
        let layer = reopened.layer(1).unwrap();
        assert_eq!(layer.offset(), MASK_AT, "{depth}: its own rectangle");
        assert_eq!(
            layer.cmyk_source().unwrap().dimensions(),
            MASK_SIZE,
            "{depth}"
        );
        free(back);
        free(doc);
    }
}

#[test]
fn a_pixel_less_cmyk_ptfl_is_carried_or_omitted_with_its_reason() {
    let dir = TempDir::new().unwrap();
    for (depth, table) in [(8u32, b"8BIMPatt"), (16, b"8BIMPat2")] {
        let doc = cmyk_doc(depth);
        let path = dir.path().join(format!("fill{depth}.psd"));
        save_layers(doc, &path);
        // The masked fill rewritten pixel-less: no rectangle, each channel
        // but its `-2` a bare compression word.
        let less = pixel_less(&std::fs::read(&path).unwrap(), "Fill 0");
        let crafted = dir.path().join(format!("less{depth}.psd"));
        std::fs::write(&crafted, &less).unwrap();
        let report = import_report(&crafted);
        assert!(
            report["observations"]
                .to_string()
                .contains("(“Fill 0”) is a Photoshop Pattern Fill layer (PtFl) with no pixels;"),
            "{depth}: {}",
            report["observations"]
        );
        assert!(
            !report["verified_losses"].to_string().contains("PtFl"),
            "{depth}: {}",
            report["verified_losses"]
        );
        let back = open(&crafted);
        let reopened = unsafe { &*back };
        assert_eq!(reopened.layer_count(), 3, "{depth}");
        assert_eq!(
            norm(&description(back, 1).expect("a description")["pattern_fill"]),
            norm(&LINKED.keys()),
            "{depth}"
        );
        assert_eq!(
            reopened.cmyk_layer_samples(1),
            unsafe { &*doc }.cmyk_layer_samples(1),
            "{depth}: samples"
        );
        free(back);
        // Its tile gone: omitted, with its reason.
        let mut bytes = less;
        let at = bytes
            .windows(8)
            .position(|w| w == table)
            .expect("the table");
        bytes[at + 4] = b'X';
        let crafted = dir.path().join(format!("less-missing{depth}.psd"));
        std::fs::write(&crafted, &bytes).unwrap();
        let losses = import_report(&crafted)["verified_losses"].to_string();
        assert!(
            losses.contains("(“Fill 0”) is a Photoshop Pattern Fill layer (PtFl) whose pattern fill names a pattern the file does not carry; layered import omits it and its mask."),
            "{depth}: {losses}"
        );
        let back = open(&crafted);
        assert_eq!(unsafe { &*back }.layer_count(), 2, "{depth}: omitted");
        free(back);
        free(doc);
    }
}
