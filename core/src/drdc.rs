//! The DRDC native document format: encoder, atomic writer and
//! bounds-checked reader, plus the hard caps both sides enforce. The doc
//! comment on [`DrDocument::encode_native`] IS the format spec.

use image::codecs::png::PngEncoder;
use image::{ExtendedColorType, ImageEncoder};

use crate::blend::BlendMode;
use crate::doc::{sane_opacity, DrDocument, Layer, LayerKind, MAX_PIXELS};
use crate::doc_channel::MAX_CHANNELS;
use crate::doc_group::validate_structure;
use crate::doc_guide::{sane_origin, sane_position, GuideOrientation, MAX_GUIDES};
use crate::doc_lock::LOCK_ALL;
use crate::dr_image::save_atomically;
use crate::metadata::Resolution;

#[path = "drdc_capacity.rs"]
mod capacity;
#[doc(hidden)]
pub use capacity::warp_decoded_capacity;

#[path = "drdc_read.rs"]
mod read;
pub(crate) use crate::native_stream::Reader;
pub(crate) use read::parse_native;

/// Hard caps applied while reading DRDC files so corrupt headers cannot ask
/// for absurd allocations. The meta cap is shared by BOTH per-layer string
/// slots (meta and the layer style) and is also what the FFI meta and style
/// setters enforce, so a document can never carry a string the writer would
/// refuse. Channel names share the layer-name slot rule (same cap, same
/// truncation, same `put_name` writer).
pub(crate) const MAX_DRDC_LAYERS: u32 = 1024;
/// Independent masks have a separate serialized-plane ceiling, retaining the
/// previous active-group plus raster budgets. Host working-memory admission is
/// separate; versions1–9 retain their original read contract (disabled groups
/// historically did not contribute to the active group-rendering budget).
pub(crate) const MAX_DRDC_TOTAL_MASK_PIXELS: u64 =
    MAX_DRDC_GROUP_CANVAS_PIXELS + MAX_DRDC_TOTAL_LAYER_PIXELS;
pub(crate) const MAX_DRDC_NAME_LEN: u32 = 64 * 1024;
pub(crate) const MAX_DRDC_PNG_LEN: u32 = 1024 * 1024 * 1024;
pub(crate) const MAX_DRDC_META_LEN: u32 = 16 * 1024 * 1024;

/// Cap on EACH of the four document blobs the version-6 tail carries — the
/// ICC profile and the EXIF, XMP and IPTC packets — applied independently,
/// so a crafted file tops out at 64 MiB of blob next to the 1.6 GB of layer
/// pixels [`MAX_DRDC_TOTAL_LAYER_PIXELS`] already admits. There is no total
/// budget and none is needed: unlike channels, blobs are not multiplied by
/// the canvas size.
///
/// 16 MiB is deliberately just ABOVE what any JPEG can carry: an ICC profile
/// travels in at most 255 APP2 chunks of 65533 - 14 payload bytes
/// (16 707 345 bytes = 15.93 MiB, the same arithmetic `image`'s JPEG encoder
/// does). So no blob that arrived in a JPEG can ever be refused on the way
/// out, while a hand-built profile (a CMYK device link is the large real
/// case, a few MB) is far inside it. The FFI setters enforce it too, so a
/// document can never hold a blob the writer would refuse.
///
/// Separate from [`MAX_DRDC_META_LEN`] on purpose: that one is a *string*
/// cap the FFI meta and style setters also enforce, and raising one must not
/// silently raise the other.
pub(crate) const MAX_DRDC_BLOB_LEN: u32 = 16 * 1024 * 1024;

/// The DRDC revision this build writes. Version 1 files (no mask, no layer
/// meta), version 2 files (no clipped flag), version 3 files (no layer style,
/// no global light), version 4 files (no channels), version 5 files (no
/// colour profile, no metadata packets, no resolution), version 6 files
/// (no group structure, no locks, no links) and version 7 files (no guides
/// and no ruler origin) still load; anything newer is refused. 67 is the
/// typed float-master word (`float_native`); 68 the typed float
/// smart-object contents word; 69 and 70 the byte and typed Stack Mode
/// words; 71 the Color Lookup abstract-look word at both depths
/// (`look_native`); 72 the byte float-master word (`float_native`); 73 the
/// byte float smart-object contents word; 74 and 75 the byte and typed path
/// words (`path_native`); 78 the typed float RAW master word
/// (`float_native`; 76 and 77 are other batches' words); 79 the
/// RAW-locals word at both depths (`raw_locals_native`). Like 67 and
/// 75, 78 is a typed word only: no byte body version above 23 exists, so the
/// body-version check in `drdc_read` still refuses 76 to 78 by name.
/// 82 is the typed CMYK document word (`cmyk_native`, #547; 80 and 81 are
/// other batches' words), typed only like 78: the first word inside 46,
/// storage tag 3 and the `CMYK` table. 83 is the same word for a CMYK
/// document holding a layer mask or a CMYK adjustment record (#547
/// phase 2), typed only; 84 the same word for one holding a group, a
/// clipping mask, a blend mode or a Fill Opacity style (#547 phase 4a),
/// typed only; 85 the same word for one holding an alpha channel or a vector
/// mask (#547 channels batch), 86 for one holding a layer effect, knockout
/// or Blend Interior (#547 styles batch) and 87 for one holding a type, shape
/// or fill description (#547 artwork batch), each admitting the words below
/// it, typed only; 88 the same word for one holding a stacked layer effect,
/// typed only. 89 is the stacked-effects word at both depths
/// (`style_instances`): the FIRST word inside 45, and in a typed file
/// the word after the CMYK slot inside 46 (non-CMYK only). 90 is the
/// pattern-overlay word at both depths (`style_pattern`): the
/// FIRST word inside 45 (ahead of 89), and in a typed file the word after the
/// CMYK slot (ahead of 89). 91 is the layer reference-point word at both
/// depths (`reference_native`, Photoshop's `fxrp`): the FIRST word
/// inside 45 (ahead of 90) with its `LREF` table before SPOT and LIDS, and
/// in a typed file the word right after the CMYK slot (ahead of 90). 92 is
/// the Generative Layer variations word at both depths (`generated_native`):
/// the FIRST word inside 45 (ahead of 91) with its `GVAR` table after
/// PATH and before `LREF`, and in a typed file the word right after the CMYK
/// slot (ahead of 91), a CMYK file's too. 93 is
/// the pattern-overlay ANGLE word at both depths (`pattern_angle_native`):
/// the FIRST word inside 45 (ahead of 92), and in a typed file
/// the word right after the CMYK slot (ahead of 92); it implies 90. 94 is the
/// gradient-options word at both depths (`gradient_options_native`: a
/// Shape Burst stroke or a dithered gradient): the FIRST word
/// inside 45 (ahead of 93), and in a typed file the word right after the
/// CMYK slot (ahead of 93). 95 is the pattern-stroke word at both depths
/// (`stroke_pattern_native`: a stroke carrying a pattern
/// fill): the FIRST word inside 45 (ahead of 94), and in a typed file the
/// word right after the CMYK slot (ahead of 94). 96 is the contour word at
/// both depths (`contour_native`: a layer style contour,
/// Anti-aliased or Range): the FIRST word inside 45 (ahead of 95), and in a
/// typed file the word right after the CMYK slot (ahead of 95). 97 is the
/// noise word at both depths (`noise_native`: a layer
/// style Noise above 0): the FIRST word inside 45 (ahead of 96), and in a
/// typed file the word right after the CMYK slot (ahead of 96). 98 is the
/// texture word at both depths (`texture_native`: a Bevel &
/// Emboss carrying a texture): the FIRST word inside 45 (ahead of 97), and
/// in a typed file the word right after the CMYK slot (ahead of 97). 99 is
/// the glow-gradient word at both depths (`glow_gradient_native`: an
/// Outer or Inner Glow carrying a gradient or Jitter): the next word inside
/// 45 (ahead of 98), and in a typed file the word right after the CMYK slot
/// (ahead of 98). 100 is the wide-colour word at both depths
/// (`wide_color_native`: a style colour slot carrying a wide colour):
/// the next word inside 45 (ahead of 99), and in a typed file the word right
/// after the CMYK slot (ahead of 99). 101 is the multichannel Blend If word
/// at both depths (`blend_if_channels_native`: a Blend If
/// with more than one range; a typed file's `LBIF` schema 2): the FIRST word
/// inside 45 (ahead of 100), and in a typed file the word right after the
/// CMYK slot (ahead of 100). 102 is the large-description word at both
/// depths (`large_description_native`: an artwork description over 64
/// KiB, admitted up to the 16 MiB meta cap): the FIRST word inside 45 (ahead
/// of 101), and in a typed file the word right after the CMYK slot (ahead of
/// 101). 103 is the gradient-smoothness word at both depths
/// (`gradient_smoothness_native`: a stroke or gradient overlay
/// gradient, or a Gradient Fill description, whose Smoothness is below 100
/// %): inside 45 ahead of 102, and in a typed file after the CMYK slot ahead
/// of 102. 104 is the gradient-midpoint word at both depths
/// (`gradient_midpoint_native`: a stroke or gradient overlay gradient,
/// or a Gradient Fill description, with a stop midpoint other than 50 %):
/// inside 45 ahead of 103, and in a typed file after the CMYK slot ahead of
/// 103. 105 is the gradient-fill alignment word at both depths
/// (`fill_align_native`: a Gradient Fill description with
/// `align_with_layer` true): the FIRST word inside 45 (ahead of 104), and in
/// a typed file the word right after the CMYK slot (ahead of 104). 106 is
/// the Grayscale document word (`gray_native`, #547): typed files only, in
/// the CMYK slot itself (never beside a CMYK word), its `GRAY` table where
/// the CMYK table goes. 107 is the Lab Color document word (`lab_native`,
/// #547): typed files only, in the CMYK slot itself (never beside a CMYK or
/// Gray word), its `LAB ` table where the CMYK table goes. 108 is the
/// Indexed Color document word (`indexed_native`, #547): typed files only,
/// in the CMYK slot itself (never beside a CMYK, Gray or Lab word), its
/// `INDX` table where the CMYK table goes. 109 is the Duotone document word
/// (`duotone_native`, #547): typed files only, in the CMYK slot in place of
/// the Gray word 106, its `DUOT` table right after the `GRAY` table. 110 is
/// the Bitmap document word (`bitmap_native`, #547): typed files only, in the
/// CMYK slot itself in place of the Gray word 106 its Gray storage would
/// write, its `GRAY` table then its `BTMP` table where the CMYK table goes.
/// 111 is the Gradient Map method word at both depths
/// (`gradient_map_method_native`: a `gradient_map` adjustment whose
/// params hold `method`): the FIRST word inside 45 (ahead of 105), and in a
/// typed file the word right after the CMYK slot (ahead of 105).
/// 112 is the spot channel word at both depths (`spot_channel_native`, #547:
/// some channel is a spot channel): the FIRST word inside 45 (ahead of 111)
/// with its `SPOT` table after `LREF`, and in a typed file the word right
/// after the CMYK slot (ahead of 111) with `SPOT` after `LREF`.
/// The next free word is 113. The constant stays 89: words 90 to 112 are
/// conditional words read ahead of the body version, never a body version
/// themselves.
/// Since 2026-10-10 words 65, 66, 68, 69, 70 and 73
/// and their tables are read and skipped, never written
/// (`native_sources`' module doc).
pub(crate) const DRDC_VERSION: u32 = 89;
/// Byte documents keep their established version-10 layout and evaluation.
/// Version 11 is the separately tagged preserving u16 document type.
const LEGACY_DRDC_VERSION: u32 = 10;

