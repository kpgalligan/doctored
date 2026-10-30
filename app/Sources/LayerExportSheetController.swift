import AppKit

/// File ▸ Export Layers… — deliver every selected (or visible, or every)
/// top-level layer or group as its own file.
///
/// The sheet chooses only WHAT and WHERE; the delivery itself is
/// `ImageDocument.exportLayers`, so the sheet, MCP and a replayed Action all
/// run the same code and the same refusals.
final class LayerExportSheetController: NSViewController {
    private weak var document: ImageDocument?
    private let itemsPopup = NSPopUpButton()
    private let presetPopup = NSPopUpButton()
    private let metadataPopup = NSPopUpButton()
    private let trimmedBox = NSButton(
        checkboxWithTitle: "Trim to content", target: nil, action: nil)
    private let destination = NSTextField()
    private let pattern = NSTextField()
    private let summary = NSTextField(wrappingLabelWithString: "")
    private let message = NSTextField(wrappingLabelWithString: "")
    private var presets: [ExportPreset] = []
    private var policies: [NamedMetadataPolicy] = []
    private var settings: ExportSettings

    init(document: ImageDocument) throws {
        guard document.doc != nil else {
            throw DoctoredCoreError(message: "No image loaded.")
        }
        self.document = document
        settings = ExportSettings.defaults(for: document)
        settings.metadata = nil
        super.init(nibName: nil, bundle: nil)
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) {
        fatalError("LayerExportSheetController does not support NSCoder")
    }

