import AppKit

// Filter ▸ Neural Filters ▸ Depth Blur: the depth model (Depth Anything V2
// Small, `ModelCatalog`) runs once on the layer or the selection's float, and the core kernel
// (`depth_blur.rs`: Lens Blur's disc at a radius that follows the guided depth, then Haze, a grade
// and Grain) runs on `dr_image_filter_op` "depth_blur" at 8 bits and the selected edit of the same
// name at 16 and 32 bits, the depth map riding in its params as Liquify's strokes do. The sheet
// (`DepthBlurSheet.swift`), `depth_blur` over MCP (`AgentServer+DepthBlur.swift`) and a replayed
// Action all go through `DepthBlurRun`. Actions record the settings only — never the depth map
// or the model version — so a replay runs the model again.

/// Where the result goes: the layer itself, or a copy of it above.
enum DepthBlurOutput: String {
    case layer
    case newLayer = "new_layer"
}

/// The sheet's controls, in the core's units.
struct DepthBlurSettings: Equatable {
    var focusSubject = true
    /// Canvas pixels; nil while no point was picked.
    var focalPoint: CGPoint?
    var focalDistance = 0.0
    var focalRange = 20.0
    var blurStrength = 50.0
    var haze = 0.0
    var warmth = 0.0
    var brightness = 0.0
    var saturation = 0.0
    var grain = 0.0
    var depthMapOnly = false
    var output: DepthBlurOutput = .layer

    /// The eight sliders: key, range and default, in the sheet's order.
    static let sliders: [(key: String, label: String, range: ClosedRange<Double>, initial: Double)] = [
        ("focal_distance", "Focal Distance:", 0...100, 0),
        ("focal_range", "Focal Range:", 0...100, 20),
        ("blur_strength", "Blur Strength:", 0...100, 50),
        ("haze", "Haze:", 0...100, 0),
        ("warmth", "Warmth:", -50...50, 0),
        ("brightness", "Brightness:", -50...50, 0),
        ("saturation", "Saturation:", -50...50, 0),
        ("grain", "Grain:", 0...100, 0),
    ]

    func value(_ key: String) -> Double {
        switch key {
        case "focal_distance": return focalDistance
        case "focal_range": return focalRange
        case "blur_strength": return blurStrength
        case "haze": return haze
        case "warmth": return warmth
        case "brightness": return brightness
        case "saturation": return saturation
        default: return grain
        }
    }

    mutating func set(_ key: String, _ value: Double) {
        switch key {
        case "focal_distance": focalDistance = value
        case "focal_range": focalRange = value
        case "blur_strength": blurStrength = value
        case "haze": haze = value
        case "warmth": warmth = value
        case "brightness": brightness = value
        case "saturation": saturation = value
        default: grain = value
        }
    }

    /// `depth_blur`'s arguments for these settings (no `layer`, no `document_id`): what the
    /// Action step records.
    var arguments: [String: Any] {
        var a: [String: Any] = ["focus_subject": focusSubject, "depth_map_only": depthMapOnly,
                                "output": output.rawValue]
        for slider in Self.sliders { a[slider.key] = ActionArgs.number(value(slider.key)) }
        if let focalPoint {
            a["focal_x"] = ActionArgs.number(Double(focalPoint.x))
            a["focal_y"] = ActionArgs.number(Double(focalPoint.y))
        }
        return a
    }

    /// The settings `depth_blur`'s arguments name; absent keys keep the defaults. A focal
    /// point without `focus_subject` turns Focus Subject off, as picking one in the sheet does.
    static func parse(_ a: [String: Any]) throws -> DepthBlurSettings {
        func number(_ key: String) throws -> Double? {
            guard let value = a[key] else { return nil }
            let parsed: Double?
            switch value {
            case let text as String: parsed = Double(text.trimmingCharacters(in: .whitespaces))
            case let number as NSNumber where CFGetTypeID(number) != CFBooleanGetTypeID():
                parsed = number.doubleValue
            default: parsed = nil
            }
            guard let parsed, parsed.isFinite else { throw DoctoredCoreError(message: "\(key) must be a number.") }
            return parsed
        }
        func flag(_ key: String) throws -> Bool? {
            guard let value = a[key] else { return nil }
            if let text = value as? String, text == "true" || text == "false" { return text == "true" }
            guard let number = value as? NSNumber, CFGetTypeID(number) == CFBooleanGetTypeID() else {
                throw DoctoredCoreError(message: "\(key) must be true or false.")
            }
            return number.boolValue
        }
        var settings = DepthBlurSettings()
        for slider in sliders {
            guard let value = try number(slider.key) else { continue }
            guard slider.range.contains(value) else {
                throw DoctoredCoreError(message: "\(slider.key) must be in \(Int(slider.range.lowerBound))…"
                    + "\(Int(slider.range.upperBound)).")
            }
            settings.set(slider.key, value)
        }
        switch (try number("focal_x"), try number("focal_y")) {
        case (nil, nil): break
        case (let x?, let y?): settings.focalPoint = CGPoint(x: x, y: y)
        default: throw DoctoredCoreError(message: "focal_x and focal_y go together.")
        }
        settings.focusSubject = try flag("focus_subject") ?? (settings.focalPoint == nil)
        settings.depthMapOnly = try flag("depth_map_only") ?? false
        if let output = a["output"] {
            guard let name = output as? String, let parsed = DepthBlurOutput(rawValue: name) else {
                throw DoctoredCoreError(message: "output must be layer or new_layer.")
            }
            settings.output = parsed
        }
        return settings
    }
}

