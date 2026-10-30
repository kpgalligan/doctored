//! The four ICC profiles this build WRITES: sRGB IEC61966-2.1, Display P3,
//! Adobe RGB (1998) and ProPhoto RGB, emitted as real, conformant ICC v2.1.0
//! matrix/TRC blobs so an exported file carries a profile any other
//! application can read; and (#1) the eight built-in Gray profiles
//! ([`gray_blob`]) and the RGB twin of any Gray profile ([`twin_blob`]).
//! `icc` parses; this module builds; `icc_transform` does the numerics. It
//! also builds [`linear_twin`], the gamma-1.0 twin of a document's own
//! profile that a 32-bit PSD embeds, as Photoshop's does, and its
//! inverse [`linear_twin_source`], which names the built-in a reopened twin
//! was made from, and their Gray pair [`gray_linear_twin`] /
//! [`gray_linear_twin_source`] for a 32-bit Gray PSD (#1 phase 3c).
//!
//! # Where the two wide profiles come from
//!
//! Adobe RGB (1998) and ProPhoto RGB are built HERE, from the published xy
//! primaries. No system or Adobe profile file is read. Both tone curves are
//! a `curv` with a count of 1, whose single u8Fixed8 entry is a gamma
//! exponent. `curv` is a v2 type, so the "no `para` in a v2 profile" rule
//! below holds for all four.
//!
//! **Adobe RGB (1998)** carries `0x0233` = 563/256 = 2.19921875, the
//! value in Adobe's `AdobeRGB1998.icc` (2.2 itself is not representable).
//! It is kept because it is what other applications recognise as Adobe RGB.
//! Core Image, which encodes a Camera RAW master into this profile, uses 2.2
//! exactly: against the 563/256 this profile declares, the worst of the
//! interior lattice points of a real development is 1.65e-4 in linear sRGB
//! light, about 0.04 of an 8-bit level (`worst_interior_linear` of the
//! `oracle adobe_rgb` case, `python3 scripts/verify_v1_raw_output.py`). No
//! pixel conversion is added to close it.
//!
//! **ProPhoto RGB** carries `0x01CD` = 461/256 = 1.80078125, the value in
//! Kodak's `ProPhoto.icm` (1.8 itself is not representable either). It is
//! ProPhoto RGB at gamma 1.8, the pure power photo applications ship and
//! exchange under that name, NOT the ROMM RGB encoding of ISO 22028-2, whose
//! curve has a slope-16 linear toe below linear 1/512. The toe matters to
//! whoever encodes the pixels. Handed THIS profile, Core Image encodes a
//! Camera RAW master with the toe, which a pure power then decodes up to 1.6
//! 8-bit levels too dark in the deep shadows; handed a sampled table of the
//! ROMM curve, it encodes with an approximation of the table that is 7.4e-4
//! linear off at a midtone (both measured by the `oracle prophoto_rgb` case
//! of the drive named below, on those two forms). So the app does not render a
//! ProPhoto master into this profile's colour space at all: it renders into
//! the system's named ROMM RGB space, which Core Image encodes as a pure 1.8
//! power, and tags the result with this profile (`RawOutputSpace.cgSpace`).
//! Against the 461/256 this profile declares, the worst of the interior
//! lattice points of a real development is 1.94e-4 in linear sRGB light,
//! about 0.05 of an 8-bit level (`worst_interior_linear` of the
//! `oracle prophoto_rgb` case, `python3 scripts/verify_v1_raw_output.py`).
//! No pixel conversion is added to close it.
//!
//! ProPhoto's native white is the PCS D50 itself, so its columns are not
//! adapted and its `chad` is the identity; Adobe RGB's native white is D65,
//! Bradford-adapted to D50 like sRGB's and Display P3's.
//!
//! # Why v2.1.0 with a 1024-entry `curv`, and not a `para` curve
//!
//! `parametricCurveType` (`para`) is an ICC **v4** tag type and is not
//! defined in v2, so a v2 header carrying one is a hybrid strict validators
//! (Java's `ICC_Profile`, some Windows ICM paths) may reject. Writing a
//! `curv` table removes the question, and it costs nothing:
//!
//! * **The shape is Apple's own.** `/System/Library/ColorSync/Profiles/sRGB
//!   Profile.icc` is version `02 10 00 00` with a `desc` and a 1024-entry
//!   `curv` — exactly what this module writes.
//! * **The bytes are Apple's own.** The generation rule below,
//!   `tab[i] = round(srgb_transfer(i / 1023) * 65535)`, reproduces that
//!   shipped file's 1024-entry rTRC table byte-for-byte, all 1024 entries
//!   (`color_tests::builtin_trc_is_the_iec_curve` recomputes it here).
//! * **The numbers do not move.** Every conversion anchor is identical
//!   whether the built-in TRC is the analytic sRGB `para` curve or this
//!   table.
//!
//! Cost: one 2060-byte `curv` blob, shared by all three TRC tags, so the
//! sRGB and Display P3 built-ins are ~2.5 KB rather than ~540 bytes.
//! Irrelevant next to a JPEG. Adobe RGB's and ProPhoto's three TRC tags
//! share one 14-byte gamma `curv` each, the same way.
//!
//! # Determinism
//!
//! The creation date in the header is a CONSTANT, never `now()`: a save must
//! produce the same bytes every run, or the "resaving a document reproduces
//! the file byte-for-byte" property the DRDC tests rely on would break the
//! moment a profile is embedded.
//!
//! Both the blob and the [`MatrixTrc`] model come from the SAME constants
//! below, so `parse(blob(w))` and `model(w)` describe one colour space and
//! `IccProfile::srgb()` never has to unwrap a parse.

