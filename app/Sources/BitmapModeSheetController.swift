import AppKit

/// Image ▸ Mode ▸ Bitmap… (#547 Bitmap phase 1a). Resolution: the document's
/// Input (its horizontal ppi) and the Output resolution with its unit (Pixels/Inch or
/// Pixels/Centimeter, sent to the core in pixels per inch); while the field still holds the
/// document's own value no `resolution` is sent, so each axis keeps its pixels and ppi (an
/// anisotropic document is untouched). Method: 50% Threshold, Pattern Dither, Diffusion Dither
/// (the default), Halftone Screen… and Custom Pattern (#547 Bitmap phase 1b). Halftone Screen's
/// Frequency (Lines/Inch or Lines/Centimeter, sent in lines per inch), Angle and Shape, and Custom
/// Pattern's Pattern (the pattern presets every Pattern Overlay offers, sent as `{"preset": id}`),
/// are enabled only under their own method and send nothing otherwise. Apply asks the core's plan
/// first: a refusal other than the one-layer rule beeps and shows its sentence here; the
/// conversion, its "Flatten layers?" question and its one undo step are
/// `EditorViewController+BitmapMode.swift`'s.
final class BitmapModeSheetController: NSViewController {
    private weak var editor: EditorViewController?
    private weak var document: ImageDocument?

    /// The methods in menu order, with their wire names.
    private static let methods: [(title: String, name: String)] = [
        ("50% Threshold", "threshold"), ("Pattern Dither", "pattern"),
        ("Diffusion Dither", "diffusion"), ("Halftone Screen…", "halftone"),
        ("Custom Pattern", "custom"),
    ]
    /// The Output resolution's units: their titles and pixels per inch of one unit's value.
    private static let units: [(title: String, perInch: Double)] = [
        ("Pixels/Inch", 1), ("Pixels/Centimeter", 2.54),
    ]
    /// Halftone Screen's frequency units: their titles and lines per inch of one unit's value.
    private static let lineUnits: [(title: String, perInch: Double)] = [
        ("Lines/Inch", 1), ("Lines/Centimeter", 2.54),
    ]
    /// Halftone Screen's shapes in menu order, with their wire names.
    private static let shapes: [(title: String, name: String)] = [
        ("Round", "round"), ("Ellipse", "ellipse"), ("Line", "line"), ("Square", "square"),
        ("Cross", "cross"), ("Diamond", "diamond"),
    ]

    private let outputField = NSTextField(string: "")
    private let unitPopup = NSPopUpButton(frame: .zero, pullsDown: false)
    private let methodPopup = NSPopUpButton(frame: .zero, pullsDown: false)
    private let frequencyField = NSTextField(string: "53")
    private let lineUnitPopup = NSPopUpButton(frame: .zero, pullsDown: false)
    private let angleField = NSTextField(string: "45")
    private let shapePopup = NSPopUpButton(frame: .zero, pullsDown: false)
    private let patternPopup = NSPopUpButton(frame: .zero, pullsDown: false)
    private let patterns = RasterDocument.patternPresets()
    private let note = NSTextField(wrappingLabelWithString: "")
    private var applyButton: NSButton!
    /// The unit the field's value is currently in.
    private var unitIndex = 0

    init(editor: EditorViewController, document: ImageDocument) {
        self.editor = editor
        self.document = document
        super.init(nibName: nil, bundle: nil)
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) {
        fatalError("BitmapModeSheetController does not support NSCoder")
    }

    /// The document's horizontal ppi (the dialog's Input).
    private var inputPPI: Double { document?.doc?.resolution.x ?? 72 }

    /// `ppi` in the unit at `index`, as the field shows it.
    private static func text(ppi: Double, unit index: Int) -> String {
        let value = ppi / units[index].perInch
        let rounded = (value * 100).rounded() / 100
        return rounded == rounded.rounded() ? String(Int(rounded)) : String(format: "%.2f", rounded)
    }

