import AppKit
import UniformTypeIdentifiers

// `ExportFormat.flatSaveCases` is `allCases` minus the host-encoded formats and is what Save As,
// `save_copy` and the typed flat export iterate; `allCases` stays the capability question
// (`AgentServer+Preservation`, `ExportSettings.decode`).

/// The formats the app can export/write, bridging display name, DrFormat,
/// UTType, and file extension.
enum ExportFormat: CaseIterable {
    case png
    case jpeg
    case tiff
    case bmp
    case gif
    case webp
    /// Host-encoded (Phase 11D): the core hands the export workspace a TIFF
    /// intermediate and `HostImageEncoder` transcodes it. Deliberately NOT in
    /// `flatSaveCases`, because every flat-save path (Save As, `save_copy`,
    /// the typed flat export) goes through `dr_doc_save_image`, which refuses
    /// it.
    case heic
    /// Host-encoded exactly as `heic` is: the same TIFF intermediate,
    /// transcoded by ImageIO's AVIF encoder.
    case avif
    /// Lossless JPEG XL, encoded by the core but only through the export
    /// workspace (8-bit, sRGB pixels). The core decodes none, so its preview
    /// is the host's (`RasterExportResult.preview`).
    case jxl
    /// OpenEXR, a 32-bit FLOAT deliverable: the export workspace
    /// writes it from a float document only (`bit_depth` 32), at f32 or half.
    /// Not in `flatSaveCases` — no flat save holds float samples.
    case exr
    /// Radiance RGBE, a 32-bit float deliverable exactly as `exr` is.
    case hdr

    var displayName: String {
        switch self {
        case .png: return "PNG"
        case .jpeg: return "JPEG"
        case .tiff: return "TIFF"
        case .bmp: return "BMP"
        case .gif: return "GIF"
        case .webp: return "WebP (lossless)"
        case .heic: return "HEIC"
        case .avif: return "AVIF"
        case .jxl: return "JPEG XL (lossless)"
        case .exr: return "OpenEXR"
        case .hdr: return "Radiance HDR"
        }
    }

    var drFormat: DrFormat {
        switch self {
        case .png: return DR_FORMAT_PNG
        case .jpeg: return DR_FORMAT_JPEG
        case .tiff: return DR_FORMAT_TIFF
        case .bmp: return DR_FORMAT_BMP
        case .gif: return DR_FORMAT_GIF
        case .webp: return DR_FORMAT_WEBP
        case .heic: return DR_FORMAT_HEIC
        case .avif: return DR_FORMAT_AVIF
        case .jxl: return DR_FORMAT_JXL
        case .exr: return DR_FORMAT_EXR
        case .hdr: return DR_FORMAT_HDR
        }
    }

    var utType: UTType {
        switch self {
        case .png: return .png
        case .jpeg: return .jpeg
        case .tiff: return .tiff
        case .bmp: return .bmp
        case .gif: return .gif
        case .webp: return .webP
        case .heic: return .heic
        case .avif: return UTType(importedAs: "public.avif")
        case .jxl: return UTType(importedAs: "public.jpeg-xl")
        case .exr: return UTType(importedAs: "com.ilm.openexr-image")
        case .hdr: return UTType(importedAs: "public.radiance")
        }
    }

