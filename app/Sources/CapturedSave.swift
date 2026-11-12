import Foundation

// Atomic saves mark only the captured handle saved; newer edits stay dirty and undo to that handle
// is clean. Projection failure retains the prior display while native saving remains subject to its
// own capacity/admission checks.

/// One immutable revision and its export settings. Encoding and filesystem
/// replacement run on the bounded worker, with no NSDocument access there.
struct CapturedSave {
    let source: RasterDocument
    let projection: RasterImage?
    let type: String
    let quality: Int
    let embedProfile: Bool
    let stripMetadata: Bool
    let scratchEstimate: UInt64
    /// The job's own non-evictable retained input, passed to `admit`.
    let ownRetainedBytes: UInt64
    let precisionSave: PrecisionCapturedSave?
    let precisionExport: PrecisionExportCapture?
    /// A CMYK document's flat save (#1): `"tiff"` or `"jpeg"`, written by
    /// `dr_precision_doc_save_cmyk` (a JPEG through its 8-bit CMYK TIFF
    /// intermediate and `HostImageEncoder.cmykJPEG`); nil for every other save.
    let cmykFormat: String?
    /// A Lab Color document's TIFF save (#1 Lab phase 1b): the captured projection
    /// `dr_precision_doc_save_lab` writes as a TIFF 6.0 CIELab file; nil for every other save.
    var labTiff: RasterPrecisionImage?

