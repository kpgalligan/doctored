# Phase 10C-2 — per-construct PSD exchange rules

Slice 2 of 10C fixes ONE rule per Photoshop construct, on both directions and
both depths. Slice 1 shipped the rules that need no descriptor codec; slice 2
phase 1 (below) carries the eight binary-body adjustment kinds through ONE
codec, and phase 2 extends that codec to Exposure, Vibrance and Black & White
through ONE bounded descriptor reader/writer. **Slice 3** (its own section
below) PRESERVES layer effects (`lfx2`), fill opacity (`iOpa`) and Blend If
(the record's blending ranges) in both directions at both depths through ONE
further codec. Nothing here reads or writes a `TySh` descriptor, and `SoCo`
stays omitted because Rasterize has no fill adjustment to carry it as.

The rule is never silent: every cell below that is not "preserve" is a sentence
in the import or export report, which the user sees in the compatibility sheet
and the model sees through `inspect_psd` / `inspect_psd_export`.

## The rule table

| construct | import 8-bit | import 16-bit | export 8-bit | export 16-bit |
|---|---|---|---|---|
| layer style (effects, fill opacity, Blend If) | PRESERVE as one Rasterize style on a raster or a folder + observation; a body Rasterize cannot render is DISCARDED + loss with the reason (slice 3) | same, except a style a typed layer cannot carry (Pass Through folder, adjustment record) is DISCARDED + loss, and the file's global light resolves into each effect | PRESERVE as `lfx2` + `iOpa` + blending ranges, on rasters and folders, + observation; only a style Photoshop's dialog or a PSD mask cannot hold is RASTERIZED + loss with the reason (a group's cannot be, so it REFUSES) | same as 8-bit, with the default light |
| adjustment record the codec reads (binary bodies `nvrt`, `thrs`, `post`, `levl`, `curv`, `blnc`, `hue2`, `phfl`, `expA`; descriptor bodies `vibA`, `blwh`) | PRESERVE as a Rasterize adjustment layer (mask, opacity, blend, clipping) + observation; a body outside the mapped subset OMIT + loss with the reason | PRESERVE as a typed adjustment layer (mask, opacity, blend key, clipping) — since the [adjustment-blending slice](phase-10d-adjustment-blending-status.md) a typed adjustment carries the same blend keys a raster does (native v37/schema7), so a recognized key is no longer a loss | written as that record + observation (`invert`, `threshold`, `posterize`, `levels`, `curves`, `color_balance`, `hue_saturation`, `photo_filter`, `exposure`, `vibrance`, `black_and_white`) | same |
| any other adjustment / fill record (no pixels) | OMIT + loss | OMIT + loss | REFUSE (the op is named) | REFUSE |
| empty record (no pixels, no adjustment key) | canvas-sized transparent raster + observation | same, typed | written as pixels | same |
| text/shape/live-photo description, smart object, retained source | pixels + marker loss | same | pixels + loss | same |
| group clipping | PRESERVE (`Layer::clipped` on the folder) | PRESERVE (`PrecisionDocumentLayer::clipped`, since [10D clipping](phase-10d-clipping-status.md)) | written | written |
| Blend If ranges in the file | PRESERVE when exactly one of gray, red, green, blue is restricted and every other pair is default; anything else stays a loss (slice 3) | same | written as the record's 40-byte blending ranges | same |

What changed in slice 2: the group-clipping import cell (8-bit), the
adjustment/fill import cells (they refused the whole layered choice through a
malformed-rectangle error before), the empty-record import cells, and the
8-bit export style cell (it refused before). What changed in slice 3: the
layer-style and Blend If rows, in every column.

## The sentences

Import, per record, in `psd.layers()` order:

- Omission (loss): *Layer {index} (“{name}”) is a Photoshop adjustment or fill
  layer ({key}) with no pixels; layered import omits it and its mask, so the
  layers beneath it appear without that adjustment. The stored composite keeps
  the saved appearance.* The record's existing feature-marker loss still fires;
  its mask report and the outside-canvas observation do not, because neither
  the mask nor the rectangle is imported.
- Empty record (observation): *Layer {index} (“{name}”) has no pixels; layered
  import creates it as an empty canvas-sized layer.* Canvas-sized because byte
  painting never grows a layer, so a zero-sized one would be unpaintable.
- Nothing retained (observation): *Every layer record is an adjustment or fill
  layer with no pixels; Import Layers opens the stored composite as a single
  layer.* `GroupRecords::retains_nothing` drives BOTH the report's error fold
  and the `open`/`open16` branch, so the report can never advertise a layered
  choice the open then refuses.
- 16-bit clipped folder: no sentence any more either. At this slice's handoff
  the typed import reported *Group {id} (“{name}”) is clipped; it is imported
  unclipped (16-bit documents do not evaluate clipping)*; since
  [the 10D clipping slice](phase-10d-clipping-status.md) both depths keep the
  clipping byte on rasters and folders.

Export, per BAKED raster (ONE sentence, with up to three extra clauses; since
slice 3 it carries the reason the style could not be written):

> Layer “{name}” has a layer style ({effects}) whose {reason}; its effects are
> rasterized into the layer's pixels for this file. Effect blend modes are evaluated against
> transparency, so effects that blend with the layers beneath can look
> different.

plus *Its Blend If ranges are not written and were not applied.* when the style
has non-identity Blend If, *Its mask shapes the effects, so it is baked into
those pixels and not written as a separate mask.* when the layer's mask is
enabled, and *Layers clipped to it are written above the rasterized effects.*
when a visible clipped entry sits directly above it.

Export refusals, in the preflight's fixed order: canvas side → file size →
per-entry side / mask side → *Layer “{name}” with its rasterized style is {w} x
{h} pixels, past the 100 MP layer limit…* (a BAKED style only) → adjustment
layer → *Group “{name}” has a layer style whose {reason}; a group's effects
cannot be rasterized without flattening its contents…* → mask
density/feather.

## How the bake is defined

`psd_export_rasterize::rasterized_style` composites the layer into the extent
`style_composite::merge_extent` gives it, through the SHIPPED compositor
(`doc::composite_layer_into`, `doc::quantize`) over a transparent accumulator.
The baked copy has opacity 1, Normal blend and no Blend If, because those three
belong to the PACKAGE — the record's own opacity byte and blend key apply them
in Photoshop, exactly as `style_composite`'s package model describes; a Blend If
ramp against alpha 0 would otherwise zero the layer. An ENABLED mask is inside
the bake (`style_render::Shape::of_layer` shapes effects with
`Layer::active_mask`, and a separately written mask would apply twice); a
DISABLED one shapes nothing, so it is still written as an ordinary disabled PSD
mask — and the density/feather refusal therefore still runs on it.

The preflight forecasts the SAME rectangle (`RzDocument::style_extent`, pixel
free), so the reported worst-case size, the 100 MP check and the 2 GiB reopen
envelope all describe the file that is actually written.

## Adjustment bodies (slice 2, phases 1–2)

`core/src/psd_adjustment.rs` is the ONE codec, both directions, both depths.
Its module doc carries the byte layout of each of the eight records and the
parameter mapping in each direction; nothing else in the crate parses or writes
a body, and nothing else inserts the typed `evaluation` key
(`typed_description` is the only byte-form → typed adapter).

Import: `psd_group_records` keeps the FIRST adjustment/fill block per record
and, for a pixel-less raster record, decodes it. `Carried` clears the record's
ADJUSTMENT marker loss and imports as a canvas-sized adjustment layer at the
origin (`Layer.meta` at 8 bits, `PrecisionDocumentLayer::adjustment` at 16)
with its mask, opacity, blend key and clipping. A typed adjustment carries the
same blend keys a raster does since the
[adjustment-blending slice](phase-10d-adjustment-blending-status.md) (native
v37/`LBLD` schema7), so a recognized key is retained at BOTH depths and is no
longer a loss. `Declined`
(a mapped-form curve, a version this codec does not read, an XYZ photo filter,
per-channel levels, more than 64 posterize levels, a body shorter than its
layout, or values `Adjustment::from_meta` rejects) stays OMITTED with the
reason in the sentence; keys outside the eight keep the slice-1 sentence
byte-for-byte. Admission arithmetic is unchanged: a carried record charges the
canvas-sized raster exactly like an empty record.

Export: `ExportEntry.adjustment` carries the op and the encoded record; an op
the codec cannot write refuses the layered mode naming the op (same position in
the fixed refusal order), a carried one is a pixel-less record (zero rectangle,
four placeholder channels, its mask, opacity, blend key and clipping) with the
block written after `lsct`, and one observation per entry says its appearance
in Photoshop is not qualified. `curv`'s natural length is always odd, so the
codec pads it to even; every other body is its listed size.

Sentences:

- Import carried (observation): *Layer {index} (“{name}”) is a Photoshop {kind}
  adjustment layer ({key}); layered import carries it as a Rasterize {op}
  adjustment layer with its mask, opacity, blend mode and clipping. Its
  parameters are mapped onto Rasterize's own arithmetic, so its appearance can
  differ from Photoshop's.*
- Import declined (loss): *Layer {index} (“{name}”) is a Photoshop {kind}
  adjustment layer ({key}) with no pixels whose {reason}; layered import omits
  it and its mask, so the layers beneath it appear without that adjustment. The
  stored composite keeps the saved appearance.*
- Export carried (observation): *Adjustment layer “{name}” ({op}) is written as
  a Photoshop {kind} adjustment ({key}) with its mask, opacity, blend key and
  clipping; its parameters are mapped onto Photoshop's own arithmetic, so its
  appearance there is not qualified.*
- Export refusal: *Adjustment layer “{name}” ({op}) cannot be exported as a
  Photoshop adjustment; merge it down or export the flattened composite*.

Receipts (`core/tests/psd_adjustments.rs`, black box through the C API with
the independent 8- and 16-bit byte builders, hand-encoded bodies):

- An 8-bit fixture with one record per kind (asymmetric `hue2` band, `curv`
  rgb+r with its pad byte, `levl` gamma 150, `phfl` v2 with preserve 0, `thrs`
  128, `post` 4, `blnc`, `nvrt`; Levels masked, Posterize clipped, Balance
  Multiply): eight carried observations, no ADJUSTMENT loss, every meta equal
  to its expected JSON, mask/clipping/blend kept; a base + `thrs` file projects
  exactly as `rz_image_adjust_op` over the base.
- Declined variants (`post` 200, `curv` is_map 1, `levl` output levels, `phfl`
  v3, `hue2` v3, `thrs` 300, the falloff-200 `hue2` band, an empty `thrs`
  body): each exact reason clause, the marker loss still present, the `SoCo`
  sentence unchanged, only `Base` opened.
- The same records at 16 bits: every `adjustment_description` equals the typed
  expected string, the Multiply key imports as Multiply with no loss, clipping
  and the mask kept.
- An 8-bit document holding all eight ops with exactly representable
  parameters: available, eight observations, each `8BIM{key}` block equals the
  hand-encoded bytes (`curv` 18 bytes for the identity), `worst_case_bytes`
  covers the written length, reopened metas and `flattened()` equal; `curv`
  blocks for one, two and four channels are even and reopen.
- The same at 16 bits through `fixture16::read`: each record pixel-less with
  its block, reopened descriptions equal the originals.
- `bcs` refuses naming the op on both depths and leaves an existing file
  byte-identical.
- `tests/fixtures/psd_10c/*.psd` goldens unchanged; the marker tests that
  attach these keys to RASTERS are unchanged (the carry rule applies to
  pixel-less records only).

Manual receipt TAKEN 2026-09-16 (`~/devel/rasterize-fixtures/phase10-receipts/R1/receipt.json`, app SHA-256
`084b4be2b97b1c84b9749e861c066bec9b72abbefb627c37f7e7c9d7aa8052a6`): driving the built app over MCP, an 8-bit and then a 16-bit document
holding all eight kinds exported with one "is written as a Photoshop …
adjustment" observation each, `inspect_psd` reported the carry observation per
record and no "omits it" loss, and `open_document psd_mode: layers` reopened
every layer with its original parameters — the 16-bit ones read back from the
typed `.rz` operator descriptions, each `evaluation: precision_v1`. Re-runnable
as `scripts/verify_phase10_receipts.py --receipts R1`.

### Phase 2: `expA`, `vibA`, `blwh` through a descriptor reader

Phase 2 adds three kinds to the SAME codec (`KINDS` is eleven entries; the
carry path in `psd_group_records`, `psd_import`/`psd_import16`,
`psd_export`/`psd_export16` and the preflight is kind-agnostic and untouched).
`core/src/psd_descriptor.rs` is the ONE Photoshop descriptor-structure
reader/writer: version 16 then a descriptor (unicode name, class id, item
count, items of id + OSType + value). It reads `long`, `doub`, `bool`,
`TEXT`, `Objc`/`GlbO`, `UntF`, `enum` and `VlLs`; it walks and skips `tdta`,
`alis`, `comp`, `type`/`GlbC`; `obj `, `ObAr` and any other OSType decline the
body ("descriptor uses an item type this importer does not read"). Every
length is checked against the remaining bytes before use, truncation is the
shared "body is shorter than its layout", nesting past 16 and counts past
4096 decline, a version other than 16 declines ("descriptor version is not
16"), and a present key of the wrong type declines naming the key
("descriptor key vibrance has an unexpected type"). The writer emits a
four-byte id as `len = 0` + the bytes, unicode with a trailing 0, and name ""
on every descriptor. `psd_adjustment` owns the meaning of every class and key;
the descriptor module is framing only.

Two decisions recorded here:

- `expA` is a BINARY body per Adobe's table (`u16 version = 1`, three `f32`,
  2 pad) — the phase-2 plan's "descriptor" label for it was this document's
  error. Its import authors `"evaluation":"document_v2"` in the params, so
  the same record evaluates identically at 8 and 16 bits (through the
  document profile) and matches the schema rule that newly authored RGB
  exposure edits are calibrated; export ignores the key, so a `legacy_srgb`
  layer and a `document_v2` layer write identical bytes and reopen as
  `document_v2`.
- `SoCo` (solid colour fill) stays OMIT + loss: Rasterize has no fill
  adjustment op, so there is nothing to carry it as. Its sentence is
  byte-for-byte the slice-1 one. Measured on a Photoshop-authored file in Run
  4: at 16 bits Photoshop writes the fill record pixel-less, so it IS omitted
  with that sentence; at 8 bits it writes a canvas-sized RLE raster with the
  `SoCo` block, so the record keeps its pixels and costs the marker and
  irrelevant-pixels losses instead.

Mappings (`vibA`: `vibrance`/`Strt` longs ÷ 100; `blwh`: the six `Rd  `…
`Mgnt` longs ÷ 100 in `adjust.rs` order, `useTint`, `tintColor` as an `RGBC`
object of three doubles 0…255 → `tint_color`, `bwPresetKind`/file name
ignored on import and written as 1/"") and written sizes (`expA` 16, `vibA`
58, `blwh` 294 — even, so no pad) are in the codec's module-doc table.

Receipts (`core/tests/psd_adjustment_descriptors.rs`, black box through the C
API with the independent 8- and 16-bit byte builders and the test file's OWN
descriptor builder, never the crate's writer):

- An 8-bit fixture with the three records (`expA` 1.5/−0.25/2.0, `vibA`
  40/−20, `blwh` 30/−50/120/60/250/0 with tint 153/138/102; Black & White
  masked, Vibrance clipped, Exposure Multiply): three carried observations, no
  ADJUSTMENT loss, exact metas (the exposure meta carries `document_v2`),
  mask/clipping/blend kept, canvas-sized at the origin; a base + `vibA` file
  projects exactly as `rz_image_adjust_op("vibrance")` over the base (the
  parity case stays on `vibA` because that entry point refuses `document_v2`).
- A `vibA` whose descriptor carries a `TEXT`, an `enum`, a `UntF`, a `tdta`
  and a `VlLs` of longs before `vibrance` is carried with the same meta.
- Declined variants (`expA` version 2, a 10-byte `expA`, exposure 25, `vibA`
  version 15, `vibrance` as `doub`, vibrance 150, `Rd  ` 350, `tintColor` as
  a long, an `obj ` item, a descriptor truncated inside an item): each exact
  reason clause, the marker loss present, the `SoCo` sentence unchanged, only
  `Base` opened.
- The same records at 16 bits: typed descriptions equal (exposure params keep
  `document_v2`), Multiply imports as Multiply with no loss, mask and clipping
  kept.
- An 8-bit document with the three ops authored with every key import
  produces: available, three observations, each `8BIM{key}` block equals the
  hand-encoded bytes (16/58/294), `worst_case_bytes` covers the written
  length, reopened metas and `flattened()` equal; empty params write the
  default bytes (exposure 0/0/1, vibrance 0/0, the factory mix untinted with
  `#998a66`).
- The same at 16 bits through `fixture16::read`: pixel-less records with the
  blocks, reopened descriptions equal the originals.
- `bcs` still refuses on both depths with the unchanged sentence.
- `tests/fixtures/psd_10c/*.psd` goldens unchanged; `psd_adjustments.rs`
  (the eight-kind counts and the `SoCo` assertion) unchanged.

Manual receipt TAKEN 2026-09-16 for phase 2 as well
(`~/devel/rasterize-fixtures/phase10-receipts/R2/receipt.json`, app SHA-256 `084b4be2b97b1c84b9749e861c066bec9b72abbefb627c37f7e7c9d7aa8052a6`): the same drive with
`exposure`, `vibrance` and `black_and_white` at both depths carried every
parameter, and `exposure` reopened with `evaluation: document_v2` on 8-bit and
16-bit. Re-runnable as `scripts/verify_phase10_receipts.py --receipts R2`.

## Slice 3 — layer effects (`lfx2`), fill opacity (`iOpa`) and Blend If

`core/src/psd_layer_effects.rs` is the ONE codec for the three constructs, both
directions, both depths; `core/src/psd_layer_effects_fx.rs` holds the per-effect
descriptor reading and writing for the nine `Objc` classes, the `Grdn` gradient,
the `ShpC` contour and the `RGBC` colour. `psd_descriptor` stays framing only —
it gained `unit`, `enumeration` and `list` accessors and owns no class or key.
The codec's module doc carries the complete mapping table (every key, every
written-only default, the `BlnM` id table, the gradient shape, the ranges and
`iOpa` layouts); it is the spec, not this file.

Import: `psd_group_records` keeps each record's blending-ranges slice and its
FIRST `lfx2` and `iOpa` bodies, decodes them after the channel walk (the
adjustment pattern) and pushes one `StyleBody` into `layer_styles` /
`group_styles` — parallel vectors, so `Group` stays `Copy`. A `Carried` merges
the three constructs into ONE `LayerStyle` (built as style JSON and parsed with
`LayerStyle::from_value(_, Strict)`, so every number clamps and quantizes
exactly as any other style's) and clears exactly the markers it carried
(`FeatureMarkers::carry_style`); an identity result is `Absent` with the markers
untouched, so a 255 `iOpa` alone stays an observation. A body that is not an
effects descriptor at all (a leading word other than 0, an unreadable
descriptor, another top-level class) is `Absent` too: the record keeps its
presence-only marker sentence byte-for-byte. Stacked instances decline, because
an `lmfx` block beside the `lfx2` means more than one instance per effect and a
Rasterize style holds one; an `lrFX` beside it is Photoshop's legacy duplicate
and does not block the carry.

Export: `ExportStyle` gained `fill_opacity`, `blend_if`, `bake` and
`block_bytes`. `bake` is `bake_reason(style).or_else(mask_bake_reason)` — an
ENABLED stroke below Photoshop's 1 px minimum, an enabled satin outside its
1–250 px distance range, or an enabled mask with density or feather, which a
PSD mask cannot store. `None` PRESERVES: the record keeps the layer's own
planes and offset, writes its mask as an ordinary `-2` channel, and carries
`lfx2` (only when the style holds an ENABLED effect — a descriptor with none
would reopen as a bare marker, which is also what Photoshop 26.10.0 itself does
with one: Run 3 found it returns no `layerEffects` key at all. Run 5 moved the
four decision sites from `!effects.is_empty()` to
`LayerStyle::has_enabled_effects()`, so an all-disabled style now writes NO
`lfx2` and no image resources 1037/1049 at either depth while its fill opacity
and Blend If still travel — `core/tests/psd_disabled_effects.rs`), `iOpa` (only
below 100 %) and the 40-byte ranges. `Some`
takes today's bake path unchanged, including dropping the baked mask from the
entry, so the density/feather refusal still only ever sees a PLAIN mask beside
a preserved style. Image resources 1037/1049 are written iff at least one
`lfx2` is, and hold the byte document's global light (the typed document has
none, so 120/30).

Sentences (fixed; the fragments are what the tests match):

- Import carried (observation): *Layer {index} (“{name}”) has Photoshop layer
  effects ({effects}{, fill opacity {pct} %}{ and Blend If ranges on
  {channel}}); layered import carries them as a Rasterize layer style. Effect
  parameters are mapped onto Rasterize's own renderer and settings it does not
  model (contours, noise, technique, texture, pattern, jitter, range) are
  ignored, so its appearance can differ from Photoshop's.* (a folder reads
  *Group {id} (“{name}”)…*).
- Import declined (loss): *Layer {index} (“{name}”) has Photoshop layer effects
  whose {reason}; layered import discards them without applying them.* The
  record's unchanged marker loss stays beside it.
- On a carried adjustment record (loss): *Layer {index} (“{name}”) has
  Photoshop layer effects or Blend If ranges on an adjustment layer, which a
  Rasterize adjustment layer cannot carry; they are discarded.*
- Typed refusal (loss): *Layer {index} (“{name}”) has Photoshop layer effects
  that a 16-bit document cannot carry ({reason}); they are discarded.*
- Typed global light, ONCE per file (observation): *This file's global light is
  {a}°/{alt}°; a 16-bit document has no global light, so effects that used it
  are imported with that direction as their own.*
- Export preserved (observation): *Layer “{name}” has a layer style
  ({effects}{, fill opacity {pct} %}{, Blend If on {channel}}); it is written
  as Photoshop layer effects and blending options (lfx2, iOpa, blending
  ranges), so Photoshop renders them itself; its appearance there is not
  qualified.*
- The default `iOpa` observation splits out of the old one: *{target} has
  fill-opacity metadata at its 100 % default; its presence alone does not
  establish nondefault opacity.* Any other framing keeps today's sentence
  verbatim.

Decline reasons, read after "whose": *layer effects are switched off*
(`masterFXSwitch` false), *instances are stacked (lmfx), which a single-instance
Rasterize style cannot hold*, *stroke uses a pattern fill, which Rasterize does
not render*, *glow uses a gradient fill, which Rasterize does not render*,
*bevel uses Stroke Emboss, which Rasterize does not render*, *gradient is not a
custom colour gradient*, *gradient does not have between 2 and 32 colour
stops*, *gradient transparency stops do not match its colour stops*, and any
`psd_descriptor` or style-parser message (an unexpected item type, an
unrecognized enum id, a number the schema rejects).

Bake reasons: *stroke size is below Photoshop's 1 px minimum*, *satin distance
is outside Photoshop's 1–250 px range*, *enabled mask has density or feather,
which a PSD mask cannot store*.

Colour: a style colour is AUTHORED sRGB, so `→` converts it into the document's
space with `Transform::color` and `←` converts it back, at both depths (a
16-bit style's colours are still 8-bit hex).

Receipts (`core/tests/psd_layer_effects.rs`, black box through the C API, with
the independent 8- and 16-bit byte builders and this suite's own hand-encoded
descriptor builder — never the crate's writer):

- An authored 8-bit record with all nine classes (nondefault values, one
  disabled instance, Multiply/Screen ids, a gradient stroke and a radial
  gradient overlay, `Scl ` 50 halving every pixel quantity), a 16-byte ranges
  section restricting red, `iOpa` 128 and image resource 1037 = 45: the carry
  observation naming all nine plus the fill and the ranges, NO loss at all,
  `Layer::style.to_json()` equal to the independently written expected style,
  and `global_light` 45°/30°.
- Eight declined variants (a pattern stroke, a gradient glow, Stroke Emboss, a
  transparency list that does not match, a one-stop gradient, `masterFXSwitch`
  false, `Md  ` written as a `long`, and the same body beside an `lmfx` block):
  each exact reason, the marker loss still beside it, and no style on the
  opened layer.
- Four bodies that are not effects descriptors (`[255; 4]`, a v15 descriptor, a
  top-level `DrSh` class, a nonzero leading word): exactly ONE loss, the
  unchanged marker sentence.
- Ranges on two channels, on alpha, and a 4-byte section: the unchanged
  RANGES loss and no style; `iOpa` `[127; 4]`: the unchanged opaque
  observation.
- A Display P3 file's effect colour reopens as the reverse conversion of a 1×1
  `RzDocument::convert_to_profile` oracle.
- The same authored record at 16 bits: `reported_style` equal to the byte
  style with `use_global_light` turned off, the ONE global-light observation,
  no loss; a styled Pass Through folder and a styled adjustment record both
  discarded with the typed reason.
- An sRGB 8-bit document (all nine effects, fill opacity 153/255, a gray Blend
  If, a plain enabled mask, global light 137°/52°): available, the preserve
  observation, the `8BIMlfx2` body equal to the hand-encoded bytes key for key,
  `8BIMiOpa` = `[153, 0, 0, 0]`, the 40-byte ranges, resources 1037 = 137 and
  1049 = 52, `worst_case_bytes` covering the written length, and a reopen whose
  `to_json` and global light are identical.
- The same document under Display P3: every written `RGBC` triple equals the
  1×1 `convert_to_profile` oracle, and every non-colour field survives the two
  conversions.
- The same style on a 16-bit document: the same three constructs, resources
  1037 = 120 / 1049 = 30, and a reopen whose `reported_style` is identical.
- A satin distance of 300, a stroke size of 0.5 and an enabled mask with
  feather 1.5: the rasterize loss carrying each reason, no `lfx2`, no 1037,
  and no style on reopen.
- An unstyled document writes no `lfx2`, no `iOpa`, no ranges and no 1037/1049,
  and still loses its global light; `tests/fixtures/psd_10c/*.psd` goldens are
  byte-identical.

`psd_constructs.rs` additionally covers the styled GROUP (written, reopened
styled), the preserve case with a plain enabled mask, the disabled-mask case
and the still-baking feathered mask; `psd_export_corpus.rs`'s `styled_effects`
case (renamed from `styled_bake`) is the committed corpus receipt.

Manual receipt TAKEN 2026-09-16 (`~/devel/rasterize-fixtures/phase10-receipts/R3/receipt.json`, app SHA-256
`084b4be2b97b1c84b9749e861c066bec9b72abbefb627c37f7e7c9d7aa8052a6`): at both depths `inspect_psd_export` reported *has a layer style
(drop_shadow, stroke, color_overlay, fill opacity 50 %, Blend If on gray) … is
written as Photoshop layer effects and blending options*, the write carried no
rasterize loss, `inspect_psd` reported the carry observation naming all three
constructs, and `get_document` on the reopened document showed the three
effects with the identical Blend If and the fill opacity byte 128 (`iOpa` is
one byte, so 0.5 reopens as 0.502). The bake case — a 0.5 px stroke — reported
*stroke size is below Photoshop's 1 px minimum; its effects are rasterized into
the layer's pixels* and reopened with no style at all. Re-runnable as
`scripts/verify_phase10_receipts.py --receipts R3`.

## Receipts

### Manual receipts (taken 2026-09-16)

Code and unit coverage for slices 1–3 were complete at this slice's handoff;
the four manual receipts that stood between them and "closed" were taken on
2026-09-16 against the Release app SHA-256 `084b4be2b97b1c84b9749e861c066bec9b72abbefb627c37f7e7c9d7aa8052a6`, under
Run 1 of the closeout (2026-09-16),
and their rows are deleted from this ledger. Each is re-runnable:

| # | Receipt | Slice | Driver | Artifact |
|---|---|---|---|---|
| 1 | MCP drive: binary-body adjustment carry | 2, phase 1 | `scripts/verify_phase10_receipts.py --receipts R1` | `~/devel/rasterize-fixtures/phase10-receipts/R1/receipt.json` |
| 2 | MCP drive: descriptor adjustment carry | 2, phase 2 | `… --receipts R2` | `…/phase10-receipts/R2/receipt.json` |
| 3 | MCP drive: `lfx2` / `iOpa` / Blend If carry | 3 | `… --receipts R3` | `…/phase10-receipts/R3/receipt.json` |
| 4 | GIMP 3.2.6 reopen of the regenerated `styled_effects` corpus case | 3 (re-opens a 10B gate) | `scripts/psd-exchange-probe/run_reopen.py` then `scripts/verify_phase10b_external.py` | `…/phase10-receipts/R4/gimp-state/receipt.json` and `…/R4/external/result.json` |

Receipt 4's finding, restated where it belongs in
[the 10B export status](phase-10b-export-status.md): GIMP 3.2.6 attached NO
filter from the record's `lfx2` — every snapshot row's new `filters` list is
empty and its log carried no effects message — so its reopened projection is
the BARE layer stack, 151/128/130/0 codes from the styled render. That number
and `projection_vs_reopened` are RECORDED, never gated; gates 1, 2 and 4 pass
in full and the re-export decodes identically with `composite`/
`composite_encoded` excluded at 1 code. The regenerated `gimp.json`,
`merged.*` and `reopened.*` are committed under
`core/tests/fixtures/psd_10b/styled_effects/`; the other six cases' fresh
outputs were byte-identical to the committed ones apart from the additive
`filters` key, so they were not recommitted and their receipts stand.

`core/tests/psd_constructs.rs`, black box through the C API, with independent
byte fixtures (`common/psd_mask_fixture.rs`, `common/psd16_fixture.rs`) and
hand-computed pictures:

- An 8-bit 2 × 1 fixture — base, a clipped masked pass-through folder holding
  one opaque raster, a `curv` record with a mask, a `SoCo` record, an empty
  record — reports both omissions and the empty-layer observation, no
  "rectangle" refusal and no outside-canvas observation; opens as
  `[Base, Art, Outer, Empty]` with the folder clipped and masked, the empty
  layer a 2 × 1 transparent raster at (0, 0), and a projection equal to the
  hand-computed source-over (left pixel `Art` over `Base`, right pixel empty
  because the folder is clipped to `Base`).
- A pixel-less record whose raw plane carries one stray byte still refuses with
  "length mismatch": pixel-less framing is exact, not unchecked.
- The same records at 16 bits, with the omitted `curv` record ALSO clipped:
  the report carries the omission, empty and clipped-folder sentences and NO
  per-layer clipping sentence for the omitted record; the typed open yields the
  three retained roots with a canvas-sized transparent `Empty`.
- A file holding only a `SoCo` record, on both depths: available, with the
  omission loss and the stored-composite observation, and mode 0 opens the same
  single raster as mode 1.
- Those synthetic bullets are now doubled by a Photoshop-authored file:
  `core/tests/psd_photoshop_depth16.rs` pins the same omission, empty-record
  and carry sentences on `core/tests/fixtures/psd_photoshop/depth16/`, where
  Photoshop itself wrote the `brit`, `SoCo`, empty and `curv` records
  ([Photoshop 16-bit import](#photoshop-16-bit-import-run-4-2026-09-16)).
- A 64 × 64 document with a styled 16 × 16 raster at (20, 20) (drop shadow,
  outside stroke, colour overlay, fill opacity 0.5, non-identity Blend If, an
  enabled feathered mask, opacity 0.6, Multiply) and a layer clipped to it:
  one rasterize loss with all three clauses, no refusal, and after the write a
  twin with no style, no mask, opacity byte 153, Multiply, whose rectangle lies
  inside the canvas and whose pixels equal the crop of the independent package
  projection — which is itself empty everywhere outside that rectangle.
- The same layer with the mask DISABLED: no mask clause, the mask written and
  reopened with its samples, offset and disabled state, pixels equal to the
  unmasked package; and with density 0.5 on that disabled mask, the ordinary
  density/feather refusal.
- A styled GROUP and a 10000 × 9999 styled raster each refuse with their own
  sentence and leave a pre-existing destination byte-identical.
- The committed 8-bit writer goldens (`tests/fixtures/psd_10c/*.psd`) are
  unchanged: nothing about an unstyled document's bytes moved.

`RZ_PSD_CONSTRUCT_FIXTURES=<dir>` writes these PSDs for review and for driving
the built app over MCP.

### Run 5 — engineering leftovers (2026-09-16)

The last closeout run took no external tool (the takes themselves ran just
after midnight, 2026-09-17): it closed the doc counts, the
disabled-effects `lfx2` nit, the three 10D verifiers that were declared out of
their slices, and the carried review nits. Every take below ran against the
same Release bundle, app SHA-256
`44edd1fde2b2f8741bbd9c735becb0278be4e25fe1b8a81b38ea4c87aa407858`.

| Verifier | Gates | Receipt |
|---|---|---|
| `scripts/verify_phase10d_threshold_modes.py` | T1 appearance within 1 code with Dissolve exact, T2 native 34 roundtrip, T3 16-bit PSD key with `losses == []`, T4 atomic `Pass Through` refusal — all pass | `~/devel/rasterize-fixtures/phase10-receipts/threshold-modes/result.json` |
| `scripts/verify_phase10d_blend_if.py` | B1 publication, B2 appearance within 1 code, B3 native 35 roundtrip, B4 the two red ramps in the 40-byte section, B5 atomic adjustment-layer refusal — all pass | `~/devel/rasterize-fixtures/phase10-receipts/blend-if/result.json` |
| `scripts/verify_phase10d_adjustment_blending.py` | A1 appearance within 1 code with alpha exact and Dissolve exact, A2 native 37 schema 7 on the kind-3 record, A3 16-bit PSD `nvrt` with `losses == []`, A4 atomic `Pass Through` and Blend If refusals — all pass | `~/devel/rasterize-fixtures/phase10-receipts/adjustment-blending/result.json` |
| `scripts/verify_phase9i_typed.py` | thirteen checks (the Actions replay is this run's addition) — all pass | `~/devel/rasterize-fixtures/phase10-receipts/typed-refine/result.json` |

Each verifier writes its own fixture, asserts the margin its branches need
BEFORE it drives anything, and records a failed gate rather than widening it.
The three new ones follow `verify_phase10d_five_modes.py`'s harness: an owned
Release instance on a free port with a dummy API key and private
Actions/recovery/preset directories, terminated with the run.

This run's code fixes, each recorded where it belongs:

- **The disabled-effects `lfx2`.** A style whose effects are ALL disabled used
  to write an empty effects descriptor that reopened as a bare marker — and
  Run 3 had already found that Photoshop answers such a block with no
  `layerEffects` key at all. The four decision sites (`psd_export.rs`,
  `psd_export16.rs` twice and `psd_export_rasterize.rs`, plus the
  `style_block_bytes` cost) now ask `LayerStyle::has_enabled_effects()`, so
  such a style writes no `lfx2` and no image resources 1037/1049 while its
  fill opacity and Blend If still travel; the import side
  (`psd_group_records.rs`) deliberately stays on `effects.is_empty()`, so a
  disabled effect read FROM a file still carries. Evidence:
  `core/tests/psd_disabled_effects.rs` at both depths. No committed golden
  moved — only `psd_10b/styled_effects` holds a style, and its three effects
  are enabled.
- **`psd_zip.rs` now refuses trailing data.** The inflate seam decoded the
  zlib stream and ignored anything after it, where the RLE path refuses the
  same thing. It runs `miniz_oxide`'s core `decompress` into the admitted
  plane size and reports `"PSD ZIP channel has trailing data"` when the stream
  does not consume the channel; the Adler-32 check is unchanged
  (`TINFL_FLAG_PARSE_ZLIB_HEADER` verifies it). Case added to
  `core/tests/psd_zip16.rs`.
- Smaller nits: `with_clipped_members` refuses at `> MAX_RECORDS` rather than
  `>=` (it adds no record of its own, so `structure`'s own bound applies),
  `style_scratch` charges the pad-0 shape plane a styled clip base builds when
  no effect is enabled, `pasteLayerStyle` surfaces the typed plane-limit
  refusal instead of swallowing it, and `SelectionRefinementCapture` promotes
  the bytes it already captured instead of rasterizing a geometric selection
  twice.

## Photoshop agreement (Run 2, 2026-09-16)

Run 2 of the closeout (2026-09-16)
built the Photoshop reference side that every claim above said was missing and
then took A1 (per-kind arithmetic agreement) and A2 (`phfl` version 3) with it.

### The harness

`scripts/photoshop-probe/` is the Photoshop twin of
`scripts/psd-exchange-probe/`: `photoshop.jsx` (one ExtendScript driver with
jobs `author`, `render`, `report`), `run_photoshop.py` (the pinned launcher and
its receipt), `psd_records.py` (a bounded, lenient reader for a PSD's record
framing, written from Adobe's published format and NOT from `core/src/`) and
`pixels.py` (PNG in/out plus the base picture). Its README holds the pin, the
ownership rules and the ExtendScript quirks. `scripts/verify_phase10_photoshop.py`
is the A1/A2 runner. Pin: **Adobe Photoshop 2025, 26.10.0**, executable
SHA-256 `a502d3b62279299a77f85d69a3d8f3d5379e0fb406d40d746a00ee49031bfa04`;
the run launched it, it was not already running, and it was quit afterwards.

### The fixture

`core/tests/fixtures/psd_photoshop/adjustments/` (120 KB, five files) is the
first Photoshop-AUTHORED PSD in this repository: a 64 x 64 base raster with
one adjustment layer per carried kind, saved at both depths.
`core/tests/psd_photoshop.rs` reads it through the C API.

| file | SHA-256 |
|---|---|
| `description.json` | `051716cab2517eb936bf5b6ea8f961345d3722b1ff281b839616145b812c03f7` |
| `base.png` | `34bf9c575aced5f5a9fb65ae449da20233c794d1e3a74596903552868bb9f1a9` |
| `source.psd` | `38eae81a9c17e1d883a7dedb9c1f95ba8489070de91c0da12b6f70333cd36be5` |
| `source16.psd` | `46f97e2ccf6b03ba5b9c8646caee6ca572a52cd2a71c020ebfc1046b2d4ee448` |
| `photoshop.json` | `f472e0861e97dabb759218fa5b5ef12709c61fdd0e2d7dbf2261a4d0738266f2` |

A re-authoring run does not reproduce `source.psd` byte for byte (Photoshop
writes its own generator metadata), but it reproduced every adjustment block
body exactly, which is what `--author` gates.

### Three import defects the reference found immediately

Before this run Rasterize REFUSED the layered choice on a Photoshop-written
PSD — twice, for reasons no synthetic fixture had ever produced:

1. Photoshop rounds the 8-bit layer-INFORMATION section to a **multiple of 4**
   (two trailing zero bytes here), while `psd_group_records` admitted at most
   one pad byte and reported "unclaimed record/channel bytes". Now up to three
   are admitted.
2. Photoshop writes a full 20-byte mask body with an **empty rectangle** and a
   bare `-2` channel on EVERY adjustment record, which `psd_raster_masks`
   pushed through `dimensions()` and refused as an empty rectangle. An empty
   rectangle at outside 255 is now `Absent` ("there is no mask"); at outside 0
   it is a reported drop, because it would hide the whole layer.
3. Every Photoshop adjustment record sets flags `0x18`, so each one was
   reported as "declares its raster pixels irrelevant" — a loss describing an
   alternative appearance a pixel-less record does not have. That marker is
   now read only on a record with a rectangle.

With those three fixes the file imports layered with 12 rows, `rgba8`, no
mask, and `verified_losses` EMPTY.

### A2 — `phfl` version 3 is never written; version 2 space 7 is

The A2 question was "author a `phfl` v3 (XYZ) sample and decide carry vs
decline from the real bytes". Photoshop 26.10.0 **never writes version 3**:
every authoring path — an RGB picker colour, a Lab picker colour, the
`warmingFilter85` preset and the default — writes version 2 with **colour
space 7 (Lab, components x100)**. The fixture's body is
`0002 0007 1a3f 0cf2 1c6f 0000 00000028 01 000000`: L 67.19, a 33.14, b 72.79.

So the carry that was actually needed was space 7, not version 3.
`psd_adjustment::decode` now reads those four components as **signed** 16-bit
hundredths (the PSD colour structure's convention, which is what makes a
cooling filter cool), converts them through `lab::lab_to_srgb8` — Lab against
the D50 PCS white, then the built-in sRGB model's inverse matrix
(`MatrixTrc::pcs_to_linear`, matrix only; there is still no curve inverter)
and the analytic sRGB OETF — and finally through the file's own sRGB->document
transform, exactly as an authored layer-style colour converts. The fixture's
Lab decodes to `#ec8a00`. Other spaces decline with *colour space is not RGB
or Lab*; version 3 keeps its existing *XYZ colour form is not mapped* decline,
now recorded as unreachable from this Photoshop rather than unknown. The
export side is unchanged and still writes space 0.

### A1 — measured per-kind agreement

Photoshop authored one layer per kind over the base and exported a flattened
PNG per kind at 8 and 16 bit; Rasterize imported the same file, showed Base
plus that one layer, and exported its own. The base renders agree at **0
codes** at both depths, so every number below is the adjustment's own.

Tolerances were fixed before the run and are NOT edited afterwards: invert 0
codes, levels 2, exposure 4, curves 8, the remaining colour kinds 16, and
threshold/posterize by the share of pixels that differ at all (<= 10 %),
because one code across their step changes a pixel by the whole step. Errors
are 8-bit code units; a 16-bit difference is divided by 257.

| kind | key | carried | tolerance | 8-bit max / mean / differing | 16-bit max / mean / differing | pass |
|---|---|---|---|---|---|---|
| invert | `nvrt` | yes | 0 codes | 0 / 0 / 0 % | 0 / 0 / 0 % | **pass** |
| threshold | `thrs` | yes | <= 10 % px | 255 / 13.57 / 5.3 % | 255 / 13.39 / 5.3 % | **pass** |
| posterize | `post` | yes | <= 10 % px | 85 / 10.99 / 34.1 % | 85 / 10.99 / 93.8 % | miss |
| levels | `levl` | yes | 2 codes | 6 / 0.23 / 34.9 % | 4.92 / 0.11 / 90.1 % | miss |
| curves | `curv` | yes | 8 codes | 6 / 2.33 / 95.9 % | 5.02 / 2.32 / 99.9 % | **pass** |
| color_balance | `blnc` | yes | 16 codes | 37 / 8.85 / 99.9 % | 36.76 / 8.81 / 99.9 % | miss |
| hue_saturation | `hue2` | yes | 16 codes | 32 / 6.36 / 91.5 % | 31.13 / 6.29 / 99.9 % | miss |
| photo_filter | `phfl` | yes | 16 codes | 105 / 16.56 / 100 % | 102.04 / 16.46 / 100 % | miss |
| exposure | `expA` | yes | 4 codes | 127 / 28.02 / 68.0 % | 125.82 / 28.04 / 67.9 % | miss |
| vibrance | `vibA` | yes | 16 codes | 97 / 12.27 / 98.7 % | 96.90 / 12.27 / 99.8 % | miss |
| black_and_white | `blwh` | yes | 16 codes | 33 / 8.39 / 81.5 % | 33.61 / 8.69 / 81.9 % | miss |

Read that honestly: **all eleven kinds CARRY** — every parameter reaches
Rasterize with the value Photoshop wrote — and **three of eleven reproduce
Photoshop's arithmetic within a tolerance fixed in advance**. Invert is exact.
Threshold agrees except on 5 % of pixels sitting within a code of the knee.
Curves is within 6 codes everywhere.

The eight misses are differences in ARITHMETIC, not in carriage, and they are
recorded, not repaired:

- `expA` is the largest (max 127, mean 28). Photoshop's Exposure is defined on
  linear light with its own offset and gamma stages in a specific order;
  Rasterize's `document_v2` evaluation is a different composition of the same
  three controls. At these settings (exposure 1.5, offset -0.25, gamma 2.0)
  the two diverge most in the shadows.
- `phfl` (max 105, mean 17) is a preserve-luminosity difference: the filter's
  colour now matches exactly, so what is left is the luminosity restoration.
- `post` misses its FRACTION tolerance while its max stays at one step (85 =
  255/3): the two implementations round the same four levels at different
  pixels, and at 16 bits nearly every pixel lands on the other side.
- `levl` is within 1 code on average (mean 0.23) and misses only by a 6-code
  worst case — a gamma-rounding difference, not a different curve.
- `blnc`, `hue2`, `vibA` and `blwh` are all mean 6-13 codes: different but
  recognisably the same operation, which is what "parameters are mapped onto
  Rasterize's own arithmetic" has always claimed.

None of these changes what the importer PROMISES, and that promise is already
in the report sentence every carried record carries: *Its parameters are
mapped onto Rasterize's own arithmetic, so its appearance can differ from
Photoshop's.* This run turns that from an unmeasured caveat into eleven
measured numbers.

### Structural gates (the run's exit status)

All four passed: **G1** the record framing (12 records, every adjustment
record a zero rectangle with exactly one kind key, `phfl` v2/space 7; the
16-bit file's layer-info length 0, an `Lr16` tail, ZIP-with-prediction
channels and a Raw composite — recorded); **G2** the import (layered,
`rgba8`, 12 rows, `has_mask` false on every row, no loss); **G4-mechanics**
Photoshop's own 16-bit PNG opening as a 64 x 64 `rgba16` document; **G5** the
16-bit PSD reporting `rgba16`, layered-available and **0 layers**.

G5 is the other finding: Photoshop's 16-bit records live in an `Lr16` tail
block with ZIP-with-prediction channels, which this build does not read, so
its 16-bit file imports as its stored composite rather than as layers. That is
already the documented rule for a file that retains no record; it is recorded
here as the concrete reason Run 4's F1 fixture has to plan for `Lr16` + ZIP.

### Artifacts

App SHA-256 `aafaf6695a9092a74fbf7292386c4b5c8f2156f4f9ccfb648a1a282a0caa2b3e`.
Receipt `~/devel/rasterize-fixtures/photoshop/adjustments/receipt.json`;
renders beside it under `photoshop/` (Photoshop's own) and in the run folder
(Rasterize's). Photoshop's render SHA-256s:

| render | 8-bit | 16-bit |
|---|---|---|
| `base` | `2862e50dabf70316b3e9c14ffd1a2cdd9dc8babd0047e670293cdef03e1955ac` | `3bf0b2300556ad2c154af6c0431d81643fe91c113a2b13f52241e781fef95ce5` |
| `nvrt` | `9d0420346557cc58a016a634b3663d0fd8013aed902459176b217ab44e2b5ed2` | `cb098e9a27576c47ea60a4ddee729efe6b06c3a33ae56120ca7631eeee63d8cb` |
| `thrs` | `c49d80fc69ff5801d46448442f78a149d592dc6810a7770a2af64cb7f4061158` | `b0aea2b4e8b7cf4889aa33b733d0b64c24540eb3cb35f3bdfd5db2b4c134f6f4` |
| `post` | `3e366d2e9e888fc4081a49736b5c462057d07ad7a9e0fe4f8f833f71eae879f5` | `67e32a78581aed724d74508721fb64a605ed17fe03a486498065210bc852ca4a` |
| `levl` | `f9b9e2513ad504bdbdc8b3cdcfbe45a9908418806e0ade7cb558abff7493b6ba` | `67116b3af2e925ff03713356160141de9959f4dfd35b105937942c4db70b3f70` |
| `curv` | `7ff3936fce717bf163eec4783b1fbcaccb441225dcc1aa9336aea15c8402d076` | `2f234e07f45a3606c5afd8d22719bdf83d17501e7a5287a3ed14786bb733b353` |
| `blnc` | `7736fa97c8ead2c36b4b9aa7679ef98548f457a69a300f28106467f19cf6cead` | `166e1e50ac205b72db5648f6727c04a503b31682190fd8131013539f99813c93` |
| `hue2` | `48ac134a98c074b3fe59d36a0b21585b4cc75c7a983eec14f2534b530f306cea` | `f48f0d8d616bd9f89e588182c420f77b76d1c85b4af4d3ab3feb2e7af85a8d42` |
| `phfl` | `2de78553915556ac3cd5c7ed1a29b8ac81e8b6f5655c84fe3e370be71c5d1830` | `f43dbcba868a0ef657b2e92ceee9ea255956fcc4eb656bf71dfce0e4c6324c91` |
| `expA` | `bb1cd397e33f5d6386095542d7e854fd2035314e7b6cbe4f7e45537cb6453351` | `8da98568e9bbd8194fb07c667adb23477fd62df04f0e69ddba2a6be7d8545f01` |
| `vibA` | `3ed1b09fc209bb6fb6e3e44271ba2dd0b9f08970654b67aee2082167cc740a89` | `2bbb1ffa5dd5d08be06076d1d897e438750af7e65a28b8cde88fd9d8a721c3ee` |
| `blwh` | `487dd8b8a9348f1342ec8e2d369c3d1ffc36b98c59d8f171223d2bd5c16ee2f3` | `fbb4ea5db9ca6b372289ee3591cd3f0405ebfe78f6d80136c2f096ba74a9dfd8` |

Re-runnable (the `--author` form re-authors in Photoshop and gates the fresh
adjustment bodies against the committed ones; drop it to render the committed
`source.psd` instead):

```sh
PYTHONDONTWRITEBYTECODE=1 python3 -B scripts/verify_phase10_photoshop.py \
  --app build/Build/Products/Release/Rasterize.app \
  --output ~/devel/rasterize-fixtures/photoshop/adjustments --author
```

## Photoshop appearance agreement (Run 3, 2026-09-16)

Run 3 of the closeout (2026-09-16)
took A3: what Photoshop RENDERS from the `lfx2`, `iOpa` and blending ranges
this slice writes. The runner is
`scripts/verify_phase10_photoshop_appearance.py --receipt A3`, over the same
harness Run 2 built.

Rasterize authored one 24 x 24 styled layer per case at (20, 20) over the base
picture, at BOTH depths, and exported with `export_psd mode layers`;
Photoshop reopened that file and rendered Base plus each case at the file's own
depth. No fixture is committed for A3 — Rasterize writes the file every time.

Tolerances were fixed before the run and are NOT edited afterwards; the style
colours are Rasterize's `257 * v`, whose <= 1-code quantization by Photoshop is
inside every one of them. Errors are 8-bit code units; a 16-bit difference is
divided by 257.

| case | tolerance | 8-bit max / mean / differing | 16-bit max / mean / differing | |
|---|---|---|---|---|
| `color_overlay` | 2 | 0 / 0 / 0 % | 0.14 / 0.01 / 14.1 % | **pass** |
| `fill_opacity` | 2 | 1 / 0.07 / 13.8 % | 0.32 / 0.02 / 14.1 % | **pass** |
| `stroke` | 2 | 16 / 0.03 / 0.4 % | 16.61 / 0.03 / 0.4 % | miss |
| `gradient_overlay` | 6 | 17 / 1.22 / 13.5 % | 17.60 / 1.22 / 14.1 % | miss |
| `drop_shadow_hard` | 4 | 0 / 0 / 0 % | 0.25 / 0.01 / 4.3 % | **pass** |
| `inner_shadow_hard` | 4 | 0 / 0 / 0 % | 0.12 / 0.00 / 2.8 % | **pass** |
| `outer_glow_hard` | 4 | 100 / 0.82 / 7.7 % | 100.67 / 0.82 / 12.2 % | miss |
| `inner_glow_hard` | 4 | 116 / 0.56 / 1.5 % | 116.28 / 0.58 / 8.6 % | miss |
| `satin_hard` | 4 | 0 / 0 / 0 % | 0.24 / 0.01 / 4.7 % | **pass** |
| `drop_shadow_soft` | 24 | 14 / 0.55 / 18.2 % | 13.27 / 0.55 / 19.0 % | **pass** |
| `inner_shadow_soft` | 24 | 7 / 0.23 / 10.6 % | 6.58 / 0.23 / 11.6 % | **pass** |
| `outer_glow_soft` | 24 | 73 / 1.35 / 16.6 % | 72.77 / 1.35 / 19.0 % | miss |
| `inner_glow_soft` | 24 | 71 / 1.84 / 11.6 % | 70.56 / 1.83 / 11.6 % | miss |
| `satin_soft` | 32 | 31 / 0.99 / 13.9 % | 30.95 / 1.00 / 14.1 % | **pass** |
| `bevel_emboss` | 32 | 57 / 1.82 / 12.2 % | 57.14 / 1.81 / 12.5 % | miss |
| `all` | 24 | 11 / 0.43 / 25.5 % | 11.10 / 0.41 / 28.3 % | **pass** |

Read that honestly: **nine of sixteen cases meet a tolerance fixed in
advance**, including the combined `all` case (fill 50 % + drop shadow + stroke
+ colour overlay at max 11 codes), and four are EXACT at 8 bits — colour
overlay and the three hard-edged offset effects (drop shadow, inner shadow and
satin at size 0) land on Photoshop's own pixels. Every 16-bit number is within
0.7 codes of its 8-bit twin, so nothing about the second depth is a separate
result.

The seven misses are differences in the effect KERNELS, recorded and not
repaired:

- The two GLOW families are the largest (`inner_glow_hard` max 116,
  `outer_glow_hard` 100, and 71-73 at the soft defaults) while their means stay
  near 1 code: the glows differ on a thin band of pixels at the shape edge, not
  across the effect. Photoshop's glow carries a technique (softer/precise) and
  a range/jitter this build does not model, which is exactly what the
  importer/exporter sentence has always disclaimed.
- `bevel_emboss` (max 57, mean 1.8) is the same shape of difference on the
  bevel's own edge band.
- `gradient_overlay` (max 17, mean 1.2) is an interpolation difference across
  the whole overlay — Photoshop's ramp is its smoothed curve, ours is linear.
- `stroke` misses by 16 codes on 0.4 % of pixels, all of them on the outside
  stroke's own corner pixels.

None of this changes what the writer PROMISES; the preflight sentence already
says *it is written as Photoshop layer effects and blending options … so
Photoshop renders them itself; its appearance there is not qualified*. This run
turns that into sixteen measured numbers per depth.

### Structural gates (the run's exit status)

All three passed. **E1** both exports carried the file's own depth, no
"rasterized" loss and one *written as Photoshop layer effects* observation per
styled layer (16 of them at each depth). **E2** Photoshop's report shows a
`layerEffects` (`Lefx`) descriptor on every row whose case declares an effect,
and `fillOpacity` **128** on the `fill_opacity` and `all` rows — the 0-255 byte
`iOpa` carries, not the DOM's 0-100 percent. **E3** the base renders agree at
**0 codes** at both depths, so every number above is the style's own.

E2 also recorded a finding: the `fill_opacity` case — a style with a fill
opacity and NO effect — still writes an `lfx2`, and Photoshop surfaces **no
`layerEffects` key at all** for it. Its `iOpa` is read correctly either way.
That is the reference's own confirmation of the Run 5 "disabled-effects empty
`lfx2`" nit, and it is why E2's effects clause is scoped to the rows that
declare an effect.

### Artifacts

App SHA-256 `a135ef31389df8c3d94ab078adcd296c674b60478611d3b1e17897e2b9906f8c`;
Photoshop 26.10.0, executable SHA-256
`a502d3b62279299a77f85d69a3d8f3d5379e0fb406d40d746a00ee49031bfa04`, launched by
the run and quit afterwards. Receipt
`~/devel/rasterize-fixtures/photoshop/effects/receipt.json`; Rasterize's renders
beside it, Photoshop's under `photoshop-effects8/` and `photoshop-effects16/`.
Photoshop's render SHA-256s:

| render | 8-bit | 16-bit |
|---|---|---|
| `base` | `303f80ce6ad38ba929d9be9bd33b1cc2df043c17cf28b786cf797d1782aa59a6` | `e62a90ef7cc99834bb001c22048163a92db72420b45fae61e130c649beb9a959` |
| `color_overlay` | `339b3e060a6b3ea949e56d220a7c9cdeaa34486413cfbd5fdd672500cd7d257f` | `48379ee1629fdbf0bfa5ea95de51772de04107cff386fdc844cdeeedac279389` |
| `fill_opacity` | `3cd4d5ddb59d2c73630b8edf26d9bf11b66043bb9315ac78ae4cfa57beefe371` | `2cb1f8800efdb6626930a08ab4d7b60daa78efedbadf464a7491b757f5cabd63` |
| `stroke` | `6956956bcbe82854721e901883c3bfdf78b7516908a758c6d4176bd28043a3fe` | `ac9ee97ec3ac90842e4a31e39ab89b60d1b5c80b13486758522aff21f6941cb3` |
| `gradient_overlay` | `28cfc6f3d420646ed7d728c4e307e3012057d1a51e156c7d117ff88f815a69b7` | `5174fa663037c329571db86b7c88f5986c6586f70a3a659ee0913fc6ff046f6f` |
| `drop_shadow_hard` | `c73d6ab153b20298f8aa62f545135c5949de58b269d2907340122e5c53a5da73` | `032b67cb13ee23ed75362c53d5f451d9d35ab0769baef194ab30e15db42a1b28` |
| `inner_shadow_hard` | `aff66183db52844831da848906d68b9d73ac38a09f6a10e76aa750100060d052` | `ec00140d740635a4fef513a76ab488d3a70a51ecb698175ef7acc97b9f6f1fc5` |
| `outer_glow_hard` | `ff864459ccc4a75ac104867a394a381459f67daa633303f563c5ef332e08a84c` | `439a45f61c330e6eb9576aac8755030bf6e552949325a071589e671e0911cab2` |
| `inner_glow_hard` | `d8bdb688258c2af87a18434dd7624593c7a5b0f55bae2c700bd0d793cb15bfec` | `5a772ff60016e966a2122b46df43d1a18037f6cc2cbc9c07e3c7a2c8c29b1790` |
| `satin_hard` | `81d1c5e26322c1e83aab522830418057fab0c73af2bce74c4ad2794199a1327a` | `4fde66a29b193272468fa8f726fb9da418646b87ed7c062edc491edfe9456ecb` |
| `drop_shadow_soft` | `b1830b35d746daecdb5eaaac0fd5001cf4c15f5fe2b60ffe62c179ba51809273` | `f536535dcda51927031d30910eb4ee871c18e7b9ebfc70731873c6ad8183fbbe` |
| `inner_shadow_soft` | `a3b3b9bce4f90747ce32861d37420238b4c5137017b23f7a9f2dda15c7de047b` | `1983d1a33b0164c4945f9046a8726ca47cbc752485e90e66c0681039b2e64e8f` |
| `outer_glow_soft` | `81cd274db2e4f8089a80da0b4f42abcbd76caff1a94133058eceab7ac880b613` | `216ce53002ad1f06f0b973edf868c44bf1a6b3cf0568e8ee1a2194e6b63e141c` |
| `inner_glow_soft` | `83609b4879f729ba02069d19afbbc1a227cc008f86551296277b4d3678a8688a` | `527dd55fde3ea63c6bd080957dbbead21a49a11718a53a6b2b4b77dd582a7686` |
| `satin_soft` | `cc89ebfda840e03be746edf027e3340d4152631fff3c3b7ff1206391e8953a9c` | `256d0f31f0bd94be92ef706675656b3bf8404c7730a8ed2facc43bd4ae0cc580` |
| `bevel_emboss` | `3f15a92c7062af51f4a14f76810682531c1cc1b6e8801f34da648498e3e62e6d` | `a20ce196a14e763857eb16867216ea6c4a648c301fd9bbb880cdeec208210ad4` |
| `all` | `93b5df7b7d93e3718074fcaf356559eef21c69380a55875b9e54d47138a69a02` | `7a64ee4adf807cc9fdda98ee05c9334bb30c9ae53f3cb302e9e077b699df47b1` |

Re-runnable:

```sh
PYTHONDONTWRITEBYTECODE=1 python3 -B scripts/verify_phase10_photoshop_appearance.py \
  --receipt A3 --app build/Build/Products/Release/Rasterize.app \
  --output ~/devel/rasterize-fixtures/photoshop/effects
```

Run 3 also committed two more Photoshop-AUTHORED fixtures beside the
`adjustments` family, read by `core/tests/psd_photoshop_constructs.rs`:
`core/tests/fixtures/psd_photoshop/blend_if/` (128 KB, six files) and
`…/masks/` (120 KB, six files). Their agreement numbers live in
[phase-10d-blend-if-status.md](phase-10d-blend-if-status.md) and
[phase-10a-relative-masks-status.md](phase-10a-relative-masks-status.md).

| file | blend_if | masks |
|---|---|---|
| `description.json` | `fc1feb7e9e9f1dd0225f18beb2f031540bf04b337f19b3a9af562ed3e43a18f4` | `ee84a1acf22c102d5e2759711b48480edd3f04fa15b3eb73151c1d0fb731889d` |
| `base.png` | `34bf9c575aced5f5a9fb65ae449da20233c794d1e3a74596903552868bb9f1a9` | `34bf9c575aced5f5a9fb65ae449da20233c794d1e3a74596903552868bb9f1a9` |
| overlay PNGs | `inverted.png` `f9a0bec479066a949f2ad35d765031f7d1efeef042e1b115c714f61fda6baf18`, `solid.png` `a8019b07eef3a1d20cdce95c2d2165d6339e10f390822c1fde0b5ff507eed48f` | `overlay.png` `30058256e48ec4f4f4abaaee323a58c3cef459201486d15fb108396860946554` |
| `source.psd` | `2dd522b54df000cfff29b33d1be9a6ccce496a7bf5ff42b176851df964cae712` | `d791c6f989ef1e3621517e6ffe7df3650316fed33a9c8ce3a65851df14dcf8a4` |
| `source_relative.psd` | — | `d638776400e49fd30c5d6a8868cf640f0fb0479271c5fe0913ed2e674403edc7` |
| `photoshop.json` | `58d50953d9a6afd599396f9d0353a5a147fbb8ecc9457c854a3f4dcf5da64c6b` | `cfa4dbf09e7d8b3879451700de977e98eda0f040c2617ca8c3e1483c273dedba` |

Both re-authored byte-identically in their blending-range bytes and mask
bodies on the recorded run (`authoring_agrees_with_fixture` true).

## Photoshop 16-bit import (Run 4, 2026-09-16)

The omission and empty-record rules above were SYNTHETIC-only until this run:
GIMP writes no adjustment record at all, and the Run 2 `adjustments` family
carries every one of its eleven. Photoshop 26.10.0 now supplies a file that
holds the uncarried constructs too —
`core/tests/fixtures/psd_photoshop/depth16/` (148 KB, eight files;
`core/tests/psd_photoshop_depth16.rs` reads it) — and, because Photoshop's own
16-bit form is an EMPTY layer-information section plus records in an `Lr16`
tail block with ZIP-with-prediction channels, this is also the first
third-party-authored 16-bit LAYERED import
([the depth status](phase-10c-depth-status.md) carries the reader and the
gates).

What Rasterize reports for each construct, measured on that file:

| record | Photoshop wrote (16-bit) | Photoshop wrote (8-bit) | Rasterize import |
|---|---|---|---|
| `Curves` (`curv`) | pixel-less, mask body `[0,0,64,64]` outside 255 with a canvas-sized comp-3 plane | pixel-less, mask body with a ZERO rectangle and a 2-byte plane | CARRIED as a typed `curves` adjustment + observation, with Photoshop's white mask at 16 bits and no mask at 8 |
| `Brightness` (`brit` + `CgEd`) | pixel-less, same mask form | pixel-less, zero-rectangle mask | OMITTED at BOTH depths + the marker loss and the omission loss naming `brit` |
| `Fill` (`SoCo`) | PIXEL-LESS | a canvas-sized RLE RASTER, flags `0x18` | OMITTED + both losses at 16 bits; at 8 bits it is a raster, so it is KEPT with the marker loss and the "declares its raster pixels irrelevant" loss |
| `Empty` | pixel-less, no adjustment key | same | one canvas-sized transparent raster + observation, both depths |
| `Shadowed` (`lfx2` + `lrFX`, flags `0x28`) | one enabled drop shadow beside four disabled slots | same | style CARRIED; the enabled drop shadow keeps opacity 0.75, angle 120, distance 6, size 0, multiply, `#000000` |
| `Masked` | mask UNTRIMMED over the whole canvas, `[0,0,64,64]` outside 0 | mask TRIMMED to `[24,28,60,60]` | mask carried either way, independent (unlinked); 64 x 64 at (0, 0) at 16 bits, 32 x 36 at (28, 24) at 8 |
| `Group` / `</Layer group>` | `lsct` 1 opener directly above its member, `lsct` 3 closer as the set's LOWEST record | same | one pass-through folder holding `Inner` (Multiply, opacity byte 153) |

So the omission receipt is no longer synthetic: a Photoshop-authored `brit`
record is omitted with its two sentences at both depths, a Photoshop-authored
`SoCo` is omitted at 16 bits and kept-as-pixels at 8, and the empty record
imports as an empty canvas-sized layer. The agreement table and the artifacts of
that run are in [the depth status](phase-10c-depth-status.md); the runner is
`scripts/verify_phase10_photoshop_depth16.py`, app SHA-256
`809b16289e8c0ae0da65087ef60edff3bf23c6101132ccdc9129a722e4c46512`, receipt
`~/devel/rasterize-fixtures/photoshop/depth16/receipt.json`.

## Open

- 10C-2 slice 2 leftovers: `SoCo` (no Rasterize fill adjustment to carry it
  as), the `curv` map form and per-channel Levels (Photoshop 26.10.0 wrote
  neither for the probe's settings, so neither has a sample yet either), and
  `phfl` version 3, which stays declined for a new reason: Photoshop 26.10.0
  never writes it — it writes version 2 colour space 7 (Lab) on every
  authoring path, and THAT is now carried (see
  [Photoshop agreement](#photoshop-agreement-run-2-2026-09-16)). Phases 1 and 2 (the
  eight binary bodies, then `expA`, `vibA`, `blwh`) are shipped above.
- ARITHMETIC agreement with Photoshop for the eleven carried kinds is now
  MEASURED, not reported: all eleven carry with the parameters Photoshop
  wrote, and `invert`, `threshold` and `curves` also reproduce its arithmetic
  within tolerances fixed before the run. The other eight are recorded with
  their measured gaps (worst `exposure`, max 127 / mean 28 codes; then
  `photo_filter`'s preserve-luminosity stage at max 105 / mean 17) in the
  Run 2 section. Closing any of them means changing Rasterize's own
  arithmetic, which is a separate decision per kind and is NOT part of the
  exchange contract this document fixes.
- Photoshop's own 16-bit files now DO import layered: Run 4 taught
  `Container::layer_info` to find the records in the `Lr16` tail block its
  empty layer-information section implies, and `psd_zip` to decode the ZIP and
  ZIP-with-prediction layer and mask planes
  ([Photoshop 16-bit import](#photoshop-16-bit-import-run-4-2026-09-16)).
  `Lr32`/32-bit, PSB, 8-bit ZIP channels and ZIP composites stay refused.
- APPEARANCE agreement with Photoshop for the written effects is now
  MEASURED, not reported: nine of sixteen effect cases meet a tolerance
  fixed before the run at both depths, four are exact at 8 bits, and the
  seven misses (both glows, bevel & emboss, the gradient overlay's ramp and
  the outside stroke's corners) are recorded with their gaps in
  [Photoshop appearance agreement](#photoshop-appearance-agreement-run-3-2026-09-16).
  Closing any of them means changing an effect KERNEL, which is a separate
  decision per effect and is not part of the exchange contract this document
  fixes.
- `designs/phase-7-carryovers.md`: P7-21 is closed by slice 3 — layer styles,
  fill opacity and Blend If now cross a PSD in both directions.
- Slice 3 leftovers: `lrFX` (legacy effects) is recognized but never read,
  `lmfx` is detected and declined, and contours, noise, technique, texture,
  pattern, jitter, glow range, `clbl` and a typed global light stay out; the
  KNOCKOUT family (`knko`/`infx`) is implemented in the COMPOSITOR at both
  depths since Phase 12E-3 / 12E-3b and CROSSES A PSD IN BOTH DIRECTIONS AT
  BOTH DEPTHS since **12E-3c** — on the record's ONE layer style, written only
  when the style is written rather than baked and only when the key is not at
  its default, so every committed golden here is byte-identical. **12E-3d**
  then measured that export against Photoshop 26.10.0 per keyed record (PX):
  Photoshop re-saved the Rasterize-written file with every `knko` / `infx`
  body intact, the OPENING folder record's included, and each keyed render
  differs from its all-keys-cleared twin by 110 / 170 / 221 / 51 codes against
  a floor of 32. `clbl` is
  still never interpreted, and it is disclosed separately now that a carried
  `infx` withdraws the shared sentence's claim; their
  appearance cost is measured in
  [Photoshop appearance agreement](#photoshop-appearance-agreement-run-3-2026-09-16),
  taken with the reference harness (`scripts/photoshop-probe/`) as A3.
- Editable type (`TySh`), vector/shape and smart-object payloads rasterize
  forever unless separately assigned. PSB, ZIP, CMYK/Lab and 16-bit grayscale
  stay refused. Typed clipping and PSD clipping in a 16-bit document are now
  closed by [the 10D clipping slice](phase-10d-clipping-status.md).
- Independent-application (GIMP 3.2.6) reopen receipts cover preserved
  clipping (the `clipping` case: GIMP wraps each of the file's two clipping
  runs in one synthesized folder and reports every member above the base as
  `clip-to-backdrop`) and, since 2026-09-16, the renamed `styled_effects` case:
  GIMP reads none of the `lfx2` (it attaches no filter), so its projection is
  the bare layer stack and the effect appearance stays RECORDED, not gated.
  Adjustment/fill omission is synthetic-only for GIMP — its `psd-export.c`
  writes no adjustment records, so no GIMP-authored file can exercise it — and
  the Photoshop-authored fixture added in Run 2 exercises the CARRY rather
  than the omission (every one of its eleven records is carried). It is no
  longer synthetic overall: Run 4's Photoshop-authored `depth16` family holds
  a `brit` adjustment, a `SoCo` fill and an empty record, and the omission,
  the 8-bit `SoCo` raster and the empty-layer observation are measured on it
  ([Photoshop 16-bit import](#photoshop-16-bit-import-run-4-2026-09-16)). A
  Rasterize-WRITTEN `nvrt` record also survives GIMP's loader as an empty
  raster keeping the blend key (receipt R8 in
  [the adjustment-blending status](phase-10d-adjustment-blending-status.md)).
  See [the 10B export status](phase-10b-export-status.md).
- There is no per-export user choice between preserve, rasterize and refuse:
  the rules above are fixed and reported.
