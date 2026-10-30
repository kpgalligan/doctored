import AppKit

/// Layer Via Copy / Layer Via Cut on a 16-bit document and
/// a 32-bit float one: the core's typed `layer_via` request
/// (`precision_layer_via.rs`), its canvas action and its MCP branch. At float
/// the copy keeps every RGB sample bit for bit and takes alpha `a·c/65535`,
/// and the cut is the float Cut's own erase.
///
/// The request is the byte `dr_doc_layer_via` at 16 bits. With a selection it
/// carries the canvas-sized u16 coverage (`selection: true`); without one —
/// which only the MCP tool can ask, because ⌘J with no selection duplicates —
/// it sends `selection: false` and no samples, and the core copies every
/// layer pixel, on or off the canvas. Like the byte op it rasterizes: the new
/// layer carries no description, style, mask or Blend If.
extension PrecisionPixelCommand {
    /// The one builder of the `layer_via` request; all five keys are required
    /// by the core.
    static func layerVia(layer: Int, cut: Bool, name: String, selection: Bool) throws -> Self {
        try Self(["op": "layer_via", "layer": layer, "cut": cut, "name": name,
                  "selection": selection])
    }
}

extension EditorViewController {
    /// ⌘J / ⇧⌘J at 16 bits, once `layerViaCopy` / `layerViaCut`
    /// (EditorViewController+MergeStamp.swift) have applied their routing
    /// rule: one plain raster entry, alone, with a live selection. Follows
    /// the 16-bit Clear (`clearPrecisionSelection`): capture, coverage, the
    /// typed edit, one publish. A cut asks the Pixels lock and then the
    /// rasterize prompt, in that order, exactly as Clear and the 8-bit cut do.
    func precisionLayerVia(cut: Bool) {
        guard let document else {
            NSSound.beep()
            return
        }
        let title = cut ? "Layer Via Cut" : "Layer Via Copy"
        do {
            let capture = try PrecisionEditorCapture(document)
            guard capture.selection != nil else {
                throw DoctoredCoreError(message: "Make a selection first.")
            }
            let layer = capture.layer
            if cut, refuseLockedEdit(layer: layer, kind: DR_EDIT_PIXELS) { return }
            let name = viaLayerName(capture.facade, layer: layer)
            // Where the copy lands, from the stack BEFORE the edit.
            let landing = capture.facade.insertionIndex(above: layer)
            let coverage = try PrecisionStrokeCoverage.selection(
                width: capture.facade.width, height: capture.facade.height,
                selection: capture.selection)
            let edit = try PrecisionPixelEdit(
                capture.source,
                command: .layerVia(layer: layer, cut: cut, name: name, selection: true),
                input: coverage)
            // Only the cut rewrites the source, so only it asks.
            if cut, !document.confirmRasterize(layer: layer) { return }
            try capture.requireCurrent(document)
            let changed = try capture.publish(
                edit.run(title: title), to: document, name: title,
                record: .layerVia(cut: cut, name: name))
            guard changed else {
                // A frozen alpha channel makes the cut an exact no-op (the
                // 8-bit action's own explanation); otherwise the selection
                // covers no pixel of the layer.
                if cut { refuseFrozenAlpha(layer: layer) } else { NSSound.beep() }
                return
            }
            setActiveLayer(min(landing, document.doc.layerCount - 1))
        } catch { presentPrecisionError(error) }
    }
}

extension AgentServer {
    /// `layer_via_copy` / `layer_via_cut` on a 16-bit or 32-bit float
    /// document (the reply's `storage` says which). Mirrors Layer
    /// ▸ Layer Via Copy (⌘J) / Via Cut (⇧⌘J) at 16 bits
    /// (`EditorViewController.precisionLayerVia`), and like the 8-bit handler
    /// it does not route: with no selection the whole layer is copied
    /// (`selection: false`), and the reply says so.
    func precisionLayerVia(_ a: [String: Any], document: ImageDocument, layer: Int,
                           cut: Bool, name: String) throws -> String {
        let title = cut ? "Layer Via Cut" : "Layer Via Copy"
        let capture = try PrecisionEditorCapture(document)
        if cut {
            // The 8-bit handler's two refusals (`performPixelEdit`), by name.
            if capture.facade.hasRetainedSource(layer) {
                throw ToolError(message: RetainedImagePolicy.rasterizeFirst)
            }
            try rejectLockedEdit(document, layer, DR_EDIT_PIXELS)
        }
        let selected = capture.selection != nil
        let coverage = selected
            ? try PrecisionStrokeCoverage.selection(
                width: capture.facade.width, height: capture.facade.height,
                selection: capture.selection)
            : nil
        let landing = capture.facade.insertionIndex(above: layer)
        // The cut rewrites the source's pixels, so a described layer loses its
        // description; reported exactly as the 8-bit handler reports it.
        let dropped = cut ? Self.description(of: document, layer: layer) : nil
        let edit = try PrecisionPixelEdit(
            capture.source,
            command: .layerVia(layer: layer, cut: cut, name: name, selection: selected),
            input: coverage)
        let changed = try capture.publish(edit.run(title: title), to: document, name: title)
        guard changed else {
            throw frozenAlphaRefusal(document, cut ? layer : nil, DR_EDIT_PIXELS, title)
                ?? ToolError(message: "\(title) copied nothing: no "
                    + (selected ? "selected " : "") + "pixel of layer \(layer) has any "
                    + "coverage, so no layer was added and no undo step was made.")
        }
        let created = min(landing, (document.doc?.layerCount ?? 1) - 1)
        retarget(document, .single(created))
        var reply: [String: Any] = ["ok": true, "layer": created, "name": name,
                                    "storage": capture.facade.isFloat
                                        ? "rgba32_float" : "rgba16_unorm"]
        if !selected {
            reply["note"] =
                "No selection was active, so the whole layer was copied. This op "
                + "rasterizes: the new layer keeps neither a text/shape description nor a "
                + "layer style — duplicate_layer keeps both."
        }
        return try pixelEditResult(reply, layer: layer, rasterized: dropped)
    }
}
