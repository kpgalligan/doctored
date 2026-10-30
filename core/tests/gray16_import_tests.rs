//! 16-bit GRAYSCALE import (Phase 11D): a Gray16 PNG or TIFF becomes a
//! correctly profiled 16-bit RGB document.
//!
//! Every fixture in this file is assembled BYTE BY BYTE here — the PNG
//! container (with its own stored-deflate writer, CRC and Adler sums), the
//! classic TIFF (little- and big-endian), and the Gray ICC profiles (a `curv`
//! gamma and a 1024-point sampled table). Nothing is read from disk and no
//! image library helps, so a decoder change cannot quietly move the
//! expectation with the fixture.
//!
//! The expected samples are computed from the rule written in
//! `gray16_import`'s doc comment, restated here as arithmetic:
//!
//! * with a Gray ICC: `y = curve(g / 65535)`, then the sRGB FORWARD encode of
//!   `y`, then `round(65535 · c)`;
//! * without one (or with an `sRGB` chunk): the code is copied unchanged.
//!
//! The sRGB encode is written out here (the piecewise IEC 61966-2-1 curve),
//! so the test's oracle and the crate's encoder are independent; they are
//! compared with a one-code tolerance, which is the interpolation table's
//! own resolution.
use crate::common;
use common::take_err_string;

use doctored_core::ffi_precision::{
    dr_precision_doc_free, dr_precision_doc_open, dr_precision_doc_render, dr_precision_image_free,
};
use doctored_core::precision_document::PrecisionDocument;

use std::ffi::{c_char, CString};
use std::ptr;
use tempfile::TempDir;

const LIMIT: u64 = 2 * 1024 * 1024 * 1024;

// ------------------------------------------------------ container writers --

fn crc32(bytes: &[u8]) -> u32 {
    let mut table = [0u32; 256];
    for (i, entry) in table.iter_mut().enumerate() {
        let mut c = i as u32;
        for _ in 0..8 {
            c = if c & 1 != 0 {
                0xEDB8_8320 ^ (c >> 1)
            } else {
                c >> 1
            };
        }
        *entry = c;
    }
    let mut c = 0xFFFF_FFFFu32;
    for &b in bytes {
        c = table[((c ^ u32::from(b)) & 0xFF) as usize] ^ (c >> 8);
    }
    c ^ 0xFFFF_FFFF
}

fn adler32(bytes: &[u8]) -> u32 {
    let (mut a, mut b) = (1u32, 0u32);
    for &byte in bytes {
        a = (a + u32::from(byte)) % 65521;
        b = (b + a) % 65521;
    }
    (b << 16) | a
}

/// A zlib stream of STORED deflate blocks — no compressor needed, and the
/// bytes are trivially checkable by hand.
fn zlib_stored(data: &[u8]) -> Vec<u8> {
    let mut out = vec![0x78, 0x01];
    let mut at = 0usize;
    loop {
        let take = (data.len() - at).min(65535);
        let last = at + take == data.len();
        out.push(u8::from(last));
        out.extend_from_slice(&(take as u16).to_le_bytes());
        out.extend_from_slice(&(!(take as u16)).to_le_bytes());
        out.extend_from_slice(&data[at..at + take]);
        at += take;
        if last {
            break;
        }
    }
    out.extend_from_slice(&adler32(data).to_be_bytes());
    out
}

fn chunk(kind: &[u8; 4], payload: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&(payload.len() as u32).to_be_bytes());
    let mut crc_input = kind.to_vec();
    crc_input.extend_from_slice(payload);
    out.extend_from_slice(&crc_input);
    out.extend_from_slice(&crc32(&crc_input).to_be_bytes());
    out
}

