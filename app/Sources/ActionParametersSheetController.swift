import AppKit

/// Asks for an action's declared parameters before it plays — one field per
/// declaration, pre-filled with its default.
///
/// **Every Play path asks.** The Actions window's Play button, File ▸ Automate
/// ▸ Play Last, and the Batch window's Run all go through
/// `ActionParametersSheet.values(for:)`, so an action with parameters can
/// never be started from the UI with none of them bound — and an action that
/// declares none is never interrupted by a sheet, because `values(for:)`
/// answers immediately in that case.
///
/// **It does not validate; `ActionParameters.bind` does.** The sheet's job is
/// to collect text and check boxes; turning those into typed values, filling
/// in defaults and refusing what will not fit is the one validator every path
/// shares, and putting a second copy of those rules here is exactly how the
/// two would come to disagree. The message line shows `bind`'s own sentence.
final class ActionParametersSheet: NSObject {
    private let action: Action
    private var fields: [String: NSTextField] = [:]
    private var checkboxes: [String: NSButton] = [:]
    private let message = NSTextField(wrappingLabelWithString: "")
    private var modal: ModalCardWindowController?
    private var bound: [String: Any]?

    private init(action: Action) {
        self.action = action
        super.init()
    }

    /// The bound values for `action`, or nil when the user cancelled.
    ///
    /// An action with no declarations answers `[:]` without showing anything:
    /// a sheet with no fields in it would be a dialog whose only content was
    /// its own OK button.
    static func values(for action: Action) -> [String: Any]? {
        guard !action.parameters.isEmpty else { return [:] }
        return ActionParametersSheet(action: action).run()
    }

    private func run() -> [String: Any]? {
        var rows: [NSView] = []
        for parameter in action.parameters {
            let label = NSTextField(labelWithString: parameter.name)
            label.font = DS.sans(12)
            label.widthAnchor.constraint(equalToConstant: 120).isActive = true
            let control: NSView
            if parameter.type == .boolean {
                let box = NSButton(checkboxWithTitle: "", target: nil, action: nil)
                box.font = DS.sans(12)
                box.state = (parameter.default as? Bool) == true ? .on : .off
                box.setAccessibilityIdentifier("action-parameter-\(parameter.name)")
                checkboxes[parameter.name] = box
                control = box
            } else {
                let field = NSTextField()
                DSField.style(field)
                field.widthAnchor.constraint(equalToConstant: 220).isActive = true
                field.placeholderString = parameter.type.rawValue
                if let value = parameter.default {
                    field.stringValue = ActionParametersSheet.text(value)
                }
                field.setAccessibilityIdentifier("action-parameter-\(parameter.name)")
                fields[parameter.name] = field
                control = field
            }
            let row = NSStackView(views: [label, control])
            row.spacing = 8
            row.alignment = .firstBaseline
            rows.append(row)
            if let description = parameter.description, !description.isEmpty {
                let help = NSTextField(wrappingLabelWithString: description)
                help.font = DS.sans(11)
                help.textColor = DS.textMuted
                help.preferredMaxLayoutWidth = 340
                rows.append(help)
            }
        }
        message.font = DS.sans(11)
        message.textColor = DS.textMuted
        message.preferredMaxLayoutWidth = 340
        rows.append(message)

        let content = NSStackView(views: rows)
        content.orientation = .vertical
        content.alignment = .leading
        content.spacing = 10

        let cancel = sheetCancelButton(target: self, action: #selector(cancelClicked(_:)))
        let apply = StickerButton(
            title: "Run", style: .primary, target: self, action: #selector(runClicked(_:)))
        apply.setAccessibilityIdentifier("action-parameters-run")
        modal = ModalCardWindowController(
            title: "\(action.name) Parameters",
            card: makeSheetView(
                title: "\(action.name) Parameters",
                hint: "These values are bound to the action's {\"$param\": …} steps for this "
                    + "run only. A field left empty uses the parameter's default; one with "
                    + "no default has to be filled in.",
                content: content, buttonRow: makeButtonRow(cancel: cancel, apply: apply)))
        guard modal?.runModal() == .OK else { return nil }
        return bound
    }

    @objc private func cancelClicked(_ sender: Any?) { modal?.end(.cancel) }

    @objc private func runClicked(_ sender: Any?) {
        var supplied: [String: Any] = [:]
        for parameter in action.parameters {
            if let box = checkboxes[parameter.name] {
                supplied[parameter.name] = box.state == .on
                continue
            }
            guard let text = fields[parameter.name]?.stringValue
                .trimmingCharacters(in: .whitespaces), !text.isEmpty
            else { continue }
            switch parameter.type {
            case .string: supplied[parameter.name] = text
            case .integer:
                // Refused HERE only because "not a number at all" has no
                // typed value to hand `bind`; the range and the type rules
                // are still `bind`'s.
                guard let value = Int(text) else {
                    return refuse("\"\(parameter.name)\" must be a whole number.")
                }
                supplied[parameter.name] = value
            case .number:
                guard let value = Double(text), value.isFinite else {
                    return refuse("\"\(parameter.name)\" must be a number.")
                }
                supplied[parameter.name] = value
            case .boolean:
                // Unreachable: a boolean is a checkbox above.
                supplied[parameter.name] = text == "true"
            }
        }
        switch ActionParameters.bind(supplied, to: action) {
        case .success(let values):
            bound = values
            modal?.end(.OK)
        case .failure(let error):
            refuse(error.message)
        }
    }

    private func refuse(_ text: String) {
        message.stringValue = text
        NSSound.beep()
    }

    /// A default as the field shows it. Whole numbers lose the ".0" a Double
    /// would print, so an `integer` default of 4 reads as `4`.
    private static func text(_ value: Any) -> String {
        if let number = ActionJSON.number(value) {
            if let whole = Int(exactly: number.doubleValue) { return String(whole) }
            return String(number.doubleValue)
        }
        return "\(value)"
    }
}
