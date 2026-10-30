import AppKit

/// Image > Channels > Channel Options… (also the channels row menu): the
/// name and the rubylith a channel is drawn in. The rubylith is DISPLAY
/// state — it never changes a byte of the channel's coverage — so the only
/// thing this sheet needs to preview is itself.
///
/// Apply chains the rename and the overlay change inside ONE transform
/// closure, so the whole dialog is one undo step. Color Indicates: Spot Color
/// (#1 1c-1) makes the channel a spot channel instead, in the same one step.
final class ChannelOptionsSheetController: NSViewController {
    private let document: ImageDocument
    private let index: Int
    private let source: RasterDocument?
    private let originalOpacity: Double
    private var opacityEdited = false

    private let nameField = NSTextField(string: "")
    private let maskedRadio = NSButton(
        radioButtonWithTitle: "Masked Areas", target: nil, action: nil)
    private let selectedRadio = NSButton(
        radioButtonWithTitle: "Selected Areas", target: nil, action: nil)
    /// The third choice (#1 1c-1): the channel becomes a spot channel, its colour the
    /// ink and its opacity the Solidity (`settingChannelKind`).
    private let spotRadio = NSButton(radioButtonWithTitle: "Spot Color", target: nil, action: nil)
    private let opacityLabel = fieldLabel("Opacity:")
    private let colorWell = NSColorWell(frame: .zero)
    private let opacitySlider = NSSlider(value: 50, minValue: 0, maxValue: 100, target: nil, action: nil)
    private let opacityField = NSTextField(string: "50")

    /// nil when the index is gone by the time the sheet opens; the caller
    /// beeps rather than presenting an empty dialog.
    init?(document: ImageDocument, channel index: Int) {
        guard let info = document.doc?.channelInfo(index) else { return nil }
        self.document = document
        self.index = index
        source = document.doc
        originalOpacity = info.opacity
        super.init(nibName: nil, bundle: nil)
        nameField.stringValue = info.name
        colorWell.color = NSColor(
            srgbRed: CGFloat(info.red) / 255, green: CGFloat(info.green) / 255,
            blue: CGFloat(info.blue) / 255, alpha: 1)
        opacitySlider.doubleValue = info.opacity * 100
        opacityField.stringValue = Self.opacityText(opacitySlider.doubleValue)
        maskedRadio.state = info.colorIndicatesSelected ? .off : .on
        selectedRadio.state = info.colorIndicatesSelected ? .on : .off
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) {
        fatalError("ChannelOptionsSheetController does not support NSCoder")
    }

