import AppKit

// Shortcuts and the panel filter bar have no MCP twin — the app-wide view-preference exemption —
// and `restore_workspace` is the only tool that moves rulers/grid/snap/tab, as a LAYOUT.

/// Phase 12C-2's six tools:
///
/// | Handler | UI path it mirrors |
/// |---|---|
/// | `getView` | the Navigator's proxy rectangle and zoom readout |
/// | `setView` | the Navigator's click-to-centre, drag and zoom slider |
/// | `listWorkspaces` | Window ▸ Workspaces (the list it is built from) |
/// | `saveWorkspace` | Window ▸ Workspaces ▸ Save Workspace… |
/// | `restoreWorkspace` | Window ▸ Workspaces ▸ <name> |
/// | `deleteWorkspace` | Window ▸ Workspaces ▸ Delete Workspace ▸ <name> |
///
/// **`set_view` writes no document.** View state is per WINDOW: no undo step,
/// no dirty flag, no history entry and no Action step — the exemption the
/// rulers, the grid and the proof setup take, for the same four reasons. It
/// therefore does NOT go through `performGroupedEdit`, and the drive checks
/// that `render`'s bytes, `get_document.unsaved_changes` and `list_history`
/// are all untouched by it.
///
/// The four workspace tools write a PREFERENCE LIBRARY, so they are
/// `notAStepTool` as well: a step naming a mutable library entry would not
/// replay the layout it was authored against.
extension AgentServer {
    /// This file's MCP tools; merged by AgentServer+Registry.swift.
    static let workspaceHandlers: [String: ToolHandler] = [
        "get_view": { $0.getView },
        "set_view": { $0.setView },
        "list_workspaces": { $0.listWorkspaces },
        "save_workspace": { $0.saveWorkspace },
        "restore_workspace": { $0.restoreWorkspace },
        "delete_workspace": { $0.deleteWorkspace },
    ]

    /// Facts for this file's tools (recording, selection, depth admission); merged in AgentServer+Registry.swift.
    static let workspaceToolFacts: [String: ToolFacts] = [
        // Phase 12C-2: the Navigator's view state is per WINDOW — no undo
        // step, no dirty flag, nothing a `save_copy` carries — so it takes the
        // proof-setup exemption, and the four workspace tools read or write a
        // preference library.
        // Phase 12C-2: view state and window furniture. None of the four
        // touches pixel storage, so both depths answer them. `list_workspaces`
        // and `delete_workspace` read no document (`documentFreeTools`).
        // Phase 12C-2: view state and window furniture, which a float
        // document has exactly like every other.
        "get_view": [.nonRecordable, .commonTool, .floatTool],
        "set_view": [.nonRecordable, .commonTool, .floatTool],
        // None of these handlers resolves a document.
        // `list_workspaces` / `delete_workspace` read no document
        // (`DocumentPrecision.documentFreeTools`).
        "list_workspaces": [.nonRecordable, .documentFree],
        "save_workspace": [.nonRecordable, .commonTool, .floatTool],
        "restore_workspace": [.nonRecordable, .commonTool, .floatTool],
        "delete_workspace": [.nonRecordable, .documentFree],
    ]

    // MARK: - View state

    /// get_view: where this window is looking. An omitted `view` is ALWAYS
    /// view 1 — the document's first window, never whichever one is key
    /// (`AgentServer+Views.swift` states the rule once for the surface).
    /// `image_hash` true adds `view_image_sha256`, which hashes the whole
    /// image the canvas draws, so it is opt-in.
    func getView(_ a: [String: Any]) throws -> String {
        let hash = boolArg(a, "image_hash")
        if a["image_hash"] != nil, hash == nil {
            throw ToolError(message: "image_hash must be true or false.")
        }
        let document = try target(a)
        let editorVC = try editor(document, view: viewArgument(a))
        let state = editorVC.viewState
        var reply = state.json
        if hash == true {
            reply["view_image_sha256"] = EditorViewState.sha256(of: state.viewImage) ?? NSNull()
        }
        return try jsonResult(reply)
    }

