import AppKit

// "curves sheet" (`designs/v1-shortcuts-fade.md`): Image ▸ Adjustments ▸ Curves… carries
// ⌘M, and its destructive sheet applies one "Curves" step that records
// `apply_filter` with filter "curves", at 8 and 16 bits — through the menu's own item and the
// sheet's own curve and Apply button.

private func curvesSheet(_ controller: EditorViewController, _ tag: String) throws
    -> CurvesAdjustmentSheetController {
    let known = (controller.presentedViewControllers ?? []).map(ObjectIdentifier.init)
    controller.showCurves(nil)
    let opened = {
        (controller.presentedViewControllers ?? []).compactMap {
            $0 as? CurvesAdjustmentSheetController
        }.last { !known.contains(ObjectIdentifier($0)) }
    }
    try pumpUntil({ opened()?.view.window?.isVisible == true }, seconds: 30)
    guard let sheet = opened() else { throw ProbeFailure(description: "\(tag) no Curves sheet") }
    return sheet
}

private func verifyCurvesSheet(depth: Int) throws {
    let tag = "[curves sheet \(depth)-bit]"
    let document = depth == 8
        ? try fixture(width: 160, height: 120) : try precisionFixture(width: 160, height: 120)
    defer {
        if ActionRecorder.shared.isRecording { _ = ActionRecorder.shared.stop() }
        document.close()
    }
    let controller = try editor(document)
    guard let base = document.doc, base.isHighDepth == (depth == 16) else {
        throw ProbeFailure(description: "\(tag) the fixture's depth")
    }
    try require(uiValidates(controller, "showCurves:"), "\(tag) Curves… validates")
    let entries = document.history.entries.count
    let recorded = try uiRecorded {
        let sheet = try curvesSheet(controller, tag)
        guard let curve = uiViews(sheet.view, as: CurveEditorView.self).first else {
            throw ProbeFailure(description: "\(tag) no curve")
        }
        // A midtone lift: (128 → 180).
        let points = [CurvePoint(x: 0, y: 0), CurvePoint(x: 128, y: 180),
                      CurvePoint(x: 255, y: 255)]
        curve.points = points
        curve.onPointsChanged?(points)
        guard let apply = buttons(sheet.view).first(where: { $0.keyEquivalent == "\r" }) else {
            throw ProbeFailure(description: "\(tag) no Apply button")
        }
        try uiSend(apply, "\(tag) Apply")
        try pumpUntil({ document.doc !== base && sheet.view.window?.isVisible != true
                        && !BackgroundWork.occupied && DocumentJobs.active == nil }, seconds: 60)
    }
    try require(document.history.entries.count == entries + 1
                && document.undoManager?.undoActionName == "Curves",
                "\(tag) one \"Curves\" step: \(document.history.entries.map(\.name))")
    try require(recorded.map(\.tool) == ["apply_filter"]
                && recorded.first?.arguments["filter"] as? String == "curves",
                "\(tag) the recorded step: \(recorded.map { "\($0.tool) \($0.arguments)" })")
    document.undoManager?.undo()
    try require(document.doc === base, "\(tag) one undo restores the original")
    print("PASS: \(tag) Curves… applies one \"Curves\" step recorded as apply_filter curves")
}

func verifyCurvesSheets() throws {
    let tag = "[curves sheet]"
    // The menu item carries ⌘M (Window ▸ Minimize moved to ⌃⌘M).
    let (windows, help) = (NSApp.windowsMenu, NSApp.helpMenu)
    let delegate = AppDelegate()
    let menu = delegate.buildMainMenu()
    NSApp.windowsMenu = windows
    NSApp.helpMenu = help
    func find(_ menu: NSMenu) -> NSMenuItem? {
        for item in menu.items {
            if let submenu = item.submenu, let found = find(submenu) { return found }
            if item.action == #selector(EditorViewController.showCurves(_:)) { return item }
        }
        return nil
    }
    let item = withExtendedLifetime(delegate) { find(menu) }
    try require(item?.keyEquivalent == "m" && item?.keyEquivalentModifierMask == [.command],
                "\(tag) Curves… is ⌘M: \(item.map(CommandPalette.shortcutText(for:)) ?? "missing")")
    try verifyCurvesSheet(depth: 8)
    try verifyCurvesSheet(depth: 16)
}
