import AppKit

// The existing central edit has an explicit depth-change flag; normal edits cannot change storage.
// Conversion resets a mask or channel target to the sole raster inside the same Undo transition
// (a flatten drops the masks; saved channels carry, #1).
//
// `ConversionStructure` is the one vocabulary; `PrecisionModeConversion.init` admits `.preserve` in
// every direction the "already there" guard lets through (8→16, 16→8, float→either) — `compute`
// runs `promotedStructure` / `narrowedStructureToU8(dither:)` / `narrowedStructureToU16`,
// `previewDelta` the matching core preview, and `refusal(plan:)` drops the layer prefix on an id-0
// row; `publish` keeps `intentionalDepthConversion: true` and re-selects the captured active row
// after a preserve (indices are identical). `convert_precision` lists only `mode` as required: the
// HANDLER enforces the flatten rule (absent `structure` still needs `flatten:true`; `preserve` +
// `flatten:true` is refused by name), `dry_run:true` returns `{dry_run, structure, conversion,
// would_refuse}` and edits nothing, and `AgentServer.swift`'s recording guard has ONE clause
// skipping a dry run — `ActionCatalogFacts.nonRecordable` is per-tool and must not list it.
// Recorded steps: flatten `{mode, flatten:true, dither}` unchanged; preserve `{mode,
// structure:"preserve"}` plus `dither` when `mode` is rgba8 (`PrecisionModeConversion.step`). An
// unreadable `mode` is refused with the neutral "Specify mode rgba8, rgba16 or rgba32."
// `rgba32` converts an 8- or 16-bit document INTO float, preserve only
// (`core/src/precision_conversion_float.rs`): each code decoded through the
// profile's TRC to linear light, every record carried; recorded `{mode:"rgba32",
// structure:"preserve"}`.

enum PrecisionMode: String { case rgba8, rgba16, rgba32 }
enum PrecisionDither: UInt32 {
    case none = 0, ordered = 1
    var name: String { self == .none ? "none" : "ordered" }
    static func parse(_ value: Any?) throws -> Self {
        guard let value else { return .none }
        switch value as? String {
        case "none": return .none
        case "ordered": return .ordered
        default: throw DoctoredCoreError(message: "dither must be none or ordered.")
        }
    }
}

/// Which conversion: the rendered appearance as ONE raster (`flatten`), or
/// every supported record carried to the target depth (`preserve`: 8→16 in
/// `core/src/precision_conversion_structure.rs`, 16→8 / float→16 / float→8 in
/// `core/src/precision_conversion_narrow.rs`). The Convert Precision sheet
/// opens on `preserve` unless the core's plan blocks it
/// (`ConvertPrecisionSheetController.initialStructure`); MCP
/// `convert_precision` names one explicitly (`structure`, or `flatten:true`).
/// The conversion into float (`rgba32`, `core/src/precision_conversion_float.rs`)
/// is `preserve` only.
enum ConversionStructure: String { case flatten, preserve }

/// Intentional conversion. `.flatten` preserves the captured rendered
/// appearance as ONE raster: promoting each legacy layer independently would
/// change old rounding and live adjustment evaluation, so the flattened path
/// is deliberately not that operation. `.preserve` IS that operation, in
/// every direction that exists, with every difference disclosed per record
/// by the core's plan and every uncarriable record blocking by name.
struct PrecisionModeConversion {
    let source: RasterDocument
    let revision: UInt64
    let editingState: EditingState
    let mode: PrecisionMode
    let dither: PrecisionDither
    let structure: ConversionStructure
    /// The active layer when the conversion was captured. Preserve keeps
    /// every index, so the same row is selected again after publication.
    let activeLayer: Int
    let plan: [String: Any]
    let scratch: UInt64

    static let losses = "The rendered canvas becomes one raster layer. Embedded image originals, editable layers, groups, adjustments, masks and styles are discarded. "
        + "Saved channels, alpha and spot, are kept with their names, options and inks: each value is multiplied by 257 to 16 bits or rounded to nearest at 8 bits. "
        + "The document profile, resolution, supported metadata, guides, ruler origin and global light remain. Undo restores the original document."

