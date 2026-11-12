//! The 8-bit native writer's capacity plan: an upper bound of the
//! bytes `DrDocument::encode_native` writes (`encoded_bound`, which the writer
//! reserves for its body buffer) and the peak scratch the save needs beside the
//! document (`write_scratch_bytes`, which the host admits for the 8-bit `.rz`
//! save and snapshot). Metadata only: it reads stored dimensions and lengths,
//! never `referenced_bytes` or the contiguous-view cache, so the figure is the
//! same with views live or evicted. It imposes no cap of its own and does not
//! run `native_save_preflight` (the writer still does, first).
//!
//! # Formula
//! `encoded_bound` W mirrors the writer's order: 256 fixed bytes (magic, base
//! version, size, count, light, channel count, resolution, origin, guide
//! count, blob flags), 4 per conditional version word (13 words, a deliberate
//! over-count), per layer `64 + name + byte_png_bound(dims, 4)` plus the mask
//! (`64 + raw`), meta and style slots (`5 + len`), per channel `32 + name +
//! byte_png_bound(canvas, 1)`, the ICC and metadata blobs, `9·guides`, the
//! source table (`header_bytes + 16 + Σ source_png_encoded_bound`), every
//! tail table through its own module's `header_bytes` / `encoded_len`, and
//! the path table through `path_native::encoded_bound` plus its own word,
//! the variations table through `generated_native::encoded_len` plus its
//! own word, and word 79 through `raw_locals_native::header_bytes`.
//!
//! The held tables beside it: the WARP payload V, the GVAR table, the sampler, comps,
//! refusal and projective tables (the last two counted twice: their encoded
//! Vec is built and then appended), `32·layers` for the id/label/outside
//! lists, the largest style JSON, 128 KiB for the descriptor transients, and
//! the sources' `extra_nonpixel_retained` (descriptor expansion and three
//! profile copies, as the typed plan charges it), and the path table's
//! `path_native::transient_bound` (its JSON text, its Vec and the JSON tree;
//! 0 without a path).
//!
//! # Per-plane peak model
//! An 8-bit plane with `c` channels (4: a layer or an 8-bit asset; 1: a
//! channel), `px = w·h`, and G its W term:
//! `f = c·px + h` (the filtered stream), `Cm = ⌈3f/2⌉ + 4096` (fdeflate's
//! UltraFast output, literal codes ≤ 12 bits), `Sm = f + 5·⌈f/65535⌉ + 11`
//! (the stored-only fallback), `K = 4·px` for `c = 4` (the `to_contiguous`
//! copy, 0 for a channel), `R = 2·(c·w + 1)` (png's two rows), and
//! `X = max(K + R + 2·Cm, K + R + Cm + 2·Sm, K + Sm + G, 2·G) + 1 MiB`: the
//! compressed Vec mid doubling-realloc, the stored fallback mid
//! doubling-realloc beside it, the IDAT copy into the reserved PNG Vec, and
//! the PNG appended into the body. `T_k = X − G`, because W already reserves
//! G unwritten while plane k encodes; a 16-bit asset's `T_k` is
//! `native_png_encode_scratch`. `T = max T_k`,
//! `write_scratch_bytes = W + held + T + BOOKKEEPING_BYTES +
//! SAVE_OVERHEAD_BYTES`. It is a structural bound, not a scaled guess: `buf`
//! and every PNG Vec are reserved, so only png's two internal Vecs grow, and
//! phases A/B charge each at twice its bound. There is no added margin: the
//! quarter the first design carried had no measurement behind it, covered
//! gate M's cases at ≥ 1.8× instead of the ≥ 1.25× the bound already gives,
//! and refused the 45 MP Background-plus-photo document in the app by
//! 34,472,958 B (the host also admits its 181.8 MB display
//! projection beside the plan).
//!
//! # Gate M
//! E = `ri_interval_max_phys_footprint` − the footprint read after
//! `proc_reset_footprint_interval`, around `dr_doc_save_native`; one case per
//! process, `MallocLargeCache=0`, three runs plus a baseline twin (every
//! twin 0), views live and after `dr_mixed_resources_evict_caches`; Apple M2
//! Max, release. S1: File ▸ New's white 8256×5504 Background plus a placed
//! RGB-noise layer (alpha 255); S2: one noise raster; S2p: S1 without the
//! Background; S3: two 7500×7500 noise layers; S4: S1 plus a
//! 64,000,000-byte incompressible resource on the placed layer; S5: S1 with
//! four-channel noise. Max E over both view states, the plan (`est`) and
//! `est` ÷ max E; `need` = est + the document's own bytes:
//!
//! | case | max E | file | encoded_bound | est | ÷ max E | need |
//! |---|---|---|---|---|---|---|
//! | S1 | 744,637,016 | 364,461,702 | 682,675,302 | 1,309,312,114 | 1.758 | 1,672,863,772 |
//! | S2 | 561,971,704 | 181,783,673 | 227,209,622 | 853,838,698 | 1.519 | 1,035,618,445 |
//! | S2p | 743,752,304 | 363,569,876 | 455,466,004 | 1,082,102,784 | 1.455 | 1,263,883,552 |
//! | S3 | 920,666,856 | 450,049,657 | 562,508,671 | 1,329,662,005 | 1.444 | 1,779,687,713 |
//! | S4 | 728,597,080 | 428,461,979 | 746,675,579 | 1,373,372,316 | 1.885 | 1,800,924,600 |
//! | S5 | 795,493,000 | 364,461,702 | 682,675,302 | 1,309,312,114 | 1.646 | 1,672,863,772 |
//!
//! `est` and `referenced − views` are identical live and evicted in every
//! case. The peaks were measured with the reserving writer, which the
//! margin's removal does not change. `est` includes the sources'
//! `extra_nonpixel_retained`, added after the measurement (+7,704 B for S1,
//! S2p and S5, +67,629 B for S4; the ratios are unchanged to three places).
//!
//! This module is the byte writer's capacity plan, which the writer reserves and the host admits
//! (`dr_doc_native_save_scratch_bytes`; this module doc holds the formula and gate M).
use crate::doc::DrDocument;
use crate::drdc::MAX_DRDC_NAME_LEN;
use crate::native_sources::SourceTable;
use crate::precision_image::BOOKKEEPING_BYTES;

