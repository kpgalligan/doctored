import Foundation

/// The Phase 12B catalog entries — `reorder_layers`, the three batch tools and
/// the two bundle tools — in their own file behind an explicit non-inlining
/// boundary, for the reason `AgentCatalog.swift`'s header gives.
extension AgentServer {
    @inline(never) func reusableActionsCatalog() -> [[String: Any]] {
        [
            tool(
                "reorder_layers",
                "Moves SEVERAL layers to one place in the stack — the layers panel's "
                    + "multi-row drag. One undo step. `at` is the insertion index in the "
                    + "CURRENT numbering (the drop line: 0 is the bottom, layer_count is "
                    + "above everything), NOT reorder_layer's `to`, which numbers the stack "
                    + "with the moved subtree already taken out. Entries keep their relative "
                    + "order and their subtrees, and every one lands at `depth`. Refused when "
                    + "`at` falls inside one of the moving subtrees, and when the (at, depth) "
                    + "pair would not leave a well-formed tree — an entry at depth d needs an "
                    + "enclosing group, and the topmost entry sits at depth 0. Returns "
                    + "`landed`, the new index of each entry in the order given.",
                [
                    "layers": [
                        "type": "array", "items": ["type": "integer"],
                        "description": "Layer indices to move; duplicates are refused. An "
                            + "entry that already travels inside another named GROUP should "
                            + "be left out.",
                    ],
                    "at": [
                        "type": "integer",
                        "description": "Insertion index in the current numbering (0 … "
                            + "layer_count).",
                    ],
                    "depth": [
                        "type": "integer",
                        "description": "Level everything lands at; omit for the level the "
                            + "entry below the line sits in.",
                    ],
                    "document_id": AgentServer.docIDProperty,
                ], required: ["layers", "at"]),
            tool(
                "run_batch",
                "Plays an action over every image in a FOLDER, writing the results into "
                    + "another folder — the Batch window, headless. Returns a batch_id "
                    + "immediately; poll batch_status for progress and the per-file report, "
                    + "and cancel_batch to stop at the next file boundary. ONE batch runs at "
                    + "a time app-wide, and while it runs every other tool is refused in band "
                    + "except batch_status, cancel_batch, job_status, cancel_job and "
                    + "resource_usage. Naming: {name} is the input's basename and {n} a "
                    + "1-based number zero-padded to the file count's width; the extension is "
                    + "always the chosen format's. {n}, and a preset pattern's {index}, are "
                    + "positions in THIS run's file list, so re-running over a folder that "
                    + "has gained or lost files renumbers the outputs instead of colliding "
                    + "with the earlier run's — the skips below cannot catch that. "
                    + "A name that would land outside the output "
                    + "folder, one a previous file in the run already claimed, and an "
                    + "existing file without overwrite are each SKIPPED with a reason — a "
                    + "batch never writes over a file it did not create in this run. "
                    + "stop_on_error (default true) halts the run at the first failed step "
                    + "AND leaves that file unwritten; false writes each file in whatever "
                    + "state the action left it and lists the failures against it. With "
                    + "`preset` the delivery goes through quick_export instead, and the "
                    + "preset's folder, pattern and options replace output/name_template/"
                    + "overwrite; the preset's name is expanded before each encode, and a "
                    + "file that already exists is SKIPPED with a reason too — never "
                    + "overwritten, and never a halt, even with stop_on_error. A parameter "
                    + "that will not bind, and a preset whose destination folder does not "
                    + "exist, are refused HERE, before any file is opened, with no batch_id. "
                    + "A preset naming a profile that cannot be exported into (describe_profile's "
                    + "export_refusal) is decided per file at run time: a file not already in "
                    + "that profile fails its row with that sentence and is not written, while "
                    + "a file already in it exports unchanged under a non-absolute intent "
                    + "without black_point_compensation.",
                [
                    "name": [
                        "type": "string", "description": "Action to play on each file.",
                    ],
                    "input": [
                        "type": "string",
                        "description": "Folder of images, top level only — a batch never "
                            + "recurses.",
                    ],
                    "output": [
                        "type": "string",
                        "description": "Folder the results are written into; created if "
                            + "missing, and it must not be the input folder. Ignored (and "
                            + "unnecessary) with `preset`.",
                    ],
                    "format": [
                        "type": "string",
                        "enum": ExportFormat.flatSaveCases.map { $0.fileExtension },
                        "description": "Output format; default png. Ignored with `preset`.",
                    ],
                    "name_template": [
                        "type": "string",
                        "description": "Output name pattern, {name} and {n}; default "
                            + "\"{name}\". No \"/\" or \":\", and no extension.",
                    ],
                    "overwrite": [
                        "type": "boolean",
                        "description": "Replace an existing output file; default false, "
                            + "which skips it with a reason.",
                    ],
                    "stop_on_error": [
                        "type": "boolean",
                        "description": "Halt at the first failed step and leave that file "
                            + "unwritten; default true.",
                    ],
                    "preset": [
                        "type": "string",
                        "description": "Export preset to deliver through (list_export_"
                            + "presets). A batch never overwrites and never suffixes: a file "
                            + "that already exists is skipped, not a halt (the -2, -3, … names "
                            + "are quick_export's own interactive form, not a batch's). Its "
                            + "destination folder must exist, or the "
                            + "run is refused with no batch_id.",
                    ],
                    "metadata_policy": [
                        "type": "string",
                        "description": "Named metadata policy overriding the preset's own "
                            + "(list_metadata_policies). Preset mode only.",
                    ],
                    "parameters": [
                        "type": "object",
                        "description": "Values for the action's declared parameters "
                            + "(list_actions {name} reports them). Refused at submission if "
                            + "a required one is missing or a value is the wrong type.",
                    ],
                    "jpeg_quality": [
                        "type": "integer",
                        "description": "0–100, JPEG only; default 90.",
                    ],
                    "embed_profile": [
                        "type": "boolean", "description": "Write the ICC profile; default true.",
                    ],
                    "strip_metadata": [
                        "type": "boolean",
                        "description": "Drop EXIF/XMP/IPTC; default false.",
                    ],
                ], required: ["name", "input"]),
            tool(
                "batch_status",
                "How the running batch — or the last one that finished in this session — is "
                    + "getting on: running, processed, total, and the full report with one "
                    + "row per file (wrote / skipped / failed, each with its reason). One of "
                    + "the few tools that answers WHILE a batch runs.",
                [
                    "batch_id": [
                        "type": "string",
                        "description": "Optional; only one batch runs at a time, but passing "
                            + "the id refuses an answer about a different run.",
                    ],
                ]),
            tool(
                "cancel_batch",
                "Stops the running batch at the next file boundary. The file in flight "
                    + "finishes — a core call cannot be interrupted, and abandoning a "
                    + "half-written output is worse than one more file. Poll batch_status "
                    + "for the final report.",
                [
                    "batch_id": [
                        "type": "string", "description": "Optional; see batch_status.",
                    ],
                ]),
            tool(
                "package_action",
                "Writes an action and the FILES it uses into a <slug>.draction bundle — the "
                    + "only way to make an action somebody else can run. Each file argument "
                    + "(place_image.path, add_live_photo_layer.path, and "
                    + "convert_profile/assign_profile.path when profile is \"file\") is "
                    + "copied into the bundle's assets/ folder and rewritten IN THE BUNDLE to "
                    + "{\"$asset\": \"<basename>\"}; the action in the library is not "
                    + "modified. Two DIFFERENT source paths sharing a basename become "
                    + "logo.png and logo-2.png, each step pointing at its own; the same path "
                    + "used twice is one entry. A missing source file refuses the whole call, "
                    + "naming the path, and leaves no partial bundle. quick_export's "
                    + "destination is deliberately NOT an asset — it is a per-machine output "
                    + "folder, not an input.",
                [
                    "name": ["type": "string", "description": "Action to package."],
                    "destination": [
                        "type": "string",
                        "description": "FOLDER the bundle is written into; the bundle itself "
                            + "is named after the action's slug.",
                    ],
                    "overwrite": [
                        "type": "boolean",
                        "description": "Replace an existing bundle of that name; default "
                            + "false, which refuses.",
                    ],
                ], required: ["name", "destination"]),
            tool(
                "import_action",
                "Installs a <slug>.draction bundle into the action library: the action, then "
                    + "its assets/ files beside it, so every {\"$asset\": …} step resolves. "
                    + "The bundle's own decode refusals surface verbatim, by step number. "
                    + "`name` renames it on the way in. Without overwrite a name that is "
                    + "already taken refuses rather than replacing an action somebody built; "
                    + "with it, both the action and its assets folder are replaced.",
                [
                    "path": [
                        "type": "string",
                        "description": "The .draction FOLDER (it holds action.json and "
                            + "assets/).",
                    ],
                    "name": [
                        "type": "string",
                        "description": "Install under this display name instead of the "
                            + "bundle's own.",
                    ],
                    "overwrite": [
                        "type": "boolean",
                        "description": "Replace a same-named action and its assets; default "
                            + "false.",
                    ],
                ], required: ["path"]),
        ]
    }
}
