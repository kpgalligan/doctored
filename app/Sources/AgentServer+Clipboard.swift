import AppKit

/// Phase 12G-1: the MCP twins of Edit ▸ Cut / Copy / Copy Merged / Paste /
/// Paste in Place, plus `get_clipboard`; 12G-2 adds `new_from_clipboard`.
///
/// At 8 bits every copy mirrors the byte UI path exactly
/// (`EditorViewController.copyToPasteboard`: `layerCanvasImage` → `masked` →
/// `cropped` → `Clipboard.writeItem`, with the byte-origin typed payload
/// beside it). At 16 bits and float they run the typed paths of
/// `EditorViewController+Clipboard.swift` through `PrecisionEditorCapture`,
/// and never a byte surrogate. Paste runs the SAME plan the Edit menu does
/// (`PastePlan` / `PasteWork`); the UI's Paste Profile Mismatch alert
/// ("Ask When Pasting") is the `profile` argument here, because a handler
/// never opens a modal. The
/// pasteboard is `Clipboard.pasteboard`, so a drive with `DR_PASTEBOARD_NAME`
/// never touches the user's.
///
/// Recording: `copy`, `copy_merged` and `cut` are recorded
/// verbatim by `ActionRecorder.recordAgentCall`, with no pixels, so a replay
/// copies from the replaying document. `paste` records its own step: LIVE by
/// default (no payload, no position: the clipboard at replay), or the saved
/// payload (`payload_path` / `record_payload`).
extension AgentServer {
    /// This file's MCP tools; merged by AgentServer+Registry.swift.
    static let clipboardHandlers: [String: ToolHandler] = [
        "cut": { $0.cut },
        "copy": { $0.copy },
        "copy_merged": { $0.copyMerged },
        "paste": { $0.paste },
        "get_clipboard": { $0.getClipboard },
        "new_from_clipboard": { $0.newFromClipboard },
    ]

    /// Facts for this file's tools (recording, selection, depth admission); merged in AgentServer+Registry.swift.
    static let clipboardToolFacts: [String: ToolFacts] = [
        // Phase 12G-1: `get_clipboard` only READS. `paste`
        // records its own step — live (no payload) or the saved payload
        // (12G-2's `$asset` form, `ClipboardPayloadStore.pasteStep`) — so the
        // generic recorder must not add a second copy. `copy`, `copy_merged`
        // and `cut` ARE steps: a live paste replays only with the copy that
        // fed it.
        // Phase 12G-1: the typed clipboard (`precision_clipboard`).
        // Phase 12G-1: Cut's document half is Clear's erase, so it takes
        // Clear's plane-target and Quick Mask gate.
        // Phase 12G-1: the typed clipboard moves samples only
        // (`clipboard_copy` / `clipboard_copy_merged` / `clipboard_paste`).
        // Phase 12G-2 admits `cut`: its float erase is the core's own f32
        // alpha erase (`clipboard_cut`), RGB retained. `get_clipboard` and
        // `new_from_clipboard` read no document
        // (`DocumentPrecision.documentFreeTools`).
        // #1 phase 4c: Copy, Cut, Copy Merged and Paste on a CMYK document
        // (`cmyk_clipboard`), RZCB v4 payloads.
        "cut": [.typedTool, .rasterTool, .floatTool, .cmykTool],
        "copy": [.selectionSensitive, .typedTool, .floatTool, .cmykTool],
        "copy_merged": [.selectionSensitive, .typedTool, .floatTool, .cmykTool],
        "paste": [.nonRecordable, .typedTool, .floatTool, .cmykTool],
        // None of these handlers resolves a document either — New
        // Document, the clipboard read and New from Clipboard, the action
        // library and recorder, and the batch and bundle tools (a batch opens
        // each file at its own depth and plays through this same gate).
        // Phase 12G: `get_clipboard` (a pasteboard read) and
        // `new_from_clipboard` (a document at the payload's own depth) read
        // no document, so they are in `documentFreeTools`.
        "get_clipboard": [.nonRecordable, .documentFree],
        // Phase 12G-2: `new_from_clipboard` CREATES a document —
        // `new_document`'s reason.
        "new_from_clipboard": [.nonRecordable, .documentFree],
    ]

