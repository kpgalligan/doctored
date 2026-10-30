import AppKit

// Phase 12E-2b: the UI-SOURCE half of the typed NEW LAYER. An MCP drive can
// call `new_layer`; what only the actual panel can prove lives here — that
// the layers panel's footer button is ENABLED on a 16-bit document at all
// (it was hard-disabled there until this phase), that ONE click makes ONE
// transparent record ABOVE the active entry through the very nil-target
// selector the Layer menu uses, that the click is ONE history step, and that
// one Undo restores the stack. The 8-bit document is measured beside it so
// "byte is unchanged" is an assertion rather than a claim.

/// Every record's name, nesting level and clipped flag, walked off the
/// document itself rather than read from any report.
private func table(_ document: ImageDocument) throws -> [[String: Any]] {
    guard let doc = document.doc else {
        throw DoctoredCoreError(message: "Owned document handle missing")
    }
    return try (0..<doc.layerCount).map { index in
        guard let info = doc.layerInfo(index) else {
            throw DoctoredCoreError(message: "Owned layer row missing")
        }
        return ["name": info.name, "level": info.depth, "clipped": doc.layerClipped(index)]
    }
}

/// True when EVERY sample of layer `index` is zero — what "transparent"
/// means, asked of the record's own storage at whichever depth it is kept.
private func isFullyTransparent(_ document: ImageDocument, _ index: Int) throws -> Bool {
    guard let doc = document.doc else {
        throw DoctoredCoreError(message: "Owned document handle missing")
    }
    if doc.isHighDepth {
        for plane in ["red", "green", "blue", "alpha"] {
            let read = try PrecisionScalarOperations.read(doc, target: plane, index: index)
            if read.samples.contains(where: { $0 != 0 }) { return false }
        }
        return true
    }
    guard let bytes = doc.layerPixels(index) else {
        throw DoctoredCoreError(message: "Owned byte layer pixels missing")
    }
    return !bytes.contains(where: { $0 != 0 })
}

/// One document's worth of the claim, at whichever depth `openDocument` made.
private func clickNewLayer(_ document: ImageDocument, _ editor: EditorViewController,
                           depth: String) throws -> [String: Any] {
    guard let panel = editor.layersPanel else {
        throw DoctoredCoreError(message: "Actual layers panel missing")
    }
    _ = panel.view
    pump()
    let button = panel.newLayerButton
    try require(button.isEnabled,
        "The New Layer button is disabled on a \(depth) document")
    try require(button.action == #selector(EditorViewController.newLayer(_:)),
        "The New Layer button no longer sends newLayer:")
    try require(button.target == nil,
        "The New Layer button no longer resolves through the responder chain")

    let before = try table(document)
    let active = document.activeLayerIndex
    let steps = document.history.entries.count
    document.undoManager?.removeAllActions()

    // ONE click. The button's target is nil, so a real click resolves
    // through the responder chain to the editor — and a headless probe has no
    // key window for AppKit to resolve it through, so the receiver that
    // resolution reaches is called directly, with the button as the sender.
    // The selector is the button's OWN, asserted above, so this is the click's
    // effect and not a second path.
    editor.newLayer(button)
    pump()

    let after = try table(document)
    try require(after.count == before.count + 1,
        "One click made \(after.count - before.count) records on a \(depth) document")
    // ABOVE the active entry, at its own level: the slot rule's `above`.
    let landing = active + 1
    try require(document.activeLayerIndex == landing,
        "The new record is not selected on a \(depth) document")
    guard let level = after[landing]["level"] as? Int,
          let previousLevel = before[active]["level"] as? Int else {
        throw DoctoredCoreError(message: "Owned level rows missing")
    }
    try require(level == previousLevel,
        "The new record landed at level \(level) rather than the anchor's on \(depth)")
    try require(after[landing]["clipped"] as? Bool == false,
        "The new record joined a clip run that does not exist on \(depth)")
    for index in 0..<before.count {
        let mirror = index < landing ? after[index] : after[index + 1]
        try require(mirror["name"] as? String == before[index]["name"] as? String
            && mirror["level"] as? Int == before[index]["level"] as? Int,
            "A neighbour moved on a \(depth) document")
    }
    try require(try isFullyTransparent(document, landing),
        "The new record is not transparent on a \(depth) document")
    try require(document.history.entries.count == steps + 1,
        "One click published \(document.history.entries.count - steps) history steps on \(depth)")

    document.undoManager?.undo()
    pump()
    let restored = try table(document)
    try require(restored.count == before.count,
        "One Undo did not restore the stack on a \(depth) document")
    for index in 0..<before.count {
        try require(restored[index]["name"] as? String == before[index]["name"] as? String,
            "One Undo left a different stack on a \(depth) document")
    }
    document.undoManager?.removeAllActions()
    return ["depth": depth, "button_enabled": true, "landing": landing,
            "level": level, "history_steps": 1, "undo_restores": true,
            "before": before, "after": after]
}

/// new-layer-panel-ui.
func newLayerPanelUI() throws -> [String: Any] {
    let (typed, typedEditor, _) = try openDocument()
    defer { typed.close(); pump() }
    try require(typed.doc?.isHighDepth == true, "Owned fixture is not a 16-bit document")
    let typedFindings = try clickNewLayer(typed, typedEditor, depth: "16-bit")

    let (byte, byteEditor, _) = try openByteDocument()
    defer { byte.close(); pump() }
    try require(byte.doc?.isHighDepth == false, "Owned byte fixture is high depth")
    let byteFindings = try clickNewLayer(byte, byteEditor, depth: "8-bit")

    return ["case": "new-layer-panel-ui",
            "typed": typedFindings, "byte": byteFindings,
            "scope": "the layers panel's own footer button, clicked through its nil-target "
                + "action; the slot rule itself is the core's and is measured in "
                + "core/tests/layer_insert.rs and scripts/verify_phase12e2b_placement.py"]
}
