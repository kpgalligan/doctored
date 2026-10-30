import AppKit

// Planar native RGB/scalar transfers retain alpha/other planes and map canvas results into raw
// storage.

/// Canvas-plane arithmetic uses the same core blend and luminosity formulas as
/// the byte tools. This adapter owns exact native inputs and selected writeback.
final class NativePlaneReader {
    let doc: RasterDocument
    let control: RasterJobControl
    let ownReservation: UInt64
    private var cached: [String: [UInt16]] = [:]
    private var leases: [PrecisionEditPayload] = []
    private var step = 0

    init(_ doc: RasterDocument, control: RasterJobControl, ownReservation: UInt64 = 0) {
        self.doc = doc; self.control = control; self.ownReservation = ownReservation
    }
    func stage<T>(_ body: (RasterJobControl) throws -> T) throws -> T {
        let start = min(0.94, Double(step) / 24)
        step += 1
        return try control.withChildOperation(from: start, to: min(0.98, Double(step) / 24), body)
    }
    func context(_ source: RasterPrecisionDocument) throws -> (UInt64, UInt64) {
        let context = try PrecisionWorkContext(source: source)
        guard context.otherRetained >= ownReservation else { throw DoctoredCoreError(message: "Native plane preview reservation is missing.") }
        return (context.limit, context.otherRetained - ownReservation)
    }
    /// One source or target plane, canvas-sized. `plate` names the CMYK plate an `.rgb` (CMYK
    /// composite) source is read as against a CMYK layer target (#1); nil reads its luminosity.
    func plane(layer: Int?, choice: PlaneChoice, component: RasterPlane? = nil,
               plate: CmykPlane? = nil) throws -> [UInt16] {
        let plane: RasterPlane
        if case .rgb = choice, let plate, doc.isCmyk {
            return try inkPlate(layer: layer, plate)
        }
        if case .ink(let plate) = choice { return try inkPlate(layer: layer, plate) }
        if case .channel(let index) = choice {
            let key = "channel:\(index)"
            if let found = cached[key] { return found }
            guard let source = doc.precision else { throw DoctoredCoreError(message: "Missing native channel owner.") }
            let request = try PrecisionPixelCommand.readPlane(target: "channel", index: index).request
            let budget = try context(source)
            let output = try stage { try source.readingPlane(request: request, workingLimit: budget.0, otherRetained: budget.1, control: $0) }
            leases.append(try PrecisionEditPayload.admitted(bytes: UInt64(output.count) * 2 + 512))
            cached[key] = output; return output
        }
        switch choice {
        // #1 Lab phase 6b-i: the Lab composite onto one plane reads Lightness (the stored red
        // codes), never `luminance`, which a Lab document refuses.
        case .rgb: plane = component ?? (doc.isLab ? .red : .luma)
        case .plane(let p): plane = p
        case .channel, .ink: throw DoctoredCoreError(message: "Invalid scalar source.")
        }
        let key = "\(layer.map(String.init) ?? "merged"):\(plane.agentName)"
        if let found = cached[key] { return found }
        let operation: [String: Any]
        switch plane {
        case .alpha: operation = ["kind": "alpha"]
        case .luma: operation = ["kind": "luminance"]
        case .red, .green, .blue: operation = ["kind": "color", "channel": plane.agentName]
        case .mask: throw DoctoredCoreError(message: "Choose a color or saved channel for channel arithmetic.")
        }
        let work = try NativeSelectionWork(source: doc.precision, width: doc.width, height: doc.height,
            operation: operation, scope: layer.map(HistogramSource.layer) ?? .composite)
        let output = try stage { try work.compute(control: $0, ownReservation: ownReservation) }
        leases.append(try PrecisionEditPayload.admitted(bytes: UInt64(output.count) * 2 + 512))
        cached[key] = output; return output
    }
    /// One CMYK plate as its stored `1 − ink`, canvas-sized (#1): a layer's own stored samples
    /// (`read_plane`, no ink outside its rect), or the composite's (`cmyk_select`'s `color`).
    private func inkPlate(layer: Int?, _ plate: CmykPlane) throws -> [UInt16] {
        let key = "\(layer.map(String.init) ?? "merged"):\(plate.key)"
        if let found = cached[key] { return found }
        guard let source = doc.precision else { throw DoctoredCoreError(message: "Missing native plate owner.") }
        let output: [UInt16]
        if let layer {
            let request = try PrecisionPixelCommand.readPlane(target: plate.key, index: layer, canvas: true).request
            let budget = try context(source)
            output = try stage { try source.readingPlane(request: request, workingLimit: budget.0, otherRetained: budget.1, control: $0) }
        } else {
            let work = try NativeSelectionWork(source: source, width: doc.width, height: doc.height,
                operation: ["kind": "color", "channel": plate.key], scope: .composite)
            output = try stage { try work.compute(control: $0, ownReservation: ownReservation) }
        }
        leases.append(try PrecisionEditPayload.admitted(bytes: UInt64(output.count) * 2 + 512))
        cached[key] = output; return output
    }
    func blend(base: [[UInt16]], source: [[UInt16]], mode: DrBlendMode, opacity: Double,
               invertBase: Bool = false, invertSource: Bool = false) throws -> [[UInt16]] {
        guard base.count == source.count, [1, 3].contains(base.count) else { throw DoctoredCoreError(message: "Channel blend needs one plane or an RGB triple.") }
        let n = doc.width * doc.height
        guard (base + source).allSatisfy({ $0.count == n }) else {
            throw DoctoredCoreError(message: "Channel blend inputs must match the canvas.")
        }
        let request = try NativeSelectionWork.encoding(["kind": "blend", "channels": base.count,
            "mode": Int(mode.rawValue), "opacity": opacity, "invert_base": invertBase, "invert_source": invertSource])
        // Admit before flattening, including the split output's overlap with
        // the returned contiguous buffer. Appending avoids two flatMap copies.
        let payload = try PrecisionEditPayload.admitted(bytes: UInt64(n) * UInt64(base.count) * 8 + 512)
        defer { withExtendedLifetime(payload) {} }
        var input: [UInt16] = []; input.reserveCapacity(n * base.count * 2)
        for plane in base { input.append(contentsOf: plane) }
        for plane in source { input.append(contentsOf: plane) }
        let budget = try NativeSelectionWork.context(nil)
        guard budget.other >= ownReservation else { throw DoctoredCoreError(message: "Native blend preview reservation is missing.") }
        let output = try stage { try RasterSelection.nativeTransform(input, width: doc.width, height: doc.height,
            request: request, workingLimit: budget.limit, otherRetained: budget.other - ownReservation, control: $0) }
        return (0..<base.count).map { Array(output[$0*n..<($0+1)*n]) }
    }
    func write(_ samples: [UInt16], to source: RasterPrecisionDocument, target: PaintTarget, layer: Int,
               selection: CanvasSelection?) throws -> RasterPrecisionDocument? {
        let addressed = try PrecisionScalarOperations.target(target, layer: layer)
        let n = doc.width * doc.height
        guard samples.count == n else { throw DoctoredCoreError(message: "Channel writeback must match the canvas.") }
        // Coverage materialization and selected concatenation are included
        // before either allocates; geometric coverage may need a byte adapter.
        let payload = try PrecisionEditPayload.admitted(bytes: UInt64(n) * (selection == nil ? 4 : 10) + 512)
        defer { withExtendedLifetime(payload) {} }
        let coverage = selection?.maskSamples16()
        let request = try PrecisionPixelCommand(["op": "scalar_edit", "target": addressed.0,
            "index": addressed.1, "selection": coverage != nil, "operation": ["kind": "replace_canvas"]]).request
        let budget = try context(source)
        return try stage { try source.editingPixels(request: request, samples: samples + (coverage ?? []),
            workingLimit: budget.0, otherRetained: budget.1, control: $0) }
    }
    /// A Grayscale layer's one plane written back to R, G and B (#1 phase 6b): the core's
    /// `scalar_edit` target `gray`, `replace_canvas` under the selection's coverage.
    func writeGray(_ samples: [UInt16], to source: RasterPrecisionDocument, layer: Int,
                   selection: CanvasSelection?) throws -> RasterPrecisionDocument? {
        let n = doc.width * doc.height
        guard samples.count == n else { throw DoctoredCoreError(message: "Channel writeback must match the canvas.") }
        let payload = try PrecisionEditPayload.admitted(bytes: UInt64(n) * (selection == nil ? 4 : 10) + 512)
        defer { withExtendedLifetime(payload) {} }
        let coverage = selection?.maskSamples16()
        let request = try PrecisionPixelCommand(["op": "scalar_edit", "target": "gray",
            "index": layer, "selection": coverage != nil, "operation": ["kind": "replace_canvas"]]).request
        let budget = try context(source)
        return try stage { try source.editingPixels(request: request, samples: samples + (coverage ?? []),
            workingLimit: budget.0, otherRetained: budget.1, control: $0) }
    }
}

