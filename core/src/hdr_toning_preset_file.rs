//! HDR Toning PRESET files (`.hdt`) from Photoshop, decoded to `hdr_toning`'s
//! Local Adaptation arguments (the request `hdr_local_adaptation`'s module
//! doc states) for the user preset library.
//!
//! # The layout
//!
//! The format is unpublished, so this reading is best effort, from the sixteen
//! sample files an installed Photoshop ships: each parses exactly, with
//! nothing left over. Big-endian throughout:
//!
//! 1. `hdrt`, u32 3, f32 strength, u32 2;
//! 2. u32 n (1…256), then n UTF-16 units of a name. Every sample stores
//!    `Default` and a NUL, so the name is skipped: the host names the preset
//!    after the FILE;
//! 3. u16 2, u16 count (the request's 2…16), count × (u16 output, u16 input),
//!    the Toning Curve, then count × u8 point flags (0 or 1);
//! 4. eight zero bytes, f32 radius, u32 method (3, Local Adaptation, in every
//!    sample; any other refuses by its number), two f32 that are not read;
//! 5. `hdra`, u32 6, f32 exposure, saturation, detail, shadow, highlight,
//!    gamma and vibrance, u8 smooth_edges (0 or 1), one u8 that is not read.
//!
//! A file that does not parse exactly is refused whole: each fixed field
//! names itself and its stored value, a short file names the field it ends
//! in, bytes after the last field are counted, and every f32 must be finite.
//! Files are capped at [`MAX_FILE_BYTES`] (a 16-point file with a 256-unit
//! name is under 700 bytes), checked before the bytes are read.
//!
//! # Two inferences
//!
//! ([#66](https://github.com/kpgalligan/doctored/issues/66) asks for the
//! take that confirms both.) The samples cannot settle two things, so each is
//! read by the rule the crate's other files from the same application follow,
//! and each is one function or constant here, so a recorded take that decides
//! otherwise changes one line:
//!
//! * The curve pair is (output, input) ([`curve_point`]), the order of the
//!   `.acv` Curves preset (`adjustment_preset_file`). Every sample curve is
//!   monotone either way; read this way, the two "High Contrast" samples
//!   are S-curves.
//! * A point flag of 1 is a smooth point and 0 a corner ([`CORNER_POINT`]),
//!   the `Cnty` (continuity) rule of the curve points in its descriptors
//!   (`psd_contour`): every sample's identity curve, and every endpoint,
//!   carries 1.
//!
//! # What the import changes
//!
//! The answer is the arguments and a report, one line for each value the
//! arguments do not carry as stored:
//!
//! * A gamma above the request's [`MAX_GAMMA`] (2) becomes 2, and the line
//!   names the stored value: the measured take `hdr_local_adaptation`'s module
//!   doc records rendered a stored 3.25 exactly as 2. Nothing else is clamped.
//!   Every other range is the request's own (`LocalAdaptation::parse`, the one
//!   implementation), and its sentence is the refusal.
//! * Corner points are counted, because Doctored's Toning Curve is smooth
//!   through every point.
//!
//! A stored f32 becomes the f64 of its shortest decimal, so a value entered
//! as 1.15 reads 1.15, not the f32's 1.1499999761….

use serde_json::{json, Map, Value};

use crate::hdr_local_adaptation::{LocalAdaptation, MAX_GAMMA, TONING_CURVE_POINTS};

/// The largest preset file accepted, in bytes.
pub(crate) const MAX_FILE_BYTES: usize = 4096;

/// The longest stored name, in UTF-16 units.
const MAX_NAME_UNITS: u32 = 256;

/// The method word of Local Adaptation, the only method read.
const LOCAL_ADAPTATION: u32 = 3;

/// The point flag of a corner (module doc, "Two inferences"); the other
/// accepted value is a smooth point.
const CORNER_POINT: u8 = 0;
const SMOOTH_POINT: u8 = 1;

/// The refusal for a preset file of `len` bytes over `MAX_FILE_BYTES`.
pub(crate) fn too_large(len: usize) -> String {
    format!("hdr toning preset file is {len} bytes; the limit is {MAX_FILE_BYTES}")
}

/// A decoded file: `hdr_toning`'s arguments and the report (module doc).
pub(crate) struct Decoded {
    pub(crate) arguments: Map<String, Value>,
    pub(crate) report: Vec<String>,
}

/// One stored curve pair as the request's `[in, out]`: the file stores the
/// output first (module doc, "Two inferences").
fn curve_point(first: u16, second: u16) -> Value {
    json!([second, first])
}

/// A cursor over the file; every read names its field for the truncation
/// refusal.
struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> Reader<'a> {
    fn take(&mut self, len: usize, field: &str) -> Result<&'a [u8], String> {
        let bytes = self.bytes;
        let end = self
            .at
            .checked_add(len)
            .filter(|end| *end <= bytes.len())
            .ok_or_else(|| format!("hdr toning preset is truncated at its {field}"))?;
        let taken = &bytes[self.at..end];
        self.at = end;
        Ok(taken)
    }

    fn array<const N: usize>(&mut self, field: &str) -> Result<[u8; N], String> {
        let mut out = [0u8; N];
        out.copy_from_slice(self.take(N, field)?);
        Ok(out)
    }

    fn u8(&mut self, field: &str) -> Result<u8, String> {
        Ok(self.array::<1>(field)?[0])
    }

    fn u16(&mut self, field: &str) -> Result<u16, String> {
        Ok(u16::from_be_bytes(self.array(field)?))
    }

    fn u32(&mut self, field: &str) -> Result<u32, String> {
        Ok(u32::from_be_bytes(self.array(field)?))
    }

    /// A finite f32 as the f64 of its shortest decimal (module doc).
    fn value(&mut self, field: &str) -> Result<f64, String> {
        let stored = f32::from_be_bytes(self.array(field)?);
        if !stored.is_finite() {
            return Err(format!("hdr toning preset {field} is not a finite number"));
        }
        Ok(stored.to_string().parse().unwrap_or(f64::from(stored)))
    }

    /// A four-byte signature that must be `expected`.
    fn signature(&mut self, expected: &[u8; 4], field: &str) -> Result<(), String> {
        let stored: [u8; 4] = self.array(field)?;
        if &stored != expected {
            return Err(format!(
                "hdr toning preset {field} is \"{}\"; expected \"{}\"",
                stored.escape_ascii(),
                expected.escape_ascii()
            ));
        }
        Ok(())
    }

    /// A u32 that must be `expected`.
    fn word(&mut self, expected: u32, field: &str) -> Result<(), String> {
        let stored = self.u32(field)?;
        if stored != expected {
            return Err(format!(
                "hdr toning preset {field} {stored} is not {expected}"
            ));
        }
        Ok(())
    }
}

