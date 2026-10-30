# Phase 10D: typed Blend If

This bounded slice gives the 16-bit document model a persisted per-layer
`Option<BlendIf>` — the byte model's `style_model::BlendIf`, a channel and two
`[lo0, lo1, hi0, hi1]` ramps over 0…255 — and applies it in the typed f32
compositor exactly where the byte compositor applies it: a weight multiplied
into the layer's effective alpha, "underlying" being the accumulator beneath.
Blend If therefore survives every typed edit, native saves, snapshots/recovery,
the shared layer-style UI/MCP/Actions path and the 16-bit PSD export report.
Layer effects and fill opacity have since shipped in the
[layer-styles slice](phase-10d-layer-styles-status.md), which verifies P7-20;
knockout and adjustment-layer blending stay outside.

## Meaning, host routing and persistence

Blend If is admitted on raster layers and isolated groups only (the
`precision_layer_blend::COLOR_MODES` target set); adjustments and Pass Through
groups refuse, and a group carrying Blend If refuses `blend_mode: Pass Through`
— in the core (`with_blend_mode`) AND in the Swift
`NativeLayerBlending.allows(_:document:layer:)` gate, which every caller (the
Layers popup, the `set_layer_properties` pre-check, apply) inherits. An
identity Blend If (both ramps full) stores as `None`, the byte `LayerStyle`
rule, so a document that carries none keeps its prior native writer.

The renderer (`precision_render.rs`) evaluates `style_blend_if::source_weight_16`
on the layer's colour and the accumulator pixel beneath, and multiplies the
weight into the effective alpha AFTER `checked_alpha` in the Raster and Group
arms; weight 0 skips the pixel like coverage 0, and Dissolve thresholds on the
weighted alpha as the byte path does. A never-touched accumulator pixel reads
transparent black → channel 0.

Host routing: a typed layer's "style" is exactly its Blend If. The three typed
branches live in `RasterCore.layerStyle` / `layerHasStyle` / `withLayerStyle`
(`NativeBlendIf.swift`), so `get_document` (the typed row loop in
`AgentServer+Precision.swift` reports the same `style` object the byte rows
do), the Layers badge, Copy/Paste/Clear Layer Style, the recorded
`set_layer_style` Action and the conversion sentence need no other depth code.
`set_layer_style` on a 16-bit document accepts a Blend If-only style; any
effect or a fill opacity other than 1 refuses with `NativeBlendIf.limitation`.
The style is reported as `{"version":1,"fill_opacity":1,"blend_if":…,
"effects":[]}` or null. Layer > Layer Style > Blending Options… opens
`NativeBlendIfSheetController` (the shared `BlendIfPane` over a
`PrecisionEditorCapture`, published once through the core's
`{op:"layer_blend_if",version:1,layer,blend_if}` edit; no live preview). The
capability family `layer_blend_if` admits raster and group targets.

Persistence: a Blend If adds the outermost conditional RZDC35 wrapper and an
`LBIF` schema1 table after every prior tail, including `LCLP`: `35 → 32 → 29 →
34 → body/tails → LBLD6 → MOUT → LCLP → LBIF`, entries `{u32 index asc, u32
channel 0…3, [u8;4] this, [u8;4] underlying}`. Nothing wraps 35. Releasing
the last Blend If restores the prior writer's exact bytes. LBLD 1–6, MOUT and
LCLP are untouched; `RZDC_VERSION` is 35 so the byte reader refuses it naming
the typed reader.

PSD at 16 bits: since [10C-2 slice 3](phase-10c-constructs-status.md) Blend If
IS written, at both depths — as the layer record's 40-byte blending ranges,
with the chosen channel's two ramps in its slot and the unrestricted default in
every other. A Blend If-only layer's preflight sentence is therefore the shared
preserve observation (*…has a layer style (Blend If on {channel}); it is
written as Photoshop layer effects and blending options…*), not a loss. An
imported file's ranges are carried back when exactly one of gray, red, green,
blue is restricted and every other pair is default; anything else keeps the
shared `psd_feature_markers` loss and opens with `blend_if: null`.

## Ramp policy

