import Foundation
import ImageIO
import UniformTypeIdentifiers

extension Bitmap {
    /// ImageIO can develop a CFA DNG while omitting its TIFF density tags
    /// from the property dictionary. Ask the shared core scanner for those
    /// declared values; its report already applies the orientation axis swap.
    static func declaredCaptureDPI(_ url: URL) -> (Double, Double)? {
        guard RasterDocument.metadataWalked(at: url),
              let report = RasterDocument.metadataImportReport(at: url),
              let resolution = report["resolution"] as? [String: Any],
              let x = resolution["x"] as? Double, let y = resolution["y"] as? Double,
              x.isFinite, y.isFinite, x > 0, y > 0 else { return nil }
        return (x, y)
    }
}

/// Capture information exposed by ImageIO for HEIC/HEIF and camera RAW.
/// ImageIO reconstructs standard fields; it does not return the original EXIF
/// byte stream. XMP is serialized separately so it does not inherit JPEG's
/// single-segment size limit. The tiny TIFF is only a metadata carrier: its
/// pixels and profile never become part of the photograph.
struct ImageIOMetadata {
    let carrier: Data?
    let xmp: Data?
    let inspected: Bool
    let warnings: [String]

    // Matches the native packet cap. The core enforces it again on attachment.
    static let packetLimit = 16 * 1024 * 1024

    /// `index` is the image read: a multi-image container's primary image
    /// when a host still opened it (`HostStillDecoder.Probe.index`).
    static func read(_ url: URL, index: Int = 0) -> ImageIOMetadata {
        guard let source = CGImageSourceCreateWithURL(url as CFURL, nil),
              CGImageSourceGetCount(source) > index,
              let properties = CGImageSourceCopyPropertiesAtIndex(source, index, nil)
                as? [String: Any]
        else {
            return ImageIOMetadata(carrier: nil, xmp: nil, inspected: false,
                warnings: ["ImageIO could not read this source's capture metadata."])
        }
        return capture(properties: properties,
            metadata: CGImageSourceCopyMetadataAtIndex(source, index, nil))
    }

    /// Also used by the source probe: the dictionary/packet serialization seam
    /// is identical for a camera decoder and an ImageIO still decoder.
    static func capture(properties: [String: Any], metadata: CGImageMetadata?) -> ImageIOMetadata {
        var warnings: [String] = []
        var output: [String: Any] = [:]
        let dictionaryKeys = [kCGImagePropertyExifDictionary, kCGImagePropertyGPSDictionary,
                              kCGImagePropertyIPTCDictionary]
        for key in dictionaryKeys {
            guard var fields = properties[key as String] as? [String: Any], !fields.isEmpty else {
                continue
            }
            if key == kCGImagePropertyExifDictionary,
               fields.removeValue(forKey: kCGImagePropertyExifMakerNote as String) != nil {
                warnings.append("The EXIF MakerNote was omitted; its private offsets cannot be relocated safely.")
            }
            if bounded(fields) { output[key as String] = fields }
            else { warnings.append("The \(key) fields exceed the 16 MiB packet limit or could not be serialized.") }
        }
        if let tiff = properties[kCGImagePropertyTIFFDictionary as String] as? [String: Any] {
            // Layout/encoding tags belong to the new image, not its capture
            // history. Keep the standard descriptive fields only.
            let keys = [kCGImagePropertyTIFFDocumentName, kCGImagePropertyTIFFImageDescription,
                        kCGImagePropertyTIFFMake, kCGImagePropertyTIFFModel,
                        kCGImagePropertyTIFFSoftware, kCGImagePropertyTIFFDateTime,
                        kCGImagePropertyTIFFArtist, kCGImagePropertyTIFFCopyright]
            var kept: [String: Any] = [:]
            for key in keys { kept[key as String] = tiff[key as String] }
            if !kept.isEmpty {
                if bounded(kept) { output[kCGImagePropertyTIFFDictionary as String] = kept }
                else { warnings.append("The TIFF description fields exceed the metadata packet limit.") }
            }
        }
        let privateKeys = properties.keys.filter {
            $0.hasPrefix("{Maker") || $0 == (kCGImagePropertyExifAuxDictionary as String)
        }
        if !privateKeys.isEmpty {
            warnings.append("Camera-specific metadata dictionaries are outside the supported capture fields: "
                + privateKeys.sorted().joined(separator: ", ") + ".")
        }
        // The photograph's own upright PPI is already set by Bitmap.imageDPI.
        // Its exporter normalizes this packet to that PPI and the real size.
        output[kCGImagePropertyOrientation as String] = 1
        var carrier: Data?
        if output.count > 1 {
            carrier = serialize(output)
            if carrier == nil { warnings.append("ImageIO could not serialize the standard capture fields.") }
            else if let carrier = carrier {
                warnings.append(contentsOf: serializationOmissions(requested: output, carrier: carrier))
            }
        }
        var xmp: Data?
        if let metadata = metadata {
            if let bytes = CGImageMetadataCreateXMPData(metadata, nil) as Data? {
                if bytes.count <= packetLimit { xmp = bytes }
                else { warnings.append("The XMP packet exceeds the 16 MiB limit and was omitted.") }
            } else if let tags = CGImageMetadataCopyTags(metadata), CFArrayGetCount(tags) > 0 {
                warnings.append("ImageIO could not serialize this source's XMP metadata.")
            }
        }
        return ImageIOMetadata(carrier: carrier, xmp: xmp, inspected: true, warnings: warnings)
    }

