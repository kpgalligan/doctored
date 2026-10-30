import AppKit

// Edit ▸ Sky Replacement… (M2a): the sky model (Select ▸ Sky's,
// `SkySelection`) finds the sky once, and the core (`sky_harmonize.rs`, `dr_doc_sky_replace`,
// DoctoredCore+Sky.swift) refines it with Shift and Fade Edge, adjusts and places the chosen sky,
// harmonizes the foreground toward it and writes one of two outputs at 8 and 16 bits
// (the command is disabled at 32 bits and in CMYK, take 1): "New Layers" (M2b, the
// default), an editable Sky Replacement Group (two Curves layers and the masked Sky,
// `sky_group.rs`) above the source, or "Duplicate Layer", ONE new flattened layer above it. The sheet (`SkyReplacementSheet.swift`), `sky_replacement` over MCP
// (`AgentServer+SkyReplacement.swift`) and a replayed Action all go through `SkyReplacementRun`.
// Actions record the settings and the sky's library id or user name, never the mask.

enum SkyLightingMode: String {
    case multiply, screen
}

/// Where the result goes (Output To): the editable Sky Replacement Group ("New Layers", the
/// default) or one flattened layer ("Duplicate Layer").
enum SkyReplacementOutput: String, CaseIterable {
    case newLayers = "new_layers"
    case duplicateLayer = "duplicate_layer"

    /// The Output To popup's item.
    var title: String {
        switch self {
        case .newLayers: return "New Layers"
        case .duplicateLayer: return "Duplicate Layer"
        }
    }

    /// The core's `dr_sky_replace_scratch_bytes` output code.
    var code: UInt32 {
        switch self {
        case .newLayers: return 1
        case .duplicateLayer: return 0
        }
    }
}

/// The sheet's controls, in the core's units; the keys are the MCP names.
struct SkyReplacementSettings: Equatable {
    var shiftEdge = 0.0
    var fadeEdge = 3.0
    var brightness = 0.0
    var temperature = 0.0
    var scale = 100.0
    var lighting = 50.0
    var colorAdjustment = 50.0
    var edgeLighting = 0.0
    var flip = false
    /// The sky's offset in canvas fractions, each −1…1.
    var offset = CGPoint.zero
    var lightingMode: SkyLightingMode = .multiply
    var output: SkyReplacementOutput = .newLayers
    /// Decontaminate Edge: the old sky's colour solved out of the horizon's edge pixels.
    /// On in the dialog; `parse` reads an absent key as off, so a step recorded before the
    /// option existed replays its old result.
    var decontaminate = true

    /// The eight sliders: key, label, range and default, in the sheet's order.
    static let sliders: [(key: String, label: String, range: ClosedRange<Double>, initial: Double)] = [
        ("shift_edge", "Shift Edge:", -100...100, 0),
        ("fade_edge", "Fade Edge:", 0...100, 3),
        ("brightness", "Brightness:", -100...100, 0),
        ("temperature", "Temperature:", -100...100, 0),
        ("scale", "Scale:", 25...400, 100),
        ("lighting", "Lighting Adjustment:", 0...100, 50),
        ("color_adjustment", "Color Adjustment:", 0...100, 50),
        ("edge_lighting", "Edge Lighting:", 0...100, 0),
    ]

    func value(_ key: String) -> Double {
        switch key {
        case "shift_edge": return shiftEdge
        case "fade_edge": return fadeEdge
        case "brightness": return brightness
        case "temperature": return temperature
        case "scale": return scale
        case "lighting": return lighting
        case "color_adjustment": return colorAdjustment
        default: return edgeLighting
        }
    }

    mutating func set(_ key: String, _ value: Double) {
        switch key {
        case "shift_edge": shiftEdge = value
        case "fade_edge": fadeEdge = value
        case "brightness": brightness = value
        case "temperature": temperature = value
        case "scale": scale = value
        case "lighting": lighting = value
        case "color_adjustment": colorAdjustment = value
        default: edgeLighting = value
        }
    }

    /// The core's `params_json` for these settings (the name stays the core's default for the
    /// output).
    var coreParams: [String: Any] {
        var p: [String: Any] = ["flip": flip, "offset": [Double(offset.x), Double(offset.y)],
                                "lighting_mode": lightingMode.rawValue, "output": output.rawValue,
                                "decontaminate": decontaminate]
        for slider in Self.sliders { p[slider.key] = value(slider.key) }
        return p
    }

