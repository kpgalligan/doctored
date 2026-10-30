import AppKit
import ObjectiveC

private struct RawMaskState: Equatable {
    let properties: LayerMaskProperties
    let rgba: [UInt8]
}
private func rawMask(_ document: RasterDocument, layer: Int = 0) throws -> RawMaskState {
    guard let properties = document.layerMaskProperties(layer), let mask = document.layerMaskStorage(layer),
          let pixels = RasterDocument.from(image: mask.image)?.layerPixels(0) else {
        throw ProbeFailure(description: "Raw mask storage missing")
    }
    return RawMaskState(properties: properties, rgba: pixels)
}
private func field(_ id: String, in view: NSView) throws -> NSTextField {
    guard let field = allViews(view).compactMap({ $0 as? NSTextField }).first(where: {
        $0.identifier?.rawValue == id
    }) else { throw ProbeFailure(description: "Missing field \(id)") }
    return field
}
private func button(_ title: String, in view: NSView) throws -> NSButton {
    guard let button = allViews(view).compactMap({ $0 as? NSButton }).first(where: { $0.title == title }) else {
        throw ProbeFailure(description: "Missing button \(title)")
    }
    return button
}
func capturedPresentationError(_ body: () throws -> Void) throws -> [NSError] {
    guard let method = class_getInstanceMethod(NSApplication.self, #selector(NSApplication.presentError(_:))) else {
        throw ProbeFailure(description: "Error presentation method missing")
    }
    var errors: [NSError] = []
    let callback: @convention(block) (AnyObject, NSError) -> Bool = { _, error in errors.append(error); return false }
    let replacement = imp_implementationWithBlock(callback)
    let original = method_setImplementation(method, replacement)
    defer { method_setImplementation(method, original); imp_removeBlock(replacement) }
    try body()
    return errors
}

