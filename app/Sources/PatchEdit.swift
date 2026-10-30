import AppKit

// Native Patch commits route through the shared retouch healer and 8-bit commits build their
// overlay on main and solve `healJob` on the worker (`startByte`), both as one nonmodal job.

/// A placed Patch region owns its source and options until release or cancel.
/// The preview is display-only; committed native RGB comes from the same
/// immutable document, with scalar region coverage supplied independently.
final class PatchEditCapture {
    /// The refusal for a displacement that carries the region and its source
    /// off the canvas, at both depths; `patch_region` rewords it by value.
    static let missesCanvas = "The displaced Patch region and source do not overlap the canvas."
    /// The refusal for a target that is not a raster layer's RGB. Content-Aware
    /// Move shares this capture and rewords it by value (`ContentAwareMoveWork.reworded`).
    static let requiresLayerRGB =
        "Patch requires raster layer RGB; masks, channels and Quick Mask are unavailable."

    let capture: ImageSourceCapture
    let layer: Int
    let choice: RetouchSamplingChoice
    let destination: Bool
    let bytePlate: RetouchPlate?
    let nativePlate: RasterPrecisionImage?
    let previewImage: CGImage?

    init(document: ImageDocument, layer: Int, choice: RetouchSamplingChoice,
         destination: Bool, preview: Bool) throws {
        capture = try ImageSourceCapture(document)
        guard document.planeEditTarget == .layer, !document.quickMaskActive,
              let info = capture.source.layerInfo(layer), !info.isGroup,
              !capture.source.layerIsAdjustment(layer) else {
            throw DoctoredCoreError(message: Self.requiresLayerRGB)
        }
        self.layer = layer
        self.choice = choice
        self.destination = destination
        if let precision = capture.source.precision {
            bytePlate = nil
            if preview {
                let plate = try precision.retouchSource(layer: layer, sample: choice.mode,
                    ignoreAdjustments: choice.ignoreAdjustments)
                let budget = try PrecisionWorkContext(image: plate)
                let display = try capture.source.display(of: plate, context: budget)
                nativePlate = plate
                previewImage = display.makeCGImage(in: capture.source.colorSpace)
            } else {
                nativePlate = nil
                previewImage = nil
            }
        } else {
            guard let plate = try RetouchSource.plate(capture.source, layer: layer, choice: choice) else {
                throw DoctoredCoreError(message: RetouchSource.missingPlate(choice.mode))
            }
            bytePlate = plate
            nativePlate = nil
            previewImage = preview ? plate.cgImage(in: capture.source.colorSpace) : nil
        }
        if preview, previewImage == nil {
            throw DoctoredCoreError(message: "Could not display the captured Patch source.")
        }
    }

