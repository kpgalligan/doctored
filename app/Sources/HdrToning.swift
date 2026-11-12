import AppKit

// Image ▸ Adjustments ▸ HDR Toning…: the settings, their MCP parse, the flatten
// prompt and the job, `HdrToningRun` — `CmykModeConversion`'s shape (captured revision and
// editing state, the core's plan, one `PrecisionCapturedWork` job, one undo step published
// through Convert Precision's path). The sheet is `HdrToningSheetController`, the menu action
// `EditorViewController+HdrToning.swift`, the MCP twin `hdr_toning` (`AgentServer+HdrToning.swift`).
//
// HDR Toning FLATTENS and tone maps the composite at native depth in the
// core (`doc_hdr_toning`): a 32-bit document becomes 16 bits (or 8, narrowed with no dither), an
// 8- or 16-bit document keeps its depth (an 8-bit one is promoted first and narrowed after).
// Local Adaptation (phase 4b, the default method) carries its values and Toning Curve in
// `HdrLocalAdaptation`; its histogram is `HdrToningRun.histogram`. Its presets (phase 4c) are
// `HdrToningPreset`: the built-ins (Default and the looks) and the user's own; `hdr_toning`'s
// `preset` starts from one and explicit keys override it.

/// The four methods, in the popup's order. The raw value is the core request's name.
enum HdrToningMethod: String, CaseIterable {
    case localAdaptation = "local_adaptation"
    case equalizeHistogram = "equalize_histogram"
    case exposureGamma = "exposure_gamma"
    case highlightCompression = "highlight_compression"

    var title: String {
        switch self {
        case .localAdaptation: return "Local Adaptation"
        case .equalizeHistogram: return "Equalize Histogram"
        case .exposureGamma: return "Exposure and Gamma"
        case .highlightCompression: return "Highlight Compression"
        }
    }
}

/// One HDR Toning request: the method, Exposure and Gamma's two values, Local Adaptation's values
/// and the output depth.
struct HdrToningSettings: Equatable {
    var method: HdrToningMethod
    /// Exposure and Gamma's own two values; Local Adaptation's are `local.exposure`/`local.gamma`.
    var exposure: Double
    var gamma: Double
    var local = HdrLocalAdaptation()
    /// 8 or 16. A 32-bit source chooses; an 8- or 16-bit source keeps its own.
    var bitDepth: Int

    /// The dialog's opening values for `doc`: Local Adaptation at its defaults (Exposure
    /// and Gamma at (0, 1)), 16 bits for a 32-bit source and the source's own depth otherwise.
    static func defaults(for doc: RasterDocument) -> Self {
        Self(method: .localAdaptation, exposure: 0, gamma: 1, local: HdrLocalAdaptation(),
             bitDepth: nativeDepth(of: doc) ?? 16)
    }

    /// The depth an 8- or 16-bit source keeps; nil for a 32-bit source, which chooses. A Grayscale
    /// document keeps its declared 8 or 16 bits (#1 phase 2e); a 32-bit one chooses 8 or 16
    /// (phase 5a).
    static func nativeDepth(of doc: RasterDocument) -> Int? {
        if doc.isGray, let depth = doc.grayDepth, depth != 32 { return depth }
        return doc.isFloat ? nil : doc.isHighDepth ? 16 : 8
    }

    /// The core's request (`doc_hdr_toning`): exposure and gamma for Exposure and Gamma, every
    /// Local Adaptation value for Local Adaptation, nothing else for the other two.
    func coreRequest() throws -> String {
        var body: [String: Any] = ["method": method.rawValue]
        if method == .exposureGamma {
            body["exposure"] = exposure
            body["gamma"] = gamma
        } else if method == .localAdaptation {
            body.merge(local.fields) { _, new in new }
        }
        let data = try JSONSerialization.data(withJSONObject: body, options: [.sortedKeys])
        return String(decoding: data, as: UTF8.self)
    }

