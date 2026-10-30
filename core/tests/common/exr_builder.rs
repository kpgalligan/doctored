//! A hand-written OpenEXR byte writer, independent of the `exr` crate: the
//! oracle the OpenEXR tests read against. Written from the file layout in the
//! OpenEXR specification ("OpenEXR File Layout"): the magic number, the
//! version field with its flag bits, one attribute list per part, an offset
//! table per part and uncompressed scanline chunks of one line each.
//!
//! It writes only what the tests need: FLOAT and HALF channels (a HALF
//! channel takes the 16 BITS of each sample, so a test states exactly what is
//! in the file), any channel names, a data window and a display window, the
//! `chromaticities` and `pixelAspectRatio` attributes, several parts, and a
//! part flagged deep (its header only — a deep part's chunks are never read
//! by the code under test, which refuses on the header).
#![allow(dead_code)]

/// One channel's samples, row 0 first, `width * height` of them.
#[derive(Clone)]
pub enum Samples {
    Float(Vec<f32>),
    /// The 16 bits of each IEEE half.
    Half(Vec<u16>),
}

#[derive(Clone)]
pub struct Channel {
    pub name: &'static str,
    pub samples: Samples,
}

/// One part of a file. Windows are `(x, y, width, height)`.
#[derive(Clone)]
pub struct Part {
    pub name: Option<&'static str>,
    pub data_window: (i32, i32, u32, u32),
    pub display_window: (i32, i32, u32, u32),
    pub channels: Vec<Channel>,
    pub chromaticities: Option<[[f32; 2]; 4]>,
    pub pixel_aspect: f32,
    pub deep: bool,
}

impl Part {
    /// A flat part whose data and display windows are both `width` x
    /// `height` at the origin.
    pub fn new(width: u32, height: u32, channels: Vec<Channel>) -> Self {
        Self {
            name: None,
            data_window: (0, 0, width, height),
            display_window: (0, 0, width, height),
            channels,
            chromaticities: None,
            pixel_aspect: 1.0,
            deep: false,
        }
    }
}

