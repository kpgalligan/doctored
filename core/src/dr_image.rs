//! The flat image handle behind the `dr_image_*` FFI: [`DrImage`], the
//! `DrFormat` mapping, file open (container sniffing, EXIF normalization,
//! the embedded-profile decision `open_profile` makes, the [`Sidecar`] a
//! document lifts), atomic save, and the per-format encoders. (Named
//! `dr_image`, not `image`, to avoid ambiguity with the `image` crate.)

use std::fs::File;
use std::io::{BufWriter, Seek, Write};

use image::codecs::bmp::BmpEncoder;
use image::codecs::gif::GifEncoder;
use image::codecs::jpeg::{JpegEncoder, PixelDensity, PixelDensityUnit};
use image::codecs::png::PngEncoder;
use image::codecs::webp::WebPEncoder;
use image::{ExtendedColorType, ImageDecoder, ImageEncoder, RgbaImage};

use crate::doc::MAX_PIXELS;
use crate::metadata::{self, Metadata, Resolution};
use crate::metadata_write::{EncodeSidecar, MetadataInjector};
use crate::open_profile::{self, CmykContainer, ProfileAction, SourceSamples};

/// Opaque image handle exposed through the FFI. Holds a non-premultiplied
/// RGBA8 buffer (row-major, no row padding).
pub struct DrImage {
    pub(crate) pixels: RgbaImage,
}

/// Everything a decode found BESIDE the pixels: the embedded ICC profile
/// when the open KEPT it (`open_profile`'s `Keep`; a profile the open
/// converted from or ignored is absent, because the pixels are then sRGB's
/// numbers), the metadata packets and the print resolution.
/// `DrDocument::open` lifts it onto the document; `dr_image_open` discards
/// it, because a bare image handle has nowhere to put it and ~40 pure ops
/// would have to state a per-field carry policy on a type with no place to
/// state one.
///
/// The resolution describes the pixels AS RETURNED — upright, with any
/// camera rotation already baked in — so it is transposed for the four
/// quarter-turn orientations rather than echoing the file's stored pair.
#[derive(Default)]
pub(crate) struct Sidecar {
    pub icc: Option<Vec<u8>>,
    pub metadata: Metadata,
    pub resolution: Option<Resolution>,
}

/// Output format, mirroring `DrFormat` in the C header.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Format {
    Png,
    Jpeg,
    Tiff,
    Bmp,
    Gif,
    Webp,
    /// Host-encoded (Phase 11D). No encoder in this crate writes it: the
    /// export workspace produces a TIFF intermediate and the host's ImageIO
    /// encoder transcodes it (`ExportResult::adopt_host_bytes`).
    Heic,
    /// Host-encoded exactly as [`Format::Heic`] is: a TIFF intermediate the
    /// host's ImageIO AVIF encoder transcodes. Export workspace only.
    Avif,
    /// Lossless JPEG XL, encoded here (`jxl_encode`) but only for the export
    /// workspace, which is where its sRGB-only rule is checked. The crate
    /// decodes none, so its preview is the host's.
    Jxl,
    /// OpenEXR, a 32-bit FLOAT deliverable (`exr_float`). Only
    /// the export workspace writes it, from a float document; every flat
    /// writer refuses it, and the flat open refuses its signature so an
    /// OpenEXR file can never arrive clamped to 8 bits.
    Exr,
    /// Radiance RGBE, a 32-bit FLOAT deliverable exactly as [`Format::Exr`]
    /// is (`radiance_hdr`).
    Hdr,
}

impl Format {
    /// Maps a raw `DrFormat` value coming across the FFI.
    pub(crate) fn from_c(value: i32) -> Option<Self> {
        match value {
            0 => Some(Format::Png),
            1 => Some(Format::Jpeg),
            2 => Some(Format::Tiff),
            3 => Some(Format::Bmp),
            4 => Some(Format::Gif),
            5 => Some(Format::Webp),
            6 => Some(Format::Heic),
            7 => Some(Format::Avif),
            8 => Some(Format::Jxl),
            9 => Some(Format::Exr),
            10 => Some(Format::Hdr),
            _ => None,
        }
    }
}

