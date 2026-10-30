import Foundation

// Core-authored plans and the existing worker/token own admission and cancellation; captured source
// and parameter/profile payload leases participate in resource accounting. Typed archives write
// manifest v2 and retain v1 neutral reads. This file does not create another editor mutation or
// Undo system.

/// Internal typed description. Parameters have the existing adjustment schema;
/// validation and all numerical evaluation remain in the shared Rust kernels.
/// This value never passes through the byte adjustment-layer writer.
struct PrecisionAdjustment {
    static let evaluation = "precision_v1"
    let description: String

    init(op: AdjustmentLayerOp, params: [String: Any] = [:]) throws {
        var params = params
        if CalibratedAdjustment.supports(op.rawValue), params["evaluation"] == nil {
            params["evaluation"] = CalibratedAdjustment.document
        }
        let object: [String: Any] = ["type": "adjust", "evaluation": Self.evaluation,
                                     "op": op.rawValue, "params": params]
        guard JSONSerialization.isValidJSONObject(object) else {
            throw DoctoredCoreError(message: "High-depth adjustment parameters must be finite JSON values.")
        }
        let bytes = try JSONSerialization.data(withJSONObject: object, options: .sortedKeys)
        guard bytes.count <= 16 * 1024 * 1024, let text = String(data: bytes, encoding: .utf8) else {
            throw DoctoredCoreError(message: "High-depth adjustment description exceeds its size limit.")
        }
        description = text
    }
}

/// Immutable inputs and a core-authored memory plan for an internal edit. The
/// caller receives a new owner, with no editor mutation or second Undo system.
/// Profile conversion preserves parameter meanings, not the appearance of a
/// nonlinear live stack after its source RGB is converted to a new profile.
struct PrecisionCapturedEdit {
    enum Operation {
        /// A live adjustment inserted at a chosen stack position relative to
        /// `anchor` (12E-2). `.above` the anchor is what every caller before
        /// it meant, and the core owns the slot rule
        /// (`core/src/layer_insert.rs`).
        case add(name: String, adjustment: PrecisionAdjustment, opacity: Float,
                 anchor: Int, position: LayerInsertPosition)
        case set(index: Int, adjustment: PrecisionAdjustment)
        case apply(index: Int, adjustment: PrecisionAdjustment)
        case selected(request: String, coverage: PrecisionScalarBuffer?)
        case assignProfile(Data)
        case convertProfile(Data)
        /// Phase 11C: a conversion under an explicit intent, BPC and
        /// adjustment policy. `convertProfile` above IS this at the
        /// defaults, and stays as its own case so every existing caller and
        /// its bytes are untouched.
        case convertProfileWith(Data, ConversionOptions)
        /// Rasterize every bound retained source, then assign — ONE result.
        /// Planned as an Assign (`convert: false`): rasterizing drops each
        /// binding without rendering, and the core's one reservation is the
        /// rasterized document's assign plan, so nothing encodes into the
        /// destination and a failing matrix/TRC profile is admitted.
        case assignProfileRasterizing(Data)
    }

    let source: RasterPrecisionDocument
    let operation: Operation
    let scratchEstimate: UInt64
    /// What the result keeps beyond its source once compute returns (the core plan's
    /// `output_bytes`). Some only for `.selected`; nil for every other operation.
    let outputEstimate: UInt64?
    let retainedPayload: PrecisionEditPayload

    init(_ source: RasterPrecisionDocument, operation: Operation) throws {
        guard !BackgroundWork.occupied else {
            throw DoctoredCoreError(message: BackgroundWork.preparingBusyMessage)
        }
        self.source = source
        self.operation = operation
        let plan: [String: Any]
        let payloadBytes: UInt64
        var output: UInt64?
        switch operation {
        case .add(let name, let adjustment, let opacity, _, _):
            guard !name.utf8.contains(0), name.utf8.count <= 64 * 1024,
                  opacity.isFinite, (0...1).contains(opacity) else {
                throw DoctoredCoreError(message: "Invalid high-depth adjustment name or opacity.")
            }
            plan = try source.adjustmentPlan(index: nil, description: adjustment.description,
                                             destructive: false)
            payloadBytes = UInt64(adjustment.description.utf8.count + name.utf8.count) * 2 + 512
        case .set(let index, let adjustment):
            plan = try source.adjustmentPlan(index: index, description: adjustment.description,
                                             destructive: false)
            payloadBytes = UInt64(adjustment.description.utf8.count) * 2 + 512
        case .apply(let index, let adjustment):
            plan = try source.adjustmentPlan(index: index, description: adjustment.description,
                                             destructive: true)
            payloadBytes = UInt64(adjustment.description.utf8.count) * 2 + 512
        case .selected(let request, _):
            let budget = try PrecisionWorkContext(source: source)
            plan = try source.selectedEditPlan(request: request, workingLimit: budget.limit, otherRetained: budget.otherRetained)
            payloadBytes = UInt64(request.utf8.count) * 2 + 512
            output = try PrecisionDocumentDescriptor.integer(plan, "output_bytes")
        case .assignProfile(let data), .assignProfileRasterizing(let data):
            let budget = try PrecisionWorkContext(source: source)
            plan = try source.colorPlan(profile: data, convert: false, workingLimit: budget.limit,
                                        otherRetained: budget.otherRetained)
            payloadBytes = UInt64(data.count) + 512
        case .convertProfile(let data):
            let budget = try PrecisionWorkContext(source: source)
            plan = try source.colorPlan(profile: data, convert: true, workingLimit: budget.limit,
                                        otherRetained: budget.otherRetained)
            payloadBytes = UInt64(data.count) + 512
        case .convertProfileWith(let data, let options):
            let budget = try PrecisionWorkContext(source: source)
            plan = try source.conversionPlan(profile: data, options: options,
                                             workingLimit: budget.limit,
                                             otherRetained: budget.otherRetained)
            payloadBytes = UInt64(data.count) + 512
        }
        _ = try PrecisionDocumentDescriptor.integer(plan, "retained_bytes")
        scratchEstimate = try PrecisionDocumentDescriptor.integer(plan, "scratch_bytes")
        guard scratchEstimate > 0 else {
            throw DoctoredCoreError(message: "High-depth operation returned an empty memory plan.")
        }
        outputEstimate = output
        retainedPayload = try PrecisionEditPayload.admitted(bytes: payloadBytes)
    }

