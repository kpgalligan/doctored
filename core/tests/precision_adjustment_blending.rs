//! Phase 10D: blend modes on 16-bit ADJUSTMENT layers
//! (designs/phase-10d-adjustment-blending-status.md). The typed renderer takes
//! the byte compositor's rule (`doc.rs::composite_adjustment_into`): the
//! adjusted colour is pushed through the layer's mode as the SOURCE against the
//! backdrop, then lerped back by `k = opacity * coverage`; alpha never changes
//! and a fully transparent backdrop pixel is untouched. Persistence is the
//! conditional native v37 / `LBLD` schema7 word — schema7 adds NO mode, only
//! the kind-3 target — and the 16-bit PSD carries the key both ways.
//!
//! The oracle is `out = cb + (B(cb, 1 - cb) - cb) * k` in f64 over an Invert
//! adjustment, with `B` the independent equations in
//! common/native_blend_oracle.rs. Threshold modes are asserted under the v34
//! band policy's WEAK form only (designs/phase-10d-threshold-modes-status.md):
//! an adjusted source is a COMPOSITED input, so no branch is guaranteed —
//! every pixel lands within 0.65 codes of ONE branch, never a mixture.
use doctored_core::doc::BlendMode;
use doctored_core::doc_mask::MaskProperties;
use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_precision::dr_precision_doc_info;
use doctored_core::ffi_precision_edit::{dr_precision_doc_edit, dr_precision_doc_edit_plan};
use doctored_core::ffi_psd::{
    dr_precision_doc_open_psd, dr_precision_doc_save_psd, dr_psd_import_report,
};
use doctored_core::icc::IccProfile;
use doctored_core::job_control::DrJobControl;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer as Layer};
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use doctored_core::precision_mask::{PrecisionMask, PrecisionPlane};
use serde_json::{json, Value};
use std::ffi::{CStr, CString};
use std::path::Path;
use std::ptr;
use tempfile::TempDir;

use crate::common;
use crate::identity;
use crate::native_blend_oracle;
use crate::psd16_fixture as fixture;
use common::{cpath, take_err_string};
use native_blend_oracle::{COLOR_BAND, DISSOLVE_BAND, HARD_MIX_BAND};

const LIMIT: u64 = REFERENCE_WORKING_BYTES;
const LAYERS: i32 = 0;
/// Every mode the native set admits, Normal first.
const ALL_MODES: [i32; 27] = [
    0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25,
    26,
];
/// Modes with a continuous `B` (separable or nonseparable HSL).
const CONTINUOUS: [i32; 23] = [
    0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 15, 18, 19, 20, 22, 23, 24, 25, 26,
];
/// The v34 threshold family; Dissolve (14) has its own all-or-nothing rule.
const PICK_MODES: [i32; 3] = [16, 17, 21];

// ----------------------------------------------------------- plumbing --

