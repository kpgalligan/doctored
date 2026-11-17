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
    /// The Background Eraser's options and their core request, latched at
    /// mouse-down (`EditorViewController.latchBackgroundEraser`).
    var backgroundEraser: BackgroundEraserOptions?
    var backgroundEraserRequest: String?
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
        // The Pencil's aliased tip, a textured stroke, a dual one and a coloured one are the
        // core's coverage (a coloured one's ink too), on every byte target, as a typed
        // document's job computes it.
        if stroke.tip.aliased || stroke.texture != nil || stroke.dual != nil || stroke.inks != nil {
            return try aliasedPixels(stroke: stroke, ink: ink, width: width, height: height,
                space: space, selection: selection, coverageOpacity: coverageOpacity)
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

    /// The overlay of a stroke the core rasterizes — an aliased (Pencil) one
    /// (`dr_brush_overlay_rgba8`), a textured one (`dr_brush_overlay_rgba8_textured`, its
    /// shapes included) or a dual one (`dr_brush_overlay_rgba8_dual`, its shapes and texture
    /// included) — so every byte target — layer, mask, plane, channel and the byte
    /// Quick Mask — takes exactly the coverage a typed document's stroke does. A coverage
    /// target's opacity rides in the request; a selection multiplies the coverage by its bytes.
    private static func aliasedPixels(
        stroke: NativeBrushStroke, ink: ByteDabInk, width: Int, height: Int,
        space: CGColorSpace, selection: CanvasSelection?, coverageOpacity: Double?
    ) throws -> [UInt8] {
        guard case .color(let color) = ink else {
            throw DoctoredCoreError(message: "A Pencil or textured stroke paints a colour; a sampled "
                + "dab has none.")
        }
        guard let bytes = ColorProfile.bytes(color, in: NSColorSpace(cgColorSpace: space) ?? .sRGB) else {
            throw DoctoredCoreError(message: "Could not convert the paint color.")
        }
        _ = try RasterResources.shared.admit(scratch: aliasedScratch(width: width, height: height,
            dabs: stroke.dabs.count / 4,
            shaped: (stroke.texture != nil || stroke.dual != nil || stroke.inks != nil)
                && stroke.shapes != nil,
            dualDabs: stroke.dual.map { $0.dabs.count / 4 }, coloured: stroke.inks != nil))
        return try stroke.coreOverlay(width: width, height: height, rgb: Array(bytes.prefix(3)),
            selection: selection?.maskBytes(), opacity: coverageOpacity ?? 1)
    }

    /// The most a core overlay of `dabs` dabs over a `width × height`
    /// canvas asks: its RGBA8 output, the selection bytes, and the core's
    /// brush plan (master: `brush_coverage::plan`, `4 · BOOKKEEPING_BYTES +
    /// 8 · |F| + 2 · n + 32 · dabs` with the footprint `|F| ≤ n`), plus 16 bytes a
    /// dab for `shaped` dabs (the textured and dual exports' shapes) and, with `dualDabs`, the
    /// second list's own plan beside it (the dual admission sums the two). A `coloured` stroke
    /// adds the ink the core keeps over the footprint, 24 bytes a pixel (`|F| ≤ n`), and its
    /// inks, 24 bytes a dab. The canvas admits the whole-canvas figure at mouse-down, so the
    /// commit never asks for more than the gesture was admitted with.
    static func aliasedScratch(width: Int, height: Int, dabs: Int, shaped: Bool = false,
                               dualDabs: Int? = nil, coloured: Bool = false) -> UInt64 {
        let n = UInt64(max(width, 0)) * UInt64(max(height, 0))
        let bookkeeping: UInt64 = 4 * 1_048_576
        var second: UInt64 = 0
        if let dualDabs { second = bookkeeping + 10 * n + 32 * UInt64(max(dualDabs, 0)) }
        let perDab: UInt64 = (shaped ? 48 : 32) + (coloured ? 24 : 0)
        let ink: UInt64 = coloured ? 24 * n : 0
        return 4 * n + n + 4096 + bookkeeping + 10 * n + perDab * UInt64(max(dabs, 0)) + second
            + ink
    }
}
