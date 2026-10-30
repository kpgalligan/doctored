import AppKit

/// Original mask coverage and placement are independent of its rendering
/// controls. Editing raw coverage never bakes density or feather into it.
struct LayerMaskProperties: Equatable {
    var linked: Bool
    var density: Double
    var feather: Double
    var outsideCoverage: UInt8 = 0
    let offsetX: Int
    let offsetY: Int
    let width: Int
    let height: Int

    var bounds: CGRect { CGRect(x: offsetX, y: offsetY, width: width, height: height) }

    var report: [String: Any] {
        ["linked": linked, "density": density, "feather": feather,
         "outside_coverage": Int(outsideCoverage),
         "offset_x": offsetX, "offset_y": offsetY, "width": width, "height": height]
    }
}

struct MaskEditCapture {
    let source: RasterDocument
    let revision: UInt64
    let layer: Int
    let fingerprint: String?
    /// The properties the edit starts from, and the mask's enabled flag: the
    /// layer mask's own, or — beside a vector mask — the raster mask's
    /// (`RasterDocument.maskEditBaseline`).
    let properties: LayerMaskProperties
    let enabled: Bool

    init(_ document: ImageDocument, layer: Int? = nil) throws {
        let index = layer ?? document.activeLayerIndex
        guard !document.isLiveEditing, let source = document.doc,
              let (properties, enabled) = source.maskEditBaseline(index) else {
            throw DoctoredCoreError(message: "Choose a layer mask after finishing the current gesture.")
        }
        if let refusal = source.lockRefusal(index, kind: DR_EDIT_MASK) {
            throw DoctoredCoreError(message: refusal.message)
        }
        self.source = source
        revision = document.revision
        self.layer = index
        fingerprint = source.layerFingerprint(index)
        self.properties = properties
        self.enabled = enabled
    }

    func validate(in document: ImageDocument) throws {
        guard !document.isLiveEditing, document.doc === source, document.revision == revision,
              document.doc?.layerFingerprint(layer) == fingerprint else {
            throw DoctoredCoreError(message: "The document changed after this mask was captured. Reopen the command and try again.")
        }
    }
}

enum MaskEdit {
    case properties(linked: Bool, density: Double, feather: Double)
    case transform(CGAffineTransform, DrResizeFilter)

    func apply(to capture: MaskEditCapture) throws -> RasterDocument? {
        switch self {
        case let .properties(linked, density, feather):
            return try capture.source.settingMaskProperties(capture.layer, linked: linked, density: density, feather: feather)
        case let .transform(matrix, filter):
            return try capture.source.transformingMask(capture.layer, matrix: matrix, filter: filter)
        }
    }

    func isIdentity(for capture: MaskEditCapture) -> Bool {
        switch self {
        case let .properties(linked, density, feather):
            return linked == capture.properties.linked && Float(density) == Float(capture.properties.density)
                && Float(feather) == Float(capture.properties.feather)
        case .transform(let matrix, _): return matrix.isIdentity
        }
    }

    func scratchEstimate(for capture: MaskEditCapture) throws -> UInt64 {
        if let source = capture.source.precision {
            let command: PrecisionPixelCommand
            switch self {
            case .properties(let linked, let density, let feather):
                command = try .maskProperties(layer: capture.layer, x: capture.properties.offsetX,
                    y: capture.properties.offsetY, linked: linked, density: density, feather: feather,
                    enabled: capture.enabled)
            case .transform(let matrix, let filter):
                command = try .affine(layer: capture.layer, target: .mask, matrix: matrix,
                                      filter: RasterDocument.precisionFilter(filter))
            }
            let context = try PrecisionWorkContext(source: source)
            let plan = try source.pixelEditPlan(request: command.request, workingLimit: context.limit,
                                                otherRetained: context.otherRetained)
            return try PrecisionDocumentDescriptor.integer(plan, "scratch_bytes") + capture.source.projectionScratchEstimate()
        }
        switch self {
        case .properties:
            // This only clones native metadata. Ask the core about the NEW
            // feather before any derived plane or display projection exists.
            guard let candidate = try apply(to: capture) ?? (isIdentity(for: capture) ? capture.source : nil) else {
                throw DoctoredCoreError(message: "The mask properties exceed the supported range or padded extent.")
            }
            return try candidate.projectionScratchEstimate() + 16 * 1024 * 1024
        case .transform(let matrix, _):
            let extent = capture.properties.bounds.applying(matrix).integral
            guard !extent.isInfinite, !extent.isNull, extent.width.isFinite, extent.height.isFinite,
                  extent.width >= 1, extent.height >= 1,
                  extent.width * extent.height <= LayerTransform.maxTransformPixels else {
                throw DoctoredCoreError(message: "The transformed mask exceeds the supported extent.")
            }
            let pixels = UInt64(extent.width * extent.height)
            var extra = pixels * 2
            if capture.properties.feather > 0 {
                // A conservative support bound, not a second blur algorithm:
                // core's multiresolution Gaussian reaches at most radius+64
                // pixels, including its two triangle resampling passes.
                let pad = ceil(capture.properties.feather) + 64
                let padded = (Double(extent.width) + 2 * pad) * (Double(extent.height) + 2 * pad)
                extra += UInt64(padded) * 12
            }
            let canvasArea = capture.source.width * capture.source.height
            let info = capture.source.layerInfo(capture.layer)
            // Group info describes the union of distant children, not an
            // allocated image. Its area can exceed Int even on a tiny canvas.
            let alignedArea = capture.source.layerIsGroup(capture.layer) ? canvasArea
                : (info?.width ?? 0) * (info?.height ?? 0)
            let aligned = UInt64(max(canvasArea, alignedArea))
            return try capture.source.projectionScratchEstimate() + extra + aligned + 16 * 1024 * 1024
        }
    }
}

extension ImageDocument {
    /// Mask properties and geometry share the bounded worker and central edit
    /// path. A stale result never overwrites newer pixels or mask coverage.
    @discardableResult
    func applyMaskEdit(_ edit: MaskEdit, capture: MaskEditCapture,
                       name: String, record: [ActionStep]) throws -> Bool {
        try capture.validate(in: self)
        if edit.isIdentity(for: capture) { return false }
        let estimate = try edit.scratchEstimate(for: capture)
        _ = try RasterResources.shared.admit(scratch: estimate)
        let scratch = BackgroundWork.scratch
        let task = try WorkerTask(scratchEstimate: estimate) { () -> PreparedDocumentEdit in
            guard let updated = try edit.apply(to: capture) else {
                throw DoctoredCoreError(message: "The mask edit could not be applied. Check its parameters and extent.")
            }
            return PreparedDocumentEdit(updated, scratch: scratch)
        }
        let result = try BackgroundWork.wait(task, title: name)
        try capture.validate(in: self)
        preparedEdit = result
        try preflightPublication(result.document, record: record)
        withUndoGroup(undoManager) { applyEdit(name, record: record) { _ in result.document } }
        return true
    }
}
