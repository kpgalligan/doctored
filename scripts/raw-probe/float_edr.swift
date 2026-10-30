// Measurement M1 of the 32-bit float (linear) Camera RAW development: which
// `extendedDynamicRangeAmount` the float recipe pins. This compiles without any
// app/core source (it is not in the `raw-probe` probe set); `float_edr.py`
// builds it, runs it and applies the bounds.
//
// It renders the pair that ships. The reference is what a 16-bit `tone_curve: 0`
// recipe applies: `.RGBA16` into sRGB, `extendedDynamicRangeAmount` 0,
// `boostAmount` 0, `localToneMapAmount` the file's own. Each candidate is what
// the float recipe applies: `.RGBAf` into extended linear sRGB, `boostAmount` 0,
// `localToneMapAmount` 0 where the decoder supports it, at amount 0, 1 and 2.
// Both turn Profile Corrections on where the file carries a lens profile, as a
// new development does (`RawCapabilities.developDefaults`).
//
// Output: `lattice.json`, a 32×20 lattice of 5×5 means per render. A reference
// point is the mean u16 code per channel. A float point is the mean linear
// sample per channel (`linear`) and the mean of the per-pixel sRGB encoding of
// the sample clamped to 0…1 (`encoded`), which is what a u16 code averages.
import Foundation
import CoreImage
import ImageIO

struct ProbeFailure: Error { let message: String }

let columns = 32, rows = 20, window = 5
let exposures: [Float] = [0, 1, 2]
let amounts: [Float] = [0, 1, 2]

func encode(_ linear: Float) -> Double {
    let value = Double(min(max(linear, 0), 1))
    return value <= 0.0031308 ? value * 12.92 : 1.055 * pow(value, 1 / 2.4) - 0.055
}

func origins(width: Int, height: Int) -> [(Int, Int)] {
    var points = [(Int, Int)]()
    for row in 0..<rows {
        for column in 0..<columns {
            let x = Int((Double(column) + 0.5) * Double(width) / Double(columns)) - window / 2
            let y = Int((Double(row) + 0.5) * Double(height) / Double(rows)) - window / 2
            points.append((min(max(x, 0), width - window), min(max(y, 0), height - window)))
        }
    }
    return points
}

func prepared(_ input: URL, exposure: Float, amount: Float, float: Bool) throws -> CIRAWFilter {
    guard let filter = CIRAWFilter(imageURL: input),
          filter.supportedDecoderVersions.contains(where: { $0 != .none }) else {
        throw ProbeFailure(message: "Camera decoder unavailable")
    }
    filter.scaleFactor = 1
    filter.isDraftModeEnabled = false
    filter.exposure = exposure
    filter.boostAmount = 0
    filter.extendedDynamicRangeAmount = amount
    if filter.isLensCorrectionSupported { filter.isLensCorrectionEnabled = true }
    if float, filter.isLocalToneMapSupported { filter.localToneMapAmount = 0 }
    return filter
}

let arguments = CommandLine.arguments
guard arguments.count == 3 else { throw ProbeFailure(message: "usage: float_edr <camera file> <output directory>") }
let input = URL(fileURLWithPath: arguments[1])
let directory = URL(fileURLWithPath: arguments[2], isDirectory: true)
guard let sRGB = CGColorSpace(name: CGColorSpace.sRGB),
      let linear = CGColorSpaceCreateExtendedLinearized(sRGB) else {
    throw ProbeFailure(message: "sRGB spaces unavailable")
}
try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
let options: [CIContextOption: Any] = [.workingFormat: CIFormat.RGBAf, .outputPremultiplied: false]

guard let probe = CIRAWFilter(imageURL: input) else { throw ProbeFailure(message: "Camera decoder unavailable") }
let localToneMapSupported = probe.isLocalToneMapSupported
let fileLocalToneMap = probe.localToneMapAmount

