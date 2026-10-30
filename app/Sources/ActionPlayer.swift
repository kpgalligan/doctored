import AppKit

// `ActionPlayer` carries a RUNNING recorded space advanced by `canvasAfter` only after a step
// SUCCEEDS, and the resolution order is fixed — `$param`, then the geometry remap, then every other
// symbol, then `document_id`.
//
// **The batch exclusion lives in `ActionPlayer.replayRefusal`, not in `AgentServer.execute`**: a
// batch's own steps and its `save_copy`/`quick_export` delivery go through `execute` too, so the
// clause sits behind the `isRunningStep` escape and `ActionPlayer.dispatchingOwnStep` marks exactly
// the three calls the machinery makes on its own behalf.

/// How a played action lands in History/Undo.
enum ActionRunHistory {
    /// The whole run is ONE undo entry under the action's name, and none when
    /// nothing changed (`run_action`, Batch).
    case oneEntry
    /// Each step that changed something is its own History/Undo state under
    /// its own tool's undo name (the Actions window's Play, File ▸ Automate ▸
    /// Play Last).
    case perStep
}

/// Running an action: symbol resolution, per-step execution through the
/// SAME dispatch an MCP client reaches, and the failure policy.
///
/// Every step runs through `AgentServer.shared.execute(tool:argumentsJSON:)`
/// on the main thread. That is deliberate and it is the whole reason Actions
/// are affordable: replay needs no second implementation of anything,
/// because every user-visible edit already has an MCP twin.
///
/// **Undo granularity is the caller's `history` (`ActionRunHistory`), and
/// no step that changed nothing leaves an entry either way.**
///
/// - `.oneEntry` (`run_action`, Batch): `run` hands the loop to
///   `ImageDocument.withReplayUndo`, which suppresses the steps' own
///   registrations and registers a single snapshot restore afterwards — but
///   only if the document handle actually moved, so a run whose steps all
///   failed or were all disabled leaves no phantom "Undo <action>" behind.
///   That method's own comment carries the measurement the design rests on.
///   `run_action` is one entry because each MCP tool call is one undo step;
///   Batch is one per file because it saves and closes each document.
/// - `.perStep` (the Actions window's Play, File ▸ Automate ▸ Play Last):
///   each step runs alone inside `ImageDocument.withUndoGroupDiscardingEmpty`,
///   so a step that changed something is one History/Undo state under its own
///   tool's undo name and counts its own change, and a failed or no-op step
///   leaves none.
///
/// **A UI-driven run can be stopped.** `progress` is the Actions window's
/// step counter and Stop button; the player consults it between steps, never
/// inside one (a core call cannot be interrupted, and abandoning a half-run
/// filter is worse than finishing it). A stopped run keeps the steps it has
/// already applied, each its own History state. `run_action` over MCP and
/// Batch pass none — Batch has its own Stop, per file.
///
/// **A UI-driven run ASKS when a step fails.** The Actions
/// window's Play and File ▸ Automate ▸ Play Last pass `prompt`
/// (`ActionFailurePrompt.ask`): a failed step whose `on_error` is `stop` puts
/// up Stop / Continue when its command is unavailable or names a missing
/// object, and an OK-only alert that stops the run when it failed inside the
/// command (a `ToolError` kinded `.commandFailed`) —
/// while `isPlaying` and the suspended recorder stay held. Stop keeps the
/// applied steps, as the progress Stop does; Continue
/// skips the failed step and goes on to the next. A `continue` step never
/// asks. `run_action`, Batch and ⌃F never ask.
enum ActionPlayer {
    // MARK: - Exclusion

    /// True for the whole of `run` / `runSingle`, progress panel included.
    ///
    /// **Why a replay has to be exclusive.** `ActionRunProgress.tick` calls
    /// `NSApp.runModalSession` between steps, and a modal session services
    /// the main run loop in `NSModalPanelRunLoopMode` — one of the modes
    /// libdispatch drains the main QUEUE in. Both agent trampolines reach
    /// `AgentServer.execute` through `DispatchQueue.main.sync`, so an MCP or
    /// assistant tool call really can land between two steps of a UI-driven
    /// replay. In a `.oneEntry` run it would land inside
    /// `ImageDocument.withReplayUndo`, which has undo registration disabled
    /// and the change count suppressed for the length of the run: the edit
    /// would apply and report `ok`, register no undo entry of its own, count
    /// no change — and the single snapshot restore the run registers
    /// afterwards would revert it along with the action, so one ⌘Z would
    /// silently throw away an edit the user watched land. In a `.perStep` run
    /// (the interactive Play) it would land between the steps' History
    /// states, an edit interleaved with the action's own that the user never
    /// asked for in the middle of it. Either way it is invisible to the
    /// recording too, since the player suspends `ActionRecorder` for the same
    /// window.
    ///
    /// So the whole tool surface is refused while a run is in flight, IN
    /// BAND, the way `start_recording` refuses a second session: a refusal a
    /// model can read and retry is better than an edit that quietly
    /// disappears.
    private(set) static var isPlaying = false

