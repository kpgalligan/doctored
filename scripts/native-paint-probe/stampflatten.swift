import AppKit

// Phase 12E-2c: the UI-SOURCE half of Stamp Visible, Flatten Image and the
// eight 12E-1b menu selectors this run whitelisted at depth.
//
// An MCP drive can call `stamp_visible` and `flatten_image`; what only the
// actual menu can prove lives here — that `validateUserInterfaceItem` ENABLES
// all ten items on a 16-bit document (they were hard-disabled by the typed
// selector whitelist until this phase), that Merge Visible and Stamp Visible
// go DARK exactly where the typed core's narrower `Content::Raster` floor
// refuses, that the Merge Down item retitles itself at depth, that a lock
// checkmark round-trips, and that each command is ONE history step one Undo
// restores. The 8-bit document is measured beside them so "byte is unchanged"
// is an assertion rather than a claim.
//
// The FLOAT half RUNS since Phase 12E-3b's carried item 1: `openFloatDocument`
// (main.swift) opens the committed 11F fixture through the app's own
// preserving path, so tables (a) and (b) are asserted at 32-bit float as well
// — the whitelisted selectors enabled, and the raster floor measured with an
// EXPOSURE adjustment (the one adjustment a float document admits).

/// The ten selectors the whitelist gained, in the order the spec names them:
/// 12E-1b's eight first, then this run's two.
private let whitelisted: [(String, Selector)] = [
    ("mergeDown:", #selector(EditorViewController.mergeDown(_:))),
    ("mergeVisible:", #selector(EditorViewController.mergeVisible(_:))),
    ("linkLayers:", #selector(EditorViewController.linkLayers(_:))),
    ("unlinkLayers:", #selector(EditorViewController.unlinkLayers(_:))),
    ("lockTransparency:", #selector(EditorViewController.lockTransparency(_:))),
    ("lockPixels:", #selector(EditorViewController.lockPixels(_:))),
    ("lockPosition:", #selector(EditorViewController.lockPosition(_:))),
    ("lockAll:", #selector(EditorViewController.lockAll(_:))),
    ("stampVisible:", #selector(EditorViewController.stampVisible(_:))),
    ("flattenImage:", #selector(EditorViewController.flattenImage(_:))),
]

/// A probe-built menu item for `selector`, the shape AppKit hands
/// `validateUserInterfaceItem` for a real Layer-menu row.
private func item(_ selector: Selector, title: String = "Item") -> NSMenuItem {
    let menuItem = NSMenuItem(title: title, action: selector, keyEquivalent: "")
    menuItem.target = nil
    return menuItem
}

private func validates(_ editor: EditorViewController, _ selector: Selector) -> Bool {
    editor.validateUserInterfaceItem(item(selector))
}

/// Every record's name and level, walked off the document itself.
private func stack(_ document: ImageDocument) throws -> [String] {
    guard let doc = document.doc else {
        throw DoctoredCoreError(message: "Owned document handle missing")
    }
    return try (0..<doc.layerCount).map { index in
        guard let info = doc.layerInfo(index) else {
            throw DoctoredCoreError(message: "Owned layer row missing")
        }
        return "\(info.name)@\(info.depth)"
    }
}

/// Runs `action` through the editor, asserting it published EXACTLY one
/// history step and that one Undo puts the stack back.
private func oneStep(_ document: ImageDocument, _ label: String,
                     _ action: () -> Void) throws -> [String] {
    let before = try stack(document)
    let steps = document.history.entries.count
    document.undoManager?.removeAllActions()
    action()
    pump()
    let after = try stack(document)
    try require(document.history.entries.count == steps + 1,
        "\(label) published \(document.history.entries.count - steps) history steps")
    document.undoManager?.undo()
    pump()
    try require(try stack(document) == before,
        "One Undo did not restore the stack after \(label)")
    document.undoManager?.removeAllActions()
    return after
}

/// (a) A 16-bit document with TWO contributing raster leaves: all ten items
/// are enabled, the Merge Down item retitles itself, a lock checkmark
/// round-trips, and Stamp / Flatten / Merge Visible are each one step.
private func typedMenuFindings(float: Bool = false) throws -> [String: Any] {
    let (document, editor, _) = try (float ? openFloatDocument() : openDocument())
    defer { document.close(); pump() }
    try require(document.doc?.isHighDepth == true, "Owned fixture is not a typed document")
    try require(document.doc?.isFloat == float,
        "Owned fixture is \(document.doc?.isFloat == true ? "float" : "u16"), expected "
            + (float ? "float" : "u16"))
    // A second contributing RASTER leaf, through the very tool the Layers
    // panel's duplicate row uses.
    let duplicated = try call("duplicate_layer", ["layer": 0], document: document)
    try require(!(try responseIsError(duplicated)), "duplicate_layer refused: \(duplicated)")
    guard let doc = document.doc else {
        throw DoctoredCoreError(message: "Owned document handle missing")
    }
    try require(doc.contributingRasterLeaves().count == 2,
        "Owned fixture does not hold two contributing raster leaves")

    // linkLayers:/Merge Layers need a multi-selection; unlinkLayers: needs a
    // link to exist, so it is asked AFTER linkLayers: has been invoked.
    document.setLayerSelection(LayerSelection(primary: 1, others: [0]))
    pump()
    var enabled: [String: Bool] = [:]
    for (name, selector) in whitelisted where name != "unlinkLayers:" {
        let answer = validates(editor, selector)
        enabled[name] = answer
        try require(answer, "\(name) is DISABLED on a 16-bit document")
    }
    // The Merge Down item's TITLE, multi- then single-selected.
    let multiple = item(#selector(EditorViewController.mergeDown(_:)))
    _ = editor.validateUserInterfaceItem(multiple)
    try require(multiple.title == "Merge Layers",
        "The multi-selected Merge item reads \"\(multiple.title)\" at depth")
    editor.linkLayers(nil)
    pump()
    let unlink = validates(editor, #selector(EditorViewController.unlinkLayers(_:)))
    enabled["unlinkLayers:"] = unlink
    try require(unlink, "unlinkLayers: is DISABLED after a link at depth")
    try require((document.doc?.layerLink(0) ?? 0) != 0, "linkLayers: linked nothing at depth")
    editor.unlinkLayers(nil)
    pump()
    document.undoManager?.removeAllActions()

    document.setLayerSelection(.single(1))
    pump()
    let single = item(#selector(EditorViewController.mergeDown(_:)))
    _ = editor.validateUserInterfaceItem(single)
    try require(single.title == "Merge Down",
        "The single-selected Merge item reads \"\(single.title)\" at depth")

    // The lock CHECKMARK: off, on after one invocation, off again after one
    // Undo — and the invocation is one history step.
    let lockItem = item(#selector(EditorViewController.lockPixels(_:)))
    _ = editor.validateUserInterfaceItem(lockItem)
    try require(lockItem.state == .off, "Lock Pixels starts checked at depth")
    let lockSteps = document.history.entries.count
    document.undoManager?.removeAllActions()
    editor.lockPixels(nil)
    pump()
    let lockedItem = item(#selector(EditorViewController.lockPixels(_:)))
    _ = editor.validateUserInterfaceItem(lockedItem)
    try require(lockedItem.state == .on, "Lock Pixels did not check at depth")
    try require(document.history.entries.count == lockSteps + 1,
        "Lock Pixels published \(document.history.entries.count - lockSteps) history steps")
    document.undoManager?.undo()
    pump()
    let releasedItem = item(#selector(EditorViewController.lockPixels(_:)))
    _ = editor.validateUserInterfaceItem(releasedItem)
    try require(releasedItem.state == .off, "One Undo did not release the lock checkmark")
    document.undoManager?.removeAllActions()

    // Stamp: one step, the landing index SELECTED, one Undo restores.
    let anchor = document.activeLayerIndex
    let stamped = try oneStepStamp(document, editor, anchor: anchor)
    let merged = try oneStep(document, "Merge Visible") { editor.mergeVisible(nil) }
    try require(merged.count == 1, "Merge Visible left \(merged.count) records at depth")
    let flattened = try oneStep(document, "Flatten Image") { editor.flattenImage(nil) }
    try require(flattened == ["Background@0"],
        "Flatten Image left \(flattened) at depth")

    return ["depth": float ? "32-bit float" : "16-bit", "enabled": enabled,
            "merge_item_titles": ["multiple": "Merge Layers", "single": "Merge Down"],
            "lock_checkmark": ["off", "on", "off"],
            "stamp": stamped, "merge_visible": merged, "flatten": flattened]
}

/// Stamp Visible on its own, because it also asserts WHICH record ends up
/// selected — the slot rule's landing index, not `layerCount - 1`.
private func oneStepStamp(_ document: ImageDocument, _ editor: EditorViewController,
                          anchor: Int) throws -> [String: Any] {
    let before = try stack(document)
    let steps = document.history.entries.count
    document.undoManager?.removeAllActions()
    editor.stampVisible(nil)
    pump()
    let after = try stack(document)
    try require(after.count == before.count + 1,
        "Stamp Visible made \(after.count - before.count) records at depth")
    try require(document.history.entries.count == steps + 1,
        "Stamp Visible published \(document.history.entries.count - steps) history steps")
    let landing = anchor + 1
    try require(document.activeLayerIndex == landing,
        "Stamp Visible selected \(document.activeLayerIndex) rather than the landing index")
    try require(after[landing].hasPrefix("Stamp@"),
        "The stamped record is \(after[landing]) rather than Stamp at the anchor's level")
    document.undoManager?.undo()
    pump()
    try require(try stack(document) == before, "One Undo did not restore the stack after Stamp")
    document.undoManager?.removeAllActions()
    return ["landing": landing, "records": after]
}

/// (b) and (c): the depth-aware floor. A 16-bit raster plus an ADJUSTMENT has
/// ONE contributing raster leaf, so Merge Visible is disabled and Stamp is
/// not; hiding the raster disables Stamp too. The same 8-bit stack answers
/// TRUE to both, because the byte predicate counts an adjustment layer.
private func adjustmentFloorFindings(byteDepth: Bool, float: Bool = false) throws -> [String: Any] {
    let (document, editor, _) = try (byteDepth
        ? openByteDocument()
        : (float ? openFloatDocument() : openDocument()))
    defer { document.close(); pump() }
    try require(document.doc?.isHighDepth == !byteDepth, "Owned fixture is the wrong depth")
    try require(document.doc?.isFloat == float, "Owned fixture has the wrong storage")
    // EXPOSURE is the adjustment a FLOAT document admits (`float_document`'s
    // operator table); every other depth uses the cheapest one there is.
    let arguments: [String: Any] = float
        ? ["op": "exposure", "params": ["exposure": 0.5], "name": "Fx"]
        : ["op": "invert", "name": "Fx"]
    let created = try call("add_adjustment_layer", arguments, document: document)
    try require(!(try responseIsError(created)), "add_adjustment_layer refused: \(created)")
    pump()
    guard let doc = document.doc else {
        throw DoctoredCoreError(message: "Owned document handle missing")
    }
    try require(doc.layerCount == 2 && doc.layerIsAdjustment(1),
        "Owned fixture is not one raster plus one adjustment")
    try require(doc.contributingLeaves().count == 2,
        "Both leaves contribute to the projection")
    try require(doc.contributingRasterLeaves().count == 1,
        "Exactly one of them is a raster")

    let mergeVisible = validates(editor, #selector(EditorViewController.mergeVisible(_:)))
    let stamp = validates(editor, #selector(EditorViewController.stampVisible(_:)))
    let flatten = validates(editor, #selector(EditorViewController.flattenImage(_:)))
    if byteDepth {
        try require(mergeVisible, "8-bit Merge Visible went dark: the byte floor changed")
        try require(stamp, "8-bit Stamp Visible went dark: the byte floor changed")
    } else {
        try require(!mergeVisible,
            "16-bit Merge Visible is ENABLED over one raster and one adjustment, which the "
                + "typed core refuses")
        try require(stamp, "16-bit Stamp Visible went dark over one contributing raster")
    }
    try require(flatten, "Flatten Image went dark over two records")

    var hiddenStamp: Bool?
    if !byteDepth {
        let hidden = try call("set_layer_properties", ["layer": 0, "visible": false],
                              document: document)
        try require(!(try responseIsError(hidden)), "set_layer_properties refused: \(hidden)")
        pump()
        try require(document.doc?.contributingRasterLeaves().isEmpty == true,
            "Hiding the raster left a contributing raster leaf")
        let answer = validates(editor, #selector(EditorViewController.stampVisible(_:)))
        hiddenStamp = answer
        try require(!answer,
            "16-bit Stamp Visible is ENABLED with no contributing raster, which the typed "
                + "core refuses")
    }
    var findings: [String: Any] = [
        "depth": byteDepth ? "8-bit" : (float ? "32-bit float" : "16-bit"),
        "merge_visible": mergeVisible, "stamp_visible": stamp, "flatten_image": flatten,
    ]
    if let hiddenStamp { findings["stamp_visible_with_raster_hidden"] = hiddenStamp }
    return findings
}

/// stamp-flatten-menu-ui.
func stampFlattenMenuUI() throws -> [String: Any] {
    let typed = try typedMenuFindings()
    let typedFloor = try adjustmentFloorFindings(byteDepth: false)
    let byteFloor = try adjustmentFloorFindings(byteDepth: true)
    // The FLOAT tables, through the 12E-3b float opener: (a) the ten
    // whitelisted selectors on a float document, (b) the same raster floor
    // with an Exposure adjustment.
    let floatMenu = try typedMenuFindings(float: true)
    let floatFloor = try adjustmentFloorFindings(byteDepth: false, float: true)
    return ["case": "stamp-flatten-menu-ui",
            "typed_menu": typed, "typed_floor": typedFloor, "byte_floor": byteFloor,
            "float_menu": floatMenu, "float_floor": floatFloor,
            "scope": "validateUserInterfaceItem over probe-built NSMenuItems plus the "
                + "editor's own nil-target actions; the ops themselves are measured in "
                + "core/tests/precision_stamp_flatten.rs and the drive"]
}
