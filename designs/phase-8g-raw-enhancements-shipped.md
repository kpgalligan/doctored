# Phase 8G — camera controls and local RAW development: shipped runs

The settled record of 8G and its follow-ups: the decisions in force, each shipped item's few lines, added by
the batch that ships it.
[phase-8g-raw-enhancements-status.md](phase-8g-raw-enhancements-status.md) keeps the head, the rejected
alternatives, the consequences, the qualification and `## Open items`.

## Decisions
- **Frozen evaluation order:** original camera bytes → pinned global camera/vendor lens development for the base
  and each enabled local variant → chosen sRGB/Display P3 integer u16 output → ordered native coverage blend (float
  accumulator) → calibrated manual vignette → u16 source quantization → explicit optics → develop Presence (Dehaze,
  then Texture and Clarity) → develop grain → existing retained layer affine/profile/fallback → layer mask/retouch.
  Variants share decoder, orientation, dimensions and output ICC. *Why:* each local develops the original, never a
  byte preview; single quantization keeps the blend exact.
- **Exact bypasses:** zero coverage, disabled/identity locals and neutral vignette are bit-exact no-ops.
- **Locals:** at most sixteen (formerly four), ordered, in top-left, oriented, full-source pixel coordinates *before* layer
  geometry. Coverage is native u16 from radial/linear primitives and ordered brush add/subtract, with inversion,
  density and feather; never derived from a byte preview. Each local changes exposure offset, WB intent and tone (see Local tone path).
  Hard radial fields use point-center sampling (not area integration). Code: `raw_local_*.rs`,
  `RawLocal*.swift`, `RawDevelopWindowController+Locals.swift`.
- **Range masks:** a local may carry a luminance band, a colour distance, or both; the range multiplies the shape's
  coverage per pixel, after density and feather, and is not blurred; a pixel it excludes keeps the base sample
  bit-exact. Mask op `full` covers the frame so a range can work alone. The range reads the *base* development
  (before any local, the vignette, optics and grain) as CIE L\*a\*b\* against D50 through the base's own profile,
  L\* clamped to 0…100. *Why:* a local's exposure or white balance cannot move its own selection, the same bounds
  select the same tones in every output space, and a falloff width is perceptually even; the clamp keeps a built-in
  profile's white (Y sums to 65537/65536) inside a hard 0…1 band. A colour may be given as a source point, resolved
  to the mean Lab of its 5×5 base pixels; a saved recipe holds only the Lab. No plane is added, so local charges
  are unchanged. The recipe stays opaque JSON to the core (no DRDC or RZLM bump); the enhancement `version` is 2
  when any local has a range and 1 otherwise, so an older build keeps the saved rendering. Code:
  `raw_local_range.rs`, `RawLocalRange.swift`, `RawLocalRangeControls.swift`; drive
  `scripts/verify_v1_raw_range.py`.
- **Generated masks:** a local's coverage may be Vision's Select Subject or Select People mask, or the M1 sky
  model's own matte (mask op `subject` / `people` / `sky`; `SkySelection.coverage`, shared with the Develop
  Filter's Sky mask), read from the *base* development like a range, so it is in canonical source coordinates.
  Vision's byte coverage is stored as u16 `v·257` in an ordinary `RZLM` v1 blob (no RZLM, DRDC or
  enhancement-version bump); the node's `generated` key records the kind, and a build that predates the key refuses
  the recipe and keeps the saved rendering. **Recompute is on request only:** stored coverage never changes by
  itself (global edit, preset, Sync Settings, Redevelop RAW, migration, reopen); the `mask` op with `subject` /
  `people` / `sky` replaces the whole plane (brush and Invert since are not replayed). Inside one open Develop
  session a not-yet-applied generating operation reads that session's base; a refusal drops the operation only on
  its first run and never deletes previewed work. Nothing found refuses with the selection commands' own sentences;
  with no sky model New Sky creates nothing and offers Settings ▸ Models, and MCP refuses with
  `ModelGate.notInstalled("sky")`. Presets and a global Sync Settings move no local, so no generated mask is copied
  or recomputed there; Sync Settings' Masking detects it again (Syncing locals). The Vision stage runs inside the
  existing local charge. Code: `RawLocalGenerated.swift`, `dr_raw_local_mask_from_coverage`; drive
  `scripts/verify_v1_raw_subject.py`.
