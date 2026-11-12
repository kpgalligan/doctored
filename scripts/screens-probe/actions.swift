import AppKit

// The UI actions a screen's `{"ui": <name>, "args": {…}}` steps name: one `uiActions` entry each,
// with its scope, its exact argument keys and a parser that reads the arguments when the plan is
// parsed and returns what the step runs. Every action calls the app's own code: no mouse or key
// event is synthesized and the app never activates. A new action is one entry here.
//
// `run_modal_card` is not here: it splits a screen's steps (plan.swift) and its cards are
// windows.swift's `cards`.

let uiActions: [String: ActionSpec] = [
    "select_tool": ActionSpec(.editor, ["tool"]) { arguments in
        let id = try arguments.string("tool")
        guard let tool = EditorTool.named(id) else { throw arguments.fail("no tool \"\(id)\"") }
        return { context in
            let editor = try context.requireEditor()
            editor.selectTool(tool)
            try require(editor.currentTool == tool, "the editor refused the \(id) tool")
        }
    },
    "show_panel_tab": ActionSpec(.editor, ["tab"]) { arguments in
        let id = try arguments.string("tab")
        guard let tab = EditorPanelTab(name: id) else { throw arguments.fail("no tab \"\(id)\"") }
        return { try $0.requireEditor().applyPanelLayout(visible: true, tab: tab) }
    },
    "open_sheet": ActionSpec(.editor, ["sheet"]) { arguments in
        let name = try arguments.string("sheet")
        guard let open = sheetOpeners[name] else { throw arguments.fail("no sheet \"\(name)\"") }
        return { try openSheet(name, open, $0) }
    },
    "show_window": ActionSpec(.editor, ["window"]) { arguments in
        let name = try arguments.string("window")
        guard let panel = panels[name], let show = panel.show else {
            throw arguments.fail("no window \"\(name)\" that show_window opens")
        }
        return { context in try showPanel(name, show: show, find: panel.window, context) }
    },
    "zoom_to_fit": ActionSpec(.editor, []) { _ in
        return { try $0.requireEditor().zoomToFit() }
    },
    "zoom_to_rect": ActionSpec(.editor, ["rect"]) { arguments in
        let rect = try arguments.rect("rect")
        return { try $0.requireEditor().zoomToRect(rect) }
    },
    "set_crop_box": ActionSpec(.editor, ["rect"]) { arguments in
        let rect = try arguments.rect("rect")
        return { try setCropBox(rect, try $0.requireEditor()) }
    },
    "move_cursor": ActionSpec(.editor, ["point"]) { arguments in
        let point = try arguments.point("point")
        // The pointer's canvas position as the canvas reports it: the Info readout and the
        // ruler marks follow it.
        return { try $0.requireEditor().cursorMoved(to: point) }
    },
    "wait": ActionSpec(.any, ["seconds"]) { arguments in
        let seconds = try arguments.number("seconds", in: 0.01...10)
        return { settle($0.card ?? $0.window, seconds: seconds) }
    },
    "set_control": ActionSpec(.dialog, ["control", "value"]) { arguments in
        let name = try arguments.string("control")
        let value = try arguments.scalar("value")
        return { context in try setControl(try dialogControl(try context.dialog(), name), to: value, name) }
    },
    "choose_popup_item": ActionSpec(.dialog, ["title"]) { arguments in
        let title = try arguments.string("title")
        return { context in try choosePopupItem(title, in: try context.dialog()) }
    },
    "press_button": ActionSpec(.dialog, ["id"]) { arguments in
        let id = try arguments.string("id")
        return { context in
            guard let button = try dialogControl(try context.dialog(), id) as? NSButton else {
                throw ProbeFailure(description: "\(id) is not a button")
            }
            try require(button.isEnabled, "\(id) is disabled")
            button.performClick(nil)
        }
    },
    "wait_enabled": ActionSpec(.dialog, ["id", "seconds"]) { arguments in
        let id = try arguments.string("id")
        let seconds = try arguments.number("seconds", in: 0.01...120)
        return { context in
            let root = try context.dialog()
            guard let control = try dialogControl(root, id) as? NSControl else {
                throw ProbeFailure(description: "\(id) is not a control")
            }
            do {
                try pumpUntil({ control.isEnabled }, seconds: seconds)
            } catch {
                throw ProbeFailure(description: "\(id) was still disabled after \(seconds) s")
            }
        }
    },
    "set_curve_points": ActionSpec(.dialog, ["points"]) { arguments in
        let pairs = try arguments.pairs("points")
        let xs = pairs.map(\.x)
        guard (2...CurveEditorView.maxPoints).contains(pairs.count), xs == xs.sorted(), Set(xs).count == xs.count,
              pairs.allSatisfy({ (0...255).contains($0.x) && (0...255).contains($0.y) }) else {
            throw arguments.fail("points are 2 to \(CurveEditorView.maxPoints) [x, y] in [0, 255], x rising")
        }
        let points = pairs.map { CurvePoint(x: Double($0.x), y: Double($0.y)) }
        return { context in
            let curves = descendants(try context.dialog()).compactMap { $0 as? CurveEditorView }
            guard curves.count == 1 else {
                throw ProbeFailure(description: "\(curves.count) curve editors in the dialog")
            }
            // What a drag ends with (scripts/jobs-probe/ui_curves.swift).
            curves[0].points = points
            curves[0].onPointsChanged?(points)
        }
    },
    "set_view_chrome": ActionSpec(.editor, ["option", "value"]) { arguments in
        let name = try arguments.string("option")
        guard let option = viewChromeOptions[name] else {
            throw arguments.fail("no view option \"\(name)\"; the options are \(viewChromeOptions.keys.sorted())")
        }
        let on = try flag(arguments, "value")
        return { context in try setViewChrome(option, on, context) }
    },
    "set_ruler_line": ActionSpec(.editor, ["from", "to"]) { arguments in
        let from = try arguments.point("from"), to = try arguments.point("to")
        guard from != to else { throw arguments.fail("the line has no length") }
        return { try setRulerLine(from: from, to: to, try $0.requireEditor()) }
    },
    "set_paint_target": ActionSpec(.editor, ["target"]) { arguments in
        let name = try arguments.string("target")
        let targets: [String: PaintTarget] = ["layer": .layer, "mask": .mask]
        guard let target = targets[name] else { throw arguments.fail("the target is layer or mask, not \"\(name)\"") }
        return { context in
            let editor = try context.requireEditor()
            // The one writer of the target: the canvas, both panels' rings and the status bar follow.
            editor.setPaintTarget(target)
            try require(editor.paintTarget == target,
                        "the editor kept the target \(editor.paintTarget) (has the active layer a mask?)")
        }
    },
    "set_channel_visibility": ActionSpec(.editor, ["rgb", "mask"]) { arguments in
        let rgb = try flag(arguments, "rgb"), mask = try flag(arguments, "mask")
        return { context in
            let editor = try context.requireEditor()
            var visibility = try context.requireDocument().channelVisibility
            visibility.rgb = rgb
            visibility.mask = mask
            // The Channels panel's eye column, its document state and the canvas wash.
            editor.restoreChannelVisibility(visibility)
        }
    },
    "open_command_palette": ActionSpec(.editor, ["query"]) { arguments in
        let query = try arguments.string("query")
        return { try openCommandPalette(query, $0) }
    },
    "set_crop_straighten": ActionSpec(.editor, ["degrees"]) { arguments in
        let degrees = try arguments.number("degrees", in: -45...45)
        return { context in
            let editor = try context.requireEditor()
            try require(editor.cropSession != nil, "no crop session (select_tool crop first)")
            // The options bar's Straighten field: the box turns and refits inside the picture.
            editor.cropStraightenDegrees = degrees
            try require(editor.cropStraightenDegrees == degrees, "the crop kept \(editor.cropStraightenDegrees)°")
        }
    },
    "seed_recovery_copy": ActionSpec(.editor, ["title"]) { arguments in
        let title = try arguments.string("title")
        return { try seedRecoveryCopy(title, try $0.requireDocument()) }
    },
    "set_layers_filter": ActionSpec(.editor, ["label"]) { arguments in
        let name = try arguments.string("label")
        guard let label = LayerLabel(name: name) else {
            throw arguments.fail("no label \"\(name)\"; the labels are \(LayerLabel.allCases.map(\.name))")
        }
        return { try setLayersFilter(label: label, try $0.requireEditor()) }
    },
    "open_layer_style": ActionSpec(.editor, ["effect"]) { arguments in
        let name = try arguments.string("effect")
        let initial: LayerStyleEffectKind?
        if name == "blending_options" {
            initial = nil
        } else if let kind = LayerStyleEffectKind(rawValue: name) {
            initial = kind
        } else {
            throw arguments.fail("no effect \"\(name)\"; the effects are blending_options and "
                                 + "\(LayerStyleEffectKind.allCases.map(\.rawValue))")
        }
        // Layer ▸ Layer Style ▸ Blending Options… or the effect's own item, on the active layer.
        return { context in
            try openSheet("layer_style", { editor in
                guard let document = editor.document else { return }
                editor.editLayerStyle(document.activeLayerIndex, initial: initial)
            }, context)
        }
    },
    "open_psd": ActionSpec(.editor, ["name"]) { arguments in
        let name = try arguments.string("name")
        guard name.hasSuffix(".psd"), !name.contains("/"), !name.hasPrefix(".") else {
            throw arguments.fail("\"\(name)\" is not a plain file name ending .psd")
        }
        return { try openPSD(name, $0) }
    },
    // The selection, channel, path and transform actions; their helpers are in
    // selection_actions.swift.
    "color_range_sample": ActionSpec(.dialog, ["point"]) { arguments in
        let point = try arguments.point("point")
        return { try colorRangeSample(point, $0) }
    },
    "set_current_path": ActionSpec(.editor, ["name"]) { arguments in
        let name = try arguments.string("name")
        return { try setCurrentPath(named: name, try $0.requireEditor()) }
    },
    "set_channel_target": ActionSpec(.editor, ["name"]) { arguments in
        let name = try arguments.string("name")
        return { try setChannelTarget(named: name, try $0.requireEditor()) }
    },
    "set_alpha_channel_visibility": ActionSpec(.editor, ["rgb", "channels"]) { arguments in
        let rgb = try flag(arguments, "rgb")
        guard let names = arguments.values["channels"] as? [String], !names.isEmpty,
              names.allSatisfy({ !$0.isEmpty }), Set(names).count == names.count else {
            throw arguments.fail("\"channels\" is not a list of distinct channel names")
        }
        return { context in try setAlphaChannelVisibility(rgb: rgb, names, context) }
    },
    "free_transform": ActionSpec(.editor, []) { _ in
        return { try freeTransform(try $0.requireEditor()) }
    },
    "set_transform": ActionSpec(.editor, ["angle", "scale_x", "scale_y"]) { arguments in
        let angle = try arguments.number("angle", in: -180...180)
        let scaleX = try arguments.number("scale_x", in: 0.01...10)
        let scaleY = try arguments.number("scale_y", in: 0.01...10)
        return { try setTransform(angle: angle, scaleX: scaleX, scaleY: scaleY, try $0.requireEditor()) }
    },
    "puppet_warp": ActionSpec(.editor, []) { _ in
        return { try openPuppetWarp(try $0.requireEditor()) }
    },
    "puppet_pin": ActionSpec(.editor, ["from", "to"]) { arguments in
        let from = try arguments.point("from"), to = try arguments.point("to")
        return { try puppetPin(from: from, to: to, try $0.requireEditor()) }
    },
    // The filter workspaces' actions; their helpers are in filter_actions.swift.
    "liquify_stroke": ActionSpec(.dialog, ["from", "to"]) { arguments in
        let from = try arguments.point("from"), to = try arguments.point("to")
        guard from != to else { throw arguments.fail("the stroke has no length") }
        return { try liquifyStroke(from: from, to: to, $0) }
    },
    "liquify_actual_size": ActionSpec(.dialog, ["center"]) { arguments in
        let center = try arguments.point("center")
        return { try liquifyActualSize(center: center, $0) }
    },
    // The Develop RAW card's actions; their helpers are in develop_actions.swift.
    "develop_wait_ready": ActionSpec(.card, []) { _ in
        return { try developWaitReady($0) }
    },
    "develop_scroll": ActionSpec(.card, ["row"]) { arguments in
        let row = try arguments.string("row")
        return { try developScroll(row, $0) }
    },
    // The paint, retouch, text and shape actions; their helpers are in retouch_actions.swift.
    "set_tool_options": ActionSpec(.editor, ["options", "values"]) { arguments in
        let name = try arguments.string("options")
        guard let group = toolOptionsGroups[name] else {
            throw arguments.fail("no options \"\(name)\"; the options are \(toolOptionsGroups.keys.sorted())")
        }
        guard let values = arguments.values["values"] as? [String: Any], !values.isEmpty else {
            throw arguments.fail("\"values\" is not a non-empty object")
        }
        let unknown = Set(values.keys).subtracting(group.keys)
        guard unknown.isEmpty else {
            throw arguments.fail("\(name) has no \(unknown.sorted()); its keys are \(group.keys.sorted())")
        }
        return { try setToolOptions(name, group, values, $0) }
    },
    "hover": ActionSpec(.editor, ["point"]) { arguments in
        let point = try arguments.point("point")
        return { try hover(point, try $0.requireEditor()) }
    },
    "set_clone_source": ActionSpec(.editor, ["point"]) { arguments in
        let point = try arguments.point("point")
        return { try setCloneSource(point, try $0.requireEditor()) }
    },
    "hold_remove_stroke": ActionSpec(.editor, ["points", "size"]) { arguments in
        let points = try arguments.pairs("points")
        let size = try arguments.number("size", in: 1...5000)
        guard points.count >= 2 else { throw arguments.fail("a stroke has at least two points") }
        return { try holdRemoveStroke(points, size: size, try $0.requireEditor()) }
    },
    "open_overflow": ActionSpec(.editor, []) { _ in
        return { try openOverflow($0) }
    },
    "set_colors": ActionSpec(.editor, ["foreground", "background"]) { arguments in
        let colors = try ["foreground", "background"].map { key -> NSColor in
            let hex = try arguments.string(key)
            guard hex.hasPrefix("#"), [7, 9].contains(hex.count), let color = TextLayer.color(fromHex: hex) else {
                throw arguments.fail("\"\(key)\" is not #RRGGBB or #RRGGBBAA")
            }
            return color
        }
        return { try setColors(foreground: colors[0], background: colors[1], $0) }
    },
    "reopen_text": ActionSpec(.editor, ["layer"]) { arguments in
        let layer = Int(try arguments.number("layer", in: 0...999))
        return { try reopenText(layer: layer, $0) }
    },
    "reopen_shape": ActionSpec(.editor, ["layer"]) { arguments in
        let layer = Int(try arguments.number("layer", in: 0...999))
        return { try reopenShape(layer: layer, $0) }
    },
]

