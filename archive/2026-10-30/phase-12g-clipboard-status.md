# High-depth clipboard (ADR)

- **Status:** Accepted — open items remain
- **Phase / dates:** 12G (carryover P7-29), runs 12G-1 … 12G-3, 2026-09-22 → 2026-09-23; issue records to 2026-10-05
- **Originals:** [archive/phase-12g-clipboard-status.md](../archive/phase-12g-clipboard-status.md) — the phase's full history, to 2026-09-26; [archive/2026-10/phase-12g-clipboard-status.md](../archive/2026-10/phase-12g-clipboard-status.md) — this doc as of 2026-10-07, with every measurement table and issue record
- **Key commits:** `e507cd6` (12G-1), `b509766` (12G-2), `5e06b0a` (12G-3)

> **Removed 2026-10-10:** smart objects, live filters and non-RAW retained sources (placed images, host
> media, Live Photo frames) are gone for v1; camera RAW layers stay. Where this record describes them, that
> code no longer exists: [removal ADR](convert-to-smart-object-status.md).

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
    bytes — UTF-8 JSON `{type, record, name?, anchor?}`, ≤ `MAX_DRDC_BLOB_LEN`; the core never parses `record`.
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
  place = the payload offset. A storage/profile mismatch never refuses: the payload is converted into the document's depth and profile.
- **Conversion** (`core/src/precision_clipboard_convert.rs`, `ClipboardPayload::converted_with`, under a
  `PasteProfile`): same storage with Preserve or the same profile → no conversion (the paste relabels); a u16
  target → f32 → u16 via the payload's own tone map (`tone_mapped_u16`'s rule), then, under Convert, the profile via
  `precision_color::convert_u16_pixels` (the same kernel as Convert to Profile); an f32 target →
  `core/src/precision_clipboard_promote.rs`: a u16 (or byte-origin) payload decoded to linear light through
  `TypedTransfer` (the payload's TRC under Convert, the destination's under Preserve), then under Convert
  `MatrixTrc::combined_with` in f64, rounded once to f32; an f32 payload in another profile takes the matrix. A float
  output outside ±65504 refuses by name. The paste admits the conversion, then the insertion with the converted
  payload and `PASTE_PROMOTE_MARGIN_PER_PIXEL` (29 B/px, fitted) × canvas when a conversion into float ran;
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
  `ExternalClipboardImage.read`) keeps its numbers in the destination's profile and never asks — measured
  against Photoshop (case f below). Paste in Place of an external image, which has no source position, lands like Paste
  (`PasteContext.landing`: the selection, the view or the centre).
- **8-bit documents write the private type too** (`dr_clipboard_payload_from_rgba8`, byte origin) so Paste in Place
  works at 8 bits and 8-bit copies promote exactly. An 8-bit Paste of a typed payload that is not u16 in
  the document's exact profile converts in the core (`dr_clipboard_payload_rgba8` with the document's profile and the
  mode), so a pasteboard payload and a `payload_path` file take one path; no CoreGraphics conversion of the public
  image remains for typed payloads.
- **New from Clipboard** (`PrecisionDocument::from_clipboard`, `dr_precision_doc_from_clipboard`): byte-origin → 8-bit
  document; u16/f32 → typed document of the payload's depth, profile, PPI (and tone map), one "Background" at
  (0,0). No sheet — the mode readout and MCP reply disclose. Not recordable. A typed payload is first TRIMMED to its
  content box, every pixel with alpha > 0
  (`ClipboardPayload::trim`, `dr_clipboard_payload_trim`; always on; a fully transparent
  payload keeps its window, an external image its size); the host then restores an editable record into the new
  document (`RasterDocument.restoringEditableDocument`):
  text/shape replace the Background as that editable layer, an adjustment lands above a transparent Background in an
  8-bit document, and a record that cannot restore leaves the pixels with `editable.reason`. No undo step, no alert.
  MCP reply adds `editable`, `trimmed`, `source_offset`.
- **Editable payloads (12G-3).** A whole-layer Copy (no selection) of a text, shape or adjustment layer carries its record
  (`app/Sources/ClipboardEditable.swift`): adjustment `record` = layer meta verbatim; artwork = its description plus a
  carried `anchor`. Paste resolves `.editable` before every depth/profile rule and restores through the tools' own paths;
  if it cannot (unknown op, missing font…; artwork into float restores since 11F phase 187b) it pastes pixels and the reply says why
  (`editable {type, restored: false, reason}`). FFI: `dr_clipboard_payload_with_editable`, `dr_clipboard_payload_editable`,
  and New from Clipboard's `dr_clipboard_payload_trim`.
