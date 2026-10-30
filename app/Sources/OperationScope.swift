import AppKit

/// Shared UI/MCP policy. Scalar targets run the same image operators on opaque
/// grayscale and retain their Rec. 709 luma. Neighborhood and automatic tonal
/// analysis read the complete source; only writes are gated by the selection.
enum OperationScope: String, CaseIterable {
    case filter, fill, gradient
    case pixelOnly, layerGeometry

    static func refusalReason(
        _ operation: OperationScope, target: PaintTarget,
        doc: RasterDocument, layerIndex: Int
    ) -> String? {
        refusal(operation, target: target, doc: doc, layerIndex: layerIndex)?.message
    }

    /// `refusalReason`'s sentence plus the entry whose own lock refused —
    /// `lockOwner` is set only when a layer lock did, nil for every
    /// other refusal. nil when the operation is allowed.
    static func refusal(
        _ operation: OperationScope, target: PaintTarget,
        doc: RasterDocument, layerIndex: Int
    ) -> (message: String, lockOwner: Int?)? {
        // #547: one ink plate of a CMYK layer is gated as a colour plane is; its selected pixels
        // move through `MoveSelectedPixels.inkRefusal`, never this scope.
        if operation != .layerGeometry, doc.hasRetainedSource(layerIndex) {
            switch target {
            case .layer, .plane, .ink: return (RetainedImagePolicy.rasterizeFirst, nil)
            case .mask, .channel: break
            }
        }
        if let reason = DocumentPrecision.refusal(operation: operation, target: target, doc: doc, layerIndex: layerIndex) { return (reason, nil) }
        let adjustmentMaskGeometry = operation == .layerGeometry && target == .mask
            && doc.layerIsAdjustment(layerIndex) && doc.layerHasMask(layerIndex)
        if operation == .pixelOnly || operation == .layerGeometry,
           target != .layer && !adjustmentMaskGeometry {
            return ("This operation supports layer pixels only; select the layer's pixels first.", nil)
        }
        guard target.isPaintable else { return ("This plane is read-only.", nil) }
        if case .channel(let index) = target {
            return (0..<doc.channelCount).contains(index)
                ? nil : ("The saved channel no longer exists.", nil)
        }
        guard let info = doc.layerInfo(layerIndex) else {
            return ("The target layer no longer exists.", nil)
        }
        if operation == .layerGeometry {
            return doc.lockRefusal(layerIndex, kind: DR_EDIT_POSITION)
                .map { ($0.message, $0.owner) }
        }
        let kind: DrEditKind
        switch target {
        case .mask:
            guard doc.layerHasMask(layerIndex) else { return ("The target layer has no mask.", nil) }
            kind = DR_EDIT_MASK
        case .layer, .plane, .ink:
            guard !info.isGroup else {
                return ("A group has no pixels to edit; target its mask instead.", nil)
            }
            guard !doc.layerIsAdjustment(layerIndex) else {
                return ("An adjustment layer has no pixels to edit; target its mask instead.", nil)
            }
            kind = DR_EDIT_PIXELS
        case .channel:
            return nil
        }
        if let refusal = doc.lockRefusal(layerIndex, kind: kind) {
            return (refusal.message, refusal.owner)
        }
        if target == .plane(.alpha), let owner = doc.transparencyLockOwner(layerIndex) {
            return (LockFlags.refusal(layerName: info.name, blocking: .transparency,
                                      group: owner == layerIndex ? nil
                                          : doc.layerInfo(owner)?.name ?? "this group"),
                    owner)
        }
        return nil
    }

    static let quickMaskRefusal = "Exit Quick Mask before editing layer pixels, masks or channels. "
        + "Use the brush or eraser to edit Quick Mask coverage."

