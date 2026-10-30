import AppKit
import UniformTypeIdentifiers

// `PSDExportCapture` captures the document, its warm composite (byte or typed) and the core's
// preflight TOGETHER before the panel opens, and refuses an unavailable mode with the core's own
// refusal sentence. The file's depth follows the DOCUMENT — an 8-bit document writes 8-bit, a
// 16-bit one writes 16-bit through `dr_precision_doc_save_psd` with the authoritative projection —
// and there is no cross-depth export; `export_psd` reports the `bit_depth` it wrote. A 32-bit float
// document writes a 32-bit PSD or PSB or a 32-bit layered TIFF from its RGBA32
// float projection; a layered file holding a fill, a style that would be rasterized or an
// adjustment with no Photoshop record is refused with the core's own sentence
// (`FloatDocument.tools` admits both MCP tools).
//
// A layer style is WRITTEN as Photoshop's own `lfx2`/`iOpa`/blending-ranges constructs at either
// depth and reported per layer; a styled raster is RASTERIZED into its pixels only when Photoshop's
// dialog or a PSD mask cannot hold it (sub-1 px stroke, out-of-range satin distance, an enabled
// mask with density/feather); a mask with density/feather beside no style and a group whose style
// must be rasterized still refuse. An adjustment layer never refuses: one with a Photoshop record
// is written as that record and any other is baked into a pixel layer, reported as a loss.
// Every sentence shown comes from the core report — this side decides layout and nothing about
// what the file can hold.
//
// The same picture is written into one of three containers (`LayeredContainer`): a
// classic PSD, a Large Document PSB (past 30000 px a side or 2 GiB) or a TIFF with Photoshop's
// layer block. The container is chosen beside the mode and both reach the core, which owns every
// per-container limit and sentence.
//
// A CMYK document (#547 phase 3) writes a colour-mode-4 file in every container and both modes at
// its DECLARED depth (its layers, masks, groups, modes, Fill and Levels/Curves/Hue/Saturation
// records, or its composite flattened onto paper): no projection is captured, the core renders the
// composite itself, and the plan is the report's own `working_memory_plan_bytes`.
//
// A Grayscale document (#547 phase 3b) writes a colour-mode-1 file in every container and both
// modes at its DECLARED depth (`grayDepth`, 8 or 16) from its RGBA16 projection, as a 16-bit RGB
// document does; a 32-bit one (phase 3c) a 32-bit colour-mode-1 PSD or PSB from its RGBA32 float
// projection, as a float RGB document does, its layered TIFF refused by the core's report (the
// 32-bit layered TIFF is RGB only).
//
// A Lab Color document (#547 Lab phase 3b) writes a colour-mode-9 file in every container and both
// modes at its DECLARED depth (`labDepth`, 8 or 16) from its RGBA16 projection (the Lab codes in
// its twin), as a Grayscale document does; no profile is embedded.
//
// A Bitmap document (#547 Bitmap phase 1c) writes a colour-mode-0 PSD or PSB at 1 bit (1 is
// black) in both modes, its one layer being the composite: no projection is captured, the core
// writes its plane, and the plan is the report's `working_memory_plan_bytes`; a layered TIFF is
// refused by the core's report.
//
// An Indexed Color document (#547) writes a colour-mode-2 PSD or PSB at 8 bits in both modes the
// same way: its palette, transparent index and index plane, no projection, the report's plan; a
// layered TIFF is refused by the core's report.

/// Which file the layered export writes. The raw values are the MCP `format`
/// spellings; `coreValue` is the C `DrLayeredContainer`.
enum LayeredContainer: String, CaseIterable {
    case psd
    case psb
    case tiff

    var coreValue: Int32 {
        switch self {
        case .psd: return 0
        case .psb: return 1
        case .tiff: return 2
        }
    }

    var title: String {
        switch self {
        case .psd: return "Photoshop (PSD)"
        case .psb: return "Large Document (PSB)"
        case .tiff: return "TIFF with layers"
        }
    }

    var utType: UTType {
        switch self {
        case .psd: return UTType(importedAs: "com.adobe.photoshop-image")
        case .psb: return UTType(importedAs: "com.adobe.photoshop-large-image")
        case .tiff: return .tiff
        }
    }

    var fileExtension: String {
        switch self {
        case .psd: return "psd"
        case .psb: return "psb"
        case .tiff: return "tif"
        }
    }
}

/// Which picture the exported Photoshop document carries. The raw values are
/// the MCP argument spellings; `coreMode` is the C enum the core takes.
enum PSDExportMode: String, CaseIterable {
    case layers
    case flattened

    var coreMode: Int { self == .layers ? 0 : 1 }

    var title: String { self == .layers ? "Layers" : "Flattened composite" }
}

/// The core's preflight for one mode, decoded once. Everything the file
/// cannot hold is either the refusal or one of the two lists — the export
/// never drops something without saying so.
struct PSDExportReport {
    let available: Bool
    let refusal: String?
    let losses: [String]
    let observations: [String]
    let generalLimitations: [String]
    let worstCaseBytes: UInt64
    /// The working memory the adjustment stamps of this export need (0 when
    /// no adjustment layer is baked); the core's `bake_scratch_bytes`.
    let bakeScratchBytes: UInt64
    let json: [String: Any]

    init(_ json: [String: Any]) throws {
        guard let available = json["available"] as? Bool else {
            throw DoctoredCoreError(message: "The PSD export report lacks an availability.")
        }
        self.available = available
        self.refusal = json["refusal"] as? String
        self.losses = json["losses"] as? [String] ?? []
        self.observations = json["observations"] as? [String] ?? []
        self.generalLimitations = json["general_limitations"] as? [String] ?? []
        self.worstCaseBytes = (json["worst_case_bytes"] as? NSNumber)?.uint64Value ?? 0
        self.bakeScratchBytes = (json["bake_scratch_bytes"] as? NSNumber)?.uint64Value ?? 0
        self.json = json
    }
}

