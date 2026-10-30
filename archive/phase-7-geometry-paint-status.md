# Phase 7D: geometry, paint and retouch status

**7D is complete within the supported internal workflow.** Implementation began
at `4061646` (7C). Public document mode and central editor/history/recovery
integration remain in 7E. The [Phase 6 cutout quality gate](phase-6-selection-mask-status.md)
remains open.

The [pixel-editing contract](phase-7-pixel-editing.md),
[geometry contract](phase-7-geometry.md) and
[operation matrix](phase-7-operation-support.md) define the supported targets and
precision boundaries. Overall progress remains in the
[high-depth status](phase-7-high-depth-status.md).

## Implemented batches

1. Persistent u16 masks and named saved channels; independent mask placement,
   linking, enabled state, density/feather and float masked rendering. Raw and
   effective inspection remain separate. Native v13 stores the new state;
   typed v11/v12 and legacy byte v1–10 reads retain their established behavior.
2. Shared typed affine/perspective sampling, whole-document resize/crop, quarter
   turns and flips. Exact operations preserve every code and latent RGB.
   Filtered transforms evaluate the shared weights in f64 before f32
   premultiplied accumulation; legacy arithmetic stays unchanged.
3. Continuous document/sRGB authored-color painting and coverage-based fill,
   erasure preserving RGB, and integer-offset cloning from captured raster
   sources. No colored byte overlay or preview becomes a master source.
4. Shared TextKit/path drawing plans produce scalar text/shape coverage, with
   separate continuous color assembly in Rust. Output is a new u16 raster layer.
   Explicit byte scalar promotion retains only 256 coverage levels.
5. Explicit-window healing through the shared membrane solver and byte/u16
   writeback. Source RGB remains authoritative, destination alpha stays exact,
   and a bounded typed residual policy rejects nonconvergence.
6. Three bounded strict C request/read exports and matching Swift wrappers,
   captured edits/reads, payload leases, existing worker/token ownership, and
   native/archive integration. Allocation/native capacity checks precede pixel
   copies; final scalar output publishes only after complete staging.

## Verification

All required checks pass on the final production sources:

| Check | Result |
| --- | --- |
| Rust formatting | Pass. |
| All-target Clippy with warnings denied | Pass. |
| Complete Release Rust suite | **974 passed, 0 failed, 2 existing ignored**, 89 result groups, exit 0. Includes all 33 new tests in four files. |
| Required opt-in million-unknown solver benchmark | **1 passed**, unchanged accuracy; 141.258 ms/channel. |
| Authoritative `make typecheck` | Pass after the final 64 KiB wrapper guard. |
| Final `make app` | Pass; executable SHA matches the app exercised below. |
| Source-linked host probe | **13 Swift groups + 2 independent Python checks**, pass. |
| Isolated Release app MCP | **16 checks**, pass; exact 140-tool catalog, no public precision tools. |
| Native 24 MP pixel workflow | **21/21 stages**, pass with no resource refusal or reduced-size substitute. |

The first full run stopped at two existing Content-Aware Fill timing-ratio
assertions while other builds were active. The unchanged isolated target passed
5/5: spanning preview/fill 0.568/1.320 s and scattered preview/fill 0.731/3.079 s.
The complete suite then passed with `--test-threads=1` and no overlapping builds.
No production code, quality gate or timing threshold changed in response.
Final optimized compilation took 18 min 44 s; Xcode's different build environment
caused another test-artifact link cycle. For future work, freeze Swift/header
inputs before the app build, then complete the Rust suite. The host probe links
an immutable copied static library and records its actual hash.

The host checks cover preserving v13 mask/channel state, actual v11/v12 framing,
v1/v2 host archives, tone/color field preservation, native/PNG16/TIFF16 checks,
strict requests/layouts, resource refusals, source ownership, and actual
TextKit/shape rasterization. The Release app checks cover byte pixel oracles,
Undo/Redo and native v10 reopen, plus v13 refusal before byte fallback while
existing documents stay unchanged. Test apps use private bundles/preferences,
a dummy key and isolated Actions/recovery paths. Every owned process exited.

