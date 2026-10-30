import AppKit

/// The agent mirrors of the model selections: `select_sky` mirrors Select ▸ Sky,
/// `select_person_parts` Select ▸ Person Parts…'s Apply, and `object_finder_select` a click, box
/// or Shift-/⌥-click of the Object Selection tool with Object Finder on. Each runs the same code
/// as the UI (`SkySelection`, `PersonPartsRun`, `ObjectFinder`), commits one selection Undo, and
/// refuses by name when its model is not installed (`ModelGate.notInstalled`).
extension AgentServer {
    /// This file's MCP tools; merged by AgentServer+Registry.swift.
    static let modelSelectionHandlers: [String: ToolHandler] = [
        "select_sky": { $0.selectSky },
        "select_person_parts": { $0.selectPersonParts },
        "object_finder_select": { $0.objectFinderSelect },
    ]

    /// Facts for this file's tools (recording, selection, depth admission); merged in
    /// AgentServer+Registry.swift. Each runs at 8, 16 and 32 bits on the analysis composite.
    static let modelSelectionToolFacts: [String: ToolFacts] = [
        "select_sky": [.commonTool, .floatTool],
        "select_person_parts": [.commonTool, .floatTool],
        "object_finder_select": [.commonTool, .floatTool],
    ]

    /// Refuses a key outside `allowed`, naming the tool.
    private func refuseUnknown(_ a: [String: Any], _ allowed: Set<String>, in tool: String) throws {
        if let unexpected = a.keys.sorted().first(where: { !allowed.contains($0) }) {
            throw ToolError(message: "Unknown \(tool) parameter: \(unexpected).")
        }
    }

    /// The reply every model selection shares: the selection's bounds (none when it is empty)
    /// and the model that made it, beside the tool's own keys.
    private func modelSelectionResult(_ document: ImageDocument, entry: ModelCatalogEntry,
                                      extra: [String: Any]) throws -> String {
        var reply = extra
        reply["ok"] = true
        reply["selection_empty"] = document.canvasSelection == nil
        reply["model"] = ["id": entry.id, "version": entry.version]
        if let selection = document.canvasSelection {
            let b = selection.bounds
            reply["bounds"] = ["x": Int(b.minX), "y": Int(b.minY), "width": Int(b.width), "height": Int(b.height)]
        }
        return try jsonResult(reply)
    }

    /// The optional `refine` (default true): false skips the edge stage and keeps the model's own
    /// matte, as `object_select`'s `refine` does.
    private func refineArgument(_ a: [String: Any]) throws -> Bool {
        if a["refine"] != nil, boolArg(a, "refine") == nil { throw ToolError(message: "refine must be true or false.") }
        return boolArg(a, "refine") ?? true
    }

    /// select_sky — mirrors Select ▸ Sky.
    func selectSky(_ a: [String: Any]) throws -> String {
        let document = try target(a)
        try refuseUnknown(a, ["document_id", "mode", "feather", "refine"], in: "select_sky")
        let refine = try refineArgument(a)
        try refuseNullOrBooleanNumbers(a, ["feather"], in: "select_sky")
        let mode = try selectionMode(a), feather = try subjectFeather(a)
        guard ModelGate.ready("sky") != nil else { throw ToolError(message: ModelGate.notInstalled("sky")) }
        let capture = try SelectionCapture(document: document)
        let found = try SkySelection.shape(capture, in: document, refine: refine)
        try capture.commit(found.shape, in: document, name: "Select Sky", mode: mode, feather: feather)
        return try modelSelectionResult(document, entry: found.entry,
                                        extra: ["sky_share": (found.share * 10_000).rounded() / 10_000])
    }

