import AppKit

/// Save the delivery currently under review as a named preset: the settings,
/// a destination folder that must already exist, and a naming pattern. A
/// preset is a PREFERENCE — it lives outside every document, so saving one
/// changes no picture and registers no undo step.
final class ExportPresetSaveSheet: NSObject {
    private let settings: ExportSettings
    private let name = NSTextField()
    private let destination = NSTextField()
    private let pattern = NSTextField()
    private let makeDefault = NSButton(
        checkboxWithTitle: "Use for File ▸ Quick Export", target: nil, action: nil)
    private let replace = NSButton(
        checkboxWithTitle: "Replace a preset with this name", target: nil, action: nil)
    private let message = NSTextField(wrappingLabelWithString: "")
    private var modal: ModalCardWindowController?

    private init(settings: ExportSettings) {
        self.settings = settings
        super.init()
    }

    @discardableResult
    static func run(settings: ExportSettings) -> Bool {
        ExportPresetSaveSheet(settings: settings).run()
    }

    private func run() -> Bool {
        for field in [name, destination, pattern] {
            DSField.style(field)
            field.widthAnchor.constraint(equalToConstant: 330).isActive = true
        }
        name.placeholderString = "Preset name"
        name.setAccessibilityIdentifier("export-preset-name")
        destination.placeholderString = "Destination folder"
        destination.stringValue =
            FileManager.default.urls(for: .picturesDirectory, in: .userDomainMask).first?.path ?? ""
        destination.setAccessibilityIdentifier("export-preset-destination")
        pattern.stringValue = ExportNaming.defaultPattern
        pattern.setAccessibilityIdentifier("export-preset-pattern")
        for box in [makeDefault, replace] { box.font = DS.sans(12) }
        message.font = DS.sans(11)
        message.textColor = DS.textMuted
        message.preferredMaxLayoutWidth = 340

        let choose = StickerButton(title: "Choose…", style: .secondary,
                                   target: self, action: #selector(chooseFolder(_:)))
        let folderRow = NSStackView(views: [destination, choose])
        folderRow.spacing = 8
        let summary = NSTextField(wrappingLabelWithString:
            "\(settings.format.displayName) · \(settings.profile.displayName) · "
            + "\(settings.bitDepth)-bit")
        summary.font = DS.sans(11)
        summary.preferredMaxLayoutWidth = 340
        let content = NSStackView(views: [name, folderRow, pattern, summary, makeDefault, replace, message])
        content.orientation = .vertical
        content.alignment = .leading
        content.spacing = 10

        let cancel = sheetCancelButton(target: self, action: #selector(cancel(_:)))
        let save = StickerButton(title: "Save Preset", style: .primary,
                                 target: self, action: #selector(save(_:)))
        save.setAccessibilityIdentifier("export-preset-save")
        modal = ModalCardWindowController(title: "Save Export Preset", card: makeSheetView(
            title: "Save Export Preset",
            hint: "The pattern names the file inside the destination folder. Tokens: "
                + ExportNaming.tokenHelp
                + " Quick Export never overwrites an existing file (a taken name gets -2, -3, …),"
                + " and never creates the folder.",
            content: content, buttonRow: makeButtonRow(cancel: cancel, apply: save)))
        return modal?.runModal() == .OK
    }

    @objc private func chooseFolder(_ sender: Any?) {
        let panel = NSOpenPanel()
        panel.canChooseDirectories = true
        panel.canChooseFiles = false
        panel.canCreateDirectories = true
        guard panel.runModal() == .OK, let url = panel.url else { return }
        destination.stringValue = url.path
    }

    @objc private func cancel(_ sender: Any?) { modal?.end(.cancel) }

    @objc private func save(_ sender: Any?) {
        do {
            let preset = ExportPreset(name: name.stringValue, settings: settings,
                                      destination: destination.stringValue,
                                      pattern: pattern.stringValue)
            let saved = try ExportPresetLibrary.save(preset, replace: replace.state == .on)
            if makeDefault.state == .on { try ExportPresetLibrary.setDefault(name: saved.name) }
            modal?.end(.OK)
        } catch {
            message.stringValue = error.localizedDescription
            NSSound.beep()
        }
    }
}

/// List, load, delete and choose the Quick Export default. The sheet only
/// PICKS; the export workspace applies, so one path serves the sheet, the
/// menu and MCP alike.
final class ExportPresetManagerSheet: NSObject {
    private var presets: [ExportPreset] = []
    private let current: ExportSettings?
    private let popup = NSPopUpButton()
    private let detail = NSTextField(wrappingLabelWithString: "")
    private let message = NSTextField(wrappingLabelWithString: "")
    private var modal: ModalCardWindowController?
    private var selected: ExportPreset?
    private var applyButton: NSButton?
    private var deleteButton: NSButton?
    private var defaultButton: NSButton?
    private var brokenSection: BrokenLibraryFilesSection?

    private init(current: ExportSettings?) {
        self.current = current
        super.init()
    }

    static func run(current: ExportSettings? = nil) -> ExportPreset? {
        ExportPresetManagerSheet(current: current).run()
    }

    private func run() -> ExportPreset? {
        popup.widthAnchor.constraint(equalToConstant: 330).isActive = true
        popup.setAccessibilityIdentifier("export-preset-manager-list")
        popup.target = self
        popup.action = #selector(selectionChanged(_:))
        let save = StickerButton(title: "Save Current…", style: .secondary,
                                 target: self, action: #selector(saveCurrent(_:)))
        save.isEnabled = current != nil
        let delete = StickerButton(title: "Delete", style: .secondary,
                                   target: self, action: #selector(deleteCurrent(_:)))
        delete.setAccessibilityIdentifier("export-preset-delete")
        deleteButton = delete
        let makeDefault = StickerButton(title: "Set as Quick Export", style: .secondary,
                                        target: self, action: #selector(setDefault(_:)))
        makeDefault.setAccessibilityIdentifier("export-preset-default")
        defaultButton = makeDefault
        let actions = NSStackView(views: [save, delete, makeDefault])
        actions.spacing = 8
        for label in [detail, message] {
            label.font = DS.sans(11)
            label.textColor = DS.textMuted
            label.preferredMaxLayoutWidth = 340
        }
        // The bad files, listed with their reasons. Delete File is the UI twin
        // of delete_export_preset {file}; there is no repair outside RAW.
        let broken = BrokenLibraryFilesSection(
            title: "Files that are not presets:", identifierPrefix: "export-preset",
            delete: { try ExportPresetLibrary.delete(file: $0) }, repair: nil,
            changed: { [weak self] in self?.reload() },
            report: { [weak self] in self?.message.stringValue = $0 })
        brokenSection = broken
        let content = NSStackView(views: [popup, detail, actions, broken.view, message])
        content.orientation = .vertical
        content.alignment = .leading
        content.spacing = 12

        let cancel = sheetCancelButton(target: self, action: #selector(cancel(_:)))
        let apply = StickerButton(title: "Load", style: .primary,
                                  target: self, action: #selector(apply(_:)))
        applyButton = apply
        reload()
        modal = ModalCardWindowController(title: "Export Presets", card: makeSheetView(
            title: "Export Presets",
            hint: "Loading a preset fills the export workspace with its settings. Quick Export "
                + "writes the default preset with no dialog and never overwrites a file: a "
                + "taken name gets -2, -3, …. With no default it writes a PNG beside the "
                + "document, or in Downloads.",
            content: content, buttonRow: makeButtonRow(cancel: cancel, apply: apply)))
        _ = modal?.runModal()
        return selected
    }

    private func reload() {
        do {
            let listing = try ExportPresetLibrary.listing()
            presets = listing.entries.map(\.item)
            let current = try ExportPresetLibrary.defaultPresetName()
            popup.removeAllItems()
            popup.addItems(withTitles: presets.map { $0.name == current ? "\($0.name) (default)" : $0.name })
            applyButton?.isEnabled = !presets.isEmpty
            deleteButton?.isEnabled = !presets.isEmpty
            defaultButton?.isEnabled = !presets.isEmpty
            message.stringValue = presets.isEmpty && listing.broken.isEmpty
                ? "No export presets saved yet." : ""
            if brokenSection?.update(listing.broken) == true { modal?.refit() }
        } catch {
            presets = []
            popup.removeAllItems()
            applyButton?.isEnabled = false
            deleteButton?.isEnabled = false
            defaultButton?.isEnabled = false
            if brokenSection?.update([]) == true { modal?.refit() }
            message.stringValue = error.localizedDescription
        }
        selectionChanged(nil)
    }

    @objc private func selectionChanged(_ sender: Any?) {
        guard presets.indices.contains(popup.indexOfSelectedItem) else {
            detail.stringValue = ""
            return
        }
        let preset = presets[popup.indexOfSelectedItem]
        detail.stringValue = "\(preset.settings.format.displayName) · "
            + "\(preset.settings.profile.displayName) · \(preset.settings.bitDepth)-bit\n"
            + "\(preset.destination)/\(preset.pattern)"
    }

    @objc private func saveCurrent(_ sender: Any?) {
        guard let current else { return }
        ExportPresetSaveSheet.run(settings: current)
        reload()
    }

    @objc private func deleteCurrent(_ sender: Any?) {
        guard presets.indices.contains(popup.indexOfSelectedItem) else { return }
        do {
            try ExportPresetLibrary.delete(name: presets[popup.indexOfSelectedItem].name)
            reload()
        } catch {
            message.stringValue = error.localizedDescription
            NSSound.beep()
        }
    }

    @objc private func setDefault(_ sender: Any?) {
        guard presets.indices.contains(popup.indexOfSelectedItem) else { return }
        do {
            try ExportPresetLibrary.setDefault(name: presets[popup.indexOfSelectedItem].name)
            reload()
        } catch {
            message.stringValue = error.localizedDescription
            NSSound.beep()
        }
    }

    @objc private func apply(_ sender: Any?) {
        guard presets.indices.contains(popup.indexOfSelectedItem) else { return }
        selected = presets[popup.indexOfSelectedItem]
        modal?.end(.OK)
    }

    @objc private func cancel(_ sender: Any?) { modal?.end(.cancel) }
}
