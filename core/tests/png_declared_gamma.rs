//! A PNG's bare `gAMA` and `cHRM` (`png_declared`): with neither an `iCCP`
//! nor an `sRGB` chunk, the declared gamma and primaries are the profile the
//! samples are read in, at 8 bits (the flat open) and at 16 bits (the typed
//! open), for RGB, gray and indexed files; the samples themselves are never
//! converted at open. An unusable declaration refuses the typed open and is
//! ignored, by sentence, in the flat one.
//!
//! Every PNG is written here chunk by chunk (`common/gray_fixture`). The
//! oracle is this file's own arithmetic: the decode exponent `100000 / gAMA`,
//! xy primaries to an RGB → XYZ matrix, a Bradford adaptation between the two
//! whites, the inverse of sRGB's own primaries matrix and the IEC 61966-2-1
//! encode. Nothing in it calls the crate. The bounds, fixed before measuring:
//! after a conversion to the built-in sRGB through the FFI, every channel
//! within 1 code at 8 bits and within 8 codes of 65535 at 16 bits (the test
//! colours keep every channel off the sRGB toe, where the built-in's
//! 1024-entry `curv`'s u16 rounding is worth up to about 6.5 codes on its
//! own).

use std::ffi::{CStr, CString};
use std::path::{Path, PathBuf};
use std::ptr;

use doctored_core::doc::DrDocument;
use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_color::{
    dr_builtin_profile, dr_builtin_profile_len, dr_doc_convert_to_profile, dr_doc_icc_profile,
    dr_doc_icc_profile_len,
};
use doctored_core::ffi_doc::{dr_doc_free, dr_doc_open};
use doctored_core::ffi_gray::{
    dr_path_gray_source, dr_precision_doc_convert_gray_to_rgb, dr_precision_doc_open_gray,
};
use doctored_core::ffi_indexed::{dr_path_indexed_source, dr_precision_doc_open_indexed};
use doctored_core::ffi_metadata_report::dr_path_metadata_import_report;
use doctored_core::ffi_precision::{
    dr_precision_doc_descriptor, dr_precision_doc_free, dr_precision_doc_open,
};
use doctored_core::ffi_precision_color::dr_precision_doc_convert_to_profile;
use doctored_core::icc::IccProfile;
use doctored_core::precision_document::PrecisionDocument;
use tempfile::TempDir;

use crate::common::{cpath, layer_pixels, take_err_string};
use crate::gray_fixture::{chunk, iccp, png, srgb_chunk};

const LIMIT: u64 = 2 * 1024 * 1024 * 1024;

/// The bounds the module doc fixes.
const BOUND_8: f64 = 1.0;
const BOUND_16: f64 = 8.0;

// ------------------------------------------------------------- the oracle --

type Xy = [f64; 2];

/// sRGB / Rec. 709 primaries and D65: red, green, blue, white.
const SRGB: [Xy; 4] = [[0.64, 0.33], [0.30, 0.60], [0.15, 0.06], [0.3127, 0.3290]];
/// Display P3 (D65).
const P3: [Xy; 4] = [
    [0.680, 0.320],
    [0.265, 0.690],
    [0.150, 0.060],
    [0.3127, 0.3290],
];
/// Adobe RGB (1998) (D65).
const ADOBE: [Xy; 4] = [[0.64, 0.33], [0.21, 0.71], [0.15, 0.06], [0.3127, 0.3290]];
/// ProPhoto RGB, whose white is D50 — the case that needs the adaptation.
const PROPHOTO: [Xy; 4] = [
    [0.7347, 0.2653],
    [0.1596, 0.8404],
    [0.0366, 0.0001],
    [0.3457, 0.3585],
];

fn xyz(xy: Xy) -> [f64; 3] {
    [xy[0] / xy[1], 1.0, (1.0 - xy[0] - xy[1]) / xy[1]]
}

fn mul(a: [[f64; 3]; 3], b: [[f64; 3]; 3]) -> [[f64; 3]; 3] {
    std::array::from_fn(|i| std::array::from_fn(|j| (0..3).map(|k| a[i][k] * b[k][j]).sum()))
}

fn apply(m: [[f64; 3]; 3], v: [f64; 3]) -> [f64; 3] {
    std::array::from_fn(|i| (0..3).map(|k| m[i][k] * v[k]).sum())
}