/// The model's answer for one capture: the depth over `frame` and, when asked, Vision's subject
/// at the depth's size.
struct DepthBlurInput {
    /// Canvas pixels: the selection's bounds on the layer, else the layer on the canvas.
    let frame: CGRect
    let depth: DepthMap
    let subject: [UInt8]?

    /// The depth `D = 1 − n` at depth-map index `i`, `n` the min/max-normalized sample.
    private func depthValue(_ i: Int, low: Float, span: Float) -> Double {
        1 - Double((depth.samples[i] - low) / span)
    }

    /// The focus the kernel uses for `settings`, in percent: the subject-weighted median depth,
    /// the depth at the focal point, or Focal Distance. Reported by `depth_blur`; the kernel's
    /// own `DepthBlur::focus` is the computation of record.
    func focalDistance(_ settings: DepthBlurSettings) -> Double {
        guard let low = depth.samples.min(), let high = depth.samples.max(), high - low > 1e-6 else {
            return settings.focalDistance
        }
        let span = high - low
        if settings.focusSubject, let subject {
            var weighted: [(Double, Double)] = []
            var total = 0.0
            for (i, s) in subject.enumerated() where s > 0 {
                weighted.append((depthValue(i, low: low, span: span), Double(s) / 255))
                total += Double(s) / 255
            }
            guard total > 0 else { return settings.focalDistance }
            weighted.sort { $0.0 < $1.0 }
            var running = 0.0
            for (d, w) in weighted {
                running += w
                if running >= total / 2 { return d * 100 }
            }
            return (weighted.last?.0 ?? 0) * 100
        }
        if let point = settings.focalPoint, frame.width > 0, frame.height > 0 {
            let x = min(depth.width - 1, max(0, Int((point.x - frame.minX) / frame.width * CGFloat(depth.width))))
            let y = min(depth.height - 1, max(0, Int((point.y - frame.minY) / frame.height * CGFloat(depth.height))))
            return depthValue(y * depth.width + x, low: low, span: span) * 100
        }
        return settings.focalDistance
    }
}

enum DepthBlurRun {
    static let notInstalled = "the depth model is not installed; download it in Settings ▸ Models"
    static let name = "Depth Blur"
    /// The capture's own term beside the model's `inferenceBytes`, per canvas pixel: the layer's
    /// canvas-sized 8-bit display and its `CGImage` (4 + 4), both held while the model runs.
    /// The record is on `ModelCatalogEntry.inferenceBytes`.
    static let captureBytesPerPixel: UInt64 = 8

    /// The catalogue's depth model, when it is ready in the store.
    static var readyModel: (entry: ModelCatalogEntry, compiled: URL)? {
        guard let entry = ModelCatalog.entry(capability: "depth"),
              let compiled = ModelStore.shared.compiledModelURL(entry.id) else { return nil }
        return (entry, compiled)
    }

