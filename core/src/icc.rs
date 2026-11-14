//! ICC profile bytes in, an [`IccProfile`] out: the 128-byte header, the tag
//! table, the tags a matrix/TRC RGB profile is made of, and the five-way
//! [`inspect`] a host needs to explain a refusal. The numerics live in
//! `icc_transform`; the four profiles this build writes live in
//! `icc_builtin`.
//!
//! # What this parser models, and what it refuses
//!
//! ICC **v2 and v4 RGB matrix/TRC** profiles — sRGB, Display P3, Adobe RGB
//! (1998), ProPhoto, Rec. 2020 and every camera-maker RGB profile a
//! photographer's JPEG, PNG or HEIC actually carries. A LUT-based (A2B/B2A)
//! RGB profile is **kept as the document's profile but not transformable**:
//! its pixels are correct, CoreGraphics displays it correctly and an export
//! re-embeds it byte-for-byte; only OUR conversion is unavailable, which is
//! the single bit `model().is_none()`. A Gray, CMYK or Lab profile is
//! refused outright and never stored, because a document's pixels are RGB —
//! and so is an RGB profile whose CLASS is a device link or an abstract
//! transform, which describes a transform, or a named-colour list, which is
//! a list of colours, rather than the space a picture's numbers live in (see
//! [`pixel_space_refusal`]).
//!
//! # The PCS white rule
//!
//! For a matrix/TRC profile the PCS white is **always D50** and the
//! `rXYZ`/`gXYZ`/`bXYZ` tags are already D50-adapted; `wtpt` and `chad` are
//! advisory and this parser uses them for nothing. macOS's shipped
//! `sRGB Profile.icc` is the standing proof: D50-adapted columns, NO `chad`
//! tag at all, and a `wtpt` holding the unadapted D65 (0.950455, 1.0,
//! 1.089050). A reader that treats `wtpt` as the PCS white gets sRGB
//! visibly wrong. See `MatrixTrc` for the same note beside the numbers.
//!
//! # Robustness
//!
//! No input may panic and no malformed profile may be half-believed. Every
//! rule below fails a TAG (making it absent) or the PROFILE (returning
//! `None` / `Inspect::NotIcc`), never an index out of bounds: the tag table
//! is bounds-checked before it is walked, a tag's offset must be 4-aligned
//! and its extent inside the profile, and every length is checked before a
//! slice is taken. Tag data is NOT required to be disjoint — sharing is
//! normal, and both Apple profiles point all three TRC tags at one blob.
//!
//! the SIX-way inspect (a profile whose CLASS is a device link, an abstract transform or a
//! named-colour list is refused with Gray/CMYK/Lab, not kept; `RgbLut` is the Phase 11C class — no
//! matrix model, readable A2B/B2A tables — and is STORED like `RgbUnconvertible`).
//! `IccProfile::model()` is and stays the MATRIX/TRC question — `document_v2` Exposure/White
//! Balance, `adopt_working_space` and every existing gate read it — while
//! `can_decode()`/`can_encode()` are the wider Phase 11C ones; `IccBytes` is the shared header/tag
//! view `icc_lut` reads Gray and CMYK profiles through. `describe_json` (`dr_icc_describe`) also
//! answers `proof_refusal` (null exactly when `proofable`; `ProofProfile::proof_refusal` or the
//! parser's sentence) and `high_depth_destination` (`adjust_calibrated::precision_model`'s
//! answer, asked only when a matrix model was read).

use std::sync::{Arc, OnceLock};

use crate::drdc::MAX_DRDC_BLOB_LEN;
use crate::icc_builtin::{self, Builtin};
use crate::icc_lut::ProofProfile;
use crate::icc_transform::{Curve, MatrixTrc};

/// The LUT half of a profile's model (Phase 11C): the parsed A2B/B2A tables
/// an RGB profile carries instead of — or as well as — the matrix/TRC tags.
/// The type is `icc_lut`'s, aliased here so `IccProfile`'s two models read
/// as a pair.
pub type LutModel = ProofProfile;

/// Longest profile name kept, in characters. A name is chrome — a status
/// bar, a popup, one MCP field — and a profile with a kilobyte of `desc` is
/// not going to be described by all of it.
const MAX_NAME_CHARS: usize = 128;

/// What a profile with no readable name is called.
const UNTITLED: &str = "Untitled profile";

