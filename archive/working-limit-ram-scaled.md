# The working limit scales with RAM (ADR)

- **Status:** Accepted
- **Date:** 2026-10-04
- **Supersedes:** the fixed limit in [phase-4-jobs-resource-status](phase-4-jobs-resource-status.md)

## Context
The working limit was `min(RAM/3, 2 GiB)` in the app and a hard 2 GiB ceiling in the core (`admit`
refused any larger limit). A 32 GB Mac refused the same 45 MP work an 8 GB Mac refused.

## Decisions
- **Curve:** `automatic(ram) = min(ram/2, max(ram/3, ram − 8 GiB))`, saturating, floored to whole MiB
  (`app/Sources/WorkingLimit.swift`). 8 GB → 2.67 GB, 12 → 4, 16 → 8, 24 → 12, 32 → 16, 64 → 32.
  *Why:* at least a third of RAM, never more than half, and from 16 GB up never less than 8 GiB left for
  macOS, display projection, Core Image's uncounted transients and other apps (below 16 GB the third wins,
  so an 8 GB Mac keeps ~5.3 GiB). A RAW develop now leaves at most 26 MB resident above the core's retained
  bytes once Core Image's release settles
  (measured; it was up to 0.5–0.7 GB with the shared `CIContext`; the release is asynchronous, up to 0.78 GB for
  under a second).
- **Precedence:** `RZ_WORKING_LIMIT_BYTES` (decimal, > 0, unclamped; an invalid value is ignored with one
  `[WorkingLimit]` stderr line), then the `WorkingLimitBytes` preference (Rasterize ▸ Working Memory…, argv
  `-WorkingLimitBytes` works; clamped to 2 GiB…¾ RAM), then the automatic curve.
- **Relaunch:** the limit is fixed for the process lifetime. History and snapshot budgets (a quarter of the
  limit) and job controls capture it at creation, so a preference change applies at the next launch.
- **Core:** `admit` refuses only a zero limit. `precision_image::process_working_limit()` (initially
  `REFERENCE_WORKING_BYTES`, 2 GiB) is read wherever no caller limit is passed: internal constructors,
  `*_plan()` defaults, the native reopen check and the PSD layered-import envelope. The host sets it once
  at launch through `rz_set_process_working_limit`.
- **MCP is read-only:** `resource_usage` reports `working_limit` (effective, source, automatic default,
  preference, physical memory) from the same report Help ▸ Memory Usage shows. There is no setter tool: an
  app-wide preference with no undo, under app/CLAUDE.md's view-preference exemption.
- **Tests pin 2 GiB:** owned test instances and every `scripts/` launcher set
  `RZ_WORKING_LIMIT_BYTES=2147483648`, so refusals stay deterministic across machines. Core tests use
  `REFERENCE_WORKING_BYTES`.
- **Estimates are unchanged:** plans keep their ≥ 1.25× measured-peak margins; only the comparison limit
  moved. Recorded byte figures were measured against the 2 GiB reference limit.
- **2026-10-09: the assumed minimum machine is a 16 GB Mac** (automatic limit 8 GiB); work is not designed or
  measured to fit smaller limits; the 2 GiB pin remains only as the deterministic test reference and the
  preference minimum. README states the minimum and frames its figures as what a job needs.

## Kept fixed caps
- `MAX_NATIVE_BYTES` (the typed native file-length cap) stays 2 GiB as its own literal: it is a format cap
  whose lengths and offsets were audited only to that size.
- The PSD 2 GiB file cap, `IDAT_CHUNK_MAX_BYTES` and the other format caps; `CONTIGUOUS_CACHE_BYTES`
  (1 GiB), `cacheLimit` (128 MiB) and the work-unit caps.

## Re-verified at 8 GiB (batch 163)
Owned background instances at bff4261, `RZ_WORKING_LIMIT_BYTES` 8,589,934,592 (what a 16 GB Mac gets) and
2,147,483,648 (the reference: test pin and Working Memory minimum), `MallocLargeCache=0`; E = interval
`phys_footprint` peak − pre around the one MCP call, 3 fresh-instance runs; P is the admission's own plan beside the
pre state. An 8 GB Mac gets 2,862,612,480 B automatically. Rows marked *under review* passed admission at 8 GiB with a
plan below 1.25× its measured peak; they are not documented as admitted until the plan is fixed. Batch 164 resolved
them in place (its section below); the figures it replaced are batch 163's HEAD readings.

