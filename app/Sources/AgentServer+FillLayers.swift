import AppKit

/// add_fill_layer and edit_fill_layer: the agent mirrors of Layer ▸
/// New Fill Layer ▸ Solid Color… / Gradient… / Pattern… and of the
/// Layers panel's double-click on a fill layer and Layer ▸ Layer Content
/// Options… (EditorViewController+FillLayers.swift). A
/// layer made or re-rendered either way is identical: the description in the
/// layer's meta, the pixels rendered from it by the core over the whole
/// canvas (an aligned gradient over its mask box) at the document's
/// depth (FillLayer.swift).
///
/// The shape tools' handlers are the template (`AgentServer+Shapes.swift`):
/// `insertionArguments` for where a new layer lands, `performArtworkEdit` for
/// the one undo step, `rejectLockedEdit` before a re-render.
extension AgentServer {
    /// This file's MCP tools; merged by AgentServer+Registry.swift.
    static let fillLayersHandlers: [String: ToolHandler] = [
        "add_fill_layer": { $0.addFillLayer },
        "edit_fill_layer": { $0.editFillLayer },
    ]

    /// Facts for this file's tools (recording, selection, depth admission); merged in AgentServer+Registry.swift.
    static let fillLayersToolFacts: [String: ToolFacts] = [
        // Fill layers render at every depth, 32-bit float included, and in ink in a CMYK
        // document (#547).
        "add_fill_layer": [.typedTool, .floatTool, .cmykTool],
        "edit_fill_layer": [.typedTool, .floatTool, .cmykTool],
    ]