/// A JSON bool argument (`Arguments.scalar` reads it; a number or string is refused).
private func flag(_ arguments: Arguments, _ key: String) throws -> Bool {
    guard case .flag(let on) = try arguments.scalar(key) else { throw arguments.fail("\"\(key)\" is not a bool") }
    return on
}

// ---- view preferences

/// The View menu's app-wide switches `set_view_chrome` sets, by name.
let viewChromeOptions: [String: WritableKeyPath<ViewToolOptions, Bool>] = [
    "rulers": \.rulers,
    "guides": \.guides,
    "grid": \.showGrid,
    "show_clipping": \.showClipping,
    "color_samplers": \.colorSamplers,
]

/// Sets one view preference as its View menu item does (`toggleShowClipping`): the store, then the
/// chrome broadcast every editor answers. The preference is app-wide and persisted, so the
/// screen's teardown puts the whole family back.
func setViewChrome(_ option: WritableKeyPath<ViewToolOptions, Bool>, _ on: Bool, _ context: Context) throws {
    let editor = try context.requireEditor()
    let before = ToolOptionsStore.shared.view
    context.restorers.append { [weak editor] in
        ToolOptionsStore.shared.view = before
        editor?.viewChromeChanged()
    }
    ToolOptionsStore.shared.view[keyPath: option] = on
    editor.viewChromeChanged()
}