    /// True only while the player's OWN step is inside
    /// `AgentServer.execute` — the one call that must get through.
    private static var isRunningStep = false

    /// The in-band refusal a tool call earns while a replay — or a BATCH —
    /// is in flight, or nil when it may run. Read by `AgentServer.execute`,
    /// which is the single door every MCP and assistant call comes through.
    ///
    /// **The batch clause lives HERE and not in `execute`, and that is the
    /// whole point.** A batch's own work reaches `execute` too: every step of
    /// every file's replay, plus the `save_copy` or `quick_export` that
    /// delivers it. A refusal written at the `execute` seam would therefore
    /// refuse the batch's own calls and every file would fail in band. Putting
    /// it behind the `isRunningStep` escape — which `dispatchingOwnStep` sets
    /// around exactly the calls the machinery makes on its own behalf — means
    /// an OUTSIDE call is refused and the run's own never are.
    ///
    /// `allowedDuringBatch` is the small set a caller needs precisely BECAUSE
    /// a batch is running: poll it, cancel it, and the three job/resource
    /// readouts that say what it is costing.
    static func replayRefusal(tool: String) -> String? {
        guard !isRunningStep else { return nil }
        if isPlaying {
            return "An action is playing on this app — \"\(tool)\" cannot run until it "
                + "finishes. Try again in a moment."
        }
        guard BatchRun.active != nil, !BatchRun.allowedDuringBatch.contains(tool) else {
            return nil
        }
        return "A batch is running on this app — \"\(tool)\" cannot run until it finishes or "
            + "is cancelled."
    }

    /// Runs `body` marked as the machinery's OWN dispatch, so the exclusions
    /// above let exactly that call through.
    ///
    /// Three callers, and they are the complete list: the player's own step,
    /// and the two ways `BatchRun` delivers a file (`save_copy` and
    /// `quick_export`). Nothing else may use it — it is the escape hatch out
    /// of the refusal, and a fourth caller would be a hole in it.
    static func dispatchingOwnStep(_ body: () -> String) -> String {
        let wasRunningStep = isRunningStep
        isRunningStep = true
        defer { isRunningStep = wasRunningStep }
        return body()
    }

    // MARK: - Running

