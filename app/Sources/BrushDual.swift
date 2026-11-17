import AppKit

/// Brush dynamics' dual brush (`designs/brush-dynamics.md`, slice 4; the rule is the core's,
/// `brush_dual.rs`): a second tip that walks the first tip's path — the polyline through its
/// spacing stamps' nominal centres, never an airbrush tick — with its own absolute size, spacing
/// and scatter, and whose accumulated coverage multiplies the first tip's before the stroke's one
/// rounding. Coverage only, so every depth, mode and target takes it; the Brush and the Eraser only
/// (`EditorTool.takesDualBrushDynamics`). A stroke records the second tip's resolved dabs beside
/// the first's (`StrokeDual`), never its seed.
extension BrushDynamics {
    struct Dual: Codable, Equatable {
        /// The second tip's diameter in pixels (1…5000), whatever the brush size.
        var size: Double
        /// The second tip's hardness, roundness (0.01…1) and angle (−180…180), the native brush
        /// request's own ranges.
        var hardness: Double = 1
        var roundness: Double = 1
        var angleDegrees: Double = 0
        /// The second walk's spacing in percent of `size` (1…200).
        var spacing: Double = 25
        /// The second tip's scatter, as the first's: amount in diameters, count and count jitter.
        var scatter: Double = 0
        var bothAxes = false
        var count = 1
        var countJitter: Double = 0

        static let sizeRange = 1.0...5000.0
        static let spacingRange = 1.0...200.0

        /// `{"size", "hardness", "roundness", "angle_degrees", "spacing", "scatter": {"amount",
        /// "both_axes", "count", "count_jitter"}}`: the stored, saved and MCP form.
        var json: [String: Any] {
            ["size": size, "hardness": hardness, "roundness": roundness,
             "angle_degrees": angleDegrees, "spacing": spacing,
             "scatter": ["amount": scatter, "both_axes": bothAxes, "count": count,
                         "count_jitter": countJitter] as [String: Any]]
        }

        /// The second tip as a native brush tip (flow 1: a second dab's deposit is 1).
        var tip: BrushTip {
            BrushTip(hardness: CGFloat(hardness), flow: 1, spacingPercent: CGFloat(spacing),
                     angleDegrees: CGFloat(angleDegrees), roundness: CGFloat(roundness))
        }

        /// The walk's request (`dr_brush_dual_create`): the first tip's `seed`, the size, the
        /// spacing and the scatter. The second tip's shape is the rasterizer's request
        /// (`StrokeDual.request`), not the walk's.
        func coreRequest(seed: UInt32) throws -> String {
            try NativeSelectionWork.encoding([
                "version": 1, "seed": Int(seed), "size": size, "spacing": spacing,
                "scatter": ["amount": scatter, "both_axes": bothAxes, "count": count,
                            "count_jitter": countJitter] as [String: Any]])
        }
    }
}

/// An extension, so `Dual` keeps its memberwise initializer.
extension BrushDynamics.Dual {
    /// Strict parse of `json`'s form: `size` required, every other key optional at its default,
    /// each refusal naming `dynamics.dual.<key>`.
    init(json raw: Any) throws {
        guard let object = raw as? [String: Any] else {
            throw DoctoredCoreError(message: "dynamics.dual must be an object.")
        }
        let keys = ["size", "hardness", "roundness", "angle_degrees", "spacing", "scatter"]
        if let key = Set(object.keys).subtracting(keys).sorted().first {
            throw DoctoredCoreError(message: "dynamics.dual takes " + keys.joined(separator: ", ")
                + "; it has no \(key).")
        }
        func number(_ value: Any?, _ name: String, _ range: ClosedRange<Double>) throws -> Double? {
            guard let value else { return nil }
            guard let number = NativeBrushStroke.jsonNumber(value), number.isFinite,
                  range.contains(number) else {
                throw DoctoredCoreError(message: "dynamics.dual.\(name) must be a number from "
                    + String(format: "%g to %g.", range.lowerBound, range.upperBound))
            }
            return number
        }
        guard let size = try number(object["size"], "size", Self.sizeRange) else {
            throw DoctoredCoreError(message: "dynamics.dual needs a size, the second tip's "
                + "diameter from 1 to 5000 px.")
        }
        self.init(size: size)
        hardness = try number(object["hardness"], "hardness", 0...1) ?? 1
        roundness = try number(object["roundness"], "roundness", 0.01...1) ?? 1
        angleDegrees = try number(object["angle_degrees"], "angle_degrees", -180...180) ?? 0
        spacing = try number(object["spacing"], "spacing", Self.spacingRange) ?? 25
        guard let rawScatter = object["scatter"] else { return }
        guard let scatter = rawScatter as? [String: Any] else {
            throw DoctoredCoreError(message: "dynamics.dual.scatter must be an object.")
        }
        let scatterKeys: Set<String> = ["amount", "both_axes", "count", "count_jitter"]
        if let key = Set(scatter.keys).subtracting(scatterKeys).sorted().first {
            throw DoctoredCoreError(message: "dynamics.dual.scatter takes amount, both_axes, count "
                + "and count_jitter; it has no \(key).")
        }
        self.scatter = try number(scatter["amount"], "scatter.amount",
                                  0...BrushDynamics.maximumScatter) ?? 0
        if let raw = scatter["both_axes"] {
            guard let flag = NativeBrushStroke.jsonBoolean(raw) else {
                throw DoctoredCoreError(message: "dynamics.dual.scatter.both_axes must be true or "
                    + "false.")
            }
            bothAxes = flag
        }
        if let count = try number(scatter["count"], "scatter.count",
                                  1...Double(BrushDynamics.maximumCount)) {
            guard count == count.rounded() else {
                throw DoctoredCoreError(message: "dynamics.dual.scatter.count must be a whole "
                    + "number from 1 to \(BrushDynamics.maximumCount).")
            }
            self.count = Int(count)
        }
        countJitter = try number(scatter["count_jitter"], "scatter.count_jitter", 0...1) ?? 0
    }
}

