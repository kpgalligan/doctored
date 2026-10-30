//! The conditional 32-bit FLOAT native wrapper (Phase 11F), in the shape
//! `evaluation_native.rs` established.
//!
//! DRDC 40 is the OUTERMOST version word — it sits before every prior word,
//! 39 included — and its table closes the file after every prior tail:
//! `FLOT`, u32 schema = 1, u32 operator (0 clip / 1 reinhard), f32 white.
//!
//! Two framing facts follow from the mode itself. A float document is
//! `linear_light` BY CONSTRUCTION, so the word after 40 is ALWAYS 39 and the
//! `EVAL` table is always present; anything else is a malformed file and is
//! refused before a pixel is read. And the body header's storage tag is 2
//! rather than 1, admitted ONLY under 40, so every reader before this one —
//! which admits tag 1 alone and does not know the word 40 at all — refuses a
//! float document instead of misreading its samples. That refusal IS the
//! disclosure.
//!
//! A float raster record's payload is RAW little-endian f32 RGBA rather than
//! a PNG: PNG has no float sample format, and the whole point of the mode is
//! that a stored sample survives exactly. The length word is therefore
//! redundant with the dimensions and is checked against them. The
//! payload decodes row by row straight into tiles at every size
//! (`PrecisionImage::from_rgba32_tiles_decoded`), so no contiguous copy is
//! held, and it is a span of the streamed reader
//! (`native_stream`), read from the file in blocks of at most 65,536 pixels
//! ([`decode_raster`]), so the file is not held either.
//!
//! `float_native` is the outermost conditional `.rz` wrapper (typed 40 before 39, body storage tag
//! 2 admitted only under 40, raw f32 raster payloads instead of PNGs, one `FLOT` table closing the
//! file), following `evaluation_native.rs`'s shape.
//!
//! # Float masters (word 67)
//!
//! A placed image's 32-bit float master (`SourcePixels::Rgba32F`) is an
//! RSRC asset whose advanced header's depth byte is 32 and whose u64
//! length/payload slot holds `w · h · 16` bytes of raw little-endian f32
//! RGBA in row order (straight alpha, the samples bit-exact) — the float
//! raster's payload ([`put_samples`] / [`decode_raster`]) without its
//! dimensions, which the asset header already carries. Version 67 is then
//! the FIRST word inside 46, ahead of 66, with no table: the writer emits it
//! iff some written asset has depth 32 ([`masters_present`]), so a document
//! without one writes exactly the prior writer's bytes. The reader admits
//! depth 32 only under 67 and refuses 67 with no depth-32 asset
//! ([`MASTER_WORD_REFUSAL`]), before any payload decodes. A build before 67
//! refuses such a file with its generic high-depth sentence. A 16-bit
//! document writes 67 as a float one does; an earlier build that admits 67
//! only in a float document refuses that file ("a 32-bit float master
//! requires a 32-bit float document").
//!
//! # Float masters in a byte file (word 72)
//!
//! An 8-bit document holding an f32 master stores it exactly as word 67
//! does (the same RSRC depth byte 32 and raw `w · h · 16` payload), under
//! [`BYTE_MASTER_VERSION`]: the FIRST word inside 45, ahead of 71, with no
//! table, written iff [`masters_present`]. The byte reader admits depth 32
//! only under 72 and refuses 72 with no depth-32 asset
//! ([`MASTER_WORD_REFUSAL`]) before any PNG decodes; a byte file without
//! one writes exactly the 71 writer's bytes, and an older byte build refuses
//! the file with "unsupported DRDC version 72".
//!
//! # Float RAW masters (word 78)
//!
//! A 32-bit float (linear) Camera RAW development keeps an f32 master with
//! its camera original: a depth-32 RSRC asset, stored exactly as word 67
//! stores it, whose RAW record (the v19 RSRC flag or the v22 `ATCH` slot) is
//! present. Before 78 both readers refused that pair ("retained RAW requires
//! a genuine RGBA16 developed master"), so the typed writer declares it:
//! [`RAW_MASTER_VERSION`] sits immediately before word 67, with no table,
//! written iff [`raw_masters_present`], and a document without such an asset
//! writes exactly the prior writer's bytes. The typed reader admits a RAW
//! record on a depth-32 asset only under 78, requires 67 behind it, and
//! refuses 78 with no such asset ([`RAW_MASTER_WORD_REFUSAL`]) before any
//! payload decodes. A build before 78 refuses such a file by its unknown
//! version word. Only a 32-bit float document holds such an asset
//! (`raw_source_edit`'s attach gate, `precision_conversion_narrow`'s block),
//! and the byte writer and reader keep refusing it: there is no byte word.
//!
//! The plan charges a float master as a float raster (`precision_native_plan`:
//! `raster_capacity` decoded, its bytes encoded, no codec terms, its pixels
//! in the reopen's `float_raster_pixels`).
//!
//! # Measurement record (phase 1)
//!
//! `retained_source_float`'s record's inputs (2026-10-10): 187a's
//! `target.rz` with the 6000×4000 sRGB f32 master placed at 0,0, in a
//! process over this core (`core_e`, `MallocLargeCache=0`, three runs): the
//! typed `save_native(path, 8 GiB, 0, None)`, and `open_native(path, 8 GiB,
//! 0, None)` of that file (1,152,006,702 bytes) written in setup with the
//! document dropped, each result held until E is read. A: `save` =
//! `native_plan().write_scratch_bytes`; `reopen` = `reopen_retained_bytes +
//! decode_scratch_bytes`. Raw output:
//! `doctored-fixtures/photoshop/float-placed-473/admission-600/runs/20261010-023712/`.
//!
//! | case | A | E (three runs) | A / max E |
//! | --- | --- | --- | --- |
//! | save32 | 2,305,062,044 | 1,152,598,856 / 1,152,598,856 / 1,152,615,240 | 2.000 |
//! | reopen32 | 2,954,129,452 | 1,539,949,696 / 1,539,916,904 / 1,539,933,288 | 1.918 |
//!
//! Both hold A ≥ ⌈1.25 × max E⌉, so `FLOAT_REOPEN_MARGIN_PER_PIXEL` stays as
//! it is; `core/tests/float_master_24mp.rs` pins each floor.
//!
//! # Measurement record (phase 3a)
//!
//! An f32 master in 8- and 16-bit documents, by phase 1's method (Release,
//! `MallocLargeCache=0`, limit 8,589,934,592, one case per process, three
//! runs, a `list_documents` baseline twin at E 0 ×3; 2026-10-10, Apple M2
//! Max). Targets: 6000×4000 opaque white untagged PNGs (sRGB) at 16 and 8
//! bits; masters phase 1's 6000×4000 f32 TIFFs (the scene's ICC, Display
//! P3); `narrow16` over 187a's float `target.rz`. Place / transform are the
//! Release app's MCP calls (A = the decoded master + the place plan's
//! scratch; the transform's `geometry_plan` scratch at 140 %); the rest run
//! in a process over this core (save / reopen as phase 1's; commit = 2b's
//! `commit32m` through `dr_*_mixed_source_edit`, A its plan's scratch;
//! narrow = `narrow_structure_to_u16`, A its plan's scratch + output).
//! Harness and raw output:
//! `doctored-fixtures/photoshop/float-placed-473/admission-600p3a/runs/20261010-095930/`.
//!
//! | case | A | max E | A / max E |
//! | --- | --- | --- | --- |
//! | place16 | 1,541,830,692 | 1,158,071,184 | 1.331 |
//! | place16p3 | 1,541,824,548 | 1,158,103,952 | 1.331 |
//! | place8 | 1,637,832,166 | 1,160,184,696 | 1.412 |
//! | transform16 | 3,094,194,285 | 762,364,456 | 4.059 |
//! | save16 | 966,675,373 | 768,492,072 | 1.258 |
//! | reopen16 | 1,928,907,533 | 770,736,680 | 2.503 |
//! | save8 | 971,880,773 | 385,450,272 | 2.521 |
//! | reopen8 | 1,579,160,704 | 481,444,176 | 3.280 |
//! | commit16 | 1,733,830,692 | 771,260,992 | 2.248 |
//! | commit8 | 1,733,836,580 | 672,973,280 | 2.576 |
//! | narrow16 | 772,194,304 | 575,570,352 | 1.342 |
//!
//! Every case holds A ≥ ⌈1.25 × max E⌉ with no change:
//! `retained_source_render::FLOAT_ENCODE_BYTES_PER_PIXEL` stays 24, the byte
//! reopen's `FLOAT_REOPEN_MARGIN_PER_PIXEL` term stays, and the host's
//! `ImageSourceFile.scratchPerPixel32` fits at 50 (≤ 60) for every place.
//! `core/tests/float_master_24mp.rs` pins each floor. The typed save and
//! reopen rows of both records (save32, reopen32, save16, reopen16) are
//! superseded by the streamed writer and reader's (below).
//!
//! # Measurement record (the streamed reader and writer)
//!
//! `float_master_24mp.rs`'s `stream_measure` (2026-10-11, Release through
//! `make t`, `MallocLargeCache=0`, limit 8,589,934,592, one case per
//! process, three runs, a `base` twin at E 0 ×3; a reopen's file is built
//! and saved by a child process, so the measured process never held the
//! document). save32 and reopen32: phase 1's float document with the sRGB
//! f32 master placed; save16 and reopen16: phase 3a's white 16-bit document
//! with the scene-profile master placed `convert_fallback`. A: a save's
//! `write_scratch_bytes`, a reopen's reservation read inside
//! `open_native_observed`'s hook; H the plan's figure. Raw output:
//! `~/.claude/litecode/rasterize/lane-b-v1-rz-stream/measure/fm/`.
//!
//! | case | H | A | E (three runs) | A / max E |
//! | --- | --- | --- | --- | --- |
//! | save32 | 6,567,240 | 6,567,240 | 16,384 / 49,152 / 32,768 | 133.6 |
//! | reopen32 | 1,806,317,808 | 1,806,317,198 | 1,160,069,960 ×3 | 1.557 |
//! | save16 | 11,871,470 | 11,871,470 | 311,296 / 360,448 / 376,832 | 31.5 |
//! | reopen16 | 974,103,630 | 974,102,988 | 388,399,512 ×2 / 388,415,896 | 2.508 |
//!
//! A save's E is what the allocator could not serve from memory the setup
//! had freed (the sink's 1 MiB buffer and the IDAT window are reused), so
//! it reads below the writer's buffers; `native_file_size`'s save cases,
//! with less freed beside them, read about 1.2 MB. Every case holds H ≥ A ≥
//! ⌈1.25 × max E⌉, so `FLOAT_REOPEN_MARGIN_PER_PIXEL` stays 9, and the four
//! floors in `float_master_24mp.rs` are re-pinned from these maxima.