| case | 2 GiB outcome | 8 GiB outcome | P | max E | P/E | remains |
|---|---|---|---|---|---|---|
| 96 MP 8-bit HEIC / gain-map HEIC / AVIF open | refused by the whole-open charge (batch 164): "This job needs 3.91 GB of working memory on its own…" (gain map 3.9 GB, AVIF 3.77 GB); 96 MP JPEG and PNG refused by the flat-open charge since batch 165 ("3.16 GB" / "3.61 GB … on its own"), 48 MP JPEG and PNG open | admitted (batch 164) | 4,196,342,077 HEIC; 4,190,291,083 gain map; 4,044,034,729 AVIF (`openScratch`) | 3,130,968,656; 3,128,789,680; 3,081,456,136 | 1.340; 1.339; 1.312 | over an 8 GB Mac; the 20 B/px first projection is the peak (batch 163's plan was 0.754×) |
| second Rotate Canvas 17° of a 24 MP 16-bit two-layer document | refused: "needs more than the 2 GB working limit to save and reopen" (reopen plan 2,663,683,195) | turn admitted | 2,810,377,520 | 1,781,793,968 | 1.577 | over an 8 GB Mac (turn plan 3,361,469,033) |
| that document's `.rz` save | not reached | admitted (1,061,842,238 B; `MAX_NATIVE_BYTES` does not bind) | 1,333,945,560 | 1,019,888,048 | 1.308 | none: the file is exactly the app document's own `encoded_bound` (1,061,842,238); batch 163's 1,061,842,216 was the probe twin's, whose layer name is 2 B longer and source id 24 B shorter (batch 164) |
| that document's reopen in a fresh instance | not reached | admitted (batch 165) | 3,794,975,709 (R 1,790,050,327 + the margined display 2,004,925,382; the reader's A 2,663,682,924 is below) | 2,521,712,392 | 1.505 | none: the core reopen is covered (1.251× its in-process 2,129,004,032) and the display after it now carries `ProjectionMargin` (batch 165) |
| 16-bit live Reduce Noise over an opened 24 MP photo | refused, 674,167,928 + 1,473,377,920 = 62,200 B over | admitted | 1,473,377,920 | 935,445,176 | 1.575 | nothing from 8 GB up (2,147,545,848 fits 2,862,612,480) |
| 45 MP 8-bit Box Blur r5 | refused (batch 164: "needs more working memory than the current budget, and no other document is open to close") | admitted (batch 164) | 2,065,143,936 (plan 1,883,379,840 + the 181,764,096 layer copy) | 1,633,240,216 | 1.264 | none from 8 GB up (2,610,458,251 fits 2,862,612,480); HEAD's plan was 1.158× |
| M1 (lane-b, m8 = 7) 45 MP 8-bit Dust & Scratches r2 | refused by memory, not work (Debug drive) | admitted, no work cap under a job | 2,113,014,146 | 1,643,070,664 | 1.286 | none |
| M2 45 MP 8-bit Surface Blur r5 T15 | not driven | admitted | 2,113,014,146 | 1,647,903,944 | 1.282 | none |
| M3 45 MP 8-bit Radial Blur Draft, Amount 1 | not driven | admitted | 2,294,778,282 | 1,640,154,312 | 1.399 | none |
| M4 24 MP 16-bit Dust & Scratches r4 | not driven | admitted | 1,084,653,991 | 774,275,624 | 1.401 | none |
| M5 24 MP 16-bit Surface Blur r5 T255 | not driven | admitted | 988,653,951 | 779,993,640 | 1.268 | none |
| after recovering a 45 MP 8-bit document with a snapshot: `save_snapshot` | refused (generic sentence, no figure) | admitted | no readout | 676,053,496 | — | 2 GiB refusal does not name the snapshots |
| after that recovery, the next `write_recovery` | refused: "needs 1.22 GB … and 866.8 MB is already in use" | admitted | 1,313,506,481 | 270,090,432 | 4.863 | recovered snapshots share no memory (908,884,269 retained); fits an 8 GB Mac (2,222,391,967) |

### Batch 164
Owned instances at 62930ad plus the batch's changes. The settings were batch 163's: 8 GiB and 2 GiB,
`MallocLargeCache=0`, 3 fresh-instance runs, and an MCP baseline twin (`list_documents`) at E 0 ×3. Pass is
P ≥ 1.25 × max E, and sequential stages are charged max(), not summed.

