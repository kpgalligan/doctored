import AppKit

// Byte encoding reserves metadata copies as well as pixel buffers.

/// Capture the same immutable document and authoritative projection before a
/// panel or worker can service another edit. This cannot accept a byte preview.
struct PrecisionExportSource {
    let source: RasterDocument
    let projection: RasterPrecisionImage

    init(_ document: ImageDocument) throws {
        guard let source = document.doc, source.precision != nil else {
            throw DoctoredCoreError(message: "No 16-bit or 32-bit float document is loaded.")
        }
        let projection = try document.currentPrecisionProjection()
        guard document.doc === source else {
            throw DoctoredCoreError(message: "The document changed while its export projection was being prepared.")
        }
        self.source = source
        self.projection = projection
    }
}

/// A flat copy of one captured typed master. Only this explicit boundary may
/// quantize to byte storage; it never installs that temporary document.
struct PrecisionExportCapture {
    let source: RasterDocument
    let projection: RasterPrecisionImage?
    let format: ExportFormat
    let bitDepth: Int
    let dither: String
    let quality: Int
    let embedProfile: Bool
    let stripMetadata: Bool
    let scratchEstimate: UInt64
    let byteEncodingScratchEstimate: UInt64
    /// The export-workspace request a 32-bit float document's 16- or 32-bit
    /// copy encodes through, so its float stage tone-maps (16) or writes the
    /// samples (32) exactly as File > Export does; a Grayscale document's copy
    /// at every depth (#1 phase 1b), so the core's kept-gray stages write one
    /// channel with the Gray profile; an Indexed Color document's copy (#1
    /// Indexed phase 1b), so the core's kept-Indexed stages write the palette
    /// and every index; a copy of a document with spot channels (#1 1c-1),
    /// so the core composites their overprint; nil for every other copy.
    let floatRequest: String?
    /// The sample width of a 32-bit copy; `.f32` at every other depth.
    let floatSamples: ExportSettings.FloatSamples
    /// A TIFF copy that keeps the spot channels as extra samples (#1 1c-2a, the core's
    /// `"spot_channels":"keep"`) rather than compositing their overprint.
    let keepsSpotChannels: Bool

