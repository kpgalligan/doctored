import AppKit

// The helpers of the selection, channel, path and transform UI actions (their `uiActions` entries
// are in actions.swift). Each calls the code a click, a panel row or a menu item calls; none
// synthesizes an event. The two canvas sessions they open (Free Transform, Puppet Warp) are
// cancelled at teardown (scenes.swift), never committed.

/// `color_range_sample`: a click on Color Range's captured image at `point` (canvas pixels), which
/// samples that pixel's colour and requests the coverage preview.
func colorRangeSample(_ point: CGPoint, _ context: Context) throws {
    guard let sheet = context.window?.attachedSheet?.contentViewController as? ColorRangeSheetController else {
        throw ProbeFailure(description: "no Color Range sheet is open (open_sheet color_range first)")
    }
    let size = CGSize(width: sheet.capture.doc.width, height: sheet.capture.doc.height)
    try require(CGRect(origin: .zero, size: size).contains(point), "\(point) is outside the \(size) canvas")
    sheet.sample(point)
}

/// `set_current_path`: a click on the path's row in the Paths panel, which makes it the current
/// path (the canvas outlines it and the Pen edits it). `set_path` stores a path without making it
/// current.
func setCurrentPath(named name: String, _ editor: EditorViewController) throws {
    guard let doc = editor.document?.doc else { throw ProbeFailure(description: "no document") }
    let paths = doc.paths
    guard let entry = paths.entry(named: name) else {
        throw ProbeFailure(description: "no path \"\(name)\"; the paths are \(paths.entries.map(\.name))")
    }
    editor.setCurrentPath(.path(entry.id))
    try require(editor.document?.currentPath == .path(entry.id), "the editor kept another current path")
}

/// The index of the document's alpha channel called `name`.
private func channelIndex(named name: String, _ editor: EditorViewController) throws -> Int {
    guard let doc = editor.document?.doc else { throw ProbeFailure(description: "no document") }
    let names = (0..<doc.channelCount).map { doc.channelInfo($0)?.name ?? "" }
    guard let index = names.firstIndex(of: name) else {
        throw ProbeFailure(description: "no channel \"\(name)\"; the channels are \(names)")
    }
    return index
}

/// `set_channel_target`: a click on the alpha channel's row in the Channels panel, which makes it
/// the edit target. The target's one writer moves the canvas, both panels and the status bar.
func setChannelTarget(named name: String, _ editor: EditorViewController) throws {
    let index = try channelIndex(named: name, editor)
    editor.setPaintTarget(.channel(index))
    try require(editor.paintTarget == .channel(index), "the editor kept the target \(editor.paintTarget)")
}

/// `set_alpha_channel_visibility`: the Channels panel's eye column with the RGB eye `rgb` and the
/// eyes of the named alpha channels on (every other alpha channel's off). An alpha channel shown
/// over the picture is its rubylith.
func setAlphaChannelVisibility(rgb: Bool, _ names: [String], _ context: Context) throws {
    let editor = try context.requireEditor()
    var visibility = try context.requireDocument().channelVisibility
    visibility.rgb = rgb
    visibility.channels = Set(try names.map { try channelIndex(named: $0, editor) })
    editor.restoreChannelVisibility(visibility)
}

/// `free_transform`: Layer ▸ Free Transform on the active layer (with no selection), which opens
/// the on-canvas box and the Free Transform bar.
func freeTransform(_ editor: EditorViewController) throws {
    try require(editor.document?.canvasSelection == nil,
                "a selection is active, so Free Transform would float the selected pixels (deselect first)")
    editor.freeTransform(nil)
    try require(editor.transformSession != nil, "no transform session opened")
}

/// `set_transform`: the Free Transform bar's Angle (degrees, clockwise) and Scale X and Y (1 is
/// 100%), as its fields set the session's parameters; the preview and the bar follow.
func setTransform(angle: Double, scaleX: Double, scaleY: Double, _ editor: EditorViewController) throws {
    guard editor.transformSession != nil else {
        throw ProbeFailure(description: "no transform session (free_transform first)")
    }
    editor.transformSession?.transform.angle = CGFloat(angle * .pi / 180)
    editor.transformSession?.transform.scaleX = CGFloat(scaleX)
    editor.transformSession?.transform.scaleY = CGFloat(scaleY)
    editor.refreshTransformPreview()
    editor.optionsBar.refreshValues()
}

/// `puppet_warp`: Edit ▸ Puppet Warp on the active layer, which builds the mesh over its opaque
/// pixels and shows the Puppet Warp bar.
func openPuppetWarp(_ editor: EditorViewController) throws {
    editor.showPuppetWarp(nil)
    try require(editor.canvas.puppetWarpSession != nil, "no Puppet Warp session opened")
}

/// `puppet_pin`: a press at `from` (canvas pixels), which adds a pin at the nearest mesh vertex or
/// selects the pin there, a drag to `to` and a release, as the canvas routes the mouse into the
/// session.
func puppetPin(from: CGPoint, to: CGPoint, _ editor: EditorViewController) throws {
    let canvas = editor.canvas
    guard let before = canvas.puppetWarpSession?.pins.count else {
        throw ProbeFailure(description: "no Puppet Warp session (puppet_warp first)")
    }
    try canvas.puppetWarpSession?.press(at: from, modifiers: [], scale: canvas.magnification)
    try require(canvas.puppetWarpSession?.drag != nil, "the press at \(from) is outside the mesh")
    if from != to { try canvas.puppetWarpSession?.dragged(to: to) }
    canvas.puppetWarpSession?.released()
    canvas.onPuppetWarpChange?()
    try require((canvas.puppetWarpSession?.pins.count ?? 0) >= before, "a pin was removed")
}
