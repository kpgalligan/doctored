import AppKit

// Image ▸ Mode ▸ Grayscale / RGB Color… / CMYK Color… / Lab Color (#1 phases 1, 1a and 5b, Lab
// phase 1): the conversion job
// and its ONE undo step, published through the Convert Precision path (`applyEdit(…,
// intentionalDepthConversion: true)`), and the three items' validation (the current mode checked
// and disabled). The sheet is `CmykModeSheetController`; Grayscale asks "Discard color
// information?" instead (`convertToGrayscaleMode:`); the MCP twin is `convert_mode`
// (`AgentServer+CmykMode.swift`).
//
// The conversions are the core's (`cmyk_convert`, `gray_convert`). An 8-bit RGB document is
// promoted first — every record carried (`dr_doc_promote_structure`), or its rendered appearance
// when it must be flattened (`promotedFlattened`) — and converted at depth 8; an 8-bit CMYK or
// Gray document converts to a 16-bit RGB document and is then narrowed to 8 bits
// (`narrowedStructureToU8`, dither none), so a document keeps its depth through Mode. CMYK →
// Grayscale runs through 16-bit ProPhoto RGB (relative colorimetric with BPC) in the same job. A
// 32-bit RGB document converts to 32-bit Grayscale and back; only the
// CMYK arms refuse a float document. Since Gray phase 5b an 8- or 16-bit Grayscale document
// converts to CMYK directly, through the same RGB → CMYK conversion on its twin's R = G = B,
// keeping its declared depth, and since Lab phase 5a an 8- or 16-bit Lab Color document does too,
// through the core's 16-bit ProPhoto RGB stage (`lab_cmyk`, CMYK → Lab's rule reversed). Since Lab
// phase 5a-ii one converts to Grayscale directly too (`lab_gray`: each pixel's L* through the Gray
// profile's tone curve), keeping its declared depth.
//
// Type, shape and fill layers (#1's artwork batch) stay editable toward CMYK, RGB and (Gray
// phase 6a-ii) Grayscale: the core converts their pixels and drops their descriptions, and the job
// re-renders each captured layer with its colours converted (`reinkingArtwork`, CmykArtwork.swift;
// toward Grayscale greyed by the Gray arm) before the display preparation. A CMYK source's inks
// toward Grayscale or Lab Color take the pixels' CMYK → RGB leg (`rgbStageOptions`). A conversion
// that flattens (the plan's `flattens`; an 8-bit source under Flatten that needs it) captures
// nothing: there is one flattened layer and no description to carry. Since Lab phase 6a-i Lab
// Color carries them both ways too (its descriptions stay authored sRGB, the core renders them as
// L*a*b* codes: `lab_artwork`), and shares Grayscale's RGB stage (`rgbStage`). Since Indexed
// phase 1a an 8-bit RGB or Grayscale document converts to Indexed Color through its sheet (the
// `.indexed` arms, `EditorViewController+IndexedMode.swift`, Grayscale's RGB stage), and an
// Indexed one back to RGB Color at once (the RGB document its storage is, narrowed to 8 bits) or
// to Grayscale through the unchanged Gray arm; it captures no artwork either way. Since Indexed
// phase 1c an Indexed document converts to CMYK Color and Lab Color directly: the palette expanded
// exactly (`indexedExpanded`), then the RGB → CMYK and RGB → Lab routes at 8 bits. Since Bitmap
// phase 1a an 8-bit Grayscale document converts to Bitmap through
// its sheet (the `.bitmap` arms, `EditorViewController+BitmapMode.swift`), and a Bitmap one back to
// Grayscale with a Size Ratio (the `.gray` arms on a Bitmap source); every other mode refuses a
// Bitmap document by name (`BitmapDocument.modeRefusal`), and it captures no artwork either way.
// Since Duotone phase 1b a Duotone document converts to RGB, CMYK, Lab and Indexed Color through
// the render of its inks (`duotonePlan`, `duotoneRendered`, `EditorViewController+DuotoneMode.swift`),
// and since the Duotone close to Grayscale and Bitmap through the inks' luminance in its own Gray
// profile (`duotoneToGray`, `duotoneToBitmap`); it captures no artwork, and asks the Flatten
// question for every target.

