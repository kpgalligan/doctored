import AppKit

// Layer ▸ Stack Mode and Layer ▸ Reduce Noise from Stack…: one per-pixel statistic across
// the selected raster layers, as ONE new raster layer above the topmost selected layer, with the
// layers it read hidden. The statistic is the core's (`stack_stat.rs`, whose module doc is the
// rule: the union extent, alpha-weighted Mean, the other modes over the layers at least half
// covered, stored codes at 8 and 16 bits, linear values at float). Reduce Noise first registers
// and warps the layers through Auto-Align's own job halves (`AutoAlignWork.registrationPlan` /
// `alignedDocument`, AutoAlign.swift) and adds no registration of its own.

/// The statistic; the raw value is the core request's and the MCP tools'
/// spelling. Declared in alphabetical order, so `allCases`
/// is the submenu's order.
enum StackMode: String, CaseIterable {
    case entropy, kurtosis, majority, maximum, mean, median, minimum, range, skewness
    case standardDeviation = "standard_deviation"
    case summation, variance

    /// The menu item's title and a Stack Mode layer's name.
    var title: String {
        switch self {
        case .entropy: return "Entropy"
        case .kurtosis: return "Kurtosis"
        case .majority: return "Majority"
        case .maximum: return "Maximum"
        case .mean: return "Mean"
        case .median: return "Median"
        case .minimum: return "Minimum"
        case .range: return "Range"
        case .skewness: return "Skewness"
        case .standardDeviation: return "Standard Deviation"
        case .summation: return "Summation"
        case .variance: return "Variance"
        }
    }

    /// The two statistics that reduce noise: the others measure a stack.
    static let noiseModes: [StackMode] = [.mean, .median]
}

/// Eligibility, the core request and what a finished job reports. The job is
/// `StackModeWork`.
enum StackModes {
    static let title = "Stack Mode"
    static let noiseTitle = "Reduce Noise from Stack"
    static let noiseLayerName = "Noise Reduced"

    struct Outcome {
        /// The new layer's index.
        let layer: Int
        let name: String
        let mode: StackMode
        /// The layers the statistic read, ascending.
        let members: [Int]
        /// The members that were visible before and are hidden now.
        let hidden: [Int]
        /// What the alignment did to each layer; nil for a plain Stack Mode.
        let align: AutoAlign.Outcome?
        /// The largest charge the job admitted.
        let planBytes: UInt64
    }

    /// `layers` as a member set: at least two distinct entries, each a raster
    /// layer — not a group, not an adjustment layer. A refusal names the
    /// layer and the command asking (`title`: `StackModes.title` or
    /// `noiseTitle`). A text or shape layer is read like any other: its stored
    /// pixels are what the statistic takes, and the layer is only hidden.
    static func members(in doc: RasterDocument, layers: [Int], title: String) throws -> [Int] {
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
        }
        return indices
    }

    /// The core's `stack_mode` request.
    static func command(layers: [Int], above: Int, mode: StackMode, name: String) throws -> String {
        try PrecisionPixelCommand([
            "op": "stack_mode", "layers": layers, "above": above, "mode": mode.rawValue,
            "name": name,
        ]).request
    }

    /// Why Reduce Noise published nothing: fewer than two layers aligned.
    /// Names every layer left out and the ones that could have been used.
    static func tooFewMessage(usable: [String], excluded: [AutoAlign.Skipped]) -> String {
        let left = excluded.map { skipped -> String in
            // A core refusal arrives as a full sentence; the host's reasons do not.
            let reason = skipped.reason.hasSuffix(".")
                ? String(skipped.reason.dropLast()) : skipped.reason
            return "“\(skipped.name)”: \(reason)"
        }.joined(separator: "; ")
        let kept = usable.map { "“\($0)”" }.joined(separator: ", ")
        return "\(noiseTitle) needs at least two layers that align, and only \(kept) could be "
            + "used. Left out — \(left). Nothing changed."
    }
}

/// The ONE compound Stack Mode job: the alignment (Reduce Noise only), the
/// statistic and the display preparation under one `RasterJobControl`, so
/// Cancel/Esc stops it anywhere and a cancel publishes nothing. The capture
/// and staleness guards are `AutoAlignWork.wait`'s. The worker captures values
/// only.
struct StackModeWork {
    let source: RasterDocument
    /// The whole selection, ascending (with `align`: its reference and
    /// floating layers). The result lands above the topmost of them.
    let layers: [Int]
    let mode: StackMode
    let name: String
    private let alignment: AutoAlignWork?
    /// Each layer's name and visibility, read on main.
    private let facts: [Int: (name: String, visible: Bool)]
    /// Where the new layer lands, read on main: neither a warp nor hiding a
    /// layer renumbers anything.
    private let landing: Int

    /// Main thread: reads the layers' names and visibility from `source`.
    /// `align` makes this Reduce Noise: the layers are registered and warped
    /// first, and the statistic reads the reference and every layer that
    /// aligned.
    init(source: RasterDocument, layers: [Int], mode: StackMode, name: String,
         align: AutoAlign.Request?) throws {
        guard Thread.isMainThread else {
            throw DoctoredCoreError(message: "Capture the layers before stack work starts.")
        }
        let sorted = Array(Set(layers)).sorted()
        guard let anchor = sorted.last, sorted.count >= 2 else {
            throw DoctoredCoreError(message: "\(StackModes.title) needs at least two layers.")
        }
        var facts: [Int: (name: String, visible: Bool)] = [:]
        for index in sorted {
            guard let info = source.layerInfo(index) else {
                throw DoctoredCoreError(message: "Layer \(index) is out of range.")
            }
            facts[index] = (info.name, info.visible)
        }
        self.source = source
        self.layers = sorted
        self.mode = mode
        self.name = name
        self.facts = facts
        self.landing = LayerInsertPosition.above.landingIndex(in: source, anchor: anchor)
        self.alignment = try align.map { try AutoAlignWork(source: source, request: $0) }
    }

