# Phase 6 selection kernels and refinement qualification

The selection essentials and ordinary coverage controls use the existing core profile, resampling and morphology implementations. The original guided brush failed its analytic quality gate and is not enabled in the editor or MCP. Experimental replacement matting and foreground-color estimation are now being qualified independently; advanced controls remain unexposed. The refinement workspace can inspect, adjust and output reusable coverage without claiming automatic high-quality matting.

The later [9I soft-prior experiments](phase-9i-cutout-status.md) separate automatic
priors from explicit user constraints without changing this corpus or its gates.
They preserve all historical results below; the combined cutout gate remains open.

## Contracts

All four new exports are in `ffi_selection_tools.rs`; the hand-maintained header is the C contract. Inputs are immutable. Caller output is canvas-sized top-first u8 coverage and is published only on success. Failures and cooperative cancellation leave it untouched. Zero-control refinement and identity transforms succeed with identical bytes; the host compares before registering history. Output may alias a coverage input because publication follows every source read. A NULL job token runs the same kernel without cancellation. Tokens are single-use and follow the Phase 4 ownership rules.

- `rz_selection_color_range(document, source, params, output, control, error)` requires the captured projection's dimensions to match the document. Sampled mode accepts `{"mode":"sampled","samples":[[200,150,120]],"tolerance":20,"softness":10}`. Color distance is Euclidean CIE76 in the document's D50 Lab space, through the existing ICC and Lab implementation. Sample count is 1–32, tolerance 0–200, softness 0–100. Coverage is full up to tolerance, then falls linearly to zero over softness; the union takes the nearest sample. Tonal mode accepts `{"mode":"tonal","lower":0,"upper":35,"softness":10}`; bounds are L* in 0–100 with linear outer shoulders. Source alpha multiplies selection coverage. LUT profiles refuse rather than silently assuming sRGB.
- `rz_selection_quick(document, source, params, output, control, error)` accepts `{"seeds":[[100,80]],"tolerance":25,"edge":12}`. Up to 4096 integer canvas seeds start a deterministic four-connected breadth-first grow. A neighbor must be within the local edge threshold **and** within the tolerance of the originating seed. Both thresholds are ΔE76 in 0–200. The second test bounds gradient drift; the first prevents crossing abrupt edges within a broad seed palette. First claims win in input order. Transparent pixels are barriers, and selected pixels retain source alpha. The host samples the stroke path and combines add/subtract/intersect coverage; the kernel does not interpolate between supplied seeds or identify semantic objects.
- `rz_selection_transform(mask, width, height, matrix6, filter, output, control, error)` uses the existing canvas-plane inverse sampler. Matrix order is `[a,b,c,d,tx,ty]`; filters are nearest 0, bilinear 1, Catmull–Rom 2, Lanczos3 3. Destination canvas size is fixed, outside samples are zero, and singular/nonfinite matrices refuse. It never transforms layer pixels.
- `rz_selection_refine(source, mask, optionalBrush, params, output, control, error)` applies smooth → signed contour shift → feather → contrast. Smooth/feather are 0–64 pixels, shift ±64 pixels, contrast 0–1. Smooth, feather and signed distance operations route to the existing `doc_select` kernels, now with explicit cancellable contexts. Shift deliberately uses the 50% contour and regenerates its antialias ramp; feather preserves intermediate coverage. Contrast narrows the soft interval around 50%, reaching a hard 128-byte threshold at 1. The internal candidate accepts integer `edge_radius` 0–16 and `edge_strength` 0–1; positive radius requires a brush mask, which controls only candidate writeback. This candidate is for qualification, with host exposure gated off.

Requests are strict JSON objects; unknown fields, invalid ranges/types and parameter strings larger than 128 KiB refuse. The core borrows the captured source; it never flattens a second projection. Progress/cancellation occurs per row and at most 4096 ordinary pixels, including the shared Gaussian and coverage sampling loops. Legacy synchronous entry points route through the same kernels with an empty context.

Conservative working reservations below exclude the borrowed source/input and host-owned output plane, which the host must account separately. N is canvas pixels and M=max(width,height):

| Operation | Core working reservation |
|---|---:|
| Color Range | N + 256 KiB |
| Quick Selection | 21N + 1 MiB |
| Coverage transform | 2N + 4096 bytes |
| Ordinary refinement | 5N + 256 KiB |
| Refinement with shift | 9N + 20M + 4 + 256 KiB |
| Candidate guided brush | 64N + 256 KiB |