    static let preserveLosses = "Every layer, group, adjustment, text and shape description, mask, saved channel, style, Blend If, knockout, clipping, lock, link, label, layer comp, embedded source, warp, guide, group open state, the ruler origin and the global light carry to the target precision: "
        + "widening to 16 bits multiplies each 8-bit value by 257; narrowing to 8 bits rounds each record's RGB (nearest, or ordered dithering) and its alpha (nearest, never dithered); a 32-bit float document is tone-mapped per record. "
        + "A float document's extended range is not carried; the plan lists every record that renders differently and any record that blocks. Undo restores the original document."

    static let floatLosses = "Every layer, group, adjustment, text and shape description, mask, saved channel, style, Blend If, knockout, clipping, lock, link, label, layer comp, embedded source, guide, group open state, the ruler origin and the global light carry to 32-bit float: "
        + "each 16-bit value (an 8-bit value first widened by 257) is decoded through the document profile's tone curve to linear light, so converting back returns every original value. "
        + "The document then composites in linear light under the clip tone map; masks and channels keep their values. A blend mode, layer effect or adjustment a float document cannot hold, and a warp, block by name, and the plan lists every record that renders differently. Undo restores the original document."

    /// The refusal of an `rgba32` conversion that does not keep every layer.
    static let floatPreserveOnly = "A conversion to 32-bit float keeps every layer: pass structure \"preserve\"."

    init(_ document: ImageDocument, mode: PrecisionMode, dither: PrecisionDither,
         structure: ConversionStructure = .flatten) throws {
        guard let source = document.doc, !document.isLiveEditing, !BackgroundWork.occupied else {
            throw DoctoredCoreError(message: "Finish the current operation before converting precision.")
        }
        if mode == .rgba32 {
            // Into float: from 8 or 16 bits, every layer carried.
            guard !source.isFloat else {
                throw DoctoredCoreError(message: "The document already uses the requested precision.")
            }
            guard structure == .preserve else {
                throw DoctoredCoreError(message: Self.floatPreserveOnly)
            }
        } else {
            // A 32-bit FLOAT document converts to EITHER integer mode through
            // its own tone map (`FloatDocument.swift`), so the "already there"
            // rule is only about the two integer modes.
            guard source.isFloat || source.isHighDepth != (mode == .rgba16) else {
                throw DoctoredCoreError(message: "The document already uses the requested precision.")
            }
        }
        guard mode == .rgba8 || dither == .none else {
            throw DoctoredCoreError(message: "Dithering applies only to conversion to 8-bit.")
        }
        guard let state = document.captureEditingState() else { throw DoctoredCoreError(message: "Could not capture the conversion state.") }
        editingState = state
        self.source = source; revision = document.revision; self.mode = mode; self.dither = dither
        self.structure = structure
        activeLayer = document.activeLayerIndex
        if structure == .preserve {
            // Preserve is admitted in every direction the guard above lets
            // through: 8-bit → 16, 16-bit → 8, float → either, 8 or 16 → float.
            if mode == .rgba32 {
                plan = try source.precision?.promoteToFloatPlan() ?? source.promoteToFloatPlan()
            } else if let typed = source.precision {
                plan = try typed.narrowStructurePlan(targetDepth: mode == .rgba8 ? 8 : 16)
            } else {
                plan = try source.promoteStructurePlan()
            }
            scratch = try PrecisionDocumentDescriptor.sum(
                PrecisionDocumentDescriptor.integer(plan, "scratch_bytes"),
                PrecisionDocumentDescriptor.integer(plan, "output_bytes"))
        } else {
            plan = try source.precision?.downconvertPlan() ?? source.promoteFlattenedPlan()
            scratch = try PrecisionDocumentDescriptor.integer(plan, "scratch_bytes")
        }
    }

    /// The core plan's first blocked row as the sentence the core's
    /// `StructureConversionPlan::refusal` words it; nil when nothing blocks.
    static func refusal(plan: [String: Any]) -> String? {
        guard let row = (plan["blocked"] as? [[String: Any]])?.first else { return nil }
        // A document-level row (id 0) names no layer.
        if (row["id"] as? Int) == 0 {
            return "Preserve Layers cannot convert this document: \(row["reason"] ?? "")"
        }
        return "Preserve Layers cannot convert this document: layer \(row["index"] ?? "") "
            + "\"\(row["name"] ?? "")\": \(row["reason"] ?? "")"
    }

