import AppKit

/// The controller half of the Paths panel (PathsPanelViewController.swift): the tab, the
/// document's current path, and the commands the panel's footer and row menu, Select ▸ Make Work
/// Path… and the menu bar send. Each command has an MCP twin in `AgentServer+Paths.swift` and
/// records it:
///
/// | Command | Records |
/// |---|---|
/// | rename (double-click) | `rename_path` |
/// | Duplicate Path | `duplicate_path` |
/// | Delete Path | `delete_path` |
/// | Make Selection… | `path_to_selection` |
/// | Fill Path | `fill_path` |
/// | Stroke Path | `stroke_path` |
/// | Make Work Path… | `selection_to_path` |
///
/// The three list edits are `applyPathEdit` (render-neutral, one undo step each). The four that
/// run a commit of their own — a selection, a fill, a brush stroke, a trace — RUN THEIR MCP STEP
/// through `ActionPlayer.runSingle`, the way Filters ▸ Repeat Last does, so the panel and an
/// agent's call are one code path; the step is recorded afterwards, because the player suspends
/// the recorder for the length of every replay. A menu command ASKS before it rasterizes a text
/// or shape layer, so Fill Path and Stroke Path prompt here first (`confirmRasterize`); the
/// agent's dispatch never would.
extension EditorViewController {
    // MARK: - The panel

    /// Built in `loadView` beside the other five panels.
    func makePathsPanel() -> PathsPanelViewController {
        let panel = PathsPanelViewController()
        panel.document = document
        panel.onShowLayers = { [weak self] in self?.showLayersTab() }
        panel.onShowChannels = { [weak self] in self?.showChannelsTab() }
        panel.onShowAssistant = { [weak self] in self?.showAssistant(nil) }
        panel.onShowInfo = { [weak self] in self?.showInfoTab() }
        panel.onShowNavigator = { [weak self] in self?.showNavigatorTab() }
        panel.onSelectTarget = { [weak self] in self?.pathsPanelDidSelect($0) }
        panel.onRenamePath = { [weak self] in self?.renamePath(id: $0, to: $1) }
        addChild(panel)
        panel.view.translatesAutoresizingMaskIntoConstraints = false
        panel.view.isHidden = true
        return panel
    }

    /// Puts the panel's view in `root`, over the frame the Navigator panel
    /// has (every right panel shares one). Called once from `loadView`, after
    /// the other panels are in.
    func installPathsPanel(in root: NSView) {
        guard let paths = pathsPanel?.view, let frame = navigatorPanel?.view else { return }
        root.addSubview(paths, positioned: .above, relativeTo: frame)
        NSLayoutConstraint.activate([
            paths.leadingAnchor.constraint(equalTo: frame.leadingAnchor),
            paths.trailingAnchor.constraint(equalTo: frame.trailingAnchor),
            paths.topAnchor.constraint(equalTo: frame.topAnchor),
            paths.bottomAnchor.constraint(equalTo: frame.bottomAnchor),
        ])
    }

    private func showLayersTab() {
        panelTab = EditorPanelTab.layers.rawValue
        updatePanelVisibility()
    }

    /// The six panels' Paths tab.
    func showPathsTab() {
        panelTab = EditorPanelTab.paths.rawValue
        updatePanelVisibility()
        pathsPanel?.reload()
    }

    // MARK: - The current path

    /// A row was selected in the panel, or the selection was cleared: view
    /// state, no undo step. The canvas outlines the new current path and the
    /// Pen edits it.
    private func pathsPanelDidSelect(_ target: PathTarget?) {
        document?.currentPath = target
        refreshCanvasPath()
    }

    /// Makes `target` the current path from a command (a new work path, a
    /// duplicate, a new vector mask), and brings the canvas and the panel
    /// along.
    func setCurrentPath(_ target: PathTarget?) {
        document?.currentPath = target
        refreshCanvasPath()
        pathsPanel?.activeLayerChanged()
    }

    /// The active layer changed (the Layers panel, `setActiveLayer` /
    /// `setSelectedLayers`): a current path on the old layer's vector mask is
    /// dropped, and the canvas and the panel follow.
    func pathsActiveLayerDidChange() {
        refreshCanvasPath()
        pathsPanel?.activeLayerChanged()
    }

    /// The vector mask row the Paths panel shows is always the ACTIVE layer's,
    /// so a current path on any other layer's vector mask is cleared (not
    /// retargeted). True when it was; a list path is never touched.
    @discardableResult
    func followActiveLayerVectorMask() -> Bool {
        guard let document, case .vectorMask(let layerID)? = document.currentPath,
              layerID != activeLayerID()
        else { return false }
        document.currentPath = nil
        return true
    }

