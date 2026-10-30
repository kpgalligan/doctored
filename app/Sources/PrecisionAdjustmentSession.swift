import AppKit

// No byte transform plate becomes a source.

/// The existing adjustment controls share this typed capture for preview and
/// Apply. It publishes through the ordinary document history, once.
final class PrecisionAdjustmentSession {
    private weak var document: ImageDocument?
    private weak var canvas: ImageCanvasView?
    private let capture: PrecisionEditorCapture
    var histogramCapture: PrecisionEditorCapture { capture }
    private let mode: AdjustmentSheetMode
    /// The exact stage, staged behind a proxy when `prepareProxy` ran.
    private let renderer = StagedPreview()
    private var proxyLoader: ProxySourceLoader<PrecisionProxySource>?
    /// The latest request's proxy filter and parameters for a proxy factor.
    private var latestProxy: ((Double) -> (String, [String: Any])?)?
    private var control: RasterJobControl?
    private var finished = false
    private var scalar: PrecisionScalarAdjustmentSession?
    let previewStatus = NSTextField(labelWithString: "")
    private let previewFailure = SelectedPreviewFailure()
    private var generation: UInt64 = 0
    /// A Levels/Curves sheet's After plot and ⌥ display; nil for
    /// every other caller, which presents exactly as before.
    var feedback: ToneSheetFeedback?
    /// A composite preview of u16 or float samples: not a scalar target. A
    /// float composite plots unit bins and shows ⌥ at `>= 1.0` /
    /// `<= 0.0`. A CMYK document (#1 phase 5) takes the ⌥ clipping display
    /// through its own ink (`cmyk_clip_display`), with no After plot.
    var supportsCompositeFeedback: Bool {
        scalar == nil
    }

    /// True for a CMYK document's session: its ⌥ display is the ink's clipping, never the view's.
    var isCmyk: Bool { capture.source.isCmyk }

    init?(_ document: ImageDocument, canvas: ImageCanvasView, mode: AdjustmentSheetMode) {
        guard document.doc?.precision != nil, let capture = try? PrecisionEditorCapture(document) else { return nil }
        self.document = document
        self.canvas = canvas
        self.capture = capture
        self.mode = mode
        if case .destructive = mode, capture.target.isCoverage {
            scalar = PrecisionScalarAdjustmentSession(document: document, canvas: canvas,
                capture: capture, status: previewStatus)
        }
        renderer.onRender = { [weak self] image in
            guard let self, !self.finished, let document = self.document else { return }
            let delivered = self.previewFailure.value
            guard delivered.generation == self.generation else { return }
            do {
                try self.capture.requireCurrent(document)
                // The Color Samplers' "after" values read the projection
                // this preview was made from, so it lands first.
                self.canvas?.previewPrecisionSource = delivered.projection.flatMap { projection in
                    image.map { (preview: $0, projection: projection) }
                }
                if let feedback = self.feedback { feedback.present(image, on: self.canvas) } else { self.canvas?.previewImage = image }
                self.previewStatus.stringValue = delivered.message ?? ""
            } catch {
                self.canvas?.previewImage = nil
                self.previewStatus.stringValue = error.localizedDescription
            }
        }
        // A proxy is smaller than the canvas, so the Color Samplers read "—"
        // (no projection) until the exact stage restores their readings.
        renderer.onProxy = { [weak self] image in
            guard let self, !self.finished, let document = self.document,
                  (try? self.capture.requireCurrent(document)) != nil else { return }
            self.canvas?.previewPrecisionSource = nil
            self.canvas?.previewImage = image
        }
    }