use std::sync::OnceLock;

use crate::gray_working::GrayWorkingSpace;
use crate::icc::{IccBytes, IccProfile};
use crate::icc_transform::{Curve, MatrixTrc};

/// Which of the four profiles this build can write.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Builtin {
    Srgb,
    DisplayP3,
    AdobeRgb,
    ProPhotoRgb,
}

impl Builtin {
    /// The `desc` text, which is also the name the UI shows.
    pub(crate) fn display_name(self) -> &'static str {
        match self {
            Builtin::Srgb => "sRGB IEC61966-2.1",
            Builtin::DisplayP3 => "Display P3",
            Builtin::AdobeRgb => "Adobe RGB (1998)",
            Builtin::ProPhotoRgb => "ProPhoto RGB",
        }
    }

    /// Maps a raw `DrBuiltinProfile` value coming across the FFI. Mapped,
    /// never transmuted: an out-of-range discriminant is a caller error, and
    /// materializing an enum from one would be undefined behaviour.
    pub(crate) fn from_c(value: i32) -> Option<Self> {
        match value {
            0 => Some(Builtin::Srgb),
            1 => Some(Builtin::DisplayP3),
            2 => Some(Builtin::AdobeRgb),
            3 => Some(Builtin::ProPhotoRgb),
            _ => None,
        }
    }

    /// The `curv` tag's entries: the 1024-entry IEC table for sRGB and
    /// Display P3, the one u8Fixed8 gamma exponent for Adobe RGB and for
    /// ProPhoto (a count of 1 is the gamma form). The module doc says where
    /// each comes from.
    fn curv_entries(self) -> Vec<u16> {
        match self {
            Builtin::Srgb | Builtin::DisplayP3 => trc_table(),
            // 563/256 = 2.19921875, as in Adobe's `AdobeRGB1998.icc`.
            Builtin::AdobeRgb => vec![0x0233],
            // 461/256 = 1.80078125, as in Kodak's `ProPhoto.icm`.
            Builtin::ProPhotoRgb => vec![0x01CD],
        }
    }

    /// The `chad` tag's nine values: the Bradford D65→D50 adaptation for the
    /// three built-ins whose native white is D65, the identity for ProPhoto,
    /// whose native white is the PCS D50 and whose columns are therefore not
    /// adapted at all.
    fn chad(self) -> [i32; 9] {
        match self {
            Builtin::Srgb | Builtin::DisplayP3 | Builtin::AdobeRgb => CHAD,
            Builtin::ProPhotoRgb => [0x0001_0000, 0, 0, 0, 0x0001_0000, 0, 0, 0, 0x0001_0000],
        }
    }

    /// The three D50 matrix columns as raw s15Fixed16 values, in r, g, b
    /// order. Recomputed from the published xy primaries — through the
    /// Bradford D65→D50 adaptation for every built-in but ProPhoto, whose
    /// white is D50 already — by
    /// `color_tests::builtin_matrix_columns_match_the_published_primaries`
    /// and `builtin_wide_profiles`.
    fn columns(self) -> [[i32; 3]; 3] {
        match self {
            // 0.436035  0.222485  0.013916
            // 0.385117  0.716888  0.097061
            // 0.143036  0.060608  0.713928
            Builtin::Srgb => [
                [0x0000_6FA0, 0x0000_38F5, 0x0000_0390],
                [0x0000_6297, 0x0000_B787, 0x0000_18D9],
                [0x0000_249F, 0x0000_0F84, 0x0000_B6C3],
            ],
            // 0.515121  0.241196 -0.001053
            // 0.291977  0.692245  0.041885
            // 0.157104  0.066574  0.784073
            //
            // Byte-identical to Apple's `Display P3.icc` at offsets 400, 420
            // and 440.
            Builtin::DisplayP3 => [
                [0x0000_83DF, 0x0000_3DBF, 0xFFFF_FFBBu32 as i32],
                [0x0000_4ABF, 0x0000_B137, 0x0000_0AB9],
                [0x0000_2838, 0x0000_110B, 0x0000_C8B9],
            ],
            // 0.609741  0.311111  0.019470
            // 0.205276  0.625671  0.060867
            // 0.149185  0.063217  0.744568
            //
            // The values in Adobe's `AdobeRGB1998.icc`.
            Builtin::AdobeRgb => [
                [0x0000_9C18, 0x0000_4FA5, 0x0000_04FC],
                [0x0000_348D, 0x0000_A02C, 0x0000_0F95],
                [0x0000_2631, 0x0000_102F, 0x0000_BE9C],
            ],
            // 0.797668  0.288040  0.000000
            // 0.135193  0.711884  0.000000
            // 0.031342  0.000092  0.824905
            //
            // The values in Kodak's `ProPhoto.icm`.
            Builtin::ProPhotoRgb => [
                [0x0000_CC34, 0x0000_49BD, 0x0000_0000],
                [0x0000_229C, 0x0000_B63E, 0x0000_0000],
                [0x0000_0806, 0x0000_0006, 0x0000_D32D],
            ],
        }
    }
}

