import AppKit
import CoreGraphics
import Foundation
import ImageIO
import UniformTypeIdentifiers

// `HostImageEncoder` is the HOST half of a host-encoded export: the core hands the export chain's
// TIFF intermediate over, ImageIO transcodes it to HEIC or AVIF, and the depth and packets reported come
// from an inspection of the BYTES THAT WERE WRITTEN — never from the request — which is why
// `RasterExportResult.adoptingHostBytes` takes them as arguments.

/// The HOST half of a host-encoded export (Phase 11D). The core has no HEIC
/// encoder, so `doc_export` produces the TIFF intermediate the `"tiff"`
/// request would have produced and this transcodes it with ImageIO.
///
/// # What is measured rather than assumed
///
/// The depth and the packets this reports come from an inspection of the
/// BYTES THAT WERE WRITTEN, never from the request or from what the encoder
/// was asked to embed. A HEIC encoder may quietly write 8 bits where 10 were
/// asked for, and it drops IPTC outright; a report built from intent would
/// say otherwise and the file would contradict it. That is the whole reason
/// `RasterExportResult.adoptingHostBytes` takes a depth and a carried mask
/// rather than deriving them.
enum HostImageEncoder {
    /// What the host encoder wrote.
    struct Encoded {
        let bytes: Data
        /// The decoded `bitsPerComponent` of the written file — 8 or 10 for
        /// HEIC, and what the export result reports.
        let bitDepth: Int
        /// The `DR_CARRIES_*` bits an inspection of the written file found.
        let carried: UInt32
    }

    /// Transcodes the export workspace's TIFF intermediate to `format`, one
    /// of the host-encoded formats (`.heic`, `.avif`).
    /// `quality` is the request's `jpeg_quality` (the one lossy-quality dial
    /// the request has); `embedProfile` mirrors the request's own flag, but
    /// the profile itself comes from the intermediate, which already carries
    /// exactly the destination profile the convert stage produced.
    ///
    /// `embedProfile == false` does not silence the writer: ImageIO always
    /// declares the image's colour, and an ICC-less intermediate simply gets
    /// an `nclx` declaration rather than an embedded profile. Which of the
    /// two happened is MEASURED from the written file by `inspect` — the
    /// reported `carried` is never the request's flag repeated back.
    static func encode(fromTIFF data: Data, as format: ExportFormat, quality: Int,
                       embedProfile: Bool) throws -> Encoded {
        let name = format.displayName
        guard format == .heic || format == .avif else {
            throw DoctoredCoreError(message: "\(name) is not encoded by the host.")
        }
        guard let source = CGImageSourceCreateWithData(data as CFData, nil),
              CGImageSourceGetCount(source) > 0,
              let image = CGImageSourceCreateImageAtIndex(source, 0, [
                  kCGImageSourceShouldCache: false,
              ] as CFDictionary)
        else {
            throw DoctoredCoreError(message: "Could not read the export's intermediate image.")
        }
        var properties = (CGImageSourceCopyPropertiesAtIndex(source, 0, nil) as? [CFString: Any]) ?? [:]
        // Orientation is already baked into the pixels by every Doctored
        // path, and the intermediate states 1; carrying anything else here
        // would rotate the delivered picture a second time.
        properties[kCGImagePropertyOrientation] = 1
        properties[kCGImageDestinationLossyCompressionQuality] =
            max(0.0, min(1.0, Double(quality) / 100.0))
        // `embedProfile` deliberately changes nothing here: ImageIO offers no
        // "write this HEIC untagged" control. What it DOES do is follow the
        // intermediate — an ICC-less intermediate is written with an `nclx`
        // colour declaration instead of an embedded profile — and the report
        // below states which of the two the file got, measured from the file.
        _ = embedProfile

        let output = NSMutableData()
        guard let destination = CGImageDestinationCreateWithData(
            output, format.utType.identifier as CFString, 1, nil)
        else {
            throw DoctoredCoreError(message: "This system has no \(name) encoder.")
        }
        if let metadata = CGImageSourceCopyMetadataAtIndex(source, 0, nil) {
            // The metadata object carries the XMP packet; the property
            // dictionary carries EXIF/GPS/TIFF and the DPI. Both are handed
            // over, and what survives is measured below.
            CGImageDestinationAddImageAndMetadata(destination, image, metadata,
                                                  properties as CFDictionary)
        } else {
            CGImageDestinationAddImage(destination, image, properties as CFDictionary)
        }
        guard CGImageDestinationFinalize(destination) else {
            throw DoctoredCoreError(message: "The \(name) encoder could not write the image.")
        }
        let written = output as Data
        guard !written.isEmpty else {
            throw DoctoredCoreError(message: "The \(name) encoder wrote no bytes.")
        }
        let measured = try inspect(written)
        let carried = measured.carried
        // Deliberately NOT a byte comparison against the intermediate's
        // profile: a host encoder may re-serialize the same space (ImageIO
        // writes its own sRGB blob), and reporting "no profile" for a file
        // that is correctly tagged would be the wrong claim. What the file
        // carries is measured above; whether the two blobs are byte-identical
        // is the DRIVE's question, asked of the decoded profiles.
        return Encoded(bytes: written, bitDepth: measured.depth, carried: carried)
    }