/// FLOAT channels named `names` out of interleaved `samples`
/// (`names.len()` per pixel).
pub fn float_channels(names: &[&'static str], samples: &[f32]) -> Vec<Channel> {
    names
        .iter()
        .enumerate()
        .map(|(index, name)| Channel {
            name,
            samples: Samples::Float(
                samples
                    .iter()
                    .skip(index)
                    .step_by(names.len())
                    .copied()
                    .collect(),
            ),
        })
        .collect()
}

/// HALF channels named `names` out of interleaved half BITS.
pub fn half_channels(names: &[&'static str], bits: &[u16]) -> Vec<Channel> {
    names
        .iter()
        .enumerate()
        .map(|(index, name)| Channel {
            name,
            samples: Samples::Half(
                bits.iter()
                    .skip(index)
                    .step_by(names.len())
                    .copied()
                    .collect(),
            ),
        })
        .collect()
}

fn text(out: &mut Vec<u8>, value: &str) {
    out.extend_from_slice(value.as_bytes());
    out.push(0);
}

fn attribute(out: &mut Vec<u8>, name: &str, kind: &str, value: &[u8]) {
    text(out, name);
    text(out, kind);
    out.extend_from_slice(&(value.len() as i32).to_le_bytes());
    out.extend_from_slice(value);
}

fn box2i(window: (i32, i32, u32, u32)) -> Vec<u8> {
    let (x, y, w, h) = window;
    [x, y, x + w as i32 - 1, y + h as i32 - 1]
        .iter()
        .flat_map(|v| v.to_le_bytes())
        .collect()
}

/// The part's channels in the order the file stores them: by name.
fn sorted(part: &Part) -> Vec<&Channel> {
    let mut channels: Vec<&Channel> = part.channels.iter().collect();
    channels.sort_by_key(|channel| channel.name);
    channels
}

fn header(part: &Part, multipart: bool) -> Vec<u8> {
    let mut out = Vec::new();
    let mut list = Vec::new();
    for channel in sorted(part) {
        text(&mut list, channel.name);
        let kind: i32 = match channel.samples {
            Samples::Half(_) => 1,
            Samples::Float(_) => 2,
        };
        list.extend_from_slice(&kind.to_le_bytes());
        list.extend_from_slice(&[0, 0, 0, 0]); // pLinear and three reserved bytes
        list.extend_from_slice(&1i32.to_le_bytes()); // xSampling
        list.extend_from_slice(&1i32.to_le_bytes()); // ySampling
    }
    list.push(0);
    attribute(&mut out, "channels", "chlist", &list);
    if let Some(c) = part.chromaticities {
        let value: Vec<u8> = c.iter().flatten().flat_map(|v| v.to_le_bytes()).collect();
        attribute(&mut out, "chromaticities", "chromaticities", &value);
    }
    attribute(&mut out, "compression", "compression", &[0]);
    attribute(&mut out, "dataWindow", "box2i", &box2i(part.data_window));
    attribute(
        &mut out,
        "displayWindow",
        "box2i",
        &box2i(part.display_window),
    );
    attribute(&mut out, "lineOrder", "lineOrder", &[0]);
    attribute(
        &mut out,
        "pixelAspectRatio",
        "float",
        &part.pixel_aspect.to_le_bytes(),
    );
    let center: Vec<u8> = [0f32, 0f32].iter().flat_map(|v| v.to_le_bytes()).collect();
    attribute(&mut out, "screenWindowCenter", "v2f", &center);
    attribute(&mut out, "screenWindowWidth", "float", &1f32.to_le_bytes());
    if multipart || part.deep {
        let kind = if part.deep {
            "deepscanline"
        } else {
            "scanlineimage"
        };
        let mut value = kind.as_bytes().to_vec();
        attribute(&mut out, "type", "string", &value);
        value = part.name.unwrap_or("part").as_bytes().to_vec();
        attribute(&mut out, "name", "string", &value);
        attribute(
            &mut out,
            "chunkCount",
            "int",
            &(part.data_window.3 as i32).to_le_bytes(),
        );
    }
    if part.deep {
        attribute(&mut out, "version", "int", &1i32.to_le_bytes());
        attribute(&mut out, "maxSamplesPerPixel", "int", &1i32.to_le_bytes());
    }
    out.push(0);
    out
}

/// One scanline's pixel bytes: each channel in file order, little-endian.
fn line(part: &Part, y: usize) -> Vec<u8> {
    let width = part.data_window.2 as usize;
    let mut out = Vec::new();
    for channel in sorted(part) {
        match &channel.samples {
            Samples::Float(values) => {
                for value in &values[y * width..(y + 1) * width] {
                    out.extend_from_slice(&value.to_le_bytes());
                }
            }
            Samples::Half(values) => {
                for value in &values[y * width..(y + 1) * width] {
                    out.extend_from_slice(&value.to_le_bytes());
                }
            }
        }
    }
    out
}

/// The whole file. More than one part writes the multi-part layout; a deep
/// part writes its header and an offset table of zeros, with no chunks.
pub fn file(parts: &[Part]) -> Vec<u8> {
    let multipart = parts.len() > 1;
    let deep = parts.iter().any(|part| part.deep);
    let mut out = vec![0x76, 0x2f, 0x31, 0x01];
    let mut version: u32 = 2;
    if deep {
        version |= 1 << 11;
    }
    if multipart {
        version |= 1 << 12;
    }
    out.extend_from_slice(&version.to_le_bytes());
    for part in parts {
        out.extend(header(part, multipart));
    }
    if multipart {
        out.push(0);
    }
    // The offset tables, one entry per scanline, patched once the chunks'
    // positions are known.
    let mut tables = Vec::new();
    for part in parts {
        tables.push(out.len());
        out.extend(std::iter::repeat_n(0u8, part.data_window.3 as usize * 8));
    }
    for (index, part) in parts.iter().enumerate() {
        if part.deep {
            continue;
        }
        for y in 0..part.data_window.3 as usize {
            let at = out.len() as u64;
            out[tables[index] + y * 8..tables[index] + y * 8 + 8]
                .copy_from_slice(&at.to_le_bytes());
            if multipart {
                out.extend_from_slice(&(index as i32).to_le_bytes());
            }
            out.extend_from_slice(&(part.data_window.1 + y as i32).to_le_bytes());
            let pixels = line(part, y);
            out.extend_from_slice(&(pixels.len() as i32).to_le_bytes());
            out.extend(pixels);
        }
    }
    out
}

/// An IEEE half's value, widened by hand from its 16 bits (sign, 5 exponent
/// bits biased by 15, 10 mantissa bits) — independent of the `half` crate.
pub fn half_value(bits: u16) -> f32 {
    let sign = if bits & 0x8000 != 0 { -1.0f64 } else { 1.0 };
    let exponent = i32::from((bits >> 10) & 0x1f);
    let mantissa = f64::from(bits & 0x3ff);
    let magnitude = match exponent {
        0 => mantissa * 2f64.powi(-24),
        31 if mantissa == 0.0 => f64::INFINITY,
        31 => f64::NAN,
        e => (1.0 + mantissa / 1024.0) * 2f64.powi(e - 15),
    };
    (sign * magnitude) as f32
}

/// The 16 bits of the half nearest `value` (ties to even), found by search
/// over the finite halves rather than by bit manipulation, so it shares no
/// logic with a converter under test. `value` must be finite with
/// `|value| <= 65504`.
pub fn nearest_half_bits(value: f32) -> u16 {
    let magnitude = f64::from(value.abs());
    // Positive finite halves are ordered by their bit patterns.
    let (mut low, mut high) = (0u16, 0x7bffu16);
    while high - low > 1 {
        let middle = low + (high - low) / 2;
        if f64::from(half_value(middle)) <= magnitude {
            low = middle;
        } else {
            high = middle;
        }
    }
    let (below, above) = (
        magnitude - f64::from(half_value(low)),
        f64::from(half_value(high)) - magnitude,
    );
    let bits = if below < above || (below == above && low & 1 == 0) {
        low
    } else {
        high
    };
    if value.is_sign_negative() {
        bits | 0x8000
    } else {
        bits
    }
}