// ---- the editor

/// `set_ruler_line`: the line a Ruler drag leaves, in canvas pixels. The session's canvas size is
/// the canvas's own, or the canvas reads the line as stale.
func setRulerLine(from: CGPoint, to: CGPoint, _ editor: EditorViewController) throws {
    try require(editor.currentTool == .ruler, "the Ruler is not the current tool (select_tool ruler first)")
    let canvas = editor.canvas
    let bounds = CGRect(origin: .zero, size: canvas.bounds.size)
    try require(bounds.contains(from) && bounds.contains(to), "the line leaves the \(bounds.size) canvas")
    canvas.rulerSession = RulerSession(line: RulerMeasurement(start: from, end: to),
                                       canvasSize: canvas.bounds.size, drag: nil)
    try require(canvas.rulerMeasurement != nil, "the canvas reads no ruler line")
}

/// `set_layers_filter`: the Layers panel's label pop-up set to `label`, as a person picks it from
/// the filter bar. The pop-up is the panel's own (its first item is All Labels); its action is the
/// filter bar's one action, which narrows the rows and changes no document.
func setLayersFilter(label: LayerLabel, _ editor: EditorViewController) throws {
    let popups = descendants(editor.layersPanel.view).compactMap { $0 as? NSPopUpButton }
        .filter { $0.itemTitles.first == "All Labels" }
    guard popups.count == 1, let popup = popups.first, let action = popup.action else {
        throw ProbeFailure(description: "\(popups.count) label pop-ups in the Layers panel")
    }
    try require(popup.selectItem(withTag: Int(label.rawValue)), "the label pop-up has no \(label.name) item")
    popup.sendAction(action, to: popup.target)
}