impl DrImage {
    /// Wraps a caller-supplied STRAIGHT (non-premultiplied) RGBA8 buffer —
    /// row-major, row 0 = top, exactly `w * h * 4` bytes — as an image: the
    /// in-memory twin of [`DrImage::open`], for pixels this crate cannot
    /// decode itself (a macOS HEIC still, a host-rasterized vector page). None for
    /// a zero dimension, a size past [`MAX_PIXELS`], or a buffer whose length
    /// disagrees with the dimensions.
    pub(crate) fn from_rgba(w: u32, h: u32, data: Vec<u8>) -> Option<Self> {
        if w == 0 || h == 0 || u64::from(w) * u64::from(h) > MAX_PIXELS {
            return None;
        }
        RgbaImage::from_raw(w, h, data).map(|pixels| DrImage { pixels })
    }

    /// Reads and decodes the file at `path`, discarding its [`Sidecar`].
    /// The thin wrapper over [`DrImage::open_bytes`], which is the ONE
    /// decoder — so a Gray- or CMYK-profiled file comes back converted to
    /// sRGB here too, with no report, and a CMYK file the open's gate
    /// refuses is an `Err` here too.
    pub(crate) fn open(path: &str) -> Result<Self, String> {
        let bytes = std::fs::read(path).map_err(|e| format!("failed to read {path}: {e}"))?;
        Ok(DrImage::open_bytes(&bytes, path)?.0)
    }