/// Which mode Image ▸ Mode converts to.
enum ColorModeTarget: String {
    case cmyk, rgb, gray, lab, indexed
    // #1 Duotone phase 1a: Image ▸ Mode ▸ Duotone… (`DuotoneEdit`, never this conversion).
    case duotone
    // #1 Bitmap phase 1a.
    case bitmap

    var title: String {
        switch self {
        case .cmyk: return "CMYK Color"
        case .rgb: return "RGB Color"
        case .gray: return "Grayscale"
        case .lab: return "Lab Color"
        case .indexed: return "Indexed Color"
        case .duotone: return "Duotone"
        case .bitmap: return "Bitmap"
        }
    }

    /// The status bar's short name for the mode (`RasterDocument.modeStatusText`).
    var statusName: String {
        switch self {
        case .cmyk: return "CMYK"
        case .rgb: return "RGB"
        case .gray: return "Grayscale"
        case .lab: return "Lab"
        case .indexed: return "Indexed"
        case .duotone: return "Duotone"
        case .bitmap: return "Bitmap"
        }
    }
}

/// A Grayscale or Lab Color conversion that must flatten first, found inside the job (an 8-bit
/// source's promoted structure, or the RGB stage of a CMYK source): the core's sentence, shown
/// beside the Flatten choice (in-band over MCP). Nothing was published.
struct ModeFlattenRequired: LocalizedError {
    let reason: String
    var errorDescription: String? { reason }
}

/// One captured Mode conversion: the source revision, its target, the destination profile and
/// the options, planned by the core before anything is computed.
struct CmykModeConversion {
    let source: RasterDocument
    let revision: UInt64
    let editingState: EditingState
    let target: ColorModeTarget
    /// The destination profile: CMYK bytes toward CMYK (nil = the working CMYK profile), RGB
    /// bytes toward RGB (nil = sRGB), Gray ICC bytes toward Grayscale (nil = `graySpace`; only a
    /// Lab document's Convert to Profile passes one, #1 Lab phase 6a-ii).
    let profile: Data?
    /// The destination as `convert_mode` names it (`profile`/`name` toward CMYK, `rgb_space`
    /// toward RGB, `gray_space` toward Grayscale), so the recorded step replays to the profile
    /// that was applied.
    let destination: [String: Any]
    let options: CmykConversionOptions
    /// Toward Grayscale, the Gray working space's wire name (Color Settings' by default).
    let graySpace: String
    /// Toward Indexed Color, the sheet's options in the core's form (`IndexedDocument.options`),
    /// without Flatten (`options.flatten`).
    let indexedOptions: [String: Any]
    /// Toward Bitmap, the sheet's options in the core's form (`BitmapDocument.options`), without
    /// Flatten; from a Bitmap document toward Grayscale, its `size_ratio`.
    let bitmapOptions: [String: Any]
    let activeLayer: Int
    /// Whether the source is an 8-bit document (promoted toward CMYK and Grayscale, narrowed back
    /// toward RGB).
    let eightBit: Bool
    let scratch: UInt64
    /// The described layers re-rendered after the conversion; empty when it flattens.
    let captures: [ArtworkCapture]
    /// The colour end the captured descriptions are in.
    let sourceEnd: ArtworkColorEnd

