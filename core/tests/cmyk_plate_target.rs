//! ONE INK PLATE AS A SCALAR TARGET on a CMYK document (#1, the
//! remaining-gaps batch: `cmyk_plate_scalar`), through `dr_precision_doc_edit`,
//! `dr_precision_doc_read_plane` and `dr_precision_doc_scalar_statistics` with
//! `"target":"cyan"|"magenta"|"yellow"|"black"`. Every expectation is this
//! file's own `1 − ink` arithmetic on the stored samples (`cmyk_layer_samples`):
//! a plate reads `65535 − ink`; a Fill, a stroke, `replace_canvas` and an
//! Invert lerp that number exactly as on an RGB plane and store `65535 −` the
//! result; the other plates and the alpha stay bit-identical; the histogram's
//! sums are the plate's numbers; locks, non-raster records and an RGB
//! document refuse by name; the CMYK table lists the new families; and a
//! `luminance` selection from one layer (Apply Image's and Calculations'
//! source) is that layer's composite alone.

use std::ffi::{CStr, CString};
use std::ptr;

use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_precision_edit::{
    dr_precision_doc_read_plane, dr_precision_doc_scalar_statistics, dr_precision_doc_select,
};
use doctored_core::precision_capability::require_cmyk;
use doctored_core::precision_document::PrecisionDocument;
use serde_json::{json, Value};

use crate::cmyk_structure_fixture::{
    cmyk_layers, free, plan, refusal, then, try_edit, varied, with_levels, RgbLayer, LIMIT,
};
use crate::common::take_err_string;

const W: u32 = 9;
const H: u32 = 7;
const PLATES: [&str; 4] = ["cyan", "magenta", "yellow", "black"];
/// The layer rect inside the canvas: 6 × 5 at (2, 1).
const LW: u32 = 6;
const LH: u32 = 5;
const OFFSET: (i32, i32) = (2, 1);

/// A CMYK document of one varied raster, smaller than the canvas and offset.
fn doc() -> *mut PrecisionDocument {
    cmyk_layers(
        W,
        H,
        vec![RgbLayer {
            name: "Ink",
            w: LW,
            h: LH,
            offset: OFFSET,
            pixels: varied(LW, LH, 3),
        }],
    )
}

fn samples(doc: *const PrecisionDocument) -> Vec<[u16; 5]> {
    unsafe { &*doc }.cmyk_layer_samples(0).unwrap()
}

/// The plate's numbers, `65535 − ink`.
fn numbers(before: &[[u16; 5]], k: usize) -> Vec<u16> {
    before.iter().map(|p| 65535 - p[k]).collect()
}

/// Canvas coverage at layer pixel `i`.
fn canvas_at(coverage: &[u16], i: usize) -> u16 {
    let (x, y) = (
        i as i32 % LW as i32 + OFFSET.0,
        i as i32 / LW as i32 + OFFSET.1,
    );
    coverage[(y as u32 * W + x as u32) as usize]
}

/// A canvas-sized ramp of selection or stroke weights.
fn ramp(seed: u32) -> Vec<u16> {
    (0..W * H)
        .map(|i| match (i + seed) % 4 {
            0 => 0,
            1 => 65535,
            2 => 21845,
            _ => 40000,
        })
        .collect()
}

/// The scalar lerp of `original` toward `value` at weight `t` (the RGB
/// plane's rule, restated).
fn lerp(original: u16, value: f64, t: f64) -> u16 {
    if t == 0.0 {
        return original;
    }
    (f64::from(original) * (1.0 - t) + value * t)
        .clamp(0.0, 65535.0)
        .round() as u16
}

/// Asserts plate `k` of `after` is `65535 − want` and every other component
/// equals `before`'s.
fn assert_plate(before: &[[u16; 5]], after: &[[u16; 5]], k: usize, want: &[u16], what: &str) {
    for (i, (b, a)) in before.iter().zip(after).enumerate() {
        for c in 0..5 {
            let expected = if c == k { 65535 - want[i] } else { b[c] };
            assert_eq!(a[c], expected, "{what}: pixel {i} component {c}");
        }
    }
}