/// The document, its composite and its preflight captured TOGETHER, before a
/// save panel opens: an agent edit landing while the panel is up must not pair
/// one revision's pixels with another's report.
///
/// The file's depth follows the DOCUMENT: an 8-bit document writes 8-bit, a
/// 16-bit one 16-bit and a 32-bit float one 32-bit. There is no
/// cross-depth export.
struct PSDExportCapture {
    let source: RasterDocument
    let projection: RasterImage?
    let typedProjection: RasterPrecisionImage?
    let container: LayeredContainer
    let mode: PSDExportMode
    let report: PSDExportReport
    let bitDepth: Int
    let scratchEstimate: UInt64

    init(_ document: ImageDocument, container: LayeredContainer, mode: PSDExportMode) throws {
        guard let source = document.doc else {
            throw DoctoredCoreError(message: "No image loaded.")
        }
        let report = try PSDExportReport(
            source.psdExportReport(container: container, mode: mode.coreMode))
        guard report.available else {
            throw DoctoredCoreError(message: report.refusal
                ?? "This document cannot be written as a Photoshop document.")
        }
        self.source = source
        self.container = container
        self.mode = mode
        self.report = report
        // #547 Bitmap phase 1c: a Bitmap document writes a colour-mode-0 file at 1 bit from its own
        // plane; no projection is captured and the plan is the report's own. An Indexed Color one
        // (#547) writes a colour-mode-2 file at 8 bits from its palette and index plane the same way.
        if source.isBitmap || source.isIndexed {
            self.bitDepth = source.isIndexed ? 8 : 1
            self.projection = nil
            self.typedProjection = nil
            self.scratchEstimate = try PrecisionDocumentDescriptor.integer(
                report.json, "working_memory_plan_bytes")
            return
        }
        if let precision = source.precision, precision.isCmyk {
            // A CMYK document (#547) writes a colour-mode-4 file at its
            // declared depth; the core renders its composite.
            self.bitDepth = precision.cmykInfo()?["depth"] as? Int ?? 16
            self.projection = nil
            self.typedProjection = nil
        } else if let precision = source.precision {
            // A float document's projection is RGBA32 float; a Grayscale or Lab one writes
            // its declared depth (#547 phase 3b, Lab phase 3b); a Duotone one writes 8 bits,
            // Photoshop's Duotone depth (`psd_export_gray`; the report's depth).
            self.bitDepth = source.isDuotone ? 8
                : source.grayDepth ?? source.labDepth ?? (precision.isFloat ? 32 : 16)
            self.projection = nil
            self.typedProjection = try document.currentPrecisionProjection()
        } else {
            self.bitDepth = 8
            self.projection = try document.currentProjection()
            self.typedProjection = nil
        }
        // #547: a CMYK document's export is admitted by the report's own plan.
        if let precision = source.precision, precision.isCmyk {
            self.scratchEstimate = try PrecisionDocumentDescriptor.integer(
                report.json, "working_memory_plan_bytes")
            return
        }
        // The whole file is built in memory, plus the composite the writer
        // mattes, the stamps of any baked adjustment layers and the bounded
        // slack every core call reserves.
        // A 32-bit float save charges 28 B a pixel beside the worst case
        // (`psd_export_float`'s measured term); 8 and 16 bits, the
        // composite's 4 and 8 (a typed projection is RGBA16 at either Gray depth).
        let canvasBytesPerPixel: UInt64 = bitDepth == 32 ? 28 : source.precision != nil ? 8 : 4
        // #547 Duotone phase 1b: a Duotone file is one more copy of the Gray writer's bytes
        // (`duotone_files::into_duotone_file`), as the core admits it.
        let duotoneCopy: UInt64 = source.isDuotone ? report.worstCaseBytes : 0
        self.scratchEstimate = report.worstCaseBytes &+ duotoneCopy
            &+ UInt64(source.width) &* UInt64(source.height) &* canvasBytesPerPixel
            &+ report.bakeScratchBytes
            &+ 16 * 1024 * 1024
    }

    func write(to url: URL) throws {
        // #547 Bitmap phase 1c, and an Indexed Color document (#547): the core writes the plane;
        // no projection.
        if let precision = source.precision, source.isBitmap || source.isIndexed {
            let context = try PrecisionWorkContext(source: precision)
            try precision.savePSD(nil, to: url, container: container, mode: mode.coreMode,
                                  workingLimit: context.limit, otherRetained: context.otherRetained)
            return
        }
        if let precision = source.precision, precision.isCmyk {
            let context = try PrecisionWorkContext(source: precision)
            try precision.savePSD(nil, to: url, container: container, mode: mode.coreMode,
                                  workingLimit: context.limit, otherRetained: context.otherRetained)
            return
        }
        if let precision = source.precision {
            guard let typedProjection else {
                throw DoctoredCoreError(message: "No 16- or 32-bit projection was captured for this export.")
            }
            let context = try PrecisionWorkContext(source: precision, image: typedProjection)
            try precision.savePSD(typedProjection, to: url, container: container,
                                  mode: mode.coreMode, workingLimit: context.limit,
                                  otherRetained: context.otherRetained)
            return
        }
        try source.savePSD(projection, to: url, container: container, mode: mode.coreMode)
    }

    /// What did not travel with the written file, as one informational
    /// message; nil when the file carried everything this document had.
    var droppedMessage: String? {
        let items = report.losses + report.observations
        guard !items.isEmpty else { return nil }
        return "The \(container.title) file was written. What did not travel with it:\n"
            + items.map { "• " + $0 }.joined(separator: "\n")
    }
}
