#!/usr/bin/env python3
"""The #1 Bitmap phases 1a to 3 MCP drive, and the close (B31 with arguments,
B33 the catalog sweep): an 8-bit Grayscale document
converts to Bitmap in one undo step (50% Threshold, Diffusion Dither, the
Output resolution; since 1b Pattern Dither, Halftone Screen and Custom
Pattern), back to Grayscale at Size Ratio 1 and 2, `.rz` reopens it,
`get_document` reports the mode, what the mode does not hold refuses with
its sentence, (1c) its 1-bit PNG, TIFF, BMP and PSD files write and
reopen, (2a-i) it paints in black and white and runs the bit-exact
geometry, (2a-ii) it takes the resampled geometry, Move, the selected pixels'
move, the selection's own edits and Cut, and (2b) phase 2b's take and menu
walk are applied: the fitted Pattern table, Sierra Lite diffusion, the walk's
refusals and Darken and Lighten, and (3) what the walk left is built through
the same adapter or refused for good in three tiers, no refusal naming a
phase, on the Debug app. Cut and Copy run on a private pasteboard
(`DR_PASTEBOARD_NAME` set here).

The gates are FIXED here BEFORE anything is measured; a failed gate IS the
receipt and no bound is widened afterwards. The instance is owned by this file
(`phase5_support.App` on its own scratch MCP socket: a dummy API key and a
PRIVATE directory for every `DR_*_DIR`, through `verification_isolation`'s
`preset_environment` list) and is terminated by its own context manager; no
Doctored process this file did not start is signalled. It never launches
Photoshop. Every fixture is written HERE in pure Python under `--output`: 8-bit
gray PNGs tagged sRGB (so the Gray profile is sGray and `render`'s sRGB codes
ARE the gray codes), a 16-bit gray PNG and an RGB PNG.

`render` is read as one code a pixel (R = G = B asserted); a Bitmap pixel is
0 or 255 and nothing else.

- **B1 threshold.** `RAMP` (16 × 4, gray `(16x + 40y) mod 256`) converts with
  `method threshold`: every pixel is 255 where the code is at least 128 and 0
  elsewhere; the reply answers `color_mode` bitmap and `depth` 1; undo answers
  gray with `render` equal to the source codes; redo answers bitmap with the
  same Bitmap render.
- **B2 diffusion.** A fresh copy of `RAMP` converts with no method (Diffusion
  Dither by default): `render` equals the drive-local Sierra Lite below
  (phase 2b; serpentine, odd rows right to left, w = g + e unclamped, white
  iff w ≥ 128, the error w − {0, 255} spread 2/4 ahead, 1/4 below-behind and
  1/4 below, dropped past the edges).
- **B3 resolution.** A fresh 4 × 4 copy (72 ppi) converts with `resolution`
  144 and `method threshold`: the document is 8 × 8, `get_document`'s
  resolution is 144 × 144, and every pixel is 0 or 255.
- **B4 Grayscale.** On B1's document `convert_mode gray` (Size Ratio 1)
  answers gray at depth 8 with a `render` equal to the Bitmap render; undo
  answers bitmap. `convert_mode gray size_ratio 2` answers an 8 × 2 gray
  document whose each pixel is `(510·W + n) // (2n)` of its 2 × 2 block of the
  Bitmap render (W whites of n pixels), computed here; undo answers bitmap.
- **B5 .rz.** `save_copy rz` of B1's document reopens as bitmap with an equal
  `render`.
- **B6 refusals.** Exactly, on B1's document: `new_layer` → `DISABLED`;
  `export_image` (PNG) is admitted since 1c (the Gradient since phase 3: B32);
  `save_copy gif` → `SAVE_REFUSAL`; `convert_mode rgb` → `MODE_RGB`. On an 8-bit RGB document
  `convert_mode bitmap` → `GRAY_FIRST`; on a 16-bit Grayscale document →
  `EIGHT_FIRST`; after `new_layer` on a fresh Gray copy, `convert_mode bitmap`
  → `FLATTEN`, then `flatten true` answers bitmap with one layer.
- **B7 get_document.** B1's document's `get_document` answers `color_mode`
  bitmap.
- **B8 pattern.** A fresh `RAMP` converts with `method pattern`: `render`
  equals the drive-local PT8-MID table (phase 2b's fit, white iff g ≥
  T[y mod 8][x mod 8], from the origin); undo answers gray with the source
  codes.
- **B9 halftone.** An 8 × 4 flat 200 document at 72 ppi converts with
  `method halftone`, `frequency` 18, `angle` 0, `shape square`: in each 4 × 4
  cell the centre 2 × 2 is black and the ring white (the core test's hand
  cell). A fresh `RAMP` at 12 lpi, 30°, `line` equals the drive-local copy of
  the shipped halftone rule (`HALFTONE` below: AREA order over the 256²
  grid, counterclockwise, cells at the corner).
- **B10 custom.** A fresh `RAMP` with `method custom`, `pattern {"preset":
  "stripes"}` is white iff x mod 8 ≥ 4 is false and g > 0 (stripes' gray is 0
  for x mod 8 < 4, 255 elsewhere); with an inline 2 × 2 tile of grays 64, 128 /
  192, 255 it is white iff g > the tile's gray at (x mod 2, y mod 2).
- **B11 option refusals.** Exactly: `frequency` with `method threshold` →
  `APPLIES_HALFTONE`; `pattern` with `method halftone` → `APPLIES_CUSTOM`;
  `frequency` 0.5 → `FREQUENCY`; `angle` 181 → `ANGLE`; `shape oval` → `SHAPE`;
  `frequency "fast"` → `HOST_FREQUENCY`; `mode rgb` with `shape` →
  `SHAPE_BITMAP_ONLY`. `frequency "53"` and `angle "45"` (string forms) convert
  and equal `frequency` 53 and `angle` 45.
- **B12 the 1-bit files (1c).** On B1's document `save_copy` png, tif and bmp
  each reply `bit_depth` 1; the files' headers are parsed here (PNG IHDR depth
  1 colour type 0; TIFF BitsPerSample 1, Photometric 1; BMP 1 bpp with a black,
  white palette) and each reopens as bitmap with B1's bits. A 1-bit gray PNG
  written here opens as bitmap with its own bits, and a copy of it as rgb with
  `precision rgba8`. `export_psd` flattened writes a header of depth 1, mode 0;
  `inspect_psd` answers `import_precision` bitmap with no layers, and
  `open_document psd_mode composite` gives B1's bits. `inspect_psd_export` for
  tiff answers `PSD_TIFF`. `save_copy jpg` replies `bit_depth` 8 with
  `GRAY_ROUTE` (JPEG) in `flat_save_losses`; `export_image` png replies
  `bit_depth` 1.
- **B13 the Brush (2a-i).** On an all-white Bitmap document a `brush_stroke`
  (3, 6)-(20, 6) size 5 in `#7F7F7F` paints black, and on an all-black one in
  `#808080` white (the ink snapped by its gray at 128): `render` stays 0 or
  255, every changed pixel takes the ink, (12, 6) does, every pixel farther
  than 4.5 px from the stroke keeps its bit, `list_history`'s `undo_count`
  grows by exactly 1, undo restores the bits and redo reapplies them.
- **B14 the Eraser.** The same stroke with `eraser_stroke` on an all-black
  document: the changed pixels, (12, 6) among them, are white (MCP's
  background), the far ones keep their bits.
- **B15 Fill.** On `FIELD` (24 × 12, black left of x = 12, a white marker at
  (3, 3), a black one at (18, 8)) `fill` at (6, 6) in white turns the black
  left region white (the black marker, unconnected, stays); `fill_selection`
  black on `select_rect` (2, 2, 8 × 6) blacks exactly the rectangle;
  `clear_selection` whitens it again; `stroke_selection` black, width 2,
  inside, blacks (2, 2) and (5, 2), leaves (5, 5) white and changes nothing
  outside (1, 1)-(10, 8).
- **B16 the geometry.** On a fresh `FIELD`: `rotate` 90, 180 and 270, `flip`
  horizontal and vertical, `crop` (2, 1, 16 × 9), `crop` past the canvas by
  2, 1 with `background_color` `#000000` (the new strip black) and
  `canvas_size` + 4 centred (the new area white) each equal the plane moved
  here (`trim` refuses since phase 2b: B19); `rotate`'s reply carries
  `working_memory_plan_bytes`.
- **B17 refusals.** Exactly: `brush_stroke` Multiply and (phase 3) `gradient`
  Multiply → `BLEND_REFUSAL`; `new_layer`, `crop` past the canvas with `fill`
  `generative_expand` or `content_aware`, and `generative_expand` →
  `DISABLED` (`generative_expand`, `content_aware_fill`).
- **B18 the resampled geometry, the selections' edits and Cut (2a-ii).** On
  fresh `FIELD` copies (MCP's background, white), each edit keeps a binary
  render and adds exactly one `undo_count` step: `image_size` 48 × 24
  `filter nearest` equals each bit doubled and undo restores it;
  `image_size` 12 × 6 `filter catmull-rom` is 12 × 6 (`rotate` 30 refuses
  since phase 2b: B21); `crop` (2, 2, 20 × 8) at `angle` 5 is 20 × 8;
  `move_layers` [0] (3, 2) equals the plane moved here, vacated area white;
  `select_rect` (2, 2, 6 × 4) + `move_selected_pixels` (4, 0) equals the
  region moved here with the vacated part white; `modify_selection` invert
  (bounds the canvas), invert again (2, 2, 6 × 4), grow 1 (1, 1, 8 × 6) and
  `transform_selection` translate_x 3 (4, 1, 8 × 6) answer those
  `get_selection` bounds and move no bit; `cut` of `select_rect` (2, 2,
  8 × 6) whitens exactly it and undo restores it. Refusals exactly:
  `select_magic_wand` → `DISABLED`, `move_layers` with `duplicate` true →
  `COPY`, `transform_layers` with `duplicate` true → `DISABLED` (phase 2b).
  Since phase 3 `quick_select` (seed (3, 6)) is admitted: `get_selection`
  answers bounds and no bit moves.
- **B19 phase 2b's menu walk (2b).** On a fresh `FIELD`, every tool of
  walk2's `build.tools.disabled` (`WALK_DISABLED`) refuses exactly
  `DISABLED`, the corrected admissions (`WALK_CORRECTED`: Free Transform,
  Distort, Transform Again, the selected pixels' transforms and Trim) among
  them; since phase 3 every tool of its `phase_3` (`WALK_PHASE_3`), called
  with `document_id` alone, is either admitted (`PHASE3_TOOLS`: its answer
  holds no Bitmap refusal, `BITMAP_REFUSALS`) or refuses with exactly its
  for-good sentence (`for_good`: RULING-1's `apply_filter` `DISABLED`); no
  bit moves; then `move_layers` (−2, 1) still moves the plane (the walk's
  `kept_shared` ops serve it).
- **B20 Darken and Lighten (2b).** On a fresh `FIELD` with `select_rect`
  (8, 2, 8 × 8): `fill_selection` black in `darken` blackens the selection,
  one step; black in `lighten` moves no bit and adds no step; white in
  `lighten` whitens the selection, one step.
- **B21 RULING-1's leaves (2b).** On a fresh `FIELD`: `rotate` 30 and 45
  (`sampler` bicubic) refuse exactly `ARBITRARY`, `apply_filter` `lens_blur`
  exactly `LENS_BLUR`, `apply_filter` `blur` and `depth_blur` (Filter ▸
  Neural Filters, disabled) exactly `DISABLED`; no bit moves and no step is
  added; then `rotate` −90 still turns the canvas.

Phase 3 (B22–B32), each on fresh `FIELD` copies. A Gray twin is `convert_mode
gray` of a fresh copy, and its result is thresholded here (its opaque render's
code ≥ 128 is white). `one_step` is B18's: one `undo_count` step, a changed plane.

- **B22 Copy.** `select_rect` (2, 2, 6 × 4) then `copy` succeeds and moves no
  bit; `copy_merged` and `paste` → `DISABLED`.
- **B23 Quick Mask.** `set_quick_mask` on, a `quick_mask_stroke` of a 8 × 5
  rectangle (`coverage_rle16`), a `gradient`, then off: `get_selection`
  answers bounds it did not before and no bit moves.
- **B24 Liquify.** One `forward_warp` stroke (8, 6)-(16, 6), size 10, strength
  100, equals the twin's liquify thresholded, in one step.
- **B25 the Clone Stamp and the History Brush.** `clone_stamp` from (4, 6)
  along (14, 6)-(20, 6), size 5, hardness 100, equals the twin's clone
  thresholded, in one step; a pixel whose centre lies within 1 px of the
  stroke and between its ends takes the source bit 10 px to the left, and past
  3.5 px nothing moves. `output new_layer` →
  `DISABLED` of `add_layer`; `blend_mode multiply` → `BLEND_REFUSAL`. On
  another copy a black `brush_stroke` along the same line, then
  `history_brush_stroke` (size 9, hardness 100, opacity 100) from the state
  before it, gives back the original bits.
- **B26 Perspective Crop.** `corners` (2, 1), (21, 2), (22, 10), (1, 9) to
  20 × 9 bilinear equals the twin's perspective crop thresholded, one step.
- **B27 Straighten Layer.** `straighten_layers` with the line (0, 0)-(60, 4)
  is one step whose canvas is the twin's `fit_crop` box for `line [0, 0, 60,
  4]` and whose bits are the twin's `crop` at that box and `angle`
  thresholded; the reply's `rotation` equals `fit_crop`'s `angle`.
- **B28 the layer row.** `set_layer_properties` name `Plate`,
  `set_layer_label` red and `set_layer_lock` pixels move no bit; a
  `brush_stroke` then refuses with no Bitmap sentence (the lock's own), and
  `opacity` → `LAYER_PROPERTIES`; a `.rz` copy reopens bitmap with the name,
  the label and the bits.
- **B29 render.** `channel` bitmap and composite each render the composite's
  pixels; `red`, and `bitmap` with `layer` 0, → `RENDER_REFUSAL`.
- **B30 for good.** Every tool phase 3 added to `disabledTools`
  (`PHASE3_DISABLED`), `oneLayer`, `channelTools`, `save_selection` and five
  tools no list names (`PHASE3_UNOFFERED`) answers exactly its tier's
  sentence (`for_good`); no bit moves and no step is added.
- **B31 admitted, with arguments (the close).** On a fresh `FIELD` copy, in
  order: `set_resolution` `ppi_x` 144 (`get_document` reads 144 × 144);
  `set_file_info` `copyright` 'Bitmap close 547' (`get_file_info` echoes it);
  `set_ruler_origin` (2, 3); `auto_select_layer` (3, 3); `set_history_budget`
  268435456; `fit_crop` `line` [0, 0, 60, 4]; `select_magnetic` around (2, 2)-(20,
  9), `close` straight, then `deselect`; `set_color_samplers` at (3, 3) and
  (18, 8) (`get_color_samplers` has 2 rows); `set_path` Box, `rename_path` to
  Frame, `duplicate_path` 'Frame copy', `path_to_selection` Frame (not empty),
  `selection_to_path`, `deselect`, `fill_path` black and `stroke_path` white
  size 2 on layer 0, `delete_path` 'Frame copy'; `create_snapshot` 'B31' (its id
  the new `list_snapshots` entry), `rename_snapshot`, `save_snapshot`,
  `load_snapshot`, `restore_snapshot`, `delete_snapshot`; `open_second_view`
  (view 2), `set_view` view 2 zoom 1, `get_view` view 2, `close_second_view`,
  `save_workspace` and `restore_workspace` 'Bitmap close'; `set_proof_setup` and
  `render_proof` (an image) at `working_cmyk`, `save_proof_preset` 'Bitmap
  close'; `export_pdf` (a `%PDF` file); `contact_sheet` and
  `contact_sheet_from_sources` to a new document, one entry each; `save_action`
  of one horizontal `flip`, whose `run_action` mirrors the plane in one
  `undo_count` step. Every other `PHASE3_TOOLS` entry keeps its `document_id`
  alone call (it may fail by its own arguments). No answer holds a Bitmap
  refusal (`BITMAP_REFUSALS`), and no call errors unless the same call on a
  fresh Gray twin errors with the same text (recorded). Afterwards the document
  is bitmap with a binary render.
- **B32 the Gradient.** A linear `#000000` → `#ffffff` ramp from (0, 0) to
  (24, 0) equals the twin's gradient thresholded, in one step; `opacity` 0.4
  on an all-white copy answers `changed` false, moves no bit and adds no
  step.
- **B33 the close's catalog sweep.** On a fresh `FIELD` copy, every
  `tools/list` name outside `BITMAP_TOOLS` and `DOCUMENT_FREE`, called with
  `document_id` alone, is an error whose text is exactly `DISABLED` (in
  `DISABLED_ALL`), `ONE_LAYER` (`ONE_LAYER_ALL`), `CHANNEL` (`CHANNEL_ALL`),
  `SAVE_SELECTION` (`save_selection`) or else `UNOFFERED`. `BITMAP_TOOLS`,
  `DISABLED_ALL`, `ONE_LAYER_ALL` and `CHANNEL_ALL` are `BitmapDocument.tools`,
  `disabledTools`, `oneLayer` and `channelTools`, copied verbatim;
  `DOCUMENT_FREE` is `verify_indexed_mode.py`'s, checked here against every
  `.documentFree` entry of `app/Sources/AgentServer+*.swift`. No reply text and
  no `get_precision` key contains "phase"; the bits and `undo_count` are
  unchanged.
"""

