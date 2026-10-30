import AppKit

/// Numeric mask-only translation/scale/rotation. LayerTransform supplies the
/// same pivot composition used by the canvas's existing Free Transform tool.
final class MaskTransformSheetController: NSViewController {
    private weak var document: ImageDocument?
    private weak var canvas: ImageCanvasView?
    private let capture: MaskEditCapture
    private let fields = ["0", "0", "100", "100", "0"].map { NSTextField(string: $0) }
    private let preview: MaskSheetPreview
    private let previewNotice = NSTextField(wrappingLabelWithString: "")

    init(document: ImageDocument, canvas: ImageCanvasView) throws {
        self.document = document; self.canvas = canvas
        let capture = try MaskEditCapture(document)
        self.capture = capture
        preview = MaskSheetPreview(document: document, canvas: canvas, capture: capture)
        super.init(nibName: nil, bundle: nil)
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) { fatalError("MaskTransformSheetController does not support NSCoder") }

    override func loadView() {
        let labels = ["Move X (px)", "Move Y (px)", "Scale X (%)", "Scale Y (%)", "Clockwise (°)"]
        let keys = ["translate_x", "translate_y", "scale_x", "scale_y", "angle"]
        for (field, key) in zip(fields, keys) {
            field.identifier = NSUserInterfaceItemIdentifier("maskTransform.\(key)")
        }
        let rows: [[NSView]] = zip(labels, fields).map { label, field in
            DSField.style(field)
            field.widthAnchor.constraint(equalToConstant: 120).isActive = true
            field.target = self; field.action = #selector(changed(_:))
            return [fieldLabel(label), field]
        }
        let grid = NSGridView(views: rows)
        grid.rowSpacing = 12; grid.columnSpacing = 16
        previewNotice.font = DS.sans(11)
        previewNotice.textColor = DS.textMuted
        previewNotice.preferredMaxLayoutWidth = 376
        let content = NSStackView(views: [grid, previewNotice])
        content.orientation = .vertical
        content.alignment = .leading
        content.spacing = 10
        let cancel = sheetCancelButton(target: self, action: #selector(cancel(_:)))
        let apply = sheetApplyButton(target: self, action: #selector(apply(_:)))
        view = makeSheetView(title: "Transform Mask", hint:
            "Move and resample the original mask around its center. Layer pixels stay in place; density and feather remain editable.",
            content: content, buttonRow: makeButtonRow(cancel: cancel, apply: apply))
    }

    private func transform() throws -> CGAffineTransform { try layerTransform().matrix }

    /// The one value both the matrix and the recorded `pivot` come from.
    private func layerTransform() throws -> LayerTransform {
        let values = fields.compactMap { Double($0.stringValue) }
        guard values.count == 5, values.allSatisfy(\.isFinite),
              abs(values[0]) <= 1_000_000, abs(values[1]) <= 1_000_000,
              (0.1...10_000).contains(abs(values[2])), (0.1...10_000).contains(abs(values[3])) else {
            throw DoctoredCoreError(message: "Enter finite offsets, nonzero scales from ±0.1% to ±10,000%, and a rotation.")
        }
        var transform = LayerTransform(pivot: CGPoint(x: capture.properties.bounds.midX, y: capture.properties.bounds.midY))
        transform.translation = CGVector(dx: values[0], dy: values[1])
        transform.scaleX = values[2] / 100; transform.scaleY = values[3] / 100
        transform.degrees = values[4]
        return transform
    }

    @objc private func changed(_ sender: Any?) {
        do {
            try preview.request(.transform(transform(), DR_FILTER_BILINEAR))
            previewNotice.stringValue = ""
        } catch {
            preview.cancel()
            previewNotice.stringValue = error.localizedDescription
        }
    }

    @objc private func apply(_ sender: Any?) {
        guard let document else { dismiss(self); return }
        do {
            let built = try layerTransform()
            let matrix = built.matrix
            preview.cancel()
            let arguments: [String: Any] = ["layer": ActionArgs.activeLayer,
                "matrix": [matrix.a, matrix.b, matrix.c, matrix.d, matrix.tx, matrix.ty].map(Double.init),
                "pivot": [Double(built.pivot.x), Double(built.pivot.y)]]
            try document.applyMaskEdit(.transform(matrix, DR_FILTER_BILINEAR), capture: capture,
                name: "Transform Mask", record: .step("transform_mask", arguments, note: "Layer ▸ Mask ▸ Transform"))
            preview.finish()
            dismiss(self)
        } catch { document.presentError(error) }
    }

    @objc private func cancel(_ sender: Any?) {
        preview.finish(); dismiss(self)
    }
}