    /// Builds the downscaled proxy source once, off main. A no-op unless
    /// this is a destructive edit of a raster layer's pixels (not a scalar
    /// target, not Quick Mask, not 32-bit float) on a canvas above 4 MP.
    func prepareProxy() {
        guard proxyLoader == nil, case .destructive = mode, scalar == nil, capture.target == .layer,
              !capture.quickMask, !capture.facade.isFloat, !capture.source.isCmyk,
              let scale = ProxyScale(width: capture.facade.width, height: capture.facade.height) else { return }
        let capture = capture
        proxyLoader = ProxySourceLoader(build: { try PrecisionProxySource(capture, scale: scale) },
                                        onReady: { [weak self] in self?.proxyReady() })
    }

    /// The source arrived after the current request: run that request's proxy
    /// stage now; its exact stays queued.
    private func proxyReady() {
        guard !finished, let work = proxyWork(latestProxy) else { return }
        renderer.lateProxy(work)
    }

    private func proxyWork(_ proxy: ((Double) -> (String, [String: Any])?)?) -> (() -> CGImage?)? {
        guard let proxy, let source = proxyLoader?.source,
              let (filter, params) = proxy(source.scale.factor) else { return nil }
        return try? source.previewWork(filter: filter, params: params)
    }

    /// Where a `.create` insertion lands — the ONE answer the mask write and
    /// the `onCommitted` callback both read, so the two cannot disagree. A
    /// typed adjustment goes ABOVE the ACTIVE entry (12E-2), or into it as
    /// its top child when it is a group (`newEntryPosition`).
    private var landingIndex: Int {
        capture.facade.newEntryLandingIndex(active: capture.layer)
    }

    private func operation(op: AdjustmentLayerOp, params: [String: Any]) throws -> PrecisionCapturedEdit {
        let adjustment = try PrecisionAdjustment(op: op, params: params)
        let operation: PrecisionCapturedEdit.Operation
        switch mode {
        case .destructive:
            return try SelectedAdjustment.edit(capture, layer: capture.layer, filter: op.rawValue, params: params)
        case .create:
            // 12E-2: a typed adjustment lands ABOVE the ACTIVE entry, exactly
            // as the byte path does — the "topmost entry" guard is gone. Quick
            // Mask still refuses, because its overlay is not a stack position.
            guard !capture.quickMask else {
                throw DoctoredCoreError(message: "Exit Quick Mask before adding a 16-bit adjustment layer.")
            }
            operation = .add(name: op.displayName, adjustment: adjustment, opacity: 1,
                             anchor: capture.layer,
                             position: capture.facade.newEntryPosition(active: capture.layer))
        case .edit(let index, _): operation = .set(index: index, adjustment: adjustment)
        }
        return try PrecisionCapturedEdit(capture.source, operation: operation)
    }

    private static func compute(_ operation: PrecisionCapturedEdit, mode: AdjustmentSheetMode,
                                width: Int, height: Int, nativeSelection: [UInt16]?, landing: Int,
                                control: RasterJobControl) throws -> RasterPrecisionDocument? {
        guard var result = try operation.compute(control: control) else { return nil }
        if case .create(let selection) = mode, let selection {
            guard !control.isCancelled else { throw JobCancelled() }
            let buffer = try PrecisionScalarBuffer(samples: nativeSelection ?? selection.map { UInt16($0) * 257 }, width: width, height: height)
            // The mask goes onto the record that was just INSERTED, which is
            // the slot rule's landing index — not `layer_count - 1`, which is
            // only the same thing when the insertion appended at the top.
            let command = try PrecisionPixelCommand.setMask(layer: landing, width: width, height: height)
            let context = try PrecisionWorkContext(source: result)
            // Each independent kernel has a single-use token. The owning
            // session checks cancellation between these bounded stages.
            if let masked = try result.editingPixels(request: command.request, samples: buffer.samples,
                workingLimit: context.limit, otherRetained: context.otherRetained) { result = masked }
        }
        guard !control.isCancelled else { throw JobCancelled() }
        return result
    }

    func request(op: AdjustmentLayerOp, params: [String: Any]) {
        if let scalar { scalar.request(filter: op.rawValue, params: params); return }
        requestEdit { [weak self] in
            guard let self else { throw JobCancelled() }
            return try self.operation(op: op, params: params)
        }
    }

