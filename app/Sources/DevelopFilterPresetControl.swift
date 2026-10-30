import AppKit

/// Filters ▸ Develop Filter…'s preset row, in
/// `AdjustmentPresetControl.swift`'s pattern: `Preset: [popup]`, the popup
/// listing Default (every control 0), the filter's own presets, then under a
/// "Develop presets" heading the Develop RAW presets (read-only,
/// `RawPresetLibrary`), Custom (shown once the controls move off the chosen
/// entry), Save Preset… and Delete Preset….
///
/// Choosing an entry hands its controls to `load` with a status sentence (nil
/// for the filter's own entries; for a Develop preset, which keys it took and
/// which it ignored by name, `DevelopFilterControls.developTransfer`), and
/// the panel calls `markCustom()` on any later change. Save and Delete write
/// the filter's library at once, independent of Apply/Cancel: they are
/// preferences, not edits. Develop presets are never written from here.
final class DevelopFilterPresetControl: NSObject {
    let row: NSView

    private let current: () -> DevelopFilterControls
    private let load: (DevelopFilterControls, String?) -> Void
    private let popup = NSPopUpButton(frame: .zero, pullsDown: false)
    private var presets: [DevelopFilterPreset] = []
    private var developPresets: [RawPreset] = []
    /// The filter preset the controls were loaded from: nil for Default or a
    /// Develop preset.
    private var selectedName: String?
    private var selectedDevelop: String?
    private var isCustom = false
    private var observers: [NSObjectProtocol] = []

    private enum Tag: Int { case defaults, preset, develop, heading, custom, save, delete }

    init(
        current: @escaping () -> DevelopFilterControls,
        load: @escaping (DevelopFilterControls, String?) -> Void
    ) {
        self.current = current
        self.load = load
        popup.font = DS.sans(13)
        popup.autoenablesItems = false
        popup.setAccessibilityIdentifier("develop-filter-preset-popup")
        popup.widthAnchor.constraint(lessThanOrEqualToConstant: 220).isActive = true
        let stack = NSStackView(views: [fieldLabel("Preset:"), popup])
        stack.orientation = .horizontal
        stack.alignment = .centerY
        stack.spacing = 8
        row = stack
        super.init()
        popup.target = self
        popup.action = #selector(popupChanged(_:))
        for name in [DevelopFilterPresetLibrary.didChange, RawPresetLibrary.didChange] {
            observers.append(NotificationCenter.default.addObserver(
                forName: name, object: nil, queue: .main
            ) { [weak self] _ in self?.rebuild() })
        }
        rebuild()
    }

    deinit {
        for observer in observers { NotificationCenter.default.removeObserver(observer) }
    }

    /// The controls moved off the loaded entry.
    func markCustom() {
        guard !isCustom else { return }
        isCustom = true
        rebuild()
    }

    /// The controls were reset to every control 0, which is the Default entry.
    func selectDefault() {
        selected(name: nil, develop: nil)
    }

    // MARK: - The popup

    private func rebuild() {
        presets = (try? DevelopFilterPresetLibrary.entries()) ?? []
        developPresets = (try? RawPresetLibrary.entries()) ?? []
        if let name = selectedName, !presets.contains(where: { $0.name == name }) {
            selectedName = nil
            isCustom = true
        }
        if let name = selectedDevelop, !developPresets.contains(where: { $0.name == name }) {
            selectedDevelop = nil
            isCustom = true
        }
        popup.removeAllItems()
        guard let menu = popup.menu else { return }
        // Menu items, not `addItem(withTitle:)`, which drops a repeated title.
        menu.addItem(item("Default", .defaults))
        for preset in presets {
            let entry = item(preset.name, .preset)
            entry.representedObject = preset.name
            menu.addItem(entry)
        }
        if !developPresets.isEmpty {
            menu.addItem(.separator())
            let heading = item("Develop presets", .heading)
            heading.isEnabled = false
            menu.addItem(heading)
            for preset in developPresets {
                let entry = item(preset.name, .develop)
                entry.representedObject = preset.name
                entry.indentationLevel = 1
                menu.addItem(entry)
            }
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
                return entry.tag == Tag.preset.rawValue && entry.representedObject as? String == selectedName
            }
            if let selectedDevelop {
                return entry.tag == Tag.develop.rawValue && entry.representedObject as? String == selectedDevelop
            }
            return entry.tag == Tag.defaults.rawValue
        }
        if let target { popup.select(target) }
    }

    @objc private func popupChanged(_ sender: Any?) {
        guard let chosen = popup.selectedItem, let tag = Tag(rawValue: chosen.tag) else { return }
        switch tag {
        case .defaults:
            load(DevelopFilterControls(), nil)
            selected(name: nil, develop: nil)
        case .preset:
            guard let name = chosen.representedObject as? String,
                  let preset = presets.first(where: { $0.name == name })
            else { rebuild(); return }
            load(preset.controls, nil)
            selected(name: name, develop: nil)
        case .develop:
            guard let name = chosen.representedObject as? String,
                  let preset = developPresets.first(where: { $0.name == name })
            else { rebuild(); return }
            let transfer = DevelopFilterControls.developTransfer(preset)
            load(transfer.controls, Self.transferStatus(name: name, taken: transfer.taken, ignored: transfer.ignored))
            selected(name: nil, develop: name)
        case .heading, .custom:
            showSelection()
        case .save:
            showSelection()
            saveClicked()
        case .delete:
            showSelection()
            deleteClicked()
        }
    }

    private func selected(name: String?, develop: String?) {
        selectedName = name
        selectedDevelop = develop
        isCustom = false
        rebuild()
    }

    /// The status line a Develop preset leaves: what the filter took and,
    /// by name, what it has no control for.
    static func transferStatus(name: String, taken: [String], ignored: [String]) -> String {
        let took = taken.isEmpty ? "took no control" : "took " + taken.joined(separator: ", ")
        let left = ignored.isEmpty ? "" : "; ignored " + ignored.joined(separator: ", ")
        return "Develop preset “\(name)” \(took)\(left)."
    }

    private func saveClicked() {
        let controls = current()
        guard let name = Self.askName() else { return }
        do {
            let saved = try DevelopFilterPresetLibrary.save(name: name, controls: controls, replace: false)
            selected(name: saved.name, develop: nil)
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
            try DevelopFilterPresetLibrary.delete(name: name)
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
        alert.messageText = "Save Develop Filter Preset"
        alert.informativeText = "A preset is a preference: it is saved for every document, and "
            + "saving or deleting one changes no picture and adds no undo step."
        let field = NSTextField(frame: NSRect(x: 0, y: 0, width: 280, height: 24))
        DSField.style(field)
        field.placeholderString = "Preset name"
        field.setAccessibilityIdentifier("develop-filter-preset-name")
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
