# Phase 9I feasibility — preserved evidence and acquisition audit

This is the September 13, 2026 work-package-1 audit against starting HEAD
`19b7928`. Both original fixture acquisitions, the complete V3/V4 reports and
the earlier ViTMatte evidence remain available. Their presence enables a future
comparison; it supplies neither photographic transmission truth nor a qualified
new model. No dataset, checkpoint or package was acquired, no annotation was
invented, and no inference or new pixel scoring ran for this audit.

The [decision record](phase-9i-feasibility-status.md) owns the milestone outcome;
the [original protocol](phase-6-matting-reference-protocol.md),
[full-image protocol](phase-6-full-image-reference-protocol.md),
[foreground evidence](phase-6-foreground-evidence.md) and
[ViTMatte receipt](phase-6-vitmatte-spike.md) retain their original claims.

## Verified local inventory

The stdlib-only [inventory script](../scripts/phase9i_feasibility_inventory.py)
ran both pinned fixture verifiers, hashed retained reports and model files,
inspected PNG headers/color chunks, and independently recomputed V3/V4 gate
decisions from their serialized measurements. It did not rerun the estimators
or independently regenerate pixel measurements. Receipts are external:

- `/private/tmp/rz-phase9i-feasibility-evidence-audit-qualified/inventory.json`, SHA-256
  `03bc134d7180e31f631a59461cc9d6c8c28ea87fa53c71e6d773bc747cc26b89`.
- Its `original90-manifest.json`, SHA-256
  `3d5406190a69a33c08195cd8b29235c338ce85d0c070b47091373384655d433a`,
  contains all 90 unique case identities, original dimensions, reference
  populations and harness estimates. **This is scorer-only data**, not an
  annotation or inference manifest.

| Evidence | Local location under `/private/tmp/` | Audit result |
| --- | --- | --- |
| Four PhotoMatte originals, archive and permission source | `rz-phase6-quality/references/` | All pinned lengths, hashes and dimensions verified; no download needed |
| Seven complete-image references and attribution | `rz-phase6-full-references/` | All image/permission pins and exact manifest verified |
| Original guided and sampled records | `rz-phase6c-qualified/`, `rz-phase6-full-quality/` | Reports present and hashed |
| Closed-form comparison records | `rz-closed-form-quality/metrics.json`, `rz-closed-form-full/metrics.json` | Reports present and hashed |
| V3 and unchanged-alpha V4 | `rz-phase9i-soft-prior-v3/`, `rz-phase9i-soft-prior-v4/` | All 90 records each; every independently recomputed alpha/color/combined decision agrees |
| Existing parity and independent audits | `rz-phase9i-stability-audit/audit.json`, `rz-phase9i-continue-parity/parity.json`, `rz-phase9i-continue-quality-audit.json` | Present and hashed; previous receipts remain historical evidence |
| ViTMatte checkpoint and environment | `rz-vitmatte-spike-yytbfsx_/` | Four checkpoint files match original pins; Python 3.11.15 and all five direct package versions match |
| ViTMatte analytic and full reports | `rz-phase6-vitmatte-author-quality-final/`, `rz-phase6-vitmatte-full-quality/` and corresponding model reports | Present and hashed; author analytic 9/24 alpha, 0/24 combined; full 42/42 refused, no eligible outputs |
| Earlier source-only exports | `rz-phase6-vitmatte-inputs/inputs.json`, `rz-phase6-vitmatte-full-inputs/inputs.json` | 24 analytic and 42 full case manifests present; their old automatic hard-trimap policy is not the new assisted protocol |

The runtime verification imports only standard-library modules and the existing
model-check helper, never Torch or a model. Installed direct versions remain
`torch==2.8.0`, `transformers==4.57.6`, `numpy==2.3.2`, `Pillow==11.3.0` and
`safetensors==0.6.2`. `requirements.lock` has SHA-256
`11d7a3346bcdeebb9ed91ca20b657bd19af95432c42d088464d663a3b8675ed1`.
TorchVision, Numba and PyMatting are absent from this
environment. An old environment's presence does not qualify a new runtime.

The inventory pins reports and original fixture bytes, not every saved image
artifact. Existing PNG comparisons are available, but must receive their own
hash/extent audit when used for a new visual finding. In particular, saved V3/V4
artifact coverage is not equivalent to exported candidate planes for every case.