    init(_ document: ImageDocument, type: String) throws {
        guard let source = document.doc else { throw DoctoredCoreError(message: "No image loaded.") }
        self.source = source
        self.type = type
        // #1: a byte flat save composites the spot channels' overprint, except a TIFF, which
        // keeps them as extra samples (1c-2a, `flatSaveProjection`); a typed one keeps them in a
        // TIFF where the core writes them (`keepSpotChannels` below,
        // `SpotChannels.flatTiffKeeps`).
        self.projection = type == ImageDocument.nativeTypeIdentifier || source.precision != nil
            ? nil : try document.flatSaveProjection(format: ExportFormat.from(fileType: type))
        self.quality = document.jpegExportQuality
        self.embedProfile = document.embedColorProfile
        self.stripMetadata = document.stripMetadata
        if let typed = source.precision, typed.isCmyk {
            // A CMYK document saves as .rz, TIFF or JPEG, flattened onto
            // paper for the two image formats; everything else refuses by name.
            precisionExport = nil
            if type == ImageDocument.nativeTypeIdentifier {
                let save = try PrecisionCapturedSave(typed, destination: .native)
                precisionSave = save
                cmykFormat = nil
                scratchEstimate = save.scratchEstimate
                ownRetainedBytes = save.ownRetainedBytes
                return
            }
            let format = ExportFormat.from(fileType: type)
            guard format == .tiff || format == .jpeg else {
                throw DoctoredCoreError(message: "A CMYK document saves as Doctored (.doctored), TIFF or JPEG; "
                    + "\(format?.displayName ?? type) is unavailable in a CMYK document. "
                    + "Convert to RGB Color (Image ▸ Mode) to save other formats.")
            }
            let name = format == .jpeg ? "jpeg_intermediate" : "tiff"
            let plan = try typed.saveCMYKPlan(format: name, embedProfile: embedProfile,
                                              stripMetadata: stripMetadata)
            if let refusal = plan["refusal"] as? String { throw DoctoredCoreError(message: refusal) }
            precisionSave = nil
            cmykFormat = format == .jpeg ? "jpeg" : "tiff"
            // The core's plan; a JPEG's ImageIO transcode runs after the core save has released
            // its scratch, so it is the larger of the two stages, not their sum: the transcode's
            // decoded copy (8 B/px) beside the intermediate 8-bit CMYK TIFF read whole (4 B/px,
            // its profile and a MiB of tags and metadata).
            let core = try PrecisionDocumentDescriptor.integer(plan, "working_memory_plan_bytes")
            let pixels = UInt64(source.width) &* UInt64(source.height)
            scratchEstimate = format == .jpeg
                ? max(core, pixels &* 12 &+ UInt64(typed.cmykProfileData.count) &+ 1024 * 1024)
                : core
            ownRetainedBytes = 0
            return
        }
        cmykFormat = nil
        // #1 Bitmap phase 1c: a Bitmap document saves as .rz, as a 1-bit PNG, TIFF or BMP keeping
        // its plane, or as JPEG through its 8-bit Grayscale render, all through
        // `PrecisionExportCapture` (the core's `bitmap_export` and kept-gray stages) at 8 bits with
        // no dither; every other format refuses (`BitmapDocument.saveRefusal`).
        if source.isBitmap, type != ImageDocument.nativeTypeIdentifier {
            guard source.precision != nil, let format = ExportFormat.from(fileType: type),
                  BitmapDocument.keptFormats.contains(format) || format == .jpeg else {
                throw DoctoredCoreError(message: BitmapDocument.saveRefusal)
            }
            let export = try PrecisionExportCapture(source: PrecisionExportSource(document), format: format,
                bitDepth: 8, dither: "none", quality: quality, embedProfile: embedProfile,
                stripMetadata: stripMetadata)
            precisionSave = nil
            precisionExport = export
            scratchEstimate = export.scratchEstimate
            ownRetainedBytes = 0
            return
        }
        // #1 Indexed phase 1b: an Indexed Color document saves as .rz, or as PNG, GIF or TIFF
        // keeping its palette through `PrecisionExportCapture` (the core's kept-Indexed stages) at
        // 8 bits with no dither; every other format refuses (`IndexedDocument.saveRefusal`).
        if source.isIndexed, type != ImageDocument.nativeTypeIdentifier {
            guard source.precision != nil, let format = ExportFormat.from(fileType: type),
                  IndexedDocument.keptFormats.contains(format) else {
                throw DoctoredCoreError(message: IndexedDocument.saveRefusal)
            }
            let export = try PrecisionExportCapture(source: PrecisionExportSource(document), format: format,
                bitDepth: 8, dither: "none", quality: quality, embedProfile: embedProfile,
                stripMetadata: stripMetadata)
            precisionSave = nil
            precisionExport = export
            scratchEstimate = export.scratchEstimate
            ownRetainedBytes = 0
            return
        }
        // #1 Lab phase 1b: a Lab Color document saves as .rz, or as a Lab TIFF through the core
        // (`lab_save`); every other format refuses (`LabDocument.saveRefusal`).
        if source.isLab, type != ImageDocument.nativeTypeIdentifier {
            guard let typed = source.precision, ExportFormat.from(fileType: type) == .tiff else {
                throw DoctoredCoreError(message: LabDocument.saveRefusal)
            }
            let projection = try PrecisionExportSource(document).projection
            let plan = try typed.saveLabPlan(projection: projection, stripMetadata: stripMetadata)
            if let refusal = plan["refusal"] as? String { throw DoctoredCoreError(message: refusal) }
            precisionSave = nil
            precisionExport = nil
            labTiff = projection
            scratchEstimate = try PrecisionDocumentDescriptor.integer(plan, "working_memory_plan_bytes")
            ownRetainedBytes = 0
            return
        }
        // #1 Duotone phase 1b: a Duotone document saves as .doctored; PSD and PSB are
        // File ▸ Export as PSD's, and every other format is an export through its inks.
        if source.isDuotone, type != ImageDocument.nativeTypeIdentifier {
            throw DoctoredCoreError(message: DuotoneDocument.saveRefusal)
        }
        // #1 phase 1b: a Grayscale document saves as .doctored, or as one-channel PNG, TIFF or
        // JPEG through `PrecisionExportCapture` (the core's kept-gray stages) at
        // `defaultBitDepth`.
        if source.isGray, type != ImageDocument.nativeTypeIdentifier,
           !(ExportFormat.from(fileType: type).map { [.png, .tiff, .jpeg].contains($0) } ?? false) {
            throw DoctoredCoreError(message: GrayDocument.saveRefusal)
        }
        if let typed = source.precision {
            if type == ImageDocument.nativeTypeIdentifier {
                let save = try PrecisionCapturedSave(typed, destination: .native)
                precisionSave = save
                precisionExport = nil
                scratchEstimate = save.scratchEstimate
                ownRetainedBytes = save.ownRetainedBytes
            } else {
                guard let format = ExportFormat.from(fileType: type) else {
                    throw DoctoredCoreError(message: "Cannot write files of type \(type).")
                }
                // The depth the loss list is read at, so Save keeps a file in place only when
                // what it writes reopens as the document (a 32-bit document's TIFF at 32).
                let bitDepth = PrecisionExportCapture.saveBitDepth(document, format: format)
                let export = try PrecisionExportCapture(source: PrecisionExportSource(document), format: format,
                    bitDepth: bitDepth,
                    // Dither applies only to an 8-bit reduction; a save at any other depth drops
                    // the export setting.
                    dither: bitDepth == 8 ? document.exportDither : "none",
                    quality: quality, embedProfile: embedProfile, stripMetadata: stripMetadata,
                    keepSpotChannels: format == .tiff)
                precisionSave = nil
                precisionExport = export
                scratchEstimate = export.scratchEstimate
                ownRetainedBytes = 0
            }
            return
        }
        precisionSave = nil
        precisionExport = nil
        if type == ImageDocument.nativeTypeIdentifier {
            self.scratchEstimate = try source.nativeSaveScratchBytes()
            // The document's own pixels, without its evictable caches.
            self.ownRetainedBytes = try source.ownRetainedBytes()
        } else {
            // …plus the spot channels a TIFF keeps (#1 1c-2a).
            let spots = ExportFormat.from(fileType: type) == .tiff ? SpotChannels.tiffSaveScratch(source) : 0
            self.scratchEstimate = Self.flatByteScratch(width: source.width, height: source.height) &+ spots
            self.ownRetainedBytes = 0
        }
    }

