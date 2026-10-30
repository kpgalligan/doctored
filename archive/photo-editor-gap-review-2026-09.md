# Rasterize Professional Photo Editor Gap Review

Rasterize now has a substantial editing foundation: layered compositing, groups, masks, channels, editable tone adjustments, typography, subject selection, healing, RAW import, and recorded automation. The next stage should emphasize preserving work, preserving image information, and completing common photographic workflows. Adding more menu commands alone will not close the remaining gaps.

The most consequential change from the previous review is the priority of the foundations. **High-bit-depth storage, reliable native saves and recovery, faithful file exchange, and responsive large-document editing now deserve explicit roadmap commitments.** They should no longer sit behind a blanket “everything else is breadth” conclusion. There are also several concrete correctness defects to fix before expanding the feature list.

## 1. Scope and assessment

This assessment covers the repository at commit `3db66097f2acb1bb959fdafd6917d0953330a490`, reviewed on September 9, 2026. Its primary benchmark is professional still-photo editing: RAW development, retouching, masking, compositing, color-correct delivery, and dependable revision. Illustration, page-layout, animation, and prepress capabilities are considered separately because they affect how broadly Rasterize can be used.

The previous [photo-editor-gap-review.md](photo-editor-gap-review.md) is a historical baseline, not an authoritative description of the current app. Its phase-1–8 progress notes correctly record major implementations, while its original §1 still describes color management, groups, styles, and channels as missing. Those statements have been superseded. The [high-leverage roadmap](high-leverage-roadmap.md) and [next-features roadmap](next-features.md) similarly contain completed foundations alongside deferred ideas.[^1]

The findings below distinguish **implemented**, **partial**, **absent**, and **unverified**. “Implemented” means the capability is present in the inspected code; it does not certify its quality. “Absent” means no implementation was found in the relevant model, UI, or tool surface. Proposed acceptance criteria are future release gates, not claims that those tests have already passed. Numerical probes and verification results are documented in §8.

The baseline capabilities assessed here are 16- and 32-bit/channel documents, retained source objects, editable filters, selection-refinement workspaces, RAW local adjustments, and output-profile soft proofing.

### Where Rasterize is already credible

| Workflow | Substantial capabilities already present | Main remaining limitation |
| --- | --- | --- |
| Everyday photo adjustments | Levels, Curves, exposure, white balance, vibrance, hue ranges, color balance, selective color, LUTs, histogram and Info | Imported and stored pixels remain 8-bit; photographic sharpening and denoise are limited |
| Layered compositing | Broad blend-mode set, groups, clipping, masks, channels, locks, multi-select, align/distribute, styles | Source preservation, mask independence, spatial filters, complex-document performance |
| Retouching | Clone, dodge/burn, healing, spot healing, patch, content-aware fill, red-eye | Sampling control, source exclusion, output-layer workflow, latency and advanced retouch tools |
| Cutouts | Geometric selection algebra, wand, subject instances, morphology, Quick Mask, saved channels | Edge refinement, Color Range, selection undo, freehand/edge-aware selection |
| Text and simple graphics | Re-editable text and shapes, paragraph text, typography, symbolic affine transforms | Mixed text styles, paths/vector masks, editable perspective/warp |
| Camera RAW | Actual `CIRAWFilter` development with supported exposure, white-balance, detail/noise and lens controls | One-time RGBA8 import; source and development recipe are not a persistent editable layer |
| Repeatable work | MCP editing, assistant, Actions, Batch, Repeat Last, command palette | Some UI edits cannot replay; pressure/timing and geometry semantics are incomplete |
| Delivery | Native `.rz`, common flat formats, ICC assignment/conversion and embedding, printing | TIFF/PSD fidelity, metadata coverage, PSD export, export presets and proofing |

These strengths are supported by the implementation and its feature tests; they should be retained and extended, not reimplemented under new names.[^7][^8][^9][^10]

### Priorities

**P0** protects saved work or correct output. **P1** closes a frequent professional workflow blocker. **P2** improves efficiency or broadens the addressable work. **P3** serves specialized use cases. These are product priorities, not severity scores or time estimates.

| ID | Priority | Recommendation | Why it comes early |
| --- | --- | --- | --- |
| F01 | P0 | Enforce native writer/reader limits consistently | A successful save can exceed the reader's supported total layer pixels |
| F02 | P0 | Detect all editable state before flat saves | Single-layer documents can contain native-only state without triggering flatten protection |
| F03 | P0 | Establish and verify autosave/recovery | A crash, power loss or forced quit must never lose an edit the user already made |
| F04 | P0 | Correct alpha handling in resize and blur | Existing operations can darken transparent edges |
| F05 | P0/P1 | Repair TIFF round trips and make PSD import losses explicit | Common professional formats can change appearance or lose information |
| F06 | P1 | Introduce high-bit-depth document storage | Floating-point compositing cannot recover source precision lost at import |
| F07 | P1 | Retain RAW and placed-image sources | Development and resampling decisions need to remain revisable |
| F08 | P1 | Complete selections and mask editing | Reliable cutouts and local adjustments depend on them |
| F09 | P1 | Add editable photographic spatial filters | Sharpening, denoise and selective blur are everyday requirements |
| F10 | P0/P1 | Make resource failures explicit and large operations responsive | Rendering can omit over-budget groups; meaningful retouch operations block the UI |
| F11 | P1 | Complete layered retouch sampling and output controls | Sampling an adjusted composite into a lower layer can apply the adjustment twice |
| F12 | P1/P2 | Complete professional export and PSD exchange | Clients and collaborators need predictable deliverables |
| F13 | P1/P2 | Make Actions faithfully reproduce supported edits | Automation becomes less dependable when recordings omit gesture semantics |
| F14 | P2 | Improve document navigation and layer management | Long editing sessions need history, snapshots, search and better views |
| F15 | P2 | Finish gradients, brush libraries, text and paths | Much of the remaining earlier catalog is still outstanding |
| F16 | P2/P3 | Add advanced photography and specialist production workflows | Liquify, alignment/stacking, panoramas, HDR and prepress expand the market |

## 2. Correctness and preservation of work

### F01. A successful native save must always be readable

The native reader rejects documents whose embedded layer images total more than **400,000,000 pixels**. `encode_native` checks layer count, structure, nesting, group budgets and individual PNG size, but does not enforce that total-layer-pixel limit. The comments say the writer enforces the reader's caps; this particular invariant is missing.[^11]

A bounded FFI probe created a 626 × 626 image, duplicated its shared pixels into 1,024 valid layers, and successfully saved a version-8 `.rz`. An independent parser counted **401,281,024 embedded layer pixels** in the 9,474,107-byte file. That exceeds the reader's explicit rejection threshold. The probe did not fully decode the file, avoiding roughly 1.6 GB of layer allocations; the rejection follows directly from the reader's checked sum. This is not just a hypothetical enormous-canvas case: seventeen full-size 24 MP layers also exceed 400 MP.

Add a shared document-capability validator used before saving and at operations that can exceed supported limits. Preserve the existing destination when validation fails. Longer term, separate protection against hostile files from the supported capacity of legitimate professional documents; a fixed aggregate cap is a limited substitute for resource-aware decoding.

**Acceptance:** Every document the writer accepts reopens in the same build. Exercise boundary values for total pixels, layers, masks, channels, nesting and styles. Any refusal arrives before replacing the previous file and explains the constraint. Include a practical 24/48 MP multilayer document in this suite.

### F02. Flat-save protection must inspect document state, not only layer count

`ImageDocument.writableTypes` and the final “Save Flattened?” guard use `doc.layerCount > 1`. However, one layer can have a mask, styles, editable text/source metadata, locks, saved alpha channels or document guides. Flat `write` serializes the composite and supported file metadata; it cannot retain those editing structures.[^12]

Consequently, the guard misses a one-layer PNG that acquires a saved selection channel or guide. The image can save successfully while its editable state will disappear on reopening. Multi-layer protection already exists and should be credited; the defect is the narrower preservation predicate.

Create one capability comparison between document state and destination format. Use it to choose the native default and to explain losses before a Save that establishes the document's saved state. Explicit flat Export/Save Copy remains useful, but should report the relevant output limitations consistently.

