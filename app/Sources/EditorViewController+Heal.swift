import AppKit

/// The Healing Brush and the Spot Healing Brush: the two seams the canvas
/// hands a healing stroke to, and no other logic.
///
/// Both tools are ONE solve per stroke, run as a nonmodal `DocumentJobs` job.
/// The canvas paints the footprint into
/// its overlay while the drag runs — the Healing Brush through the clone
/// pipeline (the sampled pixels, so the drag previews a raw clone), the Spot
/// Healing Brush through the plain dab pipeline (pure coverage, so the drag
/// only ghosts) — and hands the WHOLE footprint here once, at mouse-up,
/// while the live-edit session opened at mouse-down is still open.
/// `handOffHealStroke` owns the footprint, rolls the drag's preview back to
/// the pre-stroke handle and closes that session as a same-handle no-op, asks
/// the rasterize question, and the worker then solves against the captured
/// handle. The job's own completion is the gesture's ONE undo step; a
/// refused, cancelled or stale heal leaves no undo step and no pixel changed.
/// While the Healing Brush's job runs, the drag's last picture stays on the
/// canvas as a display-only PIN (`canvas.previewImage`) so the raw clone does
/// not blink back to the pre-stroke picture; the completed edit's own image
/// change clears it, and `clearHealPin` clears it on every other ending.
extension EditorViewController {

    /// The retouch tools' per-gesture sampling options, read once at
    /// mouse-down. The Patch tool keeps its own store; every brush shares
    /// `PaintToolOptions`.
    static func retouchOptions(for tool: EditorTool) -> PaintToolOptions? {
        switch tool {
        case .clone: return ToolOptionsStore.shared.clone
        case .heal: return ToolOptionsStore.shared.heal
        case .spotHeal: return ToolOptionsStore.shared.spotHeal
        default: return nil
        }
    }

    /// Builds this gesture's sampling plate and latches it, or refuses the
    /// stroke outright.
    ///
    /// Called from `onStrokeBegin` BEFORE the live-edit session opens, which
    /// is the whole point: a plate that cannot be built has to stop the
    /// gesture where stopping it costs nothing. Every other tool latches
    /// nothing and returns true.
    func latchRetouchPlate(_ document: ImageDocument, _ doc: RasterDocument, layer: Int) -> Bool {
        strokePlate = nil
        strokeDestination = nil
        guard let options = Self.retouchOptions(for: strokeTool) else { return true }
        let choice = RetouchSamplingChoice(
            mode: options.sample, ignoreAdjustments: options.ignoreAdjustments)
        let output: RetouchOutput = options.outputToNewLayer ? .newLayer : .layer
        do {
            guard let plate = try RetouchSource.plate(
                doc, layer: layer, choice: choice, output: output)
            else {
                presentHealFailure(
                    RetouchSource.missingPlate(choice.mode), strokeTool.displayName)
                return false
            }
            strokeDestination = options.expandDestination == true && output == .layer
                ? try doc.expandingRetouchDestination(layer) : nil
            strokePlate = plate
            return true
        } catch let error as DoctoredCoreError {
            presentHealFailure(error.message, strokeTool.displayName)
            return false
        } catch {
            presentHealFailure(error.localizedDescription, strokeTool.displayName)
            return false
        }
    }

    /// The image a clone-path stroke stamps from: this gesture's latched
    /// SAMPLING PLATE, built by the core from the document's own layers.
    ///
    /// It used to be nil for everything but one case, and nil meant "use the
    /// canvas's own image" — which is `ImageDocument.previewImage`, a DISPLAY
    /// image that is an 8-bit reduction on a 16-bit document and deliberately
    /// the PREVIOUS revision's picture whenever a render failed. The phase's
    /// rule is that no display preview may be a sampling source, so the plate
    /// answers for the Clone Stamp and the Healing Brush alike, in every
    /// sampling mode.
    ///
    /// The Spot Healing Brush still answers nil, and still means it: its
    /// overlay carries pure coverage and the core generates its source, so
    /// there is nothing to stamp.
    ///
    /// Tagged with the OVERLAY CONTEXT's space rather than the pixels' own
    /// tag. The two differ only on a profile the app cannot draw into — a
    /// LUT profile with no B2A or one the core cannot convert both ways (a
    /// LUT-mode profile is its own drawing space) — where the context
    /// is sRGB by `ColorProfile.drawingSpace`'s rule while the pixels wear the
    /// document's tag — and there CoreGraphics would convert the sampled
    /// texture on its way into the overlay, against this app's own rule that
    /// a sampled colour converts nowhere. Tagging the plate with the
    /// destination's space makes the displaced draw a plain blit on every
    /// document.
    func strokeSourceImage(_ geometry: RetouchGeometry) -> CGImage? {
        guard strokeTool == .clone || strokeTool == .heal, let doc = document?.doc else { return nil }
        do {
            let source: RasterImage
            if let stroke = precisionStroke, let typed = stroke.sourcePlate {
                let transformed = try typed.retouchTransformed(matrix: geometry.values)
                let budget = try PrecisionWorkContext(image: transformed)
                // The captured document's own display rule: a float plate goes
                // through its tone map (`RasterDocument.display`).
                return try stroke.capture.facade.display(of: transformed, context: budget)
                    .makeCGImage(in: doc.drawingSpace)
            } else if let plate = strokePlate { source = plate.image }
            else { throw DoctoredCoreError(message: "No captured retouch source is available.") }
            return try source.retouchTransformed(matrix: geometry.values).makeCGImage(in: doc.drawingSpace)
        } catch {
            document?.presentError(error)
            return nil
        }
    }

