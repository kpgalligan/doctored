//! `psd_adjustment`'s private half for a CMYK document's Gradient Map record
//! (`grdm`, #547, the styles batch), both directions; reached only through
//! `psd_adjustment::decode_cmyk` / `encode_cmyk`. The RGB document has no
//! `grdm` codec (its Gradient Map record is not carried), so this is the one.
//!
//! The layout is Adobe's "Gradient settings" block, read off Photoshop
//! 26.10.0's own CMYK file (`cmyk-styles-547/take1/psd/adjust9.psd`, a
//! three-stop K100 → C50 Y100 → paper map):
//!
//! | field | bytes | mapping |
//! |---|---|---|
//! | version | u16 = 3 | 3 carries a 4-byte interpolation method after the flags (`Gcls`, Classic); 1 has none (read as Classic) |
//! | reversed, dithered | u8, u8 | `gradient.reverse`, `dither` |
//! | method | 4 bytes (version 3 only) | `Gcls` reads as below; any other method reads the legacy map (midpoints 50 only); `Gcls` written |
//! | name | u32 length + UTF-16 units (with the NUL) | ignored on read; `Custom` written |
//! | colour stops | u16 count; per stop u32 location 0…4096, u32 midpoint 0…100, u16 colour space, 4 × u16 components, u16 stop type | `position = location / 4096`; space 2 (CMYK) components are `65535·(1 − ink)` (0 = 100 % ink, Adobe's colour structure); another space or a stop type other than 0 (user colour) declines; the midpoint is below |
//! | transparency stops | u16 count; per stop u32 location, u32 midpoint, u16 opacity 0…255 | every stop opaque, or one per colour stop at its location: its `opacity / 255`; anything else declines ← ; an opaque map writes Photoshop's two (locations 0 and 4096, opacity 255, midpoint 50), any other one per colour stop → |
//! | tail | expansion 2, interpolation, length 32, mode, seed, flags, roughness, colour model 3, minimum and maximum colour, a dummy word | the interpolation word (offset 92 of a two-stop body, 112 of three) is the Smoothness × 4096, below; the rest ignored on read and Photoshop's own written, its seed `0x44285F2B` included |
//! | pad | zero bytes to a multiple of 4 | ignored on read; written |
//!
//! The method, the Smoothness and the midpoints
//! (`scripts/photoshop-probe/gradient_map_678_take.py`, `adjust_map`'s module
//! doc). A Classic body (or version 1) whose interpolation word is 0 and
//! whose every `Mdpn` is 50 reads as the LEGACY map, exactly the params an
//! earlier build read; any other Classic body reads as `method: "classic"`
//! with `gradient.smoothness = word / 4096` (the key only when not 1, a word
//! above 4096 declines) and the midpoints SHIFTED: Photoshop segment `i`
//! reads stop `i + 1`'s `Mdpn` (the take's SEG rows), so our stop `i`'s
//! `midpoint` is Photoshop stop `i + 1`'s `Mdpn / 100`, and its
//! `opacity_midpoint` the matching transparency stop's (only for a
//! per-stop transparency list), each key only when not 50 and a `Mdpn`
//! outside 5…95 declining; stop 0's `Mdpn` governs nothing. The shift runs
//! in POSITION order, after a stable sort by location in which each stop
//! carries its midpoints, both ways. Encode writes `method: "classic"` with
//! the interpolation word `round(smoothness × 4096)` and the shifted `Mdpn`s
//! (`round(midpoint × 100)`, stop 0's 50); a legacy map (no `method`) writes
//! interpolation 0 and every `Mdpn` 50, so it reads back legacy. A deliberate
//! re-read change: a CMYK file an earlier build wrote (interpolation 4096 on
//! every map) now reopens as `method: "classic"` at Smoothness 1, as
//! Photoshop renders it.
//!
//! The pad is there because Photoshop 26.10.0 refuses a whole layered PSD or
//! PSB ("the open options are incorrect") and reads a layered TIFF flat when
//! a `grdm` body ends at Adobe's dummy word two bytes short of a multiple of
//! 4 — the 130-byte body of a two-stop map — while its own file ends the
//! 150-byte layout of its three-stop map with two zero bytes (152).
//! `cmyk-psd-open-662/take1` isolated `grdm` among every construct #547
//! added; take2 opened the padded body (`photoshop-probe/cmyk_psd_open_take.py`).
//!
//! Each decoded ink is quantized to four decimals, which recovers every
//! four-decimal ink exactly (one 16-bit step is below 0.00005).