/// A colour profile a document can carry: the canonical bytes (embedded on
/// export, handed to `CGColorSpace(iccData:)` by the host), the display
/// name, and — when the profile is one we can convert through — the parsed
/// matrix/TRC model.
pub struct IccProfile {
    bytes: Arc<[u8]>,
    name: String,
    model: Option<MatrixTrc>,
    /// The parsed A2B/B2A tables, present for an RGB profile that carries
    /// usable ones (Phase 11C). A profile may have BOTH: the matrix/TRC
    /// model still decides every existing conversion, byte for byte, and the
    /// LUT is read only by the paths that ask for it.
    lut: Option<Arc<LutModel>>,
}

impl std::fmt::Debug for IccProfile {
    /// Names the profile rather than dumping two kilobytes of it.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("IccProfile")
            .field("name", &self.name)
            .field("bytes", &self.bytes.len())
            .field("convertible", &self.model.is_some())
            .finish()
    }
}

/// What raw bytes turn out to be. Each value drives different host copy and
/// a different set of enabled commands, which is why this is five values and
/// not "a profile or an error".
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Inspect {
    /// Not an ICC profile at all: no `acsp` magic, a size field outside the
    /// buffer, or a tag table that does not fit.
    NotIcc,
    /// An ICC profile whose data colour space is not `'RGB '` (Gray, CMYK,
    /// Lab). Never stored on a document — its numbers are not what a
    /// document's pixels hold.
    NotRgb,
    /// An RGB profile whose CLASS describes something other than pixels: a
    /// device link, an abstract transform, or a named-colour list (macOS's
    /// own `WebSafeColors.icc`). Never stored either — see
    /// [`pixel_space_refusal`] for what accepting one did.
    NotImageProfile,
    /// An RGB profile this crate cannot transform with: a LUT-based
    /// (A2B/B2A) profile, or one missing or corrupting any of the six
    /// matrix/TRC tags. Kept as the document's profile; only Convert
    /// refuses.
    RgbUnconvertible,
    /// An RGB matrix/TRC profile: full function.
    RgbMatrix,
    /// An RGB profile with no matrix/TRC model but WITH usable A2B/B2A
    /// tables (Phase 11C). Kept as the document's profile like
    /// [`Inspect::RgbUnconvertible`], and `model()` is still absent — so
    /// every matrix/TRC gate keeps its meaning — but Convert to Profile and
    /// soft proofing can read it, which is the bit
    /// [`IccProfile::can_decode`] reports.
    RgbLut,
}

impl IccProfile {
    /// The profile's shared bytes and decoded table/string payloads.
    pub(crate) fn record_resources(&self, inventory: &mut crate::resources::Inventory) {
        self.record_resources_with_cache(inventory, false);
    }
    pub(crate) fn record_resources_with_cache(
        &self,
        inventory: &mut crate::resources::Inventory,
        cache: bool,
    ) {
        inventory.record(
            self.bytes.as_ptr() as usize,
            self.bytes.len(),
            "profile",
            cache,
        );
        inventory.string(&self.name, "profile", cache);
        if let Some(model) = &self.model {
            model.record_resources_with_cache(inventory, cache);
        }
    }

    /// The built-in sRGB IEC61966-2.1 profile, built once. Byte-identical on
    /// every call and every run — the creation date is a constant.
    pub fn srgb() -> Arc<IccProfile> {
        static SRGB: OnceLock<Arc<IccProfile>> = OnceLock::new();
        SRGB.get_or_init(|| Arc::new(builtin(Builtin::Srgb)))
            .clone()
    }

    /// The built-in Display P3 profile, on the same terms as [`IccProfile::srgb`].
    pub fn display_p3() -> Arc<IccProfile> {
        static P3: OnceLock<Arc<IccProfile>> = OnceLock::new();
        P3.get_or_init(|| Arc::new(builtin(Builtin::DisplayP3)))
            .clone()
    }

    /// The built-in Adobe RGB (1998) profile, on the same terms as
    /// [`IccProfile::srgb`]: built by this crate from the published
    /// primaries and the 563/256 gamma, never read from a file.
    pub fn adobe_rgb() -> Arc<IccProfile> {
        static ADOBE: OnceLock<Arc<IccProfile>> = OnceLock::new();
        ADOBE
            .get_or_init(|| Arc::new(builtin(Builtin::AdobeRgb)))
            .clone()
    }

    /// The built-in ProPhoto RGB profile, on the same terms as
    /// [`IccProfile::srgb`]: built by this crate from the published
    /// primaries and the 461/256 gamma. It is ProPhoto RGB at gamma 1.8, a
    /// pure power, not the ROMM RGB encoding with its linear toe
    /// (`icc_builtin`'s module doc).
    pub fn prophoto_rgb() -> Arc<IccProfile> {
        static PROPHOTO: OnceLock<Arc<IccProfile>> = OnceLock::new();
        PROPHOTO
            .get_or_init(|| Arc::new(builtin(Builtin::ProPhotoRgb)))
            .clone()
    }