    /// Decodes `bytes` (already read from `path`, which is used only in
    /// error messages) and returns the image plus everything else the file
    /// carried. Buffers starting with the `8BPS` magic are decoded as
    /// Photoshop documents and flattened to their composite image;
    /// everything else goes through content sniffing in the `image` crate.
    ///
    /// PSD composites use the shared compatibility decoder and retain their
    /// RGB ICC/PPI sidecar; capture packets remain outside this import slice.
    ///
    /// Everything else applies `open_profile::decide`: a grayscale
    /// image with a Gray profile and a CMYK TIFF or CMYK/YCCK JPEG with a
    /// CMYK profile are converted from that profile to sRGB as they are read;
    /// an untagged CMYK TIFF or JPEG is converted the same way from the
    /// working CMYK profile (`working_cmyk`), or read naively as sRGB
    /// when none is set; any other profile a document cannot store is
    /// ignored and the numbers read as sRGB, exactly as before. The import
    /// report names all three
    /// ([`embedded_profile_report`]). Every CMYK source must first pass
    /// `open_profile::cmyk_gate`, whatever the decision: a CMYK file
    /// the gate refuses is an `Err`, so the host's platform decode reads it.
    ///
    /// An OpenEXR or Radiance RGBE signature is refused by name: both are
    /// float containers that open only through `open_preserving`, and
    /// `image`'s own decoders would hand them over clamped to 8 bits.
    pub(crate) fn open_bytes(bytes: &[u8], path: &str) -> Result<(Self, Sidecar), String> {
        if crate::exr_float::is_exr(bytes) {
            return Err(format!("failed to decode {path}: {EXR_FLOAT_OPEN}"));
        }
        if crate::radiance_hdr::is_radiance(bytes) {
            return Err(format!("failed to decode {path}: {HDR_FLOAT_OPEN}"));
        }
        if bytes.len() >= 4 && &bytes[..4] == b"8BPS" {
            let container = crate::psd_container::Container::read(bytes)
                .map_err(|e| format!("PSD {path}: {e}"))?;
            let pixels = container
                .composite()
                .map_err(|e| format!("PSD {path}: {e}"))?;
            let profile = container
                .profile()
                .map_err(|e| format!("PSD {path}: {e}"))?;
            let resolution = container
                .resolution()
                .map_err(|e| format!("PSD {path}: {e}"))?;
            return Ok((
                DrImage { pixels },
                Sidecar {
                    icc: profile.map(|p| p.bytes().to_vec()),
                    resolution,
                    metadata: Metadata::default(),
                },
            ));
        }
        let decode_err = |e: image::ImageError| format!("failed to decode {path}: {e}");
        // A CMYK TIFF `image` cannot decode — transparency, extra samples,
        // planar (#1 phase 3) — is read by THE CMYK TIFF reader alone.
        if open_profile::cmyk_tiff_beyond_image(bytes) {
            return cmyk_tiff_flat(bytes, path);
        }
        let mut decoder = match flat_decoder(bytes, path) {
            Ok(decoder) => decoder,
            // `tiff` refuses a planar file short of a strip set a sample
            // before it knows the colour type; the CMYK gate's sentence
            // names it.
            Err(_) if crate::cmyk_tiff_read::planar_shortfall(bytes) => {
                return Err(format!(
                    "failed to decode {path}: {}",
                    crate::cmyk_tiff_read::PLANAR_STRIPS
                ));
            }
            Err(error) => return Err(error),
        };
        // Normalize camera-tagged rotation (EXIF) into the pixels at open
        // time. A preserved EXIF packet therefore CANNOT be written back
        // unchanged — `metadata_write::exif_normalized` resets its
        // Orientation to 1 on every save, which is what keeps this safe.
        let orientation = crate::metadata_tiff::orientation_of_file(bytes).unwrap_or_else(|| {
            decoder
                .orientation()
                .unwrap_or(image::metadata::Orientation::NoTransforms)
        });
        let mut scan = metadata::scan(bytes);
        // The last questions the live decoder answers before it is consumed:
        // its profile (a corrupt one is absence, not a failure to open the
        // file) and its sample kind, which together decide what the open
        // does with the colour (`open_profile`).
        let (icc, source) = embedded_color(&mut decoder, bytes, scan.icc.take());
        if let SourceSamples::Cmyk(container) = source {
            open_profile::cmyk_gate(bytes, path, container)?;
        }
        let decision = open_profile::decide(icc.as_deref(), source);
        let pixels = match &decision.action {
            ProfileAction::ConvertCmyk(profile, container) => {
                // `image` has already turned CMYK into RGB naively by the
                // time it hands samples over, so the file's own samples are
                // read again — with `tiff`, or with `zune-jpeg` asked for
                // the raw planes.
                drop(decoder);
                let rgba = match container {
                    CmykContainer::Tiff => open_profile::cmyk_tiff_to_srgb8(bytes, path, profile)?,
                    CmykContainer::Jpeg => open_profile::cmyk_jpeg_to_srgb8(
                        bytes,
                        path,
                        profile,
                        scan.adobe_transform.is_some(),
                    )?,
                };
                let mut img = image::DynamicImage::ImageRgba8(rgba);
                img.apply_orientation(orientation);
                img.into_rgba8()
            }
            ProfileAction::ConvertGray(curve) => {
                let mut img = image::DynamicImage::from_decoder(decoder).map_err(decode_err)?;
                img.apply_orientation(orientation);
                open_profile::gray_to_srgb8(&img, curve, path)?
            }
            ProfileAction::Keep | ProfileAction::Ignore => {
                let mut img = image::DynamicImage::from_decoder(decoder).map_err(decode_err)?;
                img.apply_orientation(orientation);
                img.to_rgba8()
            }
        };
        // The file states its resolution for the picture as STORED; the
        // pixels above are the picture as DISPLAYED, so a quarter turn
        // swaps the two axes — a 300 x 150 ppi frame stored sideways is
        // 150 x 300 once it is upright. This is the same rule
        // `DrDocument::geometry` applies to `rotate90`, through the same
        // `Resolution::transposed_if`; the two would otherwise disagree
        // about one operation.
        let resolution = scan
            .resolution
            .map(|r| r.transposed_if(quarter_turned(orientation)));
        // A JPEG's 8BIM run keeps no path record through an open that
        // turned it (`tiff_paths::jpeg_opened`; the import report gives the
        // sentence). A TIFF's are `DrDocument::with_tiff_paths`'.
        let metadata = if crate::metadata_tiff::is_tiff(bytes) {
            scan.metadata
        } else {
            crate::tiff_paths::jpeg_opened(scan.metadata, orientation.to_exif()).0
        };
        Ok((
            DrImage { pixels },
            Sidecar {
                // Only a kept profile is lifted: converted pixels are sRGB's
                // numbers, and ignored ones are read as sRGB.
                icc: match decision.action {
                    ProfileAction::Keep => icc,
                    _ => None,
                },
                metadata,
                resolution,
            },
        ))
    }

