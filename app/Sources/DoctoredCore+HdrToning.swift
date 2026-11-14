import Foundation

// MARK: - HDR Toning
//
// The only `dr_*hdr_toning*` calls. The request JSON is `HdrToningSettings.coreRequest()`'s
// (`core/include/doctored_core.h` holds the contract). No call changes its document: the toning
// answers a NEW one-raster 16-bit document, which `HdrToningRun` narrows to 8 bits when asked.
// `RasterHdrToningPresetFile` decodes a `.hdt` preset file for the preset import.
extension RasterPrecisionDocument {
    /// The toning's plan into `bitDepth` bits (16, or 8 for a 32-bit float document):
    /// `{working_memory_plan_bytes, output_bytes}`, after every refusal the toning gives before it
    /// allocates.
    func hdrToningPlan(_ request: String, bitDepth: Int) throws -> [String: Any] {
        try precisionCString(request, maximumBytes: HdrToning.requestMaximumBytes)
        var error: UnsafeMutablePointer<CChar>?
        return try precisionJSON(
            dr_precision_doc_hdr_toning_plan(ptr, request, UInt32(clamping: bitDepth), &error), error)
    }

    /// This document's composite flattened and tone mapped by `request` into a NEW one-raster
    /// 16-bit document.
    func hdrToned(_ request: String, workingLimit: UInt64, otherRetained: UInt64,
                  control: RasterJobControl?) throws -> RasterPrecisionDocument {
        try precisionCString(request, maximumBytes: HdrToning.requestMaximumBytes)
        var error: UnsafeMutablePointer<CChar>?
        guard let result = dr_precision_doc_hdr_toning(
            ptr, request, workingLimit, otherRetained, control?.ptr, &error) else {
            throw DoctoredCoreError(message: takeErrorMessage(error, fallback: "\(HdrToning.title) failed."))
        }
        return RasterPrecisionDocument(owning: result)
    }

    /// The Toning Curve's histogram of this document's composite:
    /// `{bins: [256 alpha-weighted bins of the clamped log-luminance position], low_stops,
    /// high_stops}`, admitted and reserved under the default Local Adaptation plan.
    func hdrToningHistogram(workingLimit: UInt64, otherRetained: UInt64,
                            control: RasterJobControl?) throws -> [String: Any] {
        var error: UnsafeMutablePointer<CChar>?
        return try precisionJSON(
            dr_precision_doc_hdr_toning_histogram(ptr, workingLimit, otherRetained, control?.ptr,
                                                  &error), error)
    }
}

/// An HDR Toning preset file (`.hdt`), decoded by the core (`dr_hdr_toning_preset_file_decode`):
/// a free function like `RasterAdjustmentPresetFile`, because a preset file owns no handle. The
/// host reads the file and names the preset; every format rule and refusal sentence is the core's.
enum RasterHdrToningPresetFile {
    /// The core's cap (`hdr_toning_preset_file::MAX_FILE_BYTES`): a host reads one byte past it,
    /// so an oversized file reaches the core's own refusal without being read whole.
    static let maximumBytes = 4096

    /// `hdr_toning`'s Local Adaptation arguments the file `bytes` hold, and the core's report
    /// lines (a value imported other than as stored), or `DoctoredCoreError` carrying the core's
    /// sentence.
    static func decode(_ bytes: Data) throws -> (arguments: [String: Any], report: [String]) {
        var err: UnsafeMutablePointer<CChar>? = nil
        let json: UnsafeMutablePointer<CChar>? = bytes.withUnsafeBytes { buffer in
            dr_hdr_toning_preset_file_decode(
                buffer.bindMemory(to: UInt8.self).baseAddress, buffer.count, &err)
        }
        let object = try precisionJSON(json, err)
        guard let arguments = object["hdr_toning"] as? [String: Any],
              let report = object["report"] as? [String]
        else {
            throw DoctoredCoreError(message: "The HDR Toning preset decoded but its values could "
                + "not be read.")
        }
        return (arguments, report)
    }
}

extension RasterDocument {
    /// An 8-bit document's toning plan as `HdrToningRun` runs it: the flattened promotion, the
    /// toning, the narrowing back to 8 bits.
    func hdrToningPlan(_ request: String) throws -> [String: Any] {
        guard let ptr else { throw DoctoredCoreError(message: "Expected an 8-bit document.") }
        try precisionCString(request, maximumBytes: HdrToning.requestMaximumBytes)
        var error: UnsafeMutablePointer<CChar>?
        return try precisionJSON(dr_doc_hdr_toning_plan(ptr, request, &error), error)
    }
}
