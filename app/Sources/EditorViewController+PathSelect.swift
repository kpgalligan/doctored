import AppKit

/// The controller half of Path Selection and Direct Selection (PathSelectSession.swift,
/// ImageCanvasView+PathSelect.swift), and the Paths panel's Fill Path… and Stroke Path… sheets.
///
/// A gesture's commit is the Pen's (`penGestureCommitted`): one `set_path` step — or, on a
/// layer's vector mask, one `set_vector_mask` step, and on a path shape's outline one
/// `edit_shape_layer` step — noted with the tool's name, so the gestures need no MCP tool of
/// their own. Combine Selected Subpaths (the options bar's four buttons and the Paths panel's row
/// menu) runs `combine_paths` with `subpath_indices` on a list path; a vector mask or an outline,
/// which no path tool can name, is combined here by the same rule
/// (`PathCombine.combineSubpaths`) and committed as the gestures are. Fill Path… and Stroke
/// Path… run `fill_path` and `stroke_path`; while subpaths are selected they paint those alone,
/// as Fill Path, Stroke Path and Make Selection… do (`currentPathStep(selectedOnly:)`).
extension EditorViewController {
    @objc func selectPathSelectTool(_ sender: Any?) { selectTool(.pathSelect) }
    @objc func selectDirectSelectTool(_ sender: Any?) { selectTool(.directSelect) }

    /// Called once from `loadView`.
    func installPathSelect() {
        canvas.onPathSelectCommit = { [weak self] in
            self?.pathSelectCommitted($0, name: $1) ?? false
        }
        canvas.onPathSelectionChange = { [weak self] in self?.pathSelectionChanged() }
    }

    /// A finished selection-tool gesture, committed as the Pen commits one.
    private func pathSelectCommitted(_ shape: PathShape, name: String) -> Bool {
        penGestureCommitted(shape, name: name, note: "\(canvas.tool.displayName) tool")
    }

    /// The selection changed: the options bar's Combine buttons follow it.
    private func pathSelectionChanged() {
        optionsBar.refreshValues()
    }

    /// The current path and the subpaths the selection tools have selected in it, ascending;
    /// nil unless one of the two is the tool, the selection was made on the current path as
    /// the document holds it now, and it names at least one subpath.
    func selectedSubpathsOfCurrentPath() -> (shape: PathShape, indices: [Int])? {
        guard canvas.tool.selectsPath, let document, let current = currentPathStep() else {
            return nil
        }
        let session = canvas.pathSelectSession
        let indices = session.selectedSubpaths
        guard !indices.isEmpty, session.target == document.currentPath,
              session.selectionBase == current.shape,
              indices.allSatisfy({ current.shape.subpaths.indices.contains($0) })
        else { return nil }
        return (current.shape, indices)
    }

    // MARK: - Combine Selected Subpaths

    /// The two tools' options bar: Combine's four operations on the selected subpaths.
    func pathSelectClusters() -> [OptionCluster] {
        let operations = PathCombineOperation.allCases
        return [
            OptionCluster(operations.enumerated().map { position, operation in
                OptionDescriptor(
                    id: "pathSelect.\(operation.rawValue)",
                    microLabel: position == 0 ? "Combine" : nil,
                    overflowLabel: "Combine: \(operation.title)",
                    kind: .button(title: operation.title, action: { [weak self] in
                        self?.commitCombineSelectedSubpaths(operation)
                    }),
                    isEnabled: { [weak self] in self?.canCombineSelectedSubpaths ?? false })
            }),
        ]
    }

    private var canCombineSelectedSubpaths: Bool {
        document?.quickMaskActive == false && selectedSubpathsOfCurrentPath() != nil
    }

    /// Paths panel ▸ Combine Selected Subpaths ▸ Unite / Subtract / Intersect / Exclude: the
    /// item's `tag` is the operation's index in `PathCombineOperation.allCases`.
    @objc func combineSelectedSubpaths(_ sender: Any?) {
        let operations = PathCombineOperation.allCases
        guard let tag = (sender as? NSValidatedUserInterfaceItem)?.tag,
              operations.indices.contains(tag)
        else {
            NSSound.beep()
            return
        }
        commitCombineSelectedSubpaths(operations[tag])
    }

    /// The selected subpaths against the rest of the current path (`PathCombine.combineSubpaths`),
    /// as ONE undo step; the selection is dropped, since the result's subpaths are new.
    func commitCombineSelectedSubpaths(_ operation: PathCombineOperation) {
        guard let document, !document.quickMaskActive,
              let selected = selectedSubpathsOfCurrentPath()
        else {
            NSSound.beep()
            return
        }
        let note = "\(canvas.tool.displayName) ▸ Combine Selected Subpaths"
        switch document.currentPath {
        case .path?:
            guard let (set, index) = currentListPath() else {
                NSSound.beep()
                return
            }
            let arguments: [String: Any] = [
                "path": set.entries[index].name, "subpath_indices": selected.indices,
                "operation": operation.rawValue,
            ]
            guard runPathStep(
                "combine_paths", arguments, undoName: "Combine Subpaths", note: note)
            else { return }
        case .vectorMask(let layerID)?:
            let result = PathCombine.combineSubpaths(
                selected.shape, selected: Set(selected.indices), operation)
            guard !result.isEmpty else {
                presentPathAlert(
                    "Combine Subpaths could not run",
                    "Nothing is left: the \(operation.rawValue) of the selected subpaths and the "
                        + "rest of the vector mask fills no area. Nothing changed.")
                return
            }
            guard document.commitPenVectorMask(
                result, layerID: layerID, name: "Combine Subpaths", note: note)
            else { return }
        case .shapeOutline(let layerID)?:
            // A path shape's outline, which no path tool can name either: the same rule,
            // committed as the gestures are (one `edit_shape_layer`).
            let result = PathCombine.combineSubpaths(
                selected.shape, selected: Set(selected.indices), operation)
            guard !result.isEmpty else {
                presentPathAlert(
                    "Combine Subpaths could not run",
                    "Nothing is left: the \(operation.rawValue) of the selected subpaths and the "
                        + "rest of the shape's outline fills no area. Nothing changed.")
                return
            }
            guard commitShapeOutlineGesture(
                result, layerID: layerID, name: "Combine Subpaths", note: note)
            else { return }
        case nil:
            NSSound.beep()
            return
        }
        canvas.pathSelectSession.clearSelection()
        refreshCanvasPath()
        pathsPanel?.reload()
    }