All executable code and headers remain unchanged after verification. The
**259 core** and **266 Swift/header/project** file hashes matched their frozen
inventories through the final host run. The subsequent required solver benchmark
refreshed only the measured table and module documentation in `poisson.rs`;
its exact diff and before/after hashes are recorded separately, and the original
inventories are preserved. Formatting passes after that documentation update.
The final app executable
SHA is `be92806793fd1ab1968d86837719cba888410d522111853a90909e2ce97f8e79`.

## Precision and cancellation

The ordinary float bound remains **2e-6** and the stored-output bound remains
**2e-6 + 0.5/65535 = 9.629510948e-6**. Exact permutations and raw scalar/native
reads require exact source codes, including all 65,536 values on a nonsquare
fixture. Painting all coverage codes had maximum normalized error
**7.759802974e-6**. Maximum filtered affine error was **7.6848e-6**.

Four independent f64 healing domains differed by at most **one u16 code**,
within the separately declared two-code solution test bound. The typed solver
also enforces a 1/16-code residual gate with at most 32 additional cycles; a
residual alone is not a universal solution-error guarantee. Pure-Neumann source
detail, fractional/zero alpha, unchanged storage and active cancellation pass.

The separate opt-in million-unknown benchmark required by `heal_tests.rs`
requalified the shared legacy solver against its f64 reference. Maximum errors
remain **0.0006 / 0.0025 / 0.0066 byte code units** at the seam, 3–10 px band and
interior respectively. The measured **141.258 ms/channel** replaces the prior
0.145 s timing in `poisson.rs`; no accuracy threshold or executable code changed.

The effective-alpha floor extends from the historical prototype's 2^-32 to
**2^-96**, admitting alpha code 1 × mask code 1 × partial opacity. Positive
underflow and positive products below the floor refuse. Independent small-alpha
checks retain the unchanged arithmetic tolerance. Mask preparation, geometry,
paint and healing use cooperative checkpoints and release reservations on every
exit. Request/codec preparation and final output copies retain their documented
stage cancellation boundaries.

## Native 24 MP workflow

Measured on **Apple M2 Max (Mac14,6), 96 GiB RAM, macOS 26.5.1, Rust 1.92.0**.
The source is a genuine **6000×4000 PNG16**. The fixed tracked working limit is
**2,147,483,648 bytes**. Geometry/scalar and paint/artwork branches each start
from the original source; old/new owners overlap during each transition, with
prior branch owners released before the next branch. This does not qualify
arbitrary independent multilayer/history sessions.

| Stage or paired stages | Seconds |
| --- | ---: |
| Preserving import | 0.765 |
| Genuine u16 mask / saved channel | 2.556 / 2.482 |
| Masked fractional-alpha projection | **0.388** |
| Mask-only exact translation | 0.187 |
| Quarter turn / reverse turn | 0.489 / 0.391 |
| Full-source bilinear affine | **0.537** |
| Geometry/scalar native save / reopen | 0.359 / 0.222 |
| Geometry/scalar archive save / reopen | 0.362 / 0.261 |
| Fresh paint/retouch source import | 0.719 |
| Captured paint / erase / clone | 2.564 / 2.555 / 2.562 |
| Local healing, including two preparatory paints | **8.049** |
| Scalar TextKit / shape artwork | 0.014 / 0.010 |
| Paint/artwork native save / reopen | 0.248 / 0.103 |

There are 257 distributed exact source/mask/channel checks, independent f64
masked/bilinear references, sparse paint/erase/clone oracles, and explicit canvas,
placement, profile and mask-property checks. Healing uses an opaque 32×32 patch
and an interior bump: the independently zero boundary correction predicts its
two-pixel translation. A changed owner and interior RGB change are required;
RGB agrees within one code, and alpha/outside storage are exact.

The first fresh run correctly failed at 20/21 stages because transparent source
columns partitioned the proposed healing domain into constant-correction
components, making the edit a legitimate no-op. Its report is preserved. Only
the fixture changed: two admitted typed paints prepare the opaque patch and bump
inside the same 24 MP source. They are included in the healing stage's time;
there was no core change, weaker numeric gate or source downscaling.

