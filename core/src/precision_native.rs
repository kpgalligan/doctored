//! Preserving DRDC version 13 for the separate u16 document type.
//!
//! All integers are little-endian. Header: `DRDC`, u32 version=13, storage=1
//! (straight RGBA16 UNORM), evaluation=1 (encoded RGB/f32 Normal evaluation),
//! canvas width, height, root count and total record count. Then u32 ICC length
//! plus ICC bytes, f32 x/y PPI, and EXIF/XMP/IPTC slots (u8 present 0/1, and
//! when present u32 length plus verbatim bytes). Metadata has the existing core
//! packet conventions; no capture metadata or profile is duplicated in pixels.
//!
//! Root/child arrays are bottom-first. Records are recursive PREORDER: u8 kind
//! (0 raster, 1 isolated Normal group, 2 pass-through group), u8 visible 0/1, u16 reserved=0, f32
//! opacity, u32 UTF-8 name length plus name, i32 x/y placement. A raster adds
//! u32 pixel width/height and u64 PNG length plus strict native RGBA16 PNG bytes.
//! Version 12 adds kind 3 adjustment: zero placement then u32 UTF-8 JSON length
//! and canonical operator description carrying evaluation="precision_v1". Version
//! 11 remains readable and permits only kinds 0–2. All operators are validated
//! against the captured profile before any raster is decoded.
//! A group has zero placement and adds u32 child count followed by its records.
//! Version 13 inserts a mask slot after each record placement: u8 presence;
//! when present u8 enabled, u8 linked, u16 reserved=0, i32 x/y, u32 width/height,
//! f32 density/feather, u64 byte length and raw little-endian u16 coverage.
//! After all roots, u32 channel count and channels: u32 UTF-8 name length/name,
//! u64 byte length and canvas-sized little-endian u16 coverage. Channels never
//! composite; their session IDs are reminted on load. Versions 11/12 have no
//! slots/tail and migrate to absent masks/empty channels.
//! Public document indices remain postorder (children before their group).
//! Unknown tags/kinds, malformed UTF-8, nonfinite values, trailing bytes and
//! mismatched dimensions refuse. Empty groups are valid; empty documents are not.
//!
//! Version 15 appends the common `RSRC` embedded source asset/reference table
//! specified in `native_sources.rs`, preserving each original RGBA16 image and
//! profile exactly once while retaining ordinary raster fallbacks. Bindings use
//! public postorder indices. Source-free typed documents keep writing version 13.
//! Version17 adds actual original depth/revision and per-binding linked path/
//! fingerprint slots (native_sources.rs). Source PNGs remain embedded, so
//! portable reopen and recovery require no access to a linked file. Versions
//! 14/16/18 are byte documents and are refused here.
//! Version 19 adds original encoded RAW payloads and captured effective recipe/
//! decoder descriptors (native_sources.rs). Saved RGBA16 masters and fallbacks
//! reopen without any camera decoder; development is an explicit host action.
//! Version20 wraps the existing optional source table with its schema marker and
//! appends the ADSC artwork table. Version22 uses the shared RAW/generic binary
//! pool specified in native_sources.rs and also ends with ADSC (possibly empty).
//! Byte21 is refused here. Prior13/15/17/19/20 files retain their own readers;
//! writers choose22 only when generic source attachments require the new pool.
//! Version24 (written until 2026-10-10) additionally stored per-binding
//! live-filter descriptions and mask references in LFLT after RBIN, then ADSC;
//! byte23 is refused here. The reader skips LFLT and drops each filtered
//! binding (native_sources.rs).
//! Version25 adds persistent channel display settings and auxiliary provenance;
//! `precision_channel_metadata.rs` defines its prior-body marker and CHPR tail.
//!
//! Version26 wraps any prior typed body (including25) by inserting u32=26
//! before its version. Its final LBLD table, specified in precision_blend_native.rs,
//! adds version1 encoded-RGB Normal/Multiply/Screen layer/group evaluation.
//! Old bodies keep their exact framing; records absent from LBLD retain their
//! original mode. Multiply/Screen-only documents write26/schema1; any added
//! schema2 mode writes27/schema2. Color Dodge/Burn, Addition or Linear Burn
//! require30/schema3. Subtract, Vivid/Linear/Pin Light or Divide require31/schema4;
//! Hue, Saturation, Color or Luminosity require33/schema5; Dissolve, Darker/
//! Lighter Color or Hard Mix require34/schema6;
//! older schema mode sets are frozen. All wrap the same
//! complete older bodies and keep their exact conditional downgrade writers.
//! If outside-mask metadata is present, version29 wraps the blend version:
//! 29 -> 34 -> prior body/tails -> LBLD schema6 -> MOUT schema1. Blend31/33/34
//! never wraps29; each optional outer feature can be removed independently.
//! If any record is a clipping member, version32 wraps whatever follows:
//! 32 -> 29 -> 34 -> prior body/tails -> LBLD6 -> MOUT -> LCLP schema1
//! (precision_clip_native.rs). Neither29 nor31/33/34 ever wraps32; releasing the last
//! flag restores the exact prior writer's bytes.
//! If any raster/isolated-group record carries Blend If, version35 wraps
//! whatever follows: 35 -> 32 -> 29 -> 34 -> prior body/tails -> LBLD6 -> MOUT
//! -> LCLP -> LBIF schema1 (precision_blend_if_native.rs; schema 2, a
//! multichannel Blend If, only under word 101). Nothing wraps35;
//! releasing the last Blend If restores the exact prior writer's bytes.
//! If any raster/isolated-group record carries a layer style (fill opacity or
//! effects), version36 wraps whatever follows: 36 -> 35 -> 32 -> 29 -> 34 ->
//! prior body/tails -> LBLD6 -> MOUT -> LCLP -> LBIF -> LSTY schema1
//! (precision_style_native.rs). Nothing wraps36; releasing the last style
//! restores the exact prior writer's bytes.
//! If any nondefault blend record is an ADJUSTMENT layer, the blend word is
//! 37/LBLD schema7 instead (precision_blend_native.rs; no new mode, only the
//! new kind-3 target), in the same slot 26/27/30/31/33/34 occupy:
//! 36 -> 35 -> 32 -> 29 -> 37 -> prior body/tails -> LBLD7 -> MOUT -> LCLP ->
//! LBIF -> LSTY. Releasing the last adjustment mode restores the prior
//! writer's exact bytes.
//! If the document's evaluation policy is `linear_light`, version39 wraps
//! whatever follows and its EVAL table closes the file (evaluation_native.rs):
//! 39 -> 36 -> 35 -> 32 -> 29 -> 37 -> prior body/tails -> LBLD -> MOUT ->
//! LCLP -> LBIF -> LSTY -> EVAL schema1. Nothing wraps39; an ENCODED document
//! writes neither the word nor the table, so its bytes stay exactly the prior
//! writer's.
//!
//! If any record carries a COLOUR LABEL, version42 wraps whatever follows and
//! its LLBL table closes the file (label_native.rs):
//! 42 -> 40 -> 39 -> 36 -> 35 -> 32 -> 29 -> 37 -> prior body/tails -> LBLD ->
//! MOUT -> LCLP -> LBIF -> LSTY -> EVAL -> FLOT -> LLBL schema1. Nothing wraps
//! 42; an unlabelled document writes neither the word nor the table, so its
//! bytes stay exactly the prior writer's.
//!
//! If the document holds any LAYER COMP, version44 wraps whatever follows
//! and its LCMP table closes the file (comps_native.rs):
//! 44 -> 42 -> 40 -> 39 -> 36 -> 35 -> 32 -> 29 -> 37 -> prior body/tails ->
//! LBLD -> MOUT -> LCLP -> LBIF -> LSTY -> EVAL -> FLOT -> LLBL -> LCMP
//! schema1. Nothing wraps44; a comp-less document writes neither the word
//! nor the table, so its bytes stay exactly the prior writer's.
//!
//! If any record carries a LOCK or a LINK, version47 sits INSIDE the
//! identity wrapper — one of the two words that do — and its LLNK table
//! lands after LCMP and before LIDS (precision_lock_native.rs):
//! 46 -> 47 -> 44 -> 42 -> 40 -> 39 -> ... -> LLBL -> LCMP -> LLNK -> LIDS.
//! 12E-1's two invariants hold unchanged: 46 is still the FIRST word and
//! LIDS is still the LAST table. A document with neither locks nor links
//! writes neither the word nor the table, so its bytes stay exactly the
//! 12E-1 writer's, and releasing the last lock restores them.
//!
//! If the document carries a GUIDE or has moved its RULER ORIGIN, version48
//! sits inside the identity wrapper beside47 and its LGDE table lands after
//! LLNK and before LIDS (precision_guide_native.rs):
//! 46 -> 47 -> 48 -> 44 -> 42 -> ... -> LCMP -> LLNK -> LGDE -> LIDS. The
//! same two invariants hold. A document with no guide and the origin still
//! at (0,0) writes neither the word nor the table, so its bytes stay exactly
//! the 12E-1b writer's, and clearing the last guide restores them.
//!
//! If any group is CLOSED (its Layers-panel disclosure), version55 is the
//! first word inside the identity wrapper after 57 when present, and its LOPN
//! table is the last one before RSPR or LIDS (precision_group_open_native.rs):
//! 46 -> 55 -> 54 -> 52 -> 47 -> 48 -> 50 -> 44 -> ... -> LLNK -> LGDE ->
//! (projective) -> WARP -> LOPN -> LIDS. The same two invariants hold. A
//! document whose groups are all open writes neither the word nor the table,
//! so its bytes stay exactly the 54 writer's, and reopening the last closed
//! group restores them.
//!
//! If any retained source asset carries a PROFILE REFUSAL, version57
//! is the FIRST word inside the identity wrapper, ahead of55, and its RSPR
//! table is the last one before LIDS (source_refusal_native.rs):
//! 46 -> 57 -> 55 -> 54 -> 52 -> 47 -> ... -> (projective) -> WARP -> LOPN ->
//! RSPR -> LIDS. The same two invariants hold. A document with no refusal
//! writes neither the word nor the table, so its bytes stay exactly the 55
//! writer's.
//!
//! If the document's GLOBAL LIGHT is not the default, version58 is the
//! FIRST word inside the identity wrapper, ahead of57, and its LGLT table is
//! the last one before LIDS (precision_light_native.rs):
//! 46 -> 58 -> 57 -> 55 -> 54 -> 52 -> 47 -> ... -> WARP -> LOPN -> RSPR ->
//! LGLT -> LIDS. The same two invariants hold. A document with the default
//! light writes neither the word nor the table, so its bytes stay exactly
//! the 57 writer's.
//!
//! If the document holds any COLOUR SAMPLER, version60 is the FIRST
//! word inside the identity wrapper, ahead of58, and its CSMP table is the
//! last one before LIDS (sampler_native.rs):
//! 46 -> 60 -> 58 -> 57 -> 55 -> ... -> LOPN -> RSPR -> LGLT -> CSMP -> LIDS.
//! The same two invariants hold. A document with no sampler writes neither
//! the word nor the table, so its bytes stay exactly the 58 writer's.
//!
//! If some levels adjustment carries an output range or a per-channel stage,
//! version62 is the FIRST word inside the identity wrapper,
//! ahead of60, with NO table — the keys ride inside the record's adjustment
//! description (levels_native.rs): 46 -> 62 -> 60 -> 58 -> ... A document
//! with no such adjustment writes nothing here, so its bytes stay exactly
//! the 60 writer's.
//!
//! If some curves adjustment carries a 256-entry map (a
//! pencil-drawn curve), version64 is the FIRST word inside the identity
//! wrapper, ahead of62, with NO table — the maps ride inside the record's
//! adjustment description (curves_native.rs): 46 -> 64 -> 62 -> 60 -> ...
//! A document with no such adjustment writes nothing here, so its bytes
//! stay exactly the 62 writer's.
//!
//! Until 2026-10-10 a document holding smart-object contents wrote word 66
//! (ahead of 64) and a SOCT table last before LIDS, float contents word 68
//! (ahead of 67), and a Stack Mode word 70 (ahead of 68) and a SOSM table
//! after SOCT: 46 -> 71 -> 70 -> 68 -> 67 -> 66 -> 64 -> ... -> CSMP -> SOCT
//! -> SOSM -> LIDS. The reader still accepts and skips them
//! (native_sources.rs); this build writes none of them.
//!
//! If some written source asset is a 32-bit FLOAT master,
//! version 67 is the FIRST word inside the identity wrapper after 71, ahead
//! of 64, with NO table — the master's raw f32 payload rides in its RSRC
//! asset at depth 32 (float_native.rs): 46 -> 67 -> 64 -> ... The same two
//! invariants hold. A document with no float master writes nothing here, so
//! its bytes stay exactly the prior writer's.
//!
//! If some Color Lookup adjustment keeps an ABSTRACT profile
//! (`abstract_profile`), version 71 — one number at both depths — is the
//! FIRST word inside the identity wrapper, ahead of 67, with NO table — the
//! profile rides inside the record's adjustment description
//! (look_native.rs): 46 -> 71 -> 67 -> 64 -> ... The same two invariants
//! hold. A document with no such adjustment writes nothing here, so its bytes
//! stay exactly the prior writer's.
//!
//! If the document holds any PATH or live VECTOR MASK, version 75 is
//! the next word inside the identity wrapper after 79, ahead of 71, and its PATH
//! table is the last one before LIDS (path_native.rs): 46 -> 75 -> 71 -> 67
//! -> ... -> LGLT -> CSMP -> PATH -> LIDS. The table's raster-part planes are
//! raw u16 LE, and a vector-masked record's own mask is the baked plane. The
//! same two invariants hold. A document with neither writes neither the word
//! nor the table, so its bytes stay exactly the prior writer's.
//!
//! If the written RAW sources hold an attachment of more than four resources
//! or more than 512 MiB of RAW and attachment resources together (16
//! local masks), version 79 — one number at both depths — is the FIRST word
//! inside the identity wrapper, ahead of 75, with NO table: the masks ride
//! in the ATCH/RBIN pool (raw_locals_native.rs): 46 -> 79 -> 75 -> 71 -> ...
//! The reader checks the claim both ways right after the source table is
//! parsed. The same two invariants hold. A document with no such sources
//! writes nothing here, so its bytes stay exactly the prior writer's.
//!
//! Version 114 (clipping_path_native.rs) is the word right after the CMYK
//! slot inside 46, ahead of 113, with no table: some path is the clipping
//! path, its key in the PATH table's entry, checked both ways once that
//! table is read. The next free word is 115.
//!
//! Version 113 (background_native.rs) is the next word after the CMYK slot
//! inside 46, ahead of 112: the document's Background mark names a record.
//! Its `LBKG` table follows SPOT and precedes the CMYK table; the reader binds
//! it to the record's final id.
//!
//! Version 112 (spot_channel_native.rs, #1) is the next word after the
//! CMYK slot inside 46, ahead of 111: some channel is a spot channel. Its
//! `SPOT` table follows LREF and precedes LBKG and the CMYK table; the reader
//! applies it after CHPR and refuses it in an Indexed or Bitmap file.
//!
//! Version 111 (gradient_map_method_native.rs) is the next word after
//! the CMYK slot inside 46, ahead of 105, with no table: some adjustment
//! record's description (RGB or CMYK) is a Gradient Map with a `method`. The
//! reader checks it both ways with the adjustment words, before any payload
//! decodes.
//!
//! Version 105 (fill_align_native.rs) is the next word after the CMYK
//! slot inside 46, ahead of 104, with no table: some record's artwork
//! description is a Gradient Fill with `align_with_layer` true. Version 104
//! (gradient_midpoint_native.rs) follows it, ahead of 103, with no
//! table: some record's or comp's style, or some Gradient Fill description,
//! holds a stop midpoint other than 50 %. Version 103
//! (gradient_smoothness_native.rs) follows it, ahead of 102, with no
//! table: some record's or comp's style, or some Gradient Fill description,
//! holds a Smoothness below 100 %. The reader checks each both ways before
//! any payload decodes (105 after the ADSC parse, 104 and 103 after LCMP). A
//! document with none writes nothing here.
//!
//! Version 102 (large_description_native.rs) is the word after the
//! CMYK slot, 105, 104 and 103 inside 46, ahead of 101, with no table: some record's
//! artwork description (its `ADSC` entry) is over 64 KiB, and the reader then
//! admits entries up to the 16 MiB meta cap. Without the word an entry over
//! 64 KiB is refused by size; the word without one is refused by number. A
//! document with none writes nothing here.
//!
//! Version 101 (blend_if_channels_native.rs) is the word
//! after the CMYK slot and 102 inside 46, ahead of 100, with no table of its
//! own: some record or comp holds a multichannel Blend If, whose records'
//! ranges ride in `LBIF` schema 2. A document with none writes nothing
//! here.
//!
//! Version 100 (wide_color_native.rs) is the word after the CMYK
//! slot and 101 inside 46, ahead of 99, with no table: some record or comp
//! holds a style colour slot carrying a wide colour. A document with none
//! writes nothing here.
//!
//! Version 99 (glow_gradient_native.rs) is the word after the CMYK
//! slot and 100 inside 46, ahead of 98, with no table: some record or comp
//! holds an Outer or Inner Glow carrying a gradient or Jitter. A document
//! with none writes nothing here.
//!
//! Version 98 (texture_native.rs) is the word after the CMYK
//! slot and 99 inside 46, ahead of 97, with no table: some record or comp
//! holds a Bevel & Emboss carrying a texture. A document with none writes
//! nothing here.
//!
//! Version 97 (noise_native.rs) is the word after the
//! CMYK slot and 98 inside 46, ahead of 96, with no table: some record or
//! comp holds a layer style Noise above 0. A document with none writes
//! nothing here.
//!
//! Version 96 (contour_native.rs) is the word after the
//! CMYK slot and 97 inside 46, ahead of 95, with no table: some record or
//! comp holds a layer style contour-family key. A document with none writes
//! nothing here.
//!
//! Version 95 (stroke_pattern_native.rs) is the word after the
//! CMYK slot and 96 inside 46, ahead of 94, with no table: some record or
//! comp holds a stroke carrying a pattern fill. A document with none writes
//! nothing here.
//!
//! Version 94 (gradient_options_native.rs) is the word after
//! the CMYK slot and 95 inside 46, ahead of 93, with no table: some record or
//! comp holds a Shape Burst stroke gradient or a dithered stroke or overlay
//! gradient. A document with none writes nothing here.
//!
//! Version 93 (pattern_angle_native.rs) is the word after the
//! CMYK slot and 94 inside 46, ahead of 92, with no table: some record or
//! comp holds a pattern overlay with a non-zero angle (it implies 90). A
//! document with none writes nothing here.
//!
//! Version 92 (generated_native.rs) is the word after the CMYK slot
//! and 93 inside 46, ahead of 91, a CMYK document's too: some raster holds
//! live Generative Layer variations, and its `GVAR` table follows PATH and
//! precedes `LREF`. A document with none writes neither.
//!
//! Version 91 (reference_native.rs) is the word after the CMYK slot,
//! 93 and 92 inside 46, ahead of 90 (`style_pattern`'s word, no table) and 89:
//! some record moved since it was made (Photoshop's `fxrp`, a reference
//! point other than (0, 0)), and its `LREF` table — u32 schema=1, u32 count,
//! then per entry a u32 ascending postorder index and i32 x, i32 y — follows
//! every prior tail but CMYK's and LIDS. A document whose records never moved
//! writes neither, so its bytes stay exactly the prior writer's.
//!
//! Version46 is the OUTERMOST wrapper (identity_native.rs) and the ONE table
//! this format always writes: 46 before every prior version word — 44
//! included — and `LIDS`, u32 schema=1, u32 count, then count u64 LE record
//! ids in POSTORDER close the file after every prior tail, LCMP included:
//! 46 -> 44 -> 42 -> 40 -> 39 -> 36 -> 35 -> 32 -> 29 -> 37 -> prior
//! body/tails -> LBLD -> MOUT -> LCLP -> LBIF -> LSTY -> EVAL -> FLOT ->
//! LLBL -> LCMP -> LIDS schema1. An id is a property of every record, so the
//! table is unconditional; a file with no 46 word reads exactly as before and
//! MINTS fresh ids, and the reader ADOPTS the ids it reads so reopen → resave
//! is byte-identical.
//!
//! If the document is a 32-bit FLOAT document, version40 wraps whatever
//! follows and its FLOT table closes the file (float_native.rs):
//! 40 -> 39 -> 36 -> 35 -> 32 -> 29 -> 37 -> prior body/tails -> LBLD -> MOUT
//! -> LCLP -> LBIF -> LSTY -> EVAL -> FLOT schema1. Under40 the body header's
//! storage tag is2 (extended-range straight RGBA32 float) and each raster
//! record's payload is u32 width, u32 height, u64 length = w*h*16 and raw
//! little-endian f32 RGBA instead of a PNG. A float document is linear light
//! by construction, so the word after40 is always39 and EVAL is always
//! present; anything else refuses before any pixel is read. Storage tag2 is
//! admitted only under40, so every earlier reader — which knows neither the
//! word nor the tag — refuses the file rather than misreading its samples.
//!
//! Version 1–10 byte documents retain their old reader/writer and evaluation.
//! A byte reader rejects version 13 before touching its pixels. Native capacity
//! counts serialized planes even when editing shares one allocation. Complete
//! borrowed-record preflight precedes all pixel decoding. The same size checks
//! protect model edits and writing; runtime admission separately includes native
//! bytes, retained pixels, worst codec scratch and caller-supplied live payload.
//!
//! byte documents retain their v10 writer and refuse typed versions before decoding.
//!
//! # Size caps
//! [`MAX_NATIVE_BYTES`] (the file) and [`MAX_DECODED_BYTES`] (its decoded
//! pixels) are both 8 GiB. The format is sequential and stores no offset.
//! Every length whose payload can pass 4 GiB in total is a u64 (a raster PNG,
//! a float raster, a mask or channel plane, a WARP original, an RSRC asset
//! payload, an RRAW or RBIN blob, a skipped SOCT entry), and every u32 is a
//! count, a dimension, a version or schema word, an index, or the length of a
//! slot capped at 16 MiB or less. One plane stays under `MAX_PIXELS` and
//! `MAX_DRDC_PNG_LEN` (1 GiB), so a PNG keeps its single IDAT below 2^31 − 1
//! bytes whatever the file's size. Raising the caps from 2 GiB and 1.6 GB
//! therefore changed no byte the writer produces, and there was no
//! `DRDC_VERSION` bump. The reader does not buffer the file: it reads
//! record by record with positional reads, and the
//! writer streams to the temporary file (`native_stream`,
//! `precision_native_write`), so neither plan charges the file
//! (`precision_native_plan`'s `# Streamed reader and writer`). At an
//! 8 GiB working limit the reopen plan binds first: eight 100 MP 16-bit
//! layers (a 6.4 GB file) plan and a ninth is refused.
//! `tests/native_file_size.rs` holds the caps and, in its ignored cases,
//! documents saved and reopened past 2^31 and past 2^32 bytes at 8 GiB.
//! `encode_native` and `parse_native` are the in-memory twins (tests and
//! examples): their buffer is the caller's and is not charged, so each
//! admits exactly the figure `save_native` and `open_native` admit.

