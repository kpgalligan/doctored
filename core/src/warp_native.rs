//! Conditional WARP table. Parse borrowed raw originals before fallback decode.
//!
//! Effective-binding guards and native writers independently ignore stale bases. Conditional native
//! words 53/54 sit inside identity45/46, with borrowed WARP preflight before fallback PNG decode
//! and the table before final LIDS; a warp-free file is pinned against stored 51/52-writer files
//! (`tests/fixtures/rz_byte_51/`, `rz_typed_52/`).
use std::sync::Arc;

use crate::doc::{DrDocument, Layer, LayerKind, LayerRaster, MAX_PIXELS};
use crate::drdc::Reader;
use crate::icc::IccProfile;
use crate::precision_document::{PrecisionDocument, PrecisionDocumentLayer};
use crate::precision_image::PrecisionImage;
use crate::precision_mask::PrecisionPlane;
use crate::warp::{WarpBinding16, WarpBinding8, WarpField, WarpFilter, WarpTarget};
use image::{GrayImage, RgbaImage};

pub(crate) const BYTE_VERSION: u32 = 53;
pub(crate) const PRECISION_VERSION: u32 = 54;

#[derive(Clone, Copy)]
pub(crate) struct Fallback {
    pub pixels: Option<((i32, i32), (u32, u32))>,
    pub mask: Option<MaskFallback>,
}
pub(crate) type MaskFallback = ((i32, i32), (u32, u32), bool, u8);
#[derive(Clone, Copy)]
pub(crate) struct RawPlane<'a> {
    pub offset: (i32, i32),
    pub dimensions: (u32, u32),
    pub bytes: &'a [u8],
}
pub(crate) struct PendingWarp<'a> {
    pub index: usize,
    pub target: WarpTarget,
    pub field: Arc<WarpField>,
    pub linked_mask: bool,
    pub mask_outside: u8,
    pub pixels: Option<RawPlane<'a>>,
    pub mask: Option<RawPlane<'a>>,
}
fn rect(p: RawPlane<'_>) -> (i32, i32, u32, u32) {
    (p.offset.0, p.offset.1, p.dimensions.0, p.dimensions.1)
}
fn put_raw(out: &mut Vec<u8>, offset: (i32, i32), dims: (u32, u32), bytes: &[u8]) {
    out.push(1);
    out.extend_from_slice(&offset.0.to_le_bytes());
    out.extend_from_slice(&offset.1.to_le_bytes());
    out.extend_from_slice(&dims.0.to_le_bytes());
    out.extend_from_slice(&dims.1.to_le_bytes());
    out.extend_from_slice(&(bytes.len() as u64).to_le_bytes());
    out.extend_from_slice(bytes);
}
/// [`put_raw`]'s header with `len = w·h·4`, then the raster's runs' bytes
/// in plane order — the same bytes, no view.
fn put_raw_raster(out: &mut Vec<u8>, offset: (i32, i32), raster: &LayerRaster) {
    let dims = raster.dimensions();
    out.push(1);
    out.extend_from_slice(&offset.0.to_le_bytes());
    out.extend_from_slice(&offset.1.to_le_bytes());
    out.extend_from_slice(&dims.0.to_le_bytes());
    out.extend_from_slice(&dims.1.to_le_bytes());
    out.extend_from_slice(&raster.byte_len().to_le_bytes());
    for run in raster.runs() {
        out.extend_from_slice(run.as_flattened());
    }
}
fn put_header(
    out: &mut Vec<u8>,
    index: usize,
    target: WarpTarget,
    field: &WarpField,
    linked: bool,
    outside: u8,
) -> Result<(), String> {
    out.extend_from_slice(
        &u32::try_from(index)
            .map_err(|_| "warp layer index overflow")?
            .to_le_bytes(),
    );
    out.push(if target == WarpTarget::Pixels { 0 } else { 1 });
    out.push(if field.filter == WarpFilter::Bilinear {
        0
    } else {
        1
    });
    out.push(u8::from(linked));
    out.push(outside);
    out.extend_from_slice(&field.columns.to_le_bytes());
    out.extend_from_slice(&field.rows.to_le_bytes());
    for d in field.displacements.iter() {
        out.extend_from_slice(&d[0].to_le_bytes());
        out.extend_from_slice(&d[1].to_le_bytes());
    }
    Ok(())
}
fn begin(out: &mut Vec<u8>, count: usize) -> Result<(), String> {
    if count == 0 {
        return Ok(());
    }
    out.extend_from_slice(b"WARP");
    out.extend_from_slice(&1u32.to_le_bytes());
    out.extend_from_slice(
        &u32::try_from(count)
            .map_err(|_| "too many warp entries")?
            .to_le_bytes(),
    );
    Ok(())
}
pub(crate) fn byte_entries(doc: &DrDocument) -> Vec<(usize, WarpBinding8)> {
    doc.layers
        .iter()
        .enumerate()
        .filter_map(|(i, l)| l.effective_warp(&doc.profile).map(|b| (i, b)))
        .collect()
}
pub(crate) fn typed_entries(doc: &PrecisionDocument) -> Vec<(usize, WarpBinding16)> {
    (0..doc.layer_count())
        .filter_map(|i| doc.layer(i)?.effective_warp(&doc.profile).map(|b| (i, b)))
        .collect()
}
pub(crate) fn typed_capacity(doc: &PrecisionDocument) -> Result<(u64, u64), String> {
    let entries = typed_entries(doc);
    if entries.is_empty() {
        return Ok((0, 0));
    }
    if doc.is_float() {
        return Err("warp is unavailable at 32-bit float".into());
    }
    let mut encoded = 16u64; // version word, tag, schema, count
    let mut originals = 0u64;
    for (_, b) in entries {
        encoded = encoded
            .checked_add(18 + u64::from(b.field.columns) * u64::from(b.field.rows) * 16)
            .ok_or("WARP capacity overflow")?;
        for (dims, channels) in [
            b.source_pixels.as_ref().map(|p| (p.dimensions(), 4)),
            b.source_mask.as_ref().map(|m| (m.dimensions(), 1)),
        ]
        .into_iter()
        .flatten()
        {
            let n = u64::from(dims.0) * u64::from(dims.1) * channels * 2;
            originals = originals
                .checked_add(n)
                .ok_or("WARP original capacity overflow")?;
            encoded = encoded
                .checked_add(24 + n)
                .ok_or("WARP encoded capacity overflow")?;
        }
    }
    Ok((encoded, originals))
}
pub(crate) fn byte_capacity(doc: &DrDocument) -> Result<(u64, u64), String> {
    let entries = byte_entries(doc);
    if entries.is_empty() {
        return Ok((0, 0));
    }
    let mut encoded = 16u64;
    let mut originals = 0u64;
    for (_, b) in entries {
        encoded = encoded
            .checked_add(18 + u64::from(b.field.columns) * u64::from(b.field.rows) * 16)
            .ok_or("WARP capacity overflow")?;
        for bytes in [
            b.source_pixels.as_ref().map(|p| p.byte_len() as usize),
            b.source_mask.as_ref().map(|m| m.as_raw().len()),
        ]
        .into_iter()
        .flatten()
        {
            originals = originals
                .checked_add(bytes as u64)
                .ok_or("WARP original capacity overflow")?;
            encoded = encoded
                .checked_add(24 + bytes as u64)
                .ok_or("WARP encoded capacity overflow")?;
        }
    }
    Ok((encoded, originals))
}
pub(crate) fn encode_byte(doc: &DrDocument) -> Result<Vec<u8>, String> {
    let entries = byte_entries(doc);
    let mut out = Vec::new();
    begin(&mut out, entries.len())?;
    for (index, b) in entries {
        put_header(
            &mut out,
            index,
            b.target,
            &b.field,
            b.linked_mask,
            b.mask_outside,
        )?;
        if let Some(p) = b.source_pixels {
            put_raw_raster(&mut out, b.source_offset, &p);
        } else {
            out.push(0);
        }
        if let Some(m) = b.source_mask {
            put_raw(&mut out, b.source_mask_offset, m.dimensions(), m.as_raw());
        } else {
            out.push(0);
        }
    }
    Ok(out)
}
pub(crate) fn encode_typed(doc: &PrecisionDocument) -> Result<Vec<u8>, String> {
    let entries = typed_entries(doc);
    let mut out = Vec::new();
    begin(&mut out, entries.len())?;
    for (index, b) in entries {
        put_header(
            &mut out,
            index,
            b.target,
            &b.field,
            b.linked_mask,
            b.mask_outside,
        )?;
        if let Some(p) = b.source_pixels {
            let runs = p.runs_rgba16().ok_or("warp native requires RGBA16")?;
            out.push(1);
            out.extend_from_slice(&b.source_offset.0.to_le_bytes());
            out.extend_from_slice(&b.source_offset.1.to_le_bytes());
            let dims = p.dimensions();
            out.extend_from_slice(&dims.0.to_le_bytes());
            out.extend_from_slice(&dims.1.to_le_bytes());
            out.extend_from_slice(&(u64::from(dims.0) * u64::from(dims.1) * 8).to_le_bytes());
            // Row-major, each run's samples written LE into its grown span
            // (HEAD's byte stream; one resize per run, not per code).
            for run in runs {
                let start = out.len();
                out.resize(start + run.len() * 8, 0);
                for (dst, c) in out[start..].chunks_exact_mut(2).zip(run.as_flattened()) {
                    dst.copy_from_slice(&c.to_le_bytes());
                }
            }
        } else {
            out.push(0);
        }
        if let Some(m) = b.source_mask {
            out.push(1);
            out.extend_from_slice(&b.source_mask_offset.0.to_le_bytes());
            out.extend_from_slice(&b.source_mask_offset.1.to_le_bytes());
            let dims = m.dimensions();
            out.extend_from_slice(&dims.0.to_le_bytes());
            out.extend_from_slice(&dims.1.to_le_bytes());
            out.extend_from_slice(&(m.samples().len() as u64 * 2).to_le_bytes());
            for c in m.samples() {
                out.extend_from_slice(&c.to_le_bytes());
            }
        } else {
            out.push(0);
        }
    }
    Ok(out)
}
fn read_plane<'a>(
    r: &mut Reader<'a>,
    sample_bytes: u64,
    channels: u64,
    decoded_cap: u64,
) -> Result<Option<RawPlane<'a>>, String> {
    let present = r.u8()?;
    if present == 0 {
        return Ok(None);
    }
    if present != 1 {
        return Err("invalid warp plane presence flag".into());
    }
    let offset = (r.i32()?, r.i32()?);
    let dimensions = (r.u32()?, r.u32()?);
    let length = r.u64()?;
    let n = u64::from(dimensions.0)
        .checked_mul(u64::from(dimensions.1))
        .ok_or("warp original area overflow")?;
    let expected = n
        .checked_mul(sample_bytes * channels)
        .ok_or("warp original length overflow")?;
    if dimensions.0 == 0
        || dimensions.1 == 0
        || n > MAX_PIXELS
        || expected != length
        || expected > decoded_cap
        || i64::from(offset.0) + i64::from(dimensions.0) > i64::from(i32::MAX)
        || i64::from(offset.1) + i64::from(dimensions.1) > i64::from(i32::MAX)
    {
        return Err("invalid warp original plane extent or length".into());
    }
    let bytes = r.take(usize::try_from(length).map_err(|_| "warp original length overflow")?)?;
    Ok(Some(RawPlane {
        offset,
        dimensions,
        bytes,
    }))
}
/// Borrows the table's entries and answers the summed bytes of their original
/// planes. `decoded_cap` is the reading format's decoded cap
/// (`drdc::MAX_DRDC_DECODED_BYTES` for a byte file,
/// `precision_native::MAX_DECODED_BYTES` for a typed one): one plane and the
/// running sum each refuse past it.
pub(crate) fn parse<'a>(
    r: &mut Reader<'a>,
    declared: usize,
    sample_bytes: u64,
    decoded_cap: u64,
    fallback: impl Fn(usize) -> Option<Fallback>,
) -> Result<(Vec<PendingWarp<'a>>, u64), String> {
    if r.take(4)? != b"WARP" || r.u32()? != 1 {
        return Err("invalid WARP table tag or schema".into());
    }
    let count = r.u32()? as usize;
    if count == 0 || count > declared {
        return Err("invalid WARP entry count".into());
    }
    let mut entries = Vec::with_capacity(count);
    let mut last = None;
    let mut total = 0u64;
    for _ in 0..count {
        let index = r.u32()? as usize;
        if index >= declared || last.is_some_and(|x| index <= x) {
            return Err("WARP indices must be unique and sorted".into());
        }
        last = Some(index);
        let target = match r.u8()? {
            0 => WarpTarget::Pixels,
            1 => WarpTarget::Mask,
            _ => return Err("invalid WARP target".into()),
        };
        let filter = match r.u8()? {
            0 => WarpFilter::Bilinear,
            1 => WarpFilter::Bicubic,
            _ => return Err("invalid WARP filter".into()),
        };
        let linked_mask = match r.u8()? {
            0 => false,
            1 => true,
            _ => return Err("invalid WARP link flag".into()),
        };
        let mask_outside = r.u8()?;
        if !matches!(mask_outside, 0 | 255) {
            return Err("invalid WARP outside coverage".into());
        }
        let columns = r.u32()?;
        let rows = r.u32()?;
        if !(2..=16).contains(&columns) || !(2..=16).contains(&rows) {
            return Err("invalid WARP lattice dimensions".into());
        }
        let mut values = Vec::with_capacity((columns * rows) as usize);
        for _ in 0..columns * rows {
            values.push([r.f64()?, r.f64()?]);
        }
        let field = Arc::new(WarpField {
            columns,
            rows,
            displacements: Arc::new(values),
            filter,
        });
        field.validate()?;
        if field.identity() {
            return Err("WARP table cannot store an identity field".into());
        }
        let pixels = read_plane(r, sample_bytes, 4, decoded_cap)?;
        let mask = read_plane(r, sample_bytes, 1, decoded_cap)?;
        if pixels.is_some() != (target == WarpTarget::Pixels)
            || mask.is_some() != (target == WarpTarget::Mask || linked_mask)
            || target == WarpTarget::Mask && linked_mask
        {
            return Err("WARP original-plane presence contradicts target".into());
        }
        let fb = fallback(index).ok_or("WARP layer index has no fallback")?;
        if target == WarpTarget::Pixels {
            match fb.mask {
                Some((_, _, linked, outside)) => {
                    if linked != linked_mask || outside != mask_outside {
                        return Err("WARP fallback mask properties differ".into());
                    }
                }
                None if linked_mask => return Err("WARP linked fallback mask missing".into()),
                None => {}
            }
        }
        let source = if target == WarpTarget::Pixels {
            pixels.ok_or("WARP original pixels missing")?
        } else {
            mask.ok_or("WARP original mask missing")?
        };
        field.validate_for_rect(rect(source))?;
        let expected = field.bounds(rect(source))?;
        let actual = if target == WarpTarget::Pixels {
            fb.pixels.ok_or("WARP pixels require a raster fallback")?
        } else {
            let (o, d, _, _) = fb.mask.ok_or("WARP mask fallback missing")?;
            (o, d)
        };
        if actual != ((expected.0, expected.1), (expected.2, expected.3)) {
            return Err("WARP fallback extent differs from field bounds".into());
        }
        if let Some(m) = mask {
            let (offset, dims, linked, outside) = fb.mask.ok_or("WARP mask fallback missing")?;
            if outside != mask_outside || target == WarpTarget::Pixels && linked != linked_mask {
                return Err("WARP fallback mask properties differ".into());
            }
            let expected = field.bounds(rect(m))?;
            if (offset, dims) != ((expected.0, expected.1), (expected.2, expected.3)) {
                return Err("WARP linked mask extent differs from field bounds".into());
            }
        }
        total = total
            .checked_add(pixels.map_or(0, |p| p.bytes.len() as u64))
            .and_then(|n| n.checked_add(mask.map_or(0, |p| p.bytes.len() as u64)))
            .ok_or("WARP original capacity overflow")?;
        if total > decoded_cap {
            return Err("WARP originals exceed native capacity".into());
        }
        entries.push(PendingWarp {
            index,
            target,
            field,
            linked_mask,
            mask_outside,
            pixels,
            mask,
        });
    }
    Ok((entries, total))
}
pub(crate) fn attach_byte(
    entries: Vec<PendingWarp<'_>>,
    layers: &mut [Layer],
    profile: &Arc<IccProfile>,
) -> Result<(), String> {
    for e in entries {
        let layer = layers.get_mut(e.index).ok_or("WARP layer index invalid")?;
        if e.target == WarpTarget::Pixels
            && (layer.kind != LayerKind::Raster
                || layer.meta.is_some()
                || layer.retained_source.is_some())
        {
            return Err("WARP pixel target must be plain raster".into());
        }
        let pixels = e
            .pixels
            .map(|raw| {
                if raw.dimensions == layer.pixels.dimensions()
                    && layer.pixels.equals_raw(raw.dimensions, raw.bytes)
                {
                    Ok(layer.pixels.clone())
                } else {
                    RgbaImage::from_raw(raw.dimensions.0, raw.dimensions.1, raw.bytes.to_vec())
                        .map(LayerRaster::unseeded)
                        .ok_or("invalid WARP RGBA original")
                }
            })
            .transpose()?;
        let mask = e
            .mask
            .map(|p| {
                GrayImage::from_raw(p.dimensions.0, p.dimensions.1, p.bytes.to_vec())
                    .map(Arc::new)
                    .ok_or("invalid WARP mask original")
            })
            .transpose()?;
        let mask =
            if let Some((raw, m)) = e.mask.zip(mask) {
                Some(
                    if layer.mask.as_ref().is_some_and(|v| {
                        v.dimensions() == raw.dimensions && v.as_raw() == m.as_raw()
                    }) {
                        layer.mask.clone().ok_or("WARP mask absent")?
                    } else {
                        m
                    },
                )
            } else {
                None
            };
        layer.warp = Some(WarpBinding8 {
            field: e.field,
            target: e.target,
            source_pixels: pixels,
            source_offset: e.pixels.map_or((0, 0), |p| p.offset),
            source_mask: mask,
            source_mask_offset: e.mask.map_or((0, 0), |p| p.offset),
            rendered_pixels: if e.target == WarpTarget::Pixels {
                Some(layer.pixels.clone())
            } else {
                None
            },
            rendered_offset: if e.target == WarpTarget::Pixels {
                layer.offset
            } else {
                (0, 0)
            },
            rendered_mask: if e.mask.is_some() {
                layer.mask.clone()
            } else {
                None
            },
            rendered_mask_offset: if e.mask.is_some() {
                layer.mask_properties.offset
            } else {
                (0, 0)
            },
            profile: profile.clone(),
            linked_mask: e.linked_mask,
            mask_outside: e.mask_outside,
        });
    }
    Ok(())
}
pub(crate) fn attach_typed(
    entries: Vec<PendingWarp<'_>>,
    layers: &mut Vec<PrecisionDocumentLayer>,
    profile: &Arc<IccProfile>,
) -> Result<(), String> {
    let mut flat = crate::precision_structure_ops::flatten(layers);
    for e in entries {
        let layer = &mut flat
            .get_mut(e.index)
            .ok_or("WARP layer index invalid")?
            .layer;
        if e.target == WarpTarget::Pixels
            && (layer.source().is_none()
                || layer.description.is_some()
                || layer.retained_source.is_some())
        {
            return Err("WARP pixel target must be plain raster".into());
        }
        let pixels = e
            .pixels
            .map(|p| {
                let raw = p
                    .bytes
                    .chunks_exact(8)
                    .map(|b| {
                        [
                            u16::from_le_bytes([b[0], b[1]]),
                            u16::from_le_bytes([b[2], b[3]]),
                            u16::from_le_bytes([b[4], b[5]]),
                            u16::from_le_bytes([b[6], b[7]]),
                        ]
                    })
                    .collect();
                PrecisionImage::from_rgba16_decoded(
                    p.dimensions.0,
                    p.dimensions.1,
                    raw,
                    profile.clone(),
                    None,
                )
            })
            .transpose()?;
        let mask = e
            .mask
            .map(|p| {
                PrecisionPlane::adopt(
                    p.dimensions.0,
                    p.dimensions.1,
                    p.bytes
                        .chunks_exact(2)
                        .map(|b| u16::from_le_bytes([b[0], b[1]]))
                        .collect(),
                )
            })
            .transpose()?;
        let pixels = if let Some((raw, p)) = e.pixels.zip(pixels) {
            Some(
                if layer
                    .source()
                    .is_some_and(|v| v.dimensions() == raw.dimensions && v.same_samples(&p))
                {
                    layer.source().ok_or("WARP pixels absent")?.clone()
                } else {
                    p
                },
            )
        } else {
            None
        };
        let mask = if let Some((raw, m)) = e.mask.zip(mask) {
            Some(
                if layer.mask.as_ref().is_some_and(|v| {
                    v.plane.dimensions() == raw.dimensions && v.plane.samples() == m.samples()
                }) {
                    layer.mask.as_ref().ok_or("WARP mask absent")?.plane.clone()
                } else {
                    m
                },
            )
        } else {
            None
        };
        layer.warp = Some(WarpBinding16 {
            field: e.field,
            target: e.target,
            source_pixels: pixels,
            source_offset: e.pixels.map_or((0, 0), |p| p.offset),
            source_mask: mask,
            source_mask_offset: e.mask.map_or((0, 0), |p| p.offset),
            rendered_pixels: if e.target == WarpTarget::Pixels {
                layer.source().cloned()
            } else {
                None
            },
            rendered_offset: if e.target == WarpTarget::Pixels {
                layer.offset()
            } else {
                (0, 0)
            },
            rendered_mask: if e.mask.is_some() {
                layer.mask.as_ref().map(|m| m.plane.clone())
            } else {
                None
            },
            rendered_mask_offset: if e.mask.is_some() {
                layer.mask.as_ref().map_or((0, 0), |m| m.properties.offset)
            } else {
                (0, 0)
            },
            profile: profile.clone(),
            linked_mask: e.linked_mask,
            mask_outside: e.mask_outside,
        });
    }
    *layers = crate::precision_structure_ops::rebuild(flat)?;
    Ok(())
}
