import AppKit

/// add_shape_layer and edit_shape_layer: the agent mirrors of the shape
/// tools (U) — EditorViewController+Shapes' commitShapeLayer and the
/// layers panel's double-click re-edit session — so a layer made or
/// re-rendered either way is identical: pixels rendered by
/// ShapeLayer.render through the layer's transform, the description in the
/// layer's meta, the position in the layer's offset (the anchor rule in
/// DescribedLayer.swift, which is what lets Move keep the description
/// honest).
extension AgentServer {
    /// This file's MCP tools; merged by AgentServer+Registry.swift.
    static let shapesHandlers: [String: ToolHandler] = [
        "add_shape_layer": { $0.addShapeLayer },
        "edit_shape_layer": { $0.editShapeLayer },
    ]

    /// Facts for this file's tools (recording, selection, depth admission); merged in AgentServer+Registry.swift.
    static let shapesToolFacts: [String: ToolFacts] = [
        // Shape layers in a 32-bit float document; #547: in ink in a CMYK one.
        "add_shape_layer": [.typedTool, .floatTool, .cmykTool],
        "edit_shape_layer": [.typedTool, .floatTool, .cmykTool],
    ]

    /// Adds a re-editable shape layer above the active layer and selects
    /// it: the same payload → addingDescribedLayer op the shape tools'
    /// commit runs, as one undo step.
    func addShapeLayer(_ a: [String: Any]) throws -> String {
        let document = try target(a)
        guard let doc = document.doc else { throw ToolError(message: "Document has no image") }
        // #547: `fill_ink` / `stroke_ink` in a CMYK document (AgentServer+CmykEdit.swift).
        let inked = try cmykArtworkArguments(a, doc: doc, pairs: Self.shapeInkPairs)
        let a = inked.arguments
        let ink = doc.authoringInk
        let kind = try requiredString(a, "kind")
        guard ShapeLayerPayload.kinds.contains(kind) else {
            throw ToolError(message: "kind must be rect, ellipse, or line (got \"\(kind)\")")
        }
        guard let x = doubleArg(a, "x"), let y = doubleArg(a, "y"),
            let w = doubleArg(a, "w"), let h = doubleArg(a, "h"),
            x.isFinite, y.isFinite, w.isFinite, h.isFinite
        else {
            throw ToolError(
                message: "add_shape_layer requires x, y (the shape box's top-left corner in "
                    + "canvas px) and w, h (the box size), all finite numbers")
        }
        try Self.validateShapeGeometry(kind: kind, x: x, y: y, w: w, h: h)

        let fill = try artworkColor(a, numeric: "fill_rgba", legacy: "fill", fallback: nil, ink: ink)
        let stroke = try artworkColor(a, numeric: "stroke_rgba", legacy: "stroke", fallback: nil, ink: ink)
        let strokeWidth = doubleArg(a, "stroke_width") ?? 2
        let radius = doubleArg(a, "radius") ?? 0
        try Self.validateShapeStyle(
            kind: kind, fill: fill, stroke: stroke, strokeWidth: strokeWidth, radius: radius)

        let transform = try linearMapArg(a, "transform") ?? .identity
        let styles = try Self.shapeStrokeStyles(a, kind: kind, current: nil)
        let payload = ShapeLayerPayload(
            kind: kind, w: w, h: h, flipped: boolArg(a, "flipped") ?? false,
            fill: fill, stroke: stroke, strokeWidth: strokeWidth, radius: radius,
            transform: transform, ink: ink,
            strokeJoin: styles.join, strokeDash: styles.dash, strokeAlign: styles.align)
        let name = ShapeLayer.layerName(for: payload)
        // The shape box lands with its top-left at (x, y) — whole pixels for
        // an untransformed box, exact otherwise: commitShapeLayer's rule.
        let anchor = DescribedLayer.placementAnchor(CGPoint(x: x, y: y), transform: transform)

        // `position` / `layer` (AgentServer+Adjustments.insertionArguments),
        // the same pair every layer-creating tool takes. Where the new layer
        // will land is the CORE's rule, read from the stack BEFORE the edit
        // and the SAME at both depths since 12E-2b.
        let placement = try insertionArguments(a, document: document)
        let below = placement.anchor
        let landing = document.doc.map {
            placement.position.landingIndex(in: $0, anchor: below)
        } ?? below + 1
        do {
            try performArtworkEdit(document, "Add \(name) Layer") {
                try $0.nativeArtwork(.shape(payload), anchor: anchor, layer: below,
                                     replacing: false, name: name,
                                     position: placement.position, matching: inked.matches)
            }
        } catch is ToolError {
            // The op is nil exactly when the render is: name the inputs.
            throw ToolError(
                message: "Could not render the shape — the box is degenerate for the kind, "
                    + "nothing would be visible, or the raster would pass the core's 100 "
                    + "megapixel ceiling for one layer. Check w, h, stroke_width and "
                    + "transform.")
        }
        let index = min(landing, (document.doc?.layerCount ?? 1) - 1)
        document.activeLayerIndex = index
        // The edit's own notification went out before the active layer
        // moved, so the panel and status bar need this one to catch up.
        NotificationCenter.default.post(
            name: .imageDocumentImageDidChange, object: document, userInfo: ["isLive": false])
        return try jsonResult(Self.withCmykPlan([
            "ok": true, "layer": index, "name": name,
            "position": placement.position.rawValue,
            "shape": Self.shapeFields(document.doc?.shapePayload(index) ?? payload,
                                      anchor: document.doc?.describedAnchor(index)),
        ], doc: document.doc))
    }