    /// Runs the job and answers the prepared edit with what it did. Throws
    /// `JobCancelled` on a cancel, the core's sentence for a refusal,
    /// `StackModes.tooFewMessage` when fewer than two layers aligned, and a
    /// staleness sentence when the document changed underneath; none of them
    /// publishes anything.
    ///
    /// Progress and admission, a plain Stack Mode: the statistic's plan is
    /// admitted on main and runs over 0…0.8, the display preparation over
    /// 0.8…1. Reduce Noise: the registration plan is admitted on main and the
    /// alignment runs over 0…0.5; the statistic is planned on the ALIGNED
    /// document, admitted on the worker and runs over 0.5…0.8; then the
    /// display.
    func wait(in document: ImageDocument, title: String) throws
        -> (prepared: PreparedDocumentEdit, outcome: StackModes.Outcome)
    {
        assert(Thread.isMainThread)
        guard document.doc === source, !document.isLiveEditing else {
            throw DoctoredCoreError(message: "The document changed before \(title) started.")
        }
        let revision = document.revision
        let state = document.captureEditingState()
        let source = self.source
        let layers = self.layers
        let mode = self.mode
        let name = self.name
        let alignment = self.alignment
        let facts = self.facts
        let landing = self.landing
        guard let anchor = layers.last else {
            throw DoctoredCoreError(message: "\(StackModes.title) needs at least two layers.")
        }
        let budget: (limit: UInt64, other: UInt64)? =
            source.isHighDepth ? nil : try source.legacyPrecisionWorkContext()
        // The first stage's plan, admitted here: the registration's for
        // Reduce Noise, else the statistic's own.
        let direct: String?
        let first: UInt64
        if let alignment {
            direct = nil
            first = try alignment.registrationPlan()
        } else {
            let command = try StackModes.command(layers: layers, above: anchor, mode: mode,
                                                 name: name)
            direct = command
            first = try PrecisionDocumentDescriptor.integer(
                source.stackModePlan(request: command), "scratch_bytes")
        }
        _ = try RasterResources.shared.admit(scratch: first)
        let control = try RasterJobControl()
        guard control.setScratchLimit(first) else {
            throw DoctoredCoreError(message: "Could not bound stack work.")
        }
        let result = try BackgroundWork.wait(
            title: title, control: control, revealAfter: DocumentJobs.panelDelay
        ) {
            try WorkerTask(scratchEstimate: first) {
                () -> (prepared: PreparedDocumentEdit, outcome: StackModes.Outcome) in
                try control.beginCompoundOperation()
                var input = source
                var members = layers
                var aligned: AutoAlign.Outcome?
                var command = direct
                var scratch = first
                var largest = first
                var from = 0.0
                if let alignment {
                    let (document, outcome) = try alignment.alignedDocument(
                        control: control, registration: first, budget: budget, share: 0...0.5)
                    // A layer no registration vouched for is never merged
                    // unaligned: only the reference and what aligned is read.
                    members = ([outcome.reference] + outcome.aligned.map(\.layer)
                        + outcome.unchanged.map(\.layer)).sorted()
                    guard members.count >= 2 else {
                        throw DoctoredCoreError(message: StackModes.tooFewMessage(
                            usable: members.map { facts[$0]?.name ?? "Layer \($0)" },
                            excluded: outcome.unregistered))
                    }
                    // `above` stays the topmost layer GIVEN, so the result is
                    // above a left-out layer that sits above the members.
                    let request = try StackModes.command(layers: members, above: anchor,
                                                         mode: mode, name: name)
                    scratch = try PrecisionDocumentDescriptor.integer(
                        document.stackModePlan(request: request), "scratch_bytes")
                    _ = try RasterResources.shared.admit(scratch: scratch)
                    largest = max(outcome.planBytes, scratch)
                    input = document
                    aligned = outcome
                    command = request
                    from = 0.5
                }
                guard let stackRequest = command else {
                    throw DoctoredCoreError(message: "The stack request could not be built.")
                }
                guard !control.isCancelled else { throw JobCancelled() }
                let limit = scratch
                let stackInput = input
                let stacked = try control.withChildOperation(from: from, to: 0.8) {
                    child -> RasterDocument? in
                    guard child.setScratchLimit(limit) else {
                        throw DoctoredCoreError(message: "Could not bound stack work.")
                    }
                    return try stackInput.editingStackMode(request: stackRequest, budget: budget,
                                                           control: child)
                }
                guard let stacked else {
                    throw DoctoredCoreError(message: "The stack produced no layer.")
                }
                let prepared = try control.withChildOperation(from: 0.8, to: 1) { child in
                    PreparedDocumentEdit(stacked, scratch: BackgroundWork.scratch, control: child)
                }
                let outcome = StackModes.Outcome(
                    layer: landing, name: name, mode: mode, members: members,
                    hidden: members.filter { facts[$0]?.visible == true }, align: aligned,
                    planBytes: largest)
                return (prepared: prepared, outcome: outcome)
            }
        }
        guard !control.isCancelled else { throw JobCancelled() }
        guard document.revision == revision, document.doc === source, !document.isLiveEditing,
              (!source.isHighDepth && source.retainedSourceIndices.isEmpty)
                || document.isCurrentEditingState(state) else {
            throw DoctoredCoreError(
                message: "The document changed while the stack was computed. Run \(title) again.")
        }
        return result
    }
}
