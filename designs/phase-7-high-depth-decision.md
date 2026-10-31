# High-depth storage representation (ADR)

- **Status:** Accepted — open items remain ([#9](https://github.com/kpgalligan/doctored/issues/9), [#11](https://github.com/kpgalligan/doctored/issues/11))
- **Phase / dates:** 7A, 2026-09-10
- **Original:** [archive/phase-7-high-depth-decision.md](../archive/phase-7-high-depth-decision.md) — full history, measurements, receipts
- **Key commits:** `082ccb1`

## Context
The editor, the byte C ABI and native v10 stored straight RGBA8, and source distinctions were lost before
a document even existed (`to_rgba8()` on import, byte group quantization, byte ICC tone encoders). Phase 7A
compared two typed stores (RGBA16 UNORM vs RGBA32 normalized float) in a safe Rust prototype, without
exposing a document mode, to pick the representation for the rest of Phase 7. The
[operation matrix](phase-7-operation-support.md) records the migration each operation needed.

## Decisions
- **Store RGBA16 unsigned-normalized, process in f32.** *Why:* every 16-bit PNG/TIFF code survives, and it halves
  retained memory vs float; float was more accurate across repeated sub-code writes and faster, a tradeoff accepted
  knowingly. Destructive u16 edits round at each committed write — never advertise sub-u16 retention.
- **Typed images are a distinct type** (`core/src/precision_image.rs`, `core/src/precision_render.rs`), never an
  `DrImage`/`DrDocument`. *Why:* no byte filter or writer can accept a typed plane by accident. Byte accessors
  (`dr_image_pixels_rgba`, painting overlays, layer-plane reads) keep their byte meaning.
- **Evaluation contract** (still binding, via `precision_render.rs` / `blend.rs`):
  - Straight alpha, row 0 top; alpha independent of the RGB transfer curve; hidden RGB under zero alpha is kept in
    storage; a fully transparent composite has zero RGB.
  - Group intermediates stay f32; quantization happens once, at an explicit storage boundary, by nearest rounding
    after clamping (`round(v·65535)` / `round(v·255)`).
  - Byte→u16 promotion is exactly `v·257`; u16→f32 is `v/65535`. Promotion adds no information.
  - Compositing requires one captured `Arc<IccProfile>`; no implicit conversion or working-space adoption.
  - A positive effective alpha below the floor refuses rather than losing straight RGB to underflow. 7A's floor was
    2^-32; 7D widened it to **2^-96** (see [pixel editing](phase-7-pixel-editing.md)).
  - Previews are explicit RGBA8 conversions — never a source, typed layer or save input.
- **Numerical gates fixed before measuring:** f32 output within `2e-6` of an independent f64 premultiplied
  reference; u16 output within `2e-6 + 0.5/65535`. Later phases reuse these bounds unchanged.
- **Legacy files keep their appearance.** Old byte storage, encoded compositing, isolated-group byte rounding and
  `document_v2` adjustments are never routed into the typed path. Converting an old editable document requires an
  explicit evaluation choice — promoting bytes ×257 alone does not preserve group/adjustment results.
- **Persistence deferred to 7B** with an explicit storage/evaluation tag, a version bump, backward readers and shared
  writer/reader limits (delivered as native v11, [neutral path](phase-7-neutral-path.md)).
- **Masks/channels planned as u16**; byte coverage may be promoted ×257 with its 256-level limit disclosed; a
  high-depth alpha/plane edit never passes through a byte mask API.
- **Resource admission:** unique retained buffers counted through the shared inventory; 1,024 records, 16 group
  levels, 2 billion pixel visits; the tracked working limit (2 GiB at the test pin; qualification at the 8 GiB
  minimum). *Why:* bounded, refusable work. The tracked limit is
  **not** an OS RSS ceiling (freed large allocations stayed resident: 3.05 GiB RSS vs 1.92 GB live).
- **A refused size is a failed target.** No resized substitute may count as a pass; the limit is the RAM-scaled one, pinned in
  tests.
- **No-op conversions** must be detected before the central mutation path so they create no Undo step.

## Rejected alternatives
- RGBA32 float as the first public store — double memory; its accuracy edge applies to live stacks, which evaluate
  in f32 anyway. (Later added as a *separate* mode by 11F, not a replacement.)
- Passing floats through the existing byte `ToneEncoder` — it returns `u8/255`; 7C had to generalize it instead.
- Reusing byte `MatrixTrc::approx_eq` tolerances for high-depth profile equality — they are sized in 8-bit codes.
- Putting a 16-bit PNG inside an old native record — the byte reader calls `to_rgba8()`; a versioned record is needed.

## Consequences
- All later typed work (7B–7E, 8–13) builds on u16/f32, the fixed error gates and the explicit-boundary rule.
- Reproduce the comparison with `cargo run --release --example phase7_precision` in `core/`
  ([`core/examples/phase7_precision.rs`](../core/examples/phase7_precision.rs)).
- The prototype's `Arc<Vec>` representation has since become private tiles (13B-3b-1); the contracts above are
  representation-independent.
- The float range was `[0,1]` finite only in 7A; 11F's float document mode later admits extended range under its own
  contract.

## Open items (verified 2026-09-26)
- `DONE` — 7B versioned typed persistence (native v11, `2d9bc69`).
- `DONE` — generalized continuous ICC encoder and float live-stack evaluation (7C, `4061646`;
  [tone/color](phase-7-tone-color.md)).
- `DONE` — no-op precision conversion refused before mutation
  (`app/Sources/PrecisionModeConversion.swift`, "The document already uses the requested precision.").
- `DONE` — public-app decode/display/Undo/recovery residency measured for the declared 24 MP workflow (7E,
  [public mode](phase-7-public-mode-status.md)).
- `OPEN` [#11](https://github.com/kpgalligan/doctored/issues/11) — 48 MP grouped workflow (refused at 2 GiB in 7A;
  qualification is now at the 8 GiB minimum) and broader large multilayer/history residency (carryover P7-33).
- `OPEN` [#9](https://github.com/kpgalligan/doctored/issues/9) — the regional render path those workloads need
  (carryover P7-31).
- `DROPPED` — "float prototype is comparison-only, no second public mode": superseded by the 11F 32-bit float
  document mode (carryover P7-25, verified).
