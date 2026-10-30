import AppKit

// MARK: - Model

/// One control point of a curves channel, in the historical meta schema's
/// [0, 255] × [0, 255] space (an `[in, out]` pair; core/src/adjust.rs). The
/// editor keeps every channel's list sorted by `x` with distinct `x` values
/// and 2...16 points — exactly what the core's parser accepts.
struct CurvePoint: Equatable {
    var x: Double
    var y: Double

    /// The identity point list — what a channel the meta omits means.
    static let identity = [CurvePoint(x: 0, y: 0), CurvePoint(x: 255, y: 255)]
}

/// The point lists of a curves op, keyed by the meta's params keys.
/// Application order in the core: the per-channel table first, then the
/// master (`rgb`, or `cmyk` in a CMYK document, #547) table. A CMYK curve's
/// points are INK, 0…255 for 0…100 % (`core/src/cmyk_adjust.rs`).
enum CurveChannel: String, CaseIterable {
    case rgb = "rgb"
    case red = "r"
    case green = "g"
    case blue = "b"
    case cmyk = "cmyk"
    case cyan = "c"
    case magenta = "m"
    case yellow = "y"
    case black = "k"

    /// The channels a dialog lists: RGB, Red, Green, Blue, or CMYK, Cyan,
    /// Magenta, Yellow, Black in a CMYK document, or Lightness (the
    /// composite), a and b (the green and blue curves) in a Lab Color one
    /// (#547 Lab phase 2c).
    static func cases(cmyk: Bool, lab: Bool = false) -> [CurveChannel] {
        if lab { return [.rgb, .green, .blue] }
        return cmyk ? [.cmyk, .cyan, .magenta, .yellow, .black] : [.rgb, .red, .green, .blue]
    }

    /// The dialog's name for this curve: Lightness, a and b in a Lab Color
    /// document, else `displayName`.
    func title(lab: Bool) -> String {
        guard lab else { return displayName }
        switch self {
        case .rgb, .red: return "Lightness"
        case .green: return "a"
        case .blue: return "b"
        default: return displayName
        }
    }

    var displayName: String {
        switch self {
        case .rgb: return "RGB"
        case .red: return "Red"
        case .green: return "Green"
        case .blue: return "Blue"
        case .cmyk: return "CMYK"
        case .cyan: return "Cyan"
        case .magenta: return "Magenta"
        case .yellow: return "Yellow"
        case .black: return "Black"
        }
    }

    /// The map-form params key for this channel: a 256-entry
    /// map drawn with the pencil (a CMYK channel's on ink).
    var mapKey: String { rawValue + "_map" }

    /// The plot's stroke for this channel's curve.
    var strokeColor: NSColor {
        switch self {
        case .rgb, .cmyk, .black: return DS.textStrong
        case .red: return .systemRed
        case .green: return .systemGreen
        case .blue: return .systemBlue
        case .cyan: return .systemCyan
        case .magenta: return .systemPink
        case .yellow: return .systemYellow
        }
    }
}

/// Round-trips between a curves payload's `params` and per-channel point
/// lists. WRITING preserves fractional coordinates in [0, 255], sorted by `x`,
/// distinct `x`s, identity channels omitted (the schema's "missing channel
/// = identity") — so the app never writes malformed meta. READING is
/// defensive: anything not shaped like a point list falls back to identity
/// (range truth stays in the core; the dialog only needs something sane to
/// prefill).
enum CurvesMeta {
    /// The point-form params keys, `rgb` first, then the CMYK document's (#547).
    static let pointKeys: [String] = CurveChannel.allCases.map(\.rawValue)
    /// The map-form params keys, RGB then a CMYK document's
    /// (`cmyk_map`, `c_map`… on ink).
    static let mapKeys: [String] = CurveChannel.allCases.map(\.mapKey)
    /// One colour mode's map keys, the composite first.
    static func mapKeys(cmyk: Bool) -> [String] {
        CurveChannel.cases(cmyk: cmyk).map(\.mapKey)
    }
    /// Every key a curves params object may hold — one form or the other,
    /// never both (the core refuses a mix).
    static let keys: [String] = pointKeys + mapKeys
    /// The identity map: input code i outputs i.
    static let identityMap: [Int] = Array(0...255)