    /// The shape tools' ink pairs: `fill_ink` beside `fill_rgba` / `fill`, `stroke_ink` beside
    /// `stroke_rgba` / `stroke`.
    static let shapeInkPairs: [(ink: String, numeric: String, legacy: String)] = [
        ("fill_ink", "fill_rgba", "fill"), ("stroke_ink", "stroke_rgba", "stroke"),
    ]

    /// `fields` with the CMYK artwork render's admitted plan (`working_memory_plan_bytes`,
    /// `scripts/cmyk-admission/`) in a CMYK document.
    static func withCmykPlan(_ fields: [String: Any], doc: RasterDocument?) -> [String: Any] {
        guard doc?.isCmyk == true else { return fields }
        var out = fields
        out["working_memory_plan_bytes"] = CmykArtworkPlan.lastBytes
        return out
    }

    /// Re-renders an existing shape layer from a changed description — the
    /// agent mirror of the layers panel's double-click re-edit session
    /// (EditorViewController+Shapes.commitShapeEditSession): pixels, offset,
    /// meta and the re-cropped mask replaced as one undo step. Omitted
    /// arguments keep the layer's current values (its transform included);
    /// the kind is fixed at creation.
    func editShapeLayer(_ a: [String: Any]) throws -> String {
        let document = try target(a)
        let index = try paintLayerIndex(a, document)
        guard let doc = document.doc else { throw ToolError(message: "Document has no image") }
        guard let info = doc.layerInfo(index) else {
            throw ToolError(message: "Layer \(index) could not be read")
        }
        let inked = try cmykArtworkArguments(a, doc: doc, pairs: Self.shapeInkPairs)
        let a = inked.arguments
        // A CMYK layer is edited on its sRGB appearance and committed back to ink (#547).
        guard let current = try doc.shapePayload(index).map({ shape -> ShapeLayerPayload in
                  guard doc.isCmyk, case let .shape(shown) = try doc.cmykDisplay(.shape(shape)) else { return shape }
                  return shown
              }),
              let currentAnchor = doc.describedAnchor(index)
        else {
            throw ToolError(
                message: "Layer \(index) (\"\(info.name)\") is not a shape layer — it is "
                    + "plain pixels with no description to re-render (get_document reports "
                    + "a \"shape\" object on the layers that have one). Make a re-editable "
                    + "shape with add_shape_layer.")
        }
        let kind = current.kind
        // Geometry: x/y name the shape box's top-left; the current one is
        // the layer's anchor (the commit's placement rule, inverted —
        // editShapeLayer's box on the UI side).
        let x = doubleArg(a, "x") ?? Double(currentAnchor.x)
        let y = doubleArg(a, "y") ?? Double(currentAnchor.y)
        let w = doubleArg(a, "w") ?? current.w
        let h = doubleArg(a, "h") ?? current.h
        guard x.isFinite, y.isFinite, w.isFinite, h.isFinite else {
            throw ToolError(message: "x, y, w and h must be finite numbers")
        }
        try Self.validateShapeGeometry(kind: kind, x: x, y: y, w: w, h: h)
        let transform = try linearMapArg(a, "transform") ?? current.transform

        // Paints: an ABSENT argument keeps the layer's paint; a present
        // empty string removes it ("" is the payload's own "none"). The
        // gate demands an actual string so JSON null or a mistyped value
        // reads as "keep" (the documented default), never as a silent
        // removal.
        let ink = doc.authoringInk
        let fill = try artworkColor(a, numeric: "fill_rgba", legacy: "fill", fallback: current.fillColor, ink: ink)
        let stroke = try artworkColor(a, numeric: "stroke_rgba", legacy: "stroke", fallback: current.strokeColor, ink: ink)
        let strokeWidth = doubleArg(a, "stroke_width") ?? current.strokeWidth
        let radius = doubleArg(a, "radius") ?? current.radius
        try Self.validateShapeStyle(
            kind: kind, fill: fill, stroke: stroke, strokeWidth: strokeWidth, radius: radius)

        let styles = try Self.shapeStrokeStyles(a, kind: kind, current: current)
        let payload = ShapeLayerPayload(
            kind: kind, w: w, h: h, flipped: boolArg(a, "flipped") ?? current.flipped,
            fill: fill, stroke: stroke, strokeWidth: strokeWidth, radius: radius,
            transform: transform, ink: ink,
            strokeJoin: styles.join, strokeDash: styles.dash, strokeAlign: styles.align)
        // A given x,y is a placement (whole pixels while untransformed, exact
        // otherwise); an omitted one keeps the exact anchor as it is.
        let anchor = a["x"] == nil && a["y"] == nil
            ? currentAnchor
            : DescribedLayer.placementAnchor(CGPoint(x: x, y: y), transform: transform)
        // A re-render REWRITES the layer's pixels, so it answers to the same
        // locks every other pixel tool does; without it a lock refusal was
        // reported as a degenerate geometry.
        try rejectLockedEdit(document, index, DR_EDIT_PIXELS)
        do {
            try performArtworkEdit(document, "Edit Shape Layer") {
                try $0.nativeArtwork(.shape(payload), anchor: anchor, layer: index, replacing: true,
                                     name: info.name, matching: inked.matches)
            }
        } catch is ToolError {
            // A transparency lock refuses a re-render that resizes the raster,
            // and it is not knowable up front — name it here rather than
            // sending the model into the geometry.
            try rejectLockedRerender(document, index)
            throw ToolError(
                message: "Could not render the shape — the box is degenerate for the kind, "
                    + "nothing would be visible, or the raster would pass the core's 100 "
                    + "megapixel ceiling for one layer. Check w, h, stroke_width and "
                    + "transform.")
        }
        return try jsonResult(Self.withCmykPlan([
            "ok": true, "layer": index,
            "shape": Self.shapeFields(document.doc?.shapePayload(index) ?? payload,
                                      anchor: document.doc?.describedAnchor(index)),
        ], doc: document.doc))
    }

