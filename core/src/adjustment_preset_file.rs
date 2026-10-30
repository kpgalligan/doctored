//! Photoshop's adjustment PRESET files: `.acv` Curves, `.alv`
//! Levels, `.ahu` Hue/Saturation, `.eap` Exposure, `.blw` Black & White and
//! `.cha` Channel Mixer, decoded to the `{op, params}` an adjustment
//! layer stores.
//!
//! There is no second codec here. Five of the six files ARE the body of the
//! PSD adjustment record of the same kind — `.alv` is a `levl` body, `.ahu` a
//! `hue2`, `.eap` an `expA`, `.blw` a `blwh` descriptor body and `.cha` a
//! 44-byte `mixr` body (an RGB one; a CMYK preset is declined) — so they go
//! to `psd_adjustment::decode` verbatim, and its every rule, range check and
//! decline reason applies unchanged. `.acv` is the one reframe: the file is
//! `u16 version (1|4), u16 count (1…32)` then `count` curves of `u16 n` and
//! `n × (u16 output, u16 input)`, which is a `curv` body without the leading
//! `is_map` byte and with a count where `curv` has a channel bitmask. The
//! first `min(count, 4)` curves are the composite and the three RGB channels
//! in `curv`'s bit order, so the reframe prefixes `is_map = 0`, keeps the
//! version, writes the mask `(1 << min(count, 4)) − 1`, copies those curves
//! byte for byte and appends `curv`'s one zero pad. A curve past the fourth
//! belongs to a CMYK or multichannel preset; it is accepted only when it is
//! exactly the two-point identity (`(0,0)`, `(255,255)`, which is what an RGB
//! preset carries in the unused slot), because anything else is a channel
//! Doctored cannot apply. Curve numbers in messages are 1-based, the order
//! they sit in the file. Bytes after the last curve are ignored, like a
//! record's tail.
//!
//! Two host-facing adjustments to the decoded params: `evaluation` is
//! removed, so an `.eap` does not pin a colour evaluation — the caller's
//! `add_adjustment_layer` default (or the dialog's) decides it, exactly as
//! for an Exposure layer created by hand; and a `.blw` `tint_color` is kept
//! verbatim, PSD import's `blwh` rule. Files are capped at `MAX_FILE_BYTES`
//! (64 KiB — the largest shipped preset, a `.blw`, is a few hundred bytes).
//!
//! `.alv`/`.ahu`/`.eap`/`.blw`/`.cha` are the `levl`/`hue2`/`expA`/`blwh`/`mixr` record bodies and
//! go to `psd_adjustment::decode` verbatim (a CMYK `.cha` declines) (its rules and decline reasons,
//! phrased "this Photoshop {kind} preset cannot be used: its {reason}"); `.acv` is reframed onto
//! `curv` (first `min(count, 4)` curves as composite/R/G/B, a curve past the fourth must be the
//! two-point identity, so a CMYK preset refuses). `evaluation` is removed from the params so the
//! caller's default decides it; files are capped at 64 KiB, checked before the bytes are read; no
//! Adobe file is committed (the one `#[ignore]` test reads an installed Photoshop's shipped
//! presets).

use serde_json::{Map, Value};

use crate::psd_adjustment::{self, AdjustmentBody};

/// The largest preset file accepted, in bytes.
pub(crate) const MAX_FILE_BYTES: usize = 65_536;

/// File extension (lowercase, no dot) → the PSD record key its body is.
const EXTENSIONS: [(&str, [u8; 4]); 6] = [
    ("acv", *b"curv"),
    ("alv", *b"levl"),
    ("ahu", *b"hue2"),
    ("eap", *b"expA"),
    ("blw", *b"blwh"),
    ("cha", *b"mixr"),
];

/// The extensions `EXTENSIONS` accepts, as the unsupported-extension
/// refusal names them.
const EXPECTED: &str = "expected acv, alv, ahu, eap, blw or cha";

/// The most curves an `.acv` may hold (Photoshop's multichannel ceiling is
/// far below this; the bound only keeps a hostile count from walking far).
pub(crate) const MAX_CURVES: u16 = 32;

/// The curves `curv`'s channel bitmask can name: composite, red, green, blue.
const RGB_CURVES: u16 = 4;

