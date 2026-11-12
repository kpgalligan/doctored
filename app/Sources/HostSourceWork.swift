import AppKit

struct PreparedHostSource {
    let asset: RasterDocument
    let descriptor: HostSourceDescriptor
    let snapshot: HostSourceSnapshot
}

/// One captured source, one bounded worker at a time, one central publication.
/// ImageIO/AVFoundation decode has stage cancellation; Rust pixel work polls.
/// The decoded samples land as an ordinary raster layer at the document's
/// depth: the core's own source render places them (profile conversion, fit
/// and depth), and the binding is rasterized in the same edit, so no original
/// file or decoded master stays in the document (2026-10-10).
enum HostSourceWork {
    static func capture(_ input: HostSourceInput, selection: HostFrameSelection? = nil,
                        expected: [String]? = nil) throws -> PreparedHostSource {
        // A pair holding an HDR movie refuses as a pair: opening either half
        // opens the pair, so the bare movie's advice would not help.
        if input.entries.count == 2, let movie = input.entries.first(where: { $0.role == "video" }),
           let signal = try HostVideoDecoder.inspect(movie.url).hdr {
            throw HostSourceError.failure(HostVideoDecoder.hdrPairRefusal(signal))
        }
        _ = try RasterResources.shared.admit(scratch: input.scratch)
        return try PrecisionCapturedWork.run(scratchEstimate: input.scratch, title: "Capturing Full Media Source") { control in
            let snapshot = try HostSourceSnapshot.capture(input)
            if let expected, expected != snapshot.hashes {
                throw HostSourceError.failure("The source bytes differ from the recorded originals. No media decoder was invoked.")
            }
            var media: [HostMediaContract] = [], resources: [HostSourceResource] = []
            var chosen: RasterDocument?
            var selected = selection ?? (input.entries.contains(where: { $0.role == "still" }) ? .still : .video(HostMediaTime(0, 1)))
            var canvasWidth = 0, canvasHeight = 0
            for (index, entry) in input.entries.enumerated() {
                let outcome = try control.withChildOperation(from: Double(index) / Double(input.entries.count),
                    to: Double(index + 1) / Double(input.entries.count)) { child -> (RasterDocument, HostMediaContract, HostFrameSelection?) in
                    if entry.role == "still" {
                        let decoded = try HostStillDecoder.decode(snapshot.urls[index],
                            filename: entry.url.lastPathComponent, maximumScratch: input.scratch, control: child)
                        return (decoded.0, decoded.1, nil)
                    }
                    let decoded = try HostVideoDecoder.decode(snapshot.urls[index], filename: entry.url.lastPathComponent,
                        requested: selection?.kind == "video" ? selection?.requested : nil,
                        maximumScratch: input.scratch, control: child)
                    return (decoded.0, decoded.1, decoded.2)
                }
                let asset = outcome.0, contract = outcome.1
                if selected.kind == "video", let frame = outcome.2 { selected = frame }
                guard contract.width == entry.width, contract.height == entry.height else {
                    throw HostSourceError.failure("The source dimensions changed during capture. Try again.")
                }
                if entry.role == "still" || canvasWidth == 0 { canvasWidth = asset.width; canvasHeight = asset.height }
                if entry.role == selected.kind { chosen = asset }
                media.append(contract)
                resources.append(HostSourceResource(id: entry.role, media_type: contract.format,
                    sha256: snapshot.hashes[index], byte_length: UInt64(snapshot.bytes[index].count)))
            }
            guard let asset = chosen else { throw HostSourceError.failure("The requested still or movie resource does not exist.") }
            let state = HostSourceState(kind: media.count == 2 ? "live_photo" : selected.kind,
                pairing: media.count == 2 ? "basename" : "none", canvas_width: canvasWidth,
                canvas_height: canvasHeight, media: media, selection: selected)
            return PreparedHostSource(asset: asset, descriptor: HostSourceDescriptor(state: state, resources: resources), snapshot: snapshot)
        }
    }

