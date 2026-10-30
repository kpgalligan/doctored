import AppKit

// At 16 bits, unversioned typed point calls generate native dabs through the same walk
// (`NativeBrushStrokeSession.generated`: unit pressure, no timing) and rasterize through
// `dr_brush_u16`, so they refuse by name at BOTH core limits (50,000 dabs, 2 billion work units);
// the ×257 byte stroke adapter is gone. Every over-cap MCP refusal is
// `NativeBrushStroke.dabLimitRefusal` or the core's own sentence verbatim — refuse by name; no
// partial stroke. Native Quick Mask retains canonical dabs and starts work only after ImageDocument
// captures the selection/target. A CANVAS stroke is the one exception: its session runs in
// `.stopInk` mode, so a drag that reaches either cap stops laying ink (one beep) and mouse-up
// commits the dabs already laid as one undo step; dab diameters reach 5,000 px
// (`EditorTool.maximumBrushSize`). A canvas SMUDGE stroke also stops ink at Smudge's own bound
// (`maximumSmudgeWork`, the core's `MAX_SMUDGE_WORK`, 256 million): the core walks it a pixel at a
// time and the 8-bit stroke runs synchronously on the final tick.

/// RZBR version 1 records resolved deposits, including tablet pressure and
/// timer ticks. Four little-endian binary64 values per dab are x, y, diameter
/// and deposit opacity. Geometry is canvas-relative; pixel work stays in Rust.
struct NativeBrushStroke {
    static let maximumDabs = 50_000
    /// The largest dab diameter a stroke carries: `EditorTool.maximumBrushSize`'s 5,000 px.
    static let maximumDiameter = 5000.0
    /// The core's work cap for one native brush job (`brush_coverage::MAX_BRUSH_WORK`).
    static let maximumWork: UInt64 = 2_000_000_000
    /// The core's work bound for one Smudge stroke (`brush_coverage::MAX_SMUDGE_WORK`).
    static let maximumSmudgeWork: UInt64 = 256_000_000
    static let dabLimitRefusal = "This stroke exceeds the native brush limit of 50,000 dabs; no partial stroke was applied. Use a shorter stroke or wider spacing."
    let tip: BrushTip
    let pixel: Bool
    let dabs: [Double]
    let payload: PrecisionEditPayload

    static func jsonNumber(_ value: Any?) -> Double? {
        guard let number = value as? NSNumber, CFGetTypeID(number) != CFBooleanGetTypeID() else { return nil }
        return number.doubleValue
    }

    static func jsonBoolean(_ value: Any?) -> Bool? {
        guard let number = value as? NSNumber, CFGetTypeID(number) == CFBooleanGetTypeID() else { return nil }
        return number.boolValue
    }

    init(tip: BrushTip, pixel: Bool, dabs: [Double]) throws {
        guard dabs.count / 4 <= Self.maximumDabs else {
            throw DoctoredCoreError(message: Self.dabLimitRefusal)
        }
        guard !dabs.isEmpty, dabs.count % 4 == 0,
              dabs.allSatisfy(\.isFinite), tip.hardness.isFinite, (0...1).contains(tip.hardness),
              tip.roundness.isFinite, (0.01...1).contains(tip.roundness),
              tip.angleDegrees.isFinite, (-180...180).contains(tip.angleDegrees) else {
            throw DoctoredCoreError(message: "Invalid native stroke tip or dab tuples; no partial stroke was applied.")
        }
        for i in stride(from: 0, to: dabs.count, by: 4) {
            guard abs(dabs[i]) <= 1_000_000, abs(dabs[i + 1]) <= 1_000_000,
                  (1...Self.maximumDiameter).contains(dabs[i + 2]), (0...1).contains(dabs[i + 3]) else {
                throw DoctoredCoreError(message: "Native dab coordinates, diameter or opacity are outside their supported range.")
            }
        }
        self.tip = tip; self.pixel = pixel; self.dabs = dabs
        payload = try PrecisionEditPayload.admitted(bytes: UInt64(dabs.count) * 8 + 4096)
    }

    var request: String {
        get throws {
            try NativeSelectionWork.encoding(["version": 1, "hardness": Double(tip.hardness),
                "roundness": Double(tip.roundness), "angle_degrees": Double(tip.angleDegrees), "pixel": pixel])
        }
    }

