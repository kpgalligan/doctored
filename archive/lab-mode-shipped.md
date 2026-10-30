# Lab Color document mode (#547): shipped runs

The settled record of [#547](https://github.com/kpgalligan/doctored-dev/issues/547)'s Lab Color phases: decisions in force
and each shipped phase's few lines, added by the batch that ships it. [lab-mode-status.md](lab-mode-status.md) keeps
the head, the unshipped contracts, "Photoshop in Lab (measured)" and `## Open items`. Takes live under
`rasterize-fixtures/photoshop/lab-mode-547/`, runners in `scripts/photoshop-probe/`.

## Decisions in force

- **Storage is the RGB precision document holding L\*, a\*, b\* codes:** a `PrecisionDocument` whose `lab`
  policy (`LabPolicy`: depth, twin) is `Some`; u16 only, R, G, B = L\*, a\*, b\* in the ICC v4 16-bit Lab
  encoding (L\* = 100·v, a\*/b\* = 255·v − 128; a\* = b\* = 0 is 0x8080). 8 bits stores each code ×257, and
  every conversion into it rounds samples and alpha to that grid (Gray's rule). No new pixel type or compositor.
- **No 32-bit Lab**: a Lab document is 8 or 16 bits; a 32-bit one converts to 16 first.
- **The twin is the profile** of the document and every layer (`icc_builtin::lab_twin_blob`: v4.3 `mntr`, data
  `RGB `, PCS `Lab `, D50 `wtpt`, identity `mAB `/`mBA ` B curves), parsed as `RgbLut`: conversions run through
  `ColorPipeline`, and Normal compositing is the RGB compositor's linear mix of Lab codes, matching Photoshop's.
- **The core makes the view:** `lab_convert::lab_display` converts to the built-in Display P3 at u16 (relative,
  no BPC) before any narrowing; the host draws Display P3. ColorSync never decodes the twin (it read a\*/b\*
  half a unit high, which missed the drive's L3 and L6; controller ruling, option b).
- **One conversion rule both ways:** each u16 pixel through `ColorPipeline::new`, clamped to 0…1 and quantized
  (×257 at depth 8), alpha kept. RGB/Gray → Lab is relative colorimetric from the document profile, no BPC (Gray
  from its RGB twin); a\*/b\* past −128…127 clip. Lab → RGB takes the destination's intent and BPC.
- **CMYK → Lab goes through 16-bit ProPhoto RGB** (relative + BPC) in one job, as CMYK → Grayscale; Lab → CMYK
  (5a, `lab_cmyk`) runs the same stage reversed in the core — the twin into u16 ProPhoto, never narrowed, then
  RGB → CMYK at the declared depth — refusing by name what CMYK → Lab's mirror refuses (`flatten_required`).
  It is NOT take-confirmed: `lab-cmyk-547/take1` (5b) read MISS, max/mean 6.06/1.04 codes at 8 bits and
  5.88/1.02 at 16, C1 (this route) = C2 (Lab PCS to B2A), so the residual is the → CMYK leg's;
  `lab_cmyk_photoshop.rs` pins it.
  Lab → Grayscale (5a-ii, `lab_gray`) is each pixel's L\* alone: Y = L\*⁻¹, then the Gray
  profile's tone curve at the declared depth, a\* and b\* discarded (take-confirmed).
- **No document profile (5b).** Proof Setup, Proof Colors and Gamut Warning proof the
  Display P3 view through the unchanged RGB proof (`proofSourceProfile`; `exact` the u16 view; `space`
  display-p3); `soft_proof` refuses the twin as a proof's document profile (`PROOF_VIEW_REFUSAL`). Convert to
  Profile… is Image ▸ Mode's conversion into a CMYK, RGB or (6a-ii) Gray profile (`CmykModeConversion`; a Gray
  row is 5a-ii's L\* route into that profile, intent and BPC not applying, `profile` a Gray working space by
  name or ICC bytes); Assign Profile… is disabled (the document has no profile); the four profile exports refuse
  (`PROFILE_EDIT_REFUSAL`).
- **Bits/Channel on Lab (5a, `lab_depth`):** 16 → 8 rounds every raster, alpha, mask and alpha-channel sample
  to the ×257 grid, no dither; 8 → 16 changes only the declared depth; 32 is disabled.
- **The file encodings are measured (3a, `lab_encode::storage_code`).** PSD, PSB and a layered TIFF's block: 8
  bits P8 (L\* = 100v/255, a\* = v − 128); 16 bits L\* = 100v/65535 and a\*/b\* on Photoshop's 15-bit grid, i =
  round(v·32768/65535), a\* = (i − 16384)/128 (`psd16_ab_code`, phase 3's readback fix 2). TIFF Photometric 8
  (CIELab) measured at 8 and 16 bits; Photometric 9 (ICCLab: P8, 16 bits L\* = 100v/65280, a\* = v/256 − 128)
  declared from the published spec, not measured against Photoshop.
- **Lab files are written as Photoshop writes Lab (3b, `psd_export_lab`).** Colour mode 9, three composite
  planes matted over Lab white (`lab_encode::matte_white`), a positive count, no ICC anywhere, 16-byte folder
  `lsct`, a layered TIFF's strip three CIELab samples (Photometric 8); samples by `lab_encode::psd_sample`
  (8 bits P8; 16 bits L\* as the code, a\*/b\* on the 15-bit grid, `psd16_ab_sample`); effect colours `LbCl`.
  Adjustment layers are written as their stamps (no Lab record was measured, H-L), reported as losses.
- **`.rz` word 107** sits in the CMYK slot (never beside a CMYK word or Gray's 106); its `LAB ` table (u32
  schema 1, u8 depth 8|16) is the last before LIDS. `RZDC_VERSION` stays 89: 107 is a conditional word like
  90–106 (`rzdc.rs`'s rule), and every earlier file reads unchanged. The reader requires word ⇔ table, u16
  storage, and the body profile byte-equal to the twin; the writer runs the reader's check first.
- **The gate:** the core's `lab_document::admit_edit` (in `ffi_precision_edit::request`) and
  `precision_capability::LAB_FAMILIES`; the host's `LabDocument`, asked first in `DocumentPrecision` and
  `validatePrecisionItem`. Every refusal is the core's sentence: "`<operation>` is unavailable in a Lab Color
  document until #547 phase `<p>`." (`LATER_PHASES` / `laterPhases`, else 6), or for the Magic Wand and Select
  Similar (`AWAITING_TAKE` / `awaitingTake`, since 4c) "… until its Lab tolerance rule is measured (<issue>).", where Select Similar's issue is #683.
  Panels never label Lab codes as RGB (1c): Channels shows the raw Lightness, a and b codes, Info reads
  L\*a\*b\* with a Lightness/a/b histogram, RGB, HSB and K at "—".
- **The blend modes are measured (phase 2's take) and composite by Lab arms (phase 3).**
  `lab_document::LAB_BLEND_MODES` (host `NativeLayerBlending.labModes`): 19 modes in menu order, the same 19
  phase 2's take read in Photoshop 26.10.0's Lab. `LAB_NOT_OFFERED` (host `labNotOffered`): Darken, Color Burn,
  Lighten, Color Dodge, Difference, Exclusion, Subtract, Divide refuse for good ("The `<mode>` blend mode is
  unavailable in a Lab Color document."). Every Lab blend routes through
  `lab_blend` (`kind_in`, `fill_in`, `lab_fade_blend`; `LAB_ART.kind`): layers, merges, flatten, render, export,
  paint, Fade, the Clone Stamp, Art History and effect inks (`Contribution::ink`'s `Encoded16`). Vivid Light and
  Hard Mix differ by depth (`LabGrid`, via `PrecisionScene::lab`, `StyleEnv::lab`). Fade and the Mode popups
  take the same list.
- **Effects composite on the Lab values (2e, `lab_style`).** Stored as written; authored sRGB colours convert at
  composite (`AuthoredColors::LabTwin`, the Mode rule, 8-bit `color8`), so RGB ↔ Lab carries a style unchanged;
  `LayerStyleEdit::target` and Mode ▸ Lab Color's blocker refuse an effect mode `blend_refusal` refuses. **A
  pattern tile is authored sRGB (2f-ii):** under `RenderContext::lab_tiles` (only `LabTwin`)
  `painted_sampler` turns each texel into 8-bit Lab codes (`lut_ink`) BEFORE the pyramid, at RGB's level size;
  the stored tile stays sRGB. A Bevel's texture keeps `fill.sampler`; byte, CMYK and RGB LUT documents keep
  `lab_tiles: false`.
- **Fill and paint are the RGB paths on the codes (1b, 2a).** `precision_paint::prepare_colors` makes every ink
  Lab codes by `LabPolicy::code` (an sRGB ink through `lut_ink`); the Brush, Fill, Paint Bucket, Gradient and
  Eraser run `precision_paint_tools` unchanged, modes `LAB_BLEND_MODES`; a partial 8-bit stroke writes off the
  ×257 grid until an 8-bit save. Stroke… (2f-ii) runs the same way (`paint_blend`, `stroke_layer_edge`, the
  foreground's Lab ink when kept); the Fill and Gradient Mode popups list the Lab modes (`toolBlendChoices`), a
  stored outside mode painting as Normal.
- **Adjustments run the RGB kernel in a Lab form (2b, 2c).** `lab_adjust::LAB_ADJUSTMENTS` decides each op;
  `PrecisionAdjustment::parse` binds its `lab_tone::LabForm` (with `LabBinding`) on the twin; the stored
  description is the RGB one, and the scalar path parses unbound. Levels, Curves and Brightness/Contrast act on
  Lightness (`[L, L, L]`); their r, g, b are Lightness, a, b, a\* and b\* by a second kernel of the channel
  stages on `[L, a, b]` (`Split`; a map-form Curves resamples its absent maps by ±1 code, as RGB); a
  Brightness/Contrast saturation scales a\*, b\* by 1 + saturation (`Chroma`). Invert and Posterize act per
  channel (the 16-bit neutral goes to a\* = −1); Threshold is black and white, a\* = b\* = 0; Desaturate
  (Hue/Saturation master −1) keeps L\* and zeroes a\*, b\*.
  The `Unavailable` ops refuse "`<Name>` is unavailable in a Lab Color document.", RGB-only ops as such;
  Shadows/Highlights and Replace Color refuse as layers. The binding's heap is recorded; no new memory plan.
- **The colour ops run through an RGB intermediate (2d).** Hue/Saturation, Color Balance, Photo Filter,
  Shadows/Highlights and Replace Color run in ENCODED 16-bit ProPhoto RGB (`lab_adjust::color_space`, twin ↔
  space `lut_pipeline`s built at parse, clamped back); Color Lookup and Gradient Map in sRGB. An untouched sample
  keeps its codes; Shadows/Highlights' guide is the space luma. Photo Filter's `color` is authored sRGB (its
  transient pipeline drops before the kernel; the retained-bytes test covers it);
  Replace Color's `colors` are the 8-bit `#LLaabb` codes (`LabDocument.codeHex` of the foreground; its label
  reads L\*, a\*, b\*). Hue/Saturation's sampler reads `CanvasSample.kernelRGB` (`rz_lab_kernel_rgb`), Replace
  Color's and Curves' the codes; Photo Filter's and Gradient Map's sheet colours are sRGB.
- **The Lightness, a, b and alpha planes are targets (2d, 2e).** The core admits `scalar_statistics`,
  `read_plane`, `scalar_edit` (asking `lab_filter::scalar_refusal`, the mask rule) and paint of red, green,
  blue and alpha; the host names them lightness, a, b, alpha (`paintTarget`, `agentName(in:)`), red, green
  and blue refusing (`planeTargetRefusal`, `scalarTargetRefusal`). A mask's, alpha channel's or plane's filter
  follows the scalar rule (`[g, g, g]`, its luma): the luma filters, Offset's edge and Clouds run unconverted;
  only the for-good rows and Liquify refuse (`LabAdjust.scalarFilterRefusal`, `allowsScalarFilterAction`).
- **RGB/Gray → Lab carries adjustment layers (2d)** byte-equal, rebound to the twin, unless
  `lab_adjust::lab_carry_refusal` names an op Lab refuses or a pixels-only op (`flatten_required`); the plan
  adds `rebind_scratch_bytes`; a CMYK adjustment layer stays a blocker; Lab → RGB carries them rebound (2b).
  **Channel Levels/Curves and Blend If
  carry by channel index (2f; not measured):** Red ↔ Lightness, Green ↔ a, Blue ↔ b both ways, byte-equal; a
  Gray or plate Blend If range blocks → Lab (`blend_if_channel_refusal`). The Lab → RGB plan refuses only a
  non-Lab document and a destination that does not prepare.
- **Color Samplers (2d)** read `sample_pixel` with `LabDocument.readingFields`; Info reads L\*a\*b\*.
- **Filters run their RGB kernel on the codes (2c),** each channel alone. Add Noise draws a\* and b\* too; High
  Pass's pivot sits half a code below neutral; Clouds' colours by the Mode rule. `lab_filter::LAB_FILTERS`: Lens
  Correction, Develop Filter, Difference Clouds refuse as `Unavailable`, Presence and Add Grain as Develop
  Filter controls, Depth Blur as RGB-only. Repeat Last Filter answers as Gray's.
- **The luma filters run in one-channel passes (2f, `lab_channel_filter`; not measured).** Pass k is a
  layer-sized sRGB RGBA16 `[c_k, c_k, c_k, α]` through `selected_edit_with_context` (frames, banding, seeds and
  the no-op rule RGB's); L\*, a\*, b\* = the passes'
  G, α = pass 0's, under the `Pixels` lock. Emboss and Edge Detect run L\* alone and mix a\*, b\* toward
  `lab_tone::NEUTRAL` by coverage (`mix_codes`); Reduce Noise runs all three, its chroma half seeing U = V = 0,
  so `color` has no effect and with luminance off nothing changes. Offset's
  fill is `fill` on L\*, neutral on a\*, b\*; on a plane the host fills that plane's code of the background's
  L\*a\*b\* (`FilterFrame.kernelParameters` `lab:`). Plan: pass 0's + layer × `PASS_BYTES_PER_PIXEL` (26) +
  canvas × `VIEW_BYTES_PER_PIXEL` (21, CMYK's measured start) + 2 MiB (`get_precision`'s `filter_plans`).
- **Blend If on Lightness, a and b (2f; not measured):** the stored red, green, blue
  ramps on the codes (a\* = 0 at 128); Gray refuses by name, an identity range included (`blend_if_refusal`);
  the sheet lists `BlendIf.labChannels`.
- **Auto Tone, Auto Contrast and Equalize act on Lightness (2c)**: `contrast_endpoints` of the L\* histogram,
  a\*, b\* at 0, 1, 1; Equalize writes L\* alone. Since 2f the Levels Auto row's Auto Tone does the same and
  Auto Color refuses (button disabled).
- **Retouch on the codes (2e, `lab_retouch`; no take).** Dodge and Burn shape L\* alone (Protect Tones on
  `[L]`); the Sponge scales chroma about a\* = b\* = 0 (`Vibrancy::Lab`), Vibrance ignored; Blur, Sharpen,
  Smudge, Clone, Healing, Spot Healing, Patch and Content-Aware Fill (and preview) and Move run per channel
  unchanged (`doc_inpaint` reads no profile), the Clone Stamp in the Lab modes; the History Brush needs a Lab
  state (and a Lab state paints only Lab), the clone from plates Lab plates. RGB plans.
- **Remove, Art History, Red Eye (2f-iii; no take).** Remove's model reads the P3 view (`view_pipeline`), its
  fill back through `from_view_pipeline`. The Art History Brush (8 bits; 16 refuse) is Gray's u16
  walk (`U16Art`, `LAB_ART`): per channel, strokes along L\*, Tolerance on all four, a Lab state only, modes
  by `blend_refusal`. Red Eye corrects each pixel's 16-bit ProPhoto colour; an untouched pixel keeps its codes.
- **The Eyedropper keeps an exact Lab ink (2e, `LabInk.swift`):** `SharedToolState.foregroundLab` (the swatch
  its clipped sRGB, `LabPaint.display`) paints a layer from the Brush, Fill, Gradient and Edit ▸ Fill as the
  twin's unit codes (`LabPaint.color`); coverage targets keep the sRGB swatch; any other swatch write
  clears it. MCP paints exact codes with `color_space document`.
- **Geometry is the RGB kernels on the codes (phase 4a; Rasterize's rule).** Every
  resample and transform runs `precision_geometry*`, `perspective_crop` and `doc_crop_extend` unchanged on
  the L\*a\*b\* codes, with no Lab arm, so a\* = b\* = 0 stays 0x8080; 8-bit results stay off the ×257 grid
  until an 8-bit save, as paint. Canvas Size and Reveal All add transparency at every mode; a crop past the
  canvas pads the Background with its colour by the Mode rule (`prepare_colors`; the host sends the stored
  `LabInk`, else the sRGB colour unquantized as `srgb`, MCP's `background_color` too). Since 6b-iii the
  crop's Generative Expand fills the new area by the 6b-iii rule (below). Since 4a-ii Warp, Puppet Warp, Perspective Warp, Liquify, Align and Distribute and the
  Move tool run their RGB kernels the same way; a Lab warp is applied at once and keeps no binding
  (`precision_warp`, Gray's rule), so `.rz` never holds a Lab warp field and `get_warp` answers null.
- **The clipboard keeps the Lab codes (4c, `lab_clipboard`; the Mode rules, no take).** A Lab copy is RZCB v6
  (u8 depth, then one v3 stream of the codes in the twin). Lab → Lab keeps the codes under both profiles; into
  8 bits a payload of EITHER depth lands on the ×257 grid (`lab_depth::on_grid`; since 6a-i an 8-bit payload
  too, since 8-bit paint leaves off-grid codes, and New from Clipboard of one); RGB → Lab is Mode ▸ Lab Color's rule
  (relative, no BPC); Lab → RGB relative + BPC (`lab_cmyk::stage_options`, the cross-mode rule); Gray and CMYK
  through 16-bit ProPhoto RGB; Don't Convert refuses across modes. The pasteboard TIFF is the Display P3
  rendering. The layers paste refuses a Lab mix (`LAB_MIX_REFUSAL`); the host converts the clip first
  (`LabLayerClip`: Gray → Lab directly, CMYK → Lab through ProPhoto). Duplicate keeps the policy.
- **Artwork is authored sRGB (6a-i, `lab_artwork`; Rasterize's rules, no take).** Type, shape and fill layer
  descriptions keep their sRGB colours; `artwork_render` renders them into the twin, every colour by `lut_ink`
  (Mode ▸ Lab Color's rule), a gradient between its converted stops' codes, a pattern tile texel by texel
  (`lab_style::twin_tile_sampler`, `painted_sampler`'s body); an 8-bit publication lands on the ×257 grid
  (`on_document_grid`). Mode conversions drop the description in the core and the host re-renders it
  (`carriesArtwork`); Bits/Channel keeps it. Export as PSD writes a Lab fill as pixels, a reported loss.
- **Comps carry unchanged and Apply skips what Lab cannot hold (6a-ii, `lab_comps`; no take).** RGB/Gray ↔ Lab
  clone the comps (`lab_convert::carried`; ids bind through `map_tree`, a recorded style is authored sRGB like
  the layers' own, no plan term); a flatten clears them first. Apply skips a recorded mode outside
  `LAB_BLEND_MODES` and a recorded style (merged with its Blend If, `reported_style`) with an effect mode or
  Blend If channel Lab refuses, leaving the layer's own value (`doc_comps`' rule, as Gray's `gray_comps`).
- **Channel math blends each channel's codes alone (6b-i; Rasterize's rule, no take).** Apply Image and
  Calculations run `NativeChannelMath`'s RGB path on the codes in `LabDocument.channelMathModes` (`labModes`
  less Darker/Lighter Color and the HSL four); a layer in the same mode composites by its `lab_blend` arm, so
  the two can differ. The Lab composite onto one plane reads Lightness, never `luminance`.
- **Color Range's other ranges (6b-i, `selection_tools`).** The tonal ranges read the codes' L\*; the hue
  ranges and Skin Tones the codes' 16-bit ProPhoto RGB encoding (Hue/Saturation's space), Skin Tones' Lab the
  codes' own; Out Of Gamut proofs the Display P3 view (`proofSourceProfile`).
- **Stack Modes are per channel on the codes (6b-i, `stack_stat`);** an 8-bit result can land off the ×257
  grid, as paint. Load Files into Stack keeps a Lab stack, each later file through `LabLayerClip` (the Mode
  rules). **Merge to HDR goes through ProPhoto:** a Lab stack or document becomes 16-bit ProPhoto RGB (BPC,
  `LabDocument.mergeStack`) and merges to 32-bit RGB, as there is no 32-bit Lab; the source stays Lab.
- **A plane's selected pixels (6b-i) and Clear (6a-ii; Rasterize's rules, unmeasured).** Move and Transform
  Selected Pixels of Lightness, a, b and alpha run the colour-plane walk on the codes (6b-i); the vacated Lightness, a
  or b value (6a-ii) is that plane's 8-bit code of the background colour's L\*a\*b\* (Offset's rule; the host
  sends `[c, c, c]`, which `color_plane_hole` reads back as c), alpha's 0. Edit ▸ Clear is the legacy `erase` on a layer and the scalar
  fill 0 on a plane, mask or channel, so a\* or b\* = −128 on those planes, as the Lab Eraser writes there.
- **The stack tools read Y and L\* (6b-ii, `lab_stack`; Rasterize's rules, no take).** Vignette Removal and
  Geometric Distortion fit a Lab layer's CIE Y; Vignette Removal's gain multiplies the codes' XYZ (the gain any
  linear RGB would take); the reprojection and warps resample the codes, as 4a. Stack Images scores, and
  Panorama's seams take their gradient, on L\* alone; Seamless Tones fits and applies one gain per frame to L\*
  alone (`[g, 1, 1]`), a\* and b\* keep their codes. Merge Panorama is those stages over the stack 6b-i keeps Lab.
  An 8-bit result can land off the ×257 grid, as paint.
- **The selections (6b-ii).** Quick Selection's ΔE76 reads the codes' own L\*a\*b\*; Select and Mask's edge
  stage guides on the codes per channel and Decontaminate Colors solves per channel on them (Normal's linear
  mix). Select Subject and its family, Sky, Person Parts and Object Finder read the Display P3 analysis
  composite (`analysisComposite` → `LabDocument.display`). Quick Mask is colour-free coverage.
- **The neural and generative tools: the view in, the Mode rule out (6b-iii, `lab_neural`; no take).** Super
  Zoom, JPEG Artifacts Removal and Neural Colorize run their models on the layer's Display P3 view
  (`lab_display`; the host's `NeuralFilterRun.modelInput`), Generative Fill, Expand and Variations send the
  plate's view (`view_samples`, Remove's rule); each result, tagged Display P3, returns through
  `from_view_pipeline` and `LabPolicy::code` (relative, no BPC, ×257 at 8 bits), so it equals Mode ▸ Lab
  Color of the same picture on an RGB Display P3 document. Super Zoom's (and Colorize's) new document is Lab
  at the source's depth; Colorize runs in Display P3's matrix/TRC model, the twin being none. Each plan adds
  the view's and the way back's tables and the 8 B/px Lab image (`published_bytes`,
  `rz_lab_neural_scratch_bytes`). Sky Replacement refuses for good by name: Rasterize's decision (no take
  measured it in Lab).
- **Exports go through RGB in the core (1b):** `doc_export` runs the resampled composite through
  `lab_rgb::lab_to_rgb_image` (Mode ▸ RGB Color's loop: the request's intent and BPC, clipped, never refused)
  before sharpening; profile
  `document` writes sRGB. **The Lab TIFF is TIFF 6.0 CIELab (1b, `lab_save`):** Photometric 8 at the
  document's depth (8 bits L byte, a\*/b\* signed; 16 bits L u16, a\*/b\* signed 1/256), alpha only when some
  pixel is not opaque, no ICC; Save As keeps `.rz` (the TIFF is never loss-free), Save a Copy and `save_copy
  tiff` write it. **The thumbnail fits first** (`lab_thumbnail_rgba8`: fit
  at u16, then the view).

## Shipped phases

### 1 to 2f — model, structure, readouts, paint, adjustments, filters, retouch, effects (shipped)

- **1** (`d4cf7e32`): New 8/16, Mode ▸ Lab Color from RGB, Gray and CMYK and back, the view, `.rz`.
  One undo step, MCP, Actions. `lab_document.rs`, `lab_convert.rs`, `lab_native.rs`; drive
  `verify_lab_mode.py`; admission `scripts/lab-admission/measure.py` (`TO_LAB_BYTES_PER_PIXEL`). **1b**
  (`fc013245`): structure, Fill, alpha channels, the Lab TIFF, exports. `lab_structure.rs`, `lab_save.rs`;
  L13–L17; admission `export-lab-png16`, `export-lab-jpeg8`, `save-lab-tiff16`. **1c** (`740003b7`): panels
  and readouts. `lab_readouts.rs`; L18–L21.
- **2's take** (`e6c31f84`): `lab_mode_take.py`, `lab-mode-547/{smoke1,take1}` (26.10.0): encoding CONFIRMED
  (lab8 ≤ 0.753 codes, lab16 ≤ 0.017; `--judge` PASS, pinned in `lab_convert.rs`); blend O(8) = O(16), 19
  modes, the gate corrected to 13; the menu walk. L22.
- **2a/2b** (`d009826a`) `lab_paint.rs`, `lab_adjust.rs`, L23–L27; **2c** (`38f5a091`) `lab_filter.rs`,
  `lab_color_adjust.rs`, L28–L31; **2d** (`ed2f675d`) `lab_wide_gamut.rs`, `lab_targets.rs`, L32–L35; **2e**
  (`fb60e994`) `lab_retouch.rs`, `lab_effects.rs`, L36–L38; **2f-i** (`be2d0252`) `lab_channel_filter.rs`,
  `lab_blend_if.rs`, L39–L42, admission `filter-*-16` (plan about 2× the need, nit 2); **2f-ii** (`11184e3d`)
  `lab_pattern_stroke.rs`, L43–L44; **2f-iii** (`71c6b5d6`, default phase 6) `lab_tools.rs`, L45–L47.

### 3, 3a, 3b — the compositor and Lab files (shipped)

- **3** (`c47305cb`): `lab_blend_take.py`, `lab-blend-681/{smoke1,take1,take2}`: take1 INVALID, take2
  VALID after readback fix 2, every mode within one 8-bit code at 8 and 16 bits; the six joined. `lab_blend.rs`
  (`common/lab_blend_oracle.rs`, take2 in place); L48–L50.
- **3a** (`4b67b0a3`): `lab_files_take.py` take2 VALID: Lab TIFF, PSD, PSB, layered TIFF open.
  `lab_psd.rs`, `lab_tiff_open.rs`, `lab_psd_photoshop.rs`; L51–L54; admission `open-lab-tiff*`.
- **3b** (`1e20fde8`): `lab_export_take.py` take1 VALID, PASS: `export_psd`, `inspect_psd_export`, Export as
  Photoshop; the Lab TIFF open charges `lab_limits::OPEN_BYTES_PER_PIXEL`. `lab_psd_export.rs`; L54–L57.

### 4 — geometry, selections, clipboard (shipped; closed in 4c)

- **4a/4a-ii** (`06acd034`, `da5a9431`): resamples, transforms, warps, Liquify, Align, Distribute, the Move
  tool; `lab_convert::view_scratch_bytes`. MCP fill/stroke send a Lab hex as unquantized `srgb` ink.
  `lab_geometry.rs`; L58–L63.
- **4b-i** (`aa55803b`): selected pixels of a layer, mask or alpha channel, Layer Via, Stamp Selected, Save and
  Load Selection (planes Lightness, a, b), Select ▸ Modify; `LUMINANCE_REFUSAL`. `lab_selection.rs`; L64–L65.
- **4b-ii** (`9200efcd`): `lab_select_take.py` take1 VALID: sampled Color Range by C2
  (`lab_select::RANGE_RULE`), other ranges naming 6; the Magic Wand fit none of C1–C8 at 16 bits,
  Similar unmeasured (#683).
  `lab_color_select.rs`; L68.
- **4c** (`8e7fa3ff`): the clipboard, Duplicate to New Document, copying layers; the Magic Wand and Select Similar
  `AWAITING_TAKE` refusals (#683 for Similar).
  `lab_clipboard.rs` (exactly Mode ▸ RGB Color with BPC and Mode ▸ Lab Color); L70–L71; admission
  `paste-rgb-into-lab-16`, `paste-lab-into-rgb-16` PASS (plan 2548 MB, needs ≤ 1322 MB).

### 5a — Lab → CMYK directly, 8/16 Bits/Channel (shipped, `e88d8164`)

Image ▸ Mode ▸ CMYK Color from Lab and 8/16 Bits/Channel (`convert_precision` lab8/lab16), one undo step
each, over MCP and in Actions (Decisions). `lab_cmyk_depth.rs` (a)–(e); L66–L67. Admission PASS
(`to-cmyk-lab16`, `depth-lab-8`; the handoff holds the figures). No take or RZDC change.

### 5a-ii — Lab → Grayscale directly (shipped, `7be03800`)

Image ▸ Mode ▸ Grayscale from Lab, one undo step, over MCP and in Actions: each pixel's L\* alone through the
Gray curve (Decisions; `lab_gray`), an adjustment layer refused with `flatten_required`. `lab_gray_take.py`,
receipts `lab-gray-547/{smoke1,take1}` (2026-10-22, 26.10.0): VALID, C1 (L\* direct) ships, max |r| 0.267
code at 8 bits, 0.011 at 16. `lab_gray.rs` (a)–(g, take1 in place); L69; admission `to-gray-lab16` PASS (plan
986,691,964 B, need 721,080,860).

### 5b — proof and the profile commands; phase 5 complete (shipped, `5f2f3892`)

View ▸ Proof Setup, Proof Colors, Gamut Warning and `get_proof_setup`, `set_proof_setup`, `render_proof`,
`save_proof_preset` on the P3 view; Convert to Profile… and `convert_profile` as a mode conversion, one undo
step, in Actions; Assign refused (Decisions). `lab_cmyk_take.py`, receipts `lab-cmyk-547/{smoke1,take1}`
(2026-10-22, 26.10.0): VALID; the menu walk read Assign disabled, the other four enabled; Lab → CMYK
MISS. `lab_proof_profile.rs`, `lab_cmyk_photoshop.rs` (take1 in place, the MISS's record); L72–L73. No RZDC,
FFI or plan change.

### 6a-i — type, shape and fill layers, paths and vector masks (shipped, `e0d670c5`)

Type, shape and Solid Color/Gradient/Pattern fill layers, Rasterize Artwork, Layer Content Options, Artwork
Colors, the Text, Shape and Pen tools, paths and vector masks on Lab (Decisions); 4c's nits: the RGBA8 read
runs under the job's second half, and an 8-bit payload is quantized on every 8-bit destination.
`lab_artwork.rs` (a)–(g), `lab_clipboard.rs` (g)–(h); L74–L77. No take, RZDC or memory-plan change.

### 6b-i — analysis, multi-image, Color Range, the planes' selected pixels (shipped, `bf95b803`)

Histogram (`histogram`, `layer_stack_mode` families), Apply Image, Calculations, Stack Modes, Reduce Noise from
Stack, Load Files into Stack, Merge to HDR (files and layers), every Color Range range and the selected pixels
of the Lightness, a, b and alpha planes (Decisions). `lab_analysis.rs` (a)–(c), `lab_color_select.rs` (c),
`lab_selection.rs` (e); L78–L82. No take, RZDC, FFI or plan change.

### 6a-ii — comps, links, snapshots, the legacy ops, Clear, Gray profiles, a plane's vacated value (shipped, `9aa9a847`)

Layer comps (`lab_comps`), links, snapshots, the legacy `erase`/`clone`/`heal` ops and Edit ▸ Clear, Convert to
Profile into a Gray profile, and the vacated value of a Lightness, a or b plane's selected pixels
(Decisions); the legacy `rasterize` op still refuses. `lab_comps.rs` (a)–(d),
`lab_legacy_ops.rs` (e)–(f), `lab_gray.rs` (h); L83–L85. No take, RZDC, FFI or
memory-plan change.

### 6b-ii — the stack tools and the model selections (shipped, `50de4191`)

Auto-Align with Vignette Removal and Geometric Distortion, Auto-Blend (Stack Images, Panorama with Seamless
Tones), Merge Panorama, Quick Mask, Quick Selection, Select Subject and its family, Sky, Person Parts, Object
Finder and Select and Mask with Decontaminate Colors (Decisions): `lab_stack` (new kernel), the Lab arms of
`typed_linear`, `remove_vignette`, `precision_auto_blend` and `quick_samples`; `winners`/`seams` take their
luma weights. Nits: Color Range builds its twin → ProPhoto pipeline only when it runs; an unknown or
wrong-class `convert_profile` profile on Lab names the Gray choices. `lab_stack_tools.rs` (a)–(d),
`lab_model_select.rs` (e)–(g); L86–L89. No take, RZDC, FFI or memory-plan change.

### 6b-iii — the neural and generative tools (this batch)

Super Zoom, JPEG Artifacts Removal, Neural Colorize and Generative Fill, Expand and Variations on Lab; Sky
Replacement refused for good (Decisions). `lab_neural` (new; Remove's view loop routes through it), the
`published` arms of `doc_neural`, `doc_gen_fill` and `doc_gen_variations`, and the value-only FFI
`rz_lab_neural_scratch_bytes`. L87's Merge Panorama crops widened (setup only). `lab_neural.rs` (a)–(h);
L90–L92. No take, RZDC or memory-constant change.
