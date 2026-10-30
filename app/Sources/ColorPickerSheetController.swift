import AppKit

/// The picker's C/M/Y/K fields (#547 phase 2). In a CMYK document (`isCmyk`) they set the ink
/// EXACTLY — the swatch keeps it (`setPaintInk`) and the HSB, RGB, Lab and hex fields show its sRGB
/// through the document's profile — and an RGB-side edit clears it. Beside an RGB document they show
/// the working CMYK equivalent of the colour and editing them converts back to sRGB; no ink is kept.
/// `document` is the CMYK document whose profile converts (nil or RGB: the working CMYK profile).
struct PickerCmykContext {
    let document: RasterPrecisionDocument?
    let isCmyk: Bool
    let ink: CmykInk?
}

/// The rail swatches' Color Picker: the saturation/brightness
/// field and hue strip, the new/current swatch, and HSB, RGB, Lab and hex
/// fields that all edit ONE `PickerColor`. At 32 bits the options bar's
/// Text and Shape swatches open it too (`presentSwatchPicker`);
/// `title` names the swatch.
///
/// No document edit: the pair and the tool options are app-wide
/// (`ToolOptionsStore`),
/// so there is no undo step, no Action step and no MCP twin (`app/CLAUDE.md`,
/// "View preferences"). OK hands `onCommit` an AUTHORED sRGB colour — and
/// only when the colour was changed, so an eyedropped document-space colour
/// opened and OK'd untouched stays exactly what it was.
///
/// At 32 bits (`intensity` non-nil) the colour is the SDR base of an
/// `ExtendedRangeColor.split` and an Intensity field (stops) sits below the
/// hex row; OK hands `onCommit` the base × 2^stops in extended linear sRGB
/// (`ExtendedRangeColor.combined`) when the colour or the intensity
/// changed.
final class ColorPickerSheetController: NSViewController {
    /// The sheet's title, naming the swatch it edits.
    private let sheetTitle: String
    private let initial: PickerColor
    private var color: PickerColor
    private let originalColor: NSColor
    private let onCommit: (NSColor, CmykInk?) -> Void
    private let onSystemColors: () -> Void
    /// nil: the SDR sheet. Otherwise the stops the sheet opened with.
    private let initialIntensity: Double?
    private var intensity: Double
    private let intensityField = NSTextField(string: "")
    /// What the Intensity field last showed (`shown`'s rule).
    private var shownIntensity = ""

    private let field = SaturationBrightnessField()
    private let hueStrip = HueStrip()
    private var swatch: ColorCompareSwatch!

    private let hueField = NSTextField(string: "")
    private let saturationField = NSTextField(string: "")
    private let brightnessField = NSTextField(string: "")
    private let redField = NSTextField(string: "")
    private let greenField = NSTextField(string: "")
    private let blueField = NSTextField(string: "")
    private let lightnessField = NSTextField(string: "")
    private let labAField = NSTextField(string: "")
    private let labBField = NSTextField(string: "")
    private let hexField = NSTextField(string: "")
    private let gamutNote = NSTextField(
        wrappingLabelWithString: "Out of sRGB gamut — clamped to the nearest sRGB colour")
    /// #547 phase 5: the out-of-gamut alarm — a ⚠ and the nearest printable colour's
    /// swatch (click: take it) when the colour falls outside the CMYK profile's gamut (the
    /// document's own, else the working CMYK).
    private let cmykAlarm = NSTextField(labelWithString: "⚠")
    private let inGamutSwatch = NSButton(title: "", target: nil, action: nil)
    private var inGamut: String?

    /// What each numeric field last SHOWED. A field's action is a no-op when
    /// its parsed value equals this, so focusing a field and pressing OK
    /// never re-derives the codes from a rounded HSB or Lab integer.
    private var shown: [NSTextField: Int] = [:]
    private var shownHex = ""

    /// #547: the C/M/Y/K fields' context; nil hides them (32 bits, or no profile to convert with).
    private let cmyk: PickerCmykContext?
    private let cyanField = NSTextField(string: "")
    private let magentaField = NSTextField(string: "")
    private let yellowField = NSTextField(string: "")
    private let blackInkField = NSTextField(string: "")
    /// What the C/M/Y/K fields show; nil when the colour cannot be converted.
    private var ink: CmykInk?
    /// A CMYK document's exact ink, kept until an RGB-side edit clears it.
    private var authoredInk: CmykInk?
    private let initialInk: CmykInk?

