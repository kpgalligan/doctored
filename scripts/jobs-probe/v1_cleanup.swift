import AppKit

// The v1 cleanup batch: the Paths panel follows the active layer (a current
// path on another layer's vector mask is dropped, a list path stays), and
// Lens Correction's grid overlay lives exactly as long as its sheet.

/// A square outline as the MCP `subpaths` argument.
private let cleanupSquare: [[String: Any]] = [
    ["closed": true, "anchors": [[20, 20], [120, 20], [120, 100], [20, 100]]],
]

/// "paths follow active layer": a vector mask made current on layer 0, then
/// the active layer moved by `setActiveLayer`, by the Layers panel's own
/// callback and by MCP `set_active_layer` — each clears the current path, the
/// panel's row and the Pen's target. A list path survives every switch. MCP
/// `new_layer` moves the active layer after its edit's notification, so the
/// stale mask is caught where it is used: no current path step, Make
/// Selection disabled, and a Pen commit refused without touching the mask.
func verifyPathsFollowActiveLayer() throws {
    let tag = "[paths follow active layer]"
    let document = try fixture(width: 200, height: 160)
    defer { document.close() }
    let controller = try editor(document)
    _ = try agentOK("new_layer", document)
    _ = try agentOK("set_vector_mask", document, ["layer": 0, "subpaths": cleanupSquare])
    _ = try agentOK("set_path", document, ["name": "Kept", "subpaths": cleanupSquare])
    guard let doc = document.doc, doc.layerCount == 2, let layer0 = doc.layerInfo(0)?.id,
          let kept = doc.paths.entry(named: "Kept")?.id else {
        throw ProbeFailure(description: "\(tag) setup: two layers, a vector mask and a saved path")
    }
    controller.showPathsTab()
    let mask = PathTarget.vectorMask(layerID: layer0)

    func makeMaskCurrent(_ step: String) throws {
        controller.setActiveLayer(0)
        controller.setCurrentPath(mask)
        try require(document.activeLayerIndex == 0 && document.currentPath == mask
                    && controller.canvas.penSession.target == mask,
                    "\(tag) \(step): layer 0's vector mask is current")
        try require(controller.pathsPanel.selectedRow?.target == mask,
                    "\(tag) \(step): the panel selects the vector-mask row")
    }
    func requireCleared(_ step: String) throws {
        try require(document.currentPath == nil, "\(tag) \(step): the current path is cleared")
        try require(controller.pathsPanel.selectedRow == nil,
                    "\(tag) \(step): no Paths row is selected")
        try require(controller.canvas.penSession.target == nil,
                    "\(tag) \(step): the Pen no longer edits the mask")
    }

    try makeMaskCurrent("setActiveLayer")
    controller.setActiveLayer(1)
    try requireCleared("setActiveLayer(1)")

    try makeMaskCurrent("Layers panel")
    document.activeLayerIndex = 1
    controller.layersPanel.onActiveLayerChange?()
    try requireCleared("Layers panel onActiveLayerChange")

    try makeMaskCurrent("MCP set_active_layer")
    _ = try agentOK("set_active_layer", document, ["index": 1])
    try requireCleared("MCP set_active_layer")

    // A list path stays current through every switch.
    controller.setActiveLayer(0)
    controller.setCurrentPath(.path(kept))
    controller.setActiveLayer(1)
    try require(document.currentPath == .path(kept), "\(tag) a list path survives setActiveLayer")
    document.activeLayerIndex = 0
    controller.layersPanel.onActiveLayerChange?()
    try require(document.currentPath == .path(kept), "\(tag) a list path survives the Layers panel")
    _ = try agentOK("set_active_layer", document, ["index": 1])
    try require(document.currentPath == .path(kept) && controller.canvas.penSession.target == .path(kept),
                "\(tag) a list path survives MCP set_active_layer")

    // MCP new_layer: the active layer moves after the notification.
    try makeMaskCurrent("MCP new_layer")
    let before = document.doc?.vectorMask(0)
    _ = try agentOK("new_layer", document)
    try require(document.activeLayerIndex != 0, "\(tag) new_layer moved the active layer")
    try require(controller.currentPathStep() == nil,
                "\(tag) another layer's vector mask is no current path step")
    let item = NSMenuItem(title: "Make Selection…",
                          action: #selector(EditorViewController.makeSelectionFromPath(_:)),
                          keyEquivalent: "")
    try require(controller.validatePathsItem(item) == false,
                "\(tag) Make Selection validates disabled for a stale vector mask")
    guard let commit = controller.canvas.onPenCommit else {
        throw ProbeFailure(description: "\(tag) the canvas has no Pen commit")
    }
    let triangle = try PathShape(json: [
        "subpaths": [["closed": true, "anchors": [[30, 30], [90, 30], [60, 90]]]],
    ] as [String: Any])
    try require(!commit(triangle, "Pen"), "\(tag) the Pen commit on a stale vector mask is refused")
    try require(document.doc?.vectorMask(0) == before, "\(tag) layer 0's vector mask is unchanged")
    try require(document.currentPath == nil && controller.canvas.penSession.target == nil,
                "\(tag) the stale vector mask is no longer current")
    print("PASS: the current path follows the active layer (setActiveLayer, Layers panel, MCP "
          + "set_active_layer), a list path stays, and after MCP new_layer the stale vector "
          + "mask is no step, Make Selection is disabled and a Pen commit is refused")
}

