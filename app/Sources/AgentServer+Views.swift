import AppKit

// `syncPanZoom` / `nextViewOrdinal` are VIEW bookkeeping on `lastAppliedLayerComp`'s precedent
// (never in `EditingState`, never persisted, never undone), and all five view tools are
// `nonRecordable`; an omitted `view` is ALWAYS view 1 — the document's first window controller, the
// same editor `AgentServer.editor(_:)` returns — never `NSApp.keyWindow`, which is nil whenever the
// app is inactive.

/// Phase 12D-2's three view tools:
///
/// | Handler | UI path it mirrors |
/// |---|---|
/// | `openSecondView` | Window ▸ New View for “<document>” |
/// | `closeSecondView` | the second window's close button |
/// | `listViews` | the Window menu's list of this document's windows |
///
/// **None of them writes a document.** A view is a window: no pixel, no undo
/// step, no dirty flag, no history entry and nothing a `save_copy` carries —
/// the ruler/grid/proof-setup exemption, which is why all three are
/// `nonRecordable` beside `get_view` / `set_view`.
///
/// **One editor-resolution rule for the whole surface.** An omitted `view` is
/// ALWAYS the document's FIRST window controller — the same editor
/// `AgentServer.editor(_:)` hands every other handler. `NSApp.keyWindow` is
/// never consulted: it is nil whenever the app is not active, which is most of
/// the time an agent is driving it.
extension AgentServer {
    /// This file's MCP tools; merged by AgentServer+Registry.swift.
    static let viewsHandlers: [String: ToolHandler] = [
        "open_second_view": { $0.openSecondView },
        "close_second_view": { $0.closeSecondView },
        "list_views": { $0.listViews },
    ]

    /// Facts for this file's tools (recording, selection, depth admission); merged in AgentServer+Registry.swift.
    static let viewsToolFacts: [String: ToolFacts] = [
        // Phase 12D-2: a VIEW is a window. Opening, closing or listing one
        // changes no pixel, no history and no dirty flag, so the three join
        // get_view/set_view under the same exemption.
        // Phase 12D-2: view rotation and second views are window state at
        // every depth.
        // Phase 12D-2: a float document opens a second view like any other.
        "open_second_view": [.nonRecordable, .commonTool, .floatTool],
        "close_second_view": [.nonRecordable, .commonTool, .floatTool],
        "list_views": [.nonRecordable, .commonTool, .floatTool],
    ]

    // MARK: - Resolution

    /// The editor a call names. `nil` → view 1; an ordinal no window carries
    /// is refused BY NAME, pointing at `list_views`.
    func editor(_ document: ImageDocument, view: Int?) throws -> EditorViewController {
        guard let view else {
            guard let first = editor(document) else {
                throw ToolError(message: "That document has no editor window.")
            }
            return first
        }
        guard let match = document.editor(ordinal: view) else {
            let known = document.windowControllers
                .compactMap { ($0 as? EditorWindowController)?.viewOrdinal }
                .map(String.init).joined(separator: ", ")
            throw ToolError(
                message: "That document has no view \(view)"
                    + (known.isEmpty ? " — it has no editor window." : " — it has \(known).")
                    + " Call list_views.")
        }
        return match
    }

    /// One row per open view: its whole state, plus whether it is the key
    /// window right now (display information only — no handler resolves
    /// anything with it).
    private func viewRows(_ document: ImageDocument) -> [[String: Any]] {
        document.windowControllers.compactMap { controller -> [String: Any]? in
            guard let editor = controller.contentViewController as? EditorViewController
            else { return nil }
            var row = editor.viewState.json
            row["key"] = controller.window?.isKeyWindow ?? false
            return row
        }
    }

    // MARK: - Handlers

