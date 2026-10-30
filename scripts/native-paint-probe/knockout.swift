import AppKit

// Phase 12E-3b: the UI-SOURCE half of KNOCKOUT — the Blending Options sheet's
// two new controls, and the three MERGE menu items.
//
// An MCP drive can call `set_layer_style`, `merge_down` and `merge_visible`;
// it proves the MODEL and the COMPOSITOR. What only the real UI can prove
// lives here: that the Knockout popup and the "Blend Interior Effects as
// Group" checkbox actually reach the style the Apply button publishes, that
// the publication is ONE history step one Undo restores, and that the three
// merge MENU items — which carry no message surface at all — merge the five
// exact scenes without changing the picture. The float half asserts the one
// thing the sheet must NOT do: open at all.
//
// Every scene is built through the REAL tools (`new_layer`, `fill`,
// `group_layers`, `set_layer_clipped`, `set_layer_style`), and every
// projection is the document's OWN — `flattened()` at 8 bits, the typed
// render at 16 — compared at EVERY canvas pixel.

/// The probe's canvas. Small on purpose: every scene is canvas-sized opaque
/// fills, so nothing hides in a quantization step and the whole projection is
/// cheap to compare pixel by pixel.
private let sceneWidth = 48
private let sceneHeight = 32

/// T1 / T3: ONE code in the document's OWN depth, alpha included — the same
/// tolerances `core/tests/knockout_merge.rs` and both drives fix.
private let toleranceCodes = 1

private let bgHex = "#cc2222"
private let midHex = "#22cc22"
private let highHex = "#2222cc"
private let extraHex = "#cccc22"

// ------------------------------------------------------------ plumbing --

/// One MCP call through the REAL server, refusing to continue on an error.
@discardableResult
private func mcp(_ tool: String, _ arguments: [String: Any],
                 _ document: ImageDocument) throws -> [String: Any] {
    let response = try call(tool, arguments, document: document)
    try require(!(try responseIsError(response)), "\(tool) refused: \(response)")
    guard let envelope = try JSONSerialization.jsonObject(with: Data(response.utf8))
            as? [String: Any],
          let content = envelope["content"] as? [[String: Any]],
          let text = content.first?["text"] as? String else {
        throw DoctoredCoreError(message: "Invalid MCP response: \(response)")
    }
    return (try? JSONSerialization.jsonObject(with: Data(text.utf8))) as? [String: Any] ?? [:]
}

/// The document's own projection, in its own depth: straight RGBA8 for a byte
/// document, u16 samples for a typed one.
private func projection(_ document: ImageDocument) throws -> (depth: Int, values: [Int]) {
    guard let doc = document.doc else {
        throw DoctoredCoreError(message: "Owned document handle missing")
    }
    if let precision = doc.precision {
        let context = try PrecisionWorkContext(source: precision)
        let image = try precision.render(workingLimit: context.limit,
                                         otherRetained: context.otherRetained)
        return (16, try image.withRGBA16 { buffer in buffer.map { Int($0) } })
    }
    return (8, try flatBytes(doc).map { Int($0) })
}

/// The worst per-channel difference between two projections of the SAME
/// picture, which must be inside one code.
private func worstDifference(_ before: (depth: Int, values: [Int]),
                             _ after: (depth: Int, values: [Int]),
                             _ label: String) throws -> Int {
    try require(before.depth == after.depth && before.values.count == after.values.count,
        "\(label): the projection changed shape")
    var worst = 0
    for index in before.values.indices {
        worst = max(worst, abs(before.values[index] - after.values[index]))
    }
    try require(worst <= toleranceCodes,
        "\(label): before != after, worst \(worst) codes of the document's own depth")
    return worst
}

private func changed(_ before: (depth: Int, values: [Int]),
                     _ after: (depth: Int, values: [Int])) -> Int {
    guard before.values.count == after.values.count else { return Int.max }
    var worst = 0
    for index in before.values.indices {
        worst = max(worst, abs(before.values[index] - after.values[index]))
    }
    return worst
}