/// `open_command_palette`: Tools ▸ Command Palette… over the editor, with `query` typed. The probe
/// has no app delegate, so it installs the main menu the app builds at launch. The probe is never
/// active and has no key window to resolve a nil-target item through, so each one is pointed at
/// the first object of the editor's responder chain that answers it, the object the chain would
/// have found (scripts/native-paint-probe/workspace.swift). The palette is a floating panel: it
/// is shown at the normal level, transparent, and ordered behind every other app's windows.
func openCommandPalette(_ query: String, _ context: Context) throws {
    let editor = try context.requireEditor()
    let document = try context.requireDocument()
    // Read before the build: the builder assigns the Window and Help menus as it builds, and the
    // restorer puts all three back.
    let previous = (main: NSApp.mainMenu, windows: NSApp.windowsMenu, help: NSApp.helpMenu)
    let delegate = AppDelegate()
    let menu = delegate.makeMainMenu()
    var chain: [NSResponder] = []
    var next: NSResponder? = editor.canvas
    while let responder = next {
        chain.append(responder)
        next = responder.nextResponder
    }
    let answerers: [NSObject] = chain + [document, NSApp, delegate, NSDocumentController.shared]
    retarget(menu, to: answerers)
    NSApp.mainMenu = menu
    context.restorers.append {
        withExtendedLifetime(delegate) {
            NSApp.mainMenu = previous.main
            NSApp.windowsMenu = previous.windows
            NSApp.helpMenu = previous.help
        }
    }
    let controller = CommandPaletteWindowController.shared
    guard let panel = controller.window else { throw ProbeFailure(description: "the palette was not built") }
    let level = panel.level
    context.restorers.append { panel.level = level }
    panel.hidesOnDeactivate = false
    panel.level = .normal
    try showBehind(panel) { controller.show() }
    guard let field = descendants(panel.contentView ?? NSView()).compactMap({ $0 as? NSTextField })
            .first(where: { $0.isEditable }) else {
        throw ProbeFailure(description: "the palette has no query field")
    }
    field.stringValue = query
    // Typed, not selected: the insertion point after the query.
    field.currentEditor()?.selectedRange = NSRange(location: (query as NSString).length, length: 0)
    controller.controlTextDidChange(Notification(name: NSControl.textDidChangeNotification, object: field))
    place(panel)
    context.shown.append(panel)
    settle(panel, seconds: 0.5)
    try require(panel.isVisible, "the palette closed (it orders out when it resigns key)")
}

