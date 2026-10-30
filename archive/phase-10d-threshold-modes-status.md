# Phase 10D: four threshold native layer modes (Hard Mix, Darker Color, Lighter Color, Dissolve)

The [Blend If slice](phase-10d-blend-if-status.md) has since added typed Blend
If with conditional native v35/`LBIF` outside the v34 wrapper described below
(`35 → 32 → 29 → 34 → body/tails → LBLD6 → MOUT → LCLP → LBIF`); schema6 and
every older schema stay frozen.

This bounded slice admits Dissolve14, Darker Color16, Lighter Color17 and Hard
Mix21 for native RGBA16 raster layers and isolated groups through the
UNCHANGED f32 `blend.rs` kernels, under the explicitly versioned precision
policy below. Existing native modes, ordinary byte rendering, adjustment
Normal-only admission and group-only Pass Through retain their meanings. With
this slice every raster/isolated-group mode the byte compositor offers is
qualified at native depth; Blend If, layer styles, knockout and
adjustment-layer blending remain outside. Full 10D and P7-19/P7-20 remain
open; the carryover ledger is unchanged at 18 verified / 18 open.

## Meaning, host routing and persistence

The four modes are the byte compositor's own definitions over encoded document
RGB (`core/src/blend.rs`, unchanged):

- Hard Mix, per channel: `B(cb, cs) = 1` when `cb + cs >= 1`, else `0`; a tie
  (`cb + cs == 1`) is 1.
- Darker Color / Lighter Color, whole triple: the triple with the lower /
  higher luminance `lum = 0.3 R + 0.59 G + 0.11 B` (f32 constants); the
  backdrop on a tie.
- Dissolve replaces the compositing formula: the source shows FULLY OPAQUE
  (its straight colour, alpha 1) when the canvas-absolute threshold
  `t(x, y) < sa`, where `sa` is the effective alpha
  (`alpha × opacity × mask coverage`); otherwise the backdrop pixel is
  untouched. `t` is the documented 24-bit Murmur3-style mix
  `h = x·0x9E3779B9 + y·0x85EBCA6B` (wrapping u32); `h ^= h>>16;
  h *= 0x85EBCA6B; h ^= h>>13; h *= 0xC2B2AE35; h ^= h>>16; t = (h>>8)/2^24`,
  exact in f32 and strictly below 1.

No renderer change was needed: `core/src/precision_render.rs::render_layers`
already passes canvas-absolute `(x, y)` to `composite_source_into` for rasters,
groups and clip units (the typed accumulator is the whole canvas at origin
0,0), so Dissolve's hash in the typed document is the byte compositor's hash,
and a translated layer changes which canvas positions show rather than the
pattern. Byte and typed evaluation are MATCHED bit-for-bit on equal normalized
f32 inputs; nothing is deliberately unmatched.

The authoritative Rust (`precision_layer_blend::COLOR_MODES`, now 27 entries)
and Swift (`NativeLayerBlending.colorModes`) mode sets feed the existing Layers
popup, MCP `set_layer_properties`, Actions and atomic captured publication.
There are no new FFI signatures or edit-request schemas: the version1
`layer_blend` request now admits `blend_mode` 14/16/17/21 on raster and
isolated-group targets. Raw source RGB/alpha/hidden RGB, masks, placement,
descriptions, retained masters, channels, ICC/PPI and metadata remain exact
through metadata edits and persistence.

Conditional RZDC34/LBLD schema6 adds exactly 14/16/17/21 to the previous
twenty-two nondefault modes. Schemas 1–5 remain explicitly frozen: a
v26/27/30/31/33 file patched to carry 14/16/17/21 refuses naming the blend
mode. The writer selects the oldest sufficient blend wrapper: 34, 33, 31, 30,
27, 26 or no blend wrapper. Its capacity stays `16 + 8 × nondefault_record_count`
bytes; the plan (`precision_native_plan.rs`) charges by count only and was not
changed. The full wrapper order is
`32 → 29 → 34 → prior body/tails → LBLD6 → MOUT → LCLP`; 34 never wraps 29 or
32, and releasing the last threshold mode restores the prior writer's exact
bytes (v33, then v31/v30/v27/v26/body as each further mode is released).
`RZDC_VERSION` is 34 so a byte read of a 32/33/34 file refuses naming the typed
reader; `isPrecisionNative` admits 34.

