import AppKit

/// Mirrors Select > Select Objects and Select People (list/image/region
/// targeting, `model`) and All People. Instance IDs are ephemeral indices for
/// this captured composite and model.
extension AgentServer {
    /// This file's MCP tools; merged by AgentServer+Registry.swift.
    static let subjectTargetsHandlers: [String: ToolHandler] = [
        "inspect_subjects": { $0.inspectSubjects },
        "select_subjects": { $0.selectSubjects },
        "select_people": { $0.selectPeople },
    ]

    /// Facts for this file's tools (recording, selection, depth admission); merged in AgentServer+Registry.swift.
    static let subjectTargetsToolFacts: [String: ToolFacts] = [
        "inspect_subjects": [.nonRecordable, .commonTool, .floatTool, .cmykTool],
        "select_subjects": [.commonTool, .floatTool, .cmykTool],
        "select_people": [.commonTool, .floatTool, .cmykTool],
    ]

    var subjectTargetingCatalog: [[String: Any]] {
        let doc = Self.docIDProperty
        let mode: [String: Any] = ["type": "string", "enum": ["replace", "add", "subtract", "intersect"]]
        let number: [String: Any] = ["type": "number"]
        let model: [String: Any] = ["type": "string", "enum": SubjectModel.allCases.map(\.rawValue)]
        return [
            tool("inspect_subjects", "Analyze foreground objects (model \"objects\", the default) or individual people (model \"people\") without changing the selection. Reports 1-based instance IDs and coarse canvas bounds from the Vision instance map. Model \"people\" lists people numbered left to right. IDs are valid only for the same model and image revision. Refuses people when Vision's human-body and face detectors find no person. In a 32-bit float document Vision reads the tone-mapped 8-bit display at view exposure 0. Mirrors the Select Objects and Select People pickers.",
                 ["document_id": doc, "model": model]),
            tool("select_subjects", "Select multiple foreground objects (model \"objects\", the default) or individual people (model \"people\", numbered left to right). Choose instances, one point [x,y], or a region {x,y,width,height}; omit all three to select every object, or every separated person. Region hit tests the coarse instance map. IDs are valid only for the same model and image revision; supply expected_revision from inspect_subjects to reject stale IDs. A recorded instances list re-resolves on replay: Person N is the Nth from the left in the replayed image. A person's edges come from Vision's accurate person segmentation. Foreground objects may include people; use select_people for dedicated aggregate person coverage. Mirrors Select > Select Objects and Select > Select People.",
                 ["document_id": doc, "model": model,
                  "instances": ["type": "array", "items": ["type": "integer", "minimum": 1], "minItems": 1],
                  "point": ["type": "array", "items": number, "minItems": 2, "maxItems": 2],
                  "region": ["type": "object", "properties": ["x": number, "y": number, "width": number, "height": number], "required": ["x", "y", "width", "height"]],
                  "mode": mode, "feather": number, "expected_revision": ["type": "integer", "minimum": 0]]),
            tool("select_people", "Select all people as one aggregate coverage mask using Vision's dedicated accurate person segmenter. Does not identify individual people; for individuals use select_subjects with model \"people\". Refuses when Vision's human-body and face detectors find no person. In a 32-bit float document Vision reads the tone-mapped 8-bit display at view exposure 0. Mirrors Select > All People and the Select People picker's All People. One selection Undo; native pixels unchanged.",
                 ["document_id": doc, "mode": mode, "feather": number]),
        ]
    }

    func inspectSubjects(_ a: [String: Any]) throws -> String {
        let document = try target(a)
        let model = try subjectModel(a)
        let capture = try SelectionCapture(document: document)
        let analysis = try SubjectTargeting.analyze(capture, model: model, in: document)
        let subjects: [[String: Any]] = analysis.instanceBounds.enumerated().map { index, box in
            ["instance": index + 1, "bounds": ["x": Int(box.minX), "y": Int(box.minY),
                "width": Int(box.width), "height": Int(box.height)]]
        }
        return try jsonResult(["ok": true, "revision": capture.revision, "instances": subjects,
            "instance_count": analysis.instanceCount, "bounds_precision": "coarse Vision instance map",
            "model": model.rawValue, "segmenter": model.segmenter])
    }

