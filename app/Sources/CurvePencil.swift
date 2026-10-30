import Foundation

/// The Curves dialog's pencil: drawing a 256-entry map (the map form the core already
/// renders, saves to PSD and takes over MCP — `rgb_map`… on RGB, `cmyk_map`… on ink), Smooth, and
/// a point curve as a map. Pure functions over `[Int]` maps (entry i = output code for input i);
/// `CurveEditorView` draws with them and the dialog converts with them.
enum CurvePencil {
    /// `map` with one pencil sample `to` written, joined to the previous sample `from` (nil on
    /// the press): every integer input between the two gets the straight line's output there,
    /// rounded and clamped to 0…255, so a fast drag leaves no gap. A lone press writes one
    /// entry. Inputs outside 0…255 are clamped first.
    static func drawn(_ map: [Int], from: (x: Int, y: Int)?, to: (x: Int, y: Int)) -> [Int] {
        guard map.count == 256 else { return map }
        let clamp = { (v: Int) in min(max(v, 0), 255) }
        let end = (x: clamp(to.x), y: clamp(to.y))
        let start = from.map { (x: clamp($0.x), y: clamp($0.y)) } ?? end
        var out = map
        if start.x == end.x {
            out[end.x] = end.y
            return out
        }
        let span = Double(end.x - start.x)
        for x in min(start.x, end.x)...max(start.x, end.x) {
            let t = Double(x - start.x) / span
            let y = Double(start.y) + t * Double(end.y - start.y)
            out[x] = clamp(Int(y.rounded()))
        }
        return out
    }

    /// Smooth: entries 0 and 255 kept; every other entry the rounded mean of the entries from
    /// `i − 2` to `i + 2` (the run clipped to 0…255 at the ends).
    static func smoothed(_ map: [Int]) -> [Int] {
        guard map.count == 256 else { return map }
        var out = map
        for i in 1..<255 {
            let run = map[max(i - 2, 0)...min(i + 2, 255)]
            out[i] = Int((Double(run.reduce(0, +)) / Double(run.count)).rounded())
        }
        return out
    }

    /// A point curve as a map, as the core's byte table samples it (`spline_lut`): the natural
    /// spline (`NaturalCurve`, its display twin) at every input code, rounded. The identity point
    /// list is the identity map.
    static func map(fromPoints points: [CurvePoint]) -> [Int] {
        guard points != CurvePoint.identity else { return CurvesMeta.identityMap }
        let curve = NaturalCurve(points: points)
        return (0...255).map { Int(curve.value(at: Double($0)).rounded()) }
    }
}
