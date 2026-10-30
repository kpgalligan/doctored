# Phase 10D: typed layer styles

This bounded slice gives the 16-bit document model a persisted per-layer
`Option<Arc<LayerStyle>>` — the byte `style::LayerStyle` (fill opacity and the
nine effects), with its `blend_if` always `None` because the layer's Blend If
stays in the v35 field of the [Blend If slice](phase-10d-blend-if-status.md) —
and composites styled typed layers, isolated groups and clip bases in the byte
`style_composite` order under the **shape-quantized byte-plane policy** below.
Styles therefore survive every typed edit, native saves (v36 / `LSTY`),
snapshots/recovery, the shared Layer Style sheet / MCP / Actions path and the
16-bit PSD export bake. Knockout, `blend_interior_as_group`, a typed global
light and `lfx2` writing stay out; P7-20 moves to verified, P7-19 and P7-21
stay open.

## Native plane policy (decision)

- **Carried:** all nine effects and `fill_opacity`, admitted on raster layers
  and isolated groups only (the `COLOR_MODES` target set); adjustments and
  Pass Through groups refuse; `with_blend_mode(PassThrough)` refuses on a
  styled group (core + Swift `allows` gate). A style whose padded plane
  `(w+2·pad)·(h+2·pad)` (raster: image dims; group: canvas) exceeds
  `doc::MAX_PIXELS` refuses **up front at every entry**: the edit/constructor
  (`validate`), the v36 `LSTY` attach step (before any PNG decode) and
  `geometry_plan` (before any resample, using the planned destination extents
  and the scaled pad) — never a silent no-effects render and never a refusal
  after pixel work.
- **Planes:** the typed SHAPE is
  `coverage[x,y] = ((a16 as f32) * cov / 257.0).round().clamp(0,255) as u8`
  (`cov` = `PreparedMask::at`, 1 unmasked; `bounds` = bbox of `a16 > 0`, mask
  left out) on a plane padded by `LayerStyle::pad()`, then the SHIPPED u8
  pipeline (`style_render::render_effects`) renders the contributions, which
  composite in f32 as `cov/255` with the authored colour `c/255` through the
  byte `icc_transform::Transform` for non-sRGB profiles (`RzDocument::style_colors`
  rule). Planes are rendered per render (no `RenderCache`); the shape rounding
  is the ONE depth reduction (16-bit alpha detail below 1/255 does not shape
  effects).
- **Guarantee for promoted documents** (every code `257·c8`, masks `257·m8` at
  density 1 / feather 0 / outside default): the shape bytes are bit-identical
  to `Shape::of_layer` (`257a·(257m/65535) = a·m/255`, no ties since 255 is
  odd), so every plane is bit-identical and `|typed − 257·byte| ≤ 129` codes
  per component over a direct backdrop at any opacity and any non-Dissolve
  blend, package path included. Everything else is f32 with the ordinary
  **0.65-code** bound against an f64 oracle that applies the same quantized
  shape.
- **Order:** Blend If snapshot of `layer rect ∩ below_landing` (only when Below
  contributions exist) → Below onto `acc` at `c.opacity × layer opacity` →
  pixels with `sa = checked_alpha(alpha, opacity, cov) × fill × weight` and the
  layer's blend → Interior. Opacity < 1 with planes and not Dissolve = package
  model on ONE canvas-sized copy of `acc` (direct path at opacity 1,
  `mix_premultiplied` back over `affected_window` only) — no strips. A styled
  clip base: Below → base pixels into `buf` at Normal/1 with `fill × weight` →
  footprint `= shape/255` (`clamp_alpha_to_shape`, byte rule; a Blend If-only
  base keeps the shipped f32 rule) → **each member in turn, clamped with `min`
  after EVERY member** (the loop lives inside `composite_styled_into`) → `buf`
  onto `acc` with the base's blend/opacity → Interior. Unstyled layers keep the
  existing arms bit-identical.