    init(source capture: PrecisionExportSource, format: ExportFormat, bitDepth: Int = 16,
         dither: String = "none", quality: Int = 90,
         embedProfile: Bool = true, stripMetadata: Bool = false,
         floatSamples: ExportSettings.FloatSamples = .f32, keepSpotChannels: Bool = false) throws {
        let source = capture.source
        guard let typed = source.precision, [8, 16, 32].contains(bitDepth),
              ["none", "ordered"].contains(dither), (1...100).contains(quality) else {
            throw DoctoredCoreError(message: "Choose 8, 16 or 32 bits, none or ordered dithering, and JPEG quality 1–100.")
        }
        // 32 bits is the float document's exact copy, in one of the three
        // float containers; OpenEXR and Radiance exist at no other depth.
        if bitDepth == 32 {
            guard typed.isFloat else {
                throw DoctoredCoreError(message: "A 32-bit float copy requires a 32-bit float document.")
            }
            guard format == .tiff || format.isFloatOnly else {
                throw DoctoredCoreError(message: "A 32-bit float copy requires tiff, exr or hdr.")
            }
        } else if format.isFloatOnly {
            throw DoctoredCoreError(message: "\(format.displayName) is written at 32 bits from a 32-bit float document; set bit_depth to 32.")
        }
        guard bitDepth != 16 || format == .png || format == .tiff else {
            throw DoctoredCoreError(message: "\(format.displayName) requires an explicit 8-bit export choice. Use PNG or TIFF to retain 16-bit precision.")
        }
        guard bitDepth == 8 || dither == "none" else {
            throw DoctoredCoreError(message: "Dithering applies only when reducing a 16-bit source to 8 bits.")
        }
        self.source = source
        self.projection = bitDepth == 8 && !source.isGray && !source.isBitmap ? nil : capture.projection
        self.format = format
        self.bitDepth = bitDepth
        self.dither = dither
        self.quality = quality
        self.embedProfile = embedProfile
        self.stripMetadata = stripMetadata
        self.floatSamples = floatSamples
        self.keepsSpotChannels = keepSpotChannels && format == .tiff && source.hasSpotChannels
        if source.isGray || source.isIndexed || source.isBitmap || source.hasSpotChannels {
            // #1 phase 1b: profile document keeps gray (`gray_export`), and an Indexed document's
            // PNG, GIF or TIFF keeps its palette (`indexed_export`); since Bitmap phase 1c a Bitmap
            // document's PNG, TIFF or BMP keeps its 1-bit plane (`bitmap_export`) and its JPEG is
            // the Gray storage's; since spot channels 1c-1 a document with spot channels composites
            // their overprint in the core's flattened export (`spot_typed`), and since 1c-2a its
            // TIFF keeps them as extra samples (`tiff_spot`); the plan is the core's.
            let request = ExportSettings(format: format, jpegQuality: quality, bitDepth: bitDepth, dither: dither,
                size: .none, filter: DR_FILTER_LANCZOS3, profile: .document, sharpen: nil,
                embedProfile: embedProfile, stripMetadata: stripMetadata, metadata: nil,
                floatSamples: floatSamples, keepSpotChannels: keepsSpotChannels).requestJSON
            floatRequest = request
            let plan = try typed.exportPlan(projection: capture.projection, request: request)
            scratchEstimate = try PrecisionDocumentDescriptor.integer(plan, "scratch_bytes")
            byteEncodingScratchEstimate = 0
        } else if bitDepth != 8 && typed.isFloat {
            let request = ExportSettings(format: format, jpegQuality: quality, bitDepth: bitDepth, dither: "none",
                size: .none, filter: DR_FILTER_LANCZOS3, profile: .document, sharpen: nil,
                embedProfile: embedProfile, stripMetadata: stripMetadata, metadata: nil,
                floatSamples: floatSamples).requestJSON
            floatRequest = request
            let plan = try typed.exportPlan(projection: capture.projection, request: request)
            scratchEstimate = try PrecisionDocumentDescriptor.integer(plan, "scratch_bytes")
            byteEncodingScratchEstimate = 0
        } else if bitDepth == 16 {
            floatRequest = nil
            let plan = try typed.projectionExportPlan(capture.projection)
            scratchEstimate = try PrecisionDocumentDescriptor.integer(plan, "host_scratch_bytes")
            byteEncodingScratchEstimate = 0
        } else {
            floatRequest = nil
            let plan = try typed.downconvertPlan()
            let conversion = try PrecisionDocumentDescriptor.integer(plan, "scratch_bytes")
            // Conversion and encoding are sequential. Reserve the larger stage
            // plus the temporary byte source, projection and encoder growth.
            let info = try typed.descriptor()
            guard let packets = info["metadata_bytes"] as? [String: Any] else {
                throw DoctoredCoreError(message: "The captured document has no metadata memory plan.")
            }
            var payload = try PrecisionDocumentDescriptor.integer(info, "profile_bytes")
            for kind in ["exif", "xmp", "iptc"] {
                payload = try PrecisionDocumentDescriptor.sum(payload,
                    PrecisionDocumentDescriptor.integer(packets, kind))
            }
            let (metadataCopies, overflow) = payload.multipliedReportingOverflow(by: 8)
            guard !overflow else { throw DoctoredCoreError(message: "Export metadata estimate overflow.") }
            let encoding = try PrecisionDocumentDescriptor.sum(metadataCopies,
                UInt64(source.width) * UInt64(source.height) * 24 + 64 * 1024 * 1024)
            byteEncodingScratchEstimate = encoding
            scratchEstimate = max(conversion, encoding)
        }
    }

    func losses() throws -> [String] {
        // #1 Bitmap phase 1c: a Bitmap copy (1-bit PNG, TIFF or BMP, or JPEG through Gray)
        // loses only what `BitmapDocument.saveLosses` names.
        if source.isBitmap {
            return try BitmapDocument.saveLosses(source, format: format, embedProfile: embedProfile,
                                             stripMetadata: stripMetadata)
        }
        // #1 Indexed phase 1b: a kept-Indexed copy (PNG, GIF or TIFF with profile document)
        // loses only what `IndexedDocument.saveLosses` names.
        if source.isIndexed, IndexedDocument.keptFormats.contains(format) {
            return try IndexedDocument.saveLosses(source, format: format, embedProfile: embedProfile,
                                              stripMetadata: stripMetadata)
        }
        // An 8- or 16-bit Grayscale copy (one channel) loses what Save would at its depth
        // (`GrayDocument.saveLosses`); a 32-bit one names its tone map below.
        var result = source.isGray && !source.isDuotone && !source.isFloat
            && [.png, .tiff, .jpeg].contains(format)
            ? try GrayDocument.saveLosses(source, format: format, bitDepth: bitDepth,
                                          embedProfile: embedProfile, stripMetadata: stripMetadata)
            : Self.losses(format: format, bitDepth: bitDepth, isFloat: source.isFloat,
                          embedProfile: embedProfile, stripMetadata: stripMetadata,
                          floatSamples: floatSamples)
        // #1 1c-1: the flattened copy carries the spot channels' overprint, not the channels;
        // a TIFF that keeps them (1c-2a) loses nothing.
        if source.hasSpotChannels, !keepsSpotChannels { result.append(SpotChannels.flatLoss) }
        return result
    }

