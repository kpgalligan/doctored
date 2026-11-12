//! Phase 187a: a placed image's fallback rendered into a
//! 32-bit float document, re-rendered from its original on every transform
//! (the order measured against Photoshop 26.10.0, below).
//! The master stays at its own precision (8, 16 or 32-bit float); the
//! fallback is f32 linear light in the document profile.
//! Since 2026-10-10 the host rasterizes every placed image except a camera
//! RAW in the same edit as the place, so a RAW layer is the only binding
//! that is re-rendered here after it.
//!
//! # The measured order
//!
//! `scripts/photoshop-probe/float_placed.py`, receipt
//! `doctored-fixtures/photoshop/float-placed-473/take1` (2026-10-09, valid):
//!
//! - COLOUR: a 16-bit Display P3 master placed into a 32-bit sRGB document
//!   reads P3 decode → M(P3 → sRGB) UNCLAMPED (H-ext, worst 1.1e-5 relative;
//!   P3 red reads (1.22494, -0.04206, -0.01964)), not clamped to [0, 1]
//!   (H-clip, 0.225 off). An 8-bit sRGB master reads its sRGB decode.
//! - DOMAIN: a ×3 Bilinear scale of a two-colour edge mixes in LINEAR light
//!   (H-lin): the two mixed samples' per-channel weight spreads are 7e-9 and
//!   3e-8 in linear light against 0.39 and 0.36 in the encoded curve.
//! - RE-RENDER: 50 % then 200 % equals the untransformed place exactly.
//! - PASTE: a 16-bit placed image (sRGB and Display P3) pasted into the
//!   32-bit document stays placed and reads the same colour rule.
//! - PL32: a 32-bit TIFF placed keeps 32-bit contents and reads its samples
//!   back exact ((0.5, 2.0, 0.1)); this module builds it (below).
//!
//! So [`render_float`] converts FIRST and resamples the f32 plane:
//!
//! 1. the master in its OWN profile as u16 —
//!    `retained_source_render::source16` with the master's own
//!    profile (an 8-bit master through `precision_conversion::promote_raster`,
//!    the u16 path's own promotion; a 16-bit one shared). An f32 master
//!    (PL32) is taken as it is, shared: it already holds
//!    linear light in its own profile;
//! 2. `precision_clipboard_promote::promoted_image` under Convert — the ONE
//!    u16 → linear f32 conversion (decode by the master's TRC, then the
//!    master → document matrix in f64, rounded once, unclamped; an f32
//!    master takes the same matrix on its linear RGB, and one in the
//!    document's exact profile bytes is shared). An f32 master whose
//!    converted samples leave ±`FLOAT_RANGE_MAX` refuses with
//!    [`FLOAT_MASTER_RANGE_REFUSAL`], not the clipboard's sentence;
//! 3. the accumulated transform, decided by
//!    `retained_source_render::AffineSampling` (an exact same-size translate
//!    shares the plane, another exact form samples Nearest, anything else the
//!    binding's filter; a projective map samples with the filter, as
//!    `retained_source_render::render_high_depth`, the u16 twin, does) with
//!    the map from `retained_source_render::source_map`, through
//!    `precision_sampling::transform_image` on the f32 plane (premultiplied
//!    f32, the D3 clamp to the plane's own visible range).
//!
//! No pixel math lives here. A u16 or 8-bit master whose profile has no
//! matrix/TRC model takes step 2's LUT arm (below); a document profile
//! with none, and an f32 master with none, refuse
//! ([`FLOAT_SOURCE_PROFILE_REFUSAL`]).
//!
//! # The LUT source
//!
//! `scripts/photoshop-probe/float_lut_placed.py`, receipt
//! `doctored-fixtures/photoshop/float-lut-placed-601/take1` (2026-10-10,
//! valid; Photoshop 26.10.0). Two hand-written ICC v2 LUT (lut16 `A2B0` /
//! `B2A0`, PCS XYZ, no matrix/TRC tags) RGB display profiles on 8-bit PNGs,
//! placed into a 32-bit sRGB document; Photoshop keeps both placed, their
//! 8-bit contents keeping the profile by name:
//!
//! - COLOUR: the profile's table → PCS XYZ → the document's matrix,
//!   UNCLAMPED (H-ext, worst 9.0e-6 relative; device red reads (1.39838,
//!   -0.00003, 0.00001), green (-0.39829, 0.99997, -0.04292)), not clamped
//!   to [0, 1] (H-clip, 0.398 off).
//! - DOMAIN: a ×3 Bilinear scale of a two-colour edge whose profile has an
//!   x² input curve mixes in LINEAR light after the conversion (H-lin: the
//!   two mixed samples' per-channel weight spreads are 1.0e-8 and 1.9e-8 in
//!   linear light against 0.246 and 0.253 in the master's device codes), so
//!   the LUT master converts first and the f32 plane is resampled, as a
//!   matrix master does.
//!
//! So step 2 for such a master is
//! `precision_clipboard_promote::lut_promoted_image` through
//! `retained_source_render::lut_pipeline` (relative colorimetric, BPC off,
//! the choice every source render makes), and steps 1 and 3 are unchanged.
//! The fixture's black is 0, so black-point compensation on a raised-black
//! LUT source is unmeasured, here as at 8 and 16 bits. A raster pixel
//! or layers paste in a LUT profile still refuses, and a 16-bit LUT
//! master cannot reach a document: the host's u16 input needs a matrix/TRC
//! profile. The plans already charge the pipeline
//! (`retained_source_render::profile_cost`).
//!
//! # Host media
//!
//! `scripts/photoshop-probe/float_host_raw.py`, receipt
//! `doctored-fixtures/photoshop/float-host-602/take2` (2026-10-10, valid;
//! Photoshop 26.10.0; take1 was invalid, two job defects the runner's
//! docstring records). A 384×96 H.264 movie placed into a 16-bit and a
//! 32-bit sRGB document, the timeline set to frame 0 and to frame 2:
//!
//! - V-SDR: Photoshop keeps the movie placed at both depths, and all 576
//!   samples of each 32-bit frame read the 16-bit frame's sRGB decode within
//!   2e-3 (the two frames are 0.378 apart, so the time was honoured); no
//!   sample leaves [0, 1].
//!
//! So a host still or movie frame takes this module's render unchanged: its
//! 8- or 16-bit master (the host's decode; HDR media is refused at intake at
//! every depth) is converted into linear light by its own profile and
//! resampled, as any placed image is. Since 2026-10-10 the host rasterizes
//! the placed layer in the same edit, so no binding outlives the place.
//!
//! # Adobe Camera Raw
//!
//! Phase 1's take placed `camera.NEF` at 722×480 (a 24 % resample: its
//! 300 ppi in a 72 ppi document), so it decided nothing.
//! `scripts/photoshop-probe/float_raw.py` placed it at 100 % (3008×2000 into
//! 300 ppi targets) in a 16-bit and a 32-bit sRGB document, with and without
//! an exposure sidecar: receipts `doctored-fixtures/photoshop/float-raw-602/`
//! `take1` (+2.00 EV) and `take2` (+4.00 EV), Photoshop 26.10.0, 2026-10-10.
//! Both missed one validity term only, a share of 16-bit highlights clipped
//! to 65535 on all three channels: Adobe Camera Raw rolls highlights off below
//! white even at +4 EV. Every R-SDR condition held: each 32-bit row reads the
//! 16-bit row's sRGB decode within 2e-3 on 100 % of the samples the 16-bit
//! row did not clip, the largest luminance is 0.99978 and none passes 1.02
//! (a channel leaves [0, 1] only as an in-range colour of Adobe Camera Raw's wider
//! workflow space, 0.13–0.37 % of samples). The controller's ruling accepted
//! **R-SDR** on take2: an Adobe Camera Raw development placed at 32 bits is its SDR
//! development.
//!
//! So a RAW layer in a float document (placed, or pasted as layers from a
//! 16-bit RAW document, `layer_clipboard_float::kept_source`) keeps its u16
//! developed master and its captured original, and every redevelopment,
//! preset, settings synchronization, decoder migration and RAW local renders
//! that master through this module unchanged, as any placed 16-bit image is
//! (`raw_source_edit` attaches the original to the float record as it does at
//! 8 and 16 bits). The host whitelists the five RAW menu items and the 13
//! document RAW tools there.
//!
//! # Admission
//!
//! [`float_render_scratch`] is the float render's term on top of
//! `retained_source_render::work_plan_within` at `fallback_depth` 32 (which
//! already charges the f32 input/output/tiling planes and the u16 promotion
//! of an 8-bit master): the promoted f32 master, `s · 16`, that sits beside
//! `transform_image`'s contiguous copy, plus
//! [`FLOAT_SOURCE_MARGIN_PER_PIXEL`] per source pixel.
//!
//! # Measurement record (phase 187a, 2026-10-09)
//!
//! `precision_clipboard_promote`'s method: a Release app with
//! `DR_WORKING_LIMIT_BYTES = 8589934592`, `MallocLargeCache=0`,
//! `DR_BACKGROUND=1`, private `DR_*_DIR`s and a private pasteboard, one fresh
//! instance per measured MCP call (8 s settle after setup); E =
//! `ri_interval_max_phys_footprint` − the footprint read right after
//! `proc_reset_footprint_interval`. Target: the float_11f `scene.tiff` resized
//! to 6000×4000; masters a 6000×4000 8-bit sRGB PNG and a 6000×4000 16-bit
//! Display P3 TIFF. A, from the core's own plans over the same files:
//! `place` = the decoded master + `source_plan` scratch; `transform` = the
//! `geometry_plan` scratch of the app's `affine_set` commit at 150 % Bilinear
//! (a 9000×6000 render); `paste` = `paste_layers_allocation` of a Convert
//! layers paste of the master placed in a 16-bit sRGB document. Harness,
//! inputs and raw output:
//! `doctored-fixtures/photoshop/float-placed-473/admission/` (`measure.py`,
//! `plan_a/`, `runs/20261009-143923/`). Other builds were running on the
//! machine during the run; `phys_footprint` is per process, so E is not
//! affected. (An earlier run measured the transform at 125 % because the
//! native plan then charged an embedded master as a raw f32 raster, which
//! refused 150 %; the plan now charges it as the PNG it is.)
//!
//! | case | A | E (three runs) | A / max E |
//! | --- | --- | --- | --- |
//! | place 8 | 1,829,830,692 | 1,060,569,952 / 1,060,684,640 / 1,060,307,832 | 1.725 |
//! | place 16 | 1,733,824,548 | 1,155,580,864 / 1,155,597,248 / 1,154,139,048 | 1.500 |
//! | transform 8 | 6,012,605,325 | 1,928,021,336 / 1,928,562,008 / 1,928,627,544 | 3.118 |
//! | transform 16 | 5,772,597,645 | 1,928,021,336 / 1,928,234,328 / 1,928,070,488 | 2.994 |
//! | paste 8 | 2,505,291,848 | 962,167,528 / 962,888,400 / 962,691,816 | 2.602 |
//! | paste 16 | 2,313,285,704 | 963,445,456 / 961,675,960 / 961,725,112 | 2.401 |
//!
//! The two transform rows are a re-run of the same harness and inputs on
//! 2026-10-10 (`float-placed-473/admission-648/`,
//! `runs/20261010-160812/record.json`, where the place and paste rows read
//! the same A and a max E within 0.1 % of the rows above, and the twin 0).
//! Their A was 3,372,536,205 and 3,132,528,525 and grew by 2,640,069,120,
//! which is 5/4 (`TRANSFORM_HEADROOM`) × the never-run raster plane every
//! edit now charges (9000·6000·16 for the output, 6000·4000·16
//! for the source copy, one more output-sized transient and 55,296 B of tile
//! rounding); the measured peak did not move.
//!
//! The `list_documents` twin read E = 0 / 0 / 0. Every case holds A ≥
//! ⌈1.25 × max E⌉ with no margin, so [`FLOAT_SOURCE_MARGIN_PER_PIXEL`] was 0
//! when measured (26 since the `RAW` record below).
//!
//! # Measurement record (32-bit float master)
//!
//! 187a's method (2026-10-10, Apple M2 Max, macOS 26.5.1): a Release app,
//! `DR_WORKING_LIMIT_BYTES = 8589934592`, `MallocLargeCache=0`, 8 s settle,
//! one fresh process per measured MCP call, three runs. Target: 187a's
//! `target.rz`; masters 6000×4000 RGBA f32 Deflate TIFFs (x and y ramps,
//! blue steps every 8 px), one in the target's sRGB, one in Display P3. A:
//! `place` = the decoded master (`w · h · 16`) + `source_plan` scratch;
//! `transform` = the `geometry_plan` scratch of the app's `affine_set`
//! commit at 140 % Bilinear (8400×5600). 150 % refuses in-band: its
//! 9000×6000 f32 fallback beside the 384 MB master and the 384 MB target
//! (1.632 GB decoded) exceeds the native 1.6 GB cap. Harness, `plan_a/`,
//! `core_e/` and raw output:
//! `doctored-fixtures/photoshop/float-placed-473/admission-600/`
//! (`measure.py`, `runs/20261010-023712/measured.json`, on the build with
//! the final constants).
//!
//! | case | A | E (three runs) | A / max E |
//! | --- | --- | --- | --- |
//! | place32 | 1,924,194,304 | 963,085,056 / 962,921,192 / 963,232,512 | 1.998 |
//! | place32p3 | 1,925,824,548 | 1,348,682,760 / 1,348,797,448 / 1,348,797,448 | 1.428 |
//! | transform32 | 5,213,748,800 | 1,319,224,256 / 1,319,060,416 / 1,321,255,872 | 3.946 |
//!
//! The `list_documents` twin read E = 0 / 16,384 / 0 in that run (one page)
//! and 0 / 0 / 0 re-run alone (`runs/20261010-024238/`) and in the earlier
//! run on the pre-final build (`runs/20261010-021434/`). Every case holds A ≥
//! ⌈1.25 × max E⌉ with no margin, so [`FLOAT_SOURCE_MARGIN_PER_PIXEL`] stays
//! 0; the host's `ImageSourceFile.scratchPerPixel32` is fitted to the same
//! place E (60, Display P3 binding). `core/tests/float_master_24mp.rs` pins
//! each floor.
//!
//! # Measurement record (LUT source)
//!
//! 187a's method (2026-10-10): a Release app,
//! `DR_WORKING_LIMIT_BYTES = 8589934592`, `MallocLargeCache=0`, 8 s settle,
//! one fresh process per measured MCP call, three runs. Target: 187a's
//! `target.rz`; master 187a's 6000×4000 8-bit ramps as a PNG whose `iCCP` is
//! the take's linear LUT profile ("rz601 LUT RGB", the same table codes). A:
//! `place` = the decoded master (`w · h · 4`) + `source_plan` scratch;
//! `transform` = the `geometry_plan` scratch of the app's `affine_set`
//! commit at 150 % Bilinear (9000×6000). Harness, `plan_a/` and raw output:
//! `doctored-fixtures/photoshop/float-placed-473/admission-601/`
//! (`measure.py`, `runs/20261010-142628/record.json`).
//!
//! | case | A | E (three runs) | A / max E |
//! | --- | --- | --- | --- |
//! | place 8 LUT | 1,829,830,420 | 1,060,340,600 / 1,060,537,184 / 1,060,569,976 | 1.725 |
//! | transform 8 LUT | 6,012,604,985 | 1,928,349,016 / 1,928,660,312 / 1,928,791,384 | 3.117 |
//!
//! The `list_documents` twin read E = 0 / 0 / 0. Both cases hold A ≥
//! ⌈1.25 × max E⌉ (1,325,712,470 and 2,410,989,230) with no margin, so
//! [`FLOAT_SOURCE_MARGIN_PER_PIXEL`] stays 0. Each E is within 0.03 % of
//! 187a's sRGB master's. The sRGB master's transform plan on this core is
//! 6,012,605,325, the 187a record's re-measured transform row, which
//! explains the growth since 187a. The measured build differs from this one
//! only in comments.
//!
//! # Measurement record (host media)
//!
//! 187a's method (2026-10-10): a Release app,
//! `DR_WORKING_LIMIT_BYTES = 8589934592`, `MallocLargeCache=0`, 8 s settle,
//! one fresh process per measured MCP call, three runs. The host admits a
//! place and a frame selection in two stages, so each stage is judged against
//! its own figure. Inputs (`scripts/host-still-admission/author.swift`):
//! 6000×4000 8-bit and 10-bit HEICs, and the 8-bit one paired with a 64×48
//! H.264 movie; target 187a's `target.rz`.
//!
//! - DECODE: A = `HostSourceInput.scratch`; E = the same file placed with no
//!   authorization into a 64×64 document of the other depth, which runs the
//!   capture stage whole and then refuses in-band (the stage takes no
//!   document, so it is the float call's decode).
//! - RENDER: A = the `source_plan` scratch of the host's request (`place` /
//!   `replace`, `convert_fallback`, the fit transform) over a PNG twin of
//!   the decoded master, + the encoded originals + 64 MiB (a frame: + 64 MiB
//!   only); E = the whole float MCP call (`place_image`), which bounds the
//!   stage from above.
//!
//! | case | stage | A | E (three runs) | A / max E |
//! | --- | --- | --- | --- | --- |
//! | place 8 | decode | 735,655,409 | 510,099,816 / 510,230,888 / 510,247,272 | 1.442 |
//! | place 8 | render | 1,814,427,167 | 1,185,743,784 / 1,186,202,560 / 1,185,711,016 | 1.530 |
//! | place 16 | decode | 1,054,349,879 | 708,870,672 / 709,231,144 / 709,181,944 | 1.487 |
//! | place 16 | render | 1,625,670,369 | 1,286,063,064 / 1,286,046,680 / 1,286,095,784 | 1.264 |
//! | frame | decode | 735,860,132 | 510,034,256 / 510,345,576 / 510,312,808 | 1.442 |
//! | frame | render | 1,800,939,556 | 962,921,144 / 962,888,376 / 962,888,376 | 1.870 |
//!
//! The frame case (`set_host_frame kind: still` on the pair) has no whole
//! call at this size: the app refuses to place the pair showing its 64×48
//! frame fitted ×83 onto the still's canvas ("source transform exceeds the
//! bounded sampling work limit"). Its decode row is the pair's capture (both
//! originals), and its render row is the stage alone in a process over this
//! core: the target, the still's twin and the originals loaded, then the one
//! `replace` and the attach of the originals from the old record, the result
//! held until E is read. The `list_documents` twin read E = 0 / 0 / 0. Every
//! row holds A ≥ ⌈1.25 × max E⌉ (the 16-bit place's render by 18 MB), so
//! [`FLOAT_SOURCE_MARGIN_PER_PIXEL`] stays 0 and the host's decode constants
//! are unchanged. Harness, `plan_a/`, `core_e/` and raw output:
//! `doctored-fixtures/photoshop/float-host-602/admission/` (`measure.py`,
//! `runs/20261010-161344/measured.json`). The measured build differs from
//! this one only in comments.
//!
//! # Measurement record (RAW)
//!
//! 187a's method (2026-10-10): a Release app,
//! `DR_WORKING_LIMIT_BYTES = 8589934592`, `MallocLargeCache=0`, 8 s settle,
//! one fresh process per measured MCP call, three runs. Inputs:
//! `synthetic_dng(path, 4000, 6000)` (orientation 6: a 6000×4000
//! development, 48,001,004 encoded bytes) opened at 16 bits and pasted in
//! place into 187a's `target.rz` (`raw32`), that layer scaled 150 % Bilinear
//! (`raw32t`), and the 16-bit `.rz` rewritten by `raw-probe/native.py`
//! `incompatible_decoder` and pasted the same way (`raw32old`). Cases:
//! `redevelop_raw` exposure 0.5 on `raw32` / `raw32t` (redev / redevt);
//! `edit_raw_development` with one / four radial locals and exposure 0.2 on
//! `raw32` / `raw32` / `raw32t` (local1 / local4 / local1t);
//! `preview_raw_decoder_migration` on `raw32old` (migrate). A, as the host
//! computes it: decode = `RawSourceWork.scratch`; render = the `source_plan`
//! scratch of `RawSourceWork.replacing`'s replace over a 16-bit twin of the
//! development; local = `RawLocalWork.measuredScratch` (k = 1 / 4 / 1);
//! migrate = `RawDecoderMigration.prepare`'s budget. A redevelopment's two
//! stages are judged apart: the whole call's E bounds both from above, and a
//! stage that fails against it is measured alone (redevt's decode by the same
//! call on the 16-bit document, the same decode under the same charge; its
//! render in a process over this core, the record and the twin loaded, the
//! one replace and `dr_precision_doc_attach_raw_source_from`, the result
//! held). Harness, `plan_a/`, `core_e/` and raw output:
//! `doctored-fixtures/photoshop/float-raw-602/admission/` (`measure.py`,
//! `runs/20261010-185738/measured.json`).
//!
//! | case | A | E (three runs) | A / max E |
//! | --- | --- | --- | --- |
//! | redev decode | 2,655,165,380 | 1,170,015,288 / 1,200,325,160 / 1,200,554,560 | 2.212 |
//! | redev render | 2,165,830,692 | 1,170,015,288 / 1,200,325,160 / 1,200,554,560 | 1.804 |
//! | redevt decode (16-bit call) | 2,655,165,380 | 1,536,852,808 / 1,392,067,256 / 1,391,313,592 | 1.728 |
//! | redevt render (alone) | 3,125,830,692 | 2,499,905,312 / 2,499,856,136 / 2,499,823,392 | 1.250 |
//! | local1 | 4,488,003,012 | 1,630,979,128 / 1,630,815,288 / 1,407,419,760 | 2.752 |
//! | local4 | 5,280,003,012 | 1,777,435,752 / 1,585,153,032 / 1,716,388,968 | 2.971 |
//! | local1t | 4,488,003,012 | 2,798,750,096 / 2,798,750,072 / 2,800,240,968 | 1.603 |
//! | migrate | 2,847,165,380 | 1,200,620,120 / 1,062,126,072 / 1,200,128,600 | 2.371 |
//!
//! The first run, with the margin 0, missed one stage: redevt's render alone
//! read 2,499,872,544 against a plan of 2,501,830,692 (1.001×;
//! `runs/20261010-183452/`), the replace of a 150 %-scaled layer rendering a
//! 9000×6000 f32 fallback whose structural terms the plan already matches
//! exactly. So [`FLOAT_SOURCE_MARGIN_PER_PIXEL`] is 26, the smallest integer
//! that passes (25 gives 3,101,830,692 < ⌈1.25 × 2,499,905,312⌉ =
//! 3,124,881,640); the table is the build with that constant, and a second
//! full run on it (`runs/20261010-184858/`) passed every case too. Its
//! whole-call E is 2,513–2,517 MB; the render charge is 1.242× its largest,
//! 2,516,994,064. The ~17 MB over the render alone is put down to the
//! decode's residue (not measured apart), and the stage-alone row judges it. The `list_documents` twin read one page
//! (16,384) in one run of each full run, as the float master record's did, and 0 / 0 / 0
//! re-run alone (`runs/20261010-185650/`). The settings tools (`apply_raw_settings`,
//! `apply_raw_preset`, `sync_raw_settings`, a patched `inspect_raw_development`)
//! run `RawSourceWork.prepare`, redev's path; `copy_raw_local_settings` and
//! the Develop window's preview run `RawLocalWork.request`, local1's; the
//! publication is the float projection every float edit shares. No host
//! constant changed. Every row above this record was measured with the margin
//! 0: each float render plan now charges 26 B more per master pixel
//! (624,000,000 B at 24 MP), so their A / max E only rises.
//! The measured build differs from this one only in comments.
//!
//! The local rows' A was later restated; E was not re-measured. The host's
//! `RawLocalWork.request` now charges the larger of two figures:
//! `measuredScratch`, refit to 522 MiB plus (73 + 13·k + 16·e) B/px plus
//! three times the file, and the replace forecast, which is the replace plan
//! plus 32 MiB plus 8·(k + 2) B/px. At the record's 24,000,000 px and
//! 48,001,004 encoded bytes: local1 2,775,385,124 (the forecast over redev's
//! 2,165,830,692 render plan; 1.702× its max E), local4 3,691,359,684
//! (`measuredScratch`; 2.077×), local1t 3,735,385,124 (the forecast over
//! redevt's 3,125,830,692; 1.334×). Each is ≥ ⌈1.25 × max E⌉, so
//! [`FLOAT_SOURCE_MARGIN_PER_PIXEL`] stays 26.

