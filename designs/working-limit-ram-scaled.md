# The working limit scales with RAM (ADR)

- **Status:** Accepted — closed
- **Date:** 2026-10-04 (condensed 2026-10-09)
- **Supersedes:** the fixed limit in [phase-4-jobs-resource-status](phase-4-jobs-resource-status.md)
- **Record:** [../archive/working-limit-ram-scaled.md](../archive/working-limit-ram-scaled.md) is the full record of batches 163–168 and of the cancellable jobs' work-ceiling removal.

> **Removed 2026-10-10:** smart objects, live filters and non-RAW retained sources (placed images, host
> media, Live Photo frames) are gone for v1; camera RAW layers stay. Where this record describes them, that
> code no longer exists: [removal ADR](convert-to-smart-object-status.md).

## Context
The working limit was `min(RAM/3, 2 GiB)` in the app and a hard 2 GiB ceiling in the core (`admit`
refused any larger limit). A 32 GB Mac refused the same 45 MP work an 8 GB Mac refused.

## Decisions
- **Curve:** `automatic(ram) = min(ram/2, max(ram/3, ram − 8 GiB))`, saturating, floored to whole MiB
  (`app/Sources/WorkingLimit.swift`). 8 GB → 2.67 GB, 12 → 4, 16 → 8, 24 → 12, 32 → 16, 64 → 32.
  *Why:* at least a third of RAM, never more than half, and from 16 GB up never less than 8 GiB left for
  macOS, display projection, Core Image's uncounted transients and other apps. A RAW develop leaves at most
  26 MB resident above the core's retained bytes once Core Image's release settles.
- **Precedence:** `DR_WORKING_LIMIT_BYTES` (decimal, > 0, unclamped; an invalid value is ignored with one
  `[WorkingLimit]` stderr line), then the `WorkingLimitBytes` preference (Doctored ▸ Working Memory…, argv
  `-WorkingLimitBytes` works; clamped to 2 GiB…¾ RAM), then the automatic curve.
- **Relaunch:** the limit is fixed for the process lifetime. History and snapshot budgets (a quarter of the
  limit) and job controls capture it at creation, so a preference change applies at the next launch.
- **Core:** `admit` refuses only a zero limit. `precision_image::process_working_limit()` (initially
  `REFERENCE_WORKING_BYTES`, 2 GiB) is read wherever no caller limit is passed: internal constructors,
  `*_plan()` defaults, the native reopen check and the PSD layered-import envelope. The host sets it once
  at launch through `dr_set_process_working_limit`.
- **MCP is read-only:** `resource_usage` reports `working_limit` (effective, source, automatic default,
  preference, physical memory) from the same report Help ▸ Memory Usage shows. There is no setter tool: an
  app-wide preference with no undo, under app/CLAUDE.md's view-preference exemption.
- **Test pin:** owned test instances and every `scripts/` launcher set `DR_WORKING_LIMIT_BYTES=2147483648`,
  and core tests use `REFERENCE_WORKING_BYTES`, so refusals stay deterministic across machines. 2 GiB is the
  deterministic reference and the preference minimum, never a user target.
- **Estimates are unchanged:** plans keep their ≥ 1.25× measured-peak margins; only the comparison limit
  moved.
- **2026-10-09: the assumed minimum machine is a 16 GB Mac** (automatic limit 8 GiB); work is not designed or
  measured to fit smaller limits. README states the minimum and frames its figures as what a job needs.
