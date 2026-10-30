//! The Bitmap document in the typed `.rz` (#1 Bitmap phase 1a, DRDC 110).
//!
//! Word 110 sits in the CMYK slot — the FIRST word inside the identity
//! wrapper 46 — written iff the document is Bitmap, in place of the Gray
//! word 106 a Bitmap document's Gray policy would otherwise write; a
//! document is never Bitmap and CMYK, Lab or Indexed, so the slot holds one
//! of 82–88 or 106–110 (109 is Duotone's). The body is the Gray format's
//! own (storage tag 1, u16, the twin profile): its one layer is the plane's
//! render. Two tables go where the CMYK table goes, last before `LIDS`: the
//! `GRAY` table unchanged (`gray_native`), then `BTMP`: `"BTMP"`, u32
//! schema 1, u32 width, u32 height, `ceil(width / 8) · height` packed bytes
//! (`bitmap_method`'s layout: MSB first, 1 white, pad bits 0). The reader
//! requires u16 storage, an 8-bit Gray depth, one canvas-sized raster at
//! the origin, no alpha channel, the plane's dimensions equal to the
//! canvas, every pad bit 0 and the body equal to the plane's render
//! ([`check`]), else it refuses by name; the writer runs the same [`check`]
//! first. A reader that knows no 110 meets it where it expects a body
//! version and refuses the file; every earlier file reads unchanged.
//! `DRDC_VERSION` stays 89: 110 is a conditional word like 90 to 108.

use crate::bitmap_document::BitmapPolicy;
use crate::drdc::Reader;
use crate::job_control::JobContext;
use crate::precision_document::{Content, PrecisionDocument};
use crate::precision_native::append;

/// The Bitmap document word.
pub(crate) const WORD: u32 = 110;

/// The `BTMP` table (module doc): tag 4, schema 4, width 4, height 4 and
/// the packed plane; 0 for every other document. The word itself takes the
/// Gray word's place, which `gray_native::header_bytes` already counts.
pub(crate) fn header_bytes(policy: Option<&BitmapPolicy>) -> u64 {
    policy.map_or(0, |p| 16 + p.packed().len() as u64)
}

/// The `BTMP` table's bytes (module doc).
fn table_bytes(policy: &BitmapPolicy) -> Vec<u8> {
    let (width, height) = policy.dimensions();
    let mut out = Vec::with_capacity(header_bytes(Some(policy)) as usize);
    out.extend_from_slice(b"BTMP");
    out.extend_from_slice(&1_u32.to_le_bytes());
    out.extend_from_slice(&width.to_le_bytes());
    out.extend_from_slice(&height.to_le_bytes());
    out.extend_from_slice(policy.packed());
    out
}

/// Writes the `BTMP` table for a Bitmap document; nothing otherwise.
pub(crate) fn encode(
    policy: Option<&BitmapPolicy>,
    out: &mut Vec<u8>,
    bound: u64,
    job: JobContext<'_>,
) -> Result<(), String> {
    match policy {
        Some(policy) => append(out, &table_bytes(policy), bound, job),
        None => Ok(()),
    }
}

/// Reads the table into a policy. A schema, size or pad bit this build does
/// not admit refuses before any pixel is compared.
pub(crate) fn parse(reader: &mut Reader<'_>) -> Result<BitmapPolicy, String> {
    if reader.take(4)? != b"BTMP" || reader.u32()? != 1 {
        return Err("unsupported native Bitmap schema".into());
    }
    let width = reader.u32()?;
    let height = reader.u32()?;
    crate::precision_image::pixel_count(width, height)?;
    let bytes = crate::bitmap_method::stride(width)
        .checked_mul(height as usize)
        .ok_or("native Bitmap plane is too large")?;
    let packed = reader.take(bytes)?.to_vec();
    BitmapPolicy::new(packed, width, height).map_err(|e| format!("native Bitmap document: {e}"))
}

/// The reader's (and writer's) checks of a policy against the document
/// (module doc).
pub(crate) fn check(
    policy: &BitmapPolicy,
    document: &PrecisionDocument,
    job: JobContext<'_>,
) -> Result<(), String> {
    if document.is_float() {
        return Err("native Bitmap document storage must be 16-bit".into());
    }
    let gray8 = document
        .gray_policy()
        .is_some_and(|p| p.depth() == crate::gray_document::GrayDepth::Eight);
    if !gray8 {
        return Err("native Bitmap document must be 8-bit Grayscale".into());
    }
    if !document.channels().is_empty() {
        return Err("native Bitmap document holds no alpha channel".into());
    }
    if policy.dimensions() != document.dimensions() {
        return Err("native Bitmap document plane does not match the canvas".into());
    }
    let image = match document.root_layers() {
        [layer] => match &layer.content {
            Content::Raster {
                image,
                offset: (0, 0),
            } => image,
            _ => return Err("native Bitmap document must hold one raster layer".into()),
        },
        _ => return Err("native Bitmap document must hold one raster layer".into()),
    };
    if !policy.renders(image, job)? {
        return Err("native Bitmap document pixels are not its plane's".into());
    }
    Ok(())
}