    /// A preview worker's reservation. Its compute and its display preparation run one
    /// after the other, so the reservation is max(scratchEstimate, outputEstimate + display)
    /// when outputEstimate is known, and scratchEstimate + display otherwise. Throws on overflow.
    func previewReservationBytes(display: UInt64) throws -> UInt64 {
        guard let outputEstimate else { return try PrecisionDocumentDescriptor.sum(scratchEstimate, display) }
        return max(scratchEstimate, try PrecisionDocumentDescriptor.sum(outputEstimate, display))
    }

    /// Core admission repeats immediately before computation, including other
    /// current owners and preview reservations. Cancellation discards the result.
    func compute(control: RasterJobControl) throws -> RasterPrecisionDocument? {
        let budget = try PrecisionWorkContext(source: source)
        switch operation {
        case .add(let name, let adjustment, let opacity, let anchor, let position):
            return try source.addingAdjustment(name: name, description: adjustment.description,
                opacity: opacity, anchor: anchor, position: position,
                workingLimit: budget.limit, otherRetained: budget.otherRetained,
                control: control)
        case .set(let index, let adjustment):
            return try source.settingAdjustment(index, description: adjustment.description,
                workingLimit: budget.limit, otherRetained: budget.otherRetained, control: control)
        case .apply(let index, let adjustment):
            return try source.applyingAdjustment(index, description: adjustment.description,
                workingLimit: budget.limit, otherRetained: budget.otherRetained, control: control)
        case .selected(let request, let coverage):
            return try source.selectedEdit(request: request, coverage: coverage?.samples ?? [], workingLimit: budget.limit,
                otherRetained: budget.otherRetained, control: control)
        case .assignProfile(let data):
            return try source.assigningProfile(data, workingLimit: budget.limit,
                                              otherRetained: budget.otherRetained, control: control)
        case .convertProfile(let data):
            return try source.convertingToProfile(data, workingLimit: budget.limit,
                                                 otherRetained: budget.otherRetained, control: control)
        case .convertProfileWith(let data, let options):
            guard let result = try source.convertingToProfile(
                data, options: options, workingLimit: budget.limit,
                otherRetained: budget.otherRetained, control: control) else { return nil }
            return RasterPrecisionDocument(owning: result.document)
        case .assignProfileRasterizing(let data):
            guard let handle = try source.assigningProfileRasterizing(
                data, workingLimit: budget.limit, otherRetained: budget.otherRetained,
                control: control) else { return nil }
            return RasterPrecisionDocument(owning: handle)
        }
    }

    func run(title: String = "Processing Captured High-Depth Source") throws -> RasterPrecisionDocument? {
        try PrecisionCapturedWork.run(scratchEstimate: scratchEstimate, title: title) {
            try compute(control: $0)
        }
    }
}

/// Shared ownership/admission for typed tone, geometry, scalar and paint jobs.
enum PrecisionCapturedWork {
    static func run<Value>(scratchEstimate: UInt64, title: String, revealAfter: TimeInterval = 0,
                           compute: @escaping (RasterJobControl) throws -> Value) throws -> Value {
        _ = try RasterResources.shared.admit(scratch: scratchEstimate)
        let control = try RasterJobControl()
        guard control.setScratchLimit(scratchEstimate) else {
            throw DoctoredCoreError(message: "Could not configure the high-depth work budget.")
        }
        let task = try WorkerTask(scratchEstimate: scratchEstimate) { try compute(control) }
        let result = try BackgroundWork.wait(task, title: title, control: control,
                                             revealAfter: revealAfter)
        guard !control.isCancelled else { throw JobCancelled() }
        return result
    }
}

/// A capture copy shares its lease; independently made captures are counted
/// conservatively even if Swift happens to share their String/Data storage.
/// Two bytes per UTF-8 byte covers native/bridged String storage plus bounded
/// object overhead. The core operation plan may also charge its borrowed input.
final class PrecisionEditPayload {
    let bytes: UInt64
    private init(bytes: UInt64) {
        self.bytes = bytes
        RasterResources.shared.register(self)
    }

    static func admitted(bytes: UInt64) throws -> PrecisionEditPayload {
        _ = try RasterResources.shared.admit(scratch: bytes)
        return PrecisionEditPayload(bytes: bytes)
    }
}
