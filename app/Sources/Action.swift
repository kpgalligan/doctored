import AppKit

// `Action` is schema 2 and reads 1 unchanged, but `fileSchemaVersion` WRITES 1 whenever the action
// uses no schema-2 construct — that is what keeps an Enabled toggle on a 12A recording rewriting
// the same bytes.
//
// The Actions contract is `designs/phase-12b-actions-status.md` (drive gates B1–B7 / P1–P9:
// `archive/phase-12b-actions-status.md`).

/// An Action: a named, replayable list of MCP tool calls.
///
/// The model and its file format only. Where the steps come from is
/// `ActionSteps.swift` (the recorded vocabulary) and `ActionRecorder`; what
/// running one means is `ActionPlayer`; where they live on disk is
/// `ActionLibrary`.
///
/// An action is a SIDECAR, not document state: nothing here is persisted in
/// a `.rz`, so the phase bumps no `DRDC_VERSION` and the core knows nothing
/// about actions.

/// The one error type this feature reports with: a sentence meant for a
/// human, whether it reaches them through the Actions window, an MCP
/// `isError` reply or a Batch report row.
///
/// It exists because `Result` requires its failure to be an `Error` and the
/// natural spelling here is a bare message — the alternative, conforming
/// `String` to `Error`, would be a module-wide side effect for one feature's
/// convenience.
struct ActionError: LocalizedError {
    let message: String
    /// What the interactive Play's prompt calls this failure.
    let kind: ActionStepFailureKind
    /// The missing object's phrase (`layer “Sky”`) when `kind` is
    /// `.missingObject`.
    let object: String?
    var errorDescription: String? { message }

    init(_ message: String, kind: ActionStepFailureKind = .unavailable, object: String? = nil) {
        self.message = message
        self.kind = kind
        self.object = object
    }
}

// MARK: - One step

/// One recorded call: a catalog tool name plus the exact arguments it is
/// replayed with.
///
/// `document_id` never appears — ids are session-scoped integers minted by
/// `AgentServer.id(for:)`, so the player rebinds the argument to whatever
/// document the run targets.
struct ActionStep {
    /// The sentinel tool for a user command that HAS no MCP twin (Paste).
    /// It is
    /// not a handler key: it exists so the gap is VISIBLE in the action —
    /// listed in the window, refused with a reason at replay — instead of
    /// being a silent hole in a recording that otherwise looks complete.
    static let unrecordedTool = "__unrecorded"

    /// What a failing step does to the rest of the run. Spelled `continue`
    /// in the file; `continueRun` in Swift, where `continue` is a keyword.
    enum OnError: String {
        case stop
        case continueRun = "continue"
    }

    var tool: String
    var arguments: [String: Any]
    var enabled: Bool
    var onError: OnError
    /// How this step's CANVAS COORDINATES are read on a document of a
    /// different size — absolute (the 12A behaviour and the default),
    /// proportional, or pinned to a canvas anchor. Omitted from the file when
    /// it is `.absolute`, so a schema-1 recording and a schema-2 one are the
    /// same bytes for the same step.
    ///
    /// It is an EDIT, never a recording: the recorder writes what the gesture
    /// did, and choosing how a step should travel is a decision about reuse
    /// that only the person reusing it can make (Actions window ▸ row menu ▸
    /// Geometry).
    var geometry: GeometryMode
    /// Free text the recorder writes for the reader's benefit — the UI path
    /// this step mirrors, or a caveat (a recorded Colour Lookup layer says it
    /// carries the whole LUT, because the layer keeps no path to the .cube).
    /// Ignored on replay.
    var note: String?

    init(
        tool: String, arguments: [String: Any] = [:], enabled: Bool = true,
        onError: OnError = .stop, geometry: GeometryMode = .absolute, note: String? = nil
    ) {
        // Sanitised here, once, so `Action.encoded()` cannot fail: every
        // factory builds from JSON primitives already, and a value that is
        // not one would otherwise become an unwritable action file.
        //
        // ASKED before it is done, because doing it is what cost: `jsonSafe`
        // REBUILDS the whole tree, bridging every coordinate to an NSNumber
        // only to hand back the value it was given, and the common case has
        // nothing to fix. Measured on a 10,000-point stroke's arguments:
        // 15.4 ms to rebuild, 4.7 ms to ask. The question is exactly the
        // right one — `isValidJSONObject` documents the same conditions this
        // sanitiser enforces, and was verified to answer false for a NaN, an
        // infinity (nested in an array included), an NSObject, a Date and a
        // CGPoint.
        self.init(
            tool: tool,
            validated: JSONSerialization.isValidJSONObject(arguments)
                ? arguments : ActionStep.jsonSafe(arguments),
            enabled: enabled, onError: onError, geometry: geometry, note: note)
    }

    /// The DECODE path's init: the arguments are a tree `JSONSerialization`
    /// itself produced and `Action.argumentProblem` has already walked, so
    /// asking `isValidJSONObject` about it would be the same walk a second
    /// time. That second walk was 4.7 ms per 10,000-point stroke — a
    /// twenty-stroke recording paid it twenty times on every load — and it
    /// could never answer anything: a parsed tree holds only JSON primitives,
    /// and the one value in it that `isValidJSONObject` would reject is a
    /// non-finite number, which the walk above refuses by path instead.
    init(
        tool: String, validated arguments: [String: Any], enabled: Bool = true,
        onError: OnError = .stop, geometry: GeometryMode = .absolute, note: String? = nil
    ) {
        // The drift guard for a hand-written factory in ActionSteps.swift:
        // nothing enforces that a recorded tool name is a real one, and a
        // typo would only surface when somebody replayed the recording. The
        // same idiom as the startup catalog-parity check, and possible for
        // the same reason — `AgentServer.handlers` is internal.
        assert(Action.isKnownStepTool(tool), "recorded step names no such tool: \(tool)")
        self.tool = tool
        self.arguments = arguments
        self.enabled = enabled
        self.onError = onError
        self.geometry = geometry
        self.note = note
    }

    /// True for the `__unrecorded` placeholder, which can never run.
    var isUnrecorded: Bool { tool == Self.unrecordedTool }

    /// The command name an `__unrecorded` placeholder stands for.
    var unrecordedName: String { arguments["action"] as? String ?? "an unrecorded command" }

    /// This step as the JSON object the file holds.
    func jsonObject() -> [String: Any] {
        var object: [String: Any] = ["tool": tool, "arguments": arguments]
        // `enabled` and `on_error` are written only when they differ from
        // the default, so a plain recording reads as a plain list.
        if !enabled { object["enabled"] = false }
        if onError != .stop { object["on_error"] = onError.rawValue }
        // Absolute is the default AND what every build before schema 2 did,
        // so omitting it keeps an unedited recording byte-identical.
        if geometry != .absolute { object["geometry"] = geometry.jsonObject }
        if let note = note { object["note"] = note }
        return object
    }

