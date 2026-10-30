# Phase 10D: five additional native layer modes

The [threshold slice](phase-10d-threshold-modes-status.md) has since admitted
Hard Mix (with Darker/Lighter Color and Dissolve) under an explicitly versioned
strong/weak band policy with conditional native v34/schema6; the Hard Mix
boundary discussion below records why this slice refused it, and its witness
now lives in `precision_threshold_blending.rs` as an in-band weak-form case.

The [HSL slice](phase-10d-hsl-modes-status.md) has since added Hue, Saturation,
Color and Luminosity with conditional native v33/schema5 above the v31 wrapper
described below; schema4 and every older schema stay frozen.

The clipping statements below ("clipping … remain outside this slice") describe
this slice at its handoff; [the 10D clipping slice](phase-10d-clipping-status.md)
has since added typed clip chains over the same compositor and a conditional
outer v32 wrapper above mask29/blend31.

The 10A status statements below describe this slice at its handoff. Bounded
10A import was subsequently [qualified and closed](phase-10a-qualification.md);
its explicit exclusions are separate from this native-blending work.

This bounded slice adds Subtract13, Vivid Light18, Linear Light19, Pin Light20
and Divide22 for native RGBA16 raster layers and isolated groups. The shared
`blend.rs` kernels and encoded-RGB/f32 evaluation remain unchanged. Existing
native modes, ordinary byte rendering, adjustment Normal-only admission and
group-only Pass Through retain their meanings. Hard Mix21 remains unsupported;
its precision decision is documented below. Darker/Lighter Color, HSL modes,
Dissolve, clipping, styles, Blend If, knockout and external exchange remain
outside this slice. Full10D and P7-19/P7-20 remain open; Phase9I is independent.

## Meaning, host routing and persistence

Subtract clamps `backdrop − source` at zero. Vivid Light applies the existing
Color Burn kernel at `2 × source` below or at0.5 and Color Dodge at
`2 × source − 1` above0.5, preserving their endpoint precedence. Linear Light
clamps `backdrop + 2 × source − 1` to0…1. Pin Light takes the smaller of backdrop
and `2 × source` below or at0.5, otherwise the larger of backdrop and
`2 × source − 1`. Divide clamps `backdrop / source` at one; source0 explicitly
returns one, **including0/0**, preserving this application's existing kernel
contract. These encoded-domain definitions do not assert another application's
blend-space or zero-divisor compatibility. The explicit formulas and endpoints
here are the native contract.

The authoritative Rust and Swift native mode sets feed the existing Layers
popup, MCP `set_layer_properties`, Actions and atomic captured publication.
There are no new FFI signatures or edit-request schemas. Native mask coverage,
density and opacity gate contribution once; group intermediates remain f32
until final projection storage. Raw source RGB/alpha/hidden RGB, masks,
placement, descriptions, retained masters, channels, ICC/PPI and metadata
remain exact through metadata edits and persistence.

Conditional RZDC31/LBLD schema4 adds exactly13/18/19/20/22 to the previous
thirteen nondefault modes. Schemas1/2/3 remain explicitly frozen. The writer
selects the oldest sufficient blend wrapper:31,30,27,26 or no blend wrapper.
Its capacity stays `16 + 8 × nondefault_record_count` bytes. Older typed bodies
and their channel/retained/artwork tails retain exact framing.

Outside255 independently adds outer29, giving canonical
`29 → 31 → prior body/tails → LBLD4 → MOUT1`. Version31 never wraps29. Removing
the last outside255 value restores exact standalone31 bytes; removing the
last new mode restores exact prior30/27/26/older bytes, including under29.
All schemas, counts, supported modes, ordered unique postorder indexes and
hidden target kinds preflight before PNG decoding. Old schemas carrying new
modes, malformed wrapper order, unsupported records, truncation/trailing data,
capacity and cancellation refuse without changing input.

## Numerical policy and the Hard Mix boundary

Ordinary RGB components and every alpha comparison retain a0.65-native-code
bound against independently calculated unrounded f64 results, including final
u16 quantization. Subtract, Linear Light, Pin Light and Divide receive no
expanded allowance. Vivid Light inherits near-singular division sensitivity
from the unchanged Dodge/Burn kernels. Only components classified from exact
authored integer codes receive a1.6-code allowance: upper branch source≥64880,
backdrop>0 and backdrop≤2×(65535−source), or lower branch source≤655,
backdrop<65535 and65535−backdrop≤2×source. Equality deliberately includes the
exact saturation boundary. Meaningful-alpha group fixtures retain0.65.