func verifyIndependentMasks() throws {
    let document = try fixture()
    defer { document.close() }
    let controller = try editor(document)
    var coverage = [UInt8](repeating: 0, count: 90 * 60)
    for y in 12..<40 { for x in 10..<30 { coverage[y * 90 + x] = 255 } }
    guard let base = document.doc?.addingLayerMask(0, kind: DR_MASK_FROM_SELECTION, selection: coverage) else {
        throw ProbeFailure(description: "Initial mask allocation")
    }
    document.doc = base
    document.updateChangeCount(.changeCleared)
    document.undoManager?.removeAllActions()
    let original = try rawMask(base), originalPixels = base.layerPixels(0)
    let properties = try MaskPropertiesSheetController(document: document, canvas: controller.canvas)
    try present(properties, in: controller)
    let density = try field("maskProperties.density", in: properties.view)
    let feather = try field("maskProperties.feather", in: properties.view)
    density.stringValue = "40"
    _ = density.sendAction(density.action, to: density.target)
    feather.stringValue = "2"
    _ = feather.sendAction(feather.action, to: feather.target)
    guard let link = allViews(properties.view).compactMap({ $0 as? NSButton }).first(where: {
        $0.identifier?.rawValue == "maskProperties.linked"
    }) else { throw ProbeFailure(description: "Mask link control") }
    link.performClick(nil)
    try pumpUntil({ SelectionWorkBudget.scratchBytes == 0 })
    try require(document.doc === base && !document.isDocumentEdited,
                "Actual mask-properties previews leave raw state and dirty marker unchanged")
    try button("Apply", in: properties.view).performClick(nil)
    guard let modified = document.doc else { throw ProbeFailure(description: "Mask Properties result") }
    let applied = try rawMask(modified)
    try require(abs(applied.properties.density - 0.4) < 0.00001 && applied.properties.feather == 2
                && !applied.properties.linked, "Actual density/feather/link controls persist editable properties")
    try require(applied.rgba == original.rgba && modified.layerPixels(0) == originalPixels,
                "Mask properties change neither original mask coverage nor layer pixels")
    try require(document.history.entries.count == 2 && document.isDocumentEdited,
                "Mask Properties Apply is one native edit")
    document.undoManager?.undo()
    try require(document.doc === base && !document.isDocumentEdited && document.undoManager?.canUndo != true,
                "One Undo restores exact original mask properties and clean saved identity")
    document.undoManager?.removeAllActions()
    print("PASS: actual mask property preview/Apply, raw coverage preservation, editable density/feather/link and single native Undo")

    let transform = try MaskTransformSheetController(document: document, canvas: controller.canvas)
    try present(transform, in: controller)
    let dx = try field("maskTransform.translate_x", in: transform.view)
    dx.stringValue = "15"
    _ = dx.sendAction(dx.action, to: dx.target)
    try pumpUntil({ SelectionWorkBudget.scratchBytes == 0 })
    try require(document.doc === base, "Mask transform preview is transient")
    try button("Apply", in: transform.view).performClick(nil)
    guard let movedDoc = document.doc else { throw ProbeFailure(description: "Mask transform result") }
    let moved = try rawMask(movedDoc)
    try require(moved.properties.offsetX == original.properties.offsetX + 15
                && moved.properties.offsetY == original.properties.offsetY
                && moved.rgba == original.rgba && movedDoc.layerPixels(0) == originalPixels,
                "Actual mask-only translation moves stored coverage independently of layer pixels")
    try require(moved.properties.linked == original.properties.linked
                && moved.properties.density == original.properties.density
                && moved.properties.feather == original.properties.feather,
                "Mask-only transform preserves rendering properties")
    document.undoManager?.undo()
    try require(document.doc === base && document.undoManager?.canUndo != true,
                "Actual Mask Transform has one exact Undo")
    let cancelled = try MaskTransformSheetController(document: document, canvas: controller.canvas)
    try present(cancelled, in: controller)
    let cancelDX = try field("maskTransform.translate_x", in: cancelled.view)
    cancelDX.stringValue = "30"
    _ = cancelDX.sendAction(cancelDX.action, to: cancelDX.target)
    try button("Cancel", in: cancelled.view).performClick(nil)
    print("MASK CANCEL immediately: unchanged=\(document.doc === base), overlay=\(controller.canvas.previewImage != nil), scratch=\(SelectionWorkBudget.scratchBytes)")
    try pumpUntil({ SelectionWorkBudget.scratchBytes == 0 })
    print("MASK CANCEL after drain: unchanged=\(document.doc === base), overlay=\(controller.canvas.previewImage != nil), attached=\(cancelled.view.window?.sheetParent != nil)")
    try require(document.doc === base, "Mask transform Cancel leaves native state intact")
    try require(controller.canvas.previewImage == nil,
                "Mask transform Cancel drops pending preview and prevents later overlay publication")
    print("PASS: actual independent Mask Transform preview/Apply/Undo/Cancel with exact raw pixels and properties")

    let stale = try MaskPropertiesSheetController(document: document, canvas: controller.canvas)
    try present(stale, in: controller)
    let staleDensity = try field("maskProperties.density", in: stale.view)
    document.applyEdit("New mask target revision", record: .notACommand) { try $0.withLayerName(0, "Changed behind sheet") }
    let newer = document.doc, count = document.history.entries.count
    staleDensity.stringValue = "30"
    _ = staleDensity.sendAction(staleDensity.action, to: staleDensity.target)
    try require(allViews(stale.view).compactMap({ $0 as? NSTextField }).contains(where: {
        $0.stringValue.contains("document changed")
    }), "Changed native target rejects stale preview with a visible message")
    let errors = try capturedPresentationError { try button("Apply", in: stale.view).performClick(nil) }
    try require(errors.contains(where: { $0.localizedDescription.contains("document changed") })
                && document.doc === newer && document.history.entries.count == count,
                "Actual stale Mask Properties Apply reports refusal without mutating newer state")
    try button("Cancel", in: stale.view).performClick(nil)
    print("PASS: stale mask preview and actual Apply refuse a changed target without adding history")

    let payload = ShapeLayerPayload(kind: "rect", w: 20, h: 15, fill: .red, stroke: nil, strokeWidth: 0)
    guard let shape = try base.addingDescribedLayer(above: 0, .shape(payload),
        anchor: CGPoint(x: 10, y: 10), name: "Editable shape")?
        .addingLayerMask(1, kind: DR_MASK_FROM_SELECTION, selection: coverage)?
        .settingMaskProperties(1, linked: false, density: 0.45, feather: 2)?
        .transformingMask(1, matrix: CGAffineTransform(translationX: 7, y: -3)) else {
        throw ProbeFailure(description: "Described independent-mask fixture")
    }
    let shapeMask = try rawMask(shape, layer: 1)
    var changedPayload = payload
    changedPayload.w = 34
    changedPayload.fill = "#00FF00"
    guard let rerendered = shape.rerenderingDescribedLayer(1, .shape(changedPayload),
        anchor: CGPoint(x: 30, y: 20)) else { throw ProbeFailure(description: "Described rerender refused") }
    let rerenderedMask = try rawMask(rerendered, layer: 1)
    try require(rerenderedMask == shapeMask,
                "Described content rerender preserves independent raw mask extent, placement and rendering properties")
    try require(rerendered.shapePayload(1)?.w == 34 && rerendered.layerPixels(1) != shape.layerPixels(1),
                "Described content actually rerenders while independent mask stays fixed")
    print("PASS: editable shape rerender preserves independent mask bytes/placement/density/feather")
}