    private func presentPathAlert(_ title: String, _ text: String) {
        let alert = NSAlert()
        alert.messageText = title
        alert.informativeText = text
        if let window = view.window {
            alert.beginSheetModal(for: window)
        } else {
            alert.runModal()
        }
    }

    // MARK: - Fill Path… and Stroke Path…

    /// What a path sheet's hint says it paints.
    private func pathPaintSubject() -> String {
        guard let count = selectedSubpathsOfCurrentPath()?.indices.count else {
            return "the current path"
        }
        return count == 1 ? "the selected subpath" : "the \(count) selected subpaths"
    }

    /// Paths panel ▸ Fill Path…: Edit ▸ Fill…'s sheet (without Pattern), then `fill_path` with
    /// the colour, blend mode, opacity and Preserve Transparency it chose.
    @objc func fillCurrentPathWithOptions(_ sender: Any?) {
        guard let document, !document.quickMaskActive,
              let current = currentPathStep(selectedOnly: true), !current.shape.isEmpty
        else {
            NSSound.beep()
            return
        }
        let hint = "Fills \(pathPaintSubject()) — inside the selection when there is one — "
            + "with one colour, through the blend mode at the opacity. An open subpath is closed "
            + "by a straight line."
        let sheet = FillSheetController(
            coverage: paintTarget.isCoverage, quickMask: false,
            options: ToolOptionsStore.shared.fillDialog, pattern: false, title: "Fill path",
            hint: hint
        ) { [weak self] in self?.commitFillPathSheet($0) }
        presentAsSheet(sheet)
    }

    /// Fill Path…'s commit. The path is read again: the sheet does not stop the run loop, and
    /// an agent may have edited or deleted it meanwhile.
    func commitFillPathSheet(_ settings: FillSettings) {
        guard let document, let doc = document.doc,
              let current = currentPathStep(selectedOnly: true), !current.shape.isEmpty,
              let color = settings.contents.color(
                  foreground: paintColor, background: backgroundColor,
                  custom: settings.customColor, in: document.nsColorSpace)
        else {
            NSSound.beep()
            return
        }
        guard admitsPathPaint(document) else { return }
        var arguments = current.arguments
        arguments["layer"] = ActionArgs.activeLayer
        arguments["target"] = paintTarget.agentName(in: doc)
        arguments.merge(colorArguments(color)) { _, new in new }
        arguments["blend_mode"] = DrBlendMode.displayName(for: settings.mode)
        arguments["opacity"] = ActionArgs.number(min(max(settings.opacity, 0), 100) / 100)
        if settings.preserveTransparency { arguments["preserve_transparency"] = true }
        let swatches = ToolOptionsStore.shared.sharedState
        let stored = settings.contents == .foreground ? swatches.foregroundInk
            : settings.contents == .background ? swatches.backgroundInk : nil
        guard cmykPathInk(&arguments, color: color, stored: stored, document: document) else {
            return
        }
        runPathStep("fill_path", arguments, undoName: "Fill Path", note: "Paths panel ▸ Fill Path…")
    }

    /// Paths panel ▸ Stroke Path…: the Brush or the Eraser, then `stroke_path` with that tool's
    /// options-bar settings.
    @objc func strokeCurrentPathWithOptions(_ sender: Any?) {
        guard let document, !document.quickMaskActive,
              let current = currentPathStep(selectedOnly: true), !current.shape.isEmpty
        else {
            NSSound.beep()
            return
        }
        let hint = "Runs the tool along \(pathPaintSubject()) with its options-bar settings: the "
            + "Brush paints the foreground colour, the Eraser removes."
        let sheet = PathStrokeSheetController(hint: hint) { [weak self] erase in
            self?.strokePath(erase: erase, note: "Paths panel ▸ Stroke Path…")
        }
        presentAsSheet(sheet)
    }

    // MARK: - Validation

    /// Enablement for this file's items; nil for anybody else's. Reached through
    /// `validatePathsItem`.
    func validatePathSelectItem(_ item: NSValidatedUserInterfaceItem) -> Bool? {
        guard let document else { return nil }
        switch item.action {
        case #selector(combineSelectedSubpaths(_:)):
            return canCombineSelectedSubpaths
        case #selector(fillCurrentPathWithOptions(_:)), #selector(strokeCurrentPathWithOptions(_:)):
            return !document.quickMaskActive
                && (currentPathStep(selectedOnly: true).map { !$0.shape.isEmpty } ?? false)
        default:
            return nil
        }
    }
}