fn edit(
    doc: &PrecisionDocument,
    request: Value,
    limit: u64,
    control: Option<&DrJobControl>,
) -> Result<Option<PrecisionDocument>, String> {
    let request = CString::new(request.to_string()).unwrap();
    let mut err = ptr::null_mut();
    let result = unsafe {
        dr_precision_doc_edit(
            doc,
            request.as_ptr(),
            ptr::null(),
            0,
            limit,
            0,
            control.map_or(ptr::null(), |c| c),
            &mut err,
        )
    };
    if !err.is_null() {
        let message = unsafe { CStr::from_ptr(err) }
            .to_string_lossy()
            .into_owned();
        unsafe { dr_string_free(err) };
        assert!(result.is_null());
        return Err(message);
    }
    Ok((!result.is_null()).then(|| *unsafe { Box::from_raw(result) }))
}
fn request(index: usize, mode: i32) -> Value {
    json!({"op":"layer_blend","version":1,"layer":index,"blend_mode":mode})
}
fn changed(doc: &PrecisionDocument, index: usize, mode: i32) -> PrecisionDocument {
    edit(doc, request(index, mode), LIMIT, None)
        .unwrap()
        .unwrap()
}
fn layer(name: &str, pixels: Vec<[u16; 4]>, offset: i32, opacity: f32) -> Layer {
    Layer::raster(
        name,
        PrecisionImage::from_rgba16(pixels.len() as u32, 1, pixels, IccProfile::srgb(), None)
            .unwrap(),
        (offset, 0),
        opacity,
        true,
    )
    .unwrap()
}
fn invert() -> String {
    json!({"type":"adjust","op":"invert","params":{},"evaluation":"precision_v1"}).to_string()
}
fn adjustment(name: &str, opacity: f32, visible: bool) -> Layer {
    Layer::adjustment(name, &invert(), opacity, visible, &IccProfile::srgb()).unwrap()
}
fn blended(name: &str, opacity: f32, mode: i32) -> Layer {
    adjustment(name, opacity, true)
        .with_blend_mode(BlendMode::from_c(mode).unwrap())
        .unwrap()
}
fn document(width: u32, layers: Vec<Layer>) -> PrecisionDocument {
    PrecisionDocument::from_parts(
        width,
        1,
        IccProfile::srgb(),
        Metadata::default(),
        Resolution::default(),
        layers,
    )
    .unwrap()
}
fn mask(samples: Vec<u16>, density: f32) -> PrecisionMask {
    let mut properties = MaskProperties::at((0, 0));
    properties.density = density;
    PrecisionMask::new(
        PrecisionPlane::from_u16(samples.len() as u32, 1, samples, None).unwrap(),
        properties,
        true,
    )
    .unwrap()
}
fn placed_mask(samples: Vec<u16>, at: (i32, i32)) -> PrecisionMask {
    PrecisionMask::new(
        PrecisionPlane::from_u16(samples.len() as u32, 1, samples, None).unwrap(),
        MaskProperties::at(at),
        true,
    )
    .unwrap()
}
fn render(doc: &PrecisionDocument) -> Vec<[u16; 4]> {
    doc.render(LIMIT, 0, None).unwrap().to_rgba16().unwrap()
}
fn info(doc: &PrecisionDocument) -> Value {
    let mut err = ptr::null_mut();
    let out = unsafe { dr_precision_doc_info(doc, &mut err) };
    assert!(!out.is_null(), "{}", take_err_string(err));
    serde_json::from_str(&take_err_string(out)).unwrap()
}
/// The layer rows a REOPENED document must match, with the one field that is
/// deliberately not persisted taken out.
///
/// `id` is session-scoped stable layer identity (`doc_identity`), the same
/// rule `Guide::id` and `Channel::id` follow: the `.rz` carries no ids and
/// every reader MINTS, so a reopened document's rows differ there by design.
/// Everything else in the row is a persistence claim and is compared.
fn persisted_layers(doc: &PrecisionDocument) -> Value {
    Value::Array(
        info(doc)["layers"]
            .as_array()
            .unwrap()
            .iter()
            .map(|row| {
                let mut row = row.as_object().unwrap().clone();
                assert!(
                    row.remove("id")
                        .and_then(|v| v.as_u64())
                        .is_some_and(|id| id != 0),
                    "every layer row carries a non-zero id"
                );
                Value::Object(row)
            })
            .collect(),
    )
}
fn bytes(doc: &PrecisionDocument) -> Vec<u8> {
    doc.encode_native(LIMIT, 0, None).unwrap()
}
fn words(values: &[u32]) -> Vec<u8> {
    values.iter().flat_map(|v| v.to_le_bytes()).collect()
}
fn word(data: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(data[at..at + 4].try_into().unwrap())
}
fn tag_at(data: &[u8], tag: &[u8]) -> usize {
    data.windows(4).rposition(|v| v == tag).unwrap()
}
fn n(p: [u16; 4]) -> [f64; 4] {
    p.map(|v| f64::from(v) / 65535.)
}
fn rgb(p: [f64; 4]) -> [f64; 3] {
    [p[0], p[1], p[2]]
}

// ------------------------------------------------------------ oracle --

