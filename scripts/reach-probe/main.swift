// Auto-Align's registration reach on real-photo views, and its
// false-accept rate on unrelated pairs. Built by run.py with the app sources
// (minus app main.swift), -O, whole module, against the release core. It calls
// `AutoAlign.register` directly: no app launch, no socket. One JSON line per
// row on stdout.
//
//   probe <photo.jpg>
//
// THE PHOTO is core/tests/fixtures/registration_photo/sky.jpg (CC0, 2048 ×
// 1366), drawn to 8-bit DeviceGray. It is wrapped on a CYLINDER at 1 px per
// unit: cylinder (u, v) is photo point (1024 + u, 450 + v), sampled bilinearly
// with pixel i's centre at i + 0.5 (clamped at the edges).
//
// A VIEW yawed ψ, W × H, f 600: its pixel (x, y) (centre x + 0.5, y + 0.5;
// xc = x + 0.5 − W/2, yc = y + 0.5 − H/2) shows cylinder point
// (f(ψ + atan2(xc, f)), f·yc / hypot(xc, f)) — a pinhole camera turned ψ about
// the vertical axis inside a cylinder of radius f. A pair is the REFERENCE at
// yaw −Δ/2 and the FLOATING view at +Δ/2. Views are opaque sRGB RGBX frames,
// grey in all three channels, as `AutoAlign.analysisFrame` draws them.
//
// THE LENS (the core's convention): an ideal point u shows at
// p = D(u) = c + R q (1 + k|q|²), q = (u − c)/R, c the frame centre,
// R = max(W, H)/2, k −0.05; a lens view's pixel p shows the ideal view at
// D⁻¹(p) (Newton). Both views of a lens row carry it.
//
// THE TRUTH T maps a floating point to the reference: T(p) = D(H_true(D⁻¹(p)))
// (D the identity without the lens), H_true the rotation by Δ about the
// vertical axis through f (the pinhole relation), or a planar row's own shift
// and rotation. D is one-to-one only where 1 + 3k|q|² > 0 (|q| < 2.58 at
// k −0.05); an ideal reference point past that has no truth (the model would
// fold it back into the frame).
//
// ROWS.
//   Family A: 600 × 400, Δ = 5…40° in 5° steps (overlap 91 → 25 %).
//   Family B: Δ ∈ {20, 30, 40}° × overlap o ∈ {15, 30, 50, 70} %,
//     W = round(2f·tan(Δ / (2(1 − o)))), H = min(round(2W/3), 800); a W past
//     2048 is omitted (40°/70 %).
//   Each yaw row runs `.perspective`.
//   Planar: 600 × 400 crops of the photo itself (no cylinder), the reference
//     at (724, 250), the floating crop shifted ((1 − o)·600, 20), o as above;
//     Reposition rows (`.translation`, a pure shift) and Collage rows
//     (`.collage`, the floating crop also turned +5° about its centre:
//     floating pixel p shows photo point Rot(p − c) + c + its origin).
//   Every row runs with and without the lens.
//
// ERROR: the worst |H_est(p) − T(p)| over a 33 × 33 grid of floating points
// p = (0.5 + i(W − 1)/32, 0.5 + j(H − 1)/32) whose T(p) lands in the
// reference frame [0, W] × [0, H]. A point H_est cannot map counts as
// infinite.
//
// BOUNDS, fixed before the baseline run:
//   - A plain row (no lens): within 2.0 px.
//   - A lens row's FLOOR is the worst grid residual against T of the
//     homography fitted to the grid's (p, T(p)) pairs by normalized DLT (this
//     file's own linear least squares): two views that both carry k −0.05
//     are not related by a homography, so no estimate can do better than
//     about this. A lens row with floor + 3.0 (the RANSAC inlier threshold)
//     ≤ 8.0 is a MUST-REGISTER row with bound 8.0 (Geometric Distortion's
//     ±8-step pass-1 search at s = 1). A lens row with floor + 3.0 > 8.0 is a
//     FLOOR row: a refusal passes, and an answer passes only within
//     floor + 3.0.
//   - An answer past its bound is a WRONG ACCEPT.
//   - FALSE-ACCEPT rows, each under `.perspective`, `.collage` and
//     `.translation`, no lens: 12 disjoint crop pairs (4 each at 400 × 267,
//     600 × 400 and 900 × 600, from the photo's corners), each Family A
//     reference against its left-right mirror, 600 × 400 views at Δ 60° and
//     90° (they share no pixels: the view spans 53.13°), and the yaw-0 view
//     against flat mid-grey. Any answer is a FALSE ACCEPT.
//
// A row's verdict: `registered` (within its bound), `refused`, `wrong`
// (a WRONG ACCEPT) or `false-accept`. A refusal records its `reason`
// (`AutoAlign.registered`).
//
// GEOMETRIC DISTORTION (ruling 2, fixed before the after-3 run). Every lens
// row runs twice: GD OFF (`<id>`, `register(distortion:
// false)`, judged as above) and GD ON (`<id>-gd`): `register(distortion:
// true)` (for Collage and Reposition as Perspective, as the planar lens path
// registers since after-3: `AutoAlignPlanarLens.swift`), then 2b-4's path, the core's `fit_distortion` over a two-layer
// document (the reference layer 0, the floating view layer 1 with the
// registration's map). Its answer is D_k ∘ M ∘ D_k⁻¹: M the fitted map between
// the corrected frames (reduced by `AutoAlign.planarMap` for Collage and
// Reposition, as the planar lens path does), D_k the fitted coefficient about
// each frame's centre. It is judged against the row's own bound on the same
// grid; a fit refusal is a refusal. The false-accept pairs also run GD ON
// (`<id>-gd`), judged the same way: any answer is a false accept.
//
// DIAGNOSTIC (added after the baseline, so it needs phase 3a's sources): each
// row also records the feature step alone (`AutoAlign.featureEstimate`, not
// vouched for): `feature_error` against the truth, `feature_usable`
// (`Homography.isUsable`) and `feature_gate` (`AutoAlign.coversOverlap`, the
// coverage and residual gate). It decides no verdict. It also
// records `feature_usable_overlap` (`isUsable` over `Homography.overlap(in:)`,
// the part of the frame the map carries into it) and, on the false-accept
// rows too, `feature_lens` (`AutoAlign.carriesLens`).

