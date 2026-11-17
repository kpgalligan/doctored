import AppKit

// The selection's rect read (`CanvasSelection.maskSamples16(in:)`, a stroke's `coverage_rect`)
// against the canvas read cropped to the same rect, row by row, for every shape kind: a
// geometric shape (quarter-pixel, decimal and integral rects, an ellipse, a concave polygon; the
// last two read the canvas bytes' rows, `maskSamples16(in:)`'s doc says why), retained byte
// coverage and native u16 coverage. Three canvases: 97 × 61, and 30,000 × 64 and 64 × 30,000,
// whose shapes sit where the canvas read's device coordinates are largest (the right edge, and
// the top rows, which its flip puts at device y ≈ 30,000). The equality is exact; the rect read's
// refusals and the canvas form (`in: nil`) are checked beside it, and each geometric kind is
// shown to hold a fractional sample in the rect named for it, so the edges are compared.

private struct RectReadCanvas {
    let name: String
    let width: Int
    let height: Int
    let shapes: [(String, CanvasSelection.Shape)]
    let rects: [CoverageRect]
    /// The rect each geometric kind must hold a fractional sample in.
    let strength: CoverageRect
}

private func rectReadHash(_ index: Int, _ salt: UInt32) -> UInt32 {
    var v = UInt32(truncatingIfNeeded: index) &* 0x9E37_79B1 &+ salt &* 0xC2B2_AE3D
    v ^= v >> 15
    v = v &* 0x2C1B_3C6D
    v ^= v >> 12
    return v
}

/// The probe's own crop of a canvas plane: `rect`'s rows, row-major.
private func rectReadCrop(_ plane: [UInt16], width: Int, _ rect: CoverageRect) -> [UInt16] {
    var out = [UInt16]()
    for y in rect.y..<(rect.y + rect.height) {
        for x in rect.x..<(rect.x + rect.width) { out.append(plane[y * width + x]) }
    }
    return out
}

/// The 64 × 30,000 twin of a 30,000 × 64 point: x and y swapped and the long axis turned, so
/// a shape at the wide canvas's right edge sits at the tall canvas's top.
private func rectReadTurnedPoint(_ point: CGPoint) -> CGPoint {
    CGPoint(x: point.y, y: 30_000 - point.x)
}

private func rectReadTurnedShape(_ rect: CGRect) -> CGRect {
    CGRect(x: rect.minY, y: 30_000 - rect.maxX, width: rect.height, height: rect.width)
}

private func rectReadTurnedArea(_ rect: CoverageRect) -> CoverageRect {
    (rect.y, 30_000 - rect.x - rect.width, rect.height, rect.width)
}

private func rectReadCanvases() throws -> [RectReadCanvas] {
    let (sw, sh) = (97, 61)
    var bytes = (0..<(sw * sh)).map { UInt8(truncatingIfNeeded: rectReadHash($0, 1) >> 16) }
    // Every byte value at least once, scattered (23 is prime to 97 × 61).
    for value in 0..<256 { bytes[(value * 23) % bytes.count] = UInt8(value) }
    var samples = (0..<(sw * sh)).map { UInt16(truncatingIfNeeded: rectReadHash($0, 2) >> 16) }
    for (at, value) in [(0, 0), (sw + 1, 1), (sw * 30 + 40, 65534), (sw * 60 + 96, 65535)] {
        samples[at] = UInt16(value)
    }
    try require(Set(bytes).count == 256, "[selection rect read] the byte mask holds every value")
    let small = RectReadCanvas(
        name: "97x61", width: sw, height: sh,
        shapes: [
            ("rect", .rect(CGRect(x: 10.25, y: 7.5, width: 40.5, height: 30.75))),
            ("integral rect", .rect(CGRect(x: 20, y: 10, width: 30, height: 25))),
            // Edges no binary fraction holds exactly (the rect above has quarter-pixel edges).
            ("decimal rect", .rect(CGRect(x: 10.3, y: 7.7, width: 40.1, height: 30.9))),
            ("ellipse", .ellipse(CGRect(x: 12.3, y: 8.7, width: 60.1, height: 41.9))),
            ("polygon", .polygon([CGPoint(x: 5.5, y: 3.25), CGPoint(x: 90.75, y: 12.5),
                                  CGPoint(x: 48.2, y: 57.9), CGPoint(x: 30, y: 25),
                                  CGPoint(x: 8.1, y: 50.3)])),
            ("mask", .mask(bytes)),
            ("mask16", .mask16(samples)),
        ],
        rects: [(0, 0, 97, 61), (0, 0, 1, 1), (13, 9, 21, 17), (8, 6, 35, 28), (92, 57, 5, 4),
                (40, 0, 1, 61), (0, 30, 97, 1)],
        strength: (8, 6, 35, 28))
    let wideRect = CGRect(x: 29_900.25, y: 7.5, width: 80.5, height: 40.75)
    let wideEllipse = CGRect(x: 29_870.3, y: 4.7, width: 120.1, height: 50.9)
    let widePolygon = [CGPoint(x: 29_850.5, y: 3.25), CGPoint(x: 29_997.75, y: 12.5),
                       CGPoint(x: 29_948.2, y: 60.9), CGPoint(x: 29_930, y: 25),
                       CGPoint(x: 29_858.1, y: 50.3)]
    let wideRects: [CoverageRect] = [(29_850, 0, 150, 64), (29_903, 9, 41, 37),
                                     (29_999, 0, 1, 64), (29_900, 63, 100, 1)]
    let wide = RectReadCanvas(
        name: "30000x64", width: 30_000, height: 64,
        shapes: [("rect", .rect(wideRect)), ("ellipse", .ellipse(wideEllipse)),
                 ("polygon", .polygon(widePolygon))],
        rects: wideRects, strength: wideRects[0])
    let tall = RectReadCanvas(
        name: "64x30000", width: 64, height: 30_000,
        shapes: [("rect", .rect(rectReadTurnedShape(wideRect))),
                 ("ellipse", .ellipse(rectReadTurnedShape(wideEllipse))),
                 ("polygon", .polygon(widePolygon.map(rectReadTurnedPoint)))],
        rects: wideRects.map(rectReadTurnedArea), strength: rectReadTurnedArea(wideRects[0]))
    return [small, wide, tall]
}

