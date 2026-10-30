//! Stack Modes in a CMYK document, through
//! `dr_precision_doc_edit` / `dr_precision_doc_edit_plan`'s `stack_mode`
//! request.
//!
//! The reference is `common/stack_oracle.rs` run in the stored `L = 65535 −
//! ink` domain: per member, `[L_c, L_m, L_y, α]` and `[L_k, L_k, L_k, α]`,
//! each statistic then mapped back as `ink = 65535 − L`; a pixel no member
//! covers is paper at α 0. Every mode but those with an f64 step
//! (`REAL_STEP_MODES`) is compared exactly, those within one code. The
//! members are read back from the document itself, never from the core's
//! arithmetic.

use doctored_core::precision_document::PrecisionDocument;
use doctored_core::stack_stat::{plan_scratch, OUTPUT_PLANE_COPIES_8};
use serde_json::json;

use crate::cmyk_structure_fixture::{
    cmyk_layers, free, plan, refusal, then, try_edit, varied, with_levels, RgbLayer,
};
use crate::stack_oracle::{self, Member, MODES, REAL_STEP_MODES};

const CW: u32 = 12;
const CH: u32 = 10;

/// Offset and size of the four members; each reaches past the canvas on a
/// different side, and none covers their union.
const SHAPES: [((i32, i32), (u32, u32)); 4] = [
    ((1, 1), (8, 7)),
    ((-3, 2), (9, 6)),
    ((5, -4), (10, 9)),
    ((2, 3), (4, 12)),
];

fn stack(n: usize) -> *mut PrecisionDocument {
    const NAMES: [&str; 4] = ["L0", "L1", "L2", "L3"];
    let layers = (0..n)
        .map(|i| {
            let ((ox, oy), (w, h)) = SHAPES[i];
            RgbLayer {
                name: NAMES[i],
                w,
                h,
                offset: (ox, oy),
                pixels: varied(w, h, 7 + i as u32 * 13),
            }
        })
        .collect();
    cmyk_layers(CW, CH, layers)
}

fn request(layers: &[usize], above: usize, mode: &str) -> String {
    json!({"op": "stack_mode", "layers": layers, "above": above, "mode": mode, "name": "Stat"})
        .to_string()
}

fn doc<'a>(handle: *const PrecisionDocument) -> &'a PrecisionDocument {
    unsafe { &*handle }
}

/// A record's stored ink, offset and size.
type Stored = (Vec<[u16; 5]>, (i32, i32), (u32, u32));

/// Record `index`'s stored ink, offset and size.
fn ink(handle: *const PrecisionDocument, index: usize) -> Stored {
    let record = doc(handle).layer(index).unwrap();
    let dims = record.cmyk_source().expect("a CMYK raster").dimensions();
    let samples = doc(handle).cmyk_layer_samples(index).unwrap();
    (samples, record.offset(), dims)
}

/// The oracle's two `L` views of record `index`: the C/M/Y pass and the K
/// pass.
fn passes(handle: *const PrecisionDocument, index: usize) -> (Member, Member) {
    let (samples, offset, (width, height)) = ink(handle, index);
    let l = |v: u16| f64::from(65535 - v);
    let cmy = samples
        .iter()
        .map(|s| [l(s[0]), l(s[1]), l(s[2]), f64::from(s[4])])
        .collect();
    let k = samples
        .iter()
        .map(|s| [l(s[3]), l(s[3]), l(s[3]), f64::from(s[4])])
        .collect();
    (
        Member {
            offset,
            width,
            height,
            pixels: cmy,
        },
        Member {
            offset,
            width,
            height,
            pixels: k,
        },
    )
}

#[test]
fn every_mode_matches_the_oracle_in_the_l_domain() {
    for n in 2..=4usize {
        let handle = stack(n);
        let layers: Vec<usize> = (0..n).collect();
        let (cmy, k): (Vec<Member>, Vec<Member>) =
            layers.iter().map(|&i| passes(handle, i)).unzip();
        let extent = stack_oracle::extent(&cmy);
        for mode in MODES {
            let out = then(
                Box::into_raw(Box::new(doc(handle).clone())),
                &request(&layers, n - 1, mode),
                &[],
            );
            assert_eq!(doc(out).layer_count(), n + 1);
            assert!(doc(out).is_cmyk());
            let (got, offset, dims) = ink(out, n);
            assert_eq!(offset, (extent.0, extent.1), "{mode}, {n}: origin");
            assert_eq!(dims, (extent.2, extent.3), "{mode}, {n}: size");
            let a = stack_oracle::integer(mode, &cmy, 65535);
            let b = stack_oracle::integer(mode, &k, 65535);
            let slack = i64::from(REAL_STEP_MODES.contains(&mode));
            let mut covered = 0;
            for (i, (g, (a, b))) in got.iter().zip(a.iter().zip(&b)).enumerate() {
                assert_eq!(u64::from(g[4]), a[3], "{mode}, {n}, pixel {i}: alpha");
                assert_eq!(a[3], b[3], "{mode}, {n}, pixel {i}: the passes agree");
                if a[3] == 0 {
                    assert_eq!(*g, [0; 5], "{mode}, {n}, pixel {i}: paper");
                    continue;
                }
                covered += 1;
                let want = [65535 - a[0], 65535 - a[1], 65535 - a[2], 65535 - b[1]];
                for c in 0..4 {
                    assert!(
                        (i64::from(g[c]) - want[c] as i64).abs() <= slack,
                        "{mode}, {n}, pixel {i} ink {c}: {} against {}",
                        g[c],
                        want[c]
                    );
                }
            }
            assert!(covered > 40, "{mode}, {n}: only {covered} covered pixels");
            assert!(covered < got.len(), "{mode}, {n}: some pixel stays paper");
            let visible: Vec<bool> = (0..=n)
                .map(|i| doc(out).layer(i).unwrap().visible())
                .collect();
            let mut want = vec![false; n];
            want.push(true);
            assert_eq!(visible, want, "{mode}, {n}: only the members are hidden");
            free(out);
        }
        free(handle);
    }
}

