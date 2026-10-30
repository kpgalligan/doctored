# Typed layer styles — shape-quantized byte-plane policy, native v36 / `LSTY` (ADR)

- **Status:** Accepted — open items remain (its 16-bit PSD "bake only" rule is superseded by [10C-2 slice 3](phase-10c-constructs-status.md); its knockout refusal by 12E-3b)
- **Phase / dates:** 10D layer-styles slice, 2026-09-15 (verifies P7-20)
- **Original:** [archive/phase-10d-layer-styles-status.md](../archive/phase-10d-layer-styles-status.md) — full history, measured maxima, scratch figures, receipts
- **Key commits:** `b8aa5b5` (slice), `fc70079` (`lfx2` written), `d1b6c27` (Photoshop appearance A3), `1a69abb` (typed knockout)

## Context
Byte documents render fill opacity and nine layer effects through u8 plane renderers (`style_render`) in the
`style_composite` order. The typed 16-bit model had no styles, so a style was lost on precision documents.
Re-implementing nine effect kernels in f32 was out of scope; the forces were parity with the byte look, bounded
scratch at photo sizes, and never a silent no-effects render.

## Decisions
- **Model:** per-layer `Option<Arc<LayerStyle>>` — the byte `style::LayerStyle` with `blend_if` always `None`; the
  layer's Blend If stays in the v35 field ([Blend If](phase-10d-blend-if-status.md)). `with_layer_style` sets BOTH
  fields from one whole style (effects/fill stored iff `fill_opacity < 1` or an effect is enabled);
  `reported_style` merges them back into canonical `to_json` (`../core/src/precision_style.rs`).
- **Targets:** every record: rasters, adjustments (styles scope phase 1; CMYK phase 2b-1) and groups. A Pass Through group
  with a style or Blend If composites ISOLATED (`PrecisionDocumentLayer::isolates`, styles scope phase 2b-2).
- **Plane policy (the decision):** the typed SHAPE is
  `((a16 as f32) * cov / 257.0).round().clamp(0,255) as u8` (`cov` = prepared mask coverage; bounds = bbox of
  `a16 > 0`, mask excluded) on a plane padded by `LayerStyle::pad()`; the SHIPPED u8 `style_render::render_effects`
  renders contributions, which composite in f32 as `cov/255` with authored colour `c/255` (through the byte
  `icc_transform::Transform` for non-sRGB profiles). The shape rounding is the ONE depth reduction; planes are
  rendered per render (no typed `RenderCache`). Code: `../core/src/precision_style_render.rs` (`typed_shape`,
  `composite_styled_into`).
- **Guarantee:** for promoted documents (codes `257·c8`, masks `257·m8` at density 1/feather 0) the shape bytes equal
  `Shape::of_layer`, so `|typed − 257·byte| ≤ 129` codes at any opacity and non-Dissolve blend; otherwise within
  0.65 codes of an f64 oracle applying the same quantized shape.
- **Order = byte order:** Blend If snapshot (only with Below contributions) → Below effects → pixels with
  `sa = checked_alpha × fill × weight` → Interior. Opacity < 1 with planes (not Dissolve) uses the package model on
  ONE canvas-sized `acc` copy, mixed back over `affected_window`. A styled clip base clamps its footprint to
  `shape/255` and clamps with `min` after EVERY member. Unstyled layers keep existing arms bit-identical.
- **Up-front 100 MP gate:** a padded plane `(w+2·pad)·(h+2·pad)` (group: canvas) past `doc::MAX_PIXELS` refuses at
  the edit (`validate`), at the v36 attach walk before any PNG decode, and in `geometry_plan` with the scaled pad —
  never a silent no-effects render or a refusal after pixel work.
- **Scratch is a HARD gate** charged at what the u8 renderer allocates (`style_scratch`), not the byte 128 B/px
  reporting estimate, which would refuse a 5000×4000 styled photo under the app's 2 GiB cap (the fixed 2 GiB limit of the time).
- **Global light:** typed effects read the document's global light (`precision_global_light.rs`); `set_global_light` and the sheet edit it at 16 bits and at 32-bit float (`style_float`).
- **Edit/FFI:** `{op:"layer_style",version:1,layer,style:null|{…}}`, strict whole-style parse, null/identity clears,
  exact no-op `None`; capability family `layer_style` on raster/group; `dr_precision_doc_info` reports `"style"`.
  Host: `DoctoredCore.layerStyle` / `withLayerStyle` → `../app/Sources/NativeLayerStyle.swift`; the shared
  `LayerStyleSheetController` over a `PrecisionEditorCapture` (one publication). The sheet previews
  live at 16 bits (`../app/Sources/PrecisionStylePreview.swift`): each request applies the style and the light
  to the captured facade as bookkeeping edits and reserves that document's own projection charge; above 4 MP
  the base is a reduced facade and the request's optional `scale` (finite, > 0) shrinks the style's lengths
  with it. Apply commits the authored style at full size. A 32-bit float document has no preview.