    /// `value` with everything JSONSerialization cannot write removed.
    /// Numbers, strings, booleans, nulls, arrays and string-keyed objects
    /// survive; anything else — and any non-finite number — is dropped
    /// rather than silently written as a description of itself.
    ///
    /// **NSNumber is tested before anything numeric, and the finite check
    /// lives in that case**, because in Swift a `Double`, a `CGFloat` and an
    /// `Int` all satisfy `is NSNumber` — so an `is NSNumber` case ahead of
    /// them (which is what this was) let an infinity straight through and
    /// made the `.isFinite` branches below it dead code. That matters more
    /// than the usual "drop what we cannot write": `JSONSerialization`
    /// RAISES an ObjC exception on a non-finite number rather than throwing,
    /// which no `try?` in Swift can catch, so one infinity in an action's
    /// arguments would terminate the process on the next save, edit or run.
    /// Foundation's own JSON parser produces one from a negative overflow
    /// literal (`-1e999`), so a hand-written action file really can carry it.
    private static func jsonSafe(_ value: Any) -> Any? {
        switch value {
        case is NSNull, is String, is Bool:
            // `is Bool` first, and the value passed through untouched: a
            // JSON boolean is an `__NSCFBoolean` that must stay one.
            return value
        case let number as NSNumber:
            return number.doubleValue.isFinite ? value : nil
        case let array as [Any]:
            return array.compactMap { jsonSafe($0) }
        case let object as [String: Any]:
            return jsonSafe(object)
        default:
            return nil
        }
    }

    private static func jsonSafe(_ object: [String: Any]) -> [String: Any] {
        var out: [String: Any] = [:]
        for (key, value) in object {
            if let kept = jsonSafe(value) { out[key] = kept }
        }
        return out
    }
}

// MARK: - Symbolic arguments

/// Telling a JSON BOOLEAN from the numbers 0 and 1.
///
/// **`value is Bool` cannot.** Foundation bridges an `NSNumber` holding 0 or 1
/// to `Bool` conditionally, so `(0 as NSNumber) is Bool` is TRUE. A guard
/// written the obvious way — `let n = value as? NSNumber, !(n is Bool)` —
/// therefore silently drops the two most common integers in the catalog: an
/// `x` of 0, an `id` of 1, a parameter value of 0 or 1. Measured: it made a
/// `crop` at x = 0 skip its geometry remap entirely, which looked correct
/// under `relative` (0 scales to 0) and was wrong under every anchor.
///
/// The question that actually distinguishes them is the CoreFoundation type
/// id, which only `true` and `false` answer to.
enum ActionJSON {
    static func isBoolean(_ value: Any) -> Bool {
        CFGetTypeID(value as CFTypeRef) == CFBooleanGetTypeID()
    }

    /// `value` as a JSON NUMBER — nil for a boolean, a null, a string and
    /// anything else.
    static func number(_ value: Any?) -> NSNumber? {
        guard let value = value, !isBoolean(value) else { return nil }
        return value as? NSNumber
    }
}


/// The ENTIRE symbol vocabulary that lets a step recorded on one document do
/// the analogous thing on another.
///
/// A symbol is a JSON object with exactly one key, beginning `$`. Anything
/// else is a literal, so a caller's own `{"$layer": …}`-shaped data (there is
/// none in the catalog today) could never be misread as one.
///
/// **Schema 1 had three forms; schema 2 adds four**, and every schema-1
/// spelling still means exactly what it meant: `{"$layer": "Sky"}` is still a
/// NAME, not an id. The additions are the object form of `$layer` (name AND
/// stable id — see `ActionIdentity`), its `$channel` twin, `$param` for a
/// declared parameter, and `$asset` for a file that travels in the action's
/// bundle.
///
/// What is deliberately NOT symbolic: canvas geometry (x, y, width, points,
/// offsets, guide positions) is still recorded as ABSOLUTE canvas pixels.
/// Proportional remapping is right for a crop and wrong for a 12 px text
/// baseline, and guessing which is which is how an Action silently produces a
/// picture nobody asked for — so the choice is an EDIT a person makes on a
/// step (`ActionStep.geometry`, `ActionGeometry`), never something the
/// recorder decides. `Action.recordedCanvas` still carries the size the
/// recording was made at, and an absolute step on a differently sized document
/// still earns the same warning. Select All is the one case where that would
/// have been a real trap, and it is fixed at the source: it records
/// `select_all`, which is canvas-relative by construction.
enum ActionSymbol {
    /// `{"$layer": "active"}` — whatever layer is active on the replay
    /// document. Cannot fail.
    case activeLayer
    /// `{"$layer": "Sky"}` — the TOPMOST layer with that exact name. The
    /// schema-1 spelling, unchanged.
    case layerNamed(String)
    /// `{"$layer": {"name": "Sky", "id": 12, "session": "…"}}` — the layer
    /// that carried that id when the step was recorded, falling back to the
    /// topmost layer of that name. The id is honoured only when `session`
    /// matches this launch's, because core ids are not persisted
    /// (`ActionIdentity`).
    case layerRef(LayerRef, session: String)
    /// `{"$channel": {"name": "Alpha 1", "id": 7, "session": "…"}}` — the same
    /// pair for a document channel, resolving to a channel INDEX.
    case channelRef(ChannelRef, session: String)
    /// `{"$param": "radius"}` — the value bound for that declared parameter
    /// (`ActionParameters`). Resolved BEFORE every other symbol and before the
    /// geometry remap, so a parameter can supply a coordinate.
    case parameter(String)
    /// `{"$asset": "logo.png"}` — a file inside this action's assets folder.
    /// One path component, no leading dot, so a bundle can never name
    /// anything outside itself.
    case asset(String)
    /// `{"$layers": "selected"}` — the replay document's layer selection,
    /// sorted and duplicate-free (`AgentServer.layerIndices` refuses
    /// duplicates and sorts, so the recorder must not hand it anything else).
    case selectedLayers
    /// `{"$guide": {"orientation": "horizontal", "position": 412}}` — the
    /// guide at that position, resolved to a live index at replay. Recording
    /// the positional index instead would delete a DIFFERENT guide after any
    /// earlier guide edit in the same action.
    case guideAt(orientation: String, position: Double)

    /// True when `value` has a symbol's SHAPE — one key, beginning `$` —
    /// whether or not its contents parse. Shape and validity are separate so
    /// a malformed symbol is refused by name instead of being read as a
    /// literal dictionary.
    static func looksLikeSymbol(_ value: Any) -> Bool {
        guard let object = value as? [String: Any], object.count == 1,
              let key = object.keys.first
        else { return false }
        return key.hasPrefix("$")
    }