    /// `encodeHDR`'s AVIF refusal, also the export sheet's reason for disabling
    /// the HDR row on AVIF (its tooltip and status line).
    static let hdrAVIFUnavailable = "This system has no HDR AVIF encoder (Core Image writes no "
        + "AVIF, and ImageIO's HDR AVIF encode fails); export the 32-bit document as an HDR "
        + "HEIC instead."

    /// A 32-bit float document's HDR HEIC (v1.x), from the core's
    /// pending intermediate. A gain map: the f32 linear TIFF relabelled in its
    /// profile's extended-linearized space with `contentHeadroom`, written by
    /// Core Image (`heifRepresentation` with `.hdrImage` and a COLOUR gain map,
    /// `.hdrGainMapAsRGB`) over an SDR base `CIToneMapHeadroom` makes. Not
    /// ImageIO's `kCGImageDestinationEncodeToISOGainmap`: on macOS 26.5.1 that
    /// encoder writes a monochrome gain map (its pixel-format request is
    /// ignored) and reconstructs at most 0.8 × the declared headroom, so a
    /// colour above or below white came back up to 30 % off and the peak 20 %
    /// dark (measured; Core Image's colour map came back within 0.6 %). PQ:
    /// the 16-bit PQ-code TIFF labelled `itur_2100_PQ` through ImageIO's
    /// `kCGImageDestinationEncodeToISOHDR`. The written file is read back and
    /// must carry an ISO gain map / a BT.2100 PQ space, else this refuses by
    /// name. AVIF refuses by name: Core Image has no AVIF writer and ImageIO's
    /// HDR AVIF encode fails on macOS 26.5.1.
    static func encodeHDR(fromTIFF data: Data, as format: ExportFormat,
                          encoding: ExportSettings.HDREncoding, contentHeadroom: Double,
                          quality: Int) throws -> Encoded {
        let name = format.displayName
        guard format == .heic || format == .avif else {
            throw DoctoredCoreError(message: "\(name) has no HDR encoding; export HEIC.")
        }
        guard let source = CGImageSourceCreateWithData(data as CFData, nil),
              CGImageSourceGetCount(source) > 0,
              let decoded = CGImageSourceCreateImageAtIndex(source, 0, [
                  kCGImageSourceShouldCache: false, kCGImageSourceShouldAllowFloat: true,
              ] as CFDictionary)
        else {
            throw DoctoredCoreError(message: "Could not read the export's intermediate image.")
        }
        guard format == .heic else {
            throw DoctoredCoreError(message: hdrAVIFUnavailable)
        }
        let quality = max(0.0, min(1.0, Double(quality) / 100.0))
        if encoding == .gainMap {
            let written = try gainMapHEIC(decoded, source: source, contentHeadroom: contentHeadroom,
                                          quality: quality)
            return try checkedHDR(written, name: name, encoding: encoding)
        }
        guard let pq = CGColorSpace(name: CGColorSpace.itur_2100_PQ),
              let image = decoded.copy(colorSpace: pq) else {
            throw DoctoredCoreError(message: "The PQ intermediate could not be labelled BT.2100 PQ.")
        }
        var properties: [CFString: Any] = [
            kCGImagePropertyOrientation: 1,
            kCGImageDestinationLossyCompressionQuality: quality,
            kCGImageDestinationEncodeRequest: kCGImageDestinationEncodeToISOHDR,
        ]
        if let copied = CGImageSourceCopyPropertiesAtIndex(source, 0, nil) as? [CFString: Any] {
            for key in [kCGImagePropertyExifDictionary, kCGImagePropertyTIFFDictionary,
                        kCGImagePropertyIPTCDictionary, kCGImagePropertyDPIWidth,
                        kCGImagePropertyDPIHeight] {
                if let value = copied[key] { properties[key] = value }
            }
        }
        let output = NSMutableData()
        guard let destination = CGImageDestinationCreateWithData(
            output, format.utType.identifier as CFString, 1, nil)
        else {
            throw DoctoredCoreError(message: "This system has no \(name) encoder.")
        }
        if let metadata = CGImageSourceCopyMetadataAtIndex(source, 0, nil) {
            CGImageDestinationAddImageAndMetadata(destination, image, metadata,
                                                  properties as CFDictionary)
        } else {
            CGImageDestinationAddImage(destination, image, properties as CFDictionary)
        }
        guard CGImageDestinationFinalize(destination), output.length > 0 else {
            throw DoctoredCoreError(message: "This system's \(name) encoder could not write the HDR "
                + "PQ \(name).")
        }
        return try checkedHDR(output as Data, name: name, encoding: encoding)
    }

