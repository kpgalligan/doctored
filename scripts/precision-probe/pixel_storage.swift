import AppKit

func pixelEdit(_ source: RasterPrecisionDocument, _ command: PrecisionPixelCommand,
               _ input: PrecisionScalarBuffer? = nil) throws -> RasterPrecisionDocument {
    guard let result = try PrecisionPixelEdit(source, command: command, input: input).run() else {
        throw ProbeFailure(description: "Expected changed typed pixel state")
    }
    return result
}

func scalarRead(_ source: RasterPrecisionDocument, _ target: PrecisionPlaneTarget,
                _ index: Int) throws -> PrecisionScalarBuffer {
    try PrecisionPlaneRead(source, target: target.rawValue, index: index).run()
}

func nativeSamples(_ source: RasterPrecisionDocument, layer: Int = 0) throws -> [UInt16] {
    guard let image = source.layerSource(layer) else { throw ProbeFailure(description: "Missing typed raster") }
    return try image.withRGBA16 { Array($0) }
}

func verifyPrecisionScalarStorage() throws {
    let files = try root()
    let source = try openTyped(files.appendingPathComponent("source16.png"))
    let raw: [UInt16] = [0, 1, 256, 257, 32768, 65535]
    let coverage = try PrecisionScalarBuffer(samples: raw, width: 3, height: 2)
    let masked = try pixelEdit(source, .setMask(layer: 0, width: 3, height: 2, x: 2, y: 3,
                                               linked: false, density: 0.625), coverage)
    let read = try scalarRead(masked, .mask, 0)
    try require(read.width == 3 && read.height == 2 && read.samples == raw,
                "Raw independent u16 mask retains sub-byte codes")
    let effective = try scalarRead(masked, .effectiveMask, 0)
    for y in 0..<12 {
        for x in 0..<32 {
            let v = (2..<5).contains(x) && (3..<5).contains(y) ? raw[(y - 3) * 3 + x - 2] : 0
            let wanted = (1 - 0.625 + 0.625 * Double(v) / 65535) * 65535
            try require(abs(Double(effective.samples[y * 32 + x]) - wanted) <= 0.7,
                        "Canvas effective mask uses continuous density and placement")
        }
    }
    try require(try scalarRead(source, .red, 0).samples == (0..<384).map { expected($0)[0] },
                "Raw RGB plane read keeps native source precision")
    let properties = try pixelEdit(masked, .maskProperties(layer: 0, x: -1, y: 1,
        linked: true, density: 0.75, feather: 1.25, enabled: false))
    try require(try scalarRead(properties, .mask, 0).samples == raw,
                "Property changes preserve raw u16 mask")
    let enabled = try pixelEdit(properties, .maskProperties(layer: 0, x: -1, y: 1,
        linked: true, density: 0.75, feather: 1.25, enabled: true))
    try require(try scalarRead(properties, .effectiveMask, 0).samples
        == scalarRead(enabled, .effectiveMask, 0).samples,
        "Effective inspection evaluates stored mask properties even when disabled")
    let channelValues = (0..<384).map { UInt16(($0 * 257 + 1) % 65536) }
    let channel = try PrecisionScalarBuffer(samples: channelValues, width: 32, height: 12)
    let withChannel = try pixelEdit(masked, .setChannel(name: "Fine coverage"), channel)
    try require(try scalarRead(withChannel, .channel, 0).samples == channelValues,
                "Saved channel keeps coverage between byte levels")
    try require(try PrecisionPixelEdit(withChannel,
        command: .setChannel(index: 0, name: "Fine coverage"), input: channel).run() == nil,
        "Identical channel replacement is a no-op")
    try refuses("Equal sample counts cannot reinterpret3x2 as2x3 mask geometry") {
        _ = try PrecisionPixelEdit(source, command: .setMask(layer: 0, width: 2, height: 3), input: coverage)
    }
    let promoted = try PrecisionScalarBuffer.promoting([0, 1, 128, 255], width: 4, height: 1)
    try require(promoted.samples == [0, 257, 32896, 65535] && promoted.provenance == .promotedU8,
                "Scalar byte promotion is exact and explicitly marked")
    let native = try PrecisionCapturedSave(withChannel)
    let archive = try PrecisionStateArchive.capture(withChannel, name: "Native scalar state")
    let later = try pixelEdit(withChannel, .removeMask(layer: 0))
    _ = try pixelEdit(later, .removeChannel(index: 0))
    let nativeURL = files.appendingPathComponent("scalar-v13.rz")
    try native.save(to: nativeURL)
    let reopened = try openTyped(nativeURL)
    try require(try scalarRead(reopened, .mask, 0).samples == raw
        && scalarRead(reopened, .channel, 0).samples == channelValues,
        "Native13 captures old raw mask/channel state before later removals")
    try verifySource(reopened)
    let archiveURL = files.appendingPathComponent("scalar-archive")
    try PrecisionStateArchive.save(archive, to: archiveURL)
    let loaded = try PrecisionStateArchive.load(from: archiveURL)
    try require(try scalarRead(loaded.document, .mask, 0).samples == raw
        && scalarRead(loaded.document, .channel, 0).samples == channelValues,
        "Host archive preserves native13 scalar state")
    print("PASS: u16 raw/effective masks, channels, explicit promotion, layout guards and captured native/archive")
}

