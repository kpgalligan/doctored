//! The two wide built-in profiles, Adobe RGB (1998) and ProPhoto RGB
//! (`icc_builtin`, `DR_PROFILE_ADOBE_RGB` = 2, `DR_PROFILE_PROPHOTO_RGB` = 3),
//! through the FFI: their bytes, and what a Convert between them and sRGB
//! does to a neutral grey and to the saturated primaries at 32-bit float, 16
//! and 8 bits.
//!
//! The oracle is this file's own and reads nothing off the core. Each space
//! is its PUBLISHED data: the xy primaries, the native white (D65 through the
//! Bradford adaptation for sRGB and Adobe RGB; the PCS D50 itself for
//! ProPhoto, with no adaptation) and the tone curve (the IEC 61966-2.1
//! function; a pure power of 563/256; a pure power of 461/256, ProPhoto RGB
//! at gamma 1.8 with no ROMM linear toe). The RGB→XYZ matrices are computed
//! here in f64 from those primaries, and a conversion is
//! `inv(destination) · source` between them.
//!
//! Comparisons of an encoded result are made in LINEAR light, after decoding
//! the result with the destination's analytic curve: a bound in encoded codes
//! is ill-conditioned near 0 under a pure power curve, whose slope there is
//! unbounded.

use std::ffi::{c_char, c_int, CStr};
use std::ptr;
use std::sync::Arc;

use doctored_core::doc::DrDocument;
use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_color::{
    dr_builtin_profile, dr_builtin_profile_len, dr_doc_assign_profile, dr_doc_convert_to_profile,
    dr_icc_inspect,
};
use doctored_core::ffi_doc::dr_doc_free;
use doctored_core::ffi_precision_color::dr_precision_doc_convert_to_profile;
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::PrecisionDocument;
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use image::{Rgba, RgbaImage};

use crate::float_fixture::float_doc;

const LIMIT: u64 = REFERENCE_WORKING_BYTES;

/// Mirrored from the header.
const PROFILE_SRGB: c_int = 0;
const PROFILE_ADOBE_RGB: c_int = 2;
const PROFILE_PROPHOTO_RGB: c_int = 3;
const ICC_RGB_MATRIX: c_int = 3;

/// Float Convert, absolute, on linear values.
const FLOAT_BOUND: f64 = 3e-4;
/// 16-bit Convert, linear light after the destination's analytic decode.
const SIXTEEN_BOUND: f64 = 3e-4;
/// 8-bit Convert, linear light: half an 8-bit code is 4.5e-3 linear at the
/// steepest point of these curves (white under the IEC curve).
const EIGHT_BOUND: f64 = 6e-3;

// ------------------------------------------------- the published spaces --

type Mat = [[f64; 3]; 3];

#[derive(Clone, Copy, PartialEq)]
enum White {
    /// D65 (0.3127, 0.3290), Bradford-adapted to the PCS D50.
    D65,
    /// The PCS D50 itself: no adaptation.
    D50,
}

#[derive(Clone, Copy)]
enum Tone {
    /// IEC 61966-2.1.
    Iec,
    /// A pure power, `raw / 256` as a u8Fixed8.
    Gamma(u16),
}

#[derive(Clone, Copy)]
struct Space {
    which: c_int,
    name: &'static str,
    primaries: [(f64, f64); 3],
    white: White,
    tone: Tone,
}

const SRGB: Space = Space {
    which: PROFILE_SRGB,
    name: "sRGB IEC61966-2.1",
    primaries: [(0.64, 0.33), (0.30, 0.60), (0.15, 0.06)],
    white: White::D65,
    tone: Tone::Iec,
};

const ADOBE: Space = Space {
    which: PROFILE_ADOBE_RGB,
    name: "Adobe RGB (1998)",
    primaries: [(0.64, 0.33), (0.21, 0.71), (0.15, 0.06)],
    white: White::D65,
    tone: Tone::Gamma(0x0233),
};

const PROPHOTO: Space = Space {
    which: PROFILE_PROPHOTO_RGB,
    name: "ProPhoto RGB",
    primaries: [(0.7347, 0.2653), (0.1596, 0.8404), (0.0366, 0.0001)],
    white: White::D50,
    tone: Tone::Gamma(0x01CD),
};