    /// Parses a symbol-shaped object, or returns the reason it is not one of
    /// the three forms — the text that becomes part of a decode refusal.
    static func parse(_ value: Any) -> Result<ActionSymbol, ActionError> {
        guard let object = value as? [String: Any], let key = object.keys.first,
              let payload = object[key]
        else {
            return .failure(ActionError("a symbol must be an object with one $-prefixed key"))
        }
        switch key {
        case "$layer":
            // The OBJECT form is tried first, so the string form's refusal
            // stays the sentence it always was for anything that is not one.
            if let fields = payload as? [String: Any] {
                switch reference(fields, key: key) {
                case .success(let parsed):
                    return .success(
                        .layerRef(
                            LayerRef(name: parsed.name, id: parsed.id), session: parsed.session))
                case .failure(let error): return .failure(error)
                }
            }
            guard let name = payload as? String, !name.isEmpty else {
                return .failure(
                    ActionError(
                        "$layer must be \"active\", a layer name, or an object with "
                            + "\"name\", \"id\" and \"session\""))
            }
            return .success(name == "active" ? .activeLayer : .layerNamed(name))
        case "$channel":
            guard let fields = payload as? [String: Any] else {
                return .failure(
                    ActionError(
                        "$channel must be an object with \"name\", \"id\" and \"session\""))
            }
            switch reference(fields, key: key) {
            case .success(let parsed):
                return .success(
                    .channelRef(
                        ChannelRef(name: parsed.name, id: parsed.id), session: parsed.session))
            case .failure(let error): return .failure(error)
            }
        case "$param":
            guard let name = payload as? String, ActionParameter.isValidName(name) else {
                return .failure(
                    ActionError(
                        "$param must be a declared parameter's name (a letter or underscore "
                            + "followed by letters, digits or underscores)"))
            }
            return .success(.parameter(name))
        case "$asset":
            guard let file = payload as? String, ActionAssets.isValidFileName(file) else {
                return .failure(
                    ActionError(
                        "$asset must be one path component inside the action's assets folder "
                            + "— no \"/\", no \"..\", no leading dot"))
            }
            return .success(.asset(file))
        case "$layers":
            guard let which = payload as? String, which == "selected" else {
                return .failure(ActionError("$layers must be \"selected\""))
            }
            return .success(.selectedLayers)
        case "$guide":
            guard let fields = payload as? [String: Any],
                  let orientation = fields["orientation"] as? String,
                  orientation == "horizontal" || orientation == "vertical",
                  let position = (fields["position"] as? NSNumber)?.doubleValue,
                  position.isFinite
            else {
                return .failure(
                    ActionError(
                        "$guide must be an object with \"orientation\" (\"horizontal\" or "
                            + "\"vertical\") and a numeric \"position\""))
            }
            return .success(.guideAt(orientation: orientation, position: position))
        default:
            return .failure(ActionError("unknown symbol \"\(key)\""))
        }
    }

    /// The `{"name", "id", "session"}` triple both reference forms take,
    /// parsed once so `$layer`'s object form and `$channel` cannot drift.
    ///
    /// `id` is read as an NSNumber and refused when negative or fractional: a
    /// core id is a `uint64_t`, and a file claiming `-1` or `1.5` is a
    /// hand-edit that would otherwise resolve to nothing with no explanation.
    private static func reference(
        _ fields: [String: Any], key: String
    ) -> Result<(name: String, id: UInt64, session: String), ActionError> {
        guard let name = fields["name"] as? String, !name.isEmpty,
              let session = fields["session"] as? String, !session.isEmpty,
              let raw = ActionJSON.number(fields["id"]),
              let id = UInt64(exactly: raw.doubleValue), id != 0
        else {
            return .failure(
                ActionError(
                    "\(key) as an object needs a non-empty \"name\", a positive whole "
                        + "\"id\" and a non-empty \"session\""))
        }
        return .success((name: name, id: id, session: session))
    }

    /// This symbol as the JSON object a step holds.
    var jsonObject: [String: Any] {
        switch self {
        case .activeLayer: return ["$layer": "active"]
        case .layerNamed(let name): return ["$layer": name]
        case .layerRef(let ref, let session):
            return ["$layer": ["name": ref.name, "id": ref.id, "session": session]]
        case .channelRef(let ref, let session):
            return ["$channel": ["name": ref.name, "id": ref.id, "session": session]]
        case .parameter(let name): return ["$param": name]
        case .asset(let file): return ["$asset": file]
        case .selectedLayers: return ["$layers": "selected"]
        case .guideAt(let orientation, let position):
            return ["$guide": ["orientation": orientation, "position": position]]
        }
    }

    /// True for the four forms schema 1 has no spelling for. The writer asks
    /// it of every argument so an action that uses none of them is still
    /// written as a schema-1 file (`Action.fileSchemaVersion`).
    var isSchema2: Bool {
        switch self {
        case .layerRef, .channelRef, .parameter, .asset: return true
        case .activeLayer, .layerNamed, .selectedLayers, .guideAt: return false
        }
    }
}

// MARK: - The action

/// A named list of steps, plus the two things a replay needs to know about
/// the session that produced it.
struct Action {
    /// The newest schema version this build writes and reads. A reader
    /// refuses an unknown one by name rather than guessing at a newer shape;
    /// every version in [`readableSchemaVersions`] is accepted.
    ///
    /// **A file is written at the LOWEST version that can hold it**
    /// (`fileSchemaVersion`): an action with no parameters, no non-absolute
    /// geometry and no schema-2 symbol is still written as `1`, so re-saving
    /// an untouched 12A recording rewrites the same bytes and an older build
    /// can still read it.
    static let schemaVersion = 2

    /// Every version this build can read. Schema 1 decodes UNCHANGED: a
    /// `{"$layer": "Sky"}` in a schema-1 file is still a name, an absent
    /// `geometry` is still absolute, and a `set_layer_properties` offset step
    /// still replays exactly as it did.
    static let readableSchemaVersions: Set<Int> = [1, 2]

    /// The key an action file names its schema version under. Every writer
    /// emits it; a reader takes it, else `legacyFileKey`.
    static let fileKey = "doctored_action"

    /// The key files written before the rename (R2) carry; read, never written.
    static let legacyFileKey = "rasterize_action"

    /// The display name, and the authority: the file name is only a slug of
    /// it (`ActionLibrary.slug`).
    var name: String
    var steps: [ActionStep]
    /// The action's declared parameters — the names a caller may supply and
    /// a `{"$param": …}` symbol may reference (`ActionParameters`). Empty for
    /// every schema-1 action and for every recording, since declaring one is
    /// an edit.
    var parameters: [ActionParameter]
    /// The canvas the recording was made on — the FIRST step's size, which is
    /// what 12A latched and what this phase deliberately did not change.
    ///
    /// It is reported on a size mismatch, and it is where a non-absolute
    /// step's remap arithmetic STARTS: the player carries a running recorded
    /// space from here, advancing it past every canvas-changing step
    /// (`ActionGeometry.canvasAfter`). An absolute step still uses it for
    /// nothing but the warning.
    var recordedCanvas: (width: Int, height: Int)?
    /// The develop settings Batch opens camera RAW files with. Nil is "as
    /// shot". It belongs to the ACTION and not to a step because Batch does
    /// the opening itself; an `open_document` step inside an action carries
    /// its own `raw` in its own arguments and Batch never touches it.
    var raw: RawDevelopSettings?
    var created: Date?
    var modified: Date?