    private func depth(of doc: RasterDocument) -> Int {
        doc.isFloat ? 32 : doc.isHighDepth ? 16 : 8
    }

    /// `public_copy_path`: an absolute `.tiff` path that receives the SAME
    /// TIFF bytes the pasteboard holds.
    private func publicCopyURL(_ a: [String: Any]) throws -> URL? {
        guard let path = stringArg(a, "public_copy_path") else { return nil }
        guard path.hasPrefix("/"), (path as NSString).pathExtension.lowercased() == "tiff" else {
            throw ToolError(message: "public_copy_path must be an absolute path ending in .tiff.")
        }
        return URL(fileURLWithPath: path)
    }

    private func writePublicCopy(_ url: URL?) throws {
        guard let url else { return }
        guard let data = Clipboard.pasteboard.data(forType: .tiff) else {
            throw ToolError(message: "The clipboard holds no TIFF to write to public_copy_path.")
        }
        do { try DurableFile.write(data, to: url) } catch {
            throw ToolError(message: "Could not write \(url.path): \(error.localizedDescription)")
        }
    }

    private func publicTypes() -> [String] {
        let pasteboard = Clipboard.pasteboard
        var out: [String] = []
        if pasteboard.availableType(from: [.tiff]) != nil { out.append("tiff") }
        if pasteboard.availableType(from: [.png]) != nil { out.append("png") }
        return out
    }

    /// `profile` — "convert" (default) or "preserve", the Paste Profile
    /// Mismatch alert's choice, for the pixel and the layers paste alike.
    func pasteProfileArgument(_ a: [String: Any]) throws -> PasteProfile {
        guard let text = stringArg(a, "profile") else { return .convert }
        guard let profile = PasteProfile(rawValue: text) else {
            throw ToolError(message: "profile must be convert or preserve.")
        }
        return profile
    }

    /// The visible canvas of the view `paste` names
    /// (`view`, an ordinal; view 1 without one), nil when the document has
    /// no window; a `view` no window carries is refused naming `list_views`.
    func pasteVisible(_ a: [String: Any], _ document: ImageDocument) throws -> CGRect? {
        if let view = try viewArgument(a) {
            return try editor(document, view: view).viewState.visible
        }
        return editor(document)?.viewState.visible
    }

    /// `Clipboard.writeItem`, its refusal as the tool's error, with the
    /// copied document's `ClipboardSource`.
    private func writeItem(_ cgImage: CGImage, typed: Data?, from document: ImageDocument,
                           tiff: Data? = nil) throws {
        let source = document.doc.map { ClipboardSource(document: document, canvas: $0) }
        do { try Clipboard.writeItem(cgImage, typed: typed, source: source, tiff: tiff) } catch {
            throw ToolError(message: (error as? DoctoredCoreError)?.message ?? error.localizedDescription)
        }
    }

    /// `typed_store` — "pasteboard", "process" or null.
    private func typedStore() -> Any {
        Clipboard.typedStore(on: Clipboard.pasteboard) ?? NSNull()
    }

    /// The `editable` a copy reply reports: the attached record's type, or
    /// null.
    private func editableType(_ info: [String: Any]?) -> Any {
        (info?["editable"] as? [String: Any])?["type"] ?? NSNull()
    }

