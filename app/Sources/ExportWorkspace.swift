import AppKit
import CoreFoundation

// An export changes NOTHING: no `applyEdit`, no `updateChangeCount`, no history entry, no archive
// state — and the new options (size, profile, sharpening) are sheet state, never stored on the
// document. The ONE exception, and it is deliberate: the sheet's successful write puts
// `jpegExportQuality` / `embedColorProfile` / `stripMetadata` (plus `exportBitDepth` /
// `exportDither` on a 16-bit document) back on the `ImageDocument` exactly as the deleted
// `exportDocument` / `exportPrecisionDocument` did, because `CapturedSave` reads all five for every
// Save/Save As and those two were their only UI writers; no `updateChangeCount`, then
// `history.refreshCurrentViewState()` so Undo cannot revert the preference. MCP and Quick Export
// never write them (`save_copy`'s precedent). `ExportPresetLibrary` is the
// `PreferenceLibrary` contract at 64 × 4 KiB under `DR_EXPORT_PRESETS_DIR`, with the Quick
// Export default in a hidden `.default` file. Nothing overwrites a file except the sheet's save
// panel, which already asked, and no destination folder is ever created. `quick_export` records ONE
// resolved step from `ImageDocument.quickExport`, so all seven tool names are `nonRecordable` for
// the generic agent recorder.
//
// `ExportPreset` is schema 2 for an optional `metadata_policy` NAME and still WRITES schema 1 when
// it names none, so an older library is byte-identical; `resolvedSettings()` resolves the name at
// delivery time, and a recorded step carries the resolved policy rather than the name
// (`quick_export`'s precedent).
//
// `ExportCapture.hostFinish` is the ONE place `HostImageEncoder` runs, at the three encode sites
// (`ExportCapture.encode`, `LayerExport.encodeLayer`, `ContactSheet.deliver`), so every consumer of
// a result's bytes, preview or info sees an ADOPTED result and the core's pending-write refusal is
// never reached in normal use.

/// The Phase 11A export workspace's model half: the settings a deliverable is
/// described by, the bounded versioned preset library that stores them beside
/// a destination folder and a naming pattern, the capture the sheet and MCP
/// both encode through, and File ▸ Quick Export.
///
/// **Nothing here changes a document.** An export reads the captured
/// immutable handle and its warm projection and produces BYTES; there is no
/// `applyEdit`, no `updateChangeCount`, no history entry and no archive
/// state. The new options — output size, destination profile, output
/// sharpening — are sheet state seeded from `ExportSettings.defaults(for:)`
/// or from a preset, and are deliberately never stored on the document: they
/// describe one delivery, not the picture.

// MARK: - Settings

/// How the output dimensions are derived. Aspect ratio is always kept; a crop
/// is a document edit, not a delivery choice.
enum OutputSize: Equatable {
    case none
    case width(Int)
    case height(Int)
    case longEdge(Int)
    case scale(Double)

    var json: [String: Any] {
        switch self {
        case .none: return ["mode": "none"]
        case .width(let value): return ["mode": "width", "value": value]
        case .height(let value): return ["mode": "height", "value": value]
        case .longEdge(let value): return ["mode": "long_edge", "value": value]
        case .scale(let value): return ["mode": "scale", "value": value]
        }
    }

    static func decode(_ object: [String: Any]) throws -> OutputSize {
        guard let mode = object["mode"] as? String else {
            throw DoctoredCoreError(message: "An export size needs a mode.")
        }
        if mode == "none" { return .none }
        guard let number = (object["value"] as? NSNumber)?.doubleValue, number.isFinite else {
            throw DoctoredCoreError(message: "An export size mode “\(mode)” needs a value.")
        }
        switch mode {
        case "width": return .width(Int(number))
        case "height": return .height(Int(number))
        case "long_edge": return .longEdge(Int(number))
        case "scale": return .scale(number)
        default:
            throw DoctoredCoreError(
                message: "An export size mode must be none, width, height, long_edge or scale.")
        }
    }
}

/// The destination colour space: the document's own, one of the two
/// built-ins, or (Phase 11C) any RGB profile on disk the core can encode
/// into — a file chosen in the sheet or one named out of the ColorSync and
/// Adobe folders.
///
/// It is no longer `String`/`CaseIterable`: a `.file` case carries a path and
/// a name, which no raw value can. `builtins` is the three fixed rows and
/// `requestName` the spelling the core's request takes.
enum OutputProfile: Equatable {
    case document
    case srgb
    case displayP3
    /// A profile read from `path`. The NAME is carried so a popup and a
    /// preset can show it without re-reading the file.
    case file(path: String, name: String)

    /// The three rows that need no file — what `allCases` used to be.
    static let builtins: [OutputProfile] = [.document, .srgb, .displayP3]

    /// The `profile` value in the core's export request.
    var requestName: String {
        switch self {
        case .document: return "document"
        case .srgb: return "srgb"
        case .displayP3: return "display-p3"
        case .file: return "file"
        }
    }

    var displayName: String {
        switch self {
        case .document: return "Document profile"
        case .srgb: return "sRGB"
        case .displayP3: return "Display P3"
        case .file(_, let name): return name
        }
    }

    /// The absolute path, for a `.file` destination only.
    var path: String? {
        if case .file(let path, _) = self { return path }
        return nil
    }

    /// Why the export cannot convert into this destination, or nil:
    /// a `.file` whose profile is a matrix/TRC model failing the precision
    /// check. Every other destination, and a file that cannot be read or
    /// described, answers nil — the core reports those.
    var destinationRefusal: String? {
        guard case .file(let path, _) = self,
              let bytes = try? Data(contentsOf: URL(fileURLWithPath: path)),
              let description = RasterProfile.describe(bytes)
        else { return nil }
        return description.exportDestinationRefusal
    }

