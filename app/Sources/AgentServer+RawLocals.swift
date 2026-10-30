import AppKit

extension AgentServer {
    /// This file's MCP tools; merged by AgentServer+Registry.swift.
    static let rawLocalsHandlers: [String: ToolHandler] = [
        "edit_raw_development": editRawDevelopment,
        "get_raw_locals": getRawLocals,
        "get_raw_local_mask": getRawLocalMask,
        "copy_raw_local_settings": copyRawLocalSettings,
    ]

    /// Facts for this file's tools (recording, selection, depth admission); merged in AgentServer+Registry.swift.
    static let rawLocalsToolFacts: [String: ToolFacts] = [
        "edit_raw_development": [.nonRecordable, .commonTool, .floatTool],
        "get_raw_locals": [.nonRecordable, .commonTool, .floatTool],
        "get_raw_local_mask": [.nonRecordable, .commonTool, .floatTool],
        "copy_raw_local_settings": [.nonRecordable, .commonTool, .floatTool],
    ]

    /// Mirrors retained Develop > Apply: one prepared global/local source edit.
    func editRawDevelopment(_ arguments: [String: Any]) throws -> String {
        try rawWorkflowKeys(arguments, allowed: ["document_id", "layer", "raw", "operations", "before_recipe", "before_masks"])
        let document = try target(arguments)
        let capture = try ImageSourceCapture(document)
        let layer = try rawWorkflowLayer(arguments, document: document)
        let state = try RawLocalWork.state(capture.source, layer: layer)
        if let expected = arguments["before_recipe"] {
            guard let text = expected as? String, text.utf8.count <= 64 * 1024,
                  try state.descriptor.json() == text else {
                throw ToolError(message: "The recorded RAW local edit belongs to a different source, decoder or recipe.")
            }
        }
        if let expected = arguments["before_masks"] {
            guard let hashes = expected as? [String], hashes == (try RawLocalWork.maskFingerprints(capture.source, layer: layer)) else {
                throw ToolError(message: "The recorded RAW local edit's starting coverage changed.")
            }
        }
        let operations: [[String: Any]]
        if let raw = arguments["operations"] {
            guard let values = raw as? [[String: Any]] else { throw ToolError(message: "operations must be an array of RAW local edits.") }
            operations = values
        } else { operations = [] }
        // `raw.output_encoding` mirrors Layer ▸ Redevelop RAW… ▸ Development.
        let settings = try rawOpenSettings(arguments) ?? .init()
        let prepared = try RawLocalWork.prepare(source: capture.source, layer: layer, settings: settings, operations: operations)
        if let prepared {
            try RawLocalWork.publish(prepared: prepared, in: document, capture: capture,
                layer: layer, settings: settings, operations: operations)
        }
        return try jsonResult(["ok": true, "unchanged": prepared == nil, "layer": layer,
            "document": summary(document)])
    }

    /// Mirrors the local list/coverage overlay; no camera development occurs.
    func getRawLocals(_ arguments: [String: Any]) throws -> String {
        try rawWorkflowKeys(arguments, allowed: ["document_id", "layer"])
        let document = try target(arguments)
        let capture = try ImageSourceCapture(document)
        let layer = try rawWorkflowLayer(arguments, document: document)
        let state = try RawLocalWork.state(capture.source, layer: layer)
        var nodes: [[String: Any]] = []
        for (node, mask) in zip(state.nodes, state.masks) {
            var value = node.controlsJSON
            value["id"] = node.id; value["mask"] = try mask.info()
            nodes.append(value)
        }
        return try jsonResult(["layer": layer, "nodes": nodes, "source_width": state.width,
            "source_height": state.height, "coordinates": "oriented-source-pixel-centers-v1",
            "maximum_locals": RawEnhancementRecipe.maximumLocals,
            "maximum_camera_locals": RawEnhancementRecipe.maximumCameraLocals,
            "maximum_operations": RawLocalWork.maximumOperations,
            "maximum_dabs": RawLocalWork.maximumDabs, "maximum_operation_bytes": RawLocalWork.maximumOperationBytes])
    }

