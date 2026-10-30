import AppKit

// Previews retain their own registered scratch reservation and exact captured
// target/view/selection.

/// Captured native math sessions share the normal UI controls and central
/// publication. A changed document, target or selection invalidates Apply.
final class NativeChannelMathSession {
    weak var document: ImageDocument?
    let capture: PrecisionEditorCapture
    let previewBase: ChannelBase
    private var control: RasterJobControl?
    private var generation: UInt64 = 0
    private let previewState = NativeMathPreviewState()
    let status = NSTextField(wrappingLabelWithString: "")

    init(_ document: ImageDocument, canvas: ImageCanvasView) throws {
        self.document = document
        capture = try PrecisionEditorCapture(document)
        previewBase = canvas.channelDisplay?.baseKind ?? .composite
    }
    func current() -> Bool {
        guard let document else { return false }
        return (try? capture.requireCurrent(document)) != nil
    }
    func cancel() { generation &+= 1; control?.cancel(); control = nil }

    func preview(apply: ApplyImageParameters? = nil, calculations: CalculationsParameters? = nil,
                 renderer: PreviewRenderer) throws {
        guard let document else { throw DoctoredCoreError(message: "The channel math document closed.") }
        try capture.requireCurrent(document)
        control?.cancel(); generation &+= 1
        let sequence = Int(truncatingIfNeeded: generation)
        let scratch = try NativeChannelMath.scratch(capture.facade, apply: apply, calculations: calculations)
        let reservation = try SelectionWorkBudget.reservePreview(bytes: scratch, outputBytes: 0)
        control = reservation.control
        renderer.cancelCompute = { reservation.control.cancel() }
        let capture = capture, base = previewBase, previewState = previewState
        renderer.request { reservation.run { token in
            do {
                let image: CGImage?
                if let apply {
                    let result = try NativeChannelMath.apply(capture, parameters: apply, control: token,
                        ownReservation: scratch) ?? capture.source
                    let doc = try RasterDocument(precision: result)
                    image = try PrecisionScalarOperations.preview(doc, base: base, layer: capture.layer, ownReservation: scratch)
                } else if let calculations {
                    let values = try NativeChannelMath.calculated(capture.facade, parameters: calculations,
                        control: token, ownReservation: scratch)
                    image = CanvasSelection.grayImage(values.map { UInt8((UInt32($0) + 128) / 257) },
                        capture.facade.width, capture.facade.height)
                } else { image = nil }
                guard !token.isCancelled else { return nil }
                previewState.set(sequence, nil)
                return image
            } catch { previewState.set(sequence, error.localizedDescription); return nil }
        } }
    }

    func acceptsPreview() -> Bool {
        let receipt = previewState.read()
        guard receipt.0 == Int(truncatingIfNeeded: generation) else { return false }
        status.stringValue = receipt.1 ?? ""
        guard current() else { status.stringValue = "The document, target or selection changed. Reopen this dialog."; return false }
        return true
    }
    func content(_ content: NSView) -> NSView {
        status.textColor = .systemRed; status.font = DS.sans(11); status.preferredMaxLayoutWidth = 350
        status.maximumNumberOfLines = 3
        let stack = NSStackView(views: [content, status]); stack.orientation = .vertical; stack.alignment = .leading; stack.spacing = 6
        return stack
    }

    func apply(_ parameters: ApplyImageParameters, record: [ActionStep]) throws {
        guard let document else { throw DoctoredCoreError(message: "The channel math document closed.") }
        cancel(); try capture.requireCurrent(document)
        if parameters.target == .layer || parameters.target.isPlane {
            guard document.confirmRasterize(layer: parameters.targetLayer) else { return }
            try capture.requireCurrent(document)
        }
        let captured = capture
        let result = try PrecisionCapturedWork.run(scratchEstimate: NativeChannelMath.scratch(capture.facade, apply: parameters), title: "Apply Image") {
            try NativeChannelMath.apply(captured, parameters: parameters, control: $0)
        }
        _ = try capture.publish(result, to: document, name: "Apply Image", record: record)
    }
    func calculate(_ parameters: CalculationsParameters, result: CalculationsResult, record: [ActionStep]) throws {
        guard let document else { throw DoctoredCoreError(message: "The channel math document closed.") }
        cancel(); try capture.requireCurrent(document)
        let captured = capture
        let resultValues = try PrecisionCapturedWork.run(scratchEstimate: NativeChannelMath.scratch(capture.facade, calculations: parameters), title: "Calculations") { token in
            try NativeChannelMath.calculated(captured.facade, parameters: parameters, control: token)
        }
        try capture.requireCurrent(document)
        if result == .selection {
            document.setCanvasSelection(CanvasSelection(shape: .mask16(resultValues), canvasWidth: capture.facade.width,
                canvasHeight: capture.facade.height), actionName: "Calculations Selection", record: record)
        } else {
            let work = try PrecisionScalarWork(source: capture.source, command: .setChannel(name: parameters.name), samples: resultValues)
            _ = try capture.publish(work.run(title: "Save Calculations Channel"), to: document, name: "Calculations", record: record)
        }
    }
}


private final class NativeMathPreviewState {
    private let lock = NSLock()
    private var value: (Int, String?) = (-1, nil)
    func set(_ generation: Int, _ message: String?) { lock.lock(); value = (generation, message); lock.unlock() }
    func read() -> (Int, String?) { lock.lock(); defer { lock.unlock() }; return value }
}
