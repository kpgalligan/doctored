# Phase 8 — retained sources

## Current slice

**8A — Embedded ordinary image source: complete within the qualified scope below.**
**8B — Linked asset workflow: complete within the qualified scope below.**
**8C — Retained RAW: complete within the qualified scope below.**
**8D — Develop workflow: complete within the qualified scope below.**
**8E — Retained artwork: complete within its [qualified scope](phase-8e-artwork-status.md).**
**8F — Retained host still/frame sources: complete within its [qualified scope](phase-8f-host-source-status.md#qualification).**
**8G — Camera controls and local RAW development: complete within its
[qualified scope](phase-8g-raw-enhancements-status.md#qualification).**
This file is the shared progress record; the
[implementation plan](professional-photo-editor-implementation-plan.md#8-retained-sources-and-revisitable-raw)
and [carryover register](phase-7-carryovers.md) retain the later assignments.
**P7-01–P7-05 and P7-35 are verified** by 8A–8F.
Six assignments are verified and 30 remain open. The [host-source status](phase-8f-host-source-status.md)
records historical 8F implementation, qualification and limits. **Phase 8 is
complete within its declared qualified scope; Phase 9 is next and has not started.**

The first slice originally added **Layer > Place Embedded Image…**, **Embedded Image
Source…**, and **Rasterize Embedded Image**, with `place_image`,
`get_image_source`, and `rasterize_image_source` MCP counterparts. Placing and
rasterizing record Actions; source inspection does not. Placement lands at the
slot the `position` / `layer` pair names — above the anchor by default, at the
top of the root level, or as a group's top child — at original size, with
optional integer canvas coordinates through MCP. It appended a top root until
[12E-2b](phase-12e-layer-ops-status.md), which routed it through the ONE slot
rule (`core/src/layer_insert.rs`) at both depths; an ABSENT pair is still that
top-root push, byte for byte.
The layer panel identifies embedded images and provides source information.

## Source and evaluation contract

- An immutable asset retains the original decoded, orientation-normalized straight
  RGBA samples and supported original RGB ICC profile. The asset ID is persistent and independent
  of a filesystem path. Ordinary image sources do not retain their original
  compressed container. Retained RAW additionally embeds the original camera
  file, effective recipe and decoder identity alongside its developed u16 master.
  RAW decoding occurs only at initial import or explicit redevelopment; geometry,
  profile conversion, native reopen and display use the captured developed master.
- The layer stores its accumulated affine map, interpolation policy and a rendered
  fallback in the document's profile. A display CGImage is never a master.
- `transform_layer` and the single-layer Free Transform path compose the new
  canvas transform with the stored source transform. Their previews and commits
  use the original asset. An identity accumulated placement restores its original
  samples when the document and source profiles and depths agree, including hidden RGB.
  Cross-depth fallbacks retain their declared conversion; the master remains exact.
- Evaluation is **original source → document-profile conversion → accumulated
  affine sampling → explicit fallback-depth conversion → layer mask → existing styles/blending/opacity**.
  Byte originals promote before typed evaluation; typed originals render before
  their byte fallback is quantized. Same-depth evaluation is unchanged. Live
  adjustment layers retain their established stack positions. A future persisted
  source filter belongs after profile conversion and before the accumulated
  transform, with its own versioned parameters/evaluation policy in Phase 9.
  Version 1 has no persisted source-filter stage. Mask/style evaluation uses the
  existing depth-specific contracts; this does not add typed style support.
- Converting the document profile rerenders each active retained layer from its
  original asset/profile. Conversion happens before sampling. The master and ID
  stay unchanged; repeated conversions do not overwrite them. Assign Profile
  requires explicit rasterization because it reinterprets existing samples.
- Linked masks follow the incremental transform using the established mask
  sampling rule. The source's original RGBA alpha remains retained; this slice
  does not give a separately edited mask an immutable master or promise lossless
  repeated mask resampling.
- Rasterize detaches the source without changing the current fallback pixels.
  Central Undo restores the binding, source and transform. Destructive pixel edits
  require this deliberate step. Core fallback-allocation/profile guards prevent
  an older caller from resurrecting a source after modifying its fallback. Ordinary
  embedded sources require an empty layer-description slot; writing other
  metadata detaches the source, and native v14 refuses competing descriptions
  before decoding pixels.

## Phase 8A baseline persistence and limits

Byte documents with active sources write native **v14**; typed documents with
active sources write **v15**. Source-free documents retain their v10/v13 writers;
prior supported readers remain available. The storage modes have distinct tags,
and opening a malformed native file cannot fall through to an image decoder.

The native `RSRC` section embeds each referenced source asset once and records
layer-to-asset references. Original samples use lossless PNG with native depth;
original ICC bytes and transforms are separate bounded records. The existing
layer record holds the usable fallback. Assets are not base64 layer metadata.
All records, references, dimensions, profiles, depths and aggregate capacity are
preflighted before decoding pixels. Source ICC slots retain the existing 16 MiB
individual limit and share a 64 MiB aggregate limit. Source-bearing documents
require a valid owning ICC profile; old byte documents retain their established
profile-repair behavior. Unknown/inconsistent records refuse.
Native save, portable snapshots and recovery retain assets without their former
file paths. In-memory history and source duplicates share immutable allocations;
resource reports count masters and fallback guards even when an obsolete binding
is no longer valid. Native serialization deduplicates referenced masters within
a document; deduplication across separate recovery/snapshot generations remains
[P7-32 / 13C](phase-7-carryovers.md#assignment-register).

The 8A baseline accepted single-image PNG, JPEG and TIFF. Source and document
must have matching depth: up-to-8-bit ordinary input for byte documents; genuine
RGB/RGBA16 PNG/TIFF for typed documents. Source-depth mismatches refuse before
pixel decode. No byte promotion, 16-bit reduction, host-decoder fallback or
working-space adoption is implicit. Byte intake retains the established import interpretation: an absent, invalid or
unsupported ICC tag uses the legacy sRGB assumption. Typed preserving intake
keeps its stricter refusal rules. This slice does not qualify CMYK/Gray profile
interpretation or recover an absent camera profile; broader input color choices
remain in 11C/11D. Valid supported RGB profiles remain embedded exactly.

File decode, native codecs and byte profile preparation retain their existing
stage cancellation boundary. Source sampling and typed preparation use the
bounded job infrastructure; stale document/target/selection results do not
publish. Admission includes the original source even if the fallback is tiny.
Admission remains an estimate, not an OS RSS ceiling.

## Qualification

These are the completed **8A** results. Phase 8B results are tracked separately below.

The fixed public source fixture is **96 × 64** in both depths, with independently
computed bilinear and Display P3 → sRGB references. It contains genuine u16 RGB
values below byte steps, tiny alpha and latent RGB. Every stored fallback sample
is compared, including after 1/8 scaling, native relocation, source-file deletion,
profile conversion and return to full size. Additional native tests exercise
source payloads larger than the description limit, sharing, old versions,
malformed references, capacity and cancellation.

Final verification results:

- Final Swift `make typecheck`: **passed, 0 errors**.
- Final-source Clippy: **passed**; formatting and diff checks pass.
- Release app build: **passed**.
- **19 Swift host groups and 3 independent Python groups passed**, including all
  source/profile/alpha samples, actual byte transform preview and cancellation,
  source-kind hardening, one-step Action replay, saved identity, snapshots and
  recovery. All 308 app/header/probe source hashes and the file set match.
- **12 external MCP groups passed** across both depths, with all 145 catalog tools
  present. Four owned processes verify fresh reopen after native relocation and
  original-file deletion; all exited. Authoritative sample and flat PNG/profile
  comparisons agree before/after restoration.
- The full release Rust suite passed **1,023 tests in 96 groups**, with no
  failures and two existing ignored tests. **46 final follow-up tests** passed
  across source operations, native storage, work planning, profile, geometry and
  structure after the last planner correction. These include three new work
  regressions, 10 source-operation tests and 14 native asset tests.
- The work planner now uses the same exact/permutation/filtered sampling decision
  as rendering and charges linked-mask sampling independently. Actual 24 MP
  byte/u16 source buffers pass identity-placement planning; expensive filtered
  cases still refuse at the unchanged two-billion-unit limit. This proves
  admission correctness, not a measured 24 MP placed-image editing workflow.

The initial host Save failures came from sandbox-denied AppKit XPC access. The
identical binary passed Save and the complete retained-source workflows outside
that sandbox; this was not a native-format failure. Subsequent final-source host
and built-app MCP runs passed too.

Final evidence saved at:

- `/tmp/rz-phase8a-host-final-summary.json` and
  `/tmp/rz-phase8a-host-final-planner.log`.
- `/tmp/rz-phase8a-mcp-final-report.json` and `/tmp/rz-phase8a-mcp-final-provenance.json`.
- `/tmp/rz-phase8a-typecheck-final-source.log`,
  `/tmp/rz-phase8a-clippy-final-source.log`, `/tmp/rz-phase8a-app-final.log`.
- Full Rust run: `/tmp/rz-phase8a-core-tests-final.log`; final follow-up:
  `/tmp/rz-phase8a-final-core-focus.log`; counts: `/tmp/rz-phase8a-rust-summary.json`.

These are functional qualification results, not a new large-document performance
claim. The existing Phase 7 24 MP result is unchanged; Phase 13 retains the larger
multilayer/source/history workload assignments.

## Phase 8B — linked asset workflow

**8B and P7-35 are complete within this qualified ordinary-image scope.**
The source-information and rasterization menus are now named **Image Source**
and **Rasterize Image Source**. New commands appear in the Layer menu and layer
context menu; placement, replacement, refresh, relink and embed have MCP/Actions
parity. Portable-copy export is an MCP command and deliberately excluded from
Actions, like save_copy.

- Replacement keeps source identity, accumulated affine and layer properties,
  while creating a new content revision. Duplicates can retain different revisions.
- Linked bindings record an absolute local path and SHA256 of the captured file.
  Import snapshots the file privately before decoding so the hash identifies the
  decoded bytes. Cached originals/profiles and rendered fallbacks are always saved.
- Source inspection reports current, changed, missing or unreadable links.
  File changes never trigger an automatic rerender. Unchanged refresh is a no-op;
  explicit refresh creates one undoable revision. Relink accepts the captured
  file at a new path; different bytes require Replace Contents.
- Cross-depth placement/replacement defaults to refusal and offers explicit
  conversion of the rendered layer while retaining actual master precision.
  Byte originals promote by 257 for typed evaluation; typed originals evaluate
  profile/affine at 16-bit precision before nearest, undithered byte fallback
  quantization. Original profiles and hidden RGB remain retained.
- Embed removes a link using its cached original, even when the file is missing.
  Save Portable Image Copy packages all cached originals into one native .rz
  copy with external image links removed. It leaves the live editor and saved
  identity unchanged. The package contains decoded originals, not original
  compressed file containers.
- Advanced source records use byte native v16 or typed v17. Source-free v10/v13
  and compatible embedded v14/v15 writers remain. Each native original retains
  its actual depth; links belong to layer bindings. Shared asset references serialize once. Distinct assets cannot claim the same
  identity/revision pair; replacing one duplicate preserves another's old revision. Link records are bounded and validated before any pixel decode.
- One resource inventory covers byte and typed owners together, deduplicating
  shared master pixels and profiles across document/history/decoded-input handles.
  Conversion estimates include both typed temporary rendering and byte fallback.
- UI, MCP and Actions use shared capture/worker/publication paths. Inspection
  is not recorded; portable-copy export is excluded from Actions like save_copy.

## Phase 8B qualification

Final checks passed:

- Full release Rust suite: **1,044 passed in 99 result groups**, no failures and
  two existing ignored tests. After the final admission/capacity corrections,
  **60 source/native/color/work/cutout tests** and **36 legacy document/capacity
  tests** passed. The follow-ups include four new regressions beyond that full
  baseline: one allocator-instrumented metadata test and three capacity tests.
- Final Rustfmt and Clippy with warnings denied: **passed**.
- Final authoritative Swift typecheck: **passed, 0 errors**.
- Final Release app build: **passed**.
- **20 Swift host groups and 5 independent Python groups passed.** All four
  combinations of 8/16-bit master and document depth pass. Independent readers
  compare 28 cached native originals and 16 fallback/profile arrays, including
  low-code RGB, tiny alpha, hidden RGB, affine down/up and profile conversion.
  All **314 app/header/probe source hashes and the file set match**.
- **18 external MCP groups passed**, with all **150 catalog tools** present.
  Fresh processes reopen linked native files and portable copies after original
  paths are deleted. Original depth/profile, identity/revision and fallback
  samples agree. All owned verification processes exited.
- UI/host paths cover actual Save, one-step Undo/Redo and saved identity, Actions,
  snapshots/recovery, different-size/depth replacement, moved/changed/missing/
  unreadable links, unchanged refresh, restored-snapshot no-op, and byte locked
  relink/embed/package. Typed layer locks remain the existing 12E assignment.
- Planning no longer copies large unrelated byte descriptions. The 8 MiB
  allocator test verifies that plans and refused edits avoid those copies;
  accepted edits preserve them within the declared scratch. Profile-conversion
  estimates include byte document strings, typed sampled-curve preparation and
  the typed temporary render before byte quantization.
- Planned and final native growth share one comparison against the **original**
  document. Boundary tests cover 1,024 layers, the 400-million weighted
  source/fallback pixel limit, duplicated old revisions, precision-changing
  replacement and non-growing oversized-state compatibility. Fixtures share
  small buffers; these are capacity tests, not 400 MP workflow qualification.
- Sparse oversized dependencies refuse before checksum reads and leave the
  cached source/history unchanged. Link checks reuse the encoded-file upper
  bound implied by intake's three file buffers plus 16 MiB preparation reserve.
  Actual decode admission also charges pixels and other retained owners.

The fixed functional fixtures are 96×64, with 48×32 replacement cases. This slice
does not claim new large-document performance or an OS memory ceiling. Original
compressed containers are not retained; a portable package is one native .rz
copy of decoded originals/profiles and fallbacks. Links use absolute local paths,
SHA256 identity and explicit refresh. Codec, source-copy and fingerprint work
retain stage cancellation; broader cancellation and source-heavy workloads have
the Phase 13 owners listed below.

Evidence:

- `/tmp/rz-phase8b-rust-summary.json`, `/tmp/rz-phase8b-core-tests.log`,
  `/tmp/rz-phase8b-source-final-followup.log`,
  `/tmp/rz-phase8b-native-legacy-followup.log`.
- `/tmp/rz-phase8b-clippy-final.log`,
  `/tmp/rz-phase8b-typecheck-final-budget.log`,
  `/tmp/rz-phase8b-app-qualified.log`.
- `/tmp/rz-phase8b-host-qualified.log` and
  `/tmp/rz-phase8b-host-qualified-summary.json`.
- `/tmp/rz-phase8b-mcp-qualified-report.json` and
  `/tmp/rz-phase8b-mcp-qualified-provenance.json`.
- `/tmp/rz-phase8b-final-source-check.json` and `/tmp/rz-phase8b-doc-check.json`:
  final app/archive and all 314 host / 493 MCP source hashes match; 171 local
  documentation links pass, with exactly P7-01/P7-35 verified and 34 rows open.

Final archive SHA256:
`524e2b614dccd5fcb38cd4573b221a1b55c98d7c25c89ddf2c116a4edd25fc10`.
Final app executable SHA256:
`80a39d64332be23b02c462fe59f12dd7842e3e9db763a7c8f3c5645e7c804230`.

## Phase 8C — retained camera RAW

**8C and P7-02 are complete within this qualified camera-source scope.**
The implementation embeds the exact captured camera file beside a genuine u16
SDR sRGB developed master and the existing layer fallback. **Layer > Redevelop
RAW…** patches the saved effective recipe below existing retouch layers; **RAW
Source…** inspects its original identity and decoder information. MCP mirrors
are `redevelop_raw` and `get_raw_source`. Redevelopment records Actions and
uses central Undo/history; inspection does not record a command.

- Initial RAW import preserves 16-bit document storage by default. An explicit
  `precision: rgba8` creates a byte fallback while retaining the genuine u16
  developed master and camera original. The initial Develop dialog offers both
  document depths. Pixels render directly from Core Image
  to UInt16 with a float working format and explicit straight-alpha output.
- A versioned dense recipe captures exposed and hidden defaults, exact as-shot
  chromaticity, orientation, output policy, decoder version, OS implementation
  and Core Image build identity. Redevelopment patches saved values. Explicit
  `as_shot_white_balance: true` restores the captured as-shot white balance.
  Unsupported controls refuse; empty and effective unchanged requests are no-ops.
- Original camera bytes are immutable and shared across recipe revisions.
  Source identity, accumulated affine, masks and layer properties survive;
  replacement gets a fresh content revision. Retouch pixels on other layers
  remain unchanged. Direct destructive editing of the RAW layer requires the
  existing explicit rasterization step.
- RAW-bearing documents write byte native v18 or typed v19. The original bytes,
  dense descriptor, developed u16 master and fallback survive native, portable
  copies, snapshots and recovery. Earlier compatible writers/readers remain.
  Original RAW payloads use a separate deduplicated RRAW table; no base64 metadata.
- Opening saved work never runs a RAW decoder. A changed/unavailable implementation,
  unknown recipe or unavailable camera decoder leaves saved pixels usable and
  refuses redevelopment. Original-file deletion does not affect embedded sources.
- Per-file original cap: 256 MiB; aggregate distinct originals: 512 MiB; descriptor
  cap: 64 KiB. Existing memory and native pixel limits also apply. Intake snapshots
  and fingerprints the same file bytes it decodes; all native records preflight
  before pixels. Resource reports count shared originals across history owners.

Qualification passes on a **3008×2000 Nikon D70s 12-bit compressed NEF** at
its native dimensions, with an independent Core Image reference and additional
small synthetic DNG fixtures. The original camera fixture is CC0; its retrieval
URL, exact size and SHA256 are in [the fixture manifest](../scripts/raw-probe/camera-fixture.json).
No camera-family-wide quality or new 24/48 MP performance claim follows from
this declared workload. Results and artifacts are recorded below.

## Phase 8C qualification

Final checks:

- Full release Rust suite: **1,065 passed**, no failures, two existing ignored,
  across 104 result groups. **43 final follow-up tests passed** after admission,
  inspection and source-guard corrections: 19 RAW and 24 legacy native/resource
  tests. All 301 core/header/test/Cargo source hashes match the follow-up record.
- Rust formatting and Clippy with warnings denied: **passed**.
- Authoritative Swift typecheck and Release app build: **passed, 0 errors**.
- **30 Swift host groups passed**: 14 per document depth, actual initial 8-bit
  depth selection, and renamed PNG16/TIFF16 `.dng` preserving import. They cover
  actual Develop cancellation and RAW UI validation, one-step Action replay,
  real NSDocument Save, saved identity, exact Undo/Redo, as-shot reset, unchanged
  request no-ops, snapshots, private recovery, new-owner native reopen and
  redevelopment after original deletion. All 305 compiled-host dependencies and
  the 503 full dependencies/file set match the final source.
- **12 host native/snapshot/recovery RAW payloads** compare exactly against the
  independently retained original file bytes.
- **22 external MCP groups passed**, 11 per fallback depth, with all **152
  catalog tools** present. All 503 dependency hashes and the complete file set
  match; all four owned app processes exited. Missing originals, fresh-process
  native portability, decoder-unavailable cached rendering/refusal, exact exposure
  round trips and repeated identical-request no-ops pass.
- Four real-camera master comparisons each check all **24,064,000 u16 components**
  against an independent Core Image reference. Maximum error is **one u16 code**,
  below the independently declared maximum of three. Both **6,016,000-pixel**
  exported composites match independent source-over exactly; PNG16 and TIFF16
  samples and profiles agree. **184 opaque cloned retouch pixels** exactly match
  their shifted original-camera source codes. Original file SHA and native
  camera payloads remain exact.
- Explicit custom white balance is canonicalized through the same chromaticity
  setter used on restore. Both synthetic and native-size camera checks verify
  an exposure change and return preserves every developed u16 code and all
  captured white-balance fields exactly; no test threshold was relaxed.
- Allocation regressions cover 8 MiB original bytes plus 8 MiB unrelated layer
  metadata, a 40 KiB recipe refused before parsing, and 96 shared 40 KiB
  descriptors whose native plans stay below 2 MiB allocation peak. Full source
  info has a cheap preflight bound; edit protection uses a separate allocation-free
  valid-binding query so failed inspection cannot bypass rasterize-first policy.

Evidence saved at:

- `/tmp/rz-phase8c-core-summary.json`, `/tmp/rz-phase8c-core-tests.log`.
- `/tmp/rz-phase8c-core-followup-summary.json`,
  `/tmp/rz-phase8c-core-final-followup.log`, `/tmp/rz-phase8c-core-clippy-final.log`.
- `/tmp/rz-phase8c-typecheck-final-routing.log`,
  `/tmp/rz-phase8c-app-final-routing.log`.
- `/tmp/rz-phase8c-host-final.json`,
  `/tmp/rz-phase8c-host-native-payloads-final.json`.
- `/tmp/rz-phase8c-mcp-final.json`, `/tmp/rz-phase8c-final-artifacts.json`.
- `/tmp/rz-phase8c-final-source-check.json`: final 301 core and both 503 host/MCP
  source sets plus all 14 saved artifact hashes match.
- `/tmp/rz-phase8c-related-doc-check.json`: 178 local documentation links pass,
  with 36 unique carryover rows, exactly P7-01/P7-02/P7-35 verified and 33 open.

Final archive SHA256:
`c6cd8e78ad3d474cbfab9de82e515d5b07090ca4ebf96ac554c006c5f64fb89c`.
Final app executable SHA256:
`fd325802d0e11040b20b81a010fd69d75a407d226d6861299fdbffa150ffcedb`.
Final host executable SHA256:
`c65b72af6dcd5b9f91e62b1c73cdcbf2185ea3e2419b060ba44356b4d3bc79be`.

The full decoder test requires macOS graphics-service access: sandbox-denied
Core Image rendering can return zero-filled output without an error. Production
intake now rejects missing/nonopaque camera output. Early probe-only failures
(source edits during Swift compilation, an argv directory interpreted as a
user document, private-recovery registration and a typed offset schema mismatch)
were fixed in the harness; final host results supersede those candidates.

Boundaries retain explicit later owners: 8D owns 100% detail inspection,
histogram/clipping, before/after, presets and synchronization; additional camera/
lens and local RAW controls remain its separate enhancements. Wider-gamut SDR
develop output choices belong to 8D with 11C. Floating/HDR RAW
output requires the versioned HDR/color policy in 11E/11F. Decoder migration
across implementation changes needs an explicit preview/choice in 8D; 8C never
silently reinterprets a saved recipe. RAW capture and decoding are noncancellable; the subsequent
source-replacement stage supports cancellation. Broader codec cancellation and
larger RAW/source-heavy budgets remain 13C/13D. Typed new-layer/retouch workflow improvements remain 12E/9D.
The byte path retains existing standard ImageIO metadata/PPI reconstruction.
Typed RAW retains all camera metadata inside the exact embedded original, but
standard capture packets and print resolution are not yet copied into typed
flat-export/document metadata. Import reports disclose this; the preserving
host metadata/PPI bridge is assigned to 11D (general typed metadata editing
remains 12E). Byte RAW keeps the existing auxiliary-matte channel import;
typed camera auxiliary matte/depth channel intake remains 9G's source-derived
scalar/selection assignment. No native-camera-family or auxiliary-model quality
claim is added by the Nikon and synthetic DNG fixtures.

## Phase 8D — Develop workflow

Implementation and final Rust, host/UI, external MCP and camera qualification
are complete. This section does not change the completed 8C evidence above.

- **Detail and comparison:** Develop opens a full-resolution u16 inspection
  workspace with Fit, 100%, Before, After and Split. On Retina displays, 100%
  maps one source pixel to one backing pixel. Pan, location sliders and arrow
  keys choose a native region, bounded to 1024 pixels per side. Fit uses a
  bounded display image, at most 1600 pixels per side. Both views derive from
  the genuine master; draft camera previews never supply detail or histograms.
  Redevelopment's Before is the saved master, including when a decoder has
  changed. Initial import's Before is the captured as-shot development.
- **Readouts:** RGB histogram bins count every alpha-positive source pixel.
  Exact zero/65535 counts remain separate from bins and byte display conversion.
  Blue/red/magenta clipping markers identify RGB zero/maximum/both; Fit ORs
  endpoints across each source cell so a small clipped feature cannot disappear
  between sampled color pixels. These are normalized output endpoints, not a
  sensor-saturation or highlight-recoverability measurement. Source readouts
  precede accumulated affine, document masks, selection and retouch. P7-07's
  selection/backdrop histogram scopes remain assigned to 9F.
- **Preview lifetime:** one full-resolution worker and one pending request
  coalesce slider/pan changes. Only the current generation enables Apply.
  Cancel clears display references; a running camera task retains its immutable
  inputs until completion and then releases the shared worker. Camera decoding
  remains noncancellable. The core's histogram, detail and clipping walks expose
  cancellation and explicit admission; no larger camera budget is claimed.
- **Presets and synchronization:** presets contain selected user controls,
  never decoder defaults, camera identity or dense recipes. White balance copies
  the custom temperature/tint pair or asks each target for its own captured
  As Shot balance. The versioned library permits 128 presets, 8 KiB each, uses
  atomic writes and a nonblocking interprocess lock, and has an isolated
  `RZ_RAW_PRESETS_DIR` test override. Synchronization accepts up to 32 explicit
  retained RAW layer targets. Every result and final projection prepares before
  any document changes; source and target state are checked through publication.
  Each changed document receives one Undo. Actions record resolved
  `apply_raw_settings` values and layer targets, so later library edits and
  session document IDs cannot alter replay. Library writes are outside document
  Undo and cannot be Action steps.
- **Output space:** an explicit sRGB / Display P3 choice controls the normalized
  SDR u16 master and its ICC profile. Changing a retained source's output space
  preserves the document profile and evaluates its fallback through the existing
  source conversion path. Byte documents retain a genuine u16 master. Existing
  sRGB recipe v1 remains readable; changed output choices use recipe v2. The
  native v18/v19 opaque recipe and master/original storage need no format change.
  Unknown policy/version combinations refuse edits before any version update.
  Additional profile/appearance choices remain 11C; floating/HDR output remains
  11E/11F.
- **Decoder migration:** Layer > Migrate RAW Decoder explicitly compares the
  saved master with a candidate from the current implementation. The reviewed
  pixels are the pixels committed in one Undo. Supported controls and output
  space transfer; the current decoder supplies camera calibration/hidden
  defaults and its As Shot balance. Lost authored controls, changed orientation
  or dimensions, unknown recipe semantics, and stale document/target/selection
  all refuse migration. Existing native opens never migrate automatically.
  The MCP preview returns one app-wide ticket, replaced by the next review,
  expired after five minutes, and released on document close. Recorded migration
  Actions require exact prior and candidate descriptors plus the same current
  implementation; they cannot silently approve a different interpretation.

New MCP paths mirror these controls: `inspect_raw_development`,
`preview_raw_decoder_migration`, `migrate_raw_decoder`, `list_raw_presets`,
`save_raw_preset`, `delete_raw_preset`, `apply_raw_preset`, `apply_raw_settings`
and `sync_raw_settings`. Inspection returns Before/After display images and
full-source u16 readouts without changing the document. Unpatched inspection of
the saved master needs no camera decoder. `raw.output_space` is shared by import,
redevelopment, presets, synchronization and Actions.

### Phase 8D qualification

External migration checks caught a JSON parsing defect: the default parser
could move a binary64 recipe value by one ULP (for example tint
`11.999950408935547` became `11.999950408935549`). Enabling correctly rounded
`serde_json` float parsing fixes exact reviewed/persisted descriptor equality.
Neither the equality gate nor the pixel tolerances changed. Two added FFI/native
regressions use 15 explicit IEEE bit patterns, including the observed values,
midpoints, subnormals, signed zero and integer limits, across eight save/open
cycles in both depths. Previously rounded values already serialized by older
builds retain that recorded value; their earlier intended bits cannot be inferred.
Both regression tests pass. This preserves binary64 numerical values, rather
than original decimal spelling or arbitrary decimal precision; native schemas,
integer/string handling and limits are unchanged.

The final full Rust suite passes **1,076 tests, zero failures and two existing
ignored tests**, across **107 result groups**. The seven new inspection tests
cover the complete u16 code range, exact native
crops/profile/alpha, both fallback depths, Fit clipping cells, admission, NULL
inputs and cancellation. Final formatting and Clippy pass. Authoritative Swift
typechecking and `MACOSX_DEPLOYMENT_TARGET=15.0 make app` pass after the final
parser and UI layout changes.
The total includes 1,065 earlier 8C full-suite passes, two previously qualified
8C follow-up tests, seven new inspection tests and two exact-number tests.
The pre-fix 1,074-test run remains separate historical evidence.

**Host: 28 shared-workflow/preset-sheet groups plus nine actual Develop UI
groups pass.** These cover strict bounded presets, explicit fields, target-local
As Shot, multi-layer/document preparation, atomic refusal, actual source-selection
changes during workers, no-op Undo and preset-independent Actions. Independent
u16 sums/bins/endpoints and exact crops exercise readouts; migration tests cover
unknown recipes, lost authored controls, stale/consumed tickets and exact replay.
Actual modal controls cover Save Preset, the preset manager, synchronization,
Fit/100%, pan, Before/After/Split, clipping, P3 output, import document depth,
Cancel and late worker release. Both saved UI captures were visually reviewed;
the controls start at the top and long migration explanations keep decisions
reachable. The final host run took 9.09 seconds with approximately 517 MB peak
RSS; this small-fixture measurement is not a production memory ceiling.

**External Release MCP: 18 groups pass; the live catalog contains 161 tools.**
Both fallback depths preserve exact reviewed/accepted/native descriptors,
preview pixels, histogram values and original camera bytes. Checks include
persisted presets, resolved Actions after preset replacement/deletion,
cross-depth synchronization, target refusal, superseded/stale tickets,
Undo/Redo, recorded migration, and fresh-process native reopen. All three owned
MCP app processes exited. This controlled migration corpus changes saved
implementation provenance; it does not qualify every historical/future OS or
camera-decoder migration. Wider migration/camera coverage remains 13D, and
geometry-changing migration remains in 12F; 8G later qualifies exact local-plane
preservation for unchanged geometry.

The real-camera color gate passes **13 groups**, six in byte documents and seven
in typed documents. Four full 3008×2000 master comparisons check **96,256,000
u16 components** against an independently compiled direct Core Image Display P3
reference. The fixed acceptance bound is three codes; the observed maximum is
**one code**. The reference contains 10,319 initial and 23,347 developed pixels
outside sRGB under an independent P3-to-sRGB matrix, and more than 17.9 million
non-byte RGB components per version. Exact checks cover original bytes/profile,
output-space and exposure round trips, unchanged-setting no-ops, Undo/Redo,
deleted-original fresh-process redevelopment, and typed PNG16 source samples
and ICC. All four owned camera-test app processes exited.

Final evidence: `/tmp/rz-phase8d-workflow-host-final.json`,
`/tmp/rz-phase8d-workflow-mcp-final.json`,
`/tmp/rz-phase8d-color-fixed-final.json`, and
`/tmp/rz-phase8d-workflow-artifacts.json`. Core evidence is
`/tmp/rz-phase8d-core-summary-final.json` and
`/tmp/rz-phase8d-core-tests-final.log`; a separately linked C probe exercises all
five new exports from the final archive in
`/tmp/rz-phase8d-archive-smoke-final.json`. Reports name their private artifacts;
reproduction is documented in `scripts/raw-workflow-probe/README.md`.
The host and MCP each verify 521 source/build inputs; the host separately
verifies all 318 compiled Swift/header inputs. The camera gate verifies 525
inputs; core qualification verifies 305 source/header/test/Cargo inputs.
File sets and hashes match their final runs. The fixture and
camera scope remain the same CC0 Nikon D70s used in 8C. This qualifies the
platform bridge and retained workflow, not calibrated photographic color
accuracy or additional camera families.

Final qualified artifact SHA256 values:

- Release app: `6248e5aa582d5f623a420c0368a8bd822dfce11a48e027b6c52fb48b5a234152`.
- Rust archive: `f752953516f31d4513503120b9b2f29df84e07d3fde572b63c21f314315c6ff6`.
- Compiled host probe: `ba15e72f8b9ede9041722db8ab4097d78a78b7c2af984b7550d8b99c22511312`.

The final Rust build/tests and Xcode use deployment target 15.0 consistently.
An earlier exploratory archive differed because Xcode rebuilt dependencies with
that environment value; it was replaced before final host/MCP/camera qualification.
The final read-only audit, `/tmp/rz-phase8d-final-audit.json`, rechecks the source
sets, 16 artifact hashes and exit of all seven final test app processes. The
documentation audit verifies 180 local links/anchors and all 36 carryover rows:
P7-01, P7-02 and P7-35 remain verified, and 33 remain open.

## Phase 8D handoff and assigned boundaries (historical)

- **8E / P7-04–05:** retained editable artwork, native generated coverage and color
  glyphs. Text/shape layers remain the Phase 7 raster subset in typed documents.
- **8F / P7-03:** host-only still decoders, additional frame/multiframe sources and
  Live Photo/video rerendering. **11D / P7-23** owns named preserving format additions.
- **8G:** the separate local RAW masks and additional camera/lens enhancements
  originally named by 8D, with explicit source-coordinate/evaluation order,
  capability, settings-transfer and persistence qualification.
- **12B: shipped.** Per-entry diagnosis, removal/repair and version migration
  for invalid RAW preset libraries. THIS slice refuses the library as a whole
  and requires manual removal of a malformed/oversized/unknown-version file;
  12B-2 replaced that with a `broken` row per file, `delete_raw_preset {file}`,
  `repair_raw_preset` and schema-2-on-write. Ordinary Develop editing remained
  available throughout, and no unknown data is silently discarded: a future
  schema version is reported and never rewritten.
- **12E / P7-26 and P7-27 — DELIVERED.** Typed arbitrary structural placement and
  editing; embedded image placement takes the `position` / `layer` pair at both
  modes since [12E-2b](phase-12e-layer-ops-status.md) rather than appending at
  the top, and the retained-source and ATTACHMENT edit families are lock-gated
  at both depths there.
- **12F / P7-28:** 12F-2 is qualified within its declared scope, with
  independent review pending. It delivers retained whole-document geometry and
  group/linked-set affine and perspective at both document depths. The original
  master remains bound through crop, resize, turn, flip, straighten and filtered
  rotate; projective bindings use the conditional `RSPJ` native table. The
  owned 8A regression passes at both depths, and 12F-2's owned drive checks
  projective reopen plus crop/rotate preservation. Grouped and linked original
  masters also rerender exactly after both set maps at both fallback depths.
  Deform fields remain 12F-3;
  filtered float geometry remains outside this delivered matrix.
- **11C / P7-22:** additional profile evaluation/appearance choices, including any
  source-preserving Assign workflow. The current explicit rasterize-first rule
  protects original source interpretation.
- **13A/13C/13D / P7-31–34:** regional work, fuller cancellation (including source
  snapshot copying and fingerprinting), codec/archive compression/deduplication
  and declared large production workload qualification. 13D owns expansion of
  encoded-input, source-heavy and current native/working-capacity limits.

At completion, 8D handed off to **8E**. The boundaries above were explicit additions
to the receiving slices; they did not close the Phase 6 quality gate or another
Phase 7 carryover.

## Phase 8E — retained artwork at native depth

Completed within the [dedicated artwork qualification](phase-8e-artwork-status.md).
Both depths retain editable text/shapes, continuous normalized sRGB and attributed
colors; generated outlines use native area coverage, while intrinsic glyph assets
retain their disclosed byte precision and independent authored ink/opacity.
Editing, affine/whole-image geometry and profile changes regenerate from retained
parameters. Numeric colors, explicit Rasterize Artwork, MCP/Actions, Undo/Redo,
native v20 mixed sources and actual crash recovery all pass. Typed descriptor
presence survives admitted-read refusal, and failed restoration never publishes
a raster-only result.

Final gates: **1,102 Rust tests passed, 0 failed, 2 ignored**; format/all-target
Clippy, zero-error typecheck and Release build pass. **37 actual-source host/UI/
admission groups and 26 external MCP groups pass**. Independent exported text and
shape alpha have 4,100 and 1,986 distinct native values; 2,359,296 components survive
native reopen exactly. Final source/archive/app/probe hashes, declared layout
and curve tolerances, scripts and reports are recorded in the dedicated status.

**At the 8E handoff, P7-04 and P7-05 were verified: five rows were verified and
31 remained open, with 8F/P7-03 next.** 8G remained a separate required slice. The historical 8D counts
and measurements above remain unchanged. Current additional boundaries are
explicit: typed artwork insertion lands at the slot `position` names, exactly as
the byte path does, since [12E-2b](phase-12e-layer-ops-status.md); true
perspective requires
rasterization, with group/linked/interactive/deform geometry in 12F/P7-28. Fonts are
not embedded; saved pixels remain usable without fonts, and semantic rerenders
require supported descriptions/fonts. Larger text/canvases remain 13D/P7-34;
brush/selection byte coverage and the Phase 6 cutout gate remain separately assigned.

## Phase 8F qualification

Phase 8F completes retained host stills and rational video frames within the
[declared SDR codec scope](phase-8f-host-source-status.md#qualification), verifying
**P7-03**. Exact encoded originals and actual full decoder samples/ICC/alpha stay
independent from the document fallback depth. The prepared frame sheet, MCP and
Actions preserve source capture, fixed framing, independent masks/retouch and one
Undo; native/snapshot/crash recovery re-edit works after deleting the originals.
Changed/unknown decoders and unsupported HDR/layouts refuse new decoding while
saved rendering remains usable. Native 21/22 pools share bounded original resources
with retained RAW and preserve the existing artwork table.

Final gates include **1,123 Rust tests passed, 0 failed, 2 ignored**, clean formatting/
all-target Clippy, zero-error typecheck and Release build, **39 actual-source host/UI/
admission groups** and **26 external MCP groups**. Independent checks preserve
10,911,744 host source components, 589,824 identity-fallback components and
9,437,184 external-MCP source components exactly. The dedicated status records
final manifests, hashes, fixture provenance, process cleanup and the numerical scope.

**At the 8F handoff, six rows were verified and 30 remained open; 8G camera
controls and local RAW development were next.** Expanded multiframe ProRes cross-decoder conformance
remains unqualified at its unchanged 64-code bound in 11D/13D; actual AVFoundation
output preservation is separately exact. Broader codecs/HDR/float remain 11D/11E/
11F; typed export-packet/PPI and auxiliary-matte bridges remain 11D/9G, retained
source group/perspective/whole-document geometry remains 12F, and preset repair
remains 12B. Historical 7A–8E measurements above are unchanged.

## Phase 8G — camera controls and local RAW development

The bounded implementation adds camera-relative baseline compensation, qualified
moiré reduction and manual profile-aware vignette, plus up to four retained local
RAW developments. Radial/linear fields, ordered brush add/subtract, inversion,
density and feather use native u16 source-coordinate coverage. Each local changes
exposure offset and white-balance intent by developing the original camera bytes;
no local receives a darkened byte preview as its source.

The evaluation order is global camera/vendor lens controls for each original-derived
variant → the chosen sRGB/P3 u16 encoding → ordered encoded-RGB float coverage blend
→ calibrated manual vignette → one u16 source quantization → existing accumulated
layer affine/document fallback → layer masks and retouch. Variants share exact
source geometry and ICC. Zero coverage, disabled/identity locals and neutral
vignette have exact bypasses. Recipe 3 stores every effective control; recipes 1/2
keep their existing meanings. `RZLM` planes share the existing native 21/22 binary
pool with original RAW data, with no sample arrays in recipe JSON.

Global presets/synchronization transfer camera-relative compensation, supported
moiré and vignette while preserving each target's local IDs and masks. Explicit
local copy transfers exposure/WB intent only. UI/MCP/Actions share bounded captured
operations and one prepared publication. Same-geometry migration preserves the
planes and reviews the complete new development; unknown recipes/mask versions
keep cached native rendering and refuse semantic edits. Known malformed resources
refuse before native pixel decode. Cheap attachment kinds keep RAW masks out of
host-frame summaries, menus and recording paths even when metadata reads are denied.

Final qualification passes: **56 host/UI groups**, 577,536,000 independent SDK
component comparisons within the fixed three-code bound (maximum one),
192,512,000 exact native master components, **1,147 Rust tests / zero failures /
two existing ignored**, authoritative Swift typecheck and Release build, and
**38 external MCP groups / 760 calls** with 145,956,864 exact persisted source
components. Source/file-set and artifact audits pass; all owned processes exit. The [8G status](phase-8g-raw-enhancements-status.md) records the exact
matrix, results, artifacts and named later limitations. Earlier 8C–8F measurements above
remain historical evidence and are not replaced by the new camera comparisons.
