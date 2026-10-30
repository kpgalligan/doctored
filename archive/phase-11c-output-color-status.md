# Phase 11C — Output color control

Status: **implemented, with one open reference-CMS gate.** Six of the drive's
seven gates pass; **C6's T3 — the soft proof against Adobe ACE — misses its
fixed tolerance and is recorded as a miss rather than absorbed** (see the
RECEIPT and "What stays open"). Every tolerance was fixed before measurement
and none was widened.

This slice also owns carryover row **P7-22**.

The plan text this delivers:

> Add the required broader ICC transforms, rendering intents/black-point
> compensation, soft proofing and gamut warnings. Verify the proof pipeline
> with a reference CMS and physical print samples where relevant. Define and
> preview live-adjustment behavior during document profile conversion: retain
> the existing keep-parameters policy, and qualify an explicit
> appearance-preserving alternative without silently flattening the master or
> changing old evaluation. Define any source-preserving Assign workflow
> explicitly; 8A retains original asset interpretation and requires
> rasterization before assignment.

---

## What ships

### 1. Broader ICC transforms (`core/src/icc_lut.rs`)

v2 `lut8Type` (`mft1`) and `lut16Type` (`mft2`), v4 `lutAtoBType` (`mAB `) and
`lutBtoAType` (`mBA `), for **RGB, Gray and CMYK** device spaces with a **Lab
or XYZ** PCS, parsed into ONE staged `LutPipeline` — curves, a 3×3 matrix with
offsets, a CLUT — so one evaluator serves all four tag types.

**The PCS encodings**, which are the thing it is easiest to get quietly wrong:

| tag | PCS Lab | PCS XYZ |
|---|---|---|
| `mft1` (8-bit) | `L = 100·x`, `a = 255·x − 128` | u1Fixed15 |
| `mft2` (16-bit) | the v2 **legacy** encoding, `L = 100·x·65535/65280` | u1Fixed15 |
| `mAB `/`mBA ` | the v4 encoding, `L = 100·x`, `a = 255·x − 128` | u1Fixed15 |

The legacy 16-bit form puts L\* = 100 at 0xFF00 rather than 0xFFFF — a 0.4 %
scale, a quarter of an 8-bit code at mid grey, and a systematic lightness
drift in any round trip that ignores it. Which of the two applies is the TAG's
question for Lab and the profile HEADER's for XYZ.

**Interpolation.** Tetrahedral for a 3-input CLUT (the PCS→device direction of
every RGB and CMYK profile), multilinear for everything else. The simplex
weights are the three fractions **themselves**, in descending order, applied
to the corner deltas along the simplex path — not their differences. The
difference form is not exact at a cube corner; a 33-point press table hides
that and the two-point synthetic table in `icc_lut_tests` exposes it, which is
how the bug was found during this run.

**Refused, each naming the element** in `rz_icc_describe`'s `refusal` field:
`multiProcessElements` (`mpet`), a profile whose only device→PCS tag is
`D2B0..3` or whose only PCS→device tag is `B2D0..3`, a named-colour list, more
than four device channels, a CLUT whose decoded samples would exceed 64 MiB,
and a truncated or malformed table. Each of those is its own fixture set and
its own reference comparison; half-believing one is how a proof lies.

**A Gray profile is its `kTRC`** — `Y = kTRC(g)`, chromaticity D50 — which is
what a Gray profile IS. A Gray profile with only A2B tables is refused.

**`IccProfile` gains `lut`, `can_decode()` and `can_encode()`.** An RGB
profile with no matrix/TRC tags but readable tables now inspects as the new
`Inspect::RgbLut`, is stored on a document exactly as before, and is a full
conversion source AND destination. `model()` is unchanged and still means
matrix/TRC (see the decision below).

### 2. Intents and black-point compensation (`core/src/icc_intent.rs`)

| intent | table | fallback |
|---|---|---|
| perceptual | `A2B0` / `B2A0` | — (table 0 is the required one) |
| relative colorimetric | `A2B1` / `B2A1` | `A2B0` / `B2A0` |
| saturation | `A2B2` / `B2A2` | `A2B0` / `B2A0` |
| absolute colorimetric | the RELATIVE pair | plus media-white scaling |

