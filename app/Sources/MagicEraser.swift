import AppKit

// The Magic Eraser (`designs/magic-eraser.md`): a click erases, on the active layer, the region the
// Magic Wand would select from the clicked pixel. It is a REGION and an ERASE, both existing core
// ops, composed the way the Paint Bucket's Mode and Sample all layers are
// (`EditorViewController+BucketFill.swift`): the wand's 0-or-full region — over the layer alone,
// or over the composite with Sample All Layers — times the selection's coverage, then the Eraser's
// rule over it, `alpha' = alpha × (1 − opacity × coverage)` with the colour kept. 8-bit RGB erases
// through Edit ▸ Fill's Clear (`selectionFilled`); every typed document through `erase_native` with
// the region as its coverage plane and the selection as its second gate. No pixel math lives here.

/// One Magic Eraser click: the seed, the wand's Tolerance (0…255, in 8-bit codes; a typed
/// document's wand takes it × 257) and Contiguous, Sample All Layers, and the erase's opacity
/// (0…1). The canvas click and `magic_eraser` both build one.
struct MagicEraserRequest: Equatable {
    var x: Int
    var y: Int
    var tolerance: Double
    var contiguous: Bool
    var sampleAllLayers: Bool
    var opacity: Double

    /// The recorded `magic_eraser` step of a canvas click.
    var record: [ActionStep] { .magicEraser(self) }
}

/// The Magic Eraser's sentences, shared by the canvas and `magic_eraser`.
enum MagicEraser {
    static let name = "Magic Eraser"

    /// A mask, a colour or ink plane, an alpha channel or the Quick Mask as the target.
    static let targetRefusal = "The Magic Eraser erases a layer's own pixels: a mask, a colour "
        + "channel, an alpha channel and the Quick Mask hold no transparency of their own to "
        + "erase. Target the layer."

    /// `opacity` outside 0…1, or a tolerance outside 0…255.
    static let argumentRefusal = "magic_eraser takes tolerance 0…255 and opacity 0…1."

    /// A `blend_mode` argument: the erase has none.
    static let blendRefusal = "The Magic Eraser has no blend mode: it lowers the region's "
        + "transparency by opacity and keeps its colour."

    /// Layer `idx`'s transparency lock (its own or an enclosing group's), which keeps exactly what
    /// the Magic Eraser changes; nil when its alpha is free.
    static func transparencyRefusal(_ doc: RasterDocument, layer idx: Int) -> (message: String, owner: Int)? {
        guard let owner = doc.transparencyLockOwner(idx) else { return nil }
        let name = doc.layerInfo(idx)?.name ?? "this layer"
        let group = owner == idx ? nil : doc.layerInfo(owner)?.name ?? "this group"
        return (LockFlags.refusal(layerName: name, blocking: .transparency, group: group)
            + " The Magic Eraser changes transparency, which the lock keeps.", owner)
    }
}

extension RasterDocument {
    /// The 8-bit Magic Eraser on layer `idx`: the bucket's region and fill
    /// (`bucketFilled(…style:)`) with Edit ▸ Fill's Clear at the request's
    /// opacity, so alpha falls by `opacity × region × mask` and the colour
    /// stays. nil when the seed is off the canvas or outside the selection,
    /// or when nothing would change.
    func magicErased(_ idx: Int, request: MagicEraserRequest, mask: [UInt8]?) -> RasterDocument? {
        bucketFilled(
            idx, x: request.x, y: request.y, tolerance: Int(request.tolerance.rounded()),
            rgba: [0, 0, 0, 255], contiguous: request.contiguous, mask: mask,
            style: BucketFillStyle(mode: DR_BLEND_CLEAR, sampleAllLayers: request.sampleAllLayers),
            opacity: request.opacity)
    }
}

extension RasterPrecisionDocument {
    /// The typed Magic Eraser on `layer`: the typed wand's region from the
    /// seed — over the layer alone, or the composite with Sample All
    /// Layers — handed to `erase_native` as its coverage plane, with
    /// `selection` beside it as the second gate (the shape of
    /// `bucketFilledSamplingComposite`). `extra` joins the request (an
    /// Indexed document's background key, `IndexedDocument.backgroundFields`).
    /// nil when the seed lies off the canvas or outside the selection, or
    /// when nothing changes.
    func magicErased(
        layer: Int, request: MagicEraserRequest, selection: CanvasSelection?, width: Int,
        height: Int, extra: [String: Any] = [:]
    ) throws -> RasterPrecisionDocument? {
        guard request.x >= 0, request.y >= 0, request.x < width, request.y < height else {
            return nil
        }
        let selected = selection?.maskSamples16()
        if let selected,
           selected.count != width * height || selected[request.y * width + request.x] == 0 {
            return nil
        }
        let region = try NativeSelectionWork(
            source: self, width: width, height: height,
            operation: ["kind": "magic_wand", "x": request.x, "y": request.y,
                        "tolerance": Int((request.tolerance * 257).rounded()),
                        "contiguous": request.contiguous],
            scope: request.sampleAllLayers ? .composite : .layer(layer)
        ).run(title: MagicEraser.name)
        var planes = [try PrecisionScalarBuffer(samples: region, width: width, height: height)]
        if let selected {
            planes.append(try PrecisionScalarBuffer(
                samples: selected, width: width, height: height,
                provenance: selection?.hasNativeCoverage == true ? .nativeU16 : .promotedU8))
        }
        let command = try PrecisionPixelCommand(
            (["op": "erase_native", "layer": layer, "opacity": request.opacity,
              "selection": selected != nil] as [String: Any]).merging(extra) { $1 })
        return try PrecisionPixelEdit(self, command: command, input: .joining(planes))
            .run(title: MagicEraser.name)
    }
}

extension Array where Element == ActionStep {
    /// The Magic Eraser's click (`magic_eraser`), on the active layer.
    static func magicEraser(_ request: MagicEraserRequest) -> [ActionStep] {
        step("magic_eraser", [
            "x": request.x, "y": request.y, "tolerance": Int(request.tolerance.rounded()),
            "contiguous": request.contiguous, "sample_all_layers": request.sampleAllLayers,
            "opacity": request.opacity, "layer": ActionArgs.activeLayer,
        ], note: "Magic Eraser tool: click to erase")
    }
}
