// Independent camera decoder reference. This compiles without any app/core source.
// It renders the actual camera file at native size directly into RGBA16, then
// applies only the three public controls requested by the qualification workflow.
import Foundation
import CoreImage
import ImageIO
import CryptoKit

struct ReferenceFailure: Error { let message: String }
let arguments = CommandLine.arguments
let input = URL(fileURLWithPath: arguments[1])
let directory = URL(fileURLWithPath: arguments[2], isDirectory: true)
guard let space = CGColorSpace(name: CGColorSpace.sRGB),
      let profile = space.copyICCData() else { throw ReferenceFailure(message: "sRGB ICC unavailable") }
try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
try (profile as Data).write(to: directory.appendingPathComponent("reference.icc"))
let context = CIContext(options: [.workingFormat: CIFormat.RGBAf, .outputPremultiplied: false])
var records = [[String: Any]]()
for variant in ["initial", "redeveloped"] {
    try autoreleasepool {
        guard let filter = CIRAWFilter(imageURL: input),
              filter.supportedDecoderVersions.contains(where: { $0 != .none }) else {
            throw ReferenceFailure(message: "Camera decoder unavailable")
        }
        filter.scaleFactor = 1
        filter.isDraftModeEnabled = false
        filter.extendedDynamicRangeAmount = 0
        if variant == "redeveloped" {
            filter.exposure = 0.35
            filter.neutralTemperature = 7000
            filter.neutralTint = 17
        }
        guard let output = filter.outputImage else { throw ReferenceFailure(message: "RAW output unavailable") }
        let extent = output.extent.integral
        let width = Int(extent.width), height = Int(extent.height)
        guard width == 3008, height == 2000 else { throw ReferenceFailure(message: "Native dimensions changed") }
        var samples = [UInt16](repeating: 0, count: width * height * 4)
        samples.withUnsafeMutableBytes { storage in
            if let base = storage.baseAddress {
                context.render(output, toBitmap: base, rowBytes: width * 8,
                               bounds: extent, format: .RGBA16, colorSpace: space)
            }
        }
        var nonbyte = 0
        var distinct = Set<UInt16>()
        for pixel in 0..<(width * height) {
            guard samples[pixel * 4 + 3] == UInt16.max else {
                throw ReferenceFailure(message: "Core Image did not render opaque camera pixels; run with graphics service access")
            }
            for component in 0..<3 {
                let value = samples[pixel * 4 + component]
                if value % 257 != 0 { nonbyte += 1 }
                distinct.insert(value)
            }
        }
        guard nonbyte > width * height, distinct.count > 4096 else {
            throw ReferenceFailure(message: "Camera reference lacks native high-depth signal")
        }
        let bytes = samples.withUnsafeBytes { Data($0) }
        let path = directory.appendingPathComponent("\(variant)-rgba16le.bin")
        try bytes.write(to: path)
        records.append(["variant": variant, "width": width, "height": height,
            "decoder": filter.decoderVersion.rawValue,
            "supported_decoders": filter.supportedDecoderVersions.map(\.rawValue),
            "nonbyte_rgb_components": nonbyte, "distinct_rgb_codes": distinct.count,
            "samples_sha256": SHA256.hash(data: bytes).map { String(format: "%02x", $0) }.joined(),
            "exposure": filter.exposure, "temperature": filter.neutralTemperature,
            "tint": filter.neutralTint])
    }
}
let report: [String: Any] = ["passed": true, "engine": "Independent Core Image reference, no app/core code",
    "output": "opaque normalized SDR sRGB RGBA16, native resolution, float working format",
    "os": ProcessInfo.processInfo.operatingSystemVersionString, "records": records]
try JSONSerialization.data(withJSONObject: report, options: [.prettyPrinted, .sortedKeys])
    .write(to: directory.appendingPathComponent("reference-report.json"))
print("PASS: independent genuine camera native RGBA16 reference, default and WB/exposure development")
