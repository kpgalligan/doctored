import AppKit
import UniformTypeIdentifiers

/// View ▸ Proof Setup…: which medium the canvas is simulated on, and how.
///
/// The sheet CHOOSES; `EditorViewController+SoftProof` applies. Nothing here
/// touches the document: a proof setup is per-window view state, so there is
/// no undo step, no dirty flag and no Action step behind this dialog — the
/// same exemption the rulers and the grid take, for the same four reasons.
///
/// The profile list is the machine's own installed profiles filtered by the
/// CORE (`RasterProfile.describe`) to the ones a proof can actually run
/// through: both an A2B and a B2A table. A CMYK press profile is the normal
/// case and is exactly what this build cannot use as a document profile, so
/// the list here is deliberately wider than the Convert sheet's. On a CMYK
/// document (#547 phase 5) it lists CMYK profiles only, shows Preserve CMYK
/// Numbers and disables Gamut Warning.
final class ProofSetupSheetController: NSViewController {
    private weak var editor: EditorViewController?

    let profilePopup = NSPopUpButton(frame: .zero, pullsDown: false)
    let intentPopup = NSPopUpButton(frame: .zero, pullsDown: false)
    let bpcBox = NSButton(checkboxWithTitle: "Black point compensation",
                          target: nil, action: nil)
    let paperBox = NSButton(checkboxWithTitle: "Simulate paper white",
                            target: nil, action: nil)
    let inkBox = NSButton(checkboxWithTitle: "Simulate black ink",
                          target: nil, action: nil)
    let proofBox = NSButton(checkboxWithTitle: "Proof Colors",
                            target: nil, action: nil)
    private let warningBox = NSButton(checkboxWithTitle: "Gamut Warning",
                                      target: nil, action: nil)
    /// Preserve CMYK Numbers (#547 phase 5): shown on a CMYK document only.
    let preserveBox = NSButton(checkboxWithTitle: "Preserve CMYK Numbers",
                               target: nil, action: nil)
    /// A CMYK document proofs through CMYK profiles only, with no gamut warning.
    private let isCmyk: Bool
    let gamutWell = NSColorWell(frame: .zero)
    private let note = ColorProfileControls.noteLabel()
    private var applyButton: NSButton!

    /// The saved-preset row (`ProofSetupSheetController+Presets.swift`).
    let presetPopup = NSPopUpButton(frame: .zero, pullsDown: false)
    let defaultBox = NSButton(checkboxWithTitle: "Default for new windows",
                              target: nil, action: nil)
    var presetSaveButton: NSButton!
    var presetDeleteButton: NSButton!
    /// The library as read ONCE when the sheet opened, and its default.
    var presets: [ProofPreset] = []
    var defaultPresetName: String?
    /// A library-level fault reading the presets, shown in the note.
    var presetProblem: String?
    /// The popup row to go back to when choosing a preset fails.
    var presetSelection = 0
    /// Why the default proof preset could not be used — the note's first
    /// line. Cleared once the user chooses a profile or a preset.
    var notice: String?

    /// The profiles the popup lists, and the bytes each one resolves to.
    let available: [InstalledProfile]
    /// The chosen profile's bytes and name, resolved when the selection
    /// changes so the sheet's value is a VALUE rather than a name.
    var chosen: (data: Data, name: String, path: String?)?

    private var display: ProofDisplay

    /// nil with no document: a proof is a picture of a document. Opened with
    /// no setup and no notice, the sheet preloads the default proof preset;
    /// a default that cannot be resolved becomes the notice.
    init?(editor: EditorViewController, notice: String? = nil) {
        guard editor.document != nil else { return nil }
        self.editor = editor
        self.display = editor.proofDisplay
        isCmyk = editor.document?.doc?.isCmyk == true
        self.available = isCmyk
            ? InstalledProfiles.proofable.filter { $0.description.space == "cmyk" }
            : InstalledProfiles.proofable
        self.notice = notice
        super.init(nibName: nil, bundle: nil)
        if display.setup == nil, notice == nil {
            do {
                if let preloaded = try editor.defaultProofSetup() {
                    // Choosing a preset turns Proof Colors on; so does this.
                    display.setup = preloaded
                    display.proofColors = true
                }
            } catch {
                self.notice = error.localizedDescription
            }
        }
        loadPresets()
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) {
        fatalError("ProofSetupSheetController does not support NSCoder")
    }