    override func loadView() {
        nameField.widthAnchor.constraint(equalToConstant: 220).isActive = true
        DSField.style(nameField)

        for radio in [maskedRadio, selectedRadio, spotRadio] {
            radio.target = self
            radio.action = #selector(polarityChanged(_:))
            radio.font = DS.sans(13)
        }
        spotRadio.setAccessibilityIdentifier("channel-options-spot")
        // Indexed Color and Bitmap have no spot channels.
        spotRadio.isEnabled = source.map { !$0.isIndexed && !$0.isBitmap } ?? false
        let polarity = NSStackView(views: [maskedRadio, selectedRadio, spotRadio])
        polarity.orientation = .horizontal
        polarity.spacing = 14

        colorWell.widthAnchor.constraint(equalToConstant: 54).isActive = true
        colorWell.heightAnchor.constraint(equalToConstant: 22).isActive = true

        opacitySlider.isContinuous = true
        opacitySlider.target = self
        opacitySlider.action = #selector(sliderChanged(_:))
        opacitySlider.widthAnchor.constraint(equalToConstant: 170).isActive = true

        let formatter = NumberFormatter()
        formatter.numberStyle = .none
        formatter.allowsFloats = true
        formatter.maximumFractionDigits = 12
        formatter.minimum = 0
        formatter.maximum = 100
        opacityField.formatter = formatter
        opacityField.target = self
        opacityField.action = #selector(fieldChanged(_:))
        opacityField.widthAnchor.constraint(equalToConstant: 56).isActive = true
        DSField.style(opacityField)

        let opacityRow = NSStackView(views: [opacitySlider, opacityField])
        opacityRow.orientation = .horizontal
        opacityRow.spacing = 10

        let grid = NSGridView(views: [
            [fieldLabel("Name:"), nameField],
            [fieldLabel("Color Indicates:"), polarity],
            [fieldLabel("Color:"), colorWell],
            [opacityLabel, opacityRow],
        ])
        grid.rowSpacing = 10
        grid.columnSpacing = 12
        grid.column(at: 0).xPlacement = .trailing
        grid.column(at: 0).width = 106

        let cancelButton = sheetCancelButton(target: self, action: #selector(cancelClicked(_:)))
        let applyButton = sheetApplyButton(target: self, action: #selector(applyClicked(_:)))
        view = makeSheetView(
            title: "Channel Options",
            hint: "The rubylith a channel is drawn in over the image. Display only — it is "
                + "never part of the picture. Masked Areas (the default) washes where the "
                + "channel is black, exactly like Quick Mask.",
            content: grid,
            buttonRow: makeButtonRow(
                cancel: cancelButton, apply: applyButton,
                leading: [sheetFootnote("one undo step")]))
    }

    @objc private func polarityChanged(_ sender: Any?) {
        guard let sender = sender as? NSButton else { return }
        maskedRadio.state = sender === maskedRadio ? .on : .off
        selectedRadio.state = sender === selectedRadio ? .on : .off
        spotRadio.state = sender === spotRadio ? .on : .off
        // Spot Color reads the opacity as the ink's Solidity, a whole percent.
        let spot = spotRadio.state == .on
        opacityLabel.stringValue = spot ? "Solidity:" : "Opacity:"
        if spot {
            opacitySlider.doubleValue = opacitySlider.doubleValue.rounded()
            opacityField.stringValue = Self.opacityText(opacitySlider.doubleValue)
        }
    }

    private static func opacityText(_ percent: Double) -> String { String(format: "%.8g", percent) }

    @objc private func sliderChanged(_ sender: Any?) {
        opacityEdited = true
        opacityField.stringValue = Self.opacityText(opacitySlider.doubleValue)
    }

    @objc private func fieldChanged(_ sender: Any?) {
        opacityEdited = true
        opacitySlider.doubleValue = min(max(opacityField.doubleValue, 0), 100)
        opacityField.stringValue = Self.opacityText(opacitySlider.doubleValue)
    }

    @objc private func applyClicked(_ sender: Any?) {
        guard document.doc === source, !document.hasClosed else {
            document.presentError(DoctoredCoreError(message: "The document changed. Reopen Channel Options to use its current channels."))
            return
        }
        // Trimmed before the emptiness test, the panel's inline field's rule:
        // a whitespace-only name draws as a blank row and can then only be
        // addressed by index, which is exactly what the agent's
        // `set_channel_options` declines to set.
        let name = nameField.stringValue.trimmingCharacters(in: .whitespacesAndNewlines)
        let color = colorWell.color.usingColorSpace(.sRGB) ?? .red
        // The picker can hand back a Display P3 colour whose sRGB
        // components fall outside 0…1; clamping before the byte conversion
        // is what keeps that from trapping (LayerStyleColor.hex's rule).
        let byte: (CGFloat) -> UInt8 = { UInt8((min(max($0, 0), 1) * 255).rounded()) }
        let red = byte(color.redComponent)
        let green = byte(color.greenComponent)
        let blue = byte(color.blueComponent)
        let opacity = opacityEdited ? min(max(opacitySlider.doubleValue, 0), 100) / 100 : originalOpacity
        let indicatesSelected = selectedRadio.state == .on
        let index = self.index
        if spotRadio.state == .on {
            applySpot(index, name: name, color: color)
            return
        }
        dismiss(self)
        // Both core calls chain inside ONE transform: either half may
        // legitimately refuse (nothing changed), so the second falls back to
        // the first's result rather than to nil.
        // Addressed by the channel's CURRENT name — what the replay document
        // still calls it — with the new name in `name`, exactly as a layer
        // rename records. A channel whose name will not read falls back to its
        // INDEX (EditorViewController.channelArgument, Phase 12A) instead of
        // the visible placeholder.
        var arguments: [String: Any] = [
            "channel": EditorViewController.channelArgument(document.doc, index),
            "overlay_color": RasterImage.hexString(
                (r: red, g: green, b: blue, a: 255)),
            "overlay_opacity": opacity,
            "color_indicates": indicatesSelected ? "selected" : "masked",
        ]
        if !name.isEmpty { arguments["name"] = name }
        let record: [ActionStep] = .channelCommand(
            "set_channel_options", arguments, note: "Channels ▸ Channel Options…")
        document.applyEdit("Channel Options", record: record, renderNeutral: true) { doc in
            let renamed = name.isEmpty ? doc : (try doc.renamingChannel(index, name) ?? doc)
            let recoloured = try renamed.settingChannelOverlay(
                index, red: red, green: green, blue: blue, opacity: opacity,
                indicatesSelected: indicatesSelected)
            let updated = recoloured ?? renamed
            // A dialog that changed nothing must not mint an undo step.
            return updated === doc ? nil : updated
        }
    }

    /// Color Indicates: Spot Color — the rename and the kind change in ONE transform, one undo
    /// step: the colour as the ink's D50 Lab, the opacity as its Solidity in whole percent, the
    /// plane inverted (what showed black becomes full ink). Recorded as `rename_channel` (when the
    /// name changed) then `set_channel_kind`.
    private func applySpot(_ index: Int, name: String, color: NSColor) {
        let unit: (CGFloat) -> Double = { Double(min(max($0, 0), 1)) }
        guard let lab = RasterLab.lab(srgb: [unit(color.redComponent), unit(color.greenComponent),
                                             unit(color.blueComponent)]) else {
            NSSound.beep()
            return
        }
        let ink = SpotInk(lab: lab, solidity: Int(min(max(opacitySlider.doubleValue, 0), 100).rounded()))
        let current = document.doc?.channelInfo(index)?.name
        let renaming = !name.isEmpty && name != current
        dismiss(self)
        let channel = EditorViewController.channelArgument(document.doc, index)
        var record: [ActionStep] = []
        if renaming {
            record += .channelCommand("rename_channel", ["channel": channel, "name": name],
                                      note: "Channels ▸ Channel Options…")
        }
        var arguments = ink.fields
        arguments["channel"] = renaming ? name as Any : channel
        arguments["kind"] = "spot"
        record += .channelCommand("set_channel_kind", arguments,
                                  note: "Channels ▸ Channel Options…: Spot Color")
        document.applyEdit("Channel Options", record: record) { doc in
            let renamed = renaming ? (try doc.renamingChannel(index, name) ?? doc) : doc
            let changed = try renamed.settingChannelKind(index, spot: ink) ?? renamed
            return changed === doc ? nil : changed
        }
    }

    @objc private func cancelClicked(_ sender: Any?) {
        dismiss(self)
    }
}