    /// set_view: the magnification, the centre, the view ROTATION, a fit, and
    /// the document-wide Sync Pan & Zoom switch. Out-of-range values are
    /// refused BY NAME rather than clamped, and the reply is the READ-BACK
    /// state — the clip view constrains its own origin, so what a caller wants
    /// to know is where it actually landed.
    func setView(_ a: [String: Any]) throws -> String {
        let document = try target(a)
        let editorVC = try editor(document, view: viewArgument(a))
        let sync = boolArg(a, "sync_pan_zoom")
        if a["sync_pan_zoom"] != nil, sync == nil {
            throw ToolError(message: "sync_pan_zoom must be true or false.")
        }
        guard a["zoom"] != nil || a["center"] != nil || a["rotation"] != nil
            || a["fit"] != nil || a["exposure"] != nil || a["hdr_preview"] != nil || sync != nil
        else {
            throw ToolError(
                message: "set_view needs at least one of zoom, center, rotation, fit, exposure, "
                    + "hdr_preview or sync_pan_zoom.")
        }
        // DOCUMENT-wide, so it is applied before the per-view numbers: a call
        // that turns sync on and pans in one go moves the other views too.
        if let sync { document.syncPanZoom = sync }
        // The per-view arguments go through the parser `open_second_view`
        // shares (`AgentServer+Views.swift`), so the two cannot read the same
        // zoom/center/rotation/fit differently.
        let state = try applyViewArguments(a, to: editorVC)
        var reply = state.json
        reply["ok"] = true
        reply["note"] = "View state only — no pixel, no undo step, no dirty flag, and nothing "
            + "a save_copy would carry."
        return try jsonResult(reply)
    }

    // MARK: - Workspace layouts

    func listWorkspaces(_ a: [String: Any]) throws -> String {
        do {
            let listing = try WorkspaceLibrary.listing()
            return try jsonResult([
                "workspaces": listing.entries.map { entry -> [String: Any] in
                    var row = entry.item.json
                    row["file"] = entry.file
                    return row
                },
                "broken": listing.broken.map(\.json),
                "maximum_workspaces": WorkspaceCodec.maximumEntries,
                "maximum_file_bytes": WorkspaceCodec.maximumFileBytes,
            ])
        } catch let error as ToolError {
            throw error
        } catch {
            throw ToolError(message: error.localizedDescription)
        }
    }

    /// save_workspace: the frontmost (or named) window's furniture under a
    /// name. `replace` defaults to FALSE — the preset convention — while the
    /// UI's Save Workspace… replaces, because its sheet already asked for the
    /// name.
    func saveWorkspace(_ a: [String: Any]) throws -> String {
        let document = try target(a)
        guard let editorVC = editor(document) else {
            throw ToolError(message: "That document has no editor window.")
        }
        let name = try requiredString(a, "name")
        do {
            let layout = editorVC.captureWorkspace(name: try WorkspaceLibrary.normalizedName(name))
            let saved = try WorkspaceLibrary.save(layout, replace: boolArg(a, "replace") ?? false)
            // The file the listing reports, not a name-derived guess: a
            // replace writes to the matched entry's OWN file.
            let file = try WorkspaceLibrary.listing().entries
                .first { $0.item.name == saved.name }?.file
            var reply: [String: Any] = ["ok": true, "workspace": saved.json]
            if let file { reply["file"] = file }
            return try jsonResult(reply)
        } catch let error as ToolError {
            throw error
        } catch {
            throw ToolError(message: error.localizedDescription)
        }
    }

    /// restore_workspace: applies a saved layout to this window. `applied` is
    /// a fresh capture AFTER the apply, so a caller reads back what actually
    /// landed rather than what was asked for.
    func restoreWorkspace(_ a: [String: Any]) throws -> String {
        let document = try target(a)
        guard let editorVC = editor(document) else {
            throw ToolError(message: "That document has no editor window.")
        }
        let name = try requiredString(a, "name")
        do {
            let layout = try WorkspaceLibrary.named(name)
            editorVC.applyWorkspace(layout)
            return try jsonResult([
                "ok": true,
                "workspace": layout.json,
                "applied": editorVC.captureWorkspace(name: layout.name).json,
            ])
        } catch let error as ToolError {
            throw error
        } catch {
            throw ToolError(message: error.localizedDescription)
        }
    }

