import AppKit
import ImageIO
import UniformTypeIdentifiers
import Darwin

struct ProbeFailure: Error, CustomStringConvertible {
    let description: String
}
func require(_ condition: @autoclosure () -> Bool, _ message: String) throws {
    if !condition() { throw ProbeFailure(description: message) }
}
let probeDirectory = URL(fileURLWithPath:
    ProcessInfo.processInfo.environment["DR_COLOR_PROBE_DIR"] ?? NSTemporaryDirectory())

func independentProperties(_ url: URL) throws -> [String: Any] {
    guard let source = CGImageSourceCreateWithURL(url as CFURL, nil),
          let properties = CGImageSourceCopyPropertiesAtIndex(source, 0, nil) as? [String: Any]
    else { throw ProbeFailure(description: "ImageIO cannot read \(url.lastPathComponent)") }
    return properties
}

func fixtureImage() throws -> CGImage {
    var values: [UInt8] = []
    for i in 0..<(64 * 48) {
        let red = UInt8(30 + i % 100)
        let green = UInt8(70 + i % 80)
        let blue = UInt8(40 + i % 150)
        values.append(contentsOf: [red, green, blue, 255])
    }
    guard let space = CGColorSpace(name: CGColorSpace.displayP3),
          let provider = CGDataProvider(data: Data(values) as CFData),
          let image = CGImage(width: 64, height: 48, bitsPerComponent: 8, bitsPerPixel: 32,
            bytesPerRow: 64 * 4, space: space,
            bitmapInfo: CGBitmapInfo(rawValue: CGImageAlphaInfo.last.rawValue),
            provider: provider, decode: nil, shouldInterpolate: false, intent: .defaultIntent)
    else { throw ProbeFailure(description: "P3 pixel fixture") }
    return image
}

func captureProperties() -> [String: Any] {
    [
        kCGImagePropertyOrientation as String: 6,
        kCGImagePropertyDPIWidth as String: 300,
        kCGImagePropertyDPIHeight as String: 150,
        kCGImagePropertyTIFFDictionary as String: [
            kCGImagePropertyTIFFMake as String: "Phase Three Camera",
            kCGImagePropertyTIFFModel as String: "Metadata Fixture",
            kCGImagePropertyTIFFArtist as String: "Phase Three Photographer",
            kCGImagePropertyTIFFCopyright as String: "Copyright Phase Three",
        ],
        kCGImagePropertyExifDictionary as String: [
            kCGImagePropertyExifDateTimeOriginal as String: "2026:09:09 12:34:56",
            kCGImagePropertyExifExposureTime as String: 0.008,
            kCGImagePropertyExifFNumber as String: 5.6,
            kCGImagePropertyExifISOSpeedRatings as String: [200],
        ],
        kCGImagePropertyGPSDictionary as String: [
            kCGImagePropertyGPSLatitude as String: 40.7,
            kCGImagePropertyGPSLatitudeRef as String: "N",
            kCGImagePropertyGPSLongitude as String: 74.0,
            kCGImagePropertyGPSLongitudeRef as String: "W",
        ],
        kCGImagePropertyIPTCDictionary as String: [
            kCGImagePropertyIPTCCaptionAbstract as String: "Phase Three Capture",
            kCGImagePropertyIPTCKeywords as String: ["photograph", "metadata"],
        ],
    ]
}

func writeImageIOFixture(_ url: URL, type: UTType) throws {
    guard let destination = CGImageDestinationCreateWithURL(url as CFURL,
        type.identifier as CFString, 1, nil) else {
        throw ProbeFailure(description: "ImageIO cannot write \(type.identifier)")
    }
    CGImageDestinationAddImage(destination, try fixtureImage(), captureProperties() as CFDictionary)
    try require(CGImageDestinationFinalize(destination), "ImageIO fixture finalize")
}

