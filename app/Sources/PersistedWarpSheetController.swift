import AppKit

// The numeric sheet holds original Double nodes, resets only on dimension changes, and refuses
// stale document/target/selection.

/// Layer ▸ Warp… numeric editor. The full binary64 lattice is local until Apply.
/// At 32-bit float and in a CMYK document (#547 phase 4b) a warp is
/// applied to the pixels at once and keeps no re-editable field, so Remove and
/// Bake are disabled and each Apply warps the current pixels again.
final class PersistedWarpSheetController: NSViewController, NSTextFieldDelegate {
    private weak var editor: EditorViewController?
    private weak var document: ImageDocument?
    private let source: RasterDocument
    private let revision: UInt64
    private let state: EditingState?
    private let layer: Int
    private let layerID: UInt64
    private var target: PersistedWarpTarget
    private var field: PersistedWarpField
    private var selectedNode = 0
    /// Float's, CMYK's, (#547 phase 4) Grayscale's and (#547 Lab phase 4a-ii) Lab's rule: the warp
    /// is applied at once, with no field.
    private var appliedAtOnce: Bool {
        source.isFloat || source.isCmyk || source.isGray || source.isLab
    }

    private let targetPopup = NSPopUpButton(frame: .zero, pullsDown: false)
    private let columnsPopup = NSPopUpButton(frame: .zero, pullsDown: false)
    private let rowsPopup = NSPopUpButton(frame: .zero, pullsDown: false)
    private let nodePopup = NSPopUpButton(frame: .zero, pullsDown: false)
    private let filterPopup = NSPopUpButton(frame: .zero, pullsDown: false)
    private let dx = NSTextField(string: "0")
    private let dy = NSTextField(string: "0")
    private let note = NSTextField(labelWithString: "")

    init?(editor: EditorViewController) {
        guard let document = editor.document, let source = document.doc,
              let info = source.layerInfo(document.activeLayerIndex),
              let warp = try? source.warpInfo(document.activeLayerIndex) else { return nil }
        self.editor = editor
        self.document = document
        self.source = source
        self.revision = document.revision
        self.state = document.captureEditingState()
        self.layer = document.activeLayerIndex
        self.layerID = info.id
        if let stored = warp["field"] as? [String: Any],
           let target = (stored["target"] as? String).flatMap(PersistedWarpTarget.init(rawValue:)),
           let filter = (stored["filter"] as? String).flatMap(PersistedWarpFilter.init(rawValue:)),
           let columns = stored["columns"] as? Int,
           let rows = stored["rows"] as? Int,
           let values = stored["displacements"] as? [[NSNumber]] {
            self.target = target
            self.field = PersistedWarpField(columns: columns, rows: rows,
                displacements: values.map { $0.map { $0.doubleValue } }, filter: filter)
        } else {
            self.target = .pixels
            self.field = .identity()
        }
        super.init(nibName: nil, bundle: nil)
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) {
        fatalError("PersistedWarpSheetController does not support NSCoder")
    }