use std::sync::Arc;

use crate::doc_transform::Affine;
use crate::icc::IccProfile;
use crate::job_control::JobContext;
use crate::precision_clipboard::PasteProfile;
use crate::precision_clipboard_promote::{
    lut_promoted_image, promoted_image, RANGE_REFUSAL_PREFIX,
};
use crate::precision_image::PrecisionImage;
use crate::precision_sampling;
use crate::retained_source::RetainedSource16;
use crate::retained_source_render::{lut_pipeline, render16, source_map, AffineSampling};
use crate::source_asset::SourceAsset;
use crate::source_transform::{self, SourceTransform};

/// The fitted margin a float render charges per SOURCE pixel on top of the
/// structural terms. Why 26: the structural terms alone admitted the place,
/// the 150 % re-render and the Convert layers paste of a 24 MP master at
/// 1.50× to 3.12× their measured peaks and the host-media stages at 1.26× to
/// 1.87×, but a RAW redevelopment of a 150 %-scaled layer
/// read a render peak the plan matched exactly (1.001×): its replace renders
/// a 9000×6000 f32 fallback with nothing to spare. 26 is the smallest integer
/// that admits it at 1.25× (the module doc's `RAW` record); every other
/// row's ratio only rises.
pub(crate) const FLOAT_SOURCE_MARGIN_PER_PIXEL: u64 = 26;

