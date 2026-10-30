import AppKit

/// Select and Mask's Edge Detection, Refine Edge brush and Decontaminate
/// Colors rows, and their actions. The controls are stored on the
/// controller; `parameters` reads them.
extension RefineSelectionSheetController {
    // MARK: - Rows

    /// Radius (field and slider) and Smart Radius.
    func makeEdgeDetectionRows() -> [NSView] {
        let field = selectionSheetField(0, identifier: "refine.edgeRadius",
                                        target: self, action: #selector(edgeRadiusFieldChanged(_:)))
        let slider = NSSlider(value: 0, minValue: 0, maxValue: 64,
                              target: self, action: #selector(edgeRadiusSliderChanged(_:)))
        slider.isContinuous = true
        slider.widthAnchor.constraint(equalToConstant: 210).isActive = true
        edgeRadiusField = field; edgeRadiusSlider = slider
        let row = NSStackView(views: [fieldLabel("Edge Radius (px)"), field])
        row.distribution = .fill; row.spacing = 8
        smartRadius.identifier = NSUserInterfaceItemIdentifier("refine.smartRadius")
        smartRadius.target = self; smartRadius.action = #selector(edgeControlChanged(_:))
        smartRadius.toolTip = "Refine only where the picture has an edge, so a hard edge across flat colour stays hard."
        return [row, slider, smartRadius]
    }

    /// Refine Edge brush: Off / Add / Subtract, Size, Undo Stroke, Clear.
    func makeBrushRows() -> [NSView] {
        brushMode.addItems(withTitles: ["Off", "Add", "Subtract"])
        brushMode.identifier = NSUserInterfaceItemIdentifier("refine.brush")
        brushMode.target = self; brushMode.action = #selector(brushModeChanged(_:))
        brushMode.toolTip = "Drag in the preview: Add always refines the edge under the stroke, Subtract never does."
        let size = selectionSheetField(brush.size, identifier: "refine.brushSize",
                                       target: self, action: #selector(brushSizeChanged(_:)))
        brushSizeField = size
        let mode = NSStackView(views: [fieldLabel("Refine Edge Brush"), brushMode])
        mode.distribution = .fill; mode.spacing = 8
        let sizeRow = NSStackView(views: [fieldLabel("Brush Size (px)"), size])
        sizeRow.distribution = .fill; sizeRow.spacing = 8
        let undo = StickerButton(title: "Undo Stroke", style: .secondary, target: self,
                                 action: #selector(undoStrokeClicked(_:)))
        undo.identifier = NSUserInterfaceItemIdentifier("refine.undoStroke")
        let clear = StickerButton(title: "Clear", style: .secondary, target: self,
                                  action: #selector(clearStrokesClicked(_:)))
        clear.identifier = NSUserInterfaceItemIdentifier("refine.clearStrokes")
        undoStrokeButton = undo; clearStrokesButton = clear
        let buttons = NSStackView(views: [undo, clear])
        buttons.spacing = 8
        return [mode, sizeRow, buttons]
    }

    /// Decontaminate Colors and its Amount. Disabled in a 32-bit float document,
    /// where the core refuses it by name: its foreground solve reads and
    /// writes 0…1 codes, so a fringe above white would be clipped.
    func makeDecontaminateRows() -> [NSView] {
        decontaminate.identifier = NSUserInterfaceItemIdentifier("refine.decontaminate")
        decontaminate.target = self; decontaminate.action = #selector(decontaminateChanged(_:))
        let float = capture.selection.doc.isFloat
        decontaminate.toolTip = float
            ? "Unavailable in a 32-bit float document: the foreground estimate works on 0–1 "
                + "colour codes, so a fringe brighter than white would be clipped."
            : "Replace fringe colour with the estimated foreground colour. Needs a New Layer output."
        decontaminate.isEnabled = !float
        let amount = selectionSheetField(100, identifier: "refine.decontaminateAmount",
                                         target: self, action: #selector(edgeControlChanged(_:)))
        amount.isEnabled = !float
        decontaminateAmountField = amount
        let row = NSStackView(views: [fieldLabel("Amount (%)"), amount])
        row.distribution = .fill; row.spacing = 8
        return [decontaminate, row]
    }

    // MARK: - Actions

    @objc func edgeRadiusSliderChanged(_ sender: NSSlider) {
        edgeRadiusField?.stringValue = String(format: "%.2f", sender.doubleValue)
        requestPreview()
    }

    @objc func edgeRadiusFieldChanged(_ sender: Any?) {
        if let value = edgeRadiusField.flatMap({ Double($0.stringValue) }), value.isFinite {
            edgeRadiusSlider?.doubleValue = value
        }
        requestPreview()
    }

    @objc func edgeControlChanged(_ sender: Any?) { requestPreview() }

    @objc func brushModeChanged(_ sender: Any?) {
        brush.tool = RefineEdgeBrushSession.Tool(rawValue: brushMode.indexOfSelectedItem) ?? .off
    }

    @objc func brushSizeChanged(_ sender: Any?) {
        guard let field = brushSizeField else { return }
        if let value = Double(field.stringValue), value.isFinite {
            brush.size = min(1024, max(1, value))
        }
        field.stringValue = String(brush.size)
    }

    @objc func undoStrokeClicked(_ sender: Any?) {
        guard brush.undo() else { NSSound.beep(); return }
        strokesChanged()
    }

    @objc func clearStrokesClicked(_ sender: Any?) {
        guard brush.clear() else { NSSound.beep(); return }
        strokesChanged()
    }

    /// Decontaminate Colors changes pixels, so turning it on moves a
    /// Selection or Layer Mask output to New Layer with Layer Mask.
    @objc func decontaminateChanged(_ sender: Any?) {
        if decontaminate.state == .on, !selectedOutput.createsLayer,
           let index = Self.outputs.firstIndex(of: .newLayerWithMask) {
            output.selectItem(at: index)
        }
        requestPreview()
    }

    /// Choosing Selection or Layer Mask turns Decontaminate Colors off: those
    /// outputs carry coverage only.
    @objc func outputChanged(_ sender: Any?) {
        if !selectedOutput.createsLayer { decontaminate.state = .off }
        requestPreview()
    }

    // MARK: - Brush drags (forwarded by `RefinementCanvasView`)

    func brushBegan(at point: CGPoint) -> Bool {
        guard brush.begin(at: point) else { return false }
        showStrokes()
        return true
    }

    func brushMoved(to point: CGPoint) {
        brush.append(point)
        showStrokes()
    }

    func brushEnded() {
        guard brush.finish() else {
            NSSound.beep()
            showStrokes()
            return
        }
        strokesChanged()
    }

    /// The committed strokes changed: redraw them and refine again.
    private func strokesChanged() {
        syncBrushControls()
        requestPreview()
    }

    func syncBrushControls() {
        showStrokes()
        undoStrokeButton?.isEnabled = brush.canUndo
        clearStrokesButton?.isEnabled = brush.canUndo
    }

    private func showStrokes() {
        inspection.strokes = brush.committed + (brush.draft.map { [$0] } ?? [])
    }
}
