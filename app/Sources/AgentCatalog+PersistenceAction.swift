import Foundation

extension AgentServer {
    @inline(never)
    func persistenceActionCatalog(_ schema: CatalogSchema) -> [[String: Any]] {
        let docID = schema.docID
        return [
            tool(
                "undo",
                "Undoes the most recent edit. An undo whose state's display goes over the working "
                    + "limit still proceeds and returns memory_warning naming the overage. "
                    + "While a job is still running on the document it "
                    + "refuses, naming the job_id: cancel_job it or wait for job_status, then undo.",
                ["document_id": docID]),
            tool(
                "redo",
                "Redoes the most recently undone edit. A redo whose state's display goes over the "
                    + "working limit still proceeds and returns memory_warning naming the overage. "
                    + "While a job is still running on the "
                    + "document it refuses, naming the job_id: cancel_job it or wait for "
                    + "job_status, then redo.",
                ["document_id": docID]),
            tool(
                "save_copy",
                "Exports the document to a file without changing the open document. "
                    + "Format comes from the extension (.tif/.tiff and .jpg/.jpeg both work) "
                    + "unless given explicitly. "
                    + "doctored (.doctored) writes the full layered document (layers, masks, metadata); "
                    + "rz is its legacy name and writes the same file; "
                    + "raster formats flatten. The result's \"wrote\" and \"dropped\" "
                    + "arrays name what the chosen format actually carried. flat_save_losses "
                    + "lists editing state or sidecar data the flat copy cannot preserve on reopen; "
                    + "get_document.save_preservation: save_types/save_as_types are File > Save's policy; "
                    + "losses_by_format is what a flat copy in each format loses (on a 16-bit document, "
                    + "save_copy's flat_save_losses at the highest depth it writes in that format — "
                    + "16 for png/tiff, else 8 — and heic and avif at export_image's 16; a 32-bit float "
                    + "document reports it the same way, its 16-bit copy tone-mapped). A kind is "
                    + "dropped when the format cannot hold it, when it was too large for "
                    + "the format's segment, or when you asked for it to be left out. "
                    + "metadata_not_captured: true means the source file's EXIF/XMP/IPTC "
                    + "was never read in (a platform-decoded HEIC or Live Photo), so none "
                    + "of it could travel out either.",
                [
                    "path": ["type": "string"],
                    "format": [
                        "type": "string",
                        "enum": ["doctored", "rz", "png", "jpeg", "tiff", "bmp", "gif", "webp"],
                    ],
                    "jpeg_quality": ["type": "integer", "minimum": 1, "maximum": 100],
                    "embed_profile": [
                        "type": "boolean",
                        "description": "Embed the document's ICC profile (default true). "
                            + "PNG, JPEG, TIFF and WebP can carry one; BMP and GIF cannot.",
                    ],
                    "strip_metadata": [
                        "type": "boolean",
                        "description": "Drop EXIF/XMP/IPTC (default false). Kept packets "
                            + "are re-spliced with the EXIF orientation reset to 1, since "
                            + "the rotation is already baked into the pixels.",
                    ],
                    "document_id": docID,
                ], required: ["path"]),

            // Actions — recorded sequences of these very tools
            // (AgentServer+Actions.swift). The batch tools are in
            // AgentCatalog+ReusableActions.swift.
            tool(
                "list_actions",
                "Lists the saved Actions — named, replayable sequences of these tools. "
                    + "Mirrors the Actions window's list. Files that could not be read "
                    + "come back in \"broken\" with the exact reason, never silently "
                    + "dropped. With `name` the reply also carries `parameters` (what "
                    + "run_action and run_batch must be given) and `missing_assets` (the "
                    + "full paths of {\"$asset\": …} files that are not there, so a "
                    + "broken bundle is visible BEFORE a run changes anything).",
                [
                    "name": [
                        "type": "string",
                        "description": "Omit to list every action; give one to return "
                            + "that action's full steps.",
                    ]
                ]),
            tool(
                "run_action",
                "Plays a saved Action on a document, as ONE undo step for the whole run. "
                    + "Mirrors the Actions window's Play button. Steps that reference "
                    + "\"the active layer\" or \"the current selection\" resolve against "
                    + "THIS document. A step's coordinates are absolute canvas pixels "
                    + "unless it carries a \"geometry\" mode, so a size difference on an "
                    + "absolute step is reported in canvas_mismatch rather than guessed "
                    + "at. Values for the action's declared parameters go in "
                    + "`parameters`; a missing required one, an unknown name or a "
                    + "wrong-typed value refuses the whole call with NOTHING applied — "
                    + "list_actions {name} reports what it declares. It never asks: a failed "
                    + "step follows its own on_error, or stop_on_error when given; the "
                    + "Continue / Stop prompt belongs to the Actions window's Play.",
                [
                    "name": ["type": "string"],
                    "parameters": [
                        "type": "object",
                        "description": "Values for the action's declared parameters, as "
                            + "{name: value}. Every declared name without a default has "
                            + "to be here.",
                    ],
                    "stop_on_error": [
                        "type": "boolean",
                        "description": "Overrides every step's own on_error for this run.",
                    ],
                    "document_id": docID,
                ], required: ["name"]),
            tool(
                "save_action",
                "Creates or replaces an Action. Mirrors Stop Recording > name it, and "
                    + "the Actions window's JSON editor. Each step is "
                    + "{\"tool\", \"arguments\", optional \"enabled\", \"on_error\", "
                    + "\"note\"} with document_id omitted — the player rebinds it. Write "
                    + "{\"$layer\": \"active\"} for the active layer, {\"$layer\": "
                    + "\"Sky\"} for one by name, {\"$layers\": \"selected\"} for the "
                    + "current layer selection. Schema 2 adds four more: {\"$layer\": "
                    + "{\"name\", \"id\", \"session\"}} resolves by its STABLE id "
                    + "when `session` matches get_document's identity_session; in "
                    + "another session by its id only when that layer also carries the "
                    + "recorded name (layer ids are persisted in the file), otherwise by "
                    + "name. Its {\"$channel\": …} twin resolves by its id only when "
                    + "`session` matches (channel ids are per-launch), otherwise by "
                    + "name; {\"$param\": \"<name>\"} is a "
                    + "declared parameter's value; {\"$asset\": \"<file>\"} is a file "
                    + "in the action's bundle, which only package_action writes. A step "
                    + "may also carry \"geometry\": {\"mode\": \"absolute\" (the "
                    + "default) | \"relative\" | \"anchor\", \"anchor\": "
                    + "\"bottom-right\"}, which remaps its canvas coordinates onto a "
                    + "differently sized document and needs recorded_canvas. A file is "
                    + "written at schema 1 when it uses none of those. Every step is "
                    + "validated before a byte is written, and an unknown tool is refused "
                    + "by step index. A step may not name, for example, a tool that runs "
                    + "or edits actions or batches (run_action, run_batch, save_action, "
                    + "delete_action, list_actions, start_recording, stop_recording, "
                    + "package_action, import_action), writes a file to a literal path "
                    + "(save_copy, export_image, export_pdf, export_psd), closes the "
                    + "document, or writes a preset, policy or workspace library; the "
                    + "refusal gives the reason and, where there is one, what to record "
                    + "instead. save_copy's "
                    + "recorded path is absolute, so a batch would write every file over "
                    + "that one path: record quick_export (a destination plus a pattern "
                    + "containing {name}, never overwriting) or let Batch choose the "
                    + "folder, the format and the name. undo and redo are refused as "
                    + "steps as well: an action IS one undo entry, so an undo inside it "
                    + "throws the document's redo history away and reports success.",
                [
                    "name": ["type": "string"],
                    "steps": ["type": "array", "items": ["type": "object"]],
                    "raw": [
                        "type": "object",
                        "description": "Camera RAW develop settings Batch opens RAW files "
                            + "with. Same shape as open_document's raw.",
                    ],
                    "parameters": [
                        "type": "array", "items": ["type": "object"],
                        "description": "Declared parameters: [{\"name\", \"type\" "
                            + "(number|integer|string|boolean), optional \"default\", "
                            + "optional \"description\"}]. A step references one with "
                            + "{\"$param\": \"<name>\"}, and run_action/run_batch bind "
                            + "the values. A name with no default is REQUIRED.",
                    ],
                    "recorded_canvas": [
                        "type": "object",
                        "description": "{\"width\", \"height\"} — the canvas the steps' "
                            + "coordinates are expressed in. REQUIRED when any step carries "
                            + "a non-absolute \"geometry\", which remaps against it.",
                    ],
                    "overwrite": [
                        "type": "boolean",
                        "description": "Replace an action of this name (default false).",
                    ],
                ], required: ["name", "steps"]),
            tool(
                "delete_action",
                "Deletes a saved Action. Mirrors the Actions window's Delete button.",
                ["name": ["type": "string"]], required: ["name"]),
            tool(
                "start_recording",
                "Starts recording every edit — yours and the user's alike — into a new "
                    + "Action. Mirrors File > Automate > Start Recording. Read-only "
                    + "tools, undo, redo, save_copy and open_document are not recorded "
                    + "(an action is replayed on a document that already exists, and in "
                    + "Batch on a different file every time); a user command with no tool "
                    + "of its own (Quick Mask) lands as a visible \"__unrecorded\" "
                    + "placeholder that names it rather than a silent gap. Copy, Copy Merged "
                    + "and Cut are recorded as steps that copy from the replaying document, "
                    + "and Paste as a step that pastes the clipboard at replay. Refused while a "
                    + "stopped-but-unsaved recording is still pending, since starting one "
                    + "throws those steps away.",
                [
                    "discard": [
                        "type": "boolean",
                        "description": "Throw away a pending unsaved recording and start a "
                            + "new one anyway. Default false.",
                    ]
                ]),
            tool(
                "stop_recording",
                "Stops recording and returns the steps. Mirrors File > Automate > Stop "
                    + "Recording. Pass name to save them as an Action; omit it to read "
                    + "the steps back without saving.",
                [
                    "name": [
                        "type": "string",
                        "description": "Save the recording under this name; omit to "
                            + "return the steps without saving.",
                    ]
                ]),
        ]
    }
}