    /// The map form's channels in `cmyk`'s colour mode, or nil when the
    /// payload holds no map key of that mode (a point curve). A channel whose
    /// map is absent or not 256 integers 0…255 reads as the identity map —
    /// range truth stays in the core.
    static func maps(in payload: AdjustmentLayerPayload, cmyk: Bool = false) -> [CurveChannel: [Int]]? {
        guard mapKeys(cmyk: cmyk).contains(where: { payload.params[$0] != nil }) else { return nil }
        var maps: [CurveChannel: [Int]] = [:]
        for channel in CurveChannel.cases(cmyk: cmyk) {
            maps[channel] = map(payload.params[channel.mapKey]) ?? identityMap
        }
        return maps
    }

    private static func map(_ raw: Any?) -> [Int]? {
        guard let list = raw as? [NSNumber], list.count == 256 else { return nil }
        var out: [Int] = []
        out.reserveCapacity(256)
        for number in list {
            let value = number.doubleValue
            guard value.isFinite, value == value.rounded(), value >= 0, value <= 255 else {
                return nil
            }
            out.append(Int(value))
        }
        return out
    }

    /// The map-form params: the composite's map (`rgb_map`, or `cmyk_map` in
    /// a CMYK document) always, so the meta stays map form, each other
    /// channel's only where it differs from identity.
    static func params(maps: [CurveChannel: [Int]], cmyk: Bool = false) -> [String: Any] {
        var params: [String: Any] = [:]
        let channels = CurveChannel.cases(cmyk: cmyk)
        for channel in channels {
            let map = maps[channel] ?? identityMap
            guard channel == channels[0] || map != identityMap else { continue }
            params[channel.mapKey] = map
        }
        return params
    }

    /// Edit Points: a map as control points. An identity map is the
    /// identity point list (so `params` omits the channel); any other map
    /// is sampled at every 17th input, `(17k, map[17k])` for k = 0…15 — a
    /// sample selection, not pixel math, so a sharp notch between samples
    /// is smoothed away, which is what converting a pencil curve does.
    static func points(fromMap map: [Int]) -> [CurvePoint] {
        guard map.count == 256, map != identityMap else { return CurvePoint.identity }
        return (0..<16).map { k in
            CurvePoint(x: Double(17 * k), y: Double(map[17 * k]))
        }
    }

    /// The meta `params` for the given channels: only channels whose points
    /// differ from identity are included; all-identity curves produce an
    /// empty params object (a valid identity adjustment).
    static func params(_ channelPoints: [CurveChannel: [CurvePoint]]) -> [String: Any] {
        var params: [String: Any] = [:]
        for channel in CurveChannel.allCases {
            let points = channelPoints[channel] ?? CurvePoint.identity
            guard points != CurvePoint.identity else { continue }
            params[channel.rawValue] = points.map { [$0.x, $0.y] }
        }
        return params
    }

    /// `channel`'s points out of a curves payload, for prefilling the
    /// editor: preserving fractional values, clamped to [0, 255], stably sorted by
    /// `x` with later duplicate `x`s dropped (the core's own rule).
    /// Identity when the channel is missing or structurally not a point
    /// list.
    static func points(of channel: CurveChannel, in payload: AdjustmentLayerPayload) -> [CurvePoint] {
        guard let raw = payload.params[channel.rawValue] else { return CurvePoint.identity }
        guard let list = raw as? [[NSNumber]], (2...16).contains(list.count) else {
            return CurvePoint.identity
        }
        var points: [CurvePoint] = []
        for pair in list {
            guard pair.count == 2 else { return CurvePoint.identity }
            let x = pair[0].doubleValue
            let y = pair[1].doubleValue
            guard x.isFinite, y.isFinite else { return CurvePoint.identity }
            points.append(
                CurvePoint(
                    x: min(max(x, 0), 255),
                    y: min(max(y, 0), 255)))
        }
        // Stable sort by x, first listed winning a duplicate — mirroring the
        // core's parse_points (integer rounding can create duplicates the
        // core's float compare kept apart; first-wins stays consistent).
        points = points.enumerated()
            .sorted { ($0.element.x, $0.offset) < ($1.element.x, $1.offset) }
            .map { $0.element }
        var deduped: [CurvePoint] = []
        for point in points where deduped.last?.x != point.x {
            deduped.append(point)
        }
        return deduped.count >= 2 ? deduped : CurvePoint.identity
    }
}

// MARK: - Natural cubic spline (display twin of the core's LUT)