/// The D50 PCS white point, fixed by the ICC spec and identical in every
/// profile macOS ships: 0.9642, 1.0, 0.8249 as s15Fixed16.
const D50: [i32; 3] = [0x0000_F6D6, 0x0001_0000, 0x0000_D32D];

/// The Bradford chromatic adaptation from D65 to D50, row-major s15Fixed16 —
///
/// ```text
///  1.047886   0.022919  -0.050216
///  0.029582   0.990484  -0.017079
/// -0.009252   0.015073   0.751678
/// ```
///
/// The `chad` tag is ADVISORY to conversion: this crate's model reader
/// ignores it (see `MatrixTrc`'s doc for why a matrix/TRC profile's PCS
/// white is always D50), and it is written because a v2 display profile
/// whose native white is not D50 is expected to record the adaptation it
/// applied. Three of these entries differ from Apple's by one raw unit
/// (Apple has `FFFFF326`, `FFFFFBA2`, `FFFFFDA3`): the Bradford product is
/// rounded here rather than copied. Its one reader is the 32-bit Photo
/// Filter's native basis (`adjust_float`), where one raw unit is 1.5e-5.
const CHAD: [i32; 9] = [
    0x0001_0C42,
    0x0000_05DE,
    0xFFFF_F325u32 as i32,
    0x0000_0793,
    0x0000_FD90,
    0xFFFF_FBA1u32 as i32,
    0xFFFF_FDA2u32 as i32,
    0x0000_03DC,
    0x0000_C06E,
];

/// Entries in the `curv` TRC table. 1024 is what Apple's own sRGB profile
/// uses, and the table below reproduces that file exactly.
const TRC_ENTRIES: usize = 1024;

/// The IEC 61966-2.1 electro-optical transfer function — the sRGB curve,
/// which Display P3 shares (P3 differs only in its red and green
/// primaries). Maps an encoded value to a linear one.
fn srgb_transfer(x: f64) -> f64 {
    if x <= 0.04045 {
        x / 12.92
    } else {
        ((x + 0.055) / 1.055).powf(2.4)
    }
}

/// The 1024 16-bit TRC samples sRGB and Display P3 share.
///
/// Anchors a reviewer can spot-check against Apple's shipped
/// `sRGB Profile.icc`: `[0] = 0x0000`, `[1] = 0x0005`, `[2] = 0x000A`,
/// `[256] = 0x0D0D`, `[512] = 0x36E9`, `[1023] = 0xFFFF`.
fn trc_table() -> Vec<u16> {
    (0..TRC_ENTRIES)
        .map(|i| {
            let x = i as f64 / (TRC_ENTRIES - 1) as f64;
            (srgb_transfer(x) * 65535.0).round() as u16
        })
        .collect()
}

/// The profile's canonical bytes, built once per built-in and byte-identical
/// on every call and every run.
pub(crate) fn blob(which: Builtin) -> &'static [u8] {
    static SRGB: OnceLock<Vec<u8>> = OnceLock::new();
    static P3: OnceLock<Vec<u8>> = OnceLock::new();
    static ADOBE: OnceLock<Vec<u8>> = OnceLock::new();
    static PROPHOTO: OnceLock<Vec<u8>> = OnceLock::new();
    let slot = match which {
        Builtin::Srgb => &SRGB,
        Builtin::DisplayP3 => &P3,
        Builtin::AdobeRgb => &ADOBE,
        Builtin::ProPhotoRgb => &PROPHOTO,
    };
    slot.get_or_init(|| {
        let curv = curv_tag(&which.curv_entries());
        build_v2_matrix_profile(which.display_name(), which.columns(), curv, which.chad())
    })
}

/// The transformable model, built from the SAME constants as [`blob`], so
/// there is one source of truth for what each built-in means.
pub(crate) fn model(which: Builtin) -> MatrixTrc {
    let cols = which.columns();
    let to_pcs = [fixed_col(cols[0]), fixed_col(cols[1]), fixed_col(cols[2])];
    let entries = which.curv_entries();
    let curve = match entries.as_slice() {
        // What `icc`'s reader makes of a count-1 `curv`: `raw / 256`.
        [raw] => Curve::Gamma(f64::from(*raw) / 256.0),
        _ => Curve::Sampled(entries),
    };
    MatrixTrc::new(to_pcs, [curve.clone(), curve.clone(), curve])
        .expect("the built-in primaries are an invertible basis")
}

fn fixed_col(raw: [i32; 3]) -> [f64; 3] {
    [
        f64::from(raw[0]) / 65536.0,
        f64::from(raw[1]) / 65536.0,
        f64::from(raw[2]) / 65536.0,
    ]
}

