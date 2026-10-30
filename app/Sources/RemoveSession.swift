import AppKit

// The Remove tool: `RemoveSession` is the canvas's drag, `RemoveStroke` turns the
// painted stroke (or the selection) into canvas coverage and starts the one job both the tool's
// commit (`EditorViewController+Remove.removeStrokeCommitted`) and `remove_stroke` run: the local
// model's fill (`RemoveModelRun`, Mode: Model) or a Content-Aware Fill (Mode: Content-Aware).

/// The Remove tool's drag, on the `RedEyeSession` pattern: the canvas owns one
/// and only points it at events.
///
/// The stroke is NOT the brush pipeline's. Nothing is painted while the drag
/// runs: the session collects the pointer's path and draws it as a translucent
/// round-capped line, and the path it hands back at mouse-up becomes the hole
/// of one Content-Aware Fill (`RemoveStroke`). Points are kept unclamped, so a
/// stroke that leaves the canvas runs up to the edge and stops there, as a
/// brush stroke does; the coverage bitmap clips it.
///
/// With "Remove after each stroke" off the controller HOLDS each finished
/// stroke here (`pending`): drawn with the live one until Apply runs them all
/// as one job or Cancel, a tool change or a canvas resize discards them.
struct RemoveSession {
    /// The finished stroke: its centre line in canvas coordinates and the
    /// brush's width, latched at mouse-down.
    struct Result {
        let points: [CGPoint]
        let size: CGFloat
    }

    /// `remove_stroke` takes at most this many points, and a recorded stroke
    /// has to replay through it.
    static let maximumPoints = 10_000
    /// At most this many strokes are held at once: `remove_stroke`'s `strokes` cap, so the
    /// recorded step of an Apply replays through it.
    static let maximumPendingStrokes = 64

    private var points: [CGPoint] = []
    private var size: CGFloat = 0
    /// The held strokes, oldest first.
    private(set) var pending: [Result] = []

    var hasPending: Bool { !pending.isEmpty }

    /// Holds `stroke` until Apply or Cancel; false (nothing held) at the cap.
    mutating func hold(_ stroke: Result) -> Bool {
        guard pending.count < Self.maximumPendingStrokes else { return false }
        pending.append(stroke)
        return true
    }

    mutating func discardPending() {
        pending = []
    }

    mutating func mouseDown(_ point: CGPoint, size: CGFloat) {
        points = [point]
        self.size = size
    }

    /// True when the point extended the stroke and the canvas should redraw.
    mutating func mouseDragged(_ point: CGPoint) -> Bool {
        guard let last = points.last, point != last else { return false }
        if points.count >= Self.maximumPoints - 1 {
            // A drag this long keeps every other sample: the path keeps its
            // shape, at half the pointer's sampling rate, and stays within
            // what one recorded step can carry.
            points = stride(from: 0, to: points.count, by: 2).map { points[$0] }
        }
        points.append(point)
        return true
    }

    /// The stroke, ending at `point`; nil when no press began one. A click is
    /// a stroke of one point, which removes a round dab.
    mutating func mouseUp(_ point: CGPoint) -> Result? {
        guard let last = points.last else { return nil }
        if point != last { points.append(point) }
        let result = Result(points: points, size: size)
        cancel()
        return result
    }

    /// Ends the drag only: held strokes stay (`discardPending` drops them).
    mutating func cancel() {
        points = []
        size = 0
    }

    /// The held strokes and the stroke so far, in the Spot Healing Brush's
    /// ghost colour (`ImageCanvasView+StrokeGhost.swift`): blue at half alpha
    /// says "this is what will be replaced", not "this is paint". Every stroke
    /// is drawn opaque inside ONE transparency layer composited at half alpha,
    /// so a stroke that crosses itself or another does not darken. `context`
    /// is in canvas coordinates, so a line is its `size` canvas px wide at any
    /// zoom and `magnification` has nothing to correct.
    func draw(in context: CGContext, magnification: CGFloat) {
        var strokes = pending
        if !points.isEmpty, size > 0 { strokes.append(Result(points: points, size: size)) }
        guard !strokes.isEmpty else { return }
        context.saveGState()
        context.setAlpha(0.5)
        context.beginTransparencyLayer(auxiliaryInfo: nil)
        context.setStrokeColor(NSColor.systemBlue.cgColor)
        for stroke in strokes where stroke.size > 0 {
            RemoveStroke.stroke(points: stroke.points, size: stroke.size, in: context)
        }
        context.endTransparencyLayer()
        context.restoreGState()
    }
}