/// The Invert operator in f64: `adjusted = 1 - cb`, the same expression
/// `adjust.rs` evaluates in f32.
fn adjusted(cb: [f64; 3]) -> [f64; 3] {
    cb.map(|v| 1. - v)
}
/// `B(cb, adjusted)` for a continuous mode.
fn effective(mode: i32, cb: [f64; 3]) -> [f64; 3] {
    let cs = adjusted(cb);
    match mode {
        23..=26 => native_blend_oracle::hsl(mode, cb, cs),
        _ => std::array::from_fn(|c| native_blend_oracle::separable(mode, cb[c], cs[c])),
    }
}
/// The documented adjustment mix: `out = cb + (effective - cb) * k`, alpha
/// untouched.
fn mixed(cb: [f64; 4], eff: [f64; 3], k: f64) -> [f64; 4] {
    let mut out = cb;
    for c in 0..3 {
        out[c] = cb[c] + (eff[c] - cb[c]) * k;
    }
    out
}
fn expected(mode: i32, cb: [f64; 4], k: f64) -> [f64; 4] {
    mixed(cb, effective(mode, rgb(cb)), k)
}
/// The qualified finite-fixture allowance: only the transformed Dodge/Burn
/// divisors near an endpoint, the SAME neighbourhoods
/// `precision_four_blending.rs` and `precision_remaining_separable.rs` declare,
/// evaluated with the adjusted source `s = 65535 - b`. Every other channel of
/// every other mode is held to the ordinary half-code-plus-slack bound.
fn tolerance(mode: i32, code: u16) -> f64 {
    let b = u32::from(code);
    let s = 65535 - b;
    let dodge_burn = (mode == 10 && s >= 64225 && b > 0 && b <= 65535 - s)
        || (mode == 11 && s <= 1310 && b < 65535 && 65535 - b <= s);
    let vivid = mode == 18
        && ((s >= 64880 && b > 0 && b <= 2 * (65535 - s))
            || (s <= 655 && b < 65535 && 65535 - b <= 2 * s));
    if dodge_burn || vivid {
        1.6
    } else {
        0.65
    }
}
fn close(actual: [u16; 4], want: [f64; 4], tolerances: [f64; 3]) -> f64 {
    let mut maximum: f64 = 0.;
    for c in 0..4 {
        let error = (f64::from(actual[c]) - want[c] * 65535.).abs();
        assert!(
            error <= if c == 3 { 0.65 } else { tolerances[c] },
            "channel {c}: {actual:?}, {want:?}, error {error}"
        );
        maximum = maximum.max(error);
    }
    maximum
}
/// The two branch references for one threshold composite: branch 0 / branch 1
/// are `B = 0` / `B = 1` per channel for Hard Mix and the backdrop / adjusted
/// triple for Darker/Lighter Color. `oracle[c]` is the independent pick.
fn branches(mode: i32, cb: [f64; 4], k: f64) -> ([[f64; 4]; 2], [usize; 3], [f64; 3]) {
    let cs = adjusted(rgb(cb));
    let (pick, margin) = native_blend_oracle::threshold(mode, rgb(cb), cs);
    let (zero, one) = if mode == 21 {
        ([0.; 3], [1.; 3])
    } else {
        (rgb(cb), cs)
    };
    let oracle = std::array::from_fn(|c| usize::from(pick[c] == one[c] && zero[c] != one[c]));
    ([mixed(cb, zero, k), mixed(cb, one, k)], oracle, margin)
}
/// The WEAK form only: an adjusted source is a composited input, so no branch
/// is guaranteed. Every channel is within 0.65 codes of one branch and the
/// whole-triple modes never mix two triples. Returns (max code error against
/// the matched branch, took_oracle_branch).
fn assert_weak(
    actual: [u16; 4],
    refs: [[f64; 4]; 2],
    oracle: [usize; 3],
    whole: bool,
) -> (f64, bool) {
    let alpha_error = (f64::from(actual[3]) - refs[0][3] * 65535.).abs();
    assert!(alpha_error <= 0.65, "alpha: {actual:?}, {refs:?}");
    let mut maximum: f64 = alpha_error;
    let error = |c: usize, k: usize| (f64::from(actual[c]) - refs[k][c] * 65535.).abs();
    let ok: [[bool; 2]; 3] = std::array::from_fn(|c| [error(c, 0) <= 0.65, error(c, 1) <= 0.65]);
    let mut took_oracle = true;
    for c in 0..3 {
        assert!(
            ok[c][0] || ok[c][1],
            "weak form: channel {c} of {actual:?} is a mixture of {refs:?}"
        );
        took_oracle &= ok[c][oracle[c]];
    }
    if whole {
        assert!(
            (0..3).all(|c| ok[c][0]) || (0..3).all(|c| ok[c][1]),
            "weak form: {actual:?} mixes the two triples {refs:?}"
        );
    }
    for c in 0..3 {
        let k = if ok[c][oracle[c]] {
            oracle[c]
        } else {
            1 - oracle[c]
        };
        maximum = maximum.max(error(c, k));
    }
    (maximum, took_oracle)
}

/// Endpoints, the documented saturation neighbours of both divisor families
/// and a spread of interior codes.
fn codes() -> Vec<u16> {
    vec![
        0, 1, 2, 3, 17, 53, 54, 106, 107, 108, 128, 129, 130, 169, 170, 171, 212, 213, 214, 255,
        256, 257, 339, 340, 341, 655, 656, 1310, 1311, 16383, 16384, 32766, 32767, 32768, 32769,
        49151, 64225, 64879, 64880, 65364, 65365, 65366, 65427, 65428, 65429, 65532, 65533, 65534,
        65535,
    ]
}
/// One opaque backdrop pixel per code, with three different channel values so
/// the whole-triple modes see real triples.
fn grid() -> Vec<[u16; 4]> {
    codes()
        .into_iter()
        .map(|c| [c, 65535 - c, c.wrapping_add(173), 65535])
        .collect()
}

// -------------------------------------------------------------- render --

#[test]
fn adjustment_continuous_modes_match_the_independent_mix_at_two_opacities() {
    let base = grid();
    for opacity in [1.0f32, 0.625] {
        for mode in CONTINUOUS {
            let doc = document(
                base.len() as u32,
                vec![
                    layer("Backdrop", base.clone(), 0, 1.),
                    blended("Invert", opacity, mode),
                ],
            );
            let mut maximum: f64 = 0.;
            for (i, pixel) in render(&doc).into_iter().enumerate() {
                let want = expected(
                    mode,
                    n(base[i]),
                    crate::common::byte_unit(f64::from(opacity)),
                );
                maximum = maximum.max(close(
                    pixel,
                    want,
                    std::array::from_fn(|c| tolerance(mode, base[i][c])),
                ));
            }
            eprintln!("opacity {opacity} mode {mode} maximum code error: {maximum}");
        }
    }
}

