# Phase 8G — camera controls and local RAW development (ADR)

- **Status:** Accepted — open items remain ([#11](https://github.com/kpgalligan/doctored/issues/11))
- **Phase / dates:** 8G, 2026-09-11 (completes Phase 8)
- **Originals:** [archive/phase-8g-raw-enhancements-status.md](../archive/phase-8g-raw-enhancements-status.md) — the phase's full history, to 2026-09-26; [archive/2026-10/phase-8g-raw-enhancements-status.md](../archive/2026-10/phase-8g-raw-enhancements-status.md) — this doc as of 2026-10-07, with every measurement table and issue record; [archive/phase-8g-raw-enhancements-shipped.md](../archive/phase-8g-raw-enhancements-shipped.md) — the shipped companion, condensed into this ADR on 2026-10-30; [archive/2026-10-30/phase-8g-raw-enhancements-status.md](../archive/2026-10-30/phase-8g-raw-enhancements-status.md) — this doc as of 2026-10-30, before the companion fold
- **Key commits:** `e386448` (follows 8F `9b3f65c`)

## Context
8C/8D ([phase-8-retained-source-status.md](phase-8-retained-source-status.md)) retained RAW originals with a
global recipe only. Photographers need local exposure/white-balance adjustments that re-develop the *camera
data* (recovering highlight detail a darkened byte preview cannot), plus camera-relative controls — without
introducing a linear/HDR document mode, and with persisted masks that survive migration, presets and sync.

## Decisions
- **Frozen evaluation order:** original camera bytes → pinned global camera/vendor lens development for the base
  and each enabled local variant → chosen sRGB/Display P3 integer u16 output → ordered native coverage blend (float
  accumulator) → calibrated manual vignette → u16 source quantization → explicit optics → develop Presence (Dehaze,
  then Texture and Clarity) → develop grain → existing retained layer affine/profile/fallback → layer mask/retouch.
  Variants share decoder, orientation, dimensions and output ICC. *Why:* each local develops the original, never a
  byte preview; single quantization keeps the blend exact.
- **Exact bypasses:** zero coverage, disabled/identity locals and neutral vignette are bit-exact no-ops.
- **Locals:** at most sixteen, ordered, in top-left, oriented, full-source pixel coordinates *before* layer
  geometry. Coverage is native u16 from radial/linear primitives and ordered brush add/subtract, with inversion,
  density and feather; never derived from a byte preview. Each local changes exposure offset, WB intent and tone
  (Local tone path). Hard radial fields use point-center sampling. Code: `raw_local_*.rs`, `RawLocal*.swift`,
  `RawDevelopWindowController+Locals.swift`.
- **Range masks:** a local may carry a luminance band, a colour distance, or both; the range multiplies the shape's
  coverage per pixel, after density and feather, unblurred, and a pixel it excludes keeps the base sample bit-exact.
  Mask op `full` covers the frame so a range can work alone. The range reads the *base* development (before any
  local, the vignette, optics and grain) as CIE L\*a\*b\* against D50 through the base's own profile, L\* clamped
  to 0…100. *Why:* a local's exposure or white balance cannot move its own selection, the same bounds select the
  same tones in every output space, and a falloff width is perceptually even. A colour given as a source point
  resolves to the mean Lab of its 5×5 base pixels; the recipe holds only the Lab. No plane, DRDC or RZLM change;
  the enhancement `version` is 2 when any local has a range, so an older build keeps the saved rendering. Code:
  `raw_local_range.rs`, `RawLocalRange.swift`, `RawLocalRangeControls.swift`; drive `scripts/verify_v1_raw_range.py`.
- **Generated masks:** a local's coverage may be Vision's Select Subject or Select People mask, or the sky model's
  matte (mask op `subject` / `people` / `sky`; `SkySelection.coverage`, shared with the Develop Filter), read from
  the *base* development like a range. Vision's byte coverage is stored as u16 `v·257` in an ordinary `RZLM` v1
  blob; the node's `generated` key records the kind, and a build that predates it refuses the recipe and keeps the
  saved rendering. **Recompute is on request only:** stored coverage never changes by itself (global edit, preset,
  Sync Settings, Redevelop RAW, migration, reopen); the `mask` op with a generated kind replaces the whole plane
  (Brush and Invert since are not replayed). Inside one open Develop session a generating operation not yet applied
  reads that session's base; a refusal drops the operation only on its first run and never deletes previewed work.
  Nothing found refuses with the selection commands' own sentences; with
  no sky model New Sky creates nothing and offers Settings ▸ Models, and MCP refuses with
  `ModelGate.notInstalled("sky")`. The Vision stage runs inside the existing local charge. Code:
  `RawLocalGenerated.swift`, `dr_raw_local_mask_from_coverage`; drive `scripts/verify_v1_raw_subject.py`.
- **32-bit float (linear) development:** a third master encoding, `raw.output_encoding: linear_float` (Develop ▸
  Document depth "32-bit float (linear)"; Layer ▸ Redevelop RAW… ▸ Development in a float document). The one decode
  seam renders `.RGBAf` into the output space's extended linear twin (`CGColorSpaceCreateExtendedLinearized`) with
  `extendedDynamicRangeAmount` 1 (`scripts/raw-probe/float_edr.py`); the f32 master is tagged with the space's
  built-in profile (a float document holds linear light in its profile's primaries). An alpha within half a 16-bit
  code of 1 is stored as exactly 1, the alphas the 16-bit development stores as 65535, and any other refuses the
  development (`RetainedRawDecoder.normalizeOpaqueAlpha`; the decoder renders a Nikon Z 6 NEF with 61 % of its
  alphas a few ulps off 1; `scripts/verify_raw_float_cameras.py`). It exists only in a 32-bit
  float document (typed DRDC word 78, `float_native`) and never narrows out of one (`convert_precision` preserve
  and a layers paste refuse it by name until it is redeveloped `sdr16`). Recipe: a policy string
  (`<space>-rgba32f-linear-v1`; `-v2` when the tone curve is above 0) beside its `extendedDynamicRange`. The
  decoder controls act as at 16 bits, the tone curve and Shadows too (`boostAmount`/`boostShadowAmount` under the
  extended range, values above white kept; `scripts/raw-probe/float_tone.py`); a float development starts at tone
  curve 0; local tone map 0 (the float local tone map is open:
  [#4](https://github.com/kpgalligan/doctored/issues/4)); manual vignette a linear gain; lens geometry resampled in
  linear float; grain on the display encoding (Doctored's rule, not measured against Adobe Camera Raw: the one Add
  Grain kernel on E, continued as a logarithm above white, so a channel in 0…1 gets the 16-bit grain of its encoded
  value and a pixel at or above white keeps its value; `tests/raw_float_grain.rs`); locals blend f32 variants in f32;
  a range reads Lab of the f32 base (above white is L\* 100). It is linear in exposure with no highlight shoulder:
  tone mapping, not clipping, is how it is meant to be viewed. Inspection shows it clipped to SDR, except on a screen
  with headroom, where Develop's Fit and 100 % views draw rule X's binary16 image (`develop_preview_extended`; MCP
  `hdr_preview`); its histogram reads the f32 master in stops. Admission, fixed before measuring: decode
  `RawSourceWork.scratch` + 16·px; local `measuredScratch` + (8·(k + 2) + 8 + 8e)·px; `replaceScratch`
  16·(k + 2)·px; a Develop session + 24·px; every charge ≥ 1.25 × the measured peak (records beside the constants).
  Drives `scripts/verify_v1_raw_float.py`, `scripts/verify_v1x_raw_hdr.py`.
- **Develop Filter:** Filters ▸ Develop Filter… (⇧⌘A, MCP `apply_filter` `develop_filter`) runs Develop's global
  controls on any layer as one destructive `SpatialKernel` arm composed only of existing ops, in a fixed order:
  white balance → exposure → contrast → highlights/shadows → whites/blacks → Presence (Dehaze, Texture, Clarity) →
  vibrance/saturation. A non-RAW layer has no as-shot white, so Temperature is relative: the `white_balance`
  illuminant `10⁶/(10⁶/6504 − t)` K (one mired a unit, + warms), Tint passed as given. Not re-editable (a second
  run starts from the pixels); 8/16/32-bit; destructive by design (smart objects were removed 2026-10-10). CMYK
  refuses by name. Left out (CIRAWFilter camera-space, no core equivalent): tone curve, shadow boost, local contrast,
  sharpness, detail, noise reduction, profile corrections, highlight recovery, moiré, baseline compensation; output
  space and decoder are RAW-only; vignette and grain are Lens Correction and Add Grain. Its own preset kind
  (`DR_DEVELOP_FILTER_PRESETS_DIR`; the folder was `Camera Raw Filter Presets` until the 2026-10-29 rename, moved
  once at launch by `DataMigration.moveRenamedFolder`) reads Develop presets taking only `exposure`; Actions,
  presets and shortcut overrides saved under the earlier names still load (`LegacyNames.swift`). Code:
  `sampling_develop_filter.rs`, `DevelopFilterSheet.swift`; tests `filter_develop_filter.rs`; drives
  `scripts/verify_v1_develop_filter.py`, `scripts/develop-filter-admission/measure.py`.
- **Develop Filter masks:** the filter's `masks` (≤ 16, in order after the globals): radial, linear, brush, full,
  range, and subject, people and sky shapes the host makes on a ≤ 1024 px analysis copy and sends as `map` shapes;
  each carries the twelve local controls (no Vibrance) as a `DevelopFilter` plane over the picture the earlier
  masks left, blended by RAW-local coverage (`Command`, `EffectiveMask`) times a `LocalRange` read from the layer's
  own samples through the document's profile. No new pixel math. 8 and 16 bits; a 32-bit float layer and a mask,
  plane or channel target refuse by name. A recorded step keeps generated shapes as asked, so a replay reruns the
  model. Local Sharpness (−100…100) and Noise Reduction (0…100) run last over a mask's value as Reduce Noise, then
  Smart Sharpen or Gaussian Blur at radii fixed in pixels (`local_detail.rs`, shared with Develop), so the sheet's
  proxy preview leaves them out. Code: `develop_filter_masks.rs`, `develop_filter_masks_request.rs`,
  `DevelopFilterMasks.swift`, `DevelopFilterMaskPanel.swift`, `AgentServer+DevelopFilterMasks.swift`; tests
  `filter_develop_filter_masks.rs`.
- **Develop Presence:** Texture, Clarity and Dehaze (−100…100, 0 = off) run the Filter ▸ Presence kernel
  (`selected_edit` op `presence`, 16-bit, a controlled job) on the finished development, after explicit optics and
  before grain; locals get none. *Why:* the kernels read the developed, display-encoded u16 tones; after locals,
  vignette and optics the bands and airlight are the published frame's; before grain they never amplify synthetic
  grain. The radii scale with the long side (`presence_radii`, pinned by `tests/filter_presence_scale.rs`), and both
  Develop previews run the full-size pipeline, so the preview is the Apply result. Persistence: an optional recipe
  `presence`, absent when all three are 0 (an older build refuses the unknown key and keeps the saved rendering).
  Presets are schema 4. The 32-bit float development refuses nonzero values (`RawPresenceSettings.floatRefusal` in
  `RawDevelopmentRecipe.updating`) and Develop disables the rows there. Code: `RawPresence.swift`; drive
  `scripts/verify_v1_raw_presence.py`.
- **Local tone path:** a local may carry a `tone` (Exposure −4…4 stops, Contrast, Highlights, Shadows, Whites,
  Blacks, Saturation, Clarity, Texture), applied in the core to the 16-bit development through the existing kernels
  (precision_v1 `exposure` under `document_v2`, `bcs` modern, `shadows_highlights` at the Presence coarse radius,
  one `levels`, `vibrance`, then the Presence plane) and blended through the coverage × range. A local with no
  camera change develops no variant: it reads the development as the earlier locals left it, so tone locals
  compound. Only a local that changes white balance or camera exposure develops a variant: at most four of those,
  sixteen locals in all, and the tone of such a local adjusts its variant. Recipe enhancement `version` 3 marks any
  tone or more than four locals; a version-1/2 recipe runs the old code, so an old `.rz` reopens bit-exact. From
  version 3 a local with no camera change and no tone is a no-op (before, it blended the base back and undid earlier
  locals inside its coverage); this appears only after a user edit moves the recipe to version 3
  (`core/tests/raw_local_tone.rs` pins both contracts).
  A tone on the 32-bit float development refuses by name (`RawLocalTone.floatRefusal`): the tone kernels are
  display-referred. Dehaze is not a local control (it needs a global haze estimate). The combined RAW + attachment
  resource cap is 1 GiB (RAW originals alone keep 512 MiB), refused up front by `RawLocalWork.request`; a document
  whose attachment holds more than four masks or passes 512 MiB writes DRDC word 79 (`raw_locals_native.rs`). Local
  Sharpness (−100…100) and Noise Reduction (0…100) run last through `local_detail.rs`, stored only when not 0, so an
  older build refuses a recipe that names them. Code: `raw_local_tone*.rs`, `raw_local_limits.rs`,
  `RawLocalTone*.swift`; drive `scripts/verify_v1_raw_local_tone.py`.
- **Global controls exposed only where a capability matrix shows effect:** camera-relative baseline compensation,
  moiré reduction (where supported), manual profile-aware vignette. Unsupported controls (e.g. vendor lens
  correction and local tone map on the D70s) are not shown.
- **Recipe 3** stores every effective control plus captured effective variants; recipes 1/2 and strict binary64
  persistence keep their meaning. Unknown recipe or mask versions keep cached native rendering and refuse
  semantic edits.
- **Syncing locals:** Synchronize RAW Settings' Masking (`sync_raw_settings.locals`, replayed by
  `apply_raw_settings.locals`) REPLACES each target's locals with the source's, in order (≤16), ids, names and
  controls kept. Normalisation is per axis in each RAW's canonical frame (EXIF orientation applied, before explicit
  lens geometry). Drawn coverage is resampled by `dr_raw_local_mask_resize` (edge-normalised bilinear). A generated
  local is detected again on the target's base, after the synced globals (Brush or Invert refinements are not
  carried); one that finds nothing or lacks its model is skipped and named. Feather is scaled by the geometric mean
  of the two axis ratios (clamped to 256). A range is copied as its stored Lab. A replace that changes nothing
  publishes nothing. A recorded step stores its drawn masks as `<sha256>.drmask` in `RawLocalMaskStore`
  (`DR_RAW_LOCAL_MASKS_DIR`). Code: `RawLocalSync.swift`; drive `scripts/verify_v1_raw_locals_sync.py`.
- **Mask storage:** versioned `RZLM` blobs in the 8F native 21/22 `RBIN` pool under `raw_local_masks`; mask
  pixels never enter the capped recipe JSON. ≤64 MP per mask; 256 MiB per resource; originals + masks within
  1 GiB together. Unchanged masks/originals share storage across revisions; every recognized header, length and
  reference validates before pixel decode.
- **Transfer rules:** global presets/sync transfer camera-relative compensation, supported moiré and vignette
  while preserving each target's local IDs and masks; Sync's Masking replaces the target's locals with the source's;
  explicit local copy transfers exposure, WB intent and tone only; never another camera's hidden recipe/decoder.
- **Migration** at unchanged geometry preserves planes exactly and reviews the complete new development
  (12F-4 later added reviewed EXIF/crop/pad and filtered-affine remap of local planes).
- **Plumbing:** UI/MCP/Actions share bounded captured operations and one prepared publication. Cheap attachment
  kinds keep RAW masks out of host-frame summaries/menus/recording even when metadata reads are denied.
- **Bounds:** ≤16 locals (≤4 camera variants), 5 cached camera developments / 256 MiB, 256 pending operations, 4,096
  aggregate brush dabs, 256 M evaluated brush-box pixels per native op, 256 KiB operation JSON. Platform
  capture/render calls are noncancellable; native stages cooperate. The 64 MP cap is not a four-local 64 MP
  qualification.

## Rejected alternatives
- Darkening/brightening a developed byte or u16 preview for locals — cannot reveal clipped-but-recoverable camera detail.
- Mask samples inside recipe JSON — capped, slow, and duplicates across revisions.
- A linear or HDR document mode for local blending — out of scope for the 16-bit development, which keeps the encoded SDR policy; the 32-bit float development blends in float.

## Consequences
- Anything that changes source extent/orientation must remap `RZLM` planes through a registered inverse map or refuse.
- Local planes are source-local; they do not satisfy document-selection, scalar painting or backdrop-histogram requirements.
- Declared corpus is Nikon D70s (CIRAW decoder `8`) + oriented synthetic DNG (`8.dng`) on macOS 26.5.1; no all-camera claim.

## Qualification
Frozen before implementation: analytical mask coverage within **1 u16 code**; ordered blend vs a float64 oracle
within **`2e-6 + 0.5/65535`**; sRGB/P3 vignette and calibrated exposure within **3 u16 codes**; complete SDK
base/local camera develops within **3 u16 codes**; outside-mask, disabled, neutral and camera alpha exact; local
negative exposure must reveal original-derived detail absent from a clipped base. Results: SDK comparisons max 1
code over 577.5 M components; native masters exact over 192.5 M components; mask oracles max 1; blend max
7.75e-6; vignette max 1; real 2× Retina gesture geometry verified. Reproduction:
[`scripts/raw-local-probe`](../scripts/raw-local-probe/README.md), `scripts/verify_phase8g.py`,
[`camera-fixture.json`](../scripts/raw-probe/camera-fixture.json).

## Open items (verified 2026-10-30)
- `DONE` — lens calibration datasets and more camera/lens controls: `core/src/lens_db.rs` bundles the Lensfun database with EXIF camera/lens matching.
- `OPEN` [#11](https://github.com/kpgalligan/doctored/issues/11) — broader camera/decoder corpus, larger mask/gesture and multi-source/history workloads (carryover P7-33).
- `OPEN` [#11](https://github.com/kpgalligan/doctored/issues/11) — source-heavy scale: the noncancellable source snapshot and fingerprint stages (carryover P7-32). The camera decode itself is a host call (CIRAWFilter) with no cancel inside; RAW work checks the shared cancellation owner before and after each camera stage (`app/Sources/RawDevelopmentRecipe.swift`), which is where it cancels.
- `DONE` — the 32-bit float (linear) RAW development, generated and Sky locals, the Develop Filter and its masks and syncing locals across pictures (Decisions above).
- `DROPPED` (2026-10-30) — expanded ProRes cross-decoder comparison (from 8F): retained host sources were removed 2026-10-10 and frames land as plain pixels, so decoder agreement has no user-visible effect.
- `DROPPED` (2026-10-30) — measuring decoder migration's `RawLocalWork.scratch` envelope as its own stage: it re-measures a shipped charge that covers every measured four-local case at ≥ 1.25×, with no failure recorded.
- `DONE` (2026-11-12) — `scripts/verify_phase8c.py --host-only`'s host probe hang: under the test-launch convention
  (`DR_BACKGROUND=1 python3 scripts/verify_phase8c.py --host-only`) the drive passes, both depths, the probe in
  6.69 s. The October 600 s hang did not recur and its cause was never recorded; a run without `DR_BACKGROUND=1`
  activates the probe at each Develop RAW card and was not repeated.
- `DONE` — Lens distortion / transverse CA as source-stage geometry and geometry-changing local-plane remap (12F-4, `6400648`; `raw_local_remap.rs`, `RawLocalRemapSheetController.swift`).
- `DONE` — Typed capture packets/PPI for RAW (11D typed metadata bridge); typed auxiliary mattes (9G, P7-08).
- `DONE` — Per-entry RAW preset repair (12B: `broken` rows, `repair_raw_preset`, schema 2 on write).
- `DONE` — the measured decode charge: `RawSourceWork.scratch` = **528 MiB + 82 B/px + 3 × the file** (518 MiB plus
  10 MiB for Develop Presence) (`RawSourceWork.swift`, record beside the constants; need = ⌈1.25 × max E⌉, measured
  by `scripts/raw-decode-probe`). `develop` renders through its own `CIContext`, cleared and dropped. A RAW opens
  on a 16 GB Mac (8 GiB) up to about 90 MP.
- `DONE` — the decode charge refit on seven real 24–61 MP CC0 camera files from raw.pixls.us (`--real`), beside the
  synthetic DNGs and the 6 MP NEF: a ≥ the largest file's whole cost per pixel (`a_floor`), F the least fixed term
  for it (`measure.py`'s `fit`). A 45 MP NEF is charged 4.45 GB (was 8.04) and opens and prepares at 8 GiB
  (`--admit`).
- `DONE` — the menu path's own terms (`RawLocalWork.swift`, record beside the constants): `restoreScratch` =
  22 MiB + 2·px + 3·encoded admits Layer ▸ Redevelop RAW…'s restore; `measuredScratch` = max(`RawSourceWork.scratch`,
  522 MiB + (73 + 13·k + 16·e)·px + 3·encoded), k = captured locals + creates (≤ 4), e = authored lens, grain or
  vignette; `RawLocalWork.request` charges max(`measuredScratch`, `replaceScratch`) (the Develop preview + 64 MiB
  display; MCP `edit_raw_development` / copy-local; enhanced `redevelop_raw`); decoder migration keeps
  `RawLocalWork.scratch` (decode + 56 B/px + 4 MiB). The preview drops a superseded candidate before the next
  admission; stages are sequential.
- `DONE` — the local charge refit on the same seven files (`measure.py`'s `fit_local`): a 45 MP NEF's preview with
  four locals is charged 6.39 GB (was 9.89), so Layer ▸ Redevelop RAW… and `edit_raw_development` with one and with
  four locals pass at 8 GiB; the 61 MP file passes with one. `replaceScratch` = the core's replace plan + 32 MiB +
  8·(k + 2)·px is the floor of the charge (it binds for a scaled layer or a float document). Core caps unchanged:
  a local mask to 64 MP, a RAW's file and masks within the resource cap.
- `DONE` — the publish projection estimate adds `ProjectionMargin`'s 6/16 at 16 bits and 7/16 at 8.
