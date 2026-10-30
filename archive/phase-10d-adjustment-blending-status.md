# Phase 10D: blend modes on 16-bit adjustment layers (v37 / `LBLD` schema 7)

This bounded slice gives a native RGBA16 ADJUSTMENT layer the same twenty-six
non-Normal blend modes a raster layer already has, evaluated by the byte
compositor's own adjustment rule and persisted as the conditional native
v37 / `LBLD` schema 7 word. Nothing else changes: every raster/isolated-group
mode, Pass Through's group-only rule, Blend If, layer styles, clipping and all
byte compositing keep their meanings, and every previously written file
evaluates bit-identically.

With this slice **P7-19 is verified**. Knockout (Shallow/Deep) is NOT part of
it and is not a 10D migration item: the byte compositor has never implemented
knockout (`core/src/style.rs` excludes it from the style model and
`core/src/psd_feature_markers.rs` discards it on import), so there is nothing
to migrate to typed evaluation. It is recorded as the new carryover **P7-37**
under [12E](professional-photo-editor-implementation-plan.md#12-high-depth-parity-and-remaining-workflows),
which owns a byte-side design first. See [Open](#open).

## Meaning, host routing and persistence

The typed renderer takes the byte compositor's rule verbatim
(`core/src/doc.rs::composite_adjustment_into`, unchanged): the layer's own
pixels are ignored, the adjustment is applied to the accumulated backdrop
colour, and the RESULT is pushed through the layer's blend mode as the SOURCE
against that same backdrop, then lerped back by `k = opacity × mask coverage`:

```
adjusted  = adjustment(backdrop_rgb)
effective = B(backdrop_rgb, adjusted)          # Normal's b_normal returns cs
out_rgb   = backdrop_rgb + (effective − backdrop_rgb) · k
```

- **Alpha is never changed**, and a fully transparent backdrop pixel
  (`acc[i][3] == 0`) is left entirely untouched, RGB included.
- **Dissolve** has no blend function and keeps its all-or-nothing dither: the
  pixel is fully adjusted when the canvas-absolute threshold
  `t(x, y) < k`, otherwise the backdrop pixel is untouched. `t` is the
  documented 24-bit Murmur3-style mix in `blend.rs::dissolve_threshold`. The
  typed accumulator is the whole canvas at origin (0,0), so the typed pattern
  IS the byte pattern.
- **Normal is bit-identical to the previous build.** `blend_kind(Normal)` is
  `Separable(b_normal)`, and `b_normal(cb, cs) = cs`, so the effective colour
  is the adjusted colour and the mix is the pre-slice `mix_adjusted_rgb` call.
- **Coverage gates exactly as before.** With no mask (or a disabled one) the
  adjustment is canvas-wide regardless of the record's placement; an enabled
  mask confines it and is 0 outside the mask's own extent.
- **The nonfinite guard is preserved structurally.** The typed arm's
  `"high-depth adjustment produced nonfinite RGB"` refusal is an existing
  typed-only contract with no byte twin. It stays on the kernel's own output,
  immediately after `apply_at` and BEFORE any blend branch, so the Dissolve
  shown branch cannot route a NaN/inf pixel to the accumulator; a SECOND check
  with the same text covers `effective` on the non-Dissolve path.

Host routing is unchanged. The authoritative Rust
(`precision_layer_blend::COLOR_MODES`, still 27 entries) and Swift
(`NativeLayerBlending.colorModes`) sets feed the same Layers popup, MCP
`set_layer_properties`, Actions and atomic captured publication. There are no
new FFI signatures and no new edit-request schema: the version-1
`layer_blend` request now admits `blend_mode` 1…26 on an adjustment target as
well. Admission moved from two booleans to a `BlendTarget`
(`Raster | IsolatedGroup | PassThroughGroup | Adjustment`), evaluated by the
same `precision_layer_blend::validate` every caller already used
(`with_blend_mode`, the edit's `target`, `native_preflight`,
`PrecisionLayer::with_blend_mode`), so no caller carries its own rule. The
refusal text is now

> native layer blending supports Normal, …, Darker Color, Lighter Color and
> Hard Mix on raster layers, adjustment layers or isolated groups; Pass Through
> requires a group

`precision_capability`'s `layer_blend` family lists `adjustment`, the Layers
blend popup is enabled on a typed adjustment (its per-item `allows` gate is
unchanged and still hides Pass Through), and `DocumentPrecision.support`
reports `adjustment_blend_modes` as the full mode-name list instead of
`["Normal"]`.

### Native format

Conditional **RZDC 37 / `LBLD` schema 7** adds **no mode**: its whitelist is
exactly schema 6's twenty-six. What it adds is a TARGET — a kind-3 adjustment
record may carry a mode. Framing is unchanged: `LBLD`, u32 schema = 7, u32
count, then strictly ascending `(u32 postorder index, u32 BlendMode)` pairs,
capacity `16 + 8 × nondefault_record_count`, and
`precision_native_plan.rs` charges by record count only, so `encoded_bound` is
identical to the v26 plan for the same document.

The writer selects 37 **iff some nondefault record is an adjustment**;
otherwise it keeps the oldest sufficient of 34/33/31/30/27/26 exactly as
before. 37 occupies the same slot those words occupy, so the full wrapper
order is

```
36 → 35 → 32 → 29 → 37 → prior body/tails → LBLD7 → MOUT → LCLP → LBIF → LSTY
```

37 never wraps 29 or 32, and releasing the last adjustment mode restores the
prior writer's exact bytes (v34 here, then 33/31/30/27/26/body as each further
mode is released).

Schemas 1–6 are explicitly frozen: a v26/27/30/31/33/34 file patched to point
at a kind-3 record still refuses with *native blend requires a raster or
isolated group*, before any PNG decode. A 37 header whose schema word is not 7
refuses naming the schema; a schema-7 table under a ≤ 34 header refuses the
same way; mode 27 (Pass Through) is never storable at any schema.
`RZDC_VERSION` is 37 and `isPrecisionNative` admits 37.

### 16-bit PSD

The export already wrote `blend_key(layer.blend_mode())` on the pixel-less
adjustment record (`psd_export16.rs`), so an adjustment at Multiply now writes
`mul ` instead of `norm` with no writer change. The import drops its
adjustment-only loss branch: a carried record takes the SAME `typed_blend`
path a raster does, so a recognized key becomes the typed mode and only a key
the typed model has no representation for is reported. The sentence

> …has the blend key X, which a 16-bit adjustment layer cannot represent; it is
> imported as Normal.

no longer exists.

## Precision policy (v37)

- **Kernels.** The unchanged f32 `blend.rs` table: `blend_kind`, the separable
  `B(cb, cs)` functions, the nonseparable HSL/whole-triple functions and
  `BlendKind::Dissolve`/`dissolve_threshold`. The mix is the shipped
  `blend::mix_adjusted_rgb`. No new pixel math, no intermediate quantization
  and no transfer-curve conversion is introduced. Byte and typed evaluation
  are MATCHED bit-for-bit on equal normalized f32 inputs; nothing is
  deliberately unmatched.
- **Oracle.** `out = cb + (B(cb, 1 − cb) − cb) · k` in f64 over an **Invert**
  adjustment, with `B` the independent equations in
  `core/tests/common/native_blend_oracle.rs` (`separable` — added by this
  slice — plus the existing `hsl`, `threshold` and `dissolve_threshold`). The
  oracle never calls `blend.rs`.
- **Continuous modes** (Normal, Multiply, Screen, Overlay, Soft Light, Hard
  Light, Darken, Lighten, Difference, Exclusion, Color Dodge, Color Burn,
  Linear Dodge (Add), Linear Burn, Subtract, Vivid Light, Linear Light, Pin
  Light, Divide, Hue, Saturation, Color, Luminosity): every RGB/alpha component
  is within **0.65 native codes** of the unrounded f64 reference, final u16
  quantization included.
- **The two divisor neighbourhoods keep their declared allowance.** Color
  Dodge, Color Burn and Vivid Light divide by a quantity that is a CANCELLED
  difference near an endpoint, so those pixels take the same 1.6-code
  finite-fixture allowance `precision_four_blending.rs` and
  `precision_remaining_separable.rs` declare, evaluated with the adjusted
  source `s = 65535 − b`: Color Dodge for `b ∈ [1, 1310]`, Color Burn for
  `b ∈ [64225, 65534]`, Vivid Light for `b ∈ [1, 655]`. The mechanism is
  exactly the documented one — `b_color_dodge` recomputes `1 − cs` from the
  ALREADY ROUNDED `cs = fl(1 − cb)`, so the quotient's denominator carries up
  to half an ulp of 1 (2^-25) against a numerator of size `cb`. Color Burn in
  this fixture is EXACT for the opposite reason: it recomputes `1 − cb` with
  the same f32 expression the Invert kernel used, so the quotient is exactly 1
  and `B = 0`.
- **Threshold modes** (Dissolve 14, Darker Color 16, Lighter Color 17, Hard
  Mix 21) are asserted under the v34 band policy's **WEAK form only**
  (`designs/phase-10d-threshold-modes-status.md`). An adjusted colour is a
  COMPOSITED input by that policy's own definition — it is a function of the
  accumulator, not of stored codes — so **no branch is guaranteed at any
  margin**. What is asserted: every channel lies within 0.65 codes of ONE of
  the two branch references (backdrop / adjusted triple for 16/17, `B = 0` /
  `B = 1` per channel for 21), never a mixture, and 16/17 never mix two
  triples. Tests RECORD whether the oracle's branch was taken; the measured
  agreement is reported below.
- **Dissolve additionally** is exact on both branches: a shown pixel equals
  `65535 − code` per channel with the backdrop's alpha (the Invert round trip
  `round(fl(fl(1 − fl(c/65535)) · 65535))` is verified equal to `65535 − c` for
  every one of the 65,536 codes), and a hidden pixel equals the render without
  that layer, exactly. Comparisons within `DISSOLVE_BAND` of the threshold
  accept either branch; no in-band pixel occurred in the fixtures.
