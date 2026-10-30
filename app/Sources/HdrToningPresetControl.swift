import AppKit

/// HDR Toning's preset row, in `DevelopFilterPresetControl.swift`'s pattern:
/// `Preset: [popup]`, the popup listing the built-in Default (`HdrToningPreset.builtIns`),
/// the user's presets, Custom (shown once the controls move off the chosen entry), Save Preset…
/// and Delete Preset… (a user preset only).
///
/// Choosing an entry hands the preset to `load`, and the sheet calls `markCustom()` on any later
/// change. Save and Delete write the library at once, independent of OK/Cancel: they are
/// preferences, not edits.
final class HdrToningPresetControl: NSObject {
    let row: NSView

    private let current: () -> HdrToningSettings
    private let load: (HdrToningPreset) -> Void
    private let popup = NSPopUpButton(frame: .zero, pullsDown: false)
    private var presets: [HdrToningPreset] = []
    /// The built-in the controls were loaded from (Default when the sheet opens).
    private var selectedBuiltIn: String? = HdrToningPreset.defaultName
    /// The user preset the controls were loaded from.
    private var selectedName: String?
    private var isCustom = false
    private var observer: NSObjectProtocol?

    private enum Tag: Int { case builtIn, preset, custom, save, delete }

    init(current: @escaping () -> HdrToningSettings, load: @escaping (HdrToningPreset) -> Void) {
        self.current = current
        self.load = load
        popup.font = DS.sans(13)
        popup.autoenablesItems = false
        popup.setAccessibilityIdentifier("hdr-toning-preset-popup")
        popup.widthAnchor.constraint(lessThanOrEqualToConstant: 240).isActive = true
        let stack = NSStackView(views: [fieldLabel("Preset:"), popup])
        stack.orientation = .horizontal
        stack.alignment = .centerY
        stack.spacing = 8
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