    /// One of the four built-ins by its FFI discriminant; `None` for a value
    /// outside the enum.
    pub fn builtin_from_c(value: i32) -> Option<Arc<IccProfile>> {
        Some(IccProfile::of_builtin(Builtin::from_c(value)?))
    }

    /// The shared profile of one of the four built-ins.
    pub(crate) fn of_builtin(which: Builtin) -> Arc<IccProfile> {
        match which {
            Builtin::Srgb => IccProfile::srgb(),
            Builtin::DisplayP3 => IccProfile::display_p3(),
            Builtin::AdobeRgb => IccProfile::adobe_rgb(),
            Builtin::ProPhotoRgb => IccProfile::prophoto_rgb(),
        }
    }

    /// Parses profile bytes. `None` when they are not an ICC profile, not a
    /// profile a picture's pixels can be interpreted in ([`inspect`]'s
    /// `NotRgb` and `NotImageProfile`), or larger than a document can carry —
    /// the cases a document must never store. An RGB profile this crate
    /// cannot model still parses, with `model()` absent.
    ///
    /// A profile **is its declared size**: only `bytes[..size]` is kept, so
    /// trailing padding (a PNG `iCCP` whose zlib stream inflates to megabytes
    /// of zeros past a 3 KB profile is the case that made this matter) is
    /// neither stored nor re-embedded on export.
    ///
    /// This is also the ONE place the [`MAX_DRDC_BLOB_LEN`] cap is enforced
    /// for a profile, deliberately: every way a profile enters a document —
    /// the FFI setters, the DRDC reader, and `DrDocument::open`, which lifts
    /// whatever a decoder inflated out of a file — goes through here, so no
    /// caller can produce a document the native writer would then refuse to
    /// save.
    pub fn parse(bytes: &[u8]) -> Option<IccProfile> {
        let end = header_end(bytes)?;
        if end > MAX_DRDC_BLOB_LEN as usize {
            return None;
        }
        if pixel_space_refusal(bytes).is_some() {
            return None;
        }
        let bytes = &bytes[..end];
        let model = read_model(bytes, end);
        // The LUT half is parsed only when the matrix/TRC half is absent:
        // a profile that HAS the six tags converts through the shipped
        // matrix path, byte for byte, and a second model would be a second
        // answer to one question. Tables that cannot be parsed simply leave
        // the profile unconvertible, exactly as before this phase.
        let lut = match model {
            Some(_) => None,
            None => ProofProfile::parse(bytes).ok().map(Arc::new),
        };
        Some(IccProfile {
            bytes: Arc::from(bytes.to_vec()),
            name: read_name(bytes, end),
            model,
            lut,
        })
    }

    /// The canonical bytes: what an export embeds and what the host hands to
    /// CoreGraphics.
    pub fn bytes(&self) -> &Arc<[u8]> {
        &self.bytes
    }

    /// The profile's own `desc`/`mluc` text, else a built-in name, else
    /// "Untitled profile".
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The `chad` tag (`s15Fixed16ArrayType`, nine values) as a row-major
    /// native→D50 matrix; `None` when absent or malformed. Advisory for
    /// conversion (the PCS white rule above); the 32-bit Photo Filter reads it
    /// (`adjust_white_balance::native_adaptation`).
    pub(crate) fn chromatic_adaptation(&self) -> Option<[[f64; 3]; 3]> {
        let data = tag(&self.bytes[..], self.bytes.len(), b"chad")?;
        if data.len() < 44 || &data[0..4] != b"sf32" {
            return None;
        }
        let mut m = [[0.0f64; 3]; 3];
        for (i, row) in m.iter_mut().enumerate() {
            for (j, entry) in row.iter_mut().enumerate() {
                *entry = s15(data, 8 + 4 * (3 * i + j))?;
            }
        }
        Some(m)
    }

    /// The transformable model, absent for an RGB profile this crate cannot
    /// convert through.
    ///
    /// This stays the MATRIX/TRC question after Phase 11C. `document_v2`
    /// Exposure and White Balance, `adopt_working_space` and every existing
    /// gate read it, and widening it to mean "can decode" would quietly move
    /// those onto a different numerical path. Ask [`IccProfile::can_decode`]
    /// for the wider question.
    pub fn model(&self) -> Option<&MatrixTrc> {
        self.model.as_ref()
    }