use serde_json::{json, Map, Value};

use crate::adjust_parse::boolean;

/// The `Lctn` scale.
const LOCATION: f64 = 4096.0;
const MIDPOINT: u32 = 50;
const CMYK_SPACE: u16 = 2;
/// The random seed Photoshop 26.10.0 wrote in its own `grdm`
/// (`cmyk-styles-547/take2/psd/adjust9.psd`); a solid map never reads it.
const SEED: u32 = 0x4428_5F2B;

/// The Classic method's code, the one written.
const CLASSIC: &[u8] = b"Gcls";
/// The interpolation word of Smoothness 100 %.
const SMOOTH: u16 = 4096;
/// The `Mdpn` Photoshop's editor admits, 5…95 %.
const MIDPOINT_RANGE: std::ops::RangeInclusive<u32> = 5..=95;

const VERSION: &str = "gradient version is not 1 or 3";
const STOP: &str = "gradient stop is not a CMYK colour at its midpoint";
const SMOOTHNESS: &str = "gradient smoothness is above 4096";
const MIDPOINTS: &str = "gradient stop midpoint is not 5 to 95 percent";
const STOPS: &str = "gradient does not have between 2 and 32 colour stops";
const TRANSPARENCY: &str = "gradient transparency stops do not match its colour stops";

struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl Reader<'_> {
    fn take(&mut self, count: usize) -> Option<&[u8]> {
        let out = self.bytes.get(self.at..self.at.checked_add(count)?)?;
        self.at += count;
        Some(out)
    }
    fn u8(&mut self) -> Option<u8> {
        Some(self.take(1)?[0])
    }
    fn u16(&mut self) -> Option<u16> {
        Some(u16::from_be_bytes(self.take(2)?.try_into().ok()?))
    }
    fn u32(&mut self) -> Option<u32> {
        Some(u32::from_be_bytes(self.take(4)?.try_into().ok()?))
    }
}

fn q4(v: f64) -> f64 {
    (v * 1e4).round() / 1e4
}

/// One decoded colour stop: its location, ink, opacity (0…255) and the
/// `Mdpn` its colour and transparency stops carry.
struct Decoded {
    location: u32,
    ink: [f64; 4],
    opacity: u16,
    midpoint: u32,
    opacity_midpoint: u32,
}

