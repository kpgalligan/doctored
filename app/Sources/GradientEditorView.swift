import AppKit

/// The reusable multi-stop gradient editor: a ramp strip over a stop bar —
/// click a stop to select it, drag to move it, click the ramp to add one,
/// drag one far outside (or press ⌫) to remove it, never below 2 stops and
/// never above 32 (the core's parser range). Structurally `CurveEditorView`:
/// one value type in, `on…` closures out, every gesture handled here, and no
/// document, sheet or renderer in sight.
///
/// **The colour rule is the caller's, and it matters.** The view draws and
/// edits stop colours in whatever `colorSpace` it is handed: the DOCUMENT's
/// space for a Gradient Map, whose stop colours are the document's own
/// numbers, and sRGB for a future layer-style caller, whose stop colours are
/// AUTHORED and convert in the core at composite time. The identical
/// `GradientFill` object means different things in those two places, which
/// is why the space is a parameter rather than an assumption.
///
/// **No separate opacity stops** — one stop carries position, colour and
/// opacity, and the colour and opacity midpoints
/// (`GradientStop.midpoint` / `opacityMidpoint`). With `showsMidpoints` the
/// view draws midpoint DIAMONDS, one per segment of non-zero
/// width at its midpoint: the colour diamond in the stop bar, the opacity
/// diamond in a row above the ramp. Clicking one selects it (and the stop
/// that owns it: segment `i`'s midpoint is stored on stop `i`); dragging it
/// moves the midpoint within 5…95 % of its segment, rounded to 1 %, stored
/// nil at 50 %. A stop it moves or recolours keeps its own midpoints, and
/// the last stop's, which governs nothing, stays as given. The Gradient Map
/// sets `showsMidpoints` too: it writes the Classic or the
/// Perceptual method, both of which the core reads them under.
///
/// **What the two strips show.** The ramp is the picture the core will
/// paint: `sample(at:)` mirrors `style_gradient::gradient_color` — linear
/// interpolation in the stored numbers between the two bracketing stops,
/// clamped to the ends outside them, each segment fraction remapped through
/// its stop's midpoint by the core's own `dr_gradient_midpoint_fraction`
/// (`gradient_field::midpoint_fraction`, the master; no Swift copy). It does
/// not draw Smoothness or the Classic Hermite: the strip stays linear in the
/// stored numbers, for a fill layer's Smoothness and for the Gradient tool's
/// own (the tool's Edit… sheet) alike. It is drawn over the transparency checkerboard, so a stop's
/// opacity reads as what it means in both callers: how much of what lies
/// underneath survives (the pixel's own colour under a
/// Gradient Map, the layers below under a style). A marker instead draws its
/// stop's colour fully opaque, so a transparent stop stays findable.
///
/// **The callbacks report edits made HERE.** Assigning `fill` or
/// `selectedStop` from outside repaints and fires nothing — the caller
/// already knows what it just wrote — exactly as `CurveEditorView.points`
/// behaves.
final class GradientEditorView: NSView {
    /// The lowest and highest stop counts the core's parser accepts.
    static let minStops = 2
    static let maxStops = 32

    /// Side margin, so a stop at 0 or 1 keeps its whole marker inside the
    /// view and stays grabbable.
    private static let sideInset: CGFloat = 8
    private static let barHeight: CGFloat = 20
    private static let gap: CGFloat = 6
    private static let markerWidth: CGFloat = 11
    private static let hitRadius: CGFloat = 9
    /// How far outside the view a dragged stop must go to be removed —
    /// `CurveEditorView.removeMargin`, so the two editors let go at the same
    /// distance. Vertical only: sideways IS the position, and dragging past
    /// an end clamps to it rather than throwing the stop away.
    private static let removeMargin: CGFloat = 32
    private static let checkerSide: CGFloat = 6
    /// The opacity diamonds' row above the ramp, and a diamond's side.
    private static let midpointRowHeight: CGFloat = 10
    private static let diamondSide: CGFloat = 7
    /// How far from a diamond's centre a click still grabs it when no stop
    /// marker claimed the click first.
    private static let diamondReach: CGFloat = 6