    init(_ document: ImageDocument, target: ColorModeTarget, profile: Data?,
         destination: [String: Any] = [:], options: CmykConversionOptions,
         graySpace: String? = nil, indexedOptions: [String: Any] = [:],
         bitmapOptions: [String: Any] = [:]) throws {
        guard let source = document.doc, !document.isLiveEditing, !BackgroundWork.occupied else {
            throw DoctoredCoreError(message: "Finish the current operation before converting the mode.")
        }
        // #1 Duotone: Duotone is `DuotoneEdit`'s (`EditorViewController+DuotoneMode.swift`); a
        // Duotone source converts through the render, or toward Grayscale and Bitmap through the
        // inks' luminance (`duotonePlan`).
        if target == .duotone {
            throw DoctoredCoreError(message: "Duotone converts through Image ▸ Mode ▸ Duotone….")
        }
        guard source.colorMode != target else {
            throw DoctoredCoreError(message: "The document is already in \(target.title) mode.")
        }
        if source.isFloat, target == .cmyk || target == .lab || source.isCmyk {
            throw DoctoredCoreError(message: "\(target.title) is unavailable in a 32-bit float document; "
                + "convert to 16 Bits/Channel first.")
        }
        // #1 Bitmap phase 1a: a Bitmap document converts to Grayscale only, and Bitmap is made
        // from 8-bit Grayscale only; both asked before the Indexed check.
        if source.isBitmap, target != .gray {
            throw DoctoredCoreError(message: BitmapDocument.modeRefusal(target))
        }
        if target == .bitmap, let reason = BitmapDocument.sourceRefusal(source) {
            throw DoctoredCoreError(message: reason)
        }
        // #1 Indexed phase 1a: an 8-bit RGB or Grayscale source only.
        if target == .indexed, let reason = IndexedDocument.sourceRefusal(source) {
            throw DoctoredCoreError(message: reason)
        }
        guard let state = document.captureEditingState() else {
            throw DoctoredCoreError(message: "Could not capture the conversion state.")
        }
        self.source = source
        revision = document.revision
        editingState = state
        self.target = target
        self.profile = profile
        self.destination = destination
        self.graySpace = graySpace ?? ColorSettings.workingGray.rawValue
        self.indexedOptions = indexedOptions
        self.bitmapOptions = bitmapOptions
        activeLayer = document.activeLayerIndex
        var options = options
        switch target {
        case .cmyk:
            // A typed source is never promoted (`eightBit` false); an 8-bit Gray or Lab one converts
            // at its declared 8 bits (#1 Gray phase 5b, Lab phase 5a).
            eightBit = !source.isHighDepth
            options.depth = (eightBit || Self.declaredDepth(source) == 8) ? 8 : 16
        case .rgb:
            eightBit = Self.declaredDepth(source) == 8
            options.depth = nil
        case .gray, .lab, .indexed, .duotone, .bitmap:
            eightBit = !source.isHighDepth || Self.declaredDepth(source) == 8
            options.depth = nil
        }
        self.options = options
        let plan = try Self.scratch(source: source, target: target, profile: profile, options: options,
                                    graySpace: self.graySpace, eightBit: eightBit,
                                    indexed: indexedOptions, bitmap: bitmapOptions)
        scratch = plan.bytes
        // A Gray source's type, shape and fill layers re-render in RGB or CMYK (#1 phase 6a-i),
        // an RGB or CMYK source's in Grayscale (Gray phase 6a-ii), and a Lab source's or target's
        // both ways (Lab phase 6a-i).
        // A Duotone source's described layers render through its inks as pixels (#1 Duotone 1b).
        captures = plan.flattens || target == .indexed || source.isIndexed || target == .bitmap
            || source.isBitmap || source.isDuotone ? []
            : try source.artworkCaptures()
        sourceEnd = source.artworkColorEnd
    }

    /// The depth a CMYK, Gray or Lab document declares (8, 16 or 32); nil for an RGB document.
    static func declaredDepth(_ doc: RasterDocument) -> Int? {
        if doc.isBitmap { return 8 }
        if doc.isIndexed { return 8 }
        if doc.isLab { return doc.labDepth }
        if doc.isGray { return (doc.precisionInfo?["gray"] as? [String: Any])?["depth"] as? Int }
        if doc.isCmyk { return doc.precision?.cmykInfo()?["depth"] as? Int }
        return nil
    }

    /// The Grayscale options the core reads (`gray_convert`): the space, Flatten and, from a
    /// u16 source, the depth.
    static func grayOptions(space: String, flatten: Bool, eightBit: Bool, float: Bool) -> [String: Any] {
        var object: [String: Any] = ["space": space, "flatten": flatten]
        if !float { object["depth"] = eightBit ? 8 : 16 }
        return object
    }

    /// The RGB Color options a Gray source's Convert to Profile reads; a 32-bit source takes the
    /// float conversion's one rule (relative colorimetric, no BPC: `float_color`), so none.
    static func rgbOptions(_ options: CmykConversionOptions, float: Bool) -> [String: Any] {
        float ? [:]
            : ["intent": options.intent.rawValue, "black_point_compensation": options.blackPointCompensation]
    }

    /// The CMYK → Grayscale and CMYK → Lab Color RGB stage's destination: the built-in ProPhoto
    /// RGB, relative colorimetric with BPC.
    static let rgbStageOptions: CmykConversionOptions = {
        var options = CmykConversionOptions()
        options.intent = .relative
        options.blackPointCompensation = true
        return options
    }()

