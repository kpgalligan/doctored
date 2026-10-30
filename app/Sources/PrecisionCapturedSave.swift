import Foundation

// `RasterPrecisionDocument` / `RasterPrecisionImage` owners live in `DoctoredCore.swift`; exact u16
// sources never accept byte previews or working-space fallback. Native-signature failures stop
// before ImageIO. Preserving decode and facade preparation use the existing worker. The weak
// resource registry includes typed captures/projections; core admission enforces typed codec/native
// plans.

/// Internal typed capture, separate from ImageDocument and its byte projection.
/// No editor mode, Undo registration or saved-file marker is created here.
struct PrecisionCapturedSave {
    enum Destination {
        case native
        case image(PrecisionExportFormat)
    }

    let source: RasterPrecisionDocument
    let destination: Destination
    let embedProfile: Bool
    let stripMetadata: Bool
    let descriptor: PrecisionDocumentDescriptor
    let scratchEstimate: UInt64
    let ownRetainedBytes: UInt64

    init(_ source: RasterPrecisionDocument, destination: Destination,
         embedProfile: Bool = true, stripMetadata: Bool = false) throws {
        self.source = source
        self.destination = destination
        self.embedProfile = embedProfile
        self.stripMetadata = stripMetadata
        let info = try source.descriptor()
        descriptor = try PrecisionDocumentDescriptor(info)
        switch destination {
        case .native:
            scratchEstimate = descriptor.nativeScratchBytes
            ownRetainedBytes = try PrecisionDocumentDescriptor.integer(info, "retained_bytes")
        case .image:
            ownRetainedBytes = 0
            guard let estimate = descriptor.exportScratchBytes else {
                throw DoctoredCoreError(message: descriptor.renderRefusal
                    ?? "This high-depth document cannot be rendered for export.")
            }
            scratchEstimate = estimate
        }
    }

    /// Encodes the captured typed source directly. An RGBA8 preview cannot be
    /// supplied to this API. Core admission independently checks the operation.
    @discardableResult
    func write(to url: URL, additionalRetainedBytes: UInt64 = 0) throws -> UInt32 {
        let budget = try PrecisionWorkContext(source: source,
                                              additionalRetainedBytes: additionalRetainedBytes)
        switch destination {
        case .native:
            try source.saveNative(to: url, workingLimit: budget.limit,
                                  otherRetained: budget.otherRetained)
            return 0
        case .image(let format):
            return try source.export16(to: url, format: format, embedProfile: embedProfile,
                                       stripMetadata: stripMetadata, workingLimit: budget.limit,
                                       otherRetained: budget.otherRetained)
        }
    }

    /// Failed encoding leaves an existing destination intact. Only the staged
    /// file owned by this call is removed; publication happens after encoding.
    @discardableResult
    func writeSafely(to url: URL) throws -> UInt32 {
        let staged = url.deletingLastPathComponent().appendingPathComponent(
            ".doctored-precision-save-\(UUID().uuidString)")
        defer { try? FileManager.default.removeItem(at: staged) }
        let carried: UInt32
        do { carried = try write(to: staged) }
        catch {
            throw DoctoredCoreError(message: error.localizedDescription
                .replacingOccurrences(of: staged.path, with: url.path))
        }
        try DurableFile.publish(staged, to: url)
        return carried
    }

    /// The internal host path uses the existing bounded worker and makes no
    /// claim of cooperative codec cancellation. Capture remains value-only.
    @discardableResult
    func save(to url: URL) throws -> UInt32 {
        _ = try RasterResources.shared.admit(scratch: scratchEstimate)
        let task = try WorkerTask(scratchEstimate: scratchEstimate) { try writeSafely(to: url) }
        return try BackgroundWork.wait(task, title: "Saving Captured High-Depth Source")
    }
}