import AppKit
import Foundation

let focal = 600.0
let lensK = -0.05
let plainBound = 2.0
let lensBound = 8.0
let inlierThreshold = 3.0

func emit(_ fields: [String: Any]) {
    // JSON has no infinity: a non-finite number is written as −1.
    let fields = fields.mapValues { value -> Any in
        if let number = value as? Double, !number.isFinite { return -1.0 }
        return value
    }
    guard let data = try? JSONSerialization.data(withJSONObject: fields, options: [.sortedKeys]),
          let text = String(data: data, encoding: .utf8) else { return }
    print(text)
    fflush(stdout)
}

func fail(_ message: String) -> Never {
    emit(["error": message])
    exit(2)
}

struct Grey {
    let width: Int
    let height: Int
    let samples: [Float]

    /// Bilinear at the continuous point (x, y), pixel i's centre at i + 0.5,
    /// clamped at the edges.
    func sample(_ x: Double, _ y: Double) -> Float {
        let fx = min(max(x - 0.5, 0), Double(width - 1))
        let fy = min(max(y - 0.5, 0), Double(height - 1))
        let x0 = min(Int(fx), width - 2), y0 = min(Int(fy), height - 2)
        let tx = Float(fx - Double(x0)), ty = Float(fy - Double(y0))
        let i = y0 * width + x0
        let top = samples[i] + (samples[i + 1] - samples[i]) * tx
        let bottom = samples[i + width] + (samples[i + width + 1] - samples[i + width]) * tx
        return top + (bottom - top) * ty
    }
}

func loadPhoto(_ path: String) -> Grey {
    guard let source = CGImageSourceCreateWithURL(URL(fileURLWithPath: path) as CFURL, nil),
          let image = CGImageSourceCreateImageAtIndex(source, 0, nil) else {
        fail("could not decode \(path)")
    }
    let width = image.width, height = image.height
    var bytes = [UInt8](repeating: 0, count: width * height)
    let drawn = bytes.withUnsafeMutableBytes { buffer -> Bool in
        guard let context = CGContext(
            data: buffer.baseAddress, width: width, height: height, bitsPerComponent: 8,
            bytesPerRow: width, space: CGColorSpaceCreateDeviceGray(),
            bitmapInfo: CGImageAlphaInfo.none.rawValue) else { return false }
        context.draw(image, in: CGRect(x: 0, y: 0, width: width, height: height))
        return true
    }
    guard drawn else { fail("could not draw \(path)") }
    return Grey(width: width, height: height, samples: bytes.map(Float.init))
}