/// An identity kernel is a no-op ONLY under Normal. Under any other mode the
/// adjusted colour — here equal to the backdrop — is still pushed through the
/// blend function against that backdrop, which is the classic "a default
/// Levels/Curves layer set to Multiply darkens" behaviour the byte
/// `doc.rs::composite_adjustment_into` has (it skips nothing).
#[test]
fn an_identity_kernel_still_blends_under_every_non_normal_mode() {
    let base = grid();
    let identity = json!({"type":"adjust","op":"bcs",
        "params":{"brightness":0.0,"contrast":0.0,"saturation":0.0},
        "evaluation":"precision_v1"})
    .to_string();
    for mode in CONTINUOUS {
        let top = Layer::adjustment("Identity", &identity, 1., true, &IccProfile::srgb())
            .unwrap()
            .with_blend_mode(BlendMode::from_c(mode).unwrap())
            .unwrap();
        let doc = document(
            base.len() as u32,
            vec![layer("Backdrop", base.clone(), 0, 1.), top],
        );
        let mut maximum: f64 = 0.;
        let mut moved = false;
        for (i, pixel) in render(&doc).into_iter().enumerate() {
            let cb = n(base[i]);
            // The identity kernel's output IS the backdrop, so `cs == cb`.
            let cs = rgb(cb);
            let eff: [f64; 3] = match mode {
                23..=26 => native_blend_oracle::hsl(mode, rgb(cb), cs),
                _ => std::array::from_fn(|c| native_blend_oracle::separable(mode, cb[c], cs[c])),
            };
            let bound = if matches!(mode, 10 | 11 | 18) {
                1.6
            } else {
                0.65
            };
            maximum = maximum.max(close(pixel, mixed(cb, eff, 1.), [bound; 3]));
            moved |= pixel[..3] != base[i][..3];
        }
        // Normal must stay the exact no-op the skip used to give; Multiply is
        // the witness that the skip no longer swallows a real blend.
        match mode {
            0 => assert!(!moved, "Normal moved an identity kernel's backdrop"),
            1 => assert!(moved, "Multiply left an identity kernel's backdrop alone"),
            _ => {}
        }
        eprintln!("identity kernel mode {mode} maximum code error: {maximum}");
    }
}

#[test]
fn adjustment_threshold_modes_take_one_branch_and_never_a_mixture() {
    let base = grid();
    for opacity in [1.0f32, 0.625] {
        for mode in PICK_MODES {
            let doc = document(
                base.len() as u32,
                vec![
                    layer("Backdrop", base.clone(), 0, 1.),
                    blended("Invert", opacity, mode),
                ],
            );
            let (mut maximum, mut agreed, mut smallest) = (0.0f64, 0usize, f64::INFINITY);
            let rendered = render(&doc);
            for (i, pixel) in rendered.iter().enumerate() {
                let (refs, oracle, margin) = branches(
                    mode,
                    n(base[i]),
                    crate::common::byte_unit(f64::from(opacity)),
                );
                let (error, took) = assert_weak(*pixel, refs, oracle, mode != 21);
                maximum = maximum.max(error);
                agreed += usize::from(took);
                if took {
                    smallest = smallest.min(margin.into_iter().fold(f64::INFINITY, f64::min));
                }
            }
            eprintln!(
                "opacity {opacity} mode {mode} (band {}): {agreed}/{} composited pixels took the oracle branch; maximum code error {maximum}, smallest agreeing margin {smallest}",
                if mode == 21 { HARD_MIX_BAND } else { COLOR_BAND },
                rendered.len()
            );
        }
    }
}

#[test]
fn adjustment_dissolve_shows_the_adjusted_pixel_or_the_backdrop_exactly() {
    let base = grid();
    for opacity in [1.0f32, 0.625] {
        let doc = document(
            base.len() as u32,
            vec![
                layer("Backdrop", base.clone(), 0, 1.),
                blended("Invert", opacity, 14),
            ],
        );
        let (mut shown, mut hidden, mut in_band) = (0usize, 0usize, 0usize);
        for (x, pixel) in render(&doc).into_iter().enumerate() {
            let backdrop = base[x];
            let inverted = [
                65535 - backdrop[0],
                65535 - backdrop[1],
                65535 - backdrop[2],
                backdrop[3],
            ];
            let k = crate::common::byte_unit(f64::from(opacity));
            let t = native_blend_oracle::dissolve_threshold(x as i64, 0);
            if (t - k).abs() < DISSOLVE_BAND {
                assert!(pixel == inverted || pixel == backdrop, "x {x}: {pixel:?}");
                in_band += 1;
            } else if t < k {
                assert_eq!(pixel, inverted, "x {x} shown");
                shown += 1;
            } else {
                assert_eq!(pixel, backdrop, "x {x} hidden");
                hidden += 1;
            }
        }
        eprintln!("dissolve opacity {opacity}: {shown} shown, {hidden} hidden, {in_band} in-band");
        assert!(shown > 0 && (opacity == 1.0 || hidden > 0));
    }
}

