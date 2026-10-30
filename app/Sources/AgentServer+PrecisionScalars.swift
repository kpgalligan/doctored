import AppKit

extension AgentServer {
    /// Mirrors Mask Properties and Select > Save/Load Selection. Only an
    /// explicit selection transfer reduces/promotes transient byte coverage;
    /// persistent mask/channel source buffers stay u16.
    func precisionScalarTool(_ tool: String, _ a: [String: Any], document: ImageDocument) throws -> String {
        let capture = try PrecisionEditorCapture(document)
        let doc = capture.facade
        let layer = intArg(a, "layer") ?? intArg(a, "index") ?? document.activeLayerIndex
        let command: PrecisionPixelCommand
        var input: PrecisionScalarBuffer?
        // Locks on the MASK ops, with the byte path's own wording
        // (`LockFlags.refusal` through `rejectLockedEdit`, which this typed
        // path does not reach). Adding, removing, enabling and re-describing
        // a mask are `Mask` edits, which only Lock All blocks; APPLYING one
        // writes the layer's ALPHA and is a `MaskApply` edit, which Lock
        // Transparency blocks too (`doc_lock`'s module doc). The channel and
        // selection ops belong to no layer, so no lock addresses them.
        switch tool {
        case "add_layer_mask", "remove_layer_mask", "set_layer_mask_enabled",
             "set_mask_properties":
            let applying = tool == "remove_layer_mask" && boolArg(a, "apply") == true
            try rejectLockedEdit(document, layer, applying ? DR_EDIT_MASK_APPLY : DR_EDIT_MASK)
        default: break
        }
        switch tool {
        case "get_mask_properties":
            // Beside a vector mask these are the RASTER mask's, which is what
            // the mask tools edit (`RasterDocument.maskEditBaseline`).
            guard let (p, enabled) = doc.maskEditBaseline(layer) else { throw ToolError(message: "The layer has no mask.") }
            return try jsonResult(p.report.merging(["layer": layer, "enabled": enabled, "storage": "u16"]) { first, _ in first })
        case "add_layer_mask":
            // A vector mask with no raster mask beside it takes one.
            guard !doc.layerHasMask(layer) || doc.vectorMask(layer).map({ !$0.hasRaster }) == true
            else { throw ToolError(message: "The layer already has a mask.") }
            let mode = stringArg(a, "kind") ?? "reveal_all"
            guard ["reveal_all", "hide_all", "from_selection"].contains(mode) else {
                throw ToolError(message: "16-bit masks support reveal_all, hide_all or from_selection.")
            }
            if mode == "from_selection", capture.selection == nil { throw ToolError(message: "Make a selection first.") }
            let selection = mode == "from_selection" ? capture.selection : nil
            if mode == "hide_all" {
                _ = try RasterResources.shared.admit(scratch: UInt64(doc.width * doc.height) * 2 + 512)
                input = try PrecisionScalarBuffer(samples: [UInt16](repeating: 0, count: doc.width * doc.height),
                    width: doc.width, height: doc.height)
            } else { input = try PrecisionStrokeCoverage.selection(width: doc.width, height: doc.height, selection: selection) }
            command = try .setMask(layer: layer, width: doc.width, height: doc.height)
        case "remove_layer_mask":
            if boolArg(a, "apply") == true { return try applyNativeLayerMask(a, document: document) }
            command = try .removeMask(layer: layer)
        case "set_layer_mask_enabled", "set_mask_properties":
            guard let (p, wasEnabled) = doc.maskEditBaseline(layer) else { throw ToolError(message: "The layer has no mask.") }
            let enabled: Bool
            if tool == "set_layer_mask_enabled" {
                guard let value = boolArg(a, "enabled") else { throw ToolError(message: "enabled must be a boolean.") }
                enabled = value
            } else { enabled = wasEnabled }
            for key in ["linked"] where a[key] != nil && boolArg(a, key) == nil {
                throw ToolError(message: "\(key) must be a boolean.")
            }
            for key in ["density", "feather"] where a[key] != nil && doubleArg(a, key) == nil {
                throw ToolError(message: "\(key) must be a finite number.")
            }
            command = try .maskProperties(layer: layer, x: p.offsetX, y: p.offsetY,
                linked: boolArg(a, "linked") ?? p.linked, density: doubleArg(a, "density") ?? p.density,
                feather: doubleArg(a, "feather") ?? p.feather, enabled: enabled)
        case "save_selection", "add_channel":
            return try savePrecisionChannel(a, document: document, adding: tool == "add_channel")
        case "delete_channel": command = try .removeChannel(index: channelArgument(a, doc))
        case "load_selection": return try loadPrecisionSelection(a, document: document)
        default: throw ToolError(message: "Unsupported 16-bit scalar operation.")
        }
        let work = try PrecisionPixelEdit(capture.source, command: command, input: input)
        let changed = try capture.publish(work.run(), to: document, name: tool.replacingOccurrences(of: "_", with: " "))
        return try jsonResult(["ok": true, "changed": changed, "storage": "u16", "document": summary(document)])
    }

