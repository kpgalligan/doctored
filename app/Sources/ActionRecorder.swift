import AppKit

// During recording, any step whose target is a document `openDuplicate` made is replaced by ONE
// named `.unrecorded` placeholder (the recorder has no document-switch step, so replaying it would
// edit the run's own document); the agent call that named no `document_id` and so resolves to the
// new front window is told apart by `latestDuplicate` and recorded.

/// The recording half of Actions: the ONE place a step lands, wherever it
/// came from.
///
/// Two feeds reach it, and only two:
///
/// 1. **The UI**, through the `record: [ActionStep]` parameter that every
///    `ImageDocument` commit entry point now takes. The commit method calls
///    `record(_:)` on its SUCCESS path only — after the handle has actually
///    been replaced — so a beeped core refusal, a cancelled sheet, a
///    cancelled rasterize prompt and a `nil` transform all record nothing,
///    for free, because they return before the record line.
/// 2. **The agent**, through one line in `AgentServer.execute`, which covers
///    every tool, the HTTP MCP server and the built-in assistant panel at
///    once.
///
/// Main thread only — every one of its callers already is (AppKit events and
/// the two agent trampolines' `DispatchQueue.main.sync`), so it needs no
/// lock and asserts the fact rather than pretending otherwise.
final class ActionRecorder {
    static let shared = ActionRecorder()

    /// Posted when a step lands, recording starts, or recording stops, so
    /// the Actions window can show a recording growing live.
    static let didChange = Notification.Name("doctoredActionRecorderDidChange")

    private(set) var isRecording = false

    /// Set for the length of a replay (an Action run, ⌃F, a Batch file) so
    /// the player never records itself. Deliberately a plain flag rather
    /// than a suspend/resume count: replays do not nest — `ActionPlayer.run`
    /// is synchronous and the only writer.
    ///
    /// A menu command run from the COMMAND PALETTE is not a replay and IS
    /// recorded: the palette is a way to reach a command, not a way to run
    /// an action.
    var isSuspended = false

    private(set) var steps: [ActionStep] = []

    /// How much of the budget the session has spent — see `maxRecordedPoints`.
    private var recordedPoints = 0

    /// Set once the session has spent its budget: the placeholder saying so
    /// is already the last step, and nothing more is appended.
    private var isFull = false

    /// What one recording may hold before it stops taking steps.
    ///
    /// **A recording of a normal retouching session was unbounded, and the
    /// file it produced was unusable.** Measured: 150 strokes at the
    /// recorder's own 10,000-point cap is 34.7 MB compact and 124.4 MB as
    /// the app wrote it; decoding one such file blocked the app for 8.7 s
    /// and left 800 MB of RSS behind, and every Enabled checkbox in the
    /// Actions window rewrote the whole thing. Nothing warned, and the
    /// in-memory recording was ~100 MB before it was ever saved.
    ///
    /// So the session has a budget, and spending it is VISIBLE: the
    /// placeholder appended at the limit says the recording stopped and why,
    /// exactly like every other gap this feature refuses to leave silent.
    /// Deleting steps in the Actions window gives the budget back
    /// (`replaceSteps` recomputes it), so a user who trims the strokes can
    /// carry on recording.
    ///
    /// 200,000 points is about 4.6 MB of compact JSON — twenty full-length
    /// sweeps, or hundreds of ordinary ones — and 1,000 steps is far more
    /// menu commands than a session anyone would replay.
    static let maxSteps = 1000
    static let maxRecordedPoints = 200_000

    /// The canvas the recording was made on — the size the FIRST recorded
    /// step was authored against, before that step ran. Reported on a size
    /// mismatch at replay; never used to remap a coordinate.
    ///
    /// Nil while nothing has been recorded, and nil again once two steps have
    /// been recorded against different documents (see `noteCanvas`).
    private(set) var recordedCanvas: (width: Int, height: Int)?

    /// The document the recording's steps have targeted so far, weakly. The
    /// canvas belongs to a DOCUMENT, so this is what decides whether a later
    /// step belongs to the same recording's coordinate space — a step that
    /// resized the canvas does not.
    private weak var recordedDocument: ImageDocument?

    /// True once `recordedDocument` has been set, so a document that has
    /// since closed is still distinguishable from one never seen.
    private var hasRecordedDocument = false

    /// Set once two recorded steps have named different documents. There is
    /// then no ONE canvas this recording was made against, and a warning
    /// about the wrong one is worse than no warning at all.
    private var canvasIsAmbiguous = false

