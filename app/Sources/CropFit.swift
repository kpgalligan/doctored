import CoreGraphics

// `CropFit` is pure CoreGraphics (a scratch `swiftc` harness compiles it alone): `fitted` scales
// `CropSession.requested` about an anchor to the largest SAME-ASPECT box whose rotated corners stay
// inside the canvas inset by 2 px (and the box itself on the canvas), whole pixels, re-checked
// after rounding; `movedInsideRotation` slides a fitted box; `lineAngle` is the Straighten line's
// rule (fold to ±45°, steep lines level to vertical).

/// The Crop tool's straighten geometry: keeping the box
/// inside the rotated image, so a straightened crop never shows transparent
/// wedges in its corners. Pure functions over CoreGraphics values only, so
/// they can be compiled and exercised on their own.
///
/// The straighten rotates the picture by −angle about the box's centre
/// (`commitCropSession`, `AgentServer.crop`), so the box's four corners read
/// the source at `c + R·d` for the box's half-diagonals `d`. Whatever the
/// sign, those corners span `±ex` and `±ey` about the centre:
/// `ex = (w|cosθ| + h|sinθ|) / 2`, `ey = (w|sinθ| + h|cosθ|) / 2`. A box is
/// wedge-free when that span lies inside the canvas inset by `margin` (the
/// resampler's filter reach at the edge).
///
/// The fit is the largest box of the SAME ASPECT as the one asked for, not
/// the largest-area box: the user's ratio (or Free's current shape) is kept.
/// It fits against the CANVAS and ignores layer pixels lying outside it —
/// those exist (crop never deletes them), but a box that reached them would
/// still show wedges wherever no layer covers.
enum CropFit {
    /// Inset from every canvas edge, in pixels.
    static let margin: CGFloat = 2

    /// Slack for floating-point comparisons, in pixels.
    private static let tolerance: CGFloat = 1e-6

    /// The half-extents of `size` turned by `angle` degrees (sign-free).
    static func extents(of size: CGSize, angle: Double) -> (x: CGFloat, y: CGFloat) {
        let radians = angle * .pi / 180
        let cosine = CGFloat(abs(cos(radians)))
        let sine = CGFloat(abs(sin(radians)))
        return ((size.width * cosine + size.height * sine) / 2,
                (size.width * sine + size.height * cosine) / 2)
    }

    /// `rect` fitted inside the canvas turned by `angle` degrees: whole
    /// pixels, never smaller than 1×1, every rotated corner inside the
    /// canvas inset by `margin`. Angle 0 returns `rect` unchanged.
    ///
    /// The box scales about `anchor` (default its centre) by the largest
    /// factor up to 1 that fits. When scaling about the anchor cannot keep a
    /// whole pixel (the anchor sits in the margin or outside the rotated
    /// image), the box takes the largest same-aspect size that fits anywhere
    /// and its centre is clamped the way `movedInsideRotation` clamps it.
    /// The result rounds inward and is re-checked, pulling an edge in one
    /// pixel at a time until the rounded box passes.
    ///
    /// On a canvas too small for even a 1×1 box at that angle inside the
    /// margin (under about 6 px a side) the result is the centred 1×1 box,
    /// and its wedges are accepted.
    static func fitted(_ rect: CGRect, angle: Double, canvas: CGSize,
                       anchor: CGPoint? = nil) -> CGRect {
        guard angle != 0 else { return rect }
        let scaled = scaledFit(rect, angle: angle, canvas: canvas, anchor: anchor).rect
        return rounded(scaled, angle: angle, canvas: canvas)
    }

