//! The EXIF / XMP / IPTC packets a document preserves, the print
//! resolution, and the ONE container walk for JPEG, PNG and TIFF (classic or BigTIFF). The write half — the segment and chunk builders, the streaming
//! injector, the in-place EXIF normalizer and the 8BIM filter — is
//! `metadata_write`; the document-level ops that use both are `doc_color`.
//!
//! # The contract on a packet
//!
//! Captured packets are **stored verbatim** — the same opaque
//! "parasite" contract a layer's `meta` has, one level up. The core does not
//! parse an EXIF block to answer questions about it; the three places it looks
//! inside are narrow and stated: [`resolution_from_exif`] here, for the ppi
//! the file states (the authority Exif/DCF names, with the container's own
//! density as the fallback — see [`resolve_resolution`]),
//! `tiff_view::lens_identity_from_exif`, for the camera, lens, exposure and
//! frame size Lensfun matching reads (v1-lensfun, `dr_lens_exif_identity`), and
//! `metadata_write::exif_normalized` on the way out, to reset the
//! orientation the pixels already absorbed and to bring the resolution, the
//! colour space and the dimensions into line with the document (a packet it
//! cannot correct is dropped rather than written contradicting the file it
//! is in). `metadata_xmp::xmp_normalized` does the same for the XMP packet's
//! own copies of the orientation and the resolution.
//!
//! JPEG and PNG retain packet bytes. TIFF (classic or BigTIFF) delegates to
//! `metadata_tiff`: its EXIF pointers address the whole image file, so
//! standard capture fields are rebuilt into a compact relocatable packet,
//! a MakerNote carried in it as one opaque value. Other private fields are
//! omitted and disclosed. WebP, BMP and GIF capture metadata are outside
//! this slice.
//!
//! Blob conventions, fixed here and matched by the writer:
//!
//! * **EXIF** is the raw TIFF stream with **no `Exif\0\0` prefix** — the PNG
//!   `eXIf` convention. The JPEG scanner strips the 6-byte prefix and
//!   `image`'s JPEG encoder adds it back. TIFF rebuilds standard capture
//!   IFDs into this same packet convention.
//! * **XMP** is the UTF-8 packet with no identifier: in a JPEG the APP1
//!   payload after `http://ns.adobe.com/xap/1.0/\0`, in a PNG the
//!   `iTXt` text under the keyword `XML:com.adobe.xmp`. Adobe writes it
//!   uncompressed (flag 0); a compressed one (flag 1, method 0) is inflated
//!   through `psd_zip`, bounded by the blob cap, and one that cannot be
//!   trusted reads as absent. The writer always writes flag 0.
//!   The slot holds the LOGICAL packet: a JPEG's ExtendedXMP chunks
//!   (`http://ns.adobe.com/xmp/extension/`) are merged into its standard
//!   packet on open (`metadata_xmp_extended`), and a JPEG export splits a
//!   packet too large for one segment back out (`metadata_xmp_split`).
//!   TIFF stores the same packet bytes in tag 700.
//! * **IPTC** is the JPEG APP13 payload after `Photoshop 3.0\0` — the whole
//!   8BIM image-resource run, never parsed on the way in. PNG carries none
//!   (ImageMagick's base64-in-`zTXt` convention is not IIM and is not read).
//!   TIFF reads the run in tag 34377 or wraps tag 33723 IIM as resource 1028.

use std::sync::Arc;

use crate::drdc::MAX_DRDC_BLOB_LEN;
use crate::metadata_xmp_extended::{extension_chunk, merged, ExtensionChunk, MAX_EXTENSION_CHUNKS};
use crate::style_json::q4;
use crate::tiff_view::resolution_from_exif;

/// The PNG signature: the one home of the eight bytes (this module walks the
/// PNG container); every reader and writer in the core routes through it.
pub(crate) const PNG_SIGNATURE: [u8; 8] = [0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A];

/// The JPEG APP1 identifier that introduces an XMP packet.
pub(crate) const XMP_ID: &[u8] = b"http://ns.adobe.com/xap/1.0/\0";

/// The JPEG APP1 identifier that introduces one ExtendedXMP chunk
/// (`metadata_xmp_extended`).
pub(crate) const XMP_EXT_ID: &[u8] = b"http://ns.adobe.com/xmp/extension/\0";

/// The JPEG APP13 identifier that introduces the 8BIM image-resource run.
pub(crate) const IPTC_ID: &[u8] = b"Photoshop 3.0\0";

