import AppKit

// The MCP `rotate` tool. It mirrors TWO UI paths, and which one it takes is
// decided by the CORE's own classification of the angle
// (`CanvasRotation.plan`, i.e. `dr_canvas_rotation_plan`):
//
//   * an EXACT quarter turn — Image ▸ Rotate 90° CW / CCW / 180°, through
//     `applyingDocumentGeometry` (DescribedLayerGeometry.swift), which
//     composes the op into every described layer's description so text
//     and shapes stay editable. This is the path every `rotate`
//     call took before Phase 12F-1b3 and its reply is unchanged;
//   * a FILTERED angle — Image ▸ Rotate Arbitrary…
//     (EditorViewController+RotateCanvas.swift), which expands the canvas,
//     resamples every plane and regenerates text and shape layers from
//     their descriptions.
//
// An IDENTITY (a whole number of full turns) is the house successful no-op at
// every depth: `changed: false`, no history step, no dirtying.
extension AgentServer {
    /// This file's MCP tools; merged by AgentServer+Registry.swift.
    static let rotateCanvasHandlers: [String: ToolHandler] = [
        "rotate": rotate,
    ]

    /// Facts for this file's tools (recording, selection, depth admission); merged in AgentServer+Registry.swift.
    static let rotateCanvasToolFacts: [String: ToolFacts] = [
        // Phase 12F-1b: the EXACT canvas geometry. A zero-angle `crop` keeps
        // every raster and mask allocation; a quarter turn and a flip are a
        // Nearest PERMUTATION, so an extended-range component is only ever
        // copied. The FILTERED geometry is admitted too — a `crop`
        // with an angle (straighten), an arbitrary `rotate`, `image_size`,
        // the layer transforms and `transform_mask` — through the core's
        // premultiplied extended-range resampler, which clamps each RGB
        // component to the layer's own visible range (D3). `transform_again`
        // repeats a layer transform; with a selection it would transform the
        // selected PIXELS, which float does not admit, so
        // `DocumentPrecision.refusal` refuses that case by name.
        "rotate": [.typedTool, .floatTool, .cmykTool],
    ]

    func rotate(_ a: [String: Any]) throws -> String {
        let document = try target(a)
        guard let doc = document.doc else { throw ToolError(message: "Document has no image") }
        // `DocumentPrecision.rotateDegrees` is the ONE parser: 90 when the
        // caller named none, `.nan` when a value is present but unreadable.
        // Both depth gates read it too, and they act only on a FILTERED
        // classification — so an unreadable angle reaches here and is refused
        // ONCE, by this sentence, at 8 bits, 16 bits and float alike.
        guard let plan = CanvasRotation.plan(a, in: doc) else {
            throw ToolError(message: CanvasRotation.unreadable)
        }
        // Crop's wording, because it is the same vocabulary and the same
        // question. The sampler is parsed even on an exact route so a
        // misspelling is never silently accepted.
        let samplerName = (stringArg(a, "sampler") ?? "bicubic").lowercased()
        guard let sampler = Self.transformSamplers[samplerName] else {
            throw ToolError(
                message: "sampler must be nearest, bilinear, bicubic or lanczos "
                    + "(got \"\(samplerName)\")")
        }

        switch plan.kind {
        case .identity:
            return try structureNoOp(
                ["document": summary(document)],
                why: "a rotation by \(Self.transformNumber(plan.degrees))° is a whole number "
                    + "of full turns, so nothing moves.")
        case .quarter(let op):
            return try exactDocumentGeometry(document, op)
        case .filtered:
            guard plan.width > 0, plan.height > 0 else {
                throw ToolError(
                    message: "the rotated canvas would exceed the supported canvas size")
            }
            // The same channel-budget question the canvas-growing tools ask
            // first, so the refusal names the cap instead of a bare failure.
            try requireChannelBudget(document, width: plan.width, height: plan.height)
            // The layers whose descriptions the resample invalidates, read
            // BEFORE the edit: exactly the UI's own set. Adjustment layers
            // keep their meta — it describes an op, not pixels.
            let described = (0..<doc.layerCount).filter { document.layerDescribesSource($0) }
            // #1 Indexed phase 2c: an Indexed document's admitted plan, read before the edit
            // (`scripts/indexed-admission/measure.py`'s `rotate-indexed`); since Bitmap phase
            // 2a-ii a Bitmap document's too (`scripts/bitmap-admission/measure.py`'s
            // `rotate5-bitmap`).
            let indexedPlan = doc.isIndexed || doc.isBitmap
                ? try IndexedDocument.geometryPlanBytes(
                    doc, .rotateCanvas(degrees: plan.degrees,
                                       filter: RasterDocument.precisionFilter(sampler.filter)))
                : nil
            // A core refusal arrives as a thrown sentence and is reported
            // verbatim, the way `crop` reports one.
            // The job prepares the display too, published through
            // `preparedEdit`.
            var failure: Error?
            defer { document.preparedEdit = nil }
            do {
                // The selection turns with the canvas.
                let carried = document.selectionCarried(
                    by: plan.map(canvasWidth: doc.width, canvasHeight: doc.height),
                    width: plan.width, height: plan.height)
                try performGroupedEdit(document, "Rotate Canvas", changesCanvasGeometry: true,
                                       resultingSelection: carried) {
                    base in
                    do {
                        guard let prepared = try base.rotatingCanvas(
                            degrees: plan.degrees, sampler: sampler.filter, described: described)
                        else { return nil }
                        document.preparedEdit = prepared
                        return prepared.document
                    } catch {
                        failure = error
                        return nil
                    }
                }
            } catch {
                if let failure { throw ToolError(message: failure.localizedDescription) }
                throw error
            }
            var result: [String: Any] = [
                "ok": true,
                "degrees": Self.transformNumber(plan.degrees),
                "sampler": sampler.name,
                "canvas": ["width": plan.width, "height": plan.height],
                "origin": ["x": plan.origin.x, "y": plan.origin.y],
                "document": summary(document),
            ]
            if let indexedPlan { result["working_memory_plan_bytes"] = indexedPlan }
            return try jsonResult(result)
        }
    }
}
