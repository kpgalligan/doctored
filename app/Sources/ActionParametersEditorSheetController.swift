import AppKit

/// The Actions window's Parameters… sheet: one row per declared parameter — its name, type,
/// default and description, and a Remove box — plus an empty row that adds one. Save turns the
/// rows into `ActionParameterEdit`s and runs them through `Action.applying`, the same all-or-nothing
/// path as MCP `edit_action_parameters`; a refusal is shown in the sheet, which stays open.
final class ActionParametersEditorSheetController: NSObject {
    private struct Row {
        let original: ActionParameter?
        let name = NSTextField()
        let type = NSPopUpButton()
        let value = NSTextField()
        let description = NSTextField()
        let remove = NSButton(checkboxWithTitle: "Remove", target: nil, action: nil)
    }

    private let action: Action
    private var rows: [Row] = []
    private let message = NSTextField(wrappingLabelWithString: "")
    private var modal: ModalCardWindowController?
    private var result: Action?

    private init(action: Action) {
        self.action = action
        super.init()
    }

    /// The edited action, or nil when the user cancelled or changed nothing.
    static func run(for action: Action) -> Action? {
        ActionParametersEditorSheetController(action: action).present()
    }

    private func present() -> Action? {
        var views: [NSView] = []
        let declared: [ActionParameter?] = action.parameters.map { $0 } + [nil]
        for parameter in declared {
            let row = makeRow(parameter)
            rows.append(row)
            views.append(rowView(row))
        }
        message.font = DS.sans(11)
        message.textColor = DS.warningText
        message.preferredMaxLayoutWidth = 520
        views.append(message)
        let content = NSStackView(views: views)
        content.orientation = .vertical
        content.alignment = .leading
        content.spacing = 8

        let cancel = sheetCancelButton(target: self, action: #selector(cancelClicked(_:)))
        let save = StickerButton(
            title: "Save", style: .primary, target: self, action: #selector(saveClicked(_:)))
        save.setAccessibilityIdentifier("action-parameters-editor-save")
        modal = ModalCardWindowController(
            title: "\(action.name) Parameters",
            card: makeSheetView(
                title: "\(action.name) Parameters",
                hint: "Name, type, default and description of each value a caller may supply. "
                    + "A rename rewrites every step that uses the parameter; an empty default "
                    + "makes it required; the last row adds one.",
                content: content, buttonRow: makeButtonRow(cancel: cancel, apply: save),
                width: 600))
        guard modal?.runModal() == .OK else { return nil }
        return result
    }

    private func makeRow(_ parameter: ActionParameter?) -> Row {
        let row = Row(original: parameter)
        for (field, width, placeholder) in [(row.name, 120.0, "new name"),
                                            (row.value, 100.0, "default"),
                                            (row.description, 170.0, "description")] {
            DSField.style(field)
            field.widthAnchor.constraint(equalToConstant: width).isActive = true
            field.placeholderString = placeholder
        }
        row.type.addItems(withTitles: ActionParameter.ParameterType.allCases.map(\.rawValue))
        row.remove.font = DS.sans(11)
        if let parameter {
            row.name.stringValue = parameter.name
            row.type.selectItem(withTitle: parameter.type.rawValue)
            row.value.stringValue = parameter.default.map(Self.text) ?? ""
            row.description.stringValue = parameter.description ?? ""
        }
        let id = parameter?.name ?? "new"
        row.name.setAccessibilityIdentifier("action-parameter-name-\(id)")
        row.type.setAccessibilityIdentifier("action-parameter-type-\(id)")
        row.value.setAccessibilityIdentifier("action-parameter-default-\(id)")
        return row
    }

    private func rowView(_ row: Row) -> NSView {
        var views: [NSView] = [row.name, row.type, row.value, row.description]
        if row.original != nil { views.append(row.remove) }
        let stack = NSStackView(views: views)
        stack.spacing = 6
        stack.alignment = .centerY
        return stack
    }

    @objc private func cancelClicked(_ sender: Any?) { modal?.end(.cancel) }

    @objc private func saveClicked(_ sender: Any?) {
        var edits: [ActionParameterEdit] = []
        for row in rows {
            let fields = Fields(
                original: row.original, name: row.name.stringValue,
                type: ActionParameter.ParameterType(
                    rawValue: row.type.titleOfSelectedItem ?? "") ?? .string,
                value: row.value.stringValue, description: row.description.stringValue,
                remove: row.remove.state == .on)
            switch Self.edits(for: fields) {
            case .success(let rowEdits): edits += rowEdits
            case .failure(let error): return refuse(error.message)
            }
        }
        guard !edits.isEmpty else {
            modal?.end(.cancel)
            return
        }
        switch action.applying(edits) {
        case .success(let edited):
            result = edited
            modal?.end(.OK)
        case .failure(let error):
            refuse(error.message)
        }
    }

    private func refuse(_ text: String) {
        message.stringValue = text
        NSSound.beep()
    }

    /// One row as Save reads it: the parameter it was filled from (nil for the add row) and the
    /// text of its fields.
    struct Fields {
        let original: ActionParameter?
        let name: String
        let type: ActionParameter.ParameterType
        let value: String
        let description: String
        let remove: Bool
    }

    /// The edits one row asks for, or the sentence refusing its default. A string default is the
    /// field's text as typed, spaces included; the other types parse the trimmed text. A default
    /// is set only when it differs from the stored one, so a field left as the sheet filled it
    /// asks for nothing.
    static func edits(for fields: Fields) -> Result<[ActionParameterEdit], DoctoredCoreError> {
        let name = fields.name.trimmingCharacters(in: .whitespaces)
        let type = fields.type
        let text = type == .string
            ? fields.value : fields.value.trimmingCharacters(in: .whitespaces)
        let value: Any?
        if text.isEmpty {
            value = nil
        } else if let parsed = Self.value(text, as: type) {
            value = parsed
        } else {
            return .failure(DoctoredCoreError(
                message: "“\(text)” is not \(ActionParameter.article(type)) \(type.rawValue)."))
        }
        guard let original = fields.original else {
            guard !name.isEmpty else { return .success([]) }
            return .success([.add(name: name, type: type, default: value,
                                  description: fields.description)])
        }
        if fields.remove { return .success([.remove(name: original.name)]) }
        var edits: [ActionParameterEdit] = []
        if type != original.type {
            edits.append(.retype(name: original.name, type: type, default: .some(value)))
        } else if value.map(Self.text) != original.default.map(Self.text) {
            edits.append(.setDefault(name: original.name, value: value))
        }
        if fields.description != (original.description ?? "") {
            edits.append(.setDescription(name: original.name, text: fields.description))
        }
        if name != original.name { edits.append(.rename(from: original.name, to: name)) }
        return .success(edits)
    }

    /// The field's text as a value of `type`, or nil when it is not one.
    private static func value(_ text: String, as type: ActionParameter.ParameterType) -> Any? {
        switch type {
        case .string: return text
        case .integer: return Int(text)
        case .number: return Double(text).flatMap { $0.isFinite ? $0 : nil }
        case .boolean: return ["true": true, "false": false][text.lowercased()]
        }
    }

    /// A default as the field shows it; a whole number without ".0".
    private static func text(_ value: Any) -> String {
        if ActionJSON.isBoolean(value), let flag = value as? Bool { return flag ? "true" : "false" }
        if let number = ActionJSON.number(value) {
            if let whole = Int(exactly: number.doubleValue) { return String(whole) }
            return String(number.doubleValue)
        }
        return "\(value)"
    }
}