    func subjectCoordinate(_ a: [String: Any], _ key: String) throws -> Double {
        guard let value = doubleArg(a, key), value.isFinite, abs(value) <= 100_000 else {
            throw ToolError(message: "\(key) must be a finite coordinate within ±100,000 pixels.")
        }
        return value
    }

    func selectSubjects(_ a: [String: Any]) throws -> String {
        let document = try target(a)
        let capture = try SelectionCapture(document: document)
        if a["expected_revision"] != nil {
            guard let expected = doubleArg(a, "expected_revision"), expected.isFinite,
                  expected >= 0, expected.rounded(.towardZero) == expected,
                  expected == Double(capture.revision) else {
                throw ToolError(message: "The image revision differs from inspect_subjects. Inspect the current image before choosing instance IDs.")
            }
        }
        let mode = try selectionMode(a)
        let feather = try subjectFeather(a)
        let model = try subjectModel(a)
        guard ["instances", "point", "region"].filter({ a[$0] != nil }).count <= 1 else {
            throw ToolError(message: "Choose exactly one targeting form: instances, point, or region.")
        }
        let analysis = try SubjectTargeting.analyze(capture, model: model, in: document)
        let indices: [Int]
        if let value = a["instances"] {
            guard let raw = value as? [Any], !raw.isEmpty, raw.count <= analysis.instanceCount else {
                throw ToolError(message: model.instancesRefusal)
            }
            indices = try raw.map { value in
                let parsed = try subjectCoordinate(["instance": value], "instance")
                guard parsed.rounded(.towardZero) == parsed, parsed >= 1,
                      parsed <= Double(analysis.instanceCount) else {
                    throw ToolError(message: "instance must be an integer from 1 to \(analysis.instanceCount).")
                }
                return Int(parsed)
            }
        } else if let value = a["point"] {
            guard let pair = value as? [Any], pair.count == 2 else { throw ToolError(message: "point requires [x, y].") }
            let point = try CGPoint(x: subjectCoordinate(["x": pair[0]], "x"),
                                    y: subjectCoordinate(["y": pair[1]], "y"))
            guard let found = analysis.subject(at: point) else { throw ToolError(message: model.noHitRefusal) }
            indices = [found]
        } else if let value = a["region"] {
            guard let region = value as? [String: Any] else { throw ToolError(message: "region requires x, y, width, height.") }
            let rect = try CGRect(x: subjectCoordinate(region, "x"), y: subjectCoordinate(region, "y"),
                width: subjectCoordinate(region, "width"), height: subjectCoordinate(region, "height"))
            guard rect.width > 0, rect.height > 0 else { throw ToolError(message: "Region width and height must be positive.") }
            indices = analysis.subjects(intersecting: rect)
        } else {
            indices = Array(1...analysis.instanceCount)
        }
        let wanted = Array(Set(indices)).sorted()
        let mask = try SubjectTargeting.mask(analysis, instances: wanted, capture: capture, in: document)
        try capture.commit(.mask(mask), in: document, name: model.title, mode: mode, feather: feather)
        return try jsonResult(["ok": true, "model": model.rawValue, "instances": wanted,
                               "instance_count": analysis.instanceCount,
                               "selection_empty": document.canvasSelection == nil])
    }

    func selectPeople(_ a: [String: Any]) throws -> String {
        let document = try target(a)
        let capture = try SelectionCapture(document: document)
        let mode = try selectionMode(a), feather = try subjectFeather(a)
        let mask = try SubjectTargeting.people(capture, in: document)
        try capture.commit(.mask(mask), in: document, name: "Select All People", mode: mode, feather: feather)
        return try jsonResult(["ok": true, "model": "Vision accurate person segmentation",
                               "target": "all_people", "selection_empty": document.canvasSelection == nil])
    }

    /// The `model` argument: absent is `.objects`, which is what every recorded
    /// step from before this argument means.
    func subjectModel(_ a: [String: Any]) throws -> SubjectModel {
        guard let value = a["model"] else { return .objects }
        guard let name = value as? String, let model = SubjectModel(rawValue: name) else {
            throw ToolError(message: "model must be \"objects\" or \"people\".")
        }
        return model
    }

    func subjectFeather(_ a: [String: Any]) throws -> Double {
        guard a["feather"] != nil else { return 0 }
        guard let feather = doubleArg(a, "feather"), feather.isFinite, (0...250).contains(feather) else {
            throw ToolError(message: "feather must be 0–250 pixels.")
        }
        return feather
    }
}