    /// Plays `action` on `document`. `stopOnError` overrides every step's
    /// own `on_error` when given (Batch's checkbox); nil leaves each step to
    /// its own setting. `parameters` are the values a `{"$param": …}` symbol
    /// resolves to.
    ///
    /// `prompt` is the interactive Play's Stop / Continue question:
    /// when it is given and `stopOnError` is nil, a failed step whose
    /// `on_error` is `stop` is put to it — `.stop` halts the run there,
    /// `.continueRun` goes on — and `report.prompted` counts the answers. A
    /// failure whose kind offers no Continue (`.commandFailed`) halts
    /// the run whatever the answer.
    /// Otherwise the rule is `stopOnError ?? (step.onError == .stop)`. Batch
    /// and `run_action` pass none, so they never ask.
    ///
    /// `history` is the undo granularity (see the type's doc): `.oneEntry`,
    /// the default, for `run_action` and Batch; `.perStep` for the two
    /// interactive Plays. `progress.tick` and `prompt` run outside the
    /// per-step groups, so no group is open during a modal session or alert.
    ///
    /// The parameters are BOUND here before a single step runs, even though
    /// every caller has bound them already: a run that discovered a missing
    /// value at step 7 would leave the picture half-edited, and the defence
    /// costs one dictionary walk.
    static func run(
        _ action: Action, on document: ImageDocument, parameters: [String: Any] = [:],
        stopOnError: Bool? = nil, progress: ActionRunProgress? = nil,
        prompt: ((ActionStepFailure) -> ActionFailureChoice)? = nil,
        history: ActionRunHistory = .oneEntry
    ) -> ActionRunReport {
        assert(Thread.isMainThread)
        let server = AgentServer.shared
        let documentID = server.documentID(for: document)
        var report = ActionRunReport(action: action.name)
        report.canvasMismatch = mismatchNote(action, document)
        let values: [String: Any]
        switch ActionParameters.bind(parameters, to: action) {
        case .success(let bound): values = bound
        case .failure(let error):
            report.ok = false
            report.refusal = error.message
            return report
        }
        // The RUNNING recorded space: where a non-absolute step's arithmetic
        // measures from. It starts at the size the recording was made on and
        // advances past every canvas-changing step that SUCCEEDS
        // (`ActionGeometry.canvasAfter`) — so an action whose first step is a
        // crop remaps everything after it against the CROPPED size, which is
        // the space those coordinates were actually recorded in. A failed
        // step advances nothing: the canvas it would have produced does not
        // exist.
        var recorded = action.recordedCanvas

        // A replay is never itself recorded — an action that recorded its own
        // playback would double every step the next time it ran.
        let wasSuspended = ActionRecorder.shared.isSuspended
        ActionRecorder.shared.isSuspended = true
        defer { ActionRecorder.shared.isSuspended = wasSuspended }

        // …and nothing else may edit while it does — see `isPlaying`.
        let wasPlaying = isPlaying
        isPlaying = true
        defer { isPlaying = wasPlaying }

        // The same activity assertion both agent trampolines hold: a run that
        // continues while the app is in the background is user-initiated
        // work, and App Nap would otherwise demote it for the rest of the
        // session (AppActivity.swift).
        AppActivity.userInitiated("playing the \(action.name) action") {
            progress?.begin(action: action.name, steps: action.steps.count)
            defer { progress?.end() }
            // One loop for both granularities; only the undo bracketing
            // differs (see `ActionRunHistory`).
            func playSteps() {
                for (index, step) in action.steps.enumerated() {
                    // The halt is tested FIRST, so nothing after a stop is
                    // counted — not even as skipped, which would read as "we
                    // considered it and passed" rather than "we never got
                    // there".
                    if report.stoppedAt != nil { break }
                    guard step.enabled else {
                        // A disabled step is never a failure; it is simply
                        // not run.
                        report.stepsSkipped += 1
                        continue
                    }
                    // Between steps, never inside one: this both draws the
                    // step counter and services the Stop button, so the app
                    // stays answerable through a long run.
                    guard progress?.tick(step: index + 1) ?? true else {
                        report.cancelled = true
                        report.stoppedAt = index + 1
                        break
                    }
                    let context = Context(action: action, values: values, recorded: recorded)
                    let outcome: StepOutcome
                    switch history {
                    case .oneEntry:
                        outcome = runStep(
                            step, on: document, id: documentID, server: server,
                            context: context)
                    case .perStep:
                        outcome = document.withUndoGroupDiscardingEmpty {
                            runStep(
                                step, on: document, id: documentID, server: server,
                                context: context)
                        }
                    }
                    guard let reason = outcome.failure else {
                        report.stepsRun += 1
                        if let next = outcome.canvas { recorded = next }
                        continue
                    }
                    report.failures.append((step: index + 1, tool: step.tool, reason: reason))
                    // An interactive Play asks. The alert is modal, so
                    // `isPlaying` and the suspended recorder stay held while
                    // it is up and an agent call in its loop is refused.
                    if stopOnError == nil, let prompt = prompt, step.onError == .stop {
                        let failure = ActionStepFailure(
                            step: index + 1, steps: action.steps.count, tool: step.tool,
                            title: step.isUnrecorded
                                ? "Not recorded — \(step.unrecordedName)"
                                : ActionCatalogFacts.displayName(
                                    for: step.tool, arguments: step.arguments),
                            reason: reason, kind: outcome.kind, object: outcome.object,
                            cause: outcome.cause)
                        report.prompted += 1
                        let choice = prompt(failure)
                        if choice == .stop || !failure.kind.offersContinue {
                            report.stoppedAt = index + 1
                        }
                    } else if stopOnError ?? (step.onError == .stop) {
                        // The run-wide override wins over the step's own
                        // policy — that is what Batch's "Stop on error"
                        // checkbox means.
                        report.stoppedAt = index + 1
                    }
                }
            }
            switch history {
            case .oneEntry: document.withReplayUndo(action.name) { playSteps() }
            case .perStep: playSteps()
            }
        }
        report.ok = report.failures.isEmpty && !report.cancelled
        return report
    }