/// The PNG `iTXt` keyword Adobe writes an XMP packet under.
pub(crate) const PNG_XMP_KEYWORD: &[u8] = b"XML:com.adobe.xmp";

/// Inches per metre's reciprocal — the PNG `pHYs` unit is pixels per metre.
const METRES_PER_INCH: f64 = 0.0254;

/// The print resolution a document with no stated one takes: 72 ppi, the
/// PostScript point, which is what every format assumes when it carries no
/// resolution at all.
const DEFAULT_PPI: f32 = 72.0;

/// Resolution clamp: 30 000 ppi at the top, and below 1 ppi the print size
/// of a 100 MP canvas is measured in miles.
pub(crate) const MIN_PPI: f64 = 1.0;
pub(crate) const MAX_PPI: f64 = 30_000.0;

/// The three metadata packets, preserved verbatim from open and re-spliced
/// on export. `Arc<[u8]>` for the same copy-on-write reason as layer pixels:
/// every pure op clones the document, and a megabyte of XMP must not be
/// copied on a brush tick.
#[derive(Clone, Default, PartialEq, Eq, Debug)]
pub struct Metadata {
    pub exif: Option<Arc<[u8]>>,
    pub xmp: Option<Arc<[u8]>>,
    pub iptc: Option<Arc<[u8]>>,
}

impl Metadata {
    /// True when no packet is present — the state a document built from bare
    /// pixels is in.
    pub fn is_empty(&self) -> bool {
        self.exif.is_none() && self.xmp.is_none() && self.iptc.is_none()
    }
}

/// Print resolution in pixels per inch, per axis (Photoshop stores both).
/// Pixels never change with it — only the print size does.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Resolution {
    pub x: f32,
    pub y: f32,
}

impl Default for Resolution {
    fn default() -> Self {
        Resolution {
            x: DEFAULT_PPI,
            y: DEFAULT_PPI,
        }
    }
}

impl Resolution {
    /// The ONE sanitizer, used by the setter, the DRDC reader, the DRDC
    /// writer and the FFI — `GlobalLight::sane`'s twin. A non-finite or
    /// non-positive component takes the 72 ppi default; finite values clamp
    /// to [1, 30000] and quantize to four decimals through the style
    /// parser's own [`q4`], the crate's ONE four-decimal quantizer, so a
    /// host that echoes a reported value back through the setter is refused
    /// as unchanged instead of registering a phantom edit.
    pub fn sane(self) -> Self {
        Resolution {
            x: sane_ppi(self.x),
            y: sane_ppi(self.y),
        }
    }

    /// The ONE home of the setters' refusal: both depths' print
    /// resolution setters call it. `Err` for a non-finite or non-positive
    /// component; otherwise the [`sane`](Self::sane) value.
    pub(crate) fn checked(x: f32, y: f32) -> Result<Self, String> {
        if !x.is_finite() || !y.is_finite() || x <= 0.0 || y <= 0.0 {
            return Err("print resolution must be finite and positive".into());
        }
        Ok(Resolution { x, y }.sane())
    }

    /// The two axes swapped when `swap` — a 300 x 150 ppi document turned
    /// 90 degrees is 150 x 300.
    pub(crate) fn transposed_if(self, swap: bool) -> Self {
        if swap {
            Resolution {
                x: self.y,
                y: self.x,
            }
        } else {
            self
        }
    }
}

fn sane_ppi(v: f32) -> f32 {
    if v.is_finite() && v > 0.0 {
        q4(f64::from(v).clamp(MIN_PPI, MAX_PPI))
    } else {
        DEFAULT_PPI
    }
}

/// Everything a container walk found. Absent fields are absent, never an
/// error: a corrupt APP1 is still an openable file.
#[derive(Default)]
pub(crate) struct Scan {
    pub icc: Option<Vec<u8>>,
    pub metadata: Metadata,
    pub resolution: Option<Resolution>,
    /// A JPEG's Adobe APP14 colour transform (0 = CMYK/RGB as stored,
    /// 1 = YCbCr, 2 = YCCK): the FIRST APP14 whose payload starts "Adobe"
    /// and is at least 12 bytes long. Its presence is what says a CMYK JPEG
    /// stores inverted samples (`cmyk_jpeg`).
    pub adobe_transform: Option<u8>,
}

/// The containers this crate walks, told apart by their magic — the ONE
/// place that test lives, so [`scan`] and [`walks`] can never disagree about
/// which files contribute their capture data.
enum Container {
    Jpeg,
    Png,
    Tiff,
}