extension BrushDynamics {
    /// A stroke's second-tip walk and tip, or nil when none applies: no `dual` in the normalized
    /// dynamics, or the size-1 pixel brush, which takes no dynamics. `seed` is the first tip's,
    /// which the core lifts to a stream of its own (`brush_dual.rs`, THE STREAM).
    static func dualHandle(_ dynamics: BrushDynamics?, size: CGFloat,
                           seed: UInt32) throws -> (handle: RasterBrushDual, tip: BrushTip)? {
        guard let dual = dynamics?.normalized?.dual, size > 1 else { return nil }
        return (try RasterBrushDual(request: dual.coreRequest(seed: seed)), dual.tip)
    }
}

/// A stroke's second tip as recorded: its tip and its resolved dabs (x, y, diameter, deposit
/// quadruples, as the first list's), which every rasterization of the stroke multiplies in.
struct StrokeDual {
    let tip: BrushTip
    let dabs: [Double]

    /// Refuses (`NativeBrushStroke.dualRefusal`) a tip outside the native brush request's ranges
    /// or aliased, and a dab list that is not 1…50,000 finite quadruples with each centre within
    /// ±1,000,000, each diameter 1…5000 and each deposit 0…1.
    init(tip: BrushTip, dabs: [Double]) throws {
        let count = dabs.count / 4
        guard !tip.aliased, tip.hardness.isFinite, (0...1).contains(tip.hardness),
              tip.roundness.isFinite, (0.01...1).contains(tip.roundness),
              tip.angleDegrees.isFinite, (-180...180).contains(tip.angleDegrees),
              dabs.count % 4 == 0, (1...NativeBrushStroke.maximumDabs).contains(count),
              stride(from: 0, to: dabs.count, by: 4).allSatisfy({
                  abs(dabs[$0]) <= 1_000_000 && abs(dabs[$0 + 1]) <= 1_000_000
                      && (1...NativeBrushStroke.maximumDiameter).contains(dabs[$0 + 2])
                      && (0...1).contains(dabs[$0 + 3])
              }) else {
            throw DoctoredCoreError(message: NativeBrushStroke.dualRefusal)
        }
        self.tip = tip
        self.dabs = dabs
    }

    /// The second list's native brush request: never the pixel brush, never aliased. Its
    /// opacity is not read (the stroke's one rounding is the first request's).
    var request: String {
        get throws {
            try NativeSelectionWork.encoding(["version": 1, "hardness": Double(tip.hardness),
                "roundness": Double(tip.roundness), "angle_degrees": Double(tip.angleDegrees),
                "pixel": false])
        }
    }

    /// `{"hardness", "roundness", "angle_degrees", "dabs"}`, `dabs` the standard base64 of the
    /// little-endian binary64 quadruples: the canonical stroke's `dual`.
    var json: [String: Any] {
        var data = Data(); data.reserveCapacity(dabs.count * 8)
        for value in dabs {
            var bits = value.bitPattern.littleEndian
            withUnsafeBytes(of: &bits) { data.append(contentsOf: $0) }
        }
        return ["hardness": Double(tip.hardness), "roundness": Double(tip.roundness),
                "angle_degrees": Double(tip.angleDegrees), "dabs": data.base64EncodedString()]
    }

    /// Strict parse of `json`'s form; every refusal is `NativeBrushStroke.dualRefusal`.
    init(json raw: Any) throws {
        let refusal = DoctoredCoreError(message: NativeBrushStroke.dualRefusal)
        guard let object = raw as? [String: Any],
              Set(object.keys) == ["hardness", "roundness", "angle_degrees", "dabs"],
              let hardness = NativeBrushStroke.jsonNumber(object["hardness"]),
              let roundness = NativeBrushStroke.jsonNumber(object["roundness"]),
              let angle = NativeBrushStroke.jsonNumber(object["angle_degrees"]),
              let text = object["dabs"] as? String,
              text.utf8.count <= NativeBrushStroke.maximumDabs * 44 else { throw refusal }
        _ = try RasterResources.shared.admit(scratch: UInt64(text.utf8.count) * 3 + 4096)
        guard let data = Data(base64Encoded: text), !data.isEmpty, data.count % 32 == 0 else {
            throw refusal
        }
        var values = [Double](); values.reserveCapacity(data.count / 8)
        data.withUnsafeBytes { bytes in
            for i in stride(from: 0, to: data.count, by: 8) {
                values.append(Double(bitPattern: UInt64(littleEndian:
                    bytes.loadUnaligned(fromByteOffset: i, as: UInt64.self))))
            }
        }
        try self.init(tip: BrushTip(hardness: CGFloat(hardness), flow: 1,
                                    angleDegrees: CGFloat(angle), roundness: CGFloat(roundness)),
                      dabs: values)
    }
}

extension EditorTool {
    /// The tools whose dynamics take a dual brush: the Brush and the Eraser. The Pencil's dab is
    /// two-valued and a second field would give it a third value, so its `dynamics.dual` is
    /// pinned nil (`ToolOptionsStore.pinnedDynamics`) and its sheet shows no Dual Brush rows.
    var takesDualBrushDynamics: Bool { [.brush, .eraser].contains(self) }
}
