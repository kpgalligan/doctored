//! The binary duotone specification (#1 Duotone phase 1b,
//! `designs/duotone-mode.md`): the colour-mode data of a colour-mode-8
//! PSD or PSB, which is also the body of a Photoshop Duotone Options (`.ado`)
//! file, read into a [`DuotoneInks`] and written from one.
//!
//! # Source
//!
//! Adobe's file format specification
//! (<https://www.adobe.com/devnet-apps/photoshop/fileformatashtml/>, "Color
//! Mode Data Section") says only that a duotone image's colour data is the
//! duotone specification, undocumented, and that a reader should preserve
//! it. Its colour records are the specification's "Color structure" (a
//! 2-byte colour space, then four 2-byte components). The layout below is
//! the one Photoshop 26.10.0 writes, read off the Duotone PSDs of the four
//! #1 1b smokes (`scripts/photoshop-probe/duotone_take.py`, whose inks,
//! names and overprints were known), 524 bytes, big-endian:
//!
//! | offset | bytes | field |
//! |---|---|---|
//! | 0 | 2 | version, 1 |
//! | 2 | 2 | the ink count, 1–4 |
//! | 4 | 4 × 10 | each ink's colour record; an unused slot is space 65535, all zero |
//! | 44 | 4 × 64 | each ink's name, a Pascal string (a length byte, then the bytes) |
//! | 300 | 4 × 28 | each ink's curve: 13 `i16` for the inputs 0, 5, 10, 20, …, 90, 95, 100 % in tenths of a percent, −1 blank, then one `i16` (0) |
//! | 412 | 2 | the dot gain, 20 |
//! | 414 | 11 × 10 | the overprint colours, one per [`overprint_subsets`] entry of the ink count, then unused slots; a blank one is space 65535 |
//!
//! **Read** ([`read_spec`]): the version, the count, each used ink's colour
//! (Lab, space 7: `L·100` unsigned, `a·100` and `b·100` signed, exactly; RGB,
//! space 0, and HSB, space 1, as 16-bit sRGB components and through sRGB
//! into Lab; Gray, space 8, `0…10000` as ink percent, so `L* = 100 − v/100`;
//! any other space refuses, naming the ink and the space), its name (UTF-8,
//! else each byte as Latin-1) and its curve, and each overprint of the count.
//! An overprint record equal, at the file's precision, to the default
//! [`write_spec`] would write for those inks reads back blank (`None`), so
//! it keeps following the inks; a blank record reads blank and any other
//! reads as authored. Carried opaque (not read, kept whole by the PSD carry
//! and by an `.ado` saved from an unchanged read): the curves' fourteenth
//! word, the dot gain, the unused slots and any bytes past 524.
//!
//! **Write** ([`write_spec`]): Lab inks and every overprint (the authored one,
//! else the default the view computes), each quantized to the file's fields
//! — Lab in steps of 0.01, curve points in steps of 0.1 % — the fields the
//! model lacks as Photoshop wrote them (the fourteenth curve word 0, the dot
//! gain 20, an unused slot blank with Photoshop's default curve 0 % / 100 %).
//! A name longer than the record's 63 bytes is cut at a character boundary.
//! The overprint slot order for three and four inks is `overprint_subsets`'
//! (by size, then lexicographic); the smokes measured the two-ink slot only.

use crate::duotone_ink::{overprint_subsets, DuotoneInk, DuotoneInks, CURVE_INPUTS, MAX_INKS};

/// The bytes of a duotone specification Photoshop writes.
pub(crate) const SPEC_BYTES: usize = 524;

/// The image resources Adobe's table lists for a duotone image (1014
/// halftoning, 1017 transfer functions, 1018 image information) and 1063,
/// which #1 1b's request named; Photoshop 26.10.0's own Duotone PSD holds
/// 1014 and 1017.
pub(crate) const DUOTONE_RESOURCE_IDS: [u16; 4] = [1014, 1017, 1018, 1063];

/// The file block a description was read from: the colour-mode data and the
/// duotone resources, verbatim, so an unchanged description writes them
/// back byte for byte (`duotone_files`).
#[derive(Clone, Debug)]
pub(crate) struct DuotoneFileBlock {
    pub mode_data: Vec<u8>,
    pub resources: Vec<(u16, Vec<u8>)>,
}

const NAMES: usize = 44;
const CURVES: usize = 300;
const DOT_GAIN: usize = 412;
const OVERPRINTS: usize = 414;
const NAME_BYTES: usize = 64;
const CURVE_BYTES: usize = 28;
const RECORD: usize = 10;
const OVERPRINT_SLOTS: usize = 11;
/// A blank colour record's space.
const BLANK: u16 = 0xffff;
/// The dot gain Photoshop writes.
const PHOTOSHOP_DOT_GAIN: u16 = 20;