    static func losses(format: ExportFormat, bitDepth: Int, isFloat: Bool, embedProfile: Bool,
                       stripMetadata: Bool,
                       floatSamples: ExportSettings.FloatSamples = .f32) -> [String] {
        var result = ["editable document mode, embedded image originals, layers, masks, saved channels, guides, the ruler origin, layer comps and the global light (flattened copy)"]
        if isFloat && bitDepth == 32 {
            // The exact copy keeps the float samples; each container names
            // what it has no place for.
            if format == .hdr {
                result.append("transparency (Radiance has no alpha: each colour is multiplied by its alpha and the file is opaque)")
                result.append("values below zero (written as zero)")
                result.append("32-bit float precision (RGBE keeps an 8-bit mantissa per channel under one shared exponent)")
            } else if floatSamples == .f16 {
                result.append("32-bit float precision (each sample is rounded to the nearest 16-bit half)")
            }
            if format.isFloatOnly {
                result.append("the embedded colour profile, capture metadata and print resolution "
                    + "(\(format.displayName) carries none; the file states the document profile's primaries instead)")
                return result
            }
        } else if isFloat && bitDepth == 16 {
            result.append("32-bit float values and the range above white "
                + "(tone-mapped to 16-bit codes through the document's tone map)")
        } else if isFloat && bitDepth == 8 {
            result.append("32-bit float values and the range above white "
                + "(tone-mapped through the document's tone map and reduced to 256 levels)")
        } else if bitDepth == 8 {
            result.append("16-bit channel precision (reduced to 256 levels)")
        }
        if (format == .heic || format == .avif) && bitDepth == 16 {
            result.append("16-bit channel precision beyond the host \(format.displayName) encoder's depth "
                + "(8 or 10 bits; the export result reports what was written)")
        }
        if format == .jpeg { result.append("transparency (composited onto white) and lossless pixel values") }
        if format == .gif { result.append("full-color and fractional-alpha values (palette encoding)") }
        if !embedProfile || !ExportCapabilities.canEmbedProfile(format) { result.append("embedded color profile") }
        if stripMetadata { result.append("capture metadata") }
        return result
    }

    func write(to url: URL) throws -> RasterSaveReport {
        guard let typed = source.precision else { throw DoctoredCoreError(message: "The captured source is not 16-bit or 32-bit float.") }
        let context = try PrecisionWorkContext(source: typed, image: projection)
        if let floatRequest {
            // The export workspace's own float chain: stage 0 tone-maps to
            // u16 codes, then the same 16-bit codec and injector a projection
            // save uses.
            let result = try typed.exportEncoded(projection: projection, request: floatRequest,
                workingLimit: context.limit, otherRetained: context.otherRetained)
            try result.write(to: url, overwrite: true)
            return try result.saveReport()
        }
        if bitDepth == 16 {
            guard let projection else {
                throw DoctoredCoreError(message: "The captured 16-bit export has no authoritative projection.")
            }
            let carried = try typed.saveProjection16(projection, to: url, format: format == .png ? .png16 : .tiff16,
                embedProfile: embedProfile, stripMetadata: stripMetadata,
                workingLimit: context.limit, otherRetained: context.otherRetained)
            return RasterSaveReport(carried: carried)
        }
        let byteDocument = try typed.downconverted(dither: dither == "ordered" ? 1 : 0,
            workingLimit: context.limit, otherRetained: context.otherRetained)
        // Encode the quantizer's raster directly. Rendering it again could
        // discard straight RGB where its alpha rounded down to zero.
        guard let image = byteDocument.layerImage(0) else {
            throw DoctoredCoreError(message: "The deliberately converted export has no raster source.")
        }
        _ = try RasterResources.shared.admit(scratch: byteEncodingScratchEstimate)
        return try byteDocument.saveImage(image, to: url, format: format.drFormat,
            jpegQuality: quality, embedProfile: embedProfile, stripMetadata: stripMetadata)
    }

    func writeSafely(to url: URL) throws -> RasterSaveReport {
        StagedFile.sweep(beside: url)
        let staged = StagedFile.url(beside: url, kind: .export)
        defer { try? FileManager.default.removeItem(at: staged) }
        let result: RasterSaveReport
        do { result = try write(to: staged) }
        catch { throw DoctoredCoreError(message: error.localizedDescription.replacingOccurrences(of: staged.path, with: url.path)) }
        try DurableFile.publish(staged, to: url)
        return result
    }

    func run(to url: URL) throws -> RasterSaveReport {
        _ = try RasterResources.shared.admit(scratch: scratchEstimate)
        return try BackgroundWork.wait(try WorkerTask(scratchEstimate: scratchEstimate) {
            try writeSafely(to: url)
        }, title: "Exporting \(bitDepth)-bit Copy")
    }
}