    init(title: String, color: NSColor, intensity: Double?, cmyk: PickerCmykContext?,
         onCommit: @escaping (NSColor, CmykInk?) -> Void, onSystemColors: @escaping () -> Void)
    {
        sheetTitle = title
        originalColor = color
        initial = PickerColor(color)
        self.color = initial
        initialIntensity = intensity
        self.intensity = intensity ?? 0
        self.onCommit = onCommit
        self.onSystemColors = onSystemColors
        let stored = cmyk?.isCmyk == true ? cmyk?.ink : nil
        authoredInk = stored
        initialInk = stored
        ink = stored ?? cmyk.flatMap { Self.ink(of: PickerColor(color), in: $0) }
        // No profile to convert with (an RGB document and no working CMYK profile): no fields.
        self.cmyk = ink == nil ? nil : cmyk
        super.init(nibName: nil, bundle: nil)
    }

    /// `color`'s ink through the context's profile; nil when it cannot convert.
    private static func ink(of color: PickerColor, in context: PickerCmykContext) -> CmykInk? {
        try? RasterPrecisionDocument.cmykInk(
            fromSRGB: [color.red, color.green, color.blue].map { Double($0) / 255 },
            document: context.document)
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) {
        fatalError("ColorPickerSheetController does not support NSCoder")
    }