    /// Runs ONE step as its own undo entry, under a caller-chosen name —
    /// Filters ▸ Repeat Last (⌃F).
    ///
    /// The rule this call sits on, stated once: **replay inside an Action is
    /// silent (batch semantics); ⌃F is a menu command and ASKS.** The
    /// rasterize prompt therefore happens in the caller
    /// (`EditorViewController.repeatLastFilter`) before this is reached,
    /// exactly as the Filters menu's own path does it — a step run through
    /// the agent dispatch would otherwise rasterize a text layer silently,
    /// which no menu command in the app does.
    static func runSingle(
        _ step: ActionStep, on document: ImageDocument, undoName: String
    ) -> ActionRunReport {
        assert(Thread.isMainThread)
        let server = AgentServer.shared
        let documentID = server.documentID(for: document)
        var report = ActionRunReport(action: undoName)
        let wasSuspended = ActionRecorder.shared.isSuspended
        ActionRecorder.shared.isSuspended = true
        defer { ActionRecorder.shared.isSuspended = wasSuspended }
        let wasPlaying = isPlaying
        isPlaying = true
        defer { isPlaying = wasPlaying }
        AppActivity.userInitiated(undoName) {
            // The same rule as a whole run's (see `run`): one undo entry, and
            // none at all when the repeat changed nothing — a ⌃F that was
            // refused must not leave an "Undo Repeat …" that restores nothing.
            document.withReplayUndo(undoName) {
                // ⌃F repeats ONE recorded step as it was recorded: it
                // declares no parameters and carries no recorded space, so
                // the context is the empty one and the step is absolute by
                // construction (`ActionSteps.filter` never sets a mode).
                let outcome = runStep(
                    step, on: document, id: documentID, server: server,
                    context: Context(action: Action(name: undoName, steps: [step]),
                                     values: [:], recorded: nil))
                if let reason = outcome.failure {
                    report.failures.append((step: 1, tool: step.tool, reason: reason))
                    report.stoppedAt = 1
                } else {
                    report.stepsRun = 1
                }
            }
        }
        report.ok = report.failures.isEmpty
        return report
    }

    /// The mark a PLAY leaves in a recording that is running — the one line
    /// every spelling of "play this action" calls, so the three cannot
    /// drift.
    ///
    /// A play is a user-visible command with no usable twin: `run_action`
    /// may not be a step (an action could then name itself and the player
    /// would recurse), so there is nothing to record that would replay.
    /// Recording NOTHING would be the silent hole this feature's recording
    /// rule forbids — the action would look complete and reproduce none of
    /// what the play did — so the gap lands as the same named placeholder
    /// every other twin-less command uses, listed in the Actions window and
    /// refused with its reason at replay.
    ///
    /// It is called after the run and never inside it: the player suspends
    /// the recorder for the length of a replay, so a record made in there
    /// would be dropped.
    static func notePlayed(
        _ action: Action, _ report: ActionRunReport, on document: ImageDocument
    ) {
        guard report.stepsRun > 0 else { return }
        ActionRecorder.shared.record(.unrecorded("Play “\(action.name)”"), on: document)
    }

    /// The document a UI-initiated play targets: the main window's, and
    /// otherwise the front-most document window's.
    ///
    /// The fallback is not a nicety, it is the normal case:
    /// `NSDocumentController.currentDocument` is nil whenever the main window
    /// belongs to no document — which is exactly the state the app is in
    /// while the Actions window is up and its Play button, or File ▸ Automate
    /// ▸ Play, is being used. One implementation so the menu item's
    /// validation, the menu item's action and the window's own button cannot
    /// disagree about whether there is anything to play on.
    static func frontDocument() -> ImageDocument? {
        if let current = NSDocumentController.shared.currentDocument as? ImageDocument {
            return current
        }
        return NSApp.orderedDocuments.compactMap { $0 as? ImageDocument }.first
    }

    // MARK: - One step

    /// Everything a step's arguments are resolved against that is not the
    /// document: which action it belongs to (for `$asset`'s folder), the
    /// bound parameter values, and the running recorded space.
    private struct Context {
        let action: Action
        let values: [String: Any]
        let recorded: (width: Int, height: Int)?
    }

    /// What one step did: why it failed, or — on success — the canvas the
    /// running recorded space becomes. `kind`, `object` and `cause` are what
    /// the interactive prompt calls a failure.
    private struct StepOutcome {
        var failure: String?
        var canvas: (width: Int, height: Int)?
        var kind: ActionStepFailureKind = .unavailable
        var object: String?
        var cause: String?
    }

