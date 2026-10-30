import AppKit

private func refinementControl<T: NSView>(_ sheet: RefineSelectionSheetController,
                                          _ id: String, as type: T.Type) throws -> T {
    guard let value = allViews(sheet.view).first(where: { $0.identifier?.rawValue == "refine.\(id)" }) as? T else {
        throw ProbeFailure(description: "Missing refinement control \(id)")
    }
    return value
}

func verifyRefinementWorkspace() throws {
    let document = try fixture()
    defer { document.close() }
    document.setCanvasSelection(CanvasSelection(shape: .rect(CGRect(x: 8, y: 8, width: 21, height: 25)),
        canvasWidth: 90, canvasHeight: 60))
    document.undoManager?.removeAllActions()
    let initial = document.selectionHistory
    let native = document.doc
    let sheet = try RefineSelectionSheetController(document: document)
    try present(sheet, in: editor(document))
    let feather = try refinementControl(sheet, "feather", as: NSTextField.self)
    feather.stringValue = "2"
    _ = feather.sendAction(feather.action, to: feather.target)
    try pumpUntil({ SelectionWorkBudget.scratchBytes == 0 && sheet.inspection.image != nil })
    let afterImage = sheet.inspection.image?.dataProvider?.data as Data?
    try require(document.selectionHistory == initial && document.doc === native,
                "Refinement preview is transient")
    let before = try refinementControl(sheet, "before", as: NSButton.self)
    before.performClick(nil)
    try pumpUntil({ SelectionWorkBudget.scratchBytes == 0 })
    try require(sheet.inspection.image?.dataProvider?.data as Data? != afterImage,
                "Before displays original coverage, distinct from feathered preview")
    before.performClick(nil)
    try pumpUntil({ SelectionWorkBudget.scratchBytes == 0 })
    let backgrounds = try refinementControl(sheet, "background", as: NSPopUpButton.self)
    for index in 0..<5 {
        backgrounds.selectItem(at: index)
        _ = backgrounds.sendAction(backgrounds.action, to: backgrounds.target)
        try pumpUntil({ SelectionWorkBudget.scratchBytes == 0 })
        try require(sheet.inspection.background.rawValue == index && sheet.inspection.image != nil,
                    "All five inspection backgrounds render")
    }
    let actual = try refinementControl(sheet, "actual", as: NSButton.self)
    actual.performClick(nil)
    let scroll = allViews(sheet.view).compactMap { $0 as? NSScrollView }.first
    try require(scroll?.magnification == 1, "100% control uses one image pixel per point")
    let expected = try sheet.capture.selection.source?.refiningSelection(sheet.capture.coverage,
        parameters: sheet.parameters.coreParameters)
    let apply = try refinementControl(sheet, "apply", as: NSButton.self)
    apply.performClick(nil)
    try require(document.canvasSelection?.maskBytes() == expected && document.doc === native
                && !document.isDocumentEdited && document.undoManager?.canUndo == true,
                "Actual refinement Apply creates reversible coverage without changing native pixels")
    document.undoManager?.undo()
    try require(document.selectionHistory == initial && document.undoManager?.canUndo != true,
                "Refinement has one exact selection Undo")
    let cancelled = try RefineSelectionSheetController(document: document)
    try present(cancelled, in: editor(document))
    cancelled.requestPreview()
    cancelled.cancelClicked(nil)
    try pumpUntil({ SelectionWorkBudget.scratchBytes == 0 })
    try require(document.selectionHistory == initial, "Cancel preserves original selection")

    // The edge stage on a canvas past the four stages' 10-bytes-a-pixel preview budget:
    // a selection whose right edge lies 4 px past the fixture's red/green boundary.
    let wide = try fixture(width: 256, height: 192)
    defer { wide.close() }
    wide.setCanvasSelection(CanvasSelection(shape: .rect(CGRect(x: 40, y: 30, width: 50, height: 90)),
        canvasWidth: 256, canvasHeight: 192))
    wide.undoManager?.removeAllActions()
    let wideInitial = wide.selectionHistory
    let edgeSheet = try RefineSelectionSheetController(document: wide)
    try present(edgeSheet, in: editor(wide))
    try pumpUntil({ SelectionWorkBudget.scratchBytes == 0 && edgeSheet.inspection.image != nil })
    let radius = try refinementControl(edgeSheet, "edgeRadius", as: NSTextField.self)
    radius.stringValue = "8"
    _ = radius.sendAction(radius.action, to: radius.target)
    try pumpUntil({ SelectionWorkBudget.scratchBytes == 0 })
    let edgeApply = try refinementControl(edgeSheet, "apply", as: NSButton.self)
    try require(edgeSheet.parameters.coreParameters["edge_radius"] as? Double == 8
                && edgeSheet.inspection.image != nil && edgeApply.isEnabled,
                "An Edge Detection radius previews on a 256×192 canvas and enables Apply")
    edgeApply.performClick(nil)
    try require(wide.selectionHistory != wideInitial && wide.undoManager?.canUndo == true
                && !wide.isDocumentEdited,
                "Applying an Edge Detection radius changes the selection and no pixel")
    wide.undoManager?.undo()
    try require(wide.selectionHistory == wideInitial && wide.undoManager?.canUndo != true,
                "Edge refinement has one exact selection Undo")

    // The same at 16 bits, with a coverage stage after the edge stage: the typed preview plans
    // both on the main thread and its worker runs one after the other under one reservation.
    let deep = try fixture(width: 256, height: 192)
    defer { deep.close() }
    _ = try call(deep, "convert_precision", ["mode": "rgba16", "structure": "preserve"])
    deep.setCanvasSelection(CanvasSelection(shape: .rect(CGRect(x: 40, y: 30, width: 50, height: 90)),
        canvasWidth: 256, canvasHeight: 192))
    deep.undoManager?.removeAllActions()
    let deepInitial = deep.selectionHistory
    let deepSheet = try RefineSelectionSheetController(document: deep)
    try require(deepSheet.capture.isNative, "A 16-bit document refines its native coverage")
    try present(deepSheet, in: editor(deep))
    try pumpUntil({ SelectionWorkBudget.scratchBytes == 0 && deepSheet.inspection.image != nil })
    for (id, value) in [("edgeRadius", "8"), ("feather", "1")] {
        let field = try refinementControl(deepSheet, id, as: NSTextField.self)
        field.stringValue = value
        _ = field.sendAction(field.action, to: field.target)
    }
    try pumpUntil({ SelectionWorkBudget.scratchBytes == 0 })
    let deepApply = try refinementControl(deepSheet, "apply", as: NSButton.self)
    try require(deepSheet.inspection.image != nil && deepApply.isEnabled,
                "An Edge Detection radius with Feather previews at 16 bits and enables Apply")
    deepApply.performClick(nil)
    try require(deep.canvasSelection?.hasNativeCoverage == true && deep.selectionHistory != deepInitial,
                "Applying at 16 bits commits native refined coverage")
    deep.undoManager?.undo()
    try require(deep.selectionHistory == deepInitial, "The 16-bit edge refinement has one exact Undo")
    print("PASS: refinement workspace before/after, five backgrounds, 100%, actual Apply/Cancel, exact Undo and an edge-radius preview and Apply at 8 and 16 bits")
}