/// The natural cubic spline through the control points (second
/// derivative 0 at both ends) — a Swift port of the core's `Spline`
/// (core/src/adjust_curves.rs) so the plotted curve visually matches the LUT
/// the compositor applies (the core additionally rounds each of its 256
/// entries to u8; that sub-pixel drift is invisible at plot scale).
/// Construction: the interior second derivatives from the tridiagonal
/// system, solved by the Thomas algorithm. Unlike the monotone cubic it
/// replaced, it may overshoot between points; the value is
/// clamped to [0, 255] as the core clamps it.
struct NaturalCurve {
    private let xs: [Double]
    private let ys: [Double]
    private let secondDerivatives: [Double]

    /// `points` sorted by `x` with distinct `x`s (the editor's invariant);
    /// fewer than 2 points falls back to identity.
    init(points: [CurvePoint]) {
        let points = points.count >= 2 ? points : CurvePoint.identity
        let xs = points.map { Double($0.x) }
        let ys = points.map { Double($0.y) }
        let n = points.count
        var h = [Double](repeating: 0, count: n - 1)
        var d = [Double](repeating: 0, count: n - 1)
        for k in 0..<(n - 1) {
            h[k] = xs[k + 1] - xs[k]
            d[k] = (ys[k + 1] - ys[k]) / h[k]
        }
        var m = [Double](repeating: 0, count: n)
        if n > 2 {
            var c = [Double](repeating: 0, count: n)
            var r = [Double](repeating: 0, count: n)
            for i in 1..<(n - 1) {
                let diagonal = 2 * (h[i - 1] + h[i]) - h[i - 1] * c[i - 1]
                c[i] = h[i] / diagonal
                r[i] = (6 * (d[i] - d[i - 1]) - h[i - 1] * r[i - 1]) / diagonal
            }
            for i in stride(from: n - 2, through: 1, by: -1) {
                m[i] = r[i] - c[i] * m[i + 1]
            }
        }
        self.xs = xs
        self.ys = ys
        self.secondDerivatives = m
    }

    /// The curve's value at `x`, exactly as the core's LUT samples it:
    /// outside the point range the nearest endpoint's value, inside it the
    /// spline segment, clamped to [0, 255].
    func value(at x: Double) -> Double {
        if x <= xs[0] { return ys[0] }
        let last = xs.count - 1
        if x >= xs[last] { return ys[last] }
        var k = last - 1
        for i in 0..<last where x < xs[i + 1] {
            k = i
            break
        }
        let h = xs[k + 1] - xs[k]
        let a = (xs[k + 1] - x) / h
        let b = (x - xs[k]) / h
        let m = secondDerivatives
        let y = a * ys[k] + b * ys[k + 1]
            + ((a * a * a - a) * m[k] + (b * b * b - b) * m[k + 1]) * h * h / 6
        return min(max(y, 0), 255)
    }
}

// MARK: - CurveEditorView

/// The interactive curves plot: a ~256pt square with quarter gridlines, the
/// faint identity diagonal, and ONE channel's curve (the controller swaps
/// `points`/`curveColor` when the channel popup changes). Control points
/// snap to integers. Interactions: click on/near the curve adds a point
/// (max 16); dragging moves a point with `x` clamped strictly between its
/// neighbors and `y` to [0, 255]; dragging a point well outside the plot
/// removes it (endpoints never; minimum 2 points remain). The dragged
/// point highlights, and `onDragReadout` carries a live "in → out" string
/// (nil when the drag ends).
final class CurveEditorView: NSView {
    static let maxPoints = 16
    /// Side of the value plot itself; the view adds `inset` around it so
    /// edge points and their hit slop stay inside.
    static let plotSide: CGFloat = 256
    private static let inset: CGFloat = 8
    /// How far outside the plot a dragged point must go to be removed.
    private static let removeMargin: CGFloat = 32
    private static let pointHitRadius: CGFloat = 8
    private static let curveHitDistance: CGFloat = 6