func verifyPrecisionGeometry() throws {
    let source = try openTyped(root().appendingPathComponent("source16.png"))
    let maskValues: [UInt16] = [1, 2, 3, 256, 257, 65535]
    let mask = try PrecisionScalarBuffer(samples: maskValues, width: 3, height: 2)
    let masked = try pixelEdit(source, .setMask(layer: 0, width: 3, height: 2,
                                               x: 2, y: 3, linked: false), mask)
    let translated = try pixelEdit(masked,
        .affine(layer: 0, target: .pixels, matrix: CGAffineTransform(translationX: 3, y: -2), filter: .nearest))
    try require(try nativeSamples(translated) == nativeSamples(source),
                "Exact source translation retains every u16 code and hidden RGB")
    try require(try scalarRead(translated, .mask, 0).samples == maskValues,
                "Unlinked mask raw storage stays fixed under content transform")
    let rotated = try pixelEdit(source, .init(["op": "rotate90"]))
    guard let rotatedImage = rotated.layerSource(0) else { throw ProbeFailure(description: "Missing rotated source") }
    try require(rotatedImage.width == 12 && rotatedImage.height == 32, "Quarter turn swaps raw dimensions")
    try rotatedImage.withRGBA16 { values in
        for y in 0..<32 {
            for x in 0..<12 {
                let original = expected((11 - x) * 32 + y)
                try require(Array(values[(y * 12 + x) * 4..<(y * 12 + x + 1) * 4]) == original,
                            "Quarter turn is an independent exact u16 permutation")
            }
        }
    }
    let restored = try pixelEdit(rotated, .init(["op": "rotate270"]))
    try require(try nativeSamples(restored) == nativeSamples(source), "Exact reverse turn restores all storage")
    let cropped = try pixelEdit(masked, .crop(x: 2, y: 1, width: 20, height: 8))
    try require(try value(cropped.descriptor(), "width") == 20 && value(cropped.descriptor(), "height") == 8,
                "Canvas crop changes only requested geometry")
    try require(try nativeSamples(cropped) == nativeSamples(source), "Crop preserves latent source storage")
    let resized = try pixelEdit(source, .resize(width: 64, height: 24, filter: .nearest))
    guard let resizedImage = resized.layerSource(0) else { throw ProbeFailure(description: "Missing resized source") }
    try resizedImage.withRGBA16 { values in
        for y in 0..<24 {
            for x in 0..<64 {
                try require(Array(values[(y * 64 + x) * 4..<(y * 64 + x + 1) * 4]) == expected((y / 2) * 32 + x / 2),
                            "Nearest resize retains original source codes")
            }
        }
    }
    let perspective = try pixelEdit(source, .perspective(layer: 0, target: .pixels,
        quad: [CGPoint(x: 1, y: 2), CGPoint(x: 33, y: 2), CGPoint(x: 33, y: 14), CGPoint(x: 1, y: 14)],
        filter: .nearest))
    try require(try nativeSamples(perspective) == nativeSamples(source),
                "Affine-equivalent perspective retains raw precision")
    let shear = CGAffineTransform(a: 1, b: 0.17, c: 0.23, d: 1, tx: 0.31, ty: 0.27)
    let filtered = try pixelEdit(source, .affine(layer: 0, target: .pixels, matrix: shear, filter: .bilinear))
    try verifyBilinearAffine(filtered, matrix: shear)
    try refuses("Degenerate typed geometry is refused") {
        _ = try PrecisionPixelEdit(source, command: .resize(width: 0, height: 12))
    }
    print("PASS: exact typed geometry, hidden RGB, crop latent storage, nearest/perspective and independent mask")
}

private func verifyBilinearAffine(_ document: RasterPrecisionDocument, matrix: CGAffineTransform) throws {
    let bounds = CGRect(x: 0, y: 0, width: 32, height: 12).applying(matrix).integral
    let inverse = matrix.inverted()
    guard let image = document.layerSource(0) else { throw ProbeFailure(description: "Missing filtered source") }
    try require(image.width == Int(bounds.width) && image.height == Int(bounds.height),
                "Affine raster bounds match independently transformed corners")
    let layers = try document.info()["layers"] as? [[String: Any]] ?? []
    try require(layers.first?["offset"] as? [Int] == [Int(bounds.minX), Int(bounds.minY)],
                "Affine raster placement matches independent outward rounding")
    var maximum = 0.0
    try image.withRGBA16 { values in
        for y in 0..<image.height {
            for x in 0..<image.width {
                let p = CGPoint(x: bounds.minX + Double(x) + 0.5,
                                y: bounds.minY + Double(y) + 0.5).applying(inverse)
                let u = Double(p.x) - 0.5, v = Double(p.y) - 0.5
                let ix = Int(floor(u)), iy = Int(floor(v))
                var alpha = 0.0
                var premultiplied = [Double](repeating: 0, count: 3)
                for dy in 0...1 {
                    for dx in 0...1 where (0..<32).contains(ix + dx) && (0..<12).contains(iy + dy) {
                        let weight = (dx == 0 ? 1 - (u - Double(ix)) : u - Double(ix))
                            * (dy == 0 ? 1 - (v - Double(iy)) : v - Double(iy))
                        let source = expected((iy + dy) * 32 + ix + dx)
                        let a = Double(source[3]) / 65535
                        alpha += weight * a
                        for c in 0..<3 { premultiplied[c] += weight * a * Double(source[c]) / 65535 }
                    }
                }
                let visible = (alpha * 65535).rounded() > 0
                let reference = premultiplied.map { visible ? $0 / alpha : 0 } + [alpha]
                for c in 0..<4 {
                    maximum = max(maximum, abs(Double(values[(y * image.width + x) * 4 + c]) / 65535 - reference[c]))
                }
            }
        }
    }
    try require(maximum <= 2e-6 + 0.5 / 65535, "Independent bilinear affine error exceeds frozen bound: \(maximum)")
    print("MEASURE: typed bilinear affine maximum_error=\(maximum)")
}