**Acceptance:** A one-layer image with each native-only feature either saves natively or obtains an explicit loss decision. Saving, closing and reopening must preserve what the UI represented as saved. Include single-layer and multi-layer cases, and keep Export from changing the master document's dirty state.

### F03. Recovery and history need an explicit product contract

The app sets `autosavesInPlace` to `false` and fixes undo at **24 levels**. No dedicated recovery/history implementation or version-browsing UI was found. This does not establish that every inherited AppKit recovery mechanism is absent; it does establish that native autosave, untitled-document recovery and interrupted-session recovery are not demonstrated product guarantees.[^12]

Atomic replacement already protects against ordinary failed writes: native and flat saves use a temporary file followed by rename. Retain that foundation. Add verified `.rz` autosave/recovery, recovery for untitled work, a History panel, named snapshots and a configurable memory budget for undo. An immutable handle is cheap to retain only while its underlying pixels are shared; successive pixel edits keep old buffers alive.[^13]

**Acceptance:** Terminate a controlled test instance with a dirty `.rz` and an untitled document, then recover pixels, masks, channels, guides and editable descriptions to a documented interval. Verify failed saves, low disk space, moved files and older native versions. History should support a long retouch session within measured memory limits rather than simply raising 24 to another arbitrary count.

### F04. Resize and blur need consistent alpha-correct filtering

The Free Transform path already interpolates premultiplied pixels. General image resize and Gaussian blur instead call `imageops` on straight RGBA8 channels. That difference produces incorrect edge colors when opaque content meets transparent black.[^14]

A tiny FFI probe resized `[255,0,0,255]` and `[0,0,0,0]` from 2 × 1 to 1 × 1 with triangle interpolation. The result was **`[128,0,0,128]`**, whereas alpha-correct filtering gives approximately **`[255,0,0,128]`** in straight RGBA. The returned red is multiplied by alpha again during compositing, creating a dark fringe. A blur probe showed the same pattern, producing `[179,0,0,179]` and `[76,0,0,76]`.

Unify alpha semantics across document Image Size, flat-image resize, blur, thumbnails and future spatial filters. Treat linear-light interpolation as a separate color decision; fixing premultiplication does not require changing every existing blend mode.

**Acceptance:** Independently computed edge fixtures pass on black, white and colored backgrounds. Verify transparent pixels with arbitrary hidden RGB, semitransparent edges and each sampler. Document-level resize must agree with the corrected filtering contract.

### F05. TIFF and PSD need explicit fidelity boundaries

**TIFF is currently an asymmetric round trip.** The encoder embeds ICC, but the current TIFF decode path does not recover it and assumes sRGB on reopen. Export also does not write the document's PPI: it writes the underlying encoder's default X/YResolution of 1/1 with ResolutionUnit “none.” Some readers present that as 1 dpi. These limitations are documented in code and partly exposed by the export UI, but they still block dependable wide-gamut TIFF interchange.[^15]

**PSD import is structurally useful but not a faithful rendering contract.** Raster layers, names, opacity, blend modes, clipping and group membership are imported. Layer masks, layer effects, editable source descriptions, alpha channels, ICC and resolution are not preserved by this path. Groups are constructed visible at full opacity in pass-through mode, and decoded layer pixels are intersected with the canvas, losing off-canvas content. A failed layer decode can fall back to the stored composite; unsupported-but-decodable content does not automatically trigger that fallback.[^16]

Fix TIFF profile, density and metadata handling through a capable decoder/encoder path. For PSD, first add a compatibility report with an explicit appearance-preserving flattened option, then improve masks, group properties, profiles, channels and other supported structures. Do not equate “PSD opened without an error” with “the Photoshop document was preserved.”

**Acceptance:** A tagged wide-gamut TIFF at 300 PPI reopens with the same profile, density and decoded colors. A PSD corpus covers masks, isolated/hidden/translucent groups, effects, adjustments, text, clipping and off-canvas pixels; each case either retains the supported appearance/editability or reports the precise loss. Compare against an independently rendered reference.

## 3. Foundations for professional photographic editing

### F06. High-bit-depth pixels and explicit color processing

The document stores `Arc<RgbaImage>`: straight RGBA8 pixels. Flat imports call `to_rgba8`; RAW development creates `CIFormat.RGBA8`. An f32 compositor and adjustment chain reduce repeated rounding while evaluating a stack, but cannot restore gradients or highlight information already quantized or clipped at import, painting, destructive edits or output.[^17]

Prioritize a coherent **16-bit/channel or suitably precise floating-point document mode**, including import, editing, masks where needed, compositing, serialization and export. Select the representation with an explicit precision/memory/performance comparison. A float preview buffer alone is not high-bit-depth document support. Add deliberate conversion to 8-bit at delivery, with appropriate dithering and clear document-mode reporting.

Make blending/compositing space an explicit, persisted choice when introducing linear-light support. Preserve existing files' rendering semantics. The older review's suggestion of a single linear-light toggle can be a useful step, but implementation must account for document profiles, authored colors, masks, effects and export rather than assuming every document is sRGB.

Color management itself is already real: assign and convert are separate, the display is profile-tagged, and compatible RGB matrix/TRC profiles can be transformed. Remaining gaps include broader ICC transform support, rendering intents and black-point compensation, soft proofing, gamut warnings and an appearance-preserving export-conversion workflow. Lab values in Info are a readout, not a Lab document mode; selective-color CMYK-style controls are not CMYK editing.[^18]

There is also a narrower processing inconsistency: Exposure and White Balance explicitly use the sRGB transfer function, and White Balance uses sRGB primaries, regardless of the document profile. White Balance is therefore not calibrated to P3 primaries; P3 shares sRGB's transfer curve, whereas spaces such as Adobe RGB also expose the transfer assumption in both operations. Matching destructive and adjustment-layer results does not resolve this color-context problem. Pass color context into a shared kernel and retain legacy rendering semantics where needed. Verify equivalent color patches across working profiles rather than using UI/MCP parity as the only oracle.[^34]

**Acceptance:** A high-depth ramp survives import, stacked edits, save/reopen and high-depth export without being reduced to 256 channel values. Highlight and low-opacity fixtures exercise the complete pipeline. Proof previews are compared with a reference CMS, and opening existing `.rz` files does not silently change their composite appearance.

### F07. Retain source images and RAW development recipes

RAW develop has shipped with meaningful controls; it should not be listed as missing. Its source explicitly defines it as a one-time development on open, with no RAW layer or re-develop command. Settings retained on `ImageDocument` support Revert during the session, but `.rz` stores the resulting core document rather than a RAW source and recipe. The current selection of the latest decoder and sparse defaults also means reproducibility across decoder changes is not a guaranteed recipe format.[^19]

Build a shared source-backed layer mechanism for RAW and ordinary placed images: retain original source, transform, rendered fallback and versioned parameters. Support embedded and linked sources, Replace Contents, relinking, and clear missing-source handling. Live Photo layers already demonstrate re-renderable descriptions, but their media paths are absolute and the media is not packaged into `.rz`; portability needs the same asset policy.[^20]

For RAW, add re-develop after compositing, high-depth output, saved presets and synchronized settings across files. Improve the Develop workspace with a 100% detail view, histogram/clipping feedback and before/after comparison; its current fixed-size `NSImageView` preview is inadequate for judging noise and sharpening at capture resolution. Local RAW masks, camera profiles and deeper lens controls are subsequent enhancements; the eventual local masks are brush, gradient and range-based adjustment.

A backdrop filter layer and a source-backed layer solve different problems. A backdrop filter modifies everything beneath it; it does not retain one placed image's original resolution or let a photographer revisit RAW white balance. Both concepts are useful and should have distinct scopes in the model.

**Acceptance:** Develop a RAW, add retouch and adjustment layers, save/reopen, then change development settings without rebuilding the composite. Downscale and enlarge a placed source without cumulative resampling loss. Copy the native document to another location and verify embedded assets, linked-asset diagnostics and fallback rendering.

### F08. Selections and masks need revision, precision and independence