    /// `hdr_toning`'s arguments for this request, every value explicit.
    var stepArguments: [String: Any] {
        var arguments: [String: Any] = ["method": method.rawValue, "bit_depth": bitDepth]
        if method == .exposureGamma {
            arguments["exposure"] = exposure
            arguments["gamma"] = gamma
        } else if method == .localAdaptation {
            arguments.merge(local.fields) { _, new in new }
        }
        return arguments
    }

    /// `hdr_toning`'s arguments: `method` required unless `preset` names a preset
    /// (`HdrToningPreset.named`), whose values start from the source's defaults and which explicit
    /// keys override; then `values(_:over:)`, then `bit_depth` 8 or 16 for a 32-bit source
    /// (default 16) and the source's own depth otherwise. Ranges are the core's; it refuses a
    /// value outside them.
    static func parse(_ a: [String: Any], source: RasterDocument) throws -> Self {
        let base: Self
        if let value = a["preset"] {
            guard let name = value as? String else {
                throw DoctoredCoreError(message: "preset must be a preset name.")
            }
            base = try HdrToningPreset.named(name).applied(to: defaults(for: source))
        } else {
            guard let name = a["method"] as? String, !name.isEmpty else {
                throw DoctoredCoreError(message: "Missing required argument: method")
            }
            base = defaults(for: source)
        }
        var settings = try values(a, over: base)
        if let value = a["bit_depth"] {
            let requested = try number(value, "bit_depth")
            guard requested.rounded() == requested, abs(requested) <= 64 else {
                throw DoctoredCoreError(message: "bit_depth must be a whole number.")
            }
            let depth = Int(requested)
            if let native = nativeDepth(of: source) {
                guard depth == native else {
                    let article = native == 8 ? "an" : "a"
                    throw DoctoredCoreError(message: "bit_depth must be \(native): HDR Toning keeps "
                        + "\(article) \(native)-bit document at \(native) bits (only a 32-bit "
                        + "document converts).")
                }
            } else {
                guard depth == 8 || depth == 16 else {
                    throw DoctoredCoreError(message: "bit_depth must be 8 or 16.")
                }
            }
            settings.bitDepth = depth
        }
        return settings
    }

    /// The method and its values from `a` over `base`: the method is `a["method"]`, else
    /// `base.method`; `exposure` and `gamma` for exposure_gamma and local_adaptation only, Local
    /// Adaptation's own keys (`HdrLocalAdaptation.ownKeys`) for it only, each checked against
    /// that final method; an absent key keeps `base`'s value. `bit_depth` is not read here.
    static func values(_ a: [String: Any], over base: Self) throws -> Self {
        var settings = base
        if let value = a["method"] {
            guard let name = value as? String, !name.isEmpty else {
                throw DoctoredCoreError(message: "Missing required argument: method")
            }
            guard let method = HdrToningMethod(rawValue: name) else {
                throw DoctoredCoreError(message: "method must be local_adaptation, equalize_histogram, "
                    + "exposure_gamma or highlight_compression.")
            }
            settings.method = method
        }
        let method = settings.method
        if method != .exposureGamma, method != .localAdaptation {
            for key in ["exposure", "gamma"] where a[key] != nil {
                throw DoctoredCoreError(message: "\(key) applies to methods exposure_gamma and "
                    + "local_adaptation only.")
            }
        }
        if method != .localAdaptation {
            for key in HdrLocalAdaptation.ownKeys where a[key] != nil {
                throw DoctoredCoreError(message: "\(key) applies to method local_adaptation only.")
            }
        }
        if method == .localAdaptation {
            try settings.local.parse(a)
        } else {
            if let value = a["exposure"] {
                settings.exposure = try number(value, "exposure")
            }
            if let value = a["gamma"] {
                settings.gamma = try number(value, "gamma")
            }
        }
        return settings
    }

    /// A finite number, or its string spelling (the argument helpers' rule).
    private static func number(_ value: Any, _ key: String) throws -> Double {
        let parsed = (value as? NSNumber)?.doubleValue ?? (value as? String).flatMap(Double.init)
        guard let parsed, parsed.isFinite else {
            throw DoctoredCoreError(message: "\(key) must be a number.")
        }
        return parsed
    }
}

