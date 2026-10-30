//! The Art History Brush: `dr_doc_art_history_brush_layer`,
//! `dr_precision_doc_art_history_brush_plan` and
//! `dr_precision_doc_art_history_brush`, through the FFI, each against an
//! oracle written here.
//!
//! The source state is the take's fixture: R = x, G = y, B = 0, so an exact
//! stroke colour names the source pixel it came from. The statistics are
//! `scripts/photoshop-probe/art_history_take.py`'s (a stroke is an exact
//! colour with B = 0 at ≥ 3 changed pixels; Len = pixels / D; Curl = Len /
//! max(diameter, 1)), measured on ISOLATED strokes (Area 0: one stroke at
//! each click, the clicks far apart) so that no stroke covers another, and
//! compared with the shipped table's expectation, written here from the
//! module doc's geometry: a stroke of travel `ℓ = L·D` turning `θ = κ·L`
//! covers about `ℓ·D + πD²/4` pixels, and its pixel set spans its chord
//! `ℓ·sin(θ/2)/(θ/2)` plus a diameter. The tool is unavailable at 16 and 32
//! bits and on 16-bit CMYK, so those depths assert the refusal by name and
//! 8-bit CMYK paints source ink exactly.

use std::collections::HashMap;
use std::ffi::{c_char, CStr, CString};
use std::ptr;
use std::sync::Arc;

use doctored_core::doc::DrDocument;
use doctored_core::doc_lock::LOCK_TRANSPARENCY;
use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_cmyk_colour::dr_precision_doc_new_cmyk;
use doctored_core::ffi_doc::dr_doc_free;
use doctored_core::ffi_precision::dr_precision_doc_free;
use doctored_core::ffi_retouch_brush::{
    dr_doc_art_history_brush_layer, dr_precision_doc_art_history_brush,
    dr_precision_doc_art_history_brush_plan,
};
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer};
use doctored_core::precision_image::PrecisionImage;
use image::{Rgba, RgbaImage};

use crate::cmyk_fixture::press_profile;
use crate::cmyk_structure_fixture::then;
use crate::common::{layer_pixels, take_doc, take_err_string};
use crate::float_fixture::float_doc;

const W: u32 = 256;
const LIMIT: u64 = 2 * 1024 * 1024 * 1024;
const HARD: &str =
    r#"{"version":1,"hardness":1.0,"roundness":1.0,"angle_degrees":0.0,"pixel":false}"#;
const STYLES: [&str; 10] = [
    "tight_short",
    "tight_medium",
    "tight_long",
    "loose_medium",
    "loose_long",
    "dab",
    "tight_curl",
    "tight_curl_long",
    "loose_curl",
    "loose_curl_long",
];
/// The shipped table (`art_history_stroke.rs`, fitted from Photoshop's
/// take1): L in diameters, κ, the density ν.
const TABLE: [(f64, f64, f64); 10] = [
    (0.54, 0., 0.45),
    (1.77, 0., 0.3229),
    (2.99, 0., 0.3229),
    (1.73, 0., 0.3594),
    (2.25, 0., 0.3021),
    (0., 0., 0.5),
    (2.6, 1.212, 0.3073),
    (1.58, 0.655, 0.3073),
    (3.55, 0.522, 0.3177),
    (3.42, 0.582, 0.3385),
];
const CURL_TWINS: [(&str, &str); 4] = [
    ("tight_curl", "tight_medium"),
    ("tight_curl_long", "tight_long"),
    ("loose_curl", "loose_medium"),
    ("loose_curl_long", "loose_long"),
];
const MODE_REFUSAL: &str = "art history brush blend_mode must be Normal, Darken, Lighten, Hue, Saturation, Color or Luminosity";

fn gradient() -> RgbaImage {
    RgbaImage::from_fn(W, W, |x, y| Rgba([x as u8, y as u8, 0, 255]))
}

fn white() -> RgbaImage {
    RgbaImage::from_pixel(W, W, Rgba([255, 255, 255, 255]))
}

fn doc(image: RgbaImage) -> DrDocument {
    DrDocument::from_pixels(image)
}

