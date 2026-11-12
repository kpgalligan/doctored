import AppKit

/// Edit ▸ Define Pattern…'s tile from a document: the COMPOSITE inside a hard-edged rectangular
/// selection, or the whole canvas with nothing selected, as straight RGBA8 at most 512 pixels a
/// side — the custom tile every pattern consumer reads at every depth and in every mode. The tile
/// is written so that filling it back into the same kind of document reproduces the source as
/// closely as 8 bits allow:
///
/// | Document | The tile |
/// |---|---|
/// | RGB 8 | the composite's bytes |
/// | RGB 16, Indexed | each code rounded to 8 bits (the copy's 8-bit form, its own numbers) |
/// | RGB 32 float | round(255 · clamp(linear, 0, 1)), alpha kept (`dr_pattern_tile_from_linear`) |
/// | Gray, Duotone, Bitmap | the gray code at 8 bits, R = G = B |
/// | Lab, CMYK | the composite converted to 8-bit sRGB |
///
/// A 32-bit tile clips above white and is 8-bit linear; a CMYK tile does not keep its
/// separation (a K-only black comes back as the profile's four-colour black). The library keeps
/// the tile (`PatternLibrary`); the UI is `EditorViewController+DefinePattern.swift` and MCP's
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
                    rgba: try bytes(of: image, in: bounds), source: source)
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
