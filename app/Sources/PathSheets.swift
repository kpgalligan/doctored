import AppKit

// The Paths panel's four dialogs. Each only CHOOSES: its commit closure is handed the values
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

/// Paths panel ▸ Clipping Path Flatness…: the clipping path's flatness, the
/// tolerance in device pixels an output device flattens its curves to.
/// `set_clipping_path`'s `flatness`.
final class ClippingFlatnessSheetController: NSViewController {
    private let flatness: Double
    private let onCommit: (_ flatness: Double) -> Void
    private let flatnessField = NSTextField(string: "")
    /// The field's value as first shown, so Apply on an untouched field
    /// (a file's own value, which may lie outside the range) changes nothing.
    private var shown = 0.0

    /// `flatness` is the clipping path's current one, 0 for none.
    init(flatness: Double, onCommit: @escaping (_ flatness: Double) -> Void) {
        self.flatness = flatness
        self.onCommit = onCommit
        super.init(nibName: nil, bundle: nil)
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) {
        fatalError("ClippingFlatnessSheetController does not support NSCoder")
    }

    override func loadView() {
        let formatter = NumberFormatter()
        formatter.numberStyle = .decimal
        formatter.minimumFractionDigits = 0
        formatter.maximumFractionDigits = 4
        flatnessField.formatter = formatter
        if flatness > 0 { flatnessField.doubleValue = flatness }
        shown = flatnessField.doubleValue
        flatnessField.placeholderString = "none"
        flatnessField.widthAnchor.constraint(equalToConstant: 80).isActive = true
        DSField.style(flatnessField)
        let unit = NSTextField(labelWithString: "px")
        unit.font = DS.mono(13)
        unit.textColor = DS.textMuted
        let row = NSStackView(views: [flatnessField, unit])
        row.orientation = .horizontal
        row.spacing = 8

        let grid = NSGridView(views: [[fieldLabel("Flatness:"), row]])
        grid.rowSpacing = 10
        grid.columnSpacing = 12
        grid.column(at: 0).xPlacement = .trailing
        grid.column(at: 0).width = 106

        let cancelButton = sheetCancelButton(target: self, action: #selector(cancelClicked(_:)))
        let applyButton = sheetApplyButton(target: self, action: #selector(applyClicked(_:)))
        view = makeSheetView(
            title: "Clipping path flatness",
            hint: "The tolerance, in device pixels, an output device flattens the clipping "
                + "path's curves to. A PSD, PSB or TIFF carries it with the clipping path. Empty "
                + "or 0 sets none.",
            content: grid,
            buttonRow: makeButtonRow(
                cancel: cancelButton, apply: applyButton,
                leading: [sheetFootnote("0.2 – 100 px, empty for none")]))
    }

    @objc private func applyClicked(_ sender: Any?) {
        let empty = flatnessField.stringValue.trimmingCharacters(in: .whitespaces).isEmpty
        let value = empty ? 0 : flatnessField.doubleValue
        guard value != shown else {
            dismiss(self)
            return
        }
        guard value.isFinite, value == 0 || (0.2...100).contains(value) else {
            NSSound.beep()
            return
        }
        dismiss(self)
        onCommit(value)
    }

    @objc private func cancelClicked(_ sender: Any?) {
        dismiss(self)
    }
}

/// Paths panel ▸ Combine with Path…: the other path and what to do with it. `combine_paths`'s
/// `with` and `operation`; the current path is its `path`.
final class PathCombineSheetController: NSViewController {
    private let path: String
    private let others: [String]
    private let onCommit: (_ with: String, _ operation: PathCombineOperation) -> Void
    private let pathPopup = NSPopUpButton(frame: .zero, pullsDown: false)
    private let operationPopup = NSPopUpButton(frame: .zero, pullsDown: false)

    /// `path` is the current path's name, `others` every other path's, in the list's order.
    init(path: String, others: [String],
         onCommit: @escaping (_ with: String, _ operation: PathCombineOperation) -> Void) {
        self.path = path
        self.others = others
        self.onCommit = onCommit
        super.init(nibName: nil, bundle: nil)
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) {
        fatalError("PathCombineSheetController does not support NSCoder")
    }

    override func loadView() {
        pathPopup.addItems(withTitles: others)
        pathPopup.selectItem(at: 0)
        pathPopup.font = DS.sans(13)
        operationPopup.addItems(withTitles: PathCombineOperation.allCases.map(\.title))
        operationPopup.selectItem(at: 0)
        operationPopup.font = DS.sans(13)

        let grid = NSGridView(views: [
            [fieldLabel("With:"), pathPopup],
            [fieldLabel("Operation:"), operationPopup],
        ])
        grid.rowSpacing = 10
        grid.columnSpacing = 12
        grid.column(at: 0).xPlacement = .trailing
        grid.column(at: 0).width = 106

        let cancelButton = sheetCancelButton(target: self, action: #selector(cancelClicked(_:)))
        let applyButton = sheetApplyButton(target: self, action: #selector(applyClicked(_:)))
        view = makeSheetView(
            title: "Combine paths",
            hint: "Replaces “\(path)” with its fill combined with the other path's. The other "
                + "path is unchanged; the result's subpaths are closed.",
            content: grid,
            buttonRow: makeButtonRow(cancel: cancelButton, apply: applyButton))
    }

    @objc private func applyClicked(_ sender: Any?) {
        let index = pathPopup.indexOfSelectedItem
        guard others.indices.contains(index) else {
            NSSound.beep()
            return
        }
        let operations = PathCombineOperation.allCases
        let operation = operations[min(max(operationPopup.indexOfSelectedItem, 0), operations.count - 1)]
        dismiss(self)
        onCommit(others[index], operation)
    }

    @objc private func cancelClicked(_ sender: Any?) {
        dismiss(self)
    }
}
