import AppKit

// The text tool's layer logic: clicking to open or re-open a text layer, the
// layers panel's double-click, and the commit that renders a session into a
// re-editable TEXT layer (pixels rendered by TextLayer through the layer's
// description, the description in the layer's meta, the anchor rule in
// DescribedLayer.swift). The canvas owns the on-canvas NSTextView session;
// this file owns the document side. Mirrored for the agent by
// `add_text_layer` and `edit_text_layer` (AgentServer+Text.swift).
extension EditorViewController {
    /// A text-tool click: re-open the topmost VISIBLE text layer under the
    /// point, or start a new text entry there.
    func textClicked(_ point: CGPoint) {
        // A channel is the one target that survives picking the text tool
        // (EditorViewController+Channels.refuseChannelTargetEdit): text
        // cannot be written into a channel, so the session refuses rather
        // than editing the picture while every indicator names the channel.
        guard !refuseChannelTargetEdit() else { return }
        canvas.textSessionDisplayInk = textSessionDisplayInk()
        guard let doc = document?.doc, let idx = topmostTextLayer(at: point, in: doc) else {
            canvas.beginTextSession(at: point)
            return
        }
        // A click puts the caret where it landed, so nothing is preselected.
        openTextSession(layer: idx, selectAll: false)
    }

    /// Double-clicking a TEXT layer in the layers panel: switch to the text
    /// tool and reopen the layer's description with the whole string
    /// selected, so typing replaces it and the options bar exposes the font,
    /// size, color, alignment and typography. The layer needn't be under the
    /// cursor or even visible — the panel already said which one.
    func editTextLayer(_ idx: Int) {
        guard let doc = document?.doc, doc.textPayload(idx) != nil else {
            NSSound.beep()
            return
        }
        // A click that ends an open session only ends it — the rule the
        // canvas follows too — because committing may insert a layer and
        // renumber everything above it, `idx` included.
        if canvas.hasActiveTextSession {
            canvas.commitTextSession()
            return
        }
        // An open shape session only commits too — editShapeLayer's rule —
        // so a refused commit never has a text session opened on top of it.
        if shapeEditSession != nil {
            commitShapeEditSession()
            return
        }
        // The session belongs to the text tool; entering it also drops any
        // mask paint target and swaps the options bar over.
        selectTool(.text)
        openTextSession(layer: idx, selectAll: true)
    }

    /// Opens the on-canvas editor on text layer `idx`, restoring its
    /// description into the options bar and the session. Shared by the
    /// text-tool click and the layers panel's double-click.
    ///
    /// A rotated or scaled layer opens IN PLACE: the editor
    /// sits at the layer's anchor (the layout origin's canvas position),
    /// turned and scaled by the layer's map (`TextSessionPlacement` — the
    /// baseline angle and √det, so shear and anisotropic scale are
    /// approximated), and the commit re-renders through the layer's own
    /// transform. A mirrored layer, or one whose editor would sit entirely
    /// off the canvas, opens upright at the canvas's clamp instead. The
    /// editor's width is the layer's stored box; a legacy layer with no
    /// stored width lets the canvas default it (the pre-change rule,
    /// `TextLayer.legacyBoxWidth`). Point text gets a viewport wide enough
    /// for its longest line: upright, at least the legacy width, so a longer
    /// line still shows unwrapped; transformed, exactly its natural width,
    /// refitted as it is typed so it never wraps (the commit lays point
    /// text out unwrapped).
    func openTextSession(layer idx: Int, selectAll: Bool) {
        // The other door into a session (the layers panel's double-click)
        // takes the same refusal as the canvas click.
        guard !refuseChannelTargetEdit() else { return }
        canvas.textSessionDisplayInk = textSessionDisplayInk()
        // A CMYK layer opens on its sRGB appearance (#547, `displayedTextPayload`).
        guard let doc = document?.doc,
              let payload = doc.displayedTextPayload(idx), let anchor = doc.describedAnchor(idx)
        else {
            NSSound.beep()
            return
        }
        // Editing a layer makes it the active one (the commit replaces its
        // content, and the panel should show what is being edited).
        // setActiveLayer is the one path that moves it: it no-ops when the
        // layer is already active and carries the Channels panel and the
        // canvas's mask display with it when it is not.
        setActiveLayer(idx)
        let placement = TextSessionPlacement(map: payload.transform)
        let width: CGFloat?
        switch payload.box {
        case let .width(w):
            width = CGFloat(w)
        case .point:
            let natural = (TextLayer.layout(payload)?.rect.width ?? 0).rounded(.up) + 1
            width = placement.isIdentity
                ? max(TextLayer.legacyBoxWidth(canvasWidth: CGFloat(doc.width), anchorX: anchor.x),
                      natural)
                : natural
        case .unspecified:
            width = nil
        }
        // The options bar reflects what is being edited, and the session
        // draws with those very parameters.
        applyTextOptions(payload)
        // Hide the layer's own raster underneath the session, or the old
        // glyphs ghost behind every edit to the string. An already-hidden
        // layer has nothing to hide: the pure op returns nil and the session
        // runs over the unmodified canvas, which is correct.
        canvas.previewImage = (try? doc.withLayerVisible(idx, false)).flatMap { PreparedDocumentEdit($0).preview }
        canvas.beginTextSession(
            at: anchor, width: width, string: payload.string, editingLayer: idx,
            selectAll: selectAll, placement: placement, pointText: payload.box == .point)
        canvas.restoreArtworkText(payload)
    }