func verifyMaskPropertiesPrecision() throws {
    let document = try fixture()
    defer { document.close() }
    guard let original = try document.doc?.addingLayerMask(0, kind: DR_MASK_REVEAL_ALL)?
        .settingMaskProperties(0, linked: false, density: 0.654321, feather: 2.123456) else {
        throw ProbeFailure(description: "Precise imported mask properties")
    }
    document.doc = original
    document.updateChangeCount(.changeCleared)
    document.undoManager?.removeAllActions()
    let sheet = try MaskPropertiesSheetController(document: document, canvas: editor(document).canvas)
    try present(sheet, in: editor(document))
    try button("Apply", in: sheet.view).performClick(nil)
    try require(document.doc === original && document.undoManager?.canUndo != true && !document.isDocumentEdited,
                "Opening and applying unchanged precise mask properties preserves original Float values and creates no Undo")
    try pumpUntil({ SelectionWorkBudget.scratchBytes == 0 })
    let precisionCanvas = try editor(document).canvas
    try require(precisionCanvas.previewImage == nil,
                "Unchanged Apply cannot revive a preview while dismissing its focused field")
    let entered = try MaskPropertiesSheetController(document: document, canvas: editor(document).canvas)
    try present(entered, in: editor(document))
    let feather = try field("maskProperties.feather", in: entered.view)
    feather.stringValue = "3.876543"
    _ = feather.sendAction(feather.action, to: feather.target)
    try pumpUntil({ SelectionWorkBudget.scratchBytes == 0 })
    try button("Apply", in: entered.view).performClick(nil)
    try require(document.doc?.layerMaskProperties(0)?.feather == Double(Float(3.876543))
                && document.doc?.layerMaskProperties(0)?.density == original.layerMaskProperties(0)?.density,
                "Typed precise feather is committed exactly as previewed while untouched density retains its original Float")
    let beforeCancel = document.doc
    let cancelled = try MaskPropertiesSheetController(document: document, canvas: precisionCanvas)
    try present(cancelled, in: editor(document))
    let density = try field("maskProperties.density", in: cancelled.view)
    density.stringValue = "20"
    _ = density.sendAction(density.action, to: density.target)
    try button("Cancel", in: cancelled.view).performClick(nil)
    try pumpUntil({ SelectionWorkBudget.scratchBytes == 0 })
    try require(document.doc === beforeCancel && precisionCanvas.previewImage == nil,
                "Mask Properties Cancel also prevents late field actions from reviving a preview")
    print("PASS: unchanged precise mask properties survive formatted fields without a phantom native edit")
}
