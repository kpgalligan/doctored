//! Shared immutable asset identity, exact-depth codecs and native size accounting.
use super::{
    append, put_asset_header, put_bindings, valid_id, validate_binding, Binding, PendingSources,
    BYTE_LINK_VERSION, BYTE_RAW_VERSION, BYTE_SOURCE_VERSION, PRECISION_LINK_VERSION,
    PRECISION_RAW_VERSION, PRECISION_SOURCE_VERSION,
};
use crate::drdc::{MAX_DRDC_DECODED_BYTES, MAX_DRDC_PNG_LEN};
use crate::icc::IccProfile;
use crate::job_control::JobContext;
use crate::native_stream::{NativeSink, Payload, RasterRef};
use crate::precision_codec::{
    decode_native_png, inspect_native_payload, native_png_band_scratch, native_png_decode_scratch,
    native_png_encoded_bound, put_native_png, NATIVE_FIXED,
};
use crate::precision_image::pixel_count;
use crate::retained_source::{SourceAsset, SourceLink, SourcePixels};
use image::codecs::png::PngDecoder;
use image::ImageDecoder;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

/// Adds one embedded original to `total`, the aggregate decoded bytes of its
/// file, and refuses past `cap`: the owner format's decoded cap, the byte
/// format's 1.6 GB (`drdc::MAX_DRDC_DECODED_BYTES`) or the typed format's
/// 8 GiB (`precision_native::MAX_DECODED_BYTES`). Each plane counts at
/// its actual sample width. In byte documents an original u16 pixel costs two
/// RGBA8 pixel units.
/// A 32-bit float master costs its exact 16 B/px; neither it
/// nor a 16-bit master (the banded reader) passes through an `image`
/// decoder, so the `image::Limits` cap applies to 8-bit originals only:
/// `pixel_count` bounds the others.
pub(crate) fn source_plane_capacity(
    dimensions: (u32, u32),
    depth: u8,
    total: &mut u64,
    cap: u64,
) -> Result<(), String> {
    if !matches!(depth, 8 | 16 | 32) {
        return Err("unsupported native source depth".into());
    }
    let bytes = pixel_count(dimensions.0, dimensions.1)? as u64 * (u64::from(depth) / 8) * 4;
    if image::Limits::default()
        .max_alloc
        .is_some_and(|cap| depth == 8 && bytes > cap)
    {
        return Err(too_large_to_keep(dimensions, depth));
    }
    *total = total
        .checked_add(bytes)
        .ok_or("native source pixel byte overflow")?;
    if *total > cap {
        return Err("embedded sources and layer fallbacks exceed the native pixel limit".into());
    }
    Ok(())
}

/// The one sentence an image too large to keep at `depth` earns: an 8-bit
/// image past the decoder allocation cap, or any image past the native PNG
/// record bound. The megapixel figure is the cap that binds at that
/// depth: the decoder's 512 MiB at 8 bits, the 1 GiB record limit at 16 —
/// about 134 megapixels either way, past `MAX_PIXELS`, so
/// `pixel_count` refuses every such extent first.
fn too_large_to_keep(dimensions: (u32, u32), depth: u8) -> String {
    let cap = if depth == 8 {
        image::Limits::default().max_alloc.unwrap_or(u64::MAX)
    } else {
        u64::from(MAX_DRDC_PNG_LEN)
    };
    format!(
        "A {} \u{d7} {} image is too large to keep at {depth} bits: one image holds at most about {} megapixels at that depth, so closing other documents will not help. Use a smaller image.",
        dimensions.0,
        dimensions.1,
        cap / (u64::from(depth) / 8 * 4) / 1_000_000
    )
}

/// Whether ONE image of `dimensions` at `depth` (8 or 16) fits what a
/// document keeps of a single image: [`source_plane_capacity`] with nothing
/// else counted (the pixel limit, and at 8 bits the decoder allocation cap)
/// and the native PNG record bound (`MAX_DRDC_PNG_LEN`, 1 GiB). The cap it
/// passes is the byte format's: one plane is at most 1.6 GB, so the typed
/// cap would answer the same. The only
/// one-image check: host stills and 16-bit file opens ask it before decoding.
/// Every extent within `MAX_PIXELS` fits at either depth
/// (a 100 MP 16-bit record is about 800 MB), so `Err` is `pixel_count`'s
/// sentence or the unsupported-depth error; the too-large sentence stays for
/// a cap lowered below the pixel limit.
pub(crate) fn single_image_capacity(dimensions: (u32, u32), depth: u8) -> Result<(), String> {
    source_plane_capacity(dimensions, depth, &mut 0, MAX_DRDC_DECODED_BYTES)?;
    if png_bound(dimensions, depth)? > u64::from(MAX_DRDC_PNG_LEN) {
        return Err(too_large_to_keep(dimensions, depth));
    }
    Ok(())
}