    /// Adds a Solid Color, Gradient or Pattern fill layer at the slot `position`
    /// names relative to `layer` (default: above the active layer) and
    /// selects it, as one undo step. Unlike the menu command it adds no mask
    /// from the selection: `add_layer_mask` is its own tool.
    func addFillLayer(_ a: [String: Any]) throws -> String {
        let document = try target(a)
        try refuseNullOrBooleanNumbers(a, ["layer"], in: "add_fill_layer")
        guard let doc = document.doc else { throw ToolError(message: "Document has no image") }
        let inked = try fillInkArguments(a, doc: doc)
        let a = inked.arguments
        let kind = try requiredString(a, "kind")
        let canvas = (doc.width, doc.height)
        let payload: FillLayerPayload
        switch kind {
        case FillLayerPayload.solid:
            guard a["gradient"] == nil, a["pattern"] == nil else {
                throw ToolError(message: "A solid fill takes color, not gradient or pattern.")
            }
            payload = FillLayerPayload(
                color: try fillColor(a, current: nil), canvas: canvas)
        case FillLayerPayload.gradientKind:
            guard a["color"] == nil, a["pattern"] == nil else {
                throw ToolError(message: "A gradient fill takes gradient, not color or pattern.")
            }
            payload = FillLayerPayload(
                gradient: try fillGradient(a["gradient"], current: nil),
                canvas: canvas)
        case FillLayerPayload.patternKind:
            guard a["color"] == nil, a["gradient"] == nil else {
                throw ToolError(message: "A pattern fill takes pattern, not color or gradient.")
            }
            payload = FillLayerPayload(
                pattern: try fillPattern(a["pattern"], current: nil), canvas: canvas)
        default:
            throw ToolError(message: "kind must be solid, gradient or pattern (got \"\(kind)\")")
        }
        let name = stringArg(a, "name") ?? FillLayer.layerName(for: payload)
        let placement = try insertionArguments(a, document: document)
        let below = placement.anchor
        let landing = placement.position.landingIndex(in: doc, anchor: below)
        try performArtworkEdit(document, "New \(FillLayer.layerName(for: payload)) Layer") {
            try $0.nativeArtwork(.fill(payload), anchor: .zero, layer: below,
                                 replacing: false, name: name, position: placement.position,
                                 matching: inked.matches)
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
            "fill": Self.fillFields(document.doc?.fillPayload(index) ?? payload),
        ], doc: document.doc))
    }

    /// The fill tools' ink arguments in a CMYK document (#547, AgentServer+CmykEdit.swift): a
    /// solid `ink` becomes the colour's exact appearance numbers (`__appearance`, read by
    /// `fillColor` only), each `gradient.stops[].ink` its stop's appearance hex; `a` as it is in an
    /// RGB document.
    private func fillInkArguments(_ a: [String: Any], doc: RasterDocument) throws
        -> (arguments: [String: Any], matches: [CmykInkMatch]) {
        var (out, matches) = try cmykArtworkArguments(a, doc: doc, pairs: [("ink", "__appearance", "color")])
        if let gradient = a["gradient"] as? [String: Any] {
            let stops = try cmykArtworkArguments(gradient, doc: doc, pairs: [])
            out["gradient"] = stops.arguments
            matches += stops.matches
        }
        return (out, matches)
    }

    /// Re-renders an existing fill layer from a changed colour or gradient —
    /// the mirror of the dialog reopened from the Layers panel — as one undo
    /// step. An omitted argument keeps the layer's value; inside `gradient`
    /// an omitted key keeps the gradient's. The kind is fixed at creation.
    func editFillLayer(_ a: [String: Any]) throws -> String {
        let document = try target(a)
        try refuseNullOrBooleanNumbers(a, ["layer"], in: "edit_fill_layer")
        let index = try paintLayerIndex(a, document)
        guard let doc = document.doc else { throw ToolError(message: "Document has no image") }
        guard let info = doc.layerInfo(index) else {
            throw ToolError(message: "Layer \(index) could not be read")
        }
        let inked = try fillInkArguments(a, doc: doc)
        let a = inked.arguments
        // A CMYK layer is edited on its sRGB appearance and committed back to ink (#547): an
        // unchanged colour or stop keeps its ink exactly (`cmykArtwork`).
        guard var payload = try doc.fillPayload(index).map({ fill -> FillLayerPayload in
            guard doc.isCmyk, case let .fill(shown) = try doc.cmykDisplay(.fill(fill)) else { return fill }
            return shown
        }) else {
            throw ToolError(
                message: "Layer \(index) (\"\(info.name)\") is not a fill layer (get_document "
                    + "reports a \"fill\" object on the layers that are). Make one with "
                    + "add_fill_layer.")
        }
        if payload.patternFill != nil {
            guard a["color"] == nil, a["gradient"] == nil else {
                throw ToolError(message: "Layer \(index) is a pattern fill: pass pattern, not "
                    + "color or gradient.")
            }
            payload.patternFill = try fillPattern(a["pattern"], current: payload.patternFill)
        } else if payload.gradient != nil {
            guard a["color"] == nil, a["pattern"] == nil else {
                throw ToolError(message: "Layer \(index) is a gradient fill: pass gradient, not "
                    + "color or pattern.")
            }
            payload.gradient = try fillGradient(a["gradient"], current: payload.gradient)
        } else {
            guard a["gradient"] == nil, a["pattern"] == nil else {
                throw ToolError(message: "Layer \(index) is a solid fill: pass color, not "
                    + "gradient or pattern.")
            }
            payload.color = try fillColor(a, current: payload.color)
        }
        // A re-render REWRITES the layer's pixels, so it answers to the same
        // locks every other pixel tool does.
        try rejectLockedEdit(document, index, DR_EDIT_PIXELS)
        let changed = try performArtworkEdit(document, "Edit Fill Layer") {
            try $0.nativeArtwork(.fill(payload), anchor: .zero, layer: index,
                                 replacing: true, name: info.name, matching: inked.matches)
        }
        var result: [String: Any] = Self.withCmykPlan([
            "ok": true, "layer": index, "changed": changed,
            "fill": Self.fillFields(document.doc?.fillPayload(index) ?? payload),
        ], doc: document.doc)
        if !changed {
            result["note"] = "Nothing changed: the layer already had that fill. "
                + "No undo step was added."
        }
        return try jsonResult(result)
    }

    /// The `fill` object get_document and the fill tools report: the kind,
    /// the canvas the pixels cover, and the colour (`#RRGGBBAA` and its four
    /// numbers), the gradient, or the pattern (its tile as `{preset}` or
    /// `{name, width, height}`, no `rgba`), in `add_fill_layer`'s own spelling.
    static func fillFields(_ payload: FillLayerPayload) -> [String: Any] {
        var fill: [String: Any] = [
            "kind": payload.fill,
            "canvas": ["width": payload.canvas[0], "height": payload.canvas[1]],
            "color_space": "srgb",
        ]
        if let color = payload.color {
            fill["color"] = TextLayer.hex(numbers: color)
            fill["rgba"] = color
        }
        if let gradient = payload.gradient {
            fill["gradient"] = Self.fillGradientArguments(gradient)
        }
        if let pattern = payload.patternFill {
            fill["pattern"] = Self.fillPatternArguments(pattern, tile: false)
        }
        // A CMYK document's fill (#547): the colour is ink, reported in percent with its alpha;
        // each stop's `color` is its ink's appearance and `ink` its percents.
        if payload.ink == .cmyk {
            fill["color_space"] = "cmyk"
            if let color = payload.color {
                fill["ink"] = inkPercent(color)
                fill["alpha"] = ArtworkColor.alpha(color)
                fill["color"] = nil
                fill["rgba"] = nil
            }
            if let gradient = payload.gradient, let inks = payload.stopInks {
                fill["gradient"] = Self.fillGradientArguments(gradient, inks: inks)
            }
        }
        return fill
    }

    /// A gradient as the `gradient` argument spells it; with `inks` (a CMYK fill's `stop_inks`,
    /// #547) each stop carries its `ink` in percent too. `smoothness` (1 when unset) and
    /// `align_with_layer` are always written, so a recorded step replays the value it had
    /// even when that is the default; a stop's `midpoint` and `opacity_midpoint` only when
    /// not 0.5, so a gradient without them records as it always did.
    static func fillGradientArguments(_ gradient: GradientFill, inks: [[Double]]? = nil) -> [String: Any] {
        [
            "stops": gradient.stops.enumerated().map { index, stop in
                var row: [String: Any] = ["position": stop.position, "color": stop.color, "opacity": stop.opacity]
                if let inks, index < inks.count { row["ink"] = inkPercent(inks[index]) }
                if let midpoint = stop.midpoint, midpoint != 0.5 { row["midpoint"] = midpoint }
                if let midpoint = stop.opacityMidpoint, midpoint != 0.5 {
                    row["opacity_midpoint"] = midpoint
                }
                return row
            },
            "style": gradient.style, "angle": gradient.angle, "scale": gradient.scale,
            "reverse": gradient.reverse, "smoothness": gradient.smoothness ?? 1,
            "align_with_layer": gradient.alignWithLayer,
        ]
    }

    /// A pattern fill as the `pattern` argument spells it: the tile as
    /// `{preset}` or `{name, width, height}` plus its `rgba` when `tile` (a
    /// recorded step carries the pixels; a report does not), then angle,
    /// scale, link_with_layer and phase.
    static func fillPatternArguments(_ fill: PatternFill, tile: Bool) -> [String: Any] {
        var pattern: [String: Any] = [:]
        if let ref = fill.pattern {
            if let preset = ref.preset {
                pattern["preset"] = preset
            } else {
                pattern["name"] = ref.name ?? "Custom"
                pattern["width"] = ref.width ?? 0
                pattern["height"] = ref.height ?? 0
                if tile { pattern["rgba"] = ref.rgba ?? "" }
            }
        }
        return [
            "pattern": pattern, "angle": fill.angle, "scale": fill.scale,
            "link_with_layer": fill.linkWithLayer, "phase": fill.phase,
        ]
    }

    /// `pattern` as a `PatternFill`: `pattern` (`{preset}` — one of
    /// `RasterDocument.patternPresets()` — or a custom tile `{name?, width,
    /// height, rgba}`, sides 1…512, `rgba` standard base64 of width × height ×
    /// 4 straight RGBA8 bytes), `angle` in degrees, `scale` 0.01…10,
    /// `link_with_layer` and `phase` [x, y] px. An absent key keeps
    /// `current`'s value, or the default for a new fill (checkerboard, angle 0,
    /// scale 1, linked, phase [0, 0]); an unknown key, `anchor` included, is
    /// refused by name. The anchor is never set: placement follows the
    /// layer's reference point.
    private func fillPattern(_ raw: Any?, current: PatternFill?) throws -> PatternFill {
        let base = current ?? PatternFill(pattern: PatternRef(preset: "checkerboard"))
        guard let raw else { return base }
        guard let object = raw as? [String: Any] else {
            throw ToolError(message: "pattern must be an object with pattern, angle, scale, "
                + "link_with_layer and phase.")
        }
        try refuseNullOrBooleanNumbers(object, ["angle", "scale"], in: "pattern")
        let known: Set<String> = ["pattern", "angle", "scale", "link_with_layer", "phase"]
        if let stray = object.keys.sorted().first(where: { !known.contains($0) }) {
            throw ToolError(message: "pattern has no key \"\(stray)\"; its keys are pattern, "
                + "angle, scale, link_with_layer and phase.")
        }
        var fill = base
        fill.anchor = nil
        if let tile = object["pattern"] {
            fill.pattern = try fillPatternTile(tile)
        }
        if object["angle"] != nil {
            guard let value = doubleArg(object, "angle"), value.isFinite else {
                throw ToolError(message: "pattern.angle must be a finite number of degrees")
            }
            fill.angle = value
        }
        if object["scale"] != nil {
            guard let value = doubleArg(object, "scale"), (0.01...10).contains(value) else {
                throw ToolError(message: "pattern.scale must be between 0.01 and 10")
            }
            fill.scale = value
        }
        if object["link_with_layer"] != nil {
            guard let value = boolArg(object, "link_with_layer") else {
                throw ToolError(message: "pattern.link_with_layer must be true or false")
            }
            fill.linkWithLayer = value
        }
        if let phase = object["phase"] {
            guard let pair = phase as? [Any], pair.count == 2,
                  let x = (pair[0] as? NSNumber)?.doubleValue, x.isFinite,
                  let y = (pair[1] as? NSNumber)?.doubleValue, y.isFinite,
                  !Self.isJSONBoolean(pair[0]), !Self.isJSONBoolean(pair[1])
            else {
                throw ToolError(message: "pattern.phase must be an array of 2 finite numbers [x, y]")
            }
            fill.phase = [x, y]
        }
        return fill
    }

    /// `pattern.pattern`: `{preset}` or `{name?, width, height, rgba}`.
    private func fillPatternTile(_ raw: Any) throws -> PatternRef {
        let ids = RasterDocument.patternPresets().map(\.id)
        guard let tile = raw as? [String: Any] else {
            throw ToolError(message: "pattern.pattern must be {preset} or {name?, width, height, "
                + "rgba}")
        }
        if tile["preset"] != nil {
            guard tile.count == 1, let preset = tile["preset"] as? String, ids.contains(preset) else {
                throw ToolError(message: "pattern.pattern.preset must be one of "
                    + ids.joined(separator: ", ") + ", alone")
            }
            return PatternRef(preset: preset)
        }
        let known: Set<String> = ["name", "width", "height", "rgba"]
        if let stray = tile.keys.sorted().first(where: { !known.contains($0) }) {
            throw ToolError(message: "pattern.pattern has no key \"\(stray)\"; a custom tile's "
                + "keys are name, width, height and rgba.")
        }
        try refuseNullOrBooleanNumbers(tile, ["width", "height"], in: "pattern.pattern")
        guard let width = intArg(tile, "width"), let height = intArg(tile, "height"),
              (1...512).contains(width), (1...512).contains(height)
        else {
            throw ToolError(message: "pattern.pattern.width and height must be integers 1…512")
        }
        guard let rgba = tile["rgba"] as? String,
              Data(base64Encoded: rgba)?.count == width * height * 4
        else {
            throw ToolError(message: "pattern.pattern.rgba must be standard base64 of width × "
                + "height × 4 straight RGBA8 bytes")
        }
        if tile["name"] != nil, !(tile["name"] is String) {
            throw ToolError(message: "pattern.pattern.name must be a string")
        }
        return PatternRef(name: (tile["name"] as? String) ?? "Custom", width: width,
                          height: height, rgba: rgba)
    }

    /// `color` as authored sRGB `[r, g, b, a]`: a hex like #RRGGBB or
    /// #RRGGBBAA. Absent keeps `current`; a new solid fill requires it.
    private func fillColor(_ a: [String: Any], current: [Double]?) throws -> [Double] {
        // An `ink`'s exact appearance (`fillInkArguments`).
        if let appearance = a["__appearance"] as? [Double], appearance.count == 4 { return appearance }
        guard a["color"] != nil else {
            if let current { return current }
            throw ToolError(message: "A solid fill requires color, a hex like #RRGGBB or #RRGGBBAA.")
        }
        guard let rgba = ArtworkColor.rgba(try parseColor(a, "color", fallback: .black)) else {
            throw ToolError(message: "Could not convert the color")
        }
        return rgba
    }

    /// `gradient` as a `GradientFill`: `stops` (2 to 32 `{position, color,
    /// opacity?, midpoint?, opacity_midpoint?}`, each position 0…1, each
    /// midpoint 0.05…0.95), `style` (linear, radial, angle,
    /// reflected, diamond), `angle` in degrees, `scale` (0.1…1.5),
    /// `reverse`, `smoothness` (0…1) and `align_with_layer`. An
    /// absent key keeps `current`'s value, or the default for a new gradient
    /// (linear, 90°, scale 1, smoothness 1, not aligned); a new gradient
    /// requires stops.
    private func fillGradient(_ raw: Any?, current: GradientFill?) throws -> GradientFill {
        guard let raw else {
            if let current { return current }
            throw ToolError(message: "A gradient fill requires gradient: {stops: [{position, "
                + "color, opacity?}, …], style?, angle?, scale?, reverse?, smoothness?, "
                + "align_with_layer?}.")
        }
        guard let object = raw as? [String: Any] else {
            throw ToolError(message: "gradient must be an object with stops, style, angle, "
                + "scale, reverse, smoothness and align_with_layer.")
        }
        try refuseNullOrBooleanNumbers(object, ["angle", "scale", "smoothness"], in: "gradient")
        let known: Set<String> = [
            "stops", "style", "angle", "scale", "reverse", "smoothness", "align_with_layer",
        ]
        if let stray = object.keys.sorted().first(where: { !known.contains($0) }) {
            throw ToolError(message: "gradient has no key \"\(stray)\"; its keys are stops, "
                + "style, angle, scale, reverse, smoothness and align_with_layer.")
        }
        var gradient = current ?? GradientFill()
        // A new fill spans the canvas at Smoothness 100 % until told otherwise.
        if current == nil {
            gradient.alignWithLayer = false
            gradient.smoothness = nil
        }
        if let list = object["stops"] {
            guard let entries = list as? [[String: Any]],
                  (GradientEditorView.minStops...GradientEditorView.maxStops).contains(entries.count)
            else {
                throw ToolError(message: "gradient.stops must be an array of 2 to 32 "
                    + "{position, color, opacity?} objects")
            }
            gradient.stops = try entries.enumerated().map { index, entry in
                try refuseNullOrBooleanNumbers(
                    entry, ["position", "opacity"], in: "gradient.stops[\(index)]")
                // Numbers go through the argument helpers, so a string form
                // reads as it does in `gradient`'s stops.
                guard !Self.isJSONBoolean(entry["position"] as Any),
                      let position = doubleArg(entry, "position"),
                      position.isFinite, (0...1).contains(position)
                else {
                    throw ToolError(message: "gradient.stops[\(index)].position must be 0…1")
                }
                // Absent is opaque; present but unreadable is refused, never
                // taken as 1.
                var opacity = 1.0
                if let given = entry["opacity"], !(given is NSNull) {
                    guard let value = doubleArg(entry, "opacity"), value.isFinite,
                          (0...1).contains(value)
                    else {
                        throw ToolError(
                            message: "gradient.stops[\(index)].opacity must be 0…1")
                    }
                    opacity = value
                }
                guard entry["color"] is String else {
                    throw ToolError(message: "gradient.stops[\(index)] requires color, a hex "
                        + "like #RRGGBB")
                }
                let color = try parseColor(entry, "color", fallback: .black)
                // Midpoints: stored nil at 0.5, as the UI stores them.
                let midpoint = try stopMidpoint(entry, at: "gradient.stops[\(index)]")
                // A hex's own alpha scales the stop's opacity.
                return GradientStop(
                    position: position, color: LayerStyleColor.hex(color),
                    opacity: opacity * Double(color.alphaComponent),
                    midpoint: midpoint.color == 0.5 ? nil : midpoint.color,
                    opacityMidpoint: midpoint.opacity == 0.5 ? nil : midpoint.opacity)
            }
        } else if current == nil {
            throw ToolError(message: "A new gradient fill requires gradient.stops.")
        }
        if let style = object["style"] {
            guard let name = style as? String, GradientFill.styles.contains(name) else {
                throw ToolError(message: "gradient.style must be one of: "
                    + GradientFill.styles.joined(separator: ", "))
            }
            gradient.style = name
        } else if current == nil {
            gradient.style = "linear"
        }
        if object["angle"] != nil {
            guard let value = doubleArg(object, "angle"), value.isFinite else {
                throw ToolError(message: "gradient.angle must be a finite number of degrees")
            }
            gradient.angle = value
        }
        if object["scale"] != nil {
            guard let value = doubleArg(object, "scale"), (0.1...1.5).contains(value) else {
                throw ToolError(message: "gradient.scale must be between 0.1 and 1.5")
            }
            gradient.scale = value
        }
        if object["reverse"] != nil {
            guard let value = boolArg(object, "reverse") else {
                throw ToolError(message: "gradient.reverse must be true or false")
            }
            gradient.reverse = value
        }
        // Smoothness: a fraction, nil (no key) at 1, as the dialog stores it.
        if object["smoothness"] != nil {
            guard let value = doubleArg(object, "smoothness"), value.isFinite,
                  (0...1).contains(value) else {
                throw ToolError(message: "gradient.smoothness must be between 0 and 1")
            }
            gradient.smoothness = value >= 1 ? nil : value
        }
        // Align with Layer: the gradient spans the layer's mask box.
        if object["align_with_layer"] != nil {
            guard let value = boolArg(object, "align_with_layer") else {
                throw ToolError(message: "gradient.align_with_layer must be true or false")
            }
            gradient.alignWithLayer = value
        }
        return gradient
    }
}

