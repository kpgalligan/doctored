import AppKit

extension EditorViewController {
    /// Layer ▸ Stack Mode ▸ Entropy / Kurtosis / Majority / Maximum / Mean /
    /// Median / Minimum / Range / Skewness / Standard Deviation / Summation /
    /// Variance: the statistic across the selected raster
    /// layers as ONE new raster layer above the topmost of them, named after
    /// the mode, with the layers it read hidden — one undo step and one
    /// cancellable job (`StackModeWork`, StackMode.swift). The item's `tag`
    /// is the mode's position in `StackMode.allCases`.
    /// MCP twin `stack_mode` (AgentServer+Stacks.swift).
    @objc func stackMode(_ sender: Any?) {
        guard let item = sender as? NSMenuItem,
              StackMode.allCases.indices.contains(item.tag) else {
            NSSound.beep()
            return
        }
        let mode = StackMode.allCases[item.tag]
        runStack(mode: mode, title: StackModes.title, name: mode.title, aligning: false,
                 record: .stackMode(mode: mode.rawValue))
    }

    /// Layer ▸ Reduce Noise from Stack…: asks for Mean or Median, then
    /// `commitReduceNoise`. MCP twin `reduce_noise_from_stack`.
    @objc func showReduceNoiseFromStack(_ sender: Any?) {
        guard canStackMode else {
            NSSound.beep()
            return
        }
        presentAsSheet(ReduceNoiseStackSheetController { [weak self] mode in
            self?.commitReduceNoise(mode: mode)
        })
    }

    /// The sheet's Apply: auto-aligns the selected layers (the menu's own
    /// registration and sampler), then takes `mode` across the reference and
    /// every layer that aligned, as one undo step and one job. A layer that
    /// did not align is left out, untouched, and named afterwards.
    func commitReduceNoise(mode: StackMode) {
        runStack(mode: mode, title: StackModes.noiseTitle, name: StackModes.noiseLayerName,
                 aligning: true, record: .reduceNoiseFromStack(mode: mode.rawValue))
    }

    /// Both commands' one path: eligibility, the job, one grouped edit, then
    /// the new layer becomes the active one.
    private func runStack(mode: StackMode, title: String, name: String, aligning: Bool,
                          record: [ActionStep]) {
        guard let document, document.doc != nil, commitPendingSessions(),
              let source = document.doc else {
            NSSound.beep()
            return
        }
        do {
            let layers = try StackModes.members(in: source, layers: document.selectedLayerIndices,
                                                title: title)
            let align = aligning
                ? try AutoAlign.request(in: source, layers: layers, reference: nil,
                                        registration: .auto, sampler: DR_FILTER_CATMULL_ROM)
                : nil
            let work = try StackModeWork(source: source, layers: layers, mode: mode, name: name,
                                         align: align)
            let (prepared, outcome) = try work.wait(in: document, title: title)
            document.preparedEdit = prepared
            guard document.admitsPublication(prepared.document, record: record) else { return }
            withUndoGroup(document.undoManager) {
                document.applyEdit(title, record: record) { _ in prepared.document }
            }
            setActiveLayer(outcome.layer)
            reportExcluded(outcome)
        } catch is JobCancelled {
            SessionCommitRefusal.note(JobCancelled.message)
        } catch {
            document.presentError(error)
        }
    }

    /// The layers Reduce Noise left out, named in one informational alert.
    private func reportExcluded(_ outcome: StackModes.Outcome) {
        guard let excluded = outcome.align?.unregistered, !excluded.isEmpty else { return }
        let alert = NSAlert()
        alert.alertStyle = .informational
        alert.messageText = excluded.count == 1
            ? "One Layer Was Left Out" : "\(excluded.count) Layers Were Left Out"
        let lines = excluded.map { skipped -> String in
            // A core refusal arrives as a full sentence; the host's reasons do not.
            let reason = skipped.reason.hasSuffix(".")
                ? String(skipped.reason.dropLast()) : skipped.reason
            return "“\(skipped.name)”: \(reason)."
        }
        alert.informativeText = lines.joined(separator: "\n")
            + "\nEach was left where it was, visible, and is not part of “\(outcome.name)”."
        alert.runModal()
    }

    /// Two or more selected entries, each a raster layer: Auto-Align's own
    /// test. The finer rules answer with a sentence naming the layer.
    var canStackMode: Bool { canAutoAlignLayers }
}