const WIDE: [Space; 2] = [ADOBE, PROPHOTO];

/// The conversions every depth is checked on.
const PAIRS: [(Space, Space); 5] = [
    (SRGB, ADOBE),
    (SRGB, PROPHOTO),
    (ADOBE, SRGB),
    (PROPHOTO, SRGB),
    (ADOBE, PROPHOTO),
];

/// The PCS illuminant, from the ICC spec.
const D50: [f64; 3] = [0.9642, 1.0, 0.8249];
const D50_RAW: [i32; 3] = [0x0000_F6D6, 0x0001_0000, 0x0000_D32D];

/// The Bradford cone response matrix, from the CIE literature.
const BRADFORD: Mat = [
    [0.8951, 0.2664, -0.1614],
    [-0.7502, 1.7135, 0.0367],
    [0.0389, -0.0685, 1.0296],
];

fn mat_mul(a: Mat, b: Mat) -> Mat {
    std::array::from_fn(|i| std::array::from_fn(|j| (0..3).map(|k| a[i][k] * b[k][j]).sum()))
}

fn mat_vec(a: Mat, v: [f64; 3]) -> [f64; 3] {
    std::array::from_fn(|i| (0..3).map(|k| a[i][k] * v[k]).sum())
}

/// The inverse by cofactors.
fn inv3(m: Mat) -> Mat {
    let det = m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1])
        - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0])
        + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0]);
    std::array::from_fn(|r| {
        std::array::from_fn(|c| {
            let (r0, r1) = ((c + 1) % 3, (c + 2) % 3);
            let (c0, c1) = ((r + 1) % 3, (r + 2) % 3);
            (m[r0][c0] * m[r1][c1] - m[r0][c1] * m[r1][c0]) / det
        })
    })
}

fn xy_to_xyz(x: f64, y: f64) -> [f64; 3] {
    [x / y, 1.0, (1.0 - x - y) / y]
}

/// `M⁻¹ · diag(M·Wd / M·Ws) · M`, the von Kries adaptation in Bradford's
/// cone space.
fn adaptation(ws: [f64; 3], wd: [f64; 3]) -> Mat {
    let s = mat_vec(BRADFORD, ws);
    let d = mat_vec(BRADFORD, wd);
    let scale = [
        [d[0] / s[0], 0.0, 0.0],
        [0.0, d[1] / s[1], 0.0],
        [0.0, 0.0, d[2] / s[2]],
    ];
    mat_mul(inv3(BRADFORD), mat_mul(scale, BRADFORD))
}

impl Space {
    fn native_white(self) -> [f64; 3] {
        match self.white {
            White::D65 => xy_to_xyz(0.3127, 0.3290),
            White::D50 => D50,
        }
    }