/// The only curve accepted past the fourth: two points, (output 0, input 0)
/// and (output 255, input 255), big-endian.
const IDENTITY_POINTS: [u8; 8] = [0, 0, 0, 0, 0, 255, 0, 255];

pub(crate) const TRUNCATED: &str = "curves preset is truncated";

/// The refusal for a preset file of `len` bytes over `MAX_FILE_BYTES`.
pub(crate) fn too_large(len: usize) -> String {
    format!("preset file is {len} bytes; the limit is {MAX_FILE_BYTES}")
}

/// Decodes a preset file's `bytes` by its `extension` (no dot, any case) to
/// the Doctored op and the params an adjustment layer of that op stores.
/// `Err` is a lowercase sentence naming why the file cannot be used. Never
/// panics.
pub(crate) fn decode(
    extension: &str,
    bytes: &[u8],
) -> Result<(&'static str, Map<String, Value>), String> {
    if bytes.len() > MAX_FILE_BYTES {
        return Err(too_large(bytes.len()));
    }
    let lower = extension.to_ascii_lowercase();
    let Some((_, key)) = EXTENSIONS.iter().find(|(candidate, _)| *candidate == lower) else {
        return Err(format!(
            "unsupported preset extension \"{extension}\"; {EXPECTED}"
        ));
    };
    let reframed;
    let body = if key == b"curv" {
        reframed = curves_body(bytes)?;
        reframed.as_slice()
    } else {
        bytes
    };
    match psd_adjustment::decode(*key, body, None) {
        AdjustmentBody::Carried(carried) => {
            let op = carried.op();
            let meta: Value = serde_json::from_str(&carried.meta)
                .map_err(|e| format!("decoded preset meta did not parse: {e}"))?;
            let mut params = match meta.get("params") {
                Some(Value::Object(params)) => params.clone(),
                _ => Map::new(),
            };
            params.remove("evaluation");
            Ok((op, params))
        }
        AdjustmentBody::Declined { kind, reason, .. } => Err(format!(
            "this Photoshop {kind} preset cannot be used: its {reason}"
        )),
        // Every key in EXTENSIONS is one of the codec's twelve.
        AdjustmentBody::Absent => Err(format!(
            "unsupported preset extension \"{extension}\"; {EXPECTED}"
        )),
    }
}

/// The big-endian u16 at `at`, or the truncation refusal.
pub(crate) fn u16_at(bytes: &[u8], at: usize) -> Result<u16, String> {
    let pair = bytes
        .get(at..at.checked_add(2).ok_or(TRUNCATED)?)
        .ok_or(TRUNCATED)?;
    Ok(u16::from_be_bytes([pair[0], pair[1]]))
}

/// The `curv` record body an `.acv` file's curves reframe to (module doc).
fn curves_body(bytes: &[u8]) -> Result<Vec<u8>, String> {
    let version = u16_at(bytes, 0)?;
    let count = u16_at(bytes, 2)?;
    if !matches!(version, 1 | 4) {
        return Err(format!("curves preset version {version} is not 1 or 4"));
    }
    if !(1..=MAX_CURVES).contains(&count) {
        return Err(format!(
            "curves preset holds {count} curves; expected 1 to {MAX_CURVES}"
        ));
    }
    let kept = count.min(RGB_CURVES);
    let mut at = 4usize;
    let mut kept_end = at;
    for index in 0..count {
        let points = usize::from(u16_at(bytes, at)?);
        let start = at + 2;
        let end = start + points * 4;
        let curve = bytes.get(start..end).ok_or(TRUNCATED)?;
        if index >= RGB_CURVES && (points != 2 || curve != IDENTITY_POINTS) {
            return Err(format!(
                "curves preset curve {} is not identity; only RGB presets (at most four \
                 curves) are supported",
                index + 1
            ));
        }
        at = end;
        if index + 1 == kept {
            kept_end = at;
        }
    }
    let mut body = Vec::with_capacity(kept_end + 4);
    body.push(0);
    body.extend_from_slice(&version.to_be_bytes());
    body.extend_from_slice(&((1u32 << kept) - 1).to_be_bytes());
    body.extend_from_slice(&bytes[4..kept_end]);
    body.push(0);
    Ok(body)
}