/// Entries in a built-in Gray profile's power-curve `kTRC` table.
const GRAY_TRC_ENTRIES: usize = 4096;

/// The built-in Gray profile of `space` (#1's Grayscale mode): a v2.1.0
/// `mntr` GRAY profile with an XYZ PCS whose tags are `desc` (the space's
/// `display_name`), `cprt`, `wtpt` (D50) and `kTRC`, a 4096-entry `curv` of
/// `space.linear` — except sGray, whose `kTRC` is sRGB's own 1024-entry IEC
/// table ([`trc_table`]). Built once per space and byte-identical every run.
pub(crate) fn gray_blob(space: GrayWorkingSpace) -> &'static [u8] {
    static BLOBS: OnceLock<Vec<Vec<u8>>> = OnceLock::new();
    let blobs = BLOBS.get_or_init(|| {
        GrayWorkingSpace::ALL
            .iter()
            .map(|&space| {
                let entries = if space == GrayWorkingSpace::SGray {
                    trc_table()
                } else {
                    (0..GRAY_TRC_ENTRIES)
                        .map(|i| {
                            let v = i as f64 / (GRAY_TRC_ENTRIES - 1) as f64;
                            (space.linear(v) * 65535.0).round() as u16
                        })
                        .collect()
                };
                let tags: [(&[u8; 4], Vec<u8>); 4] = [
                    (b"desc", desc_tag(space.display_name())),
                    (b"cprt", text_tag("Public Domain")),
                    (b"wtpt", xyz_tag(D50)),
                    (b"kTRC", curv_tag(&entries)),
                ];
                assemble(&tags, [0x02, 0x10, 0x00, 0x00], b"GRAY", b"XYZ ")
            })
            .collect()
    });
    let index = GrayWorkingSpace::ALL
        .iter()
        .position(|&s| s == space)
        .unwrap_or(0);
    &blobs[index]
}

/// The RGB TWIN of a Gray profile (#1): the RGB matrix/TRC profile a Gray
/// document's storage and view are in — sRGB's D50 columns and `chad`, the
/// Gray profile's `desc` and `kTRC` tag bytes verbatim as its `desc` and its
/// three TRCs, and the Gray profile's header version — so an R = G = B
/// pixel in it is neutral and its luminance is the Gray profile's
/// `kTRC(code)`. `Err` when `gray_icc` is not a profile or has no `kTRC`.
pub(crate) fn twin_blob(gray_icc: &[u8]) -> Result<Vec<u8>, String> {
    let icc = IccBytes::new(gray_icc).ok_or("the Gray profile is not an ICC profile")?;
    if icc.data_space() != b"GRAY" {
        return Err("the Gray profile is not a GRAY profile".into());
    }
    let trc = icc
        .tag(b"kTRC")
        .ok_or("the Gray profile has no kTRC tone curve")?
        .to_vec();
    let desc = match icc.tag(b"desc") {
        Some(tag) => tag.to_vec(),
        None => desc_tag(&icc.name()),
    };
    let mut version = [0u8; 4];
    version.copy_from_slice(&gray_icc[8..12]);
    Ok(matrix_profile(
        desc,
        Builtin::Srgb.columns(),
        trc,
        Builtin::Srgb.chad(),
        version,
    ))
}

/// The Lab TWIN (#1's Lab Color mode): the profile a Lab document's storage
/// and view are in. A v4.3 `mntr` profile whose data colour space is `RGB `
/// and whose PCS is `Lab `, with tags `desc` (mluc "Lab Color"), `cprt`
/// (mluc, the built-ins' text), `wtpt` D50, and `A2B0` (`mAB `) and `B2A0`
/// (`mBA `), each 3 in / 3 out with three `curv` count-0 (identity) B curves
/// and every other element offset 0. A device value IS therefore the ICC v4
/// Lab encoding (L\* = 100·v, a\*/b\* = 255·v − 128; an 8-bit code c is the
/// 16-bit c×257, and a\* = b\* = 0 is 0x8080), so the RGB compositor's linear
/// mix of R, G, B mixes L\*, a\*, b\*, while `IccProfile::parse` keeps the
/// profile as `RgbLut`. The host draws the core's Display P3 view of it
/// (`lab_convert::lab_display`), never the twin itself. Built once,
/// byte-identical every run.
pub(crate) fn lab_twin_blob() -> &'static [u8] {
    static BLOB: OnceLock<Vec<u8>> = OnceLock::new();
    BLOB.get_or_init(|| {
        // `lutAtoBType` / `lutBtoAType` (ICC.1:2010 10.10/10.11): signature,
        // reserved, in, out, two pad bytes, then the B, matrix, M, CLUT and A
        // offsets from the tag's start; the B curves follow the 32-byte head.
        let lut = |signature: &[u8; 4]| {
            let mut t = Vec::with_capacity(32 + 36);
            t.extend_from_slice(signature);
            t.extend_from_slice(&[0; 4]);
            t.extend_from_slice(&[3, 3, 0, 0]);
            t.extend_from_slice(&32u32.to_be_bytes());
            t.extend_from_slice(&[0; 16]);
            for _ in 0..3 {
                t.extend_from_slice(&curv_tag(&[]));
            }
            t
        };
        let tags: [(&[u8; 4], Vec<u8>); 5] = [
            (b"desc", mluc_tag("Lab Color")),
            (b"cprt", mluc_tag("Public Domain")),
            (b"wtpt", xyz_tag(D50)),
            (b"A2B0", lut(b"mAB ")),
            (b"B2A0", lut(b"mBA ")),
        ];
        assemble(&tags, [0x04, 0x30, 0x00, 0x00], b"RGB ", b"Lab ")
    })
}

