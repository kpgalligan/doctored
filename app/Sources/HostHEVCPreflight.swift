import AVFoundation

/// One bounded pass over a compressed HEVC track before its frame is
/// decoded, for `HostAVCPreflight`'s reason: the container's `hvcC` alone
/// cannot rule out an in-band change of precision. Every sample must carry
/// the configured `hvcC`; every NAL unit must be single-layer (nuh_layer_id
/// 0: no multiview, alpha or scalable layer) and of a type H.265 Table 7-1
/// assigns (no reserved or unspecified type, which is where a Dolby Vision
/// RPU travels); every in-band SPS must equal a configured one byte for byte.
/// NAL headers only: no slice is parsed. The bounds are the AVC pass's.
enum HostHEVCPreflight {
    static func check(track: AVAssetTrack, asset: AVAsset, configuration data: Data,
                      control: RasterJobControl?) throws {
        let configuration = try HostHEVCPrecision.configuration(data)
        try requireActive(control)
        let reader = try AVAssetReader(asset: asset)
        // nil returns compressed samples; no decoded buffer exists yet.
        let output = AVAssetReaderTrackOutput(track: track, outputSettings: nil)
        output.alwaysCopiesSampleData = false
        guard reader.canAdd(output) else { throw refused("The compressed track cannot be inspected.") }
        reader.add(output)
        guard reader.startReading() else {
            throw reader.error ?? refused("The compressed track could not start.")
        }
        defer { reader.cancelReading() }
        var samples = 0, controls = 0, total = 0, nals = 0, pictures = false
        while true {
            try requireActive(control)
            guard let sample = output.copyNextSampleBuffer() else { break }
            let count = CMSampleBufferGetNumSamples(sample)
            if count == 0 {
                // Attachment-only stream events carry no NAL (HostAVCPreflight).
                guard controls < HostAVCPreflight.maximumControlBuffers,
                      CMSampleBufferIsValid(sample), CMSampleBufferGetDataBuffer(sample) == nil,
                      CMSampleBufferGetImageBuffer(sample) == nil else {
                    throw refused("The HEVC stream has invalid or excessive empty control buffers.")
                }
                controls += 1
                continue
            }
            guard count > 0, count <= HostAVCPreflight.maximumSamples - samples,
                  CMSampleBufferDataIsReady(sample), let buffer = CMSampleBufferGetDataBuffer(sample),
                  let description = CMSampleBufferGetFormatDescription(sample) else {
                throw refused("Compressed samples are invalid or exceed the 4096-sample bound.")
            }
            samples += count
            let fields = CMFormatDescriptionGetExtensions(description) as? [String: Any] ?? [:]
            let atoms = fields["SampleDescriptionExtensionAtoms"] as? [String: Any] ?? [:]
            guard let current = atoms["hvcC"] as? Data, current == data else {
                throw refused("The HEVC sample description changes within the track.")
            }
            let size = CMBlockBufferGetDataLength(buffer)
            guard size > 0, size <= HostAVCPreflight.maximumPacketBytes,
                  size <= HostAVCPreflight.maximumTotalBytes - total else {
                throw refused("Compressed HEVC exceeds the 16 MiB packet or 256 MiB track bound.")
            }
            total += size
            var packet = Data(count: size)
            let status = packet.withUnsafeMutableBytes { bytes in
                guard let destination = bytes.baseAddress else { return kCMBlockBufferBadPointerParameterErr }
                return CMBlockBufferCopyDataBytes(buffer, atOffset: 0, dataLength: size,
                                                  destination: destination)
            }
            guard status == kCMBlockBufferNoErr else { throw refused("A compressed HEVC packet could not be read.") }
            let result = try inspect(packet, configuration, control)
            guard result.count <= 65536 - nals else { throw refused("The HEVC track exceeds its NAL inspection bound.") }
            nals += result.count
            pictures = pictures || result.picture
        }
        try requireActive(control)
        guard reader.status == .completed, samples > 0, pictures else {
            throw reader.error ?? refused("The compressed HEVC track did not complete inspection.")
        }
    }

    private static func inspect(_ packet: Data, _ configuration: HostHEVCPrecision.Configuration,
                                _ control: RasterJobControl?) throws -> (picture: Bool, count: Int) {
        let width = configuration.lengthBytes
        return try packet.withUnsafeBytes { (bytes: UnsafeRawBufferPointer) in
            var cursor = 0, count = 0, picture = false
            while cursor < bytes.count {
                try requireActive(control)
                guard count < 4096, width <= bytes.count - cursor else {
                    throw refused("The HEVC packet has truncated or excessive NAL units.")
                }
                var size = 0
                for _ in 0..<width { size = (size << 8) | Int(bytes[cursor]); cursor += 1 }
                guard size >= 2, size <= bytes.count - cursor else { throw refused("The HEVC NAL length is invalid.") }
                let end = cursor + size
                let first = bytes[cursor], second = bytes[cursor + 1]
                let type = Int(first >> 1) & 0x3f
                let layer = Int(first & 1) << 5 | Int(second >> 3)
                guard first & 0x80 == 0, second & 7 != 0 else { throw refused("The HEVC NAL header is invalid.") }
                guard layer == 0 else {
                    throw refused("This HEVC track carries a second layer (multiview, alpha or scalable), which does not open.")
                }
                switch type {
                case 0...9, 16...21:
                    picture = true
                case 33:
                    let unit = UnsafeRawBufferPointer(rebasing: bytes[cursor..<end])
                    guard configuration.sps.contains(where: { $0.elementsEqual(unit) }) else {
                        throw refused("An in-band HEVC sequence differs from the verified configuration.")
                    }
                case 32, 34...40:
                    break
                default:
                    throw refused("This HEVC track uses reserved or unspecified NAL units (as Dolby Vision does), which do not open.")
                }
                cursor = end
                count += 1
            }
            return (picture, count)
        }
    }

    private static func requireActive(_ control: RasterJobControl?) throws {
        guard control?.isCancelled != true else { throw JobCancelled() }
    }

    private static func refused(_ reason: String) -> DoctoredCoreError {
        HostHEVCPrecision.refused(reason)
    }
}