Falling back to table 0 rather than refusing is what every shipping CMM does
and what ICC requires of a reader. A matrix/TRC profile has no tables at all:
every intent is the matrix path, and absolute is the matrix path plus
`XYZ' = XYZ · MW_src / MW_dst` componentwise.

**BPC** is ISO 18619's affine map in XYZ, `XYZ' = a·XYZ + b` with
`a = (D50 − BP_dst)/(D50 − BP_src)` and
`b = D50·(BP_dst − BP_src)/(D50 − BP_src)`, so `BP_src ↦ BP_dst` and D50 does
not move. The black points: matrix/TRC and Gray take the profile's own device
zero pushed FORWARD (which is exactly what this crate can do — there is no
curve inverter here); a LUT profile takes the `B2A1 → A2B1` round trip of
Lab(0, 0, 0); a **v4** profile under perceptual takes the spec's fixed
perceptual black Y = 0.0034731, because a v4 perceptual table is written to
land on it and a round trip would measure the table's rounding instead.

**`ColorPipeline`'s fast path is the shipped path.** Two matrix/TRC profiles,
a non-absolute intent and BPC off hold the EXISTING `Transform` and
`PrecisionTransform` and call them. That is not an optimization — it is the
byte-identity guarantee, and
`color_intent_tests::the_default_matrix_pipeline_is_the_shipped_transform_byte_for_byte`
measures it over a 32³ cube plus every neutral and primary ramp rather than
assuming it.

### 3. Soft proofing and the gamut warning (`core/src/soft_proof.rs`)

```
document RGB
  → PCS               (the document's own A2B or matrix, RELATIVE)
  → proof device      (the proof's B2A at setup.intent, BPC per flag)
  → PCS               (the proof's A2B, relative — ABSOLUTE when
                       simulate_paper_white, which is what puts the paper's
                       own cast back in)
  → output RGB        (the document's profile when it can encode, else the
                       built-in sRGB; relative, with BPC UNLESS
                       simulate_ink_black)
```

The last BPC is inverted on purpose: off by default the proof's raised black
is mapped back onto the display's, and `simulate_ink_black` turns the mapping
OFF so the ink's actual black shows as the washed-out grey it is.

The result is TAGGED with the space it is in and handed to the host, which
draws it through CoreGraphics — so the monitor profile is applied by the
window server exactly as for every other image this app draws, and there is
deliberately **no monitor compensation in the core**.

**The gamut rule, stated once.** A pixel is out of gamut when the CIE76 ΔE\*ab
(D50) between its own Lab and the Lab of its proof ROUND TRIP exceeds **5.0**.
The round trip is the proof's `B2A1` then its `A2B1` — **relative colorimetric
always**, no BPC, no media-white scaling, independent of `setup.intent`. That
is Little-CMS's `_cmsCreateGamutCheckPipeline` rule and it is the only rule
that answers the question being asked ("can this medium hold this colour at
all"): running the round trip at the setup's intent instead flags perfectly
printable colours under perceptual, whose whole job is to move in-gamut
colours. The overlay is `gamut_color` written opaque over the proofed pixel;
the default is 50 % grey. Alpha is copied in every case.

**View state, and only view state.** `ProofDisplay` lives on
`EditorViewController`, never in `EditingState`, never in a `.rz`, never an
undo step, never a dirty-flag change and never an Action step — the same
exemption the rulers, the grid and the snap toggles take, for the same four
reasons (`app/CLAUDE.md`'s "View preferences" rule). The proof reaches the
canvas through a new display-only `ImageCanvasView.proofImage` slot drawn as
`previewImage ?? proofImage ?? image`; it is never assigned to `canvas.image`,
whose `didSet` clears the overlay, resets the selection and the Quick Mask and
cancels an in-progress selection stroke.

### 4. The conversion policy, and P7-22 (`core/src/doc_color_policy.rs`)

`ConversionOptions` is `{intent, black_point_compensation, adjustments}`,
every field optional, every default today's behaviour, with `is_default()` the
bit the byte-identity claim keys on.

**Keep parameters is still the default and is unchanged.** An adjustment's
numbers are PARAMETERS, so a conversion leaves them exactly as authored; each
layer keeps its appearance and a stack computed FROM the numbers does not, and
the host says so.

**Preserve appearance** is the explicit alternative, decided per kind:

| kind | action | why |
|---|---|---|
| `exposure` / `white_balance` at `evaluation: "document_v2"` | **kept** | both are LIGHT-LINEAR operations evaluated through the document's own profile, so the same parameters produce the same appearance in any space this build can decode. Nothing to retarget and nothing to bake. |
| every other op, and `legacy_srgb` evaluation | **baked** | the op is defined on the ENCODED numbers of whatever space it runs in (a curve point, a levels endpoint, a saturation amount), so there is no parameter that would mean the same thing afterwards. |

A baked entry is replaced IN PLACE by one raster layer named
`"<name> (baked)"` holding the projection of everything up to and including it
(`flattened_cut` / `render_cut_with_context` with every entry above it
excluded) at Normal, 100 %, no mask, visible. **The layers below it stay
exactly where they are** — the master's structure survives and nothing is
flattened. That substitution is an identity only over an opaque backdrop,
which is why a cut projection with any alpha below the maximum REFUSES,
naming the layer. Two more refusals, both naming the layer: a to-be-baked
adjustment inside a group, and a clipped one. A hidden adjustment is left
alone. Bakes run bottom-up on the running document.

Old documents keep their evaluation: nothing about `legacy_srgb` or
`document_v2` moved, and the default request is byte-identical.

**Assign, and the 8A rule.** `assign_profile` now REFUSES with a sentence
naming the count while any layer holds a retained source or a live-filter
stack — reinterpreting a fallback that is a render of an original asset in a
specific profile is exactly the silent reinterpretation Phase 8A forbids. The
disclosed way through is `assign_profile_rasterizing` (the sheet's "Rasterize
N image sources, then assign" checkbox, `rasterize_sources: true` over MCP):
every bound layer is rasterized through the shipped `SourceEdit::Rasterize`
and the profile is assigned, as ONE result the host commits as ONE undo step.

### 5. Export

`intent`, `black_point_compensation` and `profile: "file"` + `profile_path`
join the ONE export request; the host additionally accepts
`profile: "installed"` + `profile_name` and resolves it to a path before the
core sees it, so a recorded step and the request carry a value rather than a
name that could resolve differently on another machine. The convert stage now
routes the FLAT image through `ColorPipeline`; at the defaults that pipeline
holds the very `Transform` the stage used before, and `export_pipeline.rs`'s
equalities still pass unchanged. `ExportResult.info_json` gains `profile_name`,
`intent` and `black_point_compensation`.

### 6. Surfaces

- **UI:** Image ▸ Convert to Profile… gains an intent popup, a BPC checkbox
  and an "Adjustment layers:" popup with the preflight report under it;
  Image ▸ Assign Profile… gains the rasterize checkbox and the core's refusal
  sentence; View ▸ Proof Setup… / Proof Colors ⌘Y / Gamut Warning ⇧⌘Y are new
  (⌘Y and ⇧⌘Y were free); File ▸ Export… gains the
  intent popup, the BPC box and the installed-profile rows.
- **MCP:** `convert_profile` takes `intent`, `black_point_compensation`,
  `adjustments` and `profile: "installed"`; `assign_profile` takes
  `rasterize_sources`; `get_color_profile` reports `can_decode`; and five new
  tools — `get_proof_setup`, `set_proof_setup`, `render_proof`,
  `describe_profile`, `list_installed_profiles` — all five `nonRecordable`.

---

## Recorded decisions

Owner-level choices were delegated for this run (`request.md`). Each is stated
here with its reasoning.

1. **`rz_doc_profile_is_convertible` keeps its matrix/TRC meaning.** Widening
   it to "can decode" would silently admit a LUT-tagged document to
   `document_v2` Exposure and White Balance, which need a matrix/TRC model
   (`adjust_calibrated.rs`). A new `rz_doc_profile_can_decode` /
   `profileCanDecode` carries the wider question, and exactly two call sites
   move to it: the Convert sheet's source gate and `convert_profile`'s.
   `get_color_profile` reports both.
2. **The bounded tag set** is v2 `mft1`/`mft2` and v4 `mAB `/`mBA `, in RGB,
   Gray (kTRC) and CMYK, with a Lab or XYZ PCS. `mpet`, `D2Bx`/`B2Dx`-only,
   named-colour and >4-channel profiles are refused with the element named.
   Each excluded family needs its own fixtures and its own reference
   comparison before it can be believed.
3. **The intent fallback is table 0**, which is what ICC requires of a reader
   and what every shipping CMM does; refusing instead would make a
   single-table profile unusable for the two intents it plainly means.
4. **The gamut round trip is fixed at relative colorimetric**, independent of
   the setup's intent (the lcms rule). The alternative flags in-gamut colours
   under perceptual, which is a warning that lies.
5. **The per-kind retargeting rule** keeps only the two light-linear
   `document_v2` ops and bakes the rest. A general retarget would need each op
   to have a well-defined meaning in the destination's encoding, which a curve
   point and a saturation amount do not.
6. **Assign with retained sources refuses by name and offers rasterization**
   rather than silently detaching, which is what the code did before this
   phase. That is a behaviour change, and it is the one the plan asked for.
7. **A sheet's live preview wins over the proof and is unproofed.** A preview
   is a picture of a pending edit; proofing it would re-run the whole chain on
   every slider tick for no decision the user is making at that moment.
8. **Intents, BPC and LUT destinations are refused when a retained source or a
   live filter is present.** `retained_source_render` is matrix/TRC-only and
   has its own 9A cache; the preflight refuses that combination with one
   sentence instead of letting it fail half-converted.

---

## Reference-CMS results

Every tolerance below was fixed in `scripts/verify_phase11c_color.py` BEFORE
anything was measured and is recorded in the receipt. None was widened.

| id | what | tolerance |
|---|---|---|
| T1 | default-option conversions are byte-identical | exact (Rust) |
| T2 | matrix→matrix vs ACE per intent | max ≤ 2 codes, mean ≤ 0.5 |
| T3 | sRGB → {FOGRA39, SWOP} → sRGB vs ACE | mean ΔE76 ≤ 2.0, p95 ≤ 4.0, max ≤ 8.0 |
| T4 | the same through `sips -M` (ColorSync) | recorded; gate mean ≤ 3.0 |
| T5 | preserve-appearance composite vs the matched pre-conversion one | mean ≤ 0.5, max ≤ 2 (8-bit scale) |
| T6 | the gamut mask vs an independent per-pixel ΔE check | exact equality |

**T1 and T6 are Rust tests and both pass.**
`conversion_policy_tests::the_default_options_conversion_is_byte_identical_to_the_shipped_one`
compares every layer's pixels, at three spellings of the defaults, against the
shipped `icc_transform::Transform` applied inside the test — an oracle outside
the code under test, since `rz_doc_convert_to_profile` now delegates to
`rz_doc_convert_to_profile_with`. It still compares the two entry points
against each other, as exactly that delegation check, and
`color_intent_tests::the_default_matrix_pipeline_is_the_shipped_transform_byte_for_byte`
measures the same property densely over the cube.
`soft_proof_tests::the_gamut_mask_equals_an_independent_delta_e_check_under_both_intents`
compares the mask against a per-pixel oracle written in the test, over 125
colours including the three the rule was argued over, under all four intents,
and confirms sRGB (0,255,0) is flagged while (128,128,128) and (180,120,90)
are not — including under `perceptual`.

**T2–T5 are the MCP drive's.** See `RECEIPT` below for the run this slice
records; a failed gate is recorded rather than repaired.

### RECEIPT — 2026-09-17

`~/devel/rasterize-fixtures/phase11-receipts/11c/receipt.json`.
App SHA-256 `b32646f0520ae33a480cc3c6c076c8807678fd8fb3a8f5c86f5b5e2c100521bb`;
drive SHA-256 `868a1493ced182e50603ba11edd418a09a34011bf1a2c20dc6ec522619f961bc`.
Photoshop 2025 **26.10.0**, SHA-256
`a502d3b62279299a77f85d69a3d8f3d5379e0fb406d40d746a00ee49031bfa04`, was NOT
running before the take, was launched by it and was quit by it
(`still_running: false`). Every profile the run names was installed.

Six gates of seven pass. **C6's T3 misses, and the miss is recorded here
rather than absorbed.**

| gate | result |
|---|---|
| C1 intents and BPC against a reference CMS | **pass** |
| C2 preserve appearance (T5) | **pass** |
| C3 assign refuses, then rasterizes | **pass** |
| C4 proofing writes nothing | **pass** |
| C5 a correctly tagged print file | **pass** |
| C6 the proof and the gamut mask | **FAIL (T3; the mask half passes)** |
| C7 the catalog and the two profile gates | **pass** |

**T2 — the forward matrix→matrix transform against Adobe ACE**, measured in
the destination's own 8-bit encoding over a 256 × 67 plate (a hard-edged base
plus three 256-step ramps), 51 456 samples per case:

| case | mean \|Δ\| | max \|Δ\| | tolerance | |
|---|---|---|---|---|
| sRGB → Adobe RGB, perceptual | 0.0215 | 1 | ≤ 0.5 / ≤ 2 | pass |
| sRGB → Adobe RGB, relative | 0.0215 | 1 | ≤ 0.5 / ≤ 2 | pass |
| sRGB → Adobe RGB, saturation | 0.0215 | 1 | ≤ 0.5 / ≤ 2 | pass |
| sRGB → ProPhoto, relative | 0.0246 | 2 | ≤ 0.5 / ≤ 2 | pass |

The measurement is the FORWARD leg. An earlier take measured a round trip
(convert out, convert back) and read max 16 codes; splitting it showed the
forward legs agree within one code everywhere and the whole disagreement was
the RETURN leg, where the 8-bit intermediate is quantized a second time and
each side returns through a different sRGB profile (Apple's parametric curve
against Photoshop's 1024-entry table). The drive now measures what the
tolerance is about. The tolerance itself was not touched.

**T4 — the same plate through ColorSync (`sips --matchTo`), as a round trip**,
17 152 pixels, ΔE76 in sRGB:

| case | mean | p95 | max | tolerance | |
|---|---|---|---|---|---|
| sRGB → Adobe RGB → sRGB | 0.42 | 1.20 | 3.81 | mean ≤ 3.0 | pass |
| sRGB → Display P3 → sRGB | 0.51 | 1.35 | 4.20 | mean ≤ 3.0 | pass |

`sips` exposes no intent switch, so T4 is a second CMS at ColorSync's own
default for the pair and is never a per-intent claim; ACE is the per-intent
authority.

**T5 — preserve appearance**, a 16-bit document carrying a `document_v2`
exposure (kept), a BCS and a Levels (both baked), converted to Adobe RGB and
compared against the PRE-conversion composite matched into Adobe RGB by
ColorSync, 49 152 samples:

| | mean | p95 | max | tolerance | |
|---|---|---|---|---|---|
| preserve appearance | **0.126** | 1 | **1** | mean ≤ 0.5, max ≤ 2 | **pass** |
| keep parameters (recorded, no gate) | 5.19 | 29 | **65** | — | — |

That pair is P7-22's whole argument in two rows: the default policy moves
this composite by up to 65 codes, correctly, and the explicit alternative
holds it to one.

**T3 — the soft proof against ACE's round trip — MISSES.** Tolerance mean
ΔE76 ≤ 2.0, p95 ≤ 4.0, max ≤ 8.0. Measured with `simulate_ink_black` on, so
the proof's final leg is the same relative / no-BPC return the Photoshop job
used and the comparison is the transform rather than each side's screen black
mapping. 17 152 pixels, of which 8 249 (48 %) are outside FOGRA39's gamut:

| case | mean | p95 | max | in-gamut mean/p95/max | out-of-gamut mean/p95/max |
|---|---|---|---|---|---|
| FOGRA39 perceptual + BPC | 1.87 | 3.97 | 11.03 | 1.88 / 3.65 / 5.79 | 1.86 / 4.43 / 11.03 |
| FOGRA39 relative | 3.52 | 8.44 | 14.31 | 4.38 / 8.86 / 12.67 | 2.58 / 7.36 / 14.31 |
| FOGRA39 relative + BPC | 4.15 | 8.96 | 15.66 | 4.35 / 8.86 / 13.74 | 3.95 / 9.10 / 15.66 |
| FOGRA39 absolute | 5.59 | 11.95 | 25.73 | 5.66 / 11.48 / 17.04 | 5.51 / 12.79 / 25.73 |

What this is and is not:

* It is NOT a gross error. The per-component means over the whole plate are
  ΔL 0.24, Δa 0.88, Δb 1.58 for the relative case — scatter, not a shift — and
  spot colours land within a few codes of ACE's (`src (120, 10, 198)` →
  ours `(97, 63, 143)`, ACE `(97, 63, 143)`).
* Splitting by gamut does not rescue it: the in-gamut half is no better than
  the whole, so this is not the "two CMMs clip differently" story it would be
  convenient to tell.
* The perceptual case is the CLOSEST (mean 1.87, inside the mean tolerance,
  p95 inside it too; only the max is over), and the colorimetric cases are the
  furthest. That ordering is the opposite of what a plain table-evaluation bug
  would produce and points at what ACE does AROUND the table — its own black
  generation and gamut-compression smoothing on a 4-channel B2A — rather than
  at the table read itself. The LUT engine's own arithmetic is independently
  checked against synthetic tables whose answer is known exactly
  (`icc_lut_tests`), and FOGRA39's round trip lands where colour science says
  it must (paper white L\* in 95…100.5 and near neutral, rich black below
  L\* 20, mid grey in gamut, sRGB green far outside).
* One genuine defect WAS found while investigating and is fixed: absolute
  colorimetric did not apply its media-white scaling on the document → proof
  leg, so "absolute" only picked relative's table.
  `soft_proof_tests::absolute_colorimetric_puts_the_source_white_back` now
  holds that shut. The absolute case's numbers above are post-fix.

**The tolerance stands as fixed and the gate stands as failed.** Closing it
needs the ACE difference identified rather than accommodated. Acceptance: a
per-stage comparison against a THIRD CMS (Little-CMS, built from source as an
owned fixture) on the same plate, isolating the B2A read from whatever ACE
applies around it, and either a named defect fixed here or a written finding
that ACE's proof differs from the profile's own tables by that much — at which
point the tolerance is re-derived from the finding rather than from optimism.
Until then soft proofing ships with this receipt attached: the gamut warning is
exact (below), the chain is right in structure, and the colorimetric proof is
within about 4 ΔE of Adobe's on a coated press profile.

**The mask half of C6 passes exactly.** Under both `relative` and
`perceptual`, `render_proof`'s reported count, its mask count and the number
of pixels the warning overlay actually covered are all 8 249 of 17 152, and
the two intents report the identical count — which is the objection the
relative-colorimetric-always rule was written to answer.

**C3, C4, C5 and C7, in one line each.** The assign refusal reads "Assigning a
profile requires rasterizing 1 image source first. Pass rasterize_sources:
true to rasterize them and assign in one step."; the LUT/intent conversion
refusal reads "converting with these settings requires rasterizing 1 image
source first"; `get_document` is byte-identical across both and the sampled
codes survive the rasterizing assign. Every proof toggle leaves
`get_document`, `get_color_profile`, the selection, the dirty flag and an
export's sha256 identical. The TIFF exported with `profile: "installed"`,
`profile_name: "Adobe RGB (1998)"`, `intent: "perceptual"` carries tag 34675
whose sha256 is `304f569a…` — byte-identical to the installed file — and whose
`desc` reads "Adobe RGB (1998)". The catalog parses at 206 tools with all five
new ones present, and a LUT-tagged document reports
`convertible: false, can_decode: true`.

---

## Receipts

- Rust: `core/tests/icc_lut_tests.rs`, `core/tests/color_intent_tests.rs`,
  `core/tests/soft_proof_tests.rs`, `core/tests/conversion_policy_tests.rs`,
  plus the unchanged `core/tests/export_pipeline.rs` equalities and the
  `psd_10c` goldens.
- Drive: `scripts/verify_phase11c_color.py`, receipts under
  `~/devel/rasterize-fixtures/phase11-receipts/11c/`.
- Photoshop: `scripts/photoshop-probe/` job `convert`, receipts under
  `~/devel/rasterize-fixtures/photoshop/color/`.

---

## The physical print-sample runbook (deferrable; gates nothing)

The plan asks for physical print samples "where relevant". They need the owner in
person and a calibrated printer, so they are written here as a self-contained
runbook, recorded as an OPEN receipt, and they gate nothing. Nothing in this
slice waits on them and they are not the next step.

1. Open `~/devel/rasterize-fixtures/phase11-receipts/11c/fixtures/plate.png`
   in Rasterize.
2. View ▸ Proof Setup…, choose the profile of the paper actually loaded in the
   printer (Adobe's Recommended folder, or the paper maker's own), relative
   colorimetric with black point compensation, Proof Colors on.
3. Note, in writing, which patches of the plate the Gamut Warning covers.
4. File ▸ Export…, TIFF, `profile: file` naming that same paper profile,
   `intent: relative`, black point compensation ON, 16-bit.
5. Print that TIFF from Preview with colour management left to the PRINTER
   DRIVER'S OWN no-management option, so the file's own profile is the only
   transform.
6. Under a D50 viewing booth, compare the print with the proofed screen. Two
   things are being checked and they are separate: (a) do the patches the
   Gamut Warning covered come back visibly different on paper from what the
   screen showed unproofed, and (b) does the proofed screen match the print
   more closely than the unproofed one.
7. Record the answers, the paper, the printer, the driver settings and the
   date in this document under a new "Print samples" heading. A mismatch is a
   finding, not a failure to repair.

---

## What stays open

- **T3, the soft proof against Adobe ACE, MISSES its fixed tolerance** (the
  RECEIPT above). Mean ΔE76 1.87 (perceptual + BPC) to 5.59 (absolute) against
  a tolerance of 2.0, with p95 and max over as well; the gamut mask half of
  the same gate passes exactly. Acceptance: a per-stage comparison against a
  THIRD CMS (Little-CMS built from source as an owned fixture) on the same
  plate, isolating the B2A read from whatever ACE applies around it, and
  either a named defect fixed or a written finding that ACE's proof differs
  from the profile's own tables by that much — at which point the tolerance is
  re-derived from the finding rather than from optimism. The tolerance is NOT
  widened here.
- **Writing CMYK, Gray, spot or separation FILES, CMYK document mode and
  PDF/X.** The plan's own boundary makes these a separate prepress product
  decision. CMYK profiles are read here for PROOFING and gamut warnings only.
  Gray as a convert or export DESTINATION is refused with a sentence;
  acceptance for that is a grayscale document mode or a gray-encoded output
  path (11D's grayscale16 bridge).
- **`mpet`/multiProcessElements, `D2Bx`/`B2Dx`, named-colour and >4-channel
  profiles.** Refused with the element named. Acceptance: an owned fixture set
  and a reference-CMS comparison per element.
- **Intents, BPC and LUT destinations for retained-source and live-filter
  layers.** Refused by the preflight. Acceptance: `retained_source_render`
  takes `ConversionOptions` and a `ColorPipeline`, with the 9A cache keyed on
  them.
- **`document_v2` Exposure and White Balance on a LUT-tagged document.** The
  gate stays matrix/TRC. Acceptance: `adjust_calibrated` decodes through
  `ColorPipeline`.
- **Proofed sheet live previews, monitor-profile compensation in the core,
  proof presets on disk, per-layer proof**, and the RAW wider-gamut output
  choices 8D shares with this slice.
- **The physical print samples**, as the runbook above.