fn inverse(m: [[f64; 3]; 3]) -> [[f64; 3]; 3] {
    let det = m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1])
        - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0])
        + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0]);
    let c =
        |r0: usize, c0: usize, r1: usize, c1: usize| m[r0][c0] * m[r1][c1] - m[r0][c1] * m[r1][c0];
    [
        [
            c(1, 1, 2, 2) / det,
            -c(0, 1, 2, 2) / det,
            c(0, 1, 1, 2) / det,
        ],
        [
            -c(1, 0, 2, 2) / det,
            c(0, 0, 2, 2) / det,
            -c(0, 0, 1, 2) / det,
        ],
        [
            c(1, 0, 2, 1) / det,
            -c(0, 0, 2, 1) / det,
            c(0, 0, 1, 1) / det,
        ],
    ]
}

/// Linear RGB → XYZ (white Y = 1) of a set of primaries, row-major.
fn primaries_matrix(p: &[Xy; 4]) -> [[f64; 3]; 3] {
    let columns = [xyz(p[0]), xyz(p[1]), xyz(p[2])];
    let basis: [[f64; 3]; 3] = std::array::from_fn(|i| std::array::from_fn(|k| columns[k][i]));
    let scale = apply(inverse(basis), xyz(p[3]));
    std::array::from_fn(|i| std::array::from_fn(|k| basis[i][k] * scale[k]))
}

/// The Bradford adaptation from one white (XYZ) to another, row-major.
fn bradford(from: [f64; 3], to: [f64; 3]) -> [[f64; 3]; 3] {
    let m = [
        [0.8951, 0.2664, -0.1614],
        [-0.7502, 1.7135, 0.0367],
        [0.0389, -0.0685, 1.0296],
    ];
    let (s, d) = (apply(m, from), apply(m, to));
    let scale = [
        [d[0] / s[0], 0.0, 0.0],
        [0.0, d[1] / s[1], 0.0],
        [0.0, 0.0, d[2] / s[2]],
    ];
    mul(inverse(m), mul(scale, m))
}

fn srgb_decode(v: f64) -> f64 {
    if v <= 0.04045 {
        v / 12.92
    } else {
        ((v + 0.055) / 1.055).powf(2.4)
    }
}

fn srgb_encode(y: f64) -> f64 {
    let y = y.clamp(0.0, 1.0);
    if y <= 0.003_130_8 {
        12.92 * y
    } else {
        1.055 * y.powf(1.0 / 2.4) - 0.055
    }
}

/// What a file sample means, as sRGB-encoded unit values: decoded through
/// `gamma` (the decode exponent; `None` is the sRGB curve), from `primaries`
/// (`None` is sRGB's) into sRGB's primaries, then encoded.
fn oracle(rgb: [f64; 3], gamma: Option<f64>, primaries: Option<&[Xy; 4]>) -> [f64; 3] {
    let linear = rgb.map(|v| match gamma {
        Some(g) => v.powf(g),
        None => srgb_decode(v),
    });
    let srgb = match primaries {
        None => linear,
        Some(p) => {
            let adapt = bradford(xyz(p[3]), xyz(SRGB[3]));
            let to_srgb = mul(
                inverse(primaries_matrix(&SRGB)),
                mul(adapt, primaries_matrix(p)),
            );
            apply(to_srgb, linear)
        }
    };
    srgb.map(srgb_encode)
}

// ----------------------------------------------------------------- files --

/// The test colours as unit values: black, white, three neutrals and four
/// muted colours, every channel off the sRGB toe once converted.
const COLOURS: [[f64; 3]; 9] = [
    [0.0, 0.0, 0.0],
    [1.0, 1.0, 1.0],
    [0.25, 0.25, 0.25],
    [0.5, 0.5, 0.5],
    [0.75, 0.75, 0.75],
    [0.6, 0.5, 0.4],
    [0.45, 0.55, 0.6],
    [0.7, 0.6, 0.65],
    [0.5, 0.62, 0.48],
];

fn gama(file: u32) -> Vec<u8> {
    chunk(b"gAMA", &file.to_be_bytes())
}

/// `cHRM` of white, red, green, blue as xy × 100000.
fn chrm(p: &[Xy; 4]) -> Vec<u8> {
    let mut data = Vec::new();
    for xy in [p[3], p[0], p[1], p[2]] {
        for v in xy {
            data.extend_from_slice(&((v * 100_000.0).round() as u32).to_be_bytes());
        }
    }
    chunk(b"cHRM", &data)
}