    /// The parsed A2B/B2A tables, present only for an RGB profile with no
    /// matrix/TRC model (Phase 11C).
    pub fn lut(&self) -> Option<&Arc<LutModel>> {
        self.lut.as_ref()
    }

    /// True when this crate can turn the profile's pixel numbers into PCS
    /// colour: a matrix/TRC model, or a LUT model with a device→PCS table.
    /// The Convert to Profile SOURCE gate and `convert_profile`'s gate, and
    /// nothing else — see [`IccProfile::model`].
    pub fn can_decode(&self) -> bool {
        self.model.is_some() || self.lut.as_ref().is_some_and(|l| l.can_decode())
    }

    /// True when this crate can turn PCS colour back into the profile's
    /// numbers: a matrix/TRC model, or a LUT model with a PCS→device table.
    /// The Convert to Profile and export DESTINATION gate.
    pub fn can_encode(&self) -> bool {
        self.model.is_some() || self.lut.as_ref().is_some_and(|l| l.can_encode())
    }

    /// True for the profile [`IccProfile::srgb`] returns. The DRDC writer
    /// elides the ICC slot for it, so a plain document does not grow by
    /// 2.5 KB.
    pub fn is_builtin_srgb(&self) -> bool {
        let builtin = IccProfile::srgb();
        Arc::ptr_eq(&self.bytes, &builtin.bytes) || *self.bytes == *builtin.bytes
    }

    /// True when this profile describes the sRGB colour SPACE — the
    /// [`describes_same_space`] question against the built-in, asked of a
    /// document's own profile.
    ///
    /// Not [`IccProfile::is_builtin_srgb`], which is a byte comparison: the
    /// 3144-byte HP/IEC "sRGB IEC61966-2.1" blob a camera or Photoshop
    /// embeds is not this build's 2568-byte one and describes exactly the
    /// same space. The EXIF normalizer asks it to decide what the exported
    /// packet's ColorSpace tag should say, where a false negative would
    /// label an ordinary sRGB export "Uncalibrated".
    ///
    /// False for a profile this crate cannot model: an unmodellable space is
    /// one we cannot claim IS sRGB.
    pub fn describes_srgb(&self) -> bool {
        if self.is_builtin_srgb() {
            return true;
        }
        match (self.model(), IccProfile::srgb().model()) {
            (Some(a), Some(b)) => a.approx_eq(b),
            _ => false,
        }
    }
}

fn builtin(which: Builtin) -> IccProfile {
    IccProfile {
        bytes: Arc::from(icc_builtin::blob(which).to_vec()),
        name: which.display_name().to_string(),
        model: Some(icc_builtin::model(which)),
        lut: None,
    }
}

/// Classifies raw bytes and, for anything that IS an ICC profile, writes the
/// display name through `name_out`. The host asks this before assigning so
/// it can say WHY a profile was refused instead of reporting a bare failure.
pub fn inspect(bytes: &[u8], name_out: &mut String) -> Inspect {
    let Some(end) = header_end(bytes) else {
        return Inspect::NotIcc;
    };
    *name_out = read_name(bytes, end);
    if let Some(refusal) = pixel_space_refusal(bytes) {
        return refusal;
    }
    if read_model(bytes, end).is_some() {
        Inspect::RgbMatrix
    } else if ProofProfile::parse(&bytes[..end]).is_ok_and(|l| l.can_decode() || l.can_encode()) {
        Inspect::RgbLut
    } else {
        Inspect::RgbUnconvertible
    }
}

