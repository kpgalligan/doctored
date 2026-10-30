import AppKit

// Channel visibility now belongs to `ImageDocument` editing state; panel eye/target changes refresh
// the current history state, and edits/Undo/snapshots/recovery restore that exact view without
// writing image pixels.

extension RasterDocument {
    /// Explicit disposable canvas derivatives for Channels display only.
    func channelDisplayBytes(_ index: Int) -> [UInt8]? {
        guard isHighDepth else { return channelPlane(index) }
        return try? PrecisionScalarOperations.read(self, target: "channel", index: index).samples
            .map { UInt8((UInt32($0) + 128) / 257) }
    }

    func maskDisplayBytes(_ layer: Int) -> [UInt8]? {
        guard isHighDepth else { return layerPlane(layer, DR_PLANE_MASK) }
        return try? PrecisionScalarOperations.read(self, target: "mask", index: layer, canvas: true).samples
            .map { UInt8((UInt32($0) + 128) / 257) }
    }

    func nativePlaneDisplay(target: String, index: Int, maxSide: Int = 0, canvas: Bool = true) throws -> RasterImage {
        let plane = try PrecisionScalarOperations.read(self, target: target, index: index, canvas: canvas)
        guard let image = PrecisionScalarOperations.display(plane, maxSide: maxSide) else {
            throw DoctoredCoreError(message: "Could not build the plane display.")
        }
        return image
    }

    /// The 16-bit Channels panel's mask/alpha row thumbnail fitted in the
    /// core before any canvas-sized image: `target` "mask" is record
    /// `index`'s mask in its canvas view, "channel" alpha channel `index`;
    /// any other target, or any refusal, is nil. `maxSide` 0 is full size.
    func nativePlaneThumbnail(target: String, index: Int, maxSide: Int) -> RasterImage? {
        guard let precision, let context = try? PrecisionWorkContext(source: precision) else {
            return nil
        }
        switch target {
        case "mask":
            return try? precision.layerMaskCanvasThumbnailRGBA8(
                index, maxSide: maxSide, workingLimit: context.limit,
                otherRetained: context.otherRetained)
        case "channel":
            return try? precision.channelThumbnailRGBA8(
                index, maxSide: maxSide, workingLimit: context.limit,
                otherRetained: context.otherRetained)
        default:
            return nil
        }
    }

    /// One colour/alpha/luma plane of the composite or of record `layer`. At
    /// 32-bit float it is a plane of the tone-mapped display (the
    /// panel thumbnails' rule: luma by Rec.709 on display codes); at 16 bits
    /// the exact native plane read. On a Lab Color document it is `labPlaneDisplay`: the composite's
    /// Lightness, a or b codes only, and a layer refuses with `LabDocument.renderRefusal`.
    func nativeSourcePlaneDisplay(_ plane: RasterPlane, layer: Int?, maxSide: Int = 0) throws -> RasterImage {
        // #547 Lab phase 1c: a Lab Color document's Lightness, a or b of the composite's codes.
        if isLab {
            guard layer == nil else { throw DoctoredCoreError(message: LabDocument.renderRefusal) }
            return try labPlaneDisplay(plane, maxSide: maxSide)
        }
        if plane == .mask { throw DoctoredCoreError(message: "A mask requires an explicit source layer.") }
        if isFloat {
            guard let image = try nativeDisplay(layer: layer).planeImage(plane.dr, maxSide: maxSide) else {
                throw DoctoredCoreError(message: "Could not extract the \(plane.displayName) plane.")
            }
            return image
        }
        let source = try PrecisionScalarOperations.sourcePlane(self, plane: plane,
            scope: layer.map(HistogramSource.layer) ?? .composite)
        guard let image = PrecisionScalarOperations.display(source, maxSide: maxSide) else {
            throw DoctoredCoreError(message: "Could not build the plane display.")
        }
        return image
    }
}

extension AgentServer {
    static func nativePlaneRenderImage(_ doc: RasterDocument, channel: String,
                                       layer: Int?) throws -> (image: RasterImage?, what: String) {
        if channel == "mask" || channel == "effective_mask" {
            guard let layer, doc.layerHasMask(layer) else { throw ToolError(message: "Rendering a mask requires a layer with a mask.") }
            return (try doc.nativePlaneDisplay(target: channel, index: layer),
                "8-bit display of native \(channel) of layer \(layer)")
        }
        if let plane = RasterPlane.named(channel), plane != .mask {
            let what = doc.isFloat ? "8-bit tone-mapped display of" : "8-bit display of native"
            return (try doc.nativeSourcePlaneDisplay(plane, layer: layer),
                "\(what) \(channel) \(layer.map { "of layer \($0)" } ?? "composite")")
        }
        guard let index = doc.channelIndex(named: channel) else { throw ToolError(message: "No native channel named \(channel).") }
        return (try doc.nativePlaneDisplay(target: "channel", index: index),
            "8-bit display of native channel \(channel)")
    }
}