- **32-bit float (linear) development:** a third master encoding, `raw.output_encoding: linear_float` (Develop ▸
  Document depth "32-bit float (linear)"; Layer ▸ Redevelop RAW… ▸ Development in a float document). The one decode
  seam renders `.RGBAf` into the output space's extended linear twin (`CGColorSpaceCreateExtendedLinearized`) with
  `extendedDynamicRangeAmount` 1 (measurement M1, `scripts/raw-probe/float_edr.py`); the f32 master is tagged with
  the space's existing built-in profile, since a float document holds linear light in its profile's primaries, so
  no linear ICC is made. It exists only in a 32-bit float document (typed DRDC word 78, `float_native`) and never
  narrows out of one (`convert_precision` preserve and a layers paste refuse it by name until it is redeveloped
  `sdr16`). Recipe: a policy string (`<space>-rgba32f-linear-v1`; `-v2` when the tone curve is above 0, so an older
  build keeps that rendering) beside its `extendedDynamicRange`, no recipe version bump. Controls: exposure,
  baseline compensation, white balance, local contrast, sharpness, detail, both noise reductions, moiré, profile
  corrections and highlight recovery act in the decoder as at 16 bits; the tone curve and Shadows do too (the
  decoder's `boostAmount`/`boostShadowAmount` under the extended range, values above white kept: M2,
  `scripts/raw-probe/float_tone.py`); a float development still starts at tone curve 0, Shadows inert while it is
  0; local tone map 0; manual vignette a linear gain; lens geometry resampled in linear float; grain on the display
  encoding since [#680](https://github.com/kpgalligan/doctored-dev/issues/680) (Doctored's rule, not measured against
  Adobe Camera Raw: the one Add Grain kernel on E, the profile curve continued as a logarithm above white; each
  pixel's encoded offset is the 16-bit grain's for its encoded luma clamped to 0…1, added to each channel's E(v)
  unclamped and decoded back, so a channel that stays in 0…1 gets the 16-bit grain of its encoded value and a pixel
  at or above white keeps its value; `tests/raw_float_grain.rs`; charge the core's float selected-edit plan, GR32
  plan ÷ max E 1.323/1.307/1.304 on the NEF/D24/D850); locals blend f32 variants in f32; a range reads Lab of the
  f32 base (above white is L\* 100). The development is linear in exposure with no highlight shoulder, so it looks
  brighter in the highlights than the 16-bit development of the same recipe until tone-mapped; where a channel
  exceeds white the 16-bit development moves the colour toward white, so the clipped view differs in hue as well:
  tone mapping, not clipping, is how it is meant to be viewed. Inspection (Fit/100 %, clipping, Before/After) shows
  it clipped to SDR, except on a screen with headroom, where Develop's Fit and 100 % views, Clipping on or off,
  draw rule X's binary16 image (`develop_preview_extended`, the EDR view's plan at the preview size; markers at or
  below 0.0 / at or above 1.0; MCP `hdr_preview`, #680); its histogram reads the f32 master in stops (the float
  scoped histogram, #680). Drives `scripts/verify_v1_raw_float.py`, `scripts/verify_v1x_raw_hdr.py`. Admission,
  formulas fixed before measuring: decode `RawSourceWork.scratch` + 16·px; local `measuredScratch` + (8·(k + 2) +
  8 + 8e)·px; `replaceScratch` 16·(k + 2)·px; a Develop session + 24·px. `measure.py --float` on the NEF, D24 and
  D850: every charge ≥ 1.25 × the measured peak (smallest 1.269, K32-NEF), no constant raised (records beside the
  constants). At 8 GiB: a float open to about 78 MP (D850's bytes a pixel), a Develop session to about 53 / 49 /
  35.5 MP with 0 / 1 / 4 locals.
- **Develop Filter:** Filters ▸ Develop Filter… (⇧⌘A, MCP `apply_filter` `develop_filter`) runs Develop's global
  controls on any layer as one destructive `SpatialKernel` arm composed only of existing ops, in a fixed order:
  white balance → exposure → contrast → highlights/shadows → whites/blacks → Presence (Dehaze, Texture, Clarity) →
  vibrance/saturation. A non-RAW layer has no as-shot white, so Temperature is relative: the `white_balance`
  illuminant `10⁶/(10⁶/6504 − t)` K (one mired a unit, + warms), Tint passed as given. Not re-editable (a second
  run starts from the pixels); 8/16/32-bit. Destructive by design: Doctored has no Smart Objects (removed
  2026-10-12). CMYK refuses by name (take `camera-raw-masks-579/take1`: RGB 8/16/32 run, CMYK 8 "not currently
  available"). Left out (CIRAWFilter camera-space, no core equivalent): tone curve, shadow boost, local contrast,
  sharpness, detail, noise reduction, profile corrections, highlight recovery, moiré, baseline compensation; output
  space and decoder are RAW-only; vignette and grain are Lens Correction and Add Grain. Its own preset kind
  (`DR_DEVELOP_FILTER_PRESETS_DIR`; the folder was `Camera Raw Filter Presets` until the 2026-10-29 rename, moved
  once at launch by `DataMigration.moveRenamedFolder`) reads Develop presets taking only `exposure`; Actions,
  presets and shortcut overrides saved under the earlier names still load (`LegacyNames.swift`). Code:
  `sampling_develop_filter.rs`, `DevelopFilterSheet.swift`; tests `filter_develop_filter.rs`; drives
  `scripts/verify_v1_develop_filter.py`, `scripts/develop-filter-admission/measure.py`.
- **Develop Filter masks:** the filter's `masks` (≤ 16, in order after the globals): radial, linear, brush, full,
  range, and subject, people and sky shapes the host makes on a ≤ 1024 px analysis copy of the layer and sends as
  `map` shapes; each carries the twelve local controls (no Vibrance) as a `DevelopFilter` plane over the picture
  the earlier masks left, blended by RAW-local coverage (`Command`, `EffectiveMask`) times a `LocalRange` read from
  the layer's own samples through the document's profile. One implementation: no new pixel math. 8 and 16 bits; a
  32-bit float layer and a mask, plane or channel target refuse by name. A recorded step keeps generated shapes as
  asked, so a replay reruns the model. Local Sharpness (−100…100) and Noise Reduction (0…100) shipped later: run
  last over a mask's value as Reduce Noise, then Smart Sharpen (radius 1, amount up to 150 %) or Gaussian Blur (up
  to 3 px), at radii fixed in pixels (`local_detail.rs`, shared with Develop), so the sheet's proxy preview leaves
  them out. A mask's `range` is normalized host-side through `RawLocalRange(json:)` (omitted falloffs and invert,
  `range: null`). Code: `develop_filter_masks.rs`, `develop_filter_masks_request.rs`, `local_detail.rs`,
  `DevelopFilterMasks.swift`, `DevelopFilterMaskPanel.swift`, `AgentServer+DevelopFilterMasks.swift`; tests
  `filter_develop_filter_masks.rs`; drives `scripts/verify_v1_develop_filter.py` (MASK cases),
  `scripts/develop-filter-admission/measure.py` (MASK and MASKD cases); take `camera_raw_masks_take.py`.