Selections already have a good coverage-mask representation, combine modes, morphology, saved channels, luminosity masks, subject instances and Quick Mask. The gaps are the tools needed to finish and revise a difficult mask.[^8]

1. **Undo and Reselect.** Selection loading is explicitly view state without undo, and Quick Mask stroke completion creates no undo step. Add reversible selection transactions and Reselect while keeping selection changes separate from file-dirty counting where appropriate. A mistaken masking stroke should not require undoing an unrelated pixel edit.[^21]
2. **Color Range and selection transforms.** Add sampled-color fuzziness, tonal ranges, multiple samples, selection-only translate/scale/rotate and efficient fill/stroke commands. Existing luminosity channels are useful building blocks, not a substitute for the complete interactive workflow.
3. **Edge refinement.** Provide refinement brushing, edge-aware radius, smooth/contrast/feather/shift controls, color decontamination and multiple inspection backgrounds. The presence of Select Subject does not establish quality on hair, fur, translucent materials or low-contrast product edges.
4. **Selection breadth.** The current lasso is polygonal. Add freehand lasso and Quick Selection; magnetic lasso, specialized people/body-part selection, sky selection and richer object targeting follow according to observed demand.
5. **Independent masks.** Add mask-content unlinking, mask-only transforms, nondestructive density/feather and direct mask-target filtering/filling. Layer-to-layer links already exist; they are a different feature. Vector masks depend on a path subsystem.[^22]

Mask targeting also needs a correctness pass before adding controls: with the mask selected, UI filters, destructive adjustments, bucket fill and gradient currently operate on the layer's pixels. The corresponding agent operations refuse a mask target. The code documents this behavior, but it is surprising in a mask-editing workflow. Commands should consistently affect the indicated target or explicitly refuse it.[^22]

**Acceptance:** Mask a portrait, fur, glass and a hard-edged product; inspect at 100% against light, dark and middle-gray backgrounds. Undo refinement and Quick Mask strokes. Transform only a selection or mask without changing source pixels, and round-trip mask position/link/density through `.rz` and MCP.

### F09. Add photographic spatial filters with persistent parameters

The adjustment suite is broad, but the spatial-filter surface remains basic: Gaussian Blur, single-parameter Sharpen, Pixelate, Add Noise, Edge Detect and Emboss. Sharpen already uses an unsharp-mask algorithm: its parameter named `amount` is passed as blur sigma, with threshold fixed at zero. The gap is a correctly named, independently controllable radius/amount/threshold workflow and more advanced sharpening, not the complete absence of unsharp mathematics. RAW detail controls help during import, but do not cover JPEG/TIFF retouching or final-output sharpening.[^23]

Prioritize Unsharp Mask, edge-aware sharpening with noise control, luminance/chroma denoise, High Pass, median and Dust & Scratches. Then add motion/radial/zoom blur, surface/bilateral blur, lens/depth blur, vignette, lens distortion and chromatic-aberration correction. Separate artificial noise generation from denoising and film-grain emulation; separate color matching/Equalize from existing Auto Color.

Expose these as editable spatial filters with parameter recall, enable/reorder, masks, blending and presets. The earlier “filter layer” proposal is a useful scoped first implementation if its backdrop semantics are explicit. A per-source filter stack provides more control and shares dependencies with F07. The existing Shadows/Highlights implementation already does spatial work, so this is an extension of an established capability, not the first area operation in the app.[^23]

Destructive filters and adjustments also need to honor pixel selections. `applyToActiveLayer` filters the entire layer and replaces its pixels without blending through the selection; Auto Tone/Contrast/Color explicitly use the same whole-layer policy. Selecting a region and invoking Blur/Sharpen therefore does not confine the edit to that region. Adjustment-layer creation already supports capturing a selection as its mask, which is a separate implemented path. Make operation scope consistent and test soft coverage, not just rectangular bounds.[^35]

**Acceptance:** Edit filter parameters after save/reopen without repeated degradation. Check halos, transparency, noise/detail retention and selection/mask boundaries with independent fixtures and representative photographs. Evaluate capture sharpening and output sharpening separately.

### F10. Responsiveness and resource limits are product capabilities

Most image edits return new immutable document handles, but the UI refreshes the flattened projection synchronously. Painting uses a canvas-sized overlay and can clone the target layer. Full-canvas f32 compositing buffers, group buffers, masks, projections and undo history compound memory use. At 48 MP, one RGBA8 image is about **192 MB**, one four-channel f32 buffer about **768 MB**, and one byte mask about **48 MB**; these are arithmetic allocation sizes, not measured process peaks.[^24]

The healing UI explicitly documents multi-second blocking commits. Content-Aware Fill's final computation runs inside `whileBusy` on the main thread, and preview cancellation waits for in-flight work. These are known responsiveness limits even though the bounded algorithms and preview machinery are substantial engineering work.[^25]

Introduce cancellable worker execution over immutable snapshots, progress reporting, revision/target checks before commit and explicit memory estimates. Build region-based invalidation and reuse incrementally; evaluate tiles, multiresolution previews and scratch-backed storage against representative large documents. GPU acceleration may help, but should follow measured bottlenecks and preserve reproducible output.

Also fix resource-limit behavior at the rendering boundary. The group renderer returns `None` when its simultaneous buffer budget exceeds 400 MP, and callers can omit that contribution without a render error. Full-extent nested isolated groups, or pass-through groups gated by masks/reduced opacity, accumulate this charge; five 100 MP extents or nine 45 MP extents exceed it within the allowed nesting depth. Ordinary ungated pass-through groups and sequential sibling groups do not accumulate that cost. Native/edit preflights exist but do not make this runtime omission an acceptable successful render. This finding follows from source paths and their limits; no large-memory missing-group render was executed.[^24]

**Acceptance:** Publish measured open/save, brush latency, transform preview, heal/fill duration, cancellation latency and peak memory on specified hardware for 24, 48 and near-100 MP documents. Use realistic layer/group/mask counts. Keep the UI responsive during long operations; cancellation changes neither source nor undo history. Never silently omit content to satisfy a budget.

### F11. Complete the nondestructive retouch workflow

Clone Stamp always samples the flattened composite. Healing offers current-layer or all-layer sampling, and the inpainting tools have sampling options, but current-and-below and ignore-adjustment-layer policies are missing. With an adjustment above the destination, painting adjusted composite pixels into a lower retouch layer can bake the adjustment into those pixels and apply it again.[^25]

Add consistent current/current-and-below/all sampling, an option to ignore adjustment layers, source preview with offset/rotation/scale/flip, and convenient output to a new retouch layer. Empty-layer retouching must have well-defined destination and sampling behavior. Content-Aware Fill should allow painting inclusion/exclusion in the source region rather than relying only on a sampling ring; add output destination, variation comparison and clearer final-result preview.

Continue with frequency separation, Smudge/Blur/Sharpen/Sponge brushes and Liquify. Frequency separation needs tested reconstruction and high-depth arithmetic; it is not just an arbitrary blur-plus-blend preset. Liquify should retain a displacement field with reconstruct/freeze controls and undo, rather than becoming another irreversible rasterization step.

**Acceptance:** Retouch below a strong adjustment without double application. Reproduce a clone/heal action with the same source policy. Remove an object near an unwanted neighboring texture using source exclusion. Validate retouch quality on skin, edges and repetitive backgrounds; do not infer quality from algorithm names alone.

## 4. Delivery, automation and daily workflow

### F12. Treat export and exchange as complete workflows

Current Export offers format, JPEG quality, profile embedding and metadata stripping. Batch exists and should not be counted as missing. What is absent is an export workspace with output dimensions, target-profile conversion, encoded preview/file size, output sharpening, presets and reusable naming/destination rules.[^26]

**Metadata:** JPEG and PNG are the only source containers whose EXIF/XMP/IPTC packets the current core walks. RAW, HEIC, Live Photos, TIFF, WebP and PSD can arrive without their capture packets. Add format-aware extraction and writing, copyright/caption/keyword editing and metadata presets. Preserve useful capture data while making removal of location/private fields explicit. Existing dropped-packet reporting is a good foundation; it should not be confused with preservation.[^15]

