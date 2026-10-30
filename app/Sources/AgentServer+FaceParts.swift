import AppKit

/// Mirrors Select ▸ Face Parts…: `inspect_faces` is the sheet's face
/// list, `select_face_parts` its Apply. Face numbers are 1-based, left to
/// right, for this image revision.
extension AgentServer {
    /// This file's MCP tools; merged by AgentServer+Registry.swift.
    static let facePartsHandlers: [String: ToolHandler] = [
        "inspect_faces": { $0.inspectFaces },
        "select_face_parts": { $0.selectFaceParts },
    ]

    /// Facts for this file's tools (recording, selection, depth admission); merged in AgentServer+Registry.swift.
    static let facePartsToolFacts: [String: ToolFacts] = [
        "inspect_faces": [.nonRecordable, .commonTool, .floatTool, .cmykTool],
        "select_face_parts": [.commonTool, .floatTool, .cmykTool],
    ]

    var facePartsCatalog: [[String: Any]] {
        let doc = Self.docIDProperty
        let mode: [String: Any] = ["type": "string", "enum": ["replace", "add", "subtract", "intersect"]]
        let number: [String: Any] = ["type": "number"]
        let parts = FacePart.allCases.map(\.rawValue)
        return [
            tool("inspect_faces", "Find faces with Vision face landmarks without changing the selection. Reports 1-based face numbers, left to right, with canvas bounds and the parts each face has landmarks for. Numbers apply to this image revision. In a 32-bit float document Vision reads the tone-mapped 8-bit display at view exposure 0. Mirrors the Select > Face Parts picker's face list.",
                 ["document_id": doc]),
            tool("select_face_parts", "Select parts of faces as smooth anti-aliased 16-bit coverage built from Vision face landmarks: face (outline with an estimated forehead), skin (face minus eyes, brows and lips), eyes, irises, eyebrows, lips (minus the mouth opening) or mouth (the opening). Choose faces by 1-based number from inspect_faces, or one point [x,y] inside a face; omit both for every face. Refuses when no face is found. Mirrors Select > Face Parts. One selection Undo; native pixels unchanged.",
                 ["document_id": doc,
                  "parts": ["type": "array", "items": ["type": "string", "enum": parts], "minItems": 1],
                  "faces": ["type": "array", "items": ["type": "integer", "minimum": 1], "minItems": 1],
                  "point": ["type": "array", "items": number, "minItems": 2, "maxItems": 2],
                  "mode": mode, "feather": ["type": "number", "minimum": 0, "maximum": 250]],
                 required: ["parts"]),
        ]
    }

    func inspectFaces(_ a: [String: Any]) throws -> String {
        let document = try target(a)
        let capture = try SelectionCapture(document: document)
        let analysis = try SubjectTargeting.faces(capture, in: document)
        let faces: [[String: Any]] = analysis.faces.enumerated().map { index, face in
            let box = face.bounds
            return ["face": index + 1,
                    "bounds": ["x": Int(box.minX), "y": Int(box.minY),
                               "width": Int(box.width), "height": Int(box.height)],
                    "parts": face.availableParts.map(\.rawValue)]
        }
        return try jsonResult(["ok": true, "revision": capture.revision, "face_count": analysis.faces.count,
                               "faces": faces, "model": "Vision face landmarks"])
    }

    func selectFaceParts(_ a: [String: Any]) throws -> String {
        let document = try target(a)
        let capture = try SelectionCapture(document: document)
        let mode = try selectionMode(a), feather = try subjectFeather(a)
        let names = FacePart.allCases.map(\.rawValue).joined(separator: ", ")
        guard let raw = a["parts"] as? [Any], !raw.isEmpty else {
            throw ToolError(message: "parts must be a nonempty list of: \(names).")
        }
        var parts: [FacePart] = []
        for value in raw {
            guard let name = value as? String, let part = FacePart(rawValue: name) else {
                throw ToolError(message: "Unknown face part \(value). Choose from: \(names).")
            }
            if !parts.contains(part) { parts.append(part) }
        }
        guard a["faces"] == nil || a["point"] == nil else {
            throw ToolError(message: "Choose faces or point, not both.")
        }
        // The list's shape is checked before Vision runs; only the range
        // (1…count) waits for the analysis.
        var requested: [Double]?
        if let value = a["faces"] {
            guard let list = value as? [Any], !list.isEmpty else {
                throw ToolError(message: "faces must be a nonempty list of face numbers.")
            }
            requested = try list.map { value in
                guard let parsed = doubleArg(["face": value], "face"), parsed.isFinite,
                      parsed.rounded(.towardZero) == parsed, parsed >= 1 else {
                    throw ToolError(message: "faces must be a nonempty list of face numbers.")
                }
                return parsed
            }
        }
        let analysis = try SubjectTargeting.faces(capture, in: document)
        let count = analysis.faces.count
        let chosen: [Int]
        if let requested {
            guard requested.allSatisfy({ $0 <= Double(count) }) else {
                throw ToolError(message: "face must be an integer from 1 to \(count).")
            }
            chosen = requested.map { Int($0) }
        } else if let value = a["point"] {
            guard let pair = value as? [Any], pair.count == 2 else { throw ToolError(message: "point requires [x, y].") }
            let point = try CGPoint(x: subjectCoordinate(["x": pair[0]], "x"),
                                    y: subjectCoordinate(["y": pair[1]], "y"))
            guard let found = analysis.face(at: point) else { throw ToolError(message: "No face at this point.") }
            chosen = [found]
        } else {
            chosen = Array(1...count)
        }
        let wanted = Array(Set(chosen)).sorted()
        let mask = try SubjectTargeting.faceMask(analysis, parts: parts, faces: wanted, capture: capture, in: document)
        try capture.commit(.mask16(mask), in: document, name: "Select Face Parts", mode: mode, feather: feather)
        return try jsonResult(["ok": true, "parts": parts.map(\.rawValue), "faces": wanted, "face_count": count,
                               "selection_empty": document.canvasSelection == nil, "coverage_precision": "u16"])
    }
}
