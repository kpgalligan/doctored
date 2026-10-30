import AppKit

// Levels/Threshold keep normalized Action values, Curves keep fractional historical 0…255
// coordinates.

/// Numeric controls store normalized doubles. Native codes are an explicit
/// presentation unit; switching units never changes a stored parameter.
final class FineToneControls: NSStackView {
    struct Parameter {
        let key: String
        let title: String
        let range: ClosedRange<Double>
        var isCode = true
    }
    let units = NSPopUpButton(frame: .zero, pullsDown: false)
    private let parameters: [Parameter]
    private let maximum: Double
    private let read: () -> [Double]
    private let write: ([Double]) -> Bool
    private var fields: [NSTextField] = []
    private var displayed: [String] = []
    private let status = NSTextField(labelWithString: "")

    init(id: String, native: Bool, parameters: [Parameter], read: @escaping () -> [Double],
         write: @escaping ([Double]) -> Bool) {
        self.parameters = parameters; maximum = native ? 65535 : 255
        self.read = read; self.write = write
        super.init(frame: .zero)
        orientation = .vertical; alignment = .leading; spacing = 5
        units.addItems(withTitles: ["Normalized", "Native codes (\(native ? 16 : 8)-bit)"])
        units.setAccessibilityIdentifier("tone.\(id).units")
        units.target = self; units.action = #selector(unitsChanged(_:))
        let heading = NSStackView(views: [fieldLabel("Fine entry:"), units])
        heading.spacing = 8
        addArrangedSubview(heading)
        var rows: [[NSView]] = []
        for (index, parameter) in parameters.enumerated() {
            let field = NSTextField(string: "")
            field.font = DS.mono(12); field.alignment = .right
            field.widthAnchor.constraint(equalToConstant: 190).isActive = true
            field.setAccessibilityIdentifier("tone.\(id).\(parameter.key)")
            field.setAccessibilityLabel(parameter.title)
            field.tag = index; field.target = self; field.action = #selector(fieldChanged(_:))
            field.cell?.sendsActionOnEndEditing = true
            fields.append(field)
            rows.append([fieldLabel(parameter.title + ":"), field])
        }
        addArrangedSubview(AdjustmentSheet.grid(rows))
        status.font = DS.sans(11); status.textColor = .systemRed
        status.setAccessibilityIdentifier("tone.\(id).error")
        addArrangedSubview(status)
        refresh()
    }

    @available(*, unavailable) required init?(coder: NSCoder) { fatalError("FineToneControls does not support NSCoder") }

    func refresh() {
        let values = read()
        for (index, field) in fields.enumerated() where values.indices.contains(index) {
            let factor = units.indexOfSelectedItem == 1 && parameters[index].isCode ? maximum : 1
            field.stringValue = String(values[index] * factor)
        }
        displayed = fields.map(\.stringValue)
    }

    @discardableResult func accept() -> Bool {
        if fields.map(\.stringValue) == displayed { status.stringValue = ""; return true }
        var values: [Double] = []
        let original = read()
        for (index, field) in fields.enumerated() {
            let factor = units.indexOfSelectedItem == 1 && parameters[index].isCode ? maximum : 1
            guard let value = Double(field.stringValue), value.isFinite,
                  parameters[index].range.contains(value / factor) else {
                status.stringValue = "Enter a finite value in the displayed range."
                return false
            }
            values.append(displayed.indices.contains(index) && field.stringValue == displayed[index] ? original[index] : value / factor)
        }
        guard write(values) else {
            status.stringValue = "Black / input points must remain below the next point."
            return false
        }
        status.stringValue = ""; refresh(); return true
    }

    @objc private func fieldChanged(_ sender: Any?) { _ = accept() }
    @objc private func unitsChanged(_ sender: Any?) { status.stringValue = ""; refresh() }

    static func forSliders(id: String, native: Bool, sliders: [NSSlider], changed: @escaping () -> Void) -> FineToneControls? {
        guard id == "threshold" && sliders.count == 1 else { return nil }
        return FineToneControls(id: id, native: native, parameters: [Parameter(key: "level", title: "Level", range: 0...1)],
                                read: { sliders.map(\.doubleValue) }, write: { values in
            for (index, value) in values.enumerated() { sliders[index].doubleValue = value }
            changed(); return true
        })
    }
}