enum HdrToning {
    static let title = "HDR Toning"
    /// The core's request cap (`doc_hdr_toning::MAX_REQUEST_BYTES`).
    static let requestMaximumBytes = 4096

    /// Whether toning `doc` discards structure, so the user is asked first: more than one layer,
    /// or one layer that is a group, an adjustment, a described (text, shape, fill) layer, or
    /// carries a mask or a style.
    static func needsFlatten(_ doc: RasterDocument) -> Bool {
        if doc.layerCount > 1 { return true }
        guard doc.layerCount == 1 else { return false }
        return doc.layerIsGroup(0) || doc.layerIsAdjustment(0) || doc.hasArtworkDescription(0)
            || LayerDescription.claimedKind(of: doc.layerMeta(0)) != nil
            || doc.layerHasMask(0) || doc.layerHasStyle(0)
    }

    /// The prompt before the dialog opens. True to go on.
    static func confirmFlatten() -> Bool {
        let alert = NSAlert()
        alert.messageText = "Using HDR Toning will flatten the document. Proceed?"
        alert.addButton(withTitle: "Yes")
        alert.addButton(withTitle: "No")
        return alert.runModal() == .alertFirstButtonReturn
    }
}

/// One captured HDR Toning: the source revision, its settings and the core's plan, made before
/// anything is computed.
struct HdrToningRun {
    let source: RasterDocument
    let revision: UInt64
    let editingState: EditingState
    let settings: HdrToningSettings
    let request: String
    let scratch: UInt64

    init(_ document: ImageDocument, settings: HdrToningSettings) throws {
        guard let source = document.doc, !document.isLiveEditing, !BackgroundWork.occupied else {
            throw DoctoredCoreError(message: "Finish the current operation before HDR Toning.")
        }
        if source.isCmyk { throw DoctoredCoreError(message: CmykDocument.refusal(tool: HdrToning.title)) }
        guard let state = document.captureEditingState() else {
            throw DoctoredCoreError(message: "Could not capture the HDR Toning state.")
        }
        self.source = source
        revision = document.revision
        editingState = state
        self.settings = settings
        request = try settings.coreRequest()
        let plan: [String: Any]
        if let typed = source.precision {
            plan = try typed.hdrToningPlan(request, bitDepth: settings.bitDepth)
        } else {
            plan = try source.hdrToningPlan(request)
        }
        scratch = try PrecisionDocumentDescriptor.integer(plan, "working_memory_plan_bytes")
    }

    /// The toned document, computed under `control` (a compound operation): a typed source is
    /// toned (and narrowed to 8 bits when asked); an 8-bit source is promoted flattened, toned and
    /// narrowed back. Runs off main: the job's worker, or the sheet's preview lease.
    static func toned(_ source: RasterDocument, request: String, bitDepth: Int,
                      control: RasterJobControl) throws -> RasterDocument {
        try control.beginCompoundOperation()
        let typed: RasterPrecisionDocument
        let start: Double
        if let precision = source.precision {
            typed = precision
            start = 0
        } else {
            typed = try control.withChildOperation(from: 0, to: 0.3) { child in
                try source.promotedFlattened(control: child)
            }
            start = 0.3
        }
        // A Grayscale document is toned at its own depth by the core, never narrowed (#1 2e); a
        // 32-bit one is toned to 16 bits and, at 8, narrowed by the Gray depth edit (phase 5a),
        // whose plan the core's 8-bit plan holds.
        let narrow = bitDepth == 8 && !source.isGray
        let grayNarrow = bitDepth == 8 && source.isGray && source.isFloat
        let context = try PrecisionWorkContext(source: typed)
        let toned = try control.withChildOperation(from: start, to: narrow || grayNarrow ? 0.85 : 1) { child in
            try typed.hdrToned(request, workingLimit: context.limit,
                               otherRetained: context.otherRetained, control: child)
        }
        if grayNarrow {
            let grayContext = try PrecisionWorkContext(source: toned)
            let request = GrayProfileEdit.request(.depth(8), space: nil, options: CmykConversionOptions())
            return try control.withChildOperation(from: 0.85, to: 1) { child in
                let eight = try toned.grayProfileEdit(profile: nil, request: request,
                                                      workingLimit: grayContext.limit,
                                                      otherRetained: grayContext.otherRetained, control: child)
                return try RasterDocument(precision: eight ?? toned)
            }
        }
        guard narrow else { return try RasterDocument(precision: toned) }
        let narrowContext = try PrecisionWorkContext(source: toned)
        return try control.withChildOperation(from: 0.85, to: 1) { child in
            try toned.downconverted(dither: 0, workingLimit: narrowContext.limit,
                                    otherRetained: narrowContext.otherRetained, control: child)
        }
    }

