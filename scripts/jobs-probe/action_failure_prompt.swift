import AppKit

/// An interactive Play asks Stop / Continue on a failed step whose
/// `on_error` is `stop` when its command is unavailable or names a missing
/// object, and stops with an OK-only alert on an error inside a command;
/// Batch, `run_action` (no prompt) and a `continue` step never ask. Three
/// actions: `[new_layer, paste (empty pasteboard), new_layer]`, `[new_layer,
/// set_active_layer {"$layer": "AF_missing"}, new_layer]` and `[new_layer,
/// move_selected_pixels {"dx": 2, "dy": 0} (an empty area), new_layer]`.
func verifyActionFailurePrompt() throws {
    Clipboard.pasteboard.clearContents()
    let emptyArea = "the selection holds no pixels"
    let cases: [(name: String, failing: ActionStep, kind: ActionStepFailureKind,
                 object: String?, cause: String?)] = [
        ("paste", ActionStep(tool: "paste"), .unavailable, nil, nil),
        ("select", ActionStep(tool: "set_active_layer", arguments: ["index": ["$layer": "AF_missing"]]),
         .missingObject, "layer “AF_missing”", nil),
        ("move", ActionStep(tool: "move_selected_pixels", arguments: ["dx": 2, "dy": 0]),
         .commandFailed, nil, emptyArea),
    ]
    // (a)–(e) assume Continue plays on, so they exclude the command failure.
    for entry in cases where entry.kind != .commandFailed {
        func action(_ onError: ActionStep.OnError) -> Action {
            var failing = entry.failing
            failing.onError = onError
            return Action(name: "AF \(entry.name)",
                          steps: [ActionStep(tool: "new_layer"), failing, ActionStep(tool: "new_layer")])
        }
        let title = ActionCatalogFacts.displayName(for: entry.failing.tool,
                                                   arguments: entry.failing.arguments)

        // (a) Continue: the run goes on past step 2, as one undo step.
        var calls: [ActionStepFailure] = []
        try playAF(action(.stop), prompt: { calls.append($0); return .continueRun }) { report, document, base, layers in
            try require(report.stepsRun == 2 && report.failures.count == 1 && report.failures[0].step == 2
                            && report.stoppedAt == nil,
                        "[\(entry.name) a] continue: \(report.jsonObject())")
            try require(calls.count == 1, "[\(entry.name) a] the prompt was called \(calls.count) times")
            let call = calls[0]
            try require(call.step == 2 && call.steps == 3 && call.tool == entry.failing.tool
                            && call.title == title && call.reason == report.failures[0].reason
                            && call.kind == entry.kind && call.object == entry.object,
                        "[\(entry.name) a] the prompt saw \(call)")
            try require(report.prompted == 1 && !report.needsAttention,
                        "[\(entry.name) a] prompted \(report.prompted), needsAttention \(report.needsAttention)")
            try require(document.doc?.layerCount == layers + 2, "[\(entry.name) a] two layers were added")
            try requireOneUndo(document, base, "[\(entry.name) a]")
        }

        // (b) Stop: the run halts at step 2, keeping step 1 in the one undo step.
        calls = []
        try playAF(action(.stop), prompt: { calls.append($0); return .stop }) { report, document, base, layers in
            try require(report.stepsRun == 1 && report.stoppedAt == 2 && calls.count == 1,
                        "[\(entry.name) b] stop: \(report.jsonObject()), \(calls.count) prompts")
            try require(document.doc?.layerCount == layers + 1, "[\(entry.name) b] no third layer")
            try require(report.prompted == 1 && !report.needsAttention,
                        "[\(entry.name) b] needsAttention \(report.needsAttention)")
            try requireOneUndo(document, base, "[\(entry.name) b]")
        }

        // (c) A `continue` step never asks.
        calls = []
        try playAF(action(.continueRun), prompt: { calls.append($0); return .stop }) { report, _, _, _ in
            try require(calls.isEmpty && report.stepsRun == 2 && report.stoppedAt == nil
                            && report.needsAttention,
                        "[\(entry.name) c] continue step: \(report.jsonObject()), \(calls.count) prompts")
        }

        // (d) Batch's override never asks, either way.
        for stop in [true, false] {
            calls = []
            try playAF(action(.stop), stopOnError: stop, prompt: { calls.append($0); return .continueRun }) {
                report, _, _, _ in
                let expected = stop ? (run: 1, at: Optional(2)) : (run: 2, at: nil)
                try require(calls.isEmpty && report.stepsRun == expected.run && report.stoppedAt == expected.at
                                && report.needsAttention,
                            "[\(entry.name) d \(stop)] stopOnError: \(report.jsonObject()), \(calls.count) prompts")
            }
        }

        // (e) No prompt (run_action, Batch): today's rule.
        try playAF(action(.stop), prompt: nil) { report, _, _, _ in
            try require(report.stepsRun == 1 && report.stoppedAt == 2 && report.prompted == 0
                            && report.needsAttention,
                        "[\(entry.name) e] no prompt: \(report.jsonObject())")
        }
    }

    // (a′) A command failure stops the run whatever the answer; (c′) a
    // `continue` step never asks.
    let move = cases[2]
    func moveAction(_ onError: ActionStep.OnError) -> Action {
        var failing = move.failing
        failing.onError = onError
        return Action(name: "AF move",
                      steps: [ActionStep(tool: "new_layer"), failing, ActionStep(tool: "new_layer")])
    }
    let selectSquare: (ImageDocument) throws -> Void = { _ = try select($0, side: 16) }
    var moveCalls: [ActionStepFailure] = []
    try playAF(moveAction(.stop), setup: selectSquare,
               prompt: { moveCalls.append($0); return .continueRun }) { report, document, base, layers in
        try require(moveCalls.count == 1, "[move a′] the prompt was called \(moveCalls.count) times")
        let call = moveCalls[0]
        try require(call.kind == .commandFailed && call.cause == emptyArea && call.object == nil
                        && call.step == 2 && call.reason == report.failures.first?.reason,
                    "[move a′] the prompt saw \(call)")
        try require(report.stoppedAt == 2 && report.stepsRun == 1 && report.failures.count == 1,
                    "[move a′] \(report.jsonObject())")
        try require(document.doc?.layerCount == layers + 1, "[move a′] no third layer")
        try require(report.prompted == 1 && !report.needsAttention,
                    "[move a′] prompted \(report.prompted), needsAttention \(report.needsAttention)")
        try requireOneUndo(document, base, "[move a′]")
    }
    moveCalls = []
    try playAF(moveAction(.continueRun), setup: selectSquare,
               prompt: { moveCalls.append($0); return .stop }) { report, _, _, _ in
        try require(moveCalls.isEmpty && report.stepsRun == 2 && report.stoppedAt == nil,
                    "[move c′] continue step: \(report.jsonObject()), \(moveCalls.count) prompts")
    }

    // (f) The alert itself, answered by AutomationAlerts with its first button.
    try require(AutomationAlerts.isInstalled, "(f) AutomationAlerts is not installed")
    let sentences: [ActionStepFailureKind: String] = [
        .unavailable: "The step “Probe title” could not run.",
        .missingObject: "The step names layer “AF_missing”, which this document does not have.",
        .commandFailed: "Probe title stopped: the selection holds no pixels.",
    ]
    for entry in cases {
        let failure = ActionStepFailure(
            step: 2, steps: 3, tool: entry.failing.tool, title: "Probe title", reason: "probe reason",
            kind: entry.kind, object: entry.object, cause: entry.cause)
        let message = ActionFailurePrompt.message(for: failure)
        try require(message == sentences[entry.kind], "(f) \(entry.name): the sentence is \(message)")
        let titles = ActionFailurePrompt.buttonTitles(for: entry.kind)
        try require(titles == (entry.kind.offersContinue ? ["Continue", "Stop"] : ["OK"]),
                    "(f) \(entry.name): buttons \(titles)")
        let before = AutomationAlerts.recorded.count
        let choice = ActionFailurePrompt.ask(failure)
        let records = AutomationAlerts.recorded.dropFirst(before)
        try require(records.count == 1, "(f) \(entry.name): \(records.count) alerts recorded")
        guard let record = records.first else { continue }
        try require(record.kind == .alertModal && record.messageText == message
                        && record.informativeText == failure.reason && record.buttonTitles == titles,
                    "(f) \(entry.name): \(record)")
        let first: ActionFailureChoice = entry.kind.offersContinue ? .continueRun : .stop
        try require(choice == first, "(f) \(entry.name): the first button mapped to \(choice)")
    }

    // (g) The typed kind is set at the throw sites, never read off a sentence.
    let kinds: [(String, AgentServer.ToolErrorKind?)] = [
        ("The selection holds no pixels to move.", .commandFailed(cause: emptyArea)),
        ("The selection holds no pixels to transform.", .commandFailed(cause: emptyArea)),
        // A sentence since removed from the core (the transform has no
        // pixel-work refusal): no longer a command failure.
        ("Could not transform the selected pixels because the transform exceeds the bounded "
            + "pixel-work limit.", nil),
        ("There is no selection to modify. Make one with the select_* tools.", nil),
        ("Could not frobnicate the layer because it is late.", nil),
    ]
    for (message, kind) in kinds {
        let error = AgentServer.ToolError.selectedPixels(message)
        try require(error.message == message && error.kind == kind,
                    "(g) \(message) gave \(String(describing: error.kind))")
    }
    try require(AgentServer.ToolError(message: "plain").kind == nil, "(g) a plain ToolError has a kind")

    // (h) An interactive Play (`.perStep`) is one History state per
    // step that changed something, each under its own tool's undo name; a
    // failed step adds none. `[new_layer, new_layer, paste (fails), new_layer]`.
    let directName: String = try {
        let probe = try fixture(width: 64, height: 48)
        defer { probe.close() }
        let before = probe.history.entries.count
        let encoded = try JSONSerialization.data(
            withJSONObject: ["document_id": AgentServer.shared.documentID(for: probe)])
        let raw = AgentServer.shared.execute(
            tool: "new_layer", argumentsJSON: String(decoding: encoded, as: UTF8.self))
        guard probe.history.entries.count == before + 1, let last = probe.history.entries.last else {
            throw ProbeFailure(description: "(h) a direct new_layer added no History state: \(raw)")
        }
        return last.name
    }()
    func perStepAction() -> Action {
        var failing = cases[0].failing
        failing.onError = .stop
        return Action(name: "AF per step",
                      steps: [ActionStep(tool: "new_layer"), ActionStep(tool: "new_layer"), failing,
                              ActionStep(tool: "new_layer")])
    }
    var entriesBefore = 0
    let countEntries: (ImageDocument) throws -> Void = { entriesBefore = $0.history.entries.count }
    try playAF(perStepAction(), setup: countEntries, prompt: { _ in .stop }, history: .perStep) {
        report, document, base, layers in
        try require(report.stepsRun == 2 && report.stoppedAt == 3 && report.prompted == 1,
                    "[h stop] \(report.jsonObject())")
        let added = Array(document.history.entries.dropFirst(entriesBefore)).map(\.name)
        try require(added.count == 2 && added.allSatisfy { $0 == directName && $0 != "AF per step" },
                    "[h stop] the run added \(added), not two \"\(directName)\" states")
        try require(document.undoManager?.canUndo == true, "[h stop] no undo step")
        document.undoManager?.undo()
        try require(document.doc?.layerCount == layers + 1, "[h stop] one undo left one new layer")
        document.undoManager?.undo()
        try require(document.doc === base && document.undoManager?.canUndo != true
                        && !document.isDocumentEdited,
                    "[h stop] two undos did not restore a clean original")
    }
    try playAF(perStepAction(), setup: countEntries, prompt: { _ in .continueRun },
               history: .perStep) { report, document, _, layers in
        try require(report.stepsRun == 3 && report.failures.count == 1 && report.stoppedAt == nil,
                    "[h continue] \(report.jsonObject())")
        let added = Array(document.history.entries.dropFirst(entriesBefore)).map(\.name)
        try require(added.count == 3 && added.allSatisfy { $0 == directName },
                    "[h continue] the run added \(added), not three \"\(directName)\" states")
        try require(document.doc?.layerCount == layers + 3, "[h continue] three layers were added")
    }
    print("PASS: action failure prompt — Continue/Stop on an unavailable command or a missing object, "
          + "an OK-only stop on a command failure (typed kind), never for continue, stop_on_error or "
          + "no prompt; one undo step; the alert's sentence and buttons per kind; an interactive "
          + "Play is one History state per step that changed something")
}

/// Plays `action` on a fresh 64 x 48 document, after `setup`, and hands the
/// report, the document, its handle before the run and its layer count to
/// `check`.
private func playAF(
    _ action: Action, stopOnError: Bool? = nil,
    setup: (ImageDocument) throws -> Void = { _ in },
    prompt: ((ActionStepFailure) -> ActionFailureChoice)?,
    history: ActionRunHistory = .oneEntry,
    _ check: (ActionRunReport, ImageDocument, AnyObject?, Int) throws -> Void
) throws {
    let document = try fixture(width: 64, height: 48)
    defer { document.close() }
    try setup(document)
    let base = document.doc
    let layers = base?.layerCount ?? 0
    let report = ActionPlayer.run(action, on: document, stopOnError: stopOnError, prompt: prompt,
                                  history: history)
    try check(report, document, base, layers)
}

private func requireOneUndo(_ document: ImageDocument, _ base: AnyObject?, _ label: String) throws {
    try require(document.undoManager?.canUndo == true, "\(label) the run left no undo step")
    document.undoManager?.undo()
    try require(document.doc === base && document.undoManager?.canUndo != true,
                "\(label) one Undo did not restore the original")
}
