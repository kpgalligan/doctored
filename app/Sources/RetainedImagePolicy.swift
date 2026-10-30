import AppKit

// `RetainedImagePolicy` is the host's one retained-source DECISION: which edits a RAW layer, the
// only layer that keeps a retained master (2026-10-10), refuses until it is rasterized. The
// destructive pixel tools refuse (`refusal`, `allows`). Geometry keeps the master bound, and
// `canvas_size` is not in the set (12F-1, G5) because a canvas resize moves each layer's OFFSET
// and keeps its allocation while `effective_source` folds `offset − rendered_offset` into the
// source transform on every read.

/// A retained master must never silently replace painted or transformed fallback
/// pixels later. Unsupported edits require an explicit, undoable rasterization.
/// Core fallback identity checks independently prevent resurrection if an older
/// or external caller bypasses the public policy.
enum RetainedImagePolicy {
    static let rasterizeFirst = "Rasterize the RAW layer first (Layer > Rasterize RAW Layer, or rasterize_image_source). The current pixels stay exact; Undo restores the camera original."

    static func refusal(tool: String, arguments: [String: Any], document: ImageDocument) -> String? {
        // Acceptance validates the captured settings and editing state inside
        // the review owner; its ticket has no standalone output parameter.
        if tool == "content_aware_fill", arguments["preview_id"] != nil { return nil }
        guard let doc = document.doc else { return nil }
        // A Quick Mask fill writes selection coverage, never a layer's
        // pixels, so a retained master is not in its way.
        // Nor do a Quick Mask gradient and a Quick Mask blur
        // (`DocumentPrecision.refusal` states the same rule).
        let quickMaskEdit = tool == "fill_selection" || tool == "gradient"
            || (tool == "apply_filter" && arguments["filter"] as? String == "blur")
        if quickMaskEdit,
           (document.quickMaskActive && (arguments["target"] == nil || tool == "fill_selection"))
               || arguments["target"] as? String == "quick_mask" {
            return nil
        }
        let index = DocumentPrecision.integer(arguments["layer"])
            ?? DocumentPrecision.integer(arguments["index"]) ?? document.activeLayerIndex
        // `assign_profile` is deliberately NOT here since Phase 11C: the CORE
        // now refuses it by name, counting the bound layers, and offers
        // `rasterize_sources: true` as the disclosed rasterization this
        // policy asks for — one result, one undo step. Two rules for one
        // question would mean two sentences, and the core's is the one that
        // can count.
        // Phase 12F-1 (G5): `canvas_size` LEFT this set at both depths. A
        // canvas resize moves each layer's OFFSET and keeps its pixel
        // allocation, and `effective_source` folds `offset − rendered_offset`
        // into the source transform on every read, so the master stays bound
        // and the next rerender lands at the new position. Image ▸ Canvas
        // Size… has always been enabled and committed on such a document;
        // this is what makes MCP and the recorded Action agree with the sheet.
        // These are DISPATCH KEYS (`AgentServer.handlers`), not tool prose:
        // a name that does not appear there guards nothing. Three were wrong
        // and are fixed here — "spot_heal" and "patch" were never dispatch
        // keys (the tools are `spot_heal_stroke` and `patch_region`) and
        // `dodge_burn` was missing outright. `spot_heal_stroke` was the live
        // hole: it commits through `DocumentJobs`, not `performPixelEdit`,
        // whose own retained-source check covers the synchronous paths, so an
        // agent could spot-heal a layer with a retained master and silently
        // detach it.
        let pixelTools: Set<String> = ["auto_tone", "auto_contrast", "auto_color", "apply_filter", "brush_stroke", "eraser_stroke", "clone_stamp",
            "heal_stroke", "spot_heal_stroke", "patch_region", "content_aware_fill", "remove_stroke", "content_aware_move", "red_eye",
            "red_eye_auto", "dodge_burn", "blur_stroke", "sharpen_stroke", "smudge_stroke",
            "sponge_stroke", "history_brush_stroke", "art_history_brush_stroke", "fill", "fill_selection", "stroke_selection", "gradient", "clear_selection", "apply_image",
            // A path filled or stroked onto the layer's pixels.
            "fill_path", "stroke_path",
            "apply_layer_mask", "move_selected_pixels", "transform_selected_pixels"]
        // A missing `target` is the layer, as `AgentServer.paintTarget`
        // reads it, never the panel's plane.
        let target = arguments["target"] as? String ?? "layer"
        let retouchToNewLayer = ["clone_stamp", "heal_stroke", "spot_heal_stroke", "content_aware_fill", "remove_stroke"].contains(tool)
            && arguments["output"] as? String == RetouchOutput.newLayer.rawValue
        if !retouchToNewLayer, pixelTools.contains(tool), !["mask", "channel"].contains(target), !target.hasPrefix("channel:"), doc.hasRetainedSource(index) {
            return rasterizeFirst
        }
        return nil
    }

    static func allows(_ tool: EditorTool, document: ImageDocument?) -> Bool {
        guard let document, let doc = document.doc, doc.hasRetainedSource(document.activeLayerIndex),
              !document.quickMaskActive, document.planeEditTarget == .layer else { return true }
        if EditorViewController.retouchOptions(for: tool)?.outputToNewLayer == true { return true }
        // Remove to a new layer leaves the RAW layer's pixels alone.
        if tool == .remove, ToolOptionsStore.shared.remove.outputToNewLayer { return true }
        switch tool {
        case .brush, .eraser, .clone, .heal, .fill, .gradient, .dodge, .spotHeal, .patch, .redEye, .remove, .contentAwareMove: return false
        case .blurBrush, .sharpenBrush, .smudge, .sponge, .historyBrush: return false
        case .artHistoryBrush: return false
        default: return true
        }
    }

    static func validate(_ selector: Selector, document: ImageDocument) -> Bool? {
        guard document.doc != nil else { return nil }
        // Geometry now preserves retained masters. Destructive painting is
        // still guarded by `allows` and `refusal` above, not menu validation.
        _ = selector
        return nil
    }
}
