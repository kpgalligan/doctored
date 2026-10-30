import CoreGraphics

// For a refinement cutout, `PreparedDocumentEdit` consumes the already-rendered byte projection and
// admits its CGImage plus borrowed-alpha peak.
//
// `PreparedDocumentEdit` keeps authoritative typed projection separate from display, and
// `ImageDocument` prepares before publishing a typed edit/history entry.

/// Pixels and display projection evaluated on the same immutable revision.
/// CGImage construction is the existing PreviewRenderer seam, with no AppKit.
struct PreparedDocumentEdit {
    let document: RasterDocument
    let projection: RasterImage?
    let precisionProjection: RasterPrecisionImage?
    let preview: CGImage?
    let error: String?

    /// Optional control is a fresh compound child owned by the calling operation.
    /// Native composition and byte quantization poll independent core children.
    /// Legacy byte rendering and CoreGraphics construction have boundary checks;
    /// their existing internals remain noncancellable.
    init(_ document: RasterDocument, scratch: JobScratch? = nil,
         control: RasterJobControl? = nil, borrowedInputBytes: UInt64 = 0) {
        self.document = document
        do {
            try control?.beginCompoundOperation()
            let estimate = try document.projectionScratchEstimate()
            let hostEstimate = try PrecisionDocumentDescriptor.sum(estimate, borrowedInputBytes)
            _ = try RasterResources.shared.admit(
                scratch: hostEstimate, own: try document.ownRetainedBytes(),
                remedy: document.isHighDepth ? RasterResources.depthJobRemedy : RasterResources.smallerJobRemedy)
            scratch?.bytes = hostEstimate
            defer { scratch?.bytes = 0 }
            if document.isHighDepth {
                let projection: RasterPrecisionImage
                if let control {
                    projection = try control.withChildOperation(from: 0, to: 0.85) { child in
                        guard child.setScratchLimit(estimate) else {
                            throw DoctoredCoreError(message: "Could not configure native display memory.")
                        }
                        return try document.renderPrecision(control: child, additionalRetainedBytes: borrowedInputBytes)
                    }
                } else {
                    projection = try document.renderPrecision(additionalRetainedBytes: borrowedInputBytes)
                }
                let context = try PrecisionWorkContext(image: projection, additionalRetainedBytes: borrowedInputBytes)
                let display: RasterImage
                if let control {
                    display = try control.withChildOperation(from: 0.85, to: 0.98) { child in
                        guard child.setScratchLimit(estimate) else {
                            throw DoctoredCoreError(message: "Could not configure preview memory.")
                        }
                        return try document.display(of: projection, context: context,
                                                    control: child)
                    }
                } else {
                    display = try document.display(of: projection, context: context)
                }
                try Self.requireNotCancelled(control)
                let preview = display.makeCGImage(in: document.colorSpace)
                try Self.requireNotCancelled(control)
                self.precisionProjection = projection
                self.projection = nil
                self.preview = preview
            } else {
                try Self.requireNotCancelled(control)
                let projection = try document.render()
                try Self.requireNotCancelled(control)
                let preview = projection.makeCGImage(in: document.colorSpace)
                try Self.requireNotCancelled(control)
                self.precisionProjection = nil
                self.projection = projection
                self.preview = preview
            }
            self.error = nil
        } catch {
            self.projection = nil
            self.precisionProjection = nil
            self.preview = nil
            self.error = error.localizedDescription
        }
    }

    /// A qualified byte cutout has already used the shared controlled core
    /// compositor. Admit only its CGImage payload here; never render it twice.
    /// The retained control is checked at the platform-call boundaries.
    init(_ document: RasterDocument, renderedByteProjection: RasterImage,
         borrowedInputBytes: UInt64 = 0, control: RasterJobControl) {
        self.document = document
        do {
            try Self.requireNotCancelled(control)
            guard !document.isHighDepth, renderedByteProjection.width == document.width,
                  renderedByteProjection.height == document.height else {
                throw DoctoredCoreError(message: "Prepared byte projection does not match its document.")
            }
            _ = try RasterResources.shared.admit(scratch: PrecisionDocumentDescriptor.sum(
                UInt64(document.width * document.height) * 4 + 4096, borrowedInputBytes))
            let preview = renderedByteProjection.makeCGImage(in: document.colorSpace)
            try Self.requireNotCancelled(control)
            self.projection = renderedByteProjection
            self.precisionProjection = nil
            self.preview = preview
            self.error = nil
        } catch {
            self.projection = nil
            self.precisionProjection = nil
            self.preview = nil
            self.error = error.localizedDescription
        }
    }

    /// A render-neutral typed edit keeps the current projection and preview:
    /// nothing is rendered or admitted here.
    init(carrying document: RasterDocument, precisionProjection: RasterPrecisionImage, preview: CGImage) {
        self.document = document
        self.projection = nil
        self.precisionProjection = precisionProjection
        self.preview = preview
        self.error = nil
    }

    /// A render-neutral byte edit keeps the current projection and preview;
    /// nothing rendered or admitted.
    init(carrying document: RasterDocument, projection: RasterImage, preview: CGImage) {
        self.document = document
        self.projection = projection
        self.precisionProjection = nil
        self.preview = preview
        self.error = nil
    }

    private static func requireNotCancelled(_ control: RasterJobControl?) throws {
        guard control?.isCancelled != true else {
            throw JobCancelled()
        }
    }
}
