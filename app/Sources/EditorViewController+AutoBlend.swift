import AppKit

extension EditorViewController {
    /// Edit ▸ Auto-Blend Layers…: the sheet for the Panorama and Stack
    /// Images methods. Apply runs `commitAutoBlend`.
    /// MCP twin `auto_blend_layers` (AgentServer+AutoBlend.swift).
    @objc func showAutoBlendLayers(_ sender: Any?) {
        guard canAutoBlendLayers else {
            NSSound.beep()
            return
        }
        presentAsSheet(AutoBlendSheetController { [weak self] method, seamless, align, fill in
            self?.commitAutoBlend(method: method, seamless: seamless, align: align, fill: fill)
        })
    }

    /// The sheet's Apply: blends the selected raster layers by `method` as ONE
    /// undo step and one cancellable job (`AutoBlendWork`, AutoBlend.swift).
    /// Stack Images: each layer takes a mask revealing where it is sharpest,
    /// and `seamless` adds the merged layer above the top selected layer and
    /// anything clipped to it. Panorama: each layer takes a mask revealing its
    /// side of the seams, and `seamless` tone-matches the layers and writes the
    /// blend across the seams into them. `align` registers the layers first (a
    /// layer that cannot be registered refuses the whole blend by name);
    /// Panorama's alignment grows the canvas to the union, the selection
    /// carried as Auto-Align carries it. Otherwise the selection does not
    /// change. `fill` (Panorama only) adds the visible picture above the top
    /// selected layer with its transparent area filled by Content-Aware Fill
    /// (`AutoBlendFill`), in the same job and undo step.
    func commitAutoBlend(method: AutoBlend.Method, seamless: Bool, align: Bool, fill: Bool) {
        guard let document, document.doc != nil, commitPendingSessions(),
              let source = document.doc else {
            NSSound.beep()
            return
        }
        let name = AutoBlend.title
        let record: [ActionStep] = .autoBlendLayers(
            method: method.rawValue, seamless: seamless, align: align, fill: fill)
        do {
            let request = try AutoBlend.request(
                in: source, layers: document.selectedLayerIndices, method: method,
                seamless: seamless, align: align, fill: fill)
            let work = try AutoBlendWork(source: source, request: request)
            let (prepared, outcome) = try work.wait(in: document, title: name)
            document.preparedEdit = prepared
            guard document.admitsPublication(prepared.document, record: record) else { return }
            let canvas = outcome.aligned?.canvas
            let carried = canvas.flatMap { rect in
                document.selectionCarried(
                    canvasWidth: Int(rect.width), height: Int(rect.height),
                    originX: Int(-rect.minX), originY: Int(-rect.minY))
            }
            withUndoGroup(document.undoManager) {
                document.applyEdit(
                    name, record: record, changesCanvasGeometry: canvas != nil,
                    resultingSelection: carried
                ) { _ in prepared.document }
            }
        } catch is JobCancelled {
            SessionCommitRefusal.note(JobCancelled.message)
        } catch {
            document.presentError(error)
        }
    }

    /// Two or more selected entries, each a raster layer. The finer rules (a
    /// layer that already has a mask, a text or shape layer) are
    /// `AutoBlend.request`'s and answer with a sentence naming the layer.
    var canAutoBlendLayers: Bool {
        guard let document, let doc = document.doc else { return false }
        let selected = document.selectedLayerIndices
        return selected.count >= 2 && selected.allSatisfy {
            doc.layerInfo($0)?.isGroup == false && !doc.layerIsAdjustment($0)
        }
    }
}