    func request(filter: String, params: [String: Any]) {
        request(filter: filter, params: params, proxyParams: nil)
    }

    /// `proxyParams` maps the proxy factor to the proxy's parameters.
    func request(filter: String, params: [String: Any], proxyParams: ((Double) -> [String: Any])?) {
        if let scalar { scalar.request(filter: filter, params: params); return }
        let capture = capture
        requestEdit({ try SelectedAdjustment.edit(capture, layer: capture.layer, filter: filter, params: params) },
                    proxy: proxyParams.map { map in { (filter, map($0)) } })
    }

    /// The coverage the Swift side holds across the exact stage — a
    /// `.create` insertion's canvas-sized u16 mask when there is a selection.
    /// A destructive edit's coverage is its own `PrecisionScalarBuffer`
    /// payload, and its core plan already charges canvas×2 for it.
    private var coverageLeaseBytes: UInt64 {
        guard case .create(let selection) = mode, selection != nil else { return 0 }
        return UInt64(capture.facade.width * capture.facade.height) * 2
    }

    private func requestEdit(_ operation: @escaping () throws -> PrecisionCapturedEdit,
                             proxy: ((Double) -> (String, [String: Any])?)? = nil) {
        guard !finished, let document else { return }
        generation &+= 1
        let generation = generation
        latestProxy = nil
        control?.cancel()
        renderer.cancelCompute?()
        // The previous request is superseded even if this one is refused, so
        // its pending proxy or exact cannot land over the refusal status, and
        // its not-yet-run closures (and their reservations) are released now.
        renderer.supersede()
        do {
            try capture.requireCurrent(document)
        } catch { canvas?.previewImage = nil; previewStatus.stringValue = error.localizedDescription; return }
        latestProxy = proxy
        // The proxy is queued at once; the exact's plan, coverage lease and
        // reservation wait until a superseded exact has returned.
        renderer.request(proxy: proxyWork(proxy)) { [weak self] in
            guard let self, !self.finished, self.generation == generation else { return nil }
            return self.exactWork(operation, generation: generation)
        }
    }