**PSD/PSB:** Deliver layered PSD with a documented support matrix, masks, groups, opacity, clipping, profiles and supported blend behavior. Validate before promising editable interchange. Rasterize's own payload stored in private XMP would help Rasterize re-import it, but would **not** make that payload editable Photoshop text, shapes or Smart Objects. PSD file limits and eventual PSB support must be explicit parts of the interoperability design, not an undocumented extension of flat export.[^16]

**Other delivery:** Add HEIC export, then evaluate AVIF/JPEG XL based on deployment support and demand. Add layer/group export, Quick Export, contact sheets, PDF/SVG raster import at selected dimensions and dedicated PDF export. The existing macOS Print flow already provides a route to a flattened print PDF; it is not a layered PDF exchange implementation. Multi-page TIFF/multi-frame GIF import and animated export are lower-priority specialist features.

For web exports, converting a copy to sRGB is materially different from unchecking “embed profile” on wide-gamut pixels. The export workflow should make that distinction easy to execute. For printing, add soft proofing, gamut warnings, output-profile selection, rendering intent and black-point compensation. Basic File > Print and PPI controls are implemented, while this production control is not.[^18]

**Acceptance:** Produce a correctly sized, tagged web JPEG and a 300-PPI print TIFF without altering the master. Preview the actual encoded result and report final size. Exchange supported layered PSD content with Photoshop, with a visible compatibility report for everything else. Validate metadata and orientation independently after export.

### F13. Actions and agent parity need semantic completeness

Actions, recording, replay, Batch, Repeat Last and the command palette are real implemented features. The action model also explicitly represents commands it cannot replay: Paste, Quick Mask, some layer-selection/movement operations and multi-layer transforms. Stroke recording omits per-point pressure and airbrush hold timing. Geometry is deliberately stored in absolute pixels with size-mismatch warnings; it is not generally scale-independent automation.[^27]

Close recording and MCP gaps before advertising complete edit parity. Record a canonical stroke representation that preserves the actual painted result or the pressure/timing needed to reproduce it. Package external/clipboard assets, add stable layer identity and optional anchor-relative/proportional geometry, and support named action parameters. Keep size-remapping choices explicit: an absolute brush width and a crop proportional to the image require different semantics.

Color arguments also need an explicit color-space option. The agent currently accepts authored colors as sRGB and correctly reports when a sampled document color has no exact sRGB paint-back value. This prevents exact agent sample-to-paint round trips for out-of-sRGB colors, even though the UI eyedropper can retain a document-space color. Preserve the existing warning while adding a precise document-space color representation.[^37]

Retain the existing strength that an action replay produces a coherent undo step. Extend validation to unsupported steps, partial failures, cancellation, missing assets and output collisions. Conditional actions, loops and more capable authoring are worthwhile later; they are less important than replay fidelity.

The assistant should continue to operate through the same edit model. Show proposed multi-step edits clearly, preserve a recoverable checkpoint and report partial results accurately. Core editing and saved work should remain fully usable without a provider connection or API key. MCP is an opt-in local automation surface; production hardening should include client authorization and deliberate document/file access scope without weakening the ordinary offline workflow.[^28]

**Acceptance:** Replay a pressure stroke, airbrush hold, Quick Mask edit, multi-layer transform and placed asset on identical input with matching output. Run an explicitly parameterized action across different image sizes. A canceled or partially failed run reports completed changes and offers a dependable undo.

### F14. Long sessions need better navigation and organization

Groups, locks, multi-select, linking, alignment, rulers, guides, grid and snapping are implemented. The remaining organization gaps include layer color labels, kind filtering, search, solo/isolation convenience, layer comps and named snapshots. History belongs alongside these features, backed by the resource policy in F03/F10.[^9]

Add a proper **New Document** flow: dimensions, physical units/PPI, profile, background/transparent fill and presets. The app currently intentionally starts from an opened image or New from Clipboard. Add a Navigator, view-only canvas rotation, a second synchronized view of the same document, customizable shortcuts and saved workspaces. These are especially useful when alternating between pixel-level retouch and full composition.[^29]

Existing native controls, tooltips and accessibility descriptions provide some accessibility support. Keyboard navigation and VoiceOver through custom canvas controls, layer rows, gradient/curve editors and sheets still need a structured usability assessment; source inspection alone cannot establish that they are usable. Test small displays, long layer names, high UI scaling and tablet input as part of the same workflow review.

**Acceptance:** Start a transparent print-sized document without an external seed image; inspect detail and composition simultaneously; find a layer in a large stack; restore a workspace and a named document snapshot; complete core workflows using the keyboard.

### F15. Finish partially implemented creative tools

| Area | Current implementation | Recommended completion |
| --- | --- | --- |
| Gradients | Multi-stop Gradient Map editor; styles can store richer gradients; paint tool is two-endpoint linear/radial | Share the editor across paint and styles; expose interior stops, midpoint/opacity controls, gradient presets, remaining geometries and dithering |
| Fill layers and patterns | Raster fill/gradient; color and gradient overlays | Editable solid/gradient/pattern fill layers; pattern library, Define Pattern and Pattern Overlay |
| Brushes | Analytic round/elliptical tips, built-in presets, hardness/flow/spacing, smoothing, airbrush, pressure-to-size | User presets, bitmap tips, pressure curves and opacity/flow dynamics, tilt/rotation, scattering, texture and import/export |
| Paint convenience | System color panel and basic color handling | Swatches/recent colors, complete foreground/background shortcuts, brush-size outline cursor; Pencil/Background Eraser/Magic Eraser where useful |
| Text | Point/paragraph text and substantial whole-layer typography; symbolic affine transform; some missing-font diagnostics | Attributed runs, full font-face selection, variable-font axes, paragraph styles/justification and document-open font replacement/substitution workflow |
| Paths/shapes | Editable rectangle, ellipse and line | Pen/Bézier paths, node editing, boolean operations, stroke/fill, path-to-selection and vector masks; text on path later |
| Styles | Nine effects with sheet checkboxes, Fill opacity, Blend If, global light, copy/paste/scale | Blend If on adjustments, expandable layer-panel effect rows/toggles, gradient glows, presets, repeated instances, Pattern Overlay and knockout where justified |
| Geometry | Free Transform, command-corner homography, crop/manual straighten, canvas resize | Arbitrary whole-document rotation, auto-straighten, perspective crop, named constrained transform modes, Trim and Reveal All |

The gradient gap is particularly easy to misreport. The model/editor exists for Gradient Map, but the Gradient Overlay and gradient Stroke panes still primarily expose end colors, leaving interior stops created through the agent inaccessible there. Independent opacity stops and per-span midpoints are not in the current gradient model. This is partial implementation across surfaces, not a missing editor everywhere.[^30]

Likewise, layer styles are substantial, but Blend If lives inside a style object that is refused for adjustment layers. The previous review's example of limiting a warm adjustment by underlying highlights therefore still requires a mask workaround. Editable typography is present, but the text payload and on-canvas editor apply one style to the entire string.[^31]

Crop already retains off-canvas **layer pixels** by changing offsets and the canvas window. Do not implement a second “nondestructive crop” under the assumption those pixels are currently deleted. The useful gaps are convenient recropping/Reveal All, and a deliberate policy for canvas-sized alpha channels and group masks, which are actually cropped. Free Transform is also not missing; remaining geometry work concerns named modes, full source preservation under projective transforms and new deformation tools.[^32]

**Acceptance:** Author the same multistop gradient through UI and MCP; edit a mixed-style text layer after reopening; move a vector-mask node without rasterizing it; transform a source repeatedly without cumulative quality loss. Each new persisted feature must survive native save/reopen and expose equivalent document edits through the agent.

### F16. Advanced photography and specialist workflows

The following are meaningful additional gaps beyond the prior implementation phases. They should be prioritized by the work Rasterize intends to support, rather than treated as one undifferentiated parity checklist.