    /// The Text session's preview ink (#547 phase 6b): on a Grayscale document each run is shown
    /// as its gray (`grayDisplayColor`, the last colour memoized), so the session previews what
    /// the commit renders; nil elsewhere.
    private func textSessionDisplayInk() -> ((NSColor) -> NSColor)? {
        guard document?.doc?.isGray == true else { return nil }
        var last: (color: NSColor, gray: NSColor)?
        return { [weak document] color in
            if let last, last.color == color { return last.gray }
            let gray = document?.doc?.grayDisplayColor(color) ?? color
            last = (color, gray)
            return gray
        }
    }

    /// The topmost visible layer that carries a text description and whose
    /// extent contains `point` (image pixel coordinates). Plain raster layers
    /// above it do not block the hit. A transformed layer hit-tests on its
    /// source rect's canvas quad, so a click in the corner of its bounding
    /// box that the glyph block does not cover passes through; a layer whose
    /// quad cannot be computed keeps the bounding-box answer.
    func topmostTextLayer(at point: CGPoint, in doc: RasterDocument) -> Int? {
        for idx in stride(from: doc.layerCount - 1, through: 0, by: -1) {
            guard let info = doc.layerInfo(idx), info.visible else { continue }
            let rect = CGRect(
                x: CGFloat(info.offsetX), y: CGFloat(info.offsetY),
                width: CGFloat(info.width), height: CGFloat(info.height))
            guard rect.contains(point), doc.textPayload(idx) != nil else { continue }
            if let description = doc.layerDescription(idx), !description.transform.isIdentity,
               let source = description.sourceRect(), let anchor = doc.describedAnchor(idx),
               !TextSessionPlacement.convexQuad(
                   description.transform.quad(of: source, anchor: anchor), contains: point) {
                continue
            }
            return idx
        }
        return nil
    }

