import AppKit

// MARK: - Versioned evaluation (Phase 11E)
//
// The `dr_*` half of the evaluation policy. The policy itself — its names, its
// display strings, the archive tags and the sheet's readout — lives in
// `DocumentEvaluation.swift`; this file only owns the calls.

/// The core's evaluation vocabulary: the name→code mapping and the operator
/// table, neither of which needs a document.
enum EvaluationCore {
    /// The `DR_EVALUATION_*` value for a wire name; nil for a name this build
    /// does not know.
    static func code(forName name: String) -> Int32? {
        let value = dr_evaluation_from_name(name)
        return value < 0 ? nil : value
    }

    /// What linear-light evaluation evaluates and what it refuses.
    static func operatorsJSON() throws -> [String: Any] {
        guard let raw = dr_evaluation_operators() else {
            throw DoctoredCoreError(message: "The evaluation operator table is unavailable.")
        }
        defer { dr_string_free(raw) }
        guard let value = try JSONSerialization.jsonObject(
            with: Data(String(cString: raw).utf8)) as? [String: Any] else {
            throw DoctoredCoreError(message: "Invalid evaluation operator table.")
        }
        return value
    }
}

extension RasterPrecisionDocument {
    var evaluationCode: Int32 { dr_precision_doc_evaluation(ptr) }

    /// The first operator linear-light evaluation cannot evaluate; nil when
    /// there is nothing to refuse.
    func linearLightRefusalText() -> String? {
        guard let raw = dr_precision_doc_linear_light_refusal(ptr) else { return nil }
        defer { dr_string_free(raw) }
        return String(cString: raw)
    }

    /// nil WITHOUT throwing when the policy is already `code`.
    func settingEvaluation(_ code: Int32) throws -> RasterPrecisionDocument? {
        var error: UnsafeMutablePointer<CChar>?
        guard let result = dr_precision_doc_set_evaluation(ptr, code, &error) else {
            if error != nil {
                throw DoctoredCoreError(message: takeErrorMessage(
                    error, fallback: "The evaluation policy could not be changed."))
            }
            return nil
        }
        return RasterPrecisionDocument(owning: result)
    }
}

extension RasterDocument {
    /// The document's evaluation policy code, at whichever depth it stores.
    var evaluationCode: Int32 {
        if let precision { return precision.evaluationCode }
        return dr_doc_evaluation(ptr)
    }

    /// The refusal sentence, or nil. The core answers nil for an ENCODED
    /// document before walking anything, so this is cheap on every edit.
    func linearLightRefusalText() -> String? {
        if let precision { return precision.linearLightRefusalText() }
        guard let raw = dr_doc_linear_light_refusal(ptr) else { return nil }
        defer { dr_string_free(raw) }
        return String(cString: raw)
    }

    /// nil WITHOUT throwing when the policy is already `code`.
    func settingEvaluation(_ code: Int32) throws -> RasterDocument? {
        if let precision {
            guard let result = try precision.settingEvaluation(code) else { return nil }
            return try RasterDocument(precision: result)
        }
        var error: UnsafeMutablePointer<CChar>?
        guard let result = dr_doc_set_evaluation(ptr, code, &error) else {
            if error != nil {
                throw DoctoredCoreError(message: takeErrorMessage(
                    error, fallback: "The evaluation policy could not be changed."))
            }
            return nil
        }
        return RasterDocument(owning: result)
    }
}

extension RasterImage {
    /// `{"depth":8,...}` over all RGBA samples; a dimension mismatch throws.
    func differenceJSON(to other: RasterImage) throws -> [String: Any] {
        var error: UnsafeMutablePointer<CChar>?
        let raw = dr_image_compare(ptr, other.ptr, &error)
        guard let raw else {
            throw DoctoredCoreError(message: takeErrorMessage(
                error, fallback: "The two images could not be compared."))
        }
        defer { dr_string_free(raw) }
        guard let value = try JSONSerialization.jsonObject(
            with: Data(String(cString: raw).utf8)) as? [String: Any] else {
            throw DoctoredCoreError(message: "Invalid image comparison result.")
        }
        return value
    }
}

extension RasterPrecisionImage {
    /// The typed twin, in codes of 65535.
    func differenceJSON(to other: RasterPrecisionImage) throws -> [String: Any] {
        var error: UnsafeMutablePointer<CChar>?
        let raw = dr_precision_image_compare(ptr, other.ptr, &error)
        guard let raw else {
            throw DoctoredCoreError(message: takeErrorMessage(
                error, fallback: "The two high-depth images could not be compared."))
        }
        defer { dr_string_free(raw) }
        guard let value = try JSONSerialization.jsonObject(
            with: Data(String(cString: raw).utf8)) as? [String: Any] else {
            throw DoctoredCoreError(message: "Invalid high-depth image comparison result.")
        }
        return value
    }
}