- **Scratch:** the plan charges what the u8 renderer allocates, not the byte
  `render_scratch.rs` 128 B/px estimate (there a reporting bound; the typed
  `plan.scratch_bytes` is a HARD gate, and 128 B/px would refuse a 5000×4000
  styled photo at the app's 2 GiB cap).

## Meaning, host routing and persistence

The model: `PrecisionDocumentLayer::{style, with_layer_style, reported_style}`
(`precision_style.rs`). `with_layer_style` sets BOTH fields from one whole
style — the Blend If through the v35 `with_blend_if` (identity → `None`), the
effects/fill into `style` iff `fill_opacity < 1` or an effect is enabled — and
`reported_style` is the canonical `to_json` of the two merged back, `None` iff
both are `None`. The `{op:"layer_style",version:1,layer,style:null|{…}}` edit
parses the whole style strictly (the message names the key), clears on null or
an identity, and returns the exact no-op as `None`; capability family
`layer_style` on `raster`/`group`; `rz_precision_doc_info` reports `"style"`
(the canonical string or null) beside the unchanged `"blend_if"`.

The renderer (`precision_style_render.rs`): `typed_shape` builds the quantized
shape, `render_effects` renders the byte planes, and `composite_styled_into`
runs the byte order with the byte `composite_contribution`,
`clamp_alpha_to_shape`, `below_landing`, `affected_window`, `mix_premultiplied`
and `BackdropSnapshot` (now `pub(crate)`, byte behaviour unchanged); the pixel
pass is the typed twin of `composite_pixels_into` with `checked_alpha` and
`source_weight_16`. `precision_render` routes `style.is_some()` layers (the
Raster and Group arms and the clip-unit path) there with a per-member renderer
closure; one `Transform` is built per render. `style_scratch` charges
`P·(2 + 2·enabled) + plane_blur::float_plan(w_p, h_p, σ_max) + 16·min(h_p,256)·w_p`
(only when an effect is enabled) `+ 16·w·h` for a Blend If snapshot `+ 16·canvas`
for the package copy; work is `P·(2 + 8·enabled)`. Effects read the default
global light (120°, 30°); there is no typed global light.

Host routing: the three typed branches of `RasterCore.layerStyle` /
`layerHasStyle` / `withLayerStyle` read the `style` row and send the whole
style through the edit (`NativeLayerStyle.swift`), so `get_document`, the
Layers badge, Copy/Paste/Clear Layer Style, the recorded `set_layer_style`
Action and the conversion sentence need no other depth code. Layer > Layer
Style… opens the shared `LayerStyleSheetController` over a
`PrecisionEditorCapture`: no live preview, a constant default light, one
publication through `requireCurrent` / `withLayerStylePayload` / `publish`.
`set_global_light` refuses on a 16-bit document with the limitation; a styled
group refuses Pass Through in the core and in `NativeLayerBlending.allows`.

Persistence: a style adds the outermost conditional RZDC36 wrapper and an
`LSTY` schema1 table after every prior tail, including `LBIF`: `36 → 35 → 32 →
29 → 34 → body/tails → LBLD6 → MOUT → LCLP → LBIF → LSTY`, entries `{u32 index
asc, u32 len 1..=65536, canonical to_json bytes}`. Nothing wraps 36; 36 only
ever precedes `{13,15,17,19,20,22,24,25,26,27,29,30,31,32,33,34,35}`. The
reader gates before any PNG decode: strict parse, `blend_if` null, non-identity,
`to_json() == stored` (the frozen schema1 whitelist), raster or isolated-group
record, and the padded plane ≤ `MAX_PIXELS` at the attach walk. Releasing the
last style restores the prior writer's exact bytes; `RZDC_VERSION` is 36.
Hypothetical (geometry) plans charge the actual entries + 128 bytes per styled
record. Scale Effects: Resize scales every style by `sqrt(W'H'/WH)`, Affine by
`sqrt(|det|)` and Perspective by `quad_mean_scale` on the targeted pixel layer,
through `style::scaled_style`; the plane gate in `geometry_plan` uses the
scaled pad against the planned destination extents.

PSD at 16 bits: since [10C-2 slice 3](phase-10c-constructs-status.md) a typed
style is WRITTEN as Photoshop's `lfx2` + `iOpa` + blending ranges — the layer's
own rect, its own `-2` mask, `export_style16` merging the layer's Blend If back
into one `LayerStyle` for the codec — and an authored `lfx2` record is
imported back through `with_layer_style` unless `typed_style_refusal` names a
reason. The bake (`bake_style16`: the grown `style_extent16` rect over a
transparent accumulator at opacity 1, Normal, no Blend If, an ENABLED mask
inside it and no `-2` channel; opacity byte and blend key kept) is the
FALLBACK, reported by the same sentence plus the reason. The export scratch
plan adds the largest bake.

## Measured maxima (core/tests/precision_layer_styles.rs, 2026-09-15)

| case | form | result |
| --- | --- | --- |
| each of the nine effects at defaults and one non-default variant (own light + spread 0.5, size 0, gradient centre stroke, outer bevel, satin `invert:false`, radial canvas-aligned gradient, multiply colour overlay), an anti-aliased 48×40 raster at (7,5) over an opaque direct backdrop, masked (density 1) and unmasked, opacity 1 and 0.625, Normal and Multiply — 144 renders | `\|typed − 257·byte\|` | worst 128 codes, 0 pixels past 129 in every case |
| colour overlay, hard drop shadow (angle 90°, distance 2, size 0, knocked out) and a constant-colour gradient overlay, alone and together, `fill ∈ {0, 0.5, 1}`, effect blend Normal/Multiply/Screen, opacity 1 (direct) and 0.625 (package), NON-promoted codes over a non-promoted backdrop | f64 oracle | worst 0.504 codes; fill 0 leaves the overlay intact |
| styled clip base (fill 0.5 + drop shadow + Blend If `red [0,0,100,100]`) with ONE opaque member and TWO members (the second translucent Multiply, canvas-sized), transparent and opaque backdrops, opacity 1 and 0.625 | parity + confinement | worst 128 codes, 0 past 129; every pixel outside `style_extent16` transparent |
| styled isolated group (drop shadow + colour overlay 0.5), opacity 1 and 0.625 | parity | worst 128 codes, 0 past 129 |
| 16-bit PSD write of a styled raster (drop shadow, outside stroke, colour overlay, fill 0.6, enabled mask, Blend If) reopened against the source render | codes | worst 1 code |
| 5000×4000 raster, default drop shadow | plan | 1018 MiB scratch (≤ 1 GiB); renders under a 2 GiB working limit |
| 5000×4000 raster, all nine effects enabled | plan | 1353 MiB scratch (≤ 1.5 GiB) |

## Refusals

- `layer_style`: version ≠ 1, an unknown request field, a style that is not
  an object, a key of the wrong type or an unknown effect `type` (named), an
  adjustment or Pass Through target, an out-of-range layer, a padded plane
  past 100 MP — refused in plan and edit.
- `with_blend_mode(PassThrough)` on a styled group (core), and the Swift
  `allows` gate before it; `set_global_light` on a 16-bit document.
- Geometry: a Resize/Affine/Perspective whose destination extents with the
  scaled pad pass 100 MP refuses from `geometry_plan`, naming the layer, before
  any resample (the job reports no progress).
- Native: an empty `LSTY` table, count > records, an unordered/duplicate/
  out-of-range index, a zero or over-cap body length, a non-canonical body
  (reordered keys, an unknown key, a bare `version` change), an unknown
  `type`, an identity, a `blend_if` inside the body, an entry on a Pass
  Through or adjustment record, a record whose declared dimensions + pad pass
  100 MP, `LSTY` under a 35 header, 36 nested inside 35, truncation and
  trailing bytes refuse before any PNG decodes.
- A typed source asset (embedded original) refuses a styled layer, as the
  byte path refuses one.

## Receipts as run (2026-09-15)

- `cargo fmt --check && cargo clippy --all-targets -- -D warnings`: clean.
- `cargo test --release --test precision_layer_styles`: 9 passed, 0 failed.
- `precision_blend_if`, `precision_clipping`, `precision_threshold_blending`,
  `precision_hsl_blending`, `precision_blend_mask_wrappers`,
  `precision_layer_blending`, `precision_document`, `precision_geometry`,
  `psd_depth16`, `psd_constructs`, `psd_export_corpus`, `psd_export`, `style`,
  `style_format`, `style_cache`: all pass unchanged.
- `python3 scripts/verify_phase10d_blend_decoder.py`: thirteen PASS lines,
  including the new `LSTY` table (361 malformed cases) and the
  `36 → 35 → 32 → 29 → 34` / `36 → 29 → 34` whole-file compositions.
- `make test`: 167 test binaries report `ok`, 1,587 passed, 0 failed (one unrelated
  experimental `bayesian_matting` cancellation-timing test raced once while
  `make app` competed for the CPU and passed on its own and on the re-run).
- `make typecheck`: 0 errors. `make app`: BUILD SUCCEEDED.
- MCP drive of the built app (port 4917, dummy key, private preset/action/
  recovery directories): `open_document` an authored 16-bit PNG with
  `precision: preserve` → `save_copy` `.rz` (header 13) → `open_document` that
  `.rz` → `set_layer_style` `{fill_opacity 0.5, blend_if gray [0,0,200,255],
  drop_shadow, color_overlay 0.5}` → `get_document` shows the merged style →
  the same call again is `unchanged` → `render` → `undo` (style null) / `redo`
  (style back) → `save_copy` (header `36 → 35`; `native.py` reads the `LSTY`
  and `LBIF` entries) → reopen shows the style → `inspect_psd_export` lists
  ONE rasterize sentence naming `drop_shadow, color_overlay` with its Blend If
  clause and no separate Blend If sentence → `export_psd` reports `bit_depth`
  16 → `inspect_psd` (`rgba16`) → `open_document psd_mode: layers` shows
  `style` null (since 10C-2 slice 3 both of those last two now report the
  WRITTEN style instead; that MCP drive is open) → `set_global_light` returns `isError` (the 16-bit tool gate's
  limitation, which now lists layer styles) → `group_layers`, blend mode
  Normal, `set_layer_style` drop shadow on the group → `set_layer_properties
  blend_mode:"Pass Through"` returns `isError` naming Pass Through. No
  unexpected `isError`.

## Open

- **Knockout and `blend_interior_as_group` now EXIST in the byte model and
  compositor** ([12E-3](phase-12e-layer-ops-status.md), design D1–D12), and a
  typed style REFUSES both by name — `high-depth knockout and blend interior
  effects as group are not supported yet` — rather than silently dropping
  them, which is what `with_layer_style`'s filter would otherwise have done.
  The typed twin and its `.rz` word 50 SHIPPED in 12E-3b (`precision_style_knockout.rs`); the PSD `knko`/`infx` records are 12E-3c's.
- A typed global light (document field + tail; `set_global_light` refuses on
  16-bit; effects with `use_global_light` read 120°/30°), a live typed preview in the sheet, a
  typed rendered-plane cache, an f32 plane pipeline (dilate/erode and the nine
  renderers are u8), byte-precision authored-colour conversion for non-sRGB
  typed profiles (the byte `Transform` rule is kept).
- `lfx2`/Blend If descriptor writing at either depth SHIPPED in
  [10C-2 slice 3](phase-10c-constructs-status.md). Its GIMP reopen receipt was
  TAKEN 2026-09-16 (`~/devel/rasterize-fixtures/phase10-receipts/R4/gimp-state/receipt.json` and
  `…/R4/external/result.json`, app SHA-256 `084b4be2b97b1c84b9749e861c066bec9b72abbefb627c37f7e7c9d7aa8052a6`): GIMP 3.2.6 attaches NO
  filter from a written `lfx2`, so its reopened projection is the bare layer
  stack and differs from the styled render by 151/128/130/0 codes — recorded,
  never gated — while the structure, mask, profile and merged-preview gates
  pass in full. The MCP drive of the same carry is receipt R3
  (`~/devel/rasterize-fixtures/phase10-receipts/R3/receipt.json`), re-runnable as
  `scripts/verify_phase10_receipts.py --receipts R3`; a dedicated
  `verify_phase10d_layer_styles.py` stays open.
- Photoshop's own rendering of the written effects is now MEASURED, not open
  ([10C-2 Photoshop appearance agreement](phase-10c-constructs-status.md#photoshop-appearance-agreement-run-3-2026-09-16),
  receipt `~/devel/rasterize-fixtures/photoshop/effects/receipt.json`, app
  SHA-256 `a135ef31389df8c3d94ab078adcd296c674b60478611d3b1e17897e2b9906f8c`):
  Photoshop 26.10.0 reopened a Rasterize export of sixteen styled cases at both
  depths and rendered them. Nine met a tolerance fixed before the run, four
  (colour overlay and the three hard-edged offset effects) are EXACT at 8 bits,
  and the seven misses are the effect KERNELS — both glows (max 100-116 codes
  on a thin edge band, mean under 1), bevel & emboss (57), the gradient
  overlay's smoothed ramp (17) and the outside stroke's corners (16 on 0.4 % of
  pixels). Recorded, not chased; Photoshop parses every one of these `lfx2`
  blocks into `layerEffects` and the `iOpa` into `fillOpacity` 128.
- Adjustment-layer blending has since shipped ([adjustment-blending slice](phase-10d-adjustment-blending-status.md): the same twenty-six modes on a typed adjustment under the byte `composite_adjustment_into` rule, conditional v37/schema7), which verifies P7-19; knockout has no byte implementation to migrate and is tracked as P7-37 under 12E.
- P7-19 is verified; P7-21 is closed by 10C-2 slice 3.
