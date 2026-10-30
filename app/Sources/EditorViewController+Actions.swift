import AppKit

/// The editor's half of Actions: menu validation, transform and one-shot
/// record builders. Latched paint and retouch records live in
/// EditorViewController+StrokeRecording.swift.
///
/// The validation table follows `EditorViewController+Locks.swift`'s
/// contract: ONE method, returning `Bool?` — nil for "not one of mine" — so
/// the frozen `validateUserInterfaceItem` gains one early-out line rather
/// than a case per item.
extension EditorViewController {
    /// Enablement and titles for the phase's own menu items. nil when the
    /// item belongs to somebody else.
    ///
    /// Only ⌃F lives here; the four File ▸ Automate items and the command
    /// palette are on `AppDelegate`, because
    /// `validateUserInterfaceItem` returns false at its first line when
    /// there is no document and all five must work with none open.
    func validateActionsItem(_ item: NSValidatedUserInterfaceItem) -> Bool? {
        if item.action == #selector(showFade(_:)) { return validateFadeItem(item) }
        guard item.action == #selector(repeatLastFilter(_:)) else { return nil }
        let step = ActionRecorder.shared.lastRepeatable
        if let menuItem = item as? NSMenuItem {
            menuItem.title = Self.repeatTitle(for: step)
        }
        guard step != nil, document?.doc != nil else { return false }
        // ⌃F is a TEXT-EDITING hazard and is handled exactly as Edit ▸ Clear's
        // bare ⌫ is: NSTextView binds ⌃F to `moveForward:`, and a menu key
        // equivalent is resolved AHEAD of the first responder, so an enabled
        // item would swallow every Control-F typed into the options bar, a
        // layer-name field or the assistant's input. Disabled while a canvas
        // text session is open and while the field editor is first responder,
        // which is the union of those cases.
        //
        // The responder is read off the KEY window, not off this controller's
        // own: the command palette is a borderless `NSPanel` that becomes key
        // while the document window stays MAIN, so `NSApp.target(forAction:)`
        // still resolves ⌃F to this controller and this controller's own
        // first responder is still the canvas. Asking the editor's window
        // alone therefore enabled the item — and swallowed the ⌃F — while the
        // caret sat in the palette's query field, which is the exact trap the
        // guard exists for. Both windows are asked, so a field editor in
        // either one disables it.
        guard !canvas.hasActiveTextSession else { return false }
        for window in [NSApp.keyWindow, view.window] {
            if let responder = window?.firstResponder as? NSText, responder.isFieldEditor {
                return false
            }
        }
        return true
    }

    // MARK: - Transform record helper

    /// What a Free Transform commit records.
    ///
    /// A single-layer affine is a real `transform_layer` built from the
    /// session's own parameters (pivot, translation, degrees, the two
    /// scales) — the very values `transform_layer` reassembles into a
    /// `LayerTransform`, so the replay composes the identical matrix. A
    /// warped box becomes `distort_layer` with the warped quad in canvas
    /// coordinates. A SET has no twin at all: both tools transform one layer
    /// (a group and its links excepted), and there is no multi-layer form.
    func transformRecord(_ session: TransformSession, quad: [CGPoint]?) -> [ActionStep] {
        let sampler = Self.samplerAgentName(session.sampler)
        let truePerspective = quad.map {
            LayerTransform.parallelogramAffine(of: session.sourceRect, onto: $0) == nil
        } ?? false
        if !session.isSingleLayer
            || document?.doc?.layerIsGroup(session.primaryLayer) == true
            || document?.doc?.layerIsAdjustment(session.primaryLayer) == true {
            return .transformLayers(matrix: session.transform.matrix,
                pivot: session.transform.pivot, sourceRect: session.sourceRect, corners: quad, sampler: sampler,
                rasterizeArtwork: truePerspective)
        }
        // A true perspective the commit already confirmed replays with
        // `rasterize_artwork`, or the replay refuses the artwork.
        if let quad = quad {
            return .distortLayer(corners: quad, sampler: sampler,
                                 exact: session.precisionGesture != nil,
                                 rasterizeArtwork: truePerspective)
        }
        return .transformLayer(session.transform, sampler: sampler,
                               exact: session.precisionGesture != nil)
    }