    /// The core's plan for a typed source toward `target` (the sheet's and `init`'s question).
    static func plan(source: RasterDocument, target: ColorModeTarget, profile: Data?,
                     options: CmykConversionOptions, graySpace: String? = nil,
                     indexed: [String: Any] = [:], bitmap: [String: Any] = [:]) throws -> [String: Any] {
        guard let typed = source.precision else {
            throw DoctoredCoreError(message: "The document is not a typed document.")
        }
        // #1 Duotone: the core's Duotone plans (`duotonePlan`).
        if source.isDuotone {
            return try duotonePlan(source: source, typed: typed, target: target, profile: profile,
                                   options: options, indexed: indexed, bitmap: bitmap)
        }
        switch target {
        case .duotone:
            throw DoctoredCoreError(message: "Duotone converts through Image ▸ Mode ▸ Duotone….")
        // #1 Bitmap phase 1a: the core's own plans (`bitmap_convert`).
        case .bitmap:
            return try typed.convertToBitmapPlan(options: bitmapCoreOptions(bitmap, flatten: options.flatten))
        case .gray where source.isBitmap:
            return try typed.convertBitmapToGrayPlan(options: bitmap)
        case .rgb where source.isIndexed:
            return indexedToRGBPlan(source)
        // #1 Indexed phase 1c: the expanded palette's own plans (`indexedExpanded`).
        case .cmyk where source.isIndexed:
            return try indexedExpanded(typed).convertToCMYKPlan(profile: profile, options: options)
        case .lab where source.isIndexed:
            return try labPlan(source: source, typed: indexedExpanded(typed), options: options)
        case .indexed:
            return try typed.convertToIndexedPlan(options: indexedCoreOptions(indexed, flatten: options.flatten))
        case .cmyk:
            return try typed.convertToCMYKPlan(profile: profile, options: options)
        case .rgb where source.isLab:
            return try typed.convertLabToRGBPlan(profile: profile ?? RasterProfile.builtin(.sRGB),
                                                 options: rgbOptions(options, float: false))
        case .lab:
            return try labPlan(source: source, typed: typed, options: options)
        case .rgb where source.isGray:
            return try typed.convertGrayToRGBPlan(profile: profile ?? RasterProfile.builtin(.sRGB),
                                                  options: rgbOptions(options, float: source.isFloat))
        case .rgb:
            return try typed.convertCMYKToRGBPlan(profile: profile, options: options)
        case .gray where source.isCmyk:
            // The RGB stage's plan, its working memory the larger of it and the Gray stage's own
            // figure, which the core plans over the CMYK leaves' pixels (`gray_convert`).
            var stage = rgbStageOptions
            stage.flatten = options.flatten
            var plan = try typed.convertCMYKToRGBPlan(profile: RasterProfile.builtin(.proPhotoRGB),
                                                      options: stage)
            let gray = try typed.convertToGrayPlan(
                profile: profile,
                options: grayOptions(space: graySpace ?? ColorSettings.workingGray.rawValue,
                                     flatten: options.flatten, eightBit: declaredDepth(source) == 8,
                                     float: false))
            plan["working_memory_plan_bytes"] = max(
                try PrecisionDocumentDescriptor.integer(plan, "working_memory_plan_bytes"),
                try PrecisionDocumentDescriptor.integer(gray, "working_memory_plan_bytes"))
            return plan
        case .gray:
            return try typed.convertToGrayPlan(
                profile: profile,
                options: grayOptions(space: graySpace ?? ColorSettings.workingGray.rawValue,
                                     flatten: options.flatten, eightBit: declaredDepth(source) == 8,
                                     float: source.isFloat))
        }
    }

