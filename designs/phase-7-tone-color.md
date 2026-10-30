# Shared high-depth tone and color processing (ADR)

- **Status:** Accepted — open items remain
- **Phase / dates:** 7C, 2026-09-10
- **Original:** [archive/phase-7-tone-color.md](../archive/phase-7-tone-color.md) — full history, measurements, receipts
- **Key commits:** `4061646`

## Context
The byte path quantized inside tone/color operations even when the compositor was f32: Curves used 256-entry byte
LUTs, and the calibrated ICC `ToneEncoder` returned `u8/255`. 7C had to give the typed workflow all 21 existing
adjustments and profile assign/convert **without** a second adjustment implementation and without changing any
legacy byte result.

## Decisions
- **One interpreter, one set of kernels.** Typed descriptions use the existing `type`/`op`/`params` envelope plus a
  **required** top-level `evaluation: "precision_v1"` (`core/src/adjust_precision.rs`); unknown operations, targets
  or tags refuse. *Why:* a parallel suite would drift from the byte behaviour.
- **No rounding inside evaluation.** Curves evaluates continuous splines (authored coordinates stay on the 0–255
  parameter scale); posterize/threshold remain intentional effects. Live layers keep f32 across the stack and groups.
- **Destructive apply** on a captured raster rounds RGB **once** to u16, keeps alpha exact and leaves zero-alpha
  latent RGB unchanged. Repeated destructive writes still round each time.
- **Calibrated Exposure/White Balance** default to the `document_v2` parameter policy; explicit `legacy_srgb` keeps the
  old numeric sRGB interpretation (separate from the evaluation tag). Shadows/Highlights uses float luma and the
  shared Gaussian guide. Gradient Map dither is ±½ u16 code.
- **Parameter schemas unchanged:** hex colors stay byte-valued, gradient positions/opacity keep 4-decimal
  normalization, LUT grid/Float32 import limits stay. These do not quantize source pixels.
- **Continuous ICC encoder** (`core/src/icc_precision.rs`): the shared forward curve evaluator sampled at 65,537
  encoded positions, interpolation uncertainty `< 1/65536`, a separate bound of `1/65536 + 6e-8` that does **not**
  relax the `2e-6` compositor gate. Non-strict curves pick the lowest preimage (no round-trip promise); malformed,
  nonfinite or overflowing curves/LUTs refuse. No independent curve inverter.
- **Profile semantics:** assign shares exact samples; convert transforms every distinct stored RGB allocation
  (including hidden RGB), preserves alpha and sharing, clamps to destination gamut (bounded SDR). Exact ICC bytes are a
  no-op; exact numeric matrix/TRC identity bypasses the round trip; the legacy approximate-equivalence rule may
  **not** skip a high-depth conversion. Live parameters are unchanged and re-prepared in the new profile, so a
  nonlinear stack can change appearance (no silent bake).
- **Native v12** adds versioned adjustment records with complete operator/record preflight before raster decode;
  v11 reads retained; byte documents keep v1–10 evaluation. Host archives: manifest v2 with v1 reads.
- **Admission:** JSON preparation allowance `64 × description bytes + 4 × ICC bytes + 4 MiB`. The native save plan
  and native reopen charge each record's measured retained operator data (plus ~1 KiB slack) and the largest single
  preparation allowance (`AdjustmentAdmission` in `core/src/precision_adjustment.rs`); the reader admits and reserves
  incrementally per record. ICC/JSON preparation cancels at stage/operator boundaries; pixel and guide loops poll
  cooperatively. Compact descriptor queries avoid copying names/LUTs while planning.

## Rejected alternatives
- Feeding floats through the byte `ToneEncoder` — returns byte levels; high depth would be lost at that call.
- A byte-tolerance profile-equality shortcut — sized in fractions of an 8-bit code.
- Baking live adjustments on profile conversion — would silently destroy editability (later offered only as an
  explicit choice, P7-22).

## Consequences
- Every typed adjustment added later must go through the same interpreter and `precision_v1` tag.
- Old documents must not change appearance because a new color policy exists (see carryover "Contracts that remain
  intentional" in [phase-7-carryovers.md](phase-7-carryovers.md)).
- Qualification: independent scalar references for all 21 operators (max `2.384e-7`), full-u16 Levels/Curves and
  Bezier references, ICC maxima below the separate bound; 12/12 stages of a 24 MP color workflow.

## Open items (verified 2026-09-26)
- `DONE` — public central Undo, snapshots and recovery (7E, [public mode](phase-7-public-mode-status.md)).
- `DONE` — masks/scalar targets, clipping, styles, other blend modes for typed documents (7D, then carryovers
  P7-08, P7-19, P7-20, verified).
- `DONE` — appearance-preserving profile conversion as an explicit alternative (carryover P7-22, verified by 11C).
- `DONE` — the native save plan and native reopen charge
  summed measured retained operator data plus the peak single preparation allowance, admitted per record on reopen,
  so a full 1,024-record stack of simple adjustments saves and reopens; the working limit is unchanged.
- `DONE` — profile assign/convert and 8→16 / float→16
  re-preparation charge through the same `AdjustmentAdmission` rule: measured records (a rebind's bounded from the
  old record plus 4× the destination profile for a calibrated operator, charged once for a conversion, which reuses the rebind's records) plus the peak single
  allowance, so a 600-adjustment stack promotes, assigns and converts (`core/tests/adjustment_repreparation.rs`).
- `DONE` — the 8→16 and float→16 plan walks admit each
  adjustment preparation (`AdjustmentAdmission::next`, seeded with the plan's retained bytes, the bookkeeping and
  the live source shapes) BEFORE building it, against the op's own limit; a conversion reuses `rebind_layers`'
  records, so `color_plan` charges them once for assign and convert alike.
- `TRACKED` — slow full-frame live projection / stored-source conversion at 24 MP (6.08 s / 3.68 s) → regional work
  P7-31 (13A) and latency qualification P7-33 (13D).
- `TRACKED` — stage-only codec cancellation, noncancellable compound export, uncompressed PNG/native payloads →
  carryover P7-32 (13C).
