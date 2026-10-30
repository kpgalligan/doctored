import AppKit

/// The agent mirrors of Filter ▸ Neural Filters ▸ Super Zoom…, JPEG Artifacts Removal… and
/// Colorize…: `super_zoom` runs the sheet's Apply (`SuperZoomRun.run`) and opens the zoomed
/// layer as a new document; `jpeg_artifacts_removal` runs the sheet's job
/// (`JpegArtifactsRemovalRun.start`) and waits for it, one undo step; `neural_colorize` runs the
/// sheet's OK (`ColorizeRun.start`, or `ColorizeRun.runNewDocument` for output new_document). All
/// are recorded centrally with their settings.
extension AgentServer {
    /// This file's MCP tools; merged by AgentServer+Registry.swift.
    static let neuralFiltersHandlers: [String: ToolHandler] = [
        "super_zoom": { $0.superZoom },
        "jpeg_artifacts_removal": { $0.jpegArtifactsRemoval },
        "neural_colorize": { $0.neuralColorize },
    ]

    /// Facts for this file's tools; merged in AgentServer+Registry.swift. All run at 8 and 16
    /// bits on a layer's pixels and ignore the selection; 32-bit and CMYK refuse by name.
    static let neuralFiltersToolFacts: [String: ToolFacts] = [
        "super_zoom": [.typedTool, .rasterTool],
        "jpeg_artifacts_removal": [.typedTool, .rasterTool],
        "neural_colorize": [.typedTool, .rasterTool],
    ]

    /// super_zoom — mirrors Filter ▸ Neural Filters ▸ Super Zoom… and its OK.
    func superZoom(_ a: [String: Any]) throws -> String {
        let document = try target(a)
        let allowed: Set<String> = ["document_id", "layer", "zoom", "enhance_details", "remove_jpeg_artifacts",
                                    "noise_reduction", "sharpen"]
        if let unexpected = a.keys.sorted().first(where: { !allowed.contains($0) }) {
            throw ToolError(message: "Unknown super_zoom parameter: \(unexpected).")
        }
        try refuseNullOrBooleanNumbers(a, ["layer", "zoom", "noise_reduction", "sharpen"], in: "super_zoom")
        let settings: SuperZoomSettings
        do { settings = try SuperZoomSettings.parse(a) } catch { throw ToolError(message: error.localizedDescription) }
        let index = try paintLayerIndex(a, document)
        guard !document.quickMaskActive else { throw ToolError(message: OperationScope.quickMaskRefusal) }
        guard let doc = document.doc else { throw ToolError(message: "Document has no image") }
        if let reason = scopeRefusal(.filter, target: .layer, doc: doc, layer: index) {
            throw ToolError(message: reason)
        }
        let result: SuperZoomResult
        do {
            result = try SuperZoomRun.run(document: document, layer: index, settings: settings)
        } catch let error as ToolError {
            throw error
        } catch {
            throw ToolError(message: error.localizedDescription)
        }
        return try jsonResult([
            "ok": true, "document_id": documentID(for: result.document),
            "width": result.plan.width, "height": result.plan.height, "zoom": settings.zoom,
            "tiles": result.plan.tiles, "planned_bytes": result.plan.plannedBytes,
            "models": result.models.map { ["id": $0.id, "version": $0.version] },
        ])
    }