/// Everything a host can say about profile bytes, as one JSON object — the
/// `dr_icc_describe` payload (Phase 11C). Unlike [`inspect`] this reads ANY
/// profile, including the Gray and CMYK ones a document may not carry but a
/// proof setup may name, and states its refusal instead of a bare class.
///
/// Two keys answer narrower questions than `refusal`: `proof_refusal` is why
/// the profile cannot be proofed through (null exactly when `proofable`; it
/// names the missing A2B/B2A direction and its floating-point twin, or is
/// the parser's own sentence), and `high_depth_destination` is whether every
/// typed Convert, 32-bit float included, accepts it as a destination —
/// `precision_model`'s answer, asked only of a profile with a matrix/TRC
/// model (a 16-bit Convert also accepts a LUT profile that can encode).
pub fn describe_json(bytes: &[u8]) -> serde_json::Value {
    use crate::adjust_calibrated::precision_model;
    use crate::icc_intent::RenderingIntent;
    use serde_json::{json, Value};

    let Some(end) = header_end(bytes) else {
        return json!({ "refusal": "the file is not an ICC profile" });
    };
    let data = &bytes[..end];
    let lut = crate::icc_lut::ProofProfile::parse(data);
    let model = read_model(data, end);
    let (space, class, pcs) = (
        String::from_utf8_lossy(&data[16..20])
            .trim_end()
            .to_string(),
        String::from_utf8_lossy(&data[12..16])
            .trim_end()
            .to_string(),
        String::from_utf8_lossy(&data[20..24])
            .trim_end()
            .to_string(),
    );
    let proof_refusal = match &lut {
        Ok(l) => l.proof_refusal(data),
        Err(reason) => Some(reason.clone()),
    };
    let high_depth_destination =
        model.is_some() && IccProfile::parse(data).is_some_and(|p| precision_model(&p).is_ok());
    // The refusal of an encode INTO a matrix profile whose curves fail the
    // precision check: the very sentence the core's export and convert
    // preflights refuse with (`icc_encode_refusal`).
    let destination_refusal = |verb: &str| {
        (model.is_some() && !high_depth_destination)
            .then(|| crate::icc_encode_refusal::destination_sentence(verb, &read_name(data, end)))
    };
    let can_decode = model.is_some() || lut.as_ref().is_ok_and(|l| l.can_decode());
    let can_encode = model.is_some() || lut.as_ref().is_ok_and(|l| l.can_encode());
    json!({
        "name": read_name(data, end),
        "version": format!("{}.{}", data[8], data[9] >> 4),
        "class": class,
        "space": match space.as_str() {
            "RGB" => "rgb",
            "GRAY" => "gray",
            "CMYK" => "cmyk",
            "Lab" => "lab",
            _ => "other",
        },
        "pcs": pcs,
        "model": if model.is_some() { "matrix" } else if lut.is_ok() { "lut" } else { "none" },
        "can_decode": can_decode,
        "can_encode": can_encode,
        "intents": {
            "perceptual": lut.as_ref().is_ok_and(|l| l.has_table(RenderingIntent::Perceptual))
                || model.is_some(),
            "relative": lut.as_ref().is_ok_and(|l| l.has_table(RenderingIntent::RelativeColorimetric))
                || model.is_some(),
            "saturation": lut.as_ref().is_ok_and(|l| l.has_table(RenderingIntent::Saturation))
                || model.is_some(),
        },
        "media_white": lut.as_ref().map(|l| l.media_white()).unwrap_or(crate::icc_lut::D50),
        "proofable": lut.as_ref().is_ok_and(|l| l.can_decode() && l.can_encode()),
        "refusal": match &lut {
            Ok(_) => Value::Null,
            Err(reason) if model.is_some() => {
                // A matrix/TRC profile with no tables is fully usable for
                // conversion; only PROOFING through it is unavailable.
                Value::String(reason.clone())
            }
            Err(reason) => Value::String(reason.clone()),
        },
        "proof_refusal": proof_refusal,
        "high_depth_destination": high_depth_destination,
        "export_refusal": destination_refusal("export"),
        "convert_refusal": destination_refusal("convert"),
    })
}

/// True when `a` and `b` are both matrix/TRC profiles describing the SAME
/// colour space — exactly the question `DrDocument::convert_to_profile`
/// refuses on, asked without a document so a host can DISABLE a command
/// instead of discovering the refusal as a bare nil.
///
/// Byte equality is not the same question, and the gap is the common case
/// rather than a corner: the "sRGB IEC61966-2.1" blob Photoshop, GIMP and
/// most cameras embed is 3144 bytes against this build's 2568, and the two
/// describe one space. A host that gated a Convert command on the bytes
/// therefore offered a conversion the core would always refuse.
///
/// False when either side is not an ICC RGB profile or is one this crate
/// cannot model — those are DIFFERENT refusals, each with its own sentence,
/// and [`inspect`] is what names them.
pub fn describes_same_space(a: &[u8], b: &[u8]) -> bool {
    match (IccProfile::parse(a), IccProfile::parse(b)) {
        (Some(a), Some(b)) => match (a.model(), b.model()) {
            (Some(a), Some(b)) => a.approx_eq(b),
            _ => false,
        },
        _ => false,
    }
}

// -------------------------------------------------------------- header --

