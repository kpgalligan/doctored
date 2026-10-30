import AppKit

/// Duotone Options' Curve… (#1 Duotone phase 1a): one ink's 13 fields — the output percent at
/// each input of `DuotoneOptions.curveInputs`, a blank field unset; 0 % and 100 % must be set (the
/// core refuses otherwise when the options are applied). The curve through the set fields is
/// Curves' natural spline (`duotone_ink`).
final class DuotoneCurveSheetController: NSViewController {
    private let inkName: String
    private let initial: [Double?]
    private let commit: ([Double?]) -> Void
    private var fields: [NSTextField] = []

    init(inkName: String, curve: [Double?], commit: @escaping ([Double?]) -> Void) {
        self.inkName = inkName
        initial = curve.count == DuotoneOptions.curveInputs.count ? curve : DuotoneInk.straightCurve
        self.commit = commit
        super.init(nibName: nil, bundle: nil)
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) {
        fatalError("DuotoneCurveSheetController does not support NSCoder")
    }

    override func loadView() {
        var cells: [[NSView]] = []
        for (index, input) in DuotoneOptions.curveInputs.enumerated() {
            let field = NSTextField(string: initial[index].map(DuotoneSheetController.format) ?? "")
            field.font = DS.sans(13)
            field.alignment = .right
            field.placeholderString = "–"
            field.widthAnchor.constraint(equalToConstant: 56).isActive = true
            field.setAccessibilityIdentifier("duotone-curve-\(input)")
            fields.append(field)
            cells.append([fieldLabel("\(input):"), field, fieldLabel("%")])
        }
        // Two columns of fields.
        let half = (cells.count + 1) / 2
        let left = NSGridView(views: Array(cells.prefix(half)))
        let right = NSGridView(views: Array(cells.dropFirst(half)))
        for grid in [left, right] {
            grid.rowSpacing = 8
            grid.columnSpacing = 6
            grid.column(at: 0).xPlacement = .trailing
        }
        let content = NSStackView(views: [left, right])
        content.alignment = .top
        content.spacing = 28
        let cancelButton = sheetCancelButton(target: self, action: #selector(cancelClicked(_:)))
        let applyButton = sheetApplyButton(target: self, action: #selector(applyClicked(_:)))
        applyButton.setAccessibilityIdentifier("duotone-curve-apply")
        view = makeSheetView(
            title: "Duotone Curve — \(inkName)",
            hint: "The ink's coverage in percent at each gray input (0 % is paper white, 100 % solid). "
                + "Blank fields are unset; the curve passes through the set ones.",
            content: content,
            buttonRow: makeButtonRow(cancel: cancelButton, apply: applyButton))
    }

    @objc private func applyClicked(_ sender: Any?) {
        var curve: [Double?] = []
        for field in fields {
            let text = field.stringValue.trimmingCharacters(in: .whitespaces)
            if text.isEmpty {
                curve.append(nil)
                continue
            }
            // A field that is not a number from 0 to 100 refuses here rather than becoming a
            // blank slot, so an entered curve point is never lost without a word.
            guard let value = Double(text), value.isFinite, (0...100).contains(value) else {
                NSSound.beep()
                view.window?.makeFirstResponder(field)
                field.selectText(nil)
                return
            }
            curve.append(value)
        }
        dismiss(self)
        commit(curve)
    }

    @objc private func cancelClicked(_ sender: Any?) {
        dismiss(self)
    }
}

/// Duotone Options' Overprint Colors… (#1 Duotone phase 1a): each combination of two or more
/// inks with its L*, a*, b* (D50) and swatch, and a Default checkbox: Default shows the colour the
/// view uses when nothing is authored (the inks' product over paper, `duotone_ink`) and authors
/// nothing. Overprint colours change the display only.
final class DuotoneOverprintSheetController: NSViewController {
    private let keys: [String]
    private let authored: [String: [Double]]
    private let effective: [String: [Double]]
    private let commit: ([String: [Double]]) -> Void
    private var rows: [(fields: [NSTextField], swatch: NSView, box: NSButton)] = []

    init(keys: [String], authored: [String: [Double]], effective: [String: [Double]],
         commit: @escaping ([String: [Double]]) -> Void) {
        self.keys = keys
        self.authored = authored
        self.effective = effective
        self.commit = commit
        super.init(nibName: nil, bundle: nil)
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) {
        fatalError("DuotoneOverprintSheetController does not support NSCoder")
    }

    override func loadView() {
        var cells: [[NSView]] = []
        for key in keys {
            let lab = authored[key] ?? effective[key] ?? [100, 0, 0]
            let fields = lab.map { DuotoneSheetController.numberField(($0 * 100).rounded() / 100) }
            for (field, axis) in zip(fields, ["l", "a", "b"]) {
                field.target = self
                field.action = #selector(fieldsChanged(_:))
                field.setAccessibilityIdentifier("duotone-overprint-\(key)-\(axis)")
            }
            let box = NSButton(checkboxWithTitle: "Default", target: self, action: #selector(fieldsChanged(_:)))
            box.font = DS.sans(13)
            box.state = authored[key] == nil ? .on : .off
            box.setAccessibilityIdentifier("duotone-overprint-\(key)-default")
            let swatch = DuotoneSheetController.swatch()
            rows.append((fields, swatch, box))
            let line = NSStackView(views: fields + [swatch, box])
            line.spacing = 6
            cells.append([fieldLabel("\(key):"), line])
        }
        let grid = NSGridView(views: cells)
        grid.rowSpacing = 8
        grid.columnSpacing = 10
        grid.column(at: 0).xPlacement = .trailing
        let cancelButton = sheetCancelButton(target: self, action: #selector(cancelClicked(_:)))
        let applyButton = sheetApplyButton(target: self, action: #selector(applyClicked(_:)))
        applyButton.setAccessibilityIdentifier("duotone-overprint-apply")
        syncControls()
        view = makeSheetView(
            title: "Overprint Colors",
            hint: "The colour each combination of inks shows where they print over each other, as "
                + "L*, a*, b* (D50). Default is the inks' product over paper. These change the "
                + "display only.",
            content: grid,
            buttonRow: makeButtonRow(cancel: cancelButton, apply: applyButton))
    }

    private func syncControls() {
        for (index, row) in rows.enumerated() {
            let isDefault = row.box.state == .on
            row.fields.forEach { $0.isEnabled = !isDefault }
            if isDefault, let lab = effective[keys[index]] {
                for (field, value) in zip(row.fields, lab) {
                    field.stringValue = DuotoneSheetController.format((value * 100).rounded() / 100)
                }
            }
            row.swatch.layer?.backgroundColor =
                DuotoneSheetController.swatchColor(row.fields.map { $0.doubleValue })
        }
    }

    @objc private func fieldsChanged(_ sender: Any?) {
        syncControls()
    }

    @objc private func applyClicked(_ sender: Any?) {
        var chosen: [String: [Double]] = [:]
        for (key, row) in zip(keys, rows) where row.box.state == .off {
            var lab: [Double] = []
            for field in row.fields {
                // A field that is not a number refuses here, as the Curve sheet's do (#1
                // Duotone phase 1b), rather than becoming 0.
                let text = field.stringValue.trimmingCharacters(in: .whitespaces)
                guard let value = Double(text), value.isFinite else {
                    NSSound.beep()
                    view.window?.makeFirstResponder(field)
                    field.selectText(nil)
                    return
                }
                lab.append(value)
            }
            chosen[key] = lab
        }
        dismiss(self)
        commit(chosen)
    }

    @objc private func cancelClicked(_ sender: Any?) {
        dismiss(self)
    }
}