    /// The opened document: the decoded frame fitted to its canvas as ONE
    /// raster layer, in the source's profile.
    static func initialDocument(_ prepared: PreparedHostSource, highDepth: Bool) throws -> RasterDocument {
        guard let profile = prepared.asset.iccProfile else { throw HostSourceError.failure("The decoded source profile is unavailable.") }
        let state = prepared.descriptor.state
        let canvas: RasterDocument
        if highDepth {
            canvas = try RasterDocument.blankRGBA16(width: state.canvas_width, height: state.canvas_height,
                fill: [0, 0, 0, 0], profile: profile, layerName: "Background")
        } else {
            guard let image = RasterImage.from(rgba: [UInt8](repeating: 0, count: state.canvas_width * state.canvas_height * 4),
                width: state.canvas_width, height: state.canvas_height), let raw = RasterDocument.from(image: image) else {
                throw HostSourceError.failure("Could not create the media canvas.")
            }
            if raw.iccProfile == profile { canvas = raw }
            else {
                guard let tagged = try raw.assigningProfile(profile) else { throw HostSourceError.failure("The media canvas profile could not be preserved.") }
                canvas = tagged
            }
        }
        let id = UUID().uuidString
        let request = try PrecisionPixelCommand(["op": "capture_raster", "layer": 0, "source_id": id,
            "depth_conversion": "convert_fallback",
            "source_transform": matrix(fit(state, width: prepared.asset.width, height: prepared.asset.height))])
        guard let source = try canvas.editingRetainedSource(asset: prepared.asset, request: request.request) else {
            throw HostSourceError.failure("The decoded media source could not be rendered.")
        }
        let rasterize = try PrecisionPixelCommand(["op": "rasterize", "layer": 0])
        guard let raster = try source.editingRetainedSource(request: rasterize.request) else {
            throw HostSourceError.failure("The decoded media source could not be rasterized.")
        }
        return raster
    }

    /// `insertion` names WHERE the new layer lands (12E-2b); nil means the
    /// UI's own decision, `.above` the active entry. `storage` only names the
    /// history state. Returns the source's disclosures once the edit is
    /// published.
    @discardableResult
    static func place(in document: ImageDocument, url: URL, name: String,
                      x: Int = 0, y: Int = 0, storage: ImageSourceStorage = .embedded,
                      selection: HostFrameSelection? = nil, expected: [String]? = nil,
                      insertion: LayerInsertion? = nil,
                      record: [ActionStep] = .notACommand) throws -> [String] {
        let capture = try ImageSourceCapture(document)
        let input = try HostSourceInput(url: url)
        let prepared = try self.capture(input, selection: selection, expected: expected)
        try capture.requireCurrent(document)
        if let expected, expected != prepared.descriptor.resources.map(\.sha256) {
            throw HostSourceError.failure("The source bytes differ from this recorded action. No image was placed.")
        }
        let id = UUID().uuidString
        // The slot the new layer lands at — the CORE's rule, carried in the
        // request as the `anchor`/`position` pair. The rasterization below is
        // of that landing index, never of `layerCount - 1`.
        let placement = insertion ?? (document.activeLayerIndex, LayerInsertPosition.above)
        let landing = placement.position.landingIndex(in: capture.source, anchor: placement.anchor)
        let place = try PrecisionPixelCommand(["op": "place", "source_id": id, "revision_id": id,
            "depth_conversion": ImageSourceDepthConversion.convertFallback.rawValue,
            "name": name, "x": x, "y": y,
            "anchor": placement.anchor, "position": placement.position.rawValue,
            "source_transform": matrix(fit(prepared.descriptor.state, width: prepared.asset.width, height: prepared.asset.height))])
        let rasterize = try PrecisionPixelCommand(["op": "rasterize", "layer": landing])
        let landed = try RetainedImageWork.edit(in: document, source: capture.source,
            steps: [(asset: prepared.asset, request: place), (asset: nil, request: rasterize)],
            title: LinkedImageWork.placeTitle(storage), record: record, landing: landing)
        return landed ? prepared.descriptor.disclosures : []
    }

    static func fit(_ state: HostSourceState, width: Int, height: Int) -> CGAffineTransform {
        let scale = min(Double(state.canvas_width) / Double(width), Double(state.canvas_height) / Double(height))
        return CGAffineTransform(a: scale, b: 0, c: 0, d: scale,
            tx: (Double(state.canvas_width) - Double(width) * scale) / 2,
            ty: (Double(state.canvas_height) - Double(height) * scale) / 2)
    }
    static func matrix(_ value: CGAffineTransform) -> [Double] {
        [value.a, value.b, value.c, value.d, value.tx, value.ty].map(Double.init)
    }
}