/// An opaque sRGB RGBX frame of `width × height` grey values.
func frame(_ values: [Float], width: Int, height: Int) -> CGImage {
    var bytes = [UInt8](repeating: 255, count: width * height * 4)
    for i in 0..<(width * height) {
        let g = UInt8(min(max(values[i].rounded(), 0), 255))
        bytes[4 * i] = g
        bytes[4 * i + 1] = g
        bytes[4 * i + 2] = g
    }
    guard let provider = CGDataProvider(data: Data(bytes) as CFData),
          let image = CGImage(
              width: width, height: height, bitsPerComponent: 8, bitsPerPixel: 32,
              bytesPerRow: width * 4, space: ColorProfile.sRGB,
              bitmapInfo: CGBitmapInfo(rawValue: CGImageAlphaInfo.noneSkipLast.rawValue),
              provider: provider, decode: nil, shouldInterpolate: false,
              intent: .defaultIntent) else { fail("could not make a frame") }
    return image
}

// MARK: - The lens

struct Lens {
    let cx: Double, cy: Double, radius: Double, k: Double

    init(width: Int, height: Int, k: Double) {
        cx = Double(width) / 2
        cy = Double(height) / 2
        radius = Double(max(width, height)) / 2
        self.k = k
    }

    /// D(u): an ideal point to where it shows.
    func distort(_ p: (Double, Double)) -> (Double, Double) {
        let qx = (p.0 - cx) / radius, qy = (p.1 - cy) / radius
        let s = 1 + k * (qx * qx + qy * qy)
        return (cx + radius * qx * s, cy + radius * qy * s)
    }

    /// Whether D is one-to-one at the ideal point `u`: 1 + 3k|q|² > 0, the
    /// radial map's derivative.
    func isOneToOne(_ u: (Double, Double)) -> Bool {
        let qx = (u.0 - cx) / radius, qy = (u.1 - cy) / radius
        return 1 + 3 * k * (qx * qx + qy * qy) > 0
    }

    /// D⁻¹(p): Newton on the radius, r_d = r_u(1 + k r_u²).
    func undistort(_ p: (Double, Double)) -> (Double, Double) {
        let qx = (p.0 - cx) / radius, qy = (p.1 - cy) / radius
        let rd = (qx * qx + qy * qy).squareRoot()
        guard rd > 0 else { return p }
        var ru = rd
        for _ in 0..<30 {
            let f = ru * (1 + k * ru * ru) - rd
            let df = 1 + 3 * k * ru * ru
            ru -= f / df
        }
        let s = ru / rd
        return (cx + radius * qx * s, cy + radius * qy * s)
    }
}

// MARK: - Geometry

/// A pair's geometry: how a view pixel finds its photo point, and the truth.
struct Pair {
    let width: Int
    let height: Int
    /// Ideal floating point → ideal reference point; nil behind the camera.
    let ideal: ((Double, Double)) -> (Double, Double)?
    /// Ideal view point → photo point, for the reference and the floating view.
    let referenceSource: ((Double, Double)) -> (Double, Double)
    let floatingSource: ((Double, Double)) -> (Double, Double)
    let lens: Lens?

    func truth(_ p: (Double, Double)) -> (Double, Double)? {
        let u = lens.map { $0.undistort(p) } ?? p
        guard let t = ideal(u) else { return nil }
        guard let lens else { return t }
        // D is one-to-one only while 1 + 3k|q|² > 0; past it the model
        // folds a far ideal point back into the frame, which no lens shows.
        guard lens.isOneToOne(t) else { return nil }
        return lens.distort(t)
    }

    func render(_ photo: Grey, floating: Bool) -> [Float] {
        var out = [Float](repeating: 0, count: width * height)
        let source = floating ? floatingSource : referenceSource
        for y in 0..<height {
            for x in 0..<width {
                let p = (Double(x) + 0.5, Double(y) + 0.5)
                let u = lens.map { $0.undistort(p) } ?? p
                let s = source(u)
                out[y * width + x] = photo.sample(s.0, s.1)
            }
        }
        return out
    }
}

