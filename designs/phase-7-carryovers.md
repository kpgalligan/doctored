# Phase 7 carryover ledger (closed record)

- **Status:** Accepted — open items remain ([#2](https://github.com/kpgalligan/doctored/issues/2), [#9](https://github.com/kpgalligan/doctored/issues/9), [#11](https://github.com/kpgalligan/doctored/issues/11), [#31](https://github.com/kpgalligan/doctored/issues/31)); closed as a ledger 2026-10-30, its open rows tracked as those issues
- **Original:** [archive/phase-7-carryovers.md](../archive/phase-7-carryovers.md) — each row's receiving slice, close criterion and verification evidence

## Context

Phases 7A–7E shipped the qualified 16-bit public workflow, not full high-depth tool parity. The ledger gave
each remaining gap a P7 row with one receiving roadmap slice and a close criterion: a row closed only when its
UI, MCP and Actions paths, undo and dirty state, native persistence and recovery worked together at depth
against independent references. Of the 39 rows, 29 are verified (P7-39 with one residual case open), P7-12
shipped with its gate run dropped, 3 went with the retained sources removed on 2026-10-10, and 6 are open as
issues. What each depth admits today is in the
[operation support matrix](phase-7-operation-support.md).

## Register

- P7-01 — typed retained ordinary-image sources and their rerenders — removed 2026-10-10 ([removal ADR](convert-to-smart-object-status.md); verified earlier in [8A](phase-8-retained-source-status.md))
- P7-02 — high-depth RAW development and redevelopment within the declared camera and decoder scope — verified in [8C](phase-8-retained-source-status.md)
- P7-03 — high-depth host-only image sources and Live Photo/video frame rerenders — removed 2026-10-10 ([removal ADR](convert-to-smart-object-status.md); verified earlier in [8F](phase-8f-host-source-status.md))
- P7-04 — editable text and shape descriptions, rerendered after edits, transforms and profile changes — verified in [8E](phase-8e-artwork-status.md)
- P7-05 — attributed text colours, intrinsic colour glyphs and artwork coverage beyond 256 levels — verified in [8E](phase-8e-artwork-status.md)
- P7-06 — selection-limited native tone and filter edits, with fine native-code and normalized parameter entry — verified in [9F](phase-9f-selected-tone-status.md)
- P7-07 — selection-weighted and named partial-backdrop histograms and readouts — verified in [9F](phase-9f-selected-tone-status.md)
- P7-08 — mask and channel painting, fill, filter and morphology, RGBA-plane writeback and camera auxiliary-matte import — verified in [9G](phase-9g-selections-masks-status.md)
- P7-09 — scalar isolation and mask, channel and plane display, including per-layer inspection — verified in [9G](phase-9g-selections-masks-status.md)
- P7-10 — source-derived selections (alpha, luminance, colour, wand, Color Range, Quick Selection, subject) read from the typed source — verified in [9G](phase-9g-selections-masks-status.md)
- P7-11 — u16 channel-to-selection loading and selection and Quick Mask coverage beyond 256 levels — verified in [9G](phase-9g-selections-masks-status.md)
- P7-12 — Refine Selection at u16 with reusable u16 masks, and Select and Mask's Edge Detection, Refine Edge brush, Decontaminate Colors and outputs at 8 and 16 bits — typed half verified in [9I typed refinement](phase-9i-typed-cutouts-status.md); the edge tools shipped 2026-10-11 (same ADR); the assisted [gate v2](phase-9i-gate-v2-status.md) run dropped (2026-10-30): the matting gate stopped being a ship condition on 2026-10-10
- P7-13 — pressure size and opacity, airbrush timing and brush coverage beyond 256 levels — verified in [9H](phase-9h-paint-fill-status.md)
- P7-14 — the 27 paint blend modes, mode-free erasure, colour-match bucket fill, gradients, dodge/burn and paint extent expansion at depth — verified in [9H](phase-9h-paint-fill-status.md)
- P7-15 — retouch sampling (current, current and below, all layers), ignore-adjustments and new retouch-layer output — verified in [9D-1](phase-9-live-filters-status.md)
- P7-16 — fractional clone and heal sampling, source overlays and transforms, and destination extent expansion — verified in [9D](phase-9-live-filters-status.md)
- P7-17 — typed spot healing and red-eye correction — verified in [9D](phase-9-live-filters-status.md)
- P7-18 — content-aware synthesis (PatchMatch) with the full fill, output and source-mask workflow and native Patch — verified in [9E](phase-9e-content-aware-status.md)
- P7-19 — group and backdrop semantics and adjustment-layer blending, every mode at native depth — verified in [10D](phase-10d-adjustment-blending-status.md)
- P7-20 — clipping chains, Blend If and layer styles and effects at native depth — verified in 10D ([clipping](phase-10d-clipping-status.md), [Blend If](phase-10d-blend-if-status.md), [layer styles](phase-10d-layer-styles-status.md)); their PSD carriage at both depths is P7-21's
- P7-21 — high-depth PSD import and export of layer effects (`lfx2`), fill opacity (`iOpa`) and Blend If, in both directions at both depths — verified in [10C](phase-10c-constructs-status.md)
- P7-22 — an explicit appearance-preserving profile conversion (`adjustments: "preserve_appearance"`) beside the keep-parameters default — verified in [11C](phase-11c-output-color-status.md)
- P7-23 — the 11D format additions' named capability and loss matrix — verified in [11D](phase-11d-format-additions-status.md), whose matrix is the current per-format record
- P7-24 **(closed 2026-11-12)** — linear-light evaluation as an explicit per-document policy — verified in [11E](phase-11e-linear-light-status.md): phase 1, then phases 2–3 (premultiplied-linear resampling for Image Size, Free Transform, warp and a RAW layer's re-render; the non-separable modes, Dissolve, layer styles, Blend If and the Shadows/Highlights guide). Destructive paint, fill and blur stay storage edits (the ADR's D9).
- P7-25 — the 32-bit float document mode (linear light, extended range, negative components kept until display and SDR output) — verified in [11F](phase-11f-hdr-float-status.md); `convert_precision` mode `rgba32` also promotes an 8- or 16-bit document into it, structure "preserve" only (`core/src/precision_conversion_float.rs`)
- P7-26 — subset grouping and ungrouping, reorder, delete and merge, locks and links, guides and ruler origin, and PPI-only edits at depth — verified in [12E](phase-12e-layer-ops-status.md), with typed Canvas Size in [12F](phase-12f-geometry-status.md)
- P7-27 — adjustment insertion at any stack position and continuous opacity preview at depth — verified in [12E](phase-12e-layer-ops-status.md)
- P7-28 — canvas move and affine/perspective transforms, group and adjustment-content transforms, straighten, canvas expansion and warp at depth — verified in [12F](phase-12f-geometry-status.md); at 32-bit float every geometry op is admitted: exact ops copy bits and filtered ones resample premultiplied extended-range f32 (`core/src/precision_geometry.rs`)
- P7-29 — Cut, Copy, Copy Merged, Paste and New from Clipboard at depth, with editable payloads — verified in [12G](phase-12g-clipboard-status.md)
- P7-30 — structure-preserving precision conversion (`convert_precision structure:"preserve"`) in every direction, carrying RAW sources and warp bindings — verified in [12H](phase-12h-conversion-status.md)
- P7-31 — full-frame legacy promotion and mask fallbacks, and cancellation only between render and preparation stages; [13A](phase-13a-regional-status.md) shipped regional evaluation and measured checkpoints, and the regional latency and remaining fallbacks are left — open: [#9](https://github.com/kpgalligan/doctored/issues/9)
- P7-32 — synchronous, noncancellable codec and source snapshot/fingerprint work, and uncompressed or repeated native, snapshot and recovery payloads — open: [#11](https://github.com/kpgalligan/doctored/issues/11)
- P7-33 — a published large-document corpus (24/48 MP multilayer, history and snapshot workloads, encoded-input budgets, every promised export) measured at a 16 GB Mac's 8 GiB working limit — open: [#11](https://github.com/kpgalligan/doctored/issues/11)
- P7-34 — strokes, text and canvases beyond the 50,000-dab, 4,096-UTF-16-unit and plane/native-aggregate limits, through bounded processing — open: [#11](https://github.com/kpgalligan/doctored/issues/11)
- P7-35 — typed replacement, relinking, refresh, embed/package and cross-depth placement of placed sources — removed 2026-10-10 ([removal ADR](convert-to-smart-object-status.md); verified earlier in [8B](phase-8-retained-source-status.md))
- P7-36 — genuine floating-point TIFF import, 32-bit float TIFF export and deliberate tone-mapped SDR delivery — verified in [11F](phase-11f-hdr-float-status.md); F16 TIFF and OpenEXR export also shipped (`core/src/doc_export_float.rs`, `core/src/exr_float.rs`); float-predictor TIFF encoding dropped (2026-10-30): Predictor 3 is not written, and a Deflate float TIFF without it is valid and reopens bit-exact, only larger
- P7-37 — Knockout (None/Shallow/Deep) and `blend_interior_as_group` at both depths, with PSD `knko`/`infx` carriage — verified in [12E](phase-12e-layer-ops-status.md)
- P7-38 — a Background layer kind that PSD import sets, PSD export writes and `.doctored` persists, so a Deep knockout over an ordinary opaque bottom layer (`deep_over_ordinary_bottom`) matches — verified at RGB 8 and 16 bits as the Background mark (`core/src/doc_background.rs`, native word 113; `core/tests/psd_background.rs` matches the take at both depths, max 2 / mean 0.5); the Gray, Lab, CMYK and 32-bit float PSD codecs set and write no mark and keep the disclosure — open: [#31](https://github.com/kpgalligan/doctored/issues/31)
- P7-39 — `blend_interior_as_group` on a clip base with visible members, at both depths in the full, regional and budget paths — verified in [12E](phase-12e-layer-ops-status.md) (2026-10-13); a clip base below 100 % Fill is still unqualified — open: [#2](https://github.com/kpgalligan/doctored/issues/2)

## Contracts that remain intentional

- The flattened conversion keeps its flattening behaviour and is never implied: MCP `convert_precision` needs `flatten:true`, and the Convert Precision sheet opens on Preserve Layers unless the plan blocks it. Preserve Layers is the only conversion into 32-bit float, and no conversion recreates discarded source precision or editable state (`app/Sources/PrecisionModeConversion.swift`, `app/Sources/ConvertPrecisionSheetController.swift`).
- Nearest quantization, canvas-anchored 8 × 8 Bayer RGB dithering, undithered alpha and latent RGB under zero alpha (kept and converted on its own, never cleared) are explicit numerical contracts (`core/src/precision_quantize.rs`, `core/src/precision_image.rs`); native samples and display or interchange conversions stay separate.
- A 256-bin histogram is a display choice: exact endpoint counts and sampled-versus-complete statistics stay explicit (`core/src/precision_stats.rs`).
- Byte-valued tool and Action arguments keep their meaning, finer values arrive through separate arguments, and intrinsic glyph assets keep their disclosed precision.
- Keep-parameters stays the default profile conversion and `preserve_appearance` the explicit alternative, so old documents do not change (`convert_profile`'s `adjustments`).
- Encoded evaluation stays every document's default; linear light and 32-bit float are separate, versioned modes, never an implicit change (`core/src/doc_evaluation.rs`).
- Limits and bounded refusals stay enforced until a replacement is qualified, and no phase promises unlimited data. Closing an open row's issue cannot weaken an existing numerical, quality, cancellation or resource gate; workload targets are declared before measuring, and a resized substitute does not pass a native-size case. Memory admission is a tracked-bytes budget, not an OS RSS ceiling.