use crate::dr_image::save_atomically;
use crate::job_control::{DrJobControl, JobContext};
use crate::native_stream::{FileSink, NativeFile, Reader, NATIVE_STREAM_WINDOW_BYTES};
use crate::precision_document::PrecisionDocument;
use crate::precision_image::{pixel_count, BOOKKEEPING_BYTES};
#[doc(hidden)]
pub use crate::precision_native_plan::native_color_plan;

#[path = "precision_native_read.rs"]
mod read;
#[path = "precision_native_write.rs"]
mod write;
pub(crate) use write::append;

pub(crate) const STORAGE_TAG: u32 = 1;
pub(crate) const EVALUATION_TAG: u32 = 1;
/// The typed format's cap on the decoded bytes of one file: every raster,
/// mask, channel, embedded original and WARP original, each counted
/// separately (the writer's plan and the reader's preflight read it). 8 GiB,
/// the working limit of the assumed minimum machine (a 16 GB Mac): a document
/// that limit cannot hold is not one the format needs to store, and on a
/// larger machine the cap, not the limit, is what refuses. Its own literal,
/// not the byte format's `drdc::MAX_DRDC_DECODED_BYTES` (1.6 GB), which it
/// once equalled.
pub(crate) const MAX_DECODED_BYTES: u64 = 8 * 1024 * 1024 * 1024;
/// The typed format's file-length cap (the reader and the plan's encoded
/// bound). It is NOT the working limit: it stays 8 GiB whatever limit the
/// process runs at. 8 GiB because the 16-bit PNG is stored, so a file is
/// about as long as its decoded pixels and the two caps move together; the
/// module doc's `# Size caps` records why no length in the format is
/// too narrow for it. It was formerly 2 GiB.
pub(crate) const MAX_NATIVE_BYTES: u64 = 8 * 1024 * 1024 * 1024;