fn request(style: &str, area: f64, tolerance: f64, mode: u32, seed: u64) -> String {
    format!(
        r#"{{"version":1,"style":"{style}","area":{area},"tolerance":{tolerance},"opacity":1,"blend_mode":{mode},"seed":{seed}}}"#
    )
}

fn handle(d: &DrDocument) -> *mut DrDocument {
    Box::into_raw(Box::new(d.clone()))
}

/// `dr_doc_art_history_brush_layer` on layer 0 from layer 0 of `source`.
fn art(
    current: &DrDocument,
    source: &DrDocument,
    request: &str,
    dabs: &[[f64; 4]],
    selection: Option<&[u8]>,
) -> (Option<DrDocument>, Option<String>) {
    let (d, s) = (handle(current), handle(source));
    let request = CString::new(request).unwrap();
    let brush = CString::new(HARD).unwrap();
    let flat: Vec<f64> = dabs.iter().flatten().copied().collect();
    let mut err: *mut c_char = ptr::null_mut();
    let out = unsafe {
        dr_doc_art_history_brush_layer(
            d,
            0,
            s,
            0,
            request.as_ptr(),
            brush.as_ptr(),
            if flat.is_empty() {
                ptr::null()
            } else {
                flat.as_ptr()
            },
            dabs.len(),
            selection.map_or(ptr::null(), <[u8]>::as_ptr),
            selection.map_or(0, <[u8]>::len),
            &mut err,
        )
    };
    unsafe {
        dr_doc_free(d);
        dr_doc_free(s);
    }
    take_doc(out, err)
}

fn pixels(d: &DrDocument) -> Vec<[u8; 4]> {
    layer_pixels(d, 0)
        .chunks_exact(4)
        .map(|p| [p[0], p[1], p[2], p[3]])
        .collect()
}

/// The take's stroke pixels (module doc): `{(r, g): [(x, y)]}` of the B = 0
/// pixels that differ from `current`, by exact colour.
fn strokes(image: &[[u8; 4]], current: &[[u8; 4]]) -> HashMap<(u8, u8), Vec<(i64, i64)>> {
    let mut out: HashMap<(u8, u8), Vec<(i64, i64)>> = HashMap::new();
    for (i, (p, c)) in image.iter().zip(current).enumerate() {
        if p[2] == 0 && p[3] == 255 && p != c {
            out.entry((p[0], p[1]))
                .or_default()
                .push(((i as u32 % W) as i64, (i as u32 / W) as i64));
        }
    }
    out.retain(|_, pts| pts.len() >= 3);
    out
}

fn diameter(pts: &[(i64, i64)]) -> f64 {
    let mut best = 0;
    for (i, a) in pts.iter().enumerate() {
        for b in &pts[i + 1..] {
            best = best.max((a.0 - b.0).pow(2) + (a.1 - b.1).pow(2));
        }
    }
    (best as f64).sqrt()
}

fn median(mut v: Vec<f64>) -> f64 {
    v.sort_by(f64::total_cmp);
    let m = v.len() / 2;
    if v.len() % 2 == 1 {
        v[m]
    } else {
        (v[m - 1] + v[m]) / 2.
    }
}

/// The isolated strokes' brush diameter.
const D: f64 = 5.;

/// Clicks at 64 and 192 on both axes (a 2 × 2 grid): at `D` 5 the longest
/// stroke (1.5·3.55·5 ≈ 27 px) never reaches a neighbour's.
fn grid() -> Vec<[f64; 4]> {
    let mut out = Vec::new();
    for j in 0..2 {
        for i in 0..2 {
            out.push([
                64.5 + 128. * f64::from(i),
                64.5 + 128. * f64::from(j),
                D,
                1.,
            ]);
        }
    }
    out
}