| Workflow | Missing or incomplete capability | Suggested priority |
| --- | --- | --- |
| Portrait/fashion retouch | Liquify with reconstruct/freeze, frequency separation helper, deeper clone/heal controls | P1/P2 |
| Product and macro | Image registration/Auto-Align, focus-stack blending, repeatable clipping paths | P2 |
| Architecture and landscape | Lens/CA correction beyond RAW's current toggle, perspective crop/correction, panorama stitch/blend | P2 |
| Exposure blending | HDR merge, deghosting, high-dynamic-range tone mapping and display/export | P2 after high-depth foundation |
| Low-light and repetitive capture | Multi-frame denoise, registration and stack statistics | P2/P3 |
| Compositing/layout | Reusable placed assets, linked/embedded contents, comps, templates, artboards if demanded | P2/P3 |
| Commercial print/prepress | CMYK/Lab document modes, soft proofing, spot channels, separations and specialized PDF output | Proofing P1/P2; full prepress P3 until an explicit market commitment |
| Illustration | Rich brush dynamics, mixer/smudge tools, symmetry and advanced vector/text | P2/P3 |
| Animation/motion | Multi-frame editing and export; a Live Photo frame picker is not a timeline | P3 |
| Ecosystem | Packaged presets/assets, scripted extensions or a plug-in API | P2/P3; expand the existing automation surface first |
| Generative assistance | Optional removal/expansion and subject-aware editing beyond classical inpainting | P2/P3; prioritize reliable local editing and explicit source/result retention |

Manual Align/Distribute already exists and does not perform photographic image registration. Content-Aware Fill is classical inpainting and does not by itself supply generative fill, scene extension, or semantic reconstruction. Those distinctions matter when evaluating a workflow with similar-sounding commands.[^9][^10]

For adoption beyond a developer workstation, add a signed/notarized distribution process, updates with recoverable rollback, compatibility documentation, and user-oriented help. The checked-in project currently uses ad-hoc signing, while the public documentation site still contains starter example pages. The README is extensive engineering documentation; it does not replace tutorials for masking, RAW, printing, recovery and delivery.[^33]

## 5. Reconciliation of the previous gap review

The table below tracks the old review's material requested features and expanded catalog. An implemented phase can still have residual product limitations; “partial” below does not negate the work recorded in its progress notes.

### Requested foundations and photo correctness

| Previous item | Current status | Remaining work / finding |
| --- | --- | --- |
| §2.1 Nine layer effects, Fill, Blend If, global light, copy/paste/scale | Implemented with limits | Gradient glows, repeated effects, adjustment-layer Blend If, richer layer-panel presentation; F15 |
| §2.1 Knockout and blend-interior-as-group | Deferred / absent | Group support now exists, but these blending semantics still require implementation; F15 |
| §2.1 Expanded effect rows with individual visibility | Partial | Effect checkboxes exist in the sheet; expandable rows under a layer do not; F15 |
| §2.1 PSD styles import/export | Absent | `lfx2` not imported; no PSD writer; F05/F12 |
| §2.2 Symbolic text/shape/Live Photo affine transforms | Implemented | True projective transforms and crop straighten still rasterize descriptions; F07/F15 |
| §2.2 Typography additions | Implemented with limits | Whole-layer styling, not mixed attributed runs; text editing remains axis-aligned; F15 |
| §2.3 Named alpha channels, Channels panel, save/load selections | Implemented | Selection/Quick Mask undo, richer mask controls; F08 |
| §2.3 Plane/channel painting, filtering, fills, Apply Image/Calculations/luminosity masks | Implemented with target gaps | Layer-mask filter/fill/gradient scope differs from the selected target; F08/F09 |
| §2.3 Quick Mask as a temporary channel | Partial | User feature exists but retains a separate canvas buffer; undo/replay/consolidation outstanding; F08/F13 |
| §2.3 iPhone depth/portrait/semantic mattes | Implemented; quality unverified here | Real matte-bearing sample coverage remains an acceptance requirement |
| §3A Read/convert ICC, tag display, assign/convert | Implemented for supported RGB profiles | TIFF/PSD ingest gaps, LUT-profile conversion, calibrated adjustment context, proofing; F05/F06 |
| §3A Embed ICC | Implemented for PNG/JPEG/TIFF/WebP | Correct TIFF reopen; intentional handling for untaggable output; F05/F12 |
| §3A Preserve EXIF/XMP/IPTC; normalize orientation; strip toggle | Partial | JPEG/PNG input coverage; broader containers, packet forms and metadata editing; F12 |
| §3A PPI, physical size, resample-off and printing | Implemented with file limits | TIFF density, proofing/output control, real printing validation; F05/F12 |
| §3A Camera RAW develop | Implemented as one-time import | Retained source/recipe, redevelop, precision, detail preview, presets/local work; F07 |
| §3A HEIC export; AVIF/JPEG XL open/export | Absent from supported app surface | HEIC import already exists; add actual declared, tested delivery paths; F12 |
| §3A Histogram with channels/clipping indicators | Implemented | A histogram indicator is not a canvas clipping overlay; add overlays/persistent samplers if useful |
| §3A Info position/RGB/HSB/Lab/selection readout | Implemented | Persistent sampling points and specialized profile limitations remain possible improvements |
| §3A Linear-light document option | Deferred / absent | `Linear Light` blend mode is not linear-light compositing; F06 |
| §3A High-depth pixels | Deferred / absent | Promote from indefinite deferral to an explicit foundation project; F06 |
| §3B Exposure, Vibrance, hue ranges/Colorize, Color Balance, B&W/tint, Photo Filter | Implemented | Selection-aware destructive edits, reusable presets and profile-aware calibrated operations; F06/F09 |
| §3B Channel Mixer, Selective Color, Shadows/Highlights, White Balance, Gradient Map, LUT | Implemented | Same cross-cutting limits; richer shared gradient editing; F06/F09/F15 |
| §3B Auto Tone/Contrast/Color | Implemented | Whole-layer scope; plane/channel Actions parity; F09/F13 |
| §3B Match Color / Equalize | Deferred / absent | Still outstanding; F09 |
| §0 adjustment follow-ons: general preset browser, targeted on-canvas adjustment | Absent as a shared workflow | Some built-in per-adjustment presets exist; general user library and targeting do not |

Evidence: adjustment, selection, style, color and native models and their UI/agent routes.[^7][^8][^15][^17][^19][^21][^22][^30][^31]

### Retouching, selection, paint and layers

