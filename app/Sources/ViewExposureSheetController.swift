import AppKit

/// View ▸ 32-bit Exposure… — a stops field, a slider, Reset, Cancel and Done,
/// on the shared `makeSheetView` / `makeButtonRow` builders
/// (`RotateViewSheetController` is the template).
///
/// It previews LIVE: each change hands the stops to the caller, whose render
/// lands on the canvas a moment later, and there is no document to put back.
/// Cancel restores the exposure the sheet opened on; the caller owns that
/// restore, so this file never reaches into an editor.
final class ViewExposureSheetController: NSObject {
    private let field = NSTextField()
    private let slider = NSSlider()
    private let start: Double
    private let onChange: (Double) -> Void
    private var modal: ModalCardWindowController?
    private var committed: Double?
    /// The stops `apply` last put into the controls — what Done commits. The
    /// slider is NOT the source of truth: a typed value reaches it only
    /// through `apply`, so reading it back would drop a field edit.
    private var current: Double

    private init(current: Double, onChange: @escaping (Double) -> Void) {
        self.start = Self.bounded(current)
        self.current = Self.bounded(current)
        self.onChange = onChange
        super.init()
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) {
        fatalError("ViewExposureSheetController does not support NSCoder")
    }

    /// Runs the sheet. The stops to keep, or nil when the user cancelled —
    /// the caller then puts `current` back.
    static func run(current: Double, onChange: @escaping (Double) -> Void) -> Double? {
        ViewExposureSheetController(current: current, onChange: onChange).present()
    }

    private func present() -> Double? {
        DSField.style(field)
        // A typed value must survive clicking Done without a Return: ending
        // the field's editing session sends its action, and `doneClicked`
        // ends that session before it reads.
        field.cell?.sendsActionOnEndEditing = true
        field.alignment = .right
        field.stringValue = ViewExposure.text(start)
        field.target = self
        field.action = #selector(fieldChanged(_:))
        field.widthAnchor.constraint(equalToConstant: 84).isActive = true
        field.setAccessibilityIdentifier("view-exposure-stops")

        slider.minValue = ViewExposure.range.lowerBound
        slider.maxValue = ViewExposure.range.upperBound
        slider.doubleValue = start
        slider.target = self
        slider.action = #selector(sliderChanged(_:))
        slider.isContinuous = true
        slider.widthAnchor.constraint(equalToConstant: 260).isActive = true
        slider.setAccessibilityIdentifier("view-exposure-slider")

        let row = NSStackView(views: [fieldLabel("Exposure"), field,
                                      NSTextField(labelWithString: "stops")])
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
        done.setAccessibilityIdentifier("view-exposure-done")
        modal = ModalCardWindowController(title: "32-bit Exposure", card: makeSheetView(
            title: "32-bit Exposure",
            hint: "Changes how this WINDOW shows the 32-bit float picture — each stop doubles or "
                + "halves the light before the tone map, so a negative exposure brings values "
                + "above white into view. No pixel changes, nothing is added to the history and "
                + "the document is not marked edited. Readouts, exports and saved files are not "
                + "affected.",
            content: content, buttonRow: makeButtonRow(cancel: cancel, apply: done,
                                                       leading: [reset])))
        _ = modal?.runModal()
        return committed
    }

    private static func bounded(_ stops: Double) -> Double {
        min(max(stops, ViewExposure.range.lowerBound), ViewExposure.range.upperBound)
    }

    private func apply(_ stops: Double) {
        let bounded = Self.bounded(stops)
        current = bounded
        field.stringValue = ViewExposure.text(bounded)
        slider.doubleValue = bounded
        onChange(bounded)
    }

    @objc private func fieldChanged(_ sender: Any?) {
        guard let value = Double(field.stringValue.trimmingCharacters(in: .whitespaces)),
              value.isFinite else {
            NSSound.beep()
            field.stringValue = ViewExposure.text(current)
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