/// Parse-time ceiling on the canvas area a file's ISOLATED groups could ask
/// the compositor for: the canvas pixel count summed over every Group record
/// whose STORED properties already force isolation. It is a cheap, friendly
/// refusal for an absurd file — checked before a single layer PNG is decoded —
/// and deliberately NOT the guarantee, which is `doc_group`'s runtime
/// `MAX_GROUP_BUFFER_PIXELS`: that one is charged before every allocation and
/// released when the buffer is dropped, so it bounds what a NEST holds at
/// once whatever the file says, and it is the only check that can be right,
/// because a group's real extent and the `has_visible_clipped_members`
/// isolation are not knowable from stored properties alone.
///
/// SIXTY-FOUR canvases, not the runtime budget's four, and the two numbers do
/// not compare because they do not measure the same thing. This one is a SUM
/// over every isolated group in the file — the only quantity a parse can
/// compute, since a group's real extent is not knowable from stored
/// properties — while the runtime budget is a PEAK over the buffers alive at
/// once, which sibling groups never add to (each is dropped before the next
/// is built). Sixty-four sibling groups therefore cost the compositor ONE
/// canvas, not sixty-four, and this check refuses only the shape whose
/// sheer count says the file is not a document. It over-counts a real one
/// badly if made tight: at four canvases a perfectly ordinary 24 MP composite
/// would be refused at SIXTEEN isolated groups, and refusing to open (or
/// save) a real document is far worse than rendering a crafted one slowly.
/// Sixty-four puts the line at 266 isolated groups on that canvas and 64 on
/// the largest canvas this build allows, which no hand-made document reaches
/// and which still catches the "one tiny file, tens of gigabytes" shape
/// early.
const MAX_DRDC_GROUP_CANVAS_PIXELS: u64 = 64 * MAX_PIXELS;

/// Ceiling on the SUM of decoded layer pixels across one DRDC file: even when
/// every individual layer looks reasonable, a crafted file must not be able
/// to stack layers until memory is exhausted.
pub(crate) const MAX_DRDC_TOTAL_LAYER_PIXELS: u64 = 4 * MAX_PIXELS;

/// The byte (8-bit) format's cap on decoded bytes, 1.6 GB: its layer pixel
/// ceiling at four bytes each. The byte reader's and writer's embedded-source
/// and WARP capacity checks read it. The typed format has its own, larger
/// cap (`precision_native::MAX_DECODED_BYTES`), so raising that one
/// leaves every byte file's answer as it was.
pub(crate) const MAX_DRDC_DECODED_BYTES: u64 = MAX_DRDC_TOTAL_LAYER_PIXELS * 4;

/// The same ceiling for the channel list. The COUNT cap
/// (`doc_channel::MAX_CHANNELS`) alone is not enough: 256 channels on a
/// 24 MP canvas is 6.4 GB of coverage, so the reader also checks the total —
/// and, unlike layers, it can do so UP FRONT, because a channel's size is
/// always the canvas's and the count is in the header.
///
/// NINE full canvases, deliberately: `add_luminosity_masks` appends exactly
/// nine canvas-sized planes, so any smaller multiple makes the feature refuse
/// itself on a large canvas. At four it did — a 45 MP camera file (8192 x
/// 5464, the size the feature exists for) allowed only eight channels, so the
/// nine masks were refused on a document with no channels to delete. At nine
/// the whole set fits the largest canvas any op will build (`MAX_PIXELS`),
/// which is the invariant worth having: an op that creates channels is never
/// refused for a reason the user cannot act on. A crafted file is still
/// bounded — 900 MB of coverage, well under the 1.6 GB of layer pixels
/// `MAX_DRDC_TOTAL_LAYER_PIXELS` already admits.
///
/// `doc_channel::channels_fit` enforces the same number at CREATION time, so
/// for a document this build made the writer's check can never fire; it is
/// there so the writer provably enforces every cap the reader does.
pub(crate) const MAX_DRDC_TOTAL_CHANNEL_PIXELS: u64 = 9 * MAX_PIXELS;