fn container(bytes: &[u8]) -> Option<Container> {
    if bytes.starts_with(&[0xFF, 0xD8]) {
        Some(Container::Jpeg)
    } else if bytes.starts_with(&PNG_SIGNATURE) {
        Some(Container::Png)
    } else if crate::metadata_tiff::is_tiff(bytes) {
        Some(Container::Tiff)
    } else {
        None
    }
}

/// True when [`scan`] would WALK these bytes: JPEG, PNG or TIFF (classic or BigTIFF).
/// Container-specific packet limits still apply; TIFF reports detailed
/// limitations through `dr_path_metadata_import_report`.
pub(crate) fn walks(bytes: &[u8]) -> bool {
    container(bytes).is_some()
}

/// Reads the packets and the resolution out of the file's own bytes,
/// sniffing the container. Other containers scan empty.
/// A resolution found in a file is sanitized here, so no other caller has to
/// decide what an absurd one means.
pub(crate) fn scan(bytes: &[u8]) -> Scan {
    scan_report(bytes).0
}

/// The same single scan with detected TIFF losses retained for both file
/// diagnostics and platform-carrier attachment, including no-op attachment.
pub(crate) fn scan_report(bytes: &[u8]) -> (Scan, Vec<&'static str>) {
    let (mut found, warnings) = match container(bytes) {
        Some(Container::Jpeg) => scan_jpeg(bytes),
        Some(Container::Png) => (scan_png(bytes), Vec::new()),
        Some(Container::Tiff) => crate::metadata_tiff::scan(bytes),
        None => (Scan::default(), Vec::new()),
    };
    found.resolution = found.resolution.map(Resolution::sane);
    (found, warnings)
}

/// Whether a packet would be kept if it were offered: the first one wins,
/// and anything larger than the format's own blob cap is refused so a
/// captured packet can always be saved again.
///
/// Separate from [`capture`] so a walk can ask BEFORE doing work that only
/// pays off if the answer is yes — see `scan_png`, where the work is a CRC
/// over the whole chunk.
fn capturable(slot: &Option<Arc<[u8]>>, bytes: &[u8]) -> bool {
    slot.is_none() && bytes.len() <= MAX_DRDC_BLOB_LEN as usize
}

/// Stores a captured packet, first one wins, refusing anything larger than
/// the format's own blob cap so a captured packet can always be saved again.
fn capture(slot: &mut Option<Arc<[u8]>>, bytes: &[u8]) {
    if capturable(slot, bytes) {
        *slot = Some(Arc::from(bytes.to_vec()));
    }
}

// ----------------------------------------------------------------- JPEG --