    /// The last repeatable filter or adjustment, for Filters ▸ Repeat Last.
    ///
    /// Updated whether or not recording is on, because ⌃F has to work
    /// without it — which is the whole reason it lives on the recorder
    /// rather than in the recording.
    private(set) var lastRepeatable: ActionStep?

    /// Documents Layer ▸ Duplicate to New Document made during THIS
    /// recording, weakly. A step that targets one is replaced by ONE
    /// named placeholder: the recording keeps a single flat step list with no
    /// document switch, so on replay such a step would edit the run's own
    /// document — a silent wrong-document edit. The placeholder never edits
    /// anything, stays visible in the Actions window and fails by name at
    /// replay (`ActionPlayer.runStep`'s `isUnrecorded` guard).
    private var duplicatedDocuments = NSHashTable<ImageDocument>.weakObjects()

    /// The document the most recent `noteDuplicatedDocument` call made, and
    /// the one it was read from. An agent call that named no `document_id`
    /// resolves its recorded target AFTER it ran, which for
    /// `duplicate_to_new_document` is the new front window — the duplicate
    /// itself — so the duplicate's OWN step is told apart by this identity
    /// and recorded against its source, unless that source is itself a
    /// duplicate (replay would copy the run's document instead).
    private weak var latestDuplicate: ImageDocument?
    private weak var latestDuplicateSource: ImageDocument?

    /// True when `step` opens a document noted by `noteDuplicatedDocument`
    /// (Super Zoom's output is one, and Colorize's with output
    /// `new_document`), so its own step is recorded; a layer-output Colorize
    /// in a fresh duplicate stays the placeholder.
    private static func opensDuplicate(_ step: ActionStep) -> Bool {
        switch step.tool {
        case "duplicate_to_new_document", "super_zoom": return true
        case "neural_colorize": return step.arguments["output"] as? String == ColorizeOutput.newDocument.rawValue
        default: return false
        }
    }

    static let duplicatedDocumentPlaceholder =
        "Edits in a document made by Duplicate to New Document"

    /// Documents File ▸ Automate ▸ Merge Panorama… made during THIS recording,
    /// weakly: a step that targets one becomes ONE named
    /// placeholder, for `duplicatedDocuments`' reason.
    private var createdDocuments = NSHashTable<ImageDocument>.weakObjects()

    static let createdDocumentPlaceholder = "Edits in a document made by Merge Panorama"

    private init() {}

    // MARK: - The session

    /// True when a stopped session's steps are still here, waiting to be
    /// saved or thrown away. `start()` DISCARDS them, so both entry points
    /// that can start a session ask first (`AppDelegate.toggleActionRecording`
    /// puts up the alert; `start_recording` refuses unless `discard` is
    /// passed) — the steps live only in memory, no file holds them and no
    /// undo covers them.
    var hasUnsavedRecording: Bool { !isRecording && !steps.isEmpty }

    func start() {
        assert(Thread.isMainThread)
        DocumentJobs.finishBeforeRecordingChange()
        isRecording = true
        steps = []
        recordedPoints = 0
        isFull = false
        // Not captured here: `currentDocument` at the moment Record is
        // pressed need not be the document the steps go on to edit — an
        // agent driving the app in the background has no current document at
        // all. The first recorded step says which document it targeted, and
        // that is where the size comes from.
        recordedCanvas = nil
        recordedDocument = nil
        hasRecordedDocument = false
        canvasIsAmbiguous = false
        duplicatedDocuments.removeAllObjects()
        createdDocuments.removeAllObjects()
        latestDuplicate = nil
        latestDuplicateSource = nil
        NotificationCenter.default.post(name: Self.didChange, object: self)
    }

    /// Remembers a document Duplicate to New Document just opened, while a
    /// session is recording (suspended or not — a replay's duplicate is
    /// still a document the recording must not file edits under).
    func noteDuplicatedDocument(_ document: ImageDocument, from source: ImageDocument) {
        assert(Thread.isMainThread)
        guard isRecording else { return }
        duplicatedDocuments.add(document)
        latestDuplicate = document
        latestDuplicateSource = source
    }

    /// Remembers a document Merge Panorama just made, while a session is
    /// recording.
    func noteCreatedDocument(_ document: ImageDocument) {
        assert(Thread.isMainThread)
        guard isRecording else { return }
        createdDocuments.add(document)
    }

