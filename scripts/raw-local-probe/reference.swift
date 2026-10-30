// Standalone Phase 8G camera oracle: no app or Rust code is compiled here.
import Foundation
import CoreImage
import ImageIO
import CryptoKit

struct ReferenceFailure: Error { let message: String }
do {
let arguments = CommandLine.arguments
guard arguments.count == 3 else { throw ReferenceFailure(message: "reference jobs.json output-directory") }
let jobsURL = URL(fileURLWithPath: arguments[1])
let directory = URL(fileURLWithPath: arguments[2], isDirectory: true)
let jobs = try JSONSerialization.jsonObject(with: Data(contentsOf: jobsURL))
guard let requests = jobs as? [[String: Any]], requests.count <= 32 else { throw ReferenceFailure(message: "Invalid bounded camera jobs") }
try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
let context = CIContext(options: [.workingFormat: CIFormat.RGBAf, .outputPremultiplied: false])
var records: [[String: Any]] = []
for request in requests {
    try autoreleasepool {
        guard let path = request["path"] as? String, let name = request["name"] as? String,
              let spaceName = request["space"] as? String, ["srgb", "display_p3"].contains(spaceName),
              let filter = CIRAWFilter(imageURL: URL(fileURLWithPath: path)),
              let space = CGColorSpace(name: spaceName == "srgb" ? CGColorSpace.sRGB : CGColorSpace.displayP3),
              let profile = space.copyICCData() else { throw ReferenceFailure(message: "Reference input/decoder/space unavailable") }
        let start = Date()
        let baseline = filter.baselineExposure
        filter.isDraftModeEnabled = false
        filter.scaleFactor = 1
        filter.extendedDynamicRangeAmount = 0
        filter.exposure = Float((request["exposure"] as? NSNumber)?.doubleValue ?? 0)
        let compensation = (request["baseline_compensation"] as? NSNumber)?.doubleValue ?? 0
        filter.baselineExposure = Float(Double(baseline) + compensation)
        if let amount = request["moire"] as? NSNumber {
            guard filter.isMoireReductionSupported else { throw ReferenceFailure(message: "Reference moire unsupported") }
            filter.moireReductionAmount = amount.floatValue
        }
        if let temperature = request["temperature"] as? NSNumber,
           let tint = request["tint"] as? NSNumber {
            filter.neutralTemperature = temperature.floatValue
            filter.neutralTint = tint.floatValue
        }
        // Canonicalize the coupled white balance once, independently of product
        // recipes. This is the same explicitly declared saved-XY contract.
        filter.neutralChromaticity = filter.neutralChromaticity
        guard let output = filter.outputImage else { throw ReferenceFailure(message: "No complete camera image") }
        let bounds = output.extent.integral
        let width = Int(bounds.width), height = Int(bounds.height)
        guard width > 0, height > 0, width * height <= 64_000_000 else { throw ReferenceFailure(message: "Unbounded camera extent") }
        var pixels = [UInt16](repeating: 0, count: width * height * 4)
        pixels.withUnsafeMutableBytes { raw in
            if let address = raw.baseAddress {
                context.render(output, toBitmap: address, rowBytes: width * 8,
                    bounds: bounds, format: .RGBA16, colorSpace: space)
            }
        }
        var distinct = Set<UInt16>(), nonbyte = 0, clipped = 0
        for index in stride(from: 0, to: pixels.count, by: 4) {
            guard pixels[index + 3] == 65535 else { throw ReferenceFailure(message: "Nonopaque/denied camera rendering") }
            for channel in 0..<3 {
                let value = pixels[index + channel]
                distinct.insert(value)
                if value % 257 != 0 { nonbyte += 1 }
                if value == 65535 { clipped += 1 }
            }
        }
        if name.hasPrefix("initial-") || name.hasPrefix("enhanced-") {
            guard distinct.count > 4096, nonbyte > width * height else {
                throw ReferenceFailure(message: "Unclipped calibration frame lacks genuine native sensor detail")
            }
        }
        if name.hasPrefix("clipped-") {
            guard clipped > width * height else { throw ReferenceFailure(message: "Highlight reference did not intentionally clip enough channels") }
        }
        let bytes = pixels.withUnsafeBytes { Data($0) }
        try bytes.write(to: directory.appendingPathComponent(name + ".rgba16le"))
        try (profile as Data).write(to: directory.appendingPathComponent(name + ".icc"))
        records.append(["name": name, "width": width, "height": height,
            "original_sha256": SHA256.hash(data: try Data(contentsOf: URL(fileURLWithPath: path))).map { String(format: "%02x", $0) }.joined(),
            "sample_sha256": SHA256.hash(data: bytes).map { String(format: "%02x", $0) }.joined(),
            "profile_sha256": SHA256.hash(data: profile as Data).map { String(format: "%02x", $0) }.joined(),
            "decoder": filter.decoderVersion.rawValue, "orientation": filter.orientation.rawValue,
            "baseline_reference": baseline, "baseline_effective": filter.baselineExposure,
            "moire_supported": filter.isMoireReductionSupported, "moire": filter.moireReductionAmount,
            "vendor_lens_supported": filter.isLensCorrectionSupported, "local_tone_map_supported": filter.isLocalToneMapSupported,
            "temperature": filter.neutralTemperature, "tint": filter.neutralTint,
            "chromaticity": [filter.neutralChromaticity.x, filter.neutralChromaticity.y],
            "exposure": filter.exposure, "space": spaceName, "elapsed_seconds": Date().timeIntervalSince(start),
            "distinct_rgb_codes": distinct.count, "nonbyte_rgb_components": nonbyte,
            "clipped_rgb_components": clipped, "request": request])
        FileHandle.standardOutput.write(Data("Independent camera reference: \(name) \(width)×\(height), clipped=\(clipped), native=\(nonbyte)\n".utf8))
    }
}
try JSONSerialization.data(withJSONObject: ["sdk_only": true,
    "os": ProcessInfo.processInfo.operatingSystemVersionString, "camera_comparison_bound_u16": 3,
    "records": records], options: [.sortedKeys, .prettyPrinted])
    .write(to: directory.appendingPathComponent("reference.json"))

} catch {
    FileHandle.standardError.write(Data("Independent RAW reference failed: \(error)\n".utf8))
    exit(1)
}