/// Values are read from the typed core contract, not inferred from a display
/// image or a filename. Unexpected/missing fields refuse before host I/O.
struct PrecisionDocumentDescriptor {
    static let storage = "rgba16_unorm"
    /// Phase 11F: a typed document is either the u16 mode or the 32-bit
    /// FLOAT one. A storage name outside the pair refuses, which is how an
    /// older build meeting a newer document stops rather than misreading it.
    static let floatStorage = "rgba32_float"
    /// #1: a CMYK document's straight CMYKA16 ink (`cmyk_document`).
    static let cmykStorage = "cmyka16_unorm"
    static let admittedStorages = [storage, floatStorage, cmykStorage]
    static let evaluation = "encoded_rgb_f32_v1"
    /// The two tags this build admits: the 7B default and Phase 11E's
    /// linear-light policy (`DocumentEvaluation.swift`). A tag outside the
    /// pair refuses, which is how an older build meeting a newer document
    /// stops rather than misreading it.
    static let admittedEvaluations = [evaluation, "linear_light_f32_v1", "cmyk_normal_v1"]
    let evaluationPolicy: String
    /// Which typed mode this document is in.
    let storageName: String
    var isFloat: Bool { storageName == Self.floatStorage }
    var isCmyk: Bool { storageName == Self.cmykStorage }
    let width: Int
    let height: Int
    let nativeScratchBytes: UInt64
    let exportScratchBytes: UInt64?
    let renderRefusal: String?
    let decodeScratchBytes: UInt64
    let reopenedRetainedBytes: UInt64

    init(_ info: [String: Any]) throws {
        guard let storage = info["storage"] as? String,
              Self.admittedStorages.contains(storage),
              let policy = info["evaluation"] as? String,
              Self.admittedEvaluations.contains(policy) else {
            throw DoctoredCoreError(message: "Unsupported high-depth storage or evaluation policy.")
        }
        storageName = storage
        evaluationPolicy = policy
        let width = try Self.integer(info, "width")
        let height = try Self.integer(info, "height")
        guard width > 0, height > 0, width <= UInt64(RasterImage.maxResizePixels) / height else {
            throw DoctoredCoreError(message: "Invalid high-depth document dimensions.")
        }
        self.width = Int(width)
        self.height = Int(height)
        nativeScratchBytes = try Self.integer(info, "native_scratch_bytes")
        exportScratchBytes = info["export_scratch_bytes"] is NSNull
            ? nil : try Self.integer(info, "export_scratch_bytes")
        renderRefusal = info["render_refusal"] as? String
        decodeScratchBytes = try Self.integer(info, "decode_scratch_bytes")
        reopenedRetainedBytes = try Self.integer(info, "reopen_retained_bytes")
    }

    static func integer(_ object: [String: Any], _ key: String) throws -> UInt64 {
        guard let number = object[key] as? NSNumber, let value = UInt64(number.stringValue) else {
            throw DoctoredCoreError(message: "Missing or invalid high-depth resource field: \(key).")
        }
        return value
    }

    static func sum(_ first: UInt64, _ second: UInt64) throws -> UInt64 {
        let (value, overflow) = first.addingReportingOverflow(second)
        guard !overflow else {
            throw DoctoredCoreError(message: "High-depth working-memory calculation overflow.")
        }
        return value
    }
}

/// The typed core accounts its own source once. Other live byte/typed handles
/// and pending previews remain in the caller's allowance. The current worker's
/// reservation is not included twice: the core adds its operation scratch.
struct PrecisionWorkContext {
    let limit = RasterResources.shared.workingLimit
    let otherRetained: UInt64

    /// `byteImage` is a flat byte projection (the byte soft proof's input):
    /// its own unique retained bytes are subtracted like the typed source's.
    init(source: RasterPrecisionDocument? = nil, image: RasterPrecisionImage? = nil,
         byteImage: RasterImage? = nil, additionalRetainedBytes: UInt64 = 0) throws {
        let report = try RasterResources.shared.report()
        let retained = try PrecisionDocumentDescriptor.integer(report, "unique_retained_bytes")
        let transient = try PrecisionDocumentDescriptor.integer(report, "transient_estimate_bytes")
        var sourceBytes: UInt64 = 0
        if source != nil || image != nil {
            let own = try RasterPrecisionDocument.resourceReport(
                documents: source.map { [$0] } ?? [], images: image.map { [$0] } ?? [])
            sourceBytes = try PrecisionDocumentDescriptor.integer(own, "unique_retained_bytes")
        }
        if let byteImage {
            let own = try RasterDocument.resourceReport(
                documents: [], images: [byteImage], scratch: 0, limit: 0)
            sourceBytes = try PrecisionDocumentDescriptor.sum(
                sourceBytes, PrecisionDocumentDescriptor.integer(own, "unique_retained_bytes"))
        }
        guard sourceBytes <= retained else {
            throw DoctoredCoreError(message: "The source image is missing from resource accounting.")
        }
        otherRetained = try PrecisionDocumentDescriptor.sum(
            PrecisionDocumentDescriptor.sum(retained - sourceBytes, transient), additionalRetainedBytes)
    }
}
