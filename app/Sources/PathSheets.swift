import AppKit

// The Paths panel's two dialogs. Each only CHOOSES: its commit closure is handed the values
// and the editor runs the MCP step they describe (`EditorViewController+Paths.swift`), so the
// sheet, the recorded Action and an agent's call are one path.

/// Paths panel ▸ Make Selection…: how the current path's fill becomes the
/// selection — the feather radius, the anti-aliased edge and how it combines
/// with the selection already there. `path_to_selection`'s three arguments.
final class PathSelectionSheetController: NSViewController {
    /// The Operation popup, top to bottom: its title and `path_to_selection`'s
    /// `mode`.
    private static let operations: [(title: String, mode: String)] = [
        ("New Selection", "replace"), ("Add to Selection", "add"),
        ("Subtract from Selection", "subtract"), ("Intersect with Selection", "intersect"),
    ]

    private let hasSelection: Bool
    private let onCommit: (_ feather: Double, _ antiAlias: Bool, _ mode: String) -> Void

    private let featherField = NSTextField(string: "0")
    private let antiAliasCheckbox = NSButton(
        checkboxWithTitle: "Anti-aliased", target: nil, action: nil)
    private let modePopup = NSPopUpButton(frame: .zero, pullsDown: false)

    /// `hasSelection` is whether there is a selection to combine with; with
    /// none the popup offers New Selection alone.
    init(
        hasSelection: Bool,
        onCommit: @escaping (_ feather: Double, _ antiAlias: Bool, _ mode: String) -> Void
    ) {
        self.hasSelection = hasSelection
        self.onCommit = onCommit
        super.init(nibName: nil, bundle: nil)
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) {
        fatalError("PathSelectionSheetController does not support NSCoder")
    }

    override func loadView() {
        let formatter = NumberFormatter()
        formatter.numberStyle = .decimal
        formatter.minimumFractionDigits = 0
        formatter.maximumFractionDigits = 1
        featherField.formatter = formatter
        featherField.doubleValue = 0
        featherField.widthAnchor.constraint(equalToConstant: 80).isActive = true
        DSField.style(featherField)
        let unit = NSTextField(labelWithString: "px")
        unit.font = DS.mono(13)
        unit.textColor = DS.textMuted
        let featherRow = NSStackView(views: [featherField, unit])
        featherRow.orientation = .horizontal
        featherRow.spacing = 8

        antiAliasCheckbox.font = DS.sans(13)
        antiAliasCheckbox.state = .on

        let offered = hasSelection ? Self.operations : Array(Self.operations.prefix(1))
        modePopup.addItems(withTitles: offered.map(\.title))
        modePopup.selectItem(at: 0)
        modePopup.font = DS.sans(13)

        let grid = NSGridView(views: [
            [fieldLabel("Feather Radius:"), featherRow],
            [NSGridCell.emptyContentView, antiAliasCheckbox],
            [fieldLabel("Operation:"), modePopup],
        ])
        grid.rowSpacing = 10
        grid.columnSpacing = 12
        grid.column(at: 0).xPlacement = .trailing
        grid.column(at: 0).width = 106

        let cancelButton = sheetCancelButton(target: self, action: #selector(cancelClicked(_:)))
        let applyButton = sheetApplyButton(target: self, action: #selector(applyClicked(_:)))
        view = makeSheetView(
            title: "Make selection",
            hint: "The path's fill becomes the selection. An open subpath is closed by a "
                + "straight line.",
            content: grid,
            buttonRow: makeButtonRow(
                cancel: cancelButton, apply: applyButton,
                leading: [sheetFootnote("feather 0 – 250 px")]))
    }

    @objc private func applyClicked(_ sender: Any?) {
        // An empty field reads as 0, which is the field's own default.
        let feather = featherField.doubleValue
        guard feather.isFinite, (0...250).contains(feather) else {
            NSSound.beep()
            return
        }
        let index = max(modePopup.indexOfSelectedItem, 0)
        let mode = Self.operations[min(index, Self.operations.count - 1)].mode
        let antiAlias = antiAliasCheckbox.state == .on
        dismiss(self)
        onCommit(feather, antiAlias, mode)
    }

    @objc private func cancelClicked(_ sender: Any?) {
        dismiss(self)
    }
}

/// Select ▸ Make Work Path… (and the Paths panel's own item): how closely the
/// traced work path follows the selection's edge. `selection_to_path`'s
/// `tolerance`.
final class MakeWorkPathSheetController: NSViewController {
    private let replacesWorkPath: Bool
    private let onCommit: (_ tolerance: Double) -> Void
    private let toleranceField = NSTextField(string: "2")

    /// `replacesWorkPath` is whether the document already has a work path,
    /// which this replaces; the hint says so.
    init(replacesWorkPath: Bool, onCommit: @escaping (_ tolerance: Double) -> Void) {
        self.replacesWorkPath = replacesWorkPath
        self.onCommit = onCommit
        super.init(nibName: nil, bundle: nil)
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) {
        fatalError("MakeWorkPathSheetController does not support NSCoder")
    }

    override func loadView() {
        let formatter = NumberFormatter()
        formatter.numberStyle = .decimal
        formatter.minimumFractionDigits = 0
        formatter.maximumFractionDigits = 1
        toleranceField.formatter = formatter
        toleranceField.doubleValue = 2
        toleranceField.widthAnchor.constraint(equalToConstant: 80).isActive = true
        DSField.style(toleranceField)
        let unit = NSTextField(labelWithString: "px")
        unit.font = DS.mono(13)
        unit.textColor = DS.textMuted
        let row = NSStackView(views: [toleranceField, unit])
        row.orientation = .horizontal
        row.spacing = 8

        let grid = NSGridView(views: [[fieldLabel("Tolerance:"), row]])
        grid.rowSpacing = 10
        grid.columnSpacing = 12
        grid.column(at: 0).xPlacement = .trailing
        grid.column(at: 0).width = 106

        let cancelButton = sheetCancelButton(target: self, action: #selector(cancelClicked(_:)))
        let applyButton = sheetApplyButton(target: self, action: #selector(applyClicked(_:)))
        view = makeSheetView(
            title: "Make work path",
            hint: "Traces the selection's edge into the work path"
                + (replacesWorkPath ? ", replacing the one the document has" : "")
                + ". A larger tolerance follows the edge less closely and makes fewer anchors.",
            content: grid,
            buttonRow: makeButtonRow(
                cancel: cancelButton, apply: applyButton,
                leading: [sheetFootnote("0.5 – 10 px")]))
    }

    @objc private func applyClicked(_ sender: Any?) {
        let tolerance = toleranceField.doubleValue
        guard !toleranceField.stringValue.trimmingCharacters(in: .whitespaces).isEmpty,
              tolerance.isFinite, (0.5...10).contains(tolerance)
        else {
            NSSound.beep()
            return
        }
        dismiss(self)
        onCommit(tolerance)
    }

    @objc private func cancelClicked(_ sender: Any?) {
        dismiss(self)
    }
}