    static var supportTable: [String: [String]] {
        // A CMYK document's four ink plates (#547) take what an RGB colour plane takes.
        ["filter": ["layer", "mask", "red", "green", "blue", "alpha", "channel",
                    "cyan", "magenta", "yellow", "black"],
         "fill": ["layer", "mask", "red", "green", "blue", "alpha", "channel",
                  "cyan", "magenta", "yellow", "black"],
         "gradient": ["layer", "mask", "red", "green", "blue", "alpha", "channel",
                      "cyan", "magenta", "yellow", "black"],
         "pixel_only": ["layer"], "layer_geometry": ["layer", "adjustment_mask"]]
    }
}

/// Immutable edit inputs shared by a sheet's preview and commit, and by MCP.
/// A document handle is a revision: commit refuses a different handle even when
/// the old layer index is still in range after deletion or reordering.
struct OperationCapture {
    let doc: RasterDocument
    let layerIndex: Int
    let target: PaintTarget
    let selection: [UInt8]?
    let source: RasterImage
    let placement: PlanePlacement?
    let fingerprint: String?
    let previewBase: ChannelBase

    init?(
        doc: RasterDocument, layerIndex: Int, target: PaintTarget, selection: [UInt8]?,
        previewBase: ChannelBase? = nil
    ) {
        // Native scalar display helpers deliberately return byte images. They
        // must never make this legacy editing capture accept a typed source.
        guard !doc.isHighDepth,
              OperationScope.refusalReason(.filter, target: target, doc: doc,
                                           layerIndex: layerIndex) == nil,
              selection == nil || selection?.count == doc.width * doc.height
        else { return nil }
        let source: RasterImage
        let placement: PlanePlacement?
        switch target {
        case .layer:
            guard let image = doc.layerImage(layerIndex) else { return nil }
            source = image
            placement = doc.layerPlacement(layerIndex)
        case .mask:
            guard let mask = doc.layerMaskStorage(layerIndex) else { return nil }
            source = mask.image
            placement = PlanePlacement(
                x: mask.offsetX, y: mask.offsetY,
                width: mask.image.width, height: mask.image.height,
                canvasWidth: doc.width, canvasHeight: doc.height)
        case .plane(let plane):
            guard let read = doc.layerSpacePlaneImage(layerIndex, plane.dr) else { return nil }
            source = read.image
            placement = read.placement
        case .channel(let index):
            guard let image = doc.channelImage(index, maxSide: 0) else { return nil }
            source = image
            placement = nil
        case .ink:
            // #547: a CMYK document is typed; its plates edit through the scalar path, never
            // this byte capture.
            return nil
        }
        self.doc = doc
        self.layerIndex = layerIndex
        self.target = target
        self.selection = selection
        self.source = source
        self.placement = placement
        self.fingerprint = doc.layerFingerprint(layerIndex)
        if let previewBase = previewBase {
            self.previewBase = previewBase
        } else {
            switch target {
            case .layer, .mask: self.previewBase = .composite
            case .plane(let plane): self.previewBase = .plane(plane)
            case .channel(let index): self.previewBase = .channel(index)
            case .ink: self.previewBase = .composite
            }
        }
    }

    func isCurrent(in document: ImageDocument) -> Bool {
        guard let current = document.doc else { return false }
        return !document.quickMaskActive && current === doc
            && current.layerFingerprint(layerIndex) == fingerprint
            && document.activeLayerIndex == layerIndex && document.planeEditTarget == target
            && document.canvasSelection?.maskBytes() == selection
    }

    var rewritesLayerPixels: Bool { target == .layer || target.isPlane }
    var agentTarget: String { target.agentName(in: doc) }

    func writing(_ result: RasterImage, selection: [UInt8]?) -> RasterDocument? {
        switch target {
        case .layer: return doc.applyingLayerPixels(layerIndex, result, selection: selection)
        case .mask: return doc.applyingLayerMask(layerIndex, result, selection: selection)
        case .plane(let plane):
            return doc.applyingLayerPlane(layerIndex, plane.dr, result, selection: selection)
        case .channel(let index): return doc.applyingChannel(index, result, selection: selection)
        case .ink: return nil
        }
    }

