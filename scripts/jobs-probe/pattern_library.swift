import AppKit

// Edit ▸ Define Pattern…, the user pattern library and Fill…'s Pattern contents in the UI. The
// runner gives the probe a private DR_PATTERNS_DIR (`PRESET_DIRECTORIES`). Every step calls the
// app's own methods and controls, so none needs a key window.
//
// (a) Define Pattern's tile is the fixture's composite inside a rectangular selection, byte for
//     byte; a feathered selection refuses by name; the sheet's commit saves it and refuses a
//     taken name; the item validates on a layer.
// (b) Every picker lists the library: `PatternChoice.all` is the four built-ins then the
//     library, `PatternFillRows`' popup shows the library pattern with its swatch and choosing it
//     embeds its tile, and a held tile equal to it shows as that row, not "Custom".
// (c) The Fill sheet offers Pattern on a layer target with a pattern popup listing the library,
//     and Apply hands that tile to the commit; a coverage target and Quick Mask omit Pattern.
// (d) `applyFill` with Pattern contents fills the selection by the canvas-grid rule, as one
//     undo step, and undo restores the layer.
// (e) Edit ▸ Patterns holds Delete Pattern…, Import Patterns… and Export Patterns…, each enabled;
//     Export's commit writes the library, Delete Pattern… sent to the editor with its menu item
//     removes its first row (the recorded alert's first button), and Import's commit restores
//     it. The two file panels are not driven: only their commits are called.

private let side = 40, tall = 30

/// The fixture's straight RGBA8 at (x, y) (`fixture`'s own formula).
private func fixtureTexel(_ x: Int, _ y: Int) -> [UInt8] {
    [UInt8(40 + (x * 3 + y * 5) % 170), UInt8(40 + (x * 7 + y * 11) % 170),
     UInt8(40 + (x * 13 + y * 3) % 170), 255]
}

