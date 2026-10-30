import Foundation

/// Standalone Foundation-only reference for captured native RZLM migration
/// planes. No app/core code, headers, libraries or generated display images.
enum ReferenceFailure: Error { case invalid(String) }
struct Plane {
    let width: Int
    let height: Int
    let values: [UInt16]
    init(_ path: String) throws {
        let data = try Data(contentsOf: URL(fileURLWithPath: path))
        guard data.count >= 16, Array(data[0..<4]) == Array("RZLM".utf8) else {
            throw ReferenceFailure.invalid("Missing RZLM header")
        }
        func word(_ offset: Int) -> UInt32 {
            UInt32(data[offset]) | UInt32(data[offset + 1]) << 8
                | UInt32(data[offset + 2]) << 16 | UInt32(data[offset + 3]) << 24
        }
        guard word(4) == 1 else { throw ReferenceFailure.invalid("Unknown RZLM version") }
        width = Int(word(8)); height = Int(word(12))
        guard width > 0, height > 0, data.count == 16 + width * height * 2 else {
            throw ReferenceFailure.invalid("RZLM dimensions or sample length")
        }
        values = (0..<(width * height)).map { i in
            let at = 16 + i * 2
            return UInt16(data[at]) | UInt16(data[at + 1]) << 8
        }
    }
    func at(_ x: Int, _ y: Int) -> UInt16 {
        guard x >= 0, y >= 0, x < width, y < height else { return 0 }
        return values[y * width + x]
    }
}
func weight(_ distance: Double) -> Double { max(0, 1 - abs(distance)) }
func reference(_ input: Plane, _ matrix: [Double], _ x: Int, _ y: Int,
               filtered: Bool) -> UInt16 {
    let px = matrix[0] * (Double(x) + 0.5) + matrix[2] * (Double(y) + 0.5) + matrix[4]
    let py = matrix[1] * (Double(x) + 0.5) + matrix[3] * (Double(y) + 0.5) + matrix[5]
    if !filtered { return input.at(Int(floor(px)), Int(floor(py))) }
    let bx = Int(floor(px - 0.5)), by = Int(floor(py - 0.5))
    var result = 0.0
    for ty in 0..<2 {
        for tx in 0..<2 {
            let ix = bx + tx, iy = by + ty
            result += Double(input.at(ix, iy)) * weight(px - 0.5 - Double(ix))
                * weight(py - 0.5 - Double(iy))
        }
    }
    return UInt16(max(0, min(65535, result)).rounded())
}

private func nativeSamples(_ path: String, width: Int, height: Int) throws -> [UInt16] {
    let bytes = try Data(contentsOf: URL(fileURLWithPath: path))
    guard bytes.count == width * height * 8 else {
        throw ReferenceFailure.invalid("Native RGBA sample dimensions disagree with report")
    }
    return (0..<(width * height * 4)).map { n in
        UInt16(bytes[2 * n]) | UInt16(bytes[2 * n + 1]) << 8
    }
}

private func big16(_ bytes: [UInt8], _ offset: Int) -> Int {
    Int(bytes[offset]) << 8 | Int(bytes[offset + 1])
}
private func big32(_ bytes: [UInt8], _ offset: Int) -> Int {
    big16(bytes, offset) << 16 | big16(bytes, offset + 2)
}

/// Independent ICC TRC reader and inverse, using the captured camera profile.
private struct Transfer {
    let kind: Int
    let parameters: [Double]
    let curve: [Double]

    init(profile: [UInt8], channel: Int) throws {
        let signature = Array(["rTRC", "gTRC", "bTRC"][channel].utf8)
        var tag: [UInt8] = []
        for index in 0..<big32(profile, 128) {
            let offset = 132 + 12 * index
            if Array(profile[offset..<(offset + 4)]) == signature {
                let start = big32(profile, offset + 4)
                let count = big32(profile, offset + 8)
                tag = Array(profile[start..<(start + count)])
                break
            }
        }
        guard tag.count >= 12 else { throw ReferenceFailure.invalid("Camera ICC lacks TRC") }
        if Array(tag[0..<4]) == Array("curv".utf8) {
            let count = big32(tag, 8)
            kind = 0
            if count == 0 { parameters = [1]; curve = [] }
            else if count == 1 { parameters = [Double(big16(tag, 12)) / 256]; curve = [] }
            else {
                parameters = []
                curve = (0..<count).map { Double(big16(tag, 12 + 2 * $0)) / 65535 }
            }
        } else {
            guard Array(tag[0..<4]) == Array("para".utf8) else {
                throw ReferenceFailure.invalid("Unsupported camera ICC TRC")
            }
            kind = big16(tag, 8)
            guard (0...4).contains(kind) else { throw ReferenceFailure.invalid("Unsupported ICC curve kind") }
            let count = [1, 3, 4, 5, 7][kind]
            parameters = (0..<count).map {
                Double(Int32(bitPattern: UInt32(big32(tag, 12 + 4 * $0)))) / 65536
            }
            curve = []
        }
    }