16-bit PSD carries the four modes in both directions through the existing
`PSD_BLEND_KEYS` table: `diss`/`dkCl`/`lgCl`/`hMix` import as the typed mode
with no loss sentence on rasters and folders, and the export writes those
keys. Since this slice no raster or folder blend key is unrepresentable in the
typed model; adjustment records still import Normal.

## Precision policy (v34)

- **Kernels.** `b_hard_mix` (`cb + cs >= 1.0` → 1, tie → 1),
  `b_darker_color`/`b_lighter_color` (whole-triple pick by
  `lum = 0.3R + 0.59G + 0.11B` in f32; backdrop on a tie),
  `BlendKind::Dissolve`/`dissolve_threshold` in `core/src/blend.rs` are used
  as-is. No renderer change (above). Byte and typed evaluation are MATCHED
  bit-for-bit on equal normalized f32 inputs; nothing is deliberately
  unmatched.
- **Margin `m`** is computed by the independent f64 oracle
  (`core/tests/common/native_blend_oracle.rs`) from exact authored codes: Hard
  Mix `m_c = |cb_c + cs_c − 1|` per channel; Darker/Lighter Color
  `m = |lum(cs) − lum(cb)|` per pixel; Dissolve `m = |sa − t(x,y)|` where `sa`
  is the exact effective alpha (`alpha × opacity × mask coverage`) and `t` is
  the documented 24-bit hash above, re-implemented on integers.
- **Bands.** `HARD_MIX_BAND = 2^-21`, `COLOR_BAND = 2^-20`,
  `DISSOLVE_BAND = 2^-20` (normalized units). Derivation is for DIRECT inputs
  only: a u16→f32 conversion (`PrecisionImage::pixel`, ÷65535 in f32) carries
  ≤ 2^-25 absolute error; Hard Mix adds one f32 sum (≤ 2^-23 total); `lum` is
  3 products + 2 sums with f32 constants (≤ 4·2^-24 per side, 8·2^-24 for
  both, 2× slack); `checked_alpha` is two f32 products on direct
  alpha/opacity/coverage (≤ 3·2^-25) and `t` is exact in f32. The composite
  step (`blend.rs:430`, ≤ 5 roundings plus a division by `ao`) is NOT
  budgeted, which is why the strong form below is restricted.
- **Direct inputs (definition).** The source colour is a stored raster's codes,
  and the backdrop is either the transparent canvas or exactly one opaque,
  unmasked, opacity-1 raster's codes (that first composite is exact:
  `sa = 1, ab = 0` gives `out = cs`, `ao = 1`). For Dissolve, `sa` is direct
  when the layer is a raster (alpha code × opacity × mask coverage). Anything
  else — a group source, a backdrop with partial alpha or more than one
  contributing layer, a Dissolve group's accumulated alpha — is COMPOSITED.
- **Strong form (direct inputs only).** If `m ≥ band`, the render takes the
  oracle's branch and every RGB/alpha component lies within 0.65 native codes
  of that branch's f64 reference, final u16 quantization included.
- **Weak form (every input, direct or composited).** The output lies within
  0.65 codes of ONE of the two branch references (per channel for Hard Mix,
  whole triple for Darker/Lighter Color, shown/hidden for Dissolve) — never a
  mixture — and the pick is a pure function of the f32 intermediates, frozen
  as the v34 evaluation. No branch guarantee is claimed for composited inputs
  at any margin; tests RECORD (eprintln) whether the oracle's branch was taken
  and this document reports the smallest composited margin observed with
  agreement. Dissolve additionally: a shown pixel equals the source codes
  exactly with alpha 65535; a hidden pixel equals the render without that
  layer exactly.
- **Explicitly in-band by construction:** authored Hard Mix pairs with
  `b + s = 65535`; equal-luma triples; the five-modes nested-group witness
  (margin 1/65535²). These are documented, not refused.
- **Rejected alternatives.** Quantize-then-compare: intermediate group
  quantization, typed-only kernel divergence. f64 accumulator: changes every
  prior persisted evaluation, still a band at 2^-53. Per-depth composite
  bands: fixture-specific budgets the reader cannot apply to an arbitrary
  document.

