import AppKit

/// Numeric affine controls transform coverage independently of every layer.
/// The center of the captured selection bounds is the default rotation pivot.
final class SelectionTransformSheetController: NSViewController {
    private weak var editor: EditorViewController?
    let capture: SelectionCapture
    private let coverage: [UInt8]
    private let nativeCoverage: [UInt16]?
    private let anchor: CGPoint
    private let maskView = SelectionPreviewView(frame: .zero)
    private let sourceView = SelectionPreviewView(frame: .zero)
    private var fields: [String: NSTextField] = [:]
    private let filterPopup = NSPopUpButton()
    private let status = NSTextField(wrappingLabelWithString: "")
    private var applyButton: NSButton!
    private let renderer = PreviewRenderer()
    private var previewToken: RasterJobControl?
    private var previewGeneration = 0
    private let previewStamp = RefinementPreviewStamp()

    init(editor: EditorViewController, document: ImageDocument) throws {
        self.editor = editor
        capture = try SelectionCapture(document: document)
        guard let selection = capture.selection.selection else {
            throw DoctoredCoreError(message: "Make a selection before transforming its coverage.")
        }
        coverage = selection.maskBytes()
        nativeCoverage = capture.doc.isHighDepth || selection.hasNativeCoverage ? selection.maskSamples16() : nil
        anchor = CGPoint(x: selection.bounds.midX, y: selection.bounds.midY)
        super.init(nibName: nil, bundle: nil)
        sourceView.image = capture.source?.makeCGImage(in: capture.doc.colorSpace)
        maskView.image = CanvasSelection.grayImage(coverage, capture.doc.width, capture.doc.height)
        renderer.onRender = { [weak self] image in
            guard let self, self.previewStamp.value == self.previewGeneration else { return }
            guard let document = self.editor?.document, self.capture.isCurrent(in: document) else {
                self.maskView.image = nil; self.applyButton?.isEnabled = false
                self.status.stringValue = "The image, target or selection changed. Reopen Transform Selection."
                return
            }
            self.maskView.image = image
        }
    }
    @available(*, unavailable)
    required init?(coder: NSCoder) { fatalError("SelectionTransformSheetController does not support NSCoder") }

    override func loadView() {
        let specifications: [(String, String, Double)] = [
            ("translate_x", "Horizontal move (px):", 0), ("translate_y", "Vertical move (px):", 0),
            ("scale_x", "Horizontal scale (%):", 100), ("scale_y", "Vertical scale (%):", 100),
            ("angle", "Clockwise angle (°):", 0),
        ]
        var rows: [[NSView]] = specifications.map { key, label, value in
            let field = selectionSheetField(value, identifier: "selectionTransform.\(key)",
                                            target: self, action: #selector(valueChanged(_:)))
            fields[key] = field
            return [fieldLabel(label), field]
        }
        filterPopup.addItems(withTitles: ["Nearest", "Bilinear", "Bicubic", "Lanczos 3"])
        filterPopup.selectItem(at: 1)
        filterPopup.target = self
        filterPopup.action = #selector(valueChanged(_:))
        rows.append([fieldLabel("Interpolation:"), filterPopup])
        let grid = NSGridView(views: rows)
        grid.rowSpacing = 9
        grid.columnSpacing = 12
        status.font = DS.sans(11)
        status.textColor = DS.textMuted
        status.preferredMaxLayoutWidth = 510
        status.stringValue = "Pivot: \(Int(anchor.x)), \(Int(anchor.y)) px. Negative scale flips coverage."
        let content = NSStackView(views: [selectionPreviewRow(sourceView, maskView), grid, status])
        content.orientation = .vertical
        content.alignment = .leading
        content.spacing = 12
        applyButton = sheetApplyButton(target: self, action: #selector(applyClicked(_:)))
        view = makeSheetView(title: "Transform Selection",
            hint: "Move, scale, flip or rotate the current selection around its center. Coverage outside the canvas is clipped. Layer pixels stay in place.",
            content: content, buttonRow: makeButtonRow(
                cancel: sheetCancelButton(target: self, action: #selector(cancelClicked(_:))),
                apply: applyButton), width: 556)
    }

    var parameters: SelectionTransformParameters {
        func value(_ key: String) -> Double { Double(fields[key]?.stringValue ?? "") ?? .nan }
        return SelectionTransformParameters(translateX: value("translate_x"), translateY: value("translate_y"),
            scaleX: value("scale_x") / 100, scaleY: value("scale_y") / 100,
            angle: value("angle"), anchor: anchor,
            filter: ["nearest", "bilinear", "bicubic", "lanczos3"][max(0, filterPopup.indexOfSelectedItem)])
    }

    func requestPreview() {
        previewGeneration += 1
        let generation = previewGeneration, stamp = previewStamp
        maskView.image = nil
        previewToken?.cancel()
        do {
            let parameters = try parameters.validated()
            let width = capture.doc.width, height = capture.doc.height
            if let input = nativeCoverage {
                let work = try NativeSelectionWork(width: width, height: height, operation: parameters.nativeOperation, input: input)
                let total = work.scratchEstimate + UInt64(width * height) * 4
                let reservation = try SelectionWorkBudget.reservePreview(bytes: total, outputBytes: UInt64(width * height) * 4)
                let token = reservation.control; previewToken = token; renderer.cancelCompute = { token.cancel() }
                renderer.request { reservation.run { token in
                    guard let mask = try? work.compute(control: token, ownReservation: total), !token.isCancelled else { return nil }
                    stamp.set(generation)
                    return CanvasSelection.grayImage(mask.map { UInt8((UInt32($0) + 128) / 257) }, width, height)
                } }
                applyButton.isEnabled = true
                status.stringValue = "Native coverage at 65,536 levels. Preview is display only."
                return
            }
            let reservation = try SelectionWorkBudget.reservePreview(
                bytes: UInt64(capture.doc.width * capture.doc.height) * 8 + 4096,
                outputBytes: UInt64(capture.doc.width * capture.doc.height))
            let token = reservation.control
            previewToken = token
            renderer.cancelCompute = { token.cancel() }
            let coverage = self.coverage
            renderer.request { reservation.run { token in
                guard let transformed = try? RasterImage.transformingSelection(coverage,
                    width: width, height: height, matrix: parameters.matrix,
                    filter: parameters.sampler, control: token), !token.isCancelled else { return nil }
                stamp.set(generation)
                return CanvasSelection.grayImage(transformed, width, height)
            } }
            applyButton.isEnabled = true
            status.stringValue = "Pivot: \(Int(anchor.x)), \(Int(anchor.y)) px. Negative scale flips coverage."
        } catch {
            applyButton.isEnabled = false
            maskView.image = nil
            status.stringValue = error.localizedDescription
        }
    }

    @objc private func valueChanged(_ sender: Any?) { requestPreview() }
    @objc func applyClicked(_ sender: Any?) {
        guard let parameters = try? parameters.validated() else { NSSound.beep(); return }
        renderer.cancel()
        dismiss(self)
        editor?.applySelectionTransform(capture, parameters: parameters)
    }
    @objc func cancelClicked(_ sender: Any?) { renderer.cancel(); dismiss(self) }
}