use std::sync::Arc;

use crate::drdc::Reader;
use crate::icc::IccProfile;
use crate::job_control::JobContext;
use crate::native_stream::{NativeSink, Payload};
use crate::precision_document::PrecisionDocument;
use crate::precision_image::{pixel_count, PrecisionImage};
use crate::precision_native::{append, MAX_DECODED_BYTES};
use crate::tile_store::TilePlane;

pub const PRECISION_VERSION: u32 = 40;

/// The float-master word (module doc, `# Float masters`).
pub const MASTER_VERSION: u32 = 67;

/// The byte file's float-master word (module doc, `# Float masters in a
/// byte file`).
pub(crate) const BYTE_MASTER_VERSION: u32 = 72;

/// Word 67 (or 72) with no depth-32 asset in the source table.
pub(crate) const MASTER_WORD_REFUSAL: &str = "invalid native float master word";

/// Whether some asset is a 32-bit float master, the writer's word-67 (and
/// word-72) rule.
pub(crate) fn masters_present(assets: &[Arc<crate::source_asset::SourceAsset>]) -> bool {
    assets.iter().any(|asset| asset.depth() == 32)
}

/// The typed float RAW master word (module doc, `# Float RAW masters`).
pub const RAW_MASTER_VERSION: u32 = 78;

