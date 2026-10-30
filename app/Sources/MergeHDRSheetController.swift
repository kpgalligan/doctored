import AppKit

/// File ▸ Merge to HDR… and Layer ▸ Merge Layers to HDR… — the card
/// that shows each frame with its exposure value and asks for Align and
/// Remove Ghosts, on the shared `makeSheetView` / `makeButtonRow` builders.
///
/// A `ModalCardWindowController` rather than a document-modal sheet, for
/// `NewDocumentSheetController`'s reason: the files form runs with no
/// document open.
///
/// It decides nothing about the merge. A frame whose EV is unknown (the
/// estimate could not pair it, `MergeHDR.Prepared.notice`) opens with an empty
/// field, and Merge stays disabled until every field holds a finite number.
/// `evsEntered` is true when any field was edited or any frame was unknown,
/// so equal values the user typed are taken as meant.
final class MergeHDRSheetController: NSObject, NSTextFieldDelegate {
    private let frames: [MergeHDR.Frame]
    private let title: String
    private let notice: String?
    private var fields: [NSTextField] = []
    /// Each field's text as the card opened; a field that still shows it
    /// keeps the frame's own EV, undisturbed by the two-decimal display.
    private var initial: [String] = []
    private let align = NSButton(
        checkboxWithTitle: "Align the frames to the middle exposure", target: nil, action: nil)
    private let deghost = NSButton(
        checkboxWithTitle: "Remove ghosts (take moving areas from the middle exposure)",
        target: nil, action: nil)
    private var merge: NSButton?
    private var modal: ModalCardWindowController?
    private var result: MergeHDR.Options?

    private init(frames: [MergeHDR.Frame], title: String, notice: String?) {
        self.frames = frames
        self.title = title
        self.notice = notice
        super.init()
    }

    /// Runs the card and answers the options, or nil when the user cancelled.
    static func run(frames: [MergeHDR.Frame], title: String, notice: String?) -> MergeHDR.Options? {
        MergeHDRSheetController(frames: frames, title: title, notice: notice).present()
    }

    private static func text(_ ev: Double?) -> String {
        ev.map { String(format: "%.2f", $0) } ?? ""
    }

    private static func sourceText(_ source: MergeHDR.EVSource) -> String {
        switch source {
        case .exif: return "from the file's EXIF"
        case .estimated: return "estimated from the pixels"
        case .given: return "given"
        case .unknown: return "not known: enter it"
        }
    }

    private func present() -> MergeHDR.Options? {
        var rows: [NSView] = []
        if let notice {
            let label = NSTextField(wrappingLabelWithString: notice)
            label.font = DS.sans(12)
            label.textColor = DS.textStrong
            label.preferredMaxLayoutWidth = 440
            label.setAccessibilityIdentifier("merge-hdr-notice")
            rows.append(label)
        }
        for (index, frame) in frames.enumerated() {
            let name = fieldLabel(frame.name)
            name.lineBreakMode = .byTruncatingMiddle
            name.widthAnchor.constraint(equalToConstant: 190).isActive = true
            let field = NSTextField()
            DSField.style(field)
            field.alignment = .right
            field.stringValue = Self.text(frame.ev)
            field.placeholderString = "EV"
            field.delegate = self
            field.widthAnchor.constraint(equalToConstant: 72).isActive = true
            field.setAccessibilityIdentifier("merge-hdr-ev-\(index)")
            fields.append(field)
            initial.append(field.stringValue)
            let source = NSTextField(labelWithString: "EV, \(Self.sourceText(frame.source))")
            source.font = DS.sans(11)
            source.textColor = DS.textMuted
            let row = NSStackView(views: [name, field, source])
            row.orientation = .horizontal
            row.alignment = .firstBaseline
            row.spacing = 8
            rows.append(row)
        }
        align.font = DS.sans(12)
        align.state = .on
        align.setAccessibilityIdentifier("merge-hdr-align")
        deghost.font = DS.sans(12)
        deghost.state = .off
        deghost.setAccessibilityIdentifier("merge-hdr-deghost")
        rows.append(align)
        rows.append(deghost)
        let content = NSStackView(views: rows)
        content.orientation = .vertical
        content.alignment = .leading
        content.spacing = 8

        let cancel = sheetCancelButton(target: self, action: #selector(cancelClicked(_:)))
        let merge = StickerButton(
            title: "Merge", style: .primary, target: self, action: #selector(mergeClicked(_:)))
        merge.setAccessibilityIdentifier("merge-hdr-merge")
        self.merge = merge
        refreshMergeButton()
        modal = ModalCardWindowController(
            title: title,
            card: makeSheetView(
                title: title,
                hint: "Each frame is one exposure of the same scene. EV is its exposure in "
                    + "stops; only the differences between frames matter, and a larger number "
                    + "is a brighter frame. The result opens as a new 32-bit float document on "
                    + "the middle exposure's scale.",
                content: content, buttonRow: makeButtonRow(cancel: cancel, apply: merge),
                width: 500))
        guard modal?.runModal() == .OK else { return nil }
        return result
    }

    /// Every field as a finite number, or nil when one is not.
    private func parsed() -> [Double]? {
        var values: [Double] = []
        for field in fields {
            let text = field.stringValue.trimmingCharacters(in: .whitespaces)
            guard let value = Double(text), value.isFinite else { return nil }
            values.append(value)
        }
        return values
    }

    private func refreshMergeButton() {
        merge?.isEnabled = parsed() != nil
    }

    func controlTextDidChange(_ notification: Notification) {
        refreshMergeButton()
    }

    @objc private func cancelClicked(_ sender: Any?) {
        result = nil
        modal?.end(.cancel)
    }

    @objc private func mergeClicked(_ sender: Any?) {
        // Commit a half-typed field before it is read.
        modal?.window?.makeFirstResponder(nil)
        guard let values = parsed() else {
            NSSound.beep()
            refreshMergeButton()
            return
        }
        var evs: [Double] = []
        var edited = false
        for (index, frame) in frames.enumerated() {
            // An untouched field keeps the frame's own value at full precision.
            if let own = frame.ev, fields[index].stringValue == initial[index] {
                evs.append(own)
            } else {
                evs.append(values[index])
                edited = true
            }
        }
        result = MergeHDR.Options(
            evs: evs, evsEntered: edited || frames.contains { $0.source == .unknown },
            align: align.state == .on, deghost: deghost.state == .on)
        modal?.end(.OK)
    }
}
