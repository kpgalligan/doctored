//! KERNEL (#547 spot channels 1b-1): the ONE reader and writer of a PSD's
//! saved-channel resources — 1006 (Pascal names), 1045 (Unicode names), 1077
//! (DisplayInfo) and 1053 (alpha identifiers) — and the sample polarity of a
//! saved-channel plane. `psd_spot` adapts it to the container and the two
//! document types; nothing here names a document.
//!
//! The forms are Photoshop 26.10.0's own, measured by
//! `scripts/photoshop-probe/spot_psd_take.py` take1 (seven files: RGB, CMYK
//! and Grayscale at 8 and 16 bits, PSD and PSB), on top of Adobe's file
//! format specification:
//!
//! - 1006: one Pascal string per saved channel, no padding between them.
//!   Photoshop writes it in Mac Roman ("é" is 0x8E); this writer writes the
//!   UTF-8 name (`psd_export_encode::truncated_name`, as a layer record's
//!   Pascal name), and the reader decodes a byte per code point. Both
//!   readers prefer 1045, which every Photoshop file carries.
//! - 1045: per saved channel a u32 count of UTF-16 units, the count
//!   INCLUDING one trailing NUL, then the big-endian units.
//! - 1077: a u32 version (1), then per saved channel 13 bytes: u16 colour
//!   space, four u16 colour words, u16 opacity (Solidity, percent), u8 kind
//!   (2 = spot). A spot ink is colour space 7 (Lab): L × 100 in the first
//!   word, a × 100 and b × 100 as signed words, the fourth word 0.
//! - 1053: a u32 identifier per saved channel; Photoshop numbered a merged
//!   transparency 0 and its saved channels from 3.
//! - With a merged transparency (a negative layer count), every one of the
//!   four lists carries ONE more entry, FIRST: "Transparency", kind 1,
//!   colour space 0 (red, 65535), opacity 100, identifier 0. Its plane is the
//!   merged alpha, not a saved channel; [`read_saved`] sets it aside and
//!   [`encode_resources`] writes it when asked.
//!
//! THE POLARITY ([`stored`]): Photoshop stores a spot channel's plane
//! INVERTED, `max − coverage` (255 or 65535 = no ink, 0 = full ink), the way
//! it stores CMYK ink planes. take1's verdict was H1 on every file: the
//! rectangle spot exact, the ramp spot exact at 8 bits and within 1 code of
//! `257 v` at 16 (Photoshop's own 8 → 16 conversion). The map is its own
//! inverse, so ONE function reads and writes it.

use crate::psd_export_encode::{resource, truncated_name};

/// Image resource: the saved channels' Pascal names.
pub(crate) const NAMES: u16 = 1006;
/// Image resource: the saved channels' Unicode names.
pub(crate) const UNICODE_NAMES: u16 = 1045;
/// Image resource: the saved channels' DisplayInfo.
pub(crate) const DISPLAY_INFO: u16 = 1077;
/// Image resource: the saved channels' identifiers.
pub(crate) const ALPHA_IDENTIFIERS: u16 = 1053;
/// DisplayInfo's colour space for Lab (take1: every spot ink).
pub(crate) const LAB_SPACE: u16 = 7;
/// DisplayInfo's kind for a spot channel (take1: every spot channel).
pub(crate) const KIND_SPOT: u8 = 2;

/// The only DisplayInfo version Photoshop writes and this reader accepts.
const DISPLAY_VERSION: u32 = 1;
/// One DisplayInfo record's bytes.
const DISPLAY_RECORD: usize = 13;
/// The identifier Photoshop gave the first saved channel (take1).
const FIRST_IDENTIFIER: u32 = 3;
/// The merged transparency's entry (module doc).
const TRANSPARENCY_NAME: &str = "Transparency";
const TRANSPARENCY: DisplayInfo = DisplayInfo {
    color_space: 0,
    color: [65535, 0, 0, 0],
    opacity: 100,
    kind: 1,
};

/// One saved channel's DisplayInfo record, as stored.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct DisplayInfo {
    pub color_space: u16,
    pub color: [u16; 4],
    pub opacity: u16,
    pub kind: u8,
}

/// One saved channel: its name and its DisplayInfo.
#[derive(Clone, Debug)]
pub(crate) struct SavedChannel {
    pub name: String,
    pub display: DisplayInfo,
}