    /// The native→PCS adaptation: the `chad` tag's meaning.
    fn chad(self) -> Mat {
        match self.white {
            White::D65 => adaptation(self.native_white(), D50),
            White::D50 => [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
        }
    }

    /// Linear RGB → PCS XYZ (D50), rows, from the xy primaries: each primary
    /// scaled so the three sum to the native white, then adapted.
    fn to_pcs(self) -> Mat {
        let cols = self.primaries.map(|(x, y)| xy_to_xyz(x, y));
        let basis: Mat = std::array::from_fn(|r| std::array::from_fn(|c| cols[c][r]));
        let scale = mat_vec(inv3(basis), self.native_white());
        let native: Mat = std::array::from_fn(|r| std::array::from_fn(|c| basis[r][c] * scale[c]));
        mat_mul(self.chad(), native)
    }

    /// Encoded → linear, by the published curve.
    fn decode(self, x: f64) -> f64 {
        match self.tone {
            Tone::Iec => {
                if x <= 0.04045 {
                    x / 12.92
                } else {
                    ((x + 0.055) / 1.055).powf(2.4)
                }
            }
            Tone::Gamma(raw) => x.powf(f64::from(raw) / 256.0),
        }
    }

    fn bytes(self) -> Vec<u8> {
        let len = dr_builtin_profile_len(self.which);
        assert!(len > 0, "{} is a built-in", self.name);
        let mut out = vec![0u8; len];
        assert!(unsafe { dr_builtin_profile(self.which, out.as_mut_ptr(), len) });
        out
    }

    fn profile(self) -> Arc<IccProfile> {
        Arc::new(IccProfile::parse(&self.bytes()).expect("a built-in parses"))
    }
}

/// `inv(destination) · source`: linear source RGB → linear destination RGB.
fn combined(source: Space, destination: Space) -> Mat {
    mat_mul(inv3(destination.to_pcs()), source.to_pcs())
}

fn label(source: Space, destination: Space) -> String {
    format!("{} to {}", source.name, destination.name)
}

// ------------------------------------------------------------- plumbing --

fn be32(b: &[u8], at: usize) -> u32 {
    u32::from_be_bytes(b[at..at + 4].try_into().unwrap())
}

/// A tag's `(offset, data)`, walking the table by hand.
fn find_tag<'a>(profile: &'a [u8], sig: &[u8; 4]) -> Option<(usize, &'a [u8])> {
    (0..be32(profile, 128) as usize).find_map(|i| {
        let e = 132 + 12 * i;
        (&profile[e..e + 4] == sig.as_slice()).then(|| {
            let off = be32(profile, e + 4) as usize;
            let size = be32(profile, e + 8) as usize;
            (off, &profile[off..off + size])
        })
    })
}

fn fixed(v: f64) -> i32 {
    (v * 65536.0).round() as i32
}

fn owned(pointer: *mut c_char) -> Option<String> {
    if pointer.is_null() {
        return None;
    }
    let text = unsafe { CStr::from_ptr(pointer) }
        .to_string_lossy()
        .into_owned();
    unsafe { dr_string_free(pointer) };
    Some(text)
}

