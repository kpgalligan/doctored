import AppKit

// An HLG or PQ movie opened (File ▸ Open, MCP open_document): its first frame as one raster layer
// of a 32-bit float document in linear light, 203 cd/m² at 1.0 — the rule a PQ/HLG photo opens
// under (`HDRStillOpen.swift`), and its preference and refusal too. `openHostSource` routes here
// when its one entry is a movie whose probe is HDR; place, a Live Photo pair and the stack refuse
// an HDR movie instead (`HostVideoDecoder.hdrPlaceRefusal`).

extension ImageDocument {
    static func openHDRMovieFrame(_ url: URL, signal: HostVideoDecoder.HDRSignal,
                                  encoded: UInt64) throws -> Opened {
        guard HDRPhotoRequest.effective == .hdr, PrecisionImportRequest.mode != .rgba8 else {
            throw HostSourceError.failure(HDRPhotoRequest.noSDRMovieRendition(signal.kind))
        }
        let probe = try HostVideoDecoder.inspect(url)
        // A float document keeps 16 bytes a pixel: the one-image cap at 32
        // bits refuses before any admission or decode.
        try HostImageCapacity.require(width: probe.width, height: probe.height, depth: 32)
        // The PQ still's measured open plan (`HDRStillDecoder.openScratch`):
        // after the decode the pipeline is that still's own — a 16-bit
        // integer buffer of the same 8 B/px into the same intake — and its
        // 3·encoded term covers the movie capture's own (`HostSourceInput`).
        let scratch = try HDRStillDecoder.openScratch(width: probe.width, height: probe.height,
                                                      encoded: encoded)
        _ = try RasterResources.shared.admit(scratch: scratch)
        let control = try RasterJobControl()
        return try BackgroundWork.wait(try WorkerTask(scratchEstimate: scratch) {
            let (document, decodeRecord) = try HostVideoDecoder.decodeHDR(url, requested: nil,
                maximumScratch: scratch, control: control)
            var hdr = HDRPhotoRequest.openRecord(signal.kind, rendition: .hdr)
            hdr.merge(decodeRecord) { current, _ in current }
            var warnings = ["The movie frame's decoded samples are kept as linear light in a 32-bit "
                + "float document; the original movie is not. 1.0 is 203 cd/m² of the "
                + "\(signal.kind == .pq ? "PQ" : "HLG") signal. Export capture packets and the "
                + "original PPI are not reconstructed for a movie frame."]
            if signal.staticMetadata {
                warnings.append("The movie's static HDR metadata (mastering display, content light "
                    + "level) describes how it was graded; it was not applied to the frame.")
            }
            let core = document.precisionInfo?["import_warnings"] as? [String] ?? []
            warnings.append(contentsOf: core)
            let report: [String: Any] = [
                "container": "host_media", "metadata_walked": false,
                "representation": "decoded_hdr_pixels", "captured_packets": [String](),
                "hdr_open": hdr, "warnings": warnings, "import_warnings": core,
            ]
            return Opened(doc: document, metadataNotCaptured: true, rawDevelop: nil,
                          metadataReport: report, hdrRendition: .hdr)
        }, title: "Reading HDR Movie Frame", control: control)
    }
}