extension Array where Element == ActionStep {
    /// Layer ▸ New Fill Layer's commit, from the payload the layer renders
    /// from: `add_fill_layer` above the active layer.
    static func addFillLayer(_ payload: FillLayerPayload) -> [ActionStep] {
        var arguments = fillArguments(payload)
        arguments["kind"] = payload.fill
        return [ActionStep(tool: "add_fill_layer", arguments: arguments,
                           note: "Layer ▸ New Fill Layer")]
    }

    /// The fill-layer dialog reopened on an existing layer: `edit_fill_layer`.
    static func editFillLayer(_ payload: FillLayerPayload, layer ref: LayerRef?) -> [ActionStep] {
        var arguments = fillArguments(payload)
        arguments["layer"] = ActionArgs.layer(ref)
        return [ActionStep(tool: "edit_fill_layer", arguments: arguments,
                           note: "Layers panel: edit a fill layer")]
    }

    /// `color`, `gradient` or `pattern` (with its tile's `rgba`), as both fill tools take
    /// them; a CMYK fill (#547) records its `ink`
    /// (and a solid colour's alpha as `color`'s) and each stop's `ink` beside its hex.
    private static func fillArguments(_ payload: FillLayerPayload) -> [String: Any] {
        if payload.ink == .cmyk, let color = payload.color {
            var arguments: [String: Any] = ["ink": AgentServer.inkPercent(color)]
            let alpha = ArtworkColor.alpha(color)
            if alpha < 1 { arguments["color"] = TextLayer.hex(numbers: [0, 0, 0, alpha]) }
            return arguments
        }
        if let pattern = payload.patternFill {
            return ["pattern": AgentServer.fillPatternArguments(pattern, tile: true)]
        }
        if let color = payload.color { return ["color": TextLayer.hex(numbers: color)] }
        if let gradient = payload.gradient {
            return ["gradient": AgentServer.fillGradientArguments(
                gradient, inks: payload.ink == .cmyk ? payload.stopInks : nil)]
        }
        return [:]
    }
}