/// `None` = a body shorter than its layout; `Err` = declined; `Ok` = the
/// CMYK `gradient_map` params (module doc).
pub(crate) fn decode(body: &[u8]) -> Option<Result<Map<String, Value>, String>> {
    let mut r = Reader { bytes: body, at: 0 };
    let version = r.u16()?;
    let reversed = r.u8()? != 0;
    let dithered = r.u8()? != 0;
    // Version 1 carries no method: Classic, as Photoshop reads it.
    let classic_method = match version {
        3 => r.take(4)? == CLASSIC,
        1 => true,
        _ => return Some(Err(VERSION.to_string())),
    };
    let units = r.u32()? as usize;
    r.take(units.checked_mul(2)?)?;
    let count = usize::from(r.u16()?);
    if !(2..=32).contains(&count) {
        return Some(Err(STOPS.to_string()));
    }
    let mut stops = Vec::with_capacity(count);
    for _ in 0..count {
        let location = r.u32()?;
        let midpoint = r.u32()?;
        let space = r.u16()?;
        let components = [r.u16()?, r.u16()?, r.u16()?, r.u16()?];
        let kind = r.u16()?;
        if (midpoint != MIDPOINT && !classic_method)
            || space != CMYK_SPACE
            || kind != 0
            || location > 4096
        {
            return Some(Err(STOP.to_string()));
        }
        let ink = components.map(|c| q4(1.0 - f64::from(c) / 65535.0));
        stops.push(Decoded {
            location,
            ink,
            opacity: 255,
            midpoint,
            opacity_midpoint: MIDPOINT,
        });
    }
    let transparent = usize::from(r.u16()?);
    let mut opacities = Vec::with_capacity(transparent);
    for _ in 0..transparent {
        let location = r.u32()?;
        let midpoint = r.u32()?;
        let opacity = r.u16()?;
        opacities.push((location, midpoint, opacity));
    }
    let opaque = opacities.iter().all(|(_, _, opacity)| *opacity >= 255);
    let matched = opacities.len() == stops.len()
        && opacities
            .iter()
            .zip(&stops)
            .all(|((a, _, _), s)| *a == s.location);
    if !opaque && !matched {
        return Some(Err(TRANSPARENCY.to_string()));
    }
    if !opaque {
        for (stop, (_, midpoint, opacity)) in stops.iter_mut().zip(&opacities) {
            stop.opacity = (*opacity).min(255);
            stop.opacity_midpoint = *midpoint;
        }
    }
    // The tail's interpolation word (module doc); a body that ends before it
    // reads 0, the legacy map today's reader gave it.
    let interpolation = r.u16().and_then(|_| r.u16()).unwrap_or(0);
    let legacy = !classic_method
        || (interpolation == 0
            && stops.iter().all(|s| s.midpoint == MIDPOINT)
            && opacities.iter().all(|(_, m, _)| *m == MIDPOINT));
    let opacity = |s: &Decoded| {
        if opaque {
            1.0
        } else {
            q4(f64::from(s.opacity) / 255.0)
        }
    };
    let mut params = Map::new();
    if legacy {
        let list: Vec<Value> = stops
            .iter()
            .map(|s| {
                json!({"position": q4(f64::from(s.location) / LOCATION), "ink": s.ink,
                       "opacity": opacity(s)})
            })
            .collect();
        params.insert(
            "gradient".into(),
            json!({"stops": list, "reverse": reversed}),
        );
    } else {
        if interpolation > SMOOTH {
            return Some(Err(SMOOTHNESS.to_string()));
        }
        // Photoshop stop i + 1's `Mdpn` is our stop i's midpoint, in
        // POSITION order (module doc); stop 0's governs nothing.
        stops.sort_by_key(|s| s.location);
        let mut list = Vec::with_capacity(stops.len());
        for (i, s) in stops.iter().enumerate() {
            let mut stop = json!({"position": q4(f64::from(s.location) / LOCATION),
                                  "ink": s.ink, "opacity": opacity(s)});
            if let Some(next) = stops.get(i + 1) {
                let pairs = [
                    ("midpoint", next.midpoint),
                    ("opacity_midpoint", next.opacity_midpoint),
                ];
                for (key, mdpn) in pairs {
                    if !MIDPOINT_RANGE.contains(&mdpn) {
                        return Some(Err(MIDPOINTS.to_string()));
                    }
                    if mdpn != MIDPOINT && (key == "midpoint" || !opaque) {
                        stop[key] = json!(f64::from(mdpn) / 100.0);
                    }
                }
            }
            list.push(stop);
        }
        let mut gradient = json!({"stops": list, "reverse": reversed});
        if interpolation != SMOOTH {
            gradient["smoothness"] = json!(f64::from(interpolation) / f64::from(SMOOTH));
        }
        params.insert("gradient".into(), gradient);
        params.insert("method".into(), json!("classic"));
    }
    params.insert("dither".into(), json!(dithered));
    Some(Ok(params))
}

fn push_u16(out: &mut Vec<u8>, v: u16) {
    out.extend_from_slice(&v.to_be_bytes());
}

fn push_u32(out: &mut Vec<u8>, v: u32) {
    out.extend_from_slice(&v.to_be_bytes());
}