    /// The core's plan for a typed source; for an 8-bit one a conservative figure, since the core
    /// plans the typed conversion only once the document is promoted: the promoted conversion is
    /// admitted INSIDE this job's reservation, so it is at least that plan's own terms — 28 B a
    /// pixel (`cmyk_convert::TO_CMYK_BYTES_PER_PIXEL`, 41 toward Grayscale,
    /// `gray_convert::TO_GRAY_BYTES_PER_PIXEL`) and 160 MiB for the capped `[f64; 4]` ink memo
    /// (123 MiB), the view link, the profile and bookkeeping. The promotion (8 B/px) runs first and
    /// has ended by then. `scripts/cmyk-admission/measure.py` `to-cmyk-8` and
    /// `scripts/gray-admission/measure.py` `to-gray-8` hold it to ⌈1.25 × its measured peak⌉; a
    /// miss raises the per-pixel term. CMYK → Grayscale reserves the larger of the RGB stage's plan
    /// and the Gray stage's (`plan`). Also answers whether the conversion flattens: the core
    /// plan's `flattens` for a typed source, Flatten on an 8-bit source that needs it (`compute`'s
    /// own predicate).
    private static func scratch(source: RasterDocument, target: ColorModeTarget, profile: Data?,
                                options: CmykConversionOptions, graySpace: String, eightBit: Bool,
                                indexed: [String: Any] = [:], bitmap: [String: Any] = [:]) throws
        -> (bytes: UInt64, flattens: Bool) {
        let canvas = UInt64(source.width) &* UInt64(source.height)
        if source.precision != nil {
            let plan = try Self.plan(source: source, target: target, profile: profile, options: options,
                                     graySpace: graySpace, indexed: indexed, bitmap: bitmap)
            if let refusal = plan["refusal"] as? String {
                // From Duotone every target asks the Flatten question (#1 Duotone phase 1b).
                if target == .gray || target == .lab || target == .indexed || target == .bitmap
                    || source.isDuotone,
                   plan["flatten_required"] as? Bool == true,
                   !options.flatten {
                    throw ModeFlattenRequired(reason: refusal)
                }
                throw DoctoredCoreError(message: refusal)
            }
            let bytes = try PrecisionDocumentDescriptor.integer(plan, "working_memory_plan_bytes")
            let flattens = plan["flattens"] as? Bool ?? false
            switch target {
            case .rgb where eightBit:
                // Toward RGB at 8 bits the 16-bit result is narrowed again: its own planes and the
                // narrowing's fixed reservation beside them.
                return (max(bytes, canvas &* 16 &+ 16 * 1024 * 1024), flattens)
            case .cmyk where source.isIndexed, .lab where source.isIndexed:
                // #1 Indexed phase 1c: the expanded palette's plan, but never less than Indexed →
                // RGB's own term (`indexedToRGBPlan`, 41 B a pixel): the CMYK plan alone measured
                // short (`scripts/indexed-admission/measure.py` `to-cmyk-from-indexed`).
                let floor = try PrecisionDocumentDescriptor.integer(indexedToRGBPlan(source),
                                                                    "working_memory_plan_bytes")
                return (max(bytes, floor), flattens)
            default:
                return (bytes, flattens)
            }
        }
        let pixels = canvas &* UInt64(max(1, source.layerCount))
        if target == .gray || target == .lab || target == .indexed {
            // #1 Indexed phase 1b: the Adaptive palette's exact terms beside the core's own
            // (`indexed_palette::ADAPTIVE_BYTES`, 64 MiB, and 8 B a distinct colour, at most one a
            // canvas pixel and 2²⁴), which the promoted conversion reserves inside this job.
            let adaptive: UInt64 = target == .indexed && indexed["palette"] as? String == "adaptive"
                ? 64 * 1024 * 1024 &+ 8 &* min(canvas, 1 << 24) : 0
            return (pixels &* 41 &+ 160 * 1024 * 1024 &+ adaptive, options.flatten)
        }
        return (pixels &* 28 &+ 160 * 1024 * 1024, options.flatten && eightBitNeedsFlatten(source))
    }

    /// Whether an 8-bit source holds anything a phase-1 CMYK document cannot, so it must be
    /// flattened (the typed plan's `flatten_required`, asked of the byte layers directly).
    static func eightBitNeedsFlatten(_ doc: RasterDocument) -> Bool {
        (0..<doc.layerCount).contains { index in
            doc.layerIsGroup(index) || doc.layerIsAdjustment(index) || doc.layerHasMask(index)
                || doc.layerHasStyle(index) || doc.layerClipped(index)
                || doc.layerInfo(index)?.blendMode != DR_BLEND_NORMAL
        }
    }