#[test]
fn refusals_name_their_reason_in_plan_and_edit() {
    let handle = stack(3);
    let both = |handle: *const PrecisionDocument, req: &str| {
        let message = refusal(handle, req, &[]);
        assert_eq!(
            plan(handle, req).unwrap_err(),
            message,
            "{req}: the plan agrees"
        );
        message
    };
    assert_eq!(
        both(handle, &request(&[1], 1, "mean")),
        "stack mode requires at least two layers"
    );
    assert_eq!(
        both(handle, &request(&[0, 9], 9, "mean")),
        "stack mode layer 9 is out of range"
    );
    assert_eq!(
        both(handle, &request(&[0, 1], 9, "mean")),
        "stack mode cannot insert above layer 9: it is out of range"
    );
    let message = both(handle, &request(&[0, 1], 1, "trimmed_mean"));
    assert!(
        message.starts_with("invalid stack mode request") && message.contains("unknown variant"),
        "{message}"
    );
    let grouped = try_edit(
        handle,
        &json!({"op": "group_layers", "layers": [0, 1], "name": "G", "pass_through": false})
            .to_string(),
        &[],
    )
    .unwrap();
    assert!(!grouped.is_null());
    // Postorder: L0 0, L1 1, G 2, L2 3.
    assert_eq!(
        both(grouped, &request(&[0, 2], 2, "mean")),
        "Stack Mode requires raster layers; layer 2 is a group"
    );
    free(grouped);
    let adjusted = with_levels(Box::into_raw(Box::new(doc(handle).clone())), 1.0);
    assert_eq!(
        both(adjusted, &request(&[0, 3], 3, "mean")),
        "Stack Mode requires raster layers; layer 3 is an adjustment layer"
    );
    free(adjusted);
    free(handle);
}

#[test]
fn the_plan_charges_two_pass_rows_per_member_at_ten_bytes() {
    let handle = stack(4);
    let layers = [0, 1, 2, 3];
    let (cmy, _): (Vec<Member>, Vec<Member>) = layers.iter().map(|&i| passes(handle, i)).unzip();
    let extent = stack_oracle::extent(&cmy);
    let (pixels, width) = (
        u64::from(extent.2) * u64::from(extent.3),
        u64::from(extent.2),
    );
    let req = request(&layers, 3, "median");
    let planned = plan(handle, &req).unwrap();
    assert_eq!(planned["input_samples"], json!(0));
    let scratch = planned["scratch_bytes"].as_u64().unwrap();
    // `precision_structure::StructureEdit::scratch_bytes`' structure term,
    // restated: 4 MiB, every name twice, 4096 a record (no alpha channel).
    let names: u64 = (0..doc(handle).layer_count())
        .map(|i| doc(handle).layer(i).unwrap().name().len() as u64)
        .sum();
    let structure = 4 * 1024 * 1024 + ("Stat".len() as u64 + names) * 2 + 4 * 4096;
    // The 8-bit factor's planes of 10-byte samples, 2 × 4 + 1 rows, 1 MiB.
    let written =
        OUTPUT_PLANE_COPIES_8 * pixels * 10 + (2 * 4 + 1) * width * 10 + structure + 1024 * 1024;
    assert_eq!(scratch, written);
    assert_eq!(plan_scratch(pixels, width, 10, 8, structure), Some(scratch));

    // The plan is the threshold.
    let total = planned["retained_bytes"].as_u64().unwrap() + scratch;
    let at = |limit: u64| {
        let text = std::ffi::CString::new(req.clone()).unwrap();
        let mut err = std::ptr::null_mut();
        let out = unsafe {
            doctored_core::ffi_precision_edit::dr_precision_doc_edit(
                handle,
                text.as_ptr(),
                std::ptr::null(),
                0,
                limit,
                0,
                std::ptr::null(),
                &mut err,
            )
        };
        let error = (!err.is_null()).then(|| crate::common::take_err_string(err));
        if !out.is_null() {
            free(out);
        }
        error
    };
    assert!(at(total - 1).unwrap().contains("working limit"));
    assert_eq!(at(total), None);
    free(handle);
}