Final process wall time was **25.945 s**, with maximum RSS
**2,922,971,136 bytes (2.72 GiB)** and peak footprint **2,838,317,960 bytes**.
Final typed handles, captured-payload leases, tracked retained bytes and scratch
are all **zero**. The first observed post-exec RSS was 737,280 bytes at 0.054 s;
it is **not a warmed AppKit baseline**. The run used Release Rust and Swift
`-Onone`; paint/erase/clone times include full-canvas Swift coverage construction,
and healing includes that construction and its two preparatory edits. These are
workflow measurements, not isolated kernel or optimized interaction timings.
The 2 GiB admission policy is not an OS RSS ceiling.

## Evidence and reproduction

Run the [source-linked probe](../scripts/precision-probe/README.md) after the
coordinated Release library build:

```sh
python3 -B scripts/precision-probe/verify.py --keep --large-pixels
```

The independent verifier requires all 21 completed stages, full source size,
mandatory numeric checks and zero final resource counts; a successful process
exit or missing report fields cannot qualify the run.

- `/tmp/rz-phase7d-manifest.json` — final coordinator ledger.
- `/tmp/rz-phase7d-core-summary.json` and `/tmp/rz-phase7d-core-tests-final.log` — full suite.
- `/tmp/rz-phase7d-million-solver.log` — required opt-in solver qualification.
- `/tmp/rz-phase7d-documentation-only-source-update.json` — post-validation table update provenance.
- `/tmp/rz-phase7d-mcp-report.json` — final matching Release app checks.
- `/tmp/rz-phase7d-host-qualification.json` — successful final 24 MP report.
- `/tmp/rz-phase7d-host-qualification-attempt1.json` — preserved failed fixture run.
- `/tmp/rz-phase7d-final-host-artifacts.json` — source/binary/library/input provenance.

Preserved final host files are under
`$TMPDIR/rasterize-precision-probe-f0qvqwsd`.
The linked library SHA is
`8a36d79d206e529a5e4aeede44dd4542a24bd8faa6680b3f8728f1c98425fc10`;
the final probe SHA is
`707225829dac13d1b59689886244037bd0aa71b24cb016e6284ace248515b57e`.
The original PNG16 is 139,281,315 bytes with SHA
`62490fb3dab17b4694d324edf11776f6c3a2ceb2952dcd9a03d0d09b6817f770`.

## Limits and next assignment: 7E

Current policy remains normalized encoded-RGB SDR. Styles, clipping, other blend
modes, group/adjustment content free transforms, mask/channel paint/filter/
morphology targets, general RGBA-plane writeback, Sample All/composite retouch
sources, fractional clone sampling, content-aware synthesis, spot healing and
red-eye remain unavailable. Artwork is raster-only with disclosed scalar byte
coverage and uniform-color glyphs. Native payloads remain uncompressed, repeated
destructive writes round to u16, and 48 MP/general large history is unqualified.

7E should proceed through these concrete tasks:

1. Add typed ownership dispatch to the existing document/editing-state model,
   central mutation, Undo/history, captured save, snapshots and recovery. Preserve
   document/target revisions and immutable source identity; no second editor or
   Undo system.
2. Define one operation/target/depth capability decision for UI, MCP and Actions,
   backed by core refusals. Expose only the qualified matrix and preserve the
   deliberate unsupported-target/tool list.
3. Add public mode/import selection and intentional conversion/downconversion.
   Promoting raw bytes by 257 does not preserve old group/adjustment evaluation
   automatically; define and preview conversion semantics before promising
   appearance preservation. Conversion publishes as one central edit.
4. Add deliberate export/downconversion/dithering with depth-loss reporting,
   while retaining the master and captured revision.
5. Connect authoritative typed projection/sampling to display, readouts,
   histogram/clipping counts and enabled tools. A byte preview cannot be a master.
6. Qualify public save/reopen, Undo/Redo, cancellation/stale results, recovery,
   MCP/Actions and profile/depth behavior together. Use Phase 13 when measured
   interaction or memory limits require regional/tiled work.
