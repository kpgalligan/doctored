import AppKit
import UniformTypeIdentifiers

/// HDR Toning's preset row, in `DevelopFilterPresetControl.swift`'s pattern:
/// `Preset: [popup]`, the popup listing the built-ins (`HdrToningPreset.builtIns`: Default and
/// the looks), the user's presets, Custom (shown once the controls move off the chosen entry),
/// Save Preset…, Import Preset… (a `.hdt` file, `HdrToningPresetImport.swift`) and Delete
/// Preset… (a user preset only). A look's purpose is its menu item's tooltip and, while it is the
/// chosen entry, a muted caption under the row; with Default, a user preset or Custom chosen the
/// caption is hidden, so the dialog opens as it always has.
///
/// Choosing an entry hands the preset to `load`, and the sheet calls `markCustom()` on any later
/// change. Save, Import and Delete write the library at once, independent of OK/Cancel: they are
/// preferences, not edits. An imported preset is loaded and chosen, as a saved one is.
final class HdrToningPresetControl: NSObject {
    let row: NSView

    private let current: () -> HdrToningSettings
    private let load: (HdrToningPreset) -> Void
    private let popup = NSPopUpButton(frame: .zero, pullsDown: false)
    /// The chosen look's purpose, under the popup.
    private let caption = NSTextField(wrappingLabelWithString: "")
    private var presets: [HdrToningPreset] = []
    /// The built-in the controls were loaded from (Default when the sheet opens).
    private var selectedBuiltIn: String? = HdrToningPreset.defaultName
    /// The user preset the controls were loaded from.
    private var selectedName: String?
    private var isCustom = false
    private var observer: NSObjectProtocol?

    private enum Tag: Int { case builtIn, preset, custom, save, importFile, delete }

    init(current: @escaping () -> HdrToningSettings, load: @escaping (HdrToningPreset) -> Void) {
        self.current = current
        self.load = load
        popup.font = DS.sans(13)
        popup.autoenablesItems = false
        popup.setAccessibilityIdentifier("hdr-toning-preset-popup")
        popup.widthAnchor.constraint(lessThanOrEqualToConstant: 240).isActive = true
        let line = NSStackView(views: [fieldLabel("Preset:"), popup])
        line.orientation = .horizontal
        line.alignment = .centerY
        line.spacing = 8
        caption.font = DS.sans(12)
        caption.textColor = DS.textMuted
        caption.preferredMaxLayoutWidth = 380
        caption.isHidden = true
        caption.setAccessibilityIdentifier("hdr-toning-preset-purpose")
        let stack = NSStackView(views: [line, caption])
        stack.orientation = .vertical
        stack.alignment = .leading
        stack.spacing = 4
        row = stack
        super.init()
        popup.target = self
        popup.action = #selector(popupChanged(_:))
        observer = NotificationCenter.default.addObserver(
            forName: HdrToningPresetLibrary.didChange, object: nil, queue: .main
        ) { [weak self] _ in self?.rebuild() }
        rebuild()
    }

    deinit {
        if let observer { NotificationCenter.default.removeObserver(observer) }
    }

    /// The controls moved off the loaded entry.
    func markCustom() {
        guard !isCustom else { return }
        isCustom = true
        rebuild()
    }

    // MARK: - The popup

    private func rebuild() {
        presets = (try? HdrToningPresetLibrary.entries()) ?? []
        if let name = selectedName, !presets.contains(where: { $0.name == name }) {
            selectedName = nil
            isCustom = true
        }
        popup.removeAllItems()
        guard let menu = popup.menu else { return }
        // Menu items, not `addItem(withTitle:)`, which drops a repeated title.
        for preset in HdrToningPreset.builtIns {
            let entry = item(preset.name, .builtIn)
            entry.representedObject = preset.name
            entry.toolTip = preset.purpose
            menu.addItem(entry)
        }
        if !presets.isEmpty { menu.addItem(.separator()) }
        for preset in presets {
            let entry = item(preset.name, .preset)
            entry.representedObject = preset.name
            menu.addItem(entry)
        }
        let custom = item("Custom", .custom)
        custom.isHidden = !isCustom
        menu.addItem(custom)
        menu.addItem(.separator())
        menu.addItem(item("Save Preset…", .save))
        menu.addItem(item("Import Preset…", .importFile))
        let delete = item("Delete Preset…", .delete)
        delete.isEnabled = selectedName != nil && !isCustom
        menu.addItem(delete)
        showSelection()
    }

    private func item(_ title: String, _ tag: Tag) -> NSMenuItem {
        let entry = NSMenuItem(title: title, action: nil, keyEquivalent: "")
        entry.tag = tag.rawValue
        return entry
    }

    private func showSelection() {
        guard let menu = popup.menu else { return }
        let target = menu.items.first { entry in
            if isCustom { return entry.tag == Tag.custom.rawValue }
            if let selectedName {
                return entry.tag == Tag.preset.rawValue
                    && entry.representedObject as? String == selectedName
            }
            return entry.tag == Tag.builtIn.rawValue
                && entry.representedObject as? String == selectedBuiltIn
        }
        if let target { popup.select(target) }
        let look = isCustom || selectedName != nil ? nil
            : HdrToningPreset.builtIns.first { $0.name == selectedBuiltIn }
        caption.stringValue = look?.purpose ?? ""
        caption.isHidden = look?.purpose == nil
    }