func verifyCapture(_ url: URL, dpi: (Double, Double), iptc: Bool = true) throws {
    let values = try independentProperties(url)
    let exif = values[kCGImagePropertyExifDictionary as String] as? [String: Any]
    let tiff = values[kCGImagePropertyTIFFDictionary as String] as? [String: Any]
    let gps = values[kCGImagePropertyGPSDictionary as String] as? [String: Any]
    let metadata = values[kCGImagePropertyIPTCDictionary as String] as? [String: Any]
    try require(tiff?[kCGImagePropertyTIFFMake as String] as? String == "Phase Three Camera", "camera make \(url)")
    try require(exif?[kCGImagePropertyExifDateTimeOriginal as String] as? String == "2026:09:09 12:34:56", "capture date \(url)")
    try require(abs((exif?[kCGImagePropertyExifExposureTime as String] as? Double ?? 0) - 0.008) < 0.00001, "exposure time \(url)")
    try require(abs((gps?[kCGImagePropertyGPSLatitude as String] as? Double ?? 0) - 40.7) < 0.00001, "GPS \(url)")
    if iptc {
        try require(metadata?[kCGImagePropertyIPTCCaptionAbstract as String] as? String == "Phase Three Capture", "IPTC caption \(url)")
    }
    let orientation = values[kCGImagePropertyOrientation as String] as? Int ?? 1
    try require(orientation == 1, "upright export orientation \(url): \(orientation)")
    let dx = values[kCGImagePropertyDPIWidth as String] as? Double ?? 0
    let dy = values[kCGImagePropertyDPIHeight as String] as? Double ?? 0
    try require(abs(dx - dpi.0) < 0.02 && abs(dy - dpi.1) < 0.02, "upright PPI \(url): \(dx), \(dy)")
}

func verifyTIFFAndPlatformCapture() throws {
    let tiff = probeDirectory.appendingPathComponent("p3-source.tiff")
    try writeImageIOFixture(tiff, type: .tiff)
    var doc = try RasterDocument.open(url: tiff)
    try require(doc.width == 48 && doc.height == 64, "TIFF orientation dimensions")
    try require(abs(doc.resolution.0 - 150) < 0.001 && abs(doc.resolution.1 - 300) < 0.001, "TIFF orientation PPI")
    let profile = doc.iccProfile
    try require(profile != nil && doc.profileName.lowercased().contains("p3"), "TIFF P3 import")
    if let sized = try doc.settingResolution(x: 300, y: 300) { doc = sized }
    let out = probeDirectory.appendingPathComponent("p3-roundtrip.tiff")
    let report = try doc.saveImage(nil, to: out, format: DR_FORMAT_TIFF, jpegQuality: 90,
                                  embedProfile: true, stripMetadata: false)
    try require(report.carries(DR_CARRIES_PROFILE) && report.carries(DR_CARRIES_RESOLUTION), "TIFF profile/PPI report")
    try verifyCapture(out, dpi: (300, 300))
    let reopened = try RasterDocument.open(url: out)
    try require(reopened.iccProfile == profile, "TIFF exact embedded profile survives")
    try require(reopened.resolution.0 == 300 && reopened.resolution.1 == 300, "TIFF roundtrip 300 PPI")
    guard let independent = CGImageSourceCreateWithURL(out as CFURL, nil),
          let picture = CGImageSourceCreateImageAtIndex(independent, 0, nil),
          let space = picture.colorSpace, let icc = space.copyICCData() as Data?,
          let profile = profile else { throw ProbeFailure(description: "Independent TIFF profile") }
    try require(RasterProfile.describesSameSpace(icc, profile), "Independent reader recognizes P3")
    print("PASS: independent P3 TIFF open/save/open, 300 PPI, orientation and capture fields")

    let heic = probeDirectory.appendingPathComponent("capture.heic")
    try writeImageIOFixture(heic, type: .heic)
    let document = ImageDocument()
    try document.read(from: heic, ofType: UTType.heic.identifier)
    try require(document.doc.width == 48 && document.doc.height == 64, "HEIC orientation dimensions")
    try require(!document.metadataNotCaptured && document.metadataImportReport != nil, "HEIC capture report")
    try require(document.doc.metadata(DR_METADATA_EXIF) != nil && document.doc.metadata(DR_METADATA_XMP) != nil, "HEIC capture packets")
    let native = probeDirectory.appendingPathComponent("heic.rz")
    try document.doc.saveNative(to: native)
    let restored = try RasterDocument.open(url: native)
    for kind in [DR_METADATA_EXIF, DR_METADATA_XMP, DR_METADATA_IPTC] {
        try require(restored.metadata(kind) == document.doc.metadata(kind), "native capture packet preservation")
    }
    for (ext, format, iptc) in [("tiff", DR_FORMAT_TIFF, true), ("png", DR_FORMAT_PNG, false), ("jpg", DR_FORMAT_JPEG, true)] {
        let exported = probeDirectory.appendingPathComponent("heic-export.\(ext)")
        let result = try restored.saveImage(nil, to: exported, format: format, jpegQuality: 95,
                                          embedProfile: true, stripMetadata: false)
        try require(result.carries(DR_CARRIES_EXIF) && result.carries(DR_CARRIES_XMP), "HEIC export capture report \(ext)")
        try verifyCapture(exported, dpi: (150, 300), iptc: iptc)
    }
    print("PASS: actual HEIC import, native packet retention and independent TIFF/PNG/JPEG capture reads")
}