    init(
        name: String, steps: [ActionStep], parameters: [ActionParameter] = [],
        recordedCanvas: (width: Int, height: Int)? = nil,
        raw: RawDevelopSettings? = nil, created: Date? = nil, modified: Date? = nil
    ) {
        self.name = name
        self.steps = steps
        self.parameters = parameters
        self.recordedCanvas = recordedCanvas
        self.raw = raw
        self.created = created
        self.modified = modified
    }

    /// The Actions machinery's own tools, and the one tool that writes a
    /// file: a STEP may never name any of them.
    ///
    /// `run_action` is the one that matters: it is a live dispatch key, so
    /// without this an action could name itself (or two could name each
    /// other) and the player would recurse — `ActionPlayer.run` →
    /// `AgentServer.execute` → `runAction` → `ActionPlayer.run` — until the
    /// stack overflowed and the app was killed, taking every open document's
    /// unsaved work with it. The other five are here for coherence rather
    /// than for safety: an action that records, saves or deletes actions
    /// while it runs edits the library out from under the run.
    ///
    /// **`save_copy` is here for containment.** A step's path is an
    /// ABSOLUTE one recorded when the action was written, and Batch's own
    /// guards — the output-folder identity test, the `claimed` set, the
    /// exists/overwrite check — cover only the destination Batch computes.
    /// So a single `save_copy` step ran over a folder of 300 files wrote 300
    /// times to that one path, outside the output folder the user chose,
    /// with every report row still reading `ok`; aimed at a file in the INPUT
    /// folder it destroyed that input on the first file and rewrote it 299
    /// times. Batch does the saving — it owns the folder, the format and the
    /// naming rule — and an action says only what to DO to the picture.
    ///
    /// **`undo` and `redo` are here because `run_action` and Batch play an
    /// action as ONE undo entry.** `ImageDocument.withReplayUndo` disables
    /// undo registration for the whole of such a run, so the redo
    /// registration `restoreDoc` makes while NSUndoManager is undoing is
    /// dropped on the floor: measured, an action
    /// whose single step was `undo` reported `ok`, removed the top layer and
    /// left "Nothing to redo" behind, while the identical `undo` tool called
    /// directly kept redo working. The same bracket also swallows
    /// `restoreDoc`'s `.changeUndone`, so the run left the change count one
    /// edit high for a run whose net effect was an undo. Neither is
    /// something a step can be made to do correctly — an entry cannot
    /// contain itself — so the pair is refused by name, which is also what
    /// `save_action`'s catalog description tells a model.
    ///
    /// Refused at DECODE time, so a hand-edited file, `save_action` and the
    /// window's JSON editor all report it by step index instead of writing a
    /// file that crashes the app, or overwrites someone's originals, the
    /// moment anything runs it. `ActionCatalogFacts.nonRecordable` already
    /// keeps them out of a recording; this is the half that keeps them out
    /// of a file.
    static let notAStepTool: Set<String> = [
        "save_raw_preset", "delete_raw_preset", "repair_raw_preset", "sync_raw_settings", "apply_raw_preset",
        "preview_raw_decoder_migration",
        "copy_raw_local_settings", "get_raw_local_mask",
        "list_actions", "run_action", "save_action", "delete_action",
        "start_recording", "stop_recording", "save_copy", "undo", "redo",
        "export_psd",
        // Phase 12B. `run_batch` is `run_action`'s reason exactly — a batch
        // plays an action, so an action that named one would nest a replay
        // per file and recurse — and the other four edit the library or write
        // a bundle to a literal path while the run is using it.
        "run_batch", "batch_status", "cancel_batch", "package_action", "import_action",
        // The export workspace's file writer and its preset-library writes.
        // `quick_export` is deliberately NOT here: see `notAStepReason`.
        "export_image", "save_export_preset", "delete_export_preset",
        "set_default_export_preset",
        // Phase 11B: the sheet built from FILES or other documents has no
        // single document to replay against, and the policy library's writes
        // are preferences like the preset library's beside them.
        // `export_layers` and `contact_sheet` ARE steps.
        "contact_sheet_from_sources", "save_metadata_policy", "delete_metadata_policy",
        // Phase 11D: `export_pdf` writes to a literal path, exactly as
        // `export_image` does, and `inspect_vector` only reads a file.
        "export_pdf", "inspect_vector",
        // Phase 12C: the New Document preset library's writes are
        // preferences, like every other library's beside them.
        // `new_document` is deliberately NOT here — an action MAY create a
        // document deliberately, exactly as `open_document` may.
        "save_document_preset", "delete_document_preset",
        // Phase 12C-2: a workspace layout is window furniture held in a
        // mutable library, and restoring one changes no document.
        "list_workspaces", "save_workspace", "restore_workspace", "delete_workspace",
        // The proof-preset library — a mutable library name over view
        // state that no step records.
        "list_proof_presets", "save_proof_preset", "delete_proof_preset",
        "set_default_proof_preset",
        // An action replays on a document that stays open; see `notAStepReason`.
        "close_document",
    ]