/// Len and Curl of every isolated stroke of `style` over twenty seeds.
fn isolated(style: &str) -> (Vec<f64>, Vec<f64>, usize) {
    let (source, current) = (doc(gradient()), doc(white()));
    let blank = pixels(&current);
    let (mut lens, mut curls, mut count) = (Vec::new(), Vec::new(), 0);
    for seed in 0..20 {
        let (out, err) = art(
            &current,
            &source,
            &request(style, 0., 0., 0, seed),
            &grid(),
            None,
        );
        let out = out.unwrap_or_else(|| panic!("{style}: {err:?}"));
        for pts in strokes(&pixels(&out), &blank).values() {
            let len = pts.len() as f64 / D;
            lens.push(len);
            curls.push(len / diameter(pts).max(1.));
            count += 1;
        }
    }
    (lens, curls, count)
}

/// The table's expected Len and Curl at `d` (module doc). The statistic
/// counts only fully covered pixels (an edge pixel blends with the white
/// and has B > 0), so the capsule is taken at the width `d − 1` the
/// anti-aliased band leaves: `ℓ·(d − 1) + π(d − 1)²/4` pixels spanning the
/// chord plus `d − 1`.
fn expectation(index: usize, d: f64) -> (f64, f64) {
    let (l, kappa, _) = TABLE[index];
    let travel = l * d;
    let turn = kappa * l;
    let chord = if turn > 0. {
        travel * (turn / 2.).sin() / (turn / 2.)
    } else {
        travel
    };
    let width = d - 1.;
    let len = (travel * width + std::f64::consts::PI * width * width / 4.) / d;
    (len, len / (chord + width).max(1.))
}

#[test]
fn the_same_seed_paints_the_same_bytes_and_another_seed_does_not() {
    let (source, current) = (doc(gradient()), doc(white()));
    let dabs = [[100., 100., 6., 1.], [140., 120., 6., 1.]];
    let first = art(
        &current,
        &source,
        &request("tight_medium", 50., 0., 0, 7),
        &dabs,
        None,
    );
    let again = art(
        &current,
        &source,
        &request("tight_medium", 50., 0., 0, 7),
        &dabs,
        None,
    );
    let other = art(
        &current,
        &source,
        &request("tight_medium", 50., 0., 0, 8),
        &dabs,
        None,
    );
    let (first, again, other) = (
        pixels(&first.0.unwrap()),
        pixels(&again.0.unwrap()),
        pixels(&other.0.unwrap()),
    );
    assert_eq!(first, again);
    assert_ne!(first, other);
    // A dab prefix's bytes: `art_history_refit.rs`.
    // Outside the reach of the dabs nothing changes.
    for (i, p) in first.iter().enumerate() {
        let (x, y) = (i as u32 % W, i as u32 / W);
        if x < 20 || y < 20 {
            assert_eq!(*p, [255; 4], "({x}, {y}) is out of reach");
        }
    }
}