/// Points every nil-target item of `menu` at the first of `answerers` that responds to its action.
private func retarget(_ menu: NSMenu, to answerers: [NSObject]) {
    for item in menu.items {
        if let submenu = item.submenu { retarget(submenu, to: answerers) }
        guard item.target == nil, let action = item.action else { continue }
        item.target = answerers.first { $0.responds(to: action) }
    }
}

func setCropBox(_ rect: CGRect, _ editor: EditorViewController) throws {
    try require(editor.cropSession != nil, "no crop session (select_tool crop first)")
    editor.cropSession?.rect = rect
    editor.cropSession?.requested = rect
    editor.canvas.cropOverlay = CropOverlay(rect: rect, angle: 0,
                                            showsThirds: ToolOptionsStore.shared.crop.gridIndex == 1)
    editor.optionsBar.refreshValues()
}

/// `show_window`: opens the panel, keeps it visible while the probe is inactive, places it on the
/// 2x screen behind every other app's windows, and orders it out at teardown.
func showPanel(_ name: String, show: (Context) throws -> Void, find: (Context) -> NSWindow?,
               _ context: Context) throws {
    try show(context)
    guard let window = find(context) else { throw ProbeFailure(description: "show_window \(name): no window") }
    window.hidesOnDeactivate = false
    place(window)
    if !window.isVisible { window.orderBack(nil) }
    context.shown.append(window)
    settle(window, seconds: 0.5)
}

