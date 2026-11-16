//! The 8-bit native reader's capacity plan: what `parse_native` still
//! holds when it returns (`retained_bytes`) and the open's whole peak
//! (`decode_bytes`), which the host's editing-state archive charges for a
//! recovery reopen and a snapshot load. Metadata and pointer identity only:
//! it walks the document's authoritative allocations with caches excluded, so
//! the figure is the same with contiguous views live or evicted.
//!
//! # Formula
//! N = `Inventory::document(self)`'s `retained_referenced_bytes` (Σ bytes ×
//! references, caches excluded). Counting per reference is what the reader
//! re-creates: every layer plane, mask and channel is written and decoded on
//! its own, so a duplicated layer, a shared mask or a `filled` plane's
//! repeated tile counts at full size each time (`png_rows8_read` tiles a
//! decoded layer plane without sharing; a mask or channel shares its uniform
//! tiles, which N over-counts).
//!
//! From N, for each layer holding a retained source `s`, the references the
//! reader provably never creates are removed: `s.rendered` (never written; the
//! reader rebuilds it as `layer.pixels.clone()`, `materialize_byte`) and
//! `s.asset` (written once per distinct allocation, not per binding). The
//! same holds for each layer's Generative Layer binding `g`: N already
//! counts its GVAR table, every stored picture (PNG) and tag, which the reader
//! decodes and keeps, and `g.rendered` is removed, because the reader rebuilds
//! it as `layer.pixels.clone()` (`doc_gen_variations::attach_byte`). Then
//! each effective asset of `byte_sources` (the writer's pointer-deduplicated
//! table, `SourceTable::push`) is added once. That is `serialized_bytes`:
//! every record the writer writes, decoded once each.
//!
//! `retained_bytes` = `serialized_bytes` − the layer pixels of every binding
//! whose layer plane shares all of its 8-bit asset's samples
//! (`LayerRaster::shares_samples`): `materialize_byte` adopts the asset's
//! tiles for such a layer (`same_bytes` → `reidentified`), so the decoded
//! fallback is freed and only the asset stays.
//!
//! `decode_bytes` = H + `serialized_bytes` + T + `DECODE_OVERHEAD_BYTES`
//! (plus the float master margin), where H =
//! `encoded_bound − streamed_bound + NATIVE_STREAM_WINDOW_BYTES` is what the
//! streamed reader (13C-2b) holds beside the planes it decodes: every small
//! take it copies into its arena
//! (names, slots, the tail tables, WARP originals, GVAR pictures and RAW
//! payloads, all outside `drdc_save_plan`'s `streamed_bound`) and its fixed
//! window (the numeric read-ahead and one payload's section buffer). The
//! file itself is never held. T is the largest buffer the reader holds
//! beside its end state, never netted against it:
//! - the layer stage (`drdc_read.rs`'s decode loop): `band_scratch8(w, 4)`
//!   of the widest layer plane (`png_rows8_read`: sixteen PNG rows plus the
//!   fixed codec allowance, also png's limit for the decode), whose tiles N
//!   counts as they are built band by band;
//! - the channel stage: `band_scratch8(w, 1)` of the canvas when there are
//!   channels;
//! - the 8-bit asset stage (`native_sources_assets.rs`'s `decode_assets`):
//!   `band_scratch8(w, 4)` of the widest 8-bit asset;
//! - a 16-bit asset: `native_png_band_scratch` (the banded reader); a 32-bit
//!   float master decodes row by row too and is the float margin's.
//!
//! A raw mask is read a row at a time into its tiles, within the layer
//! stage's figure. S1's open (a placed 8-bit layer) holds the placed layer's
//! decoded plane and its source's together until `materialize_byte` adopts
//! the source's tiles for the layer; both are inside `serialized_bytes`,
//! which counts the layer's plane before `retained_bytes` drops it.
//!
//! The plan is of a document this build writes: every plane in the writer's
//! form and every mask within `capacity::mask_dimensions`, so the reader's
//! whole-plane arms (another writer's PNG, a version 2–9 mask over
//! `MAX_PIXELS`) are never in a plan.
//!
//! [`decode_figure`] and [`decode_transient`] are the one home of the sum
//! and of T.
//!
//! Over-counts left in place: a WARP original the reader re-shares is counted
//! per reference, a RAW payload shared across revisions per asset, and the
//! host's caches (style and mask planes) are outside N. `DECODE_OVERHEAD_BYTES`
//! was measured beside the whole-file reader and is kept until 13C-6 measures
//! this one.
//!
//! # Gate R′ (the former whole-file reader)
//! The record of the reader before 13C-2b, which held the whole file and
//! decoded each plane whole: the plan then charged `encoded_bound` as the
//! file and `4·w·h` (a channel `2·w·h`) as T. It stays as that reader's
//! record, as `drdc_save_plan` keeps gate M, until 13C-6 measures this one.
//!
//! E = `ri_interval_max_phys_footprint` − the footprint read after
//! `proc_reset_footprint_interval` (a Swift probe built from the app sources),
//! one case per process, `MallocLargeCache=0`, three runs plus a baseline
//! twin (every twin 0), Apple M2 Max, release. p45: a white 8256×5504
//! Background plus a placed plasma JPEG; n45: the same with an 8256×5504
//! RGBA noise PNG; s45: that noise PNG as one raster; a45: the noise PNG
//! placed on a 3000×2000 Background and scaled to 2999×2000 (asset 8256×5504).
//! Decode = `RecoveryStore.read` alone; reopen = it plus
//! `PreparedDocumentEdit`, against `RecoveryCapacity.validate`'s peak (the
//! host charges the 8-bit projection at P + ⌈P/4⌉). s45 and a45 are
//! structural bounds whose only slack is `encoded_bound` − file plus the
//! 64 MiB, gated at ≥ 1.15×; every other case at ≥ 1.25×.
//!
//! | case | file | serialized | retained | T | plan | max E | ÷ |
//! |---|---|---|---|---|---|---|---|
//! | a45 decode | 205,903,696 | 229,792,628 | 229,792,628 | 181,764,096 | 766,920,677 | 618,201,520 | 1.241 |
//! | s45 decode | 181,783,673 | 181,779,747 | 181,779,747 | 181,764,096 | 657,862,329 | 546,406,808 | 1.204 |
//! | n45 decode | 364,461,729 | 545,338,900 | 363,568,996 | 181,764,096 | 1,476,887,189 | 1,093,878,528 | 1.350 |
//! | p45 decode | 23,970,496 | 545,338,901 | 363,568,997 | 181,764,096 | 1,476,887,191 | 753,205,776 | 1.961 |
//! | p45 reopen, 0 snapshots | | | | | 1,726,799,717 | 1,275,413,512 | 1.354 |
//! | p45 reopen, 1 snapshot | | | | | 2,090,368,714 | 1,458,767,144 | 1.433 |
//! | n45 reopen, 0 snapshots | | | | | 1,726,799,716 | 1,275,397,128 | 1.354 |
//! | n45 reopen, 1 snapshot | | | | | 2,090,368,712 | 1,458,799,888 | 1.433 |
//!
//! p45's retained figure is the measured reopen retention less the
//! 181,764,096-byte display projection plus 9,082 B. The reader's
//! `into_rgba8` change is bit-identical: the reopened layers, masks, sources
//! and channels hash the same before and after it
//! (`tests/native_reopen_plan8.rs`'s pins, and the probe's p45, n45, s45 and
//! a45 pins).
//!
//! This module is the byte reader's reopen plan, which counts each allocation the reader shares
//! once and which the host's editing-state archive charges (`dr_doc_native_reopen_plan`; this
//! module doc holds the formula and gate R′).
use crate::doc::DrDocument;
use crate::drdc_save_plan::NativeSavePlan;
use crate::layer_raster::LayerRaster;
use crate::native_sources::SourceTable;
use crate::native_stream::NATIVE_STREAM_WINDOW_BYTES;
use crate::png_rows8_read::band_scratch8;
use crate::resources::Inventory;