    /// A diamond: the colour (`opacity == false`) or opacity midpoint of the
    /// segment from stop `segment` to `segment + 1`, stored on stop `segment`.
    struct Midpoint: Equatable {
        var segment: Int
        var opacity: Bool
    }

    /// Sorted and clamped on every write (`normalized`), because the core
    /// sorts a parsed gradient's stops by position and the strip must be the
    /// picture the core will paint, not the order a caller happened to send.
    private var storedFill = GradientFill()

    var fill: GradientFill {
        get { storedFill }
        set {
            storedFill = Self.normalized(newValue)
            selectedStop = min(max(selectedStop, 0), max(storedFill.stops.count - 1, 0))
            if let midpoint = selectedMidpoint, midpoint.segment >= storedFill.stops.count - 1 {
                selectedMidpoint = nil
            }
            needsDisplay = true
        }
    }

    /// Whether the midpoint diamonds are drawn and editable (class doc).
    var showsMidpoints = false {
        didSet {
            toolTip = Self.toolTip(showsMidpoints: showsMidpoints)
            needsDisplay = true
            invalidateIntrinsicContentSize()
        }
    }

    /// The selected diamond, if any. Assigning it from outside repaints and
    /// fires nothing.
    var selectedMidpoint: Midpoint? {
        didSet { needsDisplay = true }
    }

    /// Fires when a gesture here selects a diamond or clears that selection.
    var onMidpointSelectionChanged: ((Midpoint?) -> Void)?

    /// The space `fill`'s stop colours are expressed in — see the class doc.
    var colorSpace: NSColorSpace = .sRGB {
        didSet { needsDisplay = true }
    }

    /// Fires for a stop added, moved, removed or recoloured HERE.
    var onFillChanged: ((GradientFill) -> Void)?
    /// Fires when a gesture here changes which stop is selected, so a
    /// caller's per-stop controls can follow it.
    var onSelectionChanged: ((Int) -> Void)?
    /// A double-click on a stop asks for its colour. The view deliberately
    /// does not drive `NSColorPanel` itself: the panel is a single shared
    /// window, this view has two callers with two different colour spaces,
    /// and the sheet already owns a colour well that binds the panel
    /// correctly for its own space.
    var onEditColor: ((Int) -> Void)?

    var selectedStop = 0 {
        didSet { needsDisplay = true }
    }

    /// The stop a drag is carrying; nil when nothing is being dragged.
    private var dragIndex: Int?
    /// The diamond a drag is carrying; nil when none is.
    private var dragMidpoint: Midpoint?

    override init(frame frameRect: NSRect) {
        super.init(frame: frameRect)
        wantsLayer = true
        toolTip = Self.toolTip(showsMidpoints: false)
    }

    private static func toolTip(showsMidpoints: Bool) -> String {
        let stops = "Click to add a stop, drag to move it, ⌫ or drag away to remove it"
        return showsMidpoints ? stops + "; drag a diamond to move a midpoint" : stops
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) {
        fatalError("GradientEditorView does not support NSCoder")
    }

    override var intrinsicContentSize: NSSize {
        NSSize(width: 256, height: 56 + (showsMidpoints ? Self.midpointRowHeight : 0))
    }

    // MARK: - Geometry

    /// The stop bar: the strip along the bottom the markers live in.
    private var barRect: NSRect {
        NSRect(
            x: Self.sideInset, y: 0,
            width: max(bounds.width - Self.sideInset * 2, 0), height: Self.barHeight)
    }