    /// open_second_view: Window ▸ New View for “<document>”. The optional
    /// view-state arguments are applied to the NEW view, so "open a 100 %
    /// detail beside the fit overview" is one call.
    func openSecondView(_ a: [String: Any]) throws -> String {
        let document = try target(a)
        guard document.doc != nil else {
            throw ToolError(message: "That document has no image to open a second view on.")
        }
        guard let editorVC = document.openSecondView() else {
            throw ToolError(message: "A second view could not be opened for that document.")
        }
        if let sync = boolArg(a, "sync_pan_zoom") { document.syncPanZoom = sync }
        let state = try applyViewArguments(a, to: editorVC)
        return try jsonResult([
            "ok": true, "view": state.json, "views": viewRows(document),
            "note": "A view is a window: no pixel, no undo step and no dirty flag.",
        ])
    }

    /// close_second_view: closes ONE view. The document's LAST view is refused
    /// by name — closing that is closing the document, which an MCP client
    /// does with `close_document`; the in-app Assistant asks the user to close
    /// it in the app, with its save prompt.
    func closeSecondView(_ a: [String: Any]) throws -> String {
        let document = try target(a)
        guard let ordinal = intArg(a, "view") else {
            throw ToolError(
                message: "close_second_view needs view — the ordinal list_views reports.")
        }
        guard let controller = document.windowControllers
            .compactMap({ $0 as? EditorWindowController })
            .first(where: { $0.viewOrdinal == ordinal }) else {
            throw ToolError(message: "That document has no view \(ordinal). Call list_views.")
        }
        guard document.windowControllers.count > 1 else {
            throw ToolError(
                message: "View \(ordinal) is this document's last view; closing it would close "
                    + "the document. An MCP client closes the document with close_document; "
                    + "the in-app Assistant leaves the view open and asks the user to close the "
                    + "document in the app.")
        }
        document.removeWindowController(controller)
        controller.close()
        return try jsonResult([
            "ok": true, "closed": ordinal, "views": viewRows(document),
            "note": "Closing a view changes no document: the picture, the history and the "
                + "unsaved-changes flag are exactly what they were.",
        ])
    }

    /// list_views: every open view of one document, and the document-wide
    /// Sync Pan & Zoom switch.
    func listViews(_ a: [String: Any]) throws -> String {
        let document = try target(a)
        return try jsonResult([
            "document_id": documentID(for: document),
            "sync_pan_zoom": document.syncPanZoom,
            "views": viewRows(document),
        ])
    }

    /// The optional view-state arguments `open_second_view` shares with
    /// `set_view`, so the two cannot interpret them differently.
    func applyViewArguments(_ a: [String: Any], to editorVC: EditorViewController) throws
        -> EditorViewState
    {
        // Every present key that fails to PARSE is refused by name, exactly
        // as `rotation` already was. A key-presence check alone answered
        // `ok: true` with the view unchanged for `{"zoom": "big"}` or a
        // non-object `center` — a silent no-op the caller could not see.
        var center: CGPoint?
        if let point = a["center"] {
            guard let point = point as? [String: Any],
                  let x = (point["x"] as? NSNumber)?.doubleValue,
                  let y = (point["y"] as? NSNumber)?.doubleValue else {
                throw ToolError(message: "center must be {x, y} in canvas pixels.")
            }
            center = CGPoint(x: x, y: y)
        }
        let rotation = doubleArg(a, "rotation")
        if a["rotation"] != nil, rotation == nil {
            throw ToolError(
                message: "rotation must be a number of degrees — positive is clockwise, and the "
                    + "value is normalized into (-180, 180].")
        }
        let parsedFit = boolArg(a, "fit")
        if a["fit"] != nil, parsedFit == nil {
            throw ToolError(message: "fit must be true or false.")
        }
        let fit = parsedFit ?? false
        let zoom = doubleArg(a, "zoom")
        if a["zoom"] != nil, zoom == nil {
            throw ToolError(
                message: "zoom must be a number — 1 is 100 %. Pass fit: true to fit the window.")
        }
        // The view exposure: View ▸ 32-bit Exposure…, per window like
        // rotation. Its range and its float-only rule are the editor's.
        let exposure = doubleArg(a, "exposure")
        if a["exposure"] != nil, exposure == nil {
            throw ToolError(
                message: "exposure must be a number of stops, -20 to 20 — 0 is the document's "
                    + "own display.")
        }
        // View ▸ HDR Preview (v1.x), per window like the exposure.
        let hdrPreview = boolArg(a, "hdr_preview")
        if a["hdr_preview"] != nil, hdrPreview == nil {
            throw ToolError(message: "hdr_preview must be true or false.")
        }
        do {
            return try editorVC.applyViewState(
                zoom: zoom, center: center, rotation: rotation, fit: fit, exposure: exposure,
                hdrPreview: hdrPreview)
        } catch let error as ToolError {
            throw error
        } catch {
            throw ToolError(message: error.localizedDescription)
        }
    }