    /// The gain-map HEIC through Core Image (`encodeHDR`'s doc): the
    /// intermediate's capture properties ride on the SDR base.
    private static func gainMapHEIC(_ decoded: CGImage, source: CGImageSource,
                                    contentHeadroom: Double, quality: Double) throws -> Data {
        guard let space = decoded.colorSpace,
              let linear = CGColorSpaceCreateExtendedLinearized(space),
              let relabelled = decoded.copy(colorSpace: linear),
              let tagged = CGImageCreateCopyWithContentHeadroom(
                  Float(max(1, contentHeadroom)), relabelled)
        else {
            throw DoctoredCoreError(message: "The document profile has no linear form an HDR "
                + "gain map can be written from.")
        }
        var properties = (CGImageSourceCopyPropertiesAtIndex(source, 0, nil) as? [String: Any]) ?? [:]
        properties[kCGImagePropertyOrientation as String] = 1
        let hdr = CIImage(cgImage: tagged)
        let sdr = hdr.applyingFilter("CIToneMapHeadroom", parameters: [
            "inputSourceHeadroom": Float(max(1, contentHeadroom)), "inputTargetHeadroom": 1.0,
        ]).settingProperties(properties)
        let context = CIContext(options: [.workingColorSpace: linear, .cacheIntermediates: false])
        guard let data = context.heifRepresentation(of: sdr, format: .RGBA8, colorSpace: space, options: [
            .hdrImage: hdr, .hdrGainMapAsRGB: true,
            CIImageRepresentationOption(rawValue: kCGImageDestinationLossyCompressionQuality as String): quality,
        ]), !data.isEmpty else {
            throw DoctoredCoreError(message: "Core Image could not write the HDR gain-map HEIC.")
        }
        return data
    }

