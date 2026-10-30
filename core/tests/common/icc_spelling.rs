//! A SECOND SPELLING of sRGB: an ICC v2.1 matrix/TRC profile with the
//! built-in sRGB's colorants and its 1024-entry tone curve but other bytes —
//! the profile `color_tests.rs` builds at lines 965–976 to show that Convert
//! treats it as the same space while Assign sees different bytes. Pulled in
//! by `#[path]` (`color_intent_tests.rs`'s pattern), so it is not part of
//! `common/mod.rs`.
//!
//! The tag writers below REPEAT `color_tests.rs`'s (lines 52–135 and 302,
//! the master copies: `build_profile`, `tag_desc`, `tag_xyz`,
//! `tag_curv_table`, `srgb_decode`, `iec_trc_table`), because that builder is
//! private to that test binary.
//!
//! [`adobe_rgb_1998`] is the one profile here that is NOT sRGB: a matrix
//! profile with published primaries other than the two built-ins, in both
//! adaptation shapes (`wtpt` only, and `chad`).

/// The D50 PCS illuminant as s15Fixed16 (0.9642 / 1.0 / 0.8249).
const D50_RAW: [i32; 3] = [0x0000_F6D6, 0x0001_0000, 0x0000_D32D];

fn tag_desc(name: &str) -> Vec<u8> {
    let mut t = b"desc\0\0\0\0".to_vec();
    t.extend_from_slice(&((name.len() + 1) as u32).to_be_bytes());
    t.extend_from_slice(name.as_bytes());
    t.push(0);
    t.extend_from_slice(&[0; 4]);
    t.extend_from_slice(&[0; 4]);
    t.extend_from_slice(&[0; 2]);
    t.push(0);
    t.extend_from_slice(&[0; 67]);
    t
}

fn tag_xyz(v: [f64; 3]) -> Vec<u8> {
    let mut t = b"XYZ \0\0\0\0".to_vec();
    for c in v {
        t.extend_from_slice(&((c * 65536.0).round() as i32).to_be_bytes());
    }
    t
}

/// The IEC 61966-2.1 transfer function, encoded → linear.
fn srgb_decode(x: f64) -> f64 {
    if x <= 0.04045 {
        x / 12.92
    } else {
        ((x + 0.055) / 1.055).powf(2.4)
    }
}

fn tag_curv_table() -> Vec<u8> {
    let mut t = b"curv\0\0\0\0".to_vec();
    t.extend_from_slice(&1024u32.to_be_bytes());
    for i in 0..1024 {
        let v = (srgb_decode(f64::from(i) / 1023.0) * 65535.0).round() as u16;
        t.extend_from_slice(&v.to_be_bytes());
    }
    t
}

/// The profile's bytes: 128-byte header, tag count, 12-byte entries, then
/// the tag data padded to 4 bytes with identical blobs shared.
pub fn srgb_second_spelling() -> Vec<u8> {
    spelling(None)
}

/// [`srgb_second_spelling`] plus a `wtpt` tag holding `wtpt` and no `chad`
/// — the shape of macOS's `sRGB Profile.icc` (D50-adapted columns, the
/// unadapted D65 in `wtpt`).
#[allow(dead_code)]
pub fn srgb_with_media_white(wtpt: [f64; 3]) -> Vec<u8> {
    spelling(Some(wtpt))
}

fn spelling(wtpt: Option<[f64; 3]>) -> Vec<u8> {
    let curve = tag_curv_table();
    let mut tags: Vec<(&[u8; 4], Vec<u8>)> = vec![
        (b"desc", tag_desc("sRGB IEC61966-2.1")),
        (b"rXYZ", tag_xyz([0.436066, 0.222488, 0.013916])),
        (b"gXYZ", tag_xyz([0.385147, 0.716873, 0.097076])),
        (b"bXYZ", tag_xyz([0.143066, 0.060608, 0.714096])),
        (b"rTRC", curve.clone()),
        (b"gTRC", curve.clone()),
        (b"bTRC", curve),
    ];
    if let Some(white) = wtpt {
        tags.push((b"wtpt", tag_xyz(white)));
    }
    assemble(&tags)
}

/// `curv` with one u8Fixed8 entry, a gamma (`color_tests.rs`'s
/// `tag_curv_gamma`, the master copy).
fn tag_curv_gamma(raw: u16) -> Vec<u8> {
    let mut t = b"curv\0\0\0\0".to_vec();
    t.extend_from_slice(&1u32.to_be_bytes());
    t.extend_from_slice(&raw.to_be_bytes());
    t
}