    /// The local mask inspector's native data counterpart. An explicit path
    /// exports the exact versioned u16 coverage resource, never its byte overlay.
    func getRawLocalMask(_ arguments: [String: Any]) throws -> String {
        try rawWorkflowKeys(arguments, allowed: ["document_id", "layer", "id", "path", "space", "channel"])
        let document = try target(arguments)
        let capture = try ImageSourceCapture(document)
        let layer = try rawWorkflowLayer(arguments, document: document)
        let state = try RawLocalWork.state(capture.source, layer: layer)
        let id = try requiredString(arguments, "id")
        guard let index = state.nodes.firstIndex(where: { $0.id == id }) else { throw ToolError(message: "That RAW local is unavailable.") }
        let space: String
        if let value = arguments["space"] {
            guard let text = value as? String else {
                throw ToolError(message: "RAW local mask space must be canonical or developed.")
            }
            space = text
        } else { space = "canonical" }
        let channel: String
        if let value = arguments["channel"] {
            guard let text = value as? String else {
                throw ToolError(message: "RAW local mask channel must be red, green or blue.")
            }
            channel = text
        } else { channel = "green" }
        guard ["canonical", "developed"].contains(space),
              ["red", "green", "blue"].contains(channel),
              (space == "developed" || arguments["channel"] == nil) else {
            throw ToolError(message: "RAW local mask space is canonical or developed; channel red/green/blue applies only to developed.")
        }
        let mask: RasterRawLocalMask
        if space == "developed", let lens = state.descriptor.recipe.sourceGeometry {
            let channels: [String: UInt32] = ["red": 0, "green": 1, "blue": 2]
            mask = try state.masks[index].remapped(lens.map().json(), channel: channels[channel] ?? 1)
        } else { mask = state.masks[index] }
        var result = try mask.info()
        result["id"] = id; result["layer"] = layer
        result["coordinates"] = space == "canonical"
            ? "oriented-source-pixel-centers-v1" : "developed-source-pixel-centers-v1"
        result["space"] = space
        if space == "developed" { result["channel"] = channel; result["derived"] = true }
        if let raw = arguments["path"] {
            let maskFile = SideFileExtension.rawLocalMask
            guard let path = raw as? String, path.hasPrefix("/"), !path.utf8.contains(0),
                  maskFile.matches(path: path) else {
                throw ToolError(message: "Native mask export requires an absolute "
                    + ".\(maskFile.current) path (or the legacy .\(maskFile.legacy)).")
            }
            let budget = UInt64(state.width) * UInt64(state.height) * 4 + 1024 * 1024
            _ = try RasterResources.shared.admit(scratch: budget)
            try BackgroundWork.wait(try WorkerTask(scratchEstimate: budget) {
                let bytes = try mask.encodedBytes()
                try DurableFile.write(bytes, to: URL(fileURLWithPath: path))
            }, title: "Exporting Native RAW Mask")
            try capture.requireCurrent(document)
            result["path"] = path
            result["format"] = "RZLM version 1: little-endian header and original u16 source coverage"
        }
        return try jsonResult(result)
    }