    func healBounds(width: Int, height: Int) -> (x: Int, y: Int, width: Int, height: Int) {
        var left = Double(width), top = Double(height), right = 0.0, bottom = 0.0
        for i in stride(from: 0, to: dabs.count, by: 4) {
            let radius = dabs[i + 2] / 2 + 2
            left = min(left, dabs[i] - radius); right = max(right, dabs[i] + radius)
            top = min(top, dabs[i + 1] - radius); bottom = max(bottom, dabs[i + 1] + radius)
        }
        let x = min(width - 1, max(0, Int(floor(left))))
        let y = min(height - 1, max(0, Int(floor(top))))
        return (x, y, max(1, min(width, Int(ceil(right))) - x), max(1, min(height, Int(ceil(bottom))) - y))
    }

    var canonical: [String: Any] {
        get throws {
        _ = try RasterResources.shared.admit(scratch: UInt64(dabs.count) * 24 + 4096)
        var data = Data(); data.reserveCapacity(dabs.count * 8)
        for value in dabs {
            var bits = value.bitPattern.littleEndian
            withUnsafeBytes(of: &bits) { data.append(contentsOf: $0) }
        }
        return ["version": 1, "encoding": "le_f64_xy_diameter_opacity", "dabs": data.base64EncodedString(),
                "hardness": Double(tip.hardness), "roundness": Double(tip.roundness),
                "angle_degrees": Double(tip.angleDegrees), "pixel": pixel]
        }
    }

    init(canonical: [String: Any]) throws {
        let allowed: Set<String> = ["version", "encoding", "dabs", "hardness", "roundness", "angle_degrees", "pixel"]
        guard Set(canonical.keys) == allowed,
              Self.jsonNumber(canonical["version"]) == 1,
              canonical["encoding"] as? String == "le_f64_xy_diameter_opacity",
              let text = canonical["dabs"] as? String,
              let hardness = Self.jsonNumber(canonical["hardness"]),
              let roundness = Self.jsonNumber(canonical["roundness"]),
              let angle = Self.jsonNumber(canonical["angle_degrees"]),
              let pixel = Self.jsonBoolean(canonical["pixel"]) else {
            throw DoctoredCoreError(message: "Invalid or unsupported canonical native stroke version, encoding or fields.")
        }
        guard text.utf8.count <= Self.maximumDabs * 44 else {
            throw DoctoredCoreError(message: Self.dabLimitRefusal)
        }
        _ = try RasterResources.shared.admit(scratch: UInt64(text.utf8.count) * 3 + 4096)
        guard let data = Data(base64Encoded: text), !data.isEmpty, data.count % 32 == 0 else {
            throw DoctoredCoreError(message: "Invalid canonical dab payload length or base64 encoding.")
        }
        guard data.count / 32 <= Self.maximumDabs else {
            throw DoctoredCoreError(message: Self.dabLimitRefusal)
        }
        var values = [Double](); values.reserveCapacity(data.count / 8)
        data.withUnsafeBytes { bytes in
            for i in stride(from: 0, to: data.count, by: 8) {
                values.append(Double(bitPattern: UInt64(littleEndian: bytes.loadUnaligned(fromByteOffset: i, as: UInt64.self))))
            }
        }
        try self.init(tip: BrushTip(hardness: CGFloat(hardness),
            angleDegrees: CGFloat(angle), roundness: CGFloat(roundness)), pixel: pixel, dabs: values)
    }

    private func footprintRequest(opacity: Double = 1) throws -> String {
        try NativeSelectionWork.encoding(["version": 1, "hardness": Double(tip.hardness),
            "roundness": Double(tip.roundness), "angle_degrees": Double(tip.angleDegrees), "pixel": pixel, "opacity": opacity])
    }

    /// The core's scratch estimate for `footprint`, planned on main.
    func footprintPlan(width: Int, height: Int) throws -> UInt64 {
        let context = try NativeSelectionWork.context(nil)
        let plan = try RasterSelection.nativeBrushPlan(width: width, height: height, request: footprintRequest(),
            dabs: dabs, workingLimit: context.limit, otherRetained: context.other)
        return try PrecisionDocumentDescriptor.integer(plan, "scratch_bytes")
    }

