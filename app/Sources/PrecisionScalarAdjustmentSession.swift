import AppKit

/// Scalar previews evaluate the same native candidate as Apply and explicitly
/// reduce only its grayscale display. The source owner/target/selection are
/// captured when the existing adjustment sheet opens.
final class PrecisionScalarAdjustmentSession {
    private weak var document: ImageDocument?
    private weak var canvas: ImageCanvasView?
    private let capture: PrecisionEditorCapture
    private let previewBase: ChannelBase
    private let renderer = PreviewRenderer()
    private var control: RasterJobControl?
    private var generation: UInt64 = 0
    private let result = ScalarPreviewResult()
    let status: NSTextField

    init(document: ImageDocument, canvas: ImageCanvasView, capture: PrecisionEditorCapture, status: NSTextField) {
        self.document = document; self.canvas = canvas; self.capture = capture; self.status = status
        previewBase = canvas.channelDisplay?.baseKind ?? .composite
        renderer.onRender = { [weak self] image in
            guard let self, let document = self.document else { return }
            let delivered = self.result.read()
            guard delivered.0 == self.generation else { return }
            do {
                try self.capture.requireCurrent(document)
                self.canvas?.previewImage = image
                self.status.stringValue = delivered.1 ?? ""
            } catch { self.canvas?.previewImage = nil; self.status.stringValue = error.localizedDescription }
        }
    }

    func request(filter: String, params: [String: Any]) {
        guard let document else { return }
        generation &+= 1; let generation = generation
        control?.cancel()
        do {
            try capture.requireCurrent(document)
            let edit = try PrecisionScalarOperations.filter(capture, filter: filter, params: params)
            let reservedBytes = edit.scratchEstimate + UInt64(capture.facade.width * capture.facade.height) * 24
            let reservation = try SelectionWorkBudget.reservePreview(bytes: reservedBytes, outputBytes: 0)
            control = reservation.control
            let previewControl = reservation.control
            renderer.cancelCompute = { previewControl.cancel() }
            let captured = capture, result = result, base = previewBase
            renderer.request { reservation.run { token in
                do {
                    let source = try edit.compute(control: token, ownReservation: reservedBytes) ?? captured.source
                    guard !token.isCancelled else { return nil }
                    let doc = try RasterDocument(precision: source)
                    let image = try PrecisionScalarOperations.preview(doc, base: base, layer: captured.layer, ownReservation: reservedBytes)
                    result.set(generation, nil)
                    return token.isCancelled ? nil : image
                } catch { result.set(generation, error.localizedDescription); return nil }
            } }
        } catch { canvas?.previewImage = nil; status.stringValue = error.localizedDescription }
    }

    /// `onLateBusy`: as in `PrecisionAdjustmentSession.commit(filter:params:onLateBusy:)`.
    func commit(filter: String, params: [String: Any], onLateBusy: (() -> Void)? = nil) {
        cancel()
        guard let document else { return }
        do {
            try capture.requireCurrent(document)
            if capture.target.isPlane, !document.confirmRasterize(layer: capture.layer) { return }
            try capture.requireCurrent(document)
            BackgroundWork.waitForAnonymousOccupant()
            let edit = try PrecisionScalarOperations.filter(capture, filter: filter, params: params)
            var steps = SelectedAdjustment.record(filter: filter, params: params)
            for i in steps.indices { steps[i].arguments["target"] = capture.target.agentName(in: capture.facade) }
            _ = try capture.publish(edit.run(title: "Apply \(filter)"), to: document, name: "Apply \(filter)", record: steps)
        } catch {
            if let onLateBusy, BackgroundWork.isBusyRefusal(error) {
                NSSound.beep()
                onLateBusy()
            } else { document.presentError(error) }
        }
    }

    func cancel() {
        generation &+= 1; control?.cancel(); renderer.cancel(); canvas?.previewImage = nil
    }
}

private final class ScalarPreviewResult {
    private let lock = NSLock()
    private var value: (UInt64, String?) = (0, nil)
    func set(_ generation: UInt64, _ error: String?) { lock.lock(); value = (generation, error); lock.unlock() }
    func read() -> (UInt64, String?) { lock.lock(); defer { lock.unlock() }; return value }
}

extension PrecisionScalarOperations {
    static func preview(_ doc: RasterDocument, base: ChannelBase, layer: Int, ownReservation: UInt64 = 0) throws -> CGImage? {
        let plane: PrecisionScalarBuffer
        switch base {
        case .channel(let index): plane = try read(doc, target: "channel", index: index, ownReservation: ownReservation)
        case .layerMask: plane = try read(doc, target: "mask", index: layer, canvas: true, ownReservation: ownReservation)
        case .plane(let component): plane = try sourcePlane(doc, plane: component, scope: .composite, ownReservation: ownReservation)
        case .composite: return PreparedDocumentEdit(doc).preview
        // #1: one CMYK ink plane previews as the Channels panel shows it (the composite's plate
        // on paper, ink dark), so an edit of that plate shows on the canvas.
        case .ink(let plate):
            return doc.planeImage(for: .ink(plate), composite: nil, activeLayer: layer, maxSide: 0)?
                .makeCGImage(in: ColorProfile.sRGB)
        case .nothing: return nil
        }
        return display(plane)?.makeCGImage(in: ColorProfile.sRGB)
    }
}