    override func loadView() {
        let menu = NSMenu()
        for (index, profile) in available.enumerated() {
            let item = NSMenuItem(title: profile.name, action: nil, keyEquivalent: "")
            item.tag = index
            menu.addItem(item)
        }
        if available.isEmpty {
            let item = NSMenuItem(title: "No proofable profiles installed",
                                  action: nil, keyEquivalent: "")
            item.tag = -1
            item.isEnabled = false
            menu.addItem(item)
        }
        menu.addItem(.separator())
        let load = NSMenuItem(title: "Load from file…", action: nil, keyEquivalent: "")
        load.tag = ProofSetupSheetController.loadTag
        menu.addItem(load)
        profilePopup.menu = menu
        profilePopup.font = DS.sans(13)
        profilePopup.widthAnchor.constraint(lessThanOrEqualToConstant: 250).isActive = true
        profilePopup.target = self
        profilePopup.action = #selector(profileChanged(_:))

        for intent in RenderingIntent.allCases {
            intentPopup.addItem(withTitle: intent.displayName)
            intentPopup.lastItem?.tag = Int(intent.code)
        }
        intentPopup.font = DS.sans(13)
        intentPopup.target = self
        intentPopup.action = #selector(controlsChanged(_:))

        for box in [bpcBox, paperBox, inkBox, proofBox, warningBox, preserveBox] {
            box.font = DS.sans(13)
            box.target = self
            box.action = #selector(controlsChanged(_:))
        }
        gamutWell.target = self
        gamutWell.action = #selector(controlsChanged(_:))
        gamutWell.widthAnchor.constraint(equalToConstant: 44).isActive = true
        gamutWell.heightAnchor.constraint(equalToConstant: 22).isActive = true

        // Start from the window's current setup, so opening the sheet and
        // pressing Apply changes nothing by accident.
        if let setup = display.setup {
            chosen = (setup.profile, setup.profileName, setup.path)
            selectProfileRow(data: setup.profile, name: setup.profileName, path: setup.path)
            intentPopup.selectItem(withTag: Int(setup.intent.code))
            bpcBox.state = setup.blackPointCompensation ? .on : .off
            paperBox.state = setup.simulatePaperWhite ? .on : .off
            inkBox.state = setup.simulateInkBlack ? .on : .off
            preserveBox.state = setup.preserveNumbers ? .on : .off
            gamutWell.color = setup.gamutColor
        } else {
            let defaults = ProofSetup(profile: Data(), profileName: "")
            intentPopup.selectItem(withTag: Int(defaults.intent.code))
            bpcBox.state = defaults.blackPointCompensation ? .on : .off
            gamutWell.color = defaults.gamutColor
            resolveSelectedProfile()
        }
        proofBox.state = display.proofColors ? .on : .off
        warningBox.state = display.gamutWarning && !isCmyk ? .on : .off
        preserveBox.isHidden = !isCmyk
        preserveBox.setAccessibilityIdentifier("proof-setup-preserve-numbers")

        let grid = NSGridView(views: [
            [fieldLabel("Preset:"), makePresetRow()],
            [NSGridCell.emptyContentView, defaultBox],
            [fieldLabel("Simulate:"), profilePopup],
            [fieldLabel("Intent:"), intentPopup],
            [NSGridCell.emptyContentView, bpcBox],
            [NSGridCell.emptyContentView, preserveBox],
            [NSGridCell.emptyContentView, paperBox],
            [NSGridCell.emptyContentView, inkBox],
            [fieldLabel("Show:"), proofBox],
            [NSGridCell.emptyContentView, warningBox],
            [fieldLabel("Warning colour:"), gamutWell],
        ])
        grid.rowSpacing = 8
        grid.columnSpacing = 12
        grid.column(at: 0).xPlacement = .trailing
        grid.column(at: 0).width = 106

        let content = NSStackView(views: [grid, note])
        content.orientation = .vertical
        content.alignment = .leading
        content.spacing = 12

        let cancelButton = sheetCancelButton(target: self, action: #selector(cancelClicked(_:)))
        applyButton = sheetApplyButton(target: self, action: #selector(applyClicked(_:)))
        selectMatchingPreset()
        syncControls()
        view = makeSheetView(
            title: "Proof setup",
            hint: "A proof shows what the document would look like on another medium. It "
                + "changes nothing: the master, every render and every export are untouched, "
                + "and there is no undo step.",
            content: content,
            buttonRow: makeButtonRow(
                cancel: cancelButton, apply: applyButton,
                leading: [sheetFootnote("view only")]))
    }

    static let loadTag = -99

    // MARK: - State

    var setup: ProofSetup? {
        guard let chosen else { return nil }
        var out = ProofSetup(profile: chosen.data, profileName: chosen.name, path: chosen.path)
        out.intent = RenderingIntent.allCases
            .first { Int($0.code) == intentPopup.selectedTag() } ?? .relative
        out.blackPointCompensation = bpcBox.state == .on
        out.simulatePaperWhite = paperBox.state == .on
        out.simulateInkBlack = inkBox.state == .on
        out.gamutColor = gamutWell.color
        out.preserveNumbers = isCmyk && preserveBox.state == .on
        return out
    }

    private var noteText: String {
        let lead = [notice, presetProblem].compactMap { $0 }
        return (lead + [profileNoteText]).joined(separator: "\n")
    }

    private var profileNoteText: String {
        guard let chosen else {
            return "Choose a profile to proof through. A press or paper profile with both an "
                + "A2B and a B2A table is what a proof needs; Doctored lists the installed "
                + "ones it can read."
        }
        let intent = RenderingIntent.allCases
            .first { Int($0.code) == intentPopup.selectedTag() } ?? .relative
        var lines = ["The canvas is drawn through “\(chosen.name)” at \(intent.displayName). "
            + intent.summary]
        if paperBox.state == .on {
            lines.append("Simulating paper white shows the medium's own tint instead of the "
                + "display's white.")
        }
        if inkBox.state == .on {
            lines.append("Simulating black ink shows how far from black the medium's darkest "
                + "tone really is.")
        }
        if warningBox.state == .on {
            lines.append("Pixels this medium cannot hold are covered in the warning colour; "
                + "the rule is a CIE76 ΔE above 5 against the colour's own round trip.")
        }
        return lines.joined(separator: " ")
    }

    func syncControls() {
        note.stringValue = noteText
        gamutWell.isEnabled = warningBox.state == .on
        warningBox.isEnabled = !isCmyk
        intentPopup.isEnabled = !(isCmyk && preserveBox.state == .on)
        applyButton.isEnabled = chosen != nil
        syncPresetControls()
    }

    /// Reads the bytes for whatever the popup names now.
    private func resolveSelectedProfile() {
        let tag = profilePopup.selectedTag()
        guard tag >= 0, available.indices.contains(tag) else { return }
        let profile = available[tag]
        guard let bytes = try? Data(contentsOf: profile.url), !bytes.isEmpty else { return }
        chosen = (bytes, profile.name, profile.path)
    }

    // MARK: - Actions

    @objc private func profileChanged(_ sender: Any?) {
        if profilePopup.selectedTag() == ProofSetupSheetController.loadTag {
            loadFromFile()
            return
        }
        notice = nil
        if profilePopup.selectedTag() == ProofSetupSheetController.loadTag - 1,
           let loaded = profilePopup.selectedItem?.representedObject as? LoadedProofProfile {
            chosen = (loaded.data, loaded.name, loaded.path.isEmpty ? nil : loaded.path)
        } else {
            resolveSelectedProfile()
        }
        controlsDidChange()
    }

    @objc private func controlsChanged(_ sender: Any?) {
        controlsDidChange()
    }

    private func loadFromFile() {
        guard let window = view.window else { return }
        let panel = NSOpenPanel()
        panel.allowsMultipleSelection = false
        panel.canChooseDirectories = false
        panel.message = "Choose a profile to proof through."
        panel.prompt = "Load"
        if let type = UTType("com.apple.colorsync-profile") {
            panel.allowedContentTypes = [type]
        }
        panel.beginSheetModal(for: window) { [weak self] response in
            guard let self else { return }
            defer { self.controlsDidChange() }
            guard response == .OK, let url = panel.url,
                  let bytes = try? Data(contentsOf: url), !bytes.isEmpty
            else {
                self.restoreSelection()
                return
            }
            guard let description = RasterProfile.describe(bytes) else {
                self.presentColorProfileAlert(
                    "“\(url.lastPathComponent)” is not an ICC colour profile.")
                self.restoreSelection()
                return
            }
            guard !self.isCmyk || description.space == "cmyk" else {
                self.presentColorProfileAlert(
                    "“\(description.name)” is not CMYK; a CMYK document proofs through a CMYK profile.")
                self.restoreSelection()
                return
            }
            guard description.proofable else {
                self.presentColorProfileAlert(
                    "“\(description.name)” cannot be proofed through — "
                        + (description.proofRefusal
                            ?? "it needs both a device-to-PCS and a PCS-to-device table."))
                self.restoreSelection()
                return
            }
            // A loaded profile becomes a row of its own, so the popup can go
            // back to it after a later change.
            self.selectProfileRow(data: bytes, name: description.name, path: url.path)
            self.chosen = (bytes, description.name, url.path)
            self.notice = nil
        }
    }

    /// Puts the popup back on whatever `chosen` already holds.
    private func restoreSelection() {
        guard let chosen else {
            profilePopup.selectItem(at: 0)
            return
        }
        selectProfileRow(data: chosen.data, name: chosen.name, path: chosen.path)
    }

    @objc private func applyClicked(_ sender: Any?) {
        guard let setup else {
            NSSound.beep()
            return
        }
        let proofColors = proofBox.state == .on
        let gamutWarning = warningBox.state == .on && !isCmyk
        dismiss(self)
        editor?.applyProofSetup(setup, proofColors: proofColors, gamutWarning: gamutWarning)
    }

    @objc private func cancelClicked(_ sender: Any?) {
        dismiss(self)
    }
}
