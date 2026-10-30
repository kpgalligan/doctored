import AppKit

extension AgentServer {
    /// This file's MCP tools; merged by AgentServer+Registry.swift.
    static let puppetWarpHandlers: [String: ToolHandler] = [
        "puppet_warp": { $0.puppetWarp },
    ]

    /// Facts for this file's tools (recording, selection, depth admission); merged in AgentServer+Registry.swift.
    static let puppetWarpToolFacts: [String: ToolFacts] = [
        // The layer's pixels, or with a selection its float, at 8, 16
        // and 32 bits (`geometry_puppet`).
        "puppet_warp": [.selectionSensitive, .typedTool, .rasterTool, .floatTool, .cmykTool],
    ]

    /// `puppet_warp` — mirrors Edit ▸ Puppet Warp's commit
    /// (EditorViewController+PuppetWarp.swift): the pins, mode, density and
    /// expansion on the layer, or with a selection active on its float, as
    /// ONE "Puppet Warp" undo step through `MeshWarpWork`. A described
    /// layer rasterizes silently and says so. Unmoved pins answer
    /// `changed: false` and add no step; every refusal is the core's own
    /// sentence.
    func puppetWarp(_ a: [String: Any]) throws -> String {
        let document = try target(a)
        try commitPendingCanvasSessions(document)
        try refuseNullOrBooleanNumbers(a, ["layer"], in: "puppet_warp")
        let allowed: Set<String> = ["document_id", "layer", "pins", "mode", "density", "expansion"]
        if let unexpected = a.keys.sorted().first(where: { !allowed.contains($0) }) {
            throw ToolError(message: "Unknown puppet_warp parameter: \(unexpected).")
        }
        guard let doc = document.doc else { throw ToolError(message: "No image loaded.") }
        let layer = try paintLayerIndex(a, document)
        if let reason = scopeRefusal(.pixelOnly, target: .layer, doc: doc, layer: layer) {
            throw ToolError(message: reason)
        }
        let selection = document.canvasSelection
        try rejectLockedEdit(document, layer, DR_EDIT_PIXELS)
        if selection == nil { try rejectLockedEdit(document, layer, DR_EDIT_POSITION) }
        let parsed: (pins: [[String: Any]], extra: [String: Any])
        do { parsed = try PuppetWarp.agentFields(a) } catch {
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
        var fields: [String: Any] = [
            "op": float ? "puppet_selected" : "puppet_warp", "layer": layer, "pins": parsed.pins,
        ]
        fields.merge(parsed.extra) { _, new in new }
        if let bounds = selection?.bounds { fields["bounds"] = MoveSelectedPixels.requestBounds(bounds) }
        let coverage = selection?.maskSamples16()
        // The mesh and the pose the edit will build: the reply's snapped pins
        // and counts, and every pin refusal by name before any job runs.
        var meshFields: [String: Any] = [:]
        for key in ["density", "expansion"] { meshFields[key] = parsed.extra[key] }
        var solve: [String: Any] = ["pins": parsed.pins]
        solve["mode"] = parsed.extra["mode"]
        let identity = parsed.pins.allSatisfy(Self.unmovedPin)
        var vertices = 0, triangles = 0
        var pins: [[String: Any]] = parsed.pins.map { ["from": $0["from"] ?? NSNull(), "to": $0["to"] ?? NSNull()] }
        do {
            if !float { _ = try source.puppetWarpPlan(request: PuppetWarp.json(fields)) }
            let mesh = try source.puppetMesh(layer: layer, coverage: coverage, fields: meshFields)
            let pose = try mesh.solving(fields: solve)
            vertices = mesh.vertices.count
            triangles = mesh.triangles.count
            pins = pose.snapped.map { pin in
                let to = pose.positions[pin.vertex]
                return ["from": [Double(pin.from.x), Double(pin.from.y)],
                        "to": [Double(to.x), Double(to.y)]]
            }
        } catch {
            // The float form's edit names its own refusal (an empty
            // selection's sentence first), so its error waits for the edit.
            guard identity || float else { throw ToolError(message: error.localizedDescription) }
        }
        let work = MeshWarpWork(kind: .puppet, document: document, base: doc, source: source,
                                layer: layer, fields: fields, coverage: coverage,
                                selection: document.selectionHistory)
        var changed = false
        do {
            if float, doc.isHighDepth {
                changed = try work.publishTypedFloat(name: PuppetWarp.actionName,
                                                     record: .notACommand) == .published
            } else if let prepared = try float ? work.prepareFloat8(title: PuppetWarp.actionName)
                        : work.prepareLayer(title: PuppetWarp.actionName) {
                document.preparedEdit = prepared
                try performGroupedEdit(document, PuppetWarp.actionName, allowWarpChange: true) { _ in
                    prepared.document
                }
                changed = true
            }
        } catch let error as ToolError {
            throw error
        } catch {
            throw ToolError(message: error.localizedDescription)
        }
        let storage = !doc.isHighDepth ? "rgba8_unorm" : doc.isFloat ? "rgba32_float"
            : doc.isCmyk ? PrecisionDocumentDescriptor.cmykStorage : "rgba16_unorm"
        let result: [String: Any] = [
            "ok": true, "changed": changed, "layer": layer, "pins": pins,
            "vertices": vertices, "triangles": triangles, "storage": storage,
            "write_scope": float ? "selection" : "whole_layer",
        ]
        return try pixelEditResult(result, layer: layer, rasterized: changed ? dropped : nil)
    }

    /// A pin whose `to` equals its `from` with no Fixed angle (or 0).
    private static func unmovedPin(_ pin: [String: Any]) -> Bool {
        guard let from = pin["from"] as? [NSNumber], let to = pin["to"] as? [NSNumber],
              from.count == 2, to.count == 2 else { return false }
        let rotate = pin["rotate"]
        let still = rotate == nil || rotate is NSNull || (rotate as? NSNumber)?.doubleValue == 0
        return still && from.map(\.doubleValue) == to.map(\.doubleValue)
    }
}