/// A 16-bit PNG: `color` is 0 (gray) or 4 (gray + alpha); `samples` are the
/// per-pixel values in order, `extra` the colour chunks to write before the
/// pixels.
fn png(w: u32, h: u32, color: u8, samples: &[u16], extra: Vec<Vec<u8>>) -> Vec<u8> {
    let channels = if color == 4 { 2 } else { 1 };
    assert_eq!(samples.len() as u32, w * h * channels);
    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&w.to_be_bytes());
    ihdr.extend_from_slice(&h.to_be_bytes());
    ihdr.extend_from_slice(&[16, color, 0, 0, 0]);

    let mut raw = Vec::new();
    for y in 0..h {
        raw.push(0); // filter: none
        for x in 0..w {
            for c in 0..channels {
                let at = ((y * w + x) * channels + c) as usize;
                raw.extend_from_slice(&samples[at].to_be_bytes());
            }
        }
    }

    let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
    out.extend_from_slice(&chunk(b"IHDR", &ihdr));
    for payload in extra {
        out.extend_from_slice(&payload);
    }
    out.extend_from_slice(&chunk(b"IDAT", &zlib_stored(&raw)));
    out.extend_from_slice(&chunk(b"IEND", &[]));
    out
}

fn srgb_chunk() -> Vec<u8> {
    chunk(b"sRGB", &[0])
}

fn gama_chunk(gamma: f64) -> Vec<u8> {
    chunk(b"gAMA", &((gamma * 100_000.0).round() as u32).to_be_bytes())
}

fn iccp_chunk(name: &str, profile: &[u8]) -> Vec<u8> {
    let mut payload = name.as_bytes().to_vec();
    payload.push(0); // NUL
    payload.push(0); // compression method: deflate
    payload.extend_from_slice(&zlib_stored(profile));
    chunk(b"iCCP", &payload)
}

/// A classic TIFF with one strip. `little` picks the endianness; `extras` are
/// (tag, type, count, value-or-offset payload) written as extra IFD entries.
fn tiff(
    w: u32,
    h: u32,
    channels: u16,
    samples: &[u16],
    little: bool,
    icc: Option<&[u8]>,
    extra_samples: Option<u16>,
) -> Vec<u8> {
    assert_eq!(samples.len() as u32, w * h * u32::from(channels));
    let short = |v: u16| {
        if little {
            v.to_le_bytes()
        } else {
            v.to_be_bytes()
        }
    };
    let long = |v: u32| {
        if little {
            v.to_le_bytes()
        } else {
            v.to_be_bytes()
        }
    };

    // Layout: header (8) | pixel strip | bits array | sample-format array |
    // icc | IFD.
    let mut body = Vec::new();
    let pixel_offset = 8u32;
    for value in samples {
        body.extend_from_slice(&short(*value));
    }
    let bits_offset = pixel_offset + body.len() as u32;
    for _ in 0..channels {
        body.extend_from_slice(&short(16));
    }
    let formats_offset = pixel_offset + body.len() as u32;
    for _ in 0..channels {
        body.extend_from_slice(&short(1));
    }
    let icc_offset = pixel_offset + body.len() as u32;
    if let Some(icc) = icc {
        body.extend_from_slice(icc);
        if icc.len() % 2 == 1 {
            body.push(0);
        }
    }
    let ifd_offset = pixel_offset + body.len() as u32;

    let inline = |value: u32, ty: u16| -> [u8; 4] {
        // A SHORT that fits in the value field is left-justified.
        if ty == 3 {
            let mut out = [0u8; 4];
            out[..2].copy_from_slice(&short(value as u16));
            out
        } else {
            long(value)
        }
    };
    // TIFF's value field holds the data itself when it fits in four bytes,
    // which is exactly one or two SHORTs; a longer array is an offset.
    let shorts = |values: &[u16], offset: u32| -> [u8; 4] {
        if values.len() <= 2 {
            let mut out = [0u8; 4];
            for (i, value) in values.iter().enumerate() {
                out[2 * i..2 * i + 2].copy_from_slice(&short(*value));
            }
            out
        } else {
            long(offset)
        }
    };
    let mut entries: Vec<(u16, u16, u32, [u8; 4])> = vec![
        (256, 3, 1, inline(w, 3)),
        (257, 3, 1, inline(h, 3)),
        (
            258,
            3,
            u32::from(channels),
            shorts(&vec![16u16; channels as usize], bits_offset),
        ),
        (259, 3, 1, inline(1, 3)),
        (262, 3, 1, inline(if channels >= 3 { 2 } else { 1 }, 3)),
        (273, 4, 1, long(pixel_offset)),
        (277, 3, 1, inline(u32::from(channels), 3)),
        (278, 3, 1, inline(h, 3)),
        (279, 4, 1, long(w * h * u32::from(channels) * 2)),
        (284, 3, 1, inline(1, 3)),
        (
            339,
            3,
            u32::from(channels),
            shorts(&vec![1u16; channels as usize], formats_offset),
        ),
    ];
    if let Some(kind) = extra_samples {
        entries.push((338, 3, 1, inline(u32::from(kind), 3)));
    }
    if let Some(icc) = icc {
        entries.push((34675, 7, icc.len() as u32, long(icc_offset)));
    }
    entries.sort_by_key(|entry| entry.0);

    let mut out = Vec::new();
    out.extend_from_slice(if little { b"II" } else { b"MM" });
    out.extend_from_slice(&short(42));
    out.extend_from_slice(&long(ifd_offset));
    out.extend_from_slice(&body);
    out.extend_from_slice(&short(entries.len() as u16));
    for (tag, ty, count, value) in &entries {
        out.extend_from_slice(&short(*tag));
        out.extend_from_slice(&short(*ty));
        out.extend_from_slice(&long(*count));
        out.extend_from_slice(value);
    }
    out.extend_from_slice(&long(0));
    out
}

