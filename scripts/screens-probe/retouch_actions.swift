import AppKit

// The helpers of the paint, retouch, text and shape UI actions (their `uiActions` entries are in
// actions.swift). Each calls the code a pointer, an options-bar control or a layer double-click
// calls; none synthesizes an event. What they change app-wide (tool options, the colour pair, the
// text and shape bars) is put back at teardown; the sessions they open (a held Remove stroke, a
// reopened text or shape layer, the More panel) are cancelled there (scenes.swift).

// ---- tool options

/// One tool-options family `set_tool_options` sets: its field names (a default value's stored
/// properties, which are its JSON keys), and its value read and written as a JSON object.
struct ToolOptionsGroup {
    let keys: Set<String>
    let read: () throws -> [String: Any]
    let write: ([String: Any]) throws -> Void
}

private func jsonObject(_ data: Data) throws -> [String: Any] {
    guard let object = try JSONSerialization.jsonObject(with: data) as? [String: Any] else {
        throw ProbeFailure(description: "the options did not encode as a JSON object")
    }
    return object
}

private func codableGroup<T: Codable>(_ initial: T, get: @escaping () -> T,
                                      set: @escaping (T) -> Void) -> ToolOptionsGroup {
    ToolOptionsGroup(
        keys: Set(Mirror(reflecting: initial).children.compactMap(\.label)),
        read: { try jsonObject(try JSONEncoder().encode(get())) },
        write: { set(try JSONDecoder().decode(T.self, from: try JSONSerialization.data(withJSONObject: $0))) })
}

/// The families by name: `paint:<tool>` for every tool with paint options (`paint:clone`,
/// `paint:brush`…), the Remove tool's own `removeTool`, the Gradient tool's `gradient` (its
/// stops are what Edit… opens on), and Content-Aware Fill's sheet values.
let toolOptionsGroups: [String: ToolOptionsGroup] = {
    let store = ToolOptionsStore.shared
    var groups: [String: ToolOptionsGroup] = [
        "removeTool": codableGroup(RemoveToolOptions(), get: { store.removeTool }, set: { store.removeTool = $0 }),
        "gradient": codableGroup(GradientToolOptions(), get: { store.gradient }, set: { store.gradient = $0 }),
        "contentAwareFill": codableGroup(ContentAwareFillOptions(), get: { store.contentAwareFill },
                                         set: { store.contentAwareFill = $0 }),
    ]
    for tool in EditorTool.allCases where store.paintOptions(for: tool) != nil {
        groups["paint:\(tool.identifier)"] = codableGroup(
            PaintToolOptions(), get: { store.paintOptions(for: tool) ?? PaintToolOptions() },
            set: { store.setPaintOptions($0, for: tool) })
    }
    return groups
}()

/// `set_tool_options`: the family's stored value with `values` laid over it, as the options bar's
/// controls write it, then the bar rebuilt and the canvas given the new tip. Every given key must
/// come back as given (a value the store clamps or refuses fails the step). The family's value
/// before the step is put back at teardown.
func setToolOptions(_ name: String, _ group: ToolOptionsGroup, _ values: [String: Any],
                    _ context: Context) throws {
    let editor = try context.requireEditor()
    let before = try group.read()
    context.restorers.append { [weak editor] in
        try? group.write(before)
        editor?.updateOptionsBar()
        editor?.syncCanvasPaintState()
    }
    var merged = before
    for (key, value) in values { merged[key] = value }
    try group.write(merged)
    let after = try group.read()
    for (key, value) in values {
        guard let stored = after[key] as? NSObject, let wanted = value as? NSObject, stored.isEqual(wanted) else {
            throw ProbeFailure(description: "\(name).\(key) is \(after[key] ?? "unset"), not \(value)")
        }
    }
    editor.updateOptionsBar()
    editor.syncCanvasPaintState()
    editor.optionsBar.refreshValues()
}

// ---- the canvas

/// `hover`: the pointer resting over the canvas at `point` (canvas pixels), as a mouse-moved
/// event leaves it: the brush tip's outline drawn there, and for the Clone Stamp and the Healing
/// Brush the sampled source shown inside it. The point must be in the visible part of the canvas.
func hover(_ point: CGPoint, _ editor: EditorViewController) throws {
    let canvas = editor.canvas
    try require(canvas.tool.usesBrushTip, "the \(canvas.tool.identifier) tool draws no brush tip")
    let local = canvas.convert(point, to: nil)
    try require(canvas.visibleRect.contains(point), "\(point) is outside the visible canvas \(canvas.visibleRect)")
    canvas.selectionCursorHovered = true
    canvas.refreshHoverPoint(windowLocation: local)
    try require(canvas.brushOutline != nil, "no brush outline at \(point) (is the tip too small to draw?)")
    canvas.needsDisplay = true
}

