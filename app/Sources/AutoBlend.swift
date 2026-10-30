import AppKit

// Edit ▸ Auto-Blend Layers…, two methods. Stack Images (focus stacking): the core gives
// each selected raster layer a layer mask revealing the region where it is the sharpest frame and,
// with Seamless Tones and Colours, adds one merged layer blended through those masks. Panorama: the
// core gives each layer a mask revealing its side of a seam cut through each overlap and, with
// Seamless Tones and Colours, matches the frames' tones and writes a multi-band blend across the
// seams back into the frames, adding no layer unless the fill below adds its merged one
// (`dr_doc_auto_blend`, DoctoredCore+AutoBlend.swift).
// The host's part is eligibility, the optional "Align first" pass (Auto-Align's own registration
// and warps, AutoAlign.swift; Panorama's grows the canvas to the union, as Auto-Align does),
// Panorama's optional Content-Aware Fill Transparent Areas (AutoBlendFill.swift)
// and the one job that runs them all, `AutoBlendWork`.

/// Eligibility, the core request and the outcome of Auto-Blend Layers.
enum AutoBlend {
    static let title = "Auto-Blend Layers"
    /// The merged layer's name.
    static let mergedName = "Auto-Blend"
    /// The fill with Stack Images.
    static let fillNeedsPanorama =
        "Content-Aware Fill Transparent Areas applies only to the Panorama method."

    /// The sheet's Blend Method; the raw value is the core's `method`.
    enum Method: String, CaseIterable {
        case panorama
        case stackImages = "stack_images"

        var title: String {
            switch self {
            case .panorama: return "Panorama"
            case .stackImages: return "Stack Images"
            }
        }
    }

    struct Request {
        /// The layers to blend, ascending.
        let layers: [Int]
        let method: Method
        let seamless: Bool
        /// Auto-Align's request for the same layers when "Align first" is on.
        let align: AutoAlign.Request?
        /// Panorama's Content-Aware Fill Transparent Areas (`AutoBlendFill`).
        let fill: Bool
    }

    struct Winner {
        let layer: Int
        let name: String
        /// The share of the canvas the layer's mask reveals, 0…1.
        let share: Double
        /// Panorama with Seamless Tones: the layer's RGB gain; nil otherwise.
        let gain: [Double]?
    }

    struct Outcome {
        let method: Method
        let layers: [Winner]
        /// The merged layer's index in the result; nil without `seamless`.
        let mergedLayer: Int?
        /// What "Align first" did; nil when it was off.
        let aligned: AutoAlign.Outcome?
        /// What Content-Aware Fill Transparent Areas filled; nil when it was off.
        let fill: AutoBlendFill.Filled?
        /// The largest charge the job admitted: the registration plan, a
        /// warp's geometry plan, the blend's plan or the fill's plan.
        let planBytes: UInt64
    }

    /// Checks `layers`: at least two, each a raster layer (not a group, an
    /// adjustment layer, or a text, shape or fill layer) with no layer mask;
    /// a refusal names the layer. With `align`, Auto-Align's own rules apply
    /// too (`AutoAlign.request`: no linked layer, at most one position-locked
    /// layer, which is the reference), with its menu defaults; Panorama's
    /// alignment grows the canvas to the union of the aligned layers, as
    /// Auto-Align does. `fill` (Content-Aware Fill Transparent Areas) is Panorama's alone.
    static func request(in doc: RasterDocument, layers: [Int], method: Method, seamless: Bool,
                        align: Bool, fill: Bool) throws -> Request
    {
        guard !fill || method == .panorama else {
            throw DoctoredCoreError(message: fillNeedsPanorama)
        }
        let indices = Array(Set(layers)).sorted()
        guard indices.count >= 2 else {
            throw DoctoredCoreError(message: "\(title) needs at least two layers.")
        }
        for index in indices {
            guard let info = doc.layerInfo(index) else {
                throw DoctoredCoreError(message: "Layer \(index) is out of range.")
            }
            let name = "“\(info.name)”"
            guard !info.isGroup else {
                throw DoctoredCoreError(
                    message: "\(name) is a group. \(title) takes raster layers; select the "
                        + "layers inside it instead.")
            }
            guard !doc.layerIsAdjustment(index) else {
                throw DoctoredCoreError(
                    message: "\(name) is an adjustment layer. \(title) takes raster layers.")
            }
            guard doc.describedEntries(in: [index]).isEmpty else {
                throw DoctoredCoreError(
                    message: "\(name) is a text, shape or fill layer. Rasterize it before "
                        + "blending it.")
            }
            guard !doc.layerHasMask(index) else {
                throw DoctoredCoreError(
                    message: "\(name) already has a layer mask. Apply or remove it before "
                        + "\(title).")
            }
        }
        let alignment = align
            ? try AutoAlign.request(in: doc, layers: indices, reference: nil,
                                    registration: .auto, sampler: DR_FILTER_CATMULL_ROM,
                                    growsCanvas: method == .panorama)
            : nil
        return Request(layers: indices, method: method, seamless: seamless, align: alignment,
                       fill: fill)
    }