- **Recording (Part B).** Copy, Copy Merged and Cut are steps; `get_clipboard` and `new_from_clipboard` are
  non-recordable. No copy step stores pixels, so a recorded Copy → Paste replays on any document.
  - Edit ▸ Copy records `copy {mode: "auto"}` with no layer named. `auto` decides as ⌘C does, on the replaying
    document: a pixel selection → pixels of the active layer, none → the selected layers
    (`LayerClipboard.resolvingAutoCopy`, read by the handler and by `DocumentPrecision`'s capability gate, which
    therefore asks the family of the mode `auto` resolves to). Photoshop's Copy step re-decides the same way
    (measured, below). Edit ▸ Copy Merged records `copy_merged {}`. A Cut that degrades to Copy records nothing.
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
    `<uuid>.drcb` under `ClipboardPayloadStore.directory` (`DR_CLIPBOARD_PAYLOADS_DIR`) and the step is `paste
    {payload_path, placement, name, layer}` (plus `profile: "preserve"`; no step carries `convert`);
    `ActionAssets.table["paste"] = ["payload_path"]` packages it as `$asset`, the step is built before the edit
    publishes, and an `auto` or `at` paste (and a Paste
    in Place of an external image, or one that landed `relative_in_place`) records `placement: "at"` with the landed
    `x`/`y`. Steps recorded before Part B are saved-payload steps and replay unchanged. Since Part D such an `at`
    step takes the relative geometry mode (its payload's centre mapped by `ClipboardSource.relativeOffset`, the size
    read from its `.drcb`) and the anchor modes (field by field); a mode on any other paste step is refused at decode.
  - `paste` stays in `ActionCatalogFacts.nonRecordable` because its handler records its own step. The Actions window
    names the kind on the row ("pastes the clipboard at replay" / "pastes its saved payload",
    `ActionCatalogFacts.pasteSource`), and `copy` / `copy_merged` rows carry "uses the selection".
- **Placement (measured in Part D).** Edit ▸ Paste (⌘V)
  requests `auto` (`app/Sources/PasteLanding.swift`, `PasteContext.landing`): an active destination selection (not in
  Quick Mask, not wholly off the canvas) → centred on `clipboardWindow(selection)`, Paste Into's frame, with NO mask
  (`selection`); else → centred on the view's `visible.integral` (`EditorViewState.visible`, `view`) — never the
  source position, however much of it is visible; with no window "visible" is the canvas, so `centered`. The familiar
  copy-then-paste still lands on the original because Copy leaves the marquee active and the first rule centres the
  paste on it, its own spot (`s_same_doc`, `x_same_doc_selected`). Paste in
  Place (⇧⌘V) keeps the payload offset when the copy's source canvas (`ClipboardSource`, written by
  `Clipboard.writeItem` beside every copy, `app/Sources/ClipboardSource.swift`) has the destination's size, and
  otherwise lands at `ClipboardSource.relativeOffset` (`relative_in_place`: the copy's centre keeps its fraction of
  the canvas, the corner floored), whatever the view and any selection; a payload file has no source canvas and keeps
  its offset. Every pixel paste made with a selection deselects it in its one step, Reselect restoring it
  (`PasteKind`, `SelectionHistoryState.consumedByPaste`); a layers paste keeps it. Every frame placement lands at
  `frame.min + div_euclid(frame.size − size, 2)`; the core receives `into` plus that frame, so the core is unchanged.
  An external image has no source position. A layer paste centres the roots' union content box the same way, and its
  ⇧⌘V moves that box by the relative rule across canvas sizes. MCP `paste` defaults to `auto` over the view `view`
  names (view 1 without it) and reports the landed `placement` and `offset`; `centered`, `in_place`, `into`,
  `outside` are requests and `at` takes `x`/`y`; `get_clipboard` reports `source`. Layers take `auto` / `centered` /
  `in_place`.
- **Disclosure.** PPI mismatch → reply `ppi {payload, document, match}` and history "Paste Layer (300 → 72 ppi)".
- **Pasteboard.** `DR_PASTEBOARD_NAME` selects a private named pasteboard (drives); released in
  `applicationWillTerminate`; the general pasteboard is never released. Validation only queries type availability.