    /// `sky_replacement`'s arguments for these settings (no `layer`, `sky` or `document_id`):
    /// what the Action step records.
    var arguments: [String: Any] {
        var a: [String: Any] = ["flip": flip, "lighting_mode": lightingMode.rawValue,
                                "output": output.rawValue, "decontaminate": decontaminate,
                                "offset_x": ActionArgs.number(Double(offset.x)),
                                "offset_y": ActionArgs.number(Double(offset.y))]
        for slider in Self.sliders { a[slider.key] = ActionArgs.number(value(slider.key)) }
        return a
    }

    /// The settings `sky_replacement`'s arguments name; absent keys keep the defaults, except
    /// `decontaminate`, which is off when absent (older recorded steps lack it).
    static func parse(_ a: [String: Any]) throws -> SkyReplacementSettings {
        func number(_ key: String) throws -> Double? {
            guard let value = a[key] else { return nil }
            let parsed: Double?
            switch value {
            case let text as String: parsed = Double(text.trimmingCharacters(in: .whitespaces))
            case let number as NSNumber where CFGetTypeID(number) != CFBooleanGetTypeID():
                parsed = number.doubleValue
            default: parsed = nil
            }
            guard let parsed, parsed.isFinite else {
                throw DoctoredCoreError(message: "\(key) must be a number.")
            }
            return parsed
        }
        var settings = SkyReplacementSettings()
        settings.decontaminate = false
        for slider in sliders {
            guard let value = try number(slider.key) else { continue }
            guard slider.range.contains(value) else {
                throw DoctoredCoreError(message: "\(slider.key) must be in \(Int(slider.range.lowerBound))…"
                    + "\(Int(slider.range.upperBound)).")
            }
            settings.set(slider.key, value)
        }
        for key in ["offset_x", "offset_y"] {
            guard let value = try number(key) else { continue }
            guard (-1...1).contains(value) else {
                throw DoctoredCoreError(message: "\(key) must be in -1…1.")
            }
            if key == "offset_x" { settings.offset.x = value } else { settings.offset.y = value }
        }
        if let value = a["flip"] {
            if let text = value as? String, text == "true" || text == "false" {
                settings.flip = text == "true"
            } else if let number = value as? NSNumber, CFGetTypeID(number) == CFBooleanGetTypeID() {
                settings.flip = number.boolValue
            } else {
                throw DoctoredCoreError(message: "flip must be true or false.")
            }
        }
        if let value = a["decontaminate"] {
            if let text = value as? String, text == "true" || text == "false" {
                settings.decontaminate = text == "true"
            } else if let number = value as? NSNumber, CFGetTypeID(number) == CFBooleanGetTypeID() {
                settings.decontaminate = number.boolValue
            } else {
                throw DoctoredCoreError(message: "decontaminate must be true or false.")
            }
        }
        if let value = a["lighting_mode"] {
            guard let name = value as? String, let mode = SkyLightingMode(rawValue: name) else {
                throw DoctoredCoreError(message: "lighting_mode must be multiply or screen.")
            }
            settings.lightingMode = mode
        }
        if let value = a["output"] {
            guard let name = value as? String, let output = SkyReplacementOutput(rawValue: name) else {
                throw DoctoredCoreError(message: "output must be new_layers or duplicate_layer.")
            }
            settings.output = output
        }
        return settings
    }
}

enum SkyReplacementRun {
    static let name = "Sky Replacement"
    static let menuPath = "Edit ▸ Sky Replacement"

    /// Why `layer` of `document` cannot take Sky Replacement, nil when it can: a 32-bit or CMYK
    /// document (the command is disabled there, take 1), a Lab Color document (refused for
    /// good, a Doctored decision: `LabDocument.skyRefusal`, #1 Lab phase 6b-iii), and a group or
    /// adjustment layer (the core's own sentence).
    static func refusal(document: ImageDocument, layer: Int) -> String? {
        guard let doc = document.doc else { return "Document has no image" }
        if doc.isLab { return LabDocument.skyRefusal }
        if doc.isFloat { return "Sky Replacement is not available in a 32-bit document." }
        if doc.isCmyk { return "Sky Replacement is not available in a CMYK document." }
        guard let info = doc.layerInfo(layer) else { return "Layer \(layer) is out of range." }
        if info.isGroup || doc.layerIsAdjustment(layer) {
            return "sky replacement needs a raster layer; layer \(layer) is not one"
        }
        return nil
    }