func verifyPatternLibrary() throws {
    let tag = "[pattern library]"
    let document = try fixture(width: side, height: tall)
    defer { document.close() }
    let controller = try editor(document)
    guard let doc = document.doc else { throw ProbeFailure(description: "\(tag) no document") }

    // (a) Define Pattern's tile, refusal, commit and validation.
    try require(uiValidates(controller, "definePattern:"), "\(tag) Define Pattern… is enabled")
    let window = CGRect(x: 5, y: 6, width: 4, height: 3)
    let rect = CanvasSelection(shape: .rect(window), canvasWidth: side, canvasHeight: tall)
    let tile = try DefinePattern.tile(of: doc, in: document, selection: rect)
    let expected = (6..<9).flatMap { y in (5..<9).flatMap { x in fixtureTexel(x, y) } }
    try require(tile.width == 4 && tile.height == 3 && tile.source == "selection"
                && [UInt8](tile.rgba) == expected, "\(tag) the tile is the composite in the rectangle")
    var soft = [UInt8](repeating: 0, count: side * tall)
    for y in 6..<9 { for x in 5..<9 { soft[y * side + x] = x == 5 ? 128 : 255 } }
    let feathered = CanvasSelection(shape: .mask(soft), canvasWidth: side, canvasHeight: tall)
    do {
        _ = try DefinePattern.tile(of: doc, in: document, selection: feathered)
        throw ProbeFailure(description: "\(tag) a feathered selection defined a pattern")
    } catch let error as DoctoredCoreError {
        try require(error.message == DefinePattern.notRectangle, "\(tag) the refusal: \(error.message)")
    }
    try require(EditorViewController.savePattern(name: "Probe", tile: tile) == nil,
                "\(tag) the commit saves the tile")
    let taken = EditorViewController.savePattern(name: "probe", tile: tile) ?? ""
    try require(taken.contains("already exists"), "\(tag) a taken name refuses: \(taken)")

    // (b) The pickers.
    let library = try PatternLibrary.named("Probe").ref
    let choices = PatternChoice.all()
    try require(choices.map(\.key) == ["preset:checkerboard", "preset:stripes", "preset:dots",
                                       "preset:grid", "library:Probe"],
                "\(tag) built-ins then the library: \(choices.map(\.key))")
    var fill = PatternFill(pattern: PatternRef(preset: "checkerboard"))
    let binding = EffectBinding<PatternFill>(get: { fill }, set: { fill = $0 })
    // The rows are kept alive beside their popup: a row is its popup's (weak) target.
    func patternPopup() throws -> ([PaneRow], NSPopUpButton) {
        let rows = PatternFillRows.make(binding: binding, layerOrigin: { nil }, layerOffset: { nil })
        guard let popup = rows.first?.views.compactMap({ $0 as? NSPopUpButton }).first else {
            throw ProbeFailure(description: "\(tag) the Pattern row has no popup")
        }
        return (rows, popup)
    }
    let (rows, popup) = try patternPopup()
    guard let row = popup.itemArray.first(where: {
        ($0.representedObject as? String) == "library:Probe"
    }) else { throw ProbeFailure(description: "\(tag) the popup lists no library pattern") }
    try require(row.image != nil, "\(tag) the library row has its swatch")
    popup.select(row)
    try uiSend(popup, "\(tag) choosing the library pattern")
    try require(fill.pattern == library, "\(tag) choosing it embeds its tile")
    let (heldRows, held) = try patternPopup()
    try require((held.selectedItem?.representedObject as? String) == "library:Probe"
                && !held.itemArray.contains { ($0.representedObject as? String) == "custom" },
                "\(tag) a held library tile shows as its row")
    withExtendedLifetime((rows, heldRows)) {}

    // (c) The Fill sheet.
    var applied: FillSettings?
    let sheet = FillSheetController(coverage: false, quickMask: false,
                                    options: FillDialogOptions()) { applied = $0 }
    controller.presentAsSheet(sheet)
    let contents = uiViews(sheet.view, as: NSPopUpButton.self).first {
        $0.itemTitles.contains("Foreground Color")
    }
    guard let contents, let patternItem = contents.item(withTitle: "Pattern") else {
        throw ProbeFailure(description: "\(tag) a layer's Fill sheet offers no Pattern contents")
    }
    contents.select(patternItem)
    try uiSend(contents, "\(tag) Contents Pattern")
    let patterns = try uiControl(sheet.view, id: "fill-pattern", as: NSPopUpButton.self)
    try require(patterns.itemTitles == ["Checkerboard", "Stripes", "Dots", "Grid", "Probe"],
                "\(tag) the sheet's patterns: \(patterns.itemTitles)")
    patterns.selectItem(withTitle: "Probe")
    guard let apply = buttons(sheet.view).first(where: { $0.title == "Apply" }) else {
        throw ProbeFailure(description: "\(tag) the sheet has no Apply")
    }
    try uiSend(apply, "\(tag) Apply")
    try require(applied?.contents == .pattern && applied?.pattern == library,
                "\(tag) Apply hands the library tile to the commit")
    for (coverage, quickMask) in [(true, false), (true, true)] {
        let other = FillSheetController(coverage: coverage, quickMask: quickMask,
                                        options: FillDialogOptions()) { _ in }
        let titles = uiViews(other.view, as: NSPopUpButton.self).flatMap(\.itemTitles)
        try require(!titles.contains("Pattern"), "\(tag) coverage \(coverage), Quick Mask "
                    + "\(quickMask): no Pattern contents")
    }

    // (d) The commit, over the 4 × 3 rectangle at (5, 6).
    guard let settings = applied, let before = doc.layerPixels(0) else {
        throw ProbeFailure(description: "\(tag) no settings or pixels")
    }
    document.setCanvasSelection(rect)
    let steps = document.history.entries.count
    controller.applyFill(settings, note: "probe")
    guard let after = document.doc?.layerPixels(0) else {
        throw ProbeFailure(description: "\(tag) no pixels after the fill")
    }
    try require(document.history.entries.count == steps + 1, "\(tag) one undo step")
    for y in 0..<tall {
        for x in 0..<side {
            let at = (y * side + x) * 4
            let inside = window.contains(CGPoint(x: Double(x) + 0.5, y: Double(y) + 0.5))
            // Canvas pixel (x, y) takes texel (x mod 4, y mod 3) of the source rectangle.
            let want = inside ? fixtureTexel(5 + x % 4, 6 + y % 3) : Array(before[at..<at + 4])
            try require(Array(after[at..<at + 4]) == want, "\(tag) pixel (\(x), \(y))")
        }
    }
    document.undoManager?.undo()
    try require(document.doc?.layerPixels(0) == before, "\(tag) undo restores the layer")

    // (e) Edit ▸ Patterns, found by title and sent by selector as a menu would send them.
    guard let patternsMenu = AppDelegate().editMenu().items.first(where: {
        $0.title == "Patterns"
    })?.submenu else { throw ProbeFailure(description: "\(tag) Edit has no Patterns submenu") }
    let titles = patternsMenu.items.map(\.title)
    try require(titles == ["Delete Pattern…", "Import Patterns…", "Export Patterns…"],
                "\(tag) the Patterns submenu: \(titles)")
    for item in patternsMenu.items {
        try require(item.target == nil && item.action.map { controller.responds(to: $0) } == true
                    && controller.validateUserInterfaceItem(item),
                    "\(tag) \(item.title) is an enabled nil-target action")
    }
    let file = FileManager.default.temporaryDirectory
        .appendingPathComponent("probe-\(UUID().uuidString).\(PatternLibrary.fileExtension)")
    defer { try? FileManager.default.removeItem(at: file) }
    try require(EditorViewController.exportPatternFile(to: file) == nil,
                "\(tag) Export Patterns…' commit writes the file")
    let deleteRows = EditorViewController.patternDeleteRows(try PatternLibrary.listing())
    let deleteTitles = deleteRows.map { $0.title }
    try require(deleteTitles == ["Probe"] && deleteRows.first?.image != nil,
                "\(tag) Delete Pattern… lists the library with swatches: \(deleteTitles)")
    AutomationAlerts.reset()
    guard let delete = patternsMenu.items.first, let action = delete.action else {
        throw ProbeFailure(description: "\(tag) no Delete Pattern… item")
    }
    _ = controller.perform(action, with: delete)
    try require(AutomationAlerts.recorded.count == 1,
                "\(tag) Delete Pattern… asks once: \(AutomationAlerts.recorded)")
    let left = try PatternLibrary.listing().entries
    try require(left.isEmpty, "\(tag) Delete Pattern… removed the chosen pattern")
    try require(EditorViewController.importPatternFile(file) == nil,
                "\(tag) Import Patterns…' commit imports every entry")
    let restored = try PatternLibrary.named("Probe").ref
    try require(restored == library, "\(tag) the import restores the same tile")
    print("PASS: Define Pattern's tile and refusal, the library in every picker and the Fill "
          + "sheet, Fill…'s Pattern contents on the canvas grid as one undo step, and Edit ▸ "
          + "Patterns' Delete, Import and Export")
}