    /// The exact stage, planned and reserved on main. Its compute and its
    /// display preparation run one after the other under one reservation
    /// (`previewReservationBytes`); a refusal clears the preview and shows why.
    private func exactWork(_ operation: () throws -> PrecisionCapturedEdit,
                           generation: UInt64) -> (() -> CGImage?)? {
        guard let document else { return nil }
        do {
            try capture.requireCurrent(document)
            let edit = try operation()
            let display = try capture.facade.projectionScratchEstimate()
            // The Swift-held coverage is a payload lease, counted by every
            // admission on every thread; the reservation holds only what the
            // worker's own stages charge, so it runs as their lease.
            let leaseBytes = coverageLeaseBytes
            let coverage = leaseBytes > 0 ? try PrecisionEditPayload.admitted(bytes: leaseBytes) : nil
            let reservation = try SelectionWorkBudget.reservePreview(
                bytes: edit.previewReservationBytes(display: display), outputBytes: 0)
            control = reservation.control
            let previewControl = reservation.control
            let displayControl = try RasterJobControl()
            renderer.cancelCompute = { previewControl.cancel(); displayControl.cancel() }
            let failure = previewFailure
            previewStatus.stringValue = ""
            let mode = mode
            let nativeSelection = coverage == nil ? nil : capture.selection?.maskSamples16()
            let width = capture.facade.width, height = capture.facade.height
            let landing = self.landingIndex
            let keepsProjection = !document.colorSamplers.items.isEmpty
            let feedsBack = supportsCompositeFeedback && feedback?.isActive == true
            let box = feedsBack ? feedback?.box : nil, clip = feedsBack ? feedback?.clip : nil
            let cmyk = capture.source.isCmyk
            return { withExtendedLifetime(coverage) { reservation.runAsOwnLease { token in
                do {
                    let result = try Self.compute(edit, mode: mode, width: width, height: height, nativeSelection: nativeSelection, landing: landing, control: token) ?? edit.source
                    guard !token.isCancelled else { return nil }
                    box?.reset()
                    // #1 phase 5: a CMYK candidate's ⌥ display is its ink at paper or at 100 %
                    // (`cmyk_clip_display`), marked clipped so the samplers never read it.
                    if cmyk, let box, let clip {
                        let context = try PrecisionWorkContext(source: result)
                        let display = try result.cmykClipDisplay(
                            mode: clip == .shadows ? "shadows" : "highlights", plates: ["c", "m", "y", "k"],
                            workingLimit: context.limit, otherRetained: context.otherRetained)
                        guard !token.isCancelled else { return nil }
                        box.set(ToneFeedbackBox.Frame(bins: nil, failure: nil, clipped: true))
                        failure.set(generation: generation, message: nil)
                        return display.makeCGImage(in: ColorProfile.sRGB)
                    }
                    let facade = try RasterDocument(precision: result)
                    if !cmyk, let box, let clip {
                        // The token is consumed (single-use), so the render,
                        // histogram and display run uncontrolled, with
                        // cancellation checked on either side.
                        let projection = try facade.renderPrecision()
                        guard !token.isCancelled else { return nil }
                        var frame = Self.afterFrame(projection, facade: facade)
                        let image = try projection.clipDisplay(mode: clip.code).makeCGImage(in: ColorProfile.sRGB)
                        guard !token.isCancelled else { return nil }
                        frame.clipped = true
                        box.set(frame)
                        failure.set(generation: generation, message: nil)
                        return image
                    }
                    // The display stage polls its own control, which the next
                    // request and `cancel()` cancel with the compute's.
                    let prepared = PreparedDocumentEdit(facade, control: displayControl)
                    guard !displayControl.isCancelled, !token.isCancelled else { return nil }
                    if !cmyk, let box, let projection = prepared.precisionProjection { box.set(Self.afterFrame(projection, facade: facade)) }
                    let projection = keepsProjection ? prepared.precisionProjection : nil
                    if prepared.preview == nil { failure.set(generation: generation, message: prepared.error ?? "Native preview could not be prepared. Apply will validate the full result.", projection: projection) }
                    else { failure.set(generation: generation, message: nil, projection: projection) }
                    return token.isCancelled ? nil : prepared.preview
                } catch { if !token.isCancelled { failure.set(generation: generation, message: error.localizedDescription) }; return nil }
            } } }
        } catch { canvas?.previewImage = nil; previewStatus.stringValue = error.localizedDescription; return nil }
    }

    /// The After bins of a projection; a failed read is the frame's failure.
    /// A float facade reads the projection's unit bins through the scoped
    /// analysis; 16 bits read `developHistogram`.
    private static func afterFrame(_ projection: RasterPrecisionImage,
                                   facade: RasterDocument) -> ToneFeedbackBox.Frame {
        do {
            if facade.isFloat {
                let read = try ScopedAnalysisRead(source: facade, scope: .composite, selection: nil,
                                                  projection: projection, floatBins: .unit)
                if let bins = ScopedAnalysisRead.bins(try read.read()) {
                    return ToneFeedbackBox.Frame(bins: bins, failure: nil, clipped: false)
                }
                return ToneFeedbackBox.Frame(bins: nil, failure: "The after histogram could not be read.", clipped: false)
            }
            if let bins = RawDevelopInspectionSource.bins(try projection.developHistogram()) {
                return ToneFeedbackBox.Frame(bins: bins, failure: nil, clipped: false)
            }
            return ToneFeedbackBox.Frame(bins: nil, failure: "The after histogram could not be read.", clipped: false)
        } catch {
            return ToneFeedbackBox.Frame(bins: nil, failure: error.localizedDescription, clipped: false)
        }
    }

