import AppKit

// MARK: - Photoshop adjustment preset files

/// Photoshop's `.acv` / `.alv` / `.ahu` / `.eap` / `.blw` / `.cha` adjustment presets,
/// decoded by the core — a free function like `RasterLUT`, because a
/// preset owns no handle: it produces the `{op, params}` an adjustment layer
/// stores. The host reads the file; every format rule and refusal sentence is
/// the core's.
enum RasterAdjustmentPresetFile {
    /// The payload the preset `bytes` (a file with extension `extension`, no
    /// dot) decode to, or `DoctoredCoreError` carrying the core's sentence.
    static func decode(_ bytes: Data, extension: String) throws -> AdjustmentLayerPayload {
        var err: UnsafeMutablePointer<CChar>? = nil
        let json: UnsafeMutablePointer<CChar>? = bytes.withUnsafeBytes { buffer in
            dr_adjustment_preset_file_decode(
                buffer.bindMemory(to: UInt8.self).baseAddress, buffer.count, `extension`, &err)
        }
        guard let json else {
            throw DoctoredCoreError(
                message: takeErrorMessage(err, fallback: "Could not read the Photoshop preset."))
        }
        defer { dr_string_free(json) }
        guard let data = String(cString: json).data(using: .utf8),
              let object = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
              let op = object["op"] as? String
        else {
            throw DoctoredCoreError(
                message: "The Photoshop preset decoded but its parameters could not be read.")
        }
        return AdjustmentLayerPayload(op: op, params: object["params"] as? [String: Any] ?? [:])
    }

    /// A Photoshop `.alv` or `.acv` with ink channels as a CMYK document's Levels or Curves
    /// (#1 phase 5, `dr_adjustment_preset_file_decode_cmyk`): composite and C, M, Y, K, the
    /// curves' points reflected onto the ink axes. Every other extension and an RGB `.acv` refuse
    /// with the core's sentence.
    static func decodeCMYK(_ bytes: Data, extension: String) throws -> AdjustmentLayerPayload {
        var err: UnsafeMutablePointer<CChar>? = nil
        let json: UnsafeMutablePointer<CChar>? = bytes.withUnsafeBytes { buffer in
            dr_adjustment_preset_file_decode_cmyk(
                buffer.bindMemory(to: UInt8.self).baseAddress, buffer.count, `extension`, &err)
        }
        let object = try precisionJSON(json, err)
        guard let op = object["op"] as? String else {
            throw DoctoredCoreError(message: "The CMYK preset decoded to no operation.")
        }
        return AdjustmentLayerPayload(op: op, params: object["params"] as? [String: Any] ?? [:])
    }
}
