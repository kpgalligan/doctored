import AppKit

// Image ▸ Trim… and Image ▸ Reveal All: the two commands that set the
// canvas to what the picture holds. Trim crops the canvas inward past the
// rows and columns that are transparent, or that match the top-left or the
// bottom-right pixel, on the edges chosen; Reveal All grows it until no
// layer has pixels outside it. Both read the COMPOSITE's answer and commit
// through the ordinary canvas ops (`cropped`, `canvasResized`), so layers
// keep their pixels, and nothing is resampled at any depth. The sheet is
// `TrimSheetController`, the commands `EditorViewController+CanvasFraming`,
// the MCP twins `trim` and `reveal_canvas`.

/// What Trim trims away.
enum TrimBasis: Int, CaseIterable {
    /// Rows and columns whose every pixel is fully transparent.
    case transparent
    /// Rows and columns whose every pixel equals the top-left pixel.
    case topLeft
    /// Rows and columns whose every pixel equals the bottom-right pixel.
    case bottomRight

    var agentName: String {
        switch self {
        case .transparent: return "transparent"
        case .topLeft: return "top_left"
        case .bottomRight: return "bottom_right"
        }
    }

    var title: String {
        switch self {
        case .transparent: return "Transparent Pixels"
        case .topLeft: return "Top Left Pixel Color"
        case .bottomRight: return "Bottom Right Pixel Color"
        }
    }

    init?(agentName: String) {
        guard let match = Self.allCases.first(where: { $0.agentName == agentName }) else {
            return nil
        }
        self = match
    }
}

/// The edges Trim may move; an unchecked edge stays where it is. The raw
/// values are the core's bitmask.
struct TrimEdges: OptionSet {
    let rawValue: Int
    static let top = TrimEdges(rawValue: 1)
    static let bottom = TrimEdges(rawValue: 2)
    static let left = TrimEdges(rawValue: 4)
    static let right = TrimEdges(rawValue: 8)
    static let all: TrimEdges = [.top, .bottom, .left, .right]
}

extension ImageDocument {
    /// The canvas rectangle Trim would keep, read from the composite at the
    /// document's own depth: the 8-bit composite's bytes, the typed
    /// projection's exact samples at 16 bits and float, or a CMYK document's
    /// composite ink. nil when every pixel
    /// is trimmable (nothing would be left) or there is no image.
    func trimRect(basis: TrimBasis, edges: TrimEdges) throws -> CGRect? {
        guard let doc else { return nil }
        // #547 phase 4b: a CMYK document compares its composite INK, not the view.
        if doc.isCmyk, let precision = doc.precision {
            let context = try PrecisionWorkContext(source: precision)
            return try precision.cmykTrimRect(basis, edges, workingLimit: context.limit,
                                              otherRetained: context.otherRetained)
        }
        if doc.isHighDepth {
            return try currentPrecisionProjection().trimRect(basis, edges)
        }
        return try byteComposite(of: doc).trimRect(basis, edges)
    }
}

extension RasterDocument {
    /// The rectangle Reveal All makes the canvas, in the current canvas's
    /// coordinates: the canvas united with the content bounds of every
    /// top-level entry (a group answers for its descendants). nil when
    /// nothing reaches outside the canvas.
    func revealAllRect() -> CGRect? {
        let canvas = CGRect(x: 0, y: 0, width: width, height: height)
        var union = canvas
        for index in layerTree.topLevel() {
            guard let bounds = layerBounds(index), bounds.width > 0, bounds.height > 0 else {
                continue
            }
            union = union.union(CGRect(
                x: bounds.x, y: bounds.y, width: bounds.width, height: bounds.height))
        }
        return union == canvas ? nil : union
    }
}