    /// Encodes the image to `path` in `format`, atomically via
    /// [`save_atomically`], so a failed save never truncates or deletes an
    /// existing destination file. Attaches nothing: a bare image has no
    /// profile and no packets, so this produces exactly the bytes it always
    /// did. The document-level twin is `DrDocument::save_image`.
    pub(crate) fn save(&self, path: &str, format: Format, jpeg_quality: u8) -> Result<(), String> {
        save_flat(
            &self.pixels,
            path,
            format,
            jpeg_quality,
            &EncodeSidecar::NONE,
        )
    }
}

/// The flat open of a CMYK TIFF `image`'s decoder does not read
/// (`open_profile::cmyk_tiff_beyond_image`): converted from its embedded (or
/// the working) CMYK profile by `open_profile::cmyk_tiff_to_srgb8`, its
/// transparency kept. With no profile to convert from it is an `Err`, so the
/// host's platform decode reads it, as before #1 phase 3.
fn cmyk_tiff_flat(bytes: &[u8], path: &str) -> Result<(DrImage, Sidecar), String> {
    let mut scan = metadata::scan(bytes);
    let icc = scan.icc.take();
    let decision = open_profile::decide(icc.as_deref(), SourceSamples::Cmyk(CmykContainer::Tiff));
    let ProfileAction::ConvertCmyk(profile, _) = &decision.action else {
        return Err(format!("failed to decode {path}: a CMYK TIFF with transparency, extra samples or planes is read here only through a CMYK profile it converts from"));
    };
    let orientation = crate::metadata_tiff::orientation_of_file(bytes)
        .unwrap_or(image::metadata::Orientation::NoTransforms);
    let mut img =
        image::DynamicImage::ImageRgba8(open_profile::cmyk_tiff_to_srgb8(bytes, path, profile)?);
    img.apply_orientation(orientation);
    let resolution = scan
        .resolution
        .map(|r| r.transposed_if(quarter_turned(orientation)));
    Ok((
        DrImage {
            pixels: img.into_rgba8(),
        },
        Sidecar {
            icc: None,
            metadata: scan.metadata,
            resolution,
        },
    ))
}

/// The `image` decoder behind every flat open, chosen by content sniffing.
/// A TIFF (classic or BigTIFF, `metadata_tiff::is_tiff`) is named
/// explicitly: `image`'s own sniffing does not recognise the BigTIFF magic.
fn flat_decoder<'a>(bytes: &'a [u8], path: &str) -> Result<impl ImageDecoder + 'a, String> {
    let reader = if crate::metadata_tiff::is_tiff(bytes) {
        image::ImageReader::with_format(std::io::Cursor::new(bytes), image::ImageFormat::Tiff)
    } else {
        image::ImageReader::new(std::io::Cursor::new(bytes))
            .with_guessed_format()
            .map_err(|e| format!("failed to read {path}: {e}"))?
    };
    reader
        .into_decoder()
        .map_err(|e| format!("failed to decode {path}: {e}"))
}

/// The embedded profile — the container walk's (`scan_icc`, TIFF only) ahead
/// of the decoder's, which reassembles JPEG APP2 chunks and inflates PNG
/// `iCCP` — and what kind of samples the file holds (`SourceSamples::of`).
/// `.ok().flatten()`, never `?`: a corrupt profile is absence, not a failure
/// to open the file.
fn embedded_color(
    decoder: &mut impl ImageDecoder,
    bytes: &[u8],
    scan_icc: Option<Vec<u8>>,
) -> (Option<Vec<u8>>, SourceSamples) {
    let decoded = decoder.icc_profile().ok().flatten();
    (scan_icc.or(decoded), SourceSamples::of(decoder, bytes))
}