    override func loadView() {
        itemsPopup.addItems(withTitles: LayerExportItems.allCases.map(\.displayName))
        itemsPopup.target = self
        itemsPopup.action = #selector(controlChanged(_:))
        itemsPopup.setAccessibilityIdentifier("layer-export.items")
        for popup in [itemsPopup, presetPopup, metadataPopup] {
            popup.widthAnchor.constraint(equalToConstant: 200).isActive = true
        }
        presetPopup.target = self
        presetPopup.action = #selector(presetChosen(_:))
        presetPopup.setAccessibilityIdentifier("layer-export.preset")
        metadataPopup.target = self
        metadataPopup.action = #selector(metadataPolicyChosen(_:))
        metadataPopup.setAccessibilityIdentifier("layer-export.metadata")
        trimmedBox.font = DS.sans(12)
        trimmedBox.target = self
        trimmedBox.action = #selector(controlChanged(_:))
        trimmedBox.setAccessibilityIdentifier("layer-export.trimmed")

        for field in [destination, pattern] {
            DSField.style(field)
            field.widthAnchor.constraint(equalToConstant: 320).isActive = true
        }
        destination.placeholderString = "Destination folder"
        destination.stringValue =
            FileManager.default.urls(for: .picturesDirectory, in: .userDomainMask).first?.path ?? ""
        destination.setAccessibilityIdentifier("layer-export.destination")
        pattern.stringValue = "{name}-{index}-{layer}"
        pattern.setAccessibilityIdentifier("layer-export.pattern")

        for label in [summary, message] {
            label.font = DS.sans(11)
            label.textColor = DS.textMuted
            label.preferredMaxLayoutWidth = 360
        }

        let choose = StickerButton(title: "Choose…", style: .secondary,
                                   target: self, action: #selector(chooseFolder(_:)))
        let folderRow = NSStackView(views: [destination, choose])
        folderRow.spacing = 8

        let content = NSStackView(views: [
            row("Items", itemsPopup), trimmedBox, row("Preset", presetPopup),
            row("Metadata policy", metadataPopup), folderRow, row("Pattern", pattern),
            summary, message,
        ])
        content.orientation = .vertical
        content.alignment = .leading
        content.spacing = 10

        let export = StickerButton(title: "Export Layers", style: .primary,
                                   target: self, action: #selector(exportClicked(_:)))
        export.setAccessibilityIdentifier("layer-export.run")
        view = makeSheetView(
            title: "Export Layers",
            hint: "Each item is rendered on its own, against transparency, and delivered through "
                + "the preset's settings. The pattern must contain {layer} or {index}. Nothing is "
                + "overwritten and no folder is created. Tokens: " + LayerExport.tokenHelp,
            content: content,
            buttonRow: makeButtonRow(
                cancel: sheetCancelButton(target: self, action: #selector(cancelClicked(_:))),
                apply: export),
            width: 520)
        reloadPresets()
        reloadPolicies()
        controlChanged(nil)
    }

    private func row(_ label: String, _ control: NSView) -> NSStackView {
        let row = NSStackView(views: [fieldLabel(label), control])
        row.orientation = .horizontal
        row.spacing = 8
        return row
    }

    private var mode: LayerExportItems {
        let index = itemsPopup.indexOfSelectedItem
        return LayerExportItems.allCases.indices.contains(index)
            ? LayerExportItems.allCases[index] : .selected
    }

    @objc private func controlChanged(_ sender: Any?) {
        guard let document else { return }
        let count = LayerExport.items(document, mode).count
        summary.stringValue = count == 1 ? "1 item will be delivered."
            : "\(count) items will be delivered."
    }

    @objc private func chooseFolder(_ sender: Any?) {
        let panel = NSOpenPanel()
        panel.canChooseDirectories = true
        panel.canChooseFiles = false
        panel.canCreateDirectories = true
        guard panel.runModal() == .OK, let url = panel.url else { return }
        destination.stringValue = url.path
    }

    private func reloadPresets() {
        presetPopup.removeAllItems()
        presetPopup.addItem(withTitle: "Current settings")
        do {
            presets = try ExportPresetLibrary.entries()
            presetPopup.addItems(withTitles: presets.map(\.name))
        } catch {
            presets = []
            message.stringValue = error.localizedDescription
        }
        presetPopup.selectItem(at: 0)
    }

    @objc private func presetChosen(_ sender: Any?) {
        let index = presetPopup.indexOfSelectedItem - 1
        guard presets.indices.contains(index) else { return }
        let preset = presets[index]
        settings = (try? preset.resolvedSettings()) ?? preset.settings
        destination.stringValue = preset.destination
        pattern.stringValue = preset.pattern
        if LayerExport.patternProblem(preset.pattern) != nil {
            // A whole-document preset's pattern names one file; a batch needs
            // a per-item token, so the sheet adds one rather than refusing.
            pattern.stringValue = preset.pattern + "-{index}-{layer}"
        }
    }

    private func reloadPolicies() {
        metadataPopup.removeAllItems()
        metadataPopup.addItem(withTitle: "Preset / custom")
        do {
            policies = try MetadataPolicyLibrary.entries()
            metadataPopup.addItems(withTitles: policies.map(\.name))
        } catch {
            policies = []
            message.stringValue = error.localizedDescription
        }
        metadataPopup.selectItem(at: 0)
    }

    @objc private func metadataPolicyChosen(_ sender: Any?) {
        let index = metadataPopup.indexOfSelectedItem - 1
        if policies.indices.contains(index) {
            settings.metadata = policies[index].policy
            return
        }
        // Item 0 means "whatever the chosen preset resolves to", not "none":
        // re-derive it so picking it back does not silently drop the preset's
        // own policy.
        let chosen = presetPopup.indexOfSelectedItem - 1
        settings.metadata = presets.indices.contains(chosen)
            ? (try? presets[chosen].resolvedSettings())?.metadata : nil
    }

    @objc private func exportClicked(_ sender: Any?) {
        guard let document else {
            NSSound.beep()
            return
        }
        let request = LayerExportRequest(
            items: mode, trimmed: trimmedBox.state == .on, settings: settings,
            destination: destination.stringValue, pattern: pattern.stringValue,
            preset: presets.indices.contains(presetPopup.indexOfSelectedItem - 1)
                ? presets[presetPopup.indexOfSelectedItem - 1].name : nil)
        do {
            let report = try document.exportLayers(request)
            dismiss(self)
            document.presentExportNotice(
                "Exported \(report.written) of \(report.outcomes.count) items.\n\n" + report.text)
        } catch {
            message.stringValue = error.localizedDescription
            NSSound.beep()
        }
    }

    @objc private func cancelClicked(_ sender: Any?) { dismiss(self) }
}