    /// The read-back every HDR encode ends with (`encodeHDR`'s doc).
    private static func checkedHDR(_ written: Data, name: String,
                                   encoding: ExportSettings.HDREncoding) throws -> Encoded {
        guard let check = CGImageSourceCreateWithData(written as CFData, nil),
              CGImageSourceGetCount(check) > 0 else {
            throw DoctoredCoreError(message: "Could not read the encoded \(name) back.")
        }
        let index = CGImageSourceGetPrimaryImageIndex(check)
        switch encoding {
        case .gainMap:
            guard CGImageSourceCopyAuxiliaryDataInfoAtIndex(
                    check, index, kCGImageAuxiliaryDataTypeISOGainMap) != nil else {
                throw DoctoredCoreError(message: "The \(name) encoder did not write an ISO gain map.")
            }
        case .pq:
            guard let hdr = CGImageSourceCreateImageAtIndex(check, index, [
                      kCGImageSourceDecodeRequest: kCGImageSourceDecodeToHDR,
                      kCGImageSourceShouldCache: false,
                  ] as CFDictionary),
                  let space = hdr.colorSpace, CGColorSpaceIsPQBased(space),
                  CGColorSpaceUsesITUR_2100TF(space) else {
                throw DoctoredCoreError(message: "The \(name) encoder did not write a BT.2100 PQ image.")
            }
        }
        let measured = try inspect(written)
        return Encoded(bytes: written, bitDepth: measured.depth, carried: measured.carried)
    }

    /// A CMYK document's JPEG (#1): the core's 8-bit CMYK TIFF intermediate
    /// (`dr_precision_doc_save_cmyk`'s `jpeg_intermediate`, its CMYK profile
    /// embedded when asked) transcoded by ImageIO, which keeps the CMYK
    /// colour space and the profile the intermediate carries, so the file is
    /// a CMYK JPEG and never an RGB conversion.
    ///
    /// `keepExif` false (an export whose policy wrote no EXIF packet) leaves
    /// out what ImageIO would write as an Exif APP1 — the intermediate's
    /// `{TIFF}` IFD0, `{Exif}` and `{GPS}` dictionaries, the Orientation, and
    /// the `tiff`, `exif` and `exifEX` tags of the metadata it is handed — and
    /// drops the Exif APP1 ImageIO still synthesizes from the rest
    /// (`withoutExifSegments`), so the JPEG carries none; XMP's other
    /// namespaces, IPTC and the profile follow the intermediate unchanged, and
    /// the intermediate's DPI moves into the JFIF density the dropped segment
    /// held it in place of. `true` (every save) writes what it always wrote.
    ///
    /// `keepXmp` / `keepIptc` false (a policy that dropped the packet) drop
    /// the XMP APP1s and the `Photoshop 3.0` APP13 ImageIO writes anyway: it
    /// restates the intermediate's Exif and IFD0 tags as XMP, and XMP's
    /// `dc:rights` as IPTC, so a dropped packet would otherwise come back.
    static func cmykJPEG(fromTIFF data: Data, quality: Int, keepExif: Bool = true,
                         keepXmp: Bool = true, keepIptc: Bool = true) throws -> Data {
        guard let source = CGImageSourceCreateWithData(data as CFData, nil),
              CGImageSourceGetCount(source) > 0,
              let image = CGImageSourceCreateImageAtIndex(source, 0, [
                  kCGImageSourceShouldCache: false,
              ] as CFDictionary),
              image.colorSpace?.model == .cmyk
        else {
            throw DoctoredCoreError(message: "Could not read the CMYK JPEG's intermediate image.")
        }
        var properties = (CGImageSourceCopyPropertiesAtIndex(source, 0, nil) as? [CFString: Any]) ?? [:]
        if keepExif {
            properties[kCGImagePropertyOrientation] = 1
        } else {
            for key in [kCGImagePropertyTIFFDictionary, kCGImagePropertyExifDictionary,
                        kCGImagePropertyGPSDictionary, kCGImagePropertyOrientation] {
                properties[key] = nil
            }
        }
        properties[kCGImageDestinationLossyCompressionQuality] =
            max(0.0, min(1.0, Double(quality) / 100.0))
        let output = NSMutableData()
        guard let destination = CGImageDestinationCreateWithData(
            output, UTType.jpeg.identifier as CFString, 1, nil)
        else {
            throw DoctoredCoreError(message: "This system has no JPEG encoder.")
        }
        if let metadata = CGImageSourceCopyMetadataAtIndex(source, 0, nil) {
            CGImageDestinationAddImageAndMetadata(destination, image,
                                                  keepExif ? metadata : withoutExifTags(metadata),
                                                  properties as CFDictionary)
        } else {
            CGImageDestinationAddImage(destination, image, properties as CFDictionary)
        }
        guard CGImageDestinationFinalize(destination), output.length > 0 else {
            throw DoctoredCoreError(message: "The JPEG encoder could not write the CMYK image.")
        }
        guard !(keepExif && keepXmp && keepIptc) else { return output as Data }
        let dpi = [kCGImagePropertyDPIWidth, kCGImagePropertyDPIHeight]
            .compactMap { (properties[$0] as? NSNumber)?.doubleValue }
        return withoutSegments(output as Data, exif: !keepExif, xmp: !keepXmp, iptc: !keepIptc,
                               dpi: !keepExif && dpi.count == 2 ? (dpi[0], dpi[1]) : nil)
    }