- **Knee mapping.** An 8-bit knee `k` maps to 16-bit code `257·k` (the ×257
  promotion of `precision_conversion`). Ramps are evaluated on 16-bit CODES
  exactly as the byte compositor evaluates them on 8-bit codes
  (`style_blend_if.rs` module doc): the this-layer channel value is
  `(unit·65535).round().clamp(0,65535)` — which recovers every stored code
  exactly (unit = code/65535 in f32 is within 2^-7 codes of the integer) — and
  the underlying value is the same rounding of the f32 composite beneath (the
  code the projection would store); gray is `LUMA_R·r+LUMA_G·g+LUMA_B·b` on
  unit values, scaled and rounded the same way. The five-row table is the byte
  `ramp_weight` with knees `f32::from(k)*257.0` (exact); numerator and
  denominator are exact integers < 2^24, so each weight is the correctly
  rounded f64→f32 quotient (≤ 1 ulp). Weight multiplies the effective alpha
  AFTER `checked_alpha` (the 2^-96 floor keeps its existing meaning: alpha ×
  opacity × coverage); weight 0 skips the pixel like coverage 0. Dissolve
  thresholds on the weighted alpha, as the byte path does. A never-touched
  accumulator pixel reads transparent black → channel 0. Blend If is admitted
  on raster layers and isolated groups only (the `COLOR_MODES` target set);
  adjustments and Pass Through groups refuse, and a group carrying Blend If
  refuses `blend_mode: Pass Through` (core AND the Swift
  `NativeLayerBlending.allows` gate).
- **Clip units (the byte `composite_styled_into` clip path,
  `style_composite.rs` + `clamp_alpha_to_shape`).** The FOOTPRINT is the base's
  UNWEIGHTED alpha × mask (the byte `Shape::of_layer` rule — no Blend If, no
  fill), recorded from `buf` right after the base renders and BEFORE the
  weight is applied. The base's Blend If is then evaluated per pixel between
  the base's rendered `buf` colour (a raster base's read-back is within 1 ulp
  of its source colour, which the code rounding absorbs; a group base's is its
  rendered colour) and the accumulator beneath the unit, and multiplies the
  `buf` alpha where `buf` alpha > 0. Members composite onto `buf` and their
  alpha is CLAMPED, `pixel[3] = pixel[3].min(base_alpha[i])`, never forced —
  so members stay visible where the base's ramp hides the base (a base at
  weight 0 under an opaque member shows the member at the footprint alpha). A
  base without Blend If keeps the existing force-equal loop (bit-identical). A
  member's Blend If reads the clip buffer as its underlying.
- **Relative to the byte render.** For a document whose codes are all
  promotions (`257·c8`) over a DIRECT backdrop (transparent, or exactly one
  opaque unmasked opacity-1 raster), the red/green/blue this-layer and
  underlying weights are BIT-IDENTICAL to the byte compositor's (same
  correctly rounded quotient of the same rational), so with opacity 1 the f32
  accumulator is identical and `|typed − 257·byte| ≤ 129` codes; gray keeps
  sub-8-bit luminosity, so its rounding may land on the other side of a knee
  (documented, recorded, not matched). Composited backdrops round at 16-bit
  rather than 8-bit granularity: the depth difference, not an error. The
  clip-unit rule above holds this parity for an UNMASKED base (a masked byte
  footprint is u8-rounded coverage; a typed one is f32 — documented, not
  matched).
- **Bands.** `BLEND_IF_BAND = 2^-5` codes: a channel value (underlying
  composite, or gray luminosity of either side) whose exact f64 code value is
  within the band of a half-integer may round to the neighbouring code
  (derivation, direct inputs: unit rounding ≤ 2^-25·65535 ≈ 0.002 codes +
  product rounding 2^-8; gray adds 3 products + 2 sums ≤ 0.016; 2× slack).
  Direct inputs = the threshold slice's definition. **Strong form** (direct,
  every in-play channel value's margin ≥ band): every RGB/alpha component is
  within 0.65 codes of the f64 reference composite using the oracle's weight.
  **Weak form** (composited or in-band): within 0.65 codes of ONE of the
  references built from the neighbouring-code weights, never a mixture; tests
  `eprintln!` whether the oracle's code was taken. Rejected: continuous
  (unrounded) evaluation — a hard edge could not be made exact at the knee
  code and steep ramps carried ≈2/span codes of f32 error; evaluating the
  source ramp on raw codes but the underlying on unrounded floats — two
  footings for one weight.