- **Develop Presence:** Texture, Clarity and Dehaze (−100…100, 0 = off) run the Filter ▸ Presence kernel
  (`selected_edit` op `presence`, 16-bit, a controlled job) on the finished development, after explicit optics and
  before grain; locals get none. *Why:* the kernels are native stages on the developed, display-encoded u16 output,
  so the midtone weight and airlight read the developed tones; after locals, vignette and optics the bands and
  airlight are the published frame's and no local variant reruns them; before grain they never amplify synthetic
  grain. The radii scale with the long side (`presence_radii`, half-size agreement pinned by
  `tests/filter_presence_scale.rs`), and both Develop previews run the full-size pipeline, so the preview is the
  Apply result. Persistence: an optional recipe `presence`, absent when all three are 0 (recipes without it encode
  byte for byte as before); no DRDC, RZLM or enhancement-version bump, so an older build refuses the unknown key
  and keeps the saved rendering. Presets are schema 4. The 32-bit float (linear) development refuses nonzero values
  with `RawPresenceSettings.floatRefusal` in `RawDevelopmentRecipe.updating`, the seam every path reaches, and
  Develop disables the three rows at that depth (`scripts/verify_v1_raw_float.py`). Charge: an effect in
  `RawLocalWork.chargeShape`; the open and the import preview with Presence raised `RawSourceWork.scratchFixed` to
  528 MiB (VP16-NEF), the local charge was unchanged (PX-16-D850 1.289×). Code: `RawPresence.swift`; drive
  `scripts/verify_v1_raw_presence.py`.