    func request(meta: String?) {
        guard let meta, let payload = AdjustmentLayerPayload.decode(meta), let op = payload.knownOp else { return }
        request(op: op, params: payload.params)
    }

    func commit(meta: String?, onCommitted: ((Int) -> Void)? = nil) {
        guard let meta, let payload = AdjustmentLayerPayload.decode(meta), let op = payload.knownOp else { return }
        commit(op: op, params: payload.params, onCommitted: onCommitted)
    }

    private func filter(_ steps: [ActionStep]) -> (String, [String: Any])? {
        guard let step = steps.first, step.tool == "apply_filter",
              let name = step.arguments["filter"] as? String else { return nil }
        var parameters = step.arguments["params"] as? [String: Any] ?? step.arguments
        for key in ["filter", "layer", "target"] { parameters.removeValue(forKey: key) }
        return (name, SelectedAdjustment.numericParameters(parameters))
    }

    func request(steps: [ActionStep]) {
        request(steps: steps, proxySteps: nil)
    }

    /// `proxySteps` maps the proxy factor to the proxy's recorded steps.
    func request(steps: [ActionStep], proxySteps: ((Double) -> [ActionStep])?) {
        guard let (filter, params) = filter(steps) else { return }
        if let scalar { scalar.request(filter: filter, params: params); return }
        let capture = capture
        requestEdit({ try SelectedAdjustment.edit(capture, layer: capture.layer, filter: filter, params: params) },
                    proxy: proxySteps.map { map in { [weak self] factor in self?.proxyFilter(map(factor)) } })
    }

    /// A proxy stage's filter and parameters. A Lens Correction step names its profile ("auto" or
    /// a lens id, v1-lensfun), which is matched here against the full document, not the proxy;
    /// the profile is scale-free, so the proxy takes it unchanged. nil when it does not resolve.
    private func proxyFilter(_ steps: [ActionStep]) -> (String, [String: Any])? {
        guard let (filter, params) = filter(steps) else { return nil }
        guard filter == "lens_correction" else { return (filter, params) }
        return (try? capture.facade.lensResolvedParameters(params, layer: capture.layer)).map { (filter, $0) }
    }

    /// `name` is the sheet's own action name, so the History step reads as it does at 8 bits
    /// ("Develop Filter", the sheet's title, not a name made from the op).
    func commit(steps: [ActionStep], name: String? = nil, onLateBusy: (() -> Void)? = nil) {
        if let (filter, params) = filter(steps) {
            commit(filter: filter, params: params, name: name, onLateBusy: onLateBusy)
        }
    }

    /// `onLateBusy` is a sheet's way back: when the edit's
    /// constructor refuses because the worker slot was taken after the
    /// sheet's own slot wait — behind the rasterize prompt, and still held
    /// after the bounded wait here — the handler runs instead of the alert,
    /// so the sheet can come back with its values. Without one the refusal
    /// is presented as before. `name` is the step's name; without one it is
    /// taken from the filter key.
    func commit(filter: String, params: [String: Any], name: String? = nil, onLateBusy: (() -> Void)? = nil) {
        if let scalar { scalar.commit(filter: filter, params: params, onLateBusy: onLateBusy); return }
        drainForCommit()
        guard let document else { return }
        do {
            try capture.requireCurrent(document)
            guard document.confirmRasterize(layer: capture.layer) else { return }
            try capture.requireCurrent(document)
            BackgroundWork.waitForAnonymousOccupant()
            let edit = try SelectedAdjustment.edit(capture, layer: capture.layer, filter: filter, params: params)
            let name = name ?? filter.replacingOccurrences(of: "_", with: " ").capitalized
            let result = try edit.run(title: name)
            _ = try capture.publish(result, to: document, name: name,
                record: SelectedAdjustment.record(filter: filter, params: params))
        } catch {
            if let onLateBusy, BackgroundWork.isBusyRefusal(error) {
                NSSound.beep()
                onLateBusy()
            } else { document.presentError(error) }
        }
    }

