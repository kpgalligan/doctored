# High-depth clipboard — shipped runs

The settled records of the 12G parts, moved verbatim from
[phase-12g-clipboard-status.md](phase-12g-clipboard-status.md)'s open items when Part D took that doc past its cap.
The decisions they built are that doc's Decisions.

> **Removed 2026-10-10:** smart objects, live filters and non-RAW retained sources (placed images, host
> media, Live Photo frames) are gone for v1; camera RAW layers stay. Where this record describes them, that
> code no longer exists: [removal ADR](convert-to-smart-object-status.md).

## Parts
- `PART A DONE` — Paste converts into the destination, measured against Photoshop (Decisions above: Conversion,
  Ask When Pasting, External images; `82bec04`; 12G gates g2, g8, g12, g13 rewritten, g17 added). **Evidence:** a
  Photoshop 26.10.0 drive, `scripts/photoshop-probe/paste_depth.py` (2026-10-05). Verdicts: (a) 8-bit sRGB → 32-bit
  `linear`; (b) 16-bit sRGB → 32-bit `linear`; (c) 8 → 16 `exact` (v·257);
  (d) 16 → 8 `narrowed`; (e) 16-bit sRGB → 16-bit Display P3 `converted`, the silent Convert
  default; (f) an untagged 8-bit TIFF into 16-bit Display P3 `destination_assumed`: Doctored reads an untagged
  external image in the destination's profile, and the take measured the same; (g) Paste in Place of that TIFF with no selection `centered`. Cases
  f and g need Photoshop's own clipboard purged and Photoshop activated once, the only moment it reads the system
  clipboard. **Maintainer decision:** Don't Convert of a layers clip holding placed images or live filters is
  omitted, with its reason sentence (row 2 below).
- `PART B DONE` — a recorded Paste replays the live clipboard, and Copy and Copy Merged are recorded
  steps (Decisions above: Recording; `22852ab`; 12G gate g18 added; g10, g13, g14, g16 and g17 keep the
  saved-payload step through `record_payload`). **Evidence:** `scripts/photoshop-probe/paste_action.py`, job
  `paste_action` (Photoshop 26.10.0, 2026-10-05), on the descriptors `executeAction` returned:
  1. `carries_position` false — no paste descriptor holds a position, so live steps carry no `x`/`y`.
  2. `carries_pixels` false — one unchanged `past` descriptor pastes whatever was copied.
  3. `kinds_distinct` — Paste, Paste in Place (`inPlace: true`), Paste Into (`PstI`) and Paste Outside (`PstO`)
     differ, so each kind records its own placement.
  4. `layers_event` — a layers paste returns an empty `past {}`, differing only by lacking `antiAlias` and `as`.
  5. `empty_clipboard` — Paste is "not currently available"; Doctored's step fails with "The clipboard holds no
     image."
  6. `copy_event` — `copy` returns `copyHint` "pixels" or "layers" and `CpyM` returns `{}`: no pixels, no
     references.

  `hint_replay`: those keys do not bind a replay. Photoshop's Copy step re-decides pixels or layers at replay, and
  its Paste step pastes what the clipboard holds. **Maintainer decisions:** (1) `antiAlias` and `as` apply to
  vector clipboard content Doctored does not have; not built. (2) When a recorded command is unavailable,
  Doctored's player records the failure and stops with no prompt; the interactive Play's Stop / Continue prompt
  came later ([phase-12b-actions-status.md](phase-12b-actions-status.md), **Interactive Play prompts**).
  (3) `copy {mode: "auto"}` re-decides at replay and stores no hint. (4) UI paste
  steps carry no `mode`: a recorded mode would refuse or flatten instead of pasting what the clipboard holds.
  (5) Only MCP `record_payload` / `payload_path` records a saved-payload step. (6) Paste in Place across
  documents of different sizes keeps the copy's centre at its fraction of the canvas, not the source offset
  (`relative_in_place`, built in Part D; the take measured (8, 8) in 64 × 48 landing at (42, 42) in 200 × 150).