- **Local tone path:** a local may carry a `tone` (Exposure −4…4 stops, Contrast, Highlights, Shadows, Whites,
  Blacks, Saturation, Clarity, Texture), applied in the core to the 16-bit development through the existing kernels
  (precision_v1 `exposure` under `document_v2`, `bcs` modern, `shadows_highlights` at the Presence coarse radius,
  one `levels`, `vibrance`, then the Presence plane) and blended through the coverage × range. A local with no
  camera change develops no variant: it reads the development as the earlier locals left it, so tone locals
  compound. Only a local that changes white balance or camera exposure (`exposure_offset`, which recovers clipped
  highlights a gain cannot) develops a variant: at most four of those, sixteen locals in all, and the tone of such
  a local adjusts its variant. Recipe enhancement `version` 3 marks any tone or more than four locals (an older
  build keeps the saved rendering); a version-1/2 recipe runs the old code with every local passed a variant, so an
  old `.rz` reopens bit-exact. Intended version-3 change: a local with no camera change and no tone is a no-op
  (before, it blended the base back and undid earlier locals inside its coverage); it appears only after a user
  edit moves the recipe to version 3 (`core/tests/raw_local_tone.rs` pins both contracts). A tone on the 32-bit
  float (linear) development refuses by name (`RawLocalTone.floatRefusal`, the core's sentence) in
  `RawDevelopmentRecipe.updating`, as Presence does, and Develop disables the tone rows at that depth: the tone
  kernels are display-referred and have no rule for values above 1.0. Dehaze is not a local control (it needs a
  global haze estimate). The combined RAW + attachment resource cap is 1 GiB (RAW originals alone keep 512 MiB): 16
  masks fit up to about 32 MP, fewer above (D850 11, A7R4 8), refused up front by `RawLocalWork.request`. A `.rz`
  whose attachment holds more than four masks or whose resources pass 512 MiB writes word 79
  (`raw_locals_native.rs`); the writer is DRDC 79, after the typed float word 78. The local charge gains a
  per-local and a tone-stage term, both measured at 0 at 24 MP (the stage peaks under the decode, the masks sit
  inside the creates' camera term; record beside `RawLocalWork`'s constants). Code: `raw_local_tone*.rs`,
  `raw_local_limits.rs`, `RawLocalTone*.swift`; drive `scripts/verify_v1_raw_local_tone.py`.
- **Develop locals' Sharpness and Noise Reduction:** a local's `tone` also takes `sharpness`
  (−100…100) and `noise_reduction` (0…100), run last over the Presence plane or the pointwise steps by
  `local_detail.rs` (the filter masks' mapping); stored in the recipe only when not 0, so an older build's `sameKeys`
  refuses a recipe that names them and keeps the saved rendering, with no new `.rz` word. 32-bit float refuses them
  with the tone. Check-only LTD (LTS + both at 60, D24 and Z6) is covered by the committed charge (record beside
  `RawLocalWork`'s constants).
- **Global controls exposed only where a capability matrix shows effect:** camera-relative baseline compensation,
  moiré reduction (where supported), manual profile-aware vignette. Unsupported controls (e.g. vendor lens
  correction and local tone map on the D70s) are not shown.
- **Recipe 3** stores every effective control plus captured effective variants; recipes 1/2 and strict binary64
  persistence keep their meaning. Unknown recipe or mask versions keep cached native rendering and refuse
  semantic edits.
- **Syncing locals:** Synchronize RAW Settings' Masking (dialog checkbox, `sync_raw_settings.locals`, replayed by
  `apply_raw_settings.locals`) REPLACES each target's locals with the source's, in order (≤16), ids, names and
  controls kept. Replace is the only mode. Normalisation is per axis in each RAW's canonical frame (EXIF
  orientation applied, the whole oriented frame, before explicit lens geometry; a RAW recipe has no crop, and a
  document crop or layer transform comes later, so it is ignored): a landscape mask on a portrait target keeps its
  relative placement, stretched per axis. Drawn coverage is resampled by `dr_raw_local_mask_resize`
  (edge-normalised bilinear resize, not the affine remap, which dims the edges on enlargement). A generated local
  is detected again on the target's base, after the synced globals; Brush or Invert refinements made on it are not
  carried (Recompute's rule), and one that finds nothing or lacks its model is skipped and named. Feather, a blur
  radius in mask pixels, is scaled by the geometric mean of the two axis ratios (clamped to 256) so the blur keeps
  its share of the frame. A range is copied as its stored Lab and never re-sampled. A replace that changes nothing
  publishes nothing (a create whose removed twin has the same mask hash keeps it). A recorded step stores its drawn
  masks as `<sha256>.drmask` in `RawLocalMaskStore` (`DR_RAW_LOCAL_MASKS_DIR`). Code: `RawLocalSync.swift`; drive
  `scripts/verify_v1_raw_locals_sync.py`.
- **Mask storage:** versioned `RZLM` blobs in the 8F native 21/22 `RBIN` pool under `raw_local_masks`; mask
  pixels never enter the capped recipe JSON. ≤64 MP per mask; 256 MiB per resource; originals + masks within
  1 GiB together (formerly 512 MiB, and for originals alone). Unchanged masks/originals share storage across revisions; every recognized header,
  length and reference validates before pixel decode.
- **Transfer rules:** global presets/sync transfer camera-relative compensation, supported moiré and vignette
  while preserving each target's local IDs and masks; Sync's Masking replaces the target's locals with the source's
  (Syncing locals); explicit local copy transfers exposure, WB intent and tone only; never another camera's hidden
  recipe/decoder.
- **Migration** at unchanged geometry preserves planes exactly and reviews the complete new development
  (12F-4 later added reviewed EXIF/crop/pad and filtered-affine remap of local planes).
- **Plumbing:** UI/MCP/Actions share bounded captured operations and one prepared publication. Cheap attachment
  kinds keep RAW masks out of host-frame summaries/menus/recording even when metadata reads are denied.
- **Bounds:** ≤16 locals (≤4 camera variants), 5 cached camera developments / 256 MiB, 256 pending operations, 4,096 aggregate brush
  dabs, 256 M evaluated brush-box pixels per native op, 256 KiB operation JSON. Platform capture/render calls are
  noncancellable; native stages cooperate. The 64 MP cap is not a four-local 64 MP qualification.
