import AppKit

// The CMYK document's core wrappers (#547 phase 1): every `dr_*` call of the CMYK area. The
// vocabulary — `CmykPlane`, `CmykSource`, `CmykConversionOptions` — lives here beside them, and
// the gate that decides what a CMYK document admits is `CmykDocument.swift`.

/// One ink plane of a CMYK document, the Channels panel's rows and the `render channel:`
/// argument (`dr_precision_doc_cmyk_plane_view`'s `plane`).
enum CmykPlane: Int, CaseIterable {
    case cyan, magenta, yellow, black

    var name: String {
        switch self {
        case .cyan: return "Cyan"
        case .magenta: return "Magenta"
        case .yellow: return "Yellow"
        case .black: return "Black"
        }
    }

    /// The MCP spelling (`render`'s `channel`).
    var key: String { name.lowercased() }

    init?(key: String) {
        guard let plane = CmykPlane.allCases.first(where: { $0.key == key }) else { return nil }
        self = plane
    }
}

/// `DR_CMYK_SOURCE_*`: which container a file opens as a CMYK document from.
enum CmykSource: Int32 {
    case none = 0, tiff = 1, jpeg = 2
}

/// Image ▸ Mode's conversion options, as the core reads them. The intent and BPC start at Edit ▸
/// Color Settings' Conversion Options (relative with BPC by default).
struct CmykConversionOptions {
    var intent: RenderingIntent = ColorSettings.intent
    var blackPointCompensation = ColorSettings.blackPointCompensation
    var flatten = false
    /// 8 or 16 toward CMYK; nil keeps the core's default (16).
    var depth: Int?

    var json: String {
        var object: [String: Any] = ["intent": intent.rawValue,
                                     "black_point_compensation": blackPointCompensation,
                                     "flatten": flatten]
        if let depth { object["depth"] = depth }
        guard let data = try? JSONSerialization.data(withJSONObject: object, options: [.sortedKeys]),
              let text = String(data: data, encoding: .utf8) else { return "{}" }
        return text
    }
}

extension RasterPrecisionDocument {
    var isCmyk: Bool { dr_precision_doc_is_cmyk(ptr) }

    /// `{"profile","depth","storage","view_profile"}`; nil for an RGB document.
    func cmykInfo() -> [String: Any]? {
        guard let raw = dr_precision_doc_cmyk_info(ptr) else { return nil }
        defer { dr_string_free(raw) }
        return (try? JSONSerialization.jsonObject(with: Data(String(cString: raw).utf8)))
            as? [String: Any]
    }

    /// The CMYK profile's bytes, verbatim; empty for an RGB document.
    var cmykProfileData: Data {
        var count = 0
        guard let bytes = dr_precision_doc_cmyk_profile(ptr, &count) else { return Data() }
        return withExtendedLifetime(self) { Data(bytes: bytes, count: count) }
    }

    /// Which CMYK container `url` is, header work only.
    static func cmykSource(at url: URL) -> CmykSource {
        CmykSource(rawValue: dr_path_cmyk_source(url.path)) ?? .none
    }

    private static func withProfile<T>(_ profile: Data?,
                                       _ body: (UnsafePointer<UInt8>?, Int) throws -> T) rethrows -> T {
        guard let profile, !profile.isEmpty else { return try body(nil, 0) }
        return try profile.withUnsafeBytes { raw in
            try body(raw.bindMemory(to: UInt8.self).baseAddress, profile.count)
        }
    }

    /// Image ▸ Mode ▸ CMYK Color's plan: `working_memory_plan_bytes`, `refusal`, `flatten_required`.
    func convertToCMYKPlan(profile: Data?, options: CmykConversionOptions) throws -> [String: Any] {
        var error: UnsafeMutablePointer<CChar>?
        let result = Self.withProfile(profile) { bytes, count in
            dr_precision_doc_convert_to_cmyk_plan(ptr, bytes, count, options.json, &error)
        }
        return try precisionJSON(result, error)
    }

    /// Image ▸ Mode ▸ CMYK Color. `profile` nil is the working CMYK profile.
    func convertToCMYK(profile: Data?, options: CmykConversionOptions, workingLimit: UInt64,
                       otherRetained: UInt64, control: RasterJobControl?) throws -> RasterPrecisionDocument {
        var error: UnsafeMutablePointer<CChar>?
        let result = Self.withProfile(profile) { bytes, count in
            dr_precision_doc_convert_to_cmyk(ptr, bytes, count, options.json, workingLimit,
                                             otherRetained, control?.ptr, &error)
        }
        guard let result else {
            throw DoctoredCoreError(message: takeErrorMessage(error, fallback: "Could not convert to CMYK."))
        }
        return RasterPrecisionDocument(owning: result)
    }