#[test]
fn each_style_follows_the_shipped_table() {
    let mut curl_of = HashMap::new();
    for (index, style) in STYLES.iter().enumerate() {
        let (lens, curls, count) = isolated(style);
        assert!(count >= 70, "{style}: {count} isolated strokes");
        let (len, curl) = (median(lens), median(curls));
        let (want_len, want_curl) = expectation(index, D);
        let within = |got: f64, want: f64| got / want <= 1.3 && want / got <= 1.3;
        assert!(
            within(len, want_len),
            "{style}: Len {len:.2} against {want_len:.2}"
        );
        assert!(
            within(curl, want_curl),
            "{style}: Curl {curl:.3} against {want_curl:.3}"
        );
        if TABLE[index].1 == 0. {
            assert!(curl <= 1.1, "{style}: a straight style's Curl {curl:.3}");
        }
        curl_of.insert(*style, curl);
    }
    // A curl separates from its straight twin where the table's own
    // expectation separates them by at least 1.3 (the fitted tight_curl
    // alone: tight_curl_long turns only κ·L ≈ 1 rad and the loose curls
    // about 2 rad over a longer stroke, which the statistic barely tells
    // from straight; each curl's own Curl is checked above).
    let index = |style: &str| STYLES.iter().position(|s| *s == style).unwrap();
    for (curly, straight) in CURL_TWINS {
        if expectation(index(curly), D).1 < 1.3 * expectation(index(straight), D).1 {
            continue;
        }
        assert!(
            curl_of[curly] >= 1.15 * curl_of[straight],
            "{curly} {:.3} against {straight} {:.3}",
            curl_of[curly],
            curl_of[straight]
        );
    }
    // N: one click at Area 50, D 5. Overlapping strokes hide one another at
    // the density the table asks for, so N is counted through the
    // tolerance instead: the current state is the source except on ISLANDS,
    // the pixels whose x and y are multiples of 4, which are white. At
    // tolerance 50 only a stroke starting on an island is laid (a sixteenth
    // of them), the few that are laid do not overlap, and each is seen by its
    // exact colour, which decodes to an island. Over 160 seeds the laid
    // count over 160/16 estimates N, against round(ν·(A/D)²) clamped to
    // 1…64.
    let source = doc(gradient());
    let islands = doc(RgbaImage::from_fn(W, W, |x, y| {
        if x % 4 == 0 && y % 4 == 0 {
            Rgba([255; 4])
        } else {
            Rgba([x as u8, y as u8, 0, 255])
        }
    }));
    let before = pixels(&islands);
    for (index, style) in STYLES.iter().enumerate() {
        let mut laid = 0.;
        for seed in 0..160 {
            let (out, _) = art(
                &islands,
                &source,
                &request(style, 50., 50., 0, seed),
                &[[128., 128., 5., 1.]],
                None,
            );
            let Some(out) = out else { continue };
            laid += strokes(&pixels(&out), &before)
                .keys()
                .filter(|(r, g)| r % 4 == 0 && g % 4 == 0)
                .count() as f64;
        }
        let n = laid / 10.;
        let want = (TABLE[index].2 * 100.).round().clamp(1., 64.);
        assert!(
            n / want <= 1.3 && want / n <= 1.3,
            "{style}: N {n:.1} against {want}"
        );
    }
}

#[test]
fn tolerance_keeps_strokes_from_starting_where_the_states_agree() {
    let source = doc(gradient());
    let half = RgbaImage::from_fn(W, W, |x, y| {
        if x >= 128 {
            Rgba([255; 4])
        } else {
            Rgba([x as u8, y as u8, 0, 255])
        }
    });
    let current = doc(half);
    let before = pixels(&current);
    for seed in 0..6 {
        let (out, err) = art(
            &current,
            &source,
            &request("tight_short", 50., 50., 0, seed),
            &[[128., 128., 5., 1.]],
            None,
        );
        let out = out.unwrap_or_else(|| panic!("{err:?}"));
        let found = strokes(&pixels(&out), &before);
        assert!(!found.is_empty());
        assert!(
            found.keys().all(|(r, _)| *r >= 128),
            "a stroke started where the source equals the current pixels"
        );
    }
    // Tolerance 0 lets strokes start on both sides.
    let mut left = false;
    for seed in 0..6 {
        let (out, _) = art(
            &current,
            &source,
            &request("tight_short", 50., 0., 0, seed),
            &[[128., 128., 5., 1.]],
            None,
        );
        let image = pixels(&out.unwrap());
        // A stroke from x < 128 is visible on the white half.
        left |= image
            .iter()
            .enumerate()
            .any(|(i, p)| i as u32 % W >= 128 && p[2] == 0 && p[0] < 128);
    }
    assert!(left);
    // Tolerance 100 on an unchanged state changes nothing.
    let (out, err) = art(
        &source,
        &source,
        &request("tight_short", 50., 100., 0, 1),
        &[[128., 128., 5., 1.]],
        None,
    );
    assert!(out.is_none() && err.is_none(), "{err:?}");
}

#[test]
fn every_stroke_takes_its_colour_from_the_source_within_a_diameter() {
    let (source, current) = (doc(gradient()), doc(white()));
    let blank = pixels(&current);
    for style in STYLES {
        for seed in 0..4 {
            let (out, _) = art(
                &current,
                &source,
                &request(style, 0., 0., 0, seed),
                &grid(),
                None,
            );
            let found = strokes(&pixels(&out.unwrap()), &blank);
            assert!(!found.is_empty(), "{style}");
            for ((r, g), pts) in found {
                let nearest = pts
                    .iter()
                    .map(|&(x, y)| ((x - i64::from(r)) as f64).hypot((y - i64::from(g)) as f64))
                    .fold(f64::INFINITY, f64::min);
                assert!(nearest <= D, "{style}: ({r}, {g}) is {nearest:.1} px away");
            }
        }
    }
}