    /// The profile bytes the exported file carries, for the preview's tag and
    /// for the dropped-packet notice.
    func profileData(for document: RasterDocument) -> Data? {
        switch self {
        // #547: a CMYK document's "Document profile" export is sRGB pixels, or
        // a kept-CMYK JPEG/TIFF (at any size, sharpened or not, 8 or 16 bits)
        // whose preview the core decodes into sRGB, so the preview's tag is
        // sRGB either way.
        case .document where document.isCmyk: return RasterProfile.builtin(.sRGB)
        // #547 Duotone phase 1b: a Duotone document's export is RGB through its inks in the
        // document's own profile, the twin (`duotone_document`'s view).
        case .document where document.isDuotone: return document.iccProfile
        // #547 phase 1c: a Grayscale document's kept-gray export decodes into sRGB the same way
        // (`open_profile::gray_to_srgb8`).
        case .document where document.isGray: return RasterProfile.builtin(.sRGB)
        // #547 Lab phase 1b: a Lab Color document's export is RGB, and its "Document profile" writes
        // sRGB (`ExportRequest::destination_for`).
        case .document where document.isLab: return RasterProfile.builtin(.sRGB)
        case .document: return document.iccProfile
        case .srgb: return RasterProfile.builtin(.sRGB)
        case .displayP3: return RasterProfile.builtin(.displayP3)
        case .file(let path, _): return try? Data(contentsOf: URL(fileURLWithPath: path))
        }
    }
}

/// The optional output-sharpening stage, off by default: the shipped 9B
/// Unsharp Mask controls, or Smart Sharpen with the filter's own
/// settings (`SmartSharpen.swift`). `json` is the core's `sharpen` object, so
/// a preset records the method and its settings; an Unsharp Mask is written
/// without `method`, as every earlier preset holds it.
enum OutputSharpen: Equatable {
    case unsharpMask(radius: Double, amount: Double, threshold: Double)
    case smartSharpen(SmartSharpenSettings)

    /// The sheet's Unsharp Mask starting point.
    static let unsharpDefault = OutputSharpen.unsharpMask(radius: 1, amount: 0.8, threshold: 0.02)

    var json: [String: Any] {
        switch self {
        case let .unsharpMask(radius, amount, threshold):
            return ["radius": radius, "amount": amount, "threshold": threshold]
        case .smartSharpen(let settings):
            var object = settings.params
            object["method"] = "smart_sharpen"
            return object
        }
    }

    /// Whether the stage changes the deliverable: an Unsharp Mask with a
    /// radius and an amount, or any Smart Sharpen (never the core's bypass).
    var isActive: Bool {
        switch self {
        case let .unsharpMask(radius, amount, _): return radius > 0 && amount > 0
        case .smartSharpen: return true
        }
    }

    static func decode(_ object: [String: Any]) throws -> OutputSharpen {
        // No method is Unsharp Mask, as every preset from before Smart Sharpen holds it.
        let method: String? = object["method"] == nil ? "unsharp_mask" : object["method"] as? String
        if method == "smart_sharpen" {
            return .smartSharpen(try SmartSharpenSettings(params: object.filter { $0.key != "method" }))
        }
        guard method == "unsharp_mask" else {
            throw DoctoredCoreError(
                message: "Export sharpening's method must be unsharp_mask or smart_sharpen.")
        }
        func value(_ key: String) throws -> Double {
            guard let number = (object[key] as? NSNumber)?.doubleValue, number.isFinite else {
                throw DoctoredCoreError(message: "Export sharpening needs a numeric \(key).")
            }
            return number
        }
        return .unsharpMask(radius: try value("radius"), amount: try value("amount"),
                            threshold: try value("threshold"))
    }
}

/// One delivery, described exactly the way the core's request schema does —
/// `json` IS the request object, so a preset stores what the core parses and
/// the two can never drift.
struct ExportSettings: Equatable {
    static let schemaVersion = 1

    var format: ExportFormat
    var jpegQuality: Int
    var bitDepth: Int
    var dither: String
    var size: OutputSize
    var filter: DrResizeFilter
    var profile: OutputProfile
    /// The rendering intent the convert stage runs under (Phase 11C).
    var intent: RenderingIntent = .relative
    var blackPointCompensation: Bool = false
    var sharpen: OutputSharpen?
    var embedProfile: Bool
    var stripMetadata: Bool
    /// The Phase 11B per-packet policy. nil means the key is OMITTED from the
    /// request, which is 11A's behaviour and 11A's bytes.
    var metadata: MetadataPolicy?
    /// The sample width of a 32-bit float deliverable: f32, or half
    /// for a TIFF or OpenEXR. The key is written only for `.f16`, so every
    /// request and preset from before it is unchanged.
    var floatSamples: FloatSamples = .f32
    /// The HDR encoding of a 32-bit HEIC or AVIF (v1.x): a gain map
    /// over an SDR base, or BT.2100 PQ. nil for every other deliverable; the
    /// key is written only when set, so earlier requests are unchanged.
    var hdrEncoding: HDREncoding?
    /// A typed TIFF save that keeps the spot channels as extra samples (#547 1c-2a): the core's
    /// `"spot_channels":"keep"`, written only when true, so every other request is unchanged.
    var keepSpotChannels: Bool = false

    /// The two sample widths the core's `float_samples` names.
    enum FloatSamples: String { case f32, f16 }

    /// The two HDR encodings the core's `hdr_encoding` names.
    enum HDREncoding: String {
        case gainMap = "gain_map"
        case pq

        var displayName: String { self == .gainMap ? "Gain map" : "PQ" }
    }

    /// The four kernels the core names, in the request's own spelling.
    static let filterNames: [(DrResizeFilter, String, String)] = [
        (DR_FILTER_NEAREST, "nearest", "Nearest"),
        (DR_FILTER_BILINEAR, "bilinear", "Bilinear"),
        (DR_FILTER_CATMULL_ROM, "catmull-rom", "Catmull-Rom"),
        (DR_FILTER_LANCZOS3, "lanczos3", "Lanczos3"),
    ]

    var filterName: String {
        Self.filterNames.first { $0.0 == filter }?.1 ?? "lanczos3"
    }

    var json: [String: Any] {
        var object: [String: Any] = [
            "format": format.requestName,
            "jpeg_quality": jpegQuality,
            "bit_depth": bitDepth,
            "dither": dither,
            "size": size.json,
            "filter": filterName,
            "profile": profile.requestName,
            "embed_profile": embedProfile,
            "strip_metadata": stripMetadata,
        ]
        // `profile_path` only for a file destination, and the two 11C option
        // keys only when they differ from the defaults, so a preset saved
        // before this phase re-encodes to exactly the same bytes.
        if let path = profile.path { object["profile_path"] = path }
        if intent != .relative { object["intent"] = intent.rawValue }
        if blackPointCompensation { object["black_point_compensation"] = true }
        if let sharpen { object["sharpen"] = sharpen.json }
        if let metadata { object["metadata"] = metadata.json }
        if floatSamples == .f16 { object["float_samples"] = floatSamples.rawValue }
        if let hdrEncoding { object["hdr_encoding"] = hdrEncoding.rawValue }
        if keepSpotChannels { object["spot_channels"] = "keep" }
        return object
    }

