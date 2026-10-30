# Phase 10A: bounded import qualification

**10A import fidelity is qualified and closed within the contract below**, as
of 2026-09-13. Qualification began at baseline `1992d43` and includes both fixes
found by independent application evidence. Earlier slice records remain
historical evidence; this record owns the final 10A status. No required work
remains within this accepted import subset.

## Accepted import contract

| Area | Accepted behavior | Required evidence |
| --- | --- | --- |
| Container | Classic PSD, 8-bit RGB or grayscale, admitted Raw/PackBits channels and saved composite. Grayscale color samples become equal RGB components. | Independent application files, existing codec/framing tests, explicit unsupported choices. |
| Color and resolution | Exact supported embedded RGB ICC bytes and declared print resolution; absent ICC explicitly assumes sRGB. Invalid/unsupported profiles or resolution refuse. | Independent author metadata, resource bytes, native save/reopen and existing malformed-resource tests. |
| Raster storage | Full positive signed rectangles, including off-canvas samples, straight RGB beneath zero alpha, partial alpha and absent-alpha opacity. | Exact independently authored/decoded raw planes, offsets and native round trips; synthetic endpoint and malformed-input tests. |
| Folders | Bottom-first tree order, names, visibility, opacity, recognized blend keys, Normal isolation/Pass Through and disclosure state, within ten retained levels. | Independent application structure and bounded render comparisons; existing independent group arithmetic and depth tests. |
| Ordinary masks | One `-2` Raw/PackBits channel with a 20-byte body, outside coverage 0/255 and enabled state; absolute raster/folder placement or raster-relative placement normalized once from its original origin. Imported masks are explicitly unlinked, density 1, feather 0. | Independent application mask files where this exact form is authored; exact independent mask samples/placement and native persistence; existing absolute/relative, disabled, outside and malformed-input corpus. Since 2026-09-16 also a Photoshop-authored fixture rendered in BOTH directions at 0-2 codes, with Photoshop's own reading of the relative flag measured and recorded as a divergence ([Photoshop appearance agreement](phase-10a-relative-masks-status.md#photoshop-appearance-agreement-run-3-2026-09-16)). |
| Compatibility choice | Report before editing; explicitly select layers or the independently admitted saved composite. A layer failure never silently chooses the composite. Observed excluded markers identify the affected record; general limitations remain distinguishable from observed losses. | Independent excluded-feature files, bounded marker tests, actual UI/MCP, stale fingerprint and failed-open state checks. |
| Editing and persistence | Imported supported state remains editable through existing central Undo and mask/property Actions; `.rz` save/reopen retains exact source/mask samples, properties, profile and resolution. | Existing workflow qualification plus independent application files exercised in the production app. |
| Admission | Existing canvas, raw-plane, aggregate, record, metadata, nesting and conservative 2 GiB layered-decode limits stay enforced before expensive decode/allocation. | Existing boundary/refusal suites and unchanged-document checks. This does not reserve the initial file read or unrelated host documents. |

## Independent appearance gates and comparison domains

The layered two-code gate below was fixed before measurement. After the saved
preview diagnosis, the merged-reader and black/white comparisons described
below were declared before running the corrected decoder. They retain the same
two-code bound and preserve the failed initial assumption in its diagnostic log;
they were not part of the original reference-generation protocol.

Author small documents and reference renders in a separate image editor. Record
its version, authoring procedure, source and reference hashes, and the exact
properties the editor wrote. Do not substitute Rasterize output for reference
pixels or modify the exported PSD to make it fit the admitted contract.

For finite layered RGB examples using encoded Normal, Multiply, Screen and ordinary
group/mask opacity, require maximum absolute error **at most 2 byte codes per
RGBA component** against the independent reference. Record maximum and mean
absolute errors, and require alpha to remain exactly 255 on fixtures authored
over an opaque backdrop. Raw source/mask samples, geometry, supported metadata
and native persistence have **exact** gates; the appearance tolerance does not
apply to them. Compare file/document RGB codes without an incidental display
profile conversion. A failure must be repaired or recorded as a failed gate;
the bound must not be widened after seeing results.

