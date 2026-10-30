//! The named metadata policy an export may apply: a per-packet keep / drop /
//! default decision plus an optional author-and-copyright stamp, together
//! with the report of what the encode ACTUALLY wrote.
//!
//! # Why a policy sits inside the export request
//!
//! Phase 11A's request already carries `strip_metadata`, an all-or-nothing
//! switch over the three inherited packets. A production deliverable needs
//! finer control — a web JPEG that keeps the copyright statement and drops
//! the camera serial number is an ordinary requirement — so the policy is
//! another key of the SAME request rather than a second delivery path. With
//! the key omitted every rule is [`PacketRule::Default`], every packet defers
//! to `strip_metadata`, and the bytes are the bytes 11A already wrote.
//!
//! # Orientation is not a choice
//!
//! There is deliberately no orientation rule. `dr_image::open_bytes` bakes
//! the camera's rotation INTO the pixels, so a packet that kept
//! `Orientation: 6` would rotate the delivered picture a second time in every
//! viewer. Both normalizers (`metadata_write::exif_normalized` and
//! `metadata_xmp::xmp_normalized`) therefore reset it unconditionally, and
//! every packet this module lets through states Orientation 1.
//!
//! # Why `density: drop` is a cross-field rule
//!
//! Dropping the container's own density slot (the JFIF APP0, the PNG `pHYs`)
//! does not stop the file from stating a ppi: a kept EXIF packet restates it
//! in IFD0 — where Exif/DCF makes it the AUTHORITY — and a kept XMP packet
//! restates it again as `tiff:XResolution`. A policy that asked for
//! `density: drop` beside `exif: keep` would therefore report something the
//! bytes contradict, so [`MetadataPolicy::refusal`] refuses the combination
//! at validate time instead.
//!
//! Per packet `Default | Keep | Drop`, collapsed to a boolean by the ONE `keeps(rule,
//! strip_metadata)` that `doc_color::image_sidecar_with_policy` and `MetadataPolicy::refusal` both
//! read, so the sidecar and the refusal can never disagree. There is deliberately NO orientation
//! rule: `dr_image::open_bytes` bakes the rotation into the pixels, so both normalizers reset it
//! unconditionally and every packet written states Orientation 1. Two validate-time refusals,
//! because each would otherwise report what the bytes contradict: a stamp when the XMP packet will
//! not be written, and `density: drop` while EXIF or XMP still resolves to written (each restates
//! the ppi). The stamp is XMP-only — `metadata_xmp::xmp_stamped` inserts `dc:creator`/`dc:rights`
//! into the first non-self-closing `rdf:Description` (`None` ⇒ the normalized packet is written
//! unchanged and `stamped` is false), `xmp_minimal` is the whole packet a document with no XMP
//! gets. `MetadataPolicy::DEFAULT` is 11A exactly, and `doc_color::image_sidecar` is that case,
//! kept as its own entry point so `save_image`, the preservation preflight and the typed codec keep
//! writing exactly today's bytes.

use serde_json::{json, Value};

use crate::doc_color::{format_carries, CARRIES_EXIF, CARRIES_XMP};
use crate::dr_image::Format;

/// The longest author or copyright string a stamp may carry. A stamp is a
/// line of credit, not a document; bounding it keeps a hostile string out of
/// the XMP packet a downstream reader parses.
pub const MAX_STAMP_CHARS: usize = 512;

/// What one packet's policy says. `Default` is 11A's behaviour exactly.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PacketRule {
    /// Defer to the request's `strip_metadata`.
    Default,
    /// Write the packet even when `strip_metadata` is set.
    Keep,
    /// Never write the packet.
    Drop,
}

impl PacketRule {
    fn parse(value: Option<&Value>, key: &str) -> Result<Self, String> {
        match value {
            None | Some(Value::Null) => Ok(PacketRule::Default),
            Some(Value::String(text)) => match text.as_str() {
                "default" => Ok(PacketRule::Default),
                "keep" => Ok(PacketRule::Keep),
                "drop" => Ok(PacketRule::Drop),
                other => Err(format!(
                    "export metadata {key} \"{other}\" is not one of default, keep or drop"
                )),
            },
            Some(_) => Err(format!("export metadata {key} must be a string")),
        }
    }

    fn name(self) -> &'static str {
        match self {
            PacketRule::Default => "default",
            PacketRule::Keep => "keep",
            PacketRule::Drop => "drop",
        }
    }
}

/// The optional credit written into the XMP packet: `dc:creator` and
/// `dc:rights`. EXIF `Artist` / `Copyright` are deliberately not written by
/// a stamp — an IFD has internal offsets and inserting a tag means
/// rebuilding it, which only File Info's authored edit does
/// (`file_info_legacy`).
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct Stamp {
    pub author: Option<String>,
    pub copyright: Option<String>,
}

impl Stamp {
    fn parse(value: &Value) -> Result<Self, String> {
        let object = value
            .as_object()
            .ok_or("export metadata stamp must be a JSON object")?;
        for key in object.keys() {
            if key != "author" && key != "copyright" {
                return Err(format!(
                    "export metadata stamp has an unknown key \"{key}\""
                ));
            }
        }
        let field = |key: &str| -> Result<Option<String>, String> {
            match object.get(key) {
                None | Some(Value::Null) => Ok(None),
                Some(Value::String(text)) => {
                    if text.chars().count() > MAX_STAMP_CHARS {
                        return Err(format!(
                            "export metadata stamp {key} must be at most {MAX_STAMP_CHARS} characters"
                        ));
                    }
                    if text.chars().any(char::is_control) {
                        return Err(format!(
                            "export metadata stamp {key} must not contain control characters"
                        ));
                    }
                    // A blank string is absent, not a value: writing an empty
                    // dc:creator entry and reporting `stamped` would be a
                    // packet edit that says nothing.
                    if text.trim().is_empty() {
                        return Ok(None);
                    }
                    Ok(Some(text.clone()))
                }
                Some(_) => Err(format!("export metadata stamp {key} must be a string")),
            }
        };
        let stamp = Stamp {
            author: field("author")?,
            copyright: field("copyright")?,
        };
        if stamp.author.is_none() && stamp.copyright.is_none() {
            return Err("export metadata stamp requires an author or a copyright".into());
        }
        Ok(stamp)
    }