/// The `grdm` body for CMYK `gradient_map` params (module doc), always a
/// multiple of 4 long; `None` when the stops do not parse.
pub(crate) fn encode(params: &Map<String, Value>) -> Option<Vec<u8>> {
    let gradient = params.get("gradient")?.as_object()?;
    let reverse = boolean(gradient, "reverse", false)?;
    let dither = boolean(params, "dither", false)?;
    let classic = params.get("method").and_then(Value::as_str) == Some("classic");
    let smoothness = gradient
        .get("smoothness")
        .map_or(Some(1.0), Value::as_f64)?;
    let listed = gradient.get("stops")?.as_array()?;
    if !(2..=32).contains(&listed.len()) {
        return None;
    }
    let mut stops = Vec::with_capacity(listed.len());
    for stop in listed {
        let stop = stop.as_object()?;
        let position = stop.get("position").map_or(Some(0.0), Value::as_f64)?;
        let opacity = stop.get("opacity").map_or(Some(1.0), Value::as_f64)?;
        let midpoint = |key: &str| stop.get(key).map_or(Some(0.5), Value::as_f64);
        let midpoints = [midpoint("midpoint")?, midpoint("opacity_midpoint")?];
        let ink = stop.get("ink")?.as_array()?;
        if ink.len() != 4 {
            return None;
        }
        let ink: Vec<f64> = ink.iter().map(Value::as_f64).collect::<Option<_>>()?;
        let location = (position.clamp(0.0, 1.0) * LOCATION).round() as u32;
        stops.push((location, ink, opacity.clamp(0.0, 1.0), midpoints));
    }
    // A stable sort by position, as the map itself orders its stops; each
    // stop carries its own midpoints through it.
    stops.sort_by_key(|(location, _, _, _)| *location);
    // Photoshop stop i + 1 carries our stop i's midpoint (module doc); a
    // legacy map writes 50 everywhere.
    let mdpn = |i: usize, which: usize| -> u32 {
        match i.checked_sub(1) {
            Some(ours) if classic => {
                (stops[ours].3[which].clamp(0.05, 0.95) * 100.0).round() as u32
            }
            _ => MIDPOINT,
        }
    };
    let mut out = Vec::with_capacity(160);
    push_u16(&mut out, 3);
    out.push(u8::from(reverse));
    out.push(u8::from(dither));
    out.extend_from_slice(CLASSIC);
    let name: Vec<u16> = "Custom\0".encode_utf16().collect();
    push_u32(&mut out, name.len() as u32);
    for unit in name {
        push_u16(&mut out, unit);
    }
    push_u16(&mut out, stops.len() as u16);
    for (i, (location, ink, _, _)) in stops.iter().enumerate() {
        push_u32(&mut out, *location);
        push_u32(&mut out, mdpn(i, 0));
        push_u16(&mut out, CMYK_SPACE);
        for v in ink {
            push_u16(
                &mut out,
                ((1.0 - v.clamp(0.0, 1.0)) * 65535.0).round() as u16,
            );
        }
        push_u16(&mut out, 0);
    }
    let byte = |opacity: f64| (opacity * 255.0).round() as u16;
    let transparency: Vec<(u32, u32, u16)> = if stops.iter().all(|(_, _, o, _)| byte(*o) == 255) {
        vec![(0, MIDPOINT, 255), (LOCATION as u32, MIDPOINT, 255)]
    } else {
        stops
            .iter()
            .enumerate()
            .map(|(i, (l, _, o, _))| (*l, mdpn(i, 1), byte(*o)))
            .collect()
    };
    push_u16(&mut out, transparency.len() as u16);
    for (location, midpoint, opacity) in &transparency {
        push_u32(&mut out, *location);
        push_u32(&mut out, *midpoint);
        push_u16(&mut out, *opacity);
    }
    // Photoshop's own tail (module doc); the interpolation word is the
    // Smoothness, 0 for a legacy (linear) map.
    push_u16(&mut out, 2);
    push_u16(
        &mut out,
        if classic {
            (smoothness.clamp(0.0, 1.0) * f64::from(SMOOTH)).round() as u16
        } else {
            0
        },
    );
    push_u16(&mut out, 32);
    push_u16(&mut out, 0);
    push_u32(&mut out, SEED);
    push_u16(&mut out, 0);
    push_u16(&mut out, 0);
    push_u32(&mut out, 2048);
    push_u16(&mut out, 3);
    for _ in 0..4 {
        push_u16(&mut out, 0);
    }
    for _ in 0..4 {
        push_u16(&mut out, 32768);
    }
    push_u16(&mut out, 0);
    while !out.len().is_multiple_of(4) {
        out.push(0);
    }
    Some(out)
}
