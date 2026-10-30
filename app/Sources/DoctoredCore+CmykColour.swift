import AppKit

// The CMYK document's colour workflow wrappers (#1 phase 5): every `dr_*` call of
// `ffi_cmyk_colour.rs` — Assign / Convert to Profile and 8/16 Bits/Channel, File ▸ New in CMYK,
// the CMYK proof, Levels and Curves' histogram / Auto / eyedroppers and ⌥ clipping display, the
// one-colour separation and the CMYK preset decode.

/// One RGB colour's separation (`dr_cmyk_separate`): the ink in percent, the gamut verdict and
/// the in-gamut colour (encoded RGB 0…1 in the same profile).
struct CmykSeparation {
    let ink: CmykInk
    let outOfGamut: Bool
    let deltaE: Double
    let inGamut: [Double]
    let profile: String
}

private func jsonText(_ object: [String: Any]) -> String {
    guard let data = try? JSONSerialization.data(withJSONObject: object, options: [.sortedKeys]),
          let text = String(data: data, encoding: .utf8) else { return "{}" }
    return text
}

extension RasterPrecisionDocument {
    private static func withBytes<T>(_ data: Data?,
                                     _ body: (UnsafePointer<UInt8>?, Int) throws -> T) rethrows -> T {
        guard let data, !data.isEmpty else { return try body(nil, 0) }
        return try data.withUnsafeBytes { raw in
            try body(raw.bindMemory(to: UInt8.self).baseAddress, data.count)
        }
    }

    // MARK: - Assign / Convert to Profile, 8/16 Bits/Channel, New

    /// The profile edit's plan: `working_memory_plan_bytes`, `refusal`, `flatten_required`.
    /// `profile` nil is the working CMYK profile.
    func cmykProfilePlan(profile: Data?, request: [String: Any]) throws -> [String: Any] {
        var error: UnsafeMutablePointer<CChar>?
        let text = jsonText(request)
        let result = Self.withBytes(profile) { bytes, count in
            dr_precision_doc_cmyk_profile_plan(ptr, bytes, count, text, &error)
        }
        return try precisionJSON(result, error)
    }

    /// Assign, Convert or Depth (`{"op":"assign"|"convert"|"depth",…}`); nil when it changes
    /// nothing.
    func cmykProfileEdit(profile: Data?, request: [String: Any], workingLimit: UInt64,
                         otherRetained: UInt64, control: RasterJobControl?) throws -> RasterPrecisionDocument? {
        var error: UnsafeMutablePointer<CChar>?
        let text = jsonText(request)
        let result = Self.withBytes(profile) { bytes, count in
            dr_precision_doc_cmyk_profile_edit(ptr, bytes, count, text, workingLimit, otherRetained,
                                               control?.ptr, &error)
        }
        guard let result else {
            if let error {
                throw DoctoredCoreError(message: takeErrorMessage(error, fallback: "Could not edit the CMYK profile."))
            }
            return nil
        }
        return RasterPrecisionDocument(owning: result)
    }

    /// File ▸ New in CMYK: request `{"depth","fill":[c,m,y,k,a],"layer_name","ppi"}`; `profile`
    /// nil is the working CMYK profile.
    static func newCMYK(width: Int, height: Int, profile: Data?, request: [String: Any],
                        workingLimit: UInt64, otherRetained: UInt64) throws -> RasterPrecisionDocument {
        var error: UnsafeMutablePointer<CChar>?
        let text = jsonText(request)
        let result = withBytes(profile) { bytes, count in
            dr_precision_doc_new_cmyk(UInt32(clamping: width), UInt32(clamping: height), bytes, count,
                                      text, workingLimit, otherRetained, &error)
        }
        guard let result else {
            throw DoctoredCoreError(message: takeErrorMessage(error, fallback: "Could not create the CMYK document."))
        }
        return RasterPrecisionDocument(owning: result)
    }

    // MARK: - The CMYK proof

    /// The CMYK proof's plan: `working_memory_plan_bytes`, `refusal`.
    func cmykProofPlan(profile: Data, setupJSON: String) throws -> [String: Any] {
        var error: UnsafeMutablePointer<CChar>?
        let result = Self.withBytes(profile) { bytes, count in
            dr_precision_doc_cmyk_proof_plan(ptr, bytes, count, setupJSON, &error)
        }
        return try precisionJSON(result, error)
    }