    /// The shape object get_document and the shape tools report: the
    /// description, `transform` row-major (`LinearMap.array`), and `origin` —
    /// the exact canvas position of the box's top-left (fractional after a
    /// transform) — when the anchor is known.
    static func shapeFields(_ payload: ShapeLayerPayload, anchor: CGPoint?) -> [String: Any] {
        var shape: [String: Any] = [
            "kind": payload.kind,
            "w": payload.w,
            "h": payload.h,
            "fill": payload.fill,
            "stroke": payload.stroke,
            "stroke_width": payload.strokeWidth,
            "transform": payload.transform.array,
            // What the colour numbers are: linear light at 32 bits.
            "ink": payload.ink.rawValue,
        ]
        if let artwork = payload.artwork {
            shape["fill_rgba"] = artwork.fill.map { $0 as Any } ?? NSNull()
            shape["stroke_rgba"] = artwork.stroke.map { $0 as Any } ?? NSNull()
            shape["coverage"] = artwork.evaluation
            // A CMYK document's shape (#547): `fill`/`stroke` are the appearance hexes and
            // `fill_ink`/`stroke_ink` the C, M, Y, K percents beside them.
            if payload.ink == .cmyk {
                shape["fill_ink"] = artwork.fill.map { inkPercent($0) as Any } ?? NSNull()
                shape["stroke_ink"] = artwork.stroke.map { inkPercent($0) as Any } ?? NSNull()
                shape["fill_alpha"] = artwork.fill.map { ArtworkColor.alpha($0) as Any } ?? NSNull()
                shape["stroke_alpha"] = artwork.stroke.map { ArtworkColor.alpha($0) as Any } ?? NSNull()
                shape["fill_rgba"] = nil
                shape["stroke_rgba"] = nil
            }
        }
        if payload.kind == "rect" { shape["radius"] = payload.radius }
        if payload.kind == "line" { shape["flipped"] = payload.flipped }
        // The stroke styles, and the payload version they are stored at
        // (5 only when a style is off its default).
        shape["stroke_join"] = payload.strokeJoin.rawValue
        shape["stroke_dash"] = payload.strokeDash.rawValue
        shape["stroke_align"] = payload.strokeAlign.rawValue
        if let json = payload.json(), let data = json.data(using: .utf8),
           let stored = (try? JSONSerialization.jsonObject(with: data)) as? [String: Any],
           let version = stored["version"] as? Int {
            shape["version"] = version
        }
        if let anchor = anchor {
            shape["origin"] = ["x": Double(anchor.x), "y": Double(anchor.y)]
        }
        return shape
    }