    /// Why `tool` may not be a step, in a sentence that goes on the end of
    /// `"step N: \"tool\" cannot be a step — "`; nil when it may.
    ///
    /// Three reasons, kept apart because a reader who is told the wrong one
    /// goes looking for the wrong fix.
    static func notAStepReason(_ tool: String) -> String? {
        guard notAStepTool.contains(tool) else { return nil }
        if tool == "copy_raw_local_settings" {
            return "record resolved target controls with edit_raw_development; source-document selection belongs to the current session"
        }
        if tool == "get_raw_local_mask" { return "native mask export writes an explicit file; keep exports outside Actions replay" }
        if ["save_raw_preset", "delete_raw_preset", "repair_raw_preset", "sync_raw_settings",
            "apply_raw_preset"].contains(tool) {
            return "record resolved RAW settings with apply_raw_settings; presets and open-document targets belong to the current session"
        }
        if tool == "contact_sheet_from_sources" {
            return "a sheet built from files or other open documents has no single document to "
                + "replay against — record contact_sheet, which composes the document's own items"
        }
        if ["save_document_preset", "delete_document_preset"].contains(tool) {
            return "a document preset is a mutable library name; record new_document with the "
                + "resolved size, profile, depth and background instead"
        }
        if ["list_workspaces", "save_workspace", "restore_workspace",
            "delete_workspace"].contains(tool) {
            return "a workspace layout is window furniture in a mutable library — it changes no "
                + "document, and a step naming one would not replay the layout it was authored "
                + "against"
        }
        if ["list_proof_presets", "save_proof_preset", "delete_proof_preset",
            "set_default_proof_preset"].contains(tool) {
            return "a proof preset is a mutable library name and proofing is view state — "
                + "record nothing; set_proof_setup with explicit fields replays"
        }
        if ["save_metadata_policy", "delete_metadata_policy"].contains(tool) {
            return "a recorded export carries the RESOLVED metadata policy, so replay never "
                + "depends on a mutable library name"
        }
        if ["save_export_preset", "delete_export_preset", "set_default_export_preset"].contains(tool) {
            return "record a Quick Export instead: it carries the resolved settings, destination "
                + "and pattern, so replay never depends on a mutable library name"
        }
        if tool == "export_image" || tool == "export_pdf" {
            return "an action may not write to a path recorded here — record a Quick Export "
                + "instead: its name comes from a pattern that must contain {name} and it never "
                + "overwrites"
        }
        if tool == "inspect_vector" {
            return "a vector inspection reads a file and changes nothing"
        }
        if tool == "close_document" {
            return "an action replays on a document that stays open, so a step that closed it "
                + "would end the run's own target — and closing a document is the user's decision"
        }
        if ["run_batch", "batch_status", "cancel_batch"].contains(tool) {
            return "a batch PLAYS an action, so a step naming one would run this action "
                + "again for every file and recurse — run the batch over this action instead"
        }
        if ["package_action", "import_action"].contains(tool) {
            return "an action may not read or write the action library while it runs — "
                + "packaging and importing are library operations, not edits to a picture"
        }
        if tool == "preview_raw_decoder_migration" {
            return "review a decoder migration first; its recorded migration step retains the approved recipe and decoder identity"
        }
        if tool == "undo" || tool == "redo" {
            return "run_action and Batch play an action as ONE undo entry, so its steps' own "
                + "registrations are suppressed while it runs — an undo inside it would throw "
                + "the document's redo history away and report success. Undo the whole action "
                + "instead"
        }
        guard tool == "save_copy" || tool == "export_psd" else {
            return "an action may not run, record or edit actions"
        }
        let written = "an action may not write files. Its path would be the absolute one "
            + "recorded here, so every file a batch touched would be written over the "
            + "same one, outside the output folder — Batch chooses the folder, the "
            + "format and the name"
        // Quick Export is a substitute for save_copy alone: it writes ONE image
        // file, which is not what a layered PSD writes.
        guard tool == "save_copy" else { return written }
        return written + " — record a Quick Export instead: its name comes from "
            + "a pattern that must contain {name} and it never overwrites"
    }

    /// True for a tool a step may name: a live dispatch key that is not one
    /// of the Actions tools, or the `__unrecorded` placeholder.
    ///
    /// The placeholder has to be whitelisted here rather than refused at
    /// load: `stop_recording` produces steps containing it whenever the
    /// session touched a command with no twin, so a validator that rejected
    /// it would refuse the app's own recordings. The refusal belongs at RUN
    /// time, where it can name the command that was not recorded.
    static func isKnownStepTool(_ tool: String) -> Bool {
        guard !notAStepTool.contains(tool) else { return false }
        return tool == ActionStep.unrecordedTool || AgentServer.handlers.keys.contains(tool)
    }

    /// This action as a LISTING sees it — what `Action.decodeSummary` would
    /// have produced from the same file, so a decode the app already holds
    /// can seed the summary cache instead of being re-read out of it.
    var summary: ActionSummary {
        ActionSummary(
            name: name, stepCount: steps.count,
            disabledSteps: steps.filter { !$0.enabled }.count,
            recordedCanvas: recordedCanvas,
            // The summary mirrors the FILE, and `encoded()` omits an as-shot
            // develop entirely, so a decode of those bytes reports nil.
            raw: (raw?.isAsShot ?? true) ? nil : raw, created: created, modified: modified)
    }

    /// The LOWEST schema version that can hold this action: 2 when it
    /// declares a parameter, carries a non-absolute step, or uses one of the
    /// four schema-2 symbol forms; 1 otherwise.
    ///
    /// Written rather than always stamping 2 so that re-saving an untouched
    /// 12A recording — which every Enabled toggle and every drag-reorder in
    /// the Actions window does — produces the same bytes it did before, and
    /// so an action that uses nothing new stays readable by a build that
    /// predates this phase.
    var fileSchemaVersion: Int {
        guard parameters.isEmpty else { return Self.schemaVersion }
        for step in steps {
            if step.geometry != .absolute { return Self.schemaVersion }
            if Self.usesSchema2Symbol(step.arguments) { return Self.schemaVersion }
        }
        return 1
    }

    /// True when any value in the tree is one of the four schema-2 symbols.
    /// The same recursive shape `argumentProblem` walks, and asked only when
    /// a file is written.
    private static func usesSchema2Symbol(_ value: Any) -> Bool {
        if ActionSymbol.looksLikeSymbol(value) {
            guard case .success(let symbol) = ActionSymbol.parse(value) else { return false }
            return symbol.isSchema2
        }
        if let object = value as? [String: Any] {
            return object.values.contains { usesSchema2Symbol($0) }
        }
        if let array = value as? [Any] {
            return array.contains { usesSchema2Symbol($0) }
        }
        return false
    }

    /// The whole action as the JSON object its file holds.
    func jsonObject() -> [String: Any] {
        var object: [String: Any] = [
            Self.fileKey: fileSchemaVersion,
            "name": name,
            "steps": steps.map { $0.jsonObject() },
        ]
        if !parameters.isEmpty {
            object["parameters"] = parameters.map { $0.jsonObject() }
        }
        if let canvas = recordedCanvas {
            object["recorded_canvas"] = ["width": canvas.width, "height": canvas.height]
        }
        if let raw = raw, !raw.isAsShot { object["raw"] = raw.jsonObject() }
        if let created = created { object["created"] = Self.stamp.string(from: created) }
        if let modified = modified { object["modified"] = Self.stamp.string(from: modified) }
        return object
    }

    /// Past this many bytes an action is written compactly instead of
    /// pretty-printed.
    ///
    /// Pretty-printing is for the reader — the JSON editor, a diff — and a
    /// file this size has no reader: it is a recorded paint session, and the
    /// bulk of it is `[[x, y], …]` arrays that the printer puts on one line
    /// each. Measured: the same 150-stroke recording is 34.7 MB compact and
    /// 124.4 MB pretty-printed, and every Enabled toggle in the Actions
    /// window rewrites the whole file. 256 KB is well past any action a
    /// person would edit by hand and well below where the formatting starts
    /// to cost seconds.
    private static let prettyPrintLimit = 256 * 1024

