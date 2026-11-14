import AppKit

/// Filters ▸ Other ▸ Frequency Separation… and `frequency_separation`: one raster layer split
/// into a LOW layer (its Gaussian Blur, in place, keeping the layer's mask, opacity, blend mode
/// and style) and a HIGH layer above it (the detail, Linear Light, clipped to the low layer),
/// which together recompose the original within one code at 8 and 16 bits, as one undo step.
///
/// The detail is Apply Image's Subtract of the STORED low layer at Scale 2 and Offset 127.5
/// levels, `(source − low) / 2 + ½`, so the pair rebuilds the original whatever the blur's own
/// rounding was. With `d = source − low` in codes the detail stores `(d + max) / 2`; `max` is odd
/// at both depths, so an odd `d` is exact and an even one half a code off, which Linear Light
/// (`low + 2·high − max`) doubles to at most one code. A pivot of 128 levels would add half a
/// code to every sample and reach two (`core/tests/frequency_separation.rs` measures both
/// through the compositors). Clipping the high layer to the low one makes it blend against the
/// low layer alone, not the layers beneath, and lets the low layer's mask, opacity and blend
/// mode apply to the pair.
///
/// The split ignores an active selection: a selection-confined blur would leave the detail
/// holding the original pixels outside it.
///
/// A document evaluated in linear light refuses: the detail is a difference of stored codes,
/// but that policy runs the Linear Light blend on DECODED components, so the pair would
/// composite `encode(lin(low) + 2·lin(high) − 1)`, not the original (an sRGB mid-grey over a
/// darker low clamps to black).
struct FrequencySeparationParameters: Equatable {
    /// The layer split; the low layer keeps this index and the high layer lands above it.
    var layer: Int
    /// The Gaussian Blur radius (sigma) of the low layer, in pixels.
    var radius: Double = 4

    static let radiusRange = 0.5...64.0
}

enum FrequencySeparation {
    static let actionName = "Frequency Separation"
    /// The detail's pivot in 8-bit levels: ½, 127.5 codes at 8 bits (the type comment says why).
    static let detailOffset = 127.5

    /// Why `layer` of `doc` cannot be split, or nil. Shared by the menu's validation, its commit
    /// and the MCP handler.
    static func refusal(_ doc: RasterDocument, layer: Int, quickMask: Bool) -> String? {
        guard let info = doc.layerInfo(layer) else { return "Layer \(layer) does not exist." }
        if quickMask { return OperationScope.quickMaskRefusal }
        if doc.isFloat {
            return "Frequency separation answers at 8 and 16 bits. Convert the 32-bit document first."
        }
        guard doc.colorMode == .rgb else {
            return "Frequency separation answers in RGB documents. Convert the document to RGB first."
        }
        if doc.evaluation == .linearLight {
            return "Frequency separation recomposes on stored codes, and this document composites "
                + "in linear light, where the pair would not rebuild the layer. Set Image ▸ "
                + "Evaluation… to Encoded first."
        }
        if info.isGroup || doc.layerIsAdjustment(layer) {
            return "Frequency separation splits a pixel layer; \"\(info.name)\" has no pixels of its own."
        }
        if doc.layerClipped(layer) {
            return "\"\(info.name)\" is clipped to the layer below. Release its clipping mask first: "
                + "the detail layer is clipped to the low layer, and a clip has one base."
        }
        if doc.hasRetainedSource(layer) || doc.hasArtworkDescription(layer)
            || LayerDescription.claimedKind(of: doc.layerMeta(layer)) != nil {
            return "\"\(info.name)\" is a text, shape, fill or placed layer. Rasterize it first."
        }
        if let lock = doc.lockRefusal(layer, kind: DR_EDIT_PIXELS) { return lock.message }
        return nil
    }

    /// The detail layer's Apply Image: the low layer subtracted from the copy above it.
    static func detail(of p: FrequencySeparationParameters) -> ApplyImageParameters {
        var detail = ApplyImageParameters()
        detail.source = p.layer
        detail.sourcePlane = .rgb
        detail.blend = DR_BLEND_SUBTRACT
        detail.opacity = 1
        detail.target = .layer
        detail.targetLayer = p.layer + 1
        detail.scale = 2
        detail.offset = detailOffset
        return detail
    }

    /// The low layer's Gaussian Blur at 8 bits, admitted and run as a cancellable job on the
    /// layer's own pixels (`ByteFilterAdmission.run`, as Filters ▸ Blur ▸ Gaussian Blur… runs it).
    static func blurredLow(_ doc: RasterDocument, _ p: FrequencySeparationParameters,
                           waitForSlot: Bool) throws -> RasterImage {
        guard let image = doc.layerImage(p.layer) else {
            throw DoctoredCoreError(message: "Layer \(p.layer) has no pixels to split.")
        }
        let request = ByteFilterRequest(op: "blur", params: ["sigma": p.radius])
        return try ByteFilterAdmission.run(request, on: image, in: doc, title: actionName,
                                           waitForSlot: waitForSlot) ?? image
    }

