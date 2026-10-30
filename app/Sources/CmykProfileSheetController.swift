import AppKit

/// Image ▸ Mode ▸ Assign Profile… / Convert to Profile… on a CMYK document (#547 phase 5): the
/// destination CMYK profile and, for Convert, the intent, black-point compensation and Flatten
/// Image to Preserve Appearance, planned by the core before Apply (`CmykProfileEdit`; the job and
/// its one undo step are `EditorViewController+CmykProfile.swift`'s).
///
/// The profile popup lists the working CMYK (Edit ▸ Color Settings) and every installed CMYK
/// profile both directions of which this build reads. Convert also lists the RGB working spaces and
/// the installed RGB profiles a 16-bit conversion can write: converting to one of those is Image ▸
/// Mode ▸ RGB Color's conversion (`CmykModeConversion`). Intent and BPC start at Color Settings'
/// Conversion Options.
///
/// On a Lab Color document (#547 Lab phase 5b) only Convert opens, with the same rows: Image ▸
/// Mode's conversion into the chosen CMYK or RGB profile (`CmykModeConversion`), planned with
/// `CmykModeConversion.plan` and committed by `commitLabProfileConversion`. Since phase 6a-ii it
/// also lists the Gray destinations after the RGB rows (`GrayProfileSheetController`'s list: the
/// working Gray, the other Gray working spaces, the installed Gray profiles), each Grayscale by
/// 5a-ii's route: each pixel's L* through the profile's tone curve.
final class CmykProfileSheetController: NSViewController {
    private weak var editor: EditorViewController?
    private weak var document: ImageDocument?
    private let kind: CmykProfileEdit.Kind

    private let profilePopup = NSPopUpButton(frame: .zero, pullsDown: false)
    private let intentPopup = NSPopUpButton(frame: .zero, pullsDown: false)
    private let bpcBox = NSButton(checkboxWithTitle: "Use Black Point Compensation", target: nil, action: nil)
    private let flattenBox = NSButton(checkboxWithTitle: "Flatten Image to Preserve Appearance",
                                      target: nil, action: nil)
    private let note = NSTextField(wrappingLabelWithString: "")
    private var applyButton: NSButton!

    /// One popup row: the profile bytes (nil = the working CMYK, or toward Grayscale the working
    /// space `space`), whether it is an RGB or (Lab phase 6a-ii) a Gray destination, and the MCP
    /// destination arguments the recorded step carries.
    private struct Row {
        let data: Data?
        let rgb: Bool
        let destination: [String: Any]
        var gray = false
        var space: String?
    }
    private var rows: [Row] = []
    private var options = CmykConversionOptions()
    private var refusal: String?

    init(editor: EditorViewController, document: ImageDocument, kind: CmykProfileEdit.Kind) {
        self.editor = editor
        self.document = document
        self.kind = kind
        super.init(nibName: nil, bundle: nil)
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) {
        fatalError("CmykProfileSheetController does not support NSCoder")
    }

    private var isConvert: Bool { kind == .convert }
    /// A Lab Color source (phase 5b): every row is a mode conversion.
    private var isLab: Bool { document?.doc?.isLab == true }