    func nativeWork(_ result: PatchSession.Result) throws -> PatchNativeWork {
        guard let source = capture.source.precision else {
            throw DoctoredCoreError(message: "Patch has no native source owner.")
        }
        let width = capture.source.width, height = capture.source.height
        guard result.region.canvasWidth == width, result.region.canvasHeight == height else {
            throw DoctoredCoreError(message: "The Patch region belongs to a different canvas.")
        }
        let geometry = PatchSession.geometry(offset: result.offset, destination: destination)
        let canvas = CGRect(x: 0, y: 0, width: width, height: height)
        let bounds = result.region.bounds.offsetBy(dx: geometry.clip.dx, dy: geometry.clip.dy)
            .intersection(canvas)
        let sampled = canvas.offsetBy(dx: geometry.image.dx, dy: geometry.image.dy)
        guard !bounds.isNull, !bounds.isEmpty, !bounds.intersection(sampled).isEmpty else {
            throw DoctoredCoreError(message: Self.missesCanvas)
        }
        let x = Int(floor(bounds.minX)), y = Int(floor(bounds.minY))
        let right = min(width, Int(ceil(bounds.maxX))), bottom = min(height, Int(ceil(bounds.maxY)))
        let mapping = try RetouchGeometry(matrix: CGAffineTransform(
            translationX: -geometry.image.dx, y: -geometry.image.dy))
        let command = try PrecisionPixelCommand.retouch(layer: layer, heal: true,
            geometry: mapping, choice: choice, output: .layer, expand: false, opacity: 1,
            bounds: (x, y, right - x, bottom - y))
        // Preflight the complete operation before allocating canvas coverage.
        let context = try PrecisionWorkContext(source: source)
        let plan = try source.pixelEditPlan(request: command.request, workingLimit: context.limit,
            otherRetained: context.otherRetained)
        _ = try RasterResources.shared.admit(scratch: UInt64(width) * UInt64(height) * 5 + 4 * 1024 * 1024)
        guard !BackgroundWork.occupied else {
            throw DoctoredCoreError(message: BackgroundWork.preparingBusyMessage)
        }
        guard try PrecisionDocumentDescriptor.integer(plan, "input_samples") == UInt64(width) * UInt64(height),
              plan["output_samples"] == nil else {
            throw DoctoredCoreError(message: "High-depth input samples do not match the planned edit.")
        }
        guard try PrecisionDocumentDescriptor.integer(plan, "input_width") == UInt64(width),
              try PrecisionDocumentDescriptor.integer(plan, "input_height") == UInt64(height),
              try PrecisionDocumentDescriptor.integer(plan, "input_planes") == 1 else {
            throw DoctoredCoreError(message: "High-depth scalar layout does not match the planned edit extent.")
        }
        let region = result.region.maskSamples16()
        // A displaced Destination region is placed on the worker, not
        // in a modal wait before the job.
        let placement: NativeSelectionWork? = try geometry.clip.dx == 0 && geometry.clip.dy == 0 ? nil
            : NativeSelectionWork(width: width, height: height,
                operation: ["kind": "transform", "matrix": [1.0, 0, 0, 1, Double(geometry.clip.dx), Double(geometry.clip.dy)], "filter": "nearest"],
                input: region)
        return try PatchNativeWork(source: source, command: command, region: region, placement: placement,
            scratchEstimate: PrecisionDocumentDescriptor.sum(PrecisionDocumentDescriptor.integer(plan, "scratch_bytes"),
                placement?.scratchEstimate ?? 0),
            retainedPayload: PrecisionEditPayload.admitted(
                bytes: UInt64(region.count) * 2 + UInt64(command.request.utf8.count) * 2 + 512))
    }

    func startNative(_ result: PatchSession.Result, document: ImageDocument,
                     record: [ActionStep], showProgress: Bool) throws -> DocumentJob {
        let work = try nativeWork(result)
        try capture.requireCurrent(document)
        return try DocumentJobs.start(document: document, layer: layer, title: "Patch",
            scratch: work.heldBytes, record: record, showProgress: showProgress, compute: { _, control in
                try work.compute(control: control).map { try RasterDocument(precision: $0) }
            })
    }

    /// The 8-bit Patch as a nonmodal job: byte overlay/Poisson arithmetic
    /// unchanged, using the plate that this gesture showed rather than
    /// recapturing at mouse-up. The overlay is built on main (it draws the
    /// captured plate through CoreGraphics); only the solve — `healJob` on the
    /// captured handle — runs on the worker, which reserves its own scratch
    /// against the job's token.
    func startByte(_ result: PatchSession.Result, document: ImageDocument, record: [ActionStep],
                   showProgress: Bool) throws -> DocumentJob {
        let source = capture.source
        let layer = self.layer
        let footprint = try byteOverlay(result)
        try capture.requireCurrent(document)
        return try DocumentJobs.start(document: document, layer: layer, title: "Patch",
            scratch: UInt64(footprint.count), record: record, showProgress: showProgress,
            compute: { _, control in
                try source.healJob(layer, overlay: footprint, strength: 1, control: control)
            })
    }

