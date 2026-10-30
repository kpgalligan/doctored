import AppKit

/// Independent native24MP branches: mask/channel/geometry persistence, followed
/// by paint/retouch/artwork on a fresh source. No smaller substitute is allowed.
func verifyLargePixelWorkflow() throws {
    let directory = try root()
    let width = 6000, height = 4000, count = 24_000_000
    let indices = (0...256).map { $0 * (count - 1) / 256 }
    var records: [[String: Any]] = []
    var current: RasterPrecisionDocument?
    func stage(_ name: String, _ scope: String, _ work: () throws -> Void) {
        let start = Date()
        var row: [String: Any] = ["stage": name, "retained_scope": scope]
        do {
            row["before"] = try RasterResources.shared.report()
            try autoreleasepool { try work() }
            row["status"] = "completed"
        }
        catch let error as ProbeFailure { row["status"] = "verification_failed"; row["reason"] = error.description }
        catch { row["status"] = "refused"; row["reason"] = error.localizedDescription }
        row["seconds"] = Date().timeIntervalSince(start)
        do { row["after"] = try RasterResources.shared.report() }
        catch { row["status"] = "verification_failed"; row["resource_report_error"] = String(describing: error) }
        records.append(row)
        print("LARGE PIXELS: \(name): \(row["status"] ?? "unknown") \(row["reason"] ?? "")")
        fflush(stdout)
    }
    func requireSource() throws -> RasterPrecisionDocument {
        guard let current else { throw ProbeFailure(description: "Prior source unavailable") }
        return current
    }
    func scalar(_ value: (Int) -> UInt16) throws -> PrecisionScalarBuffer {
        _ = try RasterResources.shared.admit(scratch: UInt64(count) * 2 + 512)
        return try PrecisionScalarBuffer(samples: (0..<count).map(value), width: width, height: height)
    }
    func checkMask(_ document: RasterPrecisionDocument, rotated: Bool = false) throws {
        let info = try document.info()
        guard let layers = info["layers"] as? [[String: Any]], let layer = layers.first,
              let mask = layer["mask"] as? [String: Any] else { throw ProbeFailure(description: "Missing mask placement") }
        try require(mask["offset"] as? [Int] == (rotated ? [2, 3] : [3, -2]), "Independent mask canvas placement")
        try require(mask["dimensions"] as? [Int] == (rotated ? [height, width] : [width, height]), "Independent mask raw extent")
        try require(mask["linked"] as? Bool == false && mask["enabled"] as? Bool == true,
                    "Mask link/enabled state persists")
        try require(mask["density"] as? Double == 0.625 && mask["feather"] as? Double == 0,
                    "Mask density/feather persists")
    }
    func maskCode(_ i: Int) -> UInt16 { UInt16(truncatingIfNeeded: i * 13 + 1) }
    func channelCode(_ i: Int) -> UInt16 { UInt16(truncatingIfNeeded: i * 7 + 9) }
    func checkOriginal(_ doc: RasterPrecisionDocument, rotated: Bool = false) throws {
        guard let image = doc.layerSource(0) else { throw ProbeFailure(description: "Missing large raw source") }
        try require(image.width == (rotated ? height : width) && image.height == (rotated ? width : height),
                    "Exact operation preserves native pixel count")
        try image.withRGBA16 { pixels in
            for i in indices {
                let original = rotated ? (height - 1 - i % height) * width + i / height : i
                try require(Array(pixels[i * 4..<i * 4 + 4]) == expected(original),
                            "Distributed exact source/hidden RGB sample")
            }
        }
    }
    stage("preserving source import", "one192MB native source") {
        current = try openTyped(directory.appendingPathComponent("large-source16.png"))
        try checkOriginal(requireSource())
    }
    stage("genuine u16 independent mask", "source192MB plus48MB caller coverage and48MB retained mask during creation") {
        current = try pixelEdit(requireSource(), .setMask(layer: 0, width: width, height: height,
            linked: false, density: 0.625), scalar(maskCode))
    }
    stage("genuine u16 saved channel", "source+mask240MB;48MB caller/retained channel overlap during creation") {
        current = try pixelEdit(requireSource(), .setChannel(name: "Native16 coverage"), scalar(channelCode))
    }
    stage("masked fractional-alpha projection", "retained source/mask/channel288MB; full output/scratch released after check") {
        let doc = try requireSource(), budget = try PrecisionWorkContext(source: doc)
        let image = try doc.render(workingLimit: budget.limit, otherRetained: budget.otherRetained)
        try image.withRGBA16 { pixels in
            for i in indices {
                let input = expected(i), coverage = 0.375 + 0.625 * Double(maskCode(i)) / 65535
                let reference = (0..<3).map { input[3] == 0 ? 0 : Double(input[$0]) / 65535 }
                    + [Double(input[3]) / 65535 * coverage]
                for c in 0..<4 {
                    try require(abs(Double(pixels[i * 4 + c]) / 65535 - reference[c]) <= 2e-6 + 0.5 / 65535,
                                "Independent masked composite reference")
                }
            }
        }
    }
    stage("mask-only exact translation", "retained288MB; scalar read output is48MB and stage-local") {
        current = try pixelEdit(requireSource(), .affine(layer: 0, target: .mask,
            matrix: CGAffineTransform(translationX: 3, y: -2), filter: .nearest))
        try checkMask(requireSource())
        let read = try scalarRead(requireSource(), .mask, 0)
        for i in indices { try require(read.samples[i] == maskCode(i), "Mask translation keeps exact raw u16") }
    }
    stage("whole quarter turn", "old/new RGB, mask and channel coexist until return; old owner then released") {
        current = try pixelEdit(requireSource(), .init(["op": "rotate90"]))
        try checkOriginal(requireSource(), rotated: true)
        try checkMask(requireSource(), rotated: true)
        let channel = try scalarRead(requireSource(), .channel, 0)
        let mask = try scalarRead(requireSource(), .mask, 0)
        for i in indices {
            let old = (height - 1 - i % height) * width + i / height
            try require(channel.samples[i] == channelCode(old) && mask.samples[i] == maskCode(old),
                        "Whole geometry permutes saved u16 channel and independent mask")
        }
    }
    stage("exact reverse turn", "old/new288MB states overlap during inverse; original-sized state retained afterward") {
        current = try pixelEdit(requireSource(), .init(["op": "rotate270"]))
        try checkOriginal(requireSource())
    }
    let matrix = CGAffineTransform(a: 1, b: 0, c: 0.01, d: 1, tx: 0.23, ty: 0.31)
    stage("full-source bilinear affine", "old/new native raster overlap; unlinked mask/channel share storage; no preview reduction") {
        current = try pixelEdit(requireSource(), .affine(layer: 0, target: .pixels, matrix: matrix, filter: .bilinear))
        try checkLargeAffine(requireSource(), matrix: matrix)
    }
    let native = directory.appendingPathComponent("large-pixels-geometry.rz")
    stage("captured geometry/mask/channel native save", "current state and captured alias; native encoding scratch only") {
        try PrecisionCapturedSave(requireSource(), destination: .native).save(to: native)
    }
    stage("geometry/mask/channel native reopen", "current state plus independent reopened source/mask/channel, released after checks") {
        let reopened = try openTyped(native)
        try checkLargeAffine(reopened, matrix: matrix)
        try checkMask(reopened)
        let mask = try scalarRead(reopened, .mask, 0), channel = try scalarRead(reopened, .channel, 0)
        for i in indices {
            try require(mask.samples[i] == maskCode(i) && channel.samples[i] == channelCode(i),
                        "Native retains all scalar codes")
        }
    }
    let archive = directory.appendingPathComponent("large-pixels-archive")
    stage("captured geometry/scalar archive save", "current state, capture alias and bounded host manifest") {
        try PrecisionStateArchive.save(PrecisionStateArchive.capture(requireSource(), name: "Native24MP pixel state"),
                                       to: archive)
    }
    stage("geometry/scalar archive reopen", "current and reopened states coexist only through checks") {
        let loaded = try PrecisionStateArchive.load(from: archive)
        try checkLargeAffine(loaded.document, matrix: matrix)
        try checkMask(loaded.document)
        let mask = try scalarRead(loaded.document, .mask, 0), channel = try scalarRead(loaded.document, .channel, 0)
        for i in indices { try require(mask.samples[i] == maskCode(i) && channel.samples[i] == channelCode(i),
                                      "Archive retains all scalar codes") }
    }
    current = nil
    stage("fresh source for paint/retouch", "previous geometry/scalar owners released; one192MB source") {
        current = try openTyped(directory.appendingPathComponent("large-source16.png"))
    }
    func pixel(_ image: RasterPrecisionImage, _ i: Int) throws -> [UInt16] {
        try image.withRGBA16 { Array($0[i * 4..<i * 4 + 4]) }
    }
    func checkPaint(_ actual: [UInt16], old: [UInt16], ink: [Double], coverage: Double) throws {
        let sa = ink[3] * coverage, da = Double(old[3]) / 65535
        let a = sa + da * (1 - sa)
        let reference = (0..<3).map { a > 0 ? (ink[$0] * sa + Double(old[$0]) / 65535 * da * (1 - sa)) / a : 0 } + [a]
        for c in 0..<4 { try require(abs(Double(actual[c]) / 65535 - reference[c]) <= 2e-6 + 0.5 / 65535,
                                    "Independent sparse paint/clone reference") }
    }
    stage("captured continuous paint", "old/new192MB source plus48MB full-canvas coverage; old source released afterward") {
        let doc = try requireSource(), ink = try PrecisionInk(red: 0.41, green: 0.23, blue: 0.67, alpha: 0.7)
        current = try pixelEdit(doc, .paint(layer: 0, ink: ink, space: .document, opacity: 0.6),
                                scalar { $0 == 7 ? 32768 : 0 })
        guard let output = current?.layerSource(0) else { throw ProbeFailure(description: "Missing paint result") }
        try checkPaint(pixel(output, 7), old: expected(7), ink: ink.rgba, coverage: 0.6 * 32768 / 65535)
        for i in indices { try require(try pixel(output, i) == expected(i), "Paint preserves sampled uncovered source") }
    }
    stage("captured continuous erase", "old/new192MB source plus48MB coverage; straight RGB stays exact") {
        let doc = try requireSource()
        guard let before = doc.layerSource(0) else { throw ProbeFailure(description: "Missing erase input") }
        let old = try pixel(before, 7)
        current = try pixelEdit(doc, .erase(layer: 0, opacity: 0.25), scalar { $0 == 7 ? 32768 : 0 })
        guard let output = current?.layerSource(0) else { throw ProbeFailure(description: "Missing erase result") }
        let actual = try pixel(output, 7)
        try require(Array(actual.prefix(3)) == Array(old.prefix(3)), "Large erase preserves straight RGB")
        let expectedAlpha: Double = Double(old[3]) / 65535.0 * (1.0 - 0.25 * 32768.0 / 65535.0)
        let actualAlpha: Double = Double(actual[3]) / 65535.0
        try require(abs(actualAlpha - expectedAlpha) <= 2e-6 + 0.5 / 65535.0,
                    "Independent large erase alpha")
    }
    stage("captured source clone", "old/new192MB source and48MB coverage; sampling uses immutable pre-edit source") {
        let doc = try requireSource()
        guard let before = doc.layerSource(0) else { throw ProbeFailure(description: "Missing clone input") }
        let old = try pixel(before, 6), source = try pixel(before, 7).map { Double($0) / 65535 }
        current = try pixelEdit(doc, .clone(layer: 0, sourceLayer: 0, dx: 1, dy: 0), scalar { $0 == 6 ? 65535 : 0 })
        guard let output = current?.layerSource(0) else { throw ProbeFailure(description: "Missing clone result") }
        try checkPaint(pixel(output, 6), old: old, ink: source, coverage: 1)
    }
    stage("bounded local healing", "24MP source; two local typed paint preparations, then old/new192MB source plus48MB coverage and32x32 solver") {
        // The original patterned source has transparent columns that partition
        // a wrapped-ramp membrane into constant corrections. Make this check
        // independently nontrivial: an opaque constant patch and one interior
        // bump, with source shifted two pixels. Boundary correction is zero.
        let base = try PrecisionInk(red: 15000.0 / 65535, green: 25000.0 / 65535, blue: 35000.0 / 65535)
        current = try pixelEdit(requireSource(), .paint(layer: 0, ink: base, space: .document),
            scalar { i in (5520..<5552).contains(i % width) && (0..<32).contains(i / width) ? 65535 : 0 })
        let bump = try PrecisionInk(red: 25000.0 / 65535, green: 35000.0 / 65535, blue: 45000.0 / 65535)
        current = try pixelEdit(requireSource(), .paint(layer: 0, ink: bump, space: .document),
            scalar { $0 == 5 * width + 5535 ? 65535 : 0 })
        let doc = try requireSource()
        guard let before = doc.layerSource(0) else { throw ProbeFailure(description: "Missing heal input") }
        let coverage = try scalar { i in (5530..<5542).contains(i % width) && (3..<9).contains(i / width) ? 65535 : 0 }
        let capture = try PrecisionPixelEdit(doc, command: .heal(layer: 0, sourceLayer: 0, dx: 2, dy: 0,
            x: 5520, y: 0, width: 32, height: 32), input: coverage)
        guard let healed = try capture.run() else { throw ProbeFailure(description: "Interior bump healing must change RGB") }
        current = healed
        guard let output = current?.layerSource(0) else { throw ProbeFailure(description: "Missing heal result") }
        var changedInterior = false
        for y in 0..<32 { for x in 5520..<5552 {
            let old = try pixel(before, y * width + x)
            let new = try pixel(output, y * width + x)
            try require(new[3] == old[3], "Large local healing preserves exact alpha")
            if (5530..<5542).contains(x) && (3..<9).contains(y) {
                let reference = x == 5533 && y == 5 ? [25000, 35000, 45000] : [15000, 25000, 35000]
                for c in 0..<3 { try require(abs(Int(new[c]) - reference[c]) <= 1,
                                            "Independent zero harmonic correction and translated source bump") }
                changedInterior = changedInterior || Array(old.prefix(3)) != Array(new.prefix(3))
            } else { try require(new == old, "Healing preserves every outside-window-patch pixel") }
        } }
        try require(changedInterior, "Interior source bump produces a nontrivial heal")
        for i in indices { try require(try pixel(output, i) == pixel(before, i), "Healing preserves sampled outside storage") }
    }
    stage("scalar TextKit artwork", "current192MB source; bounded glyph coverage and one small typed raster") {
        let payload = TextLayerPayload(string: "Typed16", style: TextStyle(family: "Helvetica Neue", size: 28),
                                      color: .white, box: .point)
        let ink = try PrecisionInk(red: 0.456789, green: 0.234567, blue: 0.678912)
        current = try PrecisionRasterizedArtwork.text(requireSource(), payload: payload,
            anchor: CGPoint(x: 80.25, y: 60.5), ink: ink, space: .document).run()
        try require(current != nil, "TextKit adds typed artwork without byte RGB")
    }
    stage("scalar shape artwork", "current source+text; separate small fill/stroke coverage and typed raster") {
        let payload = ShapeLayerPayload(kind: "ellipse", w: 40, h: 30, fill: .white, stroke: .white, strokeWidth: 2)
        let fill = try PrecisionInk(red: 0.234567, green: 0.456789, blue: 0.678912, alpha: 0.7)
        let stroke = try PrecisionInk(red: 0.654321, green: 0.321987, blue: 0.123456)
        current = try PrecisionRasterizedArtwork.shape(requireSource(), payload: payload,
            anchor: CGPoint(x: 120.25, y: 90.5), fill: fill, stroke: stroke, space: .document).run()
        try require(current != nil, "Shapes add typed artwork without byte RGB")
    }
    let edited = directory.appendingPathComponent("large-pixels-edited.rz")
    stage("captured paint/artwork native save", "current source and two small rasters; native encoder scratch only") {
        try PrecisionCapturedSave(requireSource(), destination: .native).save(to: edited)
    }
    stage("paint/artwork native reopen", "current and reopened source states overlap only while checking distributed samples") {
        let original = try requireSource(), reopened = try openTyped(edited)
        try require(try value(reopened.descriptor(), "layer_count") == 3, "Native retains both artwork rasters")
        guard let before = original.layerSource(0), let after = reopened.layerSource(0) else {
            throw ProbeFailure(description: "Missing reopened edited source")
        }
        for i in indices + [6, 7, 5 * width + 5533, 5 * width + 5535] {
            try require(try pixel(before, i) == pixel(after, i), "Native retains edited source u16 samples")
        }
    }
    current = nil
    let final = try RasterResources.shared.report()
    let report: [String: Any] = ["width": width, "height": height,
        "working_limit_bytes": RasterResources.shared.workingLimit,
        "complete_workflow": records.allSatisfy { $0["status"] as? String == "completed" },
        "refusals_fail_size_target": true, "resizing_permitted": false,
        "scope": "Separate geometry/scalar and paint/artwork branches, each starting from original native24MP. Exact old/new owners overlap only during each transition.",
        "numeric_checks": "257 distributed exact source/mask/channel values and f64 masked/bilinear references; sparse paint/erase/clone scalar oracles; nontrivial source-bump heal with independent zero harmonic correction, exact alpha and outside preservation.",
        "stages": records, "after_releasing_sources": final]
    try JSONSerialization.data(withJSONObject: report, options: [.prettyPrinted, .sortedKeys])
        .write(to: directory.appendingPathComponent("large-pixels-report.json"))
}

