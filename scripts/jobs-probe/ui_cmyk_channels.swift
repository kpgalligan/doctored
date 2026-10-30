import AppKit

// The CMYK document's channels-batch in-process UI cases (#547): the Select, Channels, Paths and
// Vector Mask menu items a CMYK document now enables, the Magnetic Lasso and the Pen, the Channels
// panel's mask and alpha rows below the inks, an ink row targeting that plate (the Move tool and
// ⌘T), and Quick Mask, which edits coverage and dirties nothing. The CMYK profile is macOS's
// Generic CMYK Profile.

private let genericCMYK = URL(fileURLWithPath: "/System/Library/ColorSync/Profiles/Generic CMYK Profile.icc")

/// A fresh fixture converted to CMYK through Image ▸ Mode's commit.
private func cmykChannelsDocument(_ tag: String) throws -> (ImageDocument, EditorViewController) {
    let document = try fixture(width: 64, height: 48)
    let controller = try editor(document)
    controller.commitColorMode(.cmyk, profile: try Data(contentsOf: genericCMYK), destination: [:],
                               options: CmykConversionOptions(), document: document)
    guard document.doc?.isCmyk == true else {
        document.close()
        throw ProbeFailure(description: "\(tag) not CMYK")
    }
    return (document, controller)
}

/// "cmyk channels menus": with a selection, a layer mask, an alpha channel and a work path, every
/// selector the channels batch admits validates on a CMYK document (the vector-mask items in the
/// two states that enable them); the Magnetic Lasso and the Pen are selectable; the Channels panel
/// lists the inks, the layer mask and the channel; an ink row targets its plate, and ⌘T validates
/// with that target and a selection.
func verifyCmykChannelsMenus() throws {
    let tag = "[cmyk channels menus]"
    let (document, controller) = try cmykChannelsDocument(tag)
    defer { document.close() }
    _ = try agentOK("select_rect", document, ["x": 8, "y": 8, "width": 30, "height": 20])
    _ = try agentOK("add_layer_mask", document, ["layer": 0, "kind": "reveal_all"])
    _ = try agentOK("add_channel", document, ["from": "selection", "name": "Alpha 1"])
    _ = try agentOK("selection_to_path", document)
    guard let doc = document.doc, let work = doc.paths.entries.first else {
        throw ProbeFailure(description: "\(tag) no work path")
    }
    document.currentPath = .path(work.id)
    controller.setPaintTarget(.channel(0))
    let selectors = [
        "invertSelection:", "featherSelection:", "growSelection:", "shrinkSelection:",
        "borderSelection:", "smoothSelection:", "selectGrow:", "selectSimilar:",
        "transformSelection:", "toggleQuickMask:", "saveSelectionSheet:", "loadSelectionSheet:",
        "loadChannelAsSelection:", "newChannel:", "duplicateChannel:", "renameChannel:",
        "channelOptions:", "deleteChannel:", "invertChannel:", "addLuminosityMasks:",
        "makeWorkPath:", "makeSelectionFromPath:", "duplicateCurrentPath:", "deleteCurrentPath:",
        "addVectorMaskRevealAll:",
    ]
    for selector in selectors {
        try require(uiValidates(controller, selector), "\(tag) \(selector) validates on CMYK")
    }
    _ = try agentOK("set_vector_mask", document, ["layer": 0, "reveal_all": true])
    for selector in ["addVectorMaskFromCurrentPath:", "deleteVectorMask:", "toggleVectorMaskEnabled:",
                     "toggleVectorMaskLinked:"] {
        try require(uiValidates(controller, selector), "\(tag) \(selector) validates on CMYK")
    }
    for tool in [EditorTool.magneticLasso, .pen] {
        try require(DocumentPrecision.allows(tool, document: document),
                    "\(tag) \(tool.displayName) is selectable on CMYK")
    }
    guard let current = document.doc else { throw ProbeFailure(description: "\(tag) no document") }
    let rows = current.channelRows(activeLayer: 0)
    try require(rows == [.composite] + CmykPlane.allCases.map { ChannelRow.ink($0) }
                    + [.layerMask, .alpha(0)],
                "\(tag) the rows are the composite, four inks, the mask and the channel: \(rows)")
    try require(ChannelRow.ink(.magenta).paintTarget == .ink(.magenta)
                && ChannelRow.forTarget(.ink(.magenta), channelCount: 1) == .ink(.magenta),
                "\(tag) an ink row stands for its plate")
    controller.setPaintTarget(.ink(.cyan))
    try require(controller.paintTarget == .ink(.cyan), "\(tag) the cyan plate is the target")
    try require(uiValidates(controller, "freeTransform:"), "\(tag) ⌘T validates on the cyan plate")
    try require(ChannelRow.ink(.cyan).selectionSource(activeLayer: 0, isCmyk: true) == .inkPlate(.cyan)
                && ChannelRow.composite.selectionSource(activeLayer: 0, isCmyk: true)
                    == .compositePlane(.luma),
                "\(tag) ⌘-click on an ink row loads its plate, on the composite its luminosity")
    print("PASS: \(tag) the channels batch's items and tools validate on CMYK; an ink row targets its plate")
}

/// "cmyk quick mask": on a clean CMYK document Quick Mask turns on, one brush stroke of coverage
/// lands, and turning it off leaves a selection and the document clean.
func verifyCmykQuickMask() throws {
    let tag = "[cmyk quick mask]"
    let (document, controller) = try cmykChannelsDocument(tag)
    defer { document.close() }
    // As if just saved: the Mode conversion is an edit.
    document.updateChangeCount(.changeCleared)
    try require(!document.isDocumentEdited, "\(tag) the document starts clean")
    try require(document.canvasSelection == nil, "\(tag) nothing is selected")
    try require(uiValidates(controller, "toggleQuickMask:"), "\(tag) Quick Mask validates")
    controller.toggleQuickMask(nil)
    try require(document.quickMaskActive, "\(tag) Quick Mask is on")
    guard let doc = document.doc else { throw ProbeFailure(description: "\(tag) no document") }
    _ = try agentOK("quick_mask_stroke", document, [
        "width": doc.width, "height": doc.height, "stroke_version": 1, "size": 8,
        "points": [[10, 10], [40, 30]],
    ])
    try require(!document.isDocumentEdited, "\(tag) the stroke leaves the document clean")
    controller.toggleQuickMask(nil)
    try require(!document.quickMaskActive, "\(tag) Quick Mask is off")
    guard let selection = document.canvasSelection else {
        throw ProbeFailure(description: "\(tag) the stroke made no selection")
    }
    try require(!selection.bounds.isEmpty && selection.bounds.width < CGFloat(doc.width),
                "\(tag) the selection is the stroke: \(selection.bounds)")
    try require(!document.isDocumentEdited, "\(tag) the document stays clean")
    try require(document.doc?.isCmyk == true, "\(tag) the document stays CMYK")
    print("PASS: \(tag) Quick Mask strokes coverage into a selection and dirties nothing")
}
