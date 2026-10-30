import AVFoundation
import CoreVideo
import ImageIO

// Native v25 retains channel display options/provenance around compatible earlier bodies.

/// The platform produces oriented scalar buffers. Integer coverage is copied
/// exactly (byte coverage promotes by 257); float coverage rounds once to u16.
/// Scene depth is explicitly normalized, never described as a precision gain.
enum NativeAuxiliaryMattes {
    struct Plane {
        let name: String
        let width: Int
        let height: Int
        let samples: [UInt16]
        let provenance: [String: Any]
        let retainedPayload: PrecisionEditPayload
    }

    /// `index` is the image whose mattes are read (a host still's primary).
    static func planes(in url: URL, index: Int = 0) throws -> [Plane] {
        guard let source = CGImageSourceCreateWithURL(url as CFURL, nil), CGImageSourceGetCount(source) > index else { return [] }
        let properties = CGImageSourceCopyPropertiesAtIndex(source, index, nil) as? [CFString: Any]
        let raw = (properties?[kCGImagePropertyOrientation] as? NSNumber)?.uint32Value ?? 1
        guard let orientation = CGImagePropertyOrientation(rawValue: raw) else {
            throw DoctoredCoreError(message: "Auxiliary matte orientation is invalid.")
        }
        var found: [Plane] = []
        for entry in AuxiliaryMattes.sources where !found.contains(where: { $0.name == entry.name }) {
            guard let data = CGImageSourceCopyAuxiliaryDataInfoAtIndex(source, index, entry.type) else { continue }
            guard let info = data as? [AnyHashable: Any] else {
                throw DoctoredCoreError(message: "The \(entry.name) auxiliary image could not be decoded or oriented.")
            }
            let decodeLease = try PrecisionEditPayload.admitted(bytes: validatedInfo(info))
            defer { withExtendedLifetime(decodeLease) {} }
            let buffer = try mattingImage(entry.type, info, orientation)
            found.append(try plane(from: buffer, name: entry.name, orientation: raw))
        }
        return found
    }

    static func mattingImage(_ type: CFString, _ info: [AnyHashable: Any],
                              _ orientation: CGImagePropertyOrientation) throws -> CVPixelBuffer {
        _ = try validatedInfo(info)
        if type == kCGImageAuxiliaryDataTypeDepth || type == kCGImageAuxiliaryDataTypeDisparity {
            // Preserve the decoder's original float16/32 representation. A
            // conversion to float32 disparity would hide the input precision.
            return try AVDepthData(fromDictionaryRepresentation: info)
                .applyingExifOrientation(orientation).depthDataMap
        }
        if type == kCGImageAuxiliaryDataTypePortraitEffectsMatte {
            return try AVPortraitEffectsMatte(fromDictionaryRepresentation: info)
                .applyingExifOrientation(orientation).mattingImage
        }
        return try AVSemanticSegmentationMatte(fromImageSourceAuxiliaryDataType: type,
            dictionaryRepresentation: info).applyingExifOrientation(orientation).mattingImage
    }

    /// ImageIO's initial auxiliary extraction and the SDK decode/orientation
    /// calls are noncancellable platform allocations. Once their descriptor is
    /// available, refuse invalid extents before constructing an AV object and
    /// account the retained dictionary plus simultaneous oriented buffers.
    static func validatedInfo(_ info: [AnyHashable: Any]) throws -> UInt64 {
        guard let description = info[kCGImageAuxiliaryDataInfoDataDescription] as? [String: Any],
              let data = info[kCGImageAuxiliaryDataInfoData] as? Data,
              let width = description["Width"] as? Int, let height = description["Height"] as? Int,
              let stride = description["BytesPerRow"] as? Int,
              let format = description["PixelFormat"] as? UInt32,
              width > 0, height > 0, width <= RasterImage.maxResizePixels / height else {
            throw DoctoredCoreError(message: "Auxiliary data has an invalid scalar descriptor.")
        }
        let bytes: Int
        switch format {
        case kCVPixelFormatType_OneComponent8: bytes = 1
        case kCVPixelFormatType_OneComponent16, kCVPixelFormatType_OneComponent16Half,
             kCVPixelFormatType_DepthFloat16, kCVPixelFormatType_DisparityFloat16: bytes = 2
        case kCVPixelFormatType_OneComponent32Float, kCVPixelFormatType_DepthFloat32,
             kCVPixelFormatType_DisparityFloat32: bytes = 4
        default: throw DoctoredCoreError(message: "Unsupported auxiliary scalar pixel format \(format).")
        }
        guard stride >= width * bytes, stride <= Int.max / height,
              data.count == stride * height else {
            throw DoctoredCoreError(message: "Auxiliary data length or stride differs from its descriptor.")
        }
        return UInt64(data.count) * 3 + UInt64(width * height) * 8 + 65_536
    }