    /// `stroke_join` / `stroke_dash` / `stroke_align` (the options
    /// bar's Corner style, Dash pattern and Align stroke): an absent argument
    /// keeps `current`'s style, or the default for a new shape. A line's
    /// stroke is centred, so any other `stroke_align` on one is refused.
    private static func shapeStrokeStyles(
        _ a: [String: Any], kind: String, current: ShapeLayerPayload?
    ) throws -> (join: ShapeStrokeJoin, dash: ShapeStrokeDash, align: ShapeStrokeAlign) {
        func style<T: RawRepresentable & CaseIterable>(_ key: String, _ fallback: T) throws -> T
        where T.RawValue == String {
            guard let raw = a[key] else { return fallback }
            guard let text = raw as? String, let value = T(rawValue: text) else {
                let names = T.allCases.map { $0.rawValue }.joined(separator: ", ")
                throw ToolError(message: "\(key) must be one of: \(names)")
            }
            return value
        }
        let join = try style("stroke_join", current?.strokeJoin ?? .miter)
        let dash = try style("stroke_dash", current?.strokeDash ?? .none)
        let align = try style("stroke_align", current?.strokeAlign ?? .center)
        if kind == "line", align != .center {
            throw ToolError(
                message: "A line has no inside or outside: stroke_align must be center for a line.")
        }
        return (join, dash, align)
    }

    /// The geometry rules both shape tools share: coordinates the core can
    /// address, and ShapeLayer.render's per-kind size floors (an
    /// axis-aligned line legitimately has a zero dimension).
    private static func validateShapeGeometry(
        kind: String, x: Double, y: Double, w: Double, h: Double
    ) throws {
        let int32Min = Double(Int32.min)
        let int32Max = Double(Int32.max)
        guard x >= int32Min, x <= int32Max, y >= int32Min, y <= int32Max else {
            throw ToolError(
                message: "x and y must land within the coordinates the core can address; "
                    + "keep the shape near the canvas.")
        }
        if kind == "line" {
            guard w >= 0, h >= 0, max(w, h) >= 1 else {
                throw ToolError(
                    message: "A line's box needs w and h of at least 0 px with at least one "
                        + "axis of 1 px or more (an axis-aligned line has a zero dimension).")
            }
        } else {
            guard w >= 1, h >= 1 else {
                throw ToolError(message: "\(kind) needs w and h of at least 1 px")
            }
        }
    }

    /// The style rules both shape tools share: ranges, the line's
    /// stroke-only nature, and "at least one visible paint" (a stroke of
    /// width 0 puts no ink down, so it does not count — refusing here names
    /// the fix instead of failing the render).
    private static func validateShapeStyle(
        kind: String, fill: NSColor?, stroke: NSColor?, strokeWidth: Double, radius: Double
    ) throws {
        guard strokeWidth.isFinite, strokeWidth >= 0, strokeWidth <= 200 else {
            throw ToolError(message: "stroke_width must be between 0 and 200 px")
        }
        guard radius.isFinite, radius >= 0 else {
            throw ToolError(message: "radius must be a non-negative number of px")
        }
        if kind == "line" {
            guard stroke != nil else {
                throw ToolError(
                    message: "A line is stroke-only — pass stroke (fill is ignored).")
            }
            guard strokeWidth >= 1 else {
                throw ToolError(message: "A line needs a stroke_width of at least 1 px")
            }
        }
        guard fill != nil || (stroke != nil && strokeWidth > 0) else {
            throw ToolError(
                message: "Pass at least one visible paint: fill, and/or stroke with a "
                    + "stroke_width above 0 (hex colors, #RRGGBB or #RRGGBBAA).")
        }
    }
}
