import AppKit

extension AgentServer {
    func byteStrokePoints(_ arguments: [String: Any]) throws -> [CGPoint] {
        if arguments["points"] != nil { return try parsePoints(arguments) }
        guard let canonical = try capturedNativeStroke(arguments), canonical.dabs.count >= 2 else {
            return try parsePoints(arguments)
        }
        return [CGPoint(x: canonical.dabs[0], y: canonical.dabs[1])]
    }

    func byteCanonicalRetouchPixels(_ document: ImageDocument,
                                    stroke: NativeBrushStroke, size: CGFloat,
                                    ink: ByteDabInk) throws -> [UInt8] {
        guard let doc = document.doc else { throw ToolError(message: "Document has no image") }
        return try BytePaintRasterizer.pixels(stroke: stroke, size: size, ink: ink,
            width: doc.width, height: doc.height, space: doc.drawingSpace,
            selection: editor(document)?.agentSelection, coverageOpacity: nil)
    }

    func byteCanonicalRetouchCommit(
        _ document: ImageDocument, layer: Int, actionName: String,
        rasterizesTarget: Bool = true, pixels: [UInt8],
        commit: (RasterDocument, UnsafePointer<UInt8>, Int, Int) -> RasterDocument?
    ) throws -> DroppedDescription? {
        guard let doc = document.doc else { throw ToolError(message: "Document has no image") }
        return try performPixelEdit(document, actionName, pixelLayer: layer,
                                    rasterizesTarget: rasterizesTarget) { current in
            pixels.withUnsafeBufferPointer { buffer in
                guard let base = buffer.baseAddress else { return nil }
                return commit(current, base, doc.width, doc.height)
            }
        }
    }

    /// Canonical byte brush and eraser replay shares the canvas's admitted
    /// premultiplied overlay construction and the existing core writeback.
    func byteCanonicalPaint(
        _ document: ImageDocument, layer: Int, target: PaintTarget,
        actionName: String, color: NSColor, size: CGFloat, opacity: Double,
        blend: DrBlendMode?, stroke: NativeBrushStroke, erase: Bool
    ) throws -> String {
        guard let doc = document.doc else { throw ToolError(message: "Document has no image") }
        if target == .mask { try rejectLockedEdit(document, layer, DR_EDIT_MASK) }
        let ink: ByteDabInk = target.isCoverage
            ? .color(ColorProfile.coverageInk(erase ? .black : .white, in: doc.drawingSpace))
            : .color(color)
        let pixels = try BytePaintRasterizer.pixels(stroke: stroke, size: size,
            ink: ink, width: doc.width, height: doc.height, space: doc.drawingSpace,
            selection: editor(document)?.agentSelection,
            coverageOpacity: target.isCoverage ? opacity : nil)
        // The colour brush's Blend option blends its exact straight colour,
        // converted once, before the edit opens.
        var blendRGB: [UInt8] = []
        if let blend, blend != DR_BLEND_NORMAL, !target.isCoverage {
            guard let bytes = ColorProfile.bytes(color, in: document.nsColorSpace) else {
                throw ToolError(message: "Could not convert the paint color.")
            }
            blendRGB = Array(bytes.prefix(3))
        }
        var opRefused = false
        let rasterized: DroppedDescription?
        do {
            rasterized = try performPixelEdit(document, actionName,
                pixelLayer: pixelLayer(for: target, layer: layer)) { current in
                pixels.withUnsafeBufferPointer { buffer -> RasterDocument? in
                    guard let base = buffer.baseAddress else { return nil }
                    if target.isCoverage {
                        return self.commitCoverage(current, target: target, layer: layer,
                            overlay: base, width: doc.width, height: doc.height,
                            onRefusal: { opRefused = true })
                    }
                    if let blend, blend != DR_BLEND_NORMAL {
                        let next = current.paintingLayerBlendColor(layer, overlay: base,
                            w: doc.width, h: doc.height, rgb: blendRGB, mode: blend,
                            alpha: opacity)
                        if next == nil { opRefused = true }
                        return next
                    }
                    return current.paintingLayer(layer, overlay: base,
                        w: doc.width, h: doc.height,
                        mode: erase ? DR_COMPOSITE_ERASE : DR_COMPOSITE_OVER,
                        alpha: opacity)
                }
            }
        } catch let error as ToolError {
            guard opRefused else { throw error }
            return try noOpStrokeResult(document, target: target, layer: layer, blend: blend)
        }
        return try pixelEditResult([
            "ok": true, "action": actionName, "layer": layer,
            "dabs": stroke.dabs.count / 4,
            "target": target.agentName(in: document.doc),
            "coverage": "RZBR v1 byte dabs",
        ], layer: layer, rasterized: rasterized)
    }
}
