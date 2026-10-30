import AppKit

// `ImageCanvasView` is now a subview of an unrotated `CanvasHostView`, which is what the scroll
// view scrolls; only the canvas's `frameCenterRotation` turns, so its bounds are still document
// pixels, the clip view still scrolls along the screen's axes, and every pointer site's existing
// `convert(event.locationInWindow, from: nil)` resolves the rotation with NO call-site change.
// Positive is CLOCKWISE on screen (the host is flipped, so AppKit's counterclockwise-positive
// rotation reads that way) and `ViewRotation.hostSize`/`canvasOrigin` are the IDENTITY at 0°, which
// is what keeps every `get_view`/Navigator number bit-identical to 12C-2's; `layoutCanvasHost` is
// the ONE place that geometry is written and assigns `frameCenterRotation` only when it differs, so
// a rotate-and-return leaves no float residue.

/// Phase 12D-2's VIEW-ONLY rotation: the geometry, the ⌥-drag / trackpad
/// session, and the unrotated host view the canvas now sits inside.
///
/// **No pixel is touched.** `ImageCanvasView` keeps its document-sized bounds
/// and its own flipped coordinate system; only its `frameCenterRotation`
/// inside `CanvasHostView` changes. Every tool, guide, marquee and handle
/// therefore keeps drawing and hit-testing in DOCUMENT space, and AppKit's own
/// `convert(_:from:)` — which every pointer site in the canvas already uses —
/// resolves the rotation for free.
///
/// **Positive is CLOCKWISE on screen**. `frameCenterRotation`
/// is counterclockwise-positive in the SUPERVIEW's own numbers; the host is
/// flipped, so those numbers count y downward and the same matrix reads as a
/// clockwise turn on screen. Measured rather than assumed: for a flipped child
/// in a flipped host, `host.convert(p, from: canvas)` is exactly
/// `hostCentre + R(θ) · (p − canvasCentre)` with `R = [[cos, −sin], [sin, cos]]`,
/// which is the mapping `rotate-view-stroke-ui` recomputes independently.
enum ViewRotation {
    /// Into (−180, 180]. A half turn has two spellings and this picks one:
    /// 180 stays 180, −180 becomes 180, so a read-back never contradicts the
    /// request by a sign.
    static func normalize(_ degrees: Double) -> Double {
        guard degrees.isFinite else { return 0 }
        var value = degrees.truncatingRemainder(dividingBy: 360)
        if value <= -180 { value += 360 }
        if value > 180 { value -= 360 }
        // −0.0 is not 0.0 to a JSON reader, and "rotation": -0 in a reply
        // would be the one number a caller could not compare with 0.
        return value == 0 ? 0 : value
    }

    /// The bounding box the rotated canvas needs — the host view's size.
    ///
    /// It is the IDENTITY at 0°, which is what keeps every existing zoom,
    /// scroll and Navigator number bit-identical: the host is then exactly the
    /// canvas's old frame and the clip view's bounds mean what they always did.
    static func hostSize(canvas: CGSize, degrees: Double) -> CGSize {
        guard degrees != 0, canvas.width > 0, canvas.height > 0 else { return canvas }
        let radians = degrees * .pi / 180
        let cosine = abs(cos(radians)), sine = abs(sin(radians))
        return CGSize(width: canvas.width * cosine + canvas.height * sine,
                      height: canvas.width * sine + canvas.height * cosine)
    }

    /// Where the unrotated canvas frame sits inside that host: centred, so the
    /// rotation about the canvas's own centre is also about the host's.
    static func canvasOrigin(canvas: CGSize, host: CGSize) -> CGPoint {
        CGPoint(x: (host.width - canvas.width) / 2, y: (host.height - canvas.height) / 2)
    }
}

/// The ⌥-drag rotate-view gesture, modelled as a session struct on the
/// `TransformSession` / `PatchSession` template: one stored property on the
/// canvas, all the arithmetic here.
///
/// The anchor is the WELL's centre rather than the press point, so the picture
/// turns about what the user is looking at.
struct RotateViewSession {
    /// The point the drag turns about, in WINDOW coordinates (y up).
    let centerInWindow: CGPoint
    /// The clockwise-positive screen angle the press was at.
    let anchorAngle: Double
    /// The view rotation the press started from.
    let startDegrees: Double

    /// Clockwise-positive screen angle of `point` about `center`. A window
    /// counts y UPWARD, so the y difference is negated to make a clockwise
    /// turn on screen a positive number.
    private static func screenAngle(of point: CGPoint, about center: CGPoint) -> Double {
        atan2(Double(center.y - point.y), Double(point.x - center.x)) * 180 / .pi
    }

    static func begin(canvas: ImageCanvasView, event: NSEvent, start: Double)
        -> RotateViewSession
    {
        let well: NSView = canvas.enclosingScrollView?.contentView ?? canvas
        let middle = CGPoint(x: well.bounds.midX, y: well.bounds.midY)
        let center = well.convert(middle, to: nil)
        return RotateViewSession(
            centerInWindow: center,
            anchorAngle: screenAngle(of: event.locationInWindow, about: center),
            startDegrees: start)
    }

    /// The angle this drag tick asks for. ⇧ snaps to the nearest multiple of
    /// 15°, the marquee's own constrain modifier.
    ///
    /// The DELTA is normalized before it is added: a drag that crosses the
    /// ±180° seam would otherwise read as a 358° jump backwards.
    func angle(for event: NSEvent) -> Double {
        let now = Self.screenAngle(of: event.locationInWindow, about: centerInWindow)
        var degrees = startDegrees + ViewRotation.normalize(now - anchorAngle)
        if event.modifierFlags.contains(.shift) {
            degrees = (degrees / 15).rounded() * 15
        }
        return ViewRotation.normalize(degrees)
    }

    /// The trackpad's two-finger rotate, which any tool answers.
    /// `NSEvent.rotation` is counterclockwise-positive, and this feature's
    /// degrees are clockwise-positive, so the delta is subtracted.
    static func applyingTrackpad(_ eventRotation: CGFloat, to degrees: Double) -> Double {
        ViewRotation.normalize(degrees - Double(eventRotation))
    }
}

/// The scroll view's `documentView` since 12D-2: an UNROTATED box the canvas
/// sits centred inside.
///
/// Putting the rotation on the canvas and the scrolling on the host is what
/// keeps everything else unchanged — the clip view still scrolls along the
/// screen's axes, the hand tool still pans the way the pointer moves, and the
/// canvas's own bounds are still document pixels.
final class CanvasHostView: NSView {
    /// The canvas it hosts. Weak: the editor owns both.
    weak var canvas: ImageCanvasView?

    override var isFlipped: Bool { true }

    @available(*, unavailable)
    required init?(coder: NSCoder) {
        fatalError("CanvasHostView does not support NSCoder")
    }

    override init(frame frameRect: NSRect) {
        super.init(frame: frameRect)
    }

    /// While the view is rotated the host's own corners are exposed beside the
    /// turned picture, and AppKit asks the view under the pointer for its
    /// cursor — so the tool's cursor is added here too. At 0° the host is
    /// exactly the canvas's box and this adds nothing.
    override func resetCursorRects() {
        guard let canvas, canvas.frameCenterRotation != 0 else { return }
        addCursorRect(bounds, cursor: canvas.tool.cursor)
    }
}
