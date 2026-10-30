import AppKit

// Image ▸ Adjustments ▸ Equalize (#1), at 8, 16 and 32 bits on RGB and on a CMYK document: one
// table from the summed histograms of the colour planes (the four `1 − ink` plates on CMYK) remaps
// the target so its levels spread evenly. With a selection, a prompt asks whether to equalize the
// selected area only (the default) or the entire image based on the selected area's histogram. The
// core holds the rule (`equalize.rs`); this file is the prompt and the one apply the menu item,
// MCP's `equalize` and an Action replay share.
//
// An 8-bit RGB document runs it as an `OperationCapture` op, so the layer, its mask, one colour
// plane and a saved channel are all targets: the histogram weight is the selection cropped to the
// target's placement (`analysisCoverage(.selection)`), `dr_image_equalize` maps every pixel, and
// the result is written through the selection under "selected" (feathered coverage applied once)
// or over the whole target under "entire". A 16-bit, 32-bit float or CMYK document runs the core's
// selected edit on a raster layer's pixels, the coverage lerp in the core.

/// The prompt's answer: the selected area only, or the entire image from the selected area's
/// histogram.
enum EqualizeScope: String {
    case selected, entire
}

enum Equalize {
    /// The prompt, shown only when a selection exists: nil when cancelled. Automation
    /// answers its first button (`AutomationAlerts`), Selected Area Only.
    static func prompt() -> EqualizeScope? {
        let alert = NSAlert()
        alert.messageText = "Equalize"
        alert.informativeText = "Equalize the selected area only, or the entire image based on "
            + "the selected area's histogram?"
        alert.addButton(withTitle: "Selected Area Only")
        alert.addButton(withTitle: "Entire Image Based on Selected Area")
        alert.addButton(withTitle: "Cancel")
        switch alert.runModal() {
        case .alertFirstButtonReturn: return .selected
        case .alertSecondButtonReturn: return .entire
        default: return nil
        }
    }

    /// Equalizes `target` of `layer` in `document` under its current selection (`scope` nil is
    /// "selected", and with no selection the whole target). One undo step; `record` records the
    /// Action step (the menu item; MCP's dispatcher records its own call). True when a sample
    /// changed.
    static func apply(_ document: ImageDocument, layer: Int, target: PaintTarget,
                      scope: EqualizeScope?, record: Bool) throws -> Bool {
        guard let doc = document.doc else { throw DoctoredCoreError(message: "The document has no image.") }
        let scope = scope ?? .selected
        let hasSelection = document.canvasSelection != nil
        let steps: [ActionStep] = record
            ? .equalize(scope: hasSelection ? scope : nil, target: target.agentName(in: doc)) : .notACommand
        if doc.isHighDepth {
            return try typed(document, layer: layer, target: target, scope: scope, record: steps)
        }
        guard let capture = OperationCapture(doc: doc, layerIndex: layer, target: target,
                                             selection: document.canvasSelection?.maskBytes()) else {
            throw DoctoredCoreError(message: OperationScope.refusalReason(.filter, target: target, doc: doc,
                layerIndex: layer) ?? "Equalize cannot edit this target.")
        }
        let before = document.doc
        var failure: Error?
        document.applyCapturedRewrite(capture, "Equalize", record: steps) {
            do {
                return try byteResult(capture, scope: scope)
            } catch {
                failure = error
                return nil
            }
        }
        if let failure { throw failure }
        return document.doc !== before
    }

    /// An 8-bit RGB document's Equalize of `capture`'s target (file header): the selection cropped
    /// to the placement weighs the histogram, and the result is written through the selection
    /// under "selected" or over the whole target under "entire". nil when nothing changes.
    static func byteResult(_ capture: OperationCapture, scope: EqualizeScope) throws -> RasterDocument? {
        let weight = capture.selection == nil ? nil : try capture.analysisCoverage(.selection)
        guard let result = try capture.source.equalized(mask: weight) else { return nil }
        return capture.writing(result, selection: scope == .selected ? capture.selection : nil)
    }

    /// A 16-bit, 32-bit float or CMYK document: the core's selected edit on a raster layer.
    private static func typed(_ document: ImageDocument, layer: Int, target: PaintTarget,
                              scope: EqualizeScope, record: [ActionStep]) throws -> Bool {
        guard target == .layer else {
            throw DoctoredCoreError(message: "Equalize in a 16-bit, 32-bit or CMYK document works on a "
                + "layer's pixels: target \"\(target.agentName(in: document.doc))\" is unavailable.")
        }
        let capture = try PrecisionEditorCapture(document)
        let fields: [String: Any] = ["layer": layer, "op": "equalize", "params": ["scope": scope.rawValue]]
        let data = try JSONSerialization.data(withJSONObject: fields, options: .sortedKeys)
        let request = String(decoding: data, as: UTF8.self)
        let edit = try SelectedAdjustment.edit(capture, request: request, target: .layer)
        return try capture.publish(edit.run(title: "Equalize"), to: document, name: "Equalize",
                                   record: record)
    }
}