## Measured maxima (core/tests/precision_blend_if.rs, 2026-09-15)

| case | form | result |
| --- | --- | --- |
| every code 0…65535 on red/green/blue/gray over an opaque direct backdrop, six ramp pairs (hard `[100,100,255,255]`, soft `[10,20,240,255]`, joined-high `[0,0,200,200]`, identity, underlying `[64,96,255,255]`, both), opacity 1 and 0.625, one mask-density-0.75 run | strong | max 0.506 codes; code 25699 hidden and 25700 shown exactly on the hard edge |
| the same runs, gray odd codes `[x, x, x+7]` (luminosity fraction 0.5054, inside the band) | weak | 393,168 in-band pixels, max 0.504 codes, the oracle's code taken on every one |
| two partial-alpha layers beneath (composited backdrop), red and gray underlying ramps | weak | 8,192 pixels, oracle's code taken on all, neighbouring code on 0 |
| masked backdrop (colour read, not colour × coverage); transparent canvas (hidden unless `lo0 == 0`) | weak / exact | within 0.65; all-zero and source-exact renders |
| byte parity, red channel, hard/soft/underlying ramps, opacity 1 and 0.625 | `\|typed − 257·byte\|` | worst 128 codes, 0 pixels past 129; hard-edge split at column 100 in both |
| byte parity, gray channel | recorded | 2 pixels past 129 (20,046 and 12,593 codes: the hard edge at 100 lands on the other side of the sub-8-bit luminosity), the soft ramp within 128 |
| byte parity, UNMASKED partial-alpha clip base with `this_layer [0,0,100,100]` and an opaque clipped member | `\|typed − 257·byte\|` | worst 86 (opacity 1) / 116 (0.625) codes, 0 past 129; the member at the footprint alpha where the base is hidden |
| isolated group with Blend If; clip base with Blend If over a transparent canvas (hidden base under an opaque member shows the member at the footprint alpha, under no member is transparent); member with Blend If reading the clip buffer | within 0.65 | pass |
| Dissolve + Blend If, soft ramp over 65,536 codes | threshold on the weighted alpha | 38,494 shown, 27,041 hidden, 1 within `DISSOLVE_BAND` skipped |

## Refusals

- `layer_blend_if`: version ≠ 1, an unknown request or `blend_if` field, an
  unknown channel, an unordered or non-four-entry ramp, an adjustment or Pass
  Through target, an out-of-range layer — refused in plan and edit.
- `with_blend_mode(PassThrough)` on a group carrying Blend If (core), and the
  Swift `allows` gate before it.
- Native: an empty `LBIF` table, count > records, an unordered/duplicate/
  out-of-range index, channel > 3, an unordered ramp, an identity entry, an
  entry on a Pass Through or adjustment record, `LBIF` under a non-35 header,
  35 nested inside 32, truncation and trailing bytes refuse before any PNG
  decodes.
- A typed source asset (embedded original) refuses a layer carrying Blend If,
  as the byte path refuses a styled one.

## Receipts as run (2026-09-15)

- `cargo fmt --check && cargo clippy --all-targets -- -D warnings`: clean.
- `cargo test --release --test precision_blend_if`: 8 passed, 0 failed.
- `style_blend_if`, `style`, `precision_clipping`,
  `precision_threshold_blending`, `precision_hsl_blending`,
  `precision_blend_mask_wrappers`, `precision_layer_blending`,
  `precision_document`, `psd_depth16`, `psd_constructs`, `psd_export_corpus`,
  `psd_export`, `psd_feature_markers`: all pass unchanged.
- `make test`: 166 test binaries report `ok`, 1,578 passed, 0 failed, 2
  pre-existing ignored, exit 0.
- `python3 scripts/verify_phase10d_blend_decoder.py`: eleven PASS lines
  (the new `LCLP`/`LBIF` tables and the `32 → 29 → 34` / `35 → 32 → 29 → 34`
  whole-file compositions with 13 malformed variants).