fn be_u32(bytes: &[u8], at: usize) -> Option<u32> {
    let b = bytes.get(at..at.checked_add(4)?)?;
    Some(u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
}

fn be_u16(bytes: &[u8], at: usize) -> u16 {
    u16::from_be_bytes([bytes[at], bytes[at + 1]])
}

/// 1045's names, trailing NUL units stripped.
fn unicode_names(body: &[u8]) -> Result<Vec<String>, String> {
    let mut out = Vec::new();
    let mut at = 0usize;
    while at < body.len() {
        let count = be_u32(body, at).ok_or("truncated unicode channel name")? as usize;
        let start = at + 4;
        let end = count
            .checked_mul(2)
            .and_then(|n| start.checked_add(n))
            .filter(|&end| end <= body.len())
            .ok_or("truncated unicode channel name")?;
        let mut units: Vec<u16> = body[start..end]
            .chunks_exact(2)
            .map(|u| u16::from_be_bytes([u[0], u[1]]))
            .collect();
        while units.last() == Some(&0) {
            units.pop();
        }
        out.push(String::from_utf16_lossy(&units));
        at = end;
    }
    Ok(out)
}

/// 1006's names, a byte per code point.
fn pascal_names(body: &[u8]) -> Result<Vec<String>, String> {
    let mut out = Vec::new();
    let mut at = 0usize;
    while at < body.len() {
        let length = usize::from(body[at]);
        let name = body
            .get(at + 1..at + 1 + length)
            .ok_or("truncated channel name")?;
        out.push(name.iter().map(|&b| char::from(b)).collect());
        at += 1 + length;
    }
    Ok(out)
}

fn display_records(body: &[u8]) -> Result<Vec<DisplayInfo>, String> {
    if be_u32(body, 0) != Some(DISPLAY_VERSION) {
        return Err("unknown DisplayInfo version".into());
    }
    let records = &body[4..];
    if !records.len().is_multiple_of(DISPLAY_RECORD) {
        return Err("truncated DisplayInfo record".into());
    }
    Ok(records
        .chunks_exact(DISPLAY_RECORD)
        .map(|r| DisplayInfo {
            color_space: be_u16(r, 0),
            color: [be_u16(r, 2), be_u16(r, 4), be_u16(r, 6), be_u16(r, 8)],
            opacity: be_u16(r, 10),
            kind: r[12],
        })
        .collect())
}

/// The file's `count` saved channels, named from 1045 (else 1006) and
/// described by 1077. With `merged_alpha`, lists carrying `count + 1`
/// entries lead with the merged transparency's, which is set aside (module
/// doc). `Err` when 1077 or both name lists are absent, a list is
/// malformed, or a list does not hold exactly that many entries.
pub(crate) fn read_saved(
    pascal: Option<&[u8]>,
    unicode: Option<&[u8]>,
    display: Option<&[u8]>,
    count: usize,
    merged_alpha: bool,
) -> Result<Vec<SavedChannel>, String> {
    let mut names = match (unicode, pascal) {
        (Some(body), _) => unicode_names(body)?,
        (None, Some(body)) => pascal_names(body)?,
        (None, None) => return Err("no channel names".into()),
    };
    let mut records = display_records(display.ok_or("no DisplayInfo")?)?;
    if merged_alpha && names.len() == count + 1 && records.len() == count + 1 {
        names.remove(0);
        records.remove(0);
    }
    if names.len() != count || records.len() != count {
        return Err("the saved-channel lists differ from the channel count".into());
    }
    Ok(names
        .into_iter()
        .zip(records)
        .map(|(name, display)| SavedChannel { name, display })
        .collect())
}

/// A spot ink's D50 Lab and Solidity from its record: colour space 7 and an
/// opacity of at most 100, else `None`. The Lab range is the caller's
/// (`SpotInk::new`).
pub(crate) fn spot_lab(display: &DisplayInfo) -> Option<([f64; 3], u8)> {
    if display.color_space != LAB_SPACE || display.opacity > 100 {
        return None;
    }
    let word = |i: usize| f64::from(display.color[i] as i16) / 100.0;
    Some(([word(0), word(1), word(2)], display.opacity as u8))
}

/// The ink as 1077's colour words: each component × 100, rounded.
fn lab_words(lab: [f64; 3]) -> [u16; 4] {
    let word = |v: f64| (v * 100.0).round() as i16 as u16;
    [word(lab[0]), word(lab[1]), word(lab[2]), 0]
}

/// Resources 1006, 1045, 1077 and 1053 for `spots` (name, D50 Lab,
/// Solidity), whole resource blocks in Photoshop's order; with
/// `transparency` (the file declares merged alpha) each list leads with the
/// merged transparency's entry (module doc). Empty when `spots` is.
pub(crate) fn encode_resources(spots: &[(&str, [f64; 3], u8)], transparency: bool) -> Vec<u8> {
    if spots.is_empty() {
        return Vec::new();
    }
    let mut entries: Vec<(&str, DisplayInfo, u32)> = Vec::with_capacity(spots.len() + 1);
    if transparency {
        entries.push((TRANSPARENCY_NAME, TRANSPARENCY, 0));
    }
    for (k, &(name, lab, solidity)) in spots.iter().enumerate() {
        let display = DisplayInfo {
            color_space: LAB_SPACE,
            color: lab_words(lab),
            opacity: u16::from(solidity),
            kind: KIND_SPOT,
        };
        entries.push((name, display, FIRST_IDENTIFIER + k as u32));
    }
    let mut pascal = Vec::new();
    let mut unicode = Vec::new();
    let mut display = DISPLAY_VERSION.to_be_bytes().to_vec();
    let mut identifiers = Vec::new();
    for (name, info, identifier) in &entries {
        let short = truncated_name(name).as_bytes();
        pascal.push(short.len() as u8);
        pascal.extend_from_slice(short);
        let units: Vec<u16> = name.encode_utf16().chain(std::iter::once(0)).collect();
        unicode.extend_from_slice(&(units.len() as u32).to_be_bytes());
        for unit in units {
            unicode.extend_from_slice(&unit.to_be_bytes());
        }
        display.extend_from_slice(&info.color_space.to_be_bytes());
        for word in info.color {
            display.extend_from_slice(&word.to_be_bytes());
        }
        display.extend_from_slice(&info.opacity.to_be_bytes());
        display.push(info.kind);
        identifiers.extend_from_slice(&identifier.to_be_bytes());
    }
    let mut out = resource(NAMES, &pascal);
    out.extend_from_slice(&resource(UNICODE_NAMES, &unicode));
    out.extend_from_slice(&resource(DISPLAY_INFO, &display));
    out.extend_from_slice(&resource(ALPHA_IDENTIFIERS, &identifiers));
    out
}

/// THE polarity (module doc), both directions: a coverage sample as stored,
/// and a stored sample as coverage, `max − v`.
pub(crate) fn stored(coverage: u16, max: u16) -> u16 {
    max.saturating_sub(coverage)
}