    /// The colour ramp above the bar — everything left over below the
    /// midpoint row, so a taller frame gets a taller ramp rather than a gap.
    private var rampRect: NSRect {
        let bottom = Self.barHeight + Self.gap
        let row = showsMidpoints ? Self.midpointRowHeight : 0
        return NSRect(
            x: Self.sideInset, y: bottom,
            width: max(bounds.width - Self.sideInset * 2, 0),
            height: max(bounds.height - bottom - row, 0))
    }

    /// The row above the ramp the opacity diamonds sit in; empty without
    /// `showsMidpoints`.
    private var midpointRow: NSRect {
        guard showsMidpoints else { return .zero }
        return NSRect(
            x: Self.sideInset, y: max(bounds.height - Self.midpointRowHeight, 0),
            width: max(bounds.width - Self.sideInset * 2, 0), height: Self.midpointRowHeight)
    }

    /// Position 0…1 for a view-space x, clamped.
    private func position(forX x: CGFloat) -> Double {
        let rect = rampRect
        guard rect.width > 0 else { return 0 }
        return min(max(Double((x - rect.minX) / rect.width), 0), 1)
    }

    /// The centre of the marker for a stop at `position`.
    private func x(forPosition position: Double) -> CGFloat {
        let rect = rampRect
        return rect.minX + CGFloat(min(max(position, 0), 1)) * rect.width
    }

    // MARK: - Sampling (the core's gradient, mirrored)

    /// One stop with its colour already parsed. The ramp is drawn a column
    /// at a time, so the hex spellings are turned into numbers ONCE per
    /// draw rather than once per column.
    private typealias RampStop = (
        position: Double, rgb: [Double], opacity: Double, midpoint: Double,
        opacityMidpoint: Double
    )

    private func rampStops() -> [RampStop] {
        storedFill.stops.map {
            (
                min(max($0.position, 0), 1), Self.components($0.color), $0.opacity,
                Self.midpointValue(of: $0, opacity: false), Self.midpointValue(of: $0, opacity: true)
            )
        }
    }

    /// A stop's colour or opacity midpoint as a fraction, nil read as 0.5.
    static func midpointValue(of stop: GradientStop, opacity: Bool) -> Double {
        (opacity ? stop.opacityMidpoint : stop.midpoint) ?? 0.5
    }

    /// The colour and opacity `style_gradient::gradient_color` produces at
    /// `t`: the two stops bracketing it, interpolated linearly in the stored
    /// numbers at the segment fraction the core's `dr_gradient_midpoint_fraction`
    /// remaps through the left stop's colour and opacity midpoints, clamped
    /// to the first and last outside their range. Answers
    /// black at zero opacity for an empty stop list, which only a malformed
    /// payload can produce (the core refuses fewer than two).
    private static func sample(
        at t: Double, in stops: [RampStop]
    ) -> (rgb: [Double], opacity: Double) {
        guard let first = stops.first, let last = stops.last else { return ([0, 0, 0], 0) }
        if !t.isFinite || t <= first.position { return (first.rgb, first.opacity) }
        if t >= last.position { return (last.rgb, last.opacity) }
        for index in 0..<(stops.count - 1) {
            let (a, b) = (stops[index], stops[index + 1])
            guard t >= a.position, t < b.position else { continue }
            let span = b.position - a.position
            let f = span > 0 ? (t - a.position) / span : 1
            let fc = RasterDocument.gradientMidpointFraction(f, midpoint: a.midpoint)
            let fa = RasterDocument.gradientMidpointFraction(f, midpoint: a.opacityMidpoint)
            return (
                (0..<3).map { a.rgb[$0] + (b.rgb[$0] - a.rgb[$0]) * fc },
                a.opacity + (b.opacity - a.opacity) * fa
            )
        }
        return (last.rgb, last.opacity)
    }