    /// Runs one step.
    ///
    /// **The order the arguments are resolved in is fixed and it matters:**
    ///
    /// 1. `$param` — so a parameter may SUPPLY a coordinate, which the remap
    ///    then treats exactly like a recorded one.
    /// 2. the geometry remap — which reads numbers, so it has to run while
    ///    they are still numbers. (A `paste` step's payload size is read
    ///    from its `payload_path` first, without rewriting the argument.)
    /// 3. every other symbol — `$layer`, `$channel`, `$layers`, `$guide`,
    ///    `$asset` — which turn into indices and paths the remap has no
    ///    business rewriting.
    /// 4. `document_id`, rebound last to whatever document this run targets.
    private static func runStep(
        _ step: ActionStep, on document: ImageDocument, id: Int, server: AgentServer,
        context: Context
    ) -> StepOutcome {
        guard !step.isUnrecorded else {
            return StepOutcome(
                failure: "\"\(step.unrecordedName)\" was not recorded — it has no MCP tool, so "
                    + "it cannot be replayed. Remove or replace this step in the Actions "
                    + "window.")
        }
        // Belt to `Action.decode`'s braces: an action that names an Actions
        // tool — or `save_copy`, whose absolute path would escape a batch's
        // output folder — is refused when it is read, so this is unreachable
        // through any path that exists. But `run_action` here would re-enter
        // the player through `AgentServer.execute`, and an action that names
        // itself would recurse until the stack overflowed and the app was
        // killed.
        // A step built in code (where the decode refusal cannot reach, and a
        // release build drops `ActionStep.init`'s assert) fails here instead.
        if let reason = Action.notAStepReason(step.tool) {
            return StepOutcome(
                failure: "\"\(step.tool)\" cannot run inside an action — \(reason)")
        }
        // 1. Parameters, before anything reads a number.
        var arguments: [String: Any]
        switch substituted(step.arguments, context.values) {
        case .success(let bound): arguments = bound
        case .failure(let error): return StepOutcome(failure: error.message)
        }
        // The canvas this step would leave behind, measured on the arguments
        // the ACTION asked for — after `$param`, before the remap — because
        // that is what the recorded space is expressed in.
        var next: (width: Int, height: Int)?
        if let recorded = context.recorded {
            next = ActionGeometry.canvasAfter(
                tool: step.tool, arguments: arguments, from: recorded)
        }
        // 2. The geometry remap.
        if step.geometry != .absolute {
            guard let recorded = context.recorded, let doc = document.doc else {
                return StepOutcome(
                    failure: "this step's \(step.geometry.displayName) geometry needs both the "
                        + "recorded canvas and the document's own size")
            }
            // A saved-payload paste remaps by its payload's
            // size, read from the step's `.drcb` (a literal path or its
            // `$asset`) — the one argument resolved before the remap, and
            // only to be read. Only a relative `at` paste uses it, so anchor
            // and the `unsupported` refusal never read the payload.
            var extent: (width: Int, height: Int)?
            if step.tool == "paste", step.geometry == .relative,
               arguments["placement"] as? String == "at",
               let raw = arguments["payload_path"] {
                switch resolve(raw, in: document, context: context) {
                case .success(let path as String):
                    do {
                        extent = try TypedClipboardPayload.read(
                            file: URL(fileURLWithPath: path)).size
                    } catch {
                        return StepOutcome(
                            failure: (error as? DoctoredCoreError)?.message
                                ?? error.localizedDescription)
                    }
                case .success:
                    return StepOutcome(failure: "this paste step's payload_path is not a path")
                case .failure(let error):
                    return StepOutcome(failure: error.message)
                }
            }
            switch ActionGeometry.remap(
                arguments, tool: step.tool, mode: step.geometry, recorded: recorded,
                live: (width: doc.width, height: doc.height), extent: extent)
            {
            case .success(let remapped): arguments = remapped
            case .failure(let error): return StepOutcome(failure: error.message)
            }
        }
        // 3. Every other symbol.
        switch resolve(arguments, in: document, context: context) {
        case .success(let resolved): arguments = resolved
        case .failure(let error):
            return StepOutcome(failure: error.message, kind: error.kind, object: error.object)
        }
        // 4. Rebound, never recorded: ids are session-scoped integers, so the
        // run's own target is bound here.
        arguments["document_id"] = id
        // `isValidJSONObject` first, for the reason `AgentServer.saveAction`
        // states: `data(withJSONObject:)` RAISES an ObjC exception on a value
        // it cannot write — a non-finite number, most of all — and a raise on
        // this path terminates the process where a `try?` looks like it is
        // handling the failure. `Action.decode` refuses such a value now, so
        // this is the second lock on the same door.
        guard JSONSerialization.isValidJSONObject(arguments),
              let data = try? JSONSerialization.data(withJSONObject: arguments)
        else {
            return StepOutcome(failure: "this step's arguments could not be encoded as JSON")
        }
        server.lastToolErrorKind = nil
        let result = executeStep(
            step.tool, String(decoding: data, as: UTF8.self), server: server)
        guard let failure = failureMessage(in: result) else {
            return StepOutcome(failure: nil, canvas: next)
        }
        // The tool's typed kind, never its sentence's shape.
        if case .commandFailed(let cause)? = server.lastToolErrorKind {
            return StepOutcome(failure: failure, kind: .commandFailed, cause: cause)
        }
        return StepOutcome(failure: failure)
    }