    @objc private func popupChanged(_ sender: Any?) {
        guard let chosen = popup.selectedItem, let tag = Tag(rawValue: chosen.tag) else { return }
        switch tag {
        case .builtIn:
            guard let name = chosen.representedObject as? String,
                  let preset = HdrToningPreset.builtIns.first(where: { $0.name == name })
            else { rebuild(); return }
            load(preset)
            selected(builtIn: name, name: nil)
        case .preset:
            guard let name = chosen.representedObject as? String,
                  let preset = presets.first(where: { $0.name == name })
            else { rebuild(); return }
            load(preset)
            selected(builtIn: nil, name: name)
        case .custom:
            showSelection()
        case .save:
            showSelection()
            saveClicked()
        case .importFile:
            showSelection()
            importClicked()
        case .delete:
            showSelection()
            deleteClicked()
        }
    }

    private func selected(builtIn: String?, name: String?) {
        selectedBuiltIn = builtIn
        selectedName = name
        isCustom = false
        rebuild()
    }

    private func saveClicked() {
        let settings = current()
        guard let name = Self.askName() else { return }
        do {
            let saved = try HdrToningPresetLibrary.save(name: name, settings: settings, replace: false)
            selected(builtIn: nil, name: saved.name)
        } catch {
            Self.fail("Could Not Save the Preset", error)
        }
    }

    /// Import Preset…: a `.hdt` file chosen in a sheet on the dialog's own window, saved into the
    /// library under its file's name (`HdrToningPresetLibrary.importFile`, the whole of the
    /// import), then loaded and chosen. A report from the core is shown in one alert.
    private func importClicked() {
        guard let window = popup.window else { NSSound.beep(); return }
        let panel = NSOpenPanel()
        panel.allowsMultipleSelection = false
        panel.canChooseDirectories = false
        panel.message = "Choose a Photoshop HDR Toning preset (.hdt). It is saved under its "
            + "file's name."
        panel.prompt = "Import"
        if let type = UTType(filenameExtension: HdrToningPresetLibrary.importExtension) {
            panel.allowedContentTypes = [type]
        }
        panel.directoryURL = HdrToningPresetLibrary.installedPresetsDirectory()
        panel.beginSheetModal(for: window) { [weak self] response in
            guard response == .OK, let url = panel.url else { return }
            self?.importPreset(from: url)
        }
    }

    private func importPreset(from url: URL) {
        do {
            let imported = try HdrToningPresetLibrary.importFile(url)
            load(imported.preset)
            selected(builtIn: nil, name: imported.preset.name)
            if !imported.report.isEmpty { Self.note(imported.preset.name, imported.report) }
        } catch {
            Self.fail("Could Not Import the Preset", error)
        }
    }

    /// The import's report: one sentence per value imported other than as stored.
    private static func note(_ name: String, _ report: [String]) {
        let alert = NSAlert()
        alert.messageText = "Imported “\(name)”"
        alert.informativeText = report.map { line -> String in
            line.prefix(1).uppercased() + String(line.dropFirst()) + "."
        }.joined(separator: "\n")
        alert.runModal()
    }

    private func deleteClicked() {
        guard let name = selectedName else { NSSound.beep(); return }
        let alert = NSAlert()
        alert.messageText = "Delete the preset “\(name)”?"
        alert.informativeText = "The dialog keeps its current values."
        alert.addButton(withTitle: "Delete")
        alert.addButton(withTitle: "Cancel")
        guard alert.runModal() == .alertFirstButtonReturn else { return }
        do {
            try HdrToningPresetLibrary.delete(name: name)
            selectedName = nil
            isCustom = true
            rebuild()
        } catch {
            Self.fail("Could Not Delete the Preset", error)
        }
    }

    /// A name for a new preset, or nil when cancelled.
    private static func askName() -> String? {
        let alert = NSAlert()
        alert.messageText = "Save HDR Toning Preset"
        alert.informativeText = "A preset is a preference: it is saved for every document, and "
            + "saving or deleting one changes no picture and adds no undo step. It keeps the "
            + "method and its values, not the depth."
        let field = NSTextField(frame: NSRect(x: 0, y: 0, width: 280, height: 24))
        DSField.style(field)
        field.placeholderString = "Preset name"
        field.setAccessibilityIdentifier("hdr-toning-preset-name")
        alert.accessoryView = field
        alert.addButton(withTitle: "Save Preset")
        alert.addButton(withTitle: "Cancel")
        alert.window.initialFirstResponder = field
        guard alert.runModal() == .alertFirstButtonReturn else { return nil }
        return field.stringValue
    }

    private static func fail(_ title: String, _ error: Error) {
        NSSound.beep()
        let failure = NSAlert()
        failure.messageText = title
        failure.informativeText = error.localizedDescription
        failure.runModal()
    }
}
