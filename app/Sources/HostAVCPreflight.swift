import AVFoundation

// AVC byte output requires bounded compressed configuration/NAL inspection; attachment-only empty
// stream events are separately bounded.

/// Inspect compressed AVC before requesting a byte-valued decoder output. A
/// container's initial avcC alone cannot rule out an in-band precision change.
enum HostAVCPreflight {
    static let maximumPacketBytes = 16 * 1024 * 1024
    static let maximumTotalBytes = 256 * 1024 * 1024
    static let maximumSamples = 4096
    static let maximumControlBuffers = 64
    private static let maximumNALsPerPacket = 4096
    private static let maximumNALs = 65536
    private static let maximumSEIBytes = 1024 * 1024

    static func check(track: AVAssetTrack, asset: AVAsset, configuration: Data,
                      control: RasterJobControl?) throws {
        try HostAVCPrecision.requireByte420(configuration)
        let lengthBytes = try HostAVCPrecision.nalLengthBytes(configuration)
        let sps = try HostAVCPrecision.configuredSPS(configuration)
        try requireActive(control)
        let reader = try AVAssetReader(asset: asset)
        // nil returns compressed samples; no decoded byte buffer exists yet.
        let output = AVAssetReaderTrackOutput(track: track, outputSettings: nil)
        output.alwaysCopiesSampleData = false
        guard reader.canAdd(output) else { throw refused("The compressed track cannot be inspected.") }
        reader.add(output)
        guard reader.startReading() else { throw reader.error ?? refused("The compressed track could not start.") }
        defer { reader.cancelReading() }
        var sampleCount = 0, controlCount = 0, totalBytes = 0, nalCount = 0
        var hasPicture = false
        while true {
            try requireActive(control)
            guard let sample = output.copyNextSampleBuffer() else { break }
            let count = CMSampleBufferGetNumSamples(sample)
            if count == 0 {
                // AVAssetReader also returns attachment-only stream events
                // (edit boundaries, decoder drains and empty-media markers).
                // They contain no NALs. The decoded reader interprets those
                // events; this pass still inspects every compressed byte.
                // https://developer.apple.com/documentation/coremedia/cmsamplebuffer-api
                guard controlCount < maximumControlBuffers, CMSampleBufferIsValid(sample),
                      CMSampleBufferDataIsReady(sample), CMSampleBufferGetDataBuffer(sample) == nil,
                      CMSampleBufferGetImageBuffer(sample) == nil,
                      CMSampleBufferGetFormatDescription(sample) == nil else {
                    throw refused("The AVC stream has invalid or excessive empty control buffers.")
                }
                controlCount += 1
                continue
            }
            guard count > 0, count <= maximumSamples - sampleCount,
                  CMSampleBufferDataIsReady(sample), let buffer = CMSampleBufferGetDataBuffer(sample),
                  let description = CMSampleBufferGetFormatDescription(sample) else {
                throw refused("Compressed samples are invalid or exceed the 4096-sample bound.")
            }
            sampleCount += count
            let fields = CMFormatDescriptionGetExtensions(description) as? [String: Any] ?? [:]
            let atoms = fields["SampleDescriptionExtensionAtoms"] as? [String: Any] ?? [:]
            guard let current = atoms["avcC"] as? Data, current == configuration else {
                throw refused("The AVC sample description changes within the track.")
            }
            let size = CMBlockBufferGetDataLength(buffer)
            guard size > 0, size <= maximumPacketBytes, size <= maximumTotalBytes - totalBytes else {
                throw refused("Compressed AVC exceeds the 16 MiB packet or 256 MiB track bound.")
            }
            totalBytes += size
            var packet = Data(count: size)
            let status = packet.withUnsafeMutableBytes { bytes in
                guard let destination = bytes.baseAddress else { return kCMBlockBufferBadPointerParameterErr }
                return CMBlockBufferCopyDataBytes(buffer, atOffset: 0, dataLength: size, destination: destination)
            }
            guard status == kCMBlockBufferNoErr else { throw refused("A compressed AVC packet could not be read.") }
            let result = try inspectPacket(packet, nalLengthBytes: lengthBytes, configuredSPS: sps, control: control)
            guard result.nalCount <= maximumNALs - nalCount else { throw refused("The AVC track exceeds its NAL inspection bound.") }
            nalCount += result.nalCount
            hasPicture = hasPicture || result.hasPicture
        }
        try requireActive(control)
        guard reader.status == .completed, sampleCount > 0, hasPicture else {
            throw reader.error ?? refused("The compressed AVC track did not complete inspection.")
        }
    }

    /// This bounded packet seam is also exercised by independent malformed and
    /// mixed-precision fixtures, without invoking a pixel decoder.
    static func checkPacket(_ packet: Data, nalLengthBytes: Int, configuredSPS: [Data],
                            control: RasterJobControl? = nil) throws -> Bool {
        try inspectPacket(packet, nalLengthBytes: nalLengthBytes, configuredSPS: configuredSPS,
                          control: control).hasPicture
    }

