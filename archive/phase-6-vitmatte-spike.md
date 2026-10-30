# Phase 6C ViTMatte offline qualification spike

The pinned Hugging Face conversion of ViTMatte-S ran successfully on all 24
original native-size analytic inputs. Its packaged normalization passed 8 alpha
cases; a separately declared author-normalization variant passed 9. Neither
passed an end-to-end alpha/color case. A subsequent attempt on 42 full-image
photographic cases admitted no output under the same memory policy: 30 refused
before model execution, and 12 crossed the measured RSS threshold. This is an
offline experiment: no model, Python
runtime, package dependency, control, or inference path is added to the app.
The original thresholds and the frozen automatic trimap were not changed.

## Model provenance and implementation

The checkpoint is
[`hustvl/vitmatte-small-composition-1k`](https://huggingface.co/hustvl/vitmatte-small-composition-1k),
at revision `6a58ad7646403c1df626fbd746900aec7361ea1d`. Its model card identifies
Composition-1k training and declares Apache-2.0. The card was written by the
Hugging Face team. The author-maintained
[ViTMatte repository](https://github.com/hustvl/ViTMatte) uses MIT; the
[Transformers conversion](https://github.com/huggingface/transformers/blob/main/src/transformers/models/vitmatte/convert_vitmatte_to_hf.py)
renames the original `ViTMatte_S_Com.pth` parameters into its own implementation.
This experiment downloads only the safetensors conversion, never the original
pickle checkpoint. No form, account, credential, or acceptance was required.

| File | Bytes | SHA-256 |
| --- | ---: | --- |
| `model.safetensors` | 103,294,572 | `bda9289db1bb6762d978b42d1c62ae3f34daf7497171a347a1d09657efd788cb` |
| `config.json` | 837 | `ae1006f5a83227048b563b2e60709d4203e432b2276949ebef41a8cfeeeaf45f` |
| `preprocessor_config.json` | 284 | `0db558038b96a3f5c97e46d4ec8966fcc479e9aa58a391bca60b5094a5f7fee0` |
| `README.md` | 1,716 | `5545af4bf41cc017cd1a75ff096d27ff4e6e6ea0bb41c9a272dd4072a0fefaaf` |

The weight size and checksum are also published in the
[repository's small pointer file](https://huggingface.co/hustvl/vitmatte-small-composition-1k/raw/main/model.safetensors).
Each file is fetched at the pinned revision and checked before inference.

The saved Hugging Face processor specifies RGB mean/std `0.5`. The author
repository's [common configuration](https://github.com/hustvl/ViTMatte/blob/main/configs/common/model.py)
specifies ImageNet normalization, and its
[one-image demo](https://github.com/hustvl/ViTMatte/blob/main/run_one_image.py)
hardcodes CUDA despite accepting a device argument.
The first run is explicitly the **Hugging Face conversion with its supplied
processor**. Neither run claims to reproduce the current author's complete
execution path or paper benchmark numbers.

A subsequent primary-code audit found that the converter only renames keys;
it does not fold the normalization change into the weights. The
[author model](https://github.com/hustvl/ViTMatte/blob/main/modeling/meta_arch/vitmatte.py)
normalizes RGB explicitly, and its mean/std buffers are nonpersistent. We
therefore declared a second `--normalization author` variant using exactly
`[123.675,116.280,103.530]/255` for mean and
`[58.395,57.120,57.375]/255` for standard deviation. Every other input, weight,
padding, quantization, known-label rule, resource bound and scoring threshold
remained unchanged. This correction follows the published preprocessing code,
not the observed quality scores. The packaged run remains intact; neither
variant claims complete original-code parity.

## Frozen inference policy

A new temporary environment used existing Python 3.11.15 and these exact direct
dependencies: `torch==2.8.0`, `transformers==4.57.6`, `numpy==2.3.2`,
`Pillow==11.3.0`, and `safetensors==0.6.2`. The complete resolved environment is
recorded in `model-report.json` and the temporary `requirements.lock`. No
Detectron2, TorchVision, CUDA, or app integration is used. The model contains
25,814,753 parameters.

- CPU, float32, evaluation/inference mode, four intra-operation threads and one
  inter-operation thread; one case per subprocess.
- Only the separately exported source RGB and categorical trimap enter the
  model. The degraded prior is hashed for provenance. No reference alpha or
  foreground appears in the inference directory.
- Use `1/255` rescaling and the explicitly named normalization variant: the
  packaged RGB mean/std `0.5`, or the author constants above. Pad only
  the bottom/right to a multiple of 32; remove exactly that padding afterward.
  Do not resize the image or matte. All 24 analytic inputs are already 320×256.
- Quantize raw sigmoid output by `floor(clamp(alpha,0,1)*255+0.5)`. Then enforce
  supplied categorical background `0` and foreground `255` exactly. Unknown
  `128` pixels keep the network output. This manual-constraint policy was fixed
  before scoring; it is not an alpha cutoff chosen from reference data.
- Save separate `raw-alpha.png` and constraint-enforced `alpha.png` files, both
  native 8-bit grayscale. Record every input and output hash, padding dimensions,
  elapsed time, memory measurement, and number of enforced known bytes.
- Set the Hugging Face/Transformers offline flags; load only local safetensors
  with remote code disabled. Inference makes no network requests.

The controller watches only its own subprocesses. Each has a 120-second total
wall limit and a monitored 2 GiB RSS rejection threshold. A post-exit check of the child's `ru_maxrss`
catches brief allocation peaks missed by polling; elapsed time and manifest
geometry are checked again after exit. Refused results are removed, never left
as eligible alpha files. Five stdlib-only guard tests cover these cases and
repository-output refusal.

## Memory and platform limits

The fixed preflight is a planning estimate, followed by measured RSS admission.
It is `1 GiB + 256 × padded_pixels + 6 × attention_matrix_bytes`, where
one float32 attention matrix is `6 heads × tokens² × 4 bytes`, with 16-pixel
patches. Six simultaneous matrix copies are an allowance for attention,
relative-position additions, softmax, and staging, not an empirical claim that
all six are always retained. The full-image measurements below show that this
allowance underpredicts actual execution memory at the smaller photographic
sizes; it is not a proven upper bound.

The author and Hugging Face implementations both contain full global attention
blocks. One matrix alone is about 983 MB at 1280×1280. The full preflight there
is 7,391,412,224 bytes, so this spike refuses that size before inference. Native
320×256 has a 1,109,458,944-byte estimate. The full 763×764 cat and 800×600 bottle
fit the preflight, but every one of their 12 background/start variants crossed
the measured RSS threshold. Their observed peaks at termination ranged from
2,288,140,288 to 2,849,144,832 bytes. The other 30 full-image cases refused in
preflight. The 50 ms process monitor is a termination threshold, not an OS memory
limit: an allocation can overshoot before the next sample. No refused result is
admitted or retained as an alpha output. No reduced-size or tiled result is
presented as native full-image qualification.

Local MPS availability was true, but MPS inference and CPU/MPS parity were not
tested. The [PyTorch MPS backend](https://docs.pytorch.org/docs/stable/notes/mps.html)
provides the prospective macOS GPU route. Core ML conversion is also untested;
[Apple's conversion workflow](https://apple.github.io/coremltools/docs-guides/source/convert-pytorch-workflow.html)
requires graph capture and conversion. Dynamic position interpolation, global
attention memory, supported operators and numeric parity would need separate
measurement. Core ML Tools is not installed here.

## Results and remaining constraints

The original 24-case analytic corpus produced these results:

| Check | Result |
| --- | --- |
| Native inference completion | 24/24, no refusals |
| Peak child RSS | 963,330,048 bytes, about 919 MiB |
| Model evaluation time per case | 0.133–0.297 seconds |
| Maximum subprocess wall time | 14.63 seconds, cold first process |
| Unchanged alpha gate, packaged normalization | 8/24 pass |
| Unchanged alpha gate, author normalization | 9/24 pass |
| Color gate with estimated alpha | 0/24 pass |
| Joint alpha/color gate | 0/24 pass |
| Separate oracle-alpha color diagnostic | 20/24 pass |

Both normalization variants completed 24/24 cases without refusal; the zero
estimated-alpha color and joint pass counts apply to both. The author's variant
peaked at 959,840,256 RSS bytes. The table's first timing/memory rows describe
the packaged run; per-case measurements for both are retained separately. The
author-normalization model evaluation took 0.155–0.189 seconds per case, with a
maximum subprocess wall time of 4.386 seconds.

The separate native full-image run used author normalization and the same
weights, hard known-label enforcement, 2 GiB threshold, and 120-second wall
limit. All 42 cases are accounted for: 30 preflight refusals and 12 measured-RSS
refusals, zero eligible alpha results. The 12 executed children were terminated
after 4.20–4.39 seconds, so these wall times do not measure completed inference.
The bounded experiment provides no full-image alpha-quality claim for this
model. A new implementation or resource policy would require a separately
declared experiment; these refusals remain part of this run.

The final full-image scorer confirms 42 alpha refusals and zero joint passes.
Its separate oracle-alpha color diagnostic passes 30/42; this uses supplied
reference alpha and does not establish any successful model inference. For
refused candidates, the scorer reports unchanged-prior metrics only as a
diagnostic and never counts those values as candidate success.

The external Rust adapter reconstructs and compares each source/prior/trimap
before importing the alpha result. It first inspects PNG headers using the same
open file that will be decoded: dimensions must match the native case, source
storage must be RGB8, and prior, trimap and output alpha must be L8. It sets
strict decoder geometry limits and a best-effort decoder allocation allowance
of `16 × pixels + 1 MiB`, including reservation for decoded output, within the
harness's `256 × pixels + 64 MiB` working estimate. Palette, higher-bit-depth,
RGB alpha, mismatched geometry and changed input pixels are refused rather than
converted. The shared candidate contract additionally refuses an output that
changes any known trimap byte. It uses the unchanged scorer and foreground
estimator; oracle alpha is used only by the separate diagnostic, never inference.

After this import hardening, all 24 author-normalization results remained
identical in every field apart from timing and equivalent artifact paths
resolved to the same canonical files. The guard-parity report records this
comparison; no pixels, score thresholds or quality results changed.

Useful improvements in some blurred inputs do not establish broad success.
For example, light-background portrait/hair boundary MAD fell from 0.141659 to
0.018222. The ambiguous product binary case instead worsened from 0.001745 to
0.403556. Neither a successful inference nor a learned prior guarantees the
reference-opaque color condition.

The separate constraint audit proves that six analytic cases cannot reach the
0.95 soft-detail recall requirement while respecting the frozen known labels:
three binary portrait/hair cases have a maximum possible recall of 0.923205,
and three binary glass cases have a ceiling of 0.282109. This limit is about the
supplied automatic trimap, not a reason to alter the score or secretly relax
known user labels. A new automatic input policy or additional manual evidence
must be evaluated as a separately identified experiment, retaining these results.

The checkpoint does not require downloading Composition-1k data for inference.
The author testing instructions refer to a separate dataset containing
Adobe-licensed material; no such training/test dataset was downloaded. This run
uses the project's existing analytic corpus and the separately licensed
[full-image references](phase-6-full-image-reference-protocol.md).
Checkpoint/software licensing does not substitute for a fixture's own source
permissions.

## Reproduction and evidence

The weights and installed environment are temporary artifacts, not repository
files. Download the four pinned model files above from
`https://huggingface.co/hustvl/vitmatte-small-composition-1k/resolve/6a58ad7646403c1df626fbd746900aec7361ea1d/`
into a private model directory, preserving their names. The inference script
checks exact sizes and hashes before using them. Create a separate temporary
Python 3.11 environment and install the five pinned dependencies listed above.

```sh
python3 -B scripts/test_phase6_vitmatte_spike.py
core/target/release/examples/phase6_external_matte_quality export \
  /tmp/new-vitmatte-export-report /tmp/new-vitmatte-inputs /tmp/unused
/path/to/private/venv/bin/python -B scripts/phase6_vitmatte_spike.py \
  --model /path/to/private/model --inputs /tmp/new-vitmatte-inputs \
  --outputs /tmp/new-vitmatte-output --normalization packaged
core/target/release/examples/phase6_external_matte_quality score \
  /tmp/new-vitmatte-quality /tmp/new-vitmatte-inputs /tmp/new-vitmatte-output
```

Use `--normalization author` with a different new output and score directory
to reproduce the author-normalization variant.

For the separate full-image attempt, first acquire the verified references using
the [full-image protocol](phase-6-full-image-reference-protocol.md), then use the
external adapter's `--full-images` flag before `export` and `score`, supplying
the verified reference directory as the final positional argument. Run the
same Python command with `--normalization author` and new full-image input and
output directories. Keep the resource policy unchanged; missing alpha files
from refused cases are expected and must remain refusals in the score.

Recorded run: `/tmp/rz-vitmatte-spike-yytbfsx_/` contains the environment, pinned
model and download/install provenance. `/tmp/rz-phase6-vitmatte-output/model-report.json`
contains policy, complete package versions, hashes and case execution records.
For this external adapter, the scorer's `candidate_ms` measures PNG import and
input validation, not neural inference. Use `inference_seconds` and
`wall_seconds` in the model report for model evaluation and total subprocess
cost respectively.

Scores are `/tmp/rz-phase6-vitmatte-quality/metrics.json`; inference logging is
`/tmp/rz-phase6-vitmatte-inference.log`. Adapter integrity evidence is
`/tmp/rz-phase6-external-adapter-check/verification.json`. Author-normalization
outputs are `/tmp/rz-phase6-vitmatte-author-output/`, with scores at
`/tmp/rz-phase6-vitmatte-author-quality/metrics.json`. Full-image input provenance
is `/tmp/rz-phase6-vitmatte-full-inputs/inputs.json`, and all 42 resource decisions
are in `/tmp/rz-phase6-vitmatte-full-output/model-report.json`, with child logs
in the corresponding case directories.

Final hardened author-normalization scores are
`/tmp/rz-phase6-vitmatte-author-quality-final/metrics.json`; its
`guard-parity.json` proves exact parity with the original 24-case author score
after excluding timing and canonicalizing artifact paths. Final full-image
scores are `/tmp/rz-phase6-vitmatte-full-quality/metrics.json`.
The neighboring `run.json` joins each of the 42 score refusals to its inference
resource cause and records hashes of the model report and scorer executable.