    /// jpeg_artifacts_removal — mirrors Filter ▸ Neural Filters ▸ JPEG Artifacts Removal… and its OK.
    func jpegArtifactsRemoval(_ a: [String: Any]) throws -> String {
        let document = try target(a)
        let allowed: Set<String> = ["document_id", "layer", "strength", "output"]
        if let unexpected = a.keys.sorted().first(where: { !allowed.contains($0) }) {
            throw ToolError(message: "Unknown jpeg_artifacts_removal parameter: \(unexpected).")
        }
        try refuseNullOrBooleanNumbers(a, ["layer"], in: "jpeg_artifacts_removal")
        let settings: JpegArtifactsSettings
        do { settings = try JpegArtifactsSettings.parse(a) } catch { throw ToolError(message: error.localizedDescription) }
        let index = try paintLayerIndex(a, document)
        guard document.planeEditTarget == .layer, !document.quickMaskActive else {
            throw ToolError(message: "jpeg_artifacts_removal writes layer RGB only. Masks, channels and "
                + "Quick Mask are unavailable.")
        }
        guard let doc = document.doc else { throw ToolError(message: "Document has no image") }
        if let reason = scopeRefusal(.filter, target: .layer, doc: doc, layer: index) {
            throw ToolError(message: reason)
        }
        if settings.output == .layer { try rejectLockedEdit(document, index, DR_EDIT_PIXELS) }
        let dropped = settings.output == .layer && !doc.isHighDepth
            ? Self.description(of: document, layer: index) : nil
        let job: DocumentJob
        do {
            job = try JpegArtifactsRemovalRun.start(document: document, layer: index, settings: settings,
                                                    record: .notACommand, showProgress: false)
        } catch {
            throw ToolError(message: error.localizedDescription)
        }
        job.agentOrigin = true
        do { try DocumentJobs.wait(job) } catch {
            guard job.state == "unchanged" else { throw ToolError(message: error.localizedDescription) }
        }
        let changed = job.state == "completed"
        let written: Int? = changed ? (settings.output == .newLayer ? index + 1 : index) : nil
        return try pixelEditResult([
            "ok": true, "changed": changed, "layer": index,
            "output_layer": written.map { $0 as Any } ?? NSNull(),
            "strength": settings.strength.rawValue, "quality_factor": settings.strength.qualityFactor,
            "tiles": job.details["tiles"] ?? NSNull(), "planned_bytes": job.details["planned_bytes"] ?? NSNull(),
            "model": ModelGate.ready(JpegArtifactsRemovalRun.capability).map {
                ["id": $0.entry.id, "version": $0.entry.version] as Any
            } ?? NSNull(),
        ], layer: index, rasterized: changed ? dropped : nil)
    }

    /// neural_colorize — mirrors Filter ▸ Neural Filters ▸ Colorize… and its OK.
    func neuralColorize(_ a: [String: Any]) throws -> String {
        let document = try target(a)
        let allowed: Set<String> = ["document_id", "layer", "saturation", "cyan_red", "magenta_green",
                                    "yellow_blue", "color_artifact_reduction", "output"]
        if let unexpected = a.keys.sorted().first(where: { !allowed.contains($0) }) {
            throw ToolError(message: "Unknown neural_colorize parameter: \(unexpected).")
        }
        try refuseNullOrBooleanNumbers(a, ["layer"] + ColorizeSettings.sliders.map(\.key), in: "neural_colorize")
        let settings: ColorizeSettings
        do { settings = try ColorizeSettings.parse(a) } catch { throw ToolError(message: error.localizedDescription) }
        let index = try paintLayerIndex(a, document)
        guard document.planeEditTarget == .layer, !document.quickMaskActive else {
            throw ToolError(message: "neural_colorize writes layer RGB only. Masks, channels and "
                + "Quick Mask are unavailable.")
        }
        guard let doc = document.doc else { throw ToolError(message: "Document has no image") }
        if let reason = scopeRefusal(.filter, target: .layer, doc: doc, layer: index) {
            throw ToolError(message: reason)
        }
        let model: Any = ModelGate.ready(ColorizeRun.capability).map {
            ["id": $0.entry.id, "version": $0.entry.version] as Any
        } ?? NSNull()
        if settings.output == .newDocument {
            let result: (document: ImageDocument, changed: Bool, plannedBytes: UInt64)
            do {
                result = try ColorizeRun.runNewDocument(document: document, layer: index, settings: settings)
            } catch let error as ToolError {
                throw error
            } catch {
                throw ToolError(message: error.localizedDescription)
            }
            return try jsonResult([
                "ok": true, "changed": result.changed, "document_id": documentID(for: result.document),
                "width": result.document.doc?.width ?? 0, "height": result.document.doc?.height ?? 0,
                "output": settings.output.rawValue, "planned_bytes": result.plannedBytes, "model": model,
            ])
        }
        if settings.output == .layer { try rejectLockedEdit(document, index, DR_EDIT_PIXELS) }
        let dropped = settings.output == .layer && !doc.isHighDepth
            ? Self.description(of: document, layer: index) : nil
        let job: DocumentJob
        do {
            job = try ColorizeRun.start(document: document, layer: index, settings: settings,
                                        record: .notACommand, showProgress: false)
        } catch {
            throw ToolError(message: error.localizedDescription)
        }
        job.agentOrigin = true
        do { try DocumentJobs.wait(job) } catch {
            guard job.state == "unchanged" else { throw ToolError(message: error.localizedDescription) }
        }
        let changed = job.state == "completed"
        let written: Int? = changed ? (settings.output == .newLayer ? index + 1 : index) : nil
        return try pixelEditResult([
            "ok": true, "changed": changed, "layer": index,
            "output_layer": written.map { $0 as Any } ?? NSNull(),
            "output": settings.output.rawValue,
            "planned_bytes": job.details["planned_bytes"] ?? NSNull(), "model": model,
        ], layer: index, rasterized: changed ? dropped : nil)
    }
}