- `make typecheck`: 0 errors. `make app`: BUILD SUCCEEDED.
- **The slice's own MCP verifier (Run 5 of the closeout, 2026-09-16).**
  `scripts/verify_phase10d_blend_if.py`, receipt
  `~/devel/rasterize-fixtures/phase10-receipts/blend-if/result.json`, app
  SHA-256 `44edd1fde2b2f8741bbd9c735becb0278be4e25fe1b8a81b38ea4c87aa407858`:
  gates B1–B5 (publication, appearance within 1 code, native 35 roundtrip,
  the 16-bit PSD ranges, and the atomic adjustment-layer refusal) all pass.
- MCP drive of the built app (port 4917, dummy key, private preset/recovery
  directories): `open_document` a 16-bit `.rz` → `set_layer_style`
  `{blend_if:{channel:"gray",this_layer:[0,0,128,160],underlying:[0,0,255,255]}}`
  → `get_document` shows the style object → `render` → `undo`/`redo` →
  `save_copy` (header `RZDC` 35; `native.py` reads the `LBIF` entry) → reopen
  shows it → `inspect_psd_export` lists the Blend If loss → `export_psd`
  16-bit → `open_document psd_mode: layers` shows style null → a
  `drop_shadow` style on the 16-bit document returns `isError` with the
  limitation → `group_layers` the layer, `set_layer_style` Blend If on the
  isolated group, `set_layer_properties blend_mode:"Pass Through"` returns
  `isError` whose text is `NativeLayerBlending.limitation` → `set_layer_style`
  on an 8-bit document unchanged. No unexpected `isError`.

## Photoshop 15+1-bit agreement (Run 3, 2026-09-16) — MEASURED