## Exact corpus, profiles and baselines

Both fixture manifests match the hashes recorded before V3/V4 scoring:

| Manifest | SHA-256 |
| --- | --- |
| `rz-phase6-quality/references/references.json` | `3bcdd07cd7bbd690ba857cd14f4f70e8762e6c462178f45c977908d70a1e08d9` |
| `rz-phase6-full-references/full-references.json` | `06dd848c8178f835b80c684508b837c4ff19e2763278be3aefd1f19904ee33f2` |

| Cases | Native evaluation extent | Original source backgrounds |
| --- | --- | --- |
| 24 analytic: portrait/hair, fur, scalar translucent glass, product | 320 × 256 | light, dark, **ambiguous** |
| 24 cropped portraits: four pinned PhotoMatte images | 1280 × 1280, crop `(512,384,1280,1280)` | light, dark, checker |
| 6 complete cat | 763 × 764 | light, dark, checker |
| 6 complete bottle silhouette | 800 × 600 | light, dark, checker |
| 6 complete Arduboy product | 944 × 1338 | light, dark, checker |
| 24 complete portraits | 2304 × 3456 | light, dark, checker |

Each subject/background retains both original degraded starts: radius-3 feather
and threshold-at-128 binary. Analytic alpha remains authored geometry with 4 × 4
area sampling. Controlled composites and all metrics use encoded RGB8; the
permitted reference-derived prior recipe is unchanged. Replacement scoring
always uses white, dark and checker, including for analytic ambiguous sources.

Header inspection finds no embedded ICC profile or `sRGB` chunk in any of the
seven reference PNGs. Cat has `gAMA=45455` and `cHRM` values corresponding to sRGB
primaries/white point; bottle has `gAMA=45455`; the other five contain none of
the inspected color chunks. The inventory records exact chunk hashes/bytes.
Preserve these original bytes and distinguish original metadata from the
benchmark's encoded-RGB arithmetic. Do not apply a new gamma/ICC transform,
claim calibrated linear-light truth, or silently label untagged data as a
verified profile. Any author normalization must be declared separately and
must leave the benchmark's source values and scoring domain reproducible.

The maximum original image is 7,962,624 pixels. The preserved harness estimate
`256*N + 64 MiB` is 2,105,540,608 bytes there, leaving 41,943,040 bytes under
2 GiB. This is a planning reservation, not measured RSS or spare permission to
add a model. A new complete pipeline must forecast its own process overlap,
including import, model/activations, foreground, scoring and retained outputs.

| Preserved baseline | Alpha passes: analytic / crops / full | Combined passes: analytic / crops / full | Refusals |
| --- | --- | --- | --- |
| V3 | 12 / 12 / 20 | 1 / 3 / 4 | 0 / 90 |
| V4 unchanged alpha | 12 / 12 / 18 | 0 / 3 / 4 | 0 / 90 |

V4's complete before/after alpha metric objects are identical for all 90 cases.
Every V3/V4 per-case gate recomputation agrees with its stored result. Select
these baselines globally; their differing passing cases do not authorize
per-case algorithm selection. Oracle-alpha color rows still evaluate the old
unprotected foreground estimator, not the V3/V4 protection policy or any newly
selected complete pipeline.

| Baseline report | SHA-256 |
| --- | --- |
| V3 `run.json` | `6dec114cebd9b5609d69a8347d0e5b9da3d8e06d908dfb2c23e89c8af19a1376` |
| V3 `analytic-crops/metrics.json` | `8d8225440cb225d66fa35d18d204393c6a19d415d6df060fcf4ad3478aa99620` |
| V3 `full/metrics.json` | `e6dac625f260f180140e3ed02b32266869a0d22ac49e4137c27b224d9f8d2540` |
| V4 `run.json` | `2dbbda8e21b7750db3f536e66a6cc9b35476f45c92a7c711dd6ff5f2cd883c72` |
| V4 `analytic-crops/metrics.json` | `6333ff168ce8a50fd54236608816c1fb78dbf93e8bd3bb95296a4634977edf82` |
| V4 `full/metrics.json` | `be60ef2c39d0c40d1e1af8c4712f6e481daf14ae7ad20b17c737daaa3a8b5bca` |