/// Validates the 128-byte header and the tag table's extent, returning the
/// profile's effective length (its own declared size). `None` for anything
/// that is not an ICC profile.
fn header_end(bytes: &[u8]) -> Option<usize> {
    if bytes.len() < 132 {
        return None;
    }
    if &bytes[36..40] != b"acsp" {
        return None;
    }
    // The declared size is the authority, but only within the buffer we were
    // handed: a profile claiming more than it has is corrupt, and one
    // claiming less than a header is not a profile.
    let size = be_u32(bytes, 0)? as usize;
    if size < 132 || size > bytes.len() {
        return None;
    }
    // The tag table must fit, checked in u64 so a crafted count cannot wrap.
    let count = u64::from(be_u32(bytes, 128)?);
    if 132 + 12 * count > size as u64 {
        return None;
    }
    Some(size)
}

/// The profile CLASSES whose data colour space describes actual image
/// pixels: an input device, a display, an output device, and the
/// "colour space conversion" class a bare space like sRGB or Display P3 is
/// written as.
const PIXEL_CLASSES: [&[u8; 4]; 4] = [b"scnr", b"mntr", b"prtr", b"spac"];

/// Why the header at bytes 12..20 says these are NOT numbers a document's
/// pixels can be interpreted in, or `None` when they are: an RGB data colour
/// space, in a profile class that describes pixels.
///
/// The class half is not pedantry. A device-link (`link`), abstract (`abst`)
/// or named-colour (`nmcl`) profile describes a TRANSFORM or a list of
/// colours, not a space — its data-colour-space field names the space its
/// INPUT is in — and macOS ships one that says `RGB `: `WebSafeColors.icc`
/// is `nmcl` with a Lab PCS and no matrix, no TRC and no A2B. Reading the
/// space field alone made it a storable profile, and CoreGraphics then
/// accepted it as a tag it could not convert OUT of, so every pixel the host
/// drew through it came out empty — a blank canvas, blank thumbnails, a
/// blank copy and a blank print, silently, and re-embedded on export so the
/// file stayed blank. Refusing it here, where Gray, CMYK and Lab are already
/// refused, is the one place that cannot be worked around.
fn pixel_space_refusal(bytes: &[u8]) -> Option<Inspect> {
    if bytes.len() < 20 || &bytes[16..20] != b"RGB " {
        return Some(Inspect::NotRgb);
    }
    if !PIXEL_CLASSES.iter().any(|c| bytes[12..16] == **c) {
        return Some(Inspect::NotImageProfile);
    }
    None
}

/// The data of the first tag with signature `sig`, or `None` when it is
/// absent or its extent is not a 4-aligned range inside the profile. A bad
/// tag makes the TAG absent, not the profile.
pub(crate) fn tag<'a>(bytes: &'a [u8], end: usize, sig: &[u8; 4]) -> Option<&'a [u8]> {
    let count = be_u32(bytes, 128)? as usize;
    for i in 0..count {
        let entry = 132 + 12 * i;
        if bytes.get(entry..entry + 4)? != sig.as_slice() {
            continue;
        }
        let offset = be_u32(bytes, entry + 4)? as usize;
        let size = be_u32(bytes, entry + 8)? as usize;
        if !offset.is_multiple_of(4) || offset < 128 {
            return None;
        }
        let tag_end = offset.checked_add(size)?;
        if tag_end > end {
            return None;
        }
        return Some(&bytes[offset..tag_end]);
    }
    None
}

pub(crate) fn be_u32(bytes: &[u8], at: usize) -> Option<u32> {
    let slice = bytes.get(at..at.checked_add(4)?)?;
    Some(u32::from_be_bytes(slice.try_into().ok()?))
}

pub(crate) fn be_u16(bytes: &[u8], at: usize) -> Option<u16> {
    let slice = bytes.get(at..at.checked_add(2)?)?;
    Some(u16::from_be_bytes(slice.try_into().ok()?))
}

/// An s15Fixed16Number: a SIGNED 32-bit fixed point with 16 fractional bits.
/// Negative values are normal — Display P3's red primary has a negative Z.
pub(crate) fn s15(bytes: &[u8], at: usize) -> Option<f64> {
    Some(f64::from(be_u32(bytes, at)? as i32) / 65536.0)
}

// --------------------------------------------------------------- model --