pub(crate) fn png_bound(dimensions: (u32, u32), depth: u8) -> Result<u64, String> {
    if depth == 16 {
        native_png_encoded_bound(dimensions)
    } else if depth == 8 {
        // RGBA8 contributes four data bytes plus a conservative byte per pixel
        // for filters/deflate expansion, with the established 1 MiB fixed margin.
        Ok(pixel_count(dimensions.0, dimensions.1)? as u64 * 5 + 1024 * 1024)
    } else if depth == 32 {
        // A float master's raw payload is exact: 16 bytes per pixel.
        Ok(pixel_count(dimensions.0, dimensions.1)? as u64 * 16)
    } else {
        Err("unsupported native source depth".into())
    }
}

/// An embedded 8-bit source's pixel field: the u64 length, then the RGBA8
/// PNG of `rows` (`png_rows8`, 13C-2a), at most its [`png_bound`] and
/// `MAX_DRDC_PNG_LEN`, announced as one payload. The ONE encoder of it,
/// shared by both native writers and the tile pool's rebuild.
pub(crate) fn put_source_png8(
    out: &mut dyn NativeSink,
    (width, height): (u32, u32),
    rows: &mut crate::png_rows8::Rows<'_>,
    job: JobContext<'_>,
) -> Result<(), String> {
    let record = crate::png_rows8::Record {
        prefix: 8,
        cap: png_bound((width, height), 8)?.min(u64::from(MAX_DRDC_PNG_LEN)),
        too_large: "embedded source PNG exceeds its byte limit",
        blob: true,
    };
    let png = crate::png_rows8::Png8 {
        width,
        height,
        channels: 4,
        after_ihdr: &[],
        before_iend: &[],
    };
    crate::png_rows8::put_png8_record(out, &record, &png, rows, job).map(|_| ())
}

/// The typed plan's charge for an 8-bit original's encode beside the file:
/// the former 48 B/px figure, an over-count since the original streams row by
/// row ([`put_source_png8`]), kept until a measurement lowers it, and charged
/// only for 8-bit originals.
pub(crate) fn byte_source_png_encode_scratch(dimensions: (u32, u32)) -> Result<u64, String> {
    Ok(pixel_count(dimensions.0, dimensions.1)? as u64 * 48
        + 16 * u64::from(dimensions.0)
        + NATIVE_FIXED)
}

