//! Four threshold native modes (Dissolve, Darker Color, Lighter Color, Hard
//! Mix) through the unchanged f32 blend.rs kernels, persisted by conditional
//! native v34 / LBLD schema6 and carried by 16-bit PSD, under the v34
//! strong/weak band policy (designs/phase-10d-threshold-modes-status.md):
//! direct inputs with a margin at or above the band take the independent f64
//! oracle's branch; every pixel lands within 0.65 codes of ONE of the two
//! branch references, never a mixture. The oracle is the independent f64 set
//! in common/native_blend_oracle.rs.
use doctored_core::doc::BlendMode;
use doctored_core::doc_mask::MaskProperties;
use doctored_core::ffi::dr_string_free;
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
use crate::native_blend_oracle;
use crate::psd16_fixture as fixture;
use common::{cpath, take_err_string};
use fixture::Record;
use native_blend_oracle::{COLOR_BAND, DISSOLVE_BAND, HARD_MIX_BAND};

use crate::identity;

/// Every native encode below goes through here. Phase 12E-1 made the
/// always-written identity wrapper the OUTERMOST `.rz` word and its `LIDS`
/// table the file's last block; this file's subject is an EARLIER wrapper,
/// so the outer one is asserted and then peeled, leaving byte-for-byte what
/// the writer these tests were written against produced.
fn encoded(doc: &PrecisionDocument) -> Vec<u8> {
    identity::typed_prior_body(&doc.encode_native(LIMIT, 0, None).unwrap())
}

