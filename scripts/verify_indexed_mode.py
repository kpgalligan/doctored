#!/usr/bin/env python3
"""The #1 Indexed Color phase-1a, 1b, 1c, 2a, 2b, 2c, 2d, 3a, 3b and 3c MCP drive: indexed PNG and GIF files
open as Indexed Color documents, an 8-bit RGB document converts to Indexed Color
in one undo step and back to RGB exactly, an Indexed document converts to
Grayscale, `.rz` reopens it, `get_document` reports its palette size, and what
the mode does not hold refuses with its sentence; phase 1b's Adaptive palette,
its PNG, GIF and TIFF export and save keeping the palette, and the save losses
`get_document` reports; phase 1c's colour table (`get_color_table`,
`set_color_table`, `.act` files), the Custom and Previous palettes, and Indexed →
CMYK and Lab Color directly; phase 2a's refusals as take1 measured them
(`scripts/photoshop-probe/indexed_mode_take.py`); phase 2b's Auto
refusals and the Brush, re-indexed onto the palette; phase 2c's Fill, Stroke,
Eraser, Clear, clipboard and geometry (I27–I33); phase 2d's Paste, transforms,
Warp, layer row and Export Layers (I34–I39); phase 3a's Move tool, selected
pixels, background rule, layer row, one-layer refusals and profile commands
(I40–I45); phase 3b's other blend modes, Select menu, background colour over
MCP and for-good refusals (I46–I50); phase 3c's dithers, Forced Custom, render
channels, selections, channel commands, Clone Stamp, History Brush and the
decided tools (I51–I58), the close's catalog sweep (I59), and the Indexed Color PSD and PSB
(I60), on the Debug app.

The gates are FIXED here BEFORE anything is measured; a failed gate IS the
receipt and no bound is widened afterwards. The instance is owned by this file
(`phase5_support.App` on its own scratch MCP socket: a dummy API key and a
PRIVATE directory for every `DR_*_DIR`, through `verification_isolation`'s
`preset_environment` list) and is terminated by its own context manager; no
Doctored process this file did not start is signalled. It never launches
Photoshop. Every fixture is written HERE in pure Python under `--output`.

- **I1 palette PNG.** An 8-bit palette PNG (4 × 2, five entries with one
  duplicate, a tRNS whose entry 2 is clear) opens as `indexed`, one layer;
  `get_document`'s `indexed` is `{colors 5, transparent_index 2}`; `render`
  is each opaque pixel's palette entry exactly and alpha 0 at index 2.
- **I2 GIF.** An 8-bit RGB document saved by `save_copy gif` reopens as
  `indexed`, one layer, 2 to 256 colours.
- **I3 RGB → Indexed.** An 8-bit RGB patch PNG (four colours over six pixels)
  converts with `palette exact`, `dither none`: the reply's `indexed.colors`
  is 4, `render` equals the source exactly; undo answers rgb, redo indexed
  (one undo step).
- **I4 back to RGB.** `convert_mode rgb` answers rgb at `rgba8` and `render`
  equals the source exactly; undo answers indexed.
- **I5 Grayscale.** `convert_mode gray` on the Indexed document answers gray at
  depth 8; undo answers indexed.
- **I6 .rz.** `save_copy rz` of I3's document reopens as indexed with the same
  `indexed` and a `render` equal to the source exactly.
- **I7 refusals.** `set_layer_lock` all, then none, answers ok (phase 2d);
  exactly: `flatten_image` →
  `ONE_LAYER` (since phase 3a); `duplicate_layer` (disabled in Indexed Color, take1)
  and `new_layer` → `DISABLED`; `save_copy jpeg` → `SAVE_REFUSAL`
  (`convert_mode cmyk`, refused here until phase 1c, is I20's); on an 8-bit
  RGB document `palette
  uniform colors 7` → `UNIFORM_7`; a 16-bit RGB document → `EIGHT_BITS`; an
  8-bit RGB document of two layers → `FLATTEN`, then `flatten true` converts to
  one layer.
- **I8 get_document.** I3's document's `get_document` answers `color_mode`
  indexed and `indexed.colors` 4, the `convert_mode` reply's.
- **I9 Adaptive.** A fresh copy of I3's patches converts with `palette
  adaptive`, `colors 2`, `dither none` to `ADAPTIVE_2` (median cut by hand: the
  four colours in key order split on r at k = 2, each box's count-weighted
  mean rounded half up), its indices `ADAPTIVE_INDICES`.
- **I10 PNG.** `export_image png` of I1 writes a PNG whose `PLTE` is `PALETTE`,
  whose `tRNS` is clear at `CLEAR` only and whose `IDAT` indices (decoded here)
  are `INDICES`.
- **I11 GIF.** `save_copy gif` of I1 reopens as `indexed {colors 8,
  transparent_index 2}` with a `render` equal to I1's.
- **I12 TIFF.** `export_image tiff` of I1 reopens as `indexed {colors 256,
  transparent_index 2}` with a `render` equal to I1's.
- **I13 refusal.** `export_image png bit_depth 16` of I1 → `KEPT_DEPTH`;
  `export_image jpeg` succeeds at 8 bits.
- **I14 losses.** `get_document`'s `losses_by_format` on I1 is empty for png;
  after `add_guide`, png lists the `GUIDES_LINE` and gif lists it and
  "embedded color profile" (a typed document always carries one).
- **I15 get_color_table.** I1's table is `PALETTE` with `CLEAR`, count 5; with
  `path` it writes an `.act` whose bytes equal `act_bytes(PALETTE, 5, CLEAR)`
  (772 bytes, built by hand here).
- **I16 set_color_table.** I1's entry 0 → `ENTRY_0`, `transparent_index`
  null: `render` is each pixel's new entry exactly (index `CLEAR` now
  opaque); undo restores I1's table and `render`, redo reapplies it.
- **I17 .act load.** A hand-built 768-byte `.act` (`FULL_ACT`) loads as 256
  colours with no transparent index; a 772-byte file of count 3 refuses with
  `SHORT_3`.
- **I18 Custom.** A fresh copy of I3's patches converts with `palette custom`,
  `table CUSTOM`, `dither none`: `render` is each patch's nearest `CUSTOM`
  entry (least squared distance, ties lowest, computed here) and the table
  reads back as `CUSTOM`.
- **I19 Previous.** A fresh copy converts with `palette previous` to the
  table `CUSTOM`; after `palette adaptive colors 2` on a 2-pixel document
  holding a transparent pixel (one opaque entry left), `palette previous`
  refuses with `NO_PREVIOUS`.
- **I20 CMYK.** `convert_mode cmyk` of an Indexed copy of I1's file answers
  cmyk at depth 8 and equals `convert_mode rgb` then `cmyk` of a second copy:
  `render` equal, every pixel's `sample_color` `ink16` equal; undo answers
  indexed.
- **I21 Lab.** The same for `lab`: `render` and every pixel's `sample_color`
  `lab` equal; undo answers indexed.
- **I22 the measured table.** Every tool of take1's `build.tools.disabled`
  (`TAKE_DISABLED`) refuses with `DISABLED`; of its `build.tools.phase_3`
  (`TAKE_PHASE_3`), since phase 3b every tool is decided: `RULED_PERMANENT`
  (the four Auto items, read disabled; the controller's ruling of
  2026-10-22) and `hdr_toning` with `DISABLED`, `ONE_LAYER_TOOLS`
  (`delete_layer`, `flatten_image`) and `AUTO_TOOLS` (Auto-Align and
  Auto-Blend) with `ONE_LAYER`, the profile tools (phase 3a's, I45) and
  `SELECT_TOOLS` admitted (their answer with no arguments is no gate
  sentence, `GATE_MARKS`), on I3's Indexed document, called with its
  `document_id` alone (the gate answers before any argument is read). The
  lists are copied from `decided.json` here; this drive never reads the take.
- **I23 phase 2b's refusals.** `auto_tone`, `auto_contrast`, `auto_color` and
  `equalize` → `DISABLED`; `hdr_toning` → `HDR` (`DISABLED`, since phase
  3b; the Eraser is phase 2c's, I28).
- **I24 the Brush.** An Exact Indexed document of three bands (black, white,
  `BAND`), 30 × 12: `brush_stroke` rgba `INK` (normalised), size 7, along
  y 6 from x 3 to 27, answers changed; every `render` pixel is a
  `get_color_table` entry at alpha 255; every pixel more than 5 px from the
  segment is unchanged; the pixel at (15, 6) is the entry nearest `INK`
  (least squared distance, computed here); `color_mode` is still indexed.
- **I25 undo.** Undo restores the render from before the stroke; redo
  reapplies the stroke's render.
- **I26 .rz and the mode.** `save_copy rz` of I24's document reopens with an
  equal render; since phase 3b `brush_stroke` with `blend_mode` multiply
  along y 9 answers changed and its render is `reindexed` (the hand re-index,
  `BLENDS`, `K`) of the same stroke on the reopened copy converted to RGB
  Color.

Phase 2c (Fill, Stroke, the Paint Bucket, the Eraser, Clear, Copy, Cut and the
Image-menu geometry). `PIXEL` compares two render pixels: equal, or both clear.
The clipboard runs on a private pasteboard (`DR_PASTEBOARD_NAME` set here).

- **I27 Fill and Stroke.** A fresh copy of I24's three bands (no transparent
  index): `select_rect` (2, 2, 6, 6) and `fill_selection` `INK` → every
  pixel inside is the entry nearest `INK`, every pixel outside unchanged,
  every pixel a table entry; `stroke_selection` `#FFFFFF` width 2 inside
  answers changed, every pixel more than 3 px from the rectangle unchanged,
  every pixel a table entry; after `deselect`, `fill` at
  (25, 6) `#FFFFFF` → band 3 is the white entry; since phase 3b `fill`
  `#3C3C3C` `blend_mode` Multiply answers changed and its render is
  `reindexed` of the same fill on an RGB twin (`twin_of`: `.rz`, reopened,
  converted to RGB Color); undo restores the render.
- **I28 the Eraser and Clear.** A 30 × 12 RGBA PNG whose middle band is alpha
  0 converts Exact with Transparency (`transparent_index` 2):
  `eraser_stroke` size 7 along y 6 from x 3 to 27 → every pixel of y 6 from
  x 5 to 25 is clear, every pixel more than 5 px from the segment unchanged;
  `clear_selection` of (0, 0, 4, 4) → those pixels clear; on I27's document
  (no transparent index) `eraser_stroke` (since phase 3a) answers changed,
  every pixel of y 6 from x 5 to 25 is `BG` (the I27 table's entry nearest
  white, computed here) and every pixel more than 5 px from the segment
  unchanged.
- **I29 the clipboard.** On I28's document `copy` and `copy_merged` answer
  ok; `cut` of (22, 8, 4, 3) clears those pixels and undo restores the
  render; on I27's document `cut` of (2, 9, 4, 3) (pixels that are not `BG`
  yet) makes those pixels `BG` and leaves the rest.
- **I30 exact geometry.** A fresh copy of I28's document, each step against
  the render before it moved here: `rotate` 90 (its reply carries
  `working_memory_plan_bytes` > 0) is new (x, y) = old (y, h − 1 − x);
  `flip` horizontal; `crop` (4, 2, 10, 6), past the rotated canvas's right
  edge, clear there; `image_size` nearest 2× is new (x, y) = old (x // 2,
  y // 2). `canvas_size` +4 × +4 anchor center on I28's document: the new
  area clear, the old area equal; the same on I27's: the new area `BG`, the
  old area equal.
- **I31 resampling.** On I28's document `image_size` lanczos3 to 45 × 18,
  `rotate` 5 sampler bicubic and `perspective_crop` sampler bicubic: after
  each, every pixel is clear or a table entry at alpha 255, and `color_mode`
  is indexed.
- **I32 undo and .rz.** On I30's document `rotate` 90, then undo restores the
  render and redo reapplies it; `save_copy rz` (after the crop) reopens with
  an equal render.
- **I33 the decided ones.** (Since phase 3a.) `puppet_warp` → `DISABLED`,
  `align_layers` → `ONE_LAYER`, `reveal_canvas` answers changed false (the
  Index layer holds no pixel outside the canvas).

Phase 2d (Paste, the transforms, Warp, the layer row and Export Layers). Each
case runs on a fresh Exact copy of I28's file with Transparency, `ON_TABLE`
meaning every `render` pixel clear or a table entry at alpha 255, one layer.

- **I34 Paste.** An RGB patch (10 × 10, four quadrants) copied from its own
  document and pasted centered: `ON_TABLE`, the reply's `layer` 0 and
  `working_memory_plan_bytes` > 0, undo restores the render and redo
  reapplies it; a window (6, 3, 8, 5) copied from a twin Indexed document of
  the same palette and pasted `at` (15, 4) is the hand-pasted render (a clear
  source pixel keeps the destination); the patch pasted `into` and `outside`
  a selection (12, 3, 6, 6) changes only pixels inside, or only outside, it;
  `mode layers` → `DISABLED` of `paste_layers` (since phase 3a).
- **I35 the transforms.** `transform_layer translate_x 3 sampler nearest` is
  the render moved by hand (new area clear); the same on I27's document is
  its render moved by hand, the new area `BG`; `transform_layers` rotate 5° about the centre,
  bicubic, answers changed with `working_memory_plan_bytes` > 0 and
  `ON_TABLE`; `distort_layer` bicubic `ON_TABLE`; `transform_layers
  duplicate true` → `COPY`.
- **I36 selected pixels.** `select_rect` (6, 2, 6, 4) and
  `transform_selected_pixels` translate (4, 1) nearest is the render with the
  rectangle's hole clear and its opaque pixels moved by hand; `transform_again`
  repeats it over the reply's `selection_bounds`; `transform_again duplicate
  true` → `COPY`.
- **I37 Warp.** `warp_layer set` bicubic answers changed with `field` null and
  `ON_TABLE`; `get_warp`'s field is null; `clear` answers changed false, the
  render unchanged.
- **I38 the layer row.** `set_layer_properties name`, `set_layer_label red`,
  `set_layer_lock pixels` (then `brush_stroke` refuses naming the lock; unlocked
  again); `set_layer_properties opacity` → `PROPS`; the render unchanged;
  `save_copy rz` reopens with the name and label.
- **I39 Export Layers.** `export_layers` png of a fresh I1 file writes one
  file that reopens `indexed` with I1's render.

Phase 3a (the Move tool, the selected pixels' other targets, the background
rule, the layer row's visibility, offset and order, the one-layer refusals
and the profile commands).

- **I40 Move.** A fresh copy of I28's document: `move_layers` (3, 1) is the
  render moved by hand (new area clear, pixels past the edge gone), the layer
  offset (0, 0), and one undo step restores the render; `duplicate true` →
  `COPY`; `set_layer_properties offset_x 2` is the render moved by 2 by hand.
- **I41 selected pixels.** `select_rect` (6, 2, 6, 4) and
  `move_selected_pixels` (4, 1) is the render with the rectangle's hole clear
  and its opaque pixels moved by hand; `target red` → `INDEX_CHANNEL`;
  `transform_mask` → `DISABLED`.
- **I42 background.** On a fresh copy of I27's document (no transparent
  index): `clear_selection` of (0, 0, 4, 4) makes those pixels `BG`;
  `select_rect` (4, 2, 4, 4) (black) and `move_selected_pixels` (6, 4) leaves
  its hole `BG` and the rest moved by hand.
- **I43 row.** `set_layer_properties visible false` → every render pixel
  clear; `export_image png` profile document → `HIDDEN`; `save_copy rz`
  reopens hidden; `visible true` restores the render; `open true` → the
  groups-only sentence.
- **I44 one layer.** `distribute_layers`, `delete_layer` and `flatten_image`
  → `ONE_LAYER`; `hdr_toning` → `DISABLED` (since phase 3b).
- **I45 profiles.** `assign_profile` Display P3 keeps `get_color_table`,
  changes the profile, and undo restores it; `convert_profile` Display P3
  answers indexed with the same count and transparent index, a changed
  table, a reply `working_memory_plan_bytes` > 0, and undo restores the
  table; a profile that is not RGB → `HOST_NOT_RGB` on each tool (the host's
  `resolveProfile` refuses before the core).

Phase 3b (the Brush, Fill, Stroke and the Paint Bucket in every blend mode, the
Select menu, the background colour, and HDR Toning, Auto-Align and Auto-Blend
for good). Since phase 3c every tool refused by neither list is offered on RGB
documents only (`RGB_ONLY`), for good.

- **I46 modes.** On fresh Exact copies of I24's bands: `brush_stroke` Screen
  along the pixel centres of row 6 (x 3.5 to 26.5, y 6.5, so no edge pixel's
  coverage sits at one half) and `fill` (25, 6) `INK` Difference each answer
  changed, their
  render is `reindexed` of the same call on an RGB twin (`twin_of`), some
  pixel moved, and one undo restores the render.
- **I47 selections.** A 30 × 12 Exact document of black, white and black
  bands: `select_color_range` sampled at the white entry (`samples16`,
  tolerance 0, softness 0) selects exactly the white band (`get_selection`
  `points` over the canvas, `coverage16`); from a 1 × 1 rectangle at (0, 0)
  `select_similar` tolerance 0 contiguous selects the left band and not
  contiguous both black bands; `modify_selection grow 1` and
  `transform_selection translate_x 2` each change the selection and the
  render stays; `set_quick_mask` on, then off, leaves the render;
  `save_selection` of (2, 2, 5, 4), `deselect`, `load_selection` from the
  channel gives the same coverage; `load_selection from plane` red →
  `INDEX_CHANNEL`.
- **I48 background over MCP.** A fresh Exact copy of I24's bands (no
  transparent index): `crop` (−2, −1, w + 4, h + 2) with `background_color`
  `#3C9918` is the render moved by hand with the new area the entry nearest
  (60, 153, 24) (computed here, not white's); after undo `canvas_size` +4 ×
  +4 anchor center takes white's entry (MCP sends no swatch).
- **I49 for good.** `hdr_toning` → `DISABLED`; `auto_align_layers` and
  `auto_blend_layers` → `ONE_LAYER`.
- **I50 what named phase 3c.** Since phase 3c `select_magic_wand` at (1, 1)
  tolerance 0 selects (some `get_selection` coverage) and leaves the render;
  `render` channel red → `RENDER_REFUSAL`.

Phase 3c (the Pattern and Noise dithers, Forced Custom, `render`'s channels,
the Magic Wand, Quick Selection, the channel commands, Clone Stamp, the History
Brush and every other tool decided). Pattern and Noise are DECLARED candidates
(`indexed_dither`: `t` from the typed-out 8 × 8 Bayer matrix, `BAYER8`, or the
Murmur-style mix, `noise_t`; each pixel moved by `(t − ½)·SPREAD`, `SPREAD` 51,
then its nearest entry), kept by phase 3d's take (Pattern missed, Noise refused
through the descriptor).

- **I51 dithers.** A 16 × 8 RGB ramp converted `palette web` with `dither
  pattern` and with `dither noise` renders `ordered` (the oracle above) of its
  `get_color_table`, pixel for pixel; undo answers rgb and redo the same
  render. `amount` with `pattern` → `DIFFUSION_ONLY`, the document still RGB.
  A 16-grey ramp 17 codes apart converted `palette exact` with `pattern` and
  with `noise` has the table and render of `dither none`.
- **I52 Forced Custom.** `palette uniform colors 32` and `palette adaptive
  colors 16` with `forced custom forced_table FORCED` lead their tables with
  `FORCED`; `forced web` with a `forced_table` → `FORCED_TABLE_ONLY`.
- **I53 the unpublished palettes (phase 3d).** Each of `UNPUBLISHED_PALETTES` →
  `UNPUBLISHED` (refused for good: no candidate matched phase 3d's take).
- **I54 render channels.** On an Exact copy of I24's bands, `render`
  channel `index` and channel `composite` are the composite render's bytes;
  `save_selection` of (4, 3, 7, 5) as S54, then `render` channel S54 is 255
  inside and 0 outside; channel red → `RENDER_REFUSAL`.
- **I55 selections and channels.** On I47's split bands, `select_magic_wand`
  (0, 0) tolerance 0 not contiguous selects both black bands; `quick_select`
  seeds [[15, 5]] selects something; `add_channel`, `rename_channel`,
  `duplicate_channel`, `set_channel_options`, `invert_channel` and
  `delete_channel` each leave `render` and `get_color_table`, and
  `list_channels` counts +1, 0, +1, 0, 0, −1.
- **I56 Clone Stamp and the History Brush.** On an Exact copy of I24's bands,
  `clone_stamp` size 5, hardness 100, opacity 1 along (13, 6)–(17, 6) from
  (3, 6): every pixel a table entry, each pixel whose centre lies within 1 px
  of the segment is the source pixel 10 to its left, each beyond 4 px
  unchanged; one undo restores the render. `output new_layer` →
  `DISABLED` of `add_layer`, the render unchanged. `brush_stroke` then
  `history_brush_stroke` size 40, opacity 100 from the state before it
  (`list_history`'s `current_state_id`) restores the render.
- **I57 for good.** Each of `DISABLED_3C` → `DISABLED`, `ONE_LAYER_3C` →
  `ONE_LAYER`, `RGB_ONLY_3C` → `RGB_ONLY`.
- **I58 admitted.** Each of `ADMITTED_3C`, called with `document_id` alone on
  a fresh Exact copy, answers no `GATE_MARKS` sentence.
- **I59 the close's catalog sweep.** On a fresh Exact copy of I24's bands,
  every `tools/list` name outside `INDEXED_TOOLS`, `DOCUMENT_FREE` and
  `DUOTONE_TOOLS`, called with `document_id` alone, is an error whose text is
  exactly `DISABLED` of it when it is in `DISABLED_ALL`, `ONE_LAYER` when in
  `ONE_LAYER_ALL`, else `RGB_ONLY`; each of `DUOTONE_TOOLS` is an error
  (`DocumentPrecision.refusal` asks `DuotoneDocument.tools` before the Indexed
  gate); no reply text contains `phase` (any case), and no key of
  `get_precision` on the document does.
- **I60 the Indexed Color PSD (#1).** I1's palette PNG, opened afresh:
  `inspect_psd_export` format tiff is unavailable with `PSD_TIFF`; `export_psd`
  flattened writes a `.psd` and a `.psb` at `bit_depth` 8; `inspect_psd` of each
  answers `import_precision` indexed, `color_mode` Indexed, `layered_available`
  false and `stored_composite_available` true; `open_document` `psd_mode`
  composite with its fingerprint opens it as `indexed` with the same
  `get_color_table` (colours and transparent index), and its PNG export holds
  `PALETTE` and `INDICES` exactly.
"""