/// W3C Lum, SetLum and ClipColor in f64 (the compositing spec).
fn lum(c: [f64; 3]) -> f64 {
    0.3 * c[0] + 0.59 * c[1] + 0.11 * c[2]
}

fn set_lum(c: [f64; 3], l: f64) -> [f64; 3] {
    let d = l - lum(c);
    let mut c = c.map(|v| v + d);
    let l = lum(c);
    let n = c[0].min(c[1]).min(c[2]);
    let x = c[0].max(c[1]).max(c[2]);
    if n < 0. {
        c = c.map(|v| l + (v - l) * l / (l - n));
    }
    if x > 1. {
        c = c.map(|v| l + (v - l) * (1. - l) / (x - l));
    }
    c
}

#[test]
fn darken_and_luminosity_interiors_match_the_w3c_oracle() {
    let source = doc(gradient());
    let backdrop = [100u8, 150, 200, 255];
    let current = doc(RgbaImage::from_pixel(W, W, Rgba(backdrop)));
    let cb: [f64; 3] = [100. / 255., 150. / 255., 200. / 255.];
    // One Dab at each click, Area 0: a disc of D 9 centred on the click, its
    // colour the source at the click's pixel.
    let clicks: Vec<[f64; 4]> = (0..4)
        .map(|i| [40.5 + 50. * f64::from(i), 60.5 + 40. * f64::from(i), 9., 1.])
        .collect();
    for (mode, name) in [(6, "Darken"), (26, "Luminosity")] {
        let (out, err) = art(
            &current,
            &source,
            &request("dab", 0., 0., mode, 3),
            &clicks,
            None,
        );
        let image = pixels(&out.unwrap_or_else(|| panic!("{name}: {err:?}")));
        for click in &clicks {
            let (x, y) = (click[0] as u32, click[1] as u32);
            let cs = [f64::from(x) / 255., f64::from(y) / 255., 0.];
            let want = if mode == 6 {
                [cb[0].min(cs[0]), cb[1].min(cs[1]), cb[2].min(cs[2])]
            } else {
                set_lum(cb, lum(cs))
            };
            // The interior: within 3 px of the centre of a 9 px dab.
            for (dx, dy) in [(0i32, 0i32), (2, 0), (-2, 1), (0, -3), (1, 2)] {
                let px = image[((y as i32 + dy) as u32 * W + (x as i32 + dx) as u32) as usize];
                for c in 0..3 {
                    let code = (want[c] * 255.).round();
                    assert!(
                        (f64::from(px[c]) - code).abs() <= 1.,
                        "{name} at ({x}, {y}): {px:?} against {want:?}"
                    );
                }
                assert_eq!(px[3], 255);
            }
        }
    }
}

#[test]
fn a_selection_and_a_transparency_lock_confine_the_strokes() {
    let source = doc(gradient());
    let current = doc(RgbaImage::from_fn(W, W, |x, _| {
        Rgba([255, 255, 255, if x < 128 { 255 } else { 0 }])
    }));
    let dabs = [[128., 128., 5., 1.]];
    // A selection of the top half: nothing changes below it.
    let selection: Vec<u8> = (0..W * W)
        .map(|i| if i / W < 128 { 255 } else { 0 })
        .collect();
    let before = pixels(&current);
    let (out, err) = art(
        &current,
        &source,
        &request("tight_long", 60., 0., 0, 2),
        &dabs,
        Some(&selection),
    );
    let after = pixels(&out.unwrap_or_else(|| panic!("{err:?}")));
    for (i, (a, b)) in after.iter().zip(&before).enumerate() {
        if i as u32 / W >= 128 {
            assert_eq!(a, b);
        }
    }
    assert!(after.iter().zip(&before).any(|(a, b)| a != b));
    // A transparency lock keeps every alpha.
    let locked = current.with_layer_locks(0, LOCK_TRANSPARENCY).unwrap();
    let (out, err) = art(
        &locked,
        &source,
        &request("tight_long", 60., 0., 0, 2),
        &dabs,
        None,
    );
    let after = pixels(&out.unwrap_or_else(|| panic!("{err:?}")));
    assert!(after.iter().zip(&before).all(|(a, b)| a[3] == b[3]));
    assert!(after.iter().zip(&before).any(|(a, b)| a != b));
}

