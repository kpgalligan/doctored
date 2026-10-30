import AppKit

/// Channels ▸ New Spot Channel… and Spot Channel Options… (#547 phase 1a-2): the dialog —
/// Name, and Ink Characteristics: Color (a swatch that opens the Color Picker, plus the D50 L*, a*,
/// b* it stores) and Solidity, 0–100 %. One sheet with two modes: `channel` nil adds a spot channel
/// whose coverage is the selection's (none without one), an index edits that spot channel's name
/// and ink. Apply is one undo step through `ImageDocument.applyEdit`, recorded as
/// `add_spot_channel` / `set_spot_channel`; a field that is not a number, or a value out of range,
/// beeps, takes focus and keeps the sheet open. Since 1c-1 the options mode has a Kind pair: "Alpha
/// Channel" makes the channel an alpha channel again (`set_channel_kind`), still one step.
final class SpotChannelSheetController: NSViewController {
    private let document: ImageDocument
    private let index: Int?
    private let source: RasterDocument?
    private let originalName: String

    private let nameField = NSTextField(string: "")
    private let swatchButton = NSButton(title: "", target: nil, action: nil)
    private let labFields: [NSTextField]
    private let soliditySlider = NSSlider(value: 0, minValue: 0, maxValue: 100, target: nil, action: nil)
    private let solidityField = NSTextField(string: "0")
    private let note = NSTextField(wrappingLabelWithString: "")
    /// Spot Channel Options' kind (#547 1c-1): "Alpha Channel" turns the spot channel back into an
    /// alpha channel, its plane inverted (`settingChannelKind`).
    private let spotKindRadio = NSButton(radioButtonWithTitle: "Spot Color", target: nil, action: nil)
    private let alphaKindRadio = NSButton(radioButtonWithTitle: "Alpha Channel", target: nil, action: nil)
    /// True while the shared colour panel targets this sheet (System Colors…).
    private var usesColorPanel = false

    /// nil when `channel` is not a spot channel of the document by the time the sheet opens; the
    /// caller beeps rather than presenting an empty dialog.
    init?(document: ImageDocument, channel: Int?) {
        guard let doc = document.doc else { return nil }
        let ink: SpotInk
        if let channel {
            guard let info = doc.channelInfo(channel), let spot = info.spot else { return nil }
            ink = spot
            originalName = info.name
        } else {
            ink = SpotChannels.defaultInk
            originalName = doc.nextSpotChannelName
        }
        self.document = document
        index = channel
        source = doc
        labFields = ink.lab.map { DuotoneSheetController.numberField($0) }
        super.init(nibName: nil, bundle: nil)
        nameField.stringValue = originalName
        soliditySlider.doubleValue = Double(ink.solidity)
        solidityField.stringValue = String(ink.solidity)
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) {
        fatalError("SpotChannelSheetController does not support NSCoder")
    }

