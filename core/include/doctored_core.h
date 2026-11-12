#ifndef DOCTORED_CORE_H
#define DOCTORED_CORE_H

#include <stdint.h>
#include <stdbool.h>
#include <stddef.h>

#ifdef __cplusplus
extern "C" {
#endif

/* Opaque image handle. Holds width, height, and a non-premultiplied RGBA8
 * pixel buffer (row-major, no row padding). Immutable reads and pure operations
 * may overlap across threads. Retain the handle until every operation finishes;
 * freeing requires exclusive ownership. */
typedef struct DrImage DrImage;
typedef struct DrJobControl DrJobControl;

typedef enum {
  DR_FORMAT_PNG = 0,
  DR_FORMAT_JPEG = 1,
  DR_FORMAT_TIFF = 2,
  DR_FORMAT_BMP = 3,
  DR_FORMAT_GIF = 4,
  DR_FORMAT_WEBP = 5, /* lossless */
  DR_FORMAT_HEIC = 6, /* host-encoded; export workspace only */
  DR_FORMAT_AVIF = 7, /* host-encoded; export workspace only */
  DR_FORMAT_JXL = 8,  /* lossless, 8-bit sRGB; export workspace only */
  DR_FORMAT_EXR = 9,  /* OpenEXR; float deliverable, export workspace only */
  DR_FORMAT_HDR = 10, /* Radiance RGBE; float deliverable, export workspace only */
} DrFormat;

typedef enum {
  DR_FILTER_NEAREST = 0,
  DR_FILTER_BILINEAR = 1,
  DR_FILTER_CATMULL_ROM = 2,
  DR_FILTER_LANCZOS3 = 3,
} DrResizeFilter;

/* Open the image file at `path` (UTF-8). PNG/JPEG/TIFF/BMP/GIF/WebP are
 * detected by content sniffing; a file whose first four bytes are "8BPS" is
 * decoded as a Photoshop document and flattened to its composite image.
 * A grayscale file with a Gray ICC profile and a CMYK TIFF or CMYK/YCCK
 * JPEG with a CMYK profile are converted from that profile to sRGB as they
 * are read. A CMYK file this open does not read itself (planar, extra or
 * alpha samples, another depth, past the size limits) fails, tagged or not,
 * so a host can hand it to its platform decoder.
 * On success returns a new image (free with dr_image_free) and leaves
 * *err_out untouched. On failure returns NULL and, if err_out is non-NULL,
 * sets *err_out to a heap-allocated UTF-8 message the caller must release
 * with dr_string_free. */
DrImage *dr_image_open(const char *path, char **err_out);

/* Build an image from `src`: w*h*4 bytes of STRAIGHT (non-premultiplied)
 * RGBA8, row-major, top row first, no row padding — the in-memory twin of
 * dr_image_open, for pixels this library cannot decode itself (a HEIC still
 * or a vector page the host decoded). The buffer is copied and
 * stays the caller's. Returns NULL if src is NULL, if w or h is 0, or if
 * w*h > 100000000. */
DrImage *dr_image_from_rgba(const uint8_t *src, uint32_t w, uint32_t h);

/* Frees an image. NULL is a safe no-op. */
void dr_image_free(DrImage *img);

uint32_t dr_image_width(const DrImage *img);
uint32_t dr_image_height(const DrImage *img);

/* Borrowed pointer to width*height*4 bytes of non-premultiplied RGBA8.
 * Valid until the image is freed. Never NULL for a valid image. */
const uint8_t *dr_image_pixels_rgba(const DrImage *img);

/* All operations below are pure: they return a NEW image (caller frees) and
 * never mutate their input. They return NULL only for the invalid-argument
 * cases called out per function (or if img is NULL). Alpha is preserved
 * unchanged by color operations. */

DrImage *dr_image_rotate90(const DrImage *img);  /* 90 degrees clockwise */
DrImage *dr_image_rotate180(const DrImage *img);
DrImage *dr_image_rotate270(const DrImage *img); /* 90 degrees counter-clockwise */
DrImage *dr_image_flip_horizontal(const DrImage *img);
DrImage *dr_image_flip_vertical(const DrImage *img);

/* NULL if w == 0, h == 0, or the rect is not fully inside the image. */
DrImage *dr_image_crop(const DrImage *img, uint32_t x, uint32_t y,
                       uint32_t w, uint32_t h);

/* NULL if w == 0, h == 0, or w*h > 100000000 (guard against absurd sizes). */
DrImage *dr_image_resize(const DrImage *img, uint32_t w, uint32_t h,
                         DrResizeFilter filter);

/* brightness, contrast, saturation each in [-1.0, 1.0]; 0.0 is identity for
 * all three. Out-of-range values are clamped. Applied in the order
 * brightness, then contrast, then saturation, per pixel, alpha untouched. */
DrImage *dr_image_adjust(const DrImage *img, float brightness, float contrast,
                         float saturation);

DrImage *dr_image_grayscale(const DrImage *img);
/* RGB -> 255 - c on the encoded bytes; straight alpha, and latent RGB under
 * zero alpha, kept. No ICC conversion. */
DrImage *dr_image_invert(const DrImage *img);
DrImage *dr_image_sepia(const DrImage *img);

typedef enum {
  DR_COMPOSITE_OVER = 0,  /* source-over painting (brush, text) */
  DR_COMPOSITE_ERASE = 1, /* source alpha removes destination alpha */
} DrCompositeMode;

/* Multiplies each pixel's alpha by a full-frame u8 coverage mask, returning
 * a NEW image. `mask` points to w*h bytes, row-major, top row first, one
 * byte per pixel — the selection convention: 0 hides, 255 keeps,
 * intermediate values scale alpha proportionally; w and h must equal the
 * image's dimensions exactly. Color bytes pass through, except where the
 * scaled alpha lands on 0, which clears the pixel to transparent black;
 * full-coverage (255) pixels pass through byte-for-byte, an already
 * transparent pixel's latent color included. Returns NULL if img or mask is
 * NULL or on dimension mismatch. */
DrImage *dr_image_apply_mask(const DrImage *img, const uint8_t *mask,
                             uint32_t w, uint32_t h);

/* Unsharp-mask sharpen. amount clamped to (0, 5]; NULL if amount <= 0 or
 * not finite. */
DrImage *dr_image_sharpen(const DrImage *img, float amount);

/* Encode to `path` (UTF-8). jpeg_quality (1-100) applies to DR_FORMAT_JPEG
 * only; for JPEG the image is composited over white to drop alpha. Returns
 * true on success; on failure returns false and, if err_out is non-NULL,
 * sets *err_out as in dr_image_open. Every core file writer publishes
 * atomically and durably: temp file, F_BARRIERFSYNC, rename, then a
 * best-effort F_FULLFSYNC of the parent directory. */
bool dr_image_save(const DrImage *img, const char *path, DrFormat format,
                   uint8_t jpeg_quality, char **err_out);

/* ------------------------------------------------------------------------ */
/* Layered documents                                                         */
/* ------------------------------------------------------------------------ */

/* Opaque layered document: a canvas size plus an ordered stack of layers
 * (index 0 = BOTTOM). Every layer has straight-alpha RGBA8 pixels of its own
 * size, an integer canvas offset, a name, opacity, a blend mode, a
 * visibility flag, an optional layer mask (see "Layer masks" below) and an
 * optional layer style (see "Layer styles" below); the document carries a
 * global light (angle, altitude) the styles share.
 * Layer pixel buffers are immutable and shared between
 * document handles (copy-on-write), so dr_doc_clone and the pure "with_"/
 * stack operations are cheap: they copy only what they change. Documents
 * always contain at least one layer. Like DrImage, immutable reads may overlap across threads; caches are synchronized.
 * Retain the handle until every operation finishes; freeing requires exclusive ownership. */
typedef struct DrDocument DrDocument;

/* The PSD blend-mode set, sRGB-encoded f32 math. Modes 0-13 and 15-22
 * are separable (per-channel, W3C compositing formulas); 23-26 are the W3C
 * non-separable modes operating on the RGB triple via the spec's
 * SetLum/SetSat helpers (Lum = 0.3R + 0.59G + 0.11B); DISSOLVE replaces
 * alpha compositing with a deterministic per-canvas-position dither: each
 * pixel shows the source fully opaque with probability equal to its
 * effective alpha, otherwise the backdrop. Values are stable across
 * releases; DRDC files store them as u32, and readers map unknown values to
 * DR_BLEND_NORMAL. */
typedef enum {
  DR_BLEND_NORMAL = 0,
  DR_BLEND_MULTIPLY = 1,
  DR_BLEND_SCREEN = 2,
  DR_BLEND_OVERLAY = 3,
  DR_BLEND_SOFT_LIGHT = 4,
  DR_BLEND_HARD_LIGHT = 5,
  DR_BLEND_DARKEN = 6,
  DR_BLEND_LIGHTEN = 7,
  DR_BLEND_DIFFERENCE = 8,
  DR_BLEND_EXCLUSION = 9,
  DR_BLEND_COLOR_DODGE = 10,
  DR_BLEND_COLOR_BURN = 11,
  DR_BLEND_ADDITION = 12, /* a.k.a. Linear Dodge */
  DR_BLEND_SUBTRACT = 13,
  DR_BLEND_DISSOLVE = 14,
  DR_BLEND_LINEAR_BURN = 15,
  DR_BLEND_DARKER_COLOR = 16,  /* whole-pixel: keeps the lower-luma color */
  DR_BLEND_LIGHTER_COLOR = 17, /* whole-pixel: keeps the higher-luma color */
  DR_BLEND_VIVID_LIGHT = 18,
  DR_BLEND_LINEAR_LIGHT = 19,
  DR_BLEND_PIN_LIGHT = 20,
  DR_BLEND_HARD_MIX = 21,
  DR_BLEND_DIVIDE = 22,
  DR_BLEND_HUE = 23,
  DR_BLEND_SATURATION = 24,
  DR_BLEND_COLOR = 25,
  DR_BLEND_LUMINOSITY = 26,
  /* A GROUP whose children composite straight onto the backdrop below the
     group instead of into a private buffer. Not a blend function: legal only
     on a group entry (see "Layer groups" below), refused by
     dr_doc_with_layer_blend_mode on a raster layer and by every plane and
     paint blend. */
  DR_BLEND_PASS_THROUGH = 27,
} DrBlendMode;

/* Opens a document. Sniffing order: files starting "DRDC" (written; legacy
 * "RZDC" read) are native Doctored documents (layers preserved); "8BPS" are
 * Photoshop documents,
 * imported as one layer per PSD raster layer (name, visibility, opacity,
 * best-effort blend-mode mapping; on any per-layer failure falls back to a
 * single flattened layer); anything else decodes via dr_image_open rules to
 * a single "Background" layer. A multipage TIFF opens each page that
 * matches page 1's depth, sample format, photometric and profile as a layer
 * "Page n" at the canvas's top-left, only page 1 visible; the canvas and
 * everything else are page 1's, reduced-resolution, mask and Exif thumbnail
 * images are not pages, and a later page that fails to decode fails the open
 * ("page n of <path>: ..."). dr_path_metadata_import_report names the pages
 * left out. An RGB or CMYK TIFF's spot channels (#1
 * 1c-2a: trailing extra samples described by resource 1077 in tag 34377)
 * open as spot channels, a 16-bit file's narrowed to 8 bits;
 * dr_path_metadata_import_report names them and any it cannot keep. Errors
 * as in dr_image_open. */
DrDocument *dr_doc_open(const char *path, char **err_out);

/* Wraps an image as a single-"Background"-layer document. NULL if img NULL. */
DrDocument *dr_doc_from_image(const DrImage *img);

/* Cheap copy (shares layer pixels). NULL only if doc is NULL. */
DrDocument *dr_doc_clone(const DrDocument *doc);
void dr_doc_free(DrDocument *doc);

/* Writes the native DRDC format (all layers preserved). Atomic like
 * dr_image_save. The writer enforces the reader's limits so every file it
 * produces can be read back: more than 1024 layers, more than 256 channels,
 * more than 900000000 channel pixels in total (the budget the "Channels"
 * section below states, and the one dr_max_channels_at answers), or a layer
 * or channel PNG over 1 GiB is an error; layer and channel names longer
 * than 64 KiB are truncated on a UTF-8 character boundary, and each of the
 * four version-6 document blobs is capped at 16 MiB. A malformed depth
 * sequence, a nesting deeper than ten levels, and a document whose isolated
 * groups declare more canvas area than this build will composite are errors
 * too, at BOTH ends: the writer refuses to produce a file it could not read
 * back, and so is more than 1024 guides. Layout (little-endian): "DRDC"
 * (written; legacy "RZDC" read, core/src/native_magic.rs), u32 version=10,
 * u32 canvas width, u32 canvas height, u32 layer count, then (version 4)
 * f32 global-light angle and f32 altitude in degrees; then per layer
 * bottom-to-top: u32 name byte length + UTF-8 name, i32 offset x, i32
 * offset y, f32 opacity, u32 blend mode, u8 visible, u32 PNG byte length +
 * PNG-encoded RGBA8 layer pixels; then the version-2 fields, which a
 * version-1 record simply lacks: u8 mask present, u8 mask enabled, u32 mask
 * byte length, then (v10) u32 raw mask width and height, then RAW coverage
 * bytes when present. Versions 2–9 omit mask dimensions and infer layer size
 * (canvas size for a group). Then u8
 * layer-metadata present and, when present, u32 byte length + UTF-8 bytes (see
 * "Layer metadata" below); then the version-3 field, appended after all the
 * version-2 fields (each older record is a strict prefix of the next): u8
 * clipped (see "Clipping masks" below); then the version-4 field: u8
 * layer-style present and, when present, u32 byte length + UTF-8 canonical
 * style JSON (see "Layer styles" below); then the version-7 fields, twelve
 * bytes closing the record (see "Layer groups, locks, links and structure"
 * below): u32 lock flags (a DrLockFlags bitmask, bits 3..31 reserved and
 * masked off by the reader), u32 link-group id (0 = unlinked), u16 nesting
 * depth (0 = top level), u8 kind (0 = raster, 1 = group; any other value is
 * an error) and u8 group-open, the panel disclosure flag, which is
 * meaningless on a raster entry. A GROUP entry's pixel PNG is a 1x1 fully
 * transparent PNG, so the record shape stays uniform; its MASK, however, is
 * initially canvas-sized. V10 masks have independent dimensions. A present
 * v10 mask appends i32 canvas offset x/y, u8 content-linked (0/1), f32 density
 * (0..1) and f32 feather radius (0..1000) after group-open. Versions 1–9
 * migrate with the layer/group mask offset, linked=true, density=1, feather=0.
 * V10 bounds total raw mask pixels at 6.8 billion; individual padded masks
 * remain bounded at 100 million pixels. Note that whole-FILE
 * prefixing does not hold for version 7 — its bytes go inside each record,
 * not at the tail, exactly as version 4's did. After the LAST layer record
 * comes the version-5 block, the alpha channel list (see "Channels" below):
 * u32 channel count, then per channel u32 name byte length + UTF-8 name, u8
 * overlay red, u8 green, u8 blue, f32 overlay opacity, u8 color-indicates-
 * selected, and u32 PNG byte length + a PNG-encoded 8-bit GRAYSCALE (L8)
 * plane of exactly the canvas size (a channel is always canvas-sized, so its
 * dimensions are not stored twice). After the channel list comes the
 * version-6 DOCUMENT TAIL (see "Colour management and metadata" below):
 * f32 horizontal and f32 vertical print resolution in pixels per inch, then
 * four optional blobs in this order — the ICC colour profile, the EXIF
 * packet, the XMP packet and the IPTC packet — each written as u8 present
 * and, when present, u32 byte length + that many RAW bytes. The blobs are
 * stored verbatim and never interpreted; the ICC slot is written ABSENT
 * when the document's profile is the built-in sRGB, and an absent slot
 * reads back as that profile, so a blob-less version-6 file is a version-5
 * file plus exactly 12 bytes. Last comes the version-8 GUIDE BLOCK, closing
 * the file (see "Guides, rulers and snapping" below): f64 ruler-origin x, f64
 * ruler-origin y, u32 guide count (more than 1024 is an error), then per
 * guide u8 orientation (0 = horizontal, 1 = vertical; any other value is an
 * error) and f64 position. Because it is a TAIL, a guide-less version-8 file
 * is a version-7 file with the version word bumped plus exactly 20 bytes —
 * a SIZE property, not a literal byte prefix, since the version word itself
 * sits at byte 4. Version 9 retains that byte layout and introduces
 * params.evaluation="document_v2" for Exposure/White Balance metadata (see
 * core/src/adjust.rs). The version bump prevents older readers from ignoring
 * the key and changing the layer's appearance. Version-1 through -9 files still
 * load, missing fields taking their defaults: no mask and no metadata on any
 * layer (v1), clipped false (v1 and v2), no style on any layer and a
 * (120°, 30°) global light (v1–v3), no channels (v1–v4), a 72 × 72 ppi
 * resolution, the built-in sRGB profile and no metadata packets (v1–v5),
 * an unlocked, unlinked, open RASTER entry at depth 0 — a flat stack — for
 * every layer (v1–v6), and no guides with the ruler origin at the canvas's
 * top-left (v1–v7). A
 * style is read leniently: a style from a newer build keeps the effects this
 * build knows; the resolution is sanitized rather than refused, exactly like
 * the global light, and so are the ruler origin and each guide position (a
 * non-finite origin component becomes 0, a non-finite position drops that
 * guide, an out-of-canvas value is clamped to the nearest edge). */
bool dr_doc_save_native(const DrDocument *doc, const char *path,
                        char **err_out);

/* The 8-bit native writer's planned peak scratch (drdc_save_plan): the
 * encoded-body bound, the tables held beside it, the largest plane's encode
 * transient and fixed overhead (no added margin).
 * It does not depend on cached contiguous views. Metadata only; allocates no pixels.
 * NULL or error: 0 with err_out set (free with dr_string_free). */
uint64_t dr_doc_native_save_scratch_bytes(const DrDocument *doc, char **err_out);

/* 8-bit native reopen plan (drdc_reopen_plan): bytes the reader retains and
 * its decode peak; metadata and pointer identity only, independent of cached
 * views. Writes *retained_out and *decode_out and answers true. A NULL doc or
 * out-pointer, or an error: false with err_out set (free with
 * dr_string_free) and nothing written. */
bool dr_doc_native_reopen_plan(const DrDocument *doc, uint64_t *retained_out,
                               uint64_t *decode_out, char **err_out);

uint32_t dr_doc_width(const DrDocument *doc);
uint32_t dr_doc_height(const DrDocument *doc);
size_t dr_doc_layer_count(const DrDocument *doc);

/* Layer getters. Out-of-range idx: NULL / 0 / DR_BLEND_NORMAL / false.
 * dr_doc_layer_name returns a heap string freed with dr_string_free.
 * The four geometry getters report the entry's PIXEL BUFFER rect: for a
 * RASTER entry its offset and dimensions, unchanged, and for a GROUP (see
 * "Layer groups" below) — which has no buffer of its own — the union of its
 * raster descendants' buffer rects, 0 for a group holding none. One meaning
 * for both kinds, and cheap on both: dr_doc_layer_bounds is the CONTENT box
 * (the opaque pixels), and callers that mean that ask for it by name. */
char *dr_doc_layer_name(const DrDocument *doc, size_t idx);
float dr_doc_layer_opacity(const DrDocument *doc, size_t idx);
DrBlendMode dr_doc_layer_blend_mode(const DrDocument *doc, size_t idx);
bool dr_doc_layer_visible(const DrDocument *doc, size_t idx);
int32_t dr_doc_layer_offset_x(const DrDocument *doc, size_t idx);
int32_t dr_doc_layer_offset_y(const DrDocument *doc, size_t idx);
/* The layer's reference point (fxrp, canvas px) a linked pattern fill layer's
 * origin follows; 0 on NULL doc or out-of-range idx. */
int32_t dr_doc_layer_reference_x(const DrDocument *doc, size_t idx);
int32_t dr_doc_layer_reference_y(const DrDocument *doc, size_t idx);
/* The Background mark (doc_background): the role the document's mark gives
 * its bottom entry — 0 when no mark applies (the structural rule), 1 the
 * file's Background layer, 2 an ordinary bottom layer, which a Deep knockout
 * punches through. Only PSD import sets one. 0 on NULL doc. The typed twin is
 * dr_precision_doc_background_role, in the high-depth section. */
int32_t dr_doc_background_role(const DrDocument *doc);
/* A pattern fill layer's tile after a canvas turn or flip: `pattern`
 * is the fill's pattern object as UTF-8 JSON ({"preset"} or {"name", "width",
 * "height", "rgba"}, at most 16 MiB) and (a, b, c, d) the turn's linear part,
 * (x, y) -> (a x + c y, b x + d y). Returns the turned pattern object (a
 * custom tile under the pattern's name) as heap JSON freed with
 * dr_string_free; NULL with no error when the part is not exactly a quarter
 * turn or a flip (keep the tile); NULL with *err_out set for bad input. */
char *dr_turn_pattern(const char *pattern, double a, double b, double c, double d,
                      char **err_out);
uint32_t dr_doc_layer_width(const DrDocument *doc, size_t idx);
uint32_t dr_doc_layer_height(const DrDocument *doc, size_t idx);

/* Entry idx's STABLE IDENTITY: unique among every layer this process has
 * minted, and the handle a host hangs per-layer state on. An INDEX is
 * renumbered by every insert, delete, group, ungroup, merge, reorder and
 * duplicate, and a NAME is neither unique nor stable — the id survives all of
 * them, plus a rename, an opacity change, a geometry op and undo/redo. A new
 * entry mints (including every copy a duplicate makes, through a duplicated
 * group's whole subtree); a MERGE keeps the lowest merged entry's id; a layer
 * that is gone takes its id with it. PERSISTED in .rz since 12E-1: the
 * always-written LIDS table (version 45 byte / 46 typed, below) carries every
 * id, so save/reopen, recovery and snapshots hand back the ids the document
 * had. Uniqueness is per DOCUMENT — two files may carry the same ids — and
 * the reader raises the process counter past every id it reads, so a layer
 * minted after an open never collides with one the file brought. Only a file
 * with no 45/46 word (every file an earlier build wrote) and the PSD reader
 * mint fresh ids. 0 on NULL doc / bad index, which no live layer ever
 * answers. The typed depth answers the same value as the "id" field of each
 * dr_precision_doc_info layer row. */
uint64_t dr_doc_layer_id(const DrDocument *doc, size_t idx);
/* Opaque identity of byte entry idx's pixel STORAGE — a second,
 * content-level identity beside dr_doc_layer_id. Equal nonzero values hold
 * the same pixels (a clone, an undo handle, a name/opacity/visibility/offset
 * write); every pixel write answers a new value, even one that leaves every
 * byte equal. Compare only while both rasters are owned; never dereference.
 * 0 for a NULL doc, an out-of-range index or a GROUP. Reads no pixel,
 * allocates nothing. The typed twin is dr_precision_image_storage_identity
 * applied to dr_precision_doc_layer_source. */
uint64_t dr_doc_layer_storage_identity(const DrDocument *doc, size_t idx);
/* Opaque identity of byte entry idx's raw MASK storage — what the
 * layers panel's mask thumbnail cache keys on. Equal nonzero values hold the
 * same mask plane (a clone, an undo handle, a rename, a mask
 * density/feather/link/enabled write); painting or replacing the mask
 * answers a new value. Compare only while both planes are owned; never
 * dereference. 0 for a NULL doc, an out-of-range index, an entry with no
 * mask or a GROUP. Reads no pixel, allocates nothing. The typed twin is
 * dr_precision_doc_layer_mask_storage_identity. */
uint64_t dr_doc_layer_mask_storage_identity(const DrDocument *doc, size_t idx);

/* COLOUR LABELS. A per-entry mark the layers panel filters on: 0 none, 1..7
 * exactly Photoshop's `lclr` palette codes in Photoshop's own order — red,
 * orange, yellow, green, blue, violet, gray — so import and export carry the
 * number itself with no mapping. Nothing about a label reaches the
 * compositor, and a label is EXEMPT from the layer locks, like a rename.
 * Carried by duplicate, group/ungroup and every geometry op; a MERGE keeps
 * the LOWEST merged entry's label (dr_doc_layer_id's rule) and a FLATTEN
 * makes one new picture, so its entry is unlabelled. PERSISTED: a .rz whose
 * document carries any label writes version 41 (byte) / 42 (typed) as its
 * OUTERMOST version word and one LLBL table after every prior tail; a
 * document with no label writes exactly the bytes the prior writer made, so
 * an unlabelled file is unchanged and an older build still reads it. A
 * layered PSD carries labels in `lclr` in both directions at both depths.
 * The typed depth sets a label through the dr_precision_doc_edit op
 * {"op":"layer_label","layer":N,"label":0..7} and reads it from the "label"
 * field of each dr_precision_doc_info layer row. */
#define DR_LAYER_LABEL_MAX 7
/* NULL on a NULL doc, an out-of-range idx, a label above DR_LAYER_LABEL_MAX,
 * and on the label the entry already carries (the no-op rule below). */
DrDocument *dr_doc_with_layer_label(const DrDocument *doc, size_t idx,
                                    uint32_t label);
/* 0 on a NULL doc or a bad index, which is also what an unlabelled entry
 * answers. */
uint32_t dr_doc_layer_label(const DrDocument *doc, size_t idx);

/* LAYER COMPS. A named recording of per-layer VISIBILITY, POSITION (offset)
 * and APPEARANCE (opacity, blend mode, layer style), by STABLE IDENTITY and
 * never pixels — deliberately NOT a snapshot, which holds the whole document.
 * Which of the three attributes a comp records is the comp's own, fixed when
 * it is saved; a flag that is off records nothing and applying the comp
 * leaves that attribute alone. POSITION is recorded for RASTER entries only
 * (a group's offset is a phantom derived from its descendants, and a typed
 * adjustment has none).
 *
 * BINDING. An entry names a layer by dr_doc_layer_id. An entry whose id no
 * longer matches any layer is STALE: it is reported under the name the layer
 * carried when the comp was saved and applying the comp skips it. A RENAME
 * does not make an entry stale — the id carries through it.
 *
 * LOCKS. Visibility and appearance are lock-EXEMPT, like a rename and a
 * colour label. POSITION honours DR_LOCK_POSITION: a bound entry whose
 * recorded offset differs from the layer's current one on a position-locked
 * layer is reported "locked" and its offset is SKIPPED, while the same
 * entry's visibility and appearance still apply. A locked entry is reported,
 * never a failure. Since 12E-1b a TYPED layer carries the same locks, so
 * "locked" is a real answer at both depths.
 *
 * GEOMETRY. A comp follows the canvas at both depths. Every whole-canvas
 * geometry op — crop, canvas resize, image resize, the three quarter turns,
 * the two flips, arbitrary canvas rotation and straighten (the byte
 * "rotate_canvas" / "straighten" requests of dr_doc_geometry_edit) and their
 * dr_precision_doc_edit twins — remaps each BOUND
 * RASTER entry's recorded offset to the offset the op would have given the
 * layer had it sat there, so applying the comp afterwards lands the layer
 * where it would have landed before, and a position-locked layer is not
 * reported "locked" merely because the canvas moved. An entry whose layer
 * carries a retained source is remapped through the source bounds, exactly
 * as the re-render places the layer. Image resize scales a recorded layer
 * style by the same factor as the live one (as does a straighten whose
 * matrix scales); visibility, opacity and blend are never touched, and
 * stale, group and position-less entries are left as they are. Per-layer
 * transforms (free transform, move, align, warp) leave comps alone. A text
 * or shape layer the host re-renders or refits after the op has its entries
 * shifted to its new offset (the "shift" request).
 *
 * PERSISTED: a .rz whose document holds any comp writes version 43 (byte) /
 * 44 (typed) as its OUTERMOST version word and one LCMP table — u32 schema,
 * u32 length, then that many bytes of UTF-8 JSON — after every prior tail,
 * LLBL included; a comp-less document writes exactly the bytes the prior
 * writer made, so such a file is unchanged and an older build still reads it.
 *
 * LCMP SCHEMA 2 (12E-1b), written by BOTH depths: an entry is
 * {"id": <the entry's layer id>, "name": S, ...} with NO "layer" key, and the
 * reader binds it when some record in the FILE carries that id — the name is
 * not compared, so a reopened comp survives a reorder AND a rename. The id is
 * written VERBATIM: 0 for an entry that never bound, a dangling id for one
 * whose layer is gone, and both reopen stale. The binary schema word and the
 * JSON "schema" key are the SAME number and must agree, or the file is
 * refused; a schema-2 payload in a file with no LIDS table is refused too
 * ("native layer comps by id require a layer identity table"), so no crafted
 * file can bind by coincidence. A repeated non-zero id within one comp
 * refuses. SCHEMA 1 is still READ exactly as before — {"layer": N|null} bound
 * by INDEX + NAME — and is never written again, so a reopened schema-1 file
 * resaves as schema 2. Photoshop's own `cmls` layer comps are NOT imported or
 * exported: a recorded gap, owner 10C-3. 12E-3c measured the structure of a
 * Photoshop-authored comps family and did not import it; the import keeps no
 * comp.
 *
 * PERSISTED LAYER IDENTITY. Version 45 (byte) / 46 (typed) is the OUTERMOST
 * .rz version word and the ONE table this format ALWAYS writes: 45/46 sits
 * before every prior version word, 43/44 included, and `LIDS` — u32
 * schema=1, u32 count, then count u64 little-endian layer ids — closes the
 * file after every prior tail, LCMP included. The byte index is the
 * bottom-first layer index; the typed index is the postorder record index.
 * An id is a property of EVERY layer, so the table is unconditional: "a
 * document without the table" means a FILE with no 45/46 word, which reads
 * exactly as before and MINTS fresh ids. The reader ADOPTS the ids it reads,
 * so reopen-then-resave is byte-identical. A bad tag or schema, a count that
 * is not the record count, a zero id and a duplicate id all refuse by name
 * before a single PNG is decoded; older builds refuse 45/46 by name
 * ("unsupported DRDC version"). Since 12E-1b comps bind by this persisted id
 * (LCMP schema 2, above).
 *
 * TYPED LOCKS AND LINKS IN .rz. Version 47 is the one version word that sits
 * INSIDE the identity wrapper rather than outside it — 46 -> 47 -> 44 -> ...
 * — and its `LLNK` table (u32 schema=1, u32 count = the record count, then
 * per record u32 locks and u32 link, in postorder) lands after LCMP and
 * BEFORE LIDS. Both 12E-1 invariants therefore still hold: 46 is the first
 * word of every typed .rz this build writes and LIDS is the last table.
 * Written only when some record carries a lock or a link, so a document with
 * neither is byte-for-byte the 12E-1 writer's file and releasing the last
 * lock restores those bytes; reopen-then-resave stays byte-identical. Lock
 * bits outside DR_LOCK_ALL are MASKED on read, never refused. The BYTE depth
 * needs no table: its layer record has carried locks and link since v7.
 *
 * TYPED GUIDES AND RULER ORIGIN IN .rz. Version 48 is the SECOND word inside
 * the identity wrapper, beside 47 — 46 -> 47 -> 48 -> 44 -> ... — and its
 * `LGDE` table (u32 schema=1, f64 ruler-origin x, f64 ruler-origin y, u32
 * guide count, then per guide u8 orientation and f64 position: the version-8
 * BYTE guide block's layout after an 8-byte tag/schema prefix) lands after
 * LLNK and BEFORE LIDS. Both 12E-1 invariants still hold. Written only when
 * the document carries a guide or has moved its ruler origin, so a document
 * with neither is byte-for-byte the 12E-1b writer's file and clearing the
 * last guide with the origin back at (0, 0) restores those bytes;
 * reopen-then-resave stays byte-identical. Positions are REPAIRED (a
 * non-finite one drops that guide, an out-of-canvas one clamps, everything
 * quantizes to four decimals) while the orientation byte and the count are
 * REFUSED — the byte reader's own split. Guide IDS are deliberately not
 * stored at either depth, so a reopen mints fresh ones. The BYTE depth needs
 * no table: its version-8 tail has carried both since long before this.
 *
 * COLOUR SAMPLERS IN .rz. Version 59 (byte) / 60 (typed) sits inside
 * the identity wrapper ahead of 56 / 58 — ... -> 59 -> 56 -> ... and
 * ... -> 60 -> 58 -> ...; the later words, 74 / 75 first, come before it —
 * and its `CSMP` table (u32 schema=1, u32 count
 * 1..10, then per sampler u32 x, u32 y, u8 sample size 1/3/5, all
 * little-endian) is the last table before PATH and LIDS (byte after RSPR,
 * typed after LGLT). Written only when the document holds a sampler, so a sampler-free
 * document is byte-for-byte the prior writer's file. A bad tag or schema, a
 * count of 0 or past 10 and a bad size refuse before any PNG decodes; an
 * entry off the canvas is DROPPED. Older byte builds refuse a 59 file as
 * "unsupported DRDC version 59", older typed builds a 60 file with their
 * generic high-depth sentence. See "Colour samplers" below.
 *
 * PATHS IN .rz. Version 74 (byte) / 75 (typed) is the FIRST word
 * inside the identity wrapper — 45 -> 74 -> 72 -> ... and
 * 46 -> 75 -> 71 -> ... — and its `PATH` table (u32 schema=1, u32 JSON
 * length 1..16 MiB, that many bytes of UTF-8 JSON
 * {"paths":[{"name":S,"kind":"work"|"saved", ...shape}],
 *  "vector_masks":[{"layer":I,"enabled":B,"linked":B, ...shape,
 *    "raster":null|{"plane":I|null,"x":I,"y":I,"linked":B,"density":F,
 *                   "feather":F,"outside":0|255,"enabled":B}}]},
 * then a u32 plane count and per plane u32 width, u32 height and the raw
 * coverage — u8 at byte depth, u16 at high depth — all little-endian) is the
 * LAST table before LIDS, after CSMP. A vector mask names its layer by its
 * bottom-first index; that layer's own mask record is the BAKED plane, so
 * the picture does not depend on this table. A raster part's "plane" indexes
 * the planes that follow, each named exactly once; null says the raster part
 * IS the layer's stored mask. Written only when the document holds a path or
 * a vector mask, so any other document is byte-for-byte the prior writer's
 * file. A bad tag, schema, length or JSON, an empty table, a shape past its
 * caps, a broken list rule, a vector mask naming no layer or a layer twice,
 * planes that do not pair up with the raster parts, and a record that does
 * not agree with its layer's stored mask refuse before any PNG decodes. Path
 * ids are not stored; a reopen mints fresh ones. Older byte builds refuse a
 * 74 file as "unsupported DRDC version 74", older typed builds a 75 file
 * with their generic high-depth sentence. See "Paths" below.
 *
 * SMART OBJECTS, STACK MODE AND LIVE FILTERS IN OLD .rz FILES. Builds
 * before 2026-10-10 wrote smart-object contents under word 65 (byte) / 66
 * (typed) and a SOCT table last before LIDS, float contents under word 73 /
 * 68, Stack Modes under word 69 / 70 and a SOSM table after SOCT, and live
 * filters in the v23/v24 LFLT table after RBIN. This build writes none of
 * them and still reads them: each table's framing is checked and skipped,
 * nothing in it is decoded, and every source binding whose asset had
 * contents or whose LFLT slot was present is dropped, so its layer opens as
 * an ordinary raster with its stored pixels. A word without its companion
 * (69/70 or 73/68 without 65/66, 73/68 with no float entry) refuses
 * ("invalid native smart object contents word").
 *
 * FLOAT MASTERS IN .rz. A 32-bit float document whose placed
 * image keeps a 32-bit float master writes version 67 as the FIRST word
 * inside the identity wrapper after 71 (46 -> 67 -> 64 -> ...), with no table:
 * the master is an RSRC asset whose advanced depth byte is 32 and whose u64
 * length/payload slot holds w*h*16 bytes of raw little-endian f32 RGBA
 * (straight alpha, the samples bit-exact) instead of a PNG. Written only
 * when some written asset is a float master, so any other document is
 * byte-for-byte the prior writer's file. The reader admits depth 32 only
 * under 67 ("unsupported embedded source depth or flags" otherwise) and
 * refuses 67 with no depth-32 asset ("invalid native float master word"),
 * before any payload decodes; the reopen plan charges a
 * float master as a float raster. Older typed builds refuse a 67 file with
 * their generic high-depth sentence. A 16-bit document holding a float
 * master writes 67 the same way (earlier float-master builds refuse it:
 * "a 32-bit float master requires a 32-bit float document"), and
 * an 8-bit document writes version 72 as the FIRST word inside the byte
 * identity wrapper after 74 (45 -> 72 -> 71 -> 63 -> ...), with no table and
 * the same depth-32 RSRC asset; the byte reader admits depth 32 only under
 * 72 and refuses 72 with no depth-32 asset ("invalid native float master
 * word") before any PNG decodes; a byte file without one is byte-for-byte
 * the prior writer's. Older byte builds refuse a 72 file as "unsupported
 * DRDC version 72".
 *
 * REQUEST SCHEMA, the same at both depths:
 *   {"op":"layer_comp","action":"save",  "name":S,"comment"?:S,
 *    "visibility"?:B,"position"?:B,"appearance"?:B}
 *   {"op":"layer_comp","action":"update","comp":N,"name"?:S,"comment"?:S,
 *    "visibility"?:B,"position"?:B,"appearance"?:B,"recapture"?:B}
 *   {"op":"layer_comp","action":"delete","comp":N}
 *   {"op":"layer_comp","action":"apply", "comp":N}
 *   {"op":"layer_comp","action":"reorder","comp":N,"to":M}
 *   {"op":"layer_comp","action":"shift","layer":N,"dx":DX,"dy":DY}
 * Naming ANY of the three capture flags states the WHOLE set; an unnamed flag
 * becomes true, the same default a save with no flags takes. "recapture"
 * defaults to true. "comp" is the comp's index in the list the report form
 * returns. A reorder's "to" is the comp's FINAL index (remove, then insert),
 * as reorder_layer counts; "to" equal to "comp" is the exact no-op.
 * A shift moves every recorded offset of layer N (the byte index, or the
 * typed postorder record index) by (DX, DY) in every comp: the host's
 * bookkeeping after it re-renders a described layer, lock-exempt,
 * with no layer moved. It refuses "layer comp shift requires a layer
 * index", "layer comp shift requires dx and dy" and "layer comp shift layer
 * index is out of range"; a zero delta, a comp-less document, a non-raster
 * layer or no recorded offset of that layer is the exact no-op.
 *
 * REPORT SCHEMA (dr_doc_layer_comps_json and its typed twin):
 *   {"schema":1,"count":N,"comps":[{"index":N,"name":S,"comment":S,
 * (the REPORT's own "schema" is the report form's version and is unrelated to
 *  the LCMP file schema above)
 *     "captures":{"visibility":B,"position":B,"appearance":B},
 *     "stale_count":N,"locked_count":N,
 *     "entries":[{"layer":N|null,"id":N,"name":S,"stale":B,"locked":B,
 *                 "visible"?:B,"offset"?:[x,y],"opacity"?:F,"blend"?:N,
 *                 "style"?:S}]}]}
 * A key is present iff the comp's flag captures it. "layer" is the CURRENT
 * bound index or null. The byte index is bottom-first; the typed index is the
 * flattened postorder record index dr_precision_doc_info uses. */
#define DR_LAYER_COMPS_MAX 64
/* The report form; free with dr_string_free. NULL + an error on a NULL doc. */
char *dr_doc_layer_comps_json(const DrDocument *doc, char **err_out);
/* One comp request. NULL WITH NO ERROR is the exact no-op (an update that
 * changes nothing, an apply whose every field already holds); NULL with an
 * error is a refusal whose sentence the host shows verbatim — a duplicate
 * name, an empty name, a comp index out of range, a reorder destination out
 * of range ("layer comp destination index M is out of range (0..K)") or
 * missing, the 64-comp limit, or "layer comps exceed the .rz payload cap". */
DrDocument *dr_doc_edit_layer_comps(const DrDocument *doc, const char *request,
                                    char **err_out);
/* The TYPED report is dr_precision_doc_layer_comps_json, declared beside the
 * other typed entry points below (it needs DrPrecisionDocument). The typed
 * MUTATION is dr_precision_doc_edit with the same
 * {"op":"layer_comp","action":...} request — there is no separate export. */

/* Copy of a layer's pixels at the layer's own size. NULL on a GROUP, which
 * has no pixels of its own. */
DrImage *dr_doc_layer_image(const DrDocument *doc, size_t idx);

/* A layer's own pixels on a transparent CANVAS-sized image, placed at its
 * offset — the single-layer counterpart of dr_doc_flattened. Opacity, blend
 * mode, visibility and the layer mask are ignored: they say how the layer
 * composites, not what its pixels are. On a GROUP this is its RAW projection
 * — its children only, without the group's own opacity, blend mode, mask or
 * style — which keeps exactly the same contract. */
DrImage *dr_doc_layer_canvas_image(const DrDocument *doc, size_t idx);

/* Aspect-fit thumbnail of a layer, longest side == max_side (min 1; tiny
 * layers upscale), Triangle filter. A raster at least 16x the thumbnail per
 * axis is box-reduced out of its tiles first (premultiplied box means, then
 * the kernel), measured within 2/255 of the single Triangle pass and with no
 * whole-layer copy; a smaller raster keeps that single pass byte for byte.
 * On a GROUP it is that projection, aspect-fit; a layers panel wanting a
 * cheap group row should draw a folder glyph rather than ask for one per
 * reload. */
DrImage *dr_doc_layer_thumbnail(const DrDocument *doc, size_t idx,
                                uint32_t max_side);

/* Canvas-sized projection: visible layers composited bottom-to-top in f32,
 * straight-alpha result. Compositing follows the W3C model: with backdrop
 * (Cb, ab), source layer (Cs, as' = as * opacity) and blend function B:
 *   ao = as' + ab*(1-as')
 *   Co = ( as'*(1-ab)*Cs + as'*ab*B(Cb,Cs) + (1-as')*ab*Cb ) / ao   (ao > 0)
 * Invisible layers are skipped; areas a layer does not cover use Cb. Layers
 * flagged clipped composite in groups with the unclipped SIBLING beneath them
 * (see "Clipping masks" below), and LAYER GROUPS composite as described under
 * "Layer groups" below — pass-through groups transparently, isolated ones
 * rendered once into a private buffer. Layers carrying a style composite with their
 * effects (see "Layer styles" below). */
// Never returns a partial image: render failures return NULL with err_out.
// group_buffer_pixels: 0 = production limit; positive values only lower it.
// control may be NULL; the render runs under it and a cancel returns NULL with
// "operation cancelled". A control is single-use: a second run on it returns
// NULL with "job control has already been used".
DrImage *dr_doc_flattened(const DrDocument *doc, uint64_t group_buffer_pixels,
                          const DrJobControl *control, char **err_out);

/* Regional document render, contract version 1 (Phase 13A-1; masks and clip
 * units since 13A-2a; groups since 13A-3a; raster layer styles with effects
 * since 13A-3b-i; group layer styles with effects since 13A-3b-ii; knockout
 * and Blend Interior Effects as Group since 13A-3c). The
 * reference is dr_doc_flattened:
 * output pixel (i, j) of the window (x, y, w, h) equals the projection's pixel
 * (x + i*d, y + j*d), sample for sample, and the output is ceil(w/d) x
 * ceil(h/d) straight RGBA8. d = preview_divisor >= 1; d = 1 is authoritative,
 * a larger d is a nearest-sampled PREVIEW only. Admitted are raster layers
 * (any offset, blend mode, opacity; any layer style — enabled effects, fill
 * opacity, Blend If, knockout Shallow/Deep with the Background rule, Blend
 * Interior Effects as Group — evaluated per sample exactly as the full
 * projection's unit pass), pointwise
 * adjustment layers and isolated or pass-through GROUPS (plain or gated by
 * opacity/mask, nested, hidden or empty; any layer style — a group with an
 * enabled effect is materialized before the loop by the reference's own
 * rendered_group, charged against the group budget) at every level, and
 * (since 13A-4a) a guided Shadows/Highlights at the ROOT, lone or baseless
 * clipped, evaluated in STAGES: the layers below it at divisor 1 over a
 * declared plane, the reference's own guide built from that plane (EXACT) —
 * since 13A-4b-i also inside an ISOLATED group's level (nested, staged over
 * planes aligned to and clamped by the group's buffer, its last plane held at
 * divisor 1 even for a preview) and anywhere inside a materialized subtree,
 * since 13A-4b-ii-a directly inside a PASS THROUGH group (plain or gated,
 * nested; staged by the enclosing level, the sample as the group's children
 * began held as a plane where it is read), since 13A-4b-ii-b-1 as a CLIP
 * MEMBER of an unstyled base (staged by the enclosing level, the sample as
 * the unit began held and the unit re-run from it), and since 13A-4b-ii-b-2
 * as a clip member of a STYLED base (fill opacity, effects, knockout, a
 * styled group; at 8 bits not the package below) —
 * each with or without a layer mask (raw placement, density, feather; enabled
 * or not), grouped into CLIP UNITS: an unclipped entry plus the clipped run
 * above it (hidden and zero-opacity members included, adjustment and group
 * bases too), or a lone baseless clipped entry. Anything else is REFUSED BY
 * NAME, never rendered through a full-frame fallback, as "regional render
 * refuses layer N (name): <construct>" in this order, bottom-first over the
 * units of each level: a hidden or zero-opacity base skips its unit (and every
 * subtree in it) and hidden or zero-opacity members are skipped; then, entry
 * by entry, a group's own level (recursively) or, for a guided
 * Shadows/Highlights as a clip member of a PACKAGED styled base at 8 bits
 * (effects below opacity 1, not Dissolve), "is Shadows/Highlights with a
 * guide under a packaged styled base (8-bit effects below opacity 1)". The
 * only structure
 * refusal is
 * the
 * reference's own: a contributing group past its nesting limit refuses
 * "rendering exceeds the group nesting limit", and a materialized group's
 * nest is charged as the reference charges it ("rendering exceeds the group
 * buffer budget (N pixels); ..."; only materialized nests are charged, so a
 * budget refusal the reference raises for an unmaterialized level, or for
 * the knockout Background / pass-through Shallow snapshots it holds, does not
 * refuse here). Every refusal dr_doc_flattened raises
 * (the evaluation policy, the calibrated context) is raised by the render. The
 * region must be nonempty and inside the canvas; the RGBA8 output must fit
 * max_output_bytes. A preview below a guided row
 * still evaluates its stages at divisor 1. The render checks cancellation every 4096 samples; a cancelled or
 * refused render returns NULL plus an owned err_out and no partial pixels.
 * NULL doc -> NULL, no error. */
DrImage *dr_doc_render_region(const DrDocument *doc, uint32_t x, uint32_t y,
    uint32_t w, uint32_t h, uint32_t preview_divisor, uint64_t max_output_bytes,
    const DrJobControl *control, char **err_out);

/* Pure per-layer setters: return a NEW document (input untouched), NULL on
 * out-of-range idx or NULL args. Opacity is clamped to [0,1]. Each also
 * answers NULL when the entry ALREADY holds the value passed (the offset
 * pair included, on a group as on a layer): an op that changes nothing
 * returns nothing, so a host reading a row and writing it straight back adds
 * no undo step and does not dirty the document. */
DrDocument *dr_doc_with_layer_name(const DrDocument *doc, size_t idx,
                                   const char *name);
DrDocument *dr_doc_with_layer_opacity(const DrDocument *doc, size_t idx,
                                      float opacity);
/* DR_BLEND_PASS_THROUGH is legal only on a GROUP; naming it for a raster
 * layer is a refusal (NULL). */
DrDocument *dr_doc_with_layer_blend_mode(const DrDocument *doc, size_t idx,
                                         DrBlendMode mode);
DrDocument *dr_doc_with_layer_visible(const DrDocument *doc, size_t idx,
                                      bool visible);
/* On a GROUP this SHIFTS the whole subtree, so the origin of the rect the four
 * geometry getters report — the union of its layers' buffer rects — lands at
 * (x, y); NULL for a group holding no layer. Every canvas-sized GROUP MASK in
 * that subtree slides by the same delta when content-linked; an unlinked mask
 * keeps its independent canvas position. Raw coverage outside its bounds is 0.
 * Reading an entry's offset and writing it back is therefore the identity on
 * both kinds of entry. It is a property write and deliberately does NOT follow
 * links. Its typed twin is dr_precision_doc_edit's affine_set with
 * follow_links:false (the "Native layer offset" block). */
DrDocument *dr_doc_with_layer_offset(const DrDocument *doc, size_t idx,
                                     int32_t x, int32_t y);

/* Replaces a layer's pixels (any size; offset and properties kept).
 * dr_doc_set_layer_content takes a rendered buffer (with its offset and
 * optional mask) instead of an image handle. */
DrDocument *dr_doc_with_layer_pixels(const DrDocument *doc, size_t idx,
                                     const DrImage *img);

/* Stack operations (all pure). dr_doc_adding_image_layer inserts the new
 * layer ABOVE idx — above its whole SUBTREE, at idx's own depth, which is
 * position idx+1 on a document with no groups and dr_doc_layer_subtree's
 * *out_end in general. idx must be in range. */
DrDocument *dr_doc_adding_image_layer(const DrDocument *doc, size_t idx,
                                      const DrImage *img, const char *name);
/* dr_doc_adding_image_layer's placement with the new layer at (x, y): a paste is
 * MADE there, so its Photoshop reference point (fxrp) is (0, 0). NULL on NULL args
 * or out-of-range idx. */
DrDocument *dr_doc_add_image_layer_at(const DrDocument *doc, size_t idx, const DrImage *img,
                                      const char *name, int32_t x, int32_t y);
/* POSITIONAL insertion (both depths share this slot rule; see
 * core/src/layer_insert.rs, whose module doc IS the rule):
 *   DR_INSERT_ABOVE       above the anchor's whole SUBTREE, at its own level
 *                         — exactly dr_doc_adding_image_layer's placement.
 *   DR_INSERT_TOP         the top of the ROOT level, whatever the anchor is
 *                         (the anchor is still range-checked).
 *   DR_INSERT_INTO_GROUP  the anchor's TOP CHILD; the anchor must BE a group,
 *                         else the call refuses ("into_group requires a group
 *                         layer").
 * A new entry JOINS a clip run rather than splitting it: when the next
 * SIBLING at the slot's level is clipped, the new entry is created clipped
 * too, so no neighbour is re-based onto it. Insertion is
 * STRUCTURE and is therefore lock-EXEMPT, like delete, reorder and rename.
 * NULL for an out-of-range anchor, a refused position, a level past the
 * nesting cap, or growth past native capacity. */
typedef enum {
    DR_INSERT_ABOVE = 0,
    DR_INSERT_TOP = 1,
    DR_INSERT_INTO_GROUP = 2
} DrInsertPosition;
DrDocument *dr_doc_adding_layer_at(const DrDocument *doc, size_t anchor,
                                   uint32_t position, const char *name);
/* Duplicates the entry and, for a group, its whole subtree — depths kept,
 * " copy" appended to the top entry's name only. Metadata and style are
 * copied like every other property; LINKS are not. */
DrDocument *dr_doc_duplicating_layer(const DrDocument *doc, size_t idx);
/* Removes the entry and, for a group, its whole subtree. NULL when that would
 * leave the document with no entries at all. */
DrDocument *dr_doc_removing_layer(const DrDocument *doc, size_t idx);
/* Merges layer idx (idx >= 1) into the layer below it. BOTH layers' blend
 * modes and opacities are baked into the merged pixels (same math as the
 * projection, the lower compositing onto a transparent backdrop), so the
 * merged layer is DR_BLEND_NORMAL at opacity 1.0; it keeps only the LOWER
 * layer's name, visibility and clipped flag and covers the union of both
 * layers' extents. A CLIPPED upper layer is baked through its clipping: its
 * contribution is alpha-limited to the lower layer's footprint (the same
 * group kernel as the projection — see "Clipping masks" below). An invisible
 * upper layer is simply removed. "The layer below" is the previous SIBLING —
 * the entry immediately below within the entry's own level, which is idx - 1
 * on a document with no groups — and either operand may be a GROUP, which is
 * rendered to pixels first — over the group's WHOLE extent, off-canvas
 * content included, exactly as a raster layer's pixels are kept — so a group
 * merged down (or merged into) becomes a plain raster entry and its subtree
 * is removed. NULL if idx is at the BOTTOM
 * of its level / out of range, or if the LOWER entry is hidden (the merge
 * would discard the upper entry's content). A merged layer's KNOCKOUT is
 * evaluated against its REAL backdrop, captured from the document's own
 * projection, so the hole is filled with that backdrop's pixels and the
 * picture is kept wherever it is opaque. Where no backdrop exists BY
 * STRUCTURE — inside an isolated group, inside a merged pass-through group,
 * or clipped to a base outside the merge — the hole still bakes as
 * TRANSPARENCY ("Layer styles" above); dr_doc_flattening,
 * dr_doc_stamp_visible and dr_doc_merge_visible bake it exactly. The merged
 * layer keeps the LOWER layer's id, so a layer comp saved before the merge
 * follows it: that layer's recorded offsets shift with the merged origin (so
 * applying the comp puts its content back where it was recorded) and its
 * recorded opacity, blend mode and style become the merged layer's own
 * (opacity 1, Normal, no style), so Apply never bakes them twice. */
DrDocument *dr_doc_merging_down(const DrDocument *doc, size_t idx);

/* Single-layer document containing the projection, named "Background". */
DrDocument *dr_doc_flattening(const DrDocument *doc);

/* Paints a CANVAS-frame overlay onto layer idx, mapped through the layer's
 * offset. `src` points to w*h*4 bytes of PREMULTIPLIED RGBA8 (the
 * CoreGraphics bitmap-context layout), row-major, top row first, no row
 * padding; w and h must equal the canvas size exactly. `alpha` is clamped to
 * [0, 1] and scales the overlay's alpha before compositing; the layer stays
 * non-premultiplied. DR_COMPOSITE_OVER paints the overlay over the layer;
 * DR_COMPOSITE_ERASE uses the overlay's alpha to erase layer alpha and
 * ignores the overlay's color. Where the overlay is fully transparent the
 * layer's bytes pass through exactly. Overlay areas outside the layer's
 * extent are ignored (the layer does NOT grow). On a transparency-locked
 * layer OVER keeps every pixel's alpha: an alpha-0 pixel stays, and a
 * semi-transparent one takes lerp(Cb, Cs, sa) — the paint at its own
 * strength. NULL if doc or src is NULL,
 * on dimension mismatch, on an unknown mode, if alpha is NaN, and when the
 * layer's extent does not intersect the canvas at all (no pixel could
 * change, so there is nothing to paint). */
DrDocument *dr_doc_painting_layer(const DrDocument *doc, size_t idx,
                                  const uint8_t *src, uint32_t w, uint32_t h,
                                  DrCompositeMode mode, float alpha);

/* Paints the same canvas-frame premultiplied overlay onto layer idx
 * through a layer blend mode — the overlay-ink form: Clone Stamp's Blend
 * option (the colour brush uses dr_doc_painting_layer_blend_color). Each
 * covered pixel's colour is un-premultiplied out of the overlay and runs the
 * W3C compositing formula the layer projection uses, so the stroke lands
 * exactly what a `mode` layer holding it would flatten to against the
 * layer's current pixels; on a transparency-locked layer a semi-transparent
 * pixel takes lerp(Cb, B(Cb, Cs), sa) and keeps its alpha.
 * DR_BLEND_NORMAL delegates to dr_doc_painting_layer with DR_COMPOSITE_OVER
 * (byte-identical, refusal rules included). alpha clamps to [0, 1]. NULL on
 * NULL args, dimension mismatch, unknown mode, NaN alpha, bad idx, a layer
 * extent that misses the canvas, or — non-Normal modes only — when no pixel
 * would change. */
DrDocument *dr_doc_painting_layer_blend(const DrDocument *doc, size_t idx,
                                        const uint8_t *src, uint32_t w,
                                        uint32_t h, DrBlendMode mode,
                                        float alpha);

/* The colour brush's Blend option: dr_doc_painting_layer_blend with the
 * STRAIGHT ink `rgb` (3 bytes, the document's space) and only the overlay's
 * alpha (byte 4i+3 of the same canvas-frame buffer) read, as coverage — so a
 * low-coverage stroke blends the exact colour rather than one
 * un-premultiplied out of a few overlay codes. DR_BLEND_NORMAL
 * delegates to dr_doc_painting_layer with DR_COMPOSITE_OVER and the overlay
 * itself (byte-identical). Refusals are dr_doc_painting_layer_blend's, plus
 * NULL rgb. */
DrDocument *dr_doc_painting_layer_blend_color(const DrDocument *doc,
                                              size_t idx, const uint8_t *src,
                                              uint32_t w, uint32_t h,
                                              const uint8_t *rgb,
                                              DrBlendMode mode, float alpha);

/* Edit > Fill...: fills layer idx with the straight RGBA8 colour rgba (4
 * bytes) through blend mode `mode` at `opacity` (clamps to [0, 1]),
 * weighted by `mask` — canvas-sized selection coverage, or NULL for the
 * whole canvas; w/h must equal the canvas size either way. The fill is the
 * colour at each pixel's coverage, composited by the paint tools' one walk
 * (locks included) with the exact colour in every blend mode.
 * preserve_transparency (or a transparency lock) keeps every pixel's alpha:
 * an alpha-0 pixel stays, a semi-transparent one takes
 * lerp(Cb, B(Cb, Cs), sa) — the fill at its own opacity x coverage.
 * The layer never grows. NULL on NULL doc/rgba, dimension
 * mismatch, unknown or Pass Through mode, NaN opacity, a non-raster or
 * out-of-range idx, a pixel-locked layer, or when no pixel would change
 * (empty mask, same-colour Normal fill, a preserved fill over a fully
 * transparent area). */
DrDocument *dr_doc_fill_selection(const DrDocument *doc, size_t idx,
                                  const uint8_t *mask, uint32_t w, uint32_t h,
                                  const uint8_t *rgba, DrBlendMode mode,
                                  float opacity, bool preserve_transparency);

/* Edit > Stroke...: where the stroke band sits relative to the selection's
 * 50% contour. */
typedef enum {
  DR_STROKE_INSIDE = 0,
  DR_STROKE_CENTER = 1,
  DR_STROKE_OUTSIDE = 2
} DrStrokeLocation;

/* Edit > Stroke...: replaces a canvas-sized selection mask in place with a
 * band width_px wide at `location` — Outside dilate(mask) * (255 - mask),
 * Inside mask - erode(mask), Center half of each (Layer Style > Stroke's
 * bands). For Inside the canvas edge is a contour too (a virtual outside
 * ring one pixel beyond the canvas), so a Select All strokes a frame. The
 * band is then the mask handed to dr_doc_fill_selection (clipped to the
 * layer's own pixels). width_px <= 0 returns true with an all-zero mask.
 * Returns false, the mask untouched, on NULL mask, zero dimensions, a
 * non-finite width, or an unknown location. */
bool dr_selection_stroke(uint8_t *mask, uint32_t width, uint32_t height,
                         float width_px, DrStrokeLocation location);

/* Edit > Stroke... with nothing selected: the working memory
 * dr_doc_stroke_layer_edge reserves for layer idx at width_px, in bytes —
 * 24*|F| + 4*|layer rect U F| + 4096, saturating, F the layer's alpha > 0
 * bounds grown by ceil(width_px) + 2 on every side. 0 for a NULL doc, an
 * idx that is out of range or not raster, a width that is non-finite or
 * <= 0, or a layer with no alpha > 0 pixel (nothing to outline); UINT64_MAX
 * when the frame cannot be represented. */
uint64_t dr_doc_stroke_layer_edge_scratch(const DrDocument *doc, size_t idx,
                                          float width_px);

/* Edit > Stroke... with nothing selected: strokes layer idx's own alpha
 * edge (the mask excluded) width_px wide at `location` in LAYER space,
 * including any part beyond the canvas, with the straight RGBA8 colour
 * rgba (4 bytes) through `mode` at `opacity` (clamps to [0, 1]). The band
 * is dr_selection_stroke's, computed over the frame F above (the canvas-edge
 * contour never applies), and filled through dr_doc_fill_selection's path
 * laid out over F. The layer GROWS to its union with the band's bounds —
 * past the canvas if the band reaches there; mask placement is unchanged
 * and recorded layer-comp offsets follow the grown origin — unless
 * preserve_transparency or a transparency lock keeps its alpha, in which
 * case it never grows and a semi-transparent pixel takes the fill's
 * lerp(Cb, B(Cb, Cs), sa) rule. control (NULL or a fresh job control) bounds the
 * reservation and polls cancellation, once per row during the alpha-bounds
 * scan too; that scan is shared with dr_doc_stroke_layer_edge_scratch on the
 * same layer pixels, and a cancelled scan caches nothing.
 * NULL with *err_out untouched: a NULL doc, or no change (a layer with no
 * alpha > 0 pixel, an empty band, or a result byte-identical to the grown
 * input — opacity 0, say). NULL WITH a message: an unknown location, NULL
 * rgba, an unknown or Pass Through mode, an idx that is out of range or not
 * raster, a retained-source layer ("Rasterize the retained source before
 * stroking its pixels"), a pixel-locked layer, a width that is non-finite
 * or <= 0, NaN opacity, a scratch limit below
 * dr_doc_stroke_layer_edge_scratch, cancellation, or a result past native
 * capacity. Free *err_out with dr_string_free. */
DrDocument *dr_doc_stroke_layer_edge(const DrDocument *doc, size_t idx,
                                     float width_px, DrStrokeLocation location,
                                     const uint8_t *rgba, DrBlendMode mode,
                                     float opacity, bool preserve_transparency,
                                     const DrJobControl *control,
                                     char **err_out);

/* Dodges (brightens, burn false) or burns (darkens, burn true) layer idx
 * where a stroke overlay covers it. src is the SAME canvas-frame
 * premultiplied overlay dr_doc_painting_layer takes (w/h must equal the
 * canvas size); ONLY its alpha channel is read, as per-pixel stroke
 * coverage. Each color channel moves toward white (dodge) or black (burn),
 * banded by its own value — range 0 shadows, 1 midtones, 2 highlights —
 * and scaled by exposure (clamped to [0, 1]). protect_tones (#1) moves
 * the three channels TOGETHER instead: with V the pixel's largest channel,
 * every channel is scaled by curve(V) / V (V = 0 gives curve(0) on each), so
 * hue and saturation hold; false is the per-channel curve, byte for byte as
 * before. Layer alpha is never
 * touched; overlay outside the layer's extent is ignored (the layer does
 * NOT grow). NULL on dimension mismatch, non-finite exposure, range > 2,
 * out-of-range idx, a layer extent that misses the canvas, or when no
 * pixel would change. */
DrDocument *dr_doc_dodge_burn_layer(const DrDocument *doc, size_t idx,
                                    const uint8_t *src, uint32_t w, uint32_t h,
                                    float exposure, uint8_t range, bool burn,
                                    bool protect_tones);

/* Retouching: healing, inpainting and red-eye. */

/* WHICH pixels a retouch gesture samples. The mode is shared by every
 * retouch entry point that takes one, and by dr_doc_retouch_source below.
 *
 *   CURRENT          layer idx's OWN pixels on a transparent canvas,
 *                    deliberately raw: opacity, blend mode, mask and
 *                    visibility describe how the layer composites, not what
 *                    its pixels are, so they play no part. This is the
 *                    historic sample_all_layers = false.
 *   CURRENT_AND_BELOW the document's own projection with every entry ABOVE
 *                    idx contributing nothing. "Above" is positional and
 *                    excludes idx's own enclosing GROUPS, whose records sit
 *                    past the subtree they close and whose mask, opacity and
 *                    blend still describe how idx's level meets the picture.
 *   ALL              the document's own projection. This is the historic
 *                    sample_all_layers = true.
 *
 * ignore_adjustments additionally drops every ADJUSTMENT ENTRY above idx.
 * It is a provable identity in CURRENT (no composite runs) and in
 * CURRENT_AND_BELOW (those entries are already cut), because an adjustment
 * transforms only what is composited BELOW it and so can never reach idx's
 * own pixels; it is the meaningful control in ALL. The boundary is stated
 * rather than implied: only an adjustment ENTRY is dropped. A layer STYLE, a
 * Blend If, and a plain raster layer in a tone blend mode are ordinary
 * content and are NOT dropped. */
#define DR_RETOUCH_SAMPLE_CURRENT 0u
#define DR_RETOUCH_SAMPLE_CURRENT_AND_BELOW 1u
#define DR_RETOUCH_SAMPLE_ALL 2u

/* The canvas-sized straight-RGBA8 SAMPLING PLATE a retouch gesture reads
 * from, built from the document's own layers by the same compositor the
 * projection uses, in the document's own numbers, with no conversion
 * anywhere. It exists so that no host ever samples a DISPLAY image.
 *
 * control may be NULL. When it is not, the plate reserves its scratch
 * against that token before allocating and polls cancellation per tile; per
 * tile row through the regional tree (groups, styles: one row of <= 1024 root
 * samples through the whole tree); per row inside the full-frame fallback
 * (guided Shadows/Highlights).
 * NULL with *err_out NULL for a NULL document, an unknown sample code, or
 * (CURRENT only) an idx that names no raster layer; NULL WITH a message on a
 * rendering refusal, a reservation refusal or cancellation. Free the image
 * with dr_image_free and *err_out with dr_string_free. */
DrImage *dr_doc_retouch_source(const DrDocument *doc, size_t idx,
                               uint32_t sample, bool ignore_adjustments,
                               const DrJobControl *control, char **err_out);

/* Explicit source sampling maps destination canvas pixel centres through six
 * doubles [a,b,c,d,tx,ty]. Shared native premultiplied bilinear interpolation. */
DrImage *dr_image_transform_retouch_source(const DrImage *image, const double *matrix,
    const DrJobControl *control, char **err_out);
/* Storage union with canvas. Existing pixels and mask placement are exact;
 * the layer's recorded comp offsets follow its grown origin. */
DrDocument *dr_doc_expand_retouch_destination(const DrDocument *doc, size_t idx,
    const DrJobControl *control, char **err_out);

/* Poisson-blends the source patch an overlay carries into layer idx: the
 * source's TEXTURE with the destination's ILLUMINATION. src is the SAME
 * canvas-frame premultiplied overlay dr_doc_painting_layer takes (w/h must
 * equal the canvas size): its ALPHA is the footprint's coverage and its RGB
 * the already-aligned source pixels, so the Clone Stamp's overlay is
 * literally the healing brush's input. Covered pixels (alpha >= 128) that
 * have a covered-and-usable neighbourhood are solved; a covered pixel next
 * to an uncovered one keeps the destination, which is what makes the join
 * seamless — so a hard-edged footprint needs no feather, and coverage below
 * 128 is not written at all (hardness shrinks the footprint, it does not
 * fade the heal). The covered set is split into connected components, each
 * solved over its own bounding box. Straight colour, per channel, in the
 * document's own numbers; layer alpha is never touched and fully transparent
 * destination pixels are skipped. strength scales the write-back and is
 * clamped to [0, 1]. Overlay outside the layer's extent is ignored (the layer
 * does NOT grow). NULL with *err_out set when one region's bounding box
 * exceeds the documented memory limit, or when every region's box together
 * exceeds the documented working-area limit for one call — both are measured
 * BEFORE anything is healed, so a refusal costs a component walk and not a
 * heal; NULL with *err_out NULL on NULL args,
 * dimension mismatch, non-finite strength, out-of-range idx, an empty covered
 * set, a covered set with no interior to solve (every covered pixel is on the
 * join contour, where the heal IS the destination), a layer extent that
 * misses the canvas, or when no pixel would change. Free *err_out with
 * dr_string_free.
 *
 * control may be NULL. When it is not, the token is single-use (a consumed
 * one is refused with "job control has already been used"); the NULL-arg and
 * shape refusals above come first and leave it unclaimed. A cancel returns
 * NULL with "operation cancelled", never a partial document. Scratch is
 * reserved against the token's working-memory limit (before the plan, then
 * again sized to the measured windows before the layer read) and is 0 on
 * return. Stages: 1 once claimed, 2 when the plan is measured, 4 for each
 * component window (progress weighted by padded window area), then 1.0 at
 * stage 5 on return whatever the result. */
DrDocument *dr_doc_heal_layer(const DrDocument *doc, size_t idx,
                              const uint8_t *src, uint32_t w, uint32_t h,
                              float strength, const DrJobControl *control,
                              char **err_out);

/* Spot healing: PatchMatch-inpaints the overlay's footprint from a ring of
 * valid pixels around it, then Poisson-blends the result in exactly as
 * dr_doc_heal_layer does, hard cut at the alpha = 128 contour and all — so a
 * soft tip heals a SMALLER footprint, never a fainter one. Only the overlay's
 * ALPHA is read (its RGB is ignored — there is no sampled source). ring is
 * the sampling-ring width in
 * px, 0 = automatic, clamped to [21, 512] (the floor is three patch widths,
 * the narrowest band a source patch can move in) and then widened from below
 * by the footprint's own size; seed makes the result reproducible;
 * sample/ignore_adjustments choose the sampling plate (DR_RETOUCH_SAMPLE_*
 * above), and an unknown sample code is a plain NULL refusal. A pixel is a
 * valid SOURCE when it is outside every part of
 * the footprint and its alpha is at least 128: alpha here answers only
 * whether there is colour, and the RGB beside it is straight, so a
 * semi-transparent layer or an 80 %-opaque composite samples fine. NULL with
 * *err_out set when the footprint dilated by its ring, the working windows
 * or one region's window exceeds a documented cap (the first two bound the
 * whole CALL: every separate part of the footprint counts towards them
 * together), when the footprint is in more than 1,000,000 separate parts,
 * when one part whose window is too narrow to reduce holds more than
 * 1,000,000 pixels to search, or when there is nothing to sample from —
 * every cap is measured BEFORE any pixel is filled, so a refusal costs a
 * fraction of a fill, and every message is worded for the gesture that asked
 * (a stroke is not told to select less). The covered count itself is not
 * capped: over 1,000,000 covered pixels in the call the search runs
 * at 1/2 scale (1/4 over 4,000,000, 1/8 over 16,000,000), the finer levels
 * copy source pixels through the doubled field without searching, and the
 * dilated cap of 4,000,000 is 4 times larger per halving.
 * NULL with *err_out NULL when nothing would change. Free with
 * dr_string_free.
 *
 * spot_type is the Spot Healing Brush's Type: 0 Content-Aware
 * (the PatchMatch inpaint above), 1 Create Texture (blocks of the stroke's
 * own pixels, less the plane trend of its 4 px band of surroundings, tiled
 * over it from seed; a pixel whose residual leaves the band's own range by
 * more than an eighth of it — the blemish — is never tiled back, and with no
 * pixel left the trend alone fills; a second stroke rebuilds from the
 * first's result; NULL and "Create Texture has nothing to build a texture
 * from: ..." when every pixel under the stroke is transparent in the sampled
 * picture), 2 Proximity Match (the region copied whole from the offset whose
 * 4 px band best matches the stroke's own, its search ring 2*base+14 capped
 * at 512, base being the automatic or requested ring after its floors (21 px
 * and half the hole's radius); NULL and "Proximity Match found no clean
 * patch beside the stroke as large as the stroke itself; ..." when none
 * fits). Each replaces only the synthesis; the caps and the healing blend are
 * the same.
 *
 * The app's one spot-heal entry point. A NULL doc or src answers NULL (no
 * message) before control is read; a NULL control is then refused with "job
 * control is NULL". The token is claimed next — before the remaining shape
 * refusals (canvas-size mismatch, an unknown spot_type or sample code), which
 * consume it —
 * and is single-use ("job control has already been used"). A cancel returns
 * NULL and "operation cancelled", never a partial document. Scratch is 0 on
 * return, and a completed run ends at 1.0 / stage 5. */
DrDocument *dr_doc_spot_heal_layer_job(const DrDocument *doc, size_t idx,
                                       const uint8_t *src, uint32_t w,
                                       uint32_t h, float strength,
                                       uint32_t ring, uint64_t seed,
                                       uint32_t spot_type, uint32_t sample,
                                       bool ignore_adjustments,
                                       const DrJobControl *control,
                                       char **err_out);

/* Removes flash red inside a canvas rect on layer idx. Inside the rect,
 * pixels are scored by red DOMINANCE — the ratio R/((G+B)/2) gated by HSV
 * saturation and hue, not a naive R > G, which every skin tone passes —
 * and a scoring component is corrected only when its larger side fits within
 * pupil_size times the rect's SHORTER side AND it does not reach all four
 * sides of the rect, so a red object caught by a sloppy rectangle is left
 * alone and a rect dropped wholly inside one red region is refused rather
 * than desaturated into a hard-edged square. pupil_size is a fraction in
 * (0, 1] and 1.0 is the intended default: it exists to reject something
 * bigger than the eye, not to require a small one — the four-sides rule is
 * what gives it teeth on a square rect, where a clipped component can never
 * be larger than the rect. Red drops to (G+B)/2 and the pixel darkens
 * by darken (0.5 the default); a specular catchlight scores zero
 * and comes out bit-identical. Layer alpha is never touched. NULL on an
 * empty or off-canvas rect, out-of-range idx, non-finite parameters, nothing
 * red at all, no component within the pupil-size limit, or when no pixel
 * would change. */
DrDocument *dr_doc_red_eye_layer(const DrDocument *doc, size_t idx,
                                 int32_t x, int32_t y, uint32_t w, uint32_t h,
                                 float pupil_size, float darken);

/* Retouching onto a NEW layer instead of into the photograph. Each of these
 * inserts a transparent CANVAS-SIZED raster layer directly above idx — at
 * idx's own depth, inside the same group, carrying idx's clipped flag so an
 * unclipped insert cannot end the target's clip run — and puts the repair on
 * THAT, leaving idx's pixels exactly as they were. One pure transition, so
 * one Undo.
 *
 * All three answer NULL WITH a message when idx sits inside a group with its
 * own blending, opacity, mask or style: a repair on a new layer inside such a
 * group would be gated twice, once in the sampled picture and again by the
 * group. They answer NULL with *err_out NULL for NULL/short buffers, a w/h
 * that is not the canvas size, an out-of-range or GROUP idx, a PIXEL-LOCKED
 * target (the lock belongs to the photograph, not to the new layer), growth
 * past native capacity, or a gesture that deposits nothing.
 *
 * ALPHA is the one place this family authors coverage. Every other retouch op
 * promises that layer alpha never moves; a retouch layer is transparent until
 * the repair is put on it, so the deposit writes alpha, and writes the
 * PLATE's own alpha at that pixel — the coverage the picture already had
 * there. Where the picture is fully opaque, which is every photograph, the
 * composite is the in-place result byte for byte. Where it is not, source
 * over necessarily raises the composite's alpha from a to a*(2 - a): no
 * deposited alpha can reproduce an in-place RGB rewrite under a partial
 * backdrop.
 *
 * name may be NULL for the default ("Retouch"). */
DrDocument *dr_doc_clone_to_new_layer(const DrDocument *doc, size_t idx,
                                      const uint8_t *src, uint32_t w,
                                      uint32_t h, float alpha,
                                      const char *name, char **err_out);

/* src is dr_doc_heal_layer's overlay, unchanged in meaning. plate is the
 * canvas-sized STRAIGHT RGBA8 CURRENT_AND_BELOW plate from
 * dr_doc_retouch_source, and it must be the picture the new layer will
 * composite onto: the membrane matches ITS illumination, and a plate that is
 * not that backdrop shows as a full-amplitude outline in the shape of the
 * stroke. Every cap, contour rule and refusal is dr_doc_heal_layer's, because
 * that is literally what runs — against a one-layer temporary document made
 * from the plate, at the caller's own strength, after which the healed pixels
 * are lifted onto the new layer. control as dr_doc_heal_layer's: may be NULL,
 * single-use, left unclaimed by the NULL/shape refusals (plate included), a
 * cancel is NULL with "operation cancelled" and no partial document, scratch
 * is reserved against its limit and 0 on return, stages 1, 2, 4 per window,
 * then 1.0 / 5. The reservation includes the temporary document's copy of the
 * plate: w*h*8 while it is made, then w*h*4 on top of every heal reservation. */
DrDocument *dr_doc_heal_to_new_layer(const DrDocument *doc, size_t idx,
                                     const uint8_t *src, uint32_t w,
                                     uint32_t h, float strength,
                                     const uint8_t *plate, const char *name,
                                     const DrJobControl *control,
                                     char **err_out);

/* As dr_doc_heal_to_new_layer around dr_doc_spot_heal_layer_job: src's ALPHA
 * is the footprint and its RGB carries no meaning, and the plate is BOTH what
 * the inpaint samples and the illumination it matches — on a layer that is
 * empty there is nothing else to read. ring/seed/spot_type as
 * dr_doc_spot_heal_layer_job (an unknown spot_type: NULL, no message);
 * control may be NULL and is polled by the inpaint exactly as the plain job
 * twin's is, so a cancelled call returns NULL and "operation cancelled"; its
 * reservations include the plate copy as dr_doc_heal_to_new_layer's, and it
 * closes with a last checkpoint and 1.0 / 5 as that export does. */
DrDocument *dr_doc_spot_heal_to_new_layer(const DrDocument *doc, size_t idx,
                                          const uint8_t *src, uint32_t w,
                                          uint32_t h, float strength,
                                          uint32_t ring, uint64_t seed,
                                          uint32_t spot_type,
                                          const uint8_t *plate,
                                          const char *name,
                                          const DrJobControl *control,
                                          char **err_out);

/* Whole-document exact geometry: every layer's pixels and offset transform
 * together with the canvas. A valid content-identical result returns NULL;
 * callers treat that as a successful no-op (no history or dirty change). */
DrDocument *dr_doc_rotate90(const DrDocument *doc);
DrDocument *dr_doc_rotate180(const DrDocument *doc);
DrDocument *dr_doc_rotate270(const DrDocument *doc);
DrDocument *dr_doc_flip_horizontal(const DrDocument *doc);
DrDocument *dr_doc_flip_vertical(const DrDocument *doc);

/* Crop moves the canvas window: canvas becomes w*h, layer offsets shift by
 * (-x, -y), layer pixels are untouched (content outside the canvas is
 * retained and can be revealed by moving layers). Bounds-checked like
 * dr_image_crop against the CANVAS rect. */
DrDocument *dr_doc_crop(const DrDocument *doc, uint32_t x, uint32_t y,
                        uint32_t w, uint32_t h);

/* DELETE CROPPED PIXELS, the Crop tool's option, applied after
 * dr_doc_crop or a byte straighten: every eligible raster layer (raster, no
 * meta, no retained source or warp) is trimmed to its intersection with the
 * canvas; one wholly outside it becomes a 1x1 transparent pixel at (0, 0).
 * Masks, groups, adjustment and described layers, channels, guides and
 * samplers are untouched; lock-exempt like the crop. A layer comp entry bound
 * to a trimmed layer that recorded an offset shifts by the layer's own origin
 * delta. NULL on NULL doc, and when no eligible layer reaches outside the
 * canvas (nothing would change). */
DrDocument *dr_doc_delete_cropped_pixels(const DrDocument *doc);

/* Scales the canvas and every layer (sizes and offsets) proportionally.
 * Limits as dr_image_resize. Layer styles scale with their layers ("Scale
 * Effects", by the mean factor sqrt(fx * fy); see "Layer styles" below).
 * Alpha channels resample to the new canvas with it, so this is also NULL
 * when the enlarged channel list would break the .rz total-channel-pixel
 * budget (see "Channels") — the refusal lands here, where the user can
 * delete channels, rather than at save time. */
DrDocument *dr_doc_resize(const DrDocument *doc, uint32_t w, uint32_t h,
                          DrResizeFilter filter);

/* Changes the canvas size WITHOUT scaling anything: the canvas becomes w*h
 * and every layer's offset shifts by (origin_x, origin_y) — where the old
 * canvas's top-left corner lands in the new canvas. Layer pixels are
 * untouched; content outside the new canvas is retained (as with
 * dr_doc_crop) and can be revealed later. Alpha channels are canvas-sized,
 * so they are padded with 0 to the new canvas; growing past the .rz
 * total-channel-pixel budget is refused here for the same reason
 * dr_doc_resize refuses it. NULL if w == 0, h == 0, w*h > 100000000, that
 * budget would break, or nothing would change (same size at origin (0,0));
 * the host answers that request as a successful no-op before calling. */
DrDocument *dr_doc_canvas_resize(const DrDocument *doc, uint32_t w,
                                 uint32_t h, int32_t origin_x,
                                 int32_t origin_y);

/* --- Arbitrary canvas rotation (Phase 12F-1b3, BYTE documents) ------------
 *
 * Destructive whole-canvas rotation by ANY finite angle, with an EXPANDING
 * canvas. Positive degrees turn the picture CLOCKWISE on the y-down canvas;
 * the linear part is [cos, sin, -sin, cos], which is exactly
 * CGAffineTransform(rotationAngle:) and exactly dr_doc_rotate90's
 * [0, 1, -1, 0].
 *
 * THE CORE CLASSIFIES; THE HOST DOES NOT. dr_canvas_rotation_plan is the one
 * place an angle is normalized and a rotated canvas is sized, and it takes no
 * document handle, so a sheet with nothing committed, an MCP handler before
 * its undo group and an Action's recorded-canvas bookkeeping all read the
 * same rule.
 *
 * The angle is normalized into (-180, 180] first: -180 becomes 180, -0
 * becomes 0. Then:
 *
 *   - a whole number of full turns is the IDENTITY: nothing moves, and
 *     dr_doc_geometry_edit's "rotate_canvas" request answers NULL with NO
 *     error;
 *   - an EXACT quarter turn (within 1e-9 of the exact linear part, so
 *     |dTheta| is about 5.7e-8 degrees) is DELEGATED to dr_doc_rotate90 /
 *     180 / 270 VERBATIM — pixels, masks, channels, offsets, canvas, guides,
 *     ruler origin and the TRANSPOSED ppi. That is not an optimisation:
 *     applying the expanded-box rule to an exact quarter turn of a canvas
 *     whose W + H is odd gives dr_doc_rotate90's 48 x 63 for 63 x 48 but
 *     places the picture half a pixel off (matrix tx, ty 47.5, -0.5 against
 *     48, 0), i.e. a half-pixel resample of a lossless permutation;
 *   - every other angle is FILTERED: the canvas is the expanded box
 *     W' = ceil(W*|cos| + H*|sin|) x H' = ceil(W*|sin| + H*|cos|), the
 *     extent of the four canvas corners rotated about (W/2, H/2), and the
 *     old canvas's top-left lands at the integer
 *     (floor((W'-W)/2), floor((H'-H)/2)). W' < W is ordinary rather than an
 *     error (100x10 at 80 degrees is 28x101, with left = +36).
 *
 * The DISCONTINUITY at a quarter turn is deliberate and disclosed: 90 degrees
 * permutes the guides and transposes the ppi, while 89.999 degrees shifts and
 * DROPS guides (dr_doc_canvas_resize's padded-guide rule) and leaves the ppi
 * alone. They are two different operations.
 *
 * `out_degrees` receives the normalized angle (untouched when the angle is
 * not finite). `out_rect` receives left, top, width, height of the NEW canvas
 * in OLD-canvas coordinates: (0, 0, W, H) for identity and 180, (0, 0, H, W)
 * for the two quarter turns, the expanded box when filtered, and ALL ZEROS
 * when that box is refused (a zero side, outside the int32 offset range, or
 * past the 100000000-pixel budget). Either out pointer may be NULL. */
#define DR_ROTATION_REFUSED 0   /* degrees not finite */
#define DR_ROTATION_IDENTITY 1
#define DR_ROTATION_90 2
#define DR_ROTATION_180 3
#define DR_ROTATION_270 4
#define DR_ROTATION_FILTERED 5
int32_t dr_canvas_rotation_plan(uint32_t width, uint32_t height,
                                double degrees, double *out_degrees,
                                int32_t *out_rect);

/* Rotating the whole BYTE document is dr_doc_geometry_edit's request
 *   {"op":"rotate_canvas","degrees":D,"filter":F}   both REQUIRED,
 * F one of "nearest", "bilinear", "bicubic", "lanczos3" (it runs on the
 * host's bounded worker under a job control, polled per layer and then per
 * channel, and a cancel answers "operation cancelled" with no document). It
 * grows the canvas to hold the turned picture.
 *
 * A FILTERED angle resamples every layer and EVERY mask — linked, unlinked,
 * on a group, on an adjustment — through the byte "straighten" request's own
 * kernel under the plan's matrix, and resamples every saved CHANNEL STRAIGHT
 * from the old canvas into (0, 0, W', H') with that matrix's inverse and 0
 * outside, so a channel stays canvas-sized and no coverage that lands inside
 * the new canvas is lost even when the canvas shrinks on an axis. The guides
 * and the ruler origin ride the padded-guide rule by (floor((W'-W)/2),
 * floor((H'-H)/2)); the ppi is NOT transposed. The op is lock-EXEMPT, like every other
 * whole-canvas op: a Lock All layer rotates with the picture. Layer ids,
 * links, labels, blend modes, opacity, clipping, layer styles (the scale
 * factor is exactly 1) and the evaluation policy all ride unchanged; layer
 * comps ride with each bound raster entry's recorded offset remapped (see
 * LAYER COMPS, GEOMETRY), and there is no .rz format change.
 *
 * NULL with NO message is the IDENTITY answer. NULL WITH a message is a
 * refusal: a non-finite angle or an unknown filter name is refused by the
 * request parser ("invalid typed geometry request: ..."); "the rotated canvas
 * exceeds supported bounds"; "the rotated canvas would exceed the
 * saved-channel pixel budget"; "a layer's rotated extent is empty, outside
 * the i32 offset range or past the pixel limit"; and the working-memory
 * admission sentence. `filter` is UNREAD on the identity and quarter-turn
 * routes.
 *
 * The typed twin is dr_precision_doc_edit's "rotate_canvas" (Phase 12F-1b4,
 * below). */

/* Free transform of ONE layer by an arbitrary affine matrix.
 *
 * `affine` points to exactly six doubles in CGAffineTransform element order,
 * [a, b, c, d, tx, ty], mapping
 *     (x, y) -> (a*x + c*y + tx, b*x + d*y + ty)
 * so a CGAffineTransform's fields can be handed over unchanged.
 *
 * The matrix works in CANVAS coordinates: the layer occupies the canvas rect
 * (offset_x, offset_y, layer_width, layer_height), the matrix says where that
 * rect lands, and the layer's NEW offset and size are the axis-aligned
 * bounding box of the four transformed corners, rounded OUTWARD (floor of the
 * minima, ceil of the maxima) so nothing is clipped — after each corner within
 * 1e-9 of an integer is snapped onto it, so the floating-point residue of a
 * composed matrix (cos(PI/2) is 6e-17, not 0) cannot add a phantom transparent
 * row or column. Corners that are fractional by any user-meaningful amount are
 * untouched and still round outward. The canvas itself is not touched, and
 * neither is any other layer; a transformed layer may extend past the canvas,
 * exactly like any other offset layer.
 *
 * Pixels are inverse-mapped: every DESTINATION pixel centre is mapped back
 * through the inverse matrix and sampled in the source with `sampler` (the
 * shared DrResizeFilter values: NEAREST point-samples, BILINEAR is a 2x2
 * triangle filter, CATMULL_ROM a 4x4 bicubic, LANCZOS3 a 6x6 windowed sinc).
 * Samples landing outside the source pixels are fully transparent, so edges
 * fade out rather than smear the border. Interpolation runs in PREMULTIPLIED
 * f32 and is unpremultiplied on the way out, so anti-aliased edges keep their
 * color instead of fringing toward the (meaningless) color of transparent
 * neighbours.
 *
 * A layer MASK is carried through identically: it is resampled with the same
 * matrix, the same destination extent and the same sampler, so it lands
 * pixel-for-pixel on the transformed pixels and stays exactly the layer's
 * size. Layer `meta` is PRESERVED — the core never interprets it, so deciding
 * whether a transform invalidates a text description is the host's policy.
 * Name, opacity, blend mode, visibility and the mask-enabled flag survive too.
 *
 * Exact matrices (an integer translation, optionally composed with an
 * axis-aligned flip or a 90-degree multiple) skip resampling and copy pixels
 * losslessly, matching dr_doc_rotate90 / dr_doc_flip_* byte for byte. Exact
 * means "within 1e-9": a caller that composes its quarter turn from an angle
 * (rotated(by: M_PI_2), whose cosine is 6e-17 rather than 0) gets the same
 * lossless copy and the same extent as one passing an integer matrix. A
 * transform that misses by more than that — 89.999 degrees is 1.7e-5 off —
 * resamples normally. The dedicated whole-document ops above remain the right
 * call for menu items.
 *
 * A layer style is scaled with the layer ("Scale Effects": its pixel-valued
 * fields — distance, size, soften — follow the mean scale sqrt(|a*d - b*c|);
 * see "Layer styles" below).
 *
 * NULL if doc or affine is NULL, idx is out of range, any matrix element is
 * not finite, the matrix is singular (|a*d - b*c| < 1e-9), the sampler value
 * is unknown, or the destination extent is empty, falls outside the int32
 * offset range, or exceeds 100000000 pixels. */
DrDocument *dr_doc_transform_layer(const DrDocument *doc, size_t idx,
                                   const double *affine,
                                   DrResizeFilter sampler);

/* Perspective (projective) transform of one layer: maps the layer's rect
 * corner-for-corner onto a destination QUAD. `quad` points to eight doubles
 * — the canvas coordinates the source rect's corners land on, in the
 * source rect's own corner order top-left, top-right, bottom-right,
 * bottom-left (x then y for each, clockwise on the y-down canvas). The
 * homography realizing the mapping is solved inside the core, so no 3x3
 * element-order convention crosses this boundary.
 *
 * Everything dr_doc_transform_layer documents carries over: the new
 * offset/size are the quad corners' outward-rounded bounding box (with the
 * same 1e-9 integer snapping), pixels are inverse-mapped and interpolated in
 * premultiplied f32 with the same samplers, samples outside the source are
 * transparent, a linked mask follows the same map/kernel at its own extent, and
 * meta plus every other layer property survive. Extent pixels outside the
 * quad itself come out fully transparent. Perspective compresses sampling
 * density toward the quad's narrow side, so some aliasing there under
 * NEAREST/BILINEAR is inherent, not a defect.
 *
 * A quad that is a PARALLELOGRAM within 1e-9 canvas px (opposite sides
 * equal; the fourth corner implied by the other three) is an affine
 * transform and is delegated to dr_doc_transform_layer — an integer
 * translation handed over as corners is still a lossless pixel copy.
 *
 * NULL if doc or quad is NULL, idx is out of range, any coordinate is not
 * finite, the quad is concave or self-intersecting (the mapping would fold
 * through the horizon inside the layer; a corner's homogeneous w must stay
 * >= 1e-6 with w = 1 pinned at the rect's top-left), the quad collapses
 * toward zero area, the sampler value is unknown, or the destination extent
 * is empty, falls outside the int32 offset range, or exceeds 100000000
 * pixels. */
DrDocument *dr_doc_perspective_layer(const DrDocument *doc, size_t idx,
                                     const double *quad,
                                     DrResizeFilter sampler);

/* ------------------------------------------------------------------------ */
/* Additional filters (pure DrImage operations, NULL on invalid args)       */
/* ------------------------------------------------------------------------ */

/* Rotates hue by degrees (any finite value; NULL on NaN). Alpha untouched. */
DrImage *dr_image_hue_rotate(const DrImage *img, float degrees);

/* Luma threshold at level in [0,1]: output pixels become opaque-preserving
 * black or white by Rec.709 luma; alpha untouched. */
DrImage *dr_image_threshold(const DrImage *img, float level);

/* Reduces each color channel to `levels` values (2..=64) by the rule
 * measured against Photoshop: bin k = floor(c*levels/256) of the code c,
 * output code floor(255*k/(levels-1)). Alpha untouched. */
DrImage *dr_image_posterize(const DrImage *img, uint32_t levels);

/* Image > Adjustments > Equalize (#1): ONE table from the summed R, G and B
 * histogram at 256 levels, lut[v] = round(255 (cdf(v) - cdf(v_min)) /
 * (N - cdf(v_min))) with v_min the darkest occupied level and N the total
 * weight, applied to every pixel's RGB; alpha is never written and alpha-0
 * pixels are skipped. Each sample's histogram weight is mask/255 * alpha/255.
 * mask = histogram weight only, image-sized (w*h; the caller crops a canvas
 * selection to the layer's placement); NULL = unweighted. The result covers
 * the whole image: the kernel never lerps, the caller writes through the
 * selection. NULL with no error = nothing changes (fewer than two occupied
 * levels included); NULL with a message for a NULL image or a mask of
 * another length. The typed documents run the selected edit
 * {"op":"equalize","params":{"scope":"selected"|"entire"}}. */
DrImage *dr_image_equalize(const DrImage *image, const uint8_t *mask, size_t mask_len,
                           char **err_out);

/* Mosaic: block*block cells (1..=1024) averaged (alpha-weighted). */
DrImage *dr_image_pixelate(const DrImage *img, uint32_t block);

/* Additive uniform noise, amount in (0,1]; deterministic for a given seed.
 * Alpha untouched. */
DrImage *dr_image_noise(const DrImage *img, float amount, uint64_t seed);

/* Sobel edge magnitude on luma, rendered as an opaque grayscale image. */
DrImage *dr_image_edge_detect(const DrImage *img);

/* Emboss: 3x3 directional relief kernel on luma around mid-gray, opaque. */
DrImage *dr_image_emboss(const DrImage *img);

/* The byte twin of the 16-bit selected filters of the same names, through one
 * core dispatch, twenty ops: Box Blur, Surface Blur, Add Grain, Radial Blur,
 * Clouds, Difference Clouds, Offset, Motion Blur, Lens Blur, Tilt-Shift,
 * Lens Correction, Presence, Develop Filter and Smart Sharpen (one shared
 * spatial kernel), the
 * destructive photographic sharpening and noise cleanup, the legacy
 * Gaussian Blur, Liquify and Depth Blur. `op`
 * is exactly "blur",
 * "box_blur", "surface_blur",
 * "add_grain", "radial_blur", "clouds", "difference_clouds", "offset",
 * "motion_blur", "lens_blur", "tilt_shift", "lens_correction", "presence",
 * "develop_filter", "smart_sharpen", "liquify", "depth_blur", "unsharp_mask",
 * "high_pass", "dust_and_scratches" or "reduce_noise";
 * anything else errors ("gaussian_blur"
 * included: Gaussian Blur is "blur"). `params_json` (NULL means {}, at
 * most 64 KiB, or 8 MiB for "liquify", whose largest admitted stroke list is
 * 6,881,280 bytes, and for "depth_blur") is a JSON object of that op's keys:
 *   blur          sigma (above 0, at most 1000, default 4) — Gaussian Blur's
 *                 Radius: from 7 up the extended-box kernel (4
 *                 extended boxes at sigma 0.99 * radius, measured against
 *                 Photoshop 26.10.0); below 7 the legacy implementation
 *                 dr_image_sharpen runs too (the image crate's kernel below
 *                 4, the radius-independent filter from 4). Premultiplied,
 *                 one 128-row band at a time under the control. Filters alpha.
 *                 Never a bypass, and it has no work ceiling, controlled or
 *                 not. Refusals: "unexpected blur parameter <key>", "sigma
 *                 must be numeric", "sigma must be finite in (0,1000]".
 *   box_blur      radius (integer 1-1000, default 5) — the premultiplied mean
 *                 of a (2r+1)^2 clamped-edge window; filters alpha, as
 *                 Gaussian Blur does.
 *   surface_blur  radius (integer 1-100, default 5), threshold (8-bit LEVELS
 *                 2-255, default 15) — per channel, neighbours weighted by
 *                 max(0, 1 - |dv| / (2.5 * threshold)) with dv in levels, so
 *                 a step of 2.5x the threshold is left alone. Colour only.
 *   add_grain     amount (0-100, default 25; RMS 0.001 per unit at mid-grey),
 *                 size (Gaussian sigma 0.5-4 px, default 1), roughness (0-100,
 *                 default 50), seed (64-bit integer, default 1) — positional,
 *                 weighted by 4Y(1-Y) so black and white stay put. Colour only.
 *   radial_blur   amount (integer 1-100, default 10), method ("spin" or
 *                 "zoom", default spin), quality ("draft", "good" or "best",
 *                 default good), center_x and center_y (0-1, default 0.5:
 *                 fractions of the frame, top-left origin) — the equal-weight
 *                 premultiplied mean along each pixel's arc (spin: amount
 *                 degrees in total) or ray (zoom: amount * 0.5% of the
 *                 radius). Good is bilinear at one sample per pixel of path
 *                 (at most 65), best at two (at most 129), draft nearest at a
 *                 quarter (at most 17) with a deterministic positional jitter.
 *                 Filters alpha.
 *   clouds        seed (64-bit integer, default 1), foreground_color and
 *                 background_color ("#rrggbb", default "#000000" and
 *                 "#ffffff"), high_contrast (true/false, default false) —
 *                 positional fractal value noise mixed fg + (bg - fg) * t on
 *                 unit bytes; writes opaque pixels.
 *   difference_clouds  the same keys — |source - clouds| on the colour;
 *                 alpha exact, alpha-0 pixels untouched.
 *   offset        horizontal and vertical (integers, +/-100000000, default 0;
 *                 positive moves content right and down), undefined_areas
 *                 ("transparent", "repeat", "wrap" or "fill", default wrap),
 *                 fill (0-1, required with "fill" and refused otherwise) —
 *                 a vacated pixel is [0,0,0,0], the frame's nearest edge
 *                 pixel, the pixel modulo the frame, or [fill,fill,fill,1].
 *                 Samples move exactly.
 *   radial_blur and offset also take region_x, region_y, region_width and
 *                 region_height (integers, all four or none): the FRAME, the
 *                 selection's bounding box in the image's pixels, which the
 *                 offset shifts and wraps within and the centre is relative
 *                 to; pixels outside it are copied. A region that is empty or
 *                 not inside the image errors. Without one the frame is the
 *                 whole image.
 *   motion_blur   angle (degrees -360-360, default 0; positive is
 *                 counterclockwise), distance (integer 1-2000 px, default 10) —
 *                 the equal-weight premultiplied mean of distance + 1 bilinear
 *                 samples one pixel apart along the line through the pixel,
 *                 centred on it, clamped edges. Filters alpha.
 *   lens_blur     radius (integer 1-100, default 15), brightness (0-100,
 *                 default 0), threshold (8-bit LEVELS 0-255, default 255) — the
 *                 premultiplied mean over the disc dx*dx + dy*dy <= (r + 1/2)^2,
 *                 clamped edges, each pixel weighted 1 + brightness/100 * 8 *
 *                 clamp((Y - t) / max(1 - t, 1/255), 0, 1) with Y its
 *                 premultiplied luma and t = threshold/255, so highlights above
 *                 the threshold spread as discs. Filters alpha.
 *   tilt_shift    radius (integer 1-100, default 15), center_x and center_y
 *                 (0-1, default 0.5: fractions of the image), angle (degrees
 *                 -90-90, default 0 = a horizontal focus line; positive is
 *                 counterclockwise), focus and transition (0-1, defaults 0.15
 *                 and 0.25: fractions of the image height) — the unweighted
 *                 disc mean at radius * clamp((d - focus*H) / (transition*H),
 *                 0, 1), d the pixel's distance from the line through the
 *                 centre; a fractional radius mixes the two whole discs around
 *                 it, and transition 0 is a step. Filters alpha.
 *   lens_correction  distortion, chromatic_red_cyan, chromatic_blue_yellow,
 *                 vignette_amount, vertical_perspective and
 *                 horizontal_perspective (-100-100, default 0),
 *                 vignette_midpoint (0-100, default 50), angle (degrees
 *                 -180-180, default 0; positive turns the picture
 *                 counterclockwise), scale (percent 50-150, default 100). An
 *                 inverse map, for the output pixel centre p, c the image
 *                 centre and R half the longer side: n = (p - c) / R, divided
 *                 by scale/100, turned by the angle, divided by 1 +
 *                 0.004*(horizontal*n.x + vertical*n.y) (outside the picture
 *                 at or below 0; negative vertical widens the top, negative
 *                 horizontal the left), then the radial model RAW
 *                 development uses, k1 = -0.0008 * distortion (positive
 *                 corrects barrel), the red and blue sources scaled by 1 +
 *                 0.00002 * chromatic_red_cyan / chromatic_blue_yellow. A
 *                 point past the radial map's fold radius is outside the
 *                 picture. Catmull-Rom, premultiplied; the edge is
 *                 transparent. Then a vignette of 0.02 * vignette_amount
 *                 stops at the farthest corner (positive brightens),
 *                 falling off as fraction^(4^((midpoint - 50)/50)), applied
 *                 as an sRGB light-linear gain. Moves alpha with the colour.
 *                 All defaults are the exact bypass. profile (optional): a
 *                 resolved Lensfun profile object, dr_lens_match's
 *                 "profile" (strict; unknown keys, out-of-range terms and a
 *                 profile whose distortion, tca and vignetting are all null
 *                 are refused by name). It is composed after the sliders:
 *                 per channel the source is T_c(D(manual_c(p))), the
 *                 profile's distortion (poly3, poly5, ptlens) then its TCA
 *                 (linear, poly3) on the distorted radius, in PanoTools
 *                 units scaled by the crop factors and aspects; a point
 *                 past the distortion's fold is outside the picture. Its pa
 *                 vignetting divides out the measured falloff at the green
 *                 source point (at most 4 stops), added to the slider's
 *                 stops as one gain: 2^stops on a float layer, the image
 *                 profile's own linear basis on a 16-bit layer (its encoder
 *                 tables are in the plan), the sRGB gain at 8 bits. A
 *                 profile whose every model is the identity is the bypass.
 *   presence      texture, clarity and dehaze (-100-100, default 0; 0 is
 *                 off). Each base B_r is the edge-aware (guided-filter)
 *                 smooth of the Rec. 709 luminance Y at box half-width r, and
 *                 d_r = B_r - Y. With u = max(w, h) / 2048 the half-widths
 *                 are fine = max(1, round(u)), mid = max(fine + 1,
 *                 round(6u)), coarse = max(mid + 1, round(40u)) and haze =
 *                 max(2, round(24u)), each at most 192. Texture adds
 *                 texture/100 * (d_fine - d_mid) and Clarity adds clarity/100
 *                 * 4Y(1 - Y) * (d_mid - d_coarse) to R, G and B alike,
 *                 clamped to 0-1; a negative amount subtracts the same band.
 *                 Dehaze runs first, on the stored samples: the airlight A is
 *                 the mean colour of the visible pixels whose min(R, G, B) is
 *                 in the top 0.1%, the veil is min(B, W) of W = min over
 *                 channels of min(1, I / max(A, 0.05)) and its edge-aware
 *                 smooth B at the haze half-width, and a positive amount
 *                 answers clamp((I - A) / t + A, 0, 1) with t = max(1 - 0.95 *
 *                 dehaze/100 * veil, 0.1); a negative amount answers I + (A -
 *                 I) * 0.6 * |dehaze|/100. A picture of one colour is
 *                 unchanged at every amount. Colour only; no frame. All three
 *                 at 0 are the exact bypass. Work refusal (NULL `control`
 *                 only): "presence exceeds the bounded pixel-work limit".
 *   develop_filter temperature, tint, exposure (stops -4-4), contrast,
 *                 highlights, shadows, whites, blacks, texture, clarity,
 *                 dehaze, vibrance and saturation (-100-100 except exposure;
 *                 default 0; 0 is off) — the Develop panel's global controls
 *                 on straight RGB in display terms, each stage skipped when
 *                 its controls are 0, in this order: (1) white balance
 *                 through white_balance with temperature 6504 K at t = 0,
 *                 else 1e6 / (1e6/6504 - t) (one mired a unit; + warms) and
 *                 tint as given (+ toward magenta); (2) exposure, the
 *                 linear-light gain 2^exposure; (3) contrast, the modern
 *                 bcs S-curve at power 2^(1.5 * contrast/100); (4)
 *                 highlights and shadows, shadows_highlights at tone 0.5,
 *                 colour 0, amounts a_s = shadows/100 (shadows/200 below 0)
 *                 and a_h = -highlights/100 (-highlights/200 above 0), its
 *                 guide the alpha-weighted blurred luma of stage 3's output
 *                 at radius 30 * max(w, h)/2048 (1-1000); (5) whites and
 *                 blacks, a levels master stage: whites >= 0 sets white = 1 -
 *                 0.2 * whites/100, below 0 output_white = 1 + 0.2 *
 *                 whites/100; blacks <= 0 sets black = -0.2 * blacks/100,
 *                 above 0 output_black = 0.2 * blacks/100; (6) texture,
 *                 clarity and dehaze, exactly presence above over the toned
 *                 picture; (7) vibrance and saturation, vibrance at
 *                 vibrance/100 and saturation/100. Not re-editable: a second
 *                 run starts from its pixels. Colour only; no frame. All
 *                 thirteen at 0 are the exact bypass. Work refusal (NULL
 *                 `control` only): "develop_filter exceeds the bounded pixel-work
 *                 limit".
 *   smart_sharpen amount (percent 1-500, default 200), radius (px 0.1-64,
 *                 default 1), noise (percent 0-100, default 10), remove
 *                 ("gaussian", "lens" or "motion", default lens), angle
 *                 (degrees -180-180, default 0; read only by motion),
 *                 shadow_fade and highlight_fade (0-100, default 0),
 *                 shadow_tonal_width and highlight_tonal_width (0-100,
 *                 default 50), shadow_radius and highlight_radius (integer
 *                 px 1-100, default 1). The base D is the source, or at
 *                 noise > 0 reduce_noise's luminance half at radius 0.5,
 *                 luminance noise/1000, detail 0, color 0. The blur to
 *                 remove is a low-pass B of D: gaussian the shared Gaussian
 *                 at sigma radius; lens the lens_blur disc mean at radius
 *                 (a fraction blends the two whole discs around it); motion
 *                 the motion_blur line mean at angle over max(1,
 *                 round(2 * radius)) px. Each colour is D + a * (D - B)
 *                 clamped to 0-1, a = amount/100 * w, w = (1 - fs * s) * (1 -
 *                 fh * h), f = fade/100, s = clamp(1 - Ls/Ts, 0, 1), h =
 *                 clamp(1 - (1 - Lh)/Th, 0, 1), T = tonal_width/100 (a term
 *                 with f or T 0 is 1), L the alpha-weighted box mean of D's
 *                 Rec. 709 luma (clamped 0-1) at the section's radius. lens
 *                 then holds each colour within 2/255 of the least and
 *                 greatest of D over the 3x3 window's visible pixels, which
 *                 bounds the halo. gaussian with noise 0 and both fades 0 is
 *                 unsharp_mask at amount/100, the same radius and threshold 0,
 *                 bit for bit. Colour only; no frame; never a bypass. Work
 *                 refusal (NULL `control` only): "smart sharpen exceeds the
 *                 bounded pixel-work limit".
 *   liquify       strokes (required): an array of 1-2048 objects {tool, size,
 *                 strength, density, points, continued}, coordinates in the
 *                 image's own pixels. tool is "forward_warp", "pucker",
 *                 "bloat", "twirl_clockwise", "twirl_counterclockwise",
 *                 "reconstruct", "freeze" or "thaw"; size (required) is the
 *                 brush diameter, 1-5000 px; strength and density are 0-1
 *                 (default 0.5); points is 1-8192 finite [x, y] pairs within
 *                 +/-1000000; continued (true/false, default false) drops the
 *                 dab at point 0, for a stroke that carries on from one that
 *                 ended there. At most 131072 points in all. The strokes are
 *                 applied in order to a displacement and freeze field (one
 *                 node per ceil(max(w, h) / 2048) pixels), and the image is
 *                 resampled through it: Catmull-Rom, premultiplied, edge
 *                 pixels repeating. A pixel the field does not move keeps its
 *                 bytes, hidden colour under alpha 0 included. Moves alpha
 *                 with the colour. Refusals: "liquify needs strokes: an array
 *                 of 1 to 2048 strokes", "unexpected liquify stroke parameter
 *                 <key>", "liquify tool must be one of forward_warp, pucker,
 *                 bloat, twirl_clockwise, twirl_counterclockwise, reconstruct,
 *                 freeze, thaw", "liquify size must be finite in [1,5000]",
 *                 "liquify strength must be finite in [0,1]", "liquify density
 *                 must be finite in [0,1]", "liquify points must be 1 to 8192
 *                 finite [x,y] pairs within 1000000", "liquify continued must
 *                 be true or false", "liquify strokes hold at most 131072
 *                 points in all", and, when the strokes' summed brush boxes
 *                 pass 1 billion field nodes, "liquify strokes exceed the
 *                 limit of 1 billion field updates; use fewer or shorter
 *                 strokes or a smaller brush". It has no work ceiling beyond
 *                 that, controlled or not.
 *   depth_blur    Filter > Neural Filters > Depth Blur (depth_blur.rs's
 *                 module doc has the model). focal_distance, focal_range
 *                 (default 20), blur_strength (default 50), haze, grain
 *                 (0-100 each, default 0 otherwise), warmth, brightness,
 *                 saturation (-50-50, default 0), focus_subject and
 *                 depth_map_only (true/false, default false), focal_point
 *                 ([x, y] in image pixels, inside the depth frame, optional),
 *                 depth (required): {width, height, samples (width*height
 *                 numbers, larger = nearer, at most 1048576), frame ([x, y,
 *                 w, h] in image pixels, inside the image), subject (optional
 *                 width*height integers 0-255)}. A disc mean whose radius
 *                 follows the guided depth, then haze, a grade and grain; the
 *                 depth map alone with depth_map_only. A bypass when it would
 *                 change nothing (strength 0 or focal_range 100, and every
 *                 other control 0, depth_map_only false). Refusals include
 *                 "depth blur needs a depth map", "depth map has no range",
 *                 "depth map samples must be finite", "focus subject found no
 *                 subject in the depth input", "depth blur frame lies outside
 *                 the image", "depth blur focal point lies outside the depth
 *                 frame", "unexpected depth_blur parameter <key>". No work
 *                 ceiling, controlled or not.
 *   unsharp_mask  radius (0-64, default 1.5), amount (0-5, default 1; 1 is
 *                 100%), threshold (0-1, default 0) — detail is amplified only
 *                 where |source - blurred| exceeds the normalized threshold
 *                 (1/255 is one byte level).
 *   high_pass     radius (0-64, default 3) — the detail image around the
 *                 neutral 0.5 pivot.
 *   dust_and_scratches  radius (0-64, default 2), threshold (0-1, default 0) —
 *                 radius is the binary64 radius of a DISC of whole source
 *                 pixels (the lattice points with dx*dx + dy*dy <=
 *                 radius*radius, so the disc grows in steps), not a Gaussian
 *                 sigma. A channel takes the disc's per-channel lower median
 *                 only where it differs from it by more than the threshold, so
 *                 threshold 0 is a plain median rather than a bypass.
 *                 Transparent neighbours are excluded from the vote outright
 *                 rather than weighted: alpha decides membership.
 *   reduce_noise  radius (0-64, default 1), luminance (0-0.25, default 0.01),
 *                 detail (0-1, default 0.5), color (0-1, default 0.5) — an
 *                 alpha-weighted guided denoise over a Rec. 709 opponent
 *                 decomposition. radius sets the analysis windows,
 *                 ceil(3*radius) for luminance and four times that, capped at
 *                 256, for colour, so its cost does not grow with radius;
 *                 luminance is the luminance noise LEVEL to remove in
 *                 normalized units, so it decides what counts as grain rather
 *                 than how hard to smooth; detail is the fraction of removed
 *                 luminance detail added back, 1 turning that half off; color
 *                 is the amount of edge-aware chroma smoothing, 0 turning that
 *                 half off. A colour boundary carrying no luminance step is
 *                 invisible to the chroma guide and will bleed, which is what
 *                 color trades against.
 * Unsharp Mask and High Pass read the shared Gaussian: radius is the binary64
 * sigma in pixels, support ceil(3*radius), normalized weights, clamped edges,
 * premultiplied sampling. The four photographic ops share one neighborhood
 * dispatcher and one combine step with the 16-bit selected filters; none of
 * them is dr_image_sharpen's historical policy, which is unchanged for
 * existing recorded Actions. A colour-only
 * filter (every op but blur, box_blur, radial_blur, motion_blur, lens_blur,
 * tilt_shift, lens_correction, liquify, clouds and offset) leaves alpha
 * exact and alpha-0 pixels untouched.
 *
 * NULL `img` returns NULL with no error, checked before `control` is read and
 * before the parameters. An exact bypass returns NULL with no error, so the
 * caller adds no undo step: add_grain amount 0; offset with horizontal and
 * vertical both 0; lens_correction with every parameter at its default;
 * presence with texture, clarity and dehaze all 0; develop_filter with all
 * thirteen controls 0;
 * radius 0, or amount 0, for the
 * two sharpeners; a radius below 1 pixel, or a threshold of exactly 1, for
 * dust_and_scratches; a radius of 0, or both halves off (luminance 0 or detail
 * 1, together with color 0), for reduce_noise; liquify strokes that leave the
 * field the identity (strength 0, or a warp a reconstruct fully undoes), which
 * only the run can tell, so their plan below is above 0. Any other op, bad JSON, an
 * invalid parameter (the message names it) or, with NULL `control` only, a
 * plan over the destructive work ceiling returns NULL with a message through
 * err_out (free with dr_string_free); the ceiling is checked before any pixel
 * buffer is allocated. A controlled call has no work ceiling and is admitted
 * by memory alone. NULL `control` runs uncontrolled. A non-NULL control is claimed
 * (single-use) before the op and parameters are validated: a cancel returns
 * NULL with "operation cancelled", a consumed token NULL with "job control has
 * already been used". Controlled output is byte-identical to uncontrolled. */
DrImage *dr_image_filter_op(const DrImage *img, const char *op,
    const char *params_json, const DrJobControl *control, char **err_out);

/* dr_image_filter_op's planned peak scratch for the same op and params_json on img
 * (ops_spatial::filter_op_scratch_bytes): the kernel's plan or its held float plane plus the
 * output, whichever is larger, plus 4 MiB, plus a quarter. "blur" holds no float plane beside its
 * output, so its plan is its own: the n*4 output plus one 128-row band (from radius 7 the extended-box
 * kernel's band, from 4 the radius-independent filter's; below 4 48 bytes per pixel of the band and
 * its halo rows, plus 2 MiB), plus 4 MiB, plus a quarter. Allocates no pixels. NULL img: 0, no
 * error. Exact bypass: 0, no error. A refusal dr_image_filter_op makes before allocating (op, JSON,
 * parameter): 0 with the same message through err_out (free with dr_string_free). The plan carries
 * no work refusal. "liquify" plans 16 bytes a pixel plus 20 a field node plus 2 MiB as its
 * kernel term, and its stroke-work refusal ("liquify strokes exceed the limit of 1 billion field
 * updates; ...") is a parameter refusal, not a work ceiling, so this export makes it too. */
uint64_t dr_image_filter_op_scratch_bytes(const DrImage *img, const char *op,
    const char *params_json, char **err_out);

/* ------------------------------------------------------------------------ */
/* Liquify: the workspace's live field                                      */
/* ------------------------------------------------------------------------ */

/* A plane's displacement and freeze field with the strokes applied so far.
 * Immutable: dr_liquify_field_apply returns a new one. The Liquify workspace
 * previews through it; the committed edit is dr_image_filter_op "liquify" (8
 * bits) or the selected edit {"layer","op":"liquify","params"} (16 bits) over
 * the same strokes, which build the same field. */
typedef struct LiquifyField DrLiquifyField;

/* The identity field of a width x height plane. NULL with a message through
 * err_out (free with dr_string_free) for an empty plane or one past the pixel
 * limit. Free with dr_liquify_field_free. */
DrLiquifyField *dr_liquify_field_create(uint32_t width, uint32_t height, char **err_out);

/* Pure: a new field with strokes_json applied after the field's own.
 * strokes_json is the "liquify" params object above, {"strokes":[...]}, at
 * most 8 MiB, coordinates in PLANE pixels. The field carries the summed work
 * of every stroke applied to it, so a field built stroke by stroke and one
 * built from the whole list refuse at the same stroke, with the same
 * 1-billion message. NULL field: NULL, no error. Bad JSON, an invalid stroke
 * or the work refusal: NULL with a message through err_out; `field` is still
 * usable. */
DrLiquifyField *dr_liquify_field_apply(const DrLiquifyField *field, const char *strokes_json,
    char **err_out);

/* img, a picture of the field's plane at ANY size (the layer itself or a
 * reduced proxy), rendered through the field as dr_image_filter_op "liquify"
 * renders it. NULL previous: all of img (x, y, width, height are not read).
 * Otherwise a copy of previous (same size as img) with the rect re-rendered
 * from img; an empty rect, a rect not inside img, or a previous of another
 * size errors. NULL field or img: NULL, no error. */
DrImage *dr_liquify_field_render(const DrLiquifyField *field, const DrImage *img,
    const DrImage *previous, uint32_t x, uint32_t y, uint32_t width, uint32_t height,
    char **err_out);

/* The field's freeze as coverage 0-255, sampled at the pixel centres of a
 * width x height picture of the plane, row-major into out. False, with out
 * untouched, for a NULL field or out, an empty picture, or out_len other than
 * width * height. */
bool dr_liquify_field_freeze(const DrLiquifyField *field, uint32_t width, uint32_t height,
    uint8_t *out, size_t out_len);

/* Frees a field. NULL is ignored. */
void dr_liquify_field_free(DrLiquifyField *field);

/* ------------------------------------------------------------------------ */
/* Adjustments (the ONE destructive twin), statistics and readout           */
/* ------------------------------------------------------------------------ */

/* Applies the adjustment `op` with `params_json` — the SAME object an
 * adjustment layer's meta carries (schema table in core/src/adjust.rs) — to
 * img's pixels, destructively. This is the identical code path the
 * compositor runs for an adjustment layer, so the filter and the layer can
 * never drift. The one deliberate difference: a SPATIAL op
 * (shadows_highlights) reads its neighbourhood from THIS image, while the
 * layer reads it from the backdrop below itself, so the two agree on the
 * same input and are different pictures on different inputs.
 *
 * An adjustment is a pure function of the numbers the document already
 * holds: its parameters live in the same space as those pixels, so a colour
 * in an adjustment's params is the DOCUMENT's numbers (like an eyedropper
 * sample), not an authored sRGB colour like a layer style's. Where an op
 * needs a colour space for its arithmetic it uses the sRGB transfer function
 * and sRGB primaries, and its schema row says so.
 *
 * params_json may be NULL for "every default". NULL with a message through
 * err_out (free with dr_string_free) when op is unknown, params_json is not
 * valid UTF-8/JSON/an object, or the parameters are not valid for op; NULL
 * with no message on a NULL image. Alpha untouched. */
DrImage *dr_image_adjust_op(const DrImage *img, const char *op,
                            const char *params_json, char **err_out);

/* Parses an Adobe Cube LUT (.cube) at `path` into the params object a
 * "color_lookup" adjustment stores: {"kind","size","source_size",
 * "domain_min","domain_max","table","title"} as a heap JSON string freed
 * with dr_string_free. 1D sizes 2..=65536 and 3D sizes 2..=64 are accepted;
 * a table larger than this build STORES (1D 1024, 3D 33) is resampled down
 * to that size. "source_size" is ALWAYS emitted and is the size the FILE
 * declared, so a caller can report "resampled from 64" — "size" is what is
 * stored. "title" is present only when the file carried a TITLE directive.
 * NULL with a message through err_out for a missing/oversized/malformed file
 * (the message names the path and what was wrong); never panics. */
char *dr_lut_parse_cube(const char *path, char **err_out);

/* Reads the ICC profile at `path` as a Color Lookup LOOK and returns
 * the params object a "color_lookup" adjustment stores — the same shape
 * dr_lut_parse_cube returns: {"kind":"3d","size","source_size",
 * "domain_min","domain_max","table","title"}, title from the profile's
 * description — as a heap JSON string freed with dr_string_free. Two classes
 * load: an RGB->RGB device link ("link"), applied to the document's numbers
 * directly, and an abstract profile ("abst") from Lab
 * or XYZ to Lab or XYZ, baked through `document_icc`, relative colorimetric
 * (RGB -> PCS -> abstract -> PCS -> RGB). A link whose A2B0 is linear curves
 * around one 3-input n^3 CLUT with no matrix and n <= 33 stores "size" n, its
 * own nodes; any other link and every abstract is sampled at 33^3
 * (tetrahedrally at k/32). "source_size" is the larger of "size" and the
 * link's own CLUT grid (33 for an abstract). An abstract's params add
 * "abstract_profile", the standard base64 of the file's bytes (at most 4 MiB,
 * else refused), and every later assign, convert or Don't-Convert paste
 * re-bakes the table through the new profile, keeping every other key; a
 * link's or a .cube's table is never re-baked. NULL or empty
 * `document_icc` means no document profile: links still load and abstracts
 * refuse. Every other class or space, a missing or mpet A2B0 and an oversized
 * file are NULL with a message through err_out naming the path and the
 * reason; never panics. */
char *dr_lut_parse_profile(const char *path, const uint8_t *document_icc,
                           size_t document_len, char **err_out);

/* dr_lut_parse_profile's JSON from the look profile's BYTES: the same
 * params and the same refusals, with no path in any error message. NULL or
 * empty `look` is refused through err_out; `document_icc` follows
 * dr_lut_parse_profile's rule. Free with dr_string_free; never panics. */
char *dr_lut_bake_profile(const uint8_t *look, size_t look_len,
                          const uint8_t *document_icc, size_t document_len,
                          char **err_out);

/* Decodes the `len` bytes at `bytes` as a Photoshop adjustment PRESET file
 * of `extension` — no dot, any case: "acv" Curves, "alv" Levels, "ahu"
 * Hue/Saturation, "eap" Exposure, "blw" Black & White, "cha" Channel Mixer
 * — and returns {"op":…,"params":{…}}, the op and the params an
 * adjustment layer of that op stores, as a heap JSON string freed with
 * dr_string_free. Five of the six files are the PSD record body of the same
 * kind and decode by its rules (a .cha is a `mixr` body, so a CMYK Channel
 * Mixer preset refuses); an .acv's first
 * four curves are the composite and R, G, B, and a curve past the fourth must
 * be the two-point identity (a CMYK preset refuses). `evaluation` is never in
 * the params, so the caller's default decides it. NULL `bytes` with `len` 0 is
 * an empty file (refused by its format's truncation sentence). NULL with a
 * message through err_out on a NULL `extension`, a NULL `bytes` with a
 * non-zero `len`, `len` over 65536 (checked before the bytes are read), an
 * unsupported extension, or any file the format cannot carry; never panics. */
char *dr_adjustment_preset_file_decode(const uint8_t *bytes, size_t len,
                                       const char *extension, char **err_out);

/* Fills bins_out with 1024 counts — 256 red, then green, then blue, then
 * Rec. 709 luma — and total_out with the number of pixels counted. A pixel
 * counts when its alpha is non-zero and, when `mask` is given (a coverage
 * buffer of exactly width*height bytes), its coverage is >= 128. `stride`
 * counts every stride-th pixel in row-major order (0 and 1 both mean every
 * pixel) and total_out reports how many were actually counted, so the
 * proportions and the clipping counts stay comparable at any stride. A NULL
 * total_out is tolerated — the bins still come back — but a NULL bins_out is
 * not: false on that, or on a NULL image. */
bool dr_image_histogram(const DrImage *img, const uint8_t *mask,
                        uint32_t stride, uint32_t *bins_out,
                        uint64_t *total_out);

/* The straight RGBA at (x, y): the plain mean of the (2*reach+1) square
 * about it with out-of-bounds pixels dropped (reach 0, 1 or 2 = point, 3x3,
 * 5x5), truncating like the eyedropper it serves. Writes 4 bytes to
 * rgba_out. false only when NO pixel of the block is inside the image, reach
 * is above 2, or img/rgba_out is NULL — the CENTRE itself may be outside,
 * which is what the eyedropper does today. */
bool dr_image_sample(const DrImage *img, int32_t x, int32_t y, uint32_t reach,
                     uint8_t *rgba_out);

typedef enum {
  DR_AUTO_TONE = 0,     /* per-channel stretch */
  DR_AUTO_CONTRAST = 1, /* one stretch from the luma histogram */
  DR_AUTO_COLOR = 2,    /* per-channel stretch + neutral-candidate midtones */
} DrAutoMode;

/* Derives Levels parameters from img's own histogram — the same counting
 * rule dr_image_histogram uses, so transparent pixels never move the black
 * point — clipping `clip` of the counted pixels at each end (0..=0.1;
 * 0.001 = 0.1%), and writes nine floats to params_out:
 * black[3], white[3], gamma[3]. `mask` is optional (NULL = the whole
 * image). false on a NULL image, an unknown mode, an out-of-range clip, an
 * empty image, or when the result would be the identity (nothing to do). */
bool dr_image_auto_levels(const DrImage *img, const uint8_t *mask,
                          DrAutoMode mode, float clip, float *params_out);

/* Levels with a black point, white point and gamma PER CHANNEL (three
 * floats each, R, G, B): maps [black, white] to [0,1] then applies gamma
 * (out = t^(1/g)). NULL unless every channel has 0 <= black < white <= 1
 * and gamma in [0.1, 10]. Alpha untouched. */
DrImage *dr_image_levels_channels(const DrImage *img, const float *black,
                                  const float *white, const float *gamma);

/* CIE L*a*b* of one straight RGB triple of THIS document's pixels, through
 * the document's own profile, against the D50 PCS white — so the same bytes
 * read differently in an sRGB and a Display P3 document. Writes L, a, b to
 * lab_out. false on a NULL doc/out pointer or a profile this build cannot
 * model (a LUT profile), where the host must say so rather than assume
 * sRGB. */
bool dr_doc_lab(const DrDocument *doc, uint8_t r, uint8_t g, uint8_t b,
                float *lab_out);

/* CIE L*a*b* of one ENCODED sRGB colour — rgb is 3 floats, each clamped
 * into [0, 1] — through the BUILT-IN sRGB profile against the D50 PCS
 * white: dr_doc_lab's pipeline on an sRGB document, in float. Writes L, a,
 * b to lab_out. false on a NULL pointer, a non-finite component, or a
 * built-in sRGB profile with no matrix/TRC model. Value-only: no handle. */
bool dr_srgb_to_lab(const float *rgb, float *lab_out);

/* One CIE L*a*b* colour (D50 PCS white) as ENCODED sRGB through the
 * BUILT-IN sRGB profile's inverse matrix and the sRGB OETF. Writes the 3
 * components, each clamped into [0, 1], to rgb_out, and to *in_gamut_out
 * whether every linear component was within a 1e-3 tolerance of [0, 1]
 * before that clamp. false on a NULL pointer, a non-finite component, or a
 * built-in sRGB profile with no matrix/TRC model. Value-only: no handle. */
bool dr_lab_to_srgb(const float *lab, float *rgb_out, bool *in_gamut_out);

/* One CIE L*a*b* colour (D50) as the ENCODED RGB of the space a Lab document's `op` runs its Color form in
 * (ProPhoto RGB for hue_saturation, color_balance, photo_filter, shadows_highlights, replace_color; sRGB for
 * color_lookup, gradient_map), each clamped into [0, 1]: what that kernel reads for the sample. false on a NULL
 * pointer, a non-finite component or an op without the Color form. Value-only: no handle. */
bool dr_lab_kernel_rgb(const char *op, const float *lab, float *rgb_out);

/* ---- Selection regions and region painting ------------------------------
 *
 * Selection masks are canvas-sized u8 coverage buffers (width*height
 * bytes, row 0 top): 0 = outside, 255 = fully inside, intermediate
 * values scale paint coverage at anti-aliased edges. */

/* Similar-color selection from the flattened composite ("select what you
 * see"): writes a canvas-sized 0/255 mask into mask_out. A pixel is similar
 * when its R, G, B, A, R-G, G-B and R-B each lie within `tolerance` of the
 * seed's (measured against Photoshop; the bucket uses it too);
 * `contiguous` restricts the selection to the four-connected region around
 * the seed. `sample_size` is the Sample Size: 1 = Point Sample (the
 * seed's own pixel); 3, 5, 11, 31, 51 or 101 = the truncating per-channel
 * mean of that square about the seed, clipped to the canvas, is the
 * reference instead. false on NULL, out-of-canvas input or any other
 * sample size. */
bool dr_doc_magic_wand(const DrDocument *doc, uint32_t x, uint32_t y,
                       uint8_t tolerance, bool contiguous, uint32_t sample_size,
                       uint8_t *mask_out);

/* The wand's "Sample: Current layer": the same similar-color region,
 * grown over layer `idx` ALONE on a transparent canvas (its raw pixels, as
 * dr_doc_layer_canvas_image gives them), `sample_size` as dr_doc_magic_wand's.
 * Writes a canvas-sized 0/255 mask; false on NULL, a seed off the canvas, a
 * bad idx, an adjustment layer or an unlisted sample size. */
bool dr_doc_magic_wand_layer(const DrDocument *doc, size_t idx, uint32_t x, uint32_t y,
                             uint8_t tolerance, bool contiguous, uint32_t sample_size,
                             uint8_t *mask_out);

/* Select > Grow (contiguous) / Select > Similar from the flattened composite:
 * writes into mask_out the canvas-sized 0/255 region whose colours lie in the
 * range of every pixel `mask` covers at least half (>= 128), on R, G, B, A,
 * R-G, G-B and R-B, widened by `tolerance` on both sides. `contiguous` floods
 * four-connected from those reference pixels. The host adds the result to the
 * selection. mask_len must equal width*height. false with err_out set when it
 * does not or no pixel is at least half selected ("No pixel is more than
 * half selected, so there is no colour to match."); false with err_out untouched on NULL doc/mask/mask_out.
 * mask_out is written only on success. */
bool dr_doc_select_similar(const DrDocument *doc, const uint8_t *mask,
                           size_t mask_len, uint8_t tolerance, bool contiguous,
                           uint8_t *mask_out, char **err_out);

/* Bucket fill on layer idx: grows a similar-color region (dr_doc_magic_wand's
 * seven-projection rule) over the
 * layer's OWN pixels from canvas point (x, y) and paints rgba (straight,
 * 4 bytes) source-over it. `mask` is a selection coverage buffer or NULL
 * for none; a seed outside the canvas, the layer, or the mask is NULL. */
DrDocument *dr_doc_bucket_fill(const DrDocument *doc, size_t idx, int32_t x,
                               int32_t y, uint8_t tolerance,
                               const uint8_t *rgba, bool contiguous,
                               const uint8_t *mask);

typedef enum {
  DR_GRADIENT_LINEAR = 0, /* along p0->p1, clamped past the ends */
  DR_GRADIENT_RADIAL = 1, /* from p0, radius |p1-p0| */
  /* The three below are dr_doc_gradient_stops' only; dr_doc_gradient answers
   * NULL for them. */
  DR_GRADIENT_ANGLE = 2,     /* the turn about p0 from p0->p1, clockwise on the y-down canvas */
  DR_GRADIENT_REFLECTED = 3, /* linear, mirrored about p0 */
  DR_GRADIENT_DIAMOND = 4,   /* L1 distance from p0 in the drag's axes, over |p1-p0| */
} DrGradientKind;

/* Paints a two-color gradient source-over layer idx (the whole layer,
 * scaled by `mask` where given). Colors are straight RGBA (4 bytes
 * each), interpolated component-wise. NULL if p0 == p1 or coordinates
 * are not finite. */
DrDocument *dr_doc_gradient(const DrDocument *doc, size_t idx, float x0,
                            float y0, float x1, float y1,
                            const uint8_t *start_rgba,
                            const uint8_t *end_rgba, DrGradientKind kind,
                            const uint8_t *mask);

/* Paints a STOPS gradient onto layer idx (the whole layer, scaled by
 * `mask` where given) along p0->p1 in any DrGradientKind, through blend mode
 * `mode` (DR_BLEND_NORMAL is plain source-over, any other mode composites
 * each stop colour as dr_doc_fill_selection does). stops: stop_count
 * x 7 floats [position, r, g, b, a, midpoint, opacity_midpoint], straight,
 * 0..1, 2..32 entries in non-decreasing position; colours interpolate
 * component-wise between the two bracketing stops and of two stops at one
 * position the later wins past it. A stop's midpoint (colour) and
 * opacity_midpoint (alpha), 0.05..0.95 (0.5 the default), are the
 * fraction toward the next stop where the ramp is half way. smoothness is
 * the gradient's Smoothness as a fraction: 1 (100 %) is the
 * Classic ramp exactly, 0 the linear one, between them the two blended
 * (gradient_field::smooth_color, the fill layers' sampler); Diamond ignores it.
 * NULL for NULL doc/stops, a stop count outside 2..32, a stop out of range or
 * order, a midpoint outside 0.05..0.95, a smoothness not finite in 0..1, an
 * unknown kind, a mode
 * dr_doc_fill_selection refuses (unknown or DR_BLEND_PASS_THROUGH), p0 == p1
 * or a coordinate that is not finite, a group
 * or out-of-range idx, a pixel lock, or a gradient that changes no pixel.
 * dr_doc_gradient is unchanged. */
DrDocument *dr_doc_gradient_stops(const DrDocument *doc, size_t idx, float x0,
                                  float y0, float x1, float y1,
                                  const float *stops, size_t stop_count,
                                  DrGradientKind kind, DrBlendMode mode,
                                  float smoothness, const uint8_t *mask);

/* Gradient stop midpoints: the segment fraction f remapped through a
 * stop's midpoint, gradient_field::midpoint_fraction (the ONE remap; the
 * gradient editor's strip draws through it). f is clamped to 0..1 (NaN 0),
 * midpoint to 0.05..0.95 (NaN 0.5). At 0.5 it answers f exactly. */
float dr_gradient_midpoint_fraction(float f, float midpoint);

/* Clears the selected region of layer idx to transparency, in PROPORTION to
 * the selection's coverage. `mask` is a CANVAS-sized coverage buffer as
 * described above (w and h must equal the canvas size), mapped onto the layer
 * through its offset: each layer pixel takes the coverage c at its own canvas
 * position and its straight alpha becomes
 *
 *     alpha' = round(alpha * (255 - c) / 255)
 *
 * so full coverage erases the pixel, half coverage halves its alpha, and zero
 * coverage leaves it byte-identical — a feathered or anti-aliased selection
 * cuts a SOFT-edged hole, which is the point of the operation. A pixel whose
 * alpha reaches 0 also has its COLOR zeroed (nothing of it remains); one that
 * stays partly visible keeps its RGB exactly, since pixels are straight alpha
 * and scaling color would darken the surviving fringe. Layer pixels lying
 * outside the canvas are untouched (a selection never reaches past the
 * canvas), and only pixels change: the layer's mask, offset, name, opacity,
 * blend mode, visibility and metadata all survive.
 *
 * A mask that selects nothing is NOT an error — as with dr_doc_gradient there
 * is no seed to validate, so the call succeeds and the pixels come back
 * unchanged. NULL on a NULL doc or mask, w/h that are not the canvas size, or
 * an out-of-range idx. */
DrDocument *dr_doc_clear_selection(const DrDocument *doc, size_t idx,
                                   const uint8_t *mask, uint32_t w,
                                   uint32_t h);

/* Writes canvas rect (x, y, w, h) onto layer idx with REPLACE semantics: every sample where mask is
 * NULL or nonzero replaces all four straight RGBA8 bytes verbatim (no blending; alpha 0 writes
 * transparency). Elsewhere the layer is unchanged. rgba: w*h*4 bytes, row 0 top. mask: NULL or w*h bytes.
 * Mapped through the layer offset. Samples outside the layer's pixel rect are ignored; the layer
 * never grows. Lock-gated like every pixel writer (transparency lock restores alpha). NULL on NULL
 * doc/rgba, w or h 0, rect not fully inside the canvas, a non-raster or out-of-range idx, a rect that
 * misses the layer, a pixel-locked layer, or when no byte would change. */
DrDocument *dr_doc_put_pixels(const DrDocument *doc, size_t idx, uint32_t x, uint32_t y,
                              uint32_t w, uint32_t h, const uint8_t *rgba, const uint8_t *mask);

/* Gaussian-feathers a selection mask in place (width*height coverage
 * bytes, row 0 top). Sampling clamps to the canvas edges, so a
 * selection touching the border keeps full coverage there. false on
 * NULL mask, zero dimensions, or a non-finite radius; radius <= 0
 * returns true and leaves the mask untouched. */
/* Quick Mask source-over on transient coverage. The raw overlay is canvas-sized
 * premultiplied RGBA (red is gray); scale applies with integer /255 rounding.
 * RLE runs are UInt32LE count + premultiplied gray + alpha, already scaled.
 * Both validate the complete input before changing mask; false leaves it intact. */
bool dr_selection_paint_quick_mask(uint8_t *mask, const uint8_t *overlay,
                                   uint32_t width, uint32_t height, uint8_t scale);
bool dr_selection_paint_quick_mask_rle(uint8_t *mask, uint32_t width, uint32_t height,
                                       const uint8_t *runs, size_t runs_len);

bool dr_selection_feather(uint8_t *mask, uint32_t width, uint32_t height,
                          float radius);

/* Selection morphology: in-place companions to dr_selection_feather on the
 * same width*height coverage buffers (row 0 top). Grow, shrink and border
 * are defined on the signed Euclidean distance to the mask's 50% contour:
 * coverage is first binarized at >= 128 — any feathered softness is
 * DELIBERATELY resolved to its 50% contour — and an exact Euclidean
 * distance transform both ways yields s = distance to the contour,
 * positive inside. The canvas boundary is NOT a contour: only the
 * inside/outside pixels actually present in the buffer count, so a full
 * mask stays full under grow and shrink, an empty mask stays empty, and
 * border maps both to empty (no contour, no band). New edges come back
 * with a fresh ~1px anti-aliased ramp.
 *
 * All four return false on a NULL mask, zero dimensions, or a non-finite
 * parameter; a parameter <= 0 returns true and leaves the mask untouched,
 * exactly as dr_selection_feather treats its radius. */

/* Grows (dilates) the selection by radius pixels of true Euclidean
 * distance: coverage' = clamp(s + radius + 0.5, 0, 1) * 255, so edges move
 * outward by radius and corners round into circular arcs. */
bool dr_selection_grow(uint8_t *mask, uint32_t width, uint32_t height,
                       float radius);

/* Shrinks (erodes) the selection by radius pixels: grow with -radius, so
 * edges move inward and outside corners round the same way. */
bool dr_selection_shrink(uint8_t *mask, uint32_t width, uint32_t height,
                         float radius);

/* Replaces the selection with an anti-aliased band width_px wide straddling
 * its 50% contour: coverage' = clamp(width_px/2 - |s| + 0.5, 0, 1) * 255. */
bool dr_selection_border(uint8_t *mask, uint32_t width, uint32_t height,
                         float width_px);

/* Smooths the selection: a Gaussian blur with exactly the sigma mapping of
 * dr_selection_feather, then a smoothstep contrast remap (t = v/255,
 * v' = round(255 * t^2 * (3 - 2t))). Corners round and jagged edges
 * reconcile while a long straight edge stays put — the blur is symmetric
 * across it and smoothstep fixes 1/2. Unlike grow/shrink/border this never
 * binarizes: soft coverage stays soft. */
bool dr_selection_smooth(uint8_t *mask, uint32_t width, uint32_t height,
                         float radius);

/* ---- Layer masks --------------------------------------------------------
 *
 * Original mask coverage is a separate grayscale plane with its own dimensions
 * and canvas offset. New raster masks start at layer bounds; new group masks
 * start at canvas bounds. Linked masks follow layer geometry; unlinked masks
 * stay in canvas coordinates. Whole-document geometry transforms both.
 * Density and feather affect derived coverage while preserving original bytes:
 * effective = (1-density) + density*feathered_coverage. Outside raw bounds is
 * zero before feather/density. Numeric feather radius stays in canvas pixels
 * through geometry. Replacing pixels preserves an existing independent mask.
 * Native v10 persists placement, linking, density, feather and raw dimensions.
 *
 * The layer getters above stay deliberately mask-free: dr_doc_layer_image and
 * dr_doc_layer_thumbnail return the layer's UNMASKED pixels, and the mask has
 * an image of its own (dr_doc_layer_mask_image, fitted in the core by
 * dr_doc_layer_mask_thumbnail) for a second thumbnail beside them. Only the
 * projection (dr_doc_flattened) applies masks. */

typedef enum {
  DR_MASK_REVEAL_ALL = 0,     /* filled 255: the layer shows in full */
  DR_MASK_HIDE_ALL = 1,       /* filled 0: the layer is hidden entirely */
  DR_MASK_FROM_SELECTION = 2, /* the selection buffer, cropped to the layer */
} DrMaskKind;

/* Gives layer idx a mask (replacing any existing one) and enables it, at
 * exactly the layer's pixel size. For DR_MASK_FROM_SELECTION, `selection` is
 * a CANVAS-sized coverage buffer as described under "Selection regions"
 * above — w and h must equal the canvas size — and is CROPPED to the layer's
 * rect: each mask pixel takes the selection value at its own canvas position,
 * and layer pixels lying outside the canvas get 0 (a selection never reaches
 * past the canvas). For the other kinds `selection` is unused: pass NULL,
 * 0, 0. NULL on out-of-range idx, an unknown kind, or a NULL / not
 * canvas-sized selection buffer. */
DrDocument *dr_doc_adding_layer_mask(const DrDocument *doc, size_t idx,
                                     DrMaskKind kind, const uint8_t *selection,
                                     uint32_t w, uint32_t h);

/* Drops layer idx's mask. With apply, the coverage is first baked into the
 * layer's alpha (alpha' = alpha * mask / 255, straight alpha) REGARDLESS of
 * the enabled flag — "apply" always means what it says — so applying an
 * ENABLED mask leaves the projection unchanged. Without apply the pixels are
 * untouched and the layer is revealed in full again. NULL on out-of-range idx
 * or a layer with no mask. */
DrDocument *dr_doc_removing_layer_mask(const DrDocument *doc, size_t idx,
                                       bool apply);

/* Enables or disables layer idx's mask: a disabled mask is RETAINED (and
 * saved) but ignored while compositing, so the layer composites exactly as if
 * it had none. NULL on out-of-range idx or a layer with no mask. */
DrDocument *dr_doc_with_layer_mask_enabled(const DrDocument *doc, size_t idx,
                                           bool enabled);

/* Paints layer idx's MASK with a CANVAS-frame PREMULTIPLIED RGBA8 overlay —
 * the very buffer dr_doc_painting_layer takes, w/h equal to the canvas size —
 * mapped through the independent mask offset. Each raw mask pixel samples the overlay at
 * its canvas position and becomes
 *   mask' = round(lerp(mask, luma(straight color), overlay alpha))
 * so painting white reveals and black hides, with the stroke's own
 * anti-aliasing (and any selection the caller already clipped the overlay
 * with) carried by the alpha. Overlay pixels outside raw mask bounds are ignored
 * (the mask does NOT grow). Density/feather remain untouched. NULL on a missing
 * mask, dimension mismatch, or when raw mask bounds miss the canvas entirely
 * (no mask pixel could change), matching dr_doc_painting_layer. */
DrDocument *dr_doc_painting_layer_mask(const DrDocument *doc, size_t idx,
                                       const uint8_t *src, uint32_t w,
                                       uint32_t h);

/* Raw opaque grayscale mask thumbnail: raster masks use independent stored
 * dimensions; group masks retain the legacy canvas-aligned view. For exact raw
 * storage on either kind use dr_doc_layer_mask_storage_image. NULL without a mask. */
DrImage *dr_doc_layer_mask_image(const DrDocument *doc, size_t idx);

/* Layer idx's mask as its Layers-panel thumbnail: opaque gray RGBA8,
 * the picture dr_doc_layer_mask_image answers (a group's canvas-aligned view,
 * any other layer's raw mask at its stored size), aspect-fit with the longest
 * side max_side, NEVER upscaled (0 = full size). Below 16x the thumbnail per
 * axis it is byte for byte that image followed by dr_image_resize
 * (FILTER_BILINEAR); a larger mask is box-reduced first, within 2/255 of that
 * pair, with no whole-mask image. NULL on NULL doc, out-of-range idx, a layer
 * with no mask, a zero-size mask or an allocation failure. */
DrImage *dr_doc_layer_mask_thumbnail(const DrDocument *doc, size_t idx, uint32_t max_side);

/* Mask queries. Both are false on NULL doc or out-of-range idx;
 * dr_doc_layer_mask_enabled is also false for a layer with no mask (there is
 * nothing to enable), so it can drive a checkbox or menu item directly. */
bool dr_doc_layer_has_mask(const DrDocument *doc, size_t idx);
bool dr_doc_layer_mask_enabled(const DrDocument *doc, size_t idx);

/* ---- Channels -----------------------------------------------------------
 *
 * A channel is a NAMED, CANVAS-SIZED u8 coverage plane stored on the
 * document — a saved selection, in exactly the representation selections and
 * layer masks already use (0 outside, 255 inside, intermediate =
 * anti-aliased edge). Channels never composite: they carry an overlay
 * colour, opacity and polarity only so a host can draw one as a rubylith.
 * Every canvas-geometry op keeps them canvas-sized — dr_doc_crop crops them,
 * dr_doc_canvas_resize pads with 0, the rotations and flips permute them,
 * dr_doc_resize resamples them; the per-layer ops leave them untouched, and
 * dr_doc_flattening keeps them. dr_doc_geometry_edit's "rotate_canvas"
 * request resamples each one STRAIGHT from the old canvas into the expanded
 * one, so a channel stays canvas-sized through an arbitrary rotation too, and
 * a rotation that would push the list past the total budget is refused there
 * by its own sentence. Its "straighten" request (the Crop tool's straighten)
 * turns the channels by the same matrix as the layers inside the same edit.
 * They are written by dr_doc_save_native (they
 * are what bumped the DRDC format to version 5).
 *
 * A PLANE crossing this boundary is always canvas width*height bytes, row 0
 * top — the selection convention — with TWO exceptions: dr_doc_add_channel,
 * which resamples a caller-sized plane, and dr_image_plane, whose buffer is
 * the IMAGE's size. Each is called out at its declaration.
 *
 * At most 256 channels, and at most 900000000 channel pixels in total (nine
 * full canvases, so that dr_doc_add_luminosity_masks' fixed nine always fit
 * a canvas this library will build); every op that creates channels — and
 * every op that GROWS the canvas under them, dr_doc_resize,
 * dr_doc_canvas_resize and dr_doc_geometry_edit's "rotate_canvas" — refuses
 * rather than build a document dr_doc_save_native could write and
 * dr_doc_open could not read. dr_max_channels_at answers the same budget as
 * a number, so a host can name it in the refusal instead of just failing. */

typedef enum {
  DR_PLANE_RED = 0,
  DR_PLANE_GREEN = 1,
  DR_PLANE_BLUE = 2,
  DR_PLANE_ALPHA = 3,
  DR_PLANE_LUMA = 4, /* Rec. 709 luma; read-only */
  DR_PLANE_MASK = 5, /* a layer's mask; read-only, layers only */
} DrPlane;

/* --- channel list (getters) --- */

/* The largest channel count a w*h canvas may carry under both caps. No
 * document and no pointers: a zero dimension answers the count cap. Ask it
 * before Image Size, Canvas Size or dr_doc_add_luminosity_masks to tell the
 * user WHY those refuse, and how many channels would have to go. */
size_t dr_max_channels_at(uint32_t w, uint32_t h);

size_t dr_doc_channel_count(const DrDocument *doc);

/* Channel i's STABLE IDENTITY: unique among every channel this process has
 * minted, and the handle a host hangs per-channel view state on (which
 * channel's eye is on, say). Names are not unique and every insert, delete or
 * undo renumbers the list, so neither can identify a channel across an edit;
 * the id survives a rename, a plane edit, the geometry ops and undo/redo,
 * dr_doc_duplicate_channel mints a fresh one for the copy, and a channel that
 * is gone takes its id with it. NOT persisted — dr_doc_open mints new ids, so
 * the identity holds for as long as a document is open, not across sessions.
 * 0 on NULL doc / bad index, which no live channel ever answers. */
uint64_t dr_doc_channel_id(const DrDocument *doc, size_t i);

/* Heap UTF-8 name, free with dr_string_free; NULL on NULL doc / bad index. */
char *dr_doc_channel_name(const DrDocument *doc, size_t i);

/* Writes 3 bytes (r, g, b) into rgb_out. false on NULL / bad index. */
bool dr_doc_channel_overlay_color(const DrDocument *doc, size_t i,
                                  uint8_t *rgb_out);

/* 0.0 on NULL doc or bad index. */
float dr_doc_channel_overlay_opacity(const DrDocument *doc, size_t i);

/* "Color Indicates: Selected Areas" when true; false (the default) means the
 * wash covers the MASKED areas, matching Quick Mask. false on NULL. */
bool dr_doc_channel_color_indicates_selected(const DrDocument *doc, size_t i);

/* --- channel list (pure mutators; NULL = refusal, per the doc comments) --- */

/* Appends a channel from `plane` (w*h coverage bytes, row 0 top), with the
 * default masked-areas polarity (dr_doc_set_channel_overlay changes it).
 * THE FIRST EXCEPTION to the canvas-sized rule: when w/h differ from the
 * canvas the plane is resampled bilinearly to it (the iPhone
 * auxiliary-matte path), and the length therefore comes from the CALLER's
 * w*h with checked arithmetic, exactly as dr_doc_set_layer_content
 * does. NULL on NULL args, w or h == 0, w*h > 100000000, a full
 * channel list (256), or a channel list that would exceed the total pixel
 * budget (so every document this builds can be saved and reopened). */
DrDocument *dr_doc_add_channel(const DrDocument *doc, const char *name,
                               const uint8_t *plane, uint32_t w, uint32_t h,
                               uint8_t red, uint8_t green, uint8_t blue,
                               float overlay_opacity);

DrDocument *dr_doc_remove_channel(const DrDocument *doc, size_t i);

/* NULL on a bad index or a name the channel already has. */
DrDocument *dr_doc_rename_channel(const DrDocument *doc, size_t i,
                                  const char *name);

/* All three display options at once — colour, opacity and polarity (one undo
 * step for the options sheet).
 * NULL when none of them would change. */
DrDocument *dr_doc_set_channel_overlay(const DrDocument *doc, size_t i,
                                       uint8_t red, uint8_t green,
                                       uint8_t blue, float overlay_opacity,
                                       bool color_indicates_selected);

/* `plane` is canvas-sized (w and h must equal the canvas exactly). NULL when
 * the bytes are identical to the channel's current plane. */
DrDocument *dr_doc_set_channel_data(const DrDocument *doc, size_t i,
                                    const uint8_t *plane, uint32_t w,
                                    uint32_t h);

/* Inserts a copy right after i, named "<name> copy". */
DrDocument *dr_doc_duplicate_channel(const DrDocument *doc, size_t i);

/* 255 - v. NULL only on a bad index. */
DrDocument *dr_doc_invert_channel(const DrDocument *doc, size_t i);

/* Appends nine channels built from the composite's Rec. 709 luma L:
 * "Lights n" = L^n, "Darks n" = (1-L)^n and "Midtones n" =
 * clamp(1 - L^(n+1) - (1-L)^(n+1)) for n = 1, 2, 3, in that order. The
 * midtone exponent is offset by one deliberately: the linear pair sums to 1
 * at every pixel, so the un-offset formula would be black everywhere.
 * NULL when the nine would not fit under either cap. */
DrDocument *dr_doc_add_luminosity_masks(const DrDocument *doc);

/* --- planes out, straight into a caller buffer (no image round trip) ---
 * All write exactly w*h bytes and return false on NULL, a dimension
 * mismatch, a bad index, or a plane this source cannot supply
 * (DR_PLANE_MASK from a composite or an image, a layer with no mask). */

/* One plane of the FLATTENED composite. A ONE-SHOT op — it runs the whole
 * projection, so a host drawing a plane repeatedly reads its own cached
 * projection with dr_image_plane instead. */
bool dr_doc_composite_plane(const DrDocument *doc, DrPlane plane,
                            uint8_t *out, uint32_t w, uint32_t h);

/* The flattened composite's grey in a Gray working space ("dot_gain_10" ...
 * "dot_gain_30", "gray_gamma_18", "gray_gamma_22"): v = Y^e, Y the document
 * profile's relative PCS Y of the encoded RGB (matrix/TRC model, else LUT),
 * e = ln 0.5 / ln(0.5 - d) or 1/gamma; out = round(255 v), w*h bytes. false
 * on NULL, a dimension mismatch, an unknown name, or a profile with neither
 * a matrix/TRC model nor a LUT. Load Selection's luminosity at 8 bits. */
bool dr_doc_composite_gray_plane(const DrDocument *doc, const char *gray_space,
                                 uint8_t *out, uint32_t w, uint32_t h);

/* One plane of layer idx, CANVAS-sized: pixels outside the layer's rect read
 * 0 for every plane, DR_PLANE_MASK included (outside the layer there is
 * nothing for a mask to reveal). */
bool dr_doc_layer_plane(const DrDocument *doc, size_t idx, DrPlane plane,
                        uint8_t *out, uint32_t w, uint32_t h);

bool dr_doc_channel_plane(const DrDocument *doc, size_t i, uint8_t *out,
                          uint32_t w, uint32_t h);

/* THE SECOND EXCEPTION to the canvas-sized rule: w and h are the IMAGE's own
 * dimensions. For an opaque grayscale image DR_PLANE_LUMA is the identity,
 * which makes this the lossless reader for the plane images below — and the
 * definition of "take the result's gray" for an op that un-grays one. */
bool dr_image_plane(const DrImage *img, DrPlane plane, uint8_t *out,
                    uint32_t w, uint32_t h);

/* --- planes out as opaque GRAYSCALE RGBA images (r == g == b, alpha 255) ---
 * max_side == 0 gives the plane at full size; otherwise the image is
 * aspect-fit with its longest side == max_side (the dr_doc_layer_thumbnail
 * rule). dr_image_plane_image reads a plane out of an image the host ALREADY
 * HAS — the cached projection — and is what a Channels panel's composite
 * rows and a plane canvas view use; the dr_doc_composite_* form re-flattens
 * and is for one-shot work only. */
DrImage *dr_image_plane_image(const DrImage *img, DrPlane plane,
                              uint32_t max_side);
DrImage *dr_doc_composite_plane_image(const DrDocument *doc, DrPlane plane,
                                      uint32_t max_side);
DrImage *dr_doc_layer_plane_image(const DrDocument *doc, size_t idx,
                                  DrPlane plane, uint32_t max_side);
DrImage *dr_doc_channel_image(const DrDocument *doc, size_t i,
                              uint32_t max_side);

/* --- plane writers --- */

/* Replaces ONLY `plane` of layer idx's pixels from a CANVAS-sized `src`,
 * inside the layer's rect (mapped through the layer's offset exactly as
 * dr_doc_painting_layer maps a stroke). DR_PLANE_ALPHA writes STRAIGHT alpha
 * and clears the colour bytes of any pixel whose new alpha is 0. NULL for
 * DR_PLANE_LUMA or DR_PLANE_MASK, NULL args, dimension mismatch,
 * out-of-range idx, a layer extent that misses the canvas, or when no byte
 * would change — so a caller writing several planes must tolerate NULL per
 * plane. */
DrDocument *dr_doc_with_layer_plane(const DrDocument *doc, size_t idx,
                                    DrPlane plane, const uint8_t *src,
                                    uint32_t w, uint32_t h);

/* --- selection-scoped destructive results ---
 * Filter/analyze the FULL source first, then use these pure writers to
 * confine its result. `result` must match the target's stored dimensions;
 * scalar targets read its Rec. 709 gray (opaque grayscale is exact).
 * `selection` is NULL for the entire stored target, including off-canvas
 * pixels, or w*h coverage bytes with w/h equal to the canvas dimensions.
 * Coverage maps through the layer's stored offset (also for group masks),
 * and off-canvas storage receives zero selected coverage. Zero coverage and
 * identical results preserve every byte. RGBA interpolates in encoded-color
 * premultiplied space; scalar data interpolates its byte value.
 * All return NULL for invalid targets/dimensions, blocked locks or no change.
 * Pixels/planes respect pixel and transparency locks; masks respect Lock All.
 * These preserve metadata; hosts clear source descriptions in the same edit
 * when destructive pixel writes invalidate them. */
DrDocument *dr_doc_apply_layer_pixels(const DrDocument *doc, size_t idx,
                                       const DrImage *result, const uint8_t *selection,
                                       uint32_t w, uint32_t h);
DrDocument *dr_doc_apply_layer_mask(const DrDocument *doc, size_t idx,
                                     const DrImage *result, const uint8_t *selection,
                                     uint32_t w, uint32_t h);
/* Only RGBA planes are writable. When selected alpha changes to zero, RGB
 * clears as with the existing layer-plane writer. */
DrDocument *dr_doc_apply_layer_plane(const DrDocument *doc, size_t idx, DrPlane plane,
                                      const DrImage *result, const uint8_t *selection,
                                      uint32_t w, uint32_t h);
DrDocument *dr_doc_apply_channel(const DrDocument *doc, size_t idx,
                                  const DrImage *result, const uint8_t *selection,
                                  uint32_t w, uint32_t h);
/* Destructive-operation source: full raw mask storage, with its stored
 * offset returned through each non-NULL output on success only. Unlike the
 * thumbnail reader this never bakes/clips a translated group mask. Free the
 * returned image with dr_image_free. NULL without a valid mask. */
DrImage *dr_doc_layer_mask_storage_image(const DrDocument *doc, size_t idx,
                                         int32_t *offset_x, int32_t *offset_y);

/* --- coverage painting (the mask-painting lerp; see
 *     dr_doc_painting_layer_mask for the formula) ---
 * `src` is a canvas-sized PREMULTIPLIED RGBA8 overlay, the very buffer
 * dr_doc_painting_layer takes: white paints toward 255, black toward 0, the
 * overlay's alpha carries the stroke's coverage. Both return NULL when no
 * byte would change (white over white is not an edit).
 *
 * dr_doc_painting_layer_plane refuses DR_PLANE_LUMA and DR_PLANE_MASK
 * (DR_PLANE_MASK is dr_doc_painting_layer_mask's job) and, unlike
 * dr_doc_with_layer_plane, never clears the colour bytes when it paints
 * alpha to 0: a stroke is incremental, and the colour must survive an alpha
 * that dips and is painted back up. */
DrDocument *dr_doc_painting_channel(const DrDocument *doc, size_t i,
                                    const uint8_t *src, uint32_t w,
                                    uint32_t h);
DrDocument *dr_doc_painting_layer_plane(const DrDocument *doc, size_t idx,
                                        DrPlane plane, const uint8_t *src,
                                        uint32_t w, uint32_t h);

/* --- plane arithmetic (in place on CALLER-owned buffers, like the
 *     dr_selection_* family) ---
 * base = lerp(base, blend(base, source), opacity) per pixel, through the
 * same blend table the projection uses; a gray value is the triple (v, v, v)
 * and the result's gray is taken, so every SEPARABLE mode works, Dissolve's
 * canvas-absolute dither included. The four NON-SEPARABLE modes (Hue,
 * Saturation, Color, Luminosity) are REFUSED on a single plane: they are
 * defined over an RGB triple, and a gray triple has zero saturation, which
 * collapses three of them to the base and the fourth to Normal. Blend three
 * planes as one colour with dr_blend_planes_rgb instead — that is where they
 * mean something. invert_base and invert_source invert an operand FIRST, so
 * the complement is both blended and lerped from. These two are the ONE
 * arithmetic behind Apply Image and Calculations; Apply Image passes the
 * target plane as `base`, Calculations passes Source 2 as `base` and Source 1
 * as `source`. false on NULL, a zero dimension,
 * w*h > 100000000, an unknown mode, or a non-finite opacity; on false every
 * buffer is left untouched.
 *
 * dr_blend_planes_rgb takes red, green and blue as three separate planes
 * (base and source alike), blends them AS ONE COLOUR and writes the three
 * results back into the base buffers — Apply Image with an RGB source onto an
 * RGB target. It accepts every mode, the non-separable four included. The
 * three base buffers should be distinct; aliasing them is defined but
 * answers whichever write lands last. */
bool dr_blend_planes(uint8_t *base, const uint8_t *source, uint32_t w,
                     uint32_t h, DrBlendMode mode, float opacity,
                     bool invert_base, bool invert_source);
bool dr_blend_planes_rgb(uint8_t *base_r, uint8_t *base_g, uint8_t *base_b,
                         const uint8_t *source_r, const uint8_t *source_g,
                         const uint8_t *source_b, uint32_t w, uint32_t h,
                         DrBlendMode mode, float opacity, bool invert_base,
                         bool invert_source);

/* ---- Spot channels (#1 phase 1a-2, ffi_spot_channel.rs) -----------------
 *
 * A spot channel is a channel whose ink is set: D50 CIE Lab (L 0..100, a and
 * b -128..127, finite) and Solidity, an integer percent 0..100. Its plane is
 * the ink's coverage (255 byte / 65535 typed = full ink); everything else is
 * the alpha channel's (the dr_doc_channel_* family reads and edits it). The
 * OVERPRINT is display only: per spot channel in list order, the ink encoded
 * in the composite's profile, laid source-over at weight coverage/max with
 * the blend (1 - s)*Multiply(Cb, ink) + s*ink (s = solidity/100), so 0 %
 * shows the colour beneath through the ink and 100 % covers it; coverage 0
 * leaves a pixel exactly as it was. A profile the core cannot convert into
 * reads as sRGB. The typed functions take `struct PrecisionDocument`, the
 * DrPrecisionDocument handle declared further down. */

/* Appends a spot channel; coverage is canvas width*height bytes (the length
 * is the core's own). NULL on a NULL doc; NULL with err_out for a NULL name
 * or coverage, an ink out of range, a zero canvas or a full channel budget. */
DrDocument *dr_doc_add_spot_channel(const DrDocument *doc, const char *name,
    const uint8_t *coverage, double l, double a, double b, uint8_t solidity,
    char **err_out);
/* Sets spot channel i's name (NULL keeps it) and ink in one step. NULL with
 * no error for a NULL doc, a no-op, an alpha channel or an index out of
 * range; NULL with err_out for an ink out of range. */
DrDocument *dr_doc_set_spot_channel(const DrDocument *doc, size_t i, const char *name,
    double l, double a, double b, uint8_t solidity, char **err_out);
/* Channel i's ink into lab_out[3] and solidity_out (either may be NULL);
 * false, writing nothing, for a NULL doc, an alpha channel or out of range. */
bool dr_doc_channel_spot(const DrDocument *doc, size_t i, double *lab_out,
    uint8_t *solidity_out);
/* composite (canvas-sized RGBA8) with every spot channel overprinted, in list
 * order, in the document's profile. NULL with no error for a NULL doc or no
 * spot channel; NULL with err_out for a NULL or wrongly sized composite. */
DrImage *dr_doc_spot_overprint(const DrDocument *doc, const DrImage *composite,
    char **err_out);
/* composite with one ink overprinted; coverage_len must equal the image's
 * width*height. icc/icc_len is the composite's profile (NULL, empty or
 * unusable = sRGB). NULL with err_out for a NULL composite or coverage, a
 * wrong length or an ink out of range. */
DrImage *dr_image_spot_overprint(const DrImage *composite, const uint8_t *coverage,
    size_t coverage_len, double l, double a, double b, uint8_t solidity,
    const uint8_t *icc, size_t icc_len, char **err_out);
/* The typed add through the alpha channel's append: coverage is canvas-sized
 * native-endian u16 and length must equal width*height. NULL with err_out
 * for a NULL argument, a wrong length, an ink out of range, an Indexed or
 * Bitmap document ("Spot channels are not available in Indexed Color mode."
 * / "... in Bitmap mode."), a cap, admission or a
 * cancel. control is single-use and may be NULL. */
struct PrecisionDocument *dr_precision_doc_add_spot_channel(
    const struct PrecisionDocument *doc, const char *name, double l, double a,
    double b, uint8_t solidity, const uint16_t *coverage, size_t length, uint64_t limit,
    uint64_t other, const DrJobControl *control, char **err_out);
/* The typed set: name (NULL keeps it) and ink of spot channel index. NULL with
 * no error for a no-op; NULL with err_out for a NULL document, out of range,
 * an alpha channel ("the channel is not a spot channel"), a name past the
 * native limit, an ink out of range or admission. */
struct PrecisionDocument *dr_precision_doc_set_spot_channel(
    const struct PrecisionDocument *doc, size_t index, const char *name, double l,
    double a, double b, uint8_t solidity, uint64_t limit, uint64_t other, char **err_out);

/* ---- Spot channels 1c-1 (#1, ffi_spot_merge.rs) --------------------------
 *
 * MERGE SPOT CHANNEL flattens the document (hidden layers discarded, comps
 * cleared, every other channel kept), bakes the ONE ink in through the
 * overprint above (a CMYK document in ink on paper), and removes the channel:
 * one result. A Duotone document refuses ("Merge Spot Channel is not
 * available in Duotone mode, ..."). The KIND CHANGE makes an alpha channel a
 * spot channel (spot true, the ink as dr_doc_add_spot_channel's) or a spot
 * channel an alpha channel (spot false, the ink arguments ignored), its plane
 * inverted (max - v) both ways. The REGION overprint draws a region /
 * preview_divisor render's spot channels at the samples the full render's
 * crop holds: output (i, j) reads canvas (x + i*divisor, y + j*divisor). */

/* Merge Spot Channel on byte channel i. NULL with no error for a NULL doc, an
 * alpha channel or out of range; NULL with err_out when the flatten fails. */
DrDocument *dr_doc_merge_spot_channel(const DrDocument *doc, size_t i, char **err_out);
/* Byte channel i made spot (spot true) or alpha (spot false), plane inverted.
 * NULL with no error for a NULL doc, out of range or already that kind; NULL
 * with err_out for an ink out of range. */
DrDocument *dr_doc_set_channel_kind(const DrDocument *doc, size_t i, bool spot, double l,
    double a, double b, uint8_t solidity, char **err_out);
/* Merge Spot Channel's typed plan: owned JSON {"working_memory_plan_bytes",
 * "refusal"} (a Duotone document, out of range, an alpha channel); free with
 * dr_string_free. NULL with err_out for a NULL document. */
char *dr_precision_doc_merge_spot_channel_plan(const struct PrecisionDocument *doc,
    size_t index, char **err_out);
/* The typed Merge Spot Channel, admitted before it allocates. NULL with
 * err_out for a NULL document, the plan's refusals, admission or a cancel.
 * control is single-use and may be NULL. */
struct PrecisionDocument *dr_precision_doc_merge_spot_channel(
    const struct PrecisionDocument *doc, size_t index, uint64_t limit, uint64_t other,
    const DrJobControl *control, char **err_out);
/* The typed kind change under the channel invert's plan. NULL with no error
 * when already that kind; NULL with err_out for a NULL document, out of
 * range, an ink out of range, an Indexed or Bitmap document asked for a spot
 * channel (by sentence) or admission. */
struct PrecisionDocument *dr_precision_doc_set_channel_kind(
    const struct PrecisionDocument *doc, size_t index, bool spot, double l, double a,
    double b, uint8_t solidity, uint64_t limit, uint64_t other, char **err_out);
/* composite, a region render of doc, with every spot channel overprinted at
 * its samples. NULL with no error for a NULL doc or no spot channel; NULL
 * with err_out for a NULL composite, divisor 0 or a region past the canvas. */
DrImage *dr_doc_spot_overprint_region(const DrDocument *doc, const DrImage *composite,
    uint32_t x, uint32_t y, uint32_t divisor, char **err_out);
/* composite, a region render of a canvas_width x canvas_height canvas, with
 * one ink overprinted at its samples; coverage is the CANVAS's plane and
 * coverage_len must equal canvas_width*canvas_height. icc as
 * dr_image_spot_overprint. NULL with err_out for a NULL composite or
 * coverage, a wrong length, divisor 0, a region past the canvas or an ink
 * out of range. */
DrImage *dr_image_spot_overprint_region(const DrImage *composite, const uint8_t *coverage,
    size_t coverage_len, uint32_t canvas_width, uint32_t canvas_height, uint32_t x,
    uint32_t y, uint32_t divisor, double l, double a, double b, uint8_t solidity,
    const uint8_t *icc, size_t icc_len, char **err_out);

/* ---- Clipping masks -----------------------------------------------------
 *
 * Every layer carries a CLIPPED flag (default false): a clipped layer is
 * confined to the alpha footprint of the first unclipped SIBLING beneath it
 * — PSD clipping-mask semantics, "blend clipped layers as group". Clip
 * structure is purely POSITIONAL and re-derived at every composite, WITHIN
 * EACH LEVEL: any unclipped entry is a BASE, and its clip group is the
 * consecutive run of clipped siblings immediately above it, so reordering or
 * deleting entries needs no bookkeeping. A clipped entry at the BOTTOM of its
 * level (nothing unclipped below it there) has no base and composites as if
 * unclipped. A clip run never crosses a group boundary; a GROUP may be
 * clipped and may be a clip base, and either makes it composite as a unit
 * (see "Layer groups" below).
 *
 * A base with no clipped layers above composites exactly as it always did.
 * A non-empty group blends as one unit: the base renders into a private
 * transparent buffer at FULL opacity in Normal mode, its layer mask applied
 * as usual, and the buffer's alpha after that render is the group's
 * footprint. Each visible clipped layer then composites into the buffer
 * through the normal kernel — its own opacity, blend mode (blending against
 * the base's content), mask and offset; an adjustment layer adjusts the
 * buffer — and after each one the buffer's alpha is forced back to the
 * footprint, so clipped layers never extend or shrink it. The finished
 * buffer composites into the real backdrop with the BASE layer's blend mode
 * and opacity. Hiding the base hides its whole group; invisible clipped
 * layers are skipped.
 *
 * The flag rides along wherever the layer survives as itself (setters,
 * painting, geometry, reordering), is copied by dr_doc_duplicating_layer,
 * is baked through by dr_doc_merging_down (which keeps the LOWER layer's
 * flag — see above), and round-trips through dr_doc_save_native: it is what
 * bumped the DRDC format to version 3. */

/* Pure setter: replaces layer idx's clipped flag. NULL on NULL doc or
 * out-of-range idx. */
DrDocument *dr_doc_with_layer_clipped(const DrDocument *doc, size_t idx,
                                      bool clipped);

/* The layer's clipped flag; false on NULL doc or out-of-range idx. */
bool dr_doc_layer_clipped(const DrDocument *doc, size_t idx);

/* ---- Layer metadata -----------------------------------------------------
 *
 * Every layer carries an optional metadata string: an OPAQUE, host-owned blob
 * that the core stores, copies and serializes but never parses — with ONE
 * exception: the compositor recognizes the {"type":"adjust", ...} shape
 * (schema in core/src/adjust.rs; query with dr_doc_layer_is_adjustment below)
 * and composites such a layer as a color adjustment of the backdrop instead
 * of as pixels. For every other blob its meaning, its schema and its
 * versioning belong entirely to the host — the core only guarantees the bytes
 * come back exactly as they went in. Doctored uses it to keep the parameters
 * a layer's pixels were rendered from (a text layer's string, font, size,
 * color, alignment), making the raster a cache of a description the host can
 * re-render.
 *
 * Lifetime rules, all of them consequences of ONE principle — metadata is
 * attached to a layer's identity, not to its pixel values:
 *   - It rides along wherever the layer survives as itself: the pure per-layer
 *     setters, pixel replacement, painting/fill/gradient/clear, mask
 *     operations, duplicating (the copy gets it too), reordering, adding and
 *     removing layers, whole-document rotate/flip/crop/canvas-resize/resize.
 *   - It is DROPPED exactly where a layer stops being itself, alongside the
 *     mask: dr_doc_merging_down clears it — and the layer style — on the
 *     merged layer (the pixels are now two layers' worth, so nothing
 *     describes them) and dr_doc_flattening produces one plain "Background"
 *     layer with none.
 *   - It round-trips through dr_doc_save_native / dr_doc_open in the
 *     version-2 DRDC format, per layer. Older readers see a plain raster
 *     layer, which is the intended graceful degradation.
 * The host stays responsible for dropping metadata that its own rules say a
 * destructive edit invalidates — the core has no opinion, because it cannot
 * read the blob.
 *
 * Re-rendering a layer in place needs no special entry point: every operation
 * is pure, so chain dr_doc_set_layer_content (or
 * dr_doc_set_layer_content_preserving_mask) and dr_doc_with_layer_meta —
 * after dr_doc_transform_layer when the re-render follows a transform: that
 * moves the mask and scales the style, and the host's own rendering replaces
 * the resampled pixels — and commit only the final handle: one edit, one
 * undo step. */

/* Layer idx's metadata as a heap string freed with dr_string_free; NULL on
 * out-of-range idx or a layer with no metadata (so NULL means "none", not an
 * error). Interior NUL bytes, which only a hand-crafted DRDC file could carry,
 * come back as spaces — the price of a C string. */
char *dr_doc_layer_meta(const DrDocument *doc, size_t idx);

/* Replaces layer idx's metadata with a copy of `meta`, or CLEARS it when
 * `meta` is NULL. `meta` must be valid UTF-8 of at most 16 MiB (16777216
 * bytes) — the DRDC writer's own cap, enforced here so a document can never
 * hold metadata the format would later refuse to store. NULL on out-of-range
 * idx, invalid UTF-8 (refused, never lossily converted), or an over-long
 * payload. */
DrDocument *dr_doc_with_layer_meta(const DrDocument *doc, size_t idx,
                                   const char *meta);

/* Whether layer idx's metadata parses as a color-adjustment description
 * ({"type":"adjust", ...} — schema in core/src/adjust.rs), i.e. whether the
 * compositor treats the layer as an ADJUSTMENT LAYER: its own pixels are
 * ignored and its contribution is the adjustment applied to the accumulated
 * backdrop, pushed through its blend mode and scaled by opacity * mask
 * coverage (an unmasked adjustment layer reaches the whole canvas; alpha is
 * never changed). False on out-of-range idx, absent meta, or meta that does
 * not parse as an adjustment (such a layer composites as plain raster), and
 * ALWAYS false on a GROUP: the core never interprets a group's metadata, so a
 * blob that happened to parse cannot turn a group into an adjustment. */
bool dr_doc_layer_is_adjustment(const DrDocument *doc, size_t idx);

/* Replaces layer idx's pixels, offset and mask in ONE pure step — the
 * re-render primitive for a described layer whose raster, position and mask
 * change together (a text layer rotated by its own description; a shape
 * refitted under a transform). `src` is straight RGBA8, w*h*4 bytes, row 0
 * top; `mask` is NULL (the layer ends with no mask; mask_enabled resets to
 * true) or exactly w*h coverage bytes at the pixels' size (mask_enabled is
 * kept, so a disabled mask stays disabled). Name, opacity, blend mode,
 * visibility, metadata, style and clipped flag survive. NULL on out-of-range
 * idx, NULL src, w == 0, h == 0, or w*h > 100000000 — dimensions bounded
 * before anything is read, as for dr_image_from_rgba. */
DrDocument *dr_doc_set_layer_content(const DrDocument *doc, size_t idx,
                                     const uint8_t *src, uint32_t w,
                                     uint32_t h, int32_t x, int32_t y,
                                     const uint8_t *mask);

/* ---- Layer styles -------------------------------------------------------
 *
 * Every layer carries an optional LAYER STYLE: an effect stack plus
 * the blending options beyond opacity and mode — fill opacity (the opacity
 * of the pixels only, never of the effects) and Blend If (two split-slider
 * ramps, on this layer and on the composite beneath, weighting the layer's
 * alpha). The effects are functions of the layer's SHAPE (its alpha times
 * its enabled mask) — a GROUP may carry a style, whose shape is then its own
 * rendered projection, and an adjustment layer may not — and are rendered by
 * the projection only:
 * dr_doc_layer_image, dr_doc_layer_canvas_image and the thumbnails stay
 * effect-free, exactly as they stay mask-free.
 *
 * The style crosses this boundary as ONE JSON object — the contract:
 *   {"version": 1, "fill_opacity": 1.0, "blend_if": null,
 *    "effects": [{"type": "drop_shadow", ...}, ...]}
 * The ten effect types are drop_shadow, inner_shadow, outer_glow,
 * inner_glow, stroke, pattern_overlay, color_overlay, gradient_overlay,
 * bevel_emboss and satin — up to ten instances of drop_shadow, inner_shadow,
 * stroke, color_overlay and gradient_overlay, one of each other; the render
 * order (bottom up) is drop shadow, outer glow, stroke (outside), pattern
 * overlay, gradient overlay, colour overlay, satin, inner glow, inner shadow,
 * stroke (inside), bevel. pattern_overlay tiles a "pattern" — a
 * {"preset": id} of dr_pattern_presets_json, or {"name", "width", "height",
 * "rgba"} with base64 straight RGBA8 and sides 1..512 — at "scale"
 * 0.01..10, "phase" [x, y] px, "link_with_layer" (default true: the tile
 * starts at the canvas origin + phase when the style is set and then moves
 * with the layer; the setter records the layer's offset as "anchor", which a
 * caller echoes back to keep the tile or omits to re-anchor); it is refused
 * in a 32-bit float or CMYK document. Every effect has "enabled" (default true; a
 * disabled effect is kept but not rendered), its own blend mode ("blend",
 * the 27 layer blend modes as snake_case names: normal, multiply, screen,
 * linear_dodge, ...) and opacity. Colours are "#rrggbb"; angles are degrees
 * (0 = light from the right, 90 = from the top; the default 120° casts
 * down-right); sizes are canvas pixels; "spread" /
 * "choke" are fractions of "size". The full key table with every default
 * lives in core/src/style.rs. On input an unknown key is refused by name
 * ("style: unknown layer style field effects[0].blend_mode"), missing
 * keys take their defaults, numeric ranges are clamped, and a wrong JSON
 * type, unknown enum value, unknown effect type, duplicate type or
 * out-of-order Blend If ramp is an error naming the key. Numbers are
 * canonicalized to 4 decimals; dr_doc_layer_style returns the canonical
 * form (every key of every present effect, effects in render order, sorted
 * keys), which is also what the DRDC file stores.
 *
 * WIDE COLOURS: beside "color" on drop_shadow, inner_shadow,
 * outer_glow, inner_glow, stroke, color_overlay and satin a slot may carry
 * "linear" (on bevel_emboss "highlight_linear" / "shadow_linear"): [r, g, b],
 * the colour as LINEAR-LIGHT sRGB-primaries components, extended range
 * (negative outside sRGB's gamut), each clamped into -4..4 and NOT quantized,
 * so it round-trips exactly; anything but three finite numbers is an error
 * naming the key. One that is an sRGB byte is dropped and "color" becomes
 * that byte; otherwise "color" is replaced by its nearest sRGB byte. Written
 * only when present. The composite carries it into the document's numbers at
 * 8, 16 and 32 bits (P3 or ROMM documents and 16-bit picks keep it); a CMYK
 * document reads the slot's ink or "color". A document holding one saves
 * conditional DRDC word 100 at both depths (no table); older builds refuse
 * the file by name.
 *
 * Two more BLENDING OPTIONS sit beside fill_opacity and blend_if, and are
 * the ONE exception to "every key is written": each is emitted only when it
 * is NOT at its default, so a style authored without them is byte-identical
 * to what the writer before them produced.
 *   "knockout": "none" | "shallow" | "deep"   (default "none")
 *   "blend_interior_as_group": bool            (default false)
 * KNOCKOUT makes the layer's own shape punch a hole through a BACKDROP
 * instead of compositing onto it; where that hole stops is the whole rule.
 * The layer's own pixels, its Blend If weight and its fill opacity decide
 * how much of the hole is refilled (a pixel Blend If hides still punches:
 * Blend If is opacity, never shape), and the SHAPE decides how much of the
 * result is mixed back in — so a soft edge gets no fringe. Below effects are
 * untouched by it and layer opacity still applies to the whole package.
 * The backdrop: an ISOLATED group's buffer is a HARD STOP for both modes
 * (the hole shows what lies beneath the GROUP once it composites); a clipped
 * MEMBER punches to its clip base alone; Shallow inside a pass-through group
 * punches to the accumulator its children began with; Deep punches to the
 * Background layer — entry 0 when it is a plain, visible, unmasked,
 * unstyled, Normal, opacity-1, canvas-sized raster at (0, 0) with no clipped
 * entry above it — and to transparency when there is none. Shallow at the
 * root IS Deep. "blend_interior_as_group" moves the five GROUPED interior
 * effects (pattern overlay, gradient overlay, colour overlay, satin, inner
 * glow) inside that
 * unit, so the LAYER's blend mode applies to them; inner shadow, an
 * inside/centre stroke and the bevel stay outside it. On a clip base with
 * visible members the flag is kept but evaluates off.
 *   MERGES: dr_doc_flattening, dr_doc_stamp_visible and
 * dr_doc_merge_visible render the document's own projection, so knockout
 * bakes EXACTLY in all three. dr_doc_merging_down and dr_doc_merge_layers
 * bake into a private accumulator, but they now CAPTURE the backdrop the
 * merged unit would have read from the document's own projection and hand it
 * to that accumulator, so the hole is filled with the real backdrop's pixels
 * and the picture is kept wherever that backdrop is opaque. The STRUCTURAL
 * residual: where no backdrop exists at all — the knockout sits inside an
 * ISOLATED group (a hard stop for both modes), inside a pass-through group
 * that is itself being merged, is clipped to a base outside the merge, or
 * punches to the ROOT of a document with no qualifying Background layer —
 * the hole bakes as TRANSPARENCY and the merged layer then shows whatever
 * lies beneath IT. The PSD export bakes with a transparent backdrop too.
 * Since 12E-3c BOTH keys are WRITTEN to a PSD as Photoshop's own `knko` and
 * `infx` records at BOTH depths, on rasters and on folder records, whenever
 * the layer's style is written rather than rasterized and the key is not at
 * its default; a rasterized (baked) style writes neither and says so per
 * layer. A file whose bottom layer is Doctored's structural Background but
 * not a true Photoshop Background carries a per-file loss in BOTH the import
 * and the export report (D13, carryover P7-38): Photoshop punches such a
 * knockout through to transparency where Doctored shows that bottom layer.
 * A byte document whose style uses either key saves as DRDC
 * version 49, a word nested inside 45 with no table of its own; a TYPED one
 * saves as DRDC version 50, nested inside 46 after 47 and 48, also with no
 * table. Older builds refuse such a file by name rather than dropping the
 * keys. Both keys are SUPPORTED at 16 bits and refused by name at 32-bit
 * float, and under the linear_light evaluation policy through its existing
 * style refusal.
 *
 * Identity rule: a style that renders nothing (fill opacity 1, no Blend If,
 * knockout "none", no enabled effect) CLEARS — it is never stored — so
 * dr_doc_layer_has_style true means the layer renders something.
 * "blend_interior_as_group" alone is NOT enough to keep a style: without a
 * grouped effect it renders nothing, so such a style clears too.
 *
 * Lifetime: the style rides along wherever the layer survives as itself
 * (exactly like metadata above); dr_doc_duplicating_layer copies it;
 * dr_doc_merging_down and dr_doc_flattening BAKE the effects into the pixels
 * and drop it; dr_doc_transform_layer, dr_doc_perspective_layer and
 * dr_doc_resize apply "Scale Effects" (the pixel-valued fields follow the
 * mean scale factor). Adjustment layers ignore styles (they have no shape).
 *
 * Render order: the drop shadow, outer glow and an outside
 * stroke composite BELOW the pixels, each with its own blend mode and
 * opacity; the pixels composite at fill opacity (times the Blend If weight)
 * with the LAYER's blend mode; then gradient overlay, colour overlay, satin,
 * inner glow, inner shadow, an inside/centre stroke and the bevel composite
 * above them, each with its own blend mode ("Blend Interior Effects as
 * Group" is off, so a Multiply layer with a white colour overlay shows
 * white). Blend If reads the composite BENEATH the layer, not the layer's
 * own shadow. A styled clip base renders its shadow once under the whole
 * group, and clipped members are confined to the base's shape.
 *
 * Effects with "use_global_light" read the document's GLOBAL LIGHT (angle,
 * altitude in degrees; defaults 120°, 30°) instead of their own angle. The
 * style and the light round-trip through dr_doc_save_native / dr_doc_open:
 * they are what bumped the DRDC format to version 4. */

/* Replaces layer idx's style with a copy of `style_json` (parsed and
 * canonicalized as described above), or CLEARS it when `style_json` is
 * NULL — and also clears it when the parsed style is an identity. Two
 * failure tiers: NULL with a message through err_out (free with
 * dr_string_free) when doc is NULL ("document is NULL"), the string is not
 * valid UTF-8, not valid JSON, not a valid style (the message names the
 * offending key), or over 16 MiB (the DRDC writer's cap, shared with
 * metadata); NULL with NO message — a refusal, not an error — on an
 * out-of-range idx or a value equal to the current one ("no style"
 * included), so the host never registers a phantom undo step. */
DrDocument *dr_doc_set_layer_style(const DrDocument *doc, size_t idx,
                                   const char *style_json, char **err_out);

/* Layer idx's style as its canonical JSON, a heap string freed with
 * dr_string_free; NULL on NULL doc, out-of-range idx, or a layer with no
 * style (so NULL means "none", not an error). */
char *dr_doc_layer_style(const DrDocument *doc, size_t idx);

/* Whether layer idx carries a style — the cheap badge query; false on NULL
 * doc or out-of-range idx. Because identity styles are never stored, true
 * means the layer renders something. */
bool dr_doc_layer_has_style(const DrDocument *doc, size_t idx);

/* Pure setter for the document's global light, in degrees. NULL on NULL doc,
 * a non-finite component, or no change after sanitizing (altitude clamped to
 * [0, 90], angle normalized to [-180, 180), both quantized to four decimals
 * like every style number, so a reported value echoed back is "no change").
 * The typed twin is the {op:"global_light"} dr_precision_doc_edit request. */
DrDocument *dr_doc_set_global_light(const DrDocument *doc, float angle,
                                    float altitude);

/* The global light's components in degrees; 0.0 on NULL doc. */
float dr_doc_global_light_angle(const DrDocument *doc);
float dr_doc_global_light_altitude(const DrDocument *doc);

/* The four frozen Pattern Overlay presets as JSON, in popup order:
 * [{"preset", "name", "width", "height", "rgba"}], "rgba" the standard
 * base64 of the tile's straight RGBA8 bytes. Heap, freed with
 * dr_string_free; NULL with a message through err_out only on an internal
 * error. */
char *dr_pattern_presets_json(char **err_out);

/* Style PRESETS (the Styles panel). A preset's style is the style
 * JSON above parsed strictly with every CMYK "ink" and every pattern-fill
 * "anchor" dropped, written canonically: dr_style_preset_canonical returns
 * that form (NULL with a message through err_out on a NULL, non-UTF-8,
 * oversized (16 MiB), invalid-JSON or invalid style; the message names the
 * offending key). dr_style_preset_defaults_json returns the twelve built-in
 * presets as [{"name", "style"}] in panel order, each style canonical (NULL
 * with a message only on an internal error). Both heap, freed with
 * dr_string_free. */
char *dr_style_preset_canonical(const char *style_json, char **err_out);
char *dr_style_preset_defaults_json(char **err_out);

/* Decodes the len bytes at bytes as a Photoshop style library (.asl) and
 * returns {"styles": [{"name": s, "style": object | null, "blending":
 * {"mode": blend name, "opacity": 0..1} | null, "dropped": [s]}]} (heap,
 * freed with dr_string_free). Each style is canonical as a preset's, null
 * when nothing Doctored models is left; "dropped" names each effect or
 * blending option it could not keep ("Bevel & Emboss: <reason>",
 * "Blending Options: <key>"). Blend If's "Blnd" maps to the style's
 * "blend_if" when exactly one entry (Gray, Red, Green or Blue) is not the
 * full ramp, else it is named. An RGBC colour off the sRGB byte grid reads
 * as the slot's "linear". NULL bytes with len 0 is an empty file. NULL
 * with a message through err_out on NULL bytes with a non-zero len, len over
 * 64 MiB (refused before the bytes are read), more than 1000 styles, or a
 * file that is not a style library or is truncated; never panics. */
char *dr_asl_decode(const uint8_t *bytes, size_t len, char **err_out);

/* Encodes style presets as a Photoshop style library (.asl): presets_json is
 * [{"name": s, "style": object, "blending": {"mode", "opacity"} | null}];
 * returns {"asl": "<standard base64 of the file>", "skipped": ["<name>:
 * <reason>"]} (heap, freed with dr_string_free). Each style is written
 * canonical as a preset's, every effect at its own stored angle and
 * altitude, colours as sRGB (a wide colour as its unquantized sRGB numbers,
 * clamped outside sRGB's gamut), its fill opacity, knockout, Blend Interior
 * Effects as Group and Blend If ("Blnd") as blending options; a preset that
 * cannot be written (a style that does not parse, a setting Photoshop's
 * dialog cannot hold, Blend If on a CMYK plate) is named in "skipped".
 * dr_asl_decode reads every file this writes. NULL with a message through
 * err_out when presets_json is NULL, not UTF-8 or not valid JSON, holds more
 * than 1000 presets, leaves nothing to write, or the file would pass
 * 64 MiB; never panics. */
char *dr_asl_encode(const char *presets_json, char **err_out);

/* ---- Layer groups, locks, links and structure ---------------------------
 *
 * The layer stack is a FOREST, flattened. Every entry carries a KIND (raster
 * or group), a nesting DEPTH (0 at the top level) and — new with them — lock
 * flags, a link-group id and, for a group, a panel disclosure flag. A GROUP's
 * children are the entries immediately BELOW it at greater depth, and the
 * group's own record is the LAST record of its own subtree, so the array
 * stays bottom-first and children come before the group that holds them:
 *
 *     index 0  Layer 0   depth 0
 *     index 1  Layer A   depth 1   |
 *     index 2  Layer B   depth 1   |  children of Group 1
 *     index 3  Group 1   depth 0
 *
 * That is the layers panel's order read bottom-up and exactly how PSD stores
 * it. Every existing export still addresses an entry by its flat index, and
 * dr_doc_layer_count still counts ENTRIES, groups included. A group has no
 * pixels of its own: dr_doc_layer_image is NULL for one, every pixel writer
 * refuses one, and dr_doc_layer_offset_x/y/width/height answer the union of
 * its raster descendants' buffer rects instead of a pixel rect of its own.
 *
 * COMPOSITING. A group renders ISOLATED — its children into a private buffer
 * over their own extent, quantized once and composited as a single layer with
 * the group's opacity, blend mode, mask, style and clipped flag — when
 * anything about it needs its own RENDERED PIXELS or its own footprint: any
 * blend mode other than DR_BLEND_PASS_THROUGH, a style, its own clipped flag,
 * or a VISIBLE clipped layer above it. A plain PASS-THROUGH group (the
 * default for a new group) allocates nothing: its children composite straight
 * onto the backdrop below the group, which is what lets an adjustment layer
 * inside a group reach the layers beneath it. Note the consequence: CLIPPING
 * any layer to a pass-through group makes that group composite as a unit,
 * and an adjustment layer inside it then stops reaching the layers below
 * the group.
 *
 * A pass-through group's own MASK and OPACITY do NOT isolate it: they GATE it
 * per pixel instead. Its children composite onto a copy of the real backdrop
 * and the result is mixed back by mask/255 * opacity, so an adjustment layer
 * inside a masked group is RESTRICTED to where the mask is white (and scaled
 * by the opacity) rather than switched off — "group the adjustment layers and
 * mask the group" works, and a reveal-all mask at opacity 1 is byte-identical
 * to no mask at all. Isolating for them would composite the adjustment
 * against a fresh transparent buffer, where an adjustment never touches a
 * pixel, and the effect would disappear entirely.
 *
 * Nesting is arbitrary up to ten levels; deeper is refused, and a projection's
 * group buffers are additionally capped so a crafted file cannot exhaust
 * memory (a group whose buffer would pass the cap simply contributes
 * nothing).
 *
 * CLIPPING is re-derived WITHIN a level: a clipped entry clips to the first
 * unclipped SIBLING below it, never across a group boundary, and a clipped
 * entry at the bottom of its level has no base and composites as if
 * unclipped — a GROUP there keeps passing through, since a flag the
 * compositor ignores never forces isolation either. So clipping a group to a
 * sibling that does not exist, or sending an already-clipped group to the
 * back of its level, changes no picture.
 *
 * A GROUP's MASK is CANVAS-sized, unlike a layer's, which is always its
 * layer's own size — a group has no pixel buffer to be the size of, and
 * a PSD's group masks are canvas space too. dr_doc_adding_layer_mask
 * creates one at the canvas size for all three kinds,
 * dr_doc_painting_layer_mask validates against the canvas, and
 * dr_doc_removing_layer_mask with apply == true is REFUSED on a group (there
 * are no pixels to bake into). The whole-document geometry ops carry a group
 * mask the way they carry a channel: cut down by crop, padded by canvas
 * resize, resampled to the new canvas by resize. A group mask also TRAVELS
 * WITH ITS GROUP: dr_doc_move_layers and dr_doc_with_layer_offset slide it by
 * the move's delta and dr_doc_geometry_edit's affine_set warps it through
 * the same affine, so a masked group behaves identically whichever gesture
 * moved it.
 * A TRANSLATE moves it as BOOKKEEPING — the group entry's own offset, which
 * has no other meaning on a group — and never as a resample, so a Move drag
 * that goes out and comes back, or two opposite arrow nudges, restore the
 * mask exactly rather than eating a band of it per step. The plane is
 * resampled once, by the next op that needs the mask as a canvas plane.
 *
 * LOCKS are a bitmask (DrLockFlags). Transparency freezes the layer's alpha
 * channel — a stroke changes colour but never coverage, and an eraser cannot
 * punch a hole. Pixels refuses every pixel edit but leaves the MASK editable
 * — except APPLYING one (DR_EDIT_MASK_APPLY), which bakes
 * the coverage into the layer's alpha and so answers to Transparency too.
 * Position refuses offset changes, moves, transforms and
 * perspective; on a group it also consults every descendant's bit, because
 * moving a group moves them. All three set is "Lock All", which additionally
 * freezes the mask. A MERGE (DR_EDIT_MERGE) answers to the DESTINATION
 * entry's Pixels and Transparency bits: it replaces that entry's whole
 * picture at a new extent. A TRANSFORM is a POSITION edit and never a Pixels one: it
 * resamples the whole buffer including its alpha, so a frozen alpha channel
 * has no meaning there, and a transparency-locked layer stays transformable.
 * Locks never block delete, duplicate, reorder, group/ungroup, rename,
 * opacity, blend mode, visibility or style.
 *
 * LINKS are a group id on each entry (0 = unlinked): every entry sharing a
 * non-zero id moves and transforms with the others. dr_doc_with_layer_offset
 * deliberately does NOT follow links — it is a property write, not a move;
 * dr_doc_move_layers and dr_doc_geometry_edit's affine_set / perspective_set
 * do.
 *
 * Groups, locks, links and the disclosure flag round-trip through
 * dr_doc_save_native: they are what bumped the DRDC format to version 7. */

typedef enum {
  DR_LAYER_RASTER = 0,
  DR_LAYER_GROUP = 1,
} DrLayerKind;

/* A uint32_t bitmask. Bits 3..31 are reserved and are masked off wherever a
   value enters. */
typedef enum {
  DR_LOCK_TRANSPARENCY = 1,
  DR_LOCK_PIXELS = 2,
  DR_LOCK_POSITION = 4,
  DR_LOCK_ALL = 7,
} DrLockFlags;

typedef enum {
  DR_EDIT_PIXELS = 0,
  DR_EDIT_POSITION = 1,
  DR_EDIT_MASK = 2,
  /* Applying a layer mask: a mask op that also bakes the coverage into the
     layer's ALPHA, so Lock Transparency refuses it as well as Lock All. */
  DR_EDIT_MASK_APPLY = 3,
  /* A merge writing this entry: its whole picture is replaced, at a new
     extent. Lock Pixels and Lock Transparency both refuse it. */
  DR_EDIT_MERGE = 4,
} DrEditKind;

typedef enum {
  DR_ARRANGE_FRONT = 0,
  DR_ARRANGE_FORWARD = 1,
  DR_ARRANGE_BACKWARD = 2,
  DR_ARRANGE_BACK = 3,
} DrArrange;

typedef enum {
  DR_ALIGN_LEFT = 0,
  DR_ALIGN_CENTER_X = 1,
  DR_ALIGN_RIGHT = 2,
  DR_ALIGN_TOP = 3,
  DR_ALIGN_CENTER_Y = 4,
  DR_ALIGN_BOTTOM = 5,
} DrAlign;

/* --- the entry model (getters and pure setters) --- */

/* True when entry idx is a GROUP; false on NULL doc, out-of-range idx or a
 * raster layer. */
bool dr_doc_layer_is_group(const DrDocument *doc, size_t idx);

/* The entry's nesting depth (0 at the top level); 0 on NULL doc or
 * out-of-range idx. */
uint32_t dr_doc_layer_depth(const DrDocument *doc, size_t idx);

/* The entry's subtree as the half-open range [*out_start, *out_end): the
 * entry itself for a raster layer, the whole group for a group. *out_end is
 * exactly where a new sibling inserted "above" this entry lands, which is
 * what dr_doc_adding_layer_at's DR_INSERT_ABOVE, dr_doc_adding_image_layer
 * and dr_doc_duplicating_layer do — so a host that must select what it just
 * created asks this rather than assuming idx + 1. Either out pointer may be
 * NULL. False (nothing written) on NULL doc or out-of-range idx. */
bool dr_doc_layer_subtree(const DrDocument *doc, size_t idx,
                          size_t *out_start, size_t *out_end);

/* The entry's lock flags (a DrLockFlags bitmask); 0 on NULL doc or
 * out-of-range idx. */
uint32_t dr_doc_layer_locks(const DrDocument *doc, size_t idx);

/* Pure setter: replaces the entry's lock flags, reserved bits masked off.
 * NULL on NULL doc or out-of-range idx. */
DrDocument *dr_doc_with_layer_locks(const DrDocument *doc, size_t idx,
                                    uint32_t locks);

/* Which of the entry's lock bits would block an edit of `kind` (0 = allowed)
 * — the query a host asks to phrase a refusal that NAMES the lock. The ops
 * refuse regardless, so a host that never asks cannot get through. Enclosing
 * groups count: the answer ORs the entry's own bits with every
 * enclosing group's, and Lock All on any entry in that chain freezes the
 * mask. 0 on NULL doc, out-of-range idx or an unknown kind. */
uint32_t dr_doc_lock_block(const DrDocument *doc, size_t idx, DrEditKind kind);

/* The entry whose OWN lock bits hold every bit of `locks` (masked to
 * DR_LOCK_ALL): idx itself, else its nearest enclosing group that does, so a
 * refusal can say "inside group G". False, with nothing written, on NULL doc,
 * an empty `locks`, an out-of-range idx, or when no entry in the chain holds
 * them. out_idx may be NULL. */
bool dr_doc_lock_owner(const DrDocument *doc, size_t idx, uint32_t locks, size_t *out_idx);

/* The entry's link-group id (0 = unlinked); 0 on NULL doc or out-of-range
 * idx. */
uint32_t dr_doc_layer_link(const DrDocument *doc, size_t idx);

/* Whether GROUP idx is shown expanded in the layers panel; false on NULL doc
 * or out-of-range idx. Meaningless on a raster entry, which answers true. */
bool dr_doc_layer_open(const DrDocument *doc, size_t idx);

/* Pure setter: expands or collapses GROUP idx. NULL on NULL doc,
 * out-of-range idx or a raster entry. The high-depth twin is the
 * {op:"layer_open"} dr_precision_doc_edit request. */
DrDocument *dr_doc_with_layer_open(const DrDocument *doc, size_t idx,
                                   bool open);

/* The entry's CONTENT bounds — the canvas box of its OPAQUE pixels with its
 * enabled mask applied, and for a group the union over its raster
 * descendants — written to out_xywh as x, y, width, height. This is the rect
 * dr_doc_align_layers and dr_doc_distribute_layers act on, and it is NOT the
 * pixel rect dr_doc_layer_offset_x/width report for a raster layer: a
 * photo-shaped layer on a canvas-sized buffer has two different rectangles.
 * Not clipped to the canvas, and blind to the visible flag (this says what
 * the entry HOLDS). False (nothing written) on NULL doc, out-of-range idx, or
 * an entry with nothing opaque in it. */
bool dr_doc_layer_bounds(const DrDocument *doc, size_t idx, int32_t *out_xywh);

/* A Gradient Fill layer's Align with Layer box: the bounds of entry
 * idx's mask coverage (raw samples above zero) moved to the canvas and
 * clipped to it, when the mask is enabled and its outside coverage is 0;
 * else, or when that leaves nothing, the canvas. Density and feather do not
 * move it. Written to out_xywh as x, y, width, height; the box the host
 * passes as an aligned gradient component's "box". False (nothing written)
 * on NULL doc or an out-of-range idx; out_xywh may be NULL. */
bool dr_doc_fill_align_box(const DrDocument *doc, size_t idx, int32_t *out_xywh);

/* --- structure --- */

/* Wraps the given entries — which must all share ONE parent — in a new group
 * named `name`, whose index comes back through out_group. The subtrees are
 * gathered in their existing relative order and the group is inserted so its
 * subtree occupies the TOPMOST given entry's slot; the gathered depths shift
 * by +1. The new group is Pass Through, opacity 1, visible, open, unclipped,
 * with no mask, style, locks or link.
 *
 * Two things the caller must be able to report come back alongside it, as NEW
 * indices, in the optional buffers out_cleared_clip and out_reordered (each
 * out_cap size_t entries; their true lengths always come back through
 * out_cleared_len / out_reordered_len, so a short buffer is detectable and a
 * buffer of dr_doc_layer_count entries can never truncate):
 *   - the BOTTOM-MOST grouped entry has its clipped flag CLEARED when it was
 *     set, since nothing inside the group is below it to clip to (without
 *     it the entry would silently composite as if unclipped);
 *   - a NON-CONTIGUOUS set gathers the subtrees into the topmost given
 *     entry's slot, so entries left between them change their relative
 *     position (grouping {A, C} out of [A, B, C] leaves B below A).
 *
 * NULL on NULL doc or name, an empty / out-of-range / repeating index list,
 * entries that do not share one parent, and a nesting past ten levels. */
DrDocument *dr_doc_group_layers(const DrDocument *doc, const size_t *idx,
                                size_t len, const char *name,
                                size_t *out_group, size_t *out_cleared_clip,
                                size_t *out_cleared_len, size_t *out_reordered,
                                size_t *out_reordered_len, size_t out_cap);

/* Dissolves group idx: its children take its depth and its slot, in order,
 * and the group entry is removed. The group's own mask, style, opacity, blend
 * mode and clipped flag are DISCARDED — they cannot be expressed on the
 * children — so a caller that wants to report the loss must read them first.
 *
 * out_cleared_clip / out_cleared_len / out_cap are the same optional report
 * dr_doc_group_layers takes, and the mirror image of it: inside the group the
 * bottom-most child was at the BOTTOM of its level, so a clipped flag on it
 * was baseless and composited as unclipped; out at the parent level it could
 * land above an unclipped sibling and suddenly clip to it. An entry that was
 * baseless therefore STAYS baseless — the flag is cleared — and the entry's
 * NEW index comes back here so the caller can say so. At most one entry.
 *
 * NULL on NULL doc, out-of-range idx, a raster entry, or when the document
 * would be left with no entries at all. */
DrDocument *dr_doc_ungroup_layer(const DrDocument *doc, size_t idx,
                                 size_t *out_cleared_clip,
                                 size_t *out_cleared_len, size_t out_cap);

/* THE structural move: the entry at `from` and its whole subtree land at
 * index `to` — an index into the stack with that subtree already taken out,
 * the numbering a panel drag onto a row uses — at `depth`. This is how a
 * layer moves INTO or OUT OF a group.
 *
 * There is deliberately no "not into your own subtree" refusal: because `to`
 * numbers the stack with the subtree ALREADY removed, every value in range is
 * a legal insertion point and landing inside the moved block is impossible by
 * construction. A guard written in the PRE-drain numbering would refuse the
 * commonest panel drag — a group dropped one place up over a smaller sibling.
 * `to` past the end is clamped to the end.
 *
 * NULL on NULL doc, an out-of-range index, a depth past ten levels, any
 * (to, depth) pair whose result would be a malformed structure, and a move
 * that would leave the stack exactly as it is (the core's no-op rule, so a
 * drag that put an entry back where it started mints no undo step). */
DrDocument *dr_doc_move_layer_to(const DrDocument *doc, size_t from, size_t to,
                                 uint32_t depth);

/* Moves entry idx among its SIBLINGS only — never into or out of a group. To
 * change an entry's level use dr_doc_move_layer_to. NULL on NULL doc,
 * out-of-range idx, or when the entry is already there. */
DrDocument *dr_doc_arrange_layer(const DrDocument *doc, size_t idx,
                                 DrArrange how);

/* Links the given entries: they take the smallest unused non-zero link id, so
 * a save round trip is byte-identical. Unlink clears theirs, and an id left
 * with a single member is cleared too (a link group of one is not a link).
 * NULL on NULL doc or an empty / out-of-range / repeating index list, and
 * when nothing would change. */
DrDocument *dr_doc_link_layers(const DrDocument *doc, const size_t *idx,
                               size_t len);
DrDocument *dr_doc_unlink_layers(const DrDocument *doc, const size_t *idx,
                                 size_t len);

/* --- set operations ---
 *
 * Each takes an explicit index list and makes ONE document, never a sequence
 * of single-entry calls: every structural op renumbers, so a host loop would
 * address the wrong entries after the first step. The set is expanded by
 * SUBTREE and then by LINK GROUP, deduplicated; each call is all-or-nothing.
 * NULL on NULL doc, an empty / out-of-range / repeating list, a lock that
 * forbids the edit, and — the core's own rule — when nothing would change. */

/* Translates every given entry by (dx, dy). The ONE move-a-set op: the Move
 * tool, the arrow-key nudge and align/distribute all go through it. A GROUP
 * has no offset of its own — its raster descendants are in the expanded set
 * and move on their own — but its canvas-sized MASK slides by the same delta,
 * so a masked group drags exactly as it transforms. */
DrDocument *dr_doc_move_layers(const DrDocument *doc, const size_t *idx,
                               size_t len, int32_t dx, int32_t dy);

/* Strict bounded JSON geometry, including atomic affine_set and
 * perspective_set. A NULL edit result with no error is unchanged. The typed
 * affine_set's follow_links:false is refused here: dr_doc_with_layer_offset is
 * the byte no-link offset.
 *
 * The byte STRAIGHTEN (the Crop tool's straighten) is the request
 *   {"op":"straighten","matrix":[a,b,c,d,tx,ty],"filter":F,
 *    "x":X,"y":Y,"width":W,"height":H}
 * — the same affine applied to the WHOLE stack, every mask included, then
 * every saved CHANNEL turned by the same matrix (the channels stay
 * canvas-sized; coverage rotated off the canvas is dropped and destinations
 * with no source read 0), then the crop to (X, Y, W, H). Per-entry POSITION
 * locks do NOT gate it: straightening re-frames the picture rather than
 * moving a layer within it, which is why dr_doc_crop, dr_doc_geometry,
 * dr_doc_canvas_resize and dr_doc_resize do not consult a layer lock either.
 * The FULL window (0, 0, canvas width, canvas height) is the straighten alone,
 * with no crop; an identity matrix at the full window is NULL with no error,
 * and an identity matrix at an inner window is exactly dr_doc_crop's result,
 * with no resample. X and Y are SIGNED and the window may lie past
 * any canvas edge (Crop past the canvas): an identity matrix there is exactly
 * dr_doc_canvas_resize(W, H, (-X, -Y)), a turned one straightens the stack and
 * resamples each saved channel ONCE straight into the window (0 where the old
 * canvas had none), and the new area of every layer is transparent. Optional
 * "background":{"color":[r,g,b],"color_space":"document"} (0...1, document
 * space only at 8 bits, else "an 8-bit crop background takes color_space
 * document"; "ink" on an RGB document is "crop background takes color on an
 * RGB document and ink on a CMYK one") with "delete_cropped":true pads the
 * Background first — layer 0 when it is D5's Background (visible, Normal,
 * 100 %, unmasked, unstyled, canvas-sized at (0, 0), no clipped layer above),
 * a plain raster and fully OPAQUE — with that colour over the window mapped
 * back through the matrix (4 px further on a turned one), its comps following
 * the moved origin; nothing else is padded, and with Delete Cropped Pixels off
 * nothing is (the Background becomes a layer, take1's R2). The plan charges
 * the padded document plus the pad. Refusals before any work, each by its
 * sentence: an edge outside the signed 32-bit range ("the crop window's edges
 * must lie within the signed 32-bit range"), a zero-size or over-limit window
 * (the typed op's "precision prototype dimensions exceed the supported pixel
 * limit"), saved channels over their budget at the window's size ("the saved
 * alpha channels do not fit the extended canvas: delete some first") and a
 * pad past the pixel limit, the i32 range or the save cap ("the crop reaches
 * too far past the canvas for the Background to grow: choose a window nearer
 * the canvas"), so NULL with no error only ever means the whole canvas is
 * unchanged. A layer the transform refuses answers "byte straighten could
 * not transform all layers"; a stack with nothing to write (mask-less groups
 * and adjustment layers only) is the layers unchanged, and an inner window
 * still crops. Under a job control the layers are polled one by one and then
 * the channels, and a cancel answers "operation cancelled". Arbitrary canvas
 * rotation is the "rotate_canvas" request (see dr_canvas_rotation_plan).
 * The typed request's optional "delete_cropped" key is ignored here: the host
 * follows a byte straighten with dr_doc_delete_cropped_pixels. */
char *dr_doc_geometry_edit_plan(const DrDocument *document, const char *request, char **err_out);
// Persisted warp field: strict JSON, one layer, NULL/no error means unchanged.
// Returned strings are owned and freed with dr_string_free.
char *dr_doc_warp_info(const DrDocument *document, size_t layer, char **err_out);
char *dr_doc_warp_edit_plan(const DrDocument *document, const char *request, char **err_out);
DrDocument *dr_doc_warp_edit(const DrDocument *document, const char *request, uint64_t limit, uint64_t other_retained_bytes, const DrJobControl *control, char **err_out);
DrDocument *dr_doc_geometry_edit(const DrDocument *document, const char *request,
                                  uint64_t limit, uint64_t other_retained_bytes,
                                  const DrJobControl *control, char **err_out);
/* Perspective Crop rides both calls above, and dr_precision_doc_edit[_plan]
 * at 16 bits and float: {"op":"perspective_crop","quad":[x0,y0,...,x3,y3],
 * "width":W,"height":H,"filter":"nearest|bilinear|bicubic|lanczos3"}, the quad
 * TL, TR, BR, BL in source canvas pixels. The result canvas is W x H; every
 * raster (adjustments excepted), every mask and every saved channel is
 * inverse-mapped through the homography taking (0,0,W,H) onto the quad, and
 * a sample outside its source is transparent (masks read their outside
 * coverage, channels 0). Guides are cleared, the ruler origin returns to 0,0,
 * colour samplers follow the picture and drop off the canvas, styles scale by
 * sqrt(W*H / area(quad)); the typed op drops each resampled raster's artwork
 * description. Errors: a corner not finite or past 1e7; a concave, crossed
 * or collapsed quad; W or H 0 or W*H over the pixel cap; a layer with a
 * retained source ("Perspective Crop needs every layer as pixels: rasterize
 * RAW layers first."). The whole canvas at its own
 * size is NULL with no error. The plan is admitted before any work. */

/* Stack Modes: one per-pixel statistic across raster layers, as ONE new
 * raster. The request, here and as dr_precision_doc_edit[_plan]'s "stack_mode"
 * op, is {"op":"stack_mode","layers":[N,...],"above":N,
 * "mode":"mean|median|minimum|maximum|range|standard_deviation|summation|
 * variance|entropy|kurtosis|skewness|majority","name":S} —
 * every key REQUIRED, an unknown key refuses. "layers" are the members (two or
 * more, none twice, each a raster that is not a group or an adjustment layer);
 * "above" is the entry the result is inserted directly above (above its whole
 * subtree when it is a group, at its level, unclipped): any index in range
 * that is not below the highest member.
 * The extent is the union of the members' pixel rectangles, and a member has
 * alpha 0 outside its own. Per extent pixel, with A the full alpha: S0 = the
 * members with alpha > 0 (none: the pixel is transparent); the output alpha is
 * the largest alpha in S0 in every mode. Mean is per colour channel
 * sum(a*c)/sum(a) over S0. Every other mode reads S = the members with
 * 2*alpha >= A, or S0 when S is empty, unweighted per channel: median (the
 * middle value, or the mean of the two middle values), minimum, maximum,
 * range (max - min), standard deviation (population); with F the full value
 * (255, 65535, 1.0): summation (sum v), variance (population m2 / F, in
 * codes), skewness F/2*(1 + s/s_max) with s_max = (n-2)/sqrt(n-1), kurtosis
 * (non-excess k) F*(k-1)/(k_max-1) with k_max = (n^2-3n+3)/(n-1) (n <= 2 or
 * equal values: F/2 and 0), entropy F * H2/log2(min(n,256)) over 256 equal
 * bins of [0,F] (float clamped to [0,1] for binning; n = 1: 0), majority (the
 * most frequent value; a tie takes the median of the tied values). Integers
 * round half up and clamp to [0,F]; 8-bit statistics are of the stored codes. Masks, opacity, blend modes and
 * styles are not read. Every member that was visible is hidden; nothing else
 * changes. Lock-exempt.
 * The plan JSON holds retained_bytes, scratch_bytes and work_units (free with
 * dr_string_free); the edit admits retained + scratch + other_retained_bytes
 * against limit before any work, and under a job control polls once per extent
 * row ("operation cancelled"). NULL always carries an error: each refusal
 * above by a sentence naming the index, an extent past 100,000,000 pixels or
 * the native capacity, the working limit, a cancel, and "stack mode found no
 * pixels in the selected layers". */
char *dr_doc_stack_mode_plan(const DrDocument *document, const char *request, char **err_out);
DrDocument *dr_doc_stack_mode(const DrDocument *document, const char *request, uint64_t limit,
                              uint64_t other_retained_bytes, const DrJobControl *control,
                              char **err_out);

/* Image > Trim's kept rectangle. Basis: transparent = alpha sample 0;
 * top_left / bottom_right = all four samples exactly equal to that corner
 * pixel (float compares bit patterns). edges bitmask: 1 top, 2 bottom,
 * 4 left, 8 right; an unchecked edge never moves. rect_out = x, y, w, h.
 * false (rect_out untouched) on NULL, an unknown basis, or an image whose
 * every pixel is trimmable. */
typedef enum { DR_TRIM_TRANSPARENT = 0, DR_TRIM_TOP_LEFT = 1, DR_TRIM_BOTTOM_RIGHT = 2 } DrTrimBasis;
bool dr_image_trim_rect(const DrImage *img, DrTrimBasis basis, uint32_t edges, uint32_t *rect_out);

/* Move Selected Pixels. Request {op:"move_selected",layer,dx,dy} (strict,
 * 64 KiB). The plan is delta-independent JSON {retained_bytes, scratch_bytes,
 * input_samples}, freed with dr_string_free. The edit floats layer pixels ×
 * canvas u16 coverage (length must equal width*height) and moves them by
 * (dx,dy). The hole is a·(1−c), the composite is Normal source-over, the
 * landing is clipped to the canvas, and the mask is untouched. Lock Pixels /
 * Lock All refuse; Transparency and Position are allowed. NULL with no error
 * means no change (identity dx = dy = 0, or no stored sample changed).
 * Errors are refusal sentences. The typed depth
 * sends the same request through dr_precision_doc_edit.
 * Free Transform of the selected pixels is the same pair with
 * {op:"transform_selected",layer,matrix:[a,b,c,d,tx,ty],filter} or
 * {op:"transform_selected",layer,source_rect:[x,y,w,h],quad:[x0,y0,...,x3,y3],
 * filter} (exactly one form; quad is TL,TR,BR,BL of source_rect; filter
 * nearest|bilinear|bicubic|lanczos3). The float resamples as premultiplied
 * f32 through the map, landing wherever its kernel reaches, then composites
 * exactly as the move does. Identity is NULL with no error; an integer
 * translation IS the move. The plan charges 16 bytes per pixel of the
 * landing of the layer's on-canvas rect grown by the filter reach (0 for an
 * identity or integer translation, the whole canvas at the horizon). It
 * also refuses bad geometry. Neither has a work refusal: the run is
 * bounded by the plan's memory and the control's cancel.
 * Both ops take an optional "duplicate":true (the Option-drag,
 * Option-arrow and Free Transform a Copy): no hole, the original
 * stays and the copy lands source-over. Both ops take an optional
 * "bounds":[x,y,w,h] (four integers within i32, w and h at least 1, else
 * "selected pixels bounds must be [x, y, w, h] with w and h at least 1", also
 * for a non-null bounds of the wrong shape; null means absent): the caller's
 * promise that every non-zero coverage sample over the layer lies inside it,
 * so the plan charges the selection rect instead of the layer. A broken promise refuses
 * "selected pixels coverage lies outside the request's bounds" before any
 * read; the pixels are the same with or without it. Without either key the
 * request, result and plan are unchanged.
 * A transform_selected request and a move_selected request
 * also take "target": "layer" (the
 * default), "mask" (the layer's mask) or {"channel":N} (alpha channel N;
 * layer is then ignored), and, for a mask or channel target only,
 * "background":u16 coverage gray (default 0; with a layer target it refuses
 * "background applies to a mask or channel target only"). The plane's
 * selected coverage floats as premultiplied (v·c, c), resamples through the
 * same map and lands as lerp(hole, v, a); the hole is lerp(v, background, c)
 * (the background-colour rule for a plane without transparency;
 * duplicate leaves none). A channel stays canvas-sized; a raster/adjustment
 * mask's storage grows to R ∪ ((float ∪ landing) ∩ canvas) at its outside
 * coverage (its offset moves only on growth); a group mask is clipped to its
 * stored rect. A mask target is a mask edit (only Lock All refuses, "layer N
 * has no mask" without one); a channel ignores locks ("channel N is out of
 * range"). The plan charges plane samples (1 byte, 2 at the typed depths)
 * over mask ∪ canvas. A plane move_selected is the transform's integer
 * translation through the same walk, bit for bit.
 * Both ops also take "target":"red"|"green"|"blue"|"alpha": ONE colour
 * component of the layer floats, resamples and lands through the same plane
 * walk; the other components and the alpha are untouched and the layer never
 * grows (writes clip to its rect). For red, green or blue only, an optional
 * "swatch":[r,g,b] (the background swatch's 8-bit codes in the document
 * profile's encoding, default black; with any other target but a CMYK ink
 * plate it refuses "swatch applies to a red, green, blue, cyan, magenta,
 * yellow or black target only"); "background" with a
 * colour target refuses as above. The vacated value is the swatch's 30/59/11
 * luma rounded to an 8-bit code, /255 (measured, plane_gray.py), Alpha's 0;
 * duplicate leaves none. A CMYK document's "cyan"|"magenta"|"yellow"|"black"
 * target takes the swatch in sRGB 8-bit codes and leaves ink 1 − that gray
 * (#1, cmyk_selected_plate.rs). Red, green and blue coverage is scaled by the
 * stored alpha, so a lift from only transparent pixels refuses "The
 * selection holds no pixels in this colour channel." R/G/B are a
 * pixels edit, Alpha a merge edit (Lock Transparency refuses too); a
 * retained source refuses as the layer path does. */
char *dr_doc_selected_pixels_edit_plan(const DrDocument *document, const char *request,
                                       char **err_out);
DrDocument *dr_doc_selected_pixels_edit(const DrDocument *document, const char *request,
    const uint16_t *coverage, size_t length, uint64_t limit, uint64_t other_retained_bytes,
    const DrJobControl *control, char **err_out);
/* The FLOAT's canvas rect: the bbox of layer pixels with alpha > 0 AND
 * coverage > 0 (coverage canvas-sized u16, length must equal width*height).
 * True writes x, y, w, h to out_rect (NULL writes nothing); false with no
 * error is an empty float; false with an error is a refusal (length, index,
 * group, adjustment; typed: a 32-bit float document) or "operation
 * cancelled". Reads the layer in place, allocates nothing, checks no lock or
 * retained source. The typed twin is dr_precision_doc_selected_pixels_bounds. */
bool dr_doc_selected_pixels_bounds(const DrDocument *document, size_t layer,
    const uint16_t *coverage, size_t length, const DrJobControl *control,
    uint32_t *out_rect, char **err_out);

/* Duplicates / removes every given entry (subtrees included). Removal runs
 * top-down internally and refuses only when it would empty the document. */
DrDocument *dr_doc_duplicate_layers(const DrDocument *doc, const size_t *idx,
                                    size_t len);
DrDocument *dr_doc_remove_layers(const DrDocument *doc, const size_t *idx,
                                 size_t len);

/* Layer > Duplicate to New Document: the given entries (subtrees
 * included, entries subsumed by another given subtree dropped) copied into a
 * NEW document with the source's canvas, resolution, profile, evaluation
 * policy and global light, the roots landing at the top level in source paint
 * order. Every copied record keeps every property (pixels and masks shared,
 * off-canvas offsets kept, meta, style, retained source, warp) under a fresh
 * id; a root keeps its clip only when its source base is a copied root with
 * only clipped roots of the same base between them, and a link id only when
 * at least two copied records share it. Channels, guides, comps and metadata
 * are not carried; the ruler origin is (0,0). The source is untouched. NULL
 * on NULL doc or an empty, out-of-range or repeating list. The typed twin is
 * the {op:"duplicate_to_new_document"} edit. */
DrDocument *dr_doc_duplicate_to_new_document(const DrDocument *doc, const size_t *idx,
                                             size_t len);

/* Merges the given entries — which must all share one parent — into ONE
 * raster entry at the lowest member's slot and depth, through the same kernel
 * as the projection, keeping the lowest member's name, visibility and clipped
 * flag. Masks, meta and styles are baked and dropped exactly as
 * dr_doc_merging_down documents. A merged layer's KNOCKOUT is evaluated
 * against its REAL backdrop; only where none exists by structure does the
 * hole bake as TRANSPARENCY ("Layer styles" above). Layer comps bound to the
 * lowest member follow the merge as dr_doc_merging_down describes. */
DrDocument *dr_doc_merge_layers(const DrDocument *doc, const size_t *idx,
                                size_t len);

/* Aligns every given entry's CONTENT bounds (dr_doc_layer_bounds) to `edge`
 * of the reference rect: the union of the given entries' bounds, or the
 * canvas with to_canvas. An entry with nothing opaque is skipped. NULL when
 * fewer than two entries have content and to_canvas is false. The typed
 * twins are the align_set / distribute_set requests of dr_precision_doc_edit
 * ("Native align and distribute"). */
DrDocument *dr_doc_align_layers(const DrDocument *doc, const size_t *idx,
                                size_t len, DrAlign edge, bool to_canvas);

/* Spaces the given entries evenly along one axis: the two outermost keep
 * their positions and the gaps between adjacent CONTENT bounds are made equal
 * ("distribute spacing", not "distribute centers"). Needs at
 * least three entries with content; a negative gap is arithmetic, not an
 * error. */
DrDocument *dr_doc_distribute_layers(const DrDocument *doc, const size_t *idx,
                                     size_t len, bool vertical);

/* --- commands --- */

/* Replaces every entry that CONTRIBUTES to the projection — visible, with
 * every ancestor visible — with ONE canvas-sized raster entry holding the
 * visible projection, inserted at the slot of the bottom-most contributing
 * entry's top-level ancestor and named after that entry. Every entry that
 * does not contribute SURVIVES untouched, a visible layer inside a hidden
 * group included; a group survives if any descendant does. NULL on NULL doc
 * and when fewer than two LEAF entries contribute. This bakes the document's
 * own PROJECTION, so a merged layer's KNOCKOUT bakes EXACTLY — like
 * dr_doc_flattening and dr_doc_stamp_visible ("Layer styles" above). The
 * merged entry keeps that bottom-most leaf's id, and layer comps bound to it
 * follow the merge as dr_doc_merging_down describes (offsets shift to (0,0)). */
DrDocument *dr_doc_merge_visible(const DrDocument *doc);

/* Adds the visible projection as a NEW canvas-sized raster entry immediately
 * above above_idx's subtree, at above_idx's depth. Nothing else changes. NULL
 * on NULL doc, out-of-range above_idx or NULL name. */
DrDocument *dr_doc_stamp_visible(const DrDocument *doc, size_t above_idx,
                                 const char *name);

/* Stamp Selected (#1 phase 4c, doc_merge_selected): ONE group, or two or
 * more entries sharing one parent, composited over their union extent (the
 * dr_doc_merge_layers kernel) into ONE new raster entry — Normal, opacity 1,
 * unclipped, a new id, named `name` — directly above the topmost member's
 * subtree at its depth; every original is untouched. NULL on NULL doc or
 * name, an empty, out-of-range or repeating list, one entry that is not a
 * group, entries that do not share one parent, or a union with nothing
 * visible. The typed twin is the "stamp_selected" structure edit. */
DrDocument *dr_doc_stamp_selected(const DrDocument *doc, const size_t *idx, size_t len,
                                  const char *name);

/* Merge Group (#1 phase 4c): the visible group `idx` replaced by ONE raster
 * entry holding its RAW projection (the group at opacity 1, Normal, no mask
 * or style, unclipped, Pass Through isolated) over its union extent, keeping
 * the group's slot, depth, name, id, label, opacity, blend mode (Pass Through
 * becomes Normal), style, mask, clipping, locks and link — an isolated
 * group's composite is unchanged. NULL on NULL doc, an out-of-range index, an
 * entry that is not a visible group, locks that refuse a merge, or a group
 * with nothing visible. The typed twin is the "merge_group" structure edit. */
DrDocument *dr_doc_merge_group(const DrDocument *doc, size_t idx);

/* Layer Via Copy / Via Cut: the source's pixels inside the canvas-sized
 * coverage mask (or the whole layer when mask is NULL) become a new raster
 * entry immediately above the source, same depth and same canvas position.
 * This op RASTERIZES: it keeps neither the layer's metadata nor its style, so
 * a host whose layer is a described one (text, shape) or an adjustment layer
 * wants dr_doc_duplicating_layer instead. With cut, the same coverage is
 * cleared from the source, under the source's locks. NULL on NULL doc or
 * name, out-of-range idx, a group, an adjustment layer, a mask that is not
 * canvas-sized, or a coverage that selects nothing inside the layer. */
DrDocument *dr_doc_layer_via(const DrDocument *doc, size_t idx,
                             const uint8_t *mask, uint32_t w, uint32_t h,
                             bool cut, const char *name);

/* The entry a click at canvas (x, y) activates — the TOPMOST one whose own
 * coverage there is at least half (the same contour a selection is cut at),
 * skipping invisible entries with their subtrees and skipping adjustment
 * layers. A group hits when any descendant hits. With top_level the answer is
 * the hit entry's top-level ancestor (Auto-Select: Group). False (nothing
 * written) on NULL doc or when nothing is hit — the caller then leaves its
 * selection alone rather than emptying it. */
bool dr_doc_layer_at(const DrDocument *doc, int32_t x, int32_t y,
                     bool top_level, size_t *out_idx);

/* ---- Guides, rulers and snapping ----------------------------------------
 *
 * A document carries a list of GUIDES and a RULER ORIGIN. A guide is one
 * line across the whole canvas: an orientation (horizontal = a line of
 * constant y, vertical = a line of constant x) and a position. The ruler
 * origin is the canvas coordinate of the ruler's zero point, (0, 0) — the
 * canvas's top-left — by default.
 *
 * POSITIONS ARE GRID-LINE COORDINATES in the continuous canvas space
 * [0, width] / [0, height], not pixel indices: a vertical guide at x = 0 lies
 * on the left canvas edge and one at x = width on the right, so BOTH extremes
 * are legal and the range is inclusive at both ends. The orientation names
 * the LINE, not the axis its position is measured on, so a vertical guide's
 * position is measured against the WIDTH. Positions are quantized to four
 * decimals, exactly like the resolution and the global light, so a value read
 * back and echoed in is refused as unchanged rather than registering an edit.
 * They are absolute canvas coordinates and are NOT measured from the ruler
 * origin — the origin moves the ruler's LABELS and nothing else.
 *
 * Every canvas-geometry op moves them the way it moves the pixels, and where
 * an op could either DROP a guide or CLAMP it the choice is deliberate:
 *   - the rotations and the flips PERMUTE (a quarter turn also exchanges each
 *     guide's orientation, because the line has turned with the picture);
 *   - dr_doc_crop and dr_doc_canvas_resize shift and then DROP whatever now
 *     lies outside the new canvas — a guide cut out of the picture would
 *     otherwise be invisible, unclickable and undeletable, and reappear from
 *     nowhere on a later Canvas Size (undo restores the whole document, so
 *     nothing is lost);
 *   - dr_doc_resize scales without rounding — a guide position is a
 *     continuous coordinate, and rounding it at every Image Size would
 *     accumulate — and nothing can leave the canvas under a scale;
 *   - canvas rotation (12F-1b3, dr_doc_geometry_edit's "rotate_canvas")
 *     shifts by (floor((W'-W)/2), floor((H'-H)/2)) and
 *     DROPS, the padded rule again, because an ARBITRARY rotation turns a
 *     guide off its axis and a rotated guide is not a guide. Its EXACT
 *     quarter turns delegate and therefore PERMUTE instead, which is the one
 *     visible discontinuity at 90 degrees: 90 exchanges each guide's
 *     orientation and transposes the ppi, while 89.999 shifts, drops and
 *     leaves the ppi alone;
 *   - dr_doc_flattening carries both verbatim; the per-layer ops leave them
 *     alone. A straighten — the byte "straighten" request of
 *     dr_doc_geometry_edit, and since 12F-1b2 the
 *     typed "straighten" op that equals it — has NO guide op and needs none:
 *     a rotated guide is not a guide, so the trailing crop simply shifts and
 *     drops.
 * The RULER ORIGIN is CLAMPED where a guide is dropped: there is exactly one
 * origin and a document is never without one.
 *
 * At most dr_max_guides() guides (1024). They are written by
 * dr_doc_save_native — they are what bumped the DRDC format to version 8.
 *
 * HIGH DEPTH. Since 12E-1c a typed document carries the same two pieces of
 * state, with the same rules and the same refusals: the ten
 * dr_precision_doc_guide_* / _ruler_origin / _set_ruler_origin twins are
 * declared in the high-depth section beside dr_precision_doc_layer_comps_json
 * (DrPrecisionDocument is typedef'd only there), every typed canvas-geometry
 * op moves them exactly as the byte ops above do, and they are written to the
 * typed .rz by the conditional LGDE table at version 48. Since 12F-1 that
 * includes dr_doc_canvas_resize's padded-guide behaviour: the typed
 * {"op":"canvas_size",...} geometry edit moves the chrome through the very
 * same padded_guides / padded_origin helpers, so there is no byte-only guide
 * case left.
 *
 * WHAT THE CORE DOES NOT KNOW ABOUT. The ruler's UNIT, whether rulers, guides
 * or the grid are shown, the guide COLOUR, the guide LOCK, the grid's spacing
 * and subdivisions, the pixel grid, and every snapping toggle are all HOST
 * preferences: they describe how a user works rather than what the picture
 * contains, they are app-wide rather than per-document, and none of them is
 * in the file. In particular there is no per-guide lock byte and no
 * dr_doc_lock_guides — Lock Guides is one host-side boolean governing the
 * mouse. Snapping itself is entirely the host's: the core stores the lines
 * and answers where they are. */

typedef enum {
  DR_GUIDE_HORIZONTAL = 0, /* a line of constant y */
  DR_GUIDE_VERTICAL = 1,   /* a line of constant x */
} DrGuideOrientation;

/* The guide-count cap, 1024. No document and no pointers — and, unlike
 * dr_max_channels_at, no dimensions either: a guide is a line, not a plane,
 * so no canvas size makes fewer of them fit. It is exported rather than
 * hand-copied into a host because dr_doc_add_guide answers a single NULL for
 * four different refusals, and a host must be able to tell "the list is full"
 * apart from "that guide already exists". */
size_t dr_max_guides(void);

/* --- guide list (getters) ---
 * Per-index, with no bulk getter on purpose: a host caches the list once per
 * document change, so a canvas redraw makes no calls at all. */

size_t dr_doc_guide_count(const DrDocument *doc);

/* Guide i's STABLE IDENTITY: unique among every guide this process has
 * minted, and the handle a host hangs per-guide state on (which guide a drag
 * is moving). The list is re-SORTED by position on every edit, so an index
 * cannot identify a guide across one; the id survives a move, the geometry
 * ops and undo/redo, and a guide that is gone takes its id with it. NOT
 * persisted — dr_doc_open mints new ids, so the identity holds for as long as
 * a document is open, not across sessions. 0 on NULL doc / bad index, which
 * no live guide ever answers. */
uint64_t dr_doc_guide_id(const DrDocument *doc, size_t i);

/* A DrGuideOrientation; -1 on NULL doc / bad index (outside the enum). */
int dr_doc_guide_orientation(const DrDocument *doc, size_t i);

/* The canvas coordinate of the line. -1.0 on NULL doc / bad index: a legal
 * position is never negative, so the sentinel cannot collide with an answer. */
double dr_doc_guide_position(const DrDocument *doc, size_t i);

/* Writes the ruler origin as two doubles (x, y) into out_xy. false on a NULL
 * doc or a NULL buffer. */
bool dr_doc_ruler_origin(const DrDocument *doc, double *out_xy);

/* --- guide list (pure mutators; NULL = refusal, per the doc comments) --- */

/* Adds a guide. The list is kept sorted by (orientation, position), so the
 * new guide's INDEX is wherever it sorts, not the end. NULL on NULL doc, an
 * orientation outside DrGuideOrientation, a non-finite position, a position
 * outside [0, canvas extent], a position a guide of the same orientation
 * already occupies (adding a duplicate is not an edit), or a list already
 * holding dr_max_guides(). The four are deliberately one answer: a host that
 * needs to tell them apart checks the position and the count before calling,
 * which is also how it phrases the refusal in its own words. */
DrDocument *dr_doc_add_guide(const DrDocument *doc, int orientation,
                             double position);

/* Moves guide i along its own axis, keeping its identity and its orientation
 * (and re-sorting, so its index may change). NULL on NULL doc, a bad index, a
 * non-finite or out-of-canvas position, the position the guide already has,
 * or a position another guide of the same orientation already occupies. */
DrDocument *dr_doc_move_guide(const DrDocument *doc, size_t i, double position);

/* NULL on NULL doc or a bad index. */
DrDocument *dr_doc_remove_guide(const DrDocument *doc, size_t i);

/* NULL on NULL doc or an already empty list — clearing nothing is not an
 * edit, and an identical copy would register a phantom undo step. */
DrDocument *dr_doc_clear_guides(const DrDocument *doc);

/* Moves the ruler zero point to canvas (x, y). NULL on NULL doc, a non-finite
 * component, a point outside the canvas, or the origin the document already
 * has (after the four-decimal quantization). */
DrDocument *dr_doc_set_ruler_origin(const DrDocument *doc, double x, double y);

/* ---- Colour samplers ----------------------------------------------------
 *
 * A document carries up to 10 COLOUR SAMPLERS (saved with it): pinned
 * canvas PIXELS the host reads live, in label order (#1 first). A sampler is
 * a PIXEL INDEX — x in [0, width), y in [0, height) — plus a sample size, the
 * side of the averaged square: 1, 3 or 5. They never composite and carry no
 * identity; two samplers at the same pixel and size are equal.
 *
 * Every canvas-geometry op moves them with the content: the pixel CENTRE
 * (x + 0.5, y + 0.5) goes through the op's canvas map and is floored back to
 * a pixel. Quarter turns and flips permute; crop and canvas resize shift;
 * image resize scales (and clamps); a filtered rotate_canvas and straighten
 * map through their canvas matrix (and the straighten window) — samplers
 * ROTATE with the content where guides only shift. A sampler that lands
 * outside the new canvas is DROPPED; the order is kept, so labels renumber.
 * Per-layer transforms leave them alone. The typed twins below follow the
 * same rules through dr_precision_doc_edit's geometry ops.
 *
 * Saved in .rz (versions 59/60 and the CSMP table, see the save format
 * above) and in a PSD as image resource 1073 at 8 and 16 bits: written as
 * version 2 (f32 vertical then horizontal, the pixel index, colour space −1
 * and depth 8, Photoshop's own values) and read at versions 1–3. The PSD
 * import never refuses over 1073: an unknown version or a bad length skips
 * it, an off-canvas sampler or one past the tenth is dropped, and a
 * per-sampler colour mode is not kept, each with a report sentence;
 * imported samplers take sample size 1. A PSD export reports a loss when
 * some sampler's size is not 1 (Photoshop stores no per-sampler size). A flat
 * 8-bit save reports "color samplers" in dr_doc_flat_save_losses. */

/* Number of samplers; 0 on NULL doc. */
size_t dr_doc_color_sampler_count(const DrDocument *doc);

/* Sampler i as x, y, sample size into out_xys (3 values); false on NULL doc,
 * a NULL buffer or a bad index. */
bool dr_doc_color_sampler(const DrDocument *doc, size_t i, uint32_t *out_xys);

/* The document holding exactly `count` samplers: xys is `count` triples
 * x, y, sample size (may be NULL when count == 0, which clears the list).
 * NULL on NULL doc, count > 10, an entry off the canvas, a size other than
 * 1, 3 or 5, or the list the document already holds (an identical copy
 * would register a phantom edit). */
DrDocument *dr_doc_with_color_samplers(const DrDocument *doc, const uint32_t *xys,
                                       size_t count);

/* ---- Paths --------------------------------------------------------------
 *
 * A document carries a list of PATHS: named Bézier paths in canvas
 * space. They never composite, are saved in .rz (versions 74/75 and the PATH
 * table, see the save format above) and are not written to any other format;
 * a flat 8-bit save reports "paths" in dr_doc_flat_save_losses.
 *
 * SHAPE JSON:
 *   {"fill_rule":"nonzero"|"evenodd",
 *    "subpaths":[{"closed":bool,
 *                 "anchors":[{"x":f,"y":f,"in":[x,y],"out":[x,y],"smooth":bool}]}]}
 * Coordinates are ABSOLUTE canvas coordinates in the canvas's continuous
 * space (pixel (x, y) spans [x, x+1) x [y, y+1)), handles included. The
 * segment from anchor a to the next anchor b is the cubic (a, a.out, b.in,
 * b), a straight line when both handles sit on their points; a closed
 * subpath also has the segment from its last anchor to its first, and an
 * open one is closed by a straight line for fill only. "in" and "out"
 * default to the point, "smooth" and "closed" to false, "fill_rule" to
 * "nonzero"; an anchor may also be the pair [x, y], a corner. "smooth" is an
 * editing hint and changes no geometry. Every coordinate is quantized to
 * four decimals when read, so a shape read back and echoed in is unchanged.
 * Refused: a coordinate that is not finite or lies outside +-1000000, more
 * than 4096 subpaths or 32768 anchors in one shape, a subpath with no anchor.
 *
 * PATH-LIST JSON:
 *   {"paths":[{"id":u64,"name":string,"kind":"work"|"saved", ...shape}]}
 * LIST RULES, each refused with its own sentence: at most dr_max_paths()
 * entries; at most one "work" entry — the unnamed scratch path, always
 * stored first and always named "Work Path" ("name" may be omitted on it);
 * every "saved" name non-empty, at most 255 UTF-8 bytes, unique and not
 * "Work Path"; and at most 65536 anchors over every path of the document
 * and every vector-mask shape ("too many path anchors in the document (max
 * 65536)").
 * An id is process-unique, stable across every edit, geometry op and depth
 * conversion, and never saved. An entry whose "id" is 0 or absent is a new
 * path and is minted one, as is an entry repeating an earlier entry's id.
 *
 * Every canvas-geometry op maps each point and handle through the op's own
 * canvas map, the one a colour sampler's pixel centre takes: quarter turns
 * and flips, crop and canvas resize (a shift), image resize (a scale), a
 * filtered rotate_canvas and straighten (their canvas matrix, then the
 * straighten window) and perspective crop (its destination map). A path is
 * NEVER dropped and never clamped into the canvas — it may lie partly or
 * wholly outside it; a mapped coordinate stays unrounded to pixels and
 * inside +-1000000. Per-layer transforms leave paths alone. The typed twins
 * below follow the same rules through dr_precision_doc_edit's geometry ops. */

/* The most paths a document may hold. */
size_t dr_max_paths(void);

/* The document's paths as the path-list JSON, the work path first. NULL on
 * NULL doc. Free with dr_string_free. */
char *dr_doc_paths_json(const DrDocument *doc);

/* The document holding exactly the paths in `json` (path-list JSON): add,
 * edit, rename, reorder, delete and clear are one call. NULL on NULL doc or
 * for the list the document already holds, with err_out untouched (an
 * identical copy would register a phantom edit); NULL with err_out set for
 * NULL or malformed JSON, a refused shape, and a broken list rule or cap,
 * with the sentence above. */
DrDocument *dr_doc_with_paths_json(const DrDocument *doc, const char *json,
                                   char **err_out);

/* SHAPE OPERATIONS take one shape as the SHAPE JSON and no document, so they
 * serve both depths. SEGMENT i of a subpath runs from anchor i to anchor
 * i + 1; a closed subpath's last segment (index n - 1) returns to its first
 * anchor; an open subpath of n anchors has n - 1 segments (the straight
 * closure a fill adds is not one). */

/* The shape's FILL as u16 area coverage (65535 = wholly covered) over the
 * width x height window whose top-left pixel is canvas pixel (x, y), written
 * row-major into output. It is the artwork coverage rasterizer
 * (dr_artwork_path_coverage) under the shape's fill rule, an open subpath
 * closed by a straight line. anti_alias false cuts each sample at one half
 * (>= 32768 becomes 65535, anything less 0). clip is NULL with clip_count 0,
 * or width*height u16 samples multiplied into the result (rounded to
 * nearest) — a selection the fill is confined to. limit / other are the
 * caller's working limit and retained bytes. False with err_out set, and
 * output untouched, for NULL or malformed JSON, a refused shape,
 * output_count != width*height, a clip of any other length, a zero or
 * oversized window, a refused admission, the rasterizer's edge or work caps,
 * or cancellation. control may be NULL. */
bool dr_path_coverage(const char *shape_json, int32_t x, int32_t y,
                      uint32_t width, uint32_t height, bool anti_alias,
                      const uint16_t *clip, size_t clip_count,
                      uint16_t *output, size_t output_count,
                      uint64_t limit, uint64_t other,
                      const DrJobControl *control, char **err_out);

/* The shape as polylines: [{"closed":bool,"points":[[x,y],...]}], one entry
 * per subpath in order, every point within `tolerance` pixels of the curve
 * (four decimals). A polyline starts at its subpath's first anchor and
 * passes through every anchor; a CLOSED one covers the closing segment and
 * does NOT repeat its first point (the flag says so). A tolerance under
 * 1/4096 px flattens at 1/4096. NULL with err_out set for NULL or malformed
 * JSON, a refused shape, a tolerance that is not a positive finite number,
 * or a shape needing more than 1048576 points at that tolerance. Free with
 * dr_string_free. */
char *dr_path_flatten(const char *shape_json, double tolerance, char **err_out);

/* Traces the region of `coverage` (width*height u16 samples, row-major) at
 * or above one half into the SHAPE JSON of closed subpaths under "evenodd"
 * (a hole is the loop inside its outline), each within `tolerance` pixels of
 * the half-coverage contour: marching squares with linear edge interpolation
 * (a sample sits at its pixel's centre), Douglas-Peucker, then Schneider's
 * cubic fit. A loop no larger than `tolerance` on either side is dropped. A
 * plane with nothing at or above one half answers a shape with no subpath.
 * limit / other are the caller's working limit and retained bytes. NULL with
 * err_out set for a NULL or mis-sized plane, a zero or oversized one, a
 * tolerance outside 0.25..=64, a refused admission, an outline of more than
 * 2097152 contour points, a result past 4096 subpaths or 32768 anchors (the
 * sentence says to raise the tolerance), or cancellation. control may be
 * NULL. Free with dr_string_free. */
char *dr_path_trace(const uint16_t *coverage, size_t coverage_count,
                    uint32_t width, uint32_t height, double tolerance,
                    uint64_t limit, uint64_t other,
                    const DrJobControl *control, char **err_out);

/* What lies within `tolerance` pixels of (x, y):
 *   {"kind":"none"}
 *   {"kind":"anchor"|"handle_in"|"handle_out","subpath":i,"index":i}
 *   {"kind":"segment","subpath":i,"index":i,"t":f}
 * nearest first within a kind, and an anchor before a handle before a
 * segment. A handle sitting on its point is not offered, and with `handles`
 * false NO handle is — the answer is an anchor, a segment or none — which is
 * what a press that cannot grab a handle asks, so a handle tip lying near a
 * segment does not hide it. "t" is the parameter of the segment's nearest
 * point, in [0, 1]: the cubic's own parameter on a curved segment, the
 * fraction of the length on a straight one. NULL for NULL or malformed JSON
 * or a refused shape. Free with dr_string_free. */
char *dr_path_hit(const char *shape_json, double x, double y, double tolerance,
                  bool handles);

/* The shape with one more anchor on segment `segment` of subpath `subpath`
 * at parameter t (dr_path_hit's), its outline unchanged to four decimals: a
 * curved segment is split by de Casteljau (the new anchor is smooth and its
 * neighbours' facing handles shorten), a straight one gets a corner. NULL
 * with err_out set for NULL or malformed JSON, a refused shape, no such
 * subpath or segment, a t not strictly between 0 and 1, or a shape already
 * at 32768 anchors. Free with dr_string_free. */
char *dr_path_insert_anchor(const char *shape_json, size_t subpath,
                            size_t segment, double t, char **err_out);

/* The shape with anchor `anchor` of subpath `subpath` converted: to a corner
 * (smooth false: both handles onto the point), or to a smooth anchor whose
 * handles lie along the line from the previous anchor to the next (a closed
 * subpath wraps), each a third of the way to its neighbour; an end of an
 * open subpath gets the one handle facing its neighbour. NULL with err_out
 * UNTOUCHED when the anchor is already exactly that or a smooth request has
 * no neighbour at another point to aim at (nothing changed); NULL with
 * err_out set for NULL or malformed JSON, a refused shape, or no such
 * subpath or anchor. Free with dr_string_free. */
char *dr_path_convert_anchor(const char *shape_json, size_t subpath,
                             size_t anchor, bool smooth, char **err_out);

/* ---- Landmark regions (Select > Face Parts...) ---------------------------
 * u16 coverage (65535 = covered) of the landmark regions in regions_json over
 * the width x height canvas, row-major into output: the union (per-sample
 * max) of each region's include x clip x (65535 - exclude). The JSON is
 * {"regions":[{"include":[LOOP...],"exclude":[LOOP...],"clip":[LOOP...]}]}
 * in canvas continuous space (as the SHAPE JSON); include is required and
 * nonempty. LOOP is {"points":[[x,y],...]} (closed, at least 3 points) or
 * {"points":[[x,y],...],"half_width":h} (an open line, at least 2 points,
 * whose region is the band of half-width h around it). Each list is the
 * union of its loops, each loop the closed centripetal Catmull-Rom spline
 * through its points rasterized by the artwork coverage rasterizer; a loop
 * left with fewer than 3 distinct points (2 for a line) covers nothing.
 * Caps: 1024 regions, 16 loops a list, 1024 points a loop, 262144 points in
 * all. limit / other: working limit and retained bytes. False with err_out
 * set and output untouched for NULL or malformed JSON, a broken cap,
 * output_count != width*height, a zero/oversized window, a refused
 * admission, the rasterizer's caps, or cancellation. control may be NULL.
 * An empty "regions" covers nothing. */
bool dr_landmark_coverage(const char *regions_json, uint32_t width, uint32_t height,
                          uint16_t *output, size_t output_count, uint64_t limit, uint64_t other,
                          const DrJobControl *control, char **err_out);

/* ---- Model masks -----------------------------------------------------------
 * Select > Sky, Select > Person Parts... and the Object Finder: a
 * segmentation model's low-resolution output plane to canvas u16 coverage
 * (65535 = covered), row-major into output, and in *covered_out (when not
 * NULL) the count of samples >= 32768. plane holds plane_channels
 * channel-major planes of plane_width x plane_height f32 samples
 * (c*pw*ph + y*pw + x); plane_count must equal pw*ph*pc, each side at most
 * 4096 and at most 16 channels. request_json is strict JSON
 * {"frame":[x,y,w,h],"values":"logit"|"probability","channels":[c...]}:
 * frame is integers with w, h >= 1 inside the canvas (the canvas rectangle
 * the model saw, squashed to its input); channels are unique, each below
 * plane_channels, default [0], exactly one for "logit". The selected
 * channels are summed per cell; canvas pixel (x, y) in the frame reads the
 * sum bilinearly at u = (x + 0.5 - fx) * pw / fw (likewise v), clamped
 * edges, then "logit" is 65535 where it is > 0, else 0 (SAM's own
 * threshold; the host's edge stage softens the edge), and "probability"
 * clamps it to [0, 1], where a value below 1/256 (one 8-bit display step)
 * becomes 0, so coverage far from the object is exactly 0; the result is
 * multiplied by clip / 255 and rounded.
 * Outside the frame coverage is 0. clip is NULL (none) or clip_count ==
 * width * height bytes. limit / other: working limit and retained bytes.
 * False with err_out set and output untouched for a NULL plane, request or
 * output, a count mismatch, a broken cap, malformed JSON, a frame outside
 * the canvas, a bad channel, a non-finite selected sample ("model output is
 * not finite"), a refused admission or cancellation. control may be NULL. */
bool dr_model_mask_coverage(const float *plane, size_t plane_count, uint32_t plane_width,
                            uint32_t plane_height, uint32_t plane_channels,
                            const char *request_json, const uint8_t *clip, size_t clip_count,
                            uint32_t width, uint32_t height, uint16_t *output,
                            size_t output_count, uint64_t *covered_out, uint64_t limit,
                            uint64_t other, const DrJobControl *control, char **err_out);

/* ---- Lens profiles (v1-lensfun) ------------------------------------------
 * The bundled Lensfun lens database (v0.3.4, CC BY-SA 3.0, the Lensfun
 * project and its contributors), matched to a document's EXIF.
 *
 * Lens identity read from an EXIF packet (DR_METADATA_EXIF: TIFF stream, no
 * Exif\0\0): JSON {"make","model","lens_make","lens_model": string|null,
 * "focal_length","f_number","subject_distance","focal_length_35mm",
 * "pixel_width","pixel_height": number|null}. IFD0 0x010F/0x0110, Exif IFD
 * 0xA433/0xA434/0x920A/0x829D/0x9206/0xA405/0xA002/0xA003. NULL, len 0 or
 * len past the 16 MiB blob cap: NULL, no error. Unreadable: every field
 * null. Free with dr_string_free. */
char *dr_lens_exif_identity(const uint8_t *exif, size_t len);
/* request_json {"identity":{...as above...}, "lens":"auto"|"<lens id>",
 * "all":bool (default false), "dimensions":[w,h] (optional: the target
 * plane; auto gives no profile when its long/short aspect differs from the
 * EXIF pixel dimensions' by more than 1%)}. Returns {"database":{"name":
 * "Lensfun","tag":"v0.3.4","license":"CC BY-SA 3.0","license_url","url"},
 * "camera":{"maker","model","mount","crop_factor"}|null, "crop_factor":
 * number|null, "crop_factor_source":"camera"|"exif_35mm"|null,
 * "candidates":[Lens], "mount_lenses":[Lens] (only with all:true; every lens
 * for the camera's mount family), "selected":Lens|null, "profile":
 * Profile|null, "reason":string|null}. Lens = {"id","maker","model",
 * "crop_factor","focal_min","focal_max","distortion":bool,"tca":bool,
 * "vignetting":bool}. Profile is lens_correction's "profile" parameter:
 * {"id","crop_factor","calibration_crop_factor","aspect_ratio","center",
 * "focal_length","f_number","subject_distance","distortion","tca",
 * "vignetting"}. An unknown id, bad JSON or an unknown key: NULL + err_out.
 * No match is not an error (profile null, reason set). Free with
 * dr_string_free. */
char *dr_lens_match(const char *request_json, char **err_out);

/* VECTOR MASKS. A vector mask is a layer mask whose coverage is the
 * FILL of a shape (the SHAPE JSON, canvas coordinates), kept editable. It is
 * a BAKED ordinary mask: the layer's mask — what every mask getter, the
 * compositor and every exporter read — is
 *     raster part's effective coverage x the shape's coverage
 * and the core keeps the shape, two flags and the RASTER PART (the layer's
 * own raster mask, when it had or is later given one) beside it. The raster
 * factor is 1 with no raster part or a disabled one; the shape factor is 1
 * when the vector mask is disabled or its shape is empty (Reveal All). The
 * baked mask has outside coverage 0, density 1 and feather 0 and covers the
 * shape's pixel box inside the layer's pixel rect (the canvas for a group,
 * an adjustment layer and an unlinked mask) — or the raster part's own
 * rectangle when that part is enabled, unfeathered, at density 1 and outside
 * coverage 0. A disabled vector mask with no raster part keeps its plane in
 * the slot DISABLED; with a raster part the slot is that part verbatim.
 *
 * MASK EDITS on such a layer (add, delete, enable, paint, invert, set,
 * properties, a mask-only transform, a scoped write) edit the RASTER PART and
 * the product is baked again; with no raster part the ones that need a mask
 * refuse as they do on a layer without one. APPLYING the mask bakes the
 * product into the layer's alpha and removes both. A vector mask moves with
 * its layer (the shape is read shifted by the mask's own offset change). A
 * free transform of a linked mask, a quarter turn or flip of the canvas and
 * image resize keep a vector mask WITHOUT a raster part as a shape — mapped
 * by the op and baked again; every other resampling of the mask (perspective,
 * warp, straighten, a filtered canvas rotation, any of them with a raster
 * part) leaves the layer the resampled raster mask and no vector mask, as do
 * layer duplicate, merge and paste. Depth conversion keeps a vector mask
 * without a raster part. The shapes share the document's 65536-anchor cap
 * with the path list. Saved in the PATH table of .rz. */

/* Layer `layer`'s vector mask: the SHAPE JSON's keys beside "enabled",
 * "linked", "has_raster" (a raster mask is kept beside the shape),
 * "raster_enabled" and "raster" — null, or the raster part's own
 * {"offset_x","offset_y","width","height","linked","density","feather",
 * "outside_coverage"} (dr_doc_layer_mask_properties' keys). The layer's mask
 * getters describe the BAKED mask, so a host that edits a mask property by
 * read-modify-write starts from "raster" and "raster_enabled" instead. The
 * LINK is one flag for both halves: linking or unlinking either one sets the
 * other. NULL on NULL doc, an out-of-range index and a layer with no vector
 * mask. Free with dr_string_free. */
char *dr_doc_vector_mask_json(const DrDocument *doc, size_t layer);

/* The document with layer `layer`'s vector mask set to `shape_json`: a new
 * vector mask, enabled and linked, with the layer's ordinary mask (if any)
 * kept as its raster part — or the same vector mask with a new shape. An
 * empty shape is Reveal All. NULL with err_out untouched on NULL doc, an
 * out-of-range index, Lock All and the shape the mask already has; NULL with
 * err_out set for NULL or malformed JSON, a refused shape, a shape that takes
 * the document past its anchor cap ("too many path anchors in the document
 * (max 65536)"), the rasterizer's refusal, growth past native capacity and
 * cancellation. control may be NULL. */
DrDocument *dr_doc_set_vector_mask(const DrDocument *doc, size_t layer,
                                   const char *shape_json,
                                   const DrJobControl *control, char **err_out);

/* The document with layer `layer`'s vector mask removed: its raster part
 * alone is the layer's mask again, or the layer has none. NULL on NULL doc,
 * an out-of-range index, a layer with no vector mask and Lock All. */
DrDocument *dr_doc_remove_vector_mask(const DrDocument *doc, size_t layer);

/* The document with layer `layer`'s vector mask enabled or disabled and
 * linked or unlinked, baked again. NULL with err_out untouched on NULL doc,
 * a layer with no vector mask, Lock All and the flags it already has; NULL
 * with err_out set for the rasterizer's refusal, growth past native capacity
 * and cancellation. control may be NULL. */
DrDocument *dr_doc_set_vector_mask_properties(const DrDocument *doc, size_t layer,
                                              bool enabled, bool linked,
                                              const DrJobControl *control,
                                              char **err_out);

/* ---- Colour management and metadata -------------------------------------
 *
 * A document carries three more pieces of state: an ICC COLOUR PROFILE (the
 * space its pixel numbers belong to), the EXIF / XMP / IPTC PACKETS the file
 * it came from carried, and a print RESOLUTION in pixels per inch. Every
 * document op keeps all three; the profile, the packets and the resolution
 * are what bumped the DRDC format to version 6. The one op that does not
 * keep the resolution VERBATIM keeps it correct instead: a quarter turn
 * swaps the two ppi axes, because a 300 x 150 ppi picture turned 90 degrees
 * is a 150 x 300 ppi one. That holds for dr_doc_rotate90 and
 * dr_doc_rotate270, and for the camera rotation dr_doc_open bakes into the
 * pixels — the same turn, so the same swap. ONLY an exact quarter turn
 * transposes it: canvas rotation delegates one and therefore swaps the
 * axes, while a FILTERED angle — 89.999 degrees included — leaves the
 * resolution exactly where it was, because the picture has not been turned
 * onto its side, it has been turned by an angle.
 *
 * THE PROFILE IS NEVER ABSENT. A document with no embedded profile is
 * assumed sRGB — which is what every reader does anyway, and what lets the
 * display, the export and the transform share exactly one rule. So is a
 * document whose file carried a profile that is not RGB, or one larger than
 * the 16 MiB a document can carry (a PNG iCCP is zlib-compressed, so an
 * inflated profile is bounded by nothing else): the profile is dropped
 * rather than producing a document dr_doc_save_native would refuse to
 * write. A profile is also stored at its own DECLARED size, so trailing
 * padding is neither kept nor re-embedded. The library
 * writes its own conformant ICC v2.1.0 blobs for sRGB IEC61966-2.1,
 * Display P3, Adobe RGB (1998) and ProPhoto RGB (dr_builtin_profile), so an
 * exported file always carries a profile another application can read.
 *
 * WHAT THE LIBRARY CAN CONVERT WITH. ICC v2 and v4 RGB matrix/TRC profiles —
 * sRGB, Display P3, Adobe RGB (1998), ProPhoto, Rec. 2020 and the RGB
 * profiles a camera embeds. For such a profile the PCS white is ALWAYS D50
 * and the rXYZ/gXYZ/bXYZ tags are already D50-adapted; wtpt and chad are
 * advisory and are used for nothing (macOS's own sRGB Profile.icc has
 * D50-adapted columns, no chad at all and a wtpt holding the unadapted D65,
 * so a reader that trusts wtpt gets sRGB wrong). dr_icc_inspect classifies
 * raw bytes five ways, and each way means something different:
 *
 *   DR_ICC_NOT_ICC            not a profile          — refused, never stored
 *   DR_ICC_NOT_RGB            Gray / CMYK / Lab      — refused, never stored
 *   DR_ICC_NOT_IMAGE_PROFILE  RGB numbers, but a     — refused, never stored:
 *                             device-link, abstract    it describes a
 *                             or named-colour class    transform or a colour
 *                                                      list, not the space a
 *                                                      picture's numbers live
 *                                                      in (macOS's own
 *                                                      WebSafeColors.icc),
 *                                                      and CoreGraphics
 *                                                      cannot convert out of
 *                                                      one
 *   DR_ICC_RGB_UNCONVERTIBLE  RGB, but LUT-based     — KEPT as the document's
 *                                                      profile: pixels are
 *                                                      untouched, display is
 *                                                      correct and an export
 *                                                      re-embeds the original
 *                                                      bytes; only convert
 *                                                      refuses
 *   DR_ICC_RGB_MATRIX         RGB matrix/TRC         — full function
 *   DR_ICC_RGB_LUT            RGB, LUT-based, with   — KEPT as the document's
 *                             A2B/B2A tables this      profile AND readable:
 *                             build can read           dr_doc_profile_is_
 *                             (Phase 11C)              convertible stays
 *                                                      FALSE (it is the
 *                                                      matrix/TRC question
 *                                                      document_v2 Exposure
 *                                                      and White Balance ask)
 *                                                      while dr_doc_profile_
 *                                                      can_decode is TRUE
 *
 * ASSIGN IS NOT CONVERT. dr_doc_assign_profile REINTERPRETS — the pixels are
 * untouched and the profile is replaced, so the numbers stay and the
 * appearance changes. dr_doc_convert_to_profile TRANSFORMS every layer's
 * pixels, so the appearance stays and the numbers change. Convert touches
 * layer pixels only: layer masks, alpha channels and layer metadata are
 * coverage and descriptions, not colour. Nor does it touch the colours
 * inside a LAYER STYLE or a text layer's metadata, and that is not a limit:
 * those are AUTHORED sRGB values — the same convention every #rrggbb this
 * library is handed follows — and they reach the document's space where they
 * are USED. A style's colours convert at composite time, so a style keeps
 * its appearance across a convert, the stored JSON is never rewritten and
 * the rendered-plane cache never has to be thrown away; a text layer's
 * convert when the host re-renders it. dr_doc_open never adopts a working
 * space — it reports what the file said, and the host calls
 * dr_doc_adopt_working_space exactly once.
 *
 * CAPTURE PACKETS. JPEG/PNG retain EXIF and XMP packet bytes; JPEG also
 * retains the whole Photoshop 8BIM resource run, concatenated across APP13
 * segments, and merges its ExtendedXMP chunks into the standard XMP packet,
 * so the XMP slot holds the one LOGICAL packet (a merge that cannot be made
 * keeps the standard packet, and dr_path_metadata_import_report's warnings
 * say why). TIFF (classic or BigTIFF) rebuilds standard IFD0/EXIF/GPS/Interop fields with
 * valid offsets and reads XMP/IPTC tags. A MakerNote (BYTE or UNDEFINED) is
 * carried as one opaque value, byte for byte; other private TIFF capture
 * fields are omitted on import with a diagnostic. EXIF is a standalone TIFF
 * stream without an Exif prefix; XMP has no container identifier; IPTC is
 * an 8BIM run (raw TIFF IIM is wrapped as its IPTC resource). WebP writes ICC
 * and EXIF but does not read capture packets; BMP/GIF carry none. Platform
 * readers can attach reconstructed packets using dr_doc_import_metadata.
 * EXIF density outranks JFIF/PNG density; TIFF reads its rational PPI tags.
 *
 * TWO REWRITES HAPPEN ON EXPORT, both mandatory. (1) The EXIF block's IFD0
 * Orientation is set to 1, because the camera rotation was already baked
 * into the pixels at open time and a preserved 6 would double-rotate in
 * every viewer; its resolution and dimension tags are brought into line with
 * the document, and its next-IFD link is zeroed so the stale pre-edit
 * thumbnail is unreferenced. Every edit is in place — an ABSENT tag is never
 * inserted, because growing an IFD shifts every out-of-line datum and
 * silently corrupts MakerNotes — and a packet whose IFD0 or Exif sub-IFD
 * does not verify is dropped whole, as is one whose value offsets reach into
 * an entry table, where one rewrite would overwrite another. Nothing PAST them is read: the link to
 * IFD1 is cut anyway, so a dangling one (what a tool that strips a thumbnail
 * without rewriting the link leaves behind) costs the thumbnail rather than
 * every tag in the packet. (2) The 8BIM run loses resources 0x03ED (ResolutionInfo),
 * 0x040F (ICC), 0x0422 (Exif) and 0x0424 (XMP), which would contradict what
 * the save just wrote; every other resource survives byte-exactly. Both
 * happen on WRITE, so the stored packets stay byte-exact. On the same
 * terms the XMP packet's own orientation and resolution are rewritten, and
 * every flat export drops Google's pixel-bound GImage:/GDepth: properties
 * (the uncropped original and the depth map) and any stale
 * xmpNote:HasExtendedXMP; the .rz keeps them. A JPEG export splits an XMP
 * packet too large for one APP1 into a standard packet and ExtendedXMP
 * chunks rather than dropping it.
 *
 * A blob the format cannot carry, one too large for its segment, or an 8BIM
 * run that filters down to nothing is DROPPED AND REPORTED, never an error:
 * a save must not fail because a source file had a fat profile.
 * dr_format_carries answers what a format is able to carry;
 * dr_doc_save_image reports what was actually written. */

typedef enum {
  DR_PROFILE_SRGB = 0,
  DR_PROFILE_DISPLAY_P3 = 1,
  DR_PROFILE_ADOBE_RGB = 2,
  DR_PROFILE_PROPHOTO_RGB = 3,
} DrBuiltinProfile;

typedef enum {
  DR_METADATA_EXIF = 0,
  DR_METADATA_XMP = 1,
  DR_METADATA_IPTC = 2,
} DrMetadataKind;

/* dr_icc_inspect classifications. */
#define DR_ICC_NOT_ICC           0
#define DR_ICC_NOT_RGB           1
#define DR_ICC_RGB_UNCONVERTIBLE 2
#define DR_ICC_RGB_MATRIX        3
#define DR_ICC_NOT_IMAGE_PROFILE 4
#define DR_ICC_RGB_LUT           5

/* dr_doc_adopt_working_space outcomes. */
#define DR_ADOPT_UNCHANGED          0
#define DR_ADOPT_CONVERTED          1
#define DR_ADOPT_KEPT_UNCONVERTIBLE 2

/* dr_format_carries / dr_doc_save_image bits. */
#define DR_CARRIES_PROFILE     1u
#define DR_CARRIES_EXIF        2u
#define DR_CARRIES_XMP         4u
#define DR_CARRIES_IPTC        8u
#define DR_CARRIES_RESOLUTION 16u

/* --- the built-in profiles this build writes --- */

/* Byte length of a built-in profile; 0 for a value outside
 * DrBuiltinProfile. The bytes are identical on every call and every run. */
size_t dr_builtin_profile_len(DrBuiltinProfile which);

/* Copies a built-in profile into `out`, which the caller declares to be
 * `len` bytes. The length is recomputed from the library's own and must
 * match exactly; false on a NULL buffer, a length that disagrees, or a value
 * outside DrBuiltinProfile. */
bool dr_builtin_profile(DrBuiltinProfile which, uint8_t *out, size_t len);

/* --- inspecting raw bytes, without a document --- */

/* Classifies `bytes` as one of the DR_ICC_* values and, when `name_out` is
 * non-NULL, writes the profile's display name there as a heap string freed
 * with dr_string_free (NULL for DR_ICC_NOT_ICC). Ask this before assigning
 * to learn WHY a profile would be refused. */
int dr_icc_inspect(const uint8_t *bytes, size_t len, char **name_out);

/* True when the opener reads capture metadata from this container: JPEG,
 * PNG, TIFF (classic or BigTIFF) or native .rz. Packet limits apply; TIFF preserves
 * standard capture fields and reports private-field losses separately via
 * dr_path_metadata_import_report. False for other containers, unreadable
 * paths or NULL. */
bool dr_path_metadata_walked(const char *path);

/* JSON metadata import report: container, metadata_walked, supported_packets,
 * limitations, warnings. TIFF reports standard capture relocation limits and
 * detected packet losses. A JPEG lists "XMP (including Extended XMP)" and
 * its warnings carry any ExtendedXMP merge that could not be made. `profile_refusal`, present only for a flat file
 * whose embedded colour profile the open converts from (Gray, CMYK TIFF or
 * JPEG) or ignores, or for an untagged CMYK TIFF or JPEG (converted from the
 * working CMYK profile, which the sentence names, or read without colour
 * management when none is set — dr_set_working_cmyk_profile), is that one
 * sentence, also the last entry of `warnings`. `tiff_pages`, present only
 * for a TIFF of more than one page, is its page count; `warnings` carries
 * dr_doc_open's sentences about a TIFF's pages.
 * A CMYK file dr_image_open refuses has no `profile_refusal`; a host that
 * decodes it itself words that conversion with
 * dr_platform_conversion_sentence. Never decodes pixels. Free with
 * dr_string_free.
 * NULL on NULL path/read failure/panic, with err_out explaining the error. */
char *dr_path_metadata_import_report(const char *path, char **err_out);

/* The colour model of an image a platform decoder (ImageIO, CoreGraphics)
 * converted to sRGB as it read it. */
#define DR_PLATFORM_SOURCE_GRAY 0
#define DR_PLATFORM_SOURCE_CMYK 1
#define DR_PLATFORM_SOURCE_LAB  2

/* The import-report sentence for that conversion.
 * `bytes` is the encoded image the platform decoded — a file's contents, a
 * pasteboard's own TIFF or PNG; NULL or empty is an image with no profile.
 * When the core's own open would convert such samples from the embedded
 * profile (a Gray profile over Gray, a CMYK one with a device-to-PCS table
 * over CMYK), the open's converted sentence naming that profile; otherwise
 * "The image is {Gray|CMYK|Lab}, with no embedded colour profile Doctored
 * can convert from, so it was converted to sRGB as it was read." — always
 * for Lab. NULL with err_out ("unknown platform source N") for any other
 * `source`. Never decodes pixels. Free with dr_string_free. */
char *dr_platform_conversion_sentence(const uint8_t *bytes, size_t len, int source,
                                      char **err_out);

/* Sets the process-wide WORKING CMYK profile: an untagged CMYK TIFF
 * or JPEG the core opens is converted from it to sRGB (relative colorimetric
 * with black-point compensation, as an embedded CMYK profile converts), and
 * its import-report `profile_refusal` names it. NULL `icc` or `len == 0`
 * clears it, and an untagged CMYK file then reads without colour
 * management; returns true. False with err_out when the bytes are not a CMYK
 * ICC profile with a relative-colorimetric device-to-PCS table; the profile
 * already set is then kept. dr_platform_conversion_sentence never names it:
 * a platform decode of an untagged file did not convert from it. */
bool dr_set_working_cmyk_profile(const uint8_t *icc, size_t len, char **err_out);

/* True when both buffers are matrix/TRC RGB profiles describing the SAME
 * colour space — the question dr_doc_convert_to_profile refuses on, asked
 * without a document so a host can disable a Convert command instead of
 * discovering the refusal as a bare NULL. Byte equality is NOT the same
 * question: the "sRGB IEC61966-2.1" blob most cameras and Photoshop embed is
 * 3144 bytes against this library's 2568, and the two describe one space.
 * False on a NULL pointer, on bytes that are not an RGB ICC profile, and on
 * a profile this library cannot model — those are other refusals, which
 * dr_icc_inspect names. */
bool dr_icc_describes_same_space(const uint8_t *a, size_t a_len,
                                 const uint8_t *b, size_t b_len);

/* --- the document's profile --- */

/* Byte length of the document's ICC profile; 0 on a NULL doc. Never 0 for a
 * live document — a document always has a profile. */
size_t dr_doc_icc_profile_len(const DrDocument *doc);

/* Copies the document's profile into `out`, which the caller declares to be
 * `len` bytes; false on a NULL doc, a NULL buffer, or a `len` that
 * disagrees with the library's own length. */
bool dr_doc_icc_profile(const DrDocument *doc, uint8_t *out, size_t len);

/* Heap copy of the profile's display name (free with dr_string_free); NULL
 * on a NULL doc. */
char *dr_doc_profile_name(const DrDocument *doc);

/* True when the document's profile is an RGB matrix/TRC one — the model
 * document_v2 Exposure/White Balance and linear-light evaluation run
 * through. Since Phase 11C it is NOT the Convert source gate (that is
 * dr_doc_profile_can_decode below). False on a NULL doc and for a LUT-based
 * profile, which is kept and re-embedded all the same. */
bool dr_doc_profile_is_convertible(const DrDocument *doc);

/* True when this build can turn the document's pixel numbers into PCS colour:
 * a matrix/TRC profile, or a LUT profile with a device-to-PCS table (Phase
 * 11C). False on a NULL doc. This is the Convert to Profile SOURCE gate and
 * the one a host should ask before offering a conversion; the narrower
 * dr_doc_profile_is_convertible above keeps its matrix/TRC meaning because
 * the document_v2 calibrated adjustments depend on it. */
bool dr_doc_profile_can_decode(const DrDocument *doc);

/* --- pure ops --- */

/* Reinterprets the document in the profile `bytes` describe: pixels
 * unchanged, profile replaced. NULL WITHOUT an error on a NULL doc and on
 * bytes identical to the current profile (nothing would change, so no undo
 * step). NULL WITH err_out set (free with dr_string_free) — the typed pair's
 * sentences, from one home — for a NULL buffer or bytes that are not an RGB
 * ICC profile ("the profile is not an RGB ICC profile a document can
 * carry") and for a payload over 16 MiB ("the profile exceeds the 16 MiB
 * limit"; the DRDC writer's blob cap, enforced here so a document can never
 * hold a profile the format would refuse); then with dr_doc_assign_refusal's
 * sentence for a retained source, and with dr_doc_assign_profile_refusal's
 * sentence whenever that function is non-NULL.
 * `bytes` NULL is NOT a clear: pass the sRGB built-in to reset. */
DrDocument *dr_doc_assign_profile(const DrDocument *doc, const uint8_t *bytes,
                                  size_t len, char **err_out);

/* Transforms every layer's pixels into the profile `bytes` describe and
 * replaces the profile. NULL WITHOUT an error on a NULL doc and for a target
 * equivalent to the document's current space (nothing would change). NULL
 * WITH err_out set for dr_doc_assign_profile's profile-argument sentences,
 * for anything dr_doc_conversion_preflight refuses at the default options,
 * in its words (among them a document_v2 Exposure/White Balance layer or
 * linear-light evaluation carried into a profile with no matrix/TRC model),
 * and for a source that cannot decode or a destination that cannot encode. */
DrDocument *dr_doc_convert_to_profile(const DrDocument *doc,
                                      const uint8_t *bytes, size_t len,
                                      char **err_out);

/* Brings a freshly opened document into the host's working space, ONCE:
 * converts when the two differ, does nothing when they agree, and keeps the
 * document's own profile when it is one this library cannot convert from.
 * `outcome_out` (may be NULL) receives one of the DR_ADOPT_* values and is
 * written EVEN WHEN the return is NULL, so a "nothing changed" adoption
 * still says which case it was. Skip this for a .rz document: it carries its
 * own profile. */
DrDocument *dr_doc_adopt_working_space(const DrDocument *doc,
                                       const uint8_t *bytes, size_t len,
                                       int *outcome_out);

/* --- metadata packets --- */

/* Byte length of one packet; 0 on a NULL doc, an absent packet, or a value
 * outside DrMetadataKind. */
size_t dr_doc_metadata_len(const DrDocument *doc, DrMetadataKind kind);

/* Copies one packet into `out`, which the caller declares to be `len`
 * bytes; false on a NULL doc, a NULL buffer, an absent packet, a value
 * outside DrMetadataKind, or a `len` that disagrees with the library's own
 * length. */
bool dr_doc_metadata(const DrDocument *doc, DrMetadataKind kind, uint8_t *out,
                     size_t len);

/* Stores one packet verbatim, or CLEARS it when `bytes` is NULL — a zero
 * `len` with a non-NULL pointer stores an EMPTY packet, which the format
 * distinguishes from an absent one. NULL for a NULL doc or a value the
 * document already carries leaves err_out untouched; a refusal (a kind
 * outside DrMetadataKind, a packet over 16 MiB) returns NULL and sets
 * err_out (free with dr_string_free). */
DrDocument *dr_doc_set_metadata(const DrDocument *doc, DrMetadataKind kind,
                              const uint8_t *bytes, size_t len, char **err_out);

/* Replace capture packets from a JPEG/PNG/TIFF metadata carrier, at most 49 MiB.
   Does not decode pixels or adopt the carrier's profile/resolution. Pure; NULL
   for NULL input, unchanged packets, or an error (only errors set err_out).
   warnings_out receives a JSON array of detected packet/field omissions,
   including on unchanged metadata. NULL discards it; free with dr_string_free. */
DrDocument *dr_doc_import_metadata(const DrDocument *doc, const uint8_t *bytes,
                                 size_t len, char **warnings_out, char **err_out);

/* --- file info --- */

/* The creator, copyright notice, description and keywords the document's
 * metadata states, as JSON with all four keys:
 * {"creator":[string],"copyright":string,"description":string,"keywords":[string]}.
 * Per field the XMP value (dc:creator, dc:rights, dc:description, dc:subject),
 * else the IIM dataset (2:80, 2:116, 2:120, 2:25), else the EXIF IFD0 tag
 * (315, 33432, 270). NULL on a NULL doc. Free with dr_string_free. */
char *dr_doc_file_info(const DrDocument *doc);

/* Pure setter from that same JSON, which must hold exactly the four keys.
 * Limits, in characters: a list entry 256, 256 entries per list, copyright
 * 2000, description 8000; control characters refuse, except a line feed in
 * the description; entries are trimmed and blank entries dropped. The fields
 * are written into the XMP packet (created when absent and a field is not
 * empty); the IIM datasets and their digest are dropped from the IPTC packet;
 * an EXIF packet's IFD0 tags 315/33432/270 are set, or removed when they would
 * push the packet past what a JPEG segment holds. NULL for a NULL doc or when
 * the metadata would not change leaves err_out untouched; a refusal (NULL or
 * malformed JSON, a limit, an XMP packet with no </rdf:RDF>) returns NULL and
 * sets err_out (free with dr_string_free). PIXELS NEVER CHANGE. */
DrDocument *dr_doc_set_file_info(const DrDocument *doc, const char *json, char **err_out);

/* --- resolution --- */

/* The document's print resolution in ppi; 0.0 on a NULL doc. */
float dr_doc_resolution_x(const DrDocument *doc);
float dr_doc_resolution_y(const DrDocument *doc);

/* Pure setter for the print resolution. Sanitizing clamps to [1, 30000]
 * and quantizes to four decimals like the global light, so a reported value
 * echoed back is "no change". NULL for a NULL doc or no change leaves
 * err_out untouched; a non-finite or non-positive component returns NULL and
 * sets err_out (free with dr_string_free). PIXELS NEVER CHANGE: only the
 * print size does. */
DrDocument *dr_doc_set_resolution(const DrDocument *doc, float ppi_x,
                                  float ppi_y, char **err_out);

/* --- saving a flat file WITH the profile and the packets --- */

/* Which of the profile, the three packets and the resolution `format` is
 * able to carry, as DR_CARRIES_* bits; 0 for a value outside DrFormat.
 *
 *   PNG   profile (iCCP), exif (eXIf), xmp (iTXt), resolution (pHYs)
 *   JPEG  profile (APP2 x n), exif (APP1), xmp (APP1 + ExtendedXMP APP1 x n),
 *         iptc (APP13 x n), resolution (JFIF density, per axis)
 *   TIFF  profile (ICC tag), standard EXIF/GPS/Interop IFDs, xmp (700),
 *         iptc (34377), resolution (rational PPI, per axis)
 *   WebP  profile, exif
 *   BMP, GIF   nothing
 *
 * The resolution column is the format's OWN density slot, and a save may
 * report DR_CARRIES_RESOLUTION beyond it: a WebP has no such slot but states
 * the ppi inside the EXIF chunk it does write, so dr_doc_save_image reports
 * what the file ends up saying rather than what this table alone allows.
 *
 * TIFF reads back its written profile and PPI. EXIF export requires safe
 * relocation of standard capture fields, and a MakerNote (BYTE or
 * UNDEFINED) relocates with them as one opaque value: a packet containing
 * any other private field or malformed pointers is dropped WHOLE and
 * reported as not carried. Other private capture fields and later image
 * directories are outside this implementation. */
uint32_t dr_format_carries(DrFormat format);

/* Encodes the document to `path` as a flat image, embedding its colour
 * profile (unless `embed_profile` is false) and its EXIF, XMP and IPTC
 * packets (unless `strip_metadata` is true), format permitting, plus its
 * print resolution wherever the format has somewhere to put one.
 * `strip_metadata` governs the three INHERITED packets only — the
 * document's own resolution and profile are its state, not something it
 * inherited from a file.
 *
 * `flat` MAY BE NULL, in which case the document is flattened here; when it
 * is non-NULL its pixels are what gets encoded and the document supplies
 * only the profile, the packets and the resolution. `flat` is a
 * caller-supplied composite of THIS document; passing an unrelated image is
 * a caller bug, and the canvas dimensions are not re-checked.
 *
 * A TIFF keeps the document's spot channels (#1 1c-2a) as Photoshop does:
 * trailing extra samples (ExtraSamples 0) after the colour and, only when
 * some pixel is not opaque, a straight alpha, each stored `255 − coverage`,
 * with their names and inks as resources 1006/1045/1077/1053 after the
 * inherited 8BIM run in tag 34377. `flat` should then be the plain
 * composite: the overprint is not composited into a file that keeps the
 * channels. Every other format writes `flat` as given.
 *
 * `carried_out` (may be NULL) receives the DR_CARRIES_* bits actually
 * written. Atomic like dr_image_save; errors as in dr_image_save. */
bool dr_doc_save_image(const DrDocument *doc, const DrImage *flat,
                       const char *path, DrFormat format, uint8_t jpeg_quality,
                       bool embed_profile, bool strip_metadata,
                       uint32_t *carried_out, char **err_out);

/* Returns a heap JSON array of editable/sidecar state a flat save cannot
 * preserve on reopen, using the same packet limits as dr_doc_save_image.
 * [] means no detected state loss; JPEG compression/GIF quantization are
 * still intentional format choices. No rendering or encoding. NULL on
 * invalid doc/format or failure, with err_out. Free with dr_string_free.
 * Native save preserves this state; explicit export may deliberately lose it. */
char *dr_doc_flat_save_losses(const DrDocument *doc, DrFormat format,
                              bool embed_profile, bool strip_metadata,
                              char **err_out);
/* Its typed twin is dr_precision_doc_flat_state_losses (after the
 * DrPrecisionDocument typedef). */

/* ---- Embedded agent (MCP) server, on a Unix socket ----------------------
 *
 * A minimal MCP server (streamable-HTTP transport over a Unix socket, tools
 * only) hosted in this library so an external AI agent (any MCP client,
 * through the bundled doctored-mcp stdio helper) can drive the
 * application. The library owns the protocol; the application supplies the
 * tool catalog and executes tool calls via the handler. One listener per
 * process. */

/* Executes one tool call. Called from the server thread for every
 * `tools/call`; `arguments_json` is the call's arguments object. Must
 * return a CallToolResult JSON object (`{"content": [...], "isError":
 * bool}`) allocated with dr_agent_string_create, or NULL to report a
 * handler failure. */
typedef char *(*DrAgentToolHandler)(void *context, const char *tool_name,
                                    const char *arguments_json);

/* Allocates a library-owned copy of `s` for handler return values.
 * Ownership passes back to the library on return; never free it. */
char *dr_agent_string_create(const char *s);

/* Binds the MCP server on a Unix socket at `socket_path` (absolute, at most
 * 103 bytes) and makes the file 0600; the endpoint answers JSON-RPC POSTs
 * to /mcp only (another path is 404, another method 405). Before the bind
 * the socket's folder is made 0700 and must be a real folder owned by this
 * user with no group or world bits; a socket at the path that accepts a
 * connection is another instance's and is left alone, one that refuses is
 * a leftover and is unlinked, and a file that is not a socket is left
 * alone. `tools_json` is the tools/list catalog as a JSON array. A second
 * start fails ("agent socket already bound"). `handler` and `context` must
 * stay valid and callable from any thread for the life of the process.
 * Returns true, or false with a message through err_out (free with
 * dr_string_free). */
bool dr_agent_socket_start(const char *socket_path, const char *server_name,
                           const char *server_version, const char *tools_json,
                           DrAgentToolHandler handler, void *context,
                           char **err_out);

/* For quit: unlinks the socket file only if it is still the one this
 * listener bound (same device and inode), then abandons the listener
 * without a join, so a call in flight cannot hold the caller. Safe with
 * none bound. */
void dr_agent_socket_release(void);

/* The three containers a layered picture travels in. The layered export
 * calls take one before their mode; the import calls read all three and name
 * which in the report's "format". */
typedef enum {
  DR_LAYERED_PSD = 0,  /* classic PSD: 30000 px per side, 2 GiB */
  DR_LAYERED_PSB = 1,  /* large document: 300000 px per side */
  DR_LAYERED_TIFF = 2  /* a flat RGBA (or CMYKA) TIFF with Photoshop's layer
                          block (tags 34377 and 37724): 30000 px per side,
                          4 GiB */
} DrLayeredContainer;

/* True when `path` is a TIFF whose Photoshop layer block this core reads as
 * layers: a big-endian unsigned 8- or 16-bit RGB, CMYK (InkSet absent or
 * 1, #1), BlackIsZero gray (#1 phase 3a) or Lab TIFF holding a
 * Layr/Lr16 block, or a 32-bit IEEE-float RGB TIFF of either byte order
 * holding an Lr32 block. False for NULL, unreadable, little-endian
 * at 8 or 16 bits, any other model or sample format, or no readable block:
 * each of those opens flat. Reads the header, the first directory and the
 * block keys, never a pixel strip. */
bool dr_tiff_has_layers(const char *path);

/* PSD, PSB and layered-TIFF compatibility preflight ("format" is "PSD", "PSB"
 * or "TIFF"). A layered TIFF has no stored composite here (its flat image is
 * the ordinary TIFF open): "stored_composite_available" is false with the
 * reason, and mode 1 returns that reason. ZIP and ZIP-with-prediction
 * channels, masks and composites are read at both depths. A PSB record
 * holding a 64-bit-length block refuses the LAYERED choice by name.
 * JSON separates verified_losses, observations,
 * and general_limitations, and reports availability of each import choice.
 * A record's knko (shallow/deep) and infx ride the ONE carried layer style at
 * both depths and are named in that record's style observation; clbl is never
 * interpreted. A file with no true Photoshop Background record whose bottom
 * entry is Doctored's structural Background, under a knockout that reaches
 * it, carries ONE verified loss stating the divergence (D13 / P7-38).
 * A CMYK file (colour mode 4: PSD, PSB or a layered CMYK TIFF) answers
 * "import_precision":"cmyk" at 8 and 16 bits, its layered availability and
 * "layered_plan_bytes" (the Import Layers plan), its saved composite's
 * availability (a TIFF has none), the shared record walk's losses and
 * observations — a carried style's effects (CMYC inks), knockout, Blend
 * Interior and Blend If observed on every record kind, a Pass Through group
 * (composited isolated) and an adjustment record included — and the CMYK
 * general limitations
 * (#1); its embedded
 * profile is read as CMYK, never through the RGB profile rule.
 * A 32-bit RGB PSD or PSB answers "import_precision":"rgba32" and opens as a
 * 32-bit float document.
 * A Grayscale file (colour mode 1: PSD, PSB or a layered gray TIFF) at 8 or
 * 16 bits, or a PSD or PSB at 32, answers "import_precision":"gray" (it opens as a Grayscale
 * document through dr_precision_doc_open_gray_psd): its records decoded with
 * the file's Gray profile (a Grsc effect colour included), each style carried
 * or discarded by the Gray rules, a refused adjustment and every fill layer
 * omitted by name, "profile" the embedded Gray profile's name (null when
 * untagged: the working Gray is assigned at open) and the Gray general
 * limitations (#1 phase 3a). At 32 bits (phase 3c) the float rules apply
 * too, and the linear twin of a built-in Gray profile names that built-in.
 * A Lab file (colour mode 9: PSD, PSB or a layered Lab TIFF) at 8 or 16 bits
 * answers "import_precision":"lab" (it opens as a Lab Color document through
 * dr_precision_doc_open_lab_psd): "color_mode":"Lab", "profile" null (an
 * embedded profile is unused and named), "layered_plan_bytes", each style
 * carried or discarded by the Lab rules (an LbCl effect colour as its sRGB
 * hex), a refused adjustment and every fill layer omitted by name, and the
 * Lab general limitations (#1 Lab phase 3a).
 * An Indexed Color file (colour mode 2: PSD or PSB) answers
 * "import_precision":"indexed" (it opens as an Indexed Color document
 * through dr_precision_doc_open_indexed_psd): "color_mode":"Indexed",
 * "layered_available" false, "stored_composite_available" with the open's
 * own refusal sentence (the depth, the 768-byte table, the palette size,
 * Raw or RLE), one loss per extra saved channel, and "profile" the 1039
 * profile's name or null (#1).
 * An RGB, CMYK, Grayscale or Duotone PSD or PSB at 8 or 16 bits names its
 * spot channels (DisplayInfo kind spot, a Lab ink) in one observation: every
 * open keeps them as spot channels with name, D50 Lab ink, solidity and
 * coverage; a spot channel it cannot keep, and every alpha channel, stays a
 * loss (#1 spot channels 1b-1). A layered RGB or CMYK TIFF at 8 or 16 bits
 * names and keeps the spot channels its composite holds as trailing extra
 * samples the same way (#1 spot channels 1c-2b).
 * Free the returned string with dr_string_free; NULL on failure. */
char *dr_psd_import_report(const char *path, char **err_out);
/* Explicit choice: mode 0 layers, 1 file-stored composite. Requires the
 * source_fingerprint from preflight; a changed source refuses. No implicit
 * color/depth conversion or fallback between modes. */
DrDocument *dr_doc_open_psd(const char *path, int mode,
                          const char *expected_fingerprint, char **err_out);

/* Layered export preflight for one container (DrLayeredContainer) and mode
 * (0 layers, 1 flattened composite). JSON:
 * {"format":"PSD"|"PSB"|"TIFF","container":same,"mode":"layers"|"flattened",
 * "depth":8,"available":bool,
 * "refusal":string|null,"losses":[],"observations":[],"general_limitations":[],
 * "layer_count":n,"group_count":n,"worst_case_bytes":n,
 * "bake_scratch_bytes":n}. No pixels are read. An adjustment layer never
 * refuses: one with a Photoshop record is written as that record, any other
 * is BAKED into a canvas-sized pixel layer holding the picture up to and
 * including it, with one loss per baked layer; "bake_scratch_bytes" is the
 * working memory those stamps need: three canvas-sized RGBA planes at the
 * document's depth, 0 when nothing bakes.
 * A written style's knockout keys are named per layer in "observations"; a
 * BAKED one's are a per-layer loss (neither key is written beside baked
 * pixels), and a knockout over this document's structural Background is ONE
 * per-file loss (D13 / P7-38). Free with dr_string_free; NULL + err_out on
 * NULL doc / bad container / bad mode. */
char *dr_doc_psd_export_report(const DrDocument *doc, int container, int mode,
                               char **err_out);
/* Writes an 8-bit RGB PSD, PSB or layered TIFF (PackBits channels; a TIFF's
 * flat image is an uncompressed straight-RGBA strip). Re-runs the preflight
 * and fails with its refusal. `flat` MAY BE NULL (flattened here); non-NULL must be
 * canvas-sized or the call fails. A written layer style adds Photoshop's own
 * infx then knko records after iOpa and before lclr, on rasters and on
 * OPENING folder records, only when the key is not at its default; a baked
 * style writes neither. A PSD or PSB carries the document's spot channels
 * (at most 56 channels in all) in Photoshop's layout, and a layered TIFF
 * carries them as its composite's trailing extra samples (#1 1c-2b); alpha
 * channels are not written (#1 spot channels 1b-1). Atomic like
 * dr_image_save. */
bool dr_doc_save_psd(const DrDocument *doc, const DrImage *flat,
                     const char *path, int container, int mode, char **err_out);

/* 16-bit RGB PSDs (import report "import_precision":"rgba16") open only as
 * 16-bit documents; the three calls below are declared with the other
 * DrPrecisionDocument entry points, after their opaque typedefs. See
 * dr_precision_doc_open_psd / _psd_export_report / _save_psd. */

/* Frees strings returned via err_out parameters. NULL is a safe no-op. */
void dr_string_free(char *s);

/* Apply an adjustment to a full image using this document's colour context.
 * Same op/params as dr_image_adjust_op; params.evaluation="document_v2"
 * on Exposure/White Balance uses supported RGB matrix/TRC profiles. Missing
 * evaluation or "legacy_srgb" preserves legacy numeric processing. Alpha is
 * untouched. Selection/target writeback is separate. NULL on NULL doc/image;
 * invalid parameters or unsupported profile return NULL + err_out. A
 * document_v2 request is refused with dr_doc_calibrated_refusal's sentence
 * for its op before anything is evaluated. */
DrImage *dr_doc_adjust_image(const DrDocument *doc, const DrImage *img,
                             const char *op, const char *params_json, char **err_out);
/* Why a NEW or EDITED document_v2 `op` ("exposure" | "white_balance"; NULL or any
 * other string answers as "white_balance") -- layer create, layer edit,
 * dr_doc_adjust_image, editable paste -- cannot evaluate in the document's profile:
 * an owned sentence (free with dr_string_free) when the profile has no matrix/TRC
 * model (both ops), or when its tone curves fail the precision check (the
 * own-profile sentence naming it) -- for Exposure only where the curves are
 * repaired or unusable (#2); NULL when admitted and on a NULL doc. Existing
 * layers are not judged: an 8-bit document already holding one keeps rendering. */
char *dr_doc_calibrated_refusal(const DrDocument *doc, const char *op);


/* Background operation control. A token is single-use and must remain live until
 * worker and all polling/cancel calls finish. Cancel/query may run concurrently;
 * free and scratch-limit configuration must not race other calls. Progress is
 * monotone [0,1], stage: 0 ready,1 preparing,2 planning,3 inpainting,4 blending,
 * 5 complete,6 rendering. Scratch bytes is a conservative working reservation,
 * zero after return; excludes input handles/caller buffers and cache storage. */
DrJobControl *dr_job_control_create(void);
void dr_job_control_free(DrJobControl *control);
void dr_job_control_cancel(const DrJobControl *control);
bool dr_job_control_cancelled(const DrJobControl *control);
double dr_job_control_progress(const DrJobControl *control);
uint32_t dr_job_control_stage(const DrJobControl *control);
uint64_t dr_job_control_scratch_bytes(const DrJobControl *control);
/* 0 means unlimited. False for NULL or an already started token. */
bool dr_job_control_set_scratch_limit(const DrJobControl *control, uint64_t bytes);
/* The host's whole working limit (0 = unknown), set before start. A reservation over the scratch
 * limit that is also at least this says closing other documents will not help. False for NULL or a started token. */
bool dr_job_control_set_working_limit(const DrJobControl *control, uint64_t bytes);
/* Atomically claim a single-use parent for host-composed child operations.
 * No core scratch is reserved. The host aggregates child progress and forwards
 * cancellation; every child core call requires a separate fresh control.
 * Parent reuse by this or another core operation refuses. NULL returns false;
 * cancellation/reuse returns false plus an optional owned dr_string_free error. */
bool dr_job_control_begin_compound(const DrJobControl *control, char **err_out);
/* Exact Content-Aware Fill candidate. Request JSON (<=64 KiB) uses op:
 * "content_aware_fill", layer, x/y/width/height selection bounds, ring (0..512),
 * seed, sample (current/current_and_below/all), ignore_adjustments, output
 * (layer/new_layer), optional source_mask. Bounds must contain all nonzero
 * selection coverage. The hole is every pixel with coverage > 0, and the soft
 * bytes weight the write-back, so a feathered selection fills and fades
 * across its whole ramp. Disjoint parts fill independently from their own
 * neighbourhoods. Ring 0 is automatic, and any ring is raised to at least 21.
 * The caps bound the whole call: 4,000,000 pixels of selection dilated by its
 * ring, 80,000,000 of working windows, 1,000,000 separate parts, and
 * 32,000,000 for one part's window. The covered count is not capped:
 * over 1,000,000 covered pixels the search runs at 1/2 scale (1/4 over
 * 4,000,000, 1/8 over 16,000,000), the finer levels copy source pixels
 * through the doubled field without searching, and the dilated cap is 4 times
 * larger per halving; a part whose window is too narrow to reduce and holds
 * more than 1,000,000 pixels to search is refused. NULL with *err_out on a
 * cap or a starved sample region. The typed edit API accepts the same request with u16
 * coverage, keeping native search, solve and soft writeback.
 * source_mask: {version:1,initial:"all"|"none",operations:[...]}; ordered
 * include_rect/exclude_rect {x,y,width,height} use half-open pixel-center tests;
 * include_brush/exclude_brush {radius,points:[[x,y],...]} use connected round
 * capsules, radius 0.5..4096. Max 512 operations, 4096 points, 256M box visits.
 * Excluded samples remain boundary context but cannot vote at any pyramid level.
 * New output deposits the solved result above target; original layers are exact.
 * Gated groups refuse. Partial-alpha plates compound alpha when overlaid.
 * Scratch excludes retained inputs; 0 on error. NULL control allowed; supplied
 * tokens single-use. NULL result/no error means no change; cancellation/error
 * never publishes partial pixels. Free returned owners with their matching free.
 */
uint64_t dr_doc_content_aware_fill_scratch(const DrDocument *doc,
    const char *request_json, char **err_out);
DrDocument *dr_doc_content_aware_fill_edit(const DrDocument *doc,
    const char *request_json, const uint8_t *coverage,
    const DrJobControl *control, char **err_out);

/* Independent layer masks. Properties JSON is NULL without a mask, otherwise
 * {linked,density,feather,outside_coverage,offset_x,offset_y,width,height}; free with dr_string_free.
 * outside_coverage is raw coverage outside the stored rectangle (0 or255);
 * existing property setters preserve it. Position is absolute canvas space. Density [0,1], feather sigma [0,1000] pixels;
 * setters preserve original coverage and return NULL for no-op/refusal.
 * Mask transforms preserve layer pixels, use [a,b,c,d,tx,ty] in canvas space,
 * and accept existing DrResizeFilter numeric values. */
char *dr_doc_layer_mask_properties(const DrDocument *doc, size_t idx);
/* Exact raw coverage, placement, properties and enabled-state equality at idx.
 * No pixel copies. False on NULL documents or invalid indices. */
bool dr_doc_layer_masks_equal(const DrDocument *a, const DrDocument *b, size_t idx);
DrDocument *dr_doc_set_mask_properties(const DrDocument *doc, size_t idx,
    bool linked, float density, float feather);
DrDocument *dr_doc_transform_mask(const DrDocument *doc, size_t idx,
    const double *matrix6, int32_t filter);
/* Byte set-mask: replace raster entry idx's mask with the w*h coverage
 * plane (row 0 at top) placed at canvas (x, y), enabled, with the given linked
 * flag and outside_coverage (0 or 255); density 1, feather 0. Unlike
 * dr_doc_adding_layer_mask the rectangle is the caller's, never cropped to the
 * layer. NULL for NULL/empty/oversized coverage, a GROUP or out-of-range idx,
 * any other outside_coverage, a mask-budget refusal, Lock All or an identical
 * mask. The byte twin of the typed set_mask edit. */
DrDocument *dr_doc_set_mask(const DrDocument *doc, size_t idx,
    const uint8_t *coverage, uint32_t w, uint32_t h, int32_t x, int32_t y,
    bool linked, uint8_t outside_coverage);
/* Whole-canvas mask Invert: inverts entry idx's mask coverage inside
 * the stored rectangle AND outside it. selection: NULL, or canvas w*h coverage
 * bytes (selection_len must equal w*h). NULL inverts every stored sample and
 * flips outside_coverage 0<->255, keeping rect, offset, link, density, feather
 * and enabled. With a selection a raster entry's stored plane first grows to
 * the union of its rect and the canvas, filled with outside_coverage, then
 * each sample v becomes round((v*(255-c) + (255-v)*c)/255) at coverage c (0
 * off the canvas); outside_coverage is kept and a GROUP's mask does not grow.
 * NULL for NULL doc, a bad idx, no mask, a wrong selection_len, Lock All, a
 * mask-budget refusal or a selection that changes nothing. The typed twin is
 * the scalar_edit Invert filter on target mask. */
DrDocument *dr_doc_invert_mask(const DrDocument *doc, size_t idx,
    const uint8_t *selection, size_t selection_len);
/* Replace rendered pixels/offset while preserving the exact independent mask
 * storage and properties in canvas space, irrespective of linking. Used after
 * described-layer geometry has already transformed its mask once. */
DrDocument *dr_doc_set_layer_content_preserving_mask(const DrDocument *doc, size_t idx,
    const uint8_t *rgba, uint32_t width, uint32_t height, int32_t offset_x, int32_t offset_y);

/* Selection tools write exactly width*height coverage bytes, row 0 at top.
 * Input handles and caller buffers stay alive through the call. NULL required
 * inputs, invalid parameters, cancellation or resource refusal return false and
 * leave out_mask untouched. The optional single-use control owns cancellation,
 * progress and conservative scratch admission; error strings use dr_string_free.
 * source must match doc canvas dimensions. Color/Quick params use the document's
 * profile context; see selection_tools module documentation for JSON schema. */
bool dr_selection_color_range(const DrDocument *doc, const DrImage *source,
    const char *params, uint8_t *out_mask, const DrJobControl *control, char **err_out);
/* Quick Selection. Strict JSON {seeds:[[x,y],...] 1..4096 integer canvas
 * points, tolerance 0..200 (25), edge 0..200 (12), radius 0..256 (0), smooth
 * integer 0..4 (0)}. Every opaque pixel whose centre is within radius of a
 * seed centre is selected and anchors its own four-connected growth: a
 * neighbour joins when its Lab step is at most edge AND its distance from the
 * anchor is at most tolerance (dE76); transparent pixels are barriers. smooth
 * first averages each Lab sample over its (2*smooth+1)^2 window, counting only
 * samples within edge of it, so a hard edge is not blurred across. With radius
 * above 0, an unselected opaque region of at most ceil(pi*radius^2) pixels
 * that touches no canvas border and whose 4-neighbours are all selected is
 * filled (a region with a transparent neighbour is not). The seeds' summed
 * clipped (2*floor(radius)+1)^2 boxes must not pass 64,000,000 pixels, else an
 * error names the brush size and the seed count. radius 0 with smooth 0 grows
 * from the seed pixels alone. Source alpha is the selected coverage. */
bool dr_selection_quick(const DrDocument *doc, const DrImage *source,
    const char *params, uint8_t *out_mask, const DrJobControl *control, char **err_out);
/* Magnetic Lasso live-wire tracer: the cheapest edge-following
 * 8-connected pixel path from `from` inside the corridor of pixels whose
 * centre lies within `width` of the polyline from -> trail... -> (explicit
 * end). Strict JSON (unknown keys refuse): from:[x,y], trail:[[x,y],...]
 * 1..4096 points, end:"snap"|[x,y], width 1..256 (default 10), contrast
 * 1..100 (10), frequency 0..100 (57); coordinates finite within [0,w]x[0,h].
 * "snap" ends on the strongest edge pixel at least contrast/100 strong within
 * width of the trail's last point, else on that point. On success writes
 * *out_count vertices to out_points as x,y float pairs at pixel centres (the
 * first is from's pixel), *out_fasten = the vertex index the host fastens an
 * anchor at, or -1, and *out_corridor = the corridor's pixel count. A trail
 * that goes more than 2*width from `from`, comes back within 2*width of it
 * and encloses an area (from -> trail... -> from) of at least width times its
 * farthest distance is a loop (a path backed up along itself is not), at any
 * frequency (0 included): it is traced as two legs joined
 * on the edge near the trail point farthest from `from`, the junction is a
 * fasten candidate (the earlier of it and frequency's own fasten wins; both
 * need a tail of min(width, 32) px), and *out_corridor sums both legs'
 * corridors; the 8 * DR_EDGE_TRACE_WORK_BUDGET refusal cap applies to each
 * leg. Otherwise frequency 0 never fastens. capacity counts vertices (2*capacity floats); out_points may be
 * NULL only with capacity 0. A short capacity returns false with *out_count =
 * the count needed and an error naming both counts. Every other failure (bad
 * params, a corridor above 8 * DR_EDGE_TRACE_WORK_BUDGET pixels) leaves the
 * outputs untouched. NULL source/params/out_count/out_fasten/out_corridor
 * return false with err_out untouched. Pure: the source is only read.
 * DR_EDGE_TRACE_WORK_BUDGET is the corridor the host fastens against per
 * pointer move, so one hover trace stays inside a frame. */
#define DR_EDGE_TRACE_WORK_BUDGET 131072  /* per-move corridor px the host fastens against */
bool dr_selection_trace_edge(const DrImage *source, const char *params,
    float *out_points, size_t capacity, size_t *out_count, int32_t *out_fasten,
    uint64_t *out_corridor, char **err_out);
/* Select and Mask's coverage refinement of a source-sized mask. Strict JSON
 * {smooth 0..64, feather 0..64, shift -64..64, contrast 0..1, edge_radius
 * 0..64, smart_radius true|false}; every field optional and zero by default.
 * Order: edge, smooth, shift, feather, contrast; all zero copies the mask.
 * The edge stage re-estimates coverage from the source's colours (a guided
 * filter per RGB channel, weighted by how well each explains the coverage)
 * within edge_radius of the mask's 50% contour; smart_radius fades the
 * estimate where the picture has no edge, so a hard edge across flat colour
 * stays hard. `brush` is the AREA plane, one byte per pixel: 0 never refined,
 * 128 automatic (the radius band), 255 always refined. NULL = all automatic;
 * any other value is an error. The edge stage runs when edge_radius > 0 or the
 * plane holds a 255, and then reserves 64 bytes per pixel + 256 KiB against the
 * control (else 9N + 20*max(w,h) with a shift, 5N without). edge_radius > 0
 * with a NULL plane is valid. The source is only read: colour decontamination
 * is dr_image_estimate_foreground. */
bool dr_selection_refine(const DrImage *source, const uint8_t *mask,
    const uint8_t *brush, const char *params, uint8_t *out_mask,
    const DrJobControl *control, char **err_out);
/* Resamples width x height coverage to out_width x out_height through the
 * document resize's own plane kernel (dr_doc_resize), filter 0 nearest /
 * 1 bilinear / 2 bicubic / 3 Lanczos. Writes out_width*out_height bytes to
 * out_mask. Returns false and leaves out_mask untouched on a NULL buffer
 * (err_out untouched), or with a message through err_out (dr_string_free) on a
 * zero dimension, either product above 100,000,000 or an unknown filter.
 * Pure: the source mask is only read. */
bool dr_selection_resize(const uint8_t *mask, uint32_t width, uint32_t height,
    uint32_t out_width, uint32_t out_height, int32_t filter, uint8_t *out_mask,
    char **err_out);
/* Canvas-clipped selection-only affine. Matrix/filter conventions match masks. */
bool dr_selection_transform(const uint8_t *mask, uint32_t width, uint32_t height,
    const double *matrix6, int32_t filter, uint8_t *out_mask,
    const DrJobControl *control, char **err_out);

/* Foreground colour estimate behind Select and Mask's Decontaminate Colors.
   Opaque source matches document canvas/profile. alpha is width*height bytes,
   borrowed and unchanged. Required params color_space:"encoded"|"linear";
   optional strength0..1 (default1), strict JSON <=128KiB. Estimates foreground
   RGB; does NOT apply alpha. Source alpha and RGB at matte0/255 remain exact.
   Caller applies the matte separately. Nonopaque sources, unsupported profiles
   and constant fractional alpha refuse; so does "linear" when the document's
   own matrix/TRC profile fails the precision check (the own-profile sentence;
   "encoded" never does). Scratch <=128N+1MiB, including output,
   excluding borrowed inputs. Optional single-use control supports cancellation.
   Owned result freed by dr_image_free. NULL without error means byte no-op;
   error/cancel returns NULL with optional owned err_out (dr_string_free).
   NULL required pointers safely return NULL. All inputs remain unchanged. */
DrImage *dr_image_estimate_foreground(const DrDocument *document,
                                     const DrImage *source, const uint8_t *alpha,
                                     const char *params, const DrJobControl *control,
                                     char **err_out);

/* Select and Mask's New Layer outputs at 8 bits. The canvas-sized foreground
 * must hold RGB in the document profile (caller precondition) and keeps its
 * own alpha on the new layer; alpha is a separate canvas-sized byte plane. Adds top-level
 * "Refined Cutout" with linked/enabled/density1/feather0 mask and hides previous
 * top-level entries. Original pixel/mask/description/structure data are retained.
 * alpha * pixel alpha zero everywhere: NULL without error. Error/cancel: NULL with optional owned error
 * freed by dr_string_free. NULL required pointers safely return NULL. Inputs
 * unchanged; optional control single-use. Output freed with dr_doc_free.
 * Scratch query includes new RGBA/mask and cloned records/strings, excludes
 * borrowed inputs and display rendering, returns 0 for NULL. */
uint64_t dr_doc_refinement_cutout_scratch_bytes(const DrDocument *document);
/* The byte cutout's projection: one plain visible canvas-sized foreground
 * root (its own alpha composited as stored), Normal/opacity1 and a default
 * enabled canvas mask; all prior roots hidden. Equals the flattened document.
 * Same hidden adjustment/profile validation and byte compositor/quantizer as
 * ordinary projection. Other shapes refuse; there is no silent fallback.
 * Plan includes final4N bytes, <=4096-pixel chunks and conservative maximum
 * single-metadata parse workspace for unmodelled profiles; it parses no JSON.
 * Retained sources/records and later host CGImage are separate. NULL plan is0.
 * Each <=16MiB metadata JSON parse remains internally noncancellable, with
 * checks between records; all parser workspace is reserved before parsing.
 * Render polls a fresh optional job and returns NULL/error on cancellation;
 * host sets scratch limit before render. Free image/error with matching frees. */
uint64_t dr_doc_refinement_cutout_render_scratch_bytes(const DrDocument *document, char **err_out);
DrImage *dr_doc_render_refinement_cutout(const DrDocument *document,
                                       const DrJobControl *control, char **err_out);
DrDocument *dr_doc_add_refinement_cutout(const DrDocument *document,
    const DrImage *foreground, const uint8_t *alpha,
    const DrJobControl *control, char **err_out);

/* Categorical trimap construction; does not run or expose a matting candidate.
 * Existing input contains only0 background,128 unknown,255 foreground.
 * JSON {version:1,width,height,strokes:[{label,radius,points:[[x,y],...]}]}.
 * Labels "background"/"unknown"/"foreground" overwrite earlier labels where
 * pixel centers are <=radius from the round-cap polyline; no AA/blending.
 * Untouched labels are preserved; Unknown relaxes an earlier known constraint.
 * Caps:4MiB JSON,256strokes,8192points/stroke,65536points total,radius0.5..512,
 * finite coordinates±100000,and64million summed clipped segment-box visits.
 * Canvas size must fit MAX_PIXELS. Core scratch=N+8*JSONbytes+16MiB, excluding
 * borrowed input and caller output. Optional single-use control is cooperative.
 * Failure/cancel leaves output untouched. Input/output may alias. Identity is
 * successful with identical bytes. Error strings are freed by dr_string_free. */
bool rz_trimap_strokes(const uint8_t *trimap, uint32_t width, uint32_t height,
    const char *params, uint8_t *out_trimap, const DrJobControl *control, char **err_out);

/* Phase 9I assisted annotation replays; internal experiment, no public control.
 * The replay JSON is the app-written per-case session document; the core is its
 * only reader and validates all of it, including an in-progress document:
 * schema1, protocol "phase9i-assisted-edges-v1", the exact declared budget
 *(300s,20 modifying gestures, diameters3/9/27/81,256 points), sequential event
 * indices, non-decreasing elapsed_ms, 1-based gesture numbering, stroke tool/
 * diameter/points inside the canvas, accepted/rejected:time_budget/
 * rejected:rasterizer_limit results, undo/redo stack legality, a single final
 * terminal event and the matching outcome, and 64-hex syntax for every hash.
 * The core never hashes, decodes a PNG or scores anything.
 * dr_assisted_replay_plane writes width*height bytes of0 background,128 unknown,
 *255 foreground: an all-128 base with the reconstructed active strokes applied
 * in order through the same rz_trimap_strokes rasterizer (radius=diameter/2,
 * points at pixel centers), so its4MiB/visit caps apply unchanged. Dimensions
 * must equal the replay's own. Failure/refusal/cancel leaves out_plane
 * untouched, and a rasterizer refusal reaches err_out with a
 * "rasterizer_limit: " prefix. Optional single-use control is cooperative.
 * dr_assisted_replay_summary returns JSON {final,outcome,modifying_gestures,
 * active_strokes,human_origin,scoreable,elapsed_ms}; NULL plus err_out on an
 * invalid document. Returned and error strings are freed by dr_string_free. */
bool dr_assisted_replay_plane(const char *replay, uint32_t width, uint32_t height,
    uint8_t *out_plane, const DrJobControl *control, char **err_out);
char *dr_assisted_replay_summary(const char *replay, char **err_out);

/* Internal Phase 7B/7C preserving workflow. No public document mode yet.
 * These opaque types are NEVER interchangeable with DrDocument or DrImage.
 * Storage is straight RGBA16 UNORM; evaluation encoded RGB/f32 with isolated
 * Normal group intermediates. Unsupported depths/operations refuse. Limits
 * include other_retained_bytes supplied by the host; positive limit <=2GiB.
 * Caller owns returned handles and keeps inputs/job controls alive until all
 * concurrent calls finish. Errors use owned strings freed by dr_string_free.
 * Native v11/v12 is refused by the byte reader. PNG/TIFF import never falls back
 * to byte/platform decoding or automatic working-space profile conversion.
 * Codecs have cancellation at stage boundaries; render/preview poll in chunks.
 */
typedef struct PrecisionDocument DrPrecisionDocument;
/* A warp "set" request may add "indexed_background" (#1 Indexed phase 3b,
 * the Indexed section's shape): the colour a clear pixel's entry is nearest
 * on an Indexed Color document with no transparent index; ignored on every
 * other document. */
char *dr_precision_doc_warp_info(const DrPrecisionDocument *document, size_t layer, char **err_out);
char *dr_precision_doc_warp_edit_plan(const DrPrecisionDocument *document, const char *request, char **err_out);
DrPrecisionDocument *dr_precision_doc_warp_edit(const DrPrecisionDocument *document, const char *request, uint64_t limit, uint64_t other_retained_bytes, const DrJobControl *control, char **err_out);
typedef struct PrecisionImage DrPrecisionImage;

/* LAYER COMPS at high depth — dr_doc_layer_comps_json's twin, same report
 * schema (see the DR_LAYER_COMPS_MAX block above, which owns every rule).
 * The record indices it reports are the flattened postorder ones
 * dr_precision_doc_info uses. "locked" answers the same POSITION-lock
 * question it does at byte depth (12E-1b). The typed MUTATION is
 * dr_precision_doc_edit with
 * {"op":"layer_comp","action":"save|update|delete|apply|reorder", ...}.
 * Free with dr_string_free. */
char *dr_precision_doc_layer_comps_json(const DrPrecisionDocument *doc,
                                        char **err_out);

/* dr_doc_flat_save_losses's typed twin, its editable-state half: a heap JSON
 * array of the state a flattened file of a typed document (Grayscale, CMYK,
 * Indexed Color, Bitmap) has no place for, in the byte list's words and
 * order (layers, masks, descriptions, styles, layer properties including
 * the colour label, bounds, saved selection channels, guides and ruler
 * origin, color samplers, paths, layer comps, global light), then "pixel
 * transparency" when alpha_kept is false and a pixel is not opaque, then
 * "hidden pixel colors" when a pixel at alpha 0 holds a colour or ink. The
 * one layer may be named "Background" ("Index" in Indexed Color, "Bitmap"
 * in Bitmap) and Indexed and Bitmap documents get no pixel lines. No
 * profile, density, metadata, depth or spot-channel line: what each mode's
 * formats carry of those is the host's. [] when the flat file reopens as
 * this document. NULL on a NULL document or failure, with err_out. Free with
 * dr_string_free. */
char *dr_precision_doc_flat_state_losses(const DrPrecisionDocument *doc, bool alpha_kept,
                                         char **err_out);

/* The Background mark at high depth — dr_doc_background_role's twin, for root
 * entry 0: 0 no mark applies, 1 Background, 2 ordinary layer; 0 on NULL. */
int32_t dr_precision_doc_background_role(const DrPrecisionDocument *doc);

/* GUIDES and the RULER ORIGIN at high depth (12E-1c) — the dr_doc_guide_* /
 * dr_doc_ruler_origin / dr_doc_set_ruler_origin family's twins, each with the
 * byte export's contract, sentinels and refusals VERBATIM (see "Guides,
 * rulers and snapping", which owns every rule). Positions are the same
 * absolute grid-line canvas coordinates, the cap is the same
 * dr_max_guides(), an id is the same session-scoped handle, and a mutator
 * answers NULL for exactly the cases the byte twin answers NULL for — so a
 * host picks a family by depth and phrases nothing twice. Persisted by the
 * typed LGDE table at .rz version 48. */
size_t dr_precision_doc_guide_count(const DrPrecisionDocument *doc);
uint64_t dr_precision_doc_guide_id(const DrPrecisionDocument *doc, size_t i);
int dr_precision_doc_guide_orientation(const DrPrecisionDocument *doc, size_t i);
double dr_precision_doc_guide_position(const DrPrecisionDocument *doc, size_t i);
bool dr_precision_doc_ruler_origin(const DrPrecisionDocument *doc, double *out_xy);
DrPrecisionDocument *dr_precision_doc_add_guide(const DrPrecisionDocument *doc,
                                                int orientation, double position);
DrPrecisionDocument *dr_precision_doc_move_guide(const DrPrecisionDocument *doc,
                                                 size_t i, double position);
DrPrecisionDocument *dr_precision_doc_remove_guide(const DrPrecisionDocument *doc,
                                                   size_t i);
DrPrecisionDocument *dr_precision_doc_clear_guides(const DrPrecisionDocument *doc);
DrPrecisionDocument *dr_precision_doc_set_ruler_origin(const DrPrecisionDocument *doc,
                                                       double x, double y);

/* COLOUR SAMPLERS at high depth — the dr_doc_color_sampler* family's
 * twins, each with the byte export's contract, sentinels and refusals
 * VERBATIM (see "Colour samplers", which owns every rule). Persisted by the
 * typed CSMP table at .rz version 60. */
size_t dr_precision_doc_color_sampler_count(const DrPrecisionDocument *doc);
bool dr_precision_doc_color_sampler(const DrPrecisionDocument *doc, size_t i,
                                    uint32_t *out_xys);
DrPrecisionDocument *dr_precision_doc_with_color_samplers(const DrPrecisionDocument *doc,
                                                          const uint32_t *xys,
                                                          size_t count);

/* PATHS at high depth — the dr_doc_paths_json pair's twins, each with
 * the byte export's contract, JSON, refusals and sentences VERBATIM (see
 * "Paths", which owns every rule). Persisted by the typed PATH table at .rz
 * version 75. */
char *dr_precision_doc_paths_json(const DrPrecisionDocument *doc);
DrPrecisionDocument *dr_precision_doc_with_paths_json(const DrPrecisionDocument *doc,
                                                      const char *json, char **err_out);

/* VECTOR MASKS at high depth — the dr_doc_*vector_mask* exports'
 * twins over u16 mask planes, each with the byte export's rules ("VECTOR
 * MASKS" under "Paths" owns them). `layer` is the record index. The three
 * that change the document are admitted against the caller's working `limit`
 * and `other` retained bytes; a bake charges the coverage rasterizer's
 * scratch, one plane and the raster part's feather. Where the byte exports
 * answer NULL without a message for an out-of-range index or a lock, these
 * set err_out (the typed lock sentence names the lock); NULL with err_out
 * untouched still means NULL doc, no vector mask, or nothing to change. A
 * 32-bit float document takes the same calls: its masks are u16 planes. */
char *dr_precision_doc_vector_mask_json(const DrPrecisionDocument *doc, size_t layer);
DrPrecisionDocument *dr_precision_doc_set_vector_mask(const DrPrecisionDocument *doc,
                                                      size_t layer, const char *shape_json,
                                                      uint64_t limit, uint64_t other,
                                                      const DrJobControl *control,
                                                      char **err_out);
DrPrecisionDocument *dr_precision_doc_remove_vector_mask(const DrPrecisionDocument *doc,
                                                         size_t layer, uint64_t limit,
                                                         uint64_t other, char **err_out);
DrPrecisionDocument *dr_precision_doc_set_vector_mask_properties(
    const DrPrecisionDocument *doc, size_t layer, bool enabled, bool linked,
    uint64_t limit, uint64_t other, const DrJobControl *control, char **err_out);

/* Same modes/fingerprint contract as dr_doc_open_psd; only for files whose
 * report says "import_precision":"rgba16" (8-bit files refuse) or "cmyk"
 * (a PSD, PSB or layered TIFF at 8 or 16 bits: mode 0 imports its layers,
 * mode 1 opens the saved composite, both as a CMYK document, #1) or
 * "rgba32" (a 32-bit RGB PSD or PSB, both modes as a 32-bit float document;
 * a 32-bit float layered TIFF of either byte order, its layers).
 * A "gray" file refuses by name (dr_precision_doc_open_gray_psd opens
 * it), and so does a "lab" file (dr_precision_doc_open_lab_psd opens it).
 * Optional single-use control: begun once for the whole open, polled between layer
 * planes. Free with dr_precision_doc_free. */
DrPrecisionDocument *dr_precision_doc_open_psd(const char *path, int mode,
    const char *expected_fingerprint, uint64_t limit, uint64_t other_retained_bytes,
    const DrJobControl *control, char **err_out);
/* Opens a Grayscale (colour mode 1) PSD, PSB or layered TIFF at 8 or 16 bits,
 * or a PSD or PSB at 32 (#1 phase 3c), as a Gray document of that depth:
 * mode 0 its layers, 1 its saved composite (a TIFF has none). working_gray
 * is the working Gray's wire name, assigned to an untagged file under an
 * import warning. Same fingerprint contract as
 * dr_doc_open_psd; optional single-use control begun once (#1 phase 3a).
 * Free with dr_precision_doc_free. */
DrPrecisionDocument *dr_precision_doc_open_gray_psd(const char *path, int mode,
    const char *expected_fingerprint, const char *working_gray, uint64_t limit,
    uint64_t other_retained_bytes, const DrJobControl *control, char **err_out);
/* Opens a Lab (colour mode 9) PSD, PSB or layered TIFF at 8 or 16 bits as a
 * Lab Color document of that depth (#1 Lab phase 3a): mode 0 its layers,
 * 1 its saved composite (a TIFF has none). The file codes are Photoshop's,
 * measured (8 bits L* 100v/255, a* v - 128; 16 bits L* 100v/65535, a* on
 * Photoshop's 15-bit grid: i = round(v * 32768/65535), a* = (i - 16384)/128).
 * Same fingerprint contract as dr_doc_open_psd; optional
 * single-use control begun once. Free with dr_precision_doc_free. */
DrPrecisionDocument *dr_precision_doc_open_lab_psd(const char *path, int mode,
    const char *expected_fingerprint, uint64_t limit, uint64_t other_retained_bytes,
    const DrJobControl *control, char **err_out);
/* Same JSON as dr_doc_psd_export_report plus "depth": 16, or 32 for a float
 * document, which writes a PSD or PSB or a big-endian layered TIFF
 * at 32 bits. A CMYK
 * document answers "depth":"cmyk" with "bit_depth" (its declared 8 or 16)
 * and "working_memory_plan_bytes", for every container and both modes (#1).
 * A Grayscale document answers its declared "depth" (8, 16 or 32) with
 * "color_mode":"gray" (#1 phase 3b); at 32 it is the float report: a
 * layered TIFF refused (#1 phase 3c). A Lab Color document answers its
 * declared "depth" (8 or 16) with "color_mode":"lab" (#1 Lab phase 3b).
 * An Indexed Color document answers "depth":8,"bit_depth":8 with
 * "color_mode":"indexed", "layer_count" 0 (the file holds no layer record,
 * as Photoshop's own) and "working_memory_plan_bytes" (#1). */
char *dr_precision_doc_psd_export_report(const DrPrecisionDocument *doc,
    int container, int mode, char **err_out);
/* Writes a 16-bit RGB PSD, PSB or layered TIFF. `projection` is REQUIRED for
 * an RGB document: canvas-sized RGBA16 UNORM in the document's exact ICC
 * bytes (the host pairs it with this revision, as save_projection16). Admits
 * worst_case_bytes + projection + bake_scratch_bytes through the typed limit;
 * each adjustment stamp is a render admitted on its own. A CMYK document
 * takes NULL: a colour-mode-4 file at its declared depth (layers: ink, masks,
 * groups, modes, Fill and its Levels/Curves/Hue/Saturation records; its
 * composite rendered here), admitted against the report's
 * working_memory_plan_bytes (#1). Atomic, noncancellable. A float
 * document writes a 32-bit RGB PSD or PSB or big-endian layered TIFF
 * from its canvas-sized RGBA32 float projection. A Grayscale document
 * writes a colour-mode-1 PSD, PSB or layered TIFF at its declared 8 or 16
 * bits from its REQUIRED canvas-sized RGBA16 projection (#1 phase 3b); a
 * 32-bit one writes a 32-bit colour-mode-1 PSD or PSB from its REQUIRED
 * RGBA32 float projection, and its layered TIFF is refused (#1 phase 3c).
 * A Lab Color document writes a colour-mode-9 PSD, PSB or layered TIFF at
 * its declared 8 or 16 bits, no profile embedded, from its REQUIRED
 * canvas-sized RGBA16 projection in its Lab twin (#1 Lab phase 3b).
 * An RGB, CMYK, Grayscale or Duotone PSD or PSB at 8 or 16 bits carries the
 * document's spot channels in Photoshop's layout (#1 spot channels 1b-1);
 * a layered TIFF from RGB or CMYK at 8 or 16 bits carries them as trailing
 * extra samples (1c-2b). A Lab or 32-bit PSD or PSB refuses them, and a
 * Gray, Duotone, Lab or 32-bit layered TIFF refuses them with the TIFF
 * sentence (spot_channel::TIFF_REFUSAL). */
bool dr_precision_doc_save_psd(const DrPrecisionDocument *doc,
    const DrPrecisionImage *projection, const char *path, int container, int mode,
    uint64_t limit, uint64_t other_retained_bytes, char **err_out);
/* Select and Mask's New Layer outputs at 16 and 32 bits (native refinement
 * assembly). Foreground is canvas-sized RGBA16, or RGBA32 float in a 32-bit
 * float document, in the exact document profile. Every source
 * RGBA component, including tiny alpha, remains unchanged. The borrowed matte
 * is canvas-sized native-endian u16 and becomes a separate editable mask.
 * Assembly never premultiplies or quantizes source-alpha * refinement coverage.
 * Previous roots hide with original pixels/masks/structure retained. Empty
 * effective coverage is NULL/no error; failures/cancellation preserve inputs.
 * Plan JSON includes retained_bytes, scratch_bytes, borrowed_matte_bytes and
 * foreground_storage. Other retained state excludes the document/foreground
 * already in that plan;
 * caller raw matte bytes are included separately by core admission.
 * Control is optional/single-use. Free JSON/errors with dr_string_free and
 * returned document owners with dr_precision_doc_free. */
char *dr_precision_doc_refinement_cutout_plan(const DrPrecisionDocument *document,
    const DrPrecisionImage *foreground, char **err_out);
DrPrecisionDocument *dr_precision_doc_add_refinement_cutout(const DrPrecisionDocument *document,
    const DrPrecisionImage *foreground, const uint16_t *matte, uint64_t working_limit,
    uint64_t other_retained, const DrJobControl *control, char **err_out);
/* Edit > Auto-Blend Layers, Stack Images (focus stacking) and
 * Panorama, at 8 bits and, through the dr_precision_doc_ pair, at 16 bits
 * and 32-bit float. `request` is strict JSON of at most 64 KiB:
 * {"layers":[index,...],"seamless":bool,"method":"stack_images"|"panorama",
 * "name":string}; "method" is optional and defaults to "stack_images" (so a
 * request without it means what it always meant), "name" (Stack Images'
 * merged layer's) is optional and defaults to "Auto-Blend". Any other
 * "method" is "invalid auto-blend request". A repeated index counts once.
 *
 * Each requested layer takes a canvas-sized layer mask at (0, 0): binary (0
 * or full), linked, enabled, density 1, feather 0, outside coverage 0. The
 * masks partition the area the frames cover, and a frame never takes a pixel
 * where its alpha is 0. Visibility, opacity, blend mode and style of the
 * frames are not consulted.
 *
 * STACK IMAGES: a mask reveals the pixels where its layer is the sharpest
 * frame. With "seamless" one merged raster layer is added: a Laplacian-pyramid
 * blend of the frames through those masks, canvas-sized at (0, 0), Normal,
 * opacity 1, no mask. SLOT RULE: it lands directly above the clip run of the
 * top requested layer (above the last layer clipped to it) and is unclipped,
 * so it joins no clip run and the layers clipped to that frame stay clipped
 * to it. Its alpha is the winning frame's; a pixel no frame covers is
 * transparent. A float document scores and blends in the retouch encoding
 * and keeps values above white.
 *
 * PANORAMA: the frames are taken bottom first; each overlap with the frames
 * below gets one minimum-cut seam (through where the frames agree and
 * neither has an edge, midway on flat ground), cut on a grid of at most 4 Mi
 * blocks. No layer is added. With "seamless" (Seamless Tones and Colors)
 * each frame's RGB is first scaled by one gain per channel (solved over the
 * overlaps), the seams are cut on the gained frames, and a multi-band blend
 * across the seams is written back INTO the frames: each keeps its size,
 * offset, mask and alpha bit for bit, its RGB is the blend where its mask
 * reveals it and its gained RGB elsewhere. A float document runs in the
 * retouch encoding over the code of white. Without "seamless" no pixel
 * changes.
 *
 * The plan is owned JSON {"retained_bytes","scratch_bytes","work_units",
 * "merged_index"}: scratch_bytes is the kernel's working set plus the new
 * mask planes plus the merged layer or Panorama's written-back frames,
 * merged_index the index the merged layer would take (null without
 * "seamless" and for Panorama). The plan answers every refusal the edit
 * would. The edit admits retained_bytes + scratch_bytes +
 * other_retained_bytes against `limit` before any pixel work.
 *
 * `report_out` (may be NULL) receives, on success only, owned JSON
 * {"method","layers":[{"layer","won_pixels"}],"covered_pixels",
 * "canvas_pixels","merged_layer"} (merged_layer null without "seamless" and
 * for Panorama); with Panorama and "seamless" each layers row also carries
 * "gain":[r,g,b], the frame's gain.
 *
 * A NULL result ALWAYS carries an error, a NULL document or request
 * included. Refusals name the layer: fewer than two distinct layers, more
 * than 255, an index out of range, a group, an adjustment layer, a layer
 * that already has a layer mask, a layer whose locks block a mask edit or,
 * for Panorama with "seamless", a pixel edit ("is locked"), and a selection
 * with no pixel inside the canvas. CAPACITY refusals are decided
 * before admission and before any pixel work, each a sentence containing
 * "native capacity" that says which part does not fit: the layer masks (the
 * document's mask budget) or the merged layer (the layer count or record
 * cap, the layer-pixel budget, the name). A cancel answers "operation
 * cancelled"; `control` is optional and single-use. The input never changes;
 * one result is one undo step. An Indexed Color document refuses both
 * precision calls for good ("auto_blend_layers needs more than one layer,
 * and an Indexed Color document holds one.", #1 Indexed
 * phase 3b). Free the JSON and errors with dr_string_free
 * and the documents with dr_doc_free / dr_precision_doc_free. */
char *dr_doc_auto_blend_plan(const DrDocument *document, const char *request, char **err_out);
DrDocument *dr_doc_auto_blend(const DrDocument *document, const char *request, uint64_t limit,
    uint64_t other_retained_bytes, const DrJobControl *control, char **report_out,
    char **err_out);
char *dr_precision_doc_auto_blend_plan(const DrPrecisionDocument *document, const char *request,
    char **err_out);
DrPrecisionDocument *dr_precision_doc_auto_blend(const DrPrecisionDocument *document,
    const char *request, uint64_t limit, uint64_t other_retained_bytes,
    const DrJobControl *control, char **report_out, char **err_out);
/* dr_image_estimate_foreground's 16-bit twin (Select and Mask's Decontaminate
 * Colors). source is the document's opaque canvas-sized RGBA16
 * composite in its exact ICC profile; matte is matte_len u16 samples, which
 * must be the canvas pixel count. Strict params {"color_space":"encoded",
 * "strength":0..1 (default 1)}; "linear" refuses by name at this depth. The
 * result keeps source alpha and the RGB at matte 0 and 65535 exactly and
 * shares the tiles it leaves unchanged; the caller applies the matte
 * separately. NULL without error: strength 0, a matte with no fractional
 * sample, or no code changing. Refuses a source of another size, storage (a
 * 32-bit float composite) or profile, a pixel that is not opaque, a constant
 * fractional matte and a failed admission. The plan is JSON {retained_bytes,
 * scratch_bytes (8 MiB + 136 bytes per pixel), input_samples (the canvas pixel
 * count)}; free it and errors with dr_string_free, the image with
 * dr_precision_image_free. control is optional and single-use. */
char *dr_precision_doc_estimate_foreground_plan(const DrPrecisionDocument *document,
    const DrPrecisionImage *source, char **err_out);
DrPrecisionImage *dr_precision_doc_estimate_foreground(const DrPrecisionDocument *document,
    const DrPrecisionImage *source, const uint16_t *matte, size_t matte_len,
    const char *params, uint64_t limit, uint64_t other,
    const DrJobControl *control, char **err_out);
/* Native selected destructive tone/filters. Strict request <=20 MiB:
 * {layer,op:"adjustment",description:precision_adjustment_JSON_string}, or
 * {layer,op:filter_name,params:{...}}. Auto ops optionally analysis_scope:
 * "whole_target" (default) or "selection", independent of write coverage.
 * Spatial/guide reads always use complete original raster storage. Selection
 * is empty (unrestricted) or canvas-sized u16; exact0 copies original codes.
 * Plan includes maximum input_samples and admitted scratch before host allocates
 * coverage, and output_bytes (count*8): the rewritten layer's u16 codes the
 * result keeps beyond its source. NULL edit result/no error is exact
 * no-op. Strings use dr_string_free.
 * Native legacy blur sigma <=64 (from 7 up the extended-box Gaussian Blur kernel, at
 * 16 bits and on a float layer's linear values alike); photo radii <=64. Legacy sharpen amount remains
 * sigma capped5, 2*source-blurred including alpha. Levels_channels params are
 * three-element black/white/gamma arrays; auto resolution returns those arrays
 * and identity. Error/cancel leaves doc and coverage unchanged.
 * A 32-bit float layer takes an adjustment request (the layer's own float
 * form, lerped by coverage with no clamp or rounding; an op the float mode does
 * not apply refuses by name) or one of eleven filters in linear
 * light with no colour clamp: blur, box_blur, motion_blur, radial_blur,
 * lens_blur, tilt_shift, offset, lens_correction, liquify, depth_blur,
 * unsharp_mask, high_pass (alpha at most 1, an output alpha below 2^-24 transparent
 * black, a result outside +/-65504 refused). output_bytes is count*16. Any other
 * op on a float layer refuses "selected edits require native u16 storage".
 * Work ceiling (also the scalar_edit filters of dr_precision_doc_edit):
 * the plan carries none, a run with a control has none, and a run with NULL
 * control keeps the destructive work ceiling.
 */
/* On a CMYK document (#1) op "adjustment" applies the description to the
 * layer's ink lerped by coverage, alpha kept (dr_precision_doc_apply_adjustment's
 * CMYK rules), and Auto refuses by name (the host runs CMYK Auto as Levels).
 * Op "liquify" (phase 4b) runs the RGB kernel over the ink's five planes.
 * Since the adjustments batch every filter enabled in CMYK runs on a
 * CMYK raster as two RGBA16 passes of the RGB path above, [L_c, L_m, L_y, a]
 * and [L_k; 3, a] with L = 1 - ink, mapped back (C/M/Y from the first, K from
 * the second, alpha the first's; a transparency lock keeps the source alpha;
 * an output at alpha 0 keeps the source ink). Add Noise salts the K pass's
 * seed; clouds/difference_clouds take "foreground_ink"/"background_ink" (four
 * fractions, default K 100 % over paper) and refuse the colour keys.
 * lens_correction, develop_filter, presence, add_grain, depth_blur and
 * levels_channels refuse by name. The plan has the RGB
 * plan's keys (output_bytes = layer pixels x 10). */
/* Grayscale (#1 phase 2b): on a Gray document the selected edit, its plan
 * and selected auto levels admit "adjustment" under the adjustment exports'
 * Gray rules, auto_tone, auto_contrast, equalize, and levels_channels whose
 * black, white and gamma arrays each hold equal values (else the Levels
 * channel sentence); auto_color refuses "Auto Color is unavailable in a
 * Grayscale document.". Since phase 2c every filter runs
 * through the RGB path above and each filtered sample is folded to one channel
 * before the coverage mix: the value when R = G = B bit for bit, else the
 * Rec. 709 luma; Add Noise takes channel 0's draw. Clouds' foreground_color
 * and background_color are greyed by Image > Mode > Grayscale's rule;
 * "liquify" runs through its RGB kernel (phase 4). */
/* Lab Color (#1 phases 2b to 2f): on a Lab document the selected edit, its
 * plan and selected auto levels admit "adjustment" under the adjustment
 * exports' Lab rules (not as a layer); auto_tone and auto_contrast derive the
 * Lightness endpoints only (a* and b* keep 0, 1, 1), equalize and
 * levels_channels run; auto_color refuses "Auto Color is unavailable in a Lab
 * Color document.". Every other op is a filter, run on the
 * L*, a*, b* codes through its RGB kernel unchanged (each channel filtered
 * alone); Clouds' foreground_color and background_color become the 8-bit
 * #LLaabb of their Lab codes by Image > Mode > Lab Color's rule. lens_correction,
 * develop_filter and difference_clouds refuse "<Name> is unavailable in a Lab
 * Color document."; presence and add_grain "<Name> is unavailable in a
 * Lab Color document: it is a Develop Filter control, and the Develop
 * Filter is unavailable in Lab."; depth_blur "... Doctored offers it on
 * RGB documents only."; "liquify" runs the RGB selected edit on the codes
 * (phase 4a-ii). Since phase 2f lens_blur,
 * reduce_noise and smart_sharpen, and offset with undefined_areas fill, run
 * in three one-channel passes: each Lab channel is the G of the filter on the
 * gray [c, c, c, a] (offset's a* and b* passes fill the neutral 32896, a* =
 * b* = 0); emboss and edge_detect run one Lightness pass and mix a* and b*
 * toward the neutral by the coverage; reduce_noise's color has no effect on
 * Lab, and with its luminance half off the edit changes nothing. Their plan
 * has the same keys (output_bytes layer px x 8); get_precision's
 * lab.filter_plans names each one's scratch_bytes with no params on the top
 * Lab raster (null without one). On a Lab layer mask or alpha channel (scalar_edit,
 * phase 2d) and on the red, green, blue and alpha planes (Lightness, a, b and
 * alpha, phase 2e) a filter runs as on RGB, those five, offset's fill and
 * clouds included; the rows that refuse for good and liquify refuse alike. */
/* Equalize (#1): {layer,op:"equalize",params:{"scope":"selected"|"entire"}}
 * (scope default "selected"; ignored with empty coverage) on a raster layer
 * of an RGB 16-bit, 32-bit float or CMYK document. ONE table from the summed
 * histograms of the colour planes (R+G+B; on CMYK the four L = 1 - ink
 * plates), each sample weighted by coverage x alpha, lut[v] = round(max
 * (cdf(v) - cdf(v_min)) / (N - cdf(v_min))) with v_min the darkest occupied
 * level and N the total weight; 65536 levels at 16 bits and on the float
 * display encoding E, 256 on an 8-bit CMYK document (round(L/257)). At 32
 * bits a sample is counted at round(65535 E), decoded through the profile
 * from its table value, and keeps its bits when its level maps to itself; a
 * sample above white counts at the top level, one below zero at level 0, and
 * both are written unchanged, as is a non-finite one. The write is lerped by
 * coverage under "selected" and covers the whole layer under "entire" or with
 * no selection; alpha is never written and alpha-0 pixels are skipped. A
 * group or an adjustment layer refuses by name; fewer than two occupied
 * levels, or no changed sample, is NULL with no error. The plan has the
 * selected edit's keys. An 8-bit RGB document runs dr_image_equalize. On a
 * Lab Color document (8 or 16 bits, #1 phase 2c) the 16-bit arm runs on
 * Lightness alone: the histogram counts L* and the table writes L*, a* and
 * b* kept bit for bit. */
char *dr_precision_doc_selected_edit_plan(const DrPrecisionDocument *document,
    const char *request, uint64_t limit, uint64_t other_retained, char **err_out);
DrPrecisionDocument *dr_precision_doc_selected_edit(const DrPrecisionDocument *document,
    const char *request, const uint16_t *coverage, size_t length, uint64_t limit,
    uint64_t other_retained, const DrJobControl *control, char **err_out);
char *dr_precision_doc_selected_auto_levels(const DrPrecisionDocument *document,
    const char *request, const uint16_t *coverage, size_t length, uint64_t limit,
    uint64_t other_retained, const DrJobControl *control, char **err_out);

/* Native interpolation precedes explicit display quantization. */
DrPrecisionImage *dr_precision_image_transform_retouch_source(const DrPrecisionImage *image,
    const double *matrix, uint64_t working_limit, uint64_t other_retained,
    const DrJobControl *control, char **err_out);
/* Authoritative native canvas retouch plate; same sampling codes as byte API.
 * On a CMYK document (#1) the plate (Current: the layer's ink on paper at
 * alpha 0; Current & Below and All: the CMYK composite with the excluded
 * records at opacity 0) as its Display P3 RGBA16 VIEW, a display adapter for
 * the host's source overlay; the retouch itself samples the ink. */
DrPrecisionImage *dr_precision_doc_retouch_source(const DrPrecisionDocument *doc,
    size_t idx, uint32_t sample, bool ignore_adjustments, uint64_t working_limit,
    uint64_t other_retained, const DrJobControl *control, char **err_out);
/* dr_precision_doc_edit for one "retouch_clone" or "retouch_heal" request,
 * with the SAMPLING PLATE rendered from `plates` instead of from `document`
 * (Content-Aware Move heals the moved pixels onto the filled document
 * while reading them from the picture before the fill). Only the plate comes
 * from `plates`: the lock gate, the plan, the admission and the write are
 * `document`'s, so passing `document` as `plates` is dr_precision_doc_edit to
 * the byte. The plate's samples are read as they are; `plates` is taken to be
 * in `document`'s working space.
 * The request is the retouch request dr_precision_doc_edit takes (at most
 * 64 KiB) and `samples`/`length` are its planned input on `document`
 * (dr_precision_doc_edit_plan's input_samples; stroke, then selection when
 * selection:true). Pure. NULL with no error: nothing would change. NULL with
 * an error: a NULL handle, request or samples, a length that is not the
 * plan's, any refusal of dr_precision_doc_edit, or one of this call's own:
 * an op other than retouch_clone / retouch_heal; output other than "layer"
 * or expand_destination true; `plates` of another canvas size; `plates` of
 * another bit depth; `plates` with no raster layer at the sampled
 * index (source_layer, else layer). A CMYK `document` (#1) takes CMYK
 * `plates` with a CMYK raster at the sampled index ("retouch plates must have
 * the document's bit depth" for an RGB one), whose plate is ink. A Grayscale
 * `document` (#1 phase 2c) asks dr_precision_doc_edit's Gray gate (a mode
 * the Gray document does not offer refuses) and takes Grayscale `plates`
 * only ("A Grayscale document clones from a Grayscale document only: convert
 * the source to Grayscale first."). A Lab Color `document` (#1 phase 2e)
 * asks dr_precision_doc_edit's Lab gate and takes Lab Color `plates` only ("A
 * Lab Color document clones from a Lab Color document only: convert the
 * source to Lab Color first."). */
DrPrecisionDocument *dr_precision_doc_retouch_from(const DrPrecisionDocument *document,
    const DrPrecisionDocument *plates, const char *request, const uint16_t *samples,
    size_t length, uint64_t limit, uint64_t other_retained_bytes,
    const DrJobControl *control, char **err_out);


/* An RGB or CMYK TIFF's spot channels (#1 1c-2a: trailing extra samples
 * described by resource 1077 in tag 34377) open as spot channels at u16
 * (an 8-bit file's ×257), admitted against `limit` for the decoded samples
 * and the planes; the observation and any loss line join import_warnings.
 * A 16-bit RGB TIFF with extra samples beyond one straight alpha opens only
 * when every one of them is a kept spot channel; otherwise it still refuses
 * "RGBA TIFF must declare one unassociated alpha sample". A multipage 16-bit
 * or float TIFF opens its pages as dr_doc_open does, each later page
 * admitted against `limit` with the pages before it retained, and the
 * sentences about its pages join import_warnings; a CMYK page 1 opens alone
 * and says so. */
DrPrecisionDocument *dr_precision_doc_open(const char *path, uint64_t limit,
    uint64_t other_retained_bytes, const DrJobControl *control, char **err_out);
void dr_precision_doc_free(DrPrecisionDocument *document);
/* ---- CMYK documents (#1 phases 1, 2, 4a, 4b and 4c, and the artwork batch) ----
 * A CMYK document is a DrPrecisionDocument whose leaves are straight
 * CMYKA16 ink (0 = none, 65535 = 100 %): raster layers and (phase 2)
 * Levels/Curves/Hue/Saturation adjustment records, each with an optional
 * layer mask, and (phase 4a) Normal and Pass Through groups of them,
 * clipping masks, the CMYK blend modes (evaluated on
 * 1 - ink) and a layer style (the styles batch: Fill Opacity, every layer
 * effect with its colours as straight "ink" [c, m, y, k] and the hex
 * canonical under the profile, knockout and Blend Interior; Blend If
 * refuses by name), the global_light edit; merges and Stamp Visible
 * through the dr_precision_doc_edit structure ops; and (phase 4b) every
 * geometry op of dr_precision_doc_edit (resize, crop, canvas_size,
 * straighten, rotate_canvas, the quarter turns and flips, affine/perspective
 * on pixels or a mask and their sets, perspective_crop, align_set,
 * distribute_set, move/transform/puppet/perspective_selected on a layer or
 * its mask), the warp, Puppet Warp and Perspective Warp exports and Liquify
 * (dr_precision_doc_selected_edit op "liquify"), each resampling the five
 * planes premultiplied by alpha as RGB does. A CMYK warp is applied at once
 * and keeps no binding: dr_precision_doc_warp_info answers a null field and
 * clear/bake change nothing; and (phase 4c) Copy, Cut, Copy Merged and
 * Paste through the typed clipboard (CMYK payloads, RZCB v4), the layers
 * paste between CMYK documents, Duplicate to (New) Document, Layer Via
 * Copy / Cut, Stamp Selected and Merge Group. It keeps its CMYK ICC
 * profile verbatim and a declared depth (8 or 16; an 8-bit file stores
 * codes x257 and writes round(v/257)). Its .rz word is 82 (83 masks and
 * adjustment records, 84 groups/clipping/modes/Fill, 85 alpha channels and
 * vector masks, 87 type, shape and fill descriptions; each admits what the
 * lower ones do, and a pre-87 word holding a description refuses "a CMYK
 * text, shape or fill layer requires DRDC 87"). Its
 * dr_precision_doc_render / _render_region answer the RGBA16 VIEW in the
 * built-in Display P3 (relative colorimetric with BPC, sampled 33^4 link);
 * a region divisor above 1 refuses by name. dr_precision_doc_info /
 * _descriptor carry "color_mode":"cmyk", "storage":"cmyka16_unorm",
 * "cmyk_profile" and "cmyk_depth". Every operation outside the core's CMYK
 * table (supported_operations in the descriptor) refuses with
 * "<operation> is unavailable in a CMYK document".
 * Every export below is NULL-tolerant; errors go through err_out. */
#define DR_CMYK_SOURCE_NONE 0 /* also every file cmyk_gate refuses */
#define DR_CMYK_SOURCE_TIFF 1
#define DR_CMYK_SOURCE_JPEG 2
/* Header work only (a JPEG's header segments, a TIFF's directory; the file
 * is never read whole): which CMYK container the file is, when it opens as a
 * CMYK document through dr_precision_doc_open (a CMYK TIFF: chunky or planar,
 * 8 or 16 bits, its first extra sample read as its transparency, other extra
 * samples named in a warning). NONE for a PSD (a CMYK PSD is
 * dr_psd_import_report's "import_precision":"cmyk", opened by
 * dr_precision_doc_open_psd) and for an untagged CMYK TIFF or JPEG
 * while no working CMYK profile is set: that file opens converted to RGB. */
int32_t dr_path_cmyk_source(const char *path);
bool dr_precision_doc_is_cmyk(const DrPrecisionDocument *document);
/* {"profile","depth","storage","view_profile"}; NULL if not CMYK. Free with
 * dr_string_free. */
char *dr_precision_doc_cmyk_info(const DrPrecisionDocument *document);
/* The CMYK ICC bytes, BORROWED (valid while the document lives); NULL and
 * *length 0 if not CMYK. */
const uint8_t *dr_precision_doc_cmyk_profile(const DrPrecisionDocument *document, size_t *length);
/* options: {"intent":"perceptual|relative|saturation|absolute",
 * "black_point_compensation":bool,"flatten":bool,"depth":8|16} (defaults
 * relative, true, false, 16; depth toward CMYK only; NULL = defaults).
 * icc NULL: the working CMYK profile (toward CMYK; refused by name when none
 * is set) or the built-in sRGB (toward RGB). Plans answer
 * {"working_memory_plan_bytes":n,"refusal":null|"<sentence>",
 * "flatten_required":bool,"flattens":bool} (flattens: the conversion
 * flattens first under these options). Toward CMYK admits a 16-bit RGB document of plain
 * root raster layers; anything else names its first blocker (flatten:true
 * runs Flatten Image first); a 32-bit float document and alpha channels
 * refuse. Toward RGB yields a 16-bit RGB document, layers kept. A Lab
 * document converts toward CMYK through 16-bit ProPhoto RGB (relative
 * colorimetric with BPC, never narrowed; #1 Lab phase 5a, lab_cmyk), its
 * blocker naming a Blend If channel Lightness, a or b, its plan adding the
 * stage's bytes. */
char *dr_precision_doc_convert_to_cmyk_plan(const DrPrecisionDocument *document,
    const uint8_t *icc, size_t icc_len, const char *options, char **err_out);
DrPrecisionDocument *dr_precision_doc_convert_to_cmyk(const DrPrecisionDocument *document,
    const uint8_t *icc, size_t icc_len, const char *options, uint64_t limit,
    uint64_t other_retained_bytes, const DrJobControl *control, char **err_out);
char *dr_precision_doc_convert_cmyk_to_rgb_plan(const DrPrecisionDocument *document,
    const uint8_t *icc, size_t icc_len, const char *options, char **err_out);
DrPrecisionDocument *dr_precision_doc_convert_cmyk_to_rgb(const DrPrecisionDocument *document,
    const uint8_t *icc, size_t icc_len, const char *options, uint64_t limit,
    uint64_t other_retained_bytes, const DrJobControl *control, char **err_out);
/* ---- The Grayscale document (#1 phase 1a, ffi_gray.rs) -----------------
 * A typed document whose storage is R = G = B in the RGB twin of its Gray
 * ICC profile (u16 at 8/16 bits, the linear float at 32); its descriptor
 * reports "color_mode":"gray","gray":{"profile","depth"}. Every other
 * typed document reports "color_mode":"rgb". NULL-tolerant; errors through
 * err_out (free with dr_string_free), JSON freed with dr_string_free.
 * Styles, Blend If and channels (#1 phase 2d, gray_style): dr_precision_doc_edit's
 * layer_style takes every effect; each colour (hex, linear, gradient stop, a
 * custom pattern's texels) is stored as its gray by Mode > Grayscale's rule,
 * an effect's blend mode follows the layer modes ("The <Mode> blend mode is
 * unavailable in a Grayscale document."), and Blend If (in
 * layer_style and layer_blend_if) names Gray only ("Blend If on the <Name>
 * channel is unavailable in a Grayscale document.", an
 * identity range included). global_light and the alpha-channel edits are
 * admitted, and a scalar edit, its statistics and read_plane take target
 * mask or channel; a colour plane (red, green, blue, alpha, an ink plate,
 * paint_blend's plane) refuses for good ("A colour plane is unavailable in a
 * Grayscale document: its one colour channel, Gray, is the layer, ..."). RGB
 * to Gray carries effects and a Gray Blend If greyed; an effect in a mode
 * outside the list or a Blend If on another channel is named with
 * flatten_required.
 * Geometry, selections and the clipboard (#1 phase 4): every geometry edit
 * (resize, crop, canvas_size, straighten, rotate_canvas, the rotations and
 * flips, affine/perspective and their sets, perspective_crop, align_set,
 * distribute_set), the selected pixels of a layer, its mask or an alpha
 * channel, the source selections, Liquify, the warp, Puppet Warp and
 * Perspective Warp exports, duplicate_to_new_document (the result Gray of
 * the same policy), layer_via and stamp_selected run through the RGB kernels
 * on R = G = B. A Gray warp is applied at once and keeps no binding
 * (dr_precision_doc_warp_info answers a null field). A colour-plane target of
 * the selected pixels and a "color" selection refuse with the colour-plane
 * sentence. The layers paste refuses a Gray clip into another mode and
 * another mode's clip into a Gray document ("Grayscale layers paste only
 * into a Grayscale document, ..."). The clipboard is DrClipboardPayload's
 * GRAYSCALE paragraph.
 * Analysis and multi-image (#1 phase 6b): the histogram and stack_mode run
 * on R = G = B; scalar_edit takes target "gray" (the layer: component 0
 * read, the result written to R, G and B; replace_canvas only, u16 storage
 * only; on another document "scalar target gray is a Grayscale document's
 * layer"); Super Zoom, the neural layer apply, Generative Fill and its
 * variations and Merge to HDR fold their result to its gray by Mode >
 * Grayscale's rule (their contracts below). */
#define DR_GRAY_SOURCE_NONE 0
#define DR_GRAY_SOURCE_PNG 1
#define DR_GRAY_SOURCE_TIFF 2
#define DR_GRAY_SOURCE_JPEG 3
/* Header work only: which gray container the file is, when it opens as a
 * Grayscale document through dr_precision_doc_open_gray — a PNG of colour
 * type 0/4 at 8/16 bits, a BlackIsZero TIFF of 1-2 unsigned 8/16-bit
 * samples, a one-component 8-bit JPEG — each with no profile or a usable
 * Gray ICC (a PNG's gAMA/cHRM only beside sRGB or that ICC). NONE for every
 * other file, which keeps today's open. */
int32_t dr_path_gray_source(const char *path);
/* working_gray: the working Gray's wire name ("dot_gain_20", "sgray", ...),
 * assigned (with an import warning) to a file with no profile; a PNG sRGB
 * chunk is sGray; an embedded Gray ICC is kept verbatim. */
DrPrecisionDocument *dr_precision_doc_open_gray(const char *path, const char *working_gray,
    uint64_t limit, uint64_t other_retained_bytes, const DrJobControl *control, char **err_out);
/* request: {"depth":8|16|32,"space"?:"<gray space>","fill_srgb":[r,g,b,a] in 0..1,
 * "layer_name":"...","ppi":n}; icc (a Gray ICC) wins over space; neither
 * refuses. */
DrPrecisionDocument *dr_precision_doc_new_gray(uint32_t width, uint32_t height,
    const uint8_t *icc, size_t icc_len, const char *request, uint64_t limit,
    uint64_t other_retained_bytes, char **err_out);
/* Image > Mode > Grayscale from RGB: options {"space"?:"<gray space>",
 * "flatten"?:bool,"depth"?:8|16} (icc, a Gray ICC, wins over space; a 32-bit
 * document converts to 32-bit Grayscale). Plans answer
 * {"working_memory_plan_bytes","refusal","flatten_required","flattens"}; a
 * record a Grayscale document cannot hold is named with flatten_required. A
 * Generative Layer converts as its pixels: from a document in the Gray
 * profile's RGB twin its variations stay live, from any other profile they
 * are detached, as Convert to Profile detaches them on RGB. An 8- or 16-bit
 * Lab Color document converts too (#1 Lab phase 5a-ii): each pixel's L*
 * through the Gray profile's tone curve; an adjustment layer is named with
 * flatten_required. */
char *dr_precision_doc_convert_to_gray_plan(const DrPrecisionDocument *doc, const uint8_t *icc,
    size_t icc_len, const char *options, char **err_out);
DrPrecisionDocument *dr_precision_doc_convert_to_gray(const DrPrecisionDocument *doc,
    const uint8_t *icc, size_t icc_len, const char *options, uint64_t limit,
    uint64_t other_retained_bytes, const DrJobControl *control, char **err_out);
/* Image > Mode > RGB Color on a Grayscale document: Convert to Profile from
 * its twin into icc (an RGB profile, required); options {"intent"?,
 * "black_point_compensation"?:bool}. */
char *dr_precision_doc_convert_gray_to_rgb_plan(const DrPrecisionDocument *doc,
    const uint8_t *icc, size_t icc_len, const char *options, char **err_out);
DrPrecisionDocument *dr_precision_doc_convert_gray_to_rgb(const DrPrecisionDocument *doc,
    const uint8_t *icc, size_t icc_len, const char *options, uint64_t limit,
    uint64_t other_retained_bytes, const DrJobControl *control, char **err_out);
/* A Grayscale document's Gray ICC bytes, verbatim, BORROWED until the
 * document is freed; NULL and *length 0 for a NULL handle or a document that
 * is not Grayscale. */
const uint8_t *dr_precision_doc_gray_profile(const DrPrecisionDocument *document, size_t *length);
/* Edit > Assign Profile, Convert to Profile and Image > Mode > 8/16/32
 * Bits/Channel on a Grayscale document (#1 phase 5a, gray_profile.rs).
 * request {"op":"assign"|"convert"|"depth","space"?:"<gray space>",
 * "intent"?,"black_point_compensation"?:bool,"depth"?:8|16|32}: icc (a Gray
 * ICC) wins over space and assign/convert need one of them; depth only (and
 * always) with op depth; intent and BPC only with convert (relative without
 * BPC by default; not applied at 32 bits); an unknown key refuses. An RGB
 * or CMYK profile refuses "“<name>” is an RGB|a CMYK profile: a
 * Grayscale document is assigned and converted to Gray profiles only.";
 * 32 to 8/16 refuses "A 32-bit Grayscale document converts to 8 or 16
 * Bits/Channel through HDR Toning (hdr_toning with bit_depth 8 or 16).".
 * Assign keeps every sample; Convert is the RGB
 * conversion between the two twins, R = G = B rewritten from R (on the x257
 * grid at 8 bits); 16 to 8 rounds raster, mask and alpha-channel samples to
 * round(v/257)*257, 8 to 16 relabels, 8/16 to 32 promotes. The plan answers
 * owned JSON {"working_memory_plan_bytes","refusal","changes"}; the edit
 * answers NULL with no error when it changes nothing (the same profile bytes
 * or depth). The RGB assign/convert exports refuse a Grayscale document.
 * control is single-use. */
char *dr_precision_doc_gray_profile_plan(const DrPrecisionDocument *document, const uint8_t *icc,
    size_t icc_len, const char *request, char **err_out);
DrPrecisionDocument *dr_precision_doc_gray_profile_edit(const DrPrecisionDocument *document,
    const uint8_t *icc, size_t icc_len, const char *request, uint64_t limit,
    uint64_t other_retained_bytes, const DrJobControl *control, char **err_out);
/* #1 Gray phase 6a-i: a type, shape or fill layer's colours as their gray (Mode ▸ Grayscale's
 * rule). request {"srgb":[[r,g,b],...],"linear":[[r,g,b],...],"pattern":{pattern object}}, each
 * key optional, each list <= 1024; srgb 0..1, linear finite. Answers the same keys: an srgb or
 * linear triple with r = g = b verbatim, else [v,v,v] (srgb: the sRGB value of its luminance;
 * linear: its luminance Y); a pattern {"preset"} verbatim, a custom tile's non-neutral texels
 * greyed through the twin (a Pattern Overlay's rule). A non-Gray document refuses. Free with
 * dr_string_free. */
char *dr_precision_doc_gray_artwork_colors(const DrPrecisionDocument *document, const char *request,
    char **err_out);
/* ---- Duotone (#1 Duotone phase 1a, ffi_duotone.rs) ----------------------
 * A Duotone document is a Grayscale document (8 or 16 bits, layers and all;
 * its descriptor keeps "color_mode":"gray") whose policy carries a duotone
 * description, reported as "duotone":{"type","inks":n}. Options JSON (one
 * schema everywhere; an unknown key refuses): {"inks":[{"name":1-64 chars,
 * "lab":[L 0..100, a -128..127, b -128..127] (D50),"curve":[13 x number
 * 0..100 | null]}] (1..4),"overprints"?:{"1+2":[L,a,b]|null,...}}; curve
 * slots are the inputs 0,5,10,20,30,40,50,60,70,80,90,95,100 %, slots 0 and
 * 12 required, null blank; overprint keys every combination of 2+ inks by
 * size then lexicographic, missing or null the default. The normalized
 * reply adds "type" (monotone|duotone|tritone|quadtone), every overprint key
 * (null where default) and "overprint_display" {key:[L,a,b]} (the effective
 * colours). The view: per gray code g (the R channel, u16), k = 1 - g/65535,
 * each ink's coverage c = S(255k)/255 (Curves' natural spline through the
 * set slots x2.55), c' = 1 - kTRC(1 - c), the Neugebauer sum over the ink
 * subsets of prod c' prod (1 - c') P_S in PCS XYZ (paper Lab (100,0,0), each
 * ink its Lab, an authored overprint, else the paper times the product of
 * the inks' ratios to it), then the twin's matrix (clamped) and encoder:
 * RGBA16 in the document's profile, alpha kept. .rz holds it under word 109
 * and a DUOT table after GRAY. NULL-tolerant; errors through err_out (free
 * with dr_string_free). */
/* options checked and normalized; free with dr_string_free. */
char *dr_duotone_options_normalize(const char *options, char **err_out);
/* A Duotone document's normalized options; NULL with no error for any other
 * document. */
char *dr_precision_doc_duotone_options(const DrPrecisionDocument *document, char **err_out); /* NULL, no error: not Duotone */
/* Image > Mode > Duotone...'s plan: {"working_memory_plan_bytes","refusal",
 * "changes"}; NULL options fail (Duotone -> Grayscale is
 * dr_precision_doc_convert_duotone_to_gray). A source that is not an
 * 8- or 16-bit Grayscale document is the refusal ("Duotone converts an 8- or
 * 16-bit Grayscale document; convert to Grayscale first." /
 * "...; convert to 8 or 16 Bits/Channel first."); malformed options fail. */
char *dr_precision_doc_set_duotone_plan(const DrPrecisionDocument *document, const char *options, char **err_out);
/* Image > Mode > Duotone...: the description set or replaced, every sample
 * shared; NULL options fail. NULL with no error when nothing changes (the
 * same description). Free with dr_precision_doc_free. */
DrPrecisionDocument *dr_precision_doc_set_duotone(const DrPrecisionDocument *document, const char *options, char **err_out);
/* The Duotone view of image (an RGBA16 projection or layer source of
 * document), admitted under limit beside other_retained_bytes; refuses a
 * document that is not Duotone and an image that is not RGBA16. control is
 * single-use. Free with dr_precision_image_free. */
DrPrecisionImage *dr_precision_doc_duotone_view(const DrPrecisionDocument *document, const DrPrecisionImage *image,
    uint64_t limit, uint64_t other_retained_bytes, const DrJobControl *control, char **err_out);
/* Phase 1b (duotone_convert.rs, duotone_files.rs). The render through the
 * inks: an RGB document in the twin, each raster leaf through the view, the
 * record tree, masks, clipping, modes, a Gray Blend If, comps, channels,
 * guides, paths, samplers, light, metadata and resolution carried; an
 * adjustment layer or a layer style with effects (first in postorder) is the
 * plan's refusal with "flatten_required", and flatten runs Flatten Image
 * first. Plan: {"working_memory_plan_bytes","refusal","flatten_required",
 * "flattens"}. A document that is not Duotone refuses. control single-use;
 * free documents with dr_precision_doc_free and strings with dr_string_free. */
char *dr_precision_doc_duotone_render_plan(const DrPrecisionDocument *document, bool flatten, char **err_out);
DrPrecisionDocument *dr_precision_doc_duotone_render(const DrPrecisionDocument *document, bool flatten,
    uint64_t limit, uint64_t other_retained_bytes, const DrJobControl *control, char **err_out);
/* Duotone -> RGB Color: the render, then Convert to Profile from the twin
 * into icc (an identity pipeline assigns it). options NULL or
 * {"intent"?,"black_point_compensation"?,"flatten"?}; the plan is the
 * render's plus Gray -> RGB's, with the render's keys. */
char *dr_precision_doc_convert_duotone_to_rgb_plan(const DrPrecisionDocument *doc, const uint8_t *icc,
    size_t icc_len, const char *options, char **err_out);
DrPrecisionDocument *dr_precision_doc_convert_duotone_to_rgb(const DrPrecisionDocument *doc, const uint8_t *icc,
    size_t icc_len, const char *options, uint64_t limit, uint64_t other_retained_bytes, const DrJobControl *control,
    char **err_out);
/* Duotone -> Grayscale (duotone_gray.rs): the same Gray document, profile
 * and depth, each raster leaf's gray code g through the inks' luminance —
 * Y of the view's mix at k = 1 - g/65535, clamped to [0, 1], encoded
 * through the document's own Gray profile at its depth — alpha kept, the
 * inks dropped; the record tree, masks, clipping, modes, comps, channels,
 * guides, paths, samplers, light, metadata and resolution carried. The
 * render's blockers are the plan's refusal with "flatten_required", and
 * flatten runs Flatten Image first. Plan: {"working_memory_plan_bytes",
 * "refusal","flatten_required","flattens"}. A document that is not Duotone
 * refuses ("the Duotone Grayscale conversion takes a Duotone document").
 * control single-use; free documents with dr_precision_doc_free and strings
 * with dr_string_free. */
char *dr_precision_doc_convert_duotone_to_gray_plan(const DrPrecisionDocument *document, bool flatten, char **err_out);
DrPrecisionDocument *dr_precision_doc_convert_duotone_to_gray(const DrPrecisionDocument *document, bool flatten,
    uint64_t limit, uint64_t other_retained_bytes, const DrJobControl *control, char **err_out);
/* Duotone -> Bitmap: Duotone -> Grayscale, then Gray -> Bitmap
 * (dr_precision_doc_convert_to_bitmap) under options (NULL: the defaults;
 * Bitmap's schema, flatten included). The plan answers Bitmap's keys
 * ("width","height" included) with the refusal, flatten_required and
 * flattens of the same document with its inks dropped (Bitmap's own
 * blockers and its 8 Bits/Channel step); flatten runs Flatten Image on the
 * Duotone document first. A document that is not Duotone refuses ("the
 * Duotone Bitmap conversion takes a Duotone document"). The result carries
 * no inks. */
char *dr_precision_doc_convert_duotone_to_bitmap_plan(const DrPrecisionDocument *document, const char *options,
    char **err_out);
DrPrecisionDocument *dr_precision_doc_convert_duotone_to_bitmap(const DrPrecisionDocument *document,
    const char *options, uint64_t limit, uint64_t other_retained_bytes, const DrJobControl *control,
    char **err_out);
/* A Duotone Options (.ado) file: Photoshop's duotone specification (the
 * colour-mode data of a Duotone PSD, duotone_ado.rs). Read answers the
 * normalized options JSON; write takes options JSON and writes atomically
 * (Lab to 0.01, curve points to 0.1 %). */
char *dr_duotone_read_ado(const char *path, char **err_out);
bool dr_duotone_write_ado(const char *path, const char *options, char **err_out);
/* #1 Duotone phase 2a. A Duotone layer's (or composite's) thumbnail: image (u16, this document's
 * twin) fitted at u16 first (longest side max_side, never upscaled), then through the inks' view,
 * quantized once without dither; RGBA8 in the document's profile, no ICC. NULL with the reason for a
 * document that is not Duotone, a non-u16 image, the working limit and a cancel. Free with
 * dr_image_free. */
DrImage *dr_precision_doc_duotone_thumbnail_rgba8(const DrPrecisionDocument *document,
    const DrPrecisionImage *image, uint32_t max_side, uint64_t limit, uint64_t other_retained_bytes,
    const DrJobControl *control, char **err_out);
/* ---- Indexed Color (#1 Indexed phases 1a to 3c, ffi_indexed.rs) ---------
 * A typed document of ONE canvas-sized raster layer, "Index", whose u16
 * storage is its palette's render: each pixel palette[i] x257 with alpha
 * 65535, alpha 0 at the transparent index, in an RGB profile. The palette
 * holds 2..256 8-bit RGB entries (duplicates allowed), the index plane one
 * byte a pixel. Its descriptor reports "color_mode":"indexed",
 * "indexed":{"colors":n,"transparent_index":t|null} and supported_operations
 * (its family table, in order). dr_precision_doc_edit admits, in every
 * blend mode where a mode applies (a mode but Normal re-indexes against the
 * mode's full-strength colour over each backdrop): paint_blend,
 * bucket_fill, stroke_layer_edge,
 * paint, erase, erase_native; resize, crop, canvas_size, straighten,
 * rotate_canvas, rotate90/180/270, flip_horizontal/vertical,
 * perspective_crop; affine and perspective (target pixels), affine_set,
 * perspective_set; transform_selected and move_selected (target layer or
 * an alpha channel; a mask target refuses as transform_mask, a red, green,
 * blue, alpha or ink target with "An Indexed Color document has one
 * channel, Index: ..."); rename_layer, layer_label,
 * layer_lock, layer_visible, move_layer_to, layer_open; set_channel,
 * remove_channel, rename_channel, duplicate_channel, channel_properties,
 * import_channel, and read_plane, scalar_edit and scalar_statistics of an
 * alpha channel; retouch_clone in place (output "new_layer" refuses as
 * add_layer; re-indexed to each pixel's own nearest entry); and through
 * dr_precision_doc_select the selection kinds color_range, similar, alpha,
 * magic_wand and quick, which sample the palette colours (color and
 * luminance, and read_plane, scalar_edit or scalar_statistics of red,
 * green, blue or alpha, refuse with the one-channel sentence; refine_edge
 * refuses for good). dr_precision_doc_history_brush paints the RGB storage
 * and re-indexes the same way; dr_precision_doc_retouch_from refuses as
 * content_aware_move. A move or a
 * Nearest pick keeps every index exactly; anything else is the RGB op
 * re-indexed. A pixel an edit leaves clear (alpha < 128: an erase, Cut,
 * Clear, a canvas extension, an uncovered transform or move area) takes
 * the transparent index, else the entry nearest the background colour:
 * a straighten's own background, else the request's "indexed_background"
 * ({"color":[r,g,b],"color_space":"document"|"srgb"|"linear"}, accepted
 * on every admitted paint, geometry and selected-pixel request, on
 * dr_precision_doc_clipboard_cut and on a warp set), else white,
 * the default background colour. The warp exports set a
 * field with no binding (warp_info null after; clear and bake change
 * nothing); Paste is dr_precision_doc_indexed_paste (also reached through
 * dr_precision_doc_clipboard_paste, unmasked). Assign Profile keeps every
 * index and the table's codes; Convert to Profile converts the table and
 * keeps every index (an RGB profile only). Every other request refuses
 * ("<op> is not available in Indexed Color." for the
 * disabled ops, Duplicate Layer, the layers paste, Puppet Warp, a
 * mask transform, HDR Toning, gradient, the retouch brushes, refine_edge,
 * global_light and stack_mode among them; "<op> needs more than one
 * layer, and an Indexed Color document holds one." for
 * align_set, arrange_layer, auto_blend_layers, distribute_set, flatten,
 * link_layers, remove_layers and unlink_layers; else "<op> is unavailable
 * in an Indexed Color document: Doctored offers it on RGB documents
 * only."). A kept-Indexed
 * export refuses while the one layer is hidden. .rz holds it under
 * conditional word 108 and an INDX table. A colour-mode-2 PSD or PSB's
 * stored composite opens as one (import_precision "indexed",
 * dr_precision_doc_open_indexed_psd), and dr_precision_doc_psd_export_report
 * and dr_precision_doc_save_psd (NULL projection) write a colour-mode-2 PSD
 * or PSB in both modes, in Photoshop's form: the 768-byte table, 1046, 1047
 * (0xFFFF with no transparent index), no layer record, the RLE index plane;
 * a layered TIFF, a side past the container's limit and a hidden layer
 * refuse.
 * NULL-tolerant; errors through err_out (free with dr_string_free). */
#define DR_INDEXED_SOURCE_NONE 0
#define DR_INDEXED_SOURCE_PNG 1
#define DR_INDEXED_SOURCE_GIF 2
#define DR_INDEXED_SOURCE_TIFF 3
/* Header work only: PNG for a PNG of colour type 3 at depth 1/2/4/8 with a
 * PLTE of 2..256 entries, a tRNS (if any) of alphas 0 or 255 with at most one
 * 0, no acTL, and an RGB iCCP, sRGB or no profile (gAMA/cHRM only beside
 * them); GIF for a GIF of exactly one image descriptor at (0,0) covering the
 * logical screen, with a colour table and no ICC extension (a Graphic
 * Control transparent index inside the table); TIFF for a classic TIFF of
 * Photometric 3 (a SHORT), one sample of 1/2/4/8 bits or [8,8] with
 * ExtraSamples [2], chunky, unsigned, a lossless compression, a ColorMap of
 * 3*2^bits and an RGB ICC or none. NONE for every other file, which keeps
 * today's open. */
int32_t dr_path_indexed_source(const char *path);
/* An indexed PNG, GIF or palette TIFF as an Indexed Color document:
 * palette, transparent index and every index kept exactly (a TIFF's entry i
 * is its ColorMap's (v+128)/257; its alpha must be 0 or 255, and the clear
 * pixels exactly one index's, which becomes the transparent index); the
 * iCCP or TIFF ICC profile verbatim, else the built-in sRGB (untagged: an
 * import warning); EXIF orientation applied.
 * control is single-use. Free with dr_precision_doc_free. */
DrPrecisionDocument *dr_precision_doc_open_indexed(const char *path, uint64_t limit,
    uint64_t other_retained_bytes, const DrJobControl *control, char **err_out);
/* A colour-mode-2 PSD or PSB's stored composite (depth 8, Raw or RLE; a ZIP
 * composite, another depth, a colour table not 768 bytes, a palette outside
 * 2..256 and an index outside the palette refuse by sentence) as an Indexed
 * Color document: the count from resource 1046 (else 256), the transparent
 * index from 1047, every index as stored, an RGB ICC in 1039 kept (else
 * sRGB, warned), its resolution and colour samplers kept. The file must
 * still match expected_fingerprint (dr_psd_import_report's
 * source_fingerprint). Its report's layered choice refuses ("An Indexed
 * Color file opens from its stored composite: an Indexed Color document
 * holds one layer."). control is single-use. Free with
 * dr_precision_doc_free. */
DrPrecisionDocument *dr_precision_doc_open_indexed_psd(const char *path,
    const char *expected_fingerprint, uint64_t limit, uint64_t other_retained_bytes,
    const DrJobControl *control, char **err_out);
/* Image > Mode > Indexed Color from an 8-bit RGB or Grayscale document.
 * options {"palette":"exact"|"web"|"uniform"|"adaptive"|"system_mac"|
 * "system_windows"|"custom","table"?:[[r,g,b],...] (Custom only and required
 * with it: 2..256 entries, else "a colour table holds 2 to 256 colours"; the
 * transparent entry is appended after it, so a 256-entry table beside a
 * transparent pixel refuses),"colors"?:2..256 (256; refused with Custom),"forced"?:"none"|"black_white"|
 * "primaries"|"web"|"custom" (Uniform and Adaptive only),"forced_table"?:[[r,g,b],...] (forced
 * custom only and required with it: 2..256 entries leading the palette in order, a repeat
 * dropped),"transparency"?:bool (true),"matte"?:[r,g,b]|null,
 * "dither"?:"none"|"diffusion"|"pattern"|"noise" (diffusion; pattern and noise are phase 3c's
 * declared ordered dithers, kept by phase 3d's Photoshop take with its numbers disclosed, run
 * as none with the Exact palette),"amount"?:1..100
 * (75; diffusion only),"preserve_exact"?:bool (diffusion only),"flatten"?:bool}; an unknown
 * key refuses, and "local_perceptual", "local_selective", "master_perceptual" and
 * "master_selective" refuse for good ("The <title> palette is not available: its algorithm
 * is unpublished. Use Adaptive (median cut) instead."). The plan
 * answers {"working_memory_plan_bytes","refusal","flatten_required",
 * "flattens"}: anything but one plain layer is flatten_required ("Indexed
 * Color holds one layer: flatten the image."); a palette refusal (Exact past
 * the budget, Uniform with no cube that fits, Adaptive with no colour left
 * after the forced and transparent ones) is the plan's refusal and the
 * conversion's error, the same sentence. CMYK, Lab, float and 16-bit Gray
 * refuse by name. */
char *dr_precision_doc_convert_to_indexed_plan(const DrPrecisionDocument *doc, const char *options,
    char **err_out);
DrPrecisionDocument *dr_precision_doc_convert_to_indexed(const DrPrecisionDocument *doc,
    const char *options, uint64_t limit, uint64_t other_retained_bytes,
    const DrJobControl *control, char **err_out);
/* Image > Mode > RGB Color on an Indexed document: the RGB document its
 * storage is, every sample shared. A non-Indexed document refuses. */
DrPrecisionDocument *dr_precision_doc_convert_indexed_to_rgb(const DrPrecisionDocument *doc,
    char **err_out);
/* The palette index at (x, y); -1 for NULL, off the canvas, or a document
 * that is not Indexed Color. */
int32_t dr_precision_doc_indexed_index_at(const DrPrecisionDocument *doc, uint32_t x, uint32_t y);
/* Image > Mode > Color Table (phase 1c). A table is the JSON
 * {"colors":[[r,g,b],...] (2..256 entries),"transparent_index":n|null}
 * (absent = none; an unknown key refuses). The getter answers the
 * document's; a non-Indexed document refuses ("the document is not Indexed
 * Color"). Free strings with dr_string_free. */
char *dr_precision_doc_indexed_color_table(const DrPrecisionDocument *doc, char **err_out);
/* The plan {"working_memory_plan_bytes","refusal","changes"}: a table
 * shorter than the plane's largest index m plus one is the refusal "The
 * colour table holds n colours, but the image uses index m; it needs at
 * least m+1.", as is a non-Indexed document; a malformed table is the
 * error. */
char *dr_precision_doc_set_color_table_plan(const DrPrecisionDocument *doc, const char *table,
    char **err_out);
/* A new Indexed document with table's palette and transparent index, every
 * index kept and the one layer re-rendered. NULL with no error when the
 * table and transparent index equal the document's. control is single-use.
 * Free with dr_precision_doc_free. */
DrPrecisionDocument *dr_precision_doc_set_color_table(const DrPrecisionDocument *doc,
    const char *table, uint64_t limit, uint64_t other_retained_bytes,
    const DrJobControl *control, char **err_out);
/* Adobe Color Table (.act) files: 256 RGB triples (768 bytes), optionally
 * followed by a big-endian u16 count (2..256) and a big-endian u16
 * transparent index (0xFFFF none, else below the count), 772 bytes. A
 * 768-byte file is 256 entries with no transparent index; any other length
 * refuses. The reader answers the table JSON; the writer always writes 772
 * bytes, unused entries zero. */
char *dr_indexed_read_act(const char *path, char **err_out);
bool dr_indexed_write_act(const char *path, const char *table, char **err_out);
/* Paste and Paste Special on an Indexed document (phase 2d). request is
 * dr_precision_doc_clipboard_paste's JSON; the payload is pasted onto the
 * RGB storage above the Index layer (anchor and position ignored), masked
 * when coverage is not NULL (the canvas's u16 count, applied by set_mask at
 * outside_coverage: Paste Into and Paste Outside), merged down (a locked
 * Index layer refuses) and re-indexed against the old plane, each changed
 * pixel to t below alpha 128, else its nearest entry. NULL coverage with
 * length 0 pastes unmasked. The plan answers {"retained_bytes",
 * "scratch_bytes"}. control is single-use. A non-Indexed document refuses
 * ("the document is not Indexed Color"). Free with dr_string_free /
 * dr_precision_doc_free. The payload handle's typedef follows in the
 * clipboard section; the struct is declared here. */
struct DrClipboardPayload;
char *dr_precision_doc_indexed_paste_plan(const DrPrecisionDocument *doc,
    const struct DrClipboardPayload *payload, const char *request, char **err_out);
DrPrecisionDocument *dr_precision_doc_indexed_paste(const DrPrecisionDocument *doc,
    const struct DrClipboardPayload *payload, const char *request, const uint16_t *coverage,
    size_t coverage_len, uint8_t outside_coverage, uint64_t limit,
    uint64_t other_retained_bytes, const DrJobControl *control, char **err_out);
/* ---- Bitmap (#1 Bitmap phases 1a-3, ffi_bitmap.rs) ---------------------
 * An 8-bit Grayscale typed document (its Gray profile and twin) of ONE
 * canvas-sized opaque raster layer, "Bitmap", whose u16 storage is its
 * packed 1-bit plane's render: each pixel 65535 x4 white or (0,0,0,65535)
 * black. The plane is ceil(width/8) bytes a row, MSB first, 1 white, pad
 * bits 0; no alpha channel. Its descriptor reports "color_mode":"bitmap",
 * keeps the "gray" key, and supported_operations render, sample, convert,
 * export, (phase 2a-i) paint, erase, fill_stroke, geometry_crop,
 * geometry_canvas_size, geometry_rotate, geometry_flip and (phase 2a-ii)
 * clipboard_cut, geometry_resize, geometry_straighten, geometry_affine,
 * geometry_perspective, geometry_affine_set, geometry_perspective_set,
 * selected_pixels_move and (phase 3) clipboard_copy, layer_name,
 * layer_label, layer_lock, selection, clone and histogram.
 * dr_precision_doc_edit (and its plan) admits paint_blend, bucket_fill,
 * stroke_layer_edge, gradient and retouch_clone with blend_mode absent, 0,
 * 6 or 7 (phase 2b, phase 3; else "The Brush, Fill, Stroke, the Paint
 * Bucket, the Gradient and the Clone Stamp paint in Normal, Darken or
 * Lighten mode in a Bitmap document."), paint, erase, erase_native, crop,
 * canvas_size, rotate90/180/270, flip_horizontal/vertical and straighten
 * (phase 2a-i), resize, affine, perspective, affine_set, perspective_set
 * and the layer's move_selected (phase 2a-ii; transform_selected and
 * rotate_canvas refuse since phase 2b), and (phase 3) gradient
 * (its ramp thresholded, no ink snap), perspective_crop and an in-place
 * retouch_clone (output new_layer refuses as add_layer); rename_layer,
 * layer_label, layer_lock and a selection of any kind but magic_wand,
 * color_range, similar and refine_edge go to the shared parser and move no
 * bit. dr_precision_doc_selected_edit takes liquify only (the adjustments,
 * Auto and Equalize refuse as adjustment, the filters as filter), and
 * dr_precision_doc_history_brush paints the Gray storage, both then
 * thresholded the same way over white; each request
 * may carry "indexed_background" ({"color":[r,g,b],"color_space"?}, the
 * background swatch). The edit runs on the Gray storage with its own
 * sampler; its ink is snapped to black or white by its gray (white iff >=
 * 128), and every canvas pixel goes back on the plane white iff
 * gray*alpha + bg*(255 - alpha) >= 128*255, bg the background's bit (a
 * straighten's own background, else indexed_background, else white), so
 * area an edit uncovers or vacates takes the background. An edit that
 * moves no bit and changes neither the resolution nor the guides, ruler
 * origin, samplers, paths or comps is NULL with no error. Every other op
 * refuses in three tiers: "<op> is not available in Bitmap mode." for the
 * disabled ops (new layers, merges, masks,
 * styles, duplicate_layer, transform_selected, puppet_selected,
 * rotate_canvas, filter, adjustment, the profile ops, paste, stack_mode,
 * global_light, and a selection of kind magic_wand, color_range, similar
 * or refine_edge by its kind); "<op> needs more than one layer, and a
 * Bitmap document holds one." for align_set,
 * distribute_set, arrange_layer, move_layer_to, flatten, remove_layers,
 * link_layers and unlink_layers; else "<op> is unavailable in a Bitmap
 * document: Doctored offers it on Grayscale and colour documents only;
 * convert to Grayscale first." (smudge, art_history_brush, and
 * dr_precision_doc_retouch_from as content_aware_move, among them).
 * .rz holds it under
 * conditional word 110 (in the Gray word's place), its GRAY table then a
 * BTMP table. Phase 1c's files: a 1-bit PNG, TIFF or BMP opens as one
 * (dr_precision_doc_open_bitmap), and so does a colour-mode-0 PSD or PSB's
 * stored composite (import_precision "bitmap",
 * dr_precision_doc_open_bitmap_psd). The typed export with profile
 * document writes PNG (colour type 0, depth 1), TIFF (BitsPerSample 1,
 * Photometric 1) or BMP (1 bpp, black then white) from the plane at
 * bit_depth 8, replying bit_depth 1; a size, a sharpen or another
 * bit_depth refuses; JPEG is the 8-bit Grayscale render; any other format
 * with profile document refuses, and srgb, display-p3 and file export RGB.
 * dr_precision_doc_psd_export_report and dr_precision_doc_save_psd (NULL
 * projection) write a colour-mode-0 PSD or PSB (1 is black, no profile)
 * in both modes, with "depth":1,"color_mode":"bitmap" and
 * working_memory_plan_bytes; a layered TIFF and a side past the
 * container's limit refuse.
 * NULL-tolerant; errors through err_out (free with dr_string_free). */
/* Image > Mode > Bitmap from an 8-bit Grayscale document. options (at
 * most 2 MiB) {"method"?:"threshold"|"diffusion"|"pattern"|"halftone"|
 * "custom" (diffusion),"resolution"?:ppi (finite, 1..30000; omitted, or
 * equal to an axis's ppi, keeps that axis's pixels and ppi, else that axis
 * becomes round(n*r/ppi) pixels at r ppi, resampled Bicubic),
 * "flatten"?:bool, halftone's "frequency"?:lines per inch of the output
 * (finite, 1..999.999; 53),"angle"?:degrees (finite, -180..180; 45),
 * "shape"?:"round"|"ellipse"|"line"|"square"|"cross"|"diamond" (round),
 * custom's "pattern"?: the pattern object a Pattern Overlay takes
 * ({"preset"} or an inline {"name"?,"width","height","rgba"} tile;
 * checkerboard)}; an unknown key, or a method's key beside another method
 * ("mode option <key> applies to method halftone|custom"), refuses. 50%
 * Threshold makes gray >= 128 white; Pattern Dither is the 8x8 Bayer
 * ordered dither from the origin; Diffusion is Indexed's Floyd-Steinberg
 * at 100% onto black and white; Halftone Screen thresholds each pixel by
 * Adobe's spot function's area order in its cell; Custom makes a pixel
 * white where its gray exceeds the pattern's gray over white, tiled from
 * the origin (bitmap_method.rs). The plan answers
 * {"working_memory_plan_bytes","refusal","flatten_required","flattens",
 * "width","height"}: anything but one plain layer is flatten_required
 * ("Bitmap holds one layer: flatten the image."). A Bitmap document ("The
 * document is already in Bitmap mode."), any other mode, a Duotone
 * document included ("...; convert to Grayscale first.") and 16- or 32-bit
 * Gray ("...; convert to 8 Bits/Channel first.") refuse by name. control
 * is single-use. Free with dr_precision_doc_free. */
char *dr_precision_doc_convert_to_bitmap_plan(const DrPrecisionDocument *doc, const char *options,
    char **err_out);
DrPrecisionDocument *dr_precision_doc_convert_to_bitmap(const DrPrecisionDocument *doc,
    const char *options, uint64_t limit, uint64_t other_retained_bytes,
    const DrJobControl *control, char **err_out);
/* Which 1-bit file path is, header work only: 0 none, 1 PNG (colour type
 * 0, depth 1, no tRNS or acTL), 2 TIFF (one 1-bit sample, Photometric 0 or
 * 1, FillOrder 1), 3 BMP (1 bpp, BI_RGB, a black and white palette); an
 * ICC must be a Gray profile. Every other file answers 0 and keeps its
 * ordinary open. */
int32_t dr_path_bitmap_source(const char *path);
/* A file dr_path_bitmap_source answers for as a Bitmap document: its Gray
 * ICC, else sGray for a PNG sRGB chunk, else working_gray (a Gray working
 * space's wire name), with no untagged warning; the orientation applied;
 * the resolution the file's (a BMP's pixels per metre). Admitted as
 * pixels x 42 + the file + bookkeeping. control is single-use. Free with
 * dr_precision_doc_free. */
DrPrecisionDocument *dr_precision_doc_open_bitmap(const char *path, const char *working_gray,
    uint64_t limit, uint64_t other_retained_bytes, const DrJobControl *control, char **err_out);
/* A colour-mode-0 PSD or PSB's stored composite (depth 1, Raw or RLE; a
 * ZIP composite and another depth refuse by sentence) as a Bitmap
 * document: the file's 1 is black, its resolution and colour samplers
 * kept, a Gray ICC in resource 1039 kept, else working_gray. The file must
 * still match expected_fingerprint (dr_psd_import_report's
 * source_fingerprint). Its report's layered choice refuses ("A Bitmap file
 * holds no layers; open its stored composite."). control is single-use.
 * Free with dr_precision_doc_free. */
DrPrecisionDocument *dr_precision_doc_open_bitmap_psd(const char *path,
    const char *expected_fingerprint, const char *working_gray, uint64_t limit,
    uint64_t other_retained_bytes, const DrJobControl *control, char **err_out);
/* Image > Mode > Grayscale on a Bitmap document. options {"size_ratio"?:
 * integer 1..16 (1)}; an unknown key refuses. Ratio 1 is the Gray document
 * the storage is, every sample shared; ratio r averages each r x r block
 * (round half up of 255*whites/pixels, partial edge blocks over their
 * pixels inside the canvas) onto a ceil(w/r) x ceil(h/r) canvas at ppi/r.
 * The plan answers {"working_memory_plan_bytes","refusal","width",
 * "height"}; a non-Bitmap document refuses ("the document is not a Bitmap
 * document"). control is single-use. Free with dr_precision_doc_free. */
char *dr_precision_doc_convert_bitmap_to_gray_plan(const DrPrecisionDocument *doc,
    const char *options, char **err_out);
DrPrecisionDocument *dr_precision_doc_convert_bitmap_to_gray(const DrPrecisionDocument *doc,
    const char *options, uint64_t limit, uint64_t other_retained_bytes,
    const DrJobControl *control, char **err_out);
/* ---- Lab Color (#1, ffi_lab.rs) ----------------------------------------
 * A typed document whose u16 storage holds L*, a*, b* (D50) in R, G, B in
 * the ICC v4 Lab encoding (L* = 100v, a* and b* = 255v - 128; an 8-bit document
 * on the x257 grid), every layer in the Lab twin (an RGB-data, Lab-PCS
 * profile with identity tables; the view is dr_precision_image_lab_display's,
 * never ColorSync's decode of the twin). Its descriptor
 * reports "color_mode":"lab","lab":{"depth":8|16,"filter_plans":{...}}. 8 or
 * 16 bits only. NULL-tolerant; errors through err_out (free
 * with dr_string_free), JSON freed with dr_string_free.
 * Blend If (#1 phase 2f, lab_style): dr_precision_doc_edit's layer_style
 * and layer_blend_if take channel red, green and blue, which are Lightness, a
 * and b (ramps on the stored codes; not measured against Photoshop); gray
 * refuses, an identity range included ("Blend If on the Gray
 * channel is unavailable in a Lab Color document: its Blend If
 * channels are Lightness, a and b."). Levels Auto (a backdrop scoped
 * analysis's "auto") with mode tone stretches Lightness alone (black[0],
 * white[0]; a* and b* keep 0, 1, 1); mode color refuses at the plan and the
 * analysis ("Auto Color is unavailable in a Lab Color document."). Since
 * 2f's second part a Pattern Overlay or Pattern stroke
 * runs, its tile authored sRGB and converted to Lab codes before the pyramid
 * (the stored tile unchanged), and dr_precision_doc_edit admits
 * stroke_layer_edge in the Lab blend modes, its colour Lab codes.
 * Since #1 phase 6b-i dr_precision_doc_edit admits stack_mode (each
 * statistic per channel on the codes; an 8-bit result can land off the x257
 * grid), the selection kind color_range in every mode (tonal on the codes'
 * L*, hue and skin on the codes' 16-bit ProPhoto RGB encoding, skin's Lab the
 * codes' own) and move_selected, transform_selected, puppet_selected and
 * perspective_selected with target red, green, blue (Lightness, a, b) and
 * alpha; the family table adds histogram and layer_stack_mode. */
#define DR_LAB_SOURCE_NONE 0
#define DR_LAB_SOURCE_TIFF 1
/* Header work only (#1 Lab phase 3a): TIFF when the file is a flat Lab
 * TIFF dr_precision_doc_open_lab opens — classic II or MM, Photometric 8
 * (CIELab) or 9 (ICCLab), chunky, 3 samples or 4 with an unassociated alpha,
 * 8 or 16 bits, SampleFormat absent, all 1 or (CIELab) 1,2,2(,1), no
 * compression, LZW, Deflate or PackBits. NONE for every other file (an
 * associated-alpha or planar Lab TIFF among them), which keeps today's open. */
int32_t dr_path_lab_source(const char *path);
/* A flat Lab TIFF as a Lab Color document of its depth: CIELab samples by
 * TIFF 6.0 (measured against Photoshop), ICCLab by TIFF Technote 4 (8 bits
 * as the PSD, 16 bits L* 100v/65280, a* v/256 - 128: declared from the
 * published spec, not measured against Photoshop); resolution, metadata and
 * orientation as dr_precision_doc_open_gray. Optional single-use control
 * begun once. Free with dr_precision_doc_free. */
DrPrecisionDocument *dr_precision_doc_open_lab(const char *path, uint64_t limit,
    uint64_t other_retained_bytes, const DrJobControl *control, char **err_out);
/* request: {"depth":8|16,"fill_srgb":[r,g,b,a] in 0..1,"layer_name":"...",
 * "ppi":n}; the fill is converted by Image > Mode > Lab Color's rule. An
 * unknown key or depth 32 refuses. */
DrPrecisionDocument *dr_precision_doc_new_lab(uint32_t width, uint32_t height, const char *request,
    uint64_t limit, uint64_t other_retained_bytes, char **err_out);
/* Image > Mode > Lab Color from RGB or Grayscale (relative colorimetric
 * from the document profile, no BPC): options {"flatten"?:bool,
 * "depth"?:8|16} (default 16). Plans answer {"working_memory_plan_bytes",
 * "refusal","flatten_required","flattens"}; a record a Lab document cannot
 * hold yet is named with flatten_required; CMYK and 32-bit documents
 * refuse by name. An adjustment layer carries, its description unchanged and
 * rebound to the twin (#1 phase 2d; since phase 2f a Levels or Curves layer
 * with r, g, b, r_map, g_map or b_map too, its Red, Green and Blue stages
 * becoming Lightness, a and b by channel index), except an op a Lab document
 * refuses or a pixels-only op ("“<layer>” cannot convert to Lab Color: <its
 * Lab sentence> Flatten the image to convert it."). A layer style with
 * effects carries unchanged (#1 phase 2e; its colours convert at
 * composite), except an effect mode the Lab list lacks ("“<layer>” cannot
 * convert to Lab Color: <the style's
 * sentence> Flatten the image to convert it."). A Blend If (the layer's or
 * the style's) carries, its red, green and blue ranges becoming Lightness, a
 * and b (phase 2f), except a Gray or plate range ("“<layer>” cannot convert
 * to Lab Color: Blend If on the <Channel> channel is unavailable in a Lab
 * Color document: its Blend If channels are Lightness, a and b.
 * Flatten the image to convert it."). Layer comps carry unchanged (#1 Lab
 * phase 6a-ii); a flatten clears them first. */
char *dr_precision_doc_convert_to_lab_plan(const DrPrecisionDocument *doc, const char *options,
    char **err_out);
DrPrecisionDocument *dr_precision_doc_convert_to_lab(const DrPrecisionDocument *doc,
    const char *options, uint64_t limit, uint64_t other_retained_bytes,
    const DrJobControl *control, char **err_out);
/* Image > Mode > RGB Color on a Lab document: the twin into icc (an RGB
 * profile, required); options {"intent"?,"black_point_compensation"?:bool}.
 * The result is a 16-bit RGB document. An adjustment layer carries,
 * rebound; a Levels or Curves layer with r, g, b, r_map, g_map or b_map keeps
 * its record byte-equal, its Lightness, a and b stages becoming Red, Green and
 * Blue by channel index (#1 phase 2f). A layer style and a Blend If carry
 * unchanged (Lightness, a and b ranges become Red, Green and Blue), and so
 * do layer comps (phase 6a-ii). The plan's refusal names only a document
 * that is not Lab or a destination that does not prepare. */
char *dr_precision_doc_convert_lab_to_rgb_plan(const DrPrecisionDocument *doc, const uint8_t *icc,
    size_t icc_len, const char *options, char **err_out);
DrPrecisionDocument *dr_precision_doc_convert_lab_to_rgb(const DrPrecisionDocument *doc,
    const uint8_t *icc, size_t icc_len, const char *options, uint64_t limit,
    uint64_t other_retained_bytes, const DrJobControl *control, char **err_out);
/* #1 Lab phase 5a: 8/16 Bits/Channel on a Lab Color document (lab_depth.rs). bits 8 or 16.
 * 16 -> 8 puts every raster, alpha, mask and alpha-channel sample on the 8-bit (x257) grid;
 * 8 -> 16 changes only the declared depth. The plan answers {"working_memory_plan_bytes",
 * "refusal","changes"}; any other bits and a non-Lab document refuse. */
char *dr_precision_doc_lab_depth_plan(const DrPrecisionDocument *document, uint32_t bits, char **err_out);
DrPrecisionDocument *dr_precision_doc_lab_depth_edit(const DrPrecisionDocument *document, uint32_t bits,
    uint64_t limit, uint64_t other_retained_bytes, const DrJobControl *control, char **err_out);
/* NULL with no error when it changes nothing; free the plan with dr_string_free. */
/* The Lab document's VIEW: image (a Lab projection or layer source, its
 * profile the twin, u16) through the twin into the built-in Display P3 at
 * u16 (relative colorimetric, no BPC; Image > Mode's per-pixel rule), alpha
 * kept, tagged Display P3 -- the space a CMYK document's view is in. The
 * host narrows it (dr_precision_image_preview_rgba8 / _thumbnail_rgba8) and
 * draws it in Display P3. Charges 8 bytes a pixel plus the pipeline's
 * tables against limit. Refuses an image not in the twin, float storage,
 * scratch past limit and a cancel; control is single-use. Free with
 * dr_precision_image_free. */
DrPrecisionImage *dr_precision_image_lab_display(const DrPrecisionImage *image, uint64_t limit,
    uint64_t other_retained_bytes, const DrJobControl *control, char **err_out);
/* #1 Lab phase 6b-iii. The bytes a neural run on a Lab document reserves
 * beside its model's own plan: the Display P3 view of an in_width x
 * in_height input (dr_precision_image_lab_display's charge), the Lab image
 * of an out_width x out_height result at 8 bytes a pixel, the way back's
 * tables and bookkeeping. The result returns to L*a*b* codes by Image >
 * Mode > Lab Color's rule in dr_precision_doc_super_zoom_document and
 * dr_precision_doc_neural_layer_apply, which refuse a result not tagged
 * Display P3. Value-only; UINT64_MAX on error. */
uint64_t dr_lab_neural_scratch_bytes(uint32_t in_width, uint32_t in_height, uint32_t out_width,
    uint32_t out_height);
/* #1 phase 1b. A Lab layer's Layers-panel thumbnail, fitted BEFORE it
 * converts: image (u16, in the twin) aspect-fit at u16 with the
 * longest side max_side, never upscaled, then through the view's conversion
 * into Display P3 and quantized once without dither. Opaque-or-not RGBA8,
 * no ICC (the host tags it Display P3); free with dr_image_free. Charges the
 * fit's scratch, 12 bytes a fitted pixel and the pipeline's tables against
 * limit; refuses an image not in the twin, float storage and a cancel. */
DrImage *dr_precision_image_lab_thumbnail_rgba8(const DrPrecisionImage *image, uint32_t max_side,
    uint64_t limit, uint64_t other_retained_bytes, const DrJobControl *control, char **err_out);
/* The Lab TIFF save: the composite (projection, else a fresh render;
 * projection NULL or this canvas's u16 render) as a TIFF 6.0 CIELab file
 * (PhotometricInterpretation 8: L* unsigned, a* and b* signed; 16-bit a* and b*
 * in 1/256 units) at the document's depth, alpha only when some pixel is not
 * opaque, no ICC profile, written atomically. request {"format":"tiff",
 * "strip_metadata"?:bool}; another format refuses "Saving as <format> is
 * unavailable in a Lab Color document: it saves as a Lab TIFF, and Export as
 * PSD writes its PSD." A document holding spot channels refuses
 * "Spot channels are saved to a TIFF only from an RGB or CMYK document at 8
 * or 16 bits; save as .doctored, or delete the spot channels first." (#1
 * 1c-2a). The plan
 * answers {"working_memory_plan_bytes","refusal"}; a non-Lab document, spot
 * channels, a bad request or a foreign projection is its refusal. */
char *dr_precision_doc_save_lab_plan(const DrPrecisionDocument *document,
    const DrPrecisionImage *projection, const char *request, char **err_out);
bool dr_precision_doc_save_lab(const DrPrecisionDocument *document, const DrPrecisionImage *projection,
    const char *path, const char *request, uint64_t limit, uint64_t other_retained_bytes,
    const DrJobControl *control, char **err_out);
/* {"x","y","reach","ink_percent":[c,m,y,k],"ink16":[..],"alpha16",
 * "total_ink_percent"} over the unflattened composite; reach 0-2 is a
 * 1/3/5-pixel square, outside positions omitted. The stack's prepared
 * masks and one ink row are admitted against limit beside
 * other_retained_bytes first. */
char *dr_precision_doc_cmyk_sample(const DrPrecisionDocument *document, int32_t x, int32_t y,
    uint32_t reach, uint64_t limit, uint64_t other_retained_bytes, char **err_out);
/* Ink plane 0-3 (C, M, Y, K) of the composite on paper, opaque gray RGBA8,
 * gray = 255 - ink8 (ink dark); max_side 0 = full size. */
DrImage *dr_precision_doc_cmyk_plane_view(const DrPrecisionDocument *document, uint32_t plane,
    uint32_t max_side, uint64_t limit, uint64_t other_retained_bytes,
    const DrJobControl *control, char **err_out);
/* Record `index`'s own pixels through the view, RGBA8 fitted to max_side. */
DrImage *dr_precision_doc_cmyk_layer_thumbnail_rgba8(const DrPrecisionDocument *document,
    size_t index, uint32_t max_side, uint64_t limit, uint64_t other_retained_bytes,
    const DrJobControl *control, char **err_out);
/* Stable pixel identity of a CMYK record; 0 if none. */
uint64_t dr_precision_doc_cmyk_layer_identity(const DrPrecisionDocument *document, size_t index);
/* Image > Trim on a CMYK document (#1 phase 4b): the kept rectangle of the
 * composite ink, dr_image_trim_rect's bases and edge bits (a corner basis
 * compares all five codes; transparent = alpha 0). The composite's scratch is
 * admitted against limit beside other_retained_bytes first. True with
 * x, y, w, h in rect_out; false with *err_out NULL: every pixel is trimmable
 * (or the basis is unknown); false with *err_out set: refused. */
bool dr_precision_doc_cmyk_trim_rect(const DrPrecisionDocument *document, DrTrimBasis basis,
    uint32_t edges, uint64_t limit, uint64_t other_retained_bytes, uint32_t *rect_out,
    char **err_out);
/* request: {"format":"tiff|psd|jpeg_intermediate","embed_profile":bool,
 * "strip_metadata":bool}. Writes the composite flattened onto paper at the
 * declared depth (jpeg_intermediate: an 8-bit CMYK TIFF the host transcodes).
 * PSD: colour mode 4, four channels, no layer records, samples stored
 * inverted (max - ink), resources 1039/1005. TIFF (#1 1c-2a): the spot
 * channels follow the four inks as trailing extra samples (ExtraSamples 0),
 * each stored `max − coverage`, with resources 1006/1045/1077/1053 in tag
 * 34377; the plan adds their planes. The plan answers the convert plans'
 * shape. */
char *dr_precision_doc_save_cmyk_plan(const DrPrecisionDocument *document, const char *request,
    char **err_out);
bool dr_precision_doc_save_cmyk(const DrPrecisionDocument *document, const char *path,
    const char *request, uint64_t limit, uint64_t other_retained_bytes,
    const DrJobControl *control, char **err_out);
/* ---- CMYK selections (#1) ---- Select and Mask's New Layer on a CMYK document: the plan
 * answers the RGB cutout plan's keys with "foreground_storage":"cmyka16_unorm". decontaminate is
 * 0 (off) or a strength in (0, 1] (Decontaminate Colors on ink). The add puts the visible
 * composite's ink on a new top CMYK raster "Refined Cutout" masked by matte (width*height u16
 * coverage), hiding the other roots. NULL, no error: empty coverage. Free the plan with
 * dr_string_free and the document with dr_precision_doc_free. */
char *dr_precision_doc_cmyk_cutout_plan(const DrPrecisionDocument *document, float decontaminate,
    char **err_out);
DrPrecisionDocument *dr_precision_doc_add_cmyk_cutout(const DrPrecisionDocument *document,
    const uint16_t *matte, float decontaminate, uint64_t working_limit, uint64_t other_retained,
    const DrJobControl *control, char **err_out);   /* NULL, no error: empty coverage */
/* ---- CMYK documents (#1 phase 2) ---- One colour across the CMYK boundary (Color Picker,
 * eyedropper, MCP ink): {"from":"srgb","values":[r,g,b]} -> {"ink":[c,m,y,k],"profile"} or
 * {"from":"ink","values":[c,m,y,k]} -> {"srgb":[r,g,b],"profile"}; 0..1; optional "intent",
 * "black_point_compensation" (defaults relative, true). sRGB is the built-in profile. document: a
 * CMYK document's own profile; NULL or RGB: the working CMYK profile, refused by name when unset.
 * Free with dr_string_free. */
char *dr_cmyk_color(const DrPrecisionDocument *document, const char *request, char **err_out);
/* ---- CMYK documents (#1's artwork batch): type, shape and fill layers in ink ----
 * Assemble ink components (coverage planes component-major u16; intrinsic RGBA8 sRGB converted
 * through the document's CMYK profile, relative + BPC) into a CMYKA16 raster and publish it with
 * description: replacing rewrites layer (under its Pixels lock, mask kept); else a NEW leaf at
 * position (0 above, 1 top, 2 into_group) relative to anchor `layer`. NULL, no error: no change.
 * components: [{"kind":"coverage","ink":[c,m,y,k,a]},{"kind":"intrinsic_rgba8","opacity":o},
 * {"kind":"fill","ink":[c,m,y,k,a]},{"kind":"gradient","gradient":{GradientFill},
 * "inks":[[c,m,y,k],...],"box":[x,y,w,h] (optional, as dr_artwork_render's)},
 * {"kind":"pattern","fill":{pattern fill keys}}], 0..1; a pattern's
 * texels are separated sRGB -> ink at render, as a Pattern Overlay's. components and
 * description are each at most 16 MiB (a large custom tile); the component count and samples
 * are bounded as dr_artwork_render's; sample_count and
 * intrinsic_count must be exactly what the list names over width*height. RGB artwork
 * (dr_precision_doc_artwork/_at) on a CMYK document refuses "RGB artwork is unavailable in a CMYK
 * document". Free with dr_precision_doc_free. */
DrPrecisionDocument *dr_precision_doc_cmyk_artwork(const DrPrecisionDocument *document, size_t layer,
    bool replacing, uint32_t position, uint32_t width, uint32_t height, const char *components,
    const uint16_t *samples, size_t sample_count, const uint8_t *intrinsic, size_t intrinsic_count,
    int32_t x, int32_t y, const char *name, const char *description, uint64_t limit, uint64_t other,
    const DrJobControl *control, char **err_out);
/* request {"values":[[...],...] (<=1024), "intent", "black_point_compensation"} (dr_cmyk_color's
 * keys and defaults); a NULL/0-length profile end is sRGB [r,g,b], a CMYK profile end [c,m,y,k],
 * 0..1. Answers {"values":[[...]]}: sRGB->ink srgb_to_ink, ink->sRGB ink_to_srgb, ink->ink
 * CmykToCmyk (identical bytes: the values verbatim). Both ends sRGB, or a non-CMYK profile,
 * refuses by name. Free with dr_string_free. */
char *dr_cmyk_artwork_colors(const uint8_t *source_icc, size_t source_len, const uint8_t *destination_icc,
    size_t destination_len, const char *request, char **err_out);
/* ---- CMYK documents (#1 phase 5) ---- icc NULL = the working CMYK profile; plans
 * {"working_memory_plan_bytes","refusal","flatten_required"}; request {"op":"assign"|"convert"|
 * "depth","intent","black_point_compensation","flatten","depth":8|16} (intent/BPC default relative,
 * true; flatten convert only; depth op depth only). Assign keeps every sample; convert re-separates
 * each raster through the destination (masks, records, alpha kept; flatten:true flattens on paper
 * first); depth 16->8 rounds every raster and mask sample to round(v/257)*257, 8->16 changes the
 * declared depth only. An edit that changes nothing returns NULL with no error. */
char *dr_precision_doc_cmyk_profile_plan(const DrPrecisionDocument *document, const uint8_t *icc,
    size_t icc_len, const char *request, char **err_out);
DrPrecisionDocument *dr_precision_doc_cmyk_profile_edit(const DrPrecisionDocument *document,
    const uint8_t *icc, size_t icc_len, const char *request, uint64_t limit,
    uint64_t other_retained_bytes, const DrJobControl *control, char **err_out);
/* File > New in CMYK: one record filled with request {"depth":8|16,"fill":[c,m,y,k,a] 0..1,
 * "layer_name","ppi"} (16, transparent, "Background", 72 when absent). */
DrPrecisionDocument *dr_precision_doc_new_cmyk(uint32_t width, uint32_t height, const uint8_t *icc,
    size_t icc_len, const char *request, uint64_t limit, uint64_t other_retained_bytes,
    char **err_out);
/* The CMYK document's soft proof; setup is the PROOF SETUP JSON (preserve_numbers, plate). The proof
 * profile must be CMYK; gamut_warning refuses. Plan {"working_memory_plan_bytes","refusal"}. */
char *dr_precision_doc_cmyk_proof_plan(const DrPrecisionDocument *document, const uint8_t *proof_icc,
    size_t proof_len, const char *setup_json, char **err_out);
/* RGBA8 in the built-in Display P3; info {"space":"display-p3","out_of_gamut":0}. A plate is the
 * proof device's ink of the composite on paper, grey 255 - round(255*ink), opaque. */
DrImage *dr_precision_doc_cmyk_proof(const DrPrecisionDocument *document, const uint8_t *proof_icc,
    size_t proof_len, const char *setup_json, uint64_t limit, uint64_t other,
    const DrJobControl *control, char **info_out, char **err_out);
/* request {"source":"layer"|"backdrop"|"composite","layer","below","auto":{"algorithm":
 *   "per_channel"|"monochromatic","shadows","highlights"} (percent, 0.1 default),
 *   "eyedropper":{"kind":"black"|"gray"|"white","x","y","reach","intent","black_point_compensation"}}
 * -> {"histogram":{"cmyk","c","m","y","k"} (256 bins of L = 1 - ink, pixel units; cmyk = the four
 * summed), "auto":{"black":[4],"white":[4],"algorithm"} (L fractions) | {"empty":true},
 * "eyedropper":{"sample":[c,m,y,k],"target":[c,m,y,k]}}; coverage NULL/0 = none, else canvas-sized.
 * Plan {"working_memory_plan_bytes","refusal"}. */
char *dr_precision_doc_cmyk_tone_analysis_plan(const DrPrecisionDocument *document,
    const char *request, char **err_out);
char *dr_precision_doc_cmyk_tone_analysis(const DrPrecisionDocument *document, const char *request,
    const uint16_t *coverage, size_t coverage_len, uint64_t limit, uint64_t other,
    const DrJobControl *control, char **err_out);
/* request {"mode":"shadows"|"highlights","plates":["c","m","y","k"]} over this (candidate)
 * document's composite on paper: opaque RGBA8, highlights white where every listed plate is 0 ink,
 * shadows black where any listed plate is 100 %. */
DrImage *dr_precision_doc_cmyk_clip_display(const DrPrecisionDocument *document, const char *request,
    uint64_t limit, uint64_t other, const DrJobControl *control, char **err_out);
/* cmyk_document NULL or RGB = working CMYK (refused by name when unset); rgb_icc NULL = sRGB;
 * request {"values":[r,g,b] 0..1,"intent","black_point_compensation"} (relative, true)
 *   -> {"ink":[c,m,y,k],"out_of_gamut":bool,"delta_e":x,"in_gamut":[r,g,b],"profile"}. */
char *dr_cmyk_separate(const DrPrecisionDocument *cmyk_document, const uint8_t *rgb_icc,
    size_t rgb_icc_len, const char *request, char **err_out);
/* A Photoshop .alv (records 0-4: composite, c, m, y, k, L domain) or .acv (curves 1-5 -> cmyk, c, m,
 * y, k ink points by (255 - in, 255 - out); fewer than five curves is an RGB preset, refused) for a
 * CMYK document -> {"op","params"}. Every other extension refuses by name. */
char *dr_adjustment_preset_file_decode_cmyk(const uint8_t *bytes, size_t len, const char *extension,
    char **err_out);
/* Owned JSON: dimensions, explicit storage/evaluation, resource plans/support.
 * Each layer row carries, beside index/depth/subtree_start/id/name/kind/
 * blend_mode/opacity/visible/clipped/offset/label/blend_if/style/mask, the
 * "locks" u32 (a DrLockFlags bitmask, 0 = unlocked) and the "link" u32
 * (the link-group id, 0 = unlinked) — the typed twins of dr_doc_layer_locks
 * and dr_doc_layer_link. "global_light" is {angle,altitude} in degrees at
 * every depth (the default 120/30 on a float document). */
char *dr_precision_doc_info(const DrPrecisionDocument *document, char **err_out);
/* Compact owned storage/evaluation/resource JSON, without layer names or
 * descriptions. Captures/archives use this instead of materializing layer info. */
char *dr_precision_doc_descriptor(const DrPrecisionDocument *document, char **err_out);
/* Cloned raster source owner (shared pixels); groups/invalid indices give NULL.
 * Indices are bottom-first postorder, including group records. */
DrPrecisionImage *dr_precision_doc_layer_source(const DrPrecisionDocument *document, size_t index);
DrPrecisionImage *dr_precision_doc_render(const DrPrecisionDocument *document,
    uint64_t limit, uint64_t other_retained_bytes, const DrJobControl *control, char **err_out);
/* Record `index`'s raw pixels on a transparent CANVAS-sized typed image in the
 * document's render storage (RGBA16, or RGBA32F for a float document) — the
 * typed twin of dr_doc_layer_canvas_image, with the raw-layer scope of
 * dr_precision_doc_scoped_analysis: a raster's exact samples at its offset
 * (zeros where it does not reach); a group's RAW projection, the rule Copy
 * of a group uses at every depth: its children composited among themselves
 * without the group's own opacity, blend mode, mask, style or Blend If,
 * visible, a Pass Through group as isolated. An adjustment, an
 * out-of-range index, scratch past limit and a cancel refuse by name. NULL
 * document -> NULL with "required handle is NULL" in err_out, as
 * dr_precision_doc_render. Display goes through
 * dr_precision_image_preview_rgba8 (u16) or
 * dr_precision_doc_display_rgba8 (float). Free with dr_precision_image_free. */
DrPrecisionImage *dr_precision_doc_layer_canvas_image(const DrPrecisionDocument *document,
    size_t index, uint64_t limit, uint64_t other_retained_bytes,
    const DrJobControl *control, char **err_out);
/* The typed twin of dr_doc_render_region
 * (Phase 13A-1; masks, clip units and Blend If since 13A-2a; groups since
 * 13A-3a; layer styles since 13A-3b-i; group styles with effects since
 * 13A-3b-ii; knockout and grouped interiors since 13A-3c). The reference is
 * dr_precision_doc_render: the same admitted set
 * (groups included; every typed raster and isolated-group style — a group with
 * an enabled effect is materialized as render_styled does, into a canvas-sized
 * buffer before the loop; a knockout and a grouped interior take the typed
 * unit pass per sample, the Background from the scene's own seed), refusal
 * order, sampling,
 * preview-only divisor and 4096-sample checkpoint cadence, in the document's
 * render storage (RGBA16, or RGBA32F for a float document). Layer indices are
 * postorder; typed Blend If is admitted on a lone raster or isolated group, a
 * clip base and a member; clip units are the scene builder's (members are the
 * run's VISIBLE records, so a run of only hidden members is empty here, unlike
 * 8 bits). The evaluation policy's refusal (11E linear light, float) is
 * raised FIRST by the render. The render is
 * admitted against limit / other_retained_bytes like the full render. NULL
 * document -> NULL, no error. Free the image with dr_precision_image_free. */
DrPrecisionImage *dr_precision_doc_render_region(const DrPrecisionDocument *document,
    uint32_t x, uint32_t y, uint32_t w, uint32_t h, uint32_t preview_divisor,
    uint64_t limit, uint64_t other_retained_bytes, const DrJobControl *control,
    char **err_out);
/* Pure neutral edits. Unchanged opacity returns NULL without an error.
 * duplicate_layer matches the byte dr_doc_duplicate_layers: the copy (and every
 * record of a copied group) is unlinked, and only the copied root gets " copy".
 * An Indexed Color document refuses it ("duplicate_layer is not available in
 * Indexed Color."). */
DrPrecisionDocument *dr_precision_doc_duplicate_layer(const DrPrecisionDocument *document,
    size_t index, char **err_out);
DrPrecisionDocument *dr_precision_doc_set_layer_opacity(const DrPrecisionDocument *document,
    size_t index, float opacity, char **err_out);
/* Internal tone/color edits. The shared adjustment meta schema requires an
 * top-level evaluation:"precision_v1"; unsupported operation/target/evaluation
 * combinations refuse. Live adjustments keep float intermediates; destructive
 * application targets one raster source, rounds RGB once to u16 and keeps alpha
 * (a 32-bit float source keeps f32 under the layer's float form).
 * Pure edits return NULL without error only when unchanged.
 * On a CMYK document (#1 phase 2) the plan, add, add_at, set and apply take
 * op "levels", "curves" or "hue_saturation" on ink: Levels on L = 1 - ink
 * (top-level keys the composite stage, optional "c"/"m"/"y"/"k" stage
 * objects), Curves points in ink 0-255 (keys "cmyk", "c", "m", "y", "k"),
 * Hue/Saturation with the RGB op's own params, each ink going ink -> PCS ->
 * Adobe RGB through the document's profile and back with K re-separated by
 * the profile (a pixel the RGB kernel leaves unchanged keeps its ink; a
 * profile with no B2A table refuses it); a record composites as
 * lerp(ink, f(ink), opacity x mask), alpha kept; a map curve and the RGB
 * keys refuse by name. Since the adjustments batch also, on L = 1 - ink:
 * "invert", "posterize", "bcs" (no saturation) and "shadows_highlights" per
 * plate (its guide the ink tone luma709(L_c, L_m, L_y) x L_k); "color_balance",
 * "photo_filter" ("ink" in place of "color") and "color_lookup" on the CMY
 * triple with K held; and natively "channel_mixer" (rows cyan/magenta/yellow/
 * black/gray of {c,m,y,k,constant} on the ink), "selective_color", "threshold",
 * "gradient_map" (stops {position, ink, opacity}) and "replace_color" ("inks").
 * "shadows_highlights" and "replace_color" apply only (an add or set refuses
 * by name); exposure, vibrance and black_and_white refuse (unavailable in
 * CMYK), hue_rotate, grayscale, sepia and white_balance refuse (RGB
 * only), and any other op refuses as "The \"<op>\" adjustment is unavailable
 * in a CMYK document". */
/* Grayscale (#1 phase 2b): on a Gray document these five exports run the RGB
 * kernel on R = G = B: bcs, levels, curves, exposure, invert, posterize,
 * threshold and gradient_map as a layer or destructively, shadows_highlights
 * and replace_color destructively only (a layer refuses "A <Name> adjustment
 * layer is unavailable in a Grayscale document: <Name> applies to
 * pixels only."). Levels' r/g/b and Curves' r/g/b and r_map/g_map/b_map must be
 * all absent or all equal ("<Name> in a Grayscale document has one channel,
 * Gray: ..."); a Gradient Map's stop colours are stored greyed to their
 * luminance. vibrance, hue_saturation, color_balance, black_and_white,
 * photo_filter, channel_mixer, color_lookup and selective_color refuse "<Name>
 * is unavailable in a Grayscale document."; hue_rotate,
 * grayscale, sepia and white_balance "... Doctored offers it on RGB documents
 * only."; any other op refuses "unsupported high-depth adjustment <op>", as on
 * RGB. The plan (destructive false = a layer)
 * refuses alike. */
/* Lab Color (#1 phases 2b to 2d): on a Lab document these five exports run
 * the RGB kernel in a Lab form bound at parse: levels and curves on the
 * composite and bcs act on Lightness (the kernel on L, L, L; a* and b* kept),
 * their r, g and b settings on Lightness, a and b (r under the composite; g
 * and b by a kernel of the channel stages alone on L, a, b); a bcs saturation
 * scales a* and b* about 0; invert and posterize act on every channel,
 * threshold on L with a* = b* = 0; hue_saturation of master saturation -1
 * alone (Desaturate) keeps L* and sets a* = b* = 0; hue_saturation,
 * color_balance, photo_filter, shadows_highlights and replace_color run on the
 * sample's colour in the built-in 16-bit ProPhoto RGB, color_lookup and
 * gradient_map in sRGB (phase 2d; a sample the op leaves alone keeps its
 * codes, dr_lab_kernel_rgb answers what the kernel reads); photo_filter's
 * color is sRGB, replace_color's colors the document's own 8-bit #LLaabb Lab
 * codes; as a layer or destructively, except
 * shadows_highlights and replace_color, which as a layer refuse "A <Name>
 * adjustment layer is unavailable in a Lab Color document: <Name> applies
 * to pixels only.". exposure, vibrance, black_and_white, channel_mixer
 * and selective_color refuse "<Name> is unavailable in a Lab Color
 * document."; hue_rotate, grayscale, sepia and white_balance "...
 * Doctored offers it on RGB documents only."; any other op answers as on RGB
 * ("unsupported high-depth adjustment <op>"). The plan refuses alike. */
/* Owned JSON retained_bytes/scratch_bytes. index=-1 append, nonnegative index
 * selects an existing adjustment for replacement or a raster for destructive
 * application. Planning does not build a LUT/guide or perform pixel work. */
char *dr_precision_doc_adjustment_plan(const DrPrecisionDocument *document,
    int64_t index, const char *description, bool destructive, char **err_out);
DrPrecisionDocument *dr_precision_doc_add_adjustment(const DrPrecisionDocument *document,
    const char *name, const char *description, float opacity, uint64_t limit,
    uint64_t other_retained_bytes, const DrJobControl *control, char **err_out);
/* dr_precision_doc_add_adjustment at a chosen stack position — the same
 * admission, job and refusal contract, with DrInsertPosition's slot rule
 * ("Stack operations" above) deciding where the record lands. The new record
 * JOINS a clip run when the next sibling at its level is clipped, so no
 * neighbour is re-based; insertion is lock-EXEMPT like every other structure
 * edit. A refused position reports through err_out before any operator table
 * is prepared. */
DrPrecisionDocument *dr_precision_doc_add_adjustment_at(const DrPrecisionDocument *document,
    const char *name, const char *description, float opacity, size_t anchor,
    uint32_t position, uint64_t limit, uint64_t other_retained_bytes,
    const DrJobControl *control, char **err_out);
DrPrecisionDocument *dr_precision_doc_set_adjustment(const DrPrecisionDocument *document,
    size_t index, const char *description, uint64_t limit, uint64_t other_retained_bytes,
    const DrJobControl *control, char **err_out);
DrPrecisionDocument *dr_precision_doc_apply_adjustment(const DrPrecisionDocument *document,
    size_t index, const char *description, uint64_t limit, uint64_t other_retained_bytes,
    const DrJobControl *control, char **err_out);
/* RGB ICC input is borrowed for the call, bounded by the existing 16MiB cap.
 * Assign preserves all sample codes. Convert transforms stored RGB, including
 * hidden RGB, with exact alpha. Live adjustment parameters are unchanged and
 * are evaluated in the destination context; nonlinear stacks may change their
 * composite appearance. Exact ICC-byte identity is a no-op; legacy approximate
 * space equality never skips a high-depth conversion. Optional control is
 * single-use, cooperatively polled; failed/cancelled edits preserve the source.
 * color_plan is owned JSON with retained_bytes and scratch_bytes, including the
 * borrowed ICC input and conservative preparation overhead. Assign refuses
 * with dr_precision_doc_assign_refusal's sentence while a layer holds a
 * retained source. An assign plan (convert false) never refuses for a retained
 * source; it refuses a destination whose matrix/TRC tone curves
 * fail the precision check only for a linear-light document, one holding
 * text/shape artwork or one holding a calibrated (document_v2)
 * Exposure/White Balance adjustment, grouped ones included, in Assign
 * words naming the profile, and Assign and the rasterizing Assign answer the
 * same sentence.
 * A 32-BIT FLOAT document takes color_plan, assign_profile and
 * convert_to_profile on its linear samples: Assign keeps every f32 sample bit
 * for bit on the same storage and relabels it; Convert applies the matrix
 * inv(destination) * source between the two profiles' primaries to linear RGB
 * in f64, rounded once to f32, alpha kept, with NO clamp (a colour outside the
 * destination gamut keeps a negative component). Both keep the tone map. The
 * plan, the Assign and the Convert refuse by name a profile with no matrix/TRC
 * model ("Doctored cannot assign “name” to a 32-bit float document — it has
 * no matrix/TRC model, ..." / "Doctored cannot convert a 32-bit float document
 * into “name” — ..."); Assign refuses one whose tone curves fail the precision
 * check ("... and a float document's display and 8/16-bit export encode
 * through them. Choose another profile."), Convert with the convert_refusal
 * sentence; a converted component outside ±65504 refuses ("the document's
 * colours convert to a value outside the 32-bit float range (±65504) in
 * “name”; the conversion is refused rather than clipped"). A float Convert
 * plan's scratch is the converted planes at 16 B/px plus the largest once
 * more, plus a quarter. */
char *dr_precision_doc_color_plan(const DrPrecisionDocument *document,
    const uint8_t *icc, size_t length, bool convert, uint64_t limit,
    uint64_t other_retained_bytes, char **err_out);
/* The admission plan of dr_precision_doc_convert_to_profile_with under
 * options_json (NULL = the defaults), as owned JSON in color_plan's shape
 * (retained_bytes, scratch_bytes, converted_pixel_bytes), with the same
 * borrowed-ICC allowance. A preserve-appearance Convert that bakes adjustments
 * also charges the bakes and the baked layers; otherwise the figures
 * equal the convert color_plan's. Free with dr_string_free. */
char *dr_precision_doc_conversion_plan(const DrPrecisionDocument *document,
    const uint8_t *icc, size_t length, const char *options_json, uint64_t limit,
    uint64_t other_retained_bytes, char **err_out);
/* A Lab Color document (#1 Lab phase 5b) refuses Assign and Convert: "a Lab
 * Color document has no document profile: it converts through Image ▸ Mode
 * (dr_precision_doc_convert_lab_to_rgb, dr_precision_doc_convert_to_cmyk,
 * dr_precision_doc_convert_to_gray)".
 * An Indexed Color document (#1 Indexed phase 3a) takes an RGB profile only
 * ("An Indexed Color document's colour table is RGB: assign or convert to an
 * RGB profile, or change the mode with Image ▸ Mode."): Assign keeps every
 * index, the table's codes and every sample; Convert converts each colour
 * table entry (the RGB conversion, read at 8-bit codes) and keeps every index
 * and the transparent index. Its color_plan is the storage's assign plan,
 * plus the Color Table edit's re-render for a Convert. */
DrPrecisionDocument *dr_precision_doc_assign_profile(const DrPrecisionDocument *document,
    const uint8_t *icc, size_t length, uint64_t limit, uint64_t other_retained_bytes,
    const DrJobControl *control, char **err_out);
DrPrecisionDocument *dr_precision_doc_convert_to_profile(const DrPrecisionDocument *document,
    const uint8_t *icc, size_t length, uint64_t limit, uint64_t other_retained_bytes,
    const DrJobControl *control, char **err_out);
/* True when the typed document's profile is an RGB matrix/TRC model passing the high-depth
 * precision checks: the precondition of document_v2 calibrated Exposure/White Balance and of
 * a high-depth conversion's DESTINATION (a source that fails them still converts out).
 * False on a NULL document and for a LUT or other unmodelled profile. Typed twin of
 * dr_doc_profile_is_convertible. */
bool dr_precision_doc_profile_is_convertible(const DrPrecisionDocument *document);
/* The typed twin of dr_doc_calibrated_refusal; the same sentences for the same profile
 * and op. */
char *dr_precision_doc_calibrated_refusal(const DrPrecisionDocument *document,
    const char *op);
bool dr_precision_doc_save_native(const DrPrecisionDocument *document, const char *path,
    uint64_t limit, uint64_t other_retained_bytes, const DrJobControl *control, char **err_out);
/* format0 PNG16, format1 TIFF16. Render captured source and atomically write.
 * Compound export is noncancellable. Optional report_out receives existing
 * DR_CARRIES_* bits actually written only on success; errors leave it untouched. */
bool dr_precision_doc_export16(const DrPrecisionDocument *document, const char *path,
    uint32_t format, bool embed_profile, bool strip_metadata, uint64_t limit,
    uint64_t other_retained_bytes, uint32_t *report_out, char **err_out);
/* Reuse a captured authoritative u16 projection. Both inputs must remain live;
 * storage must be RGBA16 UNORM, dimensions equal the canvas, ICC bytes exact.
 * The host guarantees the projection belongs to this captured source/revision.
 * Owned plan JSON (dr_string_free): retained_bytes is the union of BOTH inputs;
 * host_scratch_bytes includes codec_scratch_bytes, codec_additive_overhead_bytes
 * and bookkeeping. Also width, height, storage. No pixel allocation/render.
 * Save format 0 PNG16 / 1 TIFF16; path UTF-8 <=64 KiB. other_retained_bytes
 * excludes BOTH inputs' union. Atomic/noncancellable, same packet policy as
 * export16. report_out receives DR_CARRIES_* only on success. NULL/error safe. */
char *dr_precision_doc_projection_export_plan(const DrPrecisionDocument *document,
    const DrPrecisionImage *image, char **err_out);
bool dr_precision_doc_save_projection16(const DrPrecisionDocument *document,
    const DrPrecisionImage *image, const char *path, uint32_t format,
    bool embed_profile, bool strip_metadata, uint64_t limit,
    uint64_t other_retained_bytes, uint32_t *report_out, char **err_out);
void dr_precision_image_free(DrPrecisionImage *image);
uint32_t dr_precision_image_width(const DrPrecisionImage *image);
uint32_t dr_precision_image_height(const DrPrecisionImage *image);
/* dr_image_trim_rect over a 16-bit or float image. */
bool dr_precision_image_trim_rect(const DrPrecisionImage *image, DrTrimBasis basis,
                                  uint32_t edges, uint32_t *rect_out);
/* Borrowed native-endian width*height*4 u16 elements, row0 top; NULL for NULL
 * and when the view cannot be allocated.
 * Pointer valid only while the immutable image owner remains alive. */
const uint16_t *dr_precision_image_pixels_rgba16(const DrPrecisionImage *image);
/* Opaque identity of the image's pixel STORAGE (its tile index, or its one
 * contiguous buffer). Two images answering the same nonzero value share
 * that storage: every sample is shared and none was copied (a clone,
 * dr_precision_doc_layer_source, an integer move). A new construction or a
 * changed sample answers another value. Compare only while both storages
 * are owned; never dereference. The pixels_rgba16 pointer is per-handle
 * and is not an identity. 0 for NULL. Reads no pixel, allocates nothing. */
uint64_t dr_precision_image_storage_identity(const DrPrecisionImage *image);
/* The typed twin of dr_doc_layer_mask_storage_identity for record idx (the
 * postorder numbering of every dr_precision_doc_* index): the identity of
 * the record's raw u16 mask plane, with the same contract. 0 for NULL, an
 * out-of-range index, a record with no mask or a group. */
uint64_t dr_precision_doc_layer_mask_storage_identity(const DrPrecisionDocument *document,
    size_t idx);
/* Borrowed ICC; optional length_out, zero/NULL for NULL image. */
const uint8_t *dr_precision_image_profile(const DrPrecisionImage *image, size_t *length_out);
/* Explicit lossy preview, released with dr_image_free. Profile is carried by
 * the source separately. This value cannot enter typed save/source APIs. */
DrImage *dr_precision_image_preview_rgba8(const DrPrecisionImage *image,
    uint64_t limit, uint64_t other_retained_bytes, const DrJobControl *control, char **err_out);
/* Layers-panel thumbnail of a u16 image as RGBA8 (no dither), released with
 * dr_image_free; no ICC (the source carries it). Aspect-fit with the longest
 * side max_side, NEVER upscaled (max_side 0 or a smaller image keeps its
 * size; the per-axis rounding is the host's former fit,
 * round(side * (max_side / longest))). Below 16x the thumbnail per axis it is
 * byte for byte dr_precision_image_preview_rgba8 followed by dr_image_resize
 * (FILTER_BILINEAR); a larger image is box-reduced out of its tiles at u16,
 * resized Bilinear at u16 and quantized once, measured within 2/255 of that pair,
 * with no whole-layer copy. Refuses f32 storage with the preview's sentence,
 * scratch past limit, and a cancel (the box pass polls every 4096 samples).
 * control is single-use. */
DrImage *dr_precision_image_thumbnail_rgba8(const DrPrecisionImage *image, uint32_t max_side,
    uint64_t limit, uint64_t other_retained_bytes, const DrJobControl *control, char **err_out);
/* Record idx's raw u16 MASK plane as a Layers-panel thumbnail:
 * opaque gray RGBA8 (r == g == b, alpha 255), released with dr_image_free;
 * no ICC. The raw plane at its stored size (no density, feather or enabled
 * flag). Aspect-fit with the longest side max_side, NEVER upscaled (the same
 * fit as dr_precision_image_thumbnail_rgba8). Each code reads as
 * (v + 128) / 257; below 16x the thumbnail per axis the result is byte for
 * byte that per-sample gray RGBA image followed by dr_image_resize
 * (FILTER_BILINEAR); a larger plane is box-reduced first, measured within
 * 2/255 of that pair, with no whole-plane image. Refuses an out-of-range
 * index ("layer index is out of range"), a record with no mask ("high-depth
 * target has no mask"), scratch past limit and a cancel. control is
 * single-use. A GROUP's mask (isolated or pass-through) answers its CANVAS
 * view instead — exactly dr_precision_doc_layer_mask_canvas_thumbnail_rgba8,
 * the picture dr_doc_layer_mask_thumbnail shows for a byte group;
 * every other record the raw plane at its stored size. */
DrImage *dr_precision_doc_layer_mask_thumbnail_rgba8(const DrPrecisionDocument *document,
    size_t idx, uint32_t max_side, uint64_t limit, uint64_t other_retained_bytes,
    const DrJobControl *control, char **err_out);
/* The 16-bit Channels panel's row thumbnails, opaque gray RGBA8 over
 * the CANVAS, released with dr_image_free; no ICC. ..._channel_ reads alpha
 * channel idx; ..._layer_mask_canvas_ reads record idx's mask in its canvas
 * view (the raw plane at its offset, its outside coverage beyond it; no
 * density, feather or enabled flag), i.e. dr_precision_doc_read_plane with
 * canvas true. Aspect-fit with the longest side max_side, NEVER upscaled
 * (0 = full size); each code reads as (v + 128) / 257. Below 16x the
 * thumbnail per axis (the Layers panel's guard; the 8-bit Channels plane
 * thumbnails' 4x guard measured 3 codes on a moved mask) the result is byte
 * for byte that gray canvas image followed by dr_image_resize
 * (FILTER_BILINEAR); a larger plane is box-reduced first, measured within
 * 2/255 of that pair, with no canvas-sized copy. Refuses an out-of-range index ("high-depth
 * channel index is out of range" / "layer index is out of range"), a record
 * with no mask ("high-depth target has no mask"), scratch past limit and a
 * cancel; a NULL document answers "required handle is NULL". control is
 * single-use. */
DrImage *dr_precision_doc_channel_thumbnail_rgba8(const DrPrecisionDocument *document,
    size_t idx, uint32_t max_side, uint64_t limit, uint64_t other_retained_bytes,
    const DrJobControl *control, char **err_out);
DrImage *dr_precision_doc_layer_mask_canvas_thumbnail_rgba8(const DrPrecisionDocument *document,
    size_t idx, uint32_t max_side, uint64_t limit, uint64_t other_retained_bytes,
    const DrJobControl *control, char **err_out);
/* Owned JSON union of all live typed source/projection/captured handles. Arrays
 * may contain NULL entries; NULL arrays require count0, counts <=65536. */
char *dr_precision_resources_report(const DrPrecisionDocument *const *documents,
    size_t document_count, const DrPrecisionImage *const *images, size_t image_count,
    char **err_out);

/* Internal 7D edits. Strict UTF-8 JSON <=64KiB; schemas are documented in
 * precision_geometry.rs, precision_mask_edit.rs, precision_channel_edit.rs,
 * precision_scalar_edit.rs, precision_selection.rs and precision_paint.rs.
 * Unknown operations/fields/targets refuse. All colors are straight normalized
 * continuous RGB with explicit color_space (document or srgb; paint_blend and
 * stroke_layer_edge in a 32-bit float document also take linear: RGB is
 * linear light in the document's primaries, used as is, finite in [0, 65504],
 * and every other op and depth refuses it). Coverage is
 * native-endian u16, row0 top; byte coverage may be explicitly promoted by257,
 * retaining only256levels. No colored byte buffer can enter these operations.
 * Plan is owned JSON: retained_bytes, scratch_bytes, input_samples, and for a
 * nonempty input its input_width,input_height,input_planes. Read-plane
 * plans also provide output_samples,width,height. A geometry request's plan
 * also provides work_units, the pixel work the interactive and one-shot
 * limits are compared with (the host sizes a reduced preview from it). Scratch includes borrowed
 * input/caller output and all working buffers. Free JSON with dr_string_free.
 * document/control must remain alive for the entire call; NULL is safe.
 * The mask edit set_mask takes an optional "outside_coverage" (0 default, or
 * 255: raw coverage outside the stored rectangle); any other value refuses.
 * The four transform ops "affine", "perspective", "affine_set" and
 * "perspective_set" take an optional "interactive":true, sent by the
 * 16-bit transform previews only (the Move preview's integer affine_set does
 * no pixel work and omits it), which keeps the interactive pixel-work
 * limit (the only work refusal a geometry plan carries: the host
 * answers it with a reduced preview or by running the commit request as a
 * cancellable preview job); without it a transform COMMIT is bounded by the
 * larger one-shot limit (the "straighten" block's), so a 24 MP layer
 * transforms at "bicubic". A placed image (a retained source) re-rendered by
 * a commit or a filtered turn follows the same limit. The one-shot limit
 * binds only a run with NULL control: the plan carries no work refusal for a
 * commit, and a controlled commit has no work ceiling; that holds for EVERY
 * request without "interactive" (resize, crop, canvas_size and the exact
 * turns included) and for every other edit this export plans and
 * dr_precision_doc_edit runs (masks, adjustments, paint, selection sources,
 * warp, colour). The byte request ignores the key. */
char *dr_precision_doc_edit_plan(const DrPrecisionDocument *document, const char *request,
    uint64_t limit, uint64_t other_retained_bytes, char **err_out);
/* Additional channel edits: rename_channel(index,name), duplicate_channel(index),
 * channel_properties(index, optional name/overlay_color RGB8/overlay_opacity/
 * color_indicates_selected/provenance JSON), import_channel(name,width,height,
 * optional provenance JSON). Imported source-sized u16 coverage resizes through
 * shared bilinear geometry; canvas dimensions come from the captured document.
 * Declared byte auxiliary coverage requires exact multiples of257. Version25
 * persists channel display metadata and strict camera-auxiliary provenance;
 * default-only channel documents retain their prior compatible native writer.
 * scalar_edit targets mask/channel/red/green/blue/alpha at index; operation is
 * fill/paint/bucket_fill/replace/filter/morphology. Target gray (#1 phase
 * 6b) is a Grayscale document's u16 layer, replace_canvas only, written to R,
 * G and B. Payload precedes optional
 * canvas selection (selection:true). Plan names payload_samples and raw width,
 * height,x,y; replace payload has raw dimensions and paint has canvas coverage.
 * Filter neighborhoods read raw storage; all other components remain exact.
 * In a 32-bit float RGB document a filter (not an adjustment or
 * Auto) on a mask, a channel or a float layer's red/green/blue/alpha runs the
 * float layer route on the plane read as opaque gray [v,v,v,1]: a colour plane
 * is linear light, unclamped, in the route's own domain; a mask, channel or
 * alpha is coverage (v = code/65535 or the stored alpha), never encoded, its
 * result clamped to 0..1; the result is r when r==g==b bit for bit, else the
 * Rec.709 luma. Add Noise on coverage is the 16-bit mask's additive draw.
 * liquify, depth_blur and develop_filter refuse by name on every plane, in plan
 * and edit (float_plane_filter.rs). */
/* Native layer blending: {op:"layer_blend",version:1,layer,blend_mode}.
 * blend_mode uses DrBlendMode integers: Normal0, Multiply1, Screen2; PassThrough27
 * applies only to groups. Adjustments require Normal. Unknown version/mode/fields
 * and incompatible records refuse in both plan and edit. No input samples.
 * Encoded document RGB uses shared W3C f32 source-over, mask/opacity once per
 * boundary and one final u16 store. Native26 preserves non-Normal modes; older
 * writers remain for default modes. Existing byte ABI and paint modes unchanged. */
/* Native clipping: {op:"layer_clipped",layer,clipped} on raster/group/adjustment records; no version, no samples; exact no-op is NULL; clip units render with the byte clip-group rule and conditional native32/LCLP persists the flag. */
/* Native colour labels: {op:"layer_label",layer,label} with label 0..DR_LAYER_LABEL_MAX on raster/group/adjustment records; no version, no samples; exact no-op is NULL; the "label" info row reads it and conditional native42/LLBL persists it. */
/* Native group disclosure: {op:"layer_open",layer,open} on a GROUP record only (any other record refuses by name); lock-exempt; no version, no samples; exact no-op is NULL; the "open" info row reads it (true on every non-group row) and conditional native55/LOPN persists a closed group. */
/* Native global light: {op:"global_light",angle,altitude} (degrees, both required, unknown fields refuse) — the dr_doc_set_global_light twin, sanitized by the same rule; document-level, no version, no samples; exact no-op (unchanged after sanitizing, or a non-finite component) is NULL; at every depth, 32-bit float included; the "global_light" info object reads it, every style effect with use_global_light renders with it, and conditional native58/LGLT persists a non-default light. */
/* Native structural move: {op:"move_layer_to",from,to,depth} — `from`'s whole subtree lands at `to`, an index into the record list with that subtree ALREADY taken out, at nesting level `depth`; no samples; the block landing back where it began at its own depth is the exact no-op and answers NULL. */
/* Native layer offset: {op:"affine_set",layers:[layer],matrix:[1,0,0,1,dx,dy],filter:"nearest",follow_links:false} — follow_links (affine_set only, default true) false makes the set's membership and its position-lock gate the root's own subtree, never its link partners, so a raster moves alone and a group carries its subtree, exactly dr_doc_with_layer_offset's rule; link ids are untouched; no samples; a zero delta is the exact no-op and answers NULL. */
/* Native align and distribute: {op:"align_set",layers:[..],edge:"left|horizontal_center|right|top|vertical_center|bottom",to_canvas:bool} and {op:"distribute_set",layers:[..],vertical:bool} — dr_doc_align_layers / dr_doc_distribute_layers at 16-bit and 32-bit float with the byte rules: the given entries reduced to independent roots, each root's CONTENT box (dr_precision_doc_layer_bounds; nothing opaque is skipped), one integer delta per root, a subtree and link group come along and the last root to claim an entry wins; every translation lands in ONE edit with typed Move's budget and never resamples; a POSITION lock anywhere in the expanded set (a zero-delta entry and a link-only partner included) refuses by name before any delta; an empty or out-of-range set refuses; no samples; already aligned, fewer than two boxes (without to_canvas) or fewer than three to distribute is the exact no-op and answers NULL, and the plan still succeeds. */
/* Native sibling reorder: {op:"arrange_layer",layer,how} with how front|forward|backward|back; never leaves the record's own level, a group carries its subtree; no samples; a record already where `how` would put it is the exact no-op and answers NULL. */
/* Native subset grouping: {op:"group_layers",layers:[..],name,pass_through?=true} — any set sharing ONE parent, gathered into the topmost member's slot; the container mints an id and the members keep theirs; a 32-bit float document honours pass_through as every other depth does; the plan reply carries "report":{group,cleared_clip,reordered}; no samples. */
/* Native ungrouping: {op:"ungroup_layer",layer} on a GROUP record — its children take its level and its slot and the group's own mask, style, opacity, blend mode, Blend If and clipped flag are DISCARDED; the plan reply carries "report":{cleared_clip,landed}; no samples. */
/* Native set delete: {op:"remove_layers",layers:[..]} — records subsumed by another given subtree are dropped, and a call that would leave the document with no records refuses by name; no samples. */
/* Native duplicate to new document: {op:"duplicate_to_new_document",layers:[..]} — dr_doc_duplicate_to_new_document's rule at 16-bit and 32-bit float: the result is a NEW document (the source handle is unchanged) holding the independent roots' subtrees at the top level, fresh ids, the same canvas, resolution, profile, evaluation policy, float policy and global light, and no channels, guides, comps or metadata; an empty or out-of-range set refuses by name; no samples. */
/* Native ADD: {op:"add_layer",name,anchor,position} with position
 * "above"|"top"|"into_group" — ONE transparent canvas-sized leaf at (0,0), at
 * the slot DrInsertPosition names relative to anchor (dr_doc_adding_layer_at's
 * comment IS that rule). Storage follows the document: u16 zeros at 16 bits,
 * f32 zeros at 32-bit float, where this is the only admitted layer-creating op
 * (family "layer_add", target "document"). The new record JOINS a clip run when
 * the next sibling at its level is clipped, and insertion is lock-EXEMPT, into
 * a locked group included. Refusals are NAMED (a bad anchor, into_group on a
 * non-group, the nesting cap, native capacity); no samples. */
/* Native MERGES, all three through dr_precision_doc_edit and none taking samples:
 *   {"op":"merge_layers","layers":[..]}  Merge Layers over a multi-selection.
 *   {"op":"merge_down","layer":N}        Merge Down into the previous SIBLING.
 *   {"op":"merge_visible"}               Merge Visible.
 * Every RULE is the byte one (dr_doc_merge_layers, dr_doc_merging_down,
 * dr_doc_merge_visible): merge_layers needs at least two members sharing ONE
 * parent and a VISIBLE lowest member whose locks admit a merge, and the merged
 * record keeps that member's slot, name, id, clipped flag, locks, link, label
 * and level; merge_down refuses a hidden or lock-refusing LOWER record, simply
 * removes a hidden or EMPTY UPPER one (a group with nothing visible inside),
 * and bakes a clipped upper record through its clipping; merge_visible needs two CONTRIBUTING raster leaves and lands at
 * the bottom-most contributing root's slot with that leaf's name, id and
 * label. The members composite through the ordinary typed compositor, so clip
 * runs, blend modes, opacities, masks, Blend If and styles among them bake
 * exactly as they project, and the result is Normal at opacity 1 with no mask,
 * style, Blend If, description or retained source. Layer comps bound to the
 * kept record follow the merge as at 8 bits (dr_doc_merging_down): its
 * recorded offsets shift to the merged origin and its recorded appearance
 * becomes the merged record's own. Refusals are NAMED.
 *
 * EXTENT, as at 8 bits: merge_layers and merge_down keep the UNION EXTENT of
 * their visible members (a style's grown rect, an adjustment's canvas, a
 * group's visible contents) at every depth, so pixels a member holds outside
 * the canvas survive, and the merged record sits at the union's top-left.
 * They refuse "high-depth merge has nothing visible to merge", "high-depth
 * merge covers W×H pixels, over the pixel limit; crop, trim or scale the
 * layers first" and "high-depth merge extends outside the supported
 * coordinate range". merge_visible is canvas-sized at (0,0) at every depth,
 * like 8-bit. One difference remains: a masked adjustment member gates
 * pixels beyond its mask plane by the mask's own outside coverage, where the
 * byte merge leaves them unadjusted. */
/* Native STAMP VISIBLE and FLATTEN, both through dr_precision_doc_edit and
 * neither taking samples:
 *   {"op":"stamp_visible","name":S,"anchor":N}  both fields REQUIRED.
 *   {"op":"flatten"}                            no fields; an extra key refuses.
 * Every rule is the byte one (dr_doc_stamp_visible, dr_doc_flattening), and
 * both take their pixels from the document's OWN projection:
 *   1. A STAMP does NOT join a clip run. The new record lands at the anchor's
 *      own level directly above the anchor's whole SUBTREE and is UNCLIPPED
 *      even when the next sibling is clipped, because the byte insert sets
 *      only the level ("above" in the add_layer sense, with clipped forced
 *      off).
 *   2. A FLATTEN drops the whole LAYER COMP list: every entry's identity is
 *      gone, exactly as the byte op's ..from_pixels supplies an empty list.
 *   3. A FLATTEN's one record is named "Background"; a STAMP's carries the
 *      caller's name (the host default is "Stamp"). Either way: a newly
 *      minted id, no colour label, no locks, no link, visible, opacity 1,
 *      Normal, unclipped, level 0 for the flatten, offset (0,0), and no mask,
 *      style, Blend If, description or retained source.
 *   4. BOTH ops are lock-EXEMPT, at this depth and at byte depth: a stamp adds
 *      a record and writes none, and a flatten replaces the whole stack.
 *   5. A FLATTEN never answers the exact no-op: a one-record document still
 *      answers a new document. Only the host MENU asks for two layers.
 *   6. A FLATTEN keeps the saved channels, guides, ruler origin, profile,
 *      metadata, resolution and evaluation policy; hidden records are
 *      discarded, because the projection is what survives.
 *   7. A STAMP refuses BY NAME when no CONTRIBUTING raster record exists
 *      (the byte op answers NULL there); the floor is ONE such record, not
 *      merge_visible's two, and — the 12E-1b narrowing — an adjustment record
 *      does not count at this depth where the byte predicate counts it.
 *   (8 is the projection source stated above, which
 *      precision_stamp_flatten.rs's module doc numbers; these numbers are that
 *      list's.)
 *   9. Canvas-sized at (0,0) at every depth, like 8-bit (and like
 *      merge_visible): the byte flattened() already renders the CANVAS, so
 *      the byte stamp and the byte flatten are canvas-sized at (0,0) too.
 * Families "layer_stamp" and "layer_flatten", target "document", at 16 bits
 * and at 32-bit float. Refusals are NAMED. */
/* Native STAMP SELECTED and MERGE GROUP (#1 phase 4c,
 * precision_merge_selected), through dr_precision_doc_edit, no samples:
 *   {"op":"stamp_selected","layers":[N,...],"name":S}  both REQUIRED.
 *   {"op":"merge_group","layer":N}
 * Stamp Selected: ONE group, or two or more records sharing one parent
 * (merge_layers' rule), composited over their union extent into ONE new leaf
 * named S — Normal, opacity 1, visible, unclipped, a new id — directly above
 * the topmost member's subtree at its level; every original untouched,
 * lock-exempt. Refusals by name: "Stamp Selected requires at least one
 * layer", "Stamp Selected requires a group or at least two layers; layer N
 * is not a group", "Stamp Selected requires layers that share one parent",
 * and the merge extent's sentences for a union with nothing visible.
 * Merge Group: the record must be a visible group ("Merge Group requires a
 * visible group; layer N is not one"), its locks asked as a merge's; its RAW
 * projection (opacity 1, Normal, no mask, style or Blend If, unclipped, Pass
 * Through isolated) over its union extent becomes ONE leaf in the group's
 * slot keeping its name, id, label, visibility, opacity, blend mode (Pass
 * Through becomes Normal), style, Blend If, mask, clipping, locks and link,
 * so an isolated group's composite is unchanged; comps are carried. Both at
 * 16 bits, 32-bit float and on a CMYK document; families
 * "layer_stamp_selected" and "layer_merge". The byte twins are
 * dr_doc_stamp_selected and dr_doc_merge_group. */
/* Native CANVAS SIZE, through dr_precision_doc_edit and taking no samples:
 *   {"op":"canvas_size","width":W,"height":H,"x":X,"y":Y}   all four REQUIRED.
 * (X, Y) is where the OLD canvas's top-left corner lands in the new one —
 * dr_doc_canvas_resize's `origin` argument, and the byte op is this one's
 * specification. It is a GENERALIZED CROP and reads no sample:
 *   1. every raster and every mask keeps its OWN allocation and moves its
 *      offset by (X, Y); a group's and an adjustment's mask move the same way
 *      while the record itself has no offset to move, exactly as the byte op
 *      moves only a raster's;
 *   2. every saved CHANNEL becomes a new canvas-sized plane holding an exact
 *      copy of the old one at the origin and 0 elsewhere (doc_channel's
 *      padded_plane rule);
 *   3. guides and the ruler origin follow doc_guide's padded_guides /
 *      padded_origin: a guide that left the new canvas is DROPPED, the origin
 *      is CLAMPED;
 *   4. lock-EXEMPT, like every other whole-canvas op, and the layer style's
 *      Scale Effects factor is 1;
 *   5. a RETAINED IMAGE SOURCE rides the canvas at BOTH depths and keeps its
 *      master: the layer's allocation is preserved, so the binding holds, and
 *      the source transform folds the origin in, so the next re-render lands
 *      at the new position. No rasterize is needed — unlike Image Size, crop,
 *      the turns and the flips, which still refuse by name.
 * ONE deliberate difference from the byte op, an existing typed contract: an
 * offset that would leave int32 REFUSES where the byte op saturates. (The
 * exact no-op is no longer a difference: since 12F-1b dr_doc_canvas_resize
 * answers NULL for the same size at origin (0,0) too, and the host answers
 * that request as a SUCCESSFUL no-op — no history step, no dirtying — at 8
 * bits, at 16 and at 32-bit float alike.)
 * AT 32-BIT FLOAT this is admitted (family "geometry_canvas_size", target
 * "document", at 16 bits and at float), and since 12F-1b so are the other
 * EXACT canvas ops: the zero-angle crop ("geometry_crop") and the five
 * quarter turns and flips ("geometry_rotate" / "geometry_flip"). A crop keeps
 * every raster and mask allocation and walks each u16 channel with Nearest; a
 * turn or a flip is a Nearest PERMUTATION, so an extended-range component is
 * only ever copied. The FILTERED set is admitted at float too:
 * Image Size ("geometry_resize"), straighten ("geometry_straighten", the op
 * below), ARBITRARY CANVAS ROTATION ("geometry_rotate_canvas") and free
 * transform — the per-layer and set affine and perspective, on pixels and on
 * masks ("geometry_affine", "geometry_perspective", "geometry_affine_set",
 * "geometry_perspective_set", "mask_transform"). A float kernel resamples
 * premultiplied extended-range f32 and divides by the raw accumulated alpha;
 * an output alpha below 2^-24 becomes transparent, and each output RGB
 * component is CLAMPED to the source layer's own visible range (per channel,
 * over its alpha > 0 pixels), so ringing never leaves it — the one repair a
 * float edit makes (D3). Nearest stays a bit copy. Warp still refuses at
 * float. */
/* Native STRAIGHTEN, through dr_precision_doc_edit (Phase 12F-1b2):
 *   {"op":"straighten","matrix":[a,b,c,d,tx,ty],"filter":F,
 *    "x":X,"y":Y,"width":W,"height":H}        all six REQUIRED.
 * Optional "interactive":true, sent by the 16-bit Crop tool's PREVIEW
 * only, keeps the interactive pixel-work limit; without it a filtered
 * straighten or "rotate_canvas" is bounded by the larger one-shot limit, so a
 * 24 MP photo turns at "bicubic"; the four transform ops take the same key.
 * Over either limit: "typed geometry exceeds the bounded pixel-work limit".
 * The plan carries no work refusal for a commit, a commit run with a control
 * has no work ceiling, and a run with NULL control keeps the one-shot limit;
 * every geometry request without "interactive" follows that rule, not only
 * the filtered turns and transforms. The byte request ignores the key.
 * ONE typed op equal to the byte "straighten" request of
 * dr_doc_geometry_edit, which is its specification: the whole stack turned by
 * (matrix, filter), then the channels by the same, then the crop to
 * (X, Y, W, H).
 * `matrix` turns the picture in CANVAS coordinates (the Crop tool's
 * straighten rotates by -angle about the crop box's centre) and (X, Y, W, H)
 * is the window that becomes the new canvas. Filter F is the geometry filter
 * vocabulary ("nearest", "bilinear", "bicubic", "lanczos3"). One request, one
 * undo step:
 *   1. every raster and EVERY mask — linked, unlinked, on a group, on an
 *      adjustment — is resampled by `matrix` composed with the window shift,
 *      T(-X,-Y) o matrix, into the outward-rounded bounds of its transformed
 *      corners. That integer shift commutes with the rounding, so each
 *      destination extent is exactly the byte "transform, then crop" answer,
 *      and nothing is trimmed: off-canvas storage is retained, as under
 *      dr_doc_crop;
 *   2. every saved CHANNEL is resampled STRAIGHT from the old canvas into
 *      (0, 0, W, H) with the same inverse and outside coverage 0 — which is
 *      transform_channels followed by cropped_channels for a window inside
 *      the canvas;
 *   3. an EXACT matrix (identity, integer translation, quarter turn, flip)
 *      pins the filter to Nearest and an identity plane keeps its own
 *      allocation, so those forms copy rather than resample;
 *   4. guides and the ruler origin are padded_guides / padded_origin by
 *      (-X, -Y) — the trailing crop's rule, because a rotated guide is not a
 *      guide;
 *   5. the PPI is never transposed, and the layer style's Scale Effects
 *      factor is sqrt(|det matrix|) on RASTER records only (the byte twin is
 *      transform_layer_unlocked);
 *   6. lock-EXEMPT, like every other whole-canvas op;
 *   7. a resampled raster DROPS its layer description (text, shape): the
 *      pixels it was rendered from have moved. The host puts none
 *      of them back here, unlike "crop";
 *   8. a RETAINED IMAGE SOURCE refuses by name ("rasterize embedded sources
 *      before this document or perspective transform"), like every other
 *      resampling whole-document op.
 * X and Y are SIGNED and the window may lie past any canvas edge,
 * with the byte request's refusals and sentences (the signed 32-bit range,
 * the pixel limit, the saved-channel budget at the window's size): the planes
 * above map into it unchanged, a channel reads 0 where the old canvas had
 * none, and guides and the ruler origin are padded_guides / padded_origin by
 * (-X, -Y), equal to the cropped ones for a window inside the canvas.
 * Optional "background" with "delete_cropped":true pads the opaque D5
 * Background first, as the byte request states: {"color":[r,g,b],
 * "color_space":"document"|"srgb"} on RGB ("linear" at float only; 16 bits
 * stores paint's prepared colour, float its linear ink) and {"ink":[c,m,y,k]}
 * on CMYK (the ink codes, alpha 65535); the wrong shape is "crop background
 * takes color on an RGB document and ink on a CMYK one", and a pad past the
 * pixel limit, the i32 range or the native save cap is "the crop reaches too
 * far past the canvas for the Background to grow: choose a window nearer the
 * canvas". The plan charges the padded document plus the pad. A
 * non-finite or singular matrix answers the shared affine sentences, a
 * zero-size window the pixel-limit one, and an identity matrix at the full
 * window is an exact no-op (NULL, no error).
 * Optional "delete_cropped":true (the Crop tool's Delete Cropped
 * Pixels) then clips each ELIGIBLE raster (no description, no retained source
 * or warp) to (0, 0, W, H): an exact plane copies its window at Nearest, a
 * resampled one keeps its filter and only its destination shrinks, and one
 * wholly outside becomes a 1x1 transparent pixel at (0, 0). Masks, channels
 * and other layers are as above; a layer comp entry bound to a clipped layer
 * shifts by that layer's clip delta. The host sends a plain crop with the
 * option as this op with the identity matrix, "nearest" and the window.
 * Family "geometry_straighten", target "document", at 16 bits and at
 * 32-bit float, under the float resampling rule stated above. */
/* Native ARBITRARY CANVAS ROTATION (Phase 12F-1b4):
 *   {"op":"rotate_canvas","degrees":D,"filter":F}   both REQUIRED.
 * Uses the same classification, normalization, expanded extent and matrix as
 * dr_canvas_rotation_plan / the byte "rotate_canvas" request. Identity is
 * NULL/no error;
 * exact quarter turns delegate bit-for-bit to rotate90/180/270 (including PPI
 * and chrome); every other finite angle resamples every raster, EVERY mask and
 * each saved channel directly into the expanded canvas. Guides and ruler
 * origin follow the padded rule, PPI stays untransposed, Scale Effects is 1,
 * and resampled raster descriptions are cleared. Family
 * "geometry_rotate_canvas", target "document", at 16 bits and at
 * 32-bit float. Retained-source documents refuse by their existing named
 * policy. */
/* Native layer LOCKS and LINKS, through dr_precision_doc_edit, no samples:
 *   {"op":"layer_lock","layer":N,"locks":M}   M is a DrLockFlags bitmask;
 *                                             reserved bits are masked off and
 *                                             the flags the record already
 *                                             carries are the exact no-op.
 *   {"op":"link_layers","layers":[..]}        At least two records; they take
 *                                             the SMALLEST unused link id.
 *   {"op":"unlink_layers","layers":[..]}      An id left with one member is
 *                                             cleared, as at byte depth.
 * The semantics are dr_doc_lock_block's verbatim, including the frozen alpha
 * channel, the Lock All mask freeze, the MaskApply and Merge rules, and the
 * exemption of rename/visibility/opacity/blend/label/structure. Each record's
 * flags and link id are the "locks" and "link" fields of its
 * dr_precision_doc_info row, and dr_precision_doc_lock_block is the typed twin
 * of dr_doc_lock_block. Since 12E-2b the retained-source and attachment
 * families are lock-gated at BOTH depths: capture_raster and replace answer
 * to the Merge rule, and ALSO to Position when they would move the record's
 * extent or offset, transform to Position, rasterize to Pixels (after its
 * no-source no-op), and every RAW attachment writer to Pixels. retain and
 * place are ungated: retain writes no pixels, and a PLACE creates a record,
 * and insertion is STRUCTURE. */
/* Native Blend If: {op:"layer_blend_if",version:1,layer,blend_if:null|{channel,
 * this_layer,underlying}} — the layer-style JSON's own blend_if object (channel
 * gray|red|green|blue, two ordered [lo0,lo1,hi0,hi1] ramps over 0..255; unknown
 * fields refuse) on every record kind, an adjustment and a Pass Through group
 * included (a Pass Through group carrying one composites isolated), and a group
 * carrying Blend If may switch to Pass Through.
 * No samples; null or an identity clears; exact no-op is NULL. Ramps evaluate
 * on 16-bit codes (knee k at code 257*k) and weight the effective alpha exactly
 * where the byte compositor does. dr_precision_doc_info reports blend_if per
 * layer (null or the object); conditional native35/LBIF persists it. */
/* Native layer styles: {op:"layer_style",version:1,layer,style:null|{…}} — the
 * WHOLE layer-style JSON (the "Layer styles" section: fill_opacity, blend_if,
 * the nine effects AND both knockout keys — "knockout" and
 * "blend_interior_as_group", carried verbatim since Phase 12E-3b and rendered
 * exactly as at 8 bits; a 32-bit float document holds every part of a style
 * and refuses only an effect blend mode outside its float set, BY NAME;
 * strict parse, the message names the key, and an unknown key is refused by
 * its path, e.g. "effects[0].blend_mode") on every record kind, an adjustment
 * and a Pass Through group included (a Pass Through group carrying one
 * composites isolated); a styled group may switch to Pass Through, and a
 * style whose padded plane
 * ((w+2·pad)·(h+2·pad); a raster's own size, a group's the canvas) exceeds the
 * 100 MP limit refuses up front (edit, native reader, geometry plan). No
 * samples; null or an identity clears BOTH the Blend If and the effects/fill;
 * exact no-op is NULL. Effects render from the shape-quantized u8 plane
 * (coverage = round(a16·mask/257)) through the shipped byte renderers under
 * the default global light (120°/30°; there is no typed global light) and
 * composite in f32 in the byte order (Below, pixels at alpha×opacity×fill×
 * weight, Interior; the opacity package model; a styled clip base clamps its
 * members to the shape). dr_precision_doc_info reports "style" per layer (the
 * canonical merged JSON string, or null); conditional native36/LSTY persists
 * the effects/fill while LBIF keeps the Blend If, and a style carrying either
 * knockout key adds the conditional version word 50 — nested INSIDE 46 after
 * 47 and 48, with NO table of its own, since the keys already ride in the LSTY
 * record's style JSON. Clearing the last key restores the prior writer's exact
 * bytes; a file carrying 50 with no such style, or such a style with no 50,
 * refuses by name before any pixel decodes. The PSD export writes a
 * style as the record's own lfx2/iOpa/knko/infx/blending ranges at both
 * depths, and bakes it into the layer's pixels only when bake_reason (a
 * stroke or satin outside Photoshop's range) or mask_bake_reason (an enabled
 * mask with density or feather) applies; a baked style writes neither
 * knockout key. The request's optional "scale" (finite and > 0, else an
 * error) scales the parsed style's lengths (the byte LayerStyle::scaled) before
 * the identity and equality checks: the host's reduced-resolution style preview
 * sends 1/factor so a preview at a fraction of the canvas shows the effects at
 * the same fraction. */
/* Layer Via Copy / Cut (family layer_via, 16 bits and 32-bit float):
 * {"op":"layer_via","layer":N,"cut":bool,"name":S,"selection":bool} — all five
 * keys REQUIRED, an unknown key refuses. dr_doc_layer_via's contract at 16
 * bits, and in a 32-bit float document on its f32 records: a copied
 * pixel keeps its RGB bit for bit and takes alpha alpha*coverage/65535 in
 * f32 (exact at 65535), and the cut is the float Cut's own erase,
 * alpha*(1 - coverage/65535). selection:true reads ONE canvas-sized u16 coverage plane
 * (input_samples = width*height): each layer pixel keeps its RGB exactly and
 * takes alpha round(alpha*coverage/65535), and a layer pixel off the canvas
 * has coverage 0. selection:false reads no samples (input_samples = 0) and
 * copies EVERY layer pixel at full coverage, on or off the canvas. The new
 * record has the source's size and offset, the caller's name, no mask, style,
 * Blend If or description, Normal, opacity 1, visible, UNCLIPPED, directly
 * above the source at its level. cut:true first erases the source under the
 * same coverage exactly as Clear does (selection:false erases every on-canvas
 * pixel and leaves the source's off-canvas ones). Errors by name: an index out
 * of range, a group, an adjustment layer, storage that is not the document's
 * own, and for a cut the erase's own lock refusal (a copy is
 * lock-exempt). NULL with no error when every copied alpha is 0 or the cut's
 * erase changed nothing. */
/* Stack Modes (family layer_stack_mode, 16 bits, 32-bit float and CMYK):
 * {"op":"stack_mode","layers":[N,...],"above":N,"mode":M,"name":S} — every
 * key REQUIRED, an unknown key refuses; dr_doc_stack_mode's contract and
 * rule. At 16 bits the statistic is of the stored RGBA16 codes, rounded half
 * up. In a float document it is of the linear f32 values, computed in f64
 * and stored unclamped (values above white and below zero survive; an odd
 * median, the minimum and the maximum are a member's value bit for bit), a
 * range past the storage bound 65504 stored as the bound. It reads no samples
 * (input_samples = 0). The new record is built at the union extent's origin,
 * Normal, opacity 1, visible, UNCLIPPED, directly above "above" at its level,
 * and every member is hidden. In a CMYK document the statistic runs per ink
 * on L = 65535 - ink (C/M/Y and K as two RGBA16 passes, the 16-bit rule),
 * mapped back to ink; the record is a CMYK raster, paper at alpha 0 where no
 * member covers. Errors by name as at 8 bits, plus a member whose storage is
 * not the document's own. Never NULL with no error. */
/* Move Selected Pixels: {op:"move_selected",layer,dx,dy} with the
 * canvas-sized u16 selection coverage as samples (input_samples = width*height)
 * — dr_doc_selected_pixels_edit's contract at 16 bits, quantized once to u16.
 * {op:"transform_selected",...} (family selected_pixels_transform) is
 * the same with the Free Transform request documented there; "duplicate"
 * and "bounds" mean what they mean there.
 * A raster target at 16 bits and in a 32-bit float document (f32, no
 * quantize; a filtered transform clamps RGB to the float's own visible range
 * and drops alpha below 2^-24, the D3 rule), and the
 * edit clears a described layer's description. A transform_selected with
 * "target":"mask" or {"channel":N} and "background" transforms the
 * u16 mask/channel plane as documented there, at 16 bits AND in a 32-bit
 * float document; the layer's pixels and description are untouched. A
 * move_selected with the same "target"/"background" (family
 * selected_pixels_move) moves that plane likewise, at 16 bits and in a
 * 32-bit float document. Both ops take the colour targets
 * "red"|"green"|"blue"|"alpha" and the "swatch" documented there, at
 * 16 bits and in a 32-bit float document (f32, extended values kept, the
 * vacated value unconverted); the edit clears a described layer's
 * description. */
/* Immutable result; NULL/no error means exact no-op. Input sample count is
 * recomputed from validated document/request extents and must equal length.
 * NULL samples are allowed only when input_samples and length are both zero.
 * A CMYK document (#1 phase 2) admits the structure edits, "add_layer"
 * (bare paper), "set_mask"/"remove_mask"/"mask_properties", "read_plane" and
 * "scalar_edit" of a layer mask, and the four ink tools: "paint_blend",
 * "bucket_fill", "gradient" and "erase_native" with "ink":[c,m,y,k,alpha]
 * (0..1, straight; a gradient takes "stops":[{"position","ink"}], 2..32, and
 * an optional "style", linear when absent) in place of color/color_space,
 * the input planes and plan keys of the RGB tools; a colour-channel "plane",
 * an RGB colour and every other op refuse by name. Since #1's retouch batch
 * "blend_mode" is any mode but Pass Through ("The Pass Through blend mode is
 * unavailable in a CMYK document"), composited on L = 1 - ink, and the
 * retouch ops run on a CMYK raster: "dodge_burn" (protect_tones included)
 * and "retouch_brush" blur/sharpen as the RGB16 op on the two L passes
 * [L_c, L_m, L_y, a] and [L_k; 3, a] mapped back (C/M/Y from the first, K
 * from the second, alpha the first's; a transparency lock keeps the source
 * alpha; alpha 0 keeps the source ink); saturate/desaturate on the first pass
 * alone, K held exactly; "retouch_heal" the RGB16 heal on both passes reading
 * the plate's passes; "retouch_clone" a Normal clone onto a blank pass in
 * both passes, its ink then composited onto the target through blend_mode at
 * opacity; "spot_heal" and "content_aware_fill" the shared synthesis on the
 * four ink codes, alpha untouched. The sampling plate is the layer's ink on
 * paper (current) or the CMYK composite with the excluded records at opacity
 * 0 (current_and_below, all). "new_layer" output inserts a CMYK raster;
 * "red_eye" refuses by name. Since phase 4c it also admits
 * "duplicate_to_new_document" (a CMYK document of its own policy), "layer_via"
 * (ink copied exactly, alpha * coverage; the cut through the ink erase),
 * "stamp_selected" and "merge_group". The styles batch admits "layer_style"
 * with effects (each colour slot an optional "ink":[c,m,y,k] 0..1,
 * "highlight_ink"/"shadow_ink" on a bevel, a stop's "ink"; an ink-less black
 * shadow slot stores K100; every inked hex is rewritten
 * to the ink's sRGB), knockout and "blend_interior_as_group", and
 * "global_light"; a Blend If (in a style or "layer_blend_if") refuses by
 * name. Every other document drops the inks. */
DrPrecisionDocument *dr_precision_doc_edit(const DrPrecisionDocument *document,
    const char *request, const uint16_t *samples, size_t length, uint64_t limit,
    uint64_t other_retained_bytes, const DrJobControl *control, char **err_out);
/* dr_doc_selected_pixels_bounds at 16 bits and in a 32-bit float document:
 * layer is the postorder record index. */
bool dr_precision_doc_selected_pixels_bounds(const DrPrecisionDocument *document,
    size_t layer, const uint16_t *coverage, size_t length, const DrJobControl *control,
    uint32_t *out_rect, char **err_out);
/* Request {op:"read_plane",target:"mask"|"effective_mask"|"channel"|
 * "red"|"green"|"blue"|"alpha",index:N,canvas:false}. Optional canvas:true
 * places exact raw coverage in canvas coordinates with transparent padding.
 * Raw mask/RGBA planes otherwise have stored
 * extents; effective mask/channels are canvas-sized. Effective mask inspection
 * applies density/feather regardless of whether the render mask is enabled. Exact raw u16 copies;
 * effective coverage rounds once to u16. Caller output remains unchanged on
 * error/cancellation; final memcpy publishes atomically with stage cancellation. */
bool dr_precision_doc_read_plane(const DrPrecisionDocument *document, const char *request,
    uint16_t *out, size_t length, uint64_t limit, uint64_t other_retained_bytes,
    const DrJobControl *control, char **err_out);

/* Metadata-only native channel-capacity admission for compound additions.
 * Checks count overflow and all native serialized plane/pixel caps first. */
bool dr_precision_doc_channel_capacity(const DrPrecisionDocument *document,
    size_t additional, char **err_out);
/* Derive scalar Auto Levels using the same scalar_edit filter request and
 * optional canvas coverage as edit_plan/edit. Analysis_scope selection weights
 * raw source samples independently of write scope. Owned JSONblack/white/gamma/
 * identity; apply with scalar levels_channels to avoid reanalysis. */
char *dr_precision_doc_scalar_auto_levels(const DrPrecisionDocument *document,
    const char *request, const uint16_t *samples, size_t length,
    uint64_t limit, uint64_t other_retained_bytes,
    const DrJobControl *control, char **err_out);
/* Exact raw scalar histogram; plan through dr_precision_doc_edit_plan with
 * {op:"scalar_statistics",target:"mask"|"channel"|"red"|"green"|"blue"|"alpha",
 * index:N,selection:false,stride:1}. Selection true borrows canvas u16 coverage;
 * otherwise measure the complete raw extent, including off-canvas storage.
 * Same schema2 counts/bins as scoped analysis, source="scalar", target/index.
 * Shared RGB rows repeat scalar codes with virtual opaque alpha. Only display
 * bins use stride; endpoint counts and code sums always inspect every sample.
 * NULL/errors never modify inputs. Owned JSON must use dr_string_free. */
char *dr_precision_doc_scalar_statistics(const DrPrecisionDocument *document,
    const char *request, const uint16_t *samples, size_t length,
    uint64_t limit, uint64_t other_retained_bytes,
    const DrJobControl *control, char **err_out);
/* apply_mask {layer} bakes effective density/feather into native raster alpha,
 * even if disabled, preserving hiddenRGB then removing mask. Groups refuse.
 * In a 32-bit float document alpha becomes alpha*coverage in f32 and
 * every RGB component is kept bit for bit; a transparency lock refuses.
 * Scalar replace_canvas reads canvas-sized input while preserving raw samples
 * outside the canvas; optional selection remains a separate final write gate. */
/* Native selection creation: op selection in the typed edit plan; source scopes
 * composite/layer/backdrop and alpha/luminance/color/wand/Color Range/Quick/
 * similar. Input count is zero, except similar {tolerance,contiguous}, which
 * reads the canvas-sized u16 current selection (Select > Grow/Similar), and
 * refine_edge {edge_radius 0..64, smart_radius, area} (Select and Mask's edge
 * stage, dr_selection_refine's at u16), which reads the canvas-sized coverage
 * to refine and then, when area is true, the same count of area categories as
 * u16 values 0 / 128 / 255 (2N in all); its plan adds 64 bytes per pixel and
 * twice its input. Quick's params take radius and smooth as dr_selection_quick
 * does. Output is exactly one u16 code per canvas pixel.
 * No byte preview is used. Output remains untouched on refusal/cancellation. */
bool dr_precision_doc_select(const DrPrecisionDocument *document, const char *request,
                             const uint16_t *input, size_t input_length,
                             uint16_t *out, size_t out_length,
                             uint64_t limit, uint64_t other_retained,
                             const DrJobControl *control, char **err_out);
/* Independent authoritative coverage. Request kind invert/combine/paint/
 * feather/smooth/grow/shrink/border/transform/perspective/blend/luminosity/refine; see precision_selection_plane.rs.
 * perspective takes source_rect [x,y,w,h], quad [x0,y0,...,x3,y3] (TL,TR,BR,BL)
 * and filter; a parallelogram quad is its affine, a concave quad refuses.
 * blend takes numeric mode0...26, channels1 or3, opacity[0,1] and optional
 * invert_base/invert_source. Planar input: baseR/G/B then sourceR/G/B, or
 * base/source forscalar. Output is PLANAR channels*N; fourHSLmodes requireRGB.
 * luminosity takes band lights/darks/midtones and level1...3 on one plane.
 * refine composes one coverage plane through smooth[0,64], shift[-64,64],
 * feather[0,64] then contrast[0,1] in that order, each field defaulting to0;
 * all zero copies the input codes and contrast1 thresholds at32768.
 * Input order: base then incoming(combine), coverage(paint with value), or
 * premultiplied gray then coverage(paint without value). Maximum radius250;
 * exact geometry checks precede allocation. Legacy u8 APIs unchanged.
 * Since 13A-6a feather/smooth/grow/shrink/border and refine's stages evaluate
 * over the coverage's nonzero SUPPORT grown by the kernel's reach, equal sample
 * for sample to the whole-plane result. No kind has a work refusal:
 * dr_selection_u16 is bounded by the planned scratch and its control's cancel.
 * gradient (Quick Mask): {"kind":"gradient","from":[x,y],"to":[x,y],
 * "style":"linear"|"radial"|"angle"|"reflected"|"diamond" (default linear),
 * "stops":[{"position":p,"color":[g,g,g,alpha]},...],"opacity":o (default 1)}
 * over ONE canvas plane in and out: each sample v becomes
 * round(v*(1-a) + g*65535*a), a = o*alpha, at its pixel centre. 2..32 stops in
 * non-decreasing position, every value in [0,1]. */
/* Continuous encoded RGB [0,1] to scalar coverage through shared Rec.709
 * coefficients, one native quantization. Refusal leaves *out unchanged. */
bool dr_scalar_ink_u16(double red, double green, double blue, uint16_t *out, char **err_out);
/* What Fill and Stroke write into ONE colour channel of a layer
 * (measured against Photoshop): the swatch's 0.30/0.59/0.11 luma. u8: over its
 * 8-bit codes, rounded to a code. u16: over encoded RGB [0,1] (finite, else
 * false with err_out), unrounded until the one 16-bit quantization. A NULL
 * out refuses; refusal leaves *out unchanged. Masks and alpha channels keep
 * dr_scalar_ink_u16's Rec.709 gray. */
bool dr_plane_ink_u8(uint8_t red, uint8_t green, uint8_t blue, uint8_t *out, char **err_out);
bool dr_plane_ink_u16(double red, double green, double blue, uint16_t *out, char **err_out);
char *dr_selection_u16_plan(uint32_t width, uint32_t height, const char *request,
                            uint64_t limit, uint64_t other_retained, char **err_out);
bool dr_selection_u16(uint32_t width, uint32_t height, const char *request,
                      const uint16_t *input, size_t input_length,
                      uint16_t *out, size_t out_length,
                      uint64_t limit, uint64_t other_retained,
                      const DrJobControl *control, char **err_out);

/* Native version1 brush coverage: strict JSON <=4096 bytes with required
 * version:1, hardness[0,1], roundness[.01,1], angle_degrees[-180,180], pixel:bool.
 * Optional opacity[0,1] default1 scales accumulated coverage before its sole
 * u16 quantization; callers applying opacity in a paint edit leave this at1.
 * dabs contains dab_count packed x/y/diameter/opacity DOUBLE tuples, at most
 * 50000. Coordinates finite and abs<=1e9; diameter[0,1e6], opacity[0,1].
 * Count/dimensions precede pointer access; geometry and the aggregate 2 billion
 * work cap ("native brush exceeds 2 billion footprint work units; use a
 * shorter stroke or a smaller brush") precede pixel allocation. Soft ellipses use pixel-center smoothstep;
 * hard ellipses use8x8 area integration, pixel mode exact square area. Ordered
 * source-over coverage quantizes once after all dabs. Plan JSON reports
 * retained_bytes0,scratch_bytes,output_samples,work_units (dr_string_free).
 * Output has width*height native u16 samples; errors/cancellation leave it
 * untouched. NULL dabs requires count0, inputs/output must not alias; optional
 * single-use control and every buffer owner remain alive for the call.
 * Since 13A-6c-1 the f64 field spans the stroke's footprint (the union of the
 * nonzero dabs' clipped boxes), hard dabs of diameter >= 1 classify each pixel
 * exactly (interior 1, exterior 0) and integrate 8x8 only on the edge band, and
 * work_units = n + |footprint| + per dab (0 zero dabs, box pixel/soft,
 * min(64*box, box + 64*band bound) hard): never above the earlier figure; the
 * output is still canvas-sized and equal to the canvas-field path. */
char *dr_brush_u16_plan(uint32_t width, uint32_t height, const char *request,
    const double *dabs, size_t dab_count, uint64_t limit,
    uint64_t other_retained, char **err_out);
bool dr_brush_u16(uint32_t width, uint32_t height, const char *request,
    const double *dabs, size_t dab_count, uint16_t *out, size_t out_length,
    uint64_t limit, uint64_t other_retained, const DrJobControl *control,
    char **err_out);

/* The retouch brushes (ffi_retouch_brush.rs). Pure: each returns a new
 * document, or NULL when nothing would change.
 *
 * dr_doc_retouch_brush_layer blurs (kind 0), sharpens (1), saturates (2) or
 * desaturates (3) byte layer idx where a stroke overlay covers it. src is the
 * canvas-sized premultiplied RGBA8 overlay dr_doc_dodge_burn_layer takes (w/h
 * must equal the canvas); only its alpha is read, as stroke coverage. Each
 * covered pixel moves toward its target by coverage * strength (clamped to
 * 0..1): blur is the alpha-weighted Gaussian of the layer's own colour at
 * sigma 2 px, sharpen an unsharp mask at sigma 1 px and amount 1, desaturate
 * the Rec. 709 luma Y, saturate clamp(Y + 2 * (rgb - Y)). Layer alpha is never
 * written, alpha-0 pixels are skipped and the layer does not grow. vibrance
 * (saturate and desaturate only) takes the Vibrance target
 * instead: the Vibrance stage of Image > Adjustments > Vibrance at +100
 * (saturate) or -100 (desaturate) on the stored samples, mixed the same way.
 * sample_all_layers (blur and sharpen only) reads the target from the
 * sampled picture (every visible layer, those above the target included, as
 * they composite, over the walk rect and its halo clipped to the canvas) and
 * writes each covered pixel as the premultiplied mix of all four channels of
 * the layer toward [target, picture alpha] by coverage * strength: an empty
 * layer fills with the softened picture, under a transparency lock alpha is
 * kept and alpha-0 pixels stay, and the layer still does not grow. NULL with
 * no error on NULL args, a dimension mismatch, non-finite strength,
 * kind > 3, an out-of-range or group idx, a lock that forbids a pixel edit,
 * or no byte changing; NULL with a message (err_out NULL or writable, freed
 * with dr_string_free) for an option on a kind it does not apply to
 * ("vibrance applies to the sponge only", "sample_all_layers applies to blur,
 * sharpen and smudge") or a failed render of the picture.
 *
 * dr_doc_history_brush_layer paints layer idx back toward layer source_idx of
 * source, an earlier state of the same document: the premultiplied mix of all
 * four channels toward the source layer's pixel at the same canvas position
 * (transparent where the source layer has none) by coverage * opacity. The
 * layer does not grow; under a transparency lock each pixel keeps its alpha.
 * NULL as above, and for a source of another canvas size.
 *
 * dr_doc_smudge_layer drags the picture along a stroke given as its dabs:
 * brush_request and dabs are dr_brush_u16's (canvas coordinates). Each move
 * between two dabs is walked in steps of at most one pixel, so the result does
 * not depend on the dab spacing; the work is bounded by Smudge's own 256
 * million units (not the brush's 2 billion: the byte stroke runs with no
 * control), charged per step. selection is canvas-sized u8 coverage, or
 * NULL with selection_len 0. sample_all_layers smears the sampled
 * picture (as dr_doc_retouch_brush_layer's) into the layer: the footprint is
 * clipped to the layer's part of the canvas, so pixels outside the canvas are
 * never read or written; the walk starts from the picture there; a pixel the
 * walk leaves at its starting value keeps the layer's bytes, and a moved
 * sample is written straight with its own alpha (a transparency lock keeps
 * alpha). NULL with no error for a refusal or no change; NULL with a message
 * (dr_string_free) for a bad request or dab list, a stroke past that bound
 * ("smudge stroke exceeds 256 million work units; use a shorter stroke or a
 * smaller brush"), or a selection of another length.
 *
 * The typed twins plan first: each plan's JSON (dr_string_free) reports
 * retained_bytes, scratch_bytes and input_samples. dr_precision_doc_smudge
 * takes strength in 0..1, the canvas-sized u16 selection (or NULL, 0) and
 * sample_all_layers, whose plan (dr_precision_doc_smudge_plan with the same
 * flag) adds the sampled picture's term once;
 * dr_precision_doc_history_brush takes opacity in 0..1 and samples: the
 * canvas-sized u16 stroke coverage, then the selection plane when planned with
 * selection (length n or 2n). Both refuse a target that is not a raster of
 * the document's own storage and a retained source by name; an exact no-op is
 * NULL with no error; control is an optional single-use job. The 16-bit Blur,
 * Sharpen and Sponge are dr_precision_doc_edit's op "retouch_brush":
 * {"op":"retouch_brush","layer":n,"kind":"blur"|"sharpen"|"saturate"|
 * "desaturate","strength":0..1,"selection":bool,"vibrance":bool,
 * "sample_all_layers":bool}, input laid out as dodge_burn's; vibrance and
 * sample_all_layers (both default false) as the byte call's, refused in its
 * sentences, and a sampled stroke's plan adds the picture's term once.
 *
 * A 32-bit float document (float_retouch_brush.rs) runs the same three
 * calls on an f32 raster, in linear light with the range kept: colour is never
 * clamped to 0..1 and the only clamp is the stored bound +-65504. Blur and
 * Sharpen ("retouch_brush" kinds blur and sharpen) use the float Gaussian FIR
 * (tails under 1e-7 of the weight), so a blur spreads a value above white and
 * a sharpened value below zero is stored. The Sponge (kinds saturate and
 * desaturate) writes the luma and Y + 2 * (rgb - Y) over the linear samples,
 * unclamped; with vibrance the Vibrance stage runs through the document's
 * display encoding in its Scaled form (as Image > Adjustments > Vibrance at
 * float). Smudge carries the premultiplied f32 field unquantized; an alpha
 * below 2^-24 is transparent black. Sample All Layers reads the float
 * picture, linear and unclamped.
 * dr_precision_doc_history_brush takes a float source of the same canvas
 * size ("history brush source has a different bit depth" otherwise): coverage
 * * opacity of 1 writes the source sample's bits, less than 1 the
 * premultiplied mix in binary64 rounded once to f32.
 *
 * A CMYK document (#1) runs dr_precision_doc_smudge and the History Brush
 * on a CMYK raster as the RGB16 walk on the two L passes [L_c, L_m, L_y, a]
 * and [L_k; 3, a] (the history state's layer split the same way), mapped
 * back as dr_precision_doc_edit's CMYK retouch ops are; a state that is not
 * a CMYK document with a CMYK raster at source_layer refuses "The History
 * Brush's source state is not a CMYK raster". The Sponge's vibrance runs on
 * pass A only, K held; Sample All Layers renders the ink picture once and
 * each pass reads its own L pass of it.
 *
 * A Grayscale document (#1 phase 2c) runs the three calls and the
 * "retouch_brush" op on R = G = B unchanged, except the Sponge: Saturate
 * m + 2 * (p - m) (clamped to 0..1 at 8 and 16 bits, unclamped at 32) and
 * Desaturate m, m the middle gray (0.5, or at 32 bits the linear value of code
 * 0.5), vibrance ignored. dr_precision_doc_history_brush and its plan refuse
 * a state that is not Grayscale: "The History Brush paints a Grayscale
 * document from a Grayscale state only: this state is from before Image >
 * Mode > Grayscale."
 *
 * A Lab Color document (#1 phase 2e) runs them on the L*, a*, b* codes
 * unchanged, except the Sponge: Saturate n + 2 * (p - n) on a* and b*
 * (clamped to 0..1) and Desaturate n, n the a* = b* = 0 code 0x8080, L* kept,
 * vibrance ignored. dr_precision_doc_history_brush and its plan refuse a
 * state that is not Lab on a Lab document ("The History Brush paints a Lab
 * Color document from a Lab Color state only: this state is from before
 * Image > Mode > Lab Color.") and a Lab state on any other ("The History
 * Brush cannot paint from a Lab Color state: this state is from before
 * Image > Mode > RGB Color."). */
DrDocument *dr_doc_retouch_brush_layer(const DrDocument *doc, size_t idx,
    const uint8_t *src, uint32_t w, uint32_t h, uint8_t kind, float strength,
    bool vibrance, bool sample_all_layers, char **err_out);
DrDocument *dr_doc_history_brush_layer(const DrDocument *doc, size_t idx,
    const uint8_t *src, uint32_t w, uint32_t h, const DrDocument *source,
    size_t source_idx, float opacity);
DrDocument *dr_doc_smudge_layer(const DrDocument *doc, size_t idx,
    const char *brush_request, const double *dabs, size_t dab_count,
    float strength, const uint8_t *selection, size_t selection_len,
    bool sample_all_layers, char **err_out);
char *dr_precision_doc_smudge_plan(const DrPrecisionDocument *document,
    size_t layer, const char *brush_request, const double *dabs,
    size_t dab_count, bool selection, bool sample_all_layers, uint64_t limit,
    uint64_t other_retained_bytes, char **err_out);
DrPrecisionDocument *dr_precision_doc_smudge(
    const DrPrecisionDocument *document, size_t layer,
    const char *brush_request, const double *dabs, size_t dab_count,
    double strength, const uint16_t *selection, size_t selection_len,
    bool sample_all_layers, uint64_t limit, uint64_t other_retained_bytes,
    const DrJobControl *control, char **err_out);
char *dr_precision_doc_history_brush_plan(const DrPrecisionDocument *document,
    size_t layer, const DrPrecisionDocument *source, size_t source_layer,
    bool selection, uint64_t limit, uint64_t other_retained_bytes,
    char **err_out);
DrPrecisionDocument *dr_precision_doc_history_brush(
    const DrPrecisionDocument *document, size_t layer,
    const DrPrecisionDocument *source, size_t source_layer, double opacity,
    const uint16_t *samples, size_t length, uint64_t limit,
    uint64_t other_retained_bytes, const DrJobControl *control,
    char **err_out);

/* The Art History Brush (art_history_stroke.rs, the kernel, with
 * doc_art_history.rs, precision_art_history.rs, cmyk_art_history.rs,
 * gray_art_history.rs and lab_retouch.rs).
 * request is the tool's options, JSON {"version":1,"style":S,"area":0..500,
 * "tolerance":0..100,"opacity":0..1,"blend_mode":M,"seed":0..4294967295},
 * unknown fields refused: S one of tight_short, tight_medium, tight_long,
 * loose_medium, loose_long, dab, tight_curl, tight_curl_long, loose_curl,
 * loose_curl_long; M Normal 0, Darken 6, Lighten 7, Hue 23, Saturation 24,
 * Color 25 or Luminosity 26 (any other refuses "art history brush blend_mode
 * must be Normal, Darken, Lighten, Hue, Saturation, Color or Luminosity").
 * brush_request and dabs are dr_brush_u16's (canvas coordinates). The first
 * dab is a click, then each dab at least max(area/2, its diameter, 1) px from
 * the last click; click k lays the style's strokes, each drawn from a
 * positional stream of (seed, k, stroke), so the same seed and dabs give the
 * same pixels and a dab prefix gives a stroke prefix. A stroke starts uniform
 * in the disc of radius area/2 about the click and is skipped where the
 * source has no pixel or where the premultiplied max-channel difference
 * between the source and the layer before the call, times 100, is below
 * tolerance; its colour is the source at its start, its alpha the source's
 * alpha times the click dab's opacity times opacity; it paints its dabs'
 * field (the max of their coverage) through blend_mode. The work, summed over
 * every stroke dab as the brush charges it, refuses above 2 billion ("art
 * history brush exceeds 2 billion work units; use a smaller Area, a shorter
 * stroke or a smaller brush"). The layer keeps its extent; under a
 * transparency lock each pixel keeps its alpha.
 *
 * dr_doc_art_history_brush_layer paints byte layer idx from layer source_idx
 * of source (an earlier state of the same document), selection canvas-sized
 * u8 coverage or NULL with selection_len 0. NULL with no error on NULL doc or
 * source, an out-of-range or group index on either side, a source of another
 * canvas size, a lock that forbids a pixel edit, or no byte changing; NULL
 * with a message (dr_string_free) for a bad request or dab list, the work
 * refusal, or a selection of another length.
 *
 * The tool runs on 8-bit documents only, so the typed pair runs an
 * 8-bit CMYK document's ink (cmyk_composite's moded over on 1 - ink) and
 * refuses every other typed document by name: "The Art History Brush is
 * unavailable in a 16-bit document.", "... in a 32-bit
 * document ..." and "... in a 16-bit CMYK document ...". The plan's JSON
 * (dr_string_free) reports retained_bytes, scratch_bytes and input_samples
 * (the canvas pixel count when selection, else 0); a layer that is not a
 * CMYK raster refuses "The Art History Brush paints only a CMYK raster layer",
 * judged before the source; a source that is not a CMYK document with a CMYK
 * raster at source_layer refuses "The Art History Brush's source state is not
 * a CMYK raster". An 8-bit Grayscale document (#1 phase 2e) paints its
 * R = G = B raster through blend_mode Normal, Darken or Lighten (the others
 * refuse "The <mode> blend mode is unavailable in a Grayscale document.");
 * a 16-bit one refuses as a 16-bit document does, a layer that
 * is not a u16 raster "The Art History Brush paints only a Grayscale raster
 * layer", judged before the source, and a source that is not a Grayscale
 * document's raster "The Art History Brush's source state is not a Grayscale
 * raster". An 8-bit Lab Color document (#1 Lab 2f, third part) paints
 * per channel on its L*a*b* codes, its strokes along L* (the Tolerance test
 * compares every channel), through lab_document::blend_refusal's modes
 * (Darken and Lighten refuse with its sentence; the four HSL modes are
 * admitted); a 16-bit one refuses as a 16-bit document does, a
 * layer that is not a u16 raster "The Art History Brush paints only a Lab
 * Color raster layer", judged before the source, and a source that is not a
 * Lab Color document's raster "The Art History Brush's source state is not a
 * Lab Color raster". dr_precision_doc_art_history_brush
 * takes the canvas-sized u16 selection plane or NULL with selection_len 0; an
 * exact no-op is NULL with no error; control is an optional single-use job. */
DrDocument *dr_doc_art_history_brush_layer(const DrDocument *doc, size_t idx,
    const DrDocument *source, size_t source_idx, const char *request,
    const char *brush_request, const double *dabs, size_t dab_count,
    const uint8_t *selection, size_t selection_len, char **err_out);
char *dr_precision_doc_art_history_brush_plan(const DrPrecisionDocument *document,
    size_t layer, const DrPrecisionDocument *source, size_t source_layer,
    const char *request, const char *brush_request, const double *dabs,
    size_t dab_count, bool selection, uint64_t limit,
    uint64_t other_retained_bytes, char **err_out);
DrPrecisionDocument *dr_precision_doc_art_history_brush(
    const DrPrecisionDocument *document, size_t layer,
    const DrPrecisionDocument *source, size_t source_layer,
    const char *request, const char *brush_request, const double *dabs,
    size_t dab_count, const uint16_t *selection, size_t selection_len,
    uint64_t limit, uint64_t other_retained_bytes,
    const DrJobControl *control, char **err_out);

/* Edit > Fade (v1; doc_fade.rs, precision_fade.rs, the kernel fade.rs): doc is
 * the faded step's RESULT, before the state before that step, and idx/layer
 * names the same layer in both. Per pixel: where both are transparent the
 * result's pixel stays verbatim; E is before where the result is transparent,
 * the result for Normal, else the result composited over before in mode (the
 * projection's own W3C step, Dissolve's dither included); E' is the result
 * moved toward E by the step's weight; the output is before moved toward E'
 * by opacity (0..1), both moves the premultiplied lerp. Normal at opacity 1
 * is the result, opacity 0 is before. The weight is coverage, canvas-sized
 * (u8 byte, u16 typed; NULL/0 for 1 everywhere) for a region step, or with
 * changed_only 1 where the stored pixel differs from before's and 0
 * elsewhere (a stroke). The whole layer rect is walked, never grown; before
 * is read at the same canvas position, transparent outside its layer; under
 * a transparency lock the layer keeps its alpha. Pass Through refuses; a
 * 32-bit float document refuses a mode it does not admit with the float
 * operator table's sentence. A u16 result rounds as the History Brush does;
 * an f32 result keeps its range (held to +-65504). dr_doc_fade answers NULL
 * for every refusal and for no byte changing; the typed pair refuses by name
 * through err_out (a non-raster or retained-source target, "fade source has
 * a different canvas size", "... bit depth", an invalid opacity or coverage
 * length) and answers NULL with no error for an exact no-op. The typed plan
 * is the History Brush's (input_samples n with coverage, else 0); control is
 * an optional single-use job. On a CMYK document (#1) the typed pair fades a
 * CMYK raster by the same rule on the four inks (a mode composited on
 * L = 1 - ink as its layers are; a CMYK `before`); its plan is layer pixels x
 * 20 + canvas x 2 + the display preparation. On a Grayscale document (#1
 * phase 2c) dr_precision_doc_fade refuses, before any work, a mode the Gray
 * document does not offer, in the Gray blend sentence, and on a Lab Color
 * document (#1 Lab phase 2c) a mode outside the Lab list, in the Lab blend
 * sentence (the plan takes no mode; the host refuses before planning). */
DrDocument *dr_doc_fade(const DrDocument *doc, const DrDocument *before,
    size_t idx, DrBlendMode mode, float opacity, const uint8_t *coverage,
    size_t coverage_len, bool changed_only);
char *dr_precision_doc_fade_plan(const DrPrecisionDocument *document,
    size_t layer, const DrPrecisionDocument *before, bool coverage,
    uint64_t limit, uint64_t other_retained_bytes, char **err_out);
DrPrecisionDocument *dr_precision_doc_fade(const DrPrecisionDocument *document,
    size_t layer, const DrPrecisionDocument *before, DrBlendMode mode,
    double opacity, const uint16_t *coverage, size_t coverage_len,
    bool changed_only, uint64_t limit, uint64_t other_retained_bytes,
    const DrJobControl *control, char **err_out);

/* Additive edit ops (precision_paint_tools.rs): paint_blend(layer,color RGBA,
 * color_space,opacity,blend_mode0...26); bucket_fill adds x/y,tolerance u16,
 * contiguous and defaults blend_mode0; gradient uses start/end RGBA,from/to
 * double pairs,radial,color_space,opacity and optional blend_mode0. These grow
 * raster storage only to changed canvas ink, preserving independent raw masks.
 * dodge_burn(layer,exposure[0,1],range0shadows/1midtones/2highlights,burn,
 * protect_tones default false: the three channels move together, each scaled
 * by curve(V)/V with V the largest, #1; a float layer groups its in-range
 * display codes; on a Lab Color document, #1 phase 2e, only L* is shaped and
 * protect_tones is the plain curve on L*) retains alpha and raw storage; a scalar_edit dodge_burn
 * accepts protect_tones and takes the plain curve (a single plane). erase_native(layer,opacity) changes alpha
 * alone without expansion. Every op reads one canvas coverage plane;
 * selection:true adds a second separate canvas gate, multiplied once before
 * final native quantization. No erase blend mode exists; historical paint/
 * erase schemas and byte operations retain their existing meaning.
 * scalar_edit operation gradient(from,to,radial,start/end u16,opacity default1,
 * start_alpha/end_alpha normalized doubles default1 interpolated independently)
 * uses canvas pixel centers with optional selection and no input payload.
 * STOPS, optional on both gradients, absent = the bytes above: "stops":
 * [{"position":p,"color":[r,g,b,a]},...] (2..32, non-decreasing position,
 * straight, in color_space; a scalar_edit's are [g,g,g,alpha], g = gray/65535)
 * and "style":"linear"|"radial"|"angle"|"reflected"|"diamond" (absent =
 * radial's; refused without stops). start/end stay required and carry the
 * first and last stop. The ramp is linear between bracketing stops.
 * A scalar_edit on target mask whose operation is the Invert filter
 * ({"kind":"filter","edit":{"op":"adjustment","description":<invert>}})
 * follows dr_doc_invert_mask's rule: selection:false also flips the
 * mask's outside_coverage; selection:true plans, reads and writes the union of
 * the stored rect and the canvas (a group's mask keeps its rect).
 * paint_blend's optional plane:"red"|"green"|"blue"|"alpha" targets
 * that one component of a 32-bit float layer: it becomes the blend of itself
 * with 0.30r+0.59g+0.11b of the ink's components as given (document or linear
 * ink; sRGB ink and preserve_alpha refuse), at ink alpha*opacity*coverage
 * whatever the pixel's stored alpha, over Normal, Multiply, Darken, Lighten,
 * Difference, Linear Dodge, Subtract and Dissolve; the layer never grows and
 * every other component is untouched. A u16 document refuses it (a 16-bit
 * colour plane is a scalar_edit target). */
/* Additive retouch_clone/retouch_heal/spot_heal selection:true reads stroke then
 * selection canvas planes. Clone multiplies both before source-over. Healing
 * keeps its published stroke half-coverage contour and intersects with positive
 * selection; the independent selection fades only the final native RGB write.
 * Historical retouch requests without selection retain their exact meaning.
 * retouch_clone's optional "blend_mode" (a DrBlendMode raw value 0..26,
 * default 0 = Normal, whose output is unchanged) composites the cloned source
 * colour through that mode at each pixel's canvas position; it refuses with
 * output:"new_layer" (nothing to blend with) and on every other retouch op
 * ("blend_mode is not an argument of this retouch operation"). spot_heal's
 * optional "type": "content_aware" (the default and every
 * earlier request's meaning), "create_texture" or "proximity_match", as
 * dr_doc_spot_heal_layer_job's spot_type, at 16 and 32 bits and on CMYK ink;
 * any other op refuses it ("type is not an argument of this retouch
 * operation"). */


/* Deliberate flattened-appearance depth conversion. Plans are owned JSON
 * (dr_string_free), with retained_bytes/scratch_bytes/output_bytes and explicit
 * discarded editable state. These flattened conversions preserve ICC/PPI/capture
 * packets and produce ONE raster, and carry every saved channel, alpha and spot,
 * with its name, options and ink (coverage x257 up, nearest (v+128)/257 down;
 * #1); the structure-preserving 8->16 conversion is
 * the separate dr_doc_promote_structure family below. Legacy promotion runs
 * legacy evaluation then multiplies exact bytes by 257. Plain stacks cancel by
 * tile; legacy groups/styles cancel per tile row through the regional tree;
 * guided Shadows/Highlights cancel per row inside the full-frame render.
 * scratch_bytes includes the description-context term and, on a canvas where
 * the tree's 20-byte tile exceeds 16 bytes per canvas pixel, that difference.
 * Downconversion renders authoritative u16 first. Dither 0=nearest, 1=ordered
 * 8x8 RGB; alpha always nearest, never dithered. Limits include input+scratch+
 * other live handles; cancellation/refusal returns NULL with error. */
char *dr_doc_promote_flattened_plan(const DrDocument *document, char **err_out);
DrPrecisionDocument *dr_doc_promote_flattened(const DrDocument *document,
    uint64_t limit, uint64_t other_retained_bytes, const DrJobControl *control, char **err_out);
char *dr_precision_doc_downconvert_plan(const DrPrecisionDocument *document, char **err_out);
DrDocument *dr_precision_doc_downconvert(const DrPrecisionDocument *document, uint32_t dither,
    uint64_t limit, uint64_t other_retained_bytes, const DrJobControl *control, char **err_out);
/* Structure-preserving 8->16 promotion (precision_conversion_structure.rs,
 * designs/phase-12h-conversion-status.md). Every supported record is carried
 * with its id, each byte code widened by 257 (rasters, masks, channels):
 * groups, Pass Through, adjustments, text/shape descriptions, masks and their
 * properties, channels, styles, Blend If, knockout, clipping, locks, links,
 * labels, comps, guides, ruler origin, PPI, profile, metadata and evaluation
 * policy. The plan JSON {flattened:false, structure:"preserve", direction,
 * widening, retained_bytes, scratch_bytes, output_bytes, preserved[],
 * discarded[], rendering_differences[{index,id,name,note}],
 * blocked[{index,id,name,reason}]} is computed without canvas-sized planes;
 * a non-empty blocked list makes dr_doc_promote_structure return NULL with the
 * first row's sentence before any plane is promoted. The preview JSON
 * {scale:65535, max_delta, mean_delta, compared_samples, skipped_latent_rgb}
 * is per-channel |a-b| of the byte projection x257 against the converted u16
 * render, RGB skipped where both alphas are 0. The conversions into float are
 * the dr_*_promote_to_float family below. */
char *dr_doc_promote_structure_plan(const DrDocument *document, char **err_out);
DrPrecisionDocument *dr_doc_promote_structure(const DrDocument *document, uint64_t limit,
    uint64_t other_retained_bytes, const DrJobControl *control, char **err_out);
char *dr_doc_promote_structure_preview(const DrDocument *document,
    const DrPrecisionDocument *converted, uint64_t limit, uint64_t other_retained_bytes,
    const DrJobControl *control, char **err_out);
/* Structure-preserving NARROWING (precision_conversion_narrow.rs, 12H-2):
 * 16->8, float->16 and float->8 with every supported record carried and its id
 * kept. One walk, three per-record sample mappings: u16->u8 quantizes each
 * raster with the flattened path's quantizer (dither 0 none / 1 ordered 8x8
 * RGB; alpha nearest, never dithered) and masks, channels and warp originals
 * nearest; f32->u16 is the float document's own tone map per record; f32->u8
 * is that tone map, then that quantizer. target_depth is 8 or 16 (16 on a
 * 16-bit source is an error, not a row). The plan JSON is the promote family's
 * shape with direction rgba16_to_rgba8 | rgba32_float_to_rgba16 |
 * rgba32_float_to_rgba8, rgb_quantization/alpha_quantization for 8-bit
 * targets, tone_map for float sources and "extended_range" discarded; a record
 * nested deeper than 10 levels blocks an 8-bit target FIRST. A document-level
 * row has id 0. Retained sources and warp bindings are carried with every
 * staleness guard tied to the landed layer. The preview compares this
 * document's render (tone-mapped when float) against converted_u8's
 * projection x257 or converted_u16's render; exactly one converted handle
 * must be non-NULL. The conversions into float are the promote_to_float
 * family below. */
char *dr_precision_doc_narrow_structure_plan(const DrPrecisionDocument *document,
    uint32_t target_depth, char **err_out);
DrDocument *dr_precision_doc_narrow_structure_to_u8(const DrPrecisionDocument *document,
    uint32_t dither, uint64_t limit, uint64_t other_retained_bytes,
    const DrJobControl *control, char **err_out);
DrPrecisionDocument *dr_precision_doc_narrow_structure_to_u16(
    const DrPrecisionDocument *document, uint64_t limit, uint64_t other_retained_bytes,
    const DrJobControl *control, char **err_out);
char *dr_precision_doc_narrow_structure_preview(const DrPrecisionDocument *document,
    const DrDocument *converted_u8, const DrPrecisionDocument *converted_u16,
    uint64_t limit, uint64_t other_retained_bytes, const DrJobControl *control,
    char **err_out);
/* Structure-preserving conversion INTO 32-bit float (precision_conversion_float.rs):
 * 16->float and 8->float with every supported record carried and its id
 * kept. The rule: each u16 RGB code c becomes the document profile's TRC decode
 * of c/65535 as f32 (linear light), alpha c/65535; an 8-bit source first takes
 * the exact x257 widening. The result is a float document in the same profile
 * under the clip tone map; masks and channels stay u16 coverage. Narrowing the
 * result back (dr_precision_doc_narrow_structure_to_u16 / _to_u8, dither 0)
 * returns every source code. The plan JSON is the promote family's shape with
 * direction rgba16_to_rgba32_float | rgba8_to_rgba32_float,
 * preserved[0] "raster_pixels_decoded_to_linear_light" (no evaluation_policy),
 * decoding "profile_trc_to_linear_light" and tone_map {operator:"clip"}.
 * Blocked rows, each naming the record or (id 0) the document: a blend mode,
 * effect blend mode or adjustment the float mode refuses, judged on the record
 * as it is; an effective warp binding (a stale one is a rendering-difference
 * row and is dropped); a profile with no matrix/TRC model or one failing the
 * precision check; and, only when nothing else blocked, a result too large to
 * save and reopen at 32 bits (the native plan's own sentence). A float source
 * is an error, not a row. Admission: output_bytes = 16 B/px of every converting
 * raster; scratch_bytes = the promotion's working memory (and, from 8 bits, the
 * 8->16 plan's scratch and output); the op admits and reserves the whole plan
 * before any plane is built. The preview is dr_precision_doc_narrow_structure_preview
 * with the converted float document first and the source as the converted handle. */
char *dr_precision_doc_promote_to_float_plan(const DrPrecisionDocument *document, char **err_out);
DrPrecisionDocument *dr_precision_doc_promote_to_float(const DrPrecisionDocument *document,
    uint64_t limit, uint64_t other_retained_bytes, const DrJobControl *control, char **err_out);
char *dr_doc_promote_to_float_plan(const DrDocument *document, char **err_out);
DrPrecisionDocument *dr_doc_promote_to_float(const DrDocument *document, uint64_t limit,
    uint64_t other_retained_bytes, const DrJobControl *control, char **err_out);
/* Explicit byte export pixels; source profile must be carried by the caller.
 * Source alpha and hidden RGB are independently quantized. Extra scratch=4*N+
 * 1 MiB. Returned DrImage uses dr_image_free and is never a typed master. */
DrImage *dr_precision_image_quantize(const DrPrecisionImage *image, uint32_t dither,
    uint64_t limit, uint64_t other_retained_bytes, const DrJobControl *control, char **err_out);
/* Sample JSON: exact center_rgba16 (or null outside), sum_rgba16/sample_count,
 * mean_rgba16 and normalized_rgba. Reach 0/1/2 means a 1/3/5 square; outside dropped,
 * transparent hidden RGB included in straight mean. Profile label is bounded. */
char *dr_precision_image_sample(const DrPrecisionImage *image, int32_t x, int32_t y,
    uint32_t reach, char **err_out);
/* Core-owned operation/target/depth family decision. Unknown values fail closed
 * with supported=false; NULL/invalid UTF8/string>64 bytes returns an error.
 * Individual operation planners still enforce all parameter/resource limits. */
char *dr_precision_capability(const char *operation, const char *target, uint32_t depth, char **err_out);

/* Typed immutable metadata inspection. Borrowed data remains valid while its
 * document owner remains alive. NULL length returns NULL; absent data sets0.
 * Kind is the existing DrMetadataKind (0 EXIF,1 XMP,2 IPTC). */
const uint8_t *dr_precision_doc_profile(const DrPrecisionDocument *document, size_t *length);
const uint8_t *dr_precision_doc_metadata(const DrPrecisionDocument *document, uint32_t kind, size_t *length);
/* Owned canonical adjustment JSON; free with dr_string_free. Non-adjustment or
 * invalid index returns NULL/no error; NULL document reports through err_out. */
char *dr_precision_doc_adjustment_description(const DrPrecisionDocument *document, size_t index, char **err_out);
/* Which of typed record idx's lock bits would block an edit of `kind` (0 =
 * allowed) — dr_doc_lock_block's twin at high depth, with exactly its rules,
 * enclosing groups included.
 * The host asks this to phrase a refusal that names the lock; the typed ops
 * refuse regardless, so a host that forgets cannot get through. 0 on a NULL
 * document, an out-of-range idx and an unrecognized kind. */
uint32_t dr_precision_doc_lock_block(const DrPrecisionDocument *document, size_t idx, DrEditKind kind);
/* dr_doc_lock_owner's twin at high depth: the typed record (postorder index)
 * whose own bits hold every bit of `locks`, idx first and then its enclosing
 * groups outward. False, with nothing written, on NULL, an empty `locks`, an
 * out-of-range idx or no match. out_idx may be NULL. */
bool dr_precision_doc_lock_owner(const DrPrecisionDocument *document, size_t idx, uint32_t locks, size_t *out_idx);
/* dr_doc_layer_bounds' twin at high depth, 16-bit and 32-bit float:
 * the record's CONTENT bounds — the canvas box of its OPAQUE pixels with its
 * enabled mask applied, and for a group (isolated or Pass Through) the union
 * over its raster descendants — written to out_xywh as x, y, width, height.
 * idx is the postorder record index. Not clipped to the canvas, and blind to
 * the visible flag. A mask whose feather cannot be prepared counts as no
 * mask. False (nothing written) on NULL document, out-of-range idx, an
 * adjustment record, or nothing opaque. out_xywh may be NULL. */
bool dr_precision_doc_layer_bounds(const DrPrecisionDocument *document, size_t idx, int32_t *out_xywh);
/* dr_doc_fill_align_box's twin at high depth, 16-bit, 32-bit float and
 * CMYK: the same rule over postorder record idx's u16 mask plane. False
 * (nothing written) on NULL document or an out-of-range idx; out_xywh may be
 * NULL. */
bool dr_precision_doc_fill_align_box(const DrPrecisionDocument *document, size_t idx, int32_t *out_xywh);
/* dr_doc_layer_at's twin at high depth, 16-bit and 32-bit float: the
 * postorder index of the record a click at canvas (x, y) activates, written
 * to out_idx. Same walk as the byte one: topmost first, hidden records
 * skipped with their subtrees, adjustment records skipped, a group hit when
 * a descendant hits (its own mask ignored); with top_level the answer is the
 * hit record's top-level ancestor. A raster record hits when its normalized
 * alpha, clamped to [0, 1] (NaN misses), times its enabled mask's effective
 * coverage (feather, density, outside coverage; an unpreparable feather
 * counts as no mask) is at least 0.5 — exactly the byte rule for a 16-bit
 * promotion at density 1 without feather, while density and feather are
 * evaluated at typed precision and may flip an edge pixel. False (nothing
 * written) on NULL document or when nothing is hit. out_idx may be NULL. */
bool dr_precision_doc_layer_at(const DrPrecisionDocument *document, int32_t x, int32_t y, bool top_level, size_t *out_idx);

/* Embedded ordinary-image source operations. Source input for place is a decoded
 * single plain raster document at the SAME native depth, carrying its original
 * ICC profile. It is optional/ignored for transform and rasterize. Strict JSON:
 * {op:"place",source_id:"persistent-id",name:"Image",x:0,y:0};
 * place also takes the OPTIONAL pair anchor:<size_t> and
 * position:"above"|"top"|"into_group" — the one slot rule of
 * dr_doc_adding_layer_at, whose comment IS that rule. Both present places the
 * new layer at that slot (joining a clip run, lock-EXEMPT, into a locked group
 * included); both absent is the legacy behaviour, a new TOP ROOT; one without
 * the other refuses ("placement position requires both an anchor layer and a
 * position"). The PLAN forecasts exactly the document the edit produces, so a
 * positioned plan is the positioned edit's forecast and not a pushed one.
 * {op:"transform",layer:0,matrix:[a,b,c,d,tx,ty],filter:"lanczos3"};
 * transform takes an optional "interactive":true, which the 16-bit ⌘T
 * PREVIEW of a placed image forwards from its "affine" and which keeps the
 * interactive pixel-work limit; without it a typed transform COMMIT is
 * bounded by the larger one-shot limit (the "straighten" block's): the plan
 * carries no work refusal for a commit, a controlled commit has no work
 * ceiling, and only a run with NULL control keeps that limit; every source
 * edit but a transform PREVIEW follows that rule. Over a limit:
 * "source transform exceeds the bounded sampling work limit; rasterize the
 * layer or use a smaller size". The byte request ignores the key.
 * {op:"rasterize",layer:0}. Transform is incremental in canvas coordinates;
 * master ICC conversion precedes sampling from accumulated affine. Linked masks
 * follow the incremental matrix. Doctored preserves fallback pixels exactly.
 * Plan JSON reports unique retained_bytes, additional scratch_bytes, work_units.
 * Edits are atomic, single-use job controlled, and return NULL/no error for no-op.
 * Info returns valid:false for a plain/destructively detached layer, otherwise
 * source_id, source width/height/depth/profile, affine/filter and embedded:true.
 * All JSON/error strings are owned and freed with dr_string_free.
 * On a Grayscale document (#1) place folds the rendered pixels to gray
 * (Mode > Grayscale's rule, neutral pixels kept) and keeps no retained source:
 * the new layer is a plain raster, so a following rasterize changes nothing,
 * and the plan adds the folded image (8 bytes a pixel, 16 at 32 bits).
 * On a Lab Color document (#1) place renders into the built-in ProPhoto RGB
 * at u16, converts that raster to L*a*b* codes by Image > Mode > Lab Color's
 * rule (the 8-bit grid at 8 bits) and keeps no retained source; the plan
 * prices the colour visit against ProPhoto and adds the converted image (8
 * bytes a pixel) and the conversion pipeline's scratch. An asset already in
 * the Lab twin (Load Files into Stack's Lab clip) places as before, unchanged.
 * The byte document plans and edits through dr_doc_mixed_source_plan / _edit
 * below, with source16 NULL. */
char *dr_doc_source_info(const DrDocument *document, size_t index, char **err_out);
char *dr_precision_doc_source_plan(const DrPrecisionDocument *document,
    const DrPrecisionDocument *source, const char *request, char **err_out);
DrPrecisionDocument *dr_precision_doc_source_edit(const DrPrecisionDocument *document,
    const DrPrecisionDocument *source, const char *request, uint64_t limit,
    uint64_t other_retained_bytes, const DrJobControl *control, char **err_out);
char *dr_precision_doc_source_info(const DrPrecisionDocument *document, size_t index, char **err_out);

/* Source revisions and links. Exactly one byte/typed input is required by place
 * and replace; all other operations accept NULL inputs. Existing place adds
 * optional revision_id (default source_id), link:null|{path,sha256}, and
 * depth_conversion:"reject"|"convert_fallback" (default reject).
 * Replace: {op:"replace",layer,revision_id,link:null|{path,sha256},depth_conversion}.
 * Replace preserves source_id, affine/mask/style/layer properties and requires a
 * fresh revision_id. Links are metadata only: core performs no filesystem IO.
 * Actual source depth/profile remains cached. Explicit cross-depth evaluation
 * promotes byte samples by 257 before typed processing, or quantizes a final
 * typed fallback once to nearest RGBA8, without altering the original master.
 * Place and replace (only) take an optional profile_refusal:"<sentence>":
 * the core open's profile-refusal report for the file, relayed
 * verbatim by the host, 1–4096 bytes of UTF-8 without NUL. It is kept on the
 * immutable source revision, so it follows transform/duplicate and
 * depth/profile conversion; replace sets it from the new file (absent clears
 * it); rasterize drops it with the binding. Info always reports
 * profile_refusal: null | string. A .rz holding one writes word 56 (byte) /
 * 57 (typed) inside 45/46 and an RSPR table before LIDS; an older byte build
 * refuses it as "unsupported DRDC version 56", an older typed build with its
 * generic "not a supported … high-depth DRDC document" sentence.
 * A 32-bit FLOAT document (retained_source_float) places,
 * transforms, replaces, captures and rasterizes an 8- or 16-bit master with
 * no depth_conversion authorization (reject is admitted): the master keeps
 * its own precision and the fallback is rendered into linear light in the
 * document profile (convert first, then resample the f32 plane, the order
 * measured against Photoshop 26.10.0); info reports fallback_depth 32 and
 * depth_conversion "convert_fallback". A 32-bit float source document is
 * admitted there as a 32-bit FLOAT MASTER (place, replace):
 * kept as rgba32_float linear light in its own profile, converted into the
 * document profile (the clipboard promotion's matrix, no clamp) and
 * resampled; info reports depth / master_depth 32, storage "rgba32_float"
 * and depth_conversion "none"; a converted sample past +/-65504 refuses
 * ("the placed image's colours convert to a value outside the 32-bit float
 * range (±65504) in the document's profile; the place is refused rather than
 * clipped"). An 8- or 16-bit document admits a float source document as a
 * 32-bit float master too, under the same rule as any other
 * master of a different depth: reject refuses ("source and document depths
 * differ; explicit convert_fallback authorization is required") and
 * convert_fallback renders it by the rule measured against Photoshop 26.10.0:
 * converted into the document profile in linear light, then clipped to [0, 1]
 * and encoded by the document profile's curve (16-bit codes, quantized at 8),
 * then resampled; info reports master_depth 32, storage "rgba32_float" and
 * depth_conversion "convert_fallback", and the same +/-65504 refusal
 * applies; a document profile with no matrix/TRC model refuses ("“<profile>”
 * has no matrix/TRC model this build can decode, so it cannot render a
 * 32-bit float master"). Narrowing a float document to 16 or 8 bits carries
 * a float master's binding. In a float document an 8- or 16-bit master whose
 * profile has no matrix/TRC model (A2B/B2A tables) renders through its table
 * into linear light, unclamped; a float document's own profile with
 * none, and a float master's, refuse ("placing into a 32-bit float document
 * requires an RGB matrix/TRC source profile").
 * Limits/cancellation/owned JSON/freeing follow the source APIs above. */
char *dr_doc_mixed_source_plan(const DrDocument *document, const DrDocument *source8,
    const DrPrecisionDocument *source16, const char *request, char **err_out);
DrDocument *dr_doc_mixed_source_edit(const DrDocument *document, const DrDocument *source8,
    const DrPrecisionDocument *source16, const char *request, uint64_t limit,
    uint64_t other_retained_bytes, const DrJobControl *control, char **err_out);
char *dr_precision_doc_mixed_source_plan(const DrPrecisionDocument *document,
    const DrDocument *source8, const DrPrecisionDocument *source16,
    const char *request, char **err_out);
DrPrecisionDocument *dr_precision_doc_mixed_source_edit(const DrPrecisionDocument *document,
    const DrDocument *source8, const DrPrecisionDocument *source16,
    const char *request, uint64_t limit, uint64_t other_retained_bytes,
    const DrJobControl *control, char **err_out);

/* Host decoder intake: exact native-endian straight RGBA16 samples, count=w*h*4,
 * and complete RGB matrix/TRC ICC bytes. No profile/default/depth substitution.
 * Caller sample/profile bytes and owned copies participate in admission; other
 * counts remaining live owners. Single-use token, atomic publication.
 * layer_name names the one raster layer; NULL means "Background". A name over
 * the DRDC name limit or not valid UTF-8 returns NULL and sets err_out. */
DrPrecisionDocument *dr_precision_doc_from_rgba16(const uint16_t *samples,
    size_t sample_count, uint32_t width, uint32_t height, const uint8_t *profile,
    size_t profile_length, const char *layer_name, uint64_t limit,
    uint64_t other_retained_bytes, const DrJobControl *control, char **err_out);

/* The float twin of dr_precision_doc_from_rgba16 (the 32-bit float Camera RAW
 * development is its caller): exact native-endian straight RGBA f32 samples,
 * count=w*h*4, LINEAR LIGHT in the primaries of the complete RGB matrix/TRC ICC
 * bytes. The result is a 32-bit float document under the clip tone map with one
 * raster layer. Nothing is repaired: a non-finite sample, an RGB magnitude
 * above 65504 or an alpha outside 0…1 returns NULL and sets err_out. The same
 * admission, token and layer_name rules as the u16 intake, at 16 bytes a pixel. */
DrPrecisionDocument *dr_precision_doc_from_rgba32(const float *samples,
    size_t sample_count, uint32_t width, uint32_t height, const uint8_t *profile,
    size_t profile_length, const char *layer_name, uint64_t limit,
    uint64_t other_retained_bytes, const DrJobControl *control, char **err_out);

/* Blank 16-bit document (File > New... at 16 bits): one raster layer of
 * width x height filled with the four straight RGBA16 samples at fill (read
 * as exactly four), tagged with complete RGB matrix/TRC ICC bytes. layer_name
 * names the layer; NULL means "Background". A canvas too large to reopen at 16
 * bits returns NULL and sets err_out to one sentence naming the size, the
 * working limit and the way forward (a smaller canvas or 8 bits);
 * NULL/empty/oversized/unsupported profile bytes and a NULL fill are refused
 * by name. The profile and its parsed state are admitted against limit with
 * other_retained_bytes counting remaining live owners. */
DrPrecisionDocument *dr_precision_doc_create_blank(uint32_t width, uint32_t height,
    const uint16_t *fill, const uint8_t *profile, size_t profile_length,
    const char *layer_name, uint64_t limit, uint64_t other_retained_bytes,
    char **err_out);

/* Phase 12G typed clipboard (precision_clipboard / precision_clipboard_codec /
 * precision_clipboard_convert). A payload is one window of raw samples in the
 * source storage/profile with its canvas offset, PPI and optional u16
 * coverage; a float payload carries its source's tone map, and a payload made
 * from 8-bit pixels carries the byte-origin flag (samples exactly v*257).
 * A whole-layer Copy of an editable layer may also carry the host's
 * EDITABLE SECTION (12G-3). The handle owns the payload AND its RZCB v3
 * encoding (little-endian: "RZCB", u32 version=3, u32 storage 1=u16/2=f32,
 * u32 w, u32 h, i32 ox, i32 oy, f32 ppi_x, f32 ppi_y, u32 profile_len +
 * ICC, u8 flags (bit0 coverage, bit1 byte origin, bit2 editable), u32
 * tone_map 0 clip/1 reinhard, f32 white, u32 editable_length, w*h*4
 * samples, then w*h u16 coverage if bit0, then editable_length section
 * bytes). bit2 is set iff editable_length > 0; the section is UTF-8, at most
 * 16 MiB, a JSON object of string "type", non-empty string "record",
 * optional string "name" and optional "anchor" [x, y] (two finite numbers)
 * and nothing else; the core never interprets "record". A u16 payload
 * carries tone_map 0; bit1 is refused on f32 and unless every u16 sample is
 * a multiple of 257. RZCB v2 (no editable_length word, flags <= 3) and v1
 * (flag byte = has_coverage, no tone-map words, f32 reads as clip) still
 * decode; the encoder writes v3. Every read admits the payload plus
 * its encoding and the host's public-rendering stage before allocating
 * (3 * payload + a fitted per-window-pixel term); Copy Merged also admits
 * the render. Copy
 * reads a raster record's raw samples (latent RGB kept); both copies multiply
 * alpha by coverage. Paste never refuses another storage or profile for the
 * mismatch: the payload is converted into the
 * document's depth and profile first — into 16 bits f32 -> u16 under the
 * payload's tone map, then the profile conversion; into a 32-bit float
 * document a u16 payload is PROMOTED (decoded to linear light through the
 * profile's TRC, then the source -> destination matrix) and an f32 payload
 * in another profile takes the matrix. The request's "profile" chooses
 * "convert" (default: keep the appearance) or "preserve" ("Don't Convert":
 * keep the numbers, relabelled with the document's
 * profile); depth always converts. A float output outside the stored range
 * refuses rather than clipping. Errors via err_out, freed with
 * dr_string_free.
 *
 * CMYK (#1 phase 4c, cmyk_clipboard / cmyk_clipboard_codec /
 * cmyk_payload_convert). A CMYK document copies, cuts and copies merged its
 * raw ink (a group its raw projection, an adjustment record refused) as a
 * CMYK payload: RZCB v4 — v3's layout with storage 3 = CMYKA16 (w*h*5 u16
 * [c,m,y,k,a], ink 0 = none) and one more u32 after editable_length, the
 * source's declared CMYK depth (8|16); the profile is the CMYK policy's
 * verbatim bytes; flags may carry bit0 (coverage) and bit2 (editable), the
 * tone map is 0, and the editable section (#1's artwork batch: v3's cap,
 * shape and flag rule) is the editable_length UTF-8 bytes right after the
 * depth word, before the samples. A non-4 version read as CMYK refuses "clipboard payload
 * version N is not a CMYK payload (CMYK payloads are version 4)"; storage
 * other than 3 under v4 "clipboard payload version 4 holds only CMYK
 * storage; storage code N is not CMYK"; the RGB reader still refuses v4 at
 * its version check. RGB payload bytes are unchanged (v3). A paste between
 * CMYK documents of the SAME profile bytes keeps the ink bit for bit at any
 * declared depth; another CMYK profile converts (CMYK -> PCS -> CMYK,
 * relative colorimetric with BPC, at the destination's declared depth) under
 * "convert" and keeps the ink under "preserve". Across modes — an RGB
 * payload into a CMYK document, a CMYK payload into an RGB document — the
 * pixels always convert, relative colorimetric with BPC through the
 * profiles' own tables (an RGB payload first brought to 16 bits in its own
 * profile; into float the 16 -> float promotion), and "preserve" refuses:
 * "Don't Convert keeps numbers within one colour mode; pixels pasted between
 * RGB and CMYK are always converted". dr_clipboard_payload_image answers
 * NULL WITHOUT an error for a CMYK payload; dr_clipboard_payload_editable and
 * dr_clipboard_payload_with_editable read and attach its section as an RGB
 * payload's, and every conversion carries it; dr_clipboard_payload_rgba8 converts
 * it into `profile` (NULL = Display P3) and refuses preserve;
 * dr_precision_doc_from_clipboard makes a CMYK document of its policy and
 * depth; dr_clipboard_payload_trim trims it by alpha.
 *
 * GRAYSCALE (#1 phase 4, gray_clipboard): Copy, Copy Merged and Cut of a
 * Grayscale document make a Gray payload, RZCB v5: b"RZCB", u32 5, u8 depth
 * (8/16/32), u32 ICC length + the Gray ICC, then one complete v3 stream of
 * R = G = B in the Gray profile's twin (f32 storage exactly at 32). The
 * decoder refuses a bad depth, an unusable Gray ICC, an inner stream that is
 * not v3, an inner profile not byte-equal to the twin, storage disagreeing
 * with the depth, and trailing bytes. Its info is the inner info with
 * "version":5, "mode":"gray", "gray_depth" and "gray_profile" (the name);
 * dr_clipboard_payload_icc answers the Gray ICC. Between Gray documents the
 * same Gray ICC pastes bit for bit; another Gray profile converts under
 * "convert" (the luminance through the source twin, encoded into the
 * destination's) and keeps the numbers under "preserve". Into a Gray
 * document an RGB payload converts by Image > Mode > Grayscale's luminance,
 * a CMYK payload through ProPhoto RGB first; a Gray payload into an RGB
 * document is the RGB paste of its twin pixels, into a CMYK document their
 * RGB -> CMYK conversion. Across modes "preserve" refuses: "Don't Convert
 * keeps numbers within one colour mode; pixels pasted between Grayscale and
 * another mode are always converted". dr_clipboard_payload_image answers its
 * R = G = B pixels; dr_clipboard_payload_rgba8 reads them as an RGB
 * payload's and refuses preserve with a profile (the same sentence);
 * dr_precision_doc_from_clipboard makes a Gray document of its policy;
 * editable and trim work as on an RGB payload.
 *
 * LAB COLOR (#1 Lab phase 4c, lab_clipboard): Copy, Copy Merged and Cut of
 * a Lab Color document make a Lab payload, RZCB v6: b"RZCB", u32 6, u8 depth
 * (8/16), then one complete v3 stream of the L*a*b* codes in the Lab twin,
 * RGBA16. The decoder refuses, before any sample is allocated, a bad depth,
 * an inner stream that is not v3, inner storage that is not RGBA16, an inner
 * profile not byte-equal to the twin, the byte-origin flag, and trailing
 * bytes. Its info is the inner info with "version":6, "mode":"lab" and
 * "lab_depth". Between Lab documents "convert" and "preserve" both keep the
 * codes; a payload of either depth into an 8-bit document lands on the x257
 * grid (phase 6a-i: an 8-bit payload may hold off-grid codes), into a 16-bit
 * document as it is.
 * Into a Lab document an RGB payload converts by Image > Mode > Lab Color's
 * rule (relative colorimetric, no BPC; a float payload first to 16 bits under
 * its tone map), a Gray payload from its RGB twin, a CMYK payload through
 * 16-bit ProPhoto RGB first; a Lab payload into an RGB document converts
 * into its profile relative colorimetric with BPC, into a Gray or CMYK
 * document through 16-bit ProPhoto RGB and then that mode's RGB paste. Across
 * modes "preserve" refuses: "Don't Convert keeps numbers within one colour
 * mode; pixels pasted between Lab Color and another mode are always
 * converted". dr_clipboard_payload_image answers its codes;
 * dr_clipboard_payload_rgba8 converts them into `profile` (NULL = Display
 * P3, the Lab view's space) and refuses preserve with a profile (the same
 * sentence), its progress reaching 1 over the conversion and the 8-bit read;
 * dr_precision_doc_from_clipboard makes a Lab document of its depth (an
 * 8-bit payload on the x257 grid first); editable and trim work as on an RGB
 * payload. */
typedef struct DrClipboardPayload DrClipboardPayload;
/* request {"op":"copy","layer":N,"x","y","width","height"} or
 * {"op":"copy_merged","x","y","width","height"}; coverage = canvas-sized u16 or
 * NULL with length 0. A group layer copies its raw projection (its children
 * composited without the group's own opacity, blend, mask, style or Blend
 * If). Free the result with dr_clipboard_payload_free. */
DrClipboardPayload *dr_precision_doc_clipboard_copy(const DrPrecisionDocument *document,
    const char *request, const uint16_t *coverage, size_t coverage_length,
    uint64_t limit, uint64_t other_retained_bytes, const DrJobControl *control,
    char **err_out);
/* request {"op":"cut","layer":N,"x","y","width","height"}; NULL document
 * WITHOUT error = the erase changed no byte (transparency lock), payload_out
 * still set. On error payload_out is NULL. A float document erases alpha
 * (a * (1 - c/65535)) with RGB retained. An Indexed Color or Bitmap
 * document's request may add "indexed_background" (the Indexed and Bitmap
 * sections; any other document refuses the key as an unknown field); a
 * Bitmap Cut's pixels take the background's bit, white by default. */
DrPrecisionDocument *dr_precision_doc_clipboard_cut(const DrPrecisionDocument *document,
    const char *request, const uint16_t *coverage, size_t coverage_length,
    uint64_t limit, uint64_t other_retained_bytes, const DrJobControl *control,
    DrClipboardPayload **payload_out, char **err_out);
/* request {"name","anchor","position":"above"|"top"|"into_group",
 * "placement":"centered"|"in_place"|"into","frame"?:[x,y,w,h],
 * "profile"?:"convert"|"preserve" (default "convert")}. "into" requires
 * "frame" (positive size) and centres the payload on it (floor of half the
 * difference, per axis); a frame with any other placement refuses. */
DrPrecisionDocument *dr_precision_doc_clipboard_paste(const DrPrecisionDocument *document,
    const DrClipboardPayload *payload, const char *request, uint64_t limit,
    uint64_t other_retained_bytes, const DrJobControl *control, char **err_out);
/* Decode RZCB bytes; refuses (never sanitizes) a malformed stream. */
DrClipboardPayload *dr_clipboard_payload_decode(const uint8_t *bytes, size_t length,
    uint64_t limit, uint64_t other_retained_bytes, char **err_out);
/* Borrowed RZCB bytes, valid until the handle is freed. */
const uint8_t *dr_clipboard_payload_bytes(const DrClipboardPayload *payload,
    size_t *length_out);
/* Owned JSON {"version":3,"mode":"rgb","storage","depth","origin_depth":8|16|32,
 * "width","height","offset":[x,y],"ppi":[x,y],"profile_description",
 * "has_coverage","tone_map"?:{"operator",...} (f32 only), "editable": null |
 * {"type","name"?,"anchor"?,"record_bytes"}}; a CMYK payload answers
 * {"version":4,"mode":"cmyk","storage":"cmyka16_unorm","depth":16,
 * "origin_depth","cmyk_depth":8|16,"width","height","offset","ppi",
 * "profile_description","has_coverage","editable":null}; free with
 * dr_string_free. */
char *dr_clipboard_payload_info(const DrClipboardPayload *payload, char **err_out);
/* Cloned shared-pixel owner; free with dr_precision_image_free. */
DrPrecisionImage *dr_clipboard_payload_image(const DrClipboardPayload *payload);
/* A byte-origin payload from straight RGBA8 (length = w*h*4): u16 v*257, no
 * coverage, exact matrix/TRC profile bytes, the canvas offset and PPI. Admits
 * the input, the payload and its encoding before allocating. Free with
 * dr_clipboard_payload_free. */
DrClipboardPayload *dr_clipboard_payload_from_rgba8(const uint8_t *rgba, size_t length,
    uint32_t width, uint32_t height, const uint8_t *profile, size_t profile_length,
    int32_t offset_x, int32_t offset_y, float ppi_x, float ppi_y, uint64_t limit,
    uint64_t other_retained_bytes, char **err_out);
/* The payload as RGBA8 in `profile` (NULL with profile_len 0 = the payload's
 * own profile; other bytes must be an RGB ICC profile a document can carry).
 * preserve=false converts another profile's pixels (keeping their
 * appearance), preserve=true keeps their numbers; a float payload maps
 * through its own tone map. Free with dr_image_free. */
DrImage *dr_clipboard_payload_rgba8(const DrClipboardPayload *payload,
    const uint8_t *profile, size_t profile_len, bool preserve, uint64_t limit,
    uint64_t other_retained_bytes, const DrJobControl *control, char **err_out);
/* New from Clipboard: one "Background" record at (0,0) at the payload's own
 * depth, profile, dimensions and PPI (a float payload's tone map carried);
 * offset dropped. Free with dr_precision_doc_free. */
DrPrecisionDocument *dr_precision_doc_from_clipboard(const DrClipboardPayload *payload,
    uint64_t limit, uint64_t other_retained_bytes, const DrJobControl *control,
    char **err_out);
/* A new handle carrying `section` as its editable section (replacing any
 * existing one), validated exactly as the decoder validates one. Admits,
 * before parsing or copying, the new handle (payload bytes + profile bytes
 * + twice the section) and the parse (bookkeeping + twice the section).
 * Free with dr_clipboard_payload_free. */
DrClipboardPayload *dr_clipboard_payload_with_editable(const DrClipboardPayload *payload,
    const char *section, uint64_t limit, uint64_t other_retained_bytes, char **err_out);
/* The owned editable section; NULL WITHOUT an error when the payload carries
 * none. Free with dr_string_free. */
char *dr_clipboard_payload_editable(const DrClipboardPayload *payload, char **err_out);
/* New from Clipboard's trim: a new handle whose samples are exactly the
 * payload's content box (every pixel with alpha > 0) — latent RGB kept,
 * coverage windowed the same way, offset moved by the box origin (an i32
 * overflow refuses), PPI, byte origin, tone map and editable section carried.
 * NULL WITHOUT an error when there is nothing to trim (fully transparent, or
 * the box is already the whole window); NULL + message for a NULL payload or
 * a refusal. Admits the bookkeeping first, then the new handle (twice the
 * samples + coverage + section, plus the profile bytes) before allocating.
 * Free with dr_clipboard_payload_free. */
DrClipboardPayload *dr_clipboard_payload_trim(const DrClipboardPayload *payload, uint64_t limit,
    uint64_t other_retained_bytes, char **err_out);
void dr_clipboard_payload_free(DrClipboardPayload *payload);
/* The payload's profile bytes — the RGB ICC profile or the CMYK policy's
 * verbatim bytes — BORROWED until the handle is freed; NULL and *length_out
 * 0 for a NULL handle. */
const uint8_t *dr_clipboard_payload_icc(const DrClipboardPayload *payload, size_t *length_out);
/* A Gray payload's one-channel TIFF (gray, plus straight alpha only when some
 * pixel is not opaque; 8 bits, round(v/257), for a depth-8 payload, else 16,
 * a 32-bit payload reduced under its own tone map; the Gray ICC embedded),
 * made on the first call and BORROWED until the handle is freed. NULL,
 * *length_out 0 and an error for a NULL handle or a payload that is not
 * Gray. Admits 8 B a pixel first. */
const uint8_t *dr_clipboard_payload_gray_tiff(const DrClipboardPayload *payload, uint64_t limit,
    uint64_t other_retained_bytes, size_t *length_out, char **err_out);
/* A Gray payload from a gray TIFF on the system pasteboard (BlackIsZero, 1-2
 * unsigned 8/16-bit samples): R = G = B at its depth, offset (0,0). NULL with
 * no error: the bytes are not a gray TIFF. Its embedded Gray ICC is kept,
 * and an unusable one is an error, as a gray open refuses it; an untagged
 * TIFF is assigned fallback_icc (NULL with length 0 for none; used only when
 * a usable Gray profile), else the working Gray named by working_gray, and
 * *assigned_out (NULL-able, dr_string_free) receives the untagged sentence;
 * NULL otherwise. Admitted as a gray open. Free with
 * dr_clipboard_payload_free. */
DrClipboardPayload *dr_clipboard_payload_from_gray_image(const uint8_t *bytes, size_t length,
    const uint8_t *fallback_icc, size_t fallback_length, const char *working_gray, uint64_t limit,
    uint64_t other_retained_bytes, char **assigned_out, char **err_out);
/* A CMYK payload from a CMYK TIFF or JPEG on the system pasteboard: its ink,
 * declared depth and PPI at offset (0,0), its embedded profile when usable.
 * NULL with no error: the bytes are not a CMYK TIFF/JPEG cmyk_source answers
 * (an untagged one while no working CMYK profile is set included).
 * *assigned_out (NULL-able, dr_string_free) is the untagged sentence when the
 * fallback (else the working CMYK) profile was assigned, NULL otherwise.
 * Free with dr_clipboard_payload_free. */
DrClipboardPayload *dr_clipboard_payload_from_cmyk_image(const uint8_t *bytes, size_t length,
    const uint8_t *fallback_icc, size_t fallback_length, uint64_t limit,
    uint64_t other_retained_bytes, char **assigned_out, char **err_out);

/* The LAYER clipboard (doc_paste_layers): paste every record of
 * `layers` — a layer clipboard, the document Duplicate to New Document built
 * from the copied entries — into `doc` at the slot `position`
 * (DrInsertPosition's code) names relative to `anchor`, translated by
 * (dx, dy) first through the Move tool's rule (raster offsets and linked
 * masks move, a group moves only its linked mask, an unlinked mask stays;
 * no lock gate) and skipped at (0, 0). Every record mints a fresh id, each
 * distinct link id is re-numbered to the next unused one in the result, and
 * every ROOT takes the slot's clip flag OR its own, so the block joins a clip
 * run. Typed rasters and adjustments are re-bound to the document's profile
 * handle; the document's channels, comps, guides, evaluation, global light
 * and tone map are kept. At 8 and 16 bits a clip of another profile is
 * refused under convert (`preserve` false); under `preserve` true (Don't
 * Convert) it is RELABELLED after the Assign check: refused
 * in the Assign sheet's words when the document's profile fails it for the
 * copied text/shape artwork or calibrated layers, otherwise every record
 * keeps its numbers, and a placed image effective
 * under the clip's profile stays bound, re-pointed without a re-render; one
 * that is not is dropped, its pixels kept. Refusals by name through err_out:
 * a NULL `layers`, an unknown position, the slot's refusals, "pasted layers
 * must be in the document's profile; convert them first" (convert only),
 * the Assign check's sentence (preserve only), (typed) "pasted layers must be at
 * the document's depth; convert them first", "pasting these layers here
 * would nest them N levels deep; the limit is L" (L = 10 at 8 bits, 16 at
 * high depth), and growth past native capacity. The typed paste admits
 * 4 MiB + 2 * (every name in both) + 4096 * (both record counts) + a fitted
 * per-destination-canvas-pixel margin over the document's retained bytes
 * before building. Neither input changes; one
 * result is one undo step. A NULL doc at 8 bits answers NULL with err_out
 * untouched; the typed export names a NULL handle.
 *
 * A 32-bit FLOAT `document` (layer_clipboard_float) never
 * answers the profile or the depth refusal: a clip of either depth and any
 * profile is PROMOTED into it under `preserve` (false converts into the
 * document's profile; true, Don't Convert, keeps the numbers and reads them
 * as the document's own encoding). A u16 plane is decoded to linear light,
 * an f32 plane of another profile takes the linear matrix under convert and
 * is relabelled under preserve. A placed image lands placed:
 * under convert its content and binding are re-rendered from the master
 * into the document's linear light, under preserve the binding is kept over
 * the promoted plane. A stale placed image, a text or shape description and
 * a warp binding are dropped and the record's own
 * pixels kept; a Pass Through group lands as
 * Pass Through; the nine blend modes a float document refuses
 * (color_burn, screen, color_dodge, overlay, soft_light, hard_light,
 * vivid_light, hard_mix, exclusion) land Normal (Photoshop 26.10.0 was
 * measured resetting these and linear_burn, linear_light and pin_light at
 * 32 bits, paste_layers.py; those three ride). A record the
 * float mode still cannot hold refuses the whole paste before any
 * admission: "the copied layer “NAME” cannot be pasted into a 32-bit float
 * document: FEATURE is unavailable in a 32-bit float document" (an effect
 * blend mode outside the float set, Divide, an adjustment the float mode
 * cannot evaluate). The promotion admits its own working memory (twice the
 * new f32 planes, plus the decode table, plus the largest placed re-render's
 * scratch) and then the insertion with those planes held, both
 * before any pixel work. `report_out` (may be NULL) receives, on success
 * only, an owned JSON string {"rasterized":[{"layer","reason"}],
 * "blend_reset":[{"layer","from"}],
 * "effect_blend_reset":[{"layer","effect","from","to"}]} — reason is "placed
 * image", "text or shape" or "warp"; every list is empty when
 * nothing was promoted. Free it with dr_string_free.
 *
 * CMYK (#1 phase 4c): modes never mix here — a CMYK document refuses a
 * clip that is not CMYK ("pasted layers must be CMYK to land in a CMYK
 * document; convert them to CMYK first") and an RGB document a CMYK clip
 * ("pasted CMYK layers must be converted to RGB before they land in an RGB
 * document"); the host converts first. Between CMYK documents a clip of the
 * same profile bytes splices as it is (ink kept bit for bit), one of another
 * profile converts every CMYK raster's ink, and every layer style's ink,
 * into the document's profile at its declared depth (relative colorimetric
 * with BPC) when `preserve` is
 * false and keeps it when true; a placement offset moves every CMYK raster
 * offset and every LINKED mask. The conversion admits its leg over every
 * leaf's pixels with the converted planes held.
 *
 * LAB COLOR (#1 Lab phase 4c): nor do Lab Color and the other modes — a
 * Lab clip into another document or another mode's clip into a Lab document
 * refuses, asked before the Grayscale check: "Lab Color layers paste only
 * into a Lab Color document, and a Lab Color document takes only Lab Color
 * layers: convert the clip first"; the host converts first. */
DrDocument *dr_doc_paste_layers(const DrDocument *doc, const DrDocument *layers, size_t anchor,
    uint32_t position, int32_t dx, int32_t dy, bool preserve, char **err_out);
DrPrecisionDocument *dr_precision_doc_paste_layers(const DrPrecisionDocument *document,
    const DrPrecisionDocument *layers, size_t anchor, uint32_t position, int32_t dx, int32_t dy,
    bool preserve, uint64_t limit, uint64_t other_retained_bytes, char **report_out,
    char **err_out);

/* RAW provenance is captured onto a fresh retained genuine-u16 asset, with
 * either fallback depth. First retain a one-raster typed document through
 * source_edit({op:"retain",source_id,revision_id?}, no source input), or use
 * the existing mixed Place/Replace. Attach never changes developed pixels.
 * Descriptor strict envelope: schema_version=1, original_filename,
 * content_sha256 (64 lowercase hex), decoder={identifier,version,implementation},
 * recipe={...nonempty effective host settings...}. Recipe semantics belong to
 * the host; unknown decoders remain readable. Descriptor<=64KiB; original
 * 1byte..256MiB; aggregate distinct originals<=512MiB. No decoder or file I/O.
 * Plan JSON uses retained_bytes/scratch_bytes/work_units; initial plan includes
 * caller's encoded bytes, and scratch includes the future copy. Returned strings
 * use dr_string_free; docs use their depth-specific free. From variants share
 * original allocation and require unchanged source identity/filename/fingerprint.
 * Host publishes Place/Replace+Attach as one change after all stages succeed.
 * The developed master is u16, or in a 32-bit FLOAT document also f32 (a 32-bit
 * float linear development): the four dr_precision_doc_* attach and plan
 * calls admit a depth-32 master there and refuse it by name in a 16-bit
 * document; the dr_doc_* calls always refuse it. */
char *dr_doc_raw_source_plan(const DrDocument *document, size_t layer,
    size_t encoded_length, const char *descriptor, char **err_out);
DrDocument *dr_doc_attach_raw_source(const DrDocument *document, size_t layer,
    const uint8_t *encoded, size_t encoded_length, const char *descriptor,
    uint64_t limit, uint64_t other_retained_bytes, const DrJobControl *control, char **err_out);
char *dr_doc_raw_source_from_plan(const DrDocument *document, size_t layer,
    const DrDocument *original, size_t original_layer, const char *descriptor, char **err_out);
DrDocument *dr_doc_attach_raw_source_from(const DrDocument *document, size_t layer,
    const DrDocument *original, size_t original_layer, const char *descriptor,
    uint64_t limit, uint64_t other_retained_bytes, const DrJobControl *control, char **err_out);
size_t dr_doc_raw_source_length(const DrDocument *document, size_t layer, char **err_out);
bool dr_doc_raw_source_copy(const DrDocument *document, size_t layer,
    uint8_t *output, size_t output_length, char **err_out);
char *dr_precision_doc_raw_source_plan(const DrPrecisionDocument *document, size_t layer,
    size_t encoded_length, const char *descriptor, char **err_out);
DrPrecisionDocument *dr_precision_doc_attach_raw_source(const DrPrecisionDocument *document,
    size_t layer, const uint8_t *encoded, size_t encoded_length, const char *descriptor,
    uint64_t limit, uint64_t other_retained_bytes, const DrJobControl *control, char **err_out);
char *dr_precision_doc_raw_source_from_plan(const DrPrecisionDocument *document, size_t layer,
    const DrPrecisionDocument *original, size_t original_layer, const char *descriptor, char **err_out);
DrPrecisionDocument *dr_precision_doc_attach_raw_source_from(const DrPrecisionDocument *document,
    size_t layer, const DrPrecisionDocument *original, size_t original_layer, const char *descriptor,
    uint64_t limit, uint64_t other_retained_bytes, const DrJobControl *control, char **err_out);
size_t dr_precision_doc_raw_source_length(const DrPrecisionDocument *document, size_t layer, char **err_out);
bool dr_precision_doc_raw_source_copy(const DrPrecisionDocument *document, size_t layer,
    uint8_t *output, size_t output_length, char **err_out);

/* RAW reads: length returns zero+error on refusal; copy requires an exact-sized
 * non-NULL, writable, nonoverlapping caller buffer. Validation failure writes
 * nothing. Handles and borrowed input/output remain live until return. */

/* Source-info publication scratch, counted without copying/serializing JSON.
 * Includes RAW descriptor bytes*768 plus 64KiB and ordinary source string costs.
 * NULL/plain/out-of-range returns 64KiB; overflow returns UINT64_MAX. The host must
 * admit this estimate before source_info and retain its lease while expanded
 * core/host recipe dictionaries remain live. No error output or side effects. */
uint64_t dr_doc_source_info_scratch_bytes(const DrDocument *document, size_t layer);
uint64_t dr_precision_doc_source_info_scratch_bytes(const DrPrecisionDocument *document, size_t layer);

/* Allocation-free retained-binding guard. NULL/out-of-range/plain or invalidated
 * source guards return false. This query is independent of full-info admission;
 * hosts use it to require explicit rasterization before destructive edits. */
bool dr_doc_has_retained_source(const DrDocument *document, size_t layer);
bool dr_precision_doc_has_retained_source(const DrPrecisionDocument *document, size_t layer);

/* Develop inspection uses genuine retained u16 masters BEFORE accumulated affine,
 * masks or retouch. The returned master shares samples/ICC and is released with
 * dr_precision_image_free. Plain/invalid/byte-only sources refuse with err_out. */
DrPrecisionImage *dr_doc_source_master16(const DrDocument *document, size_t layer, char **err_out);
DrPrecisionImage *dr_precision_doc_source_master16(const DrPrecisionDocument *document, size_t layer, char **err_out);
/* The retained developed master at its OWN depth: a 16-bit SDR development's
 * u16 master or a 32-bit float (linear) development's f32 one
 * (the layer's source info reports master_depth 16 or 32, and
 * dr_precision_image_pixels_rgba16 is NULL for f32). Shares samples/ICC; released
 * with dr_precision_image_free. Plain/invalid/byte-only sources refuse. */
DrPrecisionImage *dr_precision_doc_source_master(const DrPrecisionDocument *document, size_t layer, char **err_out);
/* The image the develop inspection calls below read, which are defined on u16
 * codes. A u16 image is shared as it is. An f32 image answers its SDR-clipped
 * view: RGB clamped to 0…1, encoded by the image's own profile curve and
 * rounded once to u16, alpha unchanged, so a sample at or above white reads
 * 65535 (what the highlight clipping marker then shows). The f32 case charges
 * 1MiB+16*w*h. Single-use token; released with dr_precision_image_free. */
DrPrecisionImage *dr_precision_image_develop_inspection(const DrPrecisionImage *image,
    uint64_t limit, uint64_t other_retained_bytes, const DrJobControl *control, char **err_out);

/* Histogram JSON: bins[4][256]=RGB/encoded Rec709 luma; alpha_bins[256]; total
 * counts alpha>0 RGB/luma, sampled_pixels counts all alpha. exact_zero_rgba and
 * exact_max_rgba count original 0/65535, independently of endpoint display bins.
 * stride 0/1 visits all, greater stride counts sampled positions only. No byte
 * intermediate or selection restriction. Explicit budget; 1MiB scratch. RAW
 * inspection uses stride1: all source pixels, exact endpoints independent of
 * 256 display bins. Higher strides are labeled sampled-only. */
char *dr_precision_image_histogram_checked(const DrPrecisionImage *image, uint32_t stride,
    uint64_t limit, uint64_t other_retained_bytes, const DrJobControl *control, char **err_out);

/* Exact in-bounds pixel-native detail, width/height1..1024; no scaling, quantizing
 * or ICC conversion. Scratch1MiB+8*w*h. Result uses dr_precision_image_free. */
DrPrecisionImage *dr_precision_image_develop_detail(const DrPrecisionImage *image,
    uint32_t x, uint32_t y, uint32_t width, uint32_t height, uint64_t limit,
    uint64_t other_retained_bytes, const DrJobControl *control, char **err_out);

/* Bounded DISPLAY ONLY. Region must be in bounds; divisor>=1; output dimensions
 * ceil(region/divisor) each<=1600. Divisor1 is one source pixel per output pixel;
 * larger divisors select floor(cell_width/2),floor(cell_height/2) from each
 * clipped cell. Ordinary samples use the shared nearest undithered u16->u8 rule.
 * Clipping flags0/1/2/3=none/RGB0/RGB65535/both, ignoring alpha-zero positions.
 * Flags OR EVERY original sample in each cell, so fit markers cannot miss a
 * clipped source pixel. Blue/red/magenta opaque markers mean zero/max/both;
 * they describe code endpoints, not recoverable sensor detail or gamut loss.
 * Caller carries original source ICC when displaying. Scratch1MiB+4*out_w*out_h,
 * work bounded by the source region, cancellation inside scans. Result uses
 * dr_image_free. All handles/token remain live through return; token single-use.
 * NULL/invalid/admission/cancel errors publish no result and report err_out. */
DrImage *dr_precision_image_develop_preview(const DrPrecisionImage *image,
    uint32_t x, uint32_t y, uint32_t width, uint32_t height, uint32_t divisor,
    uint32_t clipping, uint64_t limit, uint64_t other_retained_bytes,
    const DrJobControl *control, char **err_out);

/* The EXTENDED-RANGE preview of a 32-bit float development (#4), at Fit
 * or 100% (divisor 1). DISPLAY ONLY; rgba32_float storage only. Region must be
 * in bounds; divisor>=1; output dimensions ceil(region/divisor) each<=1600. Each
 * output pixel is the cell-centre sample dr_precision_image_develop_preview
 * picks, through display_encoding's rule X at gain 1 (RGB held to +/-65504,
 * times alpha; RGB and alpha each rounded to nearest-even binary16), written as
 * RGBA binary16 LE premultiplied, linear in the image's primaries (the host
 * wraps it in the profile's extended-linearized space). Clipping flags 0/1/2/3
 * select none/shadows/highlights/both: a cell any of whose alpha-positive
 * samples has an RGB channel <= 0.0 (shadows) or >= 1.0 (highlights) is written
 * opaque binary16 blue (0,0,1,1), red (1,0,0,1) or magenta (1,0,1,1). out_len
 * must equal out_w*out_h*8 (recomputed by the core). peak_out (nullable)
 * receives the largest RGB component of the picture (markers excluded);
 * marked_out (nullable) the count of marked output pixels. Its plan is the
 * extended display view's (46 bytes a pixel plus bookkeeping, float_view.rs)
 * at the output size, admitted under limit beside other_retained_bytes. false
 * with err_out set for u16 storage, a bad region, divisor, clipping or out_len,
 * admission and a cancel; `out` is written only on success. control is
 * single-use. */
bool dr_precision_image_develop_preview_extended(const DrPrecisionImage *image,
    uint32_t x, uint32_t y, uint32_t width, uint32_t height, uint32_t divisor,
    uint32_t clipping, uint8_t *out, size_t out_len, float *peak_out,
    uint64_t *marked_out, uint64_t limit, uint64_t other_retained_bytes,
    const DrJobControl *control, char **err_out);
/* Bits per channel of the image's sample storage: 16 for rgba16_unorm, 32 for
 * rgba32_float (it says which developed masters have an extended Fit preview).
 * 0 for NULL. Reads no pixel, allocates nothing. */
uint32_t dr_precision_image_storage(const DrPrecisionImage *image);

/* Clipping display (the ⌥-drag Levels/Curves threshold view).
 * DISPLAY ONLY; HIGHLIGHTS and SHADOWS are always opaque. HIGHLIGHTS: black ground, each RGB channel at
 * its maximum code (255 / 65535) lit to 255 in that channel. SHADOWS: white
 * ground, each RGB channel at 0 dropped to 0 in that channel. Alpha-zero
 * pixels show the ground. OVERLAY (View > Show Clipping) is
 * TRANSPARENT (0,0,0,0) except where a channel is clipped: any RGB channel at
 * 0 blue (0,0,255,255), any at the maximum red (255,0,0,255), both magenta
 * (255,0,255,255); alpha-zero pixels stay clear. The typed form tests exact
 * native u16 codes (65534 is not clipped), reads the tiles, admits
 * 1MiB+4*w*h against limit with other_retained, and refuses a bad mode, NULL
 * or admission through err_out. On rgba32 storage it tests linear values: a
 * channel >= 1.0 is at the maximum and one <= 0.0 at zero (inclusive, since
 * float Curves holds an inside black end point flat at exactly 0.0); a NaN or
 * non-positive alpha shows the ground and a NaN channel is never clipped. The byte form is a pure op:
 * NULL on NULL or a bad mode. Results use dr_image_free. */
#define DR_CLIP_DISPLAY_HIGHLIGHTS 1u
#define DR_CLIP_DISPLAY_SHADOWS 2u
#define DR_CLIP_DISPLAY_OVERLAY 3u
DrImage *dr_image_clip_display(const DrImage *img, uint32_t mode);
DrImage *dr_precision_image_clip_display(const DrPrecisionImage *image, uint32_t mode,
    uint64_t limit, uint64_t other_retained, char **err_out);

/* Native artwork. Paths are output-coordinate move/line/quad/cubic/close verbs
 * 0..4, with point_count xy pairs. Output is atomic u16 coverage. Curves flatten
 * within1/4096pixel control distance; polygon areas integrate continuously.
 * Caps 262144 points, 524288 flattened edges, 256M work; scratch
 * 106MiB+2*w*h+24*w before flattening.
 * Since 13A-6b the scan is an active-edge sweep (work n + Σ active per row, rows
 * outside the outline skipped); caps and sentences unchanged; equal to the full
 * scan and never charged above it. */
bool dr_artwork_path_coverage(const uint8_t *commands, size_t command_count,
    const double *points, size_t point_count, uint32_t width, uint32_t height,
    bool even_odd, uint16_t *output, size_t output_count, uint64_t limit,
    uint64_t other_retained, const DrJobControl *control, char **err_out);
/* Component JSON array (max256): {kind:"coverage",color:[r,g,b,a]} uses one
 * u16 plane; {kind:"intrinsic_rgba8",opacity:1} uses separate straight RGBA8
 * intrinsic bytes. Ordered assembly uses continuous authored sRGB/opacity and
 * the exact supplied RGB ICC. Total<=64Mscalar samples; returned genuineu16
 * owner uses dr_precision_image_free. Caller pixels never become display input.
 * Fill layers, neither owning a plane: {kind:"fill",color:[r,g,b,a]}
 * is that colour over the whole raster; {kind:"gradient",gradient:{...},
 * colors:[[r,g,b],...]} is a layer-style gradient object (dr_doc_set_layer_
 * style's `gradient`, stops in non-decreasing position) spanning the raster,
 * with one continuous colour per stop replacing the stops' #rrggbb. Colour
 * ranges are a coverage colour's. A fill may add linear:[r,g,b]: its
 * colour as linear-light sRGB components beyond sRGB, finite within +-4,
 * which then decides the colour (color's alpha still applies); at 8/16 bits
 * scratch adds one high-depth encoder. A gradient may add linear:[[r,g,b],...],
 * one such triple per stop in colors' order (same count),
 * each converted as a fill's linear is. A gradient may add box:[x,y,w,h]
 * (Align with Layer: dr_doc_fill_align_box / dr_precision_doc_fill_
 * align_box), the box in raster pixels it spans instead of the raster; a
 * width or height below 1, or a value outside int32, refuses "artwork
 * gradient box requires a positive width and height".
 * Into the Lab twin (#1 Lab phase 6a-i) every authored colour converts by
 * Image > Mode > Lab Color's rule (relative colorimetric, no BPC), a gradient
 * interpolating between its converted stops' codes, and a pattern's sRGB tile
 * converts texel by texel to 8-bit Lab codes before its pyramid (the Lab
 * Pattern Overlay's rule); every other profile reads a tile as document
 * values. */
DrPrecisionImage *dr_artwork_render(uint32_t width, uint32_t height,
    const char *description, const uint16_t *samples, size_t sample_count,
    const uint8_t *intrinsic, size_t intrinsic_count, const uint8_t *profile, size_t profile_length, uint64_t limit,
    uint64_t other_retained, const DrJobControl *control, char **err_out);
/* The 32-bit float twin: the same parameters and validation, an
 * RGBA32 float result of linear light in the ICC's primaries. A coverage
 * colour's numbers ARE linear light (Photoshop 26.10.0's 32-bit type/shape
 * colour, measured): no decode, then the sRGB->profile matrix; intrinsic RGBA8
 * pixels decode through sRGB; components mix in linear light; alpha is
 * coverage/65535. A coverage colour's RGB may be above white, 0..65504, and its
 * alpha 0..1; dr_artwork_render keeps every component 0..1. Scratch
 * charges n*16 for the output plane. */
DrPrecisionImage *dr_artwork_render_float(uint32_t width, uint32_t height,
    const char *description, const uint16_t *samples, size_t sample_count,
    const uint8_t *intrinsic, size_t intrinsic_count, const uint8_t *profile, size_t profile_length, uint64_t limit,
    uint64_t other_retained, const DrJobControl *control, char **err_out);
/* Atomically publish rendered pixels+bounded64KiB text/shape/fill description.
 * Replacement preserves layer properties and independent mask. Addition is
 * above layer in byte mode, a top root in typed mode. Byte fallback quantizes
 * genuineu16 once; descriptions preserve continuous authored values. In a
 * 32-bit float typed document the image must be the dr_artwork_render_float
 * f32 image in the document profile (a u16 image refuses). On an
 * 8-bit Lab Color document (#1 Lab phase 6a-i) the published image is put
 * on the x257 grid first (an image already on it is kept, so republishing
 * the same pixels answers no change). */
DrDocument *dr_doc_artwork(const DrDocument *document, size_t layer, bool replacing,
    const DrPrecisionImage *image, int32_t x, int32_t y, const char *name,
    const char *description, uint64_t limit, uint64_t other_retained,
    const DrJobControl *control, char **err_out);
DrPrecisionDocument *dr_precision_doc_artwork(const DrPrecisionDocument *document,
    size_t layer, bool replacing, const DrPrecisionImage *image, int32_t x, int32_t y,
    const char *name, const char *description, uint64_t limit, uint64_t other_retained,
    const DrJobControl *control, char **err_out);
/* The POSITIONAL twins: the new record lands where DrInsertPosition names
 * relative to anchor (see dr_doc_adding_layer_at, whose comment IS the slot
 * rule) instead of above layer / at the top root. Everything else — admission,
 * job, refusals, description handling — is the export above, statement for
 * statement, and these are ADDITIONS: the two exports above are unchanged and
 * keep their own placement. A new record JOINS a clip run when the next sibling
 * at its level is clipped, and insertion is lock-EXEMPT (into a locked group
 * included). There is no replacing flag: replacement is the export above. NULL
 * for an unknown position code, a refused slot or a refused publication. */
DrDocument *dr_doc_artwork_at(const DrDocument *document, size_t anchor, uint32_t position,
    const DrPrecisionImage *image, int32_t x, int32_t y, const char *name,
    const char *description, uint64_t limit, uint64_t other_retained,
    const DrJobControl *control, char **err_out);
DrPrecisionDocument *dr_precision_doc_artwork_at(const DrPrecisionDocument *document,
    size_t anchor, uint32_t position, const DrPrecisionImage *image, int32_t x, int32_t y,
    const char *name, const char *description, uint64_t limit, uint64_t other_retained,
    const DrJobControl *control, char **err_out);
/* Optional typed artwork JSON (free with dr_string_free); NULL/invalid/plain
 * returnsNULL. Layer inspection reports description_bytes before copying. */
char *dr_precision_doc_artwork_description(const DrPrecisionDocument *document, size_t layer);
/* Metadata-only patch after host rerender/geometry; NULL description clears.
 * NULL/refusal returnsNULL; unchanged also returnsNULL without an error. */
DrPrecisionDocument *dr_precision_doc_artwork_metadata(const DrPrecisionDocument *document,
    size_t layer, const char *description, uint64_t limit, uint64_t other_retained, char **err_out);

/* Native host sample intake, one original-profile raster. No codec or color
 * conversion. Layout tokens: rgba8, rgba8_premultiplied, rgb8, bgra8,
 * bgra8_opaque, rgba8_opaque, rgba16_le, rgba16_le_premultiplied, rgb16_le,
 * rgba16_le_opaque, rgb10_packed_le, argb16_be, argb16_be_premultiplied,
 * argb16_be_opaque, rgba16_le_left12, rgba16_le_opaque_left12,
 * rgba16_le_premultiplied_left12.
 * Each owner accepts only its actual sample depth. The *_opaque RGBX layouts
 * (argb16_be_opaque: XRGB) ignore the X sample. rgb10_packed_le is one little-endian u32 per pixel
 * (bits 31..30 ignored, R 29..20, G 19..10, B 9..0; minimum row width*4
 * bytes); each 10-bit code widens to (v*65535+511)/1023 with alpha 65535, so
 * its output depth is 16 and only the precision owner accepts it.
 * rgba16_le_left12 / rgba16_le_opaque_left12 are rgba16_le (straight alpha) /
 * rgba16_le_opaque whose RGB samples hold 12-bit codes left-aligned: each
 * c = v >> 4 widens to (c << 4) | (c >> 8); alpha is never expanded; precision
 * owner only. An RGB sample with v & 0xF != 0 refuses the intake ("12-bit
 * left-aligned host samples must have their low 4 bits clear").
 * rgba16_le_premultiplied_left12 is rgba16_le_left12 whose RGB is premultiplied:
 * the same low-bit check and widening come first, then the premultiplied
 * division below; alpha is untouched; precision owner only.
 * EXIF orientation 1..8 is an exact permutation, swapping extent for 5..8.
 * Row padding is skipped; length must cover all pixels and at most height rows.
 * Straight latent RGB stays exact. Premultiplied RGB divides by alpha with one
 * nearest integer rounding, clamping to range; alpha0 RGB becomes0. Opaque
 * layouts ignore any stored alpha. ICC must be exact supported matrix/TRC RGB.
 * Borrowed buffers/token stay alive for the whole call. NULL/refusal returnsNULL;
 * admission precedes output allocation; copy is cooperatively cancellable. */
DrDocument *dr_doc_from_host_pixels(const uint8_t *bytes, size_t length,
    uint32_t width, uint32_t height, size_t row_bytes, const char *layout,
    uint32_t orientation, const uint8_t *profile, size_t profile_length,
    uint64_t limit, uint64_t other_retained, const DrJobControl *control, char **err_out);
DrPrecisionDocument *dr_precision_doc_from_host_pixels(const uint8_t *bytes, size_t length,
    uint32_t width, uint32_t height, size_t row_bytes, const char *layout,
    uint32_t orientation, const uint8_t *profile, size_t profile_length,
    uint64_t limit, uint64_t other_retained, const DrJobControl *control, char **err_out);

/* A host-decoded HDR still as a 32-bit float document (v1.x): one layer
 * "Background" at the clip tone map. 1.0 is SDR reference white: the SDR
 * base's white for a gain map, 203 cd/m2 for PQ and HLG (HLG through the
 * BT.2100 OOTF at Lw 1000, gamma 1.2, Ys from the stated primaries).
 * transfer "linear" takes an rgba32f_le / rgba32f_le_premultiplied /
 * rgba32f_le_opaque layout (16 bytes a pixel, little-endian f32 linear light)
 * and the exact matrix/TRC ICC it is in, which becomes the document profile
 * (primaries NULL); "pq" / "hlg" take an integer 16-bit or rgb10_packed_le
 * layout of signal codes (E' = code / 65535 after the layout's own read) and
 * primaries "bt709" | "display_p3" | "bt2020", profile NULL / 0. bt709 opens
 * in the built-in sRGB, display_p3 in the built-in Display P3, bt2020 is
 * converted once in linear light to sRGB primaries (negatives kept; an import
 * warning names it). Orientation is EXIF 1..8. Padding, length, layout,
 * profile and admission (input + 32 B/px + bookkeeping beside other_retained,
 * under limit) are checked before any output is allocated. A non-finite f32
 * sample or one outside +/-65504 refuses by name ("HDR host pixels hold a
 * non-finite sample at (x, y)"). NULL on any refusal, with err_out set. */
DrPrecisionDocument *dr_precision_doc_from_host_hdr_pixels(const uint8_t *bytes, size_t length,
    uint32_t width, uint32_t height, size_t row_bytes, const char *layout, uint32_t orientation,
    const char *transfer, const char *primaries, const uint8_t *profile, size_t profile_length,
    uint64_t limit, uint64_t other_retained, const DrJobControl *control, char **err_out);

/* true when one image of width x height at depth (8 or 16) fits the one-image cap a document
 * keeps (the 100,000,000-pixel limit, the 8-bit decoder allocation and the 1 GiB native PNG
 * record bound; every extent within the pixel limit fits at either depth); false
 * sets err_out to the sentence (any other depth refuses as unsupported). Hosts ask it before
 * decoding a still. */
bool dr_single_image_capacity(uint32_t width, uint32_t height, uint32_t depth, char **err_out);

/* Primary-item colour facts read from an ISO-BMFF/HEIF file's own boxes, no decode.
 * true: *matrix_coefficients_out = nclx matrix_coefficients (0..65535) of the first colr/nclx
 * property ipma associates with pitm's item, or -1 if none; *premultiplied_alpha_out = an iref
 * `prem` reference has that item as its from_item_ID (pointing at the alpha
 * item). false: malformed/not ISO-BMFF, err_out set, outputs untouched.
 * NULL bytes or NULL outputs return false. Borrowed bytes live for the call. */
bool dr_heif_primary_color_facts(const uint8_t *bytes, size_t length,
    int32_t *matrix_coefficients_out, bool *premultiplied_alpha_out, char **err_out);
/* Where that `prem` reference names the primary item, no decode. true: *offset_out = the byte
 * offset in bytes of the from_item_ID field of the FIRST iref `prem` child whose from_item_ID is
 * pitm's item, *width_out = the field's width (2 for iref version 0, else 4); true with
 * *width_out 0 (and *offset_out 0) when none. false: malformed/not ISO-BMFF, err_out set,
 * outputs untouched. NULL bytes or NULL outputs return false. Borrowed bytes live for the call. */
bool dr_heif_premultiplied_reference(const uint8_t *bytes, size_t length, size_t *offset_out,
    size_t *width_out, char **err_out);

/* Retained binary resources. Envelope JSON:
 * {schema_version:1,kind:string,state:object,
 *  resources:[{id:string,media_type:string,sha256:string,byte_length:u64}]}.
 * Descriptor<=64KiB, state<=48KiB,1..16 unique resource IDs,1..256MiB/blob;
 * RAW+generic originals share a 1 GiB/doc aggregate (RAW originals alone 512MiB);
 * a .rz with an attachment over 4 resources or over 512MiB of them is DRDC 79.
 * Core preserves opaque state
 * and unknown kinds without decoding. Only dr_*_attach_raw_local_masks writes
 * one; a host_media attachment an older file carries is still read and reported.
 * All handles remain borrowed through return; ownership is never consumed.
 * NULL/refusal uses NULL/0/false and err_out; owned JSON frees with dr_string_free. */
/* Allocation-free attachment tag: 0 absent, 1 host_media, 2 raw_local_masks,
   3 any other kind. NULL/invalid layers return 0. No descriptor JSON is read. */
uint32_t dr_doc_source_attachment_kind(const DrDocument *doc, size_t layer);
uint32_t dr_precision_doc_source_attachment_kind(const DrPrecisionDocument *doc, size_t layer);

/* Native RAW local coverage. RZLM v1 is magic[4], u32LE version/width/height,
   then exact u16LE coverage in top-left source pixel-center coordinates.
   At most16 masks,64,000,000 samples per mask; generic original-byte caps apply.
   Controls use single-use jobs; all returned handles use the matching free.
   edit/develop return NULL without error for exact sample no-ops. */
typedef struct RawLocalMask DrRawLocalMask;
/* Resource diagnostics for ONE live set: include active documents, undo/redo
 * snapshots, worker inputs/results and image projections in the same call.
 * Heap buffer capacities are deduplicated by allocation identity (Arc sharing),
 * not by pixel equality. Arrays may contain duplicate/NULL handles; NULL arrays
 * require count zero. Each count is capped at 65536. All supplied handles must
 * remain live until the call returns; immutable reads and synchronized cache
 * operations may overlap across threads, but handle free must not.
 * The owned JSON (free with dr_string_free) reports unique_retained_bytes,
 * referenced_bytes, shared_bytes_saved, cache_bytes, categories, caller-supplied
 * transient_estimate_bytes, estimated_working_bytes, working_limit_bytes and
 * within_working_limit. A zero limit disables its comparison. known_unaccounted
 * explicitly distinguishes heap payload capacity from total process/platform
 * memory; scratch is an estimate, not measured allocator/RSS usage.
 * projection_scratch_estimate_bytes is the maximum full-projection scratch
 * estimate among supplied documents (zero for none); not automatically added
 * to estimated_working_bytes. A one-document call sizes that document's render.
 * NULL on invalid input/internal error, setting err_out when provided.
 * One inventory across both storage families: shared byte/typed masters count
 * once, and standalone local masks are deduplicated against their document
 * RBIN allocations. */
char *dr_mixed_resources_report_with_raw_masks(const DrDocument *const *documents, size_t document_count, const DrImage *const *images, size_t image_count, const DrPrecisionDocument *const *typed_documents, size_t typed_document_count, const DrPrecisionImage *const *typed_images, size_t typed_image_count, const DrRawLocalMask *const *masks, size_t mask_count, uint64_t transient, uint64_t limit, char **err_out);
/* Evicts disposable style/mask caches reachable from documents, and
 * process-wide LUT/content-box memos, and the process-wide contiguous layer
 * views (the tile planes stay), preserving all document pixels/state and
 * history. Same array/lifetime/error/owned-string contract as
 * dr_mixed_resources_report_with_raw_masks. Returns JSON
 * cache_bytes_before/cache_bytes_after/removed_cache_bytes over both owner
 * families. Removed planes stay alive until concurrent readers finish;
 * renderers may refill the caches. This does not promise an immediate
 * decrease in process RSS. */
char *dr_mixed_resources_evict_caches(const DrDocument *const *documents, size_t document_count,
    const DrPrecisionDocument *const *precision_documents, size_t precision_document_count, char **err_out);
/* Sets the process-wide working limit used where no caller limit is passed
 * (internal constructors, *_plan() defaults, the native reopen check, the PSD
 * layered-import envelope). The default is 2 GiB. Returns false for 0 and
 * leaves the value unchanged. Call once at launch. Thread-safe. An admission
 * with an explicit caller limit never consults the process limit. */
bool dr_set_process_working_limit(uint64_t bytes);
/* The host's post-projection trim threshold for non-view cache bytes (the
 * disposable caches dr_mixed_resources_evict_caches drops): the working
 * limit / 8, so it follows dr_set_process_working_limit. 0 on an internal
 * error. */
uint64_t dr_process_cache_trim_limit(void);
DrRawLocalMask *dr_raw_local_mask_edit(const DrRawLocalMask *prior, uint32_t width, uint32_t height, const char *command_json, uint64_t limit, uint64_t other, const DrJobControl *control, char **err_out);
/* Strict version1 inverse source-coordinate map, at most4096 UTF8 bytes.
   channel0/1/2 selects red/green/blue lens sampling; EXIF/affine require1.
   Plans report width,height,scratch_bytes,work_units,exact,unchanged.
   Identity edits return NULL without error and retain the input owner.
   dr_raw_source_geometry[_plan] take a u16 source or an f32 one (a 32-bit float
   development) and answer in the source's storage. The f32 arm runs the same
   maps and kernel with premultiplied binary64 accumulation and no quantization:
   an exact map copies samples bit for bit; a resample clamps each RGB output to
   the source plane's visible range, clamps alpha to0…1 and drops an alpha below
   2^-24 to transparent. Its plan charges 16 bytes a source pixel plus 32 (exact)
   or 80 (resampled) a destination pixel. */
char *dr_raw_local_mask_remap_plan(const DrRawLocalMask *mask, const char *request, uint32_t channel, uint64_t limit, uint64_t other, char **err_out);
DrRawLocalMask *dr_raw_local_mask_remap(const DrRawLocalMask *mask, const char *request, uint32_t channel, uint64_t limit, uint64_t other, const DrJobControl *control, char **err_out);
/* A new width x height mask holding mask's coverage in normalised coordinates
   (each axis scaled on its own; edge-normalised bilinear resize, so full coverage
   stays 65535 and a reduction is anti-aliased). Returns NULL without error when
   the size is unchanged. Error for a NULL mask, a 0 side, more than 64 MP, a
   refused admission (output, a 4*source_width*height f32 intermediate and
   bookkeeping) or a cancelled control. Free with dr_raw_local_mask_free. */
DrRawLocalMask *dr_raw_local_mask_resize(const DrRawLocalMask *mask, uint32_t width, uint32_t height, uint64_t limit, uint64_t other, const DrJobControl *control, char **err_out);
char *dr_raw_source_geometry_plan(const DrPrecisionDocument *source, const char *request, uint64_t limit, uint64_t other, char **err_out);
DrPrecisionDocument *dr_raw_source_geometry(const DrPrecisionDocument *source, const char *request, uint64_t limit, uint64_t other, const DrJobControl *control, char **err_out);
void dr_raw_local_mask_free(DrRawLocalMask *mask);
char *dr_raw_local_mask_info(const DrRawLocalMask *mask, char **err_out);
/* Borrowed bytes remain live until mask is freed; never free the result. */
const uint8_t *dr_raw_local_mask_bytes(const DrRawLocalMask *mask, size_t *length, char **err_out);
DrRawLocalMask *dr_raw_local_mask_from_bytes(const uint8_t *input, size_t length, uint64_t limit, uint64_t other, const DrJobControl *control, char **err_out);
/* A new mask from a host-generated byte coverage plane (the Vision subject or people mask):
   width*height samples, row 0 top, 0 outside…255 inside; sample v is stored as u16 v*257.
   Returns NULL without error when every sample is 0. NULL with err_out for NULL samples,
   length != width*height, a zero or over-64-megapixel size, a refused admission or a cancelled control. */
DrRawLocalMask *dr_raw_local_mask_from_coverage(const uint8_t *samples, size_t length, uint32_t width, uint32_t height, uint64_t limit, uint64_t other, const DrJobControl *control, char **err_out);
/* Complete staged copy; caller buffer unchanged on failure. */
bool dr_raw_local_mask_copy(const DrRawLocalMask *mask, uint8_t *output, size_t length, uint64_t limit, uint64_t other, const DrJobControl *control, char **err_out);
/* RGBA8 red display overlay only, bounded1600 per output side.
   base/range_json: the selected local's range (the settings JSON's "range"
   object, at most1024 UTF8 bytes) and the base development it reads; both NULL
   (no range) or both set, one alone is an error. With them the alpha is shape
   coverage times the range's coverage of the base sample under each display
   pixel, whatever `effective` says. The base is read in place; an f32 base (a
   32-bit float development) is read as linear light in its profile's primaries. */
DrImage *dr_raw_local_mask_preview(const DrRawLocalMask *mask, const DrPrecisionDocument *base, const char *range_json, uint32_t x, uint32_t y, uint32_t width, uint32_t height, uint32_t divisor, double density, double feather, bool effective, uint64_t limit, uint64_t other, const DrJobControl *control, char **err_out);
/* The colour a range's Sample Colour click stores: the mean CIE L*a*b* (D50,
   through the base's own profile, each pixel's L* clamped to0…100) of the 5x5
   square of base-development pixels about source point (x, y), clipped to the
   frame. x in0…width, y in0…height; an edge point reads the last column/row.
   Writes 3 doubles and returns true; false with err_out and lab_out untouched
   for a point off the frame, a base that is not one plain RGBA16 or RGBA32
   float raster, a profile with no matrix/TRC model, or NULL base/lab_out. An
   f32 base is read as linear light (matrix, no tone curve), so a sample above
   white reads L* 100 and one below zero L* 0. */
bool dr_precision_doc_raw_range_sample(const DrPrecisionDocument *base, double x, double y, double *lab_out /* 3 doubles */, char **err_out);
/* Validates the same preview request and reports its conservative scratch;
   no coverage/display pixels are allocated. Returns0 with err_out on refusal. */
uint64_t dr_raw_local_mask_preview_scratch(const DrRawLocalMask *mask, uint32_t x, uint32_t y, uint32_t width, uint32_t height, uint32_t divisor, double density, double feather, bool effective, char **err_out);
/* Mixes up to16 locals into the base through their masks, then the optional
   calibrated manual vignette. count is 0…16; variants is non-NULL when count > 0
   and each entry is a full-source camera variant or NULL for a local with no
   camera change, at most 4 non-NULL; every masks entry is non-NULL. A NULL-variant
   local reads the development as the earlier locals left it (overlapping tone
   locals compound); with no tone it changes nothing. settings_json (required, at
   most32768 UTF8 bytes) is an array of `count` objects, one per local in order;
   an unknown member is refused. Members: "enabled" (bool), "density" (0…1), "feather"
   (pixels,0…256) and optional "range" {"luminance": {"low","high","low_falloff",
   "high_falloff","invert"}, "color": {"lab":[L,a,b],"amount","falloff","invert"}}
   holding at least one of the two (low ≤ high in0…1, luminance falloffs0…1,
   L in0…100, a and b −256…256, amount and colour falloff0…200). A range multiplies
   the local's shape coverage by its coverage of the BASE sample, read as CIE
   L*a*b* (D50) through the base's own profile; feather and density act on the
   shape alone. A range on an enabled local refuses a base profile with no
   matrix/TRC model. Optional "tone" {"exposure" −4…4 stops (a linear-light gain
   through the base profile), "contrast", "whites", "blacks", "saturation",
   "clarity", "texture", "sharpness" −100…100, "highlights", "shadows",
   "noise_reduction" 0…100; absent keys 0, unknown refused} adjusts the local's
   source sample (its variant's, or the accumulated development's) through the
   existing adjustment, Presence, Reduce Noise, Smart Sharpen and Gaussian Blur
   kernels before the blend; any non-zero tone on a 32-bit float base refuses
   "RAW local tone controls are not available for a 32-bit float development."
   Returns NULL without error when the result equals the base exactly. An f32
   base (a 32-bit float development) needs f32 variants (a u16 variant beside it
   is refused) and answers an f32 float document: the blend and the vignette (a
   linear gain, no tone curve) run on the samples with no clamp and no
   quantization, so values above 1 and below 0 are kept and a pixel no local
   weighs keeps its base sample bit for bit. Its plan is the u16 one with 32
   bytes a pixel in place of 24.
   dr_precision_doc_from_image shares a u16 image in a 16-bit document, or an
   f32 image in a 32-bit float document under the clip tone map. */
DrPrecisionDocument *dr_precision_doc_develop_locals(const DrPrecisionDocument *base, const DrPrecisionDocument *const *variants, const DrRawLocalMask *const *masks, size_t count, const char *settings_json, const char *vignette_json, uint64_t limit, uint64_t other, const DrJobControl *control, char **err_out);
DrPrecisionDocument *dr_precision_doc_from_image(const DrPrecisionImage *image, uint64_t limit, uint64_t other, const DrJobControl *control, char **err_out);
/* Shares a validated mask's native binary resource allocation; no pixel copy. */
DrRawLocalMask *dr_doc_raw_local_mask(const DrDocument *document, size_t layer, size_t resource, char **err_out);
DrRawLocalMask *dr_precision_doc_raw_local_mask(const DrPrecisionDocument *document, size_t layer, size_t resource, char **err_out);
/* Fresh RAW revisions only, count0…16. NULL descriptor/count0 is an already-empty
   no-op; these calls never discard a host/unknown/previous revision attachment. */
DrDocument *dr_doc_attach_raw_local_masks(const DrDocument *document, size_t layer, const char *descriptor, const DrRawLocalMask *const *masks, size_t count, uint64_t limit, uint64_t other, const DrJobControl *control, char **err_out);
DrPrecisionDocument *dr_precision_doc_attach_raw_local_masks(const DrPrecisionDocument *document, size_t layer, const char *descriptor, const DrRawLocalMask *const *masks, size_t count, uint64_t limit, uint64_t other, const DrJobControl *control, char **err_out);
/* Presence/sizing do not parse JSON or allocate. NULL/invalid/absent: false/64KiB.
 * Admit scratch before full info; absent info isNULL without error. */
bool dr_doc_has_source_attachment(const DrDocument *document, size_t layer);
bool dr_precision_doc_has_source_attachment(const DrPrecisionDocument *document, size_t layer);
uint64_t dr_doc_source_attachment_scratch_bytes(const DrDocument *document, size_t layer);
uint64_t dr_precision_doc_source_attachment_scratch_bytes(const DrPrecisionDocument *document, size_t layer);
char *dr_doc_source_attachment_info(const DrDocument *document, size_t layer, char **err_out);
char *dr_precision_doc_source_attachment_info(const DrPrecisionDocument *document, size_t layer, char **err_out);

/* Additive weighted byte Auto: empty coverage preserves the legacy analysis. */
char *dr_image_selected_auto_levels(const DrImage *image,
    const uint8_t *coverage, size_t length, uint32_t mode, double clip, char **err_out);
/* Scoped statistics schema2: named composite/raw layer/backdrop (below index),
 * canvas coordinates, native u16 selection coverage (NULL/0 = whole source).
 * Plot stride never changes complete endpoint/sum counts. Strings use dr_string_free.
 * A float document answers the float schema (storage "rgba32_float"):
 * the histogram is STOP-BINNED over -12..+6 EV in 256 bins with weighted
 * exact_zero/below_zero/above_white counts, linear sums and min/max; the
 * sample carries center_rgba32, mean_rgba32, weighted_sum_rgba32, stops and
 * above_white/below_zero flags. The render charge is 16 bytes per pixel.
 * A backdrop histogram at stride 1 may carry "auto": {"mode":"tone"|"color",
 * "clip"?: 0...0.1 (default 0.001), "insert"?: {"anchor","position"}}
 * (core/src/scoped_auto.rs): Levels Auto over the WHOLE backdrop (clipped or
 * grouped alike), each pixel weighted by the coverage x the
 * edited adjustment's own mask (no insert), answered as
 * "auto": {mode, clip, empty, black[3], white[3], gamma[3], identity} beside
 * the unchanged histogram; a float document refuses it. A histogram may carry
 * "float_bins": "stops" (the default) or "unit": a float source then bins RGB
 * and luma as round(clamp(v, 0, 1) * 255), the axis of a float Levels/Curves
 * sheet, and adds "float_bins":"unit" with "range":{"min":0,"max":1}; every
 * count, sum and min/max is unchanged. 8/16-bit sources ignore it; a sample
 * refuses it ("float_bins requires a histogram"). */
char *dr_precision_doc_scoped_analysis_plan(const DrPrecisionDocument *document,
    const char *request, char **err_out);
char *dr_precision_doc_scoped_analysis(const DrPrecisionDocument *document,
    const char *request, const uint16_t *coverage, size_t length,
    uint64_t limit, uint64_t other_retained, const DrJobControl *control, char **err_out);
char *dr_doc_scoped_analysis_plan(const DrDocument *document,
    const char *request, char **err_out);
char *dr_doc_scoped_analysis(const DrDocument *document,
    const char *request, const uint16_t *coverage, size_t length,
    uint64_t limit, uint64_t other_retained, const DrJobControl *control, char **err_out);
/* The same schema over a supplied (retained) u16 or float projection,
 * composite source only: layer/backdrop refuse "projection analysis takes the
 * composite source only" before admission; float storage answers the float
 * schema. Coverage is image-sized. No render: scratch 2 MiB + 1 MiB. */
char *dr_precision_image_scoped_analysis_plan(const DrPrecisionImage *image,
    const char *request, char **err_out);
char *dr_precision_image_scoped_analysis(const DrPrecisionImage *image,
    const char *request, const uint16_t *coverage, size_t length,
    uint64_t limit, uint64_t other_retained, const DrJobControl *control, char **err_out);
/* The byte twin: the same schema over the document's supplied
 * (retained) canvas-sized 8-bit projection instead of a render, composite
 * source only (layer/backdrop refuse "projection analysis takes the
 * composite source only" before admission). The answer is
 * dr_doc_scoped_analysis's for that projection; the profile fields are the
 * document's. A projection that is not canvas-sized refuses "projection is
 * not the canvas size". Coverage is canvas-sized. No render: scratch 2 MiB
 * + 1 MiB, admitted beside the projection's pixels. */
char *dr_doc_projection_scoped_analysis_plan(const DrDocument *document,
    const char *request, char **err_out);
char *dr_doc_projection_scoped_analysis(const DrDocument *document, const DrImage *projection,
    const char *request, const uint16_t *coverage, size_t length,
    uint64_t limit, uint64_t other_retained, const DrJobControl *control, char **err_out);

/* ---- Export workspace (Phase 11A) ---------------------------------------
 *
 * One pure render -> resample -> sharpen -> convert -> reduce -> encode chain
 * that produces the ENCODED BYTES of a deliverable without touching the
 * editable master: no undo step, no dirty state, no document mutation.
 *
 * Every call takes the SAME request object, which is also what a preset
 * stores and what the MCP tools accept:
 *
 *   {"format":"png|jpeg|tiff|bmp|gif|webp",   (required)
 *    "jpeg_quality":1..100,                   (default 90)
 *    "bit_depth":8|16,                        (default 8; 16 is png/tiff only)
 *    "float_samples":"f32|f16",               (default f32; see "Float documents")
 *    "dither":"none|ordered",                 (default none; 8-bit only)
 *    "size":{"mode":"none|width|height|long_edge|scale","value":number},
 *    "filter":"nearest|bilinear|catmull-rom|lanczos3",   (default lanczos3)
 *    "profile":"document|srgb|display-p3|file",  (default document = no-op)
 *    "profile_path":"<absolute path>",        (required by profile "file")
 *    "intent":"perceptual|relative|saturation|absolute",   (default relative)
 *    "black_point_compensation":bool,         (default false)
 *    "sharpen":{"radius":0..64,"amount":0..5,"threshold":0..1}|null,
 *    "embed_profile":bool,                    (default true)
 *    "strip_metadata":bool,                   (default false)
 *    "metadata":{...},                        (default absent; Phase 11B)
 *    "spot_channels":"overprint|keep"}        (default overprint; #1 1c-2a)
 *
 * "spot_channels":"keep" (#1 1c-2a) makes a typed TIFF export keep the
 * document's spot channels as trailing extra samples, the way a TIFF save
 * does, instead of compositing their overprint. It requires format tiff
 * and size none ("export request spot_channels \"keep\" requires format
 * tiff and size none"); a CMYK document refuses it ("export request
 * spot_channels \"keep\" is not available for a CMYK document; its TIFF save
 * keeps the spot channels"), and a Grayscale, Duotone, Lab or 32-bit file
 * holding spot channels refuses with "Spot channels are saved to a TIFF
 * only from an RGB or CMYK document at 8 or 16 bits; save as .doctored, or
 * delete the spot channels first.". The plan then charges the kept planes
 * rather than the overprint. The byte export ignores it.
 *
 * "sharpen" may also carry "method": "unsharp_mask" (the keys above, the
 * same bytes as without it) or "smart_sharpen" with any of
 * dr_image_filter_op's smart_sharpen keys, ranges and defaults: the filter's
 * plane and combine on the 8-bit deliverable (dr_image_filter_op's bytes) or
 * on the u16 codes at 16 bits. Any other method errors "export sharpen
 * method must be unsharp_mask or smart_sharpen"; a smart_sharpen key refusal
 * is "export sharpen: " and the filter's sentence. Smart Sharpen is never a
 * bypass, so a 32-bit float export with it refuses as with an active Unsharp
 * Mask.
 *
 * The "metadata" key is the per-packet policy documented in the Phase 11B
 * block below; omitting it means every packet defers to strip_metadata, which
 * is 11A's behaviour exactly.
 *
 * "profile":"file" (Phase 11C) reads profile_path HERE, at parse time, so an
 * unreadable, non-RGB, unencodable or oversized (>16 MiB) profile refuses the
 * REQUEST rather than half of an export. The intent and BPC keys drive the
 * convert stage; at their defaults that stage is 11A's, byte for byte. A host
 * that offers "installed" profile names resolves them to a path before the
 * request reaches this library. An export whose convert stage would encode
 * INTO a matrix/TRC profile whose tone curves fail the precision check
 * refuses before any work with dr_icc_describe's "export_refusal" sentence
 * — except the document's own profile bytes under a non-absolute
 * intent with BPC off, which never reach the encoder. "profile":"document"
 * never converts, so it never refuses this way.
 *
 * An unknown key refuses. With every optional field omitted the bytes are
 * exactly the bytes dr_doc_save_image (8-bit) and
 * dr_precision_doc_save_projection16 (16-bit PNG/TIFF) already wrote.
 *
 * dr_export_result_info additionally reports "profile_name" (the destination
 * profile's display name), "intent" and "black_point_compensation".
 *
 * The preview contract: dr_export_result_preview decodes the very bytes
 * dr_export_result_write writes. There is no second encode. */
typedef struct DrExportResult DrExportResult;

/* Owned JSON {"width":..,"height":..} for the output dimensions `request_json`
 * derives from a w x h source. Aspect ratio is kept, the non-requested side is
 * round() then max(1). Refuses a value of 0, a side above the request
 * format's own cap (65535 for jpeg and gif, 16384 for webp, 130048 for heic;
 * png, tiff and bmp have none), a scale outside (0,16] and a pixel count past
 * the document ceiling. Free with dr_string_free; NULL plus err_out on
 * refusal. */
char *dr_export_output_size(uint32_t w, uint32_t h, const char *request_json,
                            char **err_out);

/* Owned expanded BASE file name (no extension, no directory) for `pattern`.
 * tokens_json is {"name","width","height","format","date","preset"} plus the
 * OPTIONAL Phase 11B {"layer","index","count"} and the OPTIONAL "layer_export"
 * bool; the HOST supplies `date` as a local YYYY-MM-DD string, because
 * a pure core has no calendar. {name} is MANDATORY unless "layer_export" is
 * true, which makes {layer} or {index} MANDATORY instead ("layer_export" that
 * is not a bool refuses). Refuses an unknown token, an unterminated "{", an empty result,
 * more than 200 characters, a '/', '\\', ':' or control character, and a
 * leading dot — and refuses {layer} or {index} when that token was not
 * supplied, rather than expanding it to nothing. {layer} is sanitized ('/',
 * '\\', ':' and control characters become '-', trimmed, empty becomes
 * "layer"); {index} is zero-padded to `count`'s digit width (3 of 12 -> "03").
 * Free with dr_string_free. */
char *dr_export_expand_name(const char *pattern, const char *tokens_json,
                            char **err_out);

/* Encode an 8-bit document as a deliverable. `flat` MAY BE NULL with exactly
 * dr_doc_save_image's semantics: a caller-supplied composite of THIS document,
 * or NULL to flatten here. bit_depth must be 8. NO FILE IS WRITTEN. `control`
 * is optional and single-use; a cancelled token returns NULL with "operation
 * cancelled". Free the result with dr_export_result_free. */
DrExportResult *dr_doc_export_encode(const DrDocument *doc, const DrImage *flat,
                                     const char *request_json,
                                     const DrJobControl *control, char **err_out);

/* Owned plan JSON (dr_string_free) for one typed export: retained_bytes is the
 * unique union of the document and, when supplied, its projection, so
 * other_retained_bytes must exclude both; scratch_bytes covers the source,
 * resampled, sharpened and converted planes, any byte reduction and the codec.
 * Also width and height. No pixel is allocated or rendered. */
char *dr_precision_doc_export_plan(const DrPrecisionDocument *document,
                                   const DrPrecisionImage *projection,
                                   const char *request_json, char **err_out);

/* Encode a typed document as a deliverable at 16 or 8 bits. `projection` MAY
 * BE NULL, in which case the document is rendered here; when non-NULL its
 * storage must be RGBA16 UNORM, its dimensions the canvas and its ICC bytes
 * exact. NO FILE IS WRITTEN. Free the result with dr_export_result_free.
 * A CMYK document (#1 phase 5) ignores `projection` in both calls: profile
 * "document" with jpeg or tiff KEEPS CMYK (the CMYK save's bytes, info
 * "cmyk":true; a jpeg is a pending 8-bit CMYK TIFF the host transcodes) and
 * refuses a size, a sharpen, a tiff bit_depth other than the document's and a
 * metadata policy by name; every other request flattens on paper and converts
 * (its intent and BPC) to sRGB ("document"), Display P3 or the file profile,
 * then runs the ordinary stages. A Grayscale document (#1 phase 1b) with
 * profile "document" KEEPS GRAY: png, tiff or jpeg at 8 or 16 bits (jpeg at
 * 8) written as one channel (two with alpha) with the Gray ICC embedded, and
 * every other kept format or bit_depth 32 refused naming phase 3.
 * An Indexed Color document (#1 Indexed phase 1b) ignores `projection` when
 * profile "document" with png, gif or tiff KEEPS INDEXED: the palette, the
 * transparent index and every index written as they are (png colour type 3
 * at the least depth, PLTE the palette, tRNS clear at the transparent index;
 * gif89a with the table padded black to a power of two and no profile or
 * metadata; tiff 8-bit Photometric 3, ColorMap padded black to 256, a
 * straight-alpha sample when a transparent index is set), at 8 bits;
 * bit_depth other than 8 ("...keeps the palette at 8 bits; ...") and a size
 * or an active sharpen ("...keeps every index, so it takes no size or
 * sharpen; ...") refuse. Every other request runs the RGB stages over the
 * palette's render. The preview of a kept tiff decodes its palette. */
DrExportResult *dr_precision_doc_export_encode(const DrPrecisionDocument *document,
                                               const DrPrecisionImage *projection,
                                               const char *request_json, uint64_t limit,
                                               uint64_t other_retained_bytes,
                                               const DrJobControl *control, char **err_out);

/* Borrowed encoded bytes, valid only while the result handle is alive;
 * length_out (may be NULL) receives the length. NULL result yields NULL/0. */
const uint8_t *dr_export_result_bytes(const DrExportResult *result, size_t *length_out);

/* Owned JSON: bytes, width, height, carried (the DR_CARRIES_* bits actually
 * written), bit_depth, format, profile, and the Phase 11B "metadata" object
 * {"profile","exif","xmp","iptc","density","stamped"} — always present. A
 * LAYER result additionally carries
 * "layer":{"index","name","origin":[x,y],"size":[w,h]}.
 * Free with dr_string_free. */
char *dr_export_result_info(const DrExportResult *result, char **err_out);

/* Decode the encoded bytes into a new image (free with dr_image_free) — the
 * preview contract: these are the bytes dr_export_result_write will write. */
DrImage *dr_export_result_preview(const DrExportResult *result, char **err_out);

/* Write the encoded bytes to `path`, atomically. An EXISTING path is REFUSED
 * unless `overwrite`, leaving it byte-identical. path is UTF-8 <= 64 KiB. */
bool dr_export_result_write(const DrExportResult *result, const char *path,
                            bool overwrite, char **err_out);

void dr_export_result_free(DrExportResult *result);

/* ---- Production outputs (Phase 11B) -------------------------------------
 *
 * Three additions on top of the 11A chain, all of them going through the SAME
 * request object, the SAME DrExportResult handle and the SAME naming tokens:
 * per-item layer/group exports, contact sheets, and a per-packet metadata
 * policy. The master never changes; with the new options left out the bytes
 * are 11A's bytes.
 *
 * THE METADATA POLICY is the request's "metadata" object:
 *
 *   {"exif":"default|keep|drop",     (default "default")
 *    "xmp":"default|keep|drop",
 *    "iptc":"default|keep|drop",
 *    "density":"default|keep|drop",  ("keep" is identical to "default")
 *    "stamp":{"author":string|null,"copyright":string|null}|null}
 *
 * "default" defers to strip_metadata; "keep" writes the packet even when
 * strip_metadata is set; "drop" never writes it. "density" governs the
 * container's own slot (the JFIF APP0, the PNG pHYs) and ignores
 * strip_metadata, exactly as 11A does. The stamp is written into the XMP
 * packet as dc:creator / dc:rights; a stamp string is at most 512 characters,
 * carries no control character, and at least one of the two must be present.
 *
 * There is NO orientation key: dr_image_open bakes the camera rotation into
 * the pixels, so every EXIF and XMP packet this writes states Orientation 1.
 *
 * Two cross-field rules refuse when the request is PARSED, because both would
 * otherwise report something the bytes contradict:
 *   - a stamp when the XMP packet will not be written (dropped, or a format
 *     that cannot carry one);
 *   - "density":"drop" while the EXIF or XMP packet still resolves to written,
 *     since each restates the ppi. */

/* Encode ONE top-level layer or group of an 8-bit document. `idx` is a stack
 * index at DEPTH 0; the item is rendered in isolation (only its own subtree,
 * root forced visible, composited against transparency) and then run through
 * the ordinary 11A chain. `trimmed` delivers the alpha bounding box and
 * REFUSES a fully transparent item; `trimmed == false` delivers the canvas,
 * and a fully transparent item is then a fully transparent frame. bit_depth
 * must be 8. NO FILE IS WRITTEN. Free with dr_export_result_free. */
DrExportResult *dr_doc_export_layer_encode(const DrDocument *doc, size_t idx,
                                           bool trimmed, const char *request_json,
                                           const DrJobControl *control, char **err_out);

/* Owned plan JSON (dr_string_free) for one typed LAYER export: the isolated
 * document's own export plan, widened by the scratch its one-root render and
 * any trim copy spend. No pixel is allocated or rendered. */
char *dr_precision_doc_export_layer_plan(const DrPrecisionDocument *document, size_t idx,
                                         bool trimmed, const char *request_json,
                                         char **err_out);

/* The typed twin of dr_doc_export_layer_encode, at 16 or 8 bits. `idx` is a
 * POSTORDER record index at depth 0. other_retained_bytes excludes the
 * document itself. NO FILE IS WRITTEN. */
DrExportResult *dr_precision_doc_export_layer_encode(const DrPrecisionDocument *document,
                                                     size_t idx, bool trimmed,
                                                     const char *request_json, uint64_t limit,
                                                     uint64_t other_retained_bytes,
                                                     const DrJobControl *control,
                                                     char **err_out);

/* The contact-sheet layout request, shared by both calls below:
 *
 *   {"columns":1..32,                 (default 4)
 *    "cell_width":16..2048,           (default 256)
 *    "cell_height":16..2048,          (default 256)
 *    "gutter":0..256,                 (default 16)
 *    "caption_height":0..128,         (default 0)
 *    "background":"#rrggbb"|"#rrggbbaa",   (default "#ffffff")
 *    "filter":"nearest|bilinear|catmull-rom|lanczos3"}  (default lanczos3)
 *
 * The gutter is drawn outside the grid as well as between cells:
 *   width  = gutter + columns * (cell_width + gutter)
 *   height = gutter + rows    * (cell_height + caption_height + gutter)
 * with rows = ceil(count / columns). */

/* Owned JSON {"width","height","rows"} for a sheet of `count` cells. Refuses
 * 0 cells, more than 256, a side above 30000 and a pixel count past the
 * document ceiling. Free with dr_string_free. */
char *dr_contact_sheet_size(const char *request_json, size_t count, char **err_out);

/* Compose a contact sheet into a NEW image (free with dr_image_free). Cells
 * are 8-bit sRGB, scaled to fit WITHOUT upscaling and centred; a transparent
 * cell therefore shows the background. `sources` is `count` live documents.
 * `captions` is POSITIONAL with sources: it MAY BE NULL, and any entry may be
 * NULL. The core places caption BITMAPS only — it rasterizes no text. */
DrImage *dr_contact_sheet_compose(const char *request_json,
                                  const DrDocument *const *sources,
                                  const DrImage *const *captions, size_t count,
                                  const DrJobControl *control, char **err_out);

/* Owned canonical JSON echo of a metadata policy — every rule spelled out and
 * the stamp as an object or null. Refuses an unknown key, an unknown rule, a
 * stamp string past 512 characters or holding a control character, and a stamp
 * with neither field. This validates the policy ALONE; the stamp and density
 * cross-field rules depend on the format and are checked when an export
 * request is parsed. Free with dr_string_free. */
char *dr_metadata_policy_validate(const char *policy_json, char **err_out);


/* ---- Output color control (Phase 11C) -----------------------------------
 *
 * Rendering intents, black-point compensation, LUT (A2B/B2A) profiles, soft
 * proofing, the gamut warning, and the explicit adjustment/Assign policies a
 * profile conversion runs under. Everything below is additive: with the
 * options left at their defaults, dr_doc_convert_to_profile and every 11A/11B
 * export write exactly the bytes they wrote before this section existed.
 *
 * RENDERING INTENTS. DR_INTENT_* below. A LUT profile's table is chosen per
 * intent with a documented fallback: perceptual takes A2B0/B2A0, relative and
 * absolute take A2B1/B2A1 else A2B0/B2A0, saturation takes A2B2/B2A2 else
 * A2B0/B2A0. A matrix/TRC profile has no tables: every intent is the matrix
 * path, and ABSOLUTE additionally scales XYZ by MW_src / MW_dst
 * componentwise. Black-point compensation is ISO 18619's affine map in XYZ,
 * between black points detected as Little CMS's cmsDetectBlackPoint does:
 * matrix/Gray device zero; v4 perceptual/saturation the fixed perceptual
 * black; an output-class CMYK profile at relative its perceptual black
 * (B2A0 then A2B1 of Lab 0), made neutral with L* at most 50; otherwise the
 * darkest colorant through the intent's A2B table, made neutral with L* at
 * most 50 but zero when its L* is above 95 or below 0; and zero when the
 * table is missing.
 *
 * WHAT THE LUT PARSER MODELS. v2 lut8Type (mft1) and lut16Type (mft2), v4
 * lutAtoBType (mAB ) and lutBtoAType (mBA ), for RGB, Gray (kTRC only) and
 * CMYK device spaces with a Lab or XYZ PCS. REFUSED, each naming what the
 * profile is in the `refusal` field of dr_icc_describe, checked class first,
 * then data space, then PCS, then tables: a named-colour profile (nmcl, a
 * list of colours), a device link (link) or abstract (abst) profile (each a
 * transform), any other non-pixel class; an nCLR separation space (named by
 * its colourant count, 2CLR-4CLR included) or any other space but RGB, Gray
 * and CMYK; an mpet (multiProcessElements) A2Bx/B2Ax; a profile whose only
 * tables are the floating-point D2B0..3/B2D0..3 (the first found is named);
 * and a colour table past 64 MiB decoded. A profile that parses but holds
 * only one direction cannot be PROOFED: dr_icc_describe's `proof_refusal`
 * names the missing A2B (device-to-PCS) or B2A (PCS-to-device) table and,
 * when the profile carries one, its floating-point D2Bx/B2Dx twin.
 *
 * CONVERSION OPTIONS JSON (every key optional, defaults = today's behaviour):
 *   {"intent": "perceptual"|"relative"|"saturation"|"absolute",   // relative
 *    "black_point_compensation": bool,                            // false
 *    "adjustments": "keep"|"preserve_appearance",                 // keep
 *    "rasterize_sources": bool}                                   // false
 * A layer bound to a retained source (a RAW layer) re-renders relative
 * colorimetric with BPC off, so black-point compensation, the absolute intent
 * and, beside a LUT-only profile, any intent but relative REFUSE to keep it
 * bound. "rasterize_sources": true is the disclosed way through: when
 * the settings would refuse, every bound layer is rasterized first and the
 * result converted, as ONE document the host commits as ONE undo step;
 * settings the sources can follow keep them bound whatever it says.
 * "keep" is the shipped policy: an adjustment's numbers are PARAMETERS, left
 * exactly as authored. "preserve_appearance" KEEPS exposure and
 * white_balance at evaluation "document_v2" (light-linear operations through
 * the document's own profile) and BAKES every other adjustment into one
 * raster layer "<name> (baked)" in its place, leaving every other layer where
 * it is. It REFUSES, naming the layer, when a to-be-baked adjustment is
 * inside a group, is clipped, or sits over transparent pixels.
 * Into a destination with NO matrix/TRC model (a LUT-only RGB profile) a
 * document_v2 exposure/white_balance cannot be evaluated: "keep" REFUSES,
 * naming the layer (offering preserve_appearance when its bake would
 * succeed), "preserve_appearance" BAKES a visible one under the same rules
 * and refuses a hidden one, and a linear-light document refuses the
 * destination outright. Matrix/TRC destinations are unchanged.
 *
 * PROOF SETUP JSON (every key optional):
 *   {"intent": ...,                            // relative
 *    "black_point_compensation": bool,         // true
 *    "simulate_paper_white": bool,             // false
 *    "simulate_ink_black": bool,               // false
 *    "gamut_warning": bool,                    // false
 *    "gamut_color": "#RRGGBB",                 // "#808080"
 *    "preserve_numbers": bool,                 // false; a CMYK document only
 *    "plate": "cyan"|"magenta"|"yellow"|"black"} // null
 * "preserve_numbers" on an RGB document refuses ("Preserve Numbers needs a
 * CMYK document"); "plate" shows that proof-device ink as grey, ink dark, and
 * refuses with a proof profile that is not CMYK. The info "space" is
 * "document", "srgb" or (a CMYK document's proof only) "display-p3".
 * A pixel is OUT OF GAMUT when the CIE76 dE*ab between its own Lab and the
 * Lab of its proof round trip (the proof's B2A1 then A2B1 — RELATIVE
 * COLORIMETRIC ALWAYS, no BPC and no white scaling, independent of the
 * setup's intent) exceeds 5.0.
 *
 * VIEW STATE. Nothing in this section writes a document: a proof render is a
 * new image handle, the setup lives in the host, and no undo step, dirty flag
 * or Action step comes out of it. */

/* Rendering intents; the values are ABI. */
#define DR_INTENT_PERCEPTUAL 0
#define DR_INTENT_RELATIVE   1
#define DR_INTENT_SATURATION 2
#define DR_INTENT_ABSOLUTE   3

/* Everything this build can say about profile `bytes`, as owned JSON (free
 * with dr_string_free):
 *   {"name","version","class","space":"rgb|gray|cmyk|lab|other","pcs",
 *    "model":"matrix|lut|none","can_decode","can_encode",
 *    "intents":{"perceptual","relative","saturation"},
 *    "media_white":[x,y,z],"proofable","refusal":null|string,
 *    "proof_refusal":null|string,"high_depth_destination":bool,
 *    "export_refusal":null|string,"convert_refusal":null|string}
 * "proof_refusal" is why the profile cannot be proofed through — null exactly
 * when "proofable"; the parser's sentence, or the missing direction ("it has
 * no A2B (device-to-PCS) table" / "it has no B2A (PCS-to-device) table")
 * followed by "; its <TAG> is floating-point, which this build does not read"
 * when a D2Bx/B2Dx twin exists. "high_depth_destination" is whether a
 * 16/32-bit Convert accepts the profile as a destination (a matrix/TRC model
 * that passes the precision checks, the answer of
 * dr_precision_doc_profile_is_convertible). "export_refusal" and
 * "convert_refusal" are the sentences the core's export and convert
 * preflights refuse with when an export or a Convert would encode INTO the
 * profile — null unless "model" is "matrix" and "high_depth_destination" is
 * false; a document already tagged with the profile still
 * converts OUT of it. Unlike dr_icc_inspect this reads
 * Gray and CMYK profiles too, because a proof setup may name one. NULL with
 * err_out set on a NULL buffer. */
char *dr_icc_describe(const uint8_t *bytes, size_t len, char **err_out);

/* Converts one AUTHORED straight RGB triple between sRGB and the RGB LUT profile `profile` (`len` bytes)
 * through the core's relative-colorimetric, BPC-off pipeline, the one layer styles, artwork and high-depth
 * sRGB ink take on such a document. to_document true: sRGB → profile; false: profile → sRGB. rgb and out
 * are 3 floats in 0…1 (input clamped). False, out untouched, on NULL, non-RGB bytes, a profile with a
 * matrix/TRC model (the host converts those itself) or tables that cannot convert that way. */
bool dr_authored_color(const uint8_t *profile, size_t len, bool to_document, const float *rgb, float *out);

/* --- the conversion policy --- */

/* What a conversion of `doc` to the profile in `icc` under `options_json`
 * WOULD do, without doing any of it: owned JSON
 *   {"adjustments":[{"index","name","op","action":"kept"|"baked","reason"}],
 *    "options":{...}}
 * Free with dr_string_free. NULL with err_out set when the conversion would
 * be REFUSED — that sentence is what a host disables its OK button with. A
 * NULL options_json means the defaults. The document's own profile bytes are
 * no change at any options, before every refusal: "adjustments":[]. After
 * that rule, a destination whose matrix/TRC tone curves fail the precision
 * check refuses with dr_icc_describe's "convert_refusal" sentence,
 * before any retained-source refusal. A preserve-appearance check renders
 * each baking projection under `control` (may be NULL): a cancel returns NULL
 * with "operation cancelled"; a control is single-use. */
char *dr_doc_conversion_preflight(const DrDocument *doc, const uint8_t *icc,
                                  size_t len, const char *options_json,
                                  const DrJobControl *control,
                                  char **err_out);

/* Converts `doc` into the profile in `icc` under `options_json`, writing the
 * adjustment report (the array above) through `report_out` (may be NULL) as
 * an owned JSON string. NULL WITHOUT an error is the ordinary refusal
 * (nothing would change — always so for the document's own profile bytes, at
 * any options and before every refusal); NULL WITH one is the preflight's
 * sentence. With options_json NULL or at its defaults this is
 * dr_doc_convert_to_profile, byte for byte. */
DrDocument *dr_doc_convert_to_profile_with(const DrDocument *doc,
                                           const uint8_t *icc, size_t len,
                                           const char *options_json,
                                           const DrJobControl *control,
                                           char **report_out, char **err_out);

/* Why an Assign on `doc` would reinterpret a retained source, as owned JSON
 * {"refusal":null|string,"retained_sources":n}. Free with
 * dr_string_free; NULL on a NULL doc. dr_doc_assign_profile refuses with
 * that sentence in err_out whenever "refusal" is non-null. */
char *dr_doc_assign_refusal(const DrDocument *doc);

/* Why assigning the profile in `icc` to `doc` would land a document that
 * cannot render: an owned sentence (free with dr_string_free) when the
 * profile has no matrix/TRC model and the document holds a document_v2
 * Exposure/White Balance layer (named) or uses linear-light evaluation; and
 * when the profile's matrix/TRC tone curves fail the precision
 * check and the document uses linear-light evaluation, holds text/shape
 * artwork or holds a document_v2 Exposure/White Balance layer (named), in
 * Assign words naming the profile, checked in that order.
 * NULL on a NULL doc, on bytes dr_doc_assign_profile refuses as a
 * profile, on the document's current profile bytes, and when there is no
 * refusal. dr_doc_assign_profile and dr_doc_assign_profile_rasterizing
 * refuse with this sentence in err_out whenever it is non-NULL. */
char *dr_doc_assign_profile_refusal(const DrDocument *doc, const uint8_t *icc,
                                    size_t len);

/* Rasterizes every layer still bound to a retained source and then assigns
 * the profile — ONE result, which the host commits as ONE undo step. NULL
 * WITHOUT an error on a NULL doc and when nothing would change. NULL WITH
 * err_out set (free with dr_string_free) for dr_doc_assign_profile's
 * profile-argument sentences, with dr_doc_assign_profile_refusal's sentence
 * whenever it is non-NULL, and with the rasterization's own sentence when a
 * bound layer cannot be rasterized (its pixels are locked). */
DrDocument *dr_doc_assign_profile_rasterizing(const DrDocument *doc,
                                              const uint8_t *icc, size_t len,
                                              char **err_out);

/* The typed twins. The preflight's JSON adds "retained_bytes" and
 * "scratch_bytes": the colour plan its own admission checks. The Convert's
 * job plan is dr_precision_doc_conversion_plan, which also charges a
 * preserve-appearance Convert's bakes. The same-bytes
 * rule holds after that admission: the preflight answers the plan with
 * "adjustments":[] and the convert NULL without an error. The
 * "convert_refusal" rule above holds for the typed preflight too, and so
 * does the `control` rule: a cancel returns NULL with "operation cancelled";
 * a control is single-use.
 * A 32-BIT FLOAT document has one conversion, the matrix in linear
 * light with no clamp (dr_precision_doc_convert_to_profile's float rule), at
 * relative, perceptual or saturation intent with black-point compensation off
 * and adjustments "keep", which all give the same samples. After the
 * same-bytes rule the preflight, dr_precision_doc_conversion_plan and the
 * convert refuse the first of absolute intent, black-point compensation and
 * preserve-appearance: "Convert to Profile in a 32-bit float document
 * converts in linear light with no clamp (relative colorimetric, no
 * black-point compensation): {absolute colorimetric intent | black-point
 * compensation | preserve-appearance adjustments} is unavailable there.",
 * then the profile refusals of the float colour plan. */
char *dr_precision_doc_conversion_preflight(const DrPrecisionDocument *document,
                                            const uint8_t *icc, size_t len,
                                            const char *options_json,
                                            uint64_t limit, uint64_t other,
                                            const DrJobControl *control,
                                            char **err_out);
/* An Indexed Color document (#1 Indexed phase 3a) converts its colour
 * table under options_json (each entry through this conversion, read at
 * 8-bit codes) and keeps every index; a profile that is not RGB refuses. */
DrPrecisionDocument *dr_precision_doc_convert_to_profile_with(
    const DrPrecisionDocument *document, const uint8_t *icc, size_t len,
    const char *options_json, uint64_t limit, uint64_t other_retained_bytes,
    const DrJobControl *control, char **report_out, char **err_out);
char *dr_precision_doc_assign_refusal(const DrPrecisionDocument *document);
/* dr_precision_doc_convert_to_profile_with and
 * dr_precision_doc_assign_profile_rasterizing refuse a Lab Color document
 * with dr_precision_doc_assign_profile's Lab sentence (#1 Lab phase 5b). */
DrPrecisionDocument *dr_precision_doc_assign_profile_rasterizing(
    const DrPrecisionDocument *document, const uint8_t *icc, size_t len,
    uint64_t limit, uint64_t other, const DrJobControl *control,
    char **err_out);

/* --- soft proofing --- */

/* Renders `flat` — a projection whose numbers are in the profile `doc_icc`
 * describes — through the proof profile in `proof_icc` under `setup_json`,
 * returning a NEW image (free with dr_image_free). `info_out` (may be NULL)
 * receives owned JSON {"space":"document"|"srgb","out_of_gamut":n}; the space
 * is what the host must TAG the result as. The master is never touched.
 * Both byte exports admit width*height*5 + the parsed proof profile's bytes +
 * the colour memo's peak + 10*doc_len (the parsed document profile and its
 * re-parse), plus `flat`'s own RGBA bytes and `other`, against `limit`, then
 * reserve that figure on `control` (may be NULL) — the typed twins' order. On
 * refusal or cancel both return NULL/false; the render leaves `info_out` and
 * the mask leaves `out` and `count_out` untouched. */
DrImage *dr_proof_render(const DrImage *flat, const uint8_t *doc_icc,
                         size_t doc_len, const uint8_t *proof_icc,
                         size_t proof_len, const char *setup_json,
                         uint64_t limit, uint64_t other,
                         const DrJobControl *control, char **info_out,
                         char **err_out);

/* Writes the per-pixel gamut mask for the same inputs into `out`, which the
 * caller declares to be `len` bytes — 255 out of gamut, 0 inside, `len` =
 * width * height recomputed from the image's own dimensions. `count_out` (may
 * be NULL) receives how many pixels were flagged. */
bool dr_proof_gamut_mask(const DrImage *flat, const uint8_t *doc_icc,
                         size_t doc_len, const uint8_t *proof_icc,
                         size_t proof_len, const char *setup_json,
                         uint64_t limit, uint64_t other,
                         const DrJobControl *control, uint8_t *out,
                         size_t len, uint64_t *count_out, char **err_out);

/* The typed twins: an already captured u16 projection in (its own profile IS
 * the document's), an RGBA8 DISPLAY image out — the extra depth protects the
 * chain's arithmetic, not the screen. Both admit width*height*5 + the parsed
 * proof profile's bytes + the colour memo's peak (at most 2^20 entries,
 * 75 MiB at the cap), plus the projection's retained bytes and `other`,
 * against `limit`, then reserve that figure on `control` (may be NULL). On
 * cancel both return NULL/false with "operation cancelled"; the mask leaves
 * `out` and `count_out` untouched. A projection in the Lab twin (and the byte
 * exports' doc_icc the twin) refuses: "a Lab Color projection proofs through
 * its Display P3 view (dr_precision_image_lab_display)" (#1 Lab phase 5b);
 * the host proofs that view. */
DrImage *dr_precision_proof_render(const DrPrecisionImage *projection,
                                   const uint8_t *proof_icc, size_t proof_len,
                                   const char *setup_json, uint64_t limit,
                                   uint64_t other, const DrJobControl *control,
                                   char **info_out, char **err_out);
bool dr_precision_proof_gamut_mask(const DrPrecisionImage *projection,
                                   const uint8_t *proof_icc, size_t proof_len,
                                   const char *setup_json, uint64_t limit,
                                   uint64_t other, const DrJobControl *control,
                                   uint8_t *out, size_t len, uint64_t *count_out,
                                   char **err_out);

/* Color Range ▸ Out Of Gamut: coverage = the source alpha where the gamut warning's rule flags the
 * pixel (inverted when `invert`), else 0. `len` must equal width*height. Admits and reserves the matching
 * gamut mask's scratch plus the output; returns false and leaves `out` untouched on refusal or cancel. */
bool dr_selection_out_of_gamut(const DrImage *flat, const uint8_t *doc_icc, size_t doc_len,
    const uint8_t *proof_icc, size_t proof_len, const char *setup_json, bool invert, uint64_t limit,
    uint64_t other, const DrJobControl *control, uint8_t *out, size_t len, char **err_out);
/* The bytes dr_selection_out_of_gamut admits for a width x height image: byte_proof_scratch_pixels plus
 * the width*height output. 0 when proof_icc is NULL or does not parse. */
uint64_t dr_selection_out_of_gamut_scratch(uint32_t width, uint32_t height, size_t doc_len,
    const uint8_t *proof_icc, size_t proof_len);

/* ---- Format additions (Phase 11D) --------------------------------------
 *
 * Four additions, each its own bounded flow:
 *
 * HEIC is HOST-ENCODED. DR_FORMAT_HEIC is refused by dr_doc_save_image and
 * every flat writer; only the export workspace accepts format "heic", and
 * the result it returns is PENDING — its bytes are a TIFF INTERMEDIATE
 * (info's "host_encoded" is true and "intermediate" is "tiff"),
 * dr_export_result_write and dr_export_result_preview both refuse it, and
 * the host transcodes those bytes with ImageIO and hands the written file
 * back through dr_export_result_adopt_host_bytes. `bit_depth` and `carried`
 * are what an inspection of the WRITTEN bytes measured, never what was asked
 * for. AVIF (DR_FORMAT_AVIF, format "avif") is host-encoded the same way at
 * every one of those points.
 *
 * JPEG XL (DR_FORMAT_JXL, format "jxl") is encoded by the core: lossless,
 * 8-bit, sRGB pixels only, each side at least 2 px. Its result is never
 * pending and dr_export_result_write takes it as it is; but the core decodes
 * no JPEG XL, so dr_export_result_preview refuses it by name.
 *
 * info's "host_preview" is true for "heic", "avif" and "jxl": the host
 * previews such a result by decoding the result's own (adopted) bytes.
 * "host_encoded" is true for "heic" and "avif" only.
 *
 * The three typed metadata exports are the byte twins' contracts verbatim,
 * for typed (16-bit) documents: host-decoded imports and typed RAW can now
 * reconstruct bounded capture packets and the original PPI into the typed
 * document, so a typed export carries them. */

/* Adopt the bytes the host encoder wrote for a PENDING host-encoded export.
 * Returns a NEW result (free with dr_export_result_free) whose format stays
 * "heic", whose "intermediate" becomes null, whose bit_depth is the depth the
 * host actually wrote (8 or 10) and whose carried/metadata report is rebuilt
 * from the measured `carried` bits. Refused on a result that is not pending,
 * on NULL/empty bytes, and past the bounded encoded-byte limit. */
DrExportResult *dr_export_result_adopt_host_bytes(const DrExportResult *result,
                                                  const uint8_t *bytes, size_t len,
                                                  uint32_t bit_depth, uint32_t carried,
                                                  char **err_out);

/* Replace capture packets from a JPEG/PNG/TIFF metadata carrier, at most
 * 49 MiB — the typed twin of dr_doc_import_metadata, same rules: no pixel
 * decode, no profile/resolution adoption, pure, NULL for NULL input or
 * unchanged packets, warnings_out a JSON array (free with dr_string_free). */
DrPrecisionDocument *dr_precision_doc_import_metadata(const DrPrecisionDocument *document,
                                                      const uint8_t *bytes, size_t len,
                                                      char **warnings_out, char **err_out);

/* Stores one packet verbatim, or CLEARS it when `bytes` is NULL — the typed
 * twin of dr_doc_set_metadata, same 16 MiB cap and same no-change NULL. NULL
 * for a NULL document or no change leaves err_out untouched; a refusal (a
 * kind outside DrMetadataKind, a packet over 16 MiB) returns NULL and sets
 * err_out (free with dr_string_free). */
DrPrecisionDocument *dr_precision_doc_set_metadata(const DrPrecisionDocument *document,
                                                   DrMetadataKind kind,
                                                   const uint8_t *bytes, size_t len,
                                                   char **err_out);

/* Pure setter for the typed document's print resolution — the twin of
 * dr_doc_set_resolution, same sanitizing ([1, 30000], four decimals) and the
 * same NULL on no change. NULL for a NULL document or no change leaves
 * err_out untouched; a non-finite or non-positive component returns NULL and
 * sets err_out (free with dr_string_free). PIXELS NEVER CHANGE. */
DrPrecisionDocument *dr_precision_doc_set_resolution(const DrPrecisionDocument *document,
                                                     float ppi_x, float ppi_y,
                                                     char **err_out);

/* The typed twins of dr_doc_file_info and dr_doc_set_file_info: the
 * same JSON, limits, packets and NULL-on-no-change. */
char *dr_precision_doc_file_info(const DrPrecisionDocument *document);
DrPrecisionDocument *dr_precision_doc_set_file_info(const DrPrecisionDocument *document,
                                                    const char *json, char **err_out);

/* ---- Versioned evaluation (Phase 11E) -----------------------------------
 *
 * A document carries an EVALUATION POLICY: `encoded` (0), the unchanged
 * default under which the projection composites the stored code values, or
 * `linear_light` (1), under which every sample is decoded through the
 * document profile's TRC, composited in linear light at f32 and re-encoded
 * exactly once at the output. An encoded document takes byte-for-byte the
 * code path it always took.
 *
 * THE PROJECTION IS THE INVARIANT. Only the operators phase 1 migrated are
 * evaluated in linear light; everything else is refused BY NAME — at the
 * switch, at every later edit that would introduce it (the host guards
 * before it commits), and, as the backstop, at every render of a document
 * that carries one: dr_doc_flattened, the typed render and the typed render
 * plan all fail with that sentence. dr_doc_linear_light_refusal /
 * dr_precision_doc_linear_light_refusal is that walk, asked directly;
 * dr_evaluation_operators is the table of what is migrated and what is not.
 *
 * PERSISTENCE. `.rz` gains an OUTERMOST conditional wrapper written ONLY for
 * a linear-light document: DRDC u32 38 (byte) / 39 (typed) before every prior
 * version word, then the exact prior stream, then `EVAL`, u32 schema = 1,
 * u32 policy = 1. An encoded document's bytes — first word included — are
 * exactly what the prior writer made. The typed descriptor's "evaluation" tag
 * is "encoded_rgb_f32_v1" or "linear_light_f32_v1". */

#define DR_EVALUATION_ENCODED 0
#define DR_EVALUATION_LINEAR_LIGHT 1

/* Maps an evaluation NAME ("encoded" / "linear_light") to its DR_EVALUATION_*
 * value; -1 for NULL and for a name this build does not know. */
int dr_evaluation_from_name(const char *name);

/* The operator table as owned JSON — what linear-light evaluation evaluates
 * and what it refuses, with the phase that owns each refusal:
 * {"migrated":[..],"refused":[{"operator":..,"phase":..}],"resize_refusal":".."}.
 * "resize_refusal" is the sentence Image Size carries under linear-light
 * evaluation, so a host guard never writes a second copy of it.
 * Free with dr_string_free. */
char *dr_evaluation_operators(void);

/* The document's evaluation policy; DR_EVALUATION_ENCODED for NULL. */
int dr_doc_evaluation(const DrDocument *doc);

/* The first operator in this document linear-light evaluation cannot
 * evaluate, as an owned sentence naming the layer and the operator; NULL when
 * there is nothing to refuse — which is every encoded document, answered
 * without walking the stack. Free a non-NULL result with dr_string_free. */
char *dr_doc_linear_light_refusal(const DrDocument *doc);

/* The document under `policy`. NULL WITHOUT an error when it is already in
 * that policy; NULL WITH one for an unknown policy, a profile with no
 * matrix/TRC model this build can decode, and an operator this phase does not
 * evaluate (the sentence above). */
DrDocument *dr_doc_set_evaluation(const DrDocument *doc, int policy, char **err_out);

/* The typed twins, same contracts. */
int dr_precision_doc_evaluation(const DrPrecisionDocument *document);
char *dr_precision_doc_linear_light_refusal(const DrPrecisionDocument *document);
DrPrecisionDocument *dr_precision_doc_set_evaluation(const DrPrecisionDocument *document,
                                                     int policy, char **err_out);

/* Owned JSON difference between two images over ALL RGBA samples:
 * {"depth":8|16,"width":u32,"height":u32,"mean_abs":f64,"max_abs":u32,
 *  "changed_fraction":f64}. A NULL handle or a dimension mismatch is an ERROR
 * through err_out, not a refusal. Free with dr_string_free. */
char *dr_image_compare(const DrImage *a, const DrImage *b, char **err_out);
char *dr_precision_image_compare(const DrPrecisionImage *a, const DrPrecisionImage *b,
                                 char **err_out);

/* ---- Float documents (Phase 11F) ----------------------------------------
 *
 * A typed document may be a 32-bit FLOAT document: every raster source is
 * straight RGBA32 float, its RGB components are LINEAR LIGHT in the document
 * profile's primaries, and their range is EXTENDED — any finite value with
 * |v| <= 65504, above white and below zero alike. Alpha stays coverage in
 * [0,1]. A float document is therefore ALWAYS DR_EVALUATION_LINEAR_LIGHT:
 * dr_precision_doc_set_evaluation to linear_light is a no-op and to encoded
 * is an error.
 *
 * WHERE ONE COMES FROM. Opening a genuine floating-point TIFF, an OpenEXR
 * file or a Radiance RGBE (.hdr) file through
 * dr_precision_doc_open_preserving, and reading a version-40 `.rz`. A host
 * decoder's own extended-range render enters through
 * dr_precision_doc_from_rgba32 (the 32-bit float Camera RAW development),
 * and dr_precision_doc_from_image wraps an f32 image it already holds. No
 * operation promotes a u16 document to float — there is no HDR information
 * to invent. dr_image_open and dr_doc_open REFUSE an OpenEXR or Radiance
 * signature by name, so neither can arrive clamped to 8 bits.
 *
 * OPENEXR AND RADIANCE IN. Neither carries an ICC profile; each states xy
 * primaries (`chromaticities`, `PRIMARIES=`). Absent: the built-in sRGB with
 * an import warning. Within 0.002 of Rec.709: sRGB. Within 0.002 of Display
 * P3: the built-in Display P3. Anything else: the samples are converted once,
 * in linear light, to sRGB primaries (Bradford between the whites) with a
 * warning naming the conversion; negatives stay. OpenEXR: the first part
 * that is not deep and has channels R, G and B opens (A when present, at the
 * largest resolution level); its DISPLAY window is the canvas and its DATA
 * window the one layer at its offset; alpha is ASSOCIATED and is
 * unpremultiplied ("associated alpha unpremultiplied"); other parts, a
 * window difference and a pixel aspect other than 1 are warnings. A file
 * holding any deep part, or no part with R, G and B, refuses by name.
 * Radiance: `-Y H +X W` and 32-bit_rle_rgbe only (xyze and the other
 * orientations refuse by name); it opens opaque; EXPOSURE and COLORCORR are
 * disclosed in a warning and not applied.
 *
 * THE FINITE RULE. At IMPORT ONLY, NaN becomes 0, +/-infinity becomes
 * +/-65504, a larger magnitude is clamped and alpha is clamped into [0,1];
 * the counts are reported in the document's import warnings ("{n} non-finite
 * samples replaced" / "{n} out-of-range samples clamped"). Every render and
 * edit output is CHECKED and refuses rather than repairing.
 *
 * THE BOUNDED OPERATOR SET. Normal, Multiply, Darken, Lighten and Difference
 * blending, plus the nine modes kept at 32 bits whose measured
 * formula a float document can store (Dissolve, Darker/Lighter Color, Linear
 * Dodge, Subtract, Hue, Saturation, Color, Luminosity); layer opacity/name/visibility/clipping/duplication/grouping;
 * masks and channels; Exposure and White Balance adjustment layers; placed
 * images; sample,
 * histogram, render, convert, export, native save and the tone map. Every
 * other operator refuses BY NAME — "<operator> is unavailable in a 32-bit
 * float document" — through dr_precision_doc_float_refusal, which
 * dr_precision_doc_linear_light_refusal also returns, so a host that already
 * guards the 11E policy needs no new guard. dr_float_operators is the table.
 *
 * DISPLAY AND DELIVERY. A float sample never reaches RGBA8 directly:
 * dr_precision_image_preview_rgba8 refuses float storage, and display goes
 * through dr_precision_doc_display_rgba8, which applies the document's TONE
 * MAP and then the profile's forward curve. The map is
 * {"operator":"clip"} or {"operator":"reinhard","white":W} with W in
 * [1,65504] (4.0 by default); `white` is admitted for reinhard only and any
 * other key refuses by name. dr_precision_doc_display_rgba8_view is the same
 * path under a VIEW — an exposure in stops and optionally another map — and
 * changes no document. An export at bit_depth 8 or 16 runs the document's
 * map first; bit_depth 32 writes the samples and requires format "tiff",
 * "exr" or "hdr", no sharpen, profile document and dither none:
 *   "tiff"  four samples, unassociated alpha, Deflate, the profile embedded;
 *   "exr"   OpenEXR, RGBA, ZIP scanlines, ASSOCIATED alpha (each RGB sample
 *           times alpha), the `chromaticities` attribute;
 *   "hdr"   Radiance RGBE, max(rgb, 0) * alpha, one `PRIMARIES=` line.
 * "float_samples":"f32" (default) or "f16" chooses the sample width of a
 * "tiff" or "exr"; "f16" writes the nearest IEEE half (ties to even) and
 * refuses with any other format or bit_depth. "exr" and "hdr" exist ONLY at
 * bit_depth 32, so only a float document writes them. Neither converts a
 * pixel: the file declares the document profile's primaries (the Rec.709
 * constants for an sRGB profile, the Display P3 constants for Display P3,
 * otherwise the profile's own native primaries) and carries no ICC profile
 * and no metadata packet (dr_format_carries is 0 for both). dr_export_result_info
 * reports "float_samples":"f32"|"f16"|"rgbe" on a 32-bit result, and
 * dr_export_result_preview decodes that result's own bytes and shows them
 * under the exporting document's tone map. Size mode none writes the samples
 * EXACTLY (an OpenEXR file reopened and exported at f32 is bit-exact where
 * alpha is 1, within 2 ulp where 0 < alpha < 1 with alpha exact, and RGB 0
 * where alpha is 0); any other size mode resamples the composite
 * through Image Size's extended-range resize (premultiplied, RGB clamped to
 * the composite's own visible range, an alpha below 2^-24 transparent).
 *
 * PERSISTENCE. `.rz` gains an OUTERMOST conditional wrapper written ONLY for
 * a float document: DRDC u32 40 before every prior version word (the next
 * word is ALWAYS 39, because a float document is always linear light), the
 * body header's storage tag is 2 instead of 1, each raster record's payload
 * is u32 width, u32 height, u64 length = w*h*16 and raw little-endian f32
 * RGBA instead of a PNG, and the file closes with `FLOT`, u32 schema = 1,
 * u32 operator (0 clip / 1 reinhard), f32 white. Storage tag 2 is admitted
 * only under 40, so every earlier reader refuses a float document rather
 * than misreading it. A u16 document's bytes are unchanged. */

#define DR_TONE_MAP_JSON_MAX 4096

/* True when this typed document is a 32-bit float document; false for NULL. */
bool dr_precision_doc_is_float(const DrPrecisionDocument *document);

/* The float mode's operator table as owned JSON:
 * {"admitted":[..],"refused":[{"operator":..,"reason":..}],
 *  "tone_maps":["clip","reinhard"],"range":{..},"storage":"rgba32_float"}.
 * Free with dr_string_free. */
char *dr_float_operators(void);

/* The document's tone map as owned JSON; NULL for NULL and for a document
 * that is not a float document. Free a non-NULL result with dr_string_free. */
char *dr_precision_doc_tone_map(const DrPrecisionDocument *document);

/* The document under the tone map `json` describes (at most
 * DR_TONE_MAP_JSON_MAX bytes). NULL WITHOUT an error when the map is
 * unchanged; NULL WITH one for a malformed description, an unknown operator
 * or field, and for a document that is not a float document. */
DrPrecisionDocument *dr_precision_doc_set_tone_map(const DrPrecisionDocument *document,
                                                    const char *json, char **err_out);

/* The first operator in this document the float mode cannot evaluate, as an
 * owned sentence; NULL when there is nothing to refuse — which is every u16
 * document, answered without a walk. Free with dr_string_free. */
char *dr_precision_doc_float_refusal(const DrPrecisionDocument *document);

/* The float projection `image` as display RGBA8 through `document`'s own
 * tone map, freed by dr_image_free. The ONLY way a float sample becomes
 * display bytes. */
DrImage *dr_precision_doc_display_rgba8(const DrPrecisionDocument *document,
                                         const DrPrecisionImage *image, uint64_t limit,
                                         uint64_t other_retained_bytes,
                                         const DrJobControl *control, char **err_out);

/* The float projection `image` as display RGBA8 under a VIEW, freed
 * by dr_image_free: the tone map tone_map_json describes (at most
 * DR_TONE_MAP_JSON_MAX bytes; NULL = the document's own map) and
 * exposure_stops of exposure, gain = 2^stops on RGB before the map, alpha
 * untouched. tone_map_json NULL = the document's own map. (NULL, 0.0) is
 * byte for byte dr_precision_doc_display_rgba8. Changes nothing in
 * `document`; the same scratch as dr_precision_doc_display_rgba8. Refuses a
 * malformed map, an exposure that is not finite or lies outside [-20, 20]
 * ("view exposure must be a finite number of stops in [-20, 20]") and
 * everything dr_precision_doc_display_rgba8 refuses. control is single-use. */
DrImage *dr_precision_doc_display_rgba8_view(const DrPrecisionDocument *document, const DrPrecisionImage *image,
    const char *tone_map_json, double exposure_stops, uint64_t limit, uint64_t other_retained_bytes,
    const DrJobControl *control, char **err_out);

/* The float projection `image` as the EXTENDED-RANGE view an EDR canvas
 * shows (v1.x): no tone map; each RGB sample times gain = 2^stops
 * (the same [-20, 20] range as the SDR view), held to +/-65504, times alpha;
 * RGB and alpha each rounded to nearest-even binary16. out_len must equal
 * width*height*8; writes RGBA binary16 LE premultiplied, linear in the
 * document primaries (the host wraps it in the profile's extended-linearized
 * space). peak_out (nullable) receives the largest RGB component written.
 * Its plan is 46 bytes a pixel plus bookkeeping (the buffer and what the window
 * does with it, measured: float_view.rs), admitted under limit beside
 * other_retained. false with err_out set for a document that is not a float
 * document, an image that is not rgba32_float, a wrong out_len, a bad
 * exposure, scratch past limit and a cancel. Changes nothing in `document`.
 * control is single-use. */
bool dr_precision_doc_display_extended(const DrPrecisionDocument *document, const DrPrecisionImage *image,
    double exposure_stops, uint8_t *out, size_t out_len, float *peak_out, uint64_t limit,
    uint64_t other_retained, const DrJobControl *control, char **err_out);

/* Float layer `image`'s Layers-panel thumbnail as display RGBA8 through
 * `document`'s own tone map, freed by dr_image_free; no ICC.
 * Aspect-fit with the longest side max_side, NEVER upscaled (the fit of
 * dr_precision_image_thumbnail_rgba8). Below 16x the thumbnail per axis it
 * is byte for byte dr_precision_doc_display_rgba8 followed by
 * dr_image_resize (FILTER_BILINEAR); a larger image is box-reduced out of
 * its tiles through the tone map, resized at u16 and quantized once,
 * measured within 2/255 of that pair, with no whole-layer copy. Refuses a
 * document that is not a float document ("tone mapping requires a 32-bit
 * float document"), an image that is not rgba32_float ("tone mapping
 * requires rgba32_float storage"), scratch past limit and a cancel. control
 * is single-use. */
DrImage *dr_precision_doc_display_thumbnail_rgba8(const DrPrecisionDocument *document,
    const DrPrecisionImage *image, uint32_t max_side, uint64_t limit,
    uint64_t other_retained_bytes, const DrJobControl *control, char **err_out);

/* The deliberate conversion of a float document to the u16 mode through its
 * own tone map: one raster, linear-light evaluation, same profile, metadata,
 * resolution and channels. */
DrPrecisionDocument *dr_precision_doc_float_to_u16(const DrPrecisionDocument *document,
                                                    uint64_t limit,
                                                    uint64_t other_retained_bytes,
                                                    const DrJobControl *control, char **err_out);

/* The synchronous composite extended-range sample as owned JSON, freed with
 * dr_string_free: center_rgba32, mean_rgba32, above_white, below_zero and
 * stops. The float histogram and every scoped (layer/backdrop, selection-
 * weighted) float readout go through dr_precision_doc_scoped_analysis and
 * dr_precision_image_scoped_analysis, which answer float storage with the
 * float schema. */
char *dr_precision_image_float_sample(const DrPrecisionImage *image, int32_t x, int32_t y,
                                       uint32_t reach, char **err_out);

/* Merge to HDR (doc_merge_hdr.rs, merge_hdr.rs): 2..16 raster layers of
 * an 8-bit (dr_doc_*) or 16-bit (dr_precision_doc_*) document, each one
 * exposure of one scene, merged into a NEW 32-bit float document. The source
 * is never changed. Each layer is read on the canvas grid (its offset applied,
 * alpha 0 outside it; mask, opacity, blend and style ignored) and decoded to
 * linear light through the document profile's typed transfer, a byte widened
 * by 257 first.
 *
 * request_json (at most 64 KiB, unknown keys refused):
 *   {"version":1,"layers":[int...],"evs":[number...],"reference":int,
 *    "deghost":bool}
 * layers: 2..16 distinct indices of raster leaves (postorder record indices in
 * a typed document). evs: one finite number per layer, stops of exposure,
 * larger = more light. reference: one of `layers`, the layer whose scale the
 * result is on; every |ev - ev[reference]| <= 15. The exposures call takes
 * {"version":1,"layers":[...]} alone.
 *
 * The merge, with e_i = ev_i - ev_reference, over the frames with alpha > 0:
 * w_i = alpha_i * weight(level_i) * 2^e_i and rgb = sum(w_i * linear_i /
 * 2^e_i) / sum(w_i), where level is the largest stored channel over full
 * scale and weight is 0 at or below 0.02 and at or above 0.98, 1 on
 * [0.10, 0.90] and linear between. Alpha is the largest frame alpha; a pixel
 * no frame covers is transparent. Where no frame has weight, the shortest
 * exposure is taken alone when every covering frame is clipped
 * (all_clipped_pixels), else the longest exposure below the clip level
 * (all_dark_pixels). With deghost, a pixel where a frame and the reference
 * are both fully weighted and opaque and disagree by more than
 * max(4 sigma, ln 1.1) about that frame's median residual takes the reference
 * alone (deghosted_pixels); sigma is the frame's robust noise against the
 * reference, measured on a lattice of at most about 262144 samples.
 *
 * dr_*_merge_hdr_exposures: owned JSON {"evs":[...]} relative to the first
 * listed layer (evs[0] = 0), estimated from the pixels, or, when two frames
 * neighbouring in brightness share fewer than 64 well-exposed lattice
 * samples, {"evs":null,"reason":"the exposures of ... could not be estimated
 * (too few pixels are well exposed in both); give each frame's EV"}. That
 * case is a reply, not an error.
 * dr_*_merge_hdr_plan: owned JSON {"width","height","frames","output_bytes",
 * "scratch_bytes"}: the result's 16 bytes a pixel, and the working memory
 * beside it. The merge admits the source's retained bytes + both +
 * other_retained_bytes against limit and reserves both on control.
 * dr_*_merge_hdr: the new document, one raster layer "Merged HDR", with the
 * source's profile and resolution, no metadata and the clip tone map; free it
 * with dr_precision_doc_free. A Grayscale source (#1 phase 6b) gives a
 * 32-bit Grayscale document in its Gray profile, the merge folded to its gray
 * by Mode > Grayscale's rule (its 16 bytes a pixel admitted beside the
 * result). report_out (may be NULL) receives, on success
 * only, owned JSON {"reference","all_clipped_pixels","all_dark_pixels",
 * "deghosted_pixels","noise_sigma":[number|null...]} (noise_sigma per listed
 * layer; null for the reference, an unjudged frame, and every frame without
 * deghost).
 *
 * Every call returns NULL with a message for a NULL handle or request, a
 * malformed request, and a refusal naming the layer: a 32-bit float document
 * ("... is already 32-bit float; Merge to HDR takes 8- or 16-bit exposures"),
 * a group or adjustment layer, a profile that cannot carry a float document,
 * an EV farther than 15 from the reference's; and for a working-memory
 * refusal or a cancel ("operation cancelled"). Strings are freed with
 * dr_string_free. control is single-use and may be NULL. */
char *dr_doc_merge_hdr_exposures(const DrDocument *doc, const char *request_json,
    const DrJobControl *control, char **err_out);
char *dr_doc_merge_hdr_plan(const DrDocument *doc, const char *request_json, char **err_out);
DrPrecisionDocument *dr_doc_merge_hdr(const DrDocument *doc, const char *request_json,
    uint64_t limit, uint64_t other_retained_bytes, const DrJobControl *control,
    char **report_out, char **err_out);
char *dr_precision_doc_merge_hdr_exposures(const DrPrecisionDocument *document,
    const char *request_json, const DrJobControl *control, char **err_out);
char *dr_precision_doc_merge_hdr_plan(const DrPrecisionDocument *document,
    const char *request_json, char **err_out);
DrPrecisionDocument *dr_precision_doc_merge_hdr(const DrPrecisionDocument *document,
    const char *request_json, uint64_t limit, uint64_t other_retained_bytes,
    const DrJobControl *control, char **report_out, char **err_out);

/* HDR Toning (doc_hdr_toning.rs, hdr_toning.rs, hdr_local_adaptation.rs): a
 * document's composite flattened and tone mapped at native depth into a NEW
 * one-raster 16-bit document "Background" (same profile, metadata, resolution,
 * channels, guides and paths; the source is unchanged). request_json (at
 * most 4 KiB, unknown keys refused) is {"method":"exposure_gamma",
 * "exposure":f (-20..20 stops, default 0),"gamma":f (0.01..9.99, default
 * 1)}, {"method":"highlight_compression"}, {"method":"equalize_histogram"}
 * or {"method":"local_adaptation","radius":1..500 (16),"strength":0.1..4
 * (0.52),"smooth_edges":bool (false),"gamma":0.1..2 (1),"exposure":-5..5
 * (0),"detail":-100..300 (30),"shadow","highlight","vibrance":-100..100 (0),
 * "saturation":-100..100 (20),"toning_curve":[[in,out],...] (2-16 points in
 * 0..255, identity by default)}, every key optional and each refusal naming
 * its key. A CMYK document refuses ("hdr toning is unavailable in a
 * CMYK document"), an Indexed Color one for good ("hdr_toning is not
 * available in Indexed Color.", #1 Indexed phase 3b,
 * the histogram too), as do a profile with no matrix/TRC model, a
 * working-memory refusal and a cancel. A float source's result evaluates in
 * linear light; a 16-bit source's keeps its evaluation. An 8-bit document
 * is toned by the host as promote_flattened, this call, then downconvert
 * (dither none). The plans answer owned JSON {"working_memory_plan_bytes",
 * "output_bytes"} (free with dr_string_free); bit_depth is 16, or 8 for a
 * float source. A Grayscale document (#1 phase 2e) at 8 or 16 bits tones
 * its R = G = B composite, each mapped pixel folded back to one gray value
 * (the value when the three agree, else the Rec. 709 luma), into a Grayscale
 * result at its own depth (bit_depth must be that depth, else "a <b>-bit
 * Grayscale document keeps its depth through hdr toning"; output 8 B/px; no
 * host narrowing). A 32-bit one (#1 phase 5a) tones into a 16-bit
 * Grayscale document; its plan takes bit_depth 8 or 16 (else "hdr toning
 * converts a 32-bit Grayscale document to 8 or 16 bits per channel; <n> was
 * given"), output 8 B/px, the 8-bit plan also holding the host's narrowing
 * by dr_precision_doc_gray_profile_edit's depth op. control is single-use. */
char *dr_precision_doc_hdr_toning_plan(const DrPrecisionDocument *document,
    const char *request_json, uint32_t bit_depth, char **err_out);
char *dr_doc_hdr_toning_plan(const DrDocument *doc, const char *request_json, char **err_out);
DrPrecisionDocument *dr_precision_doc_hdr_toning(const DrPrecisionDocument *document,
    const char *request_json, uint64_t limit, uint64_t other_retained_bytes,
    const DrJobControl *control, char **err_out);
/* The Toning Curve's histogram of the composite: owned JSON {"bins":[256
 * alpha-weighted bins of the clamped log-luminance position],"low_stops",
 * "high_stops"} (log2 of linear luminance at the range's ends), admitted and
 * reserved under the default Local Adaptation plan. NULL with a message on a
 * NULL handle, a CMYK document, a profile that cannot carry float, a
 * working-memory refusal and a cancel. Free with dr_string_free. */
char *dr_precision_doc_hdr_toning_histogram(const DrPrecisionDocument *document, uint64_t limit,
    uint64_t other_retained_bytes, const DrJobControl *control, char **err_out);


/* ------------------------------------------------------------------------ */
/* Puppet Warp                                                              */
/* ------------------------------------------------------------------------ */
/* Edit > Puppet Warp: a triangle mesh over a layer's opaque pixels, pins,
 * an as-rigid-as-possible solve and the resample through it. All
 * coordinates are canvas pixels.
 *
 * The request (strict, at most 64 KiB):
 * {"op":"puppet_warp","layer":N,"mode":"rigid"|"normal"|"distort",
 *  "density":"fewer"|"normal"|"more","expansion":int,
 *  "pins":[{"from":[x,y],"to":[x,y],"depth":int,"rotate":degrees|null}]}.
 * Defaults: mode normal, density normal, expansion 2, depth 0, rotate null
 * (Auto). 1..256 pins; coordinates finite within +-1000000; depth
 * -1000..1000; expansion -100..100; rotate finite degrees, clockwise-positive
 * on the y-down canvas. A pin's from snaps to the nearest vertex of the rest
 * triangle holding it; its target is to when from IS that vertex, else the
 * vertex moved by to - from; pinned vertices land on their targets exactly.
 * Refusals, verbatim ({i} the 0-based pin index):
 * "Puppet Warp needs at least one pin.", "Puppet Warp takes at most 256
 * pins.", "Puppet Warp pin {i} is outside the mesh.", "Puppet Warp pins {i}
 * and {j} share a mesh vertex.", "Puppet Warp needs pixels to mesh, and there
 * are none at expansion {e} px.", "Puppet Warp would make the layer larger
 * than the pixel limit.", "Puppet Warp mode must be rigid, normal or
 * distort.", "Puppet Warp density must be fewer, normal or more.", "Puppet
 * Warp expansion must be a whole number of pixels from -100 to 100.",
 * "Puppet Warp pin {i} needs from and to as two finite numbers within
 * 1000000.", "Puppet Warp pin {i} depth must be a whole number from -1000 to
 * 1000.", "Puppet Warp pin {i} rotate must be a finite angle in degrees or
 * null.", a group or adjustment layer, an artwork or retained-source layer
 * ("Rasterize layer N before Puppet Warp"), and the Pixels/Position locks.
 *
 * The float form is the selected-pixels edit
 * {"op":"puppet_selected","layer":N, the same fields, "bounds":[x,y,w,h]?}
 * through dr_doc_selected_pixels_edit(_plan) and dr_precision_doc_edit(_plan):
 * the layer's pixels x the selection coverage, warped and landed source-over
 * (the hole a*(1-c)); an empty float refuses "The selection holds no
 * pixels to puppet warp."
 *
 * dr_*_puppet_mesh: the mesh of layer's opaque pixels (NULL coverage) or of
 * its opaque AND selected pixels (coverage canvas-sized u16, coverage_len
 * must equal width*height). options_json {"density","expansion"}. The build
 * is admitted first against limit and other. Free with dr_puppet_mesh_free.
 * dr_puppet_mesh_describe: {"vertices":[[x,y]],"triangles":[[a,b,c]],
 * "rect":[x,y,w,h]}.
 * dr_puppet_mesh_solve: pins_json {"mode","pins"}; the pose keeps its own
 * reference to the mesh, so freeing the mesh first is safe.
 * dr_puppet_pose_describe: {"positions":[x0,y0,...],"pins":[{"vertex":k,
 * "from":[x,y]}],"rect":[x,y,w,h]} (rect the deformed vertices' outward box).
 * dr_puppet_pose_render: the preview of proxy (a picture of the canvas rect
 * sx,sy,sw,sh at any scale), optionally times coverage (proxy-sized bytes),
 * through the pose; out_rect (4 doubles) receives the canvas rect x,y,w,h
 * the image covers. Free with dr_image_free.
 * dr_*_puppet_warp_plan: {"retained_bytes","scratch_bytes","rect":[x,y,w,h],
 * "vertices"}; an identity request (every to == from, rotate null or 0)
 * answers scratch_bytes 0.
 * dr_*_puppet_warp: the new document with the layer replaced by its warp
 * (the deformed mesh's box, not clipped to the canvas; a linked mask warps
 * along, an unlinked one stays), or NULL with no error for an identity
 * request. control is single-use and may be NULL.
 * Every call returns NULL with no error for a NULL handle, and NULL with a
 * message (freed with dr_string_free) for a refusal. */
typedef struct PuppetMesh DrPuppetMesh;
typedef struct PuppetPose DrPuppetPose;
DrPuppetMesh *dr_doc_puppet_mesh(const DrDocument *document, size_t layer,
    const uint16_t *coverage, size_t coverage_len, const char *options_json, uint64_t limit,
    uint64_t other, char **err_out);
DrPuppetMesh *dr_precision_doc_puppet_mesh(const DrPrecisionDocument *document, size_t layer,
    const uint16_t *coverage, size_t coverage_len, const char *options_json, uint64_t limit,
    uint64_t other, char **err_out);
char *dr_puppet_mesh_describe(const DrPuppetMesh *mesh, char **err_out);
DrPuppetPose *dr_puppet_mesh_solve(const DrPuppetMesh *mesh, const char *pins_json,
    char **err_out);
char *dr_puppet_pose_describe(const DrPuppetPose *pose, char **err_out);
DrImage *dr_puppet_pose_render(const DrPuppetPose *pose, const DrImage *proxy, int64_t sx,
    int64_t sy, uint32_t sw, uint32_t sh, const uint8_t *coverage, size_t coverage_len,
    double *out_rect, char **err_out);
void dr_puppet_pose_free(DrPuppetPose *pose);
void dr_puppet_mesh_free(DrPuppetMesh *mesh);
char *dr_doc_puppet_warp_plan(const DrDocument *document, const char *request, uint64_t limit,
    uint64_t other, char **err_out);
DrDocument *dr_doc_puppet_warp(const DrDocument *document, const char *request, uint64_t limit,
    uint64_t other, const DrJobControl *control, char **err_out);
/* An Indexed Color document refuses both for good ("puppet_warp is not
 * available in Indexed Color.", #1 Indexed phase 3a). */
char *dr_precision_doc_puppet_warp_plan(const DrPrecisionDocument *document,
    const char *request, uint64_t limit, uint64_t other, char **err_out);
DrPrecisionDocument *dr_precision_doc_puppet_warp(const DrPrecisionDocument *document,
    const char *request, uint64_t limit, uint64_t other, const DrJobControl *control,
    char **err_out);


/* ------------------------------------------------------------------------ */
/* Perspective Warp                                                         */
/* ------------------------------------------------------------------------ */
/* Edit > Perspective Warp: quads drawn over planes of the picture, joined
 * where they share corners, each plane resampled through its own
 * homography, joined planes continuous along their shared edges, and the
 * rest of the layer following an as-rigid-as-possible extension that
 * continues each plane across its border. All coordinates are canvas pixels.
 *
 * The request (strict, at most 64 KiB):
 * {"op":"perspective_warp","layer":N,
 *  "quads":[{"from":[x0,y0,x1,y1,x2,y2,x3,y3],"to":[...8]}],
 *  "straighten":[{"quad":i,"edge":k,"direction":d}]}.
 * 1..64 quads; corners finite within +-1000000, any consistent winding;
 * edge k runs from corner k to corner k+1 mod 4; at most 256 straighten
 * entries; d is "auto" (the default), "near_horizontal", "near_vertical",
 * "horizontal", "vertical" or a number of degrees. A corner whose from equals
 * an earlier quad's corner joins it; a side two quads hold is a shared edge.
 * Refusals, verbatim ({i} a 0-based index):
 * "Perspective Warp needs at least one quad.", "Perspective Warp takes at
 * most 64 quads.", "Perspective Warp quad {i} needs from and to as eight
 * finite numbers within 1000000.", "Perspective Warp quad {i} is degenerate:
 * its corners must form a convex quad with no three in a line.",
 * "Perspective Warp quad {i} folds: its warped corners must form a convex
 * quad with no three in a line, turning the same way.", "Perspective Warp
 * quads {j} and {i} share a corner but warp it to different points.",
 * "Perspective Warp straighten {s} must name a quad, an edge from 0 to 3 and
 * a direction.", "Perspective Warp takes at most 256 straighten entries.",
 * "Perspective Warp anchor must be a quad corner.", "Perspective Warp would
 * make the layer larger than the pixel limit.", a group or adjustment layer,
 * an artwork or retained-source layer ("Rasterize layer N before Perspective
 * Warp"), the Pixels/Position locks, and on a CMYK document the CMYK
 * refusal.
 *
 * The float form is the selected-pixels edit
 * {"op":"perspective_selected","layer":N,"quads","straighten",
 *  "bounds":[x,y,w,h]?} through dr_doc_selected_pixels_edit(_plan) and
 * dr_precision_doc_edit(_plan): the layer's pixels x the selection
 * coverage, warped and landed source-over (the hole a*(1-c)); an empty
 * float refuses "The selection holds no pixels to perspective warp."
 *
 * dr_perspective_warp_layout: fields {"quads","straighten"?,"anchor"?:[x,y]}
 * (anchor: the warped corner being dragged, kept where it is by the
 * straighten) resolved to {"vertices":[{"from":[x,y],"to":[x,y]}],
 * "quads":[[a,b,c,d]],"shared_edges":[[a,b]],"straighten":[{"quad","edge",
 * "direction":"horizontal"|"vertical"|degrees}],"identity":bool}.
 * dr_perspective_warp_preview: the same fields over the layer's canvas rect
 * sx,sy,sw,sh, which proxy pictures at any scale and which is the
 * extension's subject; optionally times coverage (proxy-sized bytes);
 * out_rect (4 doubles) receives the canvas rect x,y,w,h the image covers.
 * Free with dr_image_free.
 * dr_*_perspective_warp_plan: {"retained_bytes","scratch_bytes",
 * "rect":[x,y,w,h],"vertices"}; an identity layout (every to == from after
 * straightening) answers scratch_bytes 0.
 * dr_*_perspective_warp: the new document with the layer replaced by its
 * warp (not clipped to the canvas; a linked mask warps along, an unlinked
 * one stays), or NULL with no error for an identity layout. control is
 * single-use and may be NULL.
 * Every call returns NULL with no error for a NULL handle or fields, and
 * NULL with a message (freed with dr_string_free) for a refusal. */
char *dr_perspective_warp_layout(const char *fields, char **err_out);
DrImage *dr_perspective_warp_preview(const char *fields, const DrImage *proxy, int64_t sx,
    int64_t sy, uint32_t sw, uint32_t sh, const uint8_t *coverage, size_t coverage_len,
    double *out_rect, char **err_out);
char *dr_doc_perspective_warp_plan(const DrDocument *document, const char *request,
    uint64_t limit, uint64_t other, char **err_out);
DrDocument *dr_doc_perspective_warp(const DrDocument *document, const char *request,
    uint64_t limit, uint64_t other, const DrJobControl *control, char **err_out);
char *dr_precision_doc_perspective_warp_plan(const DrPrecisionDocument *document,
    const char *request, uint64_t limit, uint64_t other, char **err_out);
DrPrecisionDocument *dr_precision_doc_perspective_warp(const DrPrecisionDocument *document,
    const char *request, uint64_t limit, uint64_t other, const DrJobControl *control,
    char **err_out);

/* ---- Auto-Align Layers: Cylindrical, Spherical and plane -------------------
 * One layer, a pinhole view, reprojected onto the reference camera's
 * cylinder or sphere (align_projection.rs). The request (strict, at most
 * 64 KiB, canvas pixels, pixel centres at half-integers):
 * {"op":"project_layer","layer":N,"projection":"cylindrical"|"spherical",
 *  "focal":f,"rotation":[9],"centre":[x,y],"origin":[x,y],"distortion":k}.
 * PLANE (Collage and Reposition with Geometric Distortion):
 * {"op":"project_layer","layer":N,"projection":"plane",
 *  "map":[9],"distortion":k}; map (row-major, finite, determinant above 0)
 * takes the layer's CORRECTED canvas point u to the destination: a
 * destination point p reads u = map^-1 * p (w above 0), which must lie in
 * the clip rect below, then the lens's point q (distortion as below), which
 * must too. Each projection has its own keys: a plane request with
 * focal/rotation/centre/origin, or a surface request with map, is refused
 * "invalid project layer request: unknown field `<key>`". Refusals: "project
 * layer map must be invertible with a positive determinant", "invalid
 * project layer request: map must be 9 finite numbers", "invalid project
 * layer request: projection must be cylindrical, spherical or plane".
 * distortion is optional, 0 when absent (bit-identical to
 * the request without it): Geometric Distortion's coefficient; the pinhole
 * point q_u reads the layer at c + R*q*(1 + k*|q|^2), q = (q_u - c)/R, c the
 * layer rect's centre and R half its longer side; a q_u past the first fold
 * is transparent. Refusals: "invalid project layer request: distortion must
 * be a finite number", "invalid project layer request: distortion folds the
 * layer" (the radial map may fold within the rect's corner, or a corner has
 * no corrected point).
 * rotation (row-major) maps a ray of the REFERENCE camera to the same ray of
 * this layer's camera; centre is the layer's principal point; origin is
 * where the reference's optical axis lands. A destination point p has
 * theta = (p.x - origin.x)/f and, Cylindrical, h = (p.y - origin.y)/f, the
 * ray d = (sin theta, h, cos theta); Spherical, phi = (p.y - origin.y)/f,
 * d = (sin theta cos phi, sin phi, cos theta cos phi). Its source point is
 * centre + f*(r.x, r.y)/r.z with r = R*d; a point past the surface
 * (|theta| > pi, Spherical |phi| > pi/2), behind the layer's camera or
 * outside the layer's PIXEL-CENTRE rect [x0+1/2, x0+w-1/2] x
 * [y0+1/2, y0+h-1/2] (closed; the index-space clip, phase 2b-5) is
 * transparent (a linked mask's outside coverage). The layer is resampled
 * (Catmull-Rom) onto the pixels whose centres the forward image of that
 * rect reaches: per axis, with lo and hi the image's extremes, x from
 * ceil(lo - 1/2 - 0.01) to floor(hi - 1/2 + 0.01) + 1 (exclusive), at least
 * one pixel; not clipped to the canvas; a linked mask
 * reprojects along, an unlinked one stays; the persisted warp binding is
 * dropped. Refusals, verbatim: "project layer focal must be a finite number
 * above 0", "project layer rotation must be an orthonormal 3x3 matrix with
 * determinant 1" (|RR^T - I| above 1e-6 anywhere, or det <= 0),
 * "Auto-Align Layers cannot project layer N: it reaches round past the far
 * side of the projection.", "Auto-Align Layers would make the layer larger
 * than the pixel limit.", a parse error "invalid project layer request:
 * ...", a group or adjustment layer, an artwork or retained-source layer
 * ("Rasterize layer N before Auto-Align Layers") and the Pixels/Position
 * locks.
 * dr_*_project_layer_plan: {"retained_bytes","scratch_bytes",
 * "rect":[x,y,w,h],"vertices":0,"output_bytes"}, rect the projected layer's,
 * output_bytes the part of retained_bytes that is the new layer and its
 * linked mask (what the result holds beyond the input document).
 * dr_*_project_layer: the new document; never NULL without a message for a
 * live document. control is single-use and may be NULL.
 * Every call returns NULL with no error for a NULL document, and NULL with a
 * message (freed with dr_string_free) for a refusal. */
char *dr_doc_project_layer_plan(const DrDocument *document, const char *request,
    uint64_t limit, uint64_t other, char **err_out);
DrDocument *dr_doc_project_layer(const DrDocument *document, const char *request,
    uint64_t limit, uint64_t other, const DrJobControl *control, char **err_out);
char *dr_precision_doc_project_layer_plan(const DrPrecisionDocument *document,
    const char *request, uint64_t limit, uint64_t other, char **err_out);
DrPrecisionDocument *dr_precision_doc_project_layer(const DrPrecisionDocument *document,
    const char *request, uint64_t limit, uint64_t other, const DrJobControl *control,
    char **err_out);

/* ---- Auto-Align Layers: Vignette Removal -----------------------------------
 * One radial falloff fitted to the registered overlaps of a stack and divided
 * out of every listed layer (align_vignette.rs). The request (strict, at most
 * 64 KiB): {"op":"remove_vignette","layers":[{"layer":N,"map":[9]},...]},
 * at least 2 distinct layers; map is row-major, finite, with a positive
 * determinant, taking the layer's canvas point to the reference's; the FIRST
 * entry's exposure is the one the fit pins (send the reference first, with
 * the identity). The model: V(rho) = 1 + a*rho^2, rho a pixel centre's
 * distance from its layer rect's centre over the distance to the farthest
 * pixel centre, one a for the stack (a in [-15/16, 1]), fitted in linear light
 * (8-bit sRGB-decoded, 16-bit through the image profile, float as stored) on
 * the nearest-pixel ratios of every overlapping pair whose mean linear
 * channel is in (0.01, 0.9) in both, a per-layer exposure solved and not
 * applied. Each pixel with any alpha is multiplied by gain / V(rho), gain the
 * mean of V over every pixel centre of every listed layer: at 8 bits in sRGB
 * linear light, clamped; at 16 bits in the image profile's linear light,
 * clamped; at float RGB * 2^stops, unclamped. Alpha, masks and offsets are
 * unchanged. Refusals, verbatim: "invalid remove vignette request: ...",
 * "Vignette Removal found too little overlap between the layers to measure
 * the vignette. Nothing was changed." (fewer than 1,000 ratios), "Vignette
 * Removal cannot change layer N: <reason>." (no such layer, a group or
 * adjustment layer, artwork or a placed source, the Pixels lock), "Vignette
 * Removal does not work in CMYK.".
 * dr_*_remove_vignette_plan: {"retained_bytes","scratch_bytes",
 * "output_bytes","coefficient","gain","samples","layers":[N...]}; it runs the
 * fit. output_bytes is every listed layer at its depth's bytes a pixel.
 * dr_*_remove_vignette: the new document; NULL with no message when no
 * sample changed (never an identical copy). control is single-use and may
 * be NULL.
 * Every call returns NULL with no error for a NULL document, and NULL with a
 * message (freed with dr_string_free) for a refusal. */
char *dr_doc_remove_vignette_plan(const DrDocument *document, const char *request,
    uint64_t limit, uint64_t other, char **err_out);
DrDocument *dr_doc_remove_vignette(const DrDocument *document, const char *request,
    uint64_t limit, uint64_t other, const DrJobControl *control, char **err_out);
char *dr_precision_doc_remove_vignette_plan(const DrPrecisionDocument *document,
    const char *request, uint64_t limit, uint64_t other, char **err_out);
DrPrecisionDocument *dr_precision_doc_remove_vignette(const DrPrecisionDocument *document,
    const char *request, uint64_t limit, uint64_t other, const DrJobControl *control,
    char **err_out);

/* ---- Auto-Align Layers: Geometric Distortion -------------------------------
 * One radial lens coefficient k fitted from patch matches in the registered
 * overlaps of a stack (align_distortion.rs); the document is never changed.
 * The request (strict, at most 64 KiB):
 * {"op":"fit_distortion","layers":[{"layer":N,"anchor":M,"map":[9]},...]},
 * at least 2 distinct layers; the FIRST entry is the reference, its own
 * anchor with the identity map; every later anchor is an earlier entry's
 * layer; map is row-major, finite, with a positive determinant, taking the
 * layer's canvas point to the reference's. The model: a layer pixel p is the
 * source of the corrected point u, p = c + R*q*(1 + k*|q|^2), q = (u - c)/R,
 * c the layer rect's centre and R half its longer side; one k for the stack,
 * k in [-0.25, 0.25]. Each layer is matched to its anchor by normalised
 * cross-correlation of linear luminance patches (8-bit sRGB-decoded, 16-bit
 * through the image profile, float as stored); per pair a homography between
 * the corrected points is fitted, and k minimises the summed squared
 * transfer error. Refusals, verbatim: "invalid fit distortion request: ...",
 * "Geometric Distortion found too little overlap between the layers to
 * measure the distortion. Nothing was changed." (a pair under 24 matches),
 * "Geometric Distortion cannot change layer N: <reason>." (no such layer, a
 * group or adjustment layer, artwork or a placed source, the Pixels lock),
 * "Geometric Distortion does not work in CMYK.".
 * dr_*_fit_distortion_plan: {"retained_bytes","scratch_bytes"}; it runs no
 * matching.
 * dr_*_fit_distortion: {"coefficient","matches","residual","layers":[N...],
 * "maps":[[9]...]}, the maps in request order, each a layer's CORRECTED
 * canvas point to the reference's corrected point (m[8] = 1); residual is
 * the RMS transfer error in pixels. control is single-use and may be NULL.
 * Every call returns NULL with no error for a NULL document, and NULL with a
 * message (freed with dr_string_free) for a refusal. */
char *dr_doc_fit_distortion_plan(const DrDocument *document, const char *request,
    uint64_t limit, uint64_t other, char **err_out);
char *dr_doc_fit_distortion(const DrDocument *document, const char *request,
    uint64_t limit, uint64_t other, const DrJobControl *control, char **err_out);
char *dr_precision_doc_fit_distortion_plan(const DrPrecisionDocument *document,
    const char *request, uint64_t limit, uint64_t other, char **err_out);
char *dr_precision_doc_fit_distortion(const DrPrecisionDocument *document,
    const char *request, uint64_t limit, uint64_t other, const DrJobControl *control,
    char **err_out);

/* ---- Auto-Align Layers: feature registration -------------------------------
 * Feature registration (feature_points.rs / feature_register.rs).
 * floating and reference are width x height grey bytes, row-major, unpadded;
 * 16 <= width, height <= 4096. On true map_out[9] is the row-major homography
 * (m[8] = 1) taking a floating point to the reference, continuous pixels, pixel
 * (i, j)'s centre at (i + 0.5, j + 0.5), y down; inliers_out the number of its
 * RANSAC inliers, at most 2000 (the kernel's feature cap; not written when
 * NULL); pairs_out, when not NULL, the first min(inliers, pairs_capacity)
 * inliers, four doubles each: floating x, y, then reference x, y, same
 * convention; departure_out, when not NULL, the lens test's departure on
 * those inliers: the worst inlier's distance, pixels, between the plain homography's
 * prediction and Geometric Distortion's model's (one radial coefficient, each
 * frame's lens at its centre, R = max(width, height) / 2), NaN when no lens
 * model fits. false with no error: a NULL input (floating, reference or map_out) or no estimate. false
 * with a message: a size outside the range. Deterministic (fixed seed). No job
 * control: bounded by the size cap and the kernel's caps. */
bool dr_feature_register(const uint8_t *floating, const uint8_t *reference,
    uint32_t width, uint32_t height, double *map_out, double *pairs_out,
    uint32_t pairs_capacity, uint32_t *inliers_out, double *departure_out,
    char **err_out);
/* The kernel's peak working bytes for one call at that size; 0 outside the range. */
uint64_t dr_feature_register_scratch_bytes(uint32_t width, uint32_t height);

/* ---- Sky Replacement ----------------------------------------------------
 * Edit > Sky Replacement's "Duplicate Layer" and "New Layers" outputs
 * (sky_harmonize.rs, sky_group.rs):
 * the sky model's u16 coverage refined by Shift/Fade Edge, the sky (RGBA8,
 * alpha ignored) adjusted and placed over the canvas, the foreground
 * harmonized toward it, composited once. params_json is strict, <=4 KiB:
 * {"shift_edge":-100..100 (0),"fade_edge":0..100 (3),"brightness":-100..100
 * (0),"temperature":-100..100 (0),"scale":25..400 (100),"flip":bool (false),
 * "offset":[fx,fy] each -1..1 ([0,0]),"lighting_mode":"multiply"|"screen"
 * ("multiply"),"lighting":0..100 (50),"color_adjustment":0..100 (50),
 * "edge_lighting":0..100 (0),"output":"duplicate_layer"|"new_layers"
 * ("duplicate_layer"),"name":string <=255 bytes ("Sky Replacement" for
 * duplicate_layer, "Sky Replacement Group" for new_layers),"decontaminate":
 * bool (false)}; an unknown key or out-of-range value is an error naming the
 * key. decontaminate solves the old sky's colour out of the horizon's
 * edge pixels before compositing, in both outputs and the preview.
 * dr_sky_replace_scratch_bytes: the plan. output 0 Duplicate Layer: bits
 * 8|16|32 (4|8|16 B/px written), decontaminate ignored; output 1 New Layers:
 * bits 8|16 (7|14 B/px written, 12|24 with decontaminate); 0 for a zero
 * size, an unknown output or other bits.
 * dr_image_sky_replace: the dialog's preview, any foreground size; the
 * coverage (coverage_width x coverage_height) is read bilinearly and the
 * edge widths come from the coverage size; output is accepted and ignored
 * (both outputs composite to this picture). Free with dr_image_free.
 * dr_doc_sky_replace / dr_precision_doc_sky_replace: canvas-sized coverage
 * (count == w*h); ONE canvas-sized raster layer at (0,0), Normal, opacity 1,
 * named params.name, directly above layer. "new_layers" instead adds a Pass
 * Through group named params.name above layer's clip run (never clipped),
 * holding bottom-first, with decontaminate only, "Foreground Edge" (canvas
 * raster at (0,0), Normal, opacity 1, the decontaminated edge colour, masked
 * to the edge band), then "Foreground Color" (Curves, Normal, opacity
 * color_adjustment/100, white mask), "Foreground Lighting" (Curves,
 * Multiply|Screen per lighting_mode, opacity lighting/100, white mask) and
 * "Sky" (canvas raster at (0,0), Normal, opacity 1, masked); it composites
 * as the Duplicate Layer output does. A source nested too deep refuses
 * "sky replacement cannot nest its group that deep". A group or adjustment
 * layer, a CMYK, a 32-bit or (#1 Lab phase 6b-iii) a Lab Color document
 * refuses by name; a picture with no sky weight refuses "sky replacement
 * found no sky on layer N". control is single-use and may be NULL. NULL always carries a message (dr_string_free), except a
 * NULL byte doc, which returns NULL with err_out untouched. */
uint64_t dr_sky_replace_scratch_bytes(uint32_t width, uint32_t height, uint32_t sky_width,
    uint32_t sky_height, uint32_t bits, uint32_t output, bool decontaminate);
DrImage *dr_image_sky_replace(const DrImage *foreground, const DrImage *sky,
    const uint16_t *coverage, size_t coverage_count, uint32_t coverage_width,
    uint32_t coverage_height, const char *params_json, uint64_t limit, uint64_t other,
    const DrJobControl *control, char **err_out);
DrDocument *dr_doc_sky_replace(const DrDocument *doc, size_t layer, const DrImage *sky,
    const uint16_t *coverage, size_t coverage_count, const char *params_json, uint64_t limit,
    uint64_t other, const DrJobControl *control, char **err_out);
DrPrecisionDocument *dr_precision_doc_sky_replace(const DrPrecisionDocument *doc, size_t layer,
    const DrImage *sky, const uint16_t *coverage, size_t coverage_count,
    const char *params_json, uint64_t limit, uint64_t other, const DrJobControl *control,
    char **err_out);

/* ---- Remove tool, model engine ------------------------------------------
 * The Remove tool's Model mode (remove_model.rs, doc_remove_model.rs): a local
 * inpainting model runs on a square crop around the stroke at its native
 * side (64..1024; MI-GAN's is 512) and the core composites its fill back at 8
 * and 16 bits. The host calls plan, then input, runs the model, then apply,
 * in one job. sample is "current"|"current_and_below"|"all"; coverage is
 * canvas-sized u16 (count == w*h). Planes are channel-major f32 in 0..1:
 * image 3*side^2 (RGB), mask side^2 (1 = remove), the model's plane 3*side^2.
 * The crop: the covered rect's longer side L, s = max(side, ceil(2L)),
 * clipped to the canvas, centred on the rect and slid inside the canvas.
 * dr_*_remove_model_plan: the job's working set (the core's own figure plus
 * the sampling plate's); 0 with err_out on refusal.
 * dr_*_remove_model_input: writes image_out (the plate sample names, the
 * exact area-weighted mean per native cell, adjustment layers left out when
 * ignore_adjustments), mask_out and crop_out [x, y, w, h].
 * dr_*_remove_model_apply: the plane composited by the coverage over layer,
 * inside its extent on the canvas, or with new_layer onto a canvas-sized
 * raster "Remove" directly above it; NULL with err_out untouched when
 * nothing changed. A NULL doc carries "required handle is NULL" at every
 * export. A Lab Color document (#1 Lab 2f) feeds the model its Display P3
 * view (lab_convert::view_pipeline, clamped, alpha kept) and converts the
 * plane back to Lab codes (from_view_pipeline, clamped) before the coverage
 * mix; its plans add the pipeline's tables.
 * Refusals (lowercase; the plan refuses the first four): "remove layer N is
 * out of range"; "remove needs a raster layer; layer N is not one"; "remove's
 * model is not available in a 32-bit float document"; "remove's model is not
 * available in a CMYK document" (the take in remove_tool_take.py); a buffer of the
 * wrong size; "the stroke covers no pixel of the picture"; input only: "the
 * hole covers the whole crop: there is no picture around it to fill from"
 * (no native cell left unmasked, e.g. a Select All hole), then "there is
 * nothing to sample from: every pixel around the stroke is transparent.";
 * apply only: "remove model output is not finite"; the working
 * limit (limit beside other); "operation cancelled". control is single-use
 * and may be NULL. Free messages with dr_string_free. */
uint64_t dr_doc_remove_model_plan(const DrDocument *doc, size_t layer, const char *sample,
    uint32_t side, bool new_layer, char **err_out);
uint64_t dr_precision_doc_remove_model_plan(const DrPrecisionDocument *doc, size_t layer,
    const char *sample, uint32_t side, bool new_layer, char **err_out);
bool dr_doc_remove_model_input(const DrDocument *doc, size_t layer, const char *sample,
    bool ignore_adjustments, const uint16_t *coverage, size_t coverage_count, uint32_t side,
    uint32_t *crop_out, float *image_out, size_t image_count, float *mask_out,
    size_t mask_count, uint64_t limit, uint64_t other, const DrJobControl *control,
    char **err_out);
bool dr_precision_doc_remove_model_input(const DrPrecisionDocument *doc, size_t layer,
    const char *sample, bool ignore_adjustments, const uint16_t *coverage,
    size_t coverage_count, uint32_t side, uint32_t *crop_out, float *image_out,
    size_t image_count, float *mask_out, size_t mask_count, uint64_t limit, uint64_t other,
    const DrJobControl *control, char **err_out);
DrDocument *dr_doc_remove_model_apply(const DrDocument *doc, size_t layer,
    const uint16_t *coverage, size_t coverage_count, uint32_t side, const float *plane,
    size_t plane_count, bool new_layer, uint64_t limit, uint64_t other,
    const DrJobControl *control, char **err_out);
DrPrecisionDocument *dr_precision_doc_remove_model_apply(const DrPrecisionDocument *doc,
    size_t layer, const uint16_t *coverage, size_t coverage_count, uint32_t side,
    const float *plane, size_t plane_count, bool new_layer, uint64_t limit, uint64_t other,
    const DrJobControl *control, char **err_out);

/* ---- Generative Fill ----------------------------------------------------
 * Generative Fill through a cloud image-edit provider (gen_fill.rs,
 * doc_gen_fill.rs, cmyk_gen_fill.rs): the host sends the crop around the
 * selection at one of the provider's native sizes, with the user's own key,
 * and the core composites the answer at 8 and 16 bits and on 8-bit CMYK.
 * The host calls frame, plan, then input, sends the request, then apply, in
 * one job. coverage is canvas-sized u16 (count == w*h). Pictures are
 * straight RGBA8, row 0 = top, in the document's encoding on RGB (no colour
 * conversion either way); on 8-bit CMYK they are sRGB (the plate's ink to
 * the built-in sRGB, the answer back to ink, relative colorimetric with
 * black point compensation).
 * dr_gen_fill_frame: sizes is size_count u32s, an even number of (w, h)
 * pairs, each side 64..2048; the native size is the one whose aspect is
 * nearest the covered rect's (least |ln ratio|, the first wins a tie), and
 * the crop k = max(1, 2w/nw, 2h/nh), k' = max(w/nw, h/nh, min(k, W/nw,
 * H/nh)), cw = min(W, ceil(k'*nw)), ch = min(H, ceil(k'*nh)), centred on the
 * covered rect and slid inside the canvas: it always contains the rect and
 * keeps the native aspect unless the rect alone forces a clip.
 * frame_out = [crop x, y, w, h, native w, native h].
 * dr_*_gen_fill_plan: the job's working set for variations stored pictures
 * (the core's own figure plus the full composite plate's); 0 with err_out
 * on refusal.
 * dr_*_gen_fill_input: layer is only the anchor (any kind). Writes image_out
 * (4*nw*nh bytes: the full composite's exact area-weighted mean per native
 * cell, alpha 0 on a hole cell, 255 elsewhere), mask_out (4*nw*nh bytes:
 * (255,255,255,0) where a cell's footprint touches a covered pixel, else
 * (0,0,0,255)) and crop_out [x, y, w, h].
 * dr_*_gen_fill_apply: results holds k pictures back to back
 * (results_count == sum of 4*w*h), result_sizes 2k u32s of (w, h), each
 * side 1..4096, 1 <= k <= 4. The FIRST is read back bilinearly over the crop
 * onto a NEW canvas-sized raster directly above layer: opaque on the covered
 * rect, transparent elsewhere, masked by the coverage (8 bits: rounded to
 * bytes; 16 bits and CMYK: the u16 coverage itself), named name (not empty)
 * and tagged with tag (an artwork description of type "generated": the byte
 * meta at 8 bits, the typed description at 16); every picture is kept as
 * the layer's VARIATIONS (stored as PNG, the first selected, persisted in
 * .rz: word 92, CMYK too); *layer_out (may be NULL) takes its index. The
 * composite outside the selection is unchanged. On a Grayscale document
 * (#1 phase 6b) every rendered picture (apply and select_variation) is
 * folded to its gray by Mode > Grayscale's rule, R = G = B, and the plan adds
 * 8 bytes a canvas pixel for it. On a Lab Color document (#1 Lab phase
 * 6b-iii) image_out is the plate's Display P3 view (relative colorimetric,
 * no BPC), and every rendered picture is read as Display P3 and returned to
 * L*a*b* codes by Image > Mode > Lab Color's rule; the plan adds the view's
 * tables, 8 bytes a canvas pixel and the way back's tables. A NULL doc carries
 * "required handle is NULL" at every export.
 * Refusals (lowercase): "generative fill layer N is out of range";
 * "generative fill is not available in a 32-bit float document";
 * "generative fill is not available in a 16-bit CMYK document" (the
 * take in generative_fill_take.py);
 * "generative fill native size must be 64…2048 on each side"; a buffer of
 * the wrong size; "the selection covers no pixel of the picture"; apply
 * only: "generative fill takes 1 to 4 results", "generative fill result
 * must be 1…4096 on each side", "generative fill needs a layer name",
 * "generative fill requires a generated artwork description", "a generative
 * variation is over 80 MiB"; the working limit (limit beside other);
 * "operation cancelled". control is single-use and may be NULL. Free
 * messages with dr_string_free. */
bool dr_gen_fill_frame(const uint16_t *coverage, size_t coverage_count, uint32_t width,
    uint32_t height, const uint32_t *sizes, size_t size_count, uint32_t *frame_out,
    char **err_out);
uint64_t dr_doc_gen_fill_plan(const DrDocument *doc, uint32_t native_w, uint32_t native_h,
    uint32_t variations, char **err_out);
uint64_t dr_precision_doc_gen_fill_plan(const DrPrecisionDocument *doc, uint32_t native_w,
    uint32_t native_h, uint32_t variations, char **err_out);
bool dr_doc_gen_fill_input(const DrDocument *doc, size_t layer, const uint16_t *coverage,
    size_t coverage_count, uint32_t native_w, uint32_t native_h, uint32_t *crop_out,
    uint8_t *image_out, size_t image_count, uint8_t *mask_out, size_t mask_count,
    uint64_t limit, uint64_t other, const DrJobControl *control, char **err_out);
bool dr_precision_doc_gen_fill_input(const DrPrecisionDocument *doc, size_t layer,
    const uint16_t *coverage, size_t coverage_count, uint32_t native_w, uint32_t native_h,
    uint32_t *crop_out, uint8_t *image_out, size_t image_count, uint8_t *mask_out,
    size_t mask_count, uint64_t limit, uint64_t other, const DrJobControl *control,
    char **err_out);
DrDocument *dr_doc_gen_fill_apply(const DrDocument *doc, size_t layer, const uint16_t *coverage,
    size_t coverage_count, uint32_t native_w, uint32_t native_h, const uint8_t *results,
    size_t results_count, const uint32_t *result_sizes, size_t size_count, const char *name,
    const char *tag, size_t *layer_out, uint64_t limit, uint64_t other,
    const DrJobControl *control, char **err_out);
DrPrecisionDocument *dr_precision_doc_gen_fill_apply(const DrPrecisionDocument *doc,
    size_t layer, const uint16_t *coverage, size_t coverage_count, uint32_t native_w,
    uint32_t native_h, const uint8_t *results, size_t results_count,
    const uint32_t *result_sizes, size_t size_count, const char *name, const char *tag,
    size_t *layer_out, uint64_t limit, uint64_t other, const DrJobControl *control,
    char **err_out);

/* ---- Generative Layer variations -----------------------------------------
 * The pictures a Generative Layer keeps (gen_variations.rs,
 * doc_gen_variations.rs, generated_native.rs). A layer holds LIVE variations
 * while its tag is a generated one, its pixels are the raster the core last
 * rendered from them at that offset, the document profile is the one they
 * were rendered in, it has a canvas-sized mask at (0, 0) and the colour
 * model matches; any paint, filter, move, transform, Convert/Assign Profile
 * or mask deletion detaches them for good (a mask edit keeps them). At most
 * 24 pictures a layer, none over 80 MiB, 256 MiB a layer, 1 GiB a document.
 * dr_*_gen_fill_variations: info_out (8 u32s) = [count, selected, native w,
 * native h, crop x, y, w, h]; false with no err for a layer holding no live
 * variations.
 * dr_*_gen_fill_select_variation: re-renders the layer from variation index
 * (transparent outside the covered rect, the picture read back inside it,
 * the mask kept) under its Pixels lock and swaps the tag for that picture's;
 * NULL with no err when index is already selected.
 * dr_*_gen_fill_more_input: the provider's two pictures for a new set
 * (image_count == mask_count == 4*nw*nh at the stored native size): the
 * input over the stored crop, the coverage the layer's own mask, the plate
 * with the layer hidden.
 * dr_*_gen_fill_add_variations: results / result_sizes as apply's; appends
 * them tagged tag, selects the first new one and re-renders.
 * Refusals (lowercase): "generative fill layer N is out of range";
 * "generative layer N holds no variations"; "generative variation K is out
 * of range (the layer holds N)"; "a generative layer keeps at most 24
 * variations"; "a generative variation is over 80 MiB"; "a generative layer
 * keeps at most 256 MiB of variations"; a lock ("generative layer N is
 * locked" at 8 bits, the typed lock sentence at 16); the depth refusals
 * above; the working limit; "operation cancelled". */
bool dr_doc_gen_fill_variations(const DrDocument *doc, size_t layer, uint32_t *info_out,
    char **err_out);
bool dr_precision_doc_gen_fill_variations(const DrPrecisionDocument *doc, size_t layer,
    uint32_t *info_out, char **err_out);
DrDocument *dr_doc_gen_fill_select_variation(const DrDocument *doc, size_t layer,
    uint32_t index, uint64_t limit, uint64_t other, const DrJobControl *control,
    char **err_out);
DrPrecisionDocument *dr_precision_doc_gen_fill_select_variation(const DrPrecisionDocument *doc,
    size_t layer, uint32_t index, uint64_t limit, uint64_t other, const DrJobControl *control,
    char **err_out);
bool dr_doc_gen_fill_more_input(const DrDocument *doc, size_t layer, uint8_t *image_out,
    size_t image_count, uint8_t *mask_out, size_t mask_count, uint64_t limit, uint64_t other,
    const DrJobControl *control, char **err_out);
bool dr_precision_doc_gen_fill_more_input(const DrPrecisionDocument *doc, size_t layer,
    uint8_t *image_out, size_t image_count, uint8_t *mask_out, size_t mask_count,
    uint64_t limit, uint64_t other, const DrJobControl *control, char **err_out);
DrDocument *dr_doc_gen_fill_add_variations(const DrDocument *doc, size_t layer,
    const uint8_t *results, size_t results_count, const uint32_t *result_sizes,
    size_t size_count, const char *tag, uint64_t limit, uint64_t other,
    const DrJobControl *control, char **err_out);
DrPrecisionDocument *dr_precision_doc_gen_fill_add_variations(const DrPrecisionDocument *doc,
    size_t layer, const uint8_t *results, size_t results_count, const uint32_t *result_sizes,
    size_t size_count, const char *tag, uint64_t limit, uint64_t other,
    const DrJobControl *control, char **err_out);

/* ---- Generative Expand --------------------------------------------------
 * Generative Expand fills the area the picture leaves transparent (what
 * Canvas Size adds) through Generative Fill's frame, input and apply
 * (doc_gen_expand.rs). dr_*_gen_expand_coverage writes coverage_out
 * (canvas-sized u16, coverage_count == w*h): 65535 - the full composite's
 * alpha (u16; a byte alpha * 257; a float alpha clamped to 0..1, then
 * * 65535 rounded), so an opaque pixel is 0 and stays bit-exact under the new
 * layer. The host passes it to dr_gen_fill_frame, dr_*_gen_fill_input and
 * dr_*_gen_fill_apply as the coverage, and Auto-Blend's Content-Aware Fill
 * Transparent Areas to the Content-Aware Fill; a 32-bit
 * float document is accepted here (its Generative Fill stays refused).
 * layer is only the anchor (range checked). The plate is admitted against
 * limit beside other. Refusals (lowercase): "generative expand layer N is out
 * of range"; "invalid generative expand coverage buffer"; "generative expand
 * is not available in a 16-bit CMYK document"; "the picture has no transparent area
 * to expand into: enlarge the canvas first"; the working limit; "operation
 * cancelled". A NULL doc carries "required handle is NULL". control is
 * single-use and may be NULL. Free messages with dr_string_free. */
bool dr_doc_gen_expand_coverage(const DrDocument *doc, size_t layer, uint16_t *coverage_out,
    size_t coverage_count, uint64_t limit, uint64_t other, const DrJobControl *control,
    char **err_out);
bool dr_precision_doc_gen_expand_coverage(const DrPrecisionDocument *doc, size_t layer,
    uint16_t *coverage_out, size_t coverage_count, uint64_t limit, uint64_t other,
    const DrJobControl *control, char **err_out);

/* ---- Neural filters -----------------------------------------------------
 * Filter > Neural Filters > Super Zoom and JPEG Artifacts Removal
 * (neural_tiling.rs, neural_stage.rs, doc_neural.rs): an image model runs on
 * tile x tile squares (a core of tile - 2*pad plus pad of context, cores
 * row-major from (0, 0), reads clamped to the image) and the core stitches
 * the cores. stage_json: {"tile" 16..1024, "pad" (multiple of 8), "scale"
 * 1|2|4, "model_scale" 1|4 (scale divides it), "model": bool,
 * "noise_reduction" 0..100, "sharpen" 0..100}. The model's input is
 * channel-major straight RGB 0..1 (input_len = 3*tile^2), its output
 * 3*(tile*model_scale)^2; each output pixel's RGB is the mean of the model
 * cells it covers, its alpha the source's Catmull-Rom sample (also RGB with
 * "model": false). Noise Reduction (Reduce Noise at radius 2) runs on the
 * source first, Sharpen (Unsharp Mask radius 1, amount v/50) on the output.
 * predict is called once per tile, in index order, on the calling thread;
 * false (or a non-finite plane) refuses "the model failed on tile N".
 * dr_neural_stage_info answers {"tiles","output_width","output_height",
 * "input_len","plane_len","planned_bytes"} at bytes_per_sample 1 or 2; an
 * output past 100 MP refuses "the zoomed image would be W x H (N.N
 * megapixels), past the 100-megapixel limit: choose a smaller zoom".
 * dr_*_neural_stage: NULL with err_out untouched when the stage changes
 * nothing; "model": true with a NULL predict errors; the precision twin is
 * RGBA16 only ("neural filters are not available in a 32-bit document").
 * dr_*_super_zoom_document: a new document, one raster layer "Background",
 * in the source's profile at its resolution. dr_precision_doc_neural_layer_
 * apply: pixels (the layer's own size) replace the layer in place (its
 * description dropped), or with new_layer a new raster layer name directly
 * above it at the same offset; NULL with err_out untouched when nothing
 * changed (a pixel-locked layer, identical pixels). Float and CMYK documents
 * refuse "neural filters are not available in a 32-bit document" / "... in a
 * CMYK document". On an 8- or 16-bit Grayscale document (#1 phase 6b) the
 * result is folded to its gray by Mode > Grayscale's rule (R = G = B), and
 * Super Zoom's document is Grayscale in the source's Gray profile. On a Lab
 * Color document (#1 Lab phase 6b-iii) the model read the Display P3 view
 * (dr_precision_image_lab_display), so zoomed / pixels must be tagged the
 * built-in Display P3 (else "a Lab Color document takes a model's result in
 * its Display P3 view"); the result returns to L*a*b* codes by Mode > Lab
 * Color's rule, and Super Zoom's document is Lab at the source's depth. The
 * working limit (limit beside other_retained_bytes) and
 * "operation cancelled" as elsewhere; control is single-use and may be NULL.
 * Free strings with dr_string_free. */
typedef bool (*DrNeuralPredict)(void *context, uint64_t tile, const float *input, size_t input_len,
                                float *output, size_t output_len);
char *dr_neural_stage_info(uint32_t width, uint32_t height, const char *stage_json,
                           uint32_t bytes_per_sample, char **err_out);
DrImage *dr_image_neural_stage(const DrImage *img, const char *stage_json, DrNeuralPredict predict,
    void *context, uint64_t limit, uint64_t other_retained_bytes, const DrJobControl *control,
    char **err_out);
DrPrecisionImage *dr_precision_image_neural_stage(const DrPrecisionImage *img,
    const char *stage_json, DrNeuralPredict predict, void *context, uint64_t limit,
    uint64_t other_retained_bytes, const DrJobControl *control, char **err_out);
DrDocument *dr_doc_super_zoom_document(const DrDocument *source, const DrImage *zoomed,
    char **err_out);
DrPrecisionDocument *dr_precision_doc_super_zoom_document(const DrPrecisionDocument *source,
    const DrPrecisionImage *zoomed, uint64_t limit, uint64_t other_retained_bytes,
    const DrJobControl *control, char **err_out);
DrPrecisionDocument *dr_precision_doc_neural_layer_apply(const DrPrecisionDocument *doc,
    size_t layer, const DrPrecisionImage *pixels, bool new_layer, const char *name,
    uint64_t limit, uint64_t other_retained_bytes, const DrJobControl *control, char **err_out);

/* Colorize (neural_colorize.rs, neural_colorize_run.rs): the host's model
 * (DDColor) runs once at side x side (16..1024; the app uses 512) between
 * two calls. dr_*_neural_colorize_input writes image_out (image_count must be
 * 3*side^2): channel-major sRGB gray of Lab(L*, 0, 0), L* the layer's own in
 * the document profile (profile bytes), bilinear-resized. The model answers
 * 2*side^2 channel-major Lab a*, b*; dr_*_neural_colorize_apply
 * (chroma_count must be 2*side^2) adjusts them with adjust_json
 * {"saturation","cyan_red","magenta_green","yellow_blue"} -50..50 and
 * "color_artifact_reduction" 0..100 (each optional, 0), samples them
 * bilinearly onto every pixel's own L*, scales chroma back into the profile's
 * gamut and keeps alpha exactly; NULL with err_out untouched when the output
 * equals the input. Refusals: "colorize needs an RGB matrix/TRC document
 * profile; convert or assign a supported profile first", "the colorize
 * input must hold 3 x side^2 samples", "the colorize chroma must hold 2 x
 * side^2 samples", "the colorize model's output is not finite", "invalid
 * colorize adjustments: ...", "colorize <key> must be -50 to 50" / "... 0 to
 * 100", the float sentence above, the working limit and "operation
 * cancelled". dr_neural_colorize_plan: 0 with err_out on refusal ("colorize
 * side must be 16 to 1024", bytes_per_sample not 1 or 2, an empty image).
 * Colorize's New Document output reuses dr_*_super_zoom_document. */
uint64_t dr_neural_colorize_plan(uint32_t width, uint32_t height, uint32_t side,
    uint32_t bytes_per_sample, char **err_out);
bool dr_image_neural_colorize_input(const DrImage *img, const uint8_t *profile,
    size_t profile_length, uint32_t side, float *image_out, size_t image_count, uint64_t limit,
    uint64_t other_retained_bytes, const DrJobControl *control, char **err_out);
bool dr_precision_image_neural_colorize_input(const DrPrecisionImage *img, const uint8_t *profile,
    size_t profile_length, uint32_t side, float *image_out, size_t image_count, uint64_t limit,
    uint64_t other_retained_bytes, const DrJobControl *control, char **err_out);
DrImage *dr_image_neural_colorize_apply(const DrImage *img, const uint8_t *profile,
    size_t profile_length, uint32_t side, const float *chroma, size_t chroma_count,
    const char *adjust_json, uint64_t limit, uint64_t other_retained_bytes,
    const DrJobControl *control, char **err_out);
DrPrecisionImage *dr_precision_image_neural_colorize_apply(const DrPrecisionImage *img,
    const uint8_t *profile, size_t profile_length, uint32_t side, const float *chroma,
    size_t chroma_count, const char *adjust_json, uint64_t limit, uint64_t other_retained_bytes,
    const DrJobControl *control, char **err_out);

#ifdef __cplusplus
}
#endif

#endif /* DOCTORED_CORE_H */