    override func loadView() {
        for (popup, titles, id) in [
            (targetPopup, ["Pixels", "Mask"], "warp-target"),
            (filterPopup, ["Bilinear", "Bicubic"], "warp-filter")
        ] {
            titles.forEach { popup.addItem(withTitle: $0) }
            popup.setAccessibilityIdentifier(id)
        }
        targetPopup.selectItem(at: target == .pixels ? 0 : 1)
        filterPopup.selectItem(at: field.filter == .bilinear ? 0 : 1)
        targetPopup.target = self; targetPopup.action = #selector(targetChanged(_:))
        filterPopup.target = self; filterPopup.action = #selector(filterChanged(_:))
        for n in 2...16 {
            columnsPopup.addItem(withTitle: String(n))
            rowsPopup.addItem(withTitle: String(n))
        }
        columnsPopup.setAccessibilityIdentifier("warp-columns")
        rowsPopup.setAccessibilityIdentifier("warp-rows")
        columnsPopup.selectItem(at: field.columns - 2)
        rowsPopup.selectItem(at: field.rows - 2)
        columnsPopup.target = self; columnsPopup.action = #selector(dimensionsChanged(_:))
        rowsPopup.target = self; rowsPopup.action = #selector(dimensionsChanged(_:))
        nodePopup.setAccessibilityIdentifier("warp-node")
        nodePopup.target = self; nodePopup.action = #selector(nodeChanged(_:))
        rebuildNodePopup()
        for (text, id) in [(dx, "warp-dx"), (dy, "warp-dy")] {
            text.setAccessibilityIdentifier(id)
            text.delegate = self
            text.alignment = .right
            text.frame.size.width = 100
        }
        showNode()
        note.font = DS.sans(11)
        note.textColor = .secondaryLabelColor
        note.lineBreakMode = .byWordWrapping
        note.maximumNumberOfLines = 3
        note.stringValue = appliedAtOnce
            ? "Each Apply warps the current pixels again. Use Edit ▸ Undo to adjust or remove a warp."
            : "Node displacements are in original canvas pixels. Changing dimensions resets the local field."
        let grid = NSGridView(views: [
            [fieldLabel("Target:"), targetPopup],
            [fieldLabel("Columns:"), columnsPopup],
            [fieldLabel("Rows:"), rowsPopup],
            [fieldLabel("Node:"), nodePopup],
            [fieldLabel("dx:"), dx],
            [fieldLabel("dy:"), dy],
            [fieldLabel("Sampler:"), filterPopup]
        ])
        grid.rowSpacing = 8; grid.columnSpacing = 12
        grid.column(at: 0).xPlacement = .trailing
        let reset = NSButton(title: "Reset Field", target: self, action: #selector(resetClicked(_:)))
        reset.setAccessibilityIdentifier("warp-reset")
        let clear = NSButton(title: "Remove Warp", target: self, action: #selector(clearClicked(_:)))
        clear.setAccessibilityIdentifier("warp-clear")
        let bake = NSButton(title: "Bake Warp", target: self, action: #selector(bakeClicked(_:)))
        bake.setAccessibilityIdentifier("warp-bake")
        // A float or CMYK warp keeps no field, so there is nothing to remove or bake.
        clear.isEnabled = !appliedAtOnce
        bake.isEnabled = !appliedAtOnce
        let secondary = NSStackView(views: [reset, clear, bake])
        secondary.orientation = .horizontal; secondary.spacing = 10
        let content = NSStackView(views: [grid, secondary, note])
        content.orientation = .vertical; content.alignment = .leading; content.spacing = 12
        let cancel = sheetCancelButton(target: self, action: #selector(cancelClicked(_:)))
        cancel.setAccessibilityIdentifier("warp-cancel")
        let apply = sheetApplyButton(target: self, action: #selector(applyClicked(_:)))
        apply.setAccessibilityIdentifier("warp-apply")
        let hint = source.isFloat
            ? "Warp the entire layer or mask. In a 32-bit float document the warp is applied to the pixels and cannot be reopened."
            : source.isCmyk
            ? "Warp the entire layer or mask. In a CMYK document the warp is applied to the ink and cannot be reopened."
            : source.isGray || source.isLab
            ? "Warp the entire layer or mask. In a Grayscale or Lab Color document the warp is applied to the pixels and cannot be reopened."
            : "Edit a re-openable displacement field on the entire layer or mask."
        view = makeSheetView(title: "Warp", hint: hint,
            content: content, buttonRow: makeButtonRow(cancel: cancel, apply: apply,
                                                        leading: [sheetFootnote("one undo step")]))
    }

    private func rebuildNodePopup() {
        nodePopup.removeAllItems()
        for row in 0..<field.rows {
            for column in 0..<field.columns {
                nodePopup.addItem(withTitle: "Row \(row + 1), Column \(column + 1)")
            }
        }
        selectedNode = 0
        nodePopup.selectItem(at: 0)
    }

    private func showNode() {
        guard field.displacements.indices.contains(selectedNode) else { return }
        dx.stringValue = String(field.displacements[selectedNode][0])
        dy.stringValue = String(field.displacements[selectedNode][1])
    }

    @discardableResult
    private func saveNode() -> Bool {
        guard let x = Double(dx.stringValue), let y = Double(dy.stringValue),
              x.isFinite, y.isFinite, abs(x) <= 30_000, abs(y) <= 30_000 else {
            note.stringValue = "Each node needs finite dx/dy within ±30000 pixels."
            NSSound.beep()
            return false
        }
        field.displacements[selectedNode] = [x, y]
        return true
    }

    func controlTextDidEndEditing(_ obj: Notification) { _ = saveNode() }

    @objc private func nodeChanged(_ sender: Any?) {
        guard saveNode() else { nodePopup.selectItem(at: selectedNode); return }
        selectedNode = nodePopup.indexOfSelectedItem
        showNode()
    }
    @objc private func dimensionsChanged(_ sender: Any?) {
        guard saveNode() else { return }
        field = .identity(columns: columnsPopup.indexOfSelectedItem + 2,
                          rows: rowsPopup.indexOfSelectedItem + 2, filter: field.filter)
        rebuildNodePopup(); showNode()
        note.stringValue = "The lattice was reset after changing dimensions."
    }
    @objc private func targetChanged(_ sender: Any?) {
        target = targetPopup.indexOfSelectedItem == 0 ? .pixels : .mask
    }
    @objc private func filterChanged(_ sender: Any?) {
        field.filter = filterPopup.indexOfSelectedItem == 0 ? .bilinear : .bicubic
    }
    @objc private func resetClicked(_ sender: Any?) {
        field = .identity(columns: field.columns, rows: field.rows, filter: field.filter)
        showNode()
    }
    @objc private func applyClicked(_ sender: Any?) {
        guard saveNode() else { return }
        // At float and in CMYK an identity Apply has no stored field to
        // remove, so it would change nothing and close silently; say so and
        // stay open.
        if appliedAtOnce && field.isIdentity {
            note.stringValue = "This warp is already in the pixels, and a zero field changes nothing. Use Edit ▸ Undo to remove it."
            NSSound.beep()
            return
        }
        commit(.set(layer: layer, target: target, field: field))
    }
    @objc private func clearClicked(_ sender: Any?) { commit(.clear(layer: layer)) }
    @objc private func bakeClicked(_ sender: Any?) { commit(.bake(layer: layer)) }
    @objc private func cancelClicked(_ sender: Any?) { dismiss(self) }

    private func commit(_ request: PersistedWarpRequest) {
        guard let document, document.doc === source, document.revision == revision,
              document.isCurrentEditingState(state),
              source.layerInfo(layer)?.id == layerID else {
            document?.presentError(DoctoredCoreError(message: "The warp sheet's document, target or selection changed. Reopen it."))
            return
        }
        dismiss(self)
        editor?.commitPersistedWarp(request, source: source)
    }
}