import argparse
import base64
import json
import math
import os
from pathlib import Path
import struct
import sys
import zlib

sys.dont_write_bytecode = True
ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'scripts'))

from phase5_support import App

# ---- the expectations, fixed here BEFORE anything is measured -------------
# Phase 3c: every tool the gate neither admits nor refuses with `DISABLED` or `ONE_LAYER` is
# offered on RGB documents only, for good (`IndexedDocument.rgbOnly`, the core's
# `refused_sentence`).
RGB_ONLY = ('{} is unavailable in an Indexed Color document: Doctored offers it on RGB '
            'documents only.')
DISABLED = '{} is not available in Indexed Color.'
# Phase 3b: HDR Toning refuses for good in Indexed Color.
HDR = DISABLED.format('hdr_toning')
# The host's render-channel sentence (`IndexedDocument.renderRefusal`, phase 3c).
RENDER_REFUSAL = ('An Indexed Color document renders its composite, its Index channel (index) or '
                  'an alpha channel by name, with no layer: it has no red, green, blue or mask '
                  'plane.')
DIFFUSION_ONLY = 'mode options amount and preserve_exact apply to Diffusion only'
FORCED_TABLE_ONLY = 'mode option forced_table applies to forced custom only'
UNPUBLISHED = ('The {} palette is not available: its algorithm is unpublished. Use Adaptive '
               '(median cut) instead.')
UNPUBLISHED_PALETTES = [('local_perceptual', 'Local Perceptual'),
                        ('local_selective', 'Local Selective'),
                        ('master_perceptual', 'Master Perceptual'),
                        ('master_selective', 'Master Selective')]
FORCED = [(12, 34, 56), (200, 10, 90)]
# The classic 8 × 8 Bayer matrix (row y, column x) and the declared spread (`indexed_dither`).
BAYER8 = [[0, 32, 8, 40, 2, 34, 10, 42], [48, 16, 56, 24, 50, 18, 58, 26],
          [12, 44, 4, 36, 14, 46, 6, 38], [60, 28, 52, 20, 62, 30, 54, 22],
          [3, 35, 11, 43, 1, 33, 9, 41], [51, 19, 59, 27, 49, 17, 57, 25],
          [15, 47, 7, 39, 13, 45, 5, 37], [63, 31, 55, 23, 61, 29, 53, 21]]
SPREAD = 51.0
# Phase 3c's for-good additions (`IndexedDocument.disabledTools`, `oneLayer`).
DISABLED_3C = [
    'gradient', 'dodge_burn', 'heal_stroke', 'spot_heal_stroke', 'remove_stroke', 'patch_region',
    'content_aware_move', 'red_eye', 'red_eye_auto', 'blur_stroke', 'sharpen_stroke',
    'smudge_stroke', 'sponge_stroke', 'depth_blur', 'discard_content_aware_fill_preview',
    'generative_expand', 'generative_variations', 'select_generative_variation',
    'generate_variations', 'select_person_parts', 'object_finder_select', 'inspect_subjects',
    'select_subjects', 'select_people', 'inspect_faces', 'select_face_parts', 'object_select',
    'edit_text_layer', 'edit_shape_layer', 'edit_fill_layer', 'set_global_light',
    'save_style_preset', 'get_mask_properties', 'get_vector_mask', 'set_vector_mask_properties',
    'remove_vector_mask', 'place_image', 'add_live_photo_layer', 'copy_layers_to_document',
    'stack_mode', 'reduce_noise_from_stack', 'convert_precision',
]
ONE_LAYER_3C = ['link_layers', 'unlink_layers', 'arrange_layer', 'reorder_layers',
                'merge_layers_to_hdr']
# The Indexed Color PSD's TIFF refusal (`indexed_psd`, I60).
PSD_TIFF = ('An Indexed Color document writes an 8-bit Indexed PSD or PSB; a TIFF with layers '
            'cannot hold it. Save a Copy writes its palette TIFF.')
# `export_psd` left this list for `INDEXED_TOOLS` with the Indexed Color PSD (#1, I60).
RGB_ONLY_3C = ['redevelop_raw', 'fade', 'art_history_brush_stroke']
# Phase 3c's admitted additions (`IndexedDocument.tools`): with `document_id` alone each answers
# no gate sentence (I58).
ADMITTED_3C = [
    'histogram', 'set_resolution', 'get_metadata', 'get_file_info', 'set_file_info',
    'get_precision', 'set_ruler_origin', 'list_channels', 'add_channel', 'delete_channel',
    'duplicate_channel', 'rename_channel', 'set_channel_options', 'invert_channel',
    'select_magic_wand', 'quick_select', 'select_magnetic', 'clone_stamp',
    'history_brush_stroke', 'auto_select_layer', 'run_action', 'contact_sheet',
    'contact_sheet_from_sources', 'list_snapshots', 'create_snapshot', 'rename_snapshot',
    'delete_snapshot', 'restore_snapshot', 'save_snapshot', 'load_snapshot',
    'set_history_budget', 'recover_document', 'discard_recovery', 'get_proof_setup',
    'set_proof_setup', 'render_proof', 'save_proof_preset', 'export_pdf', 'find_layers',
    'list_layer_comps', 'get_view', 'set_view', 'save_workspace', 'restore_workspace',
    'open_second_view', 'close_second_view', 'list_views', 'get_color_samplers',
    'set_color_samplers', 'detect_horizon', 'straighten_layers', 'list_paths', 'set_path',
    'rename_path', 'duplicate_path', 'delete_path', 'path_to_selection', 'selection_to_path',
    'fill_path', 'stroke_path', 'list_lens_profiles', 'separate_color',
]
# The close (I59): `IndexedDocument.tools`, `disabledTools` and `oneLayer`, copied verbatim;
# every `.documentFree` tool of the `<prefix>ToolFacts` tables (`app/Sources/AgentServer+*.swift`),
# which the gate never asks; and Duotone's two tools, which `DocumentPrecision.refusal` answers
# before the Indexed gate.
INDEXED_TOOLS = {
    'convert_mode', 'get_color_profile', 'get_document', 'render', 'sample_color', 'sample_pixel',
    'set_active_layer', 'set_selected_layers', 'select_rect', 'select_ellipse', 'select_polygon',
    'select_freehand', 'select_all', 'deselect', 'reselect', 'get_selection', 'undo', 'redo',
    'save_copy', 'list_guides', 'add_guide', 'remove_guide', 'clear_guides', 'list_history',
    'restore_history', 'close_document', 'get_recovery_status', 'write_recovery',
    'export_image', 'export_preview', 'quick_export', 'get_color_table', 'set_color_table',
    'brush_stroke',
    'copy', 'copy_merged', 'cut', 'clear_selection', 'fill', 'fill_selection',
    'stroke_selection', 'eraser_stroke', 'image_size', 'canvas_size', 'trim', 'rotate', 'flip',
    'crop', 'perspective_crop', 'fit_crop',
    'paste', 'transform_layer', 'transform_layers', 'distort_layer', 'transform_selected_pixels',
    'transform_again', 'get_warp', 'warp_layer', 'set_layer_properties', 'set_layer_lock',
    'set_layer_label', 'export_layers',
    'move_layers', 'move_selected_pixels', 'reveal_canvas', 'reorder_layer', 'assign_profile',
    'convert_profile',
    'select_color_range', 'select_similar', 'modify_selection', 'transform_selection',
    'set_quick_mask', 'quick_mask_stroke', 'save_selection', 'load_selection',
    'histogram', 'set_resolution', 'get_metadata', 'get_file_info', 'set_file_info',
    'get_precision', 'set_ruler_origin', 'list_channels', 'add_channel', 'delete_channel',
    'duplicate_channel', 'rename_channel', 'set_channel_options', 'invert_channel',
    'select_magic_wand', 'quick_select', 'select_magnetic', 'clone_stamp',
    'history_brush_stroke', 'auto_select_layer', 'run_action', 'contact_sheet',
    'contact_sheet_from_sources', 'list_snapshots', 'create_snapshot', 'rename_snapshot',
    'delete_snapshot', 'restore_snapshot', 'save_snapshot', 'load_snapshot',
    'set_history_budget', 'recover_document', 'discard_recovery', 'get_proof_setup',
    'set_proof_setup', 'render_proof', 'save_proof_preset', 'export_pdf', 'find_layers',
    'list_layer_comps', 'get_view', 'set_view', 'save_workspace', 'restore_workspace',
    'open_second_view', 'close_second_view', 'list_views', 'get_color_samplers',
    'set_color_samplers', 'detect_horizon', 'straighten_layers', 'list_paths', 'set_path',
    'rename_path', 'duplicate_path', 'delete_path', 'path_to_selection', 'selection_to_path',
    'fill_path', 'stroke_path', 'set_clipping_path', 'list_lens_profiles', 'separate_color',
    'export_psd', 'inspect_psd_export',
}
DISABLED_ALL = {
    'new_layer', 'group_layers', 'ungroup_layers', 'merge_down', 'merge_visible',
    'stamp_visible', 'stamp_selected', 'merge_group', 'layer_via_copy', 'layer_via_cut',
    'add_layer_mask', 'remove_layer_mask', 'set_layer_mask_enabled', 'set_mask_properties',
    'set_layer_style', 'apply_style_preset', 'set_layer_clipped', 'add_adjustment_layer',
    'edit_adjustment_layer', 'apply_filter', 'add_text_layer', 'add_shape_layer',
    'add_fill_layer', 'rasterize_artwork', 'set_vector_mask',
    'apply_image', 'calculations', 'content_aware_fill', 'preview_content_aware_fill',
    'generative_fill', 'duplicate_layer', 'duplicate_to_new_document', 'perspective_warp',
    'sky_replacement', 'select_subject', 'select_sky', 'refine_selection', 'liquify',
    'super_zoom', 'jpeg_artifacts_removal', 'neural_colorize',
    'auto_tone', 'auto_contrast', 'auto_color', 'equalize',
    'puppet_warp', 'transform_mask', 'hdr_toning',
    'gradient', 'dodge_burn', 'heal_stroke', 'spot_heal_stroke', 'remove_stroke',
    'patch_region', 'content_aware_move', 'red_eye', 'red_eye_auto', 'blur_stroke',
    'sharpen_stroke', 'smudge_stroke', 'sponge_stroke', 'depth_blur',
    'discard_content_aware_fill_preview', 'generative_expand', 'generative_variations',
    'select_generative_variation', 'generate_variations', 'select_person_parts',
    'object_finder_select', 'inspect_subjects', 'select_subjects', 'select_people',
    'inspect_faces', 'select_face_parts', 'object_select', 'edit_text_layer',
    'edit_shape_layer', 'edit_fill_layer', 'set_global_light', 'save_style_preset',
    'get_mask_properties', 'get_vector_mask', 'set_vector_mask_properties',
    'remove_vector_mask', 'place_image', 'add_live_photo_layer', 'copy_layers_to_document',
    'stack_mode', 'reduce_noise_from_stack', 'convert_precision',
}
ONE_LAYER_ALL = {
    'align_layers', 'distribute_layers', 'delete_layer', 'flatten_image',
    'auto_align_layers', 'auto_blend_layers',
    'link_layers', 'unlink_layers', 'arrange_layer', 'reorder_layers', 'merge_layers_to_hdr',
}
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
    'export_style_presets', 'list_workspaces', 'delete_workspace', 'list_brush_presets',
    'save_brush_preset', 'delete_brush_preset', 'import_brush_presets', 'export_brush_presets',
    'list_patterns', 'delete_pattern', 'import_patterns', 'export_patterns',
    'import_hdr_toning_preset',
}
DUOTONE_TOOLS = ['get_duotone', 'set_duotone']
ONE_LAYER = ('{} needs more than one layer, and an Indexed Color document holds one.')
INDEX_CHANNEL = ('An Indexed Color document has one channel, Index: it has no '
                 'red, green, blue or alpha plane to edit; target the layer.')