    /// select_person_parts — mirrors Select ▸ Person Parts…'s Apply.
    func selectPersonParts(_ a: [String: Any]) throws -> String {
        let document = try target(a)
        try refuseUnknown(a, ["document_id", "parts", "people", "mode", "feather", "refine"],
                          in: "select_person_parts")
        let refine = try refineArgument(a)
        try refuseNullOrBooleanNumbers(a, ["feather"], in: "select_person_parts")
        let mode = try selectionMode(a), feather = try subjectFeather(a)
        let names = PersonPart.allCases.map(\.rawValue).joined(separator: ", ")
        guard let raw = a["parts"] as? [Any], !raw.isEmpty else {
            throw ToolError(message: "parts must be a nonempty list of: \(names).")
        }
        var parts: [PersonPart] = []
        for value in raw {
            guard let name = value as? String, let part = PersonPart(rawValue: name) else {
                throw ToolError(message: "Unknown person part \(value). Choose from: \(names).")
            }
            if !parts.contains(part) { parts.append(part) }
        }
        var people: [Int]?
        if let value = a["people"] {
            guard let list = value as? [Any], !list.isEmpty else {
                throw ToolError(message: "people must be a nonempty list of person numbers.")
            }
            people = try list.map { value in
                guard let parsed = doubleArg(["person": value], "person"), parsed.isFinite,
                      parsed.rounded(.towardZero) == parsed, parsed >= 1, parsed <= 10_000 else {
                    throw ToolError(message: "people must be a nonempty list of person numbers.")
                }
                return Int(parsed)
            }
        }
        guard ModelGate.ready("person_parts") != nil else {
            throw ToolError(message: ModelGate.notInstalled("person_parts"))
        }
        let capture = try SelectionCapture(document: document)
        let found = try PersonPartsRun.shape(capture, in: document, parts: parts, people: people,
                                             refine: refine)
        try capture.commit(found.shape, in: document, name: "Person Parts", mode: mode, feather: feather)
        var extra: [String: Any] = ["parts": parts.map(\.rawValue), "person_count": found.personCount]
        if let people { extra["people"] = Array(Set(people)).sorted() }
        return try modelSelectionResult(document, entry: found.entry, extra: extra)
    }

    /// object_finder_select — mirrors the Object Selection tool with Object Finder on: a click
    /// (one include point), a dragged box, and Shift-/⌥-clicks that continue the prompt.
    func objectFinderSelect(_ a: [String: Any]) throws -> String {
        let document = try target(a)
        try refuseUnknown(a, ["document_id", "points", "box", "mode", "feather", "continue", "refine"],
                          in: "object_finder_select")
        let refine = try refineArgument(a)
        try refuseNullOrBooleanNumbers(a, ["feather"], in: "object_finder_select")
        if a["continue"] != nil, boolArg(a, "continue") == nil {
            throw ToolError(message: "continue must be true or false.")
        }
        let continuing = boolArg(a, "continue") ?? false
        if continuing, a["mode"] != nil || a["feather"] != nil {
            throw ToolError(message: "mode and feather come from the prompt being continued")
        }
        let mode = try selectionMode(a), feather = try subjectFeather(a)
        var points: [ObjectFinderPoint] = []
        if let value = a["points"], !(value is NSNull) {
            guard let list = value as? [Any], list.count <= ObjectFinder.maximumPoints else {
                throw ToolError(message: "points must be a list of at most \(ObjectFinder.maximumPoints) {x, y, include} objects.")
            }
            points = try list.map { item in
                guard let point = item as? [String: Any] else {
                    throw ToolError(message: "Each point must be an object {x, y, include}.")
                }
                try refuseUnknown(point, ["x", "y", "include"], in: "object_finder_select point")
                if point["include"] != nil, boolArg(point, "include") == nil {
                    throw ToolError(message: "A point's include must be true or false.")
                }
                return ObjectFinderPoint(x: try subjectCoordinate(point, "x"), y: try subjectCoordinate(point, "y"),
                                         include: boolArg(point, "include") ?? true)
            }
        }
        var box: CGRect?
        if let value = a["box"], !(value is NSNull) {
            guard let region = value as? [String: Any] else {
                throw ToolError(message: "box requires x, y, width, height.")
            }
            try refuseNullOrBooleanNumbers(region, ["x", "y", "width", "height"], in: "object_finder_select box")
            let rect = try CGRect(x: subjectCoordinate(region, "x"), y: subjectCoordinate(region, "y"),
                                  width: subjectCoordinate(region, "width"), height: subjectCoordinate(region, "height"))
            guard rect.size.width > 0, rect.size.height > 0 else {
                throw ToolError(message: "Box width and height must be positive.")
            }
            box = rect
        }
        guard !points.isEmpty || box != nil else { throw ToolError(message: "Give points, a box, or both.") }
        if continuing, !ObjectFinder.livePrompt(document) { throw ToolError(message: ObjectFinder.noPrompt) }
        guard ModelGate.ready("object_prompt") != nil else {
            throw ToolError(message: ModelGate.notInstalled("object_prompt"))
        }
        let capture = try SelectionCapture(document: document)
        let found = try ObjectFinder.select(capture, in: document, points: points, box: box, mode: mode,
                                            feather: feather, continuing: continuing, record: .notACommand,
                                            refine: refine)
        return try modelSelectionResult(document, entry: found.entry,
                                        extra: ["score": (found.score * 10_000).rounded() / 10_000,
                                                "continued": continuing])
    }
}
