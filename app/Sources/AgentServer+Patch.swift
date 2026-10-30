import AppKit

/// patch_region: the agent mirror of the Patch tool. The region comes from an
/// explicit polygon or from the current selection, and the commit — which
/// end of the drag is healed, how far the source is displaced to reach it,
/// and the job that solves it — is `PatchEditCapture.startByte` /
/// `startNative`, the same calls the tool's own commit makes
/// (EditorViewController+Patch.patchCommitted), so a patch driven from here
/// and the same patch dragged by hand land identical pixels.
extension AgentServer {
    /// This file's MCP tools; merged by AgentServer+Registry.swift.
    static let patchHandlers: [String: ToolHandler] = [
        "patch_region": { $0.patchRegion },
    ]

    /// Facts for this file's tools (recording, selection, depth admission); merged in AgentServer+Registry.swift.
    static let patchToolFacts: [String: ToolFacts] = [
        // A float layer's Patch is the typed heal (`FloatRetouch`).
        "patch_region": [.typedTool, .rasterTool, .floatTool, .cmykTool],
    ]

    /// patch_region — mirrors the Patch tool.
    ///
    /// The region IS the confinement, as it is in the tool: an explicit
    /// polygon replaces the current selection for this edit rather than
    /// intersecting with it (the Patch tool's own outline behaves the same
    /// way), and omitting `points` patches the selection itself. That is the
    /// one place this handler departs from the stroke tools, which clip to
    /// the live selection — a stroke has no region of its own to be confined
    /// by.
    func patchRegion(_ a: [String: Any]) throws -> String {
        let document = try target(a)
        let index = try paintLayerIndex(a, document)
        // The UI refuses a patch commit on an adjustment layer with this
        // sentence in an alert (EditorViewController+Patch.patchCommitted →
        // refuseAdjustmentPixelEdit): there are no pixels to rewrite.
        try rejectAdjustmentPixelEdit(document, index)
        guard let doc = document.doc else { throw ToolError(message: "Document has no image") }
        let width = doc.width
        let height = doc.height

        guard let rawX = doubleArg(a, "offset_x"), let rawY = doubleArg(a, "offset_y"),
            rawX.isFinite, rawY.isFinite
        else {
            throw ToolError(
                message: "patch_region requires offset_x and offset_y — how far the region is "
                    + "dragged, in canvas px. With direction \"source\" they point at the "
                    + "good texture the outlined region is healed from.")
        }
        // The same ±100,000 px wall `parsePoints` puts on a stroke
        // coordinate, and for the same reason: far past any real canvas, but
        // a hard stop before a displacement no rectangle arithmetic (or the
        // integer the result reports it as) can hold.
        guard abs(rawX) <= 100_000, abs(rawY) <= 100_000 else {
            throw ToolError(
                message: "offset_x and offset_y must be within ±100,000 canvas px — got "
                    + "(\(rawX), \(rawY)). A patch displaces a region inside the picture.")
        }
        // Whole pixels, like the tool's drag: a fractional offset would
        // resample the source through CoreGraphics, softening exactly the
        // texture being moved and costing the heal its exactness on a smooth
        // gradient.
        let offset = CGVector(dx: rawX.rounded(), dy: rawY.rounded())
        guard offset.dx != 0 || offset.dy != 0 else {
            throw ToolError(
                message: "patch_region needs a non-zero offset: with no displacement the "
                    + "region would be healed from itself, which changes nothing.")
        }
        let destination = try patchDirection(a)
        let choice = try retouchSampling(a, default: .all)
        try rejectRetouchOutput(a, "patch_region")
        let (region, source) = try patchRegionArg(a, document, width: width, height: height)

        try rejectLockedEdit(document, index, DR_EDIT_PIXELS)
        // Latched before the edit, like the tool's own commit: nothing can
        // change what is being sampled mid-patch.
        let captured = try PatchEditCapture(document: document, layer: index,
            choice: choice, destination: destination, preview: false)
        let result = PatchSession.Result(region: region, offset: offset)
        let job: DocumentJob
        do {
            job = doc.isHighDepth
                ? try captured.startNative(result, document: document, record: .notACommand,
                    showProgress: false)
                : try captured.startByte(result, document: document, record: .notACommand,
                    showProgress: false)
        } catch let error as DoctoredCoreError where error.message == PatchEditCapture.missesCanvas {
            throw ToolError(
                message: "The patch falls outside the picture: displaced by "
                    + "(\(Int(offset.dx)), \(Int(offset.dy))) px, the region and the pixels "
                    + "it samples do not overlap the canvas.")
        }
        job.details = ["region": source, "direction": destination ? "destination" : "source",
            "offset": ["x": Int(offset.dx), "y": Int(offset.dy)],
            "healed_bounds": Self.patchBounds(region, destination ? offset : .zero),
            "applied": choice.report]
        return try finishJobCall(job, tool: "patch_region", arguments: a,
            unchanged: { try self.patchNoOpResult(layer: index) })
    }