The Gaussian radii are bounded because direct separable convolution costs O(Nr). Quick Selection stores one Lab plane, a queue with exactly N reserved entries, and output coverage. Its queue retains originating seed indices rather than allocating a distance-to-every-seed table. Normal controls and transforms have no reduced-resolution authoritative path. These are explicit full-canvas selection requests: connected growth and signed-distance shift have global support; Gaussian controls declare radius support, and the current adapter processes the full canvas for their output coverage. Affine sampling declares its existing filter support and zero exterior. No region is silently treated as the whole image, and no preview result replaces a full-resolution mask.

## Candidate and predeclared gate

The scalar guided filter implements equations (5), (6), (8) from [He, Sun and Tang, Guided Image Filtering (ECCV 2010)](https://people.csail.mit.edu/kaiming/publications/eccv10guidedfilter.pdf). It fits a local linear relationship between guidance and coverage, then averages the local coefficients. The implementation uses the shared encoded RGB luma coefficients, an epsilon of 0.0004, clipped normalized box windows and f64 rolling sums with f32 planes. It is a coverage filter, not foreground-color estimation, linear-light matting or a semantic model.

The original reproducible analytic corpus in `core/tests/common/selection_quality.rs` covers portrait/hair, fur, translucent glass and a hard product. Ground-truth alpha uses 4×4 area samples of explicit curves/shapes, independently of the kernels being tested. Each subject has light, dark and ambiguous foreground/background color variants, with both Gaussian-degraded and thresholded initial masks: 24 cases total at 320×256. Glass includes fractional interior transmission, rims and highlights; the product includes a small hole and narrow antenna; hair/fur include thin partial-coverage detail.

The gate was set before measurements: every case must avoid boundary mean absolute alpha error regression greater than 1/255 and retain at least 95% of ground-truth soft detail. Boundary pixels include every fractional-alpha pixel and pixels whose five-by-five neighborhood changes alpha. Soft detail means ground-truth coverage 32–223, retained if result coverage is at least 16. This must be accompanied by photographic inspection at native pixel size. These deliberately limited metrics do not establish performance on a general matting benchmark.

**Result: 12 of 24 analytic cases passed; the candidate gate failed.** Representative results (normalized alpha error):

| Case | Initial boundary MAD | Candidate boundary MAD | Candidate soft-detail recall |
|---|---:|---:|---:|
| Hair, light, thresholded | 0.0939 | 0.0208 | 96.4% |
| Hair, ambiguous, blurred | 0.1417 | 0.2427 | 96.1% |
| Fur, light, thresholded | 0.0712 | 0.0107 | 99.8% |
| Fur, ambiguous, blurred | 0.1279 | 0.2545 | 99.7% |
| Glass, light, thresholded | 0.2634 | 0.2302 | 22.2% |
| Glass, dark, blurred | 0.0340 | 0.0494 | 100% |
| Product, dark, thresholded | 0.0017 | 0.0322 | 100% |
| Product, ambiguous, thresholded | 0.0017 | 0.3871 | 100% |

A filter can improve an approximate contour when image contrast is useful and damage an already accurate contour when that guidance is weak. Thresholded glass does not contain the fractional interior coverage needed to recover its transmission. Nothing here justifies enabling foreground color decontamination: it additionally needs trustworthy foreground/background estimates and representative color-bleed evaluation. The [Alpha Matting benchmark's own dataset documentation](https://www.alphamatting.com/datasets.php) distinguishes alpha ground truth from linear-RGB foreground/input data; those are different qualification needs.

## Experimental foreground-color estimation

`foreground_estimate.rs` implements a separate color candidate based on the local compositing and alpha-gradient energy in [Germer et al., Fast Multi-Level Foreground Estimation](https://arxiv.org/abs/2006.14970). It estimates foreground/background colors at successive resolutions, with ten sweeps at sizes up to 32 pixels per axis and two at larger sizes. These are fixed approximation bounds, not convergence claims. The alpha-gradient weight 0.1 and regularizer 0.005 were fixed before qualification. The pyramid uses the existing normalized triangle sampling weights, with full-resolution input at the final level.

`rz_image_estimate_foreground(document, source, alpha, params, control, error)` is an experimental FFI seam only. The opaque source must match the captured document's canvas and profile. Dimensions and document profile support are validated; bare image handles carry no profile, so supplying pixels in document space is a caller precondition. Strict JSON requires `color_space: "encoded"` or `"linear"`; optional `strength` is 0–1, default 1. Linear mode shares the document ICC forward decoder and threshold encoder. Unsupported LUT profiles refuse. Encoded mode is explicit because the original analytic corpus was composed in encoded RGB; its metrics must not silently be interpreted as linear-light accuracy.

The owned result contains estimated RGB and unchanged source alpha. **It does not apply the supplied matte.** RGB at matte 0/255 remains exact. Zero strength, a binary matte, or byte-identical output returns NULL without an error; invalid inputs/cancellation return NULL with an error. A spatially constant fractional matte refuses because it supplies no color-separation evidence. Caller inputs are immutable, tokens are single-use, and conservative scratch is 128N + 1 MiB including the returned image, excluding borrowed inputs. Pyramids, field resampling and solve sweeps check cancellation. Future UI output must preserve originals and apply colors plus a separate editable mask in one undoable operation; selection-only or mask-only output cannot carry color changes.

The focused FFI tests isolate color estimation with independently composed foreground/background colors and a broad partial-alpha ramp. Both encoded and linear modes meet the predeclared weighted mean color error ≤0.02 and reduce contamination by at least 75%; these simple fixtures do not qualify general photographs. Additional checks cover exact endpoint/source preservation, no-op semantics, strict inputs, NULL safety, scratch refusal and single-use cancellation.

## Replacement candidate qualification — September 2026

The [sampling candidate](phase-6-sampled-matting-candidate.md) adds a separate experimental alpha solver. The [reference and color protocol](phase-6-matting-reference-protocol.md) freezes the prior-only trimap, original alpha gate, additional color gates, reference rights and artifact interpretation. **No thresholds changed after measurement.** The original guided run reproduces all 24 before/after metric sets and pass flags exactly, still 12/24.

The replacement scores are **10/24 analytic alpha cases** and **8/24 photographic alpha cases**. The six analytic glass cases refuse because their trimaps contain no known foreground sample. With exact reference alpha supplied, the foreground estimator passes **20/24 analytic color cases** and **22/24 photographic color cases**. Those reference-alpha entries repeat across the two starting-mask labels: 10/12 and 11/12 unique source scenes respectively. With sampled alpha, color and combined alpha/color gates pass **0/24 in both sets**. These are separate measurements: successful color reconstruction with reference alpha does not establish a useful automatic cutout.

Representative boundary-alpha MAD and detail results:

| Case | Initial boundary MAD | Sampled boundary MAD | Soft-detail recall |
| --- | ---: | ---: | ---: |
| Hair, light, blurred | 0.14166 | 0.00076 | 100% |
| Hair, light, thresholded | 0.09392 | 0.01421 | 90.4% |
| Hair, ambiguous, blurred | 0.14166 | 0.33404 | 94.2% |
| Fur, ambiguous, blurred | 0.12789 | 0.30745 | 87.2% |
| Product, ambiguous, thresholded | 0.00174 | 0.20235 | 85.0% |
| Photo 0003, light, blurred | 0.01274 | 0.02602 | 99.5% |
| Photo 0060, light, blurred | 0.02005 | 0.01659 | 99.9% |

Every solved analytic case and every photo case changes RGB at some **reference-opaque** pixels after inaccurate sampled alpha makes them appear fractional. This fails the strict zero-change color criterion even where foreground error is small. The kernel's exact preservation guarantee applies to its **supplied-alpha** endpoints; it cannot identify those reference-opaque pixels from an inaccurate matte. Independent PNG inspection measured maximum opaque RGB changes of 11 byte levels across analytic exports and 47–59 levels across the four photographic light/blurred exports; this is not solely rounding noise. Other photographic combinations were scored but their maximum byte deltas were not separately exported/measured. Failures extend beyond that criterion: 12 analytic records (six solved cases plus six unchanged-prior glass diagnostics) and 19 photographic cases exceed the replacement-background boundary MAD limit. Reference-alpha color failures are dark/ambiguous analytic glass and the first portrait over checker (fractional foreground MAD 0.06168, above 0.05). Overlapping per-case failure reasons and artifact measurements are recorded in `/tmp/rz-phase6c-qualified/failure-reasons.json`.

Native-pixel inspection of the first photographic portrait over dark and white backgrounds confirms broken/patchy outer hair coverage and changed hair texture around the boundary in the coupled result. Analytic hair on a light background becomes sharper, while the refused glass retains its prior alpha and still has edge halos. Full-resolution planes and reference/prior/source-mask/estimated-color composites are saved for inspection; the four-column panels are native crops, not resized previews. The photographic set comprises four native 1280×1280 portrait/hair crops, each over three controlled backgrounds with two degraded priors; it does not provide fur/glass/product reference data or full-frame qualification.

The strict wrapper ran in 67.2 seconds and exited **1 because the numerical quality gate failed**. Runtime is this one measurement, excluding builds. Reports: `/tmp/rz-phase6c-qualified/run.json`, `/tmp/rz-phase6c-qualified/matting/metrics.json`; native artifacts are beside the latter. Original-baseline parity is `/tmp/rz-phase6c-guided-baseline/parity.json`. No advanced UI/MCP controls were enabled.

## Full-image and learned-candidate continuation — September 10, 2026

The [spatial closed-form candidate](phase-6-closed-form-matting-candidate.md) passes **10/24 original analytic** and **9/24 cropped photographic** alpha cases. Both sampled and closed-form candidates also evaluate [seven complete reference images](phase-6-full-image-reference-protocol.md), comprising **42 controlled recompositions**, without resizing or resource refusals: each passes **15/42 alpha cases and 0/42 combined alpha/color cases**. The new references add fur and a hard product; the glass asset supplies silhouette coverage, not transparent-interior truth. These fixtures still provide no physical photographic translucency reference. The earlier guided **12/24** and sampled **10/24 analytic, 8/24 cropped** results remain unchanged; the shared-evaluator regression comparison preserves every previous field except timings and the added diagnostic metadata.

The separate [ViTMatte offline spike](phase-6-vitmatte-spike.md) passes **8/24 analytic alpha cases** with its packaged Hugging Face normalization and **9/24** with the separately declared author normalization; neither passes a combined case. Its full-image attempt admits **no alpha output**: **30/42 cases refuse in memory preflight and 12/42 cross the measured RSS threshold**, with sampled peaks of **2.29–2.85 GB**. The 2 GiB monitor is a rejection threshold, not an OS allocation ceiling. These are recorded refusals, not completed full-image accuracy measurements; no resized substitute or increased allowance is used.

The reference-only constraint audit proves that hard labels in the frozen automatic trimap make the alpha gate impossible in **6/24 analytic**, **6/24 cropped photographic** and **6/42 full-image** cases. Reference alpha determines this post-hoc error bound and recall ceiling but never enters candidate inference. [Foreground ambiguity evidence](phase-6-foreground-evidence.md) separately shows that a compositing-feasibility projection does not repair the coupled color damage; it remains a failed hypothesis rather than a new correction option.

The original gates remain intact. Model weights, reference photographs and the isolated Python environment stay outside the repository; no model runtime or dependency is added to the app. Advanced controls remain unavailable, and the Phase 6C quality gate remains open.

## Licensed photographic inspection

`scripts/phase6_quality_fixtures.py` downloads checksum-pinned original bytes into a caller-provided temporary directory and writes provenance. No third-party photographs are committed.

| Subject | Photo | Rights and source |
|---|---|---|
| Portrait/hair | NASA portrait of Eileen Collins, 512×512 | Public domain; [scikit-image's primary data documentation](https://scikit-image.org/docs/0.24.x/api/skimage.data.html#skimage.data.astronaut) |
| Fur | Chelsea the cat, 451×300, Stefan van der Walt | CC0; [maintainer documentation](https://scikit-image.org/docs/0.24.x/api/skimage.data.html#skimage.data.chelsea) |
| Glass | Glass of water, 1573×2302, Derek Jensen (Tysto) | Photographer's public-domain dedication; [original file page](https://commons.wikimedia.org/wiki/File:Glass-of-water.jpg) |
| Hard product | Canon EOS R, 2640×1860, Tycho/Shansov.net | CC0 1.0; [original file page](https://commons.wikimedia.org/wiki/File:Canon_EOS_R.jpg) |

`scripts/phase6_initial_masks.py` drives the app's existing `select_subject` path over MCP, with a dummy API key, owned process, private port and isolated recovery directory. All four photographs produced one Vision foreground instance. It saves the selected coverage as a named channel and explicitly requests that channel by name when rendering. The qualification runner asserts that imported coverage PNGs are opaque grayscale and contain both zero and full coverage; a color composite cannot masquerade as a selection mask.

`core/examples/phase6_selection_quality.rs` exports before/candidate and before/ordinary-control comparisons on white, dark and checker backgrounds. Each inspection image is a native-pixel crop with no scaling: initial on the left, result on the right. It also saves full-resolution source, input and output masks. Ordinary controls in this probe are feather 1 and contrast 0.05. The photos have **no ground-truth alpha**; their numbers measure change and runtime, not accuracy.

Native-pixel inspection found concrete limits: the guided candidate softens the portrait's outer hair boundary into a wider halo; the cat's long whiskers missing from the initial Vision mask are not reconstructed; the product's retained white fringe spreads; the glass remains an opaque silhouette containing its original tan background. A good object silhouette is not a transmission matte. Feather/contrast remain ordinary user-controlled coverage edits and do not fix missing structure or color contamination automatically.

On this Apple M2 Max run, candidate/manual refinement timings were 11.29/1.96 ms for the portrait, 4.39/1.02 ms for fur, 127.57/26.79 ms for glass and 173.47/36.13 ms for the product. These are single optimized core calls on the stated dimensions, excluding Vision, export and UI work; they are not a latency guarantee.

## Free Transform and independent masks

Masked documents use a debounced core-rendered Free Transform preview. Geometry and projection run as separate `PreviewRenderer` stages; the main thread admits geometry allocations first and, after receiving the immutable transformed document, admits that document's actual projection estimate. Pixel operations and CGImage construction stay on the worker. The canvas draws the resulting whole projection, so unlinked masks stay fixed, linked independent masks follow their own bounds, density/feather are evaluated after transformation, and group masks use the real compositor. Pending and failed previews label the previously displayed result explicitly. Session/revision/generation checks and teardown cancellation prevent late publication. Unmasked documents retain their existing interactive plate preview.

Geometry admission counts raster destinations at four bytes per pixel and linked independent mask destinations separately at one byte per pixel, including disabled masks and group masks. A true perspective warp has no second host homography implementation: its one linked scalar mask reserves the core's maximum allowed destination, 100 million bytes. This is conservative for small masks but remains below128MiB including the16MiB staging allowance for small raster extents. The core validates actual projective bounds before allocation. The later projection stage uses the resulting native geometry's resource report.

`scripts/selection-probe/mask_transform_preview.swift` qualifies the actual Free Transform session and canvas branch against the core's exact rendered bytes, with independent/unlinked density+feather masks, rapid latest-request replacement, perspective, cancel, one final undo, fixed group masks and separate allocation estimates. Masked multi-layer sessions use the raw pixel union for their box so opening a session does not construct effective-mask feather caches on the main thread. Described-source affine commits retain their existing AppKit rerasterization path; the preview is the raster sampling view of the captured source.

## Reproduction and tests

```sh
python3 scripts/phase6_quality_fixtures.py ~/devel/rasterize-fixtures/phase6/photos
RZ_VERIFY_APP=/path/to/isolated/Rasterize.app/Contents/MacOS/Rasterize \
  PYTHONDONTWRITEBYTECODE=1 python3 scripts/phase6_initial_masks.py \
    ~/devel/rasterize-fixtures/phase6/photos
cd core
cargo test --release --test selection_tools -- --test-threads=1
cargo run --release --example phase6_selection_quality -- \
  ~/devel/rasterize-fixtures/phase9i-reports/rz-phase6-quality-results \
  ~/devel/rasterize-fixtures/phase6/photos
```

The focused FFI suite covers independently computed sample/tonal coverage, source-alpha handling, sRGB versus P3 lightness, local edge stopping and bounded gradient growth, independent half-pixel/flip transform oracles, identity and contrast, reuse of existing morphology, a brute-force local-regression oracle for the candidate, invalid parameters, all four NULL exports, cancelled tokens, scratch refusal and live cancellation without partial publication. The live 1536×1024 radius-64 refinement test requires worker return within 500 ms of cancellation.

Measured logs/artifacts for this run are `/tmp/rz-phase6-selection-tests.log`, `/tmp/rz-phase6-initial-masks.log`, `/tmp/rz-phase6-quality.log` and `/tmp/rz-phase6-quality/results/metrics.json`. The parent Phase 6 status document records final app workflow checks, whole-core tests and current build qualification.