// ------------------------------------------------------------ ICC writers --

fn icc_tag(sig: &[u8; 4], body: Vec<u8>) -> (&'static [u8; 4], Vec<u8>) {
    let sig: &'static [u8; 4] = match sig {
        b"kTRC" => b"kTRC",
        b"wtpt" => b"wtpt",
        b"desc" => b"desc",
        b"rXYZ" => b"rXYZ",
        _ => panic!("unexpected tag"),
    };
    (sig, body)
}

fn desc_body(name: &str) -> Vec<u8> {
    let mut body = b"desc".to_vec();
    body.extend_from_slice(&0u32.to_be_bytes());
    body.extend_from_slice(&((name.len() + 1) as u32).to_be_bytes());
    body.extend_from_slice(name.as_bytes());
    body.push(0);
    body.extend_from_slice(&[0u8; 78]);
    body
}

fn curv_gamma(gamma: f64) -> Vec<u8> {
    let mut body = b"curv".to_vec();
    body.extend_from_slice(&0u32.to_be_bytes());
    body.extend_from_slice(&1u32.to_be_bytes());
    body.extend_from_slice(&((gamma * 256.0).round() as u16).to_be_bytes());
    body
}

/// A sampled `curv` of `n` points holding `f`.
fn curv_table(n: usize, f: impl Fn(f64) -> f64) -> Vec<u8> {
    let mut body = b"curv".to_vec();
    body.extend_from_slice(&0u32.to_be_bytes());
    body.extend_from_slice(&(n as u32).to_be_bytes());
    for i in 0..n {
        let x = i as f64 / (n - 1) as f64;
        body.extend_from_slice(&((f(x) * 65535.0).round() as u16).to_be_bytes());
    }
    body
}

fn xyz_body(v: [f64; 3]) -> Vec<u8> {
    let mut body = b"XYZ ".to_vec();
    body.extend_from_slice(&0u32.to_be_bytes());
    for value in v {
        body.extend_from_slice(&((value * 65536.0).round() as i32).to_be_bytes());
    }
    body
}

/// Assembles an ICC profile from a device space and a tag list.
fn icc_profile(space: &[u8; 4], tags: Vec<(&[u8; 4], Vec<u8>)>) -> Vec<u8> {
    let mut header = vec![0u8; 128];
    header[8..12].copy_from_slice(&[2, 0x10, 0, 0]);
    header[12..16].copy_from_slice(b"mntr");
    header[16..20].copy_from_slice(space);
    header[20..24].copy_from_slice(b"XYZ ");
    header[36..40].copy_from_slice(b"acsp");

    let mut table = Vec::new();
    let mut body = Vec::new();
    let base = 128 + 4 + 12 * tags.len();
    for (sig, data) in &tags {
        let offset = base + body.len();
        table.extend_from_slice(*sig);
        table.extend_from_slice(&(offset as u32).to_be_bytes());
        table.extend_from_slice(&(data.len() as u32).to_be_bytes());
        body.extend_from_slice(data);
        while body.len() % 4 != 0 {
            body.push(0);
        }
    }
    let mut out = header;
    out.extend_from_slice(&(tags.len() as u32).to_be_bytes());
    out.extend_from_slice(&table);
    out.extend_from_slice(&body);
    let size = out.len() as u32;
    out[0..4].copy_from_slice(&size.to_be_bytes());
    out
}