/// Fills layer `index` edge to edge through the real fill tool.
private func paint(_ document: ImageDocument, _ index: Int, _ hex: String) throws {
    try mcp("set_active_layer", ["index": index], document)
    try mcp("select_rect", ["x": 0, "y": 0, "width": sceneWidth, "height": sceneHeight], document)
    try mcp("fill", ["x": 0, "y": 0, "color": hex, "tolerance": 255, "contiguous": false,
                     "layer": index], document)
    try mcp("deselect", [:], document)
}

/// A bottom-first stack of canvas-sized opaque layers. Layer 0 is the
/// fixture's own raster REPAINTED, so it is the plain, visible, unmasked,
/// unstyled, Normal, opacity-1, canvas-sized raster at (0, 0) D5 asks for.
private func buildStack(_ document: ImageDocument, _ colours: [String]) throws {
    for (index, hex) in colours.enumerated() {
        if index > 0 {
            try mcp("set_active_layer", ["index": index - 1], document)
            try mcp("new_layer", [:], document)
        }
        try paint(document, index, hex)
    }
    guard document.doc?.layerCount == colours.count else {
        throw DoctoredCoreError(message: "Owned scene has \(document.doc?.layerCount ?? -1) "
            + "records, expected \(colours.count)")
    }
}

private func knockoutStyle(_ mode: String, _ fill: Double) -> [String: Any] {
    ["knockout": mode, "fill_opacity": fill]
}

private func openScene(_ depth: Int) throws -> (ImageDocument, EditorViewController) {
    if depth == 8 {
        let (document, editor, _) = try openByteDocument(width: sceneWidth, height: sceneHeight)
        return (document, editor)
    }
    let (document, editor, _) = try openDocument(width: sceneWidth, height: sceneHeight)
    try require(document.doc?.isHighDepth == true, "Owned fixture is not a 16-bit document")
    return (document, editor)
}

/// A probe-built menu item, the shape AppKit hands `validateUserInterfaceItem`.
private func menuItem(_ selector: Selector) -> NSMenuItem {
    let item = NSMenuItem(title: "Item", action: selector, keyEquivalent: "")
    item.target = nil
    return item
}

// --------------------------------------------- knockout-blending-ui --

/// The presented Layer Style sheet, found the way AppKit files it.
private func presentedStyleSheet(_ editor: EditorViewController) throws
    -> LayerStyleSheetController {
    try waitUntil({
        (editor.presentedViewControllers ?? []).contains { $0 is LayerStyleSheetController }
    }, "The Layer Style sheet never came up")
    guard let sheet = (editor.presentedViewControllers ?? [])
        .compactMap({ $0 as? LayerStyleSheetController }).first else {
        throw DoctoredCoreError(message: "The Layer Style sheet never came up")
    }
    _ = sheet.view
    pump()
    return sheet
}

/// The sheet's control carrying `identifier` — the probe's whole contract
/// with `LayerStyleKnockoutRows`.
private func control<T: NSView>(_ sheet: LayerStyleSheetController,
                                _ identifier: NSUserInterfaceItemIdentifier,
                                _ what: String) throws -> T {
    guard let found = descendants(sheet.view).compactMap({ $0 as? T })
        .first(where: { $0.identifier == identifier }) else {
        throw DoctoredCoreError(message: "The Layer Style sheet has no \(what) "
            + "(\(identifier.rawValue))")
    }
    return found
}