    /// The active layer's stable id, or nil when there is none.
    private func activeLayerID() -> UInt64? {
        guard let document else { return nil }
        return document.doc?.layerInfo(document.activeLayerIndex)?.id
    }

    /// The current path as a shape and as the arguments that name it for a
    /// path tool: `path` for a path of the list (the work path answers to
    /// "Work Path"), the shape itself for a layer's vector mask, which no
    /// tool can name. nil with no current path.
    func currentPathStep() -> (shape: PathShape, arguments: [String: Any])? {
        guard let document, let doc = document.doc, let target = document.currentPath else {
            return nil
        }
        switch target {
        case .path(let id):
            guard let entry = doc.paths.entry(id: id) else { return nil }
            return (entry.shape, ["path": entry.name])
        case .vectorMask(let layerID):
            // Another layer's vector mask is stale (MCP `new_layer` moves the
            // active layer after its edit's notification). Read-only here:
            // menu validation asks this, so the clearing is left to
            // `followActiveLayerVectorMask`.
            guard layerID == activeLayerID(), let shape = document.resolvedPath(target) else {
                return nil
            }
            return (
                shape, ["subpaths": shape.subpathsJSON, "fill_rule": shape.fillRule.rawValue]
            )
        }
    }

    /// The current path when it is one of the list's, with its index there.
    private func currentListPath() -> (set: PathSet, index: Int)? {
        guard let document, let doc = document.doc, case .path(let id)? = document.currentPath
        else { return nil }
        let set = doc.paths
        guard let index = set.entries.firstIndex(where: { $0.id == id }) else { return nil }
        return (set, index)
    }

    // MARK: - Running a path tool as a menu command

    /// Runs one path tool step as the command the user just gave: through
    /// the dispatch an agent's call takes, as ONE undo entry named
    /// `undoName`, then records it. A refusal is shown in the tool's own
    /// words and records nothing. True when the step ran.
    @discardableResult
    private func runPathStep(
        _ tool: String, _ arguments: [String: Any], undoName: String, note: String
    ) -> Bool {
        guard let document else {
            NSSound.beep()
            return false
        }
        let step = ActionStep(tool: tool, arguments: arguments, note: note)
        let report = ActionPlayer.runSingle(step, on: document, undoName: undoName)
        guard report.ok else {
            let reason = report.failures.first?.reason ?? "the command could not run"
            let alert = NSAlert()
            alert.messageText = "\(undoName) could not run"
            alert.informativeText = reason.prefix(1).uppercased() + reason.dropFirst()
            if let window = view.window {
                alert.beginSheetModal(for: window)
            } else {
                alert.runModal()
            }
            return false
        }
        ActionRecorder.shared.record([step], on: document)
        return true
    }

    /// `color` as a paint tool's colour arguments: the authored sRGB hex, or
    /// on a wide-gamut document the document's own bytes with
    /// `color_space: "document"` (`ActionArgs.documentHex`), as a recorded
    /// brush stroke writes them.
    private func colorArguments(_ color: NSColor) -> [String: Any] {
        if let space = document?.nsColorSpace, let hex = ActionArgs.documentHex(color, in: space) {
            return ["color": hex, "color_space": "document"]
        }
        return ["color": ActionArgs.hex(color)]
    }

    // MARK: - List edits

    /// A row's inline rename. Renaming the work path saves it.
    private func renamePath(id: UInt64, to name: String) {
        guard let document, let set = document.doc?.paths,
              let index = set.entries.firstIndex(where: { $0.id == id })
        else {
            NSSound.beep()
            return
        }
        let old = set.entries[index]
        var next = set
        next.entries[index].name = name
        next.entries[index].kind = .saved
        document.applyPathEdit(
            old.kind == .work ? "Save Path" : "Rename Path",
            record: .step(
                "rename_path", ["path": old.name, "name": name],
                note: "Paths panel: rename a path")
        ) { _ in next }
        // A refused name (taken, empty) leaves the row showing what was typed.
        pathsPanel?.reload()
    }