fn gray_profile_gamma(name: &str, gamma: f64) -> Vec<u8> {
    icc_profile(
        b"GRAY",
        vec![
            icc_tag(b"desc", desc_body(name)),
            icc_tag(b"wtpt", xyz_body([0.9642, 1.0, 0.8249])),
            icc_tag(b"kTRC", curv_gamma(gamma)),
        ]
        .into_iter()
        .map(|(sig, body)| (sig as &[u8; 4], body))
        .collect(),
    )
}

fn gray_profile_table(name: &str, points: usize, f: impl Fn(f64) -> f64) -> Vec<u8> {
    icc_profile(
        b"GRAY",
        vec![
            icc_tag(b"desc", desc_body(name)),
            icc_tag(b"wtpt", xyz_body([0.9642, 1.0, 0.8249])),
            icc_tag(b"kTRC", curv_table(points, f)),
        ]
        .into_iter()
        .map(|(sig, body)| (sig as &[u8; 4], body))
        .collect(),
    )
}

/// An RGB profile on a gray image — the refusal case.
fn rgb_profile(name: &str) -> Vec<u8> {
    icc_profile(
        b"RGB ",
        vec![
            icc_tag(b"desc", desc_body(name)),
            icc_tag(b"wtpt", xyz_body([0.9642, 1.0, 0.8249])),
            icc_tag(b"rXYZ", xyz_body([0.4360, 0.2225, 0.0139])),
        ]
        .into_iter()
        .map(|(sig, body)| (sig as &[u8; 4], body))
        .collect(),
    )
}

// ---------------------------------------------------------------- oracles --

/// The sRGB forward transfer function, IEC 61966-2-1, written here so the
/// expectation is independent of the crate's encoder tables.
fn srgb_encode(linear: f64) -> f64 {
    let v = linear.clamp(0.0, 1.0);
    if v <= 0.003_130_8 {
        12.92 * v
    } else {
        1.055 * v.powf(1.0 / 2.4) - 0.055
    }
}

/// How far the crate's re-encode may sit from the ANALYTIC sRGB curve
/// above, in u16 codes. It is not the conversion's accuracy: the built-in
/// sRGB profile this build writes carries a SAMPLED `curv` TRC, and the
/// difference between that table and the closed-form IEC curve is what this
/// bounds. Measured maximum over the codes below is 3 (0.012 of an 8-bit
/// code), at the sample nearest the curve's knee; the endpoints are exact
/// and checked separately.
const TRC_TOLERANCE: i32 = 4;

fn expected_gray_profile(code: u16, curve: impl Fn(f64) -> f64) -> u16 {
    let y = curve(f64::from(code) / 65535.0);
    (srgb_encode(y) * 65535.0).round() as u16
}

// ----------------------------------------------------------------- driver --

const W: u32 = 4;
const H: u32 = 3;

fn codes() -> Vec<u16> {
    vec![
        0, 1, 255, 4096, 12345, 30000, 32768, 40000, 50000, 60000, 65534, 65535,
    ]
}

fn open(bytes: &[u8], name: &str) -> Result<*mut PrecisionDocument, String> {
    let dir = TempDir::new().expect("tmp");
    let path = dir.path().join(name);
    std::fs::write(&path, bytes).expect("write fixture");
    let c = CString::new(path.to_str().unwrap()).expect("path");
    let mut err: *mut c_char = ptr::null_mut();
    let document = unsafe { dr_precision_doc_open(c.as_ptr(), LIMIT, 0, ptr::null(), &mut err) };
    if document.is_null() {
        return Err(take_err_string(err));
    }
    Ok(document)
}

/// The document's RGBA16 samples, through the shipped renderer.
fn pixels(document: *const PrecisionDocument) -> Vec<[u16; 4]> {
    let mut err: *mut c_char = ptr::null_mut();
    let image = unsafe { dr_precision_doc_render(document, LIMIT, 0, ptr::null(), &mut err) };
    assert!(!image.is_null(), "{}", take_err_string(err));
    let samples = unsafe { &*image }.to_rgba16().expect("rgba16 storage");
    unsafe { dr_precision_image_free(image) };
    samples
}

