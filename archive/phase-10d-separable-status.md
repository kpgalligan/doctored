# Phase 10D: seven additional native separable layer modes

The 10A status statements below describe this slice at its handoff. Bounded
10A import was subsequently [qualified and closed](phase-10a-qualification.md);
its explicit exclusions are separate from this native-blending work.

The later [four-mode qualification](phase-10d-four-modes-status.md) adds
Color Dodge/Burn and Linear Dodge/Add/Linear Burn with conditional v30 records
and mask29 composition. This document preserves the earlier seven-mode scope
and its original qualification receipts.

This bounded follow-on adds Overlay, Soft Light, Hard Light, Darken, Lighten,
Difference and Exclusion to native RGBA16 raster layers and isolated groups.
Normal, Multiply, Screen and group-only Pass Through retain their shipped
meaning. Adjustments remain Normal-only. This does not complete Phase 10D,
P7-19 or P7-20; other layer modes, nonseparable blends, Dissolve, clipping,
styles, Blend If and knockout remain outside this slice. Phase 9I is unchanged.

The existing `blend.rs` kernels implement the
[W3C separable blend equations](https://www.w3.org/TR/2024/CRD-compositing-1-20240321/#blendingseparable).
Encoded document RGB and native coverage use the existing f32 source-over
path. The compositor retains f32 color and alpha through isolated and nested
group boundaries and stores u16 only at the requested final projection. Source
pixels, alpha, hidden RGB, independent masks, profile, metadata, artwork and
retained-source descriptions are unchanged by blend metadata edits.

`precision_layer_blend.rs` owns the Rust mode set used by target admission and
persistence. `NativeLayerBlending.swift` owns the host set used by the existing
Layers popup, capability reporting and MCP/Actions eligibility. The version1
`layer_blend` request and existing atomic throwing property preparation are
unchanged. Compound refusals publish nothing; Undo/Redo uses the central owner
history. No FFI signature or Actions schema changes.

## Conditional native persistence

Native v26 and LBLD schema1 continue to mean Multiply/Screen only. A document
with any of the seven added modes writes v27 and LBLD schema2. Both wrappers
contain the complete prior typed version/body (including v25 channel metadata),
then `LBLD`, u32 schema, u32 count and strictly increasing u32 postorder-index /
u32 mode pairs. Schema2 permits exactly modes1–9. Normal and Pass Through
retain the older record representation and never appear in the table.

The table costs the existing `16 + 8 * record_count` encoded bytes. Documents
with only Multiply/Screen still write v26/schema1; removing those modes too
restores the exact prior default-only writer. A schema1 file containing an
added mode refuses rather than acquiring a new meaning. Unknown schemas,
modes, counts, duplicate/unordered/out-of-range indices, trailing/truncated
records and incompatible hidden targets refuse before PNG decoding. Capacity
and single-use cancellation retain their existing admission boundaries. The
app routes v27 to the preserving native reader for reopen and archives.

## Qualification

The dedicated `precision_layer_blending` release target independently checks
f64 source-over arithmetic for every native mask code and all nine nondefault
modes, with partial/tiny source and backdrop alpha, density and opacity. Explicit
neighboring u16 codes straddle 0.25 and 0.5 and include endpoints and asymmetric
channel reversals. Meaningful-alpha isolated groups include translated children,
a native mask and an enclosing Pass Through gate for all nine modes. A separate
subcode-alpha witness would visibly fail if an intermediate group stored u16.

Tests also check strict FFI planning/editing, exact no-ops/refusal purity,
active Soft Light render cancellation, source sharing and hidden RGB/alpha,
metadata/PPI/profile preservation, conditional v27 over v25 and retained/artwork
bodies, exact v26 restoration and independently authored old schema1 framing.
Hidden adjustment/Pass Through table targets and malformed version/schema/mode/
count/reference records refuse before an intentionally corrupt PNG is decoded.

The independent Python decoder retains the eleven old schema1 malformed cases
and adds twenty schema2 malformed/atomic-refusal cases, accepting each of the
nine supported table modes. The AppKit source probe exercises each new actual
Layers popup item, records/replays its property Action and checks exact owner
Undo. It additionally exercises MCP compound-property refusals, native v27
reopen, snapshots, private recovery and isolated/Pass Through transitions.

Final isolated and integrated receipts are recorded after the qualification
runs. The root integrator owns the complete Rust regression and normal
production Release/MCP verification; source-probe results do not substitute
for that production qualification.


### Isolated qualification receipt (2026-09-13)

- All twelve dedicated release Rust tests pass, including all 65,536 coverage
  codes for each of nine modes and meaningful-alpha group mode qualification.
  All-target Clippy, formatting and diff checks pass. Logs:
  `/private/tmp/rz10d-separable-tests-qualified.log`,
  `/private/tmp/rz10d-separable-lint.log`, `/private/tmp/rz10d-separable-fmt.log`.
- Final authoritative typecheck passes with the shared facade predicate:
  `/private/tmp/rz10d-separable-typecheck-final.log`.
- The AppKit source probe passes, linking the normal-LTO Rust archive. Its
  actual seven popup choices, MCP/Actions, exact owner Undo, compound refusal,
  v27 reopen, snapshot/recovery and group transition checks all pass. All
  **1,025** physically copied app/core/test/header/helper/build/archive inputs
  are identical before and after. Receipt and input snapshots:
  `/private/tmp/rz10d-separable-source-probe-v5/`; executable and run log:
  `$TMPDIR/rasterize-selection-probe-js033n0g/`.
- The independent decoder passes **eleven schema1 and twenty schema2**
  malformed/atomic-refusal cases, then parses the actual source-probe native27
  save and checks all raw u16 codes in both layers, including source alpha and
  hidden RGB, plus its mode/name/visibility. Receipts:
  `/private/tmp/rz10d-separable-decoder.log` and
  `/private/tmp/rz10d-separable-independent-native.json`.
- Normal-LTO archive SHA-256:
  `6b87fdda531cf49143fe2de46c5f782d6dfb25f24ea3f90993e3383d6c396763`.
  Qualified source-probe executable SHA-256:
  `c7e863c40330e0a40942cc0940c72e3c41ac82780594f9af14f2df429ec63b26`.

The first source-probe invocation lacked the generated Info.plist; the next
sandboxed launch lacked graphics-service access. An owned normal-access run
then exposed a basic-only numeric whitelist in facade metadata parsing. That
parser now consumes the same host mode/target predicate as edit eligibility,
and the probe reuses the shared error interceptor to report UI refusals without
waiting in a modal alert. Only the final unchanged-input run above is qualified.
An independent review also requested meaningful-alpha group mode coverage in
addition to the subcode-alpha quantization witness; that coverage is present
and passes. Final independent review reports no remaining blocking finding.

Every owned probe process exited or was explicitly stopped. Verification used
dummy credentials and private Actions, recovery, preset and preferences stores.
The integrator's complete regression and final production Release/MCP receipts
are recorded separately below.

### Integrated qualification (2026-09-13)

- Full Rust regression: **1,450 passed, 0 failed, 2 existing ignored**, across
  148 result groups. Log: `/private/tmp/rz10d-separable-root-rust.log`.
  This complete run used the existing Release test cache with LTO disabled
  and local socket access; the dedicated twelve-test handoff above used
  normal-LTO Release.
- Authoritative `make typecheck`, normal-LTO `make app`, all-target `make lint`
  with warnings denied, formatting and whitespace checks pass. Logs:
  `/private/tmp/rz10d-separable-root-typecheck.log`,
  `/private/tmp/rz10d-separable-root-app.log` and
  `/private/tmp/rz10d-separable-root-lint.log`.
- The immutable production Release app passes the
  [seven-mode verifier](../scripts/verify_phase10d_separable.py). Its asymmetric
  4×3 RGBA16 fixtures include branch neighbors, endpoints, fractional opacity
  and zero/tiny/partial alpha. Every mode's saved projection matches the
  independent f64 reference exactly on these samples (maximum error **0 u16
  codes**, admitted tolerance at most1). Raw layer pixels and the retained
  embedded master, hidden RGB/alpha, profile, placement, asset bindings and
  resources remain exact. Compound metadata edits create one Undo; Undo restores
  the complete baseline native bytes, and Redo, Actions replay and v27 reopen
  restore the edited bytes exactly. Receipt:
  `/private/tmp/rz10d-separable-root-mcp/result.json`.
- The same app passes the existing basic native blend verifier, including
  v26/schema1 persistence, independent pixels and six compound refusals that
  preserve native bytes, history, selection and dirty state. Receipt:
  `/private/tmp/rz10d-separable-root-basic-mcp/result.json`.
  All13 admitted-raster/refusal PSD fixtures also pass on this final app,
  preserving the preceding 10A commit's exact storage, signed placement,
  Undo/Redo, native reopen and explicit saved-composite behavior. Receipt:
  `/private/tmp/rz10d-separable-root-pixels-mcp/result.json`.
- The final combined-source AppKit probe passes all seven actual popup items,
  MCP/Actions, exact owner Undo, compound refusal, v27 reopen, snapshots,
  private recovery and isolated/Pass Through transitions. All **1,029**
  physically captured app/core/test/header/helper/build/archive inputs remain
  unchanged. Independent decoding verifies every raw sample and the exact
  authored ICC profile. Receipt and input snapshots:
  `/private/tmp/rz10d-separable-root-source-probe/`.
  Probe executable SHA-256:
  `21be8fbb8247e34ecbcdd53ec088110744fc1307fca0af288a5d360540389216`.
- The independent decoder again passes all **eleven schema1 and twenty schema2**
  malformed/atomic-refusal cases. Log:
  `/private/tmp/rz10d-separable-root-decoder.log`.
- All **1,075** captured integrated core/app/build/test/script inputs remain
  unchanged through the production build and checks. Input hashes:
  `/private/tmp/rz10d-separable-root-inputs.json`. Production executable SHA-256:
  `d4082095a80c34b6670941228a4cca2554dc712106727f9084aa6df7434df984`;
  normal-LTO archive SHA-256:
  `1b12cc951c730cf65deefcbe893c7673bdf2931a8f8ed65fa9d5626a1e55e8f9`.
  App: `/private/tmp/rz10d-separable-root-release-candidate/Rasterize.app`.

Independent review confirms the final implementation/test/probe files match
the reviewed handoff, with the preceding PSD slice preserved. Source-probe
capture initially encountered restricted filesystem flags while copying a
system ICC file; preserving its exact bytes without those flags corrected
the capture before any probe launched. All owned app/probe processes exited;
each used dummy credentials and private test stores. Qualification documents
were updated after the build without changing compiled inputs.

This qualifies the seven added native modes within the stated scope. Full10D,
P7-19/P7-20, whole10A and the separate P7-12/9I cutout gate remain open; the
carryover ledger remains18 verified and18 open. No PSD exchange or additional
style/clipping support is implied.