    /// Ends the session and hands back what it captured.
    @discardableResult
    func stop() -> [ActionStep] {
        assert(Thread.isMainThread)
        DocumentJobs.finishBeforeRecordingChange()
        isRecording = false
        let captured = steps
        NotificationCenter.default.post(name: Self.didChange, object: self)
        return captured
    }

    /// The session's steps as an action ready to save.
    func recording(named name: String) -> Action {
        Action(
            name: name, steps: steps, recordedCanvas: recordedCanvas, raw: nil,
            created: Date(), modified: Date())
    }

    // MARK: - The one entry point

    /// Records `steps`, if a session is running and not suspended, and
    /// latches the last repeatable one either way.
    ///
    /// An empty array — `.notACommand` — is the common case at the sites
    /// that deliberately record nothing, and costs a branch.
    ///
    /// `document` is the one the steps were made against. The four
    /// `ImageDocument` commit hooks pass themselves and the agent passes the
    /// document its call resolved to; the handful of remaining UI sites pass
    /// none and mean "the front one", which for a gesture in a window is the
    /// same thing.
    ///
    /// `canvas` is that document's size BEFORE the steps ran, which is the
    /// size their absolute coordinates were written against — the four commit
    /// hooks have the pre-edit handle in hand and pass it, and the agent
    /// samples it before it dispatches. nil means "read it from the document
    /// now", which is right for the sites that cannot resize a canvas (a
    /// selection gesture, a layer-panel click) and is the only answer
    /// available to a step that CREATED the document it targeted.
    func record(
        _ steps: [ActionStep], on document: ImageDocument? = nil,
        canvas: (width: Int, height: Int)? = nil
    ) {
        assert(Thread.isMainThread)
        guard !steps.isEmpty, !isSuspended else { return }
        // A Develop Filter step keeps no generated map and no profile: a replay or ⌃F
        // makes them again on the layer it reaches.
        let steps = steps.map(DevelopFilterMasks.recorded)
        for step in steps where ActionCatalogFacts.repeatableTools.contains(step.tool) {
            lastRepeatable = step
        }
        guard isRecording, !isFull else { return }
        // Merge Panorama makes its own document from files: its step targets no
        // document, so no target check and no canvas.
        if steps.allSatisfy({ $0.tool == "merge_panorama" }) {
            append(steps)
            return
        }
        var canvasDocument = document
        var canvasSize = canvas
        if let target = document ?? (NSDocumentController.shared.currentDocument as? ImageDocument),
           createdDocuments.contains(target) {
            appendPlaceholder(Self.createdDocumentPlaceholder)
            return
        }
        if let target = document ?? (NSDocumentController.shared.currentDocument as? ImageDocument),
           duplicatedDocuments.contains(target) {
            if target === latestDuplicate,
               steps.allSatisfy(Self.opensDuplicate),
               let source = latestDuplicateSource, !duplicatedDocuments.contains(source) {
                // The duplicate's own step: file its canvas under the source
                // it was read from, not the new document.
                canvasDocument = source
                canvasSize = nil
            } else {
                appendPlaceholder(Self.duplicatedDocumentPlaceholder)
                return
            }
        }
        noteCanvas(of: canvasDocument, size: canvasSize)
        append(steps)
    }

    /// One named placeholder, unless the last step already is that one.
    private func appendPlaceholder(_ name: String) {
        if steps.last?.isUnrecorded != true || steps.last?.unrecordedName != name {
            steps.append(contentsOf: [ActionStep].unrecorded(name))
        }
        NotificationCenter.default.post(name: Self.didChange, object: self)
    }

    /// `steps` appended against the session's budget.
    private func append(_ steps: [ActionStep]) {
        self.steps.append(contentsOf: steps)
        recordedPoints += steps.reduce(0) { $0 + Self.weight(of: $1) }
        if self.steps.count >= Self.maxSteps || recordedPoints >= Self.maxRecordedPoints {
            isFull = true
            let reason =
                self.steps.count >= Self.maxSteps
                ? "\(Self.maxSteps) steps" : "\(Self.maxRecordedPoints) stroke points"
            self.steps.append(
                contentsOf: [ActionStep].unrecorded(
                    "Recording stopped at \(reason) — later edits were not recorded"))
        }
        NotificationCenter.default.post(name: Self.didChange, object: self)
    }