An immutable Vivid witness uses backdrop212/source65428: its independent
reference is `65535 × 106 / 107` native codes. Immediate source neighbors65427
and65429 are also checked. The exact upper saturation witness
backdrop340/source65365 is included in the independent branch grid. The opaque
Rust grid measured maximum errors of1.4766355138 codes for Vivid Light and
0.5000152593 for Divide; Subtract, Linear Light and Pin Light were below1e−10.
These
are finite-fixture qualifications, **not universal bounds over every possible
f32 intermediate or document**.

Hard Mix's discontinuity cannot be qualified by a larger RGB tolerance. With
an opaque backdrop65534 and an isolated Normal group containing opaque0 plus
color2 at alpha32767, exact group color plus backdrop is
`1 − 1 / 65535²`; the correct Hard Mix branch is zero. Changing that alpha to
32768 makes the sum `1 + 1 / 65535²`, whose branch is one. Current f32 group
arithmetic can lose that distinction: the lower group value rounds to1/65536
and the backdrop to65535/65536, summing exactly to one. Widening only the final
comparison cannot recover information already lost in its inputs. Direct
normalized u16 threshold and neighboring-code checks did not show this issue;
the nested intermediate is material.

The dedicated regression retains exact integer numerators on both sides and
requires Hard Mix edits on rasters/groups to refuse without changing native
bytes. A future Hard Mix slice needs an explicit native precision policy that
retains sufficient precision through the contributing group and backdrop
calculations, sharing the algorithm without silently changing prior persisted
f32 evaluation. This slice makes no such change.

## Qualification scope

`precision_remaining_separable.rs` uses independent f64 equations for endpoints,
branch neighbors, asymmetric colors, every native mask code with varied tiny/
partial/opaque alpha, density/opacity, translated nested groups and Pass Through.
It verifies exact no-ops, unsupported/hidden targets, capacity/cancellation,
old-schema refusal and exact conditional downgrade. The mask-wrapper tests
retain old30 cases and add31 over independently constructed29 wrappers, plus
combined channel/artwork/retained-source tails and strict predecode refusals.

The actual Layers source probe exercises every new popup and recorded Action,
exact-owner Undo, native31 save/reopen, snapshots/private recovery, group
transitions and all five outside255 combinations. The existing four-mode and
historical probes retain their older-format expectations. Root owns independent
Python decoder/production-MCP qualification, full regression, normal Release
build and final commit receipts.

### Focused qualification receipt (2026-09-13)

- All **27** focused release tests pass: seven new five-mode/Hard-Mix-
  refusal/exact-half-group tests, three expanded mask-wrapper tests, five prior
  four-mode tests and twelve prior layer-blending tests. The initial26-case
  run and final seven-case run together cover the final source. Logs:
  `/private/tmp/rz-five-native-focused.log` and
  `/private/tmp/rz-five-native-final-test.log`.
- The final all-target Clippy, formatting, authoritative typecheck and diff
  checks pass. Logs: `/private/tmp/rz-five-native-lint-final.log`,
  `/private/tmp/rz-five-native-fmt.log` and
  `/private/tmp/rz-five-native-typecheck.log`.
- A source-only Clippy diagnostic rejected a constant assertion in the
  explanatory Hard Mix witness. The final test checks the exact rational
  numerator and f64 branch for both authored alpha values inside the same
  fixture loop. No product change was needed. The exact-half group test also
  verifies Vivid/Linear/Pin Light with source0.5, which no native u16 source code
  can express, without quantizing its intermediate.
- The seven production files are frozen in
  `/private/tmp/rz-five-native-product-freeze.json`; the full source/test/probe
  handoff is `/private/tmp/rz-five-native-handoff-freeze.json`. Root's integrated
  product files match those hashes. Independent review reports no blocking
  finding in the product, frozen schemas, tests or source-probe workflow.

### Combined production-archive source probe (2026-09-13)

The final root source probe passed `native layer blending, mask outside
coverage`, linking the production normal-LTO archive without rebuilding Rust.
All **1,073** physically copied source/test/example/header/helper/build/archive/
production-executable inputs were unchanged before and after. The actual popup
and recorded Actions exercise the five new modes; native31 reopen, snapshots,
private recovery, atomic Hard Mix refusal, group transitions and all nine
outside255 combinations with blend30/31 pass. The byte28/native29 mask suite's
density/feather UI, Actions, exact-owner Undo/Redo, native reopen, snapshots and
private recovery also pass.