    /// Paths panel ▸ Duplicate Path: a saved copy named "<name> copy",
    /// numbered until the name is free, which becomes the current path.
    @objc func duplicateCurrentPath(_ sender: Any?) {
        guard let document, let (set, index) = currentListPath() else {
            NSSound.beep()
            return
        }
        let source = set.entries[index]
        var name = "\(source.name) copy"
        var number = 2
        while set.entry(named: name) != nil {
            name = "\(source.name) copy \(number)"
            number += 1
        }
        var next = set
        next.entries.append(PathEntry(id: 0, name: name, kind: .saved, shape: source.shape))
        document.applyPathEdit(
            "Duplicate Path",
            record: .step(
                "duplicate_path", ["path": source.name, "name": name],
                note: "Paths panel ▸ Duplicate Path")
        ) { _ in next }
        if let copy = document.doc?.paths.entry(named: name) { setCurrentPath(.path(copy.id)) }
    }

    /// Paths panel ▸ Delete: the current path leaves the list — or, when the
    /// current path is a layer's vector mask, that mask is deleted.
    @objc func deleteCurrentPath(_ sender: Any?) {
        guard let document else {
            NSSound.beep()
            return
        }
        if case .vectorMask? = document.currentPath {
            deleteVectorMask(sender)
            return
        }
        guard let (set, index) = currentListPath() else {
            NSSound.beep()
            return
        }
        var next = set
        let removed = next.entries.remove(at: index)
        document.applyPathEdit(
            "Delete Path",
            record: .step(
                "delete_path", ["path": removed.name], note: "Paths panel ▸ Delete Path")
        ) { _ in next }
        setCurrentPath(nil)
    }

    // MARK: - Selection

    /// Paths panel ▸ Make Selection…: the dialog, then `path_to_selection`.
    @objc func makeSelectionFromPath(_ sender: Any?) {
        guard let document, let current = currentPathStep(), !current.shape.isEmpty,
              !document.quickMaskActive
        else {
            NSSound.beep()
            return
        }
        let sheet = PathSelectionSheetController(hasSelection: canvas.selection != nil) {
            [weak self] feather, antiAlias, mode in
            // The path is read again at commit: the sheet does not stop the
            // run loop, and an agent may have edited or deleted it meanwhile.
            guard let self, let current = self.currentPathStep(), !current.shape.isEmpty else {
                NSSound.beep()
                return
            }
            var arguments = current.arguments
            arguments["mode"] = mode
            arguments["feather"] = ActionArgs.number(feather)
            arguments["anti_alias"] = antiAlias
            self.runPathStep(
                "path_to_selection", arguments, undoName: "Make Selection",
                note: "Paths panel ▸ Make Selection…")
        }
        presentAsSheet(sheet)
    }

    /// Select ▸ Make Work Path…, and the panel's own item: the dialog, then
    /// `selection_to_path`. The traced work path becomes the current path.
    @objc func makeWorkPath(_ sender: Any?) {
        guard let document, let doc = document.doc, canvas.selection != nil,
              !document.quickMaskActive
        else {
            NSSound.beep()
            return
        }
        let sheet = MakeWorkPathSheetController(replacesWorkPath: doc.paths.workPath != nil) {
            [weak self] tolerance in
            guard let self else { return }
            let ran = self.runPathStep(
                "selection_to_path", ["tolerance": ActionArgs.number(tolerance)],
                undoName: "Make Work Path", note: "Select ▸ Make Work Path…")
            if ran, let work = self.document?.doc?.paths.workPath {
                self.setCurrentPath(.path(work.id))
            }
        }
        presentAsSheet(sheet)
    }

    // MARK: - Fill and stroke

    /// Whether the edit target takes a fill or a stroke at all, asked the way
    /// every paint command asks, with its own alert. A LAYER target also asks
    /// before rasterizing a text or shape layer.
    private func admitsPathPaint(_ document: ImageDocument) -> Bool {
        guard !refuseOperationScope(.fill) else { return false }
        if paintTarget == .layer || paintTarget.isPlane || paintTarget.isInk {
            guard !refuseAdjustmentPixelEdit(), !refuseGroupPixelEdit() else { return false }
            guard document.confirmRasterize(layer: document.activeLayerIndex) else { return false }
        }
        return true
    }

    /// Paths panel ▸ Fill Path: the current path filled with the foreground
    /// colour on the edit target, inside the selection when there is one.
    @objc func fillCurrentPath(_ sender: Any?) {
        guard let document, let current = currentPathStep(), !current.shape.isEmpty else {
            NSSound.beep()
            return
        }
        guard admitsPathPaint(document) else { return }
        var arguments = current.arguments
        arguments["layer"] = ActionArgs.activeLayer
        arguments["target"] = paintTarget.agentName(in: document.doc)
        arguments.merge(colorArguments(paintColor)) { _, new in new }
        guard cmykPathInk(&arguments, document: document) else { return }
        runPathStep("fill_path", arguments, undoName: "Fill Path", note: "Paths panel ▸ Fill Path")
    }

