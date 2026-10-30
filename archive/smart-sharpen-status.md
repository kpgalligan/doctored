# Smart Sharpen measured against Photoshop — status

- **Phase:** 1 of 5 (the Photoshop measurement) done 2026-10-09; phases 2–5 wait on the open items below.
- **Take:** `~/devel/rasterize-fixtures/photoshop/smart-sharpen-65/take1` (`measured.json`, re-judged into
  `judged.json`), Photoshop 26.10.0. Runner `scripts/photoshop-probe/smart_sharpen.py`, job `smart_sharpen` in
  `scripts/photoshop-probe/photoshop.jsx` (schema and rule summary in `scripts/photoshop-probe/README.md`).

> **Removed 2026-10-10:** smart objects, live filters and non-RAW retained sources (placed images, host
> media, Live Photo frames) are gone for v1; camera RAW layers stay. Where this record describes them, that
> code no longer exists: [removal ADR](convert-to-smart-object-status.md).

## Decisions in force

- The Photoshop 26.10.0 take's verdicts are judged under the decision rule fixed in the runner's docstring before
  the take and never loosened after it.
- Phases: 1 the measurement; 2 the core (8/16 bits: a kernel module, the destructive op through
  `rz_image_filter_op`, the live operator with its complete control set, tests against an independent
  oracle); 3 the destructive dialog, Repeat Last Filter, recording, `apply_filter`; 4 the live smart-filter UI and
  `set_live_filter`; 5 32-bit. Each later phase is one implementer pass and one commit, its contract set here.
- Presets are Rasterize's own library; reading Photoshop's Smart Sharpen preset files is listed as not built, for
  the user, in the phase that ships presets.
- Smart Sharpen at 32 bits (measured below) is phase 5, which waits for float spatial and live filters. That
  is a wait, not a cut.
- Smart Sharpen uses the radius-to-sigma mapping measured against Photoshop; Unsharp Mask follows when that
  mapping lands.
- A NONE, adaptive or smoothing verdict is a **decision**: it goes back to the orchestrator with its tables (below)
  and phase 1 files no issue for it.
- Smart Sharpen as an export sharpening method stays parked.

## Headline verdicts

Every row: `python3 scripts/photoshop-probe/smart_sharpen.py --judge ~/devel/rasterize-fixtures/photoshop/smart-sharpen-65/take1`
(the take itself: `python3 scripts/photoshop-probe/smart_sharpen.py --output <fresh dir> --photos <dir holding fur.png>`).
Units are 16-bit readback values (code/65535); T = 3/32768.