    func compute() throws -> PreparedDocumentEdit {
        let source = source, request = request, bitDepth = settings.bitDepth
        let result: RasterDocument = try PrecisionCapturedWork.run(scratchEstimate: scratch,
                                                                   title: HdrToning.title) { control in
            try Self.toned(source, request: request, bitDepth: bitDepth, control: control)
        }
        let task = try WorkerTask { PreparedDocumentEdit(result) }
        let prepared = try BackgroundWork.wait(task, title: "Preparing HDR Toning Preview")
        guard prepared.preview != nil, prepared.error == nil else {
            throw DoctoredCoreError(message: prepared.error ?? "Could not prepare the HDR Toning preview.")
        }
        return prepared
    }

    /// ONE undo step, "HDR Toning", through Convert Precision's publication; layer 0 selected.
    func publish(_ prepared: PreparedDocumentEdit, to document: ImageDocument, record: [ActionStep]) throws {
        guard document.doc === source, document.revision == revision, !document.isLiveEditing,
              document.isCurrentEditingState(editingState) else {
            throw DoctoredCoreError(message: "The document changed while HDR Toning was prepared. "
                + "No toning was applied.")
        }
        document.preparedEdit = prepared
        defer { document.preparedEdit = nil }
        try document.preflightPublication(prepared.document, record: record,
                                          intentionalDepthConversion: true)
        withUndoGroup(document.undoManager) {
            document.applyEdit(HdrToning.title, record: record, intentionalDepthConversion: true) { _ in
                prepared.document
            }
        }
        if document.doc === prepared.document, prepared.document.isValidIndex(0) {
            document.selectLayer(0)
        }
    }

    /// The Toning Curve's histogram of `source`'s composite, computed under `control` (a compound
    /// operation; the sheet's histogram lease): a typed source directly, an 8-bit source promoted
    /// flattened first. The 256 bins over the log range fill all four channels, so the curve
    /// editor's luminosity plot shows them.
    static func histogram(_ source: RasterDocument, control: RasterJobControl) throws -> HistogramBins {
        try control.beginCompoundOperation()
        let typed: RasterPrecisionDocument
        let start: Double
        if let precision = source.precision {
            typed = precision
            start = 0
        } else {
            typed = try control.withChildOperation(from: 0, to: 0.3) { child in
                try source.promotedFlattened(control: child)
            }
            start = 0.3
        }
        let context = try PrecisionWorkContext(source: typed)
        let reply = try control.withChildOperation(from: start, to: 1) { child in
            try typed.hdrToningHistogram(workingLimit: context.limit,
                                         otherRetained: context.otherRetained, control: child)
        }
        guard let bins = (reply["bins"] as? [NSNumber])?.map(\.doubleValue), bins.count == 256,
              let histogram = HistogramBins(values: bins + bins + bins + bins,
                                            total: bins.reduce(0, +), sampled: false) else {
            throw DoctoredCoreError(message: "The HDR Toning histogram could not be read.")
        }
        return histogram
    }

    /// The recorded step (`hdr_toning`'s arguments, every value explicit).
    func step() -> ActionStep {
        ActionStep(tool: "hdr_toning", arguments: settings.stepArguments, note: HdrToning.title)
    }
}