/// The JPEG walk, and the ExtendedXMP merge's warnings
/// (`metadata_xmp_extended::merged`).
fn scan_jpeg(bytes: &[u8]) -> (Scan, Vec<&'static str>) {
    let mut out = Scan::default();
    let mut container = None;
    // The ExtendedXMP chunks, borrowed from `bytes` in file order — they may
    // come before the standard packet — and whether there were more than
    // the walk keeps.
    let mut extension: Vec<ExtensionChunk<'_>> = Vec::new();
    let mut extension_overflow = false;
    // The 8BIM run, CONCATENATED across every APP13 that carries it (see the
    // APP13 arm below), and whether it outgrew the blob cap on the way.
    let mut iptc: Vec<u8> = Vec::new();
    let mut iptc_over_cap = false;
    let mut i = 2usize;
    while i + 4 <= bytes.len() {
        if bytes[i] != 0xFF {
            break;
        }
        let marker = bytes[i + 1];
        // Fill bytes: any number of 0xFF may precede a marker code.
        if marker == 0xFF {
            i += 1;
            continue;
        }
        // Standalone markers carry no length.
        if marker == 0xD8 || marker == 0x01 || (0xD0..=0xD7).contains(&marker) {
            i += 2;
            continue;
        }
        // Entropy-coded data begins at SOS; nothing after it is a segment we
        // read.
        if marker == 0xDA || marker == 0xD9 {
            break;
        }
        let len = usize::from(u16::from_be_bytes([bytes[i + 2], bytes[i + 3]]));
        if len < 2 {
            break;
        }
        let start = i + 4;
        let Some(payload) = bytes.get(start..start + len - 2) else {
            break;
        };
        match marker {
            0xE0 => {
                if let Some(rest) = payload.strip_prefix(b"JFIF\0".as_slice()) {
                    if container.is_none() {
                        container = jfif_density(rest);
                    }
                }
            }
            0xE1 => {
                if let Some(rest) = payload.strip_prefix(b"Exif\0\0".as_slice()) {
                    capture(&mut out.metadata.exif, rest);
                } else if let Some(rest) = payload.strip_prefix(XMP_ID) {
                    capture(&mut out.metadata.xmp, rest);
                } else if let Some(rest) = payload.strip_prefix(XMP_EXT_ID) {
                    if extension.len() < MAX_EXTENSION_CHUNKS {
                        extension.extend(extension_chunk(rest));
                    } else {
                        extension_overflow = true;
                    }
                }
            }
            0xED => {
                if let Some(rest) = payload.strip_prefix(IPTC_ID) {
                    // CONCATENATED, not first-one-wins: Photoshop splits an
                    // image-resource run larger than one segment across
                    // several `Photoshop 3.0\0` APP13s, and a reader is
                    // meant to join them back into one run. Taking the first
                    // dropped every resource after it — silently, because
                    // the save then re-spliced the truncated prefix and
                    // reported the packet as written.
                    if iptc.len().saturating_add(rest.len()) <= MAX_DRDC_BLOB_LEN as usize {
                        iptc.extend_from_slice(rest);
                    } else {
                        // A run past the cap is dropped WHOLE. A truncated
                        // 8BIM run is malformed, and the export would have
                        // to claim a packet the file does not contain.
                        iptc_over_cap = true;
                    }
                }
            }
            0xEE => {
                if out.adobe_transform.is_none() && payload.starts_with(b"Adobe") {
                    out.adobe_transform = payload.get(11).copied();
                }
            }
            // APP2 ICC_PROFILE is deliberately ignored: the decoder
            // reassembles multi-chunk profiles for every format, so doing it
            // here would be a second implementation.
            _ => {}
        }
        i = start + len - 2;
    }
    if !iptc.is_empty() && !iptc_over_cap {
        capture(&mut out.metadata.iptc, &iptc);
    }
    // The XMP slot holds the LOGICAL packet: the standard one with its
    // extension merged in. `capture` cannot be asked here — the slot is
    // already `Some` — so the cap is checked explicitly.
    let read = merged(out.metadata.xmp.as_deref(), &extension, extension_overflow);
    if let Some(packet) = read.packet {
        if packet.len() <= MAX_DRDC_BLOB_LEN as usize {
            out.metadata.xmp = Some(Arc::from(packet));
        }
    }
    out.resolution = resolve_resolution(
        container,
        out.metadata.exif.as_deref().and_then(resolution_from_exif),
    );
    (out, read.warnings)
}

/// The JFIF APP0 density, after its `JFIF\0` identifier: version (2), units
/// (1), Xdensity (2), Ydensity (2).
fn jfif_density(rest: &[u8]) -> Option<Resolution> {
    let units = *rest.get(2)?;
    let x = f64::from(u16::from_be_bytes([*rest.get(3)?, *rest.get(4)?]));
    let y = f64::from(u16::from_be_bytes([*rest.get(5)?, *rest.get(6)?]));
    if x <= 0.0 || y <= 0.0 || not_stated(x, y) {
        return None;
    }
    match units {
        1 => Some(Resolution {
            x: x as f32,
            y: y as f32,
        }),
        // Units 2 is dots per centimetre.
        2 => Some(Resolution {
            x: (x * 2.54) as f32,
            y: (y * 2.54) as f32,
        }),
        // Units 0 is the pixel-aspect-ratio convention: no resolution.
        _ => None,
    }
}

/// The resolution a file states, when it states it twice: **the EXIF pair
/// wins**, and the container's density is the fallback for a file whose EXIF
/// says nothing (or says it in a unit — "none" — that is a bare aspect
/// ratio).
///
/// Both halves of that rule matter, and they point the same way:
///
/// * When the two AGREE, the EXIF pair is the more precise statement. A
///   container density — the JFIF APP0 pair, a PNG `pHYs` — is an INTEGER
///   count, of dots per inch or of pixels per metre, while
///   XResolution/YResolution are RATIONALs, so only EXIF can spell a
///   fractional ppi: a 150.5 ppi document this crate wrote reopens at 150.5
///   rather than at the 151 its own JFIF header had to round to.
/// * When they DISAGREE, the EXIF pair is the authority the specification
///   names. Exif/DCF makes IFD0's XResolution/YResolution the file's
///   statement of its print resolution — a JFIF APP0 is not even supposed to
///   be present in an Exif file — and it is what ImageIO, ImageMagick and
///   every other reader answers. Believing the container instead opened such
///   a file at the wrong ppi, which now shows up as a wrong print size in
///   Image Size and a wrong layout in Print, and made the app's two
///   "what ppi does this file state" paths (this one and ImageIO's, which
///   the host uses for the formats this crate cannot decode) contradict each
///   other about the same file.
fn resolve_resolution(
    container: Option<Resolution>,
    exif: Option<Resolution>,
) -> Option<Resolution> {
    exif.or(container)
}