// ---- dialogs

func buttons(_ view: NSView) -> [NSButton] { descendants(view).compactMap { $0 as? NSButton } }

/// The first view under `root` whose identifier or accessibility identifier is `id`, as `T`.
func control<T: NSView>(_ root: NSView, id: String, as type: T.Type) -> T? {
    descendants(root).filter {
        $0.accessibilityIdentifier() == id || $0.identifier?.rawValue == id
    }.compactMap { $0 as? T }.first
}

/// The Cancel of a sheet or card: the button whose action is `cancelClicked:` or `cancel:`.
func cancelButton(_ root: NSView) throws -> NSButton {
    let actions: Set<String> = ["cancelClicked:", "cancel:"]
    guard let button = buttons(root).first(where: {
        $0.action.map(NSStringFromSelector).map(actions.contains) == true
    }) else { throw ProbeFailure(description: "no Cancel button") }
    return button
}

/// The control named `name` in a dialog, among the views a person can see: the one control
/// whose identifier or accessibility identifier is `name` (or the control of the one card row
/// so identified, "raw-develop-texture"), else the control of the one card row labelled `name`
/// ("Exposure:"), else the first control of the one grid row labelled `name` (a filter sheet's
/// "Blur:" slider).
func dialogControl(_ root: NSView, _ name: String) throws -> NSView {
    let all = descendants(root)
    let named = all.filter { $0.accessibilityIdentifier() == name || $0.identifier?.rawValue == name }
    let shown = named.filter { !$0.isHiddenOrHasHiddenAncestor }
    try require(named.isEmpty || !shown.isEmpty, "\(name) is hidden")
    let controls = shown.filter { $0 is NSControl }
    if controls.count == 1 { return controls[0] }
    if controls.isEmpty, shown.count == 1, let row = shown[0] as? CardControlRow,
       let found = row.slider ?? row.checkbox ?? row.popUp {
        return found
    }
    if controls.isEmpty, shown.count == 1 { return shown[0] }
    try require(shown.isEmpty, "\(shown.count) views are named \(name)")
    let rows = all.compactMap { $0 as? CardControlRow }.filter {
        !$0.isHiddenOrHasHiddenAncestor && ($0.arrangedSubviews.first as? NSTextField)?.stringValue == name
    }
    if rows.count == 1, let found = rows[0].slider ?? rows[0].checkbox ?? rows[0].popUp { return found }
    try require(rows.isEmpty, "\(rows.count) card rows are labelled \(name)")
    let gridRows = all.compactMap { $0 as? NSGridView }.filter { !$0.isHiddenOrHasHiddenAncestor }
        .flatMap { grid in (0..<grid.numberOfRows).map(grid.row(at:)) }
        .filter { !$0.isHidden && $0.numberOfCells > 1
                  && ($0.cell(at: 0).contentView as? NSTextField)?.stringValue == name }
    guard gridRows.count == 1, let found = (1..<gridRows[0].numberOfCells)
            .compactMap({ gridRows[0].cell(at: $0).contentView as? NSControl })
            .first(where: { !($0 is NSTextField) || ($0 as? NSTextField)?.isEditable == true }) else {
        throw ProbeFailure(description: "no shown control named \(name) in the dialog")
    }
    return found
}