/// The refusal of a DOCUMENT profile with no matrix/TRC model, or of an f32
/// master whose own profile has none: linear light is defined by a matrix
/// (the document's primaries, an f32 master's own), and a LUT has none. A u16
/// or 8-bit master in a LUT profile does not refuse (module doc).
/// Neither refusal is reachable today: `FloatPolicy::for_profile` and
/// `float_io` already require a model.
pub(crate) const FLOAT_SOURCE_PROFILE_REFUSAL: &str =
    "placing into a 32-bit float document requires an RGB matrix/TRC source profile";

/// The refusal of an f32 master whose samples, converted into the document
/// profile, leave the float range (module doc, step 2). Whether Photoshop
/// keeps such values is unmeasured.
pub(crate) const FLOAT_MASTER_RANGE_REFUSAL: &str = "the placed image's colours convert to a value outside the 32-bit float range (±65504) in the document's profile; the place is refused rather than clipped";

/// An f32 master converted into `profile` in linear light with no clamp
/// (`promoted_image`), its range refusal reworded as
/// [`FLOAT_MASTER_RANGE_REFUSAL`]. Read by [`source_float`] and, below 32
/// bits, by `retained_source_render`'s `float_master_codes` (Photoshop
/// converts before it clips, take1 PP3 `matrix_clip`).
pub(crate) fn master_in_profile(
    master: &PrecisionImage,
    profile: &Arc<IccProfile>,
    job: JobContext<'_>,
) -> Result<PrecisionImage, String> {
    promoted_image(master, profile, PasteProfile::Convert, job).map_err(master_range_refusal)
}