    /// The recorded step: flatten unchanged; preserve `{mode, structure}`,
    /// plus `dither` when narrowing to 8 bits.
    static func step(mode: PrecisionMode, structure: ConversionStructure,
                     dither: PrecisionDither) -> ActionStep {
        switch structure {
        case .flatten:
            return ActionStep(tool: "convert_precision", arguments: ["mode": mode.rawValue, "flatten": true, "dither": dither.name], note: "Convert Document Precision")
        case .preserve:
            var arguments: [String: Any] = ["mode": mode.rawValue, "structure": ConversionStructure.preserve.rawValue]
            if mode == .rgba8 { arguments["dither"] = dither.name }
            return ActionStep(tool: "convert_precision", arguments: arguments, note: "Convert Document Precision (Preserve Layers)")
        }
    }

    func compute() throws -> PreparedDocumentEdit {
        let source = source, mode = mode, dither = dither, structure = structure
        let converted: RasterDocument = try PrecisionCapturedWork.run(scratchEstimate: scratch, title: "Preparing Precision Conversion") { control in
            if mode == .rgba32 {
                guard let typed = source.precision else {
                    return try RasterDocument(precision: source.promotedToFloat(control: control))
                }
                let context = try PrecisionWorkContext(source: typed)
                return try RasterDocument(precision: typed.promotedToFloat(
                    workingLimit: context.limit, otherRetained: context.otherRetained, control: control))
            }
            if structure == .preserve {
                guard let typed = source.precision else {
                    return try RasterDocument(precision: source.promotedStructure(control: control))
                }
                let context = try PrecisionWorkContext(source: typed)
                if mode == .rgba8 {
                    return try typed.narrowedStructureToU8(dither: dither.rawValue, workingLimit: context.limit,
                                                           otherRetained: context.otherRetained, control: control)
                }
                return try RasterDocument(precision: typed.narrowedStructureToU16(
                    workingLimit: context.limit, otherRetained: context.otherRetained, control: control))
            }
            if mode == .rgba16 {
                // The float path is the DELIBERATE tone-mapped conversion,
                // not a promotion: `float_to_u16` renders, applies the
                // document's map and keeps the result linear-light.
                if source.isFloat {
                    return try RasterDocument(precision: source.floatToU16(control: control))
                }
                return try RasterDocument(precision: source.promotedFlattened(control: control))
            }
            guard let typed = source.precision else { throw DoctoredCoreError(message: "Missing 16-bit source.") }
            let context = try PrecisionWorkContext(source: typed)
            return try typed.downconverted(dither: dither.rawValue, workingLimit: context.limit,
                otherRetained: context.otherRetained, control: control)
        }
        let task = try WorkerTask { PreparedDocumentEdit(converted) }
        let prepared = try BackgroundWork.wait(task, title: "Preparing Converted Preview")
        guard prepared.preview != nil, prepared.error == nil else {
            throw DoctoredCoreError(message: prepared.error ?? "Could not prepare the converted preview.")
        }
        return prepared
    }

    /// The preserve conversion's measured difference from the source's own
    /// projection, in codes of 65535; nil for `.flatten`.
    func previewDelta(_ prepared: PreparedDocumentEdit) throws -> [String: Any]? {
        guard structure == .preserve else { return nil }
        if mode == .rgba32 {
            // The float result tone-mapped against the source's own render:
            // the narrowing preview with the converted document first.
            guard let converted = prepared.document.precision else {
                throw DoctoredCoreError(message: "Missing converted 32-bit float document.")
            }
            let source = source
            let estimate = try PrecisionDocumentDescriptor.sum(
                prepared.document.projectionScratchEstimate(), source.projectionScratchEstimate())
            return try PrecisionCapturedWork.run(scratchEstimate: estimate, title: "Measuring Converted Preview") { control in
                let context = try PrecisionWorkContext(source: converted)
                return try converted.narrowStructurePreview(source, workingLimit: context.limit,
                                                            otherRetained: context.otherRetained, control: control)
            }
        }
        if let typed = source.precision {
            let converted = prepared.document
            let estimate = try PrecisionDocumentDescriptor.sum(
                converted.projectionScratchEstimate(), source.projectionScratchEstimate())
            return try PrecisionCapturedWork.run(scratchEstimate: estimate, title: "Measuring Converted Preview") { control in
                let context = try PrecisionWorkContext(source: typed)
                return try typed.narrowStructurePreview(converted, workingLimit: context.limit,
                                                        otherRetained: context.otherRetained, control: control)
            }
        }
        guard let converted = prepared.document.precision else {
            throw DoctoredCoreError(message: "Missing converted 16-bit document.")
        }
        let source = source
        let estimate = try PrecisionDocumentDescriptor.sum(
            prepared.document.projectionScratchEstimate(), source.projectionScratchEstimate())
        return try PrecisionCapturedWork.run(scratchEstimate: estimate, title: "Measuring Converted Preview") { control in
            try source.promoteStructurePreview(converted, control: control)
        }
    }

