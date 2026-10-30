import AppKit

// MARK: - Output color control (Phase 11C)

/// What the core can say about profile bytes — the `dr_icc_describe` payload.
/// Unlike `RasterProfileInfo` this reads Gray and CMYK profiles too, because a
/// proof setup may name one.
struct RasterProfileDescription: Equatable {
    let name: String
    let version: String
    /// The four-character profile class (`mntr`, `prtr`, `scnr`, `spac`, …).
    let profileClass: String
    /// `"rgb"`, `"gray"`, `"cmyk"`, `"lab"` or `"other"`.
    let space: String
    /// The profile connection space signature.
    let pcs: String
    /// `"matrix"`, `"lut"` or `"none"`.
    let model: String
    /// The Convert to Profile SOURCE gate.
    let canDecode: Bool
    /// The Convert to Profile and export DESTINATION gate.
    let canEncode: Bool
    let hasPerceptual: Bool
    let hasRelative: Bool
    let hasSaturation: Bool
    let mediaWhite: [Double]
    /// True when a soft proof can run through it: both directions readable.
    let proofable: Bool
    /// Why it cannot be used, naming the element — nil when it can.
    let refusal: String?
    /// Why it cannot be proofed through — nil exactly when `proofable`. Names
    /// the missing A2B/B2A direction and its floating-point twin.
    let proofRefusal: String?
    /// True when a 16/32-bit Convert accepts it as a destination.
    let highDepthDestination: Bool
    /// The core's sentence refusing an export into it (a matrix/TRC profile
    /// that fails the precision check), nil when one converts.
    let exportRefusal: String?
}

extension RasterProfile {
    /// Everything the core can say about `bytes`, or nil when it is not a
    /// profile at all. Ask this to build a profile popup or to explain why a
    /// chosen file cannot be proofed through.
    static func describe(_ bytes: Data) -> RasterProfileDescription? {
        var err: UnsafeMutablePointer<CChar>?
        let text: String? = bytes.withUnsafeBytes { buffer in
            guard let json = dr_icc_describe(
                buffer.bindMemory(to: UInt8.self).baseAddress, buffer.count, &err)
            else { return nil }
            defer { dr_string_free(json) }
            return String(cString: json)
        }
        if let err { dr_string_free(err) }
        guard let text,
              let data = text.data(using: .utf8),
              let root = try? JSONSerialization.jsonObject(with: data) as? [String: Any]
        else { return nil }
        let intents = root["intents"] as? [String: Any] ?? [:]
        guard let name = root["name"] as? String else { return nil }
        return RasterProfileDescription(
            name: name,
            version: root["version"] as? String ?? "",
            profileClass: root["class"] as? String ?? "",
            space: root["space"] as? String ?? "other",
            pcs: root["pcs"] as? String ?? "",
            model: root["model"] as? String ?? "none",
            canDecode: root["can_decode"] as? Bool ?? false,
            canEncode: root["can_encode"] as? Bool ?? false,
            hasPerceptual: intents["perceptual"] as? Bool ?? false,
            hasRelative: intents["relative"] as? Bool ?? false,
            hasSaturation: intents["saturation"] as? Bool ?? false,
            mediaWhite: root["media_white"] as? [Double] ?? [],
            proofable: root["proofable"] as? Bool ?? false,
            refusal: root["refusal"] as? String,
            proofRefusal: root["proof_refusal"] as? String,
            highDepthDestination: root["high_depth_destination"] as? Bool ?? false,
            exportRefusal: root["export_refusal"] as? String)
    }

    /// The whole description object, for `describe_profile`'s MCP payload.
    static func describeJSON(_ bytes: Data) throws -> [String: Any] {
        var err: UnsafeMutablePointer<CChar>?
        let text: String? = bytes.withUnsafeBytes { buffer in
            guard let json = dr_icc_describe(
                buffer.bindMemory(to: UInt8.self).baseAddress, buffer.count, &err)
            else { return nil }
            defer { dr_string_free(json) }
            return String(cString: json)
        }
        guard let text else {
            throw DoctoredCoreError(
                message: takeErrorMessage(err, fallback: "That file is not an ICC profile."))
        }
        if let err { dr_string_free(err) }
        guard let data = text.data(using: .utf8),
              let root = try? JSONSerialization.jsonObject(with: data) as? [String: Any]
        else { throw DoctoredCoreError(message: "The profile description could not be read.") }
        return root
    }
}