fn u16_at(bytes: &[u8], at: usize) -> u16 {
    u16::from_be_bytes([bytes[at], bytes[at + 1]])
}

/// One colour record: the space and four components.
fn record(bytes: &[u8], at: usize) -> (u16, [u16; 4]) {
    (
        u16_at(bytes, at),
        std::array::from_fn(|i| u16_at(bytes, at + 2 + 2 * i)),
    )
}

/// A colour record as D50 Lab (module doc), or the refusal naming `what`.
fn record_lab(space: u16, c: [u16; 4], what: &str) -> Result<[f64; 3], String> {
    let unit = |v: u16| f64::from(v) / 65535.0;
    let srgb = |rgb: [f64; 3]| -> Result<[f64; 3], String> {
        crate::lab::srgb_to_lab(rgb.map(|v| v as f32))
            .map(|lab| lab.map(f64::from))
            .ok_or_else(|| format!("{what}'s colour cannot be read through sRGB"))
    };
    match space {
        7 => Ok(dequantized(c)),
        0 => srgb([unit(c[0]), unit(c[1]), unit(c[2])]),
        1 => srgb(hsb_to_rgb(
            f64::from(c[0]) / 65536.0 * 360.0,
            unit(c[1]),
            unit(c[2]),
        )),
        8 => Ok([100.0 - f64::from(c[0].min(10000)) / 100.0, 0.0, 0.0]),
        other => Err(format!(
            "{what}'s colour is in colour space {other}, which Doctored does not read (Lab, RGB, HSB and Gray only)"
        )),
    }
}

/// HSB (hue in degrees, saturation and brightness in 0…1) to RGB in 0…1.
fn hsb_to_rgb(hue: f64, saturation: f64, brightness: f64) -> [f64; 3] {
    let h = (hue.rem_euclid(360.0)) / 60.0;
    let c = brightness * saturation;
    let x = c * (1.0 - (h.rem_euclid(2.0) - 1.0).abs());
    let (r, g, b) = match h as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let m = brightness - c;
    [r + m, g + m, b + m]
}

/// A Lab colour at the file's precision: the Lab record's components.
fn quantized(lab: [f64; 3]) -> [u16; 4] {
    let signed = |v: f64| (v * 100.0).round().clamp(-12800.0, 12700.0) as i16 as u16;
    [
        (lab[0] * 100.0).round().clamp(0.0, 10000.0) as u16,
        signed(lab[1]),
        signed(lab[2]),
        0,
    ]
}

fn name_of(bytes: &[u8]) -> String {
    match std::str::from_utf8(bytes) {
        Ok(text) => text.to_string(),
        Err(_) => bytes.iter().map(|&b| char::from(b)).collect(),
    }
}

/// The description a duotone specification holds (module doc). A refusal
/// names the field.
pub(crate) fn read_spec(bytes: &[u8]) -> Result<DuotoneInks, String> {
    if bytes.len() < SPEC_BYTES {
        return Err(format!(
            "the duotone specification is {} bytes, shorter than the {SPEC_BYTES} Photoshop writes",
            bytes.len()
        ));
    }
    let version = u16_at(bytes, 0);
    if version != 1 {
        return Err(format!(
            "the duotone specification's version is {version}, not 1"
        ));
    }
    let count = usize::from(u16_at(bytes, 2));
    if !(1..=MAX_INKS).contains(&count) {
        return Err(format!(
            "the duotone specification holds {count} inks; a Duotone holds 1 to {MAX_INKS}"
        ));
    }
    let mut inks = Vec::with_capacity(count);
    for i in 0..count {
        let what = format!("ink {}", i + 1);
        let (space, components) = record(bytes, 4 + RECORD * i);
        let lab = record_lab(space, components, &what)?;
        let at = NAMES + NAME_BYTES * i;
        let length = usize::from(bytes[at]);
        if length >= NAME_BYTES {
            return Err(format!("{what}'s name is longer than its 63-byte record"));
        }
        let name = name_of(&bytes[at + 1..at + 1 + length]);
        let mut curve = [None; 13];
        for (at, value) in curve.iter_mut().enumerate() {
            let raw = u16_at(bytes, CURVES + CURVE_BYTES * i + 2 * at) as i16;
            if !(raw == -1 || (0..=1000).contains(&raw)) {
                let input = CURVE_INPUTS[at];
                return Err(format!(
                    "{what}'s curve slot {input} % holds {raw}, outside 0 to 1000 tenths of a percent"
                ));
            }
            *value = (raw >= 0).then(|| f64::from(raw) / 10.0);
        }
        inks.push(DuotoneInk { name, lab, curve });
    }
    // The defaults `write_spec` would write for these inks, at the file's
    // precision: a record equal to one reads back blank.
    let defaults = DuotoneInks::new(inks.clone(), vec![None; overprint_subsets(count).len()])?
        .overprint_colours();
    let mut overprints = Vec::with_capacity(defaults.len());
    for (j, (subset, default)) in overprint_subsets(count).iter().zip(&defaults).enumerate() {
        let (space, components) = record(bytes, OVERPRINTS + RECORD * j);
        if space == BLANK {
            overprints.push(None);
            continue;
        }
        if space == 7 && components == quantized(*default) {
            overprints.push(None);
            continue;
        }
        let key: Vec<String> = subset.iter().map(|i| (i + 1).to_string()).collect();
        let what = format!("overprint {}", key.join("+"));
        overprints.push(Some(record_lab(space, components, &what)?));
    }
    DuotoneInks::new(inks, overprints)
}