impl DrDocument {
    /// Version 9 preserves version 8's byte layout and adds versioned colour
    /// evaluation in adjustment metadata (`params.evaluation = "document_v2"`).
    /// Older readers must refuse version 9 rather than ignore this semantic key
    /// and silently render a calibrated layer with legacy sRGB arithmetic.
    ///
    /// Serializes to the DRDC layout (see the header comment): "DRDC"
    /// (written; legacy "RZDC" read, `native_magic`), u32 version, u32 width, u32 height, u32 layer count; per layer
    /// bottom-to-top: u32 name len + UTF-8 name, i32 off x, i32 off y,
    /// f32 opacity, u32 blend, u8 visible, u32 PNG len + PNG pixels.
    /// All integers little-endian.
    ///
    /// Version 2 appends three fields to each layer record, after the pixel
    /// PNG (so a version-1 record is a strict prefix of a version-2 one):
    /// u8 mask present, u8 mask enabled, u32 mask len + that many RAW
    /// coverage bytes when present (the mask's dimensions are the layer's
    /// pixel dimensions and are not stored twice), then u8 meta present and,
    /// when present, u32 meta len + UTF-8 meta bytes.
    ///
    /// Version 3 appends one more field after all the version-2 fields (so a
    /// version-2 record is in turn a strict prefix of a version-3 one):
    /// u8 clipped.
    ///
    /// Version 4 adds two document fields right after the layer count — f32
    /// global-light angle, f32 global-light altitude (degrees) — and one more
    /// per-layer field after the version-3 clipped byte (a version-3 record
    /// is a strict prefix of a version-4 one): u8 style present and, when
    /// present, u32 style len + UTF-8 canonical style JSON (see `style`),
    /// encoded and capped exactly like meta.
    ///
    /// Version 5 appends the ALPHA CHANNEL list after the LAST LAYER RECORD,
    /// so a version-4 file is a strict prefix of a version-5 one: u32 channel
    /// count (at most `doc_channel::MAX_CHANNELS`), then per channel u32 name
    /// byte length + UTF-8 name (truncated exactly like a layer name), u8
    /// overlay red, u8 green, u8 blue, f32 overlay opacity, u8 "color
    /// indicates selected" (0 — the default — means the wash covers the
    /// MASKED areas, matching Quick Mask; any non-zero value reads as 1), and
    /// u32 PNG byte length + a PNG-encoded 8-bit GRAYSCALE (L8) plane of
    /// exactly the canvas size (a channel is always canvas-sized, so its
    /// dimensions are not stored twice). The plane is PNG-compressed rather
    /// than raw like a layer mask because a channel is always the WHOLE
    /// canvas: nine luminosity masks on a 24 MP canvas would be 216 MB raw,
    /// and coverage planes are exactly the flat, large-run data PNG's filters
    /// collapse. `Channel::id` is NOT written: it is a per-process handle a
    /// host hangs view state on, minted fresh whenever a channel is created,
    /// this reader included.
    ///
    /// Version 6 appends the DOCUMENT TAIL after the channel list, so a
    /// version-5 file is a strict prefix of a version-6 one: f32 horizontal
    /// resolution and f32 vertical resolution in pixels per inch (sanitized
    /// by [`Resolution::sane`] on both sides — non-finite or non-positive
    /// takes the 72 ppi default, finite values clamp to [1, 30000] and
    /// quantize to four decimals, exactly like the global light), then FOUR
    /// optional blobs in this order — the ICC colour profile, the EXIF
    /// packet, the XMP packet and the IPTC packet — each written as u8
    /// present and, when present, u32 byte length + that many RAW bytes,
    /// capped at [`MAX_DRDC_BLOB_LEN`] each. The blobs are stored VERBATIM
    /// and never interpreted here: they are what the file they came from
    /// carried, and the export path splices them back. Unlike meta and the
    /// style they are NOT UTF-8 (an ICC profile and an EXIF packet are
    /// binary; XMP merely happens to be XML), so they take the byte helpers
    /// rather than the string ones.
    ///
    /// The ICC slot is written ABSENT when the document's profile is the
    /// built-in sRGB, and an absent slot reads back as that same profile —
    /// so a plain document does not grow by 2.5 KB, and a blob-less
    /// version-6 file is a version-5 file plus exactly 12 bytes.
    ///
    /// Version 7 appends TWELVE bytes to each layer record, immediately after
    /// the version-4 style slot, so a version-6 RECORD is a strict prefix of a
    /// version-7 one: u32 lock flags (bits 0..2 meaningful — transparency,
    /// pixels, position; the reader masks 3..31 off), u32 link-group id (0 =
    /// unlinked), u16 nesting depth (0 = top level), u8 kind (0 = raster,
    /// 1 = group; any other value is REFUSED) and u8 group-open, the panel
    /// disclosure flag, which is meaningless on a raster entry. WHOLE-FILE
    /// prefixing does NOT hold for version 7: the bytes go inside each record,
    /// so a version-6 file is not a byte-prefix of the version-7 file of the
    /// same document — exactly as version 4 already broke it, and for the same
    /// reason.
    ///
    /// Version 8 appends the GUIDE BLOCK at the very end of the file, after
    /// the version-6 tail's four blobs: f64 ruler-origin x, f64 ruler-origin
    /// y (the canvas coordinate of the ruler's zero point), u32 guide count
    /// (at most `doc_guide::MAX_GUIDES`; more is an ERROR at both ends),
    /// then per guide, in the model's sorted order, u8 orientation (0 =
    /// horizontal, a line of constant y; 1 = vertical, a line of constant x;
    /// ANY OTHER VALUE IS REFUSED, because an orientation is a structural
    /// claim like the kind byte, not a cosmetic one) and f64 position, the
    /// canvas coordinate of the line. Nine bytes per guide; 1024 of them is
    /// 9 KB beside the 1.6 GB of layer pixels this format already admits.
    ///
    /// Because the block is a TAIL, the size property holds again: a
    /// GUIDE-LESS version-8 file is a version-7 file with the version word
    /// bumped plus exactly 20 bytes appended, everything after the version
    /// word unchanged. (It is a SIZE property and not a literal byte prefix —
    /// the version `u32` sits at byte offset 4, so the two files necessarily
    /// differ there. Versions 4 and 7 broke even this, because their bytes go
    /// inside each layer record.)
    ///
    /// f64 rather than the f32 this format uses for every DISPLAY scalar
    /// (opacity, ppi, the light angle): a guide position is a geometric
    /// quantity that must survive an Image Size down and back, `resize`
    /// already computes with f64 factors, and the FFI already carries
    /// geometry as `double`. Both the origin and each position are written
    /// through `doc_guide`'s own sanitizers — clamped into the canvas and
    /// quantized to four decimals, a guide with a non-finite position simply
    /// not written — so a file this build produces reads back to exactly the
    /// model that wrote it. There is deliberately NO per-guide lock byte and
    /// NO ruler-unit byte: Lock Guides and the ruler's unit are app-wide
    /// preferences, not properties of the picture (see `doc_guide`).
    /// `Guide::id` is not written either, for `Channel::id`'s reason.
    ///
    /// A GROUP entry's pixel PNG is a 1x1 fully transparent PNG (~70 bytes),
    /// so the record layout stays uniform and the writer needs no conditional
    /// shape; a group has no pixels of its own and nothing ever reads them.
    /// Its `off x` / `off y` are not a pixel position either but where that
    /// canvas-sized mask SITS: a group's mask travels with the group as
    /// bookkeeping on this offset rather than as a resample per move
    /// (`doc_align::translate_entry`), and persisting it is what makes a save
    /// round trip lossless for a group that has been dragged. It is (0, 0) for
    /// a group with no mask and for every version-1 to -6 file, which has no
    /// groups at all. Its MASK is CANVAS-sized rather than layer-sized
    /// (`doc_group`), which is the one place the record's shape depends on a
    /// field written LATER than the mask:
    ///
    /// * the writer validates dimensions with [`expected_mask_dims`] — the
    ///   layer's for a raster entry, the canvas's for a group — before PNG
    ///   encoding. A mismatched mask refuses the save rather than disappearing;
    /// * the reader DEFERS building the mask image until the kind byte has
    ///   been read. It accepts a stored `mask_len` equal to EITHER the declared
    ///   pixel count OR the canvas's, borrows the bytes, and after the
    ///   version-7 fields checks the size the kind requires. Pixel and mask
    ///   allocation waits until the whole file passes, refusing a wrong pairing (a raster mask sized to the
    ///   canvas, or a group mask sized to the layer). For every version-1 to
    ///   -6 file the kind is always Raster, so the widened acceptance can only
    ///   defer a refusal that the later check then makes identically — no
    ///   older file changes outcome.
    ///
    /// The whole depth sequence is checked for well-formedness at both ends
    /// (`doc_group::validate_structure`): the writer refuses to produce a file
    /// it could not read back, and the reader refuses a crafted one rather
    /// than loading a stack whose entries would silently vanish from every
    /// projection.
    /// Version 10 makes mask dimensions independent: a present mask's u32
    /// length is followed by u32 width and height BEFORE its raw bytes. After
    /// the v7 fields, a present mask adds i32 canvas x/y, u8 linked (0 or 1),
    /// f32 density (0..1), f32 feather radius (0..1000). Dimensions, padded
    /// feather extent and aggregate raw masks are preflighted before decoding.
    /// Versions 1–9 infer dimensions by kind, placement from layer offset,
    /// linked=true, density=1, feather=0. Raw coverage is stored unmodified;
    /// derived feather caches are never serialized.
    ///
    /// Version 14 writes this version-10 layout followed by the `RSRC` embedded
    /// source asset/reference table specified in `native_sources.rs`. Source-free
    /// byte documents continue writing version 10. Versions 11–13 and 15 belong
    /// to the separately tagged high-depth reader. Assets retain original RGBA8
    /// samples and ICC profiles independently of rendered layer fallbacks; shared
    /// source references serialize the asset once. Only active guarded bindings
    /// are serialized, so destructive edits cannot resurrect obsolete masters.
    /// Version16 extends source assets with original-depth/revision tags and
    /// bindings with bounded linked path/SHA256 metadata (native_sources.rs).
    /// Cross-depth masters retain exact original PNG samples; linked masters
    /// stay embedded and readers never consult the recorded paths.
    /// Version 18 additionally stores original encoded RAW payloads, captured
    /// effective development recipes and decoder identities. Saved RGBA16
    /// developed masters and byte fallbacks reopen without a RAW decoder. The
    /// versioned RAW framing and capacities are specified in native_sources.rs.
    /// Version21 uses the same source-aware byte layout with the shared ATCH /
    /// RBIN resource pool specified there; typed22 additionally carries ADSC.
    /// RAW-local RZLM coverage uses this existing generic resource framing,
    /// with strict known-version validation and opaque future-version retention.
    /// Version28 conditionally wraps a prior byte body and appends the strict
    /// MOUT table specified in mask_outside_native.rs. All-zero outside coverage
    /// continues to write exactly the prior version and bytes.
    /// Version38 is the next-to-outermost wrapper (evaluation_native.rs): a
    /// `linear_light` document writes 38 before every prior version word —
    /// including 28 — and appends `EVAL`, u32 schema=1, u32 policy=1 after
    /// every prior tail, MOUT included. An ENCODED document writes neither the
    /// word nor the table, so its bytes stay exactly the prior writer's.
    /// Version41 is the OUTERMOST wrapper (label_native.rs): a document with
    /// any COLOUR LABEL writes 41 before every prior version word — 38
    /// included — and appends `LLBL`, u32 schema=1, u32 count, then count
    /// pairs of u32 bottom-first layer index (ascending) and u32 label code
    /// (1..=7) after every prior tail, EVAL included. A document with no
    /// label writes neither the word nor the table, so its bytes stay exactly
    /// the prior writer's.
    /// Version43 is the OUTERMOST wrapper (comps_native.rs): a document with
    /// any LAYER COMP writes 43 before every prior version word — 41
    /// included — and appends `LCMP`, u32 schema=1, u32 length, then that
    /// many bytes of UTF-8 JSON after every prior tail, LLBL included. A
    /// document with no comp writes neither the word nor the table, so its
    /// bytes stay exactly the prior writer's.
    /// Version45 is the OUTERMOST wrapper (identity_native.rs) and the one
    /// that is ALWAYS written: 45 goes before every prior version word — 43
    /// included — and `LIDS`, u32 schema=1, u32 count, then count u64 LE
    /// layer ids in bottom-first index order close the file after every prior
    /// tail, LCMP included. An id is a property of every layer, so the table
    /// is unconditional; a file with no 45 word (every file an earlier build
    /// wrote) reads exactly as before and MINTS fresh ids, and the reader
    /// ADOPTS the ids it reads so reopen → resave is byte-identical.
    /// Version 49 is the ONE word that sits INSIDE 45 (`style_knockout`, D10):
    /// a document some layer's style gives a non-default `knockout` or
    /// `blend_interior_as_group` writes 49 after 45 and before every prior
    /// version word, 43 included, and appends NO table — both keys already
    /// ride inside that record's style JSON. A document with neither writes
    /// neither the word nor anything else, so its bytes are exactly the prior
    /// writer's; clearing the last key restores them. The word exists so an
    /// older build refuses the file by name instead of reading the style
    /// leniently, dropping both keys and clearing what then looks like an
    /// identity style.
    /// Version 56 is the fifth word inside 45, after 63, 61 and 59 (`source_refusal_native`):
    /// a document some retained source asset of which carries a profile
    /// refusal writes 56 after 45, 63, 61 and 59 and before every other conditional word,
    /// and its `RSPR` table — u32 schema=1, u32 count, then per entry a u32
    /// ascending RSRC asset index, a u32 length and that many UTF-8 bytes
    /// (1…4096, no NUL) — is the last one before LIDS, after WARP. A document
    /// with no refusal writes neither, so its bytes stay exactly the prior
    /// writer's.
    /// Version 59 is the fourth word inside 45, after 63 and 61 (`sampler_native`),
    /// ahead of 56: a document holding any COLOUR SAMPLER writes 59 after 45 (and 63, 61)
    /// and before every other conditional word, and its `CSMP` table — u32
    /// schema=1, u32 count (1…10), then per sampler u32 x, u32 y and u8
    /// sample size — is the last one before LIDS, after RSPR. A document
    /// with no sampler writes neither, so its bytes stay exactly the prior
    /// writer's.
    /// Version 61 is the third word inside 45, after 63 (`levels_native`),
    /// ahead of 59: a document some layer of which holds a `levels` meta
    /// whose params carry an output range or a per-channel stage
    /// (`output_black`, `output_white`, `r`, `g` or `b`) writes 61 after 45
    /// (and 63) and before every other conditional word, and appends NO table — the
    /// keys ride inside that record's meta. A document with no such meta
    /// writes nothing here, so its bytes stay exactly the prior writer's.
    /// Version 63 is the next word inside 45 (`curves_native`),
    /// ahead of 61: a document some layer of which holds a `curves` meta
    /// whose params carry a map (`rgb_map`, `r_map`, `g_map` or `b_map`)
    /// writes 63 after 45 and before every other conditional word, and
    /// appends NO table — the maps ride inside that record's meta. A
    /// document with no such meta writes nothing here, so its bytes stay
    /// exactly the prior writer's.
    /// Version 72 is the next word inside 45 after 74 (`float_native`),
    /// ahead of 71: a document some written source asset of which is a
    /// 32-bit float master writes 72 after 45 and before every other
    /// conditional word, and appends NO table — the master rides in the RSRC
    /// table as depth byte 32 plus its raw f32 payload. A document with none
    /// writes nothing here, so its bytes stay exactly the prior writer's.
    /// Version 71 is the next word inside 45 (`look_native`), after 72
    /// and ahead of 63: a document some layer of which holds a `color_lookup` meta whose
    /// params keep an abstract profile (`abstract_profile`) writes 71 after
    /// 45 and before every other conditional word, and appends NO table —
    /// the profile rides inside that record's meta. A document with no such
    /// meta writes nothing here, so its bytes stay exactly the prior
    /// writer's.
    /// Version 112 is the FIRST word inside 45 (`spot_channel_native`, #547),
    /// ahead of 111: a document some channel of which is a spot channel
    /// writes 112 after 45 and before every other conditional word, and its
    /// `SPOT` table after LREF, before LIDS. The next free word is 113.
    /// Version 111 is the next word inside 45 (`gradient_map_method_native`),
    /// ahead of 105, with no table: a document some layer meta of
    /// which is a Gradient Map with a `method` writes 111. Version 105 is the next
    /// (`fill_align_native`), ahead of 104, with no table: a document
    /// some layer meta of which is a Gradient Fill description with
    /// `align_with_layer` true writes it. Version 104 is the
    /// next (`gradient_midpoint_native`), ahead of 103, with no table:
    /// a document some stored style, comp or Gradient Fill description of
    /// which holds a stop midpoint other than 50 % writes it. Version 103 is
    /// the next (`gradient_smoothness_native`), ahead of 102, with no
    /// table: a document some stored style, comp or Gradient Fill
    /// description of which holds a Smoothness below 100 % writes it. A
    /// document with none writes nothing here, so its bytes stay exactly the
    /// prior writer's.
    /// Version 102 is the next word inside 45 (`large_description_native`),
    /// ahead of 101, with no table: a document some layer meta of
    /// which is an artwork description over 64 KiB writes 102 after 45 and
    /// before every other conditional word — the description rides in the
    /// layer's meta as before. A document with none writes nothing here, so
    /// its bytes stay exactly the prior writer's.
    /// Version 101 is the next word inside 45 (`blend_if_channels_native`),
    /// ahead of 100, with no table: a document some stored
    /// style (a layer's or a comp's) of which holds a multichannel Blend If
    /// writes 101 after 45 and before every other conditional word — the
    /// extra ranges ride inside the style JSON. A document with none writes
    /// nothing here, so its bytes stay exactly the prior writer's.
    /// Version 100 is the next word inside 45 (`wide_color_native`),
    /// ahead of 99, with no table: a document some stored style (a layer's
    /// or a comp's) of which holds a colour slot carrying a wide colour
    /// writes 100 after 45 and before every other conditional word — the
    /// keys ride inside the style JSON. A document with none writes nothing
    /// here, so its bytes stay exactly the prior writer's.
    /// Version 99 is the next word inside 45 (`glow_gradient_native`),
    /// ahead of 98, with no table: a document some stored style (a
    /// layer's or a comp's) of which holds a glow carrying a gradient or
    /// Jitter writes 99 after 45 and before every other conditional word —
    /// the keys ride inside the style JSON. A document with none writes
    /// nothing here, so its bytes stay exactly the prior writer's.
    /// Version 98 is the next word inside 45 (`texture_native`),
    /// ahead of 97, with no table: a document some stored style (a
    /// layer's or a comp's) of which holds a bevel carrying a texture writes
    /// 98 after 45 and before every other conditional word — the keys ride
    /// inside the style JSON. A document with none writes nothing here, so
    /// its bytes stay exactly the prior writer's.
    /// Version 97 is the next word inside 45 (`noise_native`),
    /// ahead of 96, with no table: a document some stored style (a
    /// layer's or a comp's) of which holds a Noise above 0 writes 97 after 45
    /// and before every other conditional word — the key rides inside the
    /// style JSON. A document with none writes nothing here, so its bytes
    /// stay exactly the prior writer's.
    /// Version 96 is the next word inside 45 (`contour_native`),
    /// ahead of 95, with no table: a document some stored style (a
    /// layer's or a comp's) of which holds a contour-family key writes 96
    /// after 45 and before every other conditional word — the keys ride
    /// inside the style JSON. A document with none writes nothing here, so
    /// its bytes stay exactly the prior writer's.
    /// Version 95 is the next word inside 45 (`stroke_pattern_native`),
    /// ahead of 94, with no table: a document some stored style (a
    /// layer's or a comp's) of which holds a stroke carrying a pattern fill
    /// writes 95 after 45 and before every other conditional word — the fill
    /// rides inside the style JSON. A document with none writes nothing here,
    /// so its bytes stay exactly the prior writer's.
    /// Version 94 is the next word inside 45 (`gradient_options_native`),
    /// ahead of 93, with no table: a document some stored
    /// style (a layer's or a comp's) of which holds a Shape Burst stroke
    /// gradient or a dithered stroke or overlay gradient writes 94 after 45
    /// and before every other conditional word — the options ride inside the
    /// style JSON. A document with none writes nothing here, so its bytes
    /// stay exactly the prior writer's.
    /// Version 93 is the next word inside 45 (`pattern_angle_native`),
    /// ahead of 92, with no table: a document some stored style (a
    /// layer's or a comp's) of which holds a pattern overlay with a non-zero
    /// angle writes 93 after 45 and before every other conditional word — the
    /// angle rides inside the style JSON. A document with none writes nothing
    /// here, so its bytes stay exactly the prior writer's.
    /// Version 92 is the next word inside 45 (`generated_native`),
    /// ahead of 91: a document some layer of which holds live Generative
    /// Layer variations writes 92 after 45 and before every other
    /// conditional word, and its `GVAR` table — u32 schema=1, u32 entry
    /// count, then per entry the layer index, the raster size, the covered
    /// rect, the crop, the native size, u8 ink, the selected index and the
    /// pictures (size, tag, PNG) — follows PATH and precedes LREF.
    /// A document with none writes neither, so its bytes stay exactly the
    /// prior writer's.
    /// Version 91 is the next word inside 45 (`reference_native`),
    /// ahead of 90: a document some layer of which has MOVED since it was
    /// made (a reference point other than (0, 0)) writes 91 after 45 and
    /// before every other conditional word, and its `LREF` table — u32
    /// schema=1, u32 count, then per entry a u32 ascending bottom-first index
    /// and i32 x, i32 y — comes after PATH and GVAR, before SPOT and LIDS.
    /// A document whose layers never moved writes neither, so its bytes stay
    /// exactly the prior writer's. Version 90 (`style_pattern`) is the next word, ahead of
    /// 89, with no table.
    /// Version 89 is the next word inside 45 (`style_instances`),
    /// ahead of 79: a document some layer's stored style or some comp's
    /// recorded appearance of which is STACKED (two or more instances of one
    /// effect kind) writes 89 after 45 and before every other conditional
    /// word, and appends NO table — the instances ride inside each style's
    /// JSON. A document with none writes nothing here, so its bytes stay
    /// exactly the prior writer's.
    /// Version 79 is the next word inside 45 (`raw_locals_native`),
    /// ahead of 74: a document whose written RAW sources hold an attachment of
    /// more than four resources, or more than 512 MiB of RAW and attachment
    /// resources together, writes 79 after 45 and before every other
    /// conditional word, and appends NO table — the masks ride in the ATCH /
    /// RBIN pool. A document with neither writes nothing here, so its bytes
    /// stay exactly the prior writer's.
    /// Version 74 is the next word inside 45 (`path_native`), ahead of
    /// 72: a document holding any PATH or live VECTOR MASK writes 74 after
    /// 45 and before every other conditional word, and its `PATH` table —
    /// u32 schema=1, u32 length, that many bytes of UTF-8 JSON
    /// (`{"paths":[…],"vector_masks":[…]}`), then a u32 plane count and the
    /// raster-part planes (u32 width, u32 height, raw u8 coverage) — is the
    /// last one before LIDS, after CSMP. A vector-masked layer's own mask
    /// record is the baked plane. A document with neither writes neither,
    /// so its bytes stay exactly the prior writer's.
    ///
    /// The body buffer and every layer/channel PNG Vec reserve their
    /// `drdc_save_plan` bounds up front, so only capacity changes, never a byte.
    pub(crate) fn encode_native(&self) -> Result<Vec<u8>, String> {
        self.native_save_preflight()?;
        let plan = self.native_save_plan()?;
        let count = capacity::layer_count(self.layers.len())?;
        let sources = crate::native_sources::byte_sources(self)?;
        let projective = crate::source_transform_native::entries(&sources.bindings);
        let warp = crate::warp_native::encode_byte(self)?;
        let version = sources.version();
        let mut buf = crate::drdc_save_plan::reserved(plan.encoded_bound)?;
        buf.extend_from_slice(crate::native_magic::NATIVE_MAGIC);
        // The OUTERMOST wrapper (`identity_native`), and the one that is
        // ALWAYS written: 45 before every prior version word, 43 included,
        // and one LIDS table after every prior tail. Ids are a property of
        // every layer, so there is no "document without the table".
        let ids = crate::identity_native::byte_ids(self);
        buf.extend_from_slice(&crate::identity_native::BYTE_VERSION.to_le_bytes());
        // Then `spot_channel_native`'s word 112 — the FIRST word inside 45,
        // ahead of 111, written only when some channel is a spot channel;
        // its SPOT table follows LREF (#547).
        let spots = crate::spot_channel_native::byte_entries(&self.channels);
        if !spots.is_empty() {
            buf.extend_from_slice(&crate::spot_channel_native::WORD.to_le_bytes());
        }
        // Then `gradient_map_method_native`'s word 111 — the next word
        // inside 45, ahead of 105, with no table, written only when a layer
        // meta is a Gradient Map with a `method`.
        if crate::gradient_map_method_native::byte_carries(&self.layers) {
            buf.extend_from_slice(&crate::gradient_map_method_native::WORD.to_le_bytes());
        }
        // Then `fill_align_native`'s word 105 — the next word inside 45,
        // ahead of 104, with no table, written only when a layer meta is a
        // Gradient Fill description aligned with the layer.
        if crate::fill_align_native::descriptions_align(self) {
            buf.extend_from_slice(&crate::fill_align_native::WORD.to_le_bytes());
        }
        // Then `gradient_midpoint_native`'s word 104 — the next word inside
        // 45, ahead of 103, with no table, written only when a stored style,
        // a comp or a Gradient Fill description holds a stop midpoint other
        // than 50 %.
        if crate::gradient_midpoint_native::styles_midpoints(self) {
            buf.extend_from_slice(&crate::gradient_midpoint_native::WORD.to_le_bytes());
        }
        // Then `gradient_smoothness_native`'s word 103 — the next word inside
        // 45, ahead of 102, with no table, written only when a stored style,
        // a comp or a Gradient Fill description holds a Smoothness below
        // 100 %.
        if crate::gradient_smoothness_native::styles_smoothness(self) {
            buf.extend_from_slice(&crate::gradient_smoothness_native::WORD.to_le_bytes());
        }
        // Then `large_description_native`'s word 102 — the next word inside
        // 45, ahead of 101, with no table, written only when a layer meta is
        // an artwork description over 64 KiB.
        if crate::large_description_native::descriptions_large(self) {
            buf.extend_from_slice(&crate::large_description_native::WORD.to_le_bytes());
        }
        // Then `blend_if_channels_native`'s word 101 — the next word inside
        // 45, ahead of 100, with no table, written only when a stored style
        // holds a multichannel Blend If.
        if crate::blend_if_channels_native::styles_multichannel(self) {
            buf.extend_from_slice(&crate::blend_if_channels_native::WORD.to_le_bytes());
        }
        // Then `wide_color_native`'s word 100 — the next word inside 45,
        // ahead of 99, with no table, written only when a stored style holds
        // a colour slot carrying a wide colour.
        if crate::wide_color_native::styles_wide(self) {
            buf.extend_from_slice(&crate::wide_color_native::WORD.to_le_bytes());
        }
        // Then `glow_gradient_native`'s word 99 — the next word inside 45,
        // ahead of 98, with no table, written only when a stored style holds
        // a glow carrying a gradient or Jitter.
        if crate::glow_gradient_native::styles_glow_gradient(self) {
            buf.extend_from_slice(&crate::glow_gradient_native::WORD.to_le_bytes());
        }
        // Then `texture_native`'s word 98 — the next word inside 45, ahead
        // of 97, with no table, written only when a stored style holds a
        // bevel carrying a texture.
        if crate::texture_native::styles_texture(self) {
            buf.extend_from_slice(&crate::texture_native::WORD.to_le_bytes());
        }
        // Then `noise_native`'s word 97 — the next word inside 45, ahead of
        // 96, with no table, written only when a stored style holds a Noise
        // above 0.
        if crate::noise_native::styles_noise(self) {
            buf.extend_from_slice(&crate::noise_native::WORD.to_le_bytes());
        }
        // Then `contour_native`'s word 96 — the next word inside 45, ahead
        // of 95, with no table, written only when a stored style holds a
        // contour-family key.
        if crate::contour_native::styles_contour(self) {
            buf.extend_from_slice(&crate::contour_native::WORD.to_le_bytes());
        }
        // Then `stroke_pattern_native`'s word 95 — the next word inside 45,
        // ahead of 94, with no table, written only when a stored style holds a
        // stroke carrying a pattern fill.
        if crate::stroke_pattern_native::styles_stroke_pattern(self) {
            buf.extend_from_slice(&crate::stroke_pattern_native::WORD.to_le_bytes());
        }
        // Then `gradient_options_native`'s word 94 — the next word inside 45,
        // ahead of 93, with no table, written only when a stored style holds
        // a Shape Burst stroke gradient or a dithered gradient.
        if crate::gradient_options_native::styles_options(self) {
            buf.extend_from_slice(&crate::gradient_options_native::WORD.to_le_bytes());
        }
        // Then `pattern_angle_native`'s word 93 — the next word inside 45,
        // ahead of 92, with no table, written only when a stored style (a
        // layer's or a comp's) holds a pattern overlay with a non-zero angle.
        if crate::pattern_angle_native::styles_angle(self) {
            buf.extend_from_slice(&crate::pattern_angle_native::WORD.to_le_bytes());
        }
        // Then `generated_native`'s word 92 — the next word inside 45, ahead
        // of 91, written only when some layer holds live Generative Layer
        // variations. Its GVAR table follows PATH and precedes LREF.
        let variations = crate::generated_native::encode(&self.generated_entries())?;
        if !variations.is_empty() {
            buf.extend_from_slice(&crate::generated_native::WORD.to_le_bytes());
        }
        // Then `reference_native`'s word 91 — the next word inside 45, ahead
        // of 90, written only when some layer moved since it was made; its
        // LREF table comes before SPOT and LIDS.
        let references = crate::reference_native::byte_entries(self);
        if !references.is_empty() {
            buf.extend_from_slice(&crate::reference_native::BYTE_VERSION.to_le_bytes());
        }
        // Then `style_pattern`'s word 90 — the next word inside 45, ahead of
        // 89, with no table, written only when a stored style (a layer's or a
        // comp's) holds a pattern overlay.
        if crate::style_pattern::styles_pattern(self) {
            buf.extend_from_slice(&crate::style_pattern::WORD.to_le_bytes());
        }
        // Then `style_instances`' word 89 — the next word inside 45, with no
        // table of its own, written only when a stored style (a layer's or a
        // comp's) is stacked.
        if crate::style_instances::styles_stacked(self) {
            buf.extend_from_slice(&crate::style_instances::WORD.to_le_bytes());
        }
        // Then `raw_locals_native`'s word 79 — the next word inside 45, with
        // no table of its own, written only when the written RAW sources hold
        // more than four masks on one attachment or over 512 MiB of resources.
        if crate::raw_locals_native::carries(sources.assets.iter().map(|a| &**a)) {
            buf.extend_from_slice(&crate::raw_locals_native::VERSION.to_le_bytes());
        }
        // Then `path_native`'s word 74 — the next word inside 45, written
        // only when the document holds a path or a live vector mask. Its
        // PATH table is the last one before LIDS.
        let paths = crate::path_native::encode(&self.paths.entries, &self.vector_mask_table(), 1)?;
        if !paths.is_empty() {
            buf.extend_from_slice(&crate::path_native::BYTE_VERSION.to_le_bytes());
        }
        // Then `float_native`'s word 72 — the next word inside 45, with no
        // table of its own, written only when some written asset is a 32-bit
        // float master.
        if crate::float_native::masters_present(&sources.assets) {
            buf.extend_from_slice(&crate::float_native::BYTE_MASTER_VERSION.to_le_bytes());
        }
        // Then `look_native`'s word 71 — the next word inside 45, with no
        // table of its own, written only when some Color Lookup meta keeps an
        // abstract profile.
        if crate::look_native::byte_carries(&self.layers) {
            buf.extend_from_slice(&crate::look_native::VERSION.to_le_bytes());
        }
        // Then `curves_native`'s word 63 — the next word inside 45, with no
        // table of its own, written only when some curves meta carries a map.
        if crate::curves_native::byte_carries(&self.layers) {
            buf.extend_from_slice(&crate::curves_native::BYTE_VERSION.to_le_bytes());
        }
        // Then `levels_native`'s word 61 — the next word inside 45, with no
        // table of its own, written only when some levels meta carries an
        // output range or a per-channel stage.
        if crate::levels_native::byte_carries(&self.layers) {
            buf.extend_from_slice(&crate::levels_native::BYTE_VERSION.to_le_bytes());
        }
        // Then `sampler_native`'s word 59 — the next word inside 45,
        // written only when the document holds a colour sampler. Its CSMP
        // table is the last one before LIDS.
        let samplers = crate::sampler_native::encode(&self.color_samplers)?;
        if !samplers.is_empty() {
            buf.extend_from_slice(&crate::sampler_native::BYTE_VERSION.to_le_bytes());
        }
        // Then `source_refusal_native`'s word 56 — the next word inside 45,
        // written only when some asset carries a profile refusal. Its RSPR
        // table is the last one before LIDS.
        let refusals = crate::source_refusal_native::entries(&sources.assets);
        if !refusals.is_empty() {
            buf.extend_from_slice(&crate::source_refusal_native::BYTE_VERSION.to_le_bytes());
        }
        if !warp.is_empty() {
            buf.extend_from_slice(&crate::warp_native::BYTE_VERSION.to_le_bytes());
        }
        if !projective.is_empty() {
            buf.extend_from_slice(&crate::source_transform_native::BYTE_VERSION.to_le_bytes());
        }
        // Then `style_knockout`'s word 49 — the one word INSIDE 45, and the
        // one with no table of its own (D10). Written iff some stored style
        // carries a non-default knockout key; a document with neither key
        // writes nothing here and its bytes stay exactly the prior writer's.
        if crate::style_knockout::styles_carry_keys(&self.layers) {
            buf.extend_from_slice(&crate::style_knockout::BYTE_VERSION.to_le_bytes());
        }
        // Then `comps_native`: a document
        // holding any layer comp writes 43 before every prior version word
        // and one LCMP table after every prior tail. A comp-less document
        // writes neither, so its bytes are exactly the prior writer's. The
        // payload is built ONCE here and handed to the encoder.
        let comps = crate::comps_native::payload(&self.comps, |id| {
            Some(self.layers.get(self.layer_index_by_id(id)?)?.name.clone())
        })?;
        if !comps.is_empty() {
            buf.extend_from_slice(&crate::comps_native::BYTE_VERSION.to_le_bytes());
        }
        // Then `label_native`: a document
        // carrying any colour label writes 41 before every prior version word
        // and one LLBL table after every prior tail. An unlabelled document
        // writes neither, so its bytes are exactly the prior writer's.
        let labels = crate::label_native::byte_entries(self);
        if !labels.is_empty() {
            buf.extend_from_slice(&crate::label_native::BYTE_VERSION.to_le_bytes());
        }
        // Then `evaluation_native`: a `linear_light` document writes 38
        // before every prior version word and one EVAL table after every
        // prior tail. An ENCODED document — every document before Phase 11E —
        // writes neither, so its bytes are exactly the bytes the prior writer
        // made.
        let linear_light = self.evaluation() == crate::doc_evaluation::Evaluation::LinearLight;
        if linear_light {
            buf.extend_from_slice(&crate::evaluation_native::BYTE_VERSION.to_le_bytes());
        }
        let outside = crate::mask_outside_native::byte_indices(self);
        if !outside.is_empty() {
            buf.extend_from_slice(&crate::mask_outside_native::BYTE_VERSION.to_le_bytes());
        }
        buf.extend_from_slice(&version.to_le_bytes());
        buf.extend_from_slice(&self.width.to_le_bytes());
        buf.extend_from_slice(&self.height.to_le_bytes());
        buf.extend_from_slice(&count.to_le_bytes());
        // Version 4 document fields.
        let light = self.global_light.sane();
        buf.extend_from_slice(&light.angle.to_le_bytes());
        buf.extend_from_slice(&light.altitude.to_le_bytes());
        for layer in &self.layers {
            put_name(&mut buf, &layer.name);
            buf.extend_from_slice(&layer.offset.0.to_le_bytes());
            buf.extend_from_slice(&layer.offset.1.to_le_bytes());
            buf.extend_from_slice(&layer.opacity.to_le_bytes());
            buf.extend_from_slice(&(layer.blend.to_c() as u32).to_le_bytes());
            buf.push(u8::from(layer.visible));
            let (lw, lh) = layer.pixels.dimensions();
            let mut png = crate::drdc_save_plan::reserved(crate::drdc_save_plan::byte_png_bound(
                (lw, lh),
                4,
            )?)?;
            let image = layer.pixels.to_contiguous()?;
            PngEncoder::new(&mut png)
                .write_image(image.as_raw(), lw, lh, ExtendedColorType::Rgba8)
                .map_err(|e| format!("PNG encoding failed: {e}"))?;
            drop(image);
            let png_len = u32::try_from(png.len())
                .ok()
                .filter(|&len| len <= MAX_DRDC_PNG_LEN)
                .ok_or_else(|| "layer PNG too large".to_string())?;
            buf.extend_from_slice(&png_len.to_le_bytes());
            buf.extend_from_slice(&png);
            // Version 2 fields. The mask is stored raw: its length is always
            // the pixel count the entry's KIND requires — the layer's for a
            // raster entry, the CANVAS's for a group (`expected_mask_dims`) —
            // which the shared preflight and reader both check. A broken
            // pairing refuses the save before encoding, preserving the mask
            // and the prior good file.
            let mask = layer.mask.as_deref();
            buf.push(u8::from(mask.is_some()));
            buf.push(u8::from(layer.mask_enabled));
            if let Some(mask) = mask {
                let bytes = mask.as_raw();
                let mask_len =
                    u32::try_from(bytes.len()).map_err(|_| "layer mask too large".to_string())?;
                buf.extend_from_slice(&mask_len.to_le_bytes());
                buf.extend_from_slice(&mask.width().to_le_bytes());
                buf.extend_from_slice(&mask.height().to_le_bytes());
                buf.extend_from_slice(bytes);
            }
            put_opt_string(&mut buf, layer.meta.as_deref(), "meta")?;
            // Version 3 field.
            buf.push(u8::from(layer.clipped));
            // Version 4 field: the canonical style JSON.
            let style = layer.style.as_ref().map(|s| s.to_json());
            put_opt_string(&mut buf, style.as_deref(), "style")?;
            // Version 7 fields, twelve bytes: the structure, the locks and the
            // link. Reserved lock bits are masked off on the way out as well
            // as on the way in, so a file never carries a bit this build does
            // not understand.
            buf.extend_from_slice(&(layer.locks & LOCK_ALL).to_le_bytes());
            buf.extend_from_slice(&layer.link.to_le_bytes());
            buf.extend_from_slice(&layer.depth.to_le_bytes());
            buf.push(match layer.kind {
                LayerKind::Raster => 0,
                LayerKind::Group => 1,
            });
            buf.push(u8::from(layer.open));
            if layer.mask.is_some() {
                let p = &layer.mask_properties;
                buf.extend_from_slice(&p.offset.0.to_le_bytes());
                buf.extend_from_slice(&p.offset.1.to_le_bytes());
                buf.push(u8::from(p.linked));
                buf.extend_from_slice(&p.density.to_le_bytes());
                buf.extend_from_slice(&p.feather.to_le_bytes());
            }
        }
        // Version 5: the alpha channel list, after the last layer record.
        // Every cap the reader enforces is enforced here too — the count, the
        // total pixel budget (which the reader checks up front from this very
        // count) and the per-channel PNG size — so no document this build can
        // build is one it cannot read back.
        let channel_count = capacity::channel_count(self.channels.len(), self.width, self.height)?;
        buf.extend_from_slice(&channel_count.to_le_bytes());
        for channel in &self.channels {
            let (cw, ch) = channel.data.dimensions();
            if (cw, ch) != (self.width, self.height) {
                // The model's invariant; a broken one would write a file the
                // reader must reject, so refuse to write it at all.
                return Err(format!(
                    "channel plane {cw}x{ch} does not match the {}x{} canvas",
                    self.width, self.height
                ));
            }
            put_name(&mut buf, &channel.name);
            buf.extend_from_slice(&channel.overlay_color);
            buf.extend_from_slice(&sane_opacity(channel.overlay_opacity).to_le_bytes());
            buf.push(u8::from(channel.color_indicates_selected));
            let mut png = crate::drdc_save_plan::reserved(crate::drdc_save_plan::byte_png_bound(
                (cw, ch),
                1,
            )?)?;
            PngEncoder::new(&mut png)
                .write_image(channel.data.as_raw(), cw, ch, ExtendedColorType::L8)
                .map_err(|e| format!("PNG encoding failed: {e}"))?;
            let png_len = u32::try_from(png.len())
                .ok()
                .filter(|&len| len <= MAX_DRDC_PNG_LEN)
                .ok_or_else(|| "channel PNG too large".to_string())?;
            buf.extend_from_slice(&png_len.to_le_bytes());
            buf.extend_from_slice(&png);
        }
        // Version 6: the document tail, after the channel list. Every cap the
        // reader enforces is enforced here too, and the resolution is
        // re-sanitized on the way out exactly as the global light is, so no
        // document this build can build is one it cannot read back.
        let res = self.resolution.sane();
        buf.extend_from_slice(&res.x.to_le_bytes());
        buf.extend_from_slice(&res.y.to_le_bytes());
        // The built-in sRGB is the document default and is elided: an absent
        // slot reads back as exactly that profile.
        let icc = if self.profile.is_builtin_srgb() {
            None
        } else {
            Some(&**self.profile.bytes())
        };
        put_opt_bytes(&mut buf, icc, "ICC profile", MAX_DRDC_BLOB_LEN)?;
        let meta = &self.metadata;
        put_opt_bytes(
            &mut buf,
            meta.exif.as_deref(),
            "EXIF packet",
            MAX_DRDC_BLOB_LEN,
        )?;
        put_opt_bytes(
            &mut buf,
            meta.xmp.as_deref(),
            "XMP packet",
            MAX_DRDC_BLOB_LEN,
        )?;
        put_opt_bytes(
            &mut buf,
            meta.iptc.as_deref(),
            "IPTC packet",
            MAX_DRDC_BLOB_LEN,
        )?;
        // Version 8: the ruler origin and the guide list, at the very end of
        // the file. Both go through `doc_guide`'s own sanitizers — the same
        // ones the reader applies — so the bytes and the model agree by
        // construction; a guide whose position is not finite is not written
        // at all, which is why the list is built BEFORE the count.
        let origin = sane_origin(self.ruler_origin, self.width, self.height);
        let guides: Vec<(u8, f64)> = self
            .guides
            .iter()
            .filter_map(|g| {
                let extent = match g.orientation {
                    GuideOrientation::Horizontal => f64::from(self.height),
                    GuideOrientation::Vertical => f64::from(self.width),
                };
                Some((
                    g.orientation.to_c() as u8,
                    sane_position(g.position, extent)?,
                ))
            })
            .collect();
        // The writer enforces the reader's cap, as with layers and channels.
        let guide_count = u32::try_from(guides.len())
            .ok()
            .filter(|&c| c as usize <= MAX_GUIDES)
            .ok_or_else(|| format!("too many guides (max {MAX_GUIDES})"))?;
        buf.extend_from_slice(&origin.0.to_le_bytes());
        buf.extend_from_slice(&origin.1.to_le_bytes());
        buf.extend_from_slice(&guide_count.to_le_bytes());
        for (orientation, position) in &guides {
            buf.push(*orientation);
            buf.extend_from_slice(&position.to_le_bytes());
        }
        sources.encode(&mut buf, u64::MAX, crate::job_control::JobContext::NONE)?;
        crate::mask_outside_native::encode(
            &outside,
            &mut buf,
            u64::MAX,
            crate::job_control::JobContext::NONE,
        )?;
        crate::evaluation_native::encode(
            linear_light,
            &mut buf,
            u64::MAX,
            crate::job_control::JobContext::NONE,
        )?;
        crate::label_native::encode(
            &labels,
            &mut buf,
            u64::MAX,
            crate::job_control::JobContext::NONE,
        )?;
        crate::comps_native::encode(
            &comps,
            &mut buf,
            u64::MAX,
            crate::job_control::JobContext::NONE,
        )?;
        buf.extend_from_slice(&crate::source_transform_native::encode(&projective)?);
        buf.extend_from_slice(&warp);
        buf.extend_from_slice(&crate::source_refusal_native::encode(&refusals)?);
        buf.extend_from_slice(&samplers);
        buf.extend_from_slice(&paths);
        buf.extend_from_slice(&variations);
        crate::reference_native::encode(
            &references,
            &mut buf,
            u64::MAX,
            crate::job_control::JobContext::NONE,
        )?;
        crate::spot_channel_native::encode(
            &spots,
            &mut buf,
            u64::MAX,
            crate::job_control::JobContext::NONE,
        )?;
        crate::identity_native::encode(
            &ids,
            &mut buf,
            u64::MAX,
            crate::job_control::JobContext::NONE,
        )?;
        Ok(buf)
    }

