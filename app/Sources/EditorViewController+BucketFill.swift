import AppKit

// The Fill tool's options: Contents (foreground or background), Mode
// (any paint blend mode) and Sample all layers (grow the region over the
// composite instead of the active layer alone). Mirrored for the agent by
// `fill`'s `blend_mode` and `sample_all_layers`.
//
// Normal on the current layer is the bucket fill the tool always ran
// (`dr_doc_bucket_fill`, `bucket_fill` at 16 bits) and is untouched. Any
// other combination is a REGION and a FILL, both existing core ops: the
// wand's region (`magicWand` over the composite, `magicWandLayer` over the
// layer alone — the same `grow_region`) times the selection's coverage, laid
// down through Edit ▸ Fill's blend (`selectionFilled`). At 16 bits the
// current-layer form is `bucket_fill` with its `blend_mode`, and the
// all-layers form is the typed wand's composite region handed to
// `paint_blend` as coverage, beside the selection when there is one.
//
// A mask, colour plane or channel holds coverage, not colour: a blend mode or
// a composite region means nothing there, so both options refuse by name on
// such a target at either depth.

/// The Fill tool's Mode and Sample all layers, as one value both the canvas
/// click and the `fill` tool read.
struct BucketFillStyle: Equatable {
    var mode: DrBlendMode = DR_BLEND_NORMAL
    var sampleAllLayers = false

    /// True for the fill the tool ran before its blend mode and Sample all
    /// layers options: Normal, current layer.
    var isDefault: Bool { mode == DR_BLEND_NORMAL && !sampleAllLayers }

    static let coverageTargetRefusal = "Fill's blend mode and Sample all layers apply to a "
        + "layer's pixels. A mask, colour plane or channel holds coverage: use Normal with "
        + "Sample all layers off there."

    /// The recorded `fill` step with this style's arguments added — only the
    /// ones off their defaults, so a default fill records as it always did.
    func recording(_ steps: [ActionStep]) -> [ActionStep] {
        var steps = steps
        for index in steps.indices {
            if mode != DR_BLEND_NORMAL {
                steps[index].arguments["blend_mode"] = DrBlendMode.displayName(for: mode)
            }
            if sampleAllLayers { steps[index].arguments["sample_all_layers"] = true }
        }
        return steps
    }
}

extension RasterDocument {
    /// The 8-bit bucket fill with a blend mode or a composite region: the
    /// wand's 0/255 region from (x, y) — over the composite, or over layer
    /// `idx` alone — times `mask` (the selection's coverage, nil for none),
    /// filled on layer `idx` with straight `rgba` through `style.mode`. nil
    /// when the seed is off the canvas or outside the selection, when the
    /// layer has no pixels to sample, or when nothing would change.
    func bucketFilled(
        _ idx: Int, x: Int, y: Int, tolerance: Int, rgba: [UInt8], contiguous: Bool,
        mask: [UInt8]?, style: BucketFillStyle
    ) -> RasterDocument? {
        guard x >= 0, y >= 0, x < width, y < height else { return nil }
        if let mask, mask.count != width * height || mask[y * width + x] == 0 { return nil }
        let grown = style.sampleAllLayers
            ? magicWand(x: x, y: y, tolerance: tolerance, contiguous: contiguous)
            : magicWandLayer(idx, x: x, y: y, tolerance: tolerance, contiguous: contiguous)
        guard let region = grown else { return nil }
        let coverage = mask.map { PlaneAlgebra.combine(region, $0, mode: .intersect) } ?? region
        return selectionFilled(
            idx, mask: coverage, rgba: rgba, mode: style.mode, opacity: 1,
            preserveTransparency: false)
    }
}

extension RasterPrecisionDocument {
    /// The 16-bit bucket fill with Sample all layers: the typed wand's
    /// region over the COMPOSITE from (x, y), handed to `paint_blend` on
    /// `layer` as its coverage plane, with `selection` beside it as the
    /// second gate. nil when the seed lies outside the selection or nothing
    /// changes. `tolerance` is in u16 codes.
    func bucketFilledSamplingComposite(
        layer: Int, x: Int, y: Int, tolerance: Int, contiguous: Bool, color: [Double],
        colorSpace: String, opacity: Double, mode: DrBlendMode, selection: CanvasSelection?,
        width: Int, height: Int
    ) throws -> RasterPrecisionDocument? {
        guard x >= 0, y >= 0, x < width, y < height else {
            throw DoctoredCoreError(message: "The fill's seed point is outside the canvas.")
        }
        let selected = selection?.maskSamples16()
        if let selected, selected.count != width * height || selected[y * width + x] == 0 { return nil }
        let region = try NativeSelectionWork(
            source: self, width: width, height: height,
            operation: ["kind": "magic_wand", "x": x, "y": y, "tolerance": tolerance,
                        "contiguous": contiguous],
            scope: .composite).run(title: "Fill")
        var planes = [try PrecisionScalarBuffer(samples: region, width: width, height: height)]
        if let selected {
            planes.append(try PrecisionScalarBuffer(
                samples: selected, width: width, height: height,
                provenance: selection?.hasNativeCoverage == true ? .nativeU16 : .promotedU8))
        }
        let command = try PrecisionPixelCommand([
            "op": "paint_blend", "layer": layer, "color": color, "color_space": colorSpace,
            "opacity": opacity, "blend_mode": mode.rawValue, "selection": selected != nil,
        ])
        return try PrecisionPixelEdit(self, command: command, input: .joining(planes))
            .run(title: "Fill")
    }
}