- **Native v36:** outermost conditional wrapper, nothing wraps it; `LSTY` schema 1 after `LBIF`:
  `{u32 index asc, u32 len 1..=65536, canonical to_json bytes}`. The reader requires strict parse, `blend_if` null,
  non-identity, `to_json() == stored` (frozen whitelist), raster/isolated-group record and the plane gate, all before
  PNG decode. Releasing the last style restores the prior writer's exact bytes.
- **Geometry scales effects:** Resize by `sqrt(W'H'/WH)`, Affine by `sqrt(|det|)`, Perspective by
  `quad_mean_scale`, through `style::scaled_style`.
- **16-bit PSD (since 10C-2 slice 3):** written as `lfx2` + `iOpa` + ranges (`export_style16` merges the layer's
  Blend If back); imported through `with_layer_style` unless `typed_style_refusal` names a reason. The bake
  (`bake_style16` over `style_extent16`) is the reported FALLBACK only.
- A typed source asset (embedded original) refuses a styled layer.

## Rejected alternatives
- f32 re-implementation of the nine effect kernels — out of scope; the u8 planes give exact byte parity on promoted documents.
- The byte 128 B/px scratch estimate as the typed gate — refuses ordinary photos at 2 GiB (the fixed 2 GiB limit of the time).
- Strip-wise package compositing — one canvas-sized copy is simpler and bounded.

## Consequences
- 16-bit alpha detail below 1/255 does not shape effects; changing that means an f32 plane pipeline.
- Qualification: `../core/tests/precision_layer_styles.rs` (promoted parity ≤ 129 codes, f64 oracle ≤ 0.65,
  clip bases, groups, PSD, scratch plan); `../scripts/verify_phase10d_blend_decoder.py` decodes `LSTY`;
  Photoshop appearance A3 and GIMP R4 are recorded in the [10C ADR](phase-10c-constructs-status.md).

