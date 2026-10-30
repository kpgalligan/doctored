import AppKit

// The Tone Map sheet previews LIVE, as VIEW state: each change hands the candidate map
// to the window (`EditorViewController.setToneMapCandidate`), which draws the retained projection
// through it in the display-only `canvas.proofImage` slot. The sheet captures no projection, owns
// no renderer and never writes `canvas.previewImage`.

/// Image ▸ Tone Map… — how a 32-bit float document's extended-range samples
/// become SDR numbers for display and for every 8/16-bit deliverable
/// (`FloatDocument.swift`).
///
/// The preview is the window's own: a candidate map is the same kind of view
/// state as its exposure (`EditorViewController+ViewExposure.swift`), so the
/// candidate is drawn under that exposure, from the projection the document
/// already retains — no second composite — and the samplers and the clipping
/// overlay, which read `canvas.previewImage`, never see it. With no retained
/// projection there is no preview. Cancel clears the candidate; Apply commits
/// one undo step, the canvas redraws through the new map, and the candidate is
/// cleared after the commit returns.
final class ToneMapSheetController: NSViewController {
    private weak var editor: EditorViewController?
    /// The handle captured when the sheet opened, so Apply can tell whether
    /// the document moved underneath it.
    private let capture: PrecisionEditorCapture
    private let current: ToneMap

    private let operatorPopup = NSPopUpButton(frame: .zero, pullsDown: false)
    private let whiteField = NSTextField(string: "")
    private let note = ColorProfileControls.noteLabel()
    private var applyButton: NSButton!

    private var choice: ToneMap

    /// nil when the front document is not a 32-bit float document, which is
    /// the only kind that has a map. The caller beeps.
    init?(editor: EditorViewController) {
        guard let document = editor.document, let doc = document.doc, doc.isFloat,
              let map = doc.toneMap,
              let capture = try? PrecisionEditorCapture(document) else { return nil }
        self.editor = editor
        self.capture = capture
        current = map
        choice = map
        super.init(nibName: nil, bundle: nil)
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) {
        fatalError("ToneMapSheetController does not support NSCoder")
    }

    override func loadView() {
        for op in ToneMapOperator.allCases {
            operatorPopup.addItem(withTitle: op.displayName)
            operatorPopup.lastItem?.representedObject = op.rawValue
        }
        operatorPopup.selectItem(at: ToneMapOperator.allCases.firstIndex(of: current.operator) ?? 0)
        operatorPopup.font = DS.sans(13)
        operatorPopup.target = self
        operatorPopup.action = #selector(operatorChanged(_:))

        whiteField.font = DS.sans(13)
        whiteField.alignment = .right
        whiteField.stringValue = ToneMap.format(current.white)
        whiteField.target = self
        whiteField.action = #selector(whiteChanged(_:))
        whiteField.widthAnchor.constraint(equalToConstant: 96).isActive = true

        let grid = NSGridView(views: [
            [fieldLabel("Map:"), operatorPopup],
            [fieldLabel("White:"), whiteField],
        ])
        grid.rowSpacing = 10
        grid.columnSpacing = 12
        grid.column(at: 0).xPlacement = .trailing
        grid.column(at: 0).width = 84

        let content = NSStackView(views: [grid, note])
        content.orientation = .vertical
        content.alignment = .leading
        content.spacing = 12

        let cancelButton = sheetCancelButton(target: self, action: #selector(cancelClicked(_:)))
        applyButton = sheetApplyButton(target: self, action: #selector(applyClicked(_:)))
        syncControls()
        view = makeSheetView(
            title: "Tone Map",
            hint: "This document stores linear light with an extended range: values above "
                + "white and below zero are kept exactly. The tone map is how those samples "
                + "become SDR numbers — on screen, and in every 8-bit or 16-bit export. A "
                + "32-bit float export ignores it and writes the samples as they are.",
            content: content,
            buttonRow: makeButtonRow(
                cancel: cancelButton, apply: applyButton,
                leading: [sheetFootnote("one undo step")]))
    }

    // MARK: - State

    private var canApply: Bool { choice != current }

    private func syncControls() {
        whiteField.isEnabled = choice.operator.usesWhite
        note.stringValue = choice.operator.summary
        applyButton.isEnabled = canApply
    }

    // MARK: - Actions

    @objc private func operatorChanged(_ sender: Any?) {
        let raw = operatorPopup.selectedItem?.representedObject as? String
        choice.operator = raw.flatMap(ToneMapOperator.init(rawValue:)) ?? current.operator
        syncControls()
        editor?.setToneMapCandidate(choice)
    }

    @objc private func whiteChanged(_ sender: Any?) {
        // The core is the authority on the range; this keeps the field from
        // committing something it would only refuse.
        let value = Double(whiteField.stringValue) ?? choice.white
        choice.white = min(max(value, ToneMap.whiteRange.lowerBound), ToneMap.whiteRange.upperBound)
        whiteField.stringValue = ToneMap.format(choice.white)
        syncControls()
        editor?.setToneMapCandidate(choice)
    }

    @objc private func applyClicked(_ sender: Any?) {
        // Commit whatever the white field holds even if the user never
        // pressed Return in it.
        whiteChanged(nil)
        guard canApply else {
            NSSound.beep()
            return
        }
        let map = choice
        dismiss(self)
        editor?.commitToneMap(map, capture: capture)
        // The commit is synchronous: the document now carries the map (or
        // refused it), so the candidate has nothing left to show.
        editor?.setToneMapCandidate(nil)
    }

    @objc private func cancelClicked(_ sender: Any?) {
        editor?.setToneMapCandidate(nil)
        dismiss(self)
    }
}