- **The 96 MP opens.** A -O stage harness (app sources and release core) measured `openHostSource`'s steps at 96 MP. Each
  figure is peak − pre-open footprint, given as heic8 / gain map / avif8:

  | stage | heic8 | gain map | avif8 |
  |---|---|---|---|
  | capture | 1,663,878,392 | 1,665,861,456 | 1,553,646,792 |
  | initial document | 1,995,720,192 | 1,999,324,720 | 1,936,328,168 |
  | adoption | 1,217,316,088 | 1,604,486,704 | 1,168,442,568 |
  | first projection (`render()`) | 3,138,275,920 | 3,141,257,856 | 3,089,418,808 |

  - The projection is the peak. It is the ≈ 1.22 GB rendered document plus `rz_doc_flattened`'s 1,920,959,832
    (20 B/px), so the capture figure alone could never cover the open.
  - The whole open is now charged up front for a single 8-bit still: `HostStillDecoder.openScratch` =
    30 MiB + 41·px + 3·encoded + 64 MiB.
  - The figures were fitted by batch 140's rule to MCP `open_document` at 12, 48 and 96 MP (heic8, gain map,
    avif8) and the 36 MP iMac Blue and Sonoma. The full record is beside the constants.
  - Max E at 48 MP is 1,590,461,760 / 1,593,394,520 / 1,559,610,640. At 36 MP it is 1,186,464,728 (iMac Blue) /
    1,180,156,888 (Sonoma).
  - Every case's P/E is between 1.254 and 1.557. The final 96 MP drive gives 1.340 / 1.339 / 1.312.
- **Open-charge band B.**
  - The 48 MP HEIC opens need 2,180,368,909 and 2,176,287,400 (gain map) beside the 0 retained of a fresh
    instance. Both are over 2 GiB, so at the reference limit they are refused: "This job needs 2.03 GB … on its
    own".
  - Both fit the 2,862,612,480 an 8 GB Mac gets. `scripts/host-still-admission/verify.py` therefore runs A1 and
    A2 at that limit.
  - A 48 MP AVIF (2,103,027,151) and the 36 MP system HEICs (1,661,303,031 / 1,643,535,462) stay within 2 GiB.
  - A 96 MP still needs 3.77–3.91 GB. It opens from a 16 GB Mac up.
  - The core's flat 96 MP JPEG/PNG open stayed unadmitted here (E 2,688,108,440 / 2,688,599,960); batch 165
    admits it (`FlatOpenAdmission`).
- **The second Rotate Canvas.**
  - The saved t2.rz is 1,061,842,238 B. That is exactly the reopened app document's own `encoded_bound`, and an
    in-process resave writes the same length.
  - The probe twin's bound 1,061,842,216 is its own file's exact length. The twin's header differs by the layer
    name ("Photo" against "P16", −2 B) and the source id (the 12-byte `uniform-turn` against a 36-byte UUID,
    written once, +24 B).
  - The reopen, by stage:

    | stage | charge | E | P/E |
    |---|---|---|---|
    | core `open_native` | reader's A 2,663,682,924 (plan H 2,663,683,280) | 2,129,004,032 in process | 1.251 |
    | display estimate that follows | 2,519,902,373 (`retained_bytes` 1,061,774,823 + render scratch 1,455,418,048 + overhead 2,709,502) | — | — |
    | whole app open | — | 2,521,597,704 | 0.9993 against the display estimate |

  - The display stage binds and is not covered 1.25×. It is filed separately, and the reopen row stays under
    review.
- **The 45 MP filters.**
  - Since 2026-10-09 a destructive filter, geometry, transform or placed-source commit run as a job (with a
    control) has no work ceiling and is admitted by memory alone; uncontrolled calls, live-filter stacks and export
    sharpening keep `MAX_DESTRUCTIVE_WORK`; geometry previews (`interactive`) keep `MAX_WORK_UNITS`, and exact filter
    previews run under a control. Rows M1–M5 above: Release at 8 GiB,
    `MallocLargeCache=0`, 3 fresh-instance runs plus a `get_document` twin (E ≤ 98,304), P8 = max(1,931,243,520,
    8n + 1,567,715,328 + ΔR), P16 = 795,586,399 + ΔR; fixtures and receipts in
    `rasterize-fixtures/admission-45mp`.
  - Box Blur now holds one binary64 sum at a time. G1, the core op on 8256×5504 noise, gives E 1,091,158,808
    (24.0 B/px, was 48).
  - The plan's per-pixel term was 33 bytes. That is the smallest integer from 25 for which plan + the n×4 layer
    copy is ≥ 1.25× the app's 45 MP `apply_filter` peak: G2 is 1,633,240,216, giving 2,065,143,936 = 1.264×.
  - That peak is the commit and first projection that every 8-bit filter shares. Invert peaks at
    1,633,698,968 on the same document. Batch 165 charges that stage for every 8-bit edit and puts Box Blur's
    term back to its own 24 B/px (below).
  - G3, at 16 bits: P16 Box Blur r10 E 764,133,928 against the selected-edit plan 1,041,492,608 (1.363×;
    825,492,608, 1.080×, since batch 165).