func verifyRawCapture() throws {
    let url = probeDirectory.appendingPathComponent("synthetic.dng")
    try require(RawImage.isRawFile(url), "DNG is a RAW input")
    guard RawImage.inspect(url: url) != nil else { throw ProbeFailure(description: "CIRAWFilter rejected synthetic DNG") }
    let previous = RawImportRequest.mode
    RawImportRequest.mode = .headless(RawDevelopSettings())
    defer { RawImportRequest.mode = previous }
    let document = ImageDocument()
    try document.read(from: url, ofType: UTType.rawImage.identifier)
    try require(document.rawDevelop != nil, "DNG must use actual RAW develop branch")
    try require(!document.metadataNotCaptured && document.doc.metadata(DR_METADATA_EXIF) != nil, "RAW capture attached")
    try require(document.doc.metadata(DR_METADATA_XMP) != nil, "RAW XMP attached")
    // A headless RAW import develops 16-bit by default (8C), and a typed
    // document has no byte handle, so export the way ⌘S does for it
    // (`CapturedSave` → `PrecisionExportCapture`).
    try require(document.doc.isHighDepth, "headless RAW develop is 16-bit")
    let out = probeDirectory.appendingPathComponent("raw-export.tiff")
    let report = try PrecisionExportCapture(source: PrecisionExportSource(document), format: .tiff,
                                            bitDepth: 16).write(to: out)
    try require(report.carries(DR_CARRIES_EXIF), "RAW EXIF exported")
    let values = try independentProperties(out)
    let tiff = values[kCGImagePropertyTIFFDictionary as String] as? [String: Any]
    try require(tiff?[kCGImagePropertyTIFFMake as String] as? String == "Rasterize Probe", "RAW camera make")
    try require(tiff?[kCGImagePropertyTIFFArtist as String] as? String == "Phase Three Photographer", "RAW artist")
    try require(document.doc.resolution.0 == 150 && document.doc.resolution.1 == 300, "RAW upright PPI")
    print("PASS: actual synthetic CFA RAW develop -> capture packets -> independently read TIFF")
}