enum NativeChannelMath {
    static func apply(_ capture: PrecisionEditorCapture, parameters p: ApplyImageParameters,
                      control: RasterJobControl, ownReservation: UInt64 = 0) throws -> RasterPrecisionDocument? {
        if let reason = OperationScope.refusalReason(.filter, target: p.target, doc: capture.facade, layerIndex: p.targetLayer) {
            throw DoctoredCoreError(message: reason)
        }
        guard p.target != .mask, !capture.quickMask else { throw DoctoredCoreError(message: "Apply Image requires RGB, an RGBA plane or a saved channel outside Quick Mask.") }
        if capture.source.isCmyk {
            return try applyCmyk(capture, parameters: p, control: control, ownReservation: ownReservation)
        }
        // #1 phase 6b: a Gray layer is one plane; an alpha channel target takes the RGB path.
        if capture.facade.isGray {
            if case .channel = p.target {} else {
                return try applyGray(capture, parameters: p, control: control, ownReservation: ownReservation)
            }
        }
        let reader = NativePlaneReader(capture.facade, control: control, ownReservation: ownReservation)
        let components: [RasterPlane?] = p.target == .layer ? [.red, .green, .blue] : [nil]
        var originals: [[UInt16]] = [], incoming: [[UInt16]] = []
        for component in components {
            let choice: PlaneChoice
            switch p.target {
            case .layer:
                guard let component else { throw DoctoredCoreError(message: "RGB channel arithmetic requires an explicit component.") }
                choice = .plane(component)
            case .plane(let plane): choice = .plane(plane)
            case .channel(let index): choice = .channel(index)
            case .mask: throw DoctoredCoreError(message: "Apply Image does not target masks.")
            case .ink: throw DoctoredCoreError(message: "An ink plate is a CMYK document's target.")
            }
            originals.append(try reader.plane(layer: p.targetLayer, choice: choice))
            incoming.append(try reader.plane(layer: p.source, choice: p.sourcePlane, component: component))
        }
        let blended = try reader.blend(base: originals, source: incoming, mode: p.blend, opacity: p.opacity, invertSource: p.invert)
        var current = capture.source
        var changed = false
        for (index, component) in components.enumerated() {
            if let next = try reader.write(blended[index], to: current, target: component.map(PaintTarget.plane) ?? p.target,
                layer: p.targetLayer, selection: capture.selection) { current = next; changed = true }
        }
        return changed ? current : nil
    }
    /// Apply Image on a CMYK document (#1): a layer target is its four plates, plate for plate
    /// (each one-plane blend, so the HSL modes refuse with the core's sentence), one plate or an
    /// alpha channel is one plane; every write is the plate's or channel's scalar `replace_canvas`.
    private static func applyCmyk(_ capture: PrecisionEditorCapture, parameters p: ApplyImageParameters,
                                  control: RasterJobControl, ownReservation: UInt64) throws -> RasterPrecisionDocument? {
        let reader = NativePlaneReader(capture.facade, control: control, ownReservation: ownReservation)
        let targets: [(PaintTarget, CmykPlane?)]
        switch p.target {
        case .layer: targets = CmykPlane.allCases.map { (.ink($0), $0) }
        case .ink(let plate): targets = [(.ink(plate), nil)]
        case .channel: targets = [(p.target, nil)]
        case .mask, .plane:
            throw DoctoredCoreError(message: "Apply Image on a CMYK document targets its layer, one ink plate or an alpha channel.")
        }
        var current = capture.source
        var changed = false
        for (target, plate) in targets {
            let original: [UInt16]
            switch target {
            case .ink(let targetPlate): original = try reader.plane(layer: p.targetLayer, choice: .ink(targetPlate))
            case .channel(let index): original = try reader.plane(layer: nil, choice: .channel(index))
            default: throw DoctoredCoreError(message: "Invalid CMYK Apply Image target.")
            }
            let incoming = try reader.plane(layer: p.source, choice: p.sourcePlane, plate: plate)
            let blended = try reader.blend(base: [original], source: [incoming], mode: p.blend,
                opacity: p.opacity, invertSource: p.invert)
            if let next = try reader.write(blended[0], to: current, target: target, layer: p.targetLayer,
                selection: capture.selection) { current = next; changed = true }
        }
        return changed ? current : nil
    }
    /// Apply Image onto a Grayscale layer (#1 phase 6b): the layer's luminance (its Gray, R = G =
    /// B) blended with the source plane as ONE plane, written to R, G and B by `writeGray`, so the
    /// layer keeps R = G = B. An alpha channel target takes `apply`'s RGB path.
    private static func applyGray(_ capture: PrecisionEditorCapture, parameters p: ApplyImageParameters,
                                  control: RasterJobControl, ownReservation: UInt64) throws -> RasterPrecisionDocument? {
        guard p.target == .layer else {
            throw DoctoredCoreError(message: "Apply Image on a Grayscale document targets its layer or an alpha channel.")
        }
        let reader = NativePlaneReader(capture.facade, control: control, ownReservation: ownReservation)
        let original = try reader.plane(layer: p.targetLayer, choice: .rgb)
        let incoming = try reader.plane(layer: p.source, choice: p.sourcePlane)
        let blended = try reader.blend(base: [original], source: [incoming], mode: p.blend,
            opacity: p.opacity, invertSource: p.invert)
        return try reader.writeGray(blended[0], to: capture.source, layer: p.targetLayer,
            selection: capture.selection)
    }
    static func calculated(_ doc: RasterDocument, parameters p: CalculationsParameters,
                           control: RasterJobControl, ownReservation: UInt64 = 0) throws -> [UInt16] {
        let reader = NativePlaneReader(doc, control: control, ownReservation: ownReservation)
        let base = try reader.plane(layer: p.source2.layer, choice: p.source2.plane)
        let source = try reader.plane(layer: p.source1.layer, choice: p.source1.plane)
        return try reader.blend(base: [base], source: [source], mode: p.blend, opacity: p.opacity,
            invertBase: p.source2.invert, invertSource: p.source1.invert)[0]
    }
    static func saveChannel(_ samples: [UInt16], capture: PrecisionEditorCapture, name: String,
                            control: RasterJobControl) throws -> RasterPrecisionDocument? {
        let context = try PrecisionWorkContext(source: capture.source)
        return try capture.source.editingPixels(request: PrecisionPixelCommand.setChannel(name: name).request,
            samples: samples, workingLimit: context.limit, otherRetained: context.otherRetained, control: control)
    }
}