    func content(_ content: NSView) -> NSView {
        previewStatus.textColor = .systemRed
        previewStatus.font = DS.sans(11)
        previewStatus.lineBreakMode = .byWordWrapping
        previewStatus.maximumNumberOfLines = 3
        previewStatus.setAccessibilityIdentifier("tone.preview.status")
        let stack = NSStackView(views: [content, previewStatus])
        stack.orientation = .vertical; stack.alignment = .leading; stack.spacing = 6
        previewStatus.widthAnchor.constraint(lessThanOrEqualToConstant: 360).isActive = true
        return stack
    }

    func commit(op: AdjustmentLayerOp, params: [String: Any], onCommitted: ((Int) -> Void)? = nil) {
        if let scalar { scalar.commit(filter: op.rawValue, params: params); return }
        drainForCommit()
        guard let document else { return }
        do {
            try capture.requireCurrent(document)
            if case .edit(_, let original) = mode,
               AdjustmentSchema.sameEffect(params, original.params, for: op) { return }
            if case .destructive = mode {
                guard document.confirmRasterize(layer: capture.layer) else { return }
                try capture.requireCurrent(document)
            }
            BackgroundWork.waitForAnonymousOccupant()
            let edit = try operation(op: op, params: params)
            let mode = mode, width = capture.facade.width, height = capture.facade.height
            let coverage = coverageLeaseBytes
            let nativeSelection = coverage == 0 ? nil : capture.selection?.maskSamples16()
            let landing = landingIndex
            let result = try PrecisionCapturedWork.run(scratchEstimate: edit.scratchEstimate + coverage,
                title: op.displayName) { token in
                try Self.compute(edit, mode: mode, width: width, height: height, nativeSelection: nativeSelection, landing: landing, control: token)
            }
            let meta = AdjustmentLayerPayload(op: op, params: params).json() ?? ""
            let record: [ActionStep]
            let index: Int?
            switch mode {
            case .destructive: record = SelectedAdjustment.record(filter: op.rawValue, params: params); index = nil
            case .create:
                record = .addAdjustmentLayer(
                    meta: meta, name: op.displayName,
                    position: capture.facade.newEntryPosition(active: capture.layer))
                index = landing
            case .edit(let layer, _):
                record = .editAdjustmentLayer(meta: meta, layer: capture.facade.layerRef(layer)); index = layer
            }
            if try capture.publish(result, to: document, name: op.displayName, record: record), let index { onCommitted?(index) }
        } catch { document.presentError(error) }
    }

    func cancel() {
        scalar?.cancel()
        finished = true
        control?.cancel()
        renderer.cancel()
        canvas?.previewImage = nil
    }

    /// Apply admits only after this session's own preview work has
    /// returned — the exact and proxy stages and the proxy source build — and
    /// with no sampler projection held for the preview. Depth Blur's sheet calls it before its
    /// own Apply (`DepthBlurSheetController`), which records the settings, not the request.
    func drainForCommit() {
        cancel()
        renderer.waitUntilIdle()
        proxyLoader?.waitUntilBuilt()
        previewFailure.set(generation: generation, message: nil)
        canvas?.previewPrecisionSource = nil
    }
}

/// Worker failure state crosses the same immutable preview boundary as pixels,
/// with the authoritative projection the Color Samplers read.
private final class SelectedPreviewFailure {
    typealias Value = (generation: UInt64, message: String?, projection: RasterPrecisionImage?)
    private let lock = NSLock()
    private var result: Value = (0, nil, nil)
    var value: Value { lock.lock(); defer { lock.unlock() }; return result }
    func set(generation: UInt64, message: String?, projection: RasterPrecisionImage? = nil) {
        lock.lock(); result = (generation, message, projection); lock.unlock()
    }
}
