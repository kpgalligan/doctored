import AppKit

/// Filters ▸ Other ▸ Frequency Separation… and `frequency_separation`: one raster layer split
/// into a LOW layer (its Gaussian Blur, in place, keeping the layer's mask, opacity, blend mode
/// and style) and a HIGH layer above it (the detail, clipped to the low layer), which together
/// recompose the original, as one undo step. It answers RGB documents at 8, 16 and 32 bits,
/// Grayscale (Duotone included) at every depth and Lab Color at 8 and 16; the detail takes one of
/// three forms (`Form`), each with its own bound:
///
/// - **Codes** (8 and 16 bits, encoded): Apply Image's Subtract of the STORED low layer at Scale
///   2 and the Offset of Linear Light's own pivot, `(source − low) / 2 + p`, Linear Light, so the
///   pair rebuilds the original whatever the blur's own rounding was. With `d = source − low` in
///   codes the detail stores `d / 2 + p` and Linear Light gives `low + 2·high − 2p`. An 8-bit RGB
///   document blends Linear Light on codes, `low + 2·high − 256`, so `p` is 128 levels there: an
///   even `d` is exact and an odd one half a code off, which Linear Light doubles to at most one
///   code. Every typed split keeps the float form, `low + 2·high − max` with `max` odd, so `p` is
///   ½ (127.5 levels): an odd `d` is exact and an even one half a code off. The other pivot would
///   add half a code to every sample and reach two (`core/tests/frequency_separation.rs` measures
///   both through the compositors). Grayscale runs it on its one plane
///   (`NativeChannelMath.applyGray`, R = G = B), Lab on the stored L*, a* and b* codes as the red,
///   green and blue planes; Lab's Linear Light runs on every code, a* and b* included, so the
///   pivot is ½ of the code range there too and the bound is one code per channel
///   (`frequency_separation_modes.rs`).
/// - **Difference** (32-bit float, RGB or Grayscale): the core's `layer_detail`, `source − low`
///   unclamped (a float layer keeps a negative sample), recombined by Linear Dodge (Add); the pair
///   rebuilds the layer within 2^-22 of the larger magnitude. The High layer alone shows its
///   negative half as black.
/// - **Decoded** (16 bits evaluated in linear light): that policy runs Linear Light on DECODED
///   components, so the stored-code detail would composite `encode(lin(low) + 2·lin(high) − 1)`,
///   not the original. The core's `layer_detail` computes the same scaled Subtract on decoded
///   components and encodes it once: the pair rebuilds the layer within 4 codes of 65535 in
///   linear light, which in stored codes is up to about 24 in sRGB's shadows and about two 8-bit
///   levels at black under a pure power curve. At 8 bits the same rounding is up to about twenty
///   LEVELS in the shadows, so an 8-bit document in linear light refuses.
///
/// Clipping the high layer to the low one makes it blend against the low layer alone, not the
/// layers beneath, and lets the low layer's mask, opacity and blend mode apply to the pair. The
/// split ignores an active selection: a selection-confined blur would leave the detail holding the
/// original pixels outside it.
struct FrequencySeparationParameters: Equatable {
    /// The layer split; the low layer keeps this index and the high layer lands above it.
    var layer: Int
    /// The Gaussian Blur radius (sigma) of the low layer, in pixels.
    var radius: Double = 4

    static let radiusRange = 0.5...64.0
}

enum FrequencySeparation {
    static let actionName = "Frequency Separation"

    /// The detail's pivot in 8-bit levels (the type comment says why): 128 for the 8-bit RGB byte
    /// split, whose Linear Light blends on codes; ½, 127.5 levels, for every typed split.
    static func detailOffset(byteRGB: Bool) -> Double { byteRGB ? 128 : 127.5 }

    /// How the high layer is made and recombined (the type comment).
    enum Form: Equatable {
        /// Apply Image's Subtract of the stored low at Scale 2 and Linear Light's pivot; Linear Light.
        case codes
        /// The core's `layer_detail` difference in a 32-bit float document; Linear Dodge (Add).
        case difference
        /// The core's `layer_detail` on decoded components under linear light; Linear Light.
        case decoded

        /// The high layer's blend mode.
        var highBlendMode: DrBlendMode { self == .difference ? DR_BLEND_ADDITION : DR_BLEND_LINEAR_LIGHT }
    }

    /// `doc`'s form; asked only once `refusal` has admitted it.
    static func form(of doc: RasterDocument) -> Form {
        if doc.isFloat { return .difference }
        return doc.isHighDepth && doc.evaluation == .linearLight ? .decoded : .codes
    }

    /// The modes the split answers in (the type comment).
    static let colorModes: [ColorModeTarget] = [.rgb, .gray, .duotone, .lab]

    /// Why an 8-bit document evaluated in linear light refuses (the type comment, Decoded).
    static let eightBitLinearRefusal = "Frequency separation in an 8-bit document evaluated in "
        + "linear light would rebuild the layer only within about twenty levels in the shadows: "
        + "the detail is stored as an 8-bit code, and linear light magnifies its rounding there. "
        + "Convert to 16 Bits/Channel, or set Image ▸ Evaluation… to Encoded, first."