    /// What one step spends of the budget: the length of its `points` array
    /// — a stroke's flattened polyline or a lasso's outline, which is where
    /// every large recorded payload lives — and 1 for everything else.
    ///
    /// Through `NSArray` deliberately: `as? [Any]` would COPY the array to
    /// count it, on every step, which is the kind of cost this budget exists
    /// to bound. `count` through the bridge is O(1) for both shapes the
    /// recorder sees (a UI factory's `[[Double]]` and an agent call's parsed
    /// JSON) — measured.
    private static func weight(of step: ActionStep) -> Int {
        if let native = step.arguments["native_stroke"] as? [String: Any], let data = native["dabs"] as? String {
            return max(1, (data.utf8.count + 23) / 24)
        }
        if let source = step.arguments["source_mask"] as? [String: Any],
           let operations = source["operations"] as? [[String: Any]] {
            // CAF retains ordered source geometry inside its canonical recipe.
            // Count it against the same budget as ordinary gesture geometry;
            // a rectangle carries four scalar-coordinate pairs' worth.
            return max(1, operations.reduce(0) { count, operation in
                count + ((operation["points"] as? NSArray)?.count ?? 4)
            })
        }
        if let coverage = (step.arguments["coverage_rle16"] ?? step.arguments["coverage_rle"]) as? String {
            // Bound canonical Quick Mask payloads by the same approximate
            // 24-byte point budget as brush/lasso recordings.
            return max(1, (coverage.utf8.count + 23) / 24)
        }
        if step.tool == "liquify", let strokes = step.arguments["strokes"] as? [[String: Any]] {
            // A Liquify step's points sit inside its strokes.
            return max(1, strokes.reduce(0) { $0 + (($1["points"] as? NSArray)?.count ?? 0) })
        }
        if step.tool == "puppet_warp", let pins = step.arguments["pins"] as? NSArray {
            // A Puppet Warp pin is two points, its from and its to.
            return max(1, 2 * pins.count)
        }
        if step.tool == "perspective_warp", let quads = step.arguments["quads"] as? NSArray {
            // A Perspective Warp quad is eight points, four corners and four pins.
            return max(1, 8 * quads.count)
        }
        return max(1, ((step.arguments["points"] as? NSArray)?.count ?? 0)
            + ((step.arguments["seeds"] as? NSArray)?.count ?? 0))
    }

    /// A live GESTURE's steps — the `endLiveEdit` hook — built only when a
    /// session is actually running.
    ///
    /// **Why this one is lazy and `record` is not.** A stroke's step carries
    /// the gesture's whole flattened polyline, up to the recorder's own
    /// 10,000-point cap, and building it measured 16 ms at mouse-up on a
    /// long sweep (almost all of it in `ActionStep`'s sanitising walk) — a
    /// cost the app was paying on EVERY stroke, whether or not anyone was
    /// recording, because the argument was evaluated at the call site.
    /// `record` cannot be lazy the same way: it also latches
    /// `lastRepeatable`, which ⌃F needs with no recording running, and that
    /// latch has to see the step.
    ///
    /// Nothing is lost by skipping the build here, and the assert says so: a
    /// gesture's step is never one of the four repeatable tools, which are
    /// all menu commands committing through `applyEdit`.
    func recordGesture(
        _ steps: @autoclosure () -> [ActionStep], on document: ImageDocument? = nil,
        canvas: (width: Int, height: Int)? = nil
    ) {
        let steps = gestureRecord(steps())
        guard !steps.isEmpty else { return }
        record(steps, on: document, canvas: canvas)
    }

    /// `recordGesture`'s guard, for a gesture that cannot call it: the
    /// coverage-stroke commit goes through `ImageDocument.applyEdit` /
    /// `applyRasterizingEdit`, whose `record:` is an ordinary parameter
    /// because the `lastRepeatable` latch has to see every menu command's
    /// step. The answer is the same guard, applied at the site, so the
    /// expression is built only when a session is actually collecting it.
    ///
    /// Painting into a layer MASK is the commonest coverage gesture there is,
    /// and it was paying the whole build at every mouse-up with nothing
    /// running: `recordablePoints` walks the flattened polyline, one
    /// `[Double]` is allocated per vertex up to the 10,000-point cap, and
    /// `ActionStep.init` then walks the result again to ask
    /// `isValidJSONObject` (4.7 ms of the 16 ms, measured) — for a value
    /// `record` immediately discarded.
    ///
    /// Returns the empty array — `.notACommand` — when nothing is listening,
    /// which is exactly what those commit points take for "records nothing".
    func gestureRecord(_ steps: @autoclosure () -> [ActionStep]) -> [ActionStep] {
        assert(Thread.isMainThread)
        guard isRecording, !isSuspended else { return [] }
        let steps = steps()
        assert(
            !steps.contains { ActionCatalogFacts.repeatableTools.contains($0.tool) },
            "a gesture recorded a repeatable tool — the lastRepeatable latch would miss it")
        return steps
    }

