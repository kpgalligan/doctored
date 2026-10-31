# Seven native separable layer modes (ADR)

- **Status:** Accepted — closed
- **Phase / dates:** 10D separable slice, 2026-09-13
- **Original:** [archive/phase-10d-separable-status.md](../archive/phase-10d-separable-status.md) — full history, measurements, receipts
- **Key commits:** `50cfea6` (Add seven native separable layer blend modes)

## Context
Follow-on to [phase-10d-basic-blending.md](phase-10d-basic-blending.md): add Overlay, Soft Light, Hard Light,
Darken, Lighten, Difference and Exclusion to native RGBA16 rasters and isolated groups without changing the
meaning of any existing save.

## Decisions
- **Kernels unchanged:** the existing `blend.rs` implements the
  [W3C separable equations](https://www.w3.org/TR/2024/CRD-compositing-1-20240321/#blendingseparable) over
  encoded document RGB through the f32 source-over path; f32 colour/alpha is kept through isolated and nested
  groups and stored as u16 only at the final projection.
- **Mode ownership:** `precision_layer_blend.rs` (Rust admission/persistence) and `NativeLayerBlending.swift`
  (Layers popup, capability reporting, MCP/Actions eligibility). Facade metadata parsing consumes the same host
  predicate as edit eligibility (an earlier basic-only numeric whitelist there was a bug).
- **No new FFI or Actions schema:** the version-1 `layer_blend` request and atomic throwing property
  preparation are reused; compound refusals publish nothing.
- **DRDC27 / `LBLD` schema 2** permits exactly modes 1–9 (Multiply, Screen, plus the seven). v26/schema 1 keeps
  meaning Multiply/Screen only and is still written when only those appear; removing all modes restores the
  default-only writer. A schema-1 file carrying an added mode refuses rather than acquiring meaning. Same
  framing and `16 + 8 × record_count` capacity; all validation precedes PNG decoding. The app routes v27 to the
  preserving native reader.

## Consequences
- Established the "oldest sufficient wrapper" writer rule every later slice extends.
- Qualified by [`core/tests/precision_layer_blending.rs`](../core/tests/precision_layer_blending.rs) (all 65,536
  coverage codes × nine modes, branch neighbours at 0.25/0.5, a subcode-alpha witness that fails if a group
  stores u16) and [`scripts/verify_phase10d_separable.py`](../scripts/verify_phase10d_separable.py) (0-code error
  vs independent f64 on its fixtures).

## Open items (verified 2026-09-26)
- `DONE` — Other layer modes, nonseparable modes, Dissolve — later slices (`b6c1fd7`, `9f8f37b`, `ac33773`, `dd3474f`).
- `DONE` — Clipping, styles, Blend If — `f65a0d2`, `7c130e2`, `b8aa5b5` (P7-20 verified).
- `DONE` — Adjustment-layer blending (P7-19) — `ea4d9d1`; knockout — P7-37 verified.
