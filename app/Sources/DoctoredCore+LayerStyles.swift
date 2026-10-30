import AppKit

extension RasterDocument {
    // MARK: - Layer styles

    /// Layer `idx`'s style as the core's canonical JSON, or nil when it has
    /// none. The core is the authority on the schema (core/src/style.rs);
    /// LayerStyle.swift reads it back into a typed value for the dialog.
    func layerStyle(_ idx: Int) -> String? {
        if isHighDepth { return nativeLayerStyle(idx) }
        guard isValidIndex(idx), let cStyle = dr_doc_layer_style(ptr, idx) else { return nil }
        defer { dr_string_free(cStyle) }
        return String(cString: cStyle)
    }

    /// Whether layer `idx` carries a style — the cheap badge query. Because
    /// the core never stores an identity style, true means the layer renders
    /// something.
    func layerHasStyle(_ idx: Int) -> Bool {
        if isHighDepth { return nativeLayerStyle(idx) != nil }
        guard isValidIndex(idx) else { return false }
        return dr_doc_layer_has_style(ptr, idx)
    }

    /// Attaches a style (canonical or not — the core canonicalizes) to layer
    /// `idx`, or CLEARS it when `json` is nil (an identity style also
    /// clears). nil for an out-of-range index or a value equal to the
    /// current one (the core refuses identical copies, so no phantom undo
    /// step); throws with the core's message when the JSON is not a valid
    /// style. `ptr` is never NULL here, so the core's "document is NULL"
    /// message cannot occur on this path.
    func withLayerStyle(_ idx: Int, _ json: String?) throws -> RasterDocument? {
        if isHighDepth {
            // The whole style through the core's typed edit (NativeLayerStyle).
            return try settingNativeLayerStyle(idx, json)
        }
        guard isValidIndex(idx) else { return nil }
        var err: UnsafeMutablePointer<CChar>? = nil
        let handle: OpaquePointer?
        if let json = json {
            handle = dr_doc_set_layer_style(ptr, idx, json, &err)
        } else {
            handle = dr_doc_set_layer_style(ptr, idx, nil, &err)
        }
        guard let handle = handle else {
            // NULL with no message is the refusal tier (unchanged value).
            guard err != nil else { return nil }
            throw DoctoredCoreError(
                message: takeErrorMessage(err, fallback: "Invalid layer style."))
        }
        return RasterDocument(owning: handle)
    }

    /// The document's global light, degrees: the direction every effect with
    /// use_global_light on reads (0 = from the right, 90 = from the top;
    /// altitude 0…90).
    /// A typed document reads its facade's `global_light`; a byte one
    /// asks the core.
    var globalLightAngle: Double {
        precisionMetadata?.globalLight.angle ?? Double(dr_doc_global_light_angle(ptr))
    }
    var globalLightAltitude: Double {
        precisionMetadata?.globalLight.altitude ?? Double(dr_doc_global_light_altitude(ptr))
    }

    /// The four frozen Pattern Overlay presets, in the popup's order, each with its tile
    /// as an image (the swatch) — read once from the core (`dr_pattern_presets_json`).
    static func patternPresets() -> [PatternPreset] { cachedPatternPresets }

    private static let cachedPatternPresets: [PatternPreset] = {
        guard let raw = dr_pattern_presets_json(nil) else { return [] }
        defer { dr_string_free(raw) }
        guard let list = try? JSONSerialization.jsonObject(with: Data(String(cString: raw).utf8))
            as? [[String: Any]]
        else { return [] }
        return list.compactMap { entry in
            guard let id = entry["preset"] as? String, let name = entry["name"] as? String,
                  let width = entry["width"] as? Int, let height = entry["height"] as? Int,
                  let text = entry["rgba"] as? String, let bytes = Data(base64Encoded: text),
                  bytes.count == width * height * 4,
                  let rep = NSBitmapImageRep(
                      bitmapDataPlanes: nil, pixelsWide: width, pixelsHigh: height,
                      bitsPerSample: 8, samplesPerPixel: 4, hasAlpha: true, isPlanar: false,
                      colorSpaceName: .deviceRGB, bitmapFormat: [.alphaNonpremultiplied],
                      bytesPerRow: width * 4, bitsPerPixel: 32),
                  let pixels = rep.bitmapData
            else { return nil }
            bytes.copyBytes(to: pixels, count: bytes.count)
            let image = NSImage(size: NSSize(width: 16, height: 16))
            image.addRepresentation(rep)
            return PatternPreset(id: id, name: name, image: image)
        }
    }()

    /// nil when nothing changes after the core's sanitizing (altitude clamped
    /// to 0…90, angle normalized to -180…180). At 8 bits a non-finite value
    /// also answers nil. A typed document takes the `global_light`
    /// bookkeeping edit, which throws on a non-finite value, on a float
    /// document and while background work runs.
    func withGlobalLight(angle: Double, altitude: Double) throws -> RasterDocument? {
        if isHighDepth {
            return try applyingPrecisionBookkeepingEdit(
                .init(["op": "global_light", "angle": angle, "altitude": altitude]))
        }
        return wrap(dr_doc_set_global_light(ptr, Float(angle), Float(altitude)))
    }
}
