import AppKit

/// Brush dynamics' texture on the canvas (`designs/brush-dynamics.md`, `BrushTexture.swift`): a
/// Brush or Eraser stroke whose options hold a texture gets its `StrokeTexture` on the dab
/// session at mouse-down (`attachStrokeDynamics`), and the preview draws each dab the session
/// laid as the CORE's image of that one dab — its coverage times the texture at its own canvas
/// pixels (`dr_brush_overlay_rgba8_textured` over the dab's clipped box, the box's origin as
/// the texture's) — so the drag shows the texture where mouse-up commits it. The commit
/// rasterizes the whole stroke through the core (8 bits `BytePaintRasterizer.pixels`, 16 and
/// 32 bits the stroke's job), accumulating before its one rounding where the preview rounds
/// each dab; the owner's hand check compares the two.
extension ImageCanvasView {
    /// Mouse-down, for a stroke whose session has dynamics: the options' texture (pinned:
    /// the Brush and the Eraser only), nil without one. Throws what `StrokeTexture` refuses.
    func strokeTexture(_ dynamics: BrushDynamics?) throws -> StrokeTexture? {
        guard tool.takesTextureDynamics, let texture = dynamics?.normalized?.texture else {
            return nil
        }
        return try StrokeTexture(texture)
    }

    /// The straight RGB ink a textured, dual or coloured stroke's dab images are drawn in: `color`
    /// in the document's space, as the byte commit converts it (a coloured dab draws in its own
    /// ink, `previewInk`). nil without a texture, a dual brush or colour.
    func textureInk(_ color: NSColor) -> [UInt8]? {
        guard nativeStrokeSession?.texture != nil || nativeStrokeSession?.dual != nil
                || nativeStrokeSession?.colour != nil,
              let bytes = ColorProfile.bytes(color, in: NSColorSpace(cgColorSpace: documentColorSpace) ?? .sRGB)
        else { return nil }
        return Array(bytes.prefix(3))
    }

    /// A coloured dab's preview ink in the overlay's space: its own triple, or on a Lab document
    /// (`RasterBrushColour.lab`) the codes' display colour through the swatch's Lab route
    /// (`LabPaint.display`), each 0…1.
    func previewInk(_ ink: [Double]) -> [Double] {
        guard nativeStrokeSession?.colour?.lab == true, ink.count == 3 else { return ink }
        let display = LabPaint.display(LabInk(normalized: ink))
        guard let bytes = ColorProfile.bytes(display,
                  in: NSColorSpace(cgColorSpace: documentColorSpace) ?? .sRGB) else { return ink }
        return bytes.prefix(3).map { Double($0) / 255 }
    }

    /// Mouse-down's admission for a textured or dual stroke, before anything is built: the
    /// stroke's soft dab image, one dab's core image over its box at `diameter` (no dab is
    /// larger: dynamics only shrink one) and, for a byte stroke, the commit's whole-canvas figure
    /// (`BytePaintRasterizer.aliasedScratch`), so mouse-up never asks for more than the
    /// gesture was admitted with. A dual stroke's preview box is the larger of the two tips' and
    /// may hold every dab of both lists (`stampDualRegions`), and its commit plans both lists.
    func admitTextureImages(diameter: CGFloat) throws {
        let dual = nativeStrokeSession?.dual != nil
        let coloured = nativeStrokeSession?.colour != nil
        let dualSize = brushDynamics?.normalized?.dual?.size ?? 0
        let widest = dual ? max(diameter, CGFloat(dualSize)) : diameter
        let side = Int(min(max(widest.isFinite ? widest.rounded(.up) : 1, 1),
                           CGFloat(NativeBrushStroke.maximumDiameter))) + 2
        let preview = BytePaintRasterizer.aliasedScratch(width: side, height: side,
            dabs: dual ? NativeBrushStroke.maximumDabs : 1, shaped: true,
            dualDabs: dual ? NativeBrushStroke.maximumDabs : nil, coloured: coloured)
        let commit = !nativeBrushEnabled && nativeStrokeSession != nil
            ? BytePaintRasterizer.aliasedScratch(width: overlayWidth, height: overlayHeight,
                                                 dabs: NativeBrushStroke.maximumDabs, shaped: true,
                                                 dualDabs: dual ? NativeBrushStroke.maximumDabs : nil,
                                                 coloured: coloured)
            : 0
        try SoftBrush.admitDabImage(diameter: diameter,
                                    extra: PrecisionDocumentDescriptor.sum(preview, commit))
    }

    /// One laid dab of a textured stroke drawn as the core paints it: the dab alone over its
    /// clipped integer box (`brush_coverage::bounds`: floor and ceil of the centre ∓ the
    /// radius, clamped to the canvas), shifted by the box's origin, which is also the
    /// texture's, at the dab's deposit (flow × its factor, so the context alpha stays 1), in
    /// `strokeTextureInk`, or a coloured dab in its own ink (`ResolvedDab.ink`, `previewInk`),
    /// the texture optional. The overlay context is top-left flipped and the image is not
    /// symmetric, so it is drawn un-flipped locally, row 0 at the box's top. A dab the core
    /// refuses is not drawn; the commit is exact either way.
    func stampTexturedDab(_ dab: ResolvedDab, in context: CGContext) {
        let texture = nativeStrokeSession?.texture
        guard texture != nil || dab.ink != nil, let ink = strokeTextureInk else { return }
        let (x, y, radius) = (Double(dab.point.x), Double(dab.point.y), Double(dab.diameter) / 2)
        func edge(_ value: Double, _ limit: Int) -> Int { Int(min(max(value, 0), Double(limit))) }
        let x0 = edge((x - radius).rounded(.down), overlayWidth)
        let y0 = edge((y - radius).rounded(.down), overlayHeight)
        let x1 = edge((x + radius).rounded(.up), overlayWidth)
        let y1 = edge((y + radius).rounded(.up), overlayHeight)
        guard x1 > x0, y1 > y0 else { return }
        let (width, height) = (x1 - x0, y1 - y0)
        let shapes = nativeStrokeSession?.dynamicsShaped == true
            ? [Double(dab.angleDegrees), Double(dab.roundness)] : nil
        guard let one = try? NativeBrushStroke(tip: strokeTip, pixel: false,
                  dabs: [x - Double(x0), y - Double(y0), Double(dab.diameter),
                         Double(strokeTip.flow * dab.factor)],
                  shapes: shapes, texture: texture, inks: dab.ink.map(previewInk)),
              let pixels = try? one.coreOverlay(width: width, height: height, origin: (x0, y0),
                                                rgb: ink, selection: nil, opacity: 1),
              let provider = CGDataProvider(data: Data(pixels) as CFData),
              let image = CGImage(
                width: width, height: height, bitsPerComponent: 8, bitsPerPixel: 32,
                bytesPerRow: width * 4, space: documentColorSpace,
                bitmapInfo: CGBitmapInfo(rawValue: CGImageAlphaInfo.premultipliedLast.rawValue),
                provider: provider, decode: nil, shouldInterpolate: false, intent: .defaultIntent)
        else { return }
        context.saveGState()
        context.translateBy(x: CGFloat(x0), y: CGFloat(y1))
        context.scaleBy(x: 1, y: -1)
        context.draw(image, in: CGRect(x: 0, y: 0, width: width, height: height))
        context.restoreGState()
    }
}