| Previous item | Current status | Remaining work / finding |
| --- | --- | --- |
| §3C Healing and Spot Healing | Implemented | Real-photo quality, preview/commit consistency, sampling, responsiveness; F10/F11 |
| §0 healing follow-ons: Replace, Proximity Match, Create Texture | Absent | Current spot healing is content-aware; add modes only where they improve demonstrated cases |
| §3C Patch | Implemented | Source/destination modes already exist; advanced sampling and source controls remain; F11 |
| §3C Content-Aware Fill | Implemented with caps | Source-exclusion workspace, output-layer controls, cancellation and larger-work strategy; F10/F11 |
| §3C Red-eye and automatic detection | Implemented; detector quality unverified here | Validate real faces/eyes, not only explicit injected eye regions |
| §3C Smudge/Blur/Sharpen/Sponge brushes | Absent | Add shared per-dab processing and independent photographic quality checks; F11/F15 |
| §3C Frequency separation | Absent | Needs appropriate High Pass/reconstruction and precision; F11 |
| §3C Liquify | Absent | Displacement field, reconstruct/freeze and persistent source; F11/F16 |
| §3D Freehand/magnetic lasso | Absent | Existing lasso is polygonal; F08 |
| §3D Quick Selection | Absent | Subject-instance clicking is not an edge-aware region-growing brush; F08 |
| §3D Color Range | Absent | Wand and luminosity channels are related primitives; F08 |
| §3D Refine Edge | Absent | Morphology/feather already exist, but edge-aware matting/decontamination does not; F08 |
| §3D Transform Selection | Absent | Transform current coverage independently of layer pixels; F08 |
| §3D Specialized people instances | Partial use case | Generic foreground instances are selectable individually; no specialized people/body-part model; F08 |
| §3D Stroke/Fill Selection dialogs | Partial building blocks | Existing raster fill/gradient honor selections; dedicated commands and pattern/multistop options remain; F08/F15 |
| §3D Reselect | Absent | Add alongside selection undo; F08 |
| §3D Load/Save Selection | Implemented | No need to re-add; strengthen undo/recovery and `.rz` preservation |
| §3D Grow by color | Absent | Morphological Grow and global wand do not implement this named workflow; F08 |
| §3E Multistop gradient editor and geometries/presets | Partial | Gradient Map editor exists; canvas and style UI remain incomplete; F15 |
| §3E Patterns, Pattern Overlay/library/Define Pattern | Absent | Add shared pattern assets and persistent fill semantics; F15 |
| §3E Sampled tips/scattering/dual brush/texture/color dynamics | Absent | Existing analytic brush tips, pressure-size and built-in presets are real; F15 |
| §3E Pencil/Background Eraser/Magic Eraser | Absent | Separate tools and semantics remain; F15 |
| §3E Swatches/recent colors/HSB-Lab picker/X-D/fill shortcuts | Partial | Standard macOS color panel and HSB, foreground/background wells and Lab readout exist; app swatch library, Lab picker and requested shortcuts remain; F15 |
| §3F Groups, opacity/blend/pass-through/masks | Implemented with compositing limits | Render-budget omission and pass-through clipping semantics; F10 and note below |
| §3F Transparency/pixel/position/all locks | Implemented | PSD lock import absent; F05 |
| §3F Multi-select and set move/transform/delete/group/merge | Implemented UI/core | Some gesture Actions/MCP parity missing; F13 |
| §3F Link/Align/Distribute | Implemented | Photographic registration is a different, absent capability; F16 |
| §3F Via Copy/Cut, Merge/Stamp Visible, Arrange | Implemented | Preserve native-only state even after flattening to one layer; F02 |
| §3F Move auto-select | Implemented | Shift-add selection recording gap; F13 |
| §3F Layer labels/search/filter | Absent | F14 |
| §3F Linked image layers/Replace Contents | Absent | Generalize sources beyond existing described-layer types; F07 |
| §3F Layer comps/snapshots | Absent | Define selective comp state versus a full document snapshot; F03/F14 |
| §3F History and memory-budget undo | Absent | Current undo remains 24 levels; F03/F10 |

One additional group limitation is explicitly documented: making a pass-through group a clipping base forces isolation, so an adjustment inside it can stop affecting the backdrop when a sibling becomes clipped. The behavior is test-pinned and explained in the app, but remains a fidelity limitation to revisit; group availability alone does not complete all blending semantics.[^36]

Evidence: current tool enum, canvas/controller routes, brush/gradient models, group compositor and layer operations.[^8][^9][^10][^21][^22][^25][^27][^30][^31]

### Filters, geometry, files and automation

| Previous item | Current status | Remaining work / finding |
| --- | --- | --- |
| §3G Unsharp Mask/Smart Sharpen/High Pass | Implemented for Unsharp Mask and High Pass | Phase 9B ships both as live retained filters and as selection-aware destructive twins, with independent radius/amount/threshold. Smart Sharpen's noise/motion controls remain; F09 |
| §3G Motion/radial/zoom/box/lens blur | Absent | Gaussian Blur exists; F09 |
| §3G Surface/bilateral/median/reduce noise | Implemented for Reduce Noise and a median | Phase 9C ships an edge-aware luminance/chroma denoise and a threshold-gated disc median as live retained filters and as selection-aware destructive twins, on ordinary image layers with no RAW recipe. Surface/bilateral blur as a separate creative operator remains; F09 |
| §3G Vignette/lens correction/CA | Absent as ordinary-layer filters | RAW lens toggle is partial coverage of the broader workflow; F09/F16 |
| §3G Film grain/clouds/render/offset/dust-and-scratches | Implemented for Dust & Scratches | Phase 9C ships it live and destructively, threshold0 being a plain median. Film grain, clouds, render and offset remain; Add Noise is not the complete film-grain workflow; F09 |
| §3G Editable spatial filter layers/cache | Implemented for retained sources | Phase 9A/9B give a retained binding an ordered, cached, maskable filter stack with presets; a general backdrop filter layer over arbitrary raster layers remains; F09/F10 |
| §3H Arbitrary whole-canvas rotation | Absent as a dedicated command | Quarter-turn/flip, arbitrary layer transforms and manual crop straighten already exist; F15 |
| §3H Automatic horizon straighten | Absent | F15 |
| §3H Perspective Crop | Absent | Layer homography is not a crop workflow; F15 |
| §3H Named skew/distort/perspective modes | Partial | Command-corner homography/MCP distortion exists; named constraints/UI remain; F15 |
| §3H Mesh Warp | Absent | Shares deformation/source dependencies with Liquify; F11/F15 |
| §3H Content-Aware Scale | Absent | Seam carving/protect mask remains a separate feature; P2/P3 |
| §3H Trim/Reveal All | Absent | Ordinary crop retains layer pixels already; F15 |
| §3H Guides/rulers/grid/snapping | Implemented | Per-document guides persist; app-wide view preferences intentionally lack document-scoped MCP edits |
| §3I PSD export | Absent | Supported layered handoff and compatibility reporting; F12 |
| §3I Export for Web/Quick PNG | Partial | Basic Export is present; resize/preview/size targets/presets and Quick Export remain; F12 |
| §3I Export layers/contact sheet | Absent | Batch works on documents, not this extraction/layout workflow; F12 |
| §3I Batch through an action | Implemented | Top-level folder traversal and output reporting exist; recipe fidelity/parameterization remain; F13 |
| §3I PDF/SVG import and PDF export | Absent as dedicated interchange | System Print-to-PDF is an existing flattened route; F12 |
| §3I Drag a layer out as PNG/macOS Share | Absent | Clipboard support is already present; delivery convenience follow-ons |
| §3I Multi-frame GIF/multi-page TIFF/animated output | Absent | First-frame/page behavior and GIF Save As safeguard exist; F12/F16 |
| §3I Native autosave/version browsing | Absent as the proposed native workflow | In-place autosave explicitly disabled; inherited recovery requires actual verification; F03 |
| §3J Record/edit/replay/assistant-authored Actions | Implemented with replay gaps | Unsupported edit placeholders, pressure/time, asset and target coverage; F13 |
| §3J Symbolic action geometry | Partial | Layer/guide symbols exist; geometry remains absolute with mismatch warnings; F13 |
| §3J Repeat Last | Implemented | Keep and extend as new filters ship |
| §3J Command palette | Implemented | Uses Shift-Command-K to preserve existing Crop shortcut |

Evidence: filter/transform models, menu inventory, export capabilities and Actions implementation.[^23][^26][^27][^29][^32]

### Older roadmap and cross-cutting carryovers

- The four high-leverage phases—selection algebra, masks, editable text and unified transforms—are implemented. Perspective also subsequently shipped. Transforming a floated selection, source-backed ordinary raster transforms, higher precision and scalable buffers remain open.
- `next-features.md` still contributes Paste Into, additional retouch brushes, general multistop gradients, History, Trim/Reveal All, specialized subject models and paths. Its Copy Merged, auto corrections, clipping, basic shapes, crop/manual straighten, healing and subject tools already exist.
- Style-plane caching is implemented; a general spatial-filter/tile/projection cache and global memory policy are not. Native format evolution has reached version 8 with backward readers; multiple safe version bumps are not a product defect.
- Prior notes identify validation debt around real auxiliary mattes, color/printing dialogs, adjustment-sheet previews/cancellation, real face detection, difficult inpainting and camera RAW/Develop/Batch UI. Those are outstanding evidence requirements, not proof of failure.
- Full CMYK/prepress, Bézier paths, high-depth storage, tiling and blend/compositing-space controls were deliberate deferrals. Reconsider them individually: high precision and resource management now have broad photographic value; full prepress can remain a separate market decision.

## 6. Recommended implementation sequence

### Stage A: Trustworthy saves and rendering

Fix F01, F02 and F04; reject silent group-render omissions; repair TIFF profile/PPI handling; introduce PSD compatibility reporting. Verify native recovery and create a repeatable save/reopen regression corpus. Keep the scope focused enough that these fixes can ship without waiting for an architectural rewrite.