extension RasterDocument {
    /// True when the core can turn this document's pixel numbers into PCS
    /// colour for a conversion — at 8 bits a matrix/TRC profile OR a LUT
    /// profile with an A2B table. This is the Convert to Profile source gate;
    /// `profileIsConvertible` keeps its narrower matrix/TRC meaning for the
    /// calibrated adjustments. At 16 and 32 bits a conversion's source must be
    /// matrix/TRC, and ANY matrix/TRC model decodes — one whose curves fail the
    /// precision check converts out through the parser's repairs, so
    /// this is "has a matrix/TRC model", not `profileIsConvertible`.
    var profileCanDecode: Bool {
        guard precision != nil else { return dr_doc_profile_can_decode(ptr) }
        return RasterProfile.inspect(iccProfile ?? Data()).kind == .rgbMatrix
    }

    /// Why an Assign would reinterpret a retained source, and the counts the
    /// sheet's checkbox names.
    var assignRefusal: (reason: String?, retainedSources: Int) {
        let raw: String?
        if let precision {
            raw = precision.assignRefusalJSON
        } else {
            guard let text = dr_doc_assign_refusal(ptr) else { return (nil, 0) }
            defer { dr_string_free(text) }
            raw = String(cString: text)
        }
        guard let raw, let data = raw.data(using: .utf8),
              let root = try? JSONSerialization.jsonObject(with: data) as? [String: Any]
        else { return (nil, 0) }
        return (root["refusal"] as? String, root["retained_sources"] as? Int ?? 0)
    }

    /// Why assigning `bytes` would land a document that cannot render:
    /// the core's sentence when the profile has no matrix/TRC model and this
    /// document holds a `document_v2` Exposure/White Balance layer (named) or
    /// uses linear-light evaluation; the core's Assign sentence naming the
    /// profile when its matrix/TRC tone curves fail the precision check and
    /// this document uses linear-light evaluation, holds text/shape artwork
    /// or holds a `document_v2` Exposure/White Balance layer (named);
    /// nil otherwise, and at 8 bits for bytes the core does not
    /// accept as a profile. At 16 bits it is whatever error
    /// `colorPlan(convert: false)` raises — the same Assign sentences (which
    /// never name a retained source), plus the plan's own profile-parse,
    /// byte-limit and working-memory refusals, each one the commit would
    /// also raise, since the commit plans with the same limit and at least
    /// as much other retained memory — so the sheet and `assign_profile`
    /// answer the profile refusal before the retained-source one at both
    /// depths. At 32 bits the same plan answers the float Assign's
    /// own sentences: a profile with no matrix/TRC model, or one whose tone
    /// curves fail the precision check.
    func assignProfileRefusal(_ bytes: Data) -> String? {
        if let precision {
            do {
                _ = try precision.colorPlan(
                    profile: bytes, convert: false,
                    workingLimit: RasterResources.shared.workingLimit, otherRetained: 0)
                return nil
            } catch { return error.localizedDescription }
        }
        let raw = bytes.withUnsafeBytes { buffer in
            dr_doc_assign_profile_refusal(
                ptr, buffer.bindMemory(to: UInt8.self).baseAddress, buffer.count)
        }
        guard let raw else { return nil }
        defer { dr_string_free(raw) }
        return String(cString: raw)
    }

    /// Rasterizes every layer still bound to a retained source, then assigns —
    /// ONE result the host commits as ONE undo step. nil when nothing would
    /// change. Throws the core's sentence for bytes that are not an RGB ICC
    /// profile, when `assignProfileRefusal` is non-nil, and when a bound
    /// layer's pixels are locked.
    func assigningProfileRasterizing(_ bytes: Data) throws -> RasterDocument? {
        if isHighDepth {
            return try performingPrecisionAdjustment(.assignProfileRasterizing(bytes))
        }
        var err: UnsafeMutablePointer<CChar>?
        let handle = bytes.withUnsafeBytes { raw in
            dr_doc_assign_profile_rasterizing(
                ptr, raw.bindMemory(to: UInt8.self).baseAddress, raw.count, &err)
        }
        return try fallible(handle, err, "Could not assign the profile.")
    }

