import AppKit

/// The MCP twins of the Ruler's Straighten Layer and of Perspective
/// Crop, Image ▸ Trim… and Image ▸ Reveal All: `straighten_layers`,
/// `perspective_crop`, `trim` and `reveal_canvas`. Each is one undo step, and
/// each answers a call that would change nothing as the house `changed:
/// false` no-op rather than an error.
extension AgentServer {
    /// This file's MCP tools; merged by AgentServer+Registry.swift.
    static let canvasFramingHandlers: [String: ToolHandler] = [
        "straighten_layers": { $0.straightenLayers },
        "perspective_crop": { $0.perspectiveCrop },
        "trim": { $0.trim },
        "reveal_canvas": { $0.revealCanvas },
    ]

    /// Facts for this file's tools (recording, selection, depth admission); merged in AgentServer+Registry.swift.
    static let canvasFramingToolFacts: [String: ToolFacts] = [
        // All four run at 8 and 16 bits and in 32-bit float: the layer
        // transform and the perspective resample are the core's filtered
        // geometry, Trim is a crop and Reveal All a canvas resize.
        "straighten_layers": [.typedTool, .floatTool, .cmykTool],
        "perspective_crop": [.typedTool, .floatTool, .cmykTool],
        "trim": [.typedTool, .floatTool, .cmykTool],
        "reveal_canvas": [.typedTool, .floatTool, .cmykTool],
    ]

    /// `straighten_layers` — mirrors the Ruler tool's Straighten Layer
    /// (EditorViewController+Ruler.swift): the line (x0, y0)–(x1, y1) gives
    /// the smallest rotation that lays it along an axis
    /// (`RulerMeasurement.straightenMatrix`, about the canvas centre), applied
    /// to `layers` (default: the selected layers) through `transform_layers`'
    /// own commit.
    func straightenLayers(_ a: [String: Any]) throws -> String {
        let document = try target(a)
        guard let doc = document.doc else { throw ToolError(message: "Document has no image") }
        guard let x0 = doubleArg(a, "x0"), let y0 = doubleArg(a, "y0"),
              let x1 = doubleArg(a, "x1"), let y1 = doubleArg(a, "y1"),
              [x0, y0, x1, y1].allSatisfy(\.isFinite)
        else {
            throw ToolError(message: "straighten_layers requires finite x0, y0, x1 and y1 "
                + "(the two ends of the line that should be level or plumb, canvas px).")
        }
        let line = RulerMeasurement(start: CGPoint(x: x0, y: y0), end: CGPoint(x: x1, y: y1))
        // #1 Bitmap phase 3: the straightened crop (`straightenBitmapWithRuler`'s twin): `crop`
        // at the window `BitmapDocument.straightenWindow` fits and the line's angle.
        if doc.isBitmap {
            guard let window = BitmapDocument.straightenWindow(line, width: doc.width,
                                                               height: doc.height) else {
                return try structureNoOp(
                    ["rotation": 0],
                    why: line.length == 0 ? "the line has no length."
                        : "the line already lies along an axis.")
            }
            var arguments: [String: Any] = [
                "document_id": documentID(for: document),
                "x": Int(window.rect.minX), "y": Int(window.rect.minY),
                "width": Int(window.rect.width), "height": Int(window.rect.height),
                "angle": window.angle,
            ]
            if let sampler = a["sampler"] { arguments["sampler"] = sampler }
            let reply = try crop(arguments)
            guard let data = reply.data(using: .utf8),
                  let result = (try? JSONSerialization.jsonObject(with: data)) as? [String: Any],
                  let content = result["content"] as? [[String: Any]],
                  let text = content.first?["text"] as? String, let inner = text.data(using: .utf8),
                  var fields = (try? JSONSerialization.jsonObject(with: inner)) as? [String: Any]
            else { return reply }
            fields["rotation"] = window.angle
            return try jsonResult(fields)
        }
        guard let rotation = line.straightenRotation(),
              let matrix = line.straightenMatrix(canvasWidth: doc.width, canvasHeight: doc.height)
        else {
            return try structureNoOp(
                ["rotation": 0],
                why: line.length == 0 ? "the line has no length."
                    : "the line already lies along an axis.")
        }
        var arguments: [String: Any] = [
            "document_id": documentID(for: document),
            "matrix": [matrix.a, matrix.b, matrix.c, matrix.d, matrix.tx, matrix.ty].map(Double.init),
            "layers": a["layers"] ?? document.selectedLayerIndices,
        ]
        if let sampler = a["sampler"] { arguments["sampler"] = sampler }
        // `transform_layers`' reply is a finished tool result: its fields are
        // the JSON text inside it.
        let reply = try transformLayersSet(arguments)
        guard let data = reply.data(using: .utf8),
              let result = (try? JSONSerialization.jsonObject(with: data)) as? [String: Any],
              let content = result["content"] as? [[String: Any]],
              let text = content.first?["text"] as? String, let inner = text.data(using: .utf8),
              var fields = (try? JSONSerialization.jsonObject(with: inner)) as? [String: Any]
        else { return reply }
        // Counter-clockwise on screen, in degrees: what the layers turned by.
        fields["rotation"] = rotation
        return try jsonResult(fields)
    }

