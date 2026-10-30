import AppKit

extension EditorViewController {
    /// Edit ▸ Auto-Align Layers…: asks for the projection, Vignette
    /// Removal and Geometric Distortion (`AutoAlignSheetController`), then
    /// `commitAutoAlign`.
    /// MCP twin `auto_align_layers` (AgentServer+Stacks.swift).
    @objc func autoAlignLayers(_ sender: Any?) {
        guard canAutoAlignLayers else {
            NSSound.beep()
            return
        }
        presentAsSheet(AutoAlignSheetController { [weak self] projection, vignette, distortion in
            self?.commitAutoAlign(registration: projection, vignette: vignette,
                                  distortion: distortion)
        })
    }

    /// The sheet's Apply: registers the selected raster layers with
    /// `registration` to a reference — the lowest selected layer, or the one
    /// selected layer that is position-locked — warps each into place, and
    /// grows the canvas to the union of the reference and the aligned layers
    /// (Reveal All's resize, the selection carried with it), as ONE undo step
    /// and one cancellable job (`AutoAlignWork`, AutoAlign.swift). The
    /// reference, the layer order and the selected layers do not change. A
    /// layer no registration is vouched for (`AutoAlign.register`) is left
    /// untouched and named afterwards. With `vignette`, Lens Correction's
    /// Vignette Removal runs first and the layers are reprojected
    /// (AutoAlignVignette.swift); with `distortion`, Geometric Distortion's
    /// coefficient is fitted and the reprojection undoes it
    /// (AutoAlignDistortion.swift).
    func commitAutoAlign(registration: AutoAlign.Registration, vignette: Bool = false,
                         distortion: Bool = false) {
        guard let document, document.doc != nil, commitPendingSessions(),
              let source = document.doc else {
            NSSound.beep()
            return
        }
        let name = AutoAlign.title
        let record: [ActionStep] = .autoAlignLayers(projection: registration.projectionName,
                                                     vignette: vignette, distortion: distortion)
        do {
            let request = try AutoAlign.request(
                in: source, layers: document.selectedLayerIndices, reference: nil,
                registration: registration, sampler: DR_FILTER_CATMULL_ROM, growsCanvas: true,
                vignette: vignette, distortion: distortion)
            let work = try AutoAlignWork(source: source, request: request)
            let (prepared, outcome) = try work.wait(in: document, title: name)
            if let prepared {
                document.preparedEdit = prepared
                guard document.admitsPublication(prepared.document, record: record)
                else { return }
                let carried = outcome.canvas.flatMap { rect in
                    document.selectionCarried(
                        canvasWidth: Int(rect.width), height: Int(rect.height),
                        originX: Int(-rect.minX), originY: Int(-rect.minY))
                }
                withUndoGroup(document.undoManager) {
                    document.applyEdit(
                        name, record: record, changesCanvasGeometry: outcome.canvas != nil,
                        resultingSelection: carried
                    ) { _ in prepared.document }
                }
            } else {
                SessionCommitRefusal.beep("\(name) moved no layer and nothing changed.")
            }
            reportUnregistered(outcome)
        } catch is JobCancelled {
            SessionCommitRefusal.note(JobCancelled.message)
        } catch {
            document.presentError(error)
        }
    }

    /// The layers left unregistered, named in one informational alert.
    private func reportUnregistered(_ outcome: AutoAlign.Outcome) {
        guard !outcome.unregistered.isEmpty else { return }
        let alert = NSAlert()
        alert.alertStyle = .informational
        alert.messageText = outcome.unregistered.count == 1
            ? "One Layer Was Not Aligned" : "\(outcome.unregistered.count) Layers Were Not Aligned"
        let lines = outcome.unregistered.map { skipped -> String in
            // A core refusal arrives as a full sentence; the host's reasons do not.
            let reason = skipped.reason.hasSuffix(".")
                ? String(skipped.reason.dropLast()) : skipped.reason
            return "“\(skipped.name)”: \(reason)."
        }
        alert.informativeText = lines.joined(separator: "\n") + "\nEach was left where it was."
        alert.runModal()
    }

    /// Two or more selected entries, each a raster layer. The finer rules (a
    /// linked layer, a text or shape layer, two locked layers) are
    /// `AutoAlign.request`'s and answer with a sentence naming the layer.
    var canAutoAlignLayers: Bool {
        guard let document, let doc = document.doc else { return false }
        let selected = document.selectedLayerIndices
        return selected.count >= 2 && selected.allSatisfy {
            doc.layerInfo($0)?.isGroup == false && !doc.layerIsAdjustment($0)
        }
    }
}