// ------------------------------------------------------------- assembly --

/// Assembles a complete ICC v2.1.0 RGB matrix/TRC display profile:
/// the 128-byte header, a 10-entry tag table (`desc`, `cprt`, `wtpt`,
/// `rXYZ`, `gXYZ`, `bXYZ`, `rTRC`, `gTRC`, `bTRC`, `chad`), then the tag
/// data, each blob padded to a 4-byte boundary (padding is NOT counted in a
/// tag's size) and IDENTICAL blobs shared — the three TRC tags all point at
/// one `curv` (2060 bytes for the IEC table, 14 for a gamma), exactly as
/// both Apple profiles do.
fn build_v2_matrix_profile(
    desc: &str,
    cols: [[i32; 3]; 3],
    curv: Vec<u8>,
    chad: [i32; 9],
) -> Vec<u8> {
    matrix_profile(desc_tag(desc), cols, curv, chad, [0x02, 0x10, 0x00, 0x00])
}

/// [`build_v2_matrix_profile`] with the `desc` tag's bytes and the header
/// version given: the one assembly the built-ins and [`twin_blob`] share.
fn matrix_profile(
    desc: Vec<u8>,
    cols: [[i32; 3]; 3],
    curv: Vec<u8>,
    chad: [i32; 9],
    version: [u8; 4],
) -> Vec<u8> {
    let tags: [(&[u8; 4], Vec<u8>); 10] = [
        (b"desc", desc),
        (b"cprt", text_tag("Public Domain")),
        (b"wtpt", xyz_tag(D50)),
        (b"rXYZ", xyz_tag(cols[0])),
        (b"gXYZ", xyz_tag(cols[1])),
        (b"bXYZ", xyz_tag(cols[2])),
        (b"rTRC", curv.clone()),
        (b"gTRC", curv.clone()),
        (b"bTRC", curv),
        (b"chad", sf32_tag(&chad)),
    ];
    assemble(&tags, version, b"RGB ", b"XYZ ")
}

/// The suffix Photoshop appends to a profile's name when it embeds the
/// profile's linear-light twin in a 32-bit file (take1's `ps32.psd`
/// carries "sRGB IEC61966-2.1 (Linear RGB Profile)").
pub(crate) const LINEAR_TWIN_SUFFIX: &str = " (Linear RGB Profile)";

/// The linear-light twin of a matrix/TRC `profile`, the profile Photoshop
/// embeds in a 32-bit document: the same colour space with every tone
/// curve a `curv` gamma of exactly 1.0 (`0x0100`), because a 32-bit file's
/// samples are linear light. Built the way Photoshop's own twin is, tag by
/// tag (take1's `ps32.psd`, 520 bytes): `cprt`, `desc` (the source's name plus
/// [`LINEAR_TWIN_SUFFIX`]), `rXYZ`, `gXYZ`, `bXYZ`, `wtpt`, and the three TRC
/// tags sharing one 14-byte gamma `curv`. Where it differs from
/// Photoshop's: the colorant and `wtpt` tags are the SOURCE's own bytes
/// (Photoshop recomputes them: against macOS sRGB's, its columns and white
/// differ by up to 12/65536, `bXYZ` Z); `cprt` is the source's `text`
/// copyright (else "Public Domain") rather than Adobe's; a source `chad` is
/// carried after the TRC tags (take1's source had none); and the header is
/// this module's deterministic v2.1.0 one, not Photoshop's v2.4.0 with an
/// `ADBE` creator and the save's date.
///
/// A profile whose three TRC tags are already the identity (`curv` with a
/// count of 0, or 1 and `0x0100`) — Photoshop's own twin, read back — is
/// returned unchanged, and so is a profile with no matrix/TRC model (a float
/// document refuses one before any save gets here).
pub(crate) fn linear_twin(profile: &IccProfile) -> Vec<u8> {
    let bytes = &profile.bytes()[..];
    let view = match IccBytes::new(bytes) {
        Some(view) if profile.model().is_some() => view,
        _ => return bytes.to_vec(),
    };
    let identity = |sig: &[u8; 4]| {
        view.tag(sig).is_some_and(|data| {
            data.get(..4) == Some(b"curv")
                && match crate::icc::be_u32(data, 8) {
                    Some(0) => true,
                    Some(1) => crate::icc::be_u16(data, 12) == Some(0x0100),
                    _ => false,
                }
        })
    };
    if [b"rTRC", b"gTRC", b"bTRC"].into_iter().all(identity) {
        return bytes.to_vec();
    }
    let copied = |sig: &[u8; 4]| view.tag(sig).map(<[u8]>::to_vec);
    let (Some(red), Some(green), Some(blue)) = (copied(b"rXYZ"), copied(b"gXYZ"), copied(b"bXYZ"))
    else {
        return bytes.to_vec();
    };
    let cprt = view
        .tag(b"cprt")
        .filter(|data| data.get(..4) == Some(b"text"))
        .map(<[u8]>::to_vec)
        .unwrap_or_else(|| text_tag("Public Domain"));
    // `desc_tag` writes 7-bit ASCII; anything else in a name becomes '?'.
    let name: String = format!("{}{LINEAR_TWIN_SUFFIX}", profile.name())
        .chars()
        .map(|c| {
            if c.is_ascii() && !c.is_ascii_control() {
                c
            } else {
                '?'
            }
        })
        .collect();
    let gamma = curv_tag(&[0x0100]);
    let mut tags: Vec<(&[u8; 4], Vec<u8>)> = vec![
        (b"cprt", cprt),
        (b"desc", desc_tag(&name)),
        (b"rXYZ", red),
        (b"gXYZ", green),
        (b"bXYZ", blue),
        (b"wtpt", copied(b"wtpt").unwrap_or_else(|| xyz_tag(D50))),
        (b"rTRC", gamma.clone()),
        (b"gTRC", gamma.clone()),
        (b"bTRC", gamma),
    ];
    if let Some(chad) = copied(b"chad") {
        tags.push((b"chad", chad));
    }
    assemble(&tags, [0x02, 0x10, 0x00, 0x00], b"RGB ", b"XYZ ")
}