**Exit gate:** No successful native save violates the current reader contract; no flat Save silently establishes a saved state it cannot preserve; transparent edges pass independent filtering checks; tagged TIFF round trips correctly; recovery is demonstrated.

### Stage B: Complete high-value photographic workflows

Implement selection/Quick Mask undo and Reselect, Color Range, mask refinement and independent masks. Add Unsharp Mask/High Pass/denoise with editable parameters, better retouch sampling and export presets. Add cancellable execution for measured multi-second operations. History, layer search and New Document are useful parallel UI workstreams.

**Exit gate:** Complete portrait cutout, layered retouch and web/print delivery exercises without leaving Rasterize or relying on an unexplained destructive workaround.

### Stage C: Preserve source quality across revisions

Commit to the high-depth document design, source-backed RAW/placed layers, asset persistence and filtered-source rendering. Prototype memory and performance together: increasing pixel precision increases the cost of projections, undo and spatial effects. Define format migration and legacy rendering semantics before implementation spreads across operators.

**Exit gate:** A high-depth RAW-based project retains sources, edits and precision across native save/reopen, repeated transforms and high-depth export within published resource limits.

### Stage D: Exchange and scale

Extend layered PSD fidelity/export, soft proofing and delivery controls. Introduce regional/tiled rendering and scratch-backed storage where measurement justifies them. Close action replay gaps and add explicit geometry/asset parameters for production batch work.

**Exit gate:** A realistic multilayer production job can be revised, recovered, exchanged and batch-delivered with measured latency and memory, with all compatibility losses made explicit.

### Stage E: Expand professional specialties

Add Liquify, paths/vector masks, richer typography/brushes and advanced photographic alignment/stacking according to demand. Choose whether full CMYK/prepress, illustration, animation or generative workflows are actual product commitments. Rasterize can be a strong still-photo editor before all of these are implemented.

The earlier S/M/L estimates are useful historical sketches, not dependable estimates for the professional versions of these features. In particular, native recovery, PSD fidelity, high-depth pixels, RAW source persistence, editable filters and large-document rendering cross multiple subsystems and need explicit design work. A History panel also needs an accessible history model; `NSUndoManager` is not automatically a ready-to-render list of arbitrary states.

## 7. Acceptance scenarios for a professional release

| Scenario | Required result |
| --- | --- |
| 24/48 MP RAW portrait | Develop in high depth, retain source, retouch separately, revise white balance after reopen |
| Hair/fur/glass cutout | Inspect/refine masks at 100%, remove fringe colors, undo every refinement, retain reusable mask |
| Layered client revision | Search/group layers, independently move mask and content, retain source transforms, switch named variants |
| Long retouch session | Responsive brush/heal operations, reliable cancellation, useful history within a known memory budget |
| Interrupted editing | Recover dirty native and untitled documents with no ambiguous overwrite of the original |
| Single-layer rich document | Save/reopen channels, guides, masks or editable descriptions without silent loss |
| Native capacity boundary | Every successful write reopens; over-limit state refuses before destination replacement |
| Transparent-edge delivery | Resize/blur without dark halos; identical edge colors when composited by an independent application |
| Wide-gamut print delivery | Preserve ICC/PPI/metadata, proof output, create an independently verified TIFF |
| Photoshop exchange | Preserve supported layers/masks/groups; show explicit losses and a faithful composite alternative |
| Batch production | Parameterized operations handle orientation/size differences, naming collisions, missing assets and cancellation |
| Keyboard/tablet workflow | Correct focus and discoverability, dependable pressure behavior, replay fidelity and accessibility checks |

Automate deterministic model/file invariants and MCP integration scenarios. Use human evaluation for photographic quality, mask refinement, tablet feel and accessibility. A large passing unit suite is valuable, but cannot certify these perceptual and workflow outcomes by itself.

## 8. Evidence and verification limits

| Check | Result | Scope |
| --- | --- | --- |
| `make test` | **728 passed, 0 failed, 1 ignored**, across 49 test-result groups including empty library/doc-test groups | Current repository Rust suite; completed with localhost access for transport tests |
| `make typecheck` | **Passed, 0 errors**; 14 existing warnings | Repository's authoritative Swift check; compiler caches redirected to a writable temporary location |
| Native capacity probe | **Writer accepted 401,281,024 layer pixels**; independently counted from saved PNG headers | 1,024 FFI-created/duplicated 626 × 626 layers; no full over-limit decode |
| Alpha resize/blur probes | **Darkened straight-alpha edge colors reproduced** | Tiny public-FFI image operations; expected result independently derived from alpha arithmetic |
| Existing release app: MCP catalog/open/edit/render | **Passed**; 107 advertised tools | Isolated September 8 release binary, dummy API key, non-default port; controlled 16 × 8 PNG fixture |
| Selection-scoped destructive adjustment | **Whole-layer behavior reproduced** | Selected a 2 × 2 region, then Invert changed pixel (12,4) from `#46141E` to `#B9EBE1`; Undo restored `#46141E` |
| Native save/reopen | **Preserved tested state** | One-layer `.rz` retained its saved selection channel, vertical guide at x=5, Display P3 and 300 × 300 PPI |
| TIFF save/reopen | **Profile/PPI limitation reproduced** | Export reported profile written and resolution dropped; reopening reported sRGB and 72 × 72 PPI |
| Fresh optimized app build | **Not completed** | Stopped during prolonged optimized Swift compilation; live checks used the pre-existing release binary, not a claimed new successful build |
| Report integrity | **Checked** | Local source links and footnote references resolve; no application/core source changes accompany this report |

The live-app checks establish behavior in the available release binary. Current-source inspection and current Rust/typecheck results provide separate evidence for the checkout; a binary/source identity attestation was not performed. Both isolated test app instances were terminated after their checks.

The tiny alpha probes establish an arithmetic defect on controlled transparent pixels; they do not quantify its prevalence across photographs. The native save probe establishes successful writing beyond the reader's aggregate-pixel limit; a full over-limit decode was intentionally not performed. Single-layer save-state loss follows from inspected Save predicates and flat serialization, not a recorded interaction with the macOS save dialog.

No reference corpus of real RAW cameras, masked Photoshop projects, physical printer output, tablet devices or difficult portrait cutouts was evaluated end-to-end here. Performance numbers quoted from source comments are historical implementation measurements, not new benchmark results. New memory figures are simple buffer-size calculations. Recommendations about photographic quality therefore include explicit validation gates rather than unsupported equivalence claims.

## Sources

Repository references below identify the files and relevant symbols/line anchors at the reviewed commit.