    override func loadView() {
        let input = fieldLabel(Self.text(ppi: inputPPI, unit: 0) + " Pixels/Inch")
        input.setAccessibilityIdentifier("bitmap-mode-input")
        outputField.stringValue = Self.text(ppi: inputPPI, unit: 0)
        outputField.font = DS.sans(13)
        outputField.alignment = .right
        outputField.widthAnchor.constraint(equalToConstant: 80).isActive = true
        outputField.setAccessibilityIdentifier("bitmap-mode-output")
        unitPopup.addItems(withTitles: Self.units.map(\.title))
        unitPopup.font = DS.sans(13)
        unitPopup.target = self
        unitPopup.action = #selector(unitChanged(_:))
        unitPopup.setAccessibilityIdentifier("bitmap-mode-unit")
        methodPopup.addItems(withTitles: Self.methods.map(\.title))
        methodPopup.font = DS.sans(13)
        methodPopup.selectItem(at: Self.methods.firstIndex { $0.name == "diffusion" } ?? 0)
        methodPopup.target = self
        methodPopup.action = #selector(methodChanged(_:))
        methodPopup.setAccessibilityIdentifier("bitmap-mode-method")
        for (field, identifier) in [(frequencyField, "bitmap-mode-frequency"),
                                    (angleField, "bitmap-mode-angle")] {
            field.font = DS.sans(13)
            field.alignment = .right
            field.widthAnchor.constraint(equalToConstant: 80).isActive = true
            field.setAccessibilityIdentifier(identifier)
        }
        lineUnitPopup.addItems(withTitles: Self.lineUnits.map(\.title))
        shapePopup.addItems(withTitles: Self.shapes.map(\.title))
        patternPopup.addItems(withTitles: patterns.map(\.name))
        for (popup, identifier) in [(lineUnitPopup, "bitmap-mode-line-unit"),
                                    (shapePopup, "bitmap-mode-shape"),
                                    (patternPopup, "bitmap-mode-pattern")] {
            popup.font = DS.sans(13)
            popup.setAccessibilityIdentifier(identifier)
        }
        note.font = DS.sans(12)
        note.textColor = DS.textMuted
        note.preferredMaxLayoutWidth = sheetWidth - sheetInset * 2
        note.stringValue = "50% Threshold makes gray 128 and above white; Pattern Dither is an 8 × 8 "
            + "ordered dither measured against Photoshop's; Diffusion Dither is Sierra Lite error "
            + "diffusion onto black and white; "
            + "Halftone Screen lays a screen of Adobe's spot shapes over the output, Doctored's own "
            + "and not measured against Photoshop's; Custom Pattern "
            + "makes a pixel white where its gray is above the pattern's. A different Output "
            + "resolution resamples the gray (Bicubic) first."
        note.setAccessibilityIdentifier("bitmap-mode-note")

        let outputRow = NSStackView(views: [outputField, unitPopup])
        outputRow.spacing = 8
        let frequencyRow = NSStackView(views: [frequencyField, lineUnitPopup])
        frequencyRow.spacing = 8
        let angleRow = NSStackView(views: [angleField, fieldLabel("°")])
        angleRow.spacing = 4
        let grid = NSGridView(views: [
            [fieldLabel("Input:"), input],
            [fieldLabel("Output:"), outputRow],
            [fieldLabel("Method:"), methodPopup],
            [fieldLabel("Frequency:"), frequencyRow],
            [fieldLabel("Angle:"), angleRow],
            [fieldLabel("Shape:"), shapePopup],
            [fieldLabel("Pattern:"), patternPopup],
        ])
        grid.rowSpacing = 10
        grid.columnSpacing = 12
        grid.column(at: 0).xPlacement = .trailing
        grid.column(at: 0).width = 106
        let content = NSStackView(views: [grid, note])
        content.orientation = .vertical
        content.alignment = .leading
        content.spacing = 12

        let cancelButton = sheetCancelButton(target: self, action: #selector(cancelClicked(_:)))
        cancelButton.setAccessibilityIdentifier("bitmap-mode-cancel")
        applyButton = sheetApplyButton(target: self, action: #selector(applyClicked(_:)))
        applyButton.setAccessibilityIdentifier("bitmap-mode-apply")
        view = makeSheetView(
            title: "Bitmap",
            hint: "Converts the 8-bit Grayscale image to one layer of black and white pixels; "
                + "the 1-bit plane is kept in the document.",
            content: content,
            buttonRow: makeButtonRow(cancel: cancelButton, apply: applyButton,
                                     leading: [sheetFootnote("one undo step")]))
        methodChanged(nil)
    }

    /// A field's finite number, or nil.
    private static func number(_ field: NSTextField) -> Double? {
        let text = field.stringValue.trimmingCharacters(in: .whitespaces)
        return Double(text).flatMap { $0.isFinite ? $0 : nil }
    }

    /// The selected method's wire name.
    private var methodName: String {
        let index = methodPopup.indexOfSelectedItem
        return Self.methods.indices.contains(index) ? Self.methods[index].name : "diffusion"
    }

    /// Enables Halftone Screen's rows only under Halftone Screen and the Pattern row only under
    /// Custom Pattern.
    @objc private func methodChanged(_ sender: Any?) {
        let halftone = methodName == "halftone"
        for control in [frequencyField, lineUnitPopup, angleField, shapePopup] as [NSControl] {
            control.isEnabled = halftone
        }
        patternPopup.isEnabled = methodName == "custom" && !patterns.isEmpty
    }

    /// Converts the field's value to the newly chosen unit.
    @objc private func unitChanged(_ sender: Any?) {
        let index = unitPopup.indexOfSelectedItem
        guard Self.units.indices.contains(index), index != unitIndex else { return }
        let unchanged = outputField.stringValue == Self.text(ppi: inputPPI, unit: unitIndex)
        if unchanged {
            outputField.stringValue = Self.text(ppi: inputPPI, unit: index)
        } else if let value = Double(outputField.stringValue) {
            outputField.stringValue = Self.text(ppi: value * Self.units[unitIndex].perInch, unit: index)
        }
        unitIndex = index
    }

    /// The core's options for the current settings (`bitmap_convert`), without Flatten; nil with
    /// the reason in the note when the Output field is not a resolution or a Halftone field not a
    /// number. The core checks every range.
    private func coreOptions() -> [String: Any]? {
        let method = methodName
        var options: [String: Any] = ["method": method]
        if method == "halftone" {
            let unit = Self.lineUnits.indices.contains(lineUnitPopup.indexOfSelectedItem)
                ? Self.lineUnits[lineUnitPopup.indexOfSelectedItem].perInch : 1
            guard let frequency = Self.number(frequencyField) else {
                note.stringValue = "Frequency must be a number of lines."
                return nil
            }
            guard let angle = Self.number(angleField) else {
                note.stringValue = "Angle must be a number of degrees from -180 to 180."
                return nil
            }
            options["frequency"] = frequency * unit
            options["angle"] = angle
            let shape = shapePopup.indexOfSelectedItem
            options["shape"] = Self.shapes.indices.contains(shape) ? Self.shapes[shape].name : "round"
        }
        if method == "custom", patterns.indices.contains(patternPopup.indexOfSelectedItem) {
            options["pattern"] = ["preset": patterns[patternPopup.indexOfSelectedItem].id]
        }
        let text = outputField.stringValue.trimmingCharacters(in: .whitespaces)
        // The document's own value sends no resolution: each axis keeps its pixels and ppi.
        if text == Self.text(ppi: inputPPI, unit: unitIndex) { return options }
        guard let value = Double(text), value.isFinite, value > 0 else {
            note.stringValue = "Output must be a resolution from 1 to 30000 pixels per inch."
            return nil
        }
        options["resolution"] = value * Self.units[unitIndex].perInch
        return options
    }

    @objc private func applyClicked(_ sender: Any?) {
        guard let document, let editor, let doc = document.doc else { NSSound.beep(); return }
        guard let options = coreOptions() else { NSSound.beep(); return }
        // The plan, so a refusal stays in the sheet; a flatten the conversion asks about itself.
        do {
            let plan = try CmykModeConversion.plan(source: doc, target: .bitmap, profile: nil,
                                                   options: CmykConversionOptions(), bitmap: options)
            if let refusal = plan["refusal"] as? String, plan["flatten_required"] as? Bool != true {
                NSSound.beep()
                note.stringValue = refusal
                return
            }
        } catch {
            NSSound.beep()
            note.stringValue = error.localizedDescription
            return
        }
        dismiss(self)
        editor.commitBitmap(document: document, options: options, flatten: false)
    }

    @objc private func cancelClicked(_ sender: Any?) {
        dismiss(self)
    }
}

/// Image ▸ Mode ▸ Grayscale on a Bitmap document (#547 Bitmap phase 1a): the Size Ratio,
/// an integer 1…16 (1 keeps the canvas; r averages each r × r block onto a canvas 1/r the size).
/// The conversion and its one undo step are `EditorViewController+BitmapMode.swift`'s.
final class BitmapToGraySheetController: NSViewController {
    private weak var editor: EditorViewController?
    private weak var document: ImageDocument?
    private let ratioField = NSTextField(string: "1")
    private let note = NSTextField(wrappingLabelWithString: "")

    init(editor: EditorViewController, document: ImageDocument) {
        self.editor = editor
        self.document = document
        super.init(nibName: nil, bundle: nil)
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) {
        fatalError("BitmapToGraySheetController does not support NSCoder")
    }

    override func loadView() {
        ratioField.font = DS.sans(13)
        ratioField.alignment = .right
        ratioField.widthAnchor.constraint(equalToConstant: 64).isActive = true
        ratioField.setAccessibilityIdentifier("bitmap-gray-ratio")
        note.font = DS.sans(12)
        note.textColor = DS.textMuted
        note.preferredMaxLayoutWidth = sheetWidth - sheetInset * 2
        note.stringValue = "1 keeps every pixel; a ratio r averages each r × r block of black and white "
            + "into one gray pixel."
        note.setAccessibilityIdentifier("bitmap-gray-note")
        let grid = NSGridView(views: [[fieldLabel("Size Ratio:"), ratioField]])
        grid.columnSpacing = 12
        grid.column(at: 0).xPlacement = .trailing
        grid.column(at: 0).width = 106
        let content = NSStackView(views: [grid, note])
        content.orientation = .vertical
        content.alignment = .leading
        content.spacing = 12
        let cancelButton = sheetCancelButton(target: self, action: #selector(cancelClicked(_:)))
        cancelButton.setAccessibilityIdentifier("bitmap-gray-cancel")
        let applyButton = sheetApplyButton(target: self, action: #selector(applyClicked(_:)))
        applyButton.setAccessibilityIdentifier("bitmap-gray-apply")
        view = makeSheetView(
            title: "Grayscale",
            hint: "Converts the Bitmap document back to 8-bit Grayscale in its own Gray profile.",
            content: content,
            buttonRow: makeButtonRow(cancel: cancelButton, apply: applyButton,
                                     leading: [sheetFootnote("one undo step")]))
    }

    @objc private func applyClicked(_ sender: Any?) {
        guard let document, let editor else { NSSound.beep(); return }
        let text = ratioField.stringValue.trimmingCharacters(in: .whitespaces)
        guard let ratio = Int(text), (1...16).contains(ratio) else {
            NSSound.beep()
            note.stringValue = "Size Ratio must be an integer from 1 to 16."
            return
        }
        dismiss(self)
        editor.commitBitmapToGray(document: document, sizeRatio: ratio)
    }

    @objc private func cancelClicked(_ sender: Any?) {
        dismiss(self)
    }
}
