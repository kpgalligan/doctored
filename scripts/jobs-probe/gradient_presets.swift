import AppKit

// The gradient library in the UI (`GradientPresetLibrary`, `GradientPresetRow`), on the probe's
// private `DR_GRADIENTS_DIR`:
//
// (a) The Gradient tool's stops sheet: the shared stop editor's Preset row lists a saved gradient
//     between its face and Save Preset…; choosing it sets the pane's stops and the sheet's
//     Smoothness field.
// (b) Edit ▸ Gradients holds Delete Gradient…, Import Gradients… and Export Gradients…, each an
//     enabled nil-target action; Delete Gradient… (its alert answered by the automation hook)
//     removes the gradient.

/// Every view under `view`, depth first.
private func allViews(_ view: NSView) -> [NSView] {
    view.subviews.flatMap { [$0] + allViews($0) }
}

func verifyGradientPresets() throws {
    let tag = "[gradient presets]"
    for entry in try GradientPresetLibrary.listing().entries {
        try GradientPresetLibrary.delete(name: entry.item.name)
    }
    let stops = [
        GradientStop(position: 0, color: "#102030", opacity: 1),
        GradientStop(position: 0.6, color: "#c04080", opacity: 0.7, midpoint: 0.3),
        GradientStop(position: 1, color: "#f0e0a0", opacity: 1),
    ]
    let saved = try GradientPresetLibrary.save(
        name: "Probe Ramp", stops: stops, smoothness: 0.25, replace: false)
    defer { try? GradientPresetLibrary.delete(name: "Probe Ramp") }

    // (a) The stops sheet's Preset row.
    let sheet = GradientStopsSheetController(foreground: .black, background: .white) {}
    let views = allViews(sheet.view)
    guard let pane = views.compactMap({ $0 as? GradientStopsPane }).first,
          let field = views.compactMap({ $0 as? NSTextField })
            .first(where: { $0.identifier?.rawValue == "gradient-stops-smoothness" })
    else { throw ProbeFailure(description: "\(tag) the sheet's pane and Smoothness field") }
    let titles = pane.presetTitles
    try require(titles.first == GradientPresetRow.title && titles.contains("Probe Ramp")
                && titles.last == GradientPresetRow.saveTitle,
                "\(tag) the Preset row lists the library: \(titles)")
    pane.applyPreset(saved)
    try require(pane.fill.stops == saved.stops, "\(tag) the preset's stops: \(pane.fill.stops)")
    try require(abs(field.doubleValue - 25) < 1e-9,
                "\(tag) the sheet's Smoothness follows the preset: \(field.doubleValue)")

    // (b) Edit ▸ Gradients.
    let document = try fixture(width: 40, height: 30)
    defer { document.close() }
    let controller = try editor(document)
    guard let menu = AppDelegate().editMenu().items.first(where: { $0.title == "Gradients" })?
        .submenu else { throw ProbeFailure(description: "\(tag) Edit has no Gradients submenu") }
    let items = menu.items.map(\.title)
    try require(items == ["Delete Gradient…", "Import Gradients…", "Export Gradients…"],
                "\(tag) the Gradients submenu: \(items)")
    for item in menu.items {
        try require(item.target == nil && item.action.map { controller.responds(to: $0) } == true
                    && controller.validateUserInterfaceItem(item),
                    "\(tag) \(item.title) is an enabled nil-target action")
    }
    AutomationAlerts.reset()
    guard let delete = menu.items.first, let action = delete.action else {
        throw ProbeFailure(description: "\(tag) no Delete Gradient… item")
    }
    _ = controller.perform(action, with: delete)
    try require(AutomationAlerts.recorded.count == 1,
                "\(tag) Delete Gradient… asks once: \(AutomationAlerts.recorded)")
    let left = try GradientPresetLibrary.listing().entries
    try require(left.isEmpty, "\(tag) Delete Gradient… removed the gradient")
    print("PASS: the stop editor's Preset row and Edit ▸ Gradients")
}
