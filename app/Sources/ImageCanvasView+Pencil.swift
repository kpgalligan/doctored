import AppKit

/// The Pencil on the canvas: the Brush's stroke pipeline with an aliased tip
/// (`BrushTip.aliased`). Its live preview stamps one dab image the CORE
/// rasterized (`dr_brush_overlay_rgba8` over a dab-sized canvas), drawn 1:1 at
/// the snapped geometry its session records (`NativeBrushStroke.aliasedGeometry`),
/// so the drag shows the very pixels mouse-up commits: the byte commit
/// re-rasterizes the stroke's dabs through the same rule
/// (`BytePaintRasterizer.pixels`), and a typed document's job computes its
/// coverage from them. A pressure-sized dab is the image scaled without
/// interpolation, and the commit is exact either way.
extension ImageCanvasView {
    /// The stroke's gstate (saved by `beginStroke`, restored at its end):
    /// no antialiasing and no image interpolation, so nothing drawn for the
    /// Pencil can grow a grey edge.
    func beginPencilStroke(in context: CGContext) {
        context.setShouldAntialias(false)
        context.interpolationQuality = .none
    }

    /// One snapped dab at the brush size in `color`'s straight ink, as the core
    /// paints it. It throws when the dab cannot be built, and mouse-down then
    /// refuses the stroke: without a dab the drag would preview as a path the
    /// session records no dabs for, so its commit would hold only the first.
    func pencilDab(color: NSColor) throws -> CGImage {
        let diameter = NativeBrushStroke.aliasedGeometry(.zero, diameter: brushSize).diameter
        let side = Int(diameter)
        guard let rgb = ColorProfile.bytes(color, in: NSColorSpace(cgColorSpace: documentColorSpace) ?? .sRGB)
        else { throw DoctoredCoreError(message: "Could not convert the paint color.") }
        let dab = try NativeBrushStroke(tip: strokeTip, pixel: false,
            dabs: [Double(diameter) / 2, Double(diameter) / 2, Double(diameter), 1])
        let pixels = try dab.aliasedOverlay(width: side, height: side,
            rgb: Array(rgb.prefix(3)), selection: nil, opacity: 1)
        guard let provider = CGDataProvider(data: Data(pixels) as CFData),
              let image = CGImage(
                width: side, height: side, bitsPerComponent: 8, bitsPerPixel: 32,
                bytesPerRow: side * 4, space: documentColorSpace,
                bitmapInfo: CGBitmapInfo(rawValue: CGImageAlphaInfo.premultipliedLast.rawValue),
                provider: provider, decode: nil, shouldInterpolate: false, intent: .defaultIntent)
        else { throw DoctoredCoreError(message: "Could not build the Pencil's dab image.") }
        return image
    }

    /// Mouse-down's admission for a Pencil stroke, before anything is built:
    /// its preview dab (`pencilDab`, above one pixel) and, for a byte stroke,
    /// the commit's whole-canvas figure (`BytePaintRasterizer.aliasedScratch`),
    /// so mouse-up never asks for more than the gesture was admitted with.
    func admitPencilImages(diameter: CGFloat) throws {
        let side = Int(NativeBrushStroke.aliasedGeometry(.zero, diameter: diameter).diameter)
        let preview = diameter > 1
            ? BytePaintRasterizer.aliasedScratch(width: side, height: side, dabs: 1) : 0
        let commit = !nativeBrushEnabled && nativeStrokeSession != nil
            ? BytePaintRasterizer.aliasedScratch(width: overlayWidth, height: overlayHeight,
                                                 dabs: NativeBrushStroke.maximumDabs)
            : 0
        _ = try RasterResources.shared.admit(scratch: PrecisionDocumentDescriptor.sum(preview, commit))
    }
}