    /// The player's own dispatch, marked as such for the length of the call
    /// so the exclusions `isPlaying` and `BatchRun.active` describe let
    /// exactly this one through.
    private static func executeStep(
        _ tool: String, _ argumentsJSON: String, server: AgentServer
    ) -> String {
        dispatchingOwnStep { server.execute(tool: tool, argumentsJSON: argumentsJSON) }
    }

    // MARK: - Parameters

    /// `value` with every `{"$param": …}` replaced by its bound value.
    ///
    /// Run FIRST and as its own walk rather than as another case in
    /// `resolve`, because a parameter may supply a COORDINATE and the geometry
    /// remap — which runs between the two — has to see a number there.
    private static func substituted(
        _ value: Any, _ values: [String: Any]
    ) -> Result<Any, ActionError> {
        if ActionSymbol.looksLikeSymbol(value) {
            guard case .success(.parameter(let name)) = ActionSymbol.parse(value) else {
                // Any other symbol — or a malformed one — is left exactly as
                // it is; `resolve` owns both.
                return .success(value)
            }
            guard let bound = values[name] else {
                // Unreachable through any path that exists: `Action.decode`
                // refuses an undeclared name and `ActionParameters.bind`
                // fills every declared one. Named anyway, because a step
                // built in code reaches here.
                return .failure(ActionError("no value was supplied for parameter \"\(name)\""))
            }
            return .success(bound)
        }
        if let object = value as? [String: Any] {
            var out: [String: Any] = [:]
            for key in object.keys.sorted() {
                switch substituted(object[key] as Any, values) {
                case .success(let resolved): out[key] = resolved
                case .failure(let error): return .failure(error)
                }
            }
            return .success(out)
        }
        if let array = value as? [Any] {
            var out: [Any] = []
            for element in array {
                switch substituted(element, values) {
                case .success(let resolved): out.append(resolved)
                case .failure(let error): return .failure(error)
                }
            }
            return .success(out)
        }
        return .success(value)
    }

    private static func substituted(
        _ arguments: [String: Any], _ values: [String: Any]
    ) -> Result<[String: Any], ActionError> {
        // No short-circuit on an empty `values`: a `{"$param": …}` reached
        // with nothing bound has to REFUSE, not pass through as a literal
        // dictionary the tool would misread.
        switch substituted(arguments as Any, values) {
        case .success(let resolved):
            guard let object = resolved as? [String: Any] else {
                return .failure(
                    ActionError("this step's arguments are not an object of named arguments"))
            }
            return .success(object)
        case .failure(let error):
            return .failure(error)
        }
    }

    /// The message out of a CallToolResult that reported `isError`, or nil
    /// when the call succeeded. The tools' own in-band errors are better
    /// than anything the player could write — "A polygon selection needs at
    /// least 3 points" names the fix — so they are lifted verbatim.
    private static func failureMessage(in result: String) -> String? {
        guard let parsed = try? JSONSerialization.jsonObject(with: Data(result.utf8)),
              let object = parsed as? [String: Any]
        else { return "the tool returned a result that could not be read" }
        guard (object["isError"] as? Bool) == true else { return nil }
        let content = object["content"] as? [[String: Any]] ?? []
        let text = content.compactMap { $0["text"] as? String }.joined(separator: " ")
        return text.isEmpty ? "the tool failed" : text
    }

    // MARK: - Symbols

    /// Replaces every symbol in `arguments` with what it names on THIS
    /// document. Recursive, so a symbol inside a `layers` array resolves too.
    private static func resolve(
        _ value: Any, in document: ImageDocument, context: Context
    ) -> Result<Any, ActionError> {
        if ActionSymbol.looksLikeSymbol(value) {
            switch ActionSymbol.parse(value) {
            case .failure(let error): return .failure(error)
            case .success(let symbol): return resolve(symbol, in: document, context: context)
            }
        }
        if let object = value as? [String: Any] {
            var out: [String: Any] = [:]
            for key in object.keys.sorted() {
                switch resolve(object[key] as Any, in: document, context: context) {
                case .success(let resolved): out[key] = resolved
                case .failure(let error): return .failure(error)
                }
            }
            return .success(out)
        }
        if let array = value as? [Any] {
            var out: [Any] = []
            for element in array {
                switch resolve(element, in: document, context: context) {
                case .success(let resolved): out.append(resolved)
                case .failure(let error): return .failure(error)
                }
            }
            return .success(out)
        }
        return .success(value)
    }