/// `s15Fixed16ArrayType` holding a row-major 3×3: the body of a `chad` tag.
fn tag_sf32(m: [[f64; 3]; 3]) -> Vec<u8> {
    let mut t = b"sf32\0\0\0\0".to_vec();
    for c in m.iter().flatten() {
        t.extend_from_slice(&((c * 65536.0).round() as i32).to_be_bytes());
    }
    t
}

/// Adobe RGB (1998) as a matrix/TRC profile: a space that is neither sRGB nor
/// Display P3, whose primaries its specification publishes (red 0.64 0.33,
/// green 0.21 0.71, blue 0.15 0.06, D65 white 0.3127 0.3290).
///
/// The colorants are the D50-adapted columns `AdobeRGB1998.icc` stores and the
/// curve its gamma (0x0233 = 2.19921875). `with_chad` false is that file's
/// own shape: the unadapted D65 in `wtpt` and no `chad`. True is the v4 shape:
/// a D50 `wtpt` and a `chad` holding the published Bradford D65 → D50 matrix
/// (Lindbloom's table), which is not symmetric, so a transposed read of it
/// lands on other primaries.
#[allow(dead_code)]
pub fn adobe_rgb_1998(with_chad: bool) -> Vec<u8> {
    let curve = tag_curv_gamma(0x0233);
    let mut tags: Vec<(&[u8; 4], Vec<u8>)> = vec![
        (b"desc", tag_desc("Adobe RGB (1998)")),
        (b"rXYZ", tag_xyz([0.60974, 0.31111, 0.01947])),
        (b"gXYZ", tag_xyz([0.20528, 0.62567, 0.06087])),
        (b"bXYZ", tag_xyz([0.14919, 0.06322, 0.74457])),
        (b"rTRC", curve.clone()),
        (b"gTRC", curve.clone()),
        (b"bTRC", curve),
    ];
    if with_chad {
        tags.push((b"wtpt", tag_xyz([0.9642, 1.0, 0.8249])));
        tags.push((
            b"chad",
            tag_sf32([
                [1.047_811_2, 0.022_886_6, -0.050_127_0],
                [0.029_542_4, 0.990_484_4, -0.017_049_1],
                [-0.009_234_5, 0.015_043_6, 0.752_131_6],
            ]),
        ));
    } else {
        tags.push((b"wtpt", tag_xyz([0.95045, 1.0, 1.08905])));
    }
    assemble(&tags)
}

/// The profile's bytes from its tags: 128-byte header, tag count, 12-byte
/// entries, then the tag data padded to 4 bytes with identical blobs shared.
fn assemble(tags: &[(&[u8; 4], Vec<u8>)]) -> Vec<u8> {
    let payload_start = 132 + 12 * tags.len();
    let mut table = Vec::new();
    let mut payload: Vec<u8> = Vec::new();
    let mut placed: Vec<(Vec<u8>, u32)> = Vec::new();
    for (sig, data) in tags {
        let offset = match placed.iter().find(|(seen, _)| seen == data) {
            Some((_, off)) => *off,
            None => {
                let off = (payload_start + payload.len()) as u32;
                payload.extend_from_slice(data);
                while !payload.len().is_multiple_of(4) {
                    payload.push(0);
                }
                placed.push((data.clone(), off));
                off
            }
        };
        table.extend_from_slice(sig.as_slice());
        table.extend_from_slice(&offset.to_be_bytes());
        table.extend_from_slice(&(data.len() as u32).to_be_bytes());
    }
    let total = (payload_start + payload.len()) as u32;
    let mut out = vec![0u8; 128];
    out[0..4].copy_from_slice(&total.to_be_bytes());
    out[8..12].copy_from_slice(&[0x02, 0x10, 0x00, 0x00]);
    out[12..16].copy_from_slice(b"mntr");
    out[16..20].copy_from_slice(b"RGB ");
    out[20..24].copy_from_slice(b"XYZ ");
    out[36..40].copy_from_slice(b"acsp");
    for (i, v) in D50_RAW.iter().enumerate() {
        out[68 + 4 * i..72 + 4 * i].copy_from_slice(&v.to_be_bytes());
    }
    out.extend_from_slice(&(tags.len() as u32).to_be_bytes());
    out.extend_from_slice(&table);
    out.extend_from_slice(&payload);
    out
}
