import AppKit

/// The Paths panel's two commands that turn a path into something else: Convert to Shape Layer
/// (a shape layer of the `path` kind, ShapeOutline.swift) and Combine with Path… (a path boolean,
/// PathCombine.swift). Their MCP twins are `add_shape_layer` with kind "path" and
/// `combine_paths`, and each records its twin.
extension EditorViewController {
    /// Paths panel ▸ Convert to Shape Layer: the current path — a path of the list or the active
    /// layer's vector mask — becomes a new shape layer above the active one, styled by the shape
    /// tools' options bar exactly as a dragged shape is, and committed through the shape tools'
    /// own commit (`commitNewShapeLayer`), so CMYK inks, a float document's colours and the
    /// recorded `add_shape_layer` step are the shape tools'. One undo step.
    @objc func convertPathToShape(_ sender: Any?) {
        // An open shape re-edit ends first (its layer index would move under the new layer).
        guard shapeEditSession == nil || commitShapeEditSession() else { return }
        guard let document, let doc = document.doc, !document.quickMaskActive,
              let current = currentPathStep(), !current.shape.isEmpty,
              let placed = ShapeOutline.placed(current.shape)
        else {
            NSSound.beep()
            return
        }
        let options = ToolOptionsStore.shared.shape
        let payload = ShapeLayerPayload(
            kind: "path",
            w: Double(placed.box.width), h: Double(placed.box.height),
            fill: options.fillColor(float: doc.isFloat),
            stroke: options.strokeColor(float: doc.isFloat),
            strokeWidth: options.strokeWidth,
            ink: doc.authoringInk,
            strokeJoin: options.strokeJoin, strokeDash: options.strokeDash,
            strokeAlign: options.strokeAlign,
            path: placed.outline)
        // The box's corner is whole pixels by construction (`ShapeOutline.placed`), and a new
        // shape is never transformed.
        commitNewShapeLayer(
            payload, anchor: DescribedLayer.placementAnchor(placed.box.origin, transform: .identity))
    }

    /// Paths panel ▸ Combine with Path…: the sheet picks the other path and the operation, then
    /// `combine_paths` runs on the current path through the agent's dispatch
    /// (`runPathStep`), which records it.
    @objc func combinePaths(_ sender: Any?) {
        guard let (set, index) = currentListPath(), set.entries.count > 1 else {
            NSSound.beep()
            return
        }
        let name = set.entries[index].name
        let others = set.entries.map(\.name).filter { $0 != name }
        let sheet = PathCombineSheetController(path: name, others: others) {
            [weak self] with, operation in
            self?.commitCombinePaths(with: with, operation: operation)
        }
        presentAsSheet(sheet)
    }

    /// The sheet's commit. The current path is read again: the sheet does not stop the run loop,
    /// and an agent may have renamed, edited or deleted a path meanwhile.
    func commitCombinePaths(with other: String, operation: PathCombineOperation) {
        guard let (set, index) = currentListPath() else {
            NSSound.beep()
            return
        }
        runPathStep(
            "combine_paths",
            ["path": set.entries[index].name, "with": other, "operation": operation.rawValue],
            undoName: "Combine Paths", note: "Paths panel ▸ Combine with Path…")
        refreshCanvasPath()
        pathsPanel?.reload()
    }

    /// Enablement for this file's items; nil for anybody else's. Reached through
    /// `validatePathsItem`.
    func validatePathCommandItem(_ item: NSValidatedUserInterfaceItem) -> Bool? {
        guard let document else { return nil }
        switch item.action {
        case #selector(convertPathToShape(_:)):
            return !document.quickMaskActive
                && (currentPathStep().map { !$0.shape.isEmpty } ?? false)
        case #selector(combinePaths(_:)):
            return currentListPath().map { $0.set.entries.count > 1 } ?? false
        default:
            return nil
        }
    }
}