const OVERFLOW: &str = "native save plan overflow";

/// The unattributed footprint of a byte save beside its buffers. Gate S
/// measured 16.5–20.6 MB.
pub(crate) const SAVE_OVERHEAD_BYTES: u64 = 32 * 1024 * 1024;

/// The fixed fields of the body (see the module doc).
const FIXED_BYTES: u64 = 256;
/// Four bytes for each of the 13 conditional version words, all counted.
const VERSION_WORD_BYTES: u64 = 13 * 4;
/// The two 64 KiB descriptor JSON transients.
const DESCRIPTOR_TRANSIENT_BYTES: u64 = 128 * 1024;
/// Deflate state, adler and chunk framing per plane.
const PLANE_FIXED_BYTES: u64 = 1024 * 1024;

/// The byte writer's capacity plan: what `encode_native` reserves and what
/// the host admits.
pub struct NativeSavePlan {
    pub encoded_bound: u64,
    pub write_scratch_bytes: u64,
}

fn add(a: u64, b: u64) -> Result<u64, String> {
    a.checked_add(b).ok_or_else(|| OVERFLOW.to_string())
}

fn mul(a: u64, b: u64) -> Result<u64, String> {
    a.checked_mul(b).ok_or_else(|| OVERFLOW.to_string())
}