    /// One JPEG marker segment before the scan: its marker, its whole byte
    /// range and its payload's.
    private struct JPEGSegment {
        let marker: UInt8
        let range: Range<Int>
        let body: Range<Int>

        func opens(with signature: String, in data: Data) -> Bool {
            let bytes = Data(signature.utf8)
            return body.count >= bytes.count
                && data[body.lowerBound..<(body.lowerBound + bytes.count)].elementsEqual(bytes)
        }

        func isExif(in data: Data) -> Bool { marker == 0xE1 && opens(with: "Exif\0\0", in: data) }

        func isXmp(in data: Data) -> Bool {
            marker == 0xE1 && (opens(with: "http://ns.adobe.com/xap/1.0/\0", in: data)
                || opens(with: "http://ns.adobe.com/xmp/extension/\0", in: data))
        }

        /// A `Photoshop 3.0` APP13 holding an IPTC-NAA resource (`8BIM` 0x0404).
        func isIptc(in data: Data) -> Bool {
            marker == 0xED && opens(with: "Photoshop 3.0\0", in: data)
                && data[body].range(of: Data("8BIM".utf8) + Data([0x04, 0x04])) != nil
        }
    }

    /// `data`'s marker segments up to the scan, and where the rest begins;
    /// nil when `data` is not a well-formed JPEG header.
    private static func jpegSegments(_ data: Data) -> (segments: [JPEGSegment], rest: Int)? {
        guard data.count >= 4, data[data.startIndex] == 0xFF, data[data.startIndex + 1] == 0xD8
        else { return nil }
        var segments: [JPEGSegment] = []
        var at = data.startIndex + 2
        while at + 4 <= data.endIndex, data[at] == 0xFF {
            let marker = data[at + 1]
            if marker == 0xDA || marker == 0xD9 { break }
            let end = at + 2 + (Int(data[at + 2]) << 8 | Int(data[at + 3]))
            guard end <= data.endIndex, end >= at + 4 else { return nil }
            segments.append(JPEGSegment(marker: marker, range: at..<end, body: (at + 4)..<end))
            at = end
        }
        return (segments, at)
    }

