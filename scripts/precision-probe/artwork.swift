import AppKit

private func verifyArtworkPixels(_ document: RasterPrecisionDocument, layer: Int,
                                  capture: PrecisionRasterizedArtwork, colors: [PrecisionInk]) throws {
    guard let input = capture.edit.input, let image = document.layerSource(layer) else {
        throw ProbeFailure(description: "Missing typed artwork coverage/source")
    }
    try require(input.provenance == .promotedU8 && input.samples.allSatisfy { $0 % 257 == 0 },
                "Artwork coverage explicitly retains256levels after promotion")
    try require(image.width == input.width && image.height == input.height && colors.count == input.planeCount,
                "Typed artwork uses the exact planned extent and plane count")
    var hasInk = false
    var hasSubByteRGB = false
    try image.withRGBA16 { output in
        let count = input.width * input.height
        for i in 0..<count {
            var premultiplied = [Double](repeating: 0, count: 3)
            var alpha = 0.0
            for plane in 0..<input.planeCount {
                let color = colors[plane].rgba
                let sourceAlpha = Double(input.samples[plane * count + i]) / 65535 * color[3]
                for c in 0..<3 { premultiplied[c] = color[c] * sourceAlpha + premultiplied[c] * (1 - sourceAlpha) }
                alpha = sourceAlpha + alpha * (1 - sourceAlpha)
            }
            let wanted = premultiplied.map { alpha > 0 ? $0 / alpha : 0 } + [alpha]
            for c in 0..<4 {
                try require(abs(Double(output[i * 4 + c]) / 65535 - wanted[c]) <= 2e-6 + 0.5 / 65535,
                            "Typed artwork matches independent float fill-then-stroke composition")
            }
            if output[i * 4 + 3] > 0 {
                hasInk = true
                hasSubByteRGB = hasSubByteRGB || output[i * 4] % 257 != 0
            }
        }
    }
    try require(hasInk && hasSubByteRGB, "Artwork stores actual ink with RGB precision beyond byte codes")
}

func verifyPrecisionArtwork() throws {
    let source = try openTyped(root().appendingPathComponent("source16.png"))
    let textInk = try PrecisionInk(red: 0.456789, green: 0.234567, blue: 0.678912, alpha: 0.876543)
    let style = TextStyle(family: "Helvetica Neue", size: 24, weight: 5, tracking: 0.5,
                          leading: 22, baselineShift: 2, underline: true, alignment: .center)
    let payload = TextLayerPayload(string: "Aa Ω\nTyped", style: style, color: .white, box: .width(110))
    let anchor = CGPoint(x: 4.25, y: 3.5)
    let captured = try PrecisionRasterizedArtwork.text(source, payload: payload, anchor: anchor,
                                                       ink: textInk, space: .document)
    guard let coverage = captured.edit.input,
          let rgbSpace = CGColorSpace(name: CGColorSpace.sRGB),
          let legacy = TextLayer.render(payload, anchor: anchor, space: rgbSpace) else {
        throw ProbeFailure(description: "Missing actual TextKit coverage/legacy renderer")
    }
    try require(legacy.width == coverage.width && legacy.height == coverage.height,
                "Typed text keeps established TextKit layout extent")
    for i in 0..<(legacy.width * legacy.height) {
        try require(coverage.samples[i] == UInt16(legacy.pixels[i * 4 + 3]) * 257,
                    "Typed text uses exactly legacy glyph/decorative alpha geometry")
    }
    guard let text = try captured.run() else { throw ProbeFailure(description: "Typed text raster was not added") }
    try verifyArtworkPixels(text, layer: 1, capture: captured, colors: [textInk])
    let fill = try PrecisionInk(red: 0.450123, green: 0.123456, blue: 0.650123, alpha: 0.600001)
    let stroke = try PrecisionInk(red: 0.250111, green: 0.700019, blue: 0.030029, alpha: 0.800003)
    guard let map = LinearMap(array: [1, 0.2, 0, 1]) else { throw ProbeFailure(description: "Invalid test map") }
    let shapePayload = ShapeLayerPayload(kind: "rect", w: 20, h: 14, fill: .white, stroke: .white,
                                         strokeWidth: 2.5, radius: 3.1, transform: map)
    let shapeCapture = try PrecisionRasterizedArtwork.shape(text, payload: shapePayload,
        anchor: CGPoint(x: 3.25, y: 2.5), fill: fill, stroke: stroke, space: .document)
    guard let shape = try shapeCapture.run() else { throw ProbeFailure(description: "Typed shape raster was not added") }
    try verifyArtworkPixels(shape, layer: 2, capture: shapeCapture, colors: [fill, stroke])
    try require(try nativeSamples(shape, layer: 0) == nativeSamples(source),
                "Artwork leaves the captured photograph exact")
    let native = try root().appendingPathComponent("artwork-v13.rz")
    try PrecisionCapturedSave(shape).save(to: native)
    let reopened = try openTyped(native)
    try require(try nativeSamples(reopened, layer: 1) == nativeSamples(shape, layer: 1)
        && nativeSamples(reopened, layer: 2) == nativeSamples(shape, layer: 2),
        "Typed rasterized artwork survives native reopen exactly")
    var tooLong = payload
    tooLong.string = String(repeating: "x", count: PrecisionRasterizedArtwork.maximumTextUnits + 1)
    try refuses("Typed TextKit work has a declared input bound") {
        _ = try PrecisionRasterizedArtwork.text(source, payload: tooLong, anchor: .zero,
                                                ink: textInk, space: .document)
    }
    try require(captured.coverageInformation.contains("256"), "Coverage information is explicitly disclosed")
    print("PASS: actual TextKit alpha geometry, continuous authored RGB, fill/stroke float assembly and native13 artwork")
}