/// A container density of exactly 1 x 1 means "not stated", whatever the
/// unit byte says. JFIF units 0 is the pixel-aspect-ratio convention and
/// always carries (1, 1) — it is also `PixelDensity::default()` in the very
/// crate this one encodes with — and that (1, 1) leaks into files written
/// with units 1 as well. Taking it literally makes a 300 ppi scan open
/// claiming a 4000-inch print size, because the sanitizer clamps 1 to 1
/// rather than rejecting it. A `pHYs` of 1 x 1 pixels per metre (0.0254 ppi)
/// is equally meaningless.
fn not_stated(x: f64, y: f64) -> bool {
    x == 1.0 && y == 1.0
}

// ------------------------------------------------------------------ PNG --

fn scan_png(bytes: &[u8]) -> Scan {
    let mut out = Scan::default();
    let mut container = None;
    let mut tried_deflated_xmp = false;
    let mut i = 8usize;
    while i + 12 <= bytes.len() {
        let len = u32::from_be_bytes([bytes[i], bytes[i + 1], bytes[i + 2], bytes[i + 3]]) as usize;
        // A PNG chunk length is at most 2^31 - 1.
        if len > 0x7FFF_FFFF {
            break;
        }
        let Some(end) = i.checked_add(12).and_then(|e| e.checked_add(len)) else {
            break;
        };
        if end > bytes.len() {
            break;
        }
        let kind = &bytes[i + 4..i + 8];
        let data = &bytes[i + 8..i + 8 + len];
        // The CRC is spent only on a chunk this walk would actually KEEP,
        // and that is decided FIRST — by type, then by what the chunk turns
        // out to hold — because the check costs a table-free pass over the
        // chunk's whole data. The walk's bounds come from the length field,
        // not from the CRC, so verifying bytes that are then discarded buys
        // nothing: `IDAT` is essentially the entire file, and a crafted
        // 200 MB `iTXt` (a PNG chunk may be 2 GiB, and `capture` refuses
        // anything over 16 MiB anyway) would otherwise freeze an open for
        // seconds before dropping the blob. A chunk that fails its CRC is
        // not trusted, but the walk goes on: the rest of the file may be
        // fine. A COMPRESSED XMP `iTXt` is inflated at most ONCE per walk,
        // whatever that attempt's outcome, and before its CRC: the output is
        // bounded by the blob cap, so a file's total inflate work is one
        // 16 MiB output at most, where N crafted chunks would otherwise each
        // cost one.
        match kind {
            b"pHYs" if container.is_none() => {
                if let Some(density) = phys_density(data) {
                    if chunk_crc_ok(bytes, i, len, end) {
                        container = Some(density);
                    }
                }
            }
            b"eXIf" if capturable(&out.metadata.exif, data) => {
                if chunk_crc_ok(bytes, i, len, end) {
                    capture(&mut out.metadata.exif, data);
                }
            }
            b"iTXt" => match itxt_xmp(data) {
                Some(PngXmp::Plain(text)) => {
                    if capturable(&out.metadata.xmp, text) && chunk_crc_ok(bytes, i, len, end) {
                        capture(&mut out.metadata.xmp, text);
                    }
                }
                Some(PngXmp::Deflated(stream))
                    if out.metadata.xmp.is_none() && !tried_deflated_xmp =>
                {
                    tried_deflated_xmp = true;
                    if let Ok(text) =
                        crate::psd_zip::inflate_zlib_bounded(stream, MAX_DRDC_BLOB_LEN as usize)
                    {
                        if chunk_crc_ok(bytes, i, len, end) {
                            capture(&mut out.metadata.xmp, &text);
                        }
                    }
                }
                _ => {}
            },
            _ => {}
        }
        if kind == b"IEND" {
            break;
        }
        i = end;
    }
    out.resolution = resolve_resolution(
        container,
        out.metadata.exif.as_deref().and_then(resolution_from_exif),
    );
    out
}