/// One Remove gesture, as both the tool and `remove_stroke` describe it: one stroke, or the
/// strokes an Apply runs together (their union is the one hole).
struct RemoveRequest {
    var strokes: [RemoveSession.Result]
    var choice: RetouchSamplingChoice
    var output: RetouchOutput
    /// Content-Aware's synthesis seed; the model has none.
    var seed: UInt64
    var mode: RemoveMode = .model
    /// The hole is the live selection's coverage instead of the stroke (`remove_stroke`'s
    /// `use_selection`); `strokes` is then unused.
    var useSelection = false
}

/// The stroke as a Content-Aware Fill hole.
enum RemoveStroke {
    /// Why a stroke that touches nothing is refused, in the tool's own words:
    /// the fill's sentence for this ("There is no selection to fill") names a
    /// selection the user never made.
    static let coversNothing = "The stroke covers no pixel of the picture: it lies outside the "
        + "canvas, or outside the selection, which confines it."
    /// `use_selection` with no selection to take the hole from.
    static let noSelection = "There is no selection to remove: make one first, or paint a stroke."

    /// Strokes the centre line through `points`, `size` px wide with round
    /// caps and joins, in the context's current stroke colour: the one
    /// geometry the drag's preview and the committed coverage share. A single
    /// point gets an epsilon segment so it strokes as a round dot
    /// (`AgentServer.strokeCoverage`'s rule).
    static func stroke(points: [CGPoint], size: CGFloat, in context: CGContext) {
        guard let first = points.first else { return }
        context.setLineWidth(size)
        context.setLineCap(.round)
        context.setLineJoin(.round)
        context.beginPath()
        context.move(to: first)
        if points.count == 1 {
            context.addLine(to: CGPoint(x: first.x + 0.01, y: first.y))
        } else {
            for point in points.dropFirst() { context.addLine(to: point) }
        }
        context.strokePath()
    }

    /// Canvas-sized u8 coverage of the union of `strokes` (row 0 = top, 255
    /// inside, the anti-aliased rim between), each clipped to `selection` when
    /// there is one. CoreGraphics rasterises them into one gray bitmap,
    /// `CanvasSelection.maskBytes`'s own frame, so overlapping strokes cover a
    /// pixel once. `covered` counts the nonzero bytes. nil when nothing is
    /// covered, a stroke is empty or has no width, or the bitmap cannot be
    /// made.
    static func coverage(strokes: [RemoveSession.Result], width: Int, height: Int,
                         selection: CanvasSelection?) -> (bytes: [UInt8], covered: Int)? {
        guard !strokes.isEmpty, strokes.allSatisfy({ !$0.points.isEmpty && $0.size > 0 }),
              width > 0, height > 0 else { return nil }
        var bytes = [UInt8](repeating: 0, count: width * height)
        let drawn = bytes.withUnsafeMutableBufferPointer { buffer -> Bool in
            guard let context = CGContext(
                data: buffer.baseAddress, width: width, height: height, bitsPerComponent: 8,
                bytesPerRow: width, space: CGColorSpaceCreateDeviceGray(),
                bitmapInfo: CGImageAlphaInfo.none.rawValue)
            else { return false }
            // Flipped, so byte row 0 is the canvas's top row and drawing
            // coordinates are canvas coordinates.
            context.translateBy(x: 0, y: CGFloat(height))
            context.scaleBy(x: 1, y: -1)
            selection?.clip(context)
            context.setStrokeColor(gray: 1, alpha: 1)
            for each in strokes { stroke(points: each.points, size: each.size, in: context) }
            return true
        }
        let boxes = strokes.compactMap { Self.box(points: $0.points, size: $0.size, width: width,
                                                  height: height) }
        guard drawn, let first = boxes.first else { return nil }
        let area = boxes.dropFirst().reduce(first) { union, next in
            (min(union.rows.lowerBound, next.rows.lowerBound)
                ..< max(union.rows.upperBound, next.rows.upperBound),
             min(union.columns.lowerBound, next.columns.lowerBound)
                ..< max(union.columns.upperBound, next.columns.upperBound))
        }
        // Only the strokes' box can hold a nonzero byte, so only its rows and
        // columns are scanned: a short stroke on a 100 MP canvas costs its own
        // area, not the canvas's.
        var covered = 0
        bytes.withUnsafeBufferPointer { buffer in
            for row in area.rows {
                let start = row * width
                for index in (start + area.columns.lowerBound)..<(start + area.columns.upperBound)
                where buffer[index] != 0 {
                    covered += 1
                }
            }
        }
        guard covered > 0 else { return nil }
        return (bytes, covered)
    }