    /// The resampling filter as `transform_layer` / `distort_layer` spell it.
    /// The catalog accepts two spellings for two of them; this writes the
    /// canonical one `AgentServer.transformSamplers` reports back.
    static func samplerAgentName(_ filter: DrResizeFilter) -> String {
        switch filter {
        case DR_FILTER_NEAREST: return "nearest"
        case DR_FILTER_BILINEAR: return "bilinear"
        case DR_FILTER_LANCZOS3: return "lanczos"
        default: return "bicubic"
        }
    }

    // MARK: - The rest of the editor's record builders

    /// A one-shot filter step with the ambient plane target written out.
    /// Every Filters-menu command goes through this rather than naming its
    /// target itself.
    func filterRecord(_ name: String, _ extra: [String: Any] = [:]) -> [ActionStep] {
        .filter(name, extra, target: paintTarget.layerEditAgentName(in: document?.doc))
    }

    /// The Fill tool's click, from the options it actually used. The colour
    /// is the AUTHORED one with the bar's opacity folded into its alpha,
    /// which is exactly what the tool itself passes to the core.
    func fillRecord(
        at point: CGPoint, color: NSColor, opacity: Double, options: FillToolOptions
    ) -> [ActionStep] {
        .fill(
            at: point,
            color: color.withAlphaComponent(color.alphaComponent * CGFloat(opacity)),
            tolerance: Int(options.tolerance.rounded()), contiguous: options.contiguous,
            target: paintTarget.layerEditAgentName(in: document?.doc),
            space: document?.nsColorSpace)
    }

    /// The `rotate` or `flip` step a whole-document geometry command is.
    /// `.resize` never reaches here — Image Size records its own two steps —
    /// so it answers with the visible placeholder rather than a wrong tool.
    static func geometryRecord(_ op: DocumentGeometry) -> [ActionStep] {
        switch op {
        case .rotate90: return .rotate(90)
        case .rotate180: return .rotate(180)
        case .rotate270: return .rotate(270)
        case .flipHorizontal: return .flip("horizontal")
        case .flipVertical: return .flip("vertical")
        case .resize: return .unrecorded(op.actionName)
        }
    }

    /// `add_layer_mask`'s `kind` vocabulary.
    static func layerMaskKindName(_ kind: DrMaskKind) -> String {
        switch kind {
        case DR_MASK_HIDE_ALL: return "hide_all"
        case DR_MASK_FROM_SELECTION: return "from_selection"
        default: return "reveal_all"
        }
    }

    /// What a finished Move gesture records: the RELATIVE `move_layers` step,
    /// at either size, given the gesture's own delta.
    ///
    /// The delta and not zero, and now for BOTH sizes: the drag's ticks have
    /// already moved the layers, so the offsets in the document include the
    /// drag — a step built from them would be right only for this document
    /// and this starting position. `delta` is the drag's applied total
    /// (`moveAppliedDelta`, which the snap correction is already folded
    /// into), so a drag that ended where it began records `move_layers`
    /// (0, 0) and the handler answers that as the no-op it is.
    ///
    /// Before Phase 12B the one-layer branch recorded an ABSOLUTE
    /// `set_layer_properties` offset from the post-drag position; the
    /// difference is exactly the bug that made a recorded Move snap instead
    /// of nudge (`MultiLayerEdit.moveRecord`).
    static func moveEndRecord(
        _ document: ImageDocument, delta: (x: Int, y: Int)
    ) -> [ActionStep] {
        guard let doc = document.doc else { return .notACommand }
        return moveRecord(doc, document.selectedLayerIndices, dx: delta.x, dy: delta.y)
    }
}