    /// A patch that changed nothing: a no-op, not a failure, at both depths.
    func patchNoOpResult(layer: Int) throws -> String {
        try jsonResult([
            "ok": true, "changed": false, "layer": layer,
            "note": "Patch changed nothing: the healed region never landed on layer "
                + "\(layer)'s pixels — the layer's extent may not reach it — or the two "
                + "areas already hold the same pixels. No undo step was added.",
        ])
    }

    /// The `direction` argument: which end of the drag is healed. Source (the
    /// default) heals the OUTLINED region from the texture at
    /// the offset; Destination heals the region the offset lands on, from the
    /// outlined texture.
    private func patchDirection(_ a: [String: Any]) throws -> Bool {
        switch stringArg(a, "direction") ?? "source" {
        case "source": return false
        case "destination": return true
        case let other:
            throw ToolError(
                message: "direction must be source or destination (got \"\(other)\")")
        }
    }

    /// The region to patch and the word for where it came from: an explicit
    /// polygon, or the live selection — which may be any shape at all,
    /// including a magic-wand or Select Subject mask, which is exactly why
    /// the tool and this handler pass a CanvasSelection around rather than a
    /// point list.
    func patchRegionArg(
        _ a: [String: Any], _ document: ImageDocument, width: Int, height: Int
    ) throws -> (CanvasSelection, String) {
        guard a["points"] != nil else {
            guard let selection = editor(document)?.agentSelection else {
                throw ToolError(
                    message: "patch_region needs a region: pass points to outline one, or "
                        + "make a selection first with select_rect, select_ellipse, "
                        + "select_polygon, select_magic_wand or select_subject.")
            }
            guard selection.canvasWidth == width, selection.canvasHeight == height else {
                throw ToolError(
                    message: "The selection was made on a canvas of a different size. Make "
                        + "it again on this document.")
            }
            return (selection, "selection")
        }
        let points = try parsePoints(a)
        guard points.count >= 3 else {
            throw ToolError(
                message: "points must outline a region — 3 points or more (the outline "
                    + "closes itself). Omit points to patch the current selection.")
        }
        // The SAME .polygon shape the Lasso and the Patch tool's own outline
        // commit, through the same initializer, which is also what rejects an
        // outline that encloses no canvas pixel.
        guard let region = CanvasSelection(
            shape: .polygon(points), canvasWidth: width, canvasHeight: height)
        else {
            throw ToolError(
                message: "The outline encloses nothing on this \(width) x \(height) canvas.")
        }
        return (region, "polygon")
    }

    /// The box the heal actually covers, reported back so the caller can see
    /// where the patch landed: the region's bounds in Source, and the bounds
    /// displaced by the offset in Destination — clipped to the canvas, since
    /// the part that hangs off it is not healed and saying otherwise would
    /// invite a second patch aimed at nothing.
    static func patchBounds(
        _ region: CanvasSelection, _ shift: CGVector
    ) -> [String: Int] {
        let canvas = CGRect(
            x: 0, y: 0, width: CGFloat(region.canvasWidth),
            height: CGFloat(region.canvasHeight))
        let box = region.bounds.offsetBy(dx: shift.dx, dy: shift.dy).intersection(canvas)
        // A null rect's edges are infinite, and Int(.infinity) traps. The
        // handler has already refused a patch that misses the canvas, so this
        // is unreachable — and reporting an empty box is still the answer.
        guard !box.isNull else { return ["x": 0, "y": 0, "width": 0, "height": 0] }
        return [
            "x": Int(box.minX), "y": Int(box.minY),
            "width": Int(box.width), "height": Int(box.height),
        ]
    }
}
