//! Hand-assembled gray files and a Gray ICC profile for #547's Grayscale
//! tests (`gray_document`, `gray_files`): PNGs written chunk by chunk from
//! the PNG specification, little-endian classic TIFFs from TIFF 6.0 and a v2
//! GRAY display profile from ICC.1:2001-04 — none through a decoder or
//! writer in this crate.

/// A PNG chunk: length, type, data, CRC over type and data.
pub fn chunk(kind: &[u8; 4], data: &[u8]) -> Vec<u8> {
    let mut out = (data.len() as u32).to_be_bytes().to_vec();
    out.extend_from_slice(kind);
    out.extend_from_slice(data);
    let mut crc = crc32fast::Hasher::new();
    crc.update(kind);
    crc.update(data);
    out.extend_from_slice(&crc.finalize().to_be_bytes());
    out
}

/// A non-interlaced PNG of `samples` (`channels` a pixel: 1 gray, 2 gray +
/// alpha, 3 RGB) at bit depth 8 or 16, colour type `colour`, with `extra`
/// chunks between IHDR and IDAT. Rows use filter 0. For bit depths below 8
/// (`depth` 1 or 4) the IDAT is a zero image of the right size.
pub fn png(
    w: u32,
    h: u32,
    depth: u8,
    colour: u8,
    channels: usize,
    samples: &[u16],
    extra: &[Vec<u8>],
) -> Vec<u8> {
    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&w.to_be_bytes());
    ihdr.extend_from_slice(&h.to_be_bytes());
    ihdr.extend_from_slice(&[depth, colour, 0, 0, 0]);
    let mut raw = Vec::new();
    if depth < 8 {
        let row = (w as usize * usize::from(depth)).div_ceil(8);
        for _ in 0..h {
            raw.push(0);
            raw.extend(std::iter::repeat_n(0u8, row));
        }
    } else {
        for y in 0..h as usize {
            raw.push(0);
            for v in &samples[y * w as usize * channels..(y + 1) * w as usize * channels] {
                if depth == 16 {
                    raw.extend_from_slice(&v.to_be_bytes());
                } else {
                    raw.push(*v as u8);
                }
            }
        }
    }
    let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
    out.extend(chunk(b"IHDR", &ihdr));
    for c in extra {
        out.extend_from_slice(c);
    }
    out.extend(chunk(
        b"IDAT",
        &miniz_oxide::deflate::compress_to_vec_zlib(&raw, 6),
    ));
    out.extend(chunk(b"IEND", &[]));
    out
}

/// An `iCCP` chunk carrying `profile`.
pub fn iccp(profile: &[u8]) -> Vec<u8> {
    let mut data = b"ICC Profile\0\0".to_vec();
    data.extend(miniz_oxide::deflate::compress_to_vec_zlib(profile, 6));
    chunk(b"iCCP", &data)
}

/// An `sRGB` chunk (perceptual) and a bare `gAMA` chunk of 1/2.2.
pub fn srgb_chunk() -> Vec<u8> {
    chunk(b"sRGB", &[0])
}

pub fn gama_chunk() -> Vec<u8> {
    chunk(b"gAMA", &45455u32.to_be_bytes())
}

/// A v2.1 `mntr` GRAY profile with an XYZ PCS: `desc` (textDescriptionType
/// with an empty tail), `wtpt` D50 and a `kTRC` gamma `curv` (count 1,
/// u8Fixed8 `gamma`).
pub fn gray_icc(name: &str, gamma: f64) -> Vec<u8> {
    let mut desc = b"desc\0\0\0\0".to_vec();
    desc.extend_from_slice(&(name.len() as u32 + 1).to_be_bytes());
    desc.extend_from_slice(name.as_bytes());
    desc.push(0);
    desc.extend_from_slice(&[0; 4 + 4 + 2 + 1 + 67]);
    let mut wtpt = b"XYZ \0\0\0\0".to_vec();
    for v in [0x0000_F6D6i32, 0x0001_0000, 0x0000_D32D] {
        wtpt.extend_from_slice(&v.to_be_bytes());
    }
    let mut ktrc = b"curv\0\0\0\0".to_vec();
    ktrc.extend_from_slice(&1u32.to_be_bytes());
    ktrc.extend_from_slice(&((gamma * 256.0).round() as u16).to_be_bytes());
    let tags: [(&[u8; 4], Vec<u8>); 3] = [(b"desc", desc), (b"wtpt", wtpt), (b"kTRC", ktrc)];
    let mut table = Vec::new();
    let mut payload = Vec::new();
    let start = 128 + 4 + 12 * tags.len();
    for (sig, data) in &tags {
        table.extend_from_slice(*sig);
        table.extend_from_slice(&((start + payload.len()) as u32).to_be_bytes());
        table.extend_from_slice(&(data.len() as u32).to_be_bytes());
        payload.extend_from_slice(data);
        while payload.len() % 4 != 0 {
            payload.push(0);
        }
    }
    let total = (start + payload.len()) as u32;
    let mut out = total.to_be_bytes().to_vec();
    out.extend_from_slice(&[0; 4]);
    out.extend_from_slice(&[2, 0x10, 0, 0]);
    out.extend_from_slice(b"mntrGRAYXYZ ");
    out.extend_from_slice(&[0; 12]);
    out.extend_from_slice(b"acsp");
    out.extend_from_slice(&[0; 28]);
    for v in [0x0000_F6D6i32, 0x0001_0000, 0x0000_D32D] {
        out.extend_from_slice(&v.to_be_bytes());
    }
    out.extend_from_slice(&[0; 48]);
    assert_eq!(out.len(), 128);
    out.extend_from_slice(&(tags.len() as u32).to_be_bytes());
    out.extend_from_slice(&table);
    out.extend_from_slice(&payload);
    out
}