    /// The bytes written to disk: sorted keys always — so re-saving an
    /// untouched action rewrites the same bytes — and pretty-printed while
    /// the result is small enough to be worth reading.
    func encoded() -> Data {
        let object = jsonObject()
        // `isValidJSONObject` first, exactly as `AgentServer.saveAction`
        // does: `data(withJSONObject:)` RAISES on a value it cannot write
        // rather than throwing, and a raise here — on a save, a reorder, an
        // Enabled toggle — would take the app down. `ActionStep.jsonSafe`
        // already makes that unreachable; this is the belt to its braces.
        guard JSONSerialization.isValidJSONObject(object),
              let compact = try? JSONSerialization.data(
                withJSONObject: object, options: [.sortedKeys])
        else {
            // Unreachable: `ActionStep.init` sanitises arguments to JSON
            // primitives, and every other field is a String, Int or Bool. An
            // empty object is still a decodable file that reports its own
            // problem ("doctored_action is missing") rather than a truncated
            // one.
            return Data("{}".utf8)
        }
        guard compact.count <= Self.prettyPrintLimit,
              let pretty = try? JSONSerialization.data(
                withJSONObject: object, options: [.prettyPrinted, .sortedKeys])
        else { return compact }
        return pretty
    }

    /// Parses one action file. The failure is the exact sentence shown to
    /// the user — in the Actions window beside a greyed entry, or as the
    /// in-band error of `save_action` / `run_action` — so every one of them
    /// names what is wrong and where.
    ///
    /// This is the FULL read, and it is not what a listing wants: see
    /// `decodeSummary`, which shares every check below but the one that walks
    /// a step's arguments.
    static func decode(_ data: Data) -> Result<Action, ActionError> {
        let envelope: Envelope
        switch Self.envelope(data) {
        case .success(let parsed): envelope = parsed
        case .failure(let reason): return .failure(reason)
        }
        var steps: [ActionStep] = []
        steps.reserveCapacity(envelope.rawSteps.count)
        for (index, entry) in envelope.rawSteps.enumerated() {
            switch decodeStep(entry, at: index) {
            case .success(let step): steps.append(step)
            case .failure(let reason): return .failure(reason)
            }
        }
        var action = Action(name: envelope.name, steps: steps)
        switch ActionParameter.decodeAll(envelope.object["parameters"]) {
        case .success(let declared): action.parameters = declared
        case .failure(let reason): return .failure(reason)
        }
        switch settings(envelope.object) {
        case .success(let settings):
            action.recordedCanvas = settings.canvas
            action.raw = settings.raw
            action.created = settings.created
            action.modified = settings.modified
        case .failure(let reason):
            return .failure(reason)
        }
        // Checked LAST, because it is the one refusal that needs both halves
        // of the file: a non-absolute step remaps against the canvas the
        // recording was made on, and without `recorded_canvas` there is no
        // space to remap FROM. Refusing here — by step number — beats
        // discovering it mid-run with the picture already changed.
        if let index = action.steps.firstIndex(where: { $0.geometry != .absolute }),
           action.recordedCanvas == nil
        {
            return .failure(
                ActionError(
                    "step \(index + 1): \"\(action.steps[index].tool)\" asks for "
                        + "\(action.steps[index].geometry.displayName) geometry, which remaps "
                        + "against the canvas the action was recorded on — this action has no "
                        + "recorded_canvas, so there is nothing to remap from"))
        }
        // …and a `$param` that names nothing is a typo the run would report
        // as a missing value on whichever step reached it first.
        if let problem = undeclaredParameter(action) { return .failure(problem) }
        return .success(action)
    }

    /// The first `{"$param": …}` in the file that names no declared
    /// parameter, as its own refusal.
    private static func undeclaredParameter(_ action: Action) -> ActionError? {
        let declared = Set(action.parameters.map { $0.name })
        for (index, step) in action.steps.enumerated() {
            guard let missing = firstParameterReference(step.arguments, outside: declared)
            else { continue }
            return ActionError(
                "step \(index + 1): {\"$param\": \"\(missing)\"} names no declared parameter"
                    + (declared.isEmpty
                        ? " — this action declares none"
                        : " (declared: " + declared.sorted().joined(separator: ", ") + ")"))
        }
        return nil
    }

    private static func firstParameterReference(
        _ value: Any, outside declared: Set<String>
    ) -> String? {
        if ActionSymbol.looksLikeSymbol(value) {
            guard case .success(.parameter(let name)) = ActionSymbol.parse(value) else {
                return nil
            }
            return declared.contains(name) ? nil : name
        }
        if let object = value as? [String: Any] {
            for key in object.keys.sorted() {
                if let found = firstParameterReference(object[key] as Any, outside: declared) {
                    return found
                }
            }
            return nil
        }
        if let array = value as? [Any] {
            for element in array {
                if let found = firstParameterReference(element, outside: declared) {
                    return found
                }
            }
        }
        return nil
    }

    /// Everything about a file EXCEPT its steps' arguments: the name, the
    /// step count, how many are disabled, and the three trailing fields.
    ///
    /// **The listing paths read this and never the whole thing.** A recorded
    /// paint session is almost entirely `[[x, y], …]`, and decoding those
    /// arrays to answer "what is this action called?" was the app's worst
    /// stall: measured on ten 3.1 MB recordings (one recorder budget each —
    /// 200,000 points), the first `ActionLibrary.load()` after launch cost
    /// **6.50 s of main-thread time** and left **186 MB** of decoded points
    /// resident for the life of the process. That load is reached from
    /// `validateMenuItem` every time File ▸ Automate opens, from the Actions
    /// window on every recorded step, and from the command palette — so a
    /// user with ten saved recordings beachballed on the first menu click
    /// after every launch. Nothing in a listing reads an argument.
    ///
    /// Every check a listing can afford is still made — the JSON shape, the
    /// schema version, the name, and each step's object shape and tool name
    /// — so a hand-edit that breaks any of those is still listed as broken,
    /// with its reason, exactly as before. What moves to the full read is the
    /// deep argument walk: a bad symbol or a non-finite number now reports
    /// itself when the action is selected, run or edited rather than in the
    /// pop-up, which is the one place it can name the step and the path
    /// anyway.
    static func decodeSummary(_ data: Data) -> Result<ActionSummary, ActionError> {
        let envelope: Envelope
        switch Self.envelope(data) {
        case .success(let parsed): envelope = parsed
        case .failure(let reason): return .failure(reason)
        }
        var summary = ActionSummary(name: envelope.name, stepCount: envelope.rawSteps.count)
        for (index, entry) in envelope.rawSteps.enumerated() {
            switch stepShape(entry, at: index) {
            case .success(let shape): if !shape.enabled { summary.disabledSteps += 1 }
            case .failure(let reason): return .failure(reason)
            }
        }
        switch settings(envelope.object) {
        case .success(let settings):
            summary.recordedCanvas = settings.canvas
            summary.raw = settings.raw
            summary.created = settings.created
            summary.modified = settings.modified
        case .failure(let reason):
            return .failure(reason)
        }
        return .success(summary)
    }

