import AVFoundation
import CoreVideo
import CryptoKit

// Video selection uses floor presentation time inside the track interval; requested/resolved
// rational values and the source of its coverage duration are persisted. User decimal seconds
// explicitly quantize to microseconds. Poll cancellation between samples/stages; individual
// platform codec calls are noncancellable.

enum HostVideoDecoder {
    static let maximumDuration = 120.0
    static let maximumSamples = 4096
    static let scratchPerPixel: UInt64 = 64
    struct Probe: Equatable {
        let codec: String
        let width: Int
        let height: Int
        let orientation: Int
        let trackID: Int32
        let start: HostMediaTime
        let duration: HostMediaTime
        let formatHash: String
        var highDepth: Bool { codec == "ap4h" || codec == "ap4x" }
    }

    static func inspect(_ url: URL) throws -> Probe {
        let asset = AVURLAsset(url: url, options: [AVURLAssetPreferPreciseDurationAndTimingKey: true])
        let tracks = asset.tracks(withMediaType: .video)
        guard tracks.count == 1, let track = tracks.first,
              let formats = track.formatDescriptions as? [CMFormatDescription], formats.count == 1,
              let format = formats.first else { throw HostSourceError.failure("Choose a movie with one supported video track and one stable codec format.") }
        let subtype = CMFormatDescriptionGetMediaSubType(format)
        let codec = String(bytes: [UInt8((subtype >> 24) & 255), UInt8((subtype >> 16) & 255),
            UInt8((subtype >> 8) & 255), UInt8(subtype & 255)], encoding: .ascii) ?? ""
        guard ["avc1", "ap4h", "ap4x"].contains(codec) else { throw HostSourceError.failure(HostSourceError.unsupported) }
        let dimensions = CMVideoFormatDescriptionGetDimensions(format)
        let width = Int(dimensions.width), height = Int(dimensions.height)
        let start = try HostMediaTime(track.timeRange.start)
        let duration = try HostMediaTime(track.timeRange.duration)
        guard width > 0, height > 0, width <= RasterImage.maxResizePixels / height,
              start.valid, duration.valid, start.seconds >= 0, duration.seconds > 0,
              duration.seconds <= maximumDuration else { throw HostSourceError.failure("The movie exceeds the 120-second or supported pixel bounds.") }
        let extensions = CMFormatDescriptionGetExtensions(format) as? [String: Any] ?? [:]
        try requireSDR(extensions)
        guard (extensions["CVFieldCount"] as? NSNumber)?.intValue ?? 1 == 1 else {
            throw HostSourceError.failure("Interlaced video requires a separate field interpretation policy.")
        }
        if let ratio = extensions["CVPixelAspectRatio"] as? [String: Any] {
            guard (ratio["HorizontalSpacing"] as? NSNumber)?.doubleValue == (ratio["VerticalSpacing"] as? NSNumber)?.doubleValue else {
                throw HostSourceError.failure("Non-square movie pixels are not supported by this preserving frame path.")
            }
        }
        if let aperture = extensions["CVImageBufferCleanAperture"] as? [String: Any] {
            guard (aperture["Width"] as? NSNumber)?.doubleValue == Double(width),
                  (aperture["Height"] as? NSNumber)?.doubleValue == Double(height),
                  (aperture["HorizontalOffset"] as? NSNumber)?.doubleValue ?? 0 == 0,
                  (aperture["VerticalOffset"] as? NSNumber)?.doubleValue ?? 0 == 0 else {
                throw HostSourceError.failure("A cropped movie clean aperture needs a separate preserving geometry policy.")
            }
        }
        let atoms = extensions["SampleDescriptionExtensionAtoms"] as? [String: Any] ?? [:]
        if codec == "avc1" {
            guard let configuration = atoms["avcC"] as? Data else { throw HostSourceError.failure("The H.264 source lacks a verifiable AVC sample description.") }
            try HostAVCPrecision.requireByte420(configuration)
        }
        let orientation = try orientation(track.preferredTransform, width: width, height: height)
        let fingerprint: Data
        if let bytes = extensions["VerbatimSampleDescription"] as? Data, !bytes.isEmpty, bytes.count <= 64 * 1024 {
            fingerprint = bytes
        } else {
            throw HostSourceError.failure("The movie codec description is unavailable or exceeds its bounded size.")
        }
        return Probe(codec: codec, width: width, height: height, orientation: orientation,
            trackID: track.trackID, start: start, duration: duration,
            formatHash: HostSourceError.sha256(fingerprint))
    }