import argparse
import base64
import bisect
import json
import math
import os
from pathlib import Path
import re
import struct
import sys
import zlib

sys.dont_write_bytecode = True
ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'scripts'))

from phase5_support import App

# ---- the expectations, fixed here BEFORE anything is measured -------------
W, H = 16, 4
RAMP = [(16 * x + 40 * y) % 256 for y in range(H) for x in range(W)]
DISABLED = '{} is not available in Bitmap mode.'
# Phase 3's for-good tiers (`BitmapDocument.refusal(tool:)`, the core's `refused_sentence`).
UNOFFERED = ('{} is unavailable in a Bitmap document: Doctored offers it on Grayscale and colour '
             'documents only; convert to Grayscale first.')
ONE_LAYER = '{} needs more than one layer, and a Bitmap document holds one.'
CHANNEL = ('{} needs an alpha channel, which a Bitmap document does not hold; convert to '
           'Grayscale first.')
SAVE_SELECTION = ('Save Selection on a Bitmap document is not built in Doctored: a Bitmap '
                  'document holds no alpha channel; convert to Grayscale first.')
RENDER_REFUSAL = ('A Bitmap document renders its composite or its Bitmap channel (bitmap), with no '
                  'layer: it has no colour, alpha or mask plane.')
LAYER_PROPERTIES = ("A Bitmap document's one layer takes a new name only: no opacity, blend mode, "
                    'visibility, offset or disclosure change.')
COPY = 'A copy move makes a second layer, which a Bitmap document cannot hold.'
BLEND_REFUSAL = ('The Brush, Fill, Stroke, the Paint Bucket, the Gradient and the Clone Stamp paint '
                 'in Normal, Darken or Lighten mode in a Bitmap document.')
# The fragments every Bitmap refusal sentence carries (B19, B31): an answer holding none of them
# is no Bitmap refusal.
BITMAP_REFUSALS = ['is not available in Bitmap mode', 'is unavailable in a Bitmap document',
                   'a Bitmap document holds one', 'which a Bitmap document does not hold',
                   'Save Selection on a Bitmap document', 'Bitmap phase']
# Phase 3's host lists (`BitmapDocument`), copied here before the drive ran: the tools it admits,
# and the for-good tiers it added.
PHASE3_TOOLS = [
    'gradient', 'copy', 'set_quick_mask', 'quick_mask_stroke', 'liquify', 'clone_stamp',
    'history_brush_stroke', 'quick_select', 'select_magnetic', 'perspective_crop', 'fit_crop',
    'detect_horizon', 'straighten_layers', 'reveal_canvas', 'set_layer_properties',
    'set_layer_lock', 'set_layer_label', 'histogram', 'set_resolution', 'get_metadata',
    'get_file_info', 'set_file_info', 'get_precision', 'set_ruler_origin', 'list_channels',
    'get_warp', 'auto_select_layer', 'run_action', 'contact_sheet', 'contact_sheet_from_sources',
    'list_snapshots', 'create_snapshot', 'rename_snapshot', 'delete_snapshot', 'restore_snapshot',
    'save_snapshot', 'load_snapshot', 'set_history_budget', 'recover_document', 'discard_recovery',
    'get_proof_setup', 'set_proof_setup', 'render_proof', 'save_proof_preset', 'export_pdf',
    'find_layers', 'list_layer_comps', 'get_view', 'set_view', 'save_workspace',
    'restore_workspace', 'open_second_view', 'close_second_view', 'list_views',
    'get_color_samplers', 'set_color_samplers', 'list_paths', 'set_path', 'rename_path',
    'duplicate_path', 'delete_path', 'path_to_selection', 'selection_to_path', 'fill_path',
    'stroke_path', 'list_lens_profiles']
PHASE3_DISABLED = [
    'copy_merged', 'paste', 'puppet_warp', 'transform_mask', 'get_mask_properties',
    'get_vector_mask', 'set_vector_mask_properties', 'remove_vector_mask', 'hdr_toning',
    'convert_precision', 'select_person_parts', 'object_finder_select', 'inspect_subjects',
    'select_subjects', 'select_people', 'inspect_faces', 'select_face_parts', 'object_select',
    'generative_variations', 'select_generative_variation', 'generate_variations',
    'discard_content_aware_fill_preview', 'edit_text_layer', 'edit_shape_layer',
    'edit_fill_layer', 'set_global_light', 'save_style_preset', 'place_image',
    'add_live_photo_layer', 'copy_layers_to_document', 'stack_mode', 'reduce_noise_from_stack']
PHASE3_ONE_LAYER = [
    'align_layers', 'distribute_layers', 'delete_layer', 'flatten_image', 'auto_align_layers',
    'auto_blend_layers', 'link_layers', 'unlink_layers', 'arrange_layer', 'reorder_layer',
    'reorder_layers', 'merge_layers_to_hdr']
PHASE3_CHANNEL = [
    'add_channel', 'delete_channel', 'duplicate_channel', 'rename_channel', 'set_channel_options',
    'invert_channel', 'load_selection', 'add_luminosity_masks']