    /// `perspective_crop` — mirrors the Perspective Crop tool's commit
    /// (EditorViewController+PerspectiveCrop.swift): the quadrilateral
    /// `corners` (TL, TR, BR, BL, canvas px) becomes the whole canvas at
    /// `width` × `height` (default: `PerspectiveCropRequest.autoSize`).
    /// Described layers need `rasterize_artwork`, the tool's prompt.
    func perspectiveCrop(_ a: [String: Any]) throws -> String {
        let document = try target(a)
        guard let doc = document.doc else { throw ToolError(message: "Document has no image") }
        guard let rows = a["corners"] as? [Any], rows.count == 4 else {
            throw ToolError(message: "perspective_crop requires corners: four [x, y] points "
                + "in the order top-left, top-right, bottom-right, bottom-left.")
        }
        let corners = try rows.map { row -> CGPoint in
            guard let pair = row as? [Any], pair.count == 2,
                  let x = Self.finiteNumber(pair[0]), let y = Self.finiteNumber(pair[1])
            else { throw ToolError(message: "Each corner must be two finite numbers, [x, y].") }
            return CGPoint(x: x, y: y)
        }
        let automatic = PerspectiveCropRequest.autoSize(corners)
        let width = intArg(a, "width") ?? automatic.width
        let height = intArg(a, "height") ?? automatic.height
        guard width > 0, height > 0 else {
            throw ToolError(message: "width and height must each be at least 1 px.")
        }
        let samplerName = (stringArg(a, "sampler") ?? "bicubic").lowercased()
        guard let sampler = Self.transformSamplers[samplerName] else {
            throw ToolError(
                message: "sampler must be nearest, bilinear, bicubic or lanczos "
                    + "(got \"\(samplerName)\")")
        }
        let described = (0..<doc.layerCount).filter { document.layerDescribesSource($0) }
        if !described.isEmpty, boolArg(a, "rasterize_artwork") != true {
            let list = described.map(String.init).joined(separator: ", ")
            throw ToolError(
                message: "Perspective Crop resamples every layer, which rasterizes text "
                    + "and shape layers (layer\(described.count == 1 ? "" : "s") \(list)); "
                    + "pass rasterize_artwork: true to authorize it.")
        }
        let request = PerspectiveCropRequest(
            corners: corners, width: width, height: height, sampler: sampler.filter)
        // The `crop` handler's pattern: one job that also prepares the
        // display, published through `preparedEdit`; a core refusal is
        // reported with its own sentence, and nil without one is the whole
        // canvas at its own size.
        var failure: Error?
        var unchanged = false
        defer { document.preparedEdit = nil }
        do {
            try performGroupedEdit(document, "Perspective Crop", changesCanvasGeometry: true) { base in
                do {
                    guard let prepared = try base.perspectiveCropped(request, described: described)
                    else {
                        unchanged = true
                        return nil
                    }
                    document.preparedEdit = prepared
                    return prepared.document
                } catch {
                    failure = error
                    return nil
                }
            }
        } catch {
            if let failure { throw ToolError(message: failure.localizedDescription) }
            if unchanged {
                return try structureNoOp(
                    ["document": summary(document)],
                    why: "the corners are the whole \(doc.width) × \(doc.height) canvas at its "
                        + "own size.")
            }
            throw error
        }
        return try jsonResult([
            "ok": true, "width": width, "height": height, "sampler": sampler.name,
            "rasterized_layers": described, "document": summary(document),
        ])
    }