    func compute() throws -> PreparedDocumentEdit {
        let source = source, target = target, profile = profile, options = options, eightBit = eightBit
        let graySpace = graySpace, indexedOptions = indexedOptions, bitmapOptions = bitmapOptions
        let title = "Converting to \(target.title)"
        let pixels: RasterDocument = try PrecisionCapturedWork.run(scratchEstimate: scratch, title: title) { control in
            try control.beginCompoundOperation()
            switch target {
            case .cmyk:
                let typed: RasterPrecisionDocument
                var start = eightBit ? 0.4 : 0
                if let precision = source.precision, source.isDuotone {
                    // #1 Duotone phase 1b: the render through the inks, then RGB → CMYK.
                    typed = try Self.duotoneRendered(precision, options: options, control: control,
                                                     to: 0.4)
                    start = 0.4
                } else if let precision = source.precision {
                    typed = try Self.indexedExpanded(precision)
                } else {
                    typed = try control.withChildOperation(from: 0, to: 0.4) { child in
                        options.flatten && Self.eightBitNeedsFlatten(source)
                            ? try source.promotedFlattened(control: child)
                            : try source.promotedStructure(control: child)
                    }
                }
                let context = try PrecisionWorkContext(source: typed)
                let cmyk = try control.withChildOperation(from: start, to: 1) { child in
                    try typed.convertToCMYK(profile: profile, options: options, workingLimit: context.limit,
                                            otherRetained: context.otherRetained, control: child)
                }
                return try RasterDocument(precision: cmyk)
            case .rgb:
                guard let typed = source.precision else {
                    throw DoctoredCoreError(message: "The document is not a CMYK, Grayscale or Lab Color "
                        + "document.")
                }
                let context = try PrecisionWorkContext(source: typed)
                let rgb = try control.withChildOperation(from: 0, to: eightBit ? 0.6 : 1) { child in
                    source.isDuotone
                        // #1 Duotone phase 1b: the render, then Convert to Profile (the core's).
                        ? try typed.convertDuotoneToRGB(profile: profile ?? RasterProfile.builtin(.sRGB),
                                                        options: Self.duotoneRGBOptions(options),
                                                        workingLimit: context.limit,
                                                        otherRetained: context.otherRetained, control: child)
                        : source.isIndexed
                        ? try typed.convertIndexedToRGB()
                        : source.isLab
                        ? try typed.convertLabToRGB(profile: profile ?? RasterProfile.builtin(.sRGB),
                                                    options: Self.rgbOptions(options, float: false),
                                                    workingLimit: context.limit,
                                                    otherRetained: context.otherRetained, control: child)
                        : source.isGray
                        ? try typed.convertGrayToRGB(profile: profile ?? RasterProfile.builtin(.sRGB),
                                                     options: Self.rgbOptions(options, float: source.isFloat),
                                                     workingLimit: context.limit,
                                                     otherRetained: context.otherRetained, control: child)
                        : try typed.convertCMYKToRGB(profile: profile, options: options,
                                                     workingLimit: context.limit,
                                                     otherRetained: context.otherRetained, control: child)
                }
                guard eightBit else { return try RasterDocument(precision: rgb) }
                let narrowContext = try PrecisionWorkContext(source: rgb)
                return try control.withChildOperation(from: 0.6, to: 1) { child in
                    try rgb.narrowedStructureToU8(dither: 0, workingLimit: narrowContext.limit,
                                                  otherRetained: narrowContext.otherRetained, control: child)
                }
            case .gray where source.isBitmap:
                return try RasterDocument(precision: Self.bitmapToGray(
                    source, bitmap: bitmapOptions, control: control))
            // #1 Duotone: the inks' luminance in the Gray twin, then Bitmap (the core's).
            case .gray where source.isDuotone:
                return try RasterDocument(precision: Self.duotoneToGray(
                    source, options: options, control: control))
            case .bitmap where source.isDuotone:
                return try RasterDocument(precision: Self.duotoneToBitmap(
                    source, options: options, bitmap: bitmapOptions, control: control))
            case .bitmap:
                return try RasterDocument(precision: Self.bitmap(
                    source, options: options, bitmap: bitmapOptions, control: control))
            case .gray:
                return try RasterDocument(precision: Self.grayscale(
                    source, profile: profile, options: options, space: graySpace, eightBit: eightBit,
                    control: control))
            case .lab:
                return try RasterDocument(precision: Self.lab(
                    source, options: options, eightBit: eightBit, control: control))
            case .indexed:
                return try RasterDocument(precision: Self.indexed(
                    source, options: options, indexed: indexedOptions, control: control))
            case .duotone:
                throw DoctoredCoreError(message: "Duotone converts through Image ▸ Mode ▸ Duotone….")
            }
        }
        // The captured type, shape and fill layers, re-rendered in the new mode (main thread).
        // A CMYK source's inks toward Grayscale or Lab Color take the pixels' CMYK → RGB leg
        // (`rgbStage`).
        let inkOptions = source.isCmyk && (target == .gray || target == .lab)
            ? Self.rgbStageOptions : options
        let converted = captures.isEmpty ? pixels
            : try pixels.reinkingArtwork(captures, from: sourceEnd, options: inkOptions)
        let task = try WorkerTask { PreparedDocumentEdit(converted) }
        let prepared = try BackgroundWork.wait(task, title: "Preparing Converted Preview")
        guard prepared.preview != nil, prepared.error == nil else {
            throw DoctoredCoreError(message: prepared.error ?? "Could not prepare the converted preview.")
        }
        return prepared
    }