    /// Writes the native DRDC format. Atomic exactly like `DrImage::save`
    /// (the shared [`save_atomically`] helper): the bytes go to a temporary
    /// file in the same directory which is renamed over `path` only on
    /// success, so a failed save never truncates or deletes an existing
    /// destination file.
    pub fn save_native(&self, path: &str) -> Result<(), String> {
        let bytes = self
            .encode_native()
            .map_err(|e| format!("failed to save document {path}: {e}"))?;
        save_atomically(path, |tmp_path| {
            std::fs::write(tmp_path, &bytes).map_err(|e| format!("failed to create {path}: {e}"))
        })
    }
}

/// Writes a NAME slot — u32 byte length + UTF-8 bytes — truncating an
/// over-long name on a UTF-8 character boundary rather than failing the save.
/// The ONE implementation, shared by the layer loop and the channel loop.
fn put_name(buf: &mut Vec<u8>, name: &str) {
    let mut name = name;
    if name.len() > MAX_DRDC_NAME_LEN as usize {
        let mut end = MAX_DRDC_NAME_LEN as usize;
        while !name.is_char_boundary(end) {
            end -= 1;
        }
        name = &name[..end];
    }
    let bytes = name.as_bytes();
    buf.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
    buf.extend_from_slice(bytes);
}