const LIMIT: u64 = REFERENCE_WORKING_BYTES;
const MODES: [i32; 4] = [14, 16, 17, 21];
const PICK_MODES: [i32; 3] = [16, 17, 21];
const LAYERS: i32 = 0;

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
fn render(doc: &PrecisionDocument) -> Vec<[u16; 4]> {
    doc.render(LIMIT, 0, None).unwrap().to_rgba16().unwrap()
}
fn n(p: [u16; 4]) -> [f64; 4] {
    p.map(|v| f64::from(v) / 65535.)
}
fn rgb(p: [f64; 4]) -> [f64; 3] {
    [p[0], p[1], p[2]]
}
/// W3C source-over with an explicitly chosen `B(Cb, Cs)` triple.
fn over_with(b: [f64; 4], s: [f64; 4], weight: f64, blended: [f64; 3]) -> [f64; 4] {
    let sa = s[3] * weight;
    let alpha = sa + b[3] * (1. - sa);
    if alpha == 0. {
        return [0.; 4];
    }
    let mut out = [0.; 4];
    out[3] = alpha;
    for c in 0..3 {
        out[c] = (sa * ((1. - b[3]) * s[c] + b[3] * blended[c]) + (1. - sa) * b[3] * b[c]) / alpha;
    }
    out
}
/// W3C source-over with the oracle's threshold pick evaluated on the layer
/// colors; mode 0 is plain Normal for group-internal composition.
fn over(b: [f64; 4], s: [f64; 4], weight: f64, mode: i32) -> [f64; 4] {
    let blended = if mode == 0 {
        rgb(s)
    } else {
        native_blend_oracle::threshold(mode, rgb(b), rgb(s)).0
    };
    over_with(b, s, weight, blended)
}
/// The two branch references for one composite: branch 0 / branch 1 are
/// `B = 0` / `B = 1` per channel for Hard Mix and the backdrop / source
/// triple for Darker/Lighter Color. `oracle[c]` is the oracle's pick.
fn branches(
    mode: i32,
    b: [f64; 4],
    s: [f64; 4],
    weight: f64,
) -> ([[f64; 4]; 2], [usize; 3], [f64; 3]) {
    let (pick, margin) = native_blend_oracle::threshold(mode, rgb(b), rgb(s));
    let (zero, one) = if mode == 21 {
        ([0.; 3], [1.; 3])
    } else {
        (rgb(b), rgb(s))
    };
    let oracle = std::array::from_fn(|c| usize::from(pick[c] == one[c] && zero[c] != one[c]));
    (
        [over_with(b, s, weight, zero), over_with(b, s, weight, one)],
        oracle,
        margin,
    )
}
fn band(mode: i32) -> f64 {
    if mode == 21 {
        HARD_MIX_BAND
    } else {
        COLOR_BAND
    }
}
/// `refs` = the two fully composited f64 RGBA references (branch 0 / branch
/// 1); `oracle` = the oracle's pick per channel; `whole` = the pick is one
/// triple (Darker/Lighter Color) rather than per channel (Hard Mix). Returns
/// (max code error against the matched branch, took_oracle_branch).
/// `direct && margin >= band` asserts the oracle branch (strong form);
/// otherwise asserts one of the two branches, never a mixture (weak form).
fn assert_policy(
    actual: [u16; 4],
    refs: [[f64; 4]; 2],
    oracle: [usize; 3],
    margin: [f64; 3],
    band: f64,
    direct: bool,
    whole: bool,
) -> (f64, bool) {
    let mut maximum: f64 = 0.;
    let alpha_error = (f64::from(actual[3]) - refs[0][3] * 65535.).abs();
    assert!(
        alpha_error <= 0.65 && (refs[0][3] - refs[1][3]).abs() < 1e-12,
        "alpha: {actual:?}, {refs:?}"
    );
    maximum = maximum.max(alpha_error);
    let error = |c: usize, k: usize| (f64::from(actual[c]) - refs[k][c] * 65535.).abs();
    let ok: [[bool; 2]; 3] = std::array::from_fn(|c| [error(c, 0) <= 0.65, error(c, 1) <= 0.65]);
    let mut took_oracle = true;
    for c in 0..3 {
        if direct && margin[c] >= band {
            assert!(
                ok[c][oracle[c]],
                "strong form: channel {c} of {actual:?} is not within 0.65 of branch {} {refs:?} (margin {})",
                oracle[c],
                margin[c]
            );
        } else {
            assert!(
                ok[c][0] || ok[c][1],
                "weak form: channel {c} of {actual:?} is a mixture of {refs:?}"
            );
        }
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
/// Weak-form bookkeeping for composited pixels: how often the oracle's branch
/// was taken and the smallest margins seen with agreement and disagreement.
#[derive(Default)]
struct Composited {
    pixels: usize,
    agreed: usize,
    agreed_margin: Option<f64>,
    disagreed_margin: Option<f64>,
}
impl Composited {
    fn record(&mut self, margin: [f64; 3], took_oracle: bool) {
        let margin = margin.into_iter().fold(f64::INFINITY, f64::min);
        self.pixels += 1;
        let slot = if took_oracle {
            self.agreed += 1;
            &mut self.agreed_margin
        } else {
            &mut self.disagreed_margin
        };
        *slot = Some(slot.map_or(margin, |m: f64| m.min(margin)));
    }
    fn report(&self, label: &str) {
        eprintln!(
            "{label}: {} composited pixels, {} took the oracle branch; smallest agreeing margin {:?}, smallest disagreeing margin {:?}",
            self.pixels, self.agreed, self.agreed_margin, self.disagreed_margin
        );
    }
}

/// The HSL fixture's opaque endpoint, gray and asymmetric triples.
fn colors() -> Vec<[u16; 4]> {
    let mut colors = Vec::new();
    for bits in 0..8u16 {
        colors.push([
            65535 * (bits & 1),
            65535 * ((bits >> 1) & 1),
            65535 * ((bits >> 2) & 1),
            65535,
        ]);
    }
    for gray in [1u16, 17, 32767, 32768, 65534] {
        colors.push([gray, gray, gray, 65535]);
    }
    colors.extend(asymmetric());
    colors
}
fn asymmetric() -> Vec<[u16; 4]> {
    [
        [12345u16, 34567, 56789],
        [65535, 40000, 0],
        [7000, 50000, 20000],
        [60000, 30000, 1],
        [1, 20000, 65535],
        [45000, 10000, 30000],
        [65535, 58000, 51000],
        [200, 7100, 14100],
        [32767, 25000, 40000],
        [50000, 21000, 65000],
    ]
    .into_iter()
    .map(|[r, g, b]| [r, g, b, 65535])
    .collect()
}
/// Whether the authored codes put a channel inside the band by construction:
/// Hard Mix when the codes sum to exactly 65535; Darker/Lighter Color when
/// `|30 dR + 59 dG + 11 dB| <= 6` hundredths of a code (the luma difference
/// `|L| / (100 * 65535)` crosses 2^-20 between |L| = 6 and |L| = 7).
fn intended_in_band(mode: i32, base: [u16; 4], source: [u16; 4]) -> [bool; 3] {
    if mode == 21 {
        std::array::from_fn(|c| u32::from(base[c]) + u32::from(source[c]) == 65535)
    } else {
        let d = |c: usize| i64::from(base[c]) - i64::from(source[c]);
        [(30 * d(0) + 59 * d(1) + 11 * d(2)).abs() <= 6; 3]
    }
}

#[test]
fn hard_mix_exhaustive_direct_codes_meet_the_band_policy() {
    let base: Vec<[u16; 4]> = (0..=u16::MAX).map(|x| [x, x, x, 65535]).collect();
    let mut zero_picks = 0usize;
    for (shift, name) in [
        (65535i32, "exact sum"),
        (65534, "one below"),
        (65536, "one above"),
    ] {
        let source: Vec<[u16; 4]> = (0..=u16::MAX)
            .map(|x| {
                let s = shift - i32::from(x);
                if (0..=65535).contains(&s) {
                    [s as u16, s as u16, s as u16, 65535]
                } else {
                    [0, 0, 0, 0]
                }
            })
            .collect();
        let doc = document(
            65536,
            vec![
                layer("Backdrop", base.clone(), 0, 1.),
                layer("Source", source.clone(), 0, 1.),
            ],
        );
        let mut maximum: f64 = 0.;
        let mut in_band = 0usize;
        for (i, p) in render(&changed(&doc, 1, 21)).into_iter().enumerate() {
            if source[i][3] == 0 {
                assert_eq!(p, base[i]);
                continue;
            }
            let (refs, oracle, margin) = branches(21, n(base[i]), n(source[i]), 1.);
            assert!(p[..3].iter().all(|&v| v == 0 || v == 65535), "{p:?}");
            match shift {
                65535 => {
                    assert!(margin.iter().all(|&m| m < HARD_MIX_BAND));
                    in_band += 1;
                    zero_picks += usize::from(p[0] == 0);
                }
                65534 => {
                    assert!(margin.iter().all(|&m| m >= HARD_MIX_BAND));
                    assert_eq!(p, [0, 0, 0, 65535]);
                }
                _ => {
                    assert!(margin.iter().all(|&m| m >= HARD_MIX_BAND));
                    assert_eq!(p, [65535; 4]);
                }
            }
            let (error, _) = assert_policy(p, refs, oracle, margin, HARD_MIX_BAND, true, false);
            maximum = maximum.max(error);
        }
        eprintln!("hard mix {name}: maximum code error {maximum}, {in_band} in-band pixels");
    }
    eprintln!("hard mix exact sum: {zero_picks} of 65536 in-band pixels picked zero");
}

#[test]
fn threshold_modes_grid_at_opacity_1_and_0_625() {
    let codes = [
        0, 1, 2, 3, 17, 53, 54, 106, 107, 108, 169, 170, 171, 212, 213, 214, 255, 256, 257, 339,
        340, 341, 655, 656, 16383, 16384, 32766, 32767, 32768, 32769, 49151, 64879, 64880, 65364,
        65365, 65366, 65427, 65428, 65429, 65532, 65533, 65534, 65535,
    ];
    let mut base = Vec::new();
    let mut source = Vec::new();
    for &b in &codes {
        for &s in &codes {
            base.push([b, s, 65535 - b, 65535]);
            source.push([s, b, 65535 - s, 65535]);
        }
    }
    for &b in &asymmetric() {
        for &s in &asymmetric() {
            base.push(b);
            source.push(s);
        }
    }
    for opacity in [1f32, 0.625] {
        let doc = document(
            base.len() as u32,
            vec![
                layer("Backdrop", base.clone(), 0, 1.),
                layer("Source", source.clone(), 0, opacity),
            ],
        );
        for mode in PICK_MODES {
            let mut maximum: f64 = 0.;
            let mut in_band = 0usize;
            let mut in_band_agreed = 0usize;
            for (i, p) in render(&changed(&doc, 1, mode)).into_iter().enumerate() {
                let (refs, oracle, margin) = branches(
                    mode,
                    n(base[i]),
                    n(source[i]),
                    crate::common::byte_unit(f64::from(opacity)),
                );
                let intended = intended_in_band(mode, base[i], source[i]);
                for c in 0..3 {
                    assert_eq!(
                        margin[c] < band(mode),
                        intended[c],
                        "mode {mode} pixel {i} channel {c}: margin {}",
                        margin[c]
                    );
                }
                let (error, took_oracle) =
                    assert_policy(p, refs, oracle, margin, band(mode), true, mode != 21);
                if intended.iter().any(|&v| v) {
                    in_band += 1;
                    in_band_agreed += usize::from(took_oracle);
                }
                maximum = maximum.max(error);
            }
            eprintln!(
                "grid mode {mode} opacity {opacity}: maximum code error {maximum}; {in_band} in-band pixels, {in_band_agreed} of them on the oracle branch"
            );
        }
    }
}

#[test]
fn threshold_modes_every_native_mask_code_partial_alpha_density_and_opacity() {
    let colors = colors();
    let base: Vec<_> = (0..=u16::MAX)
        .map(|i| {
            let mut p = colors[i as usize % colors.len()];
            p[3] = [0, 1, 32769, 65535][i as usize % 4];
            p
        })
        .collect();
    let source: Vec<_> = (0..=u16::MAX)
        .map(|i| {
            let mut p = colors[(i as usize * 7 + 3) % colors.len()];
            p[3] = [65535, 32768, 1, 0, 45001][i as usize % 5];
            p
        })
        .collect();
    let doc = document(
        65536,
        vec![
            layer("Base", base.clone(), 0, 1.),
            layer("Ink", source.clone(), 0, 0.625)
                .with_mask(Some(mask((0..=u16::MAX).collect(), 0.75))),
        ],
    );
    for mode in PICK_MODES {
        let next = changed(&doc, 1, mode);
        assert!(PrecisionImage::ptr_eq(
            doc.layer_source(1).unwrap(),
            next.layer_source(1).unwrap()
        ));
        let mut strong: f64 = 0.;
        let mut weak: f64 = 0.;
        let mut composited = Composited::default();
        for (i, p) in render(&next).into_iter().enumerate() {
            let (b, s) = (n(base[i]), n(source[i]));
            let weight = crate::common::byte_unit(0.625) * (0.25 + 0.75 * i as f64 / 65535.);
            let (refs, oracle, margin) = branches(mode, b, s, weight);
            let direct = base[i][3] == 65535;
            let (error, took_oracle) =
                assert_policy(p, refs, oracle, margin, band(mode), direct, mode != 21);
            if direct {
                strong = strong.max(error);
            } else {
                weak = weak.max(error);
                composited.record(margin, took_oracle);
            }
        }
        eprintln!("mask mode {mode}: maximum code error direct {strong}, composited {weak}");
        composited.report(&format!("mask mode {mode}"));
        assert_eq!(
            next.layer_source(1).unwrap().to_rgba16().ok().unwrap(),
            source
        );
        assert_eq!(doc.layer(1).unwrap().blend_mode(), BlendMode::Normal);
    }
}

#[test]
fn threshold_modes_translated_nested_groups_and_pass_through() {
    let b = [12345, 34567, 56789, 32123];
    let a = [51234, 22223, 34567, 43210];
    let z = [22444, 44555, 11666, 54321];
    for mode in PICK_MODES {
        let inner = Layer::isolated_normal(
            "Inner",
            vec![
                layer("A", vec![a; 3], -1, 0.75),
                layer("Z", vec![z; 2], 1, 0.5),
            ],
            0.625,
            true,
        )
        .unwrap()
        .with_blend_mode(BlendMode::from_c(mode).unwrap())
        .unwrap()
        .with_mask(Some(mask(vec![1, 32769, 65535], 1.)));
        let outer = Layer::pass_through("Outer", vec![inner], 0.875, true).unwrap();
        let doc = document(3, vec![layer("B", vec![b; 3], 0, 1.), outer]);
        let result = render(&doc);
        let mut maximum: f64 = 0.;
        let mut composited = Composited::default();
        // Darker/Lighter Color equal Normal wherever the oracle picks the
        // source triple; then the isolated render equals Pass Through.
        let mut normal_like = mode != 21;
        for x in 0..3 {
            let first = if x < 2 {
                over([0.; 4], n(a), crate::common::byte_unit(0.75), 0)
            } else {
                [0.; 4]
            };
            let group = if x >= 1 {
                over(first, n(z), crate::common::byte_unit(0.5), 0)
            } else {
                first
            };
            let weight = crate::common::byte_unit(0.625) * [1., 32769., 65535.][x] / 65535.;
            let (after, oracle, margin) = branches(mode, n(b), group, weight);
            normal_like &= oracle == [1; 3];
            // Both references go through the same Pass Through mixing.
            let refs = after.map(|after| {
                let alpha = n(b)[3] * (1. - crate::common::byte_unit(0.875))
                    + after[3] * crate::common::byte_unit(0.875);
                let mut expected = [0.; 4];
                expected[3] = alpha;
                for c in 0..3 {
                    expected[c] = (n(b)[c] * n(b)[3] * (1. - crate::common::byte_unit(0.875))
                        + after[c] * after[3] * crate::common::byte_unit(0.875))
                        / alpha;
                }
                expected
            });
            let (error, took_oracle) = assert_policy(
                result[x],
                refs,
                oracle,
                margin,
                band(mode),
                false,
                mode != 21,
            );
            composited.record(margin, took_oracle);
            maximum = maximum.max(error);
        }
        eprintln!("group mode {mode}: maximum code error {maximum}");
        composited.report(&format!("group mode {mode}"));
        let passing = changed(&doc, 3, 27);
        if !normal_like {
            assert_ne!(render(&passing), result);
        }
        assert_eq!(render(&changed(&passing, 3, mode)), result);
        let renamed = edit(
            &doc,
            json!({"op":"rename_layer","layer":1,"name":"Edited child"}),
            LIMIT,
            None,
        )
        .unwrap()
        .unwrap();
        assert_eq!(renamed.layer(3).unwrap().blend_mode().to_c(), mode);
        let doubled = renamed.duplicate_layer(2).unwrap().unwrap();
        assert_eq!(doubled.layer(4).unwrap().blend_mode().to_c(), mode);
    }

    // The nested-group Hard Mix witness: two exact authored group sources
    // straddle the discontinuity by only 1/65535², inside the band by
    // construction. The edit is admitted; the weak form holds (the output is
    // one branch, never a mixture) and no branch is guaranteed.
    for alpha in [32767u16, 32768] {
        let denominator = 65535u64.pow(2);
        let numerator = 65534u64 * 65535 + 2 * u64::from(alpha);
        assert_eq!(numerator >= denominator, alpha == 32768);
        assert_eq!(numerator.abs_diff(denominator), 1);
        let exact_sum = 65534. / 65535. + 2. * f64::from(alpha) / denominator as f64;
        assert_eq!(exact_sum >= 1., alpha == 32768);
        let group_color = 2. * f64::from(alpha) / denominator as f64;
        let (_, margin) =
            native_blend_oracle::threshold(21, [65534. / 65535.; 3], [group_color; 3]);
        assert!(margin.iter().all(|&m| m < HARD_MIX_BAND), "{margin:?}");
        let group = Layer::isolated_normal(
            "Threshold group",
            vec![
                layer("Black", vec![[0, 0, 0, 65535]], 0, 1.),
                layer("Two codes", vec![[2, 2, 2, alpha]], 0, 1.),
            ],
            1.,
            true,
        )
        .unwrap();
        let doc = document(
            1,
            vec![
                layer("Backdrop", vec![[65534, 65534, 65534, 65535]], 0, 1.),
                group,
            ],
        );
        let before = render(&doc);
        let on_group = render(&changed(&doc, 3, 21))[0];
        assert!(on_group[..3].iter().all(|&v| v == 0 || v == 65535));
        assert_eq!(on_group[3], 65535);
        eprintln!(
            "hard mix witness alpha {alpha} (exact sum {} 1): rendered {on_group:?}",
            if exact_sum >= 1. { ">=" } else { "<" }
        );
        assert_eq!(render(&changed(&doc, 0, 21)), before);
    }
    let backdrop = 65534f32 / 65535.;
    let group_below = (2f32 / 65535.) * (32767f32 / 65535.);
    assert_eq!(f64::from(backdrop) + f64::from(group_below), 1.);
}

#[test]
fn dissolve_shows_exact_source_or_untouched_backdrop_by_the_canvas_hash() {
    let colors = colors();
    let width = 4096u32;
    let backdrop: Vec<[u16; 4]> = (0..width)
        .map(|x| {
            // Never the source pixel's colour, so shown and hidden differ.
            let mut p = colors[(x as usize + 1) % colors.len()];
            p[3] = [65535, 32769, 1, 0][x as usize % 4];
            p
        })
        .collect();
    let source: Vec<[u16; 4]> = (0..width)
        .map(|x| {
            let mut p = colors[x as usize % colors.len()];
            p[3] = [65535, 32768, 1, 0, 45001, 60000][x as usize % 6];
            p
        })
        .collect();
    let codes: Vec<u16> = (0..width).map(|x| (x * 37 % 65536) as u16).collect();
    let coverage = |x: usize| 0.25 + 0.75 * f64::from(codes[x]) / 65535.;
    let without = render(&document(
        width,
        vec![layer("Backdrop", backdrop.clone(), 0, 1.)],
    ));
    let mut patterns = Vec::new();
    for opacity in [1f32, 0.625] {
        for offset in [0i32, 1] {
            let doc = document(
                width,
                vec![
                    layer("Backdrop", backdrop.clone(), 0, 1.),
                    layer("Dissolve", source.clone(), offset, opacity)
                        .with_blend_mode(BlendMode::Dissolve)
                        .unwrap()
                        .with_mask(Some(mask(codes.clone(), 0.75))),
                ],
            );
            let result = render(&doc);
            let mut shown_pattern = Vec::new();
            let (mut shown, mut hidden, mut in_band) = (0usize, 0usize, 0usize);
            let mut smallest_strong_margin = f64::INFINITY;
            for x in 0..width as usize {
                let t = native_blend_oracle::dissolve_threshold(x as i64, 0);
                let sa = (x as i32 - offset >= 0)
                    .then(|| source[x - offset as usize])
                    .map_or(0., |s| {
                        n(s)[3] * crate::common::byte_unit(f64::from(opacity)) * coverage(x)
                    });
                let expected_shown = t < sa;
                let margin = (sa - t).abs();
                let shown_reference = (x as i32 - offset >= 0).then(|| {
                    let s = source[x - offset as usize];
                    [s[0], s[1], s[2], 65535]
                });
                let is_shown = shown_reference.is_some_and(|r| r == result[x]);
                let is_hidden = result[x] == without[x];
                assert!(!(is_shown && is_hidden), "x {x}: ambiguous fixture");
                assert!(
                    is_shown || is_hidden,
                    "x {x}: {:?} is neither the exact source nor the untouched backdrop",
                    result[x]
                );
                if sa > 0. && margin >= DISSOLVE_BAND {
                    assert_eq!(
                        is_shown, expected_shown,
                        "x {x}: strong form (sa {sa}, t {t}, margin {margin})"
                    );
                    smallest_strong_margin = smallest_strong_margin.min(margin);
                } else if sa > 0. {
                    in_band += 1;
                }
                if sa == 0. {
                    assert!(is_hidden);
                }
                shown += usize::from(is_shown);
                hidden += usize::from(is_hidden);
                shown_pattern.push(is_shown);
            }
            assert!(shown > 0 && hidden > 0);
            eprintln!(
                "dissolve raster opacity {opacity} offset {offset}: {shown} shown, {hidden} hidden, {in_band} in-band, smallest strong margin {smallest_strong_margin}"
            );
            patterns.push(shown_pattern);
        }
    }
    assert_ne!(patterns[0], patterns[1], "translation changes the pattern");
    assert_ne!(patterns[2], patterns[3]);

    // A Dissolve isolated group: its accumulated alpha is composited, so only
    // the weak form is claimed; the shown colour is the group's f32 colour.
    let a = [51234, 22223, 34567, 43210];
    let z = [22444, 44555, 11666, 54321];
    for opacity in [1f32, 0.625] {
        let inner = Layer::isolated_normal(
            "Inner",
            vec![
                layer("A", vec![a; width as usize], -1, 0.75),
                layer("Z", vec![z; width as usize], 1, 0.5),
            ],
            opacity,
            true,
        )
        .unwrap()
        .with_blend_mode(BlendMode::Dissolve)
        .unwrap()
        .with_mask(Some(mask(codes.clone(), 0.75)));
        let doc = document(
            width,
            vec![layer("Backdrop", backdrop.clone(), 0, 1.), inner],
        );
        let result = render(&doc);
        let mut composited = Composited::default();
        let mut maximum: f64 = 0.;
        let mut shown = 0usize;
        for x in 0..width as usize {
            let first = if x + 1 < width as usize {
                over([0.; 4], n(a), crate::common::byte_unit(0.75), 0)
            } else {
                [0.; 4]
            };
            let group = if x >= 1 {
                over(first, n(z), crate::common::byte_unit(0.5), 0)
            } else {
                first
            };
            let sa = group[3] * crate::common::byte_unit(f64::from(opacity)) * coverage(x);
            let t = native_blend_oracle::dissolve_threshold(x as i64, 0);
            let is_hidden = result[x] == without[x];
            let is_shown = result[x][3] == 65535
                && (0..3).all(|c| (f64::from(result[x][c]) - group[c] * 65535.).abs() <= 0.65);
            assert!(is_shown || is_hidden, "x {x}: {:?}", result[x]);
            if is_shown && !is_hidden {
                shown += 1;
                for c in 0..3 {
                    maximum = maximum.max((f64::from(result[x][c]) - group[c] * 65535.).abs());
                }
            }
            composited.record([(sa - t).abs(); 3], (is_shown && !is_hidden) == (t < sa));
        }
        assert!(shown > 0 && shown < width as usize);
        eprintln!(
            "dissolve group opacity {opacity}: {shown} shown, maximum shown colour error {maximum}"
        );
        composited.report(&format!("dissolve group opacity {opacity}"));
    }
}

#[test]
fn threshold_mode_v34_downgrades_exactly_and_old_schemas_refuse_new_meaning() {
    let base = document(
        1,
        vec![
            layer("A", vec![[30001, 30002, 30003, 0]], -1, 0.5),
            layer("B", vec![[12345, 54321, 23456, 1]], 0, 1.),
        ],
    );
    let normal = encoded(&base);
    let basic = changed(&base, 0, 1);
    let v26 = encoded(&basic);
    let separable = changed(&basic, 1, 3);
    let v27 = encoded(&separable);
    let four = changed(&basic, 1, 10);
    let v30 = encoded(&four);
    let five = changed(&basic, 1, 13);
    let v31 = encoded(&five);
    let hsl = changed(&basic, 1, 23);
    let v33 = encoded(&hsl);
    assert_eq!(&v33[4..8], &33u32.to_le_bytes());
    for mode in MODES {
        let new = changed(&hsl, 1, mode);
        let bytes = encoded(&new);
        assert_eq!(&bytes[4..8], &34u32.to_le_bytes());
        let tag = bytes.windows(4).rposition(|v| v == b"LBLD").unwrap();
        assert_eq!(&bytes[tag + 4..tag + 8], &6u32.to_le_bytes());
        assert_eq!(&bytes[8..tag], &v33[8..tag]);
        assert_eq!(bytes.len(), v33.len());
        assert_eq!(
            new.native_plan().unwrap().encoded_bound,
            hsl.native_plan().unwrap().encoded_bound
        );
        let reopened = PrecisionDocument::parse_native(&bytes, LIMIT, 0, None).unwrap();
        assert_eq!(encoded(&reopened), bytes);
        assert_eq!(reopened.layer(1).unwrap().blend_mode().to_c(), mode);
        for (older, expected) in [(23, &v33), (13, &v31), (10, &v30), (3, &v27)] {
            assert_eq!(&encoded(&changed(&reopened, 1, older)), expected);
        }
        let back = changed(&reopened, 1, 0);
        assert_eq!(encoded(&back), v26);
        assert_eq!(encoded(&changed(&back, 0, 0)), normal);
        for index in 0..2 {
            assert_eq!(
                reopened.layer_source(index).unwrap().to_rgba16().ok(),
                base.layer_source(index).unwrap().to_rgba16().ok()
            );
        }
        // Every historical schema stays frozen when current eligibility expands.
        for (version, schema) in [(26u32, 1u32), (27, 2), (30, 3), (31, 4), (33, 5)] {
            let mut bad = bytes.clone();
            bad[4..8].copy_from_slice(&version.to_le_bytes());
            bad[tag + 4..tag + 8].copy_from_slice(&schema.to_le_bytes());
            let idat = bad.windows(4).position(|v| v == b"IDAT").unwrap();
            bad[idat + 4] ^= 255;
            assert!(PrecisionDocument::parse_native(&bad, LIMIT, 0, None)
                .err()
                .unwrap()
                .contains("blend mode"));
        }
    }
}

#[test]
fn threshold_modes_strict_ffi_and_hidden_predecode_refusals_preserve_input() {
    let adjustment = Layer::adjustment(
        "Hidden adjustment",
        r#"{"type":"adjust","op":"invert","params":{},"evaluation":"precision_v1"}"#,
        1.,
        false,
        &IccProfile::srgb(),
    )
    .unwrap();
    let doc = document(
        1,
        vec![
            layer("A", vec![[1, 2, 3, 0]], 0, 1.),
            adjustment,
            Layer::pass_through("Passing", vec![], 1., false).unwrap(),
        ],
    );
    let before = encoded(&doc);
    for mode in MODES {
        let changed = changed(&doc, 0, mode);
        assert!(edit(&changed, request(0, mode), LIMIT, None)
            .unwrap()
            .is_none());
        // A typed adjustment carries the same modes (v37/schema7).
        assert!(edit(&doc, request(1, mode), LIMIT, None).unwrap().is_some());
        assert!(edit(&doc, request(0, mode), 1, None).is_err());
        let cancel = DrJobControl::default();
        cancel.cancel();
        assert!(edit(&doc, request(0, mode), LIMIT, Some(&cancel)).is_err());
    }
    for mode in [27, -1, 99] {
        let value = request(0, mode);
        assert!(edit(&doc, value.clone(), LIMIT, None).is_err());
        let json = CString::new(value.to_string()).unwrap();
        let mut err = ptr::null_mut();
        let plan = unsafe { dr_precision_doc_edit_plan(&doc, json.as_ptr(), LIMIT, 0, &mut err) };
        assert!(plan.is_null() && !err.is_null());
        unsafe { dr_string_free(err) };
    }
    let new = changed(&doc, 0, 21);
    let bytes = encoded(&new);
    let tag = bytes.windows(4).rposition(|v| v == b"LBLD").unwrap();
    let idat = bytes.windows(4).position(|v| v == b"IDAT").unwrap();
    for (at, value, word) in [
        (tag + 4, 1, "schema"),
        (tag + 4, 2, "schema"),
        (tag + 4, 3, "schema"),
        (tag + 4, 4, "schema"),
        (tag + 4, 5, "schema"),
        (tag + 4, 7, "schema"),
        (tag + 8, 0, "count"),
        (tag + 8, 4, "count"),
        (tag + 12, 3, "index"),
        (tag + 12, 1, "isolated"),
        (tag + 12, 2, "isolated"),
        (tag + 16, 27, "mode"),
        (tag + 16, 99, "mode"),
        (8, 29, "body version"),
    ] {
        let mut bad = bytes.clone();
        bad[at..at + 4].copy_from_slice(&u32::to_le_bytes(value));
        bad[idat + 4] ^= 255;
        let error = PrecisionDocument::parse_native(&bad, LIMIT, 0, None)
            .err()
            .unwrap();
        assert!(error.contains(word), "{error}");
    }
    for end in tag..bytes.len() {
        assert!(PrecisionDocument::parse_native(&bytes[..end], LIMIT, 0, None).is_err());
    }
    let mut trailing = bytes.clone();
    trailing.push(0);
    assert!(PrecisionDocument::parse_native(&trailing, LIMIT, 0, None).is_err());
    assert_eq!(encoded(&doc), before);
}

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
    assert!(losses.is_empty(), "{losses:?}");
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
    assert!(error.is_null());
    (notice, *unsafe { Box::from_raw(document) })
}
fn info(document: &PrecisionDocument) -> Value {
    let mut error = ptr::null_mut();
    let json = unsafe { doctored_core::ffi_precision::dr_precision_doc_info(document, &mut error) };
    assert!(!json.is_null(), "{}", take_err_string(error));
    serde_json::from_str(&take_err_string(json)).expect("info JSON")
}
fn planes(rgba: &[[u16; 4]]) -> Vec<(i16, Vec<u8>)> {
    [(-1i16, 3usize), (0, 0), (1, 1), (2, 2)]
        .into_iter()
        .map(|(id, component)| {
            let samples: Vec<u16> = rgba.iter().map(|p| p[component]).collect();
            (id, fixture::raw_plane(&samples))
        })
        .collect()
}

#[test]
fn threshold_modes_round_trip_through_sixteen_bit_psd_and_import_a_hard_mix_folder() {
    let directory = TempDir::new().expect("tempdir");
    let colors = colors();
    let width = 4u32;
    let base: Vec<[u16; 4]> = colors[13..17].to_vec();
    let mut layers = vec![layer("Base", base.clone(), 0, 1.)];
    for (k, mode) in MODES.into_iter().enumerate() {
        let pixels: Vec<[u16; 4]> = colors[14 + k..18 + k].to_vec();
        layers.push(
            layer(&format!("Mode {mode}"), pixels, 0, 1.)
                .with_blend_mode(BlendMode::from_c(mode).unwrap())
                .unwrap(),
        );
    }
    let source = document(width, layers);
    let projection = source.render(LIMIT, 0, None).unwrap();
    let path = directory.path().join("threshold16.psd");
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
        [*b"norm", *b"diss", *b"dkCl", *b"lgCl", *b"hMix"],
        "one Photoshop key per mode"
    );
    let (_, back) = open16(&path);
    let layers = info(&back)["layers"].as_array().unwrap().clone();
    let modes: Vec<i64> = layers
        .iter()
        .map(|l| l["blend_mode"].as_i64().unwrap())
        .collect();
    assert_eq!(modes, [0, 14, 16, 17, 21]);
    assert_eq!(
        back.render(LIMIT, 0, None).unwrap().to_rgba16().ok(),
        projection.to_rgba16().ok()
    );
    for index in 0..5 {
        assert_eq!(
            back.layer_source(index).unwrap().to_rgba16().ok(),
            source.layer_source(index).unwrap().to_rgba16().ok()
        );
    }
    // A PSD import marks record 0 (word 113, `background_native`) first.
    assert_eq!(&encoded(&back)[4..8], &113u32.to_le_bytes());
    assert_eq!(&encoded(&back)[8..12], &34u32.to_le_bytes());

    // An independently authored closed `hMix` folder at opacity 128.
    let child: Vec<[u16; 4]> = colors[13..17].to_vec();
    let composite: Vec<[u16; 4]> = base.clone();
    let mut inner = Record::folder("Inner", 2, *b"hMix");
    inner.opacity = 128;
    let records = vec![
        Record::raster("Base", [0, 0, 1, width as i32], planes(&base)),
        Record::folder("</Layer group>", 3, *b"norm"),
        Record::raster("Child", [0, 0, 1, width as i32], planes(&child)),
        inner,
    ];
    let bytes = fixture::fixture(
        (width, 1),
        &records,
        IccProfile::srgb().bytes(),
        (72.0, 72.0),
        &composite,
        true,
        false,
    );
    let authored = directory.path().join("authored-hard-mix.psd");
    std::fs::write(&authored, bytes).unwrap();
    let (_, imported) = open16(&authored);
    let layers = info(&imported)["layers"].as_array().unwrap().clone();
    let names: Vec<&str> = layers.iter().map(|l| l["name"].as_str().unwrap()).collect();
    assert_eq!(names, ["Base", "Child", "Inner"]);
    assert_eq!(layers[2]["kind"], Value::from("isolated_blend_group"));
    assert_eq!(layers[2]["blend_mode"], Value::from(21));
    assert_eq!(layers[2]["opacity"], Value::from(128.0f32 / 255.0));
    assert_eq!(imported.layer(2).unwrap().blend_mode(), BlendMode::HardMix);
    let rendered = imported
        .render(LIMIT, 0, None)
        .unwrap()
        .to_rgba16()
        .unwrap();
    for x in 0..width as usize {
        let (refs, oracle, margin) = branches(21, n(base[x]), n(child[x]), 128. / 255.);
        assert_policy(
            rendered[x],
            refs,
            oracle,
            margin,
            HARD_MIX_BAND,
            false,
            false,
        );
    }
}
