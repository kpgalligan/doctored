# Phase 12G — high-depth clipboard: status

Slice 12G (carryover [P7-29](phase-7-carryovers.md#assignment-register)) is
THREE runs: 12G-2 split again, and editable payloads became 12G-3. All three
have shipped, and **the slice is closed**; this doc is their record.

| Run | What it owns | State |
|---|---|---|
| **12G-1** | The typed RZCB interchange payload and codec; Copy / Copy Merged / Paste / Paste in Place at 16 bits and 32-bit float; Cut at 16 bits; MCP `cut`/`copy`/`copy_merged`/`paste`/`get_clipboard` at all three depths; a private-pasteboard drive | **Implemented 2026-09-22** |
| **12G-2** | RZCB v2 (byte origin, carried tone map); New from Clipboard at depth; the explicit external-boundary choices (convert arm, external promotion, no promotion into float, 8-bit documents writing the private type); Paste as a recordable `$asset` step; float Cut; PPI-mismatch disclosure; pasteboard release | **Implemented 2026-09-22** |
| **12G-3** | RZCB v3's editable section: a whole-layer Copy of a text, shape or adjustment layer carries its record at every depth and Paste restores it; 12G-2's four carried nits | **Implemented 2026-09-23** |

---

# 12G-1 — the typed payload, the four commands, MCP

## What shipped

- **The payload** (`core/src/precision_clipboard.rs`). One canvas window of
  RAW samples in the source document's storage (RGBA16 UNORM or RGBA32
  float) and profile, the canvas offset it came from, the document's PPI and,
  under a selection, the window's u16 coverage. The public TIFF/PNG beside it
  on the pasteboard is a RENDERING (u16 preview; float through the
  document's tone map).
- **RZCB v1**, little-endian: `"RZCB"`, u32 version 1, u32 storage (1 u16,
  2 f32), u32 w, u32 h, i32 ox, i32 oy, f32 ppi x/y, u32 profile length +
  ICC bytes, u8 has_coverage, `w·h·4` samples, then `w·h` u16 coverage. The
  decoder REFUSES — never sanitizes — a bad magic/version/storage, an empty
  or oversized window, a length that disagrees with the header, a profile
  `IccProfile::parse` rejects or trims, a non-finite or non-positive PPI, and
  any f32 sample failing `extended_pixel`. It is admitted before the samples
  are allocated.
- **Copy** reads a raster record's raw samples through its offset — latent RGB
  retained at alpha 0, zeros where the record does not reach — and **Copy
  Merged** reads the projection. Both multiply alpha by the coverage (u16:
  `(a·c + 32767) / 65535`; f32: `a · c/65535`). A group or adjustment
  target refuses Copy by name (Copy Merged is the answer).
- **Cut** (16 bits): the lock refusal first (a pixels lock refuses before any
  payload exists), then Copy, then exactly Clear's `PaintEdit::Erase` under
  the same coverage. A transparency lock makes the erase a byte-exact no-op:
  the core answers `(payload, None)`, the pasteboard keeps the payload, the UI
  shows the frozen-alpha alert and MCP answers `frozenAlphaRefusal` with no
  history step. A float document refuses by the operator table's
  `clipboard_cut` row.
- **Paste / Paste in Place** (⇧⌘V) at depth read ONLY the typed payload. A
  payload of another storage or profile is refused by a sentence naming both
  sides and `convert_precision` / `convert_profile`; nothing is converted.
  Centred = `((W − w) div 2, (H − h) div 2)`, in place = the payload offset.
  The record lands at `insert_slot(anchor, position)` and shares the
  payload's pixels.
- **Admission.** Every read charges its bookkeeping, then `2 · payload_bytes`
  (payload + the encoding the `RzClipboardPayload` handle holds) before it
  allocates; Copy Merged also charges the canvas render before rendering;
  Cut passes the payload (and a full-coverage plane when there is no
  selection) as the erase's other retained bytes.
- **Host.** `Clipboard.pasteboard` is `NSPasteboard(name:)` when
  `RZ_PASTEBOARD_NAME` is set (the drive's private pasteboard) and `.general`
  otherwise. Validation asks `TypedClipboardPayload.isAvailable` — a type
  query, never a read of the blob. Paste in Place is disabled in 8-bit
  documents. The float whitelist admits Copy, Copy Merged, Paste and Paste in
  Place; Cut stays out.
- **Recording.** `copy`, `copy_merged` and `get_clipboard` are reads
  (`nonRecordable`). Byte and typed Cut record the `cut` step, so a replayed
  Cut copies too (disclosed change: the byte Cut used to record
  `clear_selection`). `paste` stays a visible `__unrecorded` placeholder at
  every depth — 12A's Paste row is closed as "refused from recording by
  name; asset form is 12G-2".
- The `.rz` format is untouched; no operator enters a float or linear-light
  document (copy/paste move samples only).

## Verification

`core/tests/precision_clipboard.rs` (codec, copy/merge/cut/paste oracles,
admission, FFI, NULL inputs) and `scripts/verify_phase12g_clipboard.py`
(g1–g6 on an owned app with a private pasteboard).

---

# 12G-2 — the boundaries

## What shipped

- **RZCB v2** (`core/src/precision_clipboard_codec.rs`, the codec moved out of
  `precision_clipboard.rs`, whose module is now the operations only). The v1
  layout with version 2, the `has_coverage` byte generalised to `u8 flags`
  (bit0 coverage, bit1 **byte origin**), then the native `FLOT` table's two
  words — `u32` tone-map operator (0 clip / 1 reinhard) and `f32` white —
  read through `ToneMap::from_code`. Refused by name: flags above 3, bit1 on
  f32 storage, bit1 with any u16 sample (alpha included) that is not a
  multiple of 257 (checked in the pass that already walks the samples),
  operator ≠ 0 on u16 storage, and whatever `from_code` refuses on f32. v1
  streams still decode (flag byte = coverage, a v1 float payload reads as
  `Clip`); the encoder writes v2. `info()` adds `version`, `origin_depth`
  (8 | 16 | 32) and, on float, `tone_map`.
- **A float payload carries its source's tone map**: float Copy / Copy Merged
  / Cut stamp `PrecisionDocument::tone_map()`, so the payload renders the same
  wherever it lands.
- **The convert arm** (`core/src/precision_clipboard_convert.rs`,
  `ClipboardPayload::converted`). The ONLY targets are u16. Depth first — f32
  → u16 in the PAYLOAD's profile under the PAYLOAD's map, `FloatPolicy::
  encode_sdr` then one `round(v · 65535)` (exactly `tone_mapped_u16`'s rule)
  — then, when profile bytes differ, `precision_color::convert_u16_pixels`,
  the per-pixel kernel extracted out of `convert_layers` so Convert to
  Profile and the paste arm cannot drift. Every f32 target refuses by name
  (a u16 source: "there is no promotion into float mode"; an f32 source of
  another profile: "Convert to Profile is not admitted on a float
  document"); a payload already in the target answers `Ok(None)`; any
  conversion clears the byte-origin flag. Admitted (`2 · payload_bytes` of
  the target, plus the encoder/transform tables) before allocating.
- **Paste** (`clipboard_paste`) takes `"convert": bool` (default false). A
  mismatch with `convert: false` is 12G-1's refusal with a depth-aware tail:
  into 16 bits "…or paste with convert to convert the clipboard's pixels into
  this document"; into float ": a 32-bit float document pastes only a float
  copy in its own profile; there is no promotion into float mode" — no
  sentence at float names an arm. A byte-origin u16 payload of a 16-bit
  document's own profile pastes with no arm (the promotion is exact).
- **External images.** `PastePlan` (`app/Sources/ClipboardBoundaries.swift`)
  decides the paste BEFORE any edit, for the menu, MCP and the probe. At 16
  bits an image from another app needs the PROMOTE arm — the Edit menu asks
  "Paste an 8-bit image into a 16-bit document?", MCP takes `external:
  "promote"` — and lands as `bytePayload` (exact ×257, the document's
  profile, CoreGraphics having converted it into the drawing space). At
  float it is refused by name; nothing is promoted into float mode.
- **8-bit documents speak the private type.** An 8-bit Copy / Copy Merged /
  Cut writes a byte-origin payload (`rz_clipboard_payload_from_rgba8`, the
  selection's offset, the document's profile and PPI) beside the public
  TIFF/PNG, so Paste in Place works at 8 bits and an 8-bit copy promotes
  exactly into 16 bits. The 8-bit PASTE keeps today's CoreGraphics profile
  conversion: a typed payload's own `rgba8()` is used only when it is u16 in
  the document's exact profile bytes; otherwise the public rendering is
  decoded into the drawing space and the reply says `converted: true`. The
  centred placement at 8 bits is now the core's `div_euclid` formula (it used
  to land at (0, 0)).
- **New from Clipboard** (`PrecisionDocument::from_clipboard`,
  `rz_precision_doc_from_clipboard`, `NewFromClipboard`). A byte-origin
  payload makes an 8-bit document of its exact bytes and PPI; a 16-bit or
  float payload a typed document of its own depth, profile, dimensions and
  PPI — one "Background" record at (0, 0), the offset dropped, coverage
  already in alpha, a float payload's tone map carried; an external image
  today's 8-bit document. **Decision: no sheet.** The window's own mode
  readout and the MCP `new_from_clipboard` reply (`typed`, `origin_depth`,
  `depth`, `profile_description`, `tone_map`) are the disclosure.
- **Float Cut.** The erase is `alpha' = blend::erase_alpha(a, c / 65535)` in
  f32 under `under_locks(layer, EditKind::Pixels, …)`, RGB retained, `Ok(None)`
  when no sample changed; a transparency lock answers `(payload, None)`.
  `clipboard_cut` joins the float operator table and both capability tables;
  its `REFUSED` row is gone, and the paint row now reads "paint, the Eraser
  tool, fill and gradient" (the Eraser tool at float stays refused).
- **Paste is a recordable `$asset` step.** While a recording runs, the landed
  payload is written atomically as `<uuid>.rzcb` under
  `ClipboardPayloadStore.directory` (`RZ_CLIPBOARD_PAYLOADS_DIR`, else
  `~/Library/Application Support/Rasterize/Clipboard Payloads`) and the step
  is `paste {payload_path, placement, name, convert, layer}`;
  `ActionAssets.table["paste"] = ["payload_path"]`, so `package_action`
  rewrites it to `{"$asset"}` and a replay pastes the same samples with no
  pasteboard. Nothing is written when no recording runs. The typed payload
  itself is recorded (a convert arm is the step's `convert: true`, so the
  replay converts through the same kernel); the promoted payload for an
  external image; `bytePayload` of the pasted image at 8 bits.
  `new_from_clipboard` is `nonRecordable` for `new_document`'s reason.
- **PPI mismatch** is disclosed: the reply's `ppi {payload, document, match}`
  and the history row "Paste Layer (300 → 72 ppi)".
- **Pasteboard release.** `Clipboard.releasePrivatePasteboard()` releases a
  drive's named pasteboard in `applicationWillTerminate`; the general
  pasteboard is never released.
- **12G-1's carried nits**: Cut admits its full-coverage plane before
  building it; the codec split (above); `raster_float`'s comment re-wrapped;
  a decode refuse/succeed pair at `2 · payload_bytes`.

## Verification

`core/tests/precision_clipboard_boundaries.rs` (v2 codec, byte origin,
`to_rgba8`, the three conversions against the test's own f64 matrix/TRC
chains, the paste arms, the float erase oracle, New from Clipboard, the three
new FFI exports and their NULL sweep) and `scripts/verify_phase12g_clipboard.py`
g7–g11. One measured tolerance: the float copy pasted into an 8-bit document
goes through the CoreGraphics decode, whose premultiplied round trip moves a
half-transparent pixel's RGB by at most one code; opaque pixels are exact.

---

# 12G-3 — editable payloads

## Decision

A whole-layer Copy — no selection — of a TEXT, SHAPE or ADJUSTMENT layer
carries its editable record as an RZCB v3 trailing section, at every depth;
Paste restores the record (artwork re-rendered at native depth from its
description, an adjustment re-created from its meta) and discloses when it
cannot. Everything else stays raster: a selection copy, a raster layer, Copy
Merged and Cut.

**Declined, recorded:** Live Photo, retained-source (placed image) and smart
layers. Their record is an immutable asset plus up to 512 MiB of
attachments, which is not a clipboard payload; Layer ▸ Duplicate and Place
are their editable paths. Their copy is pixels only and the `copy` reply
says `editable: null`.

## What shipped

- **RZCB v3** (`core/src/precision_clipboard_codec.rs`). v2 with version 3
  and ONE more header word after `f32 white`: `u32 editable_length`. Flag
  bit2 (`FLAG_EDITABLE`, 4) is set iff the length is nonzero — a
  disagreement is refused — and the section is the LAST `editable_length`
  bytes, after the coverage. A v3 flag byte is at most 7; a v2 one stays at
  most 3 with no length word; v1 is unchanged, and both still decode. The
  section is UTF-8, at most `MAX_RZDC_BLOB_LEN`, a JSON object of exactly a
  string `type`, a non-empty string `record`, an optional string `name` and
  an optional `anchor` of two finite numbers — anything else is refused by
  name, and the core never reads `record`. The declared length joins the
  `expected` stream length before any sample is allocated, and decode admits
  `2 · (payload + section)`. The encoder writes v3 only.
- **`ClipboardPayload::with_editable`** validates a section exactly as the
  decoder does (one `validate_section`) and, BEFORE parsing or cloning,
  admits what the FFI handle will hold: retained `payload_bytes()` + profile
  bytes + `2 · len` (the handle's fresh re-encoding of the `Arc`-shared
  samples, the section in the payload and again in the encoding), scratch
  bookkeeping + `2 · len`. `payload_bytes()` counts the section; `info()`
  adds `editable: null | {type, name?, anchor?, record_bytes}`; `converted`
  carries the section, `from_rgba8`/Copy/Cut make none and New from
  Clipboard ignores it. FFI: `rz_clipboard_payload_with_editable` and
  `rz_clipboard_payload_editable` (NULL with no error when there is none).
- **The host record** (`app/Sources/ClipboardEditable.swift`). Section JSON
  `{"type": "text"|"shape"|"adjust", "name": <layer name>, "record": <meta>,
  "anchor": [x, y] (artwork only)}`. An adjustment's `record` is the layer's
  meta VERBATIM (`layerMeta`, which already unifies both depths); an
  artwork's is its description with a legacy text box resolved from the
  raster, and its anchor is CARRIED (`describedAnchor`), never recovered from
  the pasted raster. An adjustment layer's Copy at every depth is a
  canvas-sized transparent byte-origin payload (u16, also in a float
  document) carrying its meta; `publicRendering(of:)` renders a byte-origin
  payload as its exact `rgba8()` — the float document's `displayRGBA8`
  refuses u16 storage. The Copy menu item is now enabled on an adjustment
  layer when there is no selection, and MCP `copy`'s capability gate
  (`DocumentPrecision`) no longer asks the raster-only `clipboard_copy`
  family about an adjustment target when there is no selection, because
  that copy reads no samples. A float document admits only its own
  adjustment ops (exposure, white balance): restoring any other into it is
  refused by the core's sentence.
- **Paste.** `PastePlan.resolve` answers `.editable` right after the typed
  read, BEFORE every depth and profile rule, whenever the record can be
  restored into this document; no pixels cross. The restore is the tools'
  own: artwork through `nativeArtwork` at `anchor + (placement offset −
  payload offset)`; an adjustment through `addingAdjustmentLayer` at 8 bits
  (the meta verbatim, a reveal-all mask) and `addingAdjustment(anchor:
  position:)` with `PrecisionAdjustment(op:params:)` at depth. A record that
  cannot be restored pastes as pixels through the ordinary plan and the
  reply says why (`editable {type, restored: false, reason}`): a record this
  build cannot decode, an adjustment op it does not know, artwork into a
  32-bit float document (which does not admit editable artwork — the
  ordinary plan then refuses the u16 payload: no promotion into float mode),
  or a text family not installed.
- **One name rule.** `PasteWork` resolves the layer name — the caller's, else
  an editable record's own name, else "Pasted Layer" — and puts it in
  `PasteOutcome.name` (the `paste` reply's `name`). The Edit menu passes nil;
  `ClipboardPayloadStore.pasteStep` records `outcome.name`, so the
  `ActionStep.paste` shape is unchanged and a replay never renames a
  restored layer (the `$asset` carries the section).
- **12G-2's four carried nits.** (1) MCP `paste` builds its recorded step
  BEFORE publishing the edit, so a failed recording write publishes nothing.
  (2) An image from another app pasted in place into a 16-bit document is
  refused with the 8-bit sentence: it carries no canvas offset. (3) At 8 bits
  a typed payload this build cannot read (another build's RZCB) beside a
  public image pastes the public image — only for the pasteboard; an
  unreadable `payload_path` always refuses with its own decode error.
  (4) Two over-long lines in `EditorViewController.copyToPasteboard` wrapped.

## Verification

`core/tests/precision_clipboard_editable.rs` (v3 round trips with and
without a section at u16 and f32, coverage on and off, bit-exact re-encode;
v1/v2 decode with no section; every flag, length and section refusal by
name; `with_editable`'s validation, replacement and two admission pairs;
`info().editable`; `payload_bytes`; `converted` carrying the section; the two
FFI exports and their NULL sweep), the v3 pins in `precision_clipboard.rs`
and `precision_clipboard_boundaries.rs`, and
`scripts/verify_phase12g_clipboard.py` g12 (text records at 8 and 16 bits
under their own name and a caller's, a shape at 8 bits, a levels record at 8
and 16 bits and refused by name at float, an exposure record into float and
copied back within it, the selection copy staying raster, text refused into
float) and g13 (a recorded editable paste
replayed from its `$asset` under its own name; nits 2 and 3; the truncated
`payload_path` refusal).

## Residual, with owner

| Item | Owner | Criterion |
|---|---|---|
| New from Clipboard restoring an editable record | 12H declines; unscheduled clipboard follow-up after Phase 12 | Today New from Clipboard ignores the section and makes the raster "Background" document; restoring the record would build the document around the restored layer. |

---

# 12G-2 — criteria (as written before the run)

- **New from Clipboard at depth.** A typed payload becomes a document at its
  own depth and profile (offset dropped, PPI kept); an external image stays a
  byte document, and the sheet says which happened.
- **Editable payloads.** A whole-layer Copy of a text, shape, adjustment or
  retained-source layer carries the editable record; anything else stays
  raster. Paste restores the record with its description.
- **External-boundary choices.** An external image pasted into a typed
  document is promoted with its profile honoured and the choice disclosed; a
  cross-depth or cross-profile typed paste gains an explicit `convert` arm
  (never silent); 8-bit documents write the private type too (byte Paste in
  Place and offsets).
- **Paste recordability** as a `$asset` step carrying the payload.
- **Float Cut**, which needs a float erase with its own oracle.
- **PPI-mismatch disclosure** on paste.
- Pasteboard ownership beyond `RZ_PASTEBOARD_NAME` (no
  `NSPasteboard.releaseGlobally` is attempted in 12G-1).