var references = [[String: Any]]()
var candidates = [[String: Any]]()
var size = (0, 0)
for exposure in exposures {
    try autoreleasepool {
        let filter = try prepared(input, exposure: exposure, amount: 0, float: false)
        guard let output = filter.outputImage else { throw ProbeFailure(message: "RAW output unavailable") }
        let extent = output.extent.integral
        let width = Int(extent.width), height = Int(extent.height)
        size = (width, height)
        var samples = [UInt16](repeating: 0, count: width * height * 4)
        let context = CIContext(options: options)
        samples.withUnsafeMutableBytes { storage in
            if let base = storage.baseAddress {
                context.render(output, toBitmap: base, rowBytes: width * 8, bounds: extent,
                               format: .RGBA16, colorSpace: sRGB)
            }
        }
        context.clearCaches()
        guard stride(from: 3, to: samples.count, by: 4).allSatisfy({ samples[$0] == UInt16.max }) else {
            throw ProbeFailure(message: "Core Image did not render opaque camera pixels; run with graphics service access")
        }
        var points = [[Double]]()
        for (x, y) in origins(width: width, height: height) {
            var sums = [Double](repeating: 0, count: 3)
            for dy in 0..<window {
                for dx in 0..<window {
                    let at = ((y + dy) * width + x + dx) * 4
                    for channel in 0..<3 { sums[channel] += Double(samples[at + channel]) }
                }
            }
            points.append(sums.map { $0 / Double(window * window) })
        }
        references.append(["exposure": Double(exposure), "codes": points,
                           "local_tone_map": Double(filter.localToneMapAmount)])
    }
    for amount in amounts {
        try autoreleasepool {
            let filter = try prepared(input, exposure: exposure, amount: amount, float: true)
            guard let output = filter.outputImage else { throw ProbeFailure(message: "RAW output unavailable") }
            let extent = output.extent.integral
            let width = Int(extent.width), height = Int(extent.height)
            guard (width, height) == size else { throw ProbeFailure(message: "The float extent differs from the reference") }
            var samples = [Float](repeating: 0, count: width * height * 4)
            let context = CIContext(options: options)
            samples.withUnsafeMutableBytes { storage in
                if let base = storage.baseAddress {
                    context.render(output, toBitmap: base, rowBytes: width * 16, bounds: extent,
                                   format: .RGBAf, colorSpace: linear)
                }
            }
            context.clearCaches()
            var finite = true, opaque = true, largest: Float = 0, smallest: Float = 0
            for index in samples.indices {
                let value = samples[index]
                if !value.isFinite { finite = false; continue }
                if index % 4 == 3 {
                    if value != 1 { opaque = false }
                } else {
                    largest = max(largest, value)
                    smallest = min(smallest, value)
                }
            }
            guard finite else { throw ProbeFailure(message: "The float rendering holds a non-finite sample") }
            var linearPoints = [[Double]](), encodedPoints = [[Double]]()
            for (x, y) in origins(width: width, height: height) {
                var sums = [Double](repeating: 0, count: 3), encoded = [Double](repeating: 0, count: 3)
                for dy in 0..<window {
                    for dx in 0..<window {
                        let at = ((y + dy) * width + x + dx) * 4
                        for channel in 0..<3 {
                            sums[channel] += Double(samples[at + channel])
                            encoded[channel] += encode(samples[at + channel])
                        }
                    }
                }
                linearPoints.append(sums.map { $0 / Double(window * window) })
                encodedPoints.append(encoded.map { $0 / Double(window * window) })
            }
            candidates.append(["exposure": Double(exposure), "amount": Double(amount), "linear": linearPoints,
                               "encoded": encodedPoints, "opaque": opaque, "largest": Double(largest),
                               "smallest": Double(smallest), "local_tone_map": Double(filter.localToneMapAmount),
                               "edr_read_back": Double(filter.extendedDynamicRangeAmount)])
        }
    }
}
let report: [String: Any] = [
    "engine": "Independent Core Image measurement, no app/core code",
    "os": ProcessInfo.processInfo.operatingSystemVersionString,
    "decoder": probe.decoderVersion.rawValue, "width": size.0, "height": size.1,
    "columns": columns, "rows": rows, "window": window,
    "is_local_tone_map_supported": localToneMapSupported, "file_local_tone_map": Double(fileLocalToneMap),
    "references": references, "candidates": candidates]
try JSONSerialization.data(withJSONObject: report, options: [.sortedKeys])
    .write(to: directory.appendingPathComponent("lattice.json"))
print("float_edr: wrote lattice.json (\(size.0)x\(size.1))")