    /// The core request string. The core owns every range and cross-field
    /// rule, so this side never restates one.
    var requestJSON: String {
        guard let data = try? JSONSerialization.data(withJSONObject: json, options: [.sortedKeys])
        else { return "{}" }
        return String(decoding: data, as: UTF8.self)
    }

    static func decode(_ object: [String: Any]) throws -> ExportSettings {
        let allowed: Set<String> = [
            "format", "jpeg_quality", "bit_depth", "dither", "size", "filter", "profile",
            "profile_path", "profile_name", "intent", "black_point_compensation",
            "sharpen", "embed_profile", "strip_metadata", "metadata", "float_samples",
            "hdr_encoding",
        ]
        guard Set(object.keys).isSubset(of: allowed) else {
            throw DoctoredCoreError(message: "Export settings carry an unknown key.")
        }
        guard let formatName = object["format"] as? String,
              let format = ExportFormat.allCases.first(where: { $0.requestName == formatName })
        else {
            throw DoctoredCoreError(
                message: "Export settings need a format of png, jpeg, tiff, bmp, gif, webp, heic, "
                    + "avif, jxl, exr or hdr.")
        }
        var settings = ExportSettings(
            format: format,
            jpegQuality: try integer(object, "jpeg_quality", default: 90),
            bitDepth: try integer(object, "bit_depth", default: 8),
            dither: try text(object, "dither", default: "none"),
            size: .none,
            filter: DR_FILTER_LANCZOS3,
            profile: .document,
            sharpen: nil,
            embedProfile: try flag(object, "embed_profile", default: true),
            stripMetadata: try flag(object, "strip_metadata", default: false),
            metadata: nil)
        if let metadata = present(object, "metadata") {
            guard let metadata = metadata as? [String: Any] else {
                throw DoctoredCoreError(message: "Export metadata must be an object or null.")
            }
            settings.metadata = try MetadataPolicy.decode(metadata)
        }
        if let size = present(object, "size") {
            guard let size = size as? [String: Any] else {
                throw DoctoredCoreError(message: "An export size must be an object.")
            }
            settings.size = try OutputSize.decode(size)
        }
        if present(object, "filter") != nil {
            let name = try text(object, "filter", default: "lanczos3")
            guard let match = filterNames.first(where: { $0.1 == name }) else {
                throw DoctoredCoreError(
                    message: "An export filter must be nearest, bilinear, catmull-rom or lanczos3.")
            }
            settings.filter = match.0
        }
        if present(object, "profile") != nil {
            let name = try text(object, "profile", default: "document")
            switch name {
            case "document": settings.profile = .document
            case "srgb": settings.profile = .srgb
            case "display-p3": settings.profile = .displayP3
            case "file":
                let path = try text(object, "profile_path", default: "")
                guard !path.isEmpty else {
                    throw DoctoredCoreError(
                        message: "An export profile of \"file\" requires a profile_path.")
                }
                let resolved = (path as NSString).expandingTildeInPath
                let name = RasterProfile.describe(
                    (try? Data(contentsOf: URL(fileURLWithPath: resolved))) ?? Data())?.name
                settings.profile = .file(
                    path: resolved,
                    name: name ?? URL(fileURLWithPath: resolved).lastPathComponent)
            case "installed":
                // Resolved to a PATH here, before the core sees the request:
                // a recorded step and the request itself must carry a value
                // rather than a name that could resolve differently.
                let wanted = try text(object, "profile_name", default: "")
                guard let profile = InstalledProfiles.named(wanted) else {
                    throw DoctoredCoreError(
                        message: "No installed profile is named \"\(wanted)\".")
                }
                settings.profile = .file(path: profile.path, name: profile.name)
            default:
                throw DoctoredCoreError(
                    message: "An export profile must be document, srgb, display-p3, file or "
                        + "installed.")
            }
        }
        if present(object, "intent") != nil {
            let name = try text(object, "intent", default: "relative")
            guard let match = RenderingIntent(rawValue: name) else {
                throw DoctoredCoreError(
                    message: "An export intent must be perceptual, relative, saturation or "
                        + "absolute.")
            }
            settings.intent = match
        }
        settings.blackPointCompensation = try flag(
            object, "black_point_compensation", default: false)
        if let sharpen = present(object, "sharpen") {
            guard let sharpen = sharpen as? [String: Any] else {
                throw DoctoredCoreError(message: "An export sharpen must be an object or null.")
            }
            settings.sharpen = try OutputSharpen.decode(sharpen)
        }
        if present(object, "float_samples") != nil {
            let name = try text(object, "float_samples", default: "f32")
            guard let match = FloatSamples(rawValue: name) else {
                throw DoctoredCoreError(message: "Export float_samples must be f32 or f16.")
            }
            settings.floatSamples = match
        }
        if present(object, "hdr_encoding") != nil {
            let name = try text(object, "hdr_encoding", default: "gain_map")
            guard let match = HDREncoding(rawValue: name) else {
                throw DoctoredCoreError(message: "Export hdr_encoding must be gain_map or pq.")
            }
            settings.hdrEncoding = match
        }
        return settings
    }

    /// A present key must carry a usable value: a mistyped scalar is refused
    /// rather than silently replaced by the default, so a caller cannot ask
    /// for one file and be handed another. String spellings are accepted, as
    /// `AgentServer`'s `intArg`/`boolArg` accept them everywhere else.
    private static func present(_ object: [String: Any], _ key: String) -> Any? {
        guard let value = object[key], !(value is NSNull) else { return nil }
        return value
    }

    private static func integer(
        _ object: [String: Any], _ key: String, default fallback: Int
    ) throws -> Int {
        guard let value = present(object, key) else { return fallback }
        if let number = value as? NSNumber { return number.intValue }
        if let spelled = value as? String, let parsed = Int(spelled) { return parsed }
        throw DoctoredCoreError(message: "The export setting \(key) must be a number.")
    }