    override func loadView() {
        fillProfiles()
        profilePopup.font = DS.sans(13)
        profilePopup.target = self
        profilePopup.action = #selector(optionsChanged(_:))
        profilePopup.setAccessibilityIdentifier("cmyk-profile-popup")
        for intent in RenderingIntent.allCases {
            intentPopup.addItem(withTitle: intent.displayName)
            intentPopup.lastItem?.tag = Int(intent.code)
        }
        intentPopup.selectItem(withTag: Int(options.intent.code))
        intentPopup.font = DS.sans(13)
        intentPopup.target = self
        intentPopup.action = #selector(optionsChanged(_:))
        intentPopup.setAccessibilityIdentifier("cmyk-profile-intent")
        for box in [bpcBox, flattenBox] {
            box.font = DS.sans(13)
            box.target = self
            box.action = #selector(optionsChanged(_:))
        }
        bpcBox.state = options.blackPointCompensation ? .on : .off
        bpcBox.setAccessibilityIdentifier("cmyk-profile-bpc")
        flattenBox.setAccessibilityIdentifier("cmyk-profile-flatten")
        note.font = DS.sans(12)
        note.textColor = DS.textMuted
        note.preferredMaxLayoutWidth = sheetWidth - sheetInset * 2
        note.setAccessibilityIdentifier("cmyk-profile-note")

        var gridRows: [[NSView]] = [[fieldLabel("Profile:"), profilePopup]]
        if isConvert {
            gridRows += [[fieldLabel("Intent:"), intentPopup],
                         [NSGridCell.emptyContentView, bpcBox],
                         [NSGridCell.emptyContentView, flattenBox]]
        }
        let grid = NSGridView(views: gridRows)
        grid.rowSpacing = 10
        grid.columnSpacing = 12
        grid.column(at: 0).xPlacement = .trailing
        grid.column(at: 0).width = 90
        let content = NSStackView(views: [grid, note])
        content.orientation = .vertical
        content.alignment = .leading
        content.spacing = 12

        let cancelButton = sheetCancelButton(target: self, action: #selector(cancelClicked(_:)))
        cancelButton.setAccessibilityIdentifier("cmyk-profile-cancel")
        applyButton = sheetApplyButton(target: self, action: #selector(applyClicked(_:)))
        applyButton.setAccessibilityIdentifier("cmyk-profile-apply")
        refreshPlan()
        view = makeSheetView(
            title: isConvert ? "Convert to Profile" : "Assign Profile",
            hint: isLab
                ? "A Lab Color document has no profile of its own: converting to a CMYK, RGB or "
                    + "Gray profile converts it to CMYK Color, RGB Color or Grayscale in that "
                    + "profile at its own depth, as Image ▸ Mode does."
                : isConvert
                ? "Re-separates every layer's ink through the destination profile so the picture keeps "
                    + "its appearance, keeping each layer, its mask and its transparency. Converting to an "
                    + "RGB profile converts the document to RGB Color."
                : "Gives the document another CMYK profile. The ink numbers are not touched, so the "
                    + "picture changes appearance.",
            content: content,
            buttonRow: makeButtonRow(cancel: cancelButton, apply: applyButton,
                                     leading: [sheetFootnote("one undo step")]))
    }

    private func fillProfiles() {
        profilePopup.removeAllItems()
        rows = []
        let working = WorkingCMYK.installedName.map { "Working CMYK — \($0)" } ?? "Working CMYK (none set)"
        profilePopup.addItem(withTitle: working)
        rows.append(Row(data: nil, rgb: false, destination: ["profile": "working_cmyk"]))
        for profile in InstalledProfiles.proofable where profile.description.space == "cmyk" {
            guard let data = try? Data(contentsOf: profile.url) else { continue }
            profilePopup.addItem(withTitle: profile.name)
            rows.append(Row(data: data, rgb: false, destination: ["profile": "installed", "name": profile.name]))
        }
        if isConvert {
            for space in WorkingSpace.allCases {
                profilePopup.addItem(withTitle: "RGB: \(space.displayName)")
                rows.append(Row(data: space.profileData, rgb: true,
                                destination: ["profile": space == .sRGB ? "srgb" : "display_p3"]))
            }
            for profile in InstalledProfiles.highDepthDestinations {
                guard let data = try? Data(contentsOf: profile.url) else { continue }
                profilePopup.addItem(withTitle: "RGB: \(profile.name)")
                rows.append(Row(data: data, rgb: true, destination: ["profile": "installed", "name": profile.name]))
            }
        }
        if isConvert, isLab { fillGrayProfiles() }
        profilePopup.selectItem(at: 0)
    }

    /// Lab phase 6a-ii: the Gray destinations, `GrayProfileSheetController.fillProfiles`' list.
    private func fillGrayProfiles() {
        let working = ColorSettings.workingGray
        profilePopup.addItem(withTitle: "Gray: Working Gray — \(working.displayName)")
        rows.append(Row(data: nil, rgb: false, destination: ["profile": "working_gray"], gray: true,
                        space: working.rawValue))
        for space in GrayWorkingSpace.allCases where space != working {
            profilePopup.addItem(withTitle: "Gray: \(space.displayName)")
            rows.append(Row(data: nil, rgb: false, destination: ["profile": space.rawValue], gray: true,
                            space: space.rawValue))
        }
        for profile in InstalledProfiles.matching(kinds: ["gray"]) {
            guard let data = try? Data(contentsOf: profile.url) else { continue }
            profilePopup.addItem(withTitle: "Gray: \(profile.name)")
            rows.append(Row(data: data, rgb: false,
                            destination: ["profile": "installed", "name": profile.name], gray: true))
        }
    }

    private var selected: Row? {
        let index = profilePopup.indexOfSelectedItem
        return rows.indices.contains(index) ? rows[index] : nil
    }

    /// Asks the core what these settings would do.
    private func refreshPlan() {
        refusal = nil
        // Phase 5b: a Lab source plans the mode conversion into the row's profile (6a-ii: or Gray
        // working space).
        if let source = document?.doc, source.isLab, let row = selected {
            do {
                let plan = try CmykModeConversion.plan(source: source, target: Self.labTarget(row),
                                                       profile: row.data, options: options,
                                                       graySpace: row.space)
                refusal = plan["refusal"] as? String
            } catch {
                refusal = error.localizedDescription
            }
            return syncControls()
        }
        guard let typed = document?.doc?.precision, let row = selected else {
            refusal = "No CMYK document."
            return syncControls()
        }
        do {
            let plan = row.rgb
                ? try typed.convertCMYKToRGBPlan(profile: row.data, options: options)
                : try typed.cmykProfilePlan(profile: row.data, request: CmykProfileEdit.request(kind, options))
            refusal = plan["refusal"] as? String
        } catch {
            refusal = error.localizedDescription
        }
        syncControls()
    }

    private func syncControls() {
        applyButton?.isEnabled = refusal == nil
        let rgb = selected?.rgb == true
        note.stringValue = refusal ?? (selected?.gray == true
            ? "The document becomes a Grayscale document in this profile, each pixel's L* through "
                + "its tone curve; intent and black point compensation do not apply."
            : rgb
            ? "The document becomes an RGB document in this profile."
            : isLab
            ? "The document becomes a CMYK document in this profile."
            : isConvert
                ? (options.flatten
                    ? "The image is flattened onto paper first, so its appearance is kept exactly."
                    : "Relative colorimetric with black point compensation is the default conversion.")
                : "Only the profile changes: every ink number stays as it is.")
    }

    @objc private func optionsChanged(_ sender: Any?) {
        options.intent = RenderingIntent.allCases.first { Int($0.code) == intentPopup.selectedTag() } ?? .relative
        options.blackPointCompensation = bpcBox.state == .on
        options.flatten = flattenBox.state == .on
        refreshPlan()
    }

    @objc private func applyClicked(_ sender: Any?) {
        guard refusal == nil, let document, let editor, let row = selected else { NSSound.beep(); return }
        let kind = self.kind, options = self.options, lab = isLab
        dismiss(self)
        if lab {
            editor.commitLabProfileConversion(Self.labTarget(row), profile: row.data,
                                              graySpace: row.space, destination: row.destination,
                                              options: options, document: document)
            return
        }
        if row.rgb {
            editor.commitCmykToRGBProfile(row.data, destination: row.destination, options: options,
                                          document: document)
            return
        }
        editor.commitCmykProfileEdit(kind, profile: row.data, destination: row.destination, options: options,
                                     document: document)
    }

    @objc private func cancelClicked(_ sender: Any?) {
        dismiss(self)
    }

    /// The mode a Lab source's row converts it to.
    private static func labTarget(_ row: Row) -> ColorModeTarget {
        row.gray ? .gray : row.rgb ? .rgb : .cmyk
    }
}