- `PART C1: ROW 1 DONE, ROW 2 STOPPED` — copied layers paste into a 32-bit float document (Decisions
  above: Layer clipboard; `fe895fd`; 12G gate g19 added). **Evidence:** `scripts/photoshop-probe/paste_layers.py`,
  job `paste_layers` (Photoshop 26.10.0, 2026-10-05; report
  `doctored-fixtures/photoshop/paste-layers-470c/take1/measured.json`). What it measured:
  - 8- and 16-bit sRGB layers land in a 32-bit document in linear light, groups kept. A 32-bit Display P3 layer is
    converted by the linear matrix, and out-of-gamut values are not clipped.
  - Twelve blend modes land Normal (color_burn, linear_burn, screen, color_dodge, overlay, soft_light, hard_light,
    vivid_light, linear_light, pin_light, hard_mix, exclusion); the other fifteen are kept. Since v1 float editing, linear_burn,
    linear_light and pin_light are kept too (`FLOAT_RESET_MODES` holds the nine).
  - Photoshop keeps a drop-shadow style, a text layer, a smart object, a layer mask, a Pass Through group and Blend
    If at 32 bits, and pastes and creates levels, curves, color_balance, hue_saturation, photo_filter, exposure,
    black_and_white and channel_mixer. Its own paste of invert, threshold, posterize and vibrance fails.
  - Admission: `paste_layers_allocation` is 2.02× the measured peak of a 24 MP 16-bit layer pasted into a 24 MP
    float document, with `LAYER_PROMOTE_MARGIN_PER_PIXEL` = 0 (record in `../core/src/layer_clipboard_float.rs`);
    10 since v1 float editing, fitted by the conversion into float, which shares it.

  **Float gap** (what Doctored does where the float mode lacks a feature).
  - Lands as pixels (`rasterized`): a live-filtered or stale placed image, warp binding; a text or shape description
    only under Don't Convert from another profile. Text and shape layers land described since 11F phase 187b, their
    numbers read in linear light.
  - A placed image with no live filter lands placed since 11F slice 5: Convert re-renders it from its master into
    linear light, Don't Convert keeps the binding over its numbers.
  - A Pass Through group lands Pass Through whatever its children since 11F slice 4 (`isolated_groups` is gone
    from the report).
  - Lands Normal (`blend_reset`): the twelve modes above.
  - Refuses: `a layer style or Blend If` (neither is baked, because both read the layers below); `the {mode} blend
    mode` for dissolve, darker_color, linear_dodge, lighter_color, subtract, divide, hue, saturation, color and
    luminosity; `the {op} adjustment` for every adjustment the float mode cannot evaluate (since 11F slice 3 Levels,
    `bcs`, Black & White without Tint, Channel Mixer and Photo Filter land beside Exposure and White Balance under
    `document_v2`, and point-form Curves since slice 3d).

  A record with a placed image was promoted under Convert even when the paste was Don't Convert; Part C1b changed
  that (decision 6 below). The Paste Profile Mismatch dialog itself was not driven (a drive may not write the
  preference).
  **ROW STOP (row 2): Don't Convert keeping placed images and live filters at 8 and 16 bits is NOT built.**
  Measured (`assign_smart_object`): Photoshop's layers paste keeps a smart object's numbers and does not re-render
  it; it re-renders a smart object only when the DOCUMENT's profile is assigned. So no re-rendering Assign was
  built. Part C1b below built row 2 from that evidence (a relabel, not an Assign) and fixed the two Convert defects.