func verifyMetadataLimits() throws {
    var properties = captureProperties()
    var exif = properties[kCGImagePropertyExifDictionary as String] as? [String: Any] ?? [:]
    exif[kCGImagePropertyExifMakerNote as String] = Data([1, 2, 3, 4])
    exif["PhaseThreeUnsupportedField"] = "must be reported"
    properties[kCGImagePropertyExifDictionary as String] = exif
    let capture = ImageIOMetadata.capture(properties: properties, metadata: nil)
    try require(capture.warnings.contains { $0.contains("MakerNote") }, "verified MakerNote omission")
    try require(capture.warnings.contains { $0.contains("PhaseThreeUnsupportedField") }, "ImageIO serialization omission")
    exif[kCGImagePropertyExifUserComment as String] = String(repeating: "x", count: ImageIOMetadata.packetLimit + 1)
    properties[kCGImagePropertyExifDictionary as String] = exif
    let oversized = ImageIOMetadata.capture(properties: properties, metadata: nil)
    try require(oversized.warnings.contains { $0.contains("16 MiB") }, "oversized EXIF fields report")
    let metadata = CGImageMetadataCreateMutable()
    try require(CGImageMetadataRegisterNamespaceForPrefix(metadata,
        "https://example.test/capture/1.0/" as CFString, "probe" as CFString, nil), "XMP fixture namespace")
    try require(CGImageMetadataSetValueWithPath(metadata, nil, "probe:Capture" as CFString,
        String(repeating: "large capture ", count: 6000) as CFString), "large XMP fixture")
    let large = ImageIOMetadata.capture(properties: [:], metadata: metadata)
    try require((large.xmp?.count ?? 0) > 65533, "XMP not forced through a JPEG segment")
    guard let image = RasterImage.from(rgba: [70, 80, 90, 255], width: 1, height: 1),
          let original = RasterDocument.from(image: image) else { throw ProbeFailure(description: "metadata receiver") }
    let captured = large.attaching(to: original).document
    // Since 60c2f17 the JPEG writer splits a packet past one APP1 segment
    // into Adobe ExtendedXMP (core/src/metadata_xmp_split.rs), so every format
    // carries it; the JPEG's bytes are read back to check the report.
    for (ext, format) in [("tiff", DR_FORMAT_TIFF), ("png", DR_FORMAT_PNG), ("jpg", DR_FORMAT_JPEG)] {
        let url = probeDirectory.appendingPathComponent("large-xmp.\(ext)")
        let result = try captured.saveImage(nil, to: url,
            format: format, jpegQuality: 90, embedProfile: true, stripMetadata: false)
        try require(result.carries(DR_CARRIES_XMP), "large XMP carried \(ext)")
        if ext == "jpg" {
            let bytes = try Data(contentsOf: url)
            try require(bytes.range(of: Data("http://ns.adobe.com/xmp/extension/\0".utf8)) != nil,
                "large XMP ExtendedXMP segment in the JPEG")
            try require(bytes.range(of: Data("HasExtendedXMP".utf8)) != nil,
                "large XMP HasExtendedXMP in the JPEG's main packet")
        }
    }
    let stripped = try captured.saveImage(nil, to: probeDirectory.appendingPathComponent("stripped.tiff"),
        format: DR_FORMAT_TIFF, jpegQuality: 90, embedProfile: true, stripMetadata: true)
    try require(!stripped.carries(DR_CARRIES_XMP) && stripped.carries(DR_CARRIES_RESOLUTION), "strip retains PPI")
    print("PASS: MakerNote/oversize reporting, large XMP carried by TIFF, PNG and JPEG ExtendedXMP, and explicit metadata stripping")
}

do {
    _ = NSApplication.shared
    AutomationAlerts.installIfEnabled()
    try FileManager.default.createDirectory(at: probeDirectory, withIntermediateDirectories: true)
    let checks: [(String, () throws -> Void)] = [
        ("TIFF/HEIC", verifyTIFFAndPlatformCapture),
        ("RAW", verifyRawCapture),
        ("metadata limits", verifyMetadataLimits),
        ("PSD imports", verifyPSDImports),
        ("calibrated operations", verifyCalibratedOperations),
        ("refused profiles", verifyRefusedProfiles),
        ("core open profiles", verifyCoreOpenProfiles),
    ]
    let requested = ProcessInfo.processInfo.environment["DR_COLOR_PROBE_CHECKS"]
        .map { Set($0.split(separator: ",").map { $0.trimmingCharacters(in: .whitespaces) }) }
    if let requested {
        try require(!requested.isEmpty && requested.isSubset(of: Set(checks.map { $0.0 })),
                    "Unknown or empty requested probe checks")
    }
    var failures = 0
    for (name, check) in checks where requested == nil || requested?.contains(name) == true {
        do { try check() }
        catch {
            fputs("FAIL [\(name)]: \(error)\n", stderr)
            failures += 1
        }
    }
    exit(failures == 0 ? 0 : 1)
} catch {
    fputs("FAIL: \(error)\n", stderr)
    exit(1)
}
