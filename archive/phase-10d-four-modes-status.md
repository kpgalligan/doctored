# Phase 10D: four additional native layer modes

The 10A status statements below describe this slice at its handoff. Bounded
10A import was subsequently [qualified and closed](phase-10a-qualification.md);
its explicit exclusions are separate from this native-blending work.

This bounded slice adds Color Dodge, Color Burn, Addition (the existing menu
name is **Linear Dodge (Add)**), and Linear Burn to native RGBA16 raster layers
and isolated groups. Existing Normal, Multiply, Screen, seven separable modes,
and group-only Pass Through retain their meanings. Adjustments remain
Normal-only; ordinary byte rendering and its mode vocabulary are unchanged.
Other layer modes, clipping, styles, Blend If, knockout and Phase 9I remain
outside this slice.

The later [five-mode continuation](phase-10d-five-modes-status.md) adds a separate
v31/schema4 while preserving every format and numerical receipt below. Its
Hard Mix precision finding keeps that mode outside the qualified native set.

## Rendering and host path

The implementation reuses `blend.rs` without changes. Color Dodge/Burn follow
[W3C endpoint precedence](https://www.w3.org/TR/2024/CRD-compositing-1-20240321/#blendingcolordodge);
Addition clamps the encoded sum at one, and Linear Burn clamps the encoded sum
minus one at zero. Native mask coverage and opacity gate source alpha once.
Groups retain f32 intermediates; only the final requested projection stores
u16. Blend edits preserve raw RGB, source alpha, hidden RGB, masks, placement,
metadata/profile/PPI, retained source assets and artwork descriptions.

The authoritative native sets remain in `precision_layer_blend.rs` and
`NativeLayerBlending.swift`. Host eligibility, facade metadata capture, the
Layers popup and capability reporting consume the same host predicate. The
existing version1 metadata edit and atomic property preparation support MCP,
Actions and central Undo without new FFI signatures or operation schemas.
The old unsupported-mode verifier case now uses Dissolve.

## Conditional format and compatibility

The persisted mode sets are explicitly frozen:

| Blend wrapper version | LBLD schema | Allowed nondefault modes |
| --- | --- | --- |
| 26 | 1 | Multiply1, Screen2 |
| 27 | 2 | Modes1–9 only |
| 30 | 3 | Modes1–9 plus Color Dodge10, Color Burn11, Addition12, Linear Burn15 |

The current editable set cannot silently widen an older schema. The writer
selects v30 only when one of the four added modes is present; removing those
modes restores exact v27, v26 or older bytes as applicable. Table framing and
capacity remain `16 + 8 * record_count` bytes. Supported older typed bodies,
including channel properties and retained/artwork tails, keep their framing.
Normal and Pass Through stay in their older record representation.

All schemas, supported modes, counts, ordered unique postorder references and
hidden target kinds preflight before PNG decoding. Old schemas containing a
new mode refuse. Truncation, trailing data, incompatible hidden adjustments/
Pass Through targets, capacity and cancellation retain atomic refusal.

The implemented integration with independent external-mask storage is outer29,
then blend30, then the prior typed body and tails, LBLD schema3, then MOUT
schema1. Version30 never wraps29. The mask reader explicitly admits30 as a29
inner version; host routing admits byte28 and typed29/30. Removing the last
outside255 value restores exact standalone30 bytes; removing the last new mode
restores exact29-over27/26/older bytes. Each wrapper remains independently
optional, and the frozen old blend schemas retain their original meaning.

## Numerical qualification policy

The unchanged f32 Color Dodge/Burn kernels can amplify rounding near a singular
division. For the explicit opaque Color Dodge witness Cb=106/65535 and
Cs=65428/65535, the independent f64 result is 65535×106/107 codes. The f32 result
has more than one code of error against that unrounded value, including final
quantization. Tests keep that witness and its immediate source-code neighbors.

The finite branch-neighbor fixture allows at most **1.6 native codes** against
unrounded f64 for Dodge/Burn division-sensitive cases. Addition, Linear Burn,
ordinary meaningful-alpha group fixtures and all alpha comparisons retain
**0.65**. Both the branch grid and native-mask sweep classify each component from exact
authored codes; only near-singular Dodge/Burn division and its exact saturation
boundary receive 1.6. Ordinary components and sums retain 0.65. These are qualification bounds for the declared finite samples, **not a
proof over every possible f32 intermediate or document**. The production helper
separately reports measured unrounded-reference error including quantization
and difference from rounded f64; only actual near-singular fixture (including exact saturation boundary)
components receive its 1.6 allowance.

## Qualification

`precision_four_blending` covers endpoint precedence, saturation neighbors,
asymmetric colors, all 65,536 native mask codes with partial/tiny alpha,
density/opacity, translated children in meaningful-alpha isolated groups,
Pass Through transitions, ancestor preservation, strict FFI planning, exact
no-ops/refusals and conditional v30 downgrade. Existing layer-blend tests also
exercise v30 over channel/retained/artwork bodies. The independent decoder
retains old malformed gates and adds schema3 plus explicit old-schema refusal.

The actual Layers source probe adds each of the four popup choices, records
and replays its Action, restores the exact owner with Undo, checks atomic
later-property refusals, and verifies Color Burn v30 reopen/snapshots/recovery
and Color Dodge group transitions. The production helper
`verify_phase10d_four_modes.py` reuses unchanged seven-mode fixture framing,
transport and history helpers with independent four-mode equations and full/
fractional opacity fixtures. Root integration owns its production execution,
full regression and final Release build.

The receipts below distinguish the original isolated slice from its subsequent
integration with the committed mask foundation. No broad phase or
exchange-compatibility completion is asserted.


### Isolated qualification receipt (2026-09-13)

- All **17** focused release tests pass: five four-mode tests and twelve
  existing layer-blending tests, including v30 channel/retained/artwork tails.
  Log: `/private/tmp/rz-four-modes-tests-qualified.log`.
- The finite opaque branch grid measured these maximum absolute errors from
  unrounded f64, **including final u16 quantization**: Color Dodge
  **1.4766355138 codes**, Color Burn **1.3888888887 codes**; Addition and
  Linear Burn were below1e-9 codes. These measurements do not establish a
  universal error bound. The per-component limits and ordinary/group/alpha
  limits above remain enforced independently.
- All-target Clippy and formatting pass:
  `/private/tmp/rz-four-modes-lint-final.log` and
  `/private/tmp/rz-four-modes-fmt-final.log`. Authoritative typecheck passes:
  `/private/tmp/rz-four-modes-typecheck.log`.
- The independent decoder passes eleven schema1, twenty schema2 and
  twenty-seven schema3 malformed/atomic-refusal cases, plus eight explicit
  old-schema/new-mode refusals. Log: `/private/tmp/rz-four-modes-decoder.log`.
- The refreshed AppKit source probe passes using the normal-LTO archive.
  All **1,031** physically copied app/core/test/header/helper/build/archive
  inputs are unchanged before and after. Its independent native30 decode
  verifies both layers' full source samples, alpha and hidden RGB, mode/name/
  visibility and exact authored ICC bytes. Receipt, physical inputs and artifact
  hashes: `/private/tmp/rz-four-modes-source-probe-final/`.
  Probe directory:
  `$TMPDIR/rasterize-selection-probe-3b21p5kw/`.
- Normal-LTO archive SHA-256:
  `e14616aecd5ccfda89fd435eb8db6ed99efd5672e61203f85ae0b969156e6d14`.
  Source-probe executable SHA-256:
  `bbecf10d50c8ee70265d8de34a168c2e93c886b4d62f7b7ecdb1d9fae4c9c930`.

Independent review requested narrowing the numerical allowance to specific
Dodge/Burn components. The final tests classify authored integer codes,
include the exact saturation boundary and its neighbors, and use different
alpha cycles so the native-mask sweep includes meaningful coverage on both
layers. The refreshed probe captures these final inputs. The reviewer reports
no remaining blocking finding. Both owned probe processes exited; credentials,
Actions/recovery/preset stores and preferences were private test values.

The original production helper was syntax/import checked for root execution;
the integration below extends its existing checks without changing historical
seven-mode expectations.

### Mask-wrapper integration receipt (2026-09-13)

The integration worktree is based on mask-foundation commit `a0715d9`. The
bounded production change adds30 to the typed mask wrapper's explicit inner
whitelist, retains canonical29→30 ordering, and routes the union of supported
native versions through the host. It does not change the mask model, blend
kernels or ordinary byte rendering.

- All **28** focused release tests pass: eight existing mask-foundation tests,
  three dedicated cross-wrapper tests, five four-mode tests and twelve prior
  layer-blending tests. The cross-wrapper tests independently construct exact
  wrappers, exercise all four modes with outside255, assert both directions of
  exact downgrade, and retain raw RGBA/hidden RGB/masks/profile/PPI/metadata.
  A combined29→30→25→20 fixture preserves channel properties, artwork and
  retained sources. Hidden malformed tables, unsupported nesting, corrupt-PNG
  preflight ordering, capacity and cancellation refuse without changing input.
  Log: `/private/tmp/rz-four-mask-integration-tests-final.log`.
- Formatting, all-target Clippy and authoritative typecheck pass. Logs:
  `/private/tmp/rz-four-mask-integration-fmt.log`,
  `/private/tmp/rz-four-mask-integration-lint.log`, and
  `/private/tmp/rz-four-mask-integration-typecheck.log`.
- The independent decoder retains every prior schema gate and adds six
  independently authored mask29/blend26/27/30 compositions and 24 malformed
  combined-wrapper cases. Log:
  `/private/tmp/rz-four-mask-integration-decoder.log`.
- The actual Layers source probe passes all four controls with outside255,
  canonical29/30 native reopen, exact source pixels and mask samples, snapshot
  restore and exact-owner Undo. The existing blend MCP/Actions/atomic-refusal/
  recovery checks also pass. All **1,038** physically copied app/core/test/
  header/helper/build/archive inputs are unchanged before and after. An
  independent decoder checks all four saved29→30→13 files, both raw sources,
  every mask field and exact authored ICC bytes. Receipt and captured inputs:
  `/private/tmp/rz-four-mask-integration-source-probe/`.
- The local normal-LTO archive SHA-256 is
  `b14a0c5eaa6a1369f5ed9217d79c2b16b0b6ea9d83f841072b546eb54503e906`;
  source-probe executable SHA-256 is
  `d933a1efb033f06c0807d9f7dca5e0cd45c1b18e2b5db8716e87da22927f6159`.

The production helper now authors independent native29 fixtures with
outside255 and exercises each four-mode29/30 edit, saved raw pixels/masks,
exact Undo/Redo, Actions, reopen, and exact nested27/26 downgrade. It retains
the original asymmetric endpoint/neighbor numerical fixtures and their
component-specific bounds. Root owns the final integrated production MCP,
full regression, normal Release build and commit receipts.

### Combined production-archive source probe (2026-09-13)

The final combined root sources passed the actual `native layer blending,
mask outside coverage` source probe using the production normal-LTO archive,
without rebuilding Rust. All **1,042** physically copied app/core/test/header/
helper/build/archive inputs were unchanged. Both suites passed: the four-mode
outside255 composition checks above, and byte28/native29 density/feather UI,
Actions, exact-owner Undo/Redo, raw source preservation, native reopen,
snapshot and private recovery. The independent decoder again verified each
saved29→30→13 file, both raw sources, all mask properties and authored ICC
bytes. Both owned integration probe processes exited.

- Receipt, physical inputs and artifact hashes:
  `/private/tmp/rz-four-mask-root-source-probe/`.
- Production archive SHA-256:
  `afcef04829d1e5f1d84c19b7c86168580f1f21daa88a026dd2239885a38a89f9`.
- Source-probe executable SHA-256:
  `64db3298e658083985f436f48c1f00680d0232768221eda1c274804a1c0f27be`.

This source-probe receipt is separate from root's full Release-app MCP and
regression receipts; it does not assert completion of broader phases.

## Integrated Release qualification (2026-09-13)

- The complete Rust suite passes **1,478 tests, zero failures and two existing
  ignored tests**, across152 result groups. The shared Release test cache uses
  LTO disabled and serialized tests; all cancellation checks remain enabled.
  Log: `/private/tmp/rz-four-modes-root-rust.log`.
- Authoritative `make typecheck`, normal-LTO production `make app`, all-target
  `make lint` with warnings denied, formatting and whitespace checks pass.
  Logs: `/private/tmp/rz-four-modes-root-{typecheck,app,lint}.log`.
- The [production verifier](../scripts/verify_phase10d_four_modes.py) passes all
  four modes at opacity1 and0.625. Exact raw RGBA16, hidden RGB/source alpha,
  retained masters, profiles, placement and metadata survive atomic edits,
  one-edit Undo/Redo, Actions and native30 reopen. Against independently rounded
  f64 references the maximum difference is **one u16 code** on these fixtures.
  Including final quantization, the largest unrounded-f64 difference is
  **1.476635514 codes**, on the explicitly retained near-singular Color Dodge
  witness; ordinary components keep their tighter bound.
- Each mode also passes independently authored outside255 masks through
  mask29/blend30 persistence, exact mask/source/profile retention, Actions,
  one-edit Undo/Redo and reopen. Nested27/26 downgrades restore exact bytes.
  These masked projections have maximum unrounded-f64 error **0.500850155
  codes**, within the unchanged0.65 bound. Both groups of production results
  are in `/private/tmp/rz-four-modes-root-mcp/result.json`.
- The same immutable app passes all53 PSD mask fixtures, the prior seven-mode
  verifier, and the basic native blend verifier including atomic compound
  refusals. Receipts: `/private/tmp/rz-four-modes-root-psd/result.json`,
  `/private/tmp/rz-four-modes-root-separable/result.json`, and
  `/private/tmp/rz-four-modes-root-basic/result.json`.
- Independent decoder checks again pass:11 schema1 malformed cases,20 schema2
  cases,27 schema3 cases, eight frozen-old-schema refusals, six valid combined
  wrappers and24 malformed-wrapper cases. Log:
  `/private/tmp/rz-four-modes-root-decoder.log`.
- All860 captured integrated compiler/test inputs and eight verification helpers
  remain unchanged. The handbook update is documentation and is excluded from
  compiler inputs. Captures: `/private/tmp/rz-four-modes-root-inputs.json` and
  `/private/tmp/rz-four-modes-root-helper-hashes.json`.
  Production executable SHA-256:
  `f9336db3f83bb2beccdceca6af6c561bb44de5c68ae49f9ceda7c2bb599569d6`;
  normal-LTO archive SHA-256:
  `afcef04829d1e5f1d84c19b7c86168580f1f21daa88a026dd2239885a38a89f9`.
  Immutable app: `/private/tmp/rz-four-modes-root-release-candidate/Rasterize.app`.

Independent review closed a rich-fixture source-authoring mistake and aligned
probe check-name whitespace handling before final qualification. No product
change was required after the production source freeze. The final complete
suite includes the corrected fixture. All owned production and source-probe
processes exited, using dummy credentials, private local ports and private
Actions/recovery/preset stores.

This completes the four-mode slice and its composition with independent masks.
Remaining native modes, clipping, styles, Blend If, knockout, broad PSD exchange,
full10A/10D and P7-19/P7-20 remain open. The separate P7-12/9I prerequisite
decision is unchanged; the carryover ledger remains18 verified and18 open.