private func checkLargeAffine(_ document: RasterPrecisionDocument, matrix: CGAffineTransform) throws {
    let width = 6000, height = 4000
    let bounds = CGRect(x: 0, y: 0, width: width, height: height).applying(matrix).integral
    let inverse = matrix.inverted()
    let descriptor = try document.descriptor()
    try require(try value(descriptor, "width") == 6000 && value(descriptor, "height") == 4000,
                "Leaf affine keeps the original canvas")
    let info = try document.info()
    guard let layers = info["layers"] as? [[String: Any]], let layer = layers.first else {
        throw ProbeFailure(description: "Missing affine placement")
    }
    try require(layer["offset"] as? [Int] == [Int(bounds.minX), Int(bounds.minY)], "Affine stored placement agrees with requested bounds")
    guard let image = document.layerSource(0) else { throw ProbeFailure(description: "Missing large affine source") }
    try require(image.width == Int(bounds.width) && image.height == Int(bounds.height), "Full requested affine extent retained")
    let profile = try Data(contentsOf: root().appendingPathComponent("source-profile.icc"))
    try require(image.profileData == profile, "Affine keeps exact document-profile context")
    try image.withRGBA16 { pixels in
        for n in 0...256 {
            let i = n * (image.width * image.height - 1) / 256
            let p = CGPoint(x: bounds.minX + CGFloat(i % image.width) + 0.5,
                            y: bounds.minY + CGFloat(i / image.width) + 0.5).applying(inverse)
            let u = Double(p.x) - 0.5, v = Double(p.y) - 0.5
            let ix = Int(floor(u)), iy = Int(floor(v))
            var alpha = 0.0, rgb = [Double](repeating: 0, count: 3)
            for dy in 0...1 { for dx in 0...1 where (0..<width).contains(ix + dx) && (0..<height).contains(iy + dy) {
                let weight = (dx == 0 ? 1 - u + Double(ix) : u - Double(ix))
                    * (dy == 0 ? 1 - v + Double(iy) : v - Double(iy))
                let input = expected((iy + dy) * width + ix + dx), a = Double(input[3]) / 65535
                alpha += a * weight
                for c in 0..<3 { rgb[c] += Double(input[c]) / 65535 * a * weight }
            } }
            let reference = rgb.map { (alpha * 65535).rounded() > 0 ? $0 / alpha : 0 } + [alpha]
            for c in 0..<4 { try require(abs(Double(pixels[i * 4 + c]) / 65535 - reference[c]) <= 2e-6 + 0.5 / 65535,
                                        "Distributed independent native-size affine reference") }
        }
    }
}