    /// The CMYK proof: RGBA8 in the built-in Display P3 and its info (`space` "display-p3").
    func cmykProof(profile: Data, setupJSON: String, workingLimit: UInt64, otherRetained: UInt64,
                   control: RasterJobControl?) throws -> (image: RasterImage, info: [String: Any]) {
        var error: UnsafeMutablePointer<CChar>?
        var info: UnsafeMutablePointer<CChar>?
        let result = Self.withBytes(profile) { bytes, count in
            dr_precision_doc_cmyk_proof(ptr, bytes, count, setupJSON, workingLimit, otherRetained,
                                        control?.ptr, &info, &error)
        }
        guard let result else {
            throw DoctoredCoreError(message: takeErrorMessage(error, fallback: "Could not render the CMYK proof."))
        }
        var parsed: [String: Any] = [:]
        if let info {
            parsed = (try? JSONSerialization.jsonObject(with: Data(String(cString: info).utf8)))
                as? [String: Any] ?? [:]
            dr_string_free(info)
        }
        return (RasterImage(owning: result), parsed)
    }

    // MARK: - Levels and Curves' tools

    /// The tone analysis's plan: `working_memory_plan_bytes`, `refusal`.
    func cmykToneAnalysisPlan(request: [String: Any]) throws -> [String: Any] {
        var error: UnsafeMutablePointer<CChar>?
        let result = dr_precision_doc_cmyk_tone_analysis_plan(ptr, jsonText(request), &error)
        return try precisionJSON(result, error)
    }

    /// The histograms (256 bins of L = 1 − ink per plate, `cmyk` summed), and as asked Auto's
    /// endpoints and an eyedropper's sample and target. `coverage` empty is no selection.
    func cmykToneAnalysis(request: [String: Any], coverage: [UInt16], workingLimit: UInt64,
                          otherRetained: UInt64, control: RasterJobControl?) throws -> [String: Any] {
        var error: UnsafeMutablePointer<CChar>?
        let text = jsonText(request)
        let result = coverage.withUnsafeBufferPointer { buffer in
            dr_precision_doc_cmyk_tone_analysis(ptr, text, coverage.isEmpty ? nil : buffer.baseAddress,
                                                coverage.count, workingLimit, otherRetained,
                                                control?.ptr, &error)
        }
        return try precisionJSON(result, error)
    }

    /// Curves' ⌥ clipping display of this (candidate) document: `mode` shadows or highlights,
    /// `plates` among c, m, y, k.
    func cmykClipDisplay(mode: String, plates: [String], workingLimit: UInt64,
                         otherRetained: UInt64) throws -> RasterImage {
        var error: UnsafeMutablePointer<CChar>?
        guard let result = dr_precision_doc_cmyk_clip_display(
            ptr, jsonText(["mode": mode, "plates": plates]), workingLimit, otherRetained, nil, &error) else {
            throw DoctoredCoreError(message: takeErrorMessage(error, fallback: "Could not render the clipping display."))
        }
        return RasterImage(owning: result)
    }

    // MARK: - One colour's separation

    /// `dr_cmyk_separate`: one encoded RGB colour (0…1 in `rgbProfile`, nil = sRGB) through
    /// `document`'s CMYK profile, or the working CMYK when `document` is nil or RGB. The intent
    /// and BPC are Color Settings'.
    static func separate(rgb: [Double], rgbProfile: Data?,
                         document: RasterPrecisionDocument?) throws -> CmykSeparation {
        let text = jsonText(["values": rgb, "intent": ColorSettings.intent.rawValue,
                             "black_point_compensation": ColorSettings.blackPointCompensation])
        var error: UnsafeMutablePointer<CChar>?
        let result = withExtendedLifetime(document) {
            withBytes(rgbProfile) { bytes, count in
                dr_cmyk_separate(document?.ptr, bytes, count, text, &error)
            }
        }
        let reply = try precisionJSON(result, error)
        let ink = (reply["ink"] as? [NSNumber])?.map { $0.doubleValue * 100 } ?? []
        guard ink.count == 4 else { throw DoctoredCoreError(message: "The separation has no ink.") }
        return CmykSeparation(
            ink: CmykInk(c: ink[0], m: ink[1], y: ink[2], k: ink[3]),
            outOfGamut: reply["out_of_gamut"] as? Bool ?? false,
            deltaE: (reply["delta_e"] as? NSNumber)?.doubleValue ?? 0,
            inGamut: (reply["in_gamut"] as? [NSNumber])?.map(\.doubleValue) ?? rgb,
            profile: reply["profile"] as? String ?? "")
    }
}