    /// The current channel's points — sorted by `x`, distinct `x`s,
    /// 2...16 of them. The controller sets this on channel switches; the
    /// view mutates it (and fires `onPointsChanged`) on user edits.
    var points: [CurvePoint] = CurvePoint.identity {
        didSet { needsDisplay = true; numericControls?.refresh() }
    }
    var curveColor: NSColor = DS.textStrong {
        didSet { needsDisplay = true }
    }
    /// A pencil-drawn (map-form) channel, shown as the polyline through its
    /// 256 entries instead of `points`' curve and control points. While it
    /// is set the mouse DRAWS into it (`CurvePencil.drawn`, each drag sample
    /// joined to the last by a straight run) and fires `onMapChanged`;
    /// Points in the dialog clears it.
    var map: [Int]? {
        didSet { needsDisplay = true }
    }
    var onMapChanged: (([Int]) -> Void)?
    /// The last pencil sample of the current stroke, in value space.
    private var pencilLast: (x: Int, y: Int)?
    /// The tones the curve moves, drawn behind it: the layer being filtered
    /// for a destructive dialog, the BACKDROP BELOW the layer for an
    /// adjustment layer's (`Histogram.forSheet`). The plot is 256 pt at the
    /// default size, so one bar is one bin and nothing is resampled.
    var histogram: HistogramBins? {
        didSet { needsDisplay = true }
    }
    /// Why `histogram` could not be read: the plot says
    /// `HistogramView.unavailableTitle` and this is the tooltip.
    var histogramUnavailableReason: String? {
        didSet {
            toolTip = histogramUnavailableReason
            needsDisplay = true
        }
    }
    var onPointsChanged: (([CurvePoint]) -> Void)?
    /// #547: a CMYK curve — axes in ink, 0 % at the left and bottom, with
    /// white→black gradient bars along them, and the readout in percent.
    var inkAxes = false {
        didSet { needsDisplay = true }
    }
    /// "in → out" while a point drags; nil when the drag ends.
    var onDragReadout: ((String?) -> Void)?
    /// ⌥ on an end point's press/drag sends its clipping display
    /// (index 0 shadows, the last highlights), anything else nil; fired
    /// before `onPointsChanged`. Release sends nil once a mode was sent.
    var onClipDrag: ((ClipDisplayMode?) -> Void)?
    private var sentClip = false

    private var numericControls: FineCurveControls?
    var selectedPointIndex = 0

    func fineControls(native: Bool) -> NSView {
        if let numericControls { return numericControls }
        let controls = FineCurveControls(curve: self, native: native)
        numericControls = controls
        return controls
    }

    func acceptFineEntry() -> Bool { numericControls?.accept() ?? true }

    private var dragIndex: Int? {
        didSet { needsDisplay = true }
    }

    override var intrinsicContentSize: NSSize {
        let side = Self.plotSide + Self.inset * 2
        return NSSize(width: side, height: side)
    }

    /// Value (0,0)..(255,255) maps across this rect, y upward (the view is
    /// not flipped, so plot space and view space agree).
    private var plotRect: NSRect {
        let side = min(bounds.width, bounds.height) - Self.inset * 2
        return NSRect(x: Self.inset, y: Self.inset, width: max(side, 0), height: max(side, 0))
    }

    override func resetCursorRects() {
        addCursorRect(plotRect, cursor: .crosshair)
    }

    // MARK: Coordinate mapping

    private func viewPoint(for point: CurvePoint) -> NSPoint {
        let rect = plotRect
        return NSPoint(
            x: rect.minX + CGFloat(point.x) / 255 * rect.width,
            y: rect.minY + CGFloat(point.y) / 255 * rect.height)
    }

    /// Unclamped value-space position of a view-space location.
    private func valuePosition(at location: NSPoint) -> (x: Double, y: Double)? {
        let rect = plotRect
        guard rect.width > 0, rect.height > 0 else { return nil }
        return (
            x: Double((location.x - rect.minX) / rect.width) * 255,
            y: Double((location.y - rect.minY) / rect.height) * 255
        )
    }

    // MARK: Drawing

