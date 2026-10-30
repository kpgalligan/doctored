import AppKit

enum NativeMaskApplication {
    static func validate(_ doc: RasterDocument, layer: Int) throws {
        guard let info = doc.layerInfo(layer), !info.isGroup, !doc.layerIsAdjustment(layer), doc.layerHasMask(layer) else {
            throw DoctoredCoreError(message: "Apply Mask requires a raster layer with its own mask.")
        }
        guard !doc.hasRetainedSource(layer) else { throw DoctoredCoreError(message: RetainedImagePolicy.rasterizeFirst) }
        if doc.hasArtworkDescription(layer), doc.layerMeta(layer) == nil {
            throw DoctoredCoreError(message: "The artwork description could not be read. Rasterize the artwork explicitly before applying its mask.")
        }
        if let refusal = doc.lockRefusal(layer, kind: DR_EDIT_MASK_APPLY) { throw DoctoredCoreError(message: refusal.message) }
    }

    static func edit(_ capture: PrecisionEditorCapture, layer: Int) throws -> PrecisionScalarWork {
        guard !capture.quickMask else { throw DoctoredCoreError(message: OperationScope.quickMaskRefusal) }
        try validate(capture.facade, layer: layer)
        return try PrecisionScalarWork(source: capture.source,
            command: PrecisionPixelCommand(["op": "apply_mask", "layer": layer]))
    }
}

extension EditorViewController {
    func applyNativeLayerMask() {
        guard let document else { return }
        do {
            let capture = try PrecisionEditorCapture(document)
            try NativeMaskApplication.validate(capture.facade, layer: capture.layer)
            guard document.confirmRasterize(layer: capture.layer) else { return }
            try capture.requireCurrent(document)
            let edit = try NativeMaskApplication.edit(capture, layer: capture.layer)
            _ = try capture.publish(edit.run(title: "Apply Layer Mask"), to: document, name: "Apply Layer Mask",
                record: .removeLayerMask(apply: true))
        } catch { document.presentError(error) }
    }
}

extension AgentServer {
    /// Mirrors Layer > Apply Layer Mask, including disabled-mask coverage, at
    /// 16 bits and 32-bit float, where alpha becomes
    /// alpha · coverage in f32 and RGB is kept bit for bit; the reply's
    /// `storage` says which.
    func applyNativeLayerMask(_ a: [String: Any], document: ImageDocument) throws -> String {
        let capture = try PrecisionEditorCapture(document)
        let layer = try maskedLayerIndex(a, document, "apply")
        let edit = try NativeMaskApplication.edit(capture, layer: layer)
        let changed = try capture.publish(edit.run(title: "Apply Layer Mask"), to: document, name: "Apply Layer Mask")
        return try jsonResult(["ok": true, "changed": changed, "layer": layer, "applied": true,
            "rasterized": changed && capture.facade.hasArtworkDescription(layer),
            "storage": capture.facade.isFloat ? "rgba32_float"
                : capture.source.isCmyk ? "cmyka16_unorm" : "rgba16_unorm"])
    }
}
