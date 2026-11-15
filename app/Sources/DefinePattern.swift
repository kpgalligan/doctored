import AppKit

/// Edit ▸ Define Pattern…'s tile from a document: the COMPOSITE inside a hard-edged rectangular
/// selection, or the whole canvas with nothing selected, at most 512 pixels a side — a straight
/// RGBA8 raster, the custom tile every pattern consumer reads at every depth and in every mode,
/// and beside it the composite's own numbers where the raster cannot hold them (`native`,
/// `dr_clipboard_payload_pattern_native`, `designs/pattern-tiles.md`). A fill in the same kind of
/// document reads the samples, so it gives the source back exactly:
///
/// | Document | The raster | The samples |
/// |---|---|---|
/// | RGB 8 | the composite's bytes | none |
/// | RGB 16 | each code rounded to 8 bits (the copy's 8-bit form) | `rgb16`, unless every code is 8-bit |
/// | Indexed | the composite's colours | none |
/// | RGB 32 float | round(255 · clamp(linear, 0, 1)) (`dr_pattern_tile_from_linear`) | `rgb32`, unless the raster holds every value |
/// | Gray, Duotone, Bitmap | the gray code at 8 bits, R = G = B | Gray 16 and 32: as RGB 16 and 32 |
/// | Lab | the composite converted to 8-bit sRGB | none |
/// | CMYK | the composite converted to 8-bit sRGB | `cmyk16`, always: the ink |
///
/// Another kind of document reads the raster, as before. The library keeps the tile
/// (`PatternLibrary`); the UI is `EditorViewController+DefinePattern.swift` and MCP's
/// `define_pattern` (`AgentServer+Patterns.swift`).
enum DefinePattern {
    static let notRectangle = "Define Pattern takes a hard-edged rectangular selection, or "
        + "nothing selected for the whole canvas: every pixel inside the selection's bounds must "
        + "be fully selected (no feather, no anti-aliased or elliptical edge)."

    static func tooLarge(width: Int, height: Int) -> String {
        "A pattern is at most \(LibraryPattern.maximumSide) pixels a side; this one would be "
            + "\(width) × \(height). Select a smaller rectangle."
    }

    struct Tile {
        let width: Int
        let height: Int
        let rgba: Data
        /// The composite's own numbers the raster cannot hold; nil when it holds them all.
        var native: PatternNative? = nil
        /// "selection" or "canvas": what the window came from.
        let source: String
    }

    /// The canvas window the tile covers: the selection's bounds when every pixel in them is
    /// fully selected, else the canvas when nothing is selected. Throws `notRectangle` or
    /// `tooLarge` by name.
    static func window(of doc: RasterDocument, selection: CanvasSelection?) throws -> CGRect {
        let canvas = CGRect(x: 0, y: 0, width: doc.width, height: doc.height)
        var window = canvas
        if let selection {
            window = selection.bounds.integral.intersection(canvas)
            guard !window.isNull, !window.isEmpty else {
                throw DoctoredCoreError(message: notRectangle)
            }
            let samples = selection.maskSamples16()
            let x0 = Int(window.minX), x1 = Int(window.maxX)
            for y in Int(window.minY)..<Int(window.maxY) {
                let row = y * selection.canvasWidth
                for x in x0..<x1 where samples[row + x] != UInt16.max {
                    throw DoctoredCoreError(message: notRectangle)
                }
            }
        }
        let width = Int(window.width), height = Int(window.height)
        guard width <= LibraryPattern.maximumSide, height <= LibraryPattern.maximumSide else {
            throw DoctoredCoreError(message: tooLarge(width: width, height: height))
        }
        return window
    }

    /// `doc`'s tile over `selection`'s window (the table above). `document` supplies the 8-bit
    /// composite it already holds.
    static func tile(of doc: RasterDocument, in document: ImageDocument,
                     selection: CanvasSelection?) throws -> Tile {
        let window = try window(of: doc, selection: selection)
        let source = selection == nil ? "canvas" : "selection"
        guard let precision = doc.precision else {
            let composite = try document.byteComposite(of: doc)
            return Tile(width: Int(window.width), height: Int(window.height),
                        rgba: try bytes(of: composite, in: window), source: source)
        }
        let payload = try doc.clipboardCopyMerged(window: window, selection: nil)
        let budget = try PrecisionWorkContext(source: precision)
        let image: RasterImage
        if doc.isFloat {
            guard let linear = payload.image() else {
                throw DoctoredCoreError(message: "The float composite could not be read.")
            }
            image = try RasterDocument.patternTile(fromLinear: linear)
        } else if doc.isLab || doc.isCmyk {
            image = try payload.rgba8(
                in: RasterProfile.builtin(.sRGB), profile: .convert,
                workingLimit: budget.limit, otherRetained: budget.otherRetained)
        } else {
            image = try payload.rgba8(workingLimit: budget.limit,
                                      otherRetained: budget.otherRetained)
        }
        let bounds = CGRect(x: 0, y: 0, width: image.width, height: image.height)
        return Tile(width: image.width, height: image.height,
                    rgba: try bytes(of: image, in: bounds),
                    native: try RasterDocument.patternNative(of: payload), source: source)
    }

    /// `image`'s straight RGBA8 bytes inside `window` (in its own pixel grid).
    private static func bytes(of image: RasterImage, in window: CGRect) throws -> Data {
        let x0 = Int(window.minX), y0 = Int(window.minY)
        let width = Int(window.width), height = Int(window.height)
        guard x0 >= 0, y0 >= 0, x0 + width <= image.width, y0 + height <= image.height,
              width > 0, height > 0
        else { throw DoctoredCoreError(message: "The pattern's window lies outside the image.") }
        let stride = image.width * 4
        let copied: Data? = image.withPlateBytes { pixels in
            var out = Data(capacity: width * height * 4)
            for y in y0..<(y0 + height) {
                out.append(pixels + y * stride + x0 * 4, count: width * 4)
            }
            return out
        }
        guard let copied else { throw DoctoredCoreError(message: "The composite has no pixels.") }
        return copied
    }
}
