# Convert to Smart Object — shipped

The settled record of Convert to Smart Object's shipped phases: each sub-slice's lines and the
rulings in force, moved verbatim from the Phase plan of
[convert-to-smart-object-status.md](convert-to-smart-object-status.md), which keeps the head, the decisions,
the verdicts, the contracts still to ship and the open items.

## Phase plan, shipped items

1. **Measurement** (done).
2. **Core model and convert, 8 and 16 bits**, split in two.
   - **2a (shipped 2026-10-09).** `SourceAsset.contents: Option<Arc<SourceContents>>` (`Byte`/`Typed`, the
     parent's depth and profile; `nesting()`); `doc_smart_object.rs` (`rz_doc_convert_to_smart_object`, the
     shared rules, sentences and `contents_frame`) and `precision_smart_object.rs` (the
     `{op:"convert_to_smart_object",layers,source_id}` structure edit); leaf `smart_object_limits.rs`
     (`MAX_SMART_OBJECT_NESTING` = 16; Photoshop's limit unmeasured); `retained_source::info` reports
     `contents`; `app/Sources/RasterCore+SmartObject.swift` (no caller until 3). Cross-group selections refuse
     until their placement is measured. Tests
     `core/tests/source_smart_object.rs`. Admission (16-bit) is measured, not Merge's: 24 MP two-layer convert,
     plan ÷ max E 1.343×, no margin; floor pinned by `core/tests/smart_object_24mp.rs` (record in
     `precision_smart_object.rs`' `# Admission`).
   - **2b (shipped 2026-10-09).** `smart_object_native.rs`: conditional byte word 65 / typed word 66, the
     FIRST word inside 45/46, and the `SOCT` table, the last before `LIDS` (u32 schema 1, u32 count, then per
     entry an ascending RSRC asset index, u32 kind 1 byte / 2 typed, u64 length, one complete nested RZDC file
     by the same writer); a contents-free document writes the prior bytes. `RZDC_VERSION` 66; the 2a writer
     guard is gone. Readers parse each nested file before their own layers under a `ContentsBudget`
     (`smart_object_limits.rs`): past `MAX_SMART_OBJECT_NESTING` refuses, and every level refuses when its own,
     its contents' and the enclosing files' decoded pixels pass the one native cap (`MAX_DECODED_BYTES`); a
     32-bit contents refuses. Plans count the nested terms (`NestedPlan`; byte `native_plans`, typed
     `checked_plan`); the byte convert refuses a result past the cap. PSD preflight names the lost contents.
     Tests `core/tests/source_smart_object_native.rs`. Admission measured, no margin (24 MP, Background +
     two converted layers, 8 GiB limit; record in `smart_object_native.rs`' `# Admission`): plan ÷ max E is
     save8 3.268, reopen8 2.632, save16 1.259, reopen16 2.500 (the reopens re-measured 2026-10-10 once the
     nested phase became a max); floors pinned by `core/tests/smart_object_24mp.rs`. At the 2 GiB test
     pin the 16-bit 24 MP convert of that fixture refuses (its plan counts the contents).
3. **App, MCP, Actions (shipped 2026-10-09).** Layer ▸ Smart Objects (D1) holds Convert to Smart Object (also the
   layers panel's row menu); Filters ▸ Convert for Smart Filters asks with an alert ("Don't show again");
   Live Filters… on a plain layer offers the conversion. Each is one undo step "Convert to Smart Object", 8 and 16
   bits (`SmartObject.swift`, `EditorViewController+SmartObject.swift`). MCP `convert_to_smart_object
   {document_id?, layers?}` (default the layer selection; `AgentServer+SmartObject.swift`, its catalog entry
   carries the disclosure); the UI records `{"layers":{"$layers":"selected"}}`. The live-filter tools refuse a
   layer with no retained source by naming the conversion (`LiveFilterWork.convertFirst`). The image-source items
   moved into the submenu in 5a. Drive: `python3 scripts/verify_issue158_convert_to_smart_object.py`.
4. **Edit Contents (shipped 2026-10-09).** Layer ▸ Smart Objects ▸ Edit Contents (also the row menu, and a
   thumbnail double-click when it can open) opens the contents in a new window titled `<layer>.psb`, clean, at
   their own depth; a placed image's master opens as one layer (`SmartObjectContents.swift`). ⌘S or the close
   prompt's Save commits back through `RetainedImageWork.edit` with the `{op:"commit_contents",source_id,
   revision_id}` source edit: ONE parent step "Update Smart Objects"; every record sharing the asset id updates to one
   new asset (E1), each frame kept centred (E2; `doc_smart_object_contents.rs`, `precision_smart_object_contents.rs`,
   read exports `rz_doc_smart_object_contents` / `rz_precision_doc_smart_object_contents`). The plan rebinds
   every instance in ONE hypothetical result over a transparent stand-in, renders nothing, and is measured:
   24 MP two-instance commit, plan ÷ max E 1.599 at 8 bits, 2.006 at 16, no margin (floors in
   `core/tests/smart_object_24mp.rs`). MCP `edit_smart_object_contents`, `commit_smart_object_contents`.
   Drive: `python3 scripts/verify_issue158_edit_contents.py`. Rulings in force (orchestrator, 2026-10-09):
   - D1 parent step "Update Smart Objects", measured (float-edit-contents-600/take2) and built; the title
     `<layer>.psb`, D2 a placed image commits its composite and keeps no contents, and D6 closing the parent
     detaches an open contents window (⌘S saves it as an untitled document, the MCP commit refuses) are still to
     be measured.
   - D3 a linked placed image refuses until phase 6 (embed first); D4 RAW routes to Develop (MCP names
     `redevelop_raw`), a host-media attachment refuses; D5 32-bit refuses; D7 no instance left refuses
     and the window stays dirty; D8 Save As detaches; D10 an odd size change centres by a half-pixel translate;
     D11 a double-click opens the contents when it can, else the info sheet; D12 one Pixels/Transparency-locked
     instance refuses the whole commit, a Position-locked one when its frame moves; D13 the window may change
     depth (8↔16) or profile.
   - D9 not recorded into Actions yet (both MCP tools `.nonRecordable`; the UI commit records an
     `.unrecorded("Update Smart Objects")` placeholder).
5. **Remaining commands**, split in three.
   - **5a (shipped 2026-10-09).** Layer ▸ Smart Objects ▸ New Smart Object via Copy (E1: Duplicate Layer's
     `<name> copy` bound to a new asset `{id = revision = source_id}` sharing every allocation, so a commit to
     either never updates the other), Reset Transform (T1: the map becomes `[1,0,0,1,round(cx − w/2),
     round(cy − h/2)]` about the current centre; mask and style stay; a no-op adds no step; Position lock
     refuses) and Embed All Linked (every linked record, groups included, one step); 8, 16 and 32 bits. Source
     edits `copy_smart_object` / `reset_transform` / `embed_all` (`doc_smart_object_commands.rs`,
     `precision_smart_object_commands.rs`, `source_transform::reset`); reset plans as the transform commit, the
     other two allocate no plane and charge the metadata formula. App `SmartObjectCommands.swift`; MCP
     `new_smart_object_via_copy`, `reset_smart_object_transform`, `embed_all_linked_image_sources`, recordable.
     The six image-source items moved into the submenu, retitled (Relink to File…, Embed Linked,
     Update Modified Content, Rasterize; selectors and MCP names unchanged; Save Portable Image Copy… after
     Embed All Linked); Image Source… stays in Layer. Drive:
     `python3 scripts/verify_issue158_smart_object_commands.py`. The copy's name, the centre rule after a move,
     a fitted placed image, the mask, the history names and the menu order are defaults until measured.
   - **5b Convert to Layers (shipped 2026-10-09).** Layer ▸ Smart Objects ▸ Convert to Layers (above Rasterize) and
     MCP `convert_smart_object_to_layers`, recordable, 8 and 16 bits, ONE undo step "Convert Smart Object to Layers":
     the `{op:"convert_to_layers",layer}` source edit (`doc_smart_object_layers.rs`, whose module doc is the build
     table as built, and `precision_smart_object_layers.rs`) wraps the contents in `group_layers`, re-canvasses them
     to `source_transform::reset`'s translation and splices them with `paste_layers`. A lone layer or a placed image
     (Reset Transform's render, then Rasterize) replaces the record; contents of another depth or profile are
     converted first. Admission measured (24 MP): the splice allocates no plane (E = 0); mismatch8 plan / max E 2.27,
     mismatch16 4.16; floors in `core/tests/smart_object_24mp.rs`. Drive:
     `python3 scripts/verify_issue158_convert_to_layers.py`.
   - **5c Export Contents, Update All Modified Content (shipped 2026-10-09).** Export Contents… (after Replace
     Contents…) and MCP `export_smart_object_contents`, not recorded: writes at the exact path (X1), first
     matching row — contents as `.rz` (PSB is not written yet,
     disclosed), a RAW's or host still's retained file, a linked file still matching its link, else the cached
     master as PNG at its depth and profile; no
     undo step, dirty state untouched; 32 bits refuses. Update All Modified Content (after Update Modified
     Content) and MCP `update_all_modified_content`, recordable, 8/16/32 bits: each changed path hashed and
     decoded once, one `{op:"update_modified",source_id,revision_id,link}` source edit per `(source_id, link
     path)` chained by `RetainedImageWork.edit(steps:)` (each step planned on its intermediate, its own job
     control) into ONE step; missing/unreadable/RAW/media skipped. The core op (`doc_smart_object_update.rs`,
     `precision_smart_object_update.rs`) rebinds every instance of the source and path to one asset by Replace's
     rule (map kept, a lock on any instance refuses) through the instance steps shared with Edit Contents'
     commit; the per-record Update Modified Content now routes through it, so Duplicate Layer copies update too.
     Admission measured (24 MP): 8/16 bits E ≈ 0 (the identity render shares the master), 32 bits plan / max E
     2.00; floors in `core/tests/smart_object_24mp.rs`. Drive: `python3 scripts/verify_issue158_export_update.py`.
6. **Layered files as smart objects**, split in two (6b's contract is in the status doc).
   - **6a Layered files (shipped 2026-10-10).** Place Embedded/Linked, Replace Contents, Update Modified Content
     and Update All Modified Content of a native `.rz` or a PSD: `layered: true` on `place` / `replace` /
     `update_modified` (`doc_smart_object_place.rs`, `precision_smart_object_place.rs`) keeps the decoded file
     as the new asset's contents at its own depth (8/16) and profile, its canvas-sized composite the master; the
     op is otherwise the plain one over that asset (`SourceInput::Asset`), so depth consent, frame, link and
     instances are unchanged. App `LayeredSourceFile.swift` (claim, snapshot decode, `3·encoded + 16 MiB`): a PSD
     decodes as File ▸ Open does (Import Layers, else the stored composite as one-layer contents with the layered
     refusal and the composite note disclosed, else refused); a 32-bit file or document refuses. The
     depth prompt, the frame and the update history names stay today's until measured (a place is "Place
     Embedded/Linked Smart Object", a layered Replace "Replace Contents"). Admission measured (24 MP;
     records in both modules' `# Admission`): byte place / replace / update plan ÷ max E 1.2505 / 1.2505 /
     1.2591 with `LAYERED_MARGIN_PER_PIXEL` = 1; place16 1.384, update16 2.050; floors in
     `core/tests/smart_object_24mp.rs`. Drive: `python3 scripts/verify_issue158_layered_place.py`.
   - **6b0 take3 (shipped 2026-10-10).** `scripts/photoshop-probe/convert_to_linked.py` (job `convert_smart_object`,
     `take3: true`; the `via_copy` and `convert_to_linked` setup steps, `csoTake3`) measured Convert to Linked
     and Edit Contents of the result into `…/take3`; valid, `--judge` reproduces it. Verdicts in the probe README's
     `### take3`; the rulings on INSTANCES `one` and LL are 6b1's below.
   - **6b1 Convert to Linked (shipped 2026-10-10).** Layer ▸ Smart Objects ▸ Convert to Linked… (after Embed All
     Linked) and MCP `convert_smart_object_to_linked`, 8 and 16 bits, not recorded (Export's fixed-path
     precedent): Export Contents' rows write the path exactly (contents as `.rz`, a placed image as
     PNG), the file is hashed, and ONE `{op:"convert_to_linked",layer,source_id,link}` source edit (one step "Convert
     Smart Object to Linked")
     (`doc_smart_object_commands.rs`, `precision_smart_object_commands.rs`) re-identifies the target in place as a
     new source sharing every allocation and links it; its Duplicate Layer siblings stay embedded on the old
     source (take3 INSTANCES `one`). An already linked target is a no-op (`changed: false`), and the menu item is
     disabled for it, as for Embed All Linked when there is nothing linked. Refusals: no source, 32 bits,
     RAW or host media and an open contents window.
     The metadata formula, no new plan. Drive: `python3 scripts/verify_issue158_convert_to_linked.py`.
   - **6b2-0 take4 (shipped 2026-10-10).** `scripts/photoshop-probe/edit_linked_contents.py` (take3's job and
     helpers, one case EP: a placed PNG converted to `ep_linked.png`, duplicated, then take3's `edit_linked`)
     measured into `…/take4`; valid, no STOP row, `--judge` reproduces it. Verdicts in the probe README's `### take4`.
   - **6b2 Edit Contents of a linked smart object (shipped 2026-10-10).** Edit Contents (menu, layer
     double-click, MCP `edit_smart_object_contents` kind `"linked"`) of a linked record opens the linked file
     itself as an ordinary document (`LinkedSmartObjectContents.swift`; reused when already open), linked
     weakly to each parent it was opened from. A successful in-place Save of that window (⌘S, the close
     prompt, MCP `commit_smart_object_contents`) runs `SmartObjectFiles.updateAllModified` on each live parent
     limited to the link path (`onlyPath`), as ONE step "Update Smart Objects", recorded as a placeholder
     (Edit Contents' precedent); a former duplicate (embedded) does not update. Save As elsewhere detaches the window
     (D8). Phase 4's contents-window path is unchanged. Refusals: a missing file, and a PSD, >8 bits or a
     format outside `ImageDocument.inPlaceSaveTypeIdentifiers`;
     unmeasured cases default. No Rust change,
     no new plan (File ▸ Open's admission and 5c's `update_modified`). Drive:
     `python3 scripts/verify_issue158_linked_edit_contents.py`.
- **Float phase 2a, Convert to Smart Object at 32 bits (shipped 2026-10-10).** In a float document the contents are
  a 32-bit float document and the record keeps a `SourcePixels::Rgba32F` master (D32: `same`, `exact`); menu items
  and MCP `.floatTool`. `.rz`: typed word 68 first inside 46 (`46 → 68 → 67 → 66`), `SOCT` kind 3, `RZDC_VERSION`
  68; files without float contents unchanged. Admission at 24 MP (A / max E): convert 1.785, save 2.000, reopen
  2.616, no margin; floors in `core/tests/float_master_24mp.rs`. Tests `core/tests/float_smart_object.rs`; 11F F23.
- **Float phase 2b, Edit Contents and its commit at 32 bits (shipped 2026-10-10).** Take `float-edit-contents-600/take2`
  (valid, no STOP; take1 invalid by a probe build defect). Float contents read as they are; an f32 master opens as a
  one-layer float document under the parent's tone map; a float input commits only into a float document, as an f32
  master; 8/16-bit inputs at their own depth. Menu, row menu, double-click and both MCP tools `.floatTool`. Admission
  at 24 MP (A / max E): contents 3.017, master 3.017, 16-bit 2.201, two instances 3.575, no margin; floors in
  `core/tests/float_master_24mp.rs`. Tests `core/tests/float_smart_object_contents.rs`; 11F F24.
- **7a-0, takes 5 and 6 (shipped 2026-10-10).** `scripts/photoshop-probe/stack_mode.py` (take5) and
  `stack_mode_take6.py` (take6, fresh stimuli, hypotheses fixed in its docstring) on take5's `convert_smart_object`
  job (`take5` steps, `csoStackMode`, isolated alpha TIFFs); None is `imageStackConvertSmartObject`. Verdicts in
  the status doc.
- **7a, Stack Mode in the core (shipped 2026-10-10).** `{op:"set_stack_mode",layer,revision_id,mode}` on
  `rz_doc_mixed_source_edit` / `rz_precision_doc_mixed_source_edit` (`doc_smart_object_stack.rs`,
  `precision_smart_object_stack.rs`, kernel `stack_mode.rs`). The mode is a field of the RECORD
  (`RetainedSource8/16::stack_mode`, inside its tail padding, so no plan moves); the contents stay shared and a
  moded record's asset is derived (new revision, the mode's render as pixels; a second record of the same
  contents and mode reuses it). Rebinding (Replace, Update Modified, RAW) resets to None; Edit Contents'
  commit re-applies each instance's mode, one asset per mode, revision `<R>-<mode>`. Convert to Layers no
  longer counts two records of one contents allocation as an id clash. `.rz`: byte word 69 / typed word 70,
  first inside 45/46, and a `SOSM` table after `SOCT`; `RZDC_VERSION` 70; files without a mode unchanged.
  Under 69/70 an asset sharing an earlier asset's contents allocation is a `SOCT` alias (kind 4, no bytes), so
  reopen keeps the instances sharing one contents (reuse and Convert to Layers behave as before the save).
  Admission at 24 MP (scratch / max E): stack8 1.374, stack16 1.750, commit16m 2.416, no margin; floors in
  `core/tests/smart_object_24mp.rs`. Tests `core/tests/source_smart_object_stack.rs`.
- **7b, Stack Mode in the app (shipped 2026-10-10).** Layer ▸ Smart Objects ▸ Stack Mode ▸ None, a separator, then
  the eleven modes, after Convert to Layers; the item's tag is the core code, the checkmark the active record's
  `contents.stack_mode`; enabled on a record with contents at 8 and 16 bits, the six encoded modes disabled over
  16-bit contents. One `set_stack_mode` source edit through `RetainedImageWork.edit` (`SmartObjectStackMode.swift`),
  one step named "Rendering Image Stack <Title>" or "Stack Mode: None"; the current mode adds none. MCP
  `set_smart_object_stack_mode` (`.floatTool`, the core's refusals in band), recorded with `mode` only. Drive
  `scripts/verify_issue158_stack_mode.py` against take6. The core now renders a one-member stack's Range and
  dispersion modes fully transparent, as take6's K1 TIFFs read (`stack_mode.rs`, both depths).
- **Float phase 2c, Convert to Layers, Export Contents and Convert to Linked at 32 bits (shipped 2026-10-10).** Take
  `float-smart-object-files-600/take1` (valid, no STOP). Float contents splice as they are into a Normal group; 8/16-bit
  contents promote through `layer_clipboard_float` (a reset mode lands Normal, MCP `blend_reset`); the float splice runs
  on the document with the record's binding dropped (the decoded cap). An f32 master exports and links as a 32-bit
  float TIFF (`master_tiff32`). Admission at 24 MP (A / max E): 11,942 (15,923 with the record moved), 39,255, 6.042, 9.037, no margin; floors
  in `core/tests/float_master_24mp.rs`. Tests `core/tests/float_smart_object_layers.rs`; 11F F25.
- **Float phase 2d, a layered file placed, replaced, updated and relinked at 32 bits (shipped 2026-10-10).** Take
  `float-layered-place-600/take1` (valid, no STOP). In a float document a layered `.rz`/PSD keeps its contents at its own
  depth (8/16/32) under a master in linear light (`kept_contents`' `float_parent`, `LayeredSourceFile.decode(floatParent:)`);
  Replace, Update Modified/All, Relink and linked Edit Contents of a layered `.rz` work there. Admission (A / max E):
  2.558, 1.822, 1.601, 2.561, 3.613, no margin; floors and the 6000×4000 float refusal in `core/tests/float_master_24mp.rs`.
  Tests `core/tests/float_smart_object_place.rs`; 11F F26; 158 drive F1 inverted.
- **8a, PSD smart objects read at 8 bits (shipped 2026-10-10).** Import Layers of an 8-bit PSD carries a record whose
  `SoLd`/`SoLE` `Idnt` names an `liFD` entry of the file's `lnk2`/`lnkD`/`lnk3` table holding an 8-bit PSD or PSB
  (`psb_transcode` re-frames it) as a smart object: contents imported layered at depth + 1 (or the stored composite
  as one layer, a loss), master its composite under the `Idnt`, transform from `nonAffineTransform` (Affine for a
  parallelogram, else Projective), records of one `Idnt` sharing one asset, `Impr`'s class as the record's mode (a
  derived asset per mode, 7a's rule), Photoshop's saved pixels as the fallback padded to the transform's bounds.
  `Type` 2 and 3 (KP's image stack) carry. Every other record keeps its pixels and its marker loss, with one of 22
  named clauses (`psd_smart_object_import::resolve`, plus a nested open that fails); the report builds what the
  open builds, so the two agree, and prefixes the contents' own losses. Admission at 24 MP (forecast / max E): A
  1.2558, B (Median) 1.2543, `MASTER_BYTES_PER_PIXEL` 33; floors in `core/tests/smart_object_24mp.rs`. Tests
  `core/tests/psd_smart_objects.rs` (P8, E1, KP and hand-built files). No `.rz` change.
- **Float phase 3a, a 32-bit float master in 8- and 16-bit documents (shipped 2026-10-10).** Takes `float-low-depth-600/take1`
  (valid; its SC16 STOP came from the rule) and `take2` (SC16 alone, `H-enc`). An f32 master places, replaces and refreshes
  there under `convert_fallback`, rendered as the takes measured: converted to the document profile, clipped, encoded
  (`float_master_codes`, `encoded_u16`; a transform resamples that encoded render, SC16); Edit Contents opens a float window under Clip and commits an f32 master; narrowing
  and the narrowed layers paste carry it; `.rz` word 67 at 16 bits, new byte word 72. Admission (A / max E) 1.258–4.059,
  no change; floors in `core/tests/float_master_24mp.rs`. Tests `core/tests/float_master_low_depth.rs`; 11F F27.
- **Float phase 3b-1, 32-bit float contents in 8- and 16-bit documents (shipped 2026-10-10).** Take
  `float-contents-low-depth-600/take1` (valid, no STOP row). Layered place, Replace, refresh, Update All and Edit Contents
  with its commit (linked too) keep float contents at every depth under `convert_fallback`, the master their linear composite
  (NRO16), rendered by 3a's clip; Export Contents is unchanged (`.rz`); narrowing, promotion
  and the layers paste carry them; `.rz` word 68 at 16 bits, new byte word 73 (`RZDC_VERSION` 73); Convert to Layers of
  them at 8/16 refuses (`FLOAT_CONTENTS_LAYERS`, 3b-2). The float→8 narrowing plan was under-charged (0.758× a bare
  24 MP document's peak): it now charges its two tiling copies. Admission (A / max E) 1.486–2.976; floors in
  `core/tests/float_master_24mp.rs`. Tests `core/tests/float_contents_low_depth.rs`; 11F F29.
- **Float phase 3b-2, Convert to Layers of 32-bit float contents in 8- and 16-bit documents (shipped 2026-10-10).** Take
  `float-contents-layers-600/take2` (valid, no STOP row; take1's CLL8 DELTA STOP was Photoshop's 8-bit dither, RULING-1).
  Each member converts at 32 bits into the document's profile (`float_contents_landing`, the layers paste's f32 kernel), then
  narrows under Clip (8 bits undithered, Convert to 8 Bits' default), as the Normal group. No `.rz` change. Admission
  (A / max E) 4.07–6.82, no change; floors in `core/tests/float_master_24mp.rs`. Tests `float_contents_low_depth.rs`; 11F F29 (g).
- **8b-1, PSD smart objects written at 8 bits (shipped 2026-10-10, commit `077b2c9`, merged with 8b-2).** In the
  layered mode, a record whose effective source has 8-bit contents is written as Photoshop's own:
  `PlLd` and `SoLd` after `lclr` (P8's eighteen keys in P8's order; `Type` 3 with `Nm  `/`evid`/`Impr` for a moded
  record, KP's), its pixels, mask and style as before, and its contents as a classic PSD from the same writer one
  level down, in one `lnk2` `liFD` v7 `8BPS` entry per instance group (`same_contents`). Every other smart object is
  pixels only with one of 11 named clauses (`psd_smart_object_export`: link, live filters, warp, baked style,
  unwritable corners, other-depth contents, a reused id, contents that refuse or pass 32,767 records, file size,
  working memory, 8a's reopen admission or the outer file's own layered reopen); the report and the save run one `export_plan`, so they agree. Re-import
  through 8a gives back the same id, transform, instances, modes, nesting and contents. Admission at 24 MP
  (forecast / max E): A 2.2286, B (nesting 2) 2.6936, `CONTENTS_EXPORT_BYTES_PER_PIXEL` 0; floors in
  `core/tests/smart_object_24mp.rs`. Tests `core/tests/psd_smart_object_export.rs`. No `.rz` change.
- **8b-2, take7 (shipped 2026-10-10, commit `e8fa3fdf`).** Photoshop opened the six `rz7_` files as written and
  authored take7c's corpus (runner `psd_smart_object_write.py`); verdicts, writer choices, Photoshop's own form and
  8a's reads of the corpus: the probe README's `### take7`. Reader fix: `linked_blocks` also reads `lnkE`.
- **8c-1, PSD smart objects read at 16 bits (shipped 2026-10-10, commit `9216282e`).** Import Layers carries a record
  whose `liFD` holds an 8- or 16-bit PSD or PSB in an 8- or 16-bit file: a 16-bit file opens typed with
  `RetainedSource16` bindings (`psd_import16::open16_within`: resolve, admit the outer with the carried files'
  held bytes, build, pad the fallback to the box with `boxed16`, bind), 16-bit contents open through
  `open16_within` as typed contents under an `Rgba16` master, in either file. `psb_transcode` re-frames an `Lr16`/
  `Lr32` tail body as layer information (C16 keeps its records there). Clause 15 (other depths) is retired; an
  encoded Stack Mode over 16-bit contents declines with `encoded_16`. Admission (`psd_smart_object_admit`,
  new): per-pixel terms × the contents' sample bytes, a native-cap sum of what the document holds (`held`), the
  16-bit forecast adding `held` to the outer term. At 24 MP (forecast / max E): A16 2.5913, B16 (Median) 2.4050,
  M8 2.2518, N16 1.5179, H16 (four contents) 2.6539, `MASTER_BYTES_PER_PIXEL` 33 unchanged; floors in
  `core/tests/smart_object_24mp.rs`. Tests `core/tests/psd_smart_objects16.rs` (C16 against `t7_C16_saved.tif`
  and the new `t7_C16_contents.tif`, hand-built 16-bit PSD/PSB). No `.rz` change.
- **8c-2a, PSD smart objects written at 8 and 16 bits (2026-10-10, commit `8bd5cf47`, merged with 8c-2b).** The layered
  export of an 8- or 16-bit document writes a record whose effective source has 8- or 16-bit contents as `PlLd`/`SoLd`
  (after `lclr`, as at 8) plus one `lnk2` `liFD` `8BPS` entry per instance group holding the contents as a classic PSD
  at their OWN depth (a typed contents over its own render); clause 6 now refuses only 32-bit contents. One plan for
  both depths (`plan_smart` over `SmartCandidate`s; the typed side in `psd_export16_plan`, entries in the
  `entries` order); per-pixel terms × the contents' sample bytes, a typed contents' render scratch in `mem_k`, 8c-1's
  native-cap sum in clause 11, and at a 16-bit outer the held file terms in the grown reopen. The typed save admits
  `smart.contents` on top of today's figure. Admission at 24 MP (forecast / max E): X16 2.0988, M8 (16-bit contents
  in 8) 2.8302, N16 (8 in 16) 2.0614, `CONTENTS_EXPORT_BYTES_PER_PIXEL` 0 unchanged; floors in
  `core/tests/smart_object_export_24mp.rs`. Tests `core/tests/psd_smart_object_export16.rs` (re-import through 8c-1;
  clauses 5 and 7 cannot be built at 16 bits: promotion refuses them). No `.rz` change.
- **8c-2b, take8 (shipped 2026-10-10, the commit after 8c-2a's).** Photoshop opened the six `rz8_` files at their
  depth as written (control take7c's C16, no contradiction), so the writer stands; verdicts and the writer choices
  they confirm: the probe README's `### take8` (`--set write16`, Part 3 of the runner's docstring).
- **Float phase 3c-1, a PSD smart object's 32-bit contents read in 8- and 16-bit files (2026-10-10, commit pending).** No
  new take (`float-contents-low-depth-600/take1` SRC16 / SRC8 and Photoshop's own bytes). Import Layers carries an
  embedded 32-bit PSD/PSB as float contents under an `Rgba32F` master (`psd_import32`: `Lr32` records, f32 planes Raw, RLE
  or ZIP with the byte-plane prediction) when every layer is a plain raster or folder in a blend a float document keeps;
  else its stored composite as one layer, and only where image resource 1057 says it is a rendering (at every depth:
  8a / 8c-1 no longer carry Photoshop's blank composite); `Mt32` merged transparency and any Stack Mode decline. Fixtures
  `f3_*` (probe README). Admission (forecast / max E): F16 2.8294, F8 2.4089, `MASTER_BYTES_PER_PIXEL` 33 unchanged;
  floors in `core/tests/float_master_24mp.rs`. Tests `core/tests/psd_smart_objects32.rs`. No `.rz` change.
- **Float phase 3c-2, 32-bit contents written in 8- and 16-bit files (2026-10-10, commit pending).** The layered export
  writes float contents as a v8 `8BPB` `liFD` entry holding a depth-32 PSB (`psb_export_encode`, `psd_export32`):
  Photoshop's 110-byte HDR toning colour-mode data, `Lr32` records, Raw f32 planes, a blank composite; a mask, style
  or adjustment inside is pixels only (clause 8), a non-Clip tone map a reported loss. take9-3 valid (take9/9-2 STOPs; a
  diagnostic found the colour-mode data). Read: nested smart objects in 32-bit contents; a ZIP or `Mt32` (white-matted)
  stored composite (`f9_*`). Admission (forecast / max E): XF16 2.4072, XF8 2.1044, NF16 2.5235, CF16 2.0486, none
  raised; floors in `float_master_24mp.rs`. Tests `psd_smart_object_export32.rs`, `psd_smart_objects32.rs`.
- **Float phase 3c-3, masks, effects and adjustments in 32-bit contents (2026-10-10, commit pending).** Read
  (`psd_import32_layers`) and written in the PSB: f32 masks, `lfx2` / `iOpa` / Blend If, adjustment layers, as
  `float-contents-kinds-600` take1 measured Photoshop store them (renders within 2e-3); fill layers and an 11th folder
  refuse by name. take2 opened every `rzk_` file; Curves needed Photoshop's `Crv ` extension (take2-2). Near matches:
  take1 COLOUR 7.6e-6 code, take2 Photo Filter 0.102 code. Admission (forecast / max E): IK16 2.8041, XK16 2.1925, XK8
  1.8964, none raised; floors in `float_master_24mp.rs`. Tests `psd_smart_objects32_kinds.rs`, `…export32_kinds.rs`.

## Float low-depth take (float phase 3a)

`python3 scripts/photoshop-probe/float_low_depth.py --judge ~/devel/rasterize-fixtures/photoshop/float-low-depth-600/take1`
(`reproduced: true`; SC16 the same with `…/take2`): valid, no leftover; take1's one STOP row (SC16 DOMAIN `none`, from its
whole-row rule) is superseded by take2. Verdicts: the probe README's `### float low depth`.

## Float contents low-depth take (float phase 3b)

`python3 scripts/photoshop-probe/float_contents_low_depth.py --judge ~/devel/rasterize-fixtures/photoshop/float-contents-low-depth-600/take1`
(`reproduced: true`): valid, no leftover, no STOP row. Verdicts: the probe README's `### float contents low depth`.