func degrees(_ d: Double) -> Double { d * .pi / 180 }

/// Ideal view point of a view yawed `psi` → photo point (the cylinder).
func cylinder(width: Int, height: Int, psi: Double) -> ((Double, Double)) -> (Double, Double) {
    { p in
        let xc = p.0 - Double(width) / 2, yc = p.1 - Double(height) / 2
        let u = focal * (psi + atan2(xc, focal))
        let v = focal * yc / hypot(xc, focal)
        return (1024 + u, 450 + v)
    }
}

func yawPair(width: Int, height: Int, delta: Double, lens: Bool) -> Pair {
    let a = degrees(delta)
    let w = Double(width), h = Double(height)
    return Pair(
        width: width, height: height,
        ideal: { p in
            let xc = p.0 - w / 2, yc = p.1 - h / 2
            let x = xc * cos(a) + focal * sin(a)
            let z = -xc * sin(a) + focal * cos(a)
            guard z > 1e-9 else { return nil }
            return (focal * x / z + w / 2, focal * yc / z + h / 2)
        },
        referenceSource: cylinder(width: width, height: height, psi: -a / 2),
        floatingSource: cylinder(width: width, height: height, psi: a / 2),
        lens: lens ? Lens(width: width, height: height, k: lensK) : nil)
}

/// A planar crop pair: the reference at `origin`, the floating crop at
/// `origin + shift`, turned `turn` degrees about its centre.
func planarPair(origin: (Double, Double), shift: (Double, Double), turn: Double,
                lens: Bool) -> Pair {
    let width = 600, height = 400
    let cx = Double(width) / 2, cy = Double(height) / 2
    let c = cos(degrees(turn)), s = sin(degrees(turn))
    func turned(_ p: (Double, Double)) -> (Double, Double) {
        let dx = p.0 - cx, dy = p.1 - cy
        return (c * dx - s * dy + cx, s * dx + c * dy + cy)
    }
    return Pair(
        width: width, height: height,
        ideal: { p in
            let q = turned(p)
            return (q.0 + shift.0, q.1 + shift.1)
        },
        referenceSource: { p in (p.0 + origin.0, p.1 + origin.1) },
        floatingSource: { p in
            let q = turned(p)
            return (q.0 + origin.0 + shift.0, q.1 + origin.1 + shift.1)
        },
        lens: lens ? Lens(width: width, height: height, k: lensK) : nil)
}

func grid(width: Int, height: Int) -> [(Double, Double)] {
    var points: [(Double, Double)] = []
    for j in 0...32 {
        for i in 0...32 {
            points.append((0.5 + Double(i) * Double(width - 1) / 32,
                           0.5 + Double(j) * Double(height - 1) / 32))
        }
    }
    return points
}

/// The grid's (p, T(p)) pairs whose truth lands in the reference frame.
func truthPairs(_ pair: Pair) -> [((Double, Double), (Double, Double))] {
    grid(width: pair.width, height: pair.height).compactMap { p in
        guard let t = pair.truth(p), t.0 >= 0, t.0 <= Double(pair.width), t.1 >= 0,
              t.1 <= Double(pair.height) else { return nil }
        return (p, t)
    }
}

func apply(_ m: [Double], _ p: (Double, Double)) -> (Double, Double)? {
    let w = m[6] * p.0 + m[7] * p.1 + m[8]
    guard w.isFinite, w > 0 else { return nil }
    let x = (m[0] * p.0 + m[1] * p.1 + m[2]) / w, y = (m[3] * p.0 + m[4] * p.1 + m[5]) / w
    guard x.isFinite, y.isFinite else { return nil }
    return (x, y)
}

func worstError(_ m: [Double], _ pairs: [((Double, Double), (Double, Double))]) -> Double {
    var worst = 0.0
    for (p, t) in pairs {
        guard let q = apply(m, p) else { return .infinity }
        worst = max(worst, hypot(q.0 - t.0, q.1 - t.1))
    }
    return worst
}

// MARK: - The floor's normalized DLT (this probe's own)

