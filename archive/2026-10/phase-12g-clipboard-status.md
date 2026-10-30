# High-depth clipboard (ADR)

- **Status:** Accepted — open items remain
- **Phase / dates:** 12G (carryover P7-29), runs 12G-1 … 12G-3, 2026-09-22 → 2026-09-23
- **Original:** [archive/phase-12g-clipboard-status.md](../archive/phase-12g-clipboard-status.md) — full history, measurements, receipts
- **Key commits:** `e507cd6` (12G-1), `b509766` (12G-2), `5e06b0a` (12G-3)

## Context
The system pasteboard carries 8-bit images, so Cut/Copy/Paste at 16 bits or 32-bit float would silently narrow
samples, lose profile, offset and selection coverage, and cross depth/profile boundaries without the user choosing.
12G added a private typed interchange format alongside the public rendering, explicit arms for every lossy boundary,
recordable Paste, and editable (text/shape/adjustment) payloads.

## Decisions
- **RZCB private payload** (`core/src/precision_clipboard_codec.rs`; operations in `core/src/precision_clipboard.rs`).
  One canvas window of RAW samples in the source storage (RGBA16 UNORM or RGBA32 float) and profile, the source canvas
  offset, PPI and optional u16 coverage. The public TIFF/PNG beside it is only a rendering (float through the document's
  tone map). Encoder writes **v3**; v1 and v2 still decode.
  - v1 layout (LE): `"RZCB"`, u32 version, u32 storage (1 u16, 2 f32), u32 w, h, i32 ox, oy, f32 ppi x/y, u32 ICC length +
    bytes, u8 flags, `w·h·4` samples, `w·h` u16 coverage.
  - v2: flags bit0 coverage, bit1 **byte origin** (u16 samples all multiples of 257); then the `FLOT` tone-map words
    (u32 operator 0 clip / 1 reinhard, f32 white).
  - v3: one more word `u32 editable_length`; flag bit2 (`FLAG_EDITABLE`) set iff nonzero; the section is the trailing
    bytes — UTF-8 JSON `{type, record, name?, anchor?}`, ≤ `MAX_RZDC_BLOB_LEN`; the core never parses `record`.
  - The decoder **refuses, never sanitizes**: bad magic/version/storage/flags, length mismatch, rejected ICC, bad PPI,
    non-finite f32, byte-origin on f32 or non-×257 samples, tone map on u16. Admission is checked before allocating
    (`2 · payload_bytes`, including the section).
- **Copy** reads a raster record's raw samples (latent RGB at alpha 0 kept); **Copy Merged** the projection; both multiply
  alpha by coverage. A group's Copy reads the group's RAW projection at the document's depth — its children composited
  without the group's own opacity, blend, mask, style or Blend If, visibility forced on — the byte Copy's rule
  (`../core/src/precision_clipboard_group.rs`). An adjustment target refuses Copy by name except the editable
  whole-layer case below. Cut of a group refuses at every depth.
- **Cut** = lock refusal first, then Copy, then exactly Clear's erase under the same coverage. A transparency lock yields
  `(payload, None)`: the pasteboard keeps the payload, no history step. Float Cut uses `blend::erase_alpha` in f32.
- **Paste at depth reads only the typed payload.** Centred = `((W − w) div 2, (H − h) div 2)` (also at 8 bits now), in
  place = the payload offset. A storage/profile
  mismatch never refuses: the payload is converted into the document's depth and profile.
- **Conversion** (`core/src/precision_clipboard_convert.rs`, `ClipboardPayload::converted_with`, under a
  `PasteProfile`): same storage with Preserve or the same profile → no conversion (the paste relabels); a u16
  target → f32 → u16 via the payload's own tone map (`tone_mapped_u16`'s rule), then, under Convert, the profile via
  `precision_color::convert_u16_pixels` (the same kernel as Convert to Profile); an f32 target →
  `core/src/precision_clipboard_promote.rs`: a u16 (or byte-origin) payload decoded to linear light through
  `TypedTransfer` (the payload's TRC under Convert, the destination's under Preserve), then under Convert
  `MatrixTrc::combined_with` in f64, rounded once to f32; an f32 payload in another profile takes the matrix. A float
  output outside ±65504 refuses by name. The paste admits the conversion, then the insertion with the converted
  payload and `PASTE_PROMOTE_MARGIN_PER_PIXEL` (29 B/px, fitted: a 24 MP 16-bit paste into a 24 MP float document
  peaked at 1,156,891,464 bytes) × canvas when a conversion into float ran;
  `clipboard_paste_plan` is the exact threshold and `clipboard_paste_allocation` the new memory (record in
  `precision_clipboard_promote.rs`).
