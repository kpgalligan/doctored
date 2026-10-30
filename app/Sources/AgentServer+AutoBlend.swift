import AppKit

/// Auto-Blend Layers (Panorama and focus stacking) for the agent:
///
/// | Handler | UI path it mirrors |
/// |---|---|
/// | `autoBlendLayers` | Edit ▸ Auto-Blend Layers… ▸ Apply (`EditorViewController.commitAutoBlend`, EditorViewController+AutoBlend.swift) |
///
/// `auto_blend_layers` is one recordable edit: one history step, one Action
/// step. It takes `layers` where the sheet takes the panel's selection,
/// `method` is the sheet's popup (default `stack_images`, so a call or a
/// recording without it means what it always meant), and `seamless` /
/// `align` / `fill` are the sheet's three checkboxes (`fill`, Content-Aware
/// Fill Transparent Areas, is Panorama's; a recording without it replays as
/// false).
extension AgentServer {
    /// This file's MCP tools; merged by AgentServer+Registry.swift.
    static let autoBlendHandlers: [String: ToolHandler] = [
        "auto_blend_layers": { $0.autoBlendLayers },
    ]

    /// Facts for this file's tools (recording, selection, depth admission); merged in AgentServer+Registry.swift.
    static let autoBlendToolFacts: [String: ToolFacts] = [
        // The core blends at 16 bits and at float through its own
        // exports, with no capability family to ask.
        "auto_blend_layers": [.typedTool, .floatTool],
    ]

    /// Mirrors Edit ▸ Auto-Blend Layers…: `AutoBlend.request`'s eligibility,
    /// `AutoBlendWork`'s one job, then one grouped edit (with Panorama's
    /// alignment, the canvas grown and the selection carried as
    /// `auto_align_layers` does). A refusal (a layer that already has a mask,
    /// a layer "align" could not register) returns in-band and leaves no
    /// history step.
    func autoBlendLayers(_ a: [String: Any]) throws -> String {
        let document = try target(a)
        let layers = try layerIndices(a, document, minimum: 2)
        guard let source = document.doc else { throw ToolError(message: "No image loaded.") }
        let method: AutoBlend.Method
        if let given = a["method"] {
            guard let name = given as? String, let known = AutoBlend.Method(rawValue: name) else {
                throw ToolError(message: "method must be panorama or stack_images.")
            }
            method = known
        } else {
            method = .stackImages
        }
        let seamless = try switchArgument(a, "seamless", default: true)
        let align = try switchArgument(a, "align", default: false)
        let fill = try switchArgument(a, "fill", default: false)
        let result: (prepared: PreparedDocumentEdit, outcome: AutoBlend.Outcome)
        do {
            let request = try AutoBlend.request(
                in: source, layers: layers, method: method, seamless: seamless, align: align,
                fill: fill)
            result = try AutoBlendWork(source: source, request: request)
                .wait(in: document, title: AutoBlend.title)
        } catch let error as ToolError {
            throw error
        } catch {
            throw ToolError(message: error.localizedDescription)
        }
        document.preparedEdit = result.prepared
        let outcome = result.outcome
        let canvas = outcome.aligned?.canvas
        let carried = canvas.flatMap { rect in
            document.selectionCarried(
                canvasWidth: Int(rect.width), height: Int(rect.height),
                originX: Int(-rect.minX), originY: Int(-rect.minY))
        }
        try performGroupedEdit(
            document, AutoBlend.title, changesCanvasGeometry: canvas != nil,
            resultingSelection: carried
        ) { _ in result.prepared.document }
        var reply: [String: Any] = [
            "ok": true,
            "changed": true,
            "method": outcome.method.rawValue,
            "layers": outcome.layers.map { layer -> [String: Any] in
                var row: [String: Any] = [
                    "layer": layer.layer, "name": layer.name, "share": layer.share,
                ]
                if let gain = layer.gain { row["gain"] = gain }
                return row
            },
            "working_memory_plan_bytes": outcome.planBytes,
            "document": summary(document),
        ]
        if let merged = outcome.mergedLayer { reply["merged_layer"] = merged }
        if let filled = outcome.fill {
            reply["fill"] = [
                "layer": filled.layer.map { $0 as Any } ?? NSNull(),
                "covered_pixels": filled.coveredPixels,
            ] as [String: Any]
        }
        if let aligned = outcome.aligned {
            // `auto_align_layers`' own shapes for the three lists and the
            // grown canvas.
            var shape: [String: Any] = [
                "reference": aligned.reference,
                "aligned": aligned.aligned.map { layer -> [String: Any] in
                    ["layer": layer.layer, "name": layer.name, "method": layer.method.rawValue,
                     "homography": layer.homography.m,
                     "quad": layer.quad.map { [Double($0.x), Double($0.y)] }]
                },
                "unchanged": aligned.unchanged.map {
                    ["layer": $0.layer, "name": $0.name] as [String: Any]
                },
            ]
            if let rect = canvas {
                // Where the old canvas's top-left sits in the new one.
                shape["canvas"] = [
                    "width": Int(rect.width), "height": Int(rect.height),
                    "origin": ["x": Int(-rect.minX), "y": Int(-rect.minY)],
                ] as [String: Any]
            }
            reply["aligned"] = shape
        }
        return try jsonResult(reply)
    }

    /// A boolean argument that may be absent: `value` when it is. Anything
    /// that is not a boolean (or its string spelling) is refused by name
    /// rather than read as the default.
    private func switchArgument(_ a: [String: Any], _ key: String, default value: Bool) throws
        -> Bool
    {
        guard a[key] != nil else { return value }
        guard let given = boolArg(a, key) else {
            throw ToolError(message: "\(key) must be true or false.")
        }
        return given
    }
}