    /// The Grayscale arms inside the job: the source's RGB stage (`rgbStage`), then the core's
    /// plan and conversion of it into `profile` (a Gray ICC), else `space`. A record the mode
    /// cannot hold ends the job with `ModeFlattenRequired`.
    private static func grayscale(_ source: RasterDocument, profile: Data?,
                                  options: CmykConversionOptions, space: String, eightBit: Bool,
                                  control: RasterJobControl) throws -> RasterPrecisionDocument {
        let gray = grayOptions(space: space, flatten: options.flatten, eightBit: eightBit,
                               float: source.isFloat)
        let (rgb, start) = try rgbStage(source, options: options, control: control)
        let plan = try rgb.convertToGrayPlan(profile: profile, options: gray)
        if let refusal = plan["refusal"] as? String {
            if plan["flatten_required"] as? Bool == true { throw ModeFlattenRequired(reason: refusal) }
            throw DoctoredCoreError(message: refusal)
        }
        let context = try PrecisionWorkContext(source: rgb)
        return try control.withChildOperation(from: start, to: 1) { child in
            try rgb.convertToGray(profile: profile, options: gray, workingLimit: context.limit,
                                  otherRetained: context.otherRetained, control: child)
        }
    }

    /// The typed document Grayscale and Lab Color convert, inside the job, and the progress it
    /// ends at: an 8-bit RGB source promoted (or flattened) first — a failure is
    /// `ModeFlattenRequired` —, a CMYK source through 16-bit ProPhoto RGB (`rgbStageOptions`), an
    /// Indexed source expanded (`indexedExpanded`), a typed RGB or Gray source as it is.
    static func rgbStage(_ source: RasterDocument, options: CmykConversionOptions,
                         control: RasterJobControl) throws -> (RasterPrecisionDocument, Double) {
        // #1 Duotone phase 1b: a Duotone source's RGB stage is the render through its inks.
        if let typed = source.precision, source.isDuotone {
            return (try duotoneRendered(typed, options: options, control: control, to: 0.5), 0.5)
        }
        if let typed = source.precision, typed.isCmyk {
            var stage = rgbStageOptions
            stage.flatten = options.flatten
            let context = try PrecisionWorkContext(source: typed)
            let rgb = try control.withChildOperation(from: 0, to: 0.5) { child in
                try typed.convertCMYKToRGB(profile: RasterProfile.builtin(.proPhotoRGB), options: stage,
                                           workingLimit: context.limit, otherRetained: context.otherRetained,
                                           control: child)
            }
            return (rgb, 0.5)
        }
        if let typed = source.precision { return (try indexedExpanded(typed), 0) }
        do {
            let rgb = try control.withChildOperation(from: 0, to: 0.4) { child in
                options.flatten ? try source.promotedFlattened(control: child)
                    : try source.promotedStructure(control: child)
            }
            return (rgb, 0.4)
        } catch let error as JobCancelled {
            throw error
        } catch {
            throw ModeFlattenRequired(reason: error.localizedDescription)
        }
    }

    /// ONE undo step, through Convert Precision's publication (`PrecisionModeConversion.publish`).
    func publish(_ prepared: PreparedDocumentEdit, to document: ImageDocument, record: [ActionStep]) throws {
        guard document.doc === source, document.revision == revision, !document.isLiveEditing,
              document.isCurrentEditingState(editingState) else {
            throw DoctoredCoreError(message: "The document changed while the conversion was prepared. "
                + "No conversion was applied.")
        }
        let name = "Convert to \(target.title)"
        document.preparedEdit = prepared
        defer { document.preparedEdit = nil }
        try document.preflightPublication(prepared.document, record: record,
                                          intentionalDepthConversion: true)
        withUndoGroup(document.undoManager) {
            document.applyEdit(name, record: record, intentionalDepthConversion: true) { _ in prepared.document }
        }
        // #1 Indexed phase 1c: the Indexed Color sheet's Previous palette.
        if target == .indexed, document.doc === prepared.document {
            IndexedPalettes.notePrevious(prepared.document)
        }
        if document.doc === prepared.document, prepared.document.isValidIndex(activeLayer) {
            document.selectLayer(activeLayer)
        }
    }