    private static func flag(
        _ object: [String: Any], _ key: String, default fallback: Bool
    ) throws -> Bool {
        guard let value = present(object, key) else { return fallback }
        if let number = value as? NSNumber { return number.boolValue }
        switch value as? String {
        case "true", "1": return true
        case "false", "0": return false
        default:
            throw DoctoredCoreError(message: "The export setting \(key) must be true or false.")
        }
    }

    private static func text(
        _ object: [String: Any], _ key: String, default fallback: String
    ) throws -> String {
        guard let value = present(object, key) else { return fallback }
        guard let spelled = value as? String else {
            throw DoctoredCoreError(message: "The export setting \(key) must be a string.")
        }
        return spelled
    }

    /// The sheet's starting point: today's behaviour for this document, so
    /// opening Export and pressing Save writes the file the old command
    /// wrote. The five per-document save preferences seed the corresponding
    /// controls; the three NEW options start neutral.
    static func defaults(for document: ImageDocument) -> ExportSettings {
        // #547: a CMYK document starts at Color Settings' intent and BPC with
        // the TIFF depth its own declared depth, so a default kept-CMYK TIFF
        // is the CMYK save's bytes (8 and 16 are both offered).
        if document.doc?.isCmyk == true {
            let own = ExportFormat.from(fileType: document.fileType ?? "")
            let depth = (document.doc?.precision?.cmykInfo()?["depth"] as? Int) ?? 16
            var settings = ExportSettings(
                format: own.flatMap { $0.isFloatOnly ? nil : $0 } ?? .png,
                jpegQuality: document.jpegExportQuality,
                bitDepth: own == .tiff ? depth : 8, dither: "none", size: .none,
                filter: DR_FILTER_LANCZOS3, profile: .document, sharpen: nil,
                embedProfile: document.embedColorProfile, stripMetadata: document.stripMetadata,
                metadata: nil)
            settings.intent = ColorSettings.intent
            settings.blackPointCompensation = ColorSettings.blackPointCompensation
            return settings
        }
        // #547 Lab phase 1b: a Lab Color document exports through RGB: PNG, 8 bits, sRGB, at Color
        // Settings' intent and BPC.
        if document.doc?.isLab == true {
            var settings = ExportSettings(
                format: .png, jpegQuality: document.jpegExportQuality, bitDepth: 8, dither: "none",
                size: .none, filter: DR_FILTER_LANCZOS3, profile: .srgb, sharpen: nil,
                embedProfile: document.embedColorProfile, stripMetadata: document.stripMetadata,
                metadata: nil)
            settings.intent = ColorSettings.intent
            settings.blackPointCompensation = ColorSettings.blackPointCompensation
            return settings
        }
        // #547 Bitmap phase 1c: a Bitmap document starts at its own kept format (else PNG), 8 bits,
        // profile document and no dither, keeping its 1-bit plane (`bitmap_export`).
        if document.doc?.isBitmap == true {
            let own = ExportFormat.from(fileType: document.fileType ?? "")
            return ExportSettings(
                format: own.flatMap { BitmapDocument.keptFormats.contains($0) ? $0 : nil } ?? .png,
                jpegQuality: document.jpegExportQuality, bitDepth: 8, dither: "none",
                size: .none, filter: DR_FILTER_LANCZOS3, profile: .document, sharpen: nil,
                embedProfile: document.embedColorProfile, stripMetadata: document.stripMetadata,
                metadata: nil)
        }
        // #547 Indexed phase 1b: an Indexed Color document starts at its own kept format (else PNG),
        // 8 bits, profile document and no dither, keeping its palette (`indexed_export`).
        if document.doc?.isIndexed == true {
            let own = ExportFormat.from(fileType: document.fileType ?? "")
            return ExportSettings(
                format: own.flatMap { IndexedDocument.keptFormats.contains($0) ? $0 : nil } ?? .png,
                jpegQuality: document.jpegExportQuality, bitDepth: 8, dither: "none",
                size: .none, filter: DR_FILTER_LANCZOS3, profile: .document, sharpen: nil,
                embedProfile: document.embedColorProfile, stripMetadata: document.stripMetadata,
                metadata: nil)
        }
        // #547 Duotone phase 1b: a Duotone document exports RGB through its inks: PNG at the Gray
        // default depth, profile document (its twin).
        if let doc = document.doc, doc.isDuotone {
            return ExportSettings(
                format: .png, jpegQuality: document.jpegExportQuality,
                bitDepth: GrayDocument.defaultBitDepth(doc, format: .png), dither: "none",
                size: .none, filter: DR_FILTER_LANCZOS3, profile: .document, sharpen: nil,
                embedProfile: document.embedColorProfile, stripMetadata: document.stripMetadata,
                metadata: nil)
        }
        // #547 phase 1b: a Grayscale document starts at its own one-channel format (else PNG) and
        // its default depth, keeping gray through profile document (`gray_export`).
        if let doc = document.doc, doc.isGray {
            let own = ExportFormat.from(fileType: document.fileType ?? "")
            let format = own.flatMap { [.png, .tiff, .jpeg].contains($0) ? $0 : nil } ?? .png
            return ExportSettings(
                format: format, jpegQuality: document.jpegExportQuality,
                bitDepth: GrayDocument.defaultBitDepth(doc, format: format), dither: "none",
                size: .none, filter: DR_FILTER_LANCZOS3, profile: .document, sharpen: nil,
                embedProfile: document.embedColorProfile, stripMetadata: document.stripMetadata,
                metadata: nil)
        }
        let typed = document.doc?.precision != nil
        // The document's own type, when it is one this depth can deliver:
        // a float document opened from an OpenEXR file defaults to OpenEXR and
        // any other float document to TIFF; the two float-only formats are
        // never another depth's default.
        let own = ExportFormat.from(fileType: document.fileType ?? "")
        return ExportSettings(
            format: document.doc?.isFloat == true
                ? (own == .exr ? .exr : .tiff)
                : own.flatMap { $0.isFloatOnly ? nil : $0 } ?? .png,
            jpegQuality: document.jpegExportQuality,
            // A 32-bit float document's default deliverable is its EXACT
            // one: the samples as they are (Phase 11F).
            bitDepth: document.doc?.isFloat == true
                ? 32 : typed ? document.exportBitDepth : 8,
            // A 32-bit float deliverable refuses dithering by name, so the
            // remembered preference is not carried into it.
            dither: typed && document.doc?.isFloat != true ? document.exportDither : "none",
            size: .none,
            filter: DR_FILTER_LANCZOS3,
            profile: .document,
            sharpen: nil,
            embedProfile: document.embedColorProfile,
            stripMetadata: document.stripMetadata,
            metadata: nil)
    }
}