    /// A flat byte export's scratch: the projection, the encoder's copies and
    /// 16 MiB of codec state. Export Contents' PNG of a byte master uses it too.
    static func flatByteScratch(width: Int, height: Int) -> UInt64 {
        UInt64(width) * UInt64(height) * 12 + 16 * 1024 * 1024
    }

    var refusalRemedy: String {
        source.precision != nil ? RasterResources.depthJobRemedy : RasterResources.smallerJobRemedy
    }

    func write(to url: URL) throws {
        if let cmykFormat, let typed = source.precision {
            let context = try PrecisionWorkContext(source: typed)
            if cmykFormat == "tiff" {
                try typed.saveCMYK(to: url, format: "tiff", embedProfile: embedProfile,
                                   stripMetadata: stripMetadata, workingLimit: context.limit,
                                   otherRetained: context.otherRetained)
                return
            }
            let intermediate = StagedFile.url(beside: url, kind: .cmykIntermediate)
            defer { try? FileManager.default.removeItem(at: intermediate) }
            try typed.saveCMYK(to: intermediate, format: "jpeg_intermediate", embedProfile: embedProfile,
                               stripMetadata: stripMetadata, workingLimit: context.limit,
                               otherRetained: context.otherRetained)
            let jpeg = try HostImageEncoder.cmykJPEG(fromTIFF: Data(contentsOf: intermediate), quality: quality)
            try jpeg.write(to: url)
            return
        }
        if let labTiff, let typed = source.precision {
            let context = try PrecisionWorkContext(source: typed, image: labTiff)
            try typed.saveLab(to: url, projection: labTiff, stripMetadata: stripMetadata,
                              workingLimit: context.limit, otherRetained: context.otherRetained)
            return
        }
        if let precisionSave { try precisionSave.write(to: url); return }
        if let precisionExport { _ = try precisionExport.write(to: url); return }
        if type == ImageDocument.nativeTypeIdentifier { try source.saveNative(to: url); return }
        guard let format = ExportFormat.from(fileType: type), let projection else {
            throw DoctoredCoreError(message: "Cannot write files of type \(type).")
        }
        _ = try source.saveImage(projection, to: url, format: format.drFormat, jpegQuality: quality,
                                 embedProfile: embedProfile, stripMetadata: stripMetadata)
    }

    func writeSafely(to url: URL) throws {
        StagedFile.sweep(beside: url)
        let staged = StagedFile.url(beside: url, kind: .save)
        let files = FileManager.default
        defer { try? files.removeItem(at: staged) }
        do { try write(to: staged) }
        catch let error as DoctoredCoreError {
            throw DoctoredCoreError(message: error.message.replacingOccurrences(of: staged.path, with: url.path))
        }
        try DurableFile.publish(staged, to: url)
    }
}
