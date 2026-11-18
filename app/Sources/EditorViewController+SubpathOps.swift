import AppKit

/// The Path Selection and Direct Selection options bar's Operation
/// (`designs/v1-paths-held-subpaths.md`, HS-7): the operation the selected subpaths HOLD, set as
/// ONE undo step through the gestures' commit (`penGestureCommitted`). On a path of the list it is
/// a render-neutral path edit recorded
/// as `set_path` with the whole shape, on a vector mask its rebake (`set_vector_mask`), on a path
/// shape's outline its re-render (`edit_shape_layer`), so the popup needs no MCP tool of its own:
/// `subpaths[i].op` on those tools is its twin. Also the alert the canvas commits show when a path
/// shape cannot hold its outline's operations (`ShapeLayerPayload.heldOpsRefusal`).
extension EditorViewController {
    /// The popup's rows: "—" (no operation held, a mix, or nothing selected), then the four.
    private static let operationItems = ["—"] + PathCombineOperation.allCases.map(\.title)

    /// The Operation popup, enabled as the Combine buttons are.
    func subpathOperationCluster() -> OptionCluster {
        OptionCluster([
            OptionDescriptor(
                id: "pathSelect.operation", microLabel: "Operation", overflowLabel: "Operation",
                kind: .popup(
                    width: 96, items: Self.operationItems,
                    get: { [weak self] in self?.selectedSubpathsOperationRow() ?? 0 },
                    set: { [weak self] row in self?.subpathOperationChosen(row) }),
                isEnabled: { [weak self] in self?.canCombineSelectedSubpaths ?? false }),
        ])
    }

    /// The popup's row: the operation every selected subpath holds, 0 when the path holds none,
    /// the selected subpaths hold a mix, or nothing is selected.
    private func selectedSubpathsOperationRow() -> Int {
        guard let selected = selectedSubpathsOfCurrentPath(), let ops = selected.shape.ops else {
            return 0
        }
        let held = Set(selected.indices.map { ops[$0] })
        guard held.count == 1, let op = held.first,
              let index = PathCombineOperation.allCases.firstIndex(of: op)
        else { return 0 }
        return index + 1
    }

    /// A row chosen: "—" does nothing; any other sets its operation.
    private func subpathOperationChosen(_ row: Int) {
        let operations = PathCombineOperation.allCases
        guard row > 0, row <= operations.count else { return }
        setSelectedSubpathsOperation(operations[row - 1])
    }

    /// The selected subpaths of the current path take `op`, as ONE undo step named
    /// "Set Subpath Operation" that keeps the selection
    /// (`ImageCanvasView.setSelectedSubpathsOperation`). A path that held no operations starts
    /// holding them, every other subpath `unite`. Nothing
    /// selected beeps; the operation the subpaths already hold records nothing. True when
    /// committed.
    @discardableResult
    func setSelectedSubpathsOperation(_ op: PathCombineOperation) -> Bool {
        guard let document, !document.quickMaskActive, selectedSubpathsOfCurrentPath() != nil else {
            NSSound.beep()
            return false
        }
        let committed = canvas.setSelectedSubpathsOperation(op)
        optionsBar.refreshValues()
        if committed { pathsPanel?.reload() }
        return committed
    }

    /// The alert a canvas commit shows when a path shape cannot hold its outline's operations
    /// (`ShapeLayerPayload.heldOpsRefusal`); the commit then changes nothing.
    func presentHeldOpsRefusal(_ refusal: String) {
        let alert = NSAlert()
        alert.messageText = "The shape cannot hold these subpath operations"
        alert.informativeText = refusal
        SessionCommitRefusal.note(refusal)
        alert.runModal()
    }
}
