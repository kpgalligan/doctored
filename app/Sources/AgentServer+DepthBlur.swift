import AppKit

/// The agent mirrors of Filter ▸ Neural Filters ▸ Depth Blur… and Settings ▸ Models:
/// `depth_blur` runs the depth model on one layer and applies the filter as one undo step, the
/// commit the sheet's Apply runs (`DepthBlurRun.apply`), recorded centrally with its arguments;
/// `list_models` reports the store. Downloading has no tool: it stays a user action in Settings.
extension AgentServer {
    /// This file's MCP tools; merged by AgentServer+Registry.swift.
    static let depthBlurHandlers: [String: ToolHandler] = [
        "depth_blur": { $0.depthBlur },
        "list_models": { $0.listModels },
    ]

    /// Facts for this file's tools; merged in AgentServer+Registry.swift. `depth_blur` runs at 8,
    /// 16 and 32 bits on a layer's pixels and writes through the selection; `list_models` reads
    /// no document and records nothing.
    static let depthBlurToolFacts: [String: ToolFacts] = [
        "depth_blur": [.selectionSensitive, .typedTool, .rasterTool, .floatTool],
        "list_models": [.nonRecordable, .documentFree],
    ]

    /// depth_blur — mirrors Filter ▸ Neural Filters ▸ Depth Blur… and its Apply.
    func depthBlur(_ a: [String: Any]) throws -> String {
        let document = try target(a)
        let allowed: Set<String> = Set(DepthBlurSettings.sliders.map(\.key)).union([
            "document_id", "layer", "focus_subject", "focal_x", "focal_y", "depth_map_only", "output",
        ])
        if let unexpected = a.keys.sorted().first(where: { !allowed.contains($0) }) {
            throw ToolError(message: "Unknown depth_blur parameter: \(unexpected).")
        }
        try refuseNullOrBooleanNumbers(
            a, ["layer", "focal_x", "focal_y"] + DepthBlurSettings.sliders.map(\.key), in: "depth_blur")
        let settings: DepthBlurSettings
        do { settings = try DepthBlurSettings.parse(a) } catch { throw ToolError(message: error.localizedDescription) }
        let index = try paintLayerIndex(a, document)
        guard let model = DepthBlurRun.readyModel else { throw ToolError(message: DepthBlurRun.notInstalled) }
        guard !document.quickMaskActive else { throw ToolError(message: OperationScope.quickMaskRefusal) }
        guard let doc = document.doc else { throw ToolError(message: "Document has no image") }
        if let reason = scopeRefusal(.filter, target: .layer, doc: doc, layer: index) {
            throw ToolError(message: reason)
        }
        if settings.output == .layer { try rejectLockedEdit(document, index, DR_EDIT_PIXELS) }
        let dropped = settings.output == .layer && !doc.isHighDepth
            ? Self.description(of: document, layer: index) : nil
        let written: Int?
        let focal: Double
        do {
            let input = try DepthBlurRun.capture(document: document, layer: index,
                                                 needsSubject: settings.focusSubject)
            focal = input.focalDistance(settings)
            written = try DepthBlurRun.apply(document: document, layer: index, settings: settings,
                                             input: input, record: .notACommand)
        } catch let error as ToolError {
            throw error
        } catch {
            throw ToolError(message: error.localizedDescription)
        }
        return try pixelEditResult([
            "ok": true, "changed": written != nil, "layer": index,
            "output_layer": written.map { $0 as Any } ?? NSNull(),
            "focal_distance_used": (focal * 100).rounded() / 100,
            "model": ["id": model.entry.id, "version": model.entry.version],
        ], layer: index, rasterized: written == nil ? nil : dropped)
    }

    /// list_models — mirrors Settings ▸ Models: every catalogue model with its state.
    func listModels(_ a: [String: Any]) throws -> String {
        if let unexpected = a.keys.sorted().first(where: { $0 != "document_id" }) {
            throw ToolError(message: "Unknown list_models parameter: \(unexpected).")
        }
        let store = ModelStore.shared
        let models: [[String: Any]] = store.catalog.map { entry in
            let state = store.state(entry.id)
            var progress: Any = NSNull()
            if case .downloading(let fraction) = state { progress = fraction }
            var model: [String: Any] = [
                "id": entry.id, "name": entry.name, "capability": entry.capability,
                "version": entry.version, "state": state.name, "progress": progress,
                "bytes_on_disk": store.bytesOnDisk(entry.id), "accepted": store.isAccepted(entry.id),
                "license": ["name": entry.license.name, "url": entry.license.url.absoluteString,
                            "tier": entry.license.tier.rawValue],
                "training_data": entry.trainingData,
            ]
            if case .failed(let reason) = state { model["failure"] = reason }
            return model
        }
        return try jsonResult(["models": models])
    }
}