    /// The byte Copy's tail, verbatim: coverage-multiplied, cropped to the
    /// selection's bounds, tagged with the document's profile. A non-nil
    /// `layer` (a whole-layer Copy, 12G-3) attaches its editable record —
    /// `typedBytes(forByteCopy:offset:layer:)`'s rule.
    private func byteCopy(_ document: ImageDocument, source: RasterImage?,
                          layer: Int?) throws -> [String: Any] {
        var image = source
        var offset = [0, 0]
        if let selection = document.canvasSelection {
            let bounds = selection.bounds
            image = image?.masked(by: selection.maskBytes())
            image = image?.cropped(x: Int(bounds.minX), y: Int(bounds.minY),
                                   w: Int(bounds.width), h: Int(bounds.height))
            offset = [Int(bounds.minX), Int(bounds.minY)]
        }
        guard let image, let cgImage = image.makeCGImage(in: document.colorSpace) else {
            throw ToolError(message: "The clipboard could not be written.")
        }
        // Phase 12G-2: the byte-origin typed payload beside the public
        // types — `typedBytes(forByteCopy:)`'s rule: public only when it
        // cannot be built.
        var typed: Data?
        var editable: Any = NSNull()
        if let doc = document.doc {
            do {
                var payload = try doc.bytePayload(image, offset: (offset[0], offset[1]))
                if let layer { payload = try doc.attachingEditable(payload, layer: layer) }
                typed = payload.bytes
                editable = editableType(try payload.info())
            } catch {
                typed = nil
                editable = NSNull()
            }
        }
        try writeItem(cgImage, typed: typed, from: document)
        return ["ok": true, "typed": typed != nil, "depth": 8, "width": image.width,
                "height": image.height, "offset": offset, "public_types": publicTypes(),
                "typed_bytes": typed?.count ?? 0, "editable": editable,
                "typed_store": typedStore()]
    }

    /// The typed Copy's tail: the RZCB payload plus its public rendering in
    /// one pasteboard item.
    private func typedWrite(_ payload: RasterClipboardPayload, from doc: RasterDocument,
                            document: ImageDocument) throws -> [String: Any] {
        let rendering = try doc.publicRendering(of: payload)
        let bytes = payload.bytes
        guard let cgImage = rendering.makeCGImage(in: document.colorSpace) else {
            throw ToolError(message: "The clipboard could not be written.")
        }
        // #1 phase 4: a Grayscale copy's TIFF is its one-channel gray image.
        let tiff = try payload.pasteboardTIFF(from: doc, held: bytes.count)
        try writeItem(cgImage, typed: bytes, from: document, tiff: tiff)
        let info = try payload.info()
        return ["ok": true, "typed": true, "depth": info["depth"] ?? depth(of: doc),
                "width": info["width"] ?? 0, "height": info["height"] ?? 0,
                "offset": info["offset"] ?? [0, 0], "public_types": publicTypes(),
                "typed_bytes": bytes.count, "editable": editableType(info),
                "typed_store": typedStore()]
    }

    /// The typed copy window, clipped to the canvas; the UI beeps where
    /// this throws (a selection entirely off the canvas).
    private func clipboardWindow(_ capture: PrecisionEditorCapture) throws -> CGRect {
        guard let window = capture.facade.clipboardWindow(capture.selection) else {
            throw ToolError(message: "The selection lies entirely outside the canvas, so there "
                + "are no pixels to copy. Select an area inside the canvas first.")
        }
        return window
    }

    /// copy: mirrors Edit ▸ Copy (⌘C) — the layer's raw pixels inside the
    /// selection's bounds, coverage applied. With no selection (12G-3) the
    /// copy carries a text, shape or adjustment layer's editable record
    /// (`copyPrecision` / `typedBytes(forByteCopy:offset:layer:)`), and an
    /// adjustment layer copies as a transparent payload carrying its meta.
    /// A group copies its raw projection at every depth.
    /// `mode: "auto"` (what a recorded Edit ▸ Copy replays)
    /// decides as the menu item does, from the document's own selection
    /// (`LayerClipboard.resolvingAutoCopy`). While recording, a successful
    /// call is recorded verbatim (`ActionRecorder.recordAgentCall`).
    func copy(_ a: [String: Any]) throws -> String {
        var a = a
        if stringArg(a, "mode") == "auto" {
            a = LayerClipboard.resolvingAutoCopy(tool: "copy", a, in: try target(a))
        }
        switch stringArg(a, "mode") ?? "pixels" {
        case "pixels": break
        case "layers":
            // Edit ▸ Copy with no pixel selection (`copyLayersReply`).
            let url = try publicCopyURL(a)
            let reply = try copyLayersReply(a)
            try writePublicCopy(url)
            return try jsonResult(reply)
        default: throw ToolError(message: "mode must be auto, pixels or layers.")
        }
        let document = try target(a)
        guard let doc = document.doc else { throw ToolError(message: "Document has no image") }
        let index = try structuralLayerIndex(a, document)
        let whole = document.canvasSelection == nil
        if !whole { try rejectAdjustmentPixelEdit(document, index) }
        let url = try publicCopyURL(a)
        let reply: [String: Any]
        if doc.isHighDepth {
            let capture = try PrecisionEditorCapture(document)
            let facade = capture.facade
            // #1 phase 4c: a CMYK adjustment record refuses by the core's
            // name; since the artwork batch a CMYK type or shape layer's
            // whole-layer copy carries its record.
            let editable = whole && !(facade.isCmyk && facade.layerIsAdjustment(index))
            var payload = editable && facade.layerIsAdjustment(index)
                ? try facade.transparentPayload()
                : try facade.clipboardCopy(
                    layer: index, window: clipboardWindow(capture), selection: capture.selection)
            if editable { payload = try facade.attachingEditable(payload, layer: index) }
            reply = try typedWrite(payload, from: facade, document: document)
        } else {
            reply = try byteCopy(document, source: doc.byteCopySource(layer: index, whole: whole),
                                 layer: whole ? index : nil)
        }
        try writePublicCopy(url)
        return try jsonResult(reply)
    }