/// Writes an optional LENGTH-PREFIXED slot — u8 present, then u32 len + that
/// many bytes when present — under `cap`. The ONE implementation: the layer
/// string slots (meta, style) and the version-6 document blobs both go
/// through it. `what` is the whole noun ("layer meta", "ICC profile").
fn put_opt_bytes(
    buf: &mut Vec<u8>,
    value: Option<&[u8]>,
    what: &str,
    cap: u32,
) -> Result<(), String> {
    buf.push(u8::from(value.is_some()));
    if let Some(b) = value {
        let len = capacity::slot_length(b.len(), cap, what)?;
        buf.extend_from_slice(&len.to_le_bytes());
        buf.extend_from_slice(b);
    }
    Ok(())
}

/// Writes an optional string slot (meta, style) — the UTF-8 twin of
/// [`put_opt_bytes`], under the shared string cap.
fn put_opt_string(buf: &mut Vec<u8>, value: Option<&str>, what: &str) -> Result<(), String> {
    put_opt_bytes(
        buf,
        value.map(str::as_bytes),
        &format!("layer {what}"),
        MAX_DRDC_META_LEN,
    )
}

/// The dimensions a layer record's MASK must have: the layer's own pixel
/// size for a raster entry, the CANVAS's for a group, which has no pixel
/// buffer to be the size of (`doc_group`). The ONE definition, shared by the
/// writer's preflight and the reader's
/// deferred construction — the two would otherwise be a matched pair that can
/// drift, and the drift silently DROPS every group mask.
fn expected_mask_dims(kind: LayerKind, pixels: (u32, u32), width: u32, height: u32) -> (u32, u32) {
    match kind {
        LayerKind::Raster => pixels,
        LayerKind::Group => (width, height),
    }
}