/// The built-in whose linear twin `profile` is, or None — the inverse of
/// [`linear_twin`] for the four built-ins, so a reopened 32-bit PSD can take
/// back the gamma profile its twin was made from.
///
/// Both the NAME and the SPACE must match, and either alone is not enough.
/// The space alone (the source's matrix with three identity curves) would
/// also claim a profile someone authored as a genuinely linear working space
/// under its own name, and relabelling that as, say, sRGB would change what
/// an 8-bit export of it encodes. The name alone would claim a profile that
/// only says it is a twin: a hand-edited or foreign file whose colorants are
/// another space's, which the built-in would then misdescribe. Requiring the
/// source's name plus [`LINEAR_TWIN_SUFFIX`] AND a model within
/// `MatrixTrc::approx_eq` of the built-in's matrix over identity curves
/// accepts exactly the twins this module and Photoshop write: Doctored's
/// twin of a built-in, its twin of macOS's sRGB (the same name, colorants
/// about 2.6e-4 apart combined) and Photoshop's recomputed twin (columns up to
/// 12/65536 off). Anything else returns `None`; no bytes can panic here.
pub(crate) fn linear_twin_source(profile: &IccProfile) -> Option<Builtin> {
    let model = profile.model()?;
    [
        Builtin::Srgb,
        Builtin::DisplayP3,
        Builtin::AdobeRgb,
        Builtin::ProPhotoRgb,
    ]
    .into_iter()
    .find(|&which| {
        let name = format!("{}{LINEAR_TWIN_SUFFIX}", which.display_name());
        if profile.name() != name {
            return false;
        }
        let cols = which.columns();
        let to_pcs = [fixed_col(cols[0]), fixed_col(cols[1]), fixed_col(cols[2])];
        let identity = [Curve::Identity, Curve::Identity, Curve::Identity];
        MatrixTrc::new(to_pcs, identity).is_some_and(|linear| model.approx_eq(&linear))
    })
}

/// The suffix of a 32-bit Gray PSD's embedded profile (#1 phase 3c):
/// Photoshop's own is "Linear Grayscale Profile" (take2's `layers32.psd`),
/// and the twin of a named Gray profile carries that name plus this.
pub(crate) const GRAY_LINEAR_TWIN_SUFFIX: &str = " (Linear Grayscale Profile)";

/// True for a `curv` tag that is the identity: a count of 0, or a count of 1
/// whose u8Fixed8 gamma is exactly 1.0 (`0x0100`).
fn identity_curv(data: &[u8]) -> bool {
    data.get(..4) == Some(b"curv")
        && match crate::icc::be_u32(data, 8) {
            Some(0) => true,
            Some(1) => crate::icc::be_u16(data, 12) == Some(0x0100),
            _ => false,
        }
}