/// Word 78 with no depth-32 RAW asset in the source table, or without word
/// 67 behind it.
pub(crate) const RAW_MASTER_WORD_REFUSAL: &str = "invalid native float RAW master word";

/// Whether some 32-bit float master carries a camera original, the typed
/// writer's word-78 rule.
pub(crate) fn raw_masters_present(assets: &[Arc<crate::source_asset::SourceAsset>]) -> bool {
    assets
        .iter()
        .any(|asset| asset.depth() == 32 && asset.raw.is_some())
}

/// The body header's storage tag for extended-range f32 samples. Tag 1
/// (RGBA16 UNORM) keeps its meaning and is the only tag any prior reader
/// admits.
pub(crate) const STORAGE_TAG_FLOAT: u32 = 2;

/// The version word plus the table — `precision_native`'s word 40 and
/// `"FLOT"` + u32 schema + u32 operator + f32 white — or 0 when neither is
/// written: the exact count the writer writes (the exact PNG bound leaves
/// the plan no slack; `evaluation_native::header_bytes` is the same rule).
pub(crate) fn header_bytes(present: bool) -> u64 {
    if present {
        20
    } else {
        0
    }
}

/// The raw payload of one float raster: `w · h · 16` bytes.
pub(crate) fn raster_capacity(dimensions: (u32, u32), total: &mut u64) -> Result<u64, String> {
    let pixels = pixel_count(dimensions.0, dimensions.1)? as u64;
    let bytes = pixels
        .checked_mul(16)
        .ok_or("float native raster byte overflow")?;
    *total = total
        .checked_add(bytes)
        .ok_or("float native raster byte overflow")?;
    if *total > MAX_DECODED_BYTES {
        return Err(
            "high-depth native serialized pixels exceed the shared byte limit; remove layers or reduce dimensions"
                .into(),
        );
    }
    Ok(bytes)
}