/// The import report's `profile_refusal` for `bytes`: the sentence for a
/// profile the flat open converts from or ignores (`open_profile::decide`),
/// including an untagged CMYK file's (converted from the working CMYK
/// profile, or read naively when none is set), from HEADERS only — no
/// pixels are decoded. `None` for a `.rz` or PSD file (neither goes through
/// the flat open), for a file the flat decoder cannot read, for a kept
/// profile or an absent one over RGB or gray samples, and for a CMYK file the
/// open's gate refuses — the open fails there, so there is no
/// core open to describe; the host's platform decode words its own
/// conversion (`open_profile::platform_conversion_sentence`). `scan_icc` is
/// the container walk's profile, as [`DrImage::open_bytes`] reads it, ahead
/// of the flat decoder's.
pub(crate) fn embedded_profile_report(bytes: &[u8], scan_icc: Option<Vec<u8>>) -> Option<String> {
    if crate::native_magic::is_native_magic(bytes) || bytes.starts_with(b"8BPS") {
        return None;
    }
    if let Some(container) = open_profile::cmyk_container(bytes) {
        if !open_profile::cmyk_convertible(bytes, container) {
            return None;
        }
    }
    // The CMYK TIFF `image` cannot decode opens through its own reader
    // (`cmyk_tiff_flat`), and only when it converts from a CMYK profile.
    if open_profile::cmyk_tiff_beyond_image(bytes) {
        let decision = open_profile::decide(
            scan_icc.as_deref(),
            SourceSamples::Cmyk(CmykContainer::Tiff),
        );
        return match decision.action {
            ProfileAction::ConvertCmyk(..) => decision.sentence,
            _ => None,
        };
    }
    let mut decoder = flat_decoder(bytes, "the file").ok()?;
    let (icc, source) = embedded_color(&mut decoder, bytes, scan_icc);
    open_profile::decide(icc.as_deref(), source).sentence
}

/// The profile embedded in an encoded flat image, header work only: the
/// container walk's (`metadata::scan`), else the flat decoder's — the
/// order [`DrImage::open_bytes`] reads them in. `None` for a `.rz` or PSD
/// file, an unreadable one and one with no profile. A CMYK+alpha or planar
/// CMYK TIFF never reaches `image`'s decoder, so the walk's is the only
/// profile it has.
pub(crate) fn embedded_icc(bytes: &[u8]) -> Option<Vec<u8>> {
    if crate::native_magic::is_native_magic(bytes) || bytes.starts_with(b"8BPS") {
        return None;
    }
    metadata::scan(bytes).icc.or_else(|| {
        flat_decoder(bytes, "the file")
            .ok()
            .and_then(|mut decoder| decoder.icc_profile().ok().flatten())
    })
}

/// The one sentence every flat writer refuses [`Format::Heic`] with. HEIC
/// has no encoder in this crate: the export workspace encodes a TIFF
/// intermediate and the host transcodes it, so a flat save that reached an
/// encoder here would be a silently wrong file rather than a missing one.
pub(crate) const HEIC_HOST_ONLY: &str =
    "HEIC is encoded by the host through the export workspace (format \"heic\")";

/// The same refusal for [`Format::Avif`], host-encoded the same way.
pub(crate) const AVIF_HOST_ONLY: &str =
    "AVIF is encoded by the host through the export workspace (format \"avif\")";

/// The flat writers' refusal of [`Format::Jxl`]: a JPEG XL codestream carries
/// no profile, and only the export workspace checks that the pixels are sRGB.
pub(crate) const JXL_EXPORT_ONLY: &str =
    "JPEG XL is written through the export workspace (format \"jxl\")";

/// The flat writers' refusal of [`Format::Exr`]: its samples are a float
/// document's, which no 8-bit writer holds.
pub(crate) const EXR_EXPORT_ONLY: &str =
    "OpenEXR is a 32-bit float deliverable written through the export workspace (format \"exr\")";

/// The same refusal for [`Format::Hdr`].
pub(crate) const HDR_EXPORT_ONLY: &str =
    "Radiance HDR is a 32-bit float deliverable written through the export workspace (format \"hdr\")";

/// The flat open's refusal of an OpenEXR signature.
pub(crate) const EXR_FLOAT_OPEN: &str = "an OpenEXR file opens only as a 32-bit float document";

/// The flat open's refusal of a Radiance RGBE signature.
pub(crate) const HDR_FLOAT_OPEN: &str = "a Radiance HDR file opens only as a 32-bit float document";

/// True for the four EXIF orientations (5-8) that turn the picture a
/// quarter of a turn, so the stored width becomes the displayed height.
fn quarter_turned(orientation: image::metadata::Orientation) -> bool {
    use image::metadata::Orientation::*;
    matches!(
        orientation,
        Rotate90 | Rotate270 | Rotate90FlipH | Rotate270FlipH
    )
}