    /// copy_merged: mirrors Edit ▸ Copy Merged (⇧⌘C) — the projection.
    /// While recording, a successful call is recorded verbatim.
    func copyMerged(_ a: [String: Any]) throws -> String {
        let document = try target(a)
        guard let doc = document.doc else { throw ToolError(message: "Document has no image") }
        let url = try publicCopyURL(a)
        let reply: [String: Any]
        if doc.isHighDepth {
            let capture = try PrecisionEditorCapture(document)
            let payload = try capture.facade.clipboardCopyMerged(
                window: clipboardWindow(capture), selection: capture.selection)
            reply = try typedWrite(payload, from: capture.facade, document: document)
        } else {
            reply = try byteCopy(document, source: try document.byteComposite(of: doc), layer: nil)
        }
        try writePublicCopy(url)
        return try jsonResult(reply)
    }

    /// cut: mirrors Edit ▸ Cut (⌘X) — Copy, then Clear's erase, as one step.
    /// A selection is required, as in the UI. The pasteboard is written
    /// before anything is published; a transparency lock leaves the payload
    /// there and answers `frozenAlphaRefusal` with no history step.
    func cut(_ a: [String: Any]) throws -> String {
        let document = try target(a)
        guard let doc = document.doc else { throw ToolError(message: "Document has no image") }
        let index = try paintLayerIndex(a, document)
        try rejectAdjustmentPixelEdit(document, index)
        guard let mask = selectionMask(document) else {
            throw ToolError(
                message: "There is no selection to cut. Make one first with select_rect, "
                    + "select_ellipse, select_polygon or select_magic_wand.")
        }
        try rejectLockedEdit(document, index, DR_EDIT_PIXELS)
        let url = try publicCopyURL(a)
        if !doc.isHighDepth {
            var reply = try byteCopy(document, source: doc.layerCanvasImage(index), layer: nil)
            try writePublicCopy(url)
            let rasterized = try performPixelEdit(document, "Cut", pixelLayer: index) { current in
                current.clearingSelection(index, mask: mask)
            }
            reply["layer"] = index
            return try pixelEditResult(reply, layer: index, rasterized: rasterized)
        }
        if doc.hasRetainedSource(index) { throw ToolError(message: RetainedImagePolicy.rasterizeFirst) }
        let capture = try PrecisionEditorCapture(document)
        let (payload, next) = try capture.facade.clipboardCut(
            layer: index, window: clipboardWindow(capture), selection: capture.selection)
        var reply = try typedWrite(payload, from: capture.facade, document: document)
        try writePublicCopy(url)
        guard let next else {
            throw frozenAlphaRefusal(document, index, DR_EDIT_PIXELS, "Cut")
                ?? ToolError(message: "Cut changed nothing: the selected pixels are already "
                    + "transparent. The clipboard holds the copy; no undo step was added.")
        }
        try capture.publish(next.precision, to: document, name: "Cut")
        reply["layer"] = index
        return try jsonResult(reply)
    }

