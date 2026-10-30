//! The floating-point TIFF seam (Phase 11D).
//!
//! Every input here is hand-assembled: the IFD, the strip, the endianness,
//! the floating-point predictor's byte shuffle and the Deflate stream (as
//! STORED deflate blocks, which needs no compressor and is a valid zlib
//! stream). So a decode that agreed with the fixture would be agreeing with
//! arithmetic written in this file, not with a library that produced both
//! sides. The BigTIFF twins are the one exception: they come from the
//! `tiff` crate's encoder, and are compared with each other, not with it.
//!
//! The seam has no FFI export of its own (the app reaches it through
//! `float_io`), so these tests pin the container layer directly: exact
//! samples, both endiannesses, both
//! widths, predictor 3, the round trip through the encoder, and every
//! refusal by its named element.

use doctored_core::tiff_float::{self, FloatSampleType, FloatTiff};

use crate::exr_builder::{half_value, nearest_half_bits};

const LIMIT: u64 = 512 * 1024 * 1024;

// ------------------------------------------------------------- the writer --

fn adler32(bytes: &[u8]) -> u32 {
    let (mut a, mut b) = (1u32, 0u32);
    for &byte in bytes {
        a = (a + u32::from(byte)) % 65521;
        b = (b + a) % 65521;
    }
    (b << 16) | a
}

/// A zlib stream of STORED deflate blocks: no compressor, still exactly what
/// TIFF compression 8 means.
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

/// TIFF's floating-point predictor, forwards, over ONE row of BIG-ENDIAN
/// sample bytes (the predictor is endianness-free: the byte planes are
/// always most-significant first, which is why the file's byte order does
/// not enter here).
///
/// Two steps, both from libtiff's `fpDiff`: transpose the row into
/// `bytes_per_sample` byte planes, most significant plane first; then
/// difference the whole transposed row BYTEWISE at a stride of one PIXEL's
/// samples.
fn float_predict(row_be: &[u8], bytes_per_sample: usize, stride: usize) -> Vec<u8> {
    let n = row_be.len() / bytes_per_sample;
    let mut shuffled = vec![0u8; row_be.len()];
    for sample in 0..n {
        for byte in 0..bytes_per_sample {
            shuffled[byte * n + sample] = row_be[sample * bytes_per_sample + byte];
        }
    }
    let mut out = shuffled.clone();
    for i in (stride..out.len()).rev() {
        out[i] = shuffled[i].wrapping_sub(shuffled[i - stride]);
    }
    out
}

struct Options {
    little: bool,
    bits: u16,
    sample_format: u16,
    channels: u16,
    predictor: u16,
    deflate: bool,
    planar: u16,
    extra_samples: Option<Vec<u16>>,
    second_page: bool,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            little: true,
            bits: 32,
            sample_format: 3,
            channels: 1,
            predictor: 1,
            deflate: false,
            planar: 1,
            extra_samples: None,
            second_page: false,
        }
    }
}

/// Encodes `values` (already in the file's sample width) as one strip.
fn strip_bytes(values: &[f32], options: &Options, width: usize) -> Vec<u8> {
    let step = usize::from(options.bits / 8);
    let mut raw = Vec::new();
    for value in values {
        match options.bits {
            16 => raw.extend_from_slice(&half_bits(*value).to_be_bytes()),
            _ => raw.extend_from_slice(&value.to_be_bytes()),
        }
    }
    if options.predictor == 3 {
        let per_row = width * usize::from(options.channels) * step;
        let mut out = Vec::new();
        for row in raw.chunks_exact(per_row) {
            out.extend_from_slice(&float_predict(row, step, usize::from(options.channels)));
        }
        raw = out;
    } else if options.little {
        for chunk in raw.chunks_exact_mut(step) {
            chunk.reverse();
        }
    }
    if options.deflate {
        raw = zlib_stored(&raw);
    }
    raw
}