/// The matrix/TRC model, present only when all six of `rXYZ`, `gXYZ`,
/// `bXYZ`, `rTRC`, `gTRC` and `bTRC` parse and the columns are an invertible
/// basis.
fn read_model(bytes: &[u8], end: usize) -> Option<MatrixTrc> {
    let cols = [
        xyz(tag(bytes, end, b"rXYZ")?)?,
        xyz(tag(bytes, end, b"gXYZ")?)?,
        xyz(tag(bytes, end, b"bXYZ")?)?,
    ];
    let trc = [
        curve(tag(bytes, end, b"rTRC")?)?,
        curve(tag(bytes, end, b"gTRC")?)?,
        curve(tag(bytes, end, b"bTRC")?)?,
    ];
    let mut model = MatrixTrc::new(cols, trc)?;
    for name in [b"rTRC", b"gTRC", b"bTRC"] {
        let data = tag(bytes, end, name)?;
        if data.get(..4) == Some(b"curv") {
            let count = be_u32(data, 8)? as usize;
            if count == 1 && be_u16(data, 12)? == 0 {
                model.mark_precision_incompatible();
            }
            if count > 1 && (1..count).any(|i| be_u16(data, 12 + i * 2) < be_u16(data, 10 + i * 2))
            {
                model.mark_precision_incompatible();
            }
        }
    }
    Some(model)
}

/// `XYZType`: the signature, four reserved bytes, three s15Fixed16.
pub(crate) fn xyz(data: &[u8]) -> Option<[f64; 3]> {
    if data.len() < 20 || &data[0..4] != b"XYZ " {
        return None;
    }
    Some([s15(data, 8)?, s15(data, 12)?, s15(data, 16)?])
}

/// `curveType` or `parametricCurveType`.
pub(crate) fn curve(data: &[u8]) -> Option<Curve> {
    match data.get(0..4)? {
        b"curv" => {
            let count = be_u32(data, 8)? as usize;
            match count {
                0 => Some(Curve::Identity),
                1 => {
                    // A single entry is a u8Fixed8Number gamma. A
                    // non-positive exponent describes nothing, so it degrades
                    // to the linear curve rather than producing infinities.
                    let g = f64::from(be_u16(data, 12)?) / 256.0;
                    Some(if g > 0.0 {
                        Curve::Gamma(g)
                    } else {
                        Curve::Identity
                    })
                }
                _ => {
                    let need = 12usize.checked_add(count.checked_mul(2)?)?;
                    if need > data.len() {
                        return None;
                    }
                    // Monotonized by a running maximum: the threshold encode
                    // in `icc_transform` binary-searches this curve's samples
                    // and a non-monotone table would break the search's
                    // precondition. A monotone table is unchanged.
                    let mut run = 0u16;
                    let table = (0..count)
                        .map(|i| {
                            let v = u16::from_be_bytes([data[12 + 2 * i], data[13 + 2 * i]]);
                            run = run.max(v);
                            run
                        })
                        .collect();
                    Some(Curve::Sampled(table))
                }
            }
        }
        b"para" => {
            let kind = be_u16(data, 8)?;
            // Function types 0 to 4 take 1, 3, 4, 5 and 7 parameters.
            let n = match kind {
                0 => 1usize,
                1 => 3,
                2 => 4,
                3 => 5,
                4 => 7,
                _ => return None,
            };
            if data.len() < 12 + 4 * n {
                return None;
            }
            let mut p = [0.0f64; 7];
            for (i, slot) in p.iter_mut().take(n).enumerate() {
                *slot = s15(data, 12 + 4 * i)?;
            }
            Some(Curve::Para {
                kind: kind as u8,
                g: p[0],
                a: p[1],
                b: p[2],
                c: p[3],
                d: p[4],
                e: p[5],
                f: p[6],
            })
        }
        _ => None,
    }
}

// ---------------------------------------------------------------- name --

/// The profile's display name. The tag is called `desc` in both ICC
/// versions; only its TYPE differs — `textDescriptionType` in v2,
/// `multiLocalizedUnicodeType` in v4 — and a few profiles put a plain
/// `textType` there. All three are read; anything else is "Untitled
/// profile".
fn read_name(bytes: &[u8], end: usize) -> String {
    let raw = tag(bytes, end, b"desc").and_then(|d| {
        desc_name(d)
            .or_else(|| mluc_name(d))
            .or_else(|| text_name(d))
    });
    sanitize(raw)
}

/// `textDescriptionType`: u32 ASCII count INCLUDING its NUL, then the
/// string. Only `12 + count <= size` is required — the 67-byte Macintosh
/// tail is written by this build but demanding it in the READER makes real
/// profiles with a truncated tail fall back to "Untitled profile".
fn desc_name(data: &[u8]) -> Option<String> {
    if data.get(0..4)? != b"desc" {
        return None;
    }
    let count = be_u32(data, 8)? as usize;
    let text = data.get(12..12usize.checked_add(count)?)?;
    Some(latin1(text))
}