extension ExportSettings {
    /// Why this export cannot convert into its destination, or nil — the
    /// core's `export_refusal` for the resolved destination bytes. A mirror
    /// of `ExportRequest::destination_refusal`, the master the core's every
    /// export entry point refuses with before any work; only Export
    /// Layers asks it, so a run refuses ONCE before its first item rather
    /// than per item. "Document profile" never converts, so it is nil; so is
    /// the document's own bytes under a non-absolute intent with BPC off.
    func destinationRefusal(for document: RasterDocument) -> String? {
        if profile == .document { return nil }
        guard let bytes = profile.profileData(for: document) else { return nil }
        if intent != .absolute, !blackPointCompensation, bytes == document.iccProfile {
            return nil
        }
        return RasterProfile.describe(bytes)?.exportDestinationRefusal
    }
}

extension ExportFormat {
    /// The request schema's spelling, which is also what a preset stores.
    var requestName: String {
        switch self {
        case .png: return "png"
        case .jpeg: return "jpeg"
        case .tiff: return "tiff"
        case .bmp: return "bmp"
        case .gif: return "gif"
        case .webp: return "webp"
        case .heic: return "heic"
        case .avif: return "avif"
        case .jxl: return "jxl"
        case .exr: return "exr"
        case .hdr: return "hdr"
        }
    }
}

// MARK: - Presets

/// One saved delivery: the settings, where the file goes, and what it is
/// called. A preset lives OUTSIDE every document, so editing or deleting one
/// can never reinterpret saved pixels or a recorded Action.
struct ExportPreset: Equatable {
    /// Schema 2 adds the optional `metadata_policy` NAME. A preset that names
    /// no policy is still written at 1, so a library built before 11B — and
    /// every file a build before 11B could read — is byte-identical.
    static let schemaVersion = 2

    let name: String
    let settings: ExportSettings
    /// An absolute destination folder. It must already exist — an export
    /// never creates one.
    let destination: String
    /// A naming pattern; `{name}` is mandatory (the core enforces it).
    let pattern: String
    /// The NAME of a metadata policy in the library, resolved at delivery
    /// time. A recorded step never carries this — it carries the resolved
    /// policy, exactly as `quick_export` carries resolved settings.
    let metadataPolicy: String?

    init(name: String, settings: ExportSettings, destination: String, pattern: String,
         metadataPolicy: String? = nil) {
        self.name = name
        self.settings = settings
        self.destination = destination
        self.pattern = pattern
        self.metadataPolicy = metadataPolicy
    }

    var json: [String: Any] {
        var object: [String: Any] = [
            "schema_version": metadataPolicy == nil ? 1 : Self.schemaVersion,
            "name": name,
            "settings": settings.json,
            "destination": destination,
            "pattern": pattern,
        ]
        if let metadataPolicy { object["metadata_policy"] = metadataPolicy }
        return object
    }

    /// The settings this preset delivers with: its own, plus the library
    /// policy it names. Resolved HERE rather than stored, so editing a policy
    /// changes what a preset delivers and deleting one is a refusal rather
    /// than a silent fallback.
    func resolvedSettings() throws -> ExportSettings {
        guard let metadataPolicy else { return settings }
        var resolved = settings
        resolved.metadata = try MetadataPolicyLibrary.named(metadataPolicy).policy
        return resolved
    }

    static func decode(_ object: [String: Any]) throws -> ExportPreset {
        let required: Set<String> = ["schema_version", "name", "settings", "destination", "pattern"]
        guard required.isSubset(of: Set(object.keys)),
              Set(object.keys).isSubset(of: required.union(["metadata_policy"])),
              let version = object["schema_version"] as? NSNumber,
              CFGetTypeID(version) != CFBooleanGetTypeID(),
              (1...schemaVersion).contains(version.intValue),
              version.doubleValue == Double(version.intValue),
              let name = object["name"] as? String,
              let settings = object["settings"] as? [String: Any],
              let destination = object["destination"] as? String,
              let pattern = object["pattern"] as? String else {
            throw DoctoredCoreError(message: "Invalid or unsupported export preset.")
        }
        var policy: String?
        if let value = object["metadata_policy"], !(value is NSNull) {
            guard version.intValue >= 2, let named = value as? String else {
                throw DoctoredCoreError(message: "Invalid or unsupported export preset.")
            }
            policy = try MetadataPolicyLibrary.normalizedName(named)
        }
        return ExportPreset(name: try ExportPresetLibrary.normalizedName(name),
                            settings: try ExportSettings.decode(settings),
                            destination: destination, pattern: pattern,
                            metadataPolicy: policy)
    }
}

/// The export-preset library's codec on the shared `PreferenceLibrary`
/// contract: `DR_EXPORT_PRESETS_DIR`, digest filenames, an advisory
/// cross-instance `flock`, atomic publication and — since 12C — per-entry
/// diagnosis rather than whole-library strictness.
enum ExportPresetCodec: PreferenceLibraryCodec {
    typealias Item = ExportPreset

    static let noun = "export preset"
    static let directoryEnvironment = "DR_EXPORT_PRESETS_DIR"
    static let directoryName = "Doctored/Export Presets"
    static let maximumEntries = 64
    static let maximumFileBytes = 4096
    static let schemaVersions = 1...ExportPreset.schemaVersion
    static let didChange = Notification.Name("ExportPresetLibraryDidChange")

    static func name(of item: ExportPreset) -> String { item.name }
    static func json(of item: ExportPreset) -> [String: Any] { item.json }

    static func decode(_ object: [String: Any], schemaVersion: Int) throws -> ExportPreset {
        try ExportPreset.decode(object)
    }
}

/// The bounded versioned export-preset library. Library writes are
/// PREFERENCES: they never enter document Undo and are never Action steps.
enum ExportPresetLibrary {
    private typealias Library = PreferenceLibrary<ExportPresetCodec>

