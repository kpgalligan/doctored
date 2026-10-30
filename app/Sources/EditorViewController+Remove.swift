import AppKit

/// The Remove tool's commit: the painted stroke becomes the hole of
/// one fill — the local model's (Mode: Model) or a Content-Aware Fill (Mode:
/// Content-Aware) — run as one nonmodal job and one undo step. The stroke's
/// coverage, the plan and the job are `RemoveStroke.start`'s, the same call
/// `remove_stroke` makes (AgentServer+ContentAwareTools.removeStroke).
extension EditorViewController {
    @objc func selectRemoveTool(_ sender: Any?) { selectTool(.remove) }

    /// Mouse-up of a Remove drag. With "Remove after each stroke" off the stroke is held
    /// (no guards, no job) and the bar's ✓ / ✕ refreshed; a stroke made with it on while
    /// strokes are held is held too and all of them apply together; otherwise the stroke
    /// commits alone. The 65th held stroke beeps and is not held.
    func removeStrokeCommitted(_ result: RemoveSession.Result) {
        let eachStroke = ToolOptionsStore.shared.removeTool.removeAfterEachStroke
        guard !eachStroke || canvas.removeSession.hasPending else {
            commitRemove([result])
            return
        }
        guard canvas.removeSession.hold(result) else {
            NSSound.beep()
            return
        }
        canvas.needsDisplay = true
        if eachStroke {
            applyPendingRemove()
        } else {
            optionsBar.refreshValues()
        }
    }

    /// ✓ / Return: every held stroke as one job and one undo step. Any refusal keeps them all
    /// held (and ✓ enabled), so fixing the cause and pressing ✓ again loses nothing.
    func applyPendingRemove() {
        guard canvas.removeSession.hasPending else { return }
        if commitRemove(canvas.removeSession.pending) {
            canvas.removeSession.discardPending()
            canvas.needsDisplay = true
        }
        optionsBar.refreshValues()
    }

    /// ✕ / Escape: the held strokes go, and nothing is recorded or undoable.
    func cancelPendingRemove() {
        guard canvas.removeSession.hasPending else { return }
        canvas.removeSession.discardPending()
        canvas.needsDisplay = true
        optionsBar.refreshValues()
    }

    /// The strokes' one fill. The guards are the Patch commit's, in its
    /// order: a busy worker beeps and leaves nothing behind; a channel, an
    /// adjustment layer, a group and a lock each refuse in their own words;
    /// and a described layer is asked about only when the fill writes into it
    /// (a new-layer fill leaves the description alone). True only when
    /// `RemoveStroke.start` returned a job.
    @discardableResult
    private func commitRemove(_ strokes: [RemoveSession.Result]) -> Bool {
        guard !strokes.isEmpty else { return false }
        if let document, DocumentJobs.refusesGesture(on: document, workerBound: true) {
            NSSound.beep()
            return false
        }
        guard !refuseChannelTargetEdit(), !refuseAdjustmentPixelEdit(), !refuseGroupPixelEdit(),
              let document,
              !refuseLockedEdit(layer: document.activeLayerIndex, kind: DR_EDIT_PIXELS)
        else { return false }
        let layer = document.activeLayerIndex
        let options = ToolOptionsStore.shared.remove
        let mode = ToolOptionsStore.shared.removeTool.mode
        // Model mode's own refusals come before anything is asked: a missing
        // model offers Settings ▸ Models, a 32-bit or CMYK document says
        // which Mode runs there.
        var model: ModelCatalogEntry?
        if mode == .model {
            guard let ready = ModelGate.ready(RemoveModelRun.capability) else {
                ModelGate.offerSettings(ModelGate.notInstalled(RemoveModelRun.capability))
                return false
            }
            if let source = document.doc, let reason = RemoveModelRun.refusal(source) {
                presentRemoveFailure(reason)
                return false
            }
            model = ready.entry
        }
        let output: RetouchOutput = options.outputToNewLayer ? .newLayer : .layer
        // The checkbox is All Layers or the layer's own pixels; the tool has
        // no Ignore Adjustment Layers control, so it is never set.
        let choice = RetouchSamplingChoice(
            mode: options.sample == .all ? .all : .current, ignoreAdjustments: false)
        do {
            if output == .layer {
                let capture = try ImageSourceCapture(document)
                guard document.confirmRasterize(layer: layer) else { return false }
                try capture.requireCurrent(document)
            }
            var arguments: [String: Any] = [
                "layer": ActionArgs.activeLayer, "output": output.rawValue, "seed": 0,
                "mode": mode.rawValue,
            ]
            // One stroke records as before; several record `strokes`.
            if strokes.count == 1, let only = strokes.first {
                arguments["points"] = ActionArgs.points(only.points)
                arguments["size"] = ActionArgs.number(only.size)
            } else {
                arguments["strokes"] = strokes.map {
                    ["points": ActionArgs.points($0.points), "size": ActionArgs.number($0.size)]
                }
            }
            arguments.merge(ActionArgs.sampling(choice.mode, choice.ignoreAdjustments)) { _, new in new }
            let request = RemoveRequest(
                strokes: strokes, choice: choice, output: output, seed: 0, mode: mode)
            let note = model.map { ModelGate.note("Remove tool", $0) } ?? "Remove tool"
            let job = try RemoveStroke.start(
                request, document: document, layer: layer, selection: canvas.selection,
                record: .step("remove_stroke", arguments, note: note),
                showProgress: true)
            let prior = job.completion
            job.completion = { [weak self] completed in
                prior?(completed)
                if ["failed", "stale"].contains(completed.state), let message = completed.message {
                    self?.presentRemoveFailure(message)
                } else if completed.state == "unchanged" {
                    NSSound.beep()
                }
            }
            return true
        } catch let error as DoctoredCoreError where error.message == RemoveStroke.coversNothing {
            // A stroke wholly outside the selection or the canvas is a brush
            // that painted nothing: a beep, not a sheet.
            NSSound.beep()
        } catch let error as DoctoredCoreError
                    where error.message == ModelGate.notInstalled(RemoveModelRun.capability) {
            ModelGate.offerSettings(error.message)
        } catch {
            presentRemoveFailure(error.localizedDescription)
        }
        return false
    }

    /// A refused Remove says why: the fill's refusals are limits with numbers
    /// in them and an instruction, and a beep throws that away. Mouse-up is
    /// over by the time this runs, so a sheet is safe.
    private func presentRemoveFailure(_ message: String) {
        let alert = NSAlert()
        alert.messageText = "Remove could not fill the painted region"
        // The core's sentences are lowercase, written to follow a lead-in.
        alert.informativeText = message.prefix(1).uppercased() + message.dropFirst()
        if let window = view.window {
            alert.beginSheetModal(for: window)
        } else {
            alert.runModal()
        }
    }
}