    private static func resolve(
        _ arguments: [String: Any], in document: ImageDocument, context: Context
    ) -> Result<[String: Any], ActionError> {
        switch resolve(arguments as Any, in: document, context: context) {
        case .success(let resolved):
            // Never `?? [:]`: the one value that resolves to something other
            // than a dictionary is a step whose WHOLE arguments object was a
            // symbol, and running that step with no arguments would silently
            // aim it at the wrong layer and report success. `Action.decode`
            // refuses the shape now, so this is the lock on the same door
            // for a step built in code.
            guard let object = resolved as? [String: Any] else {
                return .failure(
                    ActionError(
                        "this step's arguments are a symbol, not an object of named "
                            + "arguments — a symbol is one argument's VALUE, as in "
                            + "{\"layer\": {\"$layer\": \"Sky\"}}"))
            }
            return .success(object)
        case .failure(let error):
            return .failure(error)
        }
    }

    private static func resolve(
        _ symbol: ActionSymbol, in document: ImageDocument, context: Context
    ) -> Result<Any, ActionError> {
        switch symbol {
        case .activeLayer:
            return .success(document.activeLayerIndex)
        case .layerNamed(let name):
            return layerNamed(name, in: document)
        case .layerRef(let ref, let session):
            // Layer ids are PERSISTED since 12E-1, so an id can mean
            // something across sessions too — but only where it is
            // CORROBORATED. Three steps, in order:
            //
            //  (a) SAME session: the id alone, exactly as 12A shipped it.
            //      Within one launch an id is unique across every open
            //      document, so nothing can be confused with anything.
            //  (b) ANOTHER session: the id, but only when the entry carrying
            //      it ALSO carries the recorded name. Ids are unique per
            //      DOCUMENT, not per file set — a batch's second file may
            //      legitimately hold the same id on a different layer — so an
            //      uncorroborated id would mis-target, which is worse than
            //      falling through to the name.
            //  (c) Otherwise the NAME, which is what 12A always did.
            if let doc = document.doc, let index = doc.layerIndex(id: ref.id),
               session == ActionIdentity.session || doc.layerInfo(index)?.name == ref.name
            {
                return .success(index)
            }
            return layerNamed(ref.name, in: document)
        case .channelRef(let ref, let session):
            guard let doc = document.doc else {
                return .failure(ActionError("the document has no image"))
            }
            if session == ActionIdentity.session, ref.id != 0,
               let index = (0..<doc.channelCount).first(where: {
                   doc.channelInfo($0)?.id == ref.id
               })
            {
                return .success(index)
            }
            // FIRST by name, not last: the channels panel lists them bottom
            // first and a document's channel names are unique in practice,
            // so the first match is the one a reader points at.
            for index in 0..<doc.channelCount where doc.channelInfo(index)?.name == ref.name {
                return .success(index)
            }
            return .failure(
                ActionError(
                    "no channel named \"\(ref.name)\"", kind: .missingObject,
                    object: "channel “\(ref.name)”"))
        case .parameter(let name):
            // `substituted` runs first and replaces every one of these, so a
            // `$param` reaching here is a step built in code with no values.
            guard let bound = context.values[name] else {
                return .failure(ActionError("no value was supplied for parameter \"\(name)\""))
            }
            return .success(bound)
        case .asset(let file):
            guard let folder = ActionLibrary.assetsDirectory(
                    forActionNamed: context.action.name)
            else {
                return .failure(
                    ActionError(
                        "\"\(context.action.name)\" is not in the action library, so its asset "
                            + "\"\(file)\" has nowhere to come from — import the action's "
                            + "bundle first"))
            }
            let url = folder.appendingPathComponent(file)
            guard FileManager.default.fileExists(atPath: url.path) else {
                // The FULL path, because that is the thing to go and look at.
                return .failure(ActionError("this action's asset \(url.path) is not there"))
            }
            return .success(url.path)
        case .selectedLayers:
            let indices = Array(Set(document.selectedLayerIndices)).sorted()
            guard !indices.isEmpty else {
                return .failure(
                    ActionError(
                        "this step needs a layer selection; none is set", kind: .missingObject))
            }
            return .success(indices)
        case .guideAt(let orientation, let position):
            guard let doc = document.doc else {
                return .failure(ActionError("the document has no image"))
            }
            let guides = doc.guides
            // Within half a pixel: the position was written by the same
            // build that reads it, so an exact hit is the normal case and
            // the tolerance only absorbs the JSON round trip.
            for (index, guide) in guides.enumerated()
            where (guide.orientation == .horizontal ? "horizontal" : "vertical") == orientation
                && abs(guide.position - position) <= 0.5
            {
                return .success(index)
            }
            return .failure(
                ActionError(
                    "no \(orientation) guide at \(ActionArgs.number(position)) "
                        + "on this document", kind: .missingObject,
                    object: "guide “\(orientation) \(ActionArgs.number(position))”"))
        }
    }