| Statement | Verdict | Branch |
|---|---|---|
| Validity (V1 every sent key reads back, V2 `none` readbacks within T, V3 depths) | valid; the filter lives on `smartObject.filterFX` | — |
| Remembered settings put back (`restore` = `discover`, all keys) | restored true | — |
| Gaussian, non-legacy | LIN fails (shift test up to 6x TOL_LIN), amount law fails at R0.5–2 | decision |
| Lens Blur, non-legacy | LIN fails (shift up to 8.4x), amount law fails at R0.5–2 | decision |
| Motion Blur, non-legacy | LIN fails (shift up to 79x; at R2 A400 also 2δ and −δ, up to 33x); no response at R0.5 | decision |
| Gaussian, legacy | LIN and amount law hold; kernel PSGAUSS (Photoshop's own Gaussian Blur), k = 1.0002, residual 6.2e-5; limiter NONE | decision |
| Lens Blur, legacy | LIN and amount law hold; kernel NONE (no candidate, TABLE fails the x2 resampling) | decision |
| Motion Blur, legacy | amount law holds; LIN fails on the 30° shift test (8.9x) | decision |
| More Accurate (legacy) | Gaussian and Lens NOT MEASURED (every A400 +4δ window clipped); Motion NONE (SAME 0.20 off) | decision |
| Channels | `per_channel` in every mode (a red-only impulse moves G and B ≤ 3.1e-5) | — |
| Reduce Noise | NONE in both modes; it smooths (C noise std ratio 0.274 at A1 RN100) | decision |
| Shadows fade | NONE (best RAMP encoded 0.080, k 1.62 / 1.35 / 1.08 over widths 25/50/100) | decision |
| Highlights fade | QUAD in encoded L, k 1.335, residual 0.040 | (with the shadows: decision) |
| Fade law (fade 50 = 1 − 0.5 w) | fails (0.47 shadows, 0.47 highlights) | decision |
| S/H radius on D | not judged (no standing shadow model) | decision |
| Smart = destructive | same (0 difference) in all four pairs | — |
| 8 bits | not the 8-bit quantization of 16 bits: 2 (Gaussian, Motion) and 3 (Lens) codes | — |
| 32 bits | available, destructive and smart, Gaussian and Lens; range EXTEND (cell at 4.0 stays 4.0); domain not judged (no non-legacy model) | phase 5 |
| Alpha | FILTERED (alpha moves by up to 0.011 Gaussian, 0.050 Lens) | — |
| Out-of-range values | none refused: amount 0/0.5/501, radius 0.05/64.5, RN 101, angle ±181, fade 101, width 0/101 and S/H radius 0/101 all read back exactly as sent | — |
| Unsharp Mask equality | not measured: the take's F6 `usm` rows called `applyUnsharpMask` (the DOM method is `applyUnSharpMask`, fixed since) | — |

## Phase 2–5 contracts

- **Phase 2 (core):** not set. No mode reaches **build**: the only standing kernel (legacy Gaussian, PSGAUSS) has no
  standing limiter, and every other mode, Reduce Noise and the fades are decisions. The orchestrator's rulings on the
  open items below set the contract.
- **Phase 3, phase 4:** follow phase 2's contract.
- **Phase 5 (32-bit):** the take measured Photoshop's Smart Sharpen at 32 bits, as a smart filter and destructively,
  keeping values above 1.0 (EXTEND). It waits for float spatial and live filters.

## Open items (decisions with the orchestrator)

1. **Non-legacy modes are not linear.** On A16 (impulses on 0.5), the (33,33) impulse's response differs from the
   (32,32) one by more than TOL_LIN, so the filter depends on the impulse's position. LIN ratio (error / TOL_LIN) of
   the shift test:

   | Case | Gaussian | Lens | Motion |
   |---|---|---|---|
   | R0.5 A100 / A400 | 1.03 / 5.95 | 2.84 / 7.54 | 0 / 0 (no response) |
   | R1 A100 / A200 / A400 | 1.37 / 2.43 / 4.14 | 3.18 / 5.26 / 8.44 | 1.56 / 3.29 / 9.68 |
   | R2 A100 / A400 | 0.09 / 0.78 | 0.26 / 1.61 | 21.1 / 78.6 |
   | R4 A100 / A400 | 0.08 / 0.07 | 0.08 / 0.69 | 2.16 / 7.70 |

   r(+2δ) = 2r(+δ) and r(−δ) = −r(+δ) hold (≤ 0.21) except Motion R2 A400 at 0° and 90° (32.6 and 22.8, centre
   response −0.011). The amount law (A400 = 4·A100) fails at R0.5/1/2 for Gaussian (22.9/6.6/1.7) and Lens
   (11.7/4.8/1.8) and holds at R4; Motion fails at R1/2/4 (53/194/5.7). A200 = 2·A100 at R1 fails too (2.1, 1.6,
   12.2).
2. **Legacy Gaussian's limiter is NONE, and the cause is the PSGAUSS basis.** The kernel is Photoshop's own Gaussian
   Blur at k = 1.0002 (residual 6.2e-5 against TOL_FIT 3.7e-4…7.7e-4 at every R). On the B edges the linear
   prediction misses (error / TOL_FIT): R1 A100 bands 0/1 0.0055/0.0069 (tol 0.00063/0.0025), R1 A200
   0.0109/0.0139 (0.0013/0.0050), R2 A100 0.0178/0.0227 (0.0008/0.0033); every CLAMP candidate leaves the same
   residual. B_R is read off a 16-bit +4δ impulse whose rounding loses 0.4 / 1.0 / 3.2 / 10.4 % of the blur's mass at
   R 0.5 / 1 / 2 / 4, so the operator applied to a field is not zero-sum and shifts every flat. Recorded, not
   judged: putting the lost mass back at the centre leaves R1 within TOL_FIT and R2 at 0.0015/0.0059 (fails);
   dividing B_R by its sum leaves every profile within TOL_FIT (worst 0.0018 against 0.0033). Bands 2 and 3
   (0.1|0.9, 0.9|0.1) clip in all three cases.
3. **Legacy Lens Blur has no kernel.** Best residuals (TOL_FIT ≈ 3.7e-4…9.0e-4): GAUSS c 1.37 0.029, PSGAUSS 0.017,
   DISC c 1.89 0.034. TABLE: isotropic (≤ 1.1e-3) but the x2 resampling misses by 0.031/0.018/0.0063 against peaks
   0.141/0.052/0.016 (22–40 %).
4. **Legacy Motion Blur is not shift-invariant at 30°** (shift test 8.9x; 0°, 45° and 90° pass), so no kernel is fitted.
5. **More Accurate.** Gaussian and Lens: every A400 +4δ window (R1, R2) clips, so SAME / TWICE / OWN have no window;
   LIN holds on their unclipped impulses. Motion: SAME misses by 0.20 (TOL_FIT 0.004), TWICE has no legacy model,
   OWN fits nothing.
6. **Reduce Noise is a smoother, not a gain.** Centre gain g = r0(RN)/r0(0) on the A ladders goes negative (the
   impulse ends below its source), and the C noise std falls to 0.274 of the input at A1 RN100:

   | RN | g at a = 1, 2, 4, 8, 16, 32 /255 (Gaussian; Lens within 0.01) |
   |---|---|
   | 10 | −0.116, −0.111, −0.098, −0.049, 0.147, 0.720 |
   | 25 | −0.206, −0.206, −0.201, −0.184, −0.118, 0.154 |
   | 50 | −0.265, −0.262, −0.261, −0.253, −0.224, −0.104 |
   | 100 | −0.312, −0.307, −0.306, −0.302, −0.290, −0.237 |

   No HARD / SOFT / RAMP stands (best 0.31 / 0.30).
7. **Shadows fade has no closed form; the fade amount is not linear.** w(L) per width (Gaussian R1 A200, fade 100,
   + impulses):

   | L | 0.02 | 0.10 | 0.20 | 0.30 | 0.40 | 0.50 | 0.60 | 0.70 | 0.80 | 0.90 |
   |---|---|---|---|---|---|---|---|---|---|---|
   | width 25 | 0.917 | 0.710 | 0.428 | 0.212 | 0.089 | 0.031 | 0.012 | 0.004 | 0 | 0 |
   | width 50 | 0.948 | 0.842 | 0.686 | 0.522 | 0.369 | 0.232 | 0.133 | 0.067 | 0.023 | 0.004 |
   | width 100 | 0.977 | 0.929 | 0.859 | 0.775 | 0.687 | 0.585 | 0.478 | 0.368 | 0.251 | 0.124 |

   Candidates (residual; k per width 25/50/100): RAMP encoded 0.080 (1.62/1.35/1.08), SMOOTH encoded 0.124, QUAD
   encoded 0.124, QUAD linear 0.131, RAMP linear 0.185, SMOOTH linear 0.241. Highlights mirror it closely and QUAD encoded stands (k 1.35/1.35/1.31).
   Fade 50 is not 1 − 0.5w (off by up to 0.47); Lens and legacy Gaussian fade the same as Gaussian (≤ 0.002).
8. **Unsharp Mask equality is unmeasured** (the `usm` DOM-name bug above); a take of the four `usm` cases settles it.

## Shipped runs

None yet.