/// Encodes `pixels` to `path`, splicing in whatever `side` carries. The ONE
/// flat writer: [`DrImage::save`] passes [`EncodeSidecar::NONE`] and
/// `DrDocument::save_image` passes the document's profile, packets and
/// resolution.
///
/// The splice lives INSIDE the [`save_atomically`] closure, so atomicity is
/// preserved and there is no second helper; and there is exactly one
/// `match format` in the crate that dispatches an encoder.
pub(crate) fn save_flat(
    pixels: &RgbaImage,
    path: &str,
    format: Format,
    jpeg_quality: u8,
    side: &EncodeSidecar<'_>,
) -> Result<(), String> {
    match format {
        Format::Heic => return Err(HEIC_HOST_ONLY.into()),
        Format::Avif => return Err(AVIF_HOST_ONLY.into()),
        Format::Jxl => return Err(JXL_EXPORT_ONLY.into()),
        Format::Exr => return Err(EXR_EXPORT_ONLY.into()),
        Format::Hdr => return Err(HDR_EXPORT_ONLY.into()),
        _ => {}
    }
    save_atomically(path, |tmp_path| {
        let file = File::create(tmp_path).map_err(|e| format!("failed to create {path}: {e}"))?;
        let writer = BufWriter::new(file);
        // Only JPEG and PNG are ever spliced, and only when there is
        // something to splice: with nothing extra to write the injector is a
        // pass-through, which is also the ONLY mode a seeking encoder (TIFF)
        // may be given.
        let extra = match format {
            Format::Jpeg => side.jpeg_segments(),
            Format::Png => side.png_chunks(),
            _ => Vec::new(),
        };
        let mut injector = match format {
            _ if extra.is_empty() => MetadataInjector::passthrough(writer),
            Format::Jpeg => MetadataInjector::jpeg(writer, extra),
            Format::Png => MetadataInjector::png(writer, extra),
            _ => MetadataInjector::passthrough(writer),
        };
        encode_into(pixels, &mut injector, format, jpeg_quality, side)?;
        let mut writer = injector.finish()?;
        writer
            .flush()
            .map_err(|e| format!("failed to write {path}: {e}"))
    })
}

/// The per-format encoders — the ONE dispatch.
///
/// `W: Write + Seek` rather than `Write` because `TiffEncoder` is
/// `impl<W: Write + Seek>` (the `tiff` crate seeks to backpatch IFD
/// offsets), while PNG, JPEG, BMP, GIF and WebP need only `Write`.
/// `BufWriter<File>` satisfies both, and `MetadataInjector`'s `Seek` impl
/// refuses a seek on a spliced stream rather than corrupting the file.
pub(crate) fn encode_into<W: Write + Seek>(
    pixels: &RgbaImage,
    writer: &mut W,
    format: Format,
    jpeg_quality: u8,
    side: &EncodeSidecar<'_>,
) -> Result<(), String> {
    let (w, h) = pixels.dimensions();
    let raw = pixels.as_raw();
    let err = |e: image::ImageError| format!("encoding failed: {e}");
    let unsupported = |e: image::error::UnsupportedError| format!("encoding failed: {e}");
    match format {
        Format::Png => {
            let mut enc = PngEncoder::new(writer);
            if let Some(icc) = side.icc {
                enc.set_icc_profile(icc.to_vec()).map_err(unsupported)?;
            }
            if let Some(exif) = &side.exif {
                enc.set_exif_metadata(exif.clone()).map_err(unsupported)?;
            }
            enc.write_image(raw, w, h, ExtendedColorType::Rgba8)
                .map_err(err)
        }
        Format::Jpeg => {
            // Alpha is flattened over white BEFORE encoding, on the
            // document's own pixels, and the profile is embedded unchanged:
            // white is white in every RGB space this crate supports, so no
            // conversion belongs around this and adding one would be wrong.
            let rgb = composite_over_white(pixels);
            let mut enc = JpegEncoder::new_with_quality(writer, jpeg_quality.clamp(1, 100));
            if let Some(icc) = side.icc {
                enc.set_icc_profile(icc.to_vec()).map_err(unsupported)?;
            }
            if let Some(exif) = &side.exif {
                enc.set_exif_metadata(exif.clone()).map_err(unsupported)?;
            }
            if let Some(dpi) = side.dpi {
                // Per axis: `PixelDensity::dpi` takes ONE u16 and could not
                // express a 300 x 150 ppi document.
                enc.set_pixel_density(PixelDensity {
                    density: (clamp_jfif_density(dpi.x), clamp_jfif_density(dpi.y)),
                    unit: PixelDensityUnit::Inches,
                });
            }
            enc.write_image(rgb.as_raw(), w, h, ExtendedColorType::Rgb8)
                .map_err(err)
        }
        Format::Tiff => crate::tiff_encode::encode(pixels, writer, side),
        Format::Bmp => BmpEncoder::new(writer)
            .write_image(raw, w, h, ExtendedColorType::Rgba8)
            .map_err(err),
        Format::Gif => {
            let mut encoder = GifEncoder::new_with_speed(writer, 10);
            encoder
                .encode(raw, w, h, ExtendedColorType::Rgba8)
                .map_err(err)
        }
        Format::Webp => {
            let mut enc = WebPEncoder::new_lossless(writer);
            if let Some(icc) = side.icc {
                enc.set_icc_profile(icc.to_vec()).map_err(unsupported)?;
            }
            if let Some(exif) = &side.exif {
                enc.set_exif_metadata(exif.clone()).map_err(unsupported)?;
            }
            enc.write_image(raw, w, h, ExtendedColorType::Rgba8)
                .map_err(err)
        }
        Format::Heic => Err(HEIC_HOST_ONLY.into()),
        Format::Avif => Err(AVIF_HOST_ONLY.into()),
        // No 8-bit encoder writes a float container (`doc_export_float`).
        Format::Exr => Err(EXR_EXPORT_ONLY.into()),
        Format::Hdr => Err(HDR_EXPORT_ONLY.into()),
        // Reached only from the export workspace's encode stage; the flat
        // writers refused above.
        Format::Jxl => writer
            .write_all(&crate::jxl_encode::encode_rgba8(pixels)?)
            .map_err(|e| format!("encoding failed: {e}")),
    }
}