/// An RGB PNG of [`COLOURS`] at `depth` (8 or 16), one row.
fn rgb_png(depth: u8, extra: &[Vec<u8>]) -> Vec<u8> {
    let top = if depth == 16 { 65535.0 } else { 255.0 };
    let samples: Vec<u16> = COLOURS
        .iter()
        .flatten()
        .map(|v| (v * top).round() as u16)
        .collect();
    png(COLOURS.len() as u32, 1, depth, 2, 3, &samples, extra)
}

/// The unit values a file of `depth` actually stores for [`COLOURS`].
fn stored(depth: u8) -> Vec<[f64; 3]> {
    let top = if depth == 16 { 65535.0 } else { 255.0 };
    COLOURS
        .iter()
        .map(|c| c.map(|v| (v * top).round() / top))
        .collect()
}

fn write(dir: &TempDir, name: &str, bytes: &[u8]) -> PathBuf {
    let path = dir.path().join(name);
    std::fs::write(&path, bytes).unwrap();
    path
}

fn builtin(which: std::ffi::c_int) -> Vec<u8> {
    let len = dr_builtin_profile_len(which);
    let mut out = vec![0u8; len];
    assert!(unsafe { dr_builtin_profile(which, out.as_mut_ptr(), len) });
    out
}

fn srgb() -> Vec<u8> {
    builtin(0)
}

fn name_of(icc: &[u8]) -> String {
    IccProfile::parse(icc)
        .expect("the document's profile parses")
        .name()
        .to_string()
}

// ------------------------------------------------------------ the typed open --

fn open16(path: &Path) -> Result<*mut PrecisionDocument, String> {
    let c = cpath(path);
    let mut err = ptr::null_mut();
    let doc = unsafe { dr_precision_doc_open(c.as_ptr(), LIMIT, 0, ptr::null(), &mut err) };
    if doc.is_null() {
        Err(take_err_string(err))
    } else {
        Ok(doc)
    }
}

fn samples16(doc: *const PrecisionDocument) -> Vec<[u16; 4]> {
    unsafe { &*doc }
        .layer_source(0)
        .expect("one layer")
        .to_rgba16()
        .expect("rgba16 storage")
}