[Run 3 of the closeout](phase-10c-constructs-status.md#photoshop-appearance-agreement-run-3-2026-09-16)
took A4 with `scripts/verify_phase10_photoshop_appearance.py --receipt A4`
against **Adobe Photoshop 2025, 26.10.0** (executable SHA-256
`a502d3b62279299a77f85d69a3d8f3d5379e0fb406d40d746a00ee49031bfa04`), app
SHA-256 `a135ef31389df8c3d94ab078adcd296c674b60478611d3b1e17897e2b9906f8c`,
receipt `~/devel/rasterize-fixtures/photoshop/blend-if/receipt.json`.

### What 15+1 bit actually means, and how the pictures are built

Photoshop's 16-bit internals hold only the codes `round(k * 65535 / 32768)`.
Its own 8 -> 16 promotion is
`ps16(v) = round(round(v * 32768 / 255) * 65535 / 32768)` (measured: 0
mismatches over the 240 distinct values of the base picture), so a 16-bit code
`c` survives a Photoshop round trip iff `q16(c) == c` with
`q16(c) = round(round(c * 32768 / 65535) * 65535 / 32768)`. Two consequences
this run depends on: an 8-bit knee `k` sits within ONE 16-bit code of
Rasterize's `257 * k` (`ps16(v) == q16(257 * v)`, since 257/65535 = 1/255), and
`257 * v` is representable for every EVEN `v <= 126` and for no `v >= 128`
(`ps16(128) = 32895`, `ps16(192) = 49343`). Every 16-bit picture handed to
Photoshop here is written in representable codes — the runner asserts it at
write time — which is why the exactness gate below can stay at 0 and why the
overlay colour is `#40607c` rather than `#4080c0`.

### Three directions, because Photoshop's own 16-bit records are unreadable

Photoshop writes its 16-bit layer records into an `Lr16` tail block with
ZIP-with-prediction channels and an EMPTY layer-information section (Run 2's
G5), which this build does not read. There is therefore **no
Photoshop-authored 16-bit direction**; Run 4's F1 owns that reader.

1. **PS -> R, 8 bit.** Photoshop authored the committed fixture
   `core/tests/fixtures/psd_photoshop/blend_if/` (128 KB, six files: the
   description, the three PNGs, `source.psd`, `photoshop.json`) — a base plus
   four full-canvas overlays, each with one Blend If ramp — and Rasterize
   imported and rendered it.
2. **R -> PS, 16 bit.** Rasterize authored the same four ramps in a typed
   document over the `ps16`-promoted base, exported `blendif16.psd`, and
   Photoshop rendered it.
3. **R -> PS, 16 bit, the KNEE codes.** A ramp picture whose red rows are
   `q16(25668 + x)` carries both knees; the rendered knee is read on row 16 as
   the code AT the first/last visible column — never `25668 + column`, because
   `q16` merges neighbours (columns 31 and 32 both hold 25700).

The Blend If setter itself is a finding: the charID `Dstm` is UNREGISTERED in
26.10.0, so the ranges only go in through the string-ID `blendRange` list, and
they do not read back at all (`executeActionGet` has no `blendRange` key). The
written record bytes are the only readout, which is exactly what gate B1 reads.

### The measured rows

Tolerances were fixed before the run and are not edited after it: a soft ramp
by max code (<= 4), a hard ramp by the share of pixels that differ at all
(<= 2 %), because one code across a hard knee moves a pixel by the whole
difference. Errors are 8-bit code units; a 16-bit difference is divided by 257.

| case | ramp | direction / depth | tolerance | max / mean / differing | |
|---|---|---|---|---|---|
| `this_gray_soft` | this gray `[32,96,160,224]` | PS -> R, 8 | 4 codes | 116 / 15.19 / 64.5 % | miss |
| `this_red_hard` | this red `[155,155,255,255]` | PS -> R, 8 | <= 2 % px | 0 / 0 / 0 % | **pass** |
| `under_red_soft` | under red `[16,80,176,240]` | PS -> R, 8 | 4 codes | 3 / 0.22 / 29.2 % | **pass** |
| `under_gray_hard` | under gray `[100,100,255,255]` | PS -> R, 8 | <= 2 % px | 191 / 4.56 / 5.6 % | miss |
| `this_gray_soft` | (same) | R -> PS, 16 | 4 codes | 116.03 / 15.09 / 65.1 % | miss |
| `this_red_hard` | (same) | R -> PS, 16 | <= 2 % px | 0 / 0 / 0 % | **pass** |
| `under_red_soft` | (same) | R -> PS, 16 | 4 codes | 0.02 / 0.00 / 40.2 % | **pass** |
| `under_gray_hard` | (same) | R -> PS, 16 | <= 2 % px | 191 / 4.59 / 5.6 % | miss |
| `knee_this` | this red `[155,155,255,255]` | R -> PS, 16 | 4 codes | knee **39835** vs `257 * 155` = 39835 | **pass** |
| `knee_under` | under red `[100,100,255,255]` | R -> PS, 16 | 4 codes | knee **25700** vs `257 * 100` = 25700 | **pass** |

Read that honestly, because the split is clean:

- **Every RED-channel case agrees**, and the 16-bit KNEES land on the exact
  code `257 * k` from BOTH sides: Photoshop's render and Rasterize's own put
  the transition at 25700 and 39835, which is the 15+1-bit claim this document
  made and could not previously measure. `this_red_hard` is 0 codes over the
  whole canvas at both depths; `under_red_soft` is 3 codes at 8 bits and 0.02
  at 16.
- **Both GRAY cases miss**, by the same amount in both directions (max 116 and
  191, 5.6 % of pixels on the hard one): Photoshop's "Gray" channel for a
  blending range is a different luminosity than
  `style_blend_if::channel_value`'s. That is one arithmetic difference in one
  place, not a Blend If failure — the ramps themselves cross the format
  intact, which gate B2 proves independently.
- The two directions agree with each other to 0.03 codes, so nothing here is a
  depth effect; the 16-bit path is the 8-bit path.

Closing the gray gap means changing Rasterize's gray-channel arithmetic, which
is a separate decision and NOT part of this slice's contract.

### Structural gates (the run's exit status)

All four passed. **B1** the record framing: 5 records, 40-byte ranges on each,
and on every overlay exactly the ONE authored slot away from the unrestricted
`[0,0,255,255]` default. **B2** the import: layered, `rgba8`, 5 rows, no loss,
and `style.blend_if` equal to the authored ramp on all four overlays. **B3**
the base renders exact at 0 codes in all three directions, including the knee
picture. **B4** Rasterize's OWN knee readout is exactly 25700 / 39835.

`core/tests/psd_photoshop_constructs.rs` pins the same file through the C API,
including a finding B2 exposed: Photoshop writes a full `lfx2` on an ordinary
pasted layer, so each overlay also carries four DISABLED effect slots
(outer glow, satin, inner glow, bevel & emboss) which render nothing.

Re-runnable (drop `--author` to render the committed `source.psd` instead of
re-authoring it in Photoshop):

```sh
PYTHONDONTWRITEBYTECODE=1 python3 -B scripts/verify_phase10_photoshop_appearance.py \
  --receipt A4 --app build/Build/Products/Release/Rasterize.app \
  --output ~/devel/rasterize-fixtures/photoshop/blend-if --author
```

## Open

- Typed fill opacity and layer effects have since shipped
  ([layer-styles slice](phase-10d-layer-styles-status.md)). Adjustment-layer blending has since shipped ([adjustment-blending slice](phase-10d-adjustment-blending-status.md): the same twenty-six modes on a typed adjustment under the byte `composite_adjustment_into` rule, conditional v37/schema7), which verifies P7-19; knockout has no byte implementation to migrate and is tracked as P7-37 under 12E.
  Writing Blend If ranges into PSD at either depth SHIPPED in 10C-2 slice 3.
- A live typed preview in the Layer Style sheet stays open.
- The dedicated MCP verifier is TAKEN (Run 5 of the closeout, 2026-09-16):
  `scripts/verify_phase10d_blend_if.py --app <bundle> --output
  ~/devel/rasterize-fixtures/phase10-receipts/blend-if`, receipt
  `~/devel/rasterize-fixtures/phase10-receipts/blend-if/result.json`, app
  SHA-256 `44edd1fde2b2f8741bbd9c735becb0278be4e25fe1b8a81b38ea4c87aa407858`.
  All five gates fixed before the run PASS: B1 (`get_document` publishes the
  authored red ramp as `style.blend_if` with `effects: []`, and an identity
  ramp leaves `style` null), B2 (the render at opacity 1 and 0.625 within 1
  code — worst 0.50 — of an independent f64 `ramp_weight_at` oracle with knees
  at `257·k`), B3 (`RZDC` 35 with `LBIF` `[(1, red, this, underlying)]`,
  reopen at `precision: preserve`, Undo / Redo / `run_action` byte-identical,
  the identity ramp restoring the baseline bytes), B4 (16-bit `export_psd`
  naming the Blend If once, red slot bytes 8..16 carrying the two ramps with
  every other slot `[0, 0, 255, 255]`, layered reopen publishing the same
  ramp) and B5 (a Blend If on an adjustment layer refusing atomically). Its
  8 x 8 gradient keeps every read code at least 3432 codes from every knee, so
  no weight is decided by a rounding difference.
- The GIMP reopen receipt was TAKEN 2026-09-16
  (`~/devel/rasterize-fixtures/phase10-receipts/R7/receipt.json`, app SHA-256 `084b4be2b97b1c84b9749e861c066bec9b72abbefb627c37f7e7c9d7aa8052a6`; re-runnable as
  `scripts/verify_phase10_receipts.py --receipts R7`): a 16-bit export of a
  layer whose gray ramp `[250,255,255,255]` excludes it everywhere wrote the
  40-byte ranges exactly — `[250,255,255,255]` then `[0,0,255,255]` in the gray
  slot, every other slot at the unrestricted default — and Rasterize's own
  preview equals the bare base picture. GIMP 3.2.6 does not apply them: it
  renders the UNRESTRICTED source-over, matching an independent source-over of
  the decoded record at 0 codes, so the ranges cross the format intact but the
  reading application ignores them.
- Agreement with Photoshop's 16-bit (15+1 bit) Blend If arithmetic is MEASURED above,
  not reported: the red-channel cases and both 16-bit knees land on
  Photoshop's own codes, and the two gray cases are recorded with their gap
  (max 116 / 191 codes — a different gray luminosity, not a different ramp).
  A MASKED styled clip base's footprint is now the byte u8 shape (the
  layer-styles slice); a Blend If-only base keeps the f32 rule.
- P7-19 is verified by the adjustment-blending slice; P7-20 by the
  layer-styles slice; P7-21 is closed by 10C-2 slice 3.