    /// The 8-bit split of `doc` with the low layer's blurred pixels `low` (`blurredLow`).
    static func split(_ doc: RasterDocument, _ p: FrequencySeparationParameters,
                      low: RasterImage) throws -> RasterDocument {
        guard let copied = try doc.duplicatingLayer(p.layer) else {
            throw DoctoredCoreError(message: "Could not duplicate layer \(p.layer).")
        }
        let blurred = copied.withLayerPixels(p.layer, low) ?? copied
        // nil is a detail that changed no byte (a flat mid-grey layer): the copy already holds it.
        let detailed = ChannelMath.applyImage(blurred, detail(of: p), merged: nil) ?? blurred
        return try labelled(detailed, p, name: doc.layerInfo(p.layer)?.name ?? "Layer")
    }

    /// The 16-bit split: the duplicate on main, then the blur and the detail as one job, then the
    /// labels. Returns the typed result for `capture.publish`.
    static func nativeSplit(_ capture: PrecisionEditorCapture,
                            _ p: FrequencySeparationParameters) throws -> RasterPrecisionDocument {
        guard let copied = try capture.facade.duplicatingLayer(p.layer), let source = copied.precision else {
            throw DoctoredCoreError(message: "Could not duplicate layer \(p.layer).")
        }
        let request = try SelectedAdjustment.request(layer: p.layer, filter: "blur", params: ["sigma": p.radius])
        let budget = try PrecisionWorkContext(source: source)
        let blurPlan = try source.selectedEditPlan(request: request, workingLimit: budget.limit,
                                                   otherRetained: budget.otherRetained)
        let scratch = max(try PrecisionDocumentDescriptor.integer(blurPlan, "scratch_bytes"),
                          try NativeChannelMath.scratch(copied, apply: detail(of: p)))
        let detailed = try PrecisionCapturedWork.run(scratchEstimate: scratch, title: actionName) { control in
            let blurred = try control.withChildOperation(from: 0, to: 0.5) { child -> RasterPrecisionDocument in
                let context = try PrecisionWorkContext(source: source)
                return try source.selectedEdit(request: request, coverage: [], workingLimit: context.limit,
                                               otherRetained: context.otherRetained, control: child) ?? source
            }
            let facade = try RasterDocument(precision: blurred)
            return try NativeChannelMath.applyRGB(facade, source: blurred, selection: nil,
                                                  parameters: detail(of: p), control: control) ?? blurred
        }
        guard let labelled = try labelled(RasterDocument(precision: detailed), p,
                                          name: capture.facade.layerInfo(p.layer)?.name ?? "Layer").precision else {
            throw DoctoredCoreError(message: "The 16-bit split lost its typed document.")
        }
        return labelled
    }

    /// Names the pair and makes the high layer the detail: no mask, vector mask or style (Blend If
    /// included), full opacity, Linear Light, clipped to the low layer. Each step that would change
    /// nothing answers nil and is skipped.
    static func labelled(_ doc: RasterDocument, _ p: FrequencySeparationParameters,
                         name: String) throws -> RasterDocument {
        let high = p.layer + 1
        var out = doc
        func step(_ edit: (RasterDocument) throws -> RasterDocument?) rethrows {
            if let next = try edit(out) { out = next }
        }
        try step { try $0.withLayerName(p.layer, name + " Low") }
        try step { try $0.withLayerName(high, name + " High") }
        try step { try $0.removingVectorMask(high) }
        if out.layerHasMask(high) { try step { try $0.removingLayerMask(high, apply: false) } }
        if out.layerHasStyle(high) { try step { try $0.withLayerStyle(high, nil) } }
        if out.layerInfo(high)?.opacity != 1 { try step { try $0.withLayerOpacity(high, 1) } }
        try step { try $0.withLayerBlendMode(high, DR_BLEND_LINEAR_LIGHT) }
        try step { try $0.withLayerClipped(high, clipped: true) }
        return out
    }
}

extension Array where Element == ActionStep {
    /// Filters ▸ Other ▸ Frequency Separation…
    static func frequencySeparation(_ p: FrequencySeparationParameters, in doc: RasterDocument) -> [ActionStep] {
        step("frequency_separation",
             ["layer": ActionArgs.layer(doc.layerRef(p.layer)), "radius": ActionArgs.number(p.radius)],
             note: "Filters ▸ Other ▸ Frequency Separation…")
    }
}