    /// The core's request JSON for `request`.
    static func command(_ request: Request) throws -> String {
        let fields: [String: Any] = [
            "layers": request.layers, "seamless": request.seamless,
            "method": request.method.rawValue, "name": mergedName,
        ]
        return String(decoding: try JSONSerialization.data(withJSONObject: fields), as: UTF8.self)
    }

    /// Why the whole blend is refused when "Align first" left a layer where
    /// it was: frames that are not registered blend into a double image.
    /// Each layer is named beside its own reason; layers left for the same
    /// reason are listed together.
    static func unalignedMessage(_ skipped: [AutoAlign.Skipped]) -> String {
        var groups: [(reason: String, names: [String])] = []
        for entry in skipped {
            let reason = entry.reason.hasSuffix(".")
                ? String(entry.reason.dropLast()) : entry.reason
            if let at = groups.firstIndex(where: { $0.reason == reason }) {
                groups[at].names.append("“\(entry.name)”")
            } else {
                groups.append((reason: reason, names: ["“\(entry.name)”"]))
            }
        }
        let listed = groups.isEmpty
            ? "The layers (\(AutoAlign.unregisteredReason))"
            : groups.map { "\($0.names.joined(separator: ", ")) (\($0.reason))" }
                .joined(separator: "; ")
        return "\(listed) could not be aligned, so nothing was blended. Align the "
            + "layers yourself, or turn off Align first."
    }

    /// The core's report as an `Outcome`, the layers named from `names`.
    static func outcome(report: [String: Any], names: [Int: String],
                        aligned: AutoAlign.Outcome?, fill: AutoBlendFill.Filled?,
                        planBytes: UInt64) throws -> Outcome {
        let canvas = try PrecisionDocumentDescriptor.integer(report, "canvas_pixels")
        guard canvas > 0, let rows = report["layers"] as? [[String: Any]] else {
            throw DoctoredCoreError(message: "\(title) returned an unreadable report.")
        }
        let winners = try rows.map { row -> Winner in
            let layer = Int(try PrecisionDocumentDescriptor.integer(row, "layer"))
            let won = try PrecisionDocumentDescriptor.integer(row, "won_pixels")
            let gain = (row["gain"] as? [NSNumber]).map { $0.map(\.doubleValue) }
            return Winner(layer: layer, name: names[layer] ?? "Layer \(layer)",
                          share: Double(won) / Double(canvas), gain: gain)
        }
        let merged = (report["merged_layer"] as? NSNumber).map(\.intValue)
        let method = (report["method"] as? String).flatMap(Method.init(rawValue:)) ?? .stackImages
        return Outcome(method: method, layers: winners, mergedLayer: merged, aligned: aligned,
                       fill: fill, planBytes: planBytes)
    }
}

/// The ONE compound Auto-Blend job: the optional alignment, the blend and the
/// display preparation under one `RasterJobControl`, so Cancel/Esc stops it
/// inside any stage and a cancel publishes nothing. The capture, the
/// staleness guards and the compound shape are `AutoAlignWork.wait`'s. The
/// worker captures values only.
struct AutoBlendWork {
    let source: RasterDocument
    let request: AutoBlend.Request
    /// Each layer's name, read on main.
    private let names: [Int: String]
    /// The alignment stage, captured on main; nil when "Align first" is off.
    private let align: AutoAlignWork?

    /// Main thread: reads the layers' names from `source`.
    init(source: RasterDocument, request: AutoBlend.Request) throws {
        guard Thread.isMainThread else {
            throw DoctoredCoreError(message: "Capture the layers before blending work starts.")
        }
        self.source = source
        self.request = request
        var names: [Int: String] = [:]
        for index in request.layers {
            guard let info = source.layerInfo(index) else {
                throw DoctoredCoreError(message: "Layer \(index) is out of range.")
            }
            names[index] = info.name
        }
        self.names = names
        align = try request.align.map { try AutoAlignWork(source: source, request: $0) }
    }

