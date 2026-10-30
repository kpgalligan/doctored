import Foundation

private let pixelBound = 2e-6 + 0.5 / 65535.0

private func over(_ destination: [UInt16], ink: [Double], coverage: UInt16, opacity: Double) -> [Double] {
    let sa = ink[3] * Double(coverage) / 65535 * opacity
    let da = Double(destination[3]) / 65535
    let alpha = sa + da * (1 - sa)
    let rgb = (0..<3).map { c in
        alpha > 0 ? (ink[c] * sa + Double(destination[c]) / 65535 * da * (1 - sa)) / alpha : 0
    }
    return rgb + [alpha]
}

func verifyPrecisionPaintTools() throws {
    let source = try openTyped(root().appendingPathComponent("source16.png"))
    let original = try nativeSamples(source)
    var values = [UInt16](repeating: 0, count: 384)
    values[1] = 1
    values[7] = 32768
    let coverage = try PrecisionScalarBuffer(samples: values, width: 32, height: 12)
    let ink = try PrecisionInk(red: 0.123456, green: 0.345678, blue: 0.567891, alpha: 0.654321)
    let command = try PrecisionPixelCommand.paint(layer: 0, ink: ink, space: .document, opacity: 0.375)
    let capture = try PrecisionPixelEdit(source, command: command, input: coverage)
    guard let painted = try capture.run() else { throw ProbeFailure(description: "Paint unexpectedly made no change") }
    let actual = try nativeSamples(painted)
    for i in 0..<384 {
        if values[i] == 0 {
            try require(Array(actual[i * 4..<i * 4 + 4]) == expected(i), "Paint leaves uncovered raw storage exact")
        } else {
            let reference = over(expected(i), ink: ink.rgba, coverage: values[i], opacity: 0.375)
            for c in 0..<4 {
                try require(abs(Double(actual[i * 4 + c]) / 65535 - reference[c]) <= pixelBound,
                            "Typed paint matches independent continuous source-over")
            }
        }
    }
    let erased = try pixelEdit(source, .erase(layer: 0, opacity: 0.25), coverage)
    let erasePixels = try nativeSamples(erased)
    for i in 0..<384 {
        let wantedAlpha = Double(expected(i)[3]) / 65535 * (1 - 0.25 * Double(values[i]) / 65535)
        try require(abs(Double(erasePixels[i * 4 + 3]) / 65535 - wantedAlpha) <= pixelBound,
                    "Typed eraser uses continuous coverage")
        try require(Array(erasePixels[i * 4..<i * 4 + 3]) == Array(expected(i).prefix(3)),
                    "Partial erase preserves straight RGB including latent transparent samples")
    }
    guard let duplicate = try source.duplicateLayer(0) else { throw ProbeFailure(description: "Missing clone target") }
    var cloneValues = [UInt16](repeating: 0, count: 384)
    cloneValues[6] = 65535
    let cloneMask = try PrecisionScalarBuffer(samples: cloneValues, width: 32, height: 12)
    let cloned = try pixelEdit(duplicate, .clone(layer: 1, sourceLayer: 0, dx: 1, dy: 0), cloneMask)
    let clonePixels = try nativeSamples(cloned, layer: 1)
    for i in 0..<384 {
        try require(Array(clonePixels[i * 4..<i * 4 + 4]) == expected(i == 6 ? 7 : i),
                    "Clone samples the captured raw source, with exact uncovered storage")
    }
    let cancel = try RasterJobControl()
    cancel.cancel()
    try refuses("Pre-cancelled typed pixel edit refuses") { _ = try capture.compute(control: cancel) }
    try require(cancel.scratchBytes == 0, "Cancelled typed pixel job releases core reservation")
    try refuses("Typed edit plan refuses inadequate budget before output allocation") {
        _ = try source.pixelEditPlan(request: command.request, workingLimit: 1, otherRetained: 0)
    }
    try refuses("Raw wrapper refuses pixel requests over64KiB before C-string bridging") {
        _ = try source.pixelEditPlan(request: String(repeating: " ", count: 64 * 1024 + 1),
                                     workingLimit: RasterResources.shared.workingLimit, otherRetained: 0)
    }
    try refuses("Caller read allocation is preceded by admitted planning") {
        _ = try source.readingPlane(request: PrecisionPixelCommand.readPlane(target: "red", index: 0).request,
                                     workingLimit: 1, otherRetained: 0)
    }
    try require(try nativeSamples(source) == original, "All typed operations preserve their captured input")
    print("PASS: continuous paint/erase, captured clone, u16 alpha, outside-storage preservation and cancellation/admission")
}

func verifyPrecisionHealing() throws {
    let source = try openTyped(root().appendingPathComponent("source16.png"))
    let full = try PrecisionScalarBuffer(samples: [UInt16](repeating: 65535, count: 384), width: 32, height: 12)
    let target = try PrecisionInk(red: 15000.0 / 65535, green: 25000.0 / 65535, blue: 35000.0 / 65535)
    let sourceBase = try PrecisionInk(red: 10000.0 / 65535, green: 20000.0 / 65535, blue: 30000.0 / 65535)
    let targetDoc = try pixelEdit(source, .rasterize(name: "Heal destination", width: 32, height: 12,
        x: 0, y: 0, colors: [target], space: .document), full)
    let sourceDoc = try pixelEdit(targetDoc, .rasterize(name: "Heal source", width: 32, height: 12,
        x: 0, y: 0, colors: [sourceBase], space: .document), full)
    var bump = [UInt16](repeating: 0, count: 384)
    for y in 5...6 { for x in 14...16 { bump[y * 32 + x] = 65535 } }
    let bumpMask = try PrecisionScalarBuffer(samples: bump, width: 32, height: 12)
    let bumpInk = try PrecisionInk(red: 20000.0 / 65535, green: 30000.0 / 65535, blue: 40000.0 / 65535)
    let prepared = try pixelEdit(sourceDoc, .paint(layer: 2, ink: bumpInk, space: .document), bumpMask)
    var hole = [UInt16](repeating: 0, count: 384)
    for y in 3...8 { for x in 10...20 { hole[y * 32 + x] = 65535 } }
    hole[0] = 65535 // Outside the explicit solve window: deliberately ignored.
    let selection = try PrecisionScalarBuffer(samples: hole, width: 32, height: 12)
    let healed = try pixelEdit(prepared, .heal(layer: 1, sourceLayer: 2, dx: 0, dy: 0,
        x: 8, y: 1, width: 16, height: 10), selection)
    let values = try nativeSamples(healed, layer: 1)
    for i in 0..<384 {
        // The source/destination boundary difference is a constant+5000 in
        // every channel. Its harmonic extension is independently that constant.
        let expected: [Int] = bump[i] == 65535 ? [25000, 35000, 45000, 65535]
                                               : [15000, 25000, 35000, 65535]
        for c in 0..<4 {
            try require(abs(Int(values[i * 4 + c]) - expected[c]) <= (c == 3 ? 0 : 1),
                        "Bounded healing agrees with constant harmonic correction and exact alpha")
        }
    }
    try require(try nativeSamples(healed, layer: 0) == nativeSamples(source), "Heal preserves unrelated layers")
    print("PASS: bounded u16 healing, independent harmonic correction, source/profile/alpha and outside-window preservation")
}