    /// The stroke's u16 footprint at opacity 1, under the caller's control
    /// with no modal wait: a job's worker calls it.
    func footprint(width: Int, height: Int, control: RasterJobControl) throws -> [UInt16] {
        let context = try NativeSelectionWork.context(nil)
        return try RasterSelection.nativeBrush(width: width, height: height, request: footprintRequest(),
            dabs: dabs, workingLimit: context.limit, otherRetained: context.other, control: control)
    }

    func coverage(width: Int, height: Int, selection: CanvasSelection?, opacity: Double = 1) throws -> PrecisionScalarBuffer {
        let request = try footprintRequest(opacity: opacity)
        let context = try NativeSelectionWork.context(nil)
        let plan = try RasterSelection.nativeBrushPlan(width: width, height: height, request: request,
            dabs: dabs, workingLimit: context.limit, otherRetained: context.other)
        let scratch = try PrecisionDocumentDescriptor.integer(plan, "scratch_bytes")
        let samples = try PrecisionCapturedWork.run(scratchEstimate: scratch, title: "Native Brush Coverage") { control in
            let context = try NativeSelectionWork.context(nil)
            return try RasterSelection.nativeBrush(width: width, height: height, request: request,
                dabs: dabs, workingLimit: context.limit, otherRetained: context.other, control: control)
        }
        let footprint = try PrecisionScalarBuffer(samples: samples, width: width, height: height, provenance: .nativeU16)
        if let selection {
            let selected = try PrecisionStrokeCoverage.selection(width: width, height: height, selection: selection)
            let input = try PrecisionScalarBuffer.joining([footprint, selected])
            let result = try NativeSelectionWork(width: width, height: height,
                operation: ["kind": "blend", "channels": 1, "mode": Int(DR_BLEND_MULTIPLY.rawValue), "opacity": 1],
                input: input.samples).run(title: "Clip Native Stroke")
            return try PrecisionScalarBuffer(samples: result, width: width, height: height, provenance: .nativeU16)
        }
        return footprint
    }
}

/// One value session on the canvas. Actual stamps append the resolved diameter
/// and deposit, so replay never approximates pressure or reconstructs timing.
struct NativeBrushStrokeSession {
    /// What a stamp past a cap does. `.refuse` (the MCP walk, the default)
    /// refuses the whole stroke by name. `.stopInk` (a canvas stroke)
    /// drops that stamp and every later one and sets `isFull`, so the dabs
    /// already laid still resolve and commit; `canvasPixels` is the extent the
    /// commit plans over.
    enum Overflow {
        case refuse
        case stopInk(canvasPixels: Int)
    }

    let tip: BrushTip
    let size: CGFloat
    let pressureOpacity: Bool
    let strokeOpacity: Double
    let erasing: Bool
    let overflow: Overflow
    /// A Smudge stroke: in `.stopInk` mode a stamp is also charged
    /// against `NativeBrushStroke.maximumSmudgeWork` (see `smudgeCharge`).
    let smudging: Bool
    let reservation: PrecisionEditPayload?
    var dabs: [Double] = []
    var refusal: String?
    /// `.stopInk` only: a cap was reached; later stamps are dropped.
    private(set) var isFull = false
    /// `.stopInk` only: the work charged so far (see `workCharge`).
    private var usedWork: UInt64 = 0
    /// `.stopInk` and `smudging` only: the Smudge work charged so far.
    private var usedSmudgeWork: UInt64 = 0
    /// `.stopInk` and `smudging` only: the last dab laid and its `workCharge`.
    private var lastSmudgeDab: (point: CGPoint, charge: UInt64)?

    /// Whether a stamp would still lay ink: no refusal and no cap reached.
    var acceptsInk: Bool { refusal == nil && !isFull }
    private var fullNoticed = false

    /// True exactly once after the session turns full: the canvas's one beep.
    mutating func takeFullNotice() -> Bool {
        guard isFull, !fullNoticed else { return false }
        fullNoticed = true
        return true
    }