    /// Runs the job and answers the prepared edit with what each layer won.
    /// Throws `JobCancelled` on a cancel, the core's sentence for a refusal,
    /// `AutoBlend.unalignedMessage` when "Align first" left a layer
    /// unregistered, and a staleness sentence when the document changed
    /// underneath; none of them publishes anything.
    ///
    /// Progress: the alignment is 0…0.35 (skipped when off), the blend runs to
    /// 0.85 (0.6 with the fill, which runs 0.6…0.85) and the display
    /// preparation to 1. Admission is per stage: the registration plan and the
    /// blend's plan on main (so a refusal starts no job), each warp's geometry
    /// plan and the aligned document's blend plan on the worker just before it
    /// runs, the fill's coverage and Content-Aware Fill plans on the worker
    /// (`AutoBlendFill.run`), and `PreparedDocumentEdit` its own display. A
    /// fill refusal refuses the whole call (`AutoBlendFill.refusalMessage`).
    func wait(in document: ImageDocument, title: String) throws
        -> (prepared: PreparedDocumentEdit, outcome: AutoBlend.Outcome)
    {
        assert(Thread.isMainThread)
        guard document.doc === source, !document.isLiveEditing else {
            throw DoctoredCoreError(message: "The document changed before the blend started.")
        }
        let revision = document.revision
        let state = document.captureEditingState()
        let source = self.source
        let names = self.names
        let align = self.align
        let command = try AutoBlend.command(request)
        let budget: (limit: UInt64, other: UInt64)? =
            source.isHighDepth ? nil : try source.legacyPrecisionWorkContext()
        let fillBudget: (limit: UInt64, other: UInt64)? =
            request.fill ? try GenerativeFillRun.budget(source) : nil
        let top = request.layers.last
        let blendEnd = fillBudget == nil ? 0.85 : 0.6
        // The blend's plan is read from the source first: it answers every
        // core refusal before a job starts. An alignment keeps the frame
        // count; Panorama's grows the canvas, so the aligned document's own
        // plan is admitted on the worker before the blend runs.
        let planned = try PrecisionDocumentDescriptor.integer(
            source.autoBlendPlan(request: command), "scratch_bytes")
        _ = try RasterResources.shared.admit(scratch: planned)
        let registration = try align?.registrationPlan()
        if let registration { _ = try RasterResources.shared.admit(scratch: registration) }
        let initial = max(planned, registration ?? 0)
        let control = try RasterJobControl()
        guard control.setScratchLimit(initial) else {
            throw DoctoredCoreError(message: "Could not bound blending work.")
        }
        let result = try BackgroundWork.wait(
            title: title, control: control, revealAfter: DocumentJobs.panelDelay
        ) {
            try WorkerTask(scratchEstimate: initial) {
                () -> (prepared: PreparedDocumentEdit, outcome: AutoBlend.Outcome) in
                try control.beginCompoundOperation()
                var running = source
                var aligned: AutoAlign.Outcome?
                var largest = initial
                var start = 0.0
                if let align, let registration {
                    let stage = try align.alignedDocument(
                        control: control, registration: registration, budget: budget,
                        share: 0...0.35)
                    guard stage.outcome.unregistered.isEmpty else {
                        throw DoctoredCoreError(
                            message: AutoBlend.unalignedMessage(stage.outcome.unregistered))
                    }
                    running = stage.document
                    aligned = stage.outcome
                    largest = max(largest, stage.outcome.planBytes)
                    start = 0.35
                }
                guard !control.isCancelled else { throw JobCancelled() }
                let blended = try AutoBlendWork.blended(
                    running, command: command, budget: budget, control: control,
                    share: start...blendEnd)
                largest = max(largest, blended.planBytes)
                var finished = blended.document
                var filled: AutoBlendFill.Filled?
                if let fillBudget, let top {
                    let merged = (blended.report["merged_layer"] as? NSNumber).map(\.intValue)
                    let blendedDocument = finished
                    let stage = try control.withChildOperation(from: 0.6, to: 0.85) { child in
                        try AutoBlendFill.run(blendedDocument, top: top, mergedLayer: merged,
                                              budget: fillBudget, control: child)
                    }
                    finished = stage.document
                    filled = stage.filled
                    largest = max(largest, stage.planBytes)
                }
                let outcome = try AutoBlend.outcome(
                    report: blended.report, names: names, aligned: aligned, fill: filled,
                    planBytes: largest)
                let published = finished
                let prepared = try control.withChildOperation(from: 0.85, to: 1) { child in
                    PreparedDocumentEdit(published, scratch: BackgroundWork.scratch, control: child)
                }
                return (prepared: prepared, outcome: outcome)
            }
        }
        guard !control.isCancelled else { throw JobCancelled() }
        guard document.revision == revision, document.doc === source, !document.isLiveEditing,
              (!source.isHighDepth && source.retainedSourceIndices.isEmpty)
                || document.isCurrentEditingState(state) else {
            throw DoctoredCoreError(
                message: "The document changed while the blend ran. Run \(title) again.")
        }
        return result
    }

    /// The blend stage, for a caller that owns the compound `control` (`wait`
    /// above; Merge Panorama's one job, `MergePanoramaWork`, MergePanorama.swift): admits
    /// `running`'s own blend plan for `command`, then runs the core's blend in
    /// a child operation over `share` of the control's progress, its scratch
    /// limit that plan. Answers the blended document, the core's report and
    /// the plan it admitted. Runs on the worker.
    static func blended(_ running: RasterDocument, command: String,
                        budget: (limit: UInt64, other: UInt64)?, control: RasterJobControl,
                        share: ClosedRange<Double>) throws
        -> (document: RasterDocument, report: [String: Any], planBytes: UInt64)
    {
        let scratch = try PrecisionDocumentDescriptor.integer(
            running.autoBlendPlan(request: command), "scratch_bytes")
        _ = try RasterResources.shared.admit(scratch: scratch)
        let input = running
        let blended = try control.withChildOperation(
            from: share.lowerBound, to: share.upperBound
        ) { child in
            guard child.setScratchLimit(scratch) else {
                throw DoctoredCoreError(message: "Could not bound blending work.")
            }
            return try input.autoBlending(request: command, budget: budget, control: child)
        }
        return (blended.document, blended.report, scratch)
    }
}