    /// A `"#rrggbb"` spelling as three 0…1 components; black for anything
    /// that is not six hex digits (the same defensive contract
    /// `AdjustmentLayerPayload.colorHex` keeps). The numeric half of
    /// `AdjustmentColor.color(fromHex:in:)`, kept separate only because the
    /// ramp is drawn a column at a time and a stop's hex is then parsed once
    /// per draw instead of once per column.
    private static func components(_ hex: String) -> [Double] {
        var digits = Substring(hex)
        if digits.hasPrefix("#") { digits = digits.dropFirst() }
        guard digits.count == 6 || digits.count == 8,
              let value = UInt32(digits.prefix(6), radix: 16)
        else { return [0, 0, 0] }
        return [
            Double((value >> 16) & 0xff) / 255,
            Double((value >> 8) & 0xff) / 255,
            Double(value & 0xff) / 255,
        ]
    }

    /// Three 0…1 components as the `"#rrggbb"` a stop stores. Rounds like
    /// every other colour spelling in the app, so a colour picked out of the
    /// ramp and written back lands on the same bytes.
    private static func spelling(_ rgb: [Double]) -> String {
        let byte: (Double) -> Int = { Int((min(max($0, 0), 1) * 255).rounded()) }
        guard rgb.count >= 3 else { return "#000000" }
        return String(format: "#%02x%02x%02x", byte(rgb[0]), byte(rgb[1]), byte(rgb[2]))
    }

    /// The stop colour as a drawable colour, built IN `colorSpace` because
    /// those numbers already live there (the class doc's rule).
    private func color(_ rgb: [Double], _ alpha: Double) -> NSColor {
        guard rgb.count >= 3 else { return .black }
        return NSColor(
            colorSpace: colorSpace,
            components: [
                CGFloat(min(max(rgb[0], 0), 1)), CGFloat(min(max(rgb[1], 0), 1)),
                CGFloat(min(max(rgb[2], 0), 1)), CGFloat(min(max(alpha, 0), 1)),
            ],
            count: 4)
    }

    // MARK: - Drawing

    override func draw(_ dirtyRect: NSRect) {
        let ramp = rampRect
        guard ramp.width > 0, ramp.height > 0 else { return }

        drawChecker(in: ramp)
        // One column per point: the strip is the core's own interpolation
        // evaluated at each column's centre, which is cheap enough at sheet
        // width and exact, where a CGGradient would interpolate its own way
        // through premultiplied alpha.
        let stops = rampStops()
        var x = ramp.minX
        while x < ramp.maxX {
            let width = min(1, ramp.maxX - x)
            let sampled = Self.sample(at: position(forX: x + width / 2), in: stops)
            color(sampled.rgb, sampled.opacity).setFill()
            NSRect(x: x, y: ramp.minY, width: width, height: ramp.height).fill()
            x += 1
        }
        let frame = NSBezierPath(rect: ramp.insetBy(dx: 0.5, dy: 0.5))
        frame.lineWidth = 1
        DS.borderStrong.setStroke()
        frame.stroke()

        // The bar's own ground, so the markers read against something when
        // the ramp above them is pale.
        DS.canvasVoid.setFill()
        barRect.fill()
        // The keyboard target: ⌫ removes the selected stop, so say when the
        // keystroke would land here.
        if window?.firstResponder === self {
            let focus = NSBezierPath(rect: barRect.insetBy(dx: 0.5, dy: 0.5))
            focus.lineWidth = 1
            DS.accent.setStroke()
            focus.stroke()
        }

        for (index, stop) in storedFill.stops.enumerated() {
            drawMarker(stop, selected: index == selectedStop)
        }
        // After the markers, so a diamond near a stop stays visible (and,
        // through `mouseDown`'s order, grabbable).
        for (midpoint, centre) in diamonds() {
            drawDiamond(at: centre, selected: midpoint == selectedMidpoint)
        }
    }