/// Hartley's normalisation: centroid to 0, mean distance √2.
func normalisation(_ points: [(Double, Double)]) -> [Double] {
    let n = Double(points.count)
    let mx = points.map(\.0).reduce(0, +) / n, my = points.map(\.1).reduce(0, +) / n
    let mean = points.map { hypot($0.0 - mx, $0.1 - my) }.reduce(0, +) / n
    let s = 2.0.squareRoot() / mean
    return [s, 0, -s * mx, 0, s, -s * my, 0, 0, 1]
}

func multiply(_ a: [Double], _ b: [Double]) -> [Double] {
    var out = [Double](repeating: 0, count: 9)
    for r in 0..<3 {
        for c in 0..<3 {
            for k in 0..<3 { out[r * 3 + c] += a[r * 3 + k] * b[k * 3 + c] }
        }
    }
    return out
}

/// The least-squares homography (h33 = 1) of the normalised points by the
/// 8 × 8 normal equations, Gaussian elimination with partial pivoting.
func dlt(_ pairs: [((Double, Double), (Double, Double))]) -> [Double]? {
    guard pairs.count >= 4 else { return nil }
    let from = normalisation(pairs.map(\.0)), to = normalisation(pairs.map(\.1))
    var a = [[Double]](repeating: [Double](repeating: 0, count: 9), count: 8)
    for (p, q) in pairs {
        guard let u = apply(from, p), let v = apply(to, q) else { return nil }
        let rows: [([Double], Double)] = [
            ([u.0, u.1, 1, 0, 0, 0, -u.0 * v.0, -u.1 * v.0], v.0),
            ([0, 0, 0, u.0, u.1, 1, -u.0 * v.1, -u.1 * v.1], v.1),
        ]
        for (row, value) in rows {
            for r in 0..<8 {
                a[r][8] += row[r] * value
                for c in 0..<8 { a[r][c] += row[r] * row[c] }
            }
        }
    }
    for col in 0..<8 {
        guard let pivot = (col..<8).max(by: { abs(a[$0][col]) < abs(a[$1][col]) }),
              abs(a[pivot][col]) > 1e-300 else { return nil }
        a.swapAt(col, pivot)
        for row in (col + 1)..<8 {
            let f = a[row][col] / a[col][col]
            for c in col...8 { a[row][c] -= f * a[col][c] }
        }
    }
    var h = [Double](repeating: 0, count: 8)
    for row in stride(from: 7, through: 0, by: -1) {
        var sum = a[row][8]
        for k in (row + 1)..<8 { sum -= a[row][k] * h[k] }
        h[row] = sum / a[row][row]
    }
    let fitted = h + [1]
    // H = to⁻¹ · fitted · from; `to` is a similarity, its inverse closed-form.
    let s = to[0]
    let toInverse = [1 / s, 0, -to[2] / s, 0, 1 / s, -to[5] / s, 0, 0, 1]
    let m = multiply(toInverse, multiply(fitted, from))
    guard m[8] != 0 else { return nil }
    return m.map { $0 / m[8] }
}

// MARK: - Rows

func mirrored(_ values: [Float], width: Int, height: Int) -> [Float] {
    var out = values
    for y in 0..<height {
        for x in 0..<width { out[y * width + x] = values[y * width + (width - 1 - x)] }
    }
    return out
}

func registration(_ name: String) -> AutoAlign.Registration {
    switch name {
    case "collage": return .collage
    case "translation": return .translation
    default: return .perspective
    }
}

/// The registration's map and method, or the reason it refused.
func run(_ floating: CGImage, _ reference: CGImage, _ how: String, distortion: Bool)
    -> (answer: (map: Homography, method: String)?, reason: String?)
{
    autoreleasepool {
        switch AutoAlign.registered(floating: floating, reference: reference,
                                    registration: registration(how), distortion: distortion) {
        case let .aligned(map, method): return ((map, method.rawValue), nil)
        case let .refused(reason): return (nil, reason)
        }
    }
}

/// An opaque RGBA raster of `width × height` grey values.
func raster(_ values: [Float], width: Int, height: Int) -> RasterImage? {
    var bytes = [UInt8](repeating: 255, count: width * height * 4)
    for i in 0..<(width * height) {
        let g = UInt8(min(max(values[i].rounded(), 0), 255))
        bytes[4 * i] = g
        bytes[4 * i + 1] = g
        bytes[4 * i + 2] = g
    }
    return RasterImage.from(rgba: bytes, width: width, height: height)
}