[^1]: Repository design history: [previous gap review](photo-editor-gap-review.md), especially §0, §§2–5; [high-leverage roadmap](high-leverage-roadmap.md); [next features](next-features.md); [GIMP capability study](gimp-image-manipulation-capabilities.md).
[^7]: [Adjustment model](../core/src/adjust.rs), [adjustment schema](../app/Sources/AdjustmentSchema.swift), [histogram](../app/Sources/Histogram.swift), [Info panel](../app/Sources/InfoPanelViewController.swift), [adjustment tests](../core/tests/adjustment_tests.rs).
[^8]: [Selection model](../app/Sources/Selection.swift), [selection operations](../core/src/doc_select.rs), [subject selection](../app/Sources/SubjectSelection.swift), [channel model](../core/src/doc_channel.rs), [selection tests](../core/tests/selection_tests.rs), [channel tests](../core/tests/channel_tests.rs).
[^9]: [Group model/compositor](../core/src/doc_group.rs), [group operations](../core/src/doc_group_ops.rs), [locks](../core/src/doc_lock.rs), [alignment](../core/src/doc_align.rs), [guides](../core/src/doc_guide.rs), [SnapEngine](../app/Sources/SnapEngine.swift), [group tests](../core/tests/group_tests.rs).
[^10]: [Healing](../core/src/doc_heal.rs), [inpainting](../core/src/doc_inpaint.rs), [PatchMatch](../core/src/patchmatch.rs), [red-eye](../core/src/doc_redeye.rs), [content-aware tests](../core/tests/content_aware_tests.rs).
[^11]: [Native format](../core/src/rzdc.rs#L94): `MAX_RZDC_TOTAL_LAYER_PIXELS`; `encode_native` at line 257; `parse_native` aggregate check at line 755. Probe description and observed values are in §§2 and 8 of this review.
[^12]: [ImageDocument](../app/Sources/ImageDocument.swift#L137): undo depth and autosave at lines 137–140; writable-type predicate at 175; flat `write` at 425; final flat-save predicate at 1011.
[^13]: [Atomic save helper](../core/src/rz_image.rs#L312), `save_atomically`; [native saving](../core/src/rzdc.rs#L458); [document mutation and undo](../app/Sources/ImageDocument.swift#L499).
[^14]: [Flat resize/blur](../core/src/ops.rs#L47), `resize`, `blur` at 217; [document resize](../core/src/doc.rs#L1343); [transform sampling](../core/src/doc_transform.rs).
[^15]: [Color/file capabilities](../core/src/doc_color.rs#L46), TIFF limitations and `format_carries`; [metadata input](../core/src/metadata.rs); [metadata tests](../core/tests/metadata_tests.rs), `jpeg_and_png_are_the_only_walked_containers`; [platform input paths](../app/Sources/ImageDocument.swift#L264); [export notices](../app/Sources/ExportCapabilities.swift#L81).
[^16]: [PSD importer](../core/src/psd.rs#L143), `open_psd`, group construction, mask/style omissions, canvas intersection and document defaults; [PSD tests](../core/tests/psd_tests.rs); [writable types](../app/Sources/ImageDocument.swift#L122).
[^17]: [Layer storage](../core/src/doc.rs#L76), `Layer.pixels`; [flat decode](../core/src/rz_image.rs#L143), `to_rgba8`; [RAW render](../app/Sources/RawImage.swift#L350), `CIFormat.RGBA8`; [adjustment interpreter](../core/src/adjust.rs).
[^18]: [ICC profile support](../core/src/icc.rs), [ICC transforms](../core/src/icc_transform.rs), [color conversion](../core/src/doc_color.rs#L17), [profile conversion UI](../app/Sources/ColorProfileControls.swift), [print implementation](../app/Sources/ImageDocument+Print.swift), [pixel readout](../app/Sources/PixelReadout.swift).
[^19]: [RAW import contract](../app/Sources/RawImage.swift#L16), decode at 350 and decoder choice near 416; [development settings](../app/Sources/RawDevelopSettings.swift#L8); [RAW open/Revert](../app/Sources/ImageDocument.swift#L343); [Develop preview](../app/Sources/RawDevelopWindowController.swift#L56).
[^20]: [Live Photo payload](../app/Sources/LivePhotoLayer.swift#L23), absolute source paths at 33; [described layers](../app/Sources/DescribedLayer.swift); [described transforms](../app/Sources/EditorViewController+DescribedTransform.swift).
[^21]: [Selection loading](../app/Sources/EditorViewController+SelectionIO.swift#L7), non-undoable selection state; [Quick Mask completion](../app/Sources/ImageCanvasView.swift#L2498); [Select menu](../app/Sources/AppDelegate.swift#L388).
[^22]: [Layer mask model](../core/src/doc.rs#L100), layer-sized linked coverage; [mask menu](../app/Sources/AppDelegate.swift#L760); [paint targets](../app/Sources/PaintTarget.swift); [channel routing](../app/Sources/ImageDocument+Channels.swift); [selection tools](../app/Sources/Tools.swift#L7).
[^23]: [Filters menu](../app/Sources/AppDelegate.swift#L806); [sharpen](../core/src/ops.rs#L226); [filter operators](../core/src/ops_filters.rs); [spatial tone adjustment](../core/src/adjust_tone.rs).
[^24]: [Document projection](../app/Sources/ImageDocument.swift#L881), `refreshProjection`; [painting](../core/src/doc.rs#L1087), full target-buffer clone; [group budget](../core/src/doc_group.rs#L535), `rendered_group` and `composite_gated_pass`; [compositing/storage](../core/src/doc.rs).
[^25]: [Healing controller](../app/Sources/EditorViewController+Heal.swift#L18), sampling policy and blocking commit; [Content-Aware Fill commit](../app/Sources/EditorViewController+ContentAwareFill.swift#L110); [fill preview/cancel](../app/Sources/ContentAwareFillSheetController.swift#L24); [batch execution](../app/Sources/BatchRun.swift#L159).
[^26]: [Export accessory](../app/Sources/ExportAccessoryController.swift#L3), current controls; [Export flow](../app/Sources/ImageDocument.swift#L1062); [Batch](../app/Sources/BatchRun.swift); [print/PDF route](../app/Sources/ImageDocument+Print.swift).
[^27]: [Action model](../app/Sources/Action.swift#L41), unreplayable steps and absolute geometry at 177; [recorded gestures](../app/Sources/EditorViewController+Actions.swift#L64), transforms and pressure/time limits; [ActionPlayer](../app/Sources/ActionPlayer.swift); [auto-adjust recording](../app/Sources/EditorViewController+AutoAdjust.swift#L70).
[^28]: [Agent server](../core/src/agent.rs#L1), localhost transport; [assistant loop](../core/src/assistant.rs), cancellation/API boundaries; [app startup](../app/Sources/AppDelegate.swift#L20); [agent dispatch/catalog](../app/Sources/AgentCatalog.swift).
[^29]: [AppDelegate](../app/Sources/AppDelegate.swift#L10), no blank New and menu construction; [editor window](../app/Sources/EditorWindowController.swift); [layer panel](../app/Sources/LayersPanelViewController.swift); [editor navigation](../app/Sources/EditorViewController.swift#L1630).
[^30]: [Gradient editor](../app/Sources/GradientEditorView.swift#L3), model limits at 18; [paint gradient options](../app/Sources/EditorViewController+ToolOptions.swift#L815); [Gradient Overlay pane](../app/Sources/GradientOverlayPane.swift#L3); [Stroke pane](../app/Sources/StrokePane.swift#L51); [brush presets](../app/Sources/BrushPreset.swift); [brush engine](../app/Sources/SoftBrush.swift).
[^31]: [Style model](../core/src/style.rs#L30), effect uniqueness and adjustment refusal; [style UI validation](../app/Sources/EditorViewController+LayerStyle.swift#L19); [style agent validation](../app/Sources/AgentServer+LayerStyle.swift#L24); [text payload](../app/Sources/TextLayer.swift#L146); [text editing](../app/Sources/ImageCanvasView.swift#L2616); [shape kinds](../app/Sources/ShapeLayer.swift#L39).
[^32]: [Crop/canvas resize](../core/src/doc.rs#L1220), retained layer pixels and cropped canvas planes; [Crop UI](../app/Sources/EditorViewController+Crop.swift); [projective transform](../core/src/doc_perspective.rs); [described-transform rasterization](../app/Sources/EditorViewController+DescribedTransform.swift#L294).
[^33]: [Build/signing settings](../project.yml#L20); [Makefile](../Makefile); [public documentation landing page](../public-docs/src/content/docs/index.mdx); [README](../README.md).
[^34]: [Adjustment color assumptions](../core/src/adjust_math.rs#L16); [White Balance pipeline](../core/src/adjust_white_balance.rs#L52).
[^35]: [Whole-layer filter path](../app/Sources/ImageDocument.swift#L716), `applyToActiveLayer`; [Auto adjustment scope](../app/Sources/EditorViewController+AutoAdjust.swift#L49); [selection-masked adjustment creation](../app/Sources/EditorViewController+Adjustments.swift).
[^36]: [Pass-through clipping limitation](../core/src/doc_group.rs#L127); [group tests](../core/tests/group_tests.rs), `clipping_over_a_pass_through_group_isolates_it`.
[^37]: [Agent sampling/color contract](../app/Sources/AgentServer.swift#L658), `paint_hex_exact` and sRGB color arguments; [UI authored/sampled color handling](../app/Sources/ColorProfile.swift#L33).
