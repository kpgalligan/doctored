import AppKit
import ImageIO

// A host image or movie opens as ONE ordinary raster layer of its decoded samples (2026-10-10: no
// original file or decoded master is kept in the document). Stills keep the metadata/PPI/auxiliary
// import pass at both depths; a movie frame discloses that it has none.

extension ImageDocument {
    static func openHostSource(_ url: URL) throws -> Opened {
        let input = try HostSourceInput(url: url)
        // An HLG or PQ movie opens as a 32-bit float document (`HDRMovieFrameOpen.swift`).
        if input.entries.count == 1, let movie = input.entries.first, movie.role == "video",
           let signal = try HostVideoDecoder.inspect(movie.url).hdr {
            return try openHDRMovieFrame(movie.url, signal: signal, encoded: movie.bytes)
        }
        // A single 8-bit still is charged its whole open up front
        // (`HostStillDecoder.openScratch`'s record): the first projection
        // peaks above the capture figure `input.scratch` charges.
        if input.entries.count == 1, let still = input.entries.first, still.role == "still",
           let depth = still.depth, depth <= 8 {
            _ = try RasterResources.shared.admit(scratch: HostStillDecoder.openScratch(
                width: still.width, height: still.height, encoded: still.bytes))
        }
        let prepared = try HostSourceWork.capture(input)
        let highDepth: Bool
        switch PrecisionImportRequest.mode {
        case .rgba8: highDepth = false
        case .preserve: highDepth = prepared.asset.isHighDepth
        case .ask:
            if prepared.asset.isHighDepth {
                let alert = NSAlert()
                alert.messageText = "Open “\(url.lastPathComponent)” in 16-bit mode?"
                alert.informativeText = "The full decoder produced integer 16-bit samples. An 8-bit document converts them to 8 bits."
                alert.addButton(withTitle: "Preserve 16-bit")
                alert.addButton(withTitle: "8-bit")
                alert.addButton(withTitle: "Cancel")
                switch alert.runModal() {
                case .alertFirstButtonReturn: highDepth = true
                case .alertSecondButtonReturn: highDepth = false
                default: throw CocoaError(.userCancelled)
                }
            } else { highDepth = false }
        }
        _ = try RasterResources.shared.admit(scratch: input.scratch)
        return try BackgroundWork.wait(try WorkerTask(scratchEstimate: input.scratch) {
            let source = try HostSourceWork.initialDocument(prepared, highDepth: highDepth)
            if !highDepth, prepared.descriptor.state.selection.kind == "still",
               let index = prepared.descriptor.state.media.firstIndex(where: { $0.resource_id == "still" }) {
                let url = prepared.snapshot.urls[index]
                // The decoded image is the container's primary one, so its
                // metadata, PPI and mattes are read from that index too.
                let primary = try HostStillDecoder.inspect(url).index
                let metadata = ImageIOMetadata.read(url, index: primary)
                var captured = metadata.attaching(to: source)
                if let dpi = Bitmap.imageDPI(url, index: primary),
                   captured.document.resolution.x != Double(Float(dpi.0)) || captured.document.resolution.y != Double(Float(dpi.1)) {
                    guard let sized = try captured.document.settingResolution(x: dpi.0, y: dpi.1) else {
                        throw HostSourceError.failure("The captured print resolution could not be preserved.")
                    }
                    captured.document = sized
                }
                let final = AuxiliaryMattes.attaching(to: captured.document, from: url, index: primary)
                    ?? captured.document
                var report = captured.report
                if !prepared.descriptor.disclosures.isEmpty {
                    report["warnings"] = ((report["warnings"] as? [String]) ?? []) + prepared.descriptor.disclosures
                }
                return Opened(doc: final, metadataNotCaptured: !metadata.inspected, rawDevelop: nil, metadataReport: report)
            }
            var final = source
            if highDepth, prepared.descriptor.state.selection.kind == "still",
               let index = prepared.descriptor.state.media.firstIndex(where: { $0.resource_id == "still" }) {
                // Phase 11D: the TYPED still branch reconstructs exactly what
                // the byte branch does — the platform reader's capture packets
                // and the file's own PPI — through the same two facade calls,
                // which now have typed twins. So a typed export carries them
                // and the loss notice below no longer names them.
                let still = prepared.snapshot.urls[index]
                let primary = try HostStillDecoder.inspect(still).index
                let metadata = ImageIOMetadata.read(still, index: primary)
                var captured = metadata.attaching(to: source)
                if let dpi = Bitmap.imageDPI(still, index: primary),
                   captured.document.resolution.x != Double(Float(dpi.0))
                    || captured.document.resolution.y != Double(Float(dpi.1)) {
                    guard let sized = try captured.document.settingResolution(x: dpi.0, y: dpi.1) else {
                        throw HostSourceError.failure(
                            "The captured print resolution could not be preserved.")
                    }
                    captured.document = sized
                }
                final = try NativeAuxiliaryMattes.attaching(to: captured.document, from: still, index: primary)
                var report = captured.report
                report["container"] = "host_media"
                report["representation"] = "decoded_pixels_and_reconstructed_fields"
                var warnings = (report["warnings"] as? [String]) ?? []
                warnings.append("The decoded samples and source profile are kept; the original file is not. Capture packets and the original PPI are reconstructed from the platform reader's view, not the original packet bytes. Auxiliary mattes from selected stills retain their declared scalar precision and orientation in native channels.")
                warnings.append(contentsOf: prepared.descriptor.disclosures)
                report["warnings"] = warnings
                return Opened(doc: final, metadataNotCaptured: !metadata.inspected,
                              rawDevelop: nil, metadataReport: report)
            }
            let warnings = ["The decoded frame and source profile are kept; the original movie is not. Export capture packets and original PPI are not reconstructed for a selected MOVIE FRAME by this host-source import path."]
            return Opened(doc: final, metadataNotCaptured: true, rawDevelop: nil,
                metadataReport: ["container": "host_media", "metadata_walked": false,
                    "representation": "decoded_pixels", "captured_packets": [String](), "warnings": warnings])
        }, title: "Reading Media")
    }

    /// An unqualified high-depth platform format may not reach the historical
    /// byte display decoder merely because the preserving core refused it.
    static func hostFallbackWouldLosePrecision(_ url: URL) -> Bool {
        guard let source = CGImageSourceCreateWithURL(url as CFURL,
            [kCGImageSourceShouldCache: false] as CFDictionary),
            let info = CGImageSourceCopyPropertiesAtIndex(source, 0, nil) as? [CFString: Any],
            let depth = (info[kCGImagePropertyDepth] as? NSNumber)?.intValue else { return false }
        return depth > 8
    }
}