/// Writes one float raster record's payload: its dimensions, then
/// [`put_samples`].
pub(crate) fn put_raster(
    out: &mut dyn NativeSink,
    image: &PrecisionImage,
    bound: u64,
    job: JobContext<'_>,
) -> Result<(), String> {
    let (width, height) = image.dimensions();
    for value in [width, height] {
        append(out, &value.to_le_bytes(), bound, job)?;
    }
    put_samples(out, image, bound, job)
}

/// Writes a u64 LE length `w · h · 16`, then the samples as LE f32 RGBA in
/// row order: a float raster's payload after its dimensions, and a float
/// master's whole payload slot (module doc).
pub(crate) fn put_samples(
    out: &mut dyn NativeSink,
    image: &PrecisionImage,
    bound: u64,
    job: JobContext<'_>,
) -> Result<(), String> {
    let runs = image
        .runs_rgba32()
        .ok_or("a float native raster requires rgba32_float storage")?;
    let (width, height) = image.dimensions();
    let length = u64::from(width) * u64::from(height) * 16;
    append(out, &length.to_le_bytes(), bound, job)?;
    let mut bytes = [0_u8; 4096 * 16];
    let mut staged = 0;
    for mut run in runs {
        while !run.is_empty() {
            let (chunk, rest) = run.split_at((4096 - staged).min(run.len()));
            let dest = bytes[staged * 16..].chunks_exact_mut(16);
            for (pixel, dest) in chunk.iter().zip(dest) {
                for (value, slot) in pixel.iter().zip(dest.chunks_exact_mut(4)) {
                    slot.copy_from_slice(&value.to_le_bytes());
                }
            }
            staged += chunk.len();
            run = rest;
            if staged == 4096 {
                job.check()?;
                append(out, &bytes, bound, job)?;
                staged = 0;
            }
        }
    }
    if staged > 0 {
        job.check()?;
        append(out, &bytes[..staged * 16], bound, job)?;
    }
    Ok(())
}

/// Records one float raster record's payload as a span (`native_stream`)
/// after checking its declared length against its dimensions.
pub(crate) fn take_raster<'a>(
    reader: &mut Reader<'a>,
    dimensions: (u32, u32),
    decoded: &mut u64,
) -> Result<Payload<'a>, String> {
    let bytes = raster_capacity(dimensions, decoded)?;
    if reader.u64()? != bytes {
        return Err("float native raster byte length differs from its dimensions".into());
    }
    reader.take_payload(bytes)
}