    func publish(_ prepared: PreparedDocumentEdit, to document: ImageDocument, record: [ActionStep]) throws {
        guard document.doc === source, document.revision == revision, !document.isLiveEditing,
              document.isCurrentEditingState(editingState) else {
            throw DoctoredCoreError(message: "The document changed while conversion was prepared. No conversion was applied.")
        }
        let name = mode == .rgba32 ? "Convert to 32-bit Float (Preserve Layers)"
            : "Convert to \(mode == .rgba16 ? "16" : "8")-bit"
                + (structure == .preserve ? " (Preserve Layers)" : "")
        document.preparedEdit = prepared
        defer { document.preparedEdit = nil }
        try document.preflightPublication(prepared.document, record: record,
                                          intentionalDepthConversion: true)
        withUndoGroup(document.undoManager) {
            document.applyEdit(name, record: record, intentionalDepthConversion: true) { _ in prepared.document }
        }
        // Preserve keeps every record at its index, so the row that was
        // active is selected again (applyEdit resets a depth conversion to 0).
        if structure == .preserve, document.doc === prepared.document {
            document.selectLayer(activeLayer)
        }
    }
}

extension EditorViewController {
    @objc func convertDocumentPrecision(_ sender: Any?) {
        guard let document, document.doc != nil else { return }
        presentAsSheet(ConvertPrecisionSheetController(editor: self, document: document))
    }

    /// Image ▸ Mode ▸ 32 Bits/Channel… — the same sheet, opened on the
    /// conversion into float.
    @objc func convertToFloatPrecision(_ sender: Any?) {
        guard let document, document.doc != nil else { return }
        // #1 phase 5a: a Grayscale document's own depth edit, one undo step, no sheet.
        if document.doc?.isGray == true {
            commitGrayProfileEdit(.depth(32), profile: nil, space: nil, destination: [:],
                                  options: CmykConversionOptions(), document: document)
            return
        }
        presentAsSheet(ConvertPrecisionSheetController(editor: self, document: document, target: .rgba32))
    }
}

extension AgentServer {
    /// This file's MCP tools; merged by AgentServer+Registry.swift.
    static let precisionModeConversionHandlers: [String: ToolHandler] = [
        "convert_precision": { $0.convertPrecision },
    ]

    /// Facts for this file's tools (recording, selection, depth admission); merged in AgentServer+Registry.swift.
    static let precisionModeConversionToolFacts: [String: ToolFacts] = [
        // #1 phase 5: a CMYK document's 8/16 Bits/Channel (mode cmyk8 / cmyk16,
        // `AgentServer+CmykColour.swift`'s route).
        "convert_precision": [.commonTool, .floatTool, .cmykTool],
    ]