    init(tip: BrushTip, size: CGFloat, pressureOpacity: Bool,
         strokeOpacity: Double = 1, erasing: Bool = false, overflow: Overflow = .refuse,
         smudging: Bool = false) {
        self.tip = tip; self.size = size; self.pressureOpacity = pressureOpacity
        self.strokeOpacity = strokeOpacity; self.erasing = erasing; self.overflow = overflow
        self.smudging = smudging
        // The core's plan charges `n + |F|` up front, and `|F| ≤ n`.
        if case .stopInk(let canvasPixels) = overflow { usedWork = 2 * UInt64(max(canvasPixels, 0)) }
        do { reservation = try PrecisionEditPayload.admitted(bytes: UInt64(NativeBrushStroke.maximumDabs) * 64 + 4096) }
        catch { reservation = nil; refusal = error.localizedDescription }
    }

    /// One dab's work in `.stopInk` mode, never below what the core's plan
    /// charges for it (masters: `brush_coverage::plan` and `band_bound`): the
    /// clipped box is at most `s²` with `s = ⌈d⌉ + 2`, and a hard dab
    /// (hardness ≥ 1, not the size-1 pixel brush) adds 64 per band pixel,
    /// `⌈4πaρ + πρ²⌉` of them with `a = d/2` and `ρ = √2 + 2⁻¹⁶·a`.
    static func workCharge(diameter: CGFloat, tip: BrushTip, size: CGFloat) -> UInt64 {
        let d = Double(diameter)
        guard d.isFinite, d >= 0, d <= 1_000_000 else { return NativeBrushStroke.maximumWork }
        let side = UInt64(d.rounded(.up)) + 2
        var charge = side * side
        if tip.hardness >= 1, size > 1 {
            let a = d / 2
            let rho = 2.0.squareRoot() + a / 65536
            charge += 64 * UInt64((4 * Double.pi * a * rho + Double.pi * rho * rho).rounded(.up))
        }
        return charge
    }

    /// The Smudge work of the move from the last dab laid to a dab at `point`
    /// whose `workCharge` is `charge`, never below what the core charges for
    /// it (master: `retouch_smudge::smudge_work`): the move is walked in
    /// `m = max(1, ⌈distance⌉)` sub-steps, each a dab whose diameter lies
    /// between the two dabs', charged its dab work + 1. 0 for the first dab,
    /// which only anchors. nil when the figure does not fit.
    private func smudgeCharge(to point: CGPoint, charge: UInt64) -> UInt64? {
        guard let last = lastSmudgeDab else { return 0 }
        let distance = Double(hypot(point.x - last.point.x, point.y - last.point.y))
        guard distance.isFinite, distance <= Double(NativeBrushStroke.maximumSmudgeWork) else {
            return nil
        }
        let steps = max(1, UInt64(distance.rounded(.up)))
        let (product, overflow) = steps.multipliedReportingOverflow(by: max(last.charge, charge) + 1)
        return overflow ? nil : product
    }

    mutating func stamp(at point: CGPoint, diameter: CGFloat, pressure: CGFloat) {
        guard acceptsInk else { return }
        switch overflow {
        case .refuse:
            guard dabs.count / 4 < NativeBrushStroke.maximumDabs else {
                refusal = NativeBrushStroke.dabLimitRefusal
                return
            }
        case .stopInk:
            let charge = Self.workCharge(diameter: diameter, tip: tip, size: size)
            guard dabs.count / 4 < NativeBrushStroke.maximumDabs,
                  usedWork <= NativeBrushStroke.maximumWork,
                  charge <= NativeBrushStroke.maximumWork - usedWork else {
                isFull = true
                return
            }
            if smudging {
                guard let move = smudgeCharge(to: point, charge: charge),
                      usedSmudgeWork <= NativeBrushStroke.maximumSmudgeWork,
                      move <= NativeBrushStroke.maximumSmudgeWork - usedSmudgeWork else {
                    isFull = true
                    return
                }
                usedSmudgeWork += move
                lastSmudgeDab = (point, charge)
            }
            usedWork += charge
        }
        let deposit = tip.flow * (pressureOpacity ? min(max(pressure, 0), 1) : 1)
        dabs.append(contentsOf: [Double(point.x), Double(point.y), Double(diameter), Double(deposit)])
    }

