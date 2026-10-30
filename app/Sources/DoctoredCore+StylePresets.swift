import Foundation

// MARK: - Style presets

/// The core side of the Styles panel (`core/src/style_presets.rs`,
/// `core/src/asl.rs`): free functions like `RasterAdjustmentPresetFile`,
/// because a preset owns no handle. The core owns the canonical form (strict
/// parse, no CMYK inks, no pattern anchors), the twelve built-ins and every
/// `.asl` format rule and refusal sentence; the host reads the file.
enum RasterStylePresets {
    /// `json` as a preset's canonical style JSON, or `DoctoredCoreError`
    /// carrying the core's sentence (which names the offending key).
    static func canonical(_ json: String) throws -> String {
        var err: UnsafeMutablePointer<CChar>? = nil
        guard let raw = dr_style_preset_canonical(json, &err) else {
            throw DoctoredCoreError(message: takeErrorMessage(err, fallback: "Invalid layer style."))
        }
        defer { dr_string_free(raw) }
        return String(cString: raw)
    }

    /// The twelve built-ins in panel order, each style canonical (read once).
    static func defaults() -> [(name: String, style: String)] { cachedDefaults }

    private static let cachedDefaults: [(name: String, style: String)] = {
        guard let raw = dr_style_preset_defaults_json(nil) else { return [] }
        defer { dr_string_free(raw) }
        guard let list = try? JSONSerialization.jsonObject(with: Data(String(cString: raw).utf8))
            as? [[String: Any]]
        else { return [] }
        return list.compactMap { entry in
            // The core's object is canonical; its text form is the core's own
            // writer's, so it round-trips through `canonical` unchanged.
            guard let name = entry["name"] as? String, let style = entry["style"],
                  JSONSerialization.isValidJSONObject(style),
                  let data = try? JSONSerialization.data(withJSONObject: style),
                  let text = String(data: data, encoding: .utf8),
                  let canonical = try? canonical(text)
            else { return nil }
            return (name, canonical)
        }
    }()

    /// A Photoshop style library's report `{"styles": [{name, style, blending,
    /// dropped}]}` (`dr_asl_decode`), or `DoctoredCoreError` with the core's
    /// sentence.
    static func decodeASL(_ bytes: Data) throws -> [String: Any] {
        var err: UnsafeMutablePointer<CChar>? = nil
        let json: UnsafeMutablePointer<CChar>? = bytes.withUnsafeBytes { buffer in
            dr_asl_decode(buffer.bindMemory(to: UInt8.self).baseAddress, buffer.count, &err)
        }
        guard let json else {
            throw DoctoredCoreError(
                message: takeErrorMessage(err, fallback: "Could not read the style library."))
        }
        defer { dr_string_free(json) }
        guard let object = try? JSONSerialization.jsonObject(with: Data(String(cString: json).utf8))
            as? [String: Any]
        else {
            throw DoctoredCoreError(message: "The style library decoded but could not be read.")
        }
        return object
    }

    /// `presets` written as a Photoshop style library (`dr_asl_encode`): the
    /// file's bytes and a "<name>: <reason>" for each preset the core could
    /// not write, or `DoctoredCoreError` with the core's sentence (nothing left
    /// to write, more than 1000 presets, over 64 MiB).
    static func encodeASL(_ presets: [StylePreset]) throws -> (data: Data, skipped: [String]) {
        let list: [[String: Any]] = presets.map { preset in
            ["name": preset.name, "style": preset.styleObject,
             "blending": preset.blending.map { $0.json as Any } ?? NSNull()]
        }
        guard JSONSerialization.isValidJSONObject(list),
              let body = try? JSONSerialization.data(withJSONObject: list),
              let text = String(data: body, encoding: .utf8)
        else { throw DoctoredCoreError(message: "The style presets could not be encoded.") }
        var err: UnsafeMutablePointer<CChar>? = nil
        guard let raw = dr_asl_encode(text, &err) else {
            throw DoctoredCoreError(
                message: takeErrorMessage(err, fallback: "Could not write the style library."))
        }
        defer { dr_string_free(raw) }
        guard let object = try? JSONSerialization.jsonObject(with: Data(String(cString: raw).utf8))
            as? [String: Any],
              let encoded = object["asl"] as? String,
              let data = Data(base64Encoded: encoded)
        else {
            throw DoctoredCoreError(message: "The style library encoded but could not be read.")
        }
        return (data, object["skipped"] as? [String] ?? [])
    }
}