/// True when the chunk starting at `i` (type + data spanning `len` bytes,
/// its stored CRC in the four bytes before `end`) matches its CRC.
fn chunk_crc_ok(bytes: &[u8], i: usize, len: usize, end: usize) -> bool {
    let stored = u32::from_be_bytes([
        bytes[end - 4],
        bytes[end - 3],
        bytes[end - 2],
        bytes[end - 1],
    ]);
    crc32(&bytes[i + 4..i + 8 + len]) == stored
}

/// `pHYs`: x pixels per unit (4), y pixels per unit (4), unit specifier (1),
/// where unit 1 is the metre and 0 means "aspect ratio only".
fn phys_density(data: &[u8]) -> Option<Resolution> {
    if data.len() < 9 || data[8] != 1 {
        return None;
    }
    let x = f64::from(u32::from_be_bytes([data[0], data[1], data[2], data[3]]));
    let y = f64::from(u32::from_be_bytes([data[4], data[5], data[6], data[7]]));
    if x <= 0.0 || y <= 0.0 || not_stated(x, y) {
        return None;
    }
    Some(Resolution {
        x: snapped(x * METRES_PER_INCH),
        y: snapped(y * METRES_PER_INCH),
    })
}

/// How far a reconstructed ppi may sit from a whole number and still be read
/// as that number. A `pHYs` chunk counts pixels per METRE as an integer, so
/// no integral ppi can be stored exactly: 300 ppi is written as 11811 px/m
/// and reads back as 299.9994. The reconstruction error is bounded by half a
/// pixel per metre, i.e. 0.0127 ppi, so a 0.02 window recovers exactly the
/// number that was written and cannot reach a neighbouring integer. A
/// genuinely fractional resolution (299.5) is left alone.
const PPI_SNAP: f64 = 0.02;

fn snapped(ppi: f64) -> f32 {
    let whole = ppi.round();
    if (ppi - whole).abs() <= PPI_SNAP {
        whole as f32
    } else {
        ppi as f32
    }
}

/// The XMP text of an `iTXt` chunk, as stored.
enum PngXmp<'a> {
    /// Compression flag 0: the packet bytes themselves (method byte ignored, as today).
    Plain(&'a [u8]),
    /// Compression flag 1, method 0: a zlib stream that inflates to the packet.
    Deflated(&'a [u8]),
}

/// The text of an `iTXt` chunk whose keyword is `XML:com.adobe.xmp`, plain
/// (compression flag 0) or deflated (flag 1, method 0). `None` for another
/// keyword, any other flag, flag 1 with another method, or a malformed
/// layout. Layout: keyword, NUL, compression flag, compression method,
/// language tag, NUL, translated keyword, NUL, text.
fn itxt_xmp(data: &[u8]) -> Option<PngXmp<'_>> {
    // Matched as a prefix rather than by scanning to the first NUL: the
    // keyword we want has a known length, and a chunk keyed something else
    // is rejected without touching the rest of it, which for a crafted
    // 200 MB `iTXt` is the difference between one byte and a full pass.
    let key_end = PNG_XMP_KEYWORD.len();
    if !data.starts_with(PNG_XMP_KEYWORD) || *data.get(key_end)? != 0 {
        return None;
    }
    // Flag 1 is inflated by the caller (through `psd_zip`, the crate's one
    // inflate seam) only when the method is 0, the one PNG defines.
    let deflated = match *data.get(key_end + 1)? {
        0 => false,
        1 if *data.get(key_end + 2)? == 0 => true,
        _ => return None,
    };
    let after_method = key_end + 3;
    let rest = data.get(after_method..)?;
    let lang_end = rest.iter().position(|&b| b == 0)?;
    let rest = rest.get(lang_end + 1..)?;
    let translated_end = rest.iter().position(|&b| b == 0)?;
    let text = rest.get(translated_end + 1..)?;
    Some(if deflated {
        PngXmp::Deflated(text)
    } else {
        PngXmp::Plain(text)
    })
}

/// CRC-32 (the PNG polynomial), table-free — the ONE implementation, shared
/// by the walk's verification and the chunk builder's stamp.
pub(crate) fn crc32(bytes: &[u8]) -> u32 {
    let mut c: u32 = 0xFFFF_FFFF;
    for &b in bytes {
        c ^= u32::from(b);
        for _ in 0..8 {
            c = if c & 1 != 0 {
                0xEDB8_8320 ^ (c >> 1)
            } else {
                c >> 1
            };
        }
    }
    c ^ 0xFFFF_FFFF
}