### Measured maxima (Release, `precision_threshold_blending.rs`)

Maximum error in native codes against the matched branch's unrounded f64
reference, including final quantization. Strong rows are direct pixels with
`m ≥ band` (branch asserted); weak rows are composited or in-band pixels
(one-of-two asserted, branch recorded).

| Fixture | Form | Darker Color | Lighter Color | Hard Mix |
| --- | --- | ---: | ---: | ---: |
| Grid (43 × 43 code pairs + 10 × 10 asymmetric triples), opaque base, opacity 1 | strong | 0 | 0 | 0 |
| Grid, opacity 0.625 | strong | 0.500001 | 0.500001 | 0.5 |
| Every mask code × alpha × density/opacity, base alpha 65535 | strong | 0.504431 | 0.504959 | 0.504049 |
| Every mask code × alpha × density/opacity, base alpha 0/1/32769 | weak | 0.507701 | 0.505351 | 0.505351 |
| Translated nested group in Pass Through | weak | 0.434264 | 0.455943 | 0.385662 |

Values are rounded upward. The grid's in-band pixels by construction (the
oracle's classification agrees with the authored predicate on every pixel):
53 per colour mode (the 43 `b == s` diagonal pairs plus the 10 identical
asymmetric pairs, margin 0) and 32 for Hard Mix (`b + s = 65535` pairs); all
of them landed on the oracle's branch at both opacities. The Hard Mix
exhaustive direct fixture (width 65536, gray backdrop `b = x`): `s = 65534 − x`
rendered 0 and `s = 65536 − x` rendered 65535 at every pixel (margin 1/65535
≥ 2^-21, strong); `s = 65535 − x` (exact sum, 65536 in-band pixels) rendered
65535 at every pixel — **0 of 65536 in-band pixels picked zero** — with every
output in {0, 65535}. Alpha error was ≤ 0.65 everywhere.

Composited branch agreement (recorded, not asserted): the mask fixture's
49,152 composited pixels per mode all took the oracle's branch, smallest
agreeing margin 0 (the equal-colour pairs); the group fixture's 3 composited
pixels per mode all took the oracle's branch, smallest agreeing margin
0.029592 (Darker/Lighter Color) and 0.029847 (Hard Mix). No composited pixel
disagreed with the oracle in these fixtures.

The re-homed five-mode Hard Mix witness (backdrop 65534, isolated group of
opaque black plus `[2, 2, 2, alpha]`, exact sum `65534/65535 + 2·alpha/65535²`
straddling 1 by 1/65535², margin < `HARD_MIX_BAND`) is admitted on the group
(layer 3) and on the backdrop (layer 0; that render is unchanged). Both
alphas rendered `[65535, 65535, 65535, 65535]` — one branch, never a mixture;
`f64::from(65534f32/65535) + f64::from((2f32/65535)·(32767f32/65535)) == 1`
still holds, which is why no branch is claimed there.

Dissolve (width 4096, backdrop alpha cycle 65535/32769/1/0, source alpha cycle
65535/32768/1/0/45001/60000, mask codes `37x mod 65536` at density 0.75):

| Fixture | Form | Shown / hidden | In-band | Smallest strong margin | Max shown colour error |
| --- | --- | --- | ---: | ---: | ---: |
| Raster, opacity 1, offset (0,0) | strong | 1236 / 2860 | 0 | 1.84e-6 | 0 (exact) |
| Raster, opacity 1, offset (1,0) | strong | 1204 / 2892 | 0 | 1.97e-4 | 0 (exact) |
| Raster, opacity 0.625, offset (0,0) | strong | 768 / 3328 | 0 | 2.91e-5 | 0 (exact) |
| Raster, opacity 0.625, offset (1,0) | strong | 737 / 3359 | 0 | 3.47e-4 | 0 (exact) |
| Isolated group, opacity 1 | weak | 1669 shown | — | agreeing 6.18e-5 | 0.472014 |
| Isolated group, opacity 0.625 | weak | 1053 shown | — | agreeing 1.16e-4 | 0.472014 |