    /// `trim` — mirrors Image ▸ Trim… (EditorViewController+CanvasFraming):
    /// crops the canvas to what is left once the chosen edges move inward
    /// past every row and column that is transparent or matches a corner
    /// pixel.
    func trim(_ a: [String: Any]) throws -> String {
        let document = try target(a)
        guard let doc = document.doc else { throw ToolError(message: "Document has no image") }
        let name = stringArg(a, "based_on") ?? TrimBasis.transparent.agentName
        guard let basis = TrimBasis(agentName: name) else {
            let names = TrimBasis.allCases.map(\.agentName).joined(separator: ", ")
            throw ToolError(message: "based_on must be one of: \(names) (got \"\(name)\")")
        }
        var edges: TrimEdges = []
        let named: [(String, TrimEdges)] = [
            ("top", .top), ("bottom", .bottom), ("left", .left), ("right", .right),
        ]
        for (key, edge) in named where boolArg(a, key) ?? true { edges.insert(edge) }
        let kept: CGRect?
        do {
            kept = try document.trimRect(basis: basis, edges: edges)
        } catch {
            throw ToolError(message: error.localizedDescription)
        }
        let fields: [String: Any] = CanvasFraming.trimArguments(basis: basis, edges: edges)
        guard let rect = kept else {
            return try structureNoOp(
                fields, why: "every pixel would be trimmed away, so nothing would be left.")
        }
        guard rect != CGRect(x: 0, y: 0, width: doc.width, height: doc.height) else {
            return try structureNoOp(fields, why: "none of the chosen edges has anything to trim.")
        }
        try performGroupedEdit(
            document, "Trim", changesCanvasGeometry: true,
            resultingSelection: document.selectionCarried(
                croppedToX: Int(rect.minX), y: Int(rect.minY),
                width: Int(rect.width), height: Int(rect.height))
        ) {
            try $0.cropped(
                x: Int(rect.minX), y: Int(rect.minY), w: Int(rect.width), h: Int(rect.height))
        }
        return try jsonResult(fields.merging([
            "ok": true,
            "kept": ["x": Int(rect.minX), "y": Int(rect.minY),
                     "width": Int(rect.width), "height": Int(rect.height)],
            "document": summary(document),
        ]) { _, new in new })
    }

    /// `reveal_canvas` — mirrors Image ▸ Reveal All: grows the canvas until
    /// it holds every layer's pixels.
    func revealCanvas(_ a: [String: Any]) throws -> String {
        let document = try target(a)
        guard let doc = document.doc else { throw ToolError(message: "Document has no image") }
        guard let rect = doc.revealAllRect() else {
            return try structureNoOp(
                ["document": summary(document)],
                why: "no layer has pixels outside the \(doc.width) × \(doc.height) canvas.")
        }
        let width = Int(rect.width), height = Int(rect.height)
        try requireChannelBudget(document, width: width, height: height)
        try performGroupedEdit(
            document, "Reveal All", changesCanvasGeometry: true,
            resultingSelection: document.selectionCarried(
                canvasWidth: width, height: height,
                originX: Int(-rect.minX), originY: Int(-rect.minY))
        ) {
            try $0.canvasResized(
                w: width, h: height, originX: Int(-rect.minX), originY: Int(-rect.minY))
        }
        return try jsonResult([
            "ok": true,
            // Where the old canvas's top-left sits in the new one.
            "origin": ["x": Int(-rect.minX), "y": Int(-rect.minY)],
            "document": summary(document),
        ])
    }
}