/// 2b-4's path after a GD ON registration: the core's `fit_distortion` over
/// the reference (layer 0, the identity) and the floating view (layer 1,
/// `map`), and its answer as a map of floating points, D_k ∘ M ∘ D_k⁻¹ (the
/// header). nil when the fit refuses.
func distortionAnswer(floating: [Float], reference: [Float], width: Int, height: Int,
                      map: Homography, how: String)
    -> (coefficient: Double, transfer: ((Double, Double)) -> (Double, Double)?)?
{
    autoreleasepool {
        let identity = Homography(m: [1, 0, 0, 0, 1, 0, 0, 0, 1])
        guard let bottom = raster(reference, width: width, height: height),
              let top = raster(floating, width: width, height: height),
              let single = RasterDocument.from(image: bottom),
              let stack = single.addingImageLayer(above: 0, top, name: "floating"),
              let budget = try? stack.meshWarpBudget(),
              let request = try? AutoAlign.distortionRequest([(0, 0, identity), (1, 0, map)]),
              let answer = try? stack.fittingDistortion(request: request, budget: budget,
                                                        control: nil),
              let (fit, maps) = try? AutoAlign.distortionFit(answer),
              let refined = maps[1] else { return nil }
        let rect = CGRect(x: 0, y: 0, width: width, height: height)
        let placed: Homography
        switch how {
        case "collage", "translation":
            guard let reduced = AutoAlign.planarMap(refined, registration: registration(how),
                                                    over: rect) else { return nil }
            placed = reduced
        default:
            placed = refined
        }
        let lens = Lens(width: width, height: height, k: fit.coefficient)
        let m = placed.m
        return (fit.coefficient, { p in
            guard let q = apply(m, lens.undistort(p)) else { return nil }
            return lens.distort(q)
        })
    }
}

func worstError(_ transfer: ((Double, Double)) -> (Double, Double)?,
                _ pairs: [((Double, Double), (Double, Double))]) -> Double {
    var worst = 0.0
    for (p, t) in pairs {
        guard let q = transfer(p) else { return .infinity }
        worst = max(worst, hypot(q.0 - t.0, q.1 - t.1))
    }
    return worst
}