    static func decode(_ url: URL, filename: String, requested: HostMediaTime?, maximumScratch: UInt64,
                       control: RasterJobControl? = nil)
        throws -> (RasterDocument, HostMediaContract, HostFrameSelection) {
        let probe = try inspect(url)
        guard UInt64(probe.width * probe.height) * scratchPerPixel + 32 * 1024 * 1024 <= maximumScratch else {
            throw HostSourceError.failure("The movie dimensions changed beyond the captured decode allowance.")
        }
        let time = requested ?? probe.start
        let end = try HostMediaTime(CMTimeAdd(probe.start.cmTime, probe.duration.cmTime)).cmTime
        guard time.valid, CMTimeCompare(time.cmTime, probe.start.cmTime) >= 0,
              CMTimeCompare(time.cmTime, end) < 0 else {
            throw HostSourceError.failure("Frame time must be inside the captured video track, excluding its end time.")
        }
        let asset = AVURLAsset(url: url, options: [AVURLAssetPreferPreciseDurationAndTimingKey: true])
        guard let track = asset.tracks(withMediaType: .video).first, track.trackID == probe.trackID else {
            throw HostSourceError.failure("The selected movie track is unavailable.")
        }
        if probe.codec == "avc1" {
            guard let formats = track.formatDescriptions as? [CMFormatDescription], let format = formats.first,
                  let extensions = CMFormatDescriptionGetExtensions(format) as? [String: Any],
                  let atoms = extensions["SampleDescriptionExtensionAtoms"] as? [String: Any],
                  let configuration = atoms["avcC"] as? Data else { throw HostSourceError.failure("AVC configuration disappeared during capture.") }
            try HostAVCPreflight.check(track: track, asset: asset, configuration: configuration, control: control)
        }
        let reader = try AVAssetReader(asset: asset)
        let format = probe.highDepth ? kCVPixelFormatType_64ARGB : kCVPixelFormatType_32BGRA
        let output = AVAssetReaderTrackOutput(track: track,
            outputSettings: [kCVPixelBufferPixelFormatTypeKey as String: format])
        output.alwaysCopiesSampleData = false
        guard reader.canAdd(output) else { throw HostSourceError.failure("The captured movie codec cannot supply the required integer samples.") }
        reader.add(output)
        guard reader.startReading() else { throw reader.error ?? HostSourceError.failure("The captured movie decoder could not start.") }
        defer { reader.cancelReading() }
        var selected: CMSampleBuffer?
        var previous: CMTime?
        var next: CMTime?
        var count = 0
        while let sample = output.copyNextSampleBuffer() {
            guard control?.isCancelled != true else { throw JobCancelled() }
            count += 1
            guard count <= maximumSamples else { throw HostSourceError.failure("Frame selection exceeds the 4096-sample decode bound.") }
            let pts = CMSampleBufferGetPresentationTimeStamp(sample)
            guard CMSampleBufferGetNumSamples(sample) == 1,
                  pts.isNumeric, !pts.flags.contains(.hasBeenRounded), pts.epoch == 0,
                  CMTimeCompare(pts, probe.start.cmTime) >= 0, CMTimeCompare(pts, end) < 0,
                  previous.map({ CMTimeCompare(pts, $0) > 0 }) ?? true else {
                throw HostSourceError.failure("The movie decoder returned duplicate or unordered presentation times.")
            }
            previous = pts
            if CMTimeCompare(pts, time.cmTime) > 0 { next = pts; break }
            selected = sample
        }
        guard control?.isCancelled != true,
              (next == nil ? reader.status == .completed : [.reading, .completed].contains(reader.status)),
              let sample = selected,
              let buffer = CMSampleBufferGetImageBuffer(sample),
              CVPixelBufferGetPixelFormatType(buffer) == format, CVPixelBufferGetPlaneCount(buffer) == 0,
              CVPixelBufferGetWidth(buffer) == probe.width, CVPixelBufferGetHeight(buffer) == probe.height,
              let attachments = CVBufferCopyAttachments(buffer, .shouldPropagate),
              let space = CVImageBufferCreateColorSpaceFromAttachments(attachments)?.takeRetainedValue(),
              space.model == .rgb, !space.isHDR(), !CGColorSpaceUsesExtendedRange(space),
              let profile = space.copyICCData() as Data?, !profile.isEmpty else {
            throw reader.error ?? HostSourceError.failure("The selected frame has no supported full integer buffer and SDR ICC profile.")
        }
        let fields = attachments as? [String: Any] ?? [:]
        try requireSDR(fields)
        let alpha = fields["AlphaChannelMode"] as? String
        if probe.highDepth {
            guard alpha == "StraightAlpha" else {
                throw HostSourceError.failure("The ProRes decoder did not declare straight alpha.")
            }
        } else {
            guard (fields["AlphaChannelIsOpaque"] as? NSNumber)?.boolValue == true else {
                throw HostSourceError.failure("The H.264 decoder did not declare opaque byte samples.")
            }
        }
        let rowBytes = CVPixelBufferGetBytesPerRow(buffer)
        let (byteCount, overflow) = rowBytes.multipliedReportingOverflow(by: probe.height)
        guard !overflow, byteCount > 0, UInt64(byteCount) <= maximumScratch / 2,
              rowBytes >= probe.width * (probe.highDepth ? 8 : 4), byteCount <= CVPixelBufferGetDataSize(buffer),
              UInt64(byteCount) * 2 + UInt64(probe.width * probe.height) * 16 + 32 * 1024 * 1024 <= maximumScratch,
              CVPixelBufferLockBaseAddress(buffer, .readOnly) == kCVReturnSuccess else {
            throw HostSourceError.failure("The selected frame buffer exceeds its admitted layout.")
        }
        defer { CVPixelBufferUnlockBaseAddress(buffer, .readOnly) }
        guard let pointer = CVPixelBufferGetBaseAddress(buffer) else { throw HostSourceError.failure("The selected frame has no pixel storage.") }
        let lease = try PrecisionEditPayload.admitted(bytes: UInt64(byteCount) * 2 + UInt64(profile.count) + 65_536)
        defer { withExtendedLifetime(lease) {} }
        let bytes = Data(bytes: pointer, count: byteCount)
        let layout = probe.highDepth ? "argb16_be" : "bgra8_opaque"
        let document = try RasterDocument.fromHostPixels(bytes: bytes, width: probe.width, height: probe.height,
            rowBytes: rowBytes, layout: layout, profile: profile, orientation: probe.orientation, control: control)
        let pts = CMSampleBufferGetPresentationTimeStamp(sample)
        var duration = CMSampleBufferGetDuration(sample)
        var durationOrigin = "decoder_sample"
        if !duration.isNumeric || CMTimeCompare(duration, .zero) <= 0 {
            duration = CMTimeSubtract(next ?? end, pts)
            durationOrigin = next == nil ? "track_end" : "next_pts"
        }
        guard duration.isNumeric, CMTimeCompare(duration, .zero) > 0 else { throw HostSourceError.failure("The selected frame duration is invalid.") }
        let selection = HostFrameSelection(kind: "video", requested: time,
            resolved: try HostMediaTime(pts), sample_duration: try HostMediaTime(duration), duration_origin: durationOrigin)
        let decoded = HostDecodedContract(width: document.width, height: document.height,
            storage_depth: probe.highDepth ? 16 : 8, layout: layout,
            alpha: probe.highDepth ? "straight" : "opaque",
            profile_sha256: HostSourceError.sha256(profile), profile_name: space.name as String? ?? "Decoded ICC")
        let media = HostMediaContract(resource_id: "video", filename: filename, format: probe.codec,
            decoder: .current("apple-avfoundation"), encoded_depth: probe.highDepth ? nil : 8,
            encoded_precision_note: probe.highDepth
                ? "ProRes encoded color and alpha precision are codec-specific; the retained master uses actual full integer16 decoder samples."
                : "AVC sequence headers establish 8-bit 4:2:0 source samples; the decoded RGB master is byte-valued.",
            width: probe.width, height: probe.height, orientation: probe.orientation, track_id: probe.trackID,
            start: probe.start, duration: probe.duration, format_sha256: probe.formatHash, decoded: decoded)
        return (document, media, selection)
    }