    /// The 8-bit heal's overlay for `result`, drawn on main from the plate
    /// this gesture captured: canvas-sized premultiplied RGBA8, the region's
    /// coverage in alpha and the aligned source in RGB (`PatchSession.buildOverlay`).
    /// `startByte` and Content-Aware Move's heal (`ContentAwareMoveWork`) both
    /// take it. Throws `missesCanvas` for a displacement off the picture.
    func byteOverlay(_ result: PatchSession.Result) throws -> [UInt8] {
        let source = capture.source
        guard let image = bytePlate?.cgImage(in: source.drawingSpace) else {
            throw DoctoredCoreError(message: "Could not read the captured Patch source.")
        }
        var overlay = [UInt8](repeating: 0, count: source.width * source.height * 4)
        let built = overlay.withUnsafeMutableBufferPointer { buffer in
            guard let base = buffer.baseAddress else { return false }
            return PatchSession.buildOverlay(result: result, snapshot: image,
                destination: destination, space: source.drawingSpace,
                width: source.width, height: source.height, into: base)
        }
        guard built else {
            throw DoctoredCoreError(message: Self.missesCanvas)
        }
        return overlay
    }
}

/// The 16-bit Patch's captured edit: canvas-sized region coverage, placed on
/// the worker when a Destination drag displaced it, then the native heal.
struct PatchNativeWork {
    let source: RasterPrecisionDocument
    let command: PrecisionPixelCommand
    /// Canvas-sized, unshifted region coverage.
    let region: [UInt16]
    /// Non-nil only for a nonzero clip offset; shifts `region` on the worker.
    let placement: NativeSelectionWork?
    /// The edit plan's scratch plus the placement's.
    let scratchEstimate: UInt64
    let retainedPayload: PrecisionEditPayload

    /// The bytes the worker itself holds outside the core's reservation and
    /// `retainedPayload` (which already counts the region): for a displaced
    /// region, its placed copy. This, not `scratchEstimate`, is the job's
    /// `scratch`.
    var heldBytes: UInt64 {
        placement == nil ? 0 : UInt64(region.count) * 2
    }

    /// Worker. A job control is single-use at the core, so the placement and
    /// the edit each run under their own child of the job's control.
    func compute(control: RasterJobControl) throws -> RasterPrecisionDocument? {
        try healing(control: control, onto: nil)
    }

    /// Worker. `compute(control:)` with the heal written onto `filled` while
    /// its pixels are still read from `source`: Content-Aware Move
    /// heals the moved region onto the document its fill produced, and the
    /// moved pixels are the ones the fill replaced. `filled` must be an
    /// in-place edit of `source`, so the plan made on `source` holds for it.
    func compute(control: RasterJobControl,
                 onto filled: RasterPrecisionDocument) throws -> RasterPrecisionDocument? {
        try healing(control: control, onto: filled)
    }

    private func healing(control: RasterJobControl,
                         onto filled: RasterPrecisionDocument?) throws -> RasterPrecisionDocument? {
        let samples: [UInt16]
        if let placement {
            samples = try control.withChildOperation(from: 0, to: 0.2) { try placement.compute(control: $0) }
        } else {
            samples = region
        }
        if control.isCancelled { throw JobCancelled() }
        return try control.withChildOperation(from: placement == nil ? 0 : 0.2, to: 1) { child in
            guard let filled else {
                let context = try PrecisionWorkContext(source: source)
                return try source.editingPixels(request: command.request, samples: samples,
                    workingLimit: context.limit, otherRetained: context.otherRetained, control: child)
            }
            // The receiver's own bytes are the core's to count, so the context
            // is `filled`'s: `source`'s replaced layer then counts as retained
            // by someone else, which it is.
            let context = try PrecisionWorkContext(source: filled)
            return try filled.retouching(from: source, request: command.request, samples: samples,
                workingLimit: context.limit, otherRetained: context.otherRetained, control: child)
        }
    }
}