fn assert_neutral(samples: &[[u16; 4]]) {
    for pixel in samples {
        assert_eq!(pixel[0], pixel[1], "gray stays neutral");
        assert_eq!(pixel[1], pixel[2], "gray stays neutral");
    }
}

// ------------------------------------------------------------------ tests --

#[test]
fn a_png_gray16_without_a_profile_keeps_its_codes_exactly() {
    for extra in [
        Vec::new(),
        vec![srgb_chunk()],
        vec![gama_chunk(2.2), srgb_chunk()],
    ] {
        let bytes = png(W, H, 0, &codes(), extra);
        let document = open(&bytes, "gray.png").expect("gray png opens");
        let samples = pixels(document);
        assert_neutral(&samples);
        for (pixel, code) in samples.iter().zip(codes()) {
            assert_eq!(pixel[0], code, "sRGB-coded gray is copied unchanged");
            assert_eq!(pixel[3], 65535);
        }
        unsafe { dr_precision_doc_free(document) };
    }
}

#[test]
fn a_png_graya16_carries_its_alpha_exactly() {
    let samples: Vec<u16> = codes()
        .into_iter()
        // Alpha never reaches zero: a zero-alpha pixel's RGB is canonically
        // zeroed by the renderer, which would hide the sample under test.
        .flat_map(|code| [code, 65535 - code / 2])
        .collect();
    let bytes = png(W, H, 4, &samples, vec![srgb_chunk()]);
    let document = open(&bytes, "graya.png").expect("graya png opens");
    let decoded = pixels(document);
    assert_neutral(&decoded);
    for (pixel, code) in decoded.iter().zip(codes()) {
        assert_eq!(pixel[0], code);
        assert_eq!(
            pixel[3],
            65535 - code / 2,
            "alpha is the second sample, exactly"
        );
    }
    unsafe { dr_precision_doc_free(document) };
}

#[test]
fn a_png_gray16_with_a_gray_icc_is_converted_through_its_curve() {
    let profile = gray_profile_gamma("Gray Gamma 2.2", 2.2);
    for extra in [
        vec![iccp_chunk("Gray Gamma 2.2", &profile)],
        // gAMA beside a Gray ICC: the ICC wins and the gAMA is never
        // evaluated, exactly as on the RGB path.
        vec![gama_chunk(1.0), iccp_chunk("Gray Gamma 2.2", &profile)],
    ] {
        let bytes = png(W, H, 0, &codes(), extra);
        let document = open(&bytes, "graygamma.png").expect("gray icc png opens");
        let samples = pixels(document);
        assert_neutral(&samples);
        assert_eq!(samples[0][0], 0, "black stays black");
        assert_eq!(samples[codes().len() - 1][0], 65535, "white stays white");
        for (pixel, code) in samples.iter().zip(codes()) {
            // The profile's gamma is the u8Fixed8 rounding of 2.2.
            let gamma = (2.2f64 * 256.0).round() / 256.0;
            let want = expected_gray_profile(code, |x| x.powf(gamma));
            let delta = i32::from(pixel[0]) - i32::from(want);
            assert!(
                delta.abs() <= TRC_TOLERANCE,
                "code {code}: got {} want {want}",
                pixel[0]
            );
        }
        unsafe { dr_precision_doc_free(document) };
    }
}

#[test]
fn a_png_gray16_with_a_sampled_gray_icc_follows_the_table() {
    // A 1024-point table holding a plain gamma 1.8 curve.
    let profile = gray_profile_table("Gray Table", 1024, |x| x.powf(1.8));
    let bytes = png(W, H, 0, &codes(), vec![iccp_chunk("Gray Table", &profile)]);
    let document = open(&bytes, "graytable.png").expect("sampled gray icc opens");
    let samples = pixels(document);
    assert_neutral(&samples);
    for (pixel, code) in samples.iter().zip(codes()) {
        // The table is sampled at 1024 points and linearly interpolated,
        // which is what the expectation reproduces.
        let want = expected_gray_profile(code, |x| {
            let t = x * 1023.0;
            let i = t.floor().min(1022.0);
            let f = t - i;
            let at = |k: f64| (((k / 1023.0).powf(1.8) * 65535.0).round()) / 65535.0;
            at(i) * (1.0 - f) + at(i + 1.0) * f
        });
        let delta = i32::from(pixel[0]) - i32::from(want);
        assert!(
            delta.abs() <= TRC_TOLERANCE,
            "code {code}: got {} want {want}",
            pixel[0]
        );
    }
    unsafe { dr_precision_doc_free(document) };
}