#[derive(Clone, Copy, Debug)]
pub struct PrecisionNativePlan {
    pub encoded_bound: u64,
    pub serialized_pixel_bytes: u64,
    pub reopen_retained_bytes: u64,
    pub decode_scratch_bytes: u64,
    /// The streamed save's figure beside the retained pixels: twice
    /// the header (the pre-built tables and one scratch table), the largest
    /// writer transient (one row, `native_png_encode_scratch`; an 8-bit
    /// original's encode), the sink's window and bookkeeping, plus a quarter
    /// of that sum, at both depths (`precision_native_plan`'s record). Not
    /// retained input pixels.
    pub write_scratch_bytes: u64,
}

/// Shared static record-capacity rule, before encoding and before decoding.
/// A 16-bit plane never passes through an `image` decoder (the banded reader
/// decodes rows straight into tiles), so no `image::Limits` cap applies:
/// `pixel_count` bounds one plane (`MAX_PIXELS`, 800 MB) and
/// [`MAX_DECODED_BYTES`], the typed format's own cap, bounds their sum.
pub(crate) fn plane_capacity(dimensions: (u32, u32), total: &mut u64) -> Result<(), String> {
    let bytes = pixel_count(dimensions.0, dimensions.1)? as u64 * 8;
    *total = total
        .checked_add(bytes)
        .ok_or("high-depth native pixel sum overflow")?;
    if *total > MAX_DECODED_BYTES {
        return Err("high-depth native serialized pixels exceed the shared byte limit; remove layers or reduce dimensions".into());
    }
    Ok(())
}