    /// Runs the depth model (and Vision's subject, with `needsSubject`) on `layer`'s 8-bit
    /// display cropped to its frame, as one admitted, cancellable worker job. Refuses
    /// `notInstalled`, an empty layer, and Vision's own "no subject" sentence; with
    /// `subjectOptional` (the sheet), no subject returns the depth alone instead, so the model
    /// runs once either way.
    static func capture(document: ImageDocument, layer: Int, needsSubject: Bool,
                        subjectOptional: Bool = false) throws -> DepthBlurInput {
        guard let doc = document.doc else { throw DoctoredCoreError(message: "Document has no image") }
        guard let model = readyModel else { throw DoctoredCoreError(message: notInstalled) }
        let canvas = CGRect(x: 0, y: 0, width: doc.width, height: doc.height)
        let frame = FilterFrame.pickerFrame(document: document, target: .layer, layer: layer,
                                            selectionBounds: true).intersection(canvas).integral
        let empty = DoctoredCoreError(message: "depth blur needs visible pixels; layer \(layer) is empty")
        guard !frame.isNull, !frame.isEmpty, let bounds = doc.layerBounds(layer),
              CGRect(x: bounds.x, y: bounds.y, width: bounds.width, height: bounds.height).intersects(frame)
        else { throw empty }
        // In its own pool, so the canvas-sized display and its CGImage are freed when the
        // capture returns rather than when the event's pool drains, after the filter has run.
        let crop: CGImage = try autoreleasepool {
            let display: RasterImage
            if doc.isHighDepth {
                display = try doc.nativeDisplay(layer: layer)
            } else {
                guard let image = doc.layerCanvasImage(layer) else { throw empty }
                display = image
            }
            guard let full = display.makeCGImage(in: doc.colorSpace), let crop = full.cropping(to: frame) else {
                throw DoctoredCoreError(message: "The layer could not be prepared for the depth model.")
            }
            return crop
        }
        let scratch = model.entry.inferenceBytes + UInt64(doc.width * doc.height) * captureBytesPerPixel
        _ = try RasterResources.shared.admit(scratch: scratch)
        let control = try RasterJobControl()
        let compiled = model.compiled
        let result = try BackgroundWork.wait(title: "Estimating Depth", control: control,
                                             revealAfter: DocumentJobs.panelDelay) {
            try WorkerTask(scratchEstimate: scratch) { () -> (DepthMap, [UInt8]?) in
                let depth = try DepthEstimator.estimate(crop, compiledModel: compiled)
                guard !control.isCancelled else { throw JobCancelled() }
                guard needsSubject else { return (depth, nil) }
                let subjects: SubjectSelection.Subjects
                do { subjects = try SubjectSelection.mask(in: crop) } catch let failure as SubjectSelection.Failure {
                    if subjectOptional, case .noSubject = failure { return (depth, nil) }
                    throw DoctoredCoreError(message: failure.message)
                }
                guard !control.isCancelled else { throw JobCancelled() }
                guard let subject = SubjectSelection.resampled(
                    subjects.mask, crop.width, crop.height, toWidth: depth.width, height: depth.height)
                else { throw DoctoredCoreError(message: SubjectSelection.Failure.unreadableMask.message) }
                return (depth, subject)
            }
        }
        guard !control.isCancelled else { throw JobCancelled() }
        return DepthBlurInput(frame: frame, depth: result.0, subject: result.1)
    }

    /// The core request for `s` over `input`, in the target plane's pixels: the frame and the
    /// focal point move by the layer's canvas origin and divide by `scale` (a preview proxy's
    /// factor, 1 otherwise). A proxy frame is shrunk to whole proxy pixels inside the scaled
    /// frame, so it never leaves the proxy plane; the point is clamped into it.
    static func params(_ s: DepthBlurSettings, _ input: DepthBlurInput, layerOrigin: CGPoint,
                       scale: Double) -> [String: Any] {
        let factor = max(1, scale)
        let x = Double(input.frame.minX - layerOrigin.x) / factor
        let y = Double(input.frame.minY - layerOrigin.y) / factor
        let right = Double(input.frame.maxX - layerOrigin.x) / factor
        let bottom = Double(input.frame.maxY - layerOrigin.y) / factor
        let x0 = max(0, Int(x.rounded(.up))), y0 = max(0, Int(y.rounded(.up)))
        let w = max(1, Int(right.rounded(.down)) - x0), h = max(1, Int(bottom.rounded(.down)) - y0)
        let samples = input.depth.samples
        let low = samples.min() ?? 0, high = samples.max() ?? 0
        // Normalized here and written at five decimals, so a 518 × 392 map stays about 1.6 MB
        // of JSON; the kernel normalizes again and refuses a map with no range.
        let span = high - low
        let normalized: [Double] = span > 1e-6
            ? samples.map { (Double(($0 - low) / span) * 100_000).rounded() / 100_000 } : samples.map(Double.init)
        var depth: [String: Any] = [
            "width": input.depth.width, "height": input.depth.height,
            "samples": normalized, "frame": [x0, y0, w, h],
        ]
        // Only when the settings use it: the kernel holds every masked pixel sharp, so a mask
        // sent with Focus Subject off would differ from the recorded step's replay, which sends none.
        if s.focusSubject, let subject = input.subject { depth["subject"] = subject.map(Int.init) }
        var params: [String: Any] = [
            "focus_subject": s.focusSubject, "depth_map_only": s.depthMapOnly, "depth": depth,
        ]
        for slider in DepthBlurSettings.sliders { params[slider.key] = s.value(slider.key) }
        if let point = s.focalPoint, !s.focusSubject {
            let px = (Double(point.x - layerOrigin.x) / factor).clamped(Double(x0), Double(x0 + w))
            let py = (Double(point.y - layerOrigin.y) / factor).clamped(Double(y0), Double(y0 + h))
            params["focal_point"] = [px, py]
        }
        return params
    }