/// A layer (channels 4) or channel (channels 1) PNG: px·(channels + 1) + 4096.
/// Bounds the written PNG (≤ stored-only size + framing); the spare byte per
/// pixel is file-length slack only.
pub(crate) fn byte_png_bound(dimensions: (u32, u32), channels: u32) -> Result<u64, String> {
    let px = u64::from(dimensions.0) * u64::from(dimensions.1);
    add(mul(px, u64::from(channels) + 1)?, 4096)
}

/// `T_k` of one 8-bit plane (the module doc's per-plane model), given its W
/// term `g`.
fn byte_plane_transient(dimensions: (u32, u32), channels: u32, g: u64) -> Result<u64, String> {
    let (w, h) = (u64::from(dimensions.0), u64::from(dimensions.1));
    let c = u64::from(channels);
    let px = w * h;
    let f = add(mul(c, px)?, h)?;
    let cm = add(mul(f, 3)?.div_ceil(2), 4096)?;
    let sm = add(add(f, mul(f.div_ceil(65535), 5)?)?, 11)?;
    let k = if channels == 4 { mul(px, 4)? } else { 0 };
    let r = mul(add(mul(c, w)?, 1)?, 2)?;
    let kr = add(k, r)?;
    let a = add(kr, mul(cm, 2)?)?;
    let b = add(add(kr, cm)?, mul(sm, 2)?)?;
    let copy = add(add(k, sm)?, g)?;
    let append = mul(g, 2)?;
    let x = add(a.max(b).max(copy).max(append), PLANE_FIXED_BYTES)?;
    x.checked_sub(g).ok_or_else(|| OVERFLOW.to_string())
}

/// An empty Vec with exactly `bytes` of capacity reserved, for the writer's
/// body and PNG buffers, or Err when the allocation fails.
pub(crate) fn reserved(bytes: u64) -> Result<Vec<u8>, String> {
    let mut buf = Vec::new();
    usize::try_from(bytes)
        .ok()
        .and_then(|bytes| buf.try_reserve_exact(bytes).ok())
        .ok_or_else(|| "could not allocate the native document buffer".to_string())?;
    Ok(buf)
}

fn name_len(name: &str) -> u64 {
    (name.len() as u64).min(u64::from(MAX_DRDC_NAME_LEN))
}

impl DrDocument {
    /// The 8-bit native writer's plan: the encoded-body bound it reserves and
    /// the peak scratch beside the document (the module doc's formula).
    /// Depends only on stored dimensions and lengths, never on cached views.
    pub fn native_save_plan(&self) -> Result<NativeSavePlan, String> {
        Ok(self.native_plans()?.save)
    }