# Five tools no list names: Doctored offers them on Grayscale and colour documents only.
PHASE3_UNOFFERED = ['add_text', 'export_layers', 'put_pixels', 'fade', 'separate_color']
# The close (B33): `BitmapDocument.tools`, `disabledTools`, `oneLayer` and `channelTools`,
# copied verbatim before the drive ran.
BITMAP_TOOLS = {
    'convert_mode', 'get_color_profile', 'get_document', 'render', 'sample_color', 'sample_pixel',
    'set_active_layer', 'set_selected_layers', 'select_rect', 'select_ellipse', 'select_polygon',
    'select_freehand', 'select_all', 'deselect', 'reselect', 'get_selection', 'undo', 'redo',
    'save_copy', 'list_guides', 'add_guide', 'remove_guide', 'clear_guides', 'list_history',
    'restore_history', 'close_document', 'get_recovery_status', 'write_recovery',
    'export_image', 'export_preview', 'quick_export', 'export_psd', 'inspect_psd_export',
    'brush_stroke', 'eraser_stroke', 'fill', 'fill_selection', 'stroke_selection',
    'clear_selection', 'crop', 'canvas_size', 'rotate', 'flip',
    'image_size', 'move_layers', 'move_selected_pixels', 'modify_selection',
    'transform_selection', 'cut',
    'gradient', 'copy', 'set_quick_mask', 'quick_mask_stroke', 'liquify', 'clone_stamp',
    'history_brush_stroke', 'quick_select', 'select_magnetic', 'perspective_crop', 'fit_crop',
    'detect_horizon', 'straighten_layers', 'reveal_canvas', 'set_layer_properties',
    'set_layer_lock', 'set_layer_label', 'histogram', 'set_resolution', 'get_metadata',
    'get_file_info', 'set_file_info', 'get_precision', 'set_ruler_origin', 'list_channels',
    'get_warp', 'auto_select_layer', 'run_action', 'contact_sheet', 'contact_sheet_from_sources',
    'list_snapshots', 'create_snapshot', 'rename_snapshot', 'delete_snapshot',
    'restore_snapshot', 'save_snapshot', 'load_snapshot', 'set_history_budget',
    'recover_document', 'discard_recovery', 'get_proof_setup', 'set_proof_setup',
    'render_proof', 'save_proof_preset', 'export_pdf', 'find_layers', 'list_layer_comps',
    'get_view', 'set_view', 'save_workspace', 'restore_workspace', 'open_second_view',
    'close_second_view', 'list_views', 'get_color_samplers', 'set_color_samplers',
    'list_paths', 'set_path', 'rename_path', 'duplicate_path', 'delete_path',
    'path_to_selection', 'selection_to_path', 'fill_path', 'stroke_path', 'set_clipping_path',
    'list_lens_profiles',
}
DISABLED_ALL = {
    'new_layer', 'group_layers', 'ungroup_layers', 'merge_down', 'merge_visible',
    'stamp_visible', 'stamp_selected', 'merge_group', 'layer_via_copy', 'layer_via_cut',
    'add_layer_mask', 'remove_layer_mask', 'set_layer_mask_enabled', 'set_mask_properties',
    'set_layer_style', 'apply_style_preset', 'set_layer_clipped', 'add_adjustment_layer',
    'edit_adjustment_layer', 'add_text_layer', 'add_shape_layer',
    'add_fill_layer', 'rasterize_artwork', 'set_vector_mask',
    'apply_image', 'calculations', 'content_aware_fill', 'preview_content_aware_fill',
    'generative_fill', 'duplicate_layer', 'duplicate_to_new_document', 'perspective_warp',
    'sky_replacement', 'select_subject', 'select_sky', 'refine_selection',
    'super_zoom', 'jpeg_artifacts_removal', 'neural_colorize',
    'auto_tone', 'auto_contrast', 'auto_color', 'equalize', 'generative_expand',
    'select_magic_wand',
    'transform_layer', 'transform_layers', 'distort_layer', 'transform_again',
    'transform_selected_pixels', 'warp_layer', 'trim', 'assign_profile', 'convert_profile',
    'select_color_range', 'select_similar', 'apply_filter',
    'depth_blur',
    'copy_merged', 'paste', 'puppet_warp', 'transform_mask', 'get_mask_properties',
    'get_vector_mask', 'set_vector_mask_properties', 'remove_vector_mask', 'hdr_toning',
    'convert_precision', 'select_person_parts', 'object_finder_select', 'inspect_subjects',
    'select_subjects', 'select_people', 'inspect_faces', 'select_face_parts', 'object_select',
    'generative_variations', 'select_generative_variation', 'generate_variations',
    'discard_content_aware_fill_preview', 'edit_text_layer', 'edit_shape_layer',
    'edit_fill_layer', 'set_global_light', 'save_style_preset', 'place_image',
    'add_live_photo_layer', 'copy_layers_to_document', 'stack_mode', 'reduce_noise_from_stack',
}
ONE_LAYER_ALL = {
    'align_layers', 'distribute_layers', 'delete_layer', 'flatten_image', 'auto_align_layers',
    'auto_blend_layers', 'link_layers', 'unlink_layers', 'arrange_layer', 'reorder_layer',
    'reorder_layers', 'merge_layers_to_hdr',
}
CHANNEL_ALL = {
    'add_channel', 'delete_channel', 'duplicate_channel', 'rename_channel',
    'set_channel_options', 'invert_channel', 'load_selection', 'add_luminosity_masks',
}
# `scripts/verify_indexed_mode.py`'s DOCUMENT_FREE, copied verbatim; B33 checks it against every
# `.documentFree` entry of `app/Sources/AgentServer+*.swift`.
DOCUMENT_FREE = {
    'package_action', 'import_action', 'list_actions', 'save_action', 'delete_action',
    'start_recording', 'stop_recording', 'list_adjustment_presets', 'save_adjustment_preset',
    'rename_adjustment_preset', 'delete_adjustment_preset', 'assisted_workspace_status',
    'assisted_open_case', 'assisted_session_state', 'assisted_synthetic_stroke',
    'assisted_finalize_case', 'run_batch', 'batch_status', 'cancel_batch', 'get_clipboard',
    'new_from_clipboard', 'list_documents', 'open_document', 'list_models', 'list_export_presets',
    'save_export_preset', 'delete_export_preset', 'set_default_export_preset', 'inspect_vector',
    'job_status', 'cancel_job', 'resource_usage', 'merge_to_hdr', 'new_document',
    'list_document_presets', 'save_document_preset', 'delete_document_preset', 'inspect_psd',
    'merge_panorama', 'read_photoshop_adjustment_preset', 'list_metadata_policies',
    'save_metadata_policy', 'delete_metadata_policy', 'list_proof_presets', 'delete_proof_preset',
    'set_default_proof_preset', 'list_raw_presets', 'delete_raw_preset', 'repair_raw_preset',
    'list_skies', 'describe_profile', 'list_installed_profiles', 'load_files_into_stack',
    'list_style_presets', 'rename_style_preset', 'delete_style_preset', 'import_style_presets',
    'export_style_presets', 'list_workspaces', 'delete_workspace',
}


def declared_document_free():
    """Every tool a `<prefix>ToolFacts` entry marks `.documentFree` (B33)."""
    found = set()
    for path in sorted((ROOT / 'app' / 'Sources').glob('AgentServer+*.swift')):
        found.update(re.findall(r'"(\w+)":\s*\[[^\]]*\.documentFree\b', path.read_text()))
    return found


def for_good(tool):
    """The exact for-good sentence of a tool phase 3 refuses (B19, B30)."""
    if tool in PHASE3_DISABLED or tool in RULED_PERMANENT:
        return DISABLED.format(tool)
    if tool in PHASE3_ONE_LAYER:
        return ONE_LAYER.format(tool)
    if tool in PHASE3_CHANNEL:
        return CHANNEL.format(tool)
    if tool == 'save_selection':
        return SAVE_SELECTION
    return UNOFFERED.format(tool)
# Phase 2b's menu walk (`bitmap-mode-547/walk2/decided.json`, `build.tools`), copied here
# before the drive ran: what refuses for good, what the walk left to phase 3 (each now admitted or
# refused for good, B19), and the corrected admissions.
WALK_DISABLED = [
    'add_adjustment_layer', 'add_fill_layer', 'add_layer_mask', 'add_shape_layer',
    'add_text_layer', 'apply_image', 'apply_style_preset', 'assign_profile', 'auto_color',
    'auto_contrast', 'auto_tone', 'calculations', 'content_aware_fill', 'convert_profile',
    'distort_layer', 'duplicate_layer', 'duplicate_to_new_document', 'edit_adjustment_layer',
    'equalize', 'generative_expand', 'generative_fill', 'group_layers', 'jpeg_artifacts_removal',
    'layer_via_copy', 'layer_via_cut', 'merge_down', 'merge_group', 'merge_visible',
    'neural_colorize', 'new_layer', 'perspective_warp', 'preview_content_aware_fill',
    'rasterize_artwork', 'refine_selection', 'remove_layer_mask', 'select_color_range',
    'select_magic_wand', 'select_similar', 'select_sky', 'select_subject', 'set_layer_clipped',
    'set_layer_mask_enabled', 'set_layer_style', 'set_mask_properties', 'set_vector_mask',
    'sky_replacement', 'stamp_selected', 'stamp_visible', 'super_zoom', 'transform_again',
    'transform_layer', 'transform_layers', 'transform_selected_pixels', 'trim', 'ungroup_layers',
    'warp_layer']
WALK_PHASE_3 = [
    'align_layers', 'apply_filter', 'auto_align_layers', 'auto_blend_layers', 'copy',
    'copy_merged', 'delete_layer', 'distribute_layers', 'fit_crop', 'flatten_image', 'hdr_toning',
    'liquify', 'load_selection', 'paste', 'perspective_crop', 'puppet_warp', 'reveal_canvas',
    'save_selection', 'set_layer_lock', 'set_layer_properties', 'set_quick_mask']
WALK_CORRECTED = ['distort_layer', 'transform_again', 'transform_layer', 'transform_layers',
                  'transform_selected_pixels', 'trim']
# RULING-1 (controller, 2026-10-23) over the walk's row rule: `apply_filter` refuses for good,
# though its row named phase 3 (only Blur > Lens Blur was enabled, and it is not built here, by
# its own sentence), and `rotate` at an arbitrary angle refuses (Image Rotation > Arbitrary was
# measured disabled), its quarter turns staying; `depth_blur` refuses for good (Filter > Neural
# Filters, its one leaf, was measured disabled; B21).
RULED_PERMANENT = ['apply_filter']
ARBITRARY = 'Arbitrary rotation is not available in Bitmap mode.'
LENS_BLUR = ('Lens Blur on a Bitmap document is not built in Doctored; '
             'convert to Grayscale first.')
# Phase 2b's Pattern Dither table (PT8-MID, `bitmap_take.py`'s fit1): white iff g >= T[y%8][x%8].
PT8_MID = [
    [252, 124, 222, 95, 240, 121, 210, 83], [124, 221, 94, 247, 120, 209, 90, 59],
    [220, 93, 246, 119, 208, 89, 66, 187], [92, 245, 118, 215, 88, 65, 186, 35],
    [244, 117, 214, 87, 64, 185, 34, 155], [116, 213, 86, 63, 192, 33, 154, 51],
    [212, 85, 62, 191, 32, 153, 58, 179], [84, 61, 190, 31, 160, 57, 178, 27]]
# Phase 2a-i's field (B13-B16): 24 x 12, black left of x = 12, white right of it, with a white
# marker at (3, 3) and a black one at (18, 8).
FW, FH = 24, 12
FIELD = [255 if (x, y) == (3, 3) else 0 if (x, y) == (18, 8) else (0 if x < 12 else 255)
         for y in range(FH) for x in range(FW)]
SAVE_REFUSAL = ('A Bitmap document saves as .doctored, as a 1-bit PNG, TIFF or BMP, or as JPEG through '
                'its 8-bit Grayscale render; convert to Grayscale for other formats.')
GRAY_ROUTE = "the 1-bit Bitmap plane ({} holds the document's 8-bit Grayscale render)"
PSD_TIFF = ('A Bitmap document writes a 1-bit PSD or PSB; a TIFF with layers cannot hold it. Save '
            'a Copy writes its 1-bit TIFF.')
MODE_RGB = ('RGB Color is unavailable in a Bitmap document; convert to Grayscale '
            'first.')
GRAY_FIRST = ('Bitmap converts an 8-bit Grayscale document; convert to Grayscale '
              'first.')
EIGHT_FIRST = ('Bitmap converts an 8-bit Grayscale document; convert to 8 '
               'Bits/Channel first.')
FLATTEN = 'Bitmap holds one layer: flatten the image.'
APPLIES_HALFTONE = 'mode option frequency applies to method halftone'
APPLIES_CUSTOM = 'mode option pattern applies to method custom'
FREQUENCY = 'mode option frequency must be 1 to 999.999 lines per inch'
ANGLE = 'mode option angle must be -180 to 180 degrees'
SHAPE = 'mode option shape must be round, ellipse, line, square, cross or diamond'
HOST_FREQUENCY = 'frequency must be 1 to 999.999 lines per inch.'
SHAPE_BITMAP_ONLY = 'shape applies to mode bitmap only.'
# The shipped halftone rule (`bitmap_method::HALFTONE`): AREA order, turn -1
# (counterclockwise), cells at the corner.
HALFTONE = dict(area=True, turn=-1.0, centred=False)
TILE = [64, 128, 192, 255]


def spot(shape, s, t):
    """Adobe's line and square spot functions (a = |s|, b = |t|)."""
    a, b = abs(s), abs(t)
    return -b if shape == 'line' else -max(a, b)


