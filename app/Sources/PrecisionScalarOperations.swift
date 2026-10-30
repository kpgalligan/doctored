import AppKit

// Channels > Modify Plane and `modify_scalar` share the same core request. Channel display
// derivatives and native scalar ink are explicit; sources never come from display.

/// Native scalar requests carry exact coverage separately from the raw plane.
/// The core owns extraction, neighborhoods and writeback; display reductions
/// below are explicitly disposable and never accepted as editing input.
struct PrecisionScalarWork {
    let source: RasterPrecisionDocument
    let command: PrecisionPixelCommand
    let samples: [UInt16]
    let scratchEstimate: UInt64
    let payload: PrecisionEditPayload

    init(source: RasterPrecisionDocument, command: PrecisionPixelCommand, samples: [UInt16] = []) throws {
        let context = try PrecisionWorkContext(source: source)
        let plan = try source.pixelEditPlan(request: command.request, workingLimit: context.limit,
            otherRetained: context.otherRetained)
        guard try PrecisionDocumentDescriptor.integer(plan, "input_samples") == UInt64(samples.count) else {
            throw DoctoredCoreError(message: "Native scalar input does not match its captured plan.")
        }
        self.source = source; self.command = command; self.samples = samples
        scratchEstimate = try PrecisionDocumentDescriptor.integer(plan, "scratch_bytes")
        payload = try PrecisionEditPayload.admitted(bytes: UInt64(samples.count) * 2 + UInt64(command.request.utf8.count) * 2 + 512)
    }

    func compute(control: RasterJobControl? = nil, ownReservation: UInt64 = 0) throws -> RasterPrecisionDocument? {
        let context = try PrecisionWorkContext(source: source)
        guard context.otherRetained >= ownReservation else { throw DoctoredCoreError(message: "Scalar reservation is missing.") }
        return try source.editingPixels(request: command.request, samples: samples,
            workingLimit: context.limit, otherRetained: context.otherRetained - ownReservation, control: control)
    }

    func run(title: String) throws -> RasterPrecisionDocument? {
        try PrecisionCapturedWork.run(scratchEstimate: scratchEstimate, title: title) { try compute(control: $0) }
    }
}

enum PrecisionScalarOperations {
    /// Authored continuous controls, not sampled image pixels. Coverage uses
    /// the existing Rec. 709 gray meaning with one native-code rounding.
    static func ink(_ ink: PrecisionInk) throws -> UInt16 {
        let rgba = ink.rgba
        return try RasterSelection.nativeInk(red: rgba[0], green: rgba[1], blue: rgba[2])
    }

    /// The sample Edit ▸ Fill… and Stroke… write into one colour plane of a
    /// 16-bit layer: the ink's 0.30 / 0.59 / 0.11 luma (measured
    /// against Photoshop), not `ink`'s Rec. 709 gray.
    static func planeInk(_ ink: PrecisionInk) throws -> UInt16 {
        let rgba = ink.rgba
        return try RasterSelection.planeInk16(red: rgba[0], green: rgba[1], blue: rgba[2])
    }
    static func target(_ target: PaintTarget, layer: Int) throws -> (String, Int) {
        switch target {
        case .mask: return ("mask", layer)
        case .channel(let index): return ("channel", index)
        case .plane(let plane):
            guard PaintTarget.paintablePlanes.contains(plane) else { break }
            return (plane.agentName, layer)
        case .layer: break
        // #1: one ink plate of a CMYK layer, as its stored `1 − ink` (`cmyk_plate_scalar`).
        case .ink(let plate): return (plate.key, layer)
        }
        throw DoctoredCoreError(message: "Choose a mask, saved channel or writable RGBA plane.")
    }

    static func edit(_ capture: PrecisionEditorCapture, target: PaintTarget? = nil, layer: Int? = nil,
                     operation: [String: Any], input: [UInt16] = [], selection: Bool = true) throws -> PrecisionScalarWork {
        let built = try command(capture, target: target, layer: layer, operation: operation, selection: selection)
        return try PrecisionScalarWork(source: capture.source, command: built.command, samples: input + (built.selection ?? []))
    }