Every raster pixel was either the exact source codes at alpha 65535 or the
exact no-layer render; the (0,0) and (1,0) patterns differ. Every group pixel
was either the exact no-layer render or the group colour within 0.65 codes at
alpha 65535, and all 4096 composited pixels at both opacities took the
oracle's branch.

## Qualification scope

`core/tests/precision_threshold_blending.rs` (independent f64 oracle in
`core/tests/common/native_blend_oracle.rs`: `threshold`, `dissolve_threshold`,
the three band constants; `assert_policy` applies the strong/weak forms per
pixel): the exhaustive direct Hard Mix codes; the code-pair grid plus
asymmetric triples at opacity 1 and 0.625 with the authored in-band predicate
asserted against the oracle's classification; every native mask code with
tiny/partial/opaque alpha and density/opacity (`direct = base alpha == 65535`
per pixel); translated nested isolated groups in each mode inside a Pass
Through container (weak form only) plus Pass Through equality, rename and
duplicate, and the re-homed Hard Mix witness; Dissolve rasters at two offsets
and two opacities with a density mask (strong) and a Dissolve isolated group
(weak); exact v34 downgrade through v33/v31/v30/v27/v26/body with unchanged
`encoded_bound` and frozen-schema refusals for (26,1)…(33,5); strict FFI
no-ops, hidden/adjustment/pass-through targets, capacity, cancellation and
predecode refusals for every corrupted schema/count/index/mode/body word,
truncation and trailing byte; and a 16-bit PSD round trip (one raster per mode
→ `diss`/`dkCl`/`lgCl`/`hMix` → modes 14/16/17/21, empty `verified_losses`,
projection equal to the source render) plus an independently authored closed
`hMix` folder at opacity 128 importing as an `isolated_blend_group` with mode
21.

Updated existing tests: the basic, four-mode, five-mode and HSL refusal lists
drop 14/16/17/21 (the five-mode binary's Hard Mix refusal test is deleted and
re-homed above); `precision_blend_mask_wrappers.rs` adds 14/16/17/21 at
version 34 to both wrapper tests (including the v33/schema5 frozen check and a
mode-21 hidden-table case whose corrupt word is 27).
`verify_phase10d_blend_decoder.py` adds the schema6 valid/malformed/frozen
cases, four v34 mask29 compositions and four unwrapped v34 files;
`scripts/live-filter-probe/native.py` decodes v34/schema6 independently.
`verify_phase10d_five_modes.py` replaces its Hard Mix/Dissolve refusal cases
with a Pass-Through-on-raster refusal.

### Receipts (2026-09-14, implementation run)

- `cargo fmt --check` and `cargo clippy --all-targets -- -D warnings`: clean.
- `cargo test --release --test precision_threshold_blending`: 8 passed, 0
  failed.
- The listed regression binaries (`precision_remaining_separable` 6,
  `precision_four_blending` 5, `precision_hsl_blending` 7,
  `precision_blend_mask_wrappers` 3, `precision_layer_blending` 12,
  `precision_clipping` 11, `psd_depth16` 9, `psd_constructs` 8,
  `psd_export_corpus` 1, `precision_document` 8): all pass.
- Full Release suite (`make test`): **1,570 passed, 0 failed, 2 pre-existing
  ignored across 165 result groups**.
- `python3 scripts/verify_phase10d_blend_decoder.py`: eight PASS lines
  (schema6 adds 47 malformed/atomic cases, 20 frozen-old-schema refusals, four
  v34 mask29 compositions with 16 malformed variants and four unwrapped v34
  files with 16 old-schema refusals).
- `make typecheck`: 0 errors. `make app`: BUILD SUCCEEDED.
- MCP drive on an owned Release instance (port 4917, dummy key): a 16-bit
  `.rz` opened with `precision: preserve`, one duplicated layer at opacity
  0.625; for each of Hard Mix, Dissolve, Darker Color and Lighter Color:
  `set_layer_properties` → `render` → `undo` (Normal) → `redo` (mode) →
  `save_copy` (header `RZDC` 34, independent decoder reads `LBLD` schema6
  `[(1, mode)]`) → reopen shows the mode → `export_psd` (bit_depth 16, no
  losses) → `inspect_psd` (`rgba16`, no verified losses) → `open_document
  psd_mode: layers` shows the mode. No `isError` in any call.