HIDDEN = ('The Index layer is hidden: show it before exporting or saving a copy that keeps '
          'Indexed Color.')
# The host's `.notRGB` refusal (`AgentServer+Color.swift`'s `resolveProfile`), asked before the core.
HOST_NOT_RGB = 'Doctored documents are RGB'
GROUPS_ONLY = 'open applies to groups only — layer 0 is a raster layer.'
PROPS = ("An Indexed Color document's one layer takes no opacity or blend mode.")
# Phase 3a's profile tools (I45) and one-layer tools, out of take1's phase-3 list.
PROFILE_TOOLS = ['assign_profile', 'convert_profile']
ONE_LAYER_TOOLS = ['delete_layer', 'flatten_image']
# Phase 3b: Auto-Align and Auto-Blend need more than one layer; the Select menu's tools are
# admitted (I47).
AUTO_TOOLS = ['auto_align_layers', 'auto_blend_layers']
SELECT_TOOLS = ['load_selection', 'modify_selection', 'save_selection', 'select_color_range',
                'select_similar', 'set_quick_mask', 'transform_selection']
# The Indexed gate's sentences: an admitted tool's answer is none of them.
GATE_MARKS = ('until #1 Indexed phase', 'is not available in Indexed Color',
              'needs more than one layer, and an Indexed Color document holds one',
              'in an Indexed Color document: Doctored offers it on RGB documents only')
GENERIC_CMYK = Path('/System/Library/ColorSync/Profiles/Generic CMYK Profile.icc')
COPY = ('A transform copy makes a second layer, which an Indexed Color document cannot hold; '
        'with a selection, transform_selected_pixels with duplicate copies the selected pixels.')
# The four Auto items take1 read disabled, refused for good (the controller's 2026-10-22 ruling).
RULED_PERMANENT = ['auto_tone', 'auto_contrast', 'auto_color', 'equalize']
BANDS = [(0, 0, 0), (255, 255, 255), (190, 50, 50)]
BAND = BANDS[2]
INK = (220, 30, 30)
SAVE_REFUSAL = ('An Indexed Color document saves as .doctored, PNG, GIF or TIFF, keeping its palette; '
                'Export (export_image) writes other formats as RGB, or convert to RGB Color.')
KEPT_DEPTH = ('An Indexed Color export with profile document keeps the palette at 8 bits; choose '
              'bit_depth 8, or profile srgb, display-p3 or file to export RGB.')
GUIDES_LINE = 'saved channels, paths, colour samplers, guides and the ruler origin (flattened copy)'
ADAPTIVE_2 = [(11, 65, 119), (253, 83, 83)]
ADAPTIVE_INDICES = [1, 0, 1, 0, 1, 0]
UNIFORM_7 = 'The Uniform palette needs at least 8 colours with these options.'
EIGHT_BITS = ('Indexed Color converts an 8-bit RGB or Grayscale document; convert '
              'to 8 Bits/Channel first.')
FLATTEN = 'Indexed Color holds one layer: flatten the image.'
PALETTE = [(200, 30, 40), (10, 220, 30), (0, 0, 0), (10, 220, 30), (250, 250, 250)]
CLEAR = 2
INDICES = [0, 1, 2, 3, 4, 0, 1, 3]
PATCHES = [(255, 0, 0), (0, 128, 255), (255, 0, 0), (17, 34, 51), (250, 250, 250), (17, 34, 51)]
ENTRY_0 = (1, 2, 3)
FULL_ACT = [(i, (i * 7) & 255, 255 - i) for i in range(256)]
SHORT_3 = 'The colour table holds 3 colours, but the image uses index 4; it needs at least 5.'
CUSTOM = [(240, 10, 10), (20, 120, 240), (30, 30, 30)]
NO_PREVIOUS = ('There is no previous Indexed Color palette: convert a document to Indexed Color '
               'first.')
# take1's `build.tools` (`~/devel/doctored-fixtures/photoshop/indexed-mode-547/take1/decided.json`).
TAKE_DISABLED = [
    'add_adjustment_layer', 'add_fill_layer', 'add_layer_mask', 'apply_filter', 'apply_image',
    'apply_style_preset', 'calculations', 'content_aware_fill', 'duplicate_layer',
    'duplicate_to_new_document', 'edit_adjustment_layer', 'generative_fill', 'group_layers',
    'jpeg_artifacts_removal', 'layer_via_copy', 'layer_via_cut', 'liquify', 'merge_down',
    'merge_group', 'merge_visible', 'neural_colorize', 'new_layer', 'perspective_warp',
    'preview_content_aware_fill', 'rasterize_artwork', 'refine_selection', 'remove_layer_mask',
    'select_sky', 'select_subject', 'set_layer_clipped', 'set_layer_mask_enabled',
    'set_layer_style', 'set_mask_properties', 'set_vector_mask', 'sky_replacement',
    'stamp_visible', 'super_zoom', 'ungroup_layers']
TAKE_PHASE_3 = [
    'assign_profile', 'auto_align_layers', 'auto_blend_layers', 'auto_color', 'auto_contrast',
    'auto_tone', 'convert_profile', 'delete_layer', 'equalize', 'flatten_image', 'hdr_toning',
    'load_selection', 'modify_selection', 'save_selection', 'select_color_range',
    'select_similar', 'set_quick_mask', 'transform_selection']


def act_bytes(entries, count=None, transparent=None):
    """An Adobe Color Table: the entries padded with zeros to 768 bytes, then (when `count` is
    given) the big-endian count and transparent index (0xFFFF for none)."""
    body = bytes(v for e in entries for v in e).ljust(768, b'\0')
    if count is None:
        return body
    return body + struct.pack('>HH', count, 0xFFFF if transparent is None else transparent)


def nearest(table, rgb):
    """The index of `table`'s entry nearest `rgb`: least squared distance, ties to the lowest."""
    distances = [sum((a - b) ** 2 for a, b in zip(entry, rgb)) for entry in table]
    return distances.index(min(distances))


def half_up(v):
    """`f64::round` of a value at or above 0: half away from zero."""
    whole = int(v)
    return whole + (1 if v - whole >= 0.5 else 0)


def pattern_t(x, y):
    """Pattern's threshold at (x, y): `(2·M + 1)/128` of the typed-out Bayer matrix."""
    return (2 * BAYER8[y % 8][x % 8] + 1) / 128.0


def noise_t(x, y):
    """Noise's threshold at (x, y): a Murmur3-style mix of the coordinates, its top 24 bits over
    2²⁴ (re-implemented here from the rule's definition)."""
    mask = 0xFFFFFFFF
    h = (x * 0x9E3779B9 + y * 0x85EBCA6B) & mask
    h ^= h >> 16
    h = (h * 0x85EBCA6B) & mask
    h ^= h >> 13
    h = (h * 0xC2B2AE35) & mask
    h ^= h >> 16
    return (h >> 8) / 16777216.0


