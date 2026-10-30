# adjustment_styles — phase 1's Photoshop take

Authored by Adobe Photoshop 2025 26.10.0 on 2026-10-16, run
`rz-558-scope-20261016T184202Z-d6aa5517`, through
`scripts/photoshop-probe/styles_scope_558_take.py --take` (its docstring fixes the
scenes, the rules and the bounds; runner sha256 `c59d8c73…` at the take). The full
take — every TIFF, every PSD, `measured.json` and `judged.json` — is kept outside the
repo in `~/devel/doctored-fixtures/photoshop/styles-scope-558/take1`.

Copied here by `--export-fixture`, every file under 64 KB:

- `<row>_rgb8.png` / `<row>_rgb16.png` — Photoshop's flattened render of each A row
  (`base`, `adj`, `a1`, `a2`, `a3`, `a4t`, `a4u`, `a6`, `a7`, `a7b`) at 8 and 16 bits,
  written as 8-bit straight RGBA (the 16-bit row rounded to 8-bit codes). The scene is
  the runner's 192 × 128 stepped gray ramp with a transparent hole (x 24..71, y 48..79)
  and a 50 % band (x 72..119), Invert at 8/16 bits.
- `a3_rgb8.psd` — Photoshop's own save of row a3: an unmasked Invert with Fill 50 % and
  a 4 px red outside Stroke. `a6_rgb8.psd` — row a6: an unmasked Invert with a Drop
  Shadow (distance 8, angle 180, size 0). Every other PSD of the take is between 64 and
  72 KB and stays in the take folder (`rows.json` lists them).
- `rows.json` — the verdict, the winners and the per-rule summary.

Verdict MISSES (clean take). Shipped and confirmed: FILL-SCALES (A3), T-ADJUSTED (A4),
C-SHAPE (A7) at 8, 16 and 32 bits; INSPECT matched. Missed: A1 — Photoshop's shape is
the mask over the canvas (S-MASK), not the backdrop's alpha; A2 — the adjustment reads
its own Below effects (POST); A5 TWIN and A6 follow from those two. Rasterize ships
S-MASK and POST, fitted from this take (controller ruling, fix pass 2), with the
adjustment applied in place and alpha kept, as row a1's band shows; `psd_adjustment_styles.rs`
rebuilds rows a1, a2, a6 and a7 over `base` and matches them on every pixel.
