import AppKit

/// Image ▸ Mode ▸ CMYK Color… / RGB Color… (#1 phase 1): the conversion's destination profile,
/// rendering intent, black-point compensation and Flatten Image, planned by the core before Apply
/// (`EditorViewController+CmykMode.swift` owns the job and its one undo step).
///
/// Toward CMYK the profile popup lists the working CMYK profile (`WorkingCMYK`: Adobe's U.S. Web
/// Coated (SWOP) v2 when installed, else macOS's Generic CMYK Profile, read from the system and
/// never bundled) and every installed CMYK profile both directions of which this build reads.
/// Toward RGB it lists the two RGB working spaces, the current one first. The intent and BPC
/// start at Edit ▸ Color Settings' Conversion Options (relative colorimetric with BPC unless
/// changed there). A Lab Color document converts toward CMYK through 16-bit
/// ProPhoto RGB (#1 Lab phase 5a, the core's `lab_cmyk`). A document holding
/// anything a phase-1 CMYK document cannot (a group, mask, style, adjustment, clipping or blend
/// mode) must be flattened first, and the Flatten Image box says so before Apply is enabled.
final class CmykModeSheetController: NSViewController {
    private weak var editor: EditorViewController?
    private weak var document: ImageDocument?
    private let target: ColorModeTarget

    private let profilePopup = NSPopUpButton(frame: .zero, pullsDown: false)
    private let intentPopup = NSPopUpButton(frame: .zero, pullsDown: false)
    private let bpcBox = NSButton(checkboxWithTitle: "Black point compensation", target: nil, action: nil)
    private let flattenBox = NSButton(checkboxWithTitle: "Flatten image", target: nil, action: nil)
    private let note = NSTextField(wrappingLabelWithString: "")
    private var applyButton: NSButton!

    /// The popup's profiles, in item order: nil is the working CMYK profile (toward CMYK).
    private var profiles: [Data?] = []
    /// Each item's `convert_mode` destination arguments, recorded in the Action step.
    private var destinations: [[String: Any]] = []
    private var options = CmykConversionOptions()
    /// The core's refusal for the current settings, or nil.
    private var refusal: String?
    private var flattenRequired = false

    init(editor: EditorViewController, document: ImageDocument, target: ColorModeTarget) {
        self.editor = editor
        self.document = document
        self.target = target
        super.init(nibName: nil, bundle: nil)
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) {
        fatalError("CmykModeSheetController does not support NSCoder")
    }