/// Gives `view` the value by its type, as the person would, then sends its action. A text field
/// also posts the change and end-editing notices, which delegate-read fields save on.
func setControl(_ view: NSView, to value: ControlValue, _ name: String) throws {
    guard let control = view as? NSControl else { throw ProbeFailure(description: "\(name) is not a control") }
    try require(control.isEnabled, "\(name) is disabled")
    switch (control, value) {
    case (let popup as NSPopUpButton, .number(let index)):
        try require(index == index.rounded() && Int(index) >= 0 && Int(index) < popup.numberOfItems,
                    "\(name) has no item \(index)")
        popup.selectItem(at: Int(index))
    case (let popup as NSPopUpButton, .text(let title)):
        try require(popup.item(withTitle: title) != nil,
                    "\(name) has no item \"\(title)\"; it has \(popup.itemTitles)")
        popup.selectItem(withTitle: title)
    case (let segments as NSSegmentedControl, .number(let index)):
        try require(index == index.rounded() && Int(index) >= 0 && Int(index) < segments.segmentCount,
                    "\(name) has no segment \(index)")
        segments.selectedSegment = Int(index)
    case (let segments as NSSegmentedControl, .text(let label)):
        guard let index = (0..<segments.segmentCount).first(where: { segments.label(forSegment: $0) == label }) else {
            throw ProbeFailure(description: "\(name) has no segment \"\(label)\"")
        }
        segments.selectedSegment = index
    case (let button as NSButton, .flag(let on)):
        button.state = on ? .on : .off
    case (let slider as NSSlider, .number(let number)):
        try require((slider.minValue...slider.maxValue).contains(number),
                    "\(name) runs \(slider.minValue)…\(slider.maxValue), not \(number)")
        slider.doubleValue = number
    case (let field as NSTextField, .text(let text)):
        enter(text, in: field)
    case (let field as NSTextField, .number(let number)):
        enter(number == number.rounded() ? String(Int(number)) : String(number), in: field)
    default:
        throw ProbeFailure(description: "\(name) (\(type(of: control))) does not take \(value)")
    }
    if let action = control.action { control.sendAction(action, to: control.target) }
}