    /// `jpeg` without the packet segments `cmykJPEG` was told to drop:
    /// `exif`, its `Exif\0\0` APP1s — ImageIO restates the XMP's `dc:rights`
    /// and the DPI as an Exif IFD0 even with every Exif input left out, and
    /// writes the JFIF density unitless beside it (measured, #1), so the
    /// JFIF APP0 then states `dpi` in dots per inch; `xmp`, its XMP APP1s
    /// (the packet and any extension); `iptc`, its `Photoshop 3.0` APP13s,
    /// which ImageIO writes to hold the IPTC record alone. Every other
    /// segment is kept byte for byte.
    private static func withoutSegments(_ jpeg: Data, exif: Bool, xmp: Bool, iptc: Bool,
                                        dpi: (Double, Double)?) -> Data {
        guard let walk = jpegSegments(jpeg) else { return jpeg }
        let dropped = { (segment: JPEGSegment) -> Bool in
            (exif && segment.isExif(in: jpeg)) || (xmp && segment.isXmp(in: jpeg))
                || (iptc && segment.marker == 0xED && segment.opens(with: "Photoshop 3.0\0", in: jpeg))
        }
        var out = Data(jpeg.prefix(2))
        for segment in walk.segments where !dropped(segment) {
            var bytes = Data(jpeg[segment.range])
            // JFIF APP0: "JFIF\0", version (2), units (1), X and Y density (2 each).
            if let dpi, segment.marker == 0xE0, segment.opens(with: "JFIF\0", in: jpeg),
               segment.body.count >= 12 {
                let density = { (v: Double) -> UInt16 in UInt16(min(max(v.rounded(), 1), 65535)) }
                let x = density(dpi.0), y = density(dpi.1)
                bytes.replaceSubrange(11..<16, with: [1, UInt8(x >> 8), UInt8(x & 0xFF),
                                                      UInt8(y >> 8), UInt8(y & 0xFF)])
            }
            out.append(bytes)
        }
        out.append(jpeg[walk.rest...])
        return out
    }

    /// A mutable copy of `metadata` without its top-level `tiff`, `exif` and
    /// `exifEX` tags, which ImageIO would otherwise restate as Exif
    /// (`cmykJPEG`'s `keepExif`).
    private static func withoutExifTags(_ metadata: CGImageMetadata) -> CGImageMetadata {
        guard let copy = CGImageMetadataCreateMutableCopy(metadata) else { return metadata }
        let dropped: Set<String> = [kCGImageMetadataNamespaceTIFF as String,
                                    kCGImageMetadataNamespaceExif as String,
                                    kCGImageMetadataNamespaceExifEX as String]
        var paths: [CFString] = []
        CGImageMetadataEnumerateTagsUsingBlock(metadata, nil, nil) { path, tag in
            if let namespace = CGImageMetadataTagCopyNamespace(tag) as String?,
               dropped.contains(namespace) {
                paths.append(path)
            }
            return true
        }
        for path in paths {
            CGImageMetadataRemoveTagWithPath(copy, nil, path)
        }
        return copy
    }