impl PrecisionDocument {
    pub fn native_plan(&self) -> Result<PrecisionNativePlan, String> {
        self.native_preflight()
    }

    pub fn native_scratch_bytes(&self) -> Result<u64, String> {
        Ok(self.native_preflight()?.write_scratch_bytes)
    }

    /// The in-memory twin of [`Self::save_native`] (tests and examples): the
    /// same admission and the same bytes, written to a `Vec` reserved at
    /// `encoded_bound`. The `Vec` is the caller's, so it is not charged.
    pub fn encode_native(
        &self,
        limit: u64,
        other_retained: u64,
        control: Option<&DrJobControl>,
    ) -> Result<Vec<u8>, String> {
        let _run = control.map(DrJobControl::begin).transpose()?;
        let job = control.map_or(JobContext::NONE, JobContext::new);
        let plan = self.admit_native_write(limit, other_retained, job)?;
        let mut out = Vec::new();
        out.try_reserve_exact(plan.encoded_bound as usize)
            .map_err(|_| "could not allocate high-depth native output")?;
        self.write_native(&plan, &mut out, job)?;
        job.report(1.0, 5)?;
        Ok(out)
    }

    /// Streams the file into a temporary beside `path` and renames it into
    /// place (`save_atomically`); the admission runs before the temporary
    /// exists. Cancellation is checked while the bytes are written; the flush and
    /// rename that follow are not cancellable, so a cancel arriving during them
    /// still publishes the file.
    pub fn save_native(
        &self,
        path: &str,
        limit: u64,
        other_retained: u64,
        control: Option<&DrJobControl>,
    ) -> Result<(), String> {
        let _run = control.map(DrJobControl::begin).transpose()?;
        let job = control.map_or(JobContext::NONE, JobContext::new);
        let write = job.range(0.0, 0.9);
        let plan = self.admit_native_write(limit, other_retained, write)?;
        save_atomically(path, |temporary| {
            let mut file = FileSink::create(temporary, path)?;
            self.write_native(&plan, &mut file, write)?;
            file.finish()?;
            job.check()
        })?;
        // Publication has completed: a cancellation racing after rename cannot
        // turn a successful save into a claim that the destination is unchanged.
        if let Some(control) = control {
            control.report(1.0, 5);
        }
        Ok(())
    }