/// A little-endian classic TIFF of one uncompressed chunky strip:
/// `photometric` (0 WhiteIsZero, 1 BlackIsZero), `samples` a pixel at `bits`
/// (`data` already packed little-endian), `ExtraSamples` when `extra` is
/// given, `SampleFormat` when `format` is given, an optional ICC.
#[allow(clippy::too_many_arguments)]
pub fn gray_tiff(
    w: u32,
    h: u32,
    bits: u16,
    samples: u16,
    photometric: u16,
    extra: Option<u16>,
    format: Option<u16>,
    data: &[u8],
    icc: Option<&[u8]>,
) -> Vec<u8> {
    let mut body = data.to_vec();
    if body.len() % 2 == 1 {
        body.push(0);
    }
    let icc_at = icc.map(|icc| {
        let at = 8 + body.len() as u32;
        body.extend_from_slice(icc);
        if body.len() % 2 == 1 {
            body.push(0);
        }
        at
    });
    let short = |v: u16| -> [u8; 4] {
        let b = v.to_le_bytes();
        [b[0], b[1], 0, 0]
    };
    let pair = |v: u16| -> [u8; 4] {
        let b = v.to_le_bytes();
        [b[0], b[1], b[0], b[1]]
    };
    let mut entries: Vec<(u16, u16, u32, [u8; 4])> = vec![
        (256, 4, 1, w.to_le_bytes()),
        (257, 4, 1, h.to_le_bytes()),
        (
            258,
            3,
            u32::from(samples),
            if samples == 1 {
                short(bits)
            } else {
                pair(bits)
            },
        ),
        (259, 3, 1, short(1)),
        (262, 3, 1, short(photometric)),
        (273, 4, 1, 8u32.to_le_bytes()),
        (277, 3, 1, short(samples)),
        (278, 4, 1, h.to_le_bytes()),
        (279, 4, 1, (data.len() as u32).to_le_bytes()),
        (284, 3, 1, short(1)),
    ];
    if let Some(extra) = extra {
        entries.push((338, 3, 1, short(extra)));
    }
    if let Some(format) = format {
        entries.push((
            339,
            3,
            u32::from(samples),
            if samples == 1 {
                short(format)
            } else {
                pair(format)
            },
        ));
    }
    if let (Some(icc), Some(at)) = (icc, icc_at) {
        entries.push((34675, 7, icc.len() as u32, at.to_le_bytes()));
    }
    let mut out = vec![0x49, 0x49, 42, 0];
    out.extend_from_slice(&(8 + body.len() as u32).to_le_bytes());
    out.extend_from_slice(&body);
    out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
    for (tag, kind, count, value) in entries {
        out.extend_from_slice(&tag.to_le_bytes());
        out.extend_from_slice(&kind.to_le_bytes());
        out.extend_from_slice(&count.to_le_bytes());
        out.extend_from_slice(&value);
    }
    out.extend_from_slice(&0u32.to_le_bytes());
    out
}

/// IEC 61966-2-1, both ways, written from the standard.
pub fn srgb_decode(v: f64) -> f64 {
    if v <= 0.04045 {
        v / 12.92
    } else {
        ((v + 0.055) / 1.055).powf(2.4)
    }
}

pub fn srgb_encode(y: f64) -> f64 {
    if y <= 0.0031308 {
        12.92 * y
    } else {
        1.055 * y.powf(1.0 / 2.4) - 0.055
    }
}

/// sRGB's D50-adapted luminance row as the sRGB profile publishes it
/// (s15Fixed16 `rXYZ`/`gXYZ`/`bXYZ` Y values 0x38F5, 0xB787, 0x0F84).
pub const SRGB_Y_ROW: [f64; 3] = [
    0x38F5 as f64 / 65536.0,
    0xB787 as f64 / 65536.0,
    0x0F84 as f64 / 65536.0,
];
