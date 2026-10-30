# Phase 6 — selection and mask workflows

Implemented on top of Phase 5; functional app qualification has passed. The advanced cutout-quality gate remains open.

Completion of this gate is now assigned to **9I** in the [implementation plan](professional-photo-editor-implementation-plan.md#9-editable-photographic-filters-and-retouching), tracked as **P7-12** in the [carryover ledger](phase-7-carryovers.md). This schedules the remaining work without changing the original fixtures or the current disabled-control policy. The gate itself is rescoped once, in [gate v2](phase-9i-gate-v2-status.md): every threshold is unchanged, the analytic pass rule keeps the twelve recoverable cases, the exact-opaque-RGB clause becomes a bounded rule, and completion means the assisted protocol passing gate v2 on a declared 36-case subset.

The [9I continuation](phase-9i-cutout-status.md) preserves the earlier results
below and evaluates soft-prior/color candidates plus a joint Bayesian alpha/color
candidate. None qualifies the original combined gate. The [execution continuation](phase-9i-execution-status.md)
adds controlled byte/native output preparation without qualifying an estimator.
Advanced controls remain unqualified; basic typed refinement is verified in [9I typed refinement](phase-9i-typed-cutouts-status.md) — Smooth/Feather/Shift/Contrast run natively at u16 on a 16-bit document and a mask output is a canvas-sized u16 mask over an unchanged RGBA16 layer. The Phase 6 gate is rescoped as [gate v2](phase-9i-gate-v2-status.md) and stays open: the twelve recoverable analytic cases are the pass rule, the six ambiguous and six glass cases are reported diagnostics, and completion now means the assisted protocol passing gate v2 on a declared 36-case subset. Re-scoring the five preserved candidates shows none passes more than 3 of the 12 analytic pass-rule cases and none passes the 24 crops or the 42 full images. RVM is dropped; no neural matting model is added. The corpus and the preserved reports live under `~/devel/rasterize-fixtures/`.

| Slice | Progress |
| --- | --- |
| 6A — Selection essentials | Implemented: freehand lasso, sampled/tonal Color Range, selection-only transforms and shared selection history. |
| 6B — Independent masks | Implemented: independent raw mask bounds, content linking, mask-only affine transforms, persistent density/feather and native v10 migration. |
| 6C — Refinement workspace | Initial workspace implemented: five inspection backgrounds, before/after, Fit/100%, Smooth/Feather/Shift/Contrast and selection/mask output. Experimental categorical strokes, sampled/spatial alpha solvers, foreground-color estimation and reversible new-layer/mask output are implemented. A pinned external learned-model spike has been measured; advanced controls remain unexposed. The four basic controls now run natively at u16 on 16-bit documents ([9I typed refinement](phase-9i-typed-cutouts-status.md)). |
| 6D — Assisted selection | Implemented: seeded Quick Selection, multiple generic foreground-object targets and aggregate people selection. Magnetic lasso and specialized models remain follow-ons. |

The completion gate covers portrait, fur, translucent-object and hard-edged-product cutouts viewed at 100%; reversible refinement; reusable masks; mask transforms that do not change layer pixels; and native round trips of mask properties. Existing Phase 2 coverage/history and Phase 4 execution contracts remain the shared paths.

## Behavior and boundaries

Selections remain canvas-sized coverage and share Phase 2 clean Undo/Redo. Color Range uses document-profile Lab ΔE76 or L*; Quick Selection grows connected seed regions and is not a semantic brush. Object IDs come from Vision foreground instances; region hit tests use its coarse map. People coverage is one aggregate, without body-part controls. Very complex masks show a bounds outline after 200,000 contour vertices; coverage and dimming remain exact, and UI/MCP report the fallback.

Layer masks retain raw bytes at their own canvas position. Content geometry carries linked masks only; canvas geometry carries both. Paint, filters and fills edit original coverage without baking density/feather. Density lowers the hiding strength; feather uses the existing bounded multiresolution Gaussian. Its numeric radius remains in canvas pixels through geometry. Described-layer rerenders preserve the independent mask instead of recropping it. Masked Free Transform previews use admitted core geometry/projection; these previews may display the previous result while rendering. Mask kernels and Vision workers have responsive noncancellable progress; selection kernels support cooperative cancellation.

New masks use the existing layer-sized crop (canvas-sized for groups). Refinement mask output creates/replaces that raw mask and resets its properties to linked/full-density/no-feather; selection output changes only transient coverage. Save Selection/alpha channels keep coverage reusable separately. Native v10 stores independent dimensions, placement, linking, density and feather; v1–9 migrate using existing placement with linked/full-density/no-feather defaults. Current writers bound aggregate logical mask storage at 6.8 billion pixels, independently of the much smaller working-memory admission limit. Legacy records remain readable; rare old files exceeding that new aggregate cap need mask reduction before saving v10.

## Quality gate

[Algorithm contracts, fixture rights, metrics and reproduction](phase-6-selection-quality.md) record **12/24 analytic cases passing**, so the guided-edge candidate failed its predeclared gate. Native-pixel photographic inspection confirms widened portrait/product halos, missing cat whiskers and an opaque glass silhouette retaining its original background. The photographs have no alpha ground truth; their inspection is not a numerical accuracy claim. Ordinary controls remain reversible manual coverage edits. Advanced edge brushing and foreground color decontamination are unavailable; the full portrait/fur/translucency/product completion gate is not complete. This follows the plan's explicit permission to ship selection essentials, mask controls and the initial workspace independently of matting research.

## Advanced Phase 6C status and next work

The continuation implements spatial matting, reversible foreground-plus-mask output, stricter candidate validation, broader full-image references and a separate pinned learned-model experiment. **Phase 6 is not complete.** Advanced brushing and color controls remain unavailable. [Phase 7A](phase-7-high-depth-status.md) proceeds independently under the main plan's Phase 3/4 dependencies; it does not close or waive this quality gate.

| Alpha candidate | Original analytic 24 | Portrait crops 24 | Full native-image 42 |
| --- | ---: | ---: | ---: |
| Frozen sampled candidate | 10/24 | 8/24 | 15/42 |
| Spatial closed-form candidate | 10/24 | 9/24 | 15/42 |
| ViTMatte, packaged normalization | 8/24 | Not run | Not run |
| ViTMatte, author normalization | 9/24 | Not run | 0 admitted; 42 resource refusals |

Before the 9I experiments, every estimated-alpha/color pipeline had **zero combined passes**. Color estimation with reference alpha passes 20/24 analytic, 22/24 cropped and 30/42 full-image cases; these isolate the color estimator and cannot qualify estimated-alpha output. The original guided baseline remains 12/24. All 48 previous sampled records retain exact numerical and gate parity after the shared evaluator refactor, apart from timings and the new diagnostic field.

The [spatial solver](phase-6-closed-form-matting-candidate.md) converges on all non-glass analytic cases and all cropped/full photographs. It improves boundary MAD in 28/42 full cases, but also regresses some cases, so equal aggregate pass counts do not imply equal output. All six analytic glass cases still lack known foreground evidence and refuse. The [full references](phase-6-full-image-reference-protocol.md) add annotated fur and a hard product, and evaluate all seven images without cropping/resizing. The glass asset is explicitly a silhouette annotation, **not transparent-interior truth**.

The [learned-model experiment](phase-6-vitmatte-spike.md) uses a pinned public safetensors model and isolated temporary Python environment; it adds no app dependency. An upstream audit found that the packaged preprocessing differs from the authors' training normalization, without corresponding weight compensation. Both variants and their scores are retained. The author-normalized full-image attempt refuses 30 cases before model loading and 12 after measured RSS crosses 2 GiB. Those latter peaks reached 2.29–2.85 GB before owned-process termination: the guard is a monitored rejection threshold, not a hard memory ceiling. No resized substitute or partial alpha is retained.

The new constraint audit proves that frozen automatic trimaps prevent a pass in **6/24 analytic**, **6/24 cropped photographic** and **6/42 full-image** cases, regardless of solver. It calculates an error lower bound and detail-recall ceiling under unchanged known labels, with reference alpha used only for the post-hoc audit. No reference reaches a candidate. Binary analytic hair can retain at most 92.32% of soft detail and binary analytic glass 28.21%, below the 95% gate; two binary portrait crops have ceilings of 64.73% and 81.81%.

Remaining work:

1. Replace the assumption that automatically thresholded prior labels are trustworthy hard constraints. Distinguish tentative automatic labels from explicit user foreground/background strokes, and qualify a corrected input policy or source-only correction replays on the preserved corpus. `RefinementTrimap.swift` and `trimap_strokes.rs` already provide categorical Unknown strokes that release a constraint. Do not repair inputs with reference alpha or lower the gates.
2. Supply stronger foreground/background evidence for translucent objects and obtain an appropriate photographic reference. A glass silhouette and a successful independently supplied-color endpoint test do not establish automatic transmission recovery. General refraction also exceeds the fixed-foreground/scalar-alpha model.
3. Improve coupled RGB recovery while retaining the separate encoded/profile-linear estimator. The [ambiguity regressions and feasibility experiment](phase-6-foreground-evidence.md) show why a consistent color fit cannot certify opacity: the same observed image and valid constraints can admit conflicting opaque/translucent explanations. Projecting colors to a feasible compositing range does not fix the measured failures. Preserve the opaque-RGB, fractional-color and recomposition gates.
4. If learned matting is pursued, qualify a deployment implementation and its memory/cancellation behavior. The current Python spike supports the small analytic fixtures but fails native photographic resource qualification. MPS and Core ML conversion have not been validated; no model/runtime ships with Rasterize.
5. Expose controls only after the workflow qualifies. [New-layer/mask assembly](phase-6-cutout-output-contract.md) is implemented and tested with both document/selection and brush-session revision checks, source preservation and one Undo step. The [9I execution continuation](phase-9i-execution-status.md) adds controlled byte record/pixel copying, shared byte projection, native composition and preview conversion, with Cancel and owner-close forwarding. Individually admitted JSON parses and CoreGraphics calls retain boundary checks; no universal cancellation-latency guarantee is made. Current MCP continues refusing advanced and unknown parameters. Basic workspace code remains in `SelectionRefinement.swift` and `RefineSelectionSheetController.swift`, with the typed twin in `NativeSelectionRefinement.swift`; that verified basic path does not expose or qualify any advanced control.

## Advanced slice verification

### Current continuation

- Full core suite: **876 passed, 0 failed, 2 existing ignored** across 72 result groups. After review, six additional focused candidate/import tests pass, for **882 distinct passing Rust tests** across the full run and final focused checks. Logs: `/tmp/rz-phase6d-core-tests-final.log`, `/tmp/rz-phase6d-core-summary.json`, `/tmp/rz-phase6d-guard-tests.log`. The added guards reject wrong-sized alpha/confidence, known-label violations, oversized or wrong-native-mode PNG headers, and repository/symlink output paths. Native decoding has an explicit allocation allowance.
- Formatting and all-target Clippy pass. Authoritative `make typecheck` passes with **0 errors**. All **17 Python Phase 6 fixture/evidence/model-guard tests pass**. Logs: `/tmp/rz-phase6d-clippy-final.log`, `/tmp/rz-phase6d-typecheck-final.log`, `/tmp/rz-phase6d-python-tests.log`.
- [Cutout output assembly](phase-6-cutout-output-contract.md) creates a new foreground layer with a separate editable mask, hiding old roots while retaining original pixels and structure. Three core FFI tests cover native reopen, no-op/error admission and live cancellation. The actual AppKit probe passes **three checks / six groups**, covering output, exact Undo/Redo, canvas retargeting, stale/cancel/no-op paths, categorical history and advanced MCP refusal. Logs: `/tmp/rz-phase6d-output-tests.log`, `/tmp/rz-phase6d-cutout-probe.log`; artifacts: `$TMPDIR/rasterize-selection-probe-o0k_e1qo`.
- Standard optimized **`make app` passes**, with unchanged optimization settings. Isolated Release MCP verification passes raw pixels, independent masks, native v10, snapshots, forced-crash recovery, advanced argument refusals and basic refinement's exact clean Undo/Redo. Logs: `/tmp/rz-phase6d-release-app.log`, `/tmp/rz-phase6d-release-verification.log`; private artifacts: `$TMPDIR/rasterize-phase6c-release-bp94547l` and `$TMPDIR/rasterize-phase6-52f_awj3`. Test instances used dummy credentials, private state and owned processes; all were stopped.
- Numerical gates **fail as reported above**, independently of the passing functional tests. Sampled full-image evidence: `/tmp/rz-phase6-full-quality/`; 64 MiB admission refusal proof: `/tmp/rz-phase6-full-admission/`; exact earlier sampled parity: `/tmp/rz-phase6c-sampled-parity/exact-parity.json`. Spatial evidence: `/tmp/rz-closed-form-quality/` and `/tmp/rz-closed-form-full/`. Foreground evidence: `/tmp/rz-phase6c-foreground-evidence.json`.
- Learned-model evidence: `/tmp/rz-phase6-vitmatte-quality/`, `/tmp/rz-phase6-vitmatte-author-quality-final/` and `/tmp/rz-phase6-vitmatte-full-quality/`. Full `run.json` joins the scorer's 42 refusals with the 30 preflight/12 measured-RSS causes and records hashes. Hardened input validation preserves all 24 author-normalized scores exactly after canonicalizing artifact paths and excluding timings; proof: `guard-parity.json` beside the final analytic metrics. Source-only exports and model outputs have independent manifests and hashes. Models, photographs, environments and generated reports remain outside the repository.

### Previous experimental slice

- Full core suite: **861 passed, 0 failed, 2 existing ignored**, across 66 integration targets. The first sandboxed attempt stopped at the existing localhost MCP bind test; the authorized rerun with localhost access passed. Log: `/tmp/rz-phase6c-core-tests-final.log`; counts: `/tmp/rz-phase6c-core-summary.json`.
- Formatting and all-target Clippy pass. Logs: `/tmp/rz-phase6c-format-final.log`, `/tmp/rz-phase6c-clippy-final.log`. Focused checks include 10 sampling tests, 6 foreground tests (including non-sRGB tone curves), 7 trimap FFI tests and 3 independent metric tests; fixture acquisition has 5 Python checks. Live cancellation leaves outputs untouched and releases scratch.
- Authoritative `make typecheck`: **0 errors**. Log: `/tmp/rz-phase6c-typecheck-final.log`. Source-probe compilation also passes.
- Actual AppKit source probe: **4 checks / 5 groups pass**. This covers categorical Rust replay, explicit Unknown strokes, local history/caps/cancel, stale session/document rejection, strict MCP argument refusal, and the existing refinement workspace's backgrounds/100%/Apply/Cancel/Undo plus mask output/native round trip. Log: `/tmp/rz-phase6c-source-probe.log`; artifacts: `$TMPDIR/rasterize-selection-probe-te5xmzl6`.
- Standard **`make app` Release build passes** with unchanged optimization settings. Log: `/tmp/rz-phase6c-release-app.log`. Artifact: `build/Build/Products/Release/Rasterize.app`.
- Isolated **optimized Release external MCP verification passes**. The mask suite covers raw source pixels, independent geometry/properties, fill/filter, native v10, snapshots and forced-crash recovery. Advanced/unknown refinement fields preserve exact coverage and history on refusal; basic Feather remains one clean Undo/Redo with unchanged source pixels. Log: `/tmp/rz-phase6c-release-verification.log`; private report/artifacts: `$TMPDIR/rasterize-phase6c-release-w9r7vtgl`. Test instances use dummy credentials, private preferences/data and owned processes; all were stopped, and the original Release executable was preserved.
- Strict numerical quality qualification completed and **failed as reported above**; its exit 1 is distinct from the passing functional suite. Reports, binary hashes and native inspection images are under `/tmp/rz-phase6c-qualified/`. Baseline parity is `/tmp/rz-phase6c-guided-baseline/parity.json`. No photographic files or generated build artifacts are committed.

Reproduce the advanced checks using the reference protocol's acquisition/build steps, then:

```sh
PYTHONDONTWRITEBYTECODE=1 python3 scripts/phase6_matting_qualification.py \
  ~/devel/rasterize-fixtures/phase6/references \
  ~/devel/rasterize-fixtures/phase9i-reports/rz-matting-run --gate v2 --require-pass
PYTHONDONTWRITEBYTECODE=1 python3 scripts/selection-probe/verify.py \
  --checks 'refinement cutout output,trimap model,advanced refinement gate,refinement workspace,refinement mask output' --keep
PYTHONDONTWRITEBYTECODE=1 python3 scripts/verify_phase6c.py \
  --app build/Build/Products/Release/Rasterize.app --keep
```

## Earlier functional qualification

- Full core suite: **834 passed, 0 failed, 2 existing ignored**; all-target Clippy and formatting pass. The final mask-equality query also passed 11 focused independent-mask tests and the NULL sweep, followed by all-target Clippy. Logs: `/tmp/rz-phase6-core-tests.log`, `/tmp/rz-phase6-mask-equality-tests.log`, `/tmp/rz-phase6-mask-equality-null.log`, `/tmp/rz-phase6-staticlib-final.log`.
- Authoritative `make typecheck`: **0 errors**; existing deprecation/hygiene warnings only. Log: `/tmp/rz-phase6-typecheck-final.log`.
- Private app build linked the current release core and all Swift sources directly, preserving app resources and using a private bundle ID; build and ad-hoc signing passed. Log: `/tmp/rz-phase6-app-build.log`. Direct linking avoided Xcode's Cargo build script invalidating concurrent Rust test artifacts through SDK environment differences.
- Private MCP: **all 5 groups pass**, covering unchanged source pixels under mask properties/geometry, exact translated raw coverage, unlinked content motion, mask fill/filter properties, native v10 reopen, named snapshots and forced-crash recovery. Portrait 512×512, fur 451×300, glass 1573×2302 and product 2640×1860 each passed subject selection → basic refinement → mask → Undo/Redo → identical-repeat no-op → saved-channel reuse → native reopen. Source hashes and full-resolution exported pixels match. Log: `/tmp/rz-phase6-mcp-final.log`; machine-readable report and PNGs: `$TMPDIR/rasterize-phase6-99e9d8c3/`.
- Actual AppKit source probe: **all 13 checks / 21 test groups pass**. This covers actual Freehand/Quick gestures; Color Range sampling/Apply; selection transforms; a real two-instance Vision fixture, object picker/hit/region and aggregate people selection; Actions parity/replay/seed budget; bounded contour construction and responsive feather; both mask sheets' Apply/Cancel, stale targets and exact numeric precision; described shape rerender preservation; independent/group/perspective transform previews; extreme group bounds; all refinement backgrounds, before/after, 100%, Cancel, output and no-op behavior. The probe uses real sheet presentation and waits for physical dismissal. Log: `$TMPDIR/rasterize-selection-probe-2cdw0r29/run.log`.
- The preview debounce releases its completed work closure; mask sheets reject field events after finishing, so Cancel cannot restore a late overlay. Persistent mask field formatting no longer changes imported values or makes Apply differ from its preview.
- Release compiler investigation identified the former 2,800-line `AgentServer.catalogJSON` builder spending minutes in `COWArrayOpts` / `ColdBlockInfo`. It is now split into smaller builders with explicit non-inlining boundaries; optimization settings are unchanged. Mechanical verification preserved all 115 original tool blocks and 3,467 string literals. A live before/after comparison confirms **all 140 tool definitions and their order match exactly** (`/tmp/rz-phase6-catalog-after/result.txt`). Authoritative typecheck and the private app rebuild pass after this refactor.
- Final standard **`make app` Release build passes** with unchanged optimization settings (`/tmp/rz-phase6-release-app.log`). An isolated copy of that optimized app also passes the mask/native/snapshot/forced-crash smoke suite (`/tmp/rz-phase6-release-smoke.log`). The built app is `build/Build/Products/Release/Rasterize.app`.

Reproduce the functional app checks with the checksum-pinned photographs downloaded as described in the quality report:

```sh
python3 scripts/selection-probe/verify.py \
  --photos-dir ~/devel/rasterize-fixtures/phase6/photos --keep
RZ_VERIFY_APP=/path/to/private/Rasterize.app/Contents/MacOS/Rasterize \
  PYTHONDONTWRITEBYTECODE=1 python3 scripts/verify_phase6.py --keep
```

All test instances use a dummy API key, a private bundle/preferences domain, private Actions and recovery directories, and owned processes. No user app instance is signalled.