    /// The in-memory twin of [`Self::open_native`] (tests and examples): the
    /// same parse and the same admission over a caller's buffer, which is not
    /// charged.
    pub fn parse_native(
        bytes: &[u8],
        limit: u64,
        other_retained: u64,
        control: Option<&DrJobControl>,
    ) -> Result<Self, String> {
        let _run = control.map(DrJobControl::begin).transpose()?;
        let job = control.map_or(JobContext::NONE, JobContext::new);
        if bytes.len() as u64 > MAX_NATIVE_BYTES {
            return Err("high-depth native file exceeds the encoded size limit".into());
        }
        let out = read::parse_within(Reader::new(bytes), limit, other_retained, job, None)?;
        job.report(1.0, 5)?;
        Ok(out)
    }

    /// Reads the file record by record (`native_stream`): nothing holds the
    /// whole file, and a file that changes underneath is an `Err`.
    pub fn open_native(
        path: &str,
        limit: u64,
        other_retained: u64,
        control: Option<&DrJobControl>,
    ) -> Result<Self, String> {
        open_streamed(path, limit, other_retained, control, None)
    }
}

/// [`PrecisionDocument::open_native`] with `after_preflight` run once the
/// reader's final admission is reserved (while the job's run is alive, so
/// `control.scratch_bytes()` inside it is that reservation) and before any
/// adjustment is prepared or any pixel decodes. A black-box seam: the
/// streamed reader's measured admission and its changing-file cases cannot
/// otherwise be reached.
#[doc(hidden)]
pub fn open_native_observed(
    path: &str,
    limit: u64,
    other_retained: u64,
    control: Option<&DrJobControl>,
    after_preflight: &mut dyn FnMut(),
) -> Result<PrecisionDocument, String> {
    open_streamed(path, limit, other_retained, control, Some(after_preflight))
}

fn open_streamed(
    path: &str,
    limit: u64,
    other_retained: u64,
    control: Option<&DrJobControl>,
    after_preflight: Option<&mut dyn FnMut()>,
) -> Result<PrecisionDocument, String> {
    let _run = control.map(DrJobControl::begin).transpose()?;
    let job = control.map_or(JobContext::NONE, JobContext::new);
    job.check()?;
    let file = NativeFile::open(path, limit, other_retained)?;
    job.reserve(NATIVE_STREAM_WINDOW_BYTES + BOOKKEEPING_BYTES)?;
    let out = read::parse_within(
        Reader::over_file(&file),
        limit,
        other_retained,
        job,
        after_preflight,
    )?;
    job.report(1.0, 5)?;
    Ok(out)
}