func verifySelectionRectRead() throws {
    var failures = [String]()
    let refusal = "A selection's rect read must name a nonempty rect inside its canvas."
    for canvas in try rectReadCanvases() {
        for (kind, shape) in canvas.shapes {
            let label = "\(canvas.name) \(kind)"
            guard let selection = CanvasSelection(shape: shape, canvasWidth: canvas.width,
                                                  canvasHeight: canvas.height) else {
                failures.append("\(label): the fixture selection is empty")
                continue
            }
            let plane = selection.maskSamples16()
            if try selection.maskSamples16(in: nil) != plane {
                failures.append("\(label): the canvas form differs from maskSamples16()")
            }
            for rect in canvas.rects {
                let got = try selection.maskSamples16(in: rect)
                let want = rectReadCrop(plane, width: canvas.width, rect)
                guard got != want else { continue }
                var first = "count \(got.count) vs \(want.count)"
                if got.count == want.count,
                   let i = got.indices.first(where: { got[$0] != want[$0] }) {
                    first = "(\(rect.x + i % rect.width), \(rect.y + i / rect.width)) "
                        + "got \(got[i]) want \(want[i])"
                }
                failures.append("\(label) \(rect): \(first)")
            }
            let geometric: Bool
            switch shape {
            case .rect, .ellipse, .polygon: geometric = true
            case .mask, .mask16: geometric = false
            }
            if geometric, kind != "integral rect" {
                let crop = rectReadCrop(plane, width: canvas.width, canvas.strength)
                if !crop.contains(where: { $0 > 0 && $0 < 65535 }) {
                    failures.append("\(label): no fractional sample in \(canvas.strength)")
                }
            }
            guard canvas.width == 97 else { continue }
            for rect: CoverageRect in [(95, 0, 3, 1), (0, 0, 0, 1), (-1, 0, 2, 2)] {
                do {
                    _ = try selection.maskSamples16(in: rect)
                    failures.append("\(label) \(rect): read instead of refusing")
                } catch let error as DoctoredCoreError where error.message == refusal {
                } catch {
                    failures.append("\(label) \(rect): refused with \(error)")
                }
            }
        }
    }
    guard failures.isEmpty else {
        throw ProbeFailure(description: "[selection rect read] \(failures.count) failing: "
            + failures.joined(separator: "; "))
    }
    print("PASS: selection rect read — maskSamples16(in:) equals the canvas read cropped to the "
        + "rect for rect, ellipse, polygon, byte and u16 selections on a 97 × 61 canvas and at "
        + "the far edge of 30,000-wide and 30,000-tall canvases; nil reads the canvas, and an "
        + "empty or outside rect refuses")
}