extension EditorViewController {
    /// The Fill tool's colour: the options bar's Contents.
    var fillToolColor: NSColor {
        ToolOptionsStore.shared.fill.contentsIndex == 1 ? backgroundColor : paintColor
    }

    /// The Fill tool's stored CMYK ink (#547): the Contents swatch's, nil when it has none.
    var fillToolInk: CmykInk? {
        let shared = ToolOptionsStore.shared.sharedState
        return ToolOptionsStore.shared.fill.contentsIndex == 1 ? shared.backgroundInk : shared.foregroundInk
    }

    /// The bar's Mode and Sample all layers. A stored mode outside the document's list
    /// (`toolBlendChoices`, Lab's since #547 phase 2f) paints as Normal; the store is kept.
    var fillToolStyle: BucketFillStyle {
        let options = ToolOptionsStore.shared.fill
        let stored = Self.blendMode(fromStored: options.blendIndex)
        let mode = Self.toolBlendChoices(for: document?.doc).contains { $0.0 == stored }
            ? stored : DR_BLEND_NORMAL
        // #547 Indexed phase 2c: off on an Indexed document, whose one layer's region is the
        // composite's (the composite path asks the `selection` op, which the mode refuses), and
        // on a Bitmap document (#547 Bitmap phase 2a-i), which holds one layer too.
        let oneLayer = document?.doc?.isIndexed == true || document?.doc?.isBitmap == true
        return BucketFillStyle(mode: mode, sampleAllLayers: options.sampleAllLayers && !oneLayer)
    }

    /// A click on a mask, plane or channel target with Mode or Sample all
    /// layers off its default: says why and answers true.
    func refuseFillStyleOnCoverage() -> Bool {
        guard paintTarget.isCoverage, !fillToolStyle.isDefault else { return false }
        NSSound.beep()
        let alert = NSAlert()
        alert.messageText = "The Fill tool's Mode and Sample all layers need a layer target."
        alert.informativeText = BucketFillStyle.coverageTargetRefusal
        if let window = view.window { alert.beginSheetModal(for: window) } else { alert.runModal() }
        return true
    }

    /// The 8-bit click with Mode or Sample all layers off its default; false
    /// (nothing done) for the default fill, which `fillClicked` runs as before.
    /// One undo step through `applyCapturedRewrite`; a region that changes
    /// nothing leaves no step.
    func fillWithStyle(at point: CGPoint) -> Bool {
        let style = fillToolStyle
        guard !style.isDefault, let document = document else { return false }
        let options = ToolOptionsStore.shared.fill
        let idx = document.activeLayerIndex
        let opacity = min(max(options.opacity, 0), 100) / 100
        let color = fillToolColor
        let rgba = colorBytes(color.withAlphaComponent(color.alphaComponent * CGFloat(opacity)))
        guard let capture = document.captureOperation() else { NSSound.beep(); return true }
        document.applyCapturedRewrite(
            capture, "Fill",
            record: style.recording(
                fillRecord(at: point, color: color, opacity: opacity, options: options))
        ) {
            capture.doc.bucketFilled(
                idx, x: Int(point.x), y: Int(point.y),
                tolerance: Int(options.tolerance.rounded()), rgba: rgba,
                contiguous: options.contiguous, mask: capture.selection, style: style)
        }
        return true
    }
}

extension AgentServer {
    /// `fill`'s `blend_mode` (the shared blend vocabulary, default Normal)
    /// and `sample_all_layers` (default false). On a mask, plane or channel
    /// target either one off its default is refused by name.
    func bucketFillStyle(_ a: [String: Any], target: PaintTarget) throws -> BucketFillStyle {
        let style = BucketFillStyle(
            mode: try blendModeArg(a) ?? DR_BLEND_NORMAL,
            sampleAllLayers: boolArg(a, "sample_all_layers") ?? false)
        if target.isCoverage, !style.isDefault {
            throw ToolError(message: BucketFillStyle.coverageTargetRefusal)
        }
        return style
    }

    /// 8-bit `fill` with `blend_mode` or `sample_all_layers` off its default:
    /// mirrors the Fill tool's click with the options bar's Mode and Sample
    /// all layers (`EditorViewController.fillWithStyle`).
    func fillWithStyle(
        _ document: ImageDocument, layer index: Int, _ arguments: FillArguments,
        style: BucketFillStyle, mask: [UInt8]?
    ) throws -> String {
        try rejectAdjustmentPixelEdit(document, index)
        let rasterized = try performPixelEdit(document, "Fill", pixelLayer: index) { doc in
            doc.bucketFilled(
                index, x: arguments.x, y: arguments.y, tolerance: arguments.tolerance,
                rgba: arguments.rgba, contiguous: arguments.contiguous, mask: mask, style: style)
        }
        return try pixelEditResult(
            ["ok": true, "layer": index, "blend_mode": DrBlendMode.displayName(for: style.mode),
             "sample_all_layers": style.sampleAllLayers],
            layer: index, rasterized: rasterized)
    }
}