    override func loadView() {
        fillProfiles()
        profilePopup.font = DS.sans(13)
        profilePopup.target = self
        profilePopup.action = #selector(optionsChanged(_:))
        profilePopup.setAccessibilityIdentifier("cmyk-mode-profile")
        for intent in RenderingIntent.allCases {
            intentPopup.addItem(withTitle: intent.displayName)
            intentPopup.lastItem?.tag = Int(intent.code)
        }
        intentPopup.selectItem(withTag: Int(options.intent.code))
        intentPopup.font = DS.sans(13)
        intentPopup.target = self
        intentPopup.action = #selector(optionsChanged(_:))
        intentPopup.setAccessibilityIdentifier("cmyk-mode-intent")
        for box in [bpcBox, flattenBox] {
            box.font = DS.sans(13)
            box.target = self
            box.action = #selector(optionsChanged(_:))
        }
        // Edit ▸ Color Settings' Conversion Options (#1 phase 5).
        bpcBox.state = options.blackPointCompensation ? .on : .off
        bpcBox.setAccessibilityIdentifier("cmyk-mode-bpc")
        flattenBox.setAccessibilityIdentifier("cmyk-mode-flatten")
        note.font = DS.sans(12)
        note.textColor = DS.textMuted
        note.preferredMaxLayoutWidth = sheetWidth - sheetInset * 2
        note.setAccessibilityIdentifier("cmyk-mode-note")

        let grid = NSGridView(views: [
            [fieldLabel(target == .cmyk ? "Profile:" : "Destination:"), profilePopup],
            [fieldLabel("Intent:"), intentPopup],
            [NSGridCell.emptyContentView, bpcBox],
            [NSGridCell.emptyContentView, flattenBox],
        ])
        grid.rowSpacing = 10
        grid.columnSpacing = 12
        grid.column(at: 0).xPlacement = .trailing
        grid.column(at: 0).width = 106
        let content = NSStackView(views: [grid, note])
        content.orientation = .vertical
        content.alignment = .leading
        content.spacing = 12

        let cancelButton = sheetCancelButton(target: self, action: #selector(cancelClicked(_:)))
        cancelButton.setAccessibilityIdentifier("cmyk-mode-cancel")
        applyButton = sheetApplyButton(target: self, action: #selector(applyClicked(_:)))
        applyButton.setAccessibilityIdentifier("cmyk-mode-apply")
        refreshPlan()
        view = makeSheetView(
            title: target == .cmyk ? "Convert to CMYK Color" : "Convert to RGB Color",
            hint: target == .cmyk && document?.doc?.isGray == true
                ? "Converts every layer's gray into the press profile's ink through the document's "
                    + "Gray profile, keeping each layer, its opacity and its transparency. The document "
                    + "then holds C, M, Y and K channels and is shown through its profile. "
                    + CmykDocument.limitation
                : target == .cmyk && document?.doc?.isLab == true
                ? "Converts every layer's L*a*b* values into the press profile's ink through 16-bit "
                    + "ProPhoto RGB, keeping each layer, its opacity and its transparency. The document "
                    + "then holds C, M, Y and K channels and is shown through its profile. "
                    + CmykDocument.limitation
                : target == .cmyk && document?.doc?.isIndexed == true
                ? "Expands the palette and converts its colours into the press profile's ink at "
                    + "8 bits, keeping the one layer and its transparent pixels. The "
                    + "document then holds C, M, Y and K channels and is shown through its profile. "
                    + CmykDocument.limitation
                : target == .cmyk
                ? "Converts every layer's colour into the press profile's ink, keeping each layer, its "
                    + "opacity and its transparency. The document then holds C, M, Y and K channels and "
                    + "is shown through its profile. " + CmykDocument.limitation
                : document?.doc?.isGray == true
                    ? "Converts every layer's gray into RGB through the document's Gray profile, keeping "
                        + "each layer, its opacity and its transparency."
                    : document?.doc?.isLab == true
                    ? "Converts every layer's L*a*b* values into RGB (D50, through the chosen space), "
                        + "keeping each layer, its opacity and its transparency."
                    : "Converts every layer's ink into RGB through the document's CMYK profile, keeping each "
                        + "layer, its opacity and its transparency.",
            content: content,
            buttonRow: makeButtonRow(cancel: cancelButton, apply: applyButton,
                                     leading: [sheetFootnote("one undo step")]))
    }

    private func fillProfiles() {
        profilePopup.removeAllItems()
        profiles = []
        destinations = []
        switch target {
        case .cmyk:
            let working = WorkingCMYK.installedName.map { "Working CMYK — \($0)" } ?? "Working CMYK (none set)"
            profilePopup.addItem(withTitle: working)
            profiles.append(nil)
            destinations.append(["profile": "working"])
            for profile in InstalledProfiles.proofable where profile.description.space == "cmyk" {
                guard let data = try? Data(contentsOf: profile.url) else { continue }
                profilePopup.addItem(withTitle: profile.name)
                profiles.append(data)
                destinations.append(["profile": "installed", "name": profile.name])
            }
        case .rgb:
            let current = ColorSettings.workingSpace
            for space in [current] + WorkingSpace.allCases.filter({ $0 != current }) {
                profilePopup.addItem(withTitle: space == current
                    ? "Working RGB — \(space.displayName)" : space.displayName)
                profiles.append(space.profileData)
                destinations.append(["rgb_space": space.rawValue])
            }
        case .gray:
            // Grayscale asks "Discard color information?" instead of this sheet
            // (EditorViewController+GrayMode.swift); its space is the working Gray.
            profilePopup.addItem(withTitle: "Working Gray — \(ColorSettings.workingGray.displayName)")
            profiles.append(nil)
            destinations.append(["gray_space": ColorSettings.workingGray.rawValue])
        case .lab:
            // Lab Color converts at once (EditorViewController+LabMode.swift); never presented.
            profilePopup.addItem(withTitle: "Lab Color (D50)")
            profiles.append(nil)
            destinations.append([:])
        case .indexed:
            // Indexed Color has its own sheet (IndexedModeSheetController); never presented.
            profilePopup.addItem(withTitle: "Indexed Color")
            profiles.append(nil)
            destinations.append([:])
        case .duotone:
            // Duotone has its own sheet (DuotoneSheetController); never presented.
            profilePopup.addItem(withTitle: "Duotone")
        case .bitmap:
            // Bitmap has its own sheet (BitmapModeSheetController); never presented.
            profilePopup.addItem(withTitle: "Bitmap")
            profiles.append(nil)
            destinations.append([:])
        }
        profilePopup.selectItem(at: 0)
    }

    private var selectedProfile: Data? {
        let index = profilePopup.indexOfSelectedItem
        return profiles.indices.contains(index) ? profiles[index] : nil
    }

    private var selectedDestination: [String: Any] {
        let index = profilePopup.indexOfSelectedItem
        return destinations.indices.contains(index) ? destinations[index] : [:]
    }

    /// Asks the core what these settings would do (an 8-bit document is asked of its own
    /// layers, since the core plans the typed conversion once the document is promoted).
    private func refreshPlan() {
        refusal = nil
        flattenRequired = false
        guard let doc = document?.doc else { refusal = "No document."; return syncControls() }
        do {
            if doc.precision != nil {
                // #1 phase 1a: a Grayscale source toward RGB is planned by its own arm.
                let plan = try CmykModeConversion.plan(source: doc, target: target, profile: selectedProfile,
                                                       options: options)
                flattenRequired = plan["flatten_required"] as? Bool ?? false
                refusal = plan["refusal"] as? String
            } else if target == .cmyk {
                flattenRequired = CmykModeConversion.eightBitNeedsFlatten(doc)
                if flattenRequired && !options.flatten {
                    refusal = "This document holds a group, mask, style, adjustment, clipping mask or blend "
                        + "mode a CMYK document cannot hold yet; flatten the image to convert it."
                }
            }
        } catch {
            refusal = error.localizedDescription
        }
        syncControls()
    }

    private func syncControls() {
        flattenBox.isEnabled = target == .rgb || flattenRequired || options.flatten
        applyButton?.isEnabled = refusal == nil
        note.stringValue = refusal ?? (flattenRequired && options.flatten
            ? "The image is flattened first: its layers become one layer of the rendered appearance."
            : target == .cmyk
                ? "Relative colorimetric with black-point compensation is the default conversion."
                : "The RGB document is 16 bits per channel (8 when the CMYK document is 8-bit).")
    }

    @objc private func optionsChanged(_ sender: Any?) {
        options.intent = RenderingIntent.allCases.first { Int($0.code) == intentPopup.selectedTag() } ?? .relative
        options.blackPointCompensation = bpcBox.state == .on
        options.flatten = flattenBox.state == .on
        refreshPlan()
    }

    @objc private func applyClicked(_ sender: Any?) {
        guard refusal == nil, let document, let editor else { NSSound.beep(); return }
        let target = self.target, profile = selectedProfile, options = self.options
        let destination = selectedDestination
        dismiss(self)
        editor.commitColorMode(target, profile: profile, destination: destination, options: options,
                               document: document)
    }

    @objc private func cancelClicked(_ sender: Any?) {
        dismiss(self)
    }
}

extension EditorViewController {
    /// The sheet's Apply: the conversion as one undo step, recorded as `convert_mode`.
    func commitColorMode(_ target: ColorModeTarget, profile: Data?, destination: [String: Any],
                         options: CmykConversionOptions, document: ImageDocument) {
        do {
            let conversion = try CmykModeConversion(document, target: target, profile: profile,
                                                    destination: destination, options: options)
            let prepared = try conversion.compute()
            try conversion.publish(prepared, to: document, record: [conversion.step()])
        } catch is JobCancelled {
            return
        } catch {
            document.presentError(error)
        }
    }
}
