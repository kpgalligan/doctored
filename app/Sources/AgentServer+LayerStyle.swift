import AppKit

/// set_layer_style, set_global_light: the agent mirrors of Layer > Layer
/// Style… (Apply / Clear Layer Style) and the panes' Use Global Light edits
/// (EditorViewController+LayerStyle.swift / LayerStyleSheetController).
/// Catalog entries live in AgentCatalog+MaskStyle.swift and the dispatch
/// entries in this file's handler table — the parity check at server start
/// enforces it.
extension AgentServer {
    /// This file's MCP tools; merged by AgentServer+Registry.swift.
    static let layerStyleHandlers: [String: ToolHandler] = [
        "set_layer_style": { $0.setLayerStyle },
        "set_global_light": { $0.setGlobalLight },
    ]

    /// Facts for this file's tools (recording, selection, depth admission); merged in AgentServer+Registry.swift.
    static let layerStyleToolFacts: [String: ToolFacts] = [
        // A 32-bit float document holds a style (the core's
        // `layer_style` family at depth 32 refuses an effect mode outside the
        // float set by name).
        // #1: on a CMYK document every effect with its colours as inks
        // (`ink`, `highlight_ink`, `shadow_ink`, a stop's `ink`), knockout and
        // Blend Interior; a Blend If on Gray or a plate (Red, Green and Blue
        // refuse by name in the core).
        "set_layer_style": [.typedTool, .floatTool, .cmykTool],
        // The typed document's global light (`{op:"global_light"}`), at
        // 32 bits too, on a CMYK document since #1.
        "set_global_light": [.typedTool, .floatTool, .cmykTool],
    ]

    /// set_layer_style — mirrors Layer > Layer Style… Apply and Clear Layer
    /// Style: the WHOLE style at once (idempotent); `style` null or absent
    /// clears, and so does an identity style (the core's rule). The core
    /// validates and canonicalizes; a refused style comes back in-band with
    /// the core's message, which names the offending key.
    ///
    /// A GROUP may carry a style, so this takes `structuralLayerIndex` rather
    /// than the pixel-only helper: the shape the effects hang off is the
    /// group's own rendered projection, which the compositor hands to the
    /// synthetic layer it composites the group as (`Layer::renders_style`).
    /// A styled group also composites as a UNIT — the style is one of the
    /// things that makes a Pass Through group isolate.
    func setLayerStyle(_ a: [String: Any]) throws -> String {
        let document = try target(a)
        guard let doc = document.doc else { throw ToolError(message: "Document has no image") }
        let index = try structuralLayerIndex(a, document)
        // An adjustment layer's style hangs off its mask over the canvas
        // (a CMYK one since phase 2b-1), so no kind refuses here.
        // A gradient stop's midpoints out of 0.05…0.95 are refused by
        // name, as the `gradient` tool and the fill layers refuse them; the
        // core's parser would clamp them.
        try refuseStyleMidpoints(a["style"])
        let wanted = try Self.styleArgument(a["style"])
        // Validate through the core FIRST, outside any undo group: an
        // invalid style must not open one, and the core's equal-value
        // refusal is what makes the call idempotent.
        let preview: RasterDocument?
        do {
            preview = try doc.withLayerStyle(index, wanted)
        } catch {
            throw ToolError(message: error.localizedDescription)
        }
        guard let updated = preview else {
            return try jsonResult([
                "ok": true, "layer": index, "unchanged": true,
                "style": Self.layerStyleFields(doc, index) ?? NSNull(),
            ])
        }
        // The validated document IS the edit (same source document, same
        // main-thread call): the core's setter hands the previous style's
        // rendered planes to the new one and drains the old cache, so a
        // second call inside the group would commit a style with no planes
        // — a full re-render on the next composite — while this discarded
        // copy kept them.
        try performGroupedEdit(document, wanted == nil ? "Clear Layer Style" : "Layer Style") {
            _ in updated
        }
        return try jsonResult([
            "ok": true, "layer": index,
            "style": Self.layerStyleFields(document.doc, index) ?? NSNull(),
        ])
    }

    /// set_global_light — mirrors the Use Global Light angle/altitude edits
    /// in the Layer Style panes: the document's shared light, read by every
    /// effect with use_global_light on. Either argument may be omitted. At
    /// 16 and 32 bits it is the typed `{op:"global_light"}` edit (a
    /// float document holds the light).
    func setGlobalLight(_ a: [String: Any]) throws -> String {
        let document = try target(a)
        guard let doc = document.doc else { throw ToolError(message: "Document has no image") }
        var values: [String: Double] = [:]
        for name in ["angle", "altitude"] where a[name] != nil {
            guard let value = doubleArg(a, name), value.isFinite else {
                throw ToolError(message: "\(name) must be a finite number of degrees")
            }
            values[name] = value
        }
        guard !values.isEmpty else {
            throw ToolError(
                message: "set_global_light needs angle (degrees; 0 = light from the right, "
                    + "90 = from the top) and/or altitude (0..90) — nothing to change")
        }
        let angle = values["angle"] ?? doc.globalLightAngle
        let altitude = values["altitude"] ?? doc.globalLightAltitude
        // The core sanitizes (altitude clamped, angle normalized) and refuses
        // a value equal to the current one — no phantom undo step. Computed
        // ONCE: at 16 bits the edit runs a core request, so it is published
        // as the value already made rather than asked for twice.
        guard let next = try doc.withGlobalLight(angle: angle, altitude: altitude) else {
            return try jsonResult([
                "ok": true, "unchanged": true, "global_light": Self.globalLightFields(doc),
            ])
        }
        try performGroupedEdit(document, "Global Light") { _ in next }
        guard let updated = document.doc else { throw ToolError(message: "Document has no image") }
        return try jsonResult(["ok": true, "global_light": Self.globalLightFields(updated)])
    }