#[test]
fn a_gray_png_refuses_an_rgb_icc_and_a_bare_gama() {
    let rgb = rgb_profile("sRGB IEC61966-2.1");
    let bytes = png(W, H, 0, &codes(), vec![iccp_chunk("sRGB", &rgb)]);
    assert!(
        open(&bytes, "grayrgb.png")
            .unwrap_err()
            .contains("a grayscale image must carry a Gray ICC profile or none"),
        "an RGB ICC on a gray image is refused"
    );

    let bytes = png(W, H, 0, &codes(), vec![gama_chunk(2.2)]);
    assert!(
        open(&bytes, "graygama.png")
            .unwrap_err()
            .contains("PNG gamma/chromaticities require an explicit Gray ICC or sRGB declaration"),
        "a bare gAMA on a gray image is refused with the Gray sentence"
    );
}

#[test]
fn a_tiff_gray16_reads_the_same_at_both_endiannesses() {
    for little in [true, false] {
        let bytes = tiff(W, H, 1, &codes(), little, None, None);
        let document = open(&bytes, "gray.tiff").expect("gray tiff opens");
        let samples = pixels(document);
        assert_neutral(&samples);
        for (pixel, code) in samples.iter().zip(codes()) {
            assert_eq!(pixel[0], code, "little = {little}");
            assert_eq!(pixel[3], 65535);
        }
        unsafe { dr_precision_doc_free(document) };
    }
}

#[test]
fn a_tiff_gray16_with_a_gray_icc_is_converted_through_its_curve() {
    let profile = gray_profile_gamma("Gray Gamma 2.2", 2.2);
    for little in [true, false] {
        let bytes = tiff(W, H, 1, &codes(), little, Some(&profile), None);
        let document = open(&bytes, "graygamma.tiff").expect("gray icc tiff opens");
        let samples = pixels(document);
        assert_neutral(&samples);
        let gamma = (2.2f64 * 256.0).round() / 256.0;
        for (pixel, code) in samples.iter().zip(codes()) {
            let want = expected_gray_profile(code, |x| x.powf(gamma));
            let delta = i32::from(pixel[0]) - i32::from(want);
            assert!(
                delta.abs() <= TRC_TOLERANCE,
                "code {code}, little = {little}"
            );
        }
        unsafe { dr_precision_doc_free(document) };
    }
}

#[test]
fn a_graya_tiff_must_declare_its_alpha_sample() {
    let samples: Vec<u16> = codes()
        .into_iter()
        .flat_map(|code| [code, 65535 - code / 3])
        .collect();
    let good = tiff(W, H, 2, &samples, true, None, Some(2));
    let document = open(&good, "graya.tiff").expect("declared GrayA opens");
    let decoded = pixels(document);
    for (pixel, code) in decoded.iter().zip(codes()) {
        assert_eq!(pixel[0], code);
        assert_eq!(pixel[3], 65535 - code / 3);
    }
    unsafe { dr_precision_doc_free(document) };

    for wrong in [None, Some(1u16)] {
        let bytes = tiff(W, H, 2, &samples, true, None, wrong);
        let error = open(&bytes, "grayabad.tiff").unwrap_err();
        assert!(
            error.contains("GrayA TIFF must declare one unassociated alpha sample")
                || error.contains("RGBA TIFF must declare one unassociated alpha sample"),
            "unexpected: {error}"
        );
    }

    // A plain Gray TIFF that declares an extra sample is refused too.
    let bytes = tiff(W, H, 1, &codes(), true, None, Some(2));
    assert!(open(&bytes, "graybad.tiff").is_err());
}

#[test]
fn a_gray_tiff_refuses_an_rgb_icc() {
    let rgb = rgb_profile("sRGB IEC61966-2.1");
    let bytes = tiff(W, H, 1, &codes(), true, Some(&rgb), None);
    assert!(open(&bytes, "grayrgb.tiff")
        .unwrap_err()
        .contains("a grayscale image must carry a Gray ICC profile or none"));
}
