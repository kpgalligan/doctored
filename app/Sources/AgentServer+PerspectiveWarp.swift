import AppKit

extension AgentServer {
    /// This file's MCP tools; merged by AgentServer+Registry.swift.
    static let perspectiveWarpHandlers: [String: ToolHandler] = [
        "perspective_warp": { $0.perspectiveWarp },
    ]

    /// Facts for this file's tools (recording, selection, depth admission); merged in AgentServer+Registry.swift.
    static let perspectiveWarpToolFacts: [String: ToolFacts] = [
        // The layer's pixels, or with a selection its float, at 8, 16
        // and 32 bits (`geometry_perspective_warp`).
        "perspective_warp": [.selectionSensitive, .typedTool, .rasterTool, .floatTool, .cmykTool],
    ]

    /// `perspective_warp` — mirrors Edit ▸ Perspective Warp's commit
    /// (EditorViewController+PerspectiveWarp.swift): the quads (where each
    /// plane stands and where its corners go) and the straighten entries on
    /// the layer, or with a selection active on its float, as ONE
    /// "Perspective Warp" undo step through `MeshWarpWork`. The core resolves
    /// the layout first (`dr_perspective_warp_layout`: joins, straightening,
    /// shared edges), and the edit takes the resolved quads, as the canvas
    /// commit does. A described layer rasterizes silently and says so. An
    /// unwarped layout answers `changed: false` and adds no step; every
    /// refusal is the core's own sentence.
    func perspectiveWarp(_ a: [String: Any]) throws -> String {
        let document = try target(a)
        try commitPendingCanvasSessions(document)
        try refuseNullOrBooleanNumbers(a, ["layer"], in: "perspective_warp")
        let allowed: Set<String> = ["document_id", "layer", "quads", "straighten"]
        if let unexpected = a.keys.sorted().first(where: { !allowed.contains($0) }) {
            throw ToolError(message: "Unknown perspective_warp parameter: \(unexpected).")
        }
        guard let doc = document.doc else { throw ToolError(message: "No image loaded.") }
        let layer = try paintLayerIndex(a, document)
        if let reason = scopeRefusal(.pixelOnly, target: .layer, doc: doc, layer: layer) {
            throw ToolError(message: reason)
        }
        let selection = document.canvasSelection
        try rejectLockedEdit(document, layer, DR_EDIT_PIXELS)
        if selection == nil { try rejectLockedEdit(document, layer, DR_EDIT_POSITION) }
        let resolved: PerspectiveLayout
        do {
            let parsed = try PerspectiveWarp.agentFields(a)
            var fields: [String: Any] = ["quads": parsed.quads]
            if let straighten = parsed.straighten { fields["straighten"] = straighten }
            resolved = try RasterPerspectiveWarp.layout(fields: fields)
        } catch {
            throw ToolError(message: error.localizedDescription)
        }
        let float = selection != nil
        let dropped = Self.description(of: document, layer: layer)
        var source = doc
        // The typed float form drops the description in its own publication.
        if dropped != nil, !(float && doc.isHighDepth) {
            guard let plain = doc.withLayerMeta(layer, nil) else {
                throw ToolError(message: "The layer's description could not be dropped.")
            }
            source = plain
        }
        let quads = resolved.quadsJSON
        let fields = PerspectiveWarp.fields(layer: layer, quads: quads, float: float,
                                            bounds: selection?.bounds)
        let coverage = selection?.maskSamples16()
        if !float, !resolved.identity {
            // Every layer refusal by name before any job runs.
            do { _ = try source.perspectiveWarpPlan(request: PerspectiveWarp.json(fields)) } catch {
                throw ToolError(message: error.localizedDescription)
            }
        }
        let work = MeshWarpWork(kind: .perspective, document: document, base: doc, source: source,
                                layer: layer, fields: fields, coverage: coverage,
                                selection: document.selectionHistory)
        var changed = false
        do {
            if resolved.identity {
                changed = false
            } else if float, doc.isHighDepth {
                changed = try work.publishTypedFloat(name: PerspectiveWarp.actionName,
                                                     record: .notACommand) == .published
            } else if let prepared = try float ? work.prepareFloat8(title: PerspectiveWarp.actionName)
                        : work.prepareLayer(title: PerspectiveWarp.actionName) {
                document.preparedEdit = prepared
                try performGroupedEdit(document, PerspectiveWarp.actionName,
                                       allowWarpChange: true) { _ in prepared.document }
                changed = true
            }
        } catch let error as ToolError {
            throw error
        } catch {
            throw ToolError(message: error.localizedDescription)
        }
        let point = { (p: CGPoint) in [Double(p.x), Double(p.y)] }
        let shared: [[[Double]]] = resolved.sharedEdges.map { pair in
            pair.map { point(resolved.vertices[$0].from) }
        }
        let storage = !doc.isHighDepth ? "rgba8_unorm" : doc.isFloat ? "rgba32_float"
            : doc.isCmyk ? PrecisionDocumentDescriptor.cmykStorage : "rgba16_unorm"
        let result: [String: Any] = [
            "ok": true, "changed": changed, "layer": layer, "quads": quads,
            "shared_edges": shared, "storage": storage,
            "write_scope": float ? "selection" : "whole_layer",
        ]
        return try pixelEditResult(result, layer: layer, rasterized: changed ? dropped : nil)
    }
}