pub(crate) struct SourceTable {
    pub assets: Vec<Arc<SourceAsset>>,
    pub bindings: Vec<Binding>,
    owner_depth: u8,
    pointers: HashMap<usize, usize>,
    keys: HashSet<(String, String)>,
    profile_bytes: u64,
    raw: super::raw::RawTable,
    binary: super::binary::ResourceTable,
}
impl SourceTable {
    /// Every owner admits a depth-32 master: a float, 8- or 16-bit document.
    pub fn new(owner_depth: u8) -> Self {
        Self {
            assets: Vec::new(),
            bindings: Vec::new(),
            owner_depth,
            pointers: HashMap::new(),
            keys: HashSet::new(),
            profile_bytes: 0,
            raw: super::raw::RawTable::default(),
            binary: super::binary::ResourceTable::default(),
        }
    }
    pub fn push(&mut self, asset: Arc<SourceAsset>, mut binding: Binding) -> Result<(), String> {
        if let Some(link) = &binding.link {
            SourceLink::validate_parts(&link.path, &link.sha256)?;
        }
        let pointer = Arc::as_ptr(&asset) as usize;
        let next = self.assets.len();
        let index = *self.pointers.entry(pointer).or_insert(next);
        if index == next {
            if !valid_id(&asset.id)
                || !valid_id(&asset.revision_id)
                || !self
                    .keys
                    .insert((asset.id.clone(), asset.revision_id.clone()))
            {
                return Err("invalid or duplicate embedded source identity and revision".into());
            }
            super::profile_capacity(asset.profile().bytes().len(), &mut self.profile_bytes)?;
            if !matches!(asset.depth(), 8 | 16 | 32)
                || asset.rgba16().is_some_and(|image| {
                    image.storage() != crate::precision_image::PrecisionStorage::Rgba16Unorm
                })
                || asset.rgba32().is_some_and(|image| {
                    image.storage() != crate::precision_image::PrecisionStorage::Rgba32Float
                })
            {
                return Err("embedded source requires genuine RGBA8 or RGBA16 storage".into());
            }
            // A camera original rides on a u16 master, or in a typed file on
            // a 32-bit float one (word 78, `float_native`); the byte format
            // has no word for the float pair and keeps refusing it.
            if asset.raw.is_some()
                && asset.depth() != 16
                && !(asset.depth() == 32 && self.owner_depth == 16)
            {
                return Err("retained RAW requires a genuine RGBA16 developed master".into());
            }
            self.raw.push(asset.raw.as_ref())?;
            self.binary.push(&asset)?;
            self.assets.push(asset);
        }
        binding.asset = index;
        self.bindings.push(binding);
        Ok(())
    }
    fn advanced(&self) -> bool {
        self.binary.present()
            || self.raw.present()
            || self
                .assets
                .iter()
                .any(|a| a.revision_id != a.id || a.depth() as u8 != self.owner_depth)
            || self.bindings.iter().any(|b| b.link.is_some())
    }
    pub fn version(&self) -> u32 {
        if self.binary.present() {
            return if self.owner_depth == 8 {
                super::BYTE_RESOURCE_VERSION
            } else {
                super::PRECISION_RESOURCE_VERSION
            };
        }
        if self.raw.present() {
            return if self.owner_depth == 8 {
                BYTE_RAW_VERSION
            } else {
                PRECISION_RAW_VERSION
            };
        }
        match (self.owner_depth, self.assets.is_empty(), self.advanced()) {
            (8, true, _) => 10,
            (16, true, _) => 13,
            (8, false, false) => BYTE_SOURCE_VERSION,
            (16, false, false) => PRECISION_SOURCE_VERSION,
            (8, false, true) => BYTE_LINK_VERSION,
            _ => PRECISION_LINK_VERSION,
        }
    }
    pub fn validate_fallbacks(
        &self,
        mut fallback: impl FnMut(usize) -> Option<((u32, u32), (i32, i32))>,
    ) -> Result<(), String> {
        for b in &self.bindings {
            let (dimensions, offset) =
                fallback(b.layer).ok_or("embedded source requires raster fallback")?;
            validate_binding(b, self.assets[b.asset].dimensions(), dimensions, offset)?;
        }
        Ok(())
    }
    /// RGBA8-equivalent pixels, preserving the existing byte capacity API while
    /// charging u16 masters for both bytes of each component.
    pub fn pixel_count(&self) -> u64 {
        self.assets
            .iter()
            .map(|a| {
                let (w, h) = a.dimensions();
                u64::from(w) * u64::from(h) * u64::from(a.depth() as u8) / 8
            })
            .sum()
    }
    /// The byte writer's check. Refuses past the byte format's decoded cap;
    /// else the decoded bytes of the fallbacks and every asset, the reader's
    /// `preflight_byte` total.
    pub fn preflight(&self, fallback_pixels: u64) -> Result<u64, String> {
        let mut bytes = fallback_pixels
            .checked_mul(4)
            .ok_or("native fallback byte overflow")?;
        if bytes > MAX_DRDC_DECODED_BYTES {
            return Err(
                "embedded sources and layer fallbacks exceed the native pixel limit".into(),
            );
        }
        for asset in &self.assets {
            source_plane_capacity(
                asset.dimensions(),
                asset.depth() as u8,
                &mut bytes,
                MAX_DRDC_DECODED_BYTES,
            )?;
        }
        Ok(bytes)
    }
    pub fn header_bytes(&self) -> u64 {
        if self.assets.is_empty() {
            return 0;
        }
        let advanced = self.advanced();
        12 + if self.binary.present() {
            4 + self.binary.header_bytes()
        } else {
            self.raw.header_bytes()
        } + self
            .bindings
            .iter()
            .map(|b| {
                60 + if advanced {
                    1 + b.link.as_ref().map_or(0, |l| 68 + l.path.len() as u64)
                } else {
                    0
                }
            })
            .sum::<u64>()
            + self
                .assets
                .iter()
                .map(|a| {
                    24 + a.id.len() as u64
                        + a.profile().bytes().len() as u64
                        + if advanced {
                            8 + a.revision_id.len() as u64
                        } else {
                            0
                        }
                })
                .sum::<u64>()
    }
    pub fn extra_nonpixel_retained(&self) -> u64 {
        (if self.binary.present() {
            self.binary.extra_retained_bound()
        } else {
            self.raw.extra_retained_bound()
        }) + self
            .assets
            .iter()
            .map(|a| {
                3 * a.profile().bytes().len() as u64
                    + if self.advanced() {
                        0
                    } else {
                        a.revision_id.len() as u64
                    }
            })
            .sum::<u64>()
    }
    pub fn encode(
        &self,
        out: &mut dyn NativeSink,
        bound: u64,
        job: JobContext<'_>,
    ) -> Result<(), String> {
        if self.assets.is_empty() {
            return Ok(());
        }
        let advanced = self.advanced();
        if self.binary.present() {
            append(
                out,
                &(if self.owner_depth == 8 {
                    BYTE_LINK_VERSION
                } else {
                    PRECISION_LINK_VERSION
                })
                .to_le_bytes(),
                bound,
                job,
            )?;
        }
        append(out, b"RSRC", bound, job)?;
        append(out, &(self.assets.len() as u32).to_le_bytes(), bound, job)?;
        for (index, asset) in self.assets.iter().enumerate() {
            job.check()?;
            put_asset_header(out, asset, advanced, bound, job)?;
            match &asset.pixels {
                SourcePixels::Rgba16(image) => put_native_png(out, image, bound, job)?,
                SourcePixels::Rgba32F(image) => {
                    crate::float_native::put_samples(out, image, bound, job)?
                }
                SourcePixels::Rgba8 { pixels, .. } => {
                    // Offered to the tile pool's skeleton sink as tiles
                    // first (13C), else the record streamed row by row.
                    let taken = match pixels.tile_plane() {
                        Some(plane) => out.raster(RasterRef::Rgba8SourcePng(plane))?,
                        None => false,
                    };
                    if !taken {
                        let mut rows =
                            crate::drdc::rgba_rows(|y| pixels.row_segments(y, 0, u32::MAX));
                        put_source_png8(out, pixels.dimensions(), &mut rows, job)?;
                    }
                    if out.position() > bound {
                        return Err("embedded source encoding exceeds admitted byte bound".into());
                    }
                }
            }
            if !self.binary.present() {
                self.raw.encode_record(index, out, bound, job)?;
            }
        }
        put_bindings(out, &self.bindings, advanced, bound, job)?;
        if self.binary.present() {
            self.binary.encode(out, bound, job)
        } else {
            self.raw.encode_payloads(out, bound, job)
        }
    }
}
impl PendingSources<'_> {
    /// The largest asset's `(banded, whole)` decode figures: a 16-bit
    /// asset decodes through the banded reader; an 8-bit one counts as a
    /// whole decode in both slots, an over-count the typed plan keeps since
    /// `decode_assets` decodes the writer's form by rows (13C-2b). A
    /// 32-bit float master decodes row by row straight into tiles
    /// (`float_native::decode_raster`) and adds nothing to either figure;
    /// the reopen charges it as a float raster. `cap` is the reading
    /// format's decoded cap ([`source_plane_capacity`]).
    pub fn inspect_pixels(&self, total: &mut u64, cap: u64) -> Result<(u64, u64), String> {
        let (mut banded, mut whole) = (0, 0);
        for asset in &self.assets {
            source_plane_capacity(asset.dimensions, asset.depth, total, cap)?;
            if asset.depth == 32 {
                if asset.png.len()
                    != pixel_count(asset.dimensions.0, asset.dimensions.1)? as u64 * 16
                {
                    return Err(
                        "float native raster byte length differs from its dimensions".into(),
                    );
                }
                continue;
            }
            if asset.depth == 16 {
                if inspect_native_payload(&asset.png)? != asset.dimensions {
                    return Err("embedded source dimensions differ from its PNG".into());
                }
            } else {
                let decoder = PngDecoder::with_limits(asset.png.reader(), image::Limits::default())
                    .map_err(|e| format!("failed to inspect embedded source PNG: {e}"))?;
                if decoder.dimensions() != asset.dimensions
                    || decoder.color_type() != image::ColorType::Rgba8
                {
                    return Err(
                        "embedded source must preserve its declared RGBA8 dimensions and depth"
                            .into(),
                    );
                }
                if image::Limits::default()
                    .max_alloc
                    .is_some_and(|cap| decoder.total_bytes() > cap)
                {
                    return Err("embedded source exceeds decoder allocation limit".into());
                }
            }
            // The established RGBA16 codec estimate safely covers the smaller
            // RGBA8 decoder as well; encoded bounds and retained bytes use the
            // actual original precision, not a promoted surrogate.
            let decode = native_png_decode_scratch(asset.dimensions)?;
            whole = whole.max(decode);
            banded = banded.max(if asset.depth == 16 {
                native_png_band_scratch(asset.dimensions)?
            } else {
                decode
            });
        }
        Ok((banded, whole))
    }
    pub fn decode_assets(
        &self,
        profiles: Vec<Arc<IccProfile>>,
        job: JobContext<'_>,
    ) -> Result<(Vec<Arc<SourceAsset>>, Vec<Binding>), String> {
        let raw = self.raw.materialize(job)?;
        let binary = self.binary.materialize(job)?;
        let assets = self
            .assets
            .iter()
            .zip(profiles)
            .enumerate()
            .map(|(index, (pending, profile))| {
                job.check()?;
                let pixels = if pending.depth == 32 {
                    SourcePixels::Rgba32F(Box::new(crate::float_native::decode_raster(
                        &pending.png,
                        pending.dimensions,
                        profile,
                        job,
                    )?))
                } else if pending.depth == 16 {
                    SourcePixels::Rgba16(decode_native_png(
                        &pending.png,
                        pending.dimensions,
                        profile,
                        job,
                    )?)
                } else if crate::png_rows8_read::writer_form(&pending.png, 4)
                    .map_err(|e| format!("failed to decode embedded source: {e}"))?
                {
                    // The writer's form decodes by rows into tiles (13C-2b,
                    // `png_rows8_read`); preflight bounded it by `pixel_count`.
                    let plane =
                        crate::png_rows8_read::decode_rgba8(&pending.png, pending.dimensions, job)
                            .map_err(|e| format!("failed to decode embedded source: {e}"))?;
                    SourcePixels::Rgba8 {
                        pixels: crate::layer_raster::LayerRaster::from_tile_plane(plane),
                        profile,
                    }
                } else {
                    // Any other stored PNG decodes whole: a slice as it always
                    // did, a file span through the same PNG decoder over its
                    // section (`native_stream`).
                    let decoded = match pending.png {
                        Payload::Bytes(bytes) => {
                            image::load_from_memory_with_format(bytes, image::ImageFormat::Png)
                        }
                        Payload::File { .. } => {
                            image::load(pending.png.reader(), image::ImageFormat::Png)
                        }
                    };
                    let pixels = decoded
                        .map_err(|e| format!("failed to decode embedded source: {e}"))?
                        .into_rgba8();
                    job.check()?;
                    if pixels.dimensions() != pending.dimensions {
                        return Err("embedded source dimensions changed after preflight".into());
                    }
                    SourcePixels::Rgba8 {
                        pixels: crate::layer_raster::LayerRaster::unseeded(pixels),
                        profile,
                    }
                };
                Ok(Arc::new(SourceAsset {
                    id: pending.id.into(),
                    revision_id: pending.revision_id.into(),
                    pixels,
                    raw: binary
                        .get(index)
                        .and_then(|r| r.0.clone())
                        .or_else(|| raw.get(index).cloned().flatten()),
                    attachment: binary.get(index).and_then(|r| r.1.clone()),
                    profile_refusal: pending.profile_refusal.map(Into::into),
                }))
            })
            .collect::<Result<Vec<_>, String>>()?;
        Ok((assets, self.materialize_bindings()))
    }
}
