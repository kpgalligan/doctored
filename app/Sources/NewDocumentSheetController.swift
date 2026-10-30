import AppKit

// The New Document card's Save Preset… (the nested `NewDocumentPresetSaveSheet`,
// `ExportPresetSaveSheet`'s shape; the window-title name is not part of a preset) and Delete Preset
// (enabled only on a listed preset, no confirmation) are the UI twins of `save_document_preset` and
// `delete_document_preset {name}`.

/// File ▸ New… — the modal card that collects a `NewDocumentRequest`.
///
/// A `ModalCardWindowController` rather than a document-modal sheet, for
/// `ActionParametersSheet`'s reason: there is no document to attach to yet.
///
/// It decides NOTHING about what is legal: every field goes through
/// `NewDocumentRequest`/`NewDocumentProfile`/`NewDocumentBackground`, so the
/// sheet, the MCP tool and a preset are refused by exactly the same
/// sentences. The depth popup offers 8, 16 and 32 bits/channel (float:
/// the 16-bit blank converted into float). Gray is simply not offered — it is
/// a refusal by name in the model, and a popup entry that always failed would
/// be a promise the core cannot keep.
final class NewDocumentSheetController: NSObject {
    private let nameField = NSTextField()
    private let widthField = NSTextField()
    private let heightField = NSTextField()
    private let resolutionField = NSTextField()
    private let unitPopup = NSPopUpButton(frame: .zero, pullsDown: false)
    private let profilePopup = NSPopUpButton(frame: .zero, pullsDown: false)
    /// RGB Color / CMYK Color (#1 phase 5).
    private let colorModePopup = NSPopUpButton(frame: .zero, pullsDown: false)
    /// The installed CMYK profiles the CMYK profile popup lists after Working CMYK.
    private var cmykProfiles: [InstalledProfile] = []
    private let depthPopup = NSPopUpButton(frame: .zero, pullsDown: false)
    private let backgroundPopup = NSPopUpButton(frame: .zero, pullsDown: false)
    private let presetPopup = NSPopUpButton(frame: .zero, pullsDown: false)
    private let pixelLabel = NSTextField(labelWithString: "")
    private let message = NSTextField(wrappingLabelWithString: "")
    private var modal: ModalCardWindowController?
    private var result: NewDocumentRequest?
    /// The presets the popup lists, in its own order, so a selection needs no
    /// name lookup.
    private var presets: [NewDocumentPreset] = []
    private var brokenSection: BrokenLibraryFilesSection?
    private var deletePresetButton: NSButton?
    /// The shared colour panel's target is unretained and this controller is a
    /// temporary, so a pick after the card closes would message a freed
    /// object. There is no target getter, so the guard is this flag — set when
    /// the card took the panel over. `EditorViewController.deinit` does the
    /// same for the rail swatches.
    private var colorPanelTargetsSelf = false

    private override init() {
        super.init()
    }

    deinit {
        if colorPanelTargetsSelf {
            NSColorPanel.shared.setTarget(nil)
            NSColorPanel.shared.setAction(nil)
        }
    }

    /// Runs the card and answers the request, or nil when the user cancelled.
    static func run() -> NewDocumentRequest? {
        NewDocumentSheetController().present()
    }