    /// Why `layer` of `doc` cannot be split, or nil. Shared by the menu's validation, its commit
    /// and the MCP handler.
    static func refusal(_ doc: RasterDocument, layer: Int, quickMask: Bool) -> String? {
        guard let info = doc.layerInfo(layer) else { return "Layer \(layer) does not exist." }
        if quickMask { return OperationScope.quickMaskRefusal }
        guard colorModes.contains(doc.colorMode) else {
            return "Frequency separation answers in RGB, Grayscale, Duotone and Lab Color "
                + "documents. Convert the document to one of them first."
        }
        if !doc.isHighDepth, doc.evaluation == .linearLight { return eightBitLinearRefusal }
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

    /// The detail layer's Apply Image: the low layer subtracted from the copy above it, at the
    /// pivot of `detailOffset(byteRGB:)`.
    static func detail(of p: FrequencySeparationParameters, byteRGB: Bool) -> ApplyImageParameters {
        var detail = ApplyImageParameters()
        detail.source = p.layer
        detail.sourcePlane = .rgb
        detail.blend = DR_BLEND_SUBTRACT
        detail.opacity = 1
        detail.target = .layer
        detail.targetLayer = p.layer + 1
        detail.scale = 2
        detail.offset = detailOffset(byteRGB: byteRGB)
        return detail
    }

    /// The core's `layer_detail` (the difference and decoded forms): the copy above, which holds
    /// the original pixels, against the low layer.
    static func layerDetail(_ p: FrequencySeparationParameters) throws -> PrecisionPixelCommand {
        try PrecisionPixelCommand(["op": "layer_detail", "version": 1, "layer": p.layer + 1,
                                   "source": p.layer])
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
        let detailed = ChannelMath.applyImage(blurred, detail(of: p, byteRGB: true), merged: nil)
            ?? blurred
        return try labelled(detailed, p, name: doc.layerInfo(p.layer)?.name ?? "Layer", form: .codes)
    }

    /// The typed split (16 and 32 bits, and every colour mode at 8): the duplicate on main, then
    /// the blur and the detail of `form(of:)` as one job, then the labels. Returns the typed
    /// result for `capture.publish`.
    static func nativeSplit(_ capture: PrecisionEditorCapture,
                            _ p: FrequencySeparationParameters) throws -> RasterPrecisionDocument {
        let form = Self.form(of: capture.facade)
        guard let copied = try capture.facade.duplicatingLayer(p.layer), let source = copied.precision else {
            throw DoctoredCoreError(message: "Could not duplicate layer \(p.layer).")
        }
        let request = try SelectedAdjustment.request(layer: p.layer, filter: "blur", params: ["sigma": p.radius])
        let budget = try PrecisionWorkContext(source: source)
        let blurPlan = try source.selectedEditPlan(request: request, workingLimit: budget.limit,
                                                   otherRetained: budget.otherRetained)
        let detailScratch: UInt64
        if form == .codes {
            detailScratch = try NativeChannelMath.scratch(copied, apply: detail(of: p, byteRGB: false))
        } else {
            let plan = try source.pixelEditPlan(request: layerDetail(p).request, workingLimit: budget.limit,
                                                otherRetained: budget.otherRetained)
            detailScratch = try PrecisionDocumentDescriptor.integer(plan, "scratch_bytes")
        }
        let scratch = max(try PrecisionDocumentDescriptor.integer(blurPlan, "scratch_bytes"), detailScratch)
        let detailed = try PrecisionCapturedWork.run(scratchEstimate: scratch, title: actionName) { control in
            let blurred = try control.withChildOperation(from: 0, to: 0.5) { child -> RasterPrecisionDocument in
                let context = try PrecisionWorkContext(source: source)
                return try source.selectedEdit(request: request, coverage: [], workingLimit: context.limit,
                                               otherRetained: context.otherRetained, control: child) ?? source
            }
            return try detailing(blurred, p, form: form, control: control) ?? blurred
        }
        guard let labelled = try labelled(RasterDocument(precision: detailed), p,
                                          name: capture.facade.layerInfo(p.layer)?.name ?? "Layer",
                                          form: form).precision else {
            throw DoctoredCoreError(message: "The typed split lost its typed document.")
        }
        return labelled
    }

    /// The detail step on the blurred duplicate, by form: Apply Image's arms (a Grayscale layer's
    /// one plane, or RGB's and Lab's three), or the core's `layer_detail`. nil is a detail that
    /// changed nothing.
    private static func detailing(_ blurred: RasterPrecisionDocument, _ p: FrequencySeparationParameters,
                                  form: Form, control: RasterJobControl) throws -> RasterPrecisionDocument? {
        guard form == .codes else {
            let context = try PrecisionWorkContext(source: blurred)
            return try control.withChildOperation(from: 0.5, to: 1) { child -> RasterPrecisionDocument? in
                try blurred.editingPixels(request: layerDetail(p).request, samples: [],
                                          workingLimit: context.limit, otherRetained: context.otherRetained,
                                          control: child)
            }
        }
        let facade = try RasterDocument(precision: blurred)
        if facade.isGray {
            return try NativeChannelMath.applyGray(facade, source: blurred, selection: nil,
                                                   parameters: detail(of: p, byteRGB: false),
                                                   control: control)
        }
        return try NativeChannelMath.applyRGB(facade, source: blurred, selection: nil,
                                              parameters: detail(of: p, byteRGB: false),
                                              control: control)
    }

    /// Names the pair and makes the high layer the detail: no mask, vector mask or style (Blend If
    /// included), full opacity, `form`'s blend mode, clipped to the low layer. Each step that would
    /// change nothing answers nil and is skipped.
    static func labelled(_ doc: RasterDocument, _ p: FrequencySeparationParameters,
                         name: String, form: Form) throws -> RasterDocument {
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
        try step { try $0.withLayerBlendMode(high, form.highBlendMode) }
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