The first transparent-preview comparison exposed an oracle convention: GIMP
keeps RGB under zero rendered alpha, while Rasterize's established compositor
emits zero RGB there. Appearance comparisons therefore canonicalize RGB **only
where both rendered alphas are zero**. They retain the original unnormalized
errors and count those pixels in the receipt. Every source/native RGBA sample
still has the exact gate, alpha is compared everywhere, and the two-code bound
is unchanged for every RGB sample with either alpha greater than zero. Independent
review confirmed that joint-zero-alpha RGB contributes nothing on any backdrop;
the production compositor was not changed to match those unused preview codes.
The initial failed diagnostic remains in
`/private/tmp/rz-10a-external-tail-diagnostic.log`.

The finite appearance corpus qualifies its declared combinations. Preserving a
recognized PSD blend key does not promise universal Photoshop appearance under
every blend setting, color policy or advanced construct. Existing independent
kernel and group references remain the evidence for Rasterize's other mapped
byte blend semantics.

## Explicit exclusions and ownership

The following are intentional limits, not unfinished items hidden inside 10A:

- PSD export and independent reopening of a Rasterize-written PSD belong to
  **10B**. Genuine 16-bit PSD, selected editable styles/adjustments/type and
  their preserve/rasterize/refuse rules belong to **10C**. Native compositing
  migration belongs to **10D**. None is a prerequisite for this import closeout.
- Relative folder masks, extended/real-user/vector masks, density/feather,
  rendered/inverted/unknown flags, unsupported mask codecs and actual PSD mask
  linkage remain excluded. Recognized cases produce explicit losses; malformed
  admitted ordinary data refuses layers. Future mask extensions need their own
  chosen contract and independent evidence.
- ZIP/ZIP-prediction, unsupported color/depth, empty raster records and unknown
  or malformed required framing refuse the affected choice. PSB and full Smart
  Object interchange remain separately chosen work.
- Group clipping, Blend If, fill-opacity/knockout and other advanced blending,
  live descriptions/effects, locks, guides, ruler origin, saved auxiliary
  channels and capture metadata are not promised editable imports. Detection
  reports presence and discarded semantics; it does not evaluate their payloads.
  Unknown opaque records remain subject to the explicit general limitations,
  not a claim of exhaustive feature detection.
- Folders beyond ten levels dissolve with a report; pathological nesting above
  the bounded parser cap refuses. The current native limits are unchanged.

This contract closes a defined import subset. Whole Phase 10 exchange still
requires its separately assigned export and advanced/native work.

## Qualification receipts

### Independent application corpus

The [retained corpus and reproduction procedure](../scripts/psd-exchange-probe/README.md)
contain ten unmodified GIMP 3.2.6 PSD exports, author/reopened layer records,
profile bytes and two independently rendered PNGs per case. GIMP authors the
document, exports PSD, deletes the author document, then reopens the original
PSD through its layered loader and renders from those layers. Rasterize is not
involved in creating the reference. The 60 small artifacts are committed under
`core/tests/fixtures/psd_10a/`.

An additional 30 files under `core/tests/fixtures/psd_10a_merged/` capture GIMP's
separate `file-psd-load-merged` reader on those same original PSD bytes. These
reference its actual decoded preview, rather than assuming the encoded merged
RGB planes are straight color. A new GIMP process regenerates the builtin ICC
creation timestamp; the reference audit permits only bytes 24–35 to differ and
requires every other profile byte to match. P3 remains fully exact. Each PNG's
profile must exactly match its own captured reference ICC. **Rasterize's PSD
and native ICC preservation has no timestamp exception.**

Nine RGB cases cover ordinary enabled/disabled masks, signed/off-canvas and
hidden storage, partial source alpha/hidden RGB, isolated and Pass Through
masked folders, Multiply/Screen children, nested/hidden folders, explicit
Display P3 and 300/240 PPI. One of these deliberately includes an auxiliary
saved channel and requires a specific loss report. The independent source
decoder checks exact authored and reopened samples/properties against the PSD
bytes; author/reopened RGB reference samples match exactly.

The tenth file is grayscale with a gray ICC and qualifies **explicit refusal**
of both choices, including preservation of an edited sentinel's state. The
official GIMP exporter always writes its effective profile even when profile
embedding is disabled. This file was not rewritten to evade the supported-RGB-ICC
boundary. The existing synthetic grayscale cases continue to qualify admitted
untagged grayscale Raw/PackBits storage; this corpus adds no independent-app
grayscale appearance claim. GIMP writes RLE and outside-zero masks here. Raw,
outside255 and raster-relative endpoint coverage remains supplied by the
independent synthetic suites, not misattributed to the GIMP files.