- **Ask When Pasting** (`ColorSettings.askWhenPasting`, Image ▸ Mode ▸ Working Space ▸ Ask When Pasting, off by
  default). Off, every Paste converts silently. On, a typed payload,
  an external image or a layer clip whose profile `PasteProfile.differ`s from the document's (bytes AND name differ)
  asks "Paste Profile Mismatch": Convert / Don't Convert / Cancel. Don't Convert keeps the numbers (`profile:
  "preserve"`); depth always converts. The preference is app-wide with no MCP setter; MCP `paste` takes the choice as
  `profile`, and a `convert` boolean (old recordings) is accepted and ignored. An editable record is never asked about.
- **External images** (`PastePlan`, `app/Sources/ClipboardBoundaries.swift`, decided before any edit): pasted at every
  depth — decoded into the document's drawing space (Don't Convert: their own space), at 16 bits and float as
  an exact ×257 byte-origin payload the core promotes; MCP `external: "promote"` is the default and `"refuse"` refuses
  one at depth. An UNTAGGED image (no embedded profile in the pasteboard's bytes, or a space with no ICC,
  `ExternalClipboardImage.read`) keeps its numbers in the destination's profile and never asks — measured against
  Photoshop (case f below). Paste in Place of an external image, which has no source position, lands like Paste
  (`PasteContext.resolve` with no source: the selection, the view or the centre).
- **8-bit documents write the private type too** (`rz_clipboard_payload_from_rgba8`, byte origin) so Paste in Place
  works at 8 bits and 8-bit copies promote exactly. An 8-bit Paste of a typed payload that is not u16 in
  the document's exact profile converts in the core (`rz_clipboard_payload_rgba8` with the document's profile and the
  mode), so a pasteboard payload and a `payload_path` file take one path; no CoreGraphics conversion of the public
  image remains for typed payloads.
- **New from Clipboard** (`PrecisionDocument::from_clipboard`, `rz_precision_doc_from_clipboard`): byte-origin → 8-bit
  document; u16/f32 → typed document of the payload's depth, profile, PPI (and tone map), one "Background" at
  (0,0). No sheet — the mode readout and MCP reply disclose. Not recordable. A typed payload is first TRIMMED to its
  content box, every pixel with alpha > 0
  (`ClipboardPayload::trim`, `rz_clipboard_payload_trim`; always on; a fully transparent
  payload keeps its window, an external image its size); the host then restores an editable record into the new
  document (`RasterDocument.restoringEditableDocument`):
  text/shape replace the Background as that editable layer, an adjustment lands above a transparent Background in an
  8-bit document, and a record that cannot restore leaves the pixels with `editable.reason`. No undo step, no alert.
  MCP reply adds `editable`, `trimmed`, `source_offset`.
- **Editable payloads (12G-3).** A whole-layer Copy (no selection) of a text, shape or adjustment layer carries its record
  (`app/Sources/ClipboardEditable.swift`): adjustment `record` = layer meta verbatim; artwork = its description plus a
  carried `anchor`. Paste resolves `.editable` before every depth/profile rule and restores through the tools' own paths;
  if it cannot (unknown op, artwork into float, missing font…) it pastes pixels and the reply says why
  (`editable {type, restored: false, reason}`). FFI: `rz_clipboard_payload_with_editable`, `rz_clipboard_payload_editable`,
  and New from Clipboard's `rz_clipboard_payload_trim`.
- **Recording (Part B).** Copy, Copy Merged and Cut are steps; `get_clipboard` and `new_from_clipboard` are
  non-recordable. No copy step stores pixels, so a recorded Copy → Paste replays on any document.
  - Edit ▸ Copy records `copy {mode: "auto"}` with no layer named. `auto` decides as ⌘C does, on the replaying
    document: a pixel selection → pixels of the active layer, none → the selected layers
    (`LayerClipboard.resolvingAutoCopy`, read by the handler and by `DocumentPrecision`'s capability gate, which
    therefore asks the family of the mode `auto` resolves to). This agrees with Photoshop's recorded Copy
    step (measured, below). Edit ▸ Copy Merged records `copy_merged {}`. A Cut that degrades to Copy records nothing.
  - MCP `copy`, `copy_merged` and `cut` calls are recorded verbatim (`ActionRecorder.recordAgentCall`); the MCP
    default mode stays `pixels`.
  - **Paste has two kinds of step.** *Live* — the default, from the menu and over MCP, for pixels and layers alike:
    `paste {placement, layer}` with the REQUESTED placement (`auto`, `in_place`, `into`, `outside`; over MCP also
    `centered`, and `at` with its `x`/`y`), `profile: "preserve"` for a Don't Convert choice, and over MCP `name` and
    `mode` only when the call passed them. It stores no pixels and no landed position: a replay pastes whatever the
    clipboard holds then (pixels, or a held layers copy), lands by the replaying document's own Placement rule, and
    with no image fails with the handler's own refusal ("The clipboard holds no image.") through the player's
    ordinary failure path. *Saved payload* — MCP only, with `payload_path` or `record_payload: true` (an agent
    feature; refused for a layers paste): the landed payload is written atomically as
    `<uuid>.rzcb` under `ClipboardPayloadStore.directory` (`RZ_CLIPBOARD_PAYLOADS_DIR`) and the step is `paste
    {payload_path, placement, name, layer}` (plus `profile: "preserve"`; no step carries `convert`);
    `ActionAssets.table["paste"] = ["payload_path"]` packages it as `$asset`, the step is built before the edit
    publishes, and an `auto` or `at` paste (and a Paste
    in Place of an external image) records `placement: "at"` with the landed `x`/`y`. Steps recorded before Part B are
    saved-payload steps and replay unchanged.
  - `paste` stays in `ActionCatalogFacts.nonRecordable` because its handler records its own step. The Actions window
    names the kind on the row ("pastes the clipboard at replay" / "pastes its saved payload",
    `ActionCatalogFacts.pasteSource`), and `copy` / `copy_merged` rows carry "uses the selection".