    /// Every diamond with its centre: one colour and one opacity diamond per
    /// segment of non-zero width, at its stop's midpoint. Empty without
    /// `showsMidpoints`.
    private func diamonds() -> [(Midpoint, NSPoint)] {
        guard showsMidpoints, storedFill.stops.count >= 2 else { return [] }
        let stops = storedFill.stops
        var out: [(Midpoint, NSPoint)] = []
        for segment in 0..<(stops.count - 1) {
            let (a, b) = (stops[segment].position, stops[segment + 1].position)
            guard b - a > 0 else { continue }
            for opacity in [false, true] {
                let m = Self.midpointValue(of: stops[segment], opacity: opacity)
                let y = opacity ? midpointRow.midY : barRect.midY
                out.append((
                    Midpoint(segment: segment, opacity: opacity),
                    NSPoint(x: x(forPosition: a + m * (b - a)), y: y)))
            }
        }
        return out
    }

    /// A midpoint diamond: filled with the accent when selected, else
    /// outlined over the bar's ground.
    private func drawDiamond(at centre: NSPoint, selected: Bool) {
        let half = Self.diamondSide / 2
        let path = NSBezierPath()
        path.move(to: NSPoint(x: centre.x, y: centre.y + half))
        path.line(to: NSPoint(x: centre.x + half, y: centre.y))
        path.line(to: NSPoint(x: centre.x, y: centre.y - half))
        path.line(to: NSPoint(x: centre.x - half, y: centre.y))
        path.close()
        if selected {
            DS.accent.setFill()
            path.fill()
        } else {
            DS.canvasVoid.setFill()
            path.fill()
            path.lineWidth = 1
            DS.borderStrong.setStroke()
            path.stroke()
        }
    }

    private func drawChecker(in rect: NSRect) {
        DS.checkerA.setFill()
        rect.fill()
        DS.checkerB.setFill()
        let side = Self.checkerSide
        var row = 0
        var y = rect.minY
        while y < rect.maxY {
            var column = 0
            var x = rect.minX
            while x < rect.maxX {
                if (row + column) % 2 == 0 {
                    NSRect(
                        x: x, y: y, width: min(side, rect.maxX - x),
                        height: min(side, rect.maxY - y)
                    ).fill()
                }
                x += side
                column += 1
            }
            y += side
            row += 1
        }
    }

    /// A stop: a pointer aimed at the ramp over a swatch of its colour, the
    /// colour drawn OPAQUE (its opacity reads in the ramp above, and a
    /// transparent stop still has to be findable).
    private func drawMarker(_ stop: GradientStop, selected: Bool) {
        let bar = barRect
        let centre = x(forPosition: stop.position)
        let half = Self.markerWidth / 2
        let shoulder = bar.maxY - 6
        let path = NSBezierPath()
        path.move(to: NSPoint(x: centre, y: bar.maxY))
        path.line(to: NSPoint(x: centre + half, y: shoulder))
        path.line(to: NSPoint(x: centre + half, y: bar.minY + 1))
        path.line(to: NSPoint(x: centre - half, y: bar.minY + 1))
        path.line(to: NSPoint(x: centre - half, y: shoulder))
        path.close()
        // The shared reader, for the handful of markers: the stop's stored
        // numbers, built IN this view's space and converted nowhere.
        (AdjustmentColor.color(fromHex: stop.color, in: colorSpace) ?? .black).setFill()
        path.fill()
        path.lineWidth = selected ? 2 : 1
        (selected ? DS.accent : DS.borderStrong).setStroke()
        path.stroke()
    }

    override func resetCursorRects() {
        addCursorRect(rampRect, cursor: .crosshair)
    }

    // MARK: - Interaction

    override var acceptsFirstResponder: Bool { true }

    override func becomeFirstResponder() -> Bool {
        let accepted = super.becomeFirstResponder()
        needsDisplay = true
        return accepted
    }

    override func resignFirstResponder() -> Bool {
        let resigned = super.resignFirstResponder()
        needsDisplay = true
        return resigned
    }