    static func plane(from buffer: CVPixelBuffer, name: String, orientation: UInt32 = 1) throws -> Plane {
        let width = CVPixelBufferGetWidth(buffer), height = CVPixelBufferGetHeight(buffer)
        guard width > 0, height > 0, width <= RasterImage.maxResizePixels / height else {
            throw DoctoredCoreError(message: "Auxiliary matte exceeds the supported image extent.")
        }
        let retained = try PrecisionEditPayload.admitted(bytes: UInt64(width * height) * 2 + 4096)
        let rowBytes = CVPixelBufferGetBytesPerRow(buffer)
        guard rowBytes >= 0, rowBytes <= Int.max / height else { throw DoctoredCoreError(message: "Invalid auxiliary buffer stride.") }
        let platform = try PrecisionEditPayload.admitted(bytes: UInt64(rowBytes * height) + 4096)
        defer { withExtendedLifetime(platform) {} }
        guard CVPixelBufferLockBaseAddress(buffer, .readOnly) == kCVReturnSuccess else {
            throw DoctoredCoreError(message: "Could not read the auxiliary pixel buffer.")
        }
        defer { CVPixelBufferUnlockBaseAddress(buffer, .readOnly) }
        guard let base = CVPixelBufferGetBaseAddress(buffer) else { throw DoctoredCoreError(message: "Auxiliary pixel buffer is empty.") }
        let stride = CVPixelBufferGetBytesPerRow(buffer), format = CVPixelBufferGetPixelFormatType(buffer)
        let bytes: Int, bits: Int, sourceFormat: String, normalized: Bool, inverse: Bool
        switch format {
        case kCVPixelFormatType_OneComponent8:
            (bytes, bits, sourceFormat, normalized, inverse) = (1, 8, "one_component8", false, false)
        case kCVPixelFormatType_OneComponent16:
            (bytes, bits, sourceFormat, normalized, inverse) = (2, 16, "one_component16", false, false)
        case kCVPixelFormatType_OneComponent16Half:
            (bytes, bits, sourceFormat, normalized, inverse) = (2, 16, "float16", false, false)
        case kCVPixelFormatType_OneComponent32Float:
            (bytes, bits, sourceFormat, normalized, inverse) = (4, 32, "float32", false, false)
        case kCVPixelFormatType_DisparityFloat16:
            (bytes, bits, sourceFormat, normalized, inverse) = (2, 16, "disparity_float16", true, false)
        case kCVPixelFormatType_DepthFloat16:
            (bytes, bits, sourceFormat, normalized, inverse) = (2, 16, "depth_float16", true, true)
        case kCVPixelFormatType_DisparityFloat32:
            (bytes, bits, sourceFormat, normalized, inverse) = (4, 32, "disparity_float32", true, false)
        case kCVPixelFormatType_DepthFloat32:
            (bytes, bits, sourceFormat, normalized, inverse) = (4, 32, "depth_float32", true, true)
        default: throw DoctoredCoreError(message: "Unsupported auxiliary scalar pixel format \(format).")
        }
        guard stride >= width * bytes else { throw DoctoredCoreError(message: "Auxiliary row stride is shorter than its declared extent.") }
        let read: (Int, Int) -> Double = { x, y in
            let offset = y * stride + x * bytes
            switch format {
            case kCVPixelFormatType_OneComponent8: return Double(base.loadUnaligned(fromByteOffset: offset, as: UInt8.self)) / 255
            case kCVPixelFormatType_OneComponent16: return Double(base.loadUnaligned(fromByteOffset: offset, as: UInt16.self)) / 65535
            default:
                return bytes == 2 ? Double(base.loadUnaligned(fromByteOffset: offset, as: Float16.self))
                    : Double(base.loadUnaligned(fromByteOffset: offset, as: Float32.self))
            }
        }
        var low = Double.infinity, high = -Double.infinity
        if normalized {
            for y in 0..<height { for x in 0..<width {
                let value = read(x, y)
                if value.isFinite { low = min(low, value); high = max(high, value) }
            } }
        }
        var samples = [UInt16](repeating: 0, count: width * height)
        for y in 0..<height { for x in 0..<width {
            var value = read(x, y)
            guard value.isFinite else { continue }
            if normalized {
                guard high > low else { continue }
                value = (value - low) / (high - low)
                if inverse { value = 1 - value }
            }
            samples[y * width + x] = UInt16((min(max(value, 0), 1) * 65535).rounded())
        } }
        var provenance: [String: Any] = ["version": 1, "kind": "camera_auxiliary", "name": name,
            "source_format": sourceFormat, "source_bits": bits, "source_width": width, "source_height": height,
            "precision_basis": "decoded_auxiliary_buffer", "encoded_precision": "not_inspected",
            "exif_orientation": orientation, "orientation_applied": true,
            "coverage_mapping": normalized ? "finite_scene_range_near_white" : "identity_unorm",
            "alignment": "oriented_full_image_bilinear"]
        if normalized, low.isFinite, high.isFinite { provenance["finite_range"] = [low, high] }
        return Plane(name: name, width: width, height: height, samples: samples, provenance: provenance, retainedPayload: retained)
    }

    static func attaching(to document: RasterDocument, from url: URL, index: Int = 0) throws -> RasterDocument {
        var result = document
        for plane in try planes(in: url, index: index) {
            guard let source = result.precision else { throw DoctoredCoreError(message: "Typed matte attachment requires a native document.") }
            let provenance = try SelectedAdjustment.json(plane.provenance)
            let command = try PrecisionPixelCommand(["op": "import_channel", "name": plane.name,
                "width": plane.width, "height": plane.height, "provenance": provenance])
            guard let updated = try PrecisionScalarWork(source: source, command: command, samples: plane.samples).compute() else {
                throw DoctoredCoreError(message: "The \(plane.name) auxiliary matte could not be attached.")
            }
            result = try RasterDocument(precision: updated)
        }
        return result
    }
}