    /// Reads back what a written file actually holds: the decoded component
    /// depth, the `DR_CARRIES_*` bits, and the embedded profile's bytes.
    static func inspect(_ data: Data) throws -> (depth: Int, carried: UInt32, profile: Data?) {
        guard let source = CGImageSourceCreateWithData(data as CFData, nil),
              CGImageSourceGetCount(source) > 0
        else {
            throw DoctoredCoreError(message: "Could not read the encoded image back.")
        }
        let properties = (CGImageSourceCopyPropertiesAtIndex(source, 0, nil) as? [CFString: Any]) ?? [:]
        guard let image = CGImageSourceCreateImageAtIndex(source, 0, [
            kCGImageSourceShouldCache: false,
        ] as CFDictionary) else {
            throw DoctoredCoreError(message: "Could not decode the encoded image back.")
        }
        // The DECODED depth, not `kCGImagePropertyDepth`: the container's own
        // claim is recorded beside the evidence but never stands in for it
        // (the Phase 8F rule).
        let depth = image.bitsPerComponent
        let profile = image.colorSpace?.copyICCData() as Data?

        var carried: UInt32 = 0
        // NOT `profile != nil`: ImageIO synthesizes a colour space for an
        // `nclx`-declared file too, and hands back a normalized sRGB blob
        // that is nowhere in the bytes. The question "does this file carry a
        // profile" is answered by the file's own `colr` box, which is what
        // `heif-info` and `magick` read.
        if carriesEmbeddedProfile(data) { carried |= DR_CARRIES_PROFILE }
        // A JPEG's packets are its segments (a kept-CMYK export, #1): its
        // `Exif` APP1, its XMP APP1 and its `Photoshop 3.0` APP13's IPTC
        // resource. ImageIO reads XMP's `dc:rights` back as a `{TIFF}`
        // Copyright and an `{IPTC}` notice and the Exif tags back as XMP, so
        // its dictionaries would report packets the file does not hold.
        if let walk = jpegSegments(data) {
            if walk.segments.contains(where: { $0.isExif(in: data) }) { carried |= DR_CARRIES_EXIF }
            if walk.segments.contains(where: { $0.isXmp(in: data) }) { carried |= DR_CARRIES_XMP }
            if walk.segments.contains(where: { $0.isIptc(in: data) }) { carried |= DR_CARRIES_IPTC }
        } else {
            if properties[kCGImagePropertyExifDictionary] != nil
                || properties[kCGImagePropertyTIFFDictionary] != nil {
                carried |= DR_CARRIES_EXIF
            }
            if CGImageSourceCopyMetadataAtIndex(source, 0, nil).flatMap({
                CGImageMetadataCreateXMPData($0, nil)
            }) != nil {
                carried |= DR_CARRIES_XMP
            }
            if properties[kCGImagePropertyIPTCDictionary] != nil {
                carried |= DR_CARRIES_IPTC
            }
        }
        if properties[kCGImagePropertyDPIWidth] != nil
            || (properties[kCGImagePropertyTIFFDictionary] as? [CFString: Any])?[
                kCGImagePropertyTIFFXResolution] != nil {
            carried |= DR_CARRIES_RESOLUTION
        }
        return (depth, carried, profile)
    }

    /// Whether an ISO base-media file embeds an ICC profile: a `colr` box of
    /// type `prof` (or the restricted `rICC`) holds one, while `nclx` only
    /// names primaries and a transfer function. This is a read of the
    /// CONTAINER, so it agrees with `heif-info`'s "color profile" line rather
    /// than with what the decoder was able to synthesize. A JPEG (a kept-CMYK
    /// export, #1) embeds one in its APP2 `ICC_PROFILE` segments.
    private static func carriesEmbeddedProfile(_ data: Data) -> Bool {
        if let walk = jpegSegments(data) {
            return walk.segments.contains { $0.marker == 0xE2 && $0.opens(with: "ICC_PROFILE\0", in: data) }
        }
        let colr = Data("colr".utf8)
        var searchFrom = data.startIndex
        while let found = data.range(of: colr, in: searchFrom..<data.endIndex) {
            let kind = found.upperBound
            if kind + 4 <= data.endIndex {
                let type = data[kind..<(kind + 4)]
                if type == Data("prof".utf8) || type == Data("rICC".utf8) {
                    return true
                }
            }
            searchFrom = found.upperBound
        }
        return false
    }

    /// Decodes an exported file to a straight RGBA8 image, for the export
    /// preview: the core cannot decode HEIC, AVIF or JPEG XL, so the preview
    /// of those three is the host's.
    static func decodeStraightRGBA8(_ data: Data) throws -> RasterImage {
        guard let source = CGImageSourceCreateWithData(data as CFData, nil),
              CGImageSourceGetCount(source) > 0,
              let image = CGImageSourceCreateImageAtIndex(source, 0, [
                  kCGImageSourceShouldCache: false,
              ] as CFDictionary)
        else {
            throw DoctoredCoreError(message: "Could not decode the encoded export.")
        }
        // 1:1, in the file's OWN space: this is a preview of what was
        // written, so nothing may be converted on the way to the screen.
        let space = Bitmap.ingestSpace(of: image).space
        guard let pixels = Bitmap.straightRGBA(
            from: image, fitting: image.width, height: image.height, space: space),
            let preview = RasterImage.from(rgba: pixels, width: image.width, height: image.height)
        else {
            throw DoctoredCoreError(message: "Could not read the encoded export's pixels.")
        }
        return preview
    }
}