#[test]
fn adjustment_modes_hold_alpha_exact_over_every_native_mask_code_and_density() {
    let base: Vec<_> = (0..=u16::MAX)
        .map(|i| {
            [
                i,
                65535 - i,
                i.wrapping_add(173),
                [0, 1, 32769, 65535][i as usize % 4],
            ]
        })
        .collect();
    let doc = document(
        65536,
        vec![
            layer("Backdrop", base.clone(), 0, 1.),
            adjustment("Invert", 0.625, true).with_mask(Some(mask((0..=u16::MAX).collect(), 0.75))),
        ],
    );
    // The backdrop alone, for the fully transparent pixels no mode may touch.
    let untouched = render(&document(
        65536,
        vec![layer("Backdrop", base.clone(), 0, 1.)],
    ));
    for mode in ALL_MODES {
        // Mode 0 is the exact no-op: the edit returns None, so the unchanged
        // document IS the Normal case.
        let edited = edit(&doc, request(1, mode), LIMIT, None).unwrap();
        assert_eq!(edited.is_none(), mode == 0);
        let next = edited.unwrap_or_else(|| doc.clone());
        let rendered = render(&next);
        let mut maximum: f64 = 0.;
        for (i, pixel) in rendered.iter().enumerate() {
            // A fully transparent backdrop pixel is left ENTIRELY untouched,
            // RGB included, by every mode.
            if base[i][3] == 0 {
                assert_eq!(*pixel, untouched[i], "hidden {i}");
                continue;
            }
            assert_eq!(pixel[3], base[i][3], "alpha {i}");
            let k = crate::common::byte_unit(0.625) * (0.25 + 0.75 * i as f64 / 65535.);
            let cb = n(base[i]);
            match mode {
                14 => {
                    let inverted = [
                        65535 - base[i][0],
                        65535 - base[i][1],
                        65535 - base[i][2],
                        base[i][3],
                    ];
                    assert!(
                        *pixel == inverted || *pixel == base[i],
                        "dissolve {i}: {pixel:?}"
                    );
                }
                16 | 17 | 21 => {
                    let (refs, oracle, _) = branches(mode, cb, k);
                    maximum = maximum.max(assert_weak(*pixel, refs, oracle, mode != 21).0);
                }
                _ => {
                    maximum = maximum.max(close(
                        *pixel,
                        expected(mode, cb, k),
                        std::array::from_fn(|c| tolerance(mode, base[i][c])),
                    ));
                }
            }
        }
        eprintln!("masked mode {mode} maximum code error: {maximum}");
        assert_eq!(doc.layer(1).unwrap().blend_mode(), BlendMode::Normal);
    }
}

#[test]
fn a_mask_smaller_than_the_canvas_confines_a_blended_adjustment() {
    let base: Vec<[u16; 4]> = vec![[12345, 23456, 34567, 65535]; 5];
    let doc = document(
        5,
        vec![
            layer("Backdrop", base.clone(), 0, 1.),
            blended("Invert", 1., 1).with_mask(Some(placed_mask(vec![65535, 32768, 0], (1, 0)))),
        ],
    );
    let rendered = render(&doc);
    // Outside the mask's own extent the coverage is 0: the backdrop is exact.
    assert_eq!(rendered[0], base[0]);
    assert_eq!(rendered[4], base[4]);
    assert_eq!(rendered[3], base[3]);
    for (x, coverage) in [(1usize, 1.0f64), (2, 32768. / 65535.)] {
        close(rendered[x], expected(1, n(base[x]), coverage), [0.65; 3]);
    }
}

#[test]
fn a_blended_adjustment_inside_a_group_clip_unit_or_pass_through_matches_the_flat_stack() {
    let base: Vec<[u16; 4]> = (0..4)
        .map(|i| [1001 + i * 7000, 60000 - i * 9000, 30000 + i * 3, 65535])
        .collect();
    for mode in ALL_MODES {
        // An isolated group at opacity 1 in Normal is the flat stack.
        let flat = document(
            4,
            vec![
                layer("Backdrop", base.clone(), 0, 1.),
                blended("Invert", 1., mode),
            ],
        );
        let grouped = document(
            4,
            vec![Layer::isolated_normal(
                "G",
                vec![
                    layer("Backdrop", base.clone(), 0, 1.),
                    blended("Invert", 1., mode),
                ],
                1.,
                true,
            )
            .unwrap()],
        );
        assert_eq!(render(&grouped), render(&flat), "group mode {mode}");
        // A pass-through container hands its children the same accumulator.
        let passing = document(
            4,
            vec![
                layer("Backdrop", base.clone(), 0, 1.),
                Layer::pass_through("P", vec![blended("Invert", 1., mode)], 1., true).unwrap(),
            ],
        );
        assert_eq!(render(&passing), render(&flat), "pass-through mode {mode}");
        // A clipped blended adjustment over a raster base: the base renders
        // into its own buffer, the member composites there, and the unit
        // composites once. With an opaque full-canvas base and no member
        // alpha change that is again the flat stack.
        let clipped = document(
            4,
            vec![
                layer("Backdrop", base.clone(), 0, 1.),
                blended("Invert", 1., mode).with_clipped(true),
            ],
        );
        assert_eq!(render(&clipped), render(&flat), "clip mode {mode}");
    }
}

#[test]
fn a_hidden_blended_adjustment_contributes_nothing() {
    let base: Vec<[u16; 4]> = vec![[12345, 23456, 34567, 65535]; 3];
    let plain = document(3, vec![layer("Backdrop", base.clone(), 0, 1.)]);
    for mode in ALL_MODES {
        let doc = document(
            3,
            vec![
                layer("Backdrop", base.clone(), 0, 1.),
                adjustment("Invert", 1., false)
                    .with_blend_mode(BlendMode::from_c(mode).unwrap())
                    .unwrap(),
            ],
        );
        assert_eq!(render(&doc), render(&plain), "hidden mode {mode}");
        // Opacity 0 is the other identity.
        let zero = document(
            3,
            vec![
                layer("Backdrop", base.clone(), 0, 1.),
                blended("Invert", 0., mode),
            ],
        );
        assert_eq!(render(&zero), render(&plain), "zero-opacity mode {mode}");
    }
}