func reachRow(id: String, group: String, how: String, pair: Pair, photo: Grey,
              extra: [String: Any]) {
    let started = Date()
    let pairs = truthPairs(pair)
    var fields = extra
    fields["id"] = id
    fields["group"] = group
    fields["registration"] = how
    fields["lens"] = pair.lens != nil
    fields["width"] = pair.width
    fields["height"] = pair.height
    fields["grid_points"] = pairs.count
    let rowClass: String
    let bound: Double
    if pair.lens != nil {
        let floor = dlt(pairs).map { worstError($0, pairs) } ?? .infinity
        fields["floor"] = floor
        if floor + inlierThreshold <= lensBound {
            rowClass = "must"
            bound = lensBound
        } else {
            rowClass = "floor"
            bound = floor + inlierThreshold
        }
    } else {
        rowClass = "plain"
        bound = plainBound
    }
    fields["class"] = rowClass
    fields["bound"] = bound
    let floatingValues = pair.render(photo, floating: true)
    let referenceValues = pair.render(photo, floating: false)
    let floating = frame(floatingValues, width: pair.width, height: pair.height)
    let reference = frame(referenceValues, width: pair.width, height: pair.height)
    // Diagnostic: the feature step alone, unvouched.
    if let feature = autoreleasepool(invoking: {
        AutoAlign.featureEstimate(floating: floating, reference: reference)
    }) {
        let frameRect = CGRect(x: 0, y: 0, width: pair.width, height: pair.height)
        fields["feature_error"] = worstError(feature.map.m, pairs)
        fields["feature_usable"] = feature.map.isUsable(for: frameRect)
        fields["feature_usable_overlap"] = feature.map.overlap(in: frameRect).map {
            feature.map.isUsable(for: $0)
        } ?? false
        fields["feature_gate"] = AutoAlign.coversOverlap(feature, frame: frameRect)
        if let measured = AutoAlign.featureCoverage(feature, frame: frameRect) {
            fields["feature_rms"] = measured.rms
            fields["feature_covered"] = measured.covered
            fields["feature_eligible"] = measured.eligible
        }
        fields["feature_inliers"] = feature.pairs.count
        fields["feature_self_check"] = AutoAlign.crossCheckError(feature.map, on: feature)
        if let departure = feature.departure {
            fields["feature_departure"] = departure
        }
        fields["feature_lens"] = AutoAlign.carriesLens(feature)
        // Where the error is: over the grid points the matches can see (both
        // ends at least `featureEdgeMargin` inside their frames) and the rest.
        let margin = AutoAlign.featureEdgeMargin(width: pair.width, height: pair.height)
        let inner = frameRect.insetBy(dx: margin, dy: margin)
        let seen = pairs.filter {
            inner.contains(CGPoint(x: $0.0.0, y: $0.0.1)) && inner.contains(CGPoint(x: $0.1.0, y: $0.1.1))
        }
        fields["feature_error_seen"] = worstError(feature.map.m, seen)
    }
    var off = fields
    off["distortion"] = false
    let plainRun = run(floating, reference, how, distortion: false)
    if let answer = plainRun.answer {
        let error = worstError(answer.map.m, pairs)
        off["answer"] = answer.method
        off["map"] = answer.map.m
        off["error"] = error.isFinite ? error : -1
        off["verdict"] = error <= bound ? "registered" : "wrong"
    } else {
        off["answer"] = NSNull()
        off["reason"] = plainRun.reason ?? NSNull()
        off["verdict"] = "refused"
    }
    off["seconds"] = Date().timeIntervalSince(started)
    emit(off)
    guard pair.lens != nil else { return }
    // GD ON: the registration, then 2b-4's fit (the header).
    let begun = Date()
    var on = fields
    on["id"] = "\(id)-gd"
    on["distortion"] = true
    // The planar lens path registers as Perspective and reduces after the fit.
    let lensRun = run(floating, reference, group == "planar" ? "perspective" : how,
                      distortion: true)
    if let answer = lensRun.answer {
        on["registration_answer"] = answer.method
        on["registration_error"] = worstError(answer.map.m, pairs)
        if let fitted = distortionAnswer(floating: floatingValues, reference: referenceValues,
                                         width: pair.width, height: pair.height,
                                         map: answer.map, how: how) {
            let error = worstError(fitted.transfer, pairs)
            on["answer"] = answer.method
            on["coefficient"] = fitted.coefficient
            on["error"] = error.isFinite ? error : -1
            on["verdict"] = error <= bound ? "registered" : "wrong"
        } else {
            on["answer"] = NSNull()
            on["reason"] = "the distortion fit refused"
            on["verdict"] = "refused"
        }
    } else {
        on["answer"] = NSNull()
        on["reason"] = lensRun.reason ?? NSNull()
        on["verdict"] = "refused"
    }
    on["seconds"] = Date().timeIntervalSince(begun)
    emit(on)
}

func falseRow(id: String, group: String, floating: CGImage, reference: CGImage) {
    let runs = [false, true].flatMap { distortion in
        ["perspective", "collage", "translation"].map { (distortion, $0) }
    }
    for (distortion, how) in runs {
        let started = Date()
        var fields: [String: Any] = ["id": "\(id)-\(how)\(distortion ? "-gd" : "")",
                                     "group": group, "registration": how,
                                     "lens": false, "width": floating.width,
                                     "height": floating.height, "class": "false",
                                     "distortion": distortion]
        if let feature = autoreleasepool(invoking: {
            AutoAlign.featureEstimate(floating: floating, reference: reference)
        }) {
            let frameRect = CGRect(x: 0, y: 0, width: floating.width, height: floating.height)
            fields["feature_usable"] = feature.map.isUsable(for: frameRect)
            fields["feature_usable_overlap"] = feature.map.overlap(in: frameRect).map {
                feature.map.isUsable(for: $0)
            } ?? false
            fields["feature_gate"] = AutoAlign.coversOverlap(feature, frame: frameRect)
            fields["feature_lens"] = AutoAlign.carriesLens(feature)
        }
        let result = run(floating, reference, how, distortion: distortion)
        if let answer = result.answer {
            fields["answer"] = answer.method
            fields["map"] = answer.map.m
            fields["verdict"] = "false-accept"
        } else {
            fields["answer"] = NSNull()
            fields["reason"] = result.reason ?? NSNull()
            fields["verdict"] = "refused"
        }
        fields["seconds"] = Date().timeIntervalSince(started)
        emit(fields)
    }
}