    /// The parsed file with its steps still unwalked — what both reads start
    /// from, so the version, name and `steps` refusals are written once.
    private struct Envelope {
        let object: [String: Any]
        let name: String
        let rawSteps: [Any]
    }

    private static func envelope(_ data: Data) -> Result<Envelope, ActionError> {
        guard let parsed = try? JSONSerialization.jsonObject(with: data),
              let object = parsed as? [String: Any]
        else {
            return .failure(ActionError("not a JSON object"))
        }
        guard let version = ((object[fileKey] ?? object[legacyFileKey]) as? NSNumber)?.intValue
        else {
            return .failure(ActionError("\(fileKey) is missing"))
        }
        guard readableSchemaVersions.contains(version) else {
            return .failure(
                ActionError(
                    "unknown \(fileKey) version \(version) (this build reads "
                        + readableSchemaVersions.sorted().map(String.init)
                        .joined(separator: " and ") + ")"))
        }
        guard let name = object["name"] as? String, !name.isEmpty else {
            return .failure(ActionError("name is missing or not a string"))
        }
        guard let rawSteps = object["steps"] as? [Any] else {
            return .failure(ActionError("steps is not an array"))
        }
        return .success(Envelope(object: object, name: name, rawSteps: rawSteps))
    }

    /// The three trailing fields both reads carry, in one place so the `raw`
    /// refusals cannot drift between them.
    private static func settings(
        _ object: [String: Any]
    ) -> Result<
        (canvas: (width: Int, height: Int)?, raw: RawDevelopSettings?, created: Date?,
         modified: Date?), ActionError
    > {
        var canvas: (width: Int, height: Int)?
        if let recorded = object["recorded_canvas"] as? [String: Any],
           let width = (recorded["width"] as? NSNumber)?.intValue,
           let height = (recorded["height"] as? NSNumber)?.intValue,
           width > 0, height > 0
        {
            canvas = (width: width, height: height)
        }
        var raw: RawDevelopSettings?
        if object["raw"] != nil {
            // The SAME parser and the same strict range refusals as
            // open_document's `raw` object, so "raw.exposure must be between
            // -4 and 4 (got 9)" reads identically wherever it is refused.
            do {
                raw = try AgentServer.shared.rawOpenSettings(object)
            } catch let error as AgentServer.ToolError {
                return .failure(ActionError(error.message))
            } catch {
                return .failure(
                    ActionError("raw could not be read: \(error.localizedDescription)"))
            }
        }
        return .success(
            (canvas: canvas, raw: raw,
             created: (object["created"] as? String).flatMap { stamp.date(from: $0) },
             modified: (object["modified"] as? String).flatMap { stamp.date(from: $0) }))
    }

    /// The O(1)-per-step half of a step's decode: that it is an object, that
    /// it names a tool a step may be, and its `enabled` flag read leniently.
    ///
    /// Shared so a listing and a full decode agree about what a BROKEN file
    /// is. `enabled` is read strictly by `decodeStep` below, after the
    /// arguments, which keeps the reported failure the first one a reader
    /// would look for; a summary only counts, so it takes the lenient answer.
    private static func stepShape(
        _ entry: Any, at index: Int
    ) -> Result<(object: [String: Any], tool: String, enabled: Bool), ActionError> {
        let label = "step \(index + 1)"
        guard let object = entry as? [String: Any] else {
            return .failure(ActionError("\(label): not an object"))
        }
        guard let named = object["tool"] as? String else {
            return .failure(ActionError("\(label): tool is missing or not a string"))
        }
        // A step saved under a tool's earlier name reads as the tool's current one
        // (`LegacyNames`), so it loads, plays and re-saves under the new name.
        let tool = LegacyNames.currentTool(named)
        // Named before the generic refusal so the reason is the real one:
        // these ARE live tools, and "unknown tool" would send a reader
        // looking for a typo.
        if let reason = notAStepReason(tool) {
            return .failure(
                ActionError("\(label): \"\(tool)\" cannot be a step — \(reason)"))
        }
        guard isKnownStepTool(tool) else {
            return .failure(ActionError("\(label): unknown tool \"\(tool)\""))
        }
        return .success((object: object, tool: tool, enabled: object["enabled"] as? Bool ?? true))
    }

    private static func decodeStep(
        _ entry: Any, at index: Int
    ) -> Result<ActionStep, ActionError> {
        let label = "step \(index + 1)"
        let object: [String: Any]
        let tool: String
        switch stepShape(entry, at: index) {
        case .success(let shape): (object, tool) = (shape.object, shape.tool)
        case .failure(let reason): return .failure(reason)
        }
        var arguments: [String: Any] = [:]
        if let raw = object["arguments"], !(raw is NSNull) {
            guard let parsed = raw as? [String: Any] else {
                return .failure(ActionError("\(label): arguments is not an object"))
            }
            arguments = LegacyNames.currentArguments(tool: tool, parsed)
        }
        // A symbol names ONE argument's value. The whole `arguments` object is
        // not one, and the shape is plausible enough to be written by hand or
        // by a model reading the symbol docs — so it is refused here rather
        // than resolved: the player would resolve it to a layer INDEX, fail
        // to read that as a dictionary and run the step with no arguments at
        // all, which for a tool whose arguments are all optional
        // (`auto_tone`, `flatten_image`) looks like a clean success on the
        // wrong layer.
        if ActionSymbol.looksLikeSymbol(arguments) {
            return .failure(
                ActionError(
                    "\(label): arguments must be an object of named arguments, not a "
                        + "symbol — a symbol is one argument's VALUE, as in "
                        + "{\"layer\": {\"$layer\": \"Sky\"}}"))
        }
        if let problem = argumentProblem(arguments) {
            return .failure(ActionError("\(label): \(problem.message)"))
        }
        var onError = ActionStep.OnError.stop
        if let raw = object["on_error"], !(raw is NSNull) {
            guard let text = raw as? String, let parsed = ActionStep.OnError(rawValue: text)
            else {
                return .failure(
                    ActionError(
                        "\(label): on_error must be \"stop\" or \"continue\" "
                            + "(got \(quoted(object["on_error"])))"))
            }
            onError = parsed
        }
        var enabled = true
        if let raw = object["enabled"], !(raw is NSNull) {
            guard let flag = raw as? Bool else {
                return .failure(ActionError("\(label): enabled must be true or false"))
            }
            enabled = flag
        }
        var geometry = GeometryMode.absolute
        if let raw = object["geometry"], !(raw is NSNull) {
            switch GeometryMode.parse(raw) {
            case .success(let mode): geometry = mode
            case .failure(let reason):
                return .failure(ActionError("\(label): \(reason.message)"))
            }
            // Refused BY TOOL NAME, at decode: a non-absolute mode on a tool
            // whose arguments carry no canvas coordinate would be a setting
            // that silently did nothing, which is worse than being told the
            // tool has nothing to remap.
            if geometry != .absolute, let reason = ActionGeometry.unsupported(tool: tool, arguments) {
                return .failure(ActionError("\(label): \(reason)"))
            }
        }
        return .success(
            ActionStep(
                tool: tool, validated: arguments, enabled: enabled, onError: onError,
                geometry: geometry, note: object["note"] as? String))
    }

