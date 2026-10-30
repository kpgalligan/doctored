# blend_if_multichannel — the multichannel Blend If take (phase 2a's runner, run by phase 2b-1)

Authored by Adobe Photoshop 2025 26.10.0 on 2026-10-16, run
`rz-558-multi-20261016T230108Z-193cd682`, through
`scripts/photoshop-probe/blend_if_multichannel_take.py --take` (its docstring fixes the
scenes, the rules, the candidates and the bounds; runner sha256 `d4f5eaf8…` at the take).
The full take — every TIFF, every PSD, `measured.json` and `judged.json` — is kept outside
the repo in `~/devel/doctored-fixtures/photoshop/blend-if-multichannel-558/take1`.

Copied here by `--export-fixture` (the two PSDs are under 64 KB; `rows.json`, the
runner's own readings, is about 140 KB):

- `c3_rgb8.psd` — Photoshop's own save of row c3: the 96 × 64 stepped source over black
  with ONE `blendRange` list, This Gray [0, 0, 96, 192] then This Red [0, 0, 64, 224].
  Photoshop writes every effect, disabled, in the record's `lfx2` beside the ranges.
- `u3_rgb8.psd` — row u3: a gray-128 layer over the source with Underlying Gray and Red.
- `rows.json` — one reading per 4 × 4 cell of every judged COMBINE row (c0–c3 and u0–u3
  at RGB8 and RGB16, p0–p3 at CMYK8), the bound, the winner and the judged summary.

Every CMYK file of the take (p3 and the K rows k0, kadj, k1, k2) embeds the 557 KB SWOP
profile, and `c3_rgb16.psd` is over 64 KB. They stay in the take folder:
`cmyk_adjustment_styles.rs` and `precision_blend_if_multichannel.rs` read them from
`take1/jobs/` and skip when it is absent, as `cmyk_psd_styles.rs` does.

Verdict MISSES (clean take), from INSPECT k1 alone, a build-input error recorded in the
runner's docstring. PRODUCT met every COMBINE row and stays shipped (MIN 9.6–39 codes). On
CMYK adjustments, the K rows confirm POST and T-ADJUSTED at 0 codes. On a Pass Through
group, the P rows show a style, a clip and Blend If all isolate (0 codes).
