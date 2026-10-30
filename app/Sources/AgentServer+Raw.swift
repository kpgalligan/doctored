import Foundation

/// The MCP mirror of the RAW Develop dialog (`RawDevelopWindowController`):
/// `open_document`'s optional `raw` object.
///
/// Initial open resolves against the captured file. redevelop_raw reuses this
/// parser but patches saved effective values, preserving unspecified settings.
/// `output_encoding` (`sdr16` | `linear_float`) mirrors the dialog's
/// Document depth row on an open ("32-bit float (linear)") and its Development
/// row on a Redevelop or a Place in a 32-bit float document.
/// Programmatic opens are headless so they cannot hang on a Develop dialog.
///
/// Validation is deliberately STRICTER than the framework, matching the
/// `adjustmentParams` precedent: `CIRAWFilter` stores an out-of-range write
/// verbatim and renders garbage from it, so a bad value is refused BY NAME
/// rather than clamped, and a model can correct itself. The ranges refused
/// against are the framework's own documented ones — 30000 K is accepted
/// because the framework honours it — never a slider-convenience number.
/// The dialog clamps silently instead, because a slider cannot leave its
/// track.
extension AgentServer {
    /// The `raw` object parsed into a develop request, or nil when the call
    /// carries none. A present-but-empty object is a request in its own
    /// right — "develop as shot" — and is deliberately distinguishable from
    /// absence, which is why this is optional rather than defaulted here.
    func rawOpenSettings(_ a: [String: Any]) throws -> RawDevelopSettings? {
        guard let raw = a["raw"], !(raw is NSNull) else { return nil }
        guard let object = raw as? [String: Any] else {
            throw ToolError(message: "raw must be an object of develop settings")
        }
        let unknown = object.keys.filter { !Self.rawKeys.contains($0) }.sorted()
        if let first = unknown.first {
            throw ToolError(
                message: "Unknown raw setting \"\(first)\" — raw takes "
                    + "\(Self.rawKeys.sorted().joined(separator: ", ")).")
        }
        var settings = RawDevelopSettings()
        settings.baselineCompensation = try rawNumber(object, "baseline_compensation", -4...4)
        settings.moire = try rawNumber(object, "moire", 0...1)
        if let value = object["vignette"] {
            guard let vignette = value as? [String: Any] else {
                throw ToolError(message: "raw.vignette must contain edge_exposure, center:[x,y] and falloff.")
            }
            settings.vignette = try RawLocalJSON.vignette(vignette)
        }
        if let value = object["grain"] {
            guard let grain = value as? [String: Any] else {
                throw ToolError(message: "raw.grain must contain amount (0…100), size (0.5…4) and roughness (0…100).")
            }
            settings.grain = try RawGrainSettings.parse(grain)
        }
        settings.texture = try rawNumber(object, "texture", RawPresenceSettings.range)
        settings.clarity = try rawNumber(object, "clarity", RawPresenceSettings.range)
        settings.dehaze = try rawNumber(object, "dehaze", RawPresenceSettings.range)
        if let value = object["source_geometry"] {
            guard let lens = value as? [String: Any],
                  Set(lens.keys).isSubset(of: ["profile_id", "center", "radial", "red_scale",
                    "blue_scale", "filter", "reference_size", "reference_from_source"]) else {
                throw ToolError(message: "raw.source_geometry requires a strict lens object.")
            }
            settings.sourceGeometry = try JSONDecoder().decode(RawLensGeometry.self,
                from: JSONSerialization.data(withJSONObject: lens))
            try settings.sourceGeometry?.validate()
        }
        if let value = object["output_space"], !(value is NSNull) {
            guard let name = value as? String, let space = RawOutputSpace(rawValue: name) else {
                throw ToolError(message: "raw.output_space must be \(RawOutputSpace.acceptedNames) (normalized SDR).")
            }
            settings.outputSpace = space
        }
        // Mirrors Develop ▸ Document depth ("32-bit float (linear)") on an
        // open and Layer ▸ Redevelop RAW… ▸ Development on a layer.
        if let value = object["output_encoding"], !(value is NSNull) {
            guard let name = value as? String, let encoding = RawOutputEncoding(rawValue: name) else {
                throw ToolError(
                    message: "raw.output_encoding must be \(RawOutputEncoding.acceptedNames).")
            }
            settings.outputEncoding = encoding
        }
        settings.exposure = try rawNumber(object, "exposure", RawDevelopRange.exposure)
        settings.temperature = try rawNumber(
            object, "temperature", RawDevelopRange.temperature)
        settings.tint = try rawNumber(object, "tint", RawDevelopRange.tint)
        settings.asShotWhiteBalance = try rawFlag(object, "as_shot_white_balance")
        if settings.asShotWhiteBalance == true, settings.temperature != nil || settings.tint != nil {
            throw ToolError(message: "as_shot_white_balance cannot be combined with temperature or tint.")
        }
        settings.toneCurve = try rawNumber(object, "tone_curve", RawDevelopRange.toneCurve)
        settings.shadows = try rawNumber(object, "shadows", RawDevelopRange.shadows)
        settings.contrast = try rawNumber(object, "contrast", RawDevelopRange.contrast)
        settings.sharpness = try rawNumber(object, "sharpness", RawDevelopRange.sharpness)
        settings.detail = try rawNumber(object, "detail", RawDevelopRange.detail)
        settings.luminanceNoise = try rawNumber(
            object, "luminance_noise", RawDevelopRange.luminanceNoise)
        settings.colorNoise = try rawNumber(object, "color_noise", RawDevelopRange.colorNoise)
        settings.lensCorrection = try rawFlag(object, "lens_correction")
        if let recovery = try rawFlag(object, "highlight_recovery") {
            // Refused by name rather than ignored: the property pair behind
            // it is NS_AVAILABLE(16_0, 19_0), so on macOS 15 there is no way
            // to honour the request and silently dropping it would report a
            // develop that did not happen.
            guard #available(macOS 26.0, *) else {
                throw ToolError(message: "raw.highlight_recovery needs macOS 26 or newer")
            }
            settings.highlightRecovery = recovery
        }
        return settings
    }

    /// What to tell a caller who passed `raw` for a file that was not
    /// developed as one: the settings were parsed and then ignored, which is
    /// invisible otherwise. nil when there is nothing to say.
    ///
    /// Keyed on the OUTCOME — `ImageDocument.rawDevelop` is non-nil exactly
    /// when the develop ran — and not on the file's UTType, which answers a
    /// different question. `ImageDocument.openDocument` takes the RAW branch
    /// only when `RawImage.inspect` also succeeds, and its own comment names
    /// the fall-through cases (a linear DNG, a mis-extensioned file): there
    /// the core decodes the file with no develop at all while the type still
    /// says camera RAW, so a type test reported success on pixels the
    /// settings never touched.
    func rawOpenNote(requested: RawDevelopSettings?, document: ImageDocument) -> String? {
        guard requested != nil, document.rawDevelop == nil else { return nil }
        let name = document.fileURL?.lastPathComponent ?? document.displayName ?? "that file"
        return "raw settings were ignored: \(name) was not developed as a camera RAW"
    }

    /// The same note for `open_document`'s ALREADY-OPEN short circuit, where
    /// there are two different things to say and only one of them is about
    /// camera RAW.
    ///
    /// Keyed on the document, exactly as `rawOpenNote` is. Keying it on
    /// `raw != nil` alone asserted that the file was a RAW whatever it
    /// actually was, so `open_document {path: "…/plasma.jpg", raw: {…}}` on an
    /// open JPEG answered "a camera RAW is developed once — close it first",
    /// and a model that followed that advice closed the file, reopened it and
    /// was told the other thing ("was not developed as a camera RAW"). A
    /// document that carries no develop gets that second sentence the first
    /// time, and reopening is not suggested for a file reopening cannot help.
    func rawAlreadyOpenNote(
        requested: RawDevelopSettings?, document: ImageDocument
    ) -> String? {
        guard requested != nil else { return nil }
        guard document.rawDevelop != nil else {
            return rawOpenNote(requested: requested, document: document)
        }
        return "raw settings were ignored: that file is already open. Use redevelop_raw on its retained RAW layer to revise development without rebuilding retouch layers."
    }

    /// Every key the `raw` object accepts, so a typo is refused by name
    /// instead of silently developing as shot.
    private static let rawKeys: Set<String> = [
        "exposure", "temperature", "tint", "tone_curve", "shadows", "contrast",
        "sharpness", "detail", "luminance_noise", "color_noise", "lens_correction",
        "highlight_recovery", "as_shot_white_balance", "output_space", "output_encoding",
        "baseline_compensation", "moire", "vignette", "source_geometry", "grain",
        "texture", "clarity", "dehaze",
    ]

    /// One numeric setting: nil when absent (or JSON null), refused by name
    /// when it is not a finite number inside `range`. A JSON boolean is
    /// refused too — it reads as 1 or 0 through NSNumber and would silently
    /// develop at a value nobody asked for.
    private func rawNumber(
        _ object: [String: Any], _ key: String, _ range: ClosedRange<Double>
    ) throws -> Double? {
        guard let raw = object[key], !(raw is NSNull) else { return nil }
        guard !Self.isJSONBoolean(raw), let value = doubleArg(object, key), value.isFinite else {
            throw ToolError(message: "raw.\(key) must be a number")
        }
        guard range.contains(value) else {
            throw ToolError(
                message: "raw.\(key) must be between \(Self.rawNumberText(range.lowerBound)) "
                    + "and \(Self.rawNumberText(range.upperBound)) "
                    + "(got \(Self.rawNumberText(value)))")
        }
        return value
    }

    /// One boolean setting, with the same absent/refused rules.
    private func rawFlag(_ object: [String: Any], _ key: String) throws -> Bool? {
        guard let raw = object[key], !(raw is NSNull) else { return nil }
        guard let flag = boolArg(object, key) else {
            throw ToolError(message: "raw.\(key) must be true or false")
        }
        return flag
    }

    /// A number for a refusal message: "4", not "4.0".
    ///
    /// The bound is what keeps `Int(_:)` — which traps on a value outside
    /// Int's range — away from the very out-of-range number being reported;
    /// 1e9 is far above every range in `RawDevelopRange` and far below where
    /// the conversion could fail, so nothing a caller can send reaches it.
    private static func rawNumberText(_ value: Double) -> String {
        guard value.isFinite, abs(value) < 1e9, value == value.rounded() else {
            return String(format: "%g", value)
        }
        return String(Int(value))
    }
}