    func loadPrecisionSelection(_ a: [String: Any], document: ImageDocument) throws -> String {
        let capture = try PrecisionEditorCapture(document)
        guard !capture.quickMask else { throw ToolError(message: OperationScope.quickMaskRefusal) }
        let source = try selectionSource(a, document)
        var plane = try PrecisionScalarOperations.canvasPlane(capture.facade, source: source).samples
        if boolArg(a, "invert") == true { plane = plane.map { 65535 - $0 } }
        try capture.requireCurrent(document)
        guard let editor = editor(document) else { throw ToolError(message: "No editor holds this selection.") }
        let selection = CanvasSelection(shape: .mask16(plane), canvasWidth: capture.facade.width, canvasHeight: capture.facade.height)
        let mode = try selectionMode(a)
        let combined: CanvasSelection?
        if let selection { combined = CanvasSelection.combine(editor.agentSelection, with: selection, mode: mode) }
        else { combined = mode == .replace || mode == .intersect ? nil : editor.agentSelection }
        return try setCombined(editor, combined, extra: ["coverage_bits": 16])
    }

    func savePrecisionChannel(_ a: [String: Any], document: ImageDocument, adding: Bool) throws -> String {
        let capture = try PrecisionEditorCapture(document), doc = capture.facade
        let from = adding ? stringArg(a, "from") ?? "empty" : "selection"
        let input: [UInt16]
        switch from {
        case "empty": input = [UInt16](repeating: 0, count: doc.width * doc.height)
        case "selection":
            guard let selection = capture.selection else { throw ToolError(message: "Make a selection first.") }
            input = selection.maskSamples16()
        case "layer_alpha", "layer_mask", "plane":
            input = try PrecisionScalarOperations.canvasPlane(doc, source: selectionSource(a, document)).samples
        default: throw ToolError(message: "from must be empty, selection, layer_alpha, layer_mask or plane.")
        }
        let index = !adding && a["channel"] != nil ? try channelArgument(a, doc) : nil
        let name = index.flatMap { doc.channelInfo($0)?.name } ?? stringArg(a, "name") ?? doc.nextChannelName
        let samples: [UInt16]
        if let index {
            samples = try PrecisionScalarOperations.combine(
                PrecisionScalarOperations.read(doc, target: "channel", index: index).samples,
                input, mode: selectionMode(a))
        } else { samples = input }
        let work = try PrecisionScalarWork(source: capture.source, command: .setChannel(index: index, name: name), samples: samples)
        var result = try work.run(title: "Save Channel")
        if adding, let current = result {
            let channel = doc.channelCount
            let rgba = try Self.overlayBytes(parseColor(a, "overlay_color", fallback: Self.defaultOverlayColor))
            let properties = try PrecisionPixelCommand(["op": "channel_properties", "index": channel,
                "overlay_color": [rgba.red, rgba.green, rgba.blue], "overlay_opacity": Self.overlayOpacity(doubleArg(a, "overlay_opacity"))])
            result = try PrecisionScalarWork(source: current, command: properties).run(title: "Channel Options") ?? current
        }
        let changed = try capture.publish(result, to: document, name: adding ? "New Channel" : "Save Selection")
        return try jsonResult(["ok": true, "changed": changed, "channel": index ?? doc.channelCount,
            "name": name, "storage": "u16", "coverage_bits": 16])
    }
}
