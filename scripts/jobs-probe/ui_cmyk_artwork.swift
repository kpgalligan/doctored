import AppKit

// The artwork batch's in-process UI cases (#547): the Type tool on a CMYK document — a new text
// layer committed from the canvas session in the foreground's picked ink, kept exactly through an
// unchanged reopen and an edit — and a gradient fill layer's sheet reopened from the Layers panel,
// whose angle-only Apply keeps every stop's ink. The CMYK profile is macOS's Generic CMYK Profile.

private let artworkGenericCMYK = URL(fileURLWithPath: "/System/Library/ColorSync/Profiles/Generic CMYK Profile.icc")

/// A fresh fixture converted to CMYK through Image ▸ Mode's commit.
private func artworkDocument(_ tag: String) throws -> (ImageDocument, EditorViewController) {
    let document = try fixture(width: 160, height: 100)
    let controller = try editor(document)
    controller.commitColorMode(.cmyk, profile: try Data(contentsOf: artworkGenericCMYK), destination: [:],
                               options: CmykConversionOptions(), document: document)
    guard document.doc?.isCmyk == true else {
        document.close()
        throw ProbeFailure(description: "\(tag) not CMYK")
    }
    return (document, controller)
}

/// "cmyk type tool": the Type tool and the fill-layer items are admitted on CMYK; a session typed
/// in the foreground's picked ink (K 100) commits a text layer whose stored ink is exactly that
/// ink; reopening it and committing unchanged adds no step; an edit of the string keeps the ink.
func verifyCmykTypeTool() throws {
    let tag = "[cmyk type tool]"
    let (document, controller) = try artworkDocument(tag)
    defer { document.close() }
    try require(DocumentPrecision.allows(.text, document: document), "\(tag) the Type tool is admitted")
    for tool in [EditorTool.shapeRect, .shapeEllipse, .shapeLine] {
        try require(DocumentPrecision.allows(tool, document: document), "\(tag) \(tool.displayName) is admitted")
    }
    for selector in ["newSolidColorFillLayer:", "newGradientFillLayer:"] {
        try require(uiValidates(controller, selector), "\(tag) \(selector) validates")
    }
    let black = CmykInk(c: 0, m: 0, y: 0, k: 100)
    let rgb = try RasterPrecisionDocument.srgb(fromInk: black, document: document.doc?.precision)
    controller.setPaintInk(black, color: NSColor(srgbRed: rgb[0], green: rgb[1], blue: rgb[2], alpha: 1))
    controller.selectTool(.text)
    let canvas: ImageCanvasView = controller.canvas
    let before = document.doc?.layerCount ?? 0
    canvas.beginTextSession(at: CGPoint(x: 12, y: 12))
    guard let view = canvas.activeTextView else { throw ProbeFailure(description: "\(tag) no session") }
    view.insertText("Ink", replacementRange: NSRange(location: NSNotFound, length: 0))
    try require(canvas.commitTextSession(), "\(tag) the session committed")
    try pumpUntil({ !BackgroundWork.occupied }, seconds: 30)
    guard let doc = document.doc, doc.layerCount == before + 1 else {
        throw ProbeFailure(description: "\(tag) no new layer")
    }
    let index = document.activeLayerIndex
    guard let stored = doc.textPayload(index), let style = stored.artwork else {
        throw ProbeFailure(description: "\(tag) the new layer is not text")
    }
    try require(style.ink == .cmyk && style.rgba == [0, 0, 0, 1, 1],
                "\(tag) the stored ink is K 100 exactly: \(style.rgba) \(String(describing: style.ink))")
    let steps = document.history.entries.count
    controller.editTextLayer(index)
    try require(canvas.hasActiveTextSession, "\(tag) the layer reopened")
    try require(canvas.commitTextSession(), "\(tag) the unchanged session closed")
    try require(document.history.entries.count == steps, "\(tag) an unchanged reopen adds no step")
    controller.editTextLayer(index)
    guard let reopened = canvas.activeTextView else { throw ProbeFailure(description: "\(tag) no reopen") }
    reopened.setSelectedRange(NSRange(location: (reopened.string as NSString).length, length: 0))
    reopened.insertText("s", replacementRange: NSRange(location: NSNotFound, length: 0))
    try require(canvas.commitTextSession(), "\(tag) the edit committed")
    try pumpUntil({ !BackgroundWork.occupied }, seconds: 30)
    guard let edited = document.doc?.textPayload(index) else {
        throw ProbeFailure(description: "\(tag) the edited layer is not text")
    }
    try require(edited.string == "Inks" && edited.artwork?.rgba == [0, 0, 0, 1, 1],
                "\(tag) the edit keeps the ink: \(edited.string) \(String(describing: edited.artwork?.rgba))")
    try require(document.undoManager?.undoActionName == "Edit Text Layer", "\(tag) one Edit Text Layer step")
    print("PASS: \(tag) the Type tool commits and edits a CMYK text layer in the picked ink exactly")
}

/// "cmyk fill layer sheet": a gradient fill layer with a K-only stop, reopened from the Layers
/// panel; Apply after changing only the angle records one "Edit Fill Layer" step and keeps every
/// stop's ink exactly.
func verifyCmykFillLayerSheet() throws {
    let tag = "[cmyk fill layer sheet]"
    let (document, controller) = try artworkDocument(tag)
    defer { document.close() }
    let added = try agentOK("add_fill_layer", document, [
        "kind": "gradient",
        "gradient": ["stops": [["position": 0, "ink": [0, 0, 0, 100]], ["position": 1, "color": "#ffffff"]],
                     "angle": 90],
    ])
    guard let index = added["layer"] as? Int, let before = document.doc?.fillPayload(index)?.stopInks else {
        throw ProbeFailure(description: "\(tag) no CMYK gradient fill: \(added)")
    }
    try require(before.first == [0, 0, 0, 1], "\(tag) the K-only stop is stored as ink: \(before)")
    controller.editFillLayer(index)
    try pumpUntil({ controller.presentedViewControllers?.contains { $0 is FillLayerSheetController } == true },
                  seconds: 5)
    guard let sheet = controller.presentedViewControllers?.first(where: { $0 is FillLayerSheetController })
            as? FillLayerSheetController,
          let angle = Mirror(reflecting: sheet).children.first(where: { $0.label == "angleField" })?.value
            as? NSTextField else {
        throw ProbeFailure(description: "\(tag) the fill sheet did not open")
    }
    angle.doubleValue = 45
    func apply(in view: NSView) -> NSButton? {
        if let button = view as? NSButton, button.identifier?.rawValue == "fill-layer-apply" { return button }
        return view.subviews.lazy.compactMap(apply(in:)).first
    }
    guard let button = apply(in: sheet.view) else { throw ProbeFailure(description: "\(tag) no Apply") }
    button.performClick(nil)
    // A dismissed sheet stays in `presentedViewControllers` here: wait for the edit itself.
    try pumpUntil({ !BackgroundWork.occupied && document.doc?.fillPayload(index)?.gradient?.angle == 45 },
                  seconds: 30)
    guard let after = document.doc?.fillPayload(index) else {
        throw ProbeFailure(description: "\(tag) the layer is no longer a fill layer")
    }
    try require(after.gradient?.angle == 45, "\(tag) the angle changed: \(String(describing: after.gradient?.angle))")
    try require(after.stopInks == before, "\(tag) stop_inks are unchanged: \(String(describing: after.stopInks)) vs \(before)")
    try require(document.undoManager?.undoActionName == "Edit Fill Layer", "\(tag) one Edit Fill Layer step")
    print("PASS: \(tag) an angle-only Apply keeps every stop's ink exactly")
}
