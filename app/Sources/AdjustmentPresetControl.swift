import AppKit

/// The preset row in an adjustment dialog: `Preset: [popup]`, the popup
/// listing Default (the op's own defaults), the op's saved presets, Custom
/// (shown once the controls move off the chosen entry), then Save Preset…,
/// Rename Preset… and Delete Preset….
///
/// Choosing Default or a preset hands its payload to `load` — the dialog
/// replaces its controls and re-previews — and the popup then shows that
/// entry; the dialog calls `markCustom()` on any later change. Save, Rename
/// and Delete write the library at once, independent of Apply/Cancel: they
/// are preferences, not edits (no undo step, never an Action step). The
/// popup rebuilds whenever the library changes, in this window or another.
final class AdjustmentPresetControl: NSObject {
    let row: NSView

    private let op: AdjustmentLayerOp
    /// #1 phase 5: a CMYK document's dialog lists and saves CMYK presets only.
    private let cmyk: Bool
    private let current: () -> [String: Any]
    private let load: (AdjustmentLayerPayload) -> Void
    private let popup = NSPopUpButton(frame: .zero, pullsDown: false)
    private var presets: [AdjustmentPreset] = []
    /// The entry the controls were loaded from: nil for Default.
    private var selectedName: String?
    private var isCustom = false
    private var observer: NSObjectProtocol?

    private enum Tag: Int { case defaults, preset, custom, save, rename, delete }

    init(
        op: AdjustmentLayerOp, cmyk: Bool = false, current: @escaping () -> [String: Any],
        load: @escaping (AdjustmentLayerPayload) -> Void
    ) {
        self.op = op
        self.cmyk = cmyk
        self.current = current
        self.load = load
        popup.font = DS.sans(13)
        popup.autoenablesItems = false
        popup.setAccessibilityIdentifier("adjustment-preset-popup")
        popup.widthAnchor.constraint(lessThanOrEqualToConstant: 220).isActive = true
        let stack = NSStackView(views: [fieldLabel("Preset:"), popup])
        stack.orientation = .horizontal
        stack.alignment = .centerY
        stack.spacing = 8
        row = stack
        super.init()
        popup.target = self
        popup.action = #selector(popupChanged(_:))
        observer = NotificationCenter.default.addObserver(
            forName: AdjustmentPresetLibrary.didChange, object: nil, queue: .main
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
        presets = (try? AdjustmentPresetLibrary.presets(for: op, cmyk: cmyk)) ?? []
        if let name = selectedName, !presets.contains(where: { $0.name == name }) {
            selectedName = nil
            isCustom = true
        }
        popup.removeAllItems()
        guard let menu = popup.menu else { return }
        // Menu items, not `addItem(withTitle:)`, which drops a repeated
        // title — a preset named "Custom" would shift every row.
        menu.addItem(item("Default", .defaults))
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
        let named = selectedName != nil && !isCustom
        let rename = item("Rename Preset…", .rename)
        rename.isEnabled = named
        menu.addItem(rename)
        let delete = item("Delete Preset…", .delete)
        delete.isEnabled = named
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
            return entry.tag == Tag.defaults.rawValue
        }
        if let target { popup.select(target) }
    }

    @objc private func popupChanged(_ sender: Any?) {
        guard let chosen = popup.selectedItem, let tag = Tag(rawValue: chosen.tag) else { return }
        switch tag {
        case .defaults:
            load(AdjustmentLayerPayload(op: op, params: [:]))
            selectedName = nil
            isCustom = false
            rebuild()
        case .preset:
            guard let name = chosen.representedObject as? String,
                  let preset = presets.first(where: { $0.name == name })
            else { rebuild(); return }
            load(preset.payload)
            selectedName = name
            isCustom = false
            rebuild()
        case .custom:
            showSelection()
        case .save:
            showSelection()
            saveClicked()
        case .rename:
            showSelection()
            renameClicked()
        case .delete:
            showSelection()
            deleteClicked()
        }
    }

    private func saveClicked() {
        let params = current()
        let op = op, cmyk = cmyk
        guard let saved = AdjustmentPresetNameSheet.run(
            title: "Save \(op.displayName) Preset", confirm: "Save Preset", name: "",
            offersReplace: true,
            commit: { name, replace in
                try AdjustmentPresetLibrary.save(
                    AdjustmentPreset.validated(name: name, op: op, params: params, cmyk: cmyk),
                    replace: replace)
            })
        else { return }
        selectedName = saved.name
        isCustom = false
        rebuild()
    }