### Batch 165
Owned instances at 6bb2d8e plus the batch's changes: 8 GiB (port 4997), 2 GiB (4998) and 2,862,612,480 (4996),
`MallocLargeCache=0`, 3 fresh-instance runs for the fits and 1 for the final drives, an MCP baseline twin at E 0 ×3,
0 `[AutomationAlerts]` lines. Pass is P ≥ ⌈1.25 × max E⌉, P being the largest per-gate charge plus the retained
growth at that gate (R = `unique_retained − cache`).

- **The display projection.** `projectionScratchEstimate()` returns its structural estimate plus
  `ProjectionMargin`'s m/16, so every caller takes it. Every 8-bit publication that is
  not a carried projection now admits `ByteFilterAdmission.commitStageBytes(updated)` = 8n (the result and the
  written layer) + the margined projection, beside the updated document's own bytes; the byte filters' up-front
  gate charges max(plan, the same `commitStageBytes`). Fits (the records are beside the constants):

  | row | structural | ΔR | max E | P | P/E |
  |---|---|---|---|---|---|
  | C1 photo45 (45 MP) Invert | 1,090,584,576 | 181,770,626 | 1,631,274,160 | 2,044,852,610 | 1.254 |
  | C2 photo45 Box Blur r5 | 1,090,584,576 | 181,770,626 | 1,633,174,680 | 2,044,852,610 | 1.252 |
  | C3 png8-48 Invert | 1,170,505,728 | 195,091,154 | 1,752,892,664 | 2,194,705,106 | 1.252 |
  | PB-8 D24 / D26 | 576,000,000 / 623,001,600 | — | 480,248,144 / 519,438,720 | 792,000,000 / 856,627,200 | 1.649 |
  | H1 Rotate Canvas reopen (16-bit) | 1,458,127,550 | 1,790,050,327 | 2,521,712,392 | 3,794,975,709 | 1.505 |
  | H2 P16 Invert (16-bit) | 578,608,290 | 193,067,552 | 765,395,496 | 988,653,951 | 1.292 |
  | PB-16 D24 / D26 | 578,608,290 / 625,698,376 | — | 576,307,608 / 623,313,352 | 795,586,399 / 860,335,267 | 1.380 |

  m8 = 6 and m16 = 6, each the smallest from 4 that passes. Before this batch, C1–C3 were charged by nothing
  (C2 by Box Blur's 33 B/px, 1.264×), and the 16-bit display was 1.004× (PB-16) to 1.008× (H2). The spec's first
  publication arm, the margined projection without the 8n, needed m8 = 12 (C1 1.114× and C3 1.113× at m8 = 8),
  so the orchestrator ruled the 8n in. Since 2026-10-09 m8 = 7 (maintainer): C1 P 2,113,014,146 (1.295×),
  C3 2,267,861,714 (1.294×), PB-8 1.724×, and the 8-bit filter commit is 54.5 B/px (≈ 157.6 MP at 8 GiB).
- **Box Blur** goes back to its own 24 B/px stage (`BOX_BLUR_BYTES_PER_PIXEL`): plan 728,644,320 at 24 MP
  (1.264× G1) and 1,372,168,320 at 45 MP (1.258× G1); G3 at 16 bits 825,492,608 (1.080×).
- **The flat JPEG/PNG open** (`FlatOpenAdmission`, admitted before `RasterDocument.open(url:)`): F = 0, a = 34,
  so openScratch = 34·px + 3·encoded, fitted by batch 140's rule. jpeg8-12 / -48 / -96 1.262 / 1.260 / 1.260,
  png8 1.443 / 1.440 / 1.440 (E 28.0 B/px from 48 to 96 MP). **Band A:** the 48 MP charges, 1,721,414,325 (JPEG)
  and 1,967,568,597 (PNG), fit 2 GiB, so verify.py's A7 stays at the reference limit. The new A7-plan gate is
  refused at 1 GiB ("This job needs 1.6 GB … on its own"). Under rule V, A8 (a 48 MP JPEG placed into a 48 MP
  document) moved to 2,862,612,480: its margined display preparation is refused at 2 GiB.
- **Final drives.** At 8 GiB, C1 / C2 / H1 / H2 / jpeg8-96 / png8-96 are admitted, with max E 1,632,470,168 /
  1,632,371,864 / 2,521,696,008 / 764,183,056 / 2,688,714,648 / 2,689,550,232 (P/E 1.253 / 1.253 / 1.505 / 1.294
  / 1.260 / 1.440). At 2 GiB, the 45 MP Invert and Box Blur commits are refused ("needs more working memory than
  the current budget, and no other document is open to close"), the 48 MP PNG Invert is refused ("This job needs
  2.04 GB … on its own"), the 16-bit P16 Invert and the 48 MP JPEG and PNG opens are admitted, the 96 MP JPEG and
  PNG opens are refused ("3.16 GB" / "3.61 GB … on its own"), and the 45 MP place (R449) is admitted. At
  2,862,612,480, the 45 MP Invert and the 48 MP opens are admitted, and the 96 MP opens are refused.
- **Ceilings**, from the fitted charges and the measured retained bytes per px:

  | case | per px | 2 GiB | 2,862,612,480 | 8 GiB |
  |---|---|---|---|---|
  | single-layer 8-bit opened photo, filter commit (12 B/px retained + 8 + 24·22/16) | 53.0 | ≈ 40.5 MP | ≈ 54.0 MP | ≈ 162 MP |
  | single-layer 16-bit document, display after an edit (28.1 retained + 24.1·22/16) | 61.2 | ≈ 35.1 MP | ≈ 46.7 MP | ≈ 140 MP |
  | the same, display after an open (20.0 retained) | 53.2 | ≈ 40.4 MP | ≈ 53.8 MP | ≈ 161 MP |
  | JPEG open (34 + 3 × 0.43 B/px encoded) | 35.3 | ≈ 60.8 MP | ≈ 81.1 MP | ≈ 243 MP |
  | PNG open (34 + 3 × 2.11 B/px encoded) | 40.3 | ≈ 53.2 MP | ≈ 71.0 MP | ≈ 213 MP |

  The encoded rate is the fixtures'. A real photo's rate moves the open ceilings a little.
- **Remains**:
  - First projections no admission charges: undo/redo; 8-bit opens other than a ≤ 8-bit JPEG/PNG (TIFF, WebP,
    GIF, BMP, PSD, RZDC, a 16-bit PNG opened at 8 bits); the ImageIO fallback decode. Resolved in batch 166.
  - Lowering the projection peak itself: `rz_doc_flattened`'s 20 B/px, and the open's initial-document copy.

### Batch 166
Owned instances at 5eac7f3 plus the batch's changes: 8 GiB (port 4997), 2 GiB (4998) and 2,862,612,480 (4996),
`MallocLargeCache=0`, 3 fresh-instance runs for the fits and 1 for the final drives, an MCP baseline twin at E 0 ×3,
0 `[AutomationAlerts]` lines. Pass is P ≥ ⌈1.25 × max E⌉.

- **Maintainer decision (2026-10-04): undo, redo and History navigation never refuse**.
  `HistoryUndoManager.undo()`/`redo()` estimate the restored state's first projection
  (`ImageDocument.historyStepOverage`, backed by the non-throwing `RasterResources.overLimitBytes`) and then
  step. An over-limit step proceeds and leaves a `HistoryMemoryWarning`, keyed to the entry it landed on: the status
  bar's "Over working limit" segment, with the sentence as its tooltip, and `memory_warning` in MCP
  `undo`/`redo`/`restore_history`. `navigateHistory` estimates every step live and publishes one "Going to …"
  warning for its largest overage. Opens still refuse.
- **The remaining 8-bit opens** (TIFF, WebP, GIF, BMP, PSD, `.rz`, a 16-bit PNG at `rgba8`, the ImageIO fallback)
  set `Opened.projectionUncharged`, and `read(from:ofType:)` admits `FlatOpenAdmission.firstProjectionBytes`
  beside the document's own bytes after the decode and before anything is installed:
  `commitStageBytes(n)` (8n + the margined projection) + 4n when `read` runs the working-space adoption. The gate
  cannot tell whether the core will convert, so every flat open carries the 4n and a `.rz` or PSD never does.
  - Fit (P = charge + ΔR, ΔR = R_post − R_pre − 4n): tiff48 1.751, tiff96 1.749, webp48 1.755, gif48 1.751,
    bmp48 1.751, png16 at 8 1.751, PSD layers 1.360, PSD composite 1.608, `.rz` 1.749, planar CMYK TIFF via
    the fallback 1.746, Display P3 TIFF converted on open 1.361.
  - The spec's candidates failed: the margined projection alone (+ 4n adopting) gave PSD layers 1.138 and the
    converting open 1.139. `commitStageBytes` alone gave the converting open 1.2496, because a conversion costs
    ≈ 8n over the same open without one (E 1,756,300,512 vs 1,365,033,944). The orchestrator ruled
    `commitStageBytes` + 4n.
- **The history charge** is `projectionScratchEstimate()` at both depths: the first candidate at 8 bits, and
  `PreparedDocumentEdit`'s own at 16 bits. Fit (P = the step's charge; navigation adds max(0, ΔR)):
  - 8-bit: U1 45 MP invert undo 1.731, U3 48 MP invert undo 1.655, U4 45 MP place redo 1.762, U5 live-blur undo
    1.750, U6 its redo 1.657, N8 two-step `restore_history` 1.396.
  - 16-bit: U7 P16 invert undo 1.412, N16 1.400.
  - N8 and N16 logged both steps at the full-size charge, so every navigation step is estimated.
- **Final drives.**
  - **8 GiB:** the 96 MP TIFF and the 45 MP `.rz` open. R449's undo/redo and the live-blur (L45) undo/redo
    return no `memory_warning`.
  - **2 GiB:**
    - The 48 MP TIFF, 16-bit PNG at 8 bits, planar CMYK fallback and P3 TIFF opens are refused ("This job
      needs 2.23 GB of working memory on its own…").
    - The 45 MP layered PSD and `.rz` are refused (2.07 GB).
    - R449's undo/redo return no warning.
    - L45's live Gaussian Blur commit itself is refused ("…no other document is open to close"). This batch's
      code does not reach `set_live_filter`, so it is most likely batch 165's 8-bit publication charge (not
      confirmed against a pre-165 build). So L45's history
      could not be driven there.
    - UR (48 MP PNG → Image Size 2016×1512 → Invert, then six 12 MP copies): no Invert undo warned, and
      retained reached 817 MB. `restore_history` to Initial State then proceeded to 8064×6048 with "Going to
      “Initial State” in History went over the working limit by 277.7 MB. …", and the following redo
      returned no warning.
  - **2,862,612,480:** every open above is admitted, including the `.rz` and PSD. R449 and L45 (the blur
    admitted) undo/redo return no warning.
- **Ceilings**, from the fitted charges and the measured retained bytes per px (the projection is 33 B/px,
  24·22/16):

  | case | per px | 2 GiB | 2,862,612,480 | 8 GiB |
  |---|---|---|---|---|
  | single-layer 8-bit TIFF open (8 + 33 + 4 charged, 4 retained) | 49 | ≈ 43.8 MP | ≈ 58.4 MP | ≈ 175 MP |
  | single-layer 8-bit document, undo of one edit without a warning (12 retained + 33) | 45 | ≈ 47.7 MP | ≈ 63.6 MP | ≈ 191 MP |

- **Remains**:
  - The 20 B/px `rz_doc_flattened` peak and the open's initial-document copy.
  - The decode stages of these opens, still not planned up front: the gate runs after the decode.
  - Snapshot restore (`restoreEditingState`) and recovery install (`installEditingState`), which still render
    unestimated.

### Batch 168
Batch 167 measured noise-content 45 MP commits at 1.247–1.249× of the 2,044,852,610 commit-stage charge. This
batch refits over filters × content under batch 165's
harness: the Release app at 60ebea3 at 8 GiB (port 4997), `MallocLargeCache=0`, a fresh instance per run, `open_document`
then the one measured `apply_filter`, 3 runs per row, an MCP baseline twin at E 0 ×3, and 0 `[AutomationAlerts]`
lines. Pass is P ≥ ⌈1.25 × max E⌉, where P = max(P_up, P_pub). P_pub = the margined projection of the updated
document + 8n + ΔR. P_up = max(plan, the margined projection + 8n), for the byte filters only, with the plan from
`rz_image_filter_op_scratch_bytes` at 8256×5504.

Content is photo45 (`photo45.png`, high-entropy) and noise (`p8-45.png`). Flat (8256×5504, every pixel 128 gray)
runs Invert only. Every row has n = 45,441,024, ΔR = 181,770,626 and P = 2,044,852,610 (P_up 1,863,081,984):

| row | plan | max E photo45 | max E noise | P/E (lower) |
|---|---|---|---|---|
| Box Blur r5 | 1,372,168,320 | 1,633,305,752 | 1,633,256,600 | 1.252 |
| Unsharp Mask (defaults) | 1,141,268,480 | 1,632,584,880 | 1,632,371,936 | 1.253 |
| Add Grain (defaults) | 1,371,095,110 | 1,633,191,064 | 1,633,207,448 | 1.252 |
| Dust & Scratches r1 | 1,141,268,480 | 1,634,911,456 | 1,634,796,768 | 1.251 |
| Reduce Noise (defaults) | 1,141,268,480 | 1,613,874,400 | 1,613,808,864 | 1.267 |
| Clouds seed 5 | 1,141,268,480 | 1,633,453,232 | 1,633,404,104 | 1.252 |
| Difference Clouds seed 5 | 1,141,268,480 | 1,633,453,256 | 1,633,486,024 | 1.252 |
| Offset 1234 / −567 | 1,141,268,480 | 1,633,453,232 | 1,633,240,240 | 1.252 |
| Gaussian σ4 | — | 1,632,158,896 | 1,631,896,752 | 1.253 |
| Add Noise 0.2 | — | 1,632,519,344 | 1,632,502,960 | 1.253 |
| Pixelate 8 | — | 1,635,026,072 | 1,635,026,072 | 1.251 |
| Invert | — | 1,635,042,456 | 1,635,075,272 | 1.251 |
| Levels 0.05 / 0.95 / 1.2 | — | 1,632,519,344 | 1,632,568,496 | 1.253 |

Flat Invert peaks at 1,634,993,328 (1.251). The work limit refuses Surface Blur T15 at every r from 5 to 1, Dust & Scratches r2 and Radial Blur Draft Amount 1
at 45 MP on both contents, so they have no row. At 16 bits (6000×4000):

| row | structural | ΔR or R | max E | P | P/E |
|---|---|---|---|---|---|
| H2 P16 Invert | 578,608,290 | 193,067,552 | 765,035,072 | 988,653,951 | 1.292 |
| H2n `p16-24` (noise) Invert | 578,608,290 | 289,067,592 | 765,035,096 | 1,084,653,991 | 1.418 |
| H1n Rotate Canvas build with `p16-24`, reopened | 1,458,127,550 | R 1,790,050,378 | 2,520,581,872 | 3,794,975,760 | 1.506 |

H2n's R_post stayed at 770,153,957 on every run, so its ΔR is 96 MB above H2's. With H2's ΔR it still passes at
1.292. H1n credits no reader charge.

- **Cause.** Content changes nothing. Noise peaks 81,944 B above flat and photo45 49,128 B above flat. At the peak
  sample of `footprint --sample 0.1`, the only categories that differ are MALLOC_LARGE (the pixel buffers) and the
  page table. The 85–97 MB those categories differ by is the 0.1 s sampling landing at another point of the ramp:
  the three sampled runs' interval E are equal to within 16 KB. Batch 167's 3.8–6.6 MB higher noise rows are not
  reproduced. Radial Blur is now refused, so Clouds was rerun with batch 167's extra argv
  (`-RecoveryDiscoverySuppressed YES -ColorWorkingSpace srgb`): 1,633,502,432, against 1,633,404,104 without it.
- **Outcome C.** Every row passes at m8 = 6 (smallest P/E 1.2506, noise Invert) and at m16 = 6, so nothing
  changes: no constant, no `commitStageBytes` term, no ceiling. m8 = 7 would give 1.292× and m8 = 8 1.334×. One
  caution: the three `footprint`-sampled diagnosis runs read about 1.1 MB above the fit runs' maxima
  (1,636,189,360, 1.2498×), so the 8-bit margin above the 1.25 rule is about 0.8 MB, and the sampled runs sit 0.3 MB below it.