- `PART C1b: DONE` — copied placed-image and live-filter
  layers pasted across profiles at 8 and 16 bits (Decisions above: Layer clipboard; 12G gate g20 added, g17(f)
  rewritten). **Evidence:** `paste_layers.py --cases convert_p3_8,convert_p3_16,convert_srgb_16`, kind
  `convert_smart_object` (Photoshop 26.10.0, 2026-10-08, launched and quit by the session; report
  `doctored-fixtures/photoshop/paste-layers-470c1b/take1/measured.json`). Overall verdict `expected`:
  - `convert_p3_8` and `convert_p3_16`: plain, smart and filtered all `converted` (plain within 0.19 / 0.006 of
    P3 → Adobe RGB of the source numbers; smart objects within 0.11–0.13 / 0.006; numbers moved by about 27).
  - `convert_srgb_16` (source converted to sRGB first, contents still Display P3): plain `converted` (0.006);
    smart and filtered `rerendered` (0.007 from P3 → Adobe RGB of the authored contents, 9.9 from converting the
    cached sRGB numbers). The smart filter survives every paste (observation).
  - Branch `expected` was built (no detach/keep exports): Convert converts plain layers and re-renders placed
    images from their contents, filters kept; the take read the same from Photoshop's default Convert.

  **What shipped:** `DrDocument::paste_layers` takes `PasteProfile`, and at 8 and 16 bits Preserve with differing
  profile bytes RELABELS the clip (`dr_doc_paste_layers` gains `preserve`): the Assign check of the clip against the
  destination (`assign_profile_refusal` / `color_plan(_, false)`) refuses in its own words, then every record keeps
  its samples and is re-pointed at the destination's profile; a retained source effective under the clip's profile
  stays bound (byte `rendered_profile`, typed `rendered.with_profile`), one that is not is dropped. Convert with
  differing bytes keeps the profile refusal (the host converts first). `LayerPastePlan.preserveRefusal` is narrowed
  to that Assign check, so Don't Convert keeps placed images and live filters. Host Convert: at 8 bits a
  live-filtered clip goes through `LiveFilterProfileWork.run` (defect 1: it threw the live-filter route sentence); a
  conversion answering nil hands the clip to the core's relabel (defect 2: two spellings of one model threw "could
  not be converted"). **Decision 6:** into float, a placed record promotes under the paste's own mode
  (`record_mode` deleted), so Don't Convert keeps its numbers. **Admission:** no new charge — the relabel shares
  samples under the byte capacity check and `paste_layers_allocation`; the 8-bit live-filter conversion admits
  `LiveFilterProfileWork`'s measured plan; decision 6 changes no `promotion_terms` figure for u16 planes.
  **Maintainer decisions:** Photoshop re-renders a smart object on a DOCUMENT Assign Profile (C1's observation)
  while Doctored's Assign refuses or rasterizes a placed image; not built. The Paste Profile Mismatch dialog itself
  was not driven.
- `PART C2: DONE` — group Copy / Copy Merged / Cut, Cut
  with no selection, Paste Into / Outside and New from Clipboard with copied layers (Decisions above: Layer
  clipboard; 12G gate g21 added). **Evidence:** `paste_layers.py`, 13 C2 cases (Photoshop 26.10.0, 2026-10-08,
  launched and quit by the session; receipt `doctored-fixtures/photoshop/paste-layers-470c2/take3/measured.json`,
  one job per case; take1 and take2 read the same branches; the seven copy cases re-driven in `take4`). Verdicts →
  branch:
  - G/H `drop`: every `group_copy` copy succeeds (`copyHint` pixels, no history step) and REPLACES the clipboard
    (take4 seeds it with a (37,150,211) sentinel first, which no paste reads) with opaque white over the whole
    selection, over black and over grey alike, whatever the group's properties: not the group's content. take4
    targets the group's composite channel; take1–3 left `group_mask`'s mask channel targeted, which is why their
    rows 8–15 read black (the mask), not a group rule. Opacity, mask, style, hidden and pass_through read `other`;
    blend_if `raw` only because its decisive patch is white.
  - M `none` (`copy_merged_group` `composite`); C `none` (`cut_group_selection` `refused`: "…the pixels in a group
    cannot be edited without merging the group."); L `none` (both `cut_layers` `refused`: "The command “Cut” is not
    currently available." — Doctored's Cut also needs a selection).
  - P `refuse` (both `refused`: "The command “Paste Into” / “Paste Outside” is not currently available."): built as
    above.
  - N `build_content_box`: the "Clipboard" preset is 46 × 26 (the content box), 16 bits, Display P3, 300 ppi —
    every attribute the source's — with both layers by name over a white Background: built as above.

  **Maintainer decisions, not built:** a Copy with a group targeted keeps the group's raw
  projection, at every depth and hidden or not, where Photoshop pastes opaque white over the selection.
- `PART C3: DONE` — comps, alpha channels, guides, samplers,
  global light and linked placed images with copied layers (Decisions above: Layer clipboard; 12G gate g22 added;
  `../core/tests/layer_clipboard_chrome.rs`). **Evidence:** `paste_layers.py`, seven C3 cases over a sentinel-seeded
  clipboard (Photoshop 26.10.0, 2026-10-08, launched and quit by the session; receipt
  `doctored-fixtures/photoshop/paste-layers-470c3/take1/measured.json`, `c3`). Verdict against its rule → branch:
  - channels `dropped`, guides `dropped`, samplers `dropped`, in place and centred (offset (16, 8)) → `none`.
  - comps: names `kept`, but applying `Dest` hides both pasted layers (`comps_unknown` `hidden`), and so do the fresh
    `One` and the duplicate's `One` / `Two` → `phase2`.
  - light `adopts_destination` (the shadow on the 150° side), destination angle `unchanged` (the altitude does not
    read back), `PG_local` `kept`, `equal_flag` and `same_doc_flag` `flag_kept` → `none`.
  - linked: present `link_kept` (same path); missing (source `linkMissing` true) `link_kept_missing`, the cached
    pixels landed → `none`.

  Nothing behavioural was built: today's paste already does every `none` point, and the test and g22 pin it.
  Applying a comp hides a pasted layer it does not know (`comps_unknown` above): built.
  **Maintainer decisions, not built:** a linked placed image pasted into float lands as pixels (C1), unmeasured
  in Photoshop.
- `PART D: DONE` — paste placement and deselect
  (Decisions in the status doc: Placement, Recording, Layer clipboard; 12G gate g23 added; g16, g18(b), g18's layers
  replays and the layers-into-float helper rewritten). **Evidence:** `scripts/photoshop-probe/paste_placement.py`, job
  `paste_placement` (Photoshop 26.10.0, 2026-10-08, launched and quit by the session; one job per case, dialogs off,
  each copy over a sentinel-seeded clipboard; receipt
  `doctored-fixtures/photoshop/paste-placement-470d/take2/measured.json`, `d`; `explore1` / `explore2` decoded the
  view: `zoom` is a ratio, `viewTransform` maps view points to canvas pixels, `center` is in view points). Verdict
  against its rule → branch:
  - V `always_centred_without_selection` (no visibility case kept its place: whole, 60 %, 40 %, 10 %, none, a
    60 % × 60 % corner, on 4000 × 3000 at 100 %), X `always_centred_without_selection` (neither the same document, a
    same-size nor a larger one; take1 read both `other`, the map gained the reading), L `always_centred` → ⌘V never
    keeps the source position, pixels or layers (`PasteContext.landing`). `b86891c`'s rule 2 is gone. XS
    `kept_by_selection` (`x_same_doc_selected`: ⌘C, then ⌘V with the marquee still active, landed on its own spot
    (8, 8) and deselected) → none: rule 1 centres on the copy's own marquee, which Copy leaves active (g23).
  - F `visible_centre`, O `selection_first`, S `centred` → none: (2100, 1650) on a view centred on (2200, 1700),
    (24, 16) in 64 × 48, (92, 67) in 200 × 150, (112, 72) on the selection — Doctored lands the same pixels (g23).
  - R `unconfirmed (view not settable)`: every scripted view rotation is refused to scripting (26.10.0);
    with ⌘V always centred only the centre is open.
  - DV `deselects`, DP `deselects` → every pixel paste made with a selection consumes it in its one step (Reselect
    restores it); DL `keeps`, and DLP `keeps` (the orchestrator's added case, ⇧⌘V of layers over a selection) → none.
  - P `relative_floor` (the one of seven candidates matching all eight sizes), PV `view_independent`, PS_other and
    PS_same `ignored`, PL `relative`, PLS `ignored` → ⇧⌘V across canvas sizes lands `relative_in_place`: (8, 8) 16 × 16
    of 64 × 48 → (42, 42) in 200 × 150, (17, 25) in 100 × 100, (9, 9) in 70 × 51, (9, 8) in 71 × 50, (10, 8) in
    74 × 48, (2, 2) in 40 × 30; 15 × 13 at (7, 9) → (37, 41); copied layers' union (4, 4) → (61, 40).

  Item 3 needed no case: Part B measured that a recorded Paste carries no position. Item 7: MCP `paste` takes `view`;
  a saved-payload `at` step remaps under relative and anchor geometry. **Maintainer decisions, not built:** the paste
  centre under a rotated view; the saved-payload
  geometry rule is Doctored's own, since Photoshop records no paste position; transparent interiors, non-RGB and
  deep documents and Quick Mask are unmeasured; a payload file carries no
  source canvas, so its in-place paste keeps its coordinates.