- **Layer clipboard.** With NO pixel selection, ⌘C copies
  the selected entries — subtrees and every property — into an in-process layer clipboard: the document
  `duplicate_to_new_document` builds from them in one core call (`app/Sources/LayerClipboard.swift`), named on the
  pasteboard by a private token type (`com.kgalligan.doctored.layer-clipboard`) beside the flat item; a later copy by
  anything else makes the entry stale. ⌘V lands every record above the active layer in ONE core call
  (`DrDocument::paste_layers` / `PrecisionDocument::paste_layers`, `../core/src/doc_paste_layers.rs`), so one undo step:
  fresh ids, link ids re-numbered to unused ones, the roots joining the slot's clip run, typed rasters/adjustments/warps
  re-bound to the destination's profile handle, and the destination's channels, comps, guides, evaluation, global light
  and tone map kept; as measured against Photoshop (Part C3), the copied layers bring no comp, channel, guide or sampler, a pasted
  style that uses the global light casts by the destination's, and a linked placed image stays linked.
  **Placement:** ⌘V centres the pasted roots' union CONTENT box (`layerBounds` over the clip's top level) on a
  destination selection or the view (the Placement rule above, never in place), and an empty union does not move;
  ⇧⌘V keeps the source canvas coordinates on a canvas of the clip's size and otherwise moves the box by the relative
  rule; the selection is kept. The translation runs inside the same core call through the Move tool's rule
  (`doc_align::translate_entry` / `translate_records`). **Depth and profile** follow the paste policy
  (`LayerPastePlan`): into 8 and 16 bits anything pastes — depth always converts (`narrow_structure_to_u8`, no
  dither; `narrow_structure_to_u16` under the clip's own map; `promote_structure`, exact ×257) and another profile is
  converted (`convertingToProfile`; at 8 bits a live-filtered clip through `LiveFilterProfileWork.run`; a conversion
  that answers nil, two spellings of one model, lands relabelled), or, by the Ask When Pasting alert's Don't Convert
  (MCP `profile: "preserve"`), RELABELLED by the core inside the same paste call (Part C1b): every record keeps its
  numbers, and placed images and their live filters stay bound, re-pointed without a re-render. Don't Convert is
  unavailable only when the destination profile fails the Assign check for the copied text/shape or calibrated layers
  (`LayerPastePlan.preserveRefusal`, the core's relabel making the same check; asked before the alert, which then omits
  the button and says why; MCP refuses by that sentence before any edit); a float destination never asks it. **Into 32-bit float** (Part C1, measured
  against Photoshop) anything pastes too, and the CORE does it inside the same one call
  (`../core/src/layer_clipboard_float.rs`): an 8-bit clip is first promoted ×257 by the host, then every record is
  promoted — a 16-bit plane decoded to linear light and, under Convert, taken through the source → destination
  matrix; a float plane of another profile converted, or relabelled under Don't Convert; a placed image's plane under
  the paste's own mode like any other (C1b) — with no profile or depth refusal. A placed image with no live filter lands placed (11F slice 5: re-rendered from its master under Convert, its binding kept over the promoted plane under Don't Convert); a live-filtered or stale placed image and a warp binding land as the layer's own pixels, as does a text or shape description under Don't Convert from another profile (text and shape layers otherwise land described since 11F phase 187b)
  (reply `rasterized`), a Pass Through group lands Pass Through (11F slice 4), and
  the twelve blend modes Part C1 measured as reset at 32 bits land Normal (`blend_reset`). A copied layer the float mode still
  cannot hold refuses the whole paste before any edit, naming the layer and the feature ("the copied layer “Name”
  cannot be pasted into a 32-bit float document: a layer style or Blend If is unavailable in a 32-bit float
  document"); the float gap list in the Part C1 entry (shipped companion) says what does. **Flat item:** one root's flat image is its whole-layer Copy (raw layer pixels or a group's raw projection,
  with its editable record when today's Copy attaches one); several roots' is the clip's Copy Merged. Paste
  Into / Outside of a held entry are refused (Part C2; the items disabled; MCP `paste` auto
  throws `LayerClipboard.maskedPasteRefusal`; mode `pixels` pastes the flat item), and New from Clipboard lands the
  layers themselves (`LayerClipboard.Entry.newDocument`: the roots' content box at the clip's depth — float → 16 —,
  profile and PPI, over a white Background; reply `layers`). MCP: `copy {mode: "layers", layers}` (and `mode: "auto"`,
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
  (267,386,880 bytes, `HeldClipboardPayload.systemInlineLimit`; probed on macOS 26: 255 MiB wrote, 256 MiB was refused) is
  written inline as before, so it crosses to other instances. A larger one is held in process, charged as retained, and named by a UUID under
  `com.kgalligan.doctored.held-clipboard` beside the public TIFF/PNG (the layer clipboard's token pattern).
  - Safety net: an inline write the pasteboard refuses is retried as a held write; a refused held write throws "The
    system pasteboard refused the copied w×h image (… TIFF, … PNG)." — other apps then get nothing, and the clipboard
    has already been cleared, as a refused write always did.
  - Limits: the payload is the header plus w·h·8 at 8 and 16 bits, w·h·16 at float, plus 2·w·h with coverage — 24 MP
    float is 384 MB and 45 MP 8- or 16-bit 360 MB, so both are held. Another Doctored instance, or any instance after
    the copying app quits, sees the token and the flat 8-bit TIFF/PNG and pastes it as an external image: 8 bits pastes
    it, and 16 bits and float promote it (×257 of the rendering, then linear light at float).
    Other apps get the TIFF/PNG unchanged (4 B/px).
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
- A saved-payload step depends on its `.drcb`; a live step on the clipboard at replay.

## Open items (verified 2026-10-07)
- `DONE` — New from Clipboard restores an editable section: the core builds the pixel "Background"
  document (`PrecisionDocument::from_clipboard`) and the host rebuilds it around the restored layer
  (`restoringEditableDocument`, `../app/Sources/ClipboardEditable.swift`), disclosing `editable` in the MCP reply.
- `DONE` — New from Clipboard sizes the document to the payload's content box
  (`ClipboardPayload::trim`, `../core/src/precision_clipboard_trim.rs`).
- `DONE` — Copy of a group at 16 bits and float copies the group's raw projection, as 8-bit does
  (`../core/src/precision_clipboard_group.rs`); Cut of a group refuses at every depth. Measured in Part C2:
  Photoshop's Copy with a group targeted lands opaque white over the selection, not the group's content, and
  its Cut on a group refuses; the raw projection is kept (maintainer decision, Part C2 entry in the shipped companion).
- `DONE` — every typed group read (`layer_canvas_image`,
  layer-scoped analysis, a group selection source) uses the raw projection Copy does: `analysis_source` renders
  `raw_group_document` (`../core/tests/group_projection_rule.rs`, u16, float and the byte twin).
- `DONE` — Copy/Paste of layers between documents (Decisions above). **Measured against
  Photoshop in Part C3:** layer comps, alpha channels, guides and samplers do not travel, the destination's global
  light applies, and a linked placed image stays linked (applying a comp hides pasted layers it has no record
  of). **Maintainer decisions, not built:** metadata does not
  travel; the destination's evaluation policy and tone map apply; the layer clipboard is in-process (another app or
  instance gets the flat image); MCP `copy` defaults to pixels for compatibility. Paste Into / Outside and New from
  Clipboard with layers were settled by Part C2.
- `DONE` — ⌘V placement (Placement above; `b86891c`). ⌘V
  never keeps a source position (Part D removed the visibility rule `b86891c` built); a pixel paste deselects;
  a layers paste centres like pixels and keeps the selection; MCP `paste` takes `view`; a saved-payload `at` step
  takes relative and anchor geometry. Part D measured each of these against Photoshop. Still unmeasured: a rotated
  view's centre; the no-window canvas fallback is not
  driven by g16. Part B measured that Photoshop's recorded Paste carries no position and replays live.
- `DONE` — every typed clipboard read admits the host's public-rendering stage, fitted at ≥ 1.25× the
  measured peak (`precision_clipboard_host.rs`).
- `DONE` — a typed payload over 255 MiB is held in process and named on the pasteboard by a token
  (Decisions above; `8bf6ace`). The 24 MP float copies are measured in `precision_clipboard_host.rs` and the 24 MP
  float layers paste in `doc_paste_layers.rs`.
- `DONE` — Recordable Paste as `$asset` step, float Cut, New from Clipboard at depth, convert/promote arms (superseded
  by the silent conversion), PPI
  disclosure, pasteboard release (`b509766`); editable payloads and 12G-2's four nits (`5e06b0a`).
- `DONE` — Parts A–D: paste conversion, recorded Paste,
  layers into float and across profiles, group copy and cut, what travels with copied layers, and placement, each
  measured against Photoshop; their records are in the shipped companion (Shipped runs below).
- `DONE` — drag layers into another document window, from the Layers panel or with the Move tool (`6d5383d`).
- `DONE` — a Continue / Stop prompt when a recorded step cannot run
  ([phase-12b-actions-status.md](phase-12b-actions-status.md), **Interactive Play prompts**).
- `DROPPED` — Editable payloads for retained-source (in 12G-2's criteria), Live Photo and smart layers — declined in 12G-3.
- `OPEN` — the layer features the float mode lacks (the float gap in the Part C1 entry).

## Shipped runs
[phase-12g-clipboard-shipped.md](phase-12g-clipboard-shipped.md) — the records of Parts A–D.