**The original measured V3/V4 binaries are not verified as retained.** Their
shared recorded executable path now contains SHA-256
`d4d21a70de7d72891c4d75470013cfb6f5a6a1fa1a040a5b484e9ba4be61c1d4`,
different from original V3 `a8e805ce…` and V4 `893b1052…`. Four of their ten
recorded source files also differ from the current tree: the Laplacian,
soft-prior example, shared matting evaluator and full-image adapter. The full
old/current hashes are retained in the inventory. This does not rewrite old
measurements; it prevents claiming that today's same-named file is the original
measured executable. A new build must be frozen under its own identity and
compared with the retained reports, using the required legacy parity checks.

## Photographic evidence and permission are separate prerequisites

The local evaluation continues under the pinned dataset-specific evidence;
software licensing is not used as a substitute. PhotoMatte provides extracted
portrait alpha/foreground annotations. Cat and product provide edited boundary
annotations. The bottle contains only 452 fractional pixels and 190,338 opaque
pixels, so its edited silhouette does not measure transmission through the
interior. Existing annotation defects remain part of the frozen reference.

Primary publisher pages were rechecked for unavailable alternatives; no dataset
archive was acquired, and no access form or agreement was submitted:

| Source | Available evidence and exact remaining dependency |
| --- | --- |
| [PhotoMatte85 publisher](https://grail.cs.washington.edu/projects/background-matting-v2/#/datasets) | Existing pinned publisher asset permits commercial/noncommercial use, requests academic citation and a commercial-product survey. This remains local evaluation; photographs are not bundled, redistributed or used for training. No survey submitted. |
| [Transparent-460](https://github.com/AceCHQ/TransMatting#transparent-460-dataset) | Publisher limits direct access to noncommercial research, makes downloading acceptance of terms, and requires author contact for commercial use. Intended-use permission and reference capture/foreground semantics remain unresolved; none obtained. |
| [AM-2k's current author repository](https://github.com/JizhiziLi/GFM#am-2k) | Natural animal RGB and manually labeled alpha; publisher requires reading its release agreement before access. This supplies no independently measured foreground RGB merely by multiplying RGB by alpha. No data acquired or agreement accepted. |
| [alphamatting.com](https://alphamatting.com/datasets.php) | Publisher offers corresponding 16-bit linear input/foreground and alpha and explicitly reports noise, including opaque foreground/input differences. Dataset permission for this intended use remains unestablished. Even with permission, these data need a separate declared linear/high-depth evidence protocol; they cannot silently replace the encoded-RGB exact-opaque gate. |
| [withoutBG100](https://withoutbg.com/tech/datasets/withoutbg100) | Publisher describes image/alpha pairs and requires contact for access. Dataset rights and independent foreground/transmission evidence are not established. No contact made. |
| [TOM-Net](https://guanyingc.github.io/TOM-Net/) | Author targets object mask, attenuation and refractive flow; reports synthetic training references and real evaluation samples. Those are distinct optical outputs, not an eligible replacement for scalar alpha plus fixed foreground. No inference of dataset rights from its MIT code license; no assets obtained. |

Ordinary scalar transmission qualification therefore still needs appropriately
licensed photographic alpha **and** foreground evidence with documented capture,
color domain, noise and reference eligibility. Refractive recomposition needs a
separate optical contract. An actual independently captured empty background can
support a separate workflow; an invented reconstruction from reference planes
cannot supply that evidence.

The independently reviewed RVM shortlist has a different permission gap. Its
[author README](https://github.com/PeterL1n/RobustVideoMatting) provides official
weight links and inference instructions, while its licensing announcement names
the **code**. That demonstrates intended inference use but does not separately
state checkpoint terms for the project's intended evaluation.
[Issue 281](https://github.com/PeterL1n/RobustVideoMatting/issues/281) asks whether
pretrained weights are GPL and has no displayed author answer. Under the plan's
explicit code/weight-rights prerequisite, checkpoint permission remains
unestablished; this is not a conclusion that private evaluation is prohibited.
The dependency is a publisher statement or license covering the exact checkpoint
and intended use. No author was contacted and no RVM weight was downloaded.

## Source-only isolation and independent scoring

The existing external adapter can export 48 analytic/crop inputs when supplied
the crop reference directory and 42 full inputs in its full-image mode. Existing
ViTMatte disk exports contain only the original 24 analytic plus 42 full cases.
They also carry the old automatic hard trimap policy and alpha-only result
contract. Their successful isolation/PNG checks are useful foundations; they
do not qualify joint foreground import or the new assisted semantics.

Before a new run, a source-only exporter must produce only observed source RGB,
permitted prior, geometry and declared color metadata. Its scoring process may
read truth to construct the original controlled source/prior recipe, but no
truth plane, population count, alpha feasibility bound, reference filename map,
metric, failure map or scored preview may enter the annotation/inference bundle.
The manifest generated by this audit includes truth-derived counts and is
explicitly ineligible as that bundle.

Blinding is about both access and prior knowledge. The root/agents that reviewed
ground truth or failures cannot relabel their own guesses as fresh source-only
human guidance. A fresh annotator must receive only the authorized bundle, with
opaque case identifiers and scorer-only identity mapping where necessary.
Resolve symlinks and reject paths escaping the bundle; allowlist file types and
metadata, and prevent reference-directory/network access by the annotator or
inference worker. Merely placing files in sibling directories while granting
unrestricted access to both is not isolation. Preserve attribution separately
without giving the annotator reference planes or failure information.

Freeze guidance origin, actual user acceptance where applicable, replay bytes,
input hashes and candidate identity before any scoring. Hash again afterward.
Record unknown releases, contradictions and refusals under the frozen interaction
budget. AI suggestions remain model output unless a real user explicitly accepts
them as intent; neither origin certifies physical opacity. No such annotations
or acceptance exist in this audit.

Every newly scored run needs independent recomputation of each alpha, color and
combined gate for every declared case, including refusals, plus separate exact
preservation checks outside write regions and inside protected regions. Preserve
reference-opaque color failures inside authorized correction regions. Count
admitted outputs separately from refusal/unchanged-prior diagnostics. If shared
scoring changes, additionally replay legacy guided/sampled/closed-form parity;
independent recomputation of a new report alone cannot prove that old scoring
semantics survived. Import checks must reject malformed dimensions, channels,
nonfinite/range errors before conversion or clipping can conceal them.

## Recovery procedure

Recheck existing evidence without inference or acquisition:

```sh
python3 -B scripts/phase9i_feasibility_inventory.py /tmp/new-9i-evidence-inventory
```

The script records missing files, pin mismatches and changed measured binaries;
its successful process exit only means the inventory was written. Require both
fixture verifier exits to be zero, all required pins to match, both unique
baseline case counts to be 90 and every gate comparison to agree before using
those records as complete evidence. The script never replaces a missing run.
The real inventory records `preserved_fixture_and_baseline_checks_pass=true`;
its `missing_or_changed` list still records the rebuilt original executable
and four changed source files. A second invocation against an intentionally
absent temporary root records zero cases, missing paths and a false summary,
without creating that input root. Its receipt is
`/private/tmp/rz-phase9i-feasibility-evidence-audit-missing-check/inventory.json`.

If temporary fixtures disappear, use the existing pinned procedures in a new
external directory, then verify them:

```sh
python3 -B scripts/phase6_reference_fixtures.py /tmp/recovered-photo-references
python3 -B scripts/phase6_reference_fixtures.py /tmp/recovered-photo-references --verify-only
python3 -B scripts/phase6_full_reference_fixtures.py /tmp/recovered-full-references \
  --photo-source /tmp/recovered-photo-references
python3 -B scripts/phase6_full_reference_fixtures.py /tmp/recovered-full-references --verify-only
```

The PhotoMatte recipe recovers the 413,386,865-byte archive and four pinned
members; the full recipe fetches the three pinned public PNGs and immutable
permission revisions and copies the verified portraits. A changed publisher
asset, missing source, permission prompt or checksum mismatch is a concrete
recovery dependency. Preserve mismatching files; do not refresh pins or follow
legal/account forms to manufacture successful recovery.

If reports or original binaries disappear, hashes cannot reconstruct them.
Restore exact copies from a retained backup if one exists. Otherwise retain
the historical documented results as such and create a separately identified
reproduction with frozen source/build/executable hashes using the original
protocols; do not claim its bytes or timings are those of the lost run.
The [V3/V4 reproduction](phase-9i-cutout-status.md#verification-and-reproduction) and
[ViTMatte recovery](phase-6-vitmatte-spike.md#reproduction-and-evidence) specify
their source recipes. No recovery or repeated large experiment was necessary
or performed during this work package.