/// The IEEE 754 binary16 encoding of an f32, written here rather than taken
/// from a crate so the half-precision fixtures are this file's own.
fn half_bits(value: f32) -> u16 {
    let bits = value.to_bits();
    let sign = ((bits >> 16) & 0x8000) as u16;
    let exponent = ((bits >> 23) & 0xFF) as i32;
    let mantissa = bits & 0x007F_FFFF;
    if exponent == 0xFF {
        // Infinity or NaN.
        return sign | 0x7C00 | if mantissa != 0 { 0x0200 } else { 0 };
    }
    let unbiased = exponent - 127 + 15;
    if unbiased >= 0x1F {
        return sign | 0x7C00;
    }
    if unbiased <= 0 {
        return sign;
    }
    sign | ((unbiased as u16) << 10) | ((mantissa >> 13) as u16)
}

fn build(width: u32, height: u32, values: &[f32], options: &Options) -> Vec<u8> {
    let short = |v: u16| {
        if options.little {
            v.to_le_bytes()
        } else {
            v.to_be_bytes()
        }
    };
    let long = |v: u32| {
        if options.little {
            v.to_le_bytes()
        } else {
            v.to_be_bytes()
        }
    };
    let strip = strip_bytes(values, options, width as usize);

    let mut body = Vec::new();
    let strip_offset = 8u32;
    body.extend_from_slice(&strip);
    while body.len() % 2 != 0 {
        body.push(0);
    }
    let arrays_offset = strip_offset + body.len() as u32;
    let mut arrays = Vec::new();
    for _ in 0..options.channels {
        arrays.extend_from_slice(&short(options.bits));
    }
    let formats_at = arrays_offset + arrays.len() as u32;
    for _ in 0..options.channels {
        arrays.extend_from_slice(&short(options.sample_format));
    }
    body.extend_from_slice(&arrays);
    let ifd_offset = strip_offset + body.len() as u32;

    let inline_short = |values: &[u16], offset: u32| -> [u8; 4] {
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
    let channels = options.channels;
    let mut entries: Vec<(u16, u16, u32, [u8; 4])> = vec![
        (256, 3, 1, inline_short(&[width as u16], 0)),
        (257, 3, 1, inline_short(&[height as u16], 0)),
        (
            258,
            3,
            u32::from(channels),
            inline_short(&vec![options.bits; channels as usize], arrays_offset),
        ),
        (
            259,
            3,
            1,
            inline_short(&[if options.deflate { 8 } else { 1 }], 0),
        ),
        (
            262,
            3,
            1,
            inline_short(&[if channels >= 3 { 2 } else { 1 }], 0),
        ),
        (273, 4, 1, long(strip_offset)),
        (277, 3, 1, inline_short(&[channels], 0)),
        (278, 3, 1, inline_short(&[height as u16], 0)),
        (279, 4, 1, long(strip.len() as u32)),
        (284, 3, 1, inline_short(&[options.planar], 0)),
        (317, 3, 1, inline_short(&[options.predictor], 0)),
        (
            339,
            3,
            u32::from(channels),
            inline_short(&vec![options.sample_format; channels as usize], formats_at),
        ),
    ];
    if let Some(extras) = &options.extra_samples {
        entries.push((
            338,
            3,
            extras.len() as u32,
            inline_short(extras, arrays_offset),
        ));
    }
    entries.sort_by_key(|entry| entry.0);

    let mut out = Vec::new();
    out.extend_from_slice(if options.little { b"II" } else { b"MM" });
    out.extend_from_slice(&short(42));
    out.extend_from_slice(&long(ifd_offset));
    out.extend_from_slice(&body);
    let ifd_len = 2 + 12 * entries.len() as u32 + 4;
    out.extend_from_slice(&short(entries.len() as u16));
    for (tag, ty, count, value) in &entries {
        out.extend_from_slice(&short(*tag));
        out.extend_from_slice(&short(*ty));
        out.extend_from_slice(&long(*count));
        out.extend_from_slice(value);
    }
    if options.second_page {
        // A second IFD pointing at the same strip — a multipage file.
        let next = ifd_offset + ifd_len;
        out.extend_from_slice(&long(next));
        out.extend_from_slice(&short(entries.len() as u16));
        for (tag, ty, count, value) in &entries {
            out.extend_from_slice(&short(*tag));
            out.extend_from_slice(&short(*ty));
            out.extend_from_slice(&long(*count));
            out.extend_from_slice(value);
        }
        out.extend_from_slice(&long(0));
    } else {
        out.extend_from_slice(&long(0));
    }
    out
}

// ------------------------------------------------------------- the values --

const W: u32 = 4;
const H: u32 = 2;

/// Values chosen to exercise the whole range a float file can hold: zero,
/// a negative, a subnormal-ish small number, ordinary SDR values, and an
/// above-one scene value.
fn values(channels: usize) -> Vec<f32> {
    let base = [0.0f32, -0.25, 1.0, 0.5, 0.125, 7.5, 1.0e-4, 1234.5];
    (0..(W * H) as usize * channels)
        .map(|i| base[i % base.len()] + (i as f32) * 0.015_625)
        .collect()
}

// ------------------------------------------------------------- the checks --

#[test]
fn a_float32_tiff_decodes_exactly_at_both_endiannesses() {
    let samples = values(1);
    for little in [true, false] {
        let options = Options {
            little,
            ..Options::default()
        };
        let bytes = build(W, H, &samples, &options);
        let plan = tiff_float::inspect(&bytes, "fixture").expect("inspect");
        assert_eq!(plan.width, W);
        assert_eq!(plan.height, H);
        assert_eq!(plan.channels, 1);
        assert_eq!(plan.sample_type, FloatSampleType::F32);
        assert_eq!(plan.alpha, None);
        assert_eq!(plan.predictor, 1);

        let image = tiff_float::decode(&bytes, "fixture", LIMIT).expect("decode");
        assert_eq!(image.samples, samples, "little = {little}");
        assert_eq!(image.non_finite, 0);
    }
}

#[test]
fn every_channel_count_and_alpha_declaration_round_trips_through_inspect() {
    for (channels, extras, alpha) in [
        (1u16, None, None),
        (3, None, None),
        (4, Some(vec![2u16]), Some(false)),
        (4, Some(vec![1u16]), Some(true)),
    ] {
        let samples = values(channels as usize);
        let options = Options {
            channels,
            extra_samples: extras,
            ..Options::default()
        };
        let bytes = build(W, H, &samples, &options);
        let plan = tiff_float::inspect(&bytes, "fixture").expect("inspect");
        assert_eq!(plan.channels, channels as u8);
        assert_eq!(plan.alpha, alpha);
        let image = tiff_float::decode(&bytes, "fixture", LIMIT).expect("decode");
        assert_eq!(image.samples, samples);
        assert_eq!(image.alpha, alpha);
    }
}

#[test]
fn the_floating_point_predictor_is_undone() {
    let samples = values(3);
    for little in [true, false] {
        let options = Options {
            little,
            channels: 3,
            predictor: 3,
            ..Options::default()
        };
        let bytes = build(W, H, &samples, &options);
        let plan = tiff_float::inspect(&bytes, "fixture").expect("inspect");
        assert_eq!(plan.predictor, 3);
        let image = tiff_float::decode(&bytes, "fixture", LIMIT).expect("decode");
        assert_eq!(image.samples, samples, "predictor 3, little = {little}");
    }
}

#[test]
fn a_deflate_strip_decodes_to_the_same_samples() {
    let samples = values(1);
    let plain = build(W, H, &samples, &Options::default());
    let deflated = build(
        W,
        H,
        &samples,
        &Options {
            deflate: true,
            ..Options::default()
        },
    );
    assert_ne!(plain, deflated, "the two fixtures are different files");
    let plan = tiff_float::inspect(&deflated, "fixture").expect("inspect");
    assert_eq!(plan.compression, 8);
    assert_eq!(
        tiff_float::decode(&deflated, "fixture", LIMIT)
            .expect("decode")
            .samples,
        samples
    );
}

#[test]
fn half_samples_widen_exactly() {
    // Values that are all exactly representable as binary16, so "widening is
    // exact" is checkable as an equality rather than a tolerance.
    let samples: Vec<f32> = vec![0.0, -0.5, 1.0, 0.25, 2.0, -8.0, 0.125, 64.0];
    let options = Options {
        bits: 16,
        ..Options::default()
    };
    let bytes = build(W, H, &samples, &options);
    let plan = tiff_float::inspect(&bytes, "fixture").expect("inspect");
    assert_eq!(plan.sample_type, FloatSampleType::F16);
    let image = tiff_float::decode(&bytes, "fixture", LIMIT).expect("decode");
    assert_eq!(image.samples, samples);
}

#[test]
fn nan_and_infinity_are_counted_and_passed_through() {
    let samples: Vec<f32> = vec![
        0.0,
        f32::NAN,
        f32::INFINITY,
        f32::NEG_INFINITY,
        1.0,
        0.5,
        -0.25,
        3.0,
    ];
    let bytes = build(W, H, &samples, &Options::default());
    let image = tiff_float::decode(&bytes, "fixture", LIMIT).expect("decode");
    assert_eq!(image.non_finite, 3, "one NaN and two infinities");
    assert!(image.samples[1].is_nan(), "the NaN is passed through");
    assert_eq!(image.samples[2], f32::INFINITY);
    assert_eq!(image.samples[3], f32::NEG_INFINITY);
    assert_eq!(image.samples[4], 1.0);
}

#[test]
fn the_encoder_round_trips_every_written_sample() {
    for channels in [1u8, 3, 4] {
        let samples = values(channels as usize);
        for deflate in [false, true] {
            let image = FloatTiff {
                width: W,
                height: H,
                channels,
                alpha: if channels == 4 { Some(false) } else { None },
                samples: samples.clone(),
                icc: None,
                resolution: None,
                metadata: Default::default(),
                non_finite: 0,
            };
            let bytes = tiff_float::encode_to_vec(&image, deflate).expect("encode");
            let plan = tiff_float::inspect(&bytes, "written").expect("inspect");
            assert_eq!(plan.channels, channels);
            assert_eq!(plan.sample_type, FloatSampleType::F32);
            assert_eq!(plan.predictor, 1, "the encoder writes no predictor");
            assert_eq!(plan.compression, if deflate { 8 } else { 1 });
            let decoded = tiff_float::decode(&bytes, "written", LIMIT).expect("decode");
            assert_eq!(decoded.samples, samples, "channels {channels}");
            assert_eq!(decoded.width, W);
            assert_eq!(decoded.height, H);
            if channels == 4 {
                assert_eq!(decoded.alpha, Some(false));
            }
        }
    }
}

/// The half encoder against the hand-written half arithmetic
/// in `common/exr_builder.rs`: every finite half pattern survives exactly,
/// and an f32 between two halves lands on the nearer one, ties to even.
#[test]
fn the_half_encoder_round_trips_every_finite_half_and_rounds_to_nearest() {
    let patterns: Vec<u16> = (0..=u16::MAX).filter(|b| (b >> 10) & 0x1f != 31).collect();
    assert_eq!(patterns.len(), 63_488);
    let (width, height) = (248u32, 256u32);
    let exact: Vec<f32> = patterns.iter().map(|&bits| half_value(bits)).collect();
    // Values that are NOT halves: between neighbours, on ties, at both ends.
    let between: Vec<f32> = (0..width * height)
        .map(|i| {
            let x = i as f32;
            match i % 4 {
                0 => x * 0.000_123_456 - 3.0,
                1 => 1.0 + (x + 0.5) / 1024.0 / 64.0,
                2 => 65504.0 - x * 0.37,
                _ => -(x * x) * 1e-6,
            }
        })
        .collect();
    for (samples, what) in [(exact, "exact"), (between, "between")] {
        for (channels, deflate) in [(1u8, false), (1, true)] {
            let image = FloatTiff {
                width,
                height,
                channels,
                alpha: None,
                samples: samples.clone(),
                icc: None,
                resolution: None,
                metadata: Default::default(),
                non_finite: 0,
            };
            let bytes = tiff_float::encode_half_to_vec(&image, deflate).expect("encode");
            let plan = tiff_float::inspect(&bytes, "written").expect("inspect");
            assert_eq!(plan.sample_type, FloatSampleType::F16);
            assert_eq!(plan.predictor, 1);
            assert_eq!(plan.compression, if deflate { 8 } else { 1 });
            let want: Vec<u16> = samples.iter().map(|&v| nearest_half_bits(v)).collect();
            if !deflate {
                // The uncompressed strip is the little-endian words themselves.
                let words: Vec<u8> = want.iter().flat_map(|w| w.to_le_bytes()).collect();
                assert!(
                    bytes.windows(words.len()).any(|window| window == words),
                    "{what}: the strip holds the nearest halves"
                );
            }
            let decoded = tiff_float::decode(&bytes, "written", LIMIT).expect("decode");
            for (i, got) in decoded.samples.iter().enumerate() {
                assert_eq!(
                    got.to_bits(),
                    half_value(want[i]).to_bits(),
                    "{what} sample {i} = {}",
                    samples[i]
                );
            }
        }
    }
    // Four samples keep their alpha meaning at half width too.
    let rgba = FloatTiff {
        width: 2,
        height: 1,
        channels: 4,
        alpha: Some(false),
        samples: vec![0.5, 2.0, -1.0, 0.25, 8.0, 0.0, 1.0, 1.0],
        icc: None,
        resolution: None,
        metadata: Default::default(),
        non_finite: 0,
    };
    let bytes = tiff_float::encode_half_to_vec(&rgba, true).expect("encode");
    let decoded = tiff_float::decode(&bytes, "written", LIMIT).expect("decode");
    assert_eq!(decoded.alpha, Some(false));
    assert_eq!(decoded.samples, rgba.samples);
}

#[test]
fn the_encoder_refuses_what_it_cannot_write() {
    let image = FloatTiff {
        width: W,
        height: H,
        channels: 2,
        alpha: Some(false),
        samples: values(2),
        icc: None,
        resolution: None,
        metadata: Default::default(),
        non_finite: 0,
    };
    // #547 phase 3c: gray + alpha is written (BlackIsZero, ExtraSamples 2)
    // and reads back sample for sample.
    let written = tiff_float::encode_to_vec(&image, false).expect("gray + alpha encodes");
    let decoded = tiff_float::decode(&written, "written", LIMIT).expect("decode");
    assert_eq!((decoded.channels, decoded.alpha), (2, Some(false)));
    assert_eq!(decoded.samples, image.samples);

    let wrong = FloatTiff {
        samples: vec![1.0, 2.0],
        ..image
    };
    assert!(tiff_float::encode_to_vec(&wrong, false)
        .unwrap_err()
        .contains("sample count differs from the declared dimensions"));
}

// ------------------------------------------------------------- refusals --

fn refusal(options: Options, samples: &[f32]) -> String {
    let bytes = build(W, H, samples, &options);
    tiff_float::inspect(&bytes, "fixture")
        .err()
        .unwrap_or_else(|| panic!("expected a refusal"))
}

#[test]
fn every_refusal_names_the_element() {
    let one = values(1);
    assert!(refusal(
        Options {
            sample_format: 1,
            ..Options::default()
        },
        &one
    )
    .contains("SampleFormat must be 3 (IEEE floating point)"));

    assert!(refusal(
        Options {
            bits: 64,
            ..Options::default()
        },
        &one
    )
    .contains("64-bit float samples are unavailable"));

    assert!(refusal(
        Options {
            planar: 2,
            channels: 3,
            ..Options::default()
        },
        &values(3)
    )
    .contains("planar TIFF is unavailable"));

    assert!(refusal(
        Options {
            second_page: true,
            ..Options::default()
        },
        &one
    )
    .contains("multipage TIFF is unavailable"));

    assert!(refusal(
        Options {
            channels: 4,
            extra_samples: Some(vec![0]),
            ..Options::default()
        },
        &values(4)
    )
    .contains("ExtraSamples must be absent or declare one"));

    // Not a TIFF at all.
    assert!(tiff_float::inspect(b"\x89PNG\r\n\x1a\nnope", "fixture")
        .unwrap_err()
        .contains("input is not a TIFF"));
}

/// f32 RGB through the `tiff` crate's own encoder, classic or BigTIFF,
/// strips and Deflate; `planar` adds a PlanarConfiguration = 2 tag.
fn crate_written(samples: &[f32], big: bool, planar: bool) -> Vec<u8> {
    use tiff::encoder::{colortype::RGB32Float, Compression, TiffEncoder};
    use tiff::tags::Tag;
    let mut out = std::io::Cursor::new(Vec::new());
    let deflate = Compression::Deflate(Default::default());
    if big {
        let mut encoder = TiffEncoder::new_big(&mut out)
            .unwrap()
            .with_compression(deflate);
        let mut image = encoder.new_image::<RGB32Float>(W, H).unwrap();
        if planar {
            image
                .encoder()
                .write_tag(Tag::PlanarConfiguration, 2u16)
                .unwrap();
        }
        image.write_data(samples).unwrap();
    } else {
        let mut encoder = TiffEncoder::new(&mut out)
            .unwrap()
            .with_compression(deflate);
        let image = encoder.new_image::<RGB32Float>(W, H).unwrap();
        image.write_data(samples).unwrap();
    }
    out.into_inner()
}

#[test]
fn a_bigtiff_decodes_bit_identical_to_its_classic_twin() {
    let samples = values(3);
    let big = crate_written(&samples, true, false);
    assert_eq!(&big[..4], b"II\x2b\0", "the fixture is a BigTIFF");
    let classic = crate_written(&samples, false, false);
    let from_big = tiff_float::decode(&big, "big", LIMIT).expect("decode BigTIFF");
    let from_classic = tiff_float::decode(&classic, "classic", LIMIT).expect("decode classic");
    let bits = |image: &FloatTiff| {
        image
            .samples
            .iter()
            .map(|v| v.to_bits())
            .collect::<Vec<_>>()
    };
    assert_eq!(bits(&from_big), bits(&from_classic));
    assert_eq!(from_big.samples, samples);
    assert_eq!(
        (from_big.width, from_big.height, from_big.channels),
        (W, H, 3)
    );
}

#[test]
fn a_planar_bigtiff_is_refused_by_name() {
    let big = crate_written(&values(3), true, true);
    assert!(tiff_float::inspect(&big, "fixture")
        .unwrap_err()
        .contains("planar TIFF is unavailable"));
}

#[test]
fn a_decode_past_the_callers_allowance_is_refused() {
    let bytes = build(W, H, &values(1), &Options::default());
    let plan = tiff_float::inspect(&bytes, "fixture").expect("inspect");
    assert!(plan.scratch_bytes > 0);
    assert!(
        tiff_float::decode(&bytes, "fixture", plan.scratch_bytes - 1)
            .unwrap_err()
            .contains("more working memory than the caller allowed")
    );
}