    /// paste: mirrors Edit ▸ Paste (⌘V, `placement: "auto"`, the default —
    /// `PasteContext` over the view `view` names, view 1 without
    /// it; the canvas with no window), Edit ▸ Paste Special ▸ Paste in Place
    /// (⇧⌘V, `"in_place"`: the copy's `ClipboardSource` decides a
    /// `relative_in_place` landing), Paste Into (⌥⇧⌘V,
    /// `"into"`) and Paste Outside (`"outside"`) through the SAME `PastePlan`
    /// the menu resolves; `"centered"` is the canvas centre and `"at"` with
    /// `x` / `y` an exact position (what a saved-payload recording replays).
    /// The two masked placements read their `PasteMask`
    /// from the state they publish against (the document at 8 bits, the
    /// capture at depth), consume the selection in the same undo step and
    /// reply `mask`; every other pixel paste made with a selection consumes
    /// it too (`PasteKind`). Every depth and profile converts as the menu does;
    /// `profile` is the "Ask When Pasting" alert's choice ("convert",
    /// the default, or "preserve"), a `convert` boolean (old recordings) is
    /// accepted and ignored, and `external: "refuse"` refuses another app's
    /// image at 16 bits and float (default "promote").
    /// `payload_path` pastes an RZCB file instead of the pasteboard — the
    /// form a saved-payload step replays from. Records its own step.
    /// By default the LIVE step: the requested placement (`x` / `y`
    /// for `at`), `name` and `mode` when the call passed them, no payload —
    /// a replay pastes the clipboard it finds, and fails with the plan's own
    /// refusal when there is no image. With `payload_path` or
    /// `record_payload: true`, the `$asset`-backed saved-payload step
    /// (`ClipboardPayloadStore.pasteStep`), built BEFORE the edit is
    /// published so a failed recording write publishes nothing. `name`
    /// omitted is `PasteWork`'s rule: an editable record's own name,
    /// otherwise "Pasted Layer".
    func paste(_ a: [String: Any]) throws -> String {
        // `auto` pastes a held layer clipboard as layers
        // (`pasteLayers`), `layers` requires one, `pixels` ignores it.
        // `auto` routes as `pastePlanned` does: only placements auto,
        // centered and in_place with no `payload_path` (a saved-payload
        // step always carries one) paste layers.
        let entry = LayerClipboard.shared.current(on: Clipboard.pasteboard)
        let explicitMode = stringArg(a, "mode").flatMap {
            ["layers", "pixels"].contains($0) ? $0 : nil
        }
        switch stringArg(a, "mode") ?? "auto" {
        case "auto":
            let requested = stringArg(a, "placement") ?? "auto"
            let layered = ["auto", "centered", "in_place"].contains(requested)
            if let entry, a["payload_path"] == nil, layered {
                return try pasteLayers(a, entry: entry)
            }
            // Paste Into / Outside of copied layers refuses
            // (`pastePlanned` beeps, the items are disabled).
            if entry != nil, a["payload_path"] == nil,
               requested == "into" || requested == "outside" {
                throw ToolError(message: LayerClipboard.maskedPasteRefusal(
                    requested == "outside" ? .outside : .into))
            }
        case "layers":
            guard let entry else {
                throw ToolError(message: "The clipboard holds no copied layers. Copy layers with "
                    + "copy mode \"layers\" first, or pass mode \"pixels\".")
            }
            return try pasteLayers(a, entry: entry)
        case "pixels": break
        default: throw ToolError(message: "mode must be auto, layers or pixels.")
        }
        let document = try target(a)
        guard let doc = document.doc else { throw ToolError(message: "Document has no image") }
        let placement: PastePlacement
        switch stringArg(a, "placement") ?? "auto" {
        case "auto": placement = .auto
        case "centered": placement = .centered
        case "in_place": placement = .inPlace
        case "into": placement = .into
        case "outside": placement = .outside
        case "at": placement = .at
        default:
            throw ToolError(message: "placement must be auto, centered, in_place, into, outside "
                + "or at.")
        }
        let at: (x: Int, y: Int)?
        if placement == .at {
            guard let x = intArg(a, "x"), let y = intArg(a, "y") else {
                throw ToolError(message: "placement at needs integer x and y.")
            }
            at = (x, y)
        } else {
            guard a["x"] == nil, a["y"] == nil else {
                throw ToolError(message: "x and y apply only to placement at.")
            }
            at = nil
        }
        let name = stringArg(a, "name")
        let profile = try pasteProfileArgument(a)
        let externalArm = stringArg(a, "external") ?? "promote"
        guard externalArm == "promote" || externalArm == "refuse" else {
            throw ToolError(message: "external must be promote or refuse.")
        }
        var payloadFile: URL?
        if let path = stringArg(a, "payload_path") {
            let payload = SideFileExtension.clipboardPayload
            guard path.hasPrefix("/"), payload.matches(path: path) else {
                throw ToolError(message: "payload_path must be an absolute path ending in "
                    + ".\(payload.current) (or the legacy .\(payload.legacy)).")
            }
            payloadFile = URL(fileURLWithPath: path)
        }
        // The saved-payload step only when asked for; the live
        // step otherwise.
        let savesPayload = payloadFile != nil || boolArg(a, "record_payload") == true
        func recorded(_ work: PasteOutcome) throws -> [ActionStep] {
            if savesPayload {
                return try ClipboardPayloadStore.pasteStep(
                    work, placement: placement, profile: profile, note: "paste")
            }
            return .paste(payloadPath: nil, placement: placement, offset: at, name: name,
                          profile: profile, mode: explicitMode, note: "paste")
        }
        let (anchor, position) = try insertionArguments(a, document: document)
        let plan = try PastePlan.resolve(for: document, pasteboard: Clipboard.pasteboard,
                                         payloadFile: payloadFile, profile: profile)
        switch plan {
        case .external where doc.isHighDepth:
            guard externalArm == "promote" else {
                throw ToolError(message: "The clipboard holds an 8-bit image from another app "
                    + "and external is \"refuse\". Omit external (or pass \"promote\") to "
                    + "promote it exactly (×257) into this document.")
            }
        case .refused(let reason):
            throw ToolError(message: reason)
        default:
            break
        }
        // `auto` reads the named view's visible canvas (view 1 without
        // `view`); nil with no window. Paste in Place reads the
        // copy's source canvas, which a payload file does not carry.
        let visible = try pasteVisible(a, document)
        let origin = payloadFile == nil ? ClipboardSource.read(Clipboard.pasteboard) : nil
        let kind = PasteKind(pixels: placement)
        let outcome: PasteOutcome
        let steps: [ActionStep]
        if !doc.isHighDepth {
            guard position == .above else {
                throw ToolError(message: "At 8 bits a pasted layer lands above its anchor; "
                    + "position top and into_group need a 16-bit or float document.")
            }
            let mask = placement.masksSelection
                ? try PasteMask(placement: placement, selection: document.canvasSelection,
                                quickMask: document.quickMaskActive, in: doc)
                : nil
            let spot = try PasteWork.landing(
                plan, request: placement, at: at, mask: mask, canvas: (doc.width, doc.height),
                context: PasteContext(selection: document.canvasSelection,
                                      quickMask: document.quickMaskActive, in: doc,
                                      visible: visible, origin: origin))
            let (transform, work) = try PasteWork.byteTransform(
                plan, document: document, anchor: anchor, landing: spot, name: name,
                mask: mask, profile: profile)
            steps = try recorded(work)
            let state = document.selectionHistory
            try performGroupedEdit(
                document, work.historyName,
                resultingSelection: mask?.consumed(state) ?? kind.resultingSelection(state),
                transform)
            outcome = work
        } else {
            let capture = try PrecisionEditorCapture(document)
            let mask = placement.masksSelection
                ? try PasteMask(placement: placement, selection: capture.selection,
                                quickMask: capture.quickMask, in: capture.facade)
                : nil
            let spot = try PasteWork.landing(
                plan, request: placement, at: at, mask: mask,
                canvas: (capture.facade.width, capture.facade.height),
                context: PasteContext(selection: capture.selection,
                                      quickMask: capture.quickMask, in: capture.facade,
                                      visible: visible, origin: origin))
            let (pasted, work) = try PasteWork.typedDocument(
                plan, capture: capture, anchor: anchor, position: position, landing: spot,
                name: name, profile: profile, mask: mask)
            steps = try recorded(work)
            let state = capture.selectionState
            try capture.publish(pasted, to: document, name: work.historyName,
                                resultingSelection: mask?.consumed(state)
                                    ?? kind.resultingSelection(state))
            outcome = work
        }
        let count = document.doc?.layerCount ?? 1
        if outcome.mask != nil {
            document.landMaskedPaste(at: min(outcome.landing, count - 1))
        } else {
            document.activeLayerIndex = min(outcome.landing, count - 1)
        }
        ActionRecorder.shared.record(steps, on: document)
        var reply = outcome.json
        reply["layer"] = document.activeLayerIndex
        return try jsonResult(reply)
    }

