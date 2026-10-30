import AppKit

// `copy` in layers mode asks the `layer_duplicate` family (`DocumentPrecision.layerCopyFamily`).
// `copy {mode: "auto"}` (what a recorded Edit ▸ Copy replays) resolves through
// `LayerClipboard.resolvingAutoCopy` — no pixel selection → layers, the selected layers when none
// is named; a selection → pixels — in the handler and at the top of `DocumentPrecision.coreRefusal`
// / `floatCoreRefusal`, so the gate asks the family of the mode it resolves to.

/// The MCP twins of Edit ▸ Copy with no pixel selection (layers) and
/// Edit ▸ Paste / Paste in Place of a held layer clipboard
/// (`EditorViewController+LayerClipboard.swift`), dispatched from `copy` /
/// `paste` on `mode` in `AgentServer+Clipboard.swift`, plus `get_clipboard`'s
/// `layers` field. The model is `LayerClipboard.swift`'s.
extension AgentServer {
    /// copy `mode: "layers"`: mirrors Edit ▸ Copy with no pixel selection —
    /// the given layers (subtrees included) into the layer clipboard, any
    /// pixel selection ignored.
    func copyLayersReply(_ a: [String: Any]) throws -> [String: Any] {
        let document = try target(a)
        guard document.doc != nil else { throw ToolError(message: "Document has no image") }
        let indices = try structuralIndices(a, document)
        return try LayerClipboard.copy(indices, from: document).json
    }

    /// paste in layers mode: mirrors Edit ▸ Paste (⌘V, `auto`, the default:
    /// the visible canvas of the view `view` names, view 1 without it,
    /// `PasteContext`), Paste with `centered`, and Paste in Place
    /// (`in_place`, `relative_in_place` across canvas sizes) of
    /// the held layer clipboard; the selection is kept. The Paste
    /// Profile Mismatch alert's choice is the `profile` argument here
    /// ("convert", the default, or "preserve", which refuses with
    /// `LayerPastePlan.preserveRefusal` before any edit); a `convert` boolean
    /// is accepted and ignored. A 32-bit float document takes any copy:
    /// the core promotes it and the reply's `rasterized`,
    /// `blend_reset` and `effect_blend_reset` say what
    /// that changed. One undo
    /// step; recorded as the LIVE `paste` step (the requested
    /// placement, `mode` only when the call passed "layers"), so a replay
    /// pastes what the clipboard holds then. `record_payload` saves a pixel
    /// payload and is refused here. `recording` false records nothing:
    /// `copy_layers_to_document` pastes an entry the clipboard never
    /// held, so a `paste` step would replay something else.
    func pasteLayers(_ a: [String: Any], entry: LayerClipboard.Entry,
                     recording: Bool = true) throws -> String {
        let document = try target(a)
        guard let doc = document.doc else { throw ToolError(message: "Document has no image") }
        let placement: PastePlacement
        switch stringArg(a, "placement") ?? "auto" {
        case "auto": placement = .auto
        case "centered": placement = .centered
        case "in_place": placement = .inPlace
        case "into", "outside":
            throw ToolError(message: "Paste Into and Paste Outside paste pixels; pass mode "
                + "\"pixels\".")
        case "at":
            throw ToolError(message: "placement at pastes pixels; pass mode \"pixels\".")
        default:
            throw ToolError(message: "placement must be auto, centered or in_place for layers.")
        }
        guard a["x"] == nil, a["y"] == nil else {
            throw ToolError(message: "x and y apply only to placement at.")
        }
        guard a["payload_path"] == nil else {
            throw ToolError(message: "payload_path pastes a pixel payload; pass mode \"pixels\".")
        }
        guard boolArg(a, "record_payload") != true else {
            throw ToolError(message: "record_payload saves a pixel payload; pass mode \"pixels\".")
        }
        guard a["name"] == nil else {
            throw ToolError(message: "Pasted layers keep their own names; name applies to a pixel "
                + "paste. Pass mode \"pixels\" to name a pasted layer.")
        }
        let profile = try pasteProfileArgument(a)
        let (anchor, position) = try insertionArguments(a, document: document)
        // The work's own test (`LayerPasteWork.converted`): any other bytes
        // are relabelled under Preserve, so refuse before the work does.
        if profile == .preserve,
           entry.layers.iccProfile != (doc.iccProfile ?? RasterProfile.builtin(.sRGB)),
           let refusal = LayerPastePlan.preserveRefusal(entry, into: doc) {
            throw ToolError(message: refusal)
        }
        let visible = try pasteVisible(a, document)
        let outcome: LayerPasteOutcome
        if !doc.isHighDepth {
            let (transform, work) = try LayerPasteWork.byteTransform(
                entry, document: document, anchor: anchor, position: position,
                placement: placement, profile: profile,
                context: PasteContext(selection: document.canvasSelection,
                                      quickMask: document.quickMaskActive, in: doc,
                                      visible: visible, origin: nil))
            try performGroupedEdit(document, work.historyName, transform)
            outcome = work
        } else {
            let capture = try PrecisionEditorCapture(document)
            let (pasted, work) = try LayerPasteWork.typedDocument(
                entry, capture: capture, anchor: anchor, position: position,
                placement: placement, profile: profile,
                context: PasteContext(selection: capture.selection,
                                      quickMask: capture.quickMask, in: capture.facade,
                                      visible: visible, origin: nil))
            try capture.publish(pasted, to: document, name: work.historyName)
            outcome = work
        }
        document.setLayerSelection(LayerSelection(primary: outcome.topRoot,
                                                  others: Set(outcome.roots)))
        if recording {
            ActionRecorder.shared.record(
                .paste(payloadPath: nil, placement: placement, offset: nil, name: nil,
                       profile: profile, mode: stringArg(a, "mode") == "layers" ? "layers" : nil,
                       note: "paste"),
                on: document)
        }
        return try jsonResult(outcome.json)
    }

    /// `get_clipboard`'s `layers`: null, or the held layer clipboard.
    func layerClipboardJSON() -> Any {
        LayerClipboard.shared.json(on: Clipboard.pasteboard)
    }
}