    /// What a conversion to `bytes` under `options` WOULD do. Throws the
    /// core's refusal sentence, which is what a sheet disables its OK button
    /// with. The baking projections run under `control`: a cancel
    /// throws the core's `operation cancelled`.
    func conversionPreflight(_ bytes: Data, options: ConversionOptions,
                             control: RasterJobControl? = nil) throws -> ConversionReport {
        if let precision {
            let context = try PrecisionWorkContext(source: precision)
            return try precision.conversionPreflight(
                bytes, options: options,
                workingLimit: context.limit, otherRetained: context.otherRetained,
                control: control)
        }
        var err: UnsafeMutablePointer<CChar>?
        let optionsText = options.jsonText
        let text: String? = bytes.withUnsafeBytes { buffer in
            guard let json = dr_doc_conversion_preflight(
                ptr, buffer.bindMemory(to: UInt8.self).baseAddress, buffer.count,
                optionsText, control?.ptr, &err)
            else { return nil }
            defer { dr_string_free(json) }
            return String(cString: json)
        }
        guard let text else {
            throw DoctoredCoreError(
                message: takeErrorMessage(err, fallback: "The conversion was refused."))
        }
        if let err { dr_string_free(err) }
        return ConversionReport.parse(json: text)
    }

    /// TRANSFORMS every layer's pixels into `bytes` under `options`. nil is
    /// the ordinary refusal (nothing would change); a throw is the preflight's
    /// sentence.
    func convertingToProfile(_ bytes: Data, options: ConversionOptions,
                             control: RasterJobControl? = nil)
        throws -> (document: RasterDocument, report: ConversionReport)?
    {
        if isHighDepth {
            // The REPORT comes from the preflight and the DOCUMENT from the
            // captured-edit path, which owns typed admission, the worker and
            // cancellation. The two lists are the same by construction (the
            // core builds the conversion's report by running the preflight),
            // and routing the handle any other way would hand a
            // `PrecisionDocument *` to `RasterDocument(owning:)`.
            let report = try conversionPreflight(bytes, options: options)
            let operation: PrecisionCapturedEdit.Operation = options.isDefault
                ? .convertProfile(bytes) : .convertProfileWith(bytes, options)
            guard let next = try performingPrecisionAdjustment(operation) else { return nil }
            return (next, report)
        }
        var err: UnsafeMutablePointer<CChar>?
        var reportOut: UnsafeMutablePointer<CChar>?
        let optionsText = options.jsonText
        let handle = bytes.withUnsafeBytes { buffer in
            dr_doc_convert_to_profile_with(
                ptr, buffer.bindMemory(to: UInt8.self).baseAddress, buffer.count,
                optionsText, control?.ptr, &reportOut, &err)
        }
        guard let handle else {
            if let err {
                throw DoctoredCoreError(message: takeErrorMessage(err, fallback: "Refused."))
            }
            return nil
        }
        var report = ConversionReport()
        if let reportOut {
            report = ConversionReport.parse(json: String(cString: reportOut))
            dr_string_free(reportOut)
        }
        return (RasterDocument(owning: handle), report)
    }
}

extension RasterPrecisionDocument {
    /// The typed `{"refusal":…}` payload; nil when the handle answers nothing.
    var assignRefusalJSON: String? {
        guard let text = dr_precision_doc_assign_refusal(ptr) else { return nil }
        defer { dr_string_free(text) }
        return String(cString: text)
    }

    func conversionPreflight(_ bytes: Data, options: ConversionOptions,
                             workingLimit: UInt64, otherRetained: UInt64,
                             control: RasterJobControl? = nil)
        throws -> ConversionReport
    {
        var err: UnsafeMutablePointer<CChar>?
        let optionsText = options.jsonText
        let text: String? = bytes.withUnsafeBytes { buffer in
            guard let json = dr_precision_doc_conversion_preflight(
                ptr, buffer.bindMemory(to: UInt8.self).baseAddress, buffer.count,
                optionsText, workingLimit, otherRetained, control?.ptr, &err)
            else { return nil }
            defer { dr_string_free(json) }
            return String(cString: json)
        }
        guard let text else {
            throw DoctoredCoreError(
                message: takeErrorMessage(err, fallback: "The conversion was refused."))
        }
        if let err { dr_string_free(err) }
        return ConversionReport.parse(json: text)
    }

    /// `dr_precision_doc_conversion_plan`: the Convert's own admission plan
    /// under `options`, which charges a preserve-appearance Convert's bakes;
    /// `colorPlan(convert: true)`'s figures otherwise.
    func conversionPlan(profile: Data, options: ConversionOptions, workingLimit: UInt64,
                        otherRetained: UInt64) throws -> [String: Any] {
        var error: UnsafeMutablePointer<CChar>?
        let optionsText = options.jsonText
        let result = profile.withUnsafeBytes { bytes in
            dr_precision_doc_conversion_plan(
                ptr, bytes.bindMemory(to: UInt8.self).baseAddress, bytes.count,
                optionsText, workingLimit, otherRetained, &error)
        }
        return try precisionJSON(result, error)
    }