    static let maximumPresets = ExportPresetCodec.maximumEntries
    static let maximumFileBytes = ExportPresetCodec.maximumFileBytes
    static let didChange = ExportPresetCodec.didChange

    /// The default preset's name lives in the library's hidden `.default`
    /// file (`PreferenceLibraryDefault`) rather than in a preset, so marking a
    /// different preset the default rewrites nothing a recorded Quick Export
    /// depends on.
    private typealias Default = PreferenceLibraryDefault<ExportPresetCodec>

    static var directory: URL { Library.directory }

    static func normalizedName(_ name: String) throws -> String {
        try Library.normalizedName(name)
    }

    static func validatedFileName(_ file: String) throws -> String {
        try Library.validatedFileName(file)
    }

    /// Every listed preset and every bad file, each with its own code and
    /// reason; `repairable` is always false (repair stays RAW-only).
    static func listing() throws -> PreferenceLibraryListing<ExportPreset> {
        try Library.listing()
    }

    static func entries() throws -> [ExportPreset] {
        try Library.entries()
    }

    static func named(_ name: String) throws -> ExportPreset {
        try Library.named(name)
    }

    /// A source shape that exercises the size grammar (positive value, scale
    /// range, the format's own maximum side) without letting the aspect ratio
    /// decide: the named side is as long as the pixel ceiling allows and the
    /// side the mode does not name stays 1, so only sizes no document could
    /// produce are refused here. The real document's own size is checked at
    /// export.
    private static func validationSource(for size: OutputSize) -> (Int, Int) {
        switch size {
        case .width, .longEdge: return (RasterImage.maxResizePixels, 1)
        case .height: return (1, RasterImage.maxResizePixels)
        case .none, .scale: return (1, 1)
        }
    }

    @discardableResult
    static func save(_ preset: ExportPreset, replace: Bool = false) throws -> ExportPreset {
        let preset = ExportPreset(name: try normalizedName(preset.name), settings: preset.settings,
                                  destination: preset.destination, pattern: preset.pattern,
                                  metadataPolicy: preset.metadataPolicy)
        // The core owns both grammars: the request's ranges and the pattern's
        // mandatory {name}. Ask it before a bad preset reaches the library —
        // against a probe source whose aspect keeps the OTHER side at 1, since
        // the document a preset will be used on is unknown here and a square
        // probe would refuse a legal wide-document size on the pixel ceiling.
        let probe = validationSource(for: preset.settings.size)
        _ = try RasterExport.outputSize(width: probe.0, height: probe.1,
                                        request: preset.settings.requestJSON)
        _ = try RasterExport.expandName(pattern: preset.pattern, tokens: ExportNaming.sampleTokens)
        // A destination FILE the export cannot convert into is refused here,
        // with the export's own sentence, rather than on every use.
        // No document: only a `.file` destination is described.
        if let refusal = preset.settings.profile.destinationRefusal {
            throw DoctoredCoreError(message: refusal)
        }
        // A preset that names a policy must name one that exists: resolving it
        // at delivery time would otherwise turn a typo into a refused batch.
        if let policy = preset.metadataPolicy { _ = try MetadataPolicyLibrary.named(policy) }
        guard !preset.destination.isEmpty else {
            throw DoctoredCoreError(message: "An export preset needs a destination folder.")
        }
        return try Library.save(preset, replace: replace)
    }

    static func delete(name: String) throws {
        try Default.delete(name: name)
    }

    static func delete(file: String) throws {
        try Default.delete(file: file)
    }

    /// The name File ▸ Quick Export uses, or nil when no default is set —
    /// which is the shipped state: there are no seeded presets, so until the
    /// user chooses one Quick Export delivers with the built-in PNG preset
    /// (`quickExportPreset(for:)`, `QuickExportDefaults.swift`).
    static func defaultPresetName() throws -> String? {
        try Default.name()
    }

    static func setDefault(name: String?) throws {
        try Default.set(name: name)
    }
}

// MARK: - Naming

/// The naming pattern's token set and the host half of it — the date, which
/// a pure core has no calendar for.
enum ExportNaming {
    static let defaultPattern = "{name}-{width}x{height}"
    static let tokenHelp =
        "{name} (required), {width}, {height}, {format}, {date}, {preset}, "
        + "{layer} and {index} (layer exports and batches only). {index} is the item's "
        + "position within that one run, so re-running a batch over a folder that has "
        + "gained or lost files renumbers its outputs rather than colliding with the "
        + "earlier run's."

    /// A pattern's validity is checked against these, so a preset can be
    /// refused at save time rather than at delivery time. `layer` and `index`
    /// are present so a preset MAY use them; an export with no item refuses
    /// them at delivery, which is the point of the core's absent-token rule.
    static let sampleTokens: [String: Any] = [
        "name": "Sample", "width": 1000, "height": 1000, "format": "png",
        "date": "2026-01-01", "preset": "Preset", "layer": "Layer", "index": 1, "count": 1,
    ]

    /// Local `YYYY-MM-DD`, the ISO calendar date in the user's own time zone
    /// — the day they exported, not a UTC day that may be the one before.
    static func dateString(_ date: Date) -> String {
        let formatter = DateFormatter()
        formatter.locale = Locale(identifier: "en_US_POSIX")
        formatter.dateFormat = "yyyy-MM-dd"
        return formatter.string(from: date)
    }

    /// A token set naming a layer is a layer export's (`layer_export`).
    static func tokens(name: String, width: Int, height: Int, format: ExportFormat,
                       date: Date, preset: String, layer: String? = nil,
                       index: (Int, Int)? = nil) -> [String: Any] {
        var object: [String: Any] = [
            "name": name, "width": width, "height": height,
            "format": format.requestName, "date": dateString(date), "preset": preset,
        ]
        if let layer {
            object["layer"] = layer
            object["layer_export"] = true
        }
        if let index {
            object["index"] = index.0
            object["count"] = index.1
        }
        return object
    }

    /// The document's own base name — what `{name}` expands to.
    static func baseName(of document: ImageDocument) -> String {
        let display = document.displayName ?? "Untitled"
        let stripped = (display as NSString).deletingPathExtension
        return stripped.isEmpty ? display : stripped
    }
}

// MARK: - Capture