    var fileExtension: String {
        switch self {
        case .png: return "png"
        case .jpeg: return "jpg"
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

    /// True for the two formats that exist only as a 32-bit float
    /// deliverable: `bit_depth` 32, so only a float document.
    var isFloatOnly: Bool { self == .exr || self == .hdr }

    /// The formats a FLAT SAVE may write — every case but the host-encoded
    /// ones, JPEG XL and the two float deliverables, which only the export
    /// workspace writes. `allCases`
    /// stays the capability question (what the app can deliver at all); this
    /// is the "what `dr_doc_save_image` can write" question, and the two are
    /// no longer the same list.
    static let flatSaveCases: [ExportFormat] = allCases.filter {
        $0 != .heic && $0 != .avif && $0 != .jxl && !$0.isFloatOnly
    }

    static func from(utType: UTType) -> ExportFormat? {
        allCases.first { $0.utType == utType }
    }

    static func from(fileType typeName: String) -> ExportFormat? {
        guard let type = UTType(typeName) else { return nil }
        if let exact = from(utType: type) { return exact }
        return allCases.first { type.conforms(to: $0.utType) }
    }

    /// The format a file-extension or format TOKEN names — "tif"/"tiff",
    /// "jpg"/"jpeg", any case, with or without a leading ".". The one
    /// alias-aware lookup every MCP site mapping a token or path extension to
    /// a format uses; anything else is nil.
    static func from(fileExtension token: String) -> ExportFormat? {
        var name = token.lowercased()
        if name.hasPrefix(".") { name.removeFirst() }
        switch name {
        case "png": return .png
        case "jpg", "jpeg": return .jpeg
        case "tif", "tiff": return .tiff
        case "bmp": return .bmp
        case "gif": return .gif
        case "webp": return .webp
        case "heic": return .heic
        case "avif": return .avif
        case "jxl": return .jxl
        case "exr": return .exr
        case "hdr": return .hdr
        default: return nil
        }
    }
}

// MARK: - Export workspace (Phase 11A)

/// One encoded deliverable: the exact bytes, what they describe, and the two
/// things a host does with them — decode a preview, or write the file. The
/// preview and the written file come from the SAME bytes, which is what makes
/// "the preview matches the encoded output" an identity rather than a promise.
final class RasterExportResult {
    let ptr: OpaquePointer

    init(owning pointer: OpaquePointer) {
        ptr = pointer
    }
    deinit { dr_export_result_free(ptr) }

    /// A copy of the encoded bytes. The core's buffer is borrowed, so the
    /// copy is made inside this owner's lifetime.
    var bytes: Data {
        var length = 0
        guard let pointer = dr_export_result_bytes(ptr, &length), length > 0 else { return Data() }
        return withExtendedLifetime(self) { Data(bytes: pointer, count: length) }
    }

    func info() throws -> [String: Any] {
        var error: UnsafeMutablePointer<CChar>?
        let result = dr_export_result_info(ptr, &error)
        return try precisionJSON(result, error)
    }

    /// The `DR_CARRIES_*` report, so `ExportCapabilities.droppedMessage` can
    /// say what did not travel with the file exactly as a save does.
    func saveReport() throws -> RasterSaveReport {
        guard let carried = (try info()["carried"] as? NSNumber)?.uint32Value else {
            throw DoctoredCoreError(message: "The export result has no carried-packet report.")
        }
        return RasterSaveReport(carried: carried)
    }

    /// True while this is a HOST-ENCODED export whose bytes are still the
    /// intermediate the host has to transcode (Phase 11D).
    func isHostPending() throws -> Bool {
        (try info()["intermediate"] as? String) != nil
    }

    /// The result the host's own bytes produce: the same request, the same
    /// dimensions and profile, but the depth and the packets an inspection of
    /// the WRITTEN file measured. Pure — a new result, exactly as the core's
    /// ops are.
    func adoptingHostBytes(_ bytes: Data, bitDepth: Int, carried: UInt32) throws -> RasterExportResult {
        var error: UnsafeMutablePointer<CChar>?
        let adopted: OpaquePointer? = bytes.withUnsafeBytes { buffer in
            dr_export_result_adopt_host_bytes(
                ptr, buffer.bindMemory(to: UInt8.self).baseAddress, buffer.count,
                UInt32(max(0, bitDepth)), carried, &error)
        }
        guard let adopted else {
            throw DoctoredCoreError(message: takeErrorMessage(
                error, fallback: "Could not adopt the host encoder's bytes."))
        }
        return RasterExportResult(owning: adopted)
    }

    func preview() throws -> RasterImage {
        // An export the core cannot decode is previewed by the HOST: HEIC and
        // AVIF (host-encoded) and JPEG XL (core-encoded, but with no decoder
        // there either). The core refuses those by name and says so in the
        // result's `host_preview`, so the preview contract — the picture on
        // screen is a decode of the bytes about to be written — is kept on
        // this side of the boundary, over the result's own bytes.
        if (try info()["host_preview"] as? NSNumber)?.boolValue == true {
            return try HostImageEncoder.decodeStraightRGBA8(bytes)
        }
        var error: UnsafeMutablePointer<CChar>?
        guard let result = dr_export_result_preview(ptr, &error) else {
            throw DoctoredCoreError(message: takeErrorMessage(error, fallback: "Could not decode the encoded export."))
        }
        return RasterImage(owning: result)
    }

    func write(to url: URL, overwrite: Bool) throws {
        var error: UnsafeMutablePointer<CChar>?
        guard dr_export_result_write(ptr, url.path, overwrite, &error) else {
            throw DoctoredCoreError(message: takeErrorMessage(error, fallback: "Could not write \(url.lastPathComponent)."))
        }
    }
}

extension RasterDocument {
    /// Encode this 8-bit document as a deliverable. `flat` is THIS document's
    /// warm composite, exactly as `saveImage` takes it; nil makes the core
    /// flatten. No file is written and no document state changes.
    func exportEncoded(flat: RasterImage?, request: String,
                       control: RasterJobControl? = nil) throws -> RasterExportResult {
        var error: UnsafeMutablePointer<CChar>?
        guard let result = dr_doc_export_encode(ptr, flat?.ptr, request, control?.ptr, &error) else {
            throw DoctoredCoreError(message: takeErrorMessage(error, fallback: "Could not encode the export."))
        }
        return RasterExportResult(owning: result)
    }
}

extension RasterPrecisionDocument {
    func exportPlan(projection: RasterPrecisionImage?, request: String) throws -> [String: Any] {
        var error: UnsafeMutablePointer<CChar>?
        let result = dr_precision_doc_export_plan(ptr, projection?.ptr, request, &error)
        return try precisionJSON(result, error)
    }

    func exportEncoded(projection: RasterPrecisionImage?, request: String,
                       workingLimit: UInt64, otherRetained: UInt64,
                       control: RasterJobControl? = nil) throws -> RasterExportResult {
        var error: UnsafeMutablePointer<CChar>?
        guard let result = dr_precision_doc_export_encode(ptr, projection?.ptr, request,
            workingLimit, otherRetained, control?.ptr, &error) else {
            throw DoctoredCoreError(message: takeErrorMessage(error, fallback: "Could not encode the high-depth export."))
        }
        return RasterExportResult(owning: result)
    }
}

/// The two pure export queries that need no handle: the output dimensions a
/// request derives from a source size, and the expansion of a naming pattern.
enum RasterExport {
    static func outputSize(width: Int, height: Int, request: String) throws -> (Int, Int) {
        var error: UnsafeMutablePointer<CChar>?
        let result = dr_export_output_size(UInt32(max(0, width)), UInt32(max(0, height)), request, &error)
        let value = try precisionJSON(result, error)
        guard let w = (value["width"] as? NSNumber)?.intValue,
              let h = (value["height"] as? NSNumber)?.intValue else {
            throw DoctoredCoreError(message: "The core returned no export dimensions.")
        }
        return (w, h)
    }

    static func expandName(pattern: String, tokens: [String: Any]) throws -> String {
        let data = try JSONSerialization.data(withJSONObject: tokens)
        var error: UnsafeMutablePointer<CChar>?
        guard let result = dr_export_expand_name(pattern, String(decoding: data, as: UTF8.self), &error) else {
            throw DoctoredCoreError(message: takeErrorMessage(error, fallback: "Could not expand the naming pattern."))
        }
        defer { dr_string_free(result) }
        return String(cString: result)
    }

    // MARK: Production outputs (Phase 11B)

    /// The sheet geometry for `count` cells — `(width, height, rows)`.
    static func contactSheetSize(request: String, count: Int) throws -> (Int, Int, Int) {
        var error: UnsafeMutablePointer<CChar>?
        let result = dr_contact_sheet_size(request, count, &error)
        let value = try precisionJSON(result, error)
        guard let w = (value["width"] as? NSNumber)?.intValue,
              let h = (value["height"] as? NSNumber)?.intValue,
              let rows = (value["rows"] as? NSNumber)?.intValue else {
            throw DoctoredCoreError(message: "The core returned no contact sheet dimensions.")
        }
        return (w, h, rows)
    }

    /// Compose a contact sheet. `captions` is positional with `sources`; an
    /// entry may be nil and the whole array may be shorter or empty. Every
    /// handle must outlive the call, which is what `withExtendedLifetime`
    /// around the pointer arrays guarantees.
    static func contactSheetCompose(request: String, sources: [RasterDocument],
                                    captions: [RasterImage?],
                                    control: RasterJobControl? = nil) throws -> RasterImage {
        guard !sources.isEmpty else {
            throw DoctoredCoreError(message: "A contact sheet needs at least one cell.")
        }
        var documents: [OpaquePointer?] = sources.map { $0.ptr }
        var labels: [OpaquePointer?] = (0..<sources.count).map { index in
            index < captions.count ? captions[index]?.ptr : nil
        }
        var error: UnsafeMutablePointer<CChar>?
        let result: OpaquePointer? = documents.withUnsafeMutableBufferPointer { documentBuffer in
            labels.withUnsafeMutableBufferPointer { labelBuffer in
                dr_contact_sheet_compose(request, documentBuffer.baseAddress, labelBuffer.baseAddress,
                                         sources.count, control?.ptr, &error)
            }
        }
        withExtendedLifetime(sources) {}
        withExtendedLifetime(captions) {}
        guard let result else {
            throw DoctoredCoreError(message: takeErrorMessage(error, fallback: "Could not compose the contact sheet."))
        }
        return RasterImage(owning: result)
    }

    /// The canonical echo of a metadata policy. The core owns every rule, so
    /// this side never restates one.
    static func validateMetadataPolicy(_ policy: [String: Any]) throws -> [String: Any] {
        let data = try JSONSerialization.data(withJSONObject: policy, options: [.sortedKeys])
        var error: UnsafeMutablePointer<CChar>?
        let result = dr_metadata_policy_validate(String(decoding: data, as: UTF8.self), &error)
        return try precisionJSON(result, error)
    }
}

extension RasterDocument {
    /// Encode ONE top-level layer or group of this 8-bit document, rendered
    /// in isolation. No file is written and no document state changes.
    func exportLayerEncoded(index: Int, trimmed: Bool, request: String,
                            control: RasterJobControl? = nil) throws -> RasterExportResult {
        var error: UnsafeMutablePointer<CChar>?
        guard let result = dr_doc_export_layer_encode(ptr, index, trimmed, request,
                                                      control?.ptr, &error) else {
            throw DoctoredCoreError(message: takeErrorMessage(error, fallback: "Could not encode the layer export."))
        }
        return RasterExportResult(owning: result)
    }
}

extension RasterPrecisionDocument {
    func exportLayerPlan(index: Int, trimmed: Bool, request: String) throws -> [String: Any] {
        var error: UnsafeMutablePointer<CChar>?
        let result = dr_precision_doc_export_layer_plan(ptr, index, trimmed, request, &error)
        return try precisionJSON(result, error)
    }

    func exportLayerEncoded(index: Int, trimmed: Bool, request: String,
                            workingLimit: UInt64, otherRetained: UInt64,
                            control: RasterJobControl? = nil) throws -> RasterExportResult {
        var error: UnsafeMutablePointer<CChar>?
        guard let result = dr_precision_doc_export_layer_encode(ptr, index, trimmed, request,
            workingLimit, otherRetained, control?.ptr, &error) else {
            throw DoctoredCoreError(message: takeErrorMessage(error, fallback: "Could not encode the high-depth layer export."))
        }
        return RasterExportResult(owning: result)
    }
}