#[test]
fn refusals_and_nulls() {
    let (source, current) = (doc(gradient()), doc(white()));
    let dabs = [[64., 64., 5., 1.]];
    let refused = |request: &str| {
        art(&current, &source, request, &dabs, None)
            .1
            .unwrap_or_default()
    };
    assert!(refused(&request("bogus", 50., 0., 0, 1)).contains("style must be tight_short"));
    assert_eq!(refused(&request("dab", 50., 0., 1, 1)), MODE_REFUSAL);
    assert!(refused(&request("dab", 501., 0., 0, 1)).contains("area must be 0 to 500"));
    assert!(refused(&request("dab", 50., 101., 0, 1)).contains("tolerance must be 0 to 100"));
    assert!(refused(&request("dab", 50., 0., 0, 1 << 33)).contains("seed must be"));
    assert!(
        refused(&request("dab", 50., 0., 0, 1).replace("}", r#","extra":1}"#))
            .contains("invalid art history brush request")
    );
    // A selection of another length refuses by name; NULL handles are NULL
    // with no error; an out-of-range layer and no dabs are no-ops.
    let short = [255u8; 4];
    let (out, err) = art(
        &current,
        &source,
        &request("dab", 50., 0., 0, 1),
        &dabs,
        Some(&short),
    );
    assert!(out.is_none());
    assert!(err.unwrap().contains("selection length differs"));
    let (out, err) = art(&current, &source, &request("dab", 50., 0., 0, 1), &[], None);
    assert!(out.is_none() && err.is_none());
    let text = CString::new(request("dab", 50., 0., 0, 1)).unwrap();
    let brush = CString::new(HARD).unwrap();
    let flat = [64., 64., 5., 1.];
    let d = handle(&current);
    unsafe {
        let mut err: *mut c_char = ptr::null_mut();
        let call = |doc: *const DrDocument,
                    source: *const DrDocument,
                    idx: usize,
                    err: &mut *mut c_char| {
            dr_doc_art_history_brush_layer(
                doc,
                idx,
                source,
                0,
                text.as_ptr(),
                brush.as_ptr(),
                flat.as_ptr(),
                1,
                ptr::null(),
                0,
                err,
            )
        };
        assert!(call(ptr::null(), d, 0, &mut err).is_null() && err.is_null());
        assert!(call(d, ptr::null(), 0, &mut err).is_null() && err.is_null());
        assert!(call(d, d, 3, &mut err).is_null() && err.is_null());
        dr_doc_free(d);
    }
}

#[test]
fn the_work_bound_refuses_before_painting() {
    // Clicks every 8 px over a 1600 px canvas (40000 of the 50000 dabs a
    // stroke may carry), each one Loose Curl stroke of about 15 hard 8 px
    // dabs: past 2 billion units.
    let side = 1600;
    let source = DrDocument::from_pixels(RgbaImage::from_pixel(side, side, Rgba([9, 9, 9, 255])));
    let current = DrDocument::from_pixels(RgbaImage::from_pixel(side, side, Rgba([255; 4])));
    let mut dabs = Vec::new();
    for j in 0..200 {
        for i in 0..200 {
            dabs.push([4. + 8. * f64::from(i), 4. + 8. * f64::from(j), 8., 1.]);
        }
    }
    let (out, err) = art(
        &current,
        &source,
        &request("loose_curl", 0., 0., 0, 1),
        &dabs,
        None,
    );
    assert!(out.is_none());
    assert_eq!(
        err.as_deref(),
        Some("art history brush exceeds 2 billion work units; use a smaller Area, a shorter stroke or a smaller brush")
    );
}

// ------------------------------------------------------------ typed docs --

fn plan(
    doc: *const PrecisionDocument,
    source: *const PrecisionDocument,
    request: &str,
    selection: bool,
) -> Result<serde_json::Value, String> {
    let text = CString::new(request).unwrap();
    let brush = CString::new(HARD).unwrap();
    let dabs = [10., 10., 5., 1.];
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_art_history_brush_plan(
            doc,
            0,
            source,
            0,
            text.as_ptr(),
            brush.as_ptr(),
            dabs.as_ptr(),
            1,
            selection,
            LIMIT,
            0,
            &mut err,
        )
    };
    if out.is_null() {
        return Err(take_err_string(err));
    }
    let value = serde_json::from_str(unsafe { CStr::from_ptr(out) }.to_str().unwrap()).unwrap();
    unsafe { dr_string_free(out) };
    Ok(value)
}

fn typed(
    doc: *const PrecisionDocument,
    source: *const PrecisionDocument,
    request: &str,
    dabs: &[[f64; 4]],
    selection: &[u16],
) -> (*mut PrecisionDocument, Option<String>) {
    let text = CString::new(request).unwrap();
    let brush = CString::new(HARD).unwrap();
    let flat: Vec<f64> = dabs.iter().flatten().copied().collect();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_art_history_brush(
            doc,
            0,
            source,
            0,
            text.as_ptr(),
            brush.as_ptr(),
            flat.as_ptr(),
            dabs.len(),
            if selection.is_empty() {
                ptr::null()
            } else {
                selection.as_ptr()
            },
            selection.len(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    let message = (!err.is_null()).then(|| take_err_string(err));
    (out, message)
}

fn rgb16() -> *mut PrecisionDocument {
    let pixels: Vec<[u16; 4]> = (0..64 * 64)
        .map(|i| [i as u16 * 7, 300, 9000, 65535])
        .collect();
    let image = PrecisionImage::from_rgba16(64, 64, pixels, IccProfile::srgb(), None).unwrap();
    let layer = PrecisionDocumentLayer::raster("Layer", image, (0, 0), 1.0, true).unwrap();
    Box::into_raw(Box::new(
        PrecisionDocument::from_parts(
            64,
            64,
            IccProfile::srgb(),
            Metadata::default(),
            Resolution::default(),
            vec![layer],
        )
        .unwrap(),
    ))
}

fn new_cmyk(depth: u32, fill: [f64; 5]) -> *mut PrecisionDocument {
    let profile = press_profile("Art History Press");
    let text = CString::new(format!(
        r#"{{"depth":{depth},"fill":[{},{},{},{},{}],"layer_name":"Paper","ppi":72}}"#,
        fill[0], fill[1], fill[2], fill[3], fill[4]
    ))
    .unwrap();
    let mut err = ptr::null_mut();
    let doc = unsafe {
        dr_precision_doc_new_cmyk(
            64,
            64,
            profile.as_ptr(),
            profile.len(),
            text.as_ptr(),
            LIMIT,
            0,
            &mut err,
        )
    };
    assert!(!doc.is_null(), "{}", take_err_string(err));
    doc
}

#[test]
fn the_disabled_depths_refuse_by_name() {
    let req = request("tight_short", 20., 0., 0, 1);
    let sixteen = rgb16();
    let float: *mut PrecisionDocument = Box::into_raw(Box::new(float_doc(
        16,
        16,
        &Arc::clone(&IccProfile::srgb()),
        &[vec![[2.5, 0.5, 0.25, 1.0]; 256]],
    )));
    let cmyk16 = new_cmyk(16, [0.2, 0.4, 0.6, 0.1, 1.]);
    for (doc, sentence) in [
        (
            sixteen,
            "The Art History Brush is unavailable in a 16-bit document.",
        ),
        (
            float,
            "The Art History Brush is unavailable in a 32-bit document.",
        ),
        (
            cmyk16,
            "The Art History Brush is unavailable in a 16-bit CMYK document.",
        ),
    ] {
        assert_eq!(plan(doc, doc, &req, false).unwrap_err(), sentence);
        let (out, err) = typed(doc, doc, &req, &[[8., 8., 5., 1.]], &[]);
        assert!(out.is_null());
        assert_eq!(err.as_deref(), Some(sentence));
        unsafe { dr_precision_doc_free(doc) };
    }
}

#[test]
fn an_eight_bit_cmyk_document_paints_the_source_ink_exactly() {
    let ink = [0.2, 0.4, 0.6, 0.35, 1.];
    let source = new_cmyk(8, ink);
    let current = new_cmyk(8, [0., 0., 0., 0., 1.]);
    let req = request("tight_long", 30., 0., 0, 4);
    let planned = plan(current, source, &req, true).unwrap();
    assert_eq!(planned["input_samples"], 64 * 64);
    let before = unsafe { &*current }.cmyk_layer_samples(0).unwrap();
    let want = unsafe { &*source }.cmyk_layer_samples(0).unwrap()[0];
    let (out, err) = typed(current, source, &req, &[[32., 32., 5., 1.]], &[]);
    assert!(!out.is_null(), "{err:?}");
    let after = unsafe { &*out }.cmyk_layer_samples(0).unwrap();
    let changed: Vec<_> = after.iter().zip(&before).filter(|(a, b)| a != b).collect();
    assert!(!changed.is_empty());
    // The interior of every stroke is the source's ink, K included, exactly;
    // an edge pixel lies between the two inks.
    assert!(after.contains(&want));
    for (a, b) in changed {
        assert_eq!(a[4], 65535);
        for c in 0..4 {
            assert!(
                a[c] >= b[c].min(want[c]) && a[c] <= b[c].max(want[c]),
                "{a:?}"
            );
        }
    }
    // Tolerance 100 against itself changes nothing.
    let (same, err) = typed(
        source,
        source,
        &request("tight_long", 30., 100., 0, 4),
        &[[32., 32., 5., 1.]],
        &[],
    );
    assert!(same.is_null() && err.is_none(), "{err:?}");
    // A source that is not CMYK refuses in the tool's own sentence.
    let rgb = rgb16();
    assert_eq!(
        plan(current, rgb, &req, false).unwrap_err(),
        "The Art History Brush's source state is not a CMYK raster"
    );
    // A target that is not a CMYK raster (a group record) refuses in its own
    // sentence, before the source is judged.
    let grouped = then(
        new_cmyk(8, ink),
        r#"{"op":"group_layers","layers":[0],"name":"Group","pass_through":false}"#,
        &[],
    );
    let text = CString::new(req.as_str()).unwrap();
    let brush = CString::new(HARD).unwrap();
    let dab = [10., 10., 5., 1.];
    let mut err = ptr::null_mut();
    let refused = unsafe {
        dr_precision_doc_art_history_brush_plan(
            grouped,
            1,
            rgb,
            0,
            text.as_ptr(),
            brush.as_ptr(),
            dab.as_ptr(),
            1,
            false,
            LIMIT,
            0,
            &mut err,
        )
    };
    assert!(refused.is_null());
    assert_eq!(
        take_err_string(err),
        "The Art History Brush paints only a CMYK raster layer"
    );
    unsafe { dr_precision_doc_free(grouped) };
    // A bad blend mode refuses by name.
    let (bad, err) = typed(
        current,
        source,
        &request("dab", 30., 0., 2, 4),
        &[[32., 32., 5., 1.]],
        &[],
    );
    assert!(bad.is_null());
    assert_eq!(err.as_deref(), Some(MODE_REFUSAL));
    // NULL handles are refused, not dereferenced.
    assert!(plan(ptr::null(), source, &req, false).is_err());
    let (none, _) = typed(current, ptr::null(), &req, &[[32., 32., 5., 1.]], &[]);
    assert!(none.is_null());
    unsafe {
        dr_precision_doc_free(out);
        dr_precision_doc_free(source);
        dr_precision_doc_free(current);
        dr_precision_doc_free(rgb);
    }
}