/// One immutable document and its warm projection, captured TOGETHER before
/// a panel opens or a worker starts: an agent edit landing in between must
/// not pair one revision's pixels with another's profile and packets.
struct ExportCapture {
    let source: RasterDocument
    let projection: RasterImage?
    let precisionProjection: RasterPrecisionImage?
    let sourceSize: (width: Int, height: Int)
    let metadataNotCaptured: Bool
    let metadataImportReport: [String: Any]?
    /// The document's own non-evictable bytes, live for the whole export.
    let ownRetainedBytes: UInt64

    init(_ document: ImageDocument) throws {
        guard let source = document.doc else {
            throw DoctoredCoreError(message: "No image loaded.")
        }
        if source.isCmyk {
            // #547 phase 5: a CMYK document exports from its own ink
            // (`cmyk_export`), never from its Display P3 view.
            precisionProjection = nil
            projection = nil
        } else if source.precision != nil {
            precisionProjection = try document.currentPrecisionProjection()
            projection = nil
        } else {
            projection = try document.currentProjection()
            precisionProjection = nil
        }
        guard document.doc === source else {
            throw DoctoredCoreError(
                message: "The document changed while its export projection was being prepared.")
        }
        ownRetainedBytes = try source.ownRetainedBytes()
        self.source = source
        sourceSize = (source.width, source.height)
        metadataNotCaptured = document.metadataNotCaptured
        metadataImportReport = document.metadataImportReport
    }

    /// The output dimensions this settings object produces, asked of the
    /// core so the sheet's readout and the encode can never disagree.
    func outputSize(_ settings: ExportSettings) throws -> (Int, Int) {
        try RasterExport.outputSize(width: sourceSize.width, height: sourceSize.height,
                                    request: settings.requestJSON)
    }

    /// Conservative working-memory estimate for one encode. The byte figure
    /// is `CapturedSave`'s 12 B/px plus 52 B/px for an active sharpen of
    /// either method: the planes Unsharp Mask's Gaussian stage allocates (two
    /// premultiplied f32 planes, the straight f32 input and the byte output),
    /// and Smart Sharpen's kernel (at most 28 B/px), its rings and its 4 B/px
    /// output inside the same figure. The typed figure is the core's own.
    func scratchEstimate(_ settings: ExportSettings) throws -> UInt64 {
        if let typed = source.precision {
            let plan = try typed.exportPlan(projection: precisionProjection,
                                            request: settings.requestJSON)
            return try PrecisionDocumentDescriptor.integer(plan, "scratch_bytes")
        }
        let (w, h) = try outputSize(settings)
        let pixels = UInt64(max(sourceSize.width * sourceSize.height, w * h))
        let sharpening: UInt64 = settings.sharpen?.isActive == true ? 52 : 0
        return pixels * (12 + sharpening) + 16 * 1024 * 1024
    }

    /// The remedy a refusal names when the export cannot fit on its own.
    func refusalRemedy(for settings: ExportSettings) -> String {
        if source.precision != nil { return RasterResources.depthJobRemedy }
        if settings.sharpen?.isActive == true {
            return "Export without sharpening, or work on a smaller image."
        }
        return "Work on a smaller image."
    }

    /// Encode, on whatever thread the caller is on. No file is written.
    func encode(_ settings: ExportSettings, control: RasterJobControl?) throws -> RasterExportResult {
        // #547: spot channels composite into every flattened export, a TIFF too (a byte one
        // through the overprint below, a typed one in the core, 1c-1); a TIFF save keeps them
        // (1c-2a, `flatSaveProjection`, `keepSpotChannels`).
        if let typed = source.precision {
            let context = try PrecisionWorkContext(source: typed, image: precisionProjection)
            return try Self.hostFinish(
                typed.exportEncoded(projection: precisionProjection,
                                    request: settings.requestJSON,
                                    workingLimit: context.limit,
                                    otherRetained: context.otherRetained, control: control),
                settings: settings)
        }
        // The captured projection overprinted here, lazily: Export Layers and Contact Sheet, which
        // build a capture only for `encodeLayer`, never pay for it.
        let flat = try projection.map { try source.spotOverprint($0) ?? $0 }
        return try Self.hostFinish(
            source.exportEncoded(flat: flat, request: settings.requestJSON, control: control),
            settings: settings)
    }

    /// The HOST half of a host-encoded export, run at every encode site
    /// (`ExportCapture.encode`, `LayerExport.encodeLayer`,
    /// `ContactSheet.deliver`): identity unless the result is pending, and
    /// otherwise the transcode of the intermediate plus the adoption of what
    /// the host wrote.
    ///
    /// It lives HERE, at the encode, rather than at each write: the core
    /// refuses to write a pending result, and every consumer of a result's
    /// bytes, preview or info runs after one of those three sites, so an
    /// intermediate can never be delivered as a deliverable.
    static func hostFinish(_ result: RasterExportResult,
                           settings: ExportSettings) throws -> RasterExportResult {
        guard try result.isHostPending() else { return result }
        // #547: a CMYK document's kept-CMYK JPEG — the 8-bit CMYK TIFF
        // intermediate ImageIO transcodes keeping CMYK and its profile. The
        // transcode follows the policy's EXIF, XMP and IPTC answers (the
        // intermediate's carried bits), and the packets adopted are those
        // MEASURED from the written JPEG.
        if (try result.info())["cmyk"] as? Bool == true {
            let carried = UInt32((try result.info())["carried"] as? Int ?? 0)
            let jpeg = try HostImageEncoder.cmykJPEG(fromTIFF: result.bytes, quality: settings.jpegQuality,
                                                     keepExif: carried & DR_CARRIES_EXIF != 0,
                                                     keepXmp: carried & DR_CARRIES_XMP != 0,
                                                     keepIptc: carried & DR_CARRIES_IPTC != 0)
            let measured = try HostImageEncoder.inspect(jpeg)
            return try result.adoptingHostBytes(jpeg, bitDepth: 8, carried: measured.carried)
        }
        // v1.x: a 32-bit HDR HEIC/AVIF's intermediate is f32 linear
        // (gain map) or 16-bit PQ codes, written by ImageIO's HDR encoders.
        if let hdr = settings.hdrEncoding {
            let headroom = (try result.info()["content_headroom"] as? NSNumber)?.doubleValue ?? 1
            let encoded = try HostImageEncoder.encodeHDR(fromTIFF: result.bytes, as: settings.format,
                                                         encoding: hdr, contentHeadroom: headroom,
                                                         quality: settings.jpegQuality)
            return try result.adoptingHostBytes(encoded.bytes, bitDepth: encoded.bitDepth,
                                                carried: encoded.carried)
        }
        let encoded = try HostImageEncoder.encode(fromTIFF: result.bytes, as: settings.format,
                                                  quality: settings.jpegQuality,
                                                  embedProfile: settings.embedProfile)
        return try result.adoptingHostBytes(encoded.bytes, bitDepth: encoded.bitDepth,
                                            carried: encoded.carried)
    }

