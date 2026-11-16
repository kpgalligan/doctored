import AppKit

// Distance along one subpath and outline maps (`dr_path_length`, `dr_path_frames`,
// `dr_outline_map`; core `path_along.rs` / `outline_map.rs`): what text on a path places its
// glyphs and bends its decorations by (ArtworkTextOnPath.swift).

/// A point on a subpath and the unit tangent there, in the direction of travel.
struct PathFrame: Equatable {
    var point: CGPoint
    var tangent: CGVector
}

extension RasterPathCore {
    /// The subpath's index as the exports take it.
    private static func subpathIndex(_ subpath: Int) throws -> UInt32 {
        guard let index = UInt32(exactly: subpath) else {
            throw DoctoredCoreError(message: "The path has no subpath \(subpath).")
        }
        return index
    }

    /// The length of subpath `subpath` of `shape`, its closing segment included, measured
    /// over its flattening at 1/64 px. Throws for no such subpath or one with no length.
    static func pathLength(_ shape: PathShape, subpath: Int) throws -> Double {
        let index = try subpathIndex(subpath)
        let json = try text(shape)
        var error: UnsafeMutablePointer<CChar>?
        let length = dr_path_length(json, index, &error)
        guard length >= 0 else {
            throw DoctoredCoreError(
                message: takeErrorMessage(error, fallback: "Could not measure the path."))
        }
        return length
    }

    /// The frame at each distance along subpath `subpath` (from its other end when
    /// `reversed`): a closed subpath wraps; nil where an open one has no frame (past either
    /// end, or a distance that is not finite). One call for the whole list.
    static func pathFrames(
        _ shape: PathShape, subpath: Int, reversed: Bool, distances: [Double]
    ) throws -> [PathFrame?] {
        let index = try subpathIndex(subpath)
        guard !distances.isEmpty else { return [] }
        let json = try text(shape)
        var values = [Double](repeating: 0, count: distances.count * 4)
        var error: UnsafeMutablePointer<CChar>?
        let ok = distances.withUnsafeBufferPointer { wanted in
            values.withUnsafeMutableBufferPointer { frames in
                dr_path_frames(json, index, reversed, wanted.baseAddress, wanted.count,
                               frames.baseAddress, frames.count, &error)
            }
        }
        guard ok else {
            throw DoctoredCoreError(
                message: takeErrorMessage(error, fallback: "Could not frame the path."))
        }
        return stride(from: 0, to: values.count, by: 4).map { i in
            guard values[i..<(i + 4)].allSatisfy({ $0.isFinite }) else { return nil }
            return PathFrame(point: CGPoint(x: values[i], y: values[i + 1]),
                             tangent: CGVector(dx: values[i + 2], dy: values[i + 3]))
        }
    }

    /// `path` bent along subpath `subpath` of `shape` (`dr_outline_map`'s "along" map): a
    /// point (x, y) goes to `F.point + up·(baseline − y + shift)`, `F` the frame at
    /// `start + x` continued straight past an open subpath's ends and `up` its tangent turned
    /// a quarter toward screen-up. Every segment is split until it lies within `tolerance`
    /// px of the mapped curve; the result holds moves, lines and closes.
    static func mapOutline(
        _ path: CGPath, along shape: PathShape, subpath: Int, reversed: Bool,
        start: Double, baseline: Double, shift: Double, tolerance: Double
    ) throws -> CGPath {
        guard [start, baseline, shift, tolerance].allSatisfy({ $0.isFinite }), shape.isFinite else {
            throw DoctoredCoreError(message: "An outline map number is not finite.")
        }
        var verbs: [UInt8] = []
        var coordinates: [Double] = []
        path.applyWithBlock { element in
            let points = element.pointee.points
            let count: Int
            switch element.pointee.type {
            case .moveToPoint: verbs.append(0); count = 1
            case .addLineToPoint: verbs.append(1); count = 1
            case .addQuadCurveToPoint: verbs.append(2); count = 2
            case .addCurveToPoint: verbs.append(3); count = 3
            case .closeSubpath: verbs.append(4); count = 0
            @unknown default: verbs.append(255); count = 0
            }
            for k in 0..<count {
                coordinates.append(Double(points[k].x))
                coordinates.append(Double(points[k].y))
            }
        }
        let map: [String: Any] = [
            "kind": "along", "shape": shape.jsonObject, "subpath": subpath, "reversed": reversed,
            "start": start, "baseline": baseline, "shift": shift,
        ]
        let json = String(decoding: try JSONSerialization.data(withJSONObject: map), as: UTF8.self)
        var error: UnsafeMutablePointer<CChar>?
        let result = verbs.withUnsafeBufferPointer { commands in
            coordinates.withUnsafeBufferPointer { points in
                dr_outline_map(commands.baseAddress, commands.count, points.baseAddress,
                               points.count / 2, json, tolerance, &error)
            }
        }
        guard let result else {
            throw DoctoredCoreError(
                message: takeErrorMessage(error, fallback: "Could not map the outline."))
        }
        defer { dr_string_free(result) }
        let parsed = try JSONSerialization.jsonObject(with: Data(String(cString: result).utf8))
        guard let object = parsed as? [String: Any], let commands = object["commands"] as? [NSNumber],
              let points = object["points"] as? [[NSNumber]]
        else { throw DoctoredCoreError(message: "Invalid mapped outline.") }
        let out = CGMutablePath()
        var next = 0
        func point() throws -> CGPoint {
            guard next < points.count, points[next].count == 2 else {
                throw DoctoredCoreError(message: "Invalid mapped outline.")
            }
            defer { next += 1 }
            return CGPoint(x: points[next][0].doubleValue, y: points[next][1].doubleValue)
        }
        for command in commands {
            switch command.intValue {
            case 0: out.move(to: try point())
            case 1: out.addLine(to: try point())
            case 4: out.closeSubpath()
            default: throw DoctoredCoreError(message: "Invalid mapped outline.")
            }
        }
        return out
    }
}
