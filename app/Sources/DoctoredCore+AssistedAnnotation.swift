import AppKit

// MARK: - Assisted annotation replays (internal Phase 9I experiment)

extension RasterImage {
    /// The categorical label plane one validated replay describes. The core
    /// owns the whole schema, the stack reconstruction and the rasterizer's
    /// work cap; a refusal arrives with the core's message VERBATIM so the
    /// session can tell a `rasterizer_limit: ` rejection from a bad document.
    static func assistedReplayPlane(_ replayJSON: String, width: Int, height: Int,
                                    control: RasterJobControl? = nil) throws -> [UInt8] {
        guard width > 0, height > 0 else {
            throw DoctoredCoreError(message: "Assisted replay dimensions must be positive.")
        }
        var output = [UInt8](repeating: 0, count: width * height)
        var error: UnsafeMutablePointer<CChar>?
        let success = output.withUnsafeMutableBufferPointer { destination in
            dr_assisted_replay_plane(replayJSON, UInt32(width), UInt32(height),
                                     destination.baseAddress, control?.ptr, &error)
        }
        guard success else {
            throw DoctoredCoreError(message: takeErrorMessage(
                error, fallback: "The assisted replay could not be rasterized."))
        }
        return output
    }

    /// The same document's bookkeeping without rasterizing it: `final`,
    /// `outcome`, `modifying_gestures`, `active_strokes`, `human_origin`,
    /// `scoreable` and `elapsed_ms`.
    static func assistedReplaySummary(_ replayJSON: String) throws -> [String: Any] {
        var error: UnsafeMutablePointer<CChar>?
        guard let result = dr_assisted_replay_summary(replayJSON, &error) else {
            throw DoctoredCoreError(message: takeErrorMessage(
                error, fallback: "The assisted replay could not be summarized."))
        }
        defer { dr_string_free(result) }
        guard let value = try JSONSerialization.jsonObject(
            with: Data(String(cString: result).utf8)) as? [String: Any] else {
            throw DoctoredCoreError(message: "Invalid assisted replay summary.")
        }
        return value
    }
}

extension RasterImage {
    /// Experimental categorical constructor; no visible matting behavior is
    /// enabled by calling it. The base is explicit 0/128/255, never inferred.
    static func applyingTrimapStrokes(_ base: [UInt8], replay: RefinementTrimap,
                                      control: RasterJobControl? = nil) throws -> [UInt8] {
        guard base.count == replay.width * replay.height else {
            throw DoctoredCoreError(message: "Trimap base dimensions do not match the replay.")
        }
        let json = String(decoding: try replay.encodedData(), as: UTF8.self)
        var output = [UInt8](repeating: 0, count: base.count)
        var error: UnsafeMutablePointer<CChar>?
        let success = base.withUnsafeBufferPointer { input in
            output.withUnsafeMutableBufferPointer { destination in
                rz_trimap_strokes(input.baseAddress, UInt32(replay.width), UInt32(replay.height), json,
                                  destination.baseAddress, control?.ptr, &error)
            }
        }
        guard success else {
            throw DoctoredCoreError(message: takeErrorMessage(error, fallback: "Trimap strokes could not be constructed."))
        }
        return output
    }
}
