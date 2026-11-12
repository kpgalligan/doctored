import AVFoundation
import CoreVideo

// An HLG or PQ movie's frame as a 32-bit float document (`ImageDocument.openHDRMovieFrame`): the
// same frame selection and 64ARGB decode as an SDR 10-bit frame (`HostVideoDecoder.withFrame`),
// whose buffer carries the BT.2100 signal untouched (the decoder converts YCbCr to RGB and nothing
// else; measured on HLG and PQ HEVC Main10), handed to the core's PQ/HLG intake as an HDR still's
// integer decode is (`float_host_intake`, 203 cd/m² at 1.0).

extension HostVideoDecoder {
    static func decodeHDR(_ url: URL, requested: HostMediaTime?, maximumScratch: UInt64,
                          control: RasterJobControl?) throws -> (RasterDocument, [String: Any]) {
        let probe = try inspect(url)
        guard let signal = probe.hdr else {
            throw HostSourceError.failure("This movie is not HDR; it opens as an ordinary movie frame.")
        }
        return try withFrame(url, probe: probe, requested: requested, maximumScratch: maximumScratch,
                             control: control) { frame in
            let expected = signal.kind == .pq ? "SMPTE_ST_2084_PQ" : "ITU_R_2100_HLG"
            guard frame.fields["CVImageBufferTransferFunction"] as? String == expected,
                  let space = CVImageBufferCreateColorSpaceFromAttachments(frame.attachments)?.takeRetainedValue(),
                  space.model == .rgb, space.isHDR() else {
                throw HostSourceError.failure("The decoder did not return the movie's HDR signal.")
            }
            guard (frame.fields["AlphaChannelIsOpaque"] as? NSNumber)?.boolValue == true else {
                throw HostSourceError.failure("The HEVC decoder did not declare opaque samples.")
            }
            let copied = try copySamples(frame.buffer, probe: probe, maximumScratch: maximumScratch, extra: 0)
            defer { withExtendedLifetime(copied.lease) {} }
            let document = try RasterDocument.fromHostHDRPixels(bytes: copied.bytes, width: probe.width,
                height: probe.height, rowBytes: copied.rowBytes, layout: "argb16_be_opaque",
                orientation: probe.orientation, transfer: signal.kind.rawValue,
                primaries: signal.primaries, profile: nil, control: control)
            var record: [String: Any] = [
                "source": "movie_frame", "decoder": "apple-avfoundation", "format": probe.codec,
                "layout": "argb16_be_opaque", "color_space": space.name as String? ?? "Decoded HDR",
                "primaries": signal.primaries,
                "frame_time": ["value": frame.selection.resolved?.value ?? 0,
                               "timescale": frame.selection.resolved?.timescale ?? 1],
            ]
            if signal.staticMetadata { record["static_metadata"] = "present, not applied" }
            return (document, record)
        }
    }
}