    func decode(_ encoded: Double) -> Double {
        let x = min(1, max(0, encoded))
        if !curve.isEmpty {
            let point = x * Double(curve.count - 1)
            let low = Int(point), high = min(low + 1, curve.count - 1)
            return curve[low] + (curve[high] - curve[low]) * (point - Double(low))
        }
        if kind == 0 { return pow(x, parameters[0]) }
        let p = parameters
        switch kind {
        case 1: return x >= -p[2] / p[1] ? pow(p[1] * x + p[2], p[0]) : 0
        case 2: return x >= -p[2] / p[1] ? pow(p[1] * x + p[2], p[0]) + p[3] : p[3]
        case 3: return x >= p[4] ? pow(p[1] * x + p[2], p[0]) : p[3] * x
        default: return x >= p[4] ? pow(p[1] * x + p[2], p[0]) + p[5] : p[3] * x + p[6]
        }
    }

    func encode(_ linear: Double) -> Double {
        let y = min(1, max(0, linear))
        if y <= decode(0) { return 0 }
        if y >= decode(1) { return 1 }
        if !curve.isEmpty {
            var low = 0, high = curve.count - 1
            while high - low > 1 {
                let middle = (low + high) / 2
                if curve[middle] < y { low = middle } else { high = middle }
            }
            return (Double(low) + (y - curve[low]) / (curve[high] - curve[low]))
                / Double(curve.count - 1)
        }
        let p = parameters
        if kind == 0 { return pow(y, 1 / p[0]) }
        let x: Double
        switch kind {
        case 1: x = (pow(y, 1 / p[0]) - p[2]) / p[1]
        case 2: x = (pow(max(0, y - p[3]), 1 / p[0]) - p[2]) / p[1]
        case 3: x = y >= pow(p[1] * p[4] + p[2], p[0])
            ? (pow(y, 1 / p[0]) - p[2]) / p[1] : y / p[3]
        default: x = y >= pow(p[1] * p[4] + p[2], p[0]) + p[5]
            ? (pow(max(0, y - p[5]), 1 / p[0]) - p[2]) / p[1] : (y - p[6]) / p[3]
        }
        return min(1, max(0, x))
    }
}

private func transform(_ matrix: [Double], _ x: Double, _ y: Double) -> (Double, Double) {
    (matrix[0] * x + matrix[2] * y + matrix[4],
     matrix[1] * x + matrix[3] * y + matrix[5])
}

private func mapped(_ optical: [String: Any], x: Double, y: Double,
                    channel: Int, width: Int, height: Int) throws -> (Double, Double) {
    guard !optical.isEmpty else { return (x, y) }
    let size = optical["reference_size"] as? [UInt32] ?? [UInt32(width), UInt32(height)]
    let matrix = optical["reference_from_source"] as? [Double] ?? [1, 0, 0, 1, 0, 0]
    guard size.count == 2, matrix.count == 6,
          let center = optical["center"] as? [Double], center.count == 2,
          let radial = optical["radial"] as? [Double], radial.count == 3,
          let red = optical["red_scale"] as? Double,
          let blue = optical["blue_scale"] as? Double else {
        throw ReferenceFailure.invalid("Incomplete registered optical map")
    }
    let c = (center[0] * Double(size[0]), center[1] * Double(size[1]))
    let radius = Double(max(size[0], size[1])) / 2
    let p = transform(matrix, x, y)
    let q = ((p.0 - c.0) / radius, (p.1 - c.1) / radius)
    let r2 = q.0 * q.0 + q.1 * q.1
    let d = 1 + r2 * (radial[0] + r2 * (radial[1] + r2 * radial[2]))
    let scale = channel == 0 ? red : channel == 2 ? blue : 1
    let tx = c.0 + radius * q.0 * d * scale - matrix[4]
    let ty = c.1 + radius * q.1 * d * scale - matrix[5]
    let det = matrix[0] * matrix[3] - matrix[1] * matrix[2]
    return ((matrix[3] * tx - matrix[2] * ty) / det,
            (-matrix[1] * tx + matrix[0] * ty) / det)
}