    /// [`Self::native_save_plan`] over the writer's `sources` table. Called
    /// by `native_plans` only, which computes both plans once.
    pub(crate) fn save_plan_with(&self, sources: &SourceTable) -> Result<NativeSavePlan, String> {
        let canvas = (self.width, self.height);
        let mut encoded = FIXED_BYTES + VERSION_WORD_BYTES;
        let mut transient = 0u64;
        let mut largest_style = 0u64;
        for layer in &self.layers {
            let dims = layer.pixels.dimensions();
            let g = byte_png_bound(dims, 4)?;
            encoded = add(encoded, add(64 + name_len(&layer.name), g)?)?;
            if let Some(mask) = &layer.mask {
                encoded = add(encoded, add(64, mask.as_raw().len() as u64)?)?;
            }
            if let Some(meta) = &layer.meta {
                encoded = add(encoded, add(5, meta.len() as u64)?)?;
            }
            if let Some(style) = &layer.style {
                let len = style.to_json().len() as u64;
                largest_style = largest_style.max(len);
                encoded = add(encoded, add(5, len)?)?;
            }
            transient = transient.max(byte_plane_transient(dims, 4, g)?);
        }
        if !self.channels.is_empty() {
            let g = byte_png_bound(canvas, 1)?;
            for channel in &self.channels {
                encoded = add(encoded, add(32 + name_len(&channel.name), g)?)?;
            }
            transient = transient.max(byte_plane_transient(canvas, 1, g)?);
        }
        if !self.profile.is_builtin_srgb() {
            encoded = add(encoded, self.profile.bytes().len() as u64)?;
        }
        let meta = &self.metadata;
        for blob in [&meta.exif, &meta.xmp, &meta.iptc].into_iter().flatten() {
            encoded = add(encoded, add(5, blob.len() as u64)?)?;
        }
        encoded = add(encoded, mul(9, self.guides.len() as u64)?)?;

        if !sources.assets.is_empty() {
            encoded = add(encoded, add(sources.header_bytes(), 16)?)?;
            for asset in &sources.assets {
                let (dims, depth) = (asset.dimensions(), asset.depth() as u8);
                let g = crate::native_sources::source_png_encoded_bound(dims, depth)?;
                encoded = add(encoded, g)?;
                let t = if depth == 32 {
                    // A float master's raw payload has no codec: `g` is
                    // its exact `w · h · 16` bytes.
                    0
                } else if depth == 16 {
                    crate::precision_codec::native_png_encode_scratch(dims)?
                } else {
                    byte_plane_transient(dims, 4, g)?
                };
                transient = transient.max(t);
            }
        }
        let comps = crate::comps_native::payload(&self.comps, |id| {
            Some(self.layers.get(self.layer_index_by_id(id)?)?.name.clone())
        })?;
        let projective = crate::source_transform_native::encoded_len(
            crate::source_transform_native::entries(&sources.bindings).len(),
        );
        let refusals = crate::source_refusal_native::encoded_len(
            &crate::source_refusal_native::entries(&sources.assets),
        );
        let samplers = crate::sampler_native::encoded_len(&self.color_samplers);
        // Word 74 and its PATH table (`path_native`), by the table's bound,
        // the raster-part planes of live vector masks included.
        let vector_masks = self.vector_mask_table();
        let paths = match crate::path_native::encoded_bound(&self.paths.entries, &vector_masks, 1) {
            0 => 0,
            table => table + 4,
        };
        // Word 92 and its GVAR table (`generated_native`).
        let variations = match crate::generated_native::encoded_len(&self.generated_entries()) {
            0 => 0,
            table => table + 4,
        };
        let warp = crate::warp_native::byte_capacity(self)?.0;
        let linear_light = self.evaluation() == crate::doc_evaluation::Evaluation::LinearLight;
        for term in [
            crate::identity_native::header_bytes(self.layers.len()),
            crate::mask_outside_native::header_bytes(
                crate::mask_outside_native::byte_indices(self).len(),
            ),
            crate::evaluation_native::header_bytes(linear_light),
            crate::label_native::header_bytes(crate::label_native::byte_entries(self).len()),
            crate::comps_native::header_bytes(comps.len() as u64),
            projective,
            warp,
            refusals,
            samplers,
            paths,
            variations,
            crate::levels_native::header_bytes(crate::levels_native::byte_carries(&self.layers)),
            crate::curves_native::header_bytes(crate::curves_native::byte_carries(&self.layers)),
            crate::look_native::header_bytes(crate::look_native::byte_carries(&self.layers)),
            // Word 72 (`float_native`), no table.
            if crate::float_native::masters_present(&sources.assets) {
                4
            } else {
                0
            },
            // Word 79 (`raw_locals_native`), no table.
            crate::raw_locals_native::header_bytes(crate::raw_locals_native::carries(
                sources.assets.iter().map(|a| &**a),
            )),
            // Word 91 and its LREF table (`reference_native`).
            crate::reference_native::header_bytes(
                crate::reference_native::byte_entries(self).len(),
            ),
            // Word 112 and its SPOT table (`spot_channel_native`, #1).
            crate::spot_channel_native::header_bytes(
                crate::spot_channel_native::byte_entries(&self.channels).len(),
            ),
            // Word 113 and its LBKG table (`background_native`).
            crate::background_native::header_bytes(crate::background_native::byte_entry(self)),
            // Word 114 (`clipping_path_native`), no table.
            crate::clipping_path_native::header_bytes(&self.paths.entries),
            // Word 90 (`style_pattern`), no table.
            if crate::style_pattern::styles_pattern(self) {
                4
            } else {
                0
            },
            // Word 93 (`pattern_angle_native`), no table.
            if crate::pattern_angle_native::styles_angle(self) {
                4
            } else {
                0
            },
            // Word 94 (`gradient_options_native`), no table.
            if crate::gradient_options_native::styles_options(self) {
                4
            } else {
                0
            },
            // Word 95 (`stroke_pattern_native`), no table.
            if crate::stroke_pattern_native::styles_stroke_pattern(self) {
                4
            } else {
                0
            },
            // Word 96 (`contour_native`), no table.
            if crate::contour_native::styles_contour(self) {
                4
            } else {
                0
            },
            // Word 97 (`noise_native`), no table.
            if crate::noise_native::styles_noise(self) {
                4
            } else {
                0
            },
            // Word 111 (`gradient_map_method_native`), no table.
            if crate::gradient_map_method_native::byte_carries(&self.layers) {
                4
            } else {
                0
            },
            // Word 105 (`fill_align_native`), no table.
            if crate::fill_align_native::descriptions_align(self) {
                4
            } else {
                0
            },
            // Word 104 (`gradient_midpoint_native`), no table.
            if crate::gradient_midpoint_native::styles_midpoints(self) {
                4
            } else {
                0
            },
            // Word 103 (`gradient_smoothness_native`), no table.
            if crate::gradient_smoothness_native::styles_smoothness(self) {
                4
            } else {
                0
            },
            // Word 102 (`large_description_native`), no table.
            if crate::large_description_native::descriptions_large(self) {
                4
            } else {
                0
            },
            // Word 101 (`blend_if_channels_native`), no table.
            if crate::blend_if_channels_native::styles_multichannel(self) {
                4
            } else {
                0
            },
            // Word 100 (`wide_color_native`), no table.
            if crate::wide_color_native::styles_wide(self) {
                4
            } else {
                0
            },
            // Word 99 (`glow_gradient_native`), no table.
            if crate::glow_gradient_native::styles_glow_gradient(self) {
                4
            } else {
                0
            },
            // Word 98 (`texture_native`), no table.
            if crate::texture_native::styles_texture(self) {
                4
            } else {
                0
            },
            // Word 89 (`style_instances`), no table.
            if crate::style_instances::styles_stacked(self) {
                4
            } else {
                0
            },
        ] {
            encoded = add(encoded, term)?;
        }

        let mut held = add(warp, samplers)?;
        // The GVAR table's Vec, built before the body is assembled.
        held = add(held, variations)?;
        held = add(held, comps.len() as u64)?;
        held = add(held, mul(add(refusals, projective)?, 2)?)?;
        held = add(held, mul(32, self.layers.len() as u64)?)?;
        held = add(held, largest_style)?;
        held = add(held, DESCRIPTOR_TRANSIENT_BYTES)?;
        held = add(held, sources.extra_nonpixel_retained())?;
        held = add(
            held,
            crate::path_native::transient_bound(&self.paths.entries, &vector_masks, 1),
        )?;

        let mut base = add(encoded, held)?;
        for term in [transient, BOOKKEEPING_BYTES, SAVE_OVERHEAD_BYTES] {
            base = add(base, term)?;
        }
        Ok(NativeSavePlan {
            encoded_bound: encoded,
            write_scratch_bytes: base,
        })
    }

    /// The plan's `write_scratch_bytes`: what the host admits for the 8-bit
    /// native save and snapshot (`dr_doc_native_save_scratch_bytes`).
    pub fn native_save_scratch_bytes(&self) -> Result<u64, String> {
        Ok(self.native_save_plan()?.write_scratch_bytes)
    }
}