def ordered(table, pixels, width, threshold):
    """The declared ordered dither of opaque `pixels` onto `table`: each pixel moved by
    `(t − ½)·SPREAD` on r, g and b, clamped, rounded, then its nearest entry, as RGBA."""
    out = []
    for i, p in enumerate(pixels):
        offset = (threshold(i % width, i // width) - 0.5) * SPREAD
        moved = tuple(half_up(min(max(c + offset, 0.0), 255.0)) for c in p[:3])
        out.append(table[nearest(table, moved)] + (255,))
    return out


def code(v):
    """A unit value as an 8-bit code, rounded (half away from zero, as Rust's `round`)."""
    return int(math.floor(min(max(v, 0.0), 1.0) * 255.0 + 0.5))


# Phase 3b: each mode's full-strength colour of `ink` over backdrop `s` (W3C, 8-bit codes).
BLENDS = {
    'multiply': lambda s, ink: tuple(code(a / 255 * b / 255) for a, b in zip(s, ink)),
    'screen': lambda s, ink: tuple(code(a / 255 + b / 255 - a / 255 * b / 255)
                                   for a, b in zip(s, ink)),
    'difference': lambda s, ink: tuple(code(abs(a - b) / 255) for a, b in zip(s, ink)),
}

K = 131  # THRESH's k (`indexed_edge`), from the rule's definition.


def reindexed(table, before, after, ink, blend):
    """The hand re-index (phase 3b's rule, written here): each pixel of `after` (the RGB twin's
    render) over `before` (the Indexed render, opaque, no transparent index): unchanged keeps
    its colour; else T = `blend`(old, ink) and the entry nearest T when the projection of the
    pixel onto T from the old colour is at least K / 255, else the old colour."""
    out = []
    for s, c in zip(before, after):
        if c == s:
            out.append(s)
            continue
        assert s[3] == 255 and c[3] >= 128, (s, c)
        old = s[:3]
        target = blend(old, ink)
        num = sum((c[k] - old[k]) * (target[k] - old[k]) for k in range(3))
        den = sum((target[k] - old[k]) ** 2 for k in range(3))
        takes = den == 0 or 255 * min(max(num, 0), den) >= K * den
        out.append(table[nearest(table, target)] + (255,) if takes else s)
    return out


def chunk(kind, data):
    return struct.pack('>I', len(data)) + kind + data + struct.pack('>I', zlib.crc32(kind + data))


def png(path, w, h, depth, colour, samples, extra=()):
    """A PNG of `samples` (row-major, channels per colour type) at `depth`."""
    channels = {0: 1, 2: 3, 3: 1, 4: 2, 6: 4}[colour]
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


def palette_png_parts(data):
    """(PLTE entries, tRNS bytes, indices) of a non-interlaced palette PNG."""
    assert data[:8] == b'\x89PNG\r\n\x1a\n'
    at, packed = 8, b''
    width = height = depth = 0
    plte, trns = [], b''
    while at + 8 <= len(data):
        length, kind = struct.unpack('>I4s', data[at:at + 8])
        body = data[at + 8:at + 8 + length]
        if kind == b'IHDR':
            width, height, depth, colour, _, _, interlace = struct.unpack('>IIBBBBB', body)
            assert colour == 3 and interlace == 0 and depth in (1, 2, 4, 8), body
        elif kind == b'PLTE':
            plte = [tuple(body[i:i + 3]) for i in range(0, len(body), 3)]
        elif kind == b'tRNS':
            trns = body
        elif kind == b'IDAT':
            packed += body
        at += 12 + length
    stride = (width * depth + 7) // 8
    raw = zlib.decompress(packed)
    previous = bytearray(stride)
    indices = []
    for y in range(height):
        start = y * (stride + 1)
        kind = raw[start]
        row = bytearray(raw[start + 1:start + 1 + stride])
        for i in range(stride):
            left = row[i - 1] if i >= 1 else 0
            up = previous[i]
            corner = previous[i - 1] if i >= 1 else 0
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
            bit = x * depth
            indices.append((row[bit // 8] >> (8 - depth - bit % 8)) & ((1 << depth) - 1))
    return plte, trns, indices


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', required=True)
    args = parser.parse_args()
    output = Path(args.output).expanduser()
    output.mkdir(parents=True, exist_ok=True)
    folder = output / 'work'
    folder.mkdir(parents=True, exist_ok=False)
    receipts = {'expectations': 'fixed in scripts/verify_indexed_mode.py before any measurement'}
    # Phase 2c's clipboard cases (I29) touch the pasteboard: a private one, never the user's.
    os.environ['DR_PASTEBOARD_NAME'] = f'com.kgalligan.doctored.verify-indexed-mode.{os.getpid()}'

    with App(folder / 'app', folder / 'recovery', interval=600) as app:
        call = app.call

        def summary(doc):
            return next(d for d in call('list_documents')['documents'] if d['id'] == doc)

        def mode(doc):
            return summary(doc)['color_mode']

        def render(doc):
            """sRGB RGBA pixels of `render` with no channel."""
            result = app.client.rpc('tools/call', {'name': 'render',
                                                   'arguments': {'document_id': doc}})
            assert not result.get('isError'), result
            data = next(c['data'] for c in result['content'] if c['type'] == 'image')
            _, _, px = png_pixels(base64.b64decode(data))
            return [p + (255,) if len(p) == 3 else p for p in px]

        def sized(doc):
            """(width, height, `render` pixels)."""
            info = summary(doc)
            return info['width'], info['height'], render(doc)

        def opened(path, **extra):
            return call('open_document', path=str(path), **extra)['document']['id']

        source = [p + (255,) for p in PATCHES]

        # ---- I1: palette PNG -----------------------------------------------
        palette_png = folder / 'palette.png'
        trns = bytes(0 if i == CLEAR else 255 for i in range(3))
        png(palette_png, 4, 2, 8, 3, INDICES,
            extra=(chunk(b'PLTE', bytes(v for p in PALETTE for v in p)), chunk(b'tRNS', trns)))
        doc1 = opened(palette_png)
        assert mode(doc1) == 'indexed' and summary(doc1)['layer_count'] == 1, summary(doc1)
        info = call('get_document', document_id=doc1)
        assert info['indexed'] == {'colors': 5, 'transparent_index': CLEAR}, info.get('indexed')
        px = render(doc1)
        for pixel, index in zip(px, INDICES):
            if index == CLEAR:
                assert pixel[3] == 0, (index, pixel)
            else:
                assert pixel == PALETTE[index] + (255,), (index, pixel)
        receipts['i1'] = {'indexed': info['indexed'], 'render': px}
        print('PASS i1: a palette PNG opens as Indexed Color with its palette and transparent index',
              flush=True)

        # ---- I2: GIF -------------------------------------------------------
        rgb_png = folder / 'patches.png'
        png(rgb_png, len(PATCHES), 1, 8, 2, [v for p in PATCHES for v in p])
        doc_rgb = opened(rgb_png)
        gif = folder / 'patches.gif'
        call('save_copy', document_id=doc_rgb, path=str(gif), format='gif')
        doc2 = opened(gif)
        assert mode(doc2) == 'indexed' and summary(doc2)['layer_count'] == 1, summary(doc2)
        colors = call('get_document', document_id=doc2)['indexed']['colors']
        assert 2 <= colors <= 256, colors
        receipts['i2'] = {'colors': colors}
        print('PASS i2: a GIF opens as Indexed Color', flush=True)

        # ---- I3: RGB → Indexed ---------------------------------------------
        doc3 = doc_rgb
        assert summary(doc3)['precision'] == 'rgba8'
        reply = call('convert_mode', document_id=doc3, mode='indexed', palette='exact', dither='none')
        assert reply['color_mode'] == 'indexed', reply
        assert reply['indexed'] == {'colors': 4, 'transparent_index': None}, reply['indexed']
        assert render(doc3) == source, render(doc3)
        call('undo', document_id=doc3)
        assert mode(doc3) == 'rgb'
        call('redo', document_id=doc3)
        assert mode(doc3) == 'indexed'
        receipts['i3'] = reply['indexed']
        print('PASS i3: RGB → Indexed Color is exact and one undo step', flush=True)

        # ---- I4: back to RGB -----------------------------------------------
        back = call('convert_mode', document_id=doc3, mode='rgb')
        assert back['color_mode'] == 'rgb' and summary(doc3)['precision'] == 'rgba8', back
        assert render(doc3) == source
        call('undo', document_id=doc3)
        assert mode(doc3) == 'indexed'
        print('PASS i4: Indexed Color → RGB is exact at 8 bits, one undo step', flush=True)

        # ---- I5: Grayscale -------------------------------------------------
        gray = call('convert_mode', document_id=doc3, mode='gray')
        assert gray['color_mode'] == 'gray' and gray['depth'] == 8, gray
        call('undo', document_id=doc3)
        assert mode(doc3) == 'indexed'
        receipts['i5'] = gray['depth']
        print('PASS i5: Indexed Color → Grayscale at 8 bits, one undo step', flush=True)

        # ---- I6: .rz -------------------------------------------------------
        rz = folder / 'indexed.rz'
        call('save_copy', document_id=doc3, path=str(rz), format='rz')
        doc6 = opened(rz)
        assert mode(doc6) == 'indexed', summary(doc6)
        assert call('get_document', document_id=doc6)['indexed'] == reply['indexed']
        assert render(doc6) == source
        print('PASS i6: .rz reopens the Indexed Color document exactly', flush=True)

        # ---- I7: refusals --------------------------------------------------
        # Since phase 2d the layer row is admitted: Lock All, then unlocked.
        locked = call('set_layer_lock', document_id=doc3, layer=0, locks=['all'])
        assert locked.get('ok') is True, locked
        unlocked = call('set_layer_lock', document_id=doc3, layer=0, locks=[])
        assert unlocked.get('ok') is True, unlocked
        refusals = {
            'duplicate_layer': call('duplicate_layer', fail=True, document_id=doc3, index=0),
            'flatten_image': call('flatten_image', fail=True, document_id=doc3),
            'new_layer': call('new_layer', fail=True, document_id=doc3),
            'save_copy jpeg': call('save_copy', fail=True, document_id=doc3,
                                   path=str(folder / 'no.jpg'), format='jpeg'),
        }
        assert refusals['flatten_image'] == ONE_LAYER.format('flatten_image'), refusals
        assert refusals['duplicate_layer'] == DISABLED.format('duplicate_layer'), refusals
        assert refusals['new_layer'] == DISABLED.format('new_layer'), refusals
        assert refusals['save_copy jpeg'] == SAVE_REFUSAL, refusals
        # A second file: open_document answers the already-open I3 document for the first.
        rgb_png_u = folder / 'patches-u.png'
        rgb_png_u.write_bytes(rgb_png.read_bytes())
        doc_u = opened(rgb_png_u)
        refusals['uniform 7'] = call('convert_mode', fail=True, document_id=doc_u, mode='indexed',
                                     palette='uniform', colors=7)
        assert refusals['uniform 7'] == UNIFORM_7, refusals
        rgb16 = folder / 'patches16.png'
        png(rgb16, len(PATCHES), 1, 16, 2, [v * 257 for p in PATCHES for v in p])
        doc16 = opened(rgb16, precision='preserve')
        refusals['16-bit'] = call('convert_mode', fail=True, document_id=doc16, mode='indexed')
        assert refusals['16-bit'] == EIGHT_BITS, refusals
        call('new_layer', document_id=doc_u)
        assert summary(doc_u)['layer_count'] == 2
        refusals['two layers'] = call('convert_mode', fail=True, document_id=doc_u, mode='indexed')
        assert refusals['two layers'] == FLATTEN, refusals
        flat = call('convert_mode', document_id=doc_u, mode='indexed', flatten=True)
        assert flat['color_mode'] == 'indexed' and summary(doc_u)['layer_count'] == 1, flat
        refusals['set_layer_lock'] = locked
        receipts['i7'] = refusals
        print('PASS i7: the layer locks set; the mode refuses by its sentences and flattens on '
              'request', flush=True)

        # ---- I8: get_document ----------------------------------------------
        info = call('get_document', document_id=doc3)
        assert info['color_mode'] == 'indexed' and info['indexed']['colors'] == 4, info.get('indexed')
        receipts['i8'] = info['indexed']
        print('PASS i8: get_document reports the palette size', flush=True)

        # ---- I9: Adaptive --------------------------------------------------
        rgb_png_a = folder / 'patches-a.png'
        rgb_png_a.write_bytes(rgb_png.read_bytes())
        doc9 = opened(rgb_png_a)
        adaptive = call('convert_mode', document_id=doc9, mode='indexed', palette='adaptive',
                        colors=2, dither='none')
        assert adaptive['indexed'] == {'colors': 2, 'transparent_index': None}, adaptive
        expected = [ADAPTIVE_2[i] + (255,) for i in ADAPTIVE_INDICES]
        assert render(doc9) == expected, render(doc9)
        receipts['i9'] = {'indexed': adaptive['indexed'], 'render': expected}
        print('PASS i9: Adaptive is the hand-computed median cut', flush=True)

        # ---- I10: PNG keeps the palette ------------------------------------
        png_out = folder / 'kept.png'
        exported = call('export_image', document_id=doc1, path=str(png_out),
                        settings={'format': 'png'})
        plte, trns_out, indices = palette_png_parts(png_out.read_bytes())
        assert plte == PALETTE, plte
        assert list(trns_out) == [255, 255, 0], list(trns_out)
        assert indices == INDICES, indices
        receipts['i10'] = {'bit_depth': exported.get('bit_depth'), 'indices': indices}
        print('PASS i10: a PNG export keeps the palette, the transparent index and every index',
              flush=True)

        # ---- I11: GIF ------------------------------------------------------
        gif_out = folder / 'kept.gif'
        call('save_copy', document_id=doc1, path=str(gif_out), format='gif')
        doc11 = opened(gif_out)
        assert call('get_document', document_id=doc11)['indexed'] == {
            'colors': 8, 'transparent_index': CLEAR}
        assert render(doc11) == receipts['i1']['render']
        print('PASS i11: a GIF copy reopens with the palette and every index', flush=True)

        # ---- I12: TIFF -----------------------------------------------------
        tiff_out = folder / 'kept.tif'
        call('export_image', document_id=doc1, path=str(tiff_out), settings={'format': 'tiff'})
        doc12 = opened(tiff_out)
        assert call('get_document', document_id=doc12)['indexed'] == {
            'colors': 256, 'transparent_index': CLEAR}
        assert render(doc12) == receipts['i1']['render']
        print('PASS i12: a TIFF export reopens as a palette TIFF with every index', flush=True)

        # ---- I13: kept depth and RGB ---------------------------------------
        depth = call('export_image', fail=True, document_id=doc1, path=str(folder / 'no16.png'),
                     settings={'format': 'png', 'bit_depth': 16})
        assert depth == KEPT_DEPTH, depth
        jpeg = call('export_image', document_id=doc1, path=str(folder / 'rgb.jpg'),
                    settings={'format': 'jpeg'})
        assert jpeg['bit_depth'] == 8 and jpeg['format'] == 'jpeg', jpeg
        receipts['i13'] = {'refusal': depth, 'jpeg': jpeg['format']}
        print('PASS i13: a kept export refuses 16 bits; a JPEG exports RGB', flush=True)

        # ---- I14: save losses ----------------------------------------------
        before = call('get_document', document_id=doc1)['save_preservation']['losses_by_format']
        assert before['png'] == [], before['png']
        call('add_guide', document_id=doc1, orientation='vertical', position=2)
        after = call('get_document', document_id=doc1)['save_preservation']['losses_by_format']
        assert GUIDES_LINE in after['png'], after['png']
        assert GUIDES_LINE in after['gif'] and 'embedded color profile' in after['gif'], after['gif']
        receipts['i14'] = {'before': before, 'after': after}
        print('PASS i14: get_document names what a copy drops', flush=True)

        # ---- I15: get_color_table ------------------------------------------
        act_out = folder / 'i1.act'
        table = call('get_color_table', document_id=doc1, path=str(act_out))
        assert [tuple(c) for c in table['colors']] == PALETTE, table
        assert table['transparent_index'] == CLEAR and table['count'] == 5, table
        assert act_out.read_bytes() == act_bytes(PALETTE, 5, CLEAR), act_out.read_bytes()[-8:]
        receipts['i15'] = {'table': table, 'act_bytes': len(act_out.read_bytes())}
        print('PASS i15: get_color_table reads the table and writes the hand-built .act bytes',
              flush=True)

        # ---- I16: set_color_table ------------------------------------------
        edited = [ENTRY_0] + PALETTE[1:]
        reply = call('set_color_table', document_id=doc1, colors=[list(c) for c in edited],
                     transparent_index=None)
        assert reply['changed'] is True and reply['colors'] == 5, reply
        assert reply['transparent_index'] is None, reply
        expected = [edited[i] + (255,) for i in INDICES]
        assert render(doc1) == expected, render(doc1)
        call('undo', document_id=doc1)
        undone = call('get_color_table', document_id=doc1)
        assert [tuple(c) for c in undone['colors']] == PALETTE, undone
        assert undone['transparent_index'] == CLEAR, undone
        assert render(doc1) == receipts['i1']['render']
        call('redo', document_id=doc1)
        assert render(doc1) == expected
        assert call('get_color_table', document_id=doc1)['transparent_index'] is None
        receipts['i16'] = {'reply': {k: reply[k] for k in ('changed', 'colors', 'transparent_index')},
                           'render': expected}
        print('PASS i16: set_color_table re-renders every index exactly, one undo step', flush=True)

        # ---- I17: .act load ------------------------------------------------
        full_act = folder / 'full.act'
        full_act.write_bytes(act_bytes(FULL_ACT))
        loaded = call('set_color_table', document_id=doc1, path=str(full_act))
        assert loaded['colors'] == 256 and loaded['transparent_index'] is None, loaded
        table = call('get_color_table', document_id=doc1)
        assert [tuple(c) for c in table['colors']] == FULL_ACT, table['colors'][:4]
        short_act = folder / 'short.act'
        short_act.write_bytes(act_bytes(PALETTE[:3], 3, None))
        short = call('set_color_table', fail=True, document_id=doc1, path=str(short_act))
        assert short == SHORT_3, short
        receipts['i17'] = {'loaded': loaded['colors'], 'short': short}
        print('PASS i17: a 768-byte .act loads; a short 772-byte table refuses', flush=True)

        # ---- I18: Custom ---------------------------------------------------
        rgb_png_c = folder / 'patches-c.png'
        rgb_png_c.write_bytes(rgb_png.read_bytes())
        doc18 = opened(rgb_png_c)
        custom = call('convert_mode', document_id=doc18, mode='indexed', palette='custom',
                      table=[list(c) for c in CUSTOM], dither='none')
        assert custom['indexed'] == {'colors': 3, 'transparent_index': None}, custom
        expected = [CUSTOM[nearest(CUSTOM, p)] + (255,) for p in PATCHES]
        assert render(doc18) == expected, render(doc18)
        table = call('get_color_table', document_id=doc18)
        assert [tuple(c) for c in table['colors']] == CUSTOM, table
        receipts['i18'] = {'indexed': custom['indexed'], 'render': expected}
        print('PASS i18: Custom maps each pixel to its nearest table entry', flush=True)

        # ---- I19: Previous -------------------------------------------------
        rgb_png_p = folder / 'patches-p.png'
        rgb_png_p.write_bytes(rgb_png.read_bytes())
        doc19 = opened(rgb_png_p)
        previous = call('convert_mode', document_id=doc19, mode='indexed', palette='previous',
                        dither='none')
        assert previous['indexed'] == {'colors': 3, 'transparent_index': None}, previous
        table = call('get_color_table', document_id=doc19)
        assert [tuple(c) for c in table['colors']] == CUSTOM, table
        clear_png = folder / 'clear.png'
        png(clear_png, 2, 1, 8, 6, [255, 0, 0, 255, 0, 0, 0, 0])
        doc_clear = opened(clear_png)
        two = call('convert_mode', document_id=doc_clear, mode='indexed', palette='adaptive',
                   colors=2, dither='none')
        assert two['indexed'] == {'colors': 2, 'transparent_index': 1}, two
        rgb_png_q = folder / 'patches-q.png'
        rgb_png_q.write_bytes(rgb_png.read_bytes())
        doc_q = opened(rgb_png_q)
        none = call('convert_mode', fail=True, document_id=doc_q, mode='indexed', palette='previous')
        assert none == NO_PREVIOUS, none
        receipts['i19'] = {'previous': table['colors'], 'refusal': none}
        print('PASS i19: Previous is the last palette; one opaque entry leaves none', flush=True)

        # ---- I20 / I21: CMYK and Lab directly -------------------------------
        for case, target, key in (('i20', 'cmyk', 'ink16'), ('i21', 'lab', 'lab')):
            direct_png = folder / f'{case}-direct.png'
            via_png = folder / f'{case}-via.png'
            direct_png.write_bytes(palette_png.read_bytes())
            via_png.write_bytes(palette_png.read_bytes())
            direct = opened(direct_png)
            via = opened(via_png)
            assert mode(direct) == 'indexed' and mode(via) == 'indexed'
            reply = call('convert_mode', document_id=direct, mode=target)
            assert reply['color_mode'] == target and reply['depth'] == 8, reply
            call('convert_mode', document_id=via, mode='rgb')
            call('convert_mode', document_id=via, mode=target)
            assert mode(via) == target
            assert render(direct) == render(via)
            samples = []
            for y in range(2):
                for x in range(4):
                    a = call('sample_color', document_id=direct, x=x, y=y)[key]
                    b = call('sample_color', document_id=via, x=x, y=y)[key]
                    assert a == b, (case, x, y, a, b)
                    samples.append(a)
            call('undo', document_id=direct)
            assert mode(direct) == 'indexed'
            receipts[case] = {'depth': reply['depth'], key: samples}
            print(f'PASS {case}: Indexed Color → {target} equals RGB then {target}, one undo step',
                  flush=True)

        # ---- I22: the measured table --------------------------------------
        i22 = {}
        one_layer = ONE_LAYER_TOOLS + AUTO_TOOLS
        decided = RULED_PERMANENT + PROFILE_TOOLS + one_layer + ['hdr_toning'] + SELECT_TOOLS
        later = [tool for tool in TAKE_PHASE_3 if tool not in decided]
        # Since phase 3b every tool of take1's phase-3 list is decided.
        assert later == [] and sorted(decided) == sorted(TAKE_PHASE_3), (later, decided)
        for tools, sentence in ((TAKE_DISABLED, DISABLED), (RULED_PERMANENT, DISABLED),
                                (['hdr_toning'], DISABLED), (one_layer, ONE_LAYER)):
            for tool in tools:
                got = call(tool, fail=True, document_id=doc3)
                assert got == sentence.format(tool), (tool, got)
                i22[tool] = got
        # The Select menu's tools are admitted: whatever they answer with no arguments, it is no
        # gate sentence.
        for tool in SELECT_TOOLS:
            result = app.client.rpc('tools/call', {'name': tool,
                                                   'arguments': {'document_id': doc3}})
            text = next((c['text'] for c in result.get('content', []) if c['type'] == 'text'), '')
            assert not any(mark in text for mark in GATE_MARKS), (tool, text)
            i22[tool] = {'admitted': True, 'error': bool(result.get('isError'))}
        call('deselect', document_id=doc3)
        assert mode(doc3) == 'indexed'
        receipts['i22'] = i22
        print(f'PASS i22: the {len(TAKE_DISABLED)} tools take1 read disabled in Indexed Color, '
              f'the {len(RULED_PERMANENT)} Auto items, HDR Toning and the {len(one_layer)} '
              f'one-layer tools refuse for good; the {len(SELECT_TOOLS)} Select tools are '
              'admitted', flush=True)

        # ---- I23: phase 2b's refusals -------------------------------------
        i23 = {}
        for tool, sentence in ([(tool, DISABLED) for tool in RULED_PERMANENT]
                               + [('hdr_toning', DISABLED)]):
            got = call(tool, fail=True, document_id=doc3)
            assert got == sentence.format(tool), (tool, got)
            i23[tool] = got
        assert i23['hdr_toning'] == HDR, i23
        receipts['i23'] = i23
        print('PASS i23: the Auto items and HDR Toning refuse', flush=True)

        # ---- I24: the Brush -------------------------------------------------
        width, height = 30, 12
        bands_png = folder / 'bands.png'
        png(bands_png, width, height, 8, 2,
            [v for y in range(height) for x in range(width) for v in BANDS[x // 10]])
        doc24 = opened(bands_png)
        converted = call('convert_mode', document_id=doc24, mode='indexed', palette='exact',
                         dither='none')
        assert converted['indexed'] == {'colors': 3, 'transparent_index': None}, converted
        before = render(doc24)
        stroke = call('brush_stroke', document_id=doc24, layer=0, points=[[3, 6], [27, 6]], size=7,
                      rgba=[c / 255 for c in INK] + [1])
        assert stroke.get('changed') is True, stroke
        after = render(doc24)
        table = [tuple(c) for c in call('get_color_table', document_id=doc24)['colors']]
        assert all(p[3] == 255 and tuple(p[:3]) in table for p in after), after

        def far(x, y):
            dx = 0 if 3 <= x <= 27 else min(abs(x - 3), abs(x - 27))
            return (dx * dx + (y - 6) ** 2) ** 0.5 > 5

        moved = 0
        for i, (a, b) in enumerate(zip(before, after)):
            x, y = i % width, i // width
            if far(x, y):
                assert a == b, (x, y, a, b)
            moved += a != b
        entry = table[nearest(table, INK)]
        assert entry == BAND, entry
        assert tuple(after[6 * width + 15][:3]) == entry, after[6 * width + 15]
        assert mode(doc24) == 'indexed'
        receipts['i24'] = {'reply': stroke, 'moved': moved, 'entry': entry}
        print(f'PASS i24: the Brush re-indexes {moved} pixels onto the palette; full ink is the '
              'nearest entry', flush=True)

        # ---- I25: undo ------------------------------------------------------
        call('undo', document_id=doc24)
        assert render(doc24) == before
        call('redo', document_id=doc24)
        assert render(doc24) == after
        print('PASS i25: undo restores the render and redo reapplies the stroke', flush=True)

        # ---- I26: .rz and the mode -----------------------------------------
        painted_rz = folder / 'painted.rz'
        call('save_copy', document_id=doc24, path=str(painted_rz), format='rz')
        doc26 = opened(painted_rz)
        assert mode(doc26) == 'indexed' and render(doc26) == after
        # Since phase 3b Multiply paints: the hand re-index of the same stroke on an RGB twin
        # (doc26, converted to RGB Color).
        # A hex colour, which the 8-bit twin takes too (numeric rgba is 16-bit only).
        stroke26 = dict(layer=0, points=[[3, 9], [27, 9]], size=7,
                        color='#%02X%02X%02X' % INK, blend_mode='multiply')
        call('convert_mode', document_id=doc26, mode='rgb')
        call('brush_stroke', document_id=doc26, **stroke26)
        multiply = call('brush_stroke', document_id=doc24, **stroke26)
        assert multiply.get('changed') is True, multiply
        expected26 = reindexed(table, after, render(doc26), INK, BLENDS['multiply'])
        got26 = render(doc24)
        bad = [i for i, (a, b) in enumerate(zip(got26, expected26)) if a != b]
        assert not bad, ('multiply', bad[:8], [(got26[i], expected26[i]) for i in bad[:4]])
        assert got26 != after
        receipts['i26'] = {'multiply': multiply}
        print('PASS i26: a painted document reopens from .rz; Multiply paints, the RGB stroke '
              're-indexed', flush=True)

        def pixel_same(a, b):
            """`PIXEL`: equal, or both clear (a clear pixel's colour is the matte)."""
            return a == b or (a[3] == 0 and b[3] == 0)

        def moved_render(w, h, px, nw, nh, source, fill=(0, 0, 0, 0)):
            """`px` (w × h) moved onto nw × nh: `source(x, y)` names the old pixel; off the old
            canvas is `fill` (clear unless given)."""
            out = []
            for y in range(nh):
                for x in range(nw):
                    sx, sy = source(x, y)
                    out.append(px[sy * w + sx] if 0 <= sx < w and 0 <= sy < h else fill)
            return out

        def assert_moved(doc, expected, nw, nh, case):
            w, h, got = sized(doc)
            assert (w, h) == (nw, nh), (case, w, h, nw, nh)
            bad = [i for i, (a, b) in enumerate(zip(got, expected)) if not pixel_same(a, b)]
            assert not bad, (case, bad[:8], [(got[i], expected[i]) for i in bad[:4]])
            return got

        def table_of(doc):
            return [tuple(c) for c in call('get_color_table', document_id=doc)['colors']]

        def fresh(path, name, **convert):
            copy_path = folder / name
            copy_path.write_bytes(path.read_bytes())
            doc = opened(copy_path)
            reply = call('convert_mode', document_id=doc, mode='indexed', palette='exact',
                         dither='none', **convert)
            assert reply['color_mode'] == 'indexed', reply
            return doc, reply['indexed']

        def twin_of(doc, name):
            """An RGB twin of `doc` as it is: saved as .doctored, reopened, converted to RGB Color."""
            path = folder / name
            call('save_copy', document_id=doc, path=str(path), format='rz')
            twin = opened(path)
            call('convert_mode', document_id=twin, mode='rgb')
            assert mode(twin) == 'rgb', summary(twin)
            return twin

        ink_hex = '#%02X%02X%02X' % INK

        # ---- I27: Fill and Stroke -------------------------------------------
        doc27, indexed27 = fresh(bands_png, 'bands-27.png')
        assert indexed27 == {'colors': 3, 'transparent_index': None}, indexed27
        table27 = table_of(doc27)
        # Phase 3a's background rule: with no transparent index a clear pixel is the entry
        # nearest white (the default background colour), computed here.
        BG = table27[nearest(table27, (255, 255, 255))] + (255,)
        before = render(doc27)
        call('select_rect', document_id=doc27, x=2, y=2, width=6, height=6)
        filled = call('fill_selection', document_id=doc27, layer=0, color=ink_hex)
        assert filled.get('changed') is True, filled
        after = render(doc27)
        entry = table27[nearest(table27, INK)]
        for i, (a, b) in enumerate(zip(before, after)):
            x, y = i % width, i // width
            assert b[3] == 255 and tuple(b[:3]) in table27, (x, y, b)
            if 2 <= x < 8 and 2 <= y < 8:
                assert tuple(b[:3]) == entry, (x, y, b)
            else:
                assert a == b, (x, y, a, b)
        stroked_before = after
        stroked = call('stroke_selection', document_id=doc27, layer=0, color='#FFFFFF', width=2,
                       location='inside')
        assert stroked.get('changed') is True, stroked
        stroked_after = render(doc27)

        def far_from_rect(x, y):
            dx = max(2 - x, 0, x - 7)
            dy = max(2 - y, 0, y - 7)
            return (dx * dx + dy * dy) ** 0.5 > 3

        for i, (a, b) in enumerate(zip(stroked_before, stroked_after)):
            x, y = i % width, i // width
            assert b[3] == 255 and tuple(b[:3]) in table27, (x, y, b)
            if far_from_rect(x, y):
                assert a == b, (x, y, a, b)
        call('deselect', document_id=doc27)
        bucket = call('fill', document_id=doc27, layer=0, x=25, y=6, color='#FFFFFF')
        assert bucket.get('changed') is True, bucket
        white = table27[nearest(table27, (255, 255, 255))]
        assert white == (255, 255, 255), white
        bucketed = render(doc27)
        for y in range(height):
            for x in range(20, 30):
                assert tuple(bucketed[y * width + x][:3]) == white, (x, y, bucketed[y * width + x])
        # Since phase 3b the Paint Bucket paints in Multiply: the hand re-index of the same fill
        # on an RGB twin of the document as it is.
        twin27 = twin_of(doc27, 'bucketed-27.rz')
        fill27 = dict(layer=0, x=25, y=6, color='#3C3C3C', blend_mode='Multiply')
        call('fill', document_id=twin27, **fill27)
        multiply = call('fill', document_id=doc27, **fill27)
        assert multiply.get('changed') is True, multiply
        expected27 = reindexed(table27, bucketed, render(twin27), (60, 60, 60),
                               BLENDS['multiply'])
        got27 = render(doc27)
        bad = [i for i, (a, b) in enumerate(zip(got27, expected27)) if a != b]
        assert not bad, ('multiply fill', bad[:8], [(got27[i], expected27[i]) for i in bad[:4]])
        assert got27 != bucketed
        call('undo', document_id=doc27)
        assert render(doc27) == bucketed
        receipts['i27'] = {'fill': filled, 'stroke': stroked, 'bucket': bucket,
                           'multiply': multiply}
        print('PASS i27: Fill, Stroke and the Paint Bucket land on palette entries; Multiply '
              'paints, the RGB fill re-indexed', flush=True)

        # ---- I28: the Eraser and Clear --------------------------------------
        clear_bands = folder / 'clear-bands.png'
        rows = []
        for y in range(height):
            for x in range(width):
                rows += [0, 0, 0, 255] if x < 10 else [0, 0, 0, 0] if x < 20 else list(BAND) + [255]
        png(clear_bands, width, height, 8, 6, rows)
        doc28, indexed28 = fresh(clear_bands, 'clear-bands-28.png', transparency=True)
        assert indexed28 == {'colors': 3, 'transparent_index': 2}, indexed28
        before = render(doc28)
        erased = call('eraser_stroke', document_id=doc28, layer=0, points=[[3, 6], [27, 6]],
                      size=7)
        assert erased.get('changed') is True, erased
        after = render(doc28)
        for x in range(5, 26):
            assert after[6 * width + x][3] == 0, (x, after[6 * width + x])
        for i, (a, b) in enumerate(zip(before, after)):
            x, y = i % width, i // width
            if far(x, y):
                assert a == b, (x, y, a, b)
        call('select_rect', document_id=doc28, x=0, y=0, width=4, height=4)
        cleared = call('clear_selection', document_id=doc28, layer=0)
        assert cleared.get('changed') is True, cleared
        call('deselect', document_id=doc28)
        after = render(doc28)
        for y in range(4):
            for x in range(4):
                assert after[y * width + x][3] == 0, (x, y, after[y * width + x])
        held = render(doc27)
        erased27 = call('eraser_stroke', document_id=doc27, layer=0,
                        points=[[3, 6], [27, 6]], size=7)
        assert erased27.get('changed') is True, erased27
        after27 = render(doc27)
        for x in range(5, 26):
            assert after27[6 * width + x] == BG, (x, after27[6 * width + x])
        for i, (a, b) in enumerate(zip(held, after27)):
            x, y = i % width, i // width
            if far(x, y):
                assert a == b, (x, y, a, b)
        receipts['i28'] = {'eraser': erased, 'clear': cleared, 'eraser_no_t': erased27,
                           'background': BG}
        print('PASS i28: the Eraser and Clear take the transparent index; with none the Eraser '
              'paints the entry nearest white', flush=True)

        # ---- I29: the clipboard ---------------------------------------------
        copied = call('copy', document_id=doc28, layer=0)
        assert copied.get('ok') is True, copied
        merged = call('copy_merged', document_id=doc28)
        assert merged.get('ok') is True, merged
        before = render(doc28)
        call('select_rect', document_id=doc28, x=22, y=8, width=4, height=3)
        cut = call('cut', document_id=doc28, layer=0)
        assert cut.get('ok') is True, cut
        after = render(doc28)
        for i, (a, b) in enumerate(zip(before, after)):
            x, y = i % width, i // width
            if 22 <= x < 26 and 8 <= y < 11:
                assert b[3] == 0, (x, y, b)
            else:
                assert a == b, (x, y, a, b)
        call('undo', document_id=doc28)
        assert render(doc28) == before
        call('deselect', document_id=doc28)
        held = render(doc27)
        inside = [(x, y) for y in range(9, 12) for x in range(2, 6)]
        assert any(held[y * width + x] != BG for x, y in inside), 'the cut must change pixels'
        call('select_rect', document_id=doc27, x=2, y=9, width=4, height=3)
        cut_no_t = call('cut', document_id=doc27, layer=0)
        assert cut_no_t.get('ok') is True, cut_no_t
        after27 = render(doc27)
        for i, (a, b) in enumerate(zip(held, after27)):
            x, y = i % width, i // width
            assert b == (BG if (x, y) in inside else a), (x, y, a, b)
        call('deselect', document_id=doc27)
        receipts['i29'] = {'copy': copied, 'copy_merged': merged, 'cut': cut,
                           'cut_no_t': cut_no_t}
        print('PASS i29: Copy, Copy Merged and Cut; with no transparent index Cut leaves the '
              'entry nearest white', flush=True)

        # ---- I30: exact geometry --------------------------------------------
        doc30, _ = fresh(clear_bands, 'clear-bands-30.png', transparency=True)
        w, h, px = sized(doc30)
        rotated = call('rotate', document_id=doc30, degrees=90)
        plan90 = rotated.get('working_memory_plan_bytes')
        assert isinstance(plan90, int) and plan90 > 0, rotated
        expected = moved_render(w, h, px, h, w, lambda x, y: (y, h - 1 - x))
        px = assert_moved(doc30, expected, h, w, 'rotate 90')
        w, h = h, w
        call('flip', document_id=doc30, axis='horizontal')
        px = assert_moved(doc30, moved_render(w, h, px, w, h, lambda x, y: (w - 1 - x, y)), w, h,
                          'flip')
        call('crop', document_id=doc30, x=4, y=2, width=10, height=6)
        px = assert_moved(doc30, moved_render(w, h, px, 10, 6, lambda x, y: (x + 4, y + 2)), 10, 6,
                          'crop')
        w, h = 10, 6
        call('image_size', document_id=doc30, width=2 * w, height=2 * h, filter='nearest')
        px = assert_moved(doc30, moved_render(w, h, px, 2 * w, 2 * h,
                                              lambda x, y: (x // 2, y // 2)), 2 * w, 2 * h,
                          'image_size nearest')
        w, h, px = sized(doc28)
        grown = call('canvas_size', document_id=doc28, width=w + 4, height=h + 4, anchor='center')
        assert grown.get('ok') is True, grown
        assert_moved(doc28, moved_render(w, h, px, w + 4, h + 4, lambda x, y: (x - 2, y - 2)),
                     w + 4, h + 4, 'canvas_size')
        w27, h27, held = sized(doc27)
        grow_no_t = call('canvas_size', document_id=doc27, width=w27 + 4, height=h27 + 4,
                         anchor='center')
        assert grow_no_t.get('ok') is True, grow_no_t
        assert_moved(doc27, moved_render(w27, h27, held, w27 + 4, h27 + 4,
                                         lambda x, y: (x - 2, y - 2), fill=BG),
                     w27 + 4, h27 + 4, 'canvas_size with no transparent index')
        call('undo', document_id=doc27)
        assert sized(doc27) == (w27, h27, held)
        receipts['i30'] = {'rotate_plan': plan90, 'canvas_size_no_t': grow_no_t}
        print('PASS i30: rotate, flip, crop, nearest Image Size and Canvas Size move every index '
              'exactly; with no transparent index the new area is the entry nearest white',
              flush=True)

        # ---- I31: resampling ------------------------------------------------
        table28 = table_of(doc28)

        def on_palette(doc, case):
            got = render(doc)
            bad = [p for p in got if not (p[3] == 0 or (p[3] == 255 and tuple(p[:3]) in table28))]
            assert not bad, (case, bad[:4])
            assert mode(doc) == 'indexed', case

        call('image_size', document_id=doc28, width=45, height=18, filter='lanczos3')
        on_palette(doc28, 'image_size lanczos3')
        turned = call('rotate', document_id=doc28, degrees=5, sampler='bicubic')
        assert isinstance(turned.get('working_memory_plan_bytes'), int), turned
        on_palette(doc28, 'rotate 5')
        w, h, _ = sized(doc28)
        call('perspective_crop', document_id=doc28,
             corners=[[2, 1], [w - 3, 0], [w - 1, h - 1], [0, h - 2]], width=20, height=8,
             sampler='bicubic')
        assert sized(doc28)[:2] == (20, 8), sized(doc28)[:2]
        on_palette(doc28, 'perspective_crop')
        receipts['i31'] = {'rotate': turned}
        print('PASS i31: resampled Image Size, rotation and Perspective Crop land on palette '
              'entries', flush=True)

        # ---- I32: undo and .rz ----------------------------------------------
        w, h, pre = sized(doc30)
        call('rotate', document_id=doc30, degrees=90)
        turned = assert_moved(doc30, moved_render(w, h, pre, h, w, lambda x, y: (y, h - 1 - x)), h,
                              w, 'rotate 90 again')
        call('undo', document_id=doc30)
        assert sized(doc30) == (w, h, pre)
        call('redo', document_id=doc30)
        assert render(doc30) == turned
        cropped_rz = folder / 'cropped.rz'
        call('save_copy', document_id=doc30, path=str(cropped_rz), format='rz')
        doc32 = opened(cropped_rz)
        assert mode(doc32) == 'indexed' and render(doc32) == turned
        print('PASS i32: undo restores the rotation and redo reapplies it; .rz reopens the '
              'cropped document', flush=True)

        # ---- I33: the decided ones (phase 3a) -------------------------------
        i33 = {}
        for tool, sentence in (('puppet_warp', DISABLED), ('align_layers', ONE_LAYER)):
            got = call(tool, fail=True, document_id=doc30)
            assert got == sentence.format(tool), (tool, got)
            i33[tool] = got
        held = render(doc30)
        revealed = call('reveal_canvas', document_id=doc30)
        assert revealed.get('changed') is False, revealed
        assert render(doc30) == held
        i33['reveal_canvas'] = revealed
        receipts['i33'] = i33
        print('PASS i33: Puppet Warp refuses for good, Align needs more than one layer, Reveal '
              'All changes nothing', flush=True)

        def on_table(doc, table, case):
            """Every `render` pixel clear or a table entry at alpha 255."""
            got = render(doc)
            bad = [p for p in got if not (p[3] == 0 or (p[3] == 255 and tuple(p[:3]) in table))]
            assert not bad, (case, bad[:4])
            assert mode(doc) == 'indexed' and summary(doc)['layer_count'] == 1, case
            return got

        def hand_pasted(px, w, src, sw, sh, at):
            """`src` (sw × sh) pasted over `px` (w wide) at `at`: a clear source pixel keeps the
            destination."""
            out = list(px)
            for y in range(sh):
                for x in range(sw):
                    p = src[y * sw + x]
                    dx, dy = x + at[0], y + at[1]
                    if p[3] == 255 and 0 <= dx < w and 0 <= dy < len(px) // w:
                        out[dy * w + dx] = p
            return out

        # ---- I34: Paste -------------------------------------------------------
        doc34, indexed34 = fresh(clear_bands, 'clear-bands-34.png', transparency=True)
        assert indexed34['transparent_index'] is not None, indexed34
        table34 = table_of(doc34)
        patch_png = folder / 'paste-patch.png'
        patch = [(250, 240, 10), (255, 255, 255), (20, 20, 20), (180, 60, 60)]
        png(patch_png, 10, 10, 8, 2, [v for y in range(10) for x in range(10)
                                      for v in patch[(x // 5) + 2 * (y // 5)]])
        doc_patch = opened(patch_png)
        call('copy', document_id=doc_patch, layer=0)
        before34 = render(doc34)
        pasted = call('paste', document_id=doc34, placement='centered', mode='pixels')
        plan34 = pasted.get('working_memory_plan_bytes')
        assert isinstance(plan34, int) and plan34 > 0, pasted
        assert pasted.get('layer') == 0, pasted
        after34 = on_table(doc34, table34, 'paste')
        assert after34 != before34
        call('undo', document_id=doc34)
        assert render(doc34) == before34
        call('redo', document_id=doc34)
        assert render(doc34) == after34
        call('undo', document_id=doc34)
        # Paste at an offset from a twin of the same distinct palette: the hand-moved pixels.
        twin, _ = fresh(clear_bands, 'clear-bands-34-twin.png', transparency=True)
        assert table_of(twin) == table34
        call('select_rect', document_id=twin, x=6, y=3, width=8, height=5)
        call('copy', document_id=twin, layer=0)
        call('deselect', document_id=twin)
        twin_px = render(twin)
        src = [twin_px[y * width + x] for y in range(3, 8) for x in range(6, 14)]
        at = call('paste', document_id=doc34, placement='at', x=15, y=4, mode='pixels')
        assert at.get('working_memory_plan_bytes', 0) > 0, at
        expected = hand_pasted(before34, width, src, 8, 5, (15, 4))
        assert_moved(doc34, expected, width, height, 'paste at')
        call('undo', document_id=doc34)
        # Paste Into and Outside of the patch: only inside, or only outside, the selection moves.
        call('copy', document_id=doc_patch, layer=0)
        into_out = {}
        for placement in ('into', 'outside'):
            call('select_rect', document_id=doc34, x=12, y=3, width=6, height=6)
            reply = call('paste', document_id=doc34, placement=placement, mode='pixels')
            got = on_table(doc34, table34, placement)
            changed = [i for i, (a, b) in enumerate(zip(before34, got)) if a != b]
            assert changed, placement
            for i in changed:
                x, y = i % width, i // width
                inside = 12 <= x < 18 and 3 <= y < 9
                assert inside == (placement == 'into'), (placement, x, y)
            into_out[placement] = {'changed': len(changed),
                                   'plan': reply.get('working_memory_plan_bytes')}
            call('undo', document_id=doc34)
            call('deselect', document_id=doc34)
        layers = call('paste', fail=True, document_id=doc34, mode='layers')
        assert layers == DISABLED.format('paste_layers'), layers
        receipts['i34'] = {'plan': plan34, 'into_outside': into_out, 'layers': layers}
        print('PASS i34: Paste lands on the palette in one undo step; a same-palette paste is the '
              'hand-moved pixels; Paste Into and Outside follow the selection', flush=True)

        # ---- I35: the transforms ------------------------------------------------
        doc35, _ = fresh(clear_bands, 'clear-bands-35.png', transparency=True)
        table35 = table_of(doc35)
        w, h, px = sized(doc35)
        moved35 = call('transform_layer', document_id=doc35, layer=0, translate_x=3,
                       sampler='nearest')
        assert moved35.get('ok') is True, moved35
        px = assert_moved(doc35, moved_render(w, h, px, w, h, lambda x, y: (x - 3, y)), w, h,
                          'transform_layer translate')
        w27, h27, held27 = sized(doc27)
        no_t = call('transform_layer', document_id=doc27, layer=0, translate_x=3,
                    sampler='nearest')
        assert no_t.get('ok') is True, no_t
        assert_moved(doc27, moved_render(w27, h27, held27, w27, h27, lambda x, y: (x - 3, y),
                                         fill=BG),
                     w27, h27, 'transform_layer translate with no transparent index')
        c, s = math.cos(math.radians(5)), math.sin(math.radians(5))
        cx, cy = w / 2, h / 2
        turned = call('transform_layers', document_id=doc35, layers=[0], sampler='bicubic',
                      matrix=[c, s, -s, c, cx - c * cx + s * cy, cy - s * cx - c * cy])
        assert turned.get('changed') is True, turned
        plan35 = turned.get('working_memory_plan_bytes')
        assert isinstance(plan35, int) and plan35 > 0, turned
        on_table(doc35, table35, 'transform_layers rotate')
        distorted = call('distort_layer', document_id=doc35, layer=0, sampler='bicubic',
                         corners=[[1, 0], [w - 2, 1], [w - 1, h - 1], [0, h - 2]])
        assert distorted.get('ok') is True, distorted
        on_table(doc35, table35, 'distort_layer')
        copy = call('transform_layers', fail=True, document_id=doc35, layers=[0], duplicate=True,
                    matrix=[1, 0, 0, 1, 2, 0])
        assert copy == COPY, copy
        receipts['i35'] = {'plan': plan35, 'no_t': no_t, 'copy': copy}
        print('PASS i35: a nearest translation moves every index; rotation and distortion land on '
              'the palette; a copy transform refuses', flush=True)

        # ---- I36: selected pixels and Transform Again ---------------------------
        doc36, _ = fresh(clear_bands, 'clear-bands-36.png', transparency=True)

        def selected_moved(px, rect, d):
            """`px` with `rect`'s pixels floated by `d`: the hole clear, a clear float landing
            nothing."""
            x0, y0, rw, rh = rect
            out = list(px)
            for y in range(y0, y0 + rh):
                for x in range(x0, x0 + rw):
                    out[y * width + x] = (0, 0, 0, 0)
            for y in range(y0, y0 + rh):
                for x in range(x0, x0 + rw):
                    p = px[y * width + x]
                    nx, ny = x + d[0], y + d[1]
                    if p[3] == 255 and 0 <= nx < width and 0 <= ny < height:
                        out[ny * width + nx] = p
            return out

        px = render(doc36)
        call('select_rect', document_id=doc36, x=6, y=2, width=6, height=4)
        first = call('transform_selected_pixels', document_id=doc36, layer=0, translate_x=4,
                     translate_y=1, sampler='nearest')
        assert first.get('ok') is True, first
        px = assert_moved(doc36, selected_moved(px, (6, 2, 6, 4), (4, 1)), width, height,
                          'transform_selected_pixels')
        bounds = first.get('selection_bounds')
        assert isinstance(bounds, dict), first
        rect = (bounds['x'], bounds['y'], bounds['width'], bounds['height'])
        again = call('transform_again', document_id=doc36)
        assert again.get('ok') is True, again
        assert_moved(doc36, selected_moved(px, rect, (4, 1)), width, height, 'transform_again')
        copy_again = call('transform_again', fail=True, document_id=doc36, duplicate=True)
        assert copy_again == COPY, copy_again
        call('deselect', document_id=doc36)
        receipts['i36'] = {'first': first.get('selection_bounds'), 'copy': copy_again}
        print('PASS i36: the selected pixels move exactly, the hole clear; Transform Again '
              'repeats it; a copy refuses', flush=True)

        # ---- I37: Warp -----------------------------------------------------------
        doc37, _ = fresh(clear_bands, 'clear-bands-37.png', transparency=True)
        table37 = table_of(doc37)
        warped = call('warp_layer', document_id=doc37, layer=0, operation='set', target='pixels',
                      columns=2, rows=2, filter='bicubic',
                      displacements=[[0, 0], [1.5, 0.5], [0.5, 1.0], [0, 0]])
        assert warped.get('changed') is True and warped.get('field') is None, warped
        on_table(doc37, table37, 'warp')
        assert call('get_warp', document_id=doc37, layer=0)['field'] is None
        held = render(doc37)
        cleared = call('warp_layer', document_id=doc37, layer=0, operation='clear')
        assert cleared.get('changed') is False, cleared
        assert render(doc37) == held
        receipts['i37'] = {'set': warped.get('changed'), 'clear': cleared.get('changed')}
        print('PASS i37: a warp lands on the palette and keeps no field; clear changes nothing',
              flush=True)

        # ---- I38: the layer row --------------------------------------------------
        doc38, _ = fresh(clear_bands, 'clear-bands-38.png', transparency=True)
        held = render(doc38)
        call('set_layer_properties', document_id=doc38, index=0, name='Plate')
        call('set_layer_label', document_id=doc38, layer=0, label='red')
        call('set_layer_lock', document_id=doc38, layer=0, locks=['pixels'])
        locked = call('brush_stroke', fail=True, document_id=doc38, layer=0,
                      points=[[3, 6], [27, 6]], size=7, rgba=[c / 255 for c in INK] + [1])
        assert 'lock' in locked.lower(), locked
        call('set_layer_lock', document_id=doc38, layer=0, locks=[])
        props = call('set_layer_properties', fail=True, document_id=doc38, index=0, opacity=0.5)
        assert props == PROPS, props
        assert render(doc38) == held
        row_rz = folder / 'row.rz'
        call('save_copy', document_id=doc38, path=str(row_rz), format='rz')
        doc38b = opened(row_rz)
        layer = call('get_document', document_id=doc38b)['layers'][0]
        assert (layer['name'], layer['label']) == ('Plate', 'red'), layer
        assert mode(doc38b) == 'indexed' and render(doc38b) == held
        receipts['i38'] = {'locked': locked, 'props': props,
                           'reopened': (layer['name'], layer['label'])}
        print('PASS i38: rename, label and lock keep the pixels; a pixel lock refuses the Brush; '
              'opacity refuses; .rz keeps the row', flush=True)

        # ---- I39: Export Layers --------------------------------------------------
        i39_png = folder / 'palette-39.png'
        i39_png.write_bytes(palette_png.read_bytes())
        doc39 = opened(i39_png)
        layers_out = folder / 'layers-39'
        layers_out.mkdir()
        call('export_layers', document_id=doc39, items='all', destination=str(layers_out),
             pattern='{layer}', settings={'format': 'png'})
        written = sorted(layers_out.glob('*.png'))
        assert len(written) == 1, written
        doc39b = opened(written[0])
        assert mode(doc39b) == 'indexed', summary(doc39b)
        assert render(doc39b) == receipts['i1']['render']
        receipts['i39'] = {'file': written[0].name}
        print('PASS i39: Export Layers writes the Index layer keeping the palette', flush=True)

        # ---- I40: Move (phase 3a) --------------------------------------------------
        doc40, _ = fresh(clear_bands, 'clear-bands-40.png', transparency=True)
        w, h, px = sized(doc40)
        moved40 = call('move_layers', document_id=doc40, layers=[0], dx=3, dy=1)
        assert moved40.get('ok') is True, moved40
        after40 = assert_moved(doc40, moved_render(w, h, px, w, h, lambda x, y: (x - 3, y - 1)),
                               w, h, 'move_layers')
        layer40 = call('get_document', document_id=doc40)['layers'][0]
        assert (layer40['offset_x'], layer40['offset_y']) == (0, 0), layer40
        call('undo', document_id=doc40)
        assert render(doc40) == px
        call('redo', document_id=doc40)
        assert render(doc40) == after40
        call('undo', document_id=doc40)
        copy40 = call('move_layers', fail=True, document_id=doc40, layers=[0], dx=3, dy=1,
                      duplicate=True)
        assert copy40 == COPY, copy40
        offset40 = call('set_layer_properties', document_id=doc40, index=0, offset_x=2)
        assert offset40.get('ok') is True, offset40
        assert_moved(doc40, moved_render(w, h, px, w, h, lambda x, y: (x - 2, y)), w, h,
                     'set_layer_properties offset_x')
        receipts['i40'] = {'move': moved40, 'layer': layer40, 'copy': copy40}
        print('PASS i40: move_layers and the row offset move every index exactly, the layer '
              'kept at the origin; a copy move refuses', flush=True)

        # ---- I41: selected pixels (phase 3a) ---------------------------------------
        doc41, _ = fresh(clear_bands, 'clear-bands-41.png', transparency=True)
        px = render(doc41)
        call('select_rect', document_id=doc41, x=6, y=2, width=6, height=4)
        moved41 = call('move_selected_pixels', document_id=doc41, layer=0, dx=4, dy=1)
        assert moved41.get('ok') is True, moved41
        assert_moved(doc41, selected_moved(px, (6, 2, 6, 4), (4, 1)), width, height,
                     'move_selected_pixels')
        red = call('move_selected_pixels', fail=True, document_id=doc41, layer=0, dx=1, dy=0,
                   target='red')
        assert red == INDEX_CHANNEL, red
        mask = call('transform_mask', fail=True, document_id=doc41)
        assert mask == DISABLED.format('transform_mask'), mask
        call('deselect', document_id=doc41)
        receipts['i41'] = {'move': moved41.get('selection_bounds'), 'red': red, 'mask': mask}
        print('PASS i41: the selected pixels move exactly, the hole clear; a colour plane and a '
              'mask refuse', flush=True)

        # ---- I42: the background rule (phase 3a) ------------------------------------
        doc42, indexed42 = fresh(bands_png, 'bands-42.png')
        assert indexed42['transparent_index'] is None, indexed42
        table42 = table_of(doc42)
        bg42 = table42[nearest(table42, (255, 255, 255))] + (255,)
        px = render(doc42)
        call('select_rect', document_id=doc42, x=0, y=0, width=4, height=4)
        cleared42 = call('clear_selection', document_id=doc42, layer=0)
        assert cleared42.get('changed') is True, cleared42
        expected = [bg42 if (i % width < 4 and i // width < 4) else p for i, p in enumerate(px)]
        assert_moved(doc42, expected, width, height, 'clear_selection with no transparent index')
        px = expected
        call('select_rect', document_id=doc42, x=4, y=2, width=4, height=4)
        moved42 = call('move_selected_pixels', document_id=doc42, layer=0, dx=6, dy=4)
        assert moved42.get('ok') is True, moved42
        expected = list(px)
        for y in range(2, 6):
            for x in range(4, 8):
                expected[y * width + x] = bg42
        for y in range(2, 6):
            for x in range(4, 8):
                nx, ny = x + 6, y + 4
                if nx < width and ny < height:
                    expected[ny * width + nx] = px[y * width + x]
        assert_moved(doc42, expected, width, height, 'move_selected_pixels hole')
        call('deselect', document_id=doc42)
        receipts['i42'] = {'background': bg42, 'clear': cleared42,
                           'move': moved42.get('selection_bounds')}
        print('PASS i42: with no transparent index Clear and the Move tool\'s hole take the '
              'entry nearest white', flush=True)

        # ---- I43: the layer row (phase 3a) ------------------------------------------
        doc43, _ = fresh(clear_bands, 'clear-bands-43.png', transparency=True)
        held = render(doc43)
        hidden = call('set_layer_properties', document_id=doc43, index=0, visible=False)
        assert hidden.get('ok') is True, hidden
        assert all(p[3] == 0 for p in render(doc43))
        hidden_export = call('export_image', fail=True, document_id=doc43,
                             path=str(folder / 'hidden.png'), settings={'format': 'png'})
        assert hidden_export == HIDDEN, hidden_export
        hidden_rz = folder / 'hidden.rz'
        call('save_copy', document_id=doc43, path=str(hidden_rz), format='rz')
        doc43b = opened(hidden_rz)
        assert mode(doc43b) == 'indexed', summary(doc43b)
        assert call('get_document', document_id=doc43b)['layers'][0]['visible'] is False
        shown = call('set_layer_properties', document_id=doc43, index=0, visible=True)
        assert shown.get('ok') is True, shown
        assert render(doc43) == held
        opened43 = call('set_layer_properties', fail=True, document_id=doc43, index=0, open=True)
        assert opened43 == GROUPS_ONLY, opened43
        receipts['i43'] = {'export': hidden_export, 'open': opened43}
        print('PASS i43: the layer hides and shows, a hidden layer refuses a kept export and '
              'saves hidden; open applies to groups', flush=True)

        # ---- I44: one layer (phase 3a) ----------------------------------------------
        i44 = {}
        for tool, sentence in (('distribute_layers', ONE_LAYER), ('delete_layer', ONE_LAYER),
                               ('flatten_image', ONE_LAYER), ('hdr_toning', DISABLED)):
            got = call(tool, fail=True, document_id=doc43)
            assert got == sentence.format(tool), (tool, got)
            i44[tool] = got
        receipts['i44'] = i44
        print('PASS i44: Distribute, Delete Layer and Flatten Image need more than one layer; HDR '
              'Toning refuses for good', flush=True)

        # ---- I45: Assign and Convert to Profile (phase 3a) ---------------------------
        doc45, indexed45 = fresh(clear_bands, 'clear-bands-45.png', transparency=True)
        table45 = table_of(doc45)
        profile45 = call('get_color_profile', document_id=doc45)
        assigned = call('assign_profile', document_id=doc45, profile='display_p3')
        assert assigned.get('changed', True) is not False, assigned
        assert table_of(doc45) == table45
        profile_after = call('get_color_profile', document_id=doc45)
        assert profile_after != profile45, (profile45, profile_after)
        call('undo', document_id=doc45)
        assert call('get_color_profile', document_id=doc45) == profile45
        converted = call('convert_profile', document_id=doc45, profile='display_p3')
        assert converted.get('changed') is True, converted
        plan45 = converted.get('working_memory_plan_bytes')
        assert isinstance(plan45, int) and plan45 > 0, converted
        assert mode(doc45) == 'indexed', summary(doc45)
        assert call('get_document', document_id=doc45)['indexed'] == indexed45
        table_after = table_of(doc45)
        assert len(table_after) == len(table45) and table_after != table45, table_after
        call('undo', document_id=doc45)
        assert table_of(doc45) == table45
        assert GENERIC_CMYK.is_file(), GENERIC_CMYK
        not_rgb = {}
        for tool in ('assign_profile', 'convert_profile'):
            got = call(tool, fail=True, document_id=doc45, profile='file', path=str(GENERIC_CMYK))
            assert HOST_NOT_RGB in got, (tool, got)
            not_rgb[tool] = got
        receipts['i45'] = {'assign': assigned, 'convert_plan': plan45, 'table': table_after,
                           'not_rgb': not_rgb}
        print('PASS i45: Assign Profile keeps the table, Convert to Profile converts it and keeps '
              'every index, one undo step each; a non-RGB profile refuses', flush=True)

        # ---- I46: the other blend modes (phase 3b) -----------------------------------
        i46 = {}
        # The stroke runs along pixel centres, so no edge pixel's coverage sits at one half,
        # where the 8-bit twin's rasterization and the 16-bit storage's can fall either side of
        # THRESH's 131/255 (drive-1: a stroke along y 6 put row 2 at exactly radius 3.5).
        cases46 = (
            ('brush screen', 'screen',
             lambda d: call('brush_stroke', document_id=d, layer=0,
                            points=[[3.5, 6.5], [26.5, 6.5]], size=7, color=ink_hex,
                            blend_mode='screen')),
            ('fill difference', 'difference',
             lambda d: call('fill', document_id=d, layer=0, x=25, y=6, color=ink_hex,
                            blend_mode='difference')),
        )
        for k, (name, blend, run) in enumerate(cases46):
            doc46, indexed46 = fresh(bands_png, f'bands-46-{k}.png')
            assert indexed46['transparent_index'] is None, indexed46
            table46 = table_of(doc46)
            before46 = render(doc46)
            twin46 = twin_of(doc46, f'bands-46-{k}.rz')
            run(twin46)
            reply46 = run(doc46)
            assert reply46.get('changed') is True, (name, reply46)
            expected46 = reindexed(table46, before46, render(twin46), INK, BLENDS[blend])
            got46 = render(doc46)
            bad = [i for i, (a, b) in enumerate(zip(got46, expected46)) if a != b]
            assert not bad, (name, bad[:8], [(got46[i], expected46[i]) for i in bad[:4]])
            moved46 = sum(a != b for a, b in zip(before46, got46))
            assert moved46 > 0, name
            assert mode(doc46) == 'indexed'
            call('undo', document_id=doc46)
            assert render(doc46) == before46, name
            i46[name] = {'moved': moved46}
        receipts['i46'] = i46
        print('PASS i46: the Brush in Screen and the Paint Bucket in Difference are the RGB op '
              're-indexed, one undo step each', flush=True)

        # ---- I47: the Select menu (phase 3b) -------------------------------------------
        split_png = folder / 'split.png'
        split_bands = [BANDS[0], BANDS[1], BANDS[0]]
        png(split_png, width, height, 8, 2,
            [v for y in range(height) for x in range(width) for v in split_bands[x // 10]])
        doc47, indexed47 = fresh(split_png, 'split-47.png')
        assert indexed47 == {'colors': 2, 'transparent_index': None}, indexed47
        held47 = render(doc47)
        everywhere = [{'x': x, 'y': y} for y in range(height) for x in range(width)]

        def coverage47():
            reply = call('get_selection', document_id=doc47, points=everywhere)
            samples = reply.get('samples', [])
            return [s['coverage16'] for s in samples] if samples else [0] * len(everywhere)

        def band_mask(keep):
            return [65535 if keep(i % width) else 0 for i in range(width * height)]

        # Color Range at the white entry, tolerance 0: that entry's pixels.
        call('select_color_range', document_id=doc47, range='sampled',
             samples16=[[65535, 65535, 65535]], tolerance=0, softness=0)
        assert coverage47() == band_mask(lambda x: 10 <= x < 20), 'color range'
        # Grow (contiguous) and Similar from the top-left pixel: black's left band, then both.
        call('select_rect', document_id=doc47, x=0, y=0, width=1, height=1)
        call('select_similar', document_id=doc47, tolerance=0, contiguous=True)
        assert coverage47() == band_mask(lambda x: x < 10), 'grow'
        call('select_rect', document_id=doc47, x=0, y=0, width=1, height=1)
        call('select_similar', document_id=doc47, tolerance=0, contiguous=False)
        assert coverage47() == band_mask(lambda x: x < 10 or x >= 20), 'similar'
        # Modify and Transform Selection change the selection only.
        call('select_rect', document_id=doc47, x=12, y=4, width=4, height=3)
        rect47 = coverage47()
        call('modify_selection', document_id=doc47, operation='grow', radius=1)
        grown47 = coverage47()
        assert grown47 != rect47 and grown47[5 * width + 11] > 0, 'grow 1'
        call('transform_selection', document_id=doc47, translate_x=2)
        assert coverage47() != grown47, 'transform selection'
        assert render(doc47) == held47
        # Quick Mask on and off.
        call('set_quick_mask', document_id=doc47, active=True)
        call('set_quick_mask', document_id=doc47, active=False)
        assert render(doc47) == held47 and mode(doc47) == 'indexed'
        # Save Selection, then Load Selection: the same coverage.
        call('select_rect', document_id=doc47, x=2, y=2, width=5, height=4)
        saved47 = coverage47()
        call('save_selection', document_id=doc47, name='S47')
        call('deselect', document_id=doc47)
        call('load_selection', document_id=doc47, **{'from': 'channel'}, channel='S47')
        assert coverage47() == saved47, 'save then load'
        red47 = call('load_selection', fail=True, document_id=doc47, **{'from': 'plane'},
                     plane='red')
        assert red47 == INDEX_CHANNEL, red47
        call('deselect', document_id=doc47)
        assert render(doc47) == held47 and mode(doc47) == 'indexed'
        receipts['i47'] = {'red': red47}
        print('PASS i47: Color Range and Similar select by palette colour; Modify, Transform '
              'Selection and Quick Mask change only the selection; Save and Load Selection keep '
              'it; a colour plane refuses', flush=True)

        # ---- I48: the background colour over MCP (phase 3b) -----------------------------
        doc48, indexed48 = fresh(bands_png, 'bands-48.png')
        assert indexed48['transparent_index'] is None, indexed48
        table48 = table_of(doc48)
        key48 = (0x3C, 0x99, 0x18)
        entry48 = table48[nearest(table48, key48)] + (255,)
        bg48 = table48[nearest(table48, (255, 255, 255))] + (255,)
        assert entry48 != bg48, (entry48, bg48)
        w, h, px = sized(doc48)
        cropped48 = call('crop', document_id=doc48, x=-2, y=-1, width=w + 4, height=h + 2,
                         background_color='#3C9918')
        assert cropped48.get('ok', True) is not False, cropped48
        assert_moved(doc48, moved_render(w, h, px, w + 4, h + 2, lambda x, y: (x - 2, y - 1),
                                         fill=entry48), w + 4, h + 2, 'crop background_color')
        call('undo', document_id=doc48)
        assert sized(doc48) == (w, h, px)
        grown48 = call('canvas_size', document_id=doc48, width=w + 4, height=h + 4,
                       anchor='center')
        assert grown48.get('ok') is True, grown48
        assert_moved(doc48, moved_render(w, h, px, w + 4, h + 4, lambda x, y: (x - 2, y - 2),
                                         fill=bg48), w + 4, h + 4, 'canvas_size over MCP')
        receipts['i48'] = {'entry': entry48, 'white': bg48}
        print('PASS i48: over MCP a crop past the canvas takes the entry nearest its '
              'background_color; Canvas Size takes white\'s', flush=True)

        # ---- I49: for good (phase 3b) ---------------------------------------------------
        i49 = {}
        for tool, sentence in (('hdr_toning', DISABLED), ('auto_align_layers', ONE_LAYER),
                               ('auto_blend_layers', ONE_LAYER)):
            got = call(tool, fail=True, document_id=doc48)
            assert got == sentence.format(tool), (tool, got)
            i49[tool] = got
        receipts['i49'] = i49
        print('PASS i49: HDR Toning, Auto-Align and Auto-Blend refuse for good', flush=True)

        # ---- I50: what named phase 3c ----------------------------------------------------
        held50 = render(doc48)
        call('select_magic_wand', document_id=doc48, x=1, y=1, tolerance=0)
        everywhere48 = [{'x': x, 'y': y} for y in range(h) for x in range(w)]
        wand50 = call('get_selection', document_id=doc48, points=everywhere48).get('samples', [])
        assert any(s['coverage16'] > 0 for s in wand50), 'the Magic Wand selects'
        call('deselect', document_id=doc48)
        assert render(doc48) == held50
        channel50 = call('render', fail=True, document_id=doc48, channel='red')
        assert channel50 == RENDER_REFUSAL, channel50
        receipts['i50'] = {'render_channel': channel50}
        print('PASS i50: the Magic Wand selects; a red render channel refuses for good', flush=True)

        def render_data(doc, **arguments):
            """`render`'s PNG bytes (base64) with `arguments`."""
            result = app.client.rpc('tools/call', {'name': 'render',
                                                   'arguments': {'document_id': doc, **arguments}})
            assert not result.get('isError'), result
            return next(c['data'] for c in result['content'] if c['type'] == 'image')

        def coverage_of(doc, ww, hh):
            points = [{'x': x, 'y': y} for y in range(hh) for x in range(ww)]
            samples = call('get_selection', document_id=doc, points=points).get('samples', [])
            return [s['coverage16'] for s in samples] if samples else [0] * len(points)

        # ---- I51: the Pattern and Noise dithers (phase 3c) --------------------------------
        rw, rh = 16, 8
        ramp = [((x * 16 + y) & 255, (255 - x * 15 - y * 3) & 255, (y * 30 + x) & 255)
                for y in range(rh) for x in range(rw)]
        ramp_png = folder / 'ramp.png'
        png(ramp_png, rw, rh, 8, 2, [v for p in ramp for v in p])
        i51 = {}
        for k, (dither, threshold) in enumerate((('pattern', pattern_t), ('noise', noise_t))):
            copy51 = folder / f'ramp-51-{k}.png'
            copy51.write_bytes(ramp_png.read_bytes())
            doc51 = opened(copy51)
            reply51 = call('convert_mode', document_id=doc51, mode='indexed', palette='web',
                           dither=dither)
            assert reply51['color_mode'] == 'indexed', reply51
            table51 = table_of(doc51)
            got51 = render(doc51)
            expected51 = ordered(table51, ramp, rw, threshold)
            bad = [i for i, (a, b) in enumerate(zip(got51, expected51)) if a != b]
            assert not bad, (dither, bad[:8], [(got51[i], expected51[i]) for i in bad[:4]])
            call('undo', document_id=doc51)
            assert mode(doc51) == 'rgb', dither
            call('redo', document_id=doc51)
            assert mode(doc51) == 'indexed' and render(doc51) == got51, dither
            i51[dither] = {'colors': len(table51)}
        copy51 = folder / 'ramp-51-amount.png'
        copy51.write_bytes(ramp_png.read_bytes())
        amount51 = opened(copy51)
        refusal51 = call('convert_mode', fail=True, document_id=amount51, mode='indexed',
                         palette='web', dither='pattern', amount=50)
        assert refusal51 == DIFFUSION_ONLY, refusal51
        assert mode(amount51) == 'rgb'
        grey_png = folder / 'grey-ramp.png'
        png(grey_png, 16, 2, 8, 2, [v for i in range(32) for v in ((i % 16) * 17,) * 3])
        exact51 = {}
        for dither in ('none', 'pattern', 'noise'):
            copy = folder / f'grey-51-{dither}.png'
            copy.write_bytes(grey_png.read_bytes())
            doc = opened(copy)
            call('convert_mode', document_id=doc, mode='indexed', palette='exact', dither=dither)
            exact51[dither] = (table_of(doc), render(doc))
        assert exact51['pattern'] == exact51['none'] and exact51['noise'] == exact51['none']
        i51['amount'] = refusal51
        receipts['i51'] = i51
        print('PASS i51: Pattern and Noise follow their declared rule, one undo step; amount '
              'refuses with them; with Exact they keep every pixel\'s own entry', flush=True)

        # ---- I52: Forced Custom (phase 3c) --------------------------------------------------
        forced = [list(c) for c in FORCED]
        i52 = {}
        for k, (palette, colors) in enumerate((('uniform', 32), ('adaptive', 16))):
            copy = folder / f'ramp-52-{k}.png'
            copy.write_bytes(ramp_png.read_bytes())
            doc = opened(copy)
            call('convert_mode', document_id=doc, mode='indexed', palette=palette, colors=colors,
                 forced='custom', forced_table=forced, dither='none')
            table52 = table_of(doc)
            assert table52[:2] == FORCED, (palette, table52[:4])
            i52[palette] = len(table52)
        copy = folder / 'ramp-52-refused.png'
        copy.write_bytes(ramp_png.read_bytes())
        doc = opened(copy)
        refusal52 = call('convert_mode', fail=True, document_id=doc, mode='indexed',
                         palette='uniform', forced='web', forced_table=forced)
        assert refusal52 == FORCED_TABLE_ONLY, refusal52
        i52['refusal'] = refusal52
        receipts['i52'] = i52
        print('PASS i52: Forced Custom leads the Uniform and Adaptive palettes; forced_table '
              'refuses without it', flush=True)

        # ---- I53: the unpublished palettes (phase 3d) ------------------------------------
        i53 = {}
        for wire, title in UNPUBLISHED_PALETTES:
            got = call('convert_mode', fail=True, document_id=doc, mode='indexed', palette=wire)
            assert got == UNPUBLISHED.format(title), (wire, got)
            i53[wire] = got
        assert mode(doc) == 'rgb'
        receipts['i53'] = i53
        print('PASS i53: Local and Master Perceptual and Selective refuse for good, pointing to '
              'Adaptive', flush=True)

        # ---- I54: render channels (phase 3c) --------------------------------------------
        doc54, _ = fresh(bands_png, 'bands-54.png')
        composite54 = render_data(doc54)
        for channel in ('index', 'composite'):
            assert render_data(doc54, channel=channel) == composite54, channel
        call('select_rect', document_id=doc54, x=4, y=3, width=7, height=5)
        call('save_selection', document_id=doc54, name='S54')
        call('deselect', document_id=doc54)
        _, _, plane54 = png_pixels(base64.b64decode(render_data(doc54, channel='S54')))
        expected54 = [255 if 4 <= i % width < 11 and 3 <= i // width < 8 else 0
                      for i in range(width * height)]
        assert [p[0] for p in plane54] == expected54, 'alpha channel render'
        red54 = call('render', fail=True, document_id=doc54, channel='red')
        assert red54 == RENDER_REFUSAL, red54
        receipts['i54'] = {'red': red54}
        print('PASS i54: render channel index and composite are the composite; an alpha channel '
              'renders its coverage; red refuses', flush=True)

        # ---- I55: the Magic Wand, Quick Selection and the channel commands (phase 3c) ------
        doc55, _ = fresh(split_png, 'split-55.png')
        held55, table55 = render(doc55), table_of(doc55)
        call('select_magic_wand', document_id=doc55, x=0, y=0, tolerance=0, contiguous=False)
        assert coverage_of(doc55, width, height) == band_mask(lambda x: x < 10 or x >= 20), 'wand'
        call('quick_select', document_id=doc55, seeds=[[15, 5]])
        assert any(coverage_of(doc55, width, height)), 'quick select'
        call('deselect', document_id=doc55)
        assert render(doc55) == held55 and table_of(doc55) == table55

        def channel_count():
            return len(call('list_channels', document_id=doc55)['channels'])

        start55 = channel_count()
        steps55 = (
            ('add_channel', {'name': 'C55'}, 1),
            ('rename_channel', {'channel': 'C55', 'name': 'D55'}, 0),
            ('duplicate_channel', {'channel': 'D55'}, 1),
            ('set_channel_options', {'channel': 'D55', 'overlay_opacity': 0.3}, 0),
            ('invert_channel', {'channel': 'D55'}, 0),
            ('delete_channel', {'channel': 'D55 copy'}, -1),
        )
        expected_count = start55
        for tool, arguments, delta in steps55:
            call(tool, document_id=doc55, **arguments)
            expected_count += delta
            assert channel_count() == expected_count, tool
            assert render(doc55) == held55 and table_of(doc55) == table55, tool
            assert mode(doc55) == 'indexed', tool
        receipts['i55'] = {'channels': expected_count}
        print('PASS i55: the Magic Wand selects an entry\'s pixels, Quick Selection only the '
              'selection; the channel commands keep the render and the table', flush=True)

        # ---- I56: Clone Stamp and the History Brush (phase 3c) ----------------------------
        doc56, _ = fresh(bands_png, 'bands-56.png')
        table56 = table_of(doc56)
        before56 = render(doc56)
        cloned56 = call('clone_stamp', document_id=doc56, layer=0, points=[[13, 6], [17, 6]],
                        size=5, hardness=100, opacity=1, source_x=3, source_y=6)
        assert cloned56.get('changed', True) is not False, cloned56
        after56 = render(doc56)
        assert all(p[:3] in table56 for p in after56), 'every pixel an entry'
        covered = 0
        for i, pixel in enumerate(after56):
            # The pixel centre's distance from the stroke's segment (radius 2.5).
            cx, cy = i % width + 0.5, i // width + 0.5
            distance = math.hypot(max(13 - cx, 0, cx - 17), cy - 6)
            if distance <= 1.0:
                assert pixel == before56[i - 10], (cx, cy, pixel, before56[i - 10])
                covered += 1
            elif distance > 4.0:
                assert pixel == before56[i], (cx, cy)
        assert covered > 0
        call('undo', document_id=doc56)
        assert render(doc56) == before56, 'one undo step'
        new56 = call('clone_stamp', fail=True, document_id=doc56, layer=0, points=[[13, 6]],
                     source_x=3, source_y=6, output='new_layer')
        assert new56 == DISABLED.format('add_layer'), new56
        assert render(doc56) == before56
        state56 = call('list_history', document_id=doc56)['current_state_id']
        call('brush_stroke', document_id=doc56, layer=0, points=[[3, 6], [27, 6]], size=7,
             rgba=[c / 255 for c in INK] + [1])
        assert render(doc56) != before56
        call('history_brush_stroke', document_id=doc56, layer=0, points=[[3, 6], [27, 6]],
             size=40, hardness=100, opacity=100, state_id=state56)
        assert render(doc56) == before56, 'the History Brush restores the earlier render'
        assert mode(doc56) == 'indexed'
        receipts['i56'] = {'covered': covered, 'new_layer': new56}
        print('PASS i56: Clone Stamp copies the source entries, one undo step; onto a new layer '
              'it refuses; the History Brush restores the earlier state', flush=True)

        # ---- I57: for good (phase 3c) ------------------------------------------------------
        i57 = {}
        for tools, sentence in ((DISABLED_3C, DISABLED), (ONE_LAYER_3C, ONE_LAYER),
                                (RGB_ONLY_3C, RGB_ONLY)):
            for tool in tools:
                got = call(tool, fail=True, document_id=doc56)
                assert got == sentence.format(tool), (tool, got)
                i57[tool] = got
        receipts['i57'] = i57
        print(f'PASS i57: {len(DISABLED_3C)} tools are not available in Indexed Color, '
              f'{len(ONE_LAYER_3C)} need more than one layer, and {len(RGB_ONLY_3C)} are '
              'offered on RGB only', flush=True)

        # ---- I58: the admitted tools answer no gate sentence (phase 3c) --------------------
        doc58, _ = fresh(bands_png, 'bands-58.png')
        i58 = {}
        for tool in ADMITTED_3C:
            result = app.client.rpc('tools/call', {'name': tool,
                                                   'arguments': {'document_id': doc58}})
            text = next((c['text'] for c in result.get('content', []) if c['type'] == 'text'), '')
            assert not any(mark in text for mark in GATE_MARKS), (tool, text)
            i58[tool] = {'error': bool(result.get('isError')), 'text': text[:160]}
        receipts['i58'] = i58
        print(f'PASS i58: the {len(ADMITTED_3C)} tools phase 3c admits answer no gate sentence',
              flush=True)

        # ---- I59: the close's catalog sweep ------------------------------------------------
        doc59, _ = fresh(bands_png, 'bands-59.png')
        listed59 = sorted(t['name'] for t in app.client.rpc('tools/list', {})['tools'])
        counts59 = {'disabled': 0, 'one_layer': 0, 'rgb_only': 0, 'duotone': 0}
        mismatched59 = []
        phased59 = []
        for tool in listed59:
            if tool in INDEXED_TOOLS or tool in DOCUMENT_FREE:
                continue
            result = app.client.rpc('tools/call', {'name': tool,
                                                   'arguments': {'document_id': doc59}})
            text = next((c['text'] for c in result.get('content', []) if c['type'] == 'text'), '')
            if 'phase' in text.lower():
                phased59.append((tool, text))
            if tool in DUOTONE_TOOLS:
                kind, want = 'duotone', None
            elif tool in DISABLED_ALL:
                kind, want = 'disabled', DISABLED.format(tool)
            elif tool in ONE_LAYER_ALL:
                kind, want = 'one_layer', ONE_LAYER.format(tool)
            else:
                kind, want = 'rgb_only', RGB_ONLY.format(tool)
            if not result.get('isError') or (want is not None and text != want):
                mismatched59.append((tool, bool(result.get('isError')), text[:200]))
            counts59[kind] += 1
        assert not mismatched59, mismatched59
        assert not phased59, phased59
        precision59 = call('get_precision', document_id=doc59)
        phase_keys59 = [k for k in precision59 if 'phase' in k.lower()]
        assert not phase_keys59, phase_keys59
        receipts['i59'] = {'listed': len(listed59), 'counts': counts59}
        print(f'PASS i59: of {len(listed59)} catalog tools, {counts59["disabled"]} are not '
              f'available in Indexed Color, {counts59["one_layer"]} need more than one layer, '
              f'{counts59["rgb_only"]} are offered on RGB only and {counts59["duotone"]} are '
              "Duotone's; no reply and no get_precision key names a phase", flush=True)

        # ---- I60: the Indexed Color PSD and PSB (#1) -----------------------------------
        palette60 = folder / 'palette-60.png'
        trns60 = bytes(0 if i == CLEAR else 255 for i in range(3))
        png(palette60, 4, 2, 8, 3, INDICES,
            extra=(chunk(b'PLTE', bytes(v for p in PALETTE for v in p)), chunk(b'tRNS', trns60)))
        doc60 = opened(palette60)
        table60 = call('get_color_table', document_id=doc60)
        tiff60 = call('inspect_psd_export', document_id=doc60, format='tiff', mode='flattened')
        assert tiff60['available'] is False and tiff60['refusal'] == PSD_TIFF, tiff60
        i60 = {'tiff': tiff60['refusal']}
        for kind in ('psd', 'psb'):
            where = folder / f'indexed-60.{kind}'
            written = call('export_psd', document_id=doc60, path=str(where), format=kind,
                           mode='flattened')
            assert written['bit_depth'] == 8, written
            inspected = call('inspect_psd', path=str(where))
            facts = {k: inspected[k] for k in ('import_precision', 'color_mode',
                                               'layered_available',
                                               'stored_composite_available')}
            assert facts == {'import_precision': 'indexed', 'color_mode': 'Indexed',
                             'layered_available': False,
                             'stored_composite_available': True}, inspected
            back = call('open_document', path=str(where), psd_mode='composite',
                        psd_fingerprint=inspected['source_fingerprint'])['document']['id']
            assert mode(back) == 'indexed', summary(back)
            table = call('get_color_table', document_id=back)
            assert (table['colors'], table['transparent_index']) == \
                (table60['colors'], table60['transparent_index']), (table, table60)
            png60 = folder / f'indexed-60-{kind}.png'
            call('export_image', document_id=back, path=str(png60), settings={'format': 'png'})
            plte60, _, indices60 = palette_png_parts(png60.read_bytes())
            assert plte60 == PALETTE and indices60 == INDICES, (plte60, indices60)
            call('close_document', document_id=back, discard_changes=True)
            i60[kind] = {'bytes': where.stat().st_size, 'losses': written.get('losses'),
                         'plan': written.get('working_memory_plan_bytes')}
        call('close_document', document_id=doc60, discard_changes=True)
        receipts['i60'] = i60
        print('PASS i60: the Indexed Color PSD and PSB keep the palette, the transparent index '
              'and every index through inspect_psd and open_document; tiff refuses', flush=True)

    (output / 'receipts.json').write_text(json.dumps(receipts, indent=2, default=str))
    print('ALL GATES PASS', flush=True)


if __name__ == '__main__':
    main()