    /// Image ▸ Mode ▸ RGB Color's plan.
    func convertCMYKToRGBPlan(profile: Data?, options: CmykConversionOptions) throws -> [String: Any] {
        var error: UnsafeMutablePointer<CChar>?
        let result = Self.withProfile(profile) { bytes, count in
            dr_precision_doc_convert_cmyk_to_rgb_plan(ptr, bytes, count, options.json, &error)
        }
        return try precisionJSON(result, error)
    }

    /// Image ▸ Mode ▸ RGB Color: a 16-bit RGB document, layers kept. `profile` nil is sRGB.
    func convertCMYKToRGB(profile: Data?, options: CmykConversionOptions, workingLimit: UInt64,
                          otherRetained: UInt64, control: RasterJobControl?) throws -> RasterPrecisionDocument {
        var error: UnsafeMutablePointer<CChar>?
        let result = Self.withProfile(profile) { bytes, count in
            dr_precision_doc_convert_cmyk_to_rgb(ptr, bytes, count, options.json, workingLimit,
                                                 otherRetained, control?.ptr, &error)
        }
        guard let result else {
            throw DoctoredCoreError(message: takeErrorMessage(error, fallback: "Could not convert to RGB."))
        }
        return RasterPrecisionDocument(owning: result)
    }

    /// The composite's ink at a point (`ink_percent`, `ink16`, `alpha16`, `total_ink_percent`);
    /// the stack's masks are admitted against `workingLimit` beside `otherRetained`.
    func cmykSample(x: Int, y: Int, reach: Int, workingLimit: UInt64,
                    otherRetained: UInt64) throws -> [String: Any] {
        var error: UnsafeMutablePointer<CChar>?
        let result = dr_precision_doc_cmyk_sample(ptr, Int32(clamping: x), Int32(clamping: y),
                                                  UInt32(clamping: reach), workingLimit, otherRetained,
                                                  &error)
        return try precisionJSON(result, error)
    }

    /// One ink plane of the composite on paper as opaque gray, ink dark; `maxSide` 0 is full size.
    func cmykPlaneView(_ plane: CmykPlane, maxSide: Int, workingLimit: UInt64,
                       otherRetained: UInt64) throws -> RasterImage {
        var error: UnsafeMutablePointer<CChar>?
        guard let result = dr_precision_doc_cmyk_plane_view(
            ptr, UInt32(plane.rawValue), UInt32(clamping: maxSide), workingLimit, otherRetained,
            nil, &error) else {
            throw DoctoredCoreError(message: takeErrorMessage(error, fallback: "Could not render the ink plane."))
        }
        return RasterImage(owning: result)
    }

    /// Record `index`'s Layers-panel thumbnail through the CMYK view.
    func cmykLayerThumbnail(_ index: Int, maxSide: Int, workingLimit: UInt64,
                            otherRetained: UInt64) throws -> RasterImage {
        var error: UnsafeMutablePointer<CChar>?
        guard let result = dr_precision_doc_cmyk_layer_thumbnail_rgba8(
            ptr, index, UInt32(clamping: maxSide), workingLimit, otherRetained, nil, &error) else {
            throw DoctoredCoreError(message: takeErrorMessage(error, fallback: "Could not render the layer thumbnail."))
        }
        return RasterImage(owning: result)
    }

    /// Record `index`'s pixel identity; 0 when it has none.
    func cmykLayerIdentity(_ index: Int) -> UInt64 {
        dr_precision_doc_cmyk_layer_identity(ptr, index)
    }

    /// The CMYK save's plan for one request.
    func saveCMYKPlan(format: String, embedProfile: Bool, stripMetadata: Bool) throws -> [String: Any] {
        var error: UnsafeMutablePointer<CChar>?
        let result = dr_precision_doc_save_cmyk_plan(
            ptr, Self.saveRequest(format, embedProfile, stripMetadata), &error)
        return try precisionJSON(result, error)
    }

