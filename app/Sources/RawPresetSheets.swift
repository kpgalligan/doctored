import AppKit

final class RawSettingsFieldPicker {
    let view: NSStackView
    private var buttons: [RawSettingsField: NSButton] = [:]
    init(capabilities: RawCapabilities, recipe: RawDevelopmentRecipe? = nil) {
        view = NSStackView()
        view.orientation = .vertical
        view.alignment = .leading
        view.spacing = 5
        for field in RawSettingsField.allCases {
            let button = NSButton(checkboxWithTitle: field.title, target: nil, action: nil)
            button.font = DS.sans(12)
            button.isEnabled = field.supported(by: capabilities, recipe: recipe)
            button.state = [.exposure, .whiteBalance].contains(field) ? .on : .off
            button.setAccessibilityIdentifier("raw-field-" + field.rawValue)
            buttons[field] = button
            view.addArrangedSubview(button)
        }
    }
    var fields: [RawSettingsField] {
        RawSettingsField.allCases.filter { buttons[$0]?.state == .on && buttons[$0]?.isEnabled == true }
    }
}

final class RawPresetSaveSheet: NSObject {
    private let values: RawDevelopValues
    private let capabilities: RawCapabilities
    private let outputSpace: RawOutputSpace
    private let recipe: RawDevelopmentRecipe?
    private let fields: RawSettingsFieldPicker
    private let name = NSTextField()
    private let replace = NSButton(checkboxWithTitle: "Replace a preset with this name", target: nil, action: nil)
    private let message = NSTextField(wrappingLabelWithString: "")
    private var modal: ModalCardWindowController?
    private init(values: RawDevelopValues, capabilities: RawCapabilities, outputSpace: RawOutputSpace,
                 recipe: RawDevelopmentRecipe?) {
        self.values = values; self.capabilities = capabilities; self.outputSpace = outputSpace
        self.recipe = recipe
        fields = RawSettingsFieldPicker(capabilities: capabilities, recipe: recipe)
        super.init()
    }
    @discardableResult
    static func run(values: RawDevelopValues, capabilities: RawCapabilities, outputSpace: RawOutputSpace,
                    recipe: RawDevelopmentRecipe? = nil) -> Bool {
        RawPresetSaveSheet(values: values, capabilities: capabilities, outputSpace: outputSpace, recipe: recipe).run()
    }
    private func run() -> Bool {
        DSField.style(name)
        name.placeholderString = "Preset name"
        name.setAccessibilityIdentifier("raw-preset-name")
        name.widthAnchor.constraint(equalToConstant: 330).isActive = true
        replace.font = DS.sans(12)
        message.font = DS.sans(11); message.textColor = DS.textMuted
        message.preferredMaxLayoutWidth = 340
        let content = NSStackView(views: [name, fields.view, replace, message])
        content.orientation = .vertical; content.alignment = .leading; content.spacing = 10
        let cancel = sheetCancelButton(target: self, action: #selector(cancel(_:)))
        let save = StickerButton(title: "Save Preset", style: .primary, target: self, action: #selector(save(_:)))
        save.setAccessibilityIdentifier("raw-preset-save")
        modal = ModalCardWindowController(title: "Save RAW Preset", card: makeSheetView(
            title: "Save RAW Preset", hint: "Only selected controls are saved. As Shot uses each receiving camera’s own captured white balance.",
            content: content, buttonRow: makeButtonRow(cancel: cancel, apply: save)))
        return modal?.runModal() == .OK
    }
    @objc private func cancel(_ sender: Any?) { modal?.end(.cancel) }
    @objc private func save(_ sender: Any?) {
        do {
            let settings = try RawSettingsPatch.capture(values: values, capabilities: capabilities,
                                                       outputSpace: outputSpace, fields: fields.fields, recipe: recipe)
            try RawPresetLibrary.save(name: name.stringValue, settings: settings, replace: replace.state == .on)
            modal?.end(.OK)
        } catch { message.stringValue = error.localizedDescription; NSSound.beep() }
    }
}

final class RawPresetManagerSheet: NSObject {
    private let descriptor: RawSourceDescriptor
    private var presets: [RawPreset] = []
    private let popup = NSPopUpButton()
    private let message = NSTextField(wrappingLabelWithString: "")
    private var modal: ModalCardWindowController?
    private var selected: RawPreset?
    private var applyButton: NSButton?
    private var deleteButton: NSButton?
    private var brokenSection: BrokenLibraryFilesSection?
    private init(descriptor: RawSourceDescriptor) { self.descriptor = descriptor; super.init() }
    static func run(descriptor: RawSourceDescriptor) -> RawPreset? { Self(descriptor: descriptor).run() }
    private func run() -> RawPreset? {
        popup.widthAnchor.constraint(equalToConstant: 330).isActive = true
        popup.setAccessibilityIdentifier("raw-preset-manager-list")
        let save = StickerButton(title: "Save Current…", style: .secondary, target: self, action: #selector(saveCurrent(_:)))
        let delete = StickerButton(title: "Delete", style: .secondary, target: self, action: #selector(deleteCurrent(_:)))
        deleteButton = delete
        let actions = NSStackView(views: [save, delete]); actions.spacing = 8
        // The bad files, listed rather than hidden behind a refusal of the
        // whole library: each row names its file and why, and its two buttons
        // are the UI twins of repair_raw_preset and delete_raw_preset {file}.
        let broken = BrokenLibraryFilesSection(
            title: "Files that are not presets:", identifierPrefix: "raw-preset",
            delete: { try RawPresetLibrary.delete(file: $0) },
            repair: { _ = try RawPresetLibrary.repair(file: $0) },
            changed: { [weak self] in self?.reload() },
            report: { [weak self] in self?.message.stringValue = $0 })
        brokenSection = broken
        message.font = DS.sans(11); message.textColor = DS.textMuted; message.preferredMaxLayoutWidth = 340
        let content = NSStackView(views: [popup, actions, broken.view, message])
        content.orientation = .vertical; content.alignment = .leading; content.spacing = 12
        let cancel = sheetCancelButton(target: self, action: #selector(cancel(_:)))
        let apply = sheetApplyButton(target: self, action: #selector(apply(_:)))
        applyButton = apply
        reload()
        modal = ModalCardWindowController(title: "RAW Presets", card: makeSheetView(title: "RAW Presets",
            hint: "Apply a preset’s saved controls to the active RAW layer. Other settings stay as saved. Actions record resolved settings, so later preset changes do not affect replay.",
            content: content, buttonRow: makeButtonRow(cancel: cancel, apply: apply)))
        _ = modal?.runModal()
        return selected
    }
    private func reload() {
        do {
            let listing = try RawPresetLibrary.listing()
            presets = listing.entries.map(\.preset)
            popup.removeAllItems(); popup.addItems(withTitles: presets.map(\.name))
            applyButton?.isEnabled = !presets.isEmpty; deleteButton?.isEnabled = !presets.isEmpty
            message.stringValue = presets.isEmpty && listing.broken.isEmpty ? "No presets saved yet." : ""
            if brokenSection?.update(listing.broken) == true { modal?.refit() }
        } catch {
            presets = []
            popup.removeAllItems()
            applyButton?.isEnabled = false; deleteButton?.isEnabled = false
            if brokenSection?.update([]) == true { modal?.refit() }
            message.stringValue = error.localizedDescription
        }
    }
    @objc private func saveCurrent(_ sender: Any?) {
        guard let output = RawOutputSpace(policy: descriptor.recipe.outputSpace) else { return }
        RawPresetSaveSheet.run(values: descriptor.recipe.values, capabilities: descriptor.recipe.capabilities,
            outputSpace: output, recipe: descriptor.recipe)
        reload()
    }
    @objc private func deleteCurrent(_ sender: Any?) {
        guard presets.indices.contains(popup.indexOfSelectedItem) else { return }
        do { try RawPresetLibrary.delete(name: presets[popup.indexOfSelectedItem].name); reload() }
        catch { message.stringValue = error.localizedDescription; NSSound.beep() }
    }
    @objc private func apply(_ sender: Any?) {
        guard presets.indices.contains(popup.indexOfSelectedItem) else { return }
        selected = presets[popup.indexOfSelectedItem]; modal?.end(.OK)
    }
    @objc private func cancel(_ sender: Any?) { modal?.end(.cancel) }
}

final class RawSyncSheet: NSObject {
    private let targets: [RawSettingsTarget]
    private let fields: RawSettingsFieldPicker
    private var checks: [NSButton] = []
    private let masking = NSButton(checkboxWithTitle: "Masking (replaces each target’s local adjustments)",
                                   target: nil, action: nil)
    private var modal: ModalCardWindowController?
    private var result: (fields: [RawSettingsField], locals: Bool, targets: [RawSettingsTarget])?
    private let message = NSTextField(wrappingLabelWithString: "")
    private init(targets: [RawSettingsTarget], capabilities: RawCapabilities, recipe: RawDevelopmentRecipe?) {
        self.targets = targets; fields = RawSettingsFieldPicker(capabilities: capabilities, recipe: recipe); super.init()
    }
    static func run(targets: [RawSettingsTarget], capabilities: RawCapabilities, recipe: RawDevelopmentRecipe? = nil)
        -> (fields: [RawSettingsField], locals: Bool, targets: [RawSettingsTarget])? {
        Self(targets: targets, capabilities: capabilities, recipe: recipe).run()
    }
    private func run() -> (fields: [RawSettingsField], locals: Bool, targets: [RawSettingsTarget])? {
        let rows = NSStackView(); rows.orientation = .vertical; rows.alignment = .leading; rows.spacing = 4
        for target in targets {
            let name = target.document.doc?.layerInfo(target.layer)?.name ?? "Layer \(target.layer)"
            let button = NSButton(checkboxWithTitle: "\(target.document.displayName ?? "Untitled") — \(name)", target: nil, action: nil)
            button.font = DS.sans(12); button.setAccessibilityIdentifier("raw-sync-target-\(checks.count)")
            checks.append(button); rows.addArrangedSubview(button)
        }
        let scroll = NSScrollView(); scroll.hasVerticalScroller = true; scroll.documentView = rows
        rows.translatesAutoresizingMaskIntoConstraints = false
        rows.widthAnchor.constraint(equalTo: scroll.contentView.widthAnchor).isActive = true
        scroll.widthAnchor.constraint(equalToConstant: 380).isActive = true
        scroll.heightAnchor.constraint(equalToConstant: min(150, CGFloat(targets.count) * 24 + 8)).isActive = true
        message.font = DS.sans(11); message.textColor = DS.textMuted; message.preferredMaxLayoutWidth = 380
        masking.font = DS.sans(12); masking.state = .off; masking.setAccessibilityIdentifier("raw-sync-locals")
        let content = NSStackView(views: [fieldLabel("Settings to copy:"), fields.view, masking,
                                          fieldLabel("Receiving RAW layers:"), scroll, message])
        content.orientation = .vertical; content.alignment = .leading; content.spacing = 8
        modal = ModalCardWindowController(title: "Synchronize RAW Settings", card: makeSheetView(
            title: "Synchronize RAW Settings", hint: "Choose up to 32 targets. As Shot uses each target’s own captured white balance. Masking replaces each target’s local adjustments with these, in order; Subject, People and Sky are detected again on each target. Every result is prepared before changes; Undo stays separate for each document.",
            content: content, buttonRow: makeButtonRow(cancel: sheetCancelButton(target: self, action: #selector(cancel(_:))),
                apply: sheetApplyButton(target: self, action: #selector(apply(_:)))), width: 440))
        _ = modal?.runModal()
        return result
    }
    @objc private func apply(_ sender: Any?) {
        let selected = zip(targets, checks).filter { $0.1.state == .on }.map(\.0)
        let locals = masking.state == .on
        guard !fields.fields.isEmpty || locals, !selected.isEmpty,
              selected.count <= RawSettingsSynchronization.maximumTargets else {
            message.stringValue = "Choose settings, Masking or both, and 1–32 receiving RAW layers."; NSSound.beep(); return
        }
        result = (fields.fields, locals, selected); modal?.end(.OK)
    }
    @objc private func cancel(_ sender: Any?) { modal?.end(.cancel) }
}