    func convertingToProfile(_ bytes: Data, options: ConversionOptions,
                             workingLimit: UInt64, otherRetained: UInt64,
                             control: RasterJobControl?)
        throws -> (document: OpaquePointer, report: ConversionReport)?
    {
        var err: UnsafeMutablePointer<CChar>?
        var reportOut: UnsafeMutablePointer<CChar>?
        let optionsText = options.jsonText
        let handle = bytes.withUnsafeBytes { buffer in
            dr_precision_doc_convert_to_profile_with(
                ptr, buffer.bindMemory(to: UInt8.self).baseAddress, buffer.count,
                optionsText, workingLimit, otherRetained, control?.ptr, &reportOut, &err)
        }
        guard let handle else {
            if let err {
                throw DoctoredCoreError(message: takeErrorMessage(err, fallback: "Refused."))
            }
            return nil
        }
        var report = ConversionReport()
        if let reportOut {
            report = ConversionReport.parse(json: String(cString: reportOut))
            dr_string_free(reportOut)
        }
        return (handle, report)
    }

    func assigningProfileRasterizing(_ bytes: Data, workingLimit: UInt64,
                                     otherRetained: UInt64,
                                     control: RasterJobControl?) throws -> OpaquePointer?
    {
        var err: UnsafeMutablePointer<CChar>?
        let handle = bytes.withUnsafeBytes { buffer in
            dr_precision_doc_assign_profile_rasterizing(
                ptr, buffer.bindMemory(to: UInt8.self).baseAddress, buffer.count,
                workingLimit, otherRetained, control?.ptr, &err)
        }
        if handle == nil, let err {
            throw DoctoredCoreError(message: takeErrorMessage(err, fallback: "Refused."))
        }
        if let err { dr_string_free(err) }
        return handle
    }
}

extension RasterImage {
    /// Renders this flat projection through a proof profile. The result is in
    /// the reported SPACE, which the caller must tag the CGImage with. The core
    /// admits this image plus the proof's scratch against `workingLimit`.
    func proofRender(documentProfile: Data, proof: Data, setup: String, workingLimit: UInt64,
                     otherRetained: UInt64, control: RasterJobControl? = nil)
        throws -> (image: RasterImage, space: String, outOfGamut: Int)
    {
        var err: UnsafeMutablePointer<CChar>?
        var infoOut: UnsafeMutablePointer<CChar>?
        let handle = documentProfile.withUnsafeBytes { document in
            proof.withUnsafeBytes { proofBytes in
                dr_proof_render(
                    ptr,
                    document.bindMemory(to: UInt8.self).baseAddress, document.count,
                    proofBytes.bindMemory(to: UInt8.self).baseAddress, proofBytes.count,
                    setup, workingLimit, otherRetained, control?.ptr, &infoOut, &err)
            }
        }
        guard let handle else {
            throw DoctoredCoreError(
                message: takeErrorMessage(err, fallback: "The proof could not be rendered."))
        }
        if let err { dr_string_free(err) }
        let info = RasterImage.proofInfo(infoOut)
        return (RasterImage(owning: handle), info.space, info.outOfGamut)
    }

    /// The per-pixel gamut mask (255 out of gamut) and how many were flagged,
    /// admitted like `proofRender`.
    func proofGamutMask(documentProfile: Data, proof: Data, setup: String, workingLimit: UInt64,
                        otherRetained: UInt64, control: RasterJobControl? = nil)
        throws -> (mask: [UInt8], count: Int)
    {
        let length = width * height
        guard length > 0 else { return ([], 0) }
        var mask = [UInt8](repeating: 0, count: length)
        var err: UnsafeMutablePointer<CChar>?
        var count: UInt64 = 0
        let ok = mask.withUnsafeMutableBufferPointer { out -> Bool in
            documentProfile.withUnsafeBytes { document in
                proof.withUnsafeBytes { proofBytes in
                    dr_proof_gamut_mask(
                        ptr,
                        document.bindMemory(to: UInt8.self).baseAddress, document.count,
                        proofBytes.bindMemory(to: UInt8.self).baseAddress, proofBytes.count,
                        setup, workingLimit, otherRetained, control?.ptr,
                        out.baseAddress, length, &count, &err)
                }
            }
        }
        guard ok else {
            throw DoctoredCoreError(
                message: takeErrorMessage(err, fallback: "The gamut mask could not be built."))
        }
        if let err { dr_string_free(err) }
        return (mask, Int(count))
    }