    /// The sky model's refined canvas-sized coverage of the document's composite (Select ▸ Sky's
    /// own run) and the model that made it. Throws `ModelGate.notInstalled("sky")` and
    /// `SkySelection.noSky`.
    static func mask(document: ImageDocument) throws -> (coverage: [UInt16], entry: ModelCatalogEntry) {
        let capture = try SelectionCapture(document: document)
        let found = try SkySelection.shape(capture, in: document)
        let width = capture.doc.width, height = capture.doc.height
        if case .mask16(let samples) = found.shape { return (samples, found.entry) }
        guard let selection = CanvasSelection(shape: found.shape, canvasWidth: width, canvasHeight: height)
        else { throw DoctoredCoreError(message: SkySelection.noSky) }
        return (selection.maskSamples16(), found.entry)
    }

    /// Replaces `layer`'s sky with `sky` through `coverage` and publishes the output as ONE edit
    /// recording `record`, then makes the output entry active: the group for New Layers, the
    /// layer for Duplicate Layer. The plan is admitted on main; the core edit and the display
    /// preparation run as one cancellable job; a document changed underneath publishes nothing.
    /// Returns the output entry's index, nil when nothing was published.
    static func apply(document: ImageDocument, layer: Int, settings: SkyReplacementSettings,
                      sky: RasterImage, coverage: [UInt16], record: [ActionStep]) throws -> Int? {
        assert(Thread.isMainThread)
        if let reason = refusal(document: document, layer: layer) { throw DoctoredCoreError(message: reason) }
        guard let source = document.doc, !document.isLiveEditing else {
            throw DoctoredCoreError(message: "The document changed before Sky Replacement started.")
        }
        guard coverage.count == source.width * source.height else {
            throw DoctoredCoreError(message: "The sky mask does not match the canvas.")
        }
        let revision = document.revision
        let state = document.captureEditingState()
        let planned = RasterDocument.skyReplaceScratchBytes(
            width: source.width, height: source.height, skyWidth: sky.width, skyHeight: sky.height,
            bits: source.isHighDepth ? 16 : 8, output: settings.output,
            decontaminate: settings.decontaminate)
        _ = try RasterResources.shared.admit(scratch: planned)
        let budget: (limit: UInt64, other: UInt64)? =
            source.isHighDepth ? nil : try source.legacyPrecisionWorkContext()
        let params = settings.coreParams
        let control = try RasterJobControl()
        guard control.setScratchLimit(planned) else {
            throw DoctoredCoreError(message: "Could not bound Sky Replacement work.")
        }
        let prepared = try BackgroundWork.wait(
            title: name, control: control, revealAfter: DocumentJobs.panelDelay
        ) {
            try WorkerTask(scratchEstimate: planned) { () -> PreparedDocumentEdit in
                try control.beginCompoundOperation()
                let replaced = try control.withChildOperation(from: 0, to: 0.85) { child in
                    guard child.setScratchLimit(planned) else {
                        throw DoctoredCoreError(message: "Could not bound Sky Replacement work.")
                    }
                    return try source.skyReplacing(
                        layer: layer, sky: sky, coverage: coverage, params: params,
                        budget: budget, control: child)
                }
                return try control.withChildOperation(from: 0.85, to: 1) { child in
                    PreparedDocumentEdit(replaced, scratch: BackgroundWork.scratch, control: child)
                }
            }
        }
        guard !control.isCancelled else { throw JobCancelled() }
        guard document.revision == revision, document.doc === source, !document.isLiveEditing,
              (!source.isHighDepth && source.retainedSourceIndices.isEmpty)
                || document.isCurrentEditingState(state) else {
            throw DoctoredCoreError(message: "The document changed while Sky Replacement ran. Run it again.")
        }
        let published = prepared.document
        document.preparedEdit = prepared
        defer { document.preparedEdit = nil }
        try document.preflightPublication(published, record: record)
        withUndoGroup(document.undoManager) {
            document.applyEdit(name, record: record) { current in current === source ? published : nil }
        }
        guard document.doc === published else { return nil }
        // New Layers mints four entries (five with Decontaminate Edge) and the container is last
        // in the postorder, so the highest new index is the group; Duplicate Layer mints the
        // one layer.
        let before = Set((0..<source.layerCount).compactMap { source.layerInfo($0)?.id })
        guard let added = (0..<published.layerCount).last(where: { index in
            published.layerInfo(index).map { !before.contains($0.id) } ?? false
        }) else { return nil }
        document.activeLayerIndex = added
        return added
    }
}