/// Drives the two controls and Apply at one depth, on
/// `[BG, Mid, Top(fill 0)]` with Top active: setting Knockout to Deep punches
/// the hole through Mid, so the picture MUST move.
private func blendingFindings(_ depth: Int) throws -> [String: Any] {
    let (document, editor) = try openScene(depth)
    defer { document.close(); pump() }
    try buildStack(document, [bgHex, midHex, highHex])
    // Fill opacity alone, set through MCP: the sheet then adds ONLY the two
    // keys this case is about, so a changed picture can be nothing else.
    try mcp("set_layer_style", ["index": 2, "style": ["fill_opacity": 0.0]], document)
    document.setLayerSelection(.single(2))
    pump()
    document.undoManager?.removeAllActions()

    let before = try projection(document)
    let steps = document.history.entries.count
    editor.layerStyle(nil)
    let sheet = try presentedStyleSheet(editor)
    let popup: NSPopUpButton = try control(sheet, LayerStyleKnockoutRows.knockoutIdentifier,
                                           "Knockout popup")
    let checkbox: NSButton = try control(sheet, LayerStyleKnockoutRows.blendInteriorIdentifier,
                                         "Blend Interior Effects as Group checkbox")
    let apply: NSButton = try control(sheet, LayerStyleKnockoutRows.applyIdentifier,
                                      "Apply button")
    try require(popup.itemTitles == LayerStyleKnockoutRows.titles,
        "The Knockout popup lists \(popup.itemTitles)")
    guard let deep = popup.itemArray.first(where: {
        ($0.representedObject as? String) == Knockout.deep.rawValue
    }) else {
        throw DoctoredCoreError(message: "The Knockout popup has no Deep item")
    }
    // PICK, exactly as AppKit does for a user: select, then fire the
    // control's own action.
    popup.select(deep)
    guard let popupAction = popup.action else {
        throw DoctoredCoreError(message: "The Knockout popup has no action")
    }
    _ = popup.sendAction(popupAction, to: popup.target)
    pump()
    checkbox.state = .on
    guard let checkAction = checkbox.action else {
        throw DoctoredCoreError(message: "The Blend Interior checkbox has no action")
    }
    _ = checkbox.sendAction(checkAction, to: checkbox.target)
    pump()
    apply.performClick(nil)
    pump()
    try waitUntil({ document.history.entries.count == steps + 1 },
        "Apply published \(document.history.entries.count - steps) history steps")
    // The receipt reports what was MEASURED here, not a literal: a probe that
    // hard-codes the number it claims to have measured proves nothing
    // (12E-3c, carried item 1).
    let publishedSteps = document.history.entries.count - steps

    // The ECHO: what the two controls set is what the layer now carries.
    guard let style = document.doc?.layerStylePayload(2) else {
        throw DoctoredCoreError(message: "The layer carries no style after Apply")
    }
    try require(style.knockout == .deep,
        "The layer carries knockout \(style.knockout.rawValue) after picking Deep")
    try require(style.blendInteriorAsGroup,
        "The layer did not carry Blend Interior Effects as Group after checking it")
    try require(style.fillOpacity == 0,
        "Apply moved fill opacity to \(style.fillOpacity)")

    let after = try projection(document)
    let moved = changed(before, after)
    try require(moved > toleranceCodes,
        "The knockout changed the projection by \(moved) codes; this scene must move")

    // ONE Undo, and both the style and the picture are back.
    document.undoManager?.undo()
    pump()
    let restored = try projection(document)
    try require(changed(before, restored) <= toleranceCodes,
        "One Undo left the projection \(changed(before, restored)) codes from where it began")
    // `Knockout.none` SPELLED OUT: bare `.none` beside an optional resolves to
    // `Optional.none`, which compares an unwrapped enum against nil and fails
    // for every value including this one.
    let restoredStyle = document.doc?.layerStylePayload(2)
    try require((restoredStyle?.knockout ?? Knockout.none) == Knockout.none,
        "\(depth)-bit: one Undo left the knockout key behind — the layer reads "
            + "\(document.doc?.layerStyle(2) ?? "no style") and the history holds "
            + "\(document.history.entries.count) entries (started at \(steps))")
    document.undoManager?.removeAllActions()

    return ["depth": depth == 8 ? "8-bit" : "16-bit",
            "popup_titles": popup.itemTitles,
            "history_steps": publishedSteps,
            "echoed": ["knockout": style.knockout.rawValue,
                       "blend_interior_as_group": style.blendInteriorAsGroup,
                       "fill_opacity": style.fillOpacity],
            "projection_moved_codes": moved,
            "projection_after_one_undo_codes": changed(before, restored)]
}