    /// Color Range ▸ Out Of Gamut: one byte per pixel, this image's alpha where the gamut
    /// warning's rule flags the pixel (inverted when `invert`), else 0. The core admits this image
    /// plus `outOfGamutScratch` against `workingLimit` and reserves that scratch on `control`.
    func outOfGamutCoverage(documentProfile: Data, proof: Data, setup: String, invert: Bool,
                            workingLimit: UInt64, otherRetained: UInt64,
                            control: RasterJobControl? = nil) throws -> [UInt8]
    {
        let length = width * height
        guard length > 0 else { return [] }
        var coverage = [UInt8](repeating: 0, count: length)
        var err: UnsafeMutablePointer<CChar>?
        let ok = coverage.withUnsafeMutableBufferPointer { out -> Bool in
            documentProfile.withUnsafeBytes { document in
                proof.withUnsafeBytes { proofBytes in
                    dr_selection_out_of_gamut(
                        ptr,
                        document.bindMemory(to: UInt8.self).baseAddress, document.count,
                        proofBytes.bindMemory(to: UInt8.self).baseAddress, proofBytes.count,
                        setup, invert, workingLimit, otherRetained, control?.ptr,
                        out.baseAddress, length, &err)
                }
            }
        }
        guard ok else {
            throw DoctoredCoreError(
                message: takeErrorMessage(err, fallback: "Out Of Gamut could not be selected."))
        }
        if let err { dr_string_free(err) }
        return coverage
    }

    /// The bytes `outOfGamutCoverage` admits beyond its image for a `width` x `height` image: the
    /// gamut mask's scratch plus the output, computed in the core. 0 when `proof` does not parse.
    static func outOfGamutScratch(width: Int, height: Int, documentProfileBytes: Int,
                                  proof: Data) -> UInt64
    {
        guard width > 0, height > 0, width <= Int(UInt32.max), height <= Int(UInt32.max)
        else { return 0 }
        return proof.withUnsafeBytes { proofBytes in
            dr_selection_out_of_gamut_scratch(
                UInt32(width), UInt32(height), documentProfileBytes,
                proofBytes.bindMemory(to: UInt8.self).baseAddress, proofBytes.count)
        }
    }

    fileprivate static func proofInfo(
        _ raw: UnsafeMutablePointer<CChar>?
    ) -> (space: String, outOfGamut: Int) {
        guard let raw else { return ("document", 0) }
        defer { dr_string_free(raw) }
        guard let data = String(cString: raw).data(using: .utf8),
              let root = try? JSONSerialization.jsonObject(with: data) as? [String: Any]
        else { return ("document", 0) }
        return (root["space"] as? String ?? "document", root["out_of_gamut"] as? Int ?? 0)
    }
}

extension RasterPrecisionImage {
    /// The typed twin: a u16 projection in, an RGBA8 display image out.
    func proofRender(proof: Data, setup: String, workingLimit: UInt64,
                     otherRetained: UInt64, control: RasterJobControl? = nil)
        throws -> (image: RasterImage, space: String, outOfGamut: Int)
    {
        var err: UnsafeMutablePointer<CChar>?
        var infoOut: UnsafeMutablePointer<CChar>?
        let handle = proof.withUnsafeBytes { proofBytes in
            dr_precision_proof_render(
                ptr,
                proofBytes.bindMemory(to: UInt8.self).baseAddress, proofBytes.count,
                setup, workingLimit, otherRetained, control?.ptr, &infoOut, &err)
        }
        guard let handle else {
            throw DoctoredCoreError(
                message: takeErrorMessage(err, fallback: "The proof could not be rendered."))
        }
        if let err { dr_string_free(err) }
        let info = RasterImage.proofInfo(infoOut)
        return (RasterImage(owning: handle), info.space, info.outOfGamut)
    }

    func proofGamutMask(proof: Data, setup: String, workingLimit: UInt64,
                        otherRetained: UInt64, control: RasterJobControl? = nil)
        throws -> (mask: [UInt8], count: Int)
    {
        let length = width * height
        guard length > 0 else { return ([], 0) }
        var mask = [UInt8](repeating: 0, count: length)
        var err: UnsafeMutablePointer<CChar>?
        var count: UInt64 = 0
        let ok = mask.withUnsafeMutableBufferPointer { out -> Bool in
            proof.withUnsafeBytes { proofBytes in
                dr_precision_proof_gamut_mask(
                    ptr,
                    proofBytes.bindMemory(to: UInt8.self).baseAddress, proofBytes.count,
                    setup, workingLimit, otherRetained, control?.ptr,
                    out.baseAddress, length, &count, &err)
            }
        }
        guard ok else {
            throw DoctoredCoreError(
                message: takeErrorMessage(err, fallback: "The gamut mask could not be built."))
        }
        if let err { dr_string_free(err) }
        return (mask, Int(count))
    }
}