    /// Every symbol-shaped value in `arguments`, and every number, checked at
    /// LOAD time so a hand-edited `{"$layer": 3}` is refused with its path
    /// rather than failing halfway through a replay that has already changed
    /// the picture. Recursive, because a symbol may sit inside an array (a
    /// layer set) — there is nowhere in the catalog it can sit deeper than
    /// that today, and walking anyway costs nothing and cannot go stale.
    ///
    /// A non-finite number is refused HERE rather than dropped by
    /// `ActionStep.jsonSafe`, which would leave the step decoding cleanly
    /// with the key silently missing. Foundation's JSON parser turns a
    /// negative overflow literal (`-1e999`) into −infinity, so this is a
    /// value a hand-written file really can hold — and one that no
    /// `JSONSerialization` write can survive.
    ///
    /// **The path is assembled on the way OUT, never on the way in**, which
    /// is what makes the walk affordable. Interpolating `"\(path).\(key)"`
    /// and `"\(path)[\(index)]"` at every node built three strings per
    /// recorded point and was almost the whole cost of loading an action: a
    /// 3.1 MB recording of 200,000 stroke points took 2.34 s to decode, of
    /// which ~2.2 s was this formatting, for a string that is thrown away at
    /// every node but the one that fails.
    private struct ArgumentProblem {
        /// The failure, as a suffix that follows the path: " is not a finite
        /// number", or ": " and a symbol parser's own sentence.
        let suffix: String
        /// The path back up to `arguments`, INNERMOST FIRST — components are
        /// appended as the failure unwinds, and reversed to read it.
        var path: [String] = []

        var message: String { "arguments" + path.reversed().joined() + suffix }
    }

    private static func argumentProblem(_ value: Any) -> ArgumentProblem? {
        if let number = value as? NSNumber, !(value is Bool), !number.doubleValue.isFinite {
            return ArgumentProblem(suffix: " is not a finite number")
        }
        if ActionSymbol.looksLikeSymbol(value) {
            // The `arguments` object being symbol-shaped is `decodeStep`'s
            // own refusal — it is the one case with nothing to name — so a
            // symbol reached here is a value, and either parses or says why.
            if case .failure(let error) = ActionSymbol.parse(value) {
                return ArgumentProblem(suffix: ": \(error.message)")
            }
            return nil
        }
        if let object = value as? [String: Any] {
            // Sorted so the reported failure is the same one every time the
            // file is loaded, whatever order the dictionary hashes in.
            for key in object.keys.sorted() {
                if var problem = argumentProblem(object[key] as Any) {
                    problem.path.append(".\(key)")
                    return problem
                }
            }
            return nil
        }
        if let array = value as? [Any] {
            for (index, element) in array.enumerated() {
                if var problem = argumentProblem(element) {
                    problem.path.append("[\(index)]")
                    return problem
                }
            }
        }
        return nil
    }

    private static func quoted(_ value: Any?) -> String {
        guard let text = value as? String else { return "\(value ?? "nothing")" }
        return "\"\(text)\""
    }

    /// ISO 8601 in UTC, for `created` and `modified`. Both are optional on
    /// read: a hand-written action need not carry either.
    private static let stamp: ISO8601DateFormatter = {
        let formatter = ISO8601DateFormatter()
        formatter.timeZone = TimeZone(secondsFromGMT: 0)
        return formatter
    }()
}

/// What a library file turned out to be: something readable, or a file that
/// could not be read AND the reason. A broken file is listed and greyed
/// rather than silently dropped — a user who hand-edits an action must be
/// able to see what they broke.
///
/// Generic over what "readable" means, because the library answers two
/// different questions with the same shape: a listing wants an
/// `ActionSummary` and a run wants the whole `Action`.
enum ActionEntry<Value> {
    case ok(Value)
    case broken(file: String, reason: String)
}

/// One entry of the library, fully decoded.
typealias ActionLoadResult = ActionEntry<Action>

/// One entry of the library as a LISTING sees it.
typealias ActionSummaryResult = ActionEntry<ActionSummary>

/// What an action is, minus its steps: everything a listing shows.
///
/// It exists so opening a menu never walks a recorded polyline — the whole
/// argument is in `Action.decodeSummary`. Everything here is either a header
/// field or a count taken from the top level of the `steps` array, so
/// producing one costs the JSON parse and nothing per point.
struct ActionSummary {
    var name: String
    var stepCount: Int
    var disabledSteps: Int = 0
    var recordedCanvas: (width: Int, height: Int)?
    var raw: RawDevelopSettings?
    var created: Date?
    var modified: Date?
}

extension RawDevelopSettings {
    /// True when nothing is set — the develop request that means "as shot",
    /// which is written as no `raw` object at all.
    var isAsShot: Bool { self == RawDevelopSettings() }

    /// The develop request as the JSON object `open_document`'s `raw`
    /// argument and an action file both use, so the two spellings cannot
    /// drift: the keys here are exactly the ones `AgentServer.rawOpenSettings`
    /// parses.
    func jsonObject() -> [String: Any] {
        var object: [String: Any] = [:]
        if let value = baselineCompensation { object["baseline_compensation"] = value }
        if let value = moire { object["moire"] = value }
        if let value = vignette { object["vignette"] = value.json }
        if let value = grain { object["grain"] = value.json }
        if let value = texture { object["texture"] = value }
        if let value = clarity { object["clarity"] = value }
        if let value = dehaze { object["dehaze"] = value }
        if let value = sourceGeometry { object["source_geometry"] = value.jsonObject }
        if let value = outputSpace { object["output_space"] = value.rawValue }
        if let value = outputEncoding { object["output_encoding"] = value.rawValue }
        if let value = exposure { object["exposure"] = value }
        if let value = temperature { object["temperature"] = value }
        if let value = tint { object["tint"] = value }
        if let value = asShotWhiteBalance { object["as_shot_white_balance"] = value }
        if let value = toneCurve { object["tone_curve"] = value }
        if let value = shadows { object["shadows"] = value }
        if let value = contrast { object["contrast"] = value }
        if let value = sharpness { object["sharpness"] = value }
        if let value = detail { object["detail"] = value }
        if let value = luminanceNoise { object["luminance_noise"] = value }
        if let value = colorNoise { object["color_noise"] = value }
        if let value = lensCorrection { object["lens_correction"] = value }
        if let value = highlightRecovery { object["highlight_recovery"] = value }
        return object
    }
}