    /// `fitted` before rounding: the scaled box and its scale factor.
    /// Exposed for the geometry's own checks; callers want `fitted`.
    static func scaledFit(_ rect: CGRect, angle: Double, canvas: CGSize,
                          anchor: CGPoint? = nil) -> (rect: CGRect, scale: CGFloat) {
        let size = rect.size
        let center = CGPoint(x: rect.midX, y: rect.midY)
        let pivot = anchor ?? center
        let extent = extents(of: size, angle: angle)
        let m = margin
        var low: CGFloat = 0
        var high: CGFloat = 1
        // Each corner span is `pivot + s·((center − pivot) ± extent)`.
        func bound(_ pivot: CGFloat, _ slope: CGFloat, atLeast floor: CGFloat) {
            if slope > 0 {
                low = max(low, (floor - pivot) / slope)
            } else if slope < 0 {
                high = min(high, (floor - pivot) / slope)
            } else if pivot < floor - tolerance {
                high = -1
            }
        }
        func bound(_ pivot: CGFloat, _ slope: CGFloat, atMost ceiling: CGFloat) {
            if slope > 0 {
                high = min(high, (ceiling - pivot) / slope)
            } else if slope < 0 {
                low = max(low, (ceiling - pivot) / slope)
            } else if pivot > ceiling + tolerance {
                high = -1
            }
        }
        bound(pivot.x, center.x - pivot.x - extent.x, atLeast: m)
        bound(pivot.x, center.x - pivot.x + extent.x, atMost: canvas.width - m)
        bound(pivot.y, center.y - pivot.y - extent.y, atLeast: m)
        bound(pivot.y, center.y - pivot.y + extent.y, atMost: canvas.height - m)
        // The box itself stays on the canvas too (a thin box turns into a
        // span shorter than its own side).
        bound(pivot.x, rect.minX - pivot.x, atLeast: 0)
        bound(pivot.x, rect.maxX - pivot.x, atMost: canvas.width)
        bound(pivot.y, rect.minY - pivot.y, atLeast: 0)
        bound(pivot.y, rect.maxY - pivot.y, atMost: canvas.height)

        if low <= high, high * size.width >= 1, high * size.height >= 1 {
            let s = high
            let fitted = CGRect(
                x: pivot.x + s * (rect.minX - pivot.x), y: pivot.y + s * (rect.minY - pivot.y),
                width: s * size.width, height: s * size.height)
            return (fitted, s)
        }

        // The anchor cannot hold a whole pixel: the largest same-aspect box
        // that fits the inset canvas at all, its centre clamped.
        var s: CGFloat = 1
        if extent.x > 0 { s = min(s, (canvas.width - 2 * m) / (2 * extent.x)) }
        if extent.y > 0 { s = min(s, (canvas.height - 2 * m) / (2 * extent.y)) }
        s = max(s, 0)
        let scaledCenter = CGPoint(x: pivot.x + s * (center.x - pivot.x),
                                   y: pivot.y + s * (center.y - pivot.y))
        let width = s * size.width
        let height = s * size.height
        let cx = clampCenter(scaledCenter.x, extent: s * extent.x, size: width,
                             length: canvas.width)
        let cy = clampCenter(scaledCenter.y, extent: s * extent.y, size: height,
                             length: canvas.height)
        return (CGRect(x: cx - width / 2, y: cy - height / 2, width: width, height: height), s)
    }

    /// `rect` (a box that already fits, at its new position) moved the least
    /// so its rotated corners stay inside the inset canvas, keeping its size
    /// and landing on a whole-pixel origin. An axis that cannot fit centres
    /// on the canvas; when no whole-pixel origin fits, the box is `fitted`
    /// instead. Angle 0 returns `rect` unchanged.
    static func movedInsideRotation(_ rect: CGRect, angle: Double, canvas: CGSize) -> CGRect {
        guard angle != 0 else { return rect }
        let extent = extents(of: rect.size, angle: angle)
        let cx = clampCenter(rect.midX, extent: extent.x, size: rect.width,
                             length: canvas.width)
        let cy = clampCenter(rect.midY, extent: extent.y, size: rect.height,
                             length: canvas.height)
        let centered = CGRect(x: cx - rect.width / 2, y: cy - rect.height / 2,
                              width: rect.width, height: rect.height)
        guard
            let x = wholeOrigin(centered.minX, size: rect.width, extent: extent.x,
                                length: canvas.width),
            let y = wholeOrigin(centered.minY, size: rect.height, extent: extent.y,
                                length: canvas.height)
        else { return fitted(centered, angle: angle, canvas: canvas) }
        return CGRect(x: x, y: y, width: rect.width, height: rect.height)
    }

    /// The point a handle drag keeps fixed: the opposite corner for the
    /// corner handles 0–3 (0↔2, 1↔3), the opposite edge's midpoint for the
    /// edge handles 4–7 — `CropSession.handles`' order.
    static func resizeAnchor(of rect: CGRect, handle index: Int) -> CGPoint {
        switch index {
        case 0: return CGPoint(x: rect.maxX, y: rect.maxY)
        case 1: return CGPoint(x: rect.minX, y: rect.maxY)
        case 2: return CGPoint(x: rect.minX, y: rect.minY)
        case 3: return CGPoint(x: rect.maxX, y: rect.minY)
        case 4: return CGPoint(x: rect.midX, y: rect.maxY)
        case 5: return CGPoint(x: rect.minX, y: rect.midY)
        case 6: return CGPoint(x: rect.midX, y: rect.minY)
        default: return CGPoint(x: rect.maxX, y: rect.midY)
        }
    }