let arguments = CommandLine.arguments
guard arguments.count == 2 else { fail("usage: probe <photo.jpg>") }
let photo = loadPhoto(arguments[1])
emit(["photo": arguments[1], "width": photo.width, "height": photo.height])

let overlaps = [15, 30, 50, 70]

for lens in [false, true] {
    let tag = lens ? "lens" : "plain"
    for delta in stride(from: 5, through: 40, by: 5) {
        let pair = yawPair(width: 600, height: 400, delta: Double(delta), lens: lens)
        reachRow(id: "A-\(delta)-\(tag)", group: "A", how: "perspective", pair: pair, photo: photo,
                 extra: ["delta": delta])
    }
    for delta in [20, 30, 40] {
        for overlap in overlaps {
            let o = Double(overlap) / 100
            let width = Int((2 * focal * tan(degrees(Double(delta)) / (2 * (1 - o)))).rounded())
            guard width <= 2048 else { continue }
            let height = min(Int((2 * Double(width) / 3).rounded()), 800)
            let pair = yawPair(width: width, height: height, delta: Double(delta), lens: lens)
            reachRow(id: "B-\(delta)-\(overlap)-\(tag)", group: "B", how: "perspective",
                     pair: pair, photo: photo, extra: ["delta": delta, "overlap": overlap])
        }
    }
    for (how, turn) in [("translation", 0.0), ("collage", 5.0)] {
        for overlap in overlaps {
            let shift = ((1 - Double(overlap) / 100) * 600, 20.0)
            let pair = planarPair(origin: (724, 250), shift: shift, turn: turn, lens: lens)
            reachRow(id: "P-\(how)-\(overlap)-\(tag)", group: "planar", how: how, pair: pair,
                     photo: photo, extra: ["overlap": overlap, "turn": turn])
        }
    }
}

// False accepts.
func crop(_ photo: Grey, x: Int, y: Int, width: Int, height: Int) -> CGImage {
    var values = [Float](repeating: 0, count: width * height)
    for row in 0..<height {
        for column in 0..<width {
            values[row * width + column] = photo.samples[(y + row) * photo.width + x + column]
        }
    }
    return frame(values, width: width, height: height)
}

for (w, h) in [(400, 267), (600, 400), (900, 600)] {
    let right = photo.width - w, bottom = photo.height - h
    let pairs: [((Int, Int), (Int, Int))] = [
        ((0, 0), (right, bottom)), ((right, 0), (0, bottom)),
        ((0, 0), (right, 0)), ((0, bottom), (right, bottom)),
    ]
    for (index, (a, b)) in pairs.enumerated() {
        falseRow(id: "F-disjoint-\(w)x\(h)-\(index)", group: "false-disjoint",
                 floating: crop(photo, x: a.0, y: a.1, width: w, height: h),
                 reference: crop(photo, x: b.0, y: b.1, width: w, height: h))
    }
}
for delta in stride(from: 5, through: 40, by: 5) {
    let pair = yawPair(width: 600, height: 400, delta: Double(delta), lens: false)
    let values = pair.render(photo, floating: false)
    falseRow(id: "F-mirror-\(delta)", group: "false-mirror",
             floating: frame(mirrored(values, width: 600, height: 400), width: 600, height: 400),
             reference: frame(values, width: 600, height: 400))
}
for delta in [60, 90] {
    let pair = yawPair(width: 600, height: 400, delta: Double(delta), lens: false)
    falseRow(id: "F-yaw-\(delta)", group: "false-yaw",
             floating: frame(pair.render(photo, floating: true), width: 600, height: 400),
             reference: frame(pair.render(photo, floating: false), width: 600, height: 400))
}
do {
    let pair = yawPair(width: 600, height: 400, delta: 0, lens: false)
    falseRow(id: "F-flat", group: "false-flat",
             floating: frame(pair.render(photo, floating: true), width: 600, height: 400),
             reference: frame([Float](repeating: 128, count: 600 * 400), width: 600, height: 400))
}
emit(["done": true])
