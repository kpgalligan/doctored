import AppKit

/// View ▸ Rotate View… — an angle field, a slider, Reset, Cancel and Done, on
/// the shared `makeSheetView` / `makeButtonRow` builders.
///
/// It previews LIVE, which costs nothing: rotating the view is a frame
/// transform, not a render, and there is no document to put back. Cancel
/// restores the angle the sheet opened on; the caller owns that restore, so
/// this file never reaches into an editor.
final class RotateViewSheetController: NSObject {
    private let field = NSTextField()
    private let slider = NSSlider()
    private let start: Double
    private let onChange: (Double) -> Void
    private var modal: ModalCardWindowController?
    private var committed: Double?
    /// The angle `apply` last put into the controls — what Done commits. The
    /// slider is NOT the source of truth: a typed angle reaches it only
    /// through `apply`, so reading it back would drop a field edit.
    private var current: Double

    private init(current: Double, onChange: @escaping (Double) -> Void) {
        self.start = ViewRotation.normalize(current)
        self.current = ViewRotation.normalize(current)
        self.onChange = onChange
        super.init()
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) {
        fatalError("RotateViewSheetController does not support NSCoder")
    }

    /// Runs the sheet. The angle to keep, or nil when the user cancelled —
    /// the caller then puts `current` back.
    static func run(current: Double, onChange: @escaping (Double) -> Void) -> Double? {
        RotateViewSheetController(current: current, onChange: onChange).present()
    }

    private func present() -> Double? {
        DSField.style(field)
        // A typed angle must survive clicking Done without a Return: ending
        // the field's editing session sends its action, and `doneClicked`
        // ends that session before it reads.
        field.cell?.sendsActionOnEndEditing = true
        field.alignment = .right
        field.stringValue = Self.text(start)
        field.target = self
        field.action = #selector(fieldChanged(_:))
        field.widthAnchor.constraint(equalToConstant: 84).isActive = true
        field.setAccessibilityIdentifier("rotate-view-angle")

        slider.minValue = -180
        slider.maxValue = 180
        slider.doubleValue = start
        slider.target = self
        slider.action = #selector(sliderChanged(_:))
        slider.isContinuous = true
        slider.widthAnchor.constraint(equalToConstant: 260).isActive = true
        slider.setAccessibilityIdentifier("rotate-view-slider")

        let row = NSStackView(views: [fieldLabel("Angle"), field,
                                      NSTextField(labelWithString: "°")])
        row.orientation = .horizontal
        row.alignment = .firstBaseline
        row.spacing = 8
        let content = NSStackView(views: [row, slider])
        content.orientation = .vertical
        content.alignment = .leading
        content.spacing = 12

        let reset = StickerButton(title: "Reset", style: .secondary, target: self,
                                  action: #selector(resetClicked(_:)))
        let cancel = sheetCancelButton(target: self, action: #selector(cancelClicked(_:)))
        let done = StickerButton(title: "Done", style: .primary, target: self,
                                 action: #selector(doneClicked(_:)))
        done.setAccessibilityIdentifier("rotate-view-done")
        modal = ModalCardWindowController(title: "Rotate View", card: makeSheetView(
            title: "Rotate View",
            hint: "Turns the VIEW only — positive is clockwise. No pixel changes, nothing is "
                + "added to the history and the document is not marked edited. Each window has "
                + "its own rotation; the rulers hide while a view is turned.",
            content: content, buttonRow: makeButtonRow(cancel: cancel, apply: done,
                                                       leading: [reset])))
        _ = modal?.runModal()
        return committed
    }

    private static func text(_ degrees: Double) -> String {
        String(format: "%.2f", degrees)
    }

    private func apply(_ degrees: Double) {
        let normalized = ViewRotation.normalize(degrees)
        current = normalized
        field.stringValue = Self.text(normalized)
        slider.doubleValue = normalized
        onChange(normalized)
    }

    @objc private func fieldChanged(_ sender: Any?) {
        guard let value = Double(field.stringValue.trimmingCharacters(in: .whitespaces)),
              value.isFinite else {
            NSSound.beep()
            field.stringValue = Self.text(current)
            return
        }
        apply(value)
    }

    @objc private func sliderChanged(_ sender: Any?) {
        apply(slider.doubleValue)
    }

    @objc private func resetClicked(_ sender: Any?) {
        apply(0)
    }

    @objc private func cancelClicked(_ sender: Any?) {
        committed = nil
        modal?.end(.cancel)
    }

    @objc private func doneClicked(_ sender: Any?) {
        // Commit a half-typed field: this ends the field editor's session,
        // which sends `fieldChanged` and so updates `current`.
        modal?.window?.makeFirstResponder(nil)
        committed = current
        modal?.end(.OK)
    }
}