Independent decoding verifies all five standalone31 saves and all five
mask29→blend31 saves: full raw RGBA16 sources, hidden RGB/alpha, every mask
property/sample, outside255, names/opacity and exact authored ICC bytes.
Seven snapshot/recovery `image.rz` payloads byte-match those independently
decoded saves, including native31 Vivid recovery and29→31 Vivid snapshot.

- Receipt, physical inputs and artifact hashes:
  `/private/tmp/rz-five-root-source-probe/`.
- Production normal-LTO archive SHA-256:
  `bb785a94f8ead02f54c3a5c0d930e73ee220d9194e48413c24e5e76eee335b09`.
- Source-probe executable SHA-256:
  `cc1e648109abd16f0b6b103dbbfce4d8aba2670386aadcb6e4f8d24cfc449250`.
- Captured production executable SHA-256:
  `b4350df73f3a44c1dd7b95279651152dbe57991a3ee3c6f274518ad703ac9a5a`.

The owned probe exited, using dummy credentials and private Actions/recovery/
preset stores. This receipt qualifies the captured source-probe executable;
root's full Release-app MCP and regression receipts remain separate.

### Integrated Release-app qualification (2026-09-13)

The normal-LTO Release app and authoritative Swift typecheck pass, as do root
formatting and all-target Clippy. The immutable app is
`/private/tmp/rz-five-root-release-candidate/Rasterize.app`; its executable and
archive hashes match the source-probe receipt above. All 683 captured product
inputs and 1,071 source/test/example/helper/build inputs remain unchanged.
The separate Rust test-input manifest contains 449 files. Build, typecheck,
lint, input manifests and full-suite receipts use the
`/private/tmp/rz-five-root-` prefix.

`verify_phase10d_five_modes.py` passes all five modes at opacity 1 and 0.625,
plus all five combinations with independently authored outside255 masks.
Raw source/master RGB, hidden RGB, alpha, masks, profile and metadata remain
exact. Recorded Actions, one-edit Undo/Redo, native31 and outer29 reopen, and
byte-exact downgrades through blend30/27/26 and the prior body pass. Three
compound refusals (Pass Through on a raster, an oversized name, an invalid
opacity) publish no partial property edits; the Hard Mix and Dissolve
refusal cases were retired when the threshold slice admitted those modes.
Results and immutable fixtures are in `/private/tmp/rz-five-root-mcp/`.

Measured maximum error in native codes against unrounded f64, including final
quantization:

| Mode | Opacity 1 | Opacity 0.625 |
| --- | ---: | ---: |
| Subtract | 0.500023 | 0.500000 |
| Vivid Light | 1.478449 | 0.922898 |
| Linear Light | 0.500023 | 0.375067 |
| Pin Light | 0.500023 | 0.465745 |
| Divide | 0.505855 | 0.500000 |

Values are rounded upward for reporting; the result JSON retains every measured
component. Only the classified Vivid Light components receive the 1.6 bound;
ordinary components and every alpha comparison still require 0.65. Exact source
and persistence checks have no numerical tolerance.

The same Release app also passes the prior four-mode, seven-mode, basic-blend
and mask-foundation workflows, including the foundation's ten cases and eight
nested-wrapper checks. Their receipts are
`/private/tmp/rz-five-root-{four,separable,basic,foundation}/result.json`.
The independent decoder preserves all prior malformed cases and adds schema4's
53 malformed/atomic cases, 15 frozen-old-schema refusals, eleven combined mask/
blend files with 44 malformed variants, and five standalone31 files with 15
whole-file old-schema refusals. Its log is
`/private/tmp/rz-five-root-decoder.log`; helper hashes are captured in
`/private/tmp/rz-five-python-handoff-hashes.json`.

Independent production, test, helper, UI-probe and documentation review found
no remaining blocker. All owned test apps and probes exited. The broader
10A/10D, P7-19/P7-20 and 9I prerequisite gates remain open; the carryover ledger
is unchanged at 18 verified and 18 open.

The final complete root Rust run passes **1,485 tests, zero failures and two
pre-existing ignored tests across 153 result groups**. It uses the existing
isolated no-LTO Release test cache with serialized execution; the shipping app
and source probe use the separately qualified normal-LTO archive. Full log:
`/private/tmp/rz-five-root-rust.log`. All 449 Rust inputs and 1,071 combined
inputs still match their captured hashes after the run. No test assertion,
fixture or threshold was skipped or weakened to obtain this result.