    // MARK: - Catalog

    /// `view`, read the ONE way every view tool reads it: nil when absent
    /// (which always means view 1), and refused BY NAME when present and not
    /// an integer — never silently dropped onto the first window.
    func viewArgument(_ a: [String: Any]) throws -> Int? {
        guard a["view"] != nil else { return nil }
        guard let ordinal = intArg(a, "view") else {
            throw ToolError(message: "view must be an integer ordinal — call list_views.")
        }
        return ordinal
    }

    @inline(never) func viewsCatalog() -> [[String: Any]] {
        // No shared `view` property here: `close_second_view` is the only
        // tool in this catalog that takes one, and it is REQUIRED there —
        // the shared sentence in `AgentServer+Workspace.swift` says "omit it
        // for view 1", which is true of get_view/set_view and false here.
        return [
            tool("open_second_view",
                 "Window > New View for \u{201c}<document>\u{201d}: a second window on the SAME "
                 + "document. One picture, one history, one undo stack; each window keeps its own "
                 + "zoom, scroll, view rotation and — on a 32-bit float document — view "
                 + "exposure. A 100 % detail beside a fit overview is what "
                 + "this is for — pass zoom/center/rotation/fit/exposure to set the new view up "
                 + "in the same call. Writes NO document, so it is not recorded as an Action step.",
                 // No `view`: the new window's ordinal is MINTED by this
                 // call and answered in the reply, never chosen by the caller.
                 ["document_id": Self.docIDProperty,
                  "zoom": ["type": "number"], "center": ["type": "object"],
                  "rotation": ["type": "number"], "fit": ["type": "boolean"],
                  "exposure": [
                    "type": "number",
                    "description": "32-bit float documents only: the new view's exposure in "
                        + "stops, -20 to 20 (set_view describes it).",
                  ],
                  "sync_pan_zoom": ["type": "boolean"]]),
            tool("close_second_view",
                 "Close ONE view of a document. `view` is required. The document's LAST view is "
                 + "refused by name: closing it would close the document, which an MCP client "
                 + "does with close_document (the in-app Assistant asks the user to close it "
                 + "in the app). "
                 + "Closing a view changes no document: the pixels, the history and the "
                 + "unsaved-changes flag are exactly what they were.",
                 ["document_id": Self.docIDProperty,
                  "view": [
                    "type": "integer",
                    "description": "Which view (window) to close, as list_views reports it. "
                        + "REQUIRED — there is no default, because closing the wrong window "
                        + "is not something a caller can take back.",
                  ]],
                 required: ["view"]),
            tool("list_views",
                 "The document's open views: each one's zoom, centre, visible rectangle, view "
                 + "rotation, view exposure (exposure, exposure_displayed) and ordinal, plus "
                 + "whether it is the key window. `sync_pan_zoom` is "
                 + "DOCUMENT-wide, not per view. A read.",
                 ["document_id": Self.docIDProperty]),
        ]
    }
}