    /// The stop whose marker is under `location`, nearest first.
    private func stopIndex(at location: NSPoint) -> Int? {
        guard location.y <= barRect.maxY + 4 else { return nil }
        var best: (index: Int, distance: CGFloat)?
        for (index, stop) in storedFill.stops.enumerated() {
            let distance = abs(location.x - x(forPosition: stop.position))
            if distance <= Self.hitRadius, distance < (best?.distance ?? .infinity) {
                best = (index, distance)
            }
        }
        return best?.index
    }

    /// The diamond under `location` within `reach` of its centre, nearest
    /// first: a colour diamond only for a click inside the stop bar, an
    /// opacity diamond only inside its row (`colourOnly` skips those).
    private func midpoint(at location: NSPoint, reach: CGFloat, colourOnly: Bool) -> Midpoint? {
        var best: (midpoint: Midpoint, distance: CGFloat)?
        for (midpoint, centre) in diamonds() {
            if midpoint.opacity && colourOnly { continue }
            let row = midpoint.opacity ? midpointRow : barRect
            guard location.y >= row.minY, location.y <= row.maxY else { continue }
            let distance = abs(location.x - centre.x)
            if distance <= reach, distance < (best?.distance ?? .infinity) {
                best = (midpoint, distance)
            }
        }
        return best?.midpoint
    }

    /// Selects diamond `midpoint` and the stop that owns it, reporting both.
    private func selectMidpoint(_ midpoint: Midpoint) {
        let changed = selectedMidpoint != midpoint
        selectedMidpoint = midpoint
        if selectedStop != midpoint.segment {
            selectedStop = midpoint.segment
            onSelectionChanged?(midpoint.segment)
        }
        if changed { onMidpointSelectionChanged?(midpoint) }
    }

    /// Drops the diamond selection made here, reporting it when there was one.
    private func clearMidpointSelection() {
        guard selectedMidpoint != nil else { return }
        selectedMidpoint = nil
        onMidpointSelectionChanged?(nil)
    }

    override func mouseDown(with event: NSEvent) {
        window?.makeFirstResponder(self)
        let location = convert(event.locationInWindow, from: nil)
        // A colour diamond whose drawn shape holds the click wins over the
        // stop marker around it, so a diamond near a stop stays grabbable. A
        // double-click on a diamond is a single click: it has no colour.
        if let midpoint = midpoint(
            at: location, reach: Self.diamondSide / 2, colourOnly: true)
        {
            selectMidpoint(midpoint)
            dragMidpoint = midpoint
            return
        }
        if let index = stopIndex(at: location) {
            clearMidpointSelection()
            if selectedStop != index {
                selectedStop = index
                onSelectionChanged?(index)
            }
            if event.clickCount >= 2 {
                onEditColor?(index)
                return
            }
            dragIndex = index
            return
        }
        if let midpoint = midpoint(at: location, reach: Self.diamondReach, colourOnly: false) {
            selectMidpoint(midpoint)
            dragMidpoint = midpoint
            return
        }
        // A single click anywhere else on the strip adds a stop there,
        // taking the colour and opacity the ramp already shows — the second
        // click of a double would otherwise add a second stop on top of the
        // first.
        guard event.clickCount == 1, rampRect.width > 0,
              location.x >= rampRect.minX - Self.sideInset,
              location.x <= rampRect.maxX + Self.sideInset,
              location.y >= 0, location.y <= bounds.maxY
        else { return }
        guard storedFill.stops.count < Self.maxStops else {
            NSSound.beep()
            return
        }
        clearMidpointSelection()
        let t = position(forX: location.x)
        let sampled = Self.sample(at: t, in: rampStops())
        var stops = storedFill.stops
        stops.append(
            GradientStop(
                position: t, color: Self.spelling(sampled.rgb), opacity: sampled.opacity))
        let index = replaceStops(stops, keeping: stops.count - 1)
        dragIndex = index
        onSelectionChanged?(index)
        onFillChanged?(storedFill)
    }

