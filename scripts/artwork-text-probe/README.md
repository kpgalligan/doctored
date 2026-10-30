# TextKit capture qualification

`host.swift` is the integration probe: compile it and `comparison.swift` with the actual app sources, then call `runArtworkTextCaptureProbe(directory:)` on the main thread after background work finishes. It exercises the real `TextLayer.drawingPlan`, payload-v3 color runs and `ArtworkTextCapture`; it writes `artwork-text-capture.json` and representative reference/captured PNG pairs.

`standalone.swift` and `main.swift` provide a small development harness behind `ARTWORK_CAPTURE_STANDALONE`. The scaffold deliberately supplies simplified fixed-canvas layout/model/resource types, so its results establish only the capture/oracle seam, not app integration, native persistence, memory admission or authoritative Swift typechecking. Build it with:

```sh
xcrun swiftc -D ARTWORK_CAPTURE_STANDALONE \
  -module-cache-path /tmp/rz-phase8e-capture-cache \
  -target arm64-apple-macos15.0 \
  app/Sources/ArtworkTextCapture.swift \
  scripts/artwork-text-probe/standalone.swift \
  scripts/artwork-text-probe/comparison.swift \
  scripts/artwork-text-probe/main.swift \
  -o /tmp/rz-phase8e-capture-probe
/tmp/rz-phase8e-capture-probe > /tmp/rz-phase8e-text-capture-standalone.json
```

Run with normal macOS graphics/font-service access. All generated images belong to these synthetic probe contexts; no user windows are captured.

## Independent reference and declared gates

The reference is an ordinary `NSLayoutManager` superclass draw into an RGBA8 sRGB context. It does not inspect outlines or read the generated PDFs. Fixtures cover mixed Latin/combining/ligature/CJK/Arabic/Hebrew/emoji runs, wrapping, alignment, tight leading, tracking/baseline shifts, transformed geometry and continuous attributed colors.

TextKit's hinted glyph bitmap is a **layout** reference, not the numerical area-coverage oracle for unhinted native outlines. The layout gate permits two pixels of edge neighborhood and 2% alpha-area difference; no ink may appear beyond that neighborhood. The Rust coverage suite independently qualifies native area samples. Superclass decorations are isolated using color-emoji fixtures (whose glyphs become intrinsic parts), then compared to captured paths alone: their bounds must match, and alpha may differ by at most two byte codes from AA/decimal-coordinate replay. Authored colors never come from PDF color literals.

Intrinsic color pixels must equal independent opaque superclass pixels exactly, with the original authored opacity preserved separately and exactly. This is intentional: legacy TextKit color emoji ignores foreground alpha, whereas the new artwork contract applies authored opacity after decoding the intrinsic asset. Comparing a translucent new emoji to legacy TextKit's opaque emoji would test the wrong contract. Intrinsic monochrome bitmap glyphs use a separate A8 coverage part with continuous authored RGBA; the core qualifies that scalar promotion/assembly path. Verified platform facts: outline-only `EBDT`/`bdat` strikes report empty bounds and are skipped; OT-SVG and `sbix` glyphs take the color path; the reachable outline-less non-color glyphs (Apple Chancery U+0338) draw no ink, so capture probes their glyph box first and drops them instead of adding an empty A8 part (`outline_less_no_ink`: "a̸" × 150 keeps the parts of "a" × 150). The inked A8 path stays a fail-safe, qualified only by core composition.

Capture rejects over 4096 UTF-16 units, 256 ordered parts, 64 million component samples, or 262144 outline points. Decoration PDF output is bounded during production to 256 KiB; its scanner bounds operators, path points, paths, state nesting and transforms, and rejects unsupported text/raster/clipping/blend semantics. Main-thread admission precedes layout and buffer work. Actual live-set/admission qualification belongs to the app/core host suite, not the standalone resource stub.