/// "lens grid overlay": the Show Grid overlay is on the canvas while the
/// Lens Correction sheet is up and ticked, and gone after Cancel and Apply.
func verifyLensGridOverlay() throws {
    let tag = "[lens grid overlay]"
    let document = try fixture(width: 200, height: 160)
    defer { document.close() }
    let controller = try editor(document)
    let overlays = { controller.canvas.subviews.filter { $0 is LensGridOverlayView }.count }
    func press(_ button: NSButton?, _ what: String) throws {
        guard let button, let action = button.action else {
            throw ProbeFailure(description: "\(tag) \(what): the button is missing")
        }
        try require(NSApp.sendAction(action, to: button.target, from: button), "\(tag) \(what) was delivered")
    }
    func present() throws -> SliderSheetController {
        let sheet = SliderSheetController.lensCorrection(document: document, canvas: controller.canvas)
        controller.presentAsSheet(sheet)
        try pumpUntil({ sheet.view.window?.isVisible == true }, seconds: 30)
        return sheet
    }

    let first = try present()
    try require(overlays() == 1, "\(tag) one grid overlay while the sheet is up: \(overlays())")
    guard let box = buttons(first.view).first(where: { $0.title == "Show Grid" }) else {
        throw ProbeFailure(description: "\(tag) the sheet has no Show Grid checkbox")
    }
    box.state = .off
    try press(box, "unticking Show Grid")
    try require(overlays() == 0, "\(tag) unticking Show Grid removes the overlay: \(overlays())")
    box.state = .on
    try press(box, "ticking Show Grid")
    try require(overlays() == 1, "\(tag) ticking Show Grid puts it back: \(overlays())")
    try press(buttons(first.view).first { $0.title == "Cancel" }, "Cancel")
    try pumpUntil({ first.view.window?.isVisible != true }, seconds: 30)
    try require(overlays() == 0, "\(tag) Cancel removes the overlay: \(overlays())")

    let second = try present()
    try require(overlays() == 1, "\(tag) the reopened sheet shows the overlay: \(overlays())")
    try press(buttons(second.view).first { $0.keyEquivalent == "\r" }, "Apply")
    try pumpUntil({ second.view.window?.isVisible != true && !BackgroundWork.occupied }, seconds: 30)
    try require(overlays() == 0, "\(tag) Apply removes the overlay: \(overlays())")
    print("PASS: Lens Correction's grid overlay shows with the sheet, follows Show Grid, and is "
          + "removed by Cancel and by Apply")
}
