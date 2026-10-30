import AppKit

/// Targeted Adjustment and Replace Color sampling: a dialog that
/// listens to the canvas while it is up. Curves and Hue/Saturation drag on
/// the image to move the curve point or the hue range under the cursor;
/// Replace Color clicks to pick the colours it replaces. All three read the
/// adjustment's INPUT — the pixels the op is about to transform — never the
/// committed composite, which would already carry the correction.
///
/// No MCP twin: what the gesture produces is the op's own params, which
/// `apply_filter` and the adjustment-layer tools already take.

/// One left-button event the monitor delivered, in canvas (document pixel)
/// coordinates.
struct CanvasPointerEvent {
    enum Phase { case down, dragged, up }
    let phase: Phase
    let point: CGPoint
    let modifiers: NSEvent.ModifierFlags
}

/// A local event monitor on the canvas's window for the left button: a
/// press inside `canvas.visibleRect` starts a gesture, and every event of it
/// — press, drags, release — is CONSUMED and handed to `onEvent`, so the
/// active tool never sees it. A press anywhere else passes through
/// untouched. `stop()` removes the monitor; the owning sheet calls it at
/// teardown (and deinit does too).
final class CanvasPointerMonitor {
    private weak var canvas: ImageCanvasView?
    private let onEvent: (CanvasPointerEvent) -> Void
    private var monitor: Any?
    private var tracking = false

    init(canvas: ImageCanvasView, onEvent: @escaping (CanvasPointerEvent) -> Void) {
        self.canvas = canvas
        self.onEvent = onEvent
        monitor = NSEvent.addLocalMonitorForEvents(
            matching: [.leftMouseDown, .leftMouseDragged, .leftMouseUp]
        ) { [weak self] event in
            guard let self else { return event }
            return self.handle(event) ? nil : event
        }
    }

    deinit {
        stop()
    }

    func stop() {
        if let monitor { NSEvent.removeMonitor(monitor) }
        monitor = nil
        tracking = false
    }

    /// True when the event belonged to a gesture and was consumed.
    private func handle(_ event: NSEvent) -> Bool {
        guard let canvas, let window = canvas.window, event.window === window else { return false }
        let point = canvas.convert(event.locationInWindow, from: nil)
        switch event.type {
        case .leftMouseDown:
            guard canvas.visibleRect.contains(point) else { return false }
            tracking = true
            onEvent(CanvasPointerEvent(phase: .down, point: point, modifiers: event.modifierFlags))
        case .leftMouseDragged:
            guard tracking else { return false }
            onEvent(CanvasPointerEvent(phase: .dragged, point: point, modifiers: event.modifierFlags))
        case .leftMouseUp:
            guard tracking else { return false }
            tracking = false
            onEvent(CanvasPointerEvent(phase: .up, point: point, modifiers: event.modifierFlags))
        default:
            return false
        }
        return true
    }
}

/// The adjustment's input colour under a canvas point.
enum CanvasSample {
    /// The RGB (0…255 floats) of the pixel at `point` in the scope the
    /// sheet's histogram plots (`Histogram.sheetPlan`): the layer being
    /// rewritten in `.destructive`, the backdrop below the insertion index in
    /// `.create`, the backdrop below the edited layer in `.edit`. One
    /// `ScopedAnalysisRead` at 8 and 16 bits; a 16-bit sample is scaled from
    /// its native codes. nil off the canvas, on a transparent pixel or when
    /// the read fails (a 32-bit float document has no 0…255 sample).
    static func rgb(
        _ document: ImageDocument, mode: AdjustmentSheetMode, destructiveLayer: Int?,
        at point: CGPoint
    ) -> [Double]? {
        guard let doc = document.doc, !doc.isFloat else { return nil }
        let x = Int(floor(point.x)), y = Int(floor(point.y))
        guard (0..<doc.width).contains(x), (0..<doc.height).contains(y) else { return nil }
        let plan = Histogram.sheetPlan(document, mode: mode, destructiveLayer: destructiveLayer)
        let composite = plan.source == .composite
        do {
            let read = try ScopedAnalysisRead(
                source: doc, scope: plan.source, selection: nil, point: (x, y),
                projection: composite ? document.retainedPrecisionProjection : nil,
                byteProjection: composite ? document.retainedByteProjection : nil)
            let report = try read.run()
            guard let rgba = report["center_rgba_codes"] as? [NSNumber], rgba.count == 4,
                  rgba[3].doubleValue > 0
            else { return nil }
            let scale = doc.isHighDepth ? 255.0 / 65535.0 : 1.0
            return rgba.prefix(3).map { $0.doubleValue * scale }
        } catch {
            return nil
        }
    }

    /// `rgb`'s sample in the space `op`'s kernel reads it in (0…255 floats): on a Lab Color
    /// document (#1 phase 2d) the codes as L*a*b* (`LabDocument.lab`) through
    /// `RasterLab.kernelRGB` — ProPhoto RGB for Hue/Saturation — so a sheet's sampler picks what
    /// the kernel rotates; on every other document `rgb` unchanged.
    static func kernelRGB(
        _ document: ImageDocument, mode: AdjustmentSheetMode, destructiveLayer: Int?,
        at point: CGPoint, op: String
    ) -> [Double]? {
        guard let codes = rgb(document, mode: mode, destructiveLayer: destructiveLayer, at: point)
        else { return nil }
        guard document.doc?.isLab == true else { return codes }
        let lab = LabDocument.lab(codes.map { $0 / 255 })
        return RasterLab.kernelRGB(op: op, lab: [lab.l, lab.a, lab.b])?.map { $0 * 255 }
    }
}