    mutating func pixelSegment(from start: CGPoint, to end: CGPoint, fromPressure: CGFloat = 1, toPressure: CGFloat = 1) {
        let distance = hypot(end.x - start.x, end.y - start.y)
        guard distance.isFinite, distance <= CGFloat(NativeBrushStroke.maximumDabs) else {
            if case .stopInk = overflow { isFull = true } else { refusal = NativeBrushStroke.dabLimitRefusal }
            return
        }
        let steps = max(1, Int(ceil(distance)))
        for i in 1...steps {
            let t = CGFloat(i) / CGFloat(steps)
            stamp(at: CGPoint(x: floor(start.x + (end.x - start.x) * t) + 0.5,
                y: floor(start.y + (end.y - start.y) * t) + 0.5), diameter: 1,
                pressure: fromPressure + (toPressure - fromPressure) * t)
            if !acceptsInk { return }
        }
    }

    func resolved() throws -> NativeBrushStroke {
        if let refusal { throw DoctoredCoreError(message: refusal) }
        return try NativeBrushStroke(tip: tip, pixel: size <= 1, dabs: dabs)
    }
}

extension NativeBrushStrokeSession {
    /// The generated MCP walk: public points, pressures and timer ticks
    /// resolved to canonical dabs. Versioned calls and unversioned 16-bit
    /// typed point strokes share this one walk.
    static func generated(points: [CGPoint], size: CGFloat, tip: BrushTip, pressures: [Double], times: [Double],
                          duration: Double, pressureSize: Bool, pressureOpacity: Bool, airbrush: Bool) throws -> NativeBrushStroke {
        let size = Double(size)
        var session = NativeBrushStrokeSession(tip: tip, size: CGFloat(size), pressureOpacity: pressureOpacity)
        func diameter(_ pressure: Double) -> CGFloat { CGFloat(pressureSize ? max(1, size * pressure) : size) }
        session.stamp(at: points[0], diameter: diameter(pressures[0]), pressure: CGFloat(pressures[0]))
        var anchor = points[0], pressure = pressures[0]
        for i in points.indices.dropFirst() {
            if size <= 1 {
                session.pixelSegment(from: anchor, to: points[i], fromPressure: CGFloat(pressure), toPressure: CGFloat(pressures[i]))
                anchor = points[i]; pressure = pressures[i]; continue
            }
            var distance = hypot(points[i].x - anchor.x, points[i].y - anchor.y)
            var spacing = SoftBrush.spacing(for: diameter(pressure), percent: tip.spacingPercent)
            while distance >= spacing, session.refusal == nil {
                let fraction = spacing / distance
                anchor.x += (points[i].x - anchor.x) * fraction
                anchor.y += (points[i].y - anchor.y) * fraction
                pressure += (pressures[i] - pressure) * Double(fraction)
                session.stamp(at: anchor, diameter: diameter(pressure), pressure: CGFloat(pressure))
                distance = hypot(points[i].x - anchor.x, points[i].y - anchor.y)
                spacing = SoftBrush.spacing(for: diameter(pressure), percent: tip.spacingPercent)
            }
            if session.refusal != nil { break }
        }
        if airbrush, duration >= 0.05 {
            var segment = 0
            for tick in 1...Int(floor(duration / 0.05 + 1e-9)) {
                let time = Double(tick) * 0.05
                while segment + 1 < times.count, times[segment + 1] <= time { segment += 1 }
                var point = points[segment], p = pressures[segment]
                if segment + 1 < times.count, times[segment + 1] > times[segment] {
                    let t = min(1, max(0, (time - times[segment]) / (times[segment + 1] - times[segment])))
                    point.x += (points[segment + 1].x - point.x) * t
                    point.y += (points[segment + 1].y - point.y) * t
                    p += (pressures[segment + 1] - p) * t
                }
                session.stamp(at: point, diameter: diameter(p), pressure: CGFloat(p))
                if session.refusal != nil { break }
            }
        }
        return try session.resolved()
    }

    static func generated(points: [CGPoint], size: CGFloat, tip: BrushTip) throws -> NativeBrushStroke {
        try generated(points: points, size: size, tip: tip, pressures: Array(repeating: 1, count: points.count),
                      times: Array(repeating: 0, count: points.count), duration: 0,
                      pressureSize: false, pressureOpacity: false, airbrush: false)
    }
}