const OVERFLOW: &str = "native reopen plan overflow";

/// The reader's unattributed footprint beside its buffers (decoder state,
/// tables, allocator slack). The host's snapshot load requires at least this
/// much decode scratch for an 8-bit archive.
pub const DECODE_OVERHEAD_BYTES: u64 = 64 * 1024 * 1024;

/// The 8-bit reader's capacity plan (the module doc's formula).
pub struct NativeReopenPlan {
    /// Every record the 8-bit writer writes, decoded once each.
    pub serialized_bytes: u64,
    /// What `parse_native` still holds when it returns.
    pub retained_bytes: u64,
    /// The open's whole peak, including `retained_bytes`.
    pub decode_bytes: u64,
}

fn add(a: u64, b: u64) -> Result<u64, String> {
    a.checked_add(b).ok_or_else(|| OVERFLOW.to_string())
}

fn sub(a: u64, b: u64) -> Result<u64, String> {
    a.checked_sub(b).ok_or_else(|| OVERFLOW.to_string())
}

/// Σ bytes × references of the allocations `record` reports.
fn referenced(record: impl FnOnce(&mut Inventory)) -> u64 {
    let mut inventory = Inventory::default();
    record(&mut inventory);
    inventory.retained_referenced_bytes()
}

fn raster(pixels: &LayerRaster) -> u64 {
    referenced(|inventory| pixels.record_resources(inventory, "layer_pixels", false))
}

/// The byte writer's and reader's plans of one document, computed together
/// over one `byte_sources` table.
pub(crate) struct NativePlans {
    pub save: NativeSavePlan,
    pub reopen: NativeReopenPlan,
}