- **Placement.** Edit ▸ Paste (⌘V) requests `auto`
  (`app/Sources/PasteLanding.swift`, `PasteContext.resolve`), which resolves in order: an active destination
  selection (not in Quick Mask, not wholly off the canvas) → centred on `clipboardWindow(selection)`, Paste Into's
  frame, with NO mask and the selection left active (`selection`); else, when the copy's WHOLE source rect lies inside
  the view's visible canvas (`EditorViewState.visible`) → its source position (`in_place`); else → centred on
  `visible.integral` (`view`); with no window "visible" is the canvas, so the last case is `centered`. Every frame
  placement lands at `frame.min + div_euclid(frame.size − size, 2)`; the core receives `into` plus that frame, so the
  core is unchanged. An external image has no source position. A layer paste applies the same rule to the roots'
  union content box (the part on the clip's canvas decides visibility; the frame centres the whole union). MCP `paste`
  defaults to `auto` over view 1 and reports the landed `placement` and `offset`; `centered`, `in_place`, `into`,
  `outside` are unchanged and `at` takes `x`/`y`. Layers take `auto` / `centered` / `in_place`.
- **Disclosure.** PPI mismatch → reply `ppi {payload, document, match}` and history "Paste Layer (300 → 72 ppi)".
- **Pasteboard.** `RZ_PASTEBOARD_NAME` selects a private named pasteboard (drives); released in
  `applicationWillTerminate`; the general pasteboard is never released. Validation only queries type availability.
- **Layer clipboard.** With NO pixel selection, ⌘C copies
  the selected entries — subtrees and every property — into an in-process layer clipboard: the document
  `duplicate_to_new_document` builds from them in one core call (`app/Sources/LayerClipboard.swift`), named on the
  pasteboard by a private token type (`com.kgalligan.rasterize.layer-clipboard`) beside the flat item; a later copy by
  anything else makes the entry stale. ⌘V lands every record above the active layer in ONE core call
  (`RzDocument::paste_layers` / `PrecisionDocument::paste_layers`, `../core/src/doc_paste_layers.rs`), so one undo step:
  fresh ids, link ids re-numbered to unused ones, the roots joining the slot's clip run, typed rasters/adjustments/warps
  re-bound to the destination's profile handle, and the destination's channels, comps, guides, evaluation, global light
  and tone map kept. **Placement:** ⌘V places the pasted roots' union CONTENT box (`layerBounds` over the clip's top
  level) by the Placement rule above, and an empty union does
  not move; ⇧⌘V keeps the source canvas coordinates. The translation runs inside the same core call through the Move tool's rule
  (`doc_align::translate_entry` / `translate_records`). **Depth and profile** follow the paste policy
  (`LayerPastePlan`): into 8 and 16 bits anything pastes — depth always converts (`narrow_structure_to_u8`, no
  dither; `narrow_structure_to_u16` under the clip's own map; `promote_structure`, exact ×257) and another profile is
  converted (`convertingToProfile`), or, by the Ask When Pasting alert's Don't Convert (MCP `profile: "preserve"`),
  assigned (`assigningProfile`). Don't Convert is unavailable for a clip holding placed images or live filters
  (`LayerPastePlan.preserveRefusal`, asked before the alert, which then omits the button and says why; MCP refuses by
  that sentence before any edit); a float destination never asks it. **Into 32-bit float** (Part C1, measured
  against Photoshop) anything pastes too, and the CORE does it inside the same one call
  (`../core/src/layer_clipboard_float.rs`): an 8-bit clip is first promoted ×257 by the host, then every record is
  promoted — a 16-bit plane decoded to linear light and, under Convert, taken through the source → destination
  matrix; a float plane of another profile converted, or relabelled under Don't Convert — with no profile or depth
  refusal. A placed image, live filter, text or shape description and warp binding land as the layer's own pixels
  (reply `rasterized`), a Pass Through group whose direct children all land Normal and hold no adjustment lands
  isolated (`isolated_groups`; any other Pass Through group refuses, because isolating it would change the look), and
  the twelve blend modes measured as reset at 32 bits (`mode_32` below) land Normal (`blend_reset`). A copied layer
  the float mode still cannot hold refuses the whole paste before any edit, naming the layer and the feature ("the
  copied layer “Name” cannot be pasted into a 32-bit float document: a layer style or Blend If is unavailable in a
  32-bit float document"); the Float gap table in the Part C1 entry below lists what does. **Flat item:** one
  root's flat image is its whole-layer Copy (raw layer pixels or a group's raw projection,
  with its editable record when today's Copy attaches one); several roots' is the clip's Copy Merged. Paste Into /
  Outside and New from Clipboard read the flat item. MCP: `copy {mode: "layers", layers}` (and `mode: "auto"`,
  Recording above), `paste {mode: "auto" | "layers" | "pixels"}` (layers take `centered` / `in_place`),
  `get_clipboard.layers`. A layer paste records the same live `paste` step a pixel paste does (Part B), so its
  replay pastes whatever the clipboard holds then.
- **Host stage.** Every typed read (Copy, a group's Copy,
  Copy Merged, Cut) also admits the host's public rendering and TIFF/PNG encode:
  `3 · payload + PUBLIC_RENDERING_BYTES_PER_PIXEL · window` (69 B/px, fitted) as a separate admission beside the read's
  own (`../core/src/precision_clipboard_host.rs`, whose module doc holds the measurement); `group_copy_plan` /
  `group_copy_allocation` take the max with it. A typed layer paste admits `paste_layers_allocation` (the insertion
  scratch over both record lists plus `LAYER_PASTE_MARGIN_PER_PIXEL`, 65 B/px of the destination canvas, fitted to the
  publication's projection render — record in `doc_paste_layers.rs`).
- **Held payload.** `Clipboard.writeItem`
  (`../app/Sources/HeldClipboardPayload.swift`) is the one pasteboard write. A typed RZCB of at most L = 255 MiB
  (267,386,880 bytes, `HeldClipboardPayload.systemInlineLimit`; probed 2026-10-05 on macOS 26 against a named pasteboard:
  255 MiB wrote and 256 MiB was refused, beside two 96 MB public blobs and alone) is written inline as before, so it
  crosses to other instances. A larger one is held in process, charged as retained, and named by a UUID under
  `com.kgalligan.rasterize.held-clipboard` beside the public TIFF/PNG (the layer clipboard's token pattern).
  - Safety net: an inline write the pasteboard refuses is retried as a held write; a refused held write throws "The
    system pasteboard refused the copied w×h image (… TIFF, … PNG)." — other apps then get nothing, and the clipboard
    has already been cleared, as a refused write always did.
  - Limits: the payload is the header plus w·h·8 at 8 and 16 bits, w·h·16 at float, plus 2·w·h with coverage — 24 MP
    float is 384 MB and 45 MP 8- or 16-bit 360 MB, so both are held. Another Rasterize instance, or any instance after
    the copying app quits, sees the token and the flat 8-bit TIFF/PNG and pastes it as an external image: 8 bits pastes
    it, and 16 bits and float promote it (×257 of the rendering, then linear light at float).
    Other apps get the TIFF/PNG unchanged (4 B/px: 96 MB at 24 MP, 180 MB at 45 MP; a 36-byte typed blob beside two
    180,400,000-byte public blobs wrote).
  - Release: the next copy, a validation that finds the token type gone (also when another writer put an inline typed
    copy there), a stale token read, and quit. `TypedClipboardPayload.read` checks the held entry first, so a stale one
    is always dropped. MCP `copy`, `copy_merged`, `cut` and `get_clipboard` report `typed_store` ("pasteboard",
    "process" or null).
  - Rejected: a lazy `NSPasteboardItemDataProvider` still pushes the full blob through the pasteboard when another
    process reads it, and AppKit resolves it at quit, so it does not lift the size refusal.
  - Rejected: a file promise or temp file writes 384–720 MB to disk per copy.
  - Rejected: compression does not bound a float payload below the limit.
- `.rz` format untouched.

## Rejected alternatives
- Superseded (maintainer ruling 2026-10-05):
  "Silent conversion on mismatch — every lossy boundary needs an explicit arm" and "Promotion of
  8/16-bit data into a float document — standing no promotion into float contract (11F)". Paste now converts
  silently, promotes into float, and asks only under Ask When Pasting.
- Editable payloads for Live Photo, retained-source and smart layers — their record is an immutable asset plus up to
  512 MiB of attachments; Duplicate and Place are their editable paths; copy reports `editable: null`.
- A New from Clipboard sheet — the window's mode readout is the disclosure.

## Consequences
- Any new clipboard producer must write RZCB v3 and let the decoder's refusals stand; readers must keep v1/v2.
- Profile conversion for paste and Convert to Profile must stay one kernel (`convert_u16_pixels`).
- A saved-payload step depends on its `.rzcb`; a live step on the clipboard at replay.

## Open items (verified 2026-09-26)
- `DONE` — New from Clipboard restores an editable section: the
  core still builds the pixel "Background" document (`PrecisionDocument::from_clipboard`) and the host rebuilds it
  around the restored layer (`restoringEditableDocument`, `../app/Sources/ClipboardEditable.swift`), disclosing
  `editable` in the MCP reply.
- `DONE` — New from Clipboard sizes the document to the
  payload's content box (`ClipboardPayload::trim`, `../core/src/precision_clipboard_trim.rs`).
- `DONE` — Copy of a group at 16 bits and float copies the
  group's raw projection, as 8-bit does (`../core/src/precision_clipboard_group.rs`); Cut of a group keeps refusing at
  every depth. **Decision for the maintainer:** keep the raw rule at both depths (this batch), or bake the group's own
  opacity, blend and mask at both depths in a later batch. The no-selection layer-structure copy is a separate gap.
  The 8-bit behaviour is unchanged.
- `DONE` — Copy/Paste of layers between documents (Decisions
  above). **Maintainer decisions, not built:** Paste Into/Outside and New from Clipboard use the flat image; layer comps, alpha channels, guides, samplers and metadata do
  not travel; the destination's global light, evaluation policy and tone map apply; the layer clipboard is in-process
  (another app or instance gets the flat image); a recorded layer paste was a placeholder (since Part B it is
  the live `paste` step); no promotion into float (since Part C1 a layers paste promotes into float);
  drag-and-drop of layers between windows; linked
  smart objects and layer-comp carriage; MCP `copy` defaults to pixels for compatibility.
- `DONE` — ⌘V keeps a copy's position (Placement above).
  **Unconfirmed, maintainer decisions:** the visibility threshold (the WHOLE source rect must be visible); whether ⌘V
  with a destination selection deselects (Rasterize keeps it); a rotated view uses the bounding box of the turned
  view (`EditorViewState`'s rule); layer paste follows the same rule, destination selection included. MCP `paste`
  reads view 1 and takes no `view`; relative or anchor geometry modes are not supported for `at`; the no-window
  canvas fallback is not driven by g16. 12G gate g16 covers the rest.
  Measured since (Part B): Photoshop's recorded Paste carries no position and replays live, and so
  does Rasterize's; only a saved-payload step records the landed offset as `at`.
- `DONE` — every typed clipboard read admits the host's
  public-rendering stage, fitted at ≥ 1.25× the measured peak (`precision_clipboard_host.rs`). At 6000×4000 float the
  system pasteboard refuses the typed payload (≥ 384 MB) for every copy, pixel or layer — measured at 4500×3000.
- `DONE` — a typed payload over 255 MiB is held in process
  and named on the pasteboard by a token (Decisions above); the 24 MP float copies are measured in
  `precision_clipboard_host.rs` and the 24 MP float layers paste in `doc_paste_layers.rs`.
- `DONE` — Recordable Paste as `$asset` step, float Cut, New from Clipboard at depth, convert/promote arms (superseded
  by the silent conversion), PPI
  disclosure, pasteboard release (`b509766`); editable payloads and 12G-2's four nits (`5e06b0a`).
- `PART A DONE` — Paste converts into the destination
  (Decisions above: Conversion, Ask When Pasting, External images; 12G gates g2, g8, g12, g13 rewritten, g17
  added). **Measured against Photoshop.** A Photoshop 26.10.0 drive, `scripts/photoshop-probe/paste_depth.py`
  (2026-10-05; the foreign-process check found no running Photoshop; the session launched and quit Photoshop;
  clipboard snapshot restored (1 items, 56 types); colour settings "North America General Purpose 2", read only).
  Verdicts, each against its fixed rule:
  a. 8-bit sRGB → 32-bit `linear` (max error 3.4e-5 vs the IEC 61966-2.1 decode; 0.287 vs the encoded numbers);
  b. 16-bit sRGB → 32-bit `linear` (7.8e-6); c. 8 → 16 `exact` (worst 1 code of v·257); d. 16 → 8 `narrowed` (0
  codes); e. 16-bit sRGB → 16-bit Display P3 `converted` (2.0e-5 of the analytic conversion), the silent Convert
  default; f. an untagged 8-bit TIFF on the general pasteboard into 16-bit Display P3 `destination_assumed` (its
  numbers kept within 0.004 codes; 0.36 from the sRGB-assumed conversion) — Rasterize reads an untagged external
  image as the destination's profile; g. Paste in Place of that TIFF into a 200 × 150 document with no selection
  `centered` (bounds 68, 67, 132, 83). Cases f and g needed Photoshop's own clipboard purged (Edit ▸ Purge ▸
  Clipboard) and the session's Photoshop activated once, the only moment it reads the system clipboard (first take:
  it pasted its own earlier copy; second: "Paste is not currently available"). **Maintainer decisions:** (1) a layers
  Don't Convert of a clip holding placed images or live filters — Rasterize omits Don't Convert with the reason
  ("Don't Convert is unavailable: the copied layers hold N placed image(s) and M live filter(s), which keep their
  pixels only through a conversion."), because the Assign that re-renders bound sources (with the 8-bit
  live-filter one-shot plan from `live_filter_profile.rs`) is a new colour-policy op with its own admission
  measurement; (2) a layers paste into a float document keeps its refusals (Part C). **Still pending:** Part C —
  Group Copy with a selection and Cut of a group (opacity, blend, mask baked?); comps, alpha channels, guides, samplers and global
  light with a layer copy; linked smart objects; Paste Into/Outside and New from Clipboard using layers; layers pasted
  into a float document; a float clip in another profile. Part D — the placement points: partial visibility;
  whether ⌘V with a destination selection deselects; a rotated view's visible area; layers-paste placement; an MCP
  `paste` `view` argument; remapping `at` under relative/anchor action geometry.
- `PART B DONE` — a recorded Paste replays the live
  clipboard, and Copy and Copy Merged are recorded steps (Decisions above: Recording; 12G gate g18 added; g10, g13,
  g14, g16 and g17 keep the saved-payload step through `record_payload`). **Measured against Photoshop.** A
  Photoshop 26.10.0 drive, `scripts/photoshop-probe/paste_action.py`, job
  `paste_action` (2026-10-05; three takes, the third is the record: the first ran Paste Into as `PstO` through an
  ExtendScript chained-ternary quirk, the second had two miswritten observation takes; each time the foreign-process
  check found no running Photoshop, the session launched and quit Photoshop, and the line was "clipboard snapshot
  restored (0 items, 0 types)" — the general pasteboard was empty when each take began). Each verdict is the
  descriptor `executeAction` RETURNED, against its fixed rule:
  1. `carries_position` false — no returned paste descriptor holds a point, offset or rectangle key (Paste, Paste in
     Place, Paste Into, Paste Outside, the layers paste, the paste after Copy Merged). Live steps carry no `x`/`y`.
  2. `carries_pixels` false — one unchanged `past` descriptor pasted a 16 × 16 layer after a 16 × 16 copy and a
     24 × 12 layer after a 24 × 12 copy (the second centred on the target selection).
  3. `kinds_distinct` — Paste returns `past {antiAlias: none, as: pixel}`; Paste in Place the same plus `inPlace:
     true`; Paste Into is the event `PstI` and Paste Outside `PstO`, each with Paste's keys. Each kind records its own
     placement.
  4. `layers_event` confirmed — with two layers targeted and no selection, `copy` then `past` landed both layers by
     name (three layers in the target). The layers paste RETURNED an empty `past {}`: it differs from a pixel paste
     only by lacking `antiAlias` and `as`.
  5. `empty_clipboard` — after Edit ▸ Purge ▸ Clipboard, Paste raises "The command “Paste” is not currently
     available." Rasterize's step fails with "The clipboard holds no image."
  6. `copy_event` — `copy` returned `{copyHint: "pixels"}` with a selection and `{copyHint: "layers"}` without one;
     `CpyM` returned `{}`. No pixels, no layer or document reference.
  Beside the verdicts the job records `hint_replay`, whether those keys bind a replay — they do not: `copy` SENT
  `copyHint: "pixels"` with layers targeted and no selection returned `copyHint: "layers"` and its paste landed the
  layers; `copy` sent `copyHint: "layers"` with a selection returned `"pixels"` and landed one pixel layer; `past`
  sent `as: pixel` with a layers copy held pasted the layers (and returned `{}`); `past` sent `{}` after a pixel
  copy pasted the pixels. So Photoshop's Copy step re-decides pixels or layers at replay, and its Paste step pastes
  what the clipboard holds. **Maintainer decisions:**
  (1) Photoshop's `antiAlias` (`AntA`) and `as` keys apply to vector clipboard content Rasterize does not have; not
  built.
  (2) When a recorded command is unavailable, Rasterize's player appends the failure to `report.failures` and
  stops when `stopOnError ?? (step.onError == .stop)` (`../app/Sources/ActionPlayer.swift` lines 178–188;
  `on_error` defaults to stop, `../app/Sources/Action.swift` line 77), with no prompt. A Continue / Stop prompt is
  a general Actions behaviour, not built here.
  (3) Copy: Photoshop's step carries `copyHint`, which names the mode it ran in but does not bind the replay.
  Rasterize records `copy {mode: "auto"}`, which re-decides the same way, and stores no hint.
  (4) The layers paste: the fixed rule read "a distinguishing key → record `mode: "layers"` / `"pixels"`". The only
  difference is the absence of `antiAlias` and `as` (decision 1), and `hint_replay` shows a step carrying `as:
  pixel` still pastes layers, so a recorded `mode` would make a replay refuse or flatten instead of pasting what
  the clipboard holds. UI steps therefore carry no `mode` (the step is `mode: "auto"` at replay).
  (5) The menu cannot record a saved-payload step; only MCP `record_payload` / `payload_path` does.
  (6) Paste in Place of a patch copied at (8, 8) in a 64 × 48 document landed at (42, 42) in a 200 × 150 one (bounds
  42, 42, 58, 58), not at its source offset. Recorded, not built: Part D's placement points.
- `PART C1: ROW 1 DONE, ROW 2 STOPPED` — copied layers
  paste into a 32-bit float document (Decisions above: Layer clipboard; 12G gate g19 added). **Measured against
  Photoshop.** A Photoshop 26.10.0 drive, `scripts/photoshop-probe/paste_layers.py`, job
  `paste_layers` (2026-10-05, one take; the foreign-process check found no running Photoshop; the session launched
  and quit Photoshop; "clipboard snapshot restored (1 items, 3 types)"; report
  `rasterize-fixtures/photoshop/paste-layers-470c/take1/measured.json`). Verdicts, each against its fixed rule:
  1. `layers_into_32` `linear` — an 8-bit and a 16-bit sRGB copy of a raster and a group holding a raster landed in
     a 32-bit sRGB document by name, the group kept (still Pass Through); max error 3.4e-5 (8-bit) and 7.8e-6
     (16-bit) against the IEC 61966-2.1 decode, 0.287 against the encoded numbers.
  2. `float_profile` `converted` — a 32-bit Display P3 layer landed in a 32-bit sRGB document within 6.0e-8 of the
     linear P3 → sRGB matrix on its seven in-gamut patches (0.044 from the source numbers).
  3. `float_gamut` `unclipped` — the pure P3 green patch kept its negative sRGB red (matrix −0.2249).
  4. `mode_32` — twelve modes landed Normal: color_burn, linear_burn, screen, color_dodge, overlay, soft_light,
     hard_light, vivid_light, linear_light, pin_light, hard_mix, exclusion (`PHOTOSHOP_FLOAT_RESET`). Fifteen were
     kept: normal, dissolve, darken, multiply, darker_color, lighten, linear_dodge, lighter_color, difference,
     subtract, divide, hue, saturation, color, luminosity.
  5. `held_32` (observation) — a drop-shadow style, a text layer, a smart object, a layer mask and a Pass Through
     group each read `kept` (the group's mode stayed Pass Through).
  6. `blend_if_32` (observation) `kept` — the white half hidden by a This Layer range read the 32-bit Background
     (0.2159 linear) after the paste; the 16-bit source showed the range hiding it (0.502).
  7. `adjustments_32` (observation) — levels, curves, color_balance, hue_saturation, photo_filter, exposure,
     black_and_white and channel_mixer: paste `kept`, create `ok`. invert, threshold, posterize and vibrance: the
     PASTE fails ("Could not complete the Paste Layers command because an adjustment or fill layer cannot be
     converted to the destination document mode.") and the create is refused ("Could not complete the command
     because that content type is not supported for this color mode.").
  8. `assign_smart_object` `numbers_kept` with the contents tagged Display P3 — **ROW STOP (row 2)**, below.
  **Measurement gate (row 1).** A 24 MP 16-bit white layer pasted into a 24 MP float document: E = 963,068,600 /
  963,035,832 / 962,790,072 (sRGB) and 963,068,600 / 962,806,456 / 963,248,824 (Display P3), the `list_documents`
  twin 0 every run; `paste_layers_allocation` A = 1,948,202,536, A / max E = 2.02 ≥ 1.25 with
  `LAYER_PROMOTE_MARGIN_PER_PIXEL` = 0 (record: `../core/src/layer_clipboard_float.rs`).
  **Float gap table** (what a copied layer may carry that the float mode lacks; the Photoshop column is this take's
  `measured.json`):

  | Feature | Rasterize, pasting into float | Photoshop 26.10.0 at 32 bits | Land with the feature rasterized instead? |
  | --- | --- | --- | --- |
  | Placed image, live filter (retained source) | Lands as pixels; `rasterized` row. | `held_32` smart object: `kept` (still a smart object). | Built: the kept pixels are the layer's own render, so the look is unchanged. |
  | Text or shape layer (description) | Lands as pixels; `rasterized` row. | `held_32` text: `kept` (still a text layer). | Built, same reason. |
  | Warp binding | Lands as its warped pixels; `rasterized` row. | unconfirmed (not measured). | Built, same reason. |
  | Pass Through group | Lands isolated (`isolated_groups`) when every direct child lands Normal and is not an adjustment; otherwise refuses: `a Pass Through group with blended or adjustment children`. | `held_32` group: `kept`, mode still Pass Through. | Built where exact. Otherwise no: isolating changes the look when a child blends non-Normally or is an adjustment. |
  | Layer style: drop shadow, inner shadow, outer glow, inner glow, stroke, color overlay, gradient overlay, bevel/emboss, satin, fill opacity, knockout, Blend Interior Effects as Group | Refuses: `a layer style or Blend If`. | `held_32` style: `kept` (drop shadow measured; the other effects unconfirmed). | No. Default effects blend with the layers below, fill opacity and knockout act on the backdrop, and a baked effect widens a clip base's footprint. The one faithful subset (a raster, every enabled effect Normal, fill 100%, no knockout, no clipped members) needs `bake_style16` in the paste plan with its own measured charge. Not built. |
  | Blend If | Refuses: `a layer style or Blend If`. | `blend_if_32`: `kept`. | No. Underlying ranges read the layers below; This Layer ranges weight a clip base's own alpha only. An unclipped raster with This Layer ranges only could be baked through `source_weight_16` with a new pixel pass and a measured charge. Not built. |
  | color_burn, linear_burn, screen, color_dodge, overlay, soft_light, hard_light, vivid_light, linear_light, pin_light, hard_mix, exclusion | Lands Normal; `blend_reset` row. | `mode_32`: `normal` (Photoshop resets them). | Not applicable. |
  | dissolve, darker_color, linear_dodge, lighter_color, subtract, divide, hue, saturation, color, luminosity | Refuses: `the {mode} blend mode`. | `mode_32`: `kept`. | No. A mode is defined against the backdrop. |
  | levels, curves, color_balance, hue_saturation, photo_filter, black_and_white, channel_mixer, and exposure under `legacy_srgb` | Refuses: `the {op} adjustment`. | `adjustments_32`: paste `kept`, create `ok`. | No. An adjustment has no pixels of its own; baking it rewrites every layer below it. |
  | invert, threshold, posterize, vibrance | Refuses: `the {op} adjustment`. | `adjustments_32`: Photoshop's own paste FAILS ("…an adjustment or fill layer cannot be converted to the destination document mode.") and the create is refused. | No. The paste does not happen. |
  | bcs, selective_color, shadows_highlights, gradient_map, color_lookup, white_balance under `legacy_srgb` | Refuses: `the {op} adjustment`. | unconfirmed (not measured). | No, as above. |
  | Exposure and White Balance under `document_v2` | Lands as the same adjustment. | `adjustments_32` exposure: paste `kept`, create `ok`; white_balance not driven. | Not needed. |

  **Maintainer decisions** (where Photoshop holds what Rasterize rasterizes, isolates or refuses; each needs the
  float mode's operator table widened, which is Phase 11F's): (1) a smart object stays a smart object and a text
  layer stays text in Photoshop; Rasterize lands their pixels. (2) A Pass Through group stays Pass Through; Rasterize
  isolates it when that is exact and otherwise refuses. (3) A drop-shadow style and Blend If are kept; Rasterize
  refuses the layer. (4) Ten blend modes are kept that the float mode lacks; Rasterize refuses the layer. (5) Eight
  adjustment kinds paste and can be created at 32 bits; Rasterize refuses all but a calibrated Exposure. (6) A record
  with a placed image is promoted under Convert even when the paste is Don't Convert (its pixels are a render of a
  master in the clip's profile); row 2's measurement below suggests Photoshop keeps such a layer's numbers, so this
  one sub-rule is unconfirmed. (7) The Paste Profile Mismatch dialog itself was not driven: it needs Color Settings ▸
  Ask When Pasting, a preference a drive may not write; it stays unconfirmed.
  **ROW STOP (row 2): Don't Convert keeping placed images and live filters at 8 and 16 bits is NOT built.** The
  assumption was that a paste which assigns re-renders a smart object from its contents. Measured
  (`assign_smart_object`): a Display P3 document with a plain patch layer and the same patches as a smart object
  (made while tagged; its contents document, `PS_smart.psb`, reads "Display P3") was untagged, and both layers were
  pasted into a 16-bit Adobe RGB document. Both kept their numbers exactly (plain 0.0 codes of 255 from its source;
  smart object 0.0 codes from the numbers recorded in the untagged document, and 86.9 codes from the P3 → Adobe RGB
  conversion), so the paste did not re-render the smart object: `numbers_kept`. One more observation, not part of
  the rule: untagging the document itself changed the smart object's numbers (P3 (60, 170, 130) became (0, 172.8,
  127.1), the P3 → sRGB conversion) while the plain layer kept its own, so Photoshop does re-render a smart object
  when the DOCUMENT's profile is assigned, and not when layers are pasted. Nothing was built for row 2: no
  re-rendering Assign, no change to `preserveRefusal` at 8 and 16 bits, and two things the spec had folded into that
  row are also not built — the default Convert layers paste of an 8-bit clip with a live-filtered placed layer still
  throws the core's live-filter sentence, and a `.convert` between two spellings of one colour model still answers
  "could not be converted" at 8 bits. **Still pending:** row 2 (a decision from this evidence); Part C2 — Copy with
  a pixel selection while a group is targeted and Cut on a group (opacity, blend, mask baked?), Paste Into / Outside
  and New from Clipboard with layers on the clipboard; Part C3 — comps, alpha channels, guides, samplers and the
  source's global light with a layer copy, linked smart objects; Part D — the placement points.
- `DROPPED` — Editable payloads for retained-source (in 12G-2's criteria), Live Photo and smart layers — declined in 12G-3.
