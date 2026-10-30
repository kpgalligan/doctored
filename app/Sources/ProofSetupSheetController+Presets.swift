import AppKit

/// A profile row loaded from a file: the popup row's `representedObject`, so
/// reselecting the row after another choice restores ITS bytes, name and
/// path rather than leaving the sheet on the previous profile. `path` is
/// empty when the setup came with none.
final class LoadedProofProfile: NSObject {
    let data: Data
    let name: String
    let path: String

    init(data: Data, name: String, path: String) {
        self.data = data
        self.name = name
        self.path = path
        super.init()
    }
}

/// The Proof Setup sheet's saved-preset row — `Preset: [popup]
/// [Save…] [Delete]` and `Default for new windows` — and the profile-row
/// helper.
///
/// The library is read ONCE when the sheet opens. Save…, Delete and the
/// Default checkbox write the library at once, independent of Apply/Cancel:
/// they are preferences, not the window's proof. Choosing a preset loads the
/// controls and turns Proof Colors on; Apply still applies the window's setup.
extension ProofSetupSheetController {
    /// The preset the popup names now; nil on "Custom".
    var selectedPreset: ProofPreset? {
        let index = presetPopup.indexOfSelectedItem - 1
        return presets.indices.contains(index) ? presets[index] : nil
    }

    /// The one `entries()` read per sheet open, and the default's name.
    func loadPresets() {
        do {
            presets = try ProofPresetLibrary.entries()
            defaultPresetName = try ProofPresetLibrary.defaultPresetName()
            presetProblem = nil
        } catch {
            presets = []
            defaultPresetName = nil
            presetProblem = error.localizedDescription
        }
    }

