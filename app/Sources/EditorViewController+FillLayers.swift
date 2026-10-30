import AppKit

/// Layer ▸ New Fill Layer ▸ Solid Color… / Gradient… / Pattern…, and
/// the Layers panel's double-click on a fill layer or Layer ▸ Layer Content
/// Options… (FillLayer.swift). Both open
/// `FillLayerSheetController`; the commits here are one undo step each and
/// record `add_fill_layer` / `edit_fill_layer`, their MCP mirrors
/// (AgentServer+FillLayers.swift). They work at 8, 16 and 32 bits: the layer
/// is rendered by the core at the document's depth. In a CMYK document (#547)
/// the dialog works on the colours' sRGB appearance and the commit converts
/// them to ink first (`inkedArtwork`: the swatches' stored inks, and a
/// re-edited layer's own inks — a gradient stop's by its unchanged hex — kept
/// exactly), then records and renders that. A pattern fill works there too:
/// its tile's texels separate to ink at render, as a Pattern
/// Overlay's do.
extension EditorViewController {
    /// Layer ▸ New Fill Layer ▸ Solid Color…: opens on the foreground colour.
    @objc func newSolidColorFillLayer(_ sender: Any?) {
        guard let doc = fillLayerSource() else { return }
        let color = ArtworkColor.rgba(paintColor) ?? [0, 0, 0, 1]
        presentNewFillLayer(FillLayerPayload(color: color, canvas: (doc.width, doc.height)))
    }

    /// Layer ▸ New Fill Layer ▸ Gradient…: opens on Foreground → Background,
    /// linear, bottom to top, spanning the canvas (Align with Layer off).
    @objc func newGradientFillLayer(_ sender: Any?) {
        guard let doc = fillLayerSource() else { return }
        var gradient = GradientFill()
        gradient.alignWithLayer = false
        gradient.stops = [paintColor, backgroundColor].enumerated().map { index, color in
            GradientStop(
                position: Double(index), color: LayerStyleColor.hex(color),
                opacity: Double(color.alphaComponent))
        }
        presentNewFillLayer(FillLayerPayload(gradient: gradient, canvas: (doc.width, doc.height)))
    }

    /// Layer ▸ New Fill Layer ▸ Pattern…: opens on the Checkerboard
    /// preset, linked, at phase (0, 0).
    @objc func newPatternFillLayer(_ sender: Any?) {
        guard let doc = fillLayerSource() else { return }
        let pattern = PatternFill(pattern: PatternRef(preset: "checkerboard"))
        presentNewFillLayer(FillLayerPayload(pattern: pattern, canvas: (doc.width, doc.height)))
    }

    /// Layer ▸ Layer Content Options…: the active fill layer's dialog, as a
    /// double-click in the Layers panel opens it.
    @objc func layerContentOptions(_ sender: Any?) {
        guard let document = document else { return }
        editFillLayer(document.activeLayerIndex)
    }

    /// The dialog title of a fill of `payload`'s kind, `prefix` before it.
    private static func fillTitle(_ payload: FillLayerPayload, prefix: String) -> String {
        switch payload.fill {
        case FillLayerPayload.gradientKind: return prefix + "Gradient Fill"
        case FillLayerPayload.patternKind: return prefix + "Pattern Fill"
        default: return prefix + "Solid Color Fill"
        }
    }

    /// The document a fill-layer command starts from, with every pending
    /// canvas session committed first; nil (after a beep) when there is none
    /// or Quick Mask is on.
    private func fillLayerSource() -> RasterDocument? {
        guard let document = document, !document.quickMaskActive, commitPendingSessions(),
              let doc = document.doc
        else {
            NSSound.beep()
            return nil
        }
        return doc
    }

    private func presentNewFillLayer(_ payload: FillLayerPayload) {
        let title = Self.fillTitle(payload, prefix: "New ") + " Layer"
        // A new layer is made at reference point (0, 0).
        presentAsSheet(FillLayerSheetController(
            title: title, payload: payload, reference: (0, 0)
        ) { [weak self] in
            self?.commitNewFillLayer($0)
        })
    }