- **Rejected alternatives.** Moving the nonfinite guard from the kernel output
  to `effective` (a Dissolve NaN would reach the accumulator through the
  `continue` branch, regressing an existing unconditional check). Resolving
  `blend_kind` again inside the adjustment arm (`render_layers` already
  resolves it once per layer). An f64 accumulator (changes every prior
  persisted evaluation). Widening the RGB tolerance to cover the threshold
  branch (the band policy exists precisely so that never happens).

### Measured maxima (Release, `core/tests/precision_adjustment_blending.rs`)

Maximum error in native codes against the unrounded f64 reference, including
final quantization. The grid is 49 opaque backdrop codes
(endpoints, both divisor neighbourhoods' boundaries and interior codes) as
`[c, 65535 − c, c + 173, 65535]`. The masked fixture is width 65536 with every
native mask code at density 0.75, opacity 0.625 and backdrop alpha cycling
0 / 1 / 32769 / 65535.

| Mode | Grid, opacity 1 | Grid, opacity 0.625 | Every mask code × density |
| --- | ---: | ---: | ---: |
| Normal (0) | 0.000000 | 0.375 | 0.505701 |
| Multiply (1) | 0.486626 | 0.499962 | 0.505586 |
| Screen (2) | 0.486626 | 0.500573 | 0.504852 |
| Overlay (3) | 0.500069 | 0.495385 | 0.505959 |
| Soft Light (4) | 0.505315 | 0.494966 | 0.505328 |
| Hard Light (5) | 0.500069 | 0.500020 | 0.505286 |
| Darken (6) | 0.000000 | 0.375 | 0.504120 |
| Lighten (7) | 0 | 0.375 | 0.505701 |
| Difference (8) | 0.000000 | 0.500001 | 0.506438 |
| Exclusion (9) | 0.500008 | 0.499981 | 0.505959 |
| **Color Dodge (10)** | **1.000000** | **0.750000** | **1.032774** |
| Color Burn (11) | 0 | 0.5 | 0.505408 |
| Linear Dodge (Add) (12) | 0 | 0.5 | 0.505165 |
| Subtract (13) | 0.000000 | 0.500001 | 0.506438 |
| Linear Burn (15) | 0 | 0.5 | 0.505408 |
| **Vivid Light (18)** | **0.500000** | **0.812500** | **0.808541** |
| Linear Light (19) | 0.000000 | 0.375000 | 0.506309 |
| Pin Light (20) | 0.000000 | 0.5 | 0.506409 |
| Divide (22) | 0.473936 | 0.5 | 0.505206 |
| Hue (23) | 0.488777 | 0.5 | 0.506958 |
| Saturation (24) | 0.000000 | 0.000000 | 0.000000 |
| Color (25) | 0.488777 | 0.5 | 0.507410 |
| Luminosity (26) | 0.488777 | 0.5 | 0.505250 |

Values are rounded upward; entries written `0.000000` are exact to within f64
noise (≤ 8.5e-12 codes). Only Color Dodge and Vivid Light exceed 0.65, and only
inside their declared neighbourhoods — every other pixel of those two modes is
within 0.65. Hue and Saturation need no SetSat conditioning here: inverting a
triple preserves its spread, so the qualified ratio `delta / s` is 65535 at
every pixel with a nonzero spread, far above the 6830 eligibility threshold
`precision_hsl_blending.rs` declares.

Threshold modes (weak form; branch recorded, not asserted):

| Fixture | Darker Color (16) | Lighter Color (17) | Hard Mix (21) |
| --- | ---: | ---: | ---: |
| Grid, opacity 1 — max code error | 0.000000 | 0.000000 | 0 |
| Grid, opacity 1 — oracle branch taken | 49 / 49 | 49 / 49 | 49 / 49 |
| Grid, opacity 0.625 — max code error | 0.375 | 0.375 | 0.5 |
| Grid, opacity 0.625 — oracle branch taken | 49 / 49 | 49 / 49 | 49 / 49 |
| Every mask code × density — max code error | 0.504120 | 0.505701 | 0.505164 |

Smallest agreeing margin on the grid: 5.725e-4 for Darker/Lighter Color and 0
for Hard Mix (the `cb + cs = 1` identity of an inverted source makes EVERY
Hard Mix pixel in-band by construction, and every one of them took the oracle's
branch — `B = 1` on the tie). **No composited pixel disagreed with the oracle
in any fixture**, and no pixel was ever a mixture of the two branches.

Dissolve (49-code grid, one row, canvas-absolute hash re-implemented on
integers):

| Fixture | Shown | Hidden | In-band | Shown colour error |
| --- | ---: | ---: | ---: | ---: |
| Opacity 1 | 49 | 0 | 0 | 0 (exact `65535 − code`) |
| Opacity 0.625 | 32 | 17 | 0 | 0 (exact `65535 − code`) |

Every hidden pixel equalled the backdrop codes exactly, and alpha was exact
(not merely within 0.65) at every pixel of every mode in every fixture.

## Qualification scope

`core/tests/precision_adjustment_blending.rs` — 14 tests, black-box through the
typed model and the edit/inspect FFI:

- the continuous grid at opacity 1 and 0.625 (all 23 continuous modes);
- the threshold grid at both opacities (weak form, agreement recorded) and the
  Dissolve grid at both opacities (exact shown/hidden branches);
- every native mask code × density 0.75 × opacity 0.625 × backdrop alpha
  0/1/32769/65535, all 27 modes: alpha exact, alpha-0 pixels bit-identical to
  the render without the layer, and the mode-0 edit returning `None`;
- an identity kernel (a zeroed `bcs`) over the same grid: the exact no-op under
  Normal, and the ordinary mix under every other continuous mode — the renderer
  skips an identity kernel only under Normal, exactly as the byte
  `composite_adjustment_into` (which skips nothing) behaves;
- a mask smaller than the canvas confining the effect (exact backdrop outside
  its extent);
- an isolated group `[opaque raster, blended adjustment]` at opacity 1, a Pass
  Through container and a clipped blended adjustment each equal to the flat
  stack bit-for-bit, for all 27 modes;
- a hidden blended adjustment and an opacity-0 blended adjustment contributing
  nothing, for all 27 modes;
- strict FFI: each of the 26 non-Normal modes succeeding through
  `rz_precision_doc_edit` and `rz_precision_doc_edit_plan`, the exact no-op
  returning `None`, `rz_precision_doc_info` reporting `kind: "adjustment"` with
  the mode, the adjustment description unchanged, Pass Through refusing with
  the new sentence, out-of-range/unknown modes and indices refusing, and
  `precision_capability::require("layer_blend", "adjustment", 16)` admitted;
- cancellation: a cancelled render and a cancelled edit both drain scratch to
  zero; a 1-byte limit refuses;
- native: v37 chosen only for an adjustment mode (a raster-only Multiply still
  writes v26), one 37 covering a raster Dissolve beside an adjustment Multiply
  with ascending postorder entries, capacity `16 + 8n`, `encoded_bound`
  unchanged, exact downgrade to v34 then to the plain body on release, the
  `36 → 35 → 32 → 29 → 37` wrapper order with tails in
  `LBLD → MOUT → LCLP → LBIF → LSTY` order, reopen equality of info/render/bytes,
  and predecode refusals for every corrupted schema (1–6, 8), count, index,
  kind and mode word, every truncation, a trailing byte and a kind-3 entry
  under each of the six older schemas;
- a 16-bit PSD round trip: Invert at Multiply → `mul ` on the pixel-less
  record → reimport as Multiply with no adjustment-blend loss sentence and the
  projection equal to the source render, the reopened document writing v37.

Updated existing tests: the adjustment-target refusals in
`precision_layer_blending.rs`, `precision_four_blending.rs`,
`precision_threshold_blending.rs`, `precision_remaining_separable.rs` and
`precision_hsl_blending.rs` become successes (their hidden-`LBLD` predecode
refusals at v26/30/31/33/34 are unchanged, because those older schemas still
refuse a kind-3 target); `psd_adjustments.rs` and
`psd_adjustment_descriptors.rs` assert the absence of the retired loss sentence
and that the `Balance`/`Exposure` layers report `blend_mode` 1.
`scripts/live-filter-probe/native.py` decodes v37/schema 7 independently
(admitting kind 3 only at version 37), and
`scripts/verify_phase10d_blend_decoder.py` adds the schema-7 valid/malformed/
frozen block plus a 29 → 37 composition and an unwrapped 37 file.

## Receipts (2026-09-15, implementation run)

- `cargo fmt --check` and `make lint` (`cargo clippy --all-targets -D warnings`):
  clean.
- `cargo test --release --test precision_adjustment_blending`: 14 passed, 0
  failed.
- The listed regression binaries — `precision_layer_blending` 12,
  `precision_four_blending` 5, `precision_threshold_blending` 8,
  `precision_remaining_separable` 6, `precision_hsl_blending` 7,
  `precision_blend_mask_wrappers` 3, `precision_clipping` 11,
  `precision_blend_if` 8, `precision_layer_styles` 9, `psd_adjustments` 8,
  `psd_adjustment_descriptors` 9, `psd_depth16` 9, `psd_constructs` 9,
  `psd_export_corpus` 1 — all pass.
- Full Release suite (`make test`): **1,614 passed, 0 failed, 2 pre-existing
  ignored across 169 result groups**.
- **The slice's own MCP verifier (Run 5 of the closeout, 2026-09-16).**
  `scripts/verify_phase10d_adjustment_blending.py`, receipt
  `~/devel/rasterize-fixtures/phase10-receipts/adjustment-blending/result.json`,
  app SHA-256 `44edd1fde2b2f8741bbd9c735becb0278be4e25fe1b8a81b38ea4c87aa407858`:
  gates A1 (appearance within 1 code, alpha exact, Dissolve exact), A2 (native
  37 schema-7 roundtrip on the kind-3 record), A3 (16-bit PSD carrying `nvrt`
  with the key and no loss) and A4 (atomic `Pass Through` and Blend If
  refusals) all pass. Its blend check accepts either the mode name or the
  numeric blend id, because `get_document` publishes a typed layer's blend as
  an id.
- `python3 scripts/verify_phase10d_blend_decoder.py`: 14 PASS lines, including
  the schema-7 line (all twenty-six modes on kinds 0/1/3, 47
  malformed/atomic-refusal cases and 12 frozen-old-schema kind-3 refusals),
  20 mask29 compositions and 14 unwrapped files.
- `git status --porcelain core/tests/fixtures`: empty — the committed 8-bit PSD
  goldens are byte-identical.
- `make typecheck`: 0 errors. `make app`: BUILD SUCCEEDED.
- MCP drive on an owned Release instance (the shipped `HostApp` harness: an
  ephemeral non-default port, `ANTHROPIC_API_KEY=dummy`, private
  `RZ_ACTIONS_DIR`/`RZ_RECOVERY_DIR`/`RZ_RAW_PRESETS_DIR`/`RZ_FILTER_PRESETS_DIR`,
  `-ApplePersistenceIgnoreState YES -AgentServerEnabled YES`): an authored RGBA16 PNG opened with
  `precision: preserve`, `add_adjustment_layer` invert,
  `set_layer_properties {index: 1, blend_mode: "Multiply"}` → `render` →
  `undo` (Normal) → `redo` (Multiply) → `save_copy` `.rz` → reopen →
  `set_layer_properties blend_mode: "Pass Through"` → `export_psd` →
  `inspect_psd` → `open_document psd_mode: layers`. Measured: the saved
  projection matched `cb · (1 − cb)` with a **maximum code error of 0**; the
  independent decoder read header `RZDC` 37, blend word 37 and `LBLD` schema 7
  `[(1, 1)]` on the kind-3 record; the reopened `.rz` showed `blend_mode` 1 on
  the adjustment; Pass Through returned `isError` with the new limitation
  sentence; `export_psd` reported `bit_depth` 16 with no losses; `inspect_psd`
  reported `rgba16` with empty `verified_losses`; and the PSD reopened as
  `[Normal, Multiply]`. No unexpected `isError` in any call, and the owned
  instance was terminated by the harness.

## Open

- **Knockout (Shallow/Deep) and `blend_interior_as_group` are P7-37**, owned by
  [12E](professional-photo-editor-implementation-plan.md#12-high-depth-parity-and-remaining-workflows).
  They are deliberately NOT part of this slice and never were a 10D migration
  item: 10D's charter is to migrate the EXISTING byte semantics to typed
  evaluation, and knockout had no byte implementation to migrate when this
  slice was written — [12E-3](phase-12e-layer-ops-status.md) has since built
  the BYTE one (design D1–D12 there), so the typed migration this row names
  now has something to carry at both depths (12E-3b); the PSD `knko`/`infx` records SHIP at both depths since 12E-3c, whose D13 also records D5's Background divergence as a deliberate product decision (carryover P7-38, owner 10C-3)
  (`core/src/style.rs` excludes it from the layer-style model;
  `core/src/psd_feature_markers.rs` discards the record's knockout field on
  import with a disclosed loss). Closing it needs a byte-side design — the
  interior/knockout order, the group-boundary rule and a PSD carriage decision
  — before any typed work.
- Blend If and layer styles stay raster + isolated-group only on the typed
  model; an adjustment layer carries neither.
- A typed global light, an f64 SetSat for HSL near-gray conditioning and a
  typed rendered-plane cache remain open, unchanged by this slice.
- The dedicated MCP verifier is TAKEN (Run 5 of the closeout, 2026-09-16):
  `scripts/verify_phase10d_adjustment_blending.py --app <bundle> --output
  ~/devel/rasterize-fixtures/phase10-receipts/adjustment-blending`, receipt
  `~/devel/rasterize-fixtures/phase10-receipts/adjustment-blending/result.json`,
  app SHA-256 `44edd1fde2b2f8741bbd9c735becb0278be4e25fe1b8a81b38ea4c87aa407858`.
  All four gates fixed before the run PASS: A1 (Multiply, Screen, Difference
  and Dissolve on an `invert` adjustment at opacity 1 and 0.625, every opaque
  pixel within 1 code — worst 0.50 — of the independent f64
  `cb + (B(cb, 1 - cb) - cb)·k`, every alpha exact, the two alpha-0 backdrop
  pixels still `(0, 0, 0, 0)`, Dissolve EXACT at 0 codes), A2 (`RZDC` 37 with
  `LBLD` schema 7 `[(1, mode)]` on the kind-3 record, reopen at
  `precision: preserve`, Undo / Redo / `run_action` byte-identical, Normal
  restoring the baseline bytes), A3 (16-bit `export_psd` with `losses == []`,
  an independent decode reading `kind: adjustment`, `adjustment: nvrt` and
  `mul `/`scrn`/`diff`/`diss`, `inspect_psd` with no verified loss, layered
  reopen showing the mode) and A4 (`Pass Through` and a Blend If style on the
  adjustment both refusing atomically). No tolerance was edited after the run.
- The GIMP reopen receipt for an adjustment record's blend key was TAKEN
  2026-09-16 (`~/devel/rasterize-fixtures/phase10-receipts/R8/receipt.json`, app SHA-256 `084b4be2b97b1c84b9749e861c066bec9b72abbefb627c37f7e7c9d7aa8052a6`; re-runnable as
  `scripts/verify_phase10_receipts.py --receipts R8`): a 16-bit export holding
  one `nvrt` record in Multiply decodes as `kind: adjustment`,
  `adjustment: nvrt`, `blend: "mul "`, and GIMP 3.2.6 does NOT drop it — it
  arrives as an empty 32 x 32 raster at (0, 0) named "Inverted" with the
  `multiply` nick, its name/visibility/opacity/composite mode intact and no
  adjustment applied, so its projection differs from the stored composite by up
  to 56779 codes (recorded, never gated; the merged preview agrees at 0 codes).
  The record's geometry is recorded because a pixel-less record has none.
