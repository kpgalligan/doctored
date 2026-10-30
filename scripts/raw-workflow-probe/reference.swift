// Independent Phase 8D reference: no app or Rust sources are linked.
// Native camera dimensions and an explicit Display P3 SDR encoding are fixed.
import Foundation
import CoreImage
import ImageIO
import CryptoKit

struct ReferenceFailure: Error { let message: String }
let input = URL(fileURLWithPath: CommandLine.arguments[1])
let directory = URL(fileURLWithPath: CommandLine.arguments[2], isDirectory: true)
try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
guard let p3 = CGColorSpace(name: CGColorSpace.displayP3), let profile = p3.copyICCData() else {
    throw ReferenceFailure(message: "Display P3 profile unavailable")
}
try (profile as Data).write(to: directory.appendingPathComponent("reference.icc"))
let context = CIContext(options: [.workingFormat: CIFormat.RGBAf, .outputPremultiplied: false])
var records = [[String: Any]]()
for variant in ["initial", "redeveloped"] {
    try autoreleasepool {
        guard let filter = CIRAWFilter(imageURL: input) else { throw ReferenceFailure(message: "Camera decoder unavailable") }
        filter.scaleFactor = 1
        filter.isDraftModeEnabled = false
        filter.extendedDynamicRangeAmount = 0
        if variant == "redeveloped" {
            filter.exposure = 0.35
            filter.neutralTemperature = 7000
            filter.neutralTint = 17
        }
        guard let output = filter.outputImage else { throw ReferenceFailure(message: "Camera output unavailable") }
        let extent = output.extent.integral
        let width = Int(extent.width), height = Int(extent.height)
        guard width == 3008, height == 2000 else { throw ReferenceFailure(message: "Unexpected camera dimensions") }
        var pixels = [UInt16](repeating: 0, count: width * height * 4)
        pixels.withUnsafeMutableBytes { buffer in
            if let base = buffer.baseAddress {
                context.render(output, toBitmap: base, rowBytes: width * 8,
                    bounds: extent, format: .RGBA16, colorSpace: p3)
            }
        }
        var nonbyte = 0
        var distinct = Set<UInt16>()
        var outsideSRGB = 0
        // Independent standard Display P3 -> sRGB D65 linear matrix. Values
        // beyond [0,1] by >2/65535 qualify chroma that sRGB cannot retain.
        func linear(_ code: UInt16) -> Double {
            let x = Double(code) / 65535
            return x <= 0.04045 ? x / 12.92 : pow((x + 0.055) / 1.055, 2.4)
        }
        for pixel in 0..<(width * height) {
            let i = pixel * 4
            guard pixels[i + 3] == UInt16.max else { throw ReferenceFailure(message: "Nonopaque/all-zero graphics-service rendering") }
            for component in 0..<3 {
                if pixels[i + component] % 257 != 0 { nonbyte += 1 }
                distinct.insert(pixels[i + component])
            }
            let r = linear(pixels[i]), g = linear(pixels[i + 1]), b = linear(pixels[i + 2])
            let converted = [1.224940176 * r - 0.224940176 * g,
                             -0.042056955 * r + 1.042056955 * g,
                             -0.019637555 * r - 0.078636046 * g + 1.098273601 * b]
            if converted.contains(where: { $0 < -2.0 / 65535 || $0 > 1 + 2.0 / 65535 }) { outsideSRGB += 1 }
        }
        guard nonbyte > width * height, distinct.count > 4096 else { throw ReferenceFailure(message: "Insufficient native precision") }
        let data = pixels.withUnsafeBytes { Data($0) }
        try data.write(to: directory.appendingPathComponent("\(variant)-rgba16le.bin"))
        records.append(["variant": variant, "width": width, "height": height,
            "decoder": filter.decoderVersion.rawValue, "nonbyte_rgb_components": nonbyte,
            "distinct_rgb_codes": distinct.count, "pixels_outside_srgb": outsideSRGB,
            "samples_sha256": SHA256.hash(data: data).map { String(format: "%02x", $0) }.joined()])
    }
}
let report: [String: Any] = ["passed": true, "engine": "Independent Core Image camera reference, no app/core code",
    "output": "Normalized SDR Display P3 RGBA16; native dimensions; float working buffer",
    "os": ProcessInfo.processInfo.operatingSystemVersionString, "records": records]
try JSONSerialization.data(withJSONObject: report, options: [.prettyPrinted, .sortedKeys])
    .write(to: directory.appendingPathComponent("reference-report.json"))
print("PASS: independent native camera Display P3 RGBA16 reference")