fn put_u16(out: &mut [u8], at: usize, value: u16) {
    out[at..at + 2].copy_from_slice(&value.to_be_bytes());
}

fn put_record(out: &mut [u8], at: usize, space: u16, components: [u16; 4]) {
    put_u16(out, at, space);
    for (i, c) in components.iter().enumerate() {
        put_u16(out, at + 2 + 2 * i, *c);
    }
}

/// `name` cut to `max` bytes at a character boundary.
fn cut(name: &str, max: usize) -> &str {
    let mut end = name.len().min(max);
    while !name.is_char_boundary(end) {
        end -= 1;
    }
    &name[..end]
}

/// The duotone specification of `inks` (module doc), [`SPEC_BYTES`] long.
pub(crate) fn write_spec(inks: &DuotoneInks) -> Vec<u8> {
    let mut out = vec![0u8; SPEC_BYTES];
    let list = inks.inks();
    put_u16(&mut out, 0, 1);
    put_u16(&mut out, 2, list.len() as u16);
    for slot in 0..MAX_INKS {
        let ink = list.get(slot);
        match ink {
            Some(ink) => put_record(&mut out, 4 + RECORD * slot, 7, quantized(ink.lab)),
            None => put_record(&mut out, 4 + RECORD * slot, BLANK, [0; 4]),
        }
        if let Some(ink) = ink {
            let name = cut(&ink.name, NAME_BYTES - 1);
            let at = NAMES + NAME_BYTES * slot;
            out[at] = name.len() as u8;
            out[at + 1..at + 1 + name.len()].copy_from_slice(name.as_bytes());
        }
        for i in 0..13 {
            let value = match ink {
                Some(ink) => {
                    ink.curve[i].map_or(-1, |v| (v * 10.0).round().clamp(0.0, 1000.0) as i16)
                }
                // Photoshop's unused slot: 0 % and 100 % set, the rest blank.
                None => match i {
                    0 => 0,
                    12 => 1000,
                    _ => -1,
                },
            };
            put_u16(&mut out, CURVES + CURVE_BYTES * slot + 2 * i, value as u16);
        }
    }
    put_u16(&mut out, DOT_GAIN, PHOTOSHOP_DOT_GAIN);
    // A blank overprint is written as the default of the inks AS WRITTEN
    // (their Lab at the file's precision), the default `read_spec` compares
    // with, so it reads back blank.
    let written: Vec<DuotoneInk> = list
        .iter()
        .map(|ink| DuotoneInk {
            lab: dequantized(quantized(ink.lab)),
            ..ink.clone()
        })
        .collect();
    let defaults = DuotoneInks::new(written, vec![None; inks.overprints().len()])
        .map(|d| d.overprint_colours())
        .unwrap_or_else(|_| inks.overprint_colours());
    for j in 0..OVERPRINT_SLOTS {
        let at = OVERPRINTS + RECORD * j;
        match (inks.overprints().get(j), defaults.get(j)) {
            (Some(Some(lab)), _) | (Some(None), Some(lab)) => {
                put_record(&mut out, at, 7, quantized(*lab))
            }
            _ => put_record(&mut out, at, BLANK, [0; 4]),
        }
    }
    out
}

/// A Lab record's components back as Lab.
fn dequantized(c: [u16; 4]) -> [f64; 3] {
    [
        f64::from(c[0]) / 100.0,
        f64::from(c[1] as i16) / 100.0,
        f64::from(c[2] as i16) / 100.0,
    ]
}