    /// The recorded step (`convert_mode`'s arguments).
    func step() -> ActionStep {
        var arguments: [String: Any] = ["mode": target.rawValue, "intent": options.intent.rawValue,
                                        "black_point_compensation": options.blackPointCompensation,
                                        "flatten": options.flatten]
        // A Duotone source keeps its own Gray profile, so it records no space.
        if target == .gray, !source.isBitmap, !source.isDuotone {
            arguments["gray_space"] = graySpace
        }
        // #1 Bitmap phase 1a: the method and, only when sent, the Output resolution; from a
        // Bitmap document, the Size Ratio.
        if target == .bitmap || source.isBitmap {
            arguments.merge(bitmapOptions) { _, own in own }
        }
        if target == .indexed { arguments.merge(Self.indexedStepArguments(indexedOptions)) { _, own in own } }
        arguments.merge(destination) { _, recorded in recorded }
        return ActionStep(tool: "convert_mode", arguments: arguments, note: "Convert to \(target.title)")
    }
}

extension EditorViewController {
    @objc func convertToCMYKMode(_ sender: Any?) {
        presentColorModeSheet(.cmyk)
    }

    @objc func convertToRGBMode(_ sender: Any?) {
        // #1 Indexed phase 1a: an Indexed document converts at once, with no sheet.
        if document?.doc?.isIndexed == true { convertIndexedToRGBMode(); return }
        presentColorModeSheet(.rgb)
    }

    private func presentColorModeSheet(_ target: ColorModeTarget) {
        guard let document, document.doc != nil else { NSSound.beep(); return }
        presentAsSheet(CmykModeSheetController(editor: self, document: document, target: target))
    }

    /// Image ▸ Mode ▸ RGB Color / CMYK Color / Grayscale / Lab Color: the current mode is checked
    /// and disabled; a live edit disables all four; CMYK Color and Lab Color are disabled in a
    /// 32-bit document. nil for every other item.
    func validateCmykModeItem(_ item: NSValidatedUserInterfaceItem) -> Bool? {
        guard let action = item.action else { return nil }
        let target: ColorModeTarget
        switch action {
        case #selector(convertToCMYKMode(_:)): target = .cmyk
        case #selector(convertToRGBMode(_:)): target = .rgb
        case #selector(convertToGrayscaleMode(_:)): target = .gray
        case #selector(convertToLabMode(_:)): target = .lab
        case #selector(convertToIndexedMode(_:)): target = .indexed
        case #selector(convertToDuotoneMode(_:)): target = .duotone
        case #selector(convertToBitmapMode(_:)): target = .bitmap
        default: return nil
        }
        guard let document, let doc = document.doc else { return false }
        let current = doc.colorMode == target
        (item as? NSMenuItem)?.state = current ? .on : .off
        // #1 Duotone: Duotone… on 8/16-bit Grayscale and on Duotone (checked, still enabled: it
        // edits the inks). RGB, CMYK and Lab Color convert a Duotone document through its inks,
        // Grayscale and Bitmap through the inks' luminance, and Indexed Color and Bitmap by their
        // own source rules.
        if target == .duotone {
            guard DuotoneDocument.sourceRefusal(doc) == nil else { return false }
            return !document.isLiveEditing && !BackgroundWork.occupied
        }
        if target == .cmyk, doc.isFloat { return false }
        if target == .lab, doc.isFloat { return false }
        // #1 Bitmap phase 1a: a Bitmap document converts to Grayscale only (Bitmap itself is
        // checked); Bitmap from 8-bit Grayscale only.
        if doc.isBitmap, target != .gray { return false }
        if target == .bitmap, !current, BitmapDocument.sourceRefusal(doc) != nil { return false }
        // #1 Indexed phase 1a: Indexed Color from 8-bit RGB and Grayscale only.
        if target == .indexed, !current, IndexedDocument.sourceRefusal(doc) != nil { return false }
        return !current && !document.isLiveEditing && !BackgroundWork.occupied
    }
}