### Defects found and repaired

The first Release-app comparison found that a single-layer Display P3 PSD was
converted to the working sRGB space after the core had imported it correctly.
`ImageDocument.read` applied the generic flat-image working-space adoption to
both single-layer PSD imports and the explicit saved-composite choice. Explicit
PSD imports now retain their source ICC and decoded samples through publication;
ordinary flat-image working-space adoption remains in place. Reusing a document
also resets its old profile-adoption diagnostic. The source probe checks both
PSD choices, Revert, native reopen, and an ordinary P3 PNG conversion control.
The failed pre-fix diagnostic remains in
`/private/tmp/rz-10a-external-baseline.log`.

The transparent source also exposed incorrect saved-composite interpretation.
GIMP's [PSD exporter](https://github.com/GNOME/gimp/blob/GIMP_3_2_6/plug-ins/file-psd/psd-export.c#L2240)
stores merged color against white; its
[merged reader](https://github.com/GNOME/gimp/blob/GIMP_3_2_6/plug-ins/file-psd/psd-load.c#L3377)
removes that white contribution when transparency is declared. Rasterize had
treated those encoded values as straight RGB. The correction runs only on
verified merged transparency, after channel decoding and grayscale mapping.
Ordinary layer/mask channels remain straight and unchanged.

For positive alpha `A`, the decoded byte is the clamped nearest value of
`255 * (stored + A - 255) / A`, with half-up ties. Alpha stays exact, opaque
color is unchanged and decoded RGB at zero alpha is canonical zero. A separate
Python rational oracle checks every imported preview sample against this
interpretation, and GIMP's actual merged-reader reference supplies the independent
application check.

White-matte quantization irreversibly loses straight color detail at tiny alpha:
the fixture's `[63,137,201]` at alpha 1 becomes stored `[254,255,255]` and correctly
decodes to `[0,255,255]`. Consequently the saved-preview check compares decoded
RGBA against **the independent merged reader** at the unchanged two-code bound,
then compares its visible source-over result against the original layered
reference separately over black and white, also at two codes. The exact layered
source and layered-render gates remain unchanged. Comparing white-matted plane
bytes, or their quantized inverse, directly to original straight RGB would test
different representations. The initial failed assumption remains recorded in
`/private/tmp/rz-10a-closeout-external.log`.

### Finite compatibility reporting

The existing bounded record walk now collects a fixed set of style,
adjustment/fill, type, smart/placed, vector and blending metadata markers,
associated with verified raster/folder identities. Descriptor payloads are not
evaluated. Nondefault/unrecognized Blend If, protection and knockout fields are
reported; documented defaults are observations. Fill-opacity and additional
clipping/interior blending markers explicitly say their payload is not
interpreted or retained, without inventing active settings. Unknown keys and
global additional-info blocks remain outside this finite inspection contract.

Six new FFI tests and 60 independent marker/control fixtures cover exact
case-sensitive keys, benign defaults, opaque/conflicting fields, duplicate
names, divider metadata and unchanged imported pixels/native state. The actual
application verifier pins the complete corpus rather than accepting an arbitrary
subset. This adds reporting, not editable support for these Photoshop features.

### Final integrated checks

The final normal-LTO Release app, authoritative typecheck, all-target Clippy,
formatting and whitespace checks pass. The full Rust regression passes:
**1,502 passed, 0 failed, 2 existing ignored**, across 156 result groups.
Logs: `/private/tmp/rz-10a-final-{app,typecheck,lint,tests}.log`;
summary: `/private/tmp/rz-10a-final-test-summary.json`.

| Final app check | Result | Receipt |
| --- | --- | --- |
| Independent GIMP sources and merged references | **10 cases / 90 artifacts pass**: nine RGB cases and explicit gray-ICC refusal. Layered and merged-reader appearance errors are zero under the declared joint-zero-alpha convention. Worst merged-versus-layered opaque-backdrop error is **0.498040 byte codes**. Exact source/mask/geometry/profile/PPI/native checks pass. | `/private/tmp/rz-10a-final-external/result.json` |
| Finite metadata-marker corpus | **60 pass**, including 37 descriptor-marker cases and 23 defaults/opaque/header controls. | `/private/tmp/rz-10a-final-markers/result.json` |
| Existing ordinary masks | **53 pass**: supported imports, detected losses and malformed/admission refusals. | `/private/tmp/rz-10a-final-masks/result.json` |
| Existing relative masks | **44 pass**, including all 13 absolute/relative pairs and edited-sentinel refusal atomicity. | `/private/tmp/rz-10a-final-relative_masks/result.json` |
| Existing folder and raster corpora | **7 folder + 13 raster cases pass**, including signed/off-canvas storage, explicit composite choices, Undo/native reopen and stale-source refusal. | `/private/tmp/rz-10a-final-groups/result.json`, `/private/tmp/rz-10a-final-pixels/result.json` |
| Actual source UI and metadata | **Five check groups pass**: both compatibility choices/Cancel, MCP/Revert, exact P3/300×240 PPI, ten independently read PSD native files, flat-PNG conversion control and refusal behavior. | `$TMPDIR/rasterize-color-probe-7dv60e5c/result.json` |

The app checks total **187 PSD corpus cases**, including intentional loss and
refusal cases. An additional independent read of all 18 original/composite
native outputs confirms that saved auxiliary channels were actually discarded,
as the report states: `/private/tmp/rz-10a-final-channel-loss-audit.json`.
The new composite kernel's 65,536 color/alpha-pair check measures a worst
alpha-weighted error of `254/255` byte codes and a worst rounded black/white
appearance error of one code. Its 47-test normal-LTO focused run passes.

Qualified app:
`/private/tmp/rz-10a-final-release-candidate/Rasterize.app`.
Executable SHA-256:
`562bd0a7888989f2660540d6be7220d3093b7b578df75a5e2fd5b7bd21467886`.
Final production archive SHA-256:
`d3aa24e25c2b0659695fc8ef5138d0286f500f2ba300a9f6cb81528789288547`.
The archive was captured after the Xcode Rust build phase and is the same
archive independently copied and exercised by the source probe.

The pre-build captures contain 684 product inputs and 542 Rust/test/fixture
inputs. All remain unchanged after qualification. The final combined capture
contains **1,176 unchanged source, test, build and helper inputs** in
`/private/tmp/rz-10a-final-all-inputs.json`. The actual source probe separately verifies **892 physical inputs**
unchanged through compile/run. Its executable SHA-256 is
`0e9f34cad3ea1eda61115014459dccaf62d8af755002c7b7b2a97490c46adc28`;
receipt SHA-256 is
`9d3727b0e7f27760db6c1884fd91c04d96ee89e58ff23d3e1fe5b82775fc3cc9`.

The source probe initially ran without normal macOS service access and failed
LaunchServices registration. The unchanged first binary passed when rerun with
that access; an unnecessary bundle experiment was reverted. Failed diagnostics
remain preserved, and the final source probe runs with normal service access.
Every app/probe uses dummy credentials and private stores, and only owned
processes are stopped. All owned qualification processes exited. No Photoshop
instance or personal document was touched.

### Reproduction and review

The GIMP corpus and merged-reader reproduction commands are in the linked
provenance record. Its original exports are not patched to force byte identity;
reproduction checks exact pixel/structure/color payloads and separately records
GIMP's generated ICC creation timestamps.

```sh
make app
python3 -B scripts/verify_phase10a_external.py \
  --app build/Build/Products/Release/Rasterize.app \
  --output /private/tmp/rz-10a-external-new-run
python3 -B scripts/color-metadata-probe/verify.py \
  --checks 'PSD imports' \
  --archive core/target/release/librasterize_core.a --keep
```

App and source-probe runs require normal macOS graphics/LaunchServices/loopback
access. Existing slice records contain commands to regenerate the 117 synthetic
group/pixel/mask/relative cases. The 60 marker cases are generated by
`RZ_PSD_MARKER_FIXTURES=<new directory>` with `psd_feature_markers` tests and run
through `scripts/verify_phase10a_markers.py`.

Independent subagent review covers the feature matrix, original and merged
GIMP evidence, bounded reporting, both product fixes, reference domains,
direct application-to-reference thresholds, native preservation and refusal
state. No FFI signature, native format, MCP tool count or existing native-file
evaluation meaning changed. This closeout changes PSD decoding/publication and
its compatibility report only.