fn read_plane(doc: *const PrecisionDocument, target: &str, canvas: bool) -> Vec<u16> {
    let request = json!({"op":"read_plane","target":target,"index":0,"canvas":canvas});
    let planned = plan(doc, &request.to_string()).unwrap();
    let mut out = vec![999u16; planned["output_samples"].as_u64().unwrap() as usize];
    let text = CString::new(request.to_string()).unwrap();
    let mut err = ptr::null_mut();
    let ok = unsafe {
        dr_precision_doc_read_plane(
            doc,
            text.as_ptr(),
            out.as_mut_ptr(),
            out.len(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(ok, "read {target}: {}", take_err_string(err));
    out
}

fn statistics(
    doc: *const PrecisionDocument,
    target: &str,
    coverage: &[u16],
) -> Result<Value, String> {
    let request = json!({"op":"scalar_statistics","target":target,"index":0,"selection":!coverage.is_empty()});
    let text = CString::new(request.to_string()).unwrap();
    let mut err = ptr::null_mut();
    let raw = unsafe {
        dr_precision_doc_scalar_statistics(
            doc,
            text.as_ptr(),
            if coverage.is_empty() {
                ptr::null()
            } else {
                coverage.as_ptr()
            },
            coverage.len(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    if raw.is_null() {
        return Err(take_err_string(err));
    }
    let value = serde_json::from_str(unsafe { CStr::from_ptr(raw) }.to_str().unwrap()).unwrap();
    unsafe { dr_string_free(raw) };
    Ok(value)
}

fn scalar(target: &str, selection: bool, operation: Value) -> String {
    json!({"op":"scalar_edit","target":target,"index":0,"selection":selection,"operation":operation})
        .to_string()
}

#[test]
fn a_plate_reads_as_one_minus_its_ink() {
    let doc = doc();
    let before = samples(doc);
    for (k, plate) in PLATES.iter().enumerate() {
        assert_eq!(
            read_plane(doc, plate, false),
            numbers(&before, k),
            "{plate}"
        );
        // On the canvas, outside the layer rect there is no ink: 65535.
        let placed = read_plane(doc, plate, true);
        for y in 0..H as i32 {
            for x in 0..W as i32 {
                let (lx, ly) = (x - OFFSET.0, y - OFFSET.1);
                let want = if lx < 0 || ly < 0 || lx >= LW as i32 || ly >= LH as i32 {
                    65535
                } else {
                    65535 - before[(ly as u32 * LW + lx as u32) as usize][k]
                };
                assert_eq!(
                    placed[(y as u32 * W + x as u32) as usize],
                    want,
                    "{plate} ({x},{y})"
                );
            }
        }
    }
    free(doc);
}

#[test]
fn a_plate_read_plan_charges_the_cmyk_rect_copy() {
    // The read holds the layer rect's CMYKA16 copy (10 B a pixel) beside the
    // u16 output: 4 MiB bookkeeping + output × 4 + rect × 10, plus rect × 2
    // for the canvas placement.
    const MIB: u64 = 1024 * 1024;
    let doc = doc();
    let rect = u64::from(LW * LH);
    let canvas = u64::from(W * H);
    for plate in PLATES {
        for (placed, output, extra) in [(false, rect, 0), (true, canvas, rect * 2)] {
            let request = json!({"op":"read_plane","target":plate,"index":0,"canvas":placed});
            let planned = plan(doc, &request.to_string()).unwrap();
            assert_eq!(
                planned["scratch_bytes"].as_u64().unwrap(),
                4 * MIB + output * 4 + rect * 10 + extra,
                "{plate} canvas {placed}"
            );
        }
    }
    free(doc);
}

#[test]
fn a_fill_on_a_plate_is_the_scalar_lerp_of_one_minus_ink() {
    let doc = doc();
    let before = samples(doc);
    let selection = ramp(1);
    for (k, plate) in PLATES.iter().enumerate() {
        // Whole plate, opaque: every number becomes the value.
        let value = 12_345u16;
        let request = scalar(plate, false, json!({"kind":"fill","value":value}));
        let filled = try_edit(doc, &request, &[]).unwrap();
        assert!(!filled.is_null(), "{plate}");
        let want = vec![value; before.len()];
        assert_plate(&before, &samples(filled), k, &want, plate);
        free(filled);
        // Through a selection at opacity 0.6.
        let request = scalar(
            plate,
            true,
            json!({"kind":"fill","value":50_000,"opacity":0.6}),
        );
        let filled = try_edit(doc, &request, &selection).unwrap();
        let want: Vec<u16> = numbers(&before, k)
            .iter()
            .enumerate()
            .map(|(i, &l)| {
                lerp(
                    l,
                    50_000.0,
                    f64::from(canvas_at(&selection, i)) / 65535.0 * 0.6,
                )
            })
            .collect();
        assert_plate(&before, &samples(filled), k, &want, plate);
        free(filled);
    }
    free(doc);
}

#[test]
fn a_stroke_and_replace_canvas_on_a_plate_follow_their_canvas_payload() {
    let doc = doc();
    let before = samples(doc);
    let stroke = ramp(2);
    let k = 1;
    let painted = then(
        cmyk_layers(
            W,
            H,
            vec![RgbLayer {
                name: "Ink",
                w: LW,
                h: LH,
                offset: OFFSET,
                pixels: varied(LW, LH, 3),
            }],
        ),
        &scalar(
            "magenta",
            false,
            json!({"kind":"paint","value":0,"opacity":0.5}),
        ),
        &stroke,
    );
    let want: Vec<u16> = numbers(&before, k)
        .iter()
        .enumerate()
        .map(|(i, &l)| lerp(l, 0.0, f64::from(canvas_at(&stroke, i)) / 65535.0 * 0.5))
        .collect();
    assert_plate(&before, &samples(painted), k, &want, "paint");
    free(painted);
    // Apply Image's writeback: the canvas plane mapped onto the layer rect.
    let source: Vec<u16> = (0..W * H).map(|i| (i * 1031 % 65536) as u16).collect();
    let replaced = try_edit(
        doc,
        &scalar("black", false, json!({"kind":"replace_canvas"})),
        &source,
    )
    .unwrap();
    let want: Vec<u16> = (0..before.len()).map(|i| canvas_at(&source, i)).collect();
    assert_plate(&before, &samples(replaced), 3, &want, "replace_canvas");
    free(replaced);
    free(doc);
}

#[test]
fn an_invert_on_a_plate_inverts_one_minus_ink() {
    let doc = doc();
    let before = samples(doc);
    let description =
        json!({"type":"adjust","evaluation":"precision_v1","op":"invert","params":{}}).to_string();
    let operation =
        json!({"kind":"filter","edit":{"layer":0,"op":"adjustment","description":description}});
    let inverted = try_edit(doc, &scalar("yellow", false, operation), &[]).unwrap();
    assert!(!inverted.is_null());
    let want: Vec<u16> = numbers(&before, 2).iter().map(|l| 65535 - l).collect();
    assert_plate(&before, &samples(inverted), 2, &want, "invert");
    free(inverted);
    free(doc);
}

#[test]
fn a_plate_edit_that_changes_nothing_is_none() {
    let doc = doc();
    let request = scalar(
        "cyan",
        false,
        json!({"kind":"fill","value":1,"opacity":0.0}),
    );
    assert!(try_edit(doc, &request, &[]).unwrap().is_null());
    // Replacing a plate with its own numbers changes nothing either.
    let own = read_plane(doc, "cyan", true);
    let request = scalar("cyan", false, json!({"kind":"replace_canvas"}));
    assert!(try_edit(doc, &request, &own).unwrap().is_null());
    free(doc);
}

#[test]
fn a_plates_histogram_counts_its_numbers() {
    let doc = doc();
    let before = samples(doc);
    for (k, plate) in PLATES.iter().enumerate() {
        let report = statistics(doc, plate, &[]).unwrap();
        let sum: u64 = numbers(&before, k)
            .iter()
            .map(|v| u64::from(*v) * 65535)
            .sum();
        assert_eq!(report["weighted_code_sums_rgba"][0], sum, "{plate}");
        assert_eq!(report["target"], *plate);
        assert_eq!(report["scope"]["x"], OFFSET.0);
        assert_eq!(report["scope"]["width"], LW);
    }
    // A selection weights the plate's numbers by canvas coverage.
    let selection = ramp(3);
    let report = statistics(doc, "black", &selection).unwrap();
    let sum: u64 = numbers(&before, 3)
        .iter()
        .enumerate()
        .map(|(i, v)| u64::from(*v) * u64::from(canvas_at(&selection, i)))
        .sum();
    assert_eq!(report["weighted_code_sums_rgba"][0], sum);
    // A mask's and an alpha channel's histograms are admitted too; red is not.
    assert_eq!(
        statistics(doc, "red", &[]).unwrap_err(),
        "The \"scalar_statistics\" edit of anything but a layer mask, an alpha channel or an ink plate is unavailable in a CMYK document"
    );
    free(doc);
}

#[test]
fn plate_targets_refuse_by_name() {
    let doc = doc();
    let fill = scalar("cyan", false, json!({"kind":"fill","value":0}));
    // Lock Pixels refuses a plate edit, as it refuses an RGB plane's.
    let locked = then(doc, r#"{"op":"layer_lock","layer":0,"locks":2}"#, &[]);
    assert_eq!(
        refusal(locked, &fill, &[]),
        "high-depth layer 0 is locked (pixels)"
    );
    // An adjustment record has no plates.
    let adjusted = with_levels(locked, 1.0);
    let request = json!({"op":"scalar_edit","target":"magenta","index":1,"operation":{"kind":"fill","value":0}});
    assert_eq!(
        refusal(adjusted, &request.to_string(), &[]),
        "a CMYK ink plate requires a CMYK raster layer"
    );
    assert_eq!(
        plan(
            adjusted,
            &json!({"op":"read_plane","target":"black","index":1}).to_string()
        )
        .unwrap_err(),
        "a CMYK ink plate requires a CMYK raster layer"
    );
    free(adjusted);
    // An RGB document has no plates.
    let rgb = Box::into_raw(Box::new(crate::native_scalar::doc(
        2,
        2,
        vec![[1, 2, 3, 65535]; 4],
    )));
    assert_eq!(
        refusal(rgb, &fill, &[]),
        "a CMYK ink plate requires a CMYK raster layer"
    );
    free(rgb);
}

#[test]
fn the_cmyk_table_lists_the_plate_and_channel_families() {
    for plate in PLATES {
        for family in [
            "fill_stroke",
            "read_plane",
            "paint",
            "erase",
            "adjustment_apply",
        ] {
            assert!(require_cmyk(family, plate).is_ok(), "{family} {plate}");
        }
    }
    assert!(require_cmyk("adjustment_apply", "channel").is_ok());
    assert_eq!(
        require_cmyk("geometry_affine", "cyan").unwrap_err(),
        "CMYK geometry_affine does not support target cyan"
    );
}

fn select(doc: *const PrecisionDocument, request: &Value) -> Vec<u16> {
    let planned = plan(doc, &request.to_string()).unwrap();
    let mut out = vec![0u16; planned["output_samples"].as_u64().unwrap() as usize];
    let text = CString::new(request.to_string()).unwrap();
    let mut err = ptr::null_mut();
    let ok = unsafe {
        dr_precision_doc_select(
            doc,
            text.as_ptr(),
            ptr::null(),
            0,
            out.as_mut_ptr(),
            out.len(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(ok, "{request}: {}", take_err_string(err));
    out
}

#[test]
fn a_layers_luminance_is_that_layer_composited_alone() {
    // Two opaque full-canvas layers: the composite is the top one, but the
    // bottom layer's own luminance is the luminance of a document holding
    // only that layer.
    let layer = |name, seed| RgbLayer {
        name,
        w: W,
        h: H,
        offset: (0, 0),
        pixels: varied(W, H, seed)
            .into_iter()
            .map(|p| [p[0], p[1], p[2], 65535])
            .collect(),
    };
    let both = cmyk_layers(W, H, vec![layer("A", 5), layer("B", 9)]);
    let alone = cmyk_layers(W, H, vec![layer("A", 5)]);
    let from_layer = select(
        both,
        &json!({"op":"selection","source":"layer","layer":0,"operation":{"kind":"luminance"}}),
    );
    let composite = select(
        alone,
        &json!({"op":"selection","operation":{"kind":"luminance"}}),
    );
    assert_eq!(from_layer, composite);
    let top = select(
        both,
        &json!({"op":"selection","operation":{"kind":"luminance"}}),
    );
    assert_ne!(from_layer, top);
    free(both);
    free(alone);
}