    /// Mirrors Image > Mode > Convert Precision… and Image > Mode > 32
    /// Bits/Channel… (`ConvertPrecisionSheetController`).
    /// An absent `structure` is the flattened conversion and still requires
    /// the reviewed explicit flatten=true; `structure:"preserve"` carries the
    /// layers to the target depth (8→16, 16→8, float→16, float→8, 8 or 16 →
    /// float) and refuses flatten=true by name. `rgba32` is preserve only:
    /// each code is decoded through the profile's tone curve to linear light.
    /// dry_run=true returns the
    /// plan and edits nothing (and records nothing: `AgentServer.swift`'s
    /// recording guard). An MCP/Action call never opens a modal prompt.
    func convertPrecision(_ a: [String: Any]) throws -> String {
        let document = try target(a)
        guard let mode = PrecisionMode(rawValue: try requiredString(a, "mode")) else {
            throw ToolError(message: "Specify mode rgba8, rgba16 or rgba32.")
        }
        var structure = ConversionStructure.flatten
        if let raw = a["structure"] {
            guard let name = raw as? String, let parsed = ConversionStructure(rawValue: name) else {
                throw ToolError(message: "structure must be flatten or preserve.")
            }
            structure = parsed
        }
        if mode == .rgba32, a["structure"] == nil || boolArg(a, "flatten") == true || structure != .preserve {
            throw ToolError(message: PrecisionModeConversion.floatPreserveOnly)
        }
        switch structure {
        case .flatten:
            guard boolArg(a, "flatten") == true else {
                throw ToolError(message: "Specify mode rgba8 or rgba16 and flatten=true. " + PrecisionModeConversion.losses)
            }
        case .preserve:
            if boolArg(a, "flatten") == true {
                throw ToolError(message: "flatten=true contradicts structure=preserve; omit flatten or pass false. "
                    + PrecisionModeConversion.preserveLosses)
            }
        }
        let capture = try PrecisionModeConversion(document, mode: mode, dither: PrecisionDither.parse(a["dither"]),
                                                  structure: structure)
        if boolArg(a, "dry_run") == true {
            let refusal: Any = structure == .preserve
                ? PrecisionModeConversion.refusal(plan: capture.plan) ?? NSNull() : NSNull()
            return try jsonResult(["ok": true, "dry_run": true, "structure": structure.rawValue,
                                   "conversion": capture.plan, "would_refuse": refusal])
        }
        let prepared = try capture.compute()
        let delta = try capture.previewDelta(prepared)
        try capture.publish(prepared, to: document, record: .notACommand)
        let losses = mode == .rgba32 ? PrecisionModeConversion.floatLosses
            : structure == .preserve ? PrecisionModeConversion.preserveLosses : PrecisionModeConversion.losses
        var reply: [String: Any] = ["ok": true, "document": summary(document), "conversion": capture.plan,
                                    "structure": structure.rawValue, "dither": capture.dither.name,
                                    "losses": losses]
        if let delta { reply["preview_delta"] = delta }
        return try jsonResult(reply)
    }

    @inline(never) func precisionCatalog() -> [[String: Any]] {
        [tool("get_precision", "Document precision and the qualified operation/target matrix shared by UI, MCP and Actions.", ["document_id": Self.docIDProperty]),
         tool("convert_precision", "Intentionally convert document precision. structure \"flatten\" (the default) converts the captured rendered appearance to one raster layer and loses editable layers/groups/adjustments/masks, keeping every saved channel (alpha and spot: name, options, ink, each value ×257 to 16 bits or nearest to 8 bits; a 32-bit document's 16-bit channel values carry unchanged to 16 bits). structure \"preserve\" converts keeping every layer, group, adjustment, text/shape description, mask, channel, style, Blend If, knockout, clipping, lock, link, label, comp, embedded source, warp, guide and the ruler origin: 8-bit to 16 widens each value by 257; 16-bit to 8 quantizes each record (RGB per dither, alpha nearest); a float document converts to either mode with each record tone-mapped by its own map (then quantized for 8-bit). mode rgba32 converts an 8- or 16-bit document to 32-bit float and requires structure \"preserve\": each 16-bit value (an 8-bit value first times 257) is decoded through the document profile's tone curve to linear light, alpha is value/65535, masks and channels keep their values, the result composites in linear light under the clip tone map, and converting back returns every original value; a blend mode, effect or adjustment a float document cannot hold, a warp, a profile with no matrix/TRC model and a result too large to save and reopen at 32 bits block by name. Its reply carries the plan (rendering_differences, discarded, blocked) and preview_delta, and a blocked record refuses by name with the document unchanged (a layer holding a 32-bit float Camera RAW development is one: redevelop it with raw.output_encoding sdr16, or rasterize it, first). flatten=true is required unless structure=preserve; dry_run=true returns the plan without editing. One Undo restores the original. Promotion adds no missing source detail; ordered RGB dithering applies only to 8-bit output, never alpha.",
              ["document_id": Self.docIDProperty, "mode": ["type": "string", "enum": ["rgba8", "rgba16", "rgba32"]],
               "structure": ["type": "string", "enum": ["flatten", "preserve"]],
               "flatten": ["type": "boolean"], "dither": ["type": "string", "enum": ["none", "ordered"]],
               "dry_run": ["type": "boolean"]], required: ["mode"])]
    }
}
