import AppKit

/// The Magic Eraser over MCP (`designs/magic-eraser.md`, `MagicEraser.swift`).
///
/// | Handler | UI path it mirrors |
/// |---|---|
/// | `magicEraser` | the Magic Eraser tool's click (`EditorViewController.magicEraserClicked`) |
extension AgentServer {
    /// This file's MCP tools; merged by AgentServer+Registry.swift.
    static let magicEraserHandlers: [String: ToolHandler] = [
        "magic_eraser": { $0.magicEraser },
    ]

    /// Facts for this file's tools; merged in AgentServer+Registry.swift. A layer's pixels at
    /// every depth and in every colour mode but Bitmap (`BitmapDocument.disabledTools`), gated by
    /// the selection.
    static let magicEraserToolFacts: [String: ToolFacts] = [
        "magic_eraser": [.selectionSensitive, .typedTool, .rasterTool, .floatTool, .cmykTool],
    ]

    /// Mirrors the Magic Eraser's click: the Magic Wand's region from (x, y) — over the layer, or
    /// the composite with sample_all_layers — inside the selection, its alpha lowered by opacity.
    /// The result is computed before any undo group opens, so a click that changes nothing (a
    /// seed off the canvas or outside the selection, an already clear region, opacity 0) answers
    /// `changed: false` with no step at every depth.
    func magicEraser(_ a: [String: Any]) throws -> String {
        let document = try target(a)
        guard let doc = document.doc else { throw ToolError(message: "Document has no image") }
        guard a["blend_mode"] == nil else { throw ToolError(message: MagicEraser.blendRefusal) }
        guard (stringArg(a, "target") ?? "layer") == "layer" else {
            throw ToolError(message: MagicEraser.targetRefusal)
        }
        guard !document.quickMaskActive else {
            throw ToolError(message: OperationScope.quickMaskRefusal)
        }
        let index = try paintLayerIndex(a, document)
        try rejectAdjustmentPixelEdit(document, index)
        // A retained source, a pixels lock (its own or a group's).
        if let reason = scopeRefusal(.pixelOnly, target: .layer, doc: doc, layer: index) {
            throw ToolError(message: reason)
        }
        if let refusal = MagicEraser.transparencyRefusal(doc, layer: index) {
            throw ToolError(message: refusal.message + Self.lockHint(owner: refusal.owner))
        }
        let request = try magicEraserRequest(a)
        if doc.isHighDepth {
            return try precisionMagicEraser(document, layer: index, request: request)
        }
        let mask = selectionMask(document)
        guard let erased = doc.magicErased(index, request: request, mask: mask) else {
            return try jsonResult(["ok": true, "changed": false, "layer": index, "storage": "rgba8"])
        }
        let rasterized = try performPixelEdit(document, MagicEraser.name, pixelLayer: index) {
            current in
            current === doc ? erased : current.magicErased(index, request: request, mask: mask)
        }
        return try pixelEditResult(
            ["ok": true, "changed": true, "layer": index, "storage": "rgba8"],
            layer: index, rasterized: rasterized)
    }

    /// A 16-bit, 32-bit float or colour-mode document: the typed region and `erase_native`. An
    /// Indexed document's erased pixels take its transparent index, else the entry nearest white.
    private func precisionMagicEraser(
        _ document: ImageDocument, layer: Int, request: MagicEraserRequest
    ) throws -> String {
        let capture = try PrecisionEditorCapture(document)
        let result = try capture.source.magicErased(
            layer: layer, request: request, selection: capture.selection,
            width: capture.facade.width, height: capture.facade.height)
        let changed = try capture.publish(result, to: document, name: MagicEraser.name)
        let storage = capture.source.isCmyk ? "cmyka16_unorm"
            : capture.facade.isFloat ? "rgba32_float" : "rgba16_unorm"
        return try jsonResult(["ok": true, "changed": changed, "layer": layer, "storage": storage])
    }

    /// x, y, tolerance (0…255, default 32), contiguous (default true), sample_all_layers (default
    /// false) and opacity (0…1, default 1).
    private func magicEraserRequest(_ a: [String: Any]) throws -> MagicEraserRequest {
        guard let x = doubleArg(a, "x"), let y = doubleArg(a, "y"), x.isFinite, y.isFinite else {
            throw ToolError(message: "magic_eraser requires x and y, the pixel to grow the region from.")
        }
        let tolerance = doubleArg(a, "tolerance") ?? 32
        let opacity = doubleArg(a, "opacity") ?? 1
        guard tolerance.isFinite, (0...255).contains(tolerance), opacity.isFinite,
              (0...1).contains(opacity)
        else { throw ToolError(message: MagicEraser.argumentRefusal) }
        return MagicEraserRequest(
            x: Int(x.rounded(.down)), y: Int(y.rounded(.down)), tolerance: tolerance,
            contiguous: boolArg(a, "contiguous") ?? true,
            sampleAllLayers: boolArg(a, "sample_all_layers") ?? false, opacity: opacity)
    }
}