    /// One agent tool call, recorded VERBATIM apart from `document_id` and a
    /// target the handler resolved from document MODE.
    ///
    /// The agent said what it meant — an explicit layer index, an explicit
    /// target, an explicit colour — and second-guessing those into symbols
    /// would be a lie about what was asked for. The id is one exception:
    /// it is a session-scoped integer minted by `AgentServer.id(for:)` and
    /// means nothing on a later run, so the player rebinds it. The other is
    /// a target the call did NOT say but the handler chose from the
    /// document's state (`pinningModeDefaults`): recorded bare, it would be
    /// chosen again from the REPLAY document's state and could land on a
    /// different plane.
    func recordAgentCall(
        _ tool: String, _ arguments: [String: Any], on document: ImageDocument?,
        canvas: (width: Int, height: Int)? = nil
    ) {
        guard !ActionCatalogFacts.nonRecordable.contains(tool) else { return }
        guard !ActionCatalogFacts.billsCloud(tool, arguments) else { return }
        // `record`'s own two guards, asked before the step is BUILT: a call
        // nobody is collecting and whose tool the ⌃F latch does not want
        // costs an `ActionStep.init`, which walks the whole argument tree
        // with `isValidJSONObject` — the agent's stroke tools carry a
        // caller's entire polyline, so that is the same walk the gesture
        // paths take care not to make.
        guard !isSuspended, isRecording || ActionCatalogFacts.repeatableTools.contains(tool)
        else { return }
        var arguments = arguments
        arguments.removeValue(forKey: "document_id")
        arguments = Self.pinningModeDefaults(tool, arguments, on: document)
        // Instance IDs are re-evaluated on replay; a source session's revision
        // is only a stale-inspection guard for the original call.
        if tool == "select_subjects" { arguments.removeValue(forKey: "expected_revision") }
        if tool == "hdr_toning" { arguments = HdrToningPreset.recordedArguments(arguments) }
        // A library pattern `{"name": …}` records as its tile, so the step replays without the
        // library (`PatternLibrary.recordedArguments`).
        arguments = PatternLibrary.recordedArguments(tool, arguments)
        // A history state id names THIS session's history, which no
        // replay can resolve; the replay paints from the History panel's source.
        if tool == "art_history_brush_stroke" { arguments.removeValue(forKey: "state_id") }
        record(
            [ActionStep(tool: tool, arguments: arguments, note: "MCP: \(tool)")],
            on: document, canvas: canvas)
    }

    /// `arguments` with the target a handler resolved from document MODE
    /// written in, so the step replays onto what the call actually edited —
    /// the same target the UI records for the same edit. A new
    /// mode-resolved default goes here.
    ///
    /// - `fill_selection` with no `target` fills the Quick Mask while the
    ///   document is in Quick Mask and the layer otherwise
    ///   (`AgentServer.fillSelection`); pinned, a Quick Mask fill refuses by
    ///   name outside Quick Mask and a layer fill refuses by name inside it
    ///   (`FillSettings.quickMaskArguments`), as the UI's steps do.
    /// - `gradient` and `apply_filter filter: "blur"` with no `target` edit
    ///   the Quick Mask while the document is in Quick Mask; pinned,
    ///   the step refuses by name outside it.
    /// - `brush_stroke` / `eraser_stroke` on an ADJUSTMENT layer with no
    ///   `target`, `"layer"` or a paintable plane (`"red"`, `"green"`,
    ///   `"blue"`, `"alpha"`) paint its mask (`resolvePaintTarget`'s forcing,
    ///   at 8 and 16 bits), so the step says `"mask"`, as the UI's stroke
    ///   does. `"mask"` and `"channel:…"` record as called. The layer
    ///   is `layer`, else `index`, else the active one.
    ///
    /// `document` is the one AFTER the call — the only one the recorder has.
    /// That is sound because neither tool changes what is read here: a fill
    /// leaves Quick Mask on or off, and a stroke neither reorders layers,
    /// changes their kind nor moves the active one.
    private static func pinningModeDefaults(
        _ tool: String, _ arguments: [String: Any], on document: ImageDocument?
    ) -> [String: Any] {
        guard let document else { return arguments }
        var pinned = arguments
        switch tool {
        case "fill_selection":
            guard arguments["target"] == nil else { return arguments }
            pinned["target"] = document.quickMaskActive ? "quick_mask" : "layer"
        // With no `target`, `gradient` and `apply_filter filter:
        // "blur"` edit the Quick Mask while the document is in it
        // (AgentServer+QuickMaskEdits.swift). Outside Quick Mask the step
        // records as called: no target is the layer, as it always was.
        case "gradient", "apply_filter":
            guard arguments["target"] == nil, document.quickMaskActive,
                  tool == "gradient" || arguments["filter"] as? String == "blur"
            else { return arguments }
            pinned["target"] = "quick_mask"
        case "brush_stroke", "eraser_stroke":
            let named = arguments["target"] as? String
            let plane = named.flatMap(RasterPlane.named).map {
                PaintTarget.paintablePlanes.contains($0)
            } == true
            guard arguments["target"] == nil || named == "layer" || plane, let doc = document.doc
            else { return arguments }
            let index = DocumentPrecision.integer(arguments["layer"])
                ?? DocumentPrecision.integer(arguments["index"]) ?? document.activeLayerIndex
            guard doc.layerIsAdjustment(index) else { return arguments }
            pinned["target"] = "mask"
        default:
            return arguments
        }
        return pinned
    }