/// The FLOAT half: the sheet must not be reachable at all. A float document
/// refuses a layer style by name in the core, and the editor's selector
/// whitelist keeps the item dark so a user never reaches that refusal.
private func floatStyleFindings() throws -> [String: Any] {
    let (document, editor, _) = try openFloatDocument()
    defer { document.close(); pump() }
    let validated = editor.validateUserInterfaceItem(
        menuItem(#selector(EditorViewController.layerStyle(_:))))
    try require(!validated, "Layer Style is ENABLED on a 32-bit float document")
    return ["depth": "32-bit float", "layer_style_item_enabled": validated]
}

/// knockout-blending-ui.
func knockoutBlendingUI(output: URL) throws -> [String: Any] {
    let byte = try blendingFindings(8)
    let typed = try blendingFindings(16)
    let float = try floatStyleFindings()
    return ["case": "knockout-blending-ui",
            "byte": byte, "typed": typed, "float": float,
            "scope": "The real LayerStyleSheetController's own controls, driven by the "
                + "identifiers LayerStyleKnockoutRows publishes; the compositor itself is "
                + "measured in core/tests and both MCP drives"]
}

// ------------------------------------------ knockout-merge-menu-ui --

/// `[BG, MidA, MidB, Top(Deep, fill 0)]` — the scene D7's old disclosure was
/// about, and the one three of the five merges run on.
private func rootScene(_ depth: Int) throws -> (ImageDocument, EditorViewController) {
    let (document, editor) = try openScene(depth)
    try buildStack(document, [bgHex, midHex, extraHex, highHex])
    try mcp("set_layer_style", ["index": 3, "style": knockoutStyle("deep", 0.0)], document)
    return (document, editor)
}

/// `[BG, Sib, P(Pass Through){Lower, K(Shallow, fill 0)}]` — indices
/// `[BG 0, Sib 1, Lower 2, K 3, P 4]`. A fresh group is already Pass Through
/// at both depths, so nothing is asked for that would change nothing.
private func passThroughScene(_ depth: Int) throws -> (ImageDocument, EditorViewController) {
    let (document, editor) = try openScene(depth)
    try buildStack(document, [bgHex, midHex, extraHex, highHex])
    try mcp("set_layer_style", ["index": 3, "style": knockoutStyle("shallow", 0.0)], document)
    let reply = try mcp("group_layers", ["layers": [2, 3], "name": "P"], document)
    guard let group = reply["group"] as? Int, let info = document.doc?.layerInfo(group) else {
        throw DoctoredCoreError(message: "group_layers answered no group: \(reply)")
    }
    try require(info.blendMode == DR_BLEND_PASS_THROUGH,
        "A fresh group is not Pass Through; this scene needs one")
    return (document, editor)
}

/// `[BG, Base, C0(clipped), Member(clipped, Deep, fill 0)]`, merged WHOLE.
/// `C0` is what makes the knockout OBSERVABLE: the base-only snapshot is
/// Base, where a no-knockout render of the same stack shows C0.
private func clipScene(_ depth: Int, knocked: Bool = true) throws
    -> (ImageDocument, EditorViewController) {
    let (document, editor) = try openScene(depth)
    try buildStack(document, [bgHex, midHex, extraHex, highHex])
    try mcp("set_layer_clipped", ["index": 2, "clipped": true], document)
    try mcp("set_layer_clipped", ["index": 3, "clipped": true], document)
    let style: [String: Any] = knocked ? knockoutStyle("deep", 0.0) : ["fill_opacity": 0.0]
    try mcp("set_layer_style", ["index": 3, "style": style], document)
    return (document, editor)
}

/// One scene through one MENU item: the projection before the merge, the
/// invocation, ONE history step, and the projection after — equal at every
/// canvas pixel within one code.
private func mergeFindings(_ label: String, _ depth: Int, selection: [Int],
                           visible: Bool,
                           build: (Int) throws -> (ImageDocument, EditorViewController))
    throws -> [String: Any] {
    let (document, editor) = try build(depth)
    defer { document.close(); pump() }
    if selection.count > 1 {
        document.setLayerSelection(LayerSelection(primary: selection[selection.count - 1],
                                                  others: Set(selection.dropLast())))
    } else {
        document.setLayerSelection(.single(selection[0]))
    }
    pump()
    document.undoManager?.removeAllActions()
    let before = try projection(document)
    let records = document.doc?.layerCount ?? 0
    let steps = document.history.entries.count
    if visible { editor.mergeVisible(nil) } else { editor.mergeDown(nil) }
    pump()
    try require(document.history.entries.count == steps + 1,
        "\(label) published \(document.history.entries.count - steps) history steps")
    let left = document.doc?.layerCount ?? 0
    try require(left < records, "\(label) merged nothing: \(records) records before, \(left) after")
    let after = try projection(document)
    let worst = try worstDifference(before, after, "\(depth)-bit \(label)")
    document.undoManager?.undo()
    pump()
    try require(document.doc?.layerCount == records,
        "One Undo did not restore the stack after \(label)")
    document.undoManager?.removeAllActions()
    return ["worst_code": worst, "tolerance": toleranceCodes,
            "records_before": records, "records_after": left,
            "menu_item": visible ? "mergeVisible:" : "mergeDown:"]
}

/// knockout-merge-menu-ui.
func knockoutMergeMenuUI() throws -> [String: Any] {
    var findings: [String: Any] = [:]
    for depth in [8, 16] {
        var rows: [String: Any] = [:]
        // 1-3: the root scene through Merge Down, Merge Layers (the SAME menu
        // item, multi-selected) and Merge Visible.
        rows["merge_down"] = try mergeFindings("Merge Down", depth, selection: [3],
                                               visible: false, build: rootScene)
        rows["merge_layers"] = try mergeFindings("Merge Layers", depth, selection: [2, 3],
                                                 visible: false, build: rootScene)
        rows["merge_visible"] = try mergeFindings("Merge Visible", depth, selection: [3],
                                                  visible: true, build: rootScene)
        // 4: a Shallow child of a pass-through group, merged into its lower
        // sibling with a third sibling beneath both.
        rows["pass_through_shallow"] = try mergeFindings(
            "Merge Down (pass-through Shallow)", depth, selection: [3], visible: false,
            build: passThroughScene)
        // 5: the clip run merged whole. NON-VACUITY first: the knockout has
        // to be visible in the picture before its exactness means anything.
        let (knocked, _) = try clipScene(depth, knocked: true)
        let knockedPixels = try projection(knocked)
        knocked.close(); pump()
        let (plain, _) = try clipScene(depth, knocked: false)
        let plainPixels = try projection(plain)
        plain.close(); pump()
        let visibleCodes = changed(knockedPixels, plainPixels)
        try require(visibleCodes > toleranceCodes,
            "The clipped scene cannot see its own knockout (\(visibleCodes) codes)")
        rows["clipped_member"] = try mergeFindings(
            "Merge Layers (clip run)", depth, selection: [1, 2, 3], visible: false,
            build: { try clipScene($0, knocked: true) })
        rows["clipped_member_knockout_is_visible"] = visibleCodes
        findings[depth == 8 ? "8_bit" : "16_bit"] = rows
    }
    return ["case": "knockout-merge-menu-ui", "scenes": findings,
            "scope": "Layer ▸ Merge Down / Merge Layers / Merge Visible through the editor's "
                + "own actions, with the whole projection compared pixel by pixel in the "
                + "document's own depth"]
}