// ----------------------------------------------------------- strict FFI --

#[test]
fn every_admitted_mode_edits_an_adjustment_and_pass_through_refuses() {
    let base: Vec<[u16; 4]> = vec![[12345, 23456, 34567, 65535]; 2];
    let doc = document(
        2,
        vec![
            layer("Backdrop", base, 0, 1.),
            adjustment("Invert", 1., true),
        ],
    );
    for mode in 1..=26 {
        let next = edit(&doc, request(1, mode), LIMIT, None).unwrap().unwrap();
        assert_eq!(
            next.layer(1).unwrap().blend_mode(),
            BlendMode::from_c(mode).unwrap()
        );
        assert_eq!(info(&next)["layers"][1]["blend_mode"], Value::from(mode));
        assert_eq!(info(&next)["layers"][1]["kind"], Value::from("adjustment"));
        // The exact no-op returns None rather than a phantom undo step.
        assert!(edit(&next, request(1, mode), LIMIT, None)
            .unwrap()
            .is_none());
        // The plan is admitted for the same request.
        let json = CString::new(request(1, mode).to_string()).unwrap();
        let mut err = ptr::null_mut();
        let plan = unsafe { dr_precision_doc_edit_plan(&doc, json.as_ptr(), LIMIT, 0, &mut err) };
        assert!(!plan.is_null(), "{}", take_err_string(err));
        unsafe { dr_string_free(plan) };
        assert_eq!(
            next.adjustment_description(1),
            doc.adjustment_description(1)
        );
    }
    // Pass Through still requires a group.
    let message = match edit(&doc, request(1, 27), LIMIT, None) {
        Err(message) => message,
        Ok(_) => panic!("Pass Through on an adjustment must refuse"),
    };
    assert!(
        message.contains("adjustment layers or isolated groups")
            && message.contains("Pass Through requires a group"),
        "{message}"
    );
    for bad in [request(1, -1), request(1, 99), request(2, 1)] {
        assert!(edit(&doc, bad.clone(), LIMIT, None).is_err(), "{bad}");
    }
    assert!(!info(&doc)["unsupported_operations"]
        .as_array()
        .unwrap()
        .contains(&json!("blend_mode")));
    for target in ["raster", "group", "adjustment"] {
        assert!(
            doctored_core::precision_capability::require("layer_blend", target, 16).is_ok(),
            "{target}"
        );
    }
    assert!(doctored_core::precision_capability::require("layer_blend", "mask", 16).is_err());
}

#[test]
fn a_cancelled_blended_adjustment_render_releases_its_scratch() {
    let base: Vec<[u16; 4]> = vec![[12345, 23456, 34567, 65535]; 4096];
    let doc = document(
        4096,
        vec![layer("Backdrop", base, 0, 1.), blended("Invert", 0.625, 1)],
    );
    let control = DrJobControl::default();
    control.cancel();
    assert!(doc.render(LIMIT, 0, Some(&control)).is_err());
    assert_eq!(control.scratch_bytes(), 0);
    let cancel = DrJobControl::default();
    cancel.cancel();
    assert!(edit(&doc, request(1, 2), LIMIT, Some(&cancel)).is_err());
    assert_eq!(cancel.scratch_bytes(), 0);
    assert!(edit(&doc, request(1, 2), 1, None).is_err());
}

// -------------------------------------------------------------- native --

/// B(0), pass-through G{Inner masked (1), Member (2)}(3), Adj(4), Top(5).
fn native_fixture() -> PrecisionDocument {
    let px: Vec<[u16; 4]> = (0..4)
        .map(|i| {
            [
                1001 + i,
                2003 + i,
                3007 + i,
                [0, 3, 32768, 65535][i as usize],
            ]
        })
        .collect();
    document(
        4,
        vec![
            layer("B", px.clone(), 0, 1.),
            Layer::pass_through(
                "G",
                vec![
                    layer("Inner", px.clone(), 1, 0.625)
                        .with_mask(Some(placed_mask(vec![0, 123, 32769], (1, 0)))),
                    layer("Member", px.clone(), -1, 1.),
                ],
                0.875,
                true,
            )
            .unwrap(),
            adjustment("Adj", 1., true),
            layer("Top", px, 0, 0.5),
        ],
    )
}
fn outside(doc: &PrecisionDocument, index: usize) -> PrecisionDocument {
    let mask = doc.layer(index).unwrap().mask().unwrap();
    let mut p = mask.properties().clone();
    p.outside_coverage = 255;
    doc.set_layer_mask(
        index,
        Some(mask.with_properties(p, mask.enabled()).unwrap()),
        LIMIT,
        0,
        None,
    )
    .unwrap()
    .unwrap()
}
const TOP_STYLE: &str = r#"{"fill_opacity":0.5,"blend_if":{"channel":"gray","this_layer":[10,20,240,255]},"effects":[{"type":"drop_shadow"},{"type":"stroke","size":2}]}"#;
fn styled(doc: &PrecisionDocument, index: usize, style: Value) -> PrecisionDocument {
    edit(
        doc,
        json!({"op":"layer_style","version":1,"layer":index,"style":style}),
        LIMIT,
        None,
    )
    .unwrap()
    .unwrap()
}