    /// Encode on the bounded worker, with the progress panel and Cancel
    /// (Esc) up. Main thread only, like every other `BackgroundWork.wait`
    /// caller. A cancel throws the core's cancellation error before any
    /// caller writes or records.
    func run(_ settings: ExportSettings, title: String) throws -> RasterExportResult {
        let scratch = try scratchEstimate(settings)
        _ = try RasterResources.shared.admit(scratch: scratch, own: ownRetainedBytes,
                                             remedy: refusalRemedy(for: settings))
        let capture = self
        let control = try RasterJobControl()
        return try BackgroundWork.wait(title: title, control: control, makeTask: {
            try WorkerTask(scratchEstimate: scratch) {
                try capture.encode(settings, control: control)
            }
        })
    }
}

// MARK: - Quick Export

extension ExportPreset {
    /// The preset's destination folder, tilde-expanded — the one URL Quick
    /// Export, the Batch window and `run_batch` all build from it.
    var destinationFolder: URL {
        URL(fileURLWithPath: (destination as NSString).expandingTildeInPath, isDirectory: true)
    }

    /// The destination folder, or Quick Export's refusal when it is not an
    /// existing directory. An export never creates a folder.
    func requireDestinationFolder() throws -> URL {
        let folder = destinationFolder
        var isDirectory: ObjCBool = false
        guard FileManager.default.fileExists(atPath: folder.path, isDirectory: &isDirectory),
              isDirectory.boolValue else {
            throw DoctoredCoreError(
                message: "The export destination folder \(folder.path) does not exist.")
        }
        return folder
    }
}

/// What one Quick Export wrote, so every caller reports the same path.
struct QuickExportOutcome {
    let url: URL
    let result: RasterExportResult
    let preset: String
    /// The captured document the bytes came from, so the caller's
    /// dropped-packet notice asks the same handle the encode used.
    let source: RasterDocument
    /// The name the pattern produced. It differs from `url` only when that
    /// name was taken and the call asked for a suffix
    /// (`QuickExportCollision.suffix`).
    let requested: URL
}

extension ImageDocument {
    /// THE Quick Export naming: the output size, the folder check, the
    /// core's pattern expansion and the format's extension. It does not ask
    /// whether the file exists — `quickExport` refuses that itself, and a
    /// batch pre-check turns it into a skip.
    func quickExportDestination(preset: ExportPreset, settings: ExportSettings,
                                sourceSize: (width: Int, height: Int),
                                ordinal: (index: Int, count: Int), date: Date) throws -> URL {
        let (width, height) = try RasterExport.outputSize(
            width: sourceSize.width, height: sourceSize.height, request: settings.requestJSON)
        let folder = try preset.requireDestinationFolder()
        let base = try RasterExport.expandName(
            pattern: preset.pattern,
            tokens: ExportNaming.tokens(name: ExportNaming.baseName(of: self), width: width,
                                        height: height, format: settings.format,
                                        date: date, preset: preset.name,
                                        index: (ordinal.index, ordinal.count)))
        return folder.appendingPathComponent(base)
            .appendingPathExtension(settings.format.fileExtension)
    }

    /// File ▸ Quick Export and its MCP twin: expand the preset's pattern,
    /// write into its destination folder, and record ONE resolved
    /// `quick_export` step.
    ///
    /// Three refusals, all deliberate: a destination folder that does not
    /// exist is never created, an existing file is never overwritten (the
    /// name is pattern-derived, so silently replacing one would be the batch
    /// defect `save_copy` is kept out of Actions for), and the pattern must
    /// contain `{name}` (the core enforces that).
    ///
    /// `collision` says what a taken name does. `.refuse`, the default, is
    /// what a recorded step, a batch delivery and the resolved MCP form get.
    /// `.suffix` writes beside it under the first free `-2`, `-3`, … name
    /// (`QuickExportNaming.firstFree`) and is passed by exactly two callers:
    /// the menu command, and `AgentServer.quickExport` for a call without
    /// `settings`. The recorded step is the same either way.
    ///
    /// Nothing about the document changes: no edit, no change count, and
    /// none of the five per-document save preferences is written.
    func quickExport(preset: ExportPreset, policy: MetadataPolicy? = nil,
                     ordinal: (index: Int, count: Int) = (1, 1),
                     date: Date = Date(),
                     collision: QuickExportCollision = .refuse) throws -> QuickExportOutcome {
        var settings = try preset.resolvedSettings()
        // An explicit policy wins over the preset's named one: a batch's own
        // "Metadata policy:" popup is the caller saying so for this run.
        if let policy { settings.metadata = policy }
        let capture = try ExportCapture(self)
        let requested = try quickExportDestination(preset: preset, settings: settings,
                                                   sourceSize: capture.sourceSize,
                                                   ordinal: ordinal, date: date)
        let url: URL
        switch collision {
        case .refuse:
            guard !FileManager.default.fileExists(atPath: requested.path) else {
                throw DoctoredCoreError(
                    message: "\(requested.path) already exists. Quick Export never overwrites a file.")
            }
            url = requested
        case .suffix:
            url = try QuickExportNaming.firstFree(requested)
        }
        let result = try capture.run(settings, title: "Quick Export")
        try result.write(to: url, overwrite: false)
        ActionRecorder.shared.record(
            [ActionStep(tool: "quick_export",
                        arguments: ["settings": settings.json,
                                    "destination": preset.destination,
                                    "pattern": preset.pattern,
                                    "preset": preset.name,
                                    "index": ordinal.index,
                                    "count": ordinal.count],
                        note: "File ▸ Quick Export")],
            on: self)
        return QuickExportOutcome(url: url, result: result, preset: preset.name,
                                  source: capture.source, requested: requested)
    }
}