    override func loadView() {
        nameField.widthAnchor.constraint(equalToConstant: 220).isActive = true
        DSField.style(nameField)
        nameField.setAccessibilityIdentifier("spot-name")

        swatchButton.isBordered = false
        swatchButton.wantsLayer = true
        swatchButton.layer?.cornerRadius = 3
        swatchButton.layer?.borderWidth = 1
        swatchButton.layer?.borderColor = DS.textFaint.cgColor
        swatchButton.widthAnchor.constraint(equalToConstant: 22).isActive = true
        swatchButton.heightAnchor.constraint(equalToConstant: 22).isActive = true
        swatchButton.target = self
        swatchButton.action = #selector(swatchClicked(_:))
        swatchButton.toolTip = "Choose the ink colour"
        swatchButton.setAccessibilityIdentifier("spot-swatch")
        for (field, axis) in zip(labFields, ["l", "a", "b"]) {
            field.target = self
            field.action = #selector(labChanged(_:))
            field.setAccessibilityIdentifier("spot-\(axis)")
        }
        let colorRow = NSStackView(views: [swatchButton] + labFields)
        colorRow.spacing = 6

        soliditySlider.isContinuous = true
        soliditySlider.target = self
        soliditySlider.action = #selector(sliderChanged(_:))
        soliditySlider.widthAnchor.constraint(equalToConstant: 170).isActive = true
        solidityField.target = self
        solidityField.action = #selector(solidityFieldChanged(_:))
        solidityField.widthAnchor.constraint(equalToConstant: 48).isActive = true
        solidityField.alignment = .right
        DSField.style(solidityField)
        solidityField.setAccessibilityIdentifier("spot-solidity")
        let percent = NSTextField(labelWithString: "%")
        percent.font = DS.sans(13)
        let solidityRow = NSStackView(views: [soliditySlider, solidityField, percent])
        solidityRow.spacing = 8

        var rows: [[NSView]] = [
            [fieldLabel("Name:"), nameField],
            [fieldLabel("Color:"), colorRow],
            [fieldLabel("Solidity:"), solidityRow],
        ]
        if index != nil {
            spotKindRadio.state = .on
            for radio in [spotKindRadio, alphaKindRadio] {
                radio.target = self
                radio.action = #selector(kindChanged(_:))
                radio.font = DS.sans(13)
            }
            spotKindRadio.setAccessibilityIdentifier("spot-kind-spot")
            alphaKindRadio.setAccessibilityIdentifier("spot-kind-alpha")
            let kind = NSStackView(views: [spotKindRadio, alphaKindRadio])
            kind.orientation = .horizontal
            kind.spacing = 14
            rows.insert([fieldLabel("Kind:"), kind], at: 1)
        }
        let grid = NSGridView(views: rows)
        grid.rowSpacing = 10
        grid.columnSpacing = 12
        grid.column(at: 0).xPlacement = .trailing
        grid.column(at: 0).width = 106
        let axes = NSTextField(labelWithString: "Ink Characteristics: L*, a*, b* (D50)")
        axes.font = DS.sans(11)
        axes.textColor = DS.textMuted
        note.font = DS.sans(12)
        note.textColor = DS.textMuted
        note.preferredMaxLayoutWidth = sheetWidth - sheetInset * 2
        let content = NSStackView(views: [grid, axes, note])
        content.orientation = .vertical
        content.alignment = .leading
        content.spacing = 10

        let cancelButton = sheetCancelButton(target: self, action: #selector(cancelClicked(_:)))
        let applyButton = sheetApplyButton(target: self, action: #selector(applyClicked(_:)))
        applyButton.setAccessibilityIdentifier("spot-apply")
        syncSwatch()
        view = makeSheetView(
            title: index == nil ? "New Spot Channel" : "Spot Channel Options",
            hint: "A spot channel holds one ink's coverage; painting adds ink. The composite shows "
                + "each ink overprinted at its coverage: Solidity 0 % is a transparent ink over the "
                + "colours beneath, 100 % an ink that covers them. Display only.",
            content: content,
            buttonRow: makeButtonRow(cancel: cancelButton, apply: applyButton,
                                     leading: [sheetFootnote("one undo step")]))
    }

    /// The Lab fields, or nil (after a beep, with the field focused and selected) when one is not
    /// a number or is out of range.
    private func readLab() -> [Double]? {
        let ranges: [ClosedRange<Double>] = [0...100, -128...127, -128...127]
        var lab: [Double] = []
        for (field, range) in zip(labFields, ranges) {
            let text = field.stringValue.trimmingCharacters(in: .whitespaces)
            guard let value = Double(text), value.isFinite, range.contains(value) else {
                refuse(field, SpotChannels.labSentence)
                return nil
            }
            lab.append(value)
        }
        return lab
    }

    private func refuse(_ field: NSTextField, _ sentence: String) {
        NSSound.beep()
        note.stringValue = sentence
        view.window?.makeFirstResponder(field)
        field.selectText(nil)
    }

    private func setLab(_ lab: [Double]) {
        for (field, value) in zip(labFields, lab) {
            field.stringValue = DuotoneSheetController.format((value * 100).rounded() / 100)
        }
        syncSwatch()
    }

    private func syncSwatch() {
        guard let lab = readLabQuietly() else { return }
        swatchButton.layer?.backgroundColor = DuotoneSheetController.swatchColor(lab)
    }

    private func readLabQuietly() -> [Double]? {
        let values = labFields.compactMap { Double($0.stringValue.trimmingCharacters(in: .whitespaces)) }
        return values.count == 3 && values.allSatisfy(\.isFinite) ? values : nil
    }

    @objc private func labChanged(_ sender: Any?) {
        syncSwatch()
    }

    @objc private func kindChanged(_ sender: Any?) {
        guard let sender = sender as? NSButton else { return }
        spotKindRadio.state = sender === spotKindRadio ? .on : .off
        alphaKindRadio.state = sender === alphaKindRadio ? .on : .off
    }

    @objc private func sliderChanged(_ sender: Any?) {
        solidityField.stringValue = String(Int(soliditySlider.doubleValue.rounded()))
    }

    @objc private func solidityFieldChanged(_ sender: Any?) {
        guard let value = Int(solidityField.stringValue.trimmingCharacters(in: .whitespaces)),
              (0...100).contains(value) else {
            refuse(solidityField, "solidity must be from 0 to 100 %")
            return
        }
        soliditySlider.doubleValue = Double(value)
    }

    /// The swatch: the Color Picker on the ink's sRGB; its commit writes the Lab fields.
    @objc private func swatchClicked(_ sender: Any?) {
        guard let lab = readLab() else { return }
        let rgb = RasterLab.srgb(lab: lab)?.rgb ?? [1, 0, 0]
        let color = NSColor(srgbRed: CGFloat(rgb[0]), green: CGFloat(rgb[1]), blue: CGFloat(rgb[2]), alpha: 1)
        presentAsSheet(ColorPickerSheetController(
            title: "Spot Color", color: color, intensity: nil, cmyk: nil,
            onCommit: { [weak self] picked, _ in self?.pickerCommitted(picked) },
            onSystemColors: { [weak self] in self?.openSystemColors(color) }))
    }

    private func pickerCommitted(_ color: NSColor) {
        guard let srgb = color.usingColorSpace(.sRGB),
              let lab = RasterLab.lab(srgb: [Double(srgb.redComponent), Double(srgb.greenComponent),
                                             Double(srgb.blueComponent)])
        else {
            NSSound.beep()
            return
        }
        setLab(lab)
    }

    /// The picker's System Colors…: the shared colour panel, whose changes write the Lab fields.
    private func openSystemColors(_ color: NSColor) {
        let panel = NSColorPanel.shared
        usesColorPanel = true
        panel.setTarget(self)
        panel.setAction(#selector(systemColorChanged(_:)))
        panel.color = color
        panel.orderFront(nil)
    }

    @objc private func systemColorChanged(_ sender: NSColorPanel) {
        pickerCommitted(sender.color)
    }

    private func releaseColorPanel() {
        guard usesColorPanel else { return }
        usesColorPanel = false
        NSColorPanel.shared.setTarget(nil)
        NSColorPanel.shared.setAction(nil)
    }

    @objc private func applyClicked(_ sender: Any?) {
        guard document.doc === source, !document.hasClosed else {
            document.presentError(DoctoredCoreError(
                message: "The document changed. Reopen the dialog to use its current channels."))
            return
        }
        if let index, alphaKindRadio.state == .on {
            // The ink fields do not apply to an alpha channel, so they are not read.
            let typed = nameField.stringValue.trimmingCharacters(in: .whitespacesAndNewlines)
            releaseColorPanel()
            dismiss(self)
            applyAlpha(index, name: typed)
            return
        }
        guard let lab = readLab() else { return }
        guard let solidity = Int(solidityField.stringValue.trimmingCharacters(in: .whitespaces)),
              (0...100).contains(solidity) else {
            refuse(solidityField, "solidity must be from 0 to 100 %")
            return
        }
        let ink = SpotInk(lab: lab, solidity: solidity)
        let typed = nameField.stringValue.trimmingCharacters(in: .whitespacesAndNewlines)
        releaseColorPanel()
        dismiss(self)
        if let index { applyOptions(index, name: typed, ink: ink) } else { applyNew(name: typed, ink: ink) }
    }

    private func applyNew(name typed: String, ink: SpotInk) {
        let name = typed.isEmpty ? originalName : typed
        let selection = document.canvasSelection
        var arguments = ink.fields
        arguments["name"] = name
        arguments["from"] = selection == nil ? "empty" : "selection"
        document.applyEdit(
            "New Spot Channel",
            record: .channelCommand("add_spot_channel", arguments, note: "Channels ▸ New Spot Channel…")
        ) { try $0.addingSpotChannel(name: name, ink: ink, selection: selection) }
    }

    private func applyOptions(_ index: Int, name typed: String, ink: SpotInk) {
        // An empty name keeps the one the channel has, as Channel Options… does.
        let name = typed.isEmpty || typed == originalName ? nil : typed
        var arguments = ink.fields
        arguments["channel"] = EditorViewController.channelArgument(document.doc, index)
        if let name { arguments["name"] = name }
        document.applyEdit(
            "Spot Channel Options",
            record: .channelCommand("set_spot_channel", arguments, note: "Channels ▸ Spot Channel Options…"),
            renderNeutral: true
        ) { try $0.settingSpotChannel(index, name: name, ink: ink) }
    }

    /// Kind: Alpha Channel — the rename and the kind change in ONE transform, one undo step,
    /// recorded as `rename_channel` (when the name changed) then `set_channel_kind`.
    private func applyAlpha(_ index: Int, name typed: String) {
        let name = typed.isEmpty || typed == originalName ? nil : typed
        let channel = EditorViewController.channelArgument(document.doc, index)
        var record: [ActionStep] = []
        if let name {
            record += .channelCommand("rename_channel", ["channel": channel, "name": name],
                                      note: "Channels ▸ Spot Channel Options…")
        }
        let target: Any = name ?? channel
        record += .channelCommand("set_channel_kind", ["channel": target, "kind": "alpha"],
                                  note: "Channels ▸ Spot Channel Options…: Alpha Channel")
        document.applyEdit("Spot Channel Options", record: record) { doc in
            let renamed = try name.flatMap { try doc.renamingChannel(index, $0) } ?? doc
            let changed = try renamed.settingChannelKind(index, spot: nil) ?? renamed
            return changed === doc ? nil : changed
        }
    }

    @objc private func cancelClicked(_ sender: Any?) {
        releaseColorPanel()
        dismiss(self)
    }
}