    /// The scalar request and the selection coverage that follows its input,
    /// without planning: a typed stroke job appends its footprint on the worker.
    static func command(_ capture: PrecisionEditorCapture, target: PaintTarget? = nil, layer: Int? = nil,
                        operation: [String: Any], selection: Bool = true) throws -> (command: PrecisionPixelCommand, selection: [UInt16]?) {
        let target = target ?? capture.target, layer = layer ?? capture.layer
        guard !capture.quickMask else { throw DoctoredCoreError(message: OperationScope.quickMaskRefusal) }
        if let reason = OperationScope.refusalReason(.filter, target: target, doc: capture.facade, layerIndex: layer) {
            throw DoctoredCoreError(message: reason)
        }
        let addressed = try self.target(target, layer: layer)
        let coverage = selection ? capture.selection?.maskSamples16() : nil
        let command = try PrecisionPixelCommand(["op": "scalar_edit", "target": addressed.0,
            "index": addressed.1, "selection": coverage != nil, "operation": operation])
        return (command, coverage)
    }

    static func filter(_ capture: PrecisionEditorCapture, target: PaintTarget? = nil, layer: Int? = nil,
                       filter: String, params: [String: Any]) throws -> PrecisionScalarWork {
        // A recorded Lens Correction step names its profile (v1-lensfun), as in `SelectedAdjustment.edit`.
        let named = filter == "lens_correction"
            ? try capture.facade.lensResolvedParameters(params, layer: layer ?? capture.layer) : params
        let params = try FilterFrame.kernelParameters(filter, named, target: target ?? capture.target,
                                                      lab: capture.facade.isLab)
        let request = try SelectedAdjustment.request(layer: 0, filter: filter, params: params)
        guard let data = request.data(using: .utf8),
              let description = try JSONSerialization.jsonObject(with: data) as? [String: Any] else {
            throw DoctoredCoreError(message: "Invalid scalar filter description.")
        }
        return try edit(capture, target: target, layer: layer,
            operation: ["kind": "filter", "edit": description])
    }

    static func read(_ doc: RasterDocument, target: String, index: Int, canvas: Bool = false, ownReservation: UInt64 = 0) throws -> PrecisionScalarBuffer {
        guard let source = doc.precision else { throw DoctoredCoreError(message: "Missing native scalar source.") }
        if Thread.isMainThread {
            return try PrecisionPlaneRead(source, target: target, index: index, canvas: canvas).run()
        }
        let command = try PrecisionPixelCommand.readPlane(target: target, index: index, canvas: canvas)
        let context = try PrecisionWorkContext(source: source)
        let plan = try source.pixelEditPlan(request: command.request, workingLimit: context.limit,
            otherRetained: context.otherRetained)
        let width = try PrecisionDocumentDescriptor.integer(plan, "width")
        let height = try PrecisionDocumentDescriptor.integer(plan, "height")
        guard context.otherRetained >= ownReservation else { throw DoctoredCoreError(message: "Scalar display reservation is missing.") }
        let samples = try source.readingPlane(request: command.request, workingLimit: context.limit,
            otherRetained: context.otherRetained - ownReservation)
        return try PrecisionScalarBuffer(samples: samples, width: Int(width), height: Int(height))
    }

    static func channel(_ doc: RasterDocument, index: Int? = nil, name: String,
                        samples: [UInt16], width: Int, height: Int) throws -> RasterDocument? {
        guard let source = doc.precision, width == doc.width, height == doc.height else {
            throw DoctoredCoreError(message: "Saved native selection must match the canvas.")
        }
        let work = try PrecisionScalarWork(source: source, command: .setChannel(index: index, name: name), samples: samples)
        return try work.compute().map { try RasterDocument(precision: $0) }
    }

    static func combine(_ a: [UInt16], _ b: [UInt16], mode: SelectionCombineMode) throws -> [UInt16] {
        guard a.count == b.count else { throw DoctoredCoreError(message: "Scalar extents differ.") }
        return try NativeSelectionWork(width: a.count, height: 1,
            operation: ["kind": "combine", "mode": mode.agentName], input: a + b).run(title: "Combine Coverage")
    }