    override func draw(_ dirtyRect: NSRect) {
        let rect = plotRect
        guard rect.width > 0 else { return }

        DS.canvasVoid.setFill()
        NSBezierPath(rect: rect).fill()

        // The tones under the curve, clipped to the plot so a full bar
        // cannot paint over the frame.
        if let histogram = histogram {
            NSGraphicsContext.current?.saveGraphicsState()
            NSBezierPath(rect: rect).addClip()
            drawHistogram(histogram, display: .luminosity, in: rect)
            NSGraphicsContext.current?.restoreGraphicsState()
        } else if histogramUnavailableReason != nil {
            drawHistogramUnavailable(in: rect)
        }

        // Light quarter gridlines.
        let grid = NSBezierPath()
        grid.lineWidth = 1
        for quarter in 1...3 {
            let f = CGFloat(quarter) / 4
            grid.move(to: NSPoint(x: rect.minX + f * rect.width, y: rect.minY))
            grid.line(to: NSPoint(x: rect.minX + f * rect.width, y: rect.maxY))
            grid.move(to: NSPoint(x: rect.minX, y: rect.minY + f * rect.height))
            grid.line(to: NSPoint(x: rect.maxX, y: rect.minY + f * rect.height))
        }
        DS.border.setStroke()
        grid.stroke()

        // Faint identity diagonal, the curve's reference.
        let diagonal = NSBezierPath()
        diagonal.lineWidth = 1
        diagonal.move(to: NSPoint(x: rect.minX, y: rect.minY))
        diagonal.line(to: NSPoint(x: rect.maxX, y: rect.maxY))
        DS.textFaint.withAlphaComponent(0.5).setStroke()
        diagonal.stroke()

        // Plot frame.
        let frame = NSBezierPath(rect: rect)
        frame.lineWidth = 1
        DS.borderStrong.setStroke()
        frame.stroke()

        // #547: ink axes — white (no ink) to black (full ink) along the
        // bottom and the left, in the inset beside the plot.
        if inkAxes, let gradient = NSGradient(starting: .white, ending: .black) {
            let bar = Self.inset - 3
            gradient.draw(in: NSRect(x: rect.minX, y: rect.minY - bar - 1, width: rect.width, height: bar),
                          angle: 0)
            gradient.draw(in: NSRect(x: rect.minX - bar - 1, y: rect.minY, width: bar, height: rect.height),
                          angle: 90)
        }

        if let map, map.count == 256 {
            let path = NSBezierPath()
            path.lineWidth = 1.5
            for (i, value) in map.enumerated() {
                let point = NSPoint(
                    x: rect.minX + CGFloat(i) / 255 * rect.width,
                    y: rect.minY + CGFloat(value) / 255 * rect.height)
                if i == 0 {
                    path.move(to: point)
                } else {
                    path.line(to: point)
                }
            }
            curveColor.setStroke()
            path.stroke()
            return
        }

        // The channel's curve, sampled at every integer input like the
        // core's 256-entry LUT.
        let curve = NaturalCurve(points: points)
        let path = NSBezierPath()
        path.lineWidth = 1.5
        for i in 0...255 {
            let x = Double(i)
            let point = NSPoint(
                x: rect.minX + CGFloat(x) / 255 * rect.width,
                y: rect.minY + CGFloat(curve.value(at: x)) / 255 * rect.height)
            if i == 0 {
                path.move(to: point)
            } else {
                path.line(to: point)
            }
        }
        curveColor.setStroke()
        path.stroke()

        // Control points; the dragged one fills solid and slightly larger.
        for (index, point) in points.enumerated() {
            let center = viewPoint(for: point)
            let radius: CGFloat = index == dragIndex ? 4.5 : 3.5
            let dot = NSBezierPath(
                ovalIn: NSRect(
                    x: center.x - radius, y: center.y - radius,
                    width: radius * 2, height: radius * 2))
            if index == dragIndex {
                curveColor.setFill()
                dot.fill()
            } else {
                DS.chromeBackground.setFill()
                dot.fill()
                dot.lineWidth = 1.5
                curveColor.setStroke()
                dot.stroke()
            }
        }
    }

    // MARK: Interaction

    private func pointIndex(at location: NSPoint) -> Int? {
        var best: (index: Int, distance: CGFloat)?
        for (index, point) in points.enumerated() {
            let center = viewPoint(for: point)
            let distance = hypot(location.x - center.x, location.y - center.y)
            if distance <= Self.pointHitRadius, distance < (best?.distance ?? .infinity) {
                best = (index, distance)
            }
        }
        return best?.index
    }

    private func emitReadout() {
        guard let index = dragIndex, points.indices.contains(index) else { return }
        if inkAxes {
            let percent = { (v: Double) in String(format: "%.0f%%", v / 255 * 100) }
            onDragReadout?("\(percent(points[index].x)) → \(percent(points[index].y))")
            return
        }
        onDragReadout?("\(points[index].x) → \(points[index].y)")
    }

    private func reportClip(_ event: NSEvent, index: Int) {
        guard let onClipDrag else { return }
        let mode = index == 0 ? ClippingDrag.active(.shadows, event: event)
            : index == points.count - 1 ? ClippingDrag.active(.highlights, event: event) : nil
        if mode != nil { sentClip = true }
        onClipDrag(mode)
    }

