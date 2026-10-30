import AppKit

// `DoctoredCore.layerStyle` / `layerHasStyle` / `withLayerStyle` branch here (the ONLY typed branches
// of the style facade, so get_document, the Layers badge, Copy/Paste/Clear Layer Style, the
// recorded `set_layer_style` Action and the conversion sentence need no depth code):
// `nativeLayerStyle` reads the core's canonical `style` metadata row (`parseStyle`; the key is
// required, null → nil; `parseBlendIf` reads the `blend_if` row beside it) and
// `settingNativeLayerStyle` sends the whole style through the core's
// `{op:"layer_style",version:1,layer,style}` edit, which owns every rule (every record kind holds
// one, a Pass Through group included; identity clears, the 100 MP
// padded-plane limit, an unknown key refused by name).

/// The 16-bit document's layer style — the ONE typed branch of the
/// layer-style facade (`DoctoredCore.layerStyle` / `layerHasStyle` /
/// `withLayerStyle`), so get_document, the Layers badge, Copy/Paste/Clear
/// Layer Style, the recorded Action and the conversion sentence need no
/// depth-specific code. A typed layer carries fill opacity, the nine effects
/// and Blend If exactly as a byte layer does; the core reports the merged
/// canonical style JSON in the `style` metadata row and takes the whole
/// style back through its `{op:"layer_style"}` edit
/// (`core/src/precision_style.rs`), which owns every rule: raster layers,
/// adjustment layers and groups, a Pass Through one included (it then
/// composites isolated), an identity clears and the
/// padded-plane limit.
/// Effects read the document's global light, set through the core's
/// `{op:"global_light"}` edit (`RasterDocument.withGlobalLight`).
enum NativeLayerStyle {
    /// The core's `blend_if` metadata object as a typed value: null → nil;
    /// anything else must be a channel in `BlendIf.channels` (or, on a CMYK
    /// document, `BlendIf.cmykChannels`) and two
    /// non-decreasing ramps of four 0…255 integers, and its optional `extra`
    /// a list of objects of the same three keys.
    static func parseBlendIf(_ raw: Any?) throws -> BlendIf? {
        guard let raw, !(raw is NSNull) else { return nil }
        guard let object = raw as? [String: Any], let primary = range(object) else {
            throw DoctoredCoreError(message: "High-depth Blend If metadata is invalid.")
        }
        var extra: [BlendIfRange]?
        if let list = object["extra"] {
            guard let entries = list as? [Any] else {
                throw DoctoredCoreError(message: "High-depth Blend If metadata is invalid.")
            }
            extra = try entries.map { entry in
                guard let entry = entry as? [String: Any], let parsed = range(entry) else {
                    throw DoctoredCoreError(message: "High-depth Blend If metadata is invalid.")
                }
                return parsed
            }
        }
        return BlendIf(
            channel: primary.channel, thisLayer: primary.thisLayer,
            underlying: primary.underlying, extra: extra?.isEmpty == false ? extra : nil)
    }

    /// One `{channel, this_layer, underlying}` object, or nil when malformed.
    private static func range(_ object: [String: Any]) -> BlendIfRange? {
        guard let channel = object["channel"] as? String,
              BlendIf.channels.contains(channel) || BlendIf.cmykChannels.contains(channel),
              let thisLayer = ramp(object["this_layer"]), let underlying = ramp(object["underlying"])
        else { return nil }
        return BlendIfRange(channel: channel, thisLayer: thisLayer, underlying: underlying)
    }

    /// The core's `style` metadata row as the canonical JSON string: null →
    /// nil; anything else must be a string the dialog's model decodes.
    static func parseStyle(_ raw: Any?) throws -> String? {
        guard let raw, !(raw is NSNull) else { return nil }
        guard let json = raw as? String, LayerStyle.decode(json) != nil else {
            throw DoctoredCoreError(message: "High-depth layer style metadata is invalid.")
        }
        return json
    }

    /// The ONE builder of the core's `{op:"layer_style",version:1,layer,style}`
    /// request: `json` is the whole style (parsed here into the request
    /// object; the core parses it strictly) or nil to clear. `scale`,
    /// when given, has the core scale the style's lengths before it stores
    /// them — the reduced style preview's `1 / factor`; a commit sends none.
    static func command(layer: Int, json: String?, scale: Double? = nil) throws -> PrecisionPixelCommand {
        var value: Any = NSNull()
        if let json {
            guard let object = try? JSONSerialization.jsonObject(with: Data(json.utf8)) as? [String: Any] else {
                throw DoctoredCoreError(message: "Invalid layer style.")
            }
            value = object
        }
        var fields: [String: Any] = ["op": "layer_style", "version": 1, "layer": layer, "style": value]
        if let scale { fields["scale"] = scale }
        return try PrecisionPixelCommand(fields)
    }

    private static func ramp(_ raw: Any?) -> [Int]? {
        guard let list = raw as? [Any], list.count == 4 else { return nil }
        let values = list.compactMap { ($0 as? NSNumber)?.intValue }
        guard values.count == 4, values.allSatisfy({ (0...255).contains($0) }), values == values.sorted() else {
            return nil
        }
        return values
    }
}

extension RasterDocument {
    /// Layer `idx`'s canonical style JSON from the parsed typed metadata; nil
    /// for none or an out-of-range index.
    func nativeLayerStyle(_ idx: Int) -> String? {
        precisionMetadata?.layer(idx)?.style
    }

    /// `NativeLayerStyle.command` through the admitted, cancellable typed
    /// edit ABI: `json` is the whole style or nil to clear. nil for an exact
    /// no-op; throws the core's refusal (target kind, a key it names, the
    /// plane limit).
    func settingNativeLayerStyle(_ idx: Int, _ json: String?) throws -> RasterDocument? {
        try performingPrecisionPixelEdit(NativeLayerStyle.command(layer: idx, json: json))
    }

    /// The same request as a bookkeeping edit, for the Layer Style sheet's
    /// 16-bit preview (`PrecisionStylePreview`): main thread only, no
    /// pixels and never `BackgroundWork`. `scale` is `command`'s. nil for an
    /// exact no-op; throws the core's refusal, or the busy sentence while a
    /// background operation holds the worker.
    func previewingNativeLayerStyle(_ idx: Int, _ json: String?, scale: Double?) throws -> RasterDocument? {
        try applyingPrecisionBookkeepingEdit(NativeLayerStyle.command(layer: idx, json: json, scale: scale))
    }
}