#[test]
fn native37_is_chosen_only_for_an_adjustment_and_restores_the_prior_writer() {
    let plain = native_fixture();
    // 12E-1's identity wrapper (46) is the OUTERMOST typed word and its
    // `LIDS` table closes every file, so every chain below starts with it.
    assert_eq!(word(&bytes(&plain), 4), 46);
    assert_eq!(word(&bytes(&plain), 8), 13);
    // A raster-only Multiply is still the oldest sufficient wrapper.
    let raster = changed(&plain, 0, 1);
    assert_eq!(&bytes(&raster)[4..16], words(&[46, 26, 13]));
    // The same mode on the ADJUSTMENT record requires 37/schema7.
    let adjusted = changed(&plain, 4, 1);
    let data = bytes(&adjusted);
    assert_eq!(&data[4..16], words(&[46, 37, 13]));
    let tag = tag_at(&data, b"LBLD");
    assert_eq!(word(&data, tag + 4), 7, "schema");
    assert_eq!(word(&data, tag + 8), 1, "count");
    assert_eq!(word(&data, tag + 12), 4, "index");
    assert_eq!(word(&data, tag + 16), 1, "mode");
    assert_eq!(
        data.len(),
        tag + 20 + identity::identity_tail(identity::identity_records(&data)),
        "capacity is 16 + 8n, and the identity table closes the file"
    );
    assert_eq!(
        adjusted.native_plan().unwrap().encoded_bound,
        raster.native_plan().unwrap().encoded_bound,
        "the bound counts records, not schemas"
    );
    // A raster Dissolve BESIDE an adjustment Multiply still writes one 37.
    let both = changed(&adjusted, 0, 14);
    let data = bytes(&both);
    assert_eq!(&data[4..16], words(&[46, 37, 13]));
    let tag = tag_at(&data, b"LBLD");
    assert_eq!(word(&data, tag + 8), 2, "count");
    assert_eq!(
        &data[tag + 12..tag + 28],
        words(&[0, 14, 4, 1]),
        "ascending postorder"
    );
    // Releasing the last ADJUSTMENT mode restores the exact prior writer.
    let released = changed(&both, 4, 0);
    assert_eq!(&bytes(&released)[4..16], words(&[46, 34, 13]));
    assert_eq!(bytes(&changed(&released, 0, 0)), bytes(&plain));
    for doc in [&adjusted, &both] {
        let data = bytes(doc);
        let reopened = PrecisionDocument::parse_native(&data, LIMIT, 0, None).unwrap();
        assert_eq!(persisted_layers(&reopened), persisted_layers(doc));
        assert_eq!(render(&reopened), render(doc));
        assert_eq!(bytes(&reopened), data);
    }
}

#[test]
fn native37_composes_under_36_35_32_and_29_in_the_blend_slot() {
    let plain = native_fixture();
    let composed = edit(
        &outside(&changed(&plain, 4, 1), 1),
        json!({"op":"layer_clipped","layer":2,"clipped":true}),
        LIMIT,
        None,
    )
    .unwrap()
    .unwrap();
    let full = styled(&composed, 5, serde_json::from_str(TOP_STYLE).unwrap());
    let data = bytes(&full);
    assert_eq!(&data[4..32], words(&[46, 36, 35, 32, 29, 37, 13]));
    let at = |tag: &[u8]| tag_at(&data, tag);
    assert!(
        at(b"LBLD") < at(b"MOUT")
            && at(b"MOUT") < at(b"LCLP")
            && at(b"LCLP") < at(b"LBIF")
            && at(b"LBIF") < at(b"LSTY")
    );
    assert_eq!(word(&data, at(b"LBLD") + 4), 7, "schema");
    let reopened = PrecisionDocument::parse_native(&data, LIMIT, 0, None).unwrap();
    assert_eq!(reopened.layer(4).unwrap().blend_mode(), BlendMode::Multiply);
    assert!(reopened.layer(2).unwrap().clipped());
    assert_eq!(persisted_layers(&reopened), persisted_layers(&full));
    assert_eq!(render(&reopened), render(&full));
    assert_eq!(bytes(&reopened), data);
    // Releasing the adjustment's mode drops the blend word entirely.
    assert_eq!(
        &bytes(&changed(&full, 4, 0))[4..28],
        words(&[46, 36, 35, 32, 29, 13])
    );
}