- **The slice's own MCP verifier (Run 5 of the closeout, 2026-09-16).**
  `scripts/verify_phase10d_threshold_modes.py`, receipt
  `~/devel/rasterize-fixtures/phase10-receipts/threshold-modes/result.json`,
  app SHA-256 `44edd1fde2b2f8741bbd9c735becb0278be4e25fe1b8a81b38ea4c87aa407858`:
  gates T1 (appearance within 1 code, Dissolve exact), T2 (native 34
  roundtrip with Undo/Redo/Actions identical), T3 (16-bit PSD key with no
  loss) and T4 (atomic `Pass Through` refusal) all pass. Its 4 x 3 fixture is
  written by the script, with every channel sum at least 300 codes from the
  Hard Mix threshold and every luminance pair at least 1331 codes apart, so
  no branch is decided by a rounding difference; its blend check accepts
  either the mode name or the numeric blend id, because `get_document`
  publishes a typed layer's blend as an id; the placed foreground is
  detached with `rasterize_image_source` before the baseline, because a
  retained source is itself a preflight loss.

## Open

- Phase 3, typed Blend If, has since shipped in the
  [Blend If slice](phase-10d-blend-if-status.md), and phase 4, typed layer
  styles, in the [layer-styles slice](phase-10d-layer-styles-status.md).
  Adjustment-layer blending has since shipped ([adjustment-blending slice](phase-10d-adjustment-blending-status.md): the same twenty-six modes on a typed adjustment under the byte `composite_adjustment_into` rule, conditional v37/schema7), which verifies P7-19; knockout has no byte implementation to migrate and is tracked as P7-37 under 12E.
- An f64-intermediate SetSat (HSL near-gray conditioning) remains open.
- The dedicated MCP verifier is TAKEN (Run 5 of the closeout, 2026-09-16):
  `scripts/verify_phase10d_threshold_modes.py --app <bundle> --output
  ~/devel/rasterize-fixtures/phase10-receipts/threshold-modes`, receipt
  `~/devel/rasterize-fixtures/phase10-receipts/threshold-modes/result.json`,
  app SHA-256 `44edd1fde2b2f8741bbd9c735becb0278be4e25fe1b8a81b38ea4c87aa407858`.
  All four gates fixed before the run PASS: T1 (each of the four modes at
  opacity 1 and 0.625 within 1 code of an independent f64 oracle — worst 0.50
  — and Dissolve EXACT at 0 codes against the documented 24-bit hash), T2
  (`RZDC` 34 with `LBLD` `[(1, mode)]`, reopen at `precision: preserve`, Undo /
  Redo / `run_action` byte-identical, Normal restoring the baseline bytes), T3
  (16-bit `export_psd` with `losses == []`, `hMix`/`dkCl`/`lgCl`/`diss` in an
  independent decode, layered reopen showing the mode) and T4 (`Pass Through`
  on a raster refusing with bytes, history, selection and dirty state
  unchanged). No tolerance was edited after the run.
- The GIMP reopen receipt for the four keys was TAKEN 2026-09-16
  (`~/devel/rasterize-fixtures/phase10-receipts/R6/receipt.json`, app SHA-256 `084b4be2b97b1c84b9749e861c066bec9b72abbefb627c37f7e7c9d7aa8052a6`; re-runnable as
  `scripts/verify_phase10_receipts.py --receipts R6`): a 16-bit export whose
  four layers carry `hMix`, `dkCl`, `lgCl` and `diss` reopened in GIMP 3.2.6
  as `hard-mix`, `luma-darken-only`, `luma-lighten-only` and `dissolve`, with
  exact names, samples, opacity bytes and structure and a 0-code merged
  preview; its layered projection differs from the stored composite by up to
  8133 codes — recorded, never gated, because GIMP's own arithmetic for these
  modes (and its stochastic Dissolve) is not this slice's claim.
- P7-19 is verified by the adjustment-blending slice and P7-20 by the
  layer-styles slice; P7-21 is closed by 10C-2 slice 3, whose last receipt was
  taken 2026-09-16.