func verifyRefinementMaskOutput() throws {
    let document = try fixture()
    defer { document.close() }
    document.setCanvasSelection(CanvasSelection(shape: .rect(CGRect(x: 8, y: 8, width: 21, height: 25)),
        canvasWidth: 90, canvasHeight: 60))
    document.undoManager?.removeAllActions()
    guard let original = document.doc, let pixels = original.layerPixels(0) else {
        throw ProbeFailure(description: "Original layer pixels")
    }
    let sheet = try RefineSelectionSheetController(document: document)
    try present(sheet, in: editor(document))
    let output = try refinementControl(sheet, "output", as: NSPopUpButton.self)
    output.selectItem(at: 1)
    _ = output.sendAction(output.action, to: output.target)
    try pumpUntil({ SelectionWorkBudget.scratchBytes == 0 && sheet.inspection.image != nil })
    let apply = try refinementControl(sheet, "apply", as: NSButton.self)
    apply.performClick(nil)
    try require(document.doc?.layerHasMask(0) == true && document.isDocumentEdited,
                "Actual mask output creates native editable mask")
    try require(document.doc?.layerPixels(0) == pixels,
                "Refinement output preserves exact original layer pixels")
    let properties = document.doc?.layerMaskProperties(0)
    let coverage = document.doc?.layerMaskCoverage(0)
    let folder = ProcessInfo.processInfo.environment["DR_SELECTION_PROBE_DIR"] ?? NSTemporaryDirectory()
    let path = URL(fileURLWithPath: folder).appendingPathComponent("refined-mask.rz")
    try document.doc?.saveNative(to: path)
    let reopened = try RasterDocument.open(url: path)
    try require(reopened.layerMaskCoverage(0) == coverage && reopened.layerMaskProperties(0) == properties,
                "Native reopen preserves refined raw mask and editable properties")
    document.undoManager?.undo()
    try require(document.doc === original && document.undoManager?.canUndo != true,
                "Mask output has one native Undo")
    let response = try call(document, "refine_selection", ["feather": 2, "output": "mask"])
    try require(response["changed"] as? Bool == true && document.doc?.layerHasMask(0) == true,
                "MCP shares refinement mask output")
    let unchanged = document.doc, historyCount = document.history.entries.count
    let repeated = try call(document, "refine_selection", ["feather": 2, "output": "mask"])
    try require(repeated["changed"] as? Bool == false && document.doc === unchanged
                && document.history.entries.count == historyCount,
                "Repeating identical refinement mask output retains the handle and adds no Undo step")
    let stale = try SelectionRefinementCapture(document)
    document.setCanvasSelection(nil)
    do {
        _ = try stale.apply(SelectionRefinementParameters(feather: 1), output: .mask,
            in: document, record: .notACommand)
        throw ProbeFailure(description: "Stale refinement capture committed")
    } catch is DoctoredCoreError {}
    print("PASS: refinement mask UI/MCP output, unchanged source pixels, native round trip and stale capture refusal")
}