/// The canvas area a stack's groups could ask the compositor for: the canvas
/// pixel count summed over every Group entry whose STORED properties already
/// make it allocate a full accumulator — any blend but Pass Through, a style,
/// or its own clipped flag, which ISOLATE it (`doc_group::is_isolated`), plus
/// an opacity below 1 or an enabled mask, which GATE it
/// (`doc_group::composite_gated_pass`) and cost the same one f32 buffer.
///
/// Deliberately conservative in the SAFE direction: a plain pass-through group
/// allocates nothing and is not counted, and the last isolation trigger — a
/// visible CLIPPED member above the group — is not knowable from stored
/// properties alone, which is precisely why this is a friendly early refusal
/// and `doc_group::MAX_GROUP_BUFFER_PIXELS` is the guarantee.
fn isolated_group_pixels(layers: &[Layer], width: u32, height: u32) -> u64 {
    let canvas = u64::from(width) * u64::from(height);
    layers
        .iter()
        .filter(|l| {
            // A mask or a sub-1 opacity no longer ISOLATES a pass-through
            // group (`doc_group::is_isolated`), but it still makes it allocate
            // a full accumulator-sized f32 buffer — `composite_gated_pass`'s
            // copy — so it is still counted here: what this estimates is the
            // canvas area a file's groups can ask the compositor for, not
            // which of them take the isolated path.
            l.kind == LayerKind::Group
                && (l.blend != BlendMode::PassThrough
                    || sane_opacity(l.opacity) < 1.0
                    || (l.mask.is_some() && l.mask_enabled)
                    || l.style.is_some()
                    || l.clipped)
        })
        .fold(0u64, |acc, _| acc.saturating_add(canvas))
}