#[test]
fn malformed_schema7_framing_refuses_before_any_png_decode() {
    let doc = changed(&native_fixture(), 4, 1);
    let data = bytes(&doc);
    let tag = tag_at(&data, b"LBLD");
    let idat = data.windows(4).position(|v| v == b"IDAT").unwrap();
    for (at, value, word) in [
        (tag + 4, 1, "schema"),
        (tag + 4, 2, "schema"),
        (tag + 4, 3, "schema"),
        (tag + 4, 4, "schema"),
        (tag + 4, 5, "schema"),
        (tag + 4, 6, "schema"),
        (tag + 4, 8, "schema"),
        (tag + 8, 0, "count"),
        (tag + 8, 7, "count"),
        (tag + 12, 6, "index"),
        (tag + 12, 3, "isolated"),
        (tag + 16, 27, "mode"),
        (tag + 16, 99, "mode"),
        (8, 34, "schema"),
        (12, 29, "body version"),
    ] {
        let mut bad = data.clone();
        bad[at..at + 4].copy_from_slice(&u32::to_le_bytes(value));
        bad[idat + 4] ^= 255;
        let error = PrecisionDocument::parse_native(&bad, LIMIT, 0, None)
            .err()
            .unwrap();
        assert!(error.contains(word), "{value} at {at}: {error}");
    }
    // A schema7 table under a 34 header refuses; so does a kind-3 entry under
    // every older schema.
    for (version, schema) in [(26u32, 1u32), (27, 2), (30, 3), (31, 4), (33, 5), (34, 6)] {
        let mut bad = data.clone();
        bad[8..12].copy_from_slice(&version.to_le_bytes());
        bad[tag + 4..tag + 8].copy_from_slice(&schema.to_le_bytes());
        bad[idat + 4] ^= 255;
        let error = PrecisionDocument::parse_native(&bad, LIMIT, 0, None)
            .err()
            .unwrap();
        assert!(
            error.contains("raster or isolated group") || error.contains("blend mode"),
            "{version}: {error}"
        );
    }
    for end in tag..data.len() {
        assert!(PrecisionDocument::parse_native(&data[..end], LIMIT, 0, None).is_err());
    }
    let mut trailing = data.clone();
    trailing.push(0);
    assert!(PrecisionDocument::parse_native(&trailing, LIMIT, 0, None).is_err());
    assert_eq!(bytes(&doc), data);
}

// ---------------------------------------------------------- 16-bit PSD --

fn report(path: &Path) -> Value {
    let mut error = ptr::null_mut();
    let json = unsafe { dr_psd_import_report(cpath(path).as_ptr(), &mut error) };
    assert!(!json.is_null(), "{}", take_err_string(error));
    serde_json::from_str(&take_err_string(json)).expect("report JSON")
}
fn open16(path: &Path) -> (Value, PrecisionDocument) {
    let notice = report(path);
    let losses: Vec<&str> = notice["verified_losses"]
        .as_array()
        .expect("verified_losses")
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    assert!(
        !losses
            .iter()
            .any(|l| l.contains("16-bit adjustment layer cannot represent")),
        "{losses:?}"
    );
    let expected = CString::new(notice["source_fingerprint"].as_str().unwrap()).unwrap();
    let mut error = ptr::null_mut();
    let document = unsafe {
        dr_precision_doc_open_psd(
            cpath(path).as_ptr(),
            LAYERS,
            expected.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut error,
        )
    };
    assert!(!document.is_null(), "{}", take_err_string(error));
    (notice, *unsafe { Box::from_raw(document) })
}

#[test]
fn a_blended_adjustment_round_trips_through_sixteen_bit_psd() {
    let directory = TempDir::new().expect("tempdir");
    let base: Vec<[u16; 4]> = (0..4)
        .map(|i| [1001 + i * 9000, 60000 - i * 7000, 30000 + i * 5, 65535])
        .collect();
    let source = document(
        4,
        vec![layer("Base", base.clone(), 0, 1.), blended("Invert", 1., 1)],
    );
    let projection = source.render(LIMIT, 0, None).unwrap();
    let path = directory.path().join("adjustment-blend16.psd");
    let mut error = ptr::null_mut();
    assert!(
        unsafe {
            dr_precision_doc_save_psd(
                &source,
                &projection,
                cpath(&path).as_ptr(),
                0,
                LAYERS,
                LIMIT,
                0,
                &mut error,
            )
        },
        "{}",
        take_err_string(error)
    );
    assert!(error.is_null());
    let file = fixture::read(&std::fs::read(&path).unwrap());
    let keys: Vec<[u8; 4]> = file.records.iter().map(|r| r.blend).collect();
    assert_eq!(
        keys,
        [*b"norm", *b"mul "],
        "the key rides the pixel-less record"
    );
    let (_, back) = open16(&path);
    let layers = info(&back)["layers"].as_array().unwrap().clone();
    assert_eq!(layers.len(), 2);
    assert_eq!(layers[1]["kind"], Value::from("adjustment"));
    assert_eq!(layers[1]["blend_mode"], Value::from(1));
    assert_eq!(back.layer(1).unwrap().blend_mode(), BlendMode::Multiply);
    assert_eq!(
        back.render(LIMIT, 0, None).unwrap().to_rgba16().ok(),
        projection.to_rgba16().ok()
    );
    // A PSD import marks record 0 (word 113, `background_native`) first.
    assert_eq!(&bytes(&back)[8..12], &113u32.to_le_bytes());
    assert_eq!(&bytes(&back)[12..16], &37u32.to_le_bytes());
}