/// The clipboard's range refusal reworded as [`FLOAT_MASTER_RANGE_REFUSAL`];
/// any other error passes through.
fn master_range_refusal(e: String) -> String {
    if e.starts_with(RANGE_REFUSAL_PREFIX) {
        FLOAT_MASTER_RANGE_REFUSAL.into()
    } else {
        e
    }
}

/// The binding's master as f32 linear light in `profile` (module doc),
/// BEFORE any transform. Refuses a document profile, or an f32 master's,
/// with no matrix/TRC model ([`FLOAT_SOURCE_PROFILE_REFUSAL`]).
pub(crate) fn source_float(
    asset: &Arc<SourceAsset>,
    profile: &Arc<IccProfile>,
    job: JobContext<'_>,
) -> Result<PrecisionImage, String> {
    let own = asset.profile().clone();
    if profile.model().is_none() || (asset.rgba32().is_some() && own.model().is_none()) {
        return Err(FLOAT_SOURCE_PROFILE_REFUSAL.into());
    }
    if let Some(master) = asset.rgba32() {
        return master_in_profile(master, profile, job);
    }
    let master = crate::retained_source_render::source16(asset, &own, job)?;
    match lut_pipeline(&own, profile)? {
        Some(pipeline) => {
            lut_promoted_image(&master, &pipeline, profile, job).map_err(master_range_refusal)
        }
        None => promoted_image(&master, profile, PasteProfile::Convert, job),
    }
}