    /// A view location as the pencil's integer value-space sample, clamped
    /// to the plot.
    private func pencilSample(_ event: NSEvent) -> (x: Int, y: Int)? {
        guard let value = valuePosition(at: convert(event.locationInWindow, from: nil)) else {
            return nil
        }
        let clamp = { (v: Double) in Int(min(max(v.rounded(), 0), 255)) }
        return (clamp(value.x), clamp(value.y))
    }

    /// One pencil sample into `map`, joined to the stroke's last (none on the press).
    private func pencil(_ event: NSEvent, press: Bool) {
        guard let current = map, current.count == 256, let sample = pencilSample(event) else { return }
        let drawn = CurvePencil.drawn(current, from: press ? nil : pencilLast, to: sample)
        pencilLast = sample
        let text = { (v: Int) in self.inkAxes ? String(format: "%.0f%%", Double(v) / 255 * 100) : "\(v)" }
        onDragReadout?("\(text(sample.x)) → \(text(sample.y))")
        guard drawn != current else { return }
        map = drawn
        onMapChanged?(drawn)
    }

    override func mouseDown(with event: NSEvent) {
        if map != nil { pencil(event, press: true); return }
        let location = convert(event.locationInWindow, from: nil)
        if let index = pointIndex(at: location) {
            dragIndex = index
            selectedPointIndex = index; numericControls?.refresh()
            emitReadout()
            reportClip(event, index: index)
            return
        }
        // Click on/near the curve adds a point ON the curve at that input.
        guard let value = valuePosition(at: location), points.count < Self.maxPoints else { return }
        let curve = NaturalCurve(points: points)
        let rect = plotRect
        let curveY = rect.minY + CGFloat(curve.value(at: value.x)) / 255 * rect.height
        guard abs(location.y - curveY) <= Self.curveHitDistance,
              value.x > -1, value.x < 256
        else { return }
        let x = min(max(value.x, 0), 255).rounded()
        // Distinct-x invariant: a click that rounds onto an existing
        // point's input adds nothing (that point was outside hit range).
        guard !points.contains(where: { $0.x == x }) else { return }
        let y = min(max(curve.value(at: x), 0), 255).rounded()
        let insertAt = points.firstIndex { $0.x > x } ?? points.count
        points.insert(CurvePoint(x: x, y: y), at: insertAt)
        dragIndex = insertAt
        selectedPointIndex = insertAt; numericControls?.refresh()
        emitReadout()
        reportClip(event, index: insertAt)
        onPointsChanged?(points)
    }

    override func mouseDragged(with event: NSEvent) {
        if map != nil { pencil(event, press: false); return }
        guard let index = dragIndex, points.indices.contains(index) else { return }
        reportClip(event, index: index)
        let location = convert(event.locationInWindow, from: nil)
        let rect = plotRect

        // Well outside the plot removes the point — but never an endpoint,
        // and never below 2 points.
        let farOutside = location.x < rect.minX - Self.removeMargin
            || location.x > rect.maxX + Self.removeMargin
            || location.y < rect.minY - Self.removeMargin
            || location.y > rect.maxY + Self.removeMargin
        if farOutside, index != 0, index != points.count - 1, points.count > 2 {
            points.remove(at: index)
            dragIndex = nil
            onDragReadout?(nil)
            onPointsChanged?(points)
            return
        }

        guard let value = valuePosition(at: location) else { return }
        // x strictly between neighbors (integer space: at least 1 apart);
        // endpoints bound by the plot edge instead of a missing neighbor.
        let lower = index == 0 ? 0 : points[index - 1].x.nextUp
        let upper = index == points.count - 1 ? 255 : points[index + 1].x.nextDown
        let x = min(max(value.x.rounded(), lower), upper)
        let y = min(max(value.y.rounded(), 0), 255)
        guard points[index].x != x || points[index].y != y else { return }
        points[index] = CurvePoint(x: x, y: y)
        emitReadout()
        onPointsChanged?(points)
    }

    override func mouseUp(with event: NSEvent) {
        if map != nil {
            pencilLast = nil
            onDragReadout?(nil)
            return
        }
        if sentClip { sentClip = false; onClipDrag?(nil) }
        guard dragIndex != nil else { return }
        dragIndex = nil
        onDragReadout?(nil)
    }
}