    /// Each effect's gradient stop through `stopMidpoint`, so a
    /// `midpoint` or `opacity_midpoint` outside 0.05…0.95 is refused as
    /// `effects[i].gradient.stops[k].<key> must be 0.05…0.95`. Everything
    /// else is the core's to judge; a style that does not parse is left to
    /// it too.
    private func refuseStyleMidpoints(_ raw: Any?) throws {
        var object = raw as? [String: Any]
        if object == nil, let text = raw as? String, let data = text.data(using: .utf8) {
            object = (try? JSONSerialization.jsonObject(with: data)) as? [String: Any]
        }
        guard let effects = object?["effects"] as? [Any] else { return }
        for (i, effect) in effects.enumerated() {
            let gradient = (effect as? [String: Any])?["gradient"] as? [String: Any]
            guard let stops = gradient?["stops"] as? [Any] else { continue }
            for (k, stop) in stops.enumerated() {
                guard let entry = stop as? [String: Any] else { continue }
                _ = try stopMidpoint(entry, at: "effects[\(i)].gradient.stops[\(k)]")
            }
        }
    }

    /// `object` with its library pattern references resolved; a name the library does not hold
    /// refuses in-band.
    private static func resolvingLibraryPatterns(_ object: [String: Any]) throws -> [String: Any] {
        do {
            return try PatternLibrary.resolvingReferences(in: object) as? [String: Any] ?? object
        } catch {
            throw ToolError(message: error.localizedDescription)
        }
    }

    /// The `style` argument as the JSON string the core takes: an object
    /// (re-serialized; the core canonicalizes key order), a JSON string as
    /// is, or nil for null / absent / empty — which clears. A pattern object
    /// `{"name": …}` naming a library pattern, in either form, becomes that
    /// pattern's tile (`PatternLibrary.resolvingReferences`; the recorder
    /// resolves the same way, `PatternLibrary.recordedArguments`).
    private static func styleArgument(_ raw: Any?) throws -> String? {
        switch raw {
        case nil, is NSNull:
            return nil
        case let object as [String: Any]:
            let resolved = try resolvingLibraryPatterns(object)
            guard JSONSerialization.isValidJSONObject(resolved),
                  let data = try? JSONSerialization.data(withJSONObject: resolved)
            else { throw ToolError(message: "style must be a JSON object or null") }
            return String(decoding: data, as: UTF8.self)
        case let text as String:
            let trimmed = text.trimmingCharacters(in: .whitespacesAndNewlines)
            if trimmed.isEmpty || trimmed == "null" { return nil }
            guard let parsed = try? JSONSerialization.jsonObject(with: Data(text.utf8)),
                  let object = parsed as? [String: Any], PatternLibrary.containsReference(object)
            else { return text }
            let resolved = try resolvingLibraryPatterns(object)
            guard let data = try? JSONSerialization.data(withJSONObject: resolved) else {
                return text
            }
            return String(decoding: data, as: UTF8.self)
        default:
            throw ToolError(
                message: "style must be a JSON object (the shape get_document reports) or "
                    + "null to clear")
        }
    }

    /// Layer `index`'s style as the canonical object (every key of every
    /// present effect) — what get_document reports and what set_layer_style
    /// takes back unchanged; nil for an unstyled layer.
    static func layerStyleFields(_ doc: RasterDocument?, _ index: Int) -> [String: Any]? {
        guard let json = doc?.layerStyle(index) else { return nil }
        return layerStyleObject(json)
    }

    /// A style's canonical JSON text as the object get_document reports and
    /// set_layer_style takes, numbers spelled through `shortNumbers`; nil when
    /// the text is not a JSON object. `list_layer_comps` reports a comp's
    /// recorded style through this too, so the two shapes cannot drift.
    static func layerStyleObject(_ json: String) -> [String: Any]? {
        guard let object = try? JSONSerialization.jsonObject(with: Data(json.utf8)) else {
            return nil
        }
        return shortNumbers(object) as? [String: Any]
    }

    /// JSONSerialization writes a parsed double back with 17 significant
    /// digits (`0.9` → `0.90000000000000002`), which would misreport the
    /// core's 4-decimal values; a decimal number built from Swift's
    /// shortest round-trip spelling prints as the core wrote it. Integers
    /// (objCType "q") pass through untouched.
    static func shortNumbers(_ value: Any) -> Any {
        if let number = value as? NSNumber {
            let type = String(cString: number.objCType)
            guard type == "d" || type == "f" else { return number }
            return NSDecimalNumber(string: "\(number.doubleValue)")
        }
        if let object = value as? [String: Any] {
            return object.mapValues(shortNumbers)
        }
        if let array = value as? [Any] {
            return array.map(shortNumbers)
        }
        return value
    }

    /// The document's global light, degrees. The core stores the light to
    /// four decimals (its canonical precision for every style number), so
    /// the Float-to-Double widening is rounded back to that and spelled
    /// through `shortNumbers` — `33.3333`, never `33.333300000000001` — and
    /// the reported value echoed into set_global_light is refused as
    /// unchanged, exactly like a style echo.
    static func globalLightFields(_ doc: RasterDocument) -> [String: Any] {
        let round: (Double) -> Double = { ($0 * 10000).rounded() / 10000 }
        let fields: [String: Any] = [
            "angle": round(doc.globalLightAngle), "altitude": round(doc.globalLightAltitude),
        ]
        return shortNumbers(fields) as? [String: Any] ?? fields
    }
}