    /// Commits a text session: a NEW text layer above the active one, or the
    /// re-render of the layer the session was editing. Both are one pure
    /// document op (DescribedLayer.swift), so each is one undo step.
    /// `origin` is the session's origin — the layout origin's canvas
    /// position, the anchor. Answers false when the edit was refused and
    /// the canvas must keep the session open with the typed text: the
    /// core's sentence is thrown through `applyEdit`, which presents it.
    @discardableResult
    func commitTextLayer(_ payload: TextLayerPayload, origin: CGPoint, editing: Int?) -> Bool {
        guard let document = document, let doc = document.doc else {
            NSSound.beep()
            return false
        }
        // The session's numbers in the document's ink, before
        // the no-op compare below reads them against the layer's.
        let payload = payload.inInk(doc.artworkInk)
        let name = TextLayer.layerName(for: payload.string)

        // A CMYK layer compares and commits on its sRGB appearance; the commit converts the
        // colours to ink first (`inkedArtwork`, the swatches' stored inks and the layer's own
        // among the matches), and records and renders that (#547).
        let swatches = CmykInkMatch.swatches(paint: paintColor, background: backgroundColor)
        if let idx = editing, let info = doc.layerInfo(idx), let old = doc.displayedTextPayload(idx) {
            // The editor edits the string, never the map: the layer's own
            // transform re-attaches to the edited description.
            var payload = payload
            payload.transform = old.transform
            // A point-text layer stays point text — the editor's width was
            // only its viewport (D11). A stored box wider than the editor's
            // cap (`TextLayer.maxBoxWidth` — a file from elsewhere) came
            // back AT the cap: that is the same box, not an edit.
            switch (old.box, payload.box) {
            case (.point, _):
                payload.box = .point
            case let (.width(stored), .width(session))
                where stored > TextLayer.maxBoxWidth && session == TextLayer.maxBoxWidth:
                payload.box = old.box
            default:
                break
            }
            // A re-edit never moves the origin: the editor is not draggable
            // (and opened at the layer's anchor, or at the canvas's upright
            // clamp of it), so the layer keeps its EXACT anchor. Re-placing
            // the editor's origin through the whole-pixel rule would round a
            // fractional anchor — one a transform composition left behind,
            // even under a map that has snapped back to the identity — and
            // register a phantom edit that nudges the glyphs by up to half a
            // pixel. `origin` is that anchor by construction (or its upright
            // clamp); it is only consulted should the layer's anchor have
            // become unrecoverable under the session.
            let anchor = doc.describedAnchor(idx)
                ?? DescribedLayer.placementAnchor(origin, transform: old.transform)
            // Opening a text layer and closing it unchanged (⌘Return, or a
            // tool switch) must not register an undo step or dirty the file.
            // A legacy layer's box is materialized only by a REAL edit, never
            // by an unchanged ⌘Return, so the comparison treats its
            // unspecified box as the session's; the session's payload carries
            // fraction [0, 0] by construction, and the anchor — the layer's
            // own — is re-stamped by the re-render unchanged.
            let comparable = old.box == .unspecified ? old.withBox(payload.box) : old
            guard comparable.withOriginFraction(.zero) != payload else { return true }
            // The name follows the text only while it still IS the text: a
            // name the user typed themselves survives the re-edit.
            let nameFollowsText = info.name == TextLayer.layerName(for: old.string)
            // A re-render REWRITES the layer's pixels, so it answers to the
            // same locks every other pixel path does. Without this the core
            // refused and `applyEdit` beeped with nothing said.
            guard !refuseLockedEdit(layer: idx, kind: DR_EDIT_PIXELS) else { return false }
            let beforeEdit = document.doc
            guard let committed = inkedText(payload, in: doc, replacing: idx, swatches: swatches) else {
                return false
            }
            // Whether the core THREW (its sentence already presented by
            // `applyEdit`), so the lock alert below never follows it.
            var threw = false
            document.applyEdit(
                "Edit Text Layer",
                record: .editTextLayer(committed, layer: document.doc?.layerRef(idx))
            ) { doc in
                let rendered: RasterDocument?
                do {
                    rendered = try doc.rerenderingArtworkLayer(idx, .text(committed), anchor: anchor)
                } catch {
                    threw = true
                    throw error
                }
                guard let described = rendered else { return nil }
                guard nameFollowsText else { return described }
                return try described.withLayerName(idx, name) ?? described
            }
            guard document.doc !== beforeEdit else {
                // A TRANSPARENCY lock refuses a re-layout that resizes the
                // raster, and only then — so it is named here, after the
                // fact, instead of leaving `applyEdit`'s bare beep.
                if !threw { refuseLockedRerender(layer: idx) }
                return false
            }
            // The active layer is unchanged, so the change notification alone
            // refreshes the panel, the status bar and the layer boundary.
            return true
        }

        let anchor = DescribedLayer.placementAnchor(origin, transform: .identity)
        let below = document.activeLayerIndex
        let before = document.doc
        // Where the new entry lands is the CORE's answer, the SAME at both
        // depths since 12E-2b: above a GROUP it goes above the whole subtree,
        // so `below + 1` would select the wrong row (§4.5's one definition).
        let landing = before.map { LayerInsertPosition.above.landingIndex(in: $0, anchor: below) }
            ?? below + 1
        guard let committed = inkedText(payload, in: doc, replacing: nil, swatches: swatches) else {
            return false
        }
        document.applyEdit(
            "Add Text Layer", record: .addTextLayer(committed, at: anchor)
        ) {
            try $0.addingArtworkLayer(above: below, .text(committed), anchor: anchor, name: name)
        }
        guard document.doc !== before else { return false }
        // The active layer moves to the new text layer; any mask paint target
        // goes with it.
        setActiveLayer(min(landing, document.doc.layerCount - 1))
        return true
    }

    /// `payload` in `doc`'s ink (`inkedArtwork`); nil, with the error presented, when the colours
    /// cannot be converted. Any other document's payload as it is.
    private func inkedText(_ payload: TextLayerPayload, in doc: RasterDocument, replacing: Int?,
                           swatches: [CmykInkMatch]) -> TextLayerPayload? {
        do {
            guard case let .text(inked) = try doc.inkedArtwork(.text(payload), replacing: replacing,
                                                               swatches: swatches) else { return payload }
            return inked
        } catch {
            _ = document?.presentError(error)
            return nil
        }
    }
}