    private static func requireSDR(_ fields: [String: Any]) throws {
        if let transfer = fields["CVImageBufferTransferFunction"] as? String,
           !["ITU_R_709_2", "sRGB", "UseGamma"].contains(transfer) {
            throw HostSourceError.failure("HDR or unsupported movie transfer functions are not implicitly converted to SDR.")
        }
        for name in fields.keys where name.localizedCaseInsensitiveContains("Dolby")
            || name.localizedCaseInsensitiveContains("MasteringDisplay")
            || name.localizedCaseInsensitiveContains("ContentLight") {
            throw HostSourceError.failure("This movie carries HDR metadata and requires a later HDR import policy.")
        }
    }

    private static func orientation(_ transform: CGAffineTransform, width: Int, height: Int) throws -> Int {
        let matrices: [[CGFloat]] = [[1,0,0,1],[-1,0,0,1],[-1,0,0,-1],[1,0,0,-1],
            [0,1,1,0],[0,1,-1,0],[0,-1,-1,0],[0,-1,1,0]]
        let values = [transform.a, transform.b, transform.c, transform.d]
        guard let index = matrices.firstIndex(where: { zip($0, values).allSatisfy { abs($0 - $1) < 1e-9 } }) else {
            throw HostSourceError.failure("The movie track transform is not an exact supported orientation.")
        }
        let bounds = CGRect(x: 0, y: 0, width: width, height: height).applying(transform)
        // A preferred rotation often has a negative origin. A source master
        // uses the complete oriented image's canonical top-left bounding box;
        // document placement remains a separate retained affine transform.
        guard [bounds.minX, bounds.minY, bounds.maxX, bounds.maxY].allSatisfy({ $0.isFinite && abs($0) < 10_000_000 }) else {
            throw HostSourceError.failure("The movie track carries an unsupported translated extent.")
        }
        return index + 1
    }
}