/// `set_clone_source`: the ⌥-click that sets the Clone Stamp's or Healing Brush's source point.
func setCloneSource(_ point: CGPoint, _ editor: EditorViewController) throws {
    try require([.clone, .heal].contains(editor.currentTool), "the current tool takes no source point")
    let size = editor.canvas.bounds.size
    try require(CGRect(origin: .zero, size: size).contains(point), "\(point) is outside the \(size) canvas")
    editor.canvas.cloneSource = point
}

/// `hold_remove_stroke`: a Remove drag along `points` with a `size` px brush, released with
/// Remove after each stroke off, so the stroke is held over the canvas until ✓ or ✕.
func holdRemoveStroke(_ points: [CGPoint], size: Double, _ editor: EditorViewController) throws {
    try require(editor.currentTool == .remove, "the Remove tool is not the current tool (select_tool remove first)")
    try require(!ToolOptionsStore.shared.removeTool.removeAfterEachStroke,
                "Remove after each stroke is on, so the stroke would run at once "
                + "(set_tool_options removeTool removeAfterEachStroke false first)")
    editor.removeStrokeCommitted(RemoveSession.Result(points: points, size: CGFloat(size)))
    try require(editor.canvas.removeSession.hasPending, "the stroke was not held")
}

// ---- the options bar

/// `open_overflow`: the options bar's More button, which hangs the More panel under it. The panel
/// is a child of the editor window that hides while the probe is inactive, so it is kept shown and
/// ordered just above the editor window; it closes when it loses key, so nothing else may become
/// key before the capture.
func openOverflow(_ context: Context) throws {
    let editor = try context.requireEditor()
    guard let window = context.window else { throw ProbeFailure(description: "no editor window") }
    let more = descendants(editor.optionsBar).compactMap { $0 as? MoreButton }
    guard more.count == 1, let press = more[0].onPress else {
        throw ProbeFailure(description: "\(more.count) More buttons in the options bar")
    }
    press()
    guard let panel = window.childWindows?.first(where: { $0 is OverflowPanel }) as? NSPanel else {
        throw ProbeFailure(description: "the More panel did not open")
    }
    panel.hidesOnDeactivate = false
    panel.order(.above, relativeTo: window.windowNumber)
    settle(panel, seconds: 0.5)
    try require(panel.isVisible, "the More panel is not visible")
    let texts = descendants(panel.contentView ?? NSView()).compactMap { $0 as? NSTextField }.map(\.stringValue)
    try require(!texts.contains("No further options"), "the More panel holds no options")
}

// ---- the colour pair

/// Puts the colour pair (the rail's swatches and the stored pair) back at teardown as it is now.
func preserveColors(_ editor: EditorViewController, _ context: Context) {
    let shared = ToolOptionsStore.shared.sharedState
    let foreground = editor.paintColor, background = editor.backgroundColor
    context.restorers.append { [weak editor] in
        editor?.setPaintColor(foreground)
        editor?.setBackgroundColor(background)
        ToolOptionsStore.shared.sharedState = shared
    }
}

/// `set_colors`: the foreground and background swatches, as the Color Picker's OK writes them.
func setColors(foreground: NSColor, background: NSColor, _ context: Context) throws {
    let editor = try context.requireEditor()
    preserveColors(editor, context)
    editor.setPaintColor(foreground)
    editor.setBackgroundColor(background)
    editor.optionsBar.refreshValues()
}

// ---- described layers

/// `reopen_text`: a double-click on a text layer with the Type tool, which opens the on-canvas
/// editor on its string, the caret at its end, and gives the Type bar the layer's own settings.
/// The bar's settings and the foreground colour it takes are put back at teardown, which also
/// cancels the session.
func reopenText(layer: Int, _ context: Context) throws {
    let editor = try context.requireEditor()
    guard let doc = try context.requireDocument().doc, doc.displayedTextPayload(layer) != nil else {
        throw ProbeFailure(description: "layer \(layer) is not a text layer")
    }
    preserveColors(editor, context)
    let text = ToolOptionsStore.shared.text
    context.restorers.append { ToolOptionsStore.shared.text = text }
    editor.selectTool(.text)
    editor.openTextSession(layer: layer, selectAll: false)
    try require(editor.canvas.hasActiveTextSession, "no text session opened on layer \(layer)")
}

/// `reopen_shape`: a double-click on a shape layer's row, which reopens it as an editable box
/// with its handles and gives the shape bar the layer's own fill, stroke and corners. The bar's
/// settings are put back at teardown, which also cancels the session.
func reopenShape(layer: Int, _ context: Context) throws {
    let editor = try context.requireEditor()
    guard try context.requireDocument().doc?.shapePayload(layer) != nil else {
        throw ProbeFailure(description: "layer \(layer) is not a shape layer")
    }
    let shape = ToolOptionsStore.shared.shape
    context.restorers.append { ToolOptionsStore.shared.shape = shape }
    editor.editShapeLayer(layer)
    try require(editor.shapeEditSession != nil, "no shape session opened on layer \(layer)")
}
