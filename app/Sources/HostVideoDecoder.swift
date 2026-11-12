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
    /// What an HDR movie's format description states: its BT.2100 transfer,
    /// the primaries the core's intake is told, and whether static mastering
    /// or content-light metadata came with it (kept as a note, never applied).
    struct HDRSignal: Equatable {
        let kind: HDRStillKind
        let primaries: String
        let staticMetadata: Bool
    }
    struct Probe: Equatable {
        let codec: String
        let width: Int
        let height: Int
        let orientation: Int
        let trackID: Int32
        let start: HostMediaTime
        let duration: HostMediaTime
        let formatHash: String
        /// The coded sample depth: 8 for H.264, 8 or 10 for HEVC, nil for
        /// ProRes, whose precision is codec-specific.
        let encodedDepth: Int?
        /// nil for SDR; an HDR track opens only through `decodeHDR`.
        let hdr: HDRSignal?
        var prores: Bool { codec == "ap4h" || codec == "ap4x" }
        /// The frame decodes to integer 16-bit samples: ProRes 4444 and 10-bit HEVC.
        var highDepth: Bool { prores || encodedDepth == 10 }
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
        if ["dvh1", "dvhe", "dva1", "dvav"].contains(codec) { throw HostSourceError.failure(dolbyVision) }
        if codec == "hev1" {
            throw HostSourceError.failure("This HEVC movie keeps its parameter sets in the stream (hev1), "
                + "which macOS's movie decoder does not read. Rewrap it as hvc1 to open a frame.")
        }
        guard ["avc1", "hvc1", "ap4h", "ap4x"].contains(codec) else {
            throw HostSourceError.failure("Movie frames open from H.264, HEVC (hvc1) and ProRes 4444 "
                + "video; this movie's video is \(codec.isEmpty ? "another codec" : codec).")
        }
        let dimensions = CMVideoFormatDescriptionGetDimensions(format)
        let width = Int(dimensions.width), height = Int(dimensions.height)
        let start = try HostMediaTime(track.timeRange.start)
        let duration = try HostMediaTime(track.timeRange.duration)
        guard width > 0, height > 0, width <= RasterImage.maxResizePixels / height,
              start.valid, duration.valid, start.seconds >= 0, duration.seconds > 0,
              duration.seconds <= maximumDuration else { throw HostSourceError.failure("The movie exceeds the 120-second or supported pixel bounds.") }
        let extensions = CMFormatDescriptionGetExtensions(format) as? [String: Any] ?? [:]
        let atoms = extensions["SampleDescriptionExtensionAtoms"] as? [String: Any] ?? [:]
        if ["dvcC", "dvvC", "dvwC"].contains(where: { atoms[$0] != nil })
            || extensions.keys.contains(where: { $0.localizedCaseInsensitiveContains("Dolby") }) {
            throw HostSourceError.failure(dolbyVision)
        }
        let encodedDepth: Int?
        switch codec {
        case "avc1":
            guard let configuration = atoms["avcC"] as? Data else { throw HostSourceError.failure("The H.264 source lacks a verifiable AVC sample description.") }
            try HostAVCPrecision.requireByte420(configuration)
            encodedDepth = 8
        case "hvc1":
            guard let configuration = atoms["hvcC"] as? Data else { throw HostSourceError.failure("The HEVC source lacks a verifiable HEVC sample description.") }
            encodedDepth = try HostHEVCPrecision.configuration(configuration).depth
        default:
            encodedDepth = nil
        }
        let hdr = try classify(extensions, codec: codec, encodedDepth: encodedDepth)
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
        let orientation = try orientation(track.preferredTransform, width: width, height: height)
        let fingerprint: Data
        if let bytes = extensions["VerbatimSampleDescription"] as? Data, !bytes.isEmpty, bytes.count <= 64 * 1024 {
            fingerprint = bytes
        } else {
            throw HostSourceError.failure("The movie codec description is unavailable or exceeds its bounded size.")
        }
        return Probe(codec: codec, width: width, height: height, orientation: orientation,
            trackID: track.trackID, start: start, duration: duration,
            formatHash: HostSourceError.sha256(fingerprint), encodedDepth: encodedDepth, hdr: hdr)
    }

    /// The SDR frame as one raster at its decoded depth: the open's,
    /// place's, a Live Photo pair's and the stack's decode. An HDR track
    /// refuses here (`hdrPlaceRefusal`); only File ▸ Open reads it, through
    /// `decodeHDR`.
    static func decode(_ url: URL, filename: String, requested: HostMediaTime?, maximumScratch: UInt64,
                       control: RasterJobControl? = nil)
        throws -> (RasterDocument, HostMediaContract, HostFrameSelection) {
        let probe = try inspect(url)
        if let hdr = probe.hdr { throw HostSourceError.failure(hdrPlaceRefusal(hdr)) }
        return try withFrame(url, probe: probe, requested: requested, maximumScratch: maximumScratch,
                             control: control) { frame in
            guard let space = CVImageBufferCreateColorSpaceFromAttachments(frame.attachments)?.takeRetainedValue(),
                  space.model == .rgb, !space.isHDR(), !CGColorSpaceUsesExtendedRange(space),
                  let profile = space.copyICCData() as Data?, !profile.isEmpty else {
                throw HostSourceError.failure("The selected frame has no supported full integer buffer and SDR ICC profile.")
            }
            try requireSDR(frame.fields)
            if probe.prores {
                guard frame.fields["AlphaChannelMode"] as? String == "StraightAlpha" else {
                    throw HostSourceError.failure("The ProRes decoder did not declare straight alpha.")
                }
            } else {
                guard (frame.fields["AlphaChannelIsOpaque"] as? NSNumber)?.boolValue == true else {
                    throw HostSourceError.failure("The \(codecName(probe.codec)) decoder did not declare opaque samples.")
                }
            }
            let copied = try copySamples(frame.buffer, probe: probe, maximumScratch: maximumScratch,
                                         extra: profile.count)
            defer { withExtendedLifetime(copied.lease) {} }
            let layout = probe.prores ? "argb16_be" : probe.highDepth ? "argb16_be_opaque" : "bgra8_opaque"
            let document = try RasterDocument.fromHostPixels(bytes: copied.bytes, width: probe.width,
                height: probe.height, rowBytes: copied.rowBytes, layout: layout, profile: profile,
                orientation: probe.orientation, control: control)
            let decoded = HostDecodedContract(width: document.width, height: document.height,
                storage_depth: probe.highDepth ? 16 : 8, layout: layout,
                alpha: probe.prores ? "straight" : "opaque",
                profile_sha256: HostSourceError.sha256(profile), profile_name: space.name as String? ?? "Decoded ICC")
            let media = HostMediaContract(resource_id: "video", filename: filename, format: probe.codec,
                decoder: .current("apple-avfoundation"), encoded_depth: probe.encodedDepth,
                encoded_precision_note: precisionNote(probe),
                width: probe.width, height: probe.height, orientation: probe.orientation, track_id: probe.trackID,
                start: probe.start, duration: probe.duration, format_sha256: probe.formatHash, decoded: decoded)
            return (document, media, frame.selection)
        }
    }

    /// The decoded frame `decode` and `decodeHDR` read, and the time it resolved to.
    struct Frame {
        let buffer: CVPixelBuffer
        let attachments: CFDictionary
        let fields: [String: Any]
        let selection: HostFrameSelection
    }

    /// Verifies the compressed track, decodes up to the frame at or before
    /// `requested` (the track's start when nil) and hands it to `body` while
    /// the reader still owns its buffer.
    static func withFrame<R>(_ url: URL, probe: Probe, requested: HostMediaTime?, maximumScratch: UInt64,
                             control: RasterJobControl?, _ body: (Frame) throws -> R) throws -> R {
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
        if probe.codec == "avc1" || probe.codec == "hvc1" {
            let key = probe.codec == "avc1" ? "avcC" : "hvcC"
            guard let formats = track.formatDescriptions as? [CMFormatDescription], let format = formats.first,
                  let extensions = CMFormatDescriptionGetExtensions(format) as? [String: Any],
                  let atoms = extensions["SampleDescriptionExtensionAtoms"] as? [String: Any],
                  let configuration = atoms[key] as? Data else { throw HostSourceError.failure("The movie's codec configuration disappeared during capture.") }
            if probe.codec == "avc1" {
                try HostAVCPreflight.check(track: track, asset: asset, configuration: configuration, control: control)
            } else {
                try HostHEVCPreflight.check(track: track, asset: asset, configuration: configuration, control: control)
            }
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
              let attachments = CVBufferCopyAttachments(buffer, .shouldPropagate) else {
            throw reader.error ?? HostSourceError.failure("The selected frame has no supported full integer buffer.")
        }
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
        return try body(Frame(buffer: buffer, attachments: attachments,
                              fields: attachments as? [String: Any] ?? [:], selection: selection))
    }

    /// The frame's samples copied out under the admitted layout bounds, and
    /// the lease that charges the copy; `extra` is the profile's bytes.
    static func copySamples(_ buffer: CVPixelBuffer, probe: Probe, maximumScratch: UInt64, extra: Int)
        throws -> (bytes: Data, rowBytes: Int, lease: PrecisionEditPayload) {
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
        let lease = try PrecisionEditPayload.admitted(bytes: UInt64(byteCount) * 2 + UInt64(extra) + 65_536)
        return (Data(bytes: pointer, count: byteCount), rowBytes, lease)
    }

    /// SDR (nil) or the HDR signal the format description states; refuses
    /// by name what opens neither way.
    private static func classify(_ fields: [String: Any], codec: String, encodedDepth: Int?) throws -> HDRSignal? {
        let staticMetadata = fields.keys.contains {
            $0.localizedCaseInsensitiveContains("MasteringDisplay") || $0.localizedCaseInsensitiveContains("ContentLight")
        }
        let kind: HDRStillKind
        switch fields["CVImageBufferTransferFunction"] as? String {
        case nil, "ITU_R_709_2"?, "sRGB"?, "UseGamma"?:
            guard !staticMetadata else {
                throw HostSourceError.failure("This movie carries HDR mastering metadata on an SDR transfer function, which does not open.")
            }
            return nil
        case "ITU_R_2100_HLG"?: kind = .hlg
        case "SMPTE_ST_2084_PQ"?: kind = .pq
        case let other?:
            throw HostSourceError.failure("This movie's transfer function (\(other)) is neither SDR nor BT.2100 HLG or PQ, and is not converted.")
        }
        let name = kind == .pq ? "PQ" : "HLG"
        guard codec == "hvc1", encodedDepth == 10 else {
            throw HostSourceError.failure("This \(codecName(codec)) movie is coded in \(name); an HDR movie frame opens from 10-bit HEVC only.")
        }
        let primaries: String
        switch fields["CVImageBufferColorPrimaries"] as? String {
        case "ITU_R_2020"?: primaries = "bt2020"
        case "P3_D65"?: primaries = "display_p3"
        case "ITU_R_709_2"?: primaries = "bt709"
        case let other:
            throw HostSourceError.failure("This \(name) movie's primaries (\(other ?? "unstated")) are not BT.2020, Display P3 or BT.709.")
        }
        return HDRSignal(kind: kind, primaries: primaries, staticMetadata: staticMetadata)
    }

    /// A decoded SDR buffer must still say SDR.
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

    /// Placing, pairing or stacking an HDR movie's frame refuses: only an open reads one.
    static func hdrPlaceRefusal(_ signal: HDRSignal) -> String {
        "This movie is HDR (\(signal.kind == .pq ? "PQ" : "HLG")). Open it with File ▸ Open to get its "
            + "frame as a 32-bit float document; placing an HDR movie frame into a document is not supported."
    }

    /// A Live Photo pair whose movie is HDR, opened or placed as a pair.
    static func hdrPairRefusal(_ signal: HDRSignal) -> String {
        "This Live Photo's movie is HDR (\(signal.kind == .pq ? "PQ" : "HLG")): a Live Photo holding an "
            + "HDR movie does not open or place yet, and opening either file opens the pair."
    }

    static let dolbyVision = "This movie is Dolby Vision, which does not open yet: its dynamic metadata "
        + "has no import policy. Export it as HLG or PQ HEVC to open a frame."

    static func codecName(_ codec: String) -> String {
        switch codec {
        case "avc1": return "H.264"
        case "hvc1": return "HEVC"
        case "ap4h", "ap4x": return "ProRes 4444"
        default: return codec
        }
    }

    private static func precisionNote(_ probe: Probe) -> String {
        if probe.prores {
            return "ProRes encoded color and alpha precision are codec-specific; the retained master uses actual full integer16 decoder samples."
        }
        let name = probe.codec == "avc1" ? "AVC" : "HEVC"
        return probe.highDepth
            ? "\(name) sequence headers establish 10-bit 4:2:0 source samples; the retained master uses actual full integer16 decoder samples."
            : "\(name) sequence headers establish 8-bit 4:2:0 source samples; the decoded RGB master is byte-valued."
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