    /// The new-layer dialog's Apply: the fill layer above the active one and,
    /// with a selection, a mask from it — together one undo step. Recorded as
    /// the steps they are, `add_fill_layer` then `add_layer_mask`; an aligned
    /// gradient then re-renders over its new mask's box, recorded as
    /// `edit_fill_layer`, in the same undo step.
    private func commitNewFillLayer(_ payload: FillLayerPayload) {
        guard let document = document, let doc = document.doc, !document.quickMaskActive else {
            NSSound.beep()
            return
        }
        // The dialog was sized for the canvas it opened on; the commit's own
        // canvas is what the layer covers (`nativeArtwork` fits it).
        let below = document.activeLayerIndex
        let name = FillLayer.layerName(for: payload)
        // Where the new entry lands is the CORE's answer, at both depths.
        let landing = LayerInsertPosition.above.landingIndex(in: doc, anchor: below)
        let masks = canvas.selection != nil
        guard let committed = inkedFill(payload, in: doc, replacing: nil) else { return }
        document.withUndoGroupDiscardingEmpty {
            let before = document.doc
            document.applyEdit("New \(name) Layer", record: .addFillLayer(committed)) {
                try $0.addingArtworkLayer(above: below, .fill(committed), anchor: .zero, name: name)
            }
            guard document.doc !== before else { return }
            setActiveLayer(min(landing, document.doc.layerCount - 1))
            guard masks else { return }
            addLayerMaskFromSelection(nil)
            guard committed.gradient?.alignWithLayer == true else { return }
            let idx = document.activeLayerIndex
            document.applyEdit(
                "New \(name) Layer",
                record: .editFillLayer(committed, layer: document.doc.layerRef(idx))
            ) {
                try $0.rerenderingArtworkLayer(idx, .fill(committed), anchor: .zero)
            }
        }
    }

    /// Double-clicking a FILL layer in the Layers panel: the dialog reopens
    /// on the layer's own colour or gradient. Apply re-renders the layer as
    /// one undo step; an unchanged dialog commits nothing.
    func editFillLayer(_ idx: Int) {
        guard let document = document, let doc = fillLayerSource() else { return }
        guard let payload = doc.displayedFillPayload(idx) else {
            NSSound.beep()
            return
        }
        // Editing a layer makes it the active one, as the shape re-edit does.
        if document.activeLayerIndex != idx { setActiveLayer(idx) }
        let title = Self.fillTitle(payload, prefix: "")
        presentAsSheet(FillLayerSheetController(
            title: title, payload: payload, reference: doc.layerReference(idx)
        ) { [weak self] in
            self?.commitFillLayerEdit(idx, $0, opened: payload, source: doc)
        })
    }

    private func commitFillLayerEdit(
        _ idx: Int, _ payload: FillLayerPayload, opened: FillLayerPayload, source: RasterDocument
    ) {
        guard let document = document else { return }
        // A document-modal sheet does not stop an agent edit behind it: a
        // different handle means `idx` may name another layer now.
        guard document.doc === source else {
            let alert = NSAlert()
            alert.messageText = "The document changed while this dialog was open."
            alert.informativeText = "Reopen the fill layer to edit it as it is now."
            if let window = view.window { alert.beginSheetModal(for: window) } else { alert.runModal() }
            return
        }
        guard payload != opened else { return }
        // A re-render REWRITES the layer's pixels: the pixel locks answer.
        guard !refuseLockedEdit(layer: idx, kind: DR_EDIT_PIXELS) else { return }
        guard let committed = inkedFill(payload, in: source, replacing: idx) else { return }
        document.applyEdit(
            "Edit Fill Layer", record: .editFillLayer(committed, layer: source.layerRef(idx))
        ) {
            try $0.rerenderingArtworkLayer(idx, .fill(committed), anchor: .zero)
        }
    }

    /// `payload` in `doc`'s ink (`inkedArtwork`), the swatches' stored inks matched by colour
    /// and, for a gradient stop, by its `#rrggbb`. nil, with the error presented, when the colours
    /// cannot be converted.
    private func inkedFill(_ payload: FillLayerPayload, in doc: RasterDocument,
                           replacing: Int?) -> FillLayerPayload? {
        let swatches = CmykInkMatch.swatches(paint: paintColor, background: backgroundColor)
        let stops = swatches.map {
            CmykInkMatch(srgb: CmykArtworkHex.unit(CmykArtworkHex.six($0.srgb)), ink: $0.ink)
        }
        do {
            guard case let .fill(inked) = try doc.inkedArtwork(.fill(payload), replacing: replacing,
                                                               swatches: swatches + stops) else { return payload }
            return inked
        } catch {
            _ = document?.presentError(error)
            return nil
        }
    }

    /// The New Fill Layer items: live whenever there is a document outside
    /// Quick Mask; Layer Content Options…
    /// when the active layer is a fill layer. nil for any other item.
    func validateFillLayerItem(_ item: NSValidatedUserInterfaceItem) -> Bool? {
        switch item.action {
        case #selector(newSolidColorFillLayer(_:)), #selector(newGradientFillLayer(_:)),
             #selector(newPatternFillLayer(_:)):
            return document?.doc != nil && document?.quickMaskActive == false
        case #selector(layerContentOptions(_:)):
            guard let document, let doc = document.doc else { return false }
            return doc.fillPayload(document.activeLayerIndex) != nil
        default:
            return nil
        }
    }
}