    /// Hands the stroke's whole footprint to the worker. `data` is the
    /// canvas-sized premultiplied overlay the stroke rasterized — its alpha
    /// is the footprint's coverage, its RGB the already-aligned source
    /// (ignored by contract for the Spot Healing Brush) — and `actionName` is
    /// the name the undo step will carry.
    ///
    /// The channel-target question is already settled upstream:
    /// `onStrokeBegin` refuses any stroke that would land on a channel from
    /// a tool that cannot paint coverage, and neither healing brush can, so
    /// no stroke reaches this method while an indicator names a channel.
    func commitHealOverlay(_ data: UnsafePointer<UInt8>, _ actionName: String) {
        // No pre-stroke handle means no open live-edit session — nothing to
        // replace and nothing to roll back — and a target that moved since
        // mouse-down (a selection or layer change) must not be healed. Either
        // way the stroke is REFUSED, so `endBytePaintStroke` rolls back and
        // closes the session rather than leaving it open.
        guard !byteStrokeRefused, document != nil, let base = strokeBase,
              byteStrokeTargetIsCurrent(), let capture = byteStrokeCapture else {
            byteStrokeRefused = true
            NSSound.beep()
            return
        }
        let idx = capture.layer
        if strokeTool == .spotHeal {
            startSpotHealJob(data, base: base, layer: idx, actionName: actionName)
            return
        }
        // The canvas only fires this callback for the two healing tools;
        // anything else would be a wiring mistake, and committing an unsolved
        // overlay is the one outcome worth refusing loudly.
        guard strokeTool == .heal else {
            byteStrokeRefused = true
            NSSound.beep()
            return
        }
        startHealJob(data, base: base, layer: idx, actionName: actionName)
    }

    /// The hand-off both healing brushes share, in its one safe order: latch
    /// the recorded steps, OWN the footprint (mouse-up has ended
    /// `canvas.strokeActive`, so the image observer clears the borrowed
    /// overlay during `updateLiveEdit` even when the handle is unchanged),
    /// take the pin from the picture the drag left up, roll the preview back
    /// and close the same-handle session — the eventual worker completion
    /// owns the single undo — then pin (after `endLiveEdit`, whose image
    /// change would clear it) and ask the rasterize question. nil when the
    /// gesture stops here; every such path clears its own pin.
    private func handOffHealStroke(
        _ data: UnsafePointer<UInt8>, base: RasterDocument, layer: Int, actionName: String,
        pinPreview: Bool
    ) -> (steps: [ActionStep], pixels: [UInt8], pin: CGImage?)? {
        guard let document else { return nil }
        let steps = ActionRecorder.shared.gestureRecord(strokeRecord(actionName))
        let pixels = Array(UnsafeBufferPointer(start: data, count: base.width * base.height * 4))
        let pin = pinPreview ? (canvas.proofImage ?? canvas.image) : nil
        document.updateLiveEdit(base)
        document.endLiveEdit(actionName, record: .notACommand)
        canvas.previewImage = pin
        let capturedRevision = document.revision
        guard strokePlate?.backdrop != nil || document.confirmRasterize(layer: layer) else {
            clearHealPin(pin)
            return nil
        }
        guard document.doc === base, document.revision == capturedRevision else {
            clearHealPin(pin)
            presentHealFailure("The document changed while rasterization was being confirmed.", actionName)
            return nil
        }
        return (steps, pixels, pin)
    }

    /// Drops the Healing Brush's display pin — but only while it is still the
    /// pin this job put up, never a picture something else has since shown.
    private func clearHealPin(_ pin: CGImage?) {
        guard let pin, canvas.previewImage === pin else { return }
        canvas.previewImage = nil
    }