    static func canvasPlane(_ doc: RasterDocument, source: SelectionSource) throws -> PrecisionScalarBuffer {
        switch source {
        case .channel(let index): return try read(doc, target: "channel", index: index)
        case .layerMask(let index): return try read(doc, target: "mask", index: index, canvas: true)
        case .layerAlpha(let index): return try sourcePlane(doc, plane: .alpha, scope: .layer(index))
        case .compositePlane(let plane):
            // #1 Lab phase 4b-i: a Lab document's luminosity is its Lightness, the composite's
            // stored red codes (`lab_document::LUMINANCE_REFUSAL`), never a working-Gray re-encode.
            if doc.isLab, plane == .luma {
                return try sourcePlane(doc, plane: .red, scope: .composite)
            }
            // Load Selection's luminosity takes Color Settings' Gray working space at 16 bits on
            // RGB (`core/src/gray_working.rs`); CMYK keeps its LP leg and float the Y row. A Gray
            // document's luma of R = G = B is its own code (#1 phase 4): no working-Gray re-encode.
            let gray = plane == .luma && !doc.isCmyk && !doc.isFloat && !doc.isGray
                ? ColorSettings.workingGray : nil
            return try sourcePlane(doc, plane: plane, scope: .composite, gray: gray)
        case .inkPlate(let plate): return try inkPlane(doc, plate: plate)
        }
    }

    /// One CMYK ink plate of the composite as its stored `1 − ink` (#1: Load Selection from a
    /// plate, `cmyk_select`'s `color` kind with that plate).
    static func inkPlane(_ doc: RasterDocument, plate: CmykPlane) throws -> PrecisionScalarBuffer {
        let work = try NativeSelectionWork(source: doc.precision, width: doc.width, height: doc.height,
            operation: ["kind": "color", "channel": plate.key], scope: .composite)
        let samples = try Thread.isMainThread ? work.run(title: "Read Native Plane")
            : work.compute(control: RasterJobControl(), ownReservation: 0)
        return try PrecisionScalarBuffer(samples: samples, width: doc.width, height: doc.height)
    }

    /// `gray` (luma only) names the Gray working space the luminosity is taken in; nil is Rec. 709.
    static func sourcePlane(_ doc: RasterDocument, plane: RasterPlane,
                            scope: HistogramSource, ownReservation: UInt64 = 0,
                            gray: GrayWorkingSpace? = nil) throws -> PrecisionScalarBuffer {
        let operation: [String: Any]
        switch plane {
        case .alpha: operation = ["kind": "alpha"]
        case .luma:
            operation = gray.map { ["kind": "luminance", "gray": $0.rawValue] } ?? ["kind": "luminance"]
        case .red, .green, .blue: operation = ["kind": "color", "channel": plane.agentName]
        case .mask: throw DoctoredCoreError(message: "A mask requires an explicit source layer.")
        }
        let work = try NativeSelectionWork(source: doc.precision, width: doc.width, height: doc.height,
            operation: operation, scope: scope)
        let samples = try Thread.isMainThread ? work.run(title: "Read Native Plane") : work.compute(control: RasterJobControl(), ownReservation: ownReservation)
        return try PrecisionScalarBuffer(samples: samples, width: doc.width, height: doc.height)
    }

    /// A declared 8-bit grayscale display derivative, never plane storage.
    static func display(_ plane: PrecisionScalarBuffer, maxSide: Int = 0) -> RasterImage? {
        guard let lease = try? PrecisionEditPayload.admitted(bytes: UInt64(plane.samples.count) * 12 + 512) else { return nil }
        defer { withExtendedLifetime(lease) {} }
        let bytes = plane.samples.flatMap { value -> [UInt8] in
            let gray = UInt8((UInt32(value) + 128) / 257)
            return [gray, gray, gray, 255]
        }
        guard let image = RasterImage.from(rgba: bytes, width: plane.width, height: plane.height) else { return nil }
        return RasterDocument.fitted(image, maxSide: maxSide)
    }
}