    /// A CMYK layer target paints the foreground swatch's stored ink (#547), as Edit ▸ Fill…
    /// does: `ink` (C, M, Y, K percent) replaces `color`. A plate or channel target keeps `color`,
    /// whose grey it paints. False (after an alert) when the ink cannot be read.
    private func cmykPathInk(_ arguments: inout [String: Any], document: ImageDocument) -> Bool {
        guard let precision = document.doc?.precision, precision.isCmyk, paintTarget == .layer else {
            return true
        }
        do {
            let ink = try CmykPaint.ink(paintColor, stored: ToolOptionsStore.shared.sharedState.foregroundInk,
                                        in: precision)
            for key in ["color", "color_space"] { arguments.removeValue(forKey: key) }
            arguments["ink"] = ink.prefix(4).map { $0 * 100 }
            return true
        } catch {
            document.presentError(error)
            return false
        }
    }

    /// Paths panel ▸ Stroke Path: the Brush run along the current path with
    /// the Brush tool's STORED options and the foreground colour. On a 32-bit
    /// float layer a foreground above white is the Brush's linear ink
    /// (`rgba32`), as the Brush tool itself paints it.
    @objc func strokeCurrentPath(_ sender: Any?) {
        guard let document, let doc = document.doc,
              let current = currentPathStep(), !current.shape.isEmpty
        else {
            NSSound.beep()
            return
        }
        guard admitsPathPaint(document) else { return }
        let brush = ToolOptionsStore.shared.brush
        var arguments = current.arguments
        arguments["tool"] = "brush"
        arguments["layer"] = ActionArgs.activeLayer
        arguments["target"] = paintTarget.agentName(in: doc)
        arguments["size"] = ActionArgs.number(brush.size)
        arguments["hardness"] = ActionArgs.number(brush.hardness)
        arguments["flow"] = ActionArgs.number(max(brush.flow, 1))
        arguments["spacing"] = ActionArgs.number(brush.spacing)
        arguments["angle"] = ActionArgs.number(brush.angle)
        arguments["roundness"] = ActionArgs.number(max(brush.roundness, 1))
        arguments["opacity"] = ActionArgs.number(min(max(brush.opacity, 0), 100) / 100)
        if doc.isFloat, paintTarget == .layer,
           let linear = ExtendedRangeColor.aboveWhite(paintColor, in: doc) {
            // `rgba32` replaces `color` and takes no `color_space`.
            arguments["rgba32"] = linear
        } else {
            arguments.merge(colorArguments(paintColor)) { _, new in new }
        }
        guard cmykPathInk(&arguments, document: document) else { return }
        // A mask, a colour plane and a channel are coverage: `brush_stroke`
        // refuses a blend mode there, as a recorded stroke omits it.
        if !paintTarget.isCoverage {
            arguments["blend_mode"] = DrBlendMode.displayName(
                for: Self.blendMode(fromStored: brush.blendIndex))
        }
        runPathStep(
            "stroke_path", arguments, undoName: "Stroke Path", note: "Paths panel ▸ Stroke Path")
    }

    // MARK: - Validation

    /// Enablement for this file's items and `+VectorMasks.swift`'s; nil for
    /// anybody else's. One early-out in `validateUserInterfaceItem`.
    func validatePathsItem(_ item: NSValidatedUserInterfaceItem) -> Bool? {
        guard let document, let doc = document.doc else { return nil }
        let drawn = { [weak self] in self?.currentPathStep().map { !$0.shape.isEmpty } ?? false }
        switch item.action {
        case #selector(makeSelectionFromPath(_:)):
            return !document.quickMaskActive && drawn()
        case #selector(fillCurrentPath(_:)):
            return !document.quickMaskActive && drawn()
        case #selector(strokeCurrentPath(_:)):
            return !document.quickMaskActive && drawn()
        case #selector(makeWorkPath(_:)):
            return !document.quickMaskActive && canvas.selection != nil
        case #selector(duplicateCurrentPath(_:)):
            return currentListPath() != nil
        case #selector(deleteCurrentPath(_:)):
            return currentPathStep() != nil
        default:
            return validateVectorMaskItem(item)
        }
    }
}