/// The JFIF density field is a u16 per axis. The document's own resolution
/// is already sanitized to [1, 30000], so this rounds and bounds rather than
/// corrects.
pub(crate) fn clamp_jfif_density(ppi: f32) -> u16 {
    f64::from(ppi).round().clamp(1.0, f64::from(u16::MAX)) as u16
}

/// Writes a file atomically — the ONE helper behind every save path
/// (`save_flat` here, `DrDocument::save_native` in `drdc`). `write`
/// produces the content at a temporary path in the same directory (pid plus
/// a process-wide sequence number keep concurrent saves distinct), which is
/// renamed over `path` only on success, so a failed save never truncates or
/// deletes an existing destination file; the temporary file is removed on
/// failure. Every core file writer publishes atomically and durably: temp
/// file, F_BARRIERFSYNC, rename, then a best-effort F_FULLFSYNC of the parent
/// directory (`durable_file`).
pub(crate) fn save_atomically(
    path: &str,
    write: impl FnOnce(&str) -> Result<(), String>,
) -> Result<(), String> {
    static SAVE_SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let seq = SAVE_SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let tmp_path = format!("{path}.dr-tmp-{}-{seq}", std::process::id());
    let result = write(&tmp_path)
        .and_then(|()| {
            std::fs::File::open(&tmp_path)
                .and_then(|file| crate::durable_file::order_file(&file))
                .map_err(|e| format!("failed to flush {path}: {e}"))
        })
        .and_then(|()| {
            std::fs::rename(&tmp_path, path).map_err(|e| format!("failed to write {path}: {e}"))
        });
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp_path);
    } else {
        crate::durable_file::persist_parent_dir(path);
    }
    result
}

/// Alpha-composites the image over an opaque white background, producing RGB8
/// (used for JPEG output, which has no alpha channel).
pub(crate) fn composite_over_white(img: &RgbaImage) -> image::RgbImage {
    let (w, h) = img.dimensions();
    let mut out = image::RgbImage::new(w, h);
    for (src, dst) in img.pixels().zip(out.pixels_mut()) {
        let alpha = f32::from(src[3]) / 255.0;
        for (d, &s) in dst.0.iter_mut().zip(src.0.iter()) {
            *d = (f32::from(s) * alpha + 255.0 * (1.0 - alpha)).round() as u8;
        }
    }
    out
}
