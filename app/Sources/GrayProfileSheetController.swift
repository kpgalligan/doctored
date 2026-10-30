import AppKit

/// Edit ▸ Assign Profile… / Convert to Profile… on a Grayscale document (#547 phase 5a): the
/// destination Gray profile and, for Convert, the intent and black-point compensation, planned by
/// the core before Apply (`GrayProfileEdit`; the job and its one undo step are
/// `EditorViewController+GrayMode.swift`'s).
///
/// The profile popup lists Color Settings' Gray working spaces, the working one first, then every
/// installed Gray profile. Intent and BPC start at Color Settings' Conversion Options; on a 32-bit
/// document they do not apply (linear gray is the same under every Gray profile) and are disabled.
final class GrayProfileSheetController: NSViewController {
    private weak var editor: EditorViewController?
    private weak var document: ImageDocument?
    private let kind: GrayProfileEdit.Kind

    private let profilePopup = NSPopUpButton(frame: .zero, pullsDown: false)
    private let intentPopup = NSPopUpButton(frame: .zero, pullsDown: false)
    private let bpcBox = NSButton(checkboxWithTitle: "Use Black Point Compensation", target: nil, action: nil)
    private let note = NSTextField(wrappingLabelWithString: "")
    private var applyButton: NSButton!

    /// One popup row: the profile bytes (an installed profile) or the working space's wire name,
    /// and the MCP destination arguments the recorded step carries.
    private struct Row {
        let data: Data?
        let space: String?
        let destination: [String: Any]
    }
    private var rows: [Row] = []
    private var options = CmykConversionOptions()
    private var refusal: String?
    private var changes = true

    init(editor: EditorViewController, document: ImageDocument, kind: GrayProfileEdit.Kind) {
        self.editor = editor
        self.document = document
        self.kind = kind
        super.init(nibName: nil, bundle: nil)
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) {
        fatalError("GrayProfileSheetController does not support NSCoder")
    }

    private var isConvert: Bool { kind == .convert }
    private var isFloat: Bool { document?.doc?.grayDepth == 32 }

    override func loadView() {
        fillProfiles()
        profilePopup.font = DS.sans(13)
        profilePopup.target = self
        profilePopup.action = #selector(optionsChanged(_:))
        profilePopup.setAccessibilityIdentifier("gray-profile-popup")
        for intent in RenderingIntent.allCases {
            intentPopup.addItem(withTitle: intent.displayName)
            intentPopup.lastItem?.tag = Int(intent.code)
        }
        intentPopup.selectItem(withTag: Int(options.intent.code))
        intentPopup.font = DS.sans(13)
        intentPopup.target = self
        intentPopup.action = #selector(optionsChanged(_:))
        intentPopup.setAccessibilityIdentifier("gray-profile-intent")
        bpcBox.font = DS.sans(13)
        bpcBox.target = self
        bpcBox.action = #selector(optionsChanged(_:))
        bpcBox.state = options.blackPointCompensation ? .on : .off
        bpcBox.setAccessibilityIdentifier("gray-profile-bpc")
        intentPopup.isEnabled = !isFloat
        bpcBox.isEnabled = !isFloat
        note.font = DS.sans(12)
        note.textColor = DS.textMuted
        note.preferredMaxLayoutWidth = sheetWidth - sheetInset * 2
        note.setAccessibilityIdentifier("gray-profile-note")

        var gridRows: [[NSView]] = [[fieldLabel("Profile:"), profilePopup]]
        if isConvert {
            gridRows += [[fieldLabel("Intent:"), intentPopup], [NSGridCell.emptyContentView, bpcBox]]
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
        cancelButton.setAccessibilityIdentifier("gray-profile-cancel")
        applyButton = sheetApplyButton(target: self, action: #selector(applyClicked(_:)))
        applyButton.setAccessibilityIdentifier("gray-profile-apply")
        refreshPlan()
        view = makeSheetView(
            title: isConvert ? "Convert to Profile" : "Assign Profile",
            hint: isConvert
                ? "Re-encodes every layer's gray values through the destination Gray profile so the "
                    + "picture keeps its appearance, keeping each layer, its mask and its transparency."
                : "Gives the document another Gray profile. The gray values are not touched, so the "
                    + "picture changes appearance.",
            content: content,
            buttonRow: makeButtonRow(cancel: cancelButton, apply: applyButton,
                                     leading: [sheetFootnote("one undo step")]))
    }

    private func fillProfiles() {
        profilePopup.removeAllItems()
        rows = []
        let working = ColorSettings.workingGray
        profilePopup.addItem(withTitle: "Working Gray — \(working.displayName)")
        rows.append(Row(data: nil, space: working.rawValue, destination: ["profile": "working_gray"]))
        for space in GrayWorkingSpace.allCases where space != working {
            profilePopup.addItem(withTitle: space.displayName)
            rows.append(Row(data: nil, space: space.rawValue, destination: ["profile": space.rawValue]))
        }
        for profile in InstalledProfiles.matching(kinds: ["gray"]) {
            guard let data = try? Data(contentsOf: profile.url) else { continue }
            profilePopup.addItem(withTitle: profile.name)
            rows.append(Row(data: data, space: nil, destination: ["profile": "installed", "name": profile.name]))
        }
        profilePopup.selectItem(at: 0)
    }

    private var selected: Row? {
        let index = profilePopup.indexOfSelectedItem
        return rows.indices.contains(index) ? rows[index] : nil
    }

    /// Asks the core what these settings would do.
    private func refreshPlan() {
        refusal = nil
        changes = true
        guard let typed = document?.doc?.precision, let row = selected else {
            refusal = "No Grayscale document."
            return syncControls()
        }
        do {
            let plan = try typed.grayProfilePlan(
                profile: row.data, request: GrayProfileEdit.request(kind, space: row.space, options: options))
            refusal = plan["refusal"] as? String
            changes = plan["changes"] as? Bool ?? false
        } catch {
            refusal = error.localizedDescription
        }
        syncControls()
    }

    private func syncControls() {
        applyButton?.isEnabled = refusal == nil && changes
        note.stringValue = refusal ?? (!changes
            ? "The document is already in this profile."
            : isConvert
                ? (isFloat
                    ? "A 32-bit document's linear gray is the same in every Gray profile: only the "
                        + "profile changes."
                    : "Relative colorimetric is the default conversion.")
                : "Only the profile changes: every gray value stays as it is.")
    }

    @objc private func optionsChanged(_ sender: Any?) {
        options.intent = RenderingIntent.allCases.first { Int($0.code) == intentPopup.selectedTag() } ?? .relative
        options.blackPointCompensation = bpcBox.state == .on
        refreshPlan()
    }

    @objc private func applyClicked(_ sender: Any?) {
        guard refusal == nil, changes, let document, let editor, let row = selected else {
            NSSound.beep()
            return
        }
        let kind = self.kind, options = self.options
        dismiss(self)
        editor.commitGrayProfileEdit(kind, profile: row.data, space: row.space, destination: row.destination,
                                     options: options, document: document)
    }

    @objc private func cancelClicked(_ sender: Any?) {
        dismiss(self)
    }
}
