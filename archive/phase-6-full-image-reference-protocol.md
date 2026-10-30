# Phase 6C full-image reference qualification

This extends the [frozen reference protocol](phase-6-matting-reference-protocol.md)
with seven complete photographic cutouts. It does not replace the original
24-case analytic gate, change the earlier portrait crops, or reinterpret their
recorded results. All new measurements use each supplied image at its full native
size, with no crop or resampling. Source selection and the memory envelope were
fixed before candidate measurements.

## Supplied references and their limits

| Reference | Full dimensions | Author and permission | What the supplied alpha supports |
| --- | --- | --- | --- |
| [British Longhair cutout](https://commons.wikimedia.org/w/index.php?title=File%3ABLH_cat_transparent.png&oldid=810538237) | 763 × 764 | Chatterie des Millenovæ; removal by YukiKoKo; CC BY-SA 4.0 | Edited fur coverage with 44,999 fractional pixels. Red, yellow and green pixels remain on parts of the outer boundary; supplied colors and alpha are annotations, not physical truth. |
| [Colorful bottle](https://commons.wikimedia.org/w/index.php?title=File%3AColorful_bottle.png&oldid=943421598) | 800 × 600 | Matthew Bowden; transparency edit by Arc96; attribution-only permission, confirmed by Wikimedia VRT | Glass silhouette only: 452 fractional and 190,338 opaque pixels. Its interior retains background/shadow appearance. It provides no transparent-interior or refractive ground truth. |
| [Arduboy cutout](https://commons.wikimedia.org/w/index.php?title=File%3AArduboy_transparent.png&oldid=801139476) | 944 × 1338 | Drummyfish, own photograph/edit, CC0 1.0 | Supplied hard-product boundary coverage; not measured optical alpha. |
| Four previously pinned [PhotoMatte85 portraits](https://grail.cs.washington.edu/projects/background-matting-v2/#/datasets) | 2304 × 3456 each | Dataset contributors; Lin et al., CVPR 2021; publisher's recorded commercial/noncommercial permission | Full green-screen-derived foreground/alpha annotations, evaluated separately from the earlier 1280 × 1280 crops. |

All photographs remain outside the repository. The acquisition records author,
license, source revision, image checksum and exact permission-evidence checksum.
An `ATTRIBUTION.md` accompanies each fixture and output directory. Cat derivatives
retain CC BY-SA 4.0 and identify background/alpha/color changes; that asset license
does not replace the independently authored software's license. No person was
contacted, no account was used and no agreement form was submitted.

The cat's [original photograph](https://commons.wikimedia.org/wiki/File:British_Longhair_-_Black_Silver_Shaded.jpg)
was published by its creator under compatible attribution/share-alike terms.
The bottle's public file page records the copyright holder's permission for
commercial use with attribution; the private VRT correspondence was not accessed.
The product's creator explicitly identifies both the photograph and transparency
edit as their own work. Existing annotation defects are retained and disclosed;
we do not clean the reference to favor a candidate.

There is **still no photographic physical-translucency reference** in this set.
[Transparent-460's official repository](https://github.com/AceCHQ/TransMatting)
restricts direct use to noncommercial research, requires contact for commercial
use, and makes download conditional on its terms; it was not downloaded.
[AM-2k](https://github.com/JizhiziLi/animal-matting) requires a release agreement
and supplies natural image/alpha pairs, which cannot be treated as independently
known foreground RGB. It was not used to manufacture foreground truth.
[TOM-Net](https://github.com/guanyingc/TOM-Net) targets attenuation and refractive
flow, with synthetic reference generation; those targets cannot honestly be
substituted for this benchmark's scalar alpha and fixed foreground RGB.

## Pins and acquisition

[scripts/phase6_full_reference_fixtures.py](../scripts/phase6_full_reference_fixtures.py)
fetches only the three selected public PNGs and immutable description revisions.
The original four PhotoMatte PNGs and permission evidence are copied from the
already verified acquisition; there is no second archive download. Every asset
and evidence file is checked by length and SHA-256 before exclusive publication.
A mismatching existing file or manifest is preserved and refused.

| New image | SHA-256 |
| --- | --- |
| `fur-longhair.png` | `204a5dc854a60fa8bd1938e5d0c75ab6390852801ea19bf7693485a8892570d7` |
| `glass-bottle.png` | `f8fbdde9a9f423a52b4b4a3b32de1305a4e6cf82f9688397841e65195fc1b2e3` |
| `hard-arduboy.png` | `1617d139a5369f343ef932303a7c1856cd82edc5b81efa1163e6e099643f7909` |

The JSON manifest also pins each original dimension, RGBA8 storage, PNG
interlacing, source revision and reference limitation. The bottle is an interlaced
PNG; it is decoded as supplied, rather than replaced with a regenerated fixture.
Exact manifest checks reject attempts to change dimensions or label an edited
silhouette as physical transparency truth.

## Full-image execution

The full benchmark has 42 cases: seven references × light/dark/checker
backgrounds × the original blurred/binary starting masks. All compositing and
metrics explicitly use encoded RGB. The reference is the supplied foreground
and alpha used in a controlled recomposition, not the original natural scene.
The automatic trimap is still derived only from the degraded prior. Kernels
never receive the reference foreground or reference alpha as extra inference
input. Oracle-alpha foreground estimation remains an explicitly separate
experiment, duplicated across start labels for alignment rather than independent
foreground evidence.

The full adapter imports the frozen evaluator through a small solver interface:
`Candidate` receives source RGB, dimensions, categorical trimap and degraded
prior; its result supplies alpha, diagnostic metadata and optional confidence.
The sampled default keeps its original names and settings. Alternative solvers
use `alpha_candidate` and `estimated_alpha_color` report fields so their output
is not mislabeled as a sampled result. Nonconvergence or intrinsic work refusal
counts as failure, never a plausible-looking partial success.

Before decoding an image, a fixed conservative envelope admits
`256 × full_pixels + 64 MiB` against 2 GiB. It encloses foreground-estimation
scratch, borrowed/decoded inputs, float colors, coverage/result planes and
comparison canvases. The optional working limit may reduce that allowance,
but cannot raise it. An image over the envelope records all six cases refused
with its original dimensions and estimate; no smaller crop or preview silently
replaces it. The kernel's unknown-pixel, boundary-sample or iteration/work limits
remain additional refusals. Memory estimates are conservative reservations, not
measured resident-memory claims.

Each source's fixed light/blurred artifact case exports complete native-size
source, reference/prior/estimated alpha, foreground and replacement-background
composites. The 512-pixel panels are just inspection conveniences; they do not
reduce any inference or metric extent. Source attributions are copied beside
the images. The original and newer benchmark output directories remain separate.

```sh
python3 scripts/phase6_full_reference_fixtures.py ~/devel/rasterize-fixtures/phase6/full-references \
  --photo-source ~/devel/rasterize-fixtures/phase6/references
python3 scripts/phase6_full_reference_fixtures.py ~/devel/rasterize-fixtures/phase6/full-references --verify-only
python3 scripts/test_phase6_full_reference_fixtures.py
cd core
CARGO_BUILD_JOBS=4 cargo build --release --example phase6_full_image_quality
cd ..
python3 scripts/phase6_full_image_qualification.py ~/devel/rasterize-fixtures/phase6/full-references \
  ~/devel/rasterize-fixtures/phase9i-reports/rz-phase6-full-quality --require-pass
```

The output directory must be new and outside the repository. `--require-pass`
returns failure when any complete case fails the unchanged numerical gates,
while preserving `run.json`, logs and detailed metrics. A separate run with
`--working-limit-mib 64` exercises admission refusal without decoding or solving
full images. All Python entry points suppress bytecode caches before importing
repository helpers.

## Recorded full-image results: September 10, 2026

The frozen sampled candidate evaluated all 42 cases in 164.6 seconds at the default
2 GiB envelope, with no intrinsic work or memory refusal. Its alpha gate passed
15/42, foreground color given reference alpha passed 30/42, and coupled color and
end-to-end gates passed 0/42. Strict invocation returned 1 for this quality failure,
while preserving the complete report and native outputs.

| Full reference | Alpha passes /6 | Oracle-alpha color passes /6 |
| --- | ---: | ---: |
| British Longhair | 4 | 6 |
| Glass silhouette | 2 | 0 |
| Arduboy product | 1 | 0 |
| PhotoMatte 0003 | 1 | 6 |
| PhotoMatte 0031 | 0 | 6 |
| PhotoMatte 0060 | 3 | 6 |
| PhotoMatte 0088 | 4 | 6 |

Oracle-color rows repeat the same reference-alpha experiment for the two start
labels: 30/42 represents 15/21 distinct source/background scenes. It is not 42
independent foreground-color experiments. All 42 estimated-alpha outputs change
some reference-opaque RGB; additionally 31 cases exceed the replacement-background
boundary error ceiling, 17 exceed the fractional foreground error ceiling and 13
miss the required visible-fringe improvement. These overlapping failure counts
separate strict byte preservation from broader reconstruction errors.

A post-hoc constraint audit shows 6/42 cases cannot meet the alpha gate under the
frozen trimap's known labels, even with perfect unknown-region alpha. This uses
reference alpha only in the diagnostic bound; it supplies no extra input to any
candidate. The separate closed-form prototype measured 15/42 full-image alpha
passes and 0/42 end-to-end passes, with no full-image refusal. Its residual and
convergence evidence belong to the separate solver report, not to the sampled
candidate's measurements.

The 64 MiB admission check records 42/42 refusals with original dimensions and the
required estimates, and creates no PNG results. Independent header checks confirm
all 42 normal-run record dimensions and all seven full source/reference/estimated
artifact sets match the supplied native dimensions. The 512-pixel panels do not
stand in for inference or metric extent.

The shared-evaluator refactor also passed exact regression comparison against the
previous 48-case sampled report: every field and numeric value is identical after
excluding only elapsed `candidate_ms` and the added `constraint_audit` metadata.
The guided baseline remains 12/24. The existing gates and previous results are
preserved.

Private evidence:

- `/tmp/rz-phase6-full-quality/run.json`: measured executable and manifest hashes.
- `/tmp/rz-phase6-full-quality/images/metrics.json`: all 42 records and reference metadata.
- `/tmp/rz-phase6-full-quality/extent-audit.json`: complete native-dimension checks.
- `/tmp/rz-phase6-full-quality/failure-reasons.json`: independent frozen-gate reasons.
- `/tmp/rz-phase6-full-admission/`: reduced-budget refusal report.
- `/tmp/rz-phase6c-sampled-parity/exact-parity.json`: exact 48-case regression check.
- `/tmp/rz-closed-form-full/metrics.json`: separate closed-form results.

The initial full report predates serialization of the shared `color_gate` object;
its diagnostics explicitly identify the original frozen gate report used. Future
builds serialize the same contract directly. This metadata addition changes no
measured image or numerical result. All actual executable hashes remain recorded
with the runs that used them; no candidate was rerun solely for metadata.