private func enter(_ text: String, in field: NSTextField) {
    field.stringValue = text
    NotificationCenter.default.post(name: NSControl.textDidChangeNotification, object: field)
    NotificationCenter.default.post(name: NSControl.textDidEndEditingNotification, object: field)
}

/// Chooses `title` in the one pop-up of the dialog that holds an item with that title.
func choosePopupItem(_ title: String, in root: NSView) throws {
    let popups = descendants(root).compactMap { $0 as? NSPopUpButton }.filter {
        !$0.isHiddenOrHasHiddenAncestor && $0.item(withTitle: title) != nil
    }
    guard popups.count == 1 else {
        throw ProbeFailure(description: "\(popups.count) pop-ups hold \"\(title)\"; name the control with set_control")
    }
    try setControl(popups[0], to: .text(title), "the pop-up holding \"\(title)\"")
}

// ---- documents

/// `seed_recovery_copy`: the copy a crash leaves behind, of the screen's document under `title`. It
/// is written as the coordinator's checkpoint writes one (`RecoveryCoordinator.begin`), into the
/// run's private recovery folder, under a lease released at once, so Recover Unsaved Work lists it
/// as abandoned. The probe never starts the coordinator: an open document's own copy stays leased
/// and unlisted while the document is open, and closing it deletes the copy.
func seedRecoveryCopy(_ title: String, _ document: ImageDocument) throws {
    let root = RecoveryCoordinator.shared.root
    guard let scratch = ProcessInfo.processInfo.environment["DR_RECOVERY_DIR"], !scratch.isEmpty,
          root.standardizedFileURL.path == URL(fileURLWithPath: scratch).standardizedFileURL.path else {
        throw ProbeFailure(description: "DR_RECOVERY_DIR is not the recovery root (run.py sets a private one)")
    }
    guard let state = document.captureEditingState() else {
        throw ProbeFailure(description: "the document has no editing state to recover")
    }
    let id = UUID().uuidString
    let directory = try RecoveryStore.directory(root: root, id: id)
    guard let lease = try RecoveryLease.acquire(directory: directory, create: true) else {
        throw ProbeFailure(description: "the new recovery folder is already leased")
    }
    defer { lease.release() }
    let captured = RecoveryStore.Captured(
        id: id, title: title, sourceURL: nil, sourceType: document.fileType,
        generation: RecoveryStore.Generation(folder: UUID().uuidString, revision: document.revision,
                                             serial: 1, captured: Date()),
        previous: nil, state: try EditingStateArchive.capture(state),
        snapshots: try document.namedSnapshots.captureArchive())
    let manifest = try RecoveryStore.write(captured, directory: directory, gate: RecoveryPublicationGate())
    try require(manifest != nil, "the recovery copy was not published")
}

/// `open_psd`: opening a PSD that the screen's document exports as `name` into the run's private
/// home, as File ▸ Open reads one (`ImageDocument.read` asks how to open it). The screen's alert
/// capture takes the compatibility notice and answers Cancel, so nothing opens.
func openPSD(_ name: String, _ context: Context) throws {
    let document = try context.requireDocument()
    guard let home = ProcessInfo.processInfo.environment["CFFIXED_USER_HOME"], !home.isEmpty else {
        throw ProbeFailure(description: "CFFIXED_USER_HOME is not set (run.py sets a private home)")
    }
    let folder = URL(fileURLWithPath: home, isDirectory: true)
        .appendingPathComponent("psd-\(UUID().uuidString)", isDirectory: true)
    try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
    let url = folder.appendingPathComponent(name)
    try callTool("export_psd", ["path": url.path], document)
    let mode = PSDImportRequest.mode
    PSDImportRequest.mode = .ask
    defer { PSDImportRequest.mode = mode }
    do {
        _ = try ImageDocument.openPSD(at: url)
    } catch let error as CocoaError where error.code == .userCancelled {
        return
    }
    throw ProbeFailure(description: "the notice was not answered Cancel (the screen needs an alert capture)")
}