- **RAW decode refit:** `RawSourceWork.scratch` = 528 MiB + 82 B/px + 3·file (518 MiB as fitted on real
  24–61 MP camera RAWs, +10 MiB for Develop's Presence sliders), so a 45 MP RAW (4.45 GB) opens at 8 GiB.

## Fits in force
- `HostStillDecoder.openScratch` = 30 MiB + 41·px + 3·encoded + 64 MiB charges a single 8-bit still's whole
  open up front.
- The wide (10/16-bit) host-still charge `HostSourceInput.scratch` = 45 MiB + 42·px + 3·encoded + 64 MiB covers
  the whole open, measured to 100 MP (refit twice, the second time on the open that lands one raster layer:
  10 MiB + 43·px before, 12 MB more at 48 MP; binding row avif10-67): one 16-bit image now holds the document's
  100 MP (a 1 GiB stored-plane record, no decoder cap on a 16-bit plane), where the one-image cap refused above
  67 MP. A 16-bit PNG/TIFF open is the core's own charge, encoded + 34·px + 256 MiB.
- `ProjectionMargin`: every `projectionScratchEstimate()` adds m/16 of its structural estimate; m8 = 7,
  m16 = 6.
- `ByteFilterAdmission.commitStageBytes` = 8n + the margined projection, admitted for every 8-bit publication
  that is not a carried projection; the byte filters' up-front gate charges max(plan, it).
- `FlatOpenAdmission.openScratch` = 34·px + 3·encoded for a ≤ 8-bit JPEG/PNG open; every other 8-bit open
  admits `firstProjectionBytes` = `commitStageBytes(n)`, + 4n when it adopts into the working space.
- Undo, redo and History navigation never refuse: an over-limit step proceeds and leaves a
  `HistoryMemoryWarning` (the status bar segment, MCP `memory_warning`).
- Box Blur charges its own 24 B/px stage (`BOX_BLUR_BYTES_PER_PIXEL`).
- Cancellable jobs have no work ceiling: a destructive filter, geometry, transform or placed-source commit run
  under a control is admitted by memory alone. That is now every operation the app runs under a job control:
  the typed render, mask feather, adjustments, paint, selection sources and Modify Selection, the
  selected-pixels transform, Warp, Convert to Profile, the 8→16 flatten, RAW local development, every typed
  geometry edit that is not a preview (Image Size, Crop, Canvas Size) and Export's sharpening. `MAX_WORK_UNITS`
  (2·10⁹) is now only the interactive preview budget. At 8 GiB, 45 MP (8192×5504) Dust & Scratches r2, Surface
  Blur r5 T15 and Radial Blur Good, at 8 and at 16 bits, each complete as one undo step and, cancelled mid-way,
  leave the document unchanged: twelve rows of the jobs probe
  (`DR_WORKING_LIMIT_BYTES=8589934592 DR_JOBS_PROBE_CASES="large filter jobs" python3 scripts/jobs-probe/verify.py`,
  the release core under the `-Onone` probe; P and E were not measured).
- A preview is never refused for work: over the budget, a 16-bit ⌘T or Crop straighten shows the reduced
  proxy; a float or 8-bit preview with no proxy runs the commit request as its cancellable preview job at full
  size, and reports that plan's own refusal (memory) when it has one.
- Pass rule: P ≥ ⌈1.25 × max E⌉ at 8 GiB, `MallocLargeCache=0`, 3 fresh-instance runs plus a baseline twin.

## Kept fixed caps
- `MAX_NATIVE_BYTES` (the typed native file-length cap) and `MAX_DECODED_BYTES` (the typed cap on one file's
  decoded pixels) are both 8 GiB (2 GiB and 1.6 GB before), each its own literal. *Why 8 GiB:* it is the
  working limit of the assumed minimum machine, so the caps admit every document that limit can hold and refuse
  only on a larger machine. The written bytes did not change and `DRDC_VERSION` was not bumped: the format
  stores no offsets, every length that can pass 4 GiB in total was already a u64, and one plane stays under
  `MAX_PIXELS` and the 1 GiB `MAX_DRDC_PNG_LEN`. The typed reader streams the file record by record with
  positional reads and the writer streams to the temporary file (`core/src/native_stream.rs`), so neither plan
  charges the file: at 8 GiB eight 100 MP 16-bit layers (a 6.4 GB file) save and reopen and a ninth is refused
  by the reopen plan, and three 100 MP float layers plan. The byte (8-bit) format still buffers its file and
  keeps its 1.6 GB decoded cap as `MAX_DRDC_DECODED_BYTES`. Proof:
  `make t F=native_file_size T=a_typed_document_over_2_gib A="--ignored --nocapture"` (four layers, 3.2 GB),
  `T=eight_100_mp_16_bit_layers` (eight layers, past 2^32) and `T=two_100_mp_float_layers` (two float layers,
  past 2^31), all at 8 GiB.
- The PSD 2 GiB file cap, `IDAT_CHUNK_MAX_BYTES` and the other format caps; `CONTIGUOUS_CACHE_BYTES`
  (1 GiB).
- Work caps kept, each because the work is not a cancellable job or its memory plan is sized
  from the cap, and each with a sentence that says what to do: the stroke caps (`MAX_BRUSH_WORK` 2·10⁹,
  `MAX_BRUSH_DABS` 50,000, the RAW local brush's 256 M and 4,096 dabs, the trimap and sampling-area
  brushes: gesture input rasterized per tick; a canvas stroke stops laying ink at the cap); Smudge's
  own `MAX_SMUDGE_WORK` 256 M (the 8-bit stroke runs synchronously on the canvas's final tick);
  `artwork_coverage::MAX_SCAN_WORK` (re-run inside text and shape sessions); the heal
  and inpaint budgets and the matting solver budgets (their memory plans and iteration limits derive from
  them); the Magnetic Lasso's corridor cap (`edge_trace::REFUSAL_CAP`, 8 × 131,072 px: a
  per-pointer-move hover budget, its sentence unchanged); the float layers paste's re-render
  (`dr_precision_doc_paste_layers` has no control); and
  `MAX_DESTRUCTIVE_WORK` for a call with no control, which the app never makes.
- The cache bound is working limit / 16 and the host trim is working limit / 8
  (`core/src/cache_limit.rs`; sized for live-filter bindings, which were removed on 2026-10-10). The trim
  is shared and all-or-nothing.

## Fits at 8 GiB
Release owned instances at `DR_WORKING_LIMIT_BYTES` 8,589,934,592; E = interval `phys_footprint` peak − pre
around the one MCP call; P is the admission's own plan beside the pre state.

| case | 8 GiB outcome | P | max E | P/E |
|---|---|---|---|---|
| 96 MP 8-bit HEIC / gain-map HEIC / AVIF open | admitted | 4,196,342,077; 4,190,291,083; 4,044,034,729 | 3,130,968,656; 3,128,789,680; 3,081,456,136 | 1.340; 1.339; 1.312 |
| second Rotate Canvas 17° of a 24 MP 16-bit two-layer document | turn admitted | 2,810,377,520 | 1,781,793,968 | 1.577 |
| that document's `.doctored` save | admitted (1,061,842,238 B) | 1,333,945,560 | 1,019,888,048 | 1.308 |
| that document's reopen in a fresh instance | admitted | 3,794,975,709 | 2,521,712,392 | 1.505 |
| 45 MP 8-bit Box Blur r5 | admitted | 2,065,143,936 | 1,633,240,216 | 1.264 |
| M1 (m8 = 7) 45 MP 8-bit Dust & Scratches r2 | admitted, no work cap under a job | 2,113,014,146 | 1,643,070,664 | 1.286 |
| M2 45 MP 8-bit Surface Blur r5 T15 | admitted | 2,113,014,146 | 1,647,903,944 | 1.282 |
| M3 45 MP 8-bit Radial Blur Draft, Amount 1 | admitted | 2,294,778,282 | 1,640,154,312 | 1.399 |
| M4 24 MP 16-bit Dust & Scratches r4 | admitted | 1,084,653,991 | 774,275,624 | 1.401 |
| M5 24 MP 16-bit Surface Blur r5 T255 | admitted | 988,653,951 | 779,993,640 | 1.268 |
| after recovering a 45 MP 8-bit document with a snapshot: `save_snapshot` | admitted | no readout | 676,053,496 | — |
| then the next `write_recovery` | admitted | 1,313,506,481 | 270,090,432 | 4.863 |

The Rotate Canvas turn's P predates the later batch that charges the placed raster again; today's plan is larger.

The 8-bit filter commit is 54.5 B/px at m8 = 7, a ceiling of ≈ 157.6 MP at 8 GiB; the other ceilings (at
m8 = 6) are in the record.

## Open items
None.
