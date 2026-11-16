# Text ink rect (ADR)

- **Status:** Accepted — shipped 2026-11-16.
- **Code:** app `TextInkRect.swift` (the value, the measure, the stamping rule), `TextLayer.swift` (the field,
  its version and the rect), the stamp in `nativeArtwork`'s two arms (`NativeArtworkEvaluation.swift`,
  `CmykArtwork.swift`) and the refit before `missingFontAnchor` (`MissingFonts.swift`).

## Context

A straight text layer's raster rect was the layout's used rect (extended over each line's ascender and
descender band under a fixed leading) padded by `TextLayer.padding(forSize:)`, at most about 0.2 em. A glyph
whose ink reaches further was cut with no word: Zapfino's one-glyph "Zapfino" ligature at 48 px dips about
124 px below its baseline and about 12 px past its advance, so its straight render inked 294 × 181 px where
the same glyph on a path inked 297 × 223. Every saved layer's anchor is recovered from its raster (offset
minus the rect's origin), so growing the rect for every layer would move every saved layer whose ink
reaches past the padding: the rule has to tell new layers from saved ones, as each earlier rendering-rule
change did with a payload version.

## Decisions (TI-1..TI-8)

- **TI-1 — The marker is a field plus one version.** `"ink_outset": [left, top, right, bottom]`, whole source
  px, written only when a side is > 0. A payload carrying it is version 7, in any ink, always with `artwork`
  (version 6's rule) and a stored `box_width`. One version rather than three (one per ink) because
  `artwork.ink` already says the ink; a version at all because an older build that ignored the key would
  recover a wrong anchor, and at 7 it reads the layer as a plain raster.
- **TI-2 — The rect.** The straight padded rect (or TP-6's, on a path: [type-on-a-path.md](type-on-a-path.md))
  grown by the stored outsets. It stays a pure function of the payload and its layout; anchor recovery never
  measures ink.
- **TI-3 — The measure.** Over the stack that DRAWS (the aligned one), the union of each shown glyph's
  `CTFontGetBoundingRectsForGlyphs` box at its layout position, grown 1 px; per side `e` = how far that
  reaches past the padded rect; the outset is `ceil(e)` when `e > 0`, else 0. All zero is no field, so a layer
  whose ink fits keeps today's bytes at versions 1–6.
- **TI-4 — Who writes it.** Only `nativeArtwork` (both arms), on every text render, from scratch
  (`RasterDocument.inkFitted`); callers never set it. A saved layer therefore keeps its bytes and anchor until
  an edit re-renders it (a text edit, a Free Transform that re-renders, Image Size, a profile or mode
  conversion, Replace Missing Fonts), which renders at the anchor recovered under the OLD payload. A
  whole-pixel move and open/save do not render and do not mark. Replace Missing Fonts recentres, on the
  axes whose origin depends on the face, the rect its render will publish (refitted) for a marked layer,
  and the unmarked rect for a layer saved without the field, whose saved raster that rule cut: the family
  it was made in then keeps its anchor exactly, where the refitted rect would move it by half its outset.
- **TI-5 — Text on a path keeps TP-6.** `fittedToInk` clears the field there, so version-6 bytes are
  unchanged; the decoder accepts `on_path` at version 7 and the rect rule covers it, for a later measure.
- **TI-6 — Versions from here.** A payload's version is the highest feature it carries (6 `on_path`, 7
  `ink_outset`, 8 `warp`: [warp-text.md](warp-text.md)); a lower feature rides along as
  its optional field. A present `ink_outset` below 7, or a 7 without it, is "not a text layer". A warped
  layer's outset is measured on the unwarped layout (the straight overflow), and its rect is the envelope's
  image of the straight rect grown by it (WT-4).
- **TI-7 — Known limits.** Underline and strikethrough are not measured. The first Free Transform of a
  saved, clipped layer pivots on the old rect's centre; later ones pivot on the grown rect's.
- **TI-8 — A transparency lock defers the marker.** A frozen alpha channel cannot follow a raster whose size
  or offset changes, so a re-render that REPLACES a layer stored without the field, while
  `transparencyLockOwner(layer)` is non-nil, is published by the unmarked rule: the re-renders that keep the
  layout (a colour-only edit, Artwork Colors, a profile or mode conversion) keep succeeding with the same
  rect, and one that changes the layout is refused exactly as before. The layer is marked at its first
  re-render after the lock is cleared. A layer already stored WITH the field is always refitted (the same
  layout gives the same outset and the same rect), locked or not.

## Proof

- jobs-probe "text ink rect": (a) Zapfino's swash leaves the raster's outermost ring clear for left, center
  and right, baseline shift ±20, leading 30, two lines, tracking 5 and a 30° map (red before the change);
  (a-cmyk) in a CMYK 8 document it is marked, in ink, with the ring clear; (b) the codec: versions 1–6
  decode and re-encode byte for byte, a version 7 round-trips, and the malformed forms are refused; (c) a
  version 7's rect is its unmarked rect grown by the four numbers, and a layer whose ink fits is never
  marked; (d) a saved-shaped layer keeps its anchor, is not stale, a text edit marks it at that anchor with
  the directly rendered layer's meta, offset and pixels, a 30°-and-back transform about the anchor marks it
  in place, and Replace Missing Fonts back to its family keeps a leading-30 twin's anchor; (d-marked) Replace
  Missing Fonts from a marked leading-30 Zapfino layer to Helvetica Neue keeps the raster's y-centre within
  1 px; (d-lock) under a transparency lock a colour-only edit keeps it unmarked with its rect, a resizing
  edit is refused, and once unlocked a colour edit marks it, while a marked layer keeps its rect and field
  under the lock; (e) reopening a marked layer in the text session and committing unchanged adds no undo
  step. Each seen red under a mutant (the stamp removed in the RGB arm or the CMYK arm, the field written at
  all zero, the decoder taking it at 3, the rect ignoring it, the compare not ignoring it, the lock exception
  removed or applied to a marked layer, Replace Missing Fonts recentring a saved layer's refitted rect or a
  marked layer's stored one).
- `scripts/verify_text_on_path.py` T11: Zapfino's ligature straight equals its on-path twin's ink box (red
  before the change); T12: `scripts/fixtures/text-ink-rect/legacy-zapfino.doctored`, saved by the build
  before it, reopens with its bounds, origin, ink box and payload bytes, a second copy's first transform
  composes into its description (its raster is not read as stale), it keeps its rect under a transparency
  lock, and an unlocked edit marks it at the same origin with T11's uncut box (red before the change; the
  guard red under recovery measuring ink, the locked edit red with the lock exception removed).
- Hand check (the owner's): a Zapfino swash by eye.