    /// The TOPMOST layer with that exact name — the layers panel lists top
    /// first, so "the layer called Sky" means the one a reader would point
    /// at. Shared by the string form of `$layer` and by the object form's
    /// fallback, so the two can never disagree about which "Sky" is meant.
    private static func layerNamed(
        _ name: String, in document: ImageDocument
    ) -> Result<Any, ActionError> {
        guard let doc = document.doc else {
            return .failure(ActionError("the document has no image"))
        }
        for index in stride(from: doc.layerCount - 1, through: 0, by: -1)
        where doc.layerInfo(index)?.name == name {
            return .success(index)
        }
        return .failure(
            ActionError(
                "no layer named \"\(name)\"", kind: .missingObject, object: "layer “\(name)”"))
    }

    /// The one-line warning a size difference earns. Absolute coordinates
    /// are NOT remapped (see `ActionSymbol`), so saying so is the honest
    /// alternative to guessing.
    private static func mismatchNote(_ action: Action, _ document: ImageDocument) -> String? {
        guard let recorded = action.recordedCanvas, let doc = document.doc else { return nil }
        guard recorded.width != doc.width || recorded.height != doc.height else { return nil }
        return "recorded at \(recorded.width) × \(recorded.height), played on "
            + "\(doc.width) × \(doc.height) — absolute coordinates were not remapped"
    }
}

/// What a run did, in the ONE shape the Actions window, `run_action` and
/// Batch's report all read.
struct ActionRunReport {
    var ok: Bool = true
    let action: String
    var stepsRun: Int = 0
    var stepsSkipped: Int = 0
    /// The 1-based step a `stop` policy halted at, if any.
    var stoppedAt: Int?
    /// True when the user pressed Stop. The steps already applied are kept —
    /// as the run's one undo entry under `.oneEntry`, as one History state
    /// each under `.perStep` — and `stoppedAt` names the step that never ran.
    var cancelled: Bool = false
    var canvasMismatch: String?
    /// An ACTION-level refusal: the run was not started at all. Today the one
    /// producer is a parameter that would not bind, which is deliberately not
    /// a per-step failure — nothing ran, so there is no step to name.
    var refusal: String?
    var failures: [(step: Int, tool: String, reason: String)] = []
    /// Failures the user answered through the interactive prompt.
    /// Not in `jsonObject`: `run_action` and Batch never ask.
    var prompted: Int = 0
    /// True when the interactive caller should beep: the run failed and the
    /// user has not already answered every failure through the prompt.
    var needsAttention: Bool {
        !ok && (cancelled || refusal != nil || prompted < failures.count)
    }

    init(action: String) {
        self.action = action
    }

    func jsonObject() -> [String: Any] {
        var object: [String: Any] = [
            "ok": ok, "action": action, "steps_run": stepsRun, "steps_skipped": stepsSkipped,
        ]
        if let stoppedAt = stoppedAt { object["stopped_at"] = stoppedAt }
        if cancelled { object["cancelled"] = true }
        if let refusal = refusal { object["refusal"] = refusal }
        if let mismatch = canvasMismatch { object["canvas_mismatch"] = mismatch }
        if !failures.isEmpty {
            object["failures"] = failures.map {
                ["step": $0.step, "tool": $0.tool, "reason": $0.reason]
            }
        }
        return object
    }

    /// The same report as one line of prose, for the Actions window's status
    /// line and Batch's per-file rows.
    func text() -> String {
        // An action-level refusal ran nothing, so a step tally beside it
        // would read as though something had been tried.
        if let refusal = refusal { return "\(action): \(refusal)" }
        var parts = ["\(stepsRun) step\(stepsRun == 1 ? "" : "s")"]
        if stepsSkipped > 0 { parts.append("\(stepsSkipped) skipped") }
        if cancelled {
            parts.append("stopped by you\(stoppedAt.map { " at step \($0)" } ?? "")")
        } else if let stoppedAt = stoppedAt {
            parts.append("stopped at step \(stoppedAt)")
        }
        var line = (ok ? "Ran \(action): " : "\(action): ") + parts.joined(separator: ", ")
        if let failure = failures.first {
            line += " — step \(failure.step) (\(failure.tool)): \(failure.reason)"
            if failures.count > 1 { line += " (+\(failures.count - 1) more)" }
        }
        if let mismatch = canvasMismatch { line += " · \(mismatch)" }
        return line
    }
}