    /// Runs the filter for `settings` over `input` on `layer` and publishes it once, recording
    /// `record`: the layer itself, or with New Layer a copy above it named "<name> Depth Blur".
    /// Returns the written layer, nil when nothing changed. At 8 bits a described layer's
    /// description is dropped inside the same edit; the sheet asks first (`confirmRasterize`)
    /// and the agent reports it.
    static func apply(document: ImageDocument, layer: Int, settings: DepthBlurSettings,
                      input: DepthBlurInput, record: [ActionStep]) throws -> Int? {
        guard let doc = document.doc, let placement = doc.layerPlacement(layer) else {
            throw DoctoredCoreError(message: "The target layer no longer exists.")
        }
        if doc.hasRetainedSource(layer) { throw DoctoredCoreError(message: RetainedImagePolicy.rasterizeFirst) }
        let params = Self.params(settings, input, layerOrigin: CGPoint(x: placement.x, y: placement.y), scale: 1)
        let newLayer = settings.output == .newLayer
        let target = newLayer ? layer + 1 : layer
        let copyName = (doc.layerInfo(layer)?.name ?? "Layer") + " Depth Blur"
        if doc.isHighDepth {
            let capture = try PrecisionEditorCapture(document)
            guard newLayer else {
                let edit = try SelectedAdjustment.edit(capture, layer: layer, filter: "depth_blur",
                                                       params: params, target: .layer)
                let result = try edit.run(title: name)
                return try capture.publish(result, to: document, name: name, record: record) ? layer : nil
            }
            guard let copied = try capture.facade.duplicatingLayer(layer),
                  let named = try copied.withLayerName(target, copyName), let source = named.precision else {
                throw DoctoredCoreError(message: "The layer could not be duplicated for Depth Blur.")
            }
            let request = try SelectedAdjustment.request(layer: target, filter: "depth_blur", params: params)
            let coverage = try capture.selection.map {
                try PrecisionStrokeCoverage.selection(width: capture.facade.width, height: capture.facade.height,
                                                      selection: $0)
            }
            let edit = try PrecisionCapturedEdit(source, operation: .selected(request: request, coverage: coverage))
            guard let result = try edit.run(title: name) else { return nil }
            return try capture.publish(result, to: document, name: name, record: record) ? target : nil
        }
        var source = doc
        if newLayer {
            guard let copied = try doc.duplicatingLayer(layer), let named = try copied.withLayerName(target, copyName)
            else { throw DoctoredCoreError(message: "The layer could not be duplicated for Depth Blur.") }
            source = named
        }
        guard let capture = OperationCapture(doc: source, layerIndex: target, target: .layer,
                                             selection: document.canvasSelection?.maskBytes()) else {
            throw DoctoredCoreError(message: "Could not read the requested edit target")
        }
        let request = ByteFilterRequest(op: "depth_blur", params: params)
        guard let prepared = try ByteFilterAdmission.runPrepared(request, capture: capture, title: name) else {
            return nil
        }
        var updated = prepared.document
        if document.layerDescribesSource(target, in: updated) {
            updated = updated.withLayerMeta(target, nil) ?? updated
        } else {
            document.preparedEdit = prepared
        }
        defer { document.preparedEdit = nil }
        try document.preflightPublication(updated, record: [])
        let published = updated
        withUndoGroup(document.undoManager) {
            document.applyEdit(name, record: record) { current in current === doc ? published : nil }
        }
        return document.doc === published ? target : nil
    }
}

private extension Double {
    func clamped(_ low: Double, _ high: Double) -> Double { Swift.min(high, Swift.max(low, self)) }
}