/// The reader's whole decode peak: what it `held` throughout (the module
/// doc's H), the `serialized` records decoded, the largest `transient`
/// buffer beside them and `DECODE_OVERHEAD_BYTES`. The one home of that sum.
pub(crate) fn decode_figure(held: u64, serialized: u64, transient: u64) -> Result<u64, String> {
    add(
        add(add(held, serialized)?, transient)?,
        DECODE_OVERHEAD_BYTES,
    )
}

/// The float reopen's margin over `pixels` of 32-bit float master:
/// `FLOAT_REOPEN_MARGIN_PER_PIXEL`, the term measured for
/// `float_native::decode_raster`'s shape, which a byte file's master
/// decodes through too. 0 for a file without one.
pub(crate) fn float_master_margin(pixels: u64) -> u64 {
    pixels.saturating_mul(crate::precision_native_plan::FLOAT_REOPEN_MARGIN_PER_PIXEL)
}

/// T, the largest buffer the reader holds beside its end state (the module
/// doc's stages): the RGBA8 band figure of each layer plane, the gray one of
/// the canvas when there are `channels`, and per asset `(dimensions, depth)`
/// the 16-bit banded scratch or an 8-bit one's RGBA8 band figure (a float
/// master's decode is the float margin's). The one home of that rule.
pub(crate) fn decode_transient(
    layers: &[(u32, u32)],
    channels: Option<(u32, u32)>,
    assets: &[((u32, u32), u8)],
) -> Result<u64, String> {
    let mut transient = 0u64;
    for &(width, _) in layers {
        transient = transient.max(band_scratch8(width, 4));
    }
    if let Some((width, _)) = channels {
        transient = transient.max(band_scratch8(width, 1));
    }
    for &(dimensions, depth) in assets {
        let stage = match depth {
            16 => crate::precision_codec::native_png_band_scratch(dimensions)?,
            8 => band_scratch8(dimensions.0, 4),
            _ => 0,
        };
        transient = transient.max(stage);
    }
    Ok(transient)
}

impl DrDocument {
    /// The 8-bit native reader's plan: the bytes it retains and its decode
    /// peak (the module doc's formula). Depends only on metadata and pointer
    /// identity, never on cached views.
    pub fn native_reopen_plan(&self) -> Result<NativeReopenPlan, String> {
        Ok(self.native_plans()?.reopen)
    }

    /// The save plan and the reopen plan, over one `byte_sources` table.
    pub(crate) fn native_plans(&self) -> Result<NativePlans, String> {
        let sources = crate::native_sources::byte_sources(self)?;
        let save = self.save_plan_with(&sources)?;
        let reopen = self.reopen_plan_with(&sources, &save)?;
        Ok(NativePlans { save, reopen })
    }

    /// [`Self::native_reopen_plan`] over `sources`, with the reader's held
    /// bytes from the save plan's bounds (the module doc's H).
    fn reopen_plan_with(
        &self,
        sources: &SourceTable,
        save: &NativeSavePlan,
    ) -> Result<NativeReopenPlan, String> {
        let mut document = Inventory::default();
        document.document(self);
        let mut serialized = document.retained_referenced_bytes();
        for layer in &self.layers {
            if let Some(source) = &layer.retained_source {
                serialized = sub(serialized, raster(&source.rendered))?;
                serialized = sub(
                    serialized,
                    referenced(|inventory| source.asset.record_resources(inventory)),
                )?;
            }
            if let Some(generated) = &layer.generated {
                serialized = sub(serialized, raster(&generated.rendered))?;
            }
        }
        // Each written asset once.
        for asset in &sources.assets {
            serialized = add(
                serialized,
                referenced(|inventory| asset.record_resources(inventory)),
            )?;
        }

        let mut retained = serialized;
        for binding in &sources.bindings {
            let pixels = &self.layers[binding.layer].pixels;
            if let Some(asset) = sources.assets[binding.asset].rgba8() {
                if pixels.shares_samples(asset) {
                    retained = sub(retained, raster(pixels))?;
                }
            }
        }

        let layers: Vec<(u32, u32)> = self.layers.iter().map(|l| l.pixels.dimensions()).collect();
        let assets: Vec<((u32, u32), u8)> = sources
            .assets
            .iter()
            .map(|a| (a.dimensions(), a.depth() as u8))
            .collect();
        let transient = decode_transient(
            &layers,
            (!self.channels.is_empty()).then_some((self.width, self.height)),
            &assets,
        )?;
        let float_pixels = sources
            .assets
            .iter()
            .filter(|a| a.depth() == 32)
            .map(|a| u64::from(a.dimensions().0) * u64::from(a.dimensions().1))
            .fold(0u64, u64::saturating_add);
        let held = add(
            sub(save.encoded_bound, save.streamed_bound)?,
            NATIVE_STREAM_WINDOW_BYTES,
        )?;
        Ok(NativeReopenPlan {
            serialized_bytes: serialized,
            retained_bytes: retained,
            decode_bytes: add(
                decode_figure(held, serialized, transient)?,
                float_master_margin(float_pixels),
            )?,
        })
    }
}
