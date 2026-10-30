import AppKit

/// The 32-bit float mode's vocabulary (Phase 11F): the operator table, which
/// needs no document.
enum FloatCore {
    /// What the float mode evaluates, what it refuses and why, and the tone
    /// maps it offers.
    static func operatorsJSON() throws -> [String: Any] {
        guard let raw = dr_float_operators() else {
            throw DoctoredCoreError(message: "The float operator table is unavailable.")
        }
        defer { dr_string_free(raw) }
        guard let value = try JSONSerialization.jsonObject(
            with: Data(String(cString: raw).utf8)) as? [String: Any] else {
            throw DoctoredCoreError(message: "Invalid float operator table.")
        }
        return value
    }
}

extension RasterPrecisionDocument {
    var isFloat: Bool { dr_precision_doc_is_float(ptr) }

    /// The document's tone map as its wire JSON; nil on a u16 document.
    func toneMapJSON() -> String? {
        guard let raw = dr_precision_doc_tone_map(ptr) else { return nil }
        defer { dr_string_free(raw) }
        return String(cString: raw)
    }

    /// nil WITHOUT throwing when the map is unchanged.
    func settingToneMap(_ json: String) throws -> RasterPrecisionDocument? {
        var error: UnsafeMutablePointer<CChar>?
        guard let result = dr_precision_doc_set_tone_map(ptr, json, &error) else {
            if error != nil {
                throw DoctoredCoreError(message: takeErrorMessage(
                    error, fallback: "The tone map could not be changed."))
            }
            return nil
        }
        return RasterPrecisionDocument(owning: result)
    }

    /// The first operator the float mode cannot evaluate; nil when there is
    /// nothing to refuse.
    func floatRefusalText() -> String? {
        guard let raw = dr_precision_doc_float_refusal(ptr) else { return nil }
        defer { dr_string_free(raw) }
        return String(cString: raw)
    }

    /// The ONE way a float projection becomes display bytes.
    func displayRGBA8(of image: RasterPrecisionImage, workingLimit: UInt64,
                      otherRetained: UInt64,
                      control: RasterJobControl? = nil) throws -> RasterImage {
        var error: UnsafeMutablePointer<CChar>?
        guard let result = dr_precision_doc_display_rgba8(
            ptr, image.ptr, workingLimit, otherRetained, control?.ptr, &error) else {
            throw DoctoredCoreError(message: takeErrorMessage(
                error, fallback: "The tone-mapped display could not be produced."))
        }
        return RasterImage(owning: result)
    }

    /// The float projection under a VIEW: `view`'s tone map (nil = the
    /// document's own) and its exposure in stops. `.document` is byte for
    /// byte `displayRGBA8(of:workingLimit:otherRetained:control:)`; nothing
    /// in the document changes.
    func displayRGBA8(of image: RasterPrecisionImage, view: FloatDisplayView,
                      workingLimit: UInt64, otherRetained: UInt64,
                      control: RasterJobControl? = nil) throws -> RasterImage {
        var error: UnsafeMutablePointer<CChar>?
        guard let result = dr_precision_doc_display_rgba8_view(
            ptr, image.ptr, view.toneMap?.jsonText, view.exposureStops, workingLimit,
            otherRetained, control?.ptr, &error) else {
            throw DoctoredCoreError(message: takeErrorMessage(
                error, fallback: "The tone-mapped display could not be produced."))
        }
        return RasterImage(owning: result)
    }

    /// Float layer `image`'s Layers-panel thumbnail through this document's
    /// tone map: fitted to `maxSide` without upscaling, a large layer
    /// box-reduced in the core before any whole-layer copy.
    func displayThumbnailRGBA8(of image: RasterPrecisionImage, maxSide: Int,
                               workingLimit: UInt64, otherRetained: UInt64,
                               control: RasterJobControl? = nil) throws -> RasterImage {
        var error: UnsafeMutablePointer<CChar>?
        guard let result = dr_precision_doc_display_thumbnail_rgba8(
            ptr, image.ptr, UInt32(clamping: maxSide), workingLimit, otherRetained,
            control?.ptr, &error) else {
            throw DoctoredCoreError(message: takeErrorMessage(
                error, fallback: "The tone-mapped thumbnail could not be produced."))
        }
        return RasterImage(owning: result)
    }

    /// The deliberate conversion to the u16 mode through this document's map.
    func floatToU16(workingLimit: UInt64, otherRetained: UInt64,
                    control: RasterJobControl? = nil) throws -> RasterPrecisionDocument {
        var error: UnsafeMutablePointer<CChar>?
        guard let result = dr_precision_doc_float_to_u16(
            ptr, workingLimit, otherRetained, control?.ptr, &error) else {
            throw DoctoredCoreError(message: takeErrorMessage(
                error, fallback: "The document could not be converted to 16-bit."))
        }
        return RasterPrecisionDocument(owning: result)
    }
}

extension RasterPrecisionImage {
    /// The extended-range sample readout.
    func floatSampleJSON(x: Int32, y: Int32, reach: UInt32) throws -> [String: Any] {
        var error: UnsafeMutablePointer<CChar>?
        let raw = dr_precision_image_float_sample(ptr, x, y, reach, &error)
        return try floatJSON(raw, error, fallback: "The float sample could not be read.")
    }

    private func floatJSON(_ pointer: UnsafeMutablePointer<CChar>?,
                           _ error: UnsafeMutablePointer<CChar>?,
                           fallback: String) throws -> [String: Any] {
        guard let pointer else {
            throw DoctoredCoreError(message: takeErrorMessage(error, fallback: fallback))
        }
        defer { dr_string_free(pointer) }
        guard let value = try JSONSerialization.jsonObject(
            with: Data(String(cString: pointer).utf8)) as? [String: Any] else {
            throw DoctoredCoreError(message: fallback)
        }
        return value
    }
}