    func makePresetRow() -> NSView {
        presetPopup.font = DS.sans(13)
        presetPopup.widthAnchor.constraint(lessThanOrEqualToConstant: 180).isActive = true
        presetPopup.target = self
        presetPopup.action = #selector(presetChanged(_:))
        presetPopup.setAccessibilityIdentifier("proof-preset-popup")
        rebuildPresetPopup()

        let save = StickerButton(title: "Save…", style: .secondary,
                                 target: self, action: #selector(savePresetClicked(_:)))
        save.setAccessibilityIdentifier("proof-preset-save")
        presetSaveButton = save
        let delete = StickerButton(title: "Delete", style: .secondary,
                                   target: self, action: #selector(deletePresetClicked(_:)))
        delete.setAccessibilityIdentifier("proof-preset-delete")
        presetDeleteButton = delete

        defaultBox.font = DS.sans(13)
        defaultBox.target = self
        defaultBox.action = #selector(defaultChanged(_:))
        defaultBox.setAccessibilityIdentifier("proof-preset-default")

        let row = NSStackView(views: [presetPopup, save, delete])
        row.orientation = .horizontal
        row.spacing = 8
        return row
    }

    private func rebuildPresetPopup() {
        presetPopup.removeAllItems()
        // `addItem(withTitle:)` drops an existing row with the same title, so
        // a preset named "Custom" would shift every index; menu items don't.
        presetPopup.menu?.addItem(NSMenuItem(title: "Custom", action: nil, keyEquivalent: ""))
        for preset in presets {
            presetPopup.menu?.addItem(NSMenuItem(title: preset.name, action: nil, keyEquivalent: ""))
        }
    }

    /// Puts the popup on the first preset the current controls match, else
    /// "Custom" — so a sheet opened on a preset's setup names it.
    func selectMatchingPreset() {
        let current = setup
        let index = presets.firstIndex { $0.matches(current) }.map { $0 + 1 } ?? 0
        presetPopup.selectItem(at: index)
        presetSelection = index
    }

    /// Save… needs a profile; Delete and Default need a saved preset.
    func syncPresetControls() {
        guard presetSaveButton != nil, presetDeleteButton != nil else { return }
        presetSaveButton.isEnabled = chosen != nil
        let selected = selectedPreset
        presetDeleteButton.isEnabled = selected != nil
        defaultBox.isEnabled = selected != nil
        let isDefault = selected.map { preset in
            defaultPresetName.map { Self.key($0) == Self.key(preset.name) } ?? false
        } ?? false
        defaultBox.state = isDefault ? .on : .off
    }

    /// Any control edit: the popup goes back to "Custom" unless the chosen
    /// preset still describes the controls.
    func controlsDidChange() {
        if let preset = selectedPreset, !preset.matches(setup) {
            presetPopup.selectItem(at: 0)
            presetSelection = 0
        }
        syncControls()
    }

    /// The profile-row helper: an installed profile selects its own row;
    /// any other gets the ONE file row (tag `loadTag - 1`), inserted the
    /// first time and reused after, carrying the profile it stands for.
    func selectProfileRow(data: Data, name: String, path: String?) {
        if let index = available.firstIndex(where: { $0.name == name }) {
            profilePopup.selectItem(withTag: index)
            return
        }
        let tag = Self.loadTag - 1
        let item: NSMenuItem
        if let existing = profilePopup.menu?.item(withTag: tag) {
            item = existing
            item.title = name
        } else {
            item = NSMenuItem(title: name, action: nil, keyEquivalent: "")
            item.tag = tag
            profilePopup.menu?.insertItem(item, at: 0)
        }
        item.representedObject = LoadedProofProfile(data: data, name: name, path: path ?? "")
        profilePopup.select(item)
    }

    /// Loads a resolved preset's setup into the controls.
    private func load(_ resolved: ProofSetup) {
        chosen = (resolved.profile, resolved.profileName, resolved.path)
        selectProfileRow(data: resolved.profile, name: resolved.profileName, path: resolved.path)
        intentPopup.selectItem(withTag: Int(resolved.intent.code))
        bpcBox.state = resolved.blackPointCompensation ? .on : .off
        paperBox.state = resolved.simulatePaperWhite ? .on : .off
        inkBox.state = resolved.simulateInkBlack ? .on : .off
        gamutWell.color = resolved.gamutColor
    }

    private static func key(_ name: String) -> String {
        PreferenceLibrary<ProofPresetCodec>.key(name)
    }

    private func presentPresetAlert(_ title: String, _ reason: String) {
        NSSound.beep()
        let alert = NSAlert()
        alert.messageText = title
        alert.informativeText = reason
        if let window = view.window {
            alert.beginSheetModal(for: window)
        } else {
            alert.runModal()
        }
    }

    // MARK: - Actions

    @objc func presetChanged(_ sender: Any?) {
        guard let preset = selectedPreset else {
            presetSelection = 0
            syncControls()
            return
        }
        do {
            load(try preset.resolvedSetup())
            proofBox.state = .on
            presetSelection = presetPopup.indexOfSelectedItem
            notice = nil
            syncControls()
        } catch {
            presetPopup.selectItem(at: presetSelection)
            syncControls()
            presentPresetAlert("Could Not Apply the Proof Preset", error.localizedDescription)
        }
    }

    @objc func savePresetClicked(_ sender: Any?) {
        guard let current = setup else {
            NSSound.beep()
            return
        }
        guard let saved = ProofPresetSaveSheet.run(setup: current) else { return }
        presets.removeAll { Self.key($0.name) == Self.key(saved.name) }
        presets.append(saved)
        presets.sort { $0.name.localizedStandardCompare($1.name) == .orderedAscending }
        rebuildPresetPopup()
        let index = (presets.firstIndex { $0.name == saved.name } ?? -1) + 1
        presetPopup.selectItem(at: index)
        presetSelection = index
        syncControls()
    }

    @objc func deletePresetClicked(_ sender: Any?) {
        guard let preset = selectedPreset else {
            NSSound.beep()
            return
        }
        do {
            try ProofPresetLibrary.delete(name: preset.name)
        } catch {
            presentPresetAlert("Could Not Delete the Proof Preset", error.localizedDescription)
            return
        }
        presets.removeAll { $0.name == preset.name }
        if let name = defaultPresetName, Self.key(name) == Self.key(preset.name) {
            defaultPresetName = nil
        }
        rebuildPresetPopup()
        presetPopup.selectItem(at: 0)
        presetSelection = 0
        syncControls()
    }

    @objc func defaultChanged(_ sender: Any?) {
        guard let preset = selectedPreset else {
            NSSound.beep()
            syncControls()
            return
        }
        let name = defaultBox.state == .on ? preset.name : nil
        do {
            try ProofPresetLibrary.setDefault(name: name)
            defaultPresetName = name
        } catch {
            presentPresetAlert("Could Not Change the Default Proof Preset",
                               error.localizedDescription)
        }
        syncControls()
    }
}

/// Save the Proof Setup sheet's current controls as a named proof preset.
/// A preset is a PREFERENCE: saving one
/// changes no picture and registers no undo step.
final class ProofPresetSaveSheet: NSObject {
    private let setup: ProofSetup
    private let name = NSTextField()
    private let replace = NSButton(
        checkboxWithTitle: "Replace a preset with this name", target: nil, action: nil)
    private let message = NSTextField(wrappingLabelWithString: "")
    private var modal: ModalCardWindowController?
    private var saved: ProofPreset?

    private init(setup: ProofSetup) {
        self.setup = setup
        super.init()
    }

    /// The saved preset, or nil when the sheet was cancelled.
    static func run(setup: ProofSetup) -> ProofPreset? {
        ProofPresetSaveSheet(setup: setup).run()
    }

    private func run() -> ProofPreset? {
        DSField.style(name)
        name.placeholderString = "Preset name"
        name.setAccessibilityIdentifier("proof-preset-name")
        name.widthAnchor.constraint(equalToConstant: 330).isActive = true
        replace.font = DS.sans(12)
        let summary = NSTextField(wrappingLabelWithString:
            "\(setup.profileName) · \(setup.intent.displayName)")
        summary.font = DS.sans(11)
        summary.preferredMaxLayoutWidth = 340
        message.font = DS.sans(11)
        message.textColor = DS.textMuted
        message.preferredMaxLayoutWidth = 340
        let content = NSStackView(views: [name, summary, replace, message])
        content.orientation = .vertical
        content.alignment = .leading
        content.spacing = 10
        var hint = "The profile's name, file and checksum are saved with the intent, the "
            + "simulate options and the warning colour — not the profile itself, and not "
            + "whether Proof Colors or Gamut Warning are on."
        if !ProofPreset.isInstalled(path: setup.path ?? "") {
            hint += " This profile is not installed; the preset will stop working if the "
                + "file moves."
        }
        let cancel = sheetCancelButton(target: self, action: #selector(cancel(_:)))
        let save = StickerButton(title: "Save Preset", style: .primary,
                                 target: self, action: #selector(save(_:)))
        save.setAccessibilityIdentifier("proof-preset-save-confirm")
        modal = ModalCardWindowController(title: "Save Proof Preset", card: makeSheetView(
            title: "Save Proof Preset", hint: hint,
            content: content, buttonRow: makeButtonRow(cancel: cancel, apply: save)))
        _ = modal?.runModal()
        return saved
    }

    @objc private func cancel(_ sender: Any?) { modal?.end(.cancel) }

    @objc private func save(_ sender: Any?) {
        do {
            let preset = try ProofPreset(name: name.stringValue, setup: setup)
            saved = try ProofPresetLibrary.save(preset, replace: replace.state == .on)
            modal?.end(.OK)
        } catch {
            message.stringValue = error.localizedDescription
            NSSound.beep()
        }
    }
}
