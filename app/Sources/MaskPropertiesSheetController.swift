import AppKit

/// Layer > Mask > Properties. The preview and commit use the same original
/// coverage; density and feather remain editable native properties.
final class MaskPropertiesSheetController: NSViewController {
    private weak var document: ImageDocument?
    private weak var canvas: ImageCanvasView?
    private let capture: MaskEditCapture
    private let density = NSSlider(value: 100, minValue: 0, maxValue: 100, target: nil, action: nil)
    private let feather = NSSlider(value: 0, minValue: 0, maxValue: 250, target: nil, action: nil)
    private let densityValue = NSTextField(string: "100")
    private let featherValue = NSTextField(string: "0")
    private var displayedDensityText = ""
    private var displayedFeatherText = ""
    private let linked = NSButton(checkboxWithTitle: "Move mask with layer content", target: nil, action: nil)
    private let preview: MaskSheetPreview
    private let previewNotice = NSTextField(wrappingLabelWithString: "")

    init(document: ImageDocument, canvas: ImageCanvasView) throws {
        self.document = document
        self.canvas = canvas
        let capture = try MaskEditCapture(document)
        self.capture = capture
        preview = MaskSheetPreview(document: document, canvas: canvas, capture: capture)
        super.init(nibName: nil, bundle: nil)
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) { fatalError("MaskPropertiesSheetController does not support NSCoder") }

    override func loadView() {
        densityValue.identifier = NSUserInterfaceItemIdentifier("maskProperties.density")
        featherValue.identifier = NSUserInterfaceItemIdentifier("maskProperties.feather")
        linked.identifier = NSUserInterfaceItemIdentifier("maskProperties.linked")
        density.doubleValue = capture.properties.density * 100
        feather.maxValue = max(250, capture.properties.feather)
        feather.doubleValue = capture.properties.feather
        linked.state = capture.properties.linked ? .on : .off
        for slider in [density, feather] {
            slider.target = self; slider.action = #selector(changed(_:)); slider.isContinuous = true
            slider.widthAnchor.constraint(equalToConstant: 170).isActive = true
        }
        for field in [densityValue, featherValue] {
            DSField.style(field)
            field.widthAnchor.constraint(equalToConstant: 70).isActive = true
            field.target = self; field.action = #selector(fieldChanged(_:))
        }
        linked.target = self; linked.action = #selector(changed(_:))
        let grid = NSGridView(views: [[fieldLabel("Density (%)"), density, densityValue],
                                    [fieldLabel("Feather (px)"), feather, featherValue]])
        grid.columnSpacing = 10
        grid.rowSpacing = 12
        previewNotice.font = DS.sans(11)
        previewNotice.textColor = DS.textMuted
        previewNotice.preferredMaxLayoutWidth = 376
        let content = NSStackView(views: [grid, linked, previewNotice])
        content.orientation = .vertical; content.alignment = .leading; content.spacing = 16
        let cancel = sheetCancelButton(target: self, action: #selector(cancel(_:)))
        let apply = sheetApplyButton(target: self, action: #selector(apply(_:)))
        view = makeSheetView(title: "Mask Properties",
            hint: "Original mask coverage stays editable. Lower density reveals more of the layer; Feather softens the edge.",
            content: content, buttonRow: makeButtonRow(cancel: cancel, apply: apply))
        updateLabels()
    }

    private var edit: MaskEdit {
        .properties(linked: linked.state == .on, density: density.doubleValue / 100, feather: feather.doubleValue)
    }

    private func updateLabels() {
        displayedDensityText = String(format: "%.1f", density.doubleValue)
        displayedFeatherText = String(format: "%.2f", feather.doubleValue)
        densityValue.stringValue = displayedDensityText
        featherValue.stringValue = displayedFeatherText
    }

    @objc private func fieldChanged(_ sender: Any?) {
        guard readFields() else { NSSound.beep(); return }
        changed(sender)
    }

    private func readFields() -> Bool {
        // Formatting is presentation only: applying an untouched field must
        // retain imported precision and the exact value used by the preview.
        let proposedDensity = densityValue.stringValue == displayedDensityText
            ? density.doubleValue : Double(densityValue.stringValue)
        let proposedFeather = featherValue.stringValue == displayedFeatherText
            ? feather.doubleValue : Double(featherValue.stringValue)
        guard let d = proposedDensity, let f = proposedFeather,
              d.isFinite, f.isFinite, (0...100).contains(d), (0...1000).contains(f) else {
            return false
        }
        density.doubleValue = d
        feather.maxValue = max(250, f)
        feather.doubleValue = f
        return true
    }

    @objc private func changed(_ sender: Any?) {
        updateLabels()
        do {
            try preview.request(edit)
            previewNotice.stringValue = ""
        } catch {
            preview.cancel()
            previewNotice.stringValue = error.localizedDescription
        }
    }

    @objc private func apply(_ sender: Any?) {
        guard readFields() else { NSSound.beep(); return }
        preview.cancel()
        guard let document else { dismiss(self); return }
        do {
            let arguments: [String: Any] = ["layer": ActionArgs.activeLayer, "linked": linked.state == .on,
                                           "density": density.doubleValue / 100, "feather": feather.doubleValue]
            try document.applyMaskEdit(edit, capture: capture, name: "Mask Properties",
                record: .step("set_mask_properties", arguments, note: "Layer ▸ Mask ▸ Properties"))
            preview.finish()
            dismiss(self)
        } catch { document.presentError(error) }
    }

    @objc private func cancel(_ sender: Any?) {
        preview.finish(); dismiss(self)
    }
}