/// `render_float` in a float document, the u16 `render16` otherwise, which
/// resamples through `codes` (`PrecisionDocument::linear_codes`).
pub(crate) fn render_typed(
    source: RetainedSource16,
    profile: &Arc<IccProfile>,
    float: bool,
    codes: Option<&dyn crate::sampling_linear::LinearCodes<u16>>,
    job: JobContext<'_>,
) -> Result<RetainedSource16, String> {
    if float {
        render_float(source, profile, job)
    } else {
        render16(source, profile, codes, job)
    }
}

/// The binding's fallback as f32 linear light in `profile` (module doc's
/// order, [`source_float`]), and its offset. Refuses what [`source_float`]
/// refuses.
pub(crate) fn render_float(
    mut source: RetainedSource16,
    profile: &Arc<IccProfile>,
    job: JobContext<'_>,
) -> Result<RetainedSource16, String> {
    let linear = source_float(&source.asset, profile, job)?;
    let bounds = source_transform::bounds(source.asset.dimensions(), source.transform)?;
    let sampling = match source.transform {
        SourceTransform::Affine(m) => AffineSampling::new(
            linear.dimensions(),
            (bounds.2, bounds.3),
            Affine::from_array(m),
            source.filter,
        ),
        SourceTransform::Projective(_) => AffineSampling::Sample(source.filter),
    };
    source.rendered = match sampling {
        AffineSampling::Shared => linear.reidentified(),
        // A float master is linear already: no tables to resample through.
        AffineSampling::Sample(filter) => precision_sampling::transform_image(
            &linear,
            (bounds.2, bounds.3),
            &source_map(source.transform, (bounds.0, bounds.1))?,
            filter,
            None,
            job,
        )?,
    };
    source.rendered_offset = (bounds.0, bounds.1);
    Ok(source)
}

/// The float render's own scratch for a `source`-sized master rendered into
/// a `dest`-sized fallback (module doc): `s · 16 + FLOAT_SOURCE_MARGIN_PER_PIXEL
/// · s`, checked. Charged only in a float document. The measured H-lin
/// order's term is source-sized, so `_dest` charges nothing.
pub(crate) fn float_render_scratch(source: (u32, u32), _dest: (u32, u32)) -> Result<u64, String> {
    let s = u64::from(source.0)
        .checked_mul(u64::from(source.1))
        .ok_or("source render size overflow")?;
    s.checked_mul(16 + FLOAT_SOURCE_MARGIN_PER_PIXEL)
        .ok_or_else(|| "source render size overflow".into())
}