/// The linear-light twin of a Gray profile, the profile a 32-bit Gray PSD
/// embeds (#1 phase 3c), as [`linear_twin`] is for RGB: a 32-bit file's
/// samples are linear `Y`, so its profile's `kTRC` is a `curv` gamma of
/// exactly 1.0. A v2.1.0 `mntr` GRAY/XYZ profile with the tags `desc` (the
/// source's name plus [`GRAY_LINEAR_TWIN_SUFFIX`], non-ASCII as '?'), `cprt`
/// (the source's `text` copyright, else "Public Domain"), `wtpt` (the
/// source's, else D50) and `kTRC` (`curv` count 1, `0x0100`). A profile whose
/// `kTRC` is already the identity (Photoshop's 384-byte "Linear Grayscale
/// Profile") is returned unchanged, and so are bytes that are not a Gray
/// profile with a `kTRC`.
pub(crate) fn gray_linear_twin(gray_icc: &[u8]) -> Vec<u8> {
    let view = match IccBytes::new(gray_icc) {
        Some(view) if view.data_space() == b"GRAY" => view,
        _ => return gray_icc.to_vec(),
    };
    match view.tag(b"kTRC") {
        Some(trc) if !identity_curv(trc) => {}
        _ => return gray_icc.to_vec(),
    }
    let cprt = view
        .tag(b"cprt")
        .filter(|data| data.get(..4) == Some(b"text"))
        .map(<[u8]>::to_vec)
        .unwrap_or_else(|| text_tag("Public Domain"));
    // `desc_tag` writes 7-bit ASCII; anything else in a name becomes '?'.
    let name: String = format!("{}{GRAY_LINEAR_TWIN_SUFFIX}", view.name())
        .chars()
        .map(|c| {
            if c.is_ascii() && !c.is_ascii_control() {
                c
            } else {
                '?'
            }
        })
        .collect();
    let wtpt = view
        .tag(b"wtpt")
        .map(<[u8]>::to_vec)
        .unwrap_or_else(|| xyz_tag(D50));
    let tags: [(&[u8; 4], Vec<u8>); 4] = [
        (b"desc", desc_tag(&name)),
        (b"cprt", cprt),
        (b"wtpt", wtpt),
        (b"kTRC", curv_tag(&[0x0100])),
    ];
    assemble(&tags, [0x02, 0x10, 0x00, 0x00], b"GRAY", b"XYZ ")
}

/// The built-in Gray working space whose linear twin `gray_icc` is, or None
/// — the inverse of [`gray_linear_twin`] for the eight built-ins, so a
/// reopened 32-bit Gray PSD of ours takes back the profile its twin was made
/// from. As for [`linear_twin_source`], the NAME (the space's
/// `display_name` plus [`GRAY_LINEAR_TWIN_SUFFIX`]) AND the curve (an
/// identity `kTRC`) must both match: Photoshop's own "Linear Grayscale
/// Profile" names no built-in and is kept verbatim. No bytes can panic here.
pub(crate) fn gray_linear_twin_source(gray_icc: &[u8]) -> Option<GrayWorkingSpace> {
    let view = IccBytes::new(gray_icc)?;
    if view.data_space() != b"GRAY" || !view.tag(b"kTRC").is_some_and(identity_curv) {
        return None;
    }
    let name = view.name();
    GrayWorkingSpace::ALL
        .into_iter()
        .find(|space| name == format!("{}{GRAY_LINEAR_TWIN_SUFFIX}", space.display_name()))
}

/// The header, tag table and tag data of a `mntr` profile over `space` with
/// the `pcs` connection space (`XYZ ` for every built-in but the Lab twin):
/// each blob padded to a 4-byte boundary (padding is NOT counted in a tag's
/// size) and IDENTICAL blobs shared.
fn assemble(
    tags: &[(&[u8; 4], Vec<u8>)],
    version: [u8; 4],
    space: &[u8; 4],
    pcs: &[u8; 4],
) -> Vec<u8> {
    let table_start = 132;
    let payload_start = table_start + 12 * tags.len();
    let mut table = Vec::with_capacity(12 * tags.len());
    let mut payload: Vec<u8> = Vec::new();
    // (blob, offset) pairs, so an identical blob reuses the first offset.
    let mut placed: Vec<(&[u8], u32)> = Vec::new();
    for (sig, data) in tags {
        let offset = match placed.iter().find(|(seen, _)| *seen == data.as_slice()) {
            Some((_, off)) => *off,
            None => {
                let off = (payload_start + payload.len()) as u32;
                payload.extend_from_slice(data);
                // Pad to the next 4-byte boundary; the padding is not part
                // of the tag's declared size.
                while !payload.len().is_multiple_of(4) {
                    payload.push(0);
                }
                placed.push((data.as_slice(), off));
                off
            }
        };
        table.extend_from_slice(sig.as_slice());
        table.extend_from_slice(&offset.to_be_bytes());
        table.extend_from_slice(&(data.len() as u32).to_be_bytes());
    }

    let total = (payload_start + payload.len()) as u32;
    let mut out = Vec::with_capacity(total as usize);
    // --- the 128-byte header ---
    out.extend_from_slice(&total.to_be_bytes()); //   0 profile size
    out.extend_from_slice(&[0; 4]); //                4 preferred CMM: none
    out.extend_from_slice(&version); //              8 version (2.1.0 for the built-ins)
    out.extend_from_slice(b"mntr"); //               12 display device class
    out.extend_from_slice(space); //                 16 data colour space
    out.extend_from_slice(pcs); //                   20 PCS
                                //               24 creation date, a CONSTANT: 2026-01-01T00:00:00Z
    for field in [2026u16, 1, 1, 0, 0, 0] {
        out.extend_from_slice(&field.to_be_bytes());
    }
    out.extend_from_slice(b"acsp"); //               36 magic
    out.extend_from_slice(&[0; 4]); //               40 primary platform: none
    out.extend_from_slice(&[0; 4]); //               44 profile flags
    out.extend_from_slice(&[0; 4]); //               48 device manufacturer
    out.extend_from_slice(&[0; 4]); //               52 device model
    out.extend_from_slice(&[0; 8]); //               56 device attributes
    out.extend_from_slice(&[0; 4]); //               64 rendering intent: perceptual
    for v in D50 {
        out.extend_from_slice(&v.to_be_bytes()); //  68 PCS illuminant, fixed by spec
    }
    out.extend_from_slice(&[0; 4]); //               80 profile creator
    out.extend_from_slice(&[0; 16]); //              84 profile ID (zero is accepted in practice)
    out.extend_from_slice(&[0; 28]); //             100 reserved
    debug_assert_eq!(out.len(), 128);
    // --- tag table and data ---
    out.extend_from_slice(&(tags.len() as u32).to_be_bytes());
    out.extend_from_slice(&table);
    out.extend_from_slice(&payload);
    out
}