## Open items (verified 2026-09-26)
- `DONE` — layer-style verification, covered 2026-09-28 without a dedicated `verify_phase10d_layer_styles.py`: 12E-3b K1–K3 (`../scripts/verify_phase12e3b_typed_knockout.py`), Blend If B3/B5 (`../scripts/verify_phase10d_blend_if.py`), 12H g6/n4 (`../scripts/verify_phase12h_conversion.py`), A3 (`../scripts/verify_phase10_photoshop_appearance.py`), R3 (`../scripts/verify_phase10_receipts.py`), 13A-3b / 13A-3b-ii (`../scripts/verify_phase13a3b_styles.py`, `../scripts/verify_phase13a3bii_styled_groups.py`) and `../core/tests/precision_layer_styles.rs`. A 2026-09-27 Release-build MCP probe found no bug: `save_copy` `.rz` gives `style_version` 36 with one `LSTY` record equal to `get_document`'s style; reopen, undo, redo and the `unchanged` echo behave; `image_size` 2× doubles shadow distance/size and stroke size; `set_global_light` and Pass Through on a styled isolated group refuse; promoted fixture `max |typed − 257·byte|` = 128 (bound 129).
- `DONE` — live typed preview in the Layer Style sheet at 16 bits (`PrecisionStylePreview`; jobs probe "typed style preview").
- `TRACKED` — typed rendered-plane cache, f32 plane pipeline, byte-precision authored-colour conversion for non-sRGB typed profiles, a live preview at 32-bit float → plan §10D "what stays open" (all still absent: `precision_style_render.rs` "no typed `RenderCache`"). The typed global light has landed.
- `DONE` Styles scope phase 1 — a style on an RGB adjustment layer at 8, 16 and 32 bits (`style_adjustment.rs`, `precision_style_adjustment.rs`), through `.rz` (no new word), PSD both ways, the sheet, MCP, Actions and the clipboard. The rule is the take's (`../scripts/photoshop-probe/styles_scope_558_take.py`, `styles-scope-558/take1`): the shape is the mask over the canvas (S-MASK, A1), the adjustment applies in place after the layer's own Below effects (POST, A2) with alpha kept (the a1 band), Fill scales it (A3), This Layer reads the adjusted colour (A4) and a clip base's members are frozen to the shape (A7). S-MASK and POST are fitted from that take and not confirmed by a second; rebuilt over Photoshop's `base`, rows a1, a2, a6 and a7 match on every pixel within 2 codes at 8 and 16 bits (`../core/tests/psd_adjustment_styles.rs`). Unmeasured: a knockout or grouped interior on a styled adjustment over a partly transparent backdrop (its unit pass raises alpha). Later phases: CMYK adjustment layers (2); a styled Pass Through group and clip base, which the take shows ISOLATE (3); multichannel Blend If (4).
- `DONE` Styles scope phase 2b-1 — a style and Blend If on a CMYK document's adjustment layer at 8 and 16 bits, phase 1's rules on ink. `cmyk_style_row`'s adjustment interior follows `Adjusted16::interior_into` statement for statement. The shape is `StyledSource::Opaque` over the stack's read rect: the canvas, or a merge's frame, so Merge Down tones columns past the canvas. The style goes through `.rz` (no new word; the native reader admits it), CMYK PSD both ways (the adjustment record's `lfx2`, `iOpa`, `knko`/`infx` and ranges; a carried adjustment record clears its style markers), the sheet, MCP, Actions and Copy/Paste Layer Style. CONFIRMED by the multichannel take (`../scripts/photoshop-probe/blend_if_multichannel_take.py`, `blend-if-multichannel-558/take1`, K): POST 0 codes (PRE 173), This Layer T-ADJUSTED 0, and the Invert premise 0. Tests: `../core/tests/cmyk_adjustment_styles.rs`, including Photoshop's k1/k2 rebuilt over its k0 within 2 codes; drive `../scripts/verify_558_adjustment_styles.py`. Left for phase 2b-2: a styled Pass Through group, a Pass Through clip base and Blend If on one. The same take measured all three as ISOLATE (P1–P3, 0 codes), and Photoshop writes a group's effects as `lfxs`.
- `DONE` Styles scope phase 2b-2 — a style and Blend If on a Pass Through group at 8, 16 and 32 bits and on CMYK: the group composites ISOLATED (take1's P1–P3), one rule per model (`isolates()` for the scene builder and the regional walk; `cmyk_stack::entry`; the byte `doc_group::is_isolated`, unchanged). The refusals are gone at every layer: the target sentences, `with_blend_mode`'s two Pass Through refusals, the native read walks, CMYK PSD import and the Swift guards. A group's PSD effects are `lfxs` both ways at 8 and 16 bits and on CMYK; the record keeps `pass`. Photoshop's p1–p3 import within 0 codes at 8 bits and after 8→16 (bound 1). Both style setters refuse an unknown key by its path (`style_json_keys.rs`). Tests: `precision_pass_through_styles.rs`, `cmyk_pass_through_styles.rs`, `psd_group_effects.rs`, `style_unknown_keys.rs`; drive `../scripts/verify_558_pass_through.py`.
- `DONE` Styles scope close-out — INSPECT k1 re-run on take2 (`../scripts/photoshop-probe/blend_if_alpha_take.py`): Doctored's CMYK k1 PSD (a masked Invert with Stroke and a Normal Drop Shadow, sent under `blend`) renders in Photoshop within 2 / 0.22 codes of Photoshop's own k1 on every pixel and plate (bound 2), MET; take1's miss was its build input alone. Alpha-channel Blend If is not built: a PSD's blending ranges cannot hold one ([Blend If](phase-10d-blend-if-status.md)).
- `DONE` — typed knockout / `blend_interior_as_group` (`1a69abb`, `precision_style_knockout.rs`) and PSD `knko`/`infx` (`1063561`); P7-37 verified.
- `DONE` — `lfx2`/Blend If writing at both depths (`fc70079`); GIMP R4 and MCP R3 (`502cc88`, covering the 16-bit reopen showing the written style); Photoshop appearance measured (`d1b6c27`).
- `DONE` — an Inside Stroke, and a Center Stroke's inner half, REPLACES the layer under its band at 8, 16 and 32 bits: `lerp(X, over(B0, stroke), band / shape)` with B0 the stack after the Below group (a knockout's backdrop under a knockout), one body `style_composite::replace_under_band_sample` for the byte, typed and regional paths; at 0 % it still shows B0 (`Ocut`). Fitted and confirmed on two Photoshop takes (`../scripts/photoshop-probe/style_v1x_take.py`, receipts `style-parity-v1x/take{1,2}`): take2 0.88 / 0.26 codes (8), 0.37 / 0.02 (16), 0.26 / 0.02 (32), today 184 / 61. The B0 store is charged `20·w·h` (`style_scratch`, `doc_regional_budget`). Tests `../core/tests/style_inside_stroke.rs`; drive `../scripts/verify_style_parity_v1x.py`.
- `KEPT` [#2](https://github.com/kpgalligan/doctored/issues/2) (effect kernels) — the glow, bevel and gradient-overlay families were fitted on the same takes and none met its bound or was measurably closer on take2 and the receipts at 8 and 16 bits, so today's kernels stay; the runner's docstring and the #2 comment hold every residual. The overlay is exact at 0°/90° with corner sampling and a Catmull-Rom ramp (`Scorner · Fsmooth`) but not at other angles or radial, where neither span holds Photoshop's geometry.