/// The pixels one span read decodes: 65,536 (1 MiB), the reader's section
/// share of `NATIVE_STREAM_WINDOW_BYTES`, so a row of any width is read in
/// bounded blocks.
const READ_BLOCK_PIXELS: usize = 64 * 1024;

/// Materializes a payload, row by row straight into tiles (the tiles
/// and one row are all it holds beside the input). A slice is
/// read in place; a file span is read in blocks of at most
/// [`READ_BLOCK_PIXELS`], so neither the file nor a whole plane is
/// held. Every sample goes through the storage invariant
/// (`from_rgba32_tiles_decoded`), so a file carrying a NaN or an
/// out-of-range magnitude refuses rather than becoming a document the
/// editor could never have made.
pub(crate) fn decode_raster(
    data: &Payload<'_>,
    dimensions: (u32, u32),
    profile: Arc<IccProfile>,
    job: JobContext<'_>,
) -> Result<PrecisionImage, String> {
    let (width, height) = dimensions;
    let count = pixel_count(width, height)?;
    if data.len() != count as u64 * 16 {
        return Err("precision pixel buffer length does not match dimensions".into());
    }
    let stride = width as usize;
    let decode = |first: usize, row: &mut [[f32; 4]], bytes: &[u8]| {
        for (x, (pixel, chunk)) in row.iter_mut().zip(bytes.chunks_exact(16)).enumerate() {
            job.every(first + x)?;
            *pixel = std::array::from_fn(|c| {
                let at = c * 4;
                f32::from_le_bytes([chunk[at], chunk[at + 1], chunk[at + 2], chunk[at + 3]])
            });
        }
        Ok::<(), String>(())
    };
    let plane = match data {
        Payload::Bytes(data) => {
            let fill_row = |y: u32, row: &mut [[f32; 4]]| {
                let first = y as usize * stride;
                decode(first, row, &data[first * 16..(first + stride) * 16])
            };
            TilePlane::from_rows_with_job(width, height, fill_row, JobContext::NONE)?
        }
        Payload::File { .. } => {
            let mut block =
                crate::precision_image::reserved_vec(stride.min(READ_BLOCK_PIXELS) * 16)?;
            block.resize(stride.min(READ_BLOCK_PIXELS) * 16, 0_u8);
            let fill_row = |y: u32, row: &mut [[f32; 4]]| {
                let first = y as usize * stride;
                for (index, part) in row.chunks_mut(READ_BLOCK_PIXELS).enumerate() {
                    let at = first + index * READ_BLOCK_PIXELS;
                    let bytes = &mut block[..part.len() * 16];
                    data.read_at(at as u64 * 16, bytes)?;
                    decode(at, part, bytes)?;
                }
                Ok(())
            };
            TilePlane::from_rows_with_job(width, height, fill_row, JobContext::NONE)?
        }
    };
    job.check()?;
    PrecisionImage::from_rgba32_tiles_decoded(plane, profile)
}

/// Writes the `FLOT` table for a float document; writes nothing otherwise.
pub(crate) fn encode(
    document: &PrecisionDocument,
    out: &mut Vec<u8>,
    bound: u64,
    job: JobContext<'_>,
) -> Result<(), String> {
    let Some(map) = document.tone_map() else {
        return Ok(());
    };
    append(out, b"FLOT", bound, job)?;
    for value in [1_u32, map.code()] {
        append(out, &value.to_le_bytes(), bound, job)?;
    }
    append(out, &map.white().to_le_bytes(), bound, job)
}

/// Reads the table. A schema, an operator or a white point this build does
/// not admit is a refusal, not a repair.
pub(crate) fn parse(reader: &mut Reader<'_>) -> Result<crate::float_document::ToneMap, String> {
    if reader.take(4)? != b"FLOT" || reader.u32()? != 1 {
        return Err("unsupported native float schema".into());
    }
    let operator = reader.u32()?;
    let white = reader.f32()?;
    crate::float_document::ToneMap::from_code(operator, white)
}