    func attaching(to original: RasterDocument) -> (document: RasterDocument, report: [String: Any]) {
        var doc = original
        var issues = warnings
        if let carrier = carrier {
            do {
                let imported = try doc.importingMetadata(from: carrier)
                doc = imported.document ?? doc
                issues.append(contentsOf: imported.warnings)
            }
            catch { issues.append("The standard capture fields could not be attached: \(error.localizedDescription)") }
        }
        if let xmp = xmp {
            do { doc = try doc.settingMetadata(DR_METADATA_XMP, xmp) ?? doc }
            catch { issues.append("The XMP packet could not be attached: \(error.localizedDescription)") }
        }
        var captured: [String] = []
        for (name, kind) in [("exif", DR_METADATA_EXIF), ("xmp", DR_METADATA_XMP), ("iptc", DR_METADATA_IPTC)] {
            if doc.metadata(kind) != nil { captured.append(name) }
        }
        let report: [String: Any] = [
            "container": "imageio", "metadata_walked": inspected,
            "representation": "reconstructed_standard_fields",
            "captured_packets": captured,
            "supported_packets": ["exif", "xmp", "iptc"],
            "limitations": [
                "ImageIO reconstructs standard EXIF/GPS, TIFF description and IPTC fields and serializes its XMP view; original packet bytes are not retained.",
                "MakerNotes, private camera dictionaries, thumbnails and decoder-specific fields are outside this guarantee.",
            ],
            "warnings": issues,
        ]
        return (doc, report)
    }

    private static func bounded(_ value: [String: Any]) -> Bool {
        // Bound before asking the encoder to build offsets. Binary plist is
        // used only to measure property storage, never as an interchange packet.
        guard let data = try? PropertyListSerialization.data(fromPropertyList: value,
            format: .binary, options: 0) else { return false }
        return data.count <= packetLimit
    }

    /// ImageIO can silently ignore an unsupported dictionary field before the
    /// core sees its TIFF carrier. Compare only presence of bounded leaf keys:
    /// dates/numbers/arrays may change representation, and geometry is replaced
    /// deliberately. This neither decodes nor compares the carrier's pixels.
    private static func serializationOmissions(requested: [String: Any], carrier: Data) -> [String] {
        guard let source = CGImageSourceCreateWithData(carrier as CFData, nil),
              let serialized = CGImageSourceCopyPropertiesAtIndex(source, 0, nil) as? [String: Any]
        else { return ["ImageIO's serialized capture fields could not be verified."] }
        let ignored: Set<String> = ["PixelWidth", "PixelHeight", "PixelXDimension", "PixelYDimension",
                                    "Orientation", "XResolution", "YResolution", "ResolutionUnit"]
        var remaining = 4096
        var incomplete = false
        var missing: [String] = []
        func compare(_ wanted: [String: Any], _ actual: [String: Any], path: String, depth: Int) {
            guard depth <= 8 else { incomplete = true; return }
            if wanted.count > remaining { incomplete = true }
            for key in wanted.keys.prefix(remaining).sorted() where !ignored.contains(key) {
                guard remaining > 0 else { incomplete = true; return }
                remaining -= 1
                let label = String(key.prefix(96))
                let name = String((path.isEmpty ? label : path + "." + label).prefix(256))
                if let children = wanted[key] as? [String: Any] {
                    compare(children, actual[key] as? [String: Any] ?? [:], path: name, depth: depth + 1)
                } else if actual[key] == nil {
                    missing.append(name)
                }
            }
        }
        for key in [kCGImagePropertyExifDictionary, kCGImagePropertyGPSDictionary,
                    kCGImagePropertyIPTCDictionary, kCGImagePropertyTIFFDictionary] {
            guard let wanted = requested[key as String] as? [String: Any] else { continue }
            compare(wanted, serialized[key as String] as? [String: Any] ?? [:],
                    path: key as String, depth: 0)
        }
        var warnings: [String] = []
        if !missing.isEmpty {
            let suffix = missing.count > 12 ? " (and \(missing.count - 12) more)" : ""
            warnings.append("ImageIO omitted capture fields while serializing: "
                + missing.prefix(12).joined(separator: ", ") + suffix + ".")
        }
        if incomplete { warnings.append("Some capture fields exceed the bounded serialization inspection limits.") }
        return warnings
    }

    private static func serialize(_ properties: [String: Any]) -> Data? {
        let data = NSMutableData()
        guard let provider = CGDataProvider(data: Data([0, 0, 0, 255]) as CFData),
              let pixel = CGImage(width: 1, height: 1, bitsPerComponent: 8, bitsPerPixel: 32,
                bytesPerRow: 4, space: ColorProfile.sRGB,
                bitmapInfo: CGBitmapInfo(rawValue: CGImageAlphaInfo.last.rawValue),
                provider: provider, decode: nil, shouldInterpolate: false, intent: .defaultIntent),
              let destination = CGImageDestinationCreateWithData(data, UTType.tiff.identifier as CFString, 1, nil)
        else { return nil }
        CGImageDestinationAddImage(destination, pixel, properties as CFDictionary)
        guard CGImageDestinationFinalize(destination), data.length <= packetLimit * 3 + 1024 * 1024 else {
            return nil
        }
        return data as Data
    }
}