    private func present() -> NewDocumentRequest? {
        DSField.style(nameField)
        nameField.placeholderString = "Untitled"
        nameField.widthAnchor.constraint(equalToConstant: 220).isActive = true

        for field in [widthField, heightField, resolutionField] {
            DSField.style(field)
            field.widthAnchor.constraint(equalToConstant: 90).isActive = true
            field.target = self
            field.action = #selector(fieldsChanged(_:))
        }
        widthField.stringValue = "8"
        heightField.stringValue = "10"
        resolutionField.stringValue = "300"

        for unit in NewDocumentUnit.allCases {
            unitPopup.addItem(withTitle: unit.displayName)
            unitPopup.lastItem?.tag = NewDocumentUnit.allCases.firstIndex(of: unit) ?? 0
        }
        unitPopup.selectItem(at: NewDocumentUnit.allCases.firstIndex(of: .inches) ?? 0)
        unitPopup.target = self
        unitPopup.action = #selector(fieldsChanged(_:))

        depthPopup.addItems(withTitles: ["8 bits/channel", "16 bits/channel", "32 bits/channel (float)"])
        for mode in NewDocumentColorMode.allCases { colorModePopup.addItem(withTitle: mode.displayName) }
        colorModePopup.target = self
        colorModePopup.action = #selector(colorModeChanged(_:))
        colorModePopup.setAccessibilityIdentifier("new-document-color-mode")
        fillProfiles()
        profilePopup.target = self
        profilePopup.action = #selector(profileChanged(_:))
        profilePopup.setAccessibilityIdentifier("new-document-profile")

        backgroundPopup.addItems(
            withTitles: ["Transparent", "White", "Black", "Custom Colour…"])
        backgroundPopup.target = self
        backgroundPopup.action = #selector(backgroundChanged(_:))

        presetPopup.target = self
        presetPopup.action = #selector(presetChanged(_:))

        pixelLabel.font = DS.mono(11)
        pixelLabel.textColor = DS.textMuted
        message.font = DS.sans(11)
        message.textColor = DS.textMuted
        message.preferredMaxLayoutWidth = 360

        // The library's bad files, under the popup that lists its good ones.
        // Delete File is the UI twin of delete_document_preset {file}; there
        // is no repair outside RAW.
        let broken = BrokenLibraryFilesSection(
            title: "Files that are not presets:", identifierPrefix: "new-document-preset",
            delete: { try NewDocumentPresetLibrary.delete(file: $0) }, repair: nil,
            changed: { [weak self] in self?.reloadPresets() },
            report: { [weak self] in self?.message.stringValue = $0 })
        brokenSection = broken
        // The UI twins of save_document_preset and delete_document_preset {name}.
        let savePreset = StickerButton(
            title: "Save Preset…", style: .secondary, target: self,
            action: #selector(savePresetClicked(_:)))
        savePreset.setAccessibilityIdentifier("new-document-save-preset")
        let deletePreset = StickerButton(
            title: "Delete Preset", style: .secondary, target: self,
            action: #selector(deletePresetClicked(_:)))
        deletePreset.setAccessibilityIdentifier("new-document-delete-preset")
        deletePresetButton = deletePreset
        let content = NSStackView(views: [
            row("Preset", presetPopup),
            row("", savePreset, deletePreset),
            broken.view,
            row("Name", nameField),
            row("Width", widthField, unitPopup),
            row("Height", heightField),
            row("Resolution", resolutionField, NSTextField(labelWithString: "ppi")),
            row("Color Mode", colorModePopup),
            row("Profile", profilePopup),
            row("Depth", depthPopup),
            row("Background", backgroundPopup),
            pixelLabel,
            message,
        ])
        content.orientation = .vertical
        content.alignment = .leading
        content.spacing = 8
        refreshPixelLabel()
        reloadPresets()

        let cancel = sheetCancelButton(target: self, action: #selector(cancelClicked(_:)))
        let create = StickerButton(
            title: "Create", style: .primary, target: self, action: #selector(createClicked(_:)))
        create.setAccessibilityIdentifier("new-document-create")
        modal = ModalCardWindowController(
            title: "New Document",
            card: makeSheetView(
                title: "New Document",
                hint: "Pixels never change with the resolution — only the print size does. "
                    + "A 32-bit float document holds linear light: its background is the "
                    + "colour's value decoded through the profile's tone curve. A CMYK document "
                    + "holds ink in its CMYK profile at 8 or 16 bits: white is bare paper.",
                content: content, buttonRow: makeButtonRow(cancel: cancel, apply: create),
                width: 460))
        guard modal?.runModal() == .OK else { return nil }
        return result
    }

    /// The Preset popup and the broken-file section, from ONE listing: "No
    /// Preset" plus every listed name, keeping the selected title when it is
    /// still listed. A library-level fault leaves the popup with no presets,
    /// hides the section and puts the sentence in the message line.
    private func reloadPresets() {
        let kept = presetPopup.titleOfSelectedItem
        var broken: [PreferenceLibraryBroken] = []
        do {
            let listing = try NewDocumentPresetLibrary.listing()
            presets = listing.entries.map(\.item)
            broken = listing.broken
            message.stringValue = ""
        } catch {
            presets = []
            message.stringValue = error.localizedDescription
        }
        presetPopup.removeAllItems()
        presetPopup.addItem(withTitle: "No Preset")
        for preset in presets { presetPopup.addItem(withTitle: preset.name) }
        if let kept, presetPopup.item(withTitle: kept) != nil { presetPopup.selectItem(withTitle: kept) }
        updateDeletePresetButton()
        if brokenSection?.update(broken) == true { modal?.refit() }
    }

    /// Delete Preset acts on a LISTED preset only; "No Preset" has nothing
    /// to delete, and a broken file has its own Delete File.
    private func updateDeletePresetButton() {
        deletePresetButton?.isEnabled = presets.indices.contains(presetPopup.indexOfSelectedItem - 1)
    }

    private func row(_ title: String, _ views: NSView...) -> NSStackView {
        let label = NSTextField(labelWithString: title)
        label.font = DS.sans(12)
        label.alignment = .right
        label.widthAnchor.constraint(equalToConstant: 90).isActive = true
        let stack = NSStackView(views: [label] + views)
        stack.orientation = .horizontal
        stack.spacing = 8
        stack.alignment = .firstBaseline
        return stack
    }

    // MARK: - Live state

    private var unit: NewDocumentUnit {
        let tag = unitPopup.selectedItem?.tag ?? 0
        return NewDocumentUnit.allCases.indices.contains(tag)
            ? NewDocumentUnit.allCases[tag] : .px
    }

    /// Set only after the user picks a file or a colour; the popup's other
    /// entries answer from their own titles.
    private var chosenProfilePath: String?
    private var chosenColor: NewDocumentBackground?

    private var colorMode: NewDocumentColorMode {
        let modes = NewDocumentColorMode.allCases
        let index = colorModePopup.indexOfSelectedItem
        return modes.indices.contains(index) ? modes[index] : .rgb
    }

    /// The profile popup for the colour mode: sRGB, Display P3 and Profile File… for RGB;
    /// Working CMYK, the installed CMYK profiles and Profile File… for CMYK; Working Gray, the
    /// eight Gray spaces and Profile File… for Grayscale (#1 phase 1a).
    private func fillProfiles() {
        profilePopup.removeAllItems()
        switch colorMode {
        case .rgb:
            profilePopup.addItems(withTitles: ["sRGB", "Display P3", "Profile File…"])
        case .cmyk:
            cmykProfiles = InstalledProfiles.proofable.filter { $0.description.space == "cmyk" }
            profilePopup.addItem(withTitle: WorkingCMYK.installedName.map { "Working CMYK — \($0)" }
                ?? "Working CMYK")
            profilePopup.addItems(withTitles: cmykProfiles.map(\.name))
            profilePopup.addItem(withTitle: "Profile File…")
        case .gray:
            profilePopup.addItem(withTitle: "Working Gray — \(ColorSettings.workingGray.displayName)")
            profilePopup.addItems(withTitles: GrayWorkingSpace.allCases.map(\.displayName))
            profilePopup.addItem(withTitle: "Profile File…")
        case .lab:
            // #1 Lab phase 1: one profile, the core's Lab twin (D50).
            profilePopup.addItem(withTitle: "Lab Color (D50)")
        }
        chosenProfilePath = nil
        profilePopup.selectItem(at: 0)
        // A CMYK or Lab Color document is 8 or 16 bits.
        let noFloat = colorMode == .cmyk || colorMode == .lab
        depthPopup.item(at: 2)?.isHidden = noFloat
        if noFloat, depthPopup.indexOfSelectedItem == 2 { depthPopup.selectItem(at: 1) }
    }

    private var isProfileFileRow: Bool {
        profilePopup.indexOfSelectedItem == profilePopup.numberOfItems - 1
    }

    private var profile: NewDocumentProfile {
        let index = profilePopup.indexOfSelectedItem
        if colorMode == .lab { return .lab }
        if isProfileFileRow {
            return chosenProfilePath.map { .file($0) }
                ?? (colorMode == .cmyk ? .workingCMYK : colorMode == .gray ? .workingGray : .srgb)
        }
        switch colorMode {
        case .rgb:
            return index == 1 ? .displayP3 : .srgb
        case .cmyk:
            return cmykProfiles.indices.contains(index - 1) ? .file(cmykProfiles[index - 1].url.path)
                : .workingCMYK
        case .gray:
            let spaces = GrayWorkingSpace.allCases
            return spaces.indices.contains(index - 1) ? .graySpace(spaces[index - 1]) : .workingGray
        case .lab:
            return .lab
        }
    }

    @objc private func colorModeChanged(_ sender: Any?) {
        fillProfiles()
        refreshPixelLabel()
    }

    private var background: NewDocumentBackground {
        switch backgroundPopup.indexOfSelectedItem {
        case 1: return .white
        case 2: return .black
        case 3: return chosenColor ?? .white
        default: return .transparent
        }
    }

    @objc private func fieldsChanged(_ sender: Any?) { refreshPixelLabel() }

    private func refreshPixelLabel() {
        guard let request = try? buildRequest() else {
            pixelLabel.stringValue = ""
            return
        }
        pixelLabel.stringValue = "\(request.width) × \(request.height) px"
        message.stringValue = ""
    }

    @objc private func presetChanged(_ sender: Any?) {
        updateDeletePresetButton()
        let index = presetPopup.indexOfSelectedItem - 1
        guard presets.indices.contains(index) else { return }
        let preset = presets[index]
        unitPopup.selectItem(at: NewDocumentUnit.allCases.firstIndex(of: preset.unit) ?? 0)
        resolutionField.stringValue = PrintSize.ppiText(preset.request.ppi)
        widthField.stringValue = lengthText(preset.request.length(preset.request.width, in: preset.unit))
        heightField.stringValue = lengthText(preset.request.length(preset.request.height, in: preset.unit))
        colorModePopup.selectItem(at: NewDocumentColorMode.allCases.firstIndex(of: preset.request.colorMode) ?? 0)
        fillProfiles()
        depthPopup.selectItem(at: preset.request.depth == 32 ? 2 : preset.request.depth == 16 ? 1 : 0)
        switch preset.request.profile {
        case .srgb, .workingCMYK, .workingGray, .lab: profilePopup.selectItem(at: 0)
        case .displayP3: profilePopup.selectItem(at: 1)
        case let .graySpace(space):
            profilePopup.selectItem(at: (GrayWorkingSpace.allCases.firstIndex(of: space) ?? -1) + 1)
        case let .file(path):
            if let index = cmykProfiles.firstIndex(where: { $0.url.path == path }) {
                profilePopup.selectItem(at: index + 1)
            } else {
                chosenProfilePath = path
                profilePopup.selectItem(at: profilePopup.numberOfItems - 1)
            }
        }
        switch preset.request.background {
        case .transparent: backgroundPopup.selectItem(at: 0)
        case .white: backgroundPopup.selectItem(at: 1)
        case .black: backgroundPopup.selectItem(at: 2)
        case .color:
            chosenColor = preset.request.background
            backgroundPopup.selectItem(at: 3)
        }
        refreshPixelLabel()
    }

    private func lengthText(_ value: Double) -> String {
        abs(value - value.rounded()) < 0.0001 ? String(Int(value.rounded())) : String(format: "%g", value)
    }

    @objc private func profileChanged(_ sender: Any?) {
        guard isProfileFileRow, colorMode != .lab else {
            refreshPixelLabel()
            return
        }
        let panel = NSOpenPanel()
        panel.allowedContentTypes = [.init(filenameExtension: "icc"), .init(filenameExtension: "icm")]
            .compactMap { $0 }
        panel.allowsMultipleSelection = false
        panel.canChooseDirectories = false
        guard panel.runModal() == .OK, let url = panel.url else {
            profilePopup.selectItem(at: 0)
            return
        }
        // The MODEL decides: a Gray or CMYK profile is refused by name here
        // exactly as it is for the MCP tool.
        do {
            switch colorMode {
            case .cmyk: _ = try NewDocumentProfile.cmyk(name: url.path)
            case .gray: _ = try NewDocumentProfile.gray(name: url.path)
            case .rgb: _ = try NewDocumentProfile(name: url.path)
            case .lab: _ = try NewDocumentProfile.lab(name: url.path)
            }
            chosenProfilePath = url.path
            profilePopup.lastItem?.title = url.lastPathComponent
            message.stringValue = ""
        } catch {
            profilePopup.selectItem(at: 0)
            message.stringValue = error.localizedDescription
        }
        refreshPixelLabel()
    }

    @objc private func backgroundChanged(_ sender: Any?) {
        guard backgroundPopup.indexOfSelectedItem == 3 else {
            refreshPixelLabel()
            return
        }
        let panel = NSColorPanel.shared
        panel.setTarget(self)
        panel.setAction(#selector(colorPicked(_:)))
        colorPanelTargetsSelf = true
        panel.makeKeyAndOrderFront(nil)
    }

    @objc private func colorPicked(_ sender: Any?) {
        guard let rgb = NSColorPanel.shared.color.usingColorSpace(.sRGB) else { return }
        let byte = { (value: CGFloat) in UInt8(max(0, min(255, (value * 255).rounded()))) }
        chosenColor = .color(byte(rgb.redComponent), byte(rgb.greenComponent), byte(rgb.blueComponent))
        backgroundPopup.item(at: 3)?.title = chosenColor?.name ?? "Custom Colour…"
    }

    // MARK: - Preset library

    /// Save Preset… — the fields as they stand, under a name the nested card
    /// asks for. The window-title name is not part of a preset (as
    /// `save_document_preset` builds it). A library write is a preference: no
    /// Undo step, no Action step.
    @objc private func savePresetClicked(_ sender: Any?) {
        let preset: NewDocumentRequest
        do {
            let request = try buildRequest()
            preset = try NewDocumentRequest(
                width: request.width, height: request.height, ppi: request.ppi,
                profile: request.profile, depth: request.depth, background: request.background,
                colorMode: request.colorMode)
        } catch {
            message.stringValue = error.localizedDescription
            NSSound.beep()
            return
        }
        guard let saved = NewDocumentPresetSaveSheet.run(request: preset, unit: unit) else { return }
        reloadPresets()
        if presetPopup.item(withTitle: saved) != nil { presetPopup.selectItem(withTitle: saved) }
        updateDeletePresetButton()
    }

    /// Delete Preset — the selected listed preset, with no confirmation (the
    /// export preset manager's rule).
    @objc private func deletePresetClicked(_ sender: Any?) {
        let index = presetPopup.indexOfSelectedItem - 1
        guard presets.indices.contains(index) else {
            NSSound.beep()
            return
        }
        do {
            try NewDocumentPresetLibrary.delete(name: presets[index].name)
            reloadPresets()
        } catch {
            message.stringValue = error.localizedDescription
            NSSound.beep()
        }
    }

    // MARK: - Commit

    private func buildRequest() throws -> NewDocumentRequest {
        let ppi = Double(resolutionField.stringValue.trimmingCharacters(in: .whitespaces)) ?? 300
        guard let width = Double(widthField.stringValue.trimmingCharacters(in: .whitespaces)),
              let height = Double(heightField.stringValue.trimmingCharacters(in: .whitespaces)) else {
            throw DoctoredCoreError(message: "Width and height must be numbers.")
        }
        let name = nameField.stringValue.trimmingCharacters(in: .whitespacesAndNewlines)
        return try NewDocumentRequest(
            name: name.isEmpty ? nil : name,
            width: NewDocumentRequest.pixels(width, unit: unit, ppi: ppi),
            height: NewDocumentRequest.pixels(height, unit: unit, ppi: ppi),
            ppi: ppi, profile: profile,
            depth: [8, 16, 32][max(0, min(2, depthPopup.indexOfSelectedItem))],
            background: background, colorMode: colorMode)
    }

    @objc private func cancelClicked(_ sender: Any?) { modal?.end(.cancel) }

    @objc private func createClicked(_ sender: Any?) {
        do {
            result = try buildRequest()
            modal?.end(.OK)
        } catch {
            message.stringValue = error.localizedDescription
            NSSound.beep()
        }
    }
}

/// File ▸ New… ▸ Save Preset… — names the sheet's current fields as a document
/// preset. Nested inside the New Document card, `ExportPresetSaveSheet`'s shape.
final class NewDocumentPresetSaveSheet: NSObject {
    private let request: NewDocumentRequest
    private let unit: NewDocumentUnit
    private let name = NSTextField()
    private let replace = NSButton(
        checkboxWithTitle: "Replace a preset with this name", target: nil, action: nil)
    private let message = NSTextField(wrappingLabelWithString: "")
    private var modal: ModalCardWindowController?
    private var saved: String?

    private init(request: NewDocumentRequest, unit: NewDocumentUnit) {
        self.request = request
        self.unit = unit
        super.init()
    }

    /// Answers the saved preset's name, or nil when the user cancelled.
    static func run(request: NewDocumentRequest, unit: NewDocumentUnit) -> String? {
        NewDocumentPresetSaveSheet(request: request, unit: unit).run()
    }

    private func run() -> String? {
        DSField.style(name)
        name.widthAnchor.constraint(equalToConstant: 330).isActive = true
        name.placeholderString = "Preset name"
        name.setAccessibilityIdentifier("new-document-preset-name")
        replace.font = DS.sans(12)
        message.font = DS.sans(11)
        message.textColor = DS.textMuted
        message.preferredMaxLayoutWidth = 340

        let width = String(format: "%g", request.length(request.width, in: unit))
        let height = String(format: "%g", request.length(request.height, in: unit))
        let summary = NSTextField(wrappingLabelWithString:
            "\(width) × \(height) \(unit.rawValue) · \(PrintSize.ppiText(request.ppi)) ppi · "
            + "\(request.profile.name) · "
            + (request.depth == 32 ? "32-bit float" : "\(request.depth)-bit")
            + " · \(request.background.name)")
        summary.font = DS.sans(11)
        summary.preferredMaxLayoutWidth = 340
        let content = NSStackView(views: [name, summary, replace, message])
        content.orientation = .vertical
        content.alignment = .leading
        content.spacing = 10

        let cancel = sheetCancelButton(target: self, action: #selector(cancel(_:)))
        let save = StickerButton(title: "Save Preset", style: .primary,
                                 target: self, action: #selector(save(_:)))
        save.setAccessibilityIdentifier("new-document-preset-save")
        modal = ModalCardWindowController(title: "Save Document Preset", card: makeSheetView(
            title: "Save Document Preset",
            hint: "A preset stores the size, unit, resolution, profile, depth and background — "
                + "not the document name.",
            content: content, buttonRow: makeButtonRow(cancel: cancel, apply: save)))
        guard modal?.runModal() == .OK else { return nil }
        return saved
    }

    @objc private func cancel(_ sender: Any?) { modal?.end(.cancel) }

    @objc private func save(_ sender: Any?) {
        do {
            let preset = try NewDocumentPresetLibrary.save(
                NewDocumentPreset(name: name.stringValue, request: request, unit: unit),
                replace: replace.state == .on)
            saved = preset.name
            modal?.end(.OK)
        } catch {
            message.stringValue = error.localizedDescription
            NSSound.beep()
        }
    }
}