    /// Editing steps in place, for the Actions window's row commands. The
    /// window owns the presentation; the session's array lives here.
    func replaceSteps(_ steps: [ActionStep]) {
        assert(Thread.isMainThread)
        self.steps = steps
        // The budget follows the steps: deleting the paint strokes that
        // filled a recording gives it room to carry on.
        recordedPoints = steps.reduce(0) { $0 + Self.weight(of: $1) }
        isFull = steps.count >= Self.maxSteps || recordedPoints >= Self.maxRecordedPoints
        NotificationCenter.default.post(name: Self.didChange, object: self)
    }

    /// Binds the recording's canvas to the document a step actually targeted,
    /// at the size that step was authored against.
    ///
    /// It used to be taken from `currentDocument` — falling back to the
    /// OLDEST open document when there was none — at the moment recording
    /// started. Both halves were wrong for the case Actions exist for: an
    /// agent driving the app in the background has no current document, so a
    /// recording made entirely over MCP took the size of whichever file
    /// happened to be opened first, and then warned about a mismatch on the
    /// very document it was recorded from.
    ///
    /// **Two things are load-bearing here, and both were wrong.**
    ///
    /// 1. The size comes from the caller, sampled BEFORE its step ran. Every
    ///    hook records after the handle has been replaced, so re-reading the
    ///    document gave the size the step LEFT: a recording of
    ///    `crop {0, 0, 300, 200}` on a 1024 × 768 photo saved
    ///    `recorded_canvas: 300 × 200`, so replaying it on the 1024 × 768
    ///    original — the one canvas those coordinates were written for —
    ///    warned, and replaying it on a 300 × 200 document, where the crop is
    ///    meaningless, said nothing.
    /// 2. Steps that disagree about the SIZE are normal; steps that disagree
    ///    about the DOCUMENT are not. A recording holding any canvas-resizing
    ///    step (crop, rotate, image_size, canvas_size) saw two sizes and
    ///    dropped the canvas entirely — measured, a grayscale + new_layer +
    ///    rotate recording saved `recorded_canvas: null` — which silenced the
    ///    mismatch warning for exactly the actions whose coordinates a resize
    ///    makes most dangerous. Identity is the real question, and it is the
    ///    one asked now: the size is latched once, from the first step, and
    ///    dropped only when a later step targets a different document.
    private func noteCanvas(of document: ImageDocument?, size: (width: Int, height: Int)?) {
        guard !canvasIsAmbiguous else { return }
        guard let target = document
            ?? (NSDocumentController.shared.currentDocument as? ImageDocument)
        else { return }
        if hasRecordedDocument, recordedDocument !== target {
            // Including the case where the first document has been closed and
            // deallocated: a step on another one is a step in another
            // coordinate space either way.
            recordedCanvas = nil
            canvasIsAmbiguous = true
            return
        }
        recordedDocument = target
        hasRecordedDocument = true
        guard recordedCanvas == nil else { return }
        guard let authored = size ?? target.doc.map({ (width: $0.width, height: $0.height) }),
              authored.width > 0, authored.height > 0
        else { return }
        recordedCanvas = authored
    }
}