    /// new_from_clipboard: mirrors File ▸ New from Clipboard
    /// (`NewFromClipboard.make`) — a document at the payload's own depth,
    /// profile and PPI, sized to the payload's content and rebuilt
    /// around its editable record, or today's 8-bit document from an
    /// external image. A held layer clipboard lands its
    /// layers in a document sized to their content box at their depth,
    /// profile and PPI over a white Background (`layers` = the records, null
    /// otherwise).
    /// `newDocument`'s tail; nonRecordable for its reason.
    func newFromClipboard(_ a: [String: Any]) throws -> String {
        let made: NewFromClipboard.Made
        do {
            guard let result = try NewFromClipboard.make(pasteboard: Clipboard.pasteboard) else {
                throw ToolError(message: PastePlan.noImage)
            }
            made = result
        } catch let error as ToolError {
            throw error
        } catch {
            throw ToolError(message: (error as? DoctoredCoreError)?.message ?? error.localizedDescription)
        }
        let document = made.document
        NSDocumentController.shared.addDocument(document)
        document.makeWindowControllers()
        document.showWindows()
        guard let doc = document.doc else { throw ToolError(message: "Document has no image") }
        var typed = true, originDepth = depth(of: doc)
        var profile = doc.profileName
        var records: Any = NSNull()
        switch made.outcome {
        case .typed(_, let name): profile = name
        case .byteOrigin: originDepth = 8
        case .external: typed = false; originDepth = 8
        case let .layers(count, origin, name):
            typed = depth(of: doc) > 8; originDepth = origin; profile = name; records = count
        }
        var reply: [String: Any] = [
            "ok": true, "typed": typed, "origin_depth": originDepth, "depth": depth(of: doc),
            "width": doc.width, "height": doc.height,
            "ppi": [doc.resolution.x, doc.resolution.y], "profile_description": profile,
            "document": summary(document),
            "editable": PasteOutcome.editableJSON(made.editable), "trimmed": made.trimmed,
            "source_offset": made.sourceOffset.map { [$0.x, $0.y] as Any } ?? NSNull(),
            "layers": records,
        ]
        if let map = doc.precision?.toneMapJSON(),
           let object = try? JSONSerialization.jsonObject(with: Data(map.utf8)) {
            reply["tone_map"] = object
        }
        return try jsonResult(reply)
    }

    /// get_clipboard: what the clipboard holds — the typed payload's
    /// description when there is one, the public types beside it, and
    /// the source canvas a Doctored copy was made on.
    func getClipboard(_ a: [String: Any]) throws -> String {
        var reply: [String: Any] = ["typed": false, "public_types": publicTypes(),
                                    "layers": layerClipboardJSON(),
                                    "source": ClipboardSource.read(Clipboard.pasteboard)?.json
                                        ?? NSNull()]
        // #1 phase 4c: `mode` — "rgb" or "cmyk" (phase 4: or "gray"; Lab phase 4c: or "lab"),
        // the typed payload's colour mode; null without one.
        reply["mode"] = NSNull()
        if let typed = try TypedClipboardPayload.read(Clipboard.pasteboard) {
            reply["typed"] = true
            reply["info"] = typed.info
            reply["mode"] = typed.isLab ? "lab" : typed.isCmyk ? "cmyk" : typed.isGray ? "gray"
                : "rgb"
        }
        reply["typed_store"] = typedStore()
        return try jsonResult(reply)
    }
}
