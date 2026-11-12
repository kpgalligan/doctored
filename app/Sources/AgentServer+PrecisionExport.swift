import AppKit
import UniformTypeIdentifiers

extension AgentServer {
    /// Mirrors File > Export's depth/dither controls. Native copies retain the
    /// complete master; every flat copy reports its explicitly chosen losses.
    func savePrecisionCopy(_ arguments: [String: Any], document: ImageDocument) throws -> String {
        guard let source = document.doc, source.precision != nil else {
            throw ToolError(message: "No 16-bit or 32-bit float document is loaded.")
        }
        let path = try requiredString(arguments, "path")
        let url = URL(fileURLWithPath: (path as NSString).expandingTildeInPath)
        let name = (stringArg(arguments, "format") ?? url.pathExtension).lowercased()
        if ImageDocument.isNativeFormatName(name) {
            guard arguments["bit_depth"] == nil, arguments["dither"] == nil else {
                throw ToolError(message: "Native saves retain the document mode; depth and dithering are flat export options.")
            }
            let captured = try CapturedSave(document, type: ImageDocument.nativeTypeIdentifier)
            _ = try RasterResources.shared.admit(scratch: captured.scratchEstimate,
                own: captured.ownRetainedBytes, remedy: captured.refusalRemedy)
            try captured.run(to: url, title: "Saving Native Copy")
            return try jsonResult(["ok": true, "path": url.path, "format": ImageDocument.nativeFormatName,
                                   "bit_depth": source.isFloat ? 32 : 16,
                                   "flat_save_losses": [String]()])
        }
        // #1: a TIFF copy keeps the spot channels as extra samples (1c-2a, `keepSpotChannels`
        // below); any other flattened copy composites their overprint in the core (1c-1).
        // #1 Lab phase 1b: a Lab Color document's TIFF is a TIFF 6.0 CIELab file at its own
        // depth (`CapturedSave`'s Lab branch), mirroring File ▸ Save a Copy ▸ TIFF.
        if source.isLab, ["tiff", "tif"].contains(name) {
            let depth = source.labDepth ?? 16
            if let bits = intArg(arguments, "bit_depth"), bits != depth {
                throw ToolError(message: LabDocument.tiffDepthRefusal(depth))
            }
            guard arguments["dither"] == nil else {
                throw ToolError(message: "A Lab Color TIFF keeps the document's own depth, so it takes no dithering.")
            }
            let captured = try CapturedSave(document, type: UTType.tiff.identifier)
            _ = try RasterResources.shared.admit(scratch: captured.scratchEstimate,
                own: captured.ownRetainedBytes, remedy: captured.refusalRemedy)
            try captured.run(to: url, title: "Saving Lab TIFF")
            return try jsonResult(["ok": true, "path": url.path, "format": "TIFF", "bit_depth": depth,
                                   "flat_save_losses": try document.saveLosses(for: UTType.tiff.identifier),
                                   "working_memory_plan_bytes": captured.scratchEstimate])
        }
        // The two float deliverables are not flat-save formats; a
        // float document writes them here at 32 bits, through the export
        // workspace's own float stage, so the bytes are `export_image`'s.
        guard let format = ExportFormat.from(fileExtension: name),
              ExportFormat.flatSaveCases.contains(format) || format.isFloatOnly else {
            throw ToolError(message: "Unknown export format: \(name).")
        }
        if format.isFloatOnly && !source.isFloat {
            throw ToolError(message: "\(format.displayName) is written only from a 32-bit float document.")
        }
        // #1 phase 1b: a Grayscale document's copy defaults to its one-channel depth; an Indexed
        // document's (#1 Indexed phase 1b) and a Bitmap document's (#1 Bitmap phase 1c) to 8
        // bits with no dither, the palette or the 1-bit plane kept.
        let bits = intArg(arguments, "bit_depth")
            ?? (source.isGray ? GrayDocument.defaultBitDepth(source, format: format)
                : source.isIndexed || source.isBitmap ? 8
                : format.isFloatOnly ? 32 : 16)
        let dither = stringArg(arguments, "dither") ?? "none"
        var samples = ExportSettings.FloatSamples.f32
        if let name = stringArg(arguments, "float_samples") {
            guard let match = ExportSettings.FloatSamples(rawValue: name) else {
                throw ToolError(message: "float_samples must be f32 or f16.")
            }
            guard bits == 32 || match == .f32 else {
                throw ToolError(message: "float_samples f16 requires bit_depth 32 and tiff or exr.")
            }
            samples = match
        }
        let captured = try PrecisionExportCapture(source: PrecisionExportSource(document), format: format,
            bitDepth: bits, dither: dither,
            quality: intArg(arguments, "jpeg_quality") ?? document.jpegExportQuality,
            embedProfile: boolArg(arguments, "embed_profile") ?? document.embedColorProfile,
            stripMetadata: boolArg(arguments, "strip_metadata") ?? document.stripMetadata,
            floatSamples: samples, keepSpotChannels: format == .tiff)
        // Asked before the write, so a failed answer leaves no copy behind it.
        let losses = try captured.losses()
        let report = try captured.run(to: url)
        // A Bitmap document's kept PNG, TIFF or BMP holds 1 bit a pixel (#1 Bitmap phase 1c).
        let written = source.isBitmap && BitmapDocument.keptFormats.contains(format) ? 1 : bits
        var result: [String: Any] = ["ok": true, "path": url.path, "format": format.displayName,
            "bit_depth": written, "dither": dither, "flat_save_losses": losses,
            "master_bit_depth": source.isFloat ? 32 : 16]
        if bits == 32 {
            result["float_samples"] = format == .hdr ? "rgbe" : samples.rawValue
        }
        // A copy that kept spot channels (#1 1c-2a) names the plan it was admitted at.
        if captured.keepsSpotChannels { result["working_memory_plan_bytes"] = captured.scratchEstimate }
        result.merge(Self.savedFields(source, document, report: report)) { _, new in new }
        return try jsonResult(result)
    }
}