    private static func inspectPacket(_ packet: Data, nalLengthBytes: Int, configuredSPS: [Data],
                                      control: RasterJobControl?) throws -> (hasPicture: Bool, nalCount: Int) {
        guard !packet.isEmpty, packet.count <= maximumPacketBytes,
              [1, 2, 4].contains(nalLengthBytes), !configuredSPS.isEmpty,
              configuredSPS.count <= 31, configuredSPS.allSatisfy({ (5...4096).contains($0.count) }) else {
            throw refused("The AVC packet or sequence configuration is invalid.")
        }
        try requireActive(control)
        return try packet.withUnsafeBytes { (bytes: UnsafeRawBufferPointer) in
            var cursor = 0, count = 0
            var hasPicture = false
            while cursor < bytes.count {
                try requireActive(control)
                guard count < maximumNALsPerPacket, nalLengthBytes <= bytes.count - cursor else {
                    throw refused("The AVC packet has truncated or excessive NAL units.")
                }
                var size = 0
                for _ in 0..<nalLengthBytes { size = (size << 8) | Int(bytes[cursor]); cursor += 1 }
                guard size >= 2, size <= bytes.count - cursor else { throw refused("The AVC NAL length is invalid.") }
                let end = cursor + size
                let header = bytes[cursor]
                guard header & 0x80 == 0 else { throw refused("The AVC NAL header is invalid.") }
                // H.264 Table 7-1: ordinary nonpartitioned AVC only. Extension,
                // auxiliary, scalable and multiview NALs need another policy.
                switch header & 31 {
                case 1, 5:
                    if header & 31 == 5, header & 0x60 == 0 { throw refused("The AVC IDR header is invalid.") }
                    hasPicture = true
                case 7:
                    guard size <= 4096,
                          configuredSPS.contains(where: { $0.withUnsafeBytes { configured in
                              configured.elementsEqual(UnsafeRawBufferPointer(rebasing: bytes[cursor..<end]))
                          } }) else {
                        throw refused("An in-band AVC sequence differs from the verified byte-depth configuration.")
                    }
                case 6:
                    guard header & 0x60 == 0 else { throw refused("The AVC SEI header is invalid.") }
                    try checkSEI(UnsafeRawBufferPointer(rebasing: bytes[(cursor + 1)..<end]))
                case 8, 9, 10, 11, 12:
                    break
                default:
                    throw refused("This AVC track uses unsupported extension or partitioned NAL units.")
                }
                cursor = end
                count += 1
            }
            return (hasPicture, count)
        }
    }

    private static func checkSEI(_ encoded: UnsafeRawBufferPointer) throws {
        guard encoded.count <= maximumSEIBytes else { throw refused("The AVC SEI exceeds its 1 MiB bound.") }
        var bytes: [UInt8] = []
        bytes.reserveCapacity(encoded.count)
        var cursor = 0, zeros = 0
        while cursor < encoded.count {
            let byte = encoded[cursor]
            if zeros == 2 {
                if byte == 3 {
                    guard cursor + 1 < encoded.count, encoded[cursor + 1] <= 3 else {
                        throw refused("The AVC SEI emulation prevention is malformed.")
                    }
                    zeros = 0; cursor += 1; continue
                }
                guard byte > 2 else { throw refused("The AVC SEI contains an unescaped start code.") }
            }
            bytes.append(byte)
            zeros = byte == 0 ? zeros + 1 : 0
            cursor += 1
        }
        cursor = 0
        var messages = 0
        func field() throws -> Int {
            var value = 0
            while cursor < bytes.count {
                let part = Int(bytes[cursor]); cursor += 1
                value += part
                guard value <= maximumSEIBytes else { throw refused("The AVC SEI field exceeds its bound.") }
                if part != 255 { return value }
            }
            throw refused("The AVC SEI field is truncated.")
        }
        while cursor < bytes.count {
            if cursor == bytes.count - 1, bytes[cursor] == 0x80 { return }
            guard messages < 256 else { throw refused("The AVC NAL contains too many SEI messages.") }
            let type = try field(), size = try field()
            // H.264 Annex D: timing, unregistered encoder identification and
            // recovery only. All registered T.35, HDR, colour remapping and
            // unknown instructions are refused instead of silently discarded.
            guard [0, 1, 5, 6].contains(type), size <= bytes.count - cursor,
                  type != 5 || size >= 16 else {
                throw refused("The AVC track contains unsupported HDR, colour or other SEI instructions.")
            }
            cursor += size; messages += 1
        }
        throw refused("The AVC SEI lacks its RBSP trailing bits.")
    }

    private static func requireActive(_ control: RasterJobControl?) throws {
        guard control?.isCancelled != true else { throw JobCancelled() }
    }

    private static func refused(_ reason: String) -> DoctoredCoreError {
        HostSourceError.failure(reason + " No byte decode was used to replace an unverified source.")
    }
}