    fn to_json(&self) -> Value {
        json!({
            "author": self.author.clone().map_or(Value::Null, Value::String),
            "copyright": self.copyright.clone().map_or(Value::Null, Value::String),
        })
    }
}

/// One export's whole metadata policy.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MetadataPolicy {
    pub exif: PacketRule,
    pub xmp: PacketRule,
    pub iptc: PacketRule,
    pub density: PacketRule,
    pub stamp: Option<Stamp>,
}

impl MetadataPolicy {
    /// Every rule deferred, no stamp — what an omitted `metadata` key means,
    /// and what every 11A caller keeps passing.
    pub const DEFAULT: Self = MetadataPolicy {
        exif: PacketRule::Default,
        xmp: PacketRule::Default,
        iptc: PacketRule::Default,
        density: PacketRule::Default,
        stamp: None,
    };

    /// The ONE parser, shared by the export request and the standalone
    /// `dr_metadata_policy_validate` query. An unknown key refuses rather
    /// than being ignored: a misspelled rule that quietly wrote the camera's
    /// packets into a public JPEG would be discovered only by a stranger.
    pub fn parse(value: &Value) -> Result<Self, String> {
        let object = value
            .as_object()
            .ok_or("export metadata must be a JSON object")?;
        for key in object.keys() {
            if !["exif", "xmp", "iptc", "density", "stamp"].contains(&key.as_str()) {
                return Err(format!("export metadata has an unknown key \"{key}\""));
            }
        }
        let stamp = match object.get("stamp") {
            None | Some(Value::Null) => None,
            Some(value) => Some(Stamp::parse(value)?),
        };
        Ok(MetadataPolicy {
            exif: PacketRule::parse(object.get("exif"), "exif")?,
            xmp: PacketRule::parse(object.get("xmp"), "xmp")?,
            iptc: PacketRule::parse(object.get("iptc"), "iptc")?,
            density: PacketRule::parse(object.get("density"), "density")?,
            stamp,
        })
    }

    /// The canonical echo: every rule spelled out, the stamp present as an
    /// object or `null`. A host stores this, not the text it typed.
    pub fn to_json(&self) -> Value {
        json!({
            "exif": self.exif.name(),
            "xmp": self.xmp.name(),
            "iptc": self.iptc.name(),
            "density": self.density.name(),
            "stamp": self.stamp.as_ref().map_or(Value::Null, Stamp::to_json),
        })
    }

    /// Whether one packet is written, given the request's `strip_metadata`.
    /// This is the ONE place the three-valued rule collapses into a boolean,
    /// so the sidecar and the refusal can never disagree.
    pub fn keeps(rule: PacketRule, strip_metadata: bool) -> bool {
        match rule {
            PacketRule::Default => !strip_metadata,
            PacketRule::Keep => true,
            PacketRule::Drop => false,
        }
    }

    /// The validate-time refusals, checked once by `ExportRequest::validate`
    /// so both depths refuse the same combinations with the same sentence.
    ///
    /// * A stamp with nowhere to go: the XMP packet is the only thing this
    ///   build can write a credit into, so a dropped or unsupported XMP makes
    ///   the stamp a silent no-op rather than a policy.
    /// * `density: drop` beside a written EXIF or XMP packet: each of those
    ///   restates the ppi, so the drop would be reported and contradicted by
    ///   the bytes in the same file.
    pub(crate) fn refusal(&self, format: Format, strip_metadata: bool) -> Option<String> {
        let caps = format_carries(format);
        let writes =
            |rule: PacketRule, bit: u32| Self::keeps(rule, strip_metadata) && caps & bit != 0;
        if self.stamp.is_some() && !writes(self.xmp, CARRIES_XMP) {
            return Some(
                "a metadata stamp is written into the xmp packet, which this export does not write"
                    .into(),
            );
        }
        if self.density == PacketRule::Drop
            && (writes(self.exif, CARRIES_EXIF) || writes(self.xmp, CARRIES_XMP))
        {
            return Some(
                "density: drop requires the exif and xmp packets to be dropped too; each carries its own copy of the ppi"
                    .into(),
            );
        }
        None
    }
}

/// What the encode actually wrote — the report a host shows beside the
/// deliverable, and the thing an independent decoder is checked against.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MetadataOutcome {
    pub profile: bool,
    pub exif: bool,
    pub xmp: bool,
    pub iptc: bool,
    pub density: bool,
    /// True only when a stamp was requested AND the written XMP packet
    /// carries it. A packet with no `rdf:Description` opening tag is written
    /// unchanged and reports false rather than failing the export.
    pub stamped: bool,
}

impl MetadataOutcome {
    pub fn to_json(&self) -> Value {
        json!({
            "profile": self.profile,
            "exif": self.exif,
            "xmp": self.xmp,
            "iptc": self.iptc,
            "density": self.density,
            "stamped": self.stamped,
        })
    }
}