    /// The canvas rows and columns the stroke can touch: the points' extent
    /// grown by half the width plus one pixel for the anti-aliased rim,
    /// clipped to the canvas. nil when that is empty or a coordinate is not
    /// finite.
    private static func box(points: [CGPoint], size: CGFloat, width: Int, height: Int)
        -> (rows: Range<Int>, columns: Range<Int>)? {
        guard let first = points.first else { return nil }
        var minX = first.x, maxX = first.x, minY = first.y, maxY = first.y
        for point in points.dropFirst() {
            minX = min(minX, point.x)
            maxX = max(maxX, point.x)
            minY = min(minY, point.y)
            maxY = max(maxY, point.y)
        }
        let reach = size / 2 + 1
        let bounds = [minX - reach, maxX + reach, minY - reach, maxY + reach]
        guard bounds.allSatisfy({ $0.isFinite }) else { return nil }
        // Clamped as CGFloat first, so a far-off point cannot overflow Int.
        func clamp(_ value: CGFloat, _ limit: Int) -> Int {
            Int(min(max(value, 0), CGFloat(limit)))
        }
        let x0 = clamp(bounds[0].rounded(.down), width)
        let x1 = clamp(bounds[1].rounded(.up), width)
        let y0 = clamp(bounds[2].rounded(.down), height)
        let y1 = clamp(bounds[3].rounded(.up), height)
        guard x0 < x1, y0 < y1 else { return nil }
        return (y0..<y1, x0..<x1)
    }

    /// Starts the gesture's one job. Mode: Model hands the hole as u16 coverage (the stroke's
    /// bytes × 257, or the selection's own samples) to `RemoveModelRun.start`. Mode:
    /// Content-Aware makes the stroke's coverage (or the selection's bytes) the hole of a
    /// Content-Aware Fill at the automatic ring, through the same
    /// `ContentAwareFillWork` the fill's own sheet and `content_aware_fill`
    /// plan and admit with (`AgentServer.contentAwareFill`'s order: admit the
    /// work's forecast, then start the job with no host scratch of its own).
    /// The job's details carry `covered_pixels`. Throws the fill's refusals
    /// unchanged, so a stroke too large to fill is refused in the fill's
    /// sentences.
    static func start(_ request: RemoveRequest, document: ImageDocument, layer: Int,
                      selection: CanvasSelection?, record: [ActionStep],
                      showProgress: Bool) throws -> DocumentJob {
        guard let source = document.doc else {
            throw DoctoredCoreError(message: "Document has no image.")
        }
        if let selection,
           selection.canvasWidth != source.width || selection.canvasHeight != source.height {
            throw DoctoredCoreError(
                message: "The selection was made on a canvas of a different size. Make it "
                    + "again on this document.")
        }
        if request.useSelection {
            guard let selection else { throw DoctoredCoreError(message: noSelection) }
            if request.mode == .model {
                let samples = selection.maskSamples16()
                let covered = samples.reduce(0) { $1 > 0 ? $0 + 1 : $0 }
                guard covered > 0 else { throw DoctoredCoreError(message: coversNothing) }
                return try modelJob(request, coverage: samples, covered: covered, document: document,
                                    layer: layer, record: record, showProgress: showProgress)
            }
        }
        let painted: (bytes: [UInt8], covered: Int)
        if request.useSelection, let selection {
            let bytes = selection.maskBytes()
            let covered = bytes.reduce(0) { $1 > 0 ? $0 + 1 : $0 }
            guard covered > 0 else { throw DoctoredCoreError(message: coversNothing) }
            painted = (bytes, covered)
        } else {
            guard let stroke = coverage(
                strokes: request.strokes, width: source.width, height: source.height,
                selection: selection)
            else { throw DoctoredCoreError(message: coversNothing) }
            painted = stroke
        }
        if request.mode == .model {
            return try modelJob(request, coverage: painted.bytes.map { UInt16($0) * 257 },
                                covered: painted.covered, document: document, layer: layer,
                                record: record, showProgress: showProgress)
        }
        var settings = ContentAwareFillSettings()
        settings.ring = 0
        settings.seed = request.seed
        settings.choice = request.choice
        settings.output = request.output
        let work = try ContentAwareFillWork(
            source: source, layer: layer, coverage: painted.bytes, settings: settings)
        _ = try RasterResources.shared.admit(scratch: work.scratch)
        let job = try DocumentJobs.start(
            document: document, layer: layer, title: "Remove", scratch: 0, record: record,
            showProgress: showProgress, rasterizesTarget: request.output == .layer,
            compute: { _, control in try work.compute(control: control) })
        job.details["covered_pixels"] = painted.covered
        return job
    }

    /// Mode: Model's job (`RemoveModelRun.start`) with `covered_pixels` in its details.
    private static func modelJob(_ request: RemoveRequest, coverage: [UInt16], covered: Int,
                                 document: ImageDocument, layer: Int, record: [ActionStep],
                                 showProgress: Bool) throws -> DocumentJob {
        let job = try RemoveModelRun.start(request, coverage: coverage, document: document,
                                           layer: layer, record: record, showProgress: showProgress)
        job.details["covered_pixels"] = covered
        return job
    }
}