/// `multiLocalizedUnicodeType`: record count, record size (12), then per
/// record a language, a country, a byte length and an offset FROM THE TAG's
/// start; the strings are UTF-16BE with no terminator. The first record is
/// used — a profile's name is chrome, not a localization surface.
fn mluc_name(data: &[u8]) -> Option<String> {
    if data.get(0..4)? != b"mluc" {
        return None;
    }
    let records = be_u32(data, 8)? as usize;
    if records == 0 || be_u32(data, 12)? != 12 {
        return None;
    }
    let len = be_u32(data, 16 + 4)? as usize;
    let offset = be_u32(data, 16 + 8)? as usize;
    let text = data.get(offset..offset.checked_add(len)?)?;
    let units: Vec<u16> = text
        .chunks_exact(2)
        .map(|p| u16::from_be_bytes([p[0], p[1]]))
        .collect();
    Some(String::from_utf16_lossy(&units))
}

/// `textType`: NUL-terminated 7-bit ASCII after the signature and four
/// reserved bytes.
fn text_name(data: &[u8]) -> Option<String> {
    if data.get(0..4)? != b"text" {
        return None;
    }
    Some(latin1(data.get(8..)?))
}

fn latin1(bytes: &[u8]) -> String {
    bytes.iter().map(|&b| char::from(b)).collect()
}

/// Trims at the first NUL, replaces control characters, collapses the result
/// and caps it; an empty or absent name becomes "Untitled profile".
fn sanitize(raw: Option<String>) -> String {
    let Some(raw) = raw else {
        return UNTITLED.to_string();
    };
    let trimmed: String = raw
        .split('\0')
        .next()
        .unwrap_or("")
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .take(MAX_NAME_CHARS)
        .collect();
    let trimmed = trimmed.trim();
    if trimmed.is_empty() {
        UNTITLED.to_string()
    } else {
        trimmed.to_string()
    }
}

// ---------------------------------------------------------- the byte view --

/// A validated view over profile bytes: the header fields and the tag table,
/// without the RGB-only policy [`IccProfile::parse`] applies. `icc_lut` reads
/// Gray and CMYK profiles through this, so the header walk and the tag lookup
/// have exactly one implementation.
pub(crate) struct IccBytes<'a> {
    bytes: &'a [u8],
    end: usize,
}

impl<'a> IccBytes<'a> {
    /// `None` for anything that is not an ICC profile (the [`header_end`]
    /// rule).
    pub(crate) fn new(bytes: &'a [u8]) -> Option<IccBytes<'a>> {
        let end = header_end(bytes)?;
        Some(IccBytes { bytes, end })
    }

    /// The profile's own declared size — [`header_end`]'s value, which
    /// [`IccProfile::parse`] compares against the blob cap.
    pub(crate) fn declared_len(&self) -> usize {
        self.end
    }

    pub(crate) fn data_space(&self) -> &'a [u8] {
        &self.bytes[16..20]
    }

    pub(crate) fn pcs_space(&self) -> &'a [u8] {
        &self.bytes[20..24]
    }

    pub(crate) fn space_name(&self) -> String {
        String::from_utf8_lossy(self.data_space()).trim_end().into()
    }

    pub(crate) fn class_name(&self) -> String {
        String::from_utf8_lossy(&self.bytes[12..16])
            .trim_end()
            .into()
    }

    /// True for the four classes whose data colour space describes pixels —
    /// [`PIXEL_CLASSES`], the same table `pixel_space_refusal` reads.
    pub(crate) fn is_pixel_class(&self) -> bool {
        PIXEL_CLASSES.iter().any(|c| self.bytes[12..16] == **c)
    }

    pub(crate) fn tag(&self, sig: &[u8; 4]) -> Option<&'a [u8]> {
        tag(self.bytes, self.end, sig)
    }

    pub(crate) fn name(&self) -> String {
        read_name(self.bytes, self.end)
    }

    /// The header's major version number.
    /// The header's MAJOR version number. Bytes 8..12 are BCD-ish: byte 8 is
    /// the whole major number (2 or 4), byte 9's high nibble the minor one.
    /// Reading byte 8's high nibble instead makes every v4 profile look like
    /// v0, which silently moves the perceptual black-point rule.
    pub(crate) fn major_version(&self) -> u8 {
        self.bytes[8]
    }

    /// The `wtpt` tag, `None` when absent or malformed.
    pub(crate) fn media_white(&self) -> Option<[f64; 3]> {
        self.tag(b"wtpt").and_then(xyz)
    }
}