    override func loadView() {
        field.onChange = { [weak self] in self?.fieldMoved(saturation: $0, brightness: $1) }
        hueStrip.onChange = { [weak self] in self?.hueMoved($0) }
        swatch = ColorCompareSwatch(newColor: color.nsColor, currentColor: originalColor)

        configure(hueField, minimum: 0, maximum: 360)
        configure(saturationField, minimum: 0, maximum: 100)
        configure(brightnessField, minimum: 0, maximum: 100)
        for rgb in [redField, greenField, blueField] { configure(rgb, minimum: 0, maximum: 255) }
        configure(lightnessField, minimum: 0, maximum: 100)
        configure(labAField, minimum: -128, maximum: 127)
        configure(labBField, minimum: -128, maximum: 127)
        DSField.style(hexField)
        hexField.target = self
        hexField.action = #selector(hexCommitted(_:))
        hexField.cell?.sendsActionOnEndEditing = true
        hexField.widthAnchor.constraint(equalToConstant: 96).isActive = true

        let empty = { NSGridCell.emptyContentView }
        var rows: [[NSView]] = [
            [label("H"), hueField, unit("°"), label("L"), lightnessField],
            [label("S"), saturationField, unit("%"), label("a"), labAField],
            [label("B"), brightnessField, unit("%"), label("b"), labBField],
            [label("R"), redField, empty(), empty(), empty()],
            [label("G"), greenField, empty(), empty(), empty()],
            [label("B"), blueField, empty(), empty(), empty()],
            [label("#"), hexField, empty(), empty(), empty()],
        ]
        if cmyk != nil {
            // #547: C/M/Y/K beside R/G/B/#, percent.
            for (row, (name, ink)) in [("C", cyanField), ("M", magentaField), ("Y", yellowField),
                                        ("K", blackInkField)].enumerated() {
                configure(ink, minimum: 0, maximum: 100)
                ink.action = #selector(inkCommitted(_:))
                rows[3 + row][3] = label(name)
                rows[3 + row][4] = ink
            }
            cyanField.setAccessibilityIdentifier("picker-cmyk-c")
            magentaField.setAccessibilityIdentifier("picker-cmyk-m")
            yellowField.setAccessibilityIdentifier("picker-cmyk-y")
            blackInkField.setAccessibilityIdentifier("picker-cmyk-k")
        }
        if initialIntensity != nil {
            configureIntensity()
            rows.append([label("Intensity"), intensityField, unit("stops"), empty(), empty()])
        }
        let grid = NSGridView(views: rows)
        grid.rowSpacing = 6
        grid.columnSpacing = 6
        grid.column(at: 0).xPlacement = .trailing
        grid.column(at: 3).xPlacement = .trailing
        grid.mergeCells(inHorizontalRange: NSRange(location: 1, length: cmyk == nil ? 4 : 2),
                        verticalRange: NSRange(location: 6, length: 1))

        gamutNote.font = DS.sans(11)
        gamutNote.textColor = DS.textMuted
        gamutNote.preferredMaxLayoutWidth = 200

        let swatchColumn = NSStackView(views: [caption("new"), swatch, caption("current")])
        swatchColumn.orientation = .vertical
        swatchColumn.alignment = .centerX
        swatchColumn.spacing = 3

        cmykAlarm.font = DS.sans(14)
        cmykAlarm.toolTip = "Out of the CMYK gamut: click the swatch for the nearest printable colour"
        cmykAlarm.setAccessibilityIdentifier("picker-cmyk-gamut-alarm")
        inGamutSwatch.isBordered = false
        inGamutSwatch.wantsLayer = true
        inGamutSwatch.layer?.cornerRadius = 3
        inGamutSwatch.layer?.borderWidth = 1
        inGamutSwatch.layer?.borderColor = DS.border.cgColor
        inGamutSwatch.target = self
        inGamutSwatch.action = #selector(inGamutClicked(_:))
        inGamutSwatch.toolTip = "Bring into the CMYK gamut"
        inGamutSwatch.setAccessibilityIdentifier("picker-cmyk-in-gamut")
        NSLayoutConstraint.activate([
            inGamutSwatch.widthAnchor.constraint(equalToConstant: 22),
            inGamutSwatch.heightAnchor.constraint(equalToConstant: 16),
        ])
        let alarmRow = NSStackView(views: [cmykAlarm, inGamutSwatch])
        alarmRow.orientation = .horizontal
        alarmRow.spacing = 4
        let swatchRow = NSStackView(views: [swatchColumn, alarmRow])
        swatchRow.orientation = .horizontal
        swatchRow.alignment = .centerY
        swatchRow.spacing = 8
        let controls = NSStackView(views: [swatchRow, grid, gamutNote])
        controls.orientation = .vertical
        controls.alignment = .leading
        controls.spacing = 12
        controls.detachesHiddenViews = false

        let content = NSStackView(views: [field, hueStrip, controls])
        content.orientation = .horizontal
        content.alignment = .top
        content.spacing = 8
        content.setCustomSpacing(18, after: hueStrip)

        let cancelButton = sheetCancelButton(target: self, action: #selector(cancelClicked(_:)))
        let okButton = StickerButton(title: "OK", style: .primary, target: self,
                                     action: #selector(okClicked(_:)))
        let systemColors = GhostButton(
            symbol: "paintpalette", fallback: "◐", caption: nil, tooltip: "System Colors…",
            action: #selector(systemColorsClicked(_:)))
        systemColors.target = self
        view = makeSheetView(
            title: sheetTitle,
            content: content,
            buttonRow: makeButtonRow(cancel: cancelButton, apply: okButton,
                                     leading: [systemColors]),
            width: 560)
        refresh()
    }

    // MARK: - Building

    private func configure(_ numeric: NSTextField, minimum: Int, maximum: Int) {
        let formatter = NumberFormatter()
        formatter.numberStyle = .none
        formatter.allowsFloats = false
        formatter.minimum = NSNumber(value: minimum)
        formatter.maximum = NSNumber(value: maximum)
        numeric.formatter = formatter
        numeric.target = self
        numeric.action = #selector(numberCommitted(_:))
        numeric.cell?.sendsActionOnEndEditing = true
        numeric.alignment = .right
        numeric.widthAnchor.constraint(equalToConstant: 48).isActive = true
        DSField.style(numeric)
    }

    /// The Intensity field: stops with two decimals, in
    /// `ExtendedRangeColor.intensityRange`.
    private func configureIntensity() {
        let formatter = NumberFormatter()
        formatter.numberStyle = .decimal
        formatter.minimumFractionDigits = 2
        formatter.maximumFractionDigits = 2
        formatter.minimum = NSNumber(value: ExtendedRangeColor.intensityRange.lowerBound)
        formatter.maximum = NSNumber(value: ExtendedRangeColor.intensityRange.upperBound)
        intensityField.formatter = formatter
        intensityField.target = self
        intensityField.action = #selector(intensityCommitted(_:))
        intensityField.cell?.sendsActionOnEndEditing = true
        intensityField.alignment = .right
        intensityField.widthAnchor.constraint(equalToConstant: 60).isActive = true
        DSField.style(intensityField)
    }

    private func label(_ text: String) -> NSTextField {
        let label = NSTextField(labelWithString: text)
        label.font = DS.sans(13)
        label.textColor = DS.textStrong
        return label
    }

    private func unit(_ text: String) -> NSTextField {
        let label = NSTextField(labelWithString: text)
        label.font = DS.sans(12)
        label.textColor = DS.textMuted
        return label
    }

    private func caption(_ text: String) -> NSTextField {
        let label = NSTextField(labelWithString: text)
        label.font = DS.sans(10)
        label.textColor = DS.textMuted
        return label
    }

    // MARK: - Display

    /// Every control from `color` — the edited field's group included, so a
    /// Lab entry outside sRGB shows the clamped colour's Lab.
    private func refresh() {
        show(hueField, Int(color.hue.rounded()) % 360)
        show(saturationField, Int((color.saturation * 100).rounded()))
        show(brightnessField, Int((color.brightness * 100).rounded()))
        show(redField, color.red)
        show(greenField, color.green)
        show(blueField, color.blue)
        if let lab = color.lab {
            show(lightnessField, Int(lab.l.rounded()))
            show(labAField, Int(lab.a.rounded()))
            show(labBField, Int(lab.b.rounded()))
        } else {
            for unreadable in [lightnessField, labAField, labBField] {
                unreadable.stringValue = ""
                shown[unreadable] = nil
            }
        }
        shownHex = color.hex
        hexField.stringValue = shownHex
        field.hue = color.hue
        field.saturation = color.saturation
        field.brightness = color.brightness
        hueStrip.hue = color.hue
        swatch.newColor = color.nsColor
        gamutNote.isHidden = !color.outOfGamut
        if initialIntensity != nil {
            intensityField.objectValue = NSNumber(value: intensity)
            shownIntensity = intensityField.stringValue
        }
        if cmyk != nil, let ink {
            for (field, value) in zip([cyanField, magentaField, yellowField, blackInkField], ink.rounded) {
                show(field, value)
            }
        }
        refreshCmykAlarm()
    }

    /// The CMYK gamut alarm for the current colour (`dr_cmyk_separate`, sRGB through the
    /// document's CMYK profile or the working CMYK). Hidden at 32 bits and with no profile.
    private func refreshCmykAlarm() {
        guard let cmyk else {
            cmykAlarm.isHidden = true
            inGamutSwatch.isHidden = true
            return
        }
        let separation = try? RasterPrecisionDocument.separate(
            rgb: [color.red, color.green, color.blue].map { Double($0) / 255 }, rgbProfile: nil,
            document: cmyk.isCmyk ? cmyk.document : nil)
        let alarm = separation?.outOfGamut == true
        cmykAlarm.isHidden = !alarm
        inGamutSwatch.isHidden = !alarm
        guard alarm, let rgb = separation?.inGamut, rgb.count == 3 else { inGamut = nil; return }
        let codes = rgb.map { Int((min(max($0, 0), 1) * 255).rounded()) }
        inGamut = String(format: "%02X%02X%02X", codes[0], codes[1], codes[2])
        inGamutSwatch.layer?.backgroundColor = NSColor(
            srgbRed: CGFloat(codes[0]) / 255, green: CGFloat(codes[1]) / 255,
            blue: CGFloat(codes[2]) / 255, alpha: 1).cgColor
    }

    /// The alarm's swatch: take the nearest printable colour.
    @objc private func inGamutClicked(_ sender: Any?) {
        guard let inGamut, color.setHex(inGamut) else { NSSound.beep(); return }
        inkFollowsColor()
        refresh()
    }

    /// An RGB-side edit (field, strip, numbers, hex): the ink follows the colour again, so a
    /// CMYK document's exact ink is cleared (#547).
    private func inkFollowsColor() {
        guard let cmyk else { return }
        authoredInk = nil
        ink = Self.ink(of: color, in: cmyk) ?? ink
    }

    /// A C/M/Y/K field committed: the ink as typed (exact in a CMYK document), the colour its
    /// sRGB through the profile.
    @objc private func inkCommitted(_ sender: NSTextField) {
        guard let cmyk, var edited = ink, let number = sender.objectValue as? NSNumber else {
            refresh()
            return
        }
        let value = number.intValue
        guard value != shown[sender] else {
            refresh()
            return
        }
        switch sender {
        case cyanField: edited.c = Double(value)
        case magentaField: edited.m = Double(value)
        case yellowField: edited.y = Double(value)
        default: edited.k = Double(value)
        }
        guard let rgb = try? RasterPrecisionDocument.srgb(fromInk: edited, document: cmyk.document),
              rgb.count == 3 else {
            NSSound.beep()
            refresh()
            return
        }
        let code = { (v: Double) in Int((min(max(v, 0), 1) * 255).rounded()) }
        color.setRGB(r: code(rgb[0]), g: code(rgb[1]), b: code(rgb[2]))
        ink = edited
        if cmyk.isCmyk { authoredInk = edited }
        refresh()
    }

    private func show(_ numeric: NSTextField, _ value: Int) {
        numeric.integerValue = value
        shown[numeric] = value
    }

    // MARK: - Edits

    private func fieldMoved(saturation: Double, brightness: Double) {
        color.setHSB(h: color.hue, s: saturation, b: brightness)
        inkFollowsColor()
        refresh()
    }

    private func hueMoved(_ hue: Double) {
        color.setHSB(h: hue, s: color.saturation, b: color.brightness)
        inkFollowsColor()
        refresh()
    }

    /// One numeric field committed (Return, Tab or end of editing). Only a
    /// value different from the one shown reaches a `PickerColor` setter,
    /// and it changes only its own component: the others come from the
    /// model, never from the rounded integers beside it.
    @objc private func numberCommitted(_ sender: NSTextField) {
        guard let number = sender.objectValue as? NSNumber else {
            refresh()
            return
        }
        var value = number.intValue
        if sender === hueField && value == 360 { value = 0 }
        guard value != shown[sender] else {
            refresh()
            return
        }
        switch sender {
        case hueField:
            color.setHSB(h: Double(value), s: color.saturation, b: color.brightness)
        case saturationField:
            color.setHSB(h: color.hue, s: Double(value) / 100, b: color.brightness)
        case brightnessField:
            color.setHSB(h: color.hue, s: color.saturation, b: Double(value) / 100)
        case redField:
            color.setRGB(r: value, g: color.green, b: color.blue)
        case greenField:
            color.setRGB(r: color.red, g: value, b: color.blue)
        case blueField:
            color.setRGB(r: color.red, g: color.green, b: value)
        case lightnessField, labAField, labBField:
            guard let lab = color.lab else {
                NSSound.beep()
                break
            }
            color.setLab(
                l: sender === lightnessField ? Double(value) : lab.l,
                a: sender === labAField ? Double(value) : lab.a,
                b: sender === labBField ? Double(value) : lab.b)
        default:
            break
        }
        inkFollowsColor()
        refresh()
    }

    /// The Intensity field committed: only a string different from the one
    /// shown changes the stops, so a focused, untouched field keeps the
    /// unrounded intensity the sheet opened with.
    @objc private func intensityCommitted(_ sender: NSTextField) {
        guard sender.stringValue != shownIntensity,
              let number = sender.objectValue as? NSNumber else {
            refresh()
            return
        }
        intensity = number.doubleValue
        refresh()
    }

    @objc private func hexCommitted(_ sender: NSTextField) {
        var text = sender.stringValue.trimmingCharacters(in: .whitespaces).uppercased()
        if text.hasPrefix("#") { text.removeFirst() }
        guard text != shownHex else {
            refresh()
            return
        }
        if !color.setHex(text) { NSSound.beep() }
        inkFollowsColor()
        refresh()
    }

    // MARK: - Buttons

    @objc private func okClicked(_ sender: Any?) {
        // Ending the edit runs the focused field's action first, so a typed
        // value that was never committed still counts. A field whose
        // formatter rejects its text keeps focus, so OK stays open and beeps.
        if let window = view.window, !window.makeFirstResponder(nil) {
            NSSound.beep()
            return
        }
        let changed = color != initial || (initialIntensity != nil && intensity != initialIntensity)
            || authoredInk != initialInk
        // An unchanged colour keeps the unrounded base it opened with, so an
        // intensity-only change does not quantize it to the picker's codes.
        var picked = color != initial ? color.nsColor : originalColor
        if initialIntensity != nil, changed {
            // Above the float range: beep and stay open.
            guard let combined = ExtendedRangeColor.combined(picked, intensity: intensity) else {
                NSSound.beep()
                return
            }
            picked = combined
        }
        dismiss(self)
        if changed { onCommit(picked, cmyk?.isCmyk == true ? authoredInk : nil) }
    }

    @objc private func cancelClicked(_ sender: Any?) {
        dismiss(self)
    }

    @objc private func systemColorsClicked(_ sender: Any?) {
        dismiss(self)
        onSystemColors()
    }
}
