import AppKit

/// Window ▸ Layer Comps ▸ New… and Options… — a name, a comment and the three
/// capture checkboxes, on the shared `makeSheetView` / `makeButtonRow`
/// builders so it looks exactly like every other dialog in the app
/// (`WorkspaceNameSheetController` is the pattern).
///
/// It only COLLECTS the values; every rule about them — the name's bounds,
/// uniqueness, the comment's bounds, the "capture at least one attribute"
/// refusal — belongs to the core, and `ImageDocument`'s entry points are
/// where they are met.
final class LayerCompOptionsSheetController: NSObject {
    /// What the sheet collected.
    struct Result {
        let name: String
        let comment: String
        let captures: LayerCompCaptures
        /// Only meaningful for Options…: re-record the comp from the
        /// document's current state as well as changing its name and flags.
        let recapture: Bool
    }

    private let field = NSTextField()
    private let comment = NSTextField()
    private let visibility = NSButton(checkboxWithTitle: "Visibility", target: nil, action: nil)
    private let position = NSButton(checkboxWithTitle: "Position", target: nil, action: nil)
    private let appearance = NSButton(checkboxWithTitle: "Appearance", target: nil, action: nil)
    private let recapture = NSButton(
        checkboxWithTitle: "Re-record from the document's current state", target: nil, action: nil)
    private let message = NSTextField(wrappingLabelWithString: "")
    private let title: String
    private let applyTitle: String
    private let offersRecapture: Bool
    private var modal: ModalCardWindowController?
    private var result: Result?

    private init(title: String, applyTitle: String, offersRecapture: Bool) {
        self.title = title
        self.applyTitle = applyTitle
        self.offersRecapture = offersRecapture
        super.init()
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) {
        fatalError("LayerCompOptionsSheetController does not support NSCoder")
    }

    /// The New… sheet: an empty name and all three attributes captured, the
    /// core's own default.
    static func runNew() -> Result? {
        let controller = LayerCompOptionsSheetController(
            title: "New Layer Comp", applyTitle: "Save Layer Comp", offersRecapture: false)
        return controller.present(name: "", comment: "", captures: .all, recapture: true)
    }

    /// The Options… sheet for an existing comp.
    static func runOptions(for comp: LayerComp) -> Result? {
        let controller = LayerCompOptionsSheetController(
            title: "Layer Comp Options", applyTitle: "Update", offersRecapture: true)
        return controller.present(
            name: comp.name, comment: comp.comment, captures: comp.captures, recapture: false)
    }

    private func present(
        name: String, comment commentText: String, captures: LayerCompCaptures, recapture on: Bool
    ) -> Result? {
        DSField.style(field)
        field.stringValue = name
        field.placeholderString = "Layer comp name"
        field.widthAnchor.constraint(equalToConstant: 330).isActive = true
        field.setAccessibilityIdentifier("layer-comp-name")
        DSField.style(comment)
        comment.stringValue = commentText
        comment.placeholderString = "Comment (optional)"
        comment.widthAnchor.constraint(equalToConstant: 330).isActive = true
        comment.setAccessibilityIdentifier("layer-comp-comment")
        visibility.state = captures.visibility ? .on : .off
        position.state = captures.position ? .on : .off
        appearance.state = captures.appearance ? .on : .off
        recapture.state = on ? .on : .off
        recapture.setAccessibilityIdentifier("layer-comp-recapture")

        message.font = DS.sans(11)
        message.textColor = DS.textMuted
        message.preferredMaxLayoutWidth = 340
        let flags = NSStackView(views: [visibility, position, appearance])
        flags.orientation = .horizontal
        flags.spacing = 12
        var rows: [NSView] = [field, comment, flags]
        if offersRecapture { rows.append(recapture) }
        rows.append(message)
        let content = NSStackView(views: rows)
        content.orientation = .vertical
        content.alignment = .leading
        content.spacing = 10

        let cancel = sheetCancelButton(target: self, action: #selector(cancelClicked(_:)))
        let apply = StickerButton(
            title: applyTitle, style: .primary, target: self, action: #selector(applyClicked(_:)))
        apply.setAccessibilityIdentifier("layer-comp-apply")
        modal = ModalCardWindowController(title: title, card: makeSheetView(
            title: title,
            hint: "A comp records per-layer visibility, position and appearance — never pixels, "
                + "never the selection and never the history. Applying one is a single undo step, "
                + "and the list travels in the .doctored at both depths.",
            content: content, buttonRow: makeButtonRow(cancel: cancel, apply: apply)))
        _ = modal?.runModal()
        return result
    }

    @objc private func cancelClicked(_ sender: Any?) {
        modal?.end(.cancel)
    }

    @objc private func applyClicked(_ sender: Any?) {
        let trimmed = field.stringValue.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty else {
            message.stringValue = "A layer comp needs a name."
            NSSound.beep()
            return
        }
        let captures = LayerCompCaptures(
            visibility: visibility.state == .on,
            position: position.state == .on,
            appearance: appearance.state == .on)
        guard captures.visibility || captures.position || captures.appearance else {
            message.stringValue = "A layer comp must capture at least one attribute."
            NSSound.beep()
            return
        }
        result = Result(
            name: trimmed, comment: comment.stringValue, captures: captures,
            recapture: !offersRecapture || recapture.state == .on)
        modal?.end(.OK)
    }
}