    override func mouseDragged(with event: NSEvent) {
        if let midpoint = dragMidpoint {
            moveMidpoint(midpoint, to: convert(event.locationInWindow, from: nil))
            return
        }
        guard let index = dragIndex, storedFill.stops.indices.contains(index) else { return }
        let location = convert(event.locationInWindow, from: nil)
        // Dragged clear of the strip: let go of it, unless that would leave
        // a gradient the core would refuse.
        if location.y < -Self.removeMargin || location.y > bounds.maxY + Self.removeMargin {
            guard storedFill.stops.count > Self.minStops else { return }
            dragIndex = nil
            removeStop(index)
            return
        }
        let t = position(forX: location.x)
        guard storedFill.stops[index].position != t else { return }
        var stops = storedFill.stops
        stops[index].position = t
        let moved = replaceStops(stops, keeping: index)
        dragIndex = moved
        // Only when the stop actually crossed a neighbour: the fill change
        // below already tells the caller to re-read the selected stop.
        if moved != index { onSelectionChanged?(moved) }
        onFillChanged?(storedFill)
    }

    /// A diamond follows the pointer's x within its segment: the fraction of
    /// the way from stop `segment` to the next, clamped to 5…95 % and rounded
    /// to 1 %. Vertical distance never removes anything.
    private func moveMidpoint(_ midpoint: Midpoint, to location: NSPoint) {
        let stops = storedFill.stops
        guard midpoint.segment >= 0, midpoint.segment + 1 < stops.count else { return }
        let (a, b) = (stops[midpoint.segment].position, stops[midpoint.segment + 1].position)
        guard b - a > 0 else { return }
        let fraction = (position(forX: location.x) - a) / (b - a)
        if writeMidpoint(fraction, for: midpoint) { onFillChanged?(storedFill) }
    }

    /// Stores `value` (clamped to `GradientStop.midpointRange`, rounded to
    /// 0.01, nil at exactly 0.5) as `midpoint`'s, answering whether the
    /// stored value changed.
    private func writeMidpoint(_ value: Double, for midpoint: Midpoint) -> Bool {
        guard value.isFinite, midpoint.segment >= 0,
              midpoint.segment + 1 < storedFill.stops.count
        else { return false }
        let range = GradientStop.midpointRange
        let rounded = (min(max(value, range.lowerBound), range.upperBound) * 100).rounded() / 100
        let stored: Double? = rounded == 0.5 ? nil : rounded
        let current = midpoint.opacity
            ? storedFill.stops[midpoint.segment].opacityMidpoint
            : storedFill.stops[midpoint.segment].midpoint
        guard current != stored else { return false }
        if midpoint.opacity {
            storedFill.stops[midpoint.segment].opacityMidpoint = stored
        } else {
            storedFill.stops[midpoint.segment].midpoint = stored
        }
        needsDisplay = true
        return true
    }

    override func mouseUp(with event: NSEvent) {
        dragIndex = nil
        dragMidpoint = nil
    }

    override func keyDown(with event: NSEvent) {
        switch event.keyCode {
        case 51, 117: // Delete, Forward Delete
            // A midpoint cannot be removed, only moved.
            guard selectedMidpoint == nil, storedFill.stops.count > Self.minStops,
                  storedFill.stops.indices.contains(selectedStop)
            else {
                NSSound.beep()
                return
            }
            removeStop(selectedStop)
        default:
            super.keyDown(with: event)
        }
    }

    /// Drops a stop and selects its neighbour, reporting both — the removal
    /// paths (⌫ and a drag clear of the strip) share it so they can never
    /// leave the selection somewhere different.
    private func removeStop(_ index: Int) {
        guard storedFill.stops.indices.contains(index),
              storedFill.stops.count > Self.minStops
        else { return }
        clearMidpointSelection()
        var stops = storedFill.stops
        stops.remove(at: index)
        storedFill.stops = stops
        selectedStop = min(index, stops.count - 1)
        needsDisplay = true
        onSelectionChanged?(selectedStop)
        onFillChanged?(storedFill)
    }

