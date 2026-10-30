import AppKit

// This file holds `ClipDisplayMode`, `ClippingDrag.active` (the event's OWN ⌥ on a left press/drag,
// never hardware state), `ClipTrackingSlider` (Levels Black/White; `onTrackingEnded` drops the
// display), `ToneFeedbackBox` (the worker's bins/failure/clipped frame, lock-guarded) and
// `ToneSheetFeedback` (inactive for scalar targets, non-composite preview bases and while
// the channel view replaces the composite; `present` lands the frame then sets
// `canvas.clipDisplayPreview` before `previewImage`, so `colorSamplerPreviewChanged` never reads a
// threshold image). The byte worker is `byteFrame` / `OperationCapture.preview(_:clip:box:)`; the
// 16-bit and float one is `PrecisionAdjustmentSession.feedback` (float tests `>= 1.0` / `<= 0.0`).

/// The ⌥-drag clipping display and the live "After" histogram,
/// shared by the Levels and Curves sheets (destructive and adjustment-layer,
/// 8 and 16 bits). While the user ⌥-drags Levels Black/White or a Curves end
/// point the canvas shows a threshold view of the EDITED composite
/// (`dr_image_clip_display` / `dr_precision_image_clip_display`, exact
/// native 0/65535 at 16 bits); the "After" plot is the whole adjusted
/// composite the same preview render made. Both are transient: no document
/// state, no undo, no MCP twin (an agent reads `histogram` after
/// `apply_filter`).

/// Which endpoint the display thresholds — or `.overlay`, View ▸ Show
/// Clipping's transparent layer marking both (blue at 0, red at
/// the maximum, magenta for both; `EditorViewController+ClippingOverlay`).
enum ClipDisplayMode {
    case highlights, shadows, overlay

    var code: UInt32 {
        switch self {
        case .highlights: return DR_CLIP_DISPLAY_HIGHLIGHTS
        case .shadows: return DR_CLIP_DISPLAY_SHADOWS
        case .overlay: return DR_CLIP_DISPLAY_OVERLAY
        }
    }
}

enum ClippingDrag {
    /// `mode` only for a left-button press or drag whose OWN modifier flags
    /// hold ⌥ — never the hardware state, so a key change between events
    /// cannot flip a display the user did not ask for.
    static func active(_ mode: ClipDisplayMode, event: NSEvent?) -> ClipDisplayMode? {
        guard let event, event.type == .leftMouseDown || event.type == .leftMouseDragged,
              event.modifierFlags.contains(.option) else { return nil }
        return mode
    }
}

/// An `NSSlider` that says when its mouse tracking loop ends, so a ⌥-drag
/// display is dropped the moment the button is released.
final class ClipTrackingSlider: NSSlider {
    var onTrackingEnded: (() -> Void)?

    override init(frame frameRect: NSRect) {
        super.init(frame: frameRect)
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) {
        fatalError("ClipTrackingSlider does not support NSCoder")
    }

    override func mouseDown(with event: NSEvent) {
        super.mouseDown(with: event)
        onTrackingEnded?()
    }
}

/// What one preview render learned beside its image, handed from the worker
/// to the main thread. A render resets it first, so a frame is always the
/// render's own.
final class ToneFeedbackBox {
    struct Frame {
        var bins: HistogramBins?
        var failure: String?
        var clipped: Bool
    }

    private let lock = NSLock()
    private var frame: Frame?

    func reset() {
        lock.lock(); frame = nil; lock.unlock()
    }

    func set(_ frame: Frame) {
        lock.lock(); self.frame = frame; lock.unlock()
    }

    func take() -> Frame? {
        lock.lock()
        defer { lock.unlock() }
        let taken = frame
        frame = nil
        return taken
    }
}

/// One sheet's clipping/After state. Inactive (a scalar target, a
/// plane/channel/mask preview, or the channel view replacing the composite)
/// it hides the plot, never clips and presents a preview exactly as before.
final class ToneSheetFeedback {
    let isActive: Bool
    /// Main thread only; always nil when inactive.
    private(set) var clip: ClipDisplayMode?
    let box = ToneFeedbackBox()
    let afterPlot = HistogramView(frame: .zero)

    init(active: Bool) {
        isActive = active
    }

    /// The "After" caption over a 256×48 plot, hidden when inactive.
    func afterView() -> NSView {
        let caption = NSTextField(labelWithString: "After")
        caption.font = DS.sans(11)
        caption.textColor = DS.textMuted
        afterPlot.translatesAutoresizingMaskIntoConstraints = false
        NSLayoutConstraint.activate([
            afterPlot.widthAnchor.constraint(equalToConstant: 256),
            afterPlot.heightAnchor.constraint(equalToConstant: 48),
        ])
        let stack = NSStackView(views: [caption, afterPlot])
        stack.orientation = .vertical
        stack.alignment = .leading
        stack.spacing = 4
        stack.isHidden = !isActive
        return stack
    }

    /// True when the display mode changed (the caller then re-previews).
    @discardableResult func setClip(_ mode: ClipDisplayMode?) -> Bool {
        let next = isActive ? mode : nil
        guard next != clip else { return false }
        clip = next
        return true
    }

    /// Lands a render: its frame's bins (or failure) on the plot — a render
    /// with no frame keeps the plot — then the image on the canvas, marked
    /// as a threshold display so the Color Samplers never read it.
    func present(_ image: CGImage?, on canvas: ImageCanvasView?) {
        let frame = box.take()
        if let frame {
            if let bins = frame.bins {
                afterPlot.bins = bins
                afterPlot.unavailableReason = nil
            } else if let failure = frame.failure {
                afterPlot.bins = nil
                afterPlot.unavailableReason = failure
            }
        }
        canvas?.clipDisplayPreview = frame?.clipped == true ? image : nil
        canvas?.previewImage = image
    }

    /// The byte worker's frame: the After bins of `composite` (stride 1) when
    /// a box is given, then the threshold display (sRGB, display only) when
    /// clipping, else the composite itself in `colorSpace`.
    static func byteFrame(_ composite: RasterImage?, colorSpace: CGColorSpace,
                          clip: ClipDisplayMode?, box: ToneFeedbackBox?) -> CGImage? {
        box?.reset()
        guard let composite else { return nil }
        var frame = ToneFeedbackBox.Frame(bins: nil, failure: nil, clipped: false)
        if box != nil {
            frame.bins = Histogram.of(composite, selection: nil, stride: 1)
            if frame.bins == nil { frame.failure = "The after histogram could not be read." }
        }
        let image: CGImage?
        if let clip {
            image = composite.clipDisplay(mode: clip.code)?.makeCGImage(in: ColorProfile.sRGB)
            frame.clipped = true
        } else {
            image = composite.makeCGImage(in: colorSpace)
        }
        box?.set(frame)
        return image
    }
}

extension OperationCapture {
    /// `preview(_:)` with the After frame and the ⌥ display, for a composite
    /// preview only; any other base is the plain preview with an empty box.
    func preview(_ op: (RasterImage) -> RasterImage?, clip: ClipDisplayMode?,
                 box: ToneFeedbackBox?) -> CGImage? {
        guard previewBase == .composite else {
            box?.reset()
            return preview(op)
        }
        let result = applying(op) ?? doc
        return ToneSheetFeedback.byteFrame(result.flattened(), colorSpace: result.colorSpace,
                                           clip: clip, box: box)
    }
}
