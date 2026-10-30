//! A hand-assembled little-endian classic CMYK TIFF (`open_profile_tests`'
//! and `untagged_cmyk_working`'s fixtures): one uncompressed chunky strip,
//! written from the TIFF 6.0 specification, not from any decoder in this
//! crate.

/// A little-endian classic TIFF: Photometric 5 (separated), chunky, one
/// uncompressed strip of `samples_per_pixel` samples at `bits` (8 or 16) —
/// `data` holds them already packed — with an optional ICC (tag 34675) and,
/// past four samples, `ExtraSamples = [extra_samples]` (0 unspecified, 2
/// unassociated alpha).
pub fn cmyk_tiff(
    w: u32,
    h: u32,
    bits: u16,
    samples_per_pixel: u16,
    extra_samples: u16,
    data: &[u8],
    icc: Option<&[u8]>,
) -> Vec<u8> {
    let mut body = data.to_vec();
    let mut place = |bytes: &[u8]| -> u32 {
        if body.len() % 2 == 1 {
            body.push(0);
        }
        let at = 8 + body.len() as u32;
        body.extend_from_slice(bytes);
        at
    };
    let bits_at = place(
        &std::iter::repeat_n(bits.to_le_bytes(), usize::from(samples_per_pixel))
            .flatten()
            .collect::<Vec<u8>>(),
    );
    let icc_at = icc.map(&mut place);
    if body.len() % 2 == 1 {
        body.push(0);
    }
    let short = |v: u16| -> [u8; 4] {
        let b = v.to_le_bytes();
        [b[0], b[1], 0, 0]
    };
    let mut entries: Vec<(u16, u16, u32, [u8; 4])> = vec![
        (256, 4, 1, w.to_le_bytes()),
        (257, 4, 1, h.to_le_bytes()),
        (258, 3, u32::from(samples_per_pixel), bits_at.to_le_bytes()),
        (259, 3, 1, short(1)),
        (262, 3, 1, short(5)),
        (273, 4, 1, 8u32.to_le_bytes()),
        (277, 3, 1, short(samples_per_pixel)),
        (278, 4, 1, h.to_le_bytes()),
        (279, 4, 1, (data.len() as u32).to_le_bytes()),
        (284, 3, 1, short(1)),
    ];
    if samples_per_pixel > 4 {
        entries.push((338, 3, 1, short(extra_samples)));
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

/// A little-endian classic CMYK TIFF from `planes` (one per sample, values at
/// `bits`): chunky in one uncompressed strip, or planar (PlanarConfiguration
/// 2) with one strip per plane; `ExtraSamples = extra_samples` whenever there
/// are more than four planes (#547 phase 3).
pub fn cmyk_tiff_planes(
    w: u32,
    h: u32,
    bits: u16,
    planar: bool,
    extra_samples: &[u16],
    planes: &[Vec<u16>],
    icc: Option<&[u8]>,
) -> Vec<u8> {
    let samples = planes.len();
    let pixels = (w * h) as usize;
    let put = |out: &mut Vec<u8>, v: u16| {
        if bits == 8 {
            out.push(v as u8);
        } else {
            out.extend_from_slice(&v.to_le_bytes());
        }
    };
    let mut strips: Vec<Vec<u8>> = Vec::new();
    if planar {
        for plane in planes {
            let mut strip = Vec::new();
            for v in plane {
                put(&mut strip, *v);
            }
            strips.push(strip);
        }
    } else {
        let mut strip = Vec::new();
        for i in 0..pixels {
            for plane in planes {
                put(&mut strip, plane[i]);
            }
        }
        strips.push(strip);
    }
    let mut body: Vec<u8> = Vec::new();
    let mut place = |bytes: &[u8]| -> u32 {
        if body.len() % 2 == 1 {
            body.push(0);
        }
        let at = 8 + body.len() as u32;
        body.extend_from_slice(bytes);
        at
    };
    let strip_offsets: Vec<u32> = strips.iter().map(|s| place(s)).collect();
    let counts: Vec<u32> = strips.iter().map(|s| s.len() as u32).collect();
    let bits_at = place(
        &std::iter::repeat_n(bits.to_le_bytes(), samples)
            .flatten()
            .collect::<Vec<u8>>(),
    );
    let offsets_at = place(
        &strip_offsets
            .iter()
            .flat_map(|v| v.to_le_bytes())
            .collect::<Vec<u8>>(),
    );
    let counts_at = place(
        &counts
            .iter()
            .flat_map(|v| v.to_le_bytes())
            .collect::<Vec<u8>>(),
    );
    let extras_at = place(
        &extra_samples
            .iter()
            .flat_map(|v| v.to_le_bytes())
            .collect::<Vec<u8>>(),
    );
    let icc_at = icc.map(&mut place);
    if body.len() % 2 == 1 {
        body.push(0);
    }
    let short = |v: u16| -> [u8; 4] {
        let b = v.to_le_bytes();
        [b[0], b[1], 0, 0]
    };
    let one_or = |values: &[u32], at: u32| -> [u8; 4] {
        if values.len() == 1 {
            values[0].to_le_bytes()
        } else {
            at.to_le_bytes()
        }
    };
    let mut entries: Vec<(u16, u16, u32, [u8; 4])> = vec![
        (256, 4, 1, w.to_le_bytes()),
        (257, 4, 1, h.to_le_bytes()),
        (
            258,
            3,
            samples as u32,
            if samples <= 2 {
                short(bits)
            } else {
                bits_at.to_le_bytes()
            },
        ),
        (259, 3, 1, short(1)),
        (262, 3, 1, short(5)),
        (
            273,
            4,
            strips.len() as u32,
            one_or(&strip_offsets, offsets_at),
        ),
        (277, 3, 1, short(samples as u16)),
        (278, 4, 1, h.to_le_bytes()),
        (279, 4, strips.len() as u32, one_or(&counts, counts_at)),
        (284, 3, 1, short(if planar { 2 } else { 1 })),
    ];
    if !extra_samples.is_empty() {
        let value = match extra_samples {
            [one] => short(*one),
            [a, b] => {
                let (a, b) = (a.to_le_bytes(), b.to_le_bytes());
                [a[0], a[1], b[0], b[1]]
            }
            _ => extras_at.to_le_bytes(),
        };
        entries.push((338, 3, extra_samples.len() as u32, value));
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