    /// The straighten that levels a line drawn from `start` to `end`, in
    /// degrees within ±45; nil when the points coincide.
    ///
    /// The canvas is y-down, so `φ = atan2(dy, dx)` is positive for a line
    /// descending to the right — a line turned CLOCKWISE on screen. A
    /// positive crop angle turns the picture counter-clockwise on screen,
    /// so that line needs +φ. φ folds into (−90, 90] (a line has no
    /// direction), and a line steeper than 45° levels to VERTICAL instead:
    /// φ − 90 above 45°, φ + 90 below −45°.
    static func lineAngle(from start: CGPoint, to end: CGPoint) -> Double? {
        let dx = Double(end.x - start.x)
        let dy = Double(end.y - start.y)
        guard dx != 0 || dy != 0 else { return nil }
        var phi = atan2(dy, dx) * 180 / .pi
        if phi > 90 { phi -= 180 }
        if phi <= -90 { phi += 180 }
        if phi > 45 { phi -= 90 } else if phi < -45 { phi += 90 }
        return phi
    }

    // MARK: - Helpers

    /// A centre clamped so `centre ± extent` stays inside `[m, length − m]`
    /// and the box's own `size` inside `[0, length]`; the middle when the
    /// span cannot fit.
    private static func clampCenter(_ center: CGFloat, extent: CGFloat, size: CGFloat,
                                    length: CGFloat) -> CGFloat {
        let low = max(margin + extent, size / 2)
        let high = min(length - margin - extent, length - size / 2)
        guard low <= high else { return length / 2 }
        return min(max(center, low), high)
    }

    /// The whole-pixel origin nearest `origin` that keeps `size` turned to
    /// `extent` inside the inset axis (and the box on the canvas); nil when
    /// no whole pixel does.
    private static func wholeOrigin(_ origin: CGFloat, size: CGFloat, extent: CGFloat,
                                    length: CGFloat) -> CGFloat? {
        let low = max(margin + extent - size / 2, 0) - tolerance
        let high = min(length - margin - extent - size / 2, length - size) + tolerance
        var whole = origin.rounded()
        if whole > high { whole = high.rounded(.down) }
        if whole < low { whole = low.rounded(.up) }
        guard whole >= low, whole <= high else { return nil }
        return whole
    }

    /// Inward rounding plus the re-check: the rounded box turns about its
    /// own (moved) centre, so an edge that now pokes past the inset canvas
    /// (or off the canvas) is pulled in a pixel at a time. Never below 1×1.
    private static func rounded(_ rect: CGRect, angle: Double, canvas: CGSize) -> CGRect {
        var minX = (rect.minX - tolerance).rounded(.up)
        var maxX = (rect.maxX + tolerance).rounded(.down)
        var minY = (rect.minY - tolerance).rounded(.up)
        var maxY = (rect.maxY + tolerance).rounded(.down)
        if maxX - minX < 1 {
            minX = (rect.midX - 0.5).rounded()
            maxX = minX + 1
        }
        if maxY - minY < 1 {
            minY = (rect.midY - 0.5).rounded()
            maxY = minY + 1
        }
        let m = margin
        while true {
            let width = maxX - minX
            let height = maxY - minY
            let extent = extents(of: CGSize(width: width, height: height), angle: angle)
            let cx = (minX + maxX) / 2
            let cy = (minY + maxY) / 2
            let left = min(cx - extent.x - m, minX)
            let right = min(canvas.width - m - (cx + extent.x), canvas.width - maxX)
            let top = min(cy - extent.y - m, minY)
            let bottom = min(canvas.height - m - (cy + extent.y), canvas.height - maxY)
            let xFails = min(left, right) < -tolerance
            let yFails = min(top, bottom) < -tolerance
            guard xFails || yFails else {
                return CGRect(x: minX, y: minY, width: width, height: height)
            }
            // Pull the failing axis's tighter edge; a 1-pixel axis cannot
            // shrink, so the other axis gives way (its extent feeds this one).
            let pullX = xFails ? width > 1 : (height <= 1 && width > 1)
            let pullY = !pullX && height > 1
            if pullX {
                if left < right { minX += 1 } else { maxX -= 1 }
            } else if pullY {
                if top < bottom { minY += 1 } else { maxY -= 1 }
            } else {
                // A 1×1 box that still fails: the canvas is too small.
                return CGRect(x: ((canvas.width - 1) / 2).rounded(.down),
                              y: ((canvas.height - 1) / 2).rounded(.down),
                              width: 1, height: 1)
            }
        }
    }
}