    /// Writes the composite flattened onto paper: `format` tiff, psd or jpeg_intermediate.
    func saveCMYK(to url: URL, format: String, embedProfile: Bool, stripMetadata: Bool,
                  workingLimit: UInt64, otherRetained: UInt64) throws {
        var error: UnsafeMutablePointer<CChar>?
        guard dr_precision_doc_save_cmyk(ptr, url.path, Self.saveRequest(format, embedProfile, stripMetadata),
                                         workingLimit, otherRetained, nil, &error) else {
            throw DoctoredCoreError(message: takeErrorMessage(error, fallback: "Could not save the CMYK file."))
        }
    }

    private static func saveRequest(_ format: String, _ embed: Bool, _ strip: Bool) -> String {
        "{\"format\":\"\(format)\",\"embed_profile\":\(embed),\"strip_metadata\":\(strip)}"
    }

    // MARK: - Select and Mask's New Layer (#547)

    /// The CMYK cutout's plan (`dr_precision_doc_cmyk_cutout_plan`): the RGB cutout plan's keys
    /// with `foreground_storage` `cmyka16_unorm`; `decontaminate` 0 (off) or a strength in (0, 1].
    func cmykCutoutPlan(decontaminate: Double) throws -> [String: Any] {
        var error: UnsafeMutablePointer<CChar>?
        let result = dr_precision_doc_cmyk_cutout_plan(ptr, Float(decontaminate), &error)
        return try precisionJSON(result, error)
    }

    /// Select and Mask's CMYK New Layer (`dr_precision_doc_add_cmyk_cutout`): the composite's
    /// ink on a new top CMYK raster masked by `matte` (canvas-sized u16 coverage), the other roots
    /// hidden. nil when no pixel has both composite alpha and coverage.
    func addingCmykCutout(matte: [UInt16], decontaminate: Double, workingLimit: UInt64,
                          otherRetained: UInt64, control: RasterJobControl?) throws -> RasterPrecisionDocument? {
        let plan = try cmykCutoutPlan(decontaminate: decontaminate)
        let dimensions = (plan["dimensions"] as? [NSNumber])?.map(\.intValue) ?? []
        guard dimensions.count == 2, matte.count == dimensions[0] * dimensions[1] else {
            throw DoctoredCoreError(message: "Native refinement coverage must match the canvas.")
        }
        var error: UnsafeMutablePointer<CChar>?
        let result = matte.withUnsafeBufferPointer { samples in
            dr_precision_doc_add_cmyk_cutout(ptr, samples.baseAddress, Float(decontaminate),
                                             workingLimit, otherRetained, control?.ptr, &error)
        }
        return try editResult(result, error)
    }

    // MARK: - One colour across the CMYK boundary (#547 phase 2)

    /// `dr_cmyk_color`: one encoded sRGB colour (0…1) as ink in `document`'s CMYK profile, or in
    /// the working CMYK profile when `document` is nil or RGB (refused by name when none is set).
    static func cmykInk(fromSRGB rgb: [Double], document: RasterPrecisionDocument?) throws -> CmykInk {
        let values = try cmykColor(["from": "srgb", "values": rgb], document: document, key: "ink")
        guard values.count == 4 else { throw DoctoredCoreError(message: "The CMYK colour reply has no ink.") }
        return CmykInk(c: values[0] * 100, m: values[1] * 100, y: values[2] * 100, k: values[3] * 100)
    }

    /// `dr_cmyk_color`: ink (percent) as encoded sRGB 0…1, through the same profile rule.
    static func srgb(fromInk ink: CmykInk, document: RasterPrecisionDocument?) throws -> [Double] {
        let values = try cmykColor(["from": "ink", "values": ink.unit], document: document, key: "srgb")
        guard values.count == 3 else { throw DoctoredCoreError(message: "The CMYK colour reply has no sRGB.") }
        return values
    }

    private static func cmykColor(_ request: [String: Any], document: RasterPrecisionDocument?,
                                  key: String) throws -> [Double] {
        guard let data = try? JSONSerialization.data(withJSONObject: request),
              let text = String(data: data, encoding: .utf8) else {
            throw DoctoredCoreError(message: "Could not encode the CMYK colour request.")
        }
        var error: UnsafeMutablePointer<CChar>?
        let result = withExtendedLifetime(document) { dr_cmyk_color(document?.ptr, text, &error) }
        let reply = try precisionJSON(result, error)
        return (reply[key] as? [NSNumber])?.map(\.doubleValue) ?? []
    }
}