/// Decodes a `.hdt` file's `bytes` (module doc). `Err` is a lowercase
/// sentence naming why the file cannot be used. Never panics.
pub(crate) fn decode(bytes: &[u8]) -> Result<Decoded, String> {
    if bytes.len() > MAX_FILE_BYTES {
        return Err(too_large(bytes.len()));
    }
    let mut file = Reader { bytes, at: 0 };
    file.signature(b"hdrt", "signature")?;
    file.word(3, "version")?;
    let strength = file.value("strength")?;
    file.word(2, "header word")?;
    let units = file.u32("name length")?;
    if !(1..=MAX_NAME_UNITS).contains(&units) {
        return Err(format!(
            "hdr toning preset name holds {units} characters; expected 1 to {MAX_NAME_UNITS}"
        ));
    }
    file.take(units as usize * 2, "name")?;
    let head = file.u16("curve header")?;
    if head != 2 {
        return Err(format!("hdr toning preset curve header {head} is not 2"));
    }
    let count = usize::from(file.u16("curve point count")?);
    if !TONING_CURVE_POINTS.contains(&count) {
        return Err(format!(
            "hdr toning preset toning curve holds {count} points; expected {} to {}",
            TONING_CURVE_POINTS.start(),
            TONING_CURVE_POINTS.end()
        ));
    }
    let mut curve = Vec::with_capacity(count);
    for _ in 0..count {
        let first = file.u16("toning curve")?;
        let second = file.u16("toning curve")?;
        curve.push(curve_point(first, second));
    }
    let mut corners = 0;
    for point in 1..=count {
        match file.u8("corner flags")? {
            CORNER_POINT => corners += 1,
            SMOOTH_POINT => {}
            other => {
                return Err(format!(
                    "hdr toning preset corner flag of point {point} is {other}; expected 0 or 1"
                ))
            }
        }
    }
    if file.take(8, "padding")?.iter().any(|byte| *byte != 0) {
        return Err("hdr toning preset padding after the corner flags is not zero".into());
    }
    let radius = file.value("radius")?;
    let method = file.u32("method")?;
    if method != LOCAL_ADAPTATION {
        return Err(format!(
            "hdr toning preset method {method} is not supported: only local adaptation \
             ({LOCAL_ADAPTATION}) is read"
        ));
    }
    file.take(8, "unread values")?;
    file.signature(b"hdra", "local adaptation signature")?;
    file.word(6, "local adaptation version")?;
    let exposure = file.value("exposure")?;
    let saturation = file.value("saturation")?;
    let detail = file.value("detail")?;
    let shadow = file.value("shadow")?;
    let highlight = file.value("highlight")?;
    let stored_gamma = file.value("gamma")?;
    let vibrance = file.value("vibrance")?;
    let smooth_edges = match file.u8("smooth_edges")? {
        0 => false,
        1 => true,
        other => {
            return Err(format!(
                "hdr toning preset smooth_edges {other} is not 0 or 1"
            ))
        }
    };
    file.take(1, "last byte")?;
    let left = bytes.len() - file.at;
    if left > 0 {
        return Err(format!(
            "hdr toning preset has {left} bytes after its last field"
        ));
    }

    let mut report = Vec::new();
    let gamma = if stored_gamma > MAX_GAMMA {
        report.push(format!(
            "gamma {stored_gamma} is above the maximum {MAX_GAMMA}; imported as {MAX_GAMMA}"
        ));
        MAX_GAMMA
    } else {
        stored_gamma
    };
    if corners == 1 {
        report.push(format!(
            "1 of the toning curve's {count} points is a corner; imported as a smooth point"
        ));
    } else if corners > 1 {
        report.push(format!(
            "{corners} of the toning curve's {count} points are corners; imported as smooth \
             points"
        ));
    }
    let arguments: Map<String, Value> = [
        ("method", json!("local_adaptation")),
        ("radius", json!(radius)),
        ("strength", json!(strength)),
        ("smooth_edges", json!(smooth_edges)),
        ("gamma", json!(gamma)),
        ("exposure", json!(exposure)),
        ("detail", json!(detail)),
        ("shadow", json!(shadow)),
        ("highlight", json!(highlight)),
        ("vibrance", json!(vibrance)),
        ("saturation", json!(saturation)),
        ("toning_curve", Value::Array(curve)),
    ]
    .into_iter()
    .map(|(key, value)| (key.to_string(), value))
    .collect();
    LocalAdaptation::parse(&arguments)
        .map_err(|reason| format!("this hdr toning preset cannot be used: {reason}"))?;
    Ok(Decoded { arguments, report })
}