    // MARK: - Editing from a caller's own controls

    /// Moves stop `index` to `position` (clamped to 0…1), keeping THAT stop
    /// selected as the list re-sorts around it. For a caller's own control —
    /// the sheet's position field — so `onFillChanged` does not fire back at
    /// the control that just wrote.
    func setPosition(_ position: Double, forStop index: Int) {
        editStop(index) { $0.position = position }
    }

    /// Recolours stop `index`; `hex` is in this view's `colorSpace`.
    func setColor(_ hex: String, forStop index: Int) {
        editStop(index) { $0.color = hex }
    }

    /// Sets stop `index`'s opacity (0…1) — how much of what lies underneath
    /// the gradient replaces there.
    func setOpacity(_ opacity: Double, forStop index: Int) {
        editStop(index) { $0.opacity = opacity }
    }

    /// Sets diamond `midpoint`'s value (a fraction, clamped to 5…95 % and
    /// rounded to 1 %, stored nil at 50 %). For a caller's own field, so it
    /// fires nothing.
    func setMidpoint(_ value: Double, for midpoint: Midpoint) {
        _ = writeMidpoint(value, for: midpoint)
    }

    private func editStop(_ index: Int, _ body: (inout GradientStop) -> Void) {
        guard storedFill.stops.indices.contains(index) else { return }
        var stops = storedFill.stops
        body(&stops[index])
        replaceStops(stops, keeping: index)
    }

    // MARK: - Normalizing

    /// Writes `stops` back sorted the way the core sorts them, selects the
    /// stop that was at `keeping` wherever it landed, and answers its new
    /// index. Every edit — a drag, an added stop, a caller's field — goes
    /// through here, which is what keeps the selection ON the stop being
    /// edited when it crosses one of its neighbours.
    @discardableResult
    private func replaceStops(_ stops: [GradientStop], keeping index: Int) -> Int {
        let (sorted, moved) = Self.sortedStops(Self.sanitized(stops), keeping: index)
        storedFill.stops = sorted
        selectedStop = moved
        // A re-sort or a collapsed segment would leave the selected diamond
        // naming another stop's midpoint: drop it, firing nothing (the
        // caller's own sync follows its edit).
        if let midpoint = selectedMidpoint {
            let segment = midpoint.segment
            if moved != index || segment + 1 >= sorted.count
                || sorted[segment + 1].position - sorted[segment].position <= 0
            {
                selectedMidpoint = nil
            }
        }
        needsDisplay = true
        return moved
    }

    /// Every stop's numbers inside the ranges the core accepts. A non-finite
    /// position would also break the sort's ordering, which Swift's `sort`
    /// is entitled to answer with anything at all.
    private static func sanitized(_ stops: [GradientStop]) -> [GradientStop] {
        stops.map { stop in
            var out = stop
            out.position = stop.position.isFinite ? min(max(stop.position, 0), 1) : 0
            out.opacity = stop.opacity.isFinite ? min(max(stop.opacity, 0), 1) : 1
            return out
        }
    }

    /// Position order, ties broken by the incoming order — the stable sort
    /// `style_json`'s parser does, so two stops dropped on the same position
    /// keep the picture they had.
    private static func sortedStops(
        _ stops: [GradientStop], keeping index: Int
    ) -> ([GradientStop], Int) {
        let tagged = stops.enumerated().sorted { a, b in
            a.element.position == b.element.position
                ? a.offset < b.offset
                : a.element.position < b.element.position
        }
        let moved = tagged.firstIndex { $0.offset == index } ?? min(max(index, 0), stops.count - 1)
        return (tagged.map { $0.element }, max(moved, 0))
    }

    private static func normalized(_ fill: GradientFill) -> GradientFill {
        var out = fill
        out.stops = sortedStops(sanitized(fill.stops), keeping: 0).0
        return out
    }
}