/// `dr_precision_doc_convert_to_profile`, which must change the document.
fn typed_convert(doc: &PrecisionDocument, profile: &[u8], what: &str) -> PrecisionDocument {
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_convert_to_profile(
            doc,
            profile.as_ptr(),
            profile.len(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    if let Some(message) = owned(err) {
        panic!("{what}: {message}");
    }
    assert!(
        !out.is_null(),
        "{what}: the conversion changes the document"
    );
    *unsafe { Box::from_raw(out) }
}

// ---------------------------------------------------------------- bytes --

#[test]
fn the_wide_built_ins_are_well_formed_and_stable() {
    for space in WIDE {
        let p = space.bytes();
        let name = space.name;
        assert_eq!(be32(&p, 0) as usize, p.len(), "{name}: the size field");
        assert_eq!(p.len() % 4, 0, "{name}: 4-aligned");
        assert_eq!(&p[8..12], &[0x02, 0x10, 0x00, 0x00], "{name}: ICC v2.1.0");
        assert_eq!(&p[12..16], b"mntr", "{name}");
        assert_eq!(&p[16..20], b"RGB ", "{name}");
        assert_eq!(&p[20..24], b"XYZ ", "{name}");
        assert_eq!(
            &p[24..36],
            &[0x07, 0xEA, 0, 1, 0, 1, 0, 0, 0, 0, 0, 0],
            "{name}: the creation date is the constant 2026-01-01T00:00:00Z"
        );
        assert_eq!(&p[36..40], b"acsp", "{name}: the magic");
        for (i, v) in D50_RAW.iter().enumerate() {
            assert_eq!(
                be32(&p, 68 + 4 * i) as i32,
                *v,
                "{name}: the PCS illuminant"
            );
        }
        assert_eq!(be32(&p, 128), 10, "{name}: ten tags");
        let mut trc_offsets = Vec::new();
        for i in 0..10 {
            let e = 132 + 12 * i;
            let off = be32(&p, e + 4) as usize;
            let size = be32(&p, e + 8) as usize;
            assert_eq!(off % 4, 0, "{name}: tag {i} is 4-aligned");
            assert!(off + size <= p.len(), "{name}: tag {i} is inside");
            if p[e..e + 4].ends_with(b"TRC") {
                trc_offsets.push(off);
            }
        }
        assert_eq!(trc_offsets.len(), 3, "{name}");
        assert!(
            trc_offsets.iter().all(|o| *o == trc_offsets[0]),
            "{name}: the three TRC tags share one blob"
        );
        for sig in [
            b"desc", b"cprt", b"wtpt", b"rXYZ", b"gXYZ", b"bXYZ", b"rTRC", b"gTRC", b"bTRC",
            b"chad",
        ] {
            assert!(find_tag(&p, sig).is_some(), "{name}: tag {sig:?}");
        }
        let (_, wtpt) = find_tag(&p, b"wtpt").unwrap();
        for (i, v) in D50_RAW.iter().enumerate() {
            assert_eq!(be32(wtpt, 8 + 4 * i) as i32, *v, "{name}: wtpt is D50");
        }
        // The `desc` text is the name, NUL-terminated.
        let (_, desc) = find_tag(&p, b"desc").unwrap();
        assert_eq!(&desc[0..4], b"desc", "{name}");
        assert_eq!(be32(desc, 8) as usize, name.len() + 1, "{name}");
        assert_eq!(&desc[12..12 + name.len()], name.as_bytes(), "{name}");

        assert_eq!(space.bytes(), p, "{name}: two calls give the same bytes");

        let mut reported: *mut c_char = ptr::null_mut();
        assert_eq!(
            unsafe { dr_icc_inspect(p.as_ptr(), p.len(), &mut reported) },
            ICC_RGB_MATRIX,
            "{name}"
        );
        assert_eq!(owned(reported).as_deref(), Some(name));
    }
    assert_ne!(ADOBE.bytes(), PROPHOTO.bytes());
}

#[test]
fn their_columns_chad_and_tone_curve_are_the_published_data() {
    for space in WIDE {
        let p = space.bytes();
        let name = space.name;
        // The colorant columns, recomputed from the xy primaries.
        let expected = space.to_pcs();
        for (col, sig) in [b"rXYZ", b"gXYZ", b"bXYZ"].iter().enumerate() {
            let (_, data) = find_tag(&p, sig).expect("a colorant tag");
            assert_eq!(&data[0..4], b"XYZ ", "{name}");
            for (row, want_row) in expected.iter().enumerate() {
                let raw = be32(data, 8 + 4 * row) as i32;
                let want = fixed(want_row[col]);
                assert!(
                    (raw - want).abs() <= 2,
                    "{name} {sig:?}[{row}]: {raw} against {want}"
                );
            }
        }
        // `chad`: Bradford D65→D50 for Adobe RGB, the identity for ProPhoto.
        let (_, chad) = find_tag(&p, b"chad").expect("chad");
        assert_eq!(&chad[0..4], b"sf32", "{name}");
        assert_eq!(chad.len(), 44, "{name}");
        let tolerance = if space.white == White::D50 { 0 } else { 2 };
        for (row, want_row) in space.chad().iter().enumerate() {
            for (col, want) in want_row.iter().enumerate() {
                let raw = be32(chad, 8 + 4 * (3 * row + col)) as i32;
                assert!(
                    (raw - fixed(*want)).abs() <= tolerance,
                    "{name} chad[{row}][{col}]: {raw}"
                );
            }
        }
        // The tone curve: a `curv` of one u8Fixed8 gamma, 563/256 for Adobe
        // RGB and 461/256 for ProPhoto (a pure power, no ROMM toe).
        let (_, trc) = find_tag(&p, b"rTRC").expect("rTRC");
        assert_eq!(&trc[0..4], b"curv", "{name}");
        match space.tone {
            Tone::Gamma(gamma_raw) => {
                assert_eq!(trc.len(), 14, "{name}");
                assert_eq!(be32(trc, 8), 1, "{name}: one entry");
                assert_eq!(u16::from_be_bytes([trc[12], trc[13]]), gamma_raw, "{name}");
            }
            Tone::Iec => panic!("{name} is not an IEC space"),
        }

        // The blob and the direct constructor describe one space.
        let direct = match space.which {
            PROFILE_ADOBE_RGB => IccProfile::adobe_rgb(),
            _ => IccProfile::prophoto_rgb(),
        };
        assert_eq!(&direct.bytes()[..], &p[..], "{name}");
        let reparsed = IccProfile::parse(&p).expect("the built-in parses");
        assert_eq!(reparsed.name(), name);
        assert_eq!(direct.name(), name);
        assert!(
            reparsed
                .model()
                .expect("matrix/TRC")
                .approx_eq(direct.model().expect("matrix/TRC")),
            "{name}: the parsed model is the constructor's"
        );
    }
    // The two are not each other, and neither is sRGB.
    let model = |space: Space| space.profile().model().expect("matrix/TRC").clone();
    assert!(!model(ADOBE).approx_eq(&model(PROPHOTO)));
    assert!(!model(ADOBE).approx_eq(&model(SRGB)));
    assert!(!model(PROPHOTO).approx_eq(&model(SRGB)));
}

// ---------------------------------------------------------------- float --

#[test]
fn a_float_convert_is_the_matrix_between_the_published_primaries_unclamped() {
    // Grey 0.18, white and the three primaries, linear.
    let pixels: Vec<[f32; 4]> = vec![
        [0.18, 0.18, 0.18, 1.0],
        [1.0, 1.0, 1.0, 1.0],
        [1.0, 0.0, 0.0, 1.0],
        [0.0, 1.0, 0.0, 1.0],
        [0.0, 0.0, 1.0, 1.0],
    ];
    for (source, destination) in PAIRS {
        let what = label(source, destination);
        let doc = float_doc(5, 1, &source.profile(), std::slice::from_ref(&pixels));
        let out = typed_convert(&doc, &destination.bytes(), &what);
        assert!(out.is_float(), "{what}");
        assert_eq!(out.profile().name(), destination.name, "{what}");
        let got = out
            .layer_source(0)
            .expect("a raster record")
            .to_rgba32()
            .expect("float samples");
        let matrix = combined(source, destination);
        for (i, (g, p)) in got.iter().zip(&pixels).enumerate() {
            let want = mat_vec(matrix, [p[0], p[1], p[2]].map(f64::from));
            for c in 0..3 {
                assert!(
                    (f64::from(g[c]) - want[c]).abs() <= FLOAT_BOUND,
                    "{what} pixel {i} channel {c}: {} against {}",
                    g[c],
                    want[c]
                );
            }
            assert_eq!(g[3], 1.0, "{what} pixel {i}: alpha");
        }
        // Neutral stays neutral: white maps to white.
        for i in [0, 1] {
            for c in 0..3 {
                assert!(
                    (f64::from(got[i][c]) - f64::from(pixels[i][c])).abs() <= FLOAT_BOUND,
                    "{what} grey {i} channel {c}: {}",
                    got[i][c]
                );
            }
        }
        if source.which == PROFILE_ADOBE_RGB && destination.which == PROFILE_SRGB {
            // Adobe RGB green is outside sRGB: its red and blue stay
            // negative, with no clamp.
            assert!(got[3][0] < -0.3 && got[3][2] < -0.03, "{:?}", got[3]);
        }
    }
}

// ----------------------------------------------------------- 16 and 8 bit --

/// The oracle for an encoded pixel: the source's analytic decode, the
/// matrix, then the per-channel clamp to the destination's [0, 1].
fn oracle(source: Space, destination: Space, encoded: [f64; 3]) -> [f64; 3] {
    let linear = encoded.map(|v| source.decode(v));
    mat_vec(combined(source, destination), linear).map(|v| v.clamp(0.0, 1.0))
}

#[test]
fn a_sixteen_bit_convert_lands_on_the_published_matrices() {
    let codes: Vec<[u16; 4]> = vec![
        [4096, 4096, 4096, 65535],
        [32768, 32768, 32768, 65535],
        [65535, 65535, 65535, 65535],
        [65535, 0, 0, 65535],
        [0, 65535, 0, 65535],
        [0, 0, 65535, 65535],
    ];
    for (source, destination) in PAIRS {
        let what = label(source, destination);
        let image = PrecisionImage::from_rgba16(6, 1, codes.clone(), source.profile(), None)
            .expect("a 16-bit image");
        let doc = PrecisionDocument::from_image(image, Metadata::default(), Resolution::default())
            .expect("a 16-bit document");
        let out = typed_convert(&doc, &destination.bytes(), &what);
        assert!(!out.is_float(), "{what}");
        assert_eq!(out.profile().name(), destination.name, "{what}");
        let got = out
            .layer_source(0)
            .expect("a raster record")
            .to_rgba16()
            .expect("16-bit samples");
        assert_eq!(got.len(), codes.len(), "{what}");
        for (i, (g, code)) in got.iter().zip(&codes).enumerate() {
            let encoded = [code[0], code[1], code[2]].map(|v| f64::from(v) / 65535.0);
            let want = oracle(source, destination, encoded);
            for c in 0..3 {
                let linear = destination.decode(f64::from(g[c]) / 65535.0);
                assert!(
                    (linear - want[c]).abs() <= SIXTEEN_BOUND,
                    "{what} pixel {i} channel {c}: code {} is {linear} against {}",
                    g[c],
                    want[c]
                );
            }
            assert_eq!(g[3], 65535, "{what} pixel {i}: alpha");
        }
        // A grey stays a grey: its channels within one code of each other.
        for (i, g) in got.iter().take(3).enumerate() {
            let (low, high) = (g[..3].iter().min().unwrap(), g[..3].iter().max().unwrap());
            assert!(high - low <= 1, "{what} grey {i}: {g:?}");
        }
    }
}

#[test]
fn an_eight_bit_convert_lands_on_the_published_matrices() {
    // The 16-bit test's pixels at 8 bits.
    let codes: [[u8; 4]; 6] = [
        [16, 16, 16, 255],
        [128, 128, 128, 255],
        [255, 255, 255, 255],
        [255, 0, 0, 255],
        [0, 255, 0, 255],
        [0, 0, 255, 255],
    ];
    let srgb = SRGB.bytes();
    for (source, destination) in PAIRS {
        let what = label(source, destination);
        let fresh =
            DrDocument::from_pixels(RgbaImage::from_fn(6, 1, |x, _| Rgba(codes[x as usize])));
        // A fresh document is in the built-in sRGB; any other source is an
        // Assign of it, which changes no pixel.
        let tagged = if source.which == PROFILE_SRGB {
            ptr::null_mut()
        } else {
            let bytes = source.bytes();
            let out = unsafe {
                dr_doc_assign_profile(&fresh, bytes.as_ptr(), bytes.len(), ptr::null_mut())
            };
            assert!(!out.is_null(), "{what}: assign");
            out
        };
        let input: *const DrDocument = if tagged.is_null() { &fresh } else { tagged };
        let target = if destination.which == PROFILE_SRGB {
            srgb.clone()
        } else {
            destination.bytes()
        };
        let mut err: *mut c_char = ptr::null_mut();
        let converted =
            unsafe { dr_doc_convert_to_profile(input, target.as_ptr(), target.len(), &mut err) };
        assert_eq!(owned(err), None, "{what}");
        assert!(!converted.is_null(), "{what}: convert");
        let out = unsafe { &*converted };
        assert_eq!(out.profile.name(), destination.name, "{what}");
        let image = out.layers[0].pixels.contiguous_image();
        for (i, code) in codes.iter().enumerate() {
            let g = image.get_pixel(i as u32, 0).0;
            let encoded = [code[0], code[1], code[2]].map(|v| f64::from(v) / 255.0);
            let want = oracle(source, destination, encoded);
            for c in 0..3 {
                let linear = destination.decode(f64::from(g[c]) / 255.0);
                assert!(
                    (linear - want[c]).abs() <= EIGHT_BOUND,
                    "{what} pixel {i} channel {c}: code {} is {linear} against {}",
                    g[c],
                    want[c]
                );
            }
            assert_eq!(g[3], 255, "{what} pixel {i}: alpha");
        }
        unsafe {
            dr_doc_free(converted);
            if !tagged.is_null() {
                dr_doc_free(tagged);
            }
        }
    }
}