    /// The Healing Brush's worker commit: `healJob`, or `healToNewLayerJob`
    /// when the latch carries a backdrop plate (a NEW-layer gesture, whose
    /// membrane must match the picture that layer will sit on).
    private func startHealJob(_ data: UnsafePointer<UInt8>, base: RasterDocument,
                              layer: Int, actionName: String) {
        guard let document else { return }
        // Latched with the gesture, not read again on the worker: the store
        // can move while the job runs, and the footprint was drawn against
        // these choices. The backdrop is the latched HANDLE, not a copy.
        let strength = Self.healStrength(byteStrokeCapture?.options ?? ToolOptionsStore.shared.heal)
        let destination = strokeDestination ?? base
        let backdrop = strokePlate?.backdrop
        // A channel or mask display draws the preview in place of its own
        // base and stands its washes down, so a pin there would show the
        // wrong picture for the whole job: no pin while one is up.
        guard let handoff = handOffHealStroke(data, base: base, layer: layer,
            actionName: actionName, pinPreview: canvas.channelDisplay == nil) else { return }
        let pixels = handoff.pixels
        let pin = handoff.pin
        do {
            let job = try DocumentJobs.start(document: document, layer: layer, title: actionName,
                scratch: UInt64(pixels.count), record: handoff.steps, showProgress: true,
                rasterizesTarget: backdrop == nil, compute: { _, control in
                    if let backdrop {
                        return try destination.healToNewLayerJob(
                            layer, overlay: pixels, strength: strength, plate: backdrop,
                            name: Self.retouchLayerName, control: control)
                    }
                    return try destination.healJob(
                        layer, overlay: pixels, strength: strength, control: control)
                }, completion: { [weak self] job in
                    self?.clearHealPin(pin)
                    if job.state == "failed" || job.state == "stale" {
                        self?.presentHealFailure(job.message ?? "The heal could not be applied.", actionName)
                    } else if job.state == "unchanged" {
                        NSSound.beep()
                    }
                })
            // Edit ▸ Fade weighs the heal by the pixels it changed; a heal onto
            // a new layer has no state before it to fade toward.
            if backdrop == nil { job.fade = FadeHint(kind: .stroke, layer: layer) }
        } catch {
            clearHealPin(pin)
            presentHealFailure(error.localizedDescription, actionName)
        }
    }

    private func startSpotHealJob(_ data: UnsafePointer<UInt8>, base: RasterDocument,
                                  layer: Int, actionName: String) {
        guard let document else { return }
        let options = byteStrokeCapture?.options ?? ToolOptionsStore.shared.spotHeal
        let strength = Self.healStrength(options)
        // The spot tool's drag only ghosts over the unchanged picture, so
        // there is nothing to pin.
        guard let handoff = handOffHealStroke(data, base: base, layer: layer,
            actionName: actionName, pinPreview: false) else { return }
        let steps = handoff.steps
        let pixels = handoff.pixels
        // Latched with the gesture, not read again on the worker: the store
        // can move while the job runs, and the footprint was drawn against
        // this choice.
        let choice = strokePlate?.choice
            ?? RetouchSamplingChoice(
                mode: options.sample, ignoreAdjustments: options.ignoreAdjustments)
        let type = options.spotHealType ?? .contentAware
        // The new layer's backdrop plate, copied out of the handle now — the
        // worker runs off the main thread and must not reach back for it.
        let backdrop = strokePlate?.backdrop.flatMap { image in
            image.withPlateBytes { Array(UnsafeBufferPointer(start: $0, count: pixels.count)) }
        }
        let destination = strokeDestination ?? base
        do {
            let job = try DocumentJobs.start(document: document, layer: layer, title: actionName,
                scratch: UInt64(pixels.count), record: steps, showProgress: true,
                rasterizesTarget: backdrop == nil, compute: { _, control in
                    let source = destination
                    if let backdrop {
                        return try source.spotHealToNewLayerJob(
                            layer, overlay: pixels, strength: strength, ring: 0, seed: 0,
                            type: type, plate: backdrop, name: Self.retouchLayerName,
                            control: control)
                    }
                    return try source.spotHealJob(layer, overlay: pixels, strength: strength,
                        ring: 0, seed: 0, type: type, sample: choice.mode,
                        ignoreAdjustments: choice.ignoreAdjustments, control: control)
                }, completion: { [weak self] job in
                    if job.state == "failed" || job.state == "stale" {
                        self?.presentHealFailure(job.message ?? "The heal could not be applied.", actionName)
                    }
                })
            if backdrop == nil { job.fade = FadeHint(kind: .stroke, layer: layer) }
        } catch { presentHealFailure(error.localizedDescription, actionName) }
    }

    /// The name a retouch layer is created with. One constant, because the
    /// UI, the agent handlers and the core's own default must agree or a
    /// replayed Action would name a different layer than the gesture did.
    static let retouchLayerName = "Retouch"

    /// The heal's strength: the tool's Opacity, which is what Opacity means
    /// for these two tools (§0.4 — Flow is forced to 1 at stroke begin so
    /// every dab deposits full coverage for the solve's domain to be, and
    /// the strength moved here).
    private static func healStrength(_ options: PaintToolOptions) -> Double {
        min(max(options.opacity, 0), 100) / 100
    }

    /// Explains a refused heal instead of beeping at it: the core's refusals
    /// are limits with numbers in them and an instruction ("heal it in
    /// shorter strokes"), and a beep throws that away. Mouse-up is over by
    /// the time this runs, so a sheet is safe — nothing is mid-drag.
    private func presentHealFailure(_ message: String, _ actionName: String) {
        let alert = NSAlert()
        // The core's messages are lowercase sentences meant to follow a
        // "… failed: " lead-in; an alert headline starts one.
        alert.messageText = message.prefix(1).uppercased() + message.dropFirst()
        alert.informativeText =
            "\(actionName) was rolled back: the layer is unchanged and no undo step was "
            + "added."
        if let window = view.window {
            alert.beginSheetModal(for: window)
        } else {
            alert.runModal()
        }
    }
}