    private func renameClicked() {
        guard let old = selectedName else { NSSound.beep(); return }
        let op = op
        guard let renamed = AdjustmentPresetNameSheet.run(
            title: "Rename \(op.displayName) Preset", confirm: "Rename", name: old,
            offersReplace: false,
            commit: { name, _ in
                try AdjustmentPresetLibrary.rename(op: op, name: old, to: name)
            })
        else { return }
        // The library's change notice arrives inside the rename, while the
        // old name is gone: that rebuild turns the row to Custom, and the
        // renamed entry is the one the controls still show.
        selectedName = renamed.name
        isCustom = false
        rebuild()
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
            try AdjustmentPresetLibrary.delete(op: op, name: name)
            selectedName = nil
            isCustom = true
            rebuild()
        } catch {
            NSSound.beep()
            let failure = NSAlert()
            failure.messageText = "Could Not Delete the Preset"
            failure.informativeText = error.localizedDescription
            failure.runModal()
        }
    }
}

/// Names an adjustment preset — Save Preset… and Rename Preset… — in
/// `ProofPresetSaveSheet`'s shape: a modal card whose confirm button runs
/// `commit` and stays open with the refusal shown when it throws.
final class AdjustmentPresetNameSheet: NSObject {
    private let title: String
    private let confirm: String
    private let initialName: String
    private let offersReplace: Bool
    private let commit: (String, Bool) throws -> AdjustmentPreset
    private let name = NSTextField()
    private let replace = NSButton(
        checkboxWithTitle: "Replace a preset with this name", target: nil, action: nil)
    private let message = NSTextField(wrappingLabelWithString: "")
    private var modal: ModalCardWindowController?
    private var result: AdjustmentPreset?

    private init(
        title: String, confirm: String, name: String, offersReplace: Bool,
        commit: @escaping (String, Bool) throws -> AdjustmentPreset
    ) {
        self.title = title
        self.confirm = confirm
        self.initialName = name
        self.offersReplace = offersReplace
        self.commit = commit
        super.init()
    }

    /// The saved or renamed preset, or nil when the sheet was cancelled.
    static func run(
        title: String, confirm: String, name: String, offersReplace: Bool,
        commit: @escaping (String, Bool) throws -> AdjustmentPreset
    ) -> AdjustmentPreset? {
        AdjustmentPresetNameSheet(
            title: title, confirm: confirm, name: name, offersReplace: offersReplace,
            commit: commit
        ).run()
    }

    private func run() -> AdjustmentPreset? {
        DSField.style(name)
        name.stringValue = initialName
        name.placeholderString = "Preset name"
        name.setAccessibilityIdentifier("adjustment-preset-name")
        name.widthAnchor.constraint(equalToConstant: 330).isActive = true
        replace.font = DS.sans(12)
        replace.isHidden = !offersReplace
        message.font = DS.sans(11)
        message.textColor = DS.textMuted
        message.preferredMaxLayoutWidth = 340
        let content = NSStackView(views: [name, replace, message])
        content.orientation = .vertical
        content.alignment = .leading
        content.spacing = 10
        let hint = "A preset is a preference: it is saved for every document, and saving, "
            + "renaming or deleting one changes no picture and adds no undo step."
        let cancel = sheetCancelButton(target: self, action: #selector(cancel(_:)))
        let save = StickerButton(title: confirm, style: .primary,
                                 target: self, action: #selector(save(_:)))
        save.setAccessibilityIdentifier("adjustment-preset-name-confirm")
        modal = ModalCardWindowController(title: title, card: makeSheetView(
            title: title, hint: hint, content: content,
            buttonRow: makeButtonRow(cancel: cancel, apply: save)))
        _ = modal?.runModal()
        return result
    }

    @objc private func cancel(_ sender: Any?) { modal?.end(.cancel) }

    @objc private func save(_ sender: Any?) {
        do {
            result = try commit(name.stringValue, replace.state == .on)
            modal?.end(.OK)
        } catch {
            message.stringValue = error.localizedDescription
            NSSound.beep()
        }
    }
}
