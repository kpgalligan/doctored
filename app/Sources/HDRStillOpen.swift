import AppKit
import ImageIO

// An HDR photo opened as HDR (v1.x): one raster layer of a 32-bit float document, its
// metadata and PPI attached as the typed host still branch does (`HostSourceImport.swift`).
// `ImageDocument.openDocument(at:)` routes here when `HDRStillDecoder.classify` names the file
// and `HDRPhotoRequest.effective` is `.hdr`.

extension ImageDocument {
    static func openHDRStill(_ url: URL, kind: HDRStillKind) throws -> Opened {
        guard let source = CGImageSourceCreateWithURL(url as CFURL,
                  [kCGImageSourceShouldCache: false] as CFDictionary),
              CGImageSourceGetCount(source) > 0 else {
            throw HostSourceError.failure("macOS's image decoder cannot read this file as an image.")
        }
        let primary = CGImageSourceGetPrimaryImageIndex(source)
        guard let info = CGImageSourceCopyPropertiesAtIndex(source, primary, nil) as? [CFString: Any],
              let width = (info[kCGImagePropertyPixelWidth] as? NSNumber)?.intValue,
              let height = (info[kCGImagePropertyPixelHeight] as? NSNumber)?.intValue,
              width > 0, height > 0, width <= RasterImage.maxResizePixels / height else {
            throw HostSourceError.failure("This HDR photo's dimensions are outside the document size limit.")
        }
        // A float document keeps 16 bytes a pixel: the one-image cap at 32
        // bits refuses before any admission or decode.
        try HostImageCapacity.require(width: width, height: height, depth: 32)
        let encoded = (try? url.resourceValues(forKeys: [.fileSizeKey]).fileSize).flatMap { $0 }
            .map { UInt64($0) } ?? 0
        let scratch = try HDRStillDecoder.openScratch(width: width, height: height, encoded: encoded)
        _ = try RasterResources.shared.admit(scratch: scratch)
        let control = try RasterJobControl()
        return try BackgroundWork.wait(try WorkerTask(scratchEstimate: scratch) {
            let (decoded, decodeReport) = try HDRStillDecoder.decode(url, kind: kind,
                maximumScratch: scratch, control: control)
            let metadata = ImageIOMetadata.read(url, index: primary)
            var captured = metadata.attaching(to: decoded)
            if let dpi = Bitmap.imageDPI(url, index: primary),
               captured.document.resolution.x != Double(Float(dpi.0))
                || captured.document.resolution.y != Double(Float(dpi.1)) {
                guard let sized = try captured.document.settingResolution(x: dpi.0, y: dpi.1) else {
                    throw HostSourceError.failure("The captured print resolution could not be preserved.")
                }
                captured.document = sized
            }
            var report = captured.report
            report["container"] = "host_media"
            report["representation"] = "decoded_hdr_pixels_and_reconstructed_fields"
            var hdr = HDRPhotoRequest.openRecord(kind, rendition: .hdr)
            hdr.merge(decodeReport) { current, _ in current }
            report["hdr_open"] = hdr
            var warnings = (report["warnings"] as? [String]) ?? []
            warnings.append("The HDR photo's decoded samples are kept as linear light in a 32-bit "
                + "float document; the original file is not. 1.0 is "
                + (kind == .gainMap ? "the SDR base's white." : "203 cd/m² of the PQ/HLG signal.")
                + " Capture packets and the original PPI are reconstructed from the platform "
                + "reader's view; auxiliary mattes are not read for an HDR photo.")
            if let core = captured.document.precisionInfo?["import_warnings"] as? [String] {
                warnings.append(contentsOf: core)
            }
            report["warnings"] = warnings
            report["import_warnings"] = captured.document.precisionInfo?["import_warnings"] ?? []
            return Opened(doc: captured.document, metadataNotCaptured: !metadata.inspected,
                          rawDevelop: nil, metadataReport: report, hdrRendition: .hdr)
        }, title: "Reading HDR Photo", control: control)
    }
}