    /// Mirrors local Copy Settings / Paste Settings. Coverage and target mask
    /// properties remain local; only exposure, explicit WB intent and the tone transfer.
    func copyRawLocalSettings(_ arguments: [String: Any]) throws -> String {
        try rawWorkflowKeys(arguments, allowed: ["document_id", "layer", "id", "source_document_id", "source_layer", "source_id"])
        let targetDocument = try target(arguments)
        let targetCapture = try ImageSourceCapture(targetDocument)
        let layer = try rawWorkflowLayer(arguments, document: targetDocument)
        let sourceDocument: ImageDocument
        if let value = arguments["source_document_id"] {
            sourceDocument = try target(["document_id": rawWorkflowInteger(value, name: "source_document_id")])
        } else { sourceDocument = targetDocument }
        let sourceCapture = try ImageSourceCapture(sourceDocument)
        let sourceLayer = try arguments["source_layer"].map { try rawWorkflowInteger($0, name: "source_layer") }
            ?? sourceDocument.activeLayerIndex
        let state = try RawLocalWork.state(sourceCapture.source, layer: sourceLayer)
        let sourceID = try requiredString(arguments, "source_id")
        guard let node = state.nodes.first(where: { $0.id == sourceID }) else { throw ToolError(message: "The source RAW local is unavailable.") }
        var operation: [String: Any] = ["op": "update", "id": try requiredString(arguments, "id"),
            "exposure_offset": node.exposureOffset, "white_balance": node.whiteBalance.rawValue]
        if let temperature = node.temperature { operation["temperature"] = temperature }
        if let tint = node.tint { operation["tint"] = tint }
        operation["tone"] = node.tone?.json ?? NSNull()
        let prepared = try RawLocalWork.prepare(source: targetCapture.source, layer: layer, operations: [operation])
        try sourceCapture.requireCurrent(sourceDocument)
        if let prepared {
            try RawLocalWork.publish(prepared: prepared, in: targetDocument, capture: targetCapture,
                layer: layer, settings: .init(), operations: [operation])
        }
        return try jsonResult(["ok": true, "unchanged": prepared == nil, "document": summary(targetDocument)])
    }

