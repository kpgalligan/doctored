import AppKit

// Every actually stamped byte gesture, including ordinary mouse soft/flowed/non-round tips,
// pressure-opacity, timer deposits, coverage and pixel dabs, validates/serializes before
// publication. Coverage opacity is scaled once after accumulation; native Quick Mask remains a
// separate u16 path. At 8 bits, historical unversioned points retain their old meaning; the byte
// walker (`SoftBrush.stampCenters`) throws `dabLimitRefusal` instead of returning a truncated
// prefix, and every byte caller computes the walk before its non-throwing draw closure. A refused
// gesture releases its session/source and restores only its owned live preview; a foreign document
// replacement is never registered as its history. A byte stroke's size reaches
// `NativeBrushStroke.maximumDiameter` (5,000 px) and `BytePaintRasterizer.pixels` admits the dab
// image beside the overlay (`SoftBrush.dabImageScratch`); a CANVAS gesture that reaches a stroke cap
// stops laying ink and commits what was laid (`NativeBrushStrokeSession.Overflow.stopInk`).

/// Values owned by one byte gesture from mouse-down through its sole commit.
struct BytePaintStrokeCapture {
    let base: RasterDocument
    let layer: Int
    let layerID: UInt64
    let target: PaintTarget
    let selectedTarget: PaintTarget
    let channelID: UInt64?
    let selection: SelectionHistoryState
    let tool: EditorTool
    let options: PaintToolOptions
    let color: NSColor
    let blend: DrBlendMode
    /// The History Brush's source state, latched at mouse-down; nil
    /// for every other tool.
    var historySource: HistoryBrushSource?
    /// How many of the canvas session's dabs the Smudge preview has
    /// already dragged along (`EditorViewController.smudgeTick`).
    var smudgedDabs = 0
    /// The Art History Brush's seed, drawn at mouse-down and
    /// recorded, so every tick and the replay lay the same strokes.
    var artSeed: UInt32 = 0
}

enum ByteDabInk {
    case color(NSColor)
    case source(CGImage)
}

/// The byte commit and MCP replay share this exact drawing and coverage seam.
enum BytePaintRasterizer {
    static func pixels(
        stroke: NativeBrushStroke, size: CGFloat, ink: ByteDabInk,
        width: Int, height: Int, space: CGColorSpace,
        selection: CanvasSelection?, coverageOpacity: Double?
    ) throws -> [UInt8] {
        guard width > 0, height > 0, width <= RasterImage.maxResizePixels / height,
              size.isFinite, (1...CGFloat(NativeBrushStroke.maximumDiameter)).contains(size),
              coverageOpacity.map({ $0.isFinite && (0...1).contains($0) }) ?? true else {
            throw DoctoredCoreError(message: "Invalid byte stroke extent, size or coverage opacity.")
        }
        let count = width * height * 4
        // The overlay and, unless the size-1 pixel brush fills squares, the one
        // dab image or mask `rasterizeDabs` builds at `size`.
        _ = try RasterResources.shared.admit(scratch: UInt64(count) + 4096
            + (stroke.pixel ? 0 : SoftBrush.dabImageScratch(diameter: size)))
        var pixels = [UInt8](repeating: 0, count: count)
        try pixels.withUnsafeMutableBufferPointer { buffer in
            guard let base = buffer.baseAddress,
                  let context = CGContext(data: base, width: width, height: height,
                    bitsPerComponent: 8, bytesPerRow: width * 4, space: space,
                    bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue) else {
                throw DoctoredCoreError(message: "Could not allocate the byte stroke overlay.")
            }
            context.translateBy(x: 0, y: CGFloat(height))
            context.scaleBy(x: 1, y: -1)
            selection?.clip(context)
            try SoftBrush.rasterizeDabs(stroke, size: size, ink: ink, space: space, in: context)
        }
        if let coverageOpacity {
            let factor = Int((coverageOpacity * 255).rounded())
            for i in pixels.indices { pixels[i] = UInt8(Int(pixels[i]) * factor / 255) }
        }
        return pixels
    }
}