    func deleteWorkspace(_ a: [String: Any]) throws -> String {
        let hasName = stringArg(a, "name") != nil, hasFile = stringArg(a, "file") != nil
        guard hasName != hasFile else {
            throw ToolError(
                message: "delete_workspace takes exactly one of name or file — a broken entry has no name.")
        }
        do {
            if hasFile {
                let file = try WorkspaceLibrary.validatedFileName(requiredString(a, "file"))
                try WorkspaceLibrary.delete(file: file)
                return try jsonResult(["ok": true, "file": file])
            }
            let layout = try WorkspaceLibrary.named(requiredString(a, "name"))
            let file = try WorkspaceLibrary.listing().entries
                .first { $0.item.name == layout.name }?.file
            try WorkspaceLibrary.delete(name: layout.name)
            var reply: [String: Any] = ["ok": true]
            if let file { reply["file"] = file }
            return try jsonResult(reply)
        } catch let error as ToolError {
            throw error
        } catch {
            throw ToolError(message: error.localizedDescription)
        }
    }

    // MARK: - Catalog

    @inline(never) func workspaceCatalog() -> [[String: Any]] {
        let name: [String: Any] = [
            "type": "string", "description": "Workspace display name, 1–120 characters.",
        ]
        let zoom: [String: Any] = [
            "type": "number",
            "description": "Magnification: 1 is 100 %. 0.02 to 32; outside that it is refused "
                + "by name rather than clamped.",
        ]
        let center: [String: Any] = [
            "type": "object",
            "description": "The point to centre the well on, in canvas pixels (top-left origin). "
                + "Outside the canvas it is refused by name.",
            "properties": ["x": ["type": "number"], "y": ["type": "number"]],
        ]
        let viewID: [String: Any] = [
            "type": "integer",
            "description": "Which view (window) of the document, as list_views reports it. "
                + "Omit it for view 1 — the document's FIRST window, never whichever window "
                + "happens to be key.",
        ]
        let rotation: [String: Any] = [
            "type": "number",
            "description": "View-only rotation in degrees, positive CLOCKWISE. "
                + "Normalized into (-180, 180], so 370 reads back as 10. It turns the "
                + "VIEW, never the pixels: the render is byte-identical before and after, and "
                + "every coordinate this surface takes stays document space.",
        ]
        let fit: [String: Any] = [
            "type": "boolean",
            "description": "Zoom to Fit. Cannot be combined with zoom, which would be two "
                + "answers to one question.",
        ]
        let syncPanZoom: [String: Any] = [
            "type": "boolean",
            "description": "Window > Sync Pan & Zoom. DOCUMENT-wide, not per view: with it on, "
                + "a zoom or pan in one view is applied to the document's other views. Rotation "
                + "is never synchronized.",
        ]
        let exposure: [String: Any] = [
            "type": "number",
            "description": "View > 32-bit Exposure: this window's view exposure in stops, -20 to "
                + "20, on a 32-bit float document. Gain 2^stops on RGB before the tone map, "
                + "alpha untouched, so a negative exposure brings values above white into "
                + "view on screen. It changes how the WINDOW draws, never the samples: "
                + "readouts, samplers, render, exports and saved files do not see it (render "
                + "has its own exposure argument). 0 is the document's own display. Outside "
                + "-20…20, and non-zero on a document that is not 32-bit float, it is refused "
                + "by name. The canvas redraws asynchronously: the reply usually carries "
                + "exposure_displayed false, and a later get_view reports true once the "
                + "exposed image is on screen.",
        ]
        let hdrPreview: [String: Any] = [
            "type": "boolean",
            "description": "View > HDR Preview: this window's switch, on by default. On an EDR "
                + "screen (get_view edr_headroom above 1) a 32-bit float window then shows "
                + "values above 1.0 brighter than white, with the view exposure applied and no "
                + "tone map; off, or on any other screen, the tone-mapped view stays as it is. "
                + "Accepted at every depth, it acts only on a 32-bit float document, and never "
                + "while the Tone Map sheet previews a map or a spot channel's overprint is shown "
                + "(that is drawn on the SDR view). View state only: readouts, render, "
                + "exports and saves never see it. The redraw is asynchronous "
                + "(hdr_preview_displayed).",
        ]
        return [
            tool("get_view",
                 "The Navigator's readout: this window's zoom, the centre and the rectangle it "
                 + "is showing (canvas pixels, top-left origin), the canvas size, the view "
                 + "rotation, the view's ordinal, the document's revision and the document-wide "
                 + "sync_pan_zoom switch, plus exposure (the window's view exposure in stops; 0 "
                 + "unless the document is 32-bit float) and exposure_displayed (true once the "
                 + "canvas shows the image rendered for that exposure; false at 0 stops, where "
                 + "there is nothing to render, unless HDR Preview shows the extended-range "
                 + "view), and View > HDR Preview: hdr_preview (the "
                 + "window's switch), hdr_preview_displayed (true once the extended-range image "
                 + "is on screen; false while a spot channel's overprint is shown, which is drawn "
                 + "on the SDR view at the window's exposure), edr_headroom (the screen's "
                 + "potential EDR headroom; above 1 "
                 + "means HDR Preview can engage), edr_layer (the canvas layer is EDR-configured), "
                 + "edr_content_peak (the largest RGB value of the extended-range image on screen, "
                 + "null when none); with image_hash true the reply adds view_image_sha256 "
                 + "(SHA-256 of the image the canvas draws, null when none; off by default "
                 + "because it hashes the whole image). A read; view state is per window and is "
                 + "not recorded "
                 + "as an Action step. Omitting `view` ALWAYS means view 1 — the document's "
                 + "first window, never whichever one is key.",
                 ["document_id": Self.docIDProperty, "view": viewID,
                  "image_hash": ["type": "boolean", "default": false]]),
            tool("set_view",
                 "Navigator: click-to-centre, drag-to-pan and the zoom slider, plus View > "
                 + "Rotate View and View > 32-bit Exposure. Pass at least one of zoom, center, "
                 + "rotation, fit, exposure, hdr_preview or "
                 + "sync_pan_zoom. `rotation` turns the VIEW only, never a pixel: degrees, "
                 + "positive CLOCKWISE, normalized into (-180, 180]. `fit` is Zoom to Fit and "
                 + "cannot be combined with zoom. `exposure` is a 32-bit float window's view "
                 + "exposure in stops, and its redraw is asynchronous (exposure_displayed). "
                 + "`hdr_preview` is View > HDR Preview (hdr_preview_displayed). "
                 + "`sync_pan_zoom` is DOCUMENT-wide, not per "
                 + "view. This writes NO document — no pixel, no undo step, no dirty flag and "
                 + "nothing a save_copy would carry — so it is not recorded as an Action step, on "
                 + "set_proof_setup's precedent. The reply is the READ-BACK state: the clip view "
                 + "constrains its own origin, so a centre near an edge lands where it can. "
                 + "Omitting `view` ALWAYS means view 1, never whichever window is key.",
                 ["document_id": Self.docIDProperty, "view": viewID, "zoom": zoom,
                  "center": center, "rotation": rotation, "fit": fit,
                  "exposure": exposure, "hdr_preview": hdrPreview,
                  "sync_pan_zoom": syncPanZoom]),
            tool("list_workspaces",
                 "Window > Workspaces: the bounded versioned workspace-layout library. A file that "
                 + "is not a readable workspace is reported as a `broken` row — {file, code, "
                 + "reason, repairable} with code one of unreadable, oversize, not_json, "
                 + "unknown_version, invalid_schema, duplicate_name — instead of refusing the "
                 + "whole library; repair is RAW-only, so repairable is always false here.", [:]),
            tool("save_workspace",
                 "Window > Workspaces > Save Workspace…: store this window's frame, the right "
                 + "panel's visibility, tab and width, the rulers/grid/snap toggles and the "
                 + "current tool under a name. The document's zoom and scroll position are "
                 + "deliberately NOT stored — those are per document (get_view/set_view). A "
                 + "library write is a preference: no Undo step, no Action step.",
                 ["document_id": Self.docIDProperty, "name": name,
                  "replace": ["type": "boolean", "default": false]],
                 required: ["name"]),
            tool("restore_workspace",
                 "Window > Workspaces > <name>: apply a saved layout to this window. `applied` is "
                 + "a fresh capture taken AFTER the apply, so it reports what actually landed. A "
                 + "frame that lies inside the usable area (the union of the screens' visible "
                 + "frames) is kept exactly. Otherwise it is moved, and shrunk if needed, wholly "
                 + "onto one display. A full-screen window keeps its frame, and a tool the "
                 + "document's depth refuses is skipped rather than refused.",
                 ["document_id": Self.docIDProperty, "name": name], required: ["name"]),
            tool("delete_workspace",
                 "Window > Workspaces > Delete Workspace > <name>: remove ONE library file. Pass "
                 + "exactly one of name (a listed workspace) or file (any library file, including "
                 + "a broken one that has no name); passing both or neither is refused.",
                 ["name": name, "file": libraryFileProperty("list_workspaces")]),
        ]
    }
}