def halftone(width, grays, frequency, angle, shape, ppi=72):
    """The drive-local copy of the shipped halftone rule (module doc, B9)."""
    samples = [(k + 0.5) / 128.0 - 1.0 for k in range(256)]
    order = sorted(spot(shape, si, sj) for si in samples for sj in samples)
    phi = HALFTONE['turn'] * math.radians(angle)
    sin, cos = math.sin(phi), math.cos(phi)
    offset = 0.5 if HALFTONE['centred'] else 0.0
    out = []
    for i, g in enumerate(grays):
        xx, yy = (i % width + 0.5) / ppi, (i // width + 0.5) / ppi
        u = frequency * (xx * cos + yy * sin) + offset
        v = frequency * (-xx * sin + yy * cos) + offset
        here = spot(shape, 2.0 * (u - math.floor(u)) - 1.0, 2.0 * (v - math.floor(v)) - 1.0)
        fraction = bisect.bisect_left(order, here) / len(order)
        out.append(1 if g > 0.5 + 254.0 * fraction else 0)
    return out


def sierra_lite(width, height, grays):
    """The drive-local oracle (module doc, B2): 1 white, 0 black."""
    error = [0.0] * (width * height)
    out = [0] * (width * height)
    for y in range(height):
        back = y % 2 == 1
        xs = range(width - 1, -1, -1) if back else range(width)
        ahead = -1 if back else 1
        for x in xs:
            i = y * width + x
            w = grays[i] + error[i]
            white = w >= 128.0
            out[i] = 1 if white else 0
            e = w - (255.0 if white else 0.0)
            if 0 <= x + ahead < width:
                error[i + ahead] += e * 2 / 4
            if y + 1 < height:
                if 0 <= x - ahead < width:
                    error[i + width - ahead] += e * 1 / 4
                error[i + width] += e * 1 / 4
    return out


def chunk(kind, data):
    return struct.pack('>I', len(data)) + kind + data + struct.pack('>I', zlib.crc32(kind + data))


def png(path, w, h, depth, colour, samples, extra=()):
    """A PNG of `samples` (row-major, channels per colour type) at `depth`."""
    channels = {0: 1, 2: 3}[colour]
    rows = b''
    for y in range(h):
        rows += b'\0'
        for v in samples[y * w * channels:(y + 1) * w * channels]:
            rows += struct.pack('>H', v) if depth == 16 else bytes([v])
    body = chunk(b'IHDR', struct.pack('>IIBBBBB', w, h, depth, colour, 0, 0, 0))
    for c in extra:
        body += c
    path.write_bytes(b'\x89PNG\r\n\x1a\n' + body + chunk(b'IDAT', zlib.compress(rows))
                     + chunk(b'IEND', b''))


def gray_png(path, w, h, codes):
    """An 8-bit gray PNG tagged sRGB (module doc)."""
    png(path, w, h, 8, 0, codes, extra=(chunk(b'sRGB', b'\0'),))


def packed(bits, w):
    """`bits` (1 white) as rows of ceil(w/8) bytes, MSB first, pad bits 0."""
    line = (w + 7) // 8
    out = bytearray(line * (len(bits) // w))
    for i, b in enumerate(bits):
        x, y = i % w, i // w
        if b:
            out[y * line + x // 8] |= 0x80 >> (x % 8)
    return bytes(out)


def png1(path, w, h, bits):
    """A 1-bit gray PNG (colour type 0, depth 1, 1 white) of `bits`, B12."""
    line = (w + 7) // 8
    rows = packed(bits, w)
    raw = b''.join(b'\0' + rows[y * line:(y + 1) * line] for y in range(h))
    body = chunk(b'IHDR', struct.pack('>IIBBBBB', w, h, 1, 0, 0, 0, 0))
    path.write_bytes(b'\x89PNG\r\n\x1a\n' + body + chunk(b'IDAT', zlib.compress(raw))
                     + chunk(b'IEND', b''))


def tiff_tags(data):
    """IFD0 of a classic TIFF: tag → first value (SHORT or LONG), B12."""
    order = '<' if data[:2] == b'II' else '>'
    assert struct.unpack(order + 'H', data[2:4])[0] == 42
    ifd = struct.unpack(order + 'I', data[4:8])[0]
    tags = {}
    for i in range(struct.unpack(order + 'H', data[ifd:ifd + 2])[0]):
        at = ifd + 2 + 12 * i
        tag, kind, _ = struct.unpack(order + 'HHI', data[at:at + 8])
        if kind == 3:
            tags[tag] = struct.unpack(order + 'H', data[at + 8:at + 10])[0]
        elif kind == 4:
            tags[tag] = struct.unpack(order + 'I', data[at + 8:at + 12])[0]
    return tags


def png_pixels(data):
    """(width, height, pixels as tuples) of an 8/16-bit PNG of colour type 0, 2, 4 or 6."""
    assert data[:8] == b'\x89PNG\r\n\x1a\n'
    at, packed = 8, b''
    width = height = depth = colour = 0
    while at + 8 <= len(data):
        length, kind = struct.unpack('>I4s', data[at:at + 8])
        body = data[at + 8:at + 8 + length]
        if kind == b'IHDR':
            width, height, depth, colour, _, _, interlace = struct.unpack('>IIBBBBB', body)
            assert depth in (8, 16) and colour in (0, 2, 4, 6) and interlace == 0
        elif kind == b'IDAT':
            packed += body
        at += 12 + length
    channels = {0: 1, 2: 3, 4: 2, 6: 4}[colour]
    unit = channels * depth // 8
    stride = width * unit
    raw = zlib.decompress(packed)
    previous = bytearray(stride)
    pixels = []
    for y in range(height):
        start = y * (stride + 1)
        kind = raw[start]
        row = bytearray(raw[start + 1:start + 1 + stride])
        for i in range(stride):
            left = row[i - unit] if i >= unit else 0
            up = previous[i]
            corner = previous[i - unit] if i >= unit else 0
            if kind == 1:
                row[i] = (row[i] + left) & 255
            elif kind == 2:
                row[i] = (row[i] + up) & 255
            elif kind == 3:
                row[i] = (row[i] + (left + up) // 2) & 255
            elif kind == 4:
                p = left + up - corner
                pa, pb, pc = abs(p - left), abs(p - up), abs(p - corner)
                row[i] = (row[i] + (left if pa <= pb and pa <= pc else up if pb <= pc else corner)) & 255
        previous = row
        for x in range(width):
            px = row[x * unit:(x + 1) * unit]
            if depth == 16:
                pixels.append(tuple(v >> 8 for v in struct.unpack(f'>{channels}H', bytes(px))))
            else:
                pixels.append(tuple(px))
    return width, height, pixels


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', required=True)
    args = parser.parse_args()
    output = Path(args.output).expanduser()
    output.mkdir(parents=True, exist_ok=True)
    folder = output / 'work'
    folder.mkdir(parents=True, exist_ok=False)
    receipts = {'expectations': 'fixed in scripts/verify_bitmap_mode.py before any measurement'}
    # Phase 2a-ii's Cut (B18) touches the pasteboard: a private one, never the user's.
    os.environ['DR_PASTEBOARD_NAME'] = f'com.kgalligan.doctored.verify-bitmap-mode.{os.getpid()}'

    with App(folder / 'app', folder / 'recovery', interval=600) as app:
        call = app.call

        def summary(doc):
            return next(d for d in call('list_documents')['documents'] if d['id'] == doc)

        def mode(doc):
            return summary(doc)['color_mode']

        def codes(doc):
            """`render` as one code a pixel (R = G = B asserted)."""
            result = app.client.rpc('tools/call', {'name': 'render',
                                                   'arguments': {'document_id': doc}})
            assert not result.get('isError'), result
            data = next(c['data'] for c in result['content'] if c['type'] == 'image')
            _, _, px = png_pixels(base64.b64decode(data))
            out = []
            for p in px:
                assert len(set(p[:3])) == 1 or len(p) <= 2, p
                assert len(p) not in (2, 4) or p[-1] == 255, p
                out.append(p[0])
            return out

        def bits(doc):
            px = codes(doc)
            assert all(v in (0, 255) for v in px), px
            return [1 if v == 255 else 0 for v in px]

        def gray_copy(name, w=W, h=H, values=RAMP):
            path = folder / name
            gray_png(path, w, h, values)
            doc = call('open_document', path=str(path))['document']['id']
            assert mode(doc) == 'gray', summary(doc)
            return doc

        # ---- B1: threshold + undo/redo --------------------------------------
        doc1 = gray_copy('ramp1.png')
        assert codes(doc1) == RAMP, codes(doc1)
        reply = call('convert_mode', document_id=doc1, mode='bitmap', method='threshold')
        assert reply['color_mode'] == 'bitmap' and reply['depth'] == 1, reply
        expected = [1 if g >= 128 else 0 for g in RAMP]
        b1 = bits(doc1)
        assert b1 == expected, b1
        call('undo', document_id=doc1)
        assert mode(doc1) == 'gray' and codes(doc1) == RAMP
        call('redo', document_id=doc1)
        assert mode(doc1) == 'bitmap' and bits(doc1) == expected
        receipts['b1'] = {'reply': {k: reply[k] for k in ('color_mode', 'depth',
                                                         'working_memory_plan_bytes')},
                          'bits': b1}
        print('PASS b1: 50% Threshold makes 128 and above white, one undo step', flush=True)

        # ---- B2: diffusion --------------------------------------------------
        doc2 = gray_copy('ramp2.png')
        call('convert_mode', document_id=doc2, mode='bitmap')
        b2 = bits(doc2)
        oracle = sierra_lite(W, H, RAMP)
        assert b2 == oracle, (b2, oracle)
        receipts['b2'] = b2
        print('PASS b2: Diffusion Dither equals the drive-local Sierra Lite, serpentine', flush=True)

        # ---- B3: Output resolution -----------------------------------------
        doc3 = gray_copy('small.png', 4, 4, RAMP[:16])
        call('convert_mode', document_id=doc3, mode='bitmap', method='threshold', resolution=144)
        info = call('get_document', document_id=doc3)
        assert (summary(doc3)['width'], summary(doc3)['height']) == (8, 8), summary(doc3)
        assert (info['resolution']['ppi_x'], info['resolution']['ppi_y']) == (144, 144), info['resolution']
        b3 = bits(doc3)
        assert len(b3) == 64
        receipts['b3'] = {'resolution': info['resolution'], 'bits': b3}
        print('PASS b3: Output resolution 144 resamples 4 × 4 at 72 ppi to 8 × 8 at 144', flush=True)

        # ---- B4: Grayscale at Size Ratio 1 and 2 ----------------------------
        gray1 = call('convert_mode', document_id=doc1, mode='gray')
        assert gray1['color_mode'] == 'gray' and gray1['depth'] == 8, gray1
        assert codes(doc1) == [255 * b for b in expected]
        call('undo', document_id=doc1)
        assert mode(doc1) == 'bitmap'
        gray2 = call('convert_mode', document_id=doc1, mode='gray', size_ratio=2)
        assert gray2['color_mode'] == 'gray', gray2
        assert (summary(doc1)['width'], summary(doc1)['height']) == (W // 2, H // 2), summary(doc1)
        averaged = []
        for by in range(H // 2):
            for bx in range(W // 2):
                block = [expected[(2 * by + dy) * W + 2 * bx + dx] for dy in (0, 1) for dx in (0, 1)]
                averaged.append((510 * sum(block) + 4) // 8)
        got = codes(doc1)
        assert got == averaged, (got, averaged)
        call('undo', document_id=doc1)
        assert mode(doc1) == 'bitmap' and bits(doc1) == expected
        receipts['b4'] = {'ratio_2': got}
        print('PASS b4: Grayscale at Size Ratio 1 is exact and at 2 averages each block', flush=True)

        # ---- B5: .rz --------------------------------------------------------
        rz = folder / 'bitmap.rz'
        call('save_copy', document_id=doc1, path=str(rz), format='rz')
        doc5 = call('open_document', path=str(rz))['document']['id']
        assert mode(doc5) == 'bitmap' and bits(doc5) == expected, summary(doc5)
        print('PASS b5: .rz reopens the Bitmap document exactly', flush=True)

        # ---- B6: refusals ---------------------------------------------------
        refusals = {
            'new_layer': call('new_layer', fail=True, document_id=doc1),
            'save_copy gif': call('save_copy', fail=True, document_id=doc1,
                                  path=str(folder / 'no.gif'), format='gif'),
            'convert_mode rgb': call('convert_mode', fail=True, document_id=doc1, mode='rgb'),
            'convert_mode gray profile': call('convert_mode', fail=True, document_id=doc1,
                                              mode='gray', profile='working'),
        }
        assert refusals['new_layer'] == DISABLED.format('new_layer'), refusals
        assert refusals['save_copy gif'] == SAVE_REFUSAL, refusals
        admitted = call('export_image', document_id=doc1, path=str(folder / 'b6.png'),
                        settings={'format': 'png'})
        assert admitted['ok'] is True, admitted
        assert refusals['convert_mode rgb'] == MODE_RGB, refusals
        assert refusals['convert_mode gray profile'].startswith(
            'profile, name and path do not apply to a Bitmap document'), refusals
        rgb_png = folder / 'rgb.png'
        png(rgb_png, 2, 1, 8, 2, [200, 10, 10, 10, 200, 10])
        doc_rgb = call('open_document', path=str(rgb_png))['document']['id']
        refusals['rgb source'] = call('convert_mode', fail=True, document_id=doc_rgb, mode='bitmap')
        assert refusals['rgb source'] == GRAY_FIRST, refusals
        gray16 = folder / 'gray16.png'
        png(gray16, 2, 1, 16, 0, [1000, 60000])
        doc16 = call('open_document', path=str(gray16))['document']['id']
        assert mode(doc16) == 'gray', summary(doc16)
        refusals['16-bit source'] = call('convert_mode', fail=True, document_id=doc16, mode='bitmap')
        assert refusals['16-bit source'] == EIGHT_FIRST, refusals
        doc6 = gray_copy('ramp6.png')
        call('new_layer', document_id=doc6)
        assert summary(doc6)['layer_count'] == 2
        refusals['two layers'] = call('convert_mode', fail=True, document_id=doc6, mode='bitmap')
        assert refusals['two layers'] == FLATTEN, refusals
        flat = call('convert_mode', document_id=doc6, mode='bitmap', method='threshold', flatten=True)
        assert flat['color_mode'] == 'bitmap' and summary(doc6)['layer_count'] == 1, flat
        assert bits(doc6) == expected
        receipts['b6'] = refusals
        print('PASS b6: the mode refuses by its sentences and flattens on request', flush=True)

        # ---- B7: get_document -----------------------------------------------
        info = call('get_document', document_id=doc1)
        assert info['color_mode'] == 'bitmap', info.get('color_mode')
        receipts['b7'] = info['color_mode']
        print('PASS b7: get_document reports color_mode bitmap', flush=True)

        # ---- B8: Pattern Dither ---------------------------------------------
        doc8 = gray_copy('ramp8.png')
        call('convert_mode', document_id=doc8, mode='bitmap', method='pattern')
        dithered = [1 if g >= PT8_MID[(i // W) % 8][(i % W) % 8] else 0
                    for i, g in enumerate(RAMP)]
        b8 = bits(doc8)
        assert b8 == dithered, (b8, dithered)
        call('undo', document_id=doc8)
        assert mode(doc8) == 'gray' and codes(doc8) == RAMP
        receipts['b8'] = b8
        print('PASS b8: Pattern Dither equals the drive-local PT8-MID table; undo answers gray',
              flush=True)

        # ---- B9: Halftone Screen --------------------------------------------
        doc9 = gray_copy('flat9.png', 8, 4, [200] * 32)
        call('convert_mode', document_id=doc9, mode='bitmap', method='halftone', frequency=18,
             angle=0, shape='square')
        cell = [0 if (i % 8) % 4 in (1, 2) and (i // 8) % 4 in (1, 2) else 1 for i in range(32)]
        b9 = bits(doc9)
        assert b9 == cell, b9
        doc9b = gray_copy('ramp9.png')
        call('convert_mode', document_id=doc9b, mode='bitmap', method='halftone', frequency=12,
             angle=30, shape='line')
        lined = bits(doc9b)
        oracle9 = halftone(W, RAMP, 12, 30, 'line')
        assert lined == oracle9, (lined, oracle9)
        receipts['b9'] = {'cell': b9, 'line': lined}
        print('PASS b9: the hand halftone cell and a 12 lpi 30° line screen equal the rule',
              flush=True)

        # ---- B10: Custom Pattern --------------------------------------------
        doc10 = gray_copy('ramp10.png')
        call('convert_mode', document_id=doc10, mode='bitmap', method='custom',
             pattern={'preset': 'stripes'})
        striped = [1 if (i % W) % 8 < 4 and g > 0 else 0 for i, g in enumerate(RAMP)]
        b10 = bits(doc10)
        assert b10 == striped, (b10, striped)
        doc10b = gray_copy('ramp10b.png')
        rgba = base64.b64encode(bytes(c for v in TILE for c in (v, v, v, 255))).decode()
        call('convert_mode', document_id=doc10b, mode='bitmap', method='custom',
             pattern={'name': 'T', 'width': 2, 'height': 2, 'rgba': rgba})
        tiled = [1 if g > TILE[((i // W) % 2) * 2 + (i % W) % 2] else 0
                 for i, g in enumerate(RAMP)]
        b10b = bits(doc10b)
        assert b10b == tiled, (b10b, tiled)
        receipts['b10'] = {'stripes': b10, 'tile': b10b}
        print('PASS b10: Custom Pattern over stripes and an inline tile', flush=True)

        # ---- B11: option refusals and string forms --------------------------
        doc11 = gray_copy('ramp11.png')
        options = {
            'applies halftone': ({'method': 'threshold', 'frequency': 53}, APPLIES_HALFTONE),
            'applies custom': ({'method': 'halftone', 'pattern': {'preset': 'stripes'}},
                               APPLIES_CUSTOM),
            'frequency': ({'method': 'halftone', 'frequency': 0.5}, FREQUENCY),
            'angle': ({'method': 'halftone', 'angle': 181}, ANGLE),
            'shape': ({'method': 'halftone', 'shape': 'oval'}, SHAPE),
            'host frequency': ({'method': 'halftone', 'frequency': 'fast'}, HOST_FREQUENCY),
        }
        b11 = {}
        for label, (arguments, sentence) in options.items():
            b11[label] = call('convert_mode', fail=True, document_id=doc11, mode='bitmap',
                              **arguments)
            assert b11[label] == sentence, (label, b11[label])
        b11['rgb shape'] = call('convert_mode', fail=True, document_id=doc11, mode='rgb',
                                shape='round')
        assert b11['rgb shape'] == SHAPE_BITMAP_ONLY, b11
        assert mode(doc11) == 'gray', summary(doc11)
        call('convert_mode', document_id=doc11, mode='bitmap', method='halftone',
             frequency='53', angle='45')
        strings = bits(doc11)
        doc11b = gray_copy('ramp11b.png')
        call('convert_mode', document_id=doc11b, mode='bitmap', method='halftone',
             frequency=53, angle=45)
        assert strings == bits(doc11b), strings
        receipts['b11'] = b11
        print('PASS b11: every option refusal by sentence; string frequency and angle convert',
              flush=True)

        # ---- B12: the 1-bit files (1c) --------------------------------------
        b12 = {}
        plane = packed(expected, W)
        for ext in ('png', 'tif', 'bmp'):
            path = folder / f'kept.{ext}'
            reply = call('save_copy', document_id=doc1, path=str(path), format=ext)
            assert reply['bit_depth'] == 1, reply
            data = path.read_bytes()
            if ext == 'png':
                assert data[12:16] == b'IHDR' and data[24] == 1 and data[25] == 0, data[:32]
            elif ext == 'tif':
                tags = tiff_tags(data)
                assert (tags.get(258), tags.get(262), tags.get(277)) == (1, 1, 1), tags
                strip = data[tags[273]:tags[273] + tags[279]]
                assert strip == plane, strip
            else:
                bpp, = struct.unpack('<H', data[28:30])
                assert data[:2] == b'BM' and bpp == 1, data[:32]
                assert data[54:62] == bytes([0, 0, 0, 0, 255, 255, 255, 0]), data[54:62]
            reopened = call('open_document', path=str(path))['document']['id']
            assert mode(reopened) == 'bitmap' and bits(reopened) == expected, summary(reopened)
            b12[ext] = {'bit_depth': reply['bit_depth'], 'losses': reply['flat_save_losses']}
        # A 1-bit gray PNG written here, and a copy of it opened as 8-bit RGB.
        own = [1 if (x + 2 * y) % 3 else 0 for y in range(3) for x in range(13)]
        own_png = folder / 'own1.png'
        png1(own_png, 13, 3, own)
        own_doc = call('open_document', path=str(own_png))['document']['id']
        assert mode(own_doc) == 'bitmap' and bits(own_doc) == own, summary(own_doc)
        own_rgb = folder / 'own1-rgb.png'
        own_rgb.write_bytes(own_png.read_bytes())
        rgb_doc = call('open_document', path=str(own_rgb), precision='rgba8')['document']['id']
        assert mode(rgb_doc) == 'rgb', summary(rgb_doc)
        # The PSD: depth 1, mode 0, no layers; its composite reopens with B1's bits.
        psd = folder / 'bitmap.psd'
        exported = call('export_psd', document_id=doc1, path=str(psd), mode='flattened',
                        format='psd')
        assert exported['bit_depth'] == 1, exported
        header = psd.read_bytes()[:26]
        depth, color = struct.unpack('>HH', header[22:26])
        assert header[:4] == b'8BPS' and (depth, color) == (1, 0), header
        inspected = call('inspect_psd', path=str(psd))
        assert (inspected['import_precision'], inspected['layer_count'],
                inspected['layered_available']) == ('bitmap', 0, False), inspected
        psd_doc = call('open_document', path=str(psd), psd_mode='composite',
                       psd_fingerprint=inspected['source_fingerprint'])['document']['id']
        assert mode(psd_doc) == 'bitmap' and bits(psd_doc) == expected, summary(psd_doc)
        tiff_report = call('inspect_psd_export', document_id=doc1, format='tiff')
        assert tiff_report['refusal'] == PSD_TIFF, tiff_report
        # JPEG goes through the Gray render; export_image keeps the plane.
        jpeg = call('save_copy', document_id=doc1, path=str(folder / 'gray.jpg'), format='jpg')
        assert jpeg['bit_depth'] == 8, jpeg
        assert GRAY_ROUTE.format('JPEG') in jpeg['flat_save_losses'], jpeg
        image = call('export_image', document_id=doc1, path=str(folder / 'export.png'),
                     settings={'format': 'png'})
        assert image['bit_depth'] == 1, image
        b12.update(psd=exported, jpeg=jpeg['flat_save_losses'], export_image=image['bit_depth'])
        receipts['b12'] = b12
        print('PASS b12: 1-bit PNG, TIFF, BMP and PSD write and reopen; JPEG through Gray',
              flush=True)

        # ---- B13: the Brush (2a-i) ------------------------------------------
        def bitmap_copy(name, values, w=FW, h=FH):
            doc = gray_copy(name, w, h, values)
            call('convert_mode', document_id=doc, mode='bitmap', method='threshold')
            assert mode(doc) == 'bitmap', summary(doc)
            return doc

        def undo_count(doc):
            return call('list_history', document_id=doc)['undo_count']

        def far(x, y, reach=4.5):
            """Farther than `reach` from the stroke (3, 6)-(20, 6)."""
            dx = 0 if 3 <= x <= 20 else min(abs(x - 3), abs(x - 20))
            return (dx * dx + (y - 6) ** 2) ** 0.5 > reach

        stroke = dict(layer=0, points=[[3, 6], [20, 6]], size=5)
        b13 = {}
        for name, start, color, ink in (('white', 255, '#7F7F7F', 0), ('black', 0, '#808080', 1)):
            doc = bitmap_copy(f'flat13-{name}.png', [start] * (FW * FH))
            before = bits(doc)
            steps = undo_count(doc)
            reply = call('brush_stroke', document_id=doc, color=color, **stroke)
            assert reply.get('changed') is True, reply
            assert undo_count(doc) == steps + 1, (name, undo_count(doc), steps)
            after = bits(doc)
            for i, (a, b) in enumerate(zip(before, after)):
                if far(i % FW, i // FW):
                    assert a == b, (name, i % FW, i // FW)
            assert after[6 * FW + 12] == ink, (name, after[6 * FW + 12])
            painted = sum(a != b for a, b in zip(before, after))
            assert painted > 0 and all(b == ink for a, b in zip(before, after) if a != b), name
            call('undo', document_id=doc)
            assert bits(doc) == before, name
            call('redo', document_id=doc)
            assert bits(doc) == after, name
            b13[name] = {'reply': reply, 'painted': painted}
            if name == 'black':
                doc13 = doc
        receipts['b13'] = b13
        print('PASS b13: the Brush paints #7F7F7F black and #808080 white, a binary render, one '
              'history step, undo restores the bits', flush=True)

        # ---- B14: the Eraser ------------------------------------------------
        black14 = bitmap_copy('flat14.png', [0] * (FW * FH))
        before = bits(black14)
        erased = call('eraser_stroke', document_id=black14, **stroke)
        assert erased.get('changed') is True, erased
        after = bits(black14)
        for i, (a, b) in enumerate(zip(before, after)):
            if far(i % FW, i // FW):
                assert a == b, (i % FW, i // FW)
        assert after[6 * FW + 12] == 1 and all(b == 1 for a, b in zip(before, after) if a != b)
        receipts['b14'] = erased
        print('PASS b14: the Eraser over black leaves the background, white', flush=True)

        # ---- B15: the Paint Bucket, Fill, Clear and Stroke -----------------
        field = [1 if v == 255 else 0 for v in FIELD]
        doc15 = bitmap_copy('field15.png', FIELD)
        assert bits(doc15) == field
        bucket = call('fill', document_id=doc15, layer=0, x=6, y=6, color='#FFFFFF')
        assert bucket.get('changed') is True, bucket
        # The black left half is one contiguous region (the white marker at (3, 3) is already
        # white); the black marker at (18, 8) is not connected to it.
        filled = [1 if (i % FW, i // FW) != (18, 8) else 0 for i in range(FW * FH)]
        assert bits(doc15) == filled, bits(doc15)
        rect = dict(x=2, y=2, width=8, height=6)
        inside = lambda x, y: 2 <= x < 10 and 2 <= y < 8
        call('select_rect', document_id=doc15, **rect)
        filled_sel = call('fill_selection', document_id=doc15, layer=0, color='#000000')
        assert filled_sel.get('changed') is True, filled_sel
        selected = [0 if inside(i % FW, i // FW) else b for i, b in enumerate(filled)]
        assert bits(doc15) == selected, bits(doc15)
        cleared = call('clear_selection', document_id=doc15, layer=0)
        assert cleared.get('changed') is True, cleared
        assert bits(doc15) == filled, bits(doc15)
        stroked = call('stroke_selection', document_id=doc15, layer=0, color='#000000', width=2,
                       location='inside')
        assert stroked.get('changed') is True, stroked
        ring = bits(doc15)
        for i, (a, b) in enumerate(zip(filled, ring)):
            x, y = i % FW, i // FW
            if not (1 <= x < 11 and 1 <= y < 9):
                assert a == b, (x, y)
        assert ring[2 * FW + 2] == 0 and ring[2 * FW + 5] == 0, ring
        assert ring[5 * FW + 5] == 1, 'the inside stroke leaves the interior'
        call('deselect', document_id=doc15)
        receipts['b15'] = {'bucket': bucket, 'fill': filled_sel, 'clear': cleared,
                           'stroke': stroked}
        print('PASS b15: the Paint Bucket, Fill, Clear (to white) and Stroke land in black and '
              'white', flush=True)

        # ---- B16: the bit-exact geometry -----------------------------------
        def moved_bits(w, h, px, nw, nh, source, fill):
            out = []
            for y in range(nh):
                for x in range(nw):
                    sx, sy = source(x, y)
                    out.append(px[sy * w + sx] if 0 <= sx < w and 0 <= sy < h else fill)
            return out

        doc16 = bitmap_copy('field16.png', FIELD)
        w, h, px = FW, FH, field
        rotated = call('rotate', document_id=doc16, degrees=90)
        plan90 = rotated.get('working_memory_plan_bytes')
        assert isinstance(plan90, int) and plan90 > 0, rotated
        px = moved_bits(w, h, px, h, w, lambda x, y: (y, h - 1 - x), 1)
        w, h = h, w
        assert bits(doc16) == px, 'rotate 90'
        call('rotate', document_id=doc16, degrees=180)
        px = moved_bits(w, h, px, w, h, lambda x, y: (w - 1 - x, h - 1 - y), 1)
        assert bits(doc16) == px, 'rotate 180'
        call('rotate', document_id=doc16, degrees=270)
        px = moved_bits(w, h, px, h, w, lambda x, y: (w - 1 - y, x), 1)
        w, h = h, w
        assert bits(doc16) == px, 'rotate 270'
        for axis, source in (('horizontal', lambda x, y: (w - 1 - x, y)),
                             ('vertical', lambda x, y: (x, h - 1 - y))):
            call('flip', document_id=doc16, axis=axis)
            px = moved_bits(w, h, px, w, h, source, 1)
            assert bits(doc16) == px, axis
        call('crop', document_id=doc16, x=2, y=1, width=16, height=9)
        px = moved_bits(w, h, px, 16, 9, lambda x, y: (x + 2, y + 1), 1)
        w, h = 16, 9
        assert bits(doc16) == px, 'crop'
        past = call('crop', document_id=doc16, x=-2, y=-1, width=w + 4, height=h + 2,
                    background_color='#000000')
        assert past.get('ok', True) is not False, past
        px = moved_bits(w, h, px, w + 4, h + 2, lambda x, y: (x - 2, y - 1), 0)
        w, h = w + 4, h + 2
        assert bits(doc16) == px, 'crop past the canvas, black'
        grown = call('canvas_size', document_id=doc16, width=w + 4, height=h + 4, anchor='center')
        assert grown.get('ok') is True, grown
        px = moved_bits(w, h, px, w + 4, h + 4, lambda x, y: (x - 2, y - 2), 1)
        w, h = w + 4, h + 4
        assert bits(doc16) == px, 'canvas_size, white'
        # Trim refuses since phase 2b's walk: B19.
        assert mode(doc16) == 'bitmap'
        b16 = {'rotate_plan': plan90, 'size': [w, h]}
        receipts['b16'] = b16
        print('PASS b16: rotate 90/180/270, both flips, crop, a black past-canvas crop and Canvas '
              'Size (white) move the plane bit for bit', flush=True)

        # ---- B17: the refusals (2a-i) --------------------------------------
        b17 = {
            'multiply': call('brush_stroke', fail=True, document_id=doc13, blend_mode='multiply',
                             color='#000000', **stroke),
            'gradient multiply': call('gradient', fail=True, document_id=doc13, layer=0, x0=0,
                                      y0=0, x1=FW, y1=0, start_color='#000000',
                                      end_color='#ffffff', blend_mode='multiply'),
            'new_layer': call('new_layer', fail=True, document_id=doc13),
            'crop generative_expand': call('crop', fail=True, document_id=doc13, x=-2, y=-2,
                                           width=FW + 4, height=FH + 4,
                                           fill='generative_expand'),
            'crop content_aware': call('crop', fail=True, document_id=doc13, x=-2, y=-2,
                                       width=FW + 4, height=FH + 4, fill='content_aware'),
            'generative_expand': call('generative_expand', fail=True, document_id=doc13),
        }
        assert b17['multiply'] == BLEND_REFUSAL, b17
        assert b17['gradient multiply'] == BLEND_REFUSAL, b17
        assert b17['new_layer'] == DISABLED.format('new_layer'), b17
        assert b17['crop generative_expand'] == DISABLED.format('generative_expand'), b17
        assert b17['crop content_aware'] == DISABLED.format('content_aware_fill'), b17
        assert b17['generative_expand'] == DISABLED.format('generative_expand'), b17
        assert mode(doc13) == 'bitmap' and summary(doc13)['width'] == FW
        receipts['b17'] = b17
        print('PASS b17: the Brush and the Gradient in Multiply, a new layer and both crop fills '
              'refuse by their sentences', flush=True)

        # ---- B18: the resampled geometry, the selections' edits and Cut (2a-ii) ----------
        def binary(doc):
            """The render is 0 or 255 (`bits` asserts it) and the mode stays bitmap."""
            assert mode(doc) == 'bitmap', summary(doc)
            return bits(doc)

        def one_step(doc, tool, **arguments):
            """`tool` adds exactly one history step and changes the bits."""
            before, steps = bits(doc), undo_count(doc)
            reply = call(tool, document_id=doc, **arguments)
            assert reply.get('ok', True) is not False, (tool, reply)
            after = binary(doc)
            assert undo_count(doc) == steps + 1, (tool, undo_count(doc), steps)
            assert after != before, (tool, 'no bit moved')
            return reply, after

        def bounds(doc):
            return call('get_selection', document_id=doc).get('bounds')

        b18 = {}
        # Image Size: nearest doubles each bit; Catmull-Rom (the host's bicubic) is binary.
        doc18 = bitmap_copy('field18.png', FIELD)
        before = bits(doc18)
        reply, doubled = one_step(doc18, 'image_size', width=2 * FW, height=2 * FH,
                                  filter='nearest')
        assert doubled == [before[(y // 2) * FW + x // 2] for y in range(2 * FH)
                           for x in range(2 * FW)], 'image_size nearest'
        call('undo', document_id=doc18)
        assert bits(doc18) == before, 'image_size undo'
        reply, small = one_step(doc18, 'image_size', width=FW // 2, height=FH // 2,
                                filter='catmull-rom')
        assert (summary(doc18)['width'], summary(doc18)['height']) == (FW // 2, FH // 2)
        b18['image_size'] = reply
        # (Arbitrary rotation refuses since phase 2b: B21.) The Crop tool's straighten inside
        # the canvas: binary at its size.
        doc18c = bitmap_copy('field18c.png', FIELD)
        reply, _ = one_step(doc18c, 'crop', x=2, y=2, width=20, height=8, angle=5)
        assert (summary(doc18c)['width'], summary(doc18c)['height']) == (20, 8), summary(doc18c)
        b18['crop angle 5'] = reply
        # Move carries the bits; vacated area is white. (Free Transform, Transform Again,
        # Distort and the selected pixels' transforms refuse since phase 2b: B19.)
        doc18m = bitmap_copy('field18m.png', FIELD)
        px = bits(doc18m)
        one_step(doc18m, 'move_layers', layers=[0], dx=3, dy=2)
        px = moved_bits(FW, FH, px, FW, FH, lambda x, y: (x - 3, y - 2), 1)
        assert bits(doc18m) == px, 'move_layers'
        # The selected pixels: a whole-pixel move carries them, the hole white.
        doc18s = bitmap_copy('field18s.png', FIELD)
        px = bits(doc18s)
        call('select_rect', document_id=doc18s, x=2, y=2, width=6, height=4)
        inside = lambda x, y: 2 <= x < 8 and 2 <= y < 6
        one_step(doc18s, 'move_selected_pixels', layer=0, dx=4, dy=0)
        moved = [px[y * FW + x - 4] if x >= 4 and inside(x - 4, y)
                 else 1 if inside(x, y) else px[y * FW + x]
                 for y in range(FH) for x in range(FW)]
        assert bits(doc18s) == moved, 'move_selected_pixels'
        call('deselect', document_id=doc18s)
        # Inverse, Modify and Transform Selection change the selection only.
        doc18q = bitmap_copy('field18q.png', FIELD)
        held = bits(doc18q)
        call('select_rect', document_id=doc18q, x=2, y=2, width=6, height=4)
        call('modify_selection', document_id=doc18q, operation='invert')
        assert bounds(doc18q) == {'x': 0, 'y': 0, 'width': FW, 'height': FH}, bounds(doc18q)
        call('modify_selection', document_id=doc18q, operation='invert')
        assert bounds(doc18q) == {'x': 2, 'y': 2, 'width': 6, 'height': 4}, bounds(doc18q)
        call('modify_selection', document_id=doc18q, operation='grow', radius=1)
        assert bounds(doc18q) == {'x': 1, 'y': 1, 'width': 8, 'height': 6}, bounds(doc18q)
        call('transform_selection', document_id=doc18q, translate_x=3)
        assert bounds(doc18q) == {'x': 4, 'y': 1, 'width': 8, 'height': 6}, bounds(doc18q)
        assert bits(doc18q) == held, 'the selection edits move no bit'
        call('deselect', document_id=doc18q)
        # Cut whitens exactly the selection, one step; undo restores.
        doc18x = bitmap_copy('field18x.png', FIELD)
        px = bits(doc18x)
        call('select_rect', document_id=doc18x, x=2, y=2, width=8, height=6)
        reply, after = one_step(doc18x, 'cut', layer=0)
        cut_inside = lambda x, y: 2 <= x < 10 and 2 <= y < 8
        assert after == [1 if cut_inside(i % FW, i // FW) else b for i, b in enumerate(px)], 'cut'
        call('undo', document_id=doc18x)
        assert bits(doc18x) == px, 'cut undo'
        call('deselect', document_id=doc18x)
        b18['cut'] = reply
        # The refusals left.
        refused = {
            'select_magic_wand': call('select_magic_wand', fail=True, document_id=doc18x, x=1,
                                      y=1, tolerance=0),
            'move_layers duplicate': call('move_layers', fail=True, document_id=doc18x,
                                          layers=[0], dx=1, dy=0, duplicate=True),
            'transform_layers duplicate': call('transform_layers', fail=True, document_id=doc18x,
                                               layers=[0], matrix=[1, 0, 0, 1, 2, 0],
                                               duplicate=True),
        }
        assert refused['select_magic_wand'] == DISABLED.format('select_magic_wand'), refused
        assert refused['move_layers duplicate'] == COPY, refused
        assert refused['transform_layers duplicate'] == DISABLED.format('transform_layers'), \
            refused
        b18['refusals'] = refused
        # Phase 3: Quick Selection is admitted and changes only the selection.
        held = bits(doc18x)
        quick = call('quick_select', document_id=doc18x, seeds=[[3, 6]])
        assert bounds(doc18x) is not None, ('quick_select selects', quick)
        assert bits(doc18x) == held and mode(doc18x) == 'bitmap', 'quick_select moves no bit'
        call('deselect', document_id=doc18x)
        b18['quick_select'] = quick
        receipts['b18'] = b18
        print('PASS b18: Image Size, a straightened crop, Move, the selected '
              'pixels\' move, Inverse, Modify and Transform Selection and Cut land in black and '
              'white, one step each; the Magic Wand and a copy refuse; Quick Selection selects '
              'and moves no bit', flush=True)

        # ---- B19: phase 2b's menu walk, applied --------------------------------------------
        def answer(doc, tool, **arguments):
            """(isError, text) of one call, whichever it is."""
            result = app.client.rpc('tools/call', {'name': tool,
                                                   'arguments': dict(document_id=doc, **arguments)})
            text = next((c['text'] for c in result.get('content', []) if c['type'] == 'text'), '')
            return bool(result.get('isError')), text

        def bitmap_refusal(text):
            return any(fragment in text for fragment in BITMAP_REFUSALS)

        doc19 = bitmap_copy('field19.png', FIELD)
        held = bits(doc19)
        b19 = {}
        for tool in WALK_DISABLED:
            b19[tool] = call(tool, fail=True, document_id=doc19)
        wrong = {t: s for t, s in b19.items() if s != DISABLED.format(t)}
        # Phase 3: each tool the walk left to it is admitted (its answer is no Bitmap refusal,
        # with document_id alone it may fail by its own arguments) or refuses for good.
        for tool in WALK_PHASE_3:
            failed, text = answer(doc19, tool)
            b19[tool] = text
            if tool in PHASE3_TOOLS:
                if bitmap_refusal(text):
                    wrong[tool] = text
            elif not failed or text != for_good(tool):
                wrong[tool] = text
        assert not wrong, wrong
        assert all(t in WALK_DISABLED for t in WALK_CORRECTED)
        assert bits(doc19) == held and mode(doc19) == 'bitmap', 'the refusals move no bit'
        # `affine` and the other kept_shared ops still serve Move.
        px = bits(doc19)
        one_step(doc19, 'move_layers', layers=[0], dx=-2, dy=1)
        px = moved_bits(FW, FH, px, FW, FH, lambda x, y: (x + 2, y - 1), 1)
        assert bits(doc19) == px, 'move_layers after the walk'
        receipts['b19'] = b19
        print(f'PASS b19: the walk\'s {len(WALK_DISABLED)} permanent tools refuse by their '
              f'sentences (the corrected admissions {", ".join(WALK_CORRECTED)} among them), its '
              f'{len(WALK_PHASE_3)} phase-3 tools are admitted or refuse for good; Move still '
              f'moves', flush=True)

        # ---- B20: Darken and Lighten paint their predicted result -----------------------
        doc20 = bitmap_copy('field20.png', FIELD)
        px = bits(doc20)
        call('select_rect', document_id=doc20, x=8, y=2, width=8, height=8)
        inside20 = lambda x, y: 8 <= x < 16 and 2 <= y < 10
        one_step(doc20, 'fill_selection', layer=0, color='#000000', blend_mode='darken')
        darkened = [0 if inside20(i % FW, i // FW) else b for i, b in enumerate(px)]
        assert bits(doc20) == darkened, 'darken black'
        steps = undo_count(doc20)
        lighten = call('fill_selection', document_id=doc20, layer=0, color='#000000',
                       blend_mode='lighten')
        assert bits(doc20) == darkened and undo_count(doc20) == steps, ('lighten black', lighten)
        one_step(doc20, 'fill_selection', layer=0, color='#ffffff', blend_mode='lighten')
        lightened = [1 if inside20(i % FW, i // FW) else b for i, b in enumerate(darkened)]
        assert bits(doc20) == lightened, 'lighten white'
        call('deselect', document_id=doc20)
        receipts['b20'] = {'darken': darkened, 'lighten': lightened, 'lighten_black': lighten}
        print('PASS b20: a black Darken fill blackens the selection, a black Lighten moves no bit, '
              'a white Lighten whitens it', flush=True)

        # ---- B21: RULING-1's leaves: arbitrary rotation and the filters --------------------
        doc21 = bitmap_copy('field21.png', FIELD)
        held = bits(doc21)
        steps = undo_count(doc21)
        b21 = {
            'rotate 30': call('rotate', fail=True, document_id=doc21, degrees=30),
            'rotate 45 bicubic': call('rotate', fail=True, document_id=doc21, degrees=45,
                                      sampler='bicubic'),
            'apply_filter lens_blur': call('apply_filter', fail=True, document_id=doc21,
                                           filter='lens_blur'),
            'apply_filter blur': call('apply_filter', fail=True, document_id=doc21,
                                      filter='blur'),
            'depth_blur': call('depth_blur', fail=True, document_id=doc21),
        }
        assert b21['depth_blur'] == DISABLED.format('depth_blur'), b21
        assert b21['rotate 30'] == ARBITRARY and b21['rotate 45 bicubic'] == ARBITRARY, b21
        assert b21['apply_filter lens_blur'] == LENS_BLUR, b21
        assert b21['apply_filter blur'] == DISABLED.format('apply_filter'), b21
        assert bits(doc21) == held and undo_count(doc21) == steps, 'the refusals move no bit'
        # The quarter turn stays.
        one_step(doc21, 'rotate', degrees=-90)
        assert (summary(doc21)['width'], summary(doc21)['height']) == (FH, FW), summary(doc21)
        receipts['b21'] = b21
        print('PASS b21: arbitrary rotation, the filters and Depth Blur refuse for good, Lens Blur '
              'by its own sentence, and a quarter turn still runs', flush=True)

        # ---- Phase 3: the Gray twins and the threshold, by hand -------------------------
        def twin_copy(name, values=FIELD, w=FW, h=FH):
            """A fresh Bitmap copy converted back to Grayscale at Size Ratio 1."""
            doc = bitmap_copy(name, values, w, h)
            call('convert_mode', document_id=doc, mode='gray')
            assert mode(doc) == 'gray', summary(doc)
            return doc

        def thresholded(doc):
            """A twin's opaque render through 50% Threshold: white iff its code reaches 128."""
            return [1 if v >= 128 else 0 for v in codes(doc)]

        def segment_distance(x, y, a, b):
            (ax, ay), (bx, by) = a, b
            dx, dy = bx - ax, by - ay
            t = max(0.0, min(1.0, ((x - ax) * dx + (y - ay) * dy) / (dx * dx + dy * dy)))
            return math.hypot(x - (ax + t * dx), y - (ay + t * dy))

        def rle16(values):
            """`coverage_rle16`: runs of (count u32, gray u16, alpha u16), little-endian."""
            out, run = b'', None
            for v in values:
                if run and run[1] == v:
                    run[0] += 1
                    continue
                if run:
                    out += struct.pack('<IHH', run[0], run[1], run[1])
                run = [1, v]
            out += struct.pack('<IHH', run[0], run[1], run[1])
            return base64.b64encode(out).decode()

        # ---- B22: Copy; Copy Merged and Paste refuse ------------------------------------
        doc22 = bitmap_copy('field22.png', FIELD)
        held = bits(doc22)
        call('select_rect', document_id=doc22, x=2, y=2, width=6, height=4)
        b22 = {'copy': call('copy', document_id=doc22, layer=0),
               'copy_merged': call('copy_merged', fail=True, document_id=doc22),
               'paste': call('paste', fail=True, document_id=doc22)}
        assert b22['copy_merged'] == DISABLED.format('copy_merged'), b22
        assert b22['paste'] == DISABLED.format('paste'), b22
        assert bits(doc22) == held, 'copy moves no bit'
        call('deselect', document_id=doc22)
        receipts['b22'] = b22
        print('PASS b22: Copy copies the selection; Copy Merged and Paste refuse',
              flush=True)

        # ---- B23: Quick Mask edits the selection only -----------------------------------
        doc23 = bitmap_copy('field23.png', FIELD)
        held = bits(doc23)
        before = bounds(doc23)
        call('set_quick_mask', document_id=doc23, active=True)
        painted = [65535 if 4 <= i % FW < 12 and 3 <= i // FW < 8 else 0 for i in range(FW * FH)]
        call('quick_mask_stroke', document_id=doc23, width=FW, height=FH,
             coverage_rle16=rle16(painted))
        call('gradient', document_id=doc23, x0=0, y0=0, x1=FW, y1=0, start_color='#000000',
             end_color='#ffffff')
        assert bits(doc23) == held, 'Quick Mask moves no bit'
        call('set_quick_mask', document_id=doc23, active=False)
        after = bounds(doc23)
        assert after is not None and after != before, (before, after)
        assert bits(doc23) == held and mode(doc23) == 'bitmap', 'Quick Mask moves no bit'
        call('deselect', document_id=doc23)
        receipts['b23'] = {'bounds': after}
        print('PASS b23: Quick Mask on, a stroke, a Gradient and off change the selection only',
              flush=True)

        # ---- B24: Liquify is the Gray twin's thresholded --------------------------------
        strokes = [{'tool': 'forward_warp', 'size': 10, 'strength': 100,
                    'points': [[8, 6], [16, 6]]}]
        doc24 = bitmap_copy('field24.png', FIELD)
        reply, after = one_step(doc24, 'liquify', layer=0, strokes=strokes)
        twin24 = twin_copy('field24t.png')
        call('liquify', document_id=twin24, layer=0, strokes=strokes)
        assert after == thresholded(twin24), (after, thresholded(twin24))
        receipts['b24'] = reply
        print('PASS b24: Liquify warps the gray and thresholds it, one undo step', flush=True)

        # ---- B25: the Clone Stamp and the History Brush ---------------------------------
        line = ([14, 6], [20, 6])
        clone = dict(layer=0, source_x=4, source_y=6, points=list(line), size=5, hardness=100)
        doc25 = bitmap_copy('field25.png', FIELD)
        px = bits(doc25)
        reply, after = one_step(doc25, 'clone_stamp', **clone)
        twin25 = twin_copy('field25t.png')
        call('clone_stamp', document_id=twin25, **clone)
        assert after == thresholded(twin25), (after, thresholded(twin25))
        for i, bit in enumerate(after):
            x, y = i % FW, i // FW
            # The pixel's centre: within 1 px of the stroke and not past its ends is the band's
            # full coverage; past 3.5 px the stroke cannot reach.
            d = segment_distance(x + 0.5, y + 0.5, *line)
            if d <= 1.0 and line[0][0] <= x + 0.5 <= line[1][0]:
                assert bit == px[y * FW + x - 10], ('the band copies the source bits', x, y)
            elif d > 3.5:
                assert bit == px[i], ('outside the band', x, y)
        b25 = {'clone': reply,
               'new_layer': call('clone_stamp', fail=True, document_id=doc25, output='new_layer',
                                 **clone),
               'multiply': call('clone_stamp', fail=True, document_id=doc25,
                                blend_mode='multiply', **clone)}
        assert b25['new_layer'] == DISABLED.format('add_layer'), b25
        assert b25['multiply'] == BLEND_REFUSAL, b25
        doc25h = bitmap_copy('field25h.png', FIELD)
        original = bits(doc25h)
        state = call('list_history', document_id=doc25h)['current_state_id']
        stroke25 = call('brush_stroke', document_id=doc25h, layer=0, color='#000000',
                        points=list(line), size=5)
        assert stroke25.get('changed') is True and bits(doc25h) != original, stroke25
        b25['history'] = call('history_brush_stroke', document_id=doc25h, layer=0,
                              points=list(line), size=9, hardness=100, opacity=100,
                              state_id=state)
        assert bits(doc25h) == original, 'the History Brush gives the earlier bits back'
        receipts['b25'] = b25
        print('PASS b25: the Clone Stamp copies bits (the twin\'s clone thresholded), refuses a '
              'new layer and Multiply; the History Brush gives the earlier bits back', flush=True)

        # ---- B26: Perspective Crop is the Gray twin's thresholded -----------------------
        corners = [[2, 1], [21, 2], [22, 10], [1, 9]]
        crop26 = dict(corners=corners, width=20, height=9, sampler='bilinear')
        doc26 = bitmap_copy('field26.png', FIELD)
        reply, after = one_step(doc26, 'perspective_crop', **crop26)
        twin26 = twin_copy('field26t.png')
        call('perspective_crop', document_id=twin26, **crop26)
        assert (summary(doc26)['width'], summary(doc26)['height']) == (20, 9), summary(doc26)
        assert after == thresholded(twin26), (after, thresholded(twin26))
        receipts['b26'] = reply
        print('PASS b26: Perspective Crop resamples the gray and thresholds it, one undo step',
              flush=True)

        # ---- B27: Straighten Layer is the straightened crop -----------------------------
        doc27 = bitmap_copy('field27.png', FIELD)
        twin27 = twin_copy('field27t.png')
        fit = call('fit_crop', document_id=twin27, line=[0, 0, 60, 4])
        call('crop', document_id=twin27, x=fit['x'], y=fit['y'], width=fit['width'],
             height=fit['height'], angle=fit['angle'])
        reply, after = one_step(doc27, 'straighten_layers', x0=0, y0=0, x1=60, y1=4)
        assert abs(reply['rotation'] - fit['angle']) < 1e-9, (reply, fit)
        assert (summary(doc27)['width'], summary(doc27)['height']) == \
            (fit['width'], fit['height']), (summary(doc27), fit)
        assert after == thresholded(twin27), (after, thresholded(twin27))
        receipts['b27'] = {'reply': reply, 'fit_crop': fit}
        print('PASS b27: Straighten Layer crops at fit_crop\'s box and angle, the twin\'s crop '
              'thresholded, one undo step', flush=True)

        # ---- B28: the layer row ---------------------------------------------------------
        doc28 = bitmap_copy('field28.png', FIELD)
        held = bits(doc28)
        call('set_layer_properties', document_id=doc28, index=0, name='Plate')
        call('set_layer_label', document_id=doc28, layer=0, label='red')
        call('set_layer_lock', document_id=doc28, layer=0, locks=['pixels'])
        b28 = {'locked brush': call('brush_stroke', fail=True, document_id=doc28,
                                    color='#ffffff', **stroke),
               'opacity': call('set_layer_properties', fail=True, document_id=doc28, index=0,
                               opacity=0.5)}
        assert not bitmap_refusal(b28['locked brush']), b28
        assert b28['opacity'] == LAYER_PROPERTIES, b28
        assert bits(doc28) == held and mode(doc28) == 'bitmap', 'the layer row moves no bit'
        rz28 = folder / 'row.rz'
        call('save_copy', document_id=doc28, path=str(rz28), format='rz')
        back28 = call('open_document', path=str(rz28))['document']['id']
        row = call('get_document', document_id=back28)['layers'][0]
        assert (row['name'], row['label']) == ('Plate', 'red'), row
        assert mode(back28) == 'bitmap' and bits(back28) == held, summary(back28)
        receipts['b28'] = b28
        print('PASS b28: rename, label and lock keep the plane and survive .rz; the lock refuses '
              'the Brush; opacity refuses', flush=True)

        # ---- B29: the render channels ---------------------------------------------------
        def render_px(doc, **arguments):
            result = app.client.rpc('tools/call', {'name': 'render', 'arguments': dict(
                document_id=doc, **arguments)})
            assert not result.get('isError'), result
            data = next(c['data'] for c in result['content'] if c['type'] == 'image')
            return png_pixels(base64.b64decode(data))

        doc29 = bitmap_copy('field29.png', FIELD)
        composite = render_px(doc29)
        assert render_px(doc29, channel='bitmap') == composite, 'channel bitmap'
        assert render_px(doc29, channel='composite') == composite, 'channel composite'
        b29 = {'red': call('render', fail=True, document_id=doc29, channel='red'),
               'bitmap layer': call('render', fail=True, document_id=doc29, channel='bitmap',
                                    layer=0)}
        assert b29['red'] == RENDER_REFUSAL and b29['bitmap layer'] == RENDER_REFUSAL, b29
        receipts['b29'] = b29
        print('PASS b29: the bitmap and composite channels render the composite; red and a '
              'channel with a layer refuse', flush=True)

        # ---- B30: phase 3's for-good refusals -------------------------------------------
        doc30 = bitmap_copy('field30.png', FIELD)
        held = bits(doc30)
        steps = undo_count(doc30)
        b30 = {}
        for tool in (PHASE3_DISABLED + PHASE3_ONE_LAYER + PHASE3_CHANNEL + ['save_selection']
                     + PHASE3_UNOFFERED):
            b30[tool] = call(tool, fail=True, document_id=doc30)
        wrong = {t: s for t, s in b30.items() if s != for_good(t)}
        assert not wrong, wrong
        assert bits(doc30) == held and undo_count(doc30) == steps, 'the refusals move no bit'
        receipts['b30'] = b30
        print(f'PASS b30: {len(b30)} tools refuse for good by their tier\'s exact sentence',
              flush=True)

        # ---- B32: the Gradient is the Gray twin's ramp thresholded ----------------------
        ramp = dict(layer=0, x0=0, y0=0, x1=FW, y1=0, start_color='#000000',
                    end_color='#ffffff')
        doc32 = bitmap_copy('field32.png', FIELD)
        reply, after = one_step(doc32, 'gradient', **ramp)
        twin32 = twin_copy('field32t.png')
        call('gradient', document_id=twin32, **ramp)
        assert after == thresholded(twin32), (after, thresholded(twin32))
        white32 = bitmap_copy('white32.png', [255] * (FW * FH))
        steps = undo_count(white32)
        faint = call('gradient', document_id=white32, opacity=0.4, **ramp)
        assert faint.get('changed') is False, faint
        assert bits(white32) == [1] * (FW * FH) and undo_count(white32) == steps, 'opacity 0.4'
        receipts['b32'] = {'gradient': reply, 'opacity 0.4': faint}
        print('PASS b32: the Gradient is the twin\'s ramp thresholded, one undo step; at 0.4 over '
              'white it changes nothing', flush=True)

        # ---- B33: the close's catalog sweep ----------------------------------------------
        declared33 = declared_document_free()
        assert declared33 == DOCUMENT_FREE, (sorted(declared33 - DOCUMENT_FREE),
                                             sorted(DOCUMENT_FREE - declared33))
        doc33 = bitmap_copy('field33.png', FIELD)
        held = bits(doc33)
        steps = undo_count(doc33)
        listed33 = sorted(t['name'] for t in app.client.rpc('tools/list', {})['tools'])
        counts33 = {'disabled': 0, 'one_layer': 0, 'channel': 0, 'save_selection': 0,
                    'unoffered': 0}
        mismatched33, phased33 = [], []
        for tool in listed33:
            if tool in BITMAP_TOOLS or tool in DOCUMENT_FREE:
                continue
            failed, text = answer(doc33, tool)
            if 'phase' in text.lower():
                phased33.append((tool, text))
            if tool in DISABLED_ALL:
                kind, want = 'disabled', DISABLED.format(tool)
            elif tool in ONE_LAYER_ALL:
                kind, want = 'one_layer', ONE_LAYER.format(tool)
            elif tool in CHANNEL_ALL:
                kind, want = 'channel', CHANNEL.format(tool)
            elif tool == 'save_selection':
                kind, want = 'save_selection', SAVE_SELECTION
            else:
                kind, want = 'unoffered', UNOFFERED.format(tool)
            if not failed or text != want:
                mismatched33.append((tool, failed, text[:200]))
            counts33[kind] += 1
        assert not mismatched33, mismatched33
        assert not phased33, phased33
        precision33 = call('get_precision', document_id=doc33)
        phase_keys33 = [k for k in precision33 if 'phase' in k.lower()]
        assert not phase_keys33, phase_keys33
        assert mode(doc33) == 'bitmap' and bits(doc33) == held, 'the sweep moves no bit'
        assert undo_count(doc33) == steps, (undo_count(doc33), steps)
        receipts['b33'] = {'listed': len(listed33), 'counts': counts33}
        print(f'PASS b33: of {len(listed33)} catalog tools, {counts33["disabled"]} are not '
              f'available in Bitmap mode, {counts33["one_layer"]} need more than one layer, '
              f'{counts33["channel"]} need an alpha channel, {counts33["save_selection"]} is Save '
              f'Selection and {counts33["unoffered"]} are offered on Grayscale and colour only; '
              'no reply and no get_precision key names a phase', flush=True)

        # ---- B31: the admitted tools with arguments (last: they act) ---------------------
        doc31 = bitmap_copy('field31.png', FIELD)
        work = folder.resolve()
        b31, twins31, wrong31 = [], [], []

        def reach(tool, twin_setup=(), **arguments):
            """One B31 call on doc31: its reply (parsed JSON, else the raw result) and the raw
            result. No answer holds a Bitmap refusal; an error must be the same call's error on
            a fresh Gray twin, word for word (recorded), and then the reply is None. The twin
            first takes `twin_setup`, the (tool, arguments) calls the error depends on."""
            result = app.client.rpc('tools/call', {'name': tool, 'arguments': dict(
                document_id=doc31, **arguments)})
            text = next((c['text'] for c in result.get('content', []) if c['type'] == 'text'),
                        '') or ''
            if bitmap_refusal(text):
                wrong31.append((tool, text))
            if result.get('isError'):
                twin = twin_copy(f'field31-twin{len(twins31)}.png')
                for setup_tool, setup_arguments in twin_setup:
                    call(setup_tool, document_id=twin, **setup_arguments)
                twin_arguments = {k: ([twin] if v == [doc31] else v) for k, v in arguments.items()}
                twin_failed, twin_text = answer(twin, tool, **twin_arguments)
                if not (twin_failed and twin_text == text):
                    wrong31.append((tool, text, twin_text))
                twins31.append({'tool': tool, 'text': text})
                b31.append({'tool': tool, 'error': text[:300]})
                return None, result
            b31.append({'tool': tool, 'reply': text[:300] if text else 'image'})
            try:
                return json.loads(text), result
            except (json.JSONDecodeError, TypeError):
                return result, result

        def documents_count():
            return len(call('list_documents')['documents'])

        reply, _ = reach('set_resolution', ppi_x=144)
        if reply is not None:
            info = call('get_document', document_id=doc31)
            assert (info['resolution']['ppi_x'], info['resolution']['ppi_y']) == (144, 144), \
                info['resolution']
        reply, _ = reach('set_file_info', copyright='Bitmap close 547')
        if reply is not None:
            echoed, _ = reach('get_file_info')
            assert echoed is not None and echoed['copyright'] == 'Bitmap close 547', echoed
        reach('set_ruler_origin', x=2, y=3)
        reach('auto_select_layer', x=3, y=3)
        reach('set_history_budget', bytes=268435456)
        reach('fit_crop', line=[0, 0, 60, 4])
        reach('select_magnetic', points=[[2, 2], [20, 2], [20, 9], [2, 9]], close='straight')
        reach('deselect')
        reply, _ = reach('set_color_samplers', samplers=[{'x': 3, 'y': 3}, {'x': 18, 'y': 8}])
        if reply is not None:
            rows, _ = reach('get_color_samplers')
            assert rows is not None and len(rows['samplers']) == 2, rows
        # Paths.
        box = {'closed': True, 'anchors': [[4, 4], [20, 4], [20, 8], [4, 8]]}
        reach('set_path', name='Box', subpaths=[box])
        reach('rename_path', path='Box', name='Frame')
        reach('duplicate_path', path='Frame', name='Frame copy')
        selected, _ = reach('path_to_selection', path='Frame')
        assert selected is None or selected['selection_empty'] is False, selected
        reach('selection_to_path')
        reach('deselect')
        reach('fill_path', path='Frame', color='#000000', layer=0)
        reach('stroke_path', path='Frame', color='#ffffff', size=2, layer=0)
        reach('delete_path', path='Frame copy')
        # Snapshots: the id is the new list_snapshots entry (`phase5_snapshots.new_snapshot`).
        known = {e['snapshot_id'] for e in call('list_snapshots', document_id=doc31)['snapshots']}
        reach('create_snapshot', name='B31')
        added = [e['snapshot_id'] for e in call('list_snapshots', document_id=doc31)['snapshots']
                 if e['snapshot_id'] not in known]
        assert len(added) == 1, added
        snapshot = added[0]
        package = work / 'b31.drsnapshot'
        reach('rename_snapshot', snapshot_id=snapshot, name='B31 renamed')
        reach('save_snapshot', snapshot_id=snapshot, path=str(package))
        reach('load_snapshot', path=str(package))
        reach('restore_snapshot', snapshot_id=snapshot)
        reach('delete_snapshot', snapshot_id=snapshot)
        # Views and workspaces.
        opened, _ = reach('open_second_view')
        assert opened is None or opened['view']['view'] == 2, opened
        reach('set_view', view=2, zoom=1)
        reach('get_view', view=2)
        reach('close_second_view', view=2)
        reach('save_workspace', name='Bitmap close')
        reach('restore_workspace', name='Bitmap close')
        # Proofs.
        proof_setup = {'profile': 'working_cmyk'}
        reach('set_proof_setup', **proof_setup)
        proof, raw = reach('render_proof', profile='working_cmyk')
        assert proof is None or any(c['type'] == 'image' for c in raw.get('content', [])), raw
        # A preset keeps a file's profile: the twin takes the same proof setup first.
        reach('save_proof_preset', twin_setup=[('set_proof_setup', proof_setup)],
              name='Bitmap close')
        # PDF.
        pdf = work / 'b31.pdf'
        reply, _ = reach('export_pdf', path=str(pdf))
        assert reply is None or pdf.read_bytes()[:4] == b'%PDF', pdf
        # The contact sheets, each a new document.
        before = documents_count()
        reply, _ = reach('contact_sheet', items='all', output={'kind': 'document'})
        assert reply is None or documents_count() == before + 1, reply
        before = documents_count()
        reply, _ = reach('contact_sheet_from_sources', documents=[doc31],
                         output={'kind': 'document'})
        assert reply is None or documents_count() == before + 1, reply
        # An Action of one horizontal flip mirrors the plane in one step.
        reach('save_action', name='bitmap-close-547', overwrite=True,
              steps=[{'tool': 'flip', 'arguments': {'axis': 'horizontal'}}])
        plane, steps = bits(doc31), undo_count(doc31)
        w31 = summary(doc31)['width']
        reply, _ = reach('run_action', name='bitmap-close-547')
        if reply is not None:
            mirrored = [plane[(i // w31) * w31 + w31 - 1 - i % w31] for i in range(len(plane))]
            assert bits(doc31) == mirrored, 'run_action mirrors the plane'
            assert undo_count(doc31) == steps + 1, (undo_count(doc31), steps)
        # Every other tool phase 3 admits keeps its document_id-alone call.
        argued = {entry['tool'] for entry in b31}
        for tool in PHASE3_TOOLS:
            if tool in argued:
                continue
            failed, text = answer(doc31, tool)
            b31.append({'tool': tool, 'alone': True, 'error': failed, 'text': text[:300]})
            if bitmap_refusal(text):
                wrong31.append((tool, text))
        assert not wrong31, wrong31
        assert mode(doc31) == 'bitmap', summary(doc31)
        bits(doc31)
        receipts['b31'] = {'calls': b31, 'twin_matches': twins31}
        print(f'PASS b31: {len(argued)} admitted tools reach the core with arguments '
              f'({len(twins31)} errors match the Gray twin word for word), the other '
              f'{len(b31) - sum(1 for e in b31 if e["tool"] in argued)} answer no Bitmap '
              'refusal, and the document stays Bitmap', flush=True)

    (output / 'receipts.json').write_text(json.dumps(receipts, indent=2, default=str))
    print('ALL GATES PASS', flush=True)


if __name__ == '__main__':
    main()