fn to_srgb16(doc: *const PrecisionDocument) -> *mut PrecisionDocument {
    let icc = srgb();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_convert_to_profile(
            doc,
            icc.as_ptr(),
            icc.len(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(!out.is_null(), "convert to sRGB: {}", take_err_string(err));
    out
}

/// Opens a 16-bit RGB file with a declaration: the stored codes are the
/// file's, the profile is `name`, and converted to sRGB every channel lands
/// within [`BOUND_16`] of the oracle (the worst miss goes to stderr).
fn typed_case(
    dir: &TempDir,
    file: &str,
    extra: &[Vec<u8>],
    name: &str,
    gamma: Option<f64>,
    primaries: Option<&[Xy; 4]>,
) {
    let path = write(dir, file, &rgb_png(16, extra));
    let doc = open16(&path).unwrap_or_else(|e| panic!("{file} opens: {e}"));
    let profile = unsafe { &*doc }.profile().clone();
    assert_eq!(profile.name(), name, "{file}");
    for (pixel, colour) in samples16(doc).iter().zip(stored(16)) {
        let codes = colour.map(|v| (v * 65535.0).round() as u16);
        assert_eq!(pixel[..3], codes, "{file}: the samples are the file's");
    }
    let converted = to_srgb16(doc);
    let mut worst = 0.0_f64;
    for (pixel, colour) in samples16(converted).iter().zip(stored(16)) {
        let want = oracle(colour, gamma, primaries).map(|v| (v * 65535.0).round());
        for c in 0..3 {
            let miss = (f64::from(pixel[c]) - want[c]).abs();
            worst = worst.max(miss);
            assert!(
                miss <= BOUND_16,
                "{file}: {colour:?} channel {c}: {} against {}",
                pixel[c],
                want[c]
            );
        }
    }
    unsafe {
        dr_precision_doc_free(converted);
        dr_precision_doc_free(doc);
    }
    eprintln!("{file}: worst miss {worst} codes of 65535");
}

#[test]
fn a_sixteen_bit_rgb_png_is_read_in_the_gamma_and_primaries_it_declares() {
    let dir = TempDir::new().unwrap();
    // Linear light: gAMA 100000 is a decode exponent of exactly 1.
    typed_case(
        &dir,
        "linear.png",
        &[gama(100_000)],
        "PNG gamma 1.0000",
        Some(1.0),
        None,
    );
    // The common 2.2 declaration with Display P3's primaries.
    typed_case(
        &dir,
        "p3.png",
        &[gama(45_455), chrm(&P3)],
        "PNG gamma 2.2000, file primaries",
        Some(100_000.0 / 45_455.0),
        Some(&P3),
    );
    // cHRM alone: the sRGB curve, the file's primaries.
    typed_case(
        &dir,
        "adobe.png",
        &[chrm(&ADOBE)],
        "PNG sRGB curve, file primaries",
        None,
        Some(&ADOBE),
    );
    // A D50 white: the primaries are adapted to the PCS by Bradford.
    typed_case(
        &dir,
        "prophoto.png",
        &[gama(55_556), chrm(&PROPHOTO)],
        "PNG gamma 1.8000, file primaries",
        Some(100_000.0 / 55_556.0),
        Some(&PROPHOTO),
    );
}

#[test]
fn an_iccp_or_an_srgb_chunk_outranks_the_bare_declaration() {
    let dir = TempDir::new().unwrap();
    for (file, extra, name) in [
        (
            "srgb.png",
            vec![gama(100_000), chrm(&P3), srgb_chunk()],
            "sRGB IEC61966-2.1",
        ),
        (
            "iccp.png",
            vec![gama(100_000), chrm(&ADOBE), iccp(&builtin(1))],
            "Display P3",
        ),
    ] {
        let doc = open16(&write(&dir, file, &rgb_png(16, &extra))).unwrap();
        assert_eq!(unsafe { &*doc }.profile().name(), name, "{file}");
        unsafe { dr_precision_doc_free(doc) };
        let byte = open8(&write(&dir, &format!("8-{file}"), &rgb_png(8, &extra)));
        assert_eq!(name_of(&byte_profile(byte)), name, "8-bit {file}");
        unsafe { dr_doc_free(byte) };
    }
}

// ------------------------------------------------------------- the flat open --

fn open8(path: &Path) -> *mut DrDocument {
    let c = cpath(path);
    let mut err = ptr::null_mut();
    let doc = unsafe { dr_doc_open(c.as_ptr(), ptr::null(), &mut err) };
    assert!(!doc.is_null(), "open {path:?}: {}", take_err_string(err));
    doc
}

fn byte_profile(doc: *const DrDocument) -> Vec<u8> {
    let len = unsafe { dr_doc_icc_profile_len(doc) };
    let mut out = vec![0u8; len];
    assert!(unsafe { dr_doc_icc_profile(doc, out.as_mut_ptr(), len) });
    out
}

fn report(path: &Path) -> serde_json::Value {
    let c = cpath(path);
    let mut err = ptr::null_mut();
    let json = unsafe { dr_path_metadata_import_report(c.as_ptr(), &mut err) };
    assert!(!json.is_null(), "{}", take_err_string(err));
    let value = serde_json::from_str(unsafe { CStr::from_ptr(json) }.to_str().unwrap()).unwrap();
    unsafe { dr_string_free(json) };
    value
}

#[test]
fn an_eight_bit_rgb_png_is_read_in_the_gamma_and_primaries_it_declares() {
    let dir = TempDir::new().unwrap();
    let srgb = srgb();
    for (file, extra, name, gamma, primaries) in [
        (
            "linear8.png",
            vec![gama(100_000)],
            "PNG gamma 1.0000",
            Some(1.0),
            None,
        ),
        (
            "p3_8.png",
            vec![gama(45_455), chrm(&P3)],
            "PNG gamma 2.2000, file primaries",
            Some(100_000.0 / 45_455.0),
            Some(&P3),
        ),
    ] {
        let path = write(&dir, file, &rgb_png(8, &extra));
        let doc = open8(&path);
        assert_eq!(name_of(&byte_profile(doc)), name, "{file}");
        let opened = layer_pixels(doc, 0);
        for (pixel, colour) in opened.chunks_exact(4).zip(stored(8)) {
            let codes = colour.map(|v| (v * 255.0).round() as u8);
            assert_eq!(pixel[..3], codes, "{file}: the samples are the file's");
        }
        assert!(
            report(&path).get("profile_refusal").is_none(),
            "{file}: a kept profile has no sentence"
        );
        let mut err = ptr::null_mut();
        let converted =
            unsafe { dr_doc_convert_to_profile(doc, srgb.as_ptr(), srgb.len(), &mut err) };
        assert!(!converted.is_null(), "{}", take_err_string(err));
        for (pixel, colour) in layer_pixels(converted, 0).chunks_exact(4).zip(stored(8)) {
            let want = oracle(colour, gamma, primaries).map(|v| (v * 255.0).round());
            for c in 0..3 {
                let miss = (f64::from(pixel[c]) - want[c]).abs();
                assert!(
                    miss <= BOUND_8,
                    "{file}: {colour:?} channel {c}: {} against {}",
                    pixel[c],
                    want[c]
                );
            }
        }
        unsafe {
            dr_doc_free(converted);
            dr_doc_free(doc);
        }
    }
}

#[test]
fn an_unusable_declaration_refuses_the_typed_open_and_is_ignored_by_sentence_in_the_flat_one() {
    let dir = TempDir::new().unwrap();
    let degenerate = chunk(b"cHRM", &[0u8; 32]);
    for (file, extra, clause) in [
        (
            "gama0.png",
            vec![gama(0)],
            "the PNG's gAMA chunk is 0, which declares no gamma",
        ),
        (
            "steep.png",
            vec![gama(5_000)],
            "the PNG's gAMA chunk declares a decode exponent of 20.0000, outside 0.1 to 10",
        ),
        (
            "short.png",
            vec![chunk(b"gAMA", &[0, 1])],
            "the PNG's gAMA chunk is not 4 bytes long",
        ),
        (
            "chrm0.png",
            vec![degenerate.clone()],
            "the PNG's cHRM chunk does not describe three primaries and a white point",
        ),
    ] {
        let error = open16(&write(&dir, file, &rgb_png(16, &extra))).unwrap_err();
        assert!(
            error.contains(&format!("high-depth profile: {clause}")),
            "{file}: {error}"
        );
        let path = write(&dir, &format!("8-{file}"), &rgb_png(8, &extra));
        let doc = open8(&path);
        assert_eq!(name_of(&byte_profile(doc)), "sRGB IEC61966-2.1", "{file}");
        for (pixel, colour) in layer_pixels(doc, 0).chunks_exact(4).zip(stored(8)) {
            assert_eq!(pixel[..3], colour.map(|v| (v * 255.0).round() as u8));
        }
        unsafe { dr_doc_free(doc) };
        let mut sentence = clause.to_string();
        sentence.replace_range(..1, "T");
        assert_eq!(
            report(&path)["profile_refusal"],
            format!(
                "{sentence}, so it was ignored and the image's colour numbers were read as sRGB."
            ),
            "{file}"
        );
    }
}

// ------------------------------------------------------------ gray and indexed --

fn open_gray(path: &Path) -> *mut PrecisionDocument {
    let c = cpath(path);
    let working = CString::new("dot_gain_20").unwrap();
    let mut err = ptr::null_mut();
    let doc = unsafe {
        dr_precision_doc_open_gray(
            c.as_ptr(),
            working.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(
        !doc.is_null(),
        "open gray {path:?}: {}",
        take_err_string(err)
    );
    doc
}

fn gray_profile_name(doc: *const PrecisionDocument) -> String {
    let mut err = ptr::null_mut();
    let text = unsafe { dr_precision_doc_descriptor(doc, &mut err) };
    assert!(!text.is_null(), "{}", take_err_string(err));
    let value: serde_json::Value =
        serde_json::from_str(unsafe { CStr::from_ptr(text) }.to_str().unwrap()).unwrap();
    unsafe { dr_string_free(text) };
    value["gray"]["profile"]
        .as_str()
        .unwrap_or_default()
        .to_string()
}

const GRAY_LEVELS: [f64; 5] = [0.0, 0.25, 0.5, 0.75, 1.0];

#[test]
fn a_gray_png_reads_its_bare_gama_as_its_gray_profile_at_8_and_16_bits() {
    let dir = TempDir::new().unwrap();
    let srgb = srgb();
    for depth in [8u8, 16] {
        let top = if depth == 16 { 65535.0 } else { 255.0 };
        let codes: Vec<u16> = GRAY_LEVELS
            .iter()
            .map(|v| (v * top).round() as u16)
            .collect();
        let bytes = png(5, 1, depth, 0, 1, &codes, &[gama(100_000)]);
        let path = write(&dir, &format!("gray{depth}.png"), &bytes);
        let c = cpath(&path);
        assert_eq!(unsafe { dr_path_gray_source(c.as_ptr()) }, 1, "{depth}-bit");
        // The Grayscale document: the codes kept, the declared curve its
        // profile, and Mode ▸ RGB into sRGB on the oracle.
        let doc = open_gray(&path);
        assert_eq!(gray_profile_name(doc), "PNG gamma 1.0000", "{depth}-bit");
        let scale = if depth == 16 { 1 } else { 257 };
        for (pixel, code) in samples16(doc).iter().zip(&codes) {
            assert_eq!(pixel[0], code * scale, "{depth}-bit: the codes are kept");
        }
        let mut err = ptr::null_mut();
        let rgb = unsafe {
            dr_precision_doc_convert_gray_to_rgb(
                doc,
                srgb.as_ptr(),
                srgb.len(),
                ptr::null(),
                LIMIT,
                0,
                ptr::null(),
                &mut err,
            )
        };
        assert!(!rgb.is_null(), "{}", take_err_string(err));
        // An 8-bit document's samples are its codes × 257, compared as codes.
        let (bound, unit) = if depth == 16 {
            (BOUND_16, 65535.0)
        } else {
            (BOUND_8, 255.0)
        };
        for (pixel, code) in samples16(rgb).iter().zip(&codes) {
            let got = (f64::from(pixel[0]) / 65535.0 * unit).round();
            let want = (srgb_encode(f64::from(*code) / top) * unit).round();
            assert!(
                (got - want).abs() <= bound,
                "{depth}-bit code {code}: {got} against {want}"
            );
        }
        unsafe {
            dr_precision_doc_free(rgb);
            dr_precision_doc_free(doc);
        }
        // The flat open's `.rgba8` choice converts each code through the
        // declared curve to sRGB, under the converted sentence.
        let flat = open8(&path);
        for (pixel, code) in layer_pixels(flat, 0).chunks_exact(4).zip(&codes) {
            let want = (srgb_encode(f64::from(*code) / top) * 255.0).round();
            assert!(
                (f64::from(pixel[0]) - want).abs() <= BOUND_8,
                "{depth}-bit flat code {code}: {} against {want}",
                pixel[0]
            );
        }
        unsafe { dr_doc_free(flat) };
        assert_eq!(
            report(&path)["profile_refusal"],
            "The embedded colour profile “PNG gamma 1.0000” is a Gray profile, so the image was converted from it to sRGB as it was read.",
        );
    }
    // A gray image reads gAMA alone: its cHRM, even an unusable one, declares
    // nothing, and the working Gray is assigned as for an untagged file.
    let codes: Vec<u16> = GRAY_LEVELS
        .iter()
        .map(|v| (v * 255.0).round() as u16)
        .collect();
    let path = write(
        &dir,
        "gray_chrm.png",
        &png(5, 1, 8, 0, 1, &codes, &[chunk(b"cHRM", &[0u8; 32])]),
    );
    let doc = open_gray(&path);
    assert_eq!(gray_profile_name(doc), "Dot Gain 20%");
    unsafe { dr_precision_doc_free(doc) };
}

#[test]
fn an_indexed_png_with_a_bare_gama_opens_indexed_in_the_declared_profile() {
    let dir = TempDir::new().unwrap();
    let palette: Vec<u8> = vec![0, 0, 0, 128, 64, 32, 255, 255, 255];
    let indices: Vec<u16> = vec![0, 1, 2, 1];
    let bytes = png(
        4,
        1,
        8,
        3,
        1,
        &indices,
        &[gama(45_455), chunk(b"PLTE", &palette)],
    );
    let path = write(&dir, "indexed.png", &bytes);
    let c = cpath(&path);
    assert_eq!(unsafe { dr_path_indexed_source(c.as_ptr()) }, 1);
    let mut err = ptr::null_mut();
    let doc = unsafe { dr_precision_doc_open_indexed(c.as_ptr(), LIMIT, 0, ptr::null(), &mut err) };
    assert!(!doc.is_null(), "{}", take_err_string(err));
    assert_eq!(unsafe { &*doc }.profile().name(), "PNG gamma 2.2000");
    unsafe { dr_precision_doc_free(doc) };
}