/// `XYZType`: signature, four reserved zero bytes, three s15Fixed16.
fn xyz_tag(v: [i32; 3]) -> Vec<u8> {
    let mut t = Vec::with_capacity(20);
    t.extend_from_slice(b"XYZ ");
    t.extend_from_slice(&[0; 4]);
    for c in v {
        t.extend_from_slice(&c.to_be_bytes());
    }
    t
}

/// `curveType`: signature, reserved, u32 count, then that many big-endian
/// u16 samples. A count of 1 is the gamma form: its one entry is a u8Fixed8
/// exponent.
fn curv_tag(table: &[u16]) -> Vec<u8> {
    let mut t = Vec::with_capacity(12 + 2 * table.len());
    t.extend_from_slice(b"curv");
    t.extend_from_slice(&[0; 4]);
    t.extend_from_slice(&(table.len() as u32).to_be_bytes());
    for v in table {
        t.extend_from_slice(&v.to_be_bytes());
    }
    t
}

/// `s15Fixed16ArrayType`: the `chad` matrix, row-major.
fn sf32_tag(v: &[i32; 9]) -> Vec<u8> {
    let mut t = Vec::with_capacity(8 + 36);
    t.extend_from_slice(b"sf32");
    t.extend_from_slice(&[0; 4]);
    for c in v {
        t.extend_from_slice(&c.to_be_bytes());
    }
    t
}

/// `textType`: signature, reserved, NUL-terminated 7-bit ASCII.
fn text_tag(s: &str) -> Vec<u8> {
    let mut t = Vec::with_capacity(8 + s.len() + 1);
    t.extend_from_slice(b"text");
    t.extend_from_slice(&[0; 4]);
    t.extend_from_slice(s.as_bytes());
    t.push(0);
    t
}

/// `textDescriptionType`, the v2 name tag: signature, reserved, u32 ASCII
/// count INCLUDING its NUL, the string, then the fixed tail — u32 Unicode
/// language, u32 Unicode count (0), u16 ScriptCode, u8 Macintosh count, and
/// a 67-byte Macintosh buffer that is present even when unused. Total size
/// is `ascii + 90`. This build WRITES the full tail; the reader deliberately
/// does not REQUIRE it (real profiles ship truncated ones).
fn desc_tag(s: &str) -> Vec<u8> {
    let ascii = s.len() + 1;
    let mut t = Vec::with_capacity(ascii + 90);
    t.extend_from_slice(b"desc");
    t.extend_from_slice(&[0; 4]);
    t.extend_from_slice(&(ascii as u32).to_be_bytes());
    t.extend_from_slice(s.as_bytes());
    t.push(0);
    t.extend_from_slice(&[0; 4]); // Unicode language code
    t.extend_from_slice(&[0; 4]); // Unicode count: no Unicode string
    t.extend_from_slice(&[0; 2]); // ScriptCode code
    t.push(0); // Macintosh string count
    t.extend_from_slice(&[0; 67]); // Macintosh string buffer
    t
}

/// `multiLocalizedUnicodeType`, the v4 text tag: signature, reserved, one
/// record of 12 bytes (language "en", country "US", the string's byte length
/// and its offset from the tag's start, 28), then the string as UTF-16BE
/// with no terminator.
fn mluc_tag(s: &str) -> Vec<u8> {
    let units: Vec<u16> = s.encode_utf16().collect();
    let mut t = Vec::with_capacity(28 + 2 * units.len());
    t.extend_from_slice(b"mluc");
    t.extend_from_slice(&[0; 4]);
    t.extend_from_slice(&1u32.to_be_bytes());
    t.extend_from_slice(&12u32.to_be_bytes());
    t.extend_from_slice(b"enUS");
    t.extend_from_slice(&((2 * units.len()) as u32).to_be_bytes());
    t.extend_from_slice(&28u32.to_be_bytes());
    for unit in units {
        t.extend_from_slice(&unit.to_be_bytes());
    }
    t
}
