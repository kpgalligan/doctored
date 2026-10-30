import Foundation
import ImageIO
import CoreImage
import CoreVideo
import AVFoundation

// Standalone synthetic codec fixtures, with no product source linked. The
// reference reads ImageIO's unrotated auxiliary payload directly; the Python
// oracle independently applies EXIF, depth normalization and scalar resampling.
func require<T>(_ value: T?, _ text: String) throws -> T {
    guard let value else { throw NSError(domain: "AuxiliaryFixture", code: 1,
        userInfo: [NSLocalizedDescriptionKey: text]) }
    return value
}

let directory = URL(fileURLWithPath: CommandLine.arguments.dropFirst().first
    ?? "/private/tmp/rz-phase9g-auxiliary-fixtures", isDirectory: true)
try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
let width = 64, height = 48, matteWidth = 32, matteHeight = 24
var rgba: [UInt16] = []
for y in 0..<height { for x in 0..<width {
    let red = UInt16(5000 + x * 701), green = UInt16(7000 + y * 997)
    let blue = UInt16(12000 + (x + y) * 311)
    rgba.append(contentsOf: [red, green, blue, UInt16.max])
} }
let space = try require(CGColorSpace(name: CGColorSpace.sRGB), "sRGB unavailable")
let opaque = CIImage(bitmapData: rgba.withUnsafeBytes { Data($0) }, bytesPerRow: width * 8,
    size: CGSize(width: width, height: height), format: .RGBA16, colorSpace: space)
// An actual alpha plane selects ImageIO's qualified unpacked RGBA16 decoder.
// Keep the opaque counterpart below as an explicit packed-RGB10 refusal case.
rgba[3] = 32768
let image = CIImage(bitmapData: rgba.withUnsafeBytes { Data($0) }, bytesPerRow: width * 8,
    size: CGSize(width: width, height: height), format: .RGBA16, colorSpace: space)
var coverage: [UInt8] = []
for y in 0..<matteHeight { for x in 0..<matteWidth {
    coverage.append(UInt8((x * 17 + y * 23) % 256))
} }
let matteInfo: [AnyHashable: Any] = [
    kCGImageAuxiliaryDataInfoData as String: Data(coverage),
    kCGImageAuxiliaryDataInfoDataDescription as String: ["Width": matteWidth,
        "Height": matteHeight, "BytesPerRow": matteWidth, "PixelFormat": kCVPixelFormatType_OneComponent8],
    kCGImageAuxiliaryDataInfoMetadata as String: CGImageMetadataCreateMutable()]
let matte = try AVPortraitEffectsMatte(fromDictionaryRepresentation: matteInfo)
let floats = (0..<(matteWidth * matteHeight)).map { Float($0) / Float(matteWidth * matteHeight - 1) + 0.5 }
let depthInfo: [AnyHashable: Any] = [
    kCGImageAuxiliaryDataInfoData as String: floats.withUnsafeBytes { Data($0) },
    kCGImageAuxiliaryDataInfoDataDescription as String: ["Width": matteWidth,
        "Height": matteHeight, "BytesPerRow": matteWidth * 4, "PixelFormat": kCVPixelFormatType_DisparityFloat32],
    kCGImageAuxiliaryDataInfoMetadata as String: CGImageMetadataCreateMutable()]
let depth = try AVDepthData(fromDictionaryRepresentation: depthInfo)
let context = CIContext(options: [.workingFormat: CIFormat.RGBAf.rawValue, .cacheIntermediates: false])
let options: [CIImageRepresentationOption: Any] = [.avPortraitEffectsMatte: matte, .avDepthData: depth]
let opaqueName = "opaque-packed10.heic"
try context.writeHEIF10Representation(of: opaque, to: directory.appendingPathComponent(opaqueName),
    colorSpace: space, options: options)
var records: [[String: Any]] = []
for orientation in 1...8 {
    let name = "matte-\(orientation).heic"
    let url = directory.appendingPathComponent(name)
    try context.writeHEIF10Representation(of: image.settingProperties([
        kCGImagePropertyOrientation as String: orientation]), to: url, colorSpace: space, options: options)
    let source = try require(CGImageSourceCreateWithURL(url as CFURL, nil), "Could not read HEIC fixture")
    let properties = try require(CGImageSourceCopyPropertiesAtIndex(source, 0, nil) as? [String: Any], "No HEIC properties")
    guard (properties[kCGImagePropertyDepth as String] as? NSNumber)?.intValue == 10,
          (properties[kCGImagePropertyOrientation as String] as? NSNumber)?.intValue == orientation else {
        throw NSError(domain: "AuxiliaryFixture", code: 2, userInfo: [NSLocalizedDescriptionKey: "HEIC depth/orientation differs"])
    }
    let decoded = try require(CGImageSourceCreateImageAtIndex(source, 0,
        [kCGImageSourceShouldAllowFloat: true, kCGImageSourceDecodeRequest: kCGImageSourceDecodeToHDR,
         kCGImageSourceGenerateImageSpecificLumaScaling: false] as CFDictionary), "Missing full decode")
    guard decoded.bitsPerComponent == 16, decoded.bitsPerPixel == 64,
          !decoded.bitmapInfo.contains(.floatComponents) else {
        throw NSError(domain: "AuxiliaryFixture", code: 3, userInfo: [NSLocalizedDescriptionKey: "Fixture needs the qualified integer RGBA16 layout"])
    }
    var planes: [[String: Any]] = []
    for (kind, label) in [(kCGImageAuxiliaryDataTypeDisparity, "Depth"),
                           (kCGImageAuxiliaryDataTypePortraitEffectsMatte, "Portrait Matte")] {
        let info = try require(CGImageSourceCopyAuxiliaryDataInfoAtIndex(source, 0, kind) as? [String: Any], "Missing \(label)")
        let description = try require(info[kCGImageAuxiliaryDataInfoDataDescription as String] as? [String: Any], "Missing scalar description")
        let data = try require(info[kCGImageAuxiliaryDataInfoData as String] as? Data, "Missing scalar payload")
        let file = "\(orientation)-\(label.replacingOccurrences(of: " ", with: "-")).raw"
        try data.write(to: directory.appendingPathComponent(file))
        planes.append(["name": label, "raw_file": file, "description": description])
    }
    records.append(["file": name, "width": width, "height": height, "orientation": orientation,
        "image_depth": 10, "planes": planes])
}
let receipt = try JSONSerialization.data(withJSONObject: ["cases": records,
    "refused_opaque": opaqueName, "opaque_boundary": "packed integer RGB10 is outside the existing host-layout contract"], options: [.prettyPrinted, .sortedKeys])
try receipt.write(to: directory.appendingPathComponent("fixtures.json"))
print(directory.appendingPathComponent("fixtures.json").path)