    func applying(_ op: (RasterImage) throws -> RasterImage?) rethrows -> RasterDocument? {
        guard let result = try op(source) else { return nil }
        return writing(result, selection: selection)
    }

    /// Scratch fill/gradient already blend through coverage, so the result
    /// writer receives nil to avoid multiplying feathered coverage twice.
    func applyingScratch(_ op: PlaneScratchOp) -> RasterDocument? {
        applyingScratch(op, mask: selection)
    }

    /// `applyingScratch(_:)` through `mask` (canvas-sized) instead of the
    /// captured selection — Edit ▸ Stroke…'s band. The capture keeps the
    /// real selection, so `isCurrent` still guards staleness.
    func applyingScratch(_ op: PlaneScratchOp, mask: [UInt8]?) -> RasterDocument? {
        guard target.isCoverage, let scratch = RasterDocument.from(image: source),
              let edited = op.run(scratch, in: placement, mask: mask),
              let result = edited.layerImage(0)
        else { return nil }
        return writing(result, selection: nil)
    }

    func preview(_ op: (RasterImage) -> RasterImage?) -> CGImage? {
        // An identity is a valid preview even though it deliberately creates
        // no document/undo entry when Apply is pressed.
        display(applying(op) ?? doc)
    }

    /// `preview(_:)`, but nil — no flatten — once `control` is cancelled after
    /// `op`: a superseded preview stops before its display work.
    func preview(control: RasterJobControl?, _ op: (RasterImage) -> RasterImage?) -> CGImage? {
        let result = op(source)
        if control?.isCancelled == true { return nil }
        return display(result.flatMap { writing($0, selection: selection) } ?? doc)
    }

    private func display(_ result: RasterDocument) -> CGImage? {
        switch previewBase {
        case .composite:
            return result.flattened()?.makeCGImage(in: result.colorSpace)
        case .plane(let plane):
            return result.compositePlaneImage(plane.dr, maxSide: 0)?.makeCGImage(in: ColorProfile.sRGB)
        case .channel(let index):
            return result.channelImage(index, maxSide: 0)?.makeCGImage(in: ColorProfile.sRGB)
        case .layerMask:
            guard let bytes = result.layerPlane(layerIndex, DR_PLANE_MASK) else { return nil }
            return CanvasSelection.grayImage(bytes, result.width, result.height)
        // #547: a CMYK ink plane is view only; no byte operation previews over it.
        case .nothing, .ink:
            return nil
        }
    }
}

extension ImageDocument {
    func captureOperation(previewBase: ChannelBase? = nil) -> OperationCapture? {
        guard !quickMaskActive, let doc = doc else { return nil }
        return OperationCapture(
            doc: doc, layerIndex: activeLayerIndex, target: planeEditTarget,
            selection: canvasSelection?.maskBytes(), previewBase: previewBase)
    }

    func applyCapturedOperation(
        _ capture: OperationCapture?, _ actionName: String, record: [ActionStep],
        _ op: (RasterImage) throws -> RasterImage?
    ) {
        guard let capture = capture else { NSSound.beep(); return }
        applyCapturedRewrite(capture, actionName, record: record) { try capture.applying(op) }
    }

    func applyCapturedRewrite(
        _ capture: OperationCapture, _ actionName: String, record: [ActionStep],
        _ rewrite: () throws -> RasterDocument?
    ) {
        guard capture.isCurrent(in: self)
        else {
            let alert = NSAlert()
            alert.messageText = "The document or edit target changed while this edit was open."
            alert.informativeText = "Reopen the edit to use the current layer, target and selection."
            if let window = windowControllers.first?.window { alert.beginSheetModal(for: window) }
            else { alert.runModal() }
            return
        }
        if capture.rewritesLayerPixels {
            applyRasterizingEdit(actionName, layer: capture.layerIndex, record: record) { current in
                guard current === capture.doc else { return nil }
                return try rewrite()
            }
        } else {
            applyEdit(actionName, record: record) { current in
                guard current === capture.doc else { return nil }
                return try rewrite()
            }
        }
    }
}