private func sourcePixels(_ row: [String: Any]) throws -> [String: Any] {
    guard let name = row["name"] as? String, let depth = row["depth"] as? Int,
          let dimensions = row["new_dimensions"] as? [Int], dimensions.count == 2,
          let old = row["old_dimensions"] as? [Int], old.count == 2,
          let matrix = row["inverse"] as? [Double], matrix.count == 6,
          let inputPath = row["pre_vignette"] as? String,
          let outputPath = row["reviewed_values"] as? String,
          let profilePath = row["profile"] as? String,
          let vignette = row["vignette"] as? [String: Any],
          let center = vignette["center"] as? [Double], center.count == 2,
          let stops = vignette["edge_exposure"] as? Double,
          let falloff = vignette["falloff"] as? Double else {
        throw ReferenceFailure.invalid("Incomplete current-input registered vignette row")
    }
    let width = dimensions[0], height = dimensions[1]
    let input = try nativeSamples(inputPath, width: width, height: height)
    let output = try nativeSamples(outputPath, width: width, height: height)
    let profile = Array(try Data(contentsOf: URL(fileURLWithPath: profilePath)))
    let transfers = try (0..<3).map { try Transfer(profile: profile, channel: $0) }
    let c = (center[0] * Double(old[0]), center[1] * Double(old[1]))
    let corners = [(0.5, 0.5), (Double(old[0]) - 0.5, 0.5),
        (0.5, Double(old[1]) - 0.5),
        (Double(old[0]) - 0.5, Double(old[1]) - 0.5)]
    let radius = corners.map { hypot($0.0 - c.0, $0.1 - c.1) }.max() ?? 0
    let wrongCenter = (center[0] * Double(width), center[1] * Double(height))
    let wrongCorners = [(0.5, 0.5), (Double(width) - 0.5, 0.5),
        (0.5, Double(height) - 0.5), (Double(width) - 0.5, Double(height) - 0.5)]
    let wrongRadius = wrongCorners.map { hypot($0.0 - wrongCenter.0, $0.1 - wrongCenter.1) }.max() ?? 0
    let optical = row["optical"] as? [String: Any] ?? [:]
    var registered = [UInt16](repeating: 0, count: input.count)
    var wrongMaximum = 0
    var distinctInputs = 0
    for y in 0..<height {
        for x in 0..<width {
            let index = (y * width + x) * 4
            let p = transform(matrix, Double(x) + 0.5, Double(y) + 0.5)
            let distance = hypot(p.0 - c.0, p.1 - c.1)
            let gain = pow(2, stops * pow(min(1, distance / radius), falloff))
            let wrongDistance = hypot(Double(x) + 0.5 - wrongCenter.0,
                Double(y) + 0.5 - wrongCenter.1)
            let wrongGain = pow(2, stops * pow(min(1, wrongDistance / wrongRadius), falloff))
            for channel in 0..<3 {
                let encoded = Double(input[index + channel]) / 65535
                let linear = transfers[channel].decode(encoded)
                registered[index + channel] = UInt16((transfers[channel].encode(linear * gain)
                    * 65535).rounded())
                let oldPolicy = Int((transfers[channel].encode(linear * wrongGain) * 65535).rounded())
                wrongMaximum = max(wrongMaximum, abs(Int(registered[index + channel]) - oldPolicy))
                if input[index + channel] != 0 && input[index + channel] != 65535 {
                    distinctInputs += 1
                }
            }
            registered[index + 3] = input[index + 3]
        }
    }
    guard distinctInputs > 1024, wrongMaximum >= 128 else {
        throw ReferenceFailure.invalid("Registered vignette fixture is vacuous: \(distinctInputs) inputs, \(wrongMaximum) code policy delta")
    }
    var expected = registered
    if !optical.isEmpty {
        expected = [UInt16](repeating: 0, count: registered.count)
        for y in 0..<height {
            for x in 0..<width {
                let destination = (y * width + x) * 4
                var channelValues = [Double](repeating: 0, count: 3)
                var channelAlpha = [Double](repeating: 0, count: 3)
                for channel in 0..<3 {
                    let p = try mapped(optical, x: Double(x) + 0.5,
                        y: Double(y) + 0.5, channel: channel,
                        width: width, height: height)
                    let baseX = Int(floor(p.0 - 0.5)), baseY = Int(floor(p.1 - 0.5))
                    for ty in 0..<2 {
                        for tx in 0..<2 {
                            let sx = baseX + tx, sy = baseY + ty
                            guard sx >= 0, sy >= 0, sx < width, sy < height else { continue }
                            let weight = max(0, 1 - abs(p.0 - 0.5 - Double(sx)))
                                * max(0, 1 - abs(p.1 - 0.5 - Double(sy)))
                            let source = (sy * width + sx) * 4
                            let alpha = Double(registered[source + 3]) / 65535
                            channelValues[channel] += Double(registered[source + channel])
                                / 65535 * alpha * weight
                            channelAlpha[channel] += alpha * weight
                        }
                    }
                    if channelAlpha[channel] > 0 {
                        expected[destination + channel] = UInt16((min(1,
                            channelValues[channel] / channelAlpha[channel]) * 65535).rounded())
                    }
                }
                expected[destination + 3] = UInt16((min(1, channelAlpha[1]) * 65535).rounded())
                if expected[destination + 3] == 0 {
                    expected[destination] = 0
                    expected[destination + 1] = 0
                    expected[destination + 2] = 0
                }
            }
        }
    }
    var maximum = 0, compared = 0, displaced = 0
    for index in expected.indices {
        let delta = abs(Int(expected[index]) - Int(output[index]))
        maximum = max(maximum, delta)
        if index % 4 != 3 { compared += 1 }
        if !optical.isEmpty && abs(Int(output[index]) - Int(registered[index])) >= 128 {
            displaced += 1
        }
    }
    guard maximum <= 3, optical.isEmpty || displaced > 128 else {
        throw ReferenceFailure.invalid("\(name)-\(depth) actual pre-vignette/ICC source error \(maximum), optical displacement \(displaced)")
    }
    return ["name": name, "depth": depth, "compared_components": compared,
        "max_native_error": maximum, "bound_u16": 3,
        "old_policy_difference": wrongMaximum, "nonneutral_optical_components": displaced,
        "actual_input_components": input.count]
}
@main struct Main {
    static func main() throws {
        guard CommandLine.arguments.count == 3 else {
            throw ReferenceFailure.invalid("Usage: geometry-reference host-report.json output.json")
        }
        let report = try JSONSerialization.jsonObject(with: Data(contentsOf:
            URL(fileURLWithPath: CommandLine.arguments[1]))) as? [String: Any] ?? [:]
        let geometry = report["geometry"] as? [[String: Any]] ?? []
        var reports: [[String: Any]] = []
        var sourceReports: [[String: Any]] = []
        for group in geometry {
            for row in group["groups"] as? [[String: Any]] ?? [] {
                guard let name = row["name"] as? String,
                      let depth = row["depth"] as? Int,
                      let inputPath = row["old_mask"] as? String,
                      let outputPath = row["after_mask"] as? String,
                      let matrix = row["inverse"] as? [Double], matrix.count == 6 else {
                    throw ReferenceFailure.invalid("Incomplete host geometry row")
                }
                let input = try Plane(inputPath)
                let output = try Plane(outputPath)
                let filtered = name == "registered-vignette-affine-chain"
                var maximum = 0, distinct = 0
                for y in 0..<output.height {
                    for x in 0..<output.width {
                        let expected = reference(input, matrix, x, y, filtered: filtered)
                        let actual = output.at(x, y)
                        maximum = max(maximum, abs(Int(expected) - Int(actual)))
                        if x < input.width && y < input.height {
                            distinct = max(distinct, abs(Int(input.at(x, y)) - Int(actual)))
                        }
                    }
                }
                guard maximum <= 1, distinct >= 128 else {
                    throw ReferenceFailure.invalid("\(name)-\(depth) mask error \(maximum), nonvacuity \(distinct)")
                }
                reports.append(["name": name, "depth": depth,
                    "compared": output.width * output.height,
                    "max_mask_error": maximum, "nonvacuity": distinct])
                sourceReports.append(try sourcePixels(row))
            }
        }
        let names = ["registered-vignette-crop", "registered-vignette-pad",
            "registered-vignette-exif", "registered-vignette-affine-chain"]
        guard reports.count == 8, sourceReports.count == 8,
              names.allSatisfy({ name in reports.filter { $0["name"] as? String == name }.count == 2 }) else {
            throw ReferenceFailure.invalid("Missing one of the eight native geometry cases")
        }
        let result: [String: Any] = ["passed": true, "reports": reports,
            "actual_input_icc_reports": sourceReports,
            "max_native_error": sourceReports.compactMap { $0["max_native_error"] as? Int }.max() ?? -1,
            "max_mask_error": reports.compactMap { $0["max_mask_error"] as? Int }.max() ?? -1]
        let data = try JSONSerialization.data(withJSONObject: result, options: [.prettyPrinted, .sortedKeys])
        try data.write(to: URL(fileURLWithPath: CommandLine.arguments[2]))
        print("Independent RAW geometry: \(reports.count) native masks, maximum error \(result["max_mask_error"] ?? -1)")
    }
}