    @inline(never) func rawLocalCatalog() -> [[String: Any]] {
        let target: [String: Any] = ["document_id": Self.docIDProperty,
            "layer": ["type": "integer", "minimum": 0, "description": "Retained RAW layer; default active."]]
        var edit = target
        edit["raw"] = Self.rawDevelopProperty
        edit["operations"] = ["type": "array", "maxItems": 256,
            "description": "Ordered native local edits, max16 locals (at most 4 camera locals; fewer above about 32 MP, where the core's 1 GiB source resource cap binds — the refusal names the figure)/4096 total brush dabs/256KiB. Every edit has op and stable ASCII id. create adds a native mask (mask.op empty, full, radial, linear, subject, people or sky; full covers the whole frame, for a local limited by its range alone); update patches controls; mask applies a native command; remap uses map:RawSourceMap (EXIF/affine only, same canonical dimensions); reorder changes index; remove uses only id. Coordinates are canonical oriented source pixel centers before explicit lens geometry. A camera local (exposure_offset ≠ 0 or white_balance not inherit) mixes its own alternate camera development, which recovers clipped highlights; at most four. create and update take tone {exposure (−4…4 stops, a linear-light gain on the global development), contrast, whites, blacks, saturation, clarity, texture, sharpness (−100…100), highlights, shadows, noise_reduction (0…100); absent keys 0, unknown refused}, which replaces the local's whole tone (null or every key 0 removes it): it adjusts the global development as the earlier locals left it (overlapping tone locals compound in order; on a camera local it adjusts that local's camera development) inside the coverage, with no camera re-development; Highlights recovers the bright side, Shadows lifts the dark side, Whites and Blacks move the white and black points, Clarity and Texture boost mid- and fine-scale local contrast, and last Noise Reduction smooths luminance noise (Reduce Noise at radius 0.5, luminance up to 0.1) and Sharpness sharpens (Smart Sharpen, radius 1 px, amount up to 150 %) or, below 0, softens (Gaussian Blur up to 3 px); their radii are fixed in pixels. Dehaze is not a local control. Locals mix in order before vignette and optical sampling. Tone runs on the 16-bit development at every document depth. create and update take range, which multiplies the mask's coverage and replaces the local's whole range (null removes it): {luminance:{low,high,low_falloff?,high_falloff?,invert?}, color:{lab:[L,a,b] or sample:[x,y], amount, falloff?, invert?}}, one or both. It is evaluated on the global development before any local, as CIE L*a*b* (D50) through the developed profile, so a local's own exposure or white balance never moves its selection. luminance: low and high are L*/100 with 0 ≤ low ≤ high ≤ 1; each falloff (0…1, default 0 = hard) is the width of the soft edge outside its bound. color: full inside ΔE*ab amount (0…200) of lab, fading over falloff (0…200, default 0); sample names a source point instead and is resolved to the mean Lab of the 5×5 pixels there. invert (default false) flips a component. Generated masks: mask {op: subject} (every foreground subject, as Select Subject), {op: people} (as Select All People) or {op: sky} (the sky model's own matte, as Select > Sky reads it, without the edge refinement), with no other field, is made by Vision or the sky model from the global development before any local, vignette, lens geometry and grain, and stored as ordinary native coverage that brush, invert, density, feather and range then refine. It is recomputed on request only: stored coverage never changes by itself, not on a global edit, a preset, Sync Settings or redevelop_raw. {op: mask, id, command: {op: subject}} (or people, or sky) is Recompute: it replaces the local's whole coverage from the global development of this call (brush strokes and invert made since are not replayed) and marks the local generated; a radial, linear, empty or full command clears the mark, brush, invert and remap keep it. When nothing is found the call is refused with 'No subject was found in the image.', 'No people were found in this image.' or 'No sky was found in this image.' and nothing changes; with no sky model in the store a sky mask is refused with 'the sky model is not installed; download it in Settings ▸ Models'. update does not take generated.",
            "items": ["type": "object"]]
        edit["before_recipe"] = ["type": "string", "description": "Optional exact recorded prior descriptor guard; Actions supplies this automatically."]
        edit["before_masks"] = ["type": "array", "items": ["type": "string"], "description": "Optional exact ordered prior mask SHA guards."]
        var mask = target
        mask["id"] = ["type": "string"]
        mask["path"] = ["type": "string", "description": "Optional absolute .drmask path for exact native RZLM1 u16 coverage export."]
        mask["space"] = ["type": "string", "enum": ["canonical", "developed"], "description": "Canonical authored coverage (default) or derived post-optical coverage."]
        mask["channel"] = ["type": "string", "enum": ["red", "green", "blue"], "description": "Developed-space optical channel; default green."]
        var copy = target
        copy["id"] = ["type": "string", "description": "Existing receiving mask id."]
        copy["source_id"] = ["type": "string", "description": "Source local id."]
        copy["source_document_id"] = Self.docIDProperty
        copy["source_layer"] = ["type": "integer", "minimum": 0]
        return [tool("edit_raw_development", "Develop RAW > Apply: prepare globals and native local masks together, preserve original camera bytes and local resources in both fallback depths, then publish one Undo. Empty/equivalent edits add no history; Cancel/stale/unsupported requests preserve the saved result. Local mask and decoder work use bounded sequential stages. In a 32-bit float document the 16-bit development is rendered into linear light, as a placed image is; a 32-bit float (linear) development (raw.output_encoding linear_float) blends its locals in float with no clamp, reads a range's Lab from the linear base (a value above white is L* 100) and applies the vignette as a linear gain. A local tone refuses there: \"RAW local tone controls are not available for a 32-bit float development.\"", edit),
            tool("get_raw_locals", "Develop RAW > Local list/overlay: inspect saved local controls and native mask metadata without decoding camera data. Reports maximum_locals (16) and maximum_camera_locals (4). A local with a tone reports tone with all eleven keys. A local with a luminance or colour range reports it as range, every field present and a colour always as lab (a sample point is resolved when the edit is applied). A local whose coverage was generated reports generated: subject, people or sky (absent otherwise); its mask is the stored coverage, which only a recompute replaces.", target),
            tool("get_raw_local_mask", "Develop RAW > Local mask inspection: read native coverage metadata or export exact .drmask data independently from byte display overlays.", mask, required: ["id"]),
            tool("copy_raw_local_settings", "Develop RAW > Copy/Paste Local Settings: copy only exposure offset, white-balance intent and tone (a source local with no tone clears the target's) into an existing target mask. Preserve target coverage, density, feather, name, id and camera. A captured source change refuses publication; Actions records resolved target updates.", copy, required: ["id", "source_id"])]
    }
}
